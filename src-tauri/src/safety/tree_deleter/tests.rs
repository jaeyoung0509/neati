use super::*;
use crate::models::NeatiError;
use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;

/// Deletion tests exercise argument threading, not path resolution: no
/// exclusion in these tests needs the environment to expand.
fn environment() -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::current()).with_home(
        if PathFlavor::current().is_windows() {
            r"Z:\NeatiFixtureHome"
        } else {
            "/neati-fixture-home"
        },
    )
}

#[test]
fn exclusion_matching_uses_the_stated_path_flavor() {
    let environment = PlatformEnvironment::simulated(PathFlavor::Windows).with_home(
        if PathFlavor::Windows.is_windows() {
            r"Z:\NeatiFixtureHome"
        } else {
            "/neati-fixture-home"
        },
    );
    let exclusions = vec![r"C:\Users\Alice\Cache".to_string()];

    assert!(SafeTreeDeleter::is_excluded(
        Path::new(r"c:/users/alice/cache/nested/file.bin"),
        &exclusions,
        &environment
    ));
    assert!(!SafeTreeDeleter::is_excluded(
        Path::new(r"C:\Users\Alice\Cache-Other\file.bin"),
        &exclusions,
        &environment
    ));
}

#[cfg(windows)]
#[test]
fn windows_verified_handle_blocks_replacement_until_disposition() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("payload.bin");
    let renamed = dir.path().join("renamed.bin");
    std::fs::write(&target, b"payload").unwrap();

    let handle = WindowsDeleteHandle::open(&target).unwrap();
    assert!(
        std::fs::rename(&target, &renamed).is_err(),
        "a retained non-share-delete handle must prevent path replacement"
    );
    handle.delete().unwrap();
    assert!(!target.exists());
    assert!(!renamed.exists());
}

#[cfg(unix)]
#[test]
fn parent_replacement_between_verification_and_unlink_leaves_outside_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let parent = root.join("parent");
    std::fs::create_dir_all(&parent).unwrap();
    let child = parent.join("child.bin");
    std::fs::write(&child, b"inside").unwrap();

    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let outside_target = outside.join("child.bin");
    std::fs::write(&outside_target, b"outside").unwrap();

    // Open the verified parent descriptor before replacement.
    let parent_file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&parent)
        .unwrap();

    // Simulate a parent-component race: replace `parent` with a symlink to
    // the outside directory after validation.
    std::fs::remove_file(&child).unwrap();
    std::fs::remove_dir(&parent).unwrap();
    std::os::unix::fs::symlink(&outside, &parent).unwrap();

    // The verified descriptor still points at the original directory, so
    // unlinking a stale basename through it must fail without touching
    // the outside target.
    let result =
        SafeTreeDeleter::unlink_via_parent(&parent_file, std::ffi::OsStr::new("child.bin"), false);
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(&outside_target).unwrap(),
        b"outside",
        "outside target must remain untouched"
    );
}

#[cfg(unix)]
#[test]
fn pinned_scope_does_not_follow_a_replaced_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let moved_root = dir.path().join("moved-root");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(root.join("inside.bin"), b"inside").unwrap();
    let outside_target = outside.join("outside.bin");
    std::fs::write(&outside_target, b"outside").unwrap();

    let scope = SafeTreeDeleter::capture_verified_scope(&root).unwrap();
    std::fs::rename(&root, &moved_root).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();

    let redirected = root.join("outside.bin");
    assert!(SafeTreeDeleter::validate_verified_scope(&redirected, &scope).is_err());
    let mut report = TreeDeleteReport::default();
    SafeTreeDeleter::delete_entry(&redirected, &scope, &[], &environment(), None, &mut report);

    assert_eq!(std::fs::read(&outside_target).unwrap(), b"outside");
    assert!(moved_root.join("inside.bin").exists());
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("escaped the verified cleanup target")));
}

#[cfg(unix)]
#[test]
fn directory_identity_is_verified_through_the_open_handle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let moved_root = dir.path().join("moved-root");
    std::fs::create_dir_all(&root).unwrap();
    let metadata = std::fs::symlink_metadata(&root).unwrap();
    let snapshot = SafeTreeDeleter::prepare_directory(&root, &metadata).unwrap();

    std::fs::rename(&root, &moved_root).unwrap();
    std::fs::create_dir(&root).unwrap();

    assert!(SafeTreeDeleter::verify_directory_identity(&root, &snapshot).is_ok());
    drop(snapshot);
}

/// The walk must not inspect a replacement at the approved spelling and
/// then unlink the same basename from the directory handle captured before
/// the replacement. The explicit sequence between `prepare_directory` and
/// `delete_dir_contents_via_fd` is the deterministic barrier for the race.
#[cfg(unix)]
#[test]
fn root_replacement_between_handle_acquisition_and_enumeration_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("cache");
    let relocated = fixture.path().join("relocated");
    std::fs::create_dir(&root).unwrap();
    let original_payload = root.join("payload.bin");
    std::fs::write(&original_payload, b"original protected payload").unwrap();

    let scope = SafeTreeDeleter::capture_verified_scope(&root).unwrap();
    let metadata = std::fs::symlink_metadata(&root).unwrap();
    let snapshot = SafeTreeDeleter::prepare_directory(&root, &metadata).unwrap();

    // Controlled barrier: the retained descriptor names `relocated`, while
    // the approved spelling now names a different ordinary directory.
    std::fs::rename(&root, &relocated).unwrap();
    std::fs::create_dir(&root).unwrap();
    let replacement_payload = root.join("payload.bin");
    std::fs::write(&replacement_payload, b"replacement payload").unwrap();

    let relocated_payload = relocated.join("payload.bin");
    let mut executable = std::fs::metadata(&relocated_payload).unwrap().permissions();
    executable.set_mode(executable.mode() | 0o100);
    std::fs::set_permissions(&relocated_payload, executable).unwrap();

    let mut report = TreeDeleteReport {
        structured_state_policy: Some(crate::models::StructuredStatePolicy::ProtectAll),
        ..Default::default()
    };
    SafeTreeDeleter::delete_dir_contents_via_fd(
        &root,
        snapshot.directory.as_ref().unwrap(),
        &scope,
        &[],
        &environment(),
        None,
        &mut report,
    );

    assert!(
        relocated_payload.exists(),
        "the protected entry behind the retained handle must remain"
    );
    assert!(
        replacement_payload.exists(),
        "the replacement entry inspected through the pathname must remain"
    );
    assert_eq!(report.reclaimed_bytes, 0);
    assert_eq!(report.deleted_files, 0);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("changed during cleanup")),
        "the refusal must name the identity change: {:?}",
        report.errors
    );
}

#[cfg(unix)]
#[test]
fn recursive_child_open_is_bound_to_the_parent_observation() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("cache");
    let child = root.join("nested");
    let relocated = root.join("relocated");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(child.join("original.bin"), b"original").unwrap();

    let root_metadata = std::fs::symlink_metadata(&root).unwrap();
    let root_snapshot = SafeTreeDeleter::prepare_directory(&root, &root_metadata).unwrap();
    let root_handle = root_snapshot.directory.as_ref().unwrap();
    let observed = SafeTreeDeleter::metadata_at(root_handle, OsStr::new("nested")).unwrap();

    std::fs::rename(&child, &relocated).unwrap();
    std::fs::create_dir(&child).unwrap();
    std::fs::write(child.join("replacement.bin"), b"replacement").unwrap();

    let result =
        SafeTreeDeleter::prepare_directory_at(root_handle, OsStr::new("nested"), observed, &child);
    assert!(result
        .expect_err("a replacement child must not inherit the prior observation")
        .contains("changed during cleanup"));
    assert!(relocated.join("original.bin").exists());
    assert!(child.join("replacement.bin").exists());
}

#[cfg(unix)]
#[test]
fn descriptor_relative_file_removal_deletes_inside_target() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let payload = root.join("payload.bin");
    std::fs::write(&payload, b"payload").unwrap();

    let report = SafeTreeDeleter::delete_contents_unchecked(&root, &[], &environment());
    assert!(report.is_success(), "errors: {:?}", report.errors);
    assert!(!payload.exists());
    assert!(root.is_dir());
}

#[test]
fn symlink_metadata_absence_reports_missing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist-link");
    assert!(matches!(
        SymlinkGuard::is_symlink_strict(&missing),
        Err(NeatiError::Missing(_))
    ));
    let environment = neati_platform::PlatformEnvironment::native();
    assert!(matches!(
        SymlinkGuard::validate_canonical_blacklist_strict(&missing, &environment),
        Err(NeatiError::Missing(_))
    ));
}

/// A nested entry that vanished before the walk reached it is already in
/// the desired state: it is counted as skipped, never as an error, so the
/// surrounding target stays successful.
#[test]
fn vanished_entry_is_a_neutral_skip_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("vanished.bin");

    let mut report = TreeDeleteReport::default();
    let scope = SafeTreeDeleter::capture_verified_scope(dir.path()).unwrap();
    SafeTreeDeleter::delete_entry(&missing, &scope, &[], &environment(), None, &mut report);

    assert!(report.is_success(), "errors: {:?}", report.errors);
    assert_eq!(report.skipped_files, 1);
    assert_eq!(report.deleted_files, 0);
    assert_eq!(report.reclaimed_bytes, 0);
}

/// A child that vanished before the walk reached it must not fail the whole
/// recursive target, and its bytes must not be claimed as reclaimed.
#[cfg(unix)]
#[test]
fn vanished_child_is_excluded_from_recursive_reclaim() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("cache");
    std::fs::create_dir(&root).unwrap();
    let vanished = root.join("gone.bin");
    std::fs::write(&vanished, vec![7u8; 64 * 1024]).unwrap();
    let kept = root.join("kept.bin");
    std::fs::write(&kept, vec![7u8; 8 * 1024]).unwrap();
    let nested_dir = root.join("nested");
    std::fs::create_dir(&nested_dir).unwrap();
    let nested = nested_dir.join("inner.bin");
    std::fs::write(&nested, vec![7u8; 4 * 1024]).unwrap();

    let expected_bytes = std::fs::metadata(&kept).unwrap().blocks() * 512
        + std::fs::metadata(&nested).unwrap().blocks() * 512;

    std::fs::remove_file(&vanished).unwrap();

    let report = SafeTreeDeleter::delete_contents_unchecked(&root, &[], &environment());
    assert!(report.is_success(), "errors: {:?}", report.errors);
    assert!(report.errors.is_empty());
    assert_eq!(report.reclaimed_bytes, expected_bytes);
    assert!(!kept.exists());
    assert!(!nested_dir.exists());
    // `delete_contents` preserves the cache root itself.
    assert!(root.is_dir());
}

/// Removing an already-absent path is a success report, not an error.
#[test]
fn delete_path_on_an_already_removed_path_is_a_success_report() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("already-gone");
    std::fs::create_dir(&missing).unwrap();
    std::fs::remove_dir(&missing).unwrap();

    let report = SafeTreeDeleter::delete_path_unchecked(&missing, &[], &environment());
    assert!(report.is_success());
    assert!(report.errors.is_empty());
    assert_eq!(report.reclaimed_bytes, 0);
    assert_eq!(report.deleted_files, 0);
}

#[test]
fn deletes_readonly_files_and_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ro_root");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let file1 = root.join("file1.txt");
    let file2 = sub.join("file2.txt");
    std::fs::write(&file1, b"file 1").unwrap();
    std::fs::write(&file2, b"file 2").unwrap();

    let mut perms = std::fs::metadata(&file1).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&file1, perms).unwrap();

    let report = SafeTreeDeleter::delete_path_unchecked(&root, &[], &environment());
    assert!(report.is_success(), "errors: {:?}", report.errors);
    assert!(!root.exists());
}

#[cfg(windows)]
#[test]
fn windows_deletes_readonly_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("readonly.bin");
    std::fs::write(&target, b"readonly payload").unwrap();

    let mut perms = std::fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&target, perms).unwrap();
    assert!(std::fs::metadata(&target).unwrap().permissions().readonly());

    let report = SafeTreeDeleter::delete_contents_unchecked(dir.path(), &[], &environment());
    assert!(report.is_success(), "errors: {:?}", report.errors);
    assert!(!target.exists(), "readonly file must be deleted");
}

#[cfg(windows)]
#[test]
fn windows_deletes_readonly_directory_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("readonly_root");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let file1 = root.join("file1.txt");
    let file2 = sub.join("file2.txt");
    std::fs::write(&file1, b"file 1").unwrap();
    std::fs::write(&file2, b"file 2").unwrap();

    for f in [&file1, &file2] {
        let mut p = std::fs::metadata(f).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(f, p).unwrap();
        assert!(std::fs::metadata(f).unwrap().permissions().readonly());
    }

    let mut sub_p = std::fs::metadata(&sub).unwrap().permissions();
    sub_p.set_readonly(true);
    std::fs::set_permissions(&sub, sub_p).unwrap();

    let mut root_p = std::fs::metadata(&root).unwrap().permissions();
    root_p.set_readonly(true);
    std::fs::set_permissions(&root, root_p).unwrap();

    let report = SafeTreeDeleter::delete_path_unchecked(&root, &[], &environment());
    assert!(report.is_success(), "errors: {:?}", report.errors);
    assert!(!root.exists(), "readonly directory tree must be deleted");
}

#[cfg(windows)]
#[test]
fn windows_sharing_violation_retry_and_reporting() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("locked.bin");
    std::fs::write(&target, b"locked payload").unwrap();
    // The rest of the directory is what a locked file must not cost: a
    // sibling file and a nested directory that are freely removable.
    let sibling = dir.path().join("reclaimable.bin");
    std::fs::write(&sibling, vec![7u8; 4_096]).unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("inner.bin"), vec![9u8; 2_048]).unwrap();

    // Hold an exclusive lock with share_mode(0)
    let lock_handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&target)
        .unwrap();

    let report = SafeTreeDeleter::delete_contents_unchecked(dir.path(), &[], &environment());
    assert!(
        !report.is_success(),
        "deletion must fail while file is exclusively locked"
    );
    assert_eq!(
        report.errors.len(),
        1,
        "only the locked entry is a failure: {:?}",
        report.errors
    );
    let error_msg = &report.errors[0];
    assert!(
        error_msg.contains("Sharing violation")
            || error_msg.contains("used by another process")
            || error_msg.contains("os error 32"),
        "Error must report Win32 sharing violation distinctly, got: {}",
        error_msg
    );
    // The locked file is the remainder, and the rest of the directory is
    // reclaimed around it rather than abandoned with it.
    assert!(target.exists(), "the locked file stays");
    assert!(!sibling.exists(), "a removable sibling is still removed");
    assert!(
        !nested.exists(),
        "a nested directory that became empty is still removed"
    );
    assert!(
        report.reclaimed_bytes >= 4_096 + 2_048,
        "the reclaimed amount states what was removed beside the lock: {report:?}"
    );

    drop(lock_handle);

    let report2 = SafeTreeDeleter::delete_contents_unchecked(dir.path(), &[], &environment());
    assert!(report2.is_success(), "errors: {:?}", report2.errors);
    assert!(!target.exists());
}

#[cfg(windows)]
#[test]
fn windows_sharing_violation_retries_transient_lock() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("transient.bin");
    std::fs::write(&target, b"transient payload").unwrap();

    let lock_handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&target)
        .unwrap();

    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        drop(lock_handle);
    });

    let report = SafeTreeDeleter::delete_contents_unchecked(dir.path(), &[], &environment());
    handle.join().unwrap();

    assert!(
        report.is_success(),
        "transient lock must succeed after retry, got errors: {:?}",
        report.errors
    );
    assert!(!target.exists());
}

#[cfg(windows)]
#[test]
fn windows_readonly_rollback_on_deletion_failure() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("readonly_fail.bin");
    std::fs::write(&target, b"content").unwrap();
    let mut perms = std::fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&target, perms).unwrap();
    assert!(std::fs::metadata(&target).unwrap().permissions().readonly());

    let mut handle = WindowsDeleteHandle::open(&target).unwrap();
    assert!(handle.was_readonly);
    handle.clear_readonly().unwrap();
    assert!(!std::fs::metadata(&target).unwrap().permissions().readonly());

    // Restore readonly on failure
    handle.restore_readonly().unwrap();
    assert!(std::fs::metadata(&target).unwrap().permissions().readonly());
}

#[cfg(unix)]
#[test]
fn the_path_walker_refuses_recent_files_and_keeps_recent_directories() {
    use std::time::{Duration, SystemTime};

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("cache");
    let old_dir = root.join("old");
    let recent_dir = root.join("recent");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&recent_dir).unwrap();
    let old_file = old_dir.join("old.bin");
    let recent_file = old_dir.join("recent.bin");
    let recent_only = recent_dir.join("recent.bin");
    std::fs::write(&old_file, vec![0u8; 64]).unwrap();
    std::fs::write(&recent_file, vec![0u8; 64]).unwrap();
    std::fs::write(&recent_only, vec![0u8; 64]).unwrap();

    let old = SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60);
    for path in [&old_file, &recent_file, &recent_only] {
        let handle = std::fs::File::options().write(true).open(path).unwrap();
        let modified = if path == &old_file {
            old
        } else {
            SystemTime::now()
        };
        handle.set_modified(modified).unwrap();
    }
    for path in [&old_dir, &recent_dir] {
        let handle = std::fs::File::open(path).unwrap();
        handle
            .set_modified(SystemTime::now() - Duration::from_secs(10 * 24 * 60 * 60))
            .unwrap();
    }

    let environment = environment();
    let policy = super::super::StaleEntryPolicy::from_days(7);
    let report = SafeTreeDeleter::delete_path_with_policy(
        &root,
        &[],
        &environment,
        Some(crate::models::StructuredStatePolicy::ProtectAll),
        Some(policy),
    );

    assert!(!old_file.exists(), "a stale file is removed");
    assert!(recent_file.exists(), "a recent file beside it stays");
    assert!(
        recent_dir.exists(),
        "a directory is descended into and kept"
    );
    assert!(recent_only.exists(), "its recent entry stays");
    assert_eq!(report.skipped_files, 2);
    assert!(!report.is_success());
    assert_eq!(report.errors.len(), 3);
    assert!(root.is_dir());
    assert!(report.reclaimed_bytes > 0);
}

/// The path walker judges a file on its own age. It is the route a
/// filesystem that refuses a parent descriptor falls back to, and it must
/// refuse the same entries as the descriptor-relative walk.
#[cfg(unix)]
#[test]
fn a_path_entry_is_judged_on_its_own_age() {
    use std::time::{Duration, SystemTime};

    let fixture = tempfile::tempdir().unwrap();
    let dir = fixture.path().join("cache");
    std::fs::create_dir_all(&dir).unwrap();
    let recent = dir.join("recent.bin");
    let old = dir.join("old.bin");
    std::fs::write(&recent, vec![0u8; 64]).unwrap();
    std::fs::write(&old, vec![0u8; 64]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60))
        .unwrap();

    let environment = environment();
    let policy = super::super::StaleEntryPolicy::from_days(7);
    let mut report = TreeDeleteReport {
        structured_state_policy: Some(crate::models::StructuredStatePolicy::ProtectAll),
        ..Default::default()
    };
    let scope = SafeTreeDeleter::capture_verified_scope(&dir).unwrap();
    SafeTreeDeleter::delete_entry(
        &recent,
        &scope,
        &[],
        &environment,
        Some(policy),
        &mut report,
    );
    assert!(recent.exists(), "a recent entry is refused");
    assert_eq!(report.skipped_files, 1);
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    SafeTreeDeleter::delete_entry(&old, &scope, &[], &environment, Some(policy), &mut report);
    assert!(!old.exists(), "a stale entry on the same route is removed");
    assert_eq!(report.deleted_files, 1);
}
