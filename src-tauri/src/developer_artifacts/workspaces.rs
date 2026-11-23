//! Project folders: registering, restoring, suggesting and validating the
//! workspaces a developer-artifact scan may walk (#486, #488).

use super::*;

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

pub(super) fn same_workspace_directory_identity(
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

/// At most this many project folders are remembered across launches (#486),
/// the same bound the in-memory workspace registry keeps.
pub const MAX_SAVED_PROJECT_FOLDERS: usize = MAX_WORKSPACES;

/// Home-relative names that commonly hold source checkouts. A match is only
/// a suggestion: nothing is scanned until the person adds it.
pub(super) const SUGGESTED_PROJECT_ROOTS: &[&str] = &[
    "Developer",
    "Projects",
    "Project",
    "Myproject",
    "Myprojects",
    "code",
    "src",
    "dev",
    "repos",
    "repositories",
    "git",
    "GitHub",
    "workspace",
    "workspaces",
    "Work",
];

/// Direct children read per suggested root; the probe never descends further.
pub(super) const SUGGESTION_CHILD_LIMIT: usize = 256;

/// Names whose presence makes a folder count as a project for a suggestion.
pub(super) const PROJECT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Package.swift",
    "pubspec.yaml",
    "composer.json",
    "Gemfile",
    "mix.exs",
    "CMakeLists.txt",
    "build.zig",
    "deno.json",
];

pub(super) fn has_project_marker(directory: &Path) -> bool {
    PROJECT_MARKERS
        .iter()
        .any(|marker| fs::symlink_metadata(directory.join(marker)).is_ok())
}

/// Counts the folder itself and its direct, unlinked child folders that carry
/// a project marker. Bounded, metadata-only and never recursive.
pub(super) fn project_count(root: &Path) -> u32 {
    let mut count = u32::from(has_project_marker(root));
    let Ok(entries) = fs::read_dir(root) else {
        return count;
    };
    for entry in entries.flatten().take(SUGGESTION_CHILD_LIMIT) {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() && !SymlinkGuard::is_symlink(&path) && has_project_marker(&path) {
            count = count.saturating_add(1);
        }
    }
    count
}

pub(super) fn canonical_home(environment: &PlatformEnvironment) -> Option<PathBuf> {
    environment
        .user_home()
        .and_then(|home| fs::canonicalize(home).ok())
}

/// Likely project folders in the home directory that are not saved yet.
///
/// Discovery is not permission (#486): a suggestion is a valid workspace root
/// that holds at least one project, and nothing is scanned until the person
/// adds it. Names that resolve to the same folder are reported once.
///
/// `session_folders` are the working directories recent coding-agent sessions
/// recorded (#506), with their session counts. Each maps to the folder that
/// holds its project; folders agents worked in come first.
pub fn suggest_workspaces(
    environment: &PlatformEnvironment,
    saved: &[String],
    session_folders: &[(PathBuf, u32)],
) -> Vec<DeveloperWorkspaceSuggestion> {
    let Some(home) = canonical_home(environment) else {
        return Vec::new();
    };
    let saved: Vec<PathBuf> = saved
        .iter()
        .filter_map(|folder| fs::canonicalize(folder).ok())
        .collect();
    let covered = |candidate: &Path| {
        saved
            .iter()
            .any(|folder| candidate.starts_with(folder) || folder.starts_with(candidate))
    };
    let mut by_path: HashMap<PathBuf, DeveloperWorkspaceSuggestion> = HashMap::new();
    for (cwd, sessions) in session_folders {
        let Some(candidate) = session_workspace(environment, &home, cwd) else {
            continue;
        };
        if covered(&candidate) {
            continue;
        }
        let entry = by_path.entry(candidate.clone()).or_insert_with(|| {
            let path = candidate.to_string_lossy().into_owned();
            DeveloperWorkspaceSuggestion {
                display_path: path.clone(),
                path,
                project_count: project_count(&candidate),
                agent_sessions: 0,
            }
        });
        entry.agent_sessions = entry.agent_sessions.saturating_add(*sessions);
    }
    for name in SUGGESTED_PROJECT_ROOTS {
        let candidate = home.join(name);
        let Ok(canonical) = validate_workspace_root(environment, &candidate) else {
            continue;
        };
        if covered(&canonical) || by_path.contains_key(&canonical) {
            continue;
        }
        let count = project_count(&canonical);
        if count == 0 {
            continue;
        }
        let path = canonical.to_string_lossy().into_owned();
        by_path.insert(
            canonical,
            DeveloperWorkspaceSuggestion {
                display_path: path.clone(),
                path,
                project_count: count,
                agent_sessions: 0,
            },
        );
    }
    let mut suggestions: Vec<DeveloperWorkspaceSuggestion> = by_path.into_values().collect();
    suggestions.sort_by(|left, right| {
        right
            .agent_sessions
            .cmp(&left.agent_sessions)
            .then_with(|| right.project_count.cmp(&left.project_count))
            .then_with(|| left.path.cmp(&right.path))
    });
    suggestions
}

/// Deepest folders a session's working directory is walked up through while
/// looking for its project.
const SESSION_PROJECT_DEPTH: usize = 8;

/// The folder that holds the project an agent worked in: the parent of the
/// nearest project root at or above `cwd`, or the project itself when that
/// parent is the home folder or not a valid workspace. `None` for a folder
/// that is gone, outside home, hidden, a linked worktree, or not a project.
pub(super) fn session_workspace(
    environment: &PlatformEnvironment,
    home: &Path,
    cwd: &Path,
) -> Option<PathBuf> {
    let cwd = fs::canonicalize(cwd).ok()?;
    if cwd == home || !cwd.starts_with(home) {
        return None;
    }
    let relative = cwd.strip_prefix(home).ok()?;
    if relative
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        // Tool folders (`~/.codex`, `~/.claude`) and other hidden places are
        // not where a person keeps projects.
        return None;
    }
    let mut project = None;
    for folder in cwd.ancestors().take(SESSION_PROJECT_DEPTH) {
        if folder == home || !folder.starts_with(home) {
            break;
        }
        if has_project_marker(folder) {
            project = Some(folder.to_path_buf());
            break;
        }
    }
    let project = project?;
    if fs::symlink_metadata(project.join(".git")).is_ok_and(|metadata| metadata.is_file()) {
        // A linked worktree: its repository and the worktree review cover it.
        return None;
    }
    if let Some(parent) = project.parent() {
        if parent != home {
            if let Ok(parent) = validate_workspace_root(environment, parent) {
                return Some(parent);
            }
        }
    }
    validate_workspace_root(environment, &project).ok()
}

/// Registers a folder the person accepted from the current suggestions.
///
/// The interface may only hand back a path neati itself suggested; any other
/// folder goes through the native picker.
pub fn register_suggested_workspace(
    environment: &PlatformEnvironment,
    path: &str,
    saved: &[String],
    session_folders: &[(PathBuf, u32)],
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> Result<DeveloperWorkspace, String> {
    if !suggest_workspaces(environment, saved, session_folders)
        .iter()
        .any(|suggestion| suggestion.path == path)
    {
        return Err(
            "That folder is no longer a suggestion. Use Add folder to choose it.".to_string(),
        );
    }
    Ok(register_workspace_path(environment, Path::new(path), workspaces_store)?.workspace)
}

/// Registers the saved project folders again at launch.
///
/// Each one is validated as if it were picked now. A folder that moved, became
/// a link, or sits on a volume that is not mounted is reported, not dropped:
/// the setting keeps it until the person removes it.
pub fn restore_workspaces(
    environment: &PlatformEnvironment,
    saved: &[String],
    workspaces_store: &Mutex<HashMap<String, DeveloperWorkspaceRecord>>,
) -> DeveloperWorkspaceRestore {
    let home = canonical_home(environment);
    let mut restore = DeveloperWorkspaceRestore {
        workspaces: Vec::new(),
        refused: Vec::new(),
    };
    for folder in saved.iter().take(MAX_SAVED_PROJECT_FOLDERS) {
        let path = Path::new(folder);
        let is_home = home
            .as_deref()
            .is_some_and(|home| fs::canonicalize(path).ok().as_deref() == Some(home));
        let registered = if is_home {
            register_home_workspace(environment, workspaces_store)
        } else {
            register_workspace_path(environment, path, workspaces_store)
                .map(|record| record.workspace)
        };
        match registered {
            Ok(workspace) => {
                if !restore
                    .workspaces
                    .iter()
                    .any(|existing| existing.id == workspace.id)
                {
                    restore.workspaces.push(workspace);
                }
            }
            Err(reason) => restore.refused.push(DeveloperWorkspaceRefusal {
                path: folder.clone(),
                reason,
            }),
        }
    }
    restore
}

/// The saved list must be absolute paths within the bound; each is validated
/// in full when it is used, because a folder may come and go between saves.
pub fn validate_saved_project_folders(folders: &[String]) -> Result<(), String> {
    if folders.len() > MAX_SAVED_PROJECT_FOLDERS {
        return Err(format!(
            "At most {MAX_SAVED_PROJECT_FOLDERS} project folders can be remembered."
        ));
    }
    for folder in folders {
        let path = Path::new(folder);
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(format!("{folder} is not an absolute folder path."));
        }
    }
    Ok(())
}

pub(super) fn store_workspace(
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
            path: canonical.to_string_lossy().into_owned(),
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
pub(super) fn workspace_root_scope_refusal(
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

pub(super) fn native_pick_workspace_path() -> Result<Option<PathBuf>, String> {
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
