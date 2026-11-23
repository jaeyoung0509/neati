//! Automatic Clean (#507, owner direction 2026-11-24): the one-button set on a
//! schedule, off by default.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticCleanInterval {
    Daily,
    #[default]
    Weekly,
}

impl AutomaticCleanInterval {
    pub fn seconds(self) -> u64 {
        match self {
            Self::Daily => 24 * 60 * 60,
            Self::Weekly => 7 * 24 * 60 * 60,
        }
    }
}

/// The person's choice. Missing fields deserialize to off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AutomaticCleanSettings {
    pub enabled: bool,
    pub interval: AutomaticCleanInterval,
}

/// Why the last check did not clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticCleanWait {
    Disabled,
    NotDue,
    OnBattery,
    /// The person used the Mac within the last few minutes.
    PersonActive,
    /// Power source or input idleness could not be observed here.
    ConditionsUnknown,
    /// A run is already in progress.
    Running,
}

/// One automatic run, kept so Settings can say what happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AutomaticCleanRun {
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub at: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub reclaimed_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub removed: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub failed: u64,
    /// Why the run stopped before removing anything, when it did.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AutomaticCleanStatus {
    pub settings: AutomaticCleanSettings,
    pub last_run: Option<AutomaticCleanRun>,
    /// Earliest time the next run may start.
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub next_due_at: Option<u64>,
    /// The last check's reason for not running, if it did not run.
    pub waiting: Option<AutomaticCleanWait>,
    /// False where neati cannot observe idleness or power (not macOS).
    pub available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_clean_is_off_until_chosen() {
        let settings: AutomaticCleanSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.enabled);
        assert_eq!(settings.interval, AutomaticCleanInterval::Weekly);
        assert_eq!(AutomaticCleanInterval::Daily.seconds(), 86_400);
    }

    #[test]
    fn a_run_crosses_ipc_within_the_safe_range() {
        let run = AutomaticCleanRun {
            at: crate::ipc_numeric::MAX_SAFE_INTEGER,
            reclaimed_bytes: 5,
            removed: 1,
            failed: 0,
            error: None,
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(
            json["at"],
            serde_json::json!(crate::ipc_numeric::MAX_SAFE_INTEGER)
        );
        assert!(serde_json::to_value(AutomaticCleanRun {
            at: crate::ipc_numeric::MAX_SAFE_INTEGER + 1,
            ..run
        })
        .is_err());
    }
}
