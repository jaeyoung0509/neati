mod activity;
#[cfg(test)]
mod framework_tests;
mod measurement;
pub mod preselection;
mod project_caches;
#[cfg(test)]
mod project_caches_tests;
mod recognition;
mod rules;
mod workspaces;
pub(crate) use activity::{
    has_working_process, project_last_activity, running_working_directories,
};
pub(crate) use project_caches::is_inner_cache_tag;
pub(crate) use rules::artifact_is_framework_parent;
pub(crate) use rules::artifact_relative_is_allowed;
pub(crate) use rules::recheck_framework_metadata;
pub(crate) use rules::recheck_staged_framework_contract;
pub(crate) use rules::{framework_generated_kind, verify_framework_generated_contract};

use crate::large_files::identity_from_path;
use crate::models::{
    ArtifactOwnershipEvidence, DeveloperArtifact, DeveloperArtifactKind,
    DeveloperArtifactScanEvent, DeveloperArtifactScanResult, DeveloperArtifactStatus,
    DeveloperArtifactUninspected, DeveloperArtifactUninspectedReason, DeveloperEcosystem,
    DeveloperWorkspace, DeveloperWorkspaceRefusal, DeveloperWorkspaceRestore,
    DeveloperWorkspaceSuggestion, ReviewedFileIdentity,
};
use crate::safety::{Blacklist, SymlinkGuard};
use neati_platform::description::PlatformEnvironment;
use neati_platform::path_algebra;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use measurement::*;
use recognition::*;
pub use workspaces::*;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const MAX_WORKSPACES: usize = 16;
const MAX_CANDIDATES: usize = 512;
const MAX_DISCOVERY_DEPTH: usize = 64;
const MAX_MEASUREMENT_DEPTH: usize = 64;
const MAX_DISCOVERY_ENTRIES: u64 = 250_000;
const INVENTORY_TTL_SECS: u64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct DeveloperWorkspaceRecord {
    pub workspace: DeveloperWorkspace,
    pub path: PathBuf,
    pub identity: ReviewedFileIdentity,
    pub created_at: u64,
    pub whole_home: bool,
}

#[derive(Debug, Clone)]
pub struct DeveloperArtifactRecord {
    pub artifact: DeveloperArtifact,
    pub path: PathBuf,
    pub identity: ReviewedFileIdentity,
    pub workspace_path: PathBuf,
    pub workspace_identity: ReviewedFileIdentity,
    pub project_root: PathBuf,
    pub project_identity: ReviewedFileIdentity,
    pub artifact_relative: PathBuf,
    pub marker_identities: Vec<(PathBuf, ReviewedFileIdentity)>,
}

#[derive(Debug, Clone)]
pub struct DeveloperArtifactInventory {
    pub scan_id: String,
    pub records: HashMap<String, DeveloperArtifactRecord>,
    pub workspace_ids: Vec<String>,
    pub created_at: u64,
    pub discovered_count: u64,
    pub measured_count: u64,
    pub skipped_entries: u64,
    pub cancelled: bool,
    pub truncated: bool,
    pub uninspected: Vec<DeveloperArtifactUninspected>,
    /// The cleanup rules one-button inclusion was decided with (#504). An
    /// inventory decided with other rules is never reused for Clean.
    pub rules: crate::models::CleanupRules,
}

impl DeveloperArtifactInventory {
    /// Whether one-button Clean may act on this inventory's inclusion: it is
    /// fresh and was decided with the rules in force now (#504).
    pub fn is_current_for(&self, rules: &crate::models::CleanupRules) -> bool {
        self.is_fresh() && self.rules == *rules
    }

    pub fn is_fresh(&self) -> bool {
        self.is_fresh_at(unix_timestamp())
    }

    pub fn is_fresh_at(&self, now: u64) -> bool {
        neati_core::domain::is_within_window(self.created_at, now, INVENTORY_TTL_SECS)
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    workspace: DeveloperWorkspaceRecord,
    project_name: String,
    ecosystem: DeveloperEcosystem,
    kind: DeveloperArtifactKind,
    path: PathBuf,
    project_root: PathBuf,
    artifact_relative: PathBuf,
    marker_paths: Vec<PathBuf>,
    evidence: Vec<String>,
    rebuild_hint: Option<String>,
}

#[derive(Debug, Clone)]
struct ArtifactMatch {
    ecosystem: DeveloperEcosystem,
    kind: DeveloperArtifactKind,
    project_root: PathBuf,
    artifact_relative: PathBuf,
    marker_paths: Vec<PathBuf>,
    evidence: Vec<String>,
    rebuild_hint: Option<String>,
}

/// Reuse the central generated-artifact catalog for the temporary workflow.
/// Whole framework outputs and unknown wrappers grant no authority here.
pub(crate) fn temporary_generated_evidence(
    project: &Path,
    path: &Path,
) -> Option<(DeveloperArtifactKind, Vec<PathBuf>)> {
    let name = path.file_name()?.to_str()?;
    let found = rules::recognize(project, name)?;
    // Framework whole-output authority belongs to its dedicated project review;
    // never extend the temporary-folder consent workflow through a kind change.
    if artifact_is_framework_parent(found.kind) {
        return None;
    }
    (path == project.join(&found.artifact_relative)
        && artifact_relative_is_allowed(&found.artifact_relative, found.kind))
    .then_some((found.kind, found.marker_paths))
}

#[derive(Debug, Clone, Default)]
struct TreeStats {
    logical_bytes: u64,
    allocated_bytes: u64,
    file_count: u64,
    newest_mtime: Option<SystemTime>,
    complete: bool,
    safety_blocked: bool,
    cancelled: bool,
    framework_verified: bool,
    framework_advisory: Option<String>,
}

#[derive(Debug, Clone)]
struct ScanProgress {
    discovered_count: u64,
    measured_count: u64,
    skipped_entries: u64,
    cancelled: bool,
    truncated: bool,
    uninspected: Vec<DeveloperArtifactUninspected>,
}

impl TreeStats {
    fn new() -> Self {
        Self {
            complete: true,
            ..Self::default()
        }
    }
}

enum MeasurementMessage {
    Started {
        artifact_id: String,
        project_name: String,
        kind: DeveloperArtifactKind,
    },
    Finished {
        candidate: Box<Candidate>,
        stats: TreeStats,
        ownership: ArtifactOwnershipEvidence,
    },
}

/// The outcome of probing a folder the operating system gates behind a user
/// consent prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderAccess {
    /// The platform does not gate this folder, or it is not present.
    NotGated,
    /// The folder was read; the walk may enter it without a new prompt.
    Granted,
    /// Access was refused, including a denied or cancelled folder-access
    /// prompt.
    Denied,
    /// The folder exists but could not be opened for a non-permission I/O
    /// reason. Retrying permission cannot repair this state.
    Unreadable,
}

/// Probes the user's Downloads folder before the scan takes the storage gate.
///
/// macOS resolves `~/Downloads` consent on the first read and parks the calling
/// thread until the user answers. Probing on a thread that holds no gate keeps
/// that wait off the lock every other storage operation queues behind, and the
/// answer is passed into the walk so it never blocks on the same prompt twice.
///
/// The probe cannot distinguish "Don't Allow" from a cancelled prompt: both
/// arrive as the same access refusal, so both report [`FolderAccess::Denied`].
/// Returns [`FolderAccess::NotGated`] on every non-macOS platform.
pub fn probe_downloads_access(environment: &PlatformEnvironment) -> FolderAccess {
    #[cfg(target_os = "macos")]
    {
        let Some(downloads) = environment.content_dir("downloads") else {
            return FolderAccess::NotGated;
        };
        match fs::read_dir(&downloads) {
            Ok(_) => FolderAccess::Granted,
            // A missing folder raises no prompt, so it is not a refusal.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => FolderAccess::NotGated,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                FolderAccess::Denied
            }
            Err(_) => FolderAccess::Unreadable,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = environment;
        FolderAccess::NotGated
    }
}

pub struct DeveloperArtifactScanner;

impl DeveloperArtifactScanner {
    pub fn scan<F>(
        environment: &PlatformEnvironment,
        workspace_ids: &[String],
        workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
        downloads_access: FolderAccess,
        cancel: Arc<AtomicBool>,
        on_event: F,
    ) -> Result<DeveloperArtifactInventory, String>
    where
        F: FnMut(DeveloperArtifactScanEvent),
    {
        let workspaces = workspace_snapshot(workspace_ids, workspaces_store)?;
        Self::scan_workspaces(environment, &workspaces, downloads_access, cancel, on_event)
    }

    pub fn scan_workspaces<F>(
        environment: &PlatformEnvironment,
        workspaces: &[DeveloperWorkspaceRecord],
        downloads_access: FolderAccess,
        cancel: Arc<AtomicBool>,
        on_event: F,
    ) -> Result<DeveloperArtifactInventory, String>
    where
        F: FnMut(DeveloperArtifactScanEvent),
    {
        Self::scan_workspaces_with_rules(
            environment,
            workspaces,
            downloads_access,
            cancel,
            on_event,
            crate::models::CleanupRules::default(),
        )
    }

    /// As [`Self::scan_workspaces`], deciding one-button inclusion with the
    /// person's cleanup rules and the native activity and use observations.
    pub fn scan_workspaces_with_rules<F>(
        environment: &PlatformEnvironment,
        workspaces: &[DeveloperWorkspaceRecord],
        downloads_access: FolderAccess,
        cancel: Arc<AtomicBool>,
        on_event: F,
        rules: crate::models::CleanupRules,
    ) -> Result<DeveloperArtifactInventory, String>
    where
        F: FnMut(DeveloperArtifactScanEvent),
    {
        let activity = preselection::native_activity(environment);
        let selection = preselection::Selection::native(rules, &activity);
        Self::scan_workspaces_with_selection(
            environment,
            workspaces,
            downloads_access,
            cancel,
            on_event,
            &selection,
        )
    }

    /// As [`Self::scan_workspaces`], with the open-file check that decides
    /// which build outputs one-button Clean preselects supplied by the caller.
    pub fn scan_workspaces_with_use_probe<F>(
        environment: &PlatformEnvironment,
        workspaces: &[DeveloperWorkspaceRecord],
        downloads_access: FolderAccess,
        cancel: Arc<AtomicBool>,
        on_event: F,
        use_probe: &preselection::UseProbe<'_>,
    ) -> Result<DeveloperArtifactInventory, String>
    where
        F: FnMut(DeveloperArtifactScanEvent),
    {
        let activity = preselection::native_activity(environment);
        let selection = preselection::Selection {
            rules: crate::models::CleanupRules::default(),
            now: unix_timestamp(),
            use_probe,
            activity: &activity,
            working_directories: &Vec::new,
        };
        Self::scan_workspaces_with_selection(
            environment,
            workspaces,
            downloads_access,
            cancel,
            on_event,
            &selection,
        )
    }

    /// The scan itself, with every observation the selection needs supplied.
    pub fn scan_workspaces_with_selection<F>(
        environment: &PlatformEnvironment,
        workspaces: &[DeveloperWorkspaceRecord],
        downloads_access: FolderAccess,
        cancel: Arc<AtomicBool>,
        mut on_event: F,
        selection: &preselection::Selection<'_>,
    ) -> Result<DeveloperArtifactInventory, String>
    where
        F: FnMut(DeveloperArtifactScanEvent),
    {
        let workspace_ids = workspaces
            .iter()
            .map(|w| w.workspace.id.clone())
            .collect::<Vec<_>>();
        let scan_id = Uuid::new_v4().to_string();
        on_event(DeveloperArtifactScanEvent::Started {
            scan_id: scan_id.clone(),
            workspace_count: workspaces.len() as u64,
        });

        let mut candidates = Vec::new();
        let mut discovered_count = 0u64;
        let mut skipped_entries = 0u64;
        let mut truncated = false;
        let mut uninspected: Vec<DeveloperArtifactUninspected> = Vec::new();
        let mut uninspected_seen: HashSet<PathBuf> = HashSet::new();
        let mut seen_paths = HashSet::new();

        for workspace in workspaces {
            if cancel.load(Ordering::Relaxed) {
                return Self::finish(
                    scan_id,
                    &workspace_ids,
                    HashMap::new(),
                    ScanProgress {
                        discovered_count,
                        measured_count: 0,
                        skipped_entries,
                        cancelled: true,
                        truncated,
                        uninspected,
                    },
                    selection.rules,
                    &mut on_event,
                );
            }

            on_event(DeveloperArtifactScanEvent::WorkspaceStarted {
                workspace: workspace.workspace.clone(),
            });

            if let Some(candidate) =
                global_go_module_candidate(environment, workspace, &mut seen_paths)
            {
                if candidates.len() < MAX_CANDIDATES {
                    on_event(DeveloperArtifactScanEvent::ProjectDiscovered {
                        workspace_id: workspace.workspace.id.clone(),
                        project_name: candidate.project_name.clone(),
                        ecosystem: candidate.ecosystem,
                    });
                    candidates.push(candidate);
                    discovered_count = discovered_count.saturating_add(1);
                } else {
                    truncated = true;
                }
            }

            discover_workspace(
                environment,
                workspace,
                downloads_access,
                &cancel,
                &mut candidates,
                &mut discovered_count,
                &mut skipped_entries,
                &mut truncated,
                &mut seen_paths,
                &mut uninspected,
                &mut uninspected_seen,
                &mut on_event,
            );
            on_event(DeveloperArtifactScanEvent::WorkspaceFinished {
                workspace_id: workspace.workspace.id.clone(),
            });

            if cancel.load(Ordering::Relaxed) {
                return Self::finish(
                    scan_id,
                    &workspace_ids,
                    HashMap::new(),
                    ScanProgress {
                        discovered_count,
                        measured_count: 0,
                        skipped_entries,
                        cancelled: true,
                        truncated,
                        uninspected,
                    },
                    selection.rules,
                    &mut on_event,
                );
            }
        }

        // Reuse the bounded shared scan pool for independent projects. Candidates
        // with overlapping project-use scopes run sequentially: another
        // measurement's descriptors or Git subprocess must not manufacture
        // active use in a sibling's exact project probe.
        // Cancellation remains checked per candidate and per tree entry.
        let (sender, receiver) = mpsc::channel();
        let worker_cancel = cancel.clone();
        let worker_groups = project_measurement_groups(candidates);
        let worker_environment = environment.clone();
        let worker = std::thread::spawn(move || {
            if let Some(pool) = crate::execution_budget::shared_scan_pool() {
                pool.install(|| {
                    worker_groups
                        .into_par_iter()
                        .for_each_with(sender, |tx, group| {
                            for candidate in group {
                                if worker_cancel.load(Ordering::Relaxed) {
                                    break;
                                }
                                measure_candidate(
                                    &worker_environment,
                                    candidate,
                                    tx,
                                    &worker_cancel,
                                );
                            }
                        });
                });
            } else {
                // Single-worker fallback: measure sequentially without
                // touching the global Rayon pool.
                for candidate in worker_groups.into_iter().flatten() {
                    if worker_cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    measure_candidate(&worker_environment, candidate, &sender, &worker_cancel);
                }
            }
        });

        let mut records = HashMap::new();
        let mut measured_count = 0u64;
        for message in receiver {
            match message {
                MeasurementMessage::Started {
                    artifact_id,
                    project_name,
                    kind,
                } => on_event(DeveloperArtifactScanEvent::ArtifactMeasurementStarted {
                    artifact_id,
                    project_name,
                    kind,
                }),
                MeasurementMessage::Finished {
                    candidate,
                    stats,
                    ownership,
                } => {
                    measured_count = measured_count.saturating_add(1);
                    let Some(mut record) = record_from_measurement(*candidate, stats, ownership)
                    else {
                        skipped_entries = skipped_entries.saturating_add(1);
                        continue;
                    };
                    if Blacklist::is_blacklisted_with(Path::new(&record.artifact.path), environment)
                    {
                        record.artifact.status = DeveloperArtifactStatus::SafetyBlocked;
                        record.artifact.incomplete_reason = Some(
                            "This artifact is inside a protected location; cleanup is blocked."
                                .into(),
                        );
                    }

                    if record.artifact.status != DeveloperArtifactStatus::Complete {
                        skipped_entries = skipped_entries.saturating_add(1);
                    }
                    let artifact = record.artifact.clone();
                    let workspace_id = artifact.workspace_id.clone();
                    records.insert(artifact.id.clone(), record);
                    on_event(DeveloperArtifactScanEvent::ArtifactFound { artifact });
                    on_event(DeveloperArtifactScanEvent::Progress {
                        workspace_id,
                        discovered_count,
                        measured_count,
                        skipped_entries,
                    });
                }
            }
        }
        let _ = worker.join();

        let cancelled = cancel.load(Ordering::Relaxed);
        if !cancelled {
            preselection::apply(&mut records, selection, &|| cancel.load(Ordering::Relaxed));
        }
        Self::finish(
            scan_id,
            &workspace_ids,
            records,
            ScanProgress {
                discovered_count,
                measured_count,
                skipped_entries,
                cancelled,
                truncated,
                uninspected,
            },
            selection.rules,
            &mut on_event,
        )
    }

    fn finish<F>(
        scan_id: String,
        workspace_ids: &[String],
        records: HashMap<String, DeveloperArtifactRecord>,
        progress: ScanProgress,
        rules: crate::models::CleanupRules,
        on_event: &mut F,
    ) -> Result<DeveloperArtifactInventory, String>
    where
        F: FnMut(DeveloperArtifactScanEvent),
    {
        let mut records = records;
        if progress.cancelled {
            for record in records.values_mut() {
                record.artifact.ownership = ArtifactOwnershipEvidence::Incomplete(
                    crate::models::ArtifactOwnershipUncertainty::Cancelled,
                );
                record.artifact.status = DeveloperArtifactStatus::ScanCancelled;
                record.artifact.incomplete_reason = Some(
                    "The scan was cancelled before this artifact could be fully verified. Scan again before cleanup."
                        .to_string(),
                );
            }
        }
        let inventory = DeveloperArtifactInventory {
            rules,
            scan_id: scan_id.clone(),
            records,
            workspace_ids: workspace_ids.to_vec(),
            created_at: unix_timestamp(),
            discovered_count: progress.discovered_count,
            measured_count: progress.measured_count,
            skipped_entries: progress.skipped_entries,
            cancelled: progress.cancelled,
            truncated: progress.truncated,
            uninspected: progress.uninspected,
        };
        if progress.cancelled {
            on_event(DeveloperArtifactScanEvent::Cancelled {
                scan_id: scan_id.clone(),
            });
        }
        let result = result_from_inventory(&inventory);
        on_event(DeveloperArtifactScanEvent::Finished { result });
        Ok(inventory)
    }
}

pub fn result_from_inventory(
    inventory: &DeveloperArtifactInventory,
) -> DeveloperArtifactScanResult {
    let mut items = inventory
        .records
        .values()
        .map(|record| record.artifact.clone())
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .allocated_bytes
            .cmp(&left.allocated_bytes)
            .then_with(|| right.logical_bytes.cmp(&left.logical_bytes))
            .then_with(|| left.project_name.cmp(&right.project_name))
            .then_with(|| left.path.cmp(&right.path))
    });
    DeveloperArtifactScanResult {
        scan_id: inventory.scan_id.clone(),
        items,
        discovered_count: inventory.discovered_count,
        measured_count: inventory.measured_count,
        skipped_entries: inventory.skipped_entries,
        cancelled: inventory.cancelled,
        truncated: inventory.truncated,
        uninspected: inventory.uninspected.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn discover_workspace<F>(
    environment: &PlatformEnvironment,
    workspace: &DeveloperWorkspaceRecord,
    downloads_access: FolderAccess,
    cancel: &AtomicBool,
    candidates: &mut Vec<Candidate>,
    discovered_count: &mut u64,
    skipped_entries: &mut u64,
    truncated: &mut bool,
    seen_paths: &mut HashSet<PathBuf>,
    uninspected: &mut Vec<DeveloperArtifactUninspected>,
    uninspected_seen: &mut HashSet<PathBuf>,
    on_event: &mut F,
) where
    F: FnMut(DeveloperArtifactScanEvent),
{
    let mut pending = VecDeque::from([(workspace.path.clone(), 0usize)]);
    let mut entries_seen = 0u64;
    while let Some((directory, depth)) = pending.pop_front() {
        if cancel.load(Ordering::Relaxed) || entries_seen >= MAX_DISCOVERY_ENTRIES {
            if entries_seen >= MAX_DISCOVERY_ENTRIES {
                *truncated = true;
            }
            return;
        }
        if depth > MAX_DISCOVERY_DEPTH {
            *skipped_entries = skipped_entries.saturating_add(1);
            continue;
        }
        let downloads_refusal = match downloads_access {
            FolderAccess::Denied => {
                Some((DeveloperArtifactUninspectedReason::PermissionDenied, true))
            }
            FolderAccess::Unreadable => {
                Some((DeveloperArtifactUninspectedReason::Unreadable, false))
            }
            FolderAccess::NotGated | FolderAccess::Granted => None,
        };
        if let Some((reason, retryable)) =
            downloads_refusal.filter(|_| is_downloads_directory(environment, &directory))
        {
            // The gate already parked on this folder: reading it here would
            // wait on the same prompt a second time, inside the storage gate.
            record_uninspected(
                uninspected,
                uninspected_seen,
                on_event,
                &directory,
                reason,
                retryable,
            );
            *skipped_entries = skipped_entries.saturating_add(1);
            continue;
        }
        // Config-derived paths are observations only. Keep unknown wrappers
        // traversable; an advisory custom parent must not hide nested packages.
        if is_regular_file(&directory.join("package.json")) {
            for found in rules::framework_custom_matches(&directory) {
                let path = directory.join(&found.artifact_relative);
                if !path.starts_with(&workspace.path)
                    || path == directory
                    || !fs::symlink_metadata(&path).ok().is_some_and(|m| m.is_dir())
                    || validate_observed_components(&workspace.path, &path).is_err()
                    || !seen_paths.insert(path.clone())
                {
                    continue;
                }
                if candidates.len() >= MAX_CANDIDATES {
                    *truncated = true;
                    break;
                }
                let candidate =
                    candidate_from_match(workspace, path, &directory, "custom output", found);
                *discovered_count = discovered_count.saturating_add(1);
                candidates.push(candidate);
            }
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                // A refused directory is reported by name instead of being
                // folded into the anonymous skip counter, so a partial scan
                // can say what it missed and offer a retry.
                let (reason, retryable) = match error.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        (DeveloperArtifactUninspectedReason::PermissionDenied, true)
                    }
                    _ => (DeveloperArtifactUninspectedReason::Unreadable, false),
                };
                record_uninspected(
                    uninspected,
                    uninspected_seen,
                    on_event,
                    &directory,
                    reason,
                    retryable,
                );
                *skipped_entries = skipped_entries.saturating_add(1);
                continue;
            }
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen >= MAX_DISCOVERY_ENTRIES {
                *truncated = true;
                return;
            }
            let Ok(entry) = entry else {
                *skipped_entries = skipped_entries.saturating_add(1);
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                *skipped_entries = skipped_entries.saturating_add(1);
                continue;
            };
            if SymlinkGuard::is_symlink(&path) {
                *skipped_entries = skipped_entries.saturating_add(1);
                continue;
            }
            if !metadata.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" {
                continue;
            }
            if should_skip_protected_discovery_path(environment, workspace, &path, &name) {
                *skipped_entries = skipped_entries.saturating_add(1);
                continue;
            }
            if seen_paths.contains(&path)
                && !candidates
                    .iter()
                    .any(|c| c.path == path && artifact_is_framework_parent(c.kind))
            {
                // A special candidate such as ~/go/pkg/mod may have been
                // registered before its parent directory is visited.
                continue;
            }

            if let Some(candidate) = recognize_artifact(workspace, &directory, &name) {
                if seen_paths.iter().any(|seen| {
                    (candidate.path.starts_with(seen) || seen.starts_with(&candidate.path))
                        && !candidates
                            .iter()
                            .any(|c| &c.path == seen && artifact_is_framework_parent(c.kind))
                }) {
                    continue;
                }
                if !seen_paths.insert(candidate.path.clone()) {
                    continue;
                }
                if candidates.len() >= MAX_CANDIDATES {
                    *truncated = true;
                    continue;
                }
                on_event(DeveloperArtifactScanEvent::ProjectDiscovered {
                    workspace_id: workspace.workspace.id.clone(),
                    project_name: candidate.project_name.clone(),
                    ecosystem: candidate.ecosystem,
                });
                *discovered_count = discovered_count.saturating_add(1);
                let generated_children = if artifact_is_framework_parent(candidate.kind) {
                    rules::framework_generated_matches(&candidate.project_root)
                        .into_iter()
                        .filter_map(|found| {
                            let path = candidate.project_root.join(&found.artifact_relative);
                            if !fs::symlink_metadata(&path)
                                .ok()
                                .is_some_and(|metadata| metadata.is_dir())
                                || SymlinkGuard::is_symlink(&path)
                            {
                                return None;
                            }
                            Some(Candidate {
                                id: Uuid::new_v4().to_string(),
                                workspace: candidate.workspace.clone(),
                                project_name: candidate.project_name.clone(),
                                ecosystem: found.ecosystem,
                                kind: found.kind,
                                path,
                                project_root: found.project_root,
                                artifact_relative: found.artifact_relative,
                                marker_paths: found.marker_paths,
                                evidence: found.evidence,
                                rebuild_hint: found.rebuild_hint,
                            })
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                };
                let framework_parent = artifact_is_framework_parent(candidate.kind);
                candidates.push(candidate);
                for child in generated_children {
                    if candidates.len() >= MAX_CANDIDATES {
                        *truncated = true;
                        break;
                    }
                    if !seen_paths.insert(child.path.clone()) {
                        continue;
                    }
                    *discovered_count = discovered_count.saturating_add(1);
                    candidates.push(child);
                }
                // Framework recognition does not attest the complete output.
                // Keep unknown/nested packages traversable while registered
                // generated children and vendor stores retain their boundary.
                if framework_parent {
                    pending.push_back((path, depth + 1));
                }
                continue;
            }

            if should_skip_discovery_directory(&name) {
                continue;
            }
            pending.push_back((path, depth + 1));
        }
    }
}

fn should_skip_discovery_directory(name: &str) -> bool {
    name == ".git" || rules::is_artifact_directory(name)
}

/// True when the walk reached the platform's Downloads folder.
///
/// The direct comparison covers the ordinary case and, importantly, the denied
/// case: a refused folder cannot be resolved, so the canonical comparison below
/// would fail closed on the very directory it is meant to recognize.
fn is_downloads_directory(environment: &PlatformEnvironment, path: &Path) -> bool {
    let Some(downloads) = environment.content_dir("downloads") else {
        return false;
    };
    if path == downloads {
        return true;
    }
    match (fs::canonicalize(path), fs::canonicalize(&downloads)) {
        (Ok(path), Ok(downloads)) => path == downloads,
        _ => false,
    }
}

/// Records a directory the walk could not inspect and reports it once.
fn record_uninspected<F>(
    uninspected: &mut Vec<DeveloperArtifactUninspected>,
    seen: &mut HashSet<PathBuf>,
    on_event: &mut F,
    directory: &Path,
    reason: DeveloperArtifactUninspectedReason,
    retryable: bool,
) where
    F: FnMut(DeveloperArtifactScanEvent),
{
    if !seen.insert(directory.to_path_buf()) {
        return;
    }
    let name = directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| directory.to_string_lossy().into_owned());
    let path = directory.to_string_lossy().into_owned();
    on_event(DeveloperArtifactScanEvent::Uninspected {
        path: path.clone(),
        name: name.clone(),
        reason,
        retryable,
    });
    uninspected.push(DeveloperArtifactUninspected {
        path,
        name,
        reason,
        retryable,
    });
}

fn should_skip_protected_discovery_path(
    environment: &PlatformEnvironment,
    workspace: &DeveloperWorkspaceRecord,
    path: &Path,
    name: &str,
) -> bool {
    if name.ends_with(".app") {
        return true;
    }
    if !workspace.whole_home {
        return false;
    }
    // Downloads discovery is read-only and reports denied consent explicitly.
    // Its artifacts remain blocked for deletion by the policy check on the record.
    let inside_downloads = environment.content_dir("downloads").is_some_and(|root| {
        path_algebra::contains(
            &root.to_string_lossy(),
            &path.to_string_lossy(),
            environment.flavor(),
        )
    });
    if !inside_downloads && Blacklist::is_blacklisted_with(path, environment) {
        return true;
    }
    let credential_names = [
        ".ssh", ".gnupg", ".aws", ".azure", ".kube", ".config", ".Trash",
    ];
    if credential_names.contains(&name) {
        return true;
    }
    if path.parent() != Some(workspace.path.as_path()) {
        return false;
    }
    matches!(
        name,
        "Library"
            | "Desktop"
            | "Documents"
            | "Pictures"
            | "Movies"
            | "Music"
            | ".cache"
            | ".local"
            | ".cargo"
            | ".rustup"
            | ".npm"
            | ".pnpm-store"
            | ".yarn"
            | ".bun"
            | ".gradle"
            | ".m2"
            | ".ivy2"
            | ".nuget"
            | ".gem"
            | ".composer"
            | ".docker"
            | ".orbstack"
            | ".vscode"
            | ".vscode-insiders"
    )
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
