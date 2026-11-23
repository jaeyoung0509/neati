use super::*;
use neati_platform::description::KnownFolder;
use std::fs;

/// A simulated environment whose profile is the stated directory.
fn environment_with_home(home: &Path) -> PlatformEnvironment {
    PlatformEnvironment::simulated(path_algebra::PathFlavor::current()).with_home(home)
}

fn workspace_record(root: &Path) -> DeveloperWorkspaceRecord {
    let identity = identity_from_path(root).unwrap();
    DeveloperWorkspaceRecord {
        workspace: DeveloperWorkspace {
            id: Uuid::new_v4().to_string(),
            name: root.file_name().unwrap().to_string_lossy().into_owned(),
            display_path: root.to_string_lossy().into_owned(),
            path: root.to_string_lossy().into_owned(),
        },
        path: root.to_path_buf(),
        identity,
        created_at: unix_timestamp(),
        whole_home: false,
    }
}

#[test]
fn scanner_descends_into_unrecognized_output_wrappers() {
    let temp = tempfile::tempdir().unwrap();
    let environment = environment_with_home(temp.path());
    let root = temp.path().join("workspace");
    for wrapper in ["dist", "out", ".next", ".nuxt", ".svelte-kit"] {
        let project = root.join(wrapper).join("real-project");
        fs::create_dir_all(project.join("target")).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(project.join("target/output.bin"), [1u8; 128]).unwrap();
        assert!(!should_skip_discovery_directory(wrapper));
    }
    let workspace = workspace_record(&root);
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment,
        &[workspace],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert_eq!(inventory.records.len(), 5);
    for record in inventory.records.values() {
        assert_eq!(record.artifact.kind, DeveloperArtifactKind::CargoTarget);
        assert_eq!(record.artifact_relative, Path::new("target"));
        assert_eq!(record.project_root.file_name().unwrap(), "real-project");
    }
}

#[test]
fn developer_inventory_is_stale_when_the_clock_moves_backwards() {
    let inventory = DeveloperArtifactInventory {
        rules: crate::models::CleanupRules::default(),
        scan_id: "scan".to_string(),
        records: HashMap::new(),
        workspace_ids: Vec::new(),
        created_at: 1_000,
        discovered_count: 0,
        measured_count: 0,
        skipped_entries: 0,
        cancelled: false,
        truncated: false,
        uninspected: Vec::new(),
    };

    assert!(inventory.is_fresh_at(1_899));
    assert!(!inventory.is_fresh_at(1_900));
    assert!(!inventory.is_fresh_at(999));
}

#[test]
fn recognizes_ecosystem_markers_without_age_gates() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    let workspace = workspace_record(temp.path());
    let candidate = recognize_artifact(&workspace, &project, "target").unwrap();
    assert_eq!(candidate.kind, DeveloperArtifactKind::CargoTarget);
    assert_eq!(candidate.ecosystem, DeveloperEcosystem::Rust);
}

#[test]
fn recognizes_java_kotlin_php_dotnet_and_script_markers() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace_record(temp.path());

    let gradle = temp.path().join("kotlin");
    fs::create_dir_all(gradle.join("build")).unwrap();
    fs::write(gradle.join("build.gradle.kts"), "plugins {}\n").unwrap();
    assert_eq!(
        recognize_artifact(&workspace, &gradle, "build")
            .unwrap()
            .ecosystem,
        DeveloperEcosystem::Kotlin
    );

    let php = temp.path().join("php");
    fs::create_dir_all(php.join("vendor")).unwrap();
    fs::write(php.join("composer.json"), "{}\n").unwrap();
    fs::write(php.join("vendor/autoload.php"), "<?php\n").unwrap();
    assert_eq!(
        recognize_artifact(&workspace, &php, "vendor").unwrap().kind,
        DeveloperArtifactKind::ComposerVendor
    );

    let dotnet = temp.path().join("dotnet");
    fs::create_dir_all(dotnet.join("bin")).unwrap();
    fs::write(dotnet.join("demo.csproj"), "<Project />\n").unwrap();
    assert_eq!(
        recognize_artifact(&workspace, &dotnet, "bin")
            .unwrap()
            .ecosystem,
        DeveloperEcosystem::Dotnet
    );
}

#[test]
fn recognizes_scala_clojure_haskell_erlang_zig_and_c_markers() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace_record(temp.path());
    let fixtures = [
        ("scala", "target", "build.sbt", DeveloperEcosystem::Scala),
        ("clojure", "target", "deps.edn", DeveloperEcosystem::Clojure),
        (
            "haskell",
            ".stack-work",
            "stack.yaml",
            DeveloperEcosystem::Haskell,
        ),
        (
            "erlang",
            "_build",
            "rebar.config",
            DeveloperEcosystem::Erlang,
        ),
        ("zig", ".zig-cache", "build.zig", DeveloperEcosystem::Zig),
    ];
    for (name, artifact, marker, ecosystem) in fixtures {
        let project = temp.path().join(name);
        fs::create_dir_all(project.join(artifact)).unwrap();
        fs::write(project.join(marker), "fixture\n").unwrap();
        assert_eq!(
            recognize_artifact(&workspace, &project, artifact)
                .unwrap()
                .ecosystem,
            ecosystem
        );
    }

    let c_project = temp.path().join("c-project");
    fs::create_dir_all(c_project.join("build")).unwrap();
    fs::write(c_project.join("CMakeLists.txt"), "project(c_demo C)\n").unwrap();
    fs::write(c_project.join("main.c"), "int main(void) { return 0; }\n").unwrap();
    fs::write(c_project.join("build/CMakeCache.txt"), "fixture\n").unwrap();
    assert_eq!(
        recognize_artifact(&workspace, &c_project, "build")
            .unwrap()
            .ecosystem,
        DeveloperEcosystem::C
    );
}

#[cfg(unix)]
#[test]
fn scanner_streams_measured_candidates_from_an_explicit_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let environment = environment_with_home(temp.path());
    let workspace_path = temp.path().join("workspace");
    let rust_project = workspace_path.join("rust-app");
    let node_project = workspace_path.join("web-app");
    fs::create_dir_all(rust_project.join("target")).unwrap();
    fs::create_dir_all(node_project.join("node_modules")).unwrap();
    fs::write(rust_project.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    fs::write(rust_project.join("target/output.bin"), [1u8]).unwrap();
    fs::write(node_project.join("package.json"), "{}\n").unwrap();
    fs::write(node_project.join("node_modules/package.json"), "{}\n").unwrap();
    let workspace = workspace_record(&workspace_path);
    let mut events = Vec::new();
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment,
        std::slice::from_ref(&workspace),
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |event| events.push(event),
    )
    .unwrap();

    assert_eq!(inventory.records.len(), 2);
    assert!(events
        .iter()
        .any(|event| matches!(event, DeveloperArtifactScanEvent::ArtifactFound { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event, DeveloperArtifactScanEvent::Finished { .. })));
}

#[test]
fn whole_home_scan_finds_projects_and_bypasses_protected_trees() {
    let temp = tempfile::tempdir().unwrap();
    let safe_project = temp.path().join("work/rust-app");
    fs::create_dir_all(safe_project.join("target")).unwrap();
    fs::write(safe_project.join("Cargo.toml"), "[package]\nname='safe'\n").unwrap();
    fs::write(safe_project.join("target/output.bin"), [1u8]).unwrap();

    for protected in ["Library", ".ssh"] {
        let project = temp.path().join(protected).join("hidden-project");
        fs::create_dir_all(project.join("target")).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname='hidden'\n").unwrap();
        fs::write(project.join("target/output.bin"), [1u8]).unwrap();
    }
    let app = temp
        .path()
        .join("Applications/Demo.app/Contents/Resources/app");
    fs::create_dir_all(app.join("node_modules")).unwrap();
    fs::write(app.join("package.json"), "{}\n").unwrap();
    fs::write(app.join("node_modules/package.json"), "{}\n").unwrap();

    let mut workspace = workspace_record(temp.path());
    workspace.whole_home = true;
    let environment = environment_with_home(temp.path());
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment,
        std::slice::from_ref(&workspace),
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();

    assert_eq!(inventory.records.len(), 1);
    let artifact = &inventory.records.values().next().unwrap().artifact;
    assert_eq!(artifact.project_name, "rust-app");
    assert_eq!(artifact.kind, DeveloperArtifactKind::CargoTarget);
    assert!(inventory.skipped_entries >= 3);
}

/// A whole-home workspace over the stated directory.
fn whole_home_workspace(root: &Path) -> DeveloperWorkspaceRecord {
    let mut workspace = workspace_record(root);
    workspace.whole_home = true;
    workspace
}

/// An environment that states `root/Downloads` as the platform's Downloads
/// folder, the way a real profile resolves it.
fn environment_with_downloads(root: &Path) -> PlatformEnvironment {
    environment_with_home(root).with_known_folder(KnownFolder::Downloads, root.join("Downloads"))
}

/// Seeds a recognizable Node project inside `Downloads`, so a scan that
/// inspects the folder has something measurable to find.
fn seed_downloads_project(root: &Path, name: &str) {
    let project = root.join("Downloads").join(name);
    fs::create_dir_all(project.join("node_modules")).unwrap();
    fs::write(project.join("package.json"), "{}\n").unwrap();
    fs::write(project.join("node_modules/package.json"), "{}\n").unwrap();
}

fn scan_recording_events(
    environment: &PlatformEnvironment,
    workspace: &DeveloperWorkspaceRecord,
    downloads_access: FolderAccess,
) -> (DeveloperArtifactInventory, Vec<DeveloperArtifactScanEvent>) {
    let mut events = Vec::new();
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        environment,
        std::slice::from_ref(workspace),
        downloads_access,
        Arc::new(AtomicBool::new(false)),
        |event| events.push(event),
    )
    .unwrap();
    (inventory, events)
}

#[test]
fn whole_home_scan_inspects_downloads_when_access_is_granted() {
    let temp = tempfile::tempdir().unwrap();
    seed_downloads_project(temp.path(), "cloned-app");
    let environment = environment_with_downloads(temp.path());
    let workspace = whole_home_workspace(temp.path());

    let (inventory, events) =
        scan_recording_events(&environment, &workspace, FolderAccess::Granted);

    assert_eq!(inventory.records.len(), 1);
    let artifact = &inventory.records.values().next().unwrap().artifact;
    assert_eq!(artifact.status, DeveloperArtifactStatus::SafetyBlocked);
    assert_eq!(artifact.project_name, "cloned-app");
    assert_eq!(artifact.kind, DeveloperArtifactKind::NodeModules);
    assert!(artifact.allocated_bytes > 0);
    assert!(inventory.uninspected.is_empty());
    assert!(!events
        .iter()
        .any(|event| matches!(event, DeveloperArtifactScanEvent::Uninspected { .. })));
}

#[test]
fn denied_downloads_is_reported_as_uninspected_and_the_scan_completes() {
    let temp = tempfile::tempdir().unwrap();
    seed_downloads_project(temp.path(), "cloned-app");
    let safe_project = temp.path().join("work/rust-app");
    fs::create_dir_all(safe_project.join("target")).unwrap();
    fs::write(safe_project.join("Cargo.toml"), "[package]\nname='safe'\n").unwrap();
    fs::write(safe_project.join("target/output.bin"), [1u8]).unwrap();
    let environment = environment_with_downloads(temp.path());
    let workspace = whole_home_workspace(temp.path());

    let (inventory, events) = scan_recording_events(&environment, &workspace, FolderAccess::Denied);

    assert!(!inventory.cancelled);
    assert_eq!(inventory.records.len(), 1);
    let record = inventory.records.values().next().unwrap();
    assert_eq!(record.artifact.project_name, "rust-app");
    assert_eq!(inventory.uninspected.len(), 1);
    let entry = &inventory.uninspected[0];
    assert_eq!(entry.name, "Downloads");
    assert_eq!(PathBuf::from(&entry.path), temp.path().join("Downloads"));
    assert_eq!(
        entry.reason,
        DeveloperArtifactUninspectedReason::PermissionDenied
    );
    assert!(entry.retryable);
    assert!(inventory.skipped_entries >= 1);
    assert!(events.iter().any(|event| matches!(
        event,
        DeveloperArtifactScanEvent::Uninspected {
            name,
            reason: DeveloperArtifactUninspectedReason::PermissionDenied,
            retryable: true,
            ..
        } if name == "Downloads"
    )));
    let finished = events
        .iter()
        .find_map(|event| match event {
            DeveloperArtifactScanEvent::Finished { result } => Some(result.clone()),
            _ => None,
        })
        .expect("the scan must finish after a denial");
    assert_eq!(finished.uninspected, inventory.uninspected);
}

#[test]
fn unreadable_downloads_is_not_misreported_as_a_permission_denial() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("Downloads")).unwrap();
    let environment = environment_with_downloads(temp.path());
    let workspace = whole_home_workspace(temp.path());

    let (inventory, _) = scan_recording_events(&environment, &workspace, FolderAccess::Unreadable);

    assert_eq!(inventory.uninspected.len(), 1);
    assert_eq!(
        inventory.uninspected[0].reason,
        DeveloperArtifactUninspectedReason::Unreadable
    );
    assert!(!inventory.uninspected[0].retryable);
}

/// First-run denial and the Allow-then-rescan retry path, driven on the
/// same home so the retry cannot pass by scanning a fresh fixture.
#[cfg(target_os = "macos")]
#[test]
fn macos_downloads_denial_then_retry_inspects_the_same_home() {
    let temp = tempfile::tempdir().unwrap();
    seed_downloads_project(temp.path(), "cloned-app");
    let environment = environment_with_downloads(temp.path());
    let workspace = whole_home_workspace(temp.path());

    let (denied, _) = scan_recording_events(&environment, &workspace, FolderAccess::Denied);
    assert_eq!(denied.records.len(), 0);
    assert_eq!(denied.uninspected.len(), 1);
    assert!(denied.uninspected[0].retryable);

    let (retried, _) = scan_recording_events(&environment, &workspace, FolderAccess::Granted);
    assert_eq!(retried.records.len(), 1);
    let record = retried.records.values().next().unwrap();
    assert_eq!(record.artifact.project_name, "cloned-app");
    assert!(retried.uninspected.is_empty());
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_reported_as_a_retryable_permission_denial() {
    let temp = tempfile::tempdir().unwrap();
    let safe_project = temp.path().join("work/rust-app");
    fs::create_dir_all(safe_project.join("target")).unwrap();
    fs::write(safe_project.join("Cargo.toml"), "[package]\nname='safe'\n").unwrap();
    fs::write(safe_project.join("target/output.bin"), [1u8]).unwrap();
    let locked = temp.path().join("work/locked");
    fs::create_dir_all(locked.join("target")).unwrap();
    fs::write(locked.join("Cargo.toml"), "[package]\nname='locked'\n").unwrap();
    let restore = PermissionRestore::make_unreadable(&locked);
    let environment = environment_with_home(temp.path());
    let workspace = whole_home_workspace(temp.path());

    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment,
        std::slice::from_ref(&workspace),
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();

    assert!(!inventory.cancelled);
    assert_eq!(inventory.records.len(), 1);
    let record = inventory.records.values().next().unwrap();
    assert_eq!(record.artifact.project_name, "rust-app");
    assert_eq!(inventory.uninspected.len(), 1);
    assert_eq!(PathBuf::from(&inventory.uninspected[0].path), locked);
    assert_eq!(inventory.uninspected[0].name, "locked");
    assert_eq!(
        inventory.uninspected[0].reason,
        DeveloperArtifactUninspectedReason::PermissionDenied
    );
    assert!(inventory.uninspected[0].retryable);
    assert!(inventory.skipped_entries >= 1);
    drop(restore);
}

/// Restores a directory's mode when the test ends, so the temporary
/// directory can still be removed after an assertion panics.
#[cfg(unix)]
struct PermissionRestore {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl PermissionRestore {
    fn make_unreadable(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).unwrap().permissions().mode();
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        Self {
            path: path.to_path_buf(),
            mode,
        }
    }
}

#[cfg(unix)]
impl Drop for PermissionRestore {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}

#[test]
fn probing_without_a_stated_downloads_folder_reports_no_gate() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        probe_downloads_access(&environment_with_home(temp.path())),
        FolderAccess::NotGated
    );
    // Stated but absent: a missing folder raises no prompt.
    let absent = environment_with_downloads(temp.path());
    assert_eq!(probe_downloads_access(&absent), FolderAccess::NotGated);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_probe_reports_granted_and_denied_for_a_stated_downloads_folder() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("Downloads");
    fs::create_dir_all(&downloads).unwrap();
    let environment = environment_with_downloads(temp.path());
    assert_eq!(probe_downloads_access(&environment), FolderAccess::Granted);

    let mode = fs::metadata(&downloads).unwrap().permissions().mode();
    let restore = PermissionRestore {
        path: downloads.clone(),
        mode,
    };
    fs::set_permissions(&downloads, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(probe_downloads_access(&environment), FolderAccess::Denied);
    drop(restore);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_probe_keeps_non_permission_failures_distinct() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Downloads"), b"not a directory").unwrap();
    let environment = environment_with_downloads(temp.path());

    assert_eq!(
        probe_downloads_access(&environment),
        FolderAccess::Unreadable
    );
}

#[test]
fn same_named_directories_without_evidence_are_not_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace_record(temp.path());
    let project = temp.path().join("random");
    fs::create_dir_all(project.join("build")).unwrap();
    fs::create_dir_all(project.join("vendor")).unwrap();
    assert!(recognize_artifact(&workspace, &project, "build").is_none());
    assert!(recognize_artifact(&workspace, &project, "vendor").is_none());
}

#[test]
fn ancestor_markers_do_not_authorize_nested_same_named_source_directories() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace_record(temp.path());
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    let source = temp.path().join("src");
    fs::create_dir_all(source.join("target")).unwrap();
    fs::write(source.join("target/module.rs"), "pub fn keep_me() {}\n").unwrap();

    assert!(recognize_artifact(&workspace, &source, "target").is_none());
}

#[test]
fn ambiguous_environment_and_build_names_need_generated_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace_record(temp.path());

    let python = temp.path().join("python");
    fs::create_dir_all(python.join("venv")).unwrap();
    fs::write(python.join("pyproject.toml"), "[project]\nname='demo'\n").unwrap();
    assert!(recognize_artifact(&workspace, &python, "venv").is_none());
    fs::write(python.join("venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    assert!(recognize_artifact(&workspace, &python, "venv").is_some());

    let php = temp.path().join("php");
    fs::create_dir_all(php.join("vendor")).unwrap();
    fs::write(php.join("composer.json"), "{}\n").unwrap();
    assert!(recognize_artifact(&workspace, &php, "vendor").is_none());
    fs::write(php.join("vendor/autoload.php"), "<?php\n").unwrap();
    assert!(recognize_artifact(&workspace, &php, "vendor").is_some());

    let cmake = temp.path().join("cmake");
    fs::create_dir_all(cmake.join("build")).unwrap();
    fs::write(cmake.join("CMakeLists.txt"), "project(demo)\n").unwrap();
    assert!(recognize_artifact(&workspace, &cmake, "build").is_none());
    fs::write(cmake.join("build/CMakeCache.txt"), "# generated\n").unwrap();
    assert!(recognize_artifact(&workspace, &cmake, "build").is_some());
}

#[test]
fn zero_byte_artifact_directories_are_not_reported() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    let workspace = workspace_record(temp.path());
    let candidate = recognize_artifact(&workspace, &project, "target").unwrap();
    let stats = measure_tree(
        &candidate.path,
        workspace.identity.device(),
        &AtomicBool::new(false),
        0,
    );

    assert!(record_from_measurement(
        candidate,
        stats,
        ArtifactOwnershipEvidence::VerifiedGenerated
    )
    .is_none());
}

#[test]
fn measurement_collects_size_count_and_newest_mtime_in_one_pass() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("a.bin");
    fs::write(&file, [1u8, 2, 3, 4]).unwrap();
    let device = identity_from_path(temp.path()).unwrap().device();
    let stats = measure_tree(temp.path(), device, &AtomicBool::new(false), 0);
    assert!(stats.complete);
    assert_eq!(stats.file_count, 1);
    assert_eq!(stats.logical_bytes, 4);
    assert!(stats.newest_mtime.is_some());
}

#[test]
fn partial_measurement_is_typed_as_manual_cleanup_eligible() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    fs::write(project.join("target/output.bin"), [1u8]).unwrap();
    let workspace = workspace_record(temp.path());
    let candidate = recognize_artifact(&workspace, &project, "target").unwrap();
    let mut stats = measure_tree(
        &candidate.path,
        workspace.identity.device(),
        &AtomicBool::new(false),
        0,
    );
    stats.complete = false;

    let record = record_from_measurement(
        candidate,
        stats,
        ArtifactOwnershipEvidence::VerifiedGenerated,
    )
    .unwrap();
    assert_eq!(
        record.artifact.status,
        DeveloperArtifactStatus::MeasurementIncomplete
    );
    assert!(record.artifact.incomplete_reason.is_some());
    assert!(record.artifact.status.allows_manual_cleanup());
}

#[cfg(unix)]
#[test]
fn workspace_validation_accepts_profile_children_and_rejects_home_itself() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("profile");
    let workspace = home.join("src");
    fs::create_dir_all(&workspace).unwrap();
    let home = fs::canonicalize(&home).unwrap();
    let workspace = home.join("src");
    let environment = environment_with_home(&home);

    assert_eq!(
        validate_workspace_root(&environment, &workspace).unwrap(),
        workspace
    );
    assert!(validate_workspace_root(&environment, &home).is_err());

    {
        use std::os::unix::fs::symlink;
        let linked = home.join("linked");
        symlink(&workspace, &linked).unwrap();
        assert!(validate_workspace_root(&environment, &linked).is_err());
    }
}

#[cfg(unix)]
#[test]
fn workspace_validation_accepts_roots_outside_the_profile() {
    // The common D:\dev-style layout must register under the remaining
    // safety validation instead of being refused for being outside home.
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("profile");
    let workspace = temp.path().join("dev");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let environment = environment_with_home(&home);

    assert!(validate_workspace_root(&environment, &workspace).is_ok());
    assert!(validate_workspace_root(&environment, temp.path()).is_ok());
}

#[test]
fn a_non_system_drive_and_a_unc_profile_are_valid_workspace_scopes() {
    // The rules are decided by the stated flavor, so a Windows drive that
    // is not `C:` and a UNC profile are provable on any runner.
    let flavor = path_algebra::PathFlavor::Windows;
    let home = Path::new(r"D:\Users\홍 길동");

    assert!(workspace_root_scope_refusal(flavor, Path::new(r"D:\dev\neati"), Some(home)).is_none());
    assert!(workspace_root_scope_refusal(flavor, Path::new(r"D:\"), Some(home)).is_some());
    assert!(workspace_root_scope_refusal(flavor, home, Some(home)).is_some());
    assert!(workspace_root_scope_refusal(
        flavor,
        Path::new(r"D:\Users\홍 길동\Documents"),
        Some(home)
    )
    .is_some());
    assert!(
        workspace_root_scope_refusal(flavor, Path::new(r"C:\Users\other\dev"), Some(home))
            .is_none()
    );

    let unc_home = Path::new(r"\\fileserver\profiles\me");
    assert!(workspace_root_scope_refusal(
        flavor,
        Path::new(r"\\fileserver\profiles\me\dev"),
        Some(unc_home)
    )
    .is_none());
    assert!(workspace_root_scope_refusal(flavor, unc_home, Some(unc_home)).is_some());
    assert!(workspace_root_scope_refusal(
        flavor,
        Path::new(r"\\fileserver\profiles\me\Documents"),
        Some(unc_home)
    )
    .is_some());
    // `C:` and `C:\` are the drive root under Windows rules.
    assert!(workspace_root_scope_refusal(flavor, Path::new("C:"), Some(home)).is_some());
    assert!(workspace_root_scope_refusal(flavor, Path::new(r"C:\"), Some(home)).is_some());
}

#[cfg(unix)]
#[test]
fn backend_owned_home_scope_registers_without_the_folder_picker() {
    let store = Mutex::new(HashMap::new());
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("profile");
    fs::create_dir_all(&home).unwrap();
    let environment = environment_with_home(&home);
    let canonical_home = fs::canonicalize(&home).unwrap();

    let workspace = register_home_workspace(&environment, &store).unwrap();
    let workspaces = store.lock().expect("store poisoned");
    let record = workspaces.get(&workspace.id).unwrap();

    assert_eq!(record.path, canonical_home);
    assert!(record.whole_home);
    assert_eq!(workspace.name, "This Computer");
}

#[cfg(target_os = "windows")]
#[test]
fn workspace_display_path_hides_the_windows_verbatim_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let canonical = fs::canonicalize(temp.path()).unwrap();
    let store = Mutex::new(HashMap::new());
    let record = store_workspace(canonical, "Workspace".to_string(), false, &store).unwrap();

    assert!(!record.workspace.display_path.starts_with(r"\\?\"));
    assert_eq!(
        PathBuf::from(&record.workspace.display_path),
        neati_platform::path_algebra::normalize_lexical(&record.path)
    );
}

#[test]
fn workspace_identity_ignores_normal_directory_metadata_changes() {
    let temp = tempfile::tempdir().unwrap();
    let original = identity_from_path(temp.path()).unwrap();
    let registered = original
        .with_size(original.size().saturating_add(1))
        .with_modified(original.modified().map(|value| value.saturating_sub(1)));

    let current = identity_from_path(temp.path()).unwrap();
    assert!(same_workspace_directory_identity(&current, &registered));
}

#[cfg(unix)]
#[test]
fn workspace_identity_rejects_a_replaced_directory() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let original = temp.path().join("original-workspace");
    fs::create_dir(&workspace).unwrap();
    let registered = identity_from_path(&workspace).unwrap();

    fs::rename(&workspace, &original).unwrap();
    fs::create_dir(&workspace).unwrap();

    let current = identity_from_path(&workspace).unwrap();
    assert!(!same_workspace_directory_identity(&current, &registered));
}

#[cfg(unix)]
#[test]
fn nested_symlink_is_not_followed_and_remains_manually_reviewable() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let target = project.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
    fs::write(target.join("output.bin"), [1u8, 2, 3]).unwrap();
    fs::write(outside.path().join("secret"), [1u8]).unwrap();
    symlink(outside.path(), target.join("linked")).unwrap();

    let workspace = workspace_record(temp.path());
    let candidate = recognize_artifact(&workspace, &project, "target").unwrap();
    let stats = measure_tree(
        &candidate.path,
        workspace.identity.device(),
        &AtomicBool::new(false),
        0,
    );
    assert!(!stats.complete);
    assert!(!stats.safety_blocked);
    assert_eq!(stats.file_count, 2);

    let record = record_from_measurement(
        candidate,
        stats,
        ArtifactOwnershipEvidence::VerifiedGenerated,
    )
    .unwrap();
    assert_eq!(
        record.artifact.status,
        DeveloperArtifactStatus::MeasurementIncomplete
    );
    assert!(record.artifact.status.allows_manual_cleanup());
}

#[cfg(unix)]
#[test]
fn filesystem_boundary_is_typed_as_safety_blocked() {
    let temp = tempfile::tempdir().unwrap();
    let actual_device = identity_from_path(temp.path()).unwrap().device();
    let stats = measure_tree(
        temp.path(),
        actual_device.wrapping_add(1),
        &AtomicBool::new(false),
        0,
    );
    assert!(!stats.complete);
    assert!(stats.safety_blocked);
}

#[test]
fn developer_artifact_identity_roundtrips_and_is_never_zero() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("my_project");
    let target = project.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='my_project'\n").unwrap();

    let workspace = workspace_record(temp.path());
    assert!(!workspace.identity.is_unknown());
    assert_eq!(
        identity_from_path(temp.path()),
        Some(workspace.identity.clone())
    );

    fs::write(target.join("output.bin"), [1u8]).unwrap();

    let candidate = recognize_artifact(&workspace, &project, "target").unwrap();
    let stats = measure_tree(
        &candidate.path,
        workspace.identity.device(),
        &AtomicBool::new(false),
        0,
    );
    let record = record_from_measurement(
        candidate,
        stats,
        ArtifactOwnershipEvidence::VerifiedGenerated,
    )
    .unwrap();
    assert!(!record.identity.is_unknown());
    assert_eq!(
        identity_from_path(&record.path),
        Some(record.identity.clone())
    );
    assert!(same_workspace_directory_identity(
        &record.identity,
        &identity_from_path(&record.path).unwrap()
    ));
}

#[test]
fn framework_observations_stay_unselected_and_preserve_protected_bytes_and_nested_packages() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    for (name, output, dependency) in [
        ("kit", ".svelte-kit", "@sveltejs/kit"),
        ("next", ".next", "next"),
    ] {
        let project = root.join("packages").join(name);
        fs::create_dir_all(project.join(output)).unwrap();
        fs::write(
            project.join("package.json"),
            format!(r#"{{"dependencies":{{"{dependency}":"1.0.0"}}}}"#),
        )
        .unwrap();
        fs::write(project.join(output).join("generated.bin"), [1u8; 100]).unwrap();
        fs::write(
            project.join(if name == "next" {
                "next.config.js"
            } else {
                "svelte.config.js"
            }),
            "throw new Error('configuration must not execute')",
        )
        .unwrap();
    }
    let protected = root.join("packages/protected");
    fs::create_dir_all(protected.join(".next/deploy")).unwrap();
    fs::write(
        protected.join("package.json"),
        r#"{"dependencies":{"next":"16"}}"#,
    )
    .unwrap();
    fs::write(
        protected.join(".next/deploy/example-keypair.json"),
        [3u8; 20],
    )
    .unwrap();
    let vendor = root.join("node_modules/hidden");
    fs::create_dir_all(vendor.join(".next")).unwrap();
    fs::write(
        vendor.join("package.json"),
        r#"{"dependencies":{"next":"16"}}"#,
    )
    .unwrap();
    fs::write(vendor.join(".next/generated.bin"), [4u8; 50]).unwrap();
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment_with_home(temp.path()),
        &[workspace_record(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert_eq!(inventory.records.len(), 3);
    assert_eq!(
        inventory
            .records
            .values()
            .filter(|record| record.artifact.status == DeveloperArtifactStatus::ObservationOnly)
            .count(),
        2
    );
    assert!(inventory
        .records
        .values()
        .all(|record| !record.artifact.selected_by_default));
    let protected = inventory
        .records
        .values()
        .find(|record| record.artifact.project_name == "protected")
        .unwrap();
    assert_eq!(
        protected.artifact.ownership,
        ArtifactOwnershipEvidence::DeploymentKeyMaterial
    );
    assert_eq!(protected.artifact.logical_bytes, 20);
    assert_eq!(
        protected.artifact.status,
        DeveloperArtifactStatus::SafetyBlocked
    );
    assert!(!protected.artifact.status.allows_manual_cleanup());
}

#[test]
fn overlapping_workspace_registrations_cannot_duplicate_framework_units() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    let project = root.join("packages/project");
    fs::create_dir_all(project.join(".next")).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"dependencies":{"next":"16"}}"#,
    )
    .unwrap();
    fs::write(project.join(".next/generated.bin"), [1u8; 100]).unwrap();
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment_with_home(temp.path()),
        &[workspace_record(&root), workspace_record(&project)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert_eq!(inventory.discovered_count, 1);
    assert_eq!(inventory.records.len(), 1);
    assert_eq!(
        result_from_inventory(&inventory).items[0].logical_bytes,
        100
    );
}

#[test]
fn complete_measurement_cannot_override_incomplete_ownership_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
    fs::write(project.join("target/output.bin"), [1u8; 100]).unwrap();
    let candidate = recognize_artifact(&workspace_record(temp.path()), &project, "target").unwrap();
    let stats = measure_tree(
        &candidate.path,
        candidate.workspace.identity.device(),
        &AtomicBool::new(false),
        0,
    );
    assert!(stats.complete);
    let evidence = ArtifactOwnershipEvidence::Incomplete(
        crate::models::ArtifactOwnershipUncertainty::TimedOut,
    );
    let record = record_from_measurement(candidate, stats, evidence).unwrap();
    assert_eq!(record.artifact.logical_bytes, 100);
    assert_eq!(record.artifact.ownership, evidence);
    assert_eq!(
        record.artifact.status,
        DeveloperArtifactStatus::SafetyBlocked
    );
}

#[test]
fn framework_candidate_budget_and_cancelled_discovery_remain_explicit() {
    let temp = tempfile::tempdir().unwrap();
    for index in 0..=MAX_CANDIDATES {
        let project = temp.path().join(format!("package-{index:03}"));
        fs::create_dir_all(project.join(".next")).unwrap();
        fs::write(
            project.join("package.json"),
            r#"{"dependencies":{"next":"15"}}"#,
        )
        .unwrap();
        fs::write(project.join(".next/output.bin"), b"observed").unwrap();
    }
    let environment = environment_with_home(temp.path());
    let workspace = workspace_record(temp.path());
    let mut candidates = Vec::new();
    let mut discovered = 0;
    let mut skipped = 0;
    let mut truncated = false;
    let mut events = Vec::new();
    discover_workspace(
        &environment,
        &workspace,
        FolderAccess::NotGated,
        &AtomicBool::new(false),
        &mut candidates,
        &mut discovered,
        &mut skipped,
        &mut truncated,
        &mut HashSet::new(),
        &mut Vec::new(),
        &mut HashSet::new(),
        &mut |event| events.push(event),
    );
    assert_eq!(candidates.len(), MAX_CANDIDATES);
    assert_eq!(discovered, MAX_CANDIDATES as u64);
    assert_eq!(events.len(), MAX_CANDIDATES);
    assert!(truncated);
    assert!(candidates
        .iter()
        .all(|candidate| artifact_is_framework_parent(candidate.kind)));

    candidates.clear();
    events.clear();
    discovered = 0;
    truncated = false;
    discover_workspace(
        &environment,
        &workspace,
        FolderAccess::NotGated,
        &AtomicBool::new(true),
        &mut candidates,
        &mut discovered,
        &mut skipped,
        &mut truncated,
        &mut HashSet::new(),
        &mut Vec::new(),
        &mut HashSet::new(),
        &mut |event| events.push(event),
    );
    assert!(candidates.is_empty());
    assert!(events.is_empty());
    assert_eq!(discovered, 0);
    assert!(!truncated);
}

#[test]
fn terraform_folder_with_a_selected_workspace_is_observed_but_never_cleanable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    for (name, workspace) in [("plain", None), ("staging", Some("staging"))] {
        let project = root.join(name);
        let terraform = project.join(".terraform");
        fs::create_dir_all(terraform.join("providers")).unwrap();
        fs::write(project.join("main.tf"), "terraform {}\n").unwrap();
        fs::write(project.join(".terraform.lock.hcl"), "# lock\n").unwrap();
        fs::write(terraform.join("providers/plugin.bin"), [5u8; 40]).unwrap();
        if let Some(workspace) = workspace {
            fs::write(terraform.join("environment"), workspace).unwrap();
        }
    }
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment_with_home(temp.path()),
        &[workspace_record(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    let record = |project: &str| {
        inventory
            .records
            .values()
            .find(|record| record.artifact.project_name == project)
            .unwrap_or_else(|| panic!("{project} .terraform is reported"))
            .artifact
            .clone()
    };
    let plain = record("plain");
    assert_eq!(plain.kind, DeveloperArtifactKind::TerraformCache);
    assert_eq!(
        plain.ownership,
        ArtifactOwnershipEvidence::VerifiedGenerated
    );

    let selected = record("staging");
    assert_eq!(selected.kind, DeveloperArtifactKind::TerraformCache);
    assert_eq!(
        selected.ownership,
        ArtifactOwnershipEvidence::TerraformWorkspaceSelection
    );
    assert_eq!(selected.status, DeveloperArtifactStatus::SafetyBlocked);
    assert!(!selected.status.allows_manual_cleanup());
    assert!(!selected.selected_by_default);
    assert_eq!(selected.logical_bytes, 40 + "staging".len() as u64);
}

fn project(root: &Path, marker: &str) {
    fs::create_dir_all(root).unwrap();
    let marker_path = root.join(marker);
    if marker == ".git" {
        fs::create_dir_all(marker_path).unwrap();
    } else {
        fs::write(marker_path, "fixture\n").unwrap();
    }
}

#[test]
fn saved_project_folders_restore_and_report_refusals() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    let environment = environment_with_home(&home);
    project(&home.join("Myproject/app"), "Cargo.toml");
    let saved = vec![
        home.join("Myproject").to_string_lossy().into_owned(),
        home.join("gone").to_string_lossy().into_owned(),
        home.to_string_lossy().into_owned(),
        home.join("Myproject").to_string_lossy().into_owned(),
    ];
    let store = Mutex::new(HashMap::new());
    let restore = restore_workspaces(&environment, &saved, &store);

    let names: Vec<&str> = restore
        .workspaces
        .iter()
        .map(|workspace| workspace.name.as_str())
        .collect();
    assert_eq!(names, ["Myproject", "This Computer"]);
    assert_eq!(
        restore.workspaces[0].path,
        home.join("Myproject").to_string_lossy()
    );
    assert_eq!(restore.refused.len(), 1);
    assert!(restore.refused[0].path.ends_with("gone"));
    assert!(!restore.refused[0].reason.is_empty());
    let records = store.lock().unwrap();
    assert_eq!(records.len(), 2, "a folder saved twice is registered once");
    assert!(records.values().any(|record| record.whole_home));
}

#[test]
fn suggestions_name_project_roots_only_and_skip_saved_folders() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    let environment = environment_with_home(&home);
    project(&home.join("Developer/one"), ".git");
    project(&home.join("Developer/two"), "package.json");
    project(&home.join("Myproject/app"), "Cargo.toml");
    // An empty root, a root whose children are not projects, and a link to a
    // real root are not suggested.
    fs::create_dir_all(home.join("Projects")).unwrap();
    fs::create_dir_all(home.join("repos/notes")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(home.join("Developer"), home.join("code")).unwrap();

    let suggestions = suggest_workspaces(&environment, &[], &[]);
    let found: Vec<(String, u32)> = suggestions
        .iter()
        .map(|suggestion| (suggestion.path.clone(), suggestion.project_count))
        .collect();
    assert_eq!(
        found,
        [
            (home.join("Developer").to_string_lossy().into_owned(), 2),
            (home.join("Myproject").to_string_lossy().into_owned(), 1),
        ]
    );

    let saved = [home.join("Myproject").to_string_lossy().into_owned()];
    let remaining = suggest_workspaces(&environment, &saved, &[]);
    assert_eq!(remaining.len(), 1);
    assert!(remaining[0].path.ends_with("Developer"));
}

#[test]
fn folders_agents_worked_in_are_suggested_first() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    let environment = environment_with_home(&home);
    // An agent worked deep inside a project under an unusual folder name.
    project(&home.join("work/clients/acme"), "package.json");
    fs::create_dir_all(home.join("work/clients/acme/src/lib")).unwrap();
    project(&home.join("Developer/one"), ".git");
    // A project directly in home is suggested as itself.
    project(&home.join("solo"), "Cargo.toml");
    // Hidden tool folders, linked worktrees, scratch folders and gone folders
    // are not suggested.
    project(&home.join(".codex/worktrees/abc/app"), "Cargo.toml");
    let worktree = home.join("work/clients/acme-wt");
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), "gitdir: /elsewhere\n").unwrap();
    fs::create_dir_all(home.join("scratch")).unwrap();

    let sessions = vec![
        (home.join("work/clients/acme/src/lib"), 5),
        (home.join("work/clients/acme"), 2),
        (home.join("solo"), 1),
        (home.join(".codex/worktrees/abc/app"), 9),
        (worktree.clone(), 4),
        (home.join("scratch"), 3),
        (home.join("gone/project"), 7),
        (home.clone(), 6),
    ];
    let suggestions = suggest_workspaces(&environment, &[], &sessions);
    let found: Vec<(String, u32)> = suggestions
        .iter()
        .map(|suggestion| (suggestion.path.clone(), suggestion.agent_sessions))
        .collect();
    assert_eq!(
        found,
        [
            (home.join("work/clients").to_string_lossy().into_owned(), 7),
            (home.join("solo").to_string_lossy().into_owned(), 1),
            (home.join("Developer").to_string_lossy().into_owned(), 0),
        ]
    );

    // A saved folder that covers a session folder hides it.
    let saved = [home.join("work").to_string_lossy().into_owned()];
    let remaining = suggest_workspaces(&environment, &saved, &sessions);
    assert!(remaining
        .iter()
        .all(|suggestion| !suggestion.path.contains("/work")));

    // Adding re-derives the suggestions, sessions included.
    let store = Mutex::new(HashMap::new());
    let clients = home.join("work/clients").to_string_lossy().into_owned();
    assert!(register_suggested_workspace(&environment, &clients, &[], &[], &store).is_err());
    assert!(register_suggested_workspace(&environment, &clients, &[], &sessions, &store).is_ok());
}

#[test]
fn only_a_current_suggestion_can_be_added_by_path() {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    let environment = environment_with_home(&home);
    project(&home.join("Myproject/app"), "Cargo.toml");
    project(&home.join("elsewhere/app"), "Cargo.toml");
    let store = Mutex::new(HashMap::new());

    let elsewhere = home.join("elsewhere").to_string_lossy().into_owned();
    assert!(register_suggested_workspace(&environment, &elsewhere, &[], &[], &store).is_err());
    assert!(store.lock().unwrap().is_empty());

    let suggested = home.join("Myproject").to_string_lossy().into_owned();
    let workspace =
        register_suggested_workspace(&environment, &suggested, &[], &[], &store).unwrap();
    assert_eq!(workspace.path, suggested);
    assert_eq!(store.lock().unwrap().len(), 1);
}

#[test]
fn saved_project_folder_list_is_bounded_and_absolute() {
    assert!(validate_saved_project_folders(&[]).is_ok());
    assert!(validate_saved_project_folders(&["/Users/someone/code".into()]).is_ok());
    assert!(validate_saved_project_folders(&["code".into()]).is_err());
    assert!(validate_saved_project_folders(&["/Users/someone/../other".into()]).is_err());
    let too_many: Vec<String> = (0..=MAX_SAVED_PROJECT_FOLDERS)
        .map(|index| format!("/Users/someone/p{index}"))
        .collect();
    assert!(validate_saved_project_folders(&too_many).is_err());
}

#[test]
fn only_idle_verified_build_output_is_preselected() {
    use neati_platform::open_files::OpenFileState;
    use std::sync::Mutex as StdMutex;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap().join("workspace");
    let rust = root.join("rust-app");
    fs::create_dir_all(rust.join("target/debug")).unwrap();
    fs::write(rust.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
    fs::write(rust.join("target/debug/app"), [7u8; 64]).unwrap();
    let node = root.join("web-app");
    fs::create_dir_all(node.join("node_modules/pkg")).unwrap();
    fs::write(node.join("package.json"), "{}").unwrap();
    fs::write(node.join("node_modules/pkg/index.js"), [8u8; 64]).unwrap();

    for (state, expected) in [
        (OpenFileState::Idle, true),
        (OpenFileState::InUse, false),
        (OpenFileState::Unknown, false),
    ] {
        let asked = StdMutex::new(Vec::new());
        let probe = |paths: &[PathBuf], _: &dyn Fn() -> bool| {
            asked.lock().unwrap().extend(paths.iter().cloned());
            vec![state; paths.len()]
        };
        let inventory = DeveloperArtifactScanner::scan_workspaces_with_use_probe(
            &environment_with_home(temp.path()),
            &[workspace_record(&root)],
            FolderAccess::NotGated,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            &probe,
        )
        .unwrap();
        let find = |kind: DeveloperArtifactKind| {
            inventory
                .records
                .values()
                .find(|record| record.artifact.kind == kind)
                .unwrap_or_else(|| panic!("{kind:?} is reported"))
                .artifact
                .clone()
        };
        let target = find(DeveloperArtifactKind::CargoTarget);
        assert_eq!(target.status, DeveloperArtifactStatus::Complete);
        assert_eq!(target.selected_by_default, expected, "{state:?}");
        // A dependency store is never preselected and never even asked about.
        assert!(!find(DeveloperArtifactKind::NodeModules).selected_by_default);
        let asked = asked.lock().unwrap();
        assert_eq!(asked.as_slice(), [rust.join("target")], "{state:?}");
    }
}

#[test]
fn only_compiler_build_outputs_qualify_for_preselection() {
    use preselection::is_build_output;
    assert!(is_build_output(DeveloperArtifactKind::CargoTarget));
    assert!(is_build_output(DeveloperArtifactKind::GradleBuild));
    for kind in [
        DeveloperArtifactKind::NodeModules,
        DeveloperArtifactKind::PythonVenv,
        DeveloperArtifactKind::ComposerVendor,
        DeveloperArtifactKind::RubyBundle,
        DeveloperArtifactKind::ElixirDeps,
        DeveloperArtifactKind::GoModuleCache,
        DeveloperArtifactKind::FlutterTooling,
        DeveloperArtifactKind::NextOutput,
        DeveloperArtifactKind::SvelteKitOutput,
        DeveloperArtifactKind::TerraformCache,
        DeveloperArtifactKind::DotnetBin,
    ] {
        assert!(!is_build_output(kind), "{kind:?}");
    }
}

mod cleanup_rules {
    //! One-button inclusion under the person's cleanup rules (#504).
    use super::*;
    use crate::models::{CleanInclusion, CleanupRules, IdleRule};
    use neati_platform::open_files::OpenFileState;
    use preselection::Selection;

    const DAY: u64 = 24 * 60 * 60;
    const NOW: u64 = 2_000_000_000;

    struct Fixture {
        _temp: tempfile::TempDir,
        home: PathBuf,
        root: PathBuf,
        node: PathBuf,
        rust: PathBuf,
    }

    fn fixture(lockfile: bool) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(temp.path()).unwrap();
        let root = home.join("workspace");
        let node = root.join("web-app");
        fs::create_dir_all(node.join("node_modules/pkg")).unwrap();
        fs::write(node.join("package.json"), "{}").unwrap();
        if lockfile {
            fs::write(node.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        }
        fs::write(node.join("node_modules/pkg/index.js"), [8u8; 64]).unwrap();
        let rust = root.join("rust-app");
        fs::create_dir_all(rust.join("target/debug")).unwrap();
        fs::write(rust.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(rust.join("target/debug/app"), [7u8; 64]).unwrap();
        Fixture {
            _temp: temp,
            home,
            root,
            node,
            rust,
        }
    }

    fn dependency_rule(enabled: bool, idle_days: u16) -> CleanupRules {
        CleanupRules {
            build_output_min_idle_days: 0,
            dependency_stores: IdleRule { enabled, idle_days },
            ..CleanupRules::default()
        }
    }

    fn scan(
        fixture: &Fixture,
        rules: CleanupRules,
        idle_days_ago: Option<u64>,
        use_state: OpenFileState,
        working_directories: Vec<PathBuf>,
    ) -> HashMap<DeveloperArtifactKind, DeveloperArtifact> {
        let working = move || working_directories.clone();
        let activity = move |_: &Path| idle_days_ago.map(|days| NOW - days * DAY);
        let use_probe = move |paths: &[PathBuf], _: &dyn Fn() -> bool| vec![use_state; paths.len()];
        let selection = Selection {
            rules,
            now: NOW,
            use_probe: &use_probe,
            activity: &activity,
            working_directories: &working,
        };
        let inventory = DeveloperArtifactScanner::scan_workspaces_with_selection(
            &environment_with_home(&fixture.home),
            &[workspace_record(&fixture.root)],
            FolderAccess::NotGated,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            &selection,
        )
        .unwrap();
        inventory
            .records
            .into_values()
            .map(|record| (record.artifact.kind, record.artifact))
            .collect()
    }

    fn node_modules(
        found: &HashMap<DeveloperArtifactKind, DeveloperArtifact>,
    ) -> &DeveloperArtifact {
        found
            .get(&DeveloperArtifactKind::NodeModules)
            .expect("node_modules is reported")
    }

    #[test]
    fn the_dependency_rule_is_off_by_default() {
        let fixture = fixture(true);
        let found = scan(
            &fixture,
            CleanupRules::default(),
            Some(400),
            OpenFileState::Idle,
            vec![],
        );
        let node = node_modules(&found);
        assert!(!node.selected_by_default);
        assert_eq!(node.clean_inclusion, CleanInclusion::RuleOff);
        // Build output keeps the #494 behavior: idle means included.
        let target = &found[&DeveloperArtifactKind::CargoTarget];
        assert!(target.selected_by_default);
        assert_eq!(target.clean_inclusion, CleanInclusion::Included);
    }

    #[test]
    fn an_inactive_project_with_a_lockfile_includes_its_dependency_store() {
        let fixture = fixture(true);
        let found = scan(
            &fixture,
            dependency_rule(true, 30),
            Some(45),
            OpenFileState::Idle,
            vec![],
        );
        let node = node_modules(&found);
        assert!(node.selected_by_default);
        assert_eq!(node.clean_inclusion, CleanInclusion::Included);
        assert_eq!(node.project_last_activity, Some(NOW - 45 * DAY));
    }

    #[test]
    fn recent_work_unknown_activity_or_a_missing_lockfile_keep_it() {
        let locked = fixture(true);
        let fixture = &locked;
        let recent = scan(
            fixture,
            dependency_rule(true, 30),
            Some(3),
            OpenFileState::Idle,
            vec![],
        );
        assert_eq!(
            node_modules(&recent).clean_inclusion,
            CleanInclusion::RecentlyActive
        );
        assert!(!node_modules(&recent).selected_by_default);

        let unknown = scan(
            fixture,
            dependency_rule(true, 30),
            None,
            OpenFileState::Idle,
            vec![],
        );
        assert_eq!(
            node_modules(&unknown).clean_inclusion,
            CleanInclusion::ActivityUnknown
        );
        assert!(!node_modules(&unknown).selected_by_default);

        let working = scan(
            fixture,
            dependency_rule(true, 30),
            Some(400),
            OpenFileState::Idle,
            vec![fixture.node.join("src")],
        );
        assert_eq!(
            node_modules(&working).clean_inclusion,
            CleanInclusion::RecentlyActive
        );

        let unlocked = super::cleanup_rules::fixture(false);
        let found = scan(
            &unlocked,
            dependency_rule(true, 30),
            Some(400),
            OpenFileState::Idle,
            vec![],
        );
        assert_eq!(
            node_modules(&found).clean_inclusion,
            CleanInclusion::NoLockfile
        );
        assert!(!node_modules(&found).selected_by_default);
    }

    #[test]
    fn a_dependency_store_in_use_or_unknown_stays_for_review() {
        let fixture = fixture(true);
        for (state, inclusion) in [
            (OpenFileState::InUse, CleanInclusion::InUse),
            (OpenFileState::Unknown, CleanInclusion::UseUnknown),
        ] {
            let found = scan(
                &fixture,
                dependency_rule(true, 30),
                Some(400),
                state,
                vec![],
            );
            assert_eq!(node_modules(&found).clean_inclusion, inclusion, "{state:?}");
            assert!(!node_modules(&found).selected_by_default, "{state:?}");
        }
    }

    #[test]
    fn a_build_output_idle_period_keeps_active_projects() {
        let fixture = fixture(true);
        let rules = CleanupRules {
            build_output_min_idle_days: 14,
            ..CleanupRules::default()
        };
        let active = scan(&fixture, rules, Some(3), OpenFileState::Idle, vec![]);
        let target = &active[&DeveloperArtifactKind::CargoTarget];
        assert_eq!(target.clean_inclusion, CleanInclusion::RecentlyActive);
        assert!(!target.selected_by_default);

        let inactive = scan(&fixture, rules, Some(20), OpenFileState::Idle, vec![]);
        let target = &inactive[&DeveloperArtifactKind::CargoTarget];
        assert_eq!(target.clean_inclusion, CleanInclusion::Included);
        assert!(target.selected_by_default);
        assert!(fixture.rust.join("target").is_dir());
    }

    #[test]
    fn an_inventory_decided_with_other_rules_is_not_reused_for_clean() {
        let fixture = fixture(true);
        let selection_rules = dependency_rule(true, 30);
        let activity = |_: &Path| Some(NOW - 400 * DAY);
        let use_probe =
            |paths: &[PathBuf], _: &dyn Fn() -> bool| vec![OpenFileState::Idle; paths.len()];
        let selection = Selection {
            rules: selection_rules,
            now: NOW,
            use_probe: &use_probe,
            activity: &activity,
            working_directories: &Vec::new,
        };
        let inventory = DeveloperArtifactScanner::scan_workspaces_with_selection(
            &environment_with_home(&fixture.home),
            &[workspace_record(&fixture.root)],
            FolderAccess::NotGated,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            &selection,
        )
        .unwrap();
        assert_eq!(inventory.rules, selection_rules);
        assert!(inventory.is_current_for(&selection_rules));
        // The person turned the dependency rule off after the scan.
        assert!(!inventory.is_current_for(&CleanupRules::default()));
    }

    #[test]
    fn dependency_stores_and_their_lockfiles() {
        use preselection::{checks_use_before_removal, is_dependency_store, lockfiles};
        for kind in [
            DeveloperArtifactKind::NodeModules,
            DeveloperArtifactKind::PythonVenv,
            DeveloperArtifactKind::ComposerVendor,
            DeveloperArtifactKind::RubyBundle,
            DeveloperArtifactKind::ElixirDeps,
        ] {
            assert!(is_dependency_store(kind), "{kind:?}");
            assert!(checks_use_before_removal(kind), "{kind:?}");
            assert!(!lockfiles(kind).is_empty(), "{kind:?}");
        }
        for kind in [
            DeveloperArtifactKind::GoModuleCache,
            DeveloperArtifactKind::FlutterTooling,
            DeveloperArtifactKind::ToxEnvironments,
            DeveloperArtifactKind::NextOutput,
            DeveloperArtifactKind::TerraformCache,
            DeveloperArtifactKind::DotnetBin,
        ] {
            assert!(!is_dependency_store(kind), "{kind:?}");
            assert!(!checks_use_before_removal(kind), "{kind:?}");
        }
        assert!(checks_use_before_removal(
            DeveloperArtifactKind::CargoTarget
        ));
    }
}
