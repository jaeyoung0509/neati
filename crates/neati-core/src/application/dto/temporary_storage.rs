//! Observations and reviewed scopes; filesystem authority remains private.
use crate::domain::storage::{TemporaryContentKind, TemporaryRemovalMode, TemporaryUsageState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TemporaryUsageObservation {
    pub state: TemporaryUsageState,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub observed_at: u64,
    pub probe: String,
    pub evidence: Vec<String>,
    pub limitation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TemporaryRemovalOption {
    pub id: String,
    pub mode: TemporaryRemovalMode,
    pub path: String,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub allocated_bytes: Option<u64>,
    pub partial: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TemporaryStorageItem {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub logical_bytes: Option<u64>,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub allocated_bytes: Option<u64>,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub newest_activity: Option<u64>,
    pub partial: bool,
    pub contents: Vec<TemporaryContentKind>,
    pub physical_overlap: bool,
    pub usage: TemporaryUsageObservation,
    pub options: Vec<TemporaryRemovalOption>,
    pub selected_by_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TemporaryStorageInventory {
    pub scan_id: String,
    pub roots: Vec<String>,
    pub items: Vec<TemporaryStorageItem>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub observed_at: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub expires_at: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub observed_allocated_bytes: u64,
    pub unknown_estimates: usize,
    pub partial: bool,
    pub physical_overlap: bool,
    pub cancelled: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemporaryStorageEvent {
    Started {
        scan_id: String,
    },
    ItemFound {
        item: TemporaryStorageItem,
    },
    Finished {
        inventory: TemporaryStorageInventory,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TemporaryReviewPreview {
    pub id: uuid::Uuid,
    pub selected: Vec<TemporaryRemovalOption>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub known_allocated_bytes: u64,
    pub unknown_estimates: usize,
    pub has_unknown_usage: bool,
    pub has_whole_folders: bool,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub expires_at: u64,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract_fixture() -> serde_json::Value {
        let option = TemporaryRemovalOption {
            id: "scope".into(),
            mode: TemporaryRemovalMode::WholeFolder,
            path: "/fixture/temporary/unit".into(),
            allocated_bytes: Some(4096),
            partial: false,
            blocked_reason: None,
        };
        let inventory = TemporaryStorageInventory {
            scan_id: "inventory".into(),
            roots: vec!["/fixture/temporary".into()],
            items: vec![TemporaryStorageItem {
                id: "unit".into(),
                name: "unit".into(),
                path: option.path.clone(),
                logical_bytes: Some(100),
                allocated_bytes: Some(4096),
                newest_activity: Some(123),
                partial: false,
                physical_overlap: false,
                contents: vec![TemporaryContentKind::Unknown],
                usage: TemporaryUsageObservation {
                    state: TemporaryUsageState::UnableToDetermine,
                    observed_at: 123,
                    probe: "Fixture probe".into(),
                    evidence: vec!["No complete use verdict".into()],
                    limitation: "Not proof of abandonment".into(),
                },
                options: vec![option.clone()],
                selected_by_default: false,
            }],
            observed_at: 123,
            expires_at: 1023,
            observed_allocated_bytes: 4096,
            unknown_estimates: 0,
            partial: false,
            physical_overlap: false,
            cancelled: false,
            available: true,
            unavailable_reason: None,
            notes: vec!["Fixture contract".into()],
        };
        let preview = TemporaryReviewPreview {
            id: uuid::Uuid::nil(),
            selected: vec![option],
            known_allocated_bytes: 4096,
            unknown_estimates: 0,
            has_unknown_usage: true,
            has_whole_folders: true,
            expires_at: 423,
            warnings: vec!["Fixture warning".into()],
        };
        serde_json::json!({ "inventory": inventory, "preview": preview })
    }

    #[test]
    fn temporary_response_matches_native_contract_golden() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../src/lib/bindings/temporary-storage.golden.json"
        ))
        .unwrap();
        assert_eq!(contract_fixture(), golden);
    }

    #[test]
    #[ignore = "Regenerates the native serialized contract consumed by frontend fixtures"]
    fn export_temporary_storage_contract() {
        let destination = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../src/lib/bindings/temporary-storage.golden.json");
        std::fs::write(
            destination,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&contract_fixture()).unwrap()
            ),
        )
        .unwrap();
    }

    #[test]
    fn actual_temporary_models_reject_unsafe_ipc_integers() {
        let option = TemporaryRemovalOption {
            id: "one".into(),
            mode: TemporaryRemovalMode::WholeFolder,
            path: "/fixture/one".into(),
            allocated_bytes: Some(u64::MAX),
            partial: false,
            blocked_reason: None,
        };
        let preview = TemporaryReviewPreview {
            id: uuid::Uuid::nil(),
            selected: vec![option],
            known_allocated_bytes: u64::MAX,
            unknown_estimates: 0,
            has_unknown_usage: true,
            has_whole_folders: true,
            expires_at: u64::MAX,
            warnings: vec![],
        };
        assert!(serde_json::to_value(&preview).is_err());
        let mut preview = preview;
        preview.known_allocated_bytes = 9_007_199_254_740_991;
        preview.selected[0].allocated_bytes = Some(9_007_199_254_740_991);
        preview.expires_at = 9_007_199_254_740_991;
        let json = serde_json::to_value(preview).unwrap();
        assert_eq!(json["known_allocated_bytes"], 9_007_199_254_740_991u64);
        assert_eq!(
            json["selected"][0]["allocated_bytes"],
            9_007_199_254_740_991u64
        );
        assert_eq!(json["expires_at"], 9_007_199_254_740_991u64);
    }
}
