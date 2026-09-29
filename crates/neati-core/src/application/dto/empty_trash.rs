use serde::{Deserialize, Serialize};
use specta::Type;
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EmptyTrashPreview {
    pub plan_id: String,
    pub scope: String,
    pub items: Vec<String>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub observed_bytes: u64,
    pub entry_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EmptyTrashItemResult {
    pub path: String,
    pub removed: bool,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EmptyTrashResult {
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub removed_bytes: u64,
    pub removed_entries: usize,
    pub cancelled: bool,
    pub items: Vec<EmptyTrashItemResult>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trash_bytes_use_the_shared_numeric_guard() {
        let result = EmptyTrashResult {
            removed_bytes: u64::MAX,
            removed_entries: 0,
            cancelled: false,
            items: vec![],
        };
        assert!(serde_json::to_value(result).is_err());
        let preview = EmptyTrashPreview {
            plan_id: "id".into(),
            scope: "home Trash".into(),
            items: vec![],
            observed_bytes: u64::MAX,
            entry_count: 0,
        };
        assert!(serde_json::to_value(preview).is_err());
    }
}
