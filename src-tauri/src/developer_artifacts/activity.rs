//! When a project was last worked on (#504).
//!
//! Cheap metadata only. File contents are never read, and the number of
//! entries examined per project is bounded. The answer is the newest of these:
//!
//! - the project folder and its direct children, including artifact folders
//!   (a build or an install is work);
//! - the direct children of its ordinary subfolders, so an edit saved inside
//!   `src/` moves the answer;
//! - Git's own record of work in the checkout's git directory (the
//!   worktree's own one for a linked worktree): the modification time of
//!   `index` and `HEAD`, and the time written in the last `logs/HEAD` entry.
//!   The reflog's own modification time is not used: `git gc` rewrites every
//!   worktree's reflog when it expires entries, which is maintenance, not work.
//!
//! Finder's `.DS_Store` is not work and is ignored. A process whose working
//! directory is inside the project is checked separately by the caller.
use super::*;

/// Entries examined per project before the observation stops.
pub(super) const ACTIVITY_ENTRY_BUDGET: usize = 4096;

/// Files Finder writes when a folder is merely viewed.
const VIEW_ONLY_NAMES: &[&str] = &[".DS_Store"];

/// Folder names an artifact rule owns. Their own modification time counts as
/// activity, but the observation does not descend into them: they can hold
/// hundreds of thousands of entries that say nothing about the person's work.
fn is_artifact_folder_name(name: &str) -> bool {
    rules::RULES
        .iter()
        .chain(project_caches::RULES.iter())
        .any(|rule| {
            rule.discovery_name == Some(name) || rule.relative.split('/').next() == Some(name)
        })
}

fn modified_secs(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

/// The newest work signal under `project_root`, in seconds since the epoch.
///
/// `None` when the project folder itself cannot be read; a caller treats that
/// as unknown activity, never as inactivity.
pub fn project_last_activity(
    environment: &PlatformEnvironment,
    project_root: &Path,
) -> Option<u64> {
    let root_metadata = fs::symlink_metadata(project_root).ok()?;
    if !root_metadata.is_dir() {
        return None;
    }
    let mut newest = modified_secs(&root_metadata);
    let mut budget = ACTIVITY_ENTRY_BUDGET;
    let mut observe = |metadata: &fs::Metadata| {
        if let Some(seconds) = modified_secs(metadata) {
            newest = Some(newest.map_or(seconds, |current| current.max(seconds)));
        }
    };

    let mut subfolders = Vec::new();
    if let Ok(entries) = fs::read_dir(project_root) {
        for entry in entries.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if VIEW_ONLY_NAMES.contains(&name.as_ref()) {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            observe(&metadata);
            if metadata.is_dir()
                && name != ".git"
                && !is_artifact_folder_name(&name)
                && !SymlinkGuard::is_symlink(&entry.path())
            {
                subfolders.push(entry.path());
            }
        }
    }
    for folder in subfolders {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if VIEW_ONLY_NAMES.contains(&entry.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            if let Ok(metadata) = fs::symlink_metadata(entry.path()) {
                observe(&metadata);
            }
        }
        if budget == 0 {
            break;
        }
    }

    if let crate::git::GitDirectory::Resolved { git_dir, .. } =
        crate::git::git_directory(project_root, environment)
    {
        for relative in ["index", "HEAD"] {
            if let Ok(metadata) = fs::symlink_metadata(git_dir.join(relative)) {
                observe(&metadata);
            }
        }
        if let Some(seconds) = last_reflog_time(&git_dir.join("logs/HEAD")) {
            newest = Some(newest.map_or(seconds, |current| current.max(seconds)));
        }
    }
    newest
}

/// Bytes read from the end of a reflog: one entry is a few hundred bytes.
const REFLOG_TAIL_BYTES: u64 = 8 * 1024;

/// The time recorded in the last reflog entry, in seconds since the epoch.
///
/// An entry is `<old> <new> <name> <<email>> <seconds> <zone>\t<message>`.
/// Only the bounded tail of a regular, unlinked file is read.
pub(crate) fn last_reflog_time(path: &Path) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let start = metadata.len().saturating_sub(REFLOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.take(REFLOG_TAIL_BYTES).read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let header = line.split('\t').next()?;
    let mut fields = header.rsplitn(3, ' ');
    let _zone = fields.next()?;
    fields.next()?.parse().ok()
}

/// Whether a running process works inside `project_root`.
pub fn has_working_process(project_root: &Path, working_directories: &[PathBuf]) -> bool {
    working_directories
        .iter()
        .any(|directory| directory.starts_with(project_root))
}

/// The working directories of running processes, observed once per scan.
pub fn running_working_directories() -> Vec<PathBuf> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
    );
    let mut directories: Vec<PathBuf> = system
        .processes()
        .values()
        .filter_map(|process| process.cwd().map(PathBuf::from))
        .filter(|directory| directory.parent().is_some())
        .collect();
    directories.sort();
    directories.dedup();
    directories
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, FileTimes};
    use std::time::Duration;

    const DAY: u64 = 24 * 60 * 60;

    fn set_age(path: &Path, now: u64, days: u64) {
        let time = UNIX_EPOCH + Duration::from_secs(now - days * DAY);
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(time).set_accessed(time))
            .unwrap();
    }

    fn environment(home: &Path) -> PlatformEnvironment {
        PlatformEnvironment::simulated(path_algebra::PathFlavor::current()).with_home(home)
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// A project whose every entry is `days` old, children before parents.
    fn aged_project(root: &Path, days: u64) {
        let now = now();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("package.json"), "{}").unwrap();
        fs::write(root.join("src/nested/deep.ts"), "").unwrap();
        fs::write(root.join("node_modules/pkg/index.js"), "").unwrap();
        for path in [
            "src/nested/deep.ts",
            "src/nested",
            "node_modules/pkg/index.js",
            "node_modules/pkg",
            "node_modules",
            "package.json",
            "src",
            "",
        ] {
            set_age(&root.join(path), now, days);
        }
    }

    fn age_days(activity: Option<u64>) -> u64 {
        (now() - activity.unwrap()) / DAY
    }

    #[test]
    fn an_untouched_project_reports_its_newest_entry() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        aged_project(&root, 40);
        let activity = project_last_activity(&environment(temp.path()), &root);
        assert_eq!(age_days(activity), 40);
    }

    #[test]
    fn an_edit_inside_a_subfolder_counts_as_work() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        aged_project(&root, 40);
        let now = now();
        fs::write(root.join("src/main.ts"), "").unwrap();
        set_age(&root.join("src"), now, 40);
        let activity = project_last_activity(&environment(temp.path()), &root);
        assert_eq!(age_days(activity), 0);
    }

    #[test]
    fn a_finder_view_is_not_work_and_artifact_contents_are_not_read() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        aged_project(&root, 40);
        let now = now();
        fs::write(root.join(".DS_Store"), "").unwrap();
        fs::write(root.join("node_modules/pkg/new.js"), "").unwrap();
        set_age(&root.join("node_modules/pkg"), now, 40);
        set_age(&root, now, 40);
        let activity = project_last_activity(&environment(temp.path()), &root);
        assert_eq!(age_days(activity), 40);
    }

    #[test]
    fn git_records_count_as_work() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        aged_project(&root, 40);
        let now = now();
        let git = root.join(".git");
        fs::create_dir_all(git.join("logs")).unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(git.join("logs/HEAD"), "").unwrap();
        for path in ["HEAD", "logs", ""] {
            set_age(&git.join(path), now, 40);
        }
        set_age(&root, now, 40);
        let environment = environment(temp.path());
        // An empty reflog records nothing; its fresh modification time alone
        // is not work (git gc rewrites reflogs).
        assert_eq!(age_days(project_last_activity(&environment, &root)), 40);
        let entry = |seconds: u64| {
            format!(
                "{zero} {one} Fixture <fixture@example.invalid> {seconds} +0900\tcommit: work\n",
                zero = "0".repeat(40),
                one = "1".repeat(40)
            )
        };
        fs::write(
            git.join("logs/HEAD"),
            entry(now - 50 * DAY) + &entry(now - 2 * DAY),
        )
        .unwrap();
        set_age(&git.join("logs/HEAD"), now, 40);
        set_age(&git.join("logs"), now, 40);
        set_age(&git, now, 40);
        assert_eq!(age_days(project_last_activity(&environment, &root)), 2);
        // Maintenance that rewrites an old reflog does not make it recent.
        fs::write(git.join("logs/HEAD"), entry(now - 45 * DAY)).unwrap();
        set_age(&git.join("logs"), now, 40);
        set_age(&git, now, 40);
        assert_eq!(age_days(project_last_activity(&environment, &root)), 40);
    }

    #[test]
    fn a_reflog_entry_time_is_read_from_its_last_line_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("HEAD");
        fs::write(
            &path,
            "a b N <n@e> 100 +0000\tone\nc d Name With Spaces <n@e> 200 -0500\ttwo\tthree\n\n",
        )
        .unwrap();
        assert_eq!(last_reflog_time(&path), Some(200));
        fs::write(&path, "garbage\n").unwrap();
        assert_eq!(last_reflog_time(&path), None);
        assert_eq!(last_reflog_time(&temp.path().join("missing")), None);
    }

    #[test]
    fn an_unreadable_project_is_unknown() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            project_last_activity(&environment(temp.path()), &temp.path().join("missing")),
            None
        );
    }

    #[test]
    fn a_working_process_inside_the_project_is_detected_by_path_components() {
        let root = Path::new("/work/app");
        assert!(has_working_process(root, &[PathBuf::from("/work/app/src")]));
        assert!(has_working_process(root, &[PathBuf::from("/work/app")]));
        assert!(!has_working_process(
            root,
            &[PathBuf::from("/work/app-two")]
        ));
        assert!(!has_working_process(root, &[PathBuf::from("/work")]));
    }
}
