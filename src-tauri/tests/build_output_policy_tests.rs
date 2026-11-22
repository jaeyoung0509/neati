//! Xcode DerivedData is removable as a whole build-output unit (#350).
//!
//! Before this policy the scan offered DerivedData and the planner refused
//! it, because object files and built bundles are "structured state" under the
//! default rule. These tests pin both halves of the fix: the unit is now
//! cleaned, and the relaxed rule reaches nothing else.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use neati_core::domain::platform::PlatformKind;
use neati_lib::cleaner::{CleanExecutor, LifecycleProviderRegistry, OwnerProviderRegistry};
use neati_lib::models::{
    CleanStrategy, NeverCancelled, ScanItem, StructuredStatePolicy, OWNER_GUARDED_BUILD_OUTPUT,
};
use neati_lib::safety::SafetyPlanner;
use neati_lib::scanner::ScanEngine;
use neati_lib::signatures::SignatureRegistry;
use neati_platform::{PathFlavor, PlatformEnvironment};

const DERIVED_DATA: &str = "Library/Developer/Xcode/DerivedData";

fn environment(home: &Path) -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::Posix)
        .with_platform(PlatformKind::Macos)
        .with_home(home)
        .with_temp_dir(home.join("private-tmp"))
        .with_missing_tool("docker")
        .with_missing_tool("xcrun")
}

fn write(home: &Path, relative: &str) {
    let path = home.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, vec![1u8; 8192]).unwrap();
    let old = SystemTime::now() - Duration::from_secs(90 * 24 * 60 * 60);
    fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(old))
        .unwrap();
}

/// What a real DerivedData project directory holds.
const BUILD_OUTPUT: [&str; 7] = [
    "App-abc/Build/Intermediates.noindex/App.build/Objects-normal/arm64/main.o",
    "App-abc/Build/Products/Debug/libCore.dylib",
    "App-abc/Build/Products/Debug/App.app/Contents/MacOS/App",
    "App-abc/Build/Products/Debug/App.app/Contents/Info.plist",
    "App-abc/Index.noindex/DataStore/v5/records/index.sqlite",
    "App-abc/Index.noindex/DataStore/v5/records/index.sqlite-wal",
    "App-abc/info.plist",
];

fn derived_data_item(home: &Path, registry: &SignatureRegistry) -> ScanItem {
    let result = ScanEngine::scan(
        registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        true,
        &environment(home),
        &NeverCancelled,
        |_| {},
    );
    result
        .categories
        .into_iter()
        .flat_map(|category| category.items)
        .find(|item| item.signature_id == "dev.xcode.derived_data")
        .expect("DerivedData is discovered")
}

#[test]
fn only_the_reviewed_signature_receives_the_build_output_policy() {
    let home = tempfile::tempdir().unwrap();
    let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
    let granted: Vec<&str> = registry
        .all()
        .into_iter()
        .filter(|signature| {
            signature.structured_state_policy() == StructuredStatePolicy::OwnerGuardedBuildOutput
        })
        .map(|signature| signature.id.as_str())
        .collect();
    let mut granted = granted;
    granted.sort_unstable();
    let mut reviewed = OWNER_GUARDED_BUILD_OUTPUT.to_vec();
    reviewed.sort_unstable();
    assert_eq!(granted, reviewed);
}

#[test]
fn a_listed_signature_loses_the_policy_when_it_stops_being_guarded_build_output() {
    let home = tempfile::tempdir().unwrap();
    let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
    let reviewed = registry.get("dev.xcode.derived_data").unwrap().clone();
    assert_eq!(
        reviewed.structured_state_policy(),
        StructuredStatePolicy::OwnerGuardedBuildOutput
    );

    let mut unguarded = reviewed.clone();
    unguarded.fail_if_running.clear();
    assert_eq!(
        unguarded.structured_state_policy(),
        StructuredStatePolicy::ProtectAll
    );

    let mut observation_only = reviewed.clone();
    observation_only.strategy = CleanStrategy::Manual;
    assert_eq!(
        observation_only.structured_state_policy(),
        StructuredStatePolicy::ProtectAll
    );

    let mut renamed = reviewed;
    renamed.id = "dev.other.build_output".to_string();
    assert_eq!(
        renamed.structured_state_policy(),
        StructuredStatePolicy::ProtectAll
    );
}

#[test]
fn derived_data_with_objects_bundles_and_index_databases_is_cleaned_by_content() {
    let home = tempfile::tempdir().unwrap();
    for file in BUILD_OUTPUT {
        write(home.path(), &format!("{DERIVED_DATA}/{file}"));
    }
    // A sibling of DerivedData is outside the unit and must not be touched.
    write(
        home.path(),
        "Library/Developer/Xcode/UserData/IDEEditorInteractivityHistory.plist",
    );

    let env = environment(home.path());
    let registry = SignatureRegistry::load_embedded_with(&env).unwrap();
    let item = derived_data_item(home.path(), &registry);
    assert!(item.disposition.is_cleanable(), "{:?}", item.disposition);

    let owner_providers = OwnerProviderRegistry::new(Vec::new());
    let plan = SafetyPlanner::create_plan_with_environment(
        std::slice::from_ref(&item),
        &registry,
        &env,
        &owner_providers,
    )
    .expect("the scan and the planner now agree on DerivedData");
    let result = CleanExecutor::execute(
        plan,
        &env,
        &LifecycleProviderRegistry::new(Vec::new()),
        &owner_providers,
        &neati_platform::MockTrashBackend::new(),
        |_| {},
    );
    assert!(
        result.items.iter().all(|item| item.success),
        "{:?}",
        result.items
    );

    for file in BUILD_OUTPUT {
        assert!(
            !home.path().join(DERIVED_DATA).join(file).exists(),
            "{file} should have been removed"
        );
    }
    assert!(
        home.path().join(DERIVED_DATA).is_dir(),
        "the DerivedData root itself stays"
    );
    assert!(home
        .path()
        .join("Library/Developer/Xcode/UserData/IDEEditorInteractivityHistory.plist")
        .is_file());
}

#[test]
fn a_credential_inside_derived_data_still_refuses_the_whole_unit() {
    let home = tempfile::tempdir().unwrap();
    for file in BUILD_OUTPUT {
        write(home.path(), &format!("{DERIVED_DATA}/{file}"));
    }
    write(
        home.path(),
        &format!("{DERIVED_DATA}/App-abc/signing/distribution.p12"),
    );

    let env = environment(home.path());
    let registry = SignatureRegistry::load_embedded_with(&env).unwrap();
    let item = derived_data_item(home.path(), &registry);
    let refusal = SafetyPlanner::create_plan_with_environment(
        std::slice::from_ref(&item),
        &registry,
        &env,
        &OwnerProviderRegistry::new(Vec::new()),
    )
    .expect_err("a credential is never build output");
    assert!(refusal.to_string().contains("credential"), "{refusal}");
    for file in BUILD_OUTPUT {
        assert!(home.path().join(DERIVED_DATA).join(file).is_file());
    }
    assert!(home
        .path()
        .join(DERIVED_DATA)
        .join("App-abc/signing/distribution.p12")
        .is_file());
}

const DEVICE_SUPPORT: &str = "Library/Developer/Xcode/iOS DeviceSupport";

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
    .filter(|item| item.signature_id == "system.xcode_device_support")
    .collect()
}

/// An old symbol set is removed as one named unit; its siblings are their own
/// units and are untouched by a plan that does not name them.
#[test]
fn an_old_device_support_symbol_set_is_removed_as_one_named_unit() {
    let home = tempfile::tempdir().unwrap();
    let old = "17.0 (21A329)";
    let other = "18.1 (22B83)";
    for version in [old, other] {
        write(
            home.path(),
            &format!("{DEVICE_SUPPORT}/{version}/Symbols/usr/lib/libobjc.A.dylib"),
        );
        write(
            home.path(),
            &format!("{DEVICE_SUPPORT}/{version}/Symbols/System/Library/Frameworks/UIKit.framework/UIKit"),
        );
        write(
            home.path(),
            &format!("{DEVICE_SUPPORT}/{version}/Info.plist"),
        );
        // The age rule reads the unit directory as well as its files.
        let aged = SystemTime::now() - Duration::from_secs(90 * 24 * 60 * 60);
        let mut stack = vec![home.path().join(DEVICE_SUPPORT).join(version)];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    stack.push(entry.path());
                }
            }
            fs::File::open(&dir).unwrap().set_modified(aged).unwrap();
        }
    }

    let env = environment(home.path());
    let registry = SignatureRegistry::load_embedded_with(&env).unwrap();
    let items = device_support_items(home.path(), &registry);
    assert_eq!(items.len(), 2, "{items:?}");
    let target = items
        .iter()
        .find(|item| item.path.ends_with(old))
        .expect("the old symbol set is its own unit");
    assert!(
        target.disposition.is_cleanable(),
        "{:?}",
        target.disposition
    );

    let owner_providers = OwnerProviderRegistry::new(Vec::new());
    let plan = SafetyPlanner::create_plan_with_environment(
        std::slice::from_ref(target),
        &registry,
        &env,
        &owner_providers,
    )
    .expect("a symbol set made of dylibs is plannable");
    let result = CleanExecutor::execute(
        plan,
        &env,
        &LifecycleProviderRegistry::new(Vec::new()),
        &owner_providers,
        &neati_platform::MockTrashBackend::new(),
        |_| {},
    );
    assert!(
        result.items.iter().all(|item| item.success),
        "{:?}",
        result.items
    );
    assert!(!home.path().join(DEVICE_SUPPORT).join(old).exists());
    assert!(home
        .path()
        .join(DEVICE_SUPPORT)
        .join(other)
        .join("Symbols/usr/lib/libobjc.A.dylib")
        .is_file());
    assert!(home.path().join(DEVICE_SUPPORT).is_dir());
}
