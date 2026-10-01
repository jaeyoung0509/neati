//! Installed-distribution validation. Every mutable path is created inside a
//! fresh disposable home; no user cache, authentication or VM is changed.
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
    use std::{
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };
    struct IdleFixture;
    impl RunningProcessProbe for IdleFixture {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(vec![])
        }
    }
    let native = PlatformEnvironment::native();
    let executable =
        neati_lib::tooling::resolve_with("gh", &native).expect("installed GitHub CLI is required");
    let fixture = tempfile::Builder::new()
        .prefix("neati-gh-validation-")
        .tempdir_in(native.temp_dir())
        .unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let root = home.join(".cache/gh");
    let payload = root.join("ab/cd").join("0".repeat(60));
    std::fs::create_dir_all(payload.parent().unwrap()).unwrap();
    std::fs::write(&payload, vec![42; 8192]).unwrap();
    std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o600)).unwrap();
    let preserved = [
        ".config/gh/config.yml",
        ".config/gh/hosts.yml",
        ".local/share/gh/extensions/gh-fixture",
        ".local/state/gh/state",
        ".cache/unrelated/payload",
    ];
    for name in preserved {
        let path = home.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"outside-scope sentinel").unwrap();
    }
    let env = PlatformEnvironment::simulated(PathFlavor::Posix)
        .with_home(&home)
        .with_tool("gh", &executable)
        .with_current_user_id(std::fs::metadata(&home).unwrap().uid())
        .with_temp_dir(&home);
    let provider = ToolCleanupProvider::native(ToolCacheKind::GithubCli, Arc::new(IdleFixture));
    let guard = RunningProcessPolicy::guarding(vec!["gh".into()]);
    let scan = provider.scan(&env, &guard);
    assert_eq!(scan.status, ProviderStatus::Ready, "{scan:?}");
    assert_eq!(scan.units.len(), 1);
    let unit = &scan.units[0];
    assert_eq!(unit.path, root);
    let plan = provider
        .prepare(
            &env,
            &guard,
            &[OwnerProviderSelection {
                item_id: format!("fixture.{}", unit.unit_key),
                name: "fixture local HTTP cache".into(),
                path: unit.path.clone(),
                expected_bytes: unit.allocated_bytes,
            }],
        )
        .unwrap();
    let result = provider.execute(&env, &plan);
    assert_eq!(
        result.units[0].status,
        ProviderStatus::Cleaned,
        "{result:?}"
    );
    assert!(!root.exists());
    for name in preserved {
        assert_eq!(
            std::fs::read(home.join(name)).unwrap(),
            b"outside-scope sentinel"
        );
    }
    println!("gh 2.83.1: production scan/prepare/execute/post-check passed; {} fixture allocated bytes removed; five outside-scope sentinels preserved; idle process/handle port was a fixture",result.reclaimed_bytes());
    struct ChangeAtFinalUse {
        home: PathBuf,
        ancestor_link: bool,
        armed: AtomicBool,
        handle_calls: AtomicUsize,
    }
    impl RunningProcessProbe for ChangeAtFinalUse {
        fn running(&self, policy: &RunningProcessPolicy) -> Option<Vec<String>> {
            if self.armed.load(Ordering::SeqCst) && policy.open_file_path().is_some() {
                assert_eq!(self.handle_calls.fetch_add(1, Ordering::SeqCst), 0);
                if self.ancestor_link {
                    std::fs::rename(self.home.join(".cache"), self.home.join("reviewed-cache"))
                        .unwrap();
                    std::os::unix::fs::symlink(
                        self.home.join("unrelated"),
                        self.home.join(".cache"),
                    )
                    .unwrap();
                } else {
                    let file = self.home.join(".cache/gh/ab/cd").join("0".repeat(60));
                    std::fs::rename(&file, self.home.join("reviewed-payload")).unwrap();
                    std::fs::write(&file, vec![42; 8192]).unwrap();
                    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
                        .unwrap();
                }
            }
            Some(vec![])
        }
    }
    for ancestor_link in [false, true] {
        let fixture = tempfile::Builder::new()
            .prefix("neati-gh-race-validation-")
            .tempdir_in(native.temp_dir())
            .unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let root = home.join(".cache/gh");
        let file = root.join("ab/cd").join("0".repeat(60));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, vec![42; 8192]).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let outside = home.join("unrelated/gh/private-state");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, b"outside-scope sentinel").unwrap();
        let env = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home(&home)
            .with_tool("gh", &executable)
            .with_current_user_id(std::fs::metadata(&home).unwrap().uid())
            .with_temp_dir(&home);
        let process = Arc::new(ChangeAtFinalUse {
            home: home.clone(),
            ancestor_link,
            armed: AtomicBool::new(false),
            handle_calls: AtomicUsize::new(0),
        });
        let provider = ToolCleanupProvider::native(ToolCacheKind::GithubCli, process.clone());
        let scan = provider.scan(&env, &guard);
        let unit = &scan.units[0];
        let plan = provider
            .prepare(
                &env,
                &guard,
                &[OwnerProviderSelection {
                    item_id: format!("fixture.{}", unit.unit_key),
                    name: "fixture local HTTP cache".into(),
                    path: unit.path.clone(),
                    expected_bytes: unit.allocated_bytes,
                }],
            )
            .unwrap();
        process.armed.store(true, Ordering::SeqCst);
        let result = provider.execute(&env, &plan);
        assert_eq!(
            result.units[0].status,
            ProviderStatus::Blocked,
            "{result:?}"
        );
        assert_eq!(result.reclaimed_bytes(), 0);
        assert_eq!(process.handle_calls.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside-scope sentinel");
        let kept = if ancestor_link {
            home.join("reviewed-cache/gh/ab/cd").join("0".repeat(60))
        } else {
            file
        };
        assert_eq!(std::fs::read(kept).unwrap(), vec![42; 8192]);
    }
    println!("gh 2.83.1: final use-check callback ancestor link and equal-size descendant replacements both blocked before command launch; fixture cache and unrelated state preserved");
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("GitHub CLI owner validation is macOS-only");
}
