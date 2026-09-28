use super::{engine::apply_signature_owner_state, DirectoryScanner, ScanEngine};
use crate::applications::RunningApplications;
use crate::cleaner::{CleanExecutor, LifecycleProviderRegistry, OwnerProviderRegistry};
use crate::models::{
    CleanFailureReason, CleanStatus, CleanStrategy, CleanupEligibility, CleanupMode,
    CleanupUnitKind, DeletePlan, NeatiError, NeverCancelled, ObservationQuality, RiskTier,
    ScanItem, Signature, StructuredStatePolicy,
};
use crate::safety::SafetyPlanner;
use crate::signatures::{SignatureLoader, SignatureRegistry};
use neati_platform::path_algebra::PathFlavor;
use neati_platform::paths::SimulatedPaths;
use neati_platform::{MockTrashBackend, PlatformEnvironment};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CACHE_NAMES: [&str; 3] = ["DawnGraphiteCache", "DawnWebGPUCache", "GrShaderCache"];
const OWNERS: [&str; 3] = ["Codex", "Antigravity", "Cursor"];

#[test]
fn browser_gpu_scopes_measure_profiles_and_recheck_safety_before_fixture_trash() {
    for (id, relative, owner) in [
        ("system.chrome.gpu_cache", "Google/Chrome", "Google Chrome"),
        (
            "system.brave.gpu_cache",
            "BraveSoftware/Brave-Browser",
            "Brave Browser",
        ),
    ] {
        let fixture = Fixture::new();
        // Use the runner's real path flavor, even for this macOS catalog rule.
        let root = relative.split('/').fold(
            fixture
                ._directory
                .path()
                .join("home")
                .join("Library")
                .join("Application Support"),
            |path, part| path.join(part),
        );
        let signature = fixture.registry.get(id).unwrap();
        assert_eq!(signature.platforms, [crate::models::PlatformKind::Macos]);
        assert_eq!(
            signature.structured_state_policy(),
            StructuredStatePolicy::ProtectAll
        );
        assert_eq!(signature.owner, owner);
        assert_eq!(signature.paths, [
            format!("~/Library/Application Support/{relative}/*/{{GPUCache,DawnCache,DawnGraphiteCache,DawnWebGPUCache}}"),
            format!("~/Library/Application Support/{relative}/{{ShaderCache,GrShaderCache,GraphiteDawnCache}}"),
        ]);
        let caches = [
            root.join("Default").join("GPUCache"),
            root.join("Profile 2").join("DawnWebGPUCache"),
            root.join("Default").join("DawnGraphiteCache"),
            root.join("Default").join("DawnCache"),
            root.join("ShaderCache"),
            root.join("GrShaderCache"),
            root.join("GraphiteDawnCache"),
        ];
        for cache in &caches {
            write(&cache.join("shader.bin"));
        }
        let protected = root.join("Default").join("Local Storage").join("state.bin");
        let offline = root
            .join("Default")
            .join("Service Worker")
            .join("CacheStorage")
            .join("offline.bin");
        let lookalike = root
            .with_file_name("Browser-lookalike")
            .join("Default")
            .join("GPUCache")
            .join("shader.bin");
        let deeper_cache = root
            .join("Default")
            .join("Extensions")
            .join("GPUCache")
            .join("shader.bin");
        for path in [&protected, &offline, &lookalike, &deeper_cache] {
            write(path);
        }
        let mut items =
            DirectoryScanner::scan_signature(signature, &fixture.environment, &NeverCancelled);
        apply_signature_owner_state(&mut items, signature, &idle());
        assert_eq!(items.len(), caches.len());
        assert!(items
            .iter()
            .all(|item| item.is_selected && item.cleanable_bytes() > 0));
        let expected: u64 = items.iter().map(ScanItem::cleanable_bytes).sum();
        for name in &signature.fail_if_running {
            let running = RunningApplications::from_process_names([name.clone()]);
            let mut busy = items.clone();
            apply_signature_owner_state(&mut busy, signature, &running);
            assert!(busy
                .iter()
                .all(|item| item.owner_running && !item.is_selected));
            assert_refused(
                fixture.plan(&items, &running),
                CleanFailureReason::SafetyBoundary,
            );
        }
        let plan = fixture.plan(&items, &idle()).unwrap();
        let mock_trash = MockTrashBackend::new();
        let result = fixture.execute(
            plan,
            &RunningApplications::from_process_names([owner.into()]),
            &mock_trash,
        );
        assert!(result
            .items
            .iter()
            .all(|item| item.failure_reason == Some(CleanFailureReason::InUse)));
        assert!(mock_trash.moved().is_empty());
        let plan = fixture.plan(&items, &idle()).unwrap();
        let trash = FixtureTrash {
            source: root.clone(),
            destination: fixture._directory.path().join("fixture-trash"),
        };
        let result = fixture.execute(plan, &idle(), &trash);
        assert_eq!(result.total_moved_to_trash_bytes, expected);
        assert_eq!(result.total_reclaimed_bytes, 0);
        assert!(caches.iter().all(|path| !path.exists()));
        for path in [&protected, &offline, &lookalike, &deeper_cache] {
            assert!(path.exists());
        }
    }
}

#[test]
fn browser_gpu_scopes_refuse_new_structured_state_and_unknown_process_state() {
    for id in ["system.chrome.gpu_cache", "system.brave.gpu_cache"] {
        let fixture = Fixture::new();
        let signature = fixture.registry.get(id).unwrap();
        let paths = SignatureLoader::expand_path(
            &signature.paths.last().unwrap().replace(
                "{ShaderCache,GrShaderCache,GraphiteDawnCache}",
                "ShaderCache",
            ),
            &fixture.environment,
        )
        .unwrap();
        let cache = paths;
        write(&cache.join("nested").join("shader.bin"));
        let items =
            DirectoryScanner::scan_signature(signature, &fixture.environment, &NeverCancelled);
        assert_eq!(items.len(), 1);
        assert_refused(
            fixture.plan(&items, &RunningApplications::default()),
            CleanFailureReason::SafetyBoundary,
        );
        let plan = fixture.plan(&items, &idle()).unwrap();
        write(&cache.join("nested").join("auth.json"));
        assert_refused(
            fixture.plan(&items, &idle()),
            CleanFailureReason::StructuredStore,
        );
        let trash = MockTrashBackend::new();
        let result = fixture.execute(plan, &idle(), &trash);
        assert_eq!(
            result.items[0].failure_reason,
            Some(CleanFailureReason::StructuredStore)
        );
        assert!(trash.moved().is_empty());
        assert!(cache.join("nested").join("shader.bin").exists());
    }
}

#[test]
fn browser_gpu_scopes_reject_links_and_replaced_roots() {
    let fixture = Fixture::new();
    let signature = fixture.registry.get("system.brave.gpu_cache").unwrap();
    let cache = SignatureLoader::expand_path(
        "~/Library/Application Support/BraveSoftware/Brave-Browser/Default/GPUCache",
        &fixture.environment,
    )
    .unwrap();
    write(&cache.join("shader.bin"));
    let items = DirectoryScanner::scan_signature(signature, &fixture.environment, &NeverCancelled);
    assert_eq!(items.len(), 1);
    let plan = fixture.plan(&items, &idle()).unwrap();
    let original = cache.with_file_name("original");
    std::fs::rename(&cache, &original).unwrap();
    write(&cache.join("replacement.bin"));
    let trash = MockTrashBackend::new();
    let result = fixture.execute(plan, &idle(), &trash);
    assert_eq!(
        result.items[0].failure_reason,
        Some(CleanFailureReason::ChangedSinceScan)
    );
    assert!(trash.moved().is_empty());
    let link = cache.with_file_name("DawnCache");
    directory_link(&link, &original);
    let items = DirectoryScanner::scan_signature(signature, &fixture.environment, &NeverCancelled);
    assert_eq!(items.len(), 2);
    let refused_link = items
        .iter()
        .find(|item| Path::new(&item.path) == link)
        .unwrap();
    assert_eq!(refused_link.quality, ObservationQuality::Unavailable);
    assert_eq!(refused_link.cleanable_bytes(), 0);
    assert!(!refused_link.is_selected);
    assert!(items
        .iter()
        .any(|item| Path::new(&item.path) == cache && item.cleanable_bytes() > 0));
    assert!(original.join("shader.bin").exists());
}

struct Fixture {
    _directory: tempfile::TempDir,
    profiles: PathBuf,
    environment: PlatformEnvironment,
    registry: SignatureRegistry,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        // mklink treats forward slashes as switches, so fixture paths passed
        // to that Windows command must use native separators throughout.
        let roaming = home.join("AppData").join("Roaming");
        let profiles = if cfg!(target_os = "windows") {
            roaming.clone()
        } else {
            home.join("Library/Application Support")
        };
        let roots = SimulatedPaths::new()
            .with_flavor(PathFlavor::current())
            .with_home(home)
            .with_roaming_app_data(roaming);
        let environment =
            PlatformEnvironment::simulated(PathFlavor::current()).with_roots(Arc::new(roots));
        let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
        Self {
            _directory: directory,
            profiles,
            environment,
            registry,
        }
    }

    fn signature(&self, owner: &str) -> &Signature {
        self.registry
            .get(&format!(
                "ai.{}.gpu_cache{}",
                owner.to_ascii_lowercase(),
                suffix()
            ))
            .unwrap()
    }

    fn advisory(&self) -> &Signature {
        self.registry
            .get(&format!(
                "system.advisory.application_gpu_caches{}",
                suffix()
            ))
            .unwrap()
    }

    fn cache(&self, owner: &str, name: &str) -> PathBuf {
        let cache = self.profiles.join(owner).join(name);
        write(&cache.join("shader.bin"));
        cache
    }

    fn scan(&self, owner: &str, processes: &RunningApplications) -> Vec<ScanItem> {
        let signature = self.signature(owner);
        let mut items =
            DirectoryScanner::scan_signature(signature, &self.environment, &NeverCancelled);
        apply_signature_owner_state(&mut items, signature, processes);
        items
    }

    fn plan(
        &self,
        items: &[ScanItem],
        processes: &RunningApplications,
    ) -> Result<DeletePlan, NeatiError> {
        SafetyPlanner::create_plan_with_process_probe(
            items,
            &self.registry,
            &self.environment,
            &OwnerProviderRegistry::new(Vec::new()),
            processes,
        )
    }

    fn execute(
        &self,
        plan: DeletePlan,
        processes: &RunningApplications,
        trash: &dyn neati_platform::TrashBackend,
    ) -> crate::models::CleanResult {
        CleanExecutor::execute_with_process_probe(
            plan,
            &self.environment,
            &LifecycleProviderRegistry::new(Vec::new()),
            &OwnerProviderRegistry::new(Vec::new()),
            trash,
            processes,
            |_| {},
        )
    }
}

fn suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        ".windows"
    } else {
        ""
    }
}

fn idle() -> RunningApplications {
    RunningApplications::from_process_names(["unrelated-fixture-process".into()])
}

fn write(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; 8192]).unwrap();
}

struct FixtureTrash {
    source: PathBuf,
    destination: PathBuf,
}

impl neati_platform::TrashBackend for FixtureTrash {
    fn move_to_trash(&self, path: &Path) -> Result<(), String> {
        if !path.starts_with(&self.source) {
            return Err("fixture Trash must not move real user paths".into());
        }
        std::fs::create_dir_all(&self.destination).map_err(|error| error.to_string())?;
        std::fs::rename(path, self.destination.join(path.file_name().unwrap()))
            .map_err(|error| error.to_string())
    }
}

fn assert_refused(result: Result<DeletePlan, NeatiError>, reason: CleanFailureReason) {
    let Err(NeatiError::RefusedSelection(refusals)) = result else {
        panic!("expected an item-scoped refusal, got {result:?}");
    };
    assert!(!refusals.is_empty());
    assert!(
        refusals.iter().all(|refusal| refusal.reason == reason),
        "{refusals:?}"
    );
}

#[test]
fn gpu_catalog_has_exact_owner_scopes_on_both_platforms() {
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    for (suffix, root, executable_suffix) in [
        ("", "~/Library/Application Support", ""),
        (".windows", "${ROAMING_APP_DATA}", ".exe"),
    ] {
        let advisory = registry
            .get(&format!("system.advisory.application_gpu_caches{suffix}"))
            .unwrap();
        assert_eq!(advisory.risk, RiskTier::Manual);
        assert_eq!(advisory.strategy, CleanStrategy::Manual);
        assert_eq!(advisory.exclusions.len(), OWNERS.len() * CACHE_NAMES.len());
        for owner in OWNERS {
            let signature = registry
                .get(&format!(
                    "ai.{}.gpu_cache{suffix}",
                    owner.to_ascii_lowercase()
                ))
                .unwrap();
            assert_eq!(
                signature.paths,
                [format!("{root}/{owner}/{{{}}}", CACHE_NAMES.join(","))]
            );
            assert_eq!(signature.ownership().owner, owner);
            assert_eq!(signature.risk, RiskTier::Rebuild);
            assert_eq!(signature.strategy, CleanStrategy::DeleteDirectory);
            assert_eq!(signature.unit_kind(), CleanupUnitKind::NamedSubtree);
            assert_eq!(signature.min_age_days, Some(0));
            assert_eq!(
                signature.structured_state_policy(),
                StructuredStatePolicy::ProtectAll
            );
            assert!(signature
                .process_guard()
                .matches(&format!("{owner}{executable_suffix}")));
            if suffix.is_empty() {
                assert!(signature
                    .process_guard()
                    .matches(&format!("{owner} Helper (GPU)")));
            }
            for cache in CACHE_NAMES {
                assert!(advisory
                    .exclusions
                    .contains(&format!("{root}/{owner}/{cache}")));
            }
        }
    }
}

#[test]
fn gpu_discovery_keeps_unknown_owners_visible_without_shadowing_known_owners() {
    let fixture = Fixture::new();
    let mut registry = SignatureRegistry::new();
    let mut expected_bytes = 0;
    for owner in OWNERS {
        for cache in CACHE_NAMES {
            fixture.cache(owner, cache);
        }
        write(&fixture.profiles.join(owner).join("Cookies"));
        write(&fixture.profiles.join(owner).join("Local Storage/state.bin"));
        let items = fixture.scan(owner, &idle());
        assert_eq!(items.len(), CACHE_NAMES.len());
        for item in &items {
            assert!(item.observed_bytes() > 0);
            assert_eq!(item.observed_bytes(), item.cleanable_bytes());
            assert_eq!(
                item.disposition.eligibility,
                CleanupEligibility::AutoCleanable
            );
            assert!(item.is_selected);
            expected_bytes += item.observed_bytes();
        }
        registry.register(fixture.signature(owner).clone());
    }
    for owner in ["UnknownApp", "Codex-neighbor"] {
        for cache in CACHE_NAMES {
            fixture.cache(owner, cache);
        }
    }
    let unknown =
        DirectoryScanner::scan_signature(fixture.advisory(), &fixture.environment, &NeverCancelled);
    assert_eq!(unknown.len(), 2 * CACHE_NAMES.len());
    for item in &unknown {
        assert_eq!(item.risk, RiskTier::Manual);
        assert_eq!(item.disposition.eligibility, CleanupEligibility::Advisory);
        assert!(!item.is_selected);
        assert_eq!(item.cleanable_bytes(), 0);
        expected_bytes += item.observed_bytes();
    }
    let mut selected = unknown.clone();
    for item in &mut selected {
        item.is_selected = true;
    }
    assert_refused(
        fixture.plan(&selected, &idle()),
        CleanFailureReason::OwnerManaged,
    );
    registry.register(fixture.advisory().clone());
    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &fixture.environment,
        &NeverCancelled,
        |_| {},
    );
    assert_eq!(result.total_bytes, expected_bytes);
    assert_eq!(
        result
            .categories
            .iter()
            .map(|category| category.items.len())
            .sum::<usize>(),
        15
    );
    assert_eq!(result.suppressed_duplicate_count, 0);
    assert!(result
        .categories
        .iter()
        .flat_map(|category| &category.items)
        .filter(|item| item.signature_id.starts_with("ai."))
        .all(|item| item.risk == RiskTier::Rebuild && item.overlaps.is_empty()));
}

#[test]
fn gpu_owner_state_is_checked_at_scan_and_again_at_planning() {
    let fixture = Fixture::new();
    fixture.cache("Codex", CACHE_NAMES[0]);
    let idle_items = fixture.scan("Codex", &idle());
    assert_eq!(idle_items.len(), 1);
    assert!(idle_items[0].is_selected);
    for process in &fixture.signature("Codex").fail_if_running {
        let running = RunningApplications::from_process_names([process.clone()]);
        let items = fixture.scan("Codex", &running);
        assert_eq!(items.len(), 1);
        assert!(items[0].owner_running);
        assert!(!items[0].is_selected);
        assert!(items[0].observed_bytes() > 0);
        assert_refused(
            fixture.plan(&idle_items, &running),
            CleanFailureReason::SafetyBoundary,
        );
    }
    let unknown = RunningApplications::default();
    let items = fixture.scan("Codex", &unknown);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].quality, ObservationQuality::Unavailable);
    assert!(items[0].observed_bytes() > 0);
    assert_eq!(items[0].cleanable_bytes(), 0);
    assert!(!items[0].is_selected);
    assert_refused(
        fixture.plan(&idle_items, &unknown),
        CleanFailureReason::SafetyBoundary,
    );
}

#[test]
fn gpu_execution_rechecks_owner_before_any_mutation() {
    let fixture = Fixture::new();
    let cache = fixture.cache("Codex", CACHE_NAMES[0]);
    let items = fixture.scan("Codex", &idle());
    let running = RunningApplications::from_process_names([fixture
        .signature("Codex")
        .fail_if_running[0]
        .clone()]);
    for (processes, reason) in [
        (running, CleanFailureReason::InUse),
        (
            RunningApplications::default(),
            CleanFailureReason::SafetyBoundary,
        ),
    ] {
        let plan = fixture.plan(&items, &idle()).unwrap();
        assert_eq!(
            plan.targets[0].process_guard,
            fixture.signature("Codex").process_guard()
        );
        let trash = MockTrashBackend::new();
        let result = fixture.execute(plan, &processes, &trash);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].failure_reason, Some(reason));
        assert!(!result.items[0].success);
        assert_eq!(result.total_reclaimed_bytes, 0);
        assert_eq!(result.total_moved_to_trash_bytes, 0);
        assert!(trash.moved().is_empty());
        assert!(cache.join("shader.bin").exists());
    }
}

#[test]
fn gpu_cleanup_only_removes_authorized_cache_subtrees_in_a_temporary_profile() {
    let fixture = Fixture::new();
    let caches = CACHE_NAMES.map(|name| fixture.cache("Cursor", name));
    let profile = fixture.profiles.join("Cursor");
    let siblings = [
        "Cookies",
        "Sessions/current",
        "Local Storage/state",
        "Service Worker/CacheStorage/offline",
        "settings.json",
    ];
    for sibling in siblings {
        write(&profile.join(sibling));
    }
    let items = fixture.scan("Cursor", &idle());
    let plan = fixture.plan(&items, &idle()).unwrap();
    assert_eq!(plan.targets.len(), 3);
    assert_eq!(plan.mode, CleanupMode::Trash);
    let trash = MockTrashBackend::new();
    let trash_result = fixture.execute(plan.clone(), &idle(), &trash);
    assert_eq!(trash.moved().len(), 3);
    assert!(trash.moved().iter().all(|path| caches.contains(path)));
    assert!(trash_result
        .items
        .iter()
        .all(|item| item.status == CleanStatus::Success));
    // Actual mutation is confined to this fixture; never use the native Trash.
    let trash = FixtureTrash {
        source: fixture.profiles.clone(),
        destination: fixture._directory.path().join("fixture-trash"),
    };
    let result = fixture.execute(plan, &idle(), &trash);
    assert_eq!(result.items.len(), 3);
    assert!(
        result
            .items
            .iter()
            .all(|item| item.status == CleanStatus::Success),
        "{result:?}"
    );
    assert!(caches.iter().all(|cache| !cache.exists()));
    assert!(CACHE_NAMES.iter().all(|name| trash
        .destination
        .join(name)
        .join("shader.bin")
        .exists()));
    for sibling in siblings {
        assert!(profile.join(sibling).exists());
    }
}

#[test]
fn gpu_protected_state_is_refused_at_planning_and_if_added_after_planning() {
    for protected in [
        "index.db",
        "index.db-wal",
        "LOCK",
        "auth.json",
        "settings.json",
    ] {
        let fixture = Fixture::new();
        let cache = fixture.cache("Antigravity", CACHE_NAMES[0]);
        write(&cache.join("nested/shader.bin"));
        let items = fixture.scan("Antigravity", &idle());
        let plan = fixture.plan(&items, &idle()).unwrap();
        // A nested change does not replace the authorized root's identity;
        // execution must still reclassify descendants, not trust the plan.
        write(&cache.join("nested").join(protected));
        assert_refused(
            fixture.plan(&items, &idle()),
            CleanFailureReason::StructuredStore,
        );
        let trash = MockTrashBackend::new();
        let result = fixture.execute(plan, &idle(), &trash);
        assert_eq!(
            result.items[0].failure_reason,
            Some(CleanFailureReason::StructuredStore)
        );
        assert_eq!(result.total_moved_to_trash_bytes, 0);
        assert!(trash.moved().is_empty());
        assert!(cache.join("nested").join(protected).exists());
        assert!(cache.join("shader.bin").exists());
    }
}

#[test]
fn gpu_replaced_target_is_refused_after_planning() {
    let fixture = Fixture::new();
    let cache = fixture.cache("Cursor", CACHE_NAMES[0]);
    let plan = fixture
        .plan(&fixture.scan("Cursor", &idle()), &idle())
        .unwrap();
    std::fs::rename(&cache, cache.with_file_name("original-cache")).unwrap();
    write(&cache.join("replacement.bin"));
    let trash = MockTrashBackend::new();
    let result = fixture.execute(plan, &idle(), &trash);
    assert_eq!(
        result.items[0].failure_reason,
        Some(CleanFailureReason::ChangedSinceScan)
    );
    assert!(trash.moved().is_empty());
    assert!(cache.join("replacement.bin").exists());
}

#[cfg(unix)]
fn directory_link(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn directory_link(link: &Path, target: &Path) {
    let result = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(result.status.success(), "junction fixture: {result:?}");
}

#[test]
fn gpu_discovery_never_follows_linked_cache_roots_or_profiles() {
    let fixture = Fixture::new();
    let outside = fixture._directory.path().join("outside");
    write(&outside.join("shader.bin"));
    let profile = fixture.profiles.join("Codex");
    std::fs::create_dir_all(&profile).unwrap();
    directory_link(&profile.join(CACHE_NAMES[0]), &outside);
    directory_link(&fixture.profiles.join("Cursor"), &outside);
    write(&outside.join(CACHE_NAMES[1]).join("external.bin"));
    assert!(fixture.scan("Codex", &idle()).is_empty());
    let linked_profile = fixture.scan("Cursor", &idle());
    assert!(
        !linked_profile.is_empty(),
        "an unreadable selector remains visible"
    );
    assert!(linked_profile.iter().all(|item| item.observed_bytes() == 0
        && item.cleanable_bytes() == 0
        && !item.is_selected
        && item.quality == ObservationQuality::Unavailable));
    assert!(outside.join("shader.bin").exists());
}

#[test]
fn gpu_windows_paths_resolve_against_relocated_roaming_data() {
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::Windows).with_roots(Arc::new(
        SimulatedPaths::new()
            .with_flavor(PathFlavor::Windows)
            .with_home(r"C:\Users\fixture")
            .with_roaming_app_data(r"D:\Roaming"),
    ));
    let signature = registry.get("ai.codex.gpu_cache.windows").unwrap();
    let expanded = SignatureLoader::expand_path(&signature.paths[0], &environment).unwrap();
    assert!(neati_platform::path_algebra::equal(
        &expanded.to_string_lossy(),
        r"D:\Roaming\Codex\{DawnGraphiteCache,DawnWebGPUCache,GrShaderCache}",
        PathFlavor::Windows,
    ));
}
