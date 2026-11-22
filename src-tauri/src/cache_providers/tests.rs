use super::{
    cache_location_approved, node_manager_roots, owner_process_refusal, parse_discovered_path,
    parse_provider_path, process_matches_provider, process_table_activity, AbsolutePath,
    CacheProviderPruneFailure, CacheProviderRegistry, UV_PRUNE_TIMEOUT,
};
use super::{
    configure_provider_command, strip_cache_environment, NativeProviderMeasurer,
    ProviderCommandResult, ProviderCommandRunner, ProviderKind, ProviderMeasurer,
    CACHE_PATH_ENVIRONMENT,
};
use crate::models::{
    CancellationProbe, CleanStrategy, NeverCancelled, ObservationQuality, ScanGapKind,
};
use crate::scanner::{NoRootProgress, PathMeasurement, ScanLimits, TraversalCounters};
use crate::signatures::SignatureRegistry;
use neati_platform::path_algebra::PathFlavor;
use neati_platform::paths::SimulatedPaths;
use neati_platform::PlatformEnvironment;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FakeRunner {
    responses: std::collections::HashMap<ProviderKind, ProviderCommandResult>,
    prune_responses: std::collections::HashMap<ProviderKind, ProviderCommandResult>,
    prune_files: std::collections::HashMap<ProviderKind, PathBuf>,
    calls: Mutex<Vec<ProviderKind>>,
    prune_calls: Mutex<Vec<ProviderKind>>,
}

impl FakeRunner {
    fn with(mut self, provider: ProviderKind, result: ProviderCommandResult) -> Self {
        self.responses.insert(provider, result);
        self
    }

    fn with_prune(mut self, provider: ProviderKind, result: ProviderCommandResult) -> Self {
        self.prune_responses.insert(provider, result);
        self
    }

    fn delete_file_on_prune(mut self, provider: ProviderKind, path: PathBuf) -> Self {
        self.prune_files.insert(provider, path);
        self
    }

    fn absent() -> Self {
        Self::default()
    }

    fn calls(&self) -> Vec<ProviderKind> {
        self.calls.lock().unwrap().clone()
    }

    fn prune_calls(&self) -> Vec<ProviderKind> {
        self.prune_calls.lock().unwrap().clone()
    }
}

impl ProviderCommandRunner for FakeRunner {
    fn discover(
        &self,
        provider: ProviderKind,
        _environment: &PlatformEnvironment,
        _cancellation: &dyn CancellationProbe,
    ) -> ProviderCommandResult {
        self.calls.lock().unwrap().push(provider);
        self.responses
            .get(&provider)
            .cloned()
            .unwrap_or(ProviderCommandResult::Absent)
    }

    fn prune(
        &self,
        provider: ProviderKind,
        _environment: &PlatformEnvironment,
    ) -> ProviderCommandResult {
        self.prune_calls.lock().unwrap().push(provider);
        let response = self
            .prune_responses
            .get(&provider)
            .cloned()
            .unwrap_or(ProviderCommandResult::Absent);
        if matches!(
            response,
            ProviderCommandResult::Output { success: true, .. }
        ) {
            if let Some(path) = self.prune_files.get(&provider) {
                let _ = std::fs::remove_file(path);
            }
        }
        response
    }
}

struct FixedMeasurer(PathMeasurement);

impl ProviderMeasurer for FixedMeasurer {
    fn measure(
        &self,
        _path: &std::path::Path,
        _environment: &PlatformEnvironment,
        _cancellation: &dyn CancellationProbe,
        _limits: ScanLimits,
        _counters: &TraversalCounters,
    ) -> PathMeasurement {
        self.0.clone()
    }
}

struct CancelAfterVisits<'a> {
    counters: &'a TraversalCounters,
    limit: u64,
}

impl CancellationProbe for CancelAfterVisits<'_> {
    fn is_cancelled(&self) -> bool {
        self.counters.visited_entries() >= self.limit
    }
}

#[derive(Default)]
struct RecordingProgress(Mutex<Vec<(String, PathBuf)>>);

impl crate::scanner::RootProgressSink for RecordingProgress {
    fn root_started(&self, signature: &crate::models::Signature, root: &std::path::Path) {
        self.0
            .lock()
            .unwrap()
            .push((signature.id.clone(), root.to_path_buf()));
    }
}

fn fixture_environment(home: &std::path::Path) -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::current()).with_roots(Arc::new(
        SimulatedPaths::new()
            .with_flavor(PathFlavor::current())
            .with_home(home),
    ))
}

fn successful_discovery(path: &std::path::Path) -> ProviderCommandResult {
    ProviderCommandResult::Output {
        success: true,
        stdout: format!("{}\n", path.display()).into_bytes(),
        stderr: Vec::new(),
    }
}

#[test]
fn uv_prune_uses_its_lock_wait_and_bounded_owner_command() {
    assert_eq!(ProviderKind::Uv.prune_args(), &["cache", "prune"]);
    assert!(ProviderKind::Uv.active_processes().is_empty());
    assert_eq!(UV_PRUNE_TIMEOUT, std::time::Duration::from_secs(300));
}

#[test]
fn uv_owner_prune_succeeds_without_generic_tree_deletion() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let cache = home.join(".cache/uv");
    std::fs::create_dir_all(&cache).unwrap();
    let unused = cache.join("unused.whl");
    let retained = cache.join("retained.whl");
    std::fs::write(&unused, vec![1u8; 4_096]).unwrap();
    std::fs::write(&retained, vec![2u8; 2_048]).unwrap();
    let environment = fixture_environment(&home);
    let runner = FakeRunner::default()
        .with(ProviderKind::Uv, successful_discovery(&cache))
        .with_prune(
            ProviderKind::Uv,
            ProviderCommandResult::Output {
                success: true,
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        )
        .delete_file_on_prune(ProviderKind::Uv, unused.clone());

    let outcome = CacheProviderRegistry::prune_with(
        ProviderKind::Uv,
        &cache,
        &environment,
        &runner,
        &NativeProviderMeasurer,
    )
    .expect("the owner command succeeds");

    assert_eq!(outcome, Some(4_096));
    assert_eq!(runner.prune_calls(), vec![ProviderKind::Uv]);
    assert!(!unused.exists(), "the fake owner pruned its unused fixture");
    assert!(retained.exists(), "the owner retained referenced data");
    assert!(
        cache.is_dir(),
        "the store root was never recursively removed"
    );
}

#[test]
fn failed_uv_owner_prune_leaves_the_store_untouched_without_fallback() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let cache = home.join(".cache/uv");
    std::fs::create_dir_all(&cache).unwrap();
    let existing = cache.join("package.whl");
    std::fs::write(&existing, vec![3u8; 1_024]).unwrap();
    let environment = fixture_environment(&home);
    let runner = FakeRunner::default()
        .with(ProviderKind::Uv, successful_discovery(&cache))
        .with_prune(
            ProviderKind::Uv,
            ProviderCommandResult::Output {
                success: false,
                stdout: Vec::new(),
                stderr: b"fixture refusal".to_vec(),
            },
        )
        .delete_file_on_prune(ProviderKind::Uv, existing.clone());

    let error = CacheProviderRegistry::prune_with(
        ProviderKind::Uv,
        &cache,
        &environment,
        &runner,
        &NativeProviderMeasurer,
    )
    .expect_err("a failed owner command is reported honestly");

    assert!(error
        .to_string()
        .contains("uv prune failed: fixture refusal"));
    assert_eq!(runner.prune_calls(), vec![ProviderKind::Uv]);
    assert!(
        existing.exists(),
        "failure does not fall back to file deletion"
    );
    assert!(
        cache.is_dir(),
        "failure does not fall back to root deletion"
    );
}

#[test]
fn provider_command_removes_cache_path_overrides() {
    let mut command = Command::new("provider-fixture");
    for variable in CACHE_PATH_ENVIRONMENT {
        command.env(variable, "/untrusted/cache");
    }
    command.env("NEATI_UNRELATED_FIXTURE", "preserved");
    strip_cache_environment(&mut command);
    let variables: std::collections::HashMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_string_lossy().to_ascii_lowercase(), value))
        .collect();
    for variable in CACHE_PATH_ENVIRONMENT {
        assert_eq!(
            variables.get(&variable.to_ascii_lowercase()),
            Some(&None),
            "{variable}"
        );
    }
    assert_eq!(
        variables.get("neati_unrelated_fixture"),
        Some(&Some(std::ffi::OsStr::new("preserved")))
    );
}

#[cfg(unix)]
#[test]
fn supported_cache_overrides_are_validated_and_reapplied_per_owner() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().unwrap();
    let bin = fixture.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let node = bin.join("node");
    std::fs::write(&node, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = std::fs::metadata(&node).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&node, permissions).unwrap();

    for (provider, setting, forwarded) in [
        (ProviderKind::Uv, "UV_CACHE_DIR", "UV_CACHE_DIR"),
        (ProviderKind::Pip, "PIP_CACHE_DIR", "PIP_CACHE_DIR"),
        (ProviderKind::Npm, "npm_config_cache", "npm_config_cache"),
        (
            ProviderKind::Pnpm,
            "NPM_CONFIG_STORE_DIR",
            "npm_config_store_dir",
        ),
    ] {
        let cache = fixture
            .path()
            .join(format!("cache-{}", provider.executable()));
        std::fs::create_dir_all(&cache).unwrap();
        let environment = fixture_environment(fixture.path())
            .with_tool("node", &node)
            .with_cache_path_override(setting, &cache);
        let mut command = Command::new("provider-fixture");
        command.env(setting, "/untrusted/cache");
        configure_provider_command(provider, &environment, &mut command)
            .expect("supported existing cache override");
        let forwarded_value = command
            .get_envs()
            .find(|(name, _)| *name == forwarded)
            .and_then(|(_, value)| value);
        let canonical_cache = std::fs::canonicalize(&cache).unwrap();
        assert_eq!(
            forwarded_value,
            Some(canonical_cache.as_os_str()),
            "{setting} must be forwarded only after validation"
        );
    }

    let environment = fixture_environment(fixture.path())
        .with_cache_path_override("UV_CACHE_DIR", "relative/cache");
    assert!(configure_provider_command(
        ProviderKind::Uv,
        &environment,
        &mut Command::new("provider-fixture")
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn npm_cache_settings_ignore_empty_values_and_refuse_conflicts() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().unwrap();
    let cache_a = fixture.path().join("cache-a");
    let cache_b = fixture.path().join("cache-b");
    std::fs::create_dir_all(&cache_a).unwrap();
    std::fs::create_dir_all(&cache_b).unwrap();
    let bin = fixture.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let node = bin.join("node");
    std::fs::write(&node, b"fixture").unwrap();
    let mut permissions = std::fs::metadata(&node).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&node, permissions).unwrap();
    let base = fixture_environment(fixture.path()).with_tool("node", &node);

    let empty_upper = base
        .clone()
        .with_cache_path_override("NPM_CONFIG_CACHE", "")
        .with_cache_path_override("npm_config_cache", &cache_b);
    let mut command = Command::new("provider-fixture");
    configure_provider_command(ProviderKind::Npm, &empty_upper, &mut command).unwrap();
    assert_eq!(
        command
            .get_envs()
            .find(|(name, _)| *name == "npm_config_cache")
            .and_then(|(_, value)| value),
        Some(std::fs::canonicalize(&cache_b).unwrap().as_os_str())
    );

    let conflicting = base
        .with_cache_path_override("NPM_CONFIG_CACHE", &cache_a)
        .with_cache_path_override("npm_config_cache", &cache_b);
    let error = configure_provider_command(
        ProviderKind::Npm,
        &conflicting,
        &mut Command::new("provider-fixture"),
    )
    .unwrap_err();
    assert!(error.contains("Conflicting npm_config_cache and NPM_CONFIG_CACHE"));
}

#[test]
fn go_commands_disable_automatic_toolchain_downloads() {
    let mut command = Command::new("go-fixture");
    command.env("GOTOOLCHAIN", "auto");
    command.env("GOMODCACHE", "/untrusted/mod-cache");
    configure_provider_command(
        ProviderKind::GoModule,
        &PlatformEnvironment::native(),
        &mut command,
    )
    .unwrap();
    let variables: std::collections::HashMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_string_lossy().into_owned(), value))
        .collect();

    assert_eq!(
        variables.get("GOTOOLCHAIN"),
        Some(&Some(std::ffi::OsStr::new("local")))
    );
    assert_eq!(variables.get("GOMODCACHE"), Some(&None));
}

#[test]
fn npm_provider_is_wired_to_its_signature_and_commands() {
    assert_eq!(
        ProviderKind::for_signature("dev.npm.cache"),
        Some(ProviderKind::Npm)
    );
    assert_eq!(ProviderKind::Npm.executable(), "npm");
    assert_eq!(
        ProviderKind::Npm.discovery_args(),
        &["config", "get", "cache"]
    );
    assert_eq!(ProviderKind::Npm.prune_args(), &["cache", "verify"]);
    assert_eq!(ProviderKind::Npm.active_processes(), &["npm", "npm-cli"]);
    assert_eq!(ProviderKind::for_signature("dev.yarn.cache"), None);
}

#[test]
fn cache_owner_process_checks_ignore_unrelated_node_apps() {
    let unrelated_node_command = vec![
        std::ffi::OsString::from("node"),
        std::ffi::OsString::from("/workspace/server.js"),
    ];
    assert!(!process_matches_provider(
        ProviderKind::Npm,
        std::ffi::OsStr::new("node"),
        &unrelated_node_command,
    ));
    assert!(!process_matches_provider(
        ProviderKind::Pnpm,
        std::ffi::OsStr::new("node"),
        &unrelated_node_command,
    ));

    let npm_command = vec![
        std::ffi::OsString::from("node"),
        std::ffi::OsString::from("/usr/local/lib/node_modules/npm/bin/npm-cli.js"),
        std::ffi::OsString::from("cache"),
    ];
    assert!(process_matches_provider(
        ProviderKind::Npm,
        std::ffi::OsStr::new("node"),
        &npm_command,
    ));

    let pnpm_command = vec![
        std::ffi::OsString::from("node"),
        std::ffi::OsString::from("/usr/local/lib/node_modules/pnpm/bin/pnpm.cjs"),
        std::ffi::OsString::from("store"),
    ];
    assert!(process_matches_provider(
        ProviderKind::Pnpm,
        std::ffi::OsStr::new("node"),
        &pnpm_command,
    ));

    assert_eq!(
        process_table_activity::<&str>(&[], |_| true),
        None,
        "an unreadable process table is not an idle-owner signal"
    );
    assert_eq!(
        process_table_activity(&["node"], |_| false),
        Some(false),
        "a readable unrelated Node process does not block the owner command"
    );
    assert_eq!(
        owner_process_refusal(ProviderKind::Npm, None),
        Some(CacheProviderPruneFailure::OwnerStateUnknown("npm".into()))
    );
    assert_eq!(
        owner_process_refusal(ProviderKind::Npm, Some(true)),
        Some(CacheProviderPruneFailure::OwnerRunning("npm".into()))
    );
    assert_eq!(owner_process_refusal(ProviderKind::Npm, Some(false)), None);
}

#[test]
fn pip_owner_check_ignores_unrelated_python_but_catches_module_invocation() {
    let signature = SignatureRegistry::load_embedded_catalog()
        .unwrap()
        .get("dev.pip.cache")
        .unwrap()
        .process_guard();
    assert_eq!(signature.executables(), &["pip", "pip3"]);
    let unrelated = vec![
        std::ffi::OsString::from("python3"),
        std::ffi::OsString::from("/workspace/server.py"),
    ];
    assert!(!process_matches_provider(
        ProviderKind::Pip,
        std::ffi::OsStr::new("python3"),
        &unrelated,
    ));
    let module = vec![
        std::ffi::OsString::from("python3"),
        std::ffi::OsString::from("-m"),
        std::ffi::OsString::from("pip"),
        std::ffi::OsString::from("cache"),
    ];
    assert!(process_matches_provider(
        ProviderKind::Pip,
        std::ffi::OsStr::new("python3"),
        &module,
    ));
    for args in [
        vec!["python3", "-I", "-m", "pip", "install"],
        vec!["python3", "-u", "-W", "ignore", "-m", "pip"],
    ] {
        let command = args
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(process_matches_provider(
            ProviderKind::Pip,
            std::ffi::OsStr::new("python3"),
            &command,
        ));
    }
    let unrelated_argument = ["python3", "server.py", "-m", "pip"]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    assert!(!process_matches_provider(
        ProviderKind::Pip,
        std::ffi::OsStr::new("python3"),
        &unrelated_argument,
    ));
    let inline_script = ["python3", "-c", "print('ready')", "-m", "pip"]
        .into_iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    assert!(!process_matches_provider(
        ProviderKind::Pip,
        std::ffi::OsStr::new("python3"),
        &inline_script,
    ));
    assert!(process_matches_provider(
        ProviderKind::Pip,
        std::ffi::OsStr::new("pip3.13"),
        &[],
    ));
}

#[test]
fn language_cache_providers_use_fixed_owner_commands() {
    assert_eq!(ProviderKind::GoBuild.discovery_args(), &["env", "GOCACHE"]);
    assert_eq!(ProviderKind::GoBuild.prune_args(), &["clean", "-cache"]);
    assert_eq!(
        ProviderKind::GoModule.discovery_args(),
        &["env", "GOMODCACHE"]
    );
    assert_eq!(ProviderKind::GoModule.prune_args(), &["clean", "-modcache"]);
    assert_eq!(ProviderKind::Pip.discovery_args(), &["cache", "dir"]);
    assert_eq!(ProviderKind::Pip.prune_args(), &["cache", "purge"]);
    assert_eq!(
        ProviderKind::NugetHttp.discovery_args(),
        &[
            "nuget",
            "locals",
            "http-cache",
            "--list",
            "--force-english-output"
        ]
    );
    assert_eq!(
        ProviderKind::NugetGlobalPackages.prune_args(),
        &[
            "nuget",
            "locals",
            "global-packages",
            "--clear",
            "--force-english-output"
        ]
    );
    assert_eq!(
        ProviderKind::Composer.prune_args(),
        &["--no-interaction", "--no-plugins", "clear-cache"]
    );
}

#[cfg(unix)]
#[test]
fn script_providers_receive_only_a_validated_runtime_path() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let node = bin.join("node");
    std::fs::write(&node, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = std::fs::metadata(&node).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&node, permissions).unwrap();

    let environment = fixture_environment(home.path()).with_tool("node", &node);
    let mut command = Command::new("npm-fixture");
    command.env("PATH", "/untrusted/bin");
    configure_provider_command(ProviderKind::Npm, &environment, &mut command).unwrap();
    let path = command
        .get_envs()
        .find(|(key, _)| *key == std::ffi::OsStr::new("PATH"))
        .and_then(|(_, value)| value)
        .unwrap()
        .to_string_lossy();

    assert!(path.contains(".local/bin"));
    assert!(path.contains("/usr/bin"));
    assert!(!path.contains("/untrusted/bin"));
}

#[test]
fn nuget_discovery_accepts_one_expected_labeled_path() {
    let path = if cfg!(windows) {
        r"C:\Users\tester\AppData\Local\NuGet\v3-cache"
    } else {
        "/Users/tester/Library/Caches/NuGet/v3-cache"
    };
    let output = format!("http-cache: {path}\n");
    let parsed = parse_provider_path(ProviderKind::NugetHttp, output.as_bytes()).unwrap();
    assert_eq!(parsed.as_path(), std::path::Path::new(path));
    assert!(parse_provider_path(
        ProviderKind::NugetHttp,
        b"global-packages: /Users/tester/.nuget/packages\n",
    )
    .is_err());
    assert!(parse_provider_path(
        ProviderKind::NugetHttp,
        b"http-cache: /one\nhttp-cache: /two\n",
    )
    .is_err());
}

#[test]
fn npm_home_cache_is_an_approved_location() {
    let home = if cfg!(target_os = "windows") {
        PathBuf::from("C:\\Users\\tester")
    } else {
        PathBuf::from("/Users/tester")
    };
    // The default npm cache directory and the npm home itself.
    let flavor = if cfg!(target_os = "windows") {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    };
    assert!(cache_location_approved(
        &home.join(".npm/_cacache"),
        &home,
        flavor
    ));
    assert!(cache_location_approved(&home.join(".npm"), &home, flavor));
    assert!(cache_location_approved(
        &home.join(".nuget/packages"),
        &home,
        flavor
    ));
    assert!(cache_location_approved(
        &home.join(".local/share/NuGet/v3-cache"),
        &home,
        flavor
    ));
    // Broad roots still match strict subdirectories only.
    assert!(cache_location_approved(
        &home.join(".cache/npm/_cacache"),
        &home,
        flavor
    ));
    // Unrelated home children and the home root itself stay rejected.
    assert!(!cache_location_approved(
        &home.join("random-override"),
        &home,
        flavor
    ));
    assert!(!cache_location_approved(&home, &home, flavor));
}

#[test]
fn approved_cache_roots_follow_the_stated_profile() {
    // A cache on a stated non-system drive is approved against that stated
    // home, while a cache under the runner's literal profile is not: the
    // provider's approval boundary is the environment's, not the host's.
    let environment = PlatformEnvironment::simulated(PathFlavor::Windows).with_roots(Arc::new(
        SimulatedPaths::new()
            .with_flavor(PathFlavor::Windows)
            .with_home(r"D:\Cache"),
    ));
    let stated_home = environment.user_home().unwrap();

    assert!(cache_location_approved(
        &PathBuf::from(r"D:\Cache\.npm\_cacache"),
        &stated_home,
        PathFlavor::Windows
    ));
    assert!(cache_location_approved(
        &PathBuf::from(r"D:\Cache\Library\Caches\uv"),
        &stated_home,
        PathFlavor::Windows
    ));
    // The literal profile spelling of another account is not the profile.
    assert!(!cache_location_approved(
        &PathBuf::from(r"C:\Users\other\.npm\_cacache"),
        &stated_home,
        PathFlavor::Windows
    ));

    // Relocated roots come from the stated application-data container, and
    // relocation is only trusted once the root resolves.
    let local_app_data = tempfile::tempdir().unwrap();
    let environment = environment.with_roots(Arc::new(
        SimulatedPaths::new()
            .with_flavor(PathFlavor::Windows)
            .with_home(r"D:\Cache")
            .with_local_app_data(local_app_data.path()),
    ));
    let relocated = super::relocated_cache_roots(&environment);
    // `canonicalize` returns a verbatim path on Windows; the relocation
    // decision is made on the normalized spelling, so the expectation is
    // normalized the same way instead of encoding the host's prefix.
    let expected = neati_platform::NativePlatformPaths::normalize_verbatim_path(
        &local_app_data.path().canonicalize().unwrap(),
    );
    assert_eq!(
        relocated,
        vec![expected],
        "the stated application-data container is the only relocated root"
    );
    // An approved relocation must not be refused by the blacklist: a
    // redirected application-data root that protects its whole tree would
    // make every cache under it cleanable in one place and forbidden in
    // the other.
    for root in &relocated {
        assert!(
            crate::safety::Blacklist::validate_with(&root.join("uv/cache"), &environment).is_ok(),
            "a cache below the relocated container {} must stay cleanable",
            root.display()
        );
    }
    assert!(
        crate::safety::Blacklist::validate_with(local_app_data.path(), &environment).is_err(),
        "the relocated application-data root itself stays protected"
    );
}

/// Fixture paths are rooted by construction; the type only makes the
/// assumption explicit at the boundary the test is exercising.
fn rooted(path: PathBuf) -> AbsolutePath {
    AbsolutePath::new(path).expect("fixture path is absolute")
}

#[test]
fn validate_cache_path_approves_the_stated_home_and_refuses_elsewhere() {
    let stated_home = tempfile::tempdir().unwrap();
    let cache = stated_home.path().join("Library/Caches/npm/_cacache");
    std::fs::create_dir_all(&cache).unwrap();

    let environment = PlatformEnvironment::simulated(PathFlavor::current()).with_roots(Arc::new(
        SimulatedPaths::new()
            .with_flavor(PathFlavor::current())
            .with_home(stated_home.path()),
    ));
    assert!(super::validate_cache_path(rooted(cache), &environment).is_ok());

    // An in-profile directory that is not an approved cache root is refused
    // even though it exists, so the approval step still does work.
    let unapproved = stated_home.path().join("random-override");
    std::fs::create_dir_all(&unapproved).unwrap();
    assert!(super::validate_cache_path(rooted(unapproved), &environment).is_err());
}

#[cfg(unix)]
#[test]
fn nuget_provider_executes_fixed_commands_against_an_isolated_store() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    let cache = home.path().join(".nuget/packages");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("package.bin"), vec![1_u8; 4096]).unwrap();

    let executable = bin.join("dotnet");
    std::fs::write(
        &executable,
        r#"#!/bin/sh
set -eu
fixture_home=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cache="$fixture_home/.nuget/packages"
case "$*" in
  "nuget locals global-packages --list --force-english-output")
    printf 'global-packages: %s\n' "$cache"
    ;;
  "nuget locals global-packages --clear --force-english-output")
    find "$cache" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
    ;;
  *)
    exit 64
    ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    let environment = fixture_environment(home.path()).with_tool("dotnet", &executable);
    let reclaimed =
        super::CacheProviderRegistry::prune("dev.nuget.global_packages", &cache, &environment)
            .unwrap();

    assert!(reclaimed.is_some_and(|bytes| bytes >= 4096));
    assert!(cache.is_dir());
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
}

#[test]
fn a_provider_with_a_stated_missing_tool_is_skipped() {
    // npm/pnpm/uv may be installed on this host; the environment states
    // they are absent, and a stated absence is never re-discovered.
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_missing_tool("go")
        .with_missing_tool("npm")
        .with_missing_tool("pnpm")
        .with_missing_tool("uv")
        .with_missing_tool("pip3")
        .with_missing_tool("composer")
        .with_missing_tool("dotnet");
    let registry = SignatureRegistry::load_embedded().unwrap();
    let counters = TraversalCounters::default();
    let result = super::CacheProviderRegistry::scan_items(
        &registry,
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
    );
    assert!(result.items.is_empty());
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert!(!result.cancelled);
}

#[test]
fn a_detected_tool_with_no_created_cache_does_not_report_an_io_failure() {
    let home = tempfile::tempdir().unwrap();
    let absent_cache = home.path().join(".cache/uv");
    let environment = fixture_environment(home.path());
    let registry = SignatureRegistry::load_embedded().unwrap();
    let runner = FakeRunner::absent().with(ProviderKind::Uv, successful_discovery(&absent_cache));
    let counters = TraversalCounters::default();
    let result = super::CacheProviderRegistry::scan_items_with(
        &registry,
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );
    assert!(result.items.is_empty());
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert!(runner.calls().contains(&ProviderKind::Uv));

    let outside_home = home.path().join("unrelated/uv");
    let runner = FakeRunner::absent().with(ProviderKind::Uv, successful_discovery(&outside_home));
    let result = super::CacheProviderRegistry::scan_items_with(
        &registry,
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );
    assert!(result.items.is_empty());
    assert_eq!(
        result.failures.len(),
        1,
        "unknown missing roots remain gaps"
    );
}

#[cfg(unix)]
#[test]
fn a_stated_uv_cache_override_is_shared_by_discovery_and_owner_prune() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().unwrap();
    let bin = fixture.path().join(".local/bin");
    let cache = fixture.path().join("custom-uv-cache");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("stale.bin"), vec![1_u8; 4096]).unwrap();
    let executable = bin.join("uv");
    std::fs::write(
        &executable,
        r#"#!/bin/sh
set -eu
case "$*" in
  "cache dir") printf '%s\n' "$UV_CACHE_DIR" ;;
  "cache prune") rm "$UV_CACHE_DIR/stale.bin" ;;
  *) exit 64 ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).unwrap();

    let environment = fixture_environment(fixture.path())
        .with_tool("uv", &executable)
        .with_cache_path_override("UV_CACHE_DIR", &cache)
        .with_missing_tool("go")
        .with_missing_tool("pip3")
        .with_missing_tool("pnpm")
        .with_missing_tool("npm")
        .with_missing_tool("composer")
        .with_missing_tool("dotnet");
    let registry = SignatureRegistry::load_embedded().unwrap();
    let counters = TraversalCounters::default();
    let scan = super::CacheProviderRegistry::scan_items(
        &registry,
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
    );
    assert!(scan.failures.is_empty(), "{:?}", scan.failures);
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.items[0].signature_id, "dev.uv.cache");
    assert_eq!(
        std::fs::canonicalize(&scan.items[0].path).unwrap(),
        std::fs::canonicalize(&cache).unwrap()
    );

    let reclaimed = super::CacheProviderRegistry::prune("dev.uv.cache", &cache, &environment)
        .expect("owner command prunes the stated cache");
    assert!(reclaimed.is_some_and(|bytes| bytes >= 4096));
    assert!(!cache.join("stale.bin").exists());
}

#[test]
fn a_go_module_cache_with_toolchain_locks_is_plannable_through_go() {
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join("go/pkg/mod");
    let download = cache.join("cache/download/golang.org/toolchain/@v");
    std::fs::create_dir_all(&download).unwrap();
    std::fs::write(
        download.join("v0.0.1-go1.24.0.darwin-arm64.lock"),
        b"owner lock fixture",
    )
    .unwrap();

    let environment = fixture_environment(home.path());
    let registry = SignatureRegistry::load_embedded().unwrap();
    let runner = FakeRunner::absent().with(ProviderKind::GoModule, successful_discovery(&cache));
    let counters = TraversalCounters::default();
    let result = super::CacheProviderRegistry::scan_items_with(
        &registry,
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );

    assert!(result.failures.is_empty());
    assert_eq!(result.items.len(), 1);
    let mut item = result.items.into_iter().next().unwrap();
    assert_eq!(item.signature_id, "dev.go.mod");
    item.is_selected = true;

    let owner_providers = crate::cleaner::OwnerProviderRegistry::new(Vec::new());
    let plan = crate::safety::SafetyPlanner::create_plan_with_environment(
        &[item],
        &registry,
        &environment,
        &owner_providers,
    )
    .expect("Go owns its valid lockfiles; generic structured-state checks do not apply");
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].strategy, CleanStrategy::ExternalCommand);
}

#[test]
fn pip_scan_plan_carries_only_the_narrow_executable_guard() {
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join(".cache/pip");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("wheel.bin"), vec![0_u8; 4096]).unwrap();

    let environment = fixture_environment(home.path());
    let registry = SignatureRegistry::load_embedded().unwrap();
    let runner = FakeRunner::absent().with(ProviderKind::Pip, successful_discovery(&cache));
    let counters = TraversalCounters::default();
    let result = CacheProviderRegistry::scan_items_with(
        &registry,
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(result.items.len(), 1);
    let mut item = result.items.into_iter().next().unwrap();
    assert_eq!(item.signature_id, "dev.pip.cache");
    item.is_selected = true;
    let plan = crate::safety::SafetyPlanner::create_plan_with_environment(
        &[item],
        &registry,
        &environment,
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
    )
    .expect("the selected pip cache is plannable");
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(
        plan.targets[0].process_guard.executables(),
        &["pip", "pip3"]
    );
    assert_eq!(plan.targets[0].strategy, CleanStrategy::ExternalCommand);
}

#[test]
fn an_excluded_provider_is_not_invoked() {
    let registry = SignatureRegistry::load_embedded().unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current());
    let runner = FakeRunner::absent();
    let counters = TraversalCounters::default();
    let result = super::CacheProviderRegistry::scan_items_with(
        &registry,
        &["dev.uv.cache".to_string()],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );

    assert!(result.items.is_empty());
    assert_eq!(
        runner.calls(),
        vec![
            ProviderKind::GoBuild,
            ProviderKind::GoModule,
            ProviderKind::Pip,
            ProviderKind::Pnpm,
            ProviderKind::Npm,
            ProviderKind::Composer,
            ProviderKind::NugetHttp,
            ProviderKind::NugetTemp,
            ProviderKind::NugetPlugins,
            ProviderKind::NugetGlobalPackages,
        ]
    );
}

#[test]
fn command_cancellation_stops_before_later_providers() {
    let registry = SignatureRegistry::load_embedded().unwrap();
    let runner = FakeRunner::absent().with(ProviderKind::GoBuild, ProviderCommandResult::Cancelled);
    let counters = TraversalCounters::default();
    let progress = RecordingProgress::default();
    let result = super::CacheProviderRegistry::scan_items_with(
        &registry,
        &[],
        &PlatformEnvironment::simulated(PathFlavor::current()),
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &progress,
        &runner,
        &NativeProviderMeasurer,
    );

    assert!(result.cancelled);
    assert!(result.items.is_empty());
    assert_eq!(runner.calls(), vec![ProviderKind::GoBuild]);
}

#[test]
fn malformed_and_timed_out_discovery_are_typed_scan_failures() {
    let registry = SignatureRegistry::load_embedded().unwrap();
    let runner = FakeRunner::absent()
        .with(
            ProviderKind::GoBuild,
            ProviderCommandResult::Output {
                success: true,
                stdout: b"relative/cache\n".to_vec(),
                stderr: Vec::new(),
            },
        )
        .with(
            ProviderKind::GoModule,
            ProviderCommandResult::Failed("command timed out; permission denied".to_string()),
        );
    let counters = TraversalCounters::default();
    let result = super::CacheProviderRegistry::scan_items_with(
        &registry,
        &[],
        &PlatformEnvironment::simulated(PathFlavor::current()),
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );

    assert!(result.items.is_empty());
    assert_eq!(result.failures.len(), 2);
    assert_eq!(
        result
            .failures
            .iter()
            .map(|failure| failure.kind)
            .collect::<Vec<_>>(),
        vec![ScanGapKind::IoError, ScanGapKind::Unknown]
    );
}

#[test]
fn permission_refused_measurement_remains_a_visible_unavailable_row() {
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join(".cache/uv/store");
    std::fs::create_dir_all(&cache).unwrap();
    let environment = fixture_environment(home.path());
    let runner = FakeRunner::absent().with(ProviderKind::Uv, successful_discovery(&cache));
    let counters = TraversalCounters::default();
    let progress = RecordingProgress::default();
    let result = super::CacheProviderRegistry::scan_items_with(
        &SignatureRegistry::load_embedded().unwrap(),
        &[],
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &progress,
        &runner,
        &FixedMeasurer(PathMeasurement::unavailable("Permission denied")),
    );

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].quality, ObservationQuality::Unavailable);
    assert_eq!(
        result.items[0].incomplete_reason.as_deref(),
        Some("Permission denied")
    );
    assert_eq!(
        progress.0.lock().unwrap().as_slice(),
        &[("dev.uv.cache".to_string(), cache.canonicalize().unwrap())]
    );
    assert!(result.failures.is_empty());
}

#[test]
fn cancellation_inside_cache_measurement_stops_the_walk_and_later_providers() {
    let home = tempfile::tempdir().unwrap();
    let cache = home.path().join(".cache/uv/store");
    std::fs::create_dir_all(&cache).unwrap();
    for index in 0..32 {
        std::fs::write(cache.join(format!("{index}.bin")), b"fixture").unwrap();
    }
    let environment = fixture_environment(home.path());
    let runner = FakeRunner::absent().with(ProviderKind::Uv, successful_discovery(&cache));
    let counters = TraversalCounters::default();
    let cancellation = CancelAfterVisits {
        counters: &counters,
        limit: 5,
    };
    let result = super::CacheProviderRegistry::scan_items_with(
        &SignatureRegistry::load_embedded().unwrap(),
        &[],
        &environment,
        &cancellation,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
        &runner,
        &NativeProviderMeasurer,
    );

    assert!(result.cancelled);
    assert_eq!(result.items.len(), 1);
    assert_ne!(result.items[0].quality, ObservationQuality::Fresh);
    assert!(result.items[0]
        .incomplete_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("cancelled")));
    assert!(counters.visited_entries() < 33);
    assert!(counters.directories_read() > 0);
    assert_eq!(
        runner.calls(),
        vec![
            ProviderKind::GoBuild,
            ProviderKind::GoModule,
            ProviderKind::Pip,
            ProviderKind::Uv,
        ]
    );
}

#[test]
fn provider_executable_validation_uses_trusted_tool_roots() {
    #[cfg(target_os = "macos")]
    {
        use super::validate_executable;
        use std::path::Path;
        let environment = PlatformEnvironment::native();
        assert!(validate_executable(Path::new("/usr/bin/env"), &environment).is_ok());
        let dir = tempfile::tempdir().unwrap();
        let stray = dir.path().join("npm");
        std::fs::write(&stray, b"#!/bin/sh\n").unwrap();
        assert!(validate_executable(&stray, &environment).is_err());
    }
}

#[test]
fn trusted_paths_require_a_documented_tool_root() {
    use super::executable_under_roots;
    use std::path::{Path, PathBuf};

    // Windows-style fixtures stay lexically comparable on Unix hosts.
    let roots: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Users\tester\AppData\Roaming\npm"),
        PathBuf::from(r"C:\Users\tester\AppData\Local\pnpm"),
        PathBuf::from(r"C:\Program Files\nodejs"),
    ];
    assert!(executable_under_roots(
        Path::new(r"C:\Users\tester\AppData\Roaming\npm\npm.cmd"),
        &roots
    ));
    assert!(executable_under_roots(
        Path::new(r"C:\Users\tester\AppData\Local\pnpm\pnpm.cmd"),
        &roots
    ));
    // A random directory under a user-writable container is not trusted.
    assert!(!executable_under_roots(
        Path::new(r"C:\Users\tester\AppData\Local\random\npm.cmd"),
        &roots
    ));
    assert!(!executable_under_roots(
        Path::new(r"C:\Users\tester\AppData\Roaming\random\npm.cmd"),
        &roots
    ));
}

#[cfg(target_os = "windows")]
#[test]
fn trusted_tool_roots_exclude_bare_user_writable_containers() {
    use neati_platform::NativePlatformPaths;

    let roots = NativePlatformPaths::trusted_tool_roots(
        PlatformEnvironment::native().user_home().as_deref(),
    );

    // A bare user-writable container is never a trusted executable root:
    // trust comes from a named install directory below it, so no root may be
    // the container itself.
    for root in &roots {
        let text = root
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        let bare = [
            r"\appdata",
            r"\appdata\local",
            r"\appdata\roaming",
            r"\programdata",
        ]
        .iter()
        .any(|tail| text.ends_with(tail));
        assert!(!bare, "bare user-writable container is trusted: {root:?}");
    }

    // The documented %APPDATA%\npm install root stays trusted.
    let app_data = neati_platform::PlatformEnvironment::native()
        .roaming_app_data()
        .expect("a Windows session exposes the roaming application data root");
    let npm = app_data.join("npm");
    assert!(
        roots
            .iter()
            .any(|candidate| NativePlatformPaths::windows_path_eq(candidate, &npm)),
        "the documented %APPDATA%\\npm install root must stay trusted"
    );
}

#[test]
fn node_manager_roots_stay_inside_their_own_directories() {
    let home = PathBuf::from("/Users/tester");
    let roots = node_manager_roots(&home);
    assert!(roots.contains(&home.join(".nvm")));
    assert!(roots.contains(&home.join(".volta")));
    for root in &roots {
        assert!(root.starts_with(&home));
    }
}

#[test]
fn discovery_requires_one_absolute_utf8_path() {
    #[cfg(target_os = "windows")]
    let absolute = "C:\\Users\\테스트 사용자\\AppData\\Local\\uv\\cache\r\n";
    #[cfg(not(target_os = "windows"))]
    let absolute = "/Users/테스트 사용자/Library/Caches/uv\n";

    assert!(parse_discovered_path(absolute.as_bytes()).is_ok());
    assert!(parse_discovered_path(b"relative/cache\n").is_err());
    assert!(parse_discovered_path(b"/one\n/two\n").is_err());
    assert!(parse_discovered_path(&[0xff]).is_err());
}
