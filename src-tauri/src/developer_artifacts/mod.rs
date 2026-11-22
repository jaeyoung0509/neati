#[cfg(test)]
mod framework_tests;
mod rules;
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
    DeveloperWorkspace, ReviewedFileIdentity,
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
}

impl DeveloperArtifactInventory {
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
        mut on_event: F,
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
            &mut on_event,
        )
    }

    fn finish<F>(
        scan_id: String,
        workspace_ids: &[String],
        records: HashMap<String, DeveloperArtifactRecord>,
        progress: ScanProgress,
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

pub fn workspace_snapshot(
    ids: &[String],
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> Result<Vec<DeveloperWorkspaceRecord>, String> {
    if ids.is_empty() {
        return Err("Select at least one workspace to scan.".to_string());
    }
    let records = workspaces_store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut result = Vec::with_capacity(ids.len());
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let record = records
            .get(id)
            .cloned()
            .ok_or_else(|| "The selected workspace is unknown. Add it again.".to_string())?;
        let current = identity_from_path(&record.path)
            .ok_or_else(|| "The selected workspace disappeared or became a symlink.".to_string())?;
        if !record.path.is_dir() || !same_workspace_directory_identity(&current, &record.identity) {
            return Err(
                "The selected workspace changed. Add it again before scanning.".to_string(),
            );
        }
        result.push(record);
    }
    if result.is_empty() {
        return Err("Select at least one workspace to scan.".to_string());
    }
    Ok(result)
}

fn same_workspace_directory_identity(
    current: &ReviewedFileIdentity,
    expected: &ReviewedFileIdentity,
) -> bool {
    current.same_entity(expected)
}

pub fn pick_workspace(
    environment: &PlatformEnvironment,
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> Result<Option<DeveloperWorkspace>, String> {
    let Some(path) = native_pick_workspace_path()? else {
        return Ok(None);
    };
    Ok(Some(
        register_workspace_path(environment, &path, workspaces_store)?.workspace,
    ))
}

pub fn register_home_workspace(
    environment: &PlatformEnvironment,
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> Result<DeveloperWorkspace, String> {
    let home = environment
        .user_home()
        .ok_or_else(|| "Could not resolve the user home directory".to_string())?;
    if SymlinkGuard::is_symlink(&home) {
        return Err("The user home directory must not be a symbolic link or reparse point.".into());
    }
    let canonical = fs::canonicalize(&home)
        .map_err(|_| "Could not resolve the user home directory".to_string())?;
    let metadata = fs::symlink_metadata(&canonical)
        .map_err(|_| "Could not inspect the user home directory".to_string())?;
    if !metadata.is_dir() || SymlinkGuard::is_symlink(&canonical) {
        return Err("The user home directory is not a stable directory.".to_string());
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err("The user home directory must be owned by the current user.".to_string());
    }
    let name = "This Computer".to_string();
    Ok(store_workspace(canonical, name, true, workspaces_store)?.workspace)
}

pub fn register_workspace_path(
    environment: &PlatformEnvironment,
    path: &Path,
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> Result<DeveloperWorkspaceRecord, String> {
    let canonical = validate_workspace_root(environment, path)?;
    let name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Workspace")
        .to_string();
    store_workspace(canonical, name, false, workspaces_store)
}

fn store_workspace(
    canonical: PathBuf,
    name: String,
    whole_home: bool,
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> Result<DeveloperWorkspaceRecord, String> {
    let identity = identity_from_path(&canonical)
        .ok_or_else(|| "The selected workspace is not a stable directory.".to_string())?;

    let mut workspaces = workspaces_store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = workspaces.values().find(|record| record.path == canonical) {
        return Ok(existing.clone());
    }
    if workspaces.len() >= MAX_WORKSPACES {
        if let Some(oldest_id) = workspaces
            .iter()
            .min_by_key(|(_, record)| record.created_at)
            .map(|(id, _)| id.clone())
        {
            workspaces.remove(&oldest_id);
        }
    }
    let id = Uuid::new_v4().to_string();
    let record = DeveloperWorkspaceRecord {
        workspace: DeveloperWorkspace {
            id: id.clone(),
            name,
            display_path: neati_platform::path_algebra::normalize_lexical(&canonical)
                .to_string_lossy()
                .into_owned(),
        },
        path: canonical,
        identity,
        created_at: unix_timestamp(),
        whole_home,
    };
    workspaces.insert(id, record.clone());
    Ok(record)
}

/// Validates a workspace root against the environment's stated profile.
///
/// The location rules are decided by the environment's [`PathFlavor`], so a
/// `D:\dev` workspace and a UNC profile are decided by the same code that
/// decides a POSIX profile, and a Windows-shaped refusal is provable on any
/// runner. Nothing here reads the host's environment: an unstated home simply
/// has no profile-relative rules.
pub fn validate_workspace_root(
    environment: &PlatformEnvironment,
    path: &Path,
) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "Choose an existing workspace directory.".to_string())?;
    if !metadata.is_dir() || SymlinkGuard::is_symlink(path) {
        return Err("The selected workspace must be a real directory.".to_string());
    }
    let canonical = fs::canonicalize(path)
        .map_err(|_| "Could not resolve the selected workspace".to_string())?;
    let flavor = environment.flavor();
    let norm_canonical = PathBuf::from(path_algebra::strip_verbatim(
        &canonical.to_string_lossy(),
        flavor,
    ));

    let canonical_home = environment.user_home().map(|home| {
        fs::canonicalize(&home).unwrap_or_else(|_| {
            PathBuf::from(path_algebra::strip_verbatim(
                &home.to_string_lossy(),
                flavor,
            ))
        })
    });
    if let Some(reason) =
        workspace_root_scope_refusal(flavor, &norm_canonical, canonical_home.as_deref())
    {
        return Err(reason);
    }

    match &canonical_home {
        Some(home)
            if path_algebra::contains(
                &home.to_string_lossy(),
                &norm_canonical.to_string_lossy(),
                flavor,
            ) =>
        {
            SymlinkGuard::validate_no_symlink_ancestors(path, home, environment).map_err(|_| {
                "The selected workspace contains a symbolic-link component.".to_string()
            })?;
        }
        _ => {
            SymlinkGuard::validate_anchored_path(&norm_canonical, environment).map_err(|_| {
                "The selected workspace contains a symbolic-link component.".to_string()
            })?;
        }
    }

    if Blacklist::is_blacklisted_with(&norm_canonical, environment) {
        return Err("That location is protected and cannot be used as a workspace.".to_string());
    }

    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err("Workspace roots must be owned by the current user.".to_string());
    }
    Ok(norm_canonical)
}

/// Pure location rules for a workspace root, parameterized by path flavor and
/// by the profile the environment states.
///
/// Kept separate from [`validate_workspace_root`] so the rules can be asserted
/// for a flavor this runner is not: a non-`C:` system drive and a UNC profile
/// are accepted, their roots and profile-relative protected folders are not.
fn workspace_root_scope_refusal(
    flavor: path_algebra::PathFlavor,
    path: &Path,
    home: Option<&Path>,
) -> Option<String> {
    let text = path.to_string_lossy();
    if path_algebra::is_root(&text, flavor) {
        return Some("Drive root or filesystem root cannot be used as a workspace.".to_string());
    }
    if path_algebra::is_unsupported_namespace(&text, flavor) {
        return Some("A device or NT namespace path cannot be used as a workspace.".to_string());
    }
    let home = home?;
    let home_text = home.to_string_lossy();
    if path_algebra::equal(&text, &home_text, flavor) {
        return Some("Your entire home directory cannot be used as a workspace root.".to_string());
    }
    if path_algebra::contains(&home_text, &text, flavor) {
        const PROTECTED_WORKSPACE_PREFIXES: [&str; 12] = [
            "Library",
            ".ssh",
            ".gnupg",
            ".aws",
            ".azure",
            ".kube",
            ".config",
            "Desktop",
            "Documents",
            "Pictures",
            "Movies",
            "Music",
        ];
        if PROTECTED_WORKSPACE_PREFIXES.iter().any(|prefix| {
            let protected = home.join(prefix);
            path_algebra::contains(&protected.to_string_lossy(), &text, flavor)
        }) {
            return Some(
                "That location is protected and cannot be used as a workspace.".to_string(),
            );
        }
    }
    None
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

fn recognize_artifact(
    workspace: &DeveloperWorkspaceRecord,
    project_root: &Path,
    child_name: &str,
) -> Option<Candidate> {
    let artifact_match = rules::recognize(project_root, child_name)?;
    let candidate_path = project_root.join(&artifact_match.artifact_relative);
    if !candidate_path.is_dir() {
        return None;
    }
    Some(candidate_from_match(
        workspace,
        candidate_path,
        project_root,
        child_name,
        artifact_match,
    ))
}

fn validate_observed_components(root: &Path, path: &Path) -> Result<(), ()> {
    let relative = path.strip_prefix(root).map_err(|_| ())?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(());
        };
        cursor.push(name);
        if SymlinkGuard::is_symlink(&cursor) {
            return Err(());
        }
    }
    Ok(())
}

fn global_go_module_candidate(
    environment: &PlatformEnvironment,
    workspace: &DeveloperWorkspaceRecord,
    seen_paths: &mut HashSet<PathBuf>,
) -> Option<Candidate> {
    let home = environment.user_home()?;
    let canonical_home = fs::canonicalize(&home).ok()?;
    let expected_root = fs::canonicalize(home.join("go")).ok()?;
    if workspace.path != expected_root && workspace.path != canonical_home {
        return None;
    }
    let path = expected_root.join("pkg/mod");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_dir() || SymlinkGuard::is_symlink(&path) || !seen_paths.insert(path.clone()) {
        return None;
    }
    Some(Candidate {
        id: Uuid::new_v4().to_string(),
        workspace: workspace.clone(),
        project_name: "Go module cache".to_string(),
        ecosystem: DeveloperEcosystem::Go,
        kind: DeveloperArtifactKind::GoModuleCache,
        path,
        project_root: expected_root,
        artifact_relative: PathBuf::from("pkg/mod"),
        marker_paths: Vec::new(),
        evidence: vec![if workspace.whole_home {
            "Built-in whole-home scan scope".to_string()
        } else {
            "Explicitly selected ~/go workspace".to_string()
        }],
        rebuild_hint: Some("go mod download".to_string()),
    })
}

fn candidate_from_match(
    workspace: &DeveloperWorkspaceRecord,
    path: PathBuf,
    project_root: &Path,
    child_name: &str,
    artifact_match: ArtifactMatch,
) -> Candidate {
    let project_name = artifact_match
        .project_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(project_root.to_string_lossy().as_ref())
        .to_string();
    let artifact_relative = if artifact_match.artifact_relative.as_os_str().is_empty() {
        PathBuf::from(child_name)
    } else {
        artifact_match.artifact_relative
    };
    Candidate {
        id: Uuid::new_v4().to_string(),
        workspace: workspace.clone(),
        project_name,
        ecosystem: artifact_match.ecosystem,
        kind: artifact_match.kind,
        path,
        project_root: artifact_match.project_root,
        artifact_relative,
        marker_paths: artifact_match.marker_paths,
        evidence: artifact_match.evidence,
        rebuild_hint: artifact_match.rebuild_hint,
    }
}

fn recognize_node(project_root: &Path) -> Option<ArtifactMatch> {
    let package_json = find_named_marker(project_root, &["package.json"])?;
    let mut marker_paths = vec![package_json.clone()];
    let mut evidence = vec!["package.json".to_string()];
    let lockfiles = [
        ("pnpm-lock.yaml", "pnpm install"),
        ("package-lock.json", "npm ci"),
        ("yarn.lock", "yarn install"),
        ("bun.lock", "bun install"),
        ("bun.lockb", "bun install"),
    ];
    let package_root = package_json.parent()?;
    let mut hint = "npm install".to_string();
    for (lockfile, lock_hint) in lockfiles {
        let path = package_root.join(lockfile);
        if is_regular_file(&path) {
            marker_paths.push(path);
            evidence.push(lockfile.to_string());
            hint = lock_hint.to_string();
            break;
        }
    }
    Some(ArtifactMatch {
        ecosystem: DeveloperEcosystem::Node,
        kind: DeveloperArtifactKind::NodeModules,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from("node_modules"),
        marker_paths,
        evidence,
        rebuild_hint: Some(hint),
    })
}

fn recognize_python(project_root: &Path, child_name: &str) -> Option<ArtifactMatch> {
    let names = [
        "pyproject.toml",
        "uv.lock",
        "poetry.lock",
        "requirements.txt",
        "Pipfile.lock",
    ];
    let marker = find_named_marker(project_root, &names)?;
    if !is_regular_file(&project_root.join(child_name).join("pyvenv.cfg")) {
        return None;
    }
    let marker_name = marker.file_name()?.to_string_lossy();
    let hint = match marker_name.as_ref() {
        "uv.lock" => "uv sync",
        "poetry.lock" => "poetry install",
        "requirements.txt" => "python -m pip install -r requirements.txt",
        "Pipfile.lock" => "pipenv sync",
        _ => "python -m pip install -e .",
    };
    Some(ArtifactMatch {
        ecosystem: DeveloperEcosystem::Python,
        kind: DeveloperArtifactKind::PythonVenv,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from(child_name),
        marker_paths: vec![marker.clone()],
        evidence: vec![marker_name.into_owned()],
        rebuild_hint: Some(hint.to_string()),
    })
}

fn recognize_vendor(project_root: &Path) -> Option<ArtifactMatch> {
    if let Some(marker) = find_named_marker(project_root, &["composer.json"]) {
        let vendor = project_root.join("vendor");
        let installed = vendor.join("composer/installed.json");
        let autoload = vendor.join("autoload.php");
        let generated_evidence = if is_regular_file(&installed) {
            installed
        } else if is_regular_file(&autoload) {
            autoload
        } else {
            return None;
        };
        let mut marker_paths = vec![marker];
        let lock = project_root.join("composer.lock");
        let mut evidence = vec!["composer.json".to_string()];
        if is_regular_file(&lock) {
            marker_paths.push(lock);
            evidence.push("composer.lock".to_string());
        }
        marker_paths.push(generated_evidence.clone());
        evidence.push(
            generated_evidence
                .strip_prefix(project_root)
                .unwrap_or(&generated_evidence)
                .to_string_lossy()
                .into_owned(),
        );
        return Some(ArtifactMatch {
            ecosystem: DeveloperEcosystem::Php,
            kind: DeveloperArtifactKind::ComposerVendor,
            project_root: project_root.to_path_buf(),
            artifact_relative: PathBuf::from("vendor"),
            marker_paths,
            evidence,
            rebuild_hint: Some("composer install".to_string()),
        });
    }

    let marker = find_named_marker(project_root, &["Gemfile"])?;
    let bundle = project_root.join("vendor/bundle");
    if !bundle.is_dir() {
        return None;
    }
    let mut marker_paths = vec![marker];
    let lock = project_root.join("Gemfile.lock");
    let mut evidence = vec!["Gemfile".to_string()];
    if is_regular_file(&lock) {
        marker_paths.push(lock);
        evidence.push("Gemfile.lock".to_string());
    }
    Some(ArtifactMatch {
        ecosystem: DeveloperEcosystem::Ruby,
        kind: DeveloperArtifactKind::RubyBundle,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from("vendor/bundle"),
        marker_paths,
        evidence,
        rebuild_hint: Some("bundle install".to_string()),
    })
}

fn recognize_build(project_root: &Path, child_name: &str) -> Option<ArtifactMatch> {
    let gradle = find_named_marker(
        project_root,
        &[
            "build.gradle.kts",
            "build.gradle",
            "settings.gradle.kts",
            "settings.gradle",
            "gradlew",
        ],
    );
    if let Some(marker) = gradle {
        let is_kotlin = marker
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".kts"));
        return Some(ArtifactMatch {
            ecosystem: if is_kotlin {
                DeveloperEcosystem::Kotlin
            } else {
                DeveloperEcosystem::Java
            },
            kind: DeveloperArtifactKind::GradleBuild,
            project_root: project_root.to_path_buf(),
            artifact_relative: PathBuf::from(child_name),
            marker_paths: vec![marker.clone()],
            evidence: vec![marker.file_name()?.to_string_lossy().into_owned()],
            rebuild_hint: Some("./gradlew build".to_string()),
        });
    }
    let marker = find_named_marker(project_root, &["CMakeLists.txt"])?;
    let cache = project_root.join(child_name).join("CMakeCache.txt");
    if !is_regular_file(&cache) {
        return None;
    }
    Some(ArtifactMatch {
        ecosystem: native_cmake_ecosystem(project_root),
        kind: DeveloperArtifactKind::CMakeBuild,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from(child_name),
        marker_paths: vec![marker, cache],
        evidence: vec![
            "CMakeLists.txt".to_string(),
            "build/CMakeCache.txt".to_string(),
        ],
        rebuild_hint: Some("cmake --build build".to_string()),
    })
}

fn native_cmake_ecosystem(project_root: &Path) -> DeveloperEcosystem {
    let Ok(entries) = fs::read_dir(project_root) else {
        return DeveloperEcosystem::Cpp;
    };
    let mut has_c = false;
    for path in entries.flatten().map(|entry| entry.path()) {
        match path.extension().and_then(|value| value.to_str()) {
            Some("cc" | "cpp" | "cxx" | "mm") => return DeveloperEcosystem::Cpp,
            Some("c") => has_c = true,
            _ => {}
        }
    }
    if has_c {
        DeveloperEcosystem::C
    } else {
        DeveloperEcosystem::Cpp
    }
}

fn find_named_marker(project_root: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| project_root.join(name))
        .find(|path| is_regular_file(path))
}

fn find_project_extension_marker(project_root: &Path, extensions: &[&str]) -> Option<PathBuf> {
    if extensions.is_empty() {
        return None;
    }
    let entries = fs::read_dir(project_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_regular_file(&path) {
            continue;
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            return Some(path);
        }
    }
    None
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
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

fn project_measurement_groups(mut candidates: Vec<Candidate>) -> Vec<Vec<Candidate>> {
    // Place ancestors first so each group's first root covers every member's
    // project-use scope. Discovery already bounds the number of candidates.
    candidates.sort_by_key(|candidate| candidate.project_root.components().count());
    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    for candidate in candidates {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| candidate.project_root.starts_with(&group[0].project_root))
        {
            group.push(candidate);
        } else {
            groups.push(vec![candidate]);
        }
    }
    groups
}

fn measure_candidate(
    environment: &PlatformEnvironment,
    candidate: Candidate,
    tx: &mpsc::Sender<MeasurementMessage>,
    cancel: &AtomicBool,
) {
    let _ = tx.send(MeasurementMessage::Started {
        artifact_id: candidate.id.clone(),
        project_name: candidate.project_name.clone(),
        kind: candidate.kind,
    });
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let stats = measure_tree(
        &candidate.path,
        candidate.workspace.identity.device(),
        cancel,
        0,
    );
    let ownership = neati_platform::artifact_ownership::probe_artifact_ownership(
        environment,
        &candidate.project_root,
        &candidate.path,
        cancel,
    );
    let mut candidate = candidate;
    let mut stats = stats;
    if framework_generated_kind(candidate.kind).is_some() {
        if let Some(format) = framework_generated_kind(candidate.kind) {
            if let Ok(metadata) =
                neati_platform::framework_metadata::observe(&candidate.project_root, format)
            {
                candidate.evidence.extend(metadata.evidence);
            }
        }
        let contract = verify_framework_generated_contract(
            environment,
            &candidate.project_root,
            &candidate.path,
            candidate.kind,
            cancel,
        );
        if let Err(reason) = contract {
            if artifact_is_framework_parent(candidate.kind) {
                stats.framework_advisory = Some(reason.clone());
            } else {
                stats.safety_blocked = true;
            }
            candidate.evidence.push(reason);
        } else if neati_platform::temporary_storage::observe_temporary_use(
            environment,
            &candidate.project_root,
        )
        .state
            != neati_core::domain::storage::TemporaryUsageState::NoUseDetected
        {
            stats.safety_blocked = true;
            candidate.evidence.push("Framework project use is active or unknown; cleanup is unavailable until a complete idle observation.".into());
        } else {
            stats.framework_verified = true;
            candidate.evidence.push("A completed macOS use probe detected no current use. This does not establish future abandonment.".into());
        }
    }
    let _ = tx.send(MeasurementMessage::Finished {
        candidate: Box::new(candidate),
        stats,
        ownership,
    });
}

fn measure_tree(path: &Path, _root_device: u64, cancel: &AtomicBool, depth: usize) -> TreeStats {
    measure_tree_bounded(
        path,
        _root_device,
        cancel,
        depth,
        &mut (0usize, std::time::Instant::now()),
    )
}

fn measure_tree_bounded(
    path: &Path,
    _root_device: u64,
    cancel: &AtomicBool,
    depth: usize,
    budget: &mut (usize, std::time::Instant),
) -> TreeStats {
    let mut stats = TreeStats::new();
    budget.0 += 1;
    if budget.0 > MAX_DISCOVERY_ENTRIES as usize
        || budget.1.elapsed() > std::time::Duration::from_secs(5)
    {
        stats.complete = false;
        return stats;
    }
    if cancel.load(Ordering::Relaxed) {
        stats.complete = false;
        stats.cancelled = true;
        return stats;
    }
    if depth > MAX_MEASUREMENT_DEPTH {
        stats.complete = false;
        return stats;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            stats.complete = false;
            return stats;
        }
    };
    #[cfg(unix)]
    if metadata.dev() != _root_device {
        stats.complete = false;
        stats.safety_blocked = true;
        return stats;
    }
    if SymlinkGuard::is_symlink(path) {
        stats.complete = false;
        // A nested link is not followed and only makes aggregate size/count
        // partial. The reviewed artifact root itself is still rejected by
        // ReviewedFileIdentity before it can become a cleanup record.
        stats.safety_blocked = depth == 0;
        merge_mtime(&mut stats.newest_mtime, metadata.modified().ok());
        stats.logical_bytes = metadata.len();
        stats.allocated_bytes = allocated_bytes(&metadata);
        stats.file_count = 1;
        return stats;
    }
    merge_mtime(&mut stats.newest_mtime, metadata.modified().ok());
    if metadata.is_file() {
        stats.logical_bytes = metadata.len();
        stats.allocated_bytes = allocated_bytes(&metadata);
        stats.file_count = 1;
        return stats;
    }
    if !metadata.is_dir() {
        return stats;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            stats.complete = false;
            return stats;
        }
    };
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            stats.complete = false;
            stats.cancelled = true;
            break;
        }
        let Ok(entry) = entry else {
            stats.complete = false;
            continue;
        };
        let child = entry.path();
        let child_stats = measure_tree_bounded(&child, _root_device, cancel, depth + 1, budget);
        stats.logical_bytes = stats
            .logical_bytes
            .saturating_add(child_stats.logical_bytes);
        stats.allocated_bytes = stats
            .allocated_bytes
            .saturating_add(child_stats.allocated_bytes);
        stats.file_count = stats.file_count.saturating_add(child_stats.file_count);
        merge_mtime(&mut stats.newest_mtime, child_stats.newest_mtime);
        if !child_stats.complete {
            stats.complete = false;
        }
        if child_stats.cancelled {
            stats.cancelled = true;
            break;
        }
        if child_stats.safety_blocked {
            stats.safety_blocked = true;
        }
        if budget.0 > MAX_DISCOVERY_ENTRIES as usize
            || budget.1.elapsed() > std::time::Duration::from_secs(5)
        {
            stats.complete = false;
            break;
        }
    }
    stats
}

fn record_from_measurement(
    candidate: Candidate,
    stats: TreeStats,
    ownership: ArtifactOwnershipEvidence,
) -> Option<DeveloperArtifactRecord> {
    if stats.complete
        && !stats.safety_blocked
        && !stats.cancelled
        && ownership.allows_cleanup()
        && stats.logical_bytes == 0
        && stats.allocated_bytes == 0
    {
        return None;
    }
    let identity = identity_from_path(&candidate.path)?;
    let project_identity = identity_from_path(&candidate.project_root)?;
    let marker_identities = candidate
        .marker_paths
        .iter()
        .filter_map(|path| identity_from_path(path).map(|identity| (path.clone(), identity)))
        .collect::<Vec<_>>();
    let status = if stats.cancelled {
        DeveloperArtifactStatus::ScanCancelled
    } else if stats.safety_blocked
        || marker_identities.len() != candidate.marker_paths.len()
        || !ownership.allows_cleanup()
    {
        DeveloperArtifactStatus::SafetyBlocked
    } else if artifact_is_framework_parent(candidate.kind) && !stats.framework_verified {
        DeveloperArtifactStatus::ObservationOnly
    } else if stats.complete {
        DeveloperArtifactStatus::Complete
    } else {
        DeveloperArtifactStatus::MeasurementIncomplete
    };
    let incomplete_reason = match status {
        DeveloperArtifactStatus::Complete => None,
        DeveloperArtifactStatus::MeasurementIncomplete => Some(
            "Some entries could not be measured; displayed size and file counts may be partial."
                .to_string(),
        ),
        DeveloperArtifactStatus::SafetyBlocked => Some(if let Some(reason) = ownership.refusal_message() {
            reason.to_string()
        } else if stats.safety_blocked && framework_generated_kind(candidate.kind).is_some() {
            candidate.evidence.last().cloned().unwrap_or_else(|| "Generated framework evidence could not be verified.".into())
        } else if stats.safety_blocked {
            "A symbolic link or filesystem boundary could not be verified; cleanup is blocked."
                .to_string()
        } else {
            "Project markers could not be verified; cleanup is blocked.".to_string()
        }),
        DeveloperArtifactStatus::ScanCancelled => Some(
            "The scan was cancelled before this artifact could be fully verified. Scan again before cleanup."
                .to_string(),
        ),
        DeveloperArtifactStatus::ObservationOnly => Some(stats.framework_advisory.unwrap_or_else(|| "Framework output is observed only; its complete generated-only ownership could not be verified.".into())),
    };
    let artifact = DeveloperArtifact {
        id: candidate.id,
        workspace_id: candidate.workspace.workspace.id.clone(),
        project_name: candidate.project_name,
        ecosystem: candidate.ecosystem,
        kind: candidate.kind,
        path: candidate.path.to_string_lossy().into_owned(),
        logical_bytes: stats.logical_bytes,
        allocated_bytes: stats.allocated_bytes,
        file_count: stats.file_count,
        newest_mtime: stats
            .newest_mtime
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs()),
        rebuild_hint: candidate.rebuild_hint,
        evidence: candidate.evidence,
        ownership,
        status,
        incomplete_reason,
        selected_by_default: false,
    };
    Some(DeveloperArtifactRecord {
        artifact,
        path: candidate.path,
        identity,
        workspace_path: candidate.workspace.path,
        workspace_identity: candidate.workspace.identity,
        project_root: candidate.project_root,
        project_identity,
        artifact_relative: candidate.artifact_relative,
        marker_identities,
    })
}

fn merge_mtime(current: &mut Option<SystemTime>, candidate: Option<SystemTime>) {
    if let Some(candidate) = candidate {
        *current = Some(match *current {
            Some(existing) => existing.max(candidate),
            None => candidate,
        });
    }
}

fn allocated_bytes(metadata: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        metadata.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        metadata.len()
    }
}

fn native_pick_workspace_path() -> Result<Option<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    {
        let script = r#"
            try
                set selectedFolder to choose folder with prompt "Choose a developer workspace"
                return POSIX path of selectedFolder
            on error number -128
                return ""
            end try
        "#;
        let output = std::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .map_err(|error| format!("Could not open the workspace picker: {error}"))?;
        if !output.status.success() {
            return Err(crate::diagnostics::sanitize_log(
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            return Ok(None);
        }
        Ok(Some(PathBuf::from(path)))
    }
    #[cfg(target_os = "windows")]
    {
        // The native folder dialog does not depend on PowerShell language
        // mode, so Constrained Language Mode cannot block workspace selection.
        let selected = rfd::FileDialog::new()
            .set_title("Choose a developer workspace")
            .pick_folder();
        Ok(selected)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(
            "Developer workspace selection is currently available on macOS and Windows only"
                .to_string(),
        )
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
