//! Permanent removal of an already authorized unit. Callers retain scope,
//! ownership, use and provenance policy; this port binds the reviewed identity
//! and never follows links. No shell or Trash fallback is involved.
use neati_core::domain::identity::FileIdentity;
use std::path::Path;

pub trait RemovalBackend: Send + Sync {
    fn remove(&self, path: &Path, expected: FileIdentity) -> Result<(), String>;

    /// Native inspection may take time. Recheck the owning workflow's policy
    /// after that inspection, immediately before the first unlink.
    fn remove_checked(
        &self,
        path: &Path,
        expected: FileIdentity,
        final_check: &dyn Fn(&[i32]) -> Result<(), String>,
    ) -> Result<(), String> {
        final_check(&[])?;
        self.remove(path, expected)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NativeRemovalBackend;
impl RemovalBackend for NativeRemovalBackend {
    fn remove(&self, path: &Path, expected: FileIdentity) -> Result<(), String> {
        self.remove_checked(path, expected, &|_| Ok(()))
    }

    fn remove_checked(
        &self,
        path: &Path,
        expected: FileIdentity,
        final_check: &dyn Fn(&[i32]) -> Result<(), String>,
    ) -> Result<(), String> {
        if expected.is_unknown() {
            return Err("Permanent removal requires a verified identity".into());
        }
        #[cfg(unix)]
        {
            unix::remove(path, expected, final_check)
        }
        #[cfg(windows)]
        {
            windows::remove(path, expected, final_check)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, final_check);
            Err("Permanent removal is unavailable on this platform".into())
        }
    }
}

#[derive(Debug, Default)]
pub struct MockRemovalBackend {
    removed: std::sync::Mutex<Vec<std::path::PathBuf>>,
    failing: Option<std::path::PathBuf>,
}
impl MockRemovalBackend {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_failing_path(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            failing: Some(path.into()),
            ..Default::default()
        }
    }
    pub fn removed(&self) -> Vec<std::path::PathBuf> {
        self.removed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}
impl RemovalBackend for MockRemovalBackend {
    fn remove(&self, path: &Path, _: FileIdentity) -> Result<(), String> {
        if self.failing.as_deref() == Some(path) {
            return Err("Fixture removal refused".into());
        }
        self.removed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(path.into());
        Ok(())
    }
}

#[cfg(unix)]
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 128;
const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        ffi::{CStr, CString, OsString},
        fs::File,
        io,
        mem::MaybeUninit,
        os::{
            fd::{AsRawFd, FromRawFd, IntoRawFd},
            unix::{
                ffi::{OsStrExt, OsStringExt},
                fs::{MetadataExt, OpenOptionsExt},
            },
        },
        path::Component,
        time::Instant,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Stamp {
        identity: FileIdentity,
        mode: u32,
        size: i64,
        modified: i64,
        nanos: i64,
    }
    struct Node {
        name: OsString,
        stamp: Stamp,
        children: Vec<Node>,
    }
    fn error() -> String {
        io::Error::last_os_error().to_string()
    }
    fn name(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "Path contains NUL".into())
    }
    fn open(parent: &File, leaf: &Path) -> Result<File, String> {
        let leaf = name(leaf)?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            Err(error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
    fn entity(file: &File) -> Result<FileIdentity, String> {
        let m = file.metadata().map_err(|e| e.to_string())?;
        Ok(FileIdentity::new(m.dev(), m.ino()))
    }
    fn stat(parent: &File, leaf: &Path) -> Result<Stamp, String> {
        let leaf = name(leaf)?;
        let mut out = MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                out.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(error());
        }
        let m = unsafe { out.assume_init() };
        Ok(Stamp {
            identity: FileIdentity::new(m.st_dev as u64, m.st_ino),
            mode: m.st_mode as u32,
            size: m.st_size,
            modified: m.st_mtime,
            nanos: m.st_mtime_nsec,
        })
    }
    fn directory(stamp: Stamp) -> bool {
        stamp.mode & libc::S_IFMT as u32 == libc::S_IFDIR as u32
    }
    fn names(directory: &File) -> Result<Vec<OsString>, String> {
        let raw = open(directory, Path::new("."))?.into_raw_fd();
        let stream = unsafe { libc::fdopendir(raw) };
        if stream.is_null() {
            unsafe {
                libc::close(raw);
            }
            return Err(error());
        }
        struct Dir(*mut libc::DIR);
        impl Drop for Dir {
            fn drop(&mut self) {
                unsafe {
                    libc::closedir(self.0);
                }
            }
        }
        let stream = Dir(stream);
        let mut result = Vec::new();
        loop {
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
                return Err("Removal entry limit exceeded".into());
            }
            result.push(OsString::from_vec(bytes.to_vec()));
        }
        result.sort();
        Ok(result)
    }
    fn budget(start: Instant, count: usize, depth: usize) -> Result<(), String> {
        if start.elapsed() > TIME_LIMIT || count > MAX_ENTRIES || depth > MAX_DEPTH {
            Err("Permanent removal exceeded its bounded execution budget".into())
        } else {
            Ok(())
        }
    }
    fn snapshot(
        parent: &File,
        leaf: &Path,
        device: u64,
        start: Instant,
        count: &mut usize,
        depth: usize,
    ) -> Result<Node, String> {
        *count += 1;
        budget(start, *count, depth)?;
        let stamp = stat(parent, leaf)?;
        if stamp.identity.device() != device {
            return Err("A mounted filesystem is outside this removal unit".into());
        }
        let kind = stamp.mode & libc::S_IFMT as u32;
        if ![
            libc::S_IFDIR as u32,
            libc::S_IFREG as u32,
            libc::S_IFLNK as u32,
        ]
        .contains(&kind)
        {
            return Err("Unsupported entry kind in removal unit".into());
        }
        let mut children = Vec::new();
        if directory(stamp) {
            let dir = open(parent, leaf)?;
            if !entity(&dir)?.same_entity(stamp.identity) {
                return Err("Directory changed during removal inspection".into());
            }
            for child in names(&dir)? {
                children.push(snapshot(
                    &dir,
                    Path::new(&child),
                    device,
                    start,
                    count,
                    depth + 1,
                )?);
            }
        }
        if stat(parent, leaf)? != stamp {
            return Err("Entry changed during removal inspection".into());
        }
        Ok(Node {
            name: leaf.as_os_str().into(),
            stamp,
            children,
        })
    }
    // Every unlink uses the inspected parent descriptor. A replacement with
    // a different identity is refused, including after a parent rename.
    fn erase(
        parent: &File,
        node: &Node,
        start: Instant,
        count: &mut usize,
        depth: usize,
    ) -> Result<(), String> {
        *count += 1;
        budget(start, *count, depth)?;
        let leaf = Path::new(&node.name);
        if stat(parent, leaf)? != node.stamp {
            return Err("Entry changed before permanent removal".into());
        }
        if directory(node.stamp) {
            let dir = open(parent, leaf)?;
            if !entity(&dir)?.same_entity(node.stamp.identity) {
                return Err("Directory was replaced before removal".into());
            }
            let expected_names: Vec<_> = node.children.iter().map(|n| n.name.clone()).collect();
            if names(&dir)? != expected_names {
                return Err("Directory contents changed before removal".into());
            }
            let original = node.stamp.mode & 0o7777;
            let writable = original | 0o700;
            if original != writable
                && unsafe { libc::fchmod(dir.as_raw_fd(), writable as libc::mode_t) } != 0
            {
                return Err(error());
            }
            let result = (|| {
                for child in &node.children {
                    erase(&dir, child, start, count, depth + 1)?;
                }
                if !names(&dir)?.is_empty() {
                    return Err("New entries appeared during removal".into());
                }
                let current = stat(parent, leaf)?;
                if !current.identity.same_entity(node.stamp.identity) || !directory(current) {
                    return Err("Directory locator changed during removal".into());
                }
                let leaf = name(leaf)?;
                if unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), libc::AT_REMOVEDIR) }
                    != 0
                {
                    return Err(error());
                }
                Ok(())
            })();
            if let Err(reason) = &result {
                if original != writable
                    && unsafe { libc::fchmod(dir.as_raw_fd(), original as libc::mode_t) } != 0
                {
                    return Err(format!(
                        "{reason}; could not restore directory permissions: {}",
                        error()
                    ));
                }
            }
            result
        } else {
            let leaf = name(leaf)?;
            if unsafe { libc::unlinkat(parent.as_raw_fd(), leaf.as_ptr(), 0) } != 0 {
                Err(error())
            } else {
                Ok(())
            }
        }
    }
    fn parent(path: &Path) -> Result<File, String> {
        if !path.is_absolute()
            || path.file_name().is_none()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err("Removal requires a normalized absolute non-root path".into());
        }
        let mut dir = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/")
            .map_err(|e| e.to_string())?;
        // System aliases (/var, /tmp) are normalized by the caller's existing
        // path guard. Refuse every unresolved link in the supplied locator.
        for part in path.parent().unwrap().components() {
            if let Component::Normal(part) = part {
                dir = open(&dir, Path::new(part))?;
            }
        }
        Ok(dir)
    }
    pub(super) fn remove(
        path: &Path,
        expected: FileIdentity,
        final_check: &dyn Fn(&[i32]) -> Result<(), String>,
    ) -> Result<(), String> {
        let parent = parent(path)?;
        let leaf = Path::new(path.file_name().ok_or("Invalid removal leaf")?);
        let stamp = stat(&parent, leaf)?;
        if !stamp.identity.same_entity(expected)
            || stamp.mode & libc::S_IFMT as u32 == libc::S_IFLNK as u32
        {
            return Err("Reviewed root changed or is a link; removal refused".into());
        }
        let start = Instant::now();
        let tree = snapshot(&parent, leaf, expected.device(), start, &mut 0, 0)?;
        if tree.stamp != stamp {
            return Err("Reviewed root changed during inspection".into());
        }
        final_check(&[parent.as_raw_fd()])?;
        erase(&parent, &tree, start, &mut 0, 0).map_err(|e| {
            format!("Permanent removal stopped; some entries may already be removed: {e}")
        })
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{io, path::PathBuf, time::Instant};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::*,
    };
    pub(super) struct Held {
        handle: HANDLE,
        path: PathBuf,
        pub(super) identity: FileIdentity,
        directory: bool,
    }
    impl Drop for Held {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
    fn error() -> String {
        io::Error::last_os_error().to_string()
    }
    fn identity(handle: HANDLE) -> Result<FileIdentity, String> {
        let mut id: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                &mut id as *mut _ as *mut _,
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } != 0
        {
            let low = u64::from_le_bytes(id.FileId.Identifier[..8].try_into().unwrap());
            let high = u64::from_le_bytes(id.FileId.Identifier[8..].try_into().unwrap());
            let entity = FileIdentity::new(id.VolumeSerialNumber, low ^ high);
            if !entity.is_unknown() {
                return Ok(entity);
            }
        }
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err(error());
        }
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        if index == 0 {
            return Err("Permanent removal requires a stable Windows file ID".into());
        }
        Ok(FileIdentity::new(
            u64::from(info.dwVolumeSerialNumber),
            index,
        ))
    }
    pub(super) fn open(path: &Path, deleting: bool) -> Result<Held, String> {
        let wide = crate::NativePlatformPaths::to_verbatim_wide(path);
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES
                    | if deleting {
                        DELETE | FILE_WRITE_ATTRIBUTES
                    } else {
                        0
                    },
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(error());
        }
        let mut held = Held {
            handle,
            path: path.into(),
            identity: FileIdentity::UNKNOWN,
            directory: false,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err(error());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("Links and reparse points are refused by permanent removal".into());
        }
        held.identity = identity(handle)?;
        held.directory = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
        Ok(held)
    }
    struct Node {
        held: Held,
        children: Vec<Node>,
    }
    fn snapshot(
        held: Held,
        device: u64,
        start: Instant,
        count: &mut usize,
        depth: usize,
    ) -> Result<Node, String> {
        *count += 1;
        // All inspected handles deny delete-sharing and pin the complete
        // ancestry until mutation. Bound the number of simultaneously held handles.
        if *count > 4096 || depth > MAX_DEPTH || start.elapsed() > TIME_LIMIT {
            return Err("Windows removal inspection budget exceeded".into());
        }
        if held.identity.device() != device {
            return Err("Mounted filesystem is outside this removal unit".into());
        }
        let mut children = Vec::new();
        if held.directory {
            let mut paths = Vec::new();
            for entry in std::fs::read_dir(&held.path).map_err(|e| e.to_string())? {
                if paths.len() >= 4096 {
                    return Err("Windows removal entry limit exceeded".into());
                }
                paths.push(entry.map_err(|e| e.to_string())?.path());
            }
            paths.sort();
            for path in paths {
                children.push(snapshot(
                    open(&path, true)?,
                    device,
                    start,
                    count,
                    depth + 1,
                )?);
            }
        }
        Ok(Node { held, children })
    }
    fn erase(node: Node, start: Instant) -> Result<(), String> {
        if start.elapsed() > TIME_LIMIT {
            return Err("Permanent removal execution budget exceeded".into());
        }
        for child in node.children {
            erase(child, start)?;
        }
        if !identity(node.held.handle)?.same_entity(node.held.identity) {
            return Err("Windows entry identity changed".into());
        }
        let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        if unsafe {
            GetFileInformationByHandleEx(
                node.held.handle,
                FileBasicInfo,
                &mut basic as *mut _ as *mut _,
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(error());
        }
        let original = basic.FileAttributes;
        if original & FILE_ATTRIBUTE_READONLY != 0 {
            basic.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
            if basic.FileAttributes == 0 {
                basic.FileAttributes = FILE_ATTRIBUTE_NORMAL;
            }
            if unsafe {
                SetFileInformationByHandle(
                    node.held.handle,
                    FileBasicInfo,
                    &basic as *const _ as *const _,
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            } == 0
            {
                return Err(error());
            }
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                node.held.handle,
                FileDispositionInfo,
                &disposition as *const _ as *const _,
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            let reason = error();
            if original & FILE_ATTRIBUTE_READONLY != 0 {
                basic.FileAttributes = original;
                if unsafe {
                    SetFileInformationByHandle(
                        node.held.handle,
                        FileBasicInfo,
                        &basic as *const _ as *const _,
                        std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                    )
                } == 0
                {
                    return Err(format!(
                        "{reason}; could not restore attributes: {}",
                        error()
                    ));
                }
            }
            return Err(reason);
        }
        Ok(())
    }
    pub(super) fn remove(
        path: &Path,
        expected: FileIdentity,
        final_check: &dyn Fn(&[i32]) -> Result<(), String>,
    ) -> Result<(), String> {
        let text = path.to_string_lossy();
        use crate::path_algebra::{self, PathFlavor};
        if !path.is_absolute()
            || path.file_name().is_none()
            || path_algebra::has_parent_traversal(&text, PathFlavor::Windows)
            || path_algebra::contains_short_name(&text, PathFlavor::Windows)
            || path_algebra::has_alternate_data_stream(&text, PathFlavor::Windows)
            || path_algebra::has_trailing_dot_or_space(&text, PathFlavor::Windows)
        {
            return Err("Ambiguous Windows removal path".into());
        }
        let mut parents = Vec::new();
        let mut ancestors: Vec<_> = path.parent().unwrap().ancestors().collect();
        ancestors.reverse();
        for ancestor in ancestors {
            parents.push(open(ancestor, false)?);
        }
        let held = open(path, true)?;
        if !held.identity.same_entity(expected) {
            return Err("Reviewed root changed before removal".into());
        }
        let start = Instant::now();
        let tree = snapshot(held, expected.device(), start, &mut 0, 0)?;
        final_check(&[])?;
        let result = erase(tree, start).map_err(|e| {
            format!("Permanent removal stopped; some entries may already be removed: {e}")
        });
        drop(parents);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    fn identity(path: &Path) -> FileIdentity {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path).unwrap();
        FileIdentity::new(m.dev(), m.ino())
    }
    #[cfg(windows)]
    fn identity(path: &Path) -> FileIdentity {
        windows::open(path, false).unwrap().identity
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn permanent_removal_removes_only_the_reviewed_unit() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let unit = base.join("unit");
        std::fs::create_dir_all(unit.join("nested")).unwrap();
        std::fs::write(unit.join("nested/payload"), "cache").unwrap();
        let sibling = base.join("keep");
        std::fs::write(&sibling, "keep").unwrap();
        NativeRemovalBackend.remove(&unit, identity(&unit)).unwrap();
        assert!(!unit.exists());
        assert_eq!(std::fs::read_to_string(sibling).unwrap(), "keep");
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn stale_identity_never_removes_a_substitute() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let unit = base.join("unit");
        std::fs::create_dir(&unit).unwrap();
        let expected = identity(&unit);
        std::fs::rename(&unit, base.join("original")).unwrap();
        std::fs::create_dir(&unit).unwrap();
        std::fs::write(unit.join("source"), "keep").unwrap();
        assert!(NativeRemovalBackend.remove(&unit, expected).is_err());
        assert_eq!(
            std::fs::read_to_string(unit.join("source")).unwrap(),
            "keep"
        );
    }
    #[cfg(unix)]
    #[test]
    fn internal_links_are_unlinked_without_following_them_and_link_roots_are_refused() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let outside = base.join("outside");
        std::fs::write(&outside, "keep").unwrap();
        let unit = base.join("unit");
        std::fs::create_dir(&unit).unwrap();
        symlink(&outside, unit.join("link")).unwrap();
        NativeRemovalBackend.remove(&unit, identity(&unit)).unwrap();
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
        let link = base.join("link");
        symlink(&outside, &link).unwrap();
        assert!(NativeRemovalBackend.remove(&link, identity(&link)).is_err());
        assert!(outside.exists());
        assert!(link.is_symlink());
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn final_use_refusal_leaves_every_entry_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let unit = base.join("unit");
        std::fs::create_dir(&unit).unwrap();
        std::fs::write(unit.join("payload"), "keep").unwrap();
        let calls = std::cell::Cell::new(0);
        let result = NativeRemovalBackend.remove_checked(&unit, identity(&unit), &|_| {
            calls.set(calls.get() + 1);
            Err("Owner became active".into())
        });
        assert_eq!(calls.get(), 1);
        assert_eq!(result.unwrap_err(), "Owner became active");
        assert_eq!(
            std::fs::read_to_string(unit.join("payload")).unwrap(),
            "keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_leaf_replacement_after_inspection_is_never_unlinked() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let unit = base.join("unit");
        std::fs::create_dir(&unit).unwrap();
        let payload = unit.join("payload");
        std::fs::write(&payload, "original").unwrap();
        let result = NativeRemovalBackend.remove_checked(&unit, identity(&unit), &|_| {
            std::fs::rename(&payload, base.join("saved-original")).unwrap();
            std::fs::write(&payload, "substitute").unwrap();
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(payload).unwrap(), "substitute");
        assert_eq!(
            std::fs::read_to_string(base.join("saved-original")).unwrap(),
            "original"
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_only_directories_are_removed_without_mutating_siblings() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let unit = base.join("unit");
        std::fs::create_dir_all(unit.join("nested")).unwrap();
        std::fs::write(unit.join("nested/payload"), "cache").unwrap();
        std::fs::set_permissions(unit.join("nested"), std::fs::Permissions::from_mode(0o555))
            .unwrap();
        std::fs::set_permissions(&unit, std::fs::Permissions::from_mode(0o555)).unwrap();
        NativeRemovalBackend.remove(&unit, identity(&unit)).unwrap();
        assert!(!unit.exists());
    }
    #[cfg(unix)]
    #[test]
    fn a_partial_removal_reports_failure_without_claiming_restoration() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let unit = base.join("unit");
        std::fs::create_dir(&unit).unwrap();
        std::fs::write(unit.join("a"), "first").unwrap();
        std::fs::write(unit.join("b"), "second").unwrap();
        let result = NativeRemovalBackend.remove_checked(&unit, identity(&unit), &|_| {
            std::fs::write(unit.join("b"), "changed contents").unwrap();
            Ok(())
        });
        assert!(result
            .unwrap_err()
            .contains("some entries may already be removed"));
        assert!(!unit.join("a").exists());
        assert_eq!(
            std::fs::read_to_string(unit.join("b")).unwrap(),
            "changed contents"
        );
    }
}
