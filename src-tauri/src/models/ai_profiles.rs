use serde::{Deserialize, Serialize};

/// A CLI tool whose configuration lives in one home dot-directory and can be
/// duplicated per account (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, and equivalents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AiProfileTool {
    Claude,
    Codex,
    Gemini,
    Opencode,
    Cursor,
}

impl AiProfileTool {
    pub const ALL: [Self; 5] = [
        Self::Claude,
        Self::Codex,
        Self::Gemini,
        Self::Opencode,
        Self::Cursor,
    ];

    /// The default profile directory name under the home folder.
    pub fn default_dir_name(self) -> &'static str {
        match self {
            Self::Claude => ".claude",
            Self::Codex => ".codex",
            Self::Gemini => ".gemini",
            Self::Opencode => ".opencode",
            Self::Cursor => ".cursor",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini / Antigravity",
            Self::Opencode => "OpenCode",
            Self::Cursor => "Cursor",
        }
    }
}

/// An extra profile directory the person confirmed for one tool.
///
/// It widens what neati *measures*, never what it may remove: only
/// observation-only catalog entries are projected onto it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct AiToolProfile {
    pub tool: AiProfileTool,
    /// Absolute path of the profile directory.
    pub path: String,
}

/// A directory that looks like another profile. A suggestion is only a hint
/// for Settings; it is measured only after the person adds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AiToolProfileSuggestion {
    pub tool: AiProfileTool,
    pub path: String,
}
