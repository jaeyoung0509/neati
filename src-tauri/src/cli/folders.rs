//! `neati projects add|remove PATH`: the one setting the CLI writes (#500).
//!
//! A running app holds its own copy of the settings and would overwrite a
//! change made underneath it, so the edit refuses while neati is open. The
//! folder list is validated by the same rule the app's settings save applies,
//! and every other setting is written back exactly as it was read.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::models::NeatiSettings;

use super::context::{config_dir, read_settings, with_project_folders};
use super::output::{display_path, print_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Add,
    Remove,
}

#[derive(Serialize)]
struct FoldersJson<'a> {
    changed: bool,
    folders: &'a [String],
}

pub fn run(edit: Edit, folder: &str, json: bool) -> Result<i32, String> {
    let environment = neati_platform::PlatformEnvironment::native();
    let home = environment.user_home();
    if let Some(pid) = running_app_pid() {
        return Err(format!(
            "neati is open (process {pid}) and would overwrite this change. Quit neati, or change project folders in the app (Clean → Projects)."
        ));
    }
    let directory = config_dir(&environment)
        .ok_or_else(|| "The settings folder is unknown on this machine.".to_string())?;
    let working_directory = std::env::current_dir().ok();
    let absolute = absolute(folder, working_directory.as_deref())?;
    let before = read_settings(&directory)?;
    let after = edited(before.clone(), edit, &absolute)?;
    let changed = after.project_folders != before.project_folders;
    if changed {
        crate::settings_store::save(&directory, &after)?;
    }
    if json {
        print_json(
            "projects",
            FoldersJson {
                changed,
                folders: &after.project_folders,
            },
        )?;
    } else {
        let shown = display_path(&absolute.to_string_lossy(), home.as_deref());
        println!(
            "{}",
            match (edit, changed) {
                (Edit::Add, true) =>
                    format!("Saved {shown}. `neati clean` now includes what your cleanup rules allow there."),
                (Edit::Add, false) => format!("{shown} was already saved."),
                (Edit::Remove, true) => format!("Forgot {shown}."),
                (Edit::Remove, false) => format!("{shown} was not saved."),
            }
        );
    }
    Ok(0)
}

/// The settings after the edit. Adding requires an existing directory and
/// passes the saved-folder rules; removing matches the saved spelling.
pub fn edited(settings: NeatiSettings, edit: Edit, folder: &Path) -> Result<NeatiSettings, String> {
    let spelled = spelling(folder);
    match edit {
        Edit::Add => {
            if !folder.is_dir() {
                return Err(format!("{} is not a folder.", folder.display()));
            }
            with_project_folders(settings, &[spelled], None).map(NeatiSettings::sanitize)
        }
        Edit::Remove => {
            let mut settings = settings;
            settings
                .project_folders
                .retain(|saved| spelling(Path::new(saved)) != spelled);
            Ok(settings)
        }
    }
}

fn absolute(folder: &str, working_directory: Option<&Path>) -> Result<PathBuf, String> {
    let path = Path::new(folder);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    working_directory
        .map(|directory| directory.join(path))
        .ok_or_else(|| format!("{folder} is relative and the working folder is unknown."))
}

/// One spelling per folder: no trailing slash.
fn spelling(path: &Path) -> String {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Another neati process, by its bundle executable. The CLI itself is that
/// executable too, so its own process is excluded.
fn running_app_pid() -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let own = std::process::id();
    system
        .processes()
        .iter()
        .find(|(pid, process)| {
            pid.as_u32() != own
                && process
                    .exe()
                    .is_some_and(|exe| exe.ends_with("Contents/MacOS/Neati"))
        })
        .map(|(pid, _)| pid.as_u32())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_keeps_every_other_setting_and_never_duplicates() {
        let folder = tempfile::tempdir().expect("project folder");
        let settings = NeatiSettings {
            excluded_signatures: vec!["system.npm.cache".to_string()],
            ..NeatiSettings::default()
        };
        let added = edited(settings.clone(), Edit::Add, folder.path()).expect("added");
        assert_eq!(added.excluded_signatures, settings.excluded_signatures);
        assert_eq!(added.project_folders, [spelling(folder.path())]);
        let again = edited(added.clone(), Edit::Add, folder.path()).expect("added again");
        assert_eq!(again.project_folders, added.project_folders);
    }

    #[test]
    fn adding_refuses_what_the_app_would_refuse() {
        let missing = Path::new("/definitely/not/a/folder/neati");
        assert!(edited(NeatiSettings::default(), Edit::Add, missing).is_err());
        let file = tempfile::NamedTempFile::new().expect("a file");
        assert!(edited(NeatiSettings::default(), Edit::Add, file.path()).is_err());
    }

    #[test]
    fn removing_matches_the_saved_spelling() {
        let settings = NeatiSettings {
            project_folders: vec!["/Users/me/code".to_string(), "/Users/me/oss".to_string()],
            ..NeatiSettings::default()
        };
        let removed =
            edited(settings, Edit::Remove, Path::new("/Users/me/code/")).expect("removed");
        assert_eq!(removed.project_folders, ["/Users/me/oss"]);
    }

    #[test]
    fn relative_folders_resolve_against_the_working_folder() {
        assert_eq!(
            absolute("code", Some(Path::new("/Users/me"))).expect("absolute"),
            PathBuf::from("/Users/me/code")
        );
        assert!(absolute("code", None).is_err());
    }
}
