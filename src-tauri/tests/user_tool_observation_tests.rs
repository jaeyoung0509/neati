use neati_lib::applications::RunningApplications;
use neati_lib::cleaner::{LifecycleProviderRegistry, OwnerProviderRegistry};
use neati_lib::models::{
    CacheManagementMode, CacheSizeSemantics, CancellationProbe, Category, CleanFailureReason,
    CleanStrategy, EligibilityGate, NeatiError, NeverCancelled, ObservationQuality, PlatformKind,
    RiskTier, ScanGapKind, ScanItem,
};
use neati_lib::safety::SafetyPlanner;
use neati_lib::scanner::{
    DirectoryScanner, NoRootProgress, ScanEngine, ScanLimits, TraversalCounters, WalkContext,
};
use neati_lib::signatures::SignatureRegistry;
use neati_platform::{path_algebra, PathFlavor, PlatformEnvironment};
use std::fs;
use std::path::Path;

const IDS: [&str; 2] = ["dev.oh_my_zsh.cache", "dev.github_cli.http_cache"];

#[test]
fn composition_snapshot_shell_overrides_are_bounded_advisory_and_do_not_execute_configuration() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let custom = home.join(".local/omz-cache");
    let installation = home.join(".local/oh-my-zsh");
    let env = environment(&home)
        .with_cache_path_override("ZSH_CACHE_DIR", &custom)
        .with_cache_path_override("ZSH", &installation);
    write(&custom.join("completions/_generated"));
    write(&installation.join("cache/.zsh-update"));
    write(&home.join(".zshrc"));
    let registry = catalog();
    let items = observe(&registry, IDS[0], &env);
    let observed: Vec<_> = items.iter().filter(|item| item.exists).collect();
    assert_eq!(observed.len(), 2);
    assert!(observed.iter().all(|item| {
        Path::new(&item.path) == custom || Path::new(&item.path) == installation.join("cache")
    }));
    for item in &items {
        assert_advisory(item);
    }
    // DirectoryScanner retains absent fixed-root facts; ScanEngine drops them
    // from visible observations. They never become bytes or authority.
    let absent: Vec<_> = items.iter().filter(|item| !item.exists).collect();
    assert_eq!(absent.len(), 2);
    for item in absent {
        assert_eq!(item.size.logical, 0);
        assert_eq!(item.file_count, 0);
        assert!(
            Path::new(&item.path) == home.join(".oh-my-zsh/cache")
                || Path::new(&item.path) == home.join(".cache/oh-my-zsh")
        );
    }
    for broad in [
        &home,
        &home.join("Documents"),
        &home.join(".config/gh"),
        Path::new("relative"),
    ] {
        assert!(env
            .clone()
            .with_cache_path_override("ZSH_CACHE_DIR", broad)
            .expand_placeholder("${ZSH_CACHE_DIR}")
            .is_none());
    }
    assert!(home.join(".zshrc").exists());
}

#[test]
fn additional_named_browser_and_tool_scopes_preserve_adjacent_state_and_never_authorize_cleanup() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let env = environment(&home);
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let cases = [
        (
            "system.arc.profile_cache_observations",
            "Library/Application Support/Arc/User Data/Default/Cache/response",
        ),
        (
            "system.arc.profile_cache_observations",
            "Library/Application Support/Arc/Profile 1/Code Cache/response",
        ),
        (
            "system.arc.profile_cache_observations",
            "Library/Application Support/company.thebrowser.Browser/Profile 2/GPUCache/response",
        ),
        (
            "system.chrome_devtools.profile_cache_observations",
            ".cache/chrome-devtools-mcp/chrome-profile/Default/Code Cache/data",
        ),
        (
            "system.chrome_devtools.profile_cache_observations",
            ".cache/chrome-devtools-mcp/chrome-profile-beta/Profile 1/Cache/data",
        ),
        (
            "system.chrome_devtools.profile_cache_observations",
            ".cache/chrome-devtools-mcp/chrome-profile-canary/Default/GPUCache/data",
        ),
        (
            "system.chrome_devtools.profile_cache_observations",
            ".cache/chrome-devtools-mcp/chrome-profile-dev/Default/Cache/data",
        ),
        (
            "dev.kubernetes.cached_metadata",
            ".kube/cache/http/metadata",
        ),
        (
            "dev.precommit.environment_observation",
            ".cache/pre-commit/repo/environment/bin/python",
        ),
        (
            "dev.pyenv.download_observation",
            ".pyenv/cache/python.tar.gz",
        ),
        (
            "dev.pyinstaller.binary_observation",
            "Library/Application Support/pyinstaller/bincache/executable",
        ),
    ];
    for (id, relative) in cases {
        write(&home.join(relative));
        let items = observe(&registry, id, &env);
        assert!(!items.is_empty(), "{id}");
        for item in items {
            assert_advisory(&item);
            if item.exists && item.observed_bytes() > 0 {
                let mut forged = item.clone();
                forged.is_selected = true;
                forged.risk = RiskTier::Rebuild;
                forged.cache_metadata.management_mode = CacheManagementMode::Neati;
                forged.cache_metadata.size_semantics = CacheSizeSemantics::PhysicalReclaimable;
                forged.disposition = forged.derive_disposition();
                assert!(matches!(SafetyPlanner::create_plan_with_environment(
                    &[forged], &registry, &env, &OwnerProviderRegistry::new(vec![]) ),
                    Err(NeatiError::RefusedSelection(ref refused)) if refused.len() == 1 && refused[0].reason == CleanFailureReason::OwnerManaged));
            }
        }
    }
    for relative in [
        "Library/Application Support/Arc/User Data/Default/Cookies",
        "Library/Application Support/Arc/User Data/Default/Service Worker/ScriptCache/keep",
        ".cache/chrome-devtools-mcp/chrome-profile/Default/IndexedDB/keep",
        ".cache/chrome-devtools-mcp/chrome-profile-custom/Default/Cache/keep",
        ".kube/config",
        ".pyenv/versions/3.14/bin/python",
    ] {
        let path = home.join(relative);
        write(&path);
        for id in [
            "system.arc.profile_cache_observations",
            "system.chrome_devtools.profile_cache_observations",
            "dev.kubernetes.cached_metadata",
            "dev.pyenv.download_observation",
        ] {
            assert!(
                !registry.path_is_in_scope(registry.get(id).unwrap(), &path, &env),
                "{id}: {relative}"
            );
        }
        assert!(path.exists());
    }
}

fn environment(home: &Path) -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_home(home)
}

fn write(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![b'x'; 8192]).unwrap();
}

fn catalog() -> SignatureRegistry {
    let embedded = SignatureRegistry::load_embedded_catalog().unwrap();
    let mut registry = SignatureRegistry::new();
    for id in IDS {
        registry.register(embedded.get(id).unwrap().clone());
    }
    registry
}

fn observe(registry: &SignatureRegistry, id: &str, env: &PlatformEnvironment) -> Vec<ScanItem> {
    DirectoryScanner::scan_signature(
        registry.get(id).unwrap(),
        env,
        &NeverCancelled,
        &RunningApplications::default(),
    )
}

fn assert_advisory(item: &ScanItem) {
    assert_eq!(item.risk, RiskTier::Manual);
    assert_eq!(
        item.cache_metadata.management_mode,
        CacheManagementMode::Advisory
    );
    assert!(!item.allows_cleanup());
    assert!(!item.is_selected);
    assert_eq!(item.cleanable_bytes(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn verified_github_owner_supersedes_the_same_advisory_namespace_without_double_counting() {
    use neati_core::domain::cleanup::{OwnerProviderAuthorization, OwnerProviderExecution};
    use neati_lib::cleaner::OwnerScopedProvider;
    use neati_lib::models::{
        OwnerProviderRefusal, OwnerProviderSelection, OwnerStoreObservation, OwnerUnitObservation,
        RunningProcessPolicy,
    };
    use std::{os::unix::fs::MetadataExt, sync::Arc};
    struct ObservedOwner;
    impl OwnerScopedProvider for ObservedOwner {
        fn id(&self) -> &'static str {
            "github_cli.local_cache_clear"
        }
        fn platforms(&self) -> &'static [PlatformKind] {
            &[PlatformKind::Macos]
        }
        fn consequence(&self) -> &'static str {
            "Fixture whole-owner operation"
        }
        fn requires_confirmation(&self) -> bool {
            true
        }
        fn scan(
            &self,
            env: &PlatformEnvironment,
            _: &RunningProcessPolicy,
        ) -> OwnerStoreObservation {
            let root = env.user_home().unwrap().join(".cache/gh");
            let allocated = fs::metadata(root.join("payload")).unwrap().blocks() * 512;
            OwnerStoreObservation::ready(
                Some(root.clone()),
                vec![OwnerUnitObservation::ready(
                    "fixture-complete-scope",
                    root,
                    8192,
                    allocated,
                    1,
                )],
            )
        }
        fn prepare(
            &self,
            _: &PlatformEnvironment,
            _: &RunningProcessPolicy,
            _: &[OwnerProviderSelection],
        ) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
            panic!("Observation accounting never prepares an operation")
        }
        fn execute(
            &self,
            _: &PlatformEnvironment,
            _: &OwnerProviderAuthorization,
        ) -> OwnerProviderExecution {
            panic!("Observation accounting never executes an operation")
        }
    }
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let payload = home.join(".cache/gh/payload");
    write(&payload);
    let allocated = fs::metadata(&payload).unwrap().blocks() * 512;
    let embedded = SignatureRegistry::load_embedded_catalog().unwrap();
    let mut registry = catalog();
    registry.register(
        embedded
            .get("dev.github_cli.local_cache_clear")
            .unwrap()
            .clone(),
    );
    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(vec![Arc::new(ObservedOwner)]),
        Some(&[Category::Developer]),
        &[],
        false,
        &environment(&home),
        &NeverCancelled,
        |_| {},
    );
    assert_eq!(result.categories[0].items.len(), 1);
    let item = &result.categories[0].items[0];
    assert_eq!(item.signature_id, "dev.github_cli.local_cache_clear");
    assert!(item.allows_cleanup());
    assert!(!item.is_selected);
    assert!(item.requires_confirmation);
    assert_eq!(result.total_bytes, allocated);
    assert_eq!(result.cleanable_bytes, allocated);
    assert_eq!(result.ambiguous_overlap_bytes, 0);
    assert_eq!(fs::read(payload).unwrap(), vec![b'x'; 8192]);
}

#[test]
fn user_tool_catalog_names_only_vendor_cache_namespaces() {
    let registry = catalog();
    assert_eq!(
        registry.get(IDS[0]).unwrap().paths,
        [
            "~/.oh-my-zsh/cache",
            "${XDG_CACHE_HOME}/oh-my-zsh",
            "${ZSH}/cache",
            "${ZSH_CACHE_DIR}"
        ]
    );
    assert_eq!(
        registry.get(IDS[1]).unwrap().paths,
        ["${XDG_CACHE_HOME}/gh"]
    );
    for id in IDS {
        let signature = registry.get(id).unwrap();
        assert_eq!(signature.strategy, CleanStrategy::Manual);
        assert_eq!(signature.risk, RiskTier::Manual);
        assert_eq!(
            signature.platforms,
            [PlatformKind::Macos, PlatformKind::Linux]
        );
        assert!(signature.provider_id.is_none());
        assert!(signature.min_age_days.is_none());
    }
    let env = PlatformEnvironment::simulated(PathFlavor::Posix).with_home("/profile");
    assert_eq!(
        registry.resolve_paths(registry.get(IDS[1]).unwrap(), &env),
        [std::path::PathBuf::from("/profile/.cache/gh")]
    );
    for outside in [
        "/profile/.config/gh/hosts.yml",
        "/profile/.local/share/gh/extensions/tool",
        "/profile/.zcompdump",
        "/profile/.oh-my-zsh/custom/user.zsh",
        "/profile/.cache/unrelated/payload",
        "/tmp/gh-cli-cache",
    ] {
        assert!(
            IDS.iter().all(|id| !registry.path_is_in_scope(
                registry.get(id).unwrap(),
                Path::new(outside),
                &env,
            )),
            "{outside}"
        );
    }
}

#[test]
fn user_tool_observation_retains_generated_executable_and_unknown_payloads_without_authority() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let env = environment(&home);
    let registry = catalog();
    let standard = home.join(".oh-my-zsh/cache");
    let fallback = home.join(".cache/oh-my-zsh");
    let http = home.join(".cache/gh");
    for relative in [
        "completions/_gh",
        ".zsh-update",
        "localhostname",
        "unknown.sqlite",
    ] {
        write(&standard.join(relative));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            standard.join("completions/_gh"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    write(&fallback.join("completions/_tool"));
    write(&http.join("response-cache-entry"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Counting requires metadata, never a readable HTTP response body.
        fs::set_permissions(
            http.join("response-cache-entry"),
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();
    }
    let sentinels = [
        home.join(".config/gh/hosts.yml"),
        home.join(".config/gh/config.yml"),
        home.join(".local/share/gh/extensions/gh-tool"),
        home.join(".zshrc"),
        home.join(".zcompdump"),
        home.join(".oh-my-zsh/custom/user.zsh"),
        home.join(".cache/unrelated/payload"),
    ];
    for sentinel in &sentinels {
        write(sentinel);
    }
    let omz = observe(&registry, IDS[0], &env);
    assert_eq!(omz.len(), 2);
    let standard_item = omz
        .iter()
        .find(|item| path_algebra::equal(&item.path, &standard.to_string_lossy(), env.flavor()))
        .unwrap();
    assert_eq!(standard_item.size.logical, 4 * 8192);
    assert_eq!(standard_item.file_count, 4);
    assert_eq!(standard_item.quality, ObservationQuality::Fresh);
    let fallback_item = omz
        .iter()
        .find(|item| path_algebra::equal(&item.path, &fallback.to_string_lossy(), env.flavor()))
        .unwrap();
    assert_eq!(fallback_item.size.logical, 8192);
    let gh = observe(&registry, IDS[1], &env);
    assert_eq!(gh.len(), 1);
    assert_eq!(gh[0].size.logical, 8192);
    for item in omz.iter().chain(gh.iter()) {
        assert_advisory(item);
    }
    assert!(sentinels.iter().all(|path| path.exists()));
    assert!(standard.join("completions/_gh").exists());
}

#[test]
fn empty_xdg_cache_home_retains_named_observations_without_cleanup_authority() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let env = environment(&home).with_cache_path_override("XDG_CACHE_HOME", "");
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let unrelated = home.join(".cache/unrelated/payload");
    write(&unrelated);
    for (id, namespace) in [
        (IDS[0], "oh-my-zsh"),
        (IDS[1], "gh"),
        ("ai.opencode.cache", "opencode"),
    ] {
        let cache = home.join(".cache").join(namespace);
        write(&cache.join("payload"));
        let items = observe(&registry, id, &env);
        let present = items.iter().filter(|item| item.exists).collect::<Vec<_>>();
        assert_eq!(present.len(), 1, "{id}");
        assert_eq!(present[0].path, cache.to_string_lossy());
        assert_eq!(present[0].size.logical, 8192);
        assert_eq!(present[0].quality, ObservationQuality::Fresh);
        assert_advisory(present[0]);
        assert!(!registry.path_is_in_scope(registry.get(id).unwrap(), &unrelated, &env));
        assert!(cache.join("payload").exists());
    }
    assert!(unrelated.exists());
}

#[test]
fn user_tool_xdg_override_is_exact_and_never_searches_the_default_or_neighbor_store() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let xdg = home.join("named-cache-root");
    let env = environment(&home).with_cache_path_override("XDG_CACHE_HOME", &xdg);
    let registry = catalog();
    write(&home.join(".cache/gh/old-default"));
    write(&home.join(".cache/oh-my-zsh/old-default"));
    write(&xdg.join("gh/response"));
    write(&xdg.join("oh-my-zsh/completions/_tool"));
    write(&xdg.join("unrelated/private-data"));
    let gh = observe(&registry, IDS[1], &env);
    assert_eq!(gh.len(), 1);
    assert_eq!(gh[0].path, xdg.join("gh").to_string_lossy());
    assert_eq!(gh[0].size.logical, 8192);
    let omz = observe(&registry, IDS[0], &env);
    let present = omz.iter().filter(|item| item.exists).collect::<Vec<_>>();
    assert_eq!(present.len(), 1);
    assert_eq!(present[0].path, xdg.join("oh-my-zsh").to_string_lossy());
    for item in gh.iter().chain(present) {
        assert_advisory(item);
    }
}

#[test]
fn user_tool_invalid_xdg_override_does_not_fall_back_to_another_namespace() {
    let registry = catalog();
    for custom in ["relative-cache", "/profile/../cache"] {
        let env = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home("/profile")
            .with_cache_path_override("XDG_CACHE_HOME", custom);
        assert!(registry
            .resolve_paths(registry.get(IDS[1]).unwrap(), &env)
            .is_empty());
        assert_eq!(
            registry.resolve_paths(registry.get(IDS[0]).unwrap(), &env),
            [std::path::PathBuf::from("/profile/.oh-my-zsh/cache")]
        );
    }
    // A broad injected XDG base still selects only its named gh namespace and
    // remains advisory. It does not turn the base into a census or an action.
    let env = PlatformEnvironment::simulated(PathFlavor::Posix)
        .with_home("/profile")
        .with_cache_path_override("XDG_CACHE_HOME", "/profile");
    assert_eq!(
        registry.resolve_paths(registry.get(IDS[1]).unwrap(), &env),
        [std::path::PathBuf::from("/profile/gh")]
    );
    assert!(!registry.path_is_in_scope(
        registry.get(IDS[1]).unwrap(),
        Path::new("/profile/other"),
        &env
    ));
}

#[test]
fn user_tool_advisory_signature_cannot_be_forged_into_a_delete_plan() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let env = environment(&home).with_cache_path_override("XDG_CACHE_HOME", "");
    let registry = catalog();
    write(&home.join(".oh-my-zsh/cache/payload"));
    write(&home.join(".cache/gh/response"));
    for id in IDS {
        let item = observe(&registry, id, &env)
            .into_iter()
            .find(|item| item.observed_bytes() > 0)
            .unwrap();
        for forge in [false, true] {
            let mut selected = item.clone();
            selected.is_selected = true;
            if forge {
                selected.risk = RiskTier::Rebuild;
                selected.cache_metadata.management_mode = CacheManagementMode::Neati;
                selected.cache_metadata.size_semantics = CacheSizeSemantics::PhysicalReclaimable;
                selected.disposition = selected.derive_disposition();
                assert!(selected.allows_cleanup());
            }
            let error = SafetyPlanner::create_plan_with_environment(
                &[selected],
                &registry,
                &env,
                &OwnerProviderRegistry::new(Vec::new()),
            )
            .unwrap_err();
            let NeatiError::RefusedSelection(refusals) = error else {
                panic!("expected a non-authorizing advisory refusal, got {error:?}");
            };
            assert_eq!(refusals.len(), 1);
            assert_eq!(refusals[0].reason, CleanFailureReason::OwnerManaged);
        }
    }
    assert!(home.join(".oh-my-zsh/cache/payload").exists());
    assert!(home.join(".cache/gh/response").exists());
}

#[test]
fn user_tool_empty_and_absent_namespaces_do_not_become_scan_rows() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".cache/gh")).unwrap();
    let result = ScanEngine::scan(
        &catalog(),
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &environment(&home),
        &NeverCancelled,
        |_| {},
    );
    assert_eq!(result.categories.len(), 1);
    assert!(result.categories[0].items.is_empty());
    assert_eq!(result.total_bytes, 0);
    assert_eq!(result.cleanable_bytes, 0);
    assert_eq!(result.quality, ObservationQuality::Fresh);
}

#[test]
fn existing_fixed_manual_namespace_preserves_absence_and_regular_observation() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let env = environment(&home);
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let absent = observe(&registry, "dev.corepack.distributions", &env);
    assert_eq!(absent.len(), 1);
    assert!(!absent[0].exists);
    assert_eq!(absent[0].quality, ObservationQuality::Fresh);
    assert_eq!(absent[0].inspection_issue, None);
    write(&home.join(".cache/node/corepack/manager.bin"));
    let present = observe(&registry, "dev.corepack.distributions", &env);
    assert_eq!(present.len(), 1);
    assert!(present[0].exists);
    assert_eq!(present[0].quality, ObservationQuality::Fresh);
    assert_eq!(present[0].size.logical, 8192);
    assert_advisory(&present[0]);
}

#[test]
fn user_tool_owner_and_handle_readings_cannot_make_advisory_bytes_actionable() {
    use neati_platform::open_files::{FixedOpenFileProbe, OpenFileState};
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    write(&home.join(".cache/gh/response"));
    let env = environment(&home);
    let registry = catalog();
    for processes in [
        RunningApplications::default(),
        RunningApplications::from_process_names(["gh".into(), "zsh".into()]),
        RunningApplications::from_process_names(["fixture-idle".into()]),
    ] {
        for handles in [
            OpenFileState::Idle,
            OpenFileState::InUse,
            OpenFileState::Unknown,
        ] {
            let observed = DirectoryScanner::scan_signature(
                registry.get(IDS[1]).unwrap(),
                &env,
                &NeverCancelled,
                &processes
                    .clone()
                    .with_open_file_probe(std::sync::Arc::new(FixedOpenFileProbe(handles))),
            );
            assert_eq!(observed.len(), 1);
            assert_eq!(observed[0].size.logical, 8192);
            assert_advisory(&observed[0]);
        }
    }
}

#[cfg(not(windows))]
#[test]
fn nested_known_shell_namespaces_qualify_accounting_without_cleanup_authority() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let standard = home.join(".oh-my-zsh/cache");
    write(&standard.join("response"));
    write(&standard.join("oh-my-zsh/completions/_tool"));
    let result = ScanEngine::scan(
        &catalog(),
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &environment(&home).with_cache_path_override("XDG_CACHE_HOME", &standard),
        &NeverCancelled,
        |_| {},
    );
    let observed = result.categories[0]
        .items
        .iter()
        .filter(|item| item.observed_bytes() > 0)
        .collect::<Vec<_>>();
    assert_eq!(observed.len(), 2);
    let outer = observed
        .iter()
        .find(|item| item.path == standard.to_string_lossy())
        .unwrap();
    let nested = observed
        .iter()
        .find(|item| item.path == standard.join("oh-my-zsh").to_string_lossy())
        .unwrap();
    assert_eq!(outer.size.logical, 2 * 8192);
    assert_eq!(nested.size.logical, 8192);
    assert_eq!(
        result.total_bytes,
        outer.observed_bytes() + nested.observed_bytes()
    );
    assert_eq!(result.ambiguous_overlap_bytes, nested.observed_bytes());
    assert_eq!(
        result.total_bytes - result.ambiguous_overlap_bytes,
        outer.observed_bytes()
    );
    assert_eq!(result.cleanable_bytes, 0);
    for item in observed {
        assert_advisory(item);
    }
}

#[test]
fn user_tool_inaccessible_ancestor_is_explicit_instead_of_an_absent_cache() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    // This deterministic filesystem refusal does not depend on the runner's
    // effective UID or permission privileges: a file cannot contain gh/.
    fs::write(home.join(".cache"), b"not a directory").unwrap();
    let items = observe(&catalog(), IDS[1], &environment(&home));
    assert_eq!(items.len(), 1);
    assert!(items[0].exists);
    assert_eq!(items[0].quality, ObservationQuality::Unavailable);
    assert_eq!(items[0].inspection_issue, Some(ScanGapKind::Unknown));
    assert!(items[0].incomplete_reason.is_some());
    assert_eq!(items[0].size.logical, 0);
    assert_advisory(&items[0]);
}

#[test]
fn user_tool_depth_and_cancellation_bounds_never_grant_cleanup() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    write(&home.join(".cache/gh/response"));
    write(&home.join(".cache/gh/deep/nested/response"));
    let env = environment(&home);
    let registry = catalog();
    let counters = TraversalCounters::default();
    let context = WalkContext::new(
        &env,
        &NeverCancelled,
        ScanLimits::bounded(0, 1),
        &counters,
        &NoRootProgress,
    );
    let bounded = DirectoryScanner::scan_signature_with_context(
        registry.get(IDS[1]).unwrap(),
        None,
        &context,
        EligibilityGate::Open,
        &RunningApplications::default(),
    );
    assert_eq!(bounded.items.len(), 1);
    assert_eq!(bounded.items[0].quality, ObservationQuality::Partial);
    assert_eq!(bounded.items[0].size.logical, 8192);
    assert_eq!(
        bounded.items[0].inspection_issue,
        Some(ScanGapKind::DepthLimit)
    );
    assert_eq!(
        bounded.items[0].cache_metadata.size_semantics,
        CacheSizeSemantics::ConservativeLowerBound
    );
    assert_advisory(&bounded.items[0]);

    struct CancelOnRead<'a>(&'a TraversalCounters);
    impl CancellationProbe for CancelOnRead<'_> {
        fn is_cancelled(&self) -> bool {
            self.0.directories_read() > 0
        }
    }
    let stopped = TraversalCounters::default();
    let cancellation = CancelOnRead(&stopped);
    let context = WalkContext::new(
        &env,
        &cancellation,
        ScanLimits::default(),
        &stopped,
        &NoRootProgress,
    );
    let cancelled = DirectoryScanner::scan_signature_with_context(
        registry.get(IDS[1]).unwrap(),
        None,
        &context,
        EligibilityGate::Open,
        &RunningApplications::default(),
    );
    assert_eq!(cancelled.items.len(), 1);
    assert_eq!(cancelled.items[0].quality, ObservationQuality::Unavailable);
    assert_eq!(
        cancelled.items[0].inspection_issue,
        Some(ScanGapKind::Cancelled)
    );
    assert_eq!(cancelled.items[0].file_count, 0);
    assert_eq!(stopped.directories_read(), 1);
    assert_advisory(&cancelled.items[0]);
}

#[cfg(unix)]
#[test]
fn user_tool_linked_roots_and_ancestors_are_not_measured_or_followed() {
    use std::os::unix::fs::symlink;
    for link_root in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().canonicalize().unwrap().join("home");
        let outside = fixture.path().canonicalize().unwrap().join("outside");
        fs::create_dir_all(&home).unwrap();
        write(&outside.join("gh/response"));
        if link_root {
            fs::create_dir_all(home.join(".cache")).unwrap();
            symlink(outside.join("gh"), home.join(".cache/gh")).unwrap();
        } else {
            symlink(&outside, home.join(".cache")).unwrap();
        }
        for env in [
            environment(&home),
            environment(&home).with_cache_path_override("XDG_CACHE_HOME", ""),
        ] {
            let items = observe(&catalog(), IDS[1], &env);
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].quality, ObservationQuality::Unavailable);
            assert_eq!(
                items[0].inspection_issue,
                Some(ScanGapKind::SafetyProtected)
            );
            assert_eq!(items[0].size.logical, 0);
            assert_eq!(items[0].file_count, 0);
            assert_eq!(items[0].last_modified, None);
            assert_advisory(&items[0]);
        }
        assert!(outside.join("gh/response").exists());
    }
}

#[cfg(unix)]
#[test]
fn user_tool_descendant_links_report_only_link_metadata() {
    use std::os::unix::fs::symlink;
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().canonicalize().unwrap();
    let cache = home.join(".cache/gh");
    let outside = home.join("unrelated/responses");
    write(&cache.join("response"));
    write(&outside.join("one"));
    write(&outside.join("two"));
    let link = cache.join("other");
    symlink(&outside, &link).unwrap();
    let item = observe(&catalog(), IDS[1], &environment(&home)).remove(0);
    assert_eq!(
        item.size.logical,
        8192 + fs::symlink_metadata(&link).unwrap().len()
    );
    assert_eq!(item.file_count, 2);
    assert_advisory(&item);
    assert!(outside.join("one").exists() && outside.join("two").exists());
}
