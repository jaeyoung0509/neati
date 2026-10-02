//! Native exact-use observations of a separately created task-owned profile.
//! This example never executes a cleanup or moves data to the user's Trash.
#[cfg(target_os = "macos")]
fn main() {
    use neati_lib::{
        applications::RunningApplications,
        cleaner::{
            owner_providers::browser::{BrowserCacheKind, ChromiumCacheProvider},
            OwnerScopedProvider,
        },
        models::{
            OwnerProviderSelection, OwnerUnitState, RunningProcessPolicy, RunningProcessProbe,
        },
        scanner::SizeCalculatorMeasurement,
    };
    use neati_platform::{MockTrashBackend, PathFlavor, PlatformEnvironment};
    use std::{path::PathBuf, sync::Arc};
    struct NativeUse;
    impl RunningProcessProbe for NativeUse {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            RunningApplications::probe().running(guard)
        }
    }
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(args.next().expect("task-owned fixture home"));
    let mode = args.next().expect("busy or idle");
    assert_eq!(
        std::fs::read(home.join(".neati-owned-native-browser-fixture")).unwrap(),
        b"isolated native Chromium owner-use validation"
    );
    let environment = PlatformEnvironment::simulated(PathFlavor::current()).with_home(&home);
    let provider = ChromiumCacheProvider::new(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(NativeUse),
        Arc::new(SizeCalculatorMeasurement),
        Arc::new(MockTrashBackend::default()),
    );
    let guard = RunningProcessPolicy::none();
    let scan = provider.scan(&environment, &guard);
    assert_eq!(scan.units.len(), 1, "{scan:?}");
    let unit = &scan.units[0];
    assert!(unit.allocated_bytes > 0);
    let plan = provider.prepare(
        &environment,
        &guard,
        &[OwnerProviderSelection {
            item_id: format!("fixture.{}", unit.unit_key),
            name: "Native browser fixture".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        }],
    );
    match mode.as_str() {
        "busy" => {
            assert_eq!(unit.state, OwnerUnitState::InUse, "{unit:?}");
            assert!(plan.is_err());
        }
        "idle" => {
            assert_eq!(unit.state, OwnerUnitState::Ready, "{unit:?}");
            assert!(plan.is_ok());
        }
        _ => panic!("unsupported fixture phase"),
    }
    let profile = home.join("Library/Application Support/Google/Chrome/Default");
    assert!(profile.join("Service Worker/Database").is_dir());
    assert!(profile.join("Preferences").is_file());
    println!("Native Chromium exact-use {mode}: {:?}, {} allocated bytes, plan {}. No cleanup executed; registration/profile state preserved", unit.state, unit.allocated_bytes, if plan.is_ok() { "authorized" } else { "refused" });
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("Native Chromium lsof validation requires macOS");
}
