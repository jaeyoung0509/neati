//! Maps trusted cleanup items to existing, verified user-app termination leases.
//! No path, executable name, or PID supplied by the interface grants authority.
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::metrics::memory::MemorySampler;
use crate::metrics::memory_termination::{
    CreateMemoryLeaseParams, MemoryLeaseMember, MemoryTerminationStore,
};
use crate::models::{CleanupQuitApp, CleanupQuitPreview, RunningProcessPolicy, ScanResult};
use crate::signatures::SignatureRegistry;
use neati_platform::PlatformEnvironment;

#[derive(Clone)]
pub(super) struct CleanupQuitDependencies {
    pub sampler: Arc<MemorySampler>,
    pub leases: Arc<Mutex<MemoryTerminationStore>>,
}

pub(super) fn matches_owner(guard: &RunningProcessPolicy, member: &MemoryLeaseMember) -> bool {
    member.exe.as_ref().is_some_and(|exe| {
        exe.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| guard.matches(name))
            || guard
                .cache_owner()
                .is_some_and(|owner| crate::applications::executable_owns_namespace(exe, owner))
    })
}

impl super::CleanupService {
    pub(crate) fn with_quit_dependencies(
        mut self,
        sampler: Arc<MemorySampler>,
        leases: Arc<Mutex<MemoryTerminationStore>>,
    ) -> Self {
        self.quit_dependencies = Some(CleanupQuitDependencies { sampler, leases });
        self
    }

    pub async fn preview_cleanup_quit(
        &self,
        scan_id: String,
        item_ids: Vec<String>,
    ) -> Result<CleanupQuitPreview, String> {
        self.platform_capabilities
            .capabilities()
            .require(
                crate::models::PlatformFeature::ProcessTermination,
                crate::models::CapabilityAccess::Mutate,
            )
            .map_err(|error| error.to_string())?;
        if self.environment.platform() != crate::models::PlatformKind::Macos {
            return Err("Quit the apps yourself, then scan again. Graceful cleanup shutdown is unavailable on this platform.".into());
        }
        let dependencies = self
            .quit_dependencies
            .clone()
            .ok_or("App shutdown is unavailable")?;
        let observation = dependencies.sampler.get_observation(Duration::ZERO).await?;
        let scan_store = self.scan_store.clone();
        let registry = self.registry.clone();
        let environment = self.environment.clone();
        crate::blocking::run_blocking(
            move || {
                let scan = scan_store
                    .get()
                    .filter(|scan| scan.scan_id == scan_id)
                    .ok_or("The scan changed. Scan again before quitting apps.")?;
                if super::cleanup_service::unix_timestamp().saturating_sub(scan.finished_at)
                    > u64::from(ScanResult::VALID_FOR_SECONDS)
                {
                    return Err("The scan expired. Scan again before quitting apps.".into());
                }
                let ids: HashSet<_> = item_ids.into_iter().collect();
                let mut apps: BTreeMap<String, (Vec<MemoryLeaseMember>, Vec<String>)> =
                    BTreeMap::new();
                let mut unavailable = Vec::new();
                for item in scan
                    .categories
                    .iter()
                    .flat_map(|category| &category.items)
                    .filter(|item| {
                        ids.contains(&item.id) && item.owner_running && item.cleanable_bytes() > 0
                    })
                {
                    let Some(guard) = owner_guard(item, &registry, &environment) else {
                        unavailable.push(item.name.clone());
                        continue;
                    };
                    let mut matched = false;
                    for (name, members) in &observation.termination_candidates {
                        if members.iter().any(|member| matches_owner(&guard, member)) {
                            let entry = apps
                                .entry(name.clone())
                                .or_insert_with(|| (members.clone(), Vec::new()));
                            entry.1.push(item.id.clone());
                            matched = true;
                        }
                    }
                    if !matched {
                        unavailable.push(item.name.clone());
                    }
                }
                if apps.len() > crate::metrics::memory_termination::STORE_CAPACITY {
                    return Err(
                        "Too many apps to review at once. Quit some apps yourself and scan again."
                            .into(),
                    );
                }
                // A poisoned disposable authority store is cleared before reuse.
                let mut store = dependencies.leases.lock().unwrap_or_else(|poisoned| {
                    let mut store = poisoned.into_inner();
                    *store = MemoryTerminationStore::default();
                    dependencies.leases.clear_poison();
                    store
                });
                let apps = apps
                    .into_iter()
                    .map(|(name, (members, item_ids))| {
                        let lease_id = store.create_lease(CreateMemoryLeaseParams {
                            group: name.clone(),
                            members,
                            can_terminate: true,
                            force_authorized: false,
                            now: Instant::now(),
                        });
                        CleanupQuitApp {
                            name,
                            lease_id,
                            item_ids,
                        }
                    })
                    .collect();
                Ok(CleanupQuitPreview {
                    scan_id,
                    apps,
                    unavailable,
                })
            },
            "Cleanup app preview worker panicked",
        )
        .await
    }
}

fn owner_guard(
    item: &crate::models::ScanItem,
    registry: &SignatureRegistry,
    environment: &PlatformEnvironment,
) -> Option<RunningProcessPolicy> {
    let signature = registry.get(&item.signature_id)?;
    signature_owner_guard(signature, std::path::Path::new(&item.path), environment)
}

fn signature_owner_guard(
    signature: &crate::models::Signature,
    path: &std::path::Path,
    environment: &PlatformEnvironment,
) -> Option<RunningProcessPolicy> {
    if signature.provider_id.as_deref() == Some("chromium.component_downloads") {
        return crate::cleaner::owner_providers::browser::cache_owner_guard(path, environment);
    }
    // An offline unit's open handle may belong to another application. Do not
    // infer that quitting the browser will release it. Other owner adapters
    // also need a verified mapping before offering application shutdown.
    if signature.strategy == crate::models::CleanStrategy::OwnerProvider {
        return None;
    }
    Some(signature.process_guard_for(path, environment))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn member(exe: Option<std::path::PathBuf>) -> MemoryLeaseMember {
        MemoryLeaseMember {
            pid: 123,
            owner: crate::process_owner::ProcessOwner::Unix(501),
            start_time: 1,
            exe,
            group: "Untrusted display label".into(),
        }
    }

    #[test]
    fn display_names_and_missing_executables_never_authorize_quitting() {
        let guard = RunningProcessPolicy::guarding(vec!["Editor".into()]);
        assert!(!matches_owner(&guard, &member(None)));
        assert!(!matches_owner(
            &guard,
            &member(Some("/Applications/Other.app/Contents/MacOS/Other".into()))
        ));
        assert!(matches_owner(
            &guard,
            &member(Some(
                "/Applications/Editor.app/Contents/MacOS/Editor".into()
            ))
        ));
        assert!(!matches_owner(
            &RunningProcessPolicy::none().with_open_files("/cache".into()),
            &member(Some(
                "/Applications/Editor.app/Contents/MacOS/Editor".into()
            ))
        ));
    }

    #[test]
    fn an_offline_open_handle_does_not_authorize_browser_shutdown() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_platform(crate::models::PlatformKind::Macos)
            .with_home("/fixture");
        let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
        let signature = registry
            .get("system.chromium.offline_cache_storage")
            .unwrap();
        assert!(signature_owner_guard(signature,
            std::path::Path::new("/fixture/Library/Application Support/Google/Chrome/Default/Service Worker/CacheStorage/origin/cache"),
            &environment).is_none());
    }

    #[test]
    fn browser_quit_mapping_does_not_include_other_browsers() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_platform(crate::models::PlatformKind::Macos)
            .with_home("/fixture");
        let chrome = crate::cleaner::owner_providers::browser::cache_owner_guard(
            std::path::Path::new(
                "/fixture/Library/Application Support/Google/Chrome/component_crx_cache",
            ),
            &environment,
        )
        .unwrap();
        assert!(chrome.matches("Google Chrome"));
        assert!(!chrome.matches("Brave Browser"));
        assert!(crate::cleaner::owner_providers::browser::cache_owner_guard(
            std::path::Path::new(
                "/fixture/Library/Application Support/Google/Chrome Canary/component_crx_cache"
            ),
            &environment
        )
        .is_none());
        assert!(crate::cleaner::owner_providers::browser::cache_owner_guard(
            std::path::Path::new(
                "/fixture/Library/Application Support/Google/Chrome-lookalike/cache"
            ),
            &environment
        )
        .is_none());
    }
}
