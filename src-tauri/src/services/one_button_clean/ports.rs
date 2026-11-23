//! The one-button Clean use case's ports.
//!
//! [`CleanUseCase`] is the driving port an interface adapter calls.
//! [`CacheCleanup`] and [`ProjectCleanup`] are the driven ports the use case
//! needs; `crate::composition` binds them to the in-process services, and
//! tests bind them to fakes.

use std::sync::Arc;

use crate::models::{
    DeveloperArtifactScanResult, DeveloperWorkspaceRefusal, ScanResult, WorktreeInventory,
};
use crate::services::ports::{ActivityPort, PortFuture};

use super::selection::Candidate;

/// Which parts of the one-button set a run covers, and how it is narrowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanRequest {
    pub caches: bool,
    pub projects: bool,
    /// Finished worktrees the person's rule includes (#505).
    pub worktrees: bool,
    /// Use an inventory the process already holds while it is still valid for
    /// cleanup, instead of scanning again. The window sets this, because its
    /// own scan just ran; a terminal run starts with nothing to reuse.
    pub reuse_current: bool,
    /// Keep only these references. Empty keeps the whole set.
    pub only: Vec<String>,
    /// Leave these references out.
    pub skip: Vec<String>,
}

impl Default for CleanRequest {
    fn default() -> Self {
        Self {
            caches: true,
            projects: true,
            worktrees: true,
            reuse_current: false,
            only: Vec::new(),
            skip: Vec::new(),
        }
    }
}

/// The saved project folders and what scanning them found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectInventory {
    pub folders: Vec<String>,
    pub refused: Vec<DeveloperWorkspaceRefusal>,
    /// `None` when no folder could be scanned.
    pub result: Option<DeveloperArtifactScanResult>,
}

/// Units that were found but are left for review in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReviewSummary {
    pub items: u64,
    pub bytes: u64,
}

/// What a run would remove, from fresh scans. Executing it is the approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanPreview {
    pub chosen: Vec<Candidate>,
    pub total_bytes: u64,
    pub review: ReviewSummary,
    /// The whole cache inventory the set was chosen from, when scanned.
    pub caches: Option<ScanResult>,
    /// The whole project inventory the set was chosen from, when scanned.
    pub projects: Option<ProjectInventory>,
    pub worktrees: Option<WorktreeInventory>,
}

/// One unit the run did not remove, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFailure {
    pub reference: String,
    pub name: String,
    pub message: String,
}

/// What a run removed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CleanOutcome {
    pub reclaimed_bytes: u64,
    pub removed: u64,
    /// Already gone, or no longer the object that was approved.
    pub skipped: u64,
    pub failed: Vec<UnitFailure>,
}

/// Why a run did not get as far as removing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneButtonError {
    /// `--only`/`--skip` named something outside this run's set.
    UnknownReferences(Vec<String>),
    /// The preview being executed is unknown, already used, or too old.
    PreviewExpired,
    /// A scan, plan or removal could not run.
    Unavailable(String),
}

impl std::fmt::Display for OneButtonError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownReferences(references) => write!(
                formatter,
                "Not in this run's Clean set: {}. Nothing was removed. Preview again to see the current references.",
                references.join(", ")
            ),
            Self::PreviewExpired => formatter.write_str(
                "This preview is no longer current. Check again before cleaning.",
            ),
            Self::Unavailable(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for OneButtonError {
    fn from(message: String) -> Self {
        Self::Unavailable(message)
    }
}

/// Driving port: preview the one-button set, then execute that preview.
pub trait CleanUseCase: Send + Sync {
    fn preview(
        &self,
        request: CleanRequest,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<CleanPreview, OneButtonError>>;

    fn execute<'a>(
        &'a self,
        preview: &'a CleanPreview,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'a, Result<CleanOutcome, OneButtonError>>;
}

/// What one removal call through a driven port did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemovalReport {
    pub reclaimed_bytes: u64,
    /// Backend ids of the units removed (fully or partly).
    pub removed: Vec<String>,
    pub skipped: u64,
    /// Backend ids that were not removed, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Driven port: the cache inventory and its reviewed quick-clean removal.
pub trait CacheCleanup: Send + Sync {
    /// The scan this process already published, while it is still valid for
    /// cleanup. `None` means scan.
    fn current(&self) -> Option<ScanResult>;

    /// A complete scan with the person's exclusions.
    fn scan(&self, activity: Arc<dyn ActivityPort>) -> PortFuture<'_, Result<ScanResult, String>>;

    /// The backend-owned one-button membership of `scan`.
    fn one_button_ids(&self, scan: &ScanResult) -> Result<Vec<String>, String>;

    /// Removes exactly `ids` from the scan named `scan_id`; the backend
    /// re-derives every unit before it acts.
    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>>;
}

/// Driven port: build output in the saved project folders.
pub trait ProjectCleanup: Send + Sync {
    /// The project inventory this process already holds, while it is fresh.
    fn current(&self) -> Option<ProjectInventory>;

    fn scan(
        &self,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<ProjectInventory, String>>;

    /// Removes exactly `ids` through a one-shot plan; the executor repeats the
    /// ownership and open-file checks before each removal.
    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>>;
}

/// Driven port: linked Git worktrees of the saved project folders (#505).
pub trait WorktreeCleanup: Send + Sync {
    /// The published inventory, while it is recent and decided with the rules
    /// in force; `None` asks the caller to scan.
    fn current(&self) -> Option<WorktreeInventory>;

    fn scan(
        &self,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<WorktreeInventory, String>>;

    /// Removes the finished worktrees `ids` of scan `scan_id` through Git.
    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>>;
}
