//! Removing a finished worktree: the typed owner adapter (#505).
//!
//! Every fact the scan used is re-derived immediately before removal: the
//! folder is the same entity, Git still lists it as a linked worktree of the
//! same repository, it is not locked, `git status` is still empty, its commit
//! is still reachable, it is still inactive, and an exact open-file check
//! proves it idle. Only then does Git remove it, with fixed arguments and
//! never `--force`, so Git refuses a checkout that became dirty in between.
//! The branch and its commits stay in the repository.

use super::*;

/// Git removes the whole checkout, including ignored build output, which can
/// take a while for a large tree. A run that times out is reported, never
/// retried, because partial removal cannot be rolled back.
const REMOVE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Why a worktree was not removed, in words a person can act on.
fn refusal(state: WorktreeState, reason: Option<String>) -> String {
    match state {
        WorktreeState::Finished => "It is no longer finished.".into(),
        WorktreeState::RecentlyActive => {
            "Skipped because it was worked on recently or a process works inside it.".into()
        }
        WorktreeState::ActivityUnknown => {
            "Skipped because when it was last worked on could not be observed.".into()
        }
        WorktreeState::HasChanges => {
            "Skipped because it now has uncommitted or untracked changes.".into()
        }
        WorktreeState::HasSubmodules => "Skipped because it has submodules.".into(),
        WorktreeState::Locked => "Skipped because it is locked in Git.".into(),
        WorktreeState::CommitsOnlyHere => {
            "Skipped because its commit is not on any branch or tag.".into()
        }
        WorktreeState::InUse => "Skipped because a running process has files open in it.".into(),
        WorktreeState::UseUnknown => {
            "Skipped because neati could not confirm that nothing is using it. Close tools that may use it and try again.".into()
        }
        WorktreeState::Missing => "Skipped because the folder is gone.".into(),
        WorktreeState::Refused => reason.unwrap_or_else(|| "Skipped because it could not be checked.".into()),
    }
}

/// Re-derives every fact and removes the worktree through Git. Returns the
/// bytes the scan measured, which is not a free-space delta.
pub fn remove(
    environment: &PlatformEnvironment,
    record: &WorktreeRecord,
    observation: &Observation<'_>,
) -> Result<u64, String> {
    let identity = crate::large_files::identity_from_path(&record.path)
        .ok_or_else(|| "Skipped because the folder moved or became a link.".to_string())?;
    if !record
        .identity
        .as_ref()
        .is_some_and(|expected| expected.same_entity(&identity))
    {
        return Err("Skipped because the folder was replaced after the review.".into());
    }
    let listed = list_worktrees(environment, &record.repository)?;
    let entry = listed
        .iter()
        .skip(1)
        .find(|entry| !entry.bare && entry.path == record.path)
        .ok_or_else(|| {
            "Skipped because Git no longer lists it as a worktree of this repository.".to_string()
        })?;
    let mut working = None;
    let mut last_activity = None;
    pre_use_verdict(
        environment,
        &record.repository,
        entry,
        observation,
        &mut working,
        &mut last_activity,
    )
    .map_err(|(state, reason)| refusal(state, reason))?;
    let use_state = (observation.use_probe)(std::slice::from_ref(&record.path), &|| false);
    match use_state.first() {
        Some(OpenFileState::Idle) => {}
        Some(OpenFileState::InUse) => return Err(refusal(WorktreeState::InUse, None)),
        _ => return Err(refusal(WorktreeState::UseUnknown, None)),
    }

    run_git_removal(environment, &record.repository, &record.path)?;
    if fs::symlink_metadata(&record.path).is_ok() {
        return Err("Git reported success, but the folder is still there.".into());
    }
    Ok(record.item.allocated_bytes)
}

fn run_git_removal(
    environment: &PlatformEnvironment,
    repository: &Path,
    path: &Path,
) -> Result<(), String> {
    run_git(
        environment,
        repository,
        &[os("worktree"), os("remove"), path.as_os_str()],
        REMOVE_TIMEOUT,
    )
    .map(|_| ())
    .map_err(|reason| format!("Git did not remove it: {reason}"))
}
