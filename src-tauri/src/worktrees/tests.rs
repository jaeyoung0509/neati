use super::*;
use crate::models::IdleRule;
use std::process::Command;

const NOW: u64 = 2_000_000_000;

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    code: PathBuf,
    repository: PathBuf,
    environment: PlatformEnvironment,
}

fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .current_dir(directory)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository at `~/code/app` with one commit and ignored build output.
fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(temp.path()).unwrap();
    let code = home.join("code");
    let repository = code.join("app");
    fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    fs::write(repository.join(".gitignore"), "target/\n").unwrap();
    fs::write(repository.join("main.rs"), "fn main() {}\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-q", "-m", "initial"]);
    let environment = PlatformEnvironment::simulated(path_algebra::PathFlavor::current())
        .with_home(&home)
        .with_temp_dir(&home)
        .with_tool("git", PathBuf::from("/usr/bin/git"));
    Fixture {
        _temp: temp,
        home,
        code,
        repository,
        environment,
    }
}

impl Fixture {
    /// Adds a linked worktree on a new branch, with ignored build output.
    fn worktree(&self, name: &str, branch: &str) -> PathBuf {
        let path = self.code.join(name);
        git(
            &self.repository,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        );
        fs::create_dir_all(path.join("target/debug")).unwrap();
        fs::write(path.join("target/debug/app"), [7u8; 4096]).unwrap();
        path
    }

    fn folders(&self) -> Vec<String> {
        vec![self.code.to_string_lossy().into_owned()]
    }
}

fn rules(enabled: bool) -> CleanupRules {
    CleanupRules {
        worktrees: IdleRule {
            enabled,
            idle_days: 14,
        },
        ..CleanupRules::default()
    }
}

fn scan_with(
    fixture: &Fixture,
    rules: CleanupRules,
    days_ago: Option<u64>,
    use_state: OpenFileState,
    working: Vec<PathBuf>,
) -> WorktreeScan {
    let activity = move |_: &Path| days_ago.map(|days| NOW - days * DAY_SECONDS);
    let use_probe = move |paths: &[PathBuf], _: &dyn Fn() -> bool| vec![use_state; paths.len()];
    let working = move || working.clone();
    let observation = Observation {
        now: NOW,
        rules,
        use_probe: &use_probe,
        activity: &activity,
        working_directories: &working,
    };
    scan(
        &fixture.environment,
        &fixture.folders(),
        &observation,
        &|| false,
    )
}

fn only_item(scan: &WorktreeScan) -> &WorktreeItem {
    assert_eq!(scan.inventory.items.len(), 1, "{:?}", scan.inventory);
    &scan.inventory.items[0]
}

#[test]
fn porcelain_entries_keep_their_attributes() {
    let output = b"worktree /r\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
worktree /r-a\0HEAD 2222222222222222222222222222222222222222\0detached\0locked agent busy\0\0\
worktree /r-b\0HEAD 3333333333333333333333333333333333333333\0branch refs/heads/b\0prunable gitdir file points to non-existent location\0\0";
    let entries = parse_porcelain(output);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(entries[1].path, PathBuf::from("/r-a"));
    assert!(entries[1].branch.is_none());
    assert!(entries[1].locked);
    assert!(entries[2].prunable);
    assert!(parse_porcelain(b"worktree /bare\0bare\0\0")[0].bare);
}

#[test]
fn a_clean_inactive_worktree_is_finished_and_joins_clean_only_with_the_rule() {
    let fixture = fixture();
    let path = fixture.worktree("app-feature", "feature");

    let off = scan_with(
        &fixture,
        rules(false),
        Some(30),
        OpenFileState::Idle,
        vec![],
    );
    let item = only_item(&off);
    assert_eq!(off.inventory.repositories, 1);
    assert_eq!(item.state, WorktreeState::Finished);
    assert!(!item.selected_by_default);
    assert_eq!(item.branch.as_deref(), Some("feature"));
    assert_eq!(item.path, path.to_string_lossy());
    assert!(item.allocated_bytes >= 4096);

    let on = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert!(only_item(&on).selected_by_default);
}

#[test]
fn unique_work_keeps_a_worktree() {
    let fixture = fixture();
    let path = fixture.worktree("app-feature", "feature");

    fs::write(path.join("main.rs"), "fn main() { changed() }\n").unwrap();
    let modified = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert_eq!(only_item(&modified).state, WorktreeState::HasChanges);
    assert!(!only_item(&modified).selected_by_default);
    git(&path, &["checkout", "-q", "--", "main.rs"]);

    fs::write(path.join("notes.md"), "only here\n").unwrap();
    let untracked = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert_eq!(only_item(&untracked).state, WorktreeState::HasChanges);
    fs::remove_file(path.join("notes.md")).unwrap();

    git(
        &fixture.repository,
        &["worktree", "lock", path.to_str().unwrap()],
    );
    let locked = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert_eq!(only_item(&locked).state, WorktreeState::Locked);
}

#[test]
fn a_detached_commit_that_only_lives_here_keeps_the_worktree() {
    let fixture = fixture();
    let path = fixture.code.join("app-detached");
    git(
        &fixture.repository,
        &["worktree", "add", "-q", "--detach", path.to_str().unwrap()],
    );
    let contained = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert_eq!(only_item(&contained).state, WorktreeState::Finished);
    assert!(only_item(&contained).branch.is_none());

    fs::write(path.join("main.rs"), "fn main() { only_here() }\n").unwrap();
    git(&path, &["commit", "-q", "-am", "only here"]);
    let unique = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert_eq!(only_item(&unique).state, WorktreeState::CommitsOnlyHere);
}

#[test]
fn recent_work_unknown_activity_and_use_keep_a_worktree() {
    let fixture = fixture();
    let path = fixture.worktree("app-feature", "feature");
    let state = |days: Option<u64>, use_state, working: Vec<PathBuf>| {
        only_item(&scan_with(&fixture, rules(true), days, use_state, working)).state
    };
    assert_eq!(
        state(Some(3), OpenFileState::Idle, vec![]),
        WorktreeState::RecentlyActive
    );
    assert_eq!(
        state(None, OpenFileState::Idle, vec![]),
        WorktreeState::ActivityUnknown
    );
    assert_eq!(
        state(Some(30), OpenFileState::Idle, vec![path.join("src")]),
        WorktreeState::RecentlyActive
    );
    assert_eq!(
        state(Some(30), OpenFileState::InUse, vec![]),
        WorktreeState::InUse
    );
    assert_eq!(
        state(Some(30), OpenFileState::Unknown, vec![]),
        WorktreeState::UseUnknown
    );
}

#[test]
fn a_missing_folder_is_reported_and_never_removable() {
    let fixture = fixture();
    let path = fixture.worktree("app-feature", "feature");
    fs::remove_dir_all(&path).unwrap();
    let scan = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    assert_eq!(only_item(&scan).state, WorktreeState::Missing);
    assert!(!only_item(&scan).selected_by_default);
}

/// Runs the removal adapter with fixed observations.
fn remove_with(
    fixture: &Fixture,
    record: &WorktreeRecord,
    days_ago: u64,
    use_state: OpenFileState,
) -> Result<u64, String> {
    let activity = move |_: &Path| Some(NOW - days_ago * DAY_SECONDS);
    let use_probe = move |paths: &[PathBuf], _: &dyn Fn() -> bool| vec![use_state; paths.len()];
    let observation = Observation {
        now: NOW,
        rules: rules(true),
        use_probe: &use_probe,
        activity: &activity,
        working_directories: &Vec::new,
    };
    removal::remove(&fixture.environment, record, &observation)
}

#[test]
fn removal_goes_through_git_and_keeps_the_branch() {
    let fixture = fixture();
    let path = fixture.worktree("app-feature", "feature");
    let scan = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    let record = scan.records.values().next().unwrap().clone();
    assert_eq!(record.item.state, WorktreeState::Finished);

    let reclaimed = remove_with(&fixture, &record, 30, OpenFileState::Idle).unwrap();
    assert_eq!(reclaimed, record.item.allocated_bytes);
    assert!(!path.exists());
    // The branch, its commit and the main checkout stay.
    git(
        &fixture.repository,
        &["rev-parse", "--verify", "-q", "feature"],
    );
    assert!(fixture.repository.join("main.rs").is_file());
    assert!(fixture.home.join("code").is_dir());
    let listed = list_worktrees(&fixture.environment, &fixture.repository).unwrap();
    assert_eq!(listed.len(), 1);
}

#[test]
fn removal_rederives_every_fact_before_git_runs() {
    let fixture = fixture();
    let path = fixture.worktree("app-feature", "feature");
    let scan = scan_with(&fixture, rules(true), Some(30), OpenFileState::Idle, vec![]);
    let record = scan.records.values().next().unwrap().clone();

    // New work after the review.
    fs::write(path.join("notes.md"), "written after the review\n").unwrap();
    let refused = remove_with(&fixture, &record, 30, OpenFileState::Idle).unwrap_err();
    assert!(refused.contains("uncommitted or untracked"), "{refused}");
    assert!(path.join("notes.md").is_file());
    fs::remove_file(path.join("notes.md")).unwrap();

    // In use at the last moment.
    let refused = remove_with(&fixture, &record, 30, OpenFileState::InUse).unwrap_err();
    assert!(refused.contains("files open"), "{refused}");
    assert!(path.join("target/debug/app").is_file());

    // Replaced by another folder with the same name.
    let moved = fixture.code.join("app-moved");
    fs::rename(&path, &moved).unwrap();
    fs::create_dir_all(&path).unwrap();
    let refused = remove_with(&fixture, &record, 30, OpenFileState::Idle).unwrap_err();
    assert!(refused.contains("replaced"), "{refused}");
    assert!(path.is_dir());
    assert!(moved.join("target/debug/app").is_file());
}

#[test]
fn repositories_are_main_checkouts_in_saved_folders_only() {
    let fixture = fixture();
    fixture.worktree("app-feature", "feature");
    let nested = fixture.code.join("group/lib");
    fs::create_dir_all(&nested).unwrap();
    git(&nested, &["init", "-q"]);
    let (found, refused) = repositories(&fixture.environment, &fixture.folders());
    assert!(refused.is_empty());
    let mut found = found;
    found.sort();
    assert_eq!(found, vec![fixture.repository.clone(), nested]);
}

#[test]
fn agent_folders_are_named() {
    let fixture = fixture();
    let codex = fixture.home.join(".codex/worktrees/abc/app");
    assert_eq!(
        agent_of(&fixture.environment, &codex),
        Some(WorktreeAgent::Codex)
    );
    let claude = fixture.code.join("app/.claude/worktrees/task");
    assert_eq!(
        agent_of(&fixture.environment, &claude),
        Some(WorktreeAgent::ClaudeCode)
    );
    assert_eq!(
        agent_of(&fixture.environment, &fixture.code.join("app-wt")),
        None
    );
}
