//! Finished Git worktrees as a cleanup unit (#505).
//!
//! A linked worktree is a whole checkout. It is reported with its state and
//! removed only through Git (`git worktree remove`, never `--force`) when
//! nothing in it is unique: no uncommitted or untracked change, its commit
//! stays reachable, nothing is using it and it has been left alone long
//! enough. The branch and the repository are never touched.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The coding agent whose worktree folder this is, when the path says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeAgent {
    Codex,
    ClaudeCode,
}

/// Why a worktree can or cannot be removed now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    /// Nothing in it is unique and nothing uses it: removable.
    Finished,
    /// Worked on within the rule's period, or a process works inside it.
    RecentlyActive,
    /// When it was last worked on could not be observed.
    ActivityUnknown,
    /// Modified or untracked files that only exist here.
    HasChanges,
    /// Submodules, which Git does not remove without force.
    HasSubmodules,
    /// Locked with `git worktree lock`.
    Locked,
    /// A detached commit that no branch or tag contains.
    CommitsOnlyHere,
    /// A running process has files open in it.
    InUse,
    /// The open-file check could not prove it idle.
    UseUnknown,
    /// Registered with Git, but the folder is gone.
    Missing,
    /// neati could not inspect it safely; see `reason`.
    Refused,
}

impl WorktreeState {
    pub fn is_removable(self) -> bool {
        self == Self::Finished
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct WorktreeItem {
    /// Opaque, stable for the same location.
    pub id: String,
    /// The repository's main checkout.
    pub repository: String,
    pub path: String,
    pub name: String,
    /// The checked-out branch, without `refs/heads/`; `None` when detached.
    pub branch: Option<String>,
    /// Abbreviated commit.
    pub head: String,
    pub agent: Option<WorktreeAgent>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub file_count: u64,
    /// False when the measurement skipped entries; the bytes are a lower bound.
    pub measurement_complete: bool,
    /// Last work in it, in seconds since the epoch.
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub last_activity: Option<u64>,
    pub state: WorktreeState,
    /// Detail for `Refused`, already free of private paths.
    pub reason: Option<String>,
    /// In one-button Clean: finished, and the person's worktree rule is on.
    pub selected_by_default: bool,
}

impl WorktreeItem {
    /// Whether this worktree may be removed now (only `Finished`).
    pub fn is_removable(&self) -> bool {
        self.state.is_removable()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct WorktreeInventory {
    pub scan_id: String,
    /// Repositories found in the saved project folders.
    pub repositories: u32,
    pub items: Vec<WorktreeItem>,
    /// Repositories neati did not read, with the reason.
    pub refused: Vec<String>,
    /// The idle period that decided `Finished`.
    pub idle_days: u16,
    /// Whether finished worktrees are in one-button Clean.
    pub rule_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct WorktreeRemovalPreview {
    pub plan_id: Uuid,
    pub items: Vec<WorktreeItem>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub total_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub expires_in_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct WorktreeRemovalFailure {
    pub id: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub struct WorktreeRemovalResult {
    pub removed: Vec<String>,
    pub failed: Vec<WorktreeRemovalFailure>,
    /// The measured size of the removed checkouts, not a free-space delta.
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub reclaimed_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(bytes: u64) -> WorktreeItem {
        WorktreeItem {
            id: "worktree:abc".into(),
            repository: "/r".into(),
            path: "/r-wt".into(),
            name: "r-wt".into(),
            branch: None,
            head: "abc1234".into(),
            agent: Some(WorktreeAgent::Codex),
            allocated_bytes: bytes,
            file_count: bytes,
            measurement_complete: true,
            last_activity: Some(bytes),
            state: WorktreeState::Finished,
            reason: None,
            selected_by_default: false,
        }
    }

    #[test]
    fn sizes_cross_ipc_within_the_javascript_safe_range() {
        let max = crate::ipc_numeric::MAX_SAFE_INTEGER;
        let json = serde_json::to_value(item(max)).unwrap();
        assert_eq!(json["allocated_bytes"], serde_json::json!(max));
        assert_eq!(json["last_activity"], serde_json::json!(max));
        assert_eq!(json["state"], "finished");
        assert_eq!(json["agent"], "codex");
        let back: WorktreeItem = serde_json::from_value(json).unwrap();
        assert_eq!(back, item(max));
        assert!(serde_json::to_value(item(max + 1)).is_err());
    }

    #[test]
    fn only_finished_is_removable() {
        assert!(WorktreeState::Finished.is_removable());
        for state in [
            WorktreeState::RecentlyActive,
            WorktreeState::HasChanges,
            WorktreeState::CommitsOnlyHere,
            WorktreeState::Locked,
            WorktreeState::InUse,
            WorktreeState::UseUnknown,
            WorktreeState::Missing,
            WorktreeState::Refused,
        ] {
            assert!(!state.is_removable(), "{state:?}");
        }
    }
}
