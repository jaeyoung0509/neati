//! Validate an official mise binary through the production owner adapter.
//! Every mutable path is created in a fresh task-owned home; no global install.
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
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    struct UseProbe {
        mode: usize,
        file: PathBuf,
        saved: PathBuf,
        armed: AtomicBool,
    }
    impl RunningProcessProbe for UseProbe {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            if guard.open_file_path().is_some() && self.armed.swap(false, Ordering::SeqCst) {
                match self.mode {
                    3 => {
                        let bytes = fs::read(&self.file).unwrap();
                        let modified = fs::metadata(&self.file).unwrap().modified().unwrap();
                        fs::rename(&self.file, &self.saved).unwrap();
                        fs::write(&self.file, bytes).unwrap();
                        fs::File::options()
                            .write(true)
                            .open(&self.file)
                            .unwrap()
                            .set_modified(modified)
                            .unwrap();
                    }
                    4 => return Some(vec!["fixture cache reader".into()]),
                    5 => return None,
                    _ => {}
                }
            }
            Some(Vec::new())
        }
    }

    let binary = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("official mise 2026.9.18 binary"),
    );
    let bytes = fs::read(binary).unwrap();
    assert_eq!(
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "484c135bd4329975d608d3f77e26c2ece5d2f5590f18ca71f44440294f8cfa6f"
    );
    let native = PlatformEnvironment::native();
    for mode in 0..=5 {
        let fixture = tempfile::Builder::new()
            .prefix("neati-mise-owner-validation-")
            .tempdir_in(native.temp_dir())
            .unwrap();
        let home = fixture.path().canonicalize().unwrap();
        let executable = home.join(".local/bin/mise");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, &bytes).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let cache = home.join("Library/Caches/mise");
        let state = home.join(".local/state/mise");
        let data = home.join(".local/share/mise");
        let config = home.join(".config/mise");
        let external = home.join(".cache/task-output");
        let task = if mode == 1 {
            external.join("v2")
        } else {
            cache.join("task-artifacts/v2")
        };
        let payloads = [
            cache.join("fixture-tool/nested/metadata.msgpack"),
            task.join("fixture-task/manifest.json"),
            state.join("env-cache/fixture.env"),
            state.join("task-artifacts/fixture-task/state.json"),
        ];
        for path in &payloads {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, vec![3; 4096]).unwrap();
        }
        let configuration = config.join("config.toml");
        let protected = [
            (
                data.join("installs/fixture-tool/1.0/bin/tool"),
                b"installed tool".as_slice(),
            ),
            (configuration.clone(), b"[settings]\nexperimental = true\n"),
            (state.join("trusted-configs/fixture"), b"trust sentinel"),
            (home.join("project/output.txt"), b"authored project output"),
        ];
        for (path, content) in &protected {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let mut environment = PlatformEnvironment::simulated(PathFlavor::current())
            .with_home(&home)
            .with_temp_dir(&home)
            .with_tool("mise", &executable);
        for (name, path) in [
            ("MISE_CACHE_DIR", &cache),
            ("MISE_STATE_DIR", &state),
            ("MISE_DATA_DIR", &data),
            ("MISE_CONFIG_DIR", &config),
            ("MISE_CONFIG_FILE", &configuration),
            ("MISE_GLOBAL_CONFIG_FILE", &configuration),
        ] {
            environment = environment.with_cache_path_override(name, path);
        }
        if mode == 1 {
            environment = environment.with_cache_path_override("MISE_TASK_CACHE_DIR", &external);
        }
        let probe = Arc::new(UseProbe {
            mode,
            file: payloads[0].clone(),
            saved: home.join("reviewed-payload"),
            armed: AtomicBool::new(false),
        });
        let provider = ToolCleanupProvider::native(ToolCacheKind::Mise, probe.clone());
        let guard = RunningProcessPolicy::guarding(vec!["mise".into()]);
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.status, ProviderStatus::Ready, "{scan:?}");
        assert_eq!(scan.units.len(), 1);
        let unit = &scan.units[0];
        assert_eq!(unit.entry_count, if mode == 1 { 4 } else { 3 });
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &[OwnerProviderSelection {
                    item_id: format!("mise.{}", unit.unit_key),
                    name: "mise fixture".into(),
                    path: unit.path.clone(),
                    expected_bytes: unit.allocated_bytes,
                }],
            )
            .unwrap();
        if mode == 2 {
            let modified = fs::metadata(&payloads[0]).unwrap().modified().unwrap();
            fs::rename(&payloads[0], home.join("reviewed-payload")).unwrap();
            fs::write(&payloads[0], vec![3; 4096]).unwrap();
            fs::File::options()
                .write(true)
                .open(&payloads[0])
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }
        probe.armed.store(true, Ordering::SeqCst);
        let result = provider.execute(&environment, &plan);
        if mode <= 1 {
            assert_eq!(
                result.units[0].status,
                ProviderStatus::Cleaned,
                "{:?}",
                result.units[0]
            );
            assert!(result.reclaimed_bytes() >= 4 * 4096);
            assert!(payloads.iter().all(|path| !path.exists()));
        } else {
            assert_eq!(
                result.units[0].status,
                ProviderStatus::Blocked,
                "{:?}",
                result.units[0]
            );
            assert_eq!(result.reclaimed_bytes(), 0);
            assert!(payloads.iter().all(|path| path.exists()));
        }
        for (path, content) in &protected {
            assert_eq!(fs::read(path).unwrap(), *content);
        }
        assert_eq!(fs::read(&executable).unwrap(), bytes);
        println!("mise2026.9.18 production adapter fixture mode {mode}: complete scope/result/protected preservation passed; permanent removed bytes {}; process and handle ports are fixtures", result.reclaimed_bytes());
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("Official macOS mise owner validation requires macOS");
}
