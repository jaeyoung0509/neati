//! What one scan lends the owner providers it runs (#493).
//!
//! A provider's inventory asks the same questions the scan already answered:
//! which executables are running, and whether a unit's files are open. The
//! scan takes one process snapshot and observes open files in batches; an
//! inventory run by the scan reads those instead of reading the process table
//! again for every store and every unit.
//!
//! Only the scan lends them. `prepare` and `execute` never receive this
//! context: they read the live process table and observe each unit's open
//! files themselves, immediately before they authorize or remove anything.

use crate::applications::RunningApplications;
use crate::models::CancellationProbe;
use neati_core::domain::cleanup::{RunningProcessPolicy, RunningProcessProbe};
use std::path::{Path, PathBuf};

/// The process and open-file evidence one inventory reads.
#[derive(Clone, Copy)]
pub enum ProcessView<'a> {
    /// Every question reads the live process table and open files: what
    /// `prepare`, `execute` and an inventory outside a scan see.
    Live(&'a dyn RunningProcessProbe),
    /// One scan's process snapshot and the open-file verdicts it observed in
    /// batches. A question the scan's cancellation interrupts is unknown.
    Scan {
        snapshot: &'a RunningApplications,
        cancellation: &'a dyn CancellationProbe,
    },
}

impl RunningProcessProbe for ProcessView<'_> {
    fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
        match self {
            Self::Live(probe) => probe.running(guard),
            Self::Scan {
                snapshot,
                cancellation,
            } => snapshot
                .running_executables_with_cancellation(guard, &|| cancellation.is_cancelled()),
        }
    }
}

impl ProcessView<'_> {
    /// Observes the open files of many units at once, so each unit's later
    /// question reads its own verdict instead of starting another `lsof`.
    /// A live view observes each unit when it is asked, as it always has.
    pub fn prefetch_open_files(&self, paths: &[PathBuf]) {
        if let Self::Scan {
            snapshot,
            cancellation,
        } = self
        {
            if paths.len() > 1 {
                snapshot.prefetch_open_files(paths, &|| cancellation.is_cancelled());
            }
        }
    }
}

/// Where a provider says which location a long step is reading, so the
/// interface names it instead of sitting on the last root it heard about.
/// Progress is a side channel: nothing a provider reports depends on it.
pub trait ProviderProgress {
    fn location_started(&self, location: &Path);
}

/// Progress nobody listens to.
pub struct NoProviderProgress;

impl ProviderProgress for NoProviderProgress {
    fn location_started(&self, _location: &Path) {}
}

/// One provider's inventory inside one scan.
pub struct OwnerScanContext<'a> {
    /// The catalog id the provider's spans are keyed by.
    pub source_id: &'a str,
    pub cancellation: &'a dyn CancellationProbe,
    snapshot: Option<&'a RunningApplications>,
    progress: &'a dyn ProviderProgress,
}

impl<'a> OwnerScanContext<'a> {
    /// An inventory outside a scan: no snapshot is lent, so the provider reads
    /// the live process table through its own port, and nobody hears progress.
    pub fn unscanned(source_id: &'a str, cancellation: &'a dyn CancellationProbe) -> Self {
        Self {
            source_id,
            cancellation,
            snapshot: None,
            progress: &NoProviderProgress,
        }
    }

    pub(super) fn lent(
        source_id: &'a str,
        cancellation: &'a dyn CancellationProbe,
        snapshot: Option<&'a RunningApplications>,
        progress: &'a dyn ProviderProgress,
    ) -> Self {
        Self {
            source_id,
            cancellation,
            snapshot,
            progress,
        }
    }

    /// The evidence the inventory reads: the scan's snapshot when the scan
    /// lent one, otherwise the provider's own live port.
    pub fn processes<'b>(&'b self, own: &'b dyn RunningProcessProbe) -> ProcessView<'b> {
        match self.snapshot {
            Some(snapshot) => ProcessView::Scan {
                snapshot,
                cancellation: self.cancellation,
            },
            None => ProcessView::Live(own),
        }
    }

    /// The provider is about to spend time reading `location`.
    pub fn location_started(&self, location: &Path) {
        self.progress.location_started(location);
    }
}

impl ProviderProgress for OwnerScanContext<'_> {
    fn location_started(&self, location: &Path) {
        OwnerScanContext::location_started(self, location);
    }
}
