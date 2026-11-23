//! The command line and agent skill as the window reports them (#500).

use serde::{Deserialize, Serialize};

/// Where an agent loads skills from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentSkillTarget {
    /// `~/.claude/skills`
    Claude,
    /// `~/.codex/skills`
    Codex,
}

impl AgentSkillTarget {
    pub const ALL: [AgentSkillTarget; 2] = [AgentSkillTarget::Claude, AgentSkillTarget::Codex];

    /// The skills folder under the home folder.
    pub fn relative_root(self) -> &'static str {
        match self {
            AgentSkillTarget::Claude => ".claude/skills",
            AgentSkillTarget::Codex => ".codex/skills",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentSkillState {
    /// No skill file there.
    Missing,
    /// neati's own copy, matching this version.
    Current,
    /// neati's own copy from another version; installing replaces it.
    Outdated,
    /// A file neati did not write; installing never replaces it.
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentSkillStatus {
    pub target: AgentSkillTarget,
    pub path: String,
    pub state: AgentSkillState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CommandLineStatus {
    /// This app's executable, which is the `neati` command.
    pub executable: Option<String>,
    /// Where `neati` resolves on the stated PATH, when it does.
    pub command_path: Option<String>,
    /// Whether that `neati` is this app.
    pub linked_here: bool,
    /// A command that links this app as `neati`, for a person to run.
    pub link_command: Option<String>,
    pub skills: Vec<AgentSkillStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentSkillInstall {
    pub target: AgentSkillTarget,
    pub path: String,
    /// An earlier neati copy was replaced.
    pub replaced: bool,
}
