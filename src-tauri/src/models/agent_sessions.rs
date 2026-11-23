//! Agent session records as the window and the CLI see them (#508).
//!
//! Only derived facts cross IPC: identity, the project folder, dates, sizes,
//! and a capped title from the first prompt. No other transcript text and no
//! session file path beyond the project folder.

use serde::{Deserialize, Serialize};

/// The tools whose session files neati reads.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionTool {
    /// Claude Code: `~/.claude/projects/<folder>/<session>.jsonl`.
    Claude,
    /// Codex: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` and
    /// `~/.codex/archived_sessions`.
    Codex,
}

impl AgentSessionTool {
    pub const ALL: [AgentSessionTool; 2] = [AgentSessionTool::Claude, AgentSessionTool::Codex];

    /// The folder a bundle keeps this tool's files under, and the CLI name.
    pub fn slug(self) -> &'static str {
        match self {
            AgentSessionTool::Claude => "claude",
            AgentSessionTool::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgentSessionTool::Claude => "Claude Code",
            AgentSessionTool::Codex => "Codex",
        }
    }
}

/// Whether a session's project folder is on this Mac.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProjectFolderState {
    Present,
    /// The recorded folder is gone: the session is orphaned.
    Missing,
    /// No folder was recorded, or it is not checked (a network location).
    Unknown,
}

/// Why a session's details are incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionIssue {
    /// The file could not be opened or read.
    Unreadable,
    /// The first bytes held no record neati recognises.
    NoRecords,
    /// The read budget ran out; dates come from the file.
    NotRead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentSessionSummary {
    /// Opaque, stable reference for selection and `--only`.
    pub key: String,
    pub tool: AgentSessionTool,
    pub session_id: String,
    /// The working directory the session recorded first.
    pub cwd: Option<String>,
    /// The first prompt, capped. Shown locally only and never logged.
    pub title: Option<String>,
    /// Unix seconds.
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub started_at: Option<u64>,
    /// Unix seconds.
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub last_activity_at: Option<u64>,
    /// The dates come from the session's records rather than the file's.
    pub dates_from_records: bool,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub size_bytes: u64,
    pub file_count: u32,
    /// Codex keeps archived sessions apart.
    pub archived: bool,
    pub issue: Option<AgentSessionIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentSessionProject {
    /// Opaque group reference.
    pub key: String,
    pub cwd: Option<String>,
    pub display_name: String,
    pub folder: ProjectFolderState,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub total_bytes: u64,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub last_activity_at: Option<u64>,
    pub sessions: Vec<AgentSessionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentSessionToolTotal {
    pub tool: AgentSessionTool,
    /// The tool's session folder exists on this Mac.
    pub present: bool,
    pub sessions: u32,
    pub files: u32,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub bytes: u64,
}

/// The read-only inventory (#508).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentSessionInventory {
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub observed_at: u64,
    pub projects: Vec<AgentSessionProject>,
    pub tools: Vec<AgentSessionToolTotal>,
    /// A file cap or the read budget was reached.
    pub truncated: bool,
    pub cancelled: bool,
    pub incomplete_reasons: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(size_bytes: u64) -> AgentSessionSummary {
        AgentSessionSummary {
            key: "k".into(),
            tool: AgentSessionTool::Codex,
            session_id: "s".into(),
            cwd: None,
            title: None,
            started_at: Some(crate::ipc_numeric::MAX_SAFE_INTEGER),
            last_activity_at: None,
            dates_from_records: false,
            size_bytes,
            file_count: 1,
            archived: false,
            issue: None,
        }
    }

    #[test]
    fn session_integers_beyond_javascript_precision_are_refused() {
        assert!(serde_json::to_string(&summary(crate::ipc_numeric::MAX_SAFE_INTEGER + 1)).is_err());
        let mut late = summary(1);
        late.last_activity_at = Some(crate::ipc_numeric::MAX_SAFE_INTEGER + 1);
        assert!(serde_json::to_string(&late).is_err());
        let json = serde_json::to_value(summary(crate::ipc_numeric::MAX_SAFE_INTEGER))
            .expect("safe integers serialize");
        assert_eq!(json["size_bytes"], crate::ipc_numeric::MAX_SAFE_INTEGER);
        assert_eq!(json["tool"], "codex");
    }

    #[test]
    fn every_byte_total_is_guarded() {
        let too_big = crate::ipc_numeric::MAX_SAFE_INTEGER + 1;
        let tool = AgentSessionToolTotal {
            tool: AgentSessionTool::Claude,
            present: true,
            sessions: 1,
            files: 1,
            bytes: too_big,
        };
        assert!(serde_json::to_string(&tool).is_err());
        let project = AgentSessionProject {
            key: "p".into(),
            cwd: None,
            display_name: "p".into(),
            folder: ProjectFolderState::Unknown,
            total_bytes: too_big,
            last_activity_at: None,
            sessions: vec![],
        };
        assert!(serde_json::to_string(&project).is_err());
        let inventory = AgentSessionInventory {
            observed_at: too_big,
            projects: vec![],
            tools: vec![],
            truncated: false,
            cancelled: false,
            incomplete_reasons: vec![],
        };
        assert!(serde_json::to_string(&inventory).is_err());
    }
}
