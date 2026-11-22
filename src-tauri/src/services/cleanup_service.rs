use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use uuid::Uuid;

use super::cancellation::{CancellationRegistry, ScanCancellation};
use super::plan_store::{PlanStore, PlanStoreError};
use super::scan_service::ScanService;
use super::scan_store::{ScanCheckpoint, ScanLease, ScanStore};
use crate::cleaner::{CleanExecutor, LifecycleProviderRegistry, OwnerProviderRegistry};
use crate::execution_budget::ExecutionBudgets;
use crate::models::{
    CleanEvent, CleanFailureReason, CleanResult, CleanStrategy, CleanupEligibility, CleanupFailure,
    CleanupFailureScope, CleanupProgressSink, DeletePlan, NeatiError, NeatiSettings,
    ObservationQuality, PlanPreview, PlanRefusalPreview, PlatformCapabilitiesProvider,
    PlatformFeature, PublishedScan, ResumeScanRequest, ScanEvent, ScanProgressSink, ScanRequest,
    ScanResult,
};
use crate::operation_gate::StorageOperationGate;
use crate::safety::SafetyPlanner;
use crate::services::system_service::DockerStatusCache;
use crate::signatures::SignatureRegistry;
use neati_platform::PlatformEnvironment;

pub(super) fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Derives backend-owned candidates for direct cache cleanup.
///
/// Enforces:
/// - disposition.eligibility == AutoCleanable
/// - cleanable_bytes > 0
/// - category enabled in settings
///
/// Includes eligible Safe and Rebuild caches. Incomplete items, running owners,
/// and operations requiring separate confirmation never enter this set.
/// An unrelated inaccessible location does not invalidate a verified item.
pub fn select_quick_clean_safe_candidates(
    scan: &ScanResult,
    settings: &NeatiSettings,
) -> Vec<String> {
    let mut eligible_ids = Vec::new();
    for category in &scan.categories {
        if !settings.is_category_clean_enabled(category.category) {
            continue;
        }
        for item in &category.items {
            if item.has_current_disposition()
                && item.disposition.eligibility == CleanupEligibility::AutoCleanable
                && item.cleanable_bytes() > 0
            {
                eligible_ids.push(item.id.clone());
            }
        }
    }
    eligible_ids
}

/// The projection of one planner refusal onto the interface contract.
fn refusal_preview(refusal: &crate::models::PlanItemRefusal) -> PlanRefusalPreview {
    PlanRefusalPreview {
        item_id: refusal.item_id.clone(),
        name: refusal.item_name.clone(),
        reason: refusal.reason,
        message: refusal.message.clone(),
    }
}

/// Maps a planning failure onto the scope the interface reacts to.
///
/// The distinction the interface cannot make for itself: a refusal that names
/// items leaves the inventory and every other selection usable, while a scan
/// that is no longer current makes the inventory a description of a machine
/// that has changed. Reading both as the same failure is what made a correct
/// refusal look like a broken selection.
fn plan_failure(error: NeatiError) -> CleanupFailure {
    match error {
        NeatiError::RefusedSelection(refusals) => CleanupFailure::items(
            "Nothing in the selection can be cleaned right now.",
            refusals.iter().map(refusal_preview).collect(),
        ),
        NeatiError::ChangedSinceScan(message) => CleanupFailure::inventory_stale(message),
        NeatiError::UnsupportedManualOperation(name) => CleanupFailure::items(
            format!(
                "`{name}` is reported for information only; this build has no reviewed operation that removes it"
            ),
            Vec::new(),
        ),
        other => CleanupFailure::new(
            CleanupFailureScope::Internal,
            CleanFailureReason::Unknown,
            crate::diagnostics::sanitize_log(&other.to_string()),
        ),
    }
}

/// Maps a plan-store refusal onto the scope the interface reacts to.
fn store_failure(error: PlanStoreError) -> CleanupFailure {
    match error {
        PlanStoreError::Unavailable(message) => CleanupFailure::new(
            CleanupFailureScope::PlanUnavailable,
            CleanFailureReason::PlanUnavailable,
            message,
        ),
        PlanStoreError::Unusable(message) => CleanupFailure::new(
            CleanupFailureScope::Internal,
            CleanFailureReason::Unknown,
            message,
        ),
    }
}

/// The intent for a cleanup operation.
///
/// Main Clean and Quick Clean are intents routed through the same
/// unified security, validation, invalidation, and execution pipeline.
#[derive(Debug)]
enum CleanupIntent {
    ReviewedSelection {
        plan_id: Uuid,
        confirmed: bool,
    },
    QuickSafe,
    ReviewedQuickSafe {
        scan_id: String,
        selected_item_ids: Vec<String>,
    },
}

/// A lease belongs to the blocking worker, including when its caller stops
/// awaiting it. Unwinding or returning an error cannot leave discovery running.
struct ActiveScanLease {
    store: Arc<ScanStore>,
    lease: Option<ScanLease>,
}

impl ActiveScanLease {
    fn new(store: Arc<ScanStore>, lease: ScanLease) -> Self {
        Self {
            store,
            lease: Some(lease),
        }
    }

    fn finish(mut self, result: Result<PublishedScan, String>) -> Result<PublishedScan, String> {
        if let Some(lease) = self.lease.take() {
            if let Err(error) = &result {
                self.store.stop(lease, error.clone());
            }
        }
        result
    }
}

impl Drop for ActiveScanLease {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            self.store
                .stop(lease, "The scan worker stopped before publication.");
        }
    }
}

/// Retire the reported Stop handle even if a progress sink or scanner panics.
struct ActiveScanCancellation {
    registry: Arc<CancellationRegistry>,
    registered: std::sync::Mutex<Option<String>>,
}

impl Drop for ActiveScanCancellation {
    fn drop(&mut self) {
        let registered = self
            .registered
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(scan_id) = registered.take() {
            self.registry.remove(&scan_id);
        }
    }
}

/// Application service coordinating scanning, safety planning, and execution.
///
/// Manages operation serialization, execution budgets, plan TTLs, and scan invalidation
/// centrally so command handlers remain thin IPC adapters.
pub struct CleanupService {
    scan_service: Arc<ScanService>,
    plan_store: Arc<PlanStore<DeletePlan>>,
    pub(super) scan_store: Arc<ScanStore>,
    pub(super) operation_gate: StorageOperationGate,
    budgets: Arc<ExecutionBudgets>,
    pub(super) environment: Arc<PlatformEnvironment>,
    pub(super) registry: Arc<SignatureRegistry>,
    docker_status_cache: Arc<DockerStatusCache>,
    lifecycle_providers: Arc<LifecycleProviderRegistry>,
    owner_providers: Arc<OwnerProviderRegistry>,
    /// Compatibility port for explicit legacy unit-test plans. The current
    /// catalog rejects Trash disposition; Safe/Rebuild defaults are permanent.
    trash_backend: Arc<dyn neati_platform::TrashBackend>,
    /// The cancellation handles of the scans this service is running, keyed by
    /// the id each scan reports so `cancel_scan` can reach one in flight.
    scan_cancellations: Arc<CancellationRegistry>,
    /// Only scans share one publication lifecycle; unrelated storage reads
    /// continue to use the concurrent read gate.
    scan_lifecycle: Arc<AsyncMutex<()>>,
    /// Bounds executing and waiting scan sessions independently of expensive
    /// storage reads, so a waiting scan never reserves shared read capacity.
    scan_admissions: Arc<Semaphore>,
    pub(super) platform_capabilities: Arc<dyn PlatformCapabilitiesProvider>,
    pub(super) empty_trash: Arc<super::empty_trash::EmptyTrashState>,
    pub(super) quit_dependencies: Option<super::cleanup_quit::CleanupQuitDependencies>,
}

impl CleanupService {
    const SCAN_CATEGORY_SLICE_LIMIT: usize = 2;
    /// One lifecycle owner plus at most seventeen waiting requests. Admission
    /// is immediate or refused; the semaphore itself has no waiting queue.
    const MAX_ADMITTED_SCAN_SESSIONS: usize = 18;
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        scan_service: Arc<ScanService>,
        plan_store: Arc<PlanStore<DeletePlan>>,
        scan_store: Arc<ScanStore>,
        operation_gate: StorageOperationGate,
        budgets: Arc<ExecutionBudgets>,
        environment: Arc<PlatformEnvironment>,
        registry: Arc<SignatureRegistry>,
        docker_status_cache: Arc<DockerStatusCache>,
        lifecycle_providers: Arc<LifecycleProviderRegistry>,
        owner_providers: Arc<OwnerProviderRegistry>,
        trash_backend: Arc<dyn neati_platform::TrashBackend>,
        platform_capabilities: Arc<dyn PlatformCapabilitiesProvider>,
    ) -> Self {
        Self {
            quit_dependencies: None,
            empty_trash: Arc::new(super::empty_trash::EmptyTrashState::default()),
            scan_service,
            plan_store,
            scan_store,
            operation_gate,
            budgets,
            environment,
            registry,
            docker_status_cache,
            lifecycle_providers,
            owner_providers,
            trash_backend,
            scan_cancellations: Arc::new(CancellationRegistry::for_scans()),
            scan_lifecycle: Arc::new(AsyncMutex::new(())),
            scan_admissions: Arc::new(Semaphore::new(Self::MAX_ADMITTED_SCAN_SESSIONS)),
            platform_capabilities,
        }
    }

    /// Runs a full cleanup scan under the storage operation gate and execution budgets.
    pub async fn start_scan(
        &self,
        request: ScanRequest,
        progress: Arc<dyn ScanProgressSink>,
    ) -> Result<PublishedScan, String> {
        self.start_scan_with_limit(request, progress, Self::SCAN_CATEGORY_SLICE_LIMIT)
            .await
    }

    /// Runs to exhaustion for a surface that is intentionally not granted the
    /// resume command, such as the Quick Panel.
    pub async fn start_scan_complete(
        &self,
        request: ScanRequest,
        progress: Arc<dyn ScanProgressSink>,
    ) -> Result<PublishedScan, String> {
        self.start_scan_with_limit(request, progress, usize::MAX)
            .await
    }

    async fn start_scan_with_limit(
        &self,
        request: ScanRequest,
        progress: Arc<dyn ScanProgressSink>,
        slice_limit: usize,
    ) -> Result<PublishedScan, String> {
        self.platform_capabilities
            .capabilities()
            .require(
                PlatformFeature::Cleanup,
                crate::models::CapabilityAccess::Inspect,
            )
            .map_err(|e| e.to_string())?;

        if request.intensive_cleanup {
            self.platform_capabilities
                .capabilities()
                .require(
                    PlatformFeature::IntensiveCleanup,
                    crate::models::CapabilityAccess::Inspect,
                )
                .map_err(|e| e.to_string())?;
        }

        let scan_service = self.scan_service.clone();
        let scan_store = self.scan_store.clone();
        let cancellations = self.scan_cancellations.clone();
        self.run_scan_session(move || {
            cancellations.request_all();
            let lease = scan_store.begin();
            let active = ActiveScanLease::new(scan_store.clone(), lease);
            let mut categories = request.categories.clone().unwrap_or_else(|| {
                vec![
                    crate::models::Category::Ai,
                    crate::models::Category::Developer,
                    crate::models::Category::Container,
                    crate::models::Category::System,
                ]
            });
            let remaining = if categories.len() > slice_limit {
                categories.split_off(slice_limit)
            } else {
                Vec::new()
            };
            let mut pass_request = request.clone();
            pass_request.categories = Some(categories);
            let result = Self::run_scan_pass(
                &scan_service,
                cancellations,
                &pass_request,
                progress.as_ref(),
            );
            let checkpoint = (!result.cancelled && !remaining.is_empty()).then(|| ScanCheckpoint {
                request,
                remaining_categories: remaining,
                slices: vec![result.clone()],
                freshness_anchor: result.finished_at,
            });
            active.finish(if result.cancelled {
                scan_store.publish_stopped(lease, result, "Scan was cancelled before completion.")
            } else {
                scan_store.publish(lease, result, checkpoint)
            })
        })
        .await
    }

    /// Continues exactly the backend checkpoint named by the current scan.
    pub async fn resume_scan(
        &self,
        request: ResumeScanRequest,
        progress: Arc<dyn ScanProgressSink>,
    ) -> Result<PublishedScan, String> {
        self.platform_capabilities
            .capabilities()
            .require(
                PlatformFeature::Cleanup,
                crate::models::CapabilityAccess::Inspect,
            )
            .map_err(|error| error.to_string())?;
        let scan_service = self.scan_service.clone();
        let scan_store = self.scan_store.clone();
        let cancellations = self.scan_cancellations.clone();
        self.run_scan_session(move || {
            let claimed = scan_store.claim(&request, unix_timestamp())?;
            let active = ActiveScanLease::new(scan_store.clone(), claimed.lease);
            let mut checkpoint = claimed.checkpoint;
            let mut categories = std::mem::take(&mut checkpoint.remaining_categories);
            let remaining = if categories.len() > Self::SCAN_CATEGORY_SLICE_LIMIT {
                categories.split_off(Self::SCAN_CATEGORY_SLICE_LIMIT)
            } else {
                Vec::new()
            };
            let mut pass_request = checkpoint.request.clone();
            pass_request.categories = Some(categories);
            let result = Self::run_scan_pass(
                &scan_service,
                cancellations,
                &pass_request,
                progress.as_ref(),
            );
            checkpoint.slices.push(result.clone());
            let Some(mut merged) = scan_service.merge_slices(&checkpoint.slices) else {
                return active.finish(Err(
                    "The retained scan contained no observations.".to_string()
                ));
            };
            merged.finished_at = checkpoint.freshness_anchor;
            checkpoint.remaining_categories = remaining;
            let next = (!result.cancelled && !checkpoint.remaining_categories.is_empty())
                .then_some(checkpoint);
            active.finish(if result.cancelled {
                scan_store.publish_stopped(
                    claimed.lease,
                    merged,
                    "Scan was cancelled before completion.",
                )
            } else {
                scan_store.publish(claimed.lease, merged, next)
            })
        })
        .await
    }

    async fn run_scan_session(
        &self,
        operation: impl FnOnce() -> Result<PublishedScan, String> + Send + 'static,
    ) -> Result<PublishedScan, String> {
        // Bound lifecycle waiters without reserving shared storage-read slots.
        // This workflow mutex owns no domain state; no scan lease, cancellation
        // handle or storage gate exists while awaiting it or the read budget.
        let admission = self
            .scan_admissions
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                "Scan queue is full; try again after the current scan finishes.".to_string()
            })?;
        let lifecycle = self.scan_lifecycle.clone().lock_owned().await;
        let permit = self.budgets.acquire_storage_read().await?;
        let operation_gate = self.operation_gate.clone();
        crate::blocking::run_blocking(
            move || {
                // Keep all guards in the actual worker: dropping its awaiting
                // command future must not release a still-running scan session.
                let _admission = admission;
                let _permit = permit;
                let _lifecycle = lifecycle;
                operation_gate.run_read(operation)
            },
            "Scan worker panicked",
        )
        .await
    }

    fn run_scan_pass(
        scan_service: &ScanService,
        cancellations: Arc<CancellationRegistry>,
        request: &ScanRequest,
        progress: &dyn ScanProgressSink,
    ) -> ScanResult {
        let registration = ActiveScanCancellation {
            registry: cancellations,
            registered: std::sync::Mutex::new(None),
        };
        let signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = ScanCancellation::new(signal.clone());
        let sink = |event: ScanEvent| {
            if let ScanEvent::Started { scan_id } = &event {
                *registration
                    .registered
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(scan_id.clone());
                registration
                    .registry
                    .register(scan_id.clone(), signal.clone());
            }
            progress.emit(event);
        };
        scan_service.scan(request, &sink, &probe)
    }

    /// Returns the current cached scan result if available.
    pub fn get_last_scan(&self) -> Option<PublishedScan> {
        self.scan_store.get_published()
    }

    /// Requests cancellation of the scan reporting `scan_id`.
    ///
    /// A scan that already finished is not an error: there is nothing to stop,
    /// and saying so as a failure would report a normal race as a broken
    /// command. The caller sees the same answer either way, and the scan's own
    /// result is what states whether it was cancelled.
    pub fn cancel_scan(&self, scan_id: &str) -> Result<(), String> {
        self.scan_cancellations.request(scan_id);
        Ok(())
    }

    /// Creates and stores a verified cleanup plan from user-reviewed item IDs.
    ///
    /// The refusal scope is the contract's point: an item a current policy
    /// declines is reported per item, and only a scan that is no longer current
    /// invalidates the inventory the interface holds.
    pub async fn create_delete_plan(
        &self,
        scan_id: String,
        selected_item_ids: Vec<String>,
    ) -> Result<PlanPreview, CleanupFailure> {
        self.platform_capabilities
            .capabilities()
            .require(
                PlatformFeature::Cleanup,
                crate::models::CapabilityAccess::Mutate,
            )
            .map_err(|error| {
                CleanupFailure::new(
                    CleanupFailureScope::Permission,
                    CleanFailureReason::PermissionDenied,
                    error.to_string(),
                )
            })?;

        let scan_store = self.scan_store.clone();
        let plan_store = self.plan_store.clone();
        let registry = self.registry.clone();
        let environment = self.environment.clone();
        let owner_providers = self.owner_providers.clone();

        crate::blocking::run_blocking(
            move || -> Result<PlanPreview, CleanupFailure> {
                let scan = scan_store
                    .get()
                    .filter(|scan| scan.scan_id == scan_id)
                    .ok_or_else(|| {
                        CleanupFailure::inventory_stale(
                            "The scan is no longer current. Scan again before cleaning.",
                        )
                    })?;

                let plan = SafetyPlanner::create_plan_from_scan(
                    &scan,
                    &scan_id,
                    &selected_item_ids,
                    &registry,
                    &environment,
                    &owner_providers,
                )
                .map_err(plan_failure)?;

                let ttl = plan_store.ttl_seconds();
                let mut preview = plan.preview(ttl);
                preview.expires_at = preview.expires_at.min(
                    scan.finished_at
                        .saturating_add(u64::from(ScanResult::VALID_FOR_SECONDS)),
                );

                let now = unix_timestamp();
                plan_store.insert(plan, now).map_err(store_failure)?;
                Ok(preview)
            },
            "Delete plan worker panicked",
        )
        .await
    }

    /// Executes a reviewed DeletePlan by plan_id.
    pub async fn execute_clean(
        &self,
        plan_id: Uuid,
        confirmed: bool,
        progress: Arc<dyn CleanupProgressSink>,
    ) -> Result<CleanResult, CleanupFailure> {
        self.execute_intent(
            CleanupIntent::ReviewedSelection { plan_id, confirmed },
            None,
            progress,
        )
        .await
    }

    /// Executes the verified cache subset of a current scan. The legacy IPC
    /// name is retained; AutoCleanable includes regenerable Rebuild caches.
    pub async fn quick_clean_safe(
        &self,
        settings: &NeatiSettings,
        progress: Arc<dyn CleanupProgressSink>,
    ) -> Result<CleanResult, CleanupFailure> {
        self.execute_intent(CleanupIntent::QuickSafe, Some(settings.clone()), progress)
            .await
    }

    /// Executes exactly the cache identities displayed when Clean was pressed.
    /// The legacy IPC name remains compatible. The backend re-derives each
    /// candidate; the panel never supplies paths or strategies.
    pub async fn reviewed_quick_clean_safe(
        &self,
        scan_id: String,
        selected_item_ids: Vec<String>,
        settings: &NeatiSettings,
        progress: Arc<dyn CleanupProgressSink>,
    ) -> Result<CleanResult, CleanupFailure> {
        self.execute_intent(
            CleanupIntent::ReviewedQuickSafe {
                scan_id,
                selected_item_ids,
            },
            Some(settings.clone()),
            progress,
        )
        .await
    }

    /// Internal execution core ensuring identical security and lifecycle semantics for all clean intents.
    /// Completed cleanups as counts, newest first. The file holds no paths or
    /// names, so this is safe to show and to copy into a report.
    pub async fn cleanup_history(&self) -> Result<Vec<crate::models::CleanupHistoryEntry>, String> {
        let environment = self.environment.clone();
        crate::blocking::run_blocking(
            move || {
                crate::cleaner::history::list(&environment)
                    .map_err(|error| format!("Cleanup history could not be read: {error}"))
            },
            "cleanup history",
        )
        .await
    }

    async fn execute_intent(
        &self,
        intent: CleanupIntent,
        settings: Option<NeatiSettings>,
        progress: Arc<dyn CleanupProgressSink>,
    ) -> Result<CleanResult, CleanupFailure> {
        self.platform_capabilities
            .capabilities()
            .require(
                PlatformFeature::Cleanup,
                crate::models::CapabilityAccess::Mutate,
            )
            .map_err(|error| {
                CleanupFailure::new(
                    CleanupFailureScope::Permission,
                    CleanFailureReason::PermissionDenied,
                    error.to_string(),
                )
            })?;

        let plan_store = self.plan_store.clone();
        let scan_store = self.scan_store.clone();
        let operation_gate = self.operation_gate.clone();
        let environment = self.environment.clone();
        let registry = self.registry.clone();
        let docker_status_cache = self.docker_status_cache.clone();
        let lifecycle_providers = self.lifecycle_providers.clone();
        let owner_providers = self.owner_providers.clone();
        let trash_backend = self.trash_backend.clone();

        crate::blocking::run_blocking(
            move || -> Result<CleanResult, CleanupFailure> {
                operation_gate.run_write(|| -> Result<CleanResult, CleanupFailure> {
                    let now = unix_timestamp();

                    let plan: DeletePlan = match &intent {
                        CleanupIntent::ReviewedSelection { plan_id, confirmed } => {
                            let plan = plan_store.take_valid(*plan_id, now).map_err(store_failure)?;
                            if plan.requires_confirmation() && !*confirmed {
                                return Err(CleanupFailure::new(
                                    CleanupFailureScope::Internal,
                                    CleanFailureReason::Unknown,
                                    "This cleanup includes an action that requires explicit confirmation. Review the plan and confirm it before cleaning.",
                                ));
                            }
                            // Invalidate scan atomically so pre-cleanup inventory cannot be reused
                            scan_store.validate_and_invalidate_for_cleanup(&plan.scan_id, now)?;
                            plan
                        }
                        CleanupIntent::QuickSafe | CleanupIntent::ReviewedQuickSafe { .. } => {
                            let settings = settings
                                .ok_or_else(|| "Settings required for Quick Clean".to_string())?;
                            let scan = scan_store.get().ok_or_else(|| {
                                CleanupFailure::inventory_stale(
                                    "The scan is no longer current. Scan again before cleaning.",
                                )
                            })?;

                            scan.validate_for_cleanup(&scan.scan_id, now)
                                .map_err(|error| {
                                    CleanupFailure::inventory_stale(error.to_string())
                                })?;

                            let reviewed = matches!(&intent, CleanupIntent::ReviewedQuickSafe { .. });
                            if !matches!(scan.quality, ObservationQuality::Fresh | ObservationQuality::Partial) {
                                return Err(CleanupFailure::inventory_stale(
                                    "Quick Clean needs a current scan. Scan again before cleaning.",
                                ));
                            }

                            let eligible_ids = select_quick_clean_safe_candidates(&scan, &settings);
                            let selected_ids = match &intent {
                                CleanupIntent::ReviewedQuickSafe {
                                    scan_id,
                                    selected_item_ids,
                                } => {
                                    if scan_id != &scan.scan_id {
                                        return Err(CleanupFailure::inventory_stale(
                                            "The reviewed scan has changed. Scan again before cleaning.",
                                        ));
                                    }
                                    let eligible: HashSet<_> = eligible_ids.iter().collect();
                                    let unique: HashSet<_> = selected_item_ids.iter().collect();
                                    if selected_item_ids.is_empty()
                                        || unique.len() != selected_item_ids.len()
                                        || !unique.is_subset(&eligible)
                                    {
                                        return Err(CleanupFailure::items(
                                            "The selection contains a cache that is not ready to clean. Scan again.",
                                            Vec::new(),
                                        ));
                                    }
                                    selected_item_ids.clone()
                                }
                                _ => eligible_ids,
                            };
                            if selected_ids.is_empty() {
                                return Ok(CleanResult {
                                    plan_id: Uuid::new_v4(),
                                    started_at: now,
                                    finished_at: now,
                                    total_reclaimed_bytes: 0,
                                    total_moved_to_trash_bytes: 0,
                                    total_failed_bytes: 0,
                                    partial_count: 0,
                                    failed_count: 0,
                                    skipped_count: 0,
                                    items: vec![],
                                    actual_disk_free_delta: Some(0),
                                });
                            }

                            let plan = SafetyPlanner::create_plan_from_scan(
                                &scan,
                                &scan.scan_id,
                                &selected_ids,
                                &registry,
                                &environment,
                                &owner_providers,
                            )
                            .map_err(plan_failure)?;

                            if reviewed && !plan.refusals.is_empty() {
                                return Err(CleanupFailure::items(
                                    "Some caches changed before cleanup. Scan again.",
                                    plan.refusals.iter().map(refusal_preview).collect(),
                                ));
                            }

                            if plan.requires_confirmation() {
                                return Err(CleanupFailure::new(
                                    CleanupFailureScope::Internal,
                                    CleanFailureReason::Unknown,
                                    "Quick Clean cannot execute actions that require explicit confirmation.",
                                ));
                            }

                            // Invalidate scan atomically
                            scan_store
                                .validate_and_invalidate_for_cleanup(&plan.scan_id, now)
                                .map_err(CleanupFailure::inventory_stale)?;
                            plan
                        }
                    };

                    // Only a plan that authorizes a mutation may reach the
                    // executor. A projection the user reviewed and a deletion
                    // are different claims, and the plan states which one it is.
                    if !plan.mode.is_mutating() {
                        return Err(CleanupFailure::new(
                            CleanupFailureScope::Internal,
                            CleanFailureReason::Unknown,
                            format!(
                                "This plan is {} and cannot be executed",
                                plan.mode.display_name()
                            ),
                        ));
                    }

                    // A Docker prune may have changed the runtime's state, so the
                    // shared observation is dropped rather than reported stale.
                    if plan
                        .targets
                        .iter()
                        .any(|t| t.strategy == CleanStrategy::DockerPrune)
                    {
                        docker_status_cache.invalidate();
                    }

                    let result = CleanExecutor::execute(
                        plan,
                        &environment,
                        &lifecycle_providers,
                        &owner_providers,
                        trash_backend.as_ref(),
                        move |event: CleanEvent| {
                            progress.emit(event);
                        },
                    );
                    if let Err(error) = crate::cleaner::history::record(&result, &environment) {
                        crate::diagnostics::log_error(
                            "cleanup-history",
                            &format!("Cleanup history could not be recorded: {error}"),
                        );
                    }
                    Ok(result)
                })
            },
            "Cleanup worker panicked",
        )
        .await
    }
}

#[cfg(test)]
mod tests;
