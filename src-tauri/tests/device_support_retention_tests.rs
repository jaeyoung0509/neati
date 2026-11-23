//! Xcode DeviceSupport keeps the newest symbol set per platform (#480).
//!
//! Every platform folder Xcode writes is covered under the owner-guarded
//! build-output policy (#350), and within each folder the newest version is
//! reported but never offered. The scan, the planner and the execution guard
//! each list the siblings again, so a set that became the newest after the
//! scan is still kept.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use neati_core::domain::platform::PlatformKind;
use neati_lib::cleaner::{CleanExecutor, LifecycleProviderRegistry, OwnerProviderRegistry};
use neati_lib::models::{
    CacheArtifactKind, CleanFailureReason, CleanStrategy, CleanupEligibility, CleanupUnitKind,
    NeatiError, NeverCancelled, RetentionReason, ScanItem, StructuredStatePolicy,
    OWNER_GUARDED_BUILD_OUTPUT, RETAIN_NEWEST_VERSION,
};
use neati_lib::safety::SafetyPlanner;
use neati_lib::scanner::ScanEngine;
use neati_lib::signatures::SignatureRegistry;
use neati_platform::{PathFlavor, PlatformEnvironment};

const XCODE: &str = "Library/Developer/Xcode";

const PLATFORMS: [(&str, &str); 5] = [
    ("system.xcode_device_support", "iOS DeviceSupport"),
    (
        "system.xcode_watchos_device_support",
        "watchOS DeviceSupport",
    ),
    ("system.xcode_tvos_device_support", "tvOS DeviceSupport"),
    (
        "system.xcode_visionos_device_support",
        "visionOS DeviceSupport",
    ),
    ("system.xcode_macos_device_support", "macOS DeviceSupport"),
];

fn environment(home: &Path) -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::Posix)
        .with_platform(PlatformKind::Macos)
        .with_home(home)
        .with_temp_dir(home.join("private-tmp"))
        .with_missing_tool("docker")
        .with_missing_tool("xcrun")
}

/// Writes one symbol set and ages every file and folder in it past the
/// 30-day rule, so age is never the reason a set is kept.
fn symbol_set(home: &Path, folder: &str, version: &str) {
    let root = home.join(XCODE).join(folder).join(version);
    for file in [
        "Symbols/usr/lib/libobjc.A.dylib",
        "Symbols/System/Library/Frameworks/Foundation.framework/Foundation",
        "Info.plist",
    ] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![7u8; 4096]).unwrap();
    }
    let aged = SystemTime::now() - Duration::from_secs(90 * 24 * 60 * 60);
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                fs::File::options()
                    .write(true)
                    .open(entry.path())
                    .unwrap()
                    .set_modified(aged)
                    .unwrap();
            }
        }
        fs::File::open(&dir).unwrap().set_modified(aged).unwrap();
    }
}

fn device_support_items(home: &Path, registry: &SignatureRegistry) -> Vec<ScanItem> {
    ScanEngine::scan(
        registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        true,
        &environment(home),
        &NeverCancelled,
        |_| {},
    )
    .categories
    .into_iter()
    .flat_map(|category| category.items)
    .filter(|item| RETAIN_NEWEST_VERSION.contains(&item.signature_id.as_str()))
    .collect()
}

fn item<'a>(items: &'a [ScanItem], folder: &str, version: &str) -> &'a ScanItem {
    items
        .iter()
        .find(|item| item.path.ends_with(&format!("{folder}/{version}")))
        .unwrap_or_else(|| panic!("{folder}/{version} is reported: {items:#?}"))
}

fn plan(
    home: &Path,
    registry: &SignatureRegistry,
    item: &ScanItem,
) -> Result<neati_lib::models::DeletePlan, NeatiError> {
    SafetyPlanner::create_plan_with_environment(
        std::slice::from_ref(item),
        registry,
        &environment(home),
        &OwnerProviderRegistry::new(Vec::new()),
    )
}

#[test]
fn every_device_support_platform_is_reviewed_build_output_that_keeps_its_newest_set() {
    let home = tempfile::tempdir().unwrap();
    let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
    let ios = registry.get("system.xcode_device_support").unwrap();
    for (id, folder) in PLATFORMS {
        let signature = registry
            .get(id)
            .unwrap_or_else(|| panic!("{id} is catalogued"));
        assert!(OWNER_GUARDED_BUILD_OUTPUT.contains(&id), "{id}");
        assert_eq!(
            signature.structured_state_policy(),
            StructuredStatePolicy::OwnerGuardedBuildOutput,
            "{id}"
        );
        assert_eq!(
            signature.paths,
            [format!("~/{XCODE}/{folder}/*")],
            "{id} names exactly its own platform folder"
        );
        assert_eq!(signature.unit_kind(), CleanupUnitKind::NamedSubtree);
        assert_eq!(signature.strategy, CleanStrategy::DeleteDirectory);
        assert_eq!(signature.artifact_kind, CacheArtifactKind::BuildArtifact);
        assert_eq!(signature.min_age_days, ios.min_age_days);
        assert_eq!(signature.fail_if_running, ios.fail_if_running);
        assert_eq!(signature.risk, ios.risk);
    }
    let mut retained = RETAIN_NEWEST_VERSION.to_vec();
    retained.sort_unstable();
    let mut platforms = PLATFORMS.map(|(id, _)| id).to_vec();
    platforms.sort_unstable();
    assert_eq!(retained, platforms);
}

#[test]
fn the_newest_symbol_set_per_platform_is_reported_but_never_offered() {
    let home = tempfile::tempdir().unwrap();
    let ios = "iOS DeviceSupport";
    let watch = "watchOS DeviceSupport";
    let tv = "tvOS DeviceSupport";
    for version in [
        "16.7 (20H19)",
        "17.4.1 (21E236) arm64e",
        "iPhone15,2 18.1 (22B83)",
        "18.1 (22B83) arm64e",
    ] {
        symbol_set(home.path(), ios, version);
    }
    for version in ["Watch6,2 10.0 (21R356)", "Watch7,3 11.1 (22S73)"] {
        symbol_set(home.path(), watch, version);
    }
    symbol_set(home.path(), tv, "18.0 (22J357)");

    let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
    let items = device_support_items(home.path(), &registry);
    assert_eq!(items.len(), 7, "every symbol set is reported: {items:#?}");

    for (folder, version) in [
        (ios, "iPhone15,2 18.1 (22B83)"),
        (ios, "18.1 (22B83) arm64e"),
        (watch, "Watch7,3 11.1 (22S73)"),
        (tv, "18.0 (22J357)"),
    ] {
        let kept = item(&items, folder, version);
        assert_eq!(kept.retained, Some(RetentionReason::NewestVersion));
        assert_eq!(kept.disposition.eligibility, CleanupEligibility::Recent);
        assert_eq!(
            kept.disposition.reason.as_deref(),
            Some(RetentionReason::NewestVersion.explanation())
        );
        assert!(!kept.is_selected);
        assert_eq!(kept.cleanable_bytes(), 0);
        assert!(kept.observed_bytes() > 0, "kept bytes are still reported");
        assert!(kept.has_current_disposition());
        // A forged selection of the newest set is refused before planning.
        let mut forged = kept.clone();
        forged.is_selected = true;
        assert!(plan(home.path(), &registry, &forged).is_err());
    }
    for (folder, version) in [
        (ios, "16.7 (20H19)"),
        (ios, "17.4.1 (21E236) arm64e"),
        (watch, "Watch6,2 10.0 (21R356)"),
    ] {
        let older = item(&items, folder, version);
        assert_eq!(older.retained, None);
        // A running Xcode on the host turns `AutoCleanable` into `Reviewable`;
        // both are offered, and only the first is preselected.
        assert!(
            older.disposition.is_cleanable(),
            "{folder}/{version}: {:?}",
            older.disposition
        );
        assert_eq!(
            older.is_selected,
            older.disposition.eligibility == CleanupEligibility::AutoCleanable
        );
    }
}

/// The planner lists the siblings again: a set that became the newest after
/// the scan, because the newer one was removed, is not planned.
#[test]
fn planning_re_derives_which_set_is_newest() {
    let home = tempfile::tempdir().unwrap();
    let folder = "iOS DeviceSupport";
    for version in ["16.7 (20H19)", "17.4 (21E213)", "18.1 (22B83)"] {
        symbol_set(home.path(), folder, version);
    }
    let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
    let items = device_support_items(home.path(), &registry);
    let middle = item(&items, folder, "17.4 (21E213)").clone();
    assert!(middle.disposition.is_cleanable());

    fs::remove_dir_all(home.path().join(XCODE).join(folder).join("18.1 (22B83)")).unwrap();
    match plan(home.path(), &registry, &middle) {
        Err(NeatiError::RefusedSelection(refusals)) => {
            assert_eq!(refusals.len(), 1);
            assert_eq!(refusals[0].reason, CleanFailureReason::ChangedSinceScan);
            assert_eq!(
                refusals[0].message,
                RetentionReason::NewestVersion.explanation()
            );
        }
        other => panic!("the new newest set must not be planned: {other:?}"),
    }
    // The oldest set is still plannable: it is not the newest now either.
    let oldest = item(&items, folder, "16.7 (20H19)");
    assert!(plan(home.path(), &registry, oldest).is_ok());
    assert!(home
        .path()
        .join(XCODE)
        .join(folder)
        .join("17.4 (21E213)/Info.plist")
        .is_file());
}

/// The execution guard lists the siblings again immediately before removal.
#[test]
fn execution_re_derives_which_set_is_newest() {
    let home = tempfile::tempdir().unwrap();
    let folder = "watchOS DeviceSupport";
    for version in [
        "Watch6,2 9.6 (20U73)",
        "Watch6,2 10.0 (21R356)",
        "Watch7,3 11.1 (22S73)",
    ] {
        symbol_set(home.path(), folder, version);
    }
    let env = environment(home.path());
    let registry = SignatureRegistry::load_embedded_with(&env).unwrap();
    let items = device_support_items(home.path(), &registry);
    let selected = vec![
        item(&items, folder, "Watch6,2 9.6 (20U73)").clone(),
        item(&items, folder, "Watch6,2 10.0 (21R356)").clone(),
    ];
    let owner_providers = OwnerProviderRegistry::new(Vec::new());
    let plan =
        SafetyPlanner::create_plan_with_environment(&selected, &registry, &env, &owner_providers)
            .expect("both older sets are plannable while 11.1 exists");

    // The newest set disappears after review, so 10.0 is the newest now.
    fs::remove_dir_all(
        home.path()
            .join(XCODE)
            .join(folder)
            .join("Watch7,3 11.1 (22S73)"),
    )
    .unwrap();
    let result = CleanExecutor::execute(
        plan,
        &env,
        &LifecycleProviderRegistry::new(Vec::new()),
        &owner_providers,
        &neati_platform::MockTrashBackend::new(),
        |_| {},
    );
    let outcome = |version: &str| {
        result
            .items
            .iter()
            .find(|item| item.path.ends_with(version))
            .unwrap_or_else(|| panic!("{version} has a result: {:?}", result.items))
    };
    assert!(outcome("Watch6,2 9.6 (20U73)").success);
    let kept = outcome("Watch6,2 10.0 (21R356)");
    assert!(!kept.success);
    assert_eq!(
        kept.failure_reason,
        Some(CleanFailureReason::ChangedSinceScan)
    );
    assert_eq!(
        kept.error_message.as_deref(),
        Some(RetentionReason::NewestVersion.explanation()),
        "the keep-newest rule, not another guard, kept it"
    );
    let base = home.path().join(XCODE).join(folder);
    assert!(!base.join("Watch6,2 9.6 (20U73)").exists());
    assert!(base.join("Watch6,2 10.0 (21R356)/Info.plist").is_file());
}
