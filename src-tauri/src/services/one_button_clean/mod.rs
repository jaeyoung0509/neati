//! One-button Clean as an interface-agnostic use case (#498).
//!
//! The app's Clean button and `neati clean` remove the same set: the caches
//! the backend marks `AutoCleanable` and the build output it preselects in the
//! saved project folders. This use case owns the orchestration — scan both
//! inventories, choose the backend's set, narrow it by reference, remove the
//! chosen units, merge the reports — behind ports, so a terminal, a window or
//! a test drives it the same way and none of them re-implements a rule.

pub mod adapters;
pub mod ports;
pub mod selection;
pub mod sessions;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::models::CleanupEligibility;
use crate::services::ports::{ActivityPort, PortFuture};

use ports::{
    CacheCleanup, CleanOutcome, CleanPreview, CleanRequest, CleanUseCase, OneButtonError,
    ProjectCleanup, RemovalReport, ReviewSummary, UnitFailure,
};
use selection::{cache_candidates, narrow, project_candidates, Candidate, Source};

pub struct OneButtonClean {
    caches: Arc<dyn CacheCleanup>,
    projects: Arc<dyn ProjectCleanup>,
}

impl OneButtonClean {
    pub fn new(caches: Arc<dyn CacheCleanup>, projects: Arc<dyn ProjectCleanup>) -> Self {
        Self { caches, projects }
    }

    async fn preview_inner(
        &self,
        request: CleanRequest,
        activity: Arc<dyn ActivityPort>,
    ) -> Result<CleanPreview, OneButtonError> {
        let mut candidates = Vec::new();
        let mut review = ReviewSummary::default();

        let caches = if request.caches {
            let scan = match request
                .reuse_current
                .then(|| self.caches.current())
                .flatten()
            {
                Some(scan) => scan,
                None => self.caches.scan(activity.clone()).await?,
            };
            let chosen = self.caches.one_button_ids(&scan)?;
            candidates.extend(cache_candidates(&scan, &chosen));
            review.items += scan.eligibility.items(CleanupEligibility::Reviewable);
            review.bytes += scan
                .eligibility
                .cleanable_bytes(CleanupEligibility::Reviewable);
            Some(scan)
        } else {
            None
        };

        let projects = if request.projects {
            let inventory = match request
                .reuse_current
                .then(|| self.projects.current())
                .flatten()
            {
                Some(inventory) => inventory,
                None => self.projects.scan(activity).await?,
            };
            if let Some(result) = &inventory.result {
                for item in result.items.iter().filter(|item| !item.selected_by_default) {
                    review.items += 1;
                    review.bytes += item.allocated_bytes;
                }
                candidates.extend(project_candidates(result));
            }
            Some(inventory)
        } else {
            None
        };

        let chosen = narrow(candidates, &request.only, &request.skip)
            .map_err(OneButtonError::UnknownReferences)?;
        Ok(CleanPreview {
            total_bytes: chosen.iter().map(|candidate| candidate.bytes).sum(),
            chosen,
            review,
            caches,
            projects,
        })
    }

    async fn execute_inner(
        &self,
        preview: &CleanPreview,
        activity: Arc<dyn ActivityPort>,
    ) -> Result<CleanOutcome, OneButtonError> {
        let mut outcome = CleanOutcome::default();

        let cache_ids = backend_ids(&preview.chosen, Source::Cache);
        if let (Some(scan), false) = (&preview.caches, cache_ids.is_empty()) {
            let report = self
                .caches
                .remove(scan.scan_id.clone(), cache_ids, activity.clone())
                .await?;
            merge(&mut outcome, report, &preview.chosen, Source::Cache);
        }

        let project_ids = backend_ids(&preview.chosen, Source::Project);
        let project_scan = preview
            .projects
            .as_ref()
            .and_then(|inventory| inventory.result.as_ref());
        if let (Some(result), false) = (project_scan, project_ids.is_empty()) {
            let report = self
                .projects
                .remove(result.scan_id.clone(), project_ids, activity)
                .await?;
            merge(&mut outcome, report, &preview.chosen, Source::Project);
        }
        Ok(outcome)
    }
}

impl CleanUseCase for OneButtonClean {
    fn preview(
        &self,
        request: CleanRequest,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<CleanPreview, OneButtonError>> {
        Box::pin(self.preview_inner(request, activity))
    }

    fn execute<'a>(
        &'a self,
        preview: &'a CleanPreview,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'a, Result<CleanOutcome, OneButtonError>> {
        Box::pin(self.execute_inner(preview, activity))
    }
}

fn backend_ids(chosen: &[Candidate], source: Source) -> Vec<String> {
    chosen
        .iter()
        .filter(|candidate| candidate.source == source)
        .map(|candidate| candidate.backend_id.clone())
        .collect()
}

/// Folds one port's report into the outcome, naming failures by reference.
fn merge(outcome: &mut CleanOutcome, report: RemovalReport, chosen: &[Candidate], source: Source) {
    outcome.reclaimed_bytes += report.reclaimed_bytes;
    outcome.removed += report.removed.len() as u64;
    outcome.skipped += report.skipped;
    for (backend_id, message) in report.failed {
        let candidate = chosen
            .iter()
            .find(|candidate| candidate.source == source && candidate.backend_id == backend_id);
        outcome.failed.push(UnitFailure {
            reference: candidate
                .map(|candidate| candidate.reference.clone())
                .unwrap_or_else(|| backend_id.clone()),
            name: candidate
                .map(|candidate| candidate.name.clone())
                .unwrap_or(backend_id),
            message,
        });
    }
}
