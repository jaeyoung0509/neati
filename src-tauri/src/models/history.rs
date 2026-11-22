use serde::{Deserialize, Serialize};

/// One completed cleanup, as counts only.
///
/// Projected from the de-identified operation history: no path, item name or
/// identifier exists in the file, so none can appear here. `removed_bytes` is
/// file data removed, not a measured change in free space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CleanupHistoryEntry {
    /// Seconds since the Unix epoch.
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub finished_at: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub item_count: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub removed_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub partial_count: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub failed_count: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_round_trips_and_refuses_a_count_javascript_cannot_hold() {
        let entry = CleanupHistoryEntry {
            finished_at: 1_795_000_000,
            item_count: 3,
            removed_bytes: 9_007_199_254_740_991,
            partial_count: 0,
            failed_count: 1,
            skipped_count: 0,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(
            serde_json::from_str::<CleanupHistoryEntry>(&json).unwrap(),
            entry
        );

        let unsafe_entry = CleanupHistoryEntry {
            removed_bytes: u64::MAX,
            ..entry
        };
        assert!(serde_json::to_string(&unsafe_entry).is_err());
    }
}
