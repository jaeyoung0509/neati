//! Real SwiftPM validation; creates and mutates only a disposable home.
#[cfg(target_os = "macos")]
fn main() {
    use neati_lib::cleaner::{
        owner_providers::tool_cleanup::{ToolCacheKind, ToolCleanupProvider},
        OwnerScopedProvider,
    };
    use neati_lib::models::{
        OwnerProviderSelection, ProviderStatus, RunningProcessPolicy, RunningProcessProbe,
    };
    use neati_platform::{PathFlavor, PlatformEnvironment};
    use std::sync::Arc;
    struct Idle;
    impl RunningProcessProbe for Idle {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(vec![])
        }
    }
    let fixture = tempfile::Builder::new()
        .prefix("neati-swiftpm-validation-")
        .tempdir_in("/private/tmp")
        .unwrap();
    let home = fixture.path().join("home");
    let cache = home.join("Library/Caches/org.swift.swiftpm");
    for relative in [
        "repositories/download/payload",
        "registry/downloads/package/payload",
        "manifests/manifest.db",
        "artifacts/keep",
        "prebuilts/keep",
        "configuration/keep",
        "security/keep",
    ] {
        let path = cache.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![42; 8192]).unwrap();
    }
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(&home)
        .with_temp_dir(fixture.path());
    let provider = ToolCleanupProvider::native(ToolCacheKind::Swiftpm, Arc::new(Idle));
    let guard = RunningProcessPolicy::none();
    let scan = provider.scan(&environment, &guard);
    println!("scan: {scan:?}");
    assert_eq!(scan.status, ProviderStatus::Ready);
    assert_eq!(scan.units.len(), 1);
    let unit = &scan.units[0];
    let plan = provider
        .prepare(
            &environment,
            &guard,
            &[OwnerProviderSelection {
                item_id: format!("fixture.{}", unit.unit_key),
                name: "Fixture".into(),
                path: unit.path.clone(),
                expected_bytes: unit.allocated_bytes,
            }],
        )
        .unwrap();
    let outcome = provider.execute(&environment, &plan);
    println!("result: {outcome:?}");
    assert_eq!(outcome.units[0].status, ProviderStatus::Cleaned);
    for relative in [
        "repositories/download/payload",
        "registry/downloads/package/payload",
        "manifests/manifest.db",
    ] {
        assert!(!cache.join(relative).exists());
    }
    for relative in [
        "artifacts/keep",
        "prebuilts/keep",
        "configuration/keep",
        "security/keep",
    ] {
        assert!(cache.join(relative).exists());
    }
    assert!(provider.scan(&environment, &guard).units.is_empty());
    println!("All scope and preservation assertions passed; disposable fixture removed on exit.");
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("SwiftPM validation requires the macOS owner adapter");
}
