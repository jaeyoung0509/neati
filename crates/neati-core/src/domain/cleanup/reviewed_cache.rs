//! Reviewed owner stores. The core owns authorization; native evidence and
//! the final checked permanent removal are supplied by a narrow port.
use super::{
    CleanFailureReason, DeletionDisposition, OwnerProviderAuthorization, OwnerProviderExecution,
    OwnerProviderRefusal, OwnerProviderSelection, OwnerProviderUnit, OwnerStoreObservation,
    OwnerUnitObservation, OwnerUnitOutcome, OwnerUnitRefusal, OwnerUnitState, ProviderStatus,
    RunningProcessPolicy,
};
use crate::domain::{CleanupIdentity, RiskTier};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewedCacheKind {
    EditorOffline,
    NodeHeaders,
    ElectronArchives,
    ShellCompletions,
    GoogleUpdaterLogs,
    GoogleUpdaterDownloads,
    GradleMarkers,
    CodexStaging,
    UpdaterStaging,
    IdeIndexes,
    MailDownloads,
    MessagesPreviews,
    AbandonedDownloads,
    NodeCompileCache,
}
impl ReviewedCacheKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::EditorOffline => "editor.offline_cache_storage",
            Self::NodeHeaders => "node_gyp.headers",
            Self::ElectronArchives => "electron.download_archives",
            Self::ShellCompletions => "shell.completion_cache",
            Self::GoogleUpdaterLogs => "google_updater.logs",
            Self::GoogleUpdaterDownloads => "google_updater.downloads",
            Self::GradleMarkers => "gradle.notification_markers",
            Self::CodexStaging => "codex.abandoned_staging",
            Self::UpdaterStaging => "updater.unreferenced_staging",
            Self::IdeIndexes => "ide.disposable_indexes",
            Self::MailDownloads => "mail.download_copies",
            Self::MessagesPreviews => "messages.preview_images",
            Self::AbandonedDownloads => "downloads.abandoned",
            Self::NodeCompileCache => "node.temporary_compile_cache",
        }
    }
    pub fn requires_confirmation(self) -> bool {
        !matches!(
            self,
            Self::NodeHeaders
                | Self::ElectronArchives
                | Self::ShellCompletions
                | Self::GoogleUpdaterLogs
                | Self::GoogleUpdaterDownloads
                | Self::GradleMarkers
                | Self::NodeCompileCache
        )
    }
    pub fn consequence(self) -> &'static str {
        match self {
        Self::NodeCompileCache => "Verified Node module compilation caches are permanently removed. Node recompiles modules on a later run; source files, projects and installed runtimes stay intact.",
        Self::CodexStaging => "Abandoned runtime installation staging is permanently removed. An interrupted runtime download may restart; the activated runtime stays intact.",
        Self::GradleMarkers => "Gradle may show release highlights again. Build caches, dependencies and installed Gradle versions stay intact.",
        Self::GoogleUpdaterLogs => "User-level Google Updater logs are permanently removed. Previous update diagnostics will be unavailable; updater preferences, installed versions and update metadata remain intact.",
        Self::GoogleUpdaterDownloads => "Verified Google Updater download archives and their cache index are permanently removed together. Google Updater may download installers again; installed applications, updater versions and preferences stay intact.",
        Self::NodeHeaders => "Downloaded Node development headers are permanently removed and may be downloaded again during native addon builds. Installed Node runtimes stay intact.",
        Self::ElectronArchives => "Downloaded Electron ZIP archives are permanently removed and may be downloaded again. Installed Electron apps and browser bundles stay intact.",
        Self::ShellCompletions => "Shell completion dump files are permanently removed. Zsh rebuilds completion metadata on a later startup; shell settings stay intact.",
        Self::UpdaterStaging => "Unreferenced update archives are permanently removed and may be downloaded again. The installer referenced by update-info.json stays intact.",
        Self::EditorOffline => "Selected editor offline assets are permanently removed. Extensions and webviews may need a network connection to rebuild them. Settings, credentials, local storage, IndexedDB and Service Worker registrations stay intact.",
        Self::IdeIndexes => "Selected IDE indexes are permanently removed and rebuild when the IDE opens. Local History, plugins, settings and project files stay intact. This is not the IDE's full Invalidate Caches operation.",
        Self::MessagesPreviews => "Selected Messages preview and sticker images are permanently removed and may be generated again. Conversations, original attachments and databases stay intact.",
        Self::MailDownloads => "Selected downloaded attachment copies are permanently removed. Save any edits you want to keep first. Mail messages and its attachment store stay intact.",
        Self::AbandonedDownloads => "Selected incomplete downloads are permanently removed and cannot resume from this location. Review every file before continuing; completed downloads stay intact.",
    }
    }
}

pub trait ReviewedCachePort: Send + Sync {
    fn inventory(&self, kind: ReviewedCacheKind) -> OwnerStoreObservation;
    fn identity(&self, path: &Path) -> Option<CleanupIdentity>;
    /// Re-derive the registered scope, check owner/handles, tree, and identity,
    /// and permanently remove only that exact reviewed unit. Must not bypass owner validation.
    fn move_verified(
        &self,
        kind: ReviewedCacheKind,
        unit: &OwnerProviderUnit,
    ) -> Result<(), String>;
    fn is_absent(&self, path: &Path) -> bool;
}

pub fn prepare(
    kind: ReviewedCacheKind,
    port: &dyn ReviewedCachePort,
    selections: &[OwnerProviderSelection],
) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
    let inventory = port.inventory(kind);
    if !inventory.status.is_ready() {
        return Err(OwnerProviderRefusal::new(
            inventory.status,
            inventory
                .detail
                .unwrap_or_else(|| "Owner inventory is unavailable".into()),
        ));
    }
    let mut plan = OwnerProviderAuthorization {
        provider_id: kind.id().into(),
        signature_id: String::new(),
        risk: RiskTier::Rebuild,
        deletion_disposition: DeletionDisposition::PermanentDelete,
        process_guard: RunningProcessPolicy::none(),
        requires_confirmation: kind.requires_confirmation(),
        units: Vec::new(),
        refusals: Vec::new(),
    };
    for selection in selections {
        let found = inventory
            .units
            .iter()
            .find(|u| eligible_selection(u, selection));
        let identity = found.and_then(|u| port.identity(&u.path));
        if let (Some(unit), Some(identity)) = (found, identity) {
            if plan.units.iter().any(|u| u.path == unit.path) {
                continue;
            }
            plan.units.push(OwnerProviderUnit {
                item_id: selection.item_id.clone(),
                name: selection.name.clone(),
                unit_key: unit.unit_key.clone(),
                root: unit.path.parent().unwrap_or(&unit.path).into(),
                path: unit.path.clone(),
                identity,
                expected_bytes: unit.allocated_bytes,
                entry_count: unit.entry_count,
            });
        } else {
            plan.refusals.push(OwnerUnitRefusal {
                item_id: selection.item_id.clone(),
                item_name: selection.name.clone(),
                status: ProviderStatus::Blocked,
                reason: CleanFailureReason::ProviderRefused,
                detail: "The selected unit changed or is unavailable; scan again".into(),
            });
        }
    }
    if plan.units.is_empty() {
        return Err(OwnerProviderRefusal::for_selections(
            ProviderStatus::Blocked,
            "No reviewed unit is still eligible",
            plan.refusals,
        ));
    }
    Ok(plan)
}
fn eligible_selection(unit: &OwnerUnitObservation, selection: &OwnerProviderSelection) -> bool {
    unit.state == OwnerUnitState::Ready
        && unit.path == selection.path
        && unit.allocated_bytes == selection.expected_bytes
}

pub fn execute(
    kind: ReviewedCacheKind,
    port: &dyn ReviewedCachePort,
    plan: &OwnerProviderAuthorization,
) -> OwnerProviderExecution {
    let inventory = port.inventory(kind);
    let units = plan
        .units
        .iter()
        .map(|unit| {
            let refuse = |detail| {
                OwnerUnitOutcome::refused(
                    unit.item_id.clone(),
                    unit.unit_key.clone(),
                    ProviderStatus::Blocked,
                    detail,
                )
            };
            if plan.provider_id != kind.id()
                || plan.deletion_disposition != DeletionDisposition::PermanentDelete
                || plan.requires_confirmation != kind.requires_confirmation()
            {
                return refuse(
                    "The authorization does not match this reviewed owner operation".into(),
                );
            }
            let eligible = inventory.status.is_ready()
                && inventory.units.iter().any(|u| {
                    u.state == OwnerUnitState::Ready
                        && u.path == unit.path
                        && u.unit_key == unit.unit_key
                        && u.allocated_bytes == unit.expected_bytes
                });
            if !eligible || port.identity(&unit.path).as_ref() != Some(&unit.identity) {
                return refuse(
                    "The reviewed unit changed or its owner is active; scan again".into(),
                );
            }
            if let Err(detail) = port.move_verified(kind, unit) {
                return refuse(detail);
            }
            if port.is_absent(&unit.path) {
                OwnerUnitOutcome::cleaned(
                    unit.item_id.clone(),
                    unit.unit_key.clone(),
                    unit.expected_bytes,
                )
            } else {
                OwnerUnitOutcome::partially_cleaned(
                    unit.item_id.clone(),
                    unit.unit_key.clone(),
                    0,
                    None,
                    "The unit still exists after the permanent removal; a replacement was left intact",
                )
            }
        })
        .collect();
    OwnerProviderExecution { units }
}
