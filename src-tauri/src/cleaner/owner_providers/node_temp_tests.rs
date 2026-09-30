//! Unix adapter fixtures: real UID/mode/link semantics; no real cache cleanup.
use super::tests::{fixture, selection, UseState};
use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

const PAYLOAD: &[u8] =
    include_bytes!("../../../../crates/neati-core/tests/fixtures/node-26.7.0-arm64-cache.bin");
fn cache(env: &PlatformEnvironment, temp: &Path) -> PathBuf {
    let unit = temp.join("node-compile-cache").join(format!(
        "v26.7.0-arm64-8d7ad2ee-{}",
        env.current_user_id().unwrap()
    ));
    fs::create_dir_all(&unit).unwrap();
    fs::write(unit.join("ae127978"), PAYLOAD).unwrap();
    unit
}
fn node_fixture(
    process: Arc<dyn RunningProcessProbe>,
) -> (
    tempfile::TempDir,
    PlatformEnvironment,
    ReviewedCacheProvider,
) {
    let (dir, env, provider) = fixture(ReviewedCacheKind::NodeCompileCache, process);
    let root = dir.path().canonicalize().unwrap();
    let env = env
        .with_temp_dir(root.join("user-temp"))
        .with_shared_temp_dir(root.join("shared-temp"))
        .with_current_user_id(unsafe { libc::geteuid() });
    (dir, env, provider)
}
fn aged_port<'a>(
    env: &'a PlatformEnvironment,
    provider: &'a ReviewedCacheProvider,
) -> NativePort<'a> {
    NativePort {
        provider,
        environment: env,
        now: SystemTime::now() + Duration::from_secs(4 * 86400),
    }
}

#[test]
fn verified_node_cache_moves_to_fixture_trash_without_reaching_workspaces() {
    let (_dir, env, provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
    let unit = cache(&env, &env.temp_dir());
    let outside = env.temp_dir().join("neati-active-pr/.git/index");
    super::tests::file(&outside);
    let port = aged_port(&env, &provider);
    let scan = port.inventory(ReviewedCacheKind::NodeCompileCache);
    assert_eq!(scan.units.len(), 1);
    assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
    assert!(scan.units[0].allocated_bytes > 0);
    let plan = reviewed_cache::prepare(
        ReviewedCacheKind::NodeCompileCache,
        &port,
        &[selection(&scan.units[0])],
    )
    .unwrap();
    assert!(!plan.requires_confirmation);
    let result = reviewed_cache::execute(ReviewedCacheKind::NodeCompileCache, &port, &plan);
    assert_eq!(result.units.len(), 1);
    assert!(!unit.exists());
    assert!(outside.exists());
    reviewed_cache::execute(ReviewedCacheKind::NodeCompileCache, &port, &plan);
    assert!(outside.exists());
}

#[test]
fn shared_and_user_roots_are_distinct_but_equal_roots_are_observed_once() {
    let (_dir, env, provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
    cache(&env, &env.temp_dir());
    let shared = env
        .temporary_roots()
        .into_iter()
        .find(|p| p != &env.temp_dir())
        .unwrap();
    cache(&env, &shared);
    let scan = aged_port(&env, &provider).inventory(ReviewedCacheKind::NodeCompileCache);
    assert_eq!(scan.units.len(), 2);
    let same = env.clone().with_shared_temp_dir(env.temp_dir());
    assert_eq!(
        aged_port(&same, &provider)
            .inventory(ReviewedCacheKind::NodeCompileCache)
            .units
            .len(),
        1
    );
}

#[test]
fn temporary_observer_defers_only_the_exact_node_namespace_and_keeps_workspaces() {
    use crate::scanner::DirectoryScanner;
    use crate::signatures::SignatureRegistry;
    let (_dir, env, _provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
    cache(&env, &env.temp_dir());
    let workspace = env.temp_dir().join("neati-active-pr/.git/index");
    let lookalike = env.temp_dir().join("node-compile-cache-project/source.txt");
    super::tests::file(&workspace);
    super::tests::file(&lookalike);
    let registry = SignatureRegistry::load_embedded().unwrap();
    let signature = registry.get("system.developer_temp").unwrap();
    let items = DirectoryScanner::scan_signature(
        signature,
        &env,
        &crate::models::NeverCancelled,
        &crate::applications::RunningApplications::default(),
    );
    assert!(items.iter().any(|item| item.path
        == workspace
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()));
    assert!(items
        .iter()
        .any(|item| item.path == lookalike.parent().unwrap().to_string_lossy()));
    assert!(!items
        .iter()
        .any(|item| item.path == env.temp_dir().join("node-compile-cache").to_string_lossy()));
    assert!(items
        .iter()
        .all(|item| !item.is_selected && !item.allows_cleanup() && item.cleanable_bytes() == 0));
    assert!(workspace.exists());
    assert!(lookalike.exists());
}

#[test]
fn unknown_versions_and_worktrees_keep_observed_bytes_and_never_authorize() {
    let (_dir, env, provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
    let unit = cache(&env, &env.temp_dir());
    let unknown = unit.parent().unwrap().join("v27.0.0-arm64-unknown-501");
    super::tests::file(&unknown.join("source-project/.git/index"));
    let port = aged_port(&env, &provider);
    let scan = port.inventory(ReviewedCacheKind::NodeCompileCache);
    let kept = scan.units.iter().find(|u| u.path == unknown).unwrap();
    assert_eq!(kept.state, OwnerUnitState::Advisory);
    assert!(kept.allocated_bytes > 0);
    assert!(reviewed_cache::prepare(
        ReviewedCacheKind::NodeCompileCache,
        &port,
        &[selection(kept)]
    )
    .is_err());
    super::tests::file(&unit.join(".git/index"));
    let changed = port.inventory(ReviewedCacheKind::NodeCompileCache);
    let blocked = changed.units.iter().find(|u| u.path == unit).unwrap();
    assert_eq!(blocked.state, OwnerUnitState::Blocked);
    assert!(blocked.allocated_bytes > 0);
    assert!(reviewed_cache::prepare(
        ReviewedCacheKind::NodeCompileCache,
        &port,
        &[selection(blocked)]
    )
    .is_err());
    assert!(unknown.join("source-project/.git/index").exists());
}

#[test]
fn young_unknown_use_foreign_identity_and_links_are_not_ready() {
    let (_dir, env, provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
    let unit = cache(&env, &env.temp_dir());
    let young = NativePort {
        provider: &provider,
        environment: &env,
        now: SystemTime::now(),
    };
    assert_eq!(
        young.inventory(ReviewedCacheKind::NodeCompileCache).units[0].state,
        OwnerUnitState::Recent
    );
    let unknown = ReviewedCacheProvider::new(
        ReviewedCacheKind::NodeCompileCache,
        Arc::new(UseState(None)),
        provider.measuring.clone(),
        provider.trash.clone(),
    );
    assert_eq!(
        aged_port(&env, &unknown)
            .inventory(ReviewedCacheKind::NodeCompileCache)
            .units[0]
            .state,
        OwnerUnitState::Blocked
    );
    let foreign = env
        .clone()
        .with_current_user_id(env.current_user_id().unwrap() + 1);
    let foreign_named = unit.parent().unwrap().join(format!(
        "v26.7.0-arm64-8d7ad2ee-{}",
        foreign.current_user_id().unwrap()
    ));
    fs::rename(&unit, &foreign_named).unwrap();
    assert!(super::super::node_temp::inspect(&foreign, &foreign_named).is_err());
    fs::rename(&foreign_named, &unit).unwrap();
    let outside = env.temp_dir().join("outside");
    super::tests::file(&outside);
    symlink(&outside, unit.join("0123abcd")).unwrap();
    let scan = aged_port(&env, &provider).inventory(ReviewedCacheKind::NodeCompileCache);
    assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
    assert!(outside.exists());
}

#[test]
fn malformed_payload_executable_and_shared_permissions_revoke_prepared_cleanup() {
    for alteration in 0..3 {
        let (_dir, env, provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
        let unit = cache(&env, &env.temp_dir());
        let port = aged_port(&env, &provider);
        let scan = port.inventory(ReviewedCacheKind::NodeCompileCache);
        let plan = reviewed_cache::prepare(
            ReviewedCacheKind::NodeCompileCache,
            &port,
            &[selection(&scan.units[0])],
        )
        .unwrap();
        match alteration {
            0 => {
                let mut bad = PAYLOAD.to_vec();
                bad[24] ^= 1;
                fs::write(unit.join("ae127978"), bad).unwrap();
            }
            1 => fs::set_permissions(unit.join("ae127978"), fs::Permissions::from_mode(0o755))
                .unwrap(),
            _ => fs::set_permissions(&unit, fs::Permissions::from_mode(0o777)).unwrap(),
        }
        reviewed_cache::execute(ReviewedCacheKind::NodeCompileCache, &port, &plan);
        assert!(unit.exists(), "alteration {alteration}");
    }
}

#[test]
fn recent_descendant_and_replaced_unit_revoke_old_plans() {
    let (_dir, env, provider) = node_fixture(Arc::new(UseState(Some(vec![]))));
    let unit = cache(&env, &env.temp_dir());
    let scan = aged_port(&env, &provider).inventory(ReviewedCacheKind::NodeCompileCache);
    let selected = selection(&scan.units[0]);
    let plan = reviewed_cache::prepare(
        ReviewedCacheKind::NodeCompileCache,
        &aged_port(&env, &provider),
        &[selected],
    )
    .unwrap();
    fs::write(unit.join("0123abcd"), PAYLOAD).unwrap();
    fs::File::options()
        .write(true)
        .open(unit.join("0123abcd"))
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(3 * 86400)),
        )
        .unwrap();
    assert_eq!(
        aged_port(&env, &provider)
            .inventory(ReviewedCacheKind::NodeCompileCache)
            .units[0]
            .state,
        OwnerUnitState::Recent
    );
    reviewed_cache::execute(
        ReviewedCacheKind::NodeCompileCache,
        &aged_port(&env, &provider),
        &plan,
    );
    assert!(unit.exists());
    fs::rename(&unit, unit.with_extension("preserved")).unwrap();
    cache(&env, &env.temp_dir());
    reviewed_cache::execute(
        ReviewedCacheKind::NodeCompileCache,
        &aged_port(&env, &provider),
        &plan,
    );
    assert!(unit.exists());
    assert!(unit.with_extension("preserved").exists());
}

#[test]
fn owner_restart_at_final_probe_protects_the_verified_node_unit() {
    struct Restart(std::sync::atomic::AtomicUsize);
    impl RunningProcessProbe for Restart {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            let count = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(if count >= 5 {
                vec!["node".into()]
            } else {
                vec![]
            })
        }
    }
    let (_dir, env, provider) =
        node_fixture(Arc::new(Restart(std::sync::atomic::AtomicUsize::new(0))));
    let unit = cache(&env, &env.temp_dir());
    let port = aged_port(&env, &provider);
    let scan = port.inventory(ReviewedCacheKind::NodeCompileCache);
    let plan = reviewed_cache::prepare(
        ReviewedCacheKind::NodeCompileCache,
        &port,
        &[selection(&scan.units[0])],
    )
    .unwrap();
    reviewed_cache::execute(ReviewedCacheKind::NodeCompileCache, &port, &plan);
    assert!(unit.exists());
}
