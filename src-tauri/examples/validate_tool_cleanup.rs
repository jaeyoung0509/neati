//! Manual validation against tools installed in a disposable test home.
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
    let root = PathBuf::from(std::env::args().nth(1).expect("disposable validation root"));
    assert!(root.starts_with("/private/tmp"));
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("neati-cli-validation-"));
    let home = root.join("home");
    let conda = home.join("miniforge3/bin/conda");
    let mise = home.join(".local/bin/mise");
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(&home)
        .with_temp_dir(&root)
        .with_tool("conda", &conda)
        .with_tool("mise", &mise);
    let guard = RunningProcessPolicy::none();
    for kind in [ToolCacheKind::Mise, ToolCacheKind::Conda] {
        let provider = ToolCleanupProvider::native(kind, Arc::new(Idle));
        let observation = provider.scan(&environment, &guard);
        println!("{kind:?} scan {observation:?}");
        assert_eq!(observation.status, ProviderStatus::Ready);
        assert!(!observation.units.is_empty());
        let unit = &observation.units[0];
        let selection = OwnerProviderSelection {
            item_id: format!("fixture.{}", unit.unit_key),
            name: "Fixture".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        let result = provider.execute(&environment, &plan);
        println!("{kind:?} result {result:?}");
        assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
        assert!(provider.scan(&environment, &guard).units.is_empty());
    }
    for preserved in [
        "miniforge3/bin/python",
        "miniforge3/bin/conda",
        ".local/bin/mise",
        ".local/share/mise/installs/fixture/keep",
        ".config/mise/config.toml",
        ".local/state/mise/trusted-configs/keep",
    ] {
        assert!(home.join(preserved).exists(), "preserve {preserved}");
    }
    for removed in [
        "Library/Caches/mise/fixture/payload",
        ".local/state/mise/env-cache/payload",
        ".local/state/mise/task-artifacts/payload",
    ] {
        assert!(!home.join(removed).exists(), "remove {removed}");
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("This manual validation requires macOS tool adapters");
}
