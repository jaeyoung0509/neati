//! Evidence for the fixed user-level Chromium CRX cache, never installed versions.
use crate::safety::{SymlinkGuard, ToctouGuard};
use neati_platform::PlatformEnvironment;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_METADATA: u64 = 1024 * 1024;
const MAX_ARCHIVES: usize = 128;
const MAX_PAYLOAD: u64 = 2 * 1024 * 1024 * 1024;

/// The complete coupled store is one unit. Unknown files/index schemas revoke it.
pub(super) struct DownloadSnapshot(Vec<(PathBuf, crate::models::CleanupIdentity)>);
impl DownloadSnapshot {
    pub(super) fn verify(&self) -> Result<(), String> {
        for (path, identity) in &self.0 {
            ToctouGuard::verify(path, identity).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

pub(super) fn inspect(env: &PlatformEnvironment, root: &Path) -> Result<DownloadSnapshot, String> {
    let started = Instant::now();
    let expected = env
        .user_home()
        .ok_or("The user home is unavailable")?
        .join("Library/Application Support/Google/GoogleUpdater/crx_cache");
    if root != expected {
        return Err("Outside the user-level Google Updater download scope".into());
    }
    SymlinkGuard::validate_anchored_path(root, env).map_err(|e| e.to_string())?;
    let uid = env
        .current_user_id()
        .ok_or("The current user identity is unavailable")?;
    let identity = ToctouGuard::capture(root).ok_or("Cache identity is unavailable")?;
    ordinary_owned(root, uid, true)?;
    let index = root.join("metadata.json");
    let index_identity = ToctouGuard::capture(&index).ok_or("The download index is missing")?;
    let index_metadata = ordinary_owned(&index, uid, false)?;
    let size = index_metadata.len();
    if size == 0 || size > MAX_METADATA {
        return Err("Download index exceeds its supported size".into());
    }
    let mut index_bytes = Vec::new();
    let index_file = open_same(&index, &index_metadata)?;
    index_file
        .take(MAX_METADATA + 1)
        .read_to_end(&mut index_bytes)
        .map_err(|e| e.to_string())?;
    if index_bytes.len() as u64 != size {
        return Err("Download index changed while reading it".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&index_bytes).map_err(|_| "Download index is not supported JSON")?;
    let object = value.as_object().ok_or("Download index is not an object")?;
    if object.len() != 1 {
        return Err("Download index has unrecognized fields".into());
    }
    let hashes = object
        .get("hashes")
        .and_then(|v| v.as_object())
        .ok_or("Download index has no hash map")?;
    if hashes.len() > MAX_ARCHIVES {
        return Err("Download cache exceeds its archive limit".into());
    }
    for (hash, record) in hashes {
        let record = record.as_object().ok_or("Download record is unsupported")?;
        let app = record
            .get("appid")
            .and_then(|v| v.as_str())
            .ok_or("Download owner is missing")?;
        if !sha256_name(hash)
            || record.len() != 1
            || app.is_empty()
            || app.len() > 256
            || !app
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"{}._-".contains(&b))
        {
            return Err("Download record is outside the supported index schema".into());
        }
    }
    let mut found = BTreeSet::new();
    let mut total = 0u64;
    let mut identities = vec![
        (root.to_path_buf(), identity),
        (index.clone(), index_identity),
    ];
    for (number, entry) in fs::read_dir(root).map_err(|e| e.to_string())?.enumerate() {
        if number > MAX_ARCHIVES {
            return Err("Download cache exceeds its entry limit".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Unsupported download filename")?;
        if name == "metadata.json" {
            continue;
        }
        if !hashes.contains_key(&name) {
            return Err("Cache contains an unregistered file or directory".into());
        }
        let path = entry.path();
        let metadata = ordinary_owned(&path, uid, false)?;
        total = total
            .checked_add(metadata.len())
            .ok_or("Download size overflow")?;
        if total > MAX_PAYLOAD {
            return Err("Download verification exceeds its byte budget".into());
        }
        let file_identity =
            ToctouGuard::capture(&path).ok_or("Download identity is unavailable")?;
        let mut file = open_same(&path, &metadata)?;
        let mut header = [0u8; 12];
        file.read_exact(&mut header)
            .map_err(|_| "Incomplete CRX download")?;
        let header_size = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
        if &header[..4] != b"Cr24"
            || u32::from_le_bytes(header[4..8].try_into().unwrap()) != 3
            || header_size == 0
            || header_size > MAX_METADATA
            || metadata.len() <= 12 + header_size
        {
            return Err("Download is not a supported CRX3 archive".into());
        }
        let mut digest = Sha256::new();
        digest.update(header);
        let mut buffer = [0u8; 64 * 1024];
        let mut read_bytes = 12u64;
        loop {
            if started.elapsed() > Duration::from_secs(20) {
                return Err("Download verification exceeded its time limit".into());
            }
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            read_bytes += count as u64;
            if read_bytes > metadata.len() {
                return Err("Download grew during verification".into());
            }
            digest.update(&buffer[..count]);
        }
        if read_bytes != metadata.len() || crate::hash::hex(&digest.finalize()) != name {
            return Err("Download content does not match its indexed SHA-256".into());
        }
        ToctouGuard::verify(&path, &file_identity).map_err(|e| e.to_string())?;
        identities.push((path, file_identity));
        found.insert(name);
    }
    if found != hashes.keys().cloned().collect() {
        return Err("Download index and payloads do not match".into());
    }
    let snapshot = DownloadSnapshot(identities);
    snapshot.verify()?;
    Ok(snapshot)
}

fn open_same(path: &Path, metadata: &fs::Metadata) -> Result<fs::File, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err("Download changed while opening it".into());
    }
    Ok(file)
}

fn ordinary_owned(path: &Path, uid: u32, directory: bool) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
        || (!directory && (metadata.nlink() != 1 || metadata.mode() & 0o111 != 0))
    {
        return Err("Download cache contains a link, executable, shared or unowned entry".into());
    }
    Ok(metadata)
}

fn sha256_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
