//! The read-only session inventory (#508).
//!
//! Discovery lists session files under each tool's folder without following
//! links, caps how many it reads, and reads a bounded head and tail of each:
//! enough for the session id, the project folder, the dates and a short
//! title. A whole transcript is never parsed. Nothing is written or removed.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::models::{
    AgentSessionInventory, AgentSessionIssue, AgentSessionProject, AgentSessionSummary,
    AgentSessionTool, AgentSessionToolTotal, ProjectFolderState,
};

use super::records::{last_timestamp, read_head, HeadFacts};
use super::{open_no_follow, relative_slash_path, SessionCancel, SessionRoots};

/// How much the inventory may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryLimits {
    /// Session files read per tool; the most recently changed are kept.
    pub max_sessions_per_tool: usize,
    /// Files listed in one Claude session's side folder.
    pub max_companion_files: usize,
    /// First read of each file.
    pub head_bytes: u64,
    /// The head read grows to this when no prompt was found yet.
    pub extended_head_bytes: u64,
    pub tail_bytes: u64,
    /// Head and tail bytes read in one inventory, over every file.
    pub read_budget: u64,
}

impl Default for InventoryLimits {
    fn default() -> Self {
        Self {
            max_sessions_per_tool: 5_000,
            max_companion_files: 2_000,
            head_bytes: 64 * 1024,
            extended_head_bytes: 1024 * 1024,
            tail_bytes: 64 * 1024,
            read_budget: 512 * 1024 * 1024,
        }
    }
}

/// A file that belongs to a session, with its path relative to the tool root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    pub path: PathBuf,
    pub relative: String,
    pub size: u64,
}

/// One session as the backend holds it. Paths never cross IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSession {
    pub tool: AgentSessionTool,
    pub key: String,
    pub session_id: String,
    /// The `.jsonl` file first; Claude's side-folder files follow.
    pub files: Vec<SessionFile>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub started_at: Option<u64>,
    pub last_activity_at: Option<u64>,
    pub dates_from_records: bool,
    pub archived: bool,
    pub issue: Option<AgentSessionIssue>,
    /// Claude's session folder name, the grouping fallback without a `cwd`.
    pub claude_folder: Option<String>,
}

impl DiscoveredSession {
    pub fn size_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    pub sessions: Vec<DiscoveredSession>,
    /// Which tools' folders exist.
    pub present: Vec<AgentSessionTool>,
    pub truncated: bool,
    pub cancelled: bool,
    pub reasons: Vec<String>,
}

/// The opaque reference for a session file.
pub fn session_key(tool: AgentSessionTool, relative: &str) -> String {
    crate::hash::sha256_hex(format!("{}\0{relative}", tool.slug()).as_bytes())[..16].to_string()
}

struct Budget {
    remaining: u64,
    exhausted: usize,
}

/// Lists every session the roots hold, within `limits`.
pub fn discover(
    roots: &SessionRoots,
    limits: &InventoryLimits,
    cancel: &SessionCancel,
) -> Discovery {
    let mut discovery = Discovery::default();
    let mut budget = Budget {
        remaining: limits.read_budget,
        exhausted: 0,
    };
    for tool in AgentSessionTool::ALL {
        if cancel.is_cancelled() {
            discovery.cancelled = true;
            break;
        }
        let candidates = match tool {
            AgentSessionTool::Claude => claude_candidates(roots, &mut discovery),
            AgentSessionTool::Codex => codex_candidates(roots, &mut discovery),
        };
        let mut candidates = candidates;
        if candidates.len() > limits.max_sessions_per_tool {
            // Keep the most recently changed.
            candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.modified));
            let dropped = candidates.len() - limits.max_sessions_per_tool;
            candidates.truncate(limits.max_sessions_per_tool);
            discovery.truncated = true;
            discovery.reasons.push(format!(
                "{}: {dropped} older sessions were not read (limit {}).",
                tool.label(),
                limits.max_sessions_per_tool
            ));
        }
        candidates.sort_by(|left, right| left.relative.cmp(&right.relative));
        for candidate in candidates {
            if cancel.is_cancelled() {
                discovery.cancelled = true;
                break;
            }
            let session = read_session(roots, tool, candidate, limits, &mut budget, &mut discovery);
            discovery.sessions.push(session);
        }
    }
    if budget.exhausted > 0 {
        discovery.truncated = true;
        discovery.reasons.push(format!(
            "{} sessions show file dates only: the read limit was reached.",
            budget.exhausted
        ));
    }
    if discovery.cancelled {
        discovery
            .reasons
            .push("The inventory was stopped.".to_string());
    }
    discovery
}

struct Candidate {
    main: PathBuf,
    relative: String,
    size: u64,
    modified: Option<u64>,
    created: Option<u64>,
    archived: bool,
    claude_folder: Option<String>,
}

fn seconds(time: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    time.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

/// A regular, unlinked `.jsonl` file at `path`, measured without following it.
fn jsonl_candidate(root: &Path, path: &Path) -> Option<Candidate> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
        return None;
    }
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    Some(Candidate {
        main: path.to_path_buf(),
        relative: relative_slash_path(root, path)?,
        size: metadata.len(),
        modified: seconds(metadata.modified()),
        created: seconds(metadata.created()),
        archived: false,
        claude_folder: None,
    })
}

fn real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn sorted_entries(directory: &Path) -> Vec<std::fs::DirEntry> {
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .map(|entries| entries.flatten().collect())
        .unwrap_or_default();
    entries.sort_by_key(|entry| entry.file_name());
    entries
}

fn claude_candidates(roots: &SessionRoots, discovery: &mut Discovery) -> Vec<Candidate> {
    let root = &roots.claude_projects;
    if !real_directory(root) {
        return Vec::new();
    }
    discovery.present.push(AgentSessionTool::Claude);
    let mut candidates = Vec::new();
    for project in sorted_entries(root) {
        // `DirEntry::file_type` does not follow links.
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let folder = project.file_name().to_string_lossy().into_owned();
        for entry in sorted_entries(&project.path()) {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            if let Some(mut candidate) = jsonl_candidate(root, &entry.path()) {
                candidate.claude_folder = Some(folder.clone());
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn codex_candidates(roots: &SessionRoots, discovery: &mut Discovery) -> Vec<Candidate> {
    let root = &roots.codex_home;
    let mut candidates = Vec::new();
    let mut any = false;
    for (folder, archived) in [("sessions", false), ("archived_sessions", true)] {
        let directory = root.join(folder);
        if !real_directory(&directory) {
            continue;
        }
        any = true;
        for entry in walkdir::WalkDir::new(&directory)
            .follow_links(false)
            .min_depth(1)
            .max_depth(4)
            .sort_by_file_name()
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(mut candidate) = jsonl_candidate(root, entry.path()) {
                candidate.archived = archived;
                candidates.push(candidate);
            }
        }
    }
    if any {
        discovery.present.push(AgentSessionTool::Codex);
    }
    candidates
}

/// Claude's side folder `<folder>/<session>/` (subagents, tool results).
fn claude_companions(
    root: &Path,
    main: &Path,
    limits: &InventoryLimits,
    discovery: &mut Discovery,
) -> Vec<SessionFile> {
    let Some(stem) = main.file_stem() else {
        return Vec::new();
    };
    let side = main.with_file_name(stem);
    if !real_directory(&side) {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&side)
        .follow_links(false)
        .min_depth(1)
        .max_depth(4)
        .sort_by_file_name()
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if files.len() >= limits.max_companion_files {
            discovery.truncated = true;
            discovery.reasons.push(format!(
                "A Claude Code session has more than {} side files; the rest were not counted.",
                limits.max_companion_files
            ));
            break;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if let Some(relative) = relative_slash_path(root, entry.path()) {
            files.push(SessionFile {
                path: entry.path().to_path_buf(),
                relative,
                size: metadata.len(),
            });
        }
    }
    files
}

/// The session id a file name states: Claude's `<id>.jsonl`, Codex's
/// `rollout-<time>-<id>.jsonl`.
fn id_from_name(tool: AgentSessionTool, main: &Path) -> String {
    let stem = main
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    match tool {
        AgentSessionTool::Claude => stem,
        AgentSessionTool::Codex => {
            if stem.len() >= 36 && stem.is_char_boundary(stem.len() - 36) {
                let tail = &stem[stem.len() - 36..];
                if tail.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
                    return tail.to_string();
                }
            }
            stem
        }
    }
}

/// A session id may only be a plain token.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn read_session(
    roots: &SessionRoots,
    tool: AgentSessionTool,
    candidate: Candidate,
    limits: &InventoryLimits,
    budget: &mut Budget,
    discovery: &mut Discovery,
) -> DiscoveredSession {
    let root = roots.tool_root(tool);
    let mut files = vec![SessionFile {
        path: candidate.main.clone(),
        relative: candidate.relative.clone(),
        size: candidate.size,
    }];
    if tool == AgentSessionTool::Claude {
        files.extend(claude_companions(root, &candidate.main, limits, discovery));
    }
    let (facts, tail, issue) = if budget.remaining == 0 {
        budget.exhausted += 1;
        (HeadFacts::default(), None, Some(AgentSessionIssue::NotRead))
    } else {
        match read_windows(tool, &candidate.main, limits, budget) {
            Ok((facts, tail)) => {
                let issue = (facts.records == 0).then_some(AgentSessionIssue::NoRecords);
                (facts, tail, issue)
            }
            Err(_) => (
                HeadFacts::default(),
                None,
                Some(AgentSessionIssue::Unreadable),
            ),
        }
    };
    let name_id = id_from_name(tool, &candidate.main);
    // The file name is what a bundle and the tool itself address; a record
    // naming something else (a forked session) does not replace it.
    let session_id = if valid_session_id(&name_id) {
        name_id
    } else {
        facts
            .session_id
            .clone()
            .filter(|id| valid_session_id(id))
            .unwrap_or(name_id)
    };
    let dates_from_records = facts.started_at.is_some() || tail.is_some();
    let started_at = facts
        .started_at
        .or(candidate.created)
        .or(candidate.modified);
    let last_activity_at = tail.or(candidate.modified).or(started_at);
    DiscoveredSession {
        tool,
        key: session_key(tool, &candidate.relative),
        session_id,
        files,
        cwd: facts
            .cwd
            .filter(|cwd| cwd.starts_with('/') || cwd.contains(":\\")),
        title: facts.title,
        started_at,
        last_activity_at,
        dates_from_records,
        archived: candidate.archived,
        issue,
        claude_folder: candidate.claude_folder,
    }
}

/// Head facts and the last record timestamp.
fn read_windows(
    tool: AgentSessionTool,
    path: &Path,
    limits: &InventoryLimits,
    budget: &mut Budget,
) -> std::io::Result<(HeadFacts, Option<u64>)> {
    let mut file = open_no_follow(path)?;
    let length = file.metadata()?.len();
    let first = limits.head_bytes.min(budget.remaining).min(length);
    let mut head = Vec::with_capacity(first as usize);
    (&mut file).take(first).read_to_end(&mut head)?;
    let mut facts = read_head(tool, &head, head.len() as u64 >= length);
    let extended = limits.extended_head_bytes.min(length);
    if facts.title.is_none() && (head.len() as u64) < extended {
        let more = (extended - head.len() as u64).min(budget.remaining.saturating_sub(first));
        (&mut file).take(more).read_to_end(&mut head)?;
        facts = read_head(tool, &head, head.len() as u64 >= length);
    }
    budget.remaining = budget.remaining.saturating_sub(head.len() as u64);
    let tail = if head.len() as u64 >= length {
        last_timestamp(&head, false)
    } else {
        let size = limits.tail_bytes.min(budget.remaining).min(length);
        let start = length - size;
        file.seek(SeekFrom::Start(start))?;
        let mut window = Vec::with_capacity(size as usize);
        (&mut file).take(size).read_to_end(&mut window)?;
        budget.remaining = budget.remaining.saturating_sub(window.len() as u64);
        last_timestamp(&window, start > 0)
    };
    Ok((facts, tail))
}

/// The display name for a project folder: its last component.
fn display_name(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

/// The inventory the window and the CLI show: sessions grouped by project,
/// newest first, with totals per tool.
pub fn project_inventory(
    discovery: &Discovery,
    observed_at: u64,
    folder_state: &dyn Fn(&str) -> ProjectFolderState,
) -> AgentSessionInventory {
    let mut groups: BTreeMap<String, AgentSessionProject> = BTreeMap::new();
    let mut states: BTreeMap<String, ProjectFolderState> = BTreeMap::new();
    for session in &discovery.sessions {
        let (group, cwd, name) = match (&session.cwd, &session.claude_folder) {
            (Some(cwd), _) => (format!("cwd:{cwd}"), Some(cwd.clone()), display_name(cwd)),
            (None, Some(folder)) => (format!("claude:{folder}"), None, folder.clone()),
            (None, None) => ("unknown".to_string(), None, "Unknown project".to_string()),
        };
        let folder = match &cwd {
            Some(cwd) => *states
                .entry(cwd.clone())
                .or_insert_with(|| folder_state(cwd)),
            None => ProjectFolderState::Unknown,
        };
        let project = groups
            .entry(group.clone())
            .or_insert_with(|| AgentSessionProject {
                key: crate::hash::sha256_hex(group.as_bytes())[..16].to_string(),
                cwd,
                display_name: name,
                folder,
                total_bytes: 0,
                last_activity_at: None,
                sessions: Vec::new(),
            });
        project.total_bytes = project.total_bytes.saturating_add(session.size_bytes());
        project.last_activity_at = project.last_activity_at.max(session.last_activity_at);
        project.sessions.push(AgentSessionSummary {
            key: session.key.clone(),
            tool: session.tool,
            session_id: session.session_id.clone(),
            cwd: session.cwd.clone(),
            title: session.title.clone(),
            started_at: session.started_at,
            last_activity_at: session.last_activity_at,
            dates_from_records: session.dates_from_records,
            size_bytes: session.size_bytes(),
            file_count: u32::try_from(session.files.len()).unwrap_or(u32::MAX),
            archived: session.archived,
            issue: session.issue,
        });
    }
    let mut projects: Vec<_> = groups.into_values().collect();
    for project in &mut projects {
        project.sessions.sort_by(|left, right| {
            right
                .last_activity_at
                .cmp(&left.last_activity_at)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
    }
    projects.sort_by(|left, right| {
        right
            .last_activity_at
            .cmp(&left.last_activity_at)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    let tools = AgentSessionTool::ALL
        .into_iter()
        .map(|tool| {
            let sessions: Vec<_> = discovery
                .sessions
                .iter()
                .filter(|session| session.tool == tool)
                .collect();
            AgentSessionToolTotal {
                tool,
                present: discovery.present.contains(&tool),
                sessions: u32::try_from(sessions.len()).unwrap_or(u32::MAX),
                files: u32::try_from(
                    sessions
                        .iter()
                        .map(|session| session.files.len())
                        .sum::<usize>(),
                )
                .unwrap_or(u32::MAX),
                bytes: sessions
                    .iter()
                    .map(|session| session.size_bytes())
                    .fold(0u64, u64::saturating_add),
            }
        })
        .collect();
    let mut reasons = discovery.reasons.clone();
    for tool in AgentSessionTool::ALL {
        let unreadable = discovery
            .sessions
            .iter()
            .filter(|session| {
                session.tool == tool
                    && matches!(
                        session.issue,
                        Some(AgentSessionIssue::Unreadable | AgentSessionIssue::NoRecords)
                    )
            })
            .count();
        if unreadable > 0 {
            reasons.push(format!(
                "{}: {unreadable} {} could not be read.",
                tool.label(),
                if unreadable == 1 {
                    "session"
                } else {
                    "sessions"
                }
            ));
        }
    }
    AgentSessionInventory {
        observed_at,
        projects,
        tools,
        truncated: discovery.truncated,
        cancelled: discovery.cancelled,
        incomplete_reasons: reasons,
    }
}

/// Whether a recorded folder exists on this machine. A network location is
/// never checked.
pub fn native_folder_state(cwd: &str) -> ProjectFolderState {
    if !super::folder_may_be_checked(cwd) {
        return ProjectFolderState::Unknown;
    }
    if Path::new(cwd).is_dir() {
        ProjectFolderState::Present
    } else {
        ProjectFolderState::Missing
    }
}
