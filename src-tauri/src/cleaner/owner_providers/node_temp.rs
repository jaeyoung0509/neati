//! Exact default Node compile-cache namespace. Never enumerates a temp root.
use crate::safety::SymlinkGuard;
use neati_core::domain::cleanup::temporary_cache::*;
use neati_platform::PlatformEnvironment;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub(super) fn discover(
    env: &PlatformEnvironment,
) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    let uid = env
        .current_user_id()
        .ok_or("Current-user identity is unavailable")?;
    let mut units = Vec::new();
    for temp in env.temporary_roots() {
        let root = temp.join("node-compile-cache");
        match fs::symlink_metadata(&root) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
            Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
                units.push((
                    root,
                    Some("The Node cache root is not an ordinary directory".into()),
                ));
                continue;
            }
            Ok(_) => {}
        }
        SymlinkGuard::validate_anchored_path(&root, env).map_err(|e| e.to_string())?;
        let started = std::time::Instant::now();
        for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
            if units.len() >= 512 || started.elapsed() > Duration::from_secs(5) {
                return Err("Node cache discovery exceeded its bounded budget".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let supported = entry
                .file_name()
                .to_str()
                .is_some_and(|name| supported_node_group(name, uid));
            units.push((
                entry.path(),
                (!supported).then(|| {
                    "Unverified Node cache version, user or layout; kept for inspection only".into()
                }),
            ));
        }
    }
    units.sort_by(|a, b| a.0.cmp(&b.0));
    units.dedup_by(|a, b| a.0 == b.0);
    Ok(units)
}

#[cfg(unix)]
pub(super) fn inspect(env: &PlatformEnvironment, path: &Path) -> Result<SystemTime, String> {
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    };
    let uid = env
        .current_user_id()
        .ok_or("Current-user identity is unavailable")?;
    let parent = path.parent().ok_or("Node cache unit has no parent")?;
    if !env
        .temporary_roots()
        .iter()
        .any(|temp| parent == temp.join("node-compile-cache"))
        || !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| supported_node_group(n, uid))
    {
        return Err("Unit is outside the verified Node cache scope".into());
    }
    SymlinkGuard::validate_anchored_path(path, env).map_err(|e| e.to_string())?;
    let stamp = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
            m.mode(),
            m.uid(),
            m.nlink(),
        )
    };
    let owned_directory = |p: &Path| -> Result<fs::File, String> {
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(p)
            .map_err(|e| e.to_string())?;
        let m = f.metadata().map_err(|e| e.to_string())?;
        if m.uid() != uid || m.mode() & 0o022 != 0 || m.mode() & 0o300 != 0o300 {
            return Err("Node cache directory ownership or write access is unverified".into());
        }
        Ok(f)
    };
    let parent_handle = owned_directory(parent)?;
    let unit_handle = owned_directory(path)?;
    let parent_stamp = parent_handle.metadata().map_err(|e| e.to_string())?;
    let unit_stamp = unit_handle.metadata().map_err(|e| e.to_string())?;
    let mut latest = unit_stamp.modified().map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut total = 0u64;
    let started = std::time::Instant::now();
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        if entries.len() >= NODE_CACHE_MAX_FILES || started.elapsed() > Duration::from_secs(5) {
            return Err("Node cache validation exceeded its bounded budget".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        let m = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        if !m.is_file()
            || m.file_type().is_symlink()
            || m.uid() != uid
            || m.nlink() != 1
            || m.mode() & 0o133 != 0
            || m.len() > NODE_CACHE_MAX_FILE_BYTES as u64
        {
            return Err(
                "Node cache contains a link, executable, shared file or unverified object".into(),
            );
        }
        total = total
            .checked_add(m.len())
            .ok_or("Node cache size overflow")?;
        if total > NODE_CACHE_MAX_UNIT_BYTES {
            return Err("Node cache unit exceeds its validation byte budget".into());
        }
        // Open relative to the held directory descriptor; a replaced ancestor
        // must not redirect a file read. No links, FIFOs or devices are opened.
        let leaf =
            std::ffi::CString::new(entry.file_name().as_bytes()).map_err(|e| e.to_string())?;
        use std::os::fd::{AsRawFd, FromRawFd};
        let fd = unsafe {
            libc::openat(
                unit_handle.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut f = unsafe { fs::File::from_raw_fd(fd) };
        if stamp(&f.metadata().map_err(|e| e.to_string())?) != stamp(&m) {
            return Err("Node cache entry changed before inspection".into());
        }
        let mut bytes = Vec::new();
        (&mut f)
            .take(NODE_CACHE_MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| node_cache_file(name, &bytes))
        {
            return Err(
                "Node cache entry does not match the verified compilation-cache format".into(),
            );
        }
        if stamp(&f.metadata().map_err(|e| e.to_string())?) != stamp(&m) {
            return Err("Node cache entry changed during inspection".into());
        }
        if m.ctime() < 0 {
            return Err("Node cache entry has an invalid timestamp".into());
        }
        latest = latest.max(m.modified().map_err(|e| e.to_string())?).max(
            SystemTime::UNIX_EPOCH + Duration::new(m.ctime() as u64, m.ctime_nsec().max(0) as u32),
        );
        entries.push((p, m));
    }
    for (p, m) in entries {
        if stamp(&fs::symlink_metadata(&p).map_err(|e| e.to_string())?) != stamp(&m) {
            return Err("Node cache contents changed during inspection".into());
        }
    }
    if stamp(&fs::symlink_metadata(parent).map_err(|e| e.to_string())?) != stamp(&parent_stamp)
        || stamp(&fs::symlink_metadata(path).map_err(|e| e.to_string())?) != stamp(&unit_stamp)
    {
        return Err("Node cache namespace changed during inspection".into());
    }
    if unit_stamp.ctime() < 0 {
        return Err("Node cache unit has an invalid timestamp".into());
    }
    latest = latest.max(
        SystemTime::UNIX_EPOCH
            + Duration::new(
                unit_stamp.ctime() as u64,
                unit_stamp.ctime_nsec().max(0) as u32,
            ),
    );
    Ok(latest)
}

#[cfg(not(unix))]
pub(super) fn inspect(_: &PlatformEnvironment, _: &Path) -> Result<SystemTime, String> {
    Err("Node temporary cache cleanup has no native adapter on this platform".into())
}
