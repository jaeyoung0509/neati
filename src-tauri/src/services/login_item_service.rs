//! Launch at login for the copy of neati that is running.
//!
//! The service is built once at the composition root from the two facts it
//! needs (the described environment and this process's executable) and owns no
//! other state: the LaunchAgent on disk is the source of truth.

use std::path::PathBuf;
use std::sync::Arc;

use neati_core::domain::platform::PlatformKind;
use neati_platform::login_item::{LoginItem, LoginItemStatus};
use neati_platform::PlatformEnvironment;

use crate::models::LaunchAtLogin;

pub const UNAVAILABLE_PLATFORM: &str = "Start at login is available only on macOS.";
const UNAVAILABLE_HOME: &str = "neati could not find your home folder.";
const UNAVAILABLE_EXECUTABLE: &str = "neati could not determine where it is installed.";

pub struct LoginItemService {
    item: Result<LoginItem, &'static str>,
}

impl LoginItemService {
    pub fn new(environment: &Arc<PlatformEnvironment>, executable: Option<PathBuf>) -> Self {
        let item = if environment.platform() != PlatformKind::Macos {
            Err(UNAVAILABLE_PLATFORM)
        } else {
            match (environment.user_home(), executable) {
                (None, _) => Err(UNAVAILABLE_HOME),
                (_, None) => Err(UNAVAILABLE_EXECUTABLE),
                (Some(home), Some(executable)) => Ok(LoginItem::new(
                    home.join("Library").join("LaunchAgents"),
                    executable,
                )),
            }
        };
        Self { item }
    }

    pub fn status(&self) -> LaunchAtLogin {
        match &self.item {
            Err(reason) => LaunchAtLogin::Unavailable {
                reason: (*reason).to_string(),
            },
            Ok(item) => match item.status() {
                LoginItemStatus::Enabled => LaunchAtLogin::Enabled,
                LoginItemStatus::Disabled => LaunchAtLogin::Disabled,
                LoginItemStatus::Unavailable { reason } => LaunchAtLogin::Unavailable { reason },
            },
        }
    }

    /// Applies the request and reports what the system now holds.
    pub fn set(&self, enabled: bool) -> Result<LaunchAtLogin, String> {
        let item = self.item.as_ref().map_err(|reason| (*reason).to_string())?;
        let outcome = if enabled {
            item.enable()
        } else {
            item.disable()
        };
        outcome.map_err(|error| {
            let message = error.to_string();
            crate::diagnostics::log_error("login-item", &message);
            message
        })?;
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macos_environment(home: &std::path::Path) -> Arc<PlatformEnvironment> {
        Arc::new(
            PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
                .with_platform(PlatformKind::Macos)
                .with_home(home),
        )
    }

    #[test]
    fn enabling_writes_the_agent_under_the_described_home() {
        let home = tempfile::tempdir().unwrap();
        let service = LoginItemService::new(
            &macos_environment(home.path()),
            Some("/Applications/neati.app/Contents/MacOS/Neati".into()),
        );
        assert_eq!(service.status(), LaunchAtLogin::Disabled);
        assert_eq!(service.set(true).unwrap(), LaunchAtLogin::Enabled);
        assert!(home
            .path()
            .join("Library/LaunchAgents/com.neati.desktop.login.plist")
            .is_file());
        assert_eq!(service.set(false).unwrap(), LaunchAtLogin::Disabled);
    }

    #[test]
    fn a_build_tree_copy_reports_unavailable_and_refuses_to_enable() {
        let home = tempfile::tempdir().unwrap();
        let service = LoginItemService::new(
            &macos_environment(home.path()),
            Some("/Users/me/neati/target/debug/Neati".into()),
        );
        assert!(matches!(
            service.status(),
            LaunchAtLogin::Unavailable { .. }
        ));
        assert!(service.set(true).is_err());
        assert!(!home.path().join("Library/LaunchAgents").exists());
    }

    #[test]
    fn another_platform_has_no_adapter_and_says_so() {
        let home = tempfile::tempdir().unwrap();
        let environment = Arc::new(
            PlatformEnvironment::simulated(neati_platform::PathFlavor::Windows)
                .with_platform(PlatformKind::Windows)
                .with_home(home.path()),
        );
        let service = LoginItemService::new(
            &environment,
            Some("/Applications/neati.app/Contents/MacOS/Neati".into()),
        );
        assert_eq!(
            service.status(),
            LaunchAtLogin::Unavailable {
                reason: UNAVAILABLE_PLATFORM.to_string()
            }
        );
        assert_eq!(service.set(true).unwrap_err(), UNAVAILABLE_PLATFORM);
    }
}
