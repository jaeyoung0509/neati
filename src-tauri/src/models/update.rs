use serde::{Deserialize, Serialize};

/// The outcome of one user-initiated (or opted-in launch) update check.
///
/// `Unavailable` means this build cannot update itself at all (no adapter on
/// this platform, or no signing key configured); `Failed` means the check ran
/// and could not complete. The interface words each differently: one is a
/// standing fact about the build, the other is retryable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateCheck {
    UpToDate {
        current_version: String,
    },
    Available {
        current_version: String,
        version: String,
        notes: Option<String>,
        /// RFC 3339 publication time from the signed manifest, when present.
        published_at: Option<String>,
    },
    Unavailable {
        reason: String,
    },
    Failed {
        message: String,
    },
}
