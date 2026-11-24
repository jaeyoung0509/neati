//! Reviewed storage-management command handlers.
//!
//! Thin Tauri IPC adapters: each handler adapts a `Channel` into the service's
//! progress callback, or forwards the opaque identifiers the interface is
//! allowed to submit. Inventory lifetime, the storage operation gate, the
//! execution budgets, the plan store, and the Trash executor belong to
//! [`crate::services::StorageService`], so no handler decides when a mutation
//! may start or touches backend state directly.

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::commands::DesktopState;
use crate::events::storage::{TauriDeveloperArtifactProgress, TauriLargeFileScanProgress};
use crate::models::{
    AppLeftoverInventory, AppUninstallInspection, DeveloperArtifactScanEvent,
    DeveloperArtifactScanResult, DeveloperWorkspace, DeveloperWorkspaceRestore,
    DeveloperWorkspaceSuggestion, InstalledAppInventory, LargeFileScanEvent, LargeFileScanRequest,
    LargeFileScanResult, RemovalPlanPreview, RemovalResult,
};
use crate::services::progress::{DeveloperArtifactScanSink, LargeFileScanSink};

#[tauri::command]
#[specta::specta]
pub async fn start_temporary_storage_scan(
    on_event: Channel<crate::models::TemporaryStorageEvent>,
    state: State<'_, DesktopState>,
) -> Result<crate::models::TemporaryStorageInventory, String> {
    state
        .storage
        .scan_temporary_storage(Arc::new(
            crate::events::storage::TauriTemporaryStorageProgress::new(on_event),
        ))
        .await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_temporary_storage_scan(
    scan_id: String,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state.storage.cancel_temporary_storage_scan(&scan_id)
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_temporary_storage_review(
    scan_id: String,
    selected_option_ids: Vec<String>,
    state: State<'_, DesktopState>,
) -> Result<crate::models::TemporaryReviewPreview, String> {
    state
        .storage
        .prepare_temporary_storage_review(&scan_id, &selected_option_ids)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn execute_temporary_storage_review(
    plan_id: uuid::Uuid,
    consent: crate::models::TemporaryReviewConsent,
    state: State<'_, DesktopState>,
) -> Result<RemovalResult, String> {
    state
        .storage
        .execute_temporary_storage_review(plan_id, consent)
        .await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_temporary_storage_review(
    plan_id: uuid::Uuid,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state.storage.cancel_temporary_storage_review(plan_id)
}

#[tauri::command]
#[specta::specta]
pub async fn start_large_file_scan(
    request: LargeFileScanRequest,
    on_event: Channel<LargeFileScanEvent>,
    state: State<'_, DesktopState>,
) -> Result<LargeFileScanResult, String> {
    let progress: Arc<dyn LargeFileScanSink> = Arc::new(TauriLargeFileScanProgress::new(on_event));
    state.storage.scan_large_files(request, progress).await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_large_file_scan(
    scan_id: String,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state.storage.cancel_large_file_scan(&scan_id)
}

/// Reveals a scanned large file in the platform file manager.
///
/// The interface submits only the item id: the path is resolved from the
/// backend-owned inventory, so the frontend never has to reassemble a path from
/// display fields (which on Windows produced mixed separators) and a stale id
/// cannot point at a path the scan did not review.
#[tauri::command]
#[specta::specta]
pub async fn reveal_large_file(
    item_id: String,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state.storage.reveal_large_file(&item_id).await
}

#[tauri::command]
#[specta::specta]
pub fn prepare_large_file_removal(
    scan_id: String,
    selected_item_ids: Vec<String>,
    state: State<'_, DesktopState>,
) -> Result<RemovalPlanPreview, String> {
    state
        .storage
        .prepare_large_file_removal(&scan_id, &selected_item_ids)
}

#[tauri::command]
#[specta::specta]
pub async fn pick_developer_workspace(
    state: State<'_, DesktopState>,
) -> Result<Option<DeveloperWorkspace>, String> {
    state.storage.pick_developer_workspace().await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_developer_workspaces(
    state: State<'_, DesktopState>,
) -> Result<DeveloperWorkspaceRestore, String> {
    state.storage.restore_developer_workspaces().await
}

#[tauri::command]
#[specta::specta]
pub async fn suggest_developer_workspaces(
    state: State<'_, DesktopState>,
) -> Result<Vec<DeveloperWorkspaceSuggestion>, String> {
    state.storage.suggest_developer_workspaces().await
}

#[tauri::command]
#[specta::specta]
pub async fn add_suggested_developer_workspace(
    path: String,
    state: State<'_, DesktopState>,
) -> Result<DeveloperWorkspace, String> {
    state.storage.add_suggested_developer_workspace(path).await
}

#[tauri::command]
#[specta::specta]
pub async fn register_developer_home_workspace(
    state: State<'_, DesktopState>,
) -> Result<DeveloperWorkspace, String> {
    state.storage.register_developer_home_workspace().await
}

#[tauri::command]
#[specta::specta]
pub async fn start_developer_artifact_scan(
    workspace_ids: Vec<String>,
    on_event: Channel<DeveloperArtifactScanEvent>,
    state: State<'_, DesktopState>,
) -> Result<DeveloperArtifactScanResult, String> {
    let progress: Arc<dyn DeveloperArtifactScanSink> =
        Arc::new(TauriDeveloperArtifactProgress::new(on_event));
    state
        .storage
        .scan_developer_artifacts(&workspace_ids, progress)
        .await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_developer_artifact_scan(
    scan_id: String,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    state.storage.cancel_developer_artifact_scan(&scan_id)
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_developer_artifact_cleanup(
    scan_id: String,
    selected_item_ids: Vec<String>,
    state: State<'_, DesktopState>,
) -> Result<RemovalPlanPreview, String> {
    state
        .storage
        .prepare_developer_artifact_removal(&scan_id, &selected_item_ids)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_installed_apps(
    state: State<'_, DesktopState>,
) -> Result<InstalledAppInventory, String> {
    state.storage.installed_apps().await
}

#[tauri::command]
#[specta::specta]
pub async fn get_app_leftovers(
    state: State<'_, DesktopState>,
) -> Result<AppLeftoverInventory, String> {
    state.storage.app_leftovers().await
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_app_uninstall(
    app_id: String,
    state: State<'_, DesktopState>,
) -> Result<AppUninstallInspection, String> {
    state.storage.inspect_app_uninstall(&app_id).await
}

#[tauri::command]
#[specta::specta]
pub fn prepare_app_uninstall(
    inspection_id: String,
    selected_related_ids: Vec<String>,
    state: State<'_, DesktopState>,
) -> Result<RemovalPlanPreview, String> {
    state
        .storage
        .prepare_app_uninstall(&inspection_id, &selected_related_ids)
}

#[tauri::command]
#[specta::specta]
pub async fn execute_removal_plan(
    plan_id: uuid::Uuid,
    state: State<'_, DesktopState>,
) -> Result<RemovalResult, String> {
    state.storage.execute_removal_plan(plan_id).await
}

/// Scans the saved project folders' repositories for linked worktrees (#505).
#[tauri::command]
#[specta::specta]
pub async fn scan_worktrees(
    state: State<'_, DesktopState>,
) -> Result<crate::models::WorktreeInventory, String> {
    state.worktrees.scan().await
}

/// Holds the selected finished worktrees as a one-shot removal plan.
#[tauri::command]
#[specta::specta]
pub async fn prepare_worktree_removal(
    scan_id: String,
    selected_item_ids: Vec<String>,
    state: State<'_, DesktopState>,
) -> Result<crate::models::WorktreeRemovalPreview, String> {
    state
        .worktrees
        .prepare_removal(&scan_id, &selected_item_ids)
}

/// Removes exactly the worktrees of a prepared plan, through Git.
#[tauri::command]
#[specta::specta]
pub async fn execute_worktree_removal(
    plan_id: uuid::Uuid,
    state: State<'_, DesktopState>,
) -> Result<crate::models::WorktreeRemovalResult, String> {
    state.worktrees.execute_removal(plan_id, |_, _, _| {}).await
}

/// Forgets Git's records of worktrees whose folders are gone (#523).
#[tauri::command]
#[specta::specta]
pub async fn forget_missing_worktrees(
    scan_id: String,
    state: State<'_, DesktopState>,
) -> Result<crate::models::WorktreeForgetResult, String> {
    state.worktrees.forget_missing(&scan_id).await
}
