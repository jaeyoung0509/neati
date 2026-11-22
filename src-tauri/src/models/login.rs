use serde::{Deserialize, Serialize};

/// Whether this installed copy starts when the person logs in.
///
/// Read from the system's own registration, never from a saved preference:
/// the switch shows what will actually happen at the next login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LaunchAtLogin {
    Enabled,
    Disabled,
    /// No adapter on this platform, or this copy cannot be registered.
    Unavailable {
        reason: String,
    },
}
