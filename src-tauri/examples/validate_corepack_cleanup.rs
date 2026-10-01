//! Public Corepack distribution validation in fresh task-owned homes only.
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
        fs,
        os::unix::fs::MetadataExt,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };
    fn copy(source: &Path, target: &Path) {
        let metadata = fs::symlink_metadata(source).unwrap();
        assert!(!metadata.file_type().is_symlink());
        if metadata.is_dir() {
            fs::create_dir_all(target).unwrap();
            for entry in fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                copy(&entry.path(), &target.join(entry.file_name()));
            }
        } else {
            assert!(metadata.is_file());
            fs::copy(source, target).unwrap();
        }
    }
    struct UseProbe {
        home: PathBuf,
        mode: usize,
        armed: AtomicBool,
        calls: AtomicUsize,
    }
    impl RunningProcessProbe for UseProbe {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            if self.armed.load(Ordering::SeqCst) && guard.open_file_path().is_some() {
                assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
                match self.mode {
                    1 => {
                        fs::rename(self.home.join(".cache"), self.home.join("reviewed-cache"))
                            .unwrap();
                        std::os::unix::fs::symlink(
                            self.home.join("unrelated"),
                            self.home.join(".cache"),
                        )
                        .unwrap();
                    }
                    2 => {
                        let marker = self
                            .home
                            .join(".cache/node/corepack/v1/pnpm/10.17.1/.corepack");
                        let bytes = fs::read(&marker).unwrap();
                        let time = fs::metadata(&marker).unwrap().modified().unwrap();
                        fs::rename(&marker, self.home.join("reviewed-marker")).unwrap();
                        fs::write(&marker, bytes).unwrap();
                        fs::File::options()
                            .write(true)
                            .open(marker)
                            .unwrap()
                            .set_modified(time)
                            .unwrap();
                    }
                    3 => {
                        let file = self
                            .home
                            .join(".local/lib/node_modules/corepack/dist/lib/corepack.cjs");
                        let mut bytes = fs::read(&file).unwrap();
                        bytes[0] = b'!';
                        fs::write(file, bytes).unwrap();
                    }
                    4 => return Some(vec!["fixture reader".into()]),
                    5 => return None,
                    _ => {}
                }
            }
            Some(vec![])
        }
    }
    let mut args = std::env::args().skip(1);
    let distribution = PathBuf::from(args.next().expect("verified public Corepack package"));
    let seed = PathBuf::from(args.next().expect("actual Corepack-populated v1 store"));
    let native = PlatformEnvironment::native();
    let node = neati_lib::tooling::resolve_with("node", &native).expect("installed Node runtime");
    for mode in 0..=5 {
        let fixture = tempfile::Builder::new()
            .prefix("neati-corepack-validation-")
            .tempdir_in(native.temp_dir())
            .unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let script = home.join(".local/lib/node_modules/corepack/dist/corepack.js");
        copy(
            &distribution,
            &home.join(".local/lib/node_modules/corepack"),
        );
        let root = home.join(".cache/node/corepack/v1");
        copy(&seed.join("pnpm/10.17.1"), &root.join("pnpm/10.17.1"));
        let preserved = [
            (
                ".cache/node/corepack/lastKnownGood.json",
                b"{\"pnpm\":\"10.17.1\"}".as_slice(),
            ),
            ("project/package.json", b"pinned project"),
            ("project/pnpm-lock.yaml", b"keep"),
            (".local/bin/corepack", b"shim"),
            (".npmrc", b"auth sentinel"),
            ("unrelated/node/corepack/v1/private-state", b"keep"),
        ];
        for (name, bytes) in preserved {
            let path = home.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home(&home)
            .with_temp_dir(&home)
            .with_current_user_id(fs::metadata(&home).unwrap().uid())
            .with_tool("node", &node)
            .with_tool("corepack", &script);
        let probe = Arc::new(UseProbe {
            home: home.clone(),
            mode,
            armed: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
        });
        let provider = ToolCleanupProvider::native(ToolCacheKind::Corepack, probe.clone());
        let guard =
            RunningProcessPolicy::guarding(vec!["node".into(), "corepack".into(), "pnpm".into()]);
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.status, ProviderStatus::Ready, "{scan:?}");
        assert_eq!(scan.units.len(), 1);
        let unit = &scan.units[0];
        assert_eq!(unit.path, root);
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &[OwnerProviderSelection {
                    item_id: format!("fixture.{}", unit.unit_key),
                    name: "offline package-manager distributions".into(),
                    path: unit.path.clone(),
                    expected_bytes: unit.allocated_bytes,
                }],
            )
            .unwrap();
        let offline = || {
            let mut command = std::process::Command::new(&node);
            command
                .arg(&script)
                .args(["pnpm@10.17.1", "--version"])
                .env_clear()
                .env("HOME", &home)
                .env("COREPACK_HOME", root.parent().unwrap())
                .env("COREPACK_ENABLE_NETWORK", "0")
                .env("COREPACK_ENABLE_AUTO_PIN", "0")
                .env("COREPACK_DEFAULT_TO_LATEST", "0")
                .env("NODE_DISABLE_COMPILE_CACHE", "1")
                .current_dir(&home);
            neati_platform::subprocess::run_with_timeout(
                command,
                std::time::Duration::from_secs(30),
            )
            .unwrap()
        };
        if mode == 0 {
            let output = offline();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, b"10.17.1\n");
        }
        probe.armed.store(mode != 0, Ordering::SeqCst);
        let result = provider.execute(&environment, &plan);
        assert_eq!(
            result.units[0].status,
            if mode == 0 {
                ProviderStatus::Cleaned
            } else {
                ProviderStatus::Blocked
            },
            "mode {mode}: {result:?}"
        );
        if mode == 0 {
            assert!(!root.exists());
            assert!(provider.scan(&environment, &guard).units.is_empty());
            let output = offline();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("Network access disabled"));
            assert!(provider.scan(&environment, &guard).units.is_empty());
        } else {
            assert_eq!(result.reclaimed_bytes(), 0);
            assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
            let kept = if mode == 1 {
                home.join("reviewed-cache/node/corepack/v1")
            } else {
                root.clone()
            };
            assert!(kept.join("pnpm/10.17.1/.corepack").exists());
        }
        for (name, bytes) in preserved {
            let path = if mode == 1 && name.starts_with(".cache/") {
                home.join("reviewed-cache")
                    .join(name.strip_prefix(".cache/").unwrap())
            } else {
                home.join(name)
            };
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        println!("Corepack0.36.0/Node26.7.0 fixture mode {mode}: scan/prepare/final scope/result/preservation passed; reclaimed {} allocated bytes; offline reuse checked only in mode0; native process/handle port is a fixture",result.reclaimed_bytes());
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("Corepack owner validation requires macOS");
}
