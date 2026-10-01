//! Validate an installed CocoaPods distribution in an explicitly disposable home.
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
    use std::{path::PathBuf, sync::Arc};
    struct Idle;
    impl RunningProcessProbe for Idle {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(vec![])
        }
    }
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("disposable validation root"));
    assert_eq!(std::fs::canonicalize(&root).unwrap(), root);
    assert!(root.parent() == Some(std::path::Path::new("/private/tmp")));
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("neati-cocoapods-validation-"));
    let abi = args.next().expect("installed Ruby ABI, e.g. 2.6.0");
    let version = args.next().unwrap_or_else(|| "1.16.2".into());
    assert!(matches!(version.as_str(), "1.16.2" | "1.17.0"));
    let ruby = args.next().map(PathBuf::from);
    assert!(
        abi.split('.').count() == 3
            && abi
                .split('.')
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    );
    let home = root.join("home");
    let pod = home.join(".gem/ruby").join(abi).join("bin/pod");
    assert!(pod.is_file(), "install the distribution before validation");
    let cache = home.join("Library/Caches/CocoaPods/Pods");
    for relative in [
        "Release/Example/source.m",
        "Specs/Release/Example.podspec.json",
    ] {
        let path = cache.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![42; 8192]).unwrap();
    }
    std::fs::write(cache.join("VERSION"), &version).unwrap();
    let preserved = [
        ".cocoapods/repos/trunk/spec.json",
        ".cocoapods/config.yaml",
        ".netrc",
        "project/Pods/installed.m",
        "project/Podfile",
        "project/Podfile.lock",
        "Library/Caches/CocoaPods/Specs/keep",
        "Library/Caches/CocoaPods/VERSION",
    ];
    for name in preserved {
        let path = home.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"keep").unwrap();
    }
    let launcher_before = std::fs::read(&pod).unwrap();
    let mut environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(&home)
        .with_temp_dir(&root)
        .with_tool("pod", &pod);
    if let Some(ruby) = ruby {
        environment = environment.with_tool("ruby", ruby);
    }
    let provider = ToolCleanupProvider::native(ToolCacheKind::Cocoapods, Arc::new(Idle));
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
    assert!(
        cache.join("Release/Example/source.m").exists(),
        "preview must preserve payloads"
    );
    let outcome = provider.execute(&environment, &plan);
    println!("result: {outcome:?}");
    assert_eq!(outcome.units[0].status, ProviderStatus::Cleaned);
    assert!(!cache.exists());
    assert!(provider.scan(&environment, &guard).units.is_empty());
    assert_eq!(std::fs::read(&pod).unwrap(), launcher_before);
    for name in preserved {
        assert_eq!(std::fs::read(home.join(name)).unwrap(), b"keep");
    }
    println!("Installed distribution, preview and preservation assertions passed.");
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("CocoaPods validation requires the macOS owner adapter");
}
