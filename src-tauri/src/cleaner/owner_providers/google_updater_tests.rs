use super::tests::{fixture, selection, UseState};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn cache(env: &PlatformEnvironment) -> PathBuf {
    let root = env
        .user_home()
        .unwrap()
        .join("Library/Application Support/Google/GoogleUpdater/crx_cache");
    fs::create_dir_all(&root).unwrap();
    let mut bytes = b"Cr24".to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&[42; 8192]);
    let hash = crate::hash::sha256_hex(&bytes);
    fs::write(root.join(&hash), bytes).unwrap();
    fs::write(
        root.join("metadata.json"),
        serde_json::to_vec(&serde_json::json!({"hashes": {hash: {"appid": "com.google.Chrome"}}}))
            .unwrap(),
    )
    .unwrap();
    root
}

fn setup(
    process: Arc<dyn RunningProcessProbe>,
) -> (
    tempfile::TempDir,
    PlatformEnvironment,
    ReviewedCacheProvider,
) {
    let (dir, env, provider) = fixture(ReviewedCacheKind::GoogleUpdaterDownloads, process);
    (
        dir,
        env.with_current_user_id(unsafe { libc::geteuid() }),
        provider,
    )
}

#[test]
fn complete_coupled_download_cache_moves_to_fixture_trash_and_preserves_installed_state() {
    let (dir, env, provider) = setup(Arc::new(UseState(Some(vec![]))));
    let root = cache(&env);
    let sentinel = root.parent().unwrap().join("prefs.json");
    fs::write(&sentinel, b"installed updater state").unwrap();
    let scan = provider.scan(&env, &RunningProcessPolicy::none());
    assert_eq!(scan.units.len(), 1);
    assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
    assert!(scan.units[0].allocated_bytes > 0);
    let plan = provider
        .prepare(
            &env,
            &RunningProcessPolicy::none(),
            &[selection(&scan.units[0])],
        )
        .unwrap();
    assert!(!plan.requires_confirmation);
    assert_eq!(
        plan.deletion_disposition,
        neati_core::domain::cleanup::DeletionDisposition::Trash
    );
    let result = provider.execute(&env, &plan);
    assert_eq!(result.units.len(), 1);
    assert!(!root.exists());
    assert!(dir
        .path()
        .join("fixture-trash/moved/metadata.json")
        .exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"installed updater state");
    provider.execute(&env, &plan);
    assert!(sentinel.exists());
}

#[test]
fn invalid_index_payload_or_extra_state_retains_measured_bytes_without_authority() {
    for mutation in [
        "index",
        "payload",
        "extra",
        "missing",
        "link",
        "executable",
        "shared",
        "permission",
    ] {
        let (dir, env, provider) = setup(Arc::new(UseState(Some(vec![]))));
        let root = cache(&env);
        let payload = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap() != "metadata.json")
            .unwrap();
        match mutation {
            "index" => fs::write(
                root.join("metadata.json"),
                b"{\"hashes\":{},\"future\":true}",
            )
            .unwrap(),
            "payload" => fs::write(&payload, b"not the indexed CRX archive").unwrap(),
            "extra" => fs::write(root.join("credentials.json"), b"sentinel").unwrap(),
            "missing" => fs::remove_file(root.join("metadata.json")).unwrap(),
            "link" => {
                fs::remove_file(&payload).unwrap();
                std::os::unix::fs::symlink(dir.path(), &payload).unwrap();
            }
            "executable" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&payload, fs::Permissions::from_mode(0o755)).unwrap();
            }
            "shared" => fs::hard_link(&payload, dir.path().join("outside-link")).unwrap(),
            "permission" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&payload, fs::Permissions::from_mode(0o666)).unwrap();
            }
            _ => unreachable!(),
        }
        let scan = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(scan.units.len(), 1, "{mutation}");
        assert_eq!(scan.units[0].state, OwnerUnitState::Blocked, "{mutation}");
        assert!(scan.units[0].allocated_bytes > 0, "{mutation}");
        assert!(provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&scan.units[0])]
            )
            .is_err());
        assert!(root.exists());
    }
}

#[test]
fn active_and_unknown_owner_states_preserve_the_complete_observation() {
    for (process, expected) in [
        (Some(vec!["GoogleUpdater".into()]), OwnerUnitState::InUse),
        (None, OwnerUnitState::Blocked),
    ] {
        let (_dir, env, provider) = setup(Arc::new(UseState(process)));
        cache(&env);
        let scan = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(scan.units[0].state, expected);
        assert!(scan.units[0].allocated_bytes > 0);
        assert!(provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&scan.units[0])]
            )
            .is_err());
    }
}

#[test]
fn final_handle_probe_cannot_replace_an_archive_after_hash_validation() {
    struct WritesOnFinalProbe {
        calls: AtomicUsize,
        path: PathBuf,
    }
    impl RunningProcessProbe for WritesOnFinalProbe {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            assert!(guard.open_file_path().is_some());
            if self.calls.fetch_add(1, Ordering::SeqCst) == 5 {
                fs::write(&self.path, b"changed after verification").unwrap();
            }
            Some(vec![])
        }
    }
    let (_dir, env, idle) = setup(Arc::new(UseState(Some(vec![]))));
    let root = cache(&env);
    let payload = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap() != "metadata.json")
        .unwrap();
    let probe = Arc::new(WritesOnFinalProbe {
        calls: AtomicUsize::new(0),
        path: payload,
    });
    let provider = ReviewedCacheProvider::new(
        ReviewedCacheKind::GoogleUpdaterDownloads,
        probe.clone(),
        idle.measuring.clone(),
        idle.trash.clone(),
    );
    let scan = provider.scan(&env, &RunningProcessPolicy::none());
    let plan = provider
        .prepare(
            &env,
            &RunningProcessPolicy::none(),
            &[selection(&scan.units[0])],
        )
        .unwrap();
    provider.execute(&env, &plan);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 6);
    assert_eq!(
        fs::read(&probe.path).unwrap(),
        b"changed after verification"
    );
    assert!(root.exists());
}

#[test]
fn empty_cache_is_omitted_and_outside_selections_never_become_units() {
    let (_dir, env, provider) = setup(Arc::new(UseState(Some(vec![]))));
    let root = cache(&env);
    let scan = provider.scan(&env, &RunningProcessPolicy::none());
    let mut outside = selection(&scan.units[0]);
    outside.path = root.parent().unwrap().into();
    assert!(provider
        .prepare(&env, &RunningProcessPolicy::none(), &[outside])
        .is_err());
    fs::remove_dir_all(&root).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(provider
        .scan(&env, &RunningProcessPolicy::none())
        .units
        .is_empty());
}

#[test]
fn byte_budgets_and_unknown_user_identity_never_authorize_a_cache() {
    for oversized_index in [true, false] {
        let (_dir, env, provider) = setup(Arc::new(UseState(Some(vec![]))));
        let root = cache(&env);
        let path = if oversized_index {
            root.join("metadata.json")
        } else {
            fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.file_name().unwrap() != "metadata.json")
                .unwrap()
        };
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(if oversized_index {
                1024 * 1024 + 1
            } else {
                2 * 1024 * 1024 * 1024 + 1
            })
            .unwrap();
        let scan = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
        assert!(scan.units[0].allocated_bytes > 0);
        assert!(root.exists());
    }
    let (_dir, env, provider) = fixture(
        ReviewedCacheKind::GoogleUpdaterDownloads,
        Arc::new(UseState(Some(vec![]))),
    );
    assert!(env.current_user_id().is_none());
    let root = cache(&env);
    let scan = provider.scan(&env, &RunningProcessPolicy::none());
    assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
    assert!(scan.units[0].allocated_bytes > 0);
    assert!(root.exists());
}
