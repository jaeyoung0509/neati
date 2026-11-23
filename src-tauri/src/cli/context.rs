//! The CLI's composition root.
//!
//! The CLI is a driving adapter. It builds the same `DesktopState` the window
//! uses, from the same native environment, then keeps only the driving ports
//! `composition::application_ports` binds: the command handlers never see a
//! service. It reads the app's saved settings and never writes them, because
//! a running app keeps its own copy and would overwrite a change.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use neati_platform::PlatformEnvironment;

use crate::composition::ApplicationPorts;
use crate::models::NeatiSettings;

/// The bundle identifier Tauri names the configuration directory after.
const APP_IDENTIFIER: &str = "com.neati.desktop";

/// What a command handler is given: the ports and the home folder for display.
pub struct CliContext {
    pub ports: ApplicationPorts,
    pub home: Option<PathBuf>,
}

impl CliContext {
    /// The native machine with the app's stored settings, plus any project
    /// folders this run adds.
    pub fn native(extra_project_folders: &[String]) -> Result<Self, String> {
        neati_platform::subprocess::set_error_sink(|message| {
            crate::diagnostics::log_error("cli", message)
        });
        let environment = Arc::new(PlatformEnvironment::native());
        let container_host =
            crate::docker::adapter::ContainerHost::from_value(std::env::var("DOCKER_HOST").ok());
        let stored = match config_dir(&environment) {
            Some(directory) => read_settings(&directory)?,
            None => NeatiSettings::default().sanitize(),
        };
        // A relative `--folder` is the person's working directory, observed
        // once here at the root like the executable path in the app.
        let working_directory = std::env::current_dir().ok();
        let settings =
            with_project_folders(stored, extra_project_folders, working_directory.as_deref())?;
        Self::from_environment(environment, container_host, settings)
    }

    /// The same ports around a described environment and settings.
    pub fn from_environment(
        environment: Arc<PlatformEnvironment>,
        container_host: crate::docker::adapter::ContainerHost,
        settings: NeatiSettings,
    ) -> Result<Self, String> {
        let home = environment.user_home();
        let state = crate::composition::desktop_state(environment, container_host);
        state.settings.replace(settings)?;
        Ok(Self {
            ports: crate::composition::application_ports(&state),
            home,
        })
    }
}

/// Where Tauri's `app_config_dir` puts neati's settings on this platform.
pub fn config_dir(environment: &PlatformEnvironment) -> Option<PathBuf> {
    if environment.flavor().is_windows() {
        return environment
            .roaming_app_data()
            .map(|root| root.join(APP_IDENTIFIER));
    }
    let home = environment.user_home()?;
    if cfg!(target_os = "macos") {
        Some(
            home.join("Library/Application Support")
                .join(APP_IDENTIFIER),
        )
    } else {
        Some(home.join(".config").join(APP_IDENTIFIER))
    }
}

/// The app's saved settings, read without the app's repair path.
///
/// A missing file is a fresh install and means defaults, as in the app. A file
/// that does not parse refuses the run instead: defaults would drop the
/// person's exclusions, and the repair rewrites a file a running app owns.
pub fn read_settings(config_dir: &Path) -> Result<NeatiSettings, String> {
    let path = crate::settings_store::settings_path(config_dir);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(NeatiSettings::default().sanitize())
        }
        Err(error) => return Err(format!("neati settings could not be read: {error}")),
    };
    serde_json::from_str::<NeatiSettings>(&contents)
        .map(NeatiSettings::sanitize)
        .map_err(|_| {
            "neati settings could not be parsed. Open neati once to repair them, then run this again."
                .to_string()
        })
}

/// The saved project folders plus this run's, validated by the same rule the
/// settings save applies. Duplicates collapse; relative folders resolve
/// against `working_directory`.
pub fn with_project_folders(
    mut settings: NeatiSettings,
    extra: &[String],
    working_directory: Option<&Path>,
) -> Result<NeatiSettings, String> {
    for folder in extra {
        let path = Path::new(folder);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            working_directory
                .ok_or_else(|| format!("{folder} is relative and the working folder is unknown."))?
                .join(path)
        };
        let absolute = absolute.to_string_lossy().trim_end_matches('/').to_string();
        let absolute = if absolute.is_empty() {
            "/".to_string()
        } else {
            absolute
        };
        if !settings.project_folders.contains(&absolute) {
            settings.project_folders.push(absolute);
        }
    }
    crate::developer_artifacts::validate_saved_project_folders(&settings.project_folders)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neati_platform::path_algebra::PathFlavor;

    #[test]
    fn settings_live_where_the_app_keeps_them() {
        let environment = PlatformEnvironment::simulated(PathFlavor::Posix).with_home("/Users/me");
        let expected = if cfg!(target_os = "macos") {
            "/Users/me/Library/Application Support/com.neati.desktop"
        } else {
            "/Users/me/.config/com.neati.desktop"
        };
        assert_eq!(config_dir(&environment), Some(PathBuf::from(expected)));
    }

    #[test]
    fn unreadable_settings_refuse_instead_of_dropping_exclusions() {
        let directory = tempfile::tempdir().expect("temporary config directory");
        assert_eq!(
            read_settings(directory.path()).expect("a missing file is a fresh install"),
            NeatiSettings::default().sanitize()
        );

        let path = crate::settings_store::settings_path(directory.path());
        std::fs::write(&path, "{ not json").expect("write corrupt settings");
        assert!(read_settings(directory.path()).is_err());
        // The app's repair path would have replaced the file; the CLI did not.
        assert_eq!(
            std::fs::read_to_string(&path).expect("settings still present"),
            "{ not json"
        );

        let saved = NeatiSettings {
            excluded_signatures: vec!["system.npm.cache".to_string()],
            ..NeatiSettings::default()
        };
        std::fs::write(&path, serde_json::to_string(&saved).expect("serialize"))
            .expect("write settings");
        assert_eq!(
            read_settings(directory.path())
                .expect("valid settings")
                .excluded_signatures,
            ["system.npm.cache"]
        );
    }

    #[test]
    fn run_folders_join_the_saved_ones_once() {
        let settings = NeatiSettings {
            project_folders: vec!["/Users/me/code".to_string()],
            ..NeatiSettings::default()
        };
        let merged = with_project_folders(
            settings,
            &[
                "/Users/me/code/".to_string(),
                "work".to_string(),
                "/Users/me/oss".to_string(),
            ],
            Some(Path::new("/Users/me")),
        )
        .expect("valid folders merge");
        assert_eq!(
            merged.project_folders,
            ["/Users/me/code", "/Users/me/work", "/Users/me/oss"]
        );
    }

    #[test]
    fn run_folders_obey_the_saved_folder_rules() {
        let refused = with_project_folders(
            NeatiSettings::default(),
            &["/Users/me/../other".to_string()],
            None,
        );
        assert!(refused.is_err());

        let too_many: Vec<String> = (0..17).map(|index| format!("/Users/me/p{index}")).collect();
        assert!(with_project_folders(NeatiSettings::default(), &too_many, None).is_err());

        assert!(
            with_project_folders(NeatiSettings::default(), &["relative".to_string()], None)
                .is_err()
        );
    }
}
