//! No-follow, descriptor-relative acquisition of an owner's advisory lock.

use super::SymlinkGuard;
use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use zenith_platform::PlatformEnvironment;

pub struct OwnerFileLock {
    directories: Vec<(PathBuf, File)>,
    path: PathBuf,
    file: File,
}

impl OwnerFileLock {
    /// Discovery is read-only: absent metadata is legitimate, unsafe metadata is not.
    pub fn inspect(
        root: &Path,
        parts: &[&str],
        environment: &PlatformEnvironment,
    ) -> Result<(), String> {
        validate_parts(parts)?;
        SymlinkGuard::validate_anchored_path(root, environment).map_err(|e| e.to_string())?;
        let mut path = root.to_path_buf();
        for (index, part) in parts.iter().enumerate() {
            path.push(part);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.to_string()),
                Ok(metadata) => {
                    let valid = if index + 1 == parts.len() {
                        metadata.is_file() && metadata.nlink() == 1
                    } else {
                        metadata.is_dir()
                    };
                    if !valid || metadata.file_type().is_symlink() {
                        return Err("The owner lock contains a link or unexpected entry".into());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn acquire(
        root: &Path,
        parts: &[&str],
        environment: &PlatformEnvironment,
    ) -> Result<Self, String> {
        Self::inspect(root, parts, environment)?;
        let root_file = File::options()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)
            .map_err(|e| e.to_string())?;
        verify_entry(&root_file, root, true)?;
        SymlinkGuard::validate_anchored_path(root, environment).map_err(|e| e.to_string())?;
        let mut directories = vec![(root.to_path_buf(), root_file)];
        for part in &parts[..parts.len() - 1] {
            let (parent_path, parent) = directories.last().unwrap();
            verify_entry(parent, parent_path, true)?;
            let name = CString::new(*part).map_err(|e| e.to_string())?;
            // Each component is created relative to a held directory, never by
            // traversing a pathname that an intervening symlink could redirect.
            let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
            if created < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.to_string());
                }
            }
            let descriptor = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            let directory = opened_file(descriptor)?;
            let path = parent_path.join(part);
            verify_entry(&directory, &path, true)?;
            directories.push((path, directory));
        }
        let (parent_path, parent) = directories.last().unwrap();
        let name = CString::new(*parts.last().unwrap()).map_err(|e| e.to_string())?;
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        let file = opened_file(descriptor)?;
        let path = parent_path.join(parts.last().unwrap());
        verify_entry(&file, &path, false)?;
        file.try_lock()
            .map_err(|_| "The artifact is locked by its owner".to_string())?;
        let held = Self {
            directories,
            path,
            file,
        };
        held.verify(environment)?;
        Ok(held)
    }

    pub fn verify(&self, environment: &PlatformEnvironment) -> Result<(), String> {
        SymlinkGuard::validate_anchored_path(&self.path, environment).map_err(|e| e.to_string())?;
        for (path, directory) in &self.directories {
            verify_entry(directory, path, true)?;
        }
        verify_entry(&self.file, &self.path, false)
    }
}

// Closing the descriptor releases the lock. Never unlink it: a waiting owner
// must not lock an old inode while another owner opens a replacement at this path.
fn opened_file(descriptor: libc::c_int) -> Result<File, String> {
    if descriptor < 0 {
        Err(io::Error::last_os_error().to_string())
    } else {
        // SAFETY: a successful openat returns a fresh descriptor owned here.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

fn validate_parts(parts: &[&str]) -> Result<(), String> {
    if parts.is_empty()
        || parts.len() > 8
        || parts.iter().any(|part| {
            let mut components = Path::new(part).components();
            !matches!(components.next(), Some(Component::Normal(_)))
                || components.next().is_some()
                || part.contains('\0')
                || part.contains('/')
        })
    {
        return Err("The owner lock path is not a bounded relative path".into());
    }
    Ok(())
}

fn verify_entry(file: &File, path: &Path, directory: bool) -> Result<(), String> {
    let held = file.metadata().map_err(|e| e.to_string())?;
    let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let valid_type = if directory {
        held.is_dir() && current.is_dir()
    } else {
        held.is_file() && current.is_file() && held.nlink() == 1 && current.nlink() == 1
    };
    if !valid_type
        || current.file_type().is_symlink()
        || held.dev() != current.dev()
        || held.ino() != current.ino()
    {
        return Err("The owner lock path changed or is not an ordinary entry".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use zenith_platform::path_algebra::PathFlavor;

    fn fixture() -> (tempfile::TempDir, PlatformEnvironment, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let environment =
            PlatformEnvironment::simulated(PathFlavor::current()).with_home(temp.path());
        let root = temp.path().join("cache");
        fs::create_dir(&root).unwrap();
        (temp, environment, root)
    }

    #[test]
    fn absent_locks_are_read_only_until_acquisition_and_remain_after_release() {
        let (_temp, environment, root) = fixture();
        let parts = ["locks", "ab", "artifact"];
        OwnerFileLock::inspect(&root, &parts, &environment).unwrap();
        assert!(!root.join("locks").exists());
        let held = OwnerFileLock::acquire(&root, &parts, &environment).unwrap();
        let path = root.join("locks/ab/artifact");
        let owner = File::options().read(true).write(true).open(&path).unwrap();
        assert!(owner.try_lock().is_err());
        drop(held);
        owner.try_lock().unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn links_special_entries_and_parent_substitutions_are_refused() {
        let (temp, environment, root) = fixture();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("locks")).unwrap();
        assert!(OwnerFileLock::acquire(&root, &["locks", "ab", "artifact"], &environment).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_file(root.join("locks")).unwrap();
        let held =
            OwnerFileLock::acquire(&root, &["locks", "ab", "artifact"], &environment).unwrap();
        fs::rename(root.join("locks"), root.join("old-locks")).unwrap();
        symlink(root.join("old-locks"), root.join("locks")).unwrap();
        assert!(held.verify(&environment).is_err());
    }

    #[test]
    fn held_hardlinked_and_replaced_lock_files_are_refused() {
        let (_temp, environment, root) = fixture();
        let parts = ["locks", "ab", "artifact"];
        let held = OwnerFileLock::acquire(&root, &parts, &environment).unwrap();
        assert!(OwnerFileLock::acquire(&root, &parts, &environment).is_err());
        let path = root.join("locks/ab/artifact");
        fs::hard_link(&path, root.join("alias")).unwrap();
        assert!(OwnerFileLock::inspect(&root, &parts, &environment).is_err());
        assert!(held.verify(&environment).is_err());
        fs::remove_file(root.join("alias")).unwrap();
        fs::rename(&path, path.with_file_name("old")).unwrap();
        fs::write(&path, b"").unwrap();
        assert!(held.verify(&environment).is_err());
    }

    #[test]
    fn non_file_locks_and_non_component_names_never_gain_authority() {
        let (_temp, environment, root) = fixture();
        fs::create_dir_all(root.join("locks/ab/artifact")).unwrap();
        assert!(OwnerFileLock::acquire(&root, &["locks", "ab", "artifact"], &environment).is_err());
        for parts in [
            vec![],
            vec![".."],
            vec!["/outside"],
            vec!["a/b"],
            vec!["a\0b"],
        ] {
            assert!(OwnerFileLock::acquire(&root, &parts, &environment).is_err());
        }
    }
}
