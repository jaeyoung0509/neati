//! Measuring the enumerated children of an aged root on the shared scan pool
//! (#493).
//!
//! Each child is still measured by the one single-pass tree walk, with its own
//! fresh metadata, cancellation and policy checks at every entry; only the
//! children are measured side by side instead of one after another. The
//! results come back in enumeration order, so the items built from them are
//! the items a serial walk builds.

use super::{DirectoryScanner, TreeObservationPolicy, TreePhaseTimings, TreeStats};
use crate::scanner::observation::{NoRootProgress, WalkContext};
use rayon::prelude::*;
use rayon::ThreadPool;
use std::path::Path;
use std::time::{Duration, Instant};

/// What measuring a set of children cost, stated so the phase spans keep their
/// meaning: the sub-phases never add up to more than the measurement, and the
/// measurement never exceeds the wall time it took.
#[derive(Default)]
pub(super) struct ChildMeasurementTimings {
    pub(super) tree_measurement: Duration,
    pub(super) metadata_and_links: Duration,
    pub(super) age_evaluation: Duration,
    pub(super) size_accounting: Duration,
    pub(super) traversal: Duration,
}

/// One child's measurement, and whether the scan had been cancelled by the
/// time it ended. A serial walk checks for cancellation between children, so
/// a stopped child is the last one it reports.
pub(super) struct MeasuredChild {
    pub(super) stats: TreeStats,
    pub(super) stopped: bool,
}

/// Measures every child, in parallel on `pool` when there is one.
///
/// A child the walk reached only after the scan was cancelled is `None`: it
/// was never measured, exactly as a serial walk stops before it.
pub(super) fn measure_children(
    context: &WalkContext<'_>,
    pool: Option<&ThreadPool>,
    policy: &TreeObservationPolicy,
    stale_policy: Option<crate::safety::StaleEntryPolicy>,
    children: &[&Path],
) -> (Vec<Option<MeasuredChild>>, ChildMeasurementTimings) {
    // The walk context is rebuilt on each worker: the progress sink is the
    // scan's single ordered writer and never leaves the scan thread, and a
    // tree measurement reports no progress.
    let environment = context.environment;
    let cancellation = context.cancellation;
    let limits = context.limits;
    let counters = context.counters;
    let permit_application_bundles = context.permit_application_bundles;
    let measure = |path: &&Path| -> Option<(MeasuredChild, TreePhaseTimings, Duration)> {
        if cancellation.is_cancelled() {
            return None;
        }
        let mut worker =
            WalkContext::new(environment, cancellation, limits, counters, &NoRootProgress);
        worker.permit_application_bundles = permit_application_bundles;
        let started = Instant::now();
        let mut phases = TreePhaseTimings::default();
        let stats = DirectoryScanner::measure_tree_stats_profiled(
            &worker,
            path,
            policy,
            0,
            stale_policy,
            &mut phases,
        );
        let elapsed = started.elapsed();
        let stopped = cancellation.is_cancelled();
        Some((MeasuredChild { stats, stopped }, phases, elapsed))
    };
    let started = Instant::now();
    let measured: Vec<_> = match pool {
        // One child is not worth a pool round trip.
        Some(pool) if children.len() > 1 => {
            pool.install(|| children.par_iter().map(measure).collect())
        }
        _ => children.iter().map(measure).collect(),
    };
    let wall = started.elapsed();

    // Worker time adds up across workers; the spans describe wall time, so a
    // parallel measurement states each phase as its share of the wall time.
    let worker_time: Duration = measured
        .iter()
        .flatten()
        .map(|(_, _, elapsed)| *elapsed)
        .sum();
    let share = |duration: Duration| {
        if worker_time > wall && !worker_time.is_zero() {
            duration.mul_f64(wall.as_secs_f64() / worker_time.as_secs_f64())
        } else {
            duration
        }
    };
    let mut timings = ChildMeasurementTimings::default();
    let mut children = Vec::with_capacity(measured.len());
    for result in measured {
        let Some((child, phases, elapsed)) = result else {
            children.push(None);
            continue;
        };
        let elapsed = share(elapsed);
        let metadata_and_links = share(phases.metadata_and_links);
        let age_evaluation = share(phases.age_evaluation);
        let size_accounting = share(phases.size_accounting);
        timings.tree_measurement += elapsed;
        timings.metadata_and_links += metadata_and_links;
        timings.age_evaluation += age_evaluation;
        timings.size_accounting += size_accounting;
        timings.traversal += elapsed
            .saturating_sub(metadata_and_links)
            .saturating_sub(age_evaluation)
            .saturating_sub(size_accounting);
        children.push(Some(child));
    }
    (children, timings)
}
