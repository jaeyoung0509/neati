use super::{CancellationRegistry, OneShotPlan, PlanLifecycle, PlanStore};
use neati_core::{
    application::dto::empty_trash::{EmptyTrashItemResult, EmptyTrashPreview, EmptyTrashResult},
    domain::cleanup::empty_trash::TrashSnapshot,
};
use std::sync::{atomic::AtomicBool, Arc};
use uuid::Uuid;
struct EmptyTrashPlan {
    id: Uuid,
    created_at: u64,
    snapshot: TrashSnapshot,
}
impl OneShotPlan for EmptyTrashPlan {
    fn plan_id(&self) -> Uuid {
        self.id
    }
    fn created_at(&self) -> u64 {
        self.created_at
    }
}
pub(super) struct EmptyTrashState {
    plans: PlanStore<EmptyTrashPlan>,
    cancellations: CancellationRegistry,
}
impl Default for EmptyTrashState {
    fn default() -> Self {
        Self {
            plans: PlanStore::new(PlanLifecycle::trash()),
            cancellations: CancellationRegistry::for_scans(),
        }
    }
}
impl super::CleanupService {
    pub async fn preview_empty_trash(&self) -> Result<EmptyTrashPreview, String> {
        let environment = self.environment.clone();
        let gate = self.operation_gate.clone();
        let state = self.empty_trash.clone();
        crate::blocking::run_blocking(
            move || {
                gate.run_read(|| {
                    let snapshot = neati_platform::empty_trash::inventory(&environment)?;
                    let id = Uuid::new_v4();
                    let now = super::cleanup_service::unix_timestamp();
                    let mut observed_identities = std::collections::HashSet::new();
                    let preview = EmptyTrashPreview {
                        plan_id: id.to_string(),
                        scope: snapshot.home.join(".Trash").to_string_lossy().into_owned(),
                        items: snapshot
                            .entries
                            .iter()
                            .filter(|e| e.relative.components().count() == 1)
                            .map(|e| e.relative.to_string_lossy().into_owned())
                            .collect(),
                        observed_bytes: snapshot
                            .entries
                            .iter()
                            .filter(|e| observed_identities.insert(e.identity))
                            .fold(0u64, |sum, e| sum.saturating_add(e.allocated_bytes)),
                        entry_count: snapshot.entries.len(),
                    };
                    state
                        .plans
                        .insert(
                            EmptyTrashPlan {
                                id,
                                created_at: now,
                                snapshot,
                            },
                            now,
                        )
                        .map_err(|e| e.to_string())?;
                    // Register before returning the preview, so an immediate Stop cannot be lost.
                    state
                        .cancellations
                        .register(id.to_string(), Arc::new(AtomicBool::new(false)));
                    Ok(preview)
                })
            },
            "Trash preview worker panicked",
        )
        .await
    }
    pub async fn execute_empty_trash(
        &self,
        plan_id: String,
        confirmed: bool,
    ) -> Result<EmptyTrashResult, String> {
        if !confirmed {
            return Err(
                "Review the home Trash contents and confirm permanent removal first".into(),
            );
        }
        self.platform_capabilities
            .capabilities()
            .require(
                crate::models::PlatformFeature::Cleanup,
                crate::models::CapabilityAccess::Mutate,
            )
            .map_err(|e| e.to_string())?;
        let id = Uuid::parse_str(&plan_id).map_err(|e| e.to_string())?;
        let state = self.empty_trash.clone();
        let gate = self.operation_gate.clone();
        let cancellation = state
            .cancellations
            .signal(&plan_id)
            .ok_or("Trash review expired; review it again")?;
        let cleanup_state = state.clone();
        let cleanup_id = plan_id.clone();
        let result = crate::blocking::run_blocking(
            move || {
                gate.run_write(|| {
                    let plan = state
                        .plans
                        .take_valid(id, super::cleanup_service::unix_timestamp())
                        .map_err(|e| e.to_string())?;
                    let outcome =
                        neati_platform::empty_trash::execute(&plan.snapshot, &cancellation);
                    Ok(EmptyTrashResult {
                        removed_bytes: outcome.removed_bytes,
                        removed_entries: outcome.removed_entries,
                        cancelled: outcome.cancelled,
                        items: outcome
                            .items
                            .into_iter()
                            .map(|(path, removed, message)| EmptyTrashItemResult {
                                path,
                                removed,
                                message,
                            })
                            .collect(),
                    })
                })
            },
            "Trash cleanup worker panicked",
        )
        .await;
        cleanup_state.cancellations.remove(&cleanup_id);
        result
    }
    pub fn cancel_empty_trash(&self, plan_id: &str) {
        self.empty_trash.cancellations.request(plan_id);
        // An unstarted review can be revoked immediately. A running worker has
        // already consumed its plan and retains the signalled cancellation Arc.
        if Uuid::parse_str(plan_id).is_ok_and(|id| {
            self.empty_trash
                .plans
                .take_valid(id, super::cleanup_service::unix_timestamp())
                .is_ok()
        }) {
            self.empty_trash.cancellations.remove(plan_id);
        }
    }
}
