//! Finished worktrees as an application service (#505).
//!
//! The service owns the published scan, the one-shot removal plans, and the
//! storage gate around both: a command or a use-case adapter hands it a scan
//! id, item ids or a plan id and never a path.

use std::sync::{Arc, Mutex};

use neati_platform::PlatformEnvironment;
use uuid::Uuid;

use crate::developer_artifacts::preselection::{ActivityProbe, UseProbe, WorkingDirectories};
use crate::execution_budget::ExecutionBudgets;
use crate::models::{
    WorktreeForgetResult, WorktreeInventory, WorktreeRemovalFailure, WorktreeRemovalPreview,
    WorktreeRemovalResult, WorktreeState,
};
use crate::operation_gate::StorageOperationGate;
use crate::services::{OneShotPlan, PlanLifecycle, PlanStore, SettingsAuthority};
use crate::worktrees::{Observation, WorktreeRecord, WorktreeScan};

/// The records a person approved, removed once.
#[derive(Debug, Clone)]
pub struct WorktreeRemovalPlan {
    id: Uuid,
    created_at: u64,
    records: Vec<WorktreeRecord>,
}

impl OneShotPlan for WorktreeRemovalPlan {
    fn plan_id(&self) -> Uuid {
        self.id
    }

    fn created_at(&self) -> u64 {
        self.created_at
    }
}

fn now() -> u64 {
    crate::services::ports::unix_now()
}

pub struct WorktreeService {
    environment: Arc<PlatformEnvironment>,
    settings: Arc<SettingsAuthority>,
    operation_gate: StorageOperationGate,
    budgets: Arc<ExecutionBudgets>,
    current: Arc<Mutex<Option<WorktreeScan>>>,
    plans: Arc<PlanStore<WorktreeRemovalPlan>>,
}

/// Runs `work` with the native observations and the person's rules.
fn with_native_observation<T>(
    environment: &PlatformEnvironment,
    rules: crate::models::CleanupRules,
    work: impl FnOnce(&Observation<'_>) -> T,
) -> T {
    let activity = |path: &std::path::Path| {
        crate::developer_artifacts::project_last_activity(environment, path)
    };
    let activity: &ActivityProbe<'_> = &activity;
    let use_probe: &UseProbe<'_> = &neati_platform::open_files::observe_open_files_batch;
    let working: &WorkingDirectories<'_> = &crate::developer_artifacts::running_working_directories;
    work(&Observation {
        now: now(),
        rules,
        use_probe,
        activity,
        working_directories: working,
    })
}

impl WorktreeService {
    pub fn new(
        environment: Arc<PlatformEnvironment>,
        settings: Arc<SettingsAuthority>,
        operation_gate: StorageOperationGate,
        budgets: Arc<ExecutionBudgets>,
    ) -> Self {
        Self {
            environment,
            settings,
            operation_gate,
            budgets,
            current: Arc::new(Mutex::new(None)),
            plans: Arc::new(PlanStore::new(PlanLifecycle::worktrees())),
        }
    }

    /// Scans the saved project folders' repositories and publishes the result.
    pub async fn scan(&self) -> Result<WorktreeInventory, String> {
        let settings = self.settings.snapshot()?;
        let environment = self.environment.clone();
        let gate = self.operation_gate.clone();
        let current = self.current.clone();
        let permit = self.budgets.acquire_storage_read().await?;
        crate::blocking::run_blocking(
            move || -> Result<WorktreeInventory, String> {
                let _permit = permit;
                let scan = gate.run_read(|| {
                    with_native_observation(&environment, settings.cleanup_rules, |observation| {
                        crate::worktrees::scan(
                            &environment,
                            &settings.project_folders,
                            observation,
                            &|| false,
                        )
                    })
                });
                let inventory = scan.inventory.clone();
                *current
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(scan);
                Ok(inventory)
            },
            "Worktree scan panicked",
        )
        .await
    }

    /// Whether the person's worktree rule puts finished worktrees in Clean.
    pub fn rule_enabled(&self) -> bool {
        self.settings
            .snapshot()
            .map(|settings| settings.cleanup_rules.worktrees.enabled)
            .unwrap_or(false)
    }

    /// The published scan, while it is recent and decided with today's rules.
    pub fn current(&self) -> Option<WorktreeInventory> {
        let rules = self.settings.snapshot().ok()?.cleanup_rules;
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .filter(|scan| scan.is_current_for(&rules, now()))
            .map(|scan| scan.inventory.clone())
    }

    /// Holds the selected finished worktrees of scan `scan_id` as a one-shot
    /// plan. Only `Finished` items may be selected.
    pub fn prepare_removal(
        &self,
        scan_id: &str,
        ids: &[String],
    ) -> Result<WorktreeRemovalPreview, String> {
        if ids.is_empty() {
            return Err("Select at least one finished worktree.".into());
        }
        let guard = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let scan = guard
            .as_ref()
            .filter(|scan| scan.inventory.scan_id == scan_id)
            .ok_or_else(|| {
                "These worktrees were found by an older scan. Scan again.".to_string()
            })?;
        let mut records = Vec::new();
        for id in ids {
            let record = scan.records.get(id).ok_or_else(|| {
                "A selected worktree is not in this scan. Scan again.".to_string()
            })?;
            if !record.item.state.is_removable() {
                return Err(format!(
                    "{} is not finished, so it cannot be removed.",
                    record.item.name
                ));
            }
            if !records
                .iter()
                .any(|known: &WorktreeRecord| known.item.id == record.item.id)
            {
                records.push(record.clone());
            }
        }
        drop(guard);
        let plan = WorktreeRemovalPlan {
            id: Uuid::new_v4(),
            created_at: now(),
            records,
        };
        let preview = WorktreeRemovalPreview {
            plan_id: plan.id,
            items: plan
                .records
                .iter()
                .map(|record| record.item.clone())
                .collect(),
            total_bytes: plan
                .records
                .iter()
                .map(|record| record.item.allocated_bytes)
                .sum(),
            expires_in_seconds: self.plans.ttl_seconds(),
        };
        self.plans
            .insert(plan, now())
            .map_err(|error| error.to_string())?;
        Ok(preview)
    }

    /// Removes exactly the worktrees of plan `plan_id`, once, each after its
    /// own revalidation.
    pub async fn execute_removal(
        &self,
        plan_id: Uuid,
        on_progress: impl Fn(&str, usize, usize) + Send + 'static,
    ) -> Result<WorktreeRemovalResult, String> {
        let plans = self.plans.clone();
        let rules = self.settings.snapshot()?.cleanup_rules;
        let environment = self.environment.clone();
        let gate = self.operation_gate.clone();
        let current = self.current.clone();
        crate::blocking::run_blocking(
            move || -> Result<WorktreeRemovalResult, String> {
                gate.run_write(|| {
                    let plan = plans
                        .take_valid(plan_id, now())
                        .map_err(|error| error.to_string())?;
                    let mut result = WorktreeRemovalResult::default();
                    let total = plan.records.len();
                    with_native_observation(&environment, rules, |observation| {
                        for (index, record) in plan.records.iter().enumerate() {
                            on_progress(&record.item.name, index, total);
                            match crate::worktrees::removal::remove(
                                &environment,
                                record,
                                observation,
                            ) {
                                Ok(bytes) => {
                                    result.reclaimed_bytes =
                                        result.reclaimed_bytes.saturating_add(bytes);
                                    result.removed.push(record.item.id.clone());
                                }
                                Err(message) => result.failed.push(WorktreeRemovalFailure {
                                    id: record.item.id.clone(),
                                    name: record.item.name.clone(),
                                    message,
                                }),
                            }
                        }
                    });
                    // The published scan no longer describes the disk.
                    *current
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                    Ok(result)
                })
            },
            "Worktree removal panicked",
        )
        .await
    }

    /// Forgets Git's records of the gone worktrees scan `scan_id` reported,
    /// one `git worktree prune` per repository that has any (#523).
    pub async fn forget_missing(&self, scan_id: &str) -> Result<WorktreeForgetResult, String> {
        let repositories: Vec<std::path::PathBuf> = {
            let guard = self
                .current
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let scan = guard
                .as_ref()
                .filter(|scan| scan.inventory.scan_id == scan_id)
                .ok_or_else(|| {
                    "These worktrees were found by an older scan. Scan again.".to_string()
                })?;
            let mut repositories: Vec<std::path::PathBuf> = scan
                .records
                .values()
                .filter(|record| record.item.state == WorktreeState::Missing)
                .map(|record| record.repository.clone())
                .collect();
            repositories.sort();
            repositories.dedup();
            repositories
        };
        let environment = self.environment.clone();
        let gate = self.operation_gate.clone();
        let current = self.current.clone();
        crate::blocking::run_blocking(
            move || -> Result<WorktreeForgetResult, String> {
                gate.run_write(|| {
                    let mut result = WorktreeForgetResult::default();
                    for repository in &repositories {
                        let forgotten = crate::worktrees::forget_missing(&environment, repository)?;
                        if forgotten > 0 {
                            result.repositories += 1;
                            result.forgotten += forgotten;
                        }
                    }
                    *current
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
                    Ok(result)
                })
            },
            "Worktree record pruning panicked",
        )
        .await
    }
}
