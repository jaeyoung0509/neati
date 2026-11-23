use super::observation::{RootProgressSink, ScanLimits, TraversalCounters, WalkContext};
use crate::cache_providers::{CacheProviderRegistry, CacheProviderScanner};
use crate::cleaner::LifecycleProviderRegistry;
use crate::docker::DockerAdapter;
use crate::execution_budget::shared_scan_pool;
use crate::models::{
    resolve_unit_overlaps_indexed, Category, CategoryResult, EligibilitySummary,
    ObservationQuality, PathIdentity, RiskTier, ScanEvent, ScanGap, ScanGapKind, ScanItem,
    ScanResult, UnitRelationship,
};
use crate::orbstack::OrbStackAdapter;
use crate::scanner::DirectoryScanner;
use crate::signatures::SignatureRegistry;
use neati_core::domain::scan::ScanSpan;
use neati_core::domain::ScanMetrics;
use neati_platform::PlatformEnvironment;
use std::cell::RefCell;
use std::path::Path;
use std::time::{Instant, SystemTime};
use uuid::Uuid;

use crate::scanner::relationship::{ScanAnchors, ScanRelationships};

pub struct ScanEngine;

fn route_exact_owner_observations(
    categories: &mut [CategoryResult],
    registry: &SignatureRegistry,
    relationships: &RefCell<ScanRelationships>,
) {
    neati_core::domain::scan::overlap::route_exact_observations_to_owners_with(
        categories,
        |observation, owner| {
            let (Some(observation_signature), Some(owner_signature)) = (
                registry.get(&observation.signature_id),
                registry.get(&owner.signature_id),
            ) else {
                return false;
            };
            observation_signature.strategy == crate::models::CleanStrategy::Manual
                && observation_signature.provider_id.is_none()
                && owner_signature.strategy == crate::models::CleanStrategy::OwnerProvider
                && owner_signature
                    .provider_id
                    .as_ref()
                    .is_some_and(|id| !id.is_empty())
                && crate::safety::ToctouGuard::capture(Path::new(&observation.unit.path))
                    .zip(crate::safety::ToctouGuard::capture(Path::new(
                        &owner.unit.path,
                    )))
                    .is_some_and(|(left, right)| {
                        !left.entity().is_unknown() && left.entity().same_entity(right.entity())
                    })
                && relationships.borrow_mut().relationship(observation, owner)
                    == UnitRelationship::Equivalent
        },
    );
}

/// Routes exact owner observations and folds overlapping units, with one
/// relationship snapshot for the whole pass.
///
/// The fold asks only the retained units a candidate shares a path or an
/// identity with (#493); every other retained unit is one the relationship
/// would call distinct, so the verdicts are those of asking every unit.
fn resolve_scan_overlaps(
    categories: &mut [CategoryResult],
    registry: &SignatureRegistry,
) -> neati_core::domain::scan::OverlapReport {
    let relationships = RefCell::new(ScanRelationships::default());
    route_exact_owner_observations(categories, registry, &relationships);
    resolve_unit_overlaps_indexed(
        categories,
        &[],
        PathIdentity::CaseSensitive,
        |candidate, container| {
            scan_unit_relationship(candidate, container, registry, &relationships)
        },
        &ScanAnchors(&relationships),
    )
}

/// The relationship two units have, together with the cleanup policy the scan
/// can act on.
///
/// The structural answer shares `unit_relationship`'s algorithm with the
/// planner. Only this observation pass memoizes filesystem reads; planning
/// always inspects the current filesystem.
fn scan_unit_relationship(
    candidate: &ScanItem,
    container: &ScanItem,
    registry: &SignatureRegistry,
    relationships: &RefCell<ScanRelationships>,
) -> Option<UnitRelationship> {
    match relationships
        .borrow_mut()
        .relationship(candidate, container)
    {
        UnitRelationship::Equivalent
            if candidate.signature_id.starts_with(super::coverage::PREFIX)
                != container.signature_id.starts_with(super::coverage::PREFIX) =>
        {
            Some(UnitRelationship::SharedStorage)
        }
        UnitRelationship::Equivalent => Some(
            if exact_authorities_can_fold(candidate, container, registry) {
                UnitRelationship::Equivalent
            } else {
                // One location through two rules that disagree about what may be
                // done with it: the bytes are counted once and neither rule's
                // operation decides for the other.
                UnitRelationship::EquivalentConflict
            },
        ),
        UnitRelationship::Contained => {
            if containment_policies_are_compatible(candidate, container, registry) {
                Some(UnitRelationship::Contained)
            } else {
                Some(UnitRelationship::AuthorityConflict)
            }
        }
        other => Some(other),
    }
}

fn exact_authorities_can_fold(
    left: &ScanItem,
    right: &ScanItem,
    registry: &SignatureRegistry,
) -> bool {
    if left.signature_id == right.signature_id {
        return true;
    }
    if left.risk == RiskTier::Manual || right.risk == RiskTier::Manual {
        return true;
    }
    let left_specialized = !left.unit.kind.is_filesystem();
    let right_specialized = !right.unit.kind.is_filesystem();
    if left_specialized != right_specialized {
        return true;
    }
    if left_specialized {
        return false;
    }
    containment_policies_are_compatible(left, right, registry)
}

fn containment_policies_are_compatible(
    nested: &ScanItem,
    container: &ScanItem,
    registry: &SignatureRegistry,
) -> bool {
    // A rule whose scope is switched off states nothing about what may be done
    // here: it is discovered for visibility, so it cannot constrain a rule the
    // current settings do run.
    if !nested.gate.is_open() || !container.gate.is_open() {
        return true;
    }
    if !nested.unit.kind.is_filesystem()
        || !container.unit.kind.is_filesystem()
        || nested.risk == RiskTier::Manual
        || container.risk == RiskTier::Manual
    {
        return false;
    }
    if nested.signature_id == container.signature_id {
        return true;
    }
    let (Some(nested_signature), Some(container_signature)) = (
        registry.get(&nested.signature_id),
        registry.get(&container.signature_id),
    ) else {
        return false;
    };
    nested_signature.strategy == container_signature.strategy
        && nested_signature.min_age_days == container_signature.min_age_days
        && same_string_set(
            &nested_signature.exclusions,
            &container_signature.exclusions,
        )
        && same_string_set(
            &nested_signature.fail_if_running,
            &container_signature.fail_if_running,
        )
        && nested_signature.ownership() == container_signature.ownership()
}

fn same_string_set(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut left = left.iter().map(String::as_str).collect::<Vec<_>>();
    let mut right = right.iter().map(String::as_str).collect::<Vec<_>>();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

fn aggregate_quality(
    qualities: impl IntoIterator<Item = ObservationQuality>,
) -> ObservationQuality {
    let mut saw_any = false;
    let mut all_fresh = true;
    let mut all_unavailable = true;
    for quality in qualities {
        saw_any = true;
        all_fresh &= quality == ObservationQuality::Fresh;
        all_unavailable &= quality == ObservationQuality::Unavailable;
    }
    if !saw_any || all_fresh {
        ObservationQuality::Fresh
    } else if all_unavailable {
        ObservationQuality::Unavailable
    } else {
        ObservationQuality::Partial
    }
}

fn add_scan_gap(gaps: &mut Vec<ScanGap>, kind: ScanGapKind, count: u64) {
    if count == 0 {
        return;
    }
    if let Some(existing) = gaps.iter_mut().find(|gap| gap.kind == kind) {
        existing.count = existing.count.saturating_add(count);
    } else {
        gaps.push(ScanGap { kind, count });
    }
}

/// Reports the source's typed diagnostic. Legacy observations stay unknown;
/// prose and the location alone cannot prove a remediation.
pub fn scan_gap_kind(item: &ScanItem) -> Option<ScanGapKind> {
    (item.quality != ObservationQuality::Fresh)
        .then_some(item.inspection_issue.unwrap_or(ScanGapKind::Unknown))
}

fn apply_verified_cache_owner_state(item: &mut ScanItem, running: Option<bool>) {
    match running {
        Some(true) => item.owner_running = true,
        Some(false) => {}
        None => {
            item.quality = ObservationQuality::Unavailable;
            item.incomplete_reason = Some("Owner process state could not be verified".into());
            item.inspection_issue = Some(ScanGapKind::OwnerStateUnknown);
        }
    }
    item.disposition = item.derive_disposition();
    item.is_selected = item.is_pre_selectable();
}

pub(crate) fn apply_signature_owner_state(
    items: &mut [ScanItem],
    signature: &crate::models::Signature,
    processes: &dyn neati_core::domain::cleanup::RunningProcessProbe,
) {
    let guard = signature.process_guard();
    if !guard.is_empty() {
        let running = processes.running(&guard).map(|names| !names.is_empty());
        for item in items {
            apply_verified_cache_owner_state(item, running);
        }
    }
}

/// The retained items and byte populations of one category.
///
/// `total_bytes` is the observed footprint, including blocked/advisory rows;
/// `cleanable_bytes` and the Safe/Rebuild buckets include only eligible bytes.
struct CategoryAccumulator {
    items: Vec<ScanItem>,
    total_bytes: u64,
    cleanable_bytes: u64,
    safe_bytes: u64,
    rebuild_bytes: u64,
    manual_bytes: u64,
    eligibility: EligibilitySummary,
    suppressed_duplicate_count: u64,
    suppressed_duplicate_bytes: u64,
}

impl Default for CategoryAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl CategoryAccumulator {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            total_bytes: 0,
            cleanable_bytes: 0,
            safe_bytes: 0,
            rebuild_bytes: 0,
            manual_bytes: 0,
            eligibility: EligibilitySummary::default(),
            suppressed_duplicate_count: 0,
            suppressed_duplicate_bytes: 0,
        }
    }

    /// Accounts one scanned item, or drops it when it is not worth retaining.
    ///
    /// Returns the retained item so the caller can stream it to the frontend.
    ///
    /// Duplicate and containment decisions require the complete scan: stable
    /// filesystem identity, authority compatibility, and observation coverage
    /// can all cross category boundaries. This accumulator therefore retains
    /// every actionable discovery; the resolver makes the deterministic fold
    /// after every category has finished.
    fn push(&mut self, item: ScanItem) -> Option<&ScanItem> {
        let item = item.with_derived_disposition();
        let bytes = item.cleanable_bytes();
        let observed = item.observed_bytes();
        // An item that is absent, or a complete observation of an empty path,
        // carries nothing a user could act on.
        let is_empty_fresh = item.quality == ObservationQuality::Fresh && observed == 0;
        if !item.exists || is_empty_fresh {
            return None;
        }

        self.total_bytes += observed;
        self.cleanable_bytes += bytes;
        self.eligibility.add(&item);
        if item.disposition.eligibility.is_cleanable() {
            match item.risk {
                RiskTier::Safe => self.safe_bytes += bytes,
                RiskTier::Rebuild => self.rebuild_bytes += bytes,
                RiskTier::Manual => {}
            }
        } else if item.risk == RiskTier::Manual {
            self.manual_bytes += observed;
        }

        if item.quality != ObservationQuality::Fresh || item.incomplete_reason.is_some() {
            // An item whose observation cannot support a cleanup is reported
            // once, through its own `quality` and through the scan's durable
            // `incomplete_reasons`. It never becomes a second, destructive
            // error surface in the middle of a scan that otherwise succeeded.
            crate::diagnostics::log_error(
                "scanner",
                &format!(
                    "Could not fully inspect {}: {}",
                    item.name,
                    item.disposition
                        .reason
                        .as_deref()
                        .or(item.incomplete_reason.as_deref())
                        .unwrap_or("Inaccessible")
                ),
            );
        }

        self.items.push(item);
        self.items.last()
    }

    fn finalize(
        mut self,
        category: Category,
        forced_quality: Option<ObservationQuality>,
    ) -> CategoryResult {
        self.items.sort_by(|left, right| {
            right
                .size
                .observed_bytes()
                .cmp(&left.size.observed_bytes())
                .then_with(|| left.name.cmp(&right.name))
        });

        let quality = forced_quality
            .unwrap_or_else(|| aggregate_quality(self.items.iter().map(|item| item.quality)));
        let skipped_entry_count = self.items.iter().map(|item| item.skipped_entry_count).sum();
        let incomplete_item_count = self
            .items
            .iter()
            .filter(|item| item.quality != ObservationQuality::Fresh)
            .count() as u64;

        CategoryResult {
            category,
            display_name: category.display_name().to_string(),
            items: self.items,
            total_bytes: self.total_bytes,
            cleanable_bytes: self.cleanable_bytes,
            safe_bytes: self.safe_bytes,
            rebuild_bytes: self.rebuild_bytes,
            manual_bytes: self.manual_bytes,
            quality,
            skipped_entry_count,
            incomplete_item_count,
            eligibility: self.eligibility,
            suppressed_duplicate_count: self.suppressed_duplicate_count,
            suppressed_duplicate_bytes: self.suppressed_duplicate_bytes,
            // Filled by `resolve_unit_overlaps`, which is the only place that
            // can see the whole result: a unit is overlapped by one in another
            // category as readily as by one beside it.
            suppressed_overlap_count: 0,
            suppressed_overlap_bytes: 0,
            ambiguous_overlap_count: 0,
            ambiguous_overlap_bytes: 0,
        }
    }
}

impl ScanEngine {
    /// Executes a full or filtered scan across all categories, emitting streaming events.
    ///
    /// The environment is threaded explicitly: signature paths, size
    /// exclusions, and external provider caches all resolve through it, so a
    /// scan cannot silently fall back to the running process's own profile.
    /// Lifecycle providers are threaded the same way: which providers exist is
    /// decided by the caller that owns them, not by the scan.
    /// The request's own facts are passed explicitly rather than bundled: each
    /// one is a decision the caller owns, and a struct would only move the same
    /// list one call site away.
    #[allow(clippy::too_many_arguments)]
    pub fn scan<F>(
        registry: &SignatureRegistry,
        lifecycle_providers: &LifecycleProviderRegistry,
        owner_providers: &crate::cleaner::OwnerProviderRegistry,
        categories_filter: Option<&[Category]>,
        excluded_signatures: &[String],
        intensive_cleanup: bool,
        environment: &PlatformEnvironment,
        cancellation: &dyn crate::models::CancellationProbe,
        on_event: F,
    ) -> ScanResult
    where
        F: FnMut(ScanEvent),
    {
        Self::scan_with_cache_providers(
            registry,
            lifecycle_providers,
            owner_providers,
            categories_filter,
            excluded_signatures,
            intensive_cleanup,
            environment,
            cancellation,
            &CacheProviderRegistry,
            on_event,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scan_with_cache_providers<F>(
        registry: &SignatureRegistry,
        lifecycle_providers: &LifecycleProviderRegistry,
        owner_providers: &crate::cleaner::OwnerProviderRegistry,
        categories_filter: Option<&[Category]>,
        excluded_signatures: &[String],
        intensive_cleanup: bool,
        environment: &PlatformEnvironment,
        cancellation: &dyn crate::models::CancellationProbe,
        cache_providers: &dyn CacheProviderScanner,
        mut on_event: F,
    ) -> ScanResult
    where
        F: FnMut(ScanEvent),
    {
        let scan_id = Uuid::new_v4().to_string();
        let started_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let started = std::time::Instant::now();
        // The loop and the walker's root reports write through one channel, so
        // the emitted stream keeps the order the scan produced it in.
        let events = ScanEvents {
            emit: RefCell::new(&mut on_event),
        };

        events.send(ScanEvent::Started {
            scan_id: scan_id.clone(),
        });

        let target_categories = categories_filter.unwrap_or(&[
            Category::Ai,
            Category::Developer,
            Category::Container,
            Category::System,
        ]);

        let mut category_results = Vec::new();
        let mut total_bytes = 0u64;
        let mut cleanable_bytes = 0u64;
        let mut safe_bytes = 0u64;
        let mut rebuild_bytes = 0u64;
        let mut manual_bytes = 0u64;
        let mut skipped_entry_count = 0u64;
        let mut incomplete_item_count = 0u64;
        let mut was_cancelled = false;
        // Traversals that stopped before covering every root they name. A
        // cancellation is the only reason one stops today, and the scan states
        // both facts: the typed gap says the scan was stopped, and this says
        // which coverage it therefore does not have.
        let mut scanned_incomplete_selectors = 0u64;
        let mut eligibility = EligibilitySummary::default();
        let mut suppressed_duplicate_count = 0u64;
        let mut suppressed_duplicate_bytes = 0u64;
        let mut gaps = Vec::new();
        let mut spans = Vec::new();
        let mut provider_incomplete_reasons = Vec::new();
        let mut provider_scan_incomplete = false;
        // One process-table pass for the whole scan: the scan asks which
        // application bundles are running, and every signature sees the same
        // answer.
        let process_started = Instant::now();
        let running_apps = crate::applications::RunningApplications::probe();
        spans.push(ScanSpan {
            source_id: "scan.process_snapshot".into(),
            duration_ms: process_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        });
        // Reuse the explicitly bounded shared scan pool (see
        // `execution_budget`); never expand pools per request.
        let pool_started = Instant::now();
        let directory_pool = shared_scan_pool();
        spans.push(ScanSpan {
            source_id: "scan.pool_startup".into(),
            duration_ms: pool_started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        });
        // The scan's own bounds and counters. The limits are stated once, and
        // the counters are what turn them into a measurement: the peak
        // outstanding directory-task count is checked against the bound by the
        // tests and reported to the interface.
        let limits = ScanLimits::default();
        let counters = TraversalCounters::default();
        // A root two rules name with the same measurement parameters is walked
        // once per scan (#493); the memo is dropped with the scan.
        let measurements = super::measurement_memo::MeasurementMemo::default();

        for &category in target_categories {
            if cancellation.is_cancelled() {
                was_cancelled = true;
                break;
            }

            events.send(ScanEvent::CategoryStarted { category });

            let mut accumulator = CategoryAccumulator::new();
            let mut category_incomplete = false;

            // 1. Scan filesystem signatures for this category
            // Discovery and eligibility are decided separately: the registry
            // returns what this scope looks at, and the gate below states what
            // the scope permits for each unit that is found.
            let signatures = registry.by_category_for_mode(category, intensive_cleanup);
            // Extra AI tool profiles: observation-only entries with unregistered
            // ids, snapshotted so this scan sees one consistent profile list.
            let profile_overlay = registry.profile_overlay();
            let profile_signatures = profile_overlay.iter().filter(|signature| {
                signature.category == category && signature.supports_current_platform()
            });
            for sig in signatures.into_iter().chain(profile_signatures) {
                if cancellation.is_cancelled() {
                    was_cancelled = true;
                    break;
                }
                if excluded_signatures.iter().any(|id| id == &sig.id) {
                    continue;
                }
                let gate = sig.eligibility_gate(intensive_cleanup);
                let context =
                    WalkContext::new(environment, cancellation, limits, &counters, &events)
                        .for_structured_state_policy(sig.structured_state_policy())
                        .with_measurement_memo(&measurements);
                let span_started = Instant::now();
                let mut scanned = DirectoryScanner::scan_signature_with_context(
                    sig,
                    directory_pool,
                    &context,
                    gate,
                    &running_apps,
                );
                let use_started = Instant::now();
                apply_signature_owner_state(&mut scanned.items, sig, &running_apps);
                spans.push(ScanSpan {
                    source_id: format!("{}.owner_state_projection", sig.id),
                    duration_ms: use_started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                });
                spans.push(ScanSpan {
                    source_id: sig.id.clone(),
                    duration_ms: span_started.elapsed().as_millis().min(u128::from(u64::MAX))
                        as u64,
                });
                spans.extend(scanned.spans);
                if scanned.selector_incomplete {
                    scanned_incomplete_selectors = scanned_incomplete_selectors.saturating_add(1);
                }
                for item in scanned.items {
                    if let Some(retained) = accumulator.push(item) {
                        events.send(ScanEvent::ItemFound {
                            item: retained.clone(),
                        });
                    }
                }
                if cancellation.is_cancelled() {
                    was_cancelled = true;
                    break;
                }
            }

            // 2. Tool-owned package caches apply the request contract before
            //    discovery and report command/measurement coverage explicitly.
            if !was_cancelled && category == Category::Developer {
                let span_started = Instant::now();
                let provider_scan = cache_providers.scan_items(
                    registry,
                    excluded_signatures,
                    environment,
                    cancellation,
                    limits,
                    &counters,
                    &events,
                );
                spans.push(ScanSpan {
                    source_id: "cache_providers".into(),
                    duration_ms: span_started.elapsed().as_millis().min(u128::from(u64::MAX))
                        as u64,
                });
                spans.extend(provider_scan.spans);
                let provider_cancelled = provider_scan.cancelled;
                for failure in provider_scan.failures {
                    category_incomplete = true;
                    provider_scan_incomplete = true;
                    add_scan_gap(&mut gaps, failure.kind, 1);
                    if !provider_incomplete_reasons.contains(&failure.reason) {
                        provider_incomplete_reasons.push(failure.reason);
                    }
                }
                for item in provider_scan.items {
                    if let Some(retained) = accumulator.push(item) {
                        events.send(ScanEvent::ItemFound {
                            item: retained.clone(),
                        });
                    }
                }
                if provider_cancelled || cancellation.is_cancelled() {
                    was_cancelled = true;
                }
            }

            // 3. Lifecycle providers own stores no path-shaped rule may touch.
            //    Their candidates are discovered here — in the category their
            //    catalog entry declares — and are never auto-selected.
            if !was_cancelled {
                let span_started = Instant::now();
                for item in lifecycle_providers.scan_items(
                    registry,
                    category,
                    intensive_cleanup,
                    excluded_signatures,
                    environment,
                ) {
                    if cancellation.is_cancelled() {
                        was_cancelled = true;
                        break;
                    }
                    if let Some(retained) = accumulator.push(item) {
                        events.send(ScanEvent::ItemFound {
                            item: retained.clone(),
                        });
                    }
                }
                spans.push(ScanSpan {
                    source_id: format!("lifecycle_providers.{category:?}").to_lowercase(),
                    duration_ms: span_started.elapsed().as_millis().min(u128::from(u64::MAX))
                        as u64,
                });
            }

            // 4. Owner-scoped providers enumerate the units of a store whose
            //    semantics only its owner knows. Their candidates are
            //    discovered here — in the category their catalog entry declares
            //    — and every one of them is offered for explicit selection.
            //    They read the scan's own process snapshot and open-file
            //    verdicts, run side by side on the shared pool, and name the
            //    store they are reading while they read it (#493).
            if !was_cancelled {
                for item in owner_providers.scan_items_in(
                    registry,
                    category,
                    intensive_cleanup,
                    excluded_signatures,
                    environment,
                    &crate::cleaner::OwnerScan::of_scan(
                        &running_apps,
                        cancellation,
                        directory_pool,
                    ),
                    &mut spans,
                    &events,
                ) {
                    if cancellation.is_cancelled() {
                        was_cancelled = true;
                        break;
                    }
                    if let Some(retained) = accumulator.push(item) {
                        events.send(ScanEvent::ItemFound {
                            item: retained.clone(),
                        });
                    }
                }
                was_cancelled |= cancellation.is_cancelled();
            }

            // 5. Typed container adapters can report cleanable or observation-only storage.
            if !was_cancelled && category == Category::Container {
                let adapter_items = DockerAdapter::scan_items(environment)
                    .into_iter()
                    .chain(OrbStackAdapter::scan_items(environment));
                for item in adapter_items {
                    if cancellation.is_cancelled() {
                        was_cancelled = true;
                        break;
                    }
                    if let Some(retained) = accumulator.push(item) {
                        events.send(ScanEvent::ItemFound {
                            item: retained.clone(),
                        });
                    }
                }
            }

            let category_result = accumulator.finalize(
                category,
                (was_cancelled || category_incomplete).then_some(ObservationQuality::Partial),
            );
            category_results.push(category_result);

            if was_cancelled {
                break;
            }
        }

        // Counting a location once is a decision about the whole result: a unit
        // inside a broader one is that unit's provenance, not a second total,
        // and the broader unit can be in another category. The categories are
        // restated before any number derived from them is reported.
        let overlap_started = Instant::now();
        super::coverage::route_to_owners(&mut category_results);
        let overlap = resolve_scan_overlaps(&mut category_results, registry);
        spans.push(ScanSpan {
            source_id: "scan.overlap_resolution".into(),
            duration_ms: overlap_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        });
        let physical_started = Instant::now();
        let physical = super::shared_storage::audit(&mut category_results, environment);
        spans.push(ScanSpan {
            source_id: "scan.physical_overlap_audit".into(),
            duration_ms: physical_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        });
        for category_result in &category_results {
            total_bytes += category_result.total_bytes;
            cleanable_bytes += category_result.cleanable_bytes;
            safe_bytes += category_result.safe_bytes;
            rebuild_bytes += category_result.rebuild_bytes;
            manual_bytes += category_result.manual_bytes;
            skipped_entry_count += category_result.skipped_entry_count;
            incomplete_item_count += category_result.incomplete_item_count;
            eligibility.merge(&category_result.eligibility);
            suppressed_duplicate_count += category_result.suppressed_duplicate_count;
            suppressed_duplicate_bytes += category_result.suppressed_duplicate_bytes;
        }
        let suppressed_overlap_count = overlap.suppressed_count;
        let suppressed_overlap_bytes = overlap.suppressed_bytes;
        let ambiguous_overlap_count = overlap.ambiguous_count + physical.count;
        let ambiguous_overlap_bytes = overlap.ambiguous_bytes.saturating_add(physical.bytes);

        // The per-category events carry the totals the resolution settled: a
        // category that folded an overlapping unit away reports what it now
        // accounts for, not what it held mid-scan.
        for (index, category_result) in category_results.iter().enumerate() {
            if was_cancelled && index + 1 == category_results.len() {
                break;
            }
            events.send(ScanEvent::CategoryFinished {
                category: category_result.category,
                bytes: category_result.total_bytes,
                item_count: category_result.items.len(),
            });
        }

        let finished_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut incomplete_reasons = provider_incomplete_reasons;
        if physical.incomplete {
            incomplete_reasons.push(
                "Physical storage overlap could not be fully inspected; observed bytes are a range"
                    .into(),
            );
        }
        for cat in &category_results {
            for item in &cat.items {
                if let Some(reason) = &item.incomplete_reason {
                    if !incomplete_reasons.contains(reason) {
                        incomplete_reasons.push(reason.clone());
                    }
                }
                if let Some(kind) = scan_gap_kind(item) {
                    add_scan_gap(&mut gaps, kind, 1);
                }
            }
        }
        if was_cancelled {
            incomplete_reasons.push("Scan was cancelled before completion".to_string());
            add_scan_gap(&mut gaps, ScanGapKind::Cancelled, 1);
        }
        let has_selector_truncation = scanned_incomplete_selectors > 0;
        if has_selector_truncation {
            incomplete_reasons.push(
                "A pattern's traversal stopped before it covered every root it names".to_string(),
            );
        }
        // Item-derived gaps already contribute their own observation quality,
        // including the all-unavailable case. Only scan-level incompleteness
        // that is not represented by a retained item must force Partial.
        let scan_quality = if was_cancelled
            || has_selector_truncation
            || provider_scan_incomplete
            || physical.incomplete
        {
            ObservationQuality::Partial
        } else {
            aggregate_quality(category_results.iter().map(|cat| cat.quality))
        };

        let result = ScanResult {
            scan_id,
            valid_for_seconds: ScanResult::VALID_FOR_SECONDS,
            started_at,
            finished_at,
            categories: category_results,
            total_bytes,
            cleanable_bytes,
            safe_bytes,
            rebuild_bytes,
            manual_bytes,
            quality: scan_quality,
            incomplete_reasons,
            gaps,
            spans,
            skipped_entry_count,
            incomplete_item_count,
            eligibility,
            suppressed_duplicate_count,
            suppressed_duplicate_bytes,
            suppressed_overlap_count,
            suppressed_overlap_bytes,
            ambiguous_overlap_count,
            ambiguous_overlap_bytes,
            cancelled: was_cancelled,
            metrics: ScanMetrics {
                duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                visited_entries: counters.visited_entries(),
                directories_read: counters.directories_read(),
                peak_outstanding_directory_tasks: counters.peak_outstanding_directory_tasks(),
            },
        };

        events.send(ScanEvent::Finished {
            result: result.clone(),
        });

        result
    }

    /// Rebuilds one authoritative snapshot from completed scan slices.
    ///
    /// Slices are never added as byte deltas. Their retained rows are combined
    /// first and the normal cross-category overlap and aggregate rules are run
    /// again, so a later category can still suppress or conflict with an
    /// earlier observation.
    pub fn merge_slices(
        registry: &SignatureRegistry,
        environment: &PlatformEnvironment,
        slices: &[ScanResult],
    ) -> Option<ScanResult> {
        let latest = slices.last()?.clone();
        let mut categories = slices
            .iter()
            .flat_map(|slice| slice.categories.clone())
            .collect::<Vec<_>>();
        let overlap_started = Instant::now();
        for category in &mut categories {
            category.ambiguous_overlap_count = 0;
            category.ambiguous_overlap_bytes = 0;
        }
        super::coverage::route_to_owners(&mut categories);
        let overlap = resolve_scan_overlaps(&mut categories, registry);
        let physical = super::shared_storage::audit(&mut categories, environment);
        let overlap_duration_ms = overlap_started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;

        let mut total_bytes = 0u64;
        let mut cleanable_bytes = 0u64;
        let mut safe_bytes = 0u64;
        let mut rebuild_bytes = 0u64;
        let mut manual_bytes = 0u64;
        let mut skipped_entry_count = 0u64;
        let mut incomplete_item_count = 0u64;
        let mut eligibility = EligibilitySummary::default();
        let mut suppressed_duplicate_count = 0u64;
        let mut suppressed_duplicate_bytes = 0u64;
        let mut incomplete_reasons = Vec::new();
        let mut gaps = Vec::new();
        for category in &categories {
            total_bytes = total_bytes.saturating_add(category.total_bytes);
            cleanable_bytes = cleanable_bytes.saturating_add(category.cleanable_bytes);
            safe_bytes = safe_bytes.saturating_add(category.safe_bytes);
            rebuild_bytes = rebuild_bytes.saturating_add(category.rebuild_bytes);
            manual_bytes = manual_bytes.saturating_add(category.manual_bytes);
            skipped_entry_count = skipped_entry_count.saturating_add(category.skipped_entry_count);
            incomplete_item_count =
                incomplete_item_count.saturating_add(category.incomplete_item_count);
            eligibility.merge(&category.eligibility);
            suppressed_duplicate_count =
                suppressed_duplicate_count.saturating_add(category.suppressed_duplicate_count);
            suppressed_duplicate_bytes =
                suppressed_duplicate_bytes.saturating_add(category.suppressed_duplicate_bytes);
            for item in &category.items {
                if let Some(reason) = &item.incomplete_reason {
                    if !incomplete_reasons.contains(reason) {
                        incomplete_reasons.push(reason.clone());
                    }
                }
                if let Some(kind) = scan_gap_kind(item) {
                    add_scan_gap(&mut gaps, kind, 1);
                }
            }
        }
        let cancelled = slices.iter().any(|slice| slice.cancelled);
        if cancelled {
            incomplete_reasons.push("Scan was cancelled before completion".to_string());
            add_scan_gap(&mut gaps, ScanGapKind::Cancelled, 1);
        }
        if physical.incomplete {
            incomplete_reasons.push(
                "Physical storage overlap could not be fully inspected; observed bytes are a range"
                    .into(),
            );
        }
        let quality = if cancelled || physical.incomplete {
            ObservationQuality::Partial
        } else {
            aggregate_quality(categories.iter().map(|category| category.quality))
        };
        let metrics = ScanMetrics {
            duration_ms: slices
                .iter()
                .map(|slice| slice.metrics.duration_ms)
                .fold(overlap_duration_ms, u64::saturating_add),
            visited_entries: slices
                .iter()
                .map(|slice| slice.metrics.visited_entries)
                .fold(0u64, u64::saturating_add),
            directories_read: slices
                .iter()
                .map(|slice| slice.metrics.directories_read)
                .fold(0u64, u64::saturating_add),
            peak_outstanding_directory_tasks: slices
                .iter()
                .map(|slice| slice.metrics.peak_outstanding_directory_tasks)
                .max()
                .unwrap_or_default(),
        };

        Some(ScanResult {
            scan_id: latest.scan_id,
            valid_for_seconds: latest.valid_for_seconds,
            started_at: slices
                .iter()
                .map(|slice| slice.started_at)
                .min()
                .unwrap_or(latest.started_at),
            finished_at: latest.finished_at,
            categories,
            total_bytes,
            cleanable_bytes,
            safe_bytes,
            rebuild_bytes,
            manual_bytes,
            quality,
            incomplete_reasons,
            gaps,
            spans: slices
                .iter()
                .flat_map(|slice| slice.spans.clone())
                .chain(std::iter::once(ScanSpan {
                    source_id: "scan.overlap_merge".into(),
                    duration_ms: overlap_duration_ms,
                }))
                .collect(),
            skipped_entry_count,
            incomplete_item_count,
            eligibility,
            suppressed_duplicate_count,
            suppressed_duplicate_bytes,
            suppressed_overlap_count: overlap.suppressed_count,
            suppressed_overlap_bytes: overlap.suppressed_bytes,
            ambiguous_overlap_count: overlap.ambiguous_count + physical.count,
            ambiguous_overlap_bytes: overlap.ambiguous_bytes.saturating_add(physical.bytes),
            cancelled,
            metrics,
        })
    }
}

/// The scan's single event channel.
///
/// The loop and the walker's root reports write through one value, so the
/// emitted stream keeps the order the scan produced it in; the interior
/// mutability is what lets the walker hold the channel while the loop still
/// emits items.
struct ScanEvents<'a, F: FnMut(ScanEvent)> {
    emit: std::cell::RefCell<&'a mut F>,
}

impl<F: FnMut(ScanEvent)> ScanEvents<'_, F> {
    fn send(&self, event: ScanEvent) {
        (self.emit.borrow_mut())(event);
    }
}

impl<F: FnMut(ScanEvent)> RootProgressSink for ScanEvents<'_, F> {
    fn root_started(&self, signature: &crate::models::Signature, root: &std::path::Path) {
        self.send(ScanEvent::RootStarted {
            category: signature.category,
            signature_id: signature.id.clone(),
            name: signature.name.clone(),
            root: root.to_string_lossy().into_owned(),
        });
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod overlap_tests;
