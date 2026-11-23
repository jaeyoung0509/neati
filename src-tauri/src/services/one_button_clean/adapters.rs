//! Driven adapters: the use case's ports, served by the in-process services.
//!
//! Each adapter translates between the port vocabulary and one service. It
//! adds no rule: membership comes from `select_quick_clean_safe_candidates`
//! and from the developer-artifact preselection, and every removal still runs
//! through the service's own plan and execution guards.

use std::sync::Arc;

use crate::models::{
    CleanEvent, CleanStatus, CleanupProgressSink, DeveloperArtifactScanEvent, ScanEvent,
    ScanProgressSink, ScanRequest, ScanResult,
};
use crate::services::ports::{Activity, ActivityPort, PortFuture};
use crate::services::progress::DeveloperArtifactScanSink;
use crate::services::{CleanupService, SettingsAuthority, StorageService};

use super::ports::{CacheCleanup, ProjectCleanup, ProjectInventory, RemovalReport};
use super::selection::{category_label, kind_label};

/// [`CacheCleanup`] over [`CleanupService`].
pub struct ServiceCacheCleanup {
    cleanup: Arc<CleanupService>,
    settings: Arc<SettingsAuthority>,
    /// Why the catalog never loaded; an empty catalog must not scan clean.
    catalog_failure: Option<String>,
}

impl ServiceCacheCleanup {
    pub fn new(
        cleanup: Arc<CleanupService>,
        settings: Arc<SettingsAuthority>,
        catalog_failure: Option<String>,
    ) -> Self {
        Self {
            cleanup,
            settings,
            catalog_failure,
        }
    }
}

impl CacheCleanup for ServiceCacheCleanup {
    fn current(&self) -> Option<ScanResult> {
        let published = self.cleanup.get_last_scan()?.result;
        // The same window the reviewed quick-clean intent enforces, so a
        // reused scan is never one the execution would then refuse.
        published
            .validate_for_cleanup(&published.scan_id, crate::services::ports::unix_now())
            .is_ok()
            .then_some(published)
    }

    fn scan(&self, activity: Arc<dyn ActivityPort>) -> PortFuture<'_, Result<ScanResult, String>> {
        Box::pin(async move {
            if let Some(refusal) = &self.catalog_failure {
                return Err(refusal.clone());
            }
            let settings = self.settings.snapshot()?;
            let request = ScanRequest {
                categories: None,
                excluded_signatures: settings.excluded_signatures,
                intensive_cleanup: settings.intensive_cleanup,
            };
            let sink: Arc<dyn ScanProgressSink> = Arc::new(move |event: ScanEvent| match event {
                ScanEvent::RootStarted { name, .. } => {
                    activity.report(Activity::Scanning { location: name })
                }
                ScanEvent::CategoryStarted { category } => activity.report(Activity::Scanning {
                    location: category_label(category).to_string(),
                }),
                _ => {}
            });
            self.cleanup
                .start_scan_complete(request, sink)
                .await
                .map(|published| published.result)
        })
    }

    fn one_button_ids(&self, scan: &ScanResult) -> Result<Vec<String>, String> {
        let settings = self.settings.snapshot()?;
        Ok(crate::services::select_quick_clean_safe_candidates(
            scan, &settings,
        ))
    }

    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>> {
        Box::pin(async move {
            let settings = self.settings.snapshot()?;
            let sink: Arc<dyn CleanupProgressSink> = Arc::new(move |event: CleanEvent| {
                if let CleanEvent::ItemStarted {
                    name, index, total, ..
                } = event
                {
                    activity.report(Activity::Removing { name, index, total });
                }
            });
            let cleaned = self
                .cleanup
                .reviewed_quick_clean_safe(scan_id, ids, &settings, sink)
                .await
                .map_err(|failure| failure.to_string())?;
            let mut report = RemovalReport {
                reclaimed_bytes: cleaned.total_reclaimed_bytes,
                ..RemovalReport::default()
            };
            for item in cleaned.items {
                match item.status {
                    CleanStatus::Success | CleanStatus::Partial => {
                        report.removed.push(item.item_id)
                    }
                    CleanStatus::Skipped => report.skipped += 1,
                    CleanStatus::Failed => report.failed.push((
                        item.item_id,
                        item.error_message
                            .unwrap_or_else(|| "Not removed.".to_string()),
                    )),
                }
            }
            Ok(report)
        })
    }
}

/// [`ProjectCleanup`] over [`StorageService`].
pub struct ServiceProjectCleanup {
    storage: Arc<StorageService>,
    settings: Arc<SettingsAuthority>,
}

impl ServiceProjectCleanup {
    pub fn new(storage: Arc<StorageService>, settings: Arc<SettingsAuthority>) -> Self {
        Self { storage, settings }
    }
}

impl ProjectCleanup for ServiceProjectCleanup {
    fn current(&self) -> Option<ProjectInventory> {
        let folders = self.settings.snapshot().ok()?.project_folders;
        let result = self.storage.current_developer_artifacts()?;
        Some(ProjectInventory {
            folders,
            // The refusals belong to the restore that ran with that scan; a
            // reused inventory does not repeat them.
            refused: Vec::new(),
            result: Some(result),
        })
    }

    fn scan(
        &self,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<ProjectInventory, String>> {
        Box::pin(async move {
            let folders = self.settings.snapshot()?.project_folders;
            if folders.is_empty() {
                return Ok(ProjectInventory::default());
            }
            let restore = self.storage.restore_developer_workspaces().await?;
            let workspace_ids: Vec<String> = restore
                .workspaces
                .iter()
                .map(|workspace| workspace.id.clone())
                .collect();
            let result = if workspace_ids.is_empty() {
                None
            } else {
                let sink: Arc<dyn DeveloperArtifactScanSink> = Arc::new(ProjectActivity(activity));
                Some(
                    self.storage
                        .scan_developer_artifacts(&workspace_ids, sink)
                        .await?,
                )
            };
            Ok(ProjectInventory {
                folders,
                refused: restore.refused,
                result,
            })
        })
    }

    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>> {
        Box::pin(async move {
            activity.report(Activity::Removing {
                name: "project build output".to_string(),
                index: 0,
                total: ids.len(),
            });
            let plan = self
                .storage
                .prepare_developer_artifact_removal(&scan_id, &ids)
                .await?;
            let removed = self.storage.execute_removal_plan(plan.id).await?;
            let mut report = RemovalReport {
                reclaimed_bytes: removed.removed_allocated_size,
                skipped: removed.skipped_count as u64,
                ..RemovalReport::default()
            };
            for item in removed.items {
                if item.success {
                    report.removed.push(item.item_id);
                } else {
                    report.failed.push((item.item_id, item.message));
                }
            }
            Ok(report)
        })
    }
}

struct ProjectActivity(Arc<dyn ActivityPort>);

impl DeveloperArtifactScanSink for ProjectActivity {
    fn emit(&self, event: DeveloperArtifactScanEvent) {
        match event {
            DeveloperArtifactScanEvent::ProjectDiscovered { project_name, .. } => {
                self.0.report(Activity::Scanning {
                    location: project_name,
                })
            }
            DeveloperArtifactScanEvent::ArtifactMeasurementStarted {
                project_name, kind, ..
            } => self.0.report(Activity::Measuring {
                project: project_name,
                kind: kind_label(&kind),
            }),
            _ => {}
        }
    }
}

/// [`WorktreeCleanup`] over [`crate::services::WorktreeService`] (#505).
pub struct ServiceWorktreeCleanup {
    worktrees: Arc<crate::services::WorktreeService>,
}

impl ServiceWorktreeCleanup {
    pub fn new(worktrees: Arc<crate::services::WorktreeService>) -> Self {
        Self { worktrees }
    }
}

impl super::ports::WorktreeCleanup for ServiceWorktreeCleanup {
    fn rule_enabled(&self) -> bool {
        self.worktrees.rule_enabled()
    }

    fn current(&self) -> Option<crate::models::WorktreeInventory> {
        self.worktrees.current()
    }

    fn scan(
        &self,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<crate::models::WorktreeInventory, String>> {
        Box::pin(async move {
            activity.report(Activity::Scanning {
                location: "Git worktrees".to_string(),
            });
            self.worktrees.scan().await
        })
    }

    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>> {
        Box::pin(async move {
            let preview = self.worktrees.prepare_removal(&scan_id, &ids)?;
            let progress = activity.clone();
            let result = self
                .worktrees
                .execute_removal(preview.plan_id, move |name, index, total| {
                    progress.report(Activity::Removing {
                        name: name.to_string(),
                        index,
                        total,
                    });
                })
                .await?;
            Ok(RemovalReport {
                reclaimed_bytes: result.reclaimed_bytes,
                removed: result.removed,
                skipped: 0,
                failed: result
                    .failed
                    .into_iter()
                    .map(|failure| (failure.id, failure.message))
                    .collect(),
            })
        })
    }
}
