use super::*;
use crate::trash_manager::{TrashExecutor, TrashPlanner};

fn copy_fixture(name: &str, destination: &Path) {
    fn copy(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == "provenance.json" {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &destination.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
            }
        }
    }
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/frameworks")
            .join(name),
        destination,
    );
}

fn workspace(root: &Path) -> DeveloperWorkspaceRecord {
    DeveloperWorkspaceRecord {
        workspace: DeveloperWorkspace {
            id: Uuid::new_v4().to_string(),
            name: "Fixture".into(),
            display_path: root.display().to_string(),
        },
        path: root.into(),
        identity: identity_from_path(root).unwrap(),
        created_at: unix_timestamp(),
        whole_home: false,
    }
}

fn environment(root: &Path) -> PlatformEnvironment {
    let env = PlatformEnvironment::simulated(path_algebra::PathFlavor::current())
        .with_home(root)
        .with_temp_dir(root);
    #[cfg(unix)]
    {
        env.with_current_user_id(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        env
    }
}

fn scan(env: &PlatformEnvironment, root: &Path) -> DeveloperArtifactInventory {
    DeveloperArtifactScanner::scan_workspaces(
        env,
        &[workspace(root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap()
}

#[cfg(target_os = "macos")]
fn fixture_git(project: &Path, arguments: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .args([
            "-c",
            "core.fsmonitor=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.untrackedCache=false",
        ])
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

#[test]
fn custom_dynamic_outputs_stay_observed_and_nested_projects_stay_discoverable() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let project = root.join("project");
    fs::create_dir_all(project.join("custom/nested/child/target")).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"dependencies":{"next":"15.5.14"}}"#,
    )
    .unwrap();
    fs::write(
        project.join("next.config.js"),
        "module.exports = {\"distDir\":\"custom/nested\"};",
    )
    .unwrap();
    fs::write(project.join("custom/nested/payload"), [1; 10]).unwrap();
    fs::write(
        project.join("custom/nested/child/Cargo.toml"),
        "[package]\nname='fixture'\n",
    )
    .unwrap();
    fs::write(project.join("custom/nested/child/target/output"), [2; 4]).unwrap();
    let env = environment(&root);
    let inventory = scan(&env, &root);
    let custom = inventory
        .records
        .values()
        .find(|r| r.path == project.join("custom/nested"))
        .unwrap();
    assert_eq!(
        custom.artifact.status,
        DeveloperArtifactStatus::ObservationOnly
    );
    assert_eq!(
        custom.artifact.logical_bytes,
        14 + fs::metadata(project.join("custom/nested/child/Cargo.toml"))
            .unwrap()
            .len()
    );
    assert!(!custom.artifact.selected_by_default);
    assert!(inventory
        .records
        .values()
        .any(|r| r.artifact.kind == DeveloperArtifactKind::CargoTarget));
    assert!(TrashPlanner::from_developer_artifacts(
        &env,
        &inventory,
        std::slice::from_ref(&custom.artifact.id)
    )
    .is_err());
    let mut forged = inventory.clone();
    forged
        .records
        .get_mut(&custom.artifact.id)
        .unwrap()
        .artifact
        .status = DeveloperArtifactStatus::Complete;
    assert!(TrashPlanner::from_developer_artifacts(
        &env,
        &forged,
        std::slice::from_ref(&custom.artifact.id)
    )
    .is_err());
    let authored = inventory
        .records
        .values()
        .find(|r| r.artifact.kind == DeveloperArtifactKind::CargoTarget)
        .unwrap();
    let mut forged_plan = TrashPlanner::from_developer_artifacts(
        &env,
        &inventory,
        std::slice::from_ref(&authored.artifact.id),
    )
    .unwrap();
    let target = &mut forged_plan.targets[0];
    target.path = custom.path.clone();
    target.identity = custom.identity.clone();
    target.scope = crate::trash_manager::TrashScope::DeveloperArtifact {
        workspace_root: custom.workspace_path.clone(),
        workspace_identity: custom.workspace_identity.clone(),
        project_root: custom.project_root.clone(),
        project_identity: Box::new(custom.project_identity.clone()),
        artifact_relative: custom.artifact_relative.clone(),
        marker_identities: custom.marker_identities.clone(),
        kind: custom.artifact.kind,
        framework_fingerprint: Some("forged".into()),
        framework_snapshot: None,
    };
    let backend = Arc::new(neati_platform::MockTrashBackend::new());
    let execution = TrashExecutor::new(backend.clone()).execute(&env, forged_plan);
    assert_eq!(execution.moved_count, 0);
    assert!(backend.moved().is_empty());
    assert_eq!(fs::read(custom.path.join("payload")).unwrap(), [1; 10]);
    fs::write(
        project.join("next.config.js"),
        "throw new Error('must not execute'); export default {distDir: 'custom/nested'};",
    )
    .unwrap();
    assert!(scan(&env, &root)
        .records
        .values()
        .any(|r| r.path == custom.path
            && r.artifact.status == DeveloperArtifactStatus::ObservationOnly));
}

#[test]
fn framework_format_cancellation_and_layout_budgets_remain_explicit() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    copy_fixture("sveltekit-2.37.1", &root.join("project"));
    let env = environment(&root);
    let output = root.join("project/.svelte-kit");
    assert!(
        neati_platform::temporary_storage::verify_framework_generated_tree(
            &env,
            &output,
            neati_core::domain::storage::FrameworkGeneratedKind::SvelteKitOutput,
            &AtomicBool::new(true)
        )
        .is_err()
    );
    for index in 0..33 {
        fs::write(output.join(format!("unknown-{index}")), [1]).unwrap();
    }
    assert!(verify_framework_generated_contract(
        &env,
        &root.join("project"),
        &output,
        DeveloperArtifactKind::SvelteKitOutput,
        &AtomicBool::new(false)
    )
    .is_err());
    let cancelled = DeveloperArtifactScanner::scan_workspaces(
        &env,
        &[workspace(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(true)),
        |_| {},
    )
    .unwrap();
    assert!(cancelled.cancelled);
    assert!(cancelled.records.is_empty());
}

#[test]
fn byte_measurement_budgets_keep_unread_entries_partial() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("payload"), [1; 8]).unwrap();
    let device = identity_from_path(fixture.path()).unwrap().device();
    for budget in [
        (MAX_DISCOVERY_ENTRIES as usize, std::time::Instant::now()),
        (
            0,
            std::time::Instant::now() - std::time::Duration::from_secs(6),
        ),
    ] {
        let mut budget = budget;
        let measured = measure_tree_bounded(
            fixture.path(),
            device,
            &AtomicBool::new(false),
            0,
            &mut budget,
        );
        assert!(!measured.complete);
        assert!(!measured.cancelled);
        assert!(!measured.safety_blocked);
        assert_eq!(measured.logical_bytes, 0);
    }
}

#[test]
fn unverified_default_framework_outputs_preserve_nested_package_descent() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let project = root.join("project");
    fs::create_dir_all(project.join(".next/nested/child/target")).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"dependencies":{"next":"15.5.14"}}"#,
    )
    .unwrap();
    fs::write(
        project.join(".next/nested/child/Cargo.toml"),
        "[package]\nname='fixture'\n",
    )
    .unwrap();
    fs::write(project.join(".next/nested/child/target/payload"), [1; 9]).unwrap();
    let env = environment(&root);
    let inventory = scan(&env, &root);
    assert_eq!(
        inventory
            .records
            .values()
            .filter(|r| r.artifact.kind == DeveloperArtifactKind::NextOutput)
            .count(),
        1
    );
    assert!(inventory
        .records
        .values()
        .any(|r| r.artifact.kind == DeveloperArtifactKind::CargoTarget));
    assert!(inventory
        .records
        .values()
        .all(|r| !r.artifact.selected_by_default));
}

#[cfg(target_os = "macos")]
struct FixtureTrash {
    directory: PathBuf,
}
#[cfg(target_os = "macos")]
impl neati_platform::TrashBackend for FixtureTrash {
    fn move_to_trash(&self, path: &Path) -> Result<(), String> {
        assert!(path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".neati-reviewed-"));
        fs::rename(path, self.directory.join(path.file_name().unwrap())).map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
#[test]
fn actual_generated_default_whole_units_move_with_exact_accounting_and_overlap_refusal() {
    for (fixture_name, parent_kind, child_kind, output) in [
        (
            "sveltekit-2.37.1",
            DeveloperArtifactKind::SvelteKitOutput,
            DeveloperArtifactKind::SvelteKitTypes,
            ".svelte-kit",
        ),
        (
            "next-15.5.14",
            DeveloperArtifactKind::NextOutput,
            DeveloperArtifactKind::NextWebpackCache,
            ".next",
        ),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        copy_fixture(fixture_name, &project);
        fs::write(
            project.join("SOURCE-SENTINEL"),
            b"authored source outside output",
        )
        .unwrap();
        let env = environment(&root);
        let inventory = scan(&env, &root);
        let parent = inventory
            .records
            .values()
            .find(|r| r.artifact.kind == parent_kind)
            .unwrap();
        let child = inventory
            .records
            .values()
            .find(|r| r.artifact.kind == child_kind)
            .unwrap();
        assert_eq!(
            parent.artifact.status,
            DeveloperArtifactStatus::Complete,
            "{:?}",
            parent.artifact.evidence
        );
        assert_eq!(child.artifact.status, DeveloperArtifactStatus::Complete);
        assert!(parent.artifact.logical_bytes >= child.artifact.logical_bytes);
        assert!(!parent.artifact.selected_by_default && !child.artifact.selected_by_default);
        assert!(TrashPlanner::from_developer_artifacts(
            &env,
            &inventory,
            &[parent.artifact.id.clone(), child.artifact.id.clone()]
        )
        .unwrap_err()
        .contains("overlap"));
        let plan = TrashPlanner::from_developer_artifacts(
            &env,
            &inventory,
            std::slice::from_ref(&parent.artifact.id),
        )
        .unwrap();
        assert_eq!(
            plan.preview(300).allocated_size,
            parent.artifact.allocated_bytes
        );
        let trash = root.join("fixture-trash");
        fs::create_dir(&trash).unwrap();
        let result = TrashExecutor::new(Arc::new(FixtureTrash {
            directory: trash.clone(),
        }))
        .execute(&env, plan);
        assert_eq!(result.moved_count, 1, "{:?}", result.items);
        assert_eq!(result.moved_allocated_size, parent.artifact.allocated_bytes);
        assert!(trash.join(output).is_dir());
        assert!(!project.join(output).exists());
        assert_eq!(
            fs::read(project.join("SOURCE-SENTINEL")).unwrap(),
            b"authored source outside output"
        );
        assert!(project.join("package.json").is_file());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn deployment_offline_unknown_and_nested_repository_parents_stay_protected() {
    for entry in [
        "BUILD_ID",
        "server/app/index.html",
        "standalone/server.js",
        "static/chunks/app.js",
        "cache/fetch-cache/state",
        "cache/images/offline",
        "unknown",
        "nested/.git",
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        copy_fixture("next-15.5.14", &project);
        let state = project.join(".next").join(entry);
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(&state, b"protected state").unwrap();
        let env = environment(&root);
        let inventory = scan(&env, &root);
        let parent = inventory
            .records
            .values()
            .find(|r| r.artifact.kind == DeveloperArtifactKind::NextOutput)
            .unwrap();
        assert_ne!(
            parent.artifact.status,
            DeveloperArtifactStatus::Complete,
            "{entry}"
        );
        assert!(TrashPlanner::from_developer_artifacts(
            &env,
            &inventory,
            std::slice::from_ref(&parent.artifact.id)
        )
        .is_err());
        let mut forged = inventory.clone();
        forged
            .records
            .get_mut(&parent.artifact.id)
            .unwrap()
            .artifact
            .status = DeveloperArtifactStatus::Complete;
        assert!(TrashPlanner::from_developer_artifacts(
            &env,
            &forged,
            std::slice::from_ref(&parent.artifact.id)
        )
        .is_err());
        assert_eq!(fs::read(&state).unwrap(), b"protected state");
    }
    for entry in [
        "output/server/index.js",
        "output/client/service-worker.js",
        "unknown",
        "nested/.git",
        "authored.rs",
        "recovery-keypair.json",
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        copy_fixture("sveltekit-2.37.1", &project);
        let state = project.join(".svelte-kit").join(entry);
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(&state, b"protected state").unwrap();
        let env = environment(&root);
        let inventory = scan(&env, &root);
        let parent = inventory
            .records
            .values()
            .find(|r| r.artifact.kind == DeveloperArtifactKind::SvelteKitOutput)
            .unwrap();
        assert_ne!(
            parent.artifact.status,
            DeveloperArtifactStatus::Complete,
            "{entry}"
        );
        assert!(TrashPlanner::from_developer_artifacts(
            &env,
            &inventory,
            std::slice::from_ref(&parent.artifact.id)
        )
        .is_err());
        assert_eq!(fs::read(state).unwrap(), b"protected state");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn generated_metadata_banners_cannot_hide_appended_authored_sentinels() {
    for name in [
        "ambient.d.ts",
        "non-ambient.d.ts",
        "tsconfig.json",
        "types/src/routes/$types.d.ts",
        "types/route_meta_data.json",
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        copy_fixture("sveltekit-2.37.1", &project);
        let path = project.join(".svelte-kit").join(name);
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend(b"\n// authored sentinel\n");
        fs::write(&path, &bytes).unwrap();
        let env = environment(&root);
        let inventory = scan(&env, &root);
        let parent = inventory
            .records
            .values()
            .find(|r| r.artifact.kind == DeveloperArtifactKind::SvelteKitOutput)
            .unwrap();
        assert_eq!(
            parent.artifact.status,
            DeveloperArtifactStatus::ObservationOnly,
            "{name}"
        );
        assert!(TrashPlanner::from_developer_artifacts(
            &env,
            &inventory,
            std::slice::from_ref(&parent.artifact.id)
        )
        .is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn overlapping_project_probes_finish_before_the_next_measurement_starts() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let project = root.join("project");
    let nested = project.join("packages/child");
    copy_fixture("next-15.5.14", &project);
    copy_fixture("next-15.5.14", &nested);
    fixture_git(&project, &["init", "-q"]);
    fixture_git(
        &project,
        &["add", "--", "package.json", "packages/child/package.json"],
    );
    let env = environment(&root).with_tool("git", "/usr/bin/git");
    let mut events = Vec::new();
    let inventory = DeveloperArtifactScanner::scan_workspaces(
        &env,
        &[workspace(&root)],
        FolderAccess::NotGated,
        Arc::new(AtomicBool::new(false)),
        |event| events.push(event),
    )
    .unwrap();
    assert_eq!(inventory.records.len(), 4);
    assert!(inventory
        .records
        .values()
        .all(|record| record.artifact.status == DeveloperArtifactStatus::Complete));

    let mut active = None;
    let mut finished = 0;
    for event in events {
        match event {
            DeveloperArtifactScanEvent::ArtifactMeasurementStarted { artifact_id, .. } => {
                assert!(
                    active.is_none(),
                    "Overlapping project probes ran concurrently"
                );
                active = Some(artifact_id);
            }
            DeveloperArtifactScanEvent::ArtifactFound { artifact } => {
                assert_eq!(active.take().as_deref(), Some(artifact.id.as_str()));
                finished += 1;
            }
            _ => {}
        }
    }
    assert!(active.is_none());
    assert_eq!(finished, 4);
}

#[cfg(target_os = "macos")]
#[test]
fn manifest_config_and_symlink_replacements_revoke_a_prepared_whole_plan() {
    use std::os::unix::fs::symlink;
    for mutation in ["manifest", "config", "symlink", "tracked", "compressed"] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        copy_fixture("next-15.5.14", &project);
        let mut env = environment(&root);
        if mutation == "tracked" {
            env = env.with_tool("git", "/usr/bin/git");
            fixture_git(&project, &["init", "-q"]);
            fixture_git(&project, &["add", "--", "package.json"]);
        }
        let inventory = scan(&env, &root);
        let parent = inventory
            .records
            .values()
            .find(|r| r.artifact.kind == DeveloperArtifactKind::NextOutput)
            .unwrap();
        assert_eq!(
            parent.artifact.status,
            DeveloperArtifactStatus::Complete,
            "{mutation}: {:?}",
            parent.artifact.evidence
        );
        let plan = TrashPlanner::from_developer_artifacts(
            &env,
            &inventory,
            std::slice::from_ref(&parent.artifact.id),
        )
        .unwrap();
        match mutation {
            "manifest" => fs::write(
                project.join("package.json"),
                r#"{"dependencies":{"next":"other"}}"#,
            )
            .unwrap(),
            "config" => fs::write(
                project.join("next.config.js"),
                "export default {\"distDir\":\"custom\"};",
            )
            .unwrap(),
            "symlink" => {
                fs::rename(project.join(".next"), root.join("accepted")).unwrap();
                symlink(root.join("accepted"), project.join(".next")).unwrap();
            }
            "tracked" => fixture_git(&project, &["add", "--", ".next"]),
            "compressed" => fs::write(
                project.join(".next/cache/webpack/client-development/index.pack.gz"),
                [1],
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let backend = Arc::new(neati_platform::MockTrashBackend::new());
        let result = TrashExecutor::new(backend.clone()).execute(&env, plan);
        assert_eq!(result.moved_count, 0, "{mutation}");
        assert!(backend.moved().is_empty());
        assert!(project.join(".next").exists());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn active_project_working_directory_blocks_whole_and_generated_child_without_unknown_consent() {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let project = root.join("project");
    copy_fixture("next-15.5.14", &project);
    let env = environment(&root);
    let inventory = scan(&env, &root);
    let parent = inventory
        .records
        .values()
        .find(|r| r.artifact.kind == DeveloperArtifactKind::NextOutput)
        .unwrap();
    let plan = TrashPlanner::from_developer_artifacts(
        &env,
        &inventory,
        std::slice::from_ref(&parent.artifact.id),
    )
    .unwrap();
    let _owner = Child(
        std::process::Command::new("/bin/sleep")
            .arg("60")
            .current_dir(&project)
            .spawn()
            .unwrap(),
    );
    let observed = scan(&env, &root);
    assert!(observed
        .records
        .values()
        .all(|r| r.artifact.status == DeveloperArtifactStatus::SafetyBlocked));
    let backend = Arc::new(neati_platform::MockTrashBackend::new());
    let result = TrashExecutor::new(backend.clone()).execute(&env, plan);
    assert_eq!(result.moved_count, 0);
    assert!(backend.moved().is_empty());
    assert!(project.join(".next").exists());
}
