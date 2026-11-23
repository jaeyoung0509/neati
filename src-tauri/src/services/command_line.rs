//! The `neati` command and the agent skill, for both interfaces (#500).
//!
//! The skill text is compiled in from `skills/neati/SKILL.md`, so an installed
//! copy always matches the command it describes. Installing writes one file
//! and replaces only a copy neati wrote itself; it never writes through a
//! link. The window reports where `neati` resolves and offers a link command
//! for a person to run: it never edits a PATH folder itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use neati_platform::PlatformEnvironment;

use crate::models::{
    AgentSkillInstall, AgentSkillState, AgentSkillStatus, AgentSkillTarget, CommandLineStatus,
};

pub const SKILL: &str = include_str!("../../../skills/neati/SKILL.md");

/// The line that marks a file as neati's own copy.
const MARKER: &str = "<!-- neati-skill:";

/// PATH folders where a link is conventional, in the order a link is offered.
const LINK_FOLDERS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

pub struct Written {
    pub path: PathBuf,
    pub replaced: bool,
}

/// `<root>/neati/SKILL.md`.
pub fn skill_path(root: &Path) -> PathBuf {
    root.join("neati").join("SKILL.md")
}

/// What is at `<root>/neati/SKILL.md` now.
pub fn skill_state(root: &Path) -> AgentSkillState {
    match std::fs::read_to_string(skill_path(root)) {
        Ok(existing) if existing == SKILL => AgentSkillState::Current,
        Ok(existing) if existing.contains(MARKER) => AgentSkillState::Outdated,
        Ok(_) => AgentSkillState::Foreign,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AgentSkillState::Missing,
        // Unreadable is not ours to overwrite.
        Err(_) => AgentSkillState::Foreign,
    }
}

/// Writes `<root>/neati/SKILL.md`. The skill folder and the file must not be
/// links, and an existing file must be neati's own copy unless `force`.
pub fn write_skill(root: &Path, force: bool) -> Result<Written, String> {
    let folder = root.join("neati");
    let path = skill_path(root);
    for candidate in [&folder, &path] {
        if let Ok(metadata) = std::fs::symlink_metadata(candidate) {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "{} is a link; neati does not write through links.",
                    candidate.display()
                ));
            }
        }
    }
    let replaced = match std::fs::read_to_string(&path) {
        Ok(existing) => {
            if !force && !existing.contains(MARKER) {
                return Err(format!(
                    "{} already exists and was not written by neati. Pass --force to replace it.",
                    path.display()
                ));
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
    };
    neati_platform::file_ops::atomic_write(&path, SKILL.as_bytes())
        .map_err(|error| format!("{} could not be written: {error}", path.display()))?;
    Ok(Written { path, replaced })
}

pub struct CommandLineService {
    environment: Arc<PlatformEnvironment>,
    /// The running executable, observed once at the composition root.
    executable: Option<PathBuf>,
}

impl CommandLineService {
    pub fn new(environment: Arc<PlatformEnvironment>, executable: Option<PathBuf>) -> Self {
        Self {
            environment,
            executable,
        }
    }

    pub fn status(&self) -> CommandLineStatus {
        let executable = self
            .executable
            .as_ref()
            .and_then(|path| std::fs::canonicalize(path).ok());
        let command_path = self
            .environment
            .path_entries()
            .iter()
            .map(|folder| folder.join("neati"))
            .find(|candidate| candidate.is_file());
        let linked_here = match (&command_path, &executable) {
            (Some(command), Some(executable)) => {
                std::fs::canonicalize(command).ok().as_ref() == Some(executable)
            }
            _ => false,
        };
        let link_command = executable.as_ref().and_then(|executable| {
            let folder = LINK_FOLDERS.iter().map(Path::new).find(|folder| {
                self.environment
                    .path_entries()
                    .iter()
                    .any(|entry| entry == folder)
            })?;
            Some(format!(
                "ln -sf \"{}\" {}",
                executable.display(),
                folder.join("neati").display()
            ))
        });
        let home = self.environment.user_home();
        let skills = home
            .map(|home| {
                AgentSkillTarget::ALL
                    .into_iter()
                    .map(|target| {
                        let root = home.join(target.relative_root());
                        AgentSkillStatus {
                            target,
                            path: skill_path(&root).to_string_lossy().into_owned(),
                            state: skill_state(&root),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        CommandLineStatus {
            executable: executable.map(|path| path.to_string_lossy().into_owned()),
            command_path: command_path.map(|path| path.to_string_lossy().into_owned()),
            linked_here,
            link_command,
            skills,
        }
    }

    /// Installs the skill for `target`; never replaces a file neati did not
    /// write (the window has no force option).
    pub fn install_skill(&self, target: AgentSkillTarget) -> Result<AgentSkillInstall, String> {
        let home = self
            .environment
            .user_home()
            .ok_or_else(|| "The home folder is unknown.".to_string())?;
        let written = write_skill(&home.join(target.relative_root()), false)?;
        Ok(AgentSkillInstall {
            target,
            path: written.path.to_string_lossy().into_owned(),
            replaced: written.replaced,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neati_platform::path_algebra::PathFlavor;

    #[test]
    fn the_bundled_skill_is_a_valid_agent_skill() {
        assert!(SKILL.starts_with("---\nname: neati\ndescription: "));
        assert!(SKILL.contains(MARKER));
        // The skill teaches the preview-first workflow the CLI enforces.
        assert!(SKILL.contains("neati clean --json"));
        assert!(SKILL.contains("--yes"));
        assert!(SKILL.contains("permanently"));
    }

    #[test]
    fn install_writes_and_then_updates_its_own_copy() {
        let root = tempfile::tempdir().expect("temporary skills root");
        assert_eq!(skill_state(root.path()), AgentSkillState::Missing);
        let first = write_skill(root.path(), false).expect("fresh install");
        assert!(!first.replaced);
        assert_eq!(skill_state(root.path()), AgentSkillState::Current);
        std::fs::write(&first.path, format!("{MARKER} older -->")).expect("older copy");
        assert_eq!(skill_state(root.path()), AgentSkillState::Outdated);
        assert!(
            write_skill(root.path(), false)
                .expect("update own copy")
                .replaced
        );
    }

    #[test]
    fn install_never_replaces_someone_elses_skill_without_force() {
        let root = tempfile::tempdir().expect("temporary skills root");
        let path = skill_path(root.path());
        std::fs::create_dir_all(path.parent().expect("parent")).expect("skill folder");
        std::fs::write(&path, "my own notes").expect("foreign skill");
        assert_eq!(skill_state(root.path()), AgentSkillState::Foreign);
        assert!(write_skill(root.path(), false).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("kept"),
            "my own notes"
        );
        assert!(write_skill(root.path(), true).expect("forced").replaced);
    }

    #[cfg(unix)]
    #[test]
    fn install_never_writes_through_a_link() {
        let root = tempfile::tempdir().expect("temporary skills root");
        let elsewhere = tempfile::tempdir().expect("link target");
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("neati")).expect("link");
        assert!(write_skill(root.path(), true).is_err());
        assert!(!elsewhere.path().join("SKILL.md").exists());
    }

    #[test]
    fn status_finds_the_linked_command_and_each_skill() {
        let home = tempfile::tempdir().expect("home");
        let home_path = std::fs::canonicalize(home.path()).expect("canonical home");
        let bin = home_path.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let executable = home_path.join("Neati");
        std::fs::write(&executable, "binary").expect("executable");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&executable, bin.join("neati")).expect("link");
        write_skill(&home_path.join(".claude/skills"), false).expect("claude skill");

        let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home(&home_path)
            .with_path_entry(&bin);
        let status = CommandLineService::new(Arc::new(environment), Some(executable)).status();
        #[cfg(unix)]
        assert!(status.linked_here);
        assert_eq!(
            status.link_command, None,
            "no conventional PATH folder stated"
        );
        let states: Vec<_> = status
            .skills
            .iter()
            .map(|skill| (skill.target, skill.state))
            .collect();
        assert_eq!(
            states,
            [
                (AgentSkillTarget::Claude, AgentSkillState::Current),
                (AgentSkillTarget::Codex, AgentSkillState::Missing),
            ]
        );
    }

    #[test]
    fn the_link_command_names_a_conventional_path_folder() {
        let executable = tempfile::NamedTempFile::new().expect("executable");
        let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home("/Users/me")
            .with_path_entry("/opt/homebrew/bin");
        let status =
            CommandLineService::new(Arc::new(environment), Some(executable.path().to_path_buf()))
                .status();
        let command = status.link_command.expect("a link command");
        assert!(command.starts_with("ln -sf \""));
        assert!(command.ends_with(" /opt/homebrew/bin/neati"));
    }
}
