//! Project-local caches (#479): each kind needs its marker, none is
//! preselected, and the refused folders stay refused.
use super::*;
use crate::removal_manager::{RemovalExecutor, RemovalPlanner};

const TAG: &str =
    "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag.\n";

fn workspace_record(root: &Path) -> DeveloperWorkspaceRecord {
    DeveloperWorkspaceRecord {
        workspace: DeveloperWorkspace {
            id: Uuid::new_v4().to_string(),
            name: "Fixture".into(),
            display_path: root.display().to_string(),
            path: root.display().to_string(),
        },
        path: root.into(),
        identity: identity_from_path(root).unwrap(),
        created_at: unix_timestamp(),
        whole_home: false,
    }
}

fn environment(home: &Path) -> PlatformEnvironment {
    PlatformEnvironment::simulated(path_algebra::PathFlavor::current()).with_home(home)
}

fn file(path: &Path, contents: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Every new folder, the marker that authorizes it, and the kind it becomes.
const CASES: &[(&str, &str, DeveloperArtifactKind)] = &[
    (
        "__pycache__",
        "pyproject.toml",
        DeveloperArtifactKind::PythonBytecodeCache,
    ),
    (
        ".pytest_cache",
        "setup.py",
        DeveloperArtifactKind::PytestCache,
    ),
    (".mypy_cache", "setup.cfg", DeveloperArtifactKind::MypyCache),
    (
        ".ruff_cache",
        "pyproject.toml",
        DeveloperArtifactKind::RuffCache,
    ),
    (".tox", "tox.ini", DeveloperArtifactKind::ToxEnvironments),
    (".nox", "noxfile.py", DeveloperArtifactKind::NoxEnvironments),
    (".turbo", "turbo.json", DeveloperArtifactKind::TurboCache),
    (
        ".angular",
        "angular.json",
        DeveloperArtifactKind::AngularCache,
    ),
    (".nx", "nx.json", DeveloperArtifactKind::NxCache),
    ("Library", "unity", DeveloperArtifactKind::UnityLibrary),
    ("Temp", "unity", DeveloperArtifactKind::UnityTemp),
    ("Obj", "unity", DeveloperArtifactKind::UnityObj),
    ("Logs", "unity", DeveloperArtifactKind::UnityLogs),
    (
        "Intermediate",
        "Game.uproject",
        DeveloperArtifactKind::UnrealIntermediate,
    ),
    (
        "DerivedDataCache",
        "Game.uproject",
        DeveloperArtifactKind::UnrealDerivedDataCache,
    ),
    (".godot", "project.godot", DeveloperArtifactKind::GodotCache),
    ("elm-stuff", "elm.json", DeveloperArtifactKind::ElmStuff),
    (
        ".cpcache",
        "deps.edn",
        DeveloperArtifactKind::ClojureCpcache,
    ),
    (".bloop", "build.sbt", DeveloperArtifactKind::BloopCache),
    (".metals", "build.sbt", DeveloperArtifactKind::MetalsCache),
];

/// Writes the evidence a case names: a marker file, or Unity's two folders.
fn mark(project: &Path, marker: &str) {
    if marker == "unity" {
        fs::create_dir_all(project.join("Assets/Scenes")).unwrap();
        fs::create_dir_all(project.join("ProjectSettings")).unwrap();
        file(&project.join("Assets/Scenes/Main.unity"), b"scene");
        file(
            &project.join("ProjectSettings/ProjectVersion.txt"),
            b"m_EditorVersion: 6000.0.0f1",
        );
    } else {
        file(&project.join(marker), b"fixture");
    }
}

#[test]
fn every_project_cache_needs_its_direct_marker() {
    for (directory, marker, kind) in CASES {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        file(&project.join(directory).join("entry.bin"), &[1; 32]);
        assert!(
            rules::recognize(&project, directory).is_none(),
            "{directory} without evidence"
        );
        // Evidence beside an ancestor or a sibling authorizes nothing here.
        mark(temp.path(), marker);
        mark(&temp.path().join("sibling"), marker);
        assert!(
            rules::recognize(&project, directory).is_none(),
            "{directory} with only ancestor evidence"
        );
        mark(&project, marker);
        let found = rules::recognize(&project, directory)
            .unwrap_or_else(|| panic!("{directory} with {marker}"));
        assert_eq!(found.kind, *kind, "{directory}");
        assert_eq!(found.artifact_relative, Path::new(directory));
        assert!(found
            .marker_paths
            .iter()
            .all(|path| path.parent() == Some(project.as_path())));
        assert!(found.rebuild_hint.is_some());
        assert!(artifact_relative_is_allowed(Path::new(directory), *kind));
        assert!(!artifact_relative_is_allowed(
            &Path::new("nested").join(directory),
            *kind
        ));
        assert!(
            !preselection::is_build_output(*kind),
            "{kind:?} must never be a one-button build output"
        );
    }
}

#[test]
fn a_python_tool_cache_may_be_evidenced_by_a_valid_cache_tag_alone() {
    for (directory, kind) in [
        (".pytest_cache", DeveloperArtifactKind::PytestCache),
        (".mypy_cache", DeveloperArtifactKind::MypyCache),
        (".ruff_cache", DeveloperArtifactKind::RuffCache),
        ("__pycache__", DeveloperArtifactKind::PythonBytecodeCache),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("scripts");
        let cache = project.join(directory);
        file(&cache.join("v/cache/nodeids"), b"[]");
        let tag = cache.join("CACHEDIR.TAG");

        for invalid in [
            &b""[..],
            b"Signature: 00000000000000000000000000000000\n",
            b"signature: 8a477f597d28d172789f06886806bc55\n",
            b" Signature: 8a477f597d28d172789f06886806bc55\n",
        ] {
            fs::write(&tag, invalid).unwrap();
            assert!(
                rules::recognize(&project, directory).is_none(),
                "{directory}"
            );
        }
        fs::write(&tag, TAG).unwrap();
        let found = rules::recognize(&project, directory).unwrap();
        assert_eq!(found.kind, kind);
        assert_eq!(found.marker_paths, vec![tag.clone()]);
        assert!(is_inner_cache_tag(kind, &cache, &tag));
        // A tag never certifies another kind, nor a file of a different name.
        assert!(!is_inner_cache_tag(
            DeveloperArtifactKind::ToxEnvironments,
            &cache,
            &tag
        ));
        assert!(!is_inner_cache_tag(kind, &cache, &cache.join("README.md")));
    }
}

#[cfg(unix)]
#[test]
fn a_linked_cache_tag_is_never_read_as_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("scripts");
    let cache = project.join(".pytest_cache");
    fs::create_dir_all(&cache).unwrap();
    let outside = temp.path().join("outside-tag");
    fs::write(&outside, TAG).unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("CACHEDIR.TAG")).unwrap();
    assert!(rules::recognize(&project, ".pytest_cache").is_none());
}

#[test]
fn a_tag_does_not_authorize_a_cache_kind_that_needs_a_project_marker() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    for directory in [".tox", ".turbo", ".nx", "Library", ".godot"] {
        file(
            &project.join(directory).join("CACHEDIR.TAG"),
            TAG.as_bytes(),
        );
        assert!(
            rules::recognize(&project, directory).is_none(),
            "{directory}"
        );
    }
}

#[test]
fn unity_folders_need_both_assets_and_project_settings_and_never_name_them() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("game");
    fs::create_dir_all(project.join("Library/ScriptAssemblies")).unwrap();
    fs::create_dir_all(project.join("Assets")).unwrap();
    assert!(rules::recognize(&project, "Library").is_none());
    // A file named like the folder is not the folder.
    file(&project.join("ProjectSettings"), b"not a directory");
    assert!(rules::recognize(&project, "Library").is_none());
    fs::remove_file(project.join("ProjectSettings")).unwrap();
    fs::create_dir_all(project.join("ProjectSettings")).unwrap();
    assert_eq!(
        rules::recognize(&project, "Library").unwrap().kind,
        DeveloperArtifactKind::UnityLibrary
    );
    for project_state in ["Assets", "ProjectSettings", "Packages", "UserSettings"] {
        assert!(rules::recognize(&project, project_state).is_none());
    }
}

#[cfg(unix)]
#[test]
fn a_linked_unity_marker_is_not_project_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("game");
    fs::create_dir_all(project.join("Library")).unwrap();
    fs::create_dir_all(project.join("ProjectSettings")).unwrap();
    fs::create_dir_all(temp.path().join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("elsewhere"), project.join("Assets")).unwrap();
    assert!(rules::recognize(&project, "Library").is_none());
}

#[test]
fn notebook_checkpoints_and_unreal_saved_are_never_recognized() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    mark(&project, "pyproject.toml");
    mark(&project, "Game.uproject");
    mark(&project, "unity");
    for refused in [
        ".ipynb_checkpoints",
        "Saved",
        "Binaries",
        "Content",
        "Config",
    ] {
        file(&project.join(refused).join("kept.bin"), &[2; 16]);
        assert!(rules::recognize(&project, refused).is_none(), "{refused}");
    }
}

#[test]
fn a_generic_folder_name_without_evidence_is_still_traversed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    for wrapper in super::project_caches::TRAVERSE_UNLESS_RECOGNIZED {
        let nested = root.join(wrapper).join("tool");
        file(&nested.join("Cargo.toml"), b"[package]\nname='tool'\n");
        file(&nested.join("target/out.bin"), &[3; 64]);
        assert!(!should_skip_discovery_directory(wrapper), "{wrapper}");
    }
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &environment(temp.path()),
        &[workspace_record(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        inventory.records.len(),
        super::project_caches::TRAVERSE_UNLESS_RECOGNIZED.len()
    );
    assert!(inventory
        .records
        .values()
        .all(|record| record.artifact.kind == DeveloperArtifactKind::CargoTarget));
}

/// One workspace holding every new kind, scanned with an open-file probe that
/// would call any folder idle: none is preselected and none is even asked.
#[test]
fn project_caches_are_reviewable_but_never_preselected() {
    use neati_platform::open_files::OpenFileState;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap().join("workspace");
    let mut expected = Vec::new();
    for (index, (directory, marker, kind)) in CASES.iter().enumerate() {
        let project = root.join(format!("project-{index}"));
        mark(&project, marker);
        file(&project.join(directory).join("payload.bin"), &[4; 128]);
        expected.push((project.join(directory), *kind));
    }
    // The existing `.dart_tool` rule covers Dart and Flutter once.
    let dart = root.join("dart-app");
    mark(&dart, "pubspec.yaml");
    file(&dart.join(".dart_tool/package_config.json"), b"{}");

    let asked = Mutex::new(Vec::new());
    let probe = |paths: &[PathBuf], _: &dyn Fn() -> bool| {
        asked.lock().unwrap().extend(paths.iter().cloned());
        vec![OpenFileState::Idle; paths.len()]
    };
    let inventory = DeveloperArtifactScanner::scan_workspaces_with_use_probe(
        &environment(temp.path()),
        &[workspace_record(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
        &probe,
    )
    .unwrap();

    for (path, kind) in &expected {
        let record = inventory
            .records
            .values()
            .find(|record| &record.path == path)
            .unwrap_or_else(|| panic!("{kind:?} is reported"));
        assert_eq!(record.artifact.kind, *kind);
        assert_eq!(record.artifact.status, DeveloperArtifactStatus::Complete);
        assert_eq!(
            record.artifact.ownership,
            ArtifactOwnershipEvidence::VerifiedGenerated
        );
        assert!(record.artifact.status.allows_manual_cleanup());
        assert!(!record.artifact.selected_by_default, "{kind:?}");
    }
    let dart_records: Vec<_> = inventory
        .records
        .values()
        .filter(|record| record.project_root == dart)
        .collect();
    assert_eq!(dart_records.len(), 1, "`.dart_tool` is counted once");
    assert_eq!(
        dart_records[0].artifact.kind,
        DeveloperArtifactKind::FlutterTooling
    );
    assert_eq!(inventory.records.len(), expected.len() + 1);
    assert!(asked.lock().unwrap().is_empty(), "no use check was needed");
}

/// The removal scope check accepts a tag-evidenced cache, rebinds the tag, and
/// refuses once the tag changes.
#[test]
fn a_tag_evidenced_cache_is_removable_until_its_tag_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap().join("workspace");
    let cache = root.join("scripts/.pytest_cache");
    file(&cache.join("CACHEDIR.TAG"), TAG.as_bytes());
    file(&cache.join("v/cache/lastfailed"), b"{}");
    let env = environment(temp.path());
    let scan = || {
        DeveloperArtifactScanner::scan_workspaces(
            &env,
            &[workspace_record(&root)],
            FolderAccess::NotGated,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .unwrap()
    };

    let inventory = scan();
    let id = inventory.records.keys().next().unwrap().clone();
    let plan =
        RemovalPlanner::from_developer_artifacts(&env, &inventory, std::slice::from_ref(&id))
            .expect("a tag-evidenced cache is plannable");
    let backend = Arc::new(neati_platform::MockRemovalBackend::new());
    let result = RemovalExecutor::new(backend.clone()).execute(&env, plan);
    assert_eq!(result.removed_count, 1);
    assert_eq!(backend.removed(), vec![cache.clone()]);

    let inventory = scan();
    let id = inventory.records.keys().next().unwrap().clone();
    let plan =
        RemovalPlanner::from_developer_artifacts(&env, &inventory, std::slice::from_ref(&id))
            .unwrap();
    fs::remove_file(cache.join("CACHEDIR.TAG")).unwrap();
    fs::write(cache.join("CACHEDIR.TAG"), b"Signature: replaced\n").unwrap();
    let backend = Arc::new(neati_platform::MockRemovalBackend::new());
    let result = RemovalExecutor::new(backend.clone()).execute(&env, plan);
    assert_eq!(result.removed_count, 0);
    assert!(backend.removed().is_empty());
}

#[cfg(target_os = "macos")]
fn fixture_git(project: &Path, arguments: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null"])
        .args(arguments)
        .current_dir(project)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Git-tracked content inside a recognized folder revokes it, for the new
/// caches as for an existing `vendor` or `bin`.
#[cfg(target_os = "macos")]
#[test]
fn git_tracked_project_caches_vendor_and_bin_stay_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap().join("workspace");
    let project = root.join("mixed");
    mark(&project, "turbo.json");
    mark(&project, "unity");
    mark(&project, "composer.json");
    mark(&project, "App.csproj");
    file(&project.join(".turbo/cache/hash.tar.zst"), &[5; 64]);
    file(&project.join("Library/ScriptAssemblies/Game.dll"), &[6; 64]);
    file(&project.join("vendor/autoload.php"), b"<?php\n");
    file(
        &project.join("vendor/acme/patched.php"),
        b"<?php // local patch\n",
    );
    file(&project.join("bin/tool.sh"), b"#!/bin/sh\n");
    fixture_git(&project, &["init", "-q"]);
    fixture_git(
        &project,
        &[
            "add",
            "--",
            ".turbo/cache/hash.tar.zst",
            "Library/ScriptAssemblies/Game.dll",
            "vendor/acme/patched.php",
            "bin/tool.sh",
        ],
    );
    let env = environment(temp.path()).with_tool("git", "/usr/bin/git");
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &env,
        &[workspace_record(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    let mut kinds: Vec<DeveloperArtifactKind> = Vec::new();
    for record in inventory.records.values() {
        assert_eq!(
            record.artifact.ownership,
            ArtifactOwnershipEvidence::TrackedContent,
            "{:?}",
            record.artifact.kind
        );
        assert_eq!(
            record.artifact.status,
            DeveloperArtifactStatus::SafetyBlocked
        );
        assert!(!record.artifact.selected_by_default);
        kinds.push(record.artifact.kind);
        assert!(RemovalPlanner::from_developer_artifacts(
            &env,
            &inventory,
            std::slice::from_ref(&record.artifact.id)
        )
        .is_err());
    }
    kinds.sort_by_key(|kind| format!("{kind:?}"));
    assert_eq!(
        kinds,
        [
            DeveloperArtifactKind::ComposerVendor,
            DeveloperArtifactKind::DotnetBin,
            DeveloperArtifactKind::TurboCache,
            DeveloperArtifactKind::UnityLibrary,
        ]
    );
}
