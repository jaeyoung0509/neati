//! Running a scan's owner providers side by side (#493).
//!
//! Each provider inventories its own store, so one provider waiting on its
//! owner's command (`brew cleanup --dry-run` takes seconds) no longer holds
//! every other provider behind it. At most [`OWNER_PROVIDER_LANES`] providers
//! run at once, on the shared scan pool, never on threads of their own. The
//! results are taken in catalog order, with the serial pass's cancellation
//! rule, so the items a scan reports do not depend on which provider finished
//! first.

use super::scan_context::{OwnerScanContext, ProviderProgress};
use crate::applications::RunningApplications;
use crate::models::CancellationProbe;
use rayon::ThreadPool;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// The most owner providers one scan runs at once.
///
/// The shared scan pool has at most four workers, and the lanes never exceed
/// the pool, so this never adds a thread. Four lets the one slow owner command
/// overlap with the stores that are only read, without letting providers
/// crowd out the pool they share.
pub const OWNER_PROVIDER_LANES: usize = 4;

/// What a scan lends its owner-provider pass.
pub struct OwnerScan<'a> {
    /// The scan's process snapshot and batched open-file verdicts. `None`
    /// leaves every provider on its own live port.
    pub snapshot: Option<&'a RunningApplications>,
    pub cancellation: &'a dyn CancellationProbe,
    /// Where providers run side by side. `None` runs them one after another
    /// on the calling thread.
    pub pool: Option<&'a ThreadPool>,
    /// How many providers may run at once, never more than the pool has
    /// workers.
    pub lanes: usize,
}

impl<'a> OwnerScan<'a> {
    /// An inventory outside a scan: providers run one at a time on the
    /// calling thread and read the live process table themselves.
    pub fn unscanned(cancellation: &'a dyn CancellationProbe) -> Self {
        Self {
            snapshot: None,
            cancellation,
            pool: None,
            lanes: 1,
        }
    }

    /// The pass a scan runs: its own snapshot, its shared pool, bounded lanes.
    pub fn of_scan(
        snapshot: &'a RunningApplications,
        cancellation: &'a dyn CancellationProbe,
        pool: Option<&'a ThreadPool>,
    ) -> Self {
        Self {
            snapshot: Some(snapshot),
            cancellation,
            pool,
            lanes: OWNER_PROVIDER_LANES,
        }
    }

    /// The context one provider's inventory receives.
    pub(super) fn context<'c>(
        &'c self,
        source_id: &'c str,
        progress: &'c dyn ProviderProgress,
    ) -> OwnerScanContext<'c> {
        OwnerScanContext::lent(source_id, self.cancellation, self.snapshot, progress)
    }
}

/// What the lanes tell the thread that runs the scan.
pub(super) enum Report<R> {
    /// Job `index` started.
    Started(usize),
    /// Job `index` is reading a location.
    Location(usize, PathBuf),
    /// Job `index` ended with its result, or was never started because the
    /// scan had been stopped.
    Finished(usize, Option<R>),
}

/// Progress from a lane, carried to the scan's thread.
struct LaneProgress<'a, R> {
    index: usize,
    reports: &'a mpsc::Sender<Report<R>>,
}

impl<R> ProviderProgress for LaneProgress<'_, R> {
    fn location_started(&self, location: &Path) {
        // The scan's thread outlives every lane; a closed channel only means
        // nobody is listening any more.
        let _ = self
            .reports
            .send(Report::Location(self.index, location.to_path_buf()));
    }
}

/// Progress from a job that runs on the scan's own thread.
struct InlineProgress<'a, 'f, R> {
    index: usize,
    observe: &'a RefCell<&'f mut dyn FnMut(Report<R>)>,
}

impl<R> ProviderProgress for InlineProgress<'_, '_, R> {
    fn location_started(&self, location: &Path) {
        (self.observe.borrow_mut())(Report::Location(self.index, location.to_path_buf()));
    }
}

/// Runs `jobs`, at most `lanes` at a time on `pool`, and hands every report to
/// `observe` on the calling thread, as it happens.
///
/// Jobs start in index order. Once `cancellation` is set no further job
/// starts; jobs already running finish under the same cancellation. Without a
/// pool, or with one lane, the jobs run one after another on this thread.
pub(super) fn run_in_lanes<J, R>(
    jobs: &[J],
    pool: Option<&ThreadPool>,
    lanes: usize,
    cancellation: &dyn CancellationProbe,
    run: &(dyn Fn(&J, &dyn ProviderProgress) -> R + Sync),
    observe: &mut dyn FnMut(Report<R>),
) where
    J: Sync,
    R: Send,
{
    let lanes = pool.map_or(1, |pool| {
        lanes.min(pool.current_num_threads()).min(jobs.len()).max(1)
    });
    let Some(pool) = pool.filter(|_| lanes > 1) else {
        let observe = RefCell::new(observe);
        for (index, job) in jobs.iter().enumerate() {
            if cancellation.is_cancelled() {
                (observe.borrow_mut())(Report::Finished(index, None));
                continue;
            }
            (observe.borrow_mut())(Report::Started(index));
            let progress = InlineProgress {
                index,
                observe: &observe,
            };
            let result = run(job, &progress);
            (observe.borrow_mut())(Report::Finished(index, Some(result)));
        }
        return;
    };

    let next = AtomicUsize::new(0);
    let (reports, received) = mpsc::channel::<Report<R>>();
    pool.in_place_scope(|scope| {
        for _ in 0..lanes {
            let reports = reports.clone();
            let next = &next;
            scope.spawn(move |_| loop {
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(index) else {
                    break;
                };
                if cancellation.is_cancelled() {
                    let _ = reports.send(Report::Finished(index, None));
                    continue;
                }
                let _ = reports.send(Report::Started(index));
                let progress = LaneProgress {
                    index,
                    reports: &reports,
                };
                let result = run(job, &progress);
                let _ = reports.send(Report::Finished(index, Some(result)));
            });
        }
        // Every lane holds its own sender; the stream ends when the last lane
        // does.
        drop(reports);
        for report in received {
            observe(report);
        }
    });
}

/// Which running job the interface should be told about.
///
/// Lanes report starts and locations as they happen. When the job the
/// interface was last told about ends while others still run, the one that
/// has been running longest is named again, so the scan never sits on a
/// location nothing is reading any more.
#[derive(Default)]
pub(super) struct ProgressFocus {
    /// Running jobs in the order they started, with their latest location.
    running: Vec<(usize, PathBuf)>,
    announced: Option<usize>,
}

impl ProgressFocus {
    /// Returns the job and location to announce, if any.
    pub(super) fn started(&mut self, index: usize, location: PathBuf) -> (usize, PathBuf) {
        self.running.push((index, location.clone()));
        self.announced = Some(index);
        (index, location)
    }

    pub(super) fn moved(&mut self, index: usize, location: PathBuf) -> (usize, PathBuf) {
        if let Some(entry) = self.running.iter_mut().find(|(job, _)| *job == index) {
            entry.1 = location.clone();
        }
        self.announced = Some(index);
        (index, location)
    }

    pub(super) fn finished(&mut self, index: usize) -> Option<(usize, PathBuf)> {
        self.running.retain(|(job, _)| *job != index);
        if self.announced != Some(index) {
            return None;
        }
        let (job, location) = self.running.first()?.clone();
        self.announced = Some(job);
        Some((job, location))
    }
}

#[cfg(test)]
#[path = "scan_run_tests.rs"]
mod tests;
