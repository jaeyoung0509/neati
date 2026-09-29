//! Bounded observation of namespaces excluded from generic deletion.
//! These rows never authorize cleanup: their signature IDs are not registered.
use super::WalkContext;
use crate::models::{
    CacheManagementMode, FileSize, ObservationQuality, RiskTier, ScanItem, Signature,
};
use crate::safety::SymlinkGuard;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const PREFIX: &str = "coverage:";

pub(crate) fn observe(
    signature: &Signature,
    path: &Path,
    reason: &str,
    context: &WalkContext<'_>,
) -> ScanItem {
    let started = Instant::now();
    let mut pending = vec![(path.to_path_buf(), 0usize)];
    let mut bytes = 0u64;
    let mut logical = 0u64;
    let mut files = 0usize;
    let mut visited = 0usize;
    let mut gap = SymlinkGuard::validate_anchored_path(path, context.environment)
        .err()
        .map(|error| format!("Could not inspect excluded namespace: {error}"));
    if gap.is_some() {
        pending.clear();
    }
    while let Some((current, depth)) = pending.pop() {
        if visited >= 4096
            || started.elapsed() >= Duration::from_millis(50)
            || context.cancellation.is_cancelled()
        {
            gap = Some(
                "Coverage observation reached its entry/time limit or was cancelled".to_string(),
            );
            break;
        }
        visited += 1;
        context.counters.visit_entry();
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) => {
                gap = Some(format!("Could not inspect excluded namespace: {error}"));
                continue;
            }
        };
        if SymlinkGuard::is_symlink(&current) {
            gap =
                Some("Links and mount points are not traversed during coverage observation".into());
            continue;
        }
        if metadata.is_file() {
            #[cfg(unix)]
            let allocated = {
                use std::os::unix::fs::MetadataExt;
                metadata.blocks().saturating_mul(512)
            };
            #[cfg(not(unix))]
            let allocated = super::get_allocated_size(&current).unwrap_or(metadata.len());
            bytes = bytes.saturating_add(allocated);
            logical = logical.saturating_add(metadata.len());
            files += 1;
        } else if metadata.is_dir() {
            if depth >= context.limits.max_depth.min(16) {
                gap = Some("Coverage observation reached its depth limit".into());
                continue;
            }
            context.counters.directory_read();
            match std::fs::read_dir(&current) {
                Ok(entries) => {
                    for entry in entries {
                        if pending.len() + visited >= 4096 {
                            gap = Some("Coverage observation reached its entry limit".into());
                            break;
                        }
                        match entry {
                            Ok(entry) => pending.push((entry.path(), depth + 1)),
                            Err(error) => {
                                gap = Some(format!("Could not read excluded namespace: {error}"))
                            }
                        }
                    }
                }
                Err(error) => gap = Some(format!("Could not read excluded namespace: {error}")),
            }
        }
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut item = ScanItem::mock(
        format!("{PREFIX}{}:{}", signature.id, path.display()),
        format!("{PREFIX}{}", signature.id),
        name,
        signature.category,
        RiskTier::Manual,
        path.to_string_lossy(),
        FileSize::new(logical, Some(bytes)),
        files,
    );
    item.description = reason.into();
    item.cache_metadata.management_mode = CacheManagementMode::Advisory;
    item.cache_metadata.consequence = reason.into();
    if let Some(reason) = gap {
        item.quality = ObservationQuality::Partial;
        item.incomplete_reason = Some(reason);
        item.skipped_entry_count = 1;
    }
    item.rederive_disposition();
    item
}

/// An exact dedicated row already states this namespace's measured footprint.
/// Coverage observations carry no authority and must not downgrade its owner.
pub(crate) fn route_to_owners(categories: &mut [crate::models::CategoryResult]) {
    let owners = categories
        .iter()
        .flat_map(|category| &category.items)
        .filter(|item| {
            !item.signature_id.starts_with(PREFIX)
                && item.quality == ObservationQuality::Fresh
                && item.skipped_entry_count == 0
                && item.incomplete_reason.is_none()
        })
        .map(|item| (item.unit.path.clone(), item.observed_bytes()))
        .collect::<std::collections::HashMap<_, _>>();
    for category in categories {
        category.items.retain(|item| {
            !item.signature_id.starts_with(PREFIX)
                || owners
                    .get(&item.unit.path)
                    .is_none_or(|bytes| *bytes < item.observed_bytes())
        });
        category.recompute_accounting();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NeverCancelled, PlatformKind};
    use crate::scanner::{NoRootProgress, ScanLimits, TraversalCounters};
    use crate::signatures::SignatureRegistry;
    use neati_platform::{path_algebra::PathFlavor, PlatformEnvironment};

    #[test]
    fn excluded_observation_is_measured_but_never_authorizes_cleanup() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("excluded-store");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("state.db"), vec![1; 8192]).unwrap();
        let environment = PlatformEnvironment::simulated(PathFlavor::current())
            .with_platform(PlatformKind::Macos)
            .with_home(fixture.path());
        let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
        let signature = registry.get("system.intensive.user_app_caches").unwrap();
        let counters = TraversalCounters::default();
        let context = WalkContext::new(
            &environment,
            &NeverCancelled,
            ScanLimits::default(),
            &counters,
            &NoRootProgress,
        );
        let item = observe(signature, &root, "No dedicated cleanup adapter", &context);
        assert_eq!(item.file_count, 1);
        assert_eq!(item.size.logical, 8192);
        assert_eq!(item.quality, ObservationQuality::Fresh);
        assert!(!item.allows_cleanup());
        assert!(!item.is_selected);
        assert!(
            registry.get(&item.signature_id).is_none(),
            "no registered signature can authorize this observation"
        );
        assert!(root.join("state.db").exists());

        let missing = observe(signature, &root.join("missing"), "Unavailable", &context);
        assert_eq!(missing.quality, ObservationQuality::Partial);
        assert!(missing.incomplete_reason.is_some());
        assert_eq!(missing.cleanable_bytes(), 0);
        assert_eq!(missing.skipped_entry_count, 1);
    }

    #[test]
    fn depth_exhaustion_is_partial_even_when_no_bytes_were_measured() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("store");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/data"), b"kept").unwrap();
        let environment = PlatformEnvironment::simulated(PathFlavor::current())
            .with_platform(PlatformKind::Macos)
            .with_home(fixture.path());
        let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
        let counters = TraversalCounters::default();
        let context = WalkContext::new(
            &environment,
            &NeverCancelled,
            ScanLimits::bounded(0, 1),
            &counters,
            &NoRootProgress,
        );
        let item = observe(
            registry.get("system.intensive.user_app_caches").unwrap(),
            &root,
            "Outside scope",
            &context,
        );
        assert_eq!(item.observed_bytes(), 0);
        assert_eq!(item.quality, ObservationQuality::Partial);
        assert_eq!(item.skipped_entry_count, 1);
        assert!(item.incomplete_reason.unwrap().contains("depth"));
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::cleaner::{LifecycleProviderRegistry, OwnerProviderRegistry};
    use crate::models::{Category, CleanStrategy, NeverCancelled, PlatformKind};
    use crate::scanner::ScanEngine;
    use crate::signatures::SignatureRegistry;
    use neati_platform::{path_algebra::PathFlavor, PlatformEnvironment};

    #[test]
    fn name_fixture_routes_exact_owner_once_and_retains_unknown_siblings() {
        let names: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/coverage/names.json")).unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("Library/Caches");
        for key in ["owned", "excluded_unknown", "ordinary"] {
            let directory = root.join(names[key].as_str().unwrap());
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join("payload"),
                vec![1; names["payload_bytes"].as_u64().unwrap() as usize],
            )
            .unwrap();
        }
        let environment = PlatformEnvironment::simulated(PathFlavor::current())
            .with_platform(PlatformKind::Macos)
            .with_home(fixture.path());
        let embedded = SignatureRegistry::load_embedded_with(&environment).unwrap();
        let mut broad = embedded
            .get("system.intensive.user_app_caches")
            .unwrap()
            .clone();
        // Keep the shipped namespace policy, but run it against a host-neutral
        // fixture. The macOS declaration belongs to the real Library root,
        // not to these temporary directories on the Windows CI runner.
        broad.paths = vec![root.to_string_lossy().into_owned()];
        broad.platforms.clear();
        broad.exclude_prefixes = vec![
            names["owned"].as_str().unwrap().into(),
            names["excluded_unknown"].as_str().unwrap().into(),
        ];
        let mut owner = broad.clone();
        owner.id = "fixture.dedicated".into();
        owner.name = "Dedicated store".into();
        owner.paths = vec![root
            .join(names["owned"].as_str().unwrap())
            .to_string_lossy()
            .into_owned()];
        owner.min_age_days = None;
        owner.exclude_prefixes.clear();
        owner.strategy = CleanStrategy::Manual;
        owner.risk = RiskTier::Manual;
        let mut registry = SignatureRegistry::new();
        registry.register(broad);
        registry.register(owner);
        assert_eq!(
            registry.by_category_for_mode(Category::System, true).len(),
            2,
            "both fixture signatures must be discoverable on the host platform"
        );
        let result = ScanEngine::scan(
            &registry,
            &LifecycleProviderRegistry::new(Vec::new()),
            &OwnerProviderRegistry::new(Vec::new()),
            None,
            &[],
            true,
            &environment,
            &NeverCancelled,
            |_| {},
        );
        let rows: Vec<_> = result
            .categories
            .iter()
            .flat_map(|category| &category.items)
            .collect();
        assert_eq!(
            rows.len(),
            3,
            "dedicated owner, ordinary payload and unknown excluded namespace"
        );
        assert_eq!(
            rows.iter()
                .filter(|item| item.signature_id == "fixture.dedicated")
                .count(),
            1
        );
        let uncovered = rows
            .iter()
            .find(|item| item.signature_id.starts_with(PREFIX))
            .unwrap();
        assert_eq!(uncovered.name, names["excluded_unknown"].as_str().unwrap());
        assert_eq!(uncovered.size.logical, 8192);
        assert!(!uncovered.allows_cleanup());
        assert!(registry.get(&uncovered.signature_id).is_none());
        assert_eq!(
            result.total_bytes,
            rows.iter().map(|item| item.observed_bytes()).sum::<u64>()
        );
        assert!(result.cleanable_bytes <= result.total_bytes);
        assert_eq!(result.ambiguous_overlap_bytes, 0);
    }
}
