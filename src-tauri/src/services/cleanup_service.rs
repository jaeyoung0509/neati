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
    /// Native Trash / Recycle Bin adapter used only after the cleanup safety
    /// guard minted a validated non-Safe filesystem target.
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
mod tests {
    use super::*;
    use crate::cache_providers::{CacheProviderScan, CacheProviderScanner};
    use crate::models::{
        Category, CategoryResult, FileSize, ObservationQuality, PlatformCapabilities, RiskTier,
        ScanDiscovery, ScanItem, Signature,
    };
    use crate::scanner::{ScanLimits, TraversalCounters};
    use neati_platform::path_algebra::PathFlavor;
    use std::sync::Mutex;

    struct TestCapabilitiesProvider(PlatformCapabilities);

    #[derive(Default)]
    struct RecordingCacheProviders {
        excluded: Mutex<Vec<Vec<String>>>,
    }

    impl CacheProviderScanner for RecordingCacheProviders {
        fn scan_items(
            &self,
            _registry: &SignatureRegistry,
            excluded_signatures: &[String],
            _environment: &PlatformEnvironment,
            _cancellation: &dyn crate::models::CancellationProbe,
            _limits: ScanLimits,
            _counters: &TraversalCounters,
            _progress: &dyn crate::scanner::RootProgressSink,
        ) -> CacheProviderScan {
            self.excluded
                .lock()
                .unwrap()
                .push(excluded_signatures.to_vec());
            CacheProviderScan::default()
        }
    }

    impl PlatformCapabilitiesProvider for TestCapabilitiesProvider {
        fn capabilities(&self) -> PlatformCapabilities {
            self.0.clone()
        }
    }

    fn make_test_item(id: &str, risk: RiskTier, bytes: u64) -> ScanItem {
        let mut item = ScanItem::mock(
            id,
            "test_sig",
            "Test Item",
            Category::System,
            risk,
            "/tmp/test",
            FileSize::new(bytes, Some(bytes)),
            1,
        );
        item.disposition = item.derive_disposition();
        item
    }

    fn make_test_scan(items: Vec<ScanItem>) -> ScanResult {
        make_test_scan_with_quality(items, ObservationQuality::Fresh)
    }

    /// A scan whose overall quality is stated, so a caller can present the
    /// partial observations a cancelled or bounded scan produces.
    fn make_test_scan_with_quality(
        items: Vec<ScanItem>,
        quality: ObservationQuality,
    ) -> ScanResult {
        let cleanable = items.iter().map(|i| i.cleanable_bytes()).sum();
        let now = unix_timestamp();
        ScanResult {
            cancelled: false,
            metrics: Default::default(),
            scan_id: "scan_123".to_string(),
            valid_for_seconds: ScanResult::VALID_FOR_SECONDS,
            started_at: now.saturating_sub(5),
            finished_at: now,
            total_bytes: cleanable,
            cleanable_bytes: cleanable,
            safe_bytes: cleanable,
            rebuild_bytes: 0,
            manual_bytes: 0,
            categories: vec![CategoryResult {
                category: Category::System,
                display_name: Category::System.display_name().to_string(),
                total_bytes: cleanable,
                cleanable_bytes: cleanable,
                safe_bytes: cleanable,
                rebuild_bytes: 0,
                manual_bytes: 0,
                quality,
                skipped_entry_count: 0,
                incomplete_item_count: 0,
                eligibility: Default::default(),
                suppressed_duplicate_count: 0,
                suppressed_duplicate_bytes: 0,
                suppressed_overlap_count: 0,
                suppressed_overlap_bytes: 0,
                ambiguous_overlap_count: 0,
                ambiguous_overlap_bytes: 0,
                items,
            }],
            incomplete_reasons: Vec::new(),
            gaps: Vec::new(),
            spans: Vec::new(),
            quality,
            skipped_entry_count: 0,
            incomplete_item_count: 0,
            eligibility: Default::default(),
            suppressed_duplicate_count: 0,
            suppressed_duplicate_bytes: 0,
            suppressed_overlap_count: 0,
            suppressed_overlap_bytes: 0,
            ambiguous_overlap_count: 0,
            ambiguous_overlap_bytes: 0,
        }
    }

    #[test]
    fn select_quick_clean_filters_categories_and_risk() {
        let safe_item = make_test_item("item_safe", RiskTier::Safe, 100);
        let rebuild_item = make_test_item("item_rebuild", RiskTier::Rebuild, 200);
        let scan = make_test_scan(vec![safe_item, rebuild_item]);

        let settings = NeatiSettings::default();
        let selected = select_quick_clean_safe_candidates(&scan, &settings);
        assert_eq!(selected, vec!["item_safe", "item_rebuild"]);
    }

    #[test]
    fn direct_cleanup_excludes_unverified_running_stateful_and_disabled_items() {
        let ready = make_test_item("ready", RiskTier::Rebuild, 200);
        let mut partial = make_test_item("partial", RiskTier::Rebuild, 200);
        partial.quality = ObservationQuality::Partial;
        partial.rederive_disposition();
        let mut running = make_test_item("running", RiskTier::Rebuild, 200);
        running.owner_running = true;
        running.rederive_disposition();
        let mut confirmation = make_test_item("confirmation", RiskTier::Rebuild, 200);
        confirmation.requires_confirmation = true;
        confirmation.rederive_disposition();
        let manual = make_test_item("manual", RiskTier::Manual, 200);
        let mut inconsistent = make_test_item("inconsistent", RiskTier::Rebuild, 200);
        inconsistent.owner_running = true; // A forged old disposition must not grant authority.
        let mut scan = make_test_scan(vec![
            ready,
            partial,
            running,
            confirmation,
            manual,
            inconsistent,
        ]);
        scan.quality = ObservationQuality::Partial;
        assert_eq!(
            select_quick_clean_safe_candidates(&scan, &NeatiSettings::default()),
            vec!["ready"]
        );

        scan.categories[0].category = Category::Developer;
        let settings = NeatiSettings {
            clean_developer_tools: false,
            ..Default::default()
        };
        assert!(select_quick_clean_safe_candidates(&scan, &settings).is_empty());
    }

    fn isolated_scan_service() -> Arc<CleanupService> {
        let environment = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let registry = Arc::new(SignatureRegistry::new());
        let lifecycle = Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new()));
        let owners = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new_with_cache_providers(
            registry.clone(),
            lifecycle.clone(),
            owners.clone(),
            Arc::new(RecordingCacheProviders::default()),
            environment.clone(),
        ));
        Arc::new(CleanupService::new(
            scan_service,
            Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup())),
            Arc::new(ScanStore::new()),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            environment,
            registry,
            Arc::new(DockerStatusCache::new()),
            lifecycle,
            owners,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        ))
    }

    fn held_scan_progress() -> (
        Arc<dyn ScanProgressSink>,
        tokio::sync::oneshot::Receiver<String>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let started_tx = Mutex::new(Some(started_tx));
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let progress = Arc::new(move |event: ScanEvent| {
            if let ScanEvent::Started { scan_id } = event {
                started_tx
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(scan_id)
                    .unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
        });
        (progress, started_rx, release_tx)
    }

    #[tokio::test]
    async fn concurrent_windows_cannot_supersede_a_scan_before_publication() {
        let service = isolated_scan_service();
        let (first_progress, first_started, release_first) = held_scan_progress();
        let first_service = service.clone();
        let first = tokio::spawn(async move {
            first_service
                .start_scan_complete(ScanRequest::default(), first_progress)
                .await
        });
        let first_id = first_started.await.unwrap();
        let (second_tx, mut second_started) = tokio::sync::oneshot::channel();
        let second_tx = Mutex::new(Some(second_tx));
        let second_progress = Arc::new(move |event: ScanEvent| {
            if let ScanEvent::Started { scan_id } = event {
                second_tx
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(scan_id)
                    .unwrap();
            }
        });
        let second_service = service.clone();
        let second = tokio::spawn(async move {
            second_service
                .start_scan_complete(ScanRequest::default(), second_progress)
                .await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut second_started)
                .await
                .is_err(),
            "a second window started another generation before the first could publish"
        );
        assert!(service.get_last_scan().is_none());
        release_first.send(()).unwrap();
        let first_result = first
            .await
            .unwrap()
            .expect("the original scan must publish");
        assert_eq!(first_result.result.scan_id, first_id);
        assert!(!first_result.result.cancelled);
        let second_id = second_started.await.unwrap();
        let second_result = second
            .await
            .unwrap()
            .expect("the queued window must publish");
        assert_eq!(second_result.result.scan_id, second_id);
        assert!(!second_result.result.cancelled);
        assert_eq!(service.get_last_scan().unwrap().result.scan_id, second_id);
        assert!(service.scan_cancellations.signal(&first_id).is_none());
        assert!(service.scan_cancellations.signal(&second_id).is_none());
    }

    #[tokio::test]
    async fn a_new_window_waits_until_a_continuation_has_published() {
        let service = isolated_scan_service();
        let initial = service
            .start_scan(ScanRequest::default(), Arc::new(|_: ScanEvent| {}))
            .await
            .unwrap();
        let ScanDiscovery::Paused { continuation_id } = initial.discovery else {
            panic!("the bounded scan must retain a continuation")
        };
        let (progress, started, release) = held_scan_progress();
        let resume_service = service.clone();
        let resume = tokio::spawn(async move {
            resume_service
                .resume_scan(
                    ResumeScanRequest {
                        scan_id: initial.result.scan_id,
                        continuation_id,
                    },
                    progress,
                )
                .await
        });
        started.await.unwrap();
        let (next_tx, mut next_started) = tokio::sync::oneshot::channel();
        let next_tx = Mutex::new(Some(next_tx));
        let next_service = service.clone();
        let next = tokio::spawn(async move {
            next_service
                .start_scan_complete(
                    ScanRequest::default(),
                    Arc::new(move |event: ScanEvent| {
                        if let ScanEvent::Started { scan_id } = event {
                            next_tx
                                .lock()
                                .unwrap()
                                .take()
                                .unwrap()
                                .send(scan_id)
                                .unwrap();
                        }
                    }),
                )
                .await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut next_started)
                .await
                .is_err(),
            "a new start invalidated a running continuation"
        );
        assert!(
            service.scan_store.get().is_none(),
            "running discovery cannot authorize cleanup"
        );
        release.send(()).unwrap();
        let resumed = resume
            .await
            .unwrap()
            .expect("the continuation must publish");
        assert_eq!(resumed.discovery, ScanDiscovery::Exhausted);
        assert_eq!(resumed.result.categories.len(), 4);
        let next_id = next_started.await.unwrap();
        let current = next.await.unwrap().expect("the next scan must publish");
        assert_eq!(current.result.scan_id, next_id);
        assert_eq!(service.scan_store.get().unwrap().scan_id, next_id);
    }

    #[tokio::test]
    async fn cancelling_the_active_scan_does_not_cancel_the_waiting_window() {
        let service = isolated_scan_service();
        let (progress, started, release) = held_scan_progress();
        let active_service = service.clone();
        let active = tokio::spawn(async move {
            active_service
                .start_scan_complete(ScanRequest::default(), progress)
                .await
        });
        let active_id = started.await.unwrap();
        let waiting_service = service.clone();
        let waiting = tokio::spawn(async move {
            waiting_service
                .start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {}))
                .await
        });
        service.cancel_scan(&active_id).unwrap();
        release.send(()).unwrap();
        let cancelled = active
            .await
            .unwrap()
            .expect("a cancelled scan must publish its stopped result");
        assert!(cancelled.result.cancelled);
        assert!(matches!(cancelled.discovery, ScanDiscovery::Stopped { .. }));
        let current = waiting
            .await
            .unwrap()
            .expect("the waiting scan must finish");
        assert!(!current.result.cancelled);
        assert_eq!(current.discovery, ScanDiscovery::Exhausted);
        assert!(service.scan_cancellations.signal(&active_id).is_none());
        assert!(service
            .scan_cancellations
            .signal(&current.result.scan_id)
            .is_none());
    }

    #[tokio::test]
    async fn a_panicking_scan_retires_its_handle_and_releases_the_session() {
        let service = isolated_scan_service();
        let started_id = Arc::new(Mutex::new(None));
        let captured = started_id.clone();
        let failed = service
            .start_scan_complete(
                ScanRequest::default(),
                Arc::new(move |event: ScanEvent| {
                    if let ScanEvent::Started { scan_id } = event {
                        *captured.lock().unwrap() = Some(scan_id);
                        panic!("controlled scan progress failure");
                    }
                }),
            )
            .await;
        assert!(failed.is_err());
        let failed_id = started_id.lock().unwrap().clone().unwrap();
        assert!(
            service.scan_cancellations.signal(&failed_id).is_none(),
            "a failed worker left a cancellation handle"
        );
        assert!(service.scan_store.get().is_none());
        let recovered = service
            .start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {}))
            .await
            .unwrap();
        assert_eq!(recovered.discovery, ScanDiscovery::Exhausted);
        assert_eq!(
            service.scan_store.get().unwrap().scan_id,
            recovered.result.scan_id
        );
    }

    #[tokio::test]
    async fn an_abandoned_caller_keeps_the_worker_serialized_and_cancellable() {
        let service = isolated_scan_service();
        let (progress, started, release) = held_scan_progress();
        let active_service = service.clone();
        let active = tokio::spawn(async move {
            active_service
                .start_scan_complete(ScanRequest::default(), progress)
                .await
        });
        let active_id = started.await.unwrap();
        active.abort();
        assert!(active.await.unwrap_err().is_cancelled());

        let (next_tx, mut next_started) = tokio::sync::oneshot::channel();
        let next_tx = Mutex::new(Some(next_tx));
        let waiting_service = service.clone();
        let waiting = tokio::spawn(async move {
            waiting_service
                .start_scan_complete(
                    ScanRequest::default(),
                    Arc::new(move |event: ScanEvent| {
                        if let ScanEvent::Started { scan_id } = event {
                            next_tx
                                .lock()
                                .unwrap()
                                .take()
                                .unwrap()
                                .send(scan_id)
                                .unwrap();
                        }
                    }),
                )
                .await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut next_started)
                .await
                .is_err(),
            "dropping the caller released a still-running worker's lifecycle"
        );
        assert!(service.scan_cancellations.signal(&active_id).is_some());
        service.cancel_scan(&active_id).unwrap();
        release.send(()).unwrap();

        let next_id = next_started.await.unwrap();
        let current = waiting.await.unwrap().unwrap();
        assert_eq!(current.result.scan_id, next_id);
        assert!(!current.result.cancelled);
        assert_eq!(current.discovery, ScanDiscovery::Exhausted);
        assert_eq!(service.scan_store.get().unwrap().scan_id, next_id);
        assert!(service.scan_cancellations.signal(&active_id).is_none());
        assert!(service.scan_cancellations.signal(&next_id).is_none());
    }

    #[tokio::test]
    async fn an_active_scan_keeps_unrelated_storage_reads_concurrent() {
        let service = isolated_scan_service();
        let (progress, started, release) = held_scan_progress();
        let active_service = service.clone();
        let active = tokio::spawn(async move {
            active_service
                .start_scan_complete(ScanRequest::default(), progress)
                .await
        });
        started.await.unwrap();

        let read_service = service.clone();
        let read = tokio::spawn(async move {
            let permit = read_service.budgets.acquire_storage_read().await?;
            let operation_gate = read_service.operation_gate.clone();
            crate::blocking::run_blocking(
                move || {
                    let _permit = permit;
                    Ok::<_, String>(operation_gate.run_read(|| 42))
                },
                "Fixture read worker panicked",
            )
            .await
        });
        let observed = tokio::time::timeout(std::time::Duration::from_secs(1), read)
            .await
            .expect("a scan must not exclude unrelated storage readers")
            .unwrap()
            .unwrap();
        assert_eq!(observed, 42);
        assert!(service.get_last_scan().is_none());
        release.send(()).unwrap();
        assert_eq!(
            active.await.unwrap().unwrap().discovery,
            ScanDiscovery::Exhausted
        );
    }

    #[tokio::test]
    async fn a_queued_scan_does_not_reserve_the_remaining_storage_read_permit() {
        let service = isolated_scan_service();
        let (progress, started, release) = held_scan_progress();
        let active_service = service.clone();
        let active = tokio::spawn(async move {
            active_service
                .start_scan_complete(ScanRequest::default(), progress)
                .await
        });
        let active_id = started.await.unwrap();

        let mut waiting = Box::pin(
            service.start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {})),
        );
        // Poll once so this request is actually waiting behind the active
        // scan, rather than relying on a spawned task's scheduling order.
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(waiting.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;

        let read_service = service.clone();
        let mut read = tokio::spawn(async move {
            let permit = read_service.budgets.acquire_storage_read().await?;
            let operation_gate = read_service.operation_gate.clone();
            crate::blocking::run_blocking(
                move || {
                    let _permit = permit;
                    Ok::<_, String>(operation_gate.run_read(|| 42))
                },
                "Fixture read worker panicked",
            )
            .await
        });
        let before_release =
            tokio::time::timeout(std::time::Duration::from_secs(1), &mut read).await;
        assert!(service.scan_cancellations.signal(&active_id).is_some());
        release.send(()).unwrap();
        assert_eq!(
            active.await.unwrap().unwrap().discovery,
            ScanDiscovery::Exhausted
        );
        assert_eq!(waiting.await.unwrap().discovery, ScanDiscovery::Exhausted);
        match before_release {
            Ok(observed) => assert_eq!(observed.unwrap().unwrap(), 42),
            Err(_) => {
                // Finish every fixture worker before reporting the regression.
                assert_eq!(read.await.unwrap().unwrap(), 42);
                panic!("a queued scan consumed the shared read permit before it could execute");
            }
        }
    }

    #[tokio::test]
    async fn scan_admission_is_bounded_and_dropping_a_waiter_returns_its_slot() {
        let service = isolated_scan_service();
        let (progress, started, release) = held_scan_progress();
        let active_service = service.clone();
        let active = tokio::spawn(async move {
            active_service
                .start_scan_complete(ScanRequest::default(), progress)
                .await
        });
        let active_id = started.await.unwrap();
        let mut waiting = (1..CleanupService::MAX_ADMITTED_SCAN_SESSIONS)
            .map(|_| {
                Box::pin(
                    service
                        .start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {})),
                )
            })
            .collect::<Vec<_>>();
        for request in &mut waiting {
            std::future::poll_fn(|context| {
                assert!(std::future::Future::poll(request.as_mut(), context).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
        }
        let overflow = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            service.start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {})),
        )
        .await
        .expect("scan admission overflow must fail without joining a wait queue");
        assert_eq!(
            overflow.unwrap_err(),
            "Scan queue is full; try again after the current scan finishes."
        );
        assert!(!service
            .scan_cancellations
            .signal(&active_id)
            .unwrap()
            .load(std::sync::atomic::Ordering::SeqCst));

        drop(waiting.pop().unwrap());
        let mut replacement = Box::pin(
            service.start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {})),
        );
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(replacement.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(waiting);
        release.send(()).unwrap();
        let original = active.await.unwrap().unwrap();
        assert_eq!(original.result.scan_id, active_id);
        assert!(!original.result.cancelled);
        let current = replacement.await.unwrap();
        assert_eq!(current.discovery, ScanDiscovery::Exhausted);
        assert!(service.scan_cancellations.signal(&active_id).is_none());
    }

    #[tokio::test]
    async fn abandoning_a_scan_waiting_for_the_read_budget_preserves_current_authority() {
        let service = isolated_scan_service();
        let initial = service
            .start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {}))
            .await
            .unwrap();
        let mut held_reads = Vec::new();
        for _ in 0..ExecutionBudgets::storage_read_permits() {
            held_reads.push(service.budgets.acquire_storage_read().await.unwrap());
        }
        let mut waiting = Box::pin(service.start_scan_complete(
            ScanRequest::default(),
            Arc::new(|_: ScanEvent| panic!("a scan without a read permit must not begin")),
        ));
        std::future::poll_fn(|context| {
            assert!(std::future::Future::poll(waiting.as_mut(), context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(service.get_last_scan().unwrap(), initial);
        assert_eq!(
            service.scan_store.get().unwrap().scan_id,
            initial.result.scan_id
        );
        drop(waiting);
        drop(held_reads);

        let current = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            service.start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {})),
        )
        .await
        .expect("abandoning a pre-dispatch scan must release its lifecycle")
        .unwrap();
        assert_eq!(current.discovery, ScanDiscovery::Exhausted);
        assert_ne!(current.result.scan_id, initial.result.scan_id);
    }

    #[tokio::test]
    async fn a_panicking_continuation_retires_its_lease_and_handle() {
        let service = isolated_scan_service();
        let initial = service
            .start_scan(ScanRequest::default(), Arc::new(|_: ScanEvent| {}))
            .await
            .unwrap();
        let ScanDiscovery::Paused { continuation_id } = initial.discovery else {
            panic!("the bounded scan must retain a continuation")
        };
        let request = ResumeScanRequest {
            scan_id: initial.result.scan_id.clone(),
            continuation_id,
        };
        let started_id = Arc::new(Mutex::new(None));
        let captured = started_id.clone();
        let failed = service
            .resume_scan(
                request.clone(),
                Arc::new(move |event: ScanEvent| {
                    if let ScanEvent::Started { scan_id } = event {
                        *captured.lock().unwrap() = Some(scan_id);
                        panic!("controlled continuation progress failure");
                    }
                }),
            )
            .await;
        assert!(failed.is_err());
        let failed_id = started_id.lock().unwrap().clone().unwrap();
        assert!(service.scan_cancellations.signal(&failed_id).is_none());
        let stopped = service.get_last_scan().unwrap();
        assert_eq!(stopped.result.scan_id, initial.result.scan_id);
        assert_eq!(
            stopped.discovery,
            ScanDiscovery::Stopped {
                reason: "The scan worker stopped before publication.".into(),
            },
            "the failed continuation must retire its running lease"
        );
        assert!(service.scan_store.get().is_none());
        assert!(service
            .resume_scan(request, Arc::new(|_: ScanEvent| {}))
            .await
            .is_err());
        let recovered = service
            .start_scan_complete(ScanRequest::default(), Arc::new(|_: ScanEvent| {}))
            .await
            .unwrap();
        assert_eq!(recovered.discovery, ScanDiscovery::Exhausted);
    }

    #[tokio::test]
    async fn resumed_developer_scan_keeps_the_original_provider_exclusions() {
        let environment = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let registry = Arc::new(SignatureRegistry::new());
        let lifecycle = Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new()));
        let owners = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let cache_providers = Arc::new(RecordingCacheProviders::default());
        let scan_service = Arc::new(ScanService::new_with_cache_providers(
            registry.clone(),
            lifecycle.clone(),
            owners.clone(),
            cache_providers.clone(),
            environment.clone(),
        ));
        let service = CleanupService::new(
            scan_service,
            Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup())),
            Arc::new(ScanStore::new()),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            environment,
            registry,
            Arc::new(DockerStatusCache::new()),
            lifecycle,
            owners,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        );
        let progress: Arc<dyn ScanProgressSink> = Arc::new(|_: ScanEvent| {});
        let excluded = vec!["dev.uv.cache".to_string()];

        let first = service
            .start_scan(
                ScanRequest {
                    categories: Some(vec![Category::System, Category::Ai, Category::Developer]),
                    excluded_signatures: excluded.clone(),
                    intensive_cleanup: false,
                },
                progress.clone(),
            )
            .await
            .unwrap();
        assert!(cache_providers.excluded.lock().unwrap().is_empty());
        let ScanDiscovery::Paused { continuation_id } = first.discovery else {
            panic!("the developer category should remain for the resumed slice")
        };

        let completed = service
            .resume_scan(
                ResumeScanRequest {
                    scan_id: first.result.scan_id,
                    continuation_id,
                },
                progress,
            )
            .await
            .unwrap();

        assert_eq!(completed.discovery, ScanDiscovery::Exhausted);
        assert_eq!(
            cache_providers.excluded.lock().unwrap().as_slice(),
            &[excluded]
        );
    }

    /// A cancel requested while a scan runs stops it, and the scan says so:
    /// the result is partial with a cancellation reason, the categories that
    /// had not started are absent, and the flag distinguishes it from any other
    /// incomplete scan.
    #[tokio::test]
    async fn cancelling_a_running_scan_stops_it_and_the_result_says_so() {
        struct CancelOnFirstItem {
            service: Arc<CleanupService>,
        }

        impl ScanProgressSink for CancelOnFirstItem {
            fn emit(&self, event: crate::models::ScanEvent) {
                if let crate::models::ScanEvent::Started { scan_id } = &event {
                    // The cancel is requested as soon as the scan states which
                    // scan it is, exactly as the command does from the UI.
                    self.service
                        .cancel_scan(scan_id)
                        .expect("a cancel request is accepted");
                }
            }
        }

        let fixture = tempfile::tempdir().unwrap();
        let first_root = fixture.path().join("first-cache");
        let second_root = fixture.path().join("second-cache");
        std::fs::create_dir_all(&first_root).unwrap();
        std::fs::create_dir_all(&second_root).unwrap();
        std::fs::write(first_root.join("data.bin"), vec![1u8; 512]).unwrap();
        std::fs::write(second_root.join("data.bin"), vec![2u8; 512]).unwrap();

        let env = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let mut registry = SignatureRegistry::new();
        for (id, name, path) in [
            ("test.cancel.first", "First cache", first_root),
            ("test.cancel.second", "Second cache", second_root),
        ] {
            registry.register(Signature {
                deletion_disposition: None,
                id: id.to_string(),
                name: name.to_string(),
                category: Category::Developer,
                family: Default::default(),
                risk: RiskTier::Safe,
                strategy: CleanStrategy::DeleteDirectory,
                paths: vec![path.to_string_lossy().into_owned()],
                exclusions: Vec::new(),
                description: "test-only signature".to_string(),
                min_age_days: None,
                include_prefixes: Vec::new(),
                exclude_prefixes: Vec::new(),
                intensive_only: false,
                platforms: Vec::new(),
                discovery: Default::default(),
                unit: None,
                owner: String::new(),
                priority: 0,
                fail_if_running: Vec::new(),
                provider: String::new(),
                provider_id: None,
                artifact_kind: Default::default(),
                consequence: String::new(),
            });
        }
        let registry = Arc::new(registry);
        let providers = Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new()));
        let owner_providers = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            providers.clone(),
            owner_providers.clone(),
            env.clone(),
        ));
        let service = Arc::new(CleanupService::new(
            scan_service,
            Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup())),
            Arc::new(ScanStore::new()),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            env.clone(),
            registry,
            Arc::new(DockerStatusCache::new()),
            providers,
            owner_providers,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        ));

        let result = service
            .start_scan(
                ScanRequest {
                    categories: Some(vec![Category::Developer]),
                    excluded_signatures: Vec::new(),
                    intensive_cleanup: false,
                },
                Arc::new(CancelOnFirstItem {
                    service: service.clone(),
                }),
            )
            .await
            .expect("a cancelled scan still returns its partial result");

        assert!(
            result.result.cancelled,
            "the result states that it was cancelled: {result:?}"
        );
        assert_eq!(result.result.quality, ObservationQuality::Partial);
        assert!(result
            .result
            .incomplete_reasons
            .iter()
            .any(|reason| reason.contains("cancelled")));
        assert!(
            result.result.categories.is_empty(),
            "the cancelled scan stopped before finishing any category: {result:?}"
        );

        // The finished scan keeps no cancellation handle: requesting it again
        // is a no-op rather than a second cancel of something else.
        assert!(service.cancel_scan(&result.result.scan_id).is_ok());
    }

    #[tokio::test]
    async fn a_bounded_scan_resumes_forward_and_publishes_one_merged_snapshot() {
        let environment = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let registry = Arc::new(SignatureRegistry::new());
        let lifecycle = Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new()));
        let owners = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            lifecycle.clone(),
            owners.clone(),
            environment.clone(),
        ));
        let service = CleanupService::new(
            scan_service,
            Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup())),
            Arc::new(ScanStore::new()),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            environment,
            registry,
            Arc::new(DockerStatusCache::new()),
            lifecycle,
            owners,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        );
        let progress: Arc<dyn ScanProgressSink> = Arc::new(|_: ScanEvent| {});

        let first = service
            .start_scan(ScanRequest::default(), progress.clone())
            .await
            .expect("the first bounded pass publishes");
        let ScanDiscovery::Paused { continuation_id } = first.discovery else {
            panic!("the default four-category scan should pause after two categories")
        };
        assert_eq!(first.result.categories.len(), 2);
        let freshness_anchor = first.result.finished_at;

        let completed = service
            .resume_scan(
                ResumeScanRequest {
                    scan_id: first.result.scan_id,
                    continuation_id,
                },
                progress.clone(),
            )
            .await
            .expect("the retained pass resumes");

        assert_eq!(completed.discovery, ScanDiscovery::Exhausted);
        assert_eq!(completed.result.categories.len(), 4);
        assert_eq!(completed.result.finished_at, freshness_anchor);
        assert_eq!(
            completed
                .result
                .categories
                .iter()
                .map(|category| category.category)
                .collect::<Vec<_>>(),
            vec![
                Category::Ai,
                Category::Developer,
                Category::Container,
                Category::System,
            ]
        );

        let one_action = service
            .start_scan_complete(ScanRequest::default(), progress)
            .await
            .expect("one scan action exhausts all categories");
        assert_eq!(one_action.discovery, ScanDiscovery::Exhausted);
        assert_eq!(one_action.result.categories.len(), 4);
    }

    #[tokio::test]
    async fn cleanup_service_plan_and_execution_lifecycle() {
        let env = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let registry = Arc::new(SignatureRegistry::new());
        let owner_providers = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers.clone(),
            env.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup()));
        let scan_store = Arc::new(ScanStore::new());
        let operation_gate = StorageOperationGate::default();
        let budgets = Arc::new(ExecutionBudgets::new());
        let docker_cache = Arc::new(DockerStatusCache::new());
        let capabilities = Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current()));

        let service = CleanupService::new(
            scan_service,
            plan_store.clone(),
            scan_store.clone(),
            operation_gate,
            budgets,
            env,
            registry,
            docker_cache,
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers,
            Arc::new(neati_platform::MockTrashBackend::new()),
            capabilities,
        );

        // Populate a scan
        let item = make_test_item("item1", RiskTier::Safe, 500);
        let scan = make_test_scan(vec![item]);
        scan_store.set(scan.clone());

        assert_eq!(service.get_last_scan().unwrap().result.scan_id, "scan_123");

        // Requesting plan with unknown item fails
        let err = service
            .create_delete_plan("scan_123".to_string(), vec!["unknown".to_string()])
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn a_confirmation_required_provider_cannot_execute_without_confirmation() {
        use crate::cleaner::providers::test_support::StatedProvider;

        let env = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let mut registry = SignatureRegistry::new();
        registry.register(Signature {
            deletion_disposition: None,
            id: "test.stated.store".to_string(),
            name: "Stated Store".to_string(),
            category: Category::System,
            family: Default::default(),
            risk: RiskTier::Manual,
            strategy: CleanStrategy::LifecycleProvider,
            paths: Vec::new(),
            exclusions: Vec::new(),
            description: "A provider-owned store.".to_string(),
            min_age_days: None,
            include_prefixes: Vec::new(),
            exclude_prefixes: Vec::new(),
            intensive_only: false,
            platforms: vec![crate::models::PlatformKind::current()],
            discovery: Default::default(),
            unit: None,
            owner: String::new(),
            priority: 0,
            fail_if_running: Vec::new(),
            provider: "Stated Owner".to_string(),
            provider_id: Some("test.stated".to_string()),
            artifact_kind: Default::default(),
            consequence: String::new(),
        });
        let registry = Arc::new(registry);
        let providers = Arc::new(crate::cleaner::LifecycleProviderRegistry::new(vec![
            StatedProvider::holding(2_048, 1).shared(),
        ]));
        let owner_providers = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            providers.clone(),
            owner_providers.clone(),
            env.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup()));
        let scan_store = Arc::new(ScanStore::new());
        let service = CleanupService::new(
            scan_service,
            plan_store,
            scan_store.clone(),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            env.clone(),
            registry.clone(),
            Arc::new(DockerStatusCache::new()),
            providers.clone(),
            owner_providers,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        );

        let items = providers.scan_items(&registry, Category::System, false, &[], &env);
        assert_eq!(items.len(), 1);
        assert!(items[0].requires_confirmation);
        assert_eq!(
            items[0].disposition.eligibility,
            crate::models::CleanupEligibility::Reviewable
        );
        scan_store.set(make_test_scan(items));

        let preview = service
            .create_delete_plan(
                "scan_123".to_string(),
                vec!["test.stated.store".to_string()],
            )
            .await
            .expect("provider selection creates a plan");
        assert!(preview.requires_confirmation);
        assert!(preview.targets[0].requires_confirmation);

        let progress: Arc<dyn CleanupProgressSink> = Arc::new(|_| {});
        let refused = service
            .execute_clean(preview.id, false, progress.clone())
            .await
            .expect_err("confirmation-required plan must fail closed");
        assert!(
            refused.message.contains("explicit confirmation"),
            "{}",
            refused.message
        );

        let confirmed_preview = service
            .create_delete_plan(
                "scan_123".to_string(),
                vec!["test.stated.store".to_string()],
            )
            .await
            .expect("the unconfirmed refusal leaves the scan available for review");
        let result = service
            .execute_clean(confirmed_preview.id, true, progress)
            .await
            .expect("a confirmed provider action executes");
        assert_eq!(result.failed_count, 0);
        assert_eq!(result.total_reclaimed_bytes, 2_048);
    }

    #[tokio::test]
    async fn a_consumed_plan_cannot_be_replayed_through_the_service() {
        let fixture = tempfile::tempdir().unwrap();
        let cache = fixture.path().join("fixture-cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("payload.bin"), vec![7u8; 512]).unwrap();

        let env = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let mut registry = SignatureRegistry::new();
        registry.register(Signature {
            deletion_disposition: None,
            id: "test_sig".to_string(),
            name: "Fixture cache".to_string(),
            category: Category::System,
            family: Default::default(),
            risk: RiskTier::Safe,
            strategy: CleanStrategy::DeleteDirectory,
            paths: vec![cache.to_string_lossy().to_string()],
            description: "test-only signature".to_string(),
            min_age_days: None,
            include_prefixes: Vec::new(),
            exclude_prefixes: Vec::new(),
            intensive_only: false,
            platforms: Vec::new(),
            discovery: Default::default(),
            unit: None,
            owner: String::new(),
            priority: 0,
            fail_if_running: Vec::new(),
            provider: String::new(),
            provider_id: None,
            artifact_kind: Default::default(),
            consequence: String::new(),
            exclusions: Vec::new(),
        });
        let registry = Arc::new(registry);

        let owner_providers = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers.clone(),
            env.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup()));
        let scan_store = Arc::new(ScanStore::new());
        let service = CleanupService::new(
            scan_service,
            plan_store,
            scan_store.clone(),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            env,
            registry,
            Arc::new(DockerStatusCache::new()),
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        );

        let published = service
            .start_scan_complete(
                ScanRequest {
                    categories: Some(vec![Category::System]),
                    ..ScanRequest::default()
                },
                Arc::new(|_: ScanEvent| {}),
            )
            .await
            .expect("fixture scan completes");
        assert_eq!(published.discovery, ScanDiscovery::Exhausted);
        let item = published
            .result
            .categories
            .iter()
            .flat_map(|category| &category.items)
            .find(|item| item.signature_id == "test_sig")
            .expect("fixture is discovered");
        assert_eq!(
            item.disposition.eligibility,
            CleanupEligibility::AutoCleanable
        );

        let preview = service
            .create_delete_plan(published.result.scan_id.clone(), vec![item.id.clone()])
            .await
            .expect("a reviewed item creates a plan");
        let progress: Arc<dyn CleanupProgressSink> = Arc::new(|_| {});

        let first = service
            .execute_clean(preview.id, false, progress.clone())
            .await;
        assert!(
            first.is_ok(),
            "the first execution of a plan runs: {:?}",
            first.err()
        );
        assert!(!cache.exists(), "the plan's target was deleted");

        // One-shot means the same plan ID cannot authorize a second mutation,
        // through this service or any other caller.
        let replay = service.execute_clean(preview.id, false, progress).await;
        let error = replay.expect_err("a consumed plan must be refused");
        assert_eq!(error.scope, CleanupFailureScope::PlanUnavailable);
        assert_eq!(error.reason, CleanFailureReason::PlanUnavailable);
        assert!(
            error.message.contains("Review it again"),
            "unexpected error: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn quick_clean_executes_verified_items_from_a_partial_scan() {
        let fixture = tempfile::tempdir().unwrap();
        let cache = fixture.path().join("fixture-cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("payload.bin"), vec![9u8; 256]).unwrap();

        let env = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let mut registry = SignatureRegistry::new();
        registry.register(Signature {
            deletion_disposition: None,
            id: "test_sig".to_string(),
            name: "Fixture cache".to_string(),
            category: Category::System,
            family: Default::default(),
            risk: RiskTier::Safe,
            strategy: CleanStrategy::DeleteDirectory,
            paths: vec![cache.to_string_lossy().to_string()],
            description: "test-only signature".to_string(),
            min_age_days: None,
            include_prefixes: Vec::new(),
            exclude_prefixes: Vec::new(),
            intensive_only: false,
            platforms: Vec::new(),
            discovery: Default::default(),
            unit: None,
            owner: String::new(),
            priority: 0,
            fail_if_running: Vec::new(),
            provider: String::new(),
            provider_id: None,
            artifact_kind: Default::default(),
            consequence: String::new(),
            exclusions: Vec::new(),
        });
        let registry = Arc::new(registry);

        let owner_providers = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers.clone(),
            env.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup()));
        let scan_store = Arc::new(ScanStore::new());
        let service = CleanupService::new(
            scan_service,
            plan_store,
            scan_store.clone(),
            StorageOperationGate::default(),
            Arc::new(ExecutionBudgets::new()),
            env,
            registry,
            Arc::new(DockerStatusCache::new()),
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers,
            Arc::new(neati_platform::MockTrashBackend::new()),
            Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current())),
        );

        // A partial scan is the state a cancelled or bounded scan leaves behind:
        // some items were observed, and the machine as a whole was not.
        let mut item = ScanItem::mock(
            "item1",
            "test_sig",
            "Fixture cache",
            Category::System,
            RiskTier::Safe,
            cache.to_string_lossy().to_string(),
            FileSize::new(256, Some(256)),
            1,
        );
        item.disposition = item.derive_disposition();
        scan_store.set(make_test_scan_with_quality(
            vec![item],
            ObservationQuality::Partial,
        ));

        let progress: Arc<dyn CleanupProgressSink> = Arc::new(|_| {});
        let settings = NeatiSettings::default();
        for (scan_id, ids) in [
            ("older_scan", vec!["item1".to_string()]),
            ("scan_123", vec!["forged_item".to_string()]),
            ("scan_123", vec!["item1".to_string(), "item1".to_string()]),
        ] {
            assert!(service
                .reviewed_quick_clean_safe(scan_id.into(), ids, &settings, progress.clone())
                .await
                .is_err());
            assert!(cache.join("payload.bin").exists());
        }
        let result = service
            .reviewed_quick_clean_safe("scan_123".into(), vec!["item1".into()], &settings, progress)
            .await
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert!(result.items[0].success);
        assert!(!cache.exists(), "the verified fixture was removed");
        assert!(
            service
                .quick_clean_safe(&settings, Arc::new(|_| {}))
                .await
                .is_err(),
            "a consumed scan cannot authorize another cleanup"
        );
    }

    #[tokio::test]
    async fn quick_clean_safe_handles_empty_candidates_gracefully() {
        let env = Arc::new(
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(
                if PathFlavor::current().is_windows() {
                    r"Z:\NeatiFixtureHome"
                } else {
                    "/neati-fixture-home"
                },
            ),
        );
        let registry = Arc::new(SignatureRegistry::new());
        let owner_providers = Arc::new(crate::cleaner::OwnerProviderRegistry::new(Vec::new()));
        let scan_service = Arc::new(ScanService::new(
            registry.clone(),
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers.clone(),
            env.clone(),
        ));
        let plan_store = Arc::new(PlanStore::new(crate::services::PlanLifecycle::cleanup()));
        let scan_store = Arc::new(ScanStore::new());
        let operation_gate = StorageOperationGate::default();
        let budgets = Arc::new(ExecutionBudgets::new());
        let docker_cache = Arc::new(DockerStatusCache::new());
        let capabilities = Arc::new(TestCapabilitiesProvider(PlatformCapabilities::current()));

        let service = CleanupService::new(
            scan_service,
            plan_store,
            scan_store.clone(),
            operation_gate,
            budgets,
            env,
            registry,
            docker_cache,
            Arc::new(crate::cleaner::LifecycleProviderRegistry::new(Vec::new())),
            owner_providers,
            Arc::new(neati_platform::MockTrashBackend::new()),
            capabilities,
        );

        // A running owner remains outside direct cleanup.
        let mut rebuild_item = make_test_item("item_rebuild", RiskTier::Rebuild, 300);
        rebuild_item.owner_running = true;
        rebuild_item.rederive_disposition();
        let scan = make_test_scan(vec![rebuild_item]);
        scan_store.set(scan);

        let progress = Arc::new(|_| {});
        let settings = NeatiSettings::default();
        let result = service.quick_clean_safe(&settings, progress).await.unwrap();

        assert_eq!(result.total_reclaimed_bytes, 0);
        assert_eq!(result.items.len(), 0);
    }
}
