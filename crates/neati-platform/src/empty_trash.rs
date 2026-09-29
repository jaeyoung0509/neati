//! Bounded home-Trash removal using directory descriptors. This adapter takes
//! only a private snapshot produced by inventory; it never follows links or
//! expands the scope to volume Trash. Authorization/confirmation stays in the
//! application service's expiring one-shot plan store.
use crate::PlatformEnvironment;
use neati_core::domain::cleanup::empty_trash::{EmptyTrashOutcome, TrashSnapshot};
use std::sync::atomic::AtomicBool;

pub fn inventory(environment: &PlatformEnvironment) -> Result<TrashSnapshot, String> {
    if environment.platform() != neati_core::domain::platform::PlatformKind::Macos {
        return Err("Home Trash emptying is available on macOS only".into());
    }
    #[cfg(unix)]
    {
        native::inventory(&environment.user_home().ok_or("User home is unavailable")?)
    }
    #[cfg(not(unix))]
    {
        Err("Home Trash emptying is unavailable on this platform".into())
    }
}
pub fn execute(snapshot: &TrashSnapshot, cancelled: &AtomicBool) -> EmptyTrashOutcome {
    #[cfg(unix)]
    {
        native::execute(snapshot, cancelled)
    }
    #[cfg(not(unix))]
    {
        let _ = (snapshot, cancelled);
        EmptyTrashOutcome {
            items: vec![(
                String::new(),
                false,
                "Home Trash emptying is unavailable".into(),
            )],
            ..Default::default()
        }
    }
}

#[cfg(unix)]
mod native {
    use super::*;
    use neati_core::domain::{cleanup::empty_trash::TrashEntry, identity::FileIdentity};
    use std::{
        ffi::{CStr, CString, OsString},
        fs::File,
        io,
        mem::MaybeUninit,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                ffi::{OsStrExt, OsStringExt},
                fs::{MetadataExt, OpenOptionsExt},
            },
        },
        path::{Component, Path},
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    const MAX_ENTRIES: usize = 20_000;
    const MAX_DEPTH: usize = 32;
    const LIMIT: Duration = Duration::from_secs(30);
    fn error() -> String {
        io::Error::last_os_error().to_string()
    }
    fn name(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "Path contains NUL".into())
    }
    fn open_child(parent: &File, child: &Path) -> Result<File, String> {
        let child = name(child)?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                child.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
    fn root(home: &Path) -> Result<File, String> {
        if !home.is_absolute() || home.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("Invalid home path".into());
        }
        // Resolve each component without following a user-controlled symlink.
        let canonical = home.canonicalize().map_err(|e| e.to_string())?;
        if canonical != home {
            return Err("The home path must use its canonical location".into());
        }
        let mut directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/")
            .map_err(|e| e.to_string())?;
        for part in home.components() {
            if let Component::Normal(part) = part {
                directory = open_child(&directory, Path::new(part))?;
            }
        }
        let trash = open_child(&directory, Path::new(".Trash"))?;
        let m = trash.metadata().map_err(|e| e.to_string())?;
        if m.uid() != unsafe { libc::geteuid() } {
            return Err("The home Trash is owned by another user".into());
        }
        Ok(trash)
    }
    fn identity(file: &File) -> Result<FileIdentity, String> {
        let m = file.metadata().map_err(|e| e.to_string())?;
        Ok(FileIdentity::new(m.dev(), m.ino()))
    }
    fn stat(parent: &File, relative: &Path, label: &Path) -> Result<TrashEntry, String> {
        let name = name(relative)?;
        let mut out = MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                out.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(error());
        }
        let m = unsafe { out.assume_init() };
        if m.st_uid != unsafe { libc::geteuid() } {
            return Err("A Trash entry is owned by another user".into());
        }
        let mode = m.st_mode as u32;
        Ok(TrashEntry {
            relative: label.into(),
            identity: FileIdentity::new(m.st_dev as u64, m.st_ino),
            directory: mode & libc::S_IFMT as u32 == libc::S_IFDIR as u32,
            mode,
            size: m.st_size.max(0) as u64,
            modified_seconds: m.st_mtime,
            modified_nanos: m.st_mtime_nsec,
            allocated_bytes: (m.st_blocks.max(0) as u64).saturating_mul(512),
            links: m.st_nlink as u64,
        })
    }
    fn names(directory: &File) -> Result<Vec<OsString>, String> {
        // openat(".") gives an independent directory offset; dup would share it.
        let independent = open_child(directory, Path::new("."))?;
        use std::os::fd::IntoRawFd;
        let raw = independent.into_raw_fd();
        let stream = unsafe { libc::fdopendir(raw) };
        if stream.is_null() {
            unsafe { libc::close(raw) };
            return Err(error());
        }
        struct Dir(*mut libc::DIR);
        impl Drop for Dir {
            fn drop(&mut self) {
                unsafe { libc::closedir(self.0) };
            }
        }
        let stream = Dir(stream);
        let mut result = Vec::new();
        loop {
            // errno must distinguish end-of-directory from an incomplete read.
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            #[cfg(not(target_os = "macos"))]
            unsafe {
                *libc::__errno_location() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if io::Error::last_os_error().raw_os_error().unwrap_or(0) != 0 {
                    return Err(error());
                }
                break;
            }
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            if result.len() >= MAX_ENTRIES {
                return Err("Trash has too many entries for one review".into());
            }
            result.push(OsString::from_vec(bytes.to_vec()));
        }
        result.sort();
        Ok(result)
    }
    fn walk(
        directory: &File,
        relative: &Path,
        device: u64,
        entries: &mut Vec<TrashEntry>,
        start: Instant,
    ) -> Result<(), String> {
        if relative.components().count() > MAX_DEPTH || start.elapsed() > LIMIT {
            return Err("Trash inventory exceeded its execution budget".into());
        }
        for child in names(directory)? {
            if entries.len() >= MAX_ENTRIES {
                return Err("Trash inventory exceeded its entry limit".into());
            }
            let path = relative.join(&child);
            let entry = stat(directory, Path::new(&child), &path)?;
            if entry.identity.device() != device {
                return Err("Trash contains a mounted filesystem".into());
            }
            if entry.directory {
                let nested = open_child(directory, Path::new(&child))?;
                if !identity(&nested)?.same_entity(entry.identity) {
                    return Err("Trash changed during inventory".into());
                }
                walk(&nested, &path, device, entries, start)?;
            }
            entries.push(entry);
        }
        Ok(())
    }
    pub(super) fn inventory(home: &Path) -> Result<TrashSnapshot, String> {
        let root = root(home)?;
        let root_identity = identity(&root)?;
        let mut entries = Vec::new();
        walk(
            &root,
            Path::new(""),
            root_identity.device(),
            &mut entries,
            Instant::now(),
        )?;
        Ok(TrashSnapshot {
            home: home.into(),
            root_identity,
            entries,
        })
    }
    fn parent(root: &File, entry: &TrashEntry, snapshot: &TrashSnapshot) -> Result<File, String> {
        let mut directory = open_child(root, Path::new("."))?;
        let mut relative = std::path::PathBuf::new();
        for part in entry
            .relative
            .parent()
            .unwrap_or(Path::new(""))
            .components()
        {
            let Component::Normal(part) = part else {
                return Err("Invalid Trash entry path".into());
            };
            relative.push(part);
            directory = open_child(&directory, Path::new(part))?;
            let expected = snapshot
                .entries
                .iter()
                .find(|e| e.relative == relative && e.directory)
                .ok_or("Trash parent was not reviewed")?;
            if !identity(&directory)?.same_entity(expected.identity) {
                return Err("Trash parent changed since review".into());
            }
        }
        Ok(directory)
    }
    pub(super) fn execute(snapshot: &TrashSnapshot, cancelled: &AtomicBool) -> EmptyTrashOutcome {
        let mut result = EmptyTrashOutcome::default();
        let start = Instant::now();
        let run = || -> Result<File, String> {
            let current = inventory(&snapshot.home)?;
            if current.root_identity != snapshot.root_identity
                || current.entries != snapshot.entries
            {
                return Err("Trash changed since review; review it again".into());
            }
            let directory = root(&snapshot.home)?;
            if !identity(&directory)?.same_entity(snapshot.root_identity) {
                return Err("Trash root changed since review".into());
            }
            Ok(directory)
        };
        let root = match run() {
            Ok(root) => root,
            Err(e) => {
                result.items.push((String::new(), false, e));
                return result;
            }
        };
        for entry in &snapshot.entries {
            if cancelled.load(Ordering::Relaxed) || start.elapsed() > LIMIT {
                result.cancelled = true;
                break;
            }
            let remove = || -> Result<(), String> {
                let parent = parent(&root, entry, snapshot)?;
                let leaf = entry.relative.file_name().ok_or("Invalid Trash entry")?;
                let current = stat(&parent, Path::new(leaf), &entry.relative)?;
                if !current.identity.same_entity(entry.identity)
                    || current.mode != entry.mode
                    || (!entry.directory && current != *entry)
                {
                    return Err("Entry changed since review".into());
                }
                let leaf = name(Path::new(leaf))?;
                let flags = if entry.directory {
                    libc::AT_REMOVEDIR
                } else {
                    0
                };
                if unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), flags) } != 0 {
                    return Err(error());
                }
                Ok(())
            };
            match remove() {
                Ok(()) => {
                    result.removed_entries += 1;
                    if entry.links <= 1 {
                        result.removed_bytes =
                            result.removed_bytes.saturating_add(entry.allocated_bytes);
                    }
                    result.items.push((
                        entry.relative.to_string_lossy().into_owned(),
                        true,
                        "Permanently removed from home Trash".into(),
                    ));
                }
                Err(e) => {
                    result
                        .items
                        .push((entry.relative.to_string_lossy().into_owned(), false, e))
                }
            }
        }
        result
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn trash_removal_is_scoped_and_links_never_reach_their_targets() {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().canonicalize().unwrap();
            std::fs::create_dir_all(home.join(".Trash/nested")).unwrap();
            std::fs::write(home.join("keep"), b"outside").unwrap();
            std::fs::write(home.join(".Trash/nested/payload"), b"inside").unwrap();
            std::os::unix::fs::symlink(home.join("keep"), home.join(".Trash/link")).unwrap();
            let scan = inventory(&home).unwrap();
            let result = execute(&scan, &AtomicBool::new(false));
            assert_eq!(result.removed_entries, 3);
            assert!(result.items.iter().all(|i| i.1));
            assert!(home.join("keep").exists());
            assert!(home.join(".Trash").exists());
        }
        #[test]
        fn changed_inventory_and_cancellation_leave_items_intact() {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().canonicalize().unwrap();
            std::fs::create_dir(home.join(".Trash")).unwrap();
            std::fs::write(home.join(".Trash/file"), b"before").unwrap();
            let scan = inventory(&home).unwrap();
            std::fs::write(home.join(".Trash/new"), b"new").unwrap();
            assert_eq!(execute(&scan, &AtomicBool::new(false)).removed_entries, 0);
            let fresh = inventory(&home).unwrap();
            let result = execute(&fresh, &AtomicBool::new(true));
            assert!(result.cancelled);
            assert_eq!(result.removed_entries, 0);
            assert!(home.join(".Trash/file").exists());
        }
    }
}
