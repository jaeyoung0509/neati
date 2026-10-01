//! Dedicated explicit temporary review. Its private drafts cannot authorize
//! ordinary cleanup, Developer Artifacts, arbitrary paths or permission changes.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::progress::TemporaryStorageSink;
use super::{CancellationRegistry, OneShotPlan, PlanLifecycle, PlanStore};
use crate::execution_budget::ExecutionBudgets;
use crate::models::{
    DeveloperArtifactKind, PlatformKind, ReviewedFileIdentity, TrashItemResult, TrashResult,
};
use crate::operation_gate::StorageOperationGate;
use neati_core::application::dto::temporary_storage::*;
use neati_core::domain::storage::{
    require_temporary_consent, TemporaryRemovalMode, TemporaryReviewConsent, TemporaryUsageState,
};
use neati_platform::temporary_storage::{self as native, TemporaryUnitSnapshot};
use neati_platform::PlatformEnvironment;

const INVENTORY_TTL: u64 = 900;
const SCAN_BUDGET: Duration = Duration::from_secs(90);
const REVIEW_WORK_BUDGET: Duration = Duration::from_secs(90);

struct ExecutionAdmission {
    id: uuid::Uuid,
    active: Arc<Mutex<HashSet<uuid::Uuid>>>,
    signals: Arc<CancellationRegistry>,
    cancel: Arc<AtomicBool>,
}
impl Drop for ExecutionAdmission {
    fn drop(&mut self) {
        self.signals.remove(&self.id.to_string());
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

#[derive(Clone)]
struct GeneratedContext {
    project: PathBuf,
    project_identity: ReviewedFileIdentity,
    kind: DeveloperArtifactKind,
    markers: Vec<(PathBuf, ReviewedFileIdentity)>,
}

#[derive(Clone)]
struct ReviewedUnit {
    snapshot: TemporaryUnitSnapshot,
    option: TemporaryRemovalOption,
    usage: TemporaryUsageObservation,
    generated: Option<GeneratedContext>,
}

struct Inventory {
    scan_id: String,
    created_at: u64,
    inserted_at: Instant,
    cancelled: bool,
    units: HashMap<String, ReviewedUnit>,
}

/// A draft is not mutation authority. Execution consumes it once and creates
/// `AcceptedTemporaryPlan` only after validating the exact preview's consent.
struct TemporaryReviewDraft {
    id: uuid::Uuid,
    created_at: u64,
    units: Vec<ReviewedUnit>,
}
struct AcceptedTemporaryPlan {
    units: Vec<ReviewedUnit>,
    accept_unknown_usage: bool,
}
impl TemporaryReviewDraft {
    fn accept(self, consent: TemporaryReviewConsent) -> Result<AcceptedTemporaryPlan, String> {
        require_temporary_consent(
            consent,
            self.units
                .iter()
                .any(|unit| unit.usage.state == TemporaryUsageState::UnableToDetermine),
            self.units
                .iter()
                .any(|unit| unit.option.mode == TemporaryRemovalMode::WholeFolder),
        )
        .map_err(str::to_string)?;
        Ok(AcceptedTemporaryPlan {
            units: self.units,
            accept_unknown_usage: consent.accept_unknown_usage,
        })
    }
}
impl OneShotPlan for TemporaryReviewDraft {
    fn plan_id(&self) -> uuid::Uuid {
        self.id
    }
    fn created_at(&self) -> u64 {
        self.created_at
    }
}

trait TemporaryUsePort: Send + Sync {
    fn observe(
        &self,
        environment: &PlatformEnvironment,
        path: &std::path::Path,
    ) -> TemporaryUsageObservation;
    fn move_unit(
        &self,
        environment: &PlatformEnvironment,
        reviewed: &TemporaryUnitSnapshot,
        accept_unknown: bool,
        final_owner_scope: Option<&std::path::Path>,
        trash: &dyn neati_platform::TrashBackend,
    ) -> Result<(), String>;
}
struct NativeTemporaryUse;
impl TemporaryUsePort for NativeTemporaryUse {
    fn observe(
        &self,
        environment: &PlatformEnvironment,
        path: &std::path::Path,
    ) -> TemporaryUsageObservation {
        native::observe_temporary_use(environment, path)
    }
    fn move_unit(
        &self,
        environment: &PlatformEnvironment,
        reviewed: &TemporaryUnitSnapshot,
        accept_unknown: bool,
        final_owner_scope: Option<&std::path::Path>,
        trash: &dyn neati_platform::TrashBackend,
    ) -> Result<(), String> {
        native::move_reviewed_unit(
            environment,
            reviewed,
            accept_unknown,
            final_owner_scope,
            trash,
        )
    }
}

pub(crate) struct TemporaryStorageService {
    environment: Arc<PlatformEnvironment>,
    gate: StorageOperationGate,
    budgets: Arc<ExecutionBudgets>,
    trash: Arc<dyn neati_platform::TrashBackend>,
    use_port: Arc<dyn TemporaryUsePort>,
    inventory: Arc<Mutex<Option<Inventory>>>,
    plans: Arc<PlanStore<TemporaryReviewDraft>>,
    scans: Arc<CancellationRegistry>,
    executions: Arc<CancellationRegistry>,
    active_executions: Arc<Mutex<HashSet<uuid::Uuid>>>,
    generation: Arc<AtomicU64>,
}

impl TemporaryStorageService {
    pub(crate) fn new(
        environment: Arc<PlatformEnvironment>,
        gate: StorageOperationGate,
        budgets: Arc<ExecutionBudgets>,
        trash: Arc<dyn neati_platform::TrashBackend>,
    ) -> Self {
        Self {
            environment,
            gate,
            budgets,
            trash,
            use_port: Arc::new(NativeTemporaryUse),
            inventory: Arc::new(Mutex::new(None)),
            plans: Arc::new(PlanStore::new(PlanLifecycle::trash())),
            scans: Arc::new(CancellationRegistry::for_scans()),
            executions: Arc::new(CancellationRegistry::for_scans()),
            active_executions: Arc::new(Mutex::new(HashSet::new())),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) async fn scan(
        &self,
        sink: Arc<dyn TemporaryStorageSink>,
    ) -> Result<TemporaryStorageInventory, String> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.scans.request_all();
        *self.inventory.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let permit = self.budgets.acquire_storage_read().await?;
        let scan_id = uuid::Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.scans.register(scan_id.clone(), cancel.clone());
        sink.emit(TemporaryStorageEvent::Started {
            scan_id: scan_id.clone(),
        });
        let environment = self.environment.clone();
        let gate = self.gate.clone();
        let inventory_store = self.inventory.clone();
        let use_port = self.use_port.clone();
        let generations = self.generation.clone();
        let worker_id = scan_id.clone();
        let outcome = crate::blocking::run_blocking(
            move || {
                let _permit = permit;
                gate.run_read(|| {
                    let (result, inventory) = scan_inventory(
                        &environment,
                        &worker_id,
                        &cancel,
                        use_port.as_ref(),
                        sink.as_ref(),
                    );
                    if generations.load(Ordering::SeqCst) != generation {
                        return Err("A newer temporary review superseded this scan.".into());
                    }
                    *inventory_store.lock().unwrap_or_else(|p| p.into_inner()) = Some(inventory);
                    sink.emit(TemporaryStorageEvent::Finished {
                        inventory: result.clone(),
                    });
                    Ok(result)
                })
            },
            "Temporary storage scan worker panicked",
        )
        .await;
        self.scans.remove(&scan_id);
        outcome
    }

    pub(crate) fn cancel_scan(&self, scan_id: &str) -> Result<(), String> {
        if self.scans.request(scan_id) {
            Ok(())
        } else {
            Err("The temporary scan is no longer running.".into())
        }
    }

    pub(crate) async fn prepare(
        &self,
        scan_id: &str,
        selected_ids: &[String],
    ) -> Result<TemporaryReviewPreview, String> {
        let units = {
            let inventory = self.inventory.lock().unwrap_or_else(|p| p.into_inner());
            let inventory = inventory
                .as_ref()
                .filter(|inventory| {
                    inventory.scan_id == scan_id
                        && !inventory.cancelled
                        && inventory.inserted_at.elapsed() < Duration::from_secs(INVENTORY_TTL)
                        && neati_core::domain::is_within_window(
                            inventory.created_at,
                            native::unix_timestamp(),
                            INVENTORY_TTL,
                        )
                })
                .ok_or("The temporary inventory expired, was cancelled or changed. Scan again.")?;
            selected_units(inventory, selected_ids)?
        };
        let environment = self.environment.clone();
        let gate = self.gate.clone();
        let use_port = self.use_port.clone();
        let permit = self.budgets.acquire_storage_read().await?;
        let units = crate::blocking::run_blocking(
            move || {
                let _permit = permit;
            gate.run_read(|| {
                let started = Instant::now();
                units
                        .into_iter()
                    .map(|mut unit| {
                        if started.elapsed() >= REVIEW_WORK_BUDGET { return Err("The bounded review planning budget was reached. Select fewer scopes and try again.".into()); }
                            validate_unit(&environment, &unit, &AtomicBool::new(false))?;
                            unit.usage = use_port.observe(&environment, usage_scope(&unit));
                            require_usage(&unit, true)?;
                            validate_unit(&environment, &unit, &AtomicBool::new(false))?;
                            Ok(unit)
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
            },
            "Temporary review planning worker panicked",
        )
        .await?;
        let now = native::unix_timestamp();
        let preview = TemporaryReviewPreview {
            id: uuid::Uuid::new_v4(), selected: units.iter().map(|unit| unit.option.clone()).collect(),
            known_allocated_bytes: units.iter().filter_map(|unit| unit.option.allocated_bytes).fold(0u64, u64::saturating_add),
            unknown_estimates: units.iter().filter(|unit| unit.option.allocated_bytes.is_none() || unit.option.partial).count(),
            has_unknown_usage: units.iter().any(|unit| unit.usage.state == TemporaryUsageState::UnableToDetermine),
            has_whole_folders: units.iter().any(|unit| unit.option.mode == TemporaryRemovalMode::WholeFolder),
            expires_at: now.saturating_add(self.plans.ttl_seconds()),
            warnings: vec!["No use detected does not establish abandonment or recoverability.".into(),
                "Trash does not free disk space immediately. Restore staged items manually to the listed original paths; Git worktree registrations may need repair.".into(),
                "Allocation estimates stay separate from cache totals; distinct storage and free-space recovery are unverified.".into()],
        };
        self.plans
            .insert(
                TemporaryReviewDraft {
                    id: preview.id,
                    created_at: now,
                    units,
                },
                now,
            )
            .map_err(|e| e.to_string())?;
        Ok(preview)
    }

    pub(crate) async fn execute(
        &self,
        id: uuid::Uuid,
        consent: TemporaryReviewConsent,
    ) -> Result<TrashResult, String> {
        let admission = self.admit_execution(id)?;
        let permit = self.budgets.acquire_storage_read().await?;
        let plans = self.plans.clone();
        let environment = self.environment.clone();
        let gate = self.gate.clone();
        let trash = self.trash.clone();
        let use_port = self.use_port.clone();
        let cancel = admission.cancel.clone();
        let outcome = crate::blocking::run_blocking(
            move || {
                let _permit = permit;
                // The worker owns admission even if its waiting future drops.
                // A duplicate cannot replace this operation's Stop signal.
                let _admission = admission;
                gate.run_write(|| {
                    // Check expiration after waiting for the write gate, consume
                    // on every outcome, and bind consent to this exact draft.
                    let draft = plans
                        .take_valid(id, native::unix_timestamp())
                        .map_err(|e| e.to_string())?;
                    let accepted = draft.accept(consent)?;
                    Ok(execute_accepted(
                        &environment,
                        accepted,
                        &cancel,
                        use_port.as_ref(),
                        trash.as_ref(),
                    ))
                })
            },
            "Temporary review execution worker panicked",
        )
        .await;
        outcome
    }

    fn admit_execution(&self, id: uuid::Uuid) -> Result<ExecutionAdmission, String> {
        let mut active = self
            .active_executions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if active.contains(&id) {
            return Err("This temporary review is already executing.".into());
        }
        if active.len() >= super::cancellation::DEFAULT_MAX_ACTIVE_CANCELLATIONS {
            return Err("Temporary execution admission is at its bounded capacity.".into());
        }
        active.insert(id);
        let cancel = Arc::new(AtomicBool::new(false));
        self.executions.register(id.to_string(), cancel.clone());
        Ok(ExecutionAdmission {
            id,
            active: self.active_executions.clone(),
            signals: self.executions.clone(),
            cancel,
        })
    }

    pub(crate) fn cancel_execution(&self, id: uuid::Uuid) -> Result<(), String> {
        if self.executions.request(&id.to_string()) {
            let _ = self.plans.take_valid(id, native::unix_timestamp());
            return Ok(());
        }
        // Cancel also consumes a prepared draft or an execution still queued
        // for a budget. There is no dormant authority after a dismissed review.
        self.plans
            .take_valid(id, native::unix_timestamp())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn selected_units(inventory: &Inventory, ids: &[String]) -> Result<Vec<ReviewedUnit>, String> {
    if ids.is_empty() || ids.len() > native::MAX_TEMPORARY_UNITS {
        return Err("Select a bounded set of temporary scopes first.".into());
    }
    let mut seen = HashSet::new();
    let mut units: Vec<ReviewedUnit> = vec![];
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let unit = inventory
            .units
            .get(id)
            .ok_or("The temporary selection changed. Scan again.")?;
        if let Some(reason) = &unit.option.blocked_reason {
            return Err(reason.clone());
        }
        if units.iter().any(|other| {
            unit.snapshot.path.starts_with(&other.snapshot.path)
                || other.snapshot.path.starts_with(&unit.snapshot.path)
        }) {
            return Err(
                "A folder and its generated subtree overlap. Choose one scope per folder.".into(),
            );
        }
        units.push(unit.clone());
    }
    Ok(units)
}

fn usage_scope(unit: &ReviewedUnit) -> &std::path::Path {
    unit.generated
        .as_ref()
        .map_or(unit.snapshot.path.as_path(), |generated| {
            generated.project.as_path()
        })
}

fn require_usage(unit: &ReviewedUnit, accept_unknown: bool) -> Result<(), String> {
    match unit.usage.state {
        TemporaryUsageState::InUse => Err("Active use was detected. Stop the owner and scan again.".into()),
        TemporaryUsageState::UnableToDetermine if unit.generated.is_some() || !accept_unknown => Err("Generated-only cleanup requires complete use observations. Review the whole temporary folder separately or scan again.".into()),
        _ => Ok(()),
    }
}

fn validate_unit(
    environment: &PlatformEnvironment,
    unit: &ReviewedUnit,
    cancel: &AtomicBool,
) -> Result<(), String> {
    native::recheck_temporary_unit(environment, &unit.snapshot, cancel)?;
    if let Some(context) = &unit.generated {
        let project_identity = crate::large_files::identity_from_path(&context.project)
            .ok_or("The generated artifact project changed")?;
        if !project_identity.same_entity(&context.project_identity) {
            return Err("The generated artifact project changed.".into());
        }
        let (kind, markers) = crate::developer_artifacts::temporary_generated_evidence(
            &context.project,
            &unit.snapshot.path,
        )
        .ok_or("The generated artifact contract changed")?;
        if kind != context.kind
            || markers.len() != context.markers.len()
            || context.markers.iter().any(|(path, identity)| {
                !markers.contains(path)
                    || crate::large_files::identity_from_path(path).as_ref() != Some(identity)
            })
        {
            return Err("Generated artifact markers changed. Scan again.".into());
        }
        let ownership = neati_platform::artifact_ownership::probe_artifact_ownership(
            environment,
            &context.project,
            &unit.snapshot.path,
            cancel,
        );
        if !ownership.allows_cleanup() {
            return Err(ownership
                .refusal_message()
                .unwrap_or("Generated ownership is incomplete")
                .into());
        }
        native::recheck_temporary_unit(environment, &unit.snapshot, cancel)?;
    }
    Ok(())
}

fn execute_accepted(
    environment: &PlatformEnvironment,
    plan: AcceptedTemporaryPlan,
    cancel: &AtomicBool,
    use_port: &dyn TemporaryUsePort,
    trash: &dyn neati_platform::TrashBackend,
) -> TrashResult {
    let started = Instant::now();
    let mut result = TrashResult {
        moved_count: 0,
        failed_count: 0,
        skipped_count: 0,
        moved_allocated_size: 0,
        items: vec![],
        size_is_lower_bound: false,
    };
    for mut unit in plan.units {
        let ready = (|| -> Result<(), String> {
            if started.elapsed() >= REVIEW_WORK_BUDGET {
                return Err("Stopped at the bounded execution budget; this unit was kept. Scan again to review the remainder.".into());
            }
            if cancel.load(Ordering::SeqCst) {
                return Err("Stopped before this unit; it was kept.".into());
            }
            validate_unit(environment, &unit, cancel)?;
            unit.usage = use_port.observe(environment, usage_scope(&unit));
            require_usage(&unit, plan.accept_unknown_usage)?;
            validate_unit(environment, &unit, cancel)?;
            if cancel.load(Ordering::SeqCst) {
                return Err("Stopped before this unit; it was kept.".into());
            }
            Ok(())
        })();
        let attempted = ready.is_ok();
        let outcome = ready.and_then(|_| {
            use_port.move_unit(
                environment,
                &unit.snapshot,
                plan.accept_unknown_usage && unit.generated.is_none(),
                unit.generated
                    .as_ref()
                    .map(|generated| generated.project.as_path()),
                trash,
            )
        });
        match outcome {
            Ok(()) => {
                result.moved_count += 1;
                result.moved_allocated_size = result
                    .moved_allocated_size
                    .saturating_add(unit.option.allocated_bytes.unwrap_or(0));
                result.size_is_lower_bound |= unit.option.partial;
                result.items.push(TrashItemResult {
                    item_id: unit.option.id,
                    success: true,
                    message: format!(
                        "Moved {} to Trash; disk-space recovery was not established.",
                        unit.option.path
                    ),
                });
            }
            Err(message) => {
                if attempted {
                    result.failed_count += 1;
                } else {
                    result.skipped_count += 1;
                }
                result.items.push(TrashItemResult {
                    item_id: unit.option.id,
                    success: false,
                    message,
                });
            }
        }
    }
    result
}

fn scan_inventory(
    environment: &PlatformEnvironment,
    scan_id: &str,
    cancel: &AtomicBool,
    use_port: &dyn TemporaryUsePort,
    sink: &dyn TemporaryStorageSink,
) -> (TemporaryStorageInventory, Inventory) {
    let now = native::unix_timestamp();
    let started = Instant::now();
    let available = environment.platform() == PlatformKind::Macos;
    let mut result = TemporaryStorageInventory { scan_id: scan_id.into(), roots: environment.temporary_roots().iter().map(|path| path.to_string_lossy().into()).collect(),
        items: vec![], observed_at: now, expires_at: now.saturating_add(INVENTORY_TTL), observed_allocated_bytes: 0, unknown_estimates: 0,
        partial: false, physical_overlap: false, cancelled: false, available, unavailable_reason: (!available).then(|| "Temporary review has a tested macOS adapter only; this platform is unavailable.".into()),
        notes: vec!["Direct temporary units are reviewed separately from Quick Clean. No prefix proves generated content or abandoned work.".into()] };
    let mut units = HashMap::new();
    if available {
        let (candidates, notes, partial) = native::enumerate_temporary_units(environment);
        result.notes.extend(notes);
        result.partial |= partial;
        for (root, path) in candidates {
            if cancel.load(Ordering::SeqCst) {
                result.cancelled = true;
                result.partial = true;
                break;
            }
            let budget_exhausted = started.elapsed() >= SCAN_BUDGET;
            let budget_cancel = AtomicBool::new(budget_exhausted);
            let snapshot = native::snapshot_temporary_unit(
                environment,
                &root,
                &path,
                if budget_exhausted {
                    &budget_cancel
                } else {
                    cancel
                },
            );
            // Root aliases are deduplicated by the platform observer. Separate
            // hard-link entries remain distinct reviewed path operations.
            let usage = if budget_exhausted {
                native::unknown_usage("The whole-inventory time budget was reached.")
            } else {
                use_port.observe(environment, &path)
            };
            let id = uuid::Uuid::new_v4().to_string();
            let option_id = uuid::Uuid::new_v4().to_string();
            let blocked = snapshot.blocked_reason.clone().or_else(|| {
                (usage.state == TemporaryUsageState::InUse)
                    .then(|| "Active use detected. Stop the owner and scan again.".into())
            });
            let option = TemporaryRemovalOption {
                id: option_id.clone(),
                mode: TemporaryRemovalMode::WholeFolder,
                path: path.to_string_lossy().into(),
                allocated_bytes: snapshot.allocated_bytes,
                partial: snapshot.partial,
                blocked_reason: blocked,
            };
            let mut options = vec![option.clone()];
            units.insert(
                option_id,
                ReviewedUnit {
                    snapshot: snapshot.clone(),
                    option,
                    usage: usage.clone(),
                    generated: None,
                },
            );
            // Exact project-backed subtrees only. The parent unit is still the
            // sole contribution to observed totals, regardless of option count.
            if !budget_exhausted && snapshot.blocked_reason.is_none() {
                for name in ["target", "node_modules", ".venv", "build"] {
                    let candidate = path.join(name);
                    if cancel.load(Ordering::SeqCst) || started.elapsed() >= SCAN_BUDGET {
                        break;
                    }
                    if !std::fs::symlink_metadata(&candidate)
                        .ok()
                        .is_some_and(|metadata| metadata.is_dir())
                    {
                        continue;
                    }
                    let Some((kind, markers)) =
                        crate::developer_artifacts::temporary_generated_evidence(&path, &candidate)
                    else {
                        continue;
                    };
                    let Some(project_identity) = crate::large_files::identity_from_path(&path)
                    else {
                        continue;
                    };
                    let marker_identities = markers
                        .iter()
                        .filter_map(|marker| {
                            crate::large_files::identity_from_path(marker)
                                .map(|identity| (marker.clone(), identity))
                        })
                        .collect::<Vec<_>>();
                    if marker_identities.len() != markers.len() {
                        continue;
                    }
                    let generated_snapshot =
                        native::snapshot_temporary_unit(environment, &root, &candidate, cancel);
                    let context = GeneratedContext {
                        project: path.clone(),
                        project_identity,
                        kind,
                        markers: marker_identities,
                    };
                    let generated_id = uuid::Uuid::new_v4().to_string();
                    let mut generated_unit = ReviewedUnit {
                        snapshot: generated_snapshot.clone(),
                        option: TemporaryRemovalOption {
                            id: generated_id.clone(),
                            mode: TemporaryRemovalMode::GeneratedSubtree,
                            path: candidate.to_string_lossy().into(),
                            allocated_bytes: generated_snapshot.allocated_bytes,
                            partial: generated_snapshot.partial,
                            blocked_reason: None,
                        },
                        usage: usage.clone(),
                        generated: Some(context),
                    };
                    generated_unit.option.blocked_reason =
                        validate_unit(environment, &generated_unit, cancel)
                            .err()
                            .or_else(|| require_usage(&generated_unit, false).err());
                    options.push(generated_unit.option.clone());
                    units.insert(generated_id, generated_unit);
                }
            }
            let item = TemporaryStorageItem {
                id,
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                path: path.to_string_lossy().into(),
                logical_bytes: snapshot.logical_bytes,
                allocated_bytes: snapshot.allocated_bytes,
                newest_activity: snapshot.newest_activity,
                partial: snapshot.partial,
                physical_overlap: snapshot.physical_overlap,
                contents: snapshot.contents.clone(),
                usage,
                options,
                selected_by_default: false,
            };
            result.physical_overlap |= item.physical_overlap;
            result.observed_allocated_bytes = result
                .observed_allocated_bytes
                .saturating_add(item.allocated_bytes.unwrap_or(0));
            result.unknown_estimates += usize::from(item.allocated_bytes.is_none() || item.partial);
            result.partial |= item.partial;
            sink.emit(TemporaryStorageEvent::ItemFound { item: item.clone() });
            result.items.push(item);
        }
    }
    result
        .items
        .sort_by_key(|item| std::cmp::Reverse(item.allocated_bytes.unwrap_or(0)));
    if result.partial {
        result.notes.push("Partial or unknown sizes are qualified; they are not zero or guaranteed reclaimable bytes.".into());
    }
    if result.physical_overlap {
        result.notes.push("Hard-linked storage may overlap across temporary units. Measured allocation entries do not establish distinct storage; each selected path keeps its own operation.".into());
    }
    let inventory = Inventory {
        scan_id: scan_id.into(),
        created_at: now,
        inserted_at: Instant::now(),
        cancelled: result.cancelled,
        units,
    };
    (result, inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::path::Path;

    #[cfg(unix)]
    struct FixtureUse {
        state: Mutex<TemporaryUsageState>,
        trash: PathBuf,
        moves: Mutex<Vec<(PathBuf, bool)>>,
        change_during_probe: AtomicBool,
        pause_after_move:
            Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    }
    #[cfg(unix)]
    impl TemporaryUsePort for FixtureUse {
        fn observe(&self, _: &PlatformEnvironment, path: &Path) -> TemporaryUsageObservation {
            if self.change_during_probe.swap(false, Ordering::SeqCst) {
                std::fs::write(path.join("payload"), b"changed during probe").unwrap();
            }
            let mut observation = native::unknown_usage("Fixture use observation");
            observation.state = *self.state.lock().unwrap();
            observation
        }
        fn move_unit(
            &self,
            environment: &PlatformEnvironment,
            reviewed: &TemporaryUnitSnapshot,
            accept_unknown: bool,
            _: Option<&Path>,
            _: &dyn neati_platform::TrashBackend,
        ) -> Result<(), String> {
            native::recheck_temporary_unit(environment, reviewed, &AtomicBool::new(false))?;
            let destination = self.trash.join(reviewed.path.file_name().unwrap());
            std::fs::rename(&reviewed.path, &destination).map_err(|error| error.to_string())?;
            self.moves
                .lock()
                .unwrap()
                .push((reviewed.path.clone(), accept_unknown));
            if let Some((started, release)) = self.pause_after_move.lock().unwrap().take() {
                started.send(()).map_err(|error| error.to_string())?;
                release
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        }
    }

    #[cfg(unix)]
    fn fixture() -> (
        tempfile::TempDir,
        PathBuf,
        TemporaryStorageService,
        Arc<FixtureUse>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().canonicalize().unwrap();
        let root = base.join("temporary");
        let trash = base.join("fixture-trash");
        let unit = root.join("unclassified-worktree");
        std::fs::create_dir_all(unit.join(".git")).unwrap();
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(unit.join("payload"), b"unpublished fixture").unwrap();
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_temp_dir(&root)
            .with_current_user_id(unsafe { libc::geteuid() });
        let use_port = Arc::new(FixtureUse {
            state: Mutex::new(TemporaryUsageState::UnableToDetermine),
            trash,
            moves: Mutex::new(vec![]),
            change_during_probe: AtomicBool::new(false),
            pause_after_move: Mutex::new(None),
        });
        let mut service = TemporaryStorageService::new(
            Arc::new(environment),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            Arc::new(neati_platform::MockTrashBackend::new()),
        );
        service.use_port = use_port.clone();
        (directory, unit, service, use_port)
    }

    #[cfg(unix)]
    fn consent() -> TemporaryReviewConsent {
        TemporaryReviewConsent {
            confirmed: true,
            accept_unknown_usage: true,
            accept_source_loss: true,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exact_unknown_review_moves_only_selected_fixture_and_cannot_replay() {
        let (_directory, unit, service, port) = fixture();
        let result = service
            .scan(Arc::new(|_: TemporaryStorageEvent| {}))
            .await
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert!(!result.items[0].selected_by_default);
        assert_eq!(
            result.items[0].usage.state,
            TemporaryUsageState::UnableToDetermine
        );
        assert!(result.items[0]
            .contents
            .contains(&neati_core::domain::storage::TemporaryContentKind::GitMetadata));
        let preview = service
            .prepare(&result.scan_id, &[result.items[0].options[0].id.clone()])
            .await
            .unwrap();
        assert!(preview.has_unknown_usage && preview.has_whole_folders);
        assert_eq!(preview.selected[0].path, unit.to_string_lossy());
        let outcome = service.execute(preview.id, consent()).await.unwrap();
        assert_eq!(outcome.moved_count, 1);
        assert!(!unit.exists());
        assert!(port.trash.join("unclassified-worktree/payload").exists());
        assert_eq!(port.moves.lock().unwrap()[0], (unit, true));
        assert!(service.execute(preview.id, consent()).await.is_err());
        assert!(!service.executions.request(&preview.id.to_string()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn missing_consent_consumes_draft_without_mutation() {
        let (_directory, unit, service, port) = fixture();
        let result = service
            .scan(Arc::new(|_: TemporaryStorageEvent| {}))
            .await
            .unwrap();
        let preview = service
            .prepare(&result.scan_id, &[result.items[0].options[0].id.clone()])
            .await
            .unwrap();
        assert!(service
            .execute(preview.id, TemporaryReviewConsent::default())
            .await
            .is_err());
        assert!(service.execute(preview.id, consent()).await.is_err());
        assert!(unit.exists());
        assert!(port.moves.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fresh_positive_use_blocks_even_explicit_unknown_consent() {
        let (_directory, unit, service, port) = fixture();
        let result = service
            .scan(Arc::new(|_: TemporaryStorageEvent| {}))
            .await
            .unwrap();
        let preview = service
            .prepare(&result.scan_id, &[result.items[0].options[0].id.clone()])
            .await
            .unwrap();
        *port.state.lock().unwrap() = TemporaryUsageState::InUse;
        let outcome = service.execute(preview.id, consent()).await.unwrap();
        assert_eq!(outcome.skipped_count, 1);
        assert_eq!(outcome.moved_count, 0);
        assert!(outcome.items[0].message.contains("Active use"));
        assert!(unit.exists());
        assert!(port.moves.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn duplicate_execution_cannot_replace_the_active_stop_signal() {
        let (_directory, unit, service, port) = fixture();
        let second = unit.parent().unwrap().join("second-unit");
        std::fs::create_dir(&second).unwrap();
        std::fs::write(second.join("payload"), b"remaining").unwrap();
        let service = Arc::new(service);
        let inventory = service
            .scan(Arc::new(|_: TemporaryStorageEvent| {}))
            .await
            .unwrap();
        let selected = inventory
            .items
            .iter()
            .map(|item| item.options[0].id.clone())
            .collect::<Vec<_>>();
        let preview = service
            .prepare(&inventory.scan_id, &selected)
            .await
            .unwrap();
        let (entered, reached) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        *port.pause_after_move.lock().unwrap() = Some((entered, resume));
        let worker_service = service.clone();
        let id = preview.id;
        let first = tokio::spawn(async move { worker_service.execute(id, consent()).await });
        tokio::task::spawn_blocking(move || reached.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        let duplicate = service.execute(id, consent()).await.unwrap_err();
        assert!(duplicate.contains("already executing"));
        service.cancel_execution(id).unwrap();
        release.send(()).unwrap();
        let outcome = first.await.unwrap().unwrap();
        assert_eq!(outcome.moved_count, 1);
        assert_eq!(outcome.skipped_count, 1);
        assert_eq!(port.moves.lock().unwrap().len(), 1);
        assert!(service.active_executions.lock().unwrap().is_empty());
        assert!(!service.executions.request(&id.to_string()));
        let kept = outcome.items.iter().find(|item| !item.success).unwrap();
        let kept_scope = inventory
            .items
            .iter()
            .flat_map(|item| &item.options)
            .find(|scope| scope.id == kept.item_id)
            .unwrap();
        assert!(Path::new(&kept_scope.path).join("payload").exists());
    }

    #[cfg(unix)]
    #[test]
    fn separate_hard_link_units_keep_distinct_operations_and_qualified_totals() {
        let (_directory, unit, service, port) = fixture();
        let first = unit.parent().unwrap().join("first-file");
        let second = unit.parent().unwrap().join("second-file");
        std::fs::write(&first, b"shared physical fixture").unwrap();
        std::fs::hard_link(&first, &second).unwrap();
        let (observed, inventory) = scan_inventory(
            &service.environment,
            "links",
            &AtomicBool::new(false),
            port.as_ref(),
            &|_: TemporaryStorageEvent| {},
        );
        assert_eq!(observed.items.len(), 3);
        assert!(observed.physical_overlap);
        let links = observed
            .items
            .iter()
            .filter(|item| item.physical_overlap)
            .collect::<Vec<_>>();
        assert_eq!(links.len(), 2);
        let selected = links
            .iter()
            .map(|item| item.options[0].id.clone())
            .collect::<Vec<_>>();
        assert_eq!(selected_units(&inventory, &selected).unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn changes_during_final_probe_and_expired_inventory_keep_fixture() {
        let (_directory, unit, service, port) = fixture();
        let result = service
            .scan(Arc::new(|_: TemporaryStorageEvent| {}))
            .await
            .unwrap();
        let preview = service
            .prepare(&result.scan_id, &[result.items[0].options[0].id.clone()])
            .await
            .unwrap();
        port.change_during_probe.store(true, Ordering::SeqCst);
        let outcome = service.execute(preview.id, consent()).await.unwrap();
        assert_eq!(outcome.skipped_count, 1);
        assert!(outcome.items[0].message.contains("changed"));
        assert!(unit.exists());
        assert!(port.moves.lock().unwrap().is_empty());
        service
            .inventory
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .created_at = native::unix_timestamp().saturating_sub(INVENTORY_TTL + 1);
        assert!(service
            .prepare(&result.scan_id, &[result.items[0].options[0].id.clone()])
            .await
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn root_child_overlap_forgery_cancellation_and_draft_expiry_refuse_authority() {
        let (_directory, unit_path, service, port) = fixture();
        let (_, mut inventory) = scan_inventory(
            &service.environment,
            "fixture",
            &AtomicBool::new(false),
            port.as_ref(),
            &|_: TemporaryStorageEvent| {},
        );
        let whole = inventory.units.values().next().unwrap().clone();
        let mut child = whole.clone();
        child.option.id = "child".into();
        child.snapshot.path = unit_path.join("target");
        inventory.units.insert(child.option.id.clone(), child);
        assert!(selected_units(&inventory, &[whole.option.id.clone(), "child".into()]).is_err());
        assert!(selected_units(&inventory, &[whole.option.id.clone(), "forged".into()]).is_err());
        let (cancelled, cancelled_inventory) = scan_inventory(
            &service.environment,
            "cancelled",
            &AtomicBool::new(true),
            port.as_ref(),
            &|_: TemporaryStorageEvent| {},
        );
        assert!(cancelled.cancelled && cancelled.partial);
        assert!(cancelled_inventory.units.is_empty());
        let id = uuid::Uuid::new_v4();
        let now = native::unix_timestamp();
        service
            .plans
            .insert(
                TemporaryReviewDraft {
                    id,
                    created_at: now.saturating_sub(301),
                    units: vec![whole.clone()],
                },
                now,
            )
            .unwrap();
        assert!(service.plans.take_valid(id, now).is_err());
        let accepted = TemporaryReviewDraft {
            id,
            created_at: now,
            units: vec![whole],
        }
        .accept(consent())
        .unwrap();
        let outcome = execute_accepted(
            &service.environment,
            accepted,
            &AtomicBool::new(true),
            port.as_ref(),
            service.trash.as_ref(),
        );
        assert_eq!(outcome.skipped_count, 1);
        assert!(unit_path.exists());
    }

    #[tokio::test]
    async fn unsupported_platform_returns_unavailable_not_successful_empty_support() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::Windows)
            .with_platform(PlatformKind::Windows);
        let service = TemporaryStorageService::new(
            Arc::new(environment),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            Arc::new(neati_platform::MockTrashBackend::new()),
        );
        let result = service
            .scan(Arc::new(|_: TemporaryStorageEvent| {}))
            .await
            .unwrap();
        assert!(!result.available);
        assert!(result.unavailable_reason.is_some());
        assert!(service
            .prepare(&result.scan_id, &["forged".into()])
            .await
            .is_err());
    }
}
