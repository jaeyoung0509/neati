//! Session bundles: export and import (#509).
//!
//! A bundle is a zip archive (`.neati-sessions`) holding session files byte
//! for byte under `<tool>/<path relative to the tool's folder>`, and a
//! `manifest.json` with the schema, the neati version, and for each session
//! its tool, id, original project folder and every file's relative path, size
//! and SHA-256. No machine name and no account is recorded.
//!
//! Export never replaces an existing file and refuses to finish when likely
//! secrets were found unless the person allowed them. Import verifies the
//! manifest and every checksum before writing anything, refuses absolute
//! paths, `..`, links and unlisted entries, writes into the tool's own session
//! folder with the original layout, never through a link, and never over an
//! existing file: a collision is kept and reported.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::{AgentSessionTool, ProjectFolderState, SessionRemap};

use super::inventory::{valid_session_id, SessionFile};
use super::remap::{claude_project_folder, remap_path, rewrite_line};
use super::secrets::{SecretCounts, SecretScanner};
use super::{open_no_follow, SessionCancel, SessionRoots};

pub const SCHEMA: &str = "neati.sessions/1";
pub const EXTENSION: &str = "neati-sessions";
const MANIFEST: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_PATH_BYTES: usize = 1024;
const CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub neati_version: String,
    pub sessions: Vec<ManifestSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSession {
    pub tool: AgentSessionTool,
    pub session_id: String,
    pub original_cwd: Option<String>,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    /// Relative to the tool's session folder, `/`-separated.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// The archive entry that holds `file` of a `tool` session.
pub fn entry_name(tool: AgentSessionTool, path: &str) -> String {
    format!("{}/{path}", tool.slug())
}

/// Refuses anything but a plain relative path of the shape the tool uses.
pub fn validate_relative_path(
    tool: AgentSessionTool,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("The bundle names an unsafe path ({why})."));
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return refuse("empty or too long");
    }
    if path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return refuse("absolute or not a plain relative path");
    }
    if path.chars().any(char::is_control) {
        return refuse("control character");
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == ".." || part.len() > 255)
    {
        return refuse("a `..`, `.` or empty component");
    }
    let main = format!("{session_id}.jsonl");
    let shaped = match tool {
        AgentSessionTool::Claude => {
            parts.len() >= 2
                && parts.len() <= 6
                && !parts[0].starts_with('.')
                && (parts[1] == main || (parts.len() >= 3 && parts[1] == session_id))
        }
        AgentSessionTool::Codex => {
            matches!(parts[0], "sessions" | "archived_sessions")
                && parts.len() >= 2
                && parts.len() <= 5
                && path.ends_with(".jsonl")
        }
    };
    if shaped {
        Ok(())
    } else {
        refuse("not where the tool keeps a session")
    }
}

/// Checks a manifest's shape: schema, ids, paths, one main file per session,
/// and no path listed twice.
pub fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema != SCHEMA {
        return Err(format!(
            "This is not a {SCHEMA} bundle (schema {}).",
            manifest.schema
        ));
    }
    if manifest.sessions.is_empty() {
        return Err("The bundle holds no sessions.".to_string());
    }
    let mut seen = BTreeSet::new();
    let mut entries = 0usize;
    for session in &manifest.sessions {
        if !valid_session_id(&session.session_id) {
            return Err("The bundle names a session id neati does not accept.".to_string());
        }
        if session.files.is_empty() {
            return Err("A session in the bundle lists no files.".to_string());
        }
        if let Some(cwd) = &session.original_cwd {
            if cwd.is_empty() || cwd.chars().any(char::is_control) {
                return Err("The bundle names an unusable project folder.".to_string());
            }
        }
        let mut mains = 0;
        let mut folders = BTreeSet::new();
        for file in &session.files {
            validate_relative_path(session.tool, &session.session_id, &file.path)?;
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("The bundle lists a malformed checksum.".to_string());
            }
            if !seen.insert(entry_name(session.tool, &file.path)) {
                return Err("The bundle lists a file twice.".to_string());
            }
            let is_main = match session.tool {
                AgentSessionTool::Claude => {
                    file.path.split('/').nth(1) == Some(&format!("{}.jsonl", session.session_id))
                }
                AgentSessionTool::Codex => true,
            };
            mains += usize::from(is_main);
            folders.insert(file.path.split('/').next().unwrap_or_default().to_string());
        }
        let one_main = match session.tool {
            AgentSessionTool::Claude => mains == 1 && folders.len() == 1,
            AgentSessionTool::Codex => session.files.len() == 1,
        };
        if !one_main {
            return Err(
                "A session in the bundle does not have exactly one session file.".to_string(),
            );
        }
        entries += session.files.len();
    }
    if entries > MAX_ENTRIES {
        return Err("The bundle lists too many files.".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Export

/// A session chosen for export, as the backend resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSession {
    pub tool: AgentSessionTool,
    pub session_id: String,
    pub original_cwd: Option<String>,
    pub files: Vec<SessionFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportScan {
    pub sessions: u32,
    pub files: u32,
    pub bytes: u64,
    pub secrets: SecretCounts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub path: PathBuf,
    pub sessions: u32,
    pub files: u32,
    pub bytes: u64,
    pub archive_bytes: u64,
    pub secrets: SecretCounts,
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn cancelled() -> String {
    "The session operation was stopped.".to_string()
}

/// Streams the bytes of `file` as they are when it is opened, without
/// following a link.
fn stream_file(
    file: &SessionFile,
    cancel: &SessionCancel,
    mut sink: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<u64, String> {
    let handle = open_no_follow(&file.path)
        .map_err(|error| format!("A session file could not be read: {error}"))?;
    let length = handle
        .metadata()
        .map_err(|error| format!("A session file could not be read: {error}"))?
        .len();
    let mut reader = handle.take(length);
    let mut buffer = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("A session file could not be read: {error}"))?;
        if read == 0 {
            break;
        }
        sink(&buffer[..read])?;
        total += read as u64;
    }
    Ok(total)
}

fn check_export_shape(sessions: &[ExportSession]) -> Result<(), String> {
    if sessions.is_empty() {
        return Err("Choose at least one session to export.".to_string());
    }
    let mut seen = BTreeSet::new();
    for session in sessions {
        if !valid_session_id(&session.session_id) {
            return Err("A chosen session has an id neati cannot bundle.".to_string());
        }
        for file in &session.files {
            validate_relative_path(session.tool, &session.session_id, &file.relative)?;
            if !seen.insert(entry_name(session.tool, &file.relative)) {
                return Err("A file was chosen twice.".to_string());
            }
        }
    }
    Ok(())
}

/// Counts likely secrets in the chosen sessions before anything is written.
pub fn scan_for_export(
    sessions: &[ExportSession],
    cancel: &SessionCancel,
) -> Result<ExportScan, String> {
    check_export_shape(sessions)?;
    let mut secrets = SecretCounts::default();
    let mut bytes = 0u64;
    let mut files = 0usize;
    for session in sessions {
        for file in &session.files {
            let mut scanner = SecretScanner::new();
            bytes += stream_file(file, cancel, |chunk| {
                scanner.feed(chunk);
                Ok(())
            })?;
            secrets.merge(&scanner.finish());
            files += 1;
        }
    }
    Ok(ExportScan {
        sessions: count(sessions.len()),
        files: count(files),
        bytes,
        secrets,
    })
}

/// Writes a bundle to `destination`, which must not exist. Secrets found while
/// writing refuse the bundle unless `allow_secrets`.
pub fn write_bundle(
    sessions: &[ExportSession],
    destination: &Path,
    allow_secrets: bool,
    neati_version: &str,
    cancel: &SessionCancel,
) -> Result<ExportReport, String> {
    check_export_shape(sessions)?;
    match std::fs::symlink_metadata(destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(format!(
                "{} already exists. neati never replaces a file; choose another name.",
                destination.display()
            ))
        }
        Err(error) => return Err(format!("The destination could not be checked: {error}")),
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !std::fs::metadata(parent).is_ok_and(|metadata| metadata.is_dir()) {
        return Err("The destination folder does not exist.".to_string());
    }
    let temporary = tempfile::Builder::new()
        .prefix(".neati-sessions-")
        .suffix(".partial")
        .tempfile_in(parent)
        .map_err(|error| format!("The bundle could not be created: {error}"))?;
    let write_error =
        |error: &dyn std::fmt::Display| format!("The bundle could not be written: {error}");
    let mut zip = zip::ZipWriter::new(temporary.as_file());
    let mut manifest = Manifest {
        schema: SCHEMA.to_string(),
        neati_version: neati_version.to_string(),
        sessions: Vec::new(),
    };
    let mut secrets = SecretCounts::default();
    let mut bytes = 0u64;
    let mut files = 0usize;
    for session in sessions {
        let mut listed = Vec::new();
        for file in &session.files {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o600)
                .large_file(file.size >= u64::from(u32::MAX));
            zip.start_file(entry_name(session.tool, &file.relative), options)
                .map_err(|error| write_error(&error))?;
            let mut hasher = Sha256::new();
            let mut scanner = SecretScanner::new();
            let size = stream_file(file, cancel, |chunk| {
                hasher.update(chunk);
                scanner.feed(chunk);
                zip.write_all(chunk).map_err(|error| write_error(&error))
            })?;
            secrets.merge(&scanner.finish());
            bytes += size;
            files += 1;
            listed.push(ManifestFile {
                path: file.relative.clone(),
                size,
                sha256: crate::hash::hex(&hasher.finalize()),
            });
        }
        manifest.sessions.push(ManifestSession {
            tool: session.tool,
            session_id: session.session_id.clone(),
            original_cwd: session.original_cwd.clone(),
            files: listed,
        });
    }
    if !secrets.is_empty() && !allow_secrets {
        // The partial archive is neati's own temporary file and goes with it.
        return Err(
            "Likely secrets were found in these sessions. Review the counts and allow them to export."
                .to_string(),
        );
    }
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|error| write_error(&error))?;
    zip.start_file(
        MANIFEST,
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600),
    )
    .map_err(|error| write_error(&error))?;
    zip.write_all(&manifest_bytes)
        .map_err(|error| write_error(&error))?;
    zip.finish().map_err(|error| write_error(&error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| write_error(&error))?;
    temporary.persist_noclobber(destination).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "{} already exists. neati never replaces a file; choose another name.",
                destination.display()
            )
        } else {
            write_error(&error.error)
        }
    })?;
    let archive_bytes = std::fs::metadata(destination)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(ExportReport {
        path: destination.to_path_buf(),
        sessions: count(sessions.len()),
        files: count(files),
        bytes,
        archive_bytes,
        secrets,
    })
}

// ---------------------------------------------------------------------------
// Import

/// A bundle whose manifest and every checksum were verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBundle {
    pub path: PathBuf,
    pub manifest: Manifest,
    length: u64,
    modified: Option<std::time::SystemTime>,
}

impl VerifiedBundle {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn not_a_bundle() -> String {
    "This file is not a neati session bundle.".to_string()
}

fn open_archive(
    path: &Path,
) -> Result<
    (
        zip::ZipArchive<std::fs::File>,
        u64,
        Option<std::time::SystemTime>,
    ),
    String,
> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("The bundle could not be opened: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("The bundle could not be opened: {error}"))?;
    if !metadata.is_file() {
        return Err(not_a_bundle());
    }
    let archive = zip::ZipArchive::new(file).map_err(|_| not_a_bundle())?;
    Ok((archive, metadata.len(), metadata.modified().ok()))
}

/// Hashes what passes through.
struct Hashing<R> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        self.bytes += read as u64;
        Ok(read)
    }
}

fn open_entry<'a>(
    archive: &'a mut zip::ZipArchive<std::fs::File>,
    name: &str,
    size: u64,
) -> Result<Hashing<std::io::Take<zip::read::ZipFile<'a, std::fs::File>>>, String> {
    let entry = archive
        .by_name(name)
        .map_err(|_| "The bundle is missing a file its manifest lists.".to_string())?;
    if entry.is_dir() || entry.is_symlink() || !entry.is_file() {
        return Err("The bundle holds a link or a folder where a file belongs.".to_string());
    }
    if entry.encrypted() {
        return Err("The bundle holds an encrypted entry.".to_string());
    }
    if entry.size() != size {
        return Err("A file in the bundle does not match its manifest size.".to_string());
    }
    Ok(Hashing {
        inner: entry.take(size.saturating_add(1)),
        hasher: Sha256::new(),
        bytes: 0,
    })
}

fn verify_end<R>(hashing: Hashing<R>, file: &ManifestFile) -> Result<(), String> {
    if hashing.bytes != file.size || crate::hash::hex(&hashing.hasher.finalize()) != file.sha256 {
        return Err(
            "A file in the bundle does not match its checksum. Nothing more was imported."
                .to_string(),
        );
    }
    Ok(())
}

/// Reads `path`, checks its manifest and the checksum of every file.
pub fn read_bundle(path: &Path, cancel: &SessionCancel) -> Result<VerifiedBundle, String> {
    let (mut archive, length, modified) = open_archive(path)?;
    if archive.len() > MAX_ENTRIES + 1 {
        return Err("The bundle holds too many entries.".to_string());
    }
    let manifest = {
        let entry = archive.by_name(MANIFEST).map_err(|_| not_a_bundle())?;
        if entry.size() > MAX_MANIFEST_BYTES || !entry.is_file() || entry.encrypted() {
            return Err(not_a_bundle());
        }
        let mut text = Vec::new();
        entry
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut text)
            .map_err(|_| not_a_bundle())?;
        serde_json::from_slice::<Manifest>(&text)
            .map_err(|_| "The bundle's manifest could not be read.".to_string())?
    };
    validate_manifest(&manifest)?;
    let expected: BTreeSet<String> = manifest
        .sessions
        .iter()
        .flat_map(|session| {
            session
                .files
                .iter()
                .map(|file| entry_name(session.tool, &file.path))
        })
        .collect();
    let names: Vec<String> = archive
        .file_names()
        .map(|name| name.map(|name| name.into_owned()))
        .collect::<Result<_, _>>()
        .map_err(|_| not_a_bundle())?;
    let unique: BTreeSet<&String> = names.iter().collect();
    if unique.len() != names.len() || names.len() != archive.len() {
        return Err("The bundle lists an entry twice.".to_string());
    }
    for name in &names {
        if name != MANIFEST && !expected.contains(name) {
            return Err("The bundle holds a file its manifest does not list.".to_string());
        }
    }
    for session in &manifest.sessions {
        for file in &session.files {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            let mut hashing = open_entry(
                &mut archive,
                &entry_name(session.tool, &file.path),
                file.size,
            )?;
            std::io::copy(&mut hashing, &mut std::io::sink())
                .map_err(|_| "A file in the bundle could not be read.".to_string())?;
            verify_end(hashing, file)?;
        }
    }
    Ok(VerifiedBundle {
        path: path.to_path_buf(),
        manifest,
        length,
        modified,
    })
}

/// Where one bundled file goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub file: ManifestFile,
    pub entry: String,
    pub destination: PathBuf,
    /// `tool/relative` as written, for reports.
    pub display: String,
    pub exists: bool,
    pub rewrite: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedSession {
    pub tool: AgentSessionTool,
    pub session_id: String,
    pub original_cwd: Option<String>,
    pub files: Vec<PlannedFile>,
}

fn trimmed(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        path
    } else {
        trimmed
    }
}

/// Checks remaps against the bundle and this machine.
fn check_remaps(
    manifest: &Manifest,
    remaps: &[SessionRemap],
    folder_state: &dyn Fn(&str) -> ProjectFolderState,
) -> Result<(), String> {
    let recorded: BTreeSet<&str> = manifest
        .sessions
        .iter()
        .filter_map(|session| session.original_cwd.as_deref())
        .map(trimmed)
        .collect();
    let mut seen = BTreeSet::new();
    for remap in remaps {
        let from = trimmed(&remap.from);
        let to = trimmed(&remap.to);
        if !recorded.contains(from) {
            return Err("A remap names a folder no session in the bundle recorded.".to_string());
        }
        if !seen.insert(from) {
            return Err("A folder is remapped twice.".to_string());
        }
        if folder_state(from) != ProjectFolderState::Missing {
            return Err(format!(
                "{from} exists on this Mac or cannot be checked; only a missing folder is remapped."
            ));
        }
        if !Path::new(to).is_absolute() || to.chars().any(char::is_control) || from == to {
            return Err("A remap's new folder must be a different absolute path.".to_string());
        }
        if folder_state(to) != ProjectFolderState::Present {
            return Err(format!("{to} is not a folder on this Mac."));
        }
    }
    Ok(())
}

/// Decides where every file goes, which ones exist already, and which are
/// rewritten by a remap. Writes nothing.
pub fn plan_import(
    bundle: &VerifiedBundle,
    roots: &SessionRoots,
    remaps: &[SessionRemap],
    folder_state: &dyn Fn(&str) -> ProjectFolderState,
) -> Result<Vec<PlannedSession>, String> {
    check_remaps(&bundle.manifest, remaps, folder_state)?;
    let mut planned = Vec::new();
    for session in &bundle.manifest.sessions {
        let remap = session.original_cwd.as_deref().and_then(|cwd| {
            remaps.iter().find_map(|remap| {
                remap_path(cwd, &remap.from, &remap.to).map(|moved| {
                    (
                        trimmed(&remap.from).to_string(),
                        trimmed(&remap.to).to_string(),
                        moved,
                    )
                })
            })
        });
        let claude_folder = match (&remap, session.tool) {
            (Some((_, _, moved)), AgentSessionTool::Claude) => {
                Some(claude_project_folder(moved).ok_or_else(|| {
                    "The new folder's path is too long for Claude Code's session folders."
                        .to_string()
                })?)
            }
            _ => None,
        };
        let root = roots.tool_root(session.tool);
        let mut files = Vec::new();
        for file in &session.files {
            let mut parts: Vec<String> = file.path.split('/').map(str::to_string).collect();
            if let Some(folder) = &claude_folder {
                parts[0] = folder.clone();
            }
            let relative = parts.join("/");
            let destination = parts
                .iter()
                .fold(root.to_path_buf(), |path, part| path.join(part));
            let exists = std::fs::symlink_metadata(&destination).is_ok();
            let rewrite = remap
                .as_ref()
                .filter(|_| file.path.ends_with(".jsonl"))
                .map(|(from, to, _)| (from.clone(), to.clone()));
            files.push(PlannedFile {
                file: file.clone(),
                entry: entry_name(session.tool, &file.path),
                destination,
                display: entry_name(session.tool, &relative),
                exists,
                rewrite,
            });
        }
        planned.push(PlannedSession {
            tool: session.tool,
            session_id: session.session_id.clone(),
            original_cwd: session.original_cwd.clone(),
            files,
        });
    }
    Ok(planned)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub sessions: u32,
    pub written: u32,
    pub kept_existing: Vec<String>,
    pub rewritten: u32,
}

/// Creates `root/parts…` as real folders, refusing any link on the way.
fn ensure_folders(root: &Path, parts: &[&str]) -> Result<PathBuf, String> {
    let refuse = |path: &Path| {
        format!(
            "{} is a link or not a folder; neati does not write through links.",
            path.display()
        )
    };
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(refuse(root)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(root)
                .map_err(|error| format!("{} could not be created: {error}", root.display()))?;
            if !std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_dir())
            {
                return Err(refuse(root));
            }
        }
        Err(error) => return Err(format!("{} could not be checked: {error}", root.display())),
    }
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(refuse(&path)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                match builder.create(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        if !std::fs::symlink_metadata(&path)
                            .is_ok_and(|metadata| metadata.file_type().is_dir())
                        {
                            return Err(refuse(&path));
                        }
                    }
                    Err(error) => {
                        return Err(format!("{} could not be created: {error}", path.display()))
                    }
                }
            }
            Err(error) => return Err(format!("{} could not be checked: {error}", path.display())),
        }
    }
    Ok(path)
}

/// Copies one entry into a new temporary file, rewriting `cwd` fields when
/// asked, and verifies the source bytes against the manifest.
fn copy_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    planned: &PlannedFile,
    output: &mut std::fs::File,
) -> Result<bool, String> {
    let hashing = open_entry(archive, &planned.entry, planned.file.size)?;
    let write_error =
        |error: std::io::Error| format!("A session file could not be written: {error}");
    let mut changed = false;
    let hashing = match &planned.rewrite {
        None => {
            let mut hashing = hashing;
            std::io::copy(&mut hashing, output).map_err(write_error)?;
            hashing
        }
        Some((from, to)) => {
            let mut reader = BufReader::with_capacity(CHUNK, hashing);
            let mut line = Vec::new();
            loop {
                line.clear();
                let read = reader
                    .read_until(b'\n', &mut line)
                    .map_err(|_| "A file in the bundle could not be read.".to_string())?;
                if read == 0 {
                    break;
                }
                let ending = if line.ends_with(b"\r\n") {
                    2
                } else {
                    usize::from(line.ends_with(b"\n"))
                };
                let (content, terminator) = line.split_at(line.len() - ending);
                match rewrite_line(content, from, to) {
                    Some(rewritten) => {
                        changed = true;
                        output.write_all(&rewritten).map_err(write_error)?;
                        output.write_all(terminator).map_err(write_error)?;
                    }
                    None => output.write_all(&line).map_err(write_error)?,
                }
            }
            reader.into_inner()
        }
    };
    verify_end(hashing, &planned.file)?;
    Ok(changed)
}

/// Imports a verified bundle. Existing files are kept and reported; nothing is
/// replaced or removed.
pub fn import_bundle(
    bundle: &VerifiedBundle,
    roots: &SessionRoots,
    remaps: &[SessionRemap],
    folder_state: &dyn Fn(&str) -> ProjectFolderState,
    cancel: &SessionCancel,
) -> Result<ImportReport, String> {
    let plan = plan_import(bundle, roots, remaps, folder_state)?;
    let (mut archive, length, modified) = open_archive(&bundle.path)?;
    if length != bundle.length || modified != bundle.modified {
        return Err("The bundle changed since it was checked. Open it again.".to_string());
    }
    let mut report = ImportReport::default();
    let mut touched_sessions = BTreeSet::new();
    for session in &plan {
        for planned in &session.files {
            if cancel.is_cancelled() {
                return Err(format!(
                    "{} {} files were written before it stopped.",
                    cancelled(),
                    report.written
                ));
            }
            if planned.exists {
                report.kept_existing.push(planned.display.clone());
                continue;
            }
            let root = roots.tool_root(session.tool);
            let relative = planned
                .destination
                .strip_prefix(root)
                .map_err(|_| "A destination left the session folder.".to_string())?;
            let parts: Vec<&str> = relative
                .parent()
                .map(|parent| {
                    parent
                        .components()
                        .filter_map(|component| match component {
                            std::path::Component::Normal(part) => part.to_str(),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let folder = ensure_folders(root, &parts)?;
            let mut temporary = tempfile::Builder::new()
                .prefix(".neati-import-")
                .suffix(".partial")
                .tempfile_in(&folder)
                .map_err(|error| format!("A session file could not be written: {error}"))?;
            let changed =
                copy_entry(&mut archive, planned, temporary.as_file_mut()).map_err(|error| {
                    if report.written > 0 {
                        format!("{error} {} files were written before it.", report.written)
                    } else {
                        error
                    }
                })?;
            temporary
                .as_file()
                .sync_all()
                .map_err(|error| format!("A session file could not be written: {error}"))?;
            match temporary.persist_noclobber(&planned.destination) {
                Ok(_) => {
                    report.written += 1;
                    report.rewritten += u32::from(changed);
                    touched_sessions.insert((session.tool, session.session_id.clone()));
                }
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    report.kept_existing.push(planned.display.clone());
                }
                Err(error) => {
                    return Err(format!(
                        "A session file could not be written: {}",
                        error.error
                    ))
                }
            }
        }
    }
    report.sessions = count(touched_sessions.len());
    Ok(report)
}

/// The recorded project folders and whether each exists here.
pub fn bundle_projects(
    manifest: &Manifest,
    folder_state: &dyn Fn(&str) -> ProjectFolderState,
) -> Vec<(String, ProjectFolderState, u32)> {
    let mut projects: BTreeMap<String, u32> = BTreeMap::new();
    for session in &manifest.sessions {
        if let Some(cwd) = &session.original_cwd {
            *projects.entry(trimmed(cwd).to_string()).or_default() += 1;
        }
    }
    projects
        .into_iter()
        .map(|(cwd, sessions)| {
            let state = folder_state(&cwd);
            (cwd, state, sessions)
        })
        .collect()
}
