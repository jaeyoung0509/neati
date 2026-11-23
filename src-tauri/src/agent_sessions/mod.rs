//! Agent sessions as a bounded context (#508, #509).
//!
//! Agent sessions are the one part of AI-assisted work that cannot be
//! regenerated. Claude Code and Codex keep each one as JSONL under the home
//! folder. This module reads them without changing them and carries them in
//! a bundle:
//!
//! - [`inventory`] lists sessions from a bounded head and tail read of each
//!   file. It never parses a whole transcript, never follows a link and never
//!   writes.
//! - [`bundle`] exports chosen sessions byte for byte with a checksummed
//!   manifest, and imports a verified bundle without overwriting any file.
//! - [`secrets`] counts likely secret shapes. A value is never kept.
//! - [`remap`] rewrites `cwd` fields when a project moved.
//!
//! Nothing here deletes a session file or reaches the network. The home folder
//! comes from the injected `PlatformEnvironment`.

pub mod bundle;
pub mod inventory;
pub mod picker;
pub mod records;
pub mod remap;
pub mod secrets;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use neati_platform::PlatformEnvironment;

use crate::models::AgentSessionTool;

/// Where each tool keeps its sessions on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRoots {
    /// `~/.claude/projects`
    pub claude_projects: PathBuf,
    /// `~/.codex`; sessions live in its `sessions` and `archived_sessions`.
    pub codex_home: PathBuf,
}

impl SessionRoots {
    pub fn from_home(home: &Path) -> Self {
        Self {
            claude_projects: home.join(".claude").join("projects"),
            codex_home: home.join(".codex"),
        }
    }

    /// The stated home folder's roots, when the environment states one.
    pub fn from_environment(environment: &PlatformEnvironment) -> Option<Self> {
        environment.user_home().map(|home| Self::from_home(&home))
    }

    /// The folder a session's relative paths are relative to.
    pub fn tool_root(&self, tool: AgentSessionTool) -> &Path {
        match tool {
            AgentSessionTool::Claude => &self.claude_projects,
            AgentSessionTool::Codex => &self.codex_home,
        }
    }
}

/// A stop request shared by one running session operation and whoever may
/// cancel it.
#[derive(Debug, Clone, Default)]
pub struct SessionCancel(Arc<AtomicBool>);

impl SessionCancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Whether `other` is this same signal.
    pub fn is_same(&self, other: &SessionCancel) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Opens a regular file without following a link at its final component.
pub fn open_no_follow(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other("the file is a link"));
        }
        File::open(path)?
    };
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// `a/b/c` from path components, the way bundles and keys spell a relative
/// path on every platform.
pub fn relative_slash_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_str()?.to_string()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Whether neati may check that a recorded folder exists. Checking a network
/// automount location would reach the network, so those stay unknown.
pub fn folder_may_be_checked(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        && !["/net", "/Network", "/afs"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
}
