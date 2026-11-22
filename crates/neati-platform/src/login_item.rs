//! Launch at login through a per-user LaunchAgent.
//!
//! neati is not signed with an Apple Developer ID, so the adapter writes one
//! property list it owns under `~/Library/LaunchAgents` instead of registering
//! with `SMAppService`. The plist is the only state: `status` reads it back, so
//! the interface never shows a preference the system does not hold. launchd
//! reads the directory at the next login; nothing is loaded into the current
//! session and no process is started here.
//!
//! The adapter is pure with respect to its inputs (a directory and the app's
//! executable), so the tests run against a temporary directory on any host.
//! Whether the platform has LaunchAgents at all is decided by the caller.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Reverse-DNS label of the agent, also its file stem.
pub const LOGIN_ITEM_LABEL: &str = "com.neati.desktop.login";
/// Argument the agent passes so a login start stays in the menu bar.
pub const BACKGROUND_LAUNCH_ARGUMENT: &str = "--background";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginItemStatus {
    /// The agent exists and starts this installed copy.
    Enabled,
    Disabled,
    /// This copy cannot be registered; `reason` says what to do about it.
    Unavailable {
        reason: String,
    },
}

#[derive(Debug)]
pub enum LoginItemError {
    Unavailable(String),
    /// The agent path is occupied by something neati did not write.
    Foreign(PathBuf),
    Io(io::Error),
}

impl std::fmt::Display for LoginItemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => formatter.write_str(reason),
            Self::Foreign(path) => write!(
                formatter,
                "{} is not a file neati created, so it was left unchanged.",
                path.display()
            ),
            Self::Io(error) => write!(formatter, "The login item could not be updated: {error}"),
        }
    }
}

impl From<io::Error> for LoginItemError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// One installed copy of neati and the directory its agent lives in.
#[derive(Debug, Clone)]
pub struct LoginItem {
    agents_dir: PathBuf,
    executable: PathBuf,
}

impl LoginItem {
    /// `agents_dir` is `~/Library/LaunchAgents`; `executable` is the running
    /// binary inside the application bundle.
    pub fn new(agents_dir: impl Into<PathBuf>, executable: impl Into<PathBuf>) -> Self {
        Self {
            agents_dir: agents_dir.into(),
            executable: executable.into(),
        }
    }

    pub fn agent_path(&self) -> PathBuf {
        self.agents_dir.join(format!("{LOGIN_ITEM_LABEL}.plist"))
    }

    /// Why this copy cannot be registered, if it cannot.
    ///
    /// A build-tree binary would register a path that the next build deletes,
    /// and a translocated app runs from a random read-only mount that does not
    /// exist after the next launch. Both would leave a dangling agent.
    fn unavailable_reason(&self) -> Option<String> {
        let path = self.executable.to_string_lossy();
        if !self.executable.is_absolute() {
            return Some("neati could not determine where it is installed.".to_string());
        }
        if path.contains("/AppTranslocation/") {
            return Some(
                "Move neati to the Applications folder and open it from there to start it at login."
                    .to_string(),
            );
        }
        let in_bundle = self
            .executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .is_some_and(|bundle| bundle.extension().is_some_and(|ext| ext == "app"))
            && path.contains(".app/Contents/MacOS/");
        if !in_bundle {
            return Some(
                "Start at login is available only in the installed neati app.".to_string(),
            );
        }
        None
    }

    pub fn status(&self) -> LoginItemStatus {
        if let Some(reason) = self.unavailable_reason() {
            return LoginItemStatus::Unavailable { reason };
        }
        match fs::symlink_metadata(self.agent_path()) {
            Ok(meta) if meta.file_type().is_file() => match fs::read_to_string(self.agent_path()) {
                // An agent for another copy (an old install path) is not this
                // copy's registration; enabling rewrites it.
                Ok(contents) if contents == self.render() => LoginItemStatus::Enabled,
                _ => LoginItemStatus::Disabled,
            },
            _ => LoginItemStatus::Disabled,
        }
    }

    pub fn enable(&self) -> Result<(), LoginItemError> {
        if let Some(reason) = self.unavailable_reason() {
            return Err(LoginItemError::Unavailable(reason));
        }
        self.refuse_foreign_entry()?;
        fs::create_dir_all(&self.agents_dir)?;
        crate::file_ops::atomic_write(&self.agent_path(), self.render().as_bytes())?;
        Ok(())
    }

    /// Removes neati's agent. A missing agent is already the requested state.
    pub fn disable(&self) -> Result<(), LoginItemError> {
        self.refuse_foreign_entry()?;
        match fs::remove_file(self.agent_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// The agent path must be absent or a regular file. A link or directory
    /// there was not written by neati and is never followed or replaced.
    fn refuse_foreign_entry(&self) -> Result<(), LoginItemError> {
        match fs::symlink_metadata(self.agent_path()) {
            Ok(meta) if meta.file_type().is_file() => Ok(()),
            Ok(_) => Err(LoginItemError::Foreign(self.agent_path())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn render(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{executable}</string>
		<string>{argument}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
	<key>ProcessType</key>
	<string>Interactive</string>
</dict>
</plist>
"#,
            label = LOGIN_ITEM_LABEL,
            executable = xml_escape(&self.executable.to_string_lossy()),
            argument = BACKGROUND_LAUNCH_ARGUMENT,
        )
    }
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLED: &str = "/Applications/neati.app/Contents/MacOS/Neati";

    fn item(dir: &Path, executable: &str) -> LoginItem {
        LoginItem::new(dir.join("LaunchAgents"), executable)
    }

    #[test]
    fn enable_then_disable_round_trips_through_the_plist() {
        let home = tempfile::tempdir().unwrap();
        let login = item(home.path(), INSTALLED);
        assert_eq!(login.status(), LoginItemStatus::Disabled);

        login.enable().unwrap();
        assert_eq!(login.status(), LoginItemStatus::Enabled);
        let plist = fs::read_to_string(login.agent_path()).unwrap();
        assert!(plist.contains("<string>/Applications/neati.app/Contents/MacOS/Neati</string>"));
        assert!(plist.contains("<string>--background</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));

        login.disable().unwrap();
        assert_eq!(login.status(), LoginItemStatus::Disabled);
        assert!(!login.agent_path().exists());
        // Disabling an absent agent is already the requested state.
        login.disable().unwrap();
    }

    #[test]
    fn an_agent_for_another_install_path_is_not_this_copys_registration() {
        let home = tempfile::tempdir().unwrap();
        item(home.path(), "/Users/me/Apps/neati.app/Contents/MacOS/Neati")
            .enable()
            .unwrap();
        let current = item(home.path(), INSTALLED);
        assert_eq!(current.status(), LoginItemStatus::Disabled);
        current.enable().unwrap();
        assert_eq!(current.status(), LoginItemStatus::Enabled);
    }

    #[test]
    fn build_tree_and_translocated_copies_are_unavailable_and_write_nothing() {
        let home = tempfile::tempdir().unwrap();
        for executable in [
            "/Users/me/project/target/debug/Neati",
            "/private/var/folders/ab/T/AppTranslocation/1234/d/neati.app/Contents/MacOS/Neati",
            "Neati",
        ] {
            let login = item(home.path(), executable);
            assert!(
                matches!(login.status(), LoginItemStatus::Unavailable { .. }),
                "{executable}"
            );
            assert!(matches!(
                login.enable(),
                Err(LoginItemError::Unavailable(_))
            ));
            assert!(!login.agent_path().exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_agent_path_is_never_followed_or_replaced() {
        let home = tempfile::tempdir().unwrap();
        let login = item(home.path(), INSTALLED);
        fs::create_dir_all(home.path().join("LaunchAgents")).unwrap();
        let victim = home.path().join("victim.txt");
        fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, login.agent_path()).unwrap();

        assert_eq!(login.status(), LoginItemStatus::Disabled);
        assert!(matches!(login.enable(), Err(LoginItemError::Foreign(_))));
        assert!(matches!(login.disable(), Err(LoginItemError::Foreign(_))));
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
        assert!(fs::symlink_metadata(login.agent_path())
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn the_executable_path_is_xml_escaped() {
        let home = tempfile::tempdir().unwrap();
        let login = item(
            home.path(),
            "/Applications/R&D <tools>/neati.app/Contents/MacOS/Neati",
        );
        login.enable().unwrap();
        let plist = fs::read_to_string(login.agent_path()).unwrap();
        assert!(
            plist.contains("/Applications/R&amp;D &lt;tools&gt;/neati.app/Contents/MacOS/Neati")
        );
        assert_eq!(login.status(), LoginItemStatus::Enabled);
    }
}
