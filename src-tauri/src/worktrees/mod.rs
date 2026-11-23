//! Finished Git worktrees (#505, owner direction 2026-11-24).
//!
//! Coding agents and people create a linked worktree per task. After the task
//! lands, the checkout stays with its own build output and dependencies. This
//! module reports the linked worktrees of repositories in the saved project
//! folders and decides which are *finished*:
//!
//! - registered with Git, present, not locked and without submodules;
//! - `git status` shows nothing but ignored files, so every tracked file is
//!   committed and nothing untracked would be lost;
//! - its commit stays reachable after removal: a checked-out branch survives
//!   the removal, and a detached commit must be contained in another ref;
//! - no running process works inside it, it has been left alone for the
//!   person's idle period, and an exact open-file check proves it idle.
//!
//! Removal goes through Git only (`git worktree remove`, never `--force`), so
//! Git's own clean check is a second guard, and every fact is re-derived
//! immediately before it runs ([`removal`]). The branch, its commits and the
//! repository are never touched. Discovery never implies permission: every
//! worktree is reported with its state, and only `Finished` is removable.

pub mod removal;
#[cfg(test)]
mod tests;

use crate::developer_artifacts::preselection::{ActivityProbe, UseProbe, WorkingDirectories};
use crate::models::{
    CleanupRules, ReviewedFileIdentity, WorktreeAgent, WorktreeInventory, WorktreeItem,
    WorktreeState,
};
use crate::safety::{Blacklist, SymlinkGuard};
use neati_platform::open_files::OpenFileState;
use neati_platform::path_algebra;
use neati_platform::PlatformEnvironment;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Repositories read per scan.
pub const MAX_REPOSITORIES: usize = 64;
/// Linked worktrees reported per scan.
pub const MAX_WORKTREES: usize = 256;
/// Direct children read per folder while looking for repositories.
const CHILD_LIMIT: usize = 256;
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
const STATUS_TIMEOUT: Duration = Duration::from_secs(60);
const REF_TIMEOUT: Duration = Duration::from_secs(10);
const DAY_SECONDS: u64 = 24 * 60 * 60;
/// How long a published inventory may decide one-button Clean.
pub const INVENTORY_TTL_SECS: u64 = 15 * 60;

/// One entry of `git worktree list --porcelain -z`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListedWorktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// The full ref, e.g. `refs/heads/main`; `None` when detached.
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub bare: bool,
}

/// Parses `git worktree list --porcelain -z`. The first entry is the main
/// worktree; attributes are NUL-terminated and an empty field ends an entry.
pub fn parse_porcelain(output: &[u8]) -> Vec<ListedWorktree> {
    let mut entries = Vec::new();
    let mut current: Option<ListedWorktree> = None;
    for field in output.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }
        let text = String::from_utf8_lossy(field);
        let (key, value) = match text.split_once(' ') {
            Some((key, value)) => (key, Some(value)),
            None => (text.as_ref(), None),
        };
        match key {
            "worktree" => {
                if let Some(entry) = current.take() {
                    entries.push(entry);
                }
                current = value.map(|path| ListedWorktree {
                    path: PathBuf::from(path),
                    ..ListedWorktree::default()
                });
            }
            "HEAD" => {
                if let Some(entry) = current.as_mut() {
                    entry.head = value.map(str::to_string);
                }
            }
            "branch" => {
                if let Some(entry) = current.as_mut() {
                    entry.branch = value.map(str::to_string);
                }
            }
            "locked" => {
                if let Some(entry) = current.as_mut() {
                    entry.locked = true;
                }
            }
            "prunable" => {
                if let Some(entry) = current.as_mut() {
                    entry.prunable = true;
                }
            }
            "bare" => {
                if let Some(entry) = current.as_mut() {
                    entry.bare = true;
                }
            }
            _ => {}
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    entries
}

/// Runs a fixed `git` subcommand through the hardened constructor, with
/// optional locks off so an inspection never rewrites the index.
pub(crate) fn run_git(
    environment: &PlatformEnvironment,
    root: &Path,
    args: &[&std::ffi::OsStr],
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut command =
        crate::git::git_command(root, environment).map_err(|refusal| refusal.message())?;
    command.env("GIT_OPTIONAL_LOCKS", "0").args(args);
    let output = neati_platform::subprocess::run_with_timeout(command, timeout)
        .map_err(|error| crate::privacy::paths::mask_paths_in_text(&error.to_string()))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let first = message.lines().next().unwrap_or("git failed").trim();
        return Err(crate::privacy::paths::mask_paths_in_text(first));
    }
    Ok(output.stdout)
}

fn os(text: &str) -> &std::ffi::OsStr {
    std::ffi::OsStr::new(text)
}

/// The worktrees Git has registered for `repository`, main worktree first.
pub fn list_worktrees(
    environment: &PlatformEnvironment,
    repository: &Path,
) -> Result<Vec<ListedWorktree>, String> {
    let output = run_git(
        environment,
        repository,
        &[os("worktree"), os("list"), os("--porcelain"), os("-z")],
        LIST_TIMEOUT,
    )?;
    Ok(parse_porcelain(&output))
}

/// Main checkouts (a `.git` directory, not a pointer file) in the saved
/// project folders: each folder, its children and their children. Bounded,
/// metadata only, never through a link or into a hidden folder.
pub fn repositories(
    environment: &PlatformEnvironment,
    folders: &[String],
) -> (Vec<PathBuf>, Vec<String>) {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut refused = Vec::new();
    for folder in folders {
        let root = match crate::developer_artifacts::validate_workspace_root(
            environment,
            Path::new(folder),
        ) {
            Ok(root) => root,
            Err(reason) => {
                refused.push(reason);
                continue;
            }
        };
        let mut level = vec![root];
        for depth in 0..3 {
            let mut next = Vec::new();
            for directory in &level {
                if found.len() >= MAX_REPOSITORIES {
                    break;
                }
                if is_main_checkout(directory) {
                    if seen.insert(directory.clone()) {
                        found.push(directory.clone());
                    }
                    // A repository's own folders are not searched further.
                    continue;
                }
                if depth == 2 {
                    continue;
                }
                let Ok(entries) = fs::read_dir(directory) else {
                    continue;
                };
                for entry in entries.flatten().take(CHILD_LIMIT) {
                    let path = entry.path();
                    let hidden = entry.file_name().to_string_lossy().starts_with('.');
                    let Ok(metadata) = fs::symlink_metadata(&path) else {
                        continue;
                    };
                    if !hidden && metadata.is_dir() && !SymlinkGuard::is_symlink(&path) {
                        next.push(path);
                    }
                }
            }
            level = next;
        }
        if found.len() >= MAX_REPOSITORIES {
            found.truncate(MAX_REPOSITORIES);
            break;
        }
    }
    (found, refused)
}

fn is_main_checkout(directory: &Path) -> bool {
    fs::symlink_metadata(directory.join(".git"))
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

/// A stable, opaque name for a worktree location.
pub fn worktree_id(path: &Path) -> String {
    let digest = crate::hash::sha256_hex(path.to_string_lossy().as_bytes());
    format!("worktree:{}", &digest[..12])
}

fn agent_of(environment: &PlatformEnvironment, path: &Path) -> Option<WorktreeAgent> {
    let text = path.to_string_lossy();
    if let Some(home) = environment.user_home() {
        let codex = home.join(".codex/worktrees");
        if path_algebra::contains(
            &codex.to_string_lossy(),
            &text,
            path_algebra::PathFlavor::current(),
        ) {
            return Some(WorktreeAgent::Codex);
        }
    }
    if path
        .components()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0].as_os_str() == ".claude" && pair[1].as_os_str() == "worktrees")
    {
        return Some(WorktreeAgent::ClaudeCode);
    }
    None
}

/// What a scan or a removal reads besides the repository: the clock, the
/// person's rules, and how activity and use are observed.
pub struct Observation<'a> {
    pub now: u64,
    pub rules: CleanupRules,
    pub use_probe: &'a UseProbe<'a>,
    pub activity: &'a ActivityProbe<'a>,
    pub working_directories: &'a WorkingDirectories<'a>,
}

/// A worktree with what removal needs to revalidate it.
#[derive(Debug, Clone)]
pub struct WorktreeRecord {
    pub item: WorktreeItem,
    pub repository: PathBuf,
    pub path: PathBuf,
    pub identity: Option<ReviewedFileIdentity>,
}

/// A scan's private result: the projection the interface sees plus the
/// records removal revalidates.
#[derive(Debug, Clone)]
pub struct WorktreeScan {
    pub inventory: WorktreeInventory,
    pub records: HashMap<String, WorktreeRecord>,
    pub created_at: u64,
    pub rules: CleanupRules,
}

impl WorktreeScan {
    /// Whether one-button Clean may act on this scan now: it is recent and
    /// was decided with the rules in force.
    pub fn is_current_for(&self, rules: &CleanupRules, now: u64) -> bool {
        now.saturating_sub(self.created_at) <= INVENTORY_TTL_SECS && self.rules == *rules
    }
}

/// Every fact except current use. `Ok` means the worktree is a candidate for
/// the exact open-file check; `Err` carries its state and an optional reason.
pub(crate) fn pre_use_verdict(
    environment: &PlatformEnvironment,
    repository: &Path,
    listed: &ListedWorktree,
    observation: &Observation<'_>,
    working: &mut Option<Vec<PathBuf>>,
    last_activity: &mut Option<u64>,
) -> Result<(), (WorktreeState, Option<String>)> {
    let path = &listed.path;
    let metadata = fs::symlink_metadata(path);
    if listed.prunable || !metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
        return Err((WorktreeState::Missing, None));
    }
    let home = environment.user_home();
    let inside_home = home.as_ref().is_some_and(|home| {
        path != home
            && path_algebra::contains(
                &home.to_string_lossy(),
                &path.to_string_lossy(),
                path_algebra::PathFlavor::current(),
            )
    });
    if SymlinkGuard::is_symlink(path)
        || !inside_home
        || Blacklist::is_blacklisted_with(path, environment)
    {
        return Err((
            WorktreeState::Refused,
            Some("It is outside the home folder or in a protected location.".into()),
        ));
    }
    if listed.locked {
        return Err((WorktreeState::Locked, None));
    }
    if fs::symlink_metadata(path.join(".gitmodules")).is_ok() {
        return Err((WorktreeState::HasSubmodules, None));
    }
    let status = run_git(
        environment,
        path,
        &[
            os("status"),
            os("--porcelain=v1"),
            os("--untracked-files=all"),
            os("--ignore-submodules=none"),
            os("-z"),
        ],
        STATUS_TIMEOUT,
    )
    .map_err(|reason| (WorktreeState::Refused, Some(reason)))?;
    if !status.is_empty() {
        return Err((WorktreeState::HasChanges, None));
    }
    if listed.branch.is_none() {
        let head = listed
            .head
            .as_deref()
            .filter(|head| head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or((
                WorktreeState::Refused,
                Some("Git did not report a commit for it.".to_string()),
            ))?;
        let containing = run_git(
            environment,
            repository,
            &[
                os("for-each-ref"),
                os("--count=1"),
                os("--format=%(refname)"),
                os("--contains"),
                os(head),
            ],
            REF_TIMEOUT,
        )
        .map_err(|reason| (WorktreeState::Refused, Some(reason)))?;
        if containing.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Err((WorktreeState::CommitsOnlyHere, None));
        }
    }
    let working = working.get_or_insert_with(|| (observation.working_directories)());
    if crate::developer_artifacts::has_working_process(path, working) {
        return Err((WorktreeState::RecentlyActive, None));
    }
    *last_activity = (observation.activity)(path);
    let Some(last) = *last_activity else {
        return Err((WorktreeState::ActivityUnknown, None));
    };
    let idle_for = observation.now.saturating_sub(last);
    if idle_for < u64::from(observation.rules.worktrees.idle_days) * DAY_SECONDS {
        return Err((WorktreeState::RecentlyActive, None));
    }
    Ok(())
}

fn base_item(
    environment: &PlatformEnvironment,
    repository: &Path,
    listed: &ListedWorktree,
) -> WorktreeItem {
    WorktreeItem {
        id: worktree_id(&listed.path),
        repository: repository.to_string_lossy().into_owned(),
        path: listed.path.to_string_lossy().into_owned(),
        name: listed
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        branch: listed.branch.as_deref().map(|branch| {
            branch
                .strip_prefix("refs/heads/")
                .unwrap_or(branch)
                .to_string()
        }),
        head: listed
            .head
            .as_deref()
            .map(|head| head.chars().take(7).collect())
            .unwrap_or_default(),
        agent: agent_of(environment, &listed.path),
        allocated_bytes: 0,
        file_count: 0,
        measurement_complete: false,
        last_activity: None,
        state: WorktreeState::Refused,
        reason: None,
        selected_by_default: false,
    }
}

/// Reports the linked worktrees of the repositories in `folders`.
pub fn scan(
    environment: &PlatformEnvironment,
    folders: &[String],
    observation: &Observation<'_>,
    is_cancelled: &dyn Fn() -> bool,
) -> WorktreeScan {
    let (repositories, mut refused) = repositories(environment, folders);
    let mut records = Vec::new();
    let mut working = None;
    let mut candidates = Vec::new();
    'repositories: for repository in &repositories {
        if is_cancelled() {
            break;
        }
        let listed = match list_worktrees(environment, repository) {
            Ok(listed) => listed,
            Err(reason) => {
                refused.push(reason);
                continue;
            }
        };
        // The first entry is the main worktree, which is never a unit.
        for entry in listed.iter().skip(1).filter(|entry| !entry.bare) {
            if records.len() >= MAX_WORKTREES || is_cancelled() {
                break 'repositories;
            }
            let mut item = base_item(environment, repository, entry);
            let mut last_activity = None;
            let verdict = pre_use_verdict(
                environment,
                repository,
                entry,
                observation,
                &mut working,
                &mut last_activity,
            );
            item.last_activity = last_activity;
            match verdict {
                Ok(()) => candidates.push(records.len()),
                Err((state, reason)) => {
                    item.state = state;
                    item.reason = reason;
                }
            }
            if item.state != WorktreeState::Missing {
                let measurement = crate::scanner::size::SizeCalculator::measure_path_full(
                    &entry.path,
                    &[],
                    environment,
                );
                item.allocated_bytes = measurement
                    .size
                    .allocated
                    .unwrap_or(measurement.size.logical);
                item.file_count = measurement.file_count as u64;
                item.measurement_complete = measurement.complete;
            }
            records.push(WorktreeRecord {
                identity: crate::large_files::identity_from_path(&entry.path),
                item,
                repository: repository.clone(),
                path: entry.path.clone(),
            });
        }
    }

    if !candidates.is_empty() && !is_cancelled() {
        let paths: Vec<PathBuf> = candidates
            .iter()
            .map(|index| records[*index].path.clone())
            .collect();
        let states = (observation.use_probe)(&paths, is_cancelled);
        for (position, index) in candidates.iter().enumerate() {
            let state = if states.len() == candidates.len() && !is_cancelled() {
                states[position]
            } else {
                OpenFileState::Unknown
            };
            records[*index].item.state = match state {
                OpenFileState::Idle => WorktreeState::Finished,
                OpenFileState::InUse => WorktreeState::InUse,
                OpenFileState::Unknown => WorktreeState::UseUnknown,
            };
        }
    } else {
        for index in &candidates {
            records[*index].item.state = WorktreeState::UseUnknown;
        }
    }
    for record in &mut records {
        record.item.selected_by_default =
            observation.rules.worktrees.enabled && record.item.state == WorktreeState::Finished;
    }

    let scan_id = uuid::Uuid::new_v4().to_string();
    let mut items: Vec<WorktreeItem> = records.iter().map(|record| record.item.clone()).collect();
    items.sort_by(|left, right| {
        right
            .allocated_bytes
            .cmp(&left.allocated_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    WorktreeScan {
        inventory: WorktreeInventory {
            scan_id,
            repositories: repositories.len() as u32,
            items,
            refused,
            idle_days: observation.rules.worktrees.idle_days,
            rule_enabled: observation.rules.worktrees.enabled,
        },
        records: records
            .into_iter()
            .map(|record| (record.item.id.clone(), record))
            .collect(),
        created_at: observation.now,
        rules: observation.rules,
    }
}
