//! `neati skill`: the agent skill that ships inside the app.
//!
//! The text is compiled in from `skills/neati/SKILL.md`, so an installed copy
//! always matches the CLI it describes. Install writes one file and replaces
//! only a copy neati wrote itself, unless the person passes `--force`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::args::SkillTarget;
use super::output::{display_path, print_json};

pub const SKILL: &str = include_str!("../../../skills/neati/SKILL.md");

/// The line that marks a file as neati's own copy.
const MARKER: &str = "<!-- neati-skill:";

#[derive(Serialize)]
struct InstallReport {
    path: String,
    replaced: bool,
}

pub fn print() -> i32 {
    print!("{SKILL}");
    0
}

pub fn install(
    home: Option<&Path>,
    target: &SkillTarget,
    force: bool,
    json: bool,
) -> Result<i32, String> {
    let root = skills_root(home, target)?;
    let destination = write_skill(&root, force)?;
    let report = InstallReport {
        path: destination.path.to_string_lossy().into_owned(),
        replaced: destination.replaced,
    };
    if json {
        print_json("skill", report)?;
    } else {
        println!(
            "{} the neati skill at {}",
            if report.replaced {
                "Updated"
            } else {
                "Installed"
            },
            display_path(&report.path, home)
        );
        println!("Start a new agent session to load it.");
    }
    Ok(0)
}

fn skills_root(home: Option<&Path>, target: &SkillTarget) -> Result<PathBuf, String> {
    let home = || home.ok_or_else(|| "The home folder is unknown; pass --dir PATH.".to_string());
    Ok(match target {
        SkillTarget::Claude => home()?.join(".claude/skills"),
        SkillTarget::Codex => home()?.join(".codex/skills"),
        SkillTarget::Directory(directory) => {
            if !directory.is_absolute() {
                return Err("--dir needs an absolute path.".to_string());
            }
            directory.clone()
        }
    })
}

pub struct Written {
    pub path: PathBuf,
    pub replaced: bool,
}

/// Writes `<root>/neati/SKILL.md`. The skill folder and the file must not be
/// links, and an existing file must be neati's own copy unless `force`.
pub fn write_skill(root: &Path, force: bool) -> Result<Written, String> {
    let folder = root.join("neati");
    let path = folder.join("SKILL.md");
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let first = write_skill(root.path(), false).expect("fresh install");
        assert!(!first.replaced);
        assert_eq!(
            std::fs::read_to_string(&first.path).expect("written"),
            SKILL
        );
        let second = write_skill(root.path(), false).expect("update own copy");
        assert!(second.replaced);
    }

    #[test]
    fn install_never_replaces_someone_elses_skill_without_force() {
        let root = tempfile::tempdir().expect("temporary skills root");
        let path = root.path().join("neati/SKILL.md");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("skill folder");
        std::fs::write(&path, "my own notes").expect("foreign skill");
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
    fn named_directories_must_be_absolute() {
        assert!(skills_root(None, &SkillTarget::Directory(PathBuf::from("rel"))).is_err());
        assert!(skills_root(None, &SkillTarget::Claude).is_err());
        assert_eq!(
            skills_root(Some(Path::new("/Users/me")), &SkillTarget::Codex).expect("codex root"),
            PathBuf::from("/Users/me/.codex/skills")
        );
    }
}
