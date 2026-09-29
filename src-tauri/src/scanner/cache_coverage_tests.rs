use super::{DirectoryScanner, NoRootProgress, ScanLimits, TraversalCounters, WalkContext};
use crate::applications::RunningApplications;
use crate::cleaner::{CleanExecutor, LifecycleProviderRegistry, OwnerProviderRegistry};
use crate::models::{CleanupEligibility, NeverCancelled, PlatformKind, ScanItem};
use crate::safety::SafetyPlanner;
use crate::signatures::SignatureRegistry;
use neati_platform::{path_algebra::PathFlavor, MockTrashBackend, PlatformEnvironment};
use std::path::Path;

fn write(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; 8192]).unwrap();
}

fn idle() -> RunningApplications {
    RunningApplications::from_process_names(["fixture-unrelated".into()])
}

fn scan(
    registry: &SignatureRegistry,
    id: &str,
    environment: &PlatformEnvironment,
    processes: &RunningApplications,
) -> Vec<ScanItem> {
    let counters = TraversalCounters::default();
    let context = WalkContext::new(
        environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
    );
    DirectoryScanner::scan_signature_with_context(
        registry.get(id).unwrap(),
        None,
        &context,
        Default::default(),
        processes,
    )
    .items
    .into_iter()
    .filter(|item| {
        item.exists
            && (item.observed_bytes() > 0
                || item.quality != crate::models::ObservationQuality::Fresh)
    })
    .collect()
}

#[test]
fn apple_payloads_are_immediately_eligible_but_nested_models_are_not_reclaimable() {
    let fixture = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
    let root = fixture.path().join("Library/Caches");
    for relative in [
        "com.apple.python/payload",
        "com.apple.unlisted/payload",
        "com.apple.e5rt.e5bundlecache/model",
        "com.example.inference/com.apple.e5rt.e5bundlecache/model",
        "com.example.inference/payload",
        "com.apple.dt.Xcode/payload",
        "com.apple.dt.Xcode-lookalike/payload",
    ] {
        write(&root.join(relative));
    }
    let items = scan(
        &registry,
        "system.intensive.user_app_caches",
        &environment,
        &idle(),
    );
    assert_eq!(items.len(), 4);
    let python = items.iter().find(|i| i.name == "com.apple.python").unwrap();
    assert!(python.is_selected);
    assert_eq!(
        python.disposition.eligibility,
        CleanupEligibility::AutoCleanable
    );
    assert_eq!(python.cleanable_bytes(), python.observed_bytes());
    let nested = items
        .iter()
        .find(|i| i.name == "com.example.inference")
        .unwrap();
    assert_eq!(nested.observed_bytes(), 2 * python.observed_bytes());
    assert_eq!(nested.cleanable_bytes(), python.observed_bytes());
    let protected = scan(
        &registry,
        "system.advisory.protected_apple_caches",
        &environment,
        &idle(),
    );
    assert_eq!(protected.len(), 1);
    assert_eq!(protected[0].cleanable_bytes(), 0);
    assert!(!protected[0].is_selected);
    assert_eq!(protected[0].observed_bytes(), python.observed_bytes());
    assert!(items
        .iter()
        .any(|i| i.name == "com.apple.dt.Xcode-lookalike"));
    assert!(!registry.path_is_in_scope(
        registry.get("system.intensive.user_app_caches").unwrap(),
        &root.join("com.apple.dt.Xcode"),
        &environment
    ));
}

#[test]
fn inferred_owner_is_rechecked_at_planning_and_execution() {
    let fixture = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
    let payload = fixture
        .path()
        .join("Library/Caches/com.apple.parsecd/payload");
    write(&payload);
    let busy = RunningApplications::from_process_names(["parsecd".into()]);
    let busy_items = scan(
        &registry,
        "system.intensive.user_app_caches",
        &environment,
        &busy,
    );
    assert!(busy_items[0].owner_running);
    assert!(!busy_items[0].is_selected);
    let unknown = scan(
        &registry,
        "system.intensive.user_app_caches",
        &environment,
        &RunningApplications::default(),
    );
    assert_eq!(
        unknown[0].quality,
        crate::models::ObservationQuality::Unavailable
    );
    let items = scan(
        &registry,
        "system.intensive.user_app_caches",
        &environment,
        &idle(),
    );
    let owners = OwnerProviderRegistry::new(Vec::new());
    let refused = SafetyPlanner::create_plan_with_process_probe(
        &items,
        &registry,
        &environment,
        &owners,
        &busy,
    );
    assert!(matches!(
        refused,
        Err(crate::models::NeatiError::RefusedSelection(_))
    ));
    let plan = SafetyPlanner::create_plan_with_process_probe(
        &items,
        &registry,
        &environment,
        &owners,
        &idle(),
    )
    .unwrap();
    assert_eq!(
        plan.targets[0].process_guard.cache_owner(),
        Some("com.apple.parsecd")
    );
    let result = CleanExecutor::execute_with_process_probe(
        plan,
        &environment,
        &LifecycleProviderRegistry::new(Vec::new()),
        &owners,
        &MockTrashBackend::default(),
        &busy,
        |_| {},
    );
    assert_eq!(result.items[0].status, crate::models::CleanStatus::Failed);
    assert_eq!(result.items[0].bytes_reclaimed, 0);
    assert!(payload.exists());
}

#[test]
fn exact_developer_roots_leave_runtime_and_sibling_data_outside_cleanup() {
    let fixture = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
    for tool in ["typescript", "vite", "webpack", "eslint", "prettier"] {
        write(&fixture.path().join(format!(".cache/{tool}/payload")));
        write(
            &fixture
                .path()
                .join(format!(".cache/{tool}-project/payload")),
        );
        let id = format!("dev.{tool}.user_cache");
        let items = scan(&registry, &id, &environment, &idle());
        assert_eq!(items.len(), 1);
        assert!(items[0].is_selected);
        assert_eq!(items[0].cleanable_bytes(), items[0].observed_bytes());
        assert!(!registry.path_is_in_scope(
            registry.get(&id).unwrap(),
            &fixture
                .path()
                .join(format!(".cache/{tool}-project/payload")),
            &environment
        ));
        let busy = scan(
            &registry,
            &id,
            &environment,
            &RunningApplications::from_process_names(["node".into()]),
        );
        assert!(!busy[0].is_selected);
    }
    write(
        &fixture
            .path()
            .join(".cache/codex-runtimes/install-active/runtime"),
    );
    let runtime = scan(&registry, "dev.codex.runtimes", &environment, &idle());
    assert_eq!(runtime.len(), 1);
    assert_eq!(
        runtime[0].disposition.eligibility,
        CleanupEligibility::Advisory
    );
    assert_eq!(runtime[0].cleanable_bytes(), 0);
    assert!(!runtime[0].is_selected);
}

#[test]
fn stale_cleanup_reclaims_only_the_measured_payload_and_preserves_nested_state() {
    use crate::safety::{RevalidationOutcome, SafeTreeDeleter, SafetyValidator};
    let fixture = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
    let cache = fixture.path().join("Library/Caches/com.example.inference");
    let payload = cache.join("payload");
    write(&payload);
    let protected = [
        cache.join("com.apple.e5rt.e5bundlecache/model"),
        cache.join("credentials/token"),
        cache.join("state.db"),
        cache.join("state.db-wal"),
        cache.join("settings.json"),
    ];
    for path in &protected {
        write(path);
    }
    let items = scan(
        &registry,
        "system.intensive.user_app_caches",
        &environment,
        &idle(),
    );
    assert_eq!(items.len(), 1);
    let payload_size = super::get_allocated_size(&payload).unwrap();
    assert_eq!(items[0].cleanable_bytes(), payload_size);
    assert_eq!(items[0].observed_bytes(), payload_size * 6);
    let owners = OwnerProviderRegistry::new(Vec::new());
    let plan = SafetyPlanner::create_plan_with_process_probe(
        &items,
        &registry,
        &environment,
        &owners,
        &idle(),
    )
    .unwrap();
    let target = match SafetyValidator::revalidate(&plan.targets[0], &environment) {
        RevalidationOutcome::Validated(target) => target,
        other => panic!("fixture must revalidate: {other:?}"),
    };
    let report = SafeTreeDeleter::prune_stale_contents_validated(&target, &environment);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!payload.exists());
    for path in &protected {
        assert!(path.exists(), "{} must survive", path.display());
    }
}

#[test]
fn prefix_and_explicit_exclusions_are_enforced_again_at_authorization() {
    let fixture = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
    let broad = registry.get("system.intensive.user_app_caches").unwrap();
    for name in [
        "CloudKit",
        "com.apple.CloudKit",
        "Homebrew",
        "com.apple.e5rt.e5bundlecache",
        "com.apple.helpd",
        "com.apple.dt.Xcode",
    ] {
        assert!(!registry.path_is_in_scope(
            broad,
            &fixture.path().join("Library/Caches").join(name),
            &environment
        ));
    }
    assert!(!registry.path_is_in_scope(
        broad,
        &fixture.path().join("Library/Caches"),
        &environment
    ));
    let mut included = broad.clone();
    included.include_prefixes = vec!["allowed-".into()];
    assert!(!registry.path_is_in_scope(
        &included,
        &fixture.path().join("Library/Caches/unknown"),
        &environment
    ));
    assert!(registry.path_is_in_scope(
        &included,
        &fixture.path().join("Library/Caches/allowed-payload"),
        &environment
    ));
    let support = registry.get("system.app_support_cache_segments").unwrap();
    assert!(!registry.path_is_in_scope(
        support,
        &fixture
            .path()
            .join("Library/Application Support/Cursor/Cache"),
        &environment
    ));
    assert!(registry.path_is_in_scope(
        support,
        &fixture
            .path()
            .join("Library/Application Support/Cursor-lookalike/Cache"),
        &environment
    ));
}

#[test]
fn zed_download_cache_scope_preserves_installed_runtimes_and_editor_content() {
    let fixture = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_with(&environment).unwrap();
    let root = fixture.path().join("Library/Application Support/Zed");
    let caches = [
        root.join("node/cache/archive"),
        root.join("node/node-v22/cache/archive"),
    ];
    let installed = [
        root.join("node/node-v22/bin/node"),
        root.join("extensions/installed/manifest.json"),
        root.join("languages/server"),
        root.join("agents/runtime"),
    ];
    for path in caches.iter().chain(installed.iter()) {
        write(path);
    }
    let items = scan(
        &registry,
        "dev.zed.node_download_cache",
        &environment,
        &idle(),
    );
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .all(|item| item.is_selected && item.cleanable_bytes() == item.observed_bytes()));
    let signature = registry.get("dev.zed.node_download_cache").unwrap();
    for path in &installed {
        assert!(!registry.path_is_in_scope(signature, path, &environment));
        assert!(path.exists());
    }
    let busy = scan(
        &registry,
        "dev.zed.node_download_cache",
        &environment,
        &RunningApplications::from_process_names(["Zed".into()]),
    );
    assert_eq!(busy.len(), 2);
    assert!(busy
        .iter()
        .all(|item| item.owner_running && !item.is_selected));
}
