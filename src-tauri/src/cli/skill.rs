//! `neati skill`: the terminal adapter for installing the agent skill.
//!
//! The skill text and the install rules (own copy only, never through a link)
//! live in `services::command_line`, shared with the window's Settings.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::services::command_line::{write_skill, SKILL};

use super::args::SkillTarget;
use super::output::{display_path, print_json};

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
    let written = write_skill(&root, force)?;
    let report = InstallReport {
        path: written.path.to_string_lossy().into_owned(),
        replaced: written.replaced,
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
        SkillTarget::Claude => {
            home()?.join(crate::models::AgentSkillTarget::Claude.relative_root())
        }
        SkillTarget::Codex => home()?.join(crate::models::AgentSkillTarget::Codex.relative_root()),
        SkillTarget::Directory(directory) => {
            if !directory.is_absolute() {
                return Err("--dir needs an absolute path.".to_string());
            }
            directory.clone()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
