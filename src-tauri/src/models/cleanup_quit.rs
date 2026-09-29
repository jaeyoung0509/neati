use serde::{Deserialize, Serialize};

/// Adapter projection of short-lived, backend-owned process authority.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CleanupQuitApp {
    pub name: String,
    pub lease_id: String,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CleanupQuitPreview {
    pub scan_id: String,
    pub apps: Vec<CleanupQuitApp>,
    /// Names of selected caches whose owner cannot be safely quit by neati.
    pub unavailable: Vec<String>,
}
