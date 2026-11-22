use super::*;
use crate::PathFlavor;

fn is_index_query(command: &Command) -> bool {
    command.get_args().any(|argument| argument == "ls-files")
}

fn environment(root: &Path) -> PlatformEnvironment {
    let git = if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Git\cmd\git.exe")
    } else {
        PathBuf::from("/usr/bin/git")
    };
    assert!(
        git.is_file(),
        "Git is a prerequisite for repository fixtures"
    );
    PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(root)
        .with_temp_dir(root)
        .with_tool("git", git)
}

fn git(root: &Path, arguments: &[&str]) {
    let environment = environment(root);
    let git = environment.tool("git").unwrap().path().unwrap();
    let output = Command::new(git)
        .current_dir(root)
        .args([
            "-c",
            "core.fsmonitor=",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(arguments)
        .env("GIT_CONFIG_GLOBAL", INERT_PATH)
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

fn repository() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap().join("repo");
    let artifact = root.join("target");
    fs::create_dir_all(artifact.join("assets")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
    fs::write(artifact.join("assets/generated.bin"), b"generated").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "--", "Cargo.toml"]);
    (temp, root, artifact)
}

fn probe(root: &Path, artifact: &Path) -> Evidence {
    probe_artifact_ownership(&environment(root), root, artifact, &AtomicBool::new(false))
}

#[test]
fn staged_artifacts_keep_the_original_git_index_namespace() {
    for tracked in [false, true] {
        let (_fixture, root, original) = repository();
        if tracked {
            git(&root, &["add", "--", "target"]);
        }
        let stage = root.join(".neati-reviewed-fixture");
        fs::create_dir(&stage).unwrap();
        let staged = stage.join("target");
        fs::rename(&original, &staged).unwrap();
        let evidence = probe_staged_artifact_ownership(
            &environment(&root),
            &root,
            &original,
            &staged,
            &AtomicBool::new(false),
        );
        assert_eq!(
            evidence,
            if tracked {
                Evidence::TrackedContent
            } else {
                Evidence::VerifiedGenerated
            }
        );
        assert_eq!(
            fs::read(staged.join("assets/generated.bin")).unwrap(),
            b"generated"
        );
        assert!(!original.exists());
    }
}

fn fixture_budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget {
        cancel,
        started: Instant::now(),
        entries: 0,
        max_entries: MAX_ENTRIES,
        stamps: Vec::new(),
        component_entities: Vec::new(),
        repository_absences: Vec::new(),
        metadata_absences: Vec::new(),
        configurations: Vec::new(),
    }
}

fn set_fixture_modified(path: &Path, modified: SystemTime) {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        };
        options
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[test]
fn directory_bindings_ignore_sibling_timestamps_but_keep_the_native_entity() {
    let temp = tempfile::tempdir().unwrap();
    let ancestor = fs::canonicalize(temp.path()).unwrap().join("ancestor");
    let route = ancestor.join("bound-route");
    fs::create_dir_all(&route).unwrap();
    let original = fs::symlink_metadata(&ancestor).unwrap();
    let expected = directory_entity(&ancestor, &original).unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = fixture_budget(&cancel);
    verify_components(&route, &mut budget).unwrap();
    budget
        .repository_absences
        .push((ancestor.clone(), expected));

    fs::write(ancestor.join("unrelated-sibling"), b"fixture").unwrap();
    set_fixture_modified(
        &ancestor,
        original.modified().unwrap() - Duration::from_secs(60),
    );
    let changed = fs::symlink_metadata(&ancestor).unwrap();
    assert_ne!(changed.modified().unwrap(), original.modified().unwrap());
    // Deterministically exercise metadata captured before a sibling write.
    assert_eq!(directory_entity(&ancestor, &original).unwrap(), expected);
    assert_eq!(budget.unchanged(), Ok(()));

    // Entity-only absence bindings still re-enumerate the Git namespace.
    fs::create_dir(ancestor.join(".git")).unwrap();
    assert_eq!(budget.unchanged(), Err(Uncertainty::ChangedDuringProbe));
}

#[test]
fn metadata_content_stamps_reject_writes_to_the_same_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("index");
    fs::write(&path, b"before").unwrap();
    let original = fs::symlink_metadata(&path).unwrap();
    let expected = stamp(&path, &original).unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = fixture_budget(&cancel);
    budget.record(&path, &original).unwrap();

    fs::write(&path, b"after!").unwrap();
    set_fixture_modified(
        &path,
        original.modified().unwrap() - Duration::from_secs(60),
    );
    let current = stamp(&path, &fs::symlink_metadata(&path).unwrap()).unwrap();
    assert_eq!(current.entity, expected.entity);
    assert_eq!(current.size, expected.size);
    assert_ne!(current.modified, expected.modified);
    assert_eq!(budget.unchanged(), Err(Uncertainty::ChangedDuringProbe));
}

#[test]
fn generated_untracked_payload_is_verified_and_new_tracked_content_is_protected() {
    let (_temp, root, artifact) = repository();
    assert_eq!(probe(&root, &artifact), Evidence::VerifiedGenerated);
    git(&root, &["add", "--", "target/assets/generated.bin"]);
    assert_eq!(probe(&root, &artifact), Evidence::TrackedContent);
    git(
        &root,
        &[
            "update-index",
            "--skip-worktree",
            "target/assets/generated.bin",
        ],
    );
    assert_eq!(probe(&root, &artifact), Evidence::TrackedContent);
    git(&root, &["update-index", "--split-index"]);
    assert_eq!(
        probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("split indexes must be refused before Git runs"),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::UnsupportedIndex)
    );
}

#[test]
fn names_protect_key_material_and_nested_git_directory_or_worktree_file() {
    for name in [
        ".git",
        ".GIT",
        "example-keypair.json",
        "EXAMPLE-KEYPAIR.JSON",
    ] {
        let (_temp, root, artifact) = repository();
        let path = artifact.join("assets").join(name);
        if name == ".git" {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(&path, b"this is not read").unwrap();
        }
        assert_eq!(
            probe(&root, &artifact),
            if name.eq_ignore_ascii_case(".git") {
                Evidence::NestedRepository
            } else {
                Evidence::DeploymentKeyMaterial
            }
        );
    }
}

#[cfg(unix)]
#[test]
fn keypair_name_is_refused_without_opening_a_fifo_or_following_a_link() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    let (_temp, root, artifact) = repository();
    let key = artifact.join("assets/example-keypair.json");
    let name = std::ffi::CString::new(key.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(probe(&root, &artifact), Evidence::DeploymentKeyMaterial);
    fs::remove_file(&key).unwrap();
    symlink("/neati-outside-sentinel", &key).unwrap();
    assert_eq!(probe(&root, &artifact), Evidence::DeploymentKeyMaterial);
}

#[test]
fn linked_worktree_uses_its_own_index_and_refuses_forged_backlinks() {
    let (_temp, root, _) = repository();
    git(&root, &["commit", "-qm", "fixture"]);
    let linked = root.parent().unwrap().join("linked");
    let linked_operand = crate::path_algebra::normalize_lexical(&linked);
    git(
        &root,
        &[
            "worktree",
            "add",
            "--detach",
            linked_operand.to_str().unwrap(),
        ],
    );
    let artifact = linked.join("target");
    fs::create_dir(&artifact).unwrap();
    fs::write(artifact.join("generated.bin"), b"generated").unwrap();
    assert_eq!(probe(&linked, &artifact), Evidence::VerifiedGenerated);
    let pointer = fs::read_to_string(linked.join(".git")).unwrap();
    let git_dir = PathBuf::from(pointer.trim().strip_prefix("gitdir: ").unwrap());
    let common_pointer = git_dir.join("commondir");
    let original_common = fs::read(&common_pointer).unwrap();
    let canonical_common = fs::canonicalize(root.join(".git")).unwrap();
    // The same native directory can be recorded with a lexical spelling
    // or the verbatim spelling returned by Windows canonicalization.
    for common in [
        crate::path_algebra::normalize_lexical(&canonical_common),
        canonical_common,
    ] {
        fs::write(&common_pointer, common.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(probe(&linked, &artifact), Evidence::VerifiedGenerated);
    }
    fs::write(&common_pointer, &original_common).unwrap();
    git(&linked, &["add", "--", "target/generated.bin"]);
    assert_eq!(probe(&linked, &artifact), Evidence::TrackedContent);
    fs::write(&common_pointer, root.to_string_lossy().as_bytes()).unwrap();
    assert_eq!(
        probe(&linked, &artifact),
        Evidence::Incomplete(Uncertainty::MalformedMetadata)
    );
    fs::write(&common_pointer, original_common).unwrap();
    fs::write(
        git_dir.join("gitdir"),
        root.join(".git").to_string_lossy().as_bytes(),
    )
    .unwrap();
    assert_eq!(
        probe(&linked, &artifact),
        Evidence::Incomplete(Uncertainty::MalformedMetadata)
    );
}

#[test]
fn corrupt_missing_and_misrouted_repository_metadata_preserves_uncertainty() {
    let (_temp, root, artifact) = repository();
    fs::write(root.join(".git/index"), b"not an index but long enough").unwrap();
    assert_eq!(
        probe(&root, &artifact),
        Evidence::Incomplete(Uncertainty::MalformedMetadata)
    );
    fs::remove_file(root.join(".git/index")).unwrap();
    assert_eq!(
        probe(&root, &artifact),
        Evidence::Incomplete(Uncertainty::UnreadableMetadata)
    );
    fs::remove_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git"), "gitdir: /another-repository\n").unwrap();
    assert!(!probe(&root, &artifact).allows_cleanup());
    fs::write(root.join(".git"), "broken pointer").unwrap();
    assert_eq!(
        probe(&root, &artifact),
        Evidence::Incomplete(Uncertainty::MalformedMetadata)
    );
}

#[test]
fn cancellation_timeout_output_limit_and_missing_git_cannot_authorize() {
    let (_temp, root, artifact) = repository();
    let env = environment(&root);
    assert_eq!(
        probe_artifact_ownership(&env, &root, &artifact, &AtomicBool::new(true)),
        Evidence::Incomplete(Uncertainty::Cancelled)
    );
    assert_eq!(
        probe_artifact_ownership(
            &env.clone().with_missing_tool("git"),
            &root,
            &artifact,
            &AtomicBool::new(false)
        ),
        Evidence::Incomplete(Uncertainty::MissingGit)
    );
    assert_eq!(
        probe_with(
            &env,
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, timeout, _| Err(SubprocessError::Timeout("fixture-git".into(), timeout)),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::TimedOut)
    );
    assert_eq!(
        probe_with(
            &env,
            &root,
            &artifact,
            &AtomicBool::new(false),
            1,
            &|_, _, _| panic!("an incomplete walk cannot run Git"),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::BudgetExceeded)
    );
    assert!(index_intersects_artifact(
        crate::PathFlavor::current(),
        b"unterminated metadata",
        b"target"
    )
    .is_err());
    for cap_stdout in [true, false] {
        assert_eq!(
            probe_with(
                &env,
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|command, timeout, cancel| {
                    let index_query = is_index_query(&command);
                    let mut output = run_with_timeout_cancellable(command, timeout, &|| {
                        cancel.load(Ordering::Relaxed)
                    })?;
                    let capped = vec![b'x'; crate::subprocess::MAX_CAPTURE_BYTES];
                    if index_query && cap_stdout {
                        output.stdout = capped;
                    } else if index_query {
                        output.stderr = capped;
                    }
                    Ok(output)
                },
                &|| {},
            ),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
    }
}

#[test]
fn nul_delimited_paths_case_differences_sparse_directories_and_unicode_are_safe() {
    let oid = "a".repeat(40);
    for path in [
        "target/assets/authored file\nwith\ttabs",
        "TARGET/assets/source.rs",
        "target/non-ASCII-é.rs",
    ] {
        let output = format!("100644 {oid} 0\t{path}\0");
        assert_eq!(
            index_intersects_artifact(crate::PathFlavor::current(), output.as_bytes(), b"target"),
            Ok(true)
        );
    }
    let sparse = format!("040000 {oid} 0\tpackages/\0");
    assert_eq!(
        index_intersects_artifact(
            crate::PathFlavor::current(),
            sparse.as_bytes(),
            b"packages/project/target"
        ),
        Ok(true)
    );
    let gitlink = format!("160000 {oid} 0\tpackages/project\0");
    assert_eq!(
        index_intersects_artifact(
            crate::PathFlavor::current(),
            gitlink.as_bytes(),
            b"packages/project/target"
        ),
        Ok(true)
    );
    for malformed in [
        "target//source.rs",
        "target/source.rs/",
        "/target/file",
        "target/../file",
    ] {
        let output = format!("100644 {oid} 0\t{malformed}\0");
        assert_eq!(
            index_intersects_artifact(crate::PathFlavor::current(), output.as_bytes(), b"target"),
            Err(Uncertainty::MalformedMetadata)
        );
    }
    for (indexed, observed) in [
        ("packages/é/target/authored.rs", "packages/É/target"),
        ("packages/e\u{301}/target/authored.rs", "packages/é/target"),
        ("K/target/source.rs", "k/target"),
    ] {
        let output = format!("100644 {oid} 0\t{indexed}\0");
        assert_eq!(
            index_intersects_artifact(
                crate::PathFlavor::current(),
                output.as_bytes(),
                observed.as_bytes()
            ),
            Err(Uncertainty::MalformedMetadata)
        );
    }
    assert_eq!(
        index_intersects_artifact(
            crate::PathFlavor::current(),
            format!("100644 {oid} 0\ttarget-other/source.rs\0").as_bytes(),
            b"target"
        ),
        Ok(false)
    );
}

#[test]
fn new_repository_or_new_optional_config_during_probe_revokes_authority() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap().join("repo");
    let artifact = root.join("target");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(artifact.join("output.bin"), b"generated").unwrap();
    let before = stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap();
    let result = probe_with(
        &environment(&root),
        &root,
        &artifact,
        &AtomicBool::new(false),
        MAX_ENTRIES,
        &|_, _, _| panic!("no repository existed before the recheck"),
        &|| {
            git(&root, &["init", "-q"]);
        },
    );
    assert_eq!(
        stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        result,
        Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
    );

    let (_temp, root, artifact) = repository();
    let result = probe_with(
        &environment(&root),
        &root,
        &artifact,
        &AtomicBool::new(false),
        MAX_ENTRIES,
        &|command, timeout, cancel| {
            run_with_timeout_cancellable(command, timeout, &|| cancel.load(Ordering::Relaxed))
        },
        &|| {
            fs::write(
                root.join(".git/config.worktree"),
                "[core]\nfsmonitor=bad-command\n",
            )
            .unwrap();
        },
    );
    assert_eq!(
        result,
        Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
    );
}

#[test]
fn descendant_changes_during_index_probe_are_refused_with_unchanged_artifact_root() {
    let (_temp, root, artifact) = repository();
    let before = stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap();
    let result = probe_with(
        &environment(&root),
        &root,
        &artifact,
        &AtomicBool::new(false),
        MAX_ENTRIES,
        &|command, timeout, cancel| {
            let index_query = is_index_query(&command);
            let output =
                run_with_timeout_cancellable(command, timeout, &|| cancel.load(Ordering::Relaxed))?;
            if index_query {
                fs::write(
                    artifact.join("assets/new-keypair.json"),
                    b"unread key material",
                )
                .unwrap();
            }
            Ok(output)
        },
        &|| {},
    );
    assert_eq!(
        stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        result,
        Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
    );
}

#[test]
fn repository_fsmonitor_hooks_and_config_includes_never_execute() {
    let (_temp, root, artifact) = repository();
    let sentinel = root.join("executed-sentinel");
    let program = root.join(if cfg!(windows) {
        "fsmonitor.cmd"
    } else {
        "fsmonitor.sh"
    });
    fs::write(
        &program,
        if cfg!(windows) {
            format!("@echo executed>\"{}\"\r\n", sentinel.display())
        } else {
            format!("#!/bin/sh\nprintf executed > '{}'\n", sentinel.display())
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    }
    git(
        &root,
        &["config", "core.fsmonitor", program.to_str().unwrap()],
    );
    git(&root, &["config", "core.hooksPath", root.to_str().unwrap()]);
    assert_eq!(probe(&root, &artifact), Evidence::VerifiedGenerated);
    assert!(!sentinel.exists());
    // Includes are refused before Git can follow an arbitrary config path.
    git(
        &root,
        &["config", "include.path", program.to_str().unwrap()],
    );
    assert_eq!(
        probe(&root, &artifact),
        Evidence::Incomplete(Uncertainty::MalformedMetadata)
    );
    assert!(!sentinel.exists());
}

#[test]
fn inherited_git_routing_global_config_and_program_settings_are_scrubbed() {
    let (_temp, root, artifact) = repository();
    let global = root.parent().unwrap().join("global.gitconfig");
    fs::write(&global, "[invalid configuration\n").unwrap();
    let sentinel = root.parent().unwrap().join("outside-sentinel");
    fs::write(&sentinel, b"unchanged").unwrap();
    let env = environment(&root);
    let git = env.tool("git").unwrap().path().unwrap();
    let route = RepositoryRoute {
        root: root.clone(),
        git_dir: root.join(".git"),
        common_dir: root.join(".git"),
        configurations: Vec::new(),
    };
    let mut command = Command::new(git);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_EXEC_PATH",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_SSH_COMMAND",
        "GIT_EXTERNAL_DIFF",
    ] {
        command.env(key, &global);
    }
    configure_index_command(&mut command, &route, &env).unwrap();
    let arguments = command
        .get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert!(arguments.iter().any(|arg| arg == "core.fsmonitor="));
    assert!(!arguments.iter().any(|arg| arg == "core.fsmonitor=false"));
    assert!(arguments.ends_with(
        &[
            "ls-files",
            "--cached",
            "--stage",
            "--full-name",
            "--sparse",
            "-z"
        ]
        .map(str::to_string)
    ));
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_EXEC_PATH",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_SSH_COMMAND",
        "GIT_EXTERNAL_DIFF",
    ] {
        assert!(!command
            .get_envs()
            .any(|(name, value)| name == key && value.is_some()));
    }
    assert!(command.get_envs().any(
        |(name, value)| name == "GIT_ALLOW_PROTOCOL" && value == Some(std::ffi::OsStr::new(""))
    ));
    assert!(command.get_envs().any(
        |(name, value)| name == "GIT_NO_LAZY_FETCH" && value == Some(std::ffi::OsStr::new("1"))
    ));
    let output = run_with_timeout_cancellable(command, GIT_TIMEOUT, &|| false).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        index_intersects_artifact(crate::PathFlavor::current(), &output.stdout, b"target"),
        Ok(false)
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"unchanged");
    assert_eq!(probe(&root, &artifact), Evidence::VerifiedGenerated);
}

#[test]
fn git_path_operands_use_plain_windows_spelling_without_changing_bound_routes() {
    let native = tempfile::tempdir().unwrap();
    let env = environment(native.path());
    let git = env.tool("git").unwrap().path().unwrap();
    let status = Command::new(git).arg("--version").output().unwrap().status;
    assert!(status.success());
    for (root, metadata, expected_root, expected_metadata) in [
        (
            r"\\?\D:\Users\홍 길동\project",
            r"\\?\D:\scratch\private-index",
            r"D:\Users\홍 길동\project",
            r"D:\scratch\private-index",
        ),
        (
            r"\\?\UNC\server\share\project",
            r"\\?\UNC\server\share\private-index",
            r"\\server\share\project",
            r"\\server\share\private-index",
        ),
        (
            r"\\?\D:\",
            r"\\?\D:\scratch\private-index",
            r"D:\",
            r"D:\scratch\private-index",
        ),
    ] {
        let route = RepositoryRoute {
            root: root.into(),
            git_dir: metadata.into(),
            common_dir: metadata.into(),
            configurations: Vec::new(),
        };
        let windows = PlatformEnvironment::simulated(crate::PathFlavor::Windows);
        let mut command = Command::new(git);
        configure_index_command(&mut command, &route, &windows).unwrap();
        let args = command.get_args().collect::<Vec<_>>();
        for (flag, expected) in [
            ("--git-dir", expected_metadata),
            ("--work-tree", expected_root),
        ] {
            let index = args.iter().position(|arg| *arg == flag).unwrap();
            assert_eq!(args[index + 1], std::ffi::OsStr::new(expected));
        }
        for (name, expected) in [
            ("GIT_COMMON_DIR", expected_metadata.to_string()),
            ("GIT_INDEX_FILE", format!(r"{expected_metadata}\index")),
        ] {
            assert_eq!(
                command.get_envs().find(|(key, _)| *key == name).unwrap().1,
                Some(std::ffi::OsStr::new(&expected))
            );
        }
        assert_eq!(command.get_current_dir(), Some(Path::new(root)));
        assert!(!args.iter().any(|arg| *arg == "core.protectNTFS=false"));
        let snapshot = GitIndexSnapshot {
            _directory: tempfile::tempdir().unwrap(),
            configurations: vec![route.git_dir.join("reviewed-config-0")],
            route,
        };
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            started: Instant::now(),
            entries: 0,
            max_entries: MAX_ENTRIES,
            stamps: Vec::new(),
            component_entities: Vec::new(),
            repository_absences: Vec::new(),
            metadata_absences: Vec::new(),
            configurations: Vec::new(),
        };
        snapshot
            .verify_configuration(git, &windows, &mut budget, &|command, _, _| {
                let args = command.get_args().collect::<Vec<_>>();
                let index = args.iter().position(|arg| *arg == "--file").unwrap();
                assert_eq!(
                    args[index + 1],
                    std::ffi::OsStr::new(&format!(r"{expected_metadata}\reviewed-config-0"))
                );
                Ok(Output {
                    status,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            })
            .unwrap();
        assert_eq!(snapshot.route.root, Path::new(root));
        assert_eq!(snapshot.route.git_dir, Path::new(metadata));
        assert_eq!(snapshot.route.common_dir, Path::new(metadata));
    }
    for ambiguous in [
        r"C:relative\index",
        r"\\?\C:\private\index:stream",
        r"\\?\C:\PRIVAT~1\index",
        r"\\?\C:\private.\index",
        r"\\?\C:\private \index",
        r"\\?\C:\private\NUL",
    ] {
        assert_eq!(
            git_command_path(Path::new(ambiguous), crate::PathFlavor::Windows),
            Err(Uncertainty::MalformedMetadata)
        );
    }
    assert_eq!(
        git_command_path(Path::new("/tmp/private-index"), crate::PathFlavor::Posix),
        Ok(PathBuf::from("/tmp/private-index"))
    );
}

#[test]
fn direct_marker_cannot_route_to_an_external_common_directory() {
    let (_temp, root, artifact) = repository();
    let outside = root.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join("config"),
        "[include]\npath=secret-keypair.json\n",
    )
    .unwrap();
    fs::write(
        root.join(".git/commondir"),
        outside.to_string_lossy().as_bytes(),
    )
    .unwrap();
    assert_eq!(
        probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("an unverified commondir must not run Git"),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::MalformedMetadata)
    );
}

#[test]
fn case_variant_shared_index_metadata_is_refused_without_running_git() {
    let (_temp, root, artifact) = repository();
    fs::write(root.join(".git/SHAREDINDEX.fixture"), b"unsupported").unwrap();
    assert_eq!(
        probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("shared-index layouts must not launch Git"),
            &|| {},
        ),
        Evidence::Incomplete(Uncertainty::UnsupportedIndex)
    );
}

#[test]
fn a_split_index_created_at_child_start_cannot_freshen_live_repository_metadata() {
    let (_temp, root, artifact) = repository();
    let result = probe_with(
        &environment(&root),
        &root,
        &artifact,
        &AtomicBool::new(false),
        MAX_ENTRIES,
        &|command, timeout, cancel| {
            let live_git_dir = root.join(".git");
            for name in ["GIT_COMMON_DIR", "GIT_INDEX_FILE"] {
                let (_, value) = command.get_envs().find(|(key, _)| *key == name).unwrap();
                assert!(!Path::new(value.unwrap()).starts_with(&live_git_dir));
            }
            let observed_shared = if is_index_query(&command) {
                git(&root, &["update-index", "--split-index"]);
                let shared = fs::read_dir(&live_git_dir)
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("sharedindex.")
                    })
                    .unwrap()
                    .path();
                let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
                fs::File::options()
                    .write(true)
                    .open(&shared)
                    .unwrap()
                    .set_times(fs::FileTimes::new().set_modified(old_time))
                    .unwrap();
                let before = stamp(&shared, &fs::metadata(&shared).unwrap()).unwrap();
                Some((shared, before))
            } else {
                None
            };
            let output =
                run_with_timeout_cancellable(command, timeout, &|| cancel.load(Ordering::Relaxed))?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            if let Some((shared, before)) = observed_shared {
                assert_eq!(
                    stamp(&shared, &fs::metadata(&shared).unwrap()).unwrap(),
                    before
                );
            }
            Ok(output)
        },
        &|| {},
    );
    assert_eq!(
        result,
        Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
    );
}

#[test]
fn private_index_capture_is_bounded_and_never_writes_inside_the_artifact() {
    let (_temp, root, artifact) = repository();
    let env = environment(&root);
    assert_eq!(
        probe_with(
            &env.clone().with_temp_dir(&artifact),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("an artifact cannot hold the probe's scratch repository"),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::OutsideScope)
    );
    fs::File::options()
        .write(true)
        .open(root.join(".git/index"))
        .unwrap()
        .set_len(MAX_INDEX_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(
        probe_with(
            &env,
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("an oversized index must not reach Git"),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::BudgetExceeded)
    );
}

#[test]
fn non_sha1_repositories_are_refused_before_any_git_child() {
    let (_temp, root, artifact) = repository();
    fs::remove_dir_all(root.join(".git")).unwrap();
    git(&root, &["init", "-q", "--object-format=sha256"]);
    git(
        &root,
        &["add", "--", "Cargo.toml", "target/assets/generated.bin"],
    );
    assert_eq!(
        probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("non-SHA-1 metadata must not reach the private SHA-1 decoder"),
            &|| {}
        ),
        Evidence::Incomplete(Uncertainty::UnsupportedIndex)
    );
}

#[test]
fn scratch_metadata_cannot_normalize_broken_source_head_configuration_or_index() {
    for (path, contents) in [
        ("HEAD", "not a reference"),
        ("config", "[broken configuration\n"),
        ("config", "[core]\nbare=invalid\n"),
        ("config", "[extensions]\nworktreeConfig=invalid\n"),
    ] {
        let (_temp, root, artifact) = repository();
        fs::write(root.join(".git").join(path), contents).unwrap();
        assert_eq!(
            probe(&root, &artifact),
            Evidence::Incomplete(Uncertainty::MalformedMetadata),
            "{path}: {contents}"
        );
    }
    for zero_checksum in [false, true] {
        let (_temp, root, artifact) = repository();
        let path = root.join(".git/index");
        let mut bytes = fs::read(&path).unwrap();
        if zero_checksum {
            let start = bytes.len() - 20;
            bytes[start..].fill(0);
        } else {
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
        }
        fs::write(path, bytes).unwrap();
        assert_eq!(
            probe_with(
                &environment(&root),
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("unverified index integrity must not reach Git"),
                &|| {}
            ),
            Evidence::Incomplete(if zero_checksum {
                Uncertainty::UnsupportedIndex
            } else {
                Uncertainty::MalformedMetadata
            })
        );
    }
}

#[test]
fn original_repository_shape_and_worktree_routing_must_be_verified() {
    for name in ["objects", "refs"] {
        for replace_with_file in [false, true] {
            let (_temp, root, artifact) = repository();
            let path = root.join(".git").join(name);
            fs::remove_dir_all(&path).unwrap();
            if replace_with_file {
                fs::write(&path, b"invalid shape").unwrap();
            }
            assert_eq!(
                probe_with(
                    &environment(&root),
                    &root,
                    &artifact,
                    &AtomicBool::new(false),
                    MAX_ENTRIES,
                    &|_, _, _| panic!("broken repository directories must not reach Git"),
                    &|| {}
                ),
                Evidence::Incomplete(if replace_with_file {
                    Uncertainty::MalformedMetadata
                } else {
                    Uncertainty::UnreadableMetadata
                })
            );
        }
    }
    let (_temp, root, artifact) = repository();
    fs::write(root.join(".git/config"), "[core]\nworktree=/outside\n").unwrap();
    assert_eq!(
        probe(&root, &artifact),
        Evidence::Incomplete(Uncertainty::OutsideScope)
    );
}

#[test]
fn nearer_valid_or_broken_repository_metadata_cannot_mask_ancestor_tracked_content() {
    let (_temp, root, _) = repository();
    let project = root.join("package");
    let artifact = project.join("target");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname='nested'\n").unwrap();
    fs::write(artifact.join("authored.rs"), b"authored").unwrap();
    git(&root, &["add", "--", "package/target/authored.rs"]);
    git(&project, &["init", "-q"]);
    git(&project, &["add", "--", "Cargo.toml"]);
    assert_eq!(probe(&project, &artifact), Evidence::TrackedContent);
    fs::remove_dir_all(project.join(".git/objects")).unwrap();
    assert_eq!(
        probe(&project, &artifact),
        Evidence::Incomplete(Uncertainty::UnreadableMetadata)
    );
}

#[test]
fn windows_index_path_ambiguities_fail_closed_on_every_runner() {
    let oid = "a".repeat(40);
    for path in [
        r"target\authored.rs",
        "target./authored.rs",
        "target /authored.rs",
        "TARGET~1/authored.rs",
        "target:stream",
        "C:target/file",
        "target/NUL",
    ] {
        let output = format!("100644 {oid} 0\t{path}\0");
        assert_eq!(
            index_intersects_artifact(crate::PathFlavor::Windows, output.as_bytes(), b"target"),
            Err(Uncertainty::MalformedMetadata)
        );
    }
    assert_eq!(
        index_intersects_artifact(crate::PathFlavor::Windows, b"", b"TARGET~1"),
        Err(Uncertainty::MalformedMetadata)
    );
    let output = format!("100644 {oid} 0\ttarget\\authored.rs\0");
    assert_eq!(
        index_intersects_artifact(crate::PathFlavor::Posix, output.as_bytes(), b"target"),
        Ok(false)
    );
}

#[cfg(windows)]
#[test]
fn windows_native_file_ids_refuse_replacement_with_restored_creation_and_write_times() {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows_sys::Win32::{
        Foundation::FILETIME,
        Storage::FileSystem::{
            SetFileTime, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_WRITE_ATTRIBUTES,
        },
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("metadata");
    fs::create_dir(&path).unwrap();
    let metadata = fs::symlink_metadata(&path).unwrap();
    let before = stamp(&path, &metadata).unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = fixture_budget(&cancel);
    budget
        .component_entities
        .push((path.clone(), directory_entity(&path, &metadata).unwrap()));
    fs::rename(&path, temp.path().join("original-metadata")).unwrap();
    fs::create_dir(&path).unwrap();
    let handle = OpenOptions::new()
        .read(true)
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .unwrap();
    let time = |value: u64| FILETIME {
        dwLowDateTime: value as u32,
        dwHighDateTime: (value >> 32) as u32,
    };
    assert_ne!(
        unsafe {
            SetFileTime(
                handle.as_raw_handle().cast(),
                &time(metadata.creation_time()),
                std::ptr::null(),
                &time(metadata.last_write_time()),
            )
        },
        0
    );
    drop(handle);
    let restored = fs::symlink_metadata(&path).unwrap();
    assert_eq!(restored.creation_time(), metadata.creation_time());
    assert_eq!(restored.last_write_time(), metadata.last_write_time());
    let after = stamp(&path, &restored).unwrap();
    assert_eq!(before.size, after.size);
    assert_eq!(before.modified, after.modified);
    assert_ne!(before.entity, after.entity);
    assert_eq!(budget.unchanged(), Err(Uncertainty::ChangedDuringProbe));
}

#[cfg(unix)]
#[test]
fn replacing_a_git_metadata_ancestor_with_a_link_revokes_its_original_route() {
    let temp = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(temp.path()).unwrap();
    let parent = base.join("metadata");
    let git_dir = parent.join("worktrees/project");
    fs::create_dir_all(&git_dir).unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        started: Instant::now(),
        entries: 0,
        max_entries: MAX_ENTRIES,
        stamps: Vec::new(),
        component_entities: Vec::new(),
        repository_absences: Vec::new(),
        metadata_absences: Vec::new(),
        configurations: Vec::new(),
    };
    verify_components(&git_dir, &mut budget).unwrap();
    let original = stamp(&git_dir, &fs::metadata(&git_dir).unwrap()).unwrap();
    let moved = base.join("original-metadata");
    fs::rename(&parent, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &parent).unwrap();
    assert_eq!(
        stamp(&git_dir, &fs::metadata(&git_dir).unwrap()).unwrap(),
        original
    );
    assert_eq!(budget.unchanged(), Err(Uncertainty::ChangedDuringProbe));
}
