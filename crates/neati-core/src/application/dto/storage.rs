//! What the storage workflows report across the interface boundary.
//!
//! Every structure here is an observation: an inventory, a scan result, a
//! preview, or a progress step. The taxonomy those observations are described
//! with — what kind of large file this is, what a filter means, how strong a
//! relationship is — lives in [`crate::domain::storage`]. The authority that
//! turns a selection into a mutation never appears here: a `TrashPlanPreview`
//! carries a plan ID and byte totals, not the plan.

use crate::domain::storage::{
    AppInstallSource, AppLeftoverClassification, AppRelatedConfidence, AppRelatedKind,
    LargeFileFilter, LargeFileKind,
};
use crate::domain::ObservationQuality;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LargeFileItem {
    pub id: String,
    pub name: String,
    pub display_parent: String,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub logical_size: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_size: u64,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub modified_at: Option<u64>,
    pub kind: LargeFileKind,
    pub extension: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LargeFileScanRequest {
    pub roots: Vec<String>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub min_size_bytes: u64,
    #[serde(default)]
    pub filter: LargeFileFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct LargeFileScanResult {
    pub scan_id: String,
    pub items: Vec<LargeFileItem>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub entries_scanned: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_entries: u64,
    pub cancelled: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LargeFileScanEvent {
    Started {
        scan_id: String,
    },
    RootStarted {
        root: String,
    },
    Progress {
        root: String,
        #[serde(with = "crate::ipc_numeric::u64")]
        #[specta(type = u64)]
        entries_scanned: u64,
        #[serde(with = "crate::ipc_numeric::u64")]
        #[specta(type = u64)]
        matches_found: u64,
    },
    ItemFound {
        item: LargeFileItem,
    },
    RootFinished {
        root: String,
    },
    Finished {
        result: LargeFileScanResult,
    },
    Cancelled {
        scan_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub display_path: String,
    pub executable_name: Option<String>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub logical_size: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_size: u64,
    #[serde(with = "crate::ipc_numeric::option_u64")]
    #[specta(type = Option<u64>)]
    pub modified_at: Option<u64>,
    pub install_source: AppInstallSource,
    pub is_running: bool,
    pub is_system_protected: bool,
    #[serde(default)]
    pub quality: ObservationQuality,
    #[serde(default)]
    pub size_quality: ObservationQuality,
    #[serde(default)]
    pub incomplete_reason: Option<String>,
    #[serde(default, with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_entries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct InstalledAppInventory {
    pub apps: Vec<InstalledApp>,
    pub quality: ObservationQuality,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_entry_count: u64,
    pub incomplete_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AppLeftoverItem {
    pub id: String,
    pub name: String,
    pub display_path: String,
    pub kind: AppRelatedKind,
    pub classification: AppLeftoverClassification,
    pub owner_names: Vec<String>,
    pub evidence: String,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub logical_size: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_size: u64,
    pub quality: ObservationQuality,
    pub incomplete_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AppLeftoverInventory {
    pub items: Vec<AppLeftoverItem>,
    pub quality: ObservationQuality,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub observed_roots: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_entry_count: u64,
    pub incomplete_reasons: Vec<String>,
    pub limitation: String,
}

#[cfg(test)]
mod leftover_numeric_tests {
    use super::*;
    #[test]
    fn leftover_real_models_preserve_safe_numbers_and_refuse_unsafe_bytes_and_counts() {
        let mut item = AppLeftoverItem {
            id: "read-only".into(),
            name: "fixture".into(),
            display_path: "/fixture".into(),
            kind: AppRelatedKind::Cache,
            classification: AppLeftoverClassification::PossibleRemovedOwner,
            owner_names: vec![],
            evidence: "limited observation".into(),
            logical_size: crate::ipc_numeric::MAX_SAFE_INTEGER,
            allocated_size: 4096,
            quality: ObservationQuality::Fresh,
            incomplete_reason: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(
            serde_json::from_str::<AppLeftoverItem>(&json).unwrap(),
            item
        );
        item.logical_size += 1;
        assert!(serde_json::to_string(&item).is_err());
        item.logical_size = 1;
        item.allocated_size = crate::ipc_numeric::MAX_SAFE_INTEGER + 1;
        assert!(serde_json::to_string(&item).is_err());
        let mut inventory = AppLeftoverInventory {
            items: vec![],
            quality: ObservationQuality::Fresh,
            observed_roots: crate::ipc_numeric::MAX_SAFE_INTEGER + 1,
            skipped_entry_count: 0,
            incomplete_reasons: vec![],
            limitation: "read-only".into(),
        };
        assert!(serde_json::to_string(&inventory).is_err());
        inventory.observed_roots = 6;
        inventory.skipped_entry_count = crate::ipc_numeric::MAX_SAFE_INTEGER + 1;
        assert!(serde_json::to_string(&inventory).is_err());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AppRelatedItem {
    pub id: String,
    pub name: String,
    pub display_path: String,
    pub kind: AppRelatedKind,
    pub confidence: AppRelatedConfidence,
    pub evidence: String,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub logical_size: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_size: u64,
    pub selected_by_default: bool,
    #[serde(default)]
    pub quality: ObservationQuality,
    #[serde(default)]
    pub incomplete_reason: Option<String>,
    #[serde(default, with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped_entries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AppUninstallInspection {
    pub inspection_id: String,
    pub app: InstalledApp,
    pub related_items: Vec<AppRelatedItem>,
    pub incomplete: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TrashPlanPreview {
    pub id: uuid::Uuid,
    pub item_count: usize,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub logical_size: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub allocated_size: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub expires_at: u64,
    #[serde(default)]
    pub size_is_lower_bound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TrashItemResult {
    pub item_id: String,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TrashResult {
    pub moved_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub moved_allocated_size: u64,
    pub items: Vec<TrashItemResult>,
    #[serde(default)]
    pub size_is_lower_bound: bool,
}
