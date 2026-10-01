//! Bounded no-follow observations of exact temporary units. These snapshots
//! are evidence, not permission: only the dedicated reviewed service may use
//! `move_reviewed_unit` after binding explicit consent to its private plan.
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::atomic::Ordering;
#[cfg(unix)]
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::PlatformEnvironment;
use neati_core::application::dto::temporary_storage::TemporaryUsageObservation;
use neati_core::domain::identity::FileIdentity;
use neati_core::domain::platform::PlatformKind;
use neati_core::domain::storage::{TemporaryContentKind, TemporaryUsageState};

pub const MAX_TEMPORARY_UNITS: usize = 512;
#[cfg(unix)]
const MAX_UNIT_ENTRIES: usize = 250_000;
#[cfg(unix)]
const MAX_UNIT_DEPTH: usize = 96;
#[cfg(unix)]
const UNIT_TIME: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct TemporaryUnitSnapshot {
    framework: Option<neati_core::domain::storage::FrameworkGeneratedKind>,
    pub root: PathBuf,
    pub root_identity: FileIdentity,
    pub path: PathBuf,
    pub identity: FileIdentity,
    pub ancestors: Vec<FileIdentity>,
    pub fingerprint: String,
    pub changed_at: Option<(i64, i64)>,
    pub physical_overlap: bool,
    pub logical_bytes: Option<u64>,
    pub allocated_bytes: Option<u64>,
    pub newest_activity: Option<u64>,
    pub partial: bool,
    pub blocked_reason: Option<String>,
    pub contents: Vec<TemporaryContentKind>,
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn unknown_usage(reason: impl Into<String>) -> TemporaryUsageObservation {
    TemporaryUsageObservation {
        state: TemporaryUsageState::UnableToDetermine,
        observed_at: unix_timestamp(), probe: "Open files, working directories and held build locks".into(),
        evidence: vec![reason.into()],
        limitation: "Missing evidence is not an idle verdict. Absence of detected use does not establish abandonment or recoverability.".into(),
    }
}

pub fn observe_temporary_use(
    environment: &PlatformEnvironment,
    path: &Path,
) -> TemporaryUsageObservation {
    observe_with_inspection_handles(environment, path, &[])
}

fn observe_with_inspection_handles(
    environment: &PlatformEnvironment,
    path: &Path,
    inspection_fds: &[i32],
) -> TemporaryUsageObservation {
    if environment.platform() != PlatformKind::Macos {
        return unknown_usage("This platform has no implemented temporary-unit use probe.");
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("/usr/sbin/lsof");
        command.args(["-nP", "-Fpcfn"]);
        if std::fs::symlink_metadata(path)
            .ok()
            .is_some_and(|meta| meta.is_dir())
        {
            command.arg("+D");
        } else {
            command.arg("--");
        }
        command.arg(path);
        match crate::subprocess::run_with_timeout(command, Duration::from_secs(3)) {
            Ok(output) => classify_usage_with_inspection_handles(
                output.status.code(),
                &output.stdout,
                &output.stderr,
                Some(std::process::id()),
                inspection_fds,
            ),
            Err(error) => unknown_usage(format!(
                "The bounded use probe failed or timed out: {error}"
            )),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, inspection_fds);
        unknown_usage("The macOS use adapter cannot run on this host.")
    }
}

#[cfg(test)]
fn classify_usage(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> TemporaryUsageObservation {
    classify_usage_with_inspection_handles(code, stdout, stderr, None, &[])
}

#[cfg(any(test, target_os = "macos"))]
fn classify_usage_with_inspection_handles(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    inspection_pid: Option<u32>,
    inspection_fds: &[i32],
) -> TemporaryUsageObservation {
    let text = String::from_utf8_lossy(stdout);
    let mut pid = String::new();
    let mut command = String::new();
    let mut descriptor = String::new();
    let mut evidence = Vec::new();
    let mut excluded = 0usize;
    let mut named = 0usize;
    // Ignoring our own inspection handles is safe only for a complete lsof
    // field stream. A malformed suffix, orphan field or truncated descriptor
    // must never turn a failed probe into a negative use observation.
    let mut complete_fields = std::str::from_utf8(stdout).is_ok() && stdout.last() == Some(&b'\n');
    let mut process_named = false;
    let mut pending_descriptor = false;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix('p') {
            if pending_descriptor || (!pid.is_empty() && !process_named) {
                complete_fields = false;
            }
            if value.parse::<u32>().ok().is_none_or(|value| value == 0) {
                complete_fields = false;
            }
            pid = value.to_string();
            command.clear();
            descriptor.clear();
            process_named = false;
            pending_descriptor = false;
            continue;
        }
        if let Some(value) = line.strip_prefix('c') {
            if pid.is_empty() || !command.is_empty() || pending_descriptor || value.is_empty() {
                complete_fields = false;
            }
            command = value.to_string();
            continue;
        }
        if let Some(value) = line.strip_prefix('f') {
            if pid.is_empty() || command.is_empty() || pending_descriptor || value.is_empty() {
                complete_fields = false;
            }
            descriptor = value.to_string();
            pending_descriptor = true;
            continue;
        }
        if let Some(value) = line.strip_prefix('n') {
            if !pending_descriptor || value.is_empty() {
                complete_fields = false;
            }
            pending_descriptor = false;
            process_named = true;
            named += 1;
            let digits = descriptor.bytes().take_while(u8::is_ascii_digit).count();
            let suffix = &descriptor[digits..];
            let fd = matches!(suffix, "" | "r" | "w" | "u")
                .then(|| descriptor[..digits].parse::<i32>().ok())
                .flatten();
            if digits > 0 && fd.is_none() {
                complete_fields = false;
            }
            if inspection_pid.is_some()
                && pid.parse::<u32>().ok() == inspection_pid
                && fd.is_some_and(|fd| inspection_fds.contains(&fd))
            {
                excluded += 1;
                continue;
            }
            let kind = if descriptor == "cwd" {
                "Working directory"
            } else if value.ends_with(".lock") || value.ends_with(".cargo-lock") {
                "Open build/lock file"
            } else {
                "Open file"
            };
            if evidence.len() < 12 {
                evidence.push(format!("{kind}: {value} · {command} (PID {pid})"));
            }
            continue;
        }
        complete_fields = false;
    }
    complete_fields &= !pending_descriptor && (pid.is_empty() || process_named);
    let state = if !evidence.is_empty() {
        TemporaryUsageState::InUse
    } else if stderr.is_empty()
        && ((code == Some(1) && stdout.is_empty())
            || (matches!(code, Some(0 | 1))
                && complete_fields
                && excluded > 0
                && named == excluded))
    {
        TemporaryUsageState::NoUseDetected
    } else {
        TemporaryUsageState::UnableToDetermine
    };
    if (stdout.len() >= 1024 * 1024 || stderr.len() >= 1024 * 1024)
        && state != TemporaryUsageState::InUse
    {
        return unknown_usage("The use probe exceeded its output budget.");
    }
    if state == TemporaryUsageState::NoUseDetected {
        evidence.push("The completed lsof probe detected no open file, working directory or held build lock in this unit.".into());
    } else if state == TemporaryUsageState::UnableToDetermine {
        evidence.push("The probe was unsupported, incomplete or unsuccessful; no negative use verdict was established.".into());
    }
    if !stderr.is_empty() {
        evidence.push("The probe reported incomplete coverage or an access failure.".into());
    }
    TemporaryUsageObservation {
        state, observed_at: unix_timestamp(), probe: "macOS lsof · 3-second budget".into(), evidence,
        limitation: "No use detected is an observation, not proof that a folder is abandoned. A future session may still need it; unpublished work and recovery were not verified.".into(),
    }
}

/// Direct children only. Aliased roots are deduplicated by stable identity;
/// nested stated roots are omitted from the root list to avoid double counting.
pub fn enumerate_temporary_units(
    environment: &PlatformEnvironment,
) -> (Vec<(PathBuf, PathBuf)>, Vec<String>, bool) {
    #[cfg(unix)]
    {
        unix::enumerate(environment)
    }
    #[cfg(not(unix))]
    {
        let _ = environment;
        (
            vec![],
            vec!["Temporary review is unavailable on this host.".into()],
            false,
        )
    }
}

pub fn snapshot_temporary_unit(
    environment: &PlatformEnvironment,
    root: &Path,
    path: &Path,
    cancel: &AtomicBool,
) -> TemporaryUnitSnapshot {
    #[cfg(unix)]
    {
        unix::snapshot(environment, root, path, cancel, None)
    }
    #[cfg(not(unix))]
    {
        let _ = (environment, cancel);
        TemporaryUnitSnapshot {
            framework: None,
            root: root.into(),
            path: path.into(),
            root_identity: FileIdentity::UNKNOWN,
            identity: FileIdentity::UNKNOWN,
            fingerprint: String::new(),
            changed_at: None,
            physical_overlap: false,
            logical_bytes: None,
            ancestors: vec![],
            allocated_bytes: None,
            newest_activity: None,
            partial: true,
            blocked_reason: Some(
                "This platform has no implemented current-user/no-follow temporary adapter.".into(),
            ),
            contents: vec![TemporaryContentKind::Unknown],
        }
    }
}

pub fn recheck_temporary_unit(
    environment: &PlatformEnvironment,
    reviewed: &TemporaryUnitSnapshot,
    cancel: &AtomicBool,
) -> Result<TemporaryUnitSnapshot, String> {
    if reviewed.blocked_reason.is_some() {
        return Err(reviewed.blocked_reason.clone().unwrap_or_default());
    }
    let current = if let Some(kind) = reviewed.framework {
        snapshot_framework_unit(environment, &reviewed.root, &reviewed.path, kind, cancel)?
    } else {
        snapshot_temporary_unit(environment, &reviewed.root, &reviewed.path, cancel)
    };
    if let Some(reason) = &current.blocked_reason {
        return Err(reason.clone());
    }
    if !current.root_identity.same_entity(reviewed.root_identity)
        || !current.identity.same_entity(reviewed.identity)
        || current.ancestors != reviewed.ancestors
        || current.changed_at != reviewed.changed_at
        || current.fingerprint != reviewed.fingerprint
    {
        return Err("The reviewed target or its contents changed. Stop and scan again.".into());
    }
    Ok(current)
}

/// The caller owns consent. Native mutation first binds the reviewed identity
/// through held descriptors and atomically stages the exact unit. A substituted
/// name is restored rather than moved to Trash; a failed Trash move is restored
/// when the original name remains free. Source paths are never shell arguments.
pub fn move_reviewed_unit(
    environment: &PlatformEnvironment,
    reviewed: &TemporaryUnitSnapshot,
    accept_unknown_usage: bool,
    final_owner_scope: Option<&Path>,
    backend: &dyn crate::trash::TrashBackend,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        unix::move_to_trash(
            environment,
            reviewed,
            accept_unknown_usage,
            final_owner_scope,
            backend,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (
            environment,
            reviewed,
            accept_unknown_usage,
            final_owner_scope,
            backend,
        );
        Err("Temporary-folder removal is unavailable on this platform.".into())
    }
}

/// A bounded no-follow inspection of a known framework subtree. It never
/// authorizes the output parent or evaluates JavaScript configuration.
pub fn verify_framework_generated_tree(
    environment: &PlatformEnvironment,
    path: &Path,
    kind: neati_core::domain::storage::FrameworkGeneratedKind,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if environment.platform() != PlatformKind::Macos {
        return Err("Generated framework cleanup has a macOS use/ownership adapter only.".into());
    }
    #[cfg(unix)]
    {
        unix::verify_framework(environment, path, kind, cancel)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, kind, cancel);
        Err("This host has no implemented generated-framework filesystem adapter.".into())
    }
}

/// Exact typed framework scope, independently of temporary-root facts. The
/// domain planner still owns manifest/config/Git and explicit review consent.
pub fn snapshot_framework_unit(
    environment: &PlatformEnvironment,
    project: &Path,
    path: &Path,
    kind: neati_core::domain::storage::FrameworkGeneratedKind,
    cancel: &AtomicBool,
) -> Result<TemporaryUnitSnapshot, String> {
    if environment.platform() != PlatformKind::Macos || path != project.join(kind.relative()) {
        return Err("Framework removal requires the exact reviewed macOS default scope.".into());
    }
    #[cfg(unix)]
    {
        Ok(unix::snapshot(
            environment,
            project,
            path,
            cancel,
            Some(kind),
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = cancel;
        Err("No framework filesystem adapter on this host.".into())
    }
}

pub fn move_reviewed_framework_unit(
    environment: &PlatformEnvironment,
    reviewed: &TemporaryUnitSnapshot,
    backend: &dyn crate::trash::TrashBackend,
    final_provenance: &dyn Fn(&Path) -> Result<(), String>,
    final_metadata: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    if reviewed.framework.is_none() {
        return Err("A typed framework snapshot is required.".into());
    }
    #[cfg(target_os = "macos")]
    {
        unix::move_to_trash_with_final_check(
            environment,
            reviewed,
            false,
            Some(&reviewed.root),
            backend,
            &observe_with_inspection_handles,
            Some((final_provenance, final_metadata)),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (environment, backend, final_provenance, final_metadata);
        Err("Framework removal is unavailable on this host.".into())
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use sha1::{Digest, Sha1};
    use std::ffi::{CStr, CString};
    use std::fs::File;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    type FinalFrameworkChecks<'a> = (
        &'a dyn Fn(&Path) -> Result<(), String>,
        &'a dyn Fn() -> Result<(), String>,
    );

    fn identity(meta: &std::fs::Metadata) -> FileIdentity {
        FileIdentity::new(meta.dev(), meta.ino())
    }
    fn open_child(parent: &File, name: &std::ffi::OsStr, directory: bool) -> Result<File, String> {
        let name = CString::new(name.as_bytes()).map_err(|_| "Invalid temporary-unit name")?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC
            | if directory { libc::O_DIRECTORY } else { 0 };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(
                "The unit contains an inaccessible entry, link or unsupported special file.".into(),
            );
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn open_root(path: &Path) -> Result<File, String> {
        if !path.is_absolute() {
            return Err("The stated temporary root is not absolute.".into());
        }
        let mut file = File::open("/").map_err(|_| "The temporary root cannot be inspected")?;
        for component in path.components().skip(1) {
            let std::path::Component::Normal(name) = component else {
                return Err("Ambiguous temporary root components are refused.".into());
            };
            file = open_child(&file, name, true)?;
        }
        Ok(file)
    }

    fn names(directory: &File, cap: usize) -> Result<(Vec<std::ffi::OsString>, bool), String> {
        // An independent open-file description resets the directory cursor;
        // dup would share the cursor with an earlier fingerprint traversal.
        let duplicate = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if duplicate < 0 {
            return Err("Could not inspect the temporary directory.".into());
        }
        let stream = unsafe { libc::fdopendir(duplicate) };
        if stream.is_null() {
            unsafe {
                libc::close(duplicate);
            }
            return Err("The temporary directory cannot be read.".into());
        }
        let mut found = Vec::new();
        let mut truncated = false;
        loop {
            #[cfg(target_os = "macos")]
            unsafe {
                *libc::__error() = 0;
            }
            #[cfg(target_os = "linux")]
            unsafe {
                *libc::__errno_location() = 0;
            }
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error().unwrap_or(0) != 0 {
                    unsafe {
                        libc::closedir(stream);
                    }
                    return Err("The temporary directory could not be read completely.".into());
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if found.len() == cap {
                truncated = true;
                break;
            }
            found.push(std::ffi::OsStr::from_bytes(name).to_os_string());
        }
        unsafe {
            libc::closedir(stream);
        }
        found.sort();
        Ok((found, truncated))
    }

    pub(super) fn enumerate(
        environment: &PlatformEnvironment,
    ) -> (Vec<(PathBuf, PathBuf)>, Vec<String>, bool) {
        let mut roots = environment.temporary_roots();
        roots.sort_by_key(|root| root.components().count());
        let mut seen = Vec::<(PathBuf, FileIdentity)>::new();
        let mut units = Vec::new();
        let mut notes = Vec::new();
        let mut partial = false;
        for root in roots {
            if seen.iter().any(|(parent, _)| root.starts_with(parent)) {
                continue;
            }
            let directory = match open_root(&root) {
                Ok(dir) => dir,
                Err(reason) => {
                    notes.push(format!("{}: {reason}", root.display()));
                    partial = true;
                    continue;
                }
            };
            let entity = match directory.metadata() {
                Ok(metadata) => identity(&metadata),
                Err(_) => {
                    notes.push(format!(
                        "{}: root metadata became unavailable",
                        root.display()
                    ));
                    partial = true;
                    continue;
                }
            };
            if seen.iter().any(|(_, other)| other.same_entity(entity)) {
                continue;
            }
            seen.push((root.clone(), entity));
            match names(&directory, MAX_TEMPORARY_UNITS.saturating_sub(units.len())) {
                Ok((children, truncated)) => {
                    partial |= truncated;
                    for name in children {
                        units.push((root.clone(), root.join(name)));
                    }
                }
                Err(reason) => {
                    partial = true;
                    notes.push(format!("{}: {reason}", root.display()));
                }
            }
        }
        if partial {
            notes.push(
                "The bounded temporary inventory is partial; unmeasured locations are not zero."
                    .into(),
            );
        }
        (units, notes, partial)
    }

    struct Tree {
        hash: Sha1,
        entries: usize,
        logical: u64,
        allocated: u64,
        newest: u64,
        source: bool,
        git: bool,
        build: bool,
        session: bool,
        reason: Option<String>,
        started: Instant,
        allocated_entities: std::collections::HashSet<FileIdentity>,
        physical_overlap: bool,
    }
    impl Tree {
        fn new() -> Self {
            Self {
                hash: Sha1::new(),
                entries: 0,
                logical: 0,
                allocated: 0,
                newest: 0,
                source: false,
                git: false,
                build: false,
                session: false,
                reason: None,
                started: Instant::now(),
                allocated_entities: std::collections::HashSet::new(),
                physical_overlap: false,
            }
        }
        fn walk(
            &mut self,
            file: &File,
            relative: &Path,
            depth: usize,
            device: u64,
            uid: u32,
            cancel: &AtomicBool,
        ) {
            if self.reason.is_some() {
                return;
            }
            if cancel.load(Ordering::Relaxed) {
                self.reason =
                    Some("Inspection was cancelled; scan again before reviewing this unit.".into());
                return;
            }
            if depth > MAX_UNIT_DEPTH
                || self.entries >= MAX_UNIT_ENTRIES
                || self.started.elapsed() >= UNIT_TIME
            {
                self.reason = Some("The unit reached its bounded structural inspection limit. Its measured bytes are partial.".into());
                return;
            }
            let meta = match file.metadata() {
                Ok(meta) => meta,
                Err(_) => {
                    self.reason = Some("Entry metadata could not be verified.".into());
                    return;
                }
            };
            if meta.uid() != uid {
                self.reason = Some("The unit contains an entry owned by another user.".into());
                return;
            }
            if meta.dev() != device || (!meta.is_dir() && !meta.is_file()) {
                self.reason = Some(
                    "The unit crosses a filesystem boundary or contains a special file.".into(),
                );
                return;
            }
            if meta.mode() & 0o022 != 0 {
                self.reason = Some("The unit contains entries writable by another user.".into());
                return;
            }
            self.entries += 1;
            self.hash.update(relative.as_os_str().as_bytes());
            self.hash.update([0]);
            for value in [
                meta.dev(),
                meta.ino(),
                meta.mode() as u64,
                meta.uid() as u64,
                meta.nlink(),
                meta.len(),
                meta.mtime() as u64,
                meta.mtime_nsec() as u64,
            ] {
                self.hash.update(value.to_le_bytes());
            }
            // Staging renames the root and changes its ctime. Descendant ctime
            // remains stable and catches edits that restore length and mtime.
            if depth > 0 {
                self.hash.update(meta.ctime().to_le_bytes());
                self.hash.update(meta.ctime_nsec().to_le_bytes());
            }
            self.newest = self
                .newest
                .max(meta.mtime().max(meta.ctime()).max(0) as u64);
            if meta.is_file() {
                self.logical = self.logical.saturating_add(meta.len());
                self.physical_overlap |= meta.nlink() > 1;
                if self.allocated_entities.insert(identity(&meta)) {
                    self.allocated = self
                        .allocated
                        .saturating_add(meta.blocks().saturating_mul(512));
                }
            }
            let name = relative
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            self.git |= name == ".git";
            self.source |= matches!(
                name,
                "Cargo.toml" | "package.json" | "pyproject.toml" | "pom.xml"
            ) || matches!(
                relative.extension().and_then(|s| s.to_str()),
                Some("rs" | "svelte" | "tsx" | "py")
            );
            self.build |= matches!(
                name,
                "target"
                    | "debug"
                    | "release"
                    | "node_modules"
                    | ".next"
                    | ".svelte-kit"
                    | ".fingerprint"
            );
            self.session |= matches!(
                name,
                "Local State"
                    | "Cookies"
                    | "History"
                    | "session.json"
                    | "Session Storage"
                    | "Default"
            );
            if meta.is_dir() {
                let (children, truncated) =
                    match names(file, MAX_UNIT_ENTRIES.saturating_sub(self.entries)) {
                        Ok(names) => names,
                        Err(reason) => {
                            self.reason = Some(reason);
                            return;
                        }
                    };
                if truncated {
                    self.reason = Some(
                        "The unit reached its entry budget; its observed bytes are partial.".into(),
                    );
                    return;
                }
                for name in children {
                    let child = match open_child(file, &name, false) {
                        Ok(file) => file,
                        Err(reason) => {
                            self.reason = Some(reason);
                            break;
                        }
                    };
                    self.walk(&child, &relative.join(name), depth + 1, device, uid, cancel);
                    if self.reason.is_some() {
                        break;
                    }
                }
            }
            if file.metadata().ok().is_none_or(|after| {
                meta.dev() != after.dev()
                    || meta.ino() != after.ino()
                    || meta.mtime() != after.mtime()
                    || meta.mtime_nsec() != after.mtime_nsec()
                    || meta.ctime() != after.ctime()
                    || meta.ctime_nsec() != after.ctime_nsec()
            }) {
                self.reason = Some("The unit changed during inspection. Scan again.".into());
            }
        }
    }

    pub(super) fn verify_framework(
        environment: &PlatformEnvironment,
        path: &Path,
        kind: neati_core::domain::storage::FrameworkGeneratedKind,
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        use neati_core::domain::storage::FrameworkGeneratedKind;
        use std::io::Read;
        if kind.is_whole() {
            return verify_whole_framework(environment, path, kind, cancel);
        }
        let uid = environment
            .current_user_id()
            .ok_or("Current-user ownership could not be established")?;
        let root = open_root(path)?;
        let device = root
            .metadata()
            .map_err(|_| "Framework metadata unavailable")?
            .dev();
        let mut tree = Tree::new();
        tree.walk(&root, Path::new(""), 0, device, uid, cancel);
        if let Some(reason) = tree.reason {
            return Err(reason);
        }
        let fingerprint = format!("{:x}", tree.hash.finalize());
        let mut pending = vec![(root, PathBuf::new())];
        let mut checked = 0usize;
        let mut positive_types = 0usize;
        let started = Instant::now();
        while let Some((directory, relative)) = pending.pop() {
            let (children, truncated) = names(&directory, 8192)?;
            if truncated {
                return Err("The generated-format inspection reached its entry budget.".into());
            }
            for name in children {
                if cancel.load(Ordering::Relaxed)
                    || started.elapsed() > UNIT_TIME
                    || checked >= 8192
                {
                    return Err(
                        "Generated-format inspection was cancelled or exceeded its budget.".into(),
                    );
                }
                let child = open_child(&directory, &name, false)?;
                let metadata = child
                    .metadata()
                    .map_err(|_| "Generated entry unavailable")?;
                let child_relative = relative.join(&name);
                let name = name.to_string_lossy();
                if metadata.is_dir() {
                    if child_relative.components().count() > 32 {
                        return Err(
                            "The generated layout is deeper than the verified contract.".into()
                        );
                    }
                    if kind == FrameworkGeneratedKind::NextWebpackCache
                        && (relative != Path::new("")
                            || ![
                                "client-development",
                                "server-development",
                                "edge-server-development",
                                "client-production",
                                "server-production",
                                "edge-server-production",
                                "client-development-fallback",
                            ]
                            .contains(&name.as_ref()))
                    {
                        return Err(
                            "The Webpack cache layout is unverified; cleanup is unavailable."
                                .into(),
                        );
                    }
                    pending.push((child, child_relative));
                    continue;
                }
                checked += 1;
                if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o111 != 0 {
                    return Err(
                        "Generated cleanup refuses executable, linked or special entries.".into(),
                    );
                }
                let mut file = child;
                match kind {
                    FrameworkGeneratedKind::NextWebpackCache => {
                        let stem = name.strip_suffix(".pack").or_else(|| name.strip_suffix(".pack.old")).ok_or("Compressed or unknown Webpack cache formats remain observation-only")?;
                        if stem != "index" && !stem.bytes().all(|c| c.is_ascii_hexdigit()) {
                            return Err("The Webpack payload name is unverified.".into());
                        }
                        let mut header = [0u8; 8];
                        file.read_exact(&mut header)
                            .map_err(|_| "Webpack cache header is incomplete")?;
                        let count = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
                        if count == 0 || count > 8192 {
                            return Err("The Webpack header exceeds the verified contract.".into());
                        }
                        let mut bytes = header.to_vec();
                        bytes.resize(8 + count * 4, 0);
                        file.read_exact(&mut bytes[8..])
                            .map_err(|_| "Webpack section table is incomplete")?;
                        if !neati_core::domain::storage::supported_webpack_pack(
                            &bytes,
                            metadata.len(),
                        ) {
                            return Err("The Webpack payload is not a verified v1 pack.".into());
                        }
                    }
                    FrameworkGeneratedKind::SvelteKitTypes => {
                        if metadata.len() > 1024 * 1024 {
                            return Err(
                                "The generated type metadata exceeds the supported size.".into()
                            );
                        }
                        if name == "$types.d.ts" {
                            let mut prefix = vec![0u8; metadata.len().min(512) as usize];
                            file.read_exact(&mut prefix)
                                .map_err(|_| "Generated type data is unreadable")?;
                            if !prefix.starts_with(b"import type * as Kit from '@sveltejs/kit';")
                                || !String::from_utf8_lossy(&prefix).contains("type RouteParams")
                            {
                                return Err(
                                    "The SvelteKit type-generation contract is unverified.".into(),
                                );
                            }
                            positive_types += 1;
                        } else if name == "route_meta_data.json" && relative == Path::new("") {
                            let mut bytes = vec![];
                            file.take(1024 * 1024 + 1)
                                .read_to_end(&mut bytes)
                                .map_err(|_| "Generated route metadata unreadable")?;
                            let metadata: serde_json::Value = serde_json::from_slice(&bytes)
                                .map_err(|_| "Generated route metadata is not JSON")?;
                            if !metadata.as_object().is_some_and(|routes| {
                                routes.values().all(|value| {
                                    value.as_array().is_some_and(|entries| {
                                        entries.iter().all(serde_json::Value::is_string)
                                    })
                                })
                            }) {
                                return Err(
                                    "The generated route metadata layout is unknown.".into()
                                );
                            }
                        } else {
                            return Err("Unknown or proxy type layouts remain observed; cleanup is unavailable.".into());
                        }
                    }
                    FrameworkGeneratedKind::SvelteKitOutput
                    | FrameworkGeneratedKind::NextOutput => {
                        unreachable!("whole contracts use their own bounded inspector")
                    }
                }
            }
        }
        if checked == 0 || (kind == FrameworkGeneratedKind::SvelteKitTypes && positive_types == 0) {
            return Err("No positive generated-format evidence was found.".into());
        }
        // Hold the root identity throughout and reject a concurrently changed
        // tree after the bounded positive-format pass.
        let root = open_root(path)?;
        let mut final_tree = Tree::new();
        final_tree.walk(&root, Path::new(""), 0, device, uid, cancel);
        if final_tree.reason.is_some() || format!("{:x}", final_tree.hash.finalize()) != fingerprint
        {
            return Err("Generated contents changed during verification.".into());
        }
        Ok(fingerprint)
    }

    fn verify_whole_framework(
        environment: &PlatformEnvironment,
        path: &Path,
        kind: neati_core::domain::storage::FrameworkGeneratedKind,
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        use neati_core::domain::storage::FrameworkGeneratedKind;
        use std::io::Read;
        let uid = environment
            .current_user_id()
            .ok_or("Current-user identity unavailable")?;
        let root = open_root(path)?;
        let device = root
            .metadata()
            .map_err(|_| "Framework root unreadable")?
            .dev();
        let mut before = Tree::new();
        before.walk(&root, Path::new(""), 0, device, uid, cancel);
        if let Some(reason) = before.reason {
            return Err(reason);
        }
        let fingerprint = format!("{:x}", before.hash.finalize());
        let (children, truncated) = names(&root, 32)?;
        if truncated {
            return Err("Whole framework metadata exceeds its layout budget.".into());
        }
        let mut positive = false;
        for name in children {
            if cancel.load(Ordering::Relaxed) {
                return Err("Framework verification was cancelled.".into());
            }
            let name_text = name
                .to_str()
                .ok_or("Ambiguous framework names are protected")?;
            let child = open_child(&root, &name, false)?;
            let metadata = child.metadata().map_err(|_| "Framework entry unreadable")?;
            if metadata.is_file() && (metadata.nlink() != 1 || metadata.mode() & 0o111 != 0) {
                return Err(
                    "Whole generated outputs refuse executable or hard-linked files.".into(),
                );
            }
            match (kind, name_text, metadata.is_dir()) {
                (FrameworkGeneratedKind::SvelteKitOutput, "types", true) => {
                    verify_framework(environment, &path.join(&name), FrameworkGeneratedKind::SvelteKitTypes, cancel)?;
                    // Whole authority attests every file, including full type
                    // bodies. The existing generated-child contract remains
                    // separate; an unknown route variant cannot expand it into
                    // authority over the parent output.
                    let (entries, overflow) = names(&child, 3)?;
                    if overflow || entries != [std::ffi::OsString::from("route_meta_data.json"), std::ffi::OsString::from("src")] {
                        return Err("This whole sync route layout is unverified.".into());
                    }
                    let src = open_child(&child, std::ffi::OsStr::new("src"), true)?;
                    let routes = open_child(&src, std::ffi::OsStr::new("routes"), true)?;
                    if names(&src, 2)?.0 != [std::ffi::OsString::from("routes")]
                        || names(&routes, 2)?.0 != [std::ffi::OsString::from("$types.d.ts")] {
                        return Err("This whole sync route layout is unverified.".into());
                    }
                    for (directory, filename, expected) in [
                        (&child, "route_meta_data.json", "bd321925d3b07346350ddea68825ccef96a8cf32"),
                        (&routes, "$types.d.ts", "0e4d4cef58b6ddd5d067d35a0b4775ebc550a02d"),
                    ] {
                        let file = open_child(directory, std::ffi::OsStr::new(filename), false)?;
                        if file.metadata().map_err(|_| "Generated route metadata unreadable")?.len() > 1024 * 1024 {
                            return Err("Generated route metadata exceeds its bound.".into());
                        }
                        let mut bytes = Vec::new(); file.take(1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "Generated route metadata unreadable")?;
                        if format!("{:x}", Sha1::digest(&bytes)) != expected {
                            return Err("This complete generated route variant is unverified; the whole output stays observed.".into());
                        }
                    }
                    positive = true;
                }
                (FrameworkGeneratedKind::NextOutput, "cache", true) => {
                    let (cache_entries, overflow) = names(&child, 2)?;
                    if overflow || cache_entries != [std::ffi::OsString::from("webpack")] {
                        return Err("Next data/fetch/image/offline or unknown caches remain protected.".into());
                    }
                    verify_framework(environment, &path.join("cache/webpack"), FrameworkGeneratedKind::NextWebpackCache, cancel)?;
                    positive = true;
                }
                (FrameworkGeneratedKind::NextOutput, "package.json", false) => {
                    if metadata.len() > 4096 { return Err("The generated package marker exceeds its bound.".into()); }
                    let mut bytes = Vec::new();
                    child.take(4097).read_to_end(&mut bytes).map_err(|_| "Generated package marker unreadable")?;
                    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| "Generated package marker is not JSON")?;
                    let value = neati_core::domain::storage::static_framework_config(std::str::from_utf8(&bytes).map_err(|_| "Generated package marker is not UTF-8")?)
                        .ok_or("Unknown Next package marker is protected")?;
                    if value != serde_json::json!({"type":"commonjs"}) { return Err("Unknown Next package marker is protected.".into()); }
                }
                (FrameworkGeneratedKind::SvelteKitOutput, "tsconfig.json", false) => {
                    if metadata.len() > 64 * 1024 { return Err("The generated tsconfig exceeds its bound.".into()); }
                    let mut bytes = Vec::new();
                    child.take(64 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "Generated tsconfig unreadable")?;
                    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| "Generated tsconfig is not JSON")?;
                    let value = neati_core::domain::storage::static_framework_config(std::str::from_utf8(&bytes).map_err(|_| "Generated tsconfig is not UTF-8")?)
                        .ok_or("Unknown or authored tsconfig is protected")?;
                    // The inert config allowlist permits no custom files,
                    // aliases or tsconfig transformer. Check every option and
                    // path from the recorded 2.37.1 default writer.
                    let generated = value == serde_json::json!({
                        "compilerOptions": {
                            "paths": {"$app/types": ["./types/index.d.ts"]},
                            "rootDirs": ["..", "./types"], "verbatimModuleSyntax": true,
                            "isolatedModules": true, "lib": ["esnext", "DOM", "DOM.Iterable"],
                            "moduleResolution": "bundler", "module": "esnext", "noEmit": true, "target": "esnext"
                        },
                        "include": ["ambient.d.ts", "non-ambient.d.ts", "./types/**/$types.d.ts",
                            "../vite.config.js", "../vite.config.ts", "../src/**/*.js", "../src/**/*.ts",
                            "../src/**/*.svelte", "../tests/**/*.js", "../tests/**/*.ts", "../tests/**/*.svelte"],
                        "exclude": ["../node_modules/**", "../src/service-worker.js", "../src/service-worker/**/*.js",
                            "../src/service-worker.ts", "../src/service-worker/**/*.ts", "../src/service-worker.d.ts",
                            "../src/service-worker/**/*.d.ts"]
                    });
                    if !generated { return Err("The generated SvelteKit tsconfig format is unverified.".into()); }
                }
                (FrameworkGeneratedKind::SvelteKitOutput, "ambient.d.ts" | "non-ambient.d.ts", false) => {
                    let mut bytes = Vec::new();
                    child.take(64 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "Generated declarations unreadable")?;
                    let expected = if name_text == "ambient.d.ts" { "59a82452fa3e49b9f33d7a129ee075c50db0a23f" }
                        else { "7d0ac5229014f84b6ae06ce68d8d909fd087348b" };
                    // A banner can hide appended authored content. Only the
                    // complete recorded declaration variant is authority.
                    // Other env/route variants retain their observed bytes.
                    if bytes.len() > 64 * 1024 || format!("{:x}", Sha1::digest(&bytes)) != expected {
                        return Err("This generated declaration variant is unverified; the whole output stays observed.".into());
                    }
                }
                _ => return Err("Deployment, offline, authored or unknown output entries remain protected; choose a verified generated child instead.".into()),
            }
        }
        if !positive {
            return Err("A whole default output needs positive generated-tree evidence.".into());
        }
        let rebound = open_root(path)?;
        let mut after = Tree::new();
        after.walk(&rebound, Path::new(""), 0, device, uid, cancel);
        if after.reason.is_some() || format!("{:x}", after.hash.finalize()) != fingerprint {
            return Err("The whole framework output changed during verification.".into());
        }
        Ok(fingerprint)
    }

    pub(super) fn snapshot(
        environment: &PlatformEnvironment,
        root: &Path,
        path: &Path,
        cancel: &AtomicBool,
        framework: Option<neati_core::domain::storage::FrameworkGeneratedKind>,
    ) -> TemporaryUnitSnapshot {
        let mut result = TemporaryUnitSnapshot {
            framework,
            root: root.into(),
            path: path.into(),
            root_identity: FileIdentity::UNKNOWN,
            identity: FileIdentity::UNKNOWN,
            ancestors: vec![],
            fingerprint: String::new(),
            changed_at: None,
            physical_overlap: false,
            logical_bytes: None,
            allocated_bytes: None,
            newest_activity: None,
            partial: true,
            blocked_reason: None,
            contents: vec![],
        };
        let observed = (|| -> Result<(File, File, u32), String> {
            if (framework.is_none()
                && !environment
                    .temporary_roots()
                    .iter()
                    .any(|stated| root == stated))
                || path == root
                || !path.starts_with(root)
            {
                return Err("The unit is outside the exact stated temporary roots.".into());
            }
            let uid = environment
                .current_user_id()
                .ok_or("The current-user ownership identity is unavailable.")?;
            let parent = open_root(root)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "The unit is outside its root")?;
            let mut cursor = parent
                .try_clone()
                .map_err(|_| "The root descriptor could not be held")?;
            let mut ancestors = vec![];
            let components = relative.components().collect::<Vec<_>>();
            for (index, component) in components.iter().enumerate() {
                let std::path::Component::Normal(name) = component else {
                    return Err("Ambiguous temporary-unit components are refused.".into());
                };
                cursor = open_child(&cursor, name, index + 1 < components.len())?;
                if index + 1 < components.len() {
                    let meta = cursor
                        .metadata()
                        .map_err(|_| "The reviewed ancestor could not be inspected")?;
                    if meta.uid() != uid || meta.mode() & 0o022 != 0 {
                        return Err("A temporary-unit ancestor is foreign-owned or writable by another user.".into());
                    }
                    ancestors.push(identity(&meta));
                }
            }
            result.ancestors = ancestors;
            let child = cursor;
            Ok((parent, child, uid))
        })();
        match observed {
            Err(reason) => result.blocked_reason = Some(reason),
            Ok((parent, child, uid)) => {
                let (Ok(parent_meta), Ok(child_meta)) = (parent.metadata(), child.metadata())
                else {
                    result.blocked_reason = Some("Held unit metadata became unavailable.".into());
                    result.contents.push(TemporaryContentKind::Unknown);
                    return result;
                };
                result.root_identity = identity(&parent_meta);
                result.identity = identity(&child_meta);
                result.changed_at = Some((child_meta.ctime(), child_meta.ctime_nsec()));
                let mut tree = Tree::new();
                tree.walk(&child, Path::new(""), 0, parent_meta.dev(), uid, cancel);
                result.logical_bytes = (tree.entries > 0).then_some(tree.logical);
                result.allocated_bytes = (tree.entries > 0).then_some(tree.allocated);
                result.physical_overlap = tree.physical_overlap;
                result.newest_activity = (tree.newest != 0).then_some(tree.newest);
                result.partial = tree.reason.is_some();
                result.blocked_reason = tree.reason;
                result.fingerprint = format!("{:x}", tree.hash.finalize());
                if tree.build {
                    result.contents.push(TemporaryContentKind::BuildOutput);
                }
                if tree.source {
                    result.contents.push(TemporaryContentKind::SourceCheckout);
                }
                if tree.git {
                    result.contents.push(TemporaryContentKind::GitMetadata);
                }
                if tree.session {
                    result
                        .contents
                        .push(TemporaryContentKind::BrowserOrSessionData);
                }
            }
        }
        if result.contents.is_empty() {
            result.contents.push(TemporaryContentKind::Unknown);
        }
        result
    }

    #[cfg(target_os = "macos")]
    fn bind_original_locator(
        reviewed: &TemporaryUnitSnapshot,
        held_root: &File,
        held_parent: &File,
        uid: u32,
    ) -> Result<File, String> {
        let root = open_root(&reviewed.root)?;
        let entity = identity(
            &root
                .metadata()
                .map_err(|_| "Temporary root became unreadable")?,
        );
        if !entity.same_entity(reviewed.root_identity)
            || !entity.same_entity(identity(
                &held_root
                    .metadata()
                    .map_err(|_| "Held root became unreadable")?,
            ))
        {
            return Err("The temporary root pathname changed at the final boundary.".into());
        }
        let parent_path = reviewed
            .path
            .parent()
            .ok_or("Temporary roots are never targets")?;
        let relative = parent_path
            .strip_prefix(&reviewed.root)
            .map_err(|_| "The reviewed parent left its root")?;
        let mut cursor = root
            .try_clone()
            .map_err(|_| "The temporary root could not be rebound")?;
        let mut count = 0;
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err("Ambiguous reviewed parent components are refused.".into());
            };
            cursor = open_child(&cursor, name, true)?;
            let metadata = cursor
                .metadata()
                .map_err(|_| "A reviewed ancestor became unreadable")?;
            if reviewed.ancestors.get(count) != Some(&identity(&metadata))
                || metadata.uid() != uid
                || metadata.mode() & 0o022 != 0
            {
                return Err("A reviewed ancestor pathname changed at the final boundary.".into());
            }
            count += 1;
        }
        if count != reviewed.ancestors.len()
            || !identity(
                &cursor
                    .metadata()
                    .map_err(|_| "Reviewed parent unavailable")?,
            )
            .same_entity(identity(
                &held_parent
                    .metadata()
                    .map_err(|_| "Held parent unavailable")?,
            ))
        {
            return Err("The reviewed parent changed at the final boundary.".into());
        }
        Ok(root)
    }

    #[cfg(target_os = "macos")]
    fn remove_empty_stage(root: &File, name: &std::ffi::OsStr, stage: &File) -> bool {
        let Ok(current) = open_child(root, name, true) else {
            return false;
        };
        let (Ok(current), Ok(held)) = (current.metadata(), stage.metadata()) else {
            return false;
        };
        if !identity(&current).same_entity(identity(&held)) {
            return false;
        }
        let Ok(name) = CString::new(name.as_bytes()) else {
            return false;
        };
        // Only the held root's verified, empty stage may be removed. Never
        // resolve a raw absolute path that a concurrent actor could replace.
        unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) == 0 }
    }

    #[cfg(target_os = "macos")]
    fn descriptor_path(file: &File) -> Option<PathBuf> {
        let mut bytes = [0u8; libc::PATH_MAX as usize];
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, bytes.as_mut_ptr()) } != 0 {
            return None;
        }
        let end = bytes.iter().position(|byte| *byte == 0)?;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end])))
    }

    #[cfg(target_os = "macos")]
    fn restore_reviewed_object(
        stage: &File,
        parent: &File,
        destination: &CStr,
        reviewed: FileIdentity,
    ) -> bool {
        // If the staged leaf was renamed/replaced, restore only the accepted
        // identity. A substitute must never be moved to the original path.
        let Ok((children, truncated)) = names(stage, 512) else {
            return false;
        };
        if truncated {
            return false;
        }
        let mut accepted_name = None;
        for name in children {
            let Ok(file) = open_child(stage, &name, false) else {
                continue;
            };
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            if !identity(&metadata).same_entity(reviewed) {
                continue;
            }
            if accepted_name.is_some() {
                return false;
            }
            accepted_name = CString::new(name.as_bytes()).ok();
        }
        accepted_name.is_some_and(|name| unsafe {
            libc::renameatx_np(
                stage.as_raw_fd(),
                name.as_ptr(),
                parent.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_EXCL,
            ) == 0
        })
    }

    #[cfg(target_os = "macos")]
    pub(super) fn move_to_trash(
        environment: &PlatformEnvironment,
        reviewed: &TemporaryUnitSnapshot,
        accept_unknown_usage: bool,
        final_owner_scope: Option<&Path>,
        backend: &dyn crate::trash::TrashBackend,
    ) -> Result<(), String> {
        move_to_trash_with_probe(
            environment,
            reviewed,
            accept_unknown_usage,
            final_owner_scope,
            backend,
            &observe_with_inspection_handles,
        )
    }

    #[cfg(target_os = "macos")]
    pub(super) fn move_to_trash_with_probe(
        environment: &PlatformEnvironment,
        reviewed: &TemporaryUnitSnapshot,
        accept_unknown_usage: bool,
        final_owner_scope: Option<&Path>,
        backend: &dyn crate::trash::TrashBackend,
        final_probe: &dyn Fn(&PlatformEnvironment, &Path, &[i32]) -> TemporaryUsageObservation,
    ) -> Result<(), String> {
        move_to_trash_with_final_check(
            environment,
            reviewed,
            accept_unknown_usage,
            final_owner_scope,
            backend,
            final_probe,
            None,
        )
    }

    #[cfg(target_os = "macos")]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn move_to_trash_with_final_check(
        environment: &PlatformEnvironment,
        reviewed: &TemporaryUnitSnapshot,
        accept_unknown_usage: bool,
        final_owner_scope: Option<&Path>,
        backend: &dyn crate::trash::TrashBackend,
        final_probe: &dyn Fn(&PlatformEnvironment, &Path, &[i32]) -> TemporaryUsageObservation,
        final_provenance: Option<FinalFrameworkChecks<'_>>,
    ) -> Result<(), String> {
        if final_owner_scope.is_some_and(|scope| {
            (scope == reviewed.root && reviewed.framework.is_none())
                || scope == reviewed.path
                || !scope.starts_with(&reviewed.root)
                || !reviewed.path.starts_with(scope)
        }) {
            return Err("The generated owner-use scope is outside its reviewed ancestry.".into());
        }
        let uid = environment
            .current_user_id()
            .ok_or("Current user unavailable")?;
        recheck_temporary_unit(environment, reviewed, &AtomicBool::new(false))?;
        let root = open_root(&reviewed.root)?;
        let parent = open_root(
            reviewed
                .path
                .parent()
                .ok_or("The temporary root itself is never a target")?,
        )?;
        if !identity(
            &root
                .metadata()
                .map_err(|_| "Temporary root became unreadable")?,
        )
        .same_entity(reviewed.root_identity)
        {
            return Err("The temporary root changed after review.".into());
        }
        bind_original_locator(reviewed, &root, &parent, uid)?;
        let stage_name = CString::new(format!(".neati-reviewed-{}", uuid::Uuid::new_v4())).unwrap();
        if unsafe { libc::mkdirat(root.as_raw_fd(), stage_name.as_ptr(), 0o700) } != 0 {
            return Err("Could not create the private same-volume Trash staging directory.".into());
        }
        let stage_path = reviewed
            .root
            .join(std::ffi::OsStr::from_bytes(stage_name.as_bytes()));
        let stage = open_child(
            &root,
            std::ffi::OsStr::from_bytes(stage_name.as_bytes()),
            true,
        )?;
        let name = CString::new(reviewed.path.file_name().unwrap().as_bytes()).unwrap();
        let moved = unsafe {
            libc::renameatx_np(
                parent.as_raw_fd(),
                name.as_ptr(),
                stage.as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if moved != 0 {
            remove_empty_stage(
                &root,
                std::ffi::OsStr::from_bytes(stage_name.as_bytes()),
                &stage,
            );
            return Err(
                "The temporary unit could not be staged without overwriting another entry.".into(),
            );
        }
        let staged_path = stage_path.join(reviewed.path.file_name().unwrap());
        let outcome = (|| -> Result<(), String> {
            let held = open_child(&stage, reviewed.path.file_name().unwrap(), false)?;
            let meta = held
                .metadata()
                .map_err(|_| "The staged unit could not be verified")?;
            if !identity(&meta).same_entity(reviewed.identity) {
                return Err("The temporary target changed at the mutation boundary.".into());
            }
            let mut tree = Tree::new();
            tree.walk(
                &held,
                Path::new(""),
                0,
                reviewed.identity.device(),
                environment
                    .current_user_id()
                    .ok_or("Current user unavailable")?,
                &AtomicBool::new(false),
            );
            if tree.reason.is_some()
                || format!("{:x}", tree.hash.finalize()) != reviewed.fingerprint
            {
                return Err(
                    "The temporary unit changed at the mutation boundary. Scan again.".into(),
                );
            }
            // Close our measurement handle before lsof; our own held file or
            // directory must not manufacture an active-owner observation.
            drop(held);
            if let Some((check, _)) = final_provenance {
                check(&staged_path)?;
            }
            let inspection_fds = [root.as_raw_fd(), parent.as_raw_fd(), stage.as_raw_fd()];
            if let Some(scope) = final_owner_scope {
                // Generated scopes retain their complete project-use policy
                // through the final boundary. #397 unknown consent cannot
                // authorize this narrower generated-only operation.
                let owner_use = final_probe(environment, scope, &inspection_fds);
                if owner_use.state != TemporaryUsageState::NoUseDetected {
                    return Err(format!("Generated project use is active or unknown at the final boundary; the unit was kept. {}", owner_use.evidence.join(" ")));
                }
            }
            let usage = final_probe(environment, &staged_path, &inspection_fds);
            if usage.state == TemporaryUsageState::InUse {
                return Err("Active use was newly detected. Stop the owner and scan again.".into());
            }
            if usage.state == TemporaryUsageState::UnableToDetermine
                && (!accept_unknown_usage || final_owner_scope.is_some())
            {
                return Err(
                    "Use could not be determined at the final check. Review the uncertainty again."
                        .into(),
                );
            }
            let final_held = open_child(&stage, reviewed.path.file_name().unwrap(), false)?;
            let mut final_tree = Tree::new();
            final_tree.walk(
                &final_held,
                Path::new(""),
                0,
                reviewed.identity.device(),
                environment
                    .current_user_id()
                    .ok_or("Current user unavailable")?,
                &AtomicBool::new(false),
            );
            if final_tree.reason.is_some()
                || format!("{:x}", final_tree.hash.finalize()) != reviewed.fingerprint
            {
                return Err(
                    "The staged contents changed during the final use check. Scan again.".into(),
                );
            }
            if let Some((_, check)) = final_provenance {
                check()?;
            }
            let rebound_root = bind_original_locator(reviewed, &root, &parent, uid)?;
            let rebound_stage = open_child(
                &rebound_root,
                std::ffi::OsStr::from_bytes(stage_name.as_bytes()),
                true,
            )?;
            let current_stage = rebound_stage
                .metadata()
                .map_err(|_| "Staging metadata became unreadable")?;
            let held_stage = stage
                .metadata()
                .map_err(|_| "Held staging metadata became unreadable")?;
            if !identity(&current_stage).same_entity(identity(&held_stage))
                || current_stage.uid() != uid
                || current_stage.mode() & 0o077 != 0
            {
                return Err("The private staging pathname changed at the final boundary.".into());
            }
            let rebound_leaf =
                open_child(&rebound_stage, reviewed.path.file_name().unwrap(), false)?;
            if !identity(
                &rebound_leaf
                    .metadata()
                    .map_err(|_| "Staged leaf became unreadable")?,
            )
            .same_entity(reviewed.identity)
            {
                return Err("The staged leaf pathname changed at the final boundary.".into());
            }
            backend.move_to_trash(&staged_path)
        })();
        if let Err(reason) = outcome {
            let restored = restore_reviewed_object(&stage, &parent, &name, reviewed.identity);
            let recovery_parent =
                descriptor_path(&parent).unwrap_or_else(|| reviewed.path.parent().unwrap().into());
            let recovery_stage = descriptor_path(&stage).unwrap_or(stage_path.clone());
            let removed_stage = remove_empty_stage(
                &root,
                std::ffi::OsStr::from_bytes(stage_name.as_bytes()),
                &stage,
            );
            return Err(if restored {
                format!(
                    "{reason} The reviewed unit was kept at {}.{}",
                    recovery_parent
                        .join(reviewed.path.file_name().unwrap())
                        .display(),
                    if removed_stage {
                        String::new()
                    } else {
                        format!(
                            " Unverified staging entries were preserved at {}.",
                            recovery_stage.display()
                        )
                    }
                )
            } else {
                format!("{reason} The original name is occupied or the accepted object was renamed; inspect {} and {} to restore it manually. No substitute was moved.", recovery_parent.display(), recovery_stage.display())
            });
        }
        remove_empty_stage(
            &root,
            std::ffi::OsStr::from_bytes(stage_name.as_bytes()),
            &stage,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_states_preserve_positive_partial_and_negative_observations() {
        assert_eq!(
            classify_usage(Some(1), b"", b"").state,
            TemporaryUsageState::NoUseDetected
        );
        assert_eq!(
            classify_usage(Some(0), b"p42\ncnode\nfcwd\nn/fixture\n", b"").state,
            TemporaryUsageState::InUse
        );
        assert_eq!(
            classify_usage(
                Some(0),
                b"p42\ncnode\nf3\nn/fixture/.cargo-lock\n",
                b"partial"
            )
            .state,
            TemporaryUsageState::InUse
        );
        assert_eq!(
            classify_usage(Some(1), b"", b"permission denied").state,
            TemporaryUsageState::UnableToDetermine
        );
        assert!(classify_usage(Some(1), b"", b"")
            .limitation
            .contains("not proof"));
    }

    #[test]
    fn unsupported_platform_reports_unknown_use_without_inspecting_real_roots() {
        let env = PlatformEnvironment::simulated(crate::PathFlavor::Windows)
            .with_platform(PlatformKind::Windows);
        let observation = observe_temporary_use(&env, Path::new("C:\\fixture\\unit"));
        assert_eq!(observation.state, TemporaryUsageState::UnableToDetermine);
        assert!(observation.evidence[0].contains("no implemented"));
    }

    #[test]
    fn final_probe_excludes_only_its_exact_owned_inspection_fds() {
        let own = b"p42\ncNeati\nf6\nn/fixture/project\n";
        for code in [0, 1] {
            assert_eq!(
                classify_usage_with_inspection_handles(Some(code), own, b"", Some(42), &[6]).state,
                TemporaryUsageState::NoUseDetected
            );
        }
        assert_eq!(
            classify_usage_with_inspection_handles(
                Some(0),
                own,
                b"permission denied",
                Some(42),
                &[6]
            )
            .state,
            TemporaryUsageState::UnableToDetermine
        );
        assert_eq!(
            classify_usage_with_inspection_handles(
                Some(0),
                b"p42\ncNeati\nfcwd\nn/fixture/project\n",
                b"",
                Some(42),
                &[6]
            )
            .state,
            TemporaryUsageState::InUse
        );
        assert_eq!(
            classify_usage_with_inspection_handles(Some(0), own, b"", Some(42), &[7]).state,
            TemporaryUsageState::InUse
        );
        assert_eq!(
            classify_usage_with_inspection_handles(
                Some(0),
                b"p43\ncowner\nf6\nn/fixture/project\n",
                b"",
                Some(42),
                &[6]
            )
            .state,
            TemporaryUsageState::InUse
        );
        for malformed in [
            b"p42\ncNeati\nf6\nn/fixture/project\nunknown-field\n".as_slice(),
            b"p42\ncNeati\nf6\nn/fixture/project\nf7\n".as_slice(),
            b"p42\ncNeati\nf6\nn/fixture/project\np43\ncowner\n".as_slice(),
            b"p42\nf6\nn/fixture/project\n".as_slice(),
            b"p42\ncNeati\nf6\nn/fixture/\xff\n".as_slice(),
            b"p42\ncNeati\nf6\nn/fixture/project".as_slice(),
        ] {
            assert_eq!(
                classify_usage_with_inspection_handles(Some(1), malformed, b"", Some(42), &[6])
                    .state,
                TemporaryUsageState::UnableToDetermine,
                "an incomplete field stream cannot establish idle"
            );
        }
        assert_eq!(
            classify_usage_with_inspection_handles(
                Some(1),
                b"p42\ncNeati\nf6invalid\nn/fixture/project\n",
                b"",
                Some(42),
                &[6]
            )
            .state,
            TemporaryUsageState::InUse
        );
        assert_eq!(
            classify_usage_with_inspection_handles(
                Some(1),
                b"p42\ncNeati\nf6r\nn/fixture/project\nf7\nn/fixture/project/.next\n",
                b"",
                Some(42),
                &[6, 7]
            )
            .state,
            TemporaryUsageState::NoUseDetected
        );
    }

    #[cfg(unix)]
    fn fixture_environment(root: &Path) -> PlatformEnvironment {
        PlatformEnvironment::simulated(crate::PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_temp_dir(root)
            .with_current_user_id(unsafe { libc::geteuid() })
    }

    #[cfg(unix)]
    #[test]
    fn temporary_boundaries_refuse_roots_links_foreign_ownership_and_cancelled_estimates() {
        use std::os::unix::fs::symlink;
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let unit = root.join("unclassified");
        std::fs::create_dir(&unit).unwrap();
        std::fs::write(unit.join("payload"), b"fixture").unwrap();
        let env = fixture_environment(&root);
        let cancel = AtomicBool::new(false);
        assert!(snapshot_temporary_unit(&env, &root, &root, &cancel)
            .blocked_reason
            .is_some());
        let clean = snapshot_temporary_unit(&env, &root, &unit, &cancel);
        assert!(clean.blocked_reason.is_none());
        assert_eq!(clean.logical_bytes, Some(7));
        let foreign = env
            .clone()
            .with_current_user_id(unsafe { libc::geteuid() }.wrapping_add(1));
        let blocked = snapshot_temporary_unit(&foreign, &root, &unit, &cancel);
        assert!(blocked.blocked_reason.unwrap().contains("another user"));
        assert_eq!(blocked.allocated_bytes, None);
        let cancelled = snapshot_temporary_unit(&env, &root, &unit, &AtomicBool::new(true));
        assert!(cancelled.partial);
        assert_eq!(cancelled.allocated_bytes, None);
        symlink(fixture.path().join("outside"), unit.join("link")).unwrap();
        assert!(snapshot_temporary_unit(&env, &root, &unit, &cancel)
            .blocked_reason
            .is_some());
        assert!(recheck_temporary_unit(&env, &clean, &cancel).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_measured_once_with_explicit_physical_uncertainty() {
        use std::os::unix::fs::MetadataExt;
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let unit = root.join("unit");
        std::fs::create_dir(&unit).unwrap();
        std::fs::write(unit.join("one"), vec![1; 8192]).unwrap();
        std::fs::hard_link(unit.join("one"), unit.join("two")).unwrap();
        let snapshot = snapshot_temporary_unit(
            &fixture_environment(&root),
            &root,
            &unit,
            &AtomicBool::new(false),
        );
        assert!(snapshot.physical_overlap);
        assert!(snapshot.blocked_reason.is_none());
        assert_eq!(snapshot.logical_bytes, Some(16384));
        assert_eq!(
            snapshot.allocated_bytes,
            Some(std::fs::metadata(unit.join("one")).unwrap().blocks() * 512)
        );
    }

    #[cfg(unix)]
    #[test]
    fn positive_framework_contracts_preserve_unknown_executable_and_proxy_layouts() {
        use neati_core::domain::storage::FrameworkGeneratedKind;
        use std::os::unix::fs::PermissionsExt;
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let env = fixture_environment(&root);
        let cancel = AtomicBool::new(false);
        let webpack = root.join(".next/cache/webpack");
        let cache = webpack.join("client-development");
        std::fs::create_dir_all(&cache).unwrap();
        let mut pack = 0x0163_7077u32.to_le_bytes().to_vec();
        pack.extend(1u32.to_le_bytes());
        pack.extend(4i32.to_le_bytes());
        pack.extend([1, 2, 3, 4]);
        std::fs::write(cache.join("index.pack"), &pack).unwrap();
        let first = verify_framework_generated_tree(
            &env,
            &webpack,
            FrameworkGeneratedKind::NextWebpackCache,
            &cancel,
        )
        .unwrap();
        let second = verify_framework_generated_tree(
            &env,
            &webpack,
            FrameworkGeneratedKind::NextWebpackCache,
            &cancel,
        )
        .unwrap();
        assert_eq!(first, second); // Repeated traversal must reset descriptor cursors.
        std::fs::write(cache.join("unknown.pack.gz"), [1]).unwrap();
        assert!(verify_framework_generated_tree(
            &env,
            &webpack,
            FrameworkGeneratedKind::NextWebpackCache,
            &cancel
        )
        .is_err());
        std::fs::remove_file(cache.join("unknown.pack.gz")).unwrap();
        std::fs::set_permissions(
            cache.join("index.pack"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(verify_framework_generated_tree(
            &env,
            &webpack,
            FrameworkGeneratedKind::NextWebpackCache,
            &cancel
        )
        .is_err());
        let types = root.join(".svelte-kit/types");
        let route = types.join("src/routes");
        std::fs::create_dir_all(&route).unwrap();
        std::fs::write(
            route.join("$types.d.ts"),
            "import type * as Kit from '@sveltejs/kit';\ntype RouteParams = {};\n",
        )
        .unwrap();
        std::fs::write(
            types.join("route_meta_data.json"),
            r#"{"/":["src/routes/+page.svelte"]}"#,
        )
        .unwrap();
        assert!(verify_framework_generated_tree(
            &env,
            &types,
            FrameworkGeneratedKind::SvelteKitTypes,
            &cancel
        )
        .is_ok());
        std::fs::write(route.join("proxy+page.js"), b"unknown proxy format").unwrap();
        assert!(verify_framework_generated_tree(
            &env,
            &types,
            FrameworkGeneratedKind::SvelteKitTypes,
            &cancel
        )
        .is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn framework_final_project_use_and_metadata_changes_restore_the_whole_accepted_unit() {
        use neati_core::domain::storage::FrameworkGeneratedKind;
        for scenario in ["active", "unknown", "marker"] {
            let fixture = tempfile::tempdir().unwrap();
            let base = fixture.path().canonicalize().unwrap();
            let project = base.join("project");
            let target = project.join(".next");
            let cache = target.join("cache/webpack/client-development");
            std::fs::create_dir_all(&cache).unwrap();
            let mut pack = 0x0163_7077u32.to_le_bytes().to_vec();
            pack.extend(1u32.to_le_bytes());
            pack.extend(4i32.to_le_bytes());
            pack.extend([1, 2, 3, 4]);
            std::fs::write(cache.join("index.pack"), &pack).unwrap();
            std::fs::write(
                project.join("package.json"),
                r#"{"dependencies":{"next":"15.5.14"}}"#,
            )
            .unwrap();
            let env = fixture_environment(&base);
            let reviewed = snapshot_framework_unit(
                &env,
                &project,
                &target,
                FrameworkGeneratedKind::NextOutput,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(reviewed.blocked_reason.is_none());
            let expected =
                crate::framework_metadata::observe(&project, FrameworkGeneratedKind::NextOutput)
                    .unwrap()
                    .fingerprint;
            let backend = crate::MockTrashBackend::new();
            let project_probed = AtomicBool::new(false);
            let check_metadata = || {
                let current = crate::framework_metadata::observe(
                    &project,
                    FrameworkGeneratedKind::NextOutput,
                )?;
                if current.fingerprint != expected {
                    return Err("Metadata changed during final use".into());
                }
                Ok(())
            };
            let result = unix::move_to_trash_with_final_check(
                &env,
                &reviewed,
                false,
                Some(&project),
                &backend,
                &|_, scope, _| {
                    if scope != project {
                        assert!(project_probed.load(Ordering::Relaxed));
                        assert_eq!(
                            scope.parent().and_then(Path::parent),
                            Some(project.as_path())
                        );
                        return classify_usage(Some(1), b"", b"");
                    }
                    assert_eq!(
                        scope, project,
                        "the final use scope covers the entire project"
                    );
                    project_probed.store(true, Ordering::Relaxed);
                    match scenario {
                        "active" => classify_usage(Some(0), b"p42\ncnode\nfcwd\nn/project\n", b""),
                        "unknown" => unknown_usage("incomplete fixture owner evidence"),
                        "marker" => {
                            std::fs::write(
                                project.join("next.config.js"),
                                "export default {\"distDir\":\"custom\"};",
                            )
                            .unwrap();
                            classify_usage(Some(1), b"", b"")
                        }
                        _ => unreachable!(),
                    }
                },
                Some((&|_| Ok(()), &check_metadata)),
            );
            assert!(result.is_err(), "{scenario}");
            assert!(project_probed.load(Ordering::Relaxed));
            assert!(backend.moved().is_empty(), "{scenario}");
            assert_eq!(
                std::fs::read(cache.join("index.pack")).unwrap(),
                pack,
                "{scenario}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_timestamp_restored_generated_edit_revokes_the_private_snapshot() {
        use neati_core::domain::storage::FrameworkGeneratedKind;
        use std::fs::{File, FileTimes};
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let project = base.join("project");
        let target = project.join(".next");
        std::fs::create_dir_all(&target).unwrap();
        let payload = target.join("payload");
        std::fs::write(&payload, b"original").unwrap();
        let env = fixture_environment(&base);
        let reviewed = snapshot_framework_unit(
            &env,
            &project,
            &target,
            FrameworkGeneratedKind::NextOutput,
            &AtomicBool::new(false),
        )
        .unwrap();
        let modified = std::fs::metadata(&payload).unwrap().modified().unwrap();
        std::fs::write(&payload, b"tampered").unwrap();
        File::open(&payload)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            std::fs::metadata(&payload).unwrap().modified().unwrap(),
            modified
        );
        assert!(
            recheck_temporary_unit(&env, &reviewed, &AtomicBool::new(false))
                .unwrap_err()
                .contains("changed")
        );
        assert_eq!(std::fs::read(&payload).unwrap(), b"tampered");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn descriptor_staging_uses_fixture_trash_and_restores_failed_moves() {
        struct FixtureTrash {
            destination: PathBuf,
            fail: bool,
        }
        impl crate::TrashBackend for FixtureTrash {
            fn move_to_trash(&self, path: &Path) -> Result<(), String> {
                assert!(path
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".neati-reviewed-"));
                if self.fail {
                    return Err("Fixture Trash failure".into());
                }
                std::fs::rename(path, self.destination.join(path.file_name().unwrap()))
                    .map_err(|error| error.to_string())
            }
        }
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let root = base.join("temporary");
        let trash = base.join("fixture-trash");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&trash).unwrap();
        let unit = root.join("whole-worktree");
        std::fs::create_dir(&unit).unwrap();
        std::fs::write(unit.join("source.rs"), b"unpublished fixture").unwrap();
        let env = fixture_environment(&root);
        let opened = std::fs::File::open(unit.join("source.rs")).unwrap();
        let reviewed = snapshot_temporary_unit(&env, &root, &unit, &AtomicBool::new(false));
        assert!(move_reviewed_unit(
            &env,
            &reviewed,
            true,
            None,
            &FixtureTrash {
                destination: trash.clone(),
                fail: false
            }
        )
        .is_err());
        assert!(unit.join("source.rs").exists());
        drop(opened);
        let reviewed = snapshot_temporary_unit(&env, &root, &unit, &AtomicBool::new(false));
        assert!(move_reviewed_unit(
            &env,
            &reviewed,
            true,
            None,
            &FixtureTrash {
                destination: trash.clone(),
                fail: true
            }
        )
        .is_err());
        assert!(unit.join("source.rs").exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let reviewed = snapshot_temporary_unit(&env, &root, &unit, &AtomicBool::new(false));
        move_reviewed_unit(
            &env,
            &reviewed,
            true,
            None,
            &FixtureTrash {
                destination: trash.clone(),
                fail: false,
            },
        )
        .unwrap();
        assert!(trash.join("whole-worktree/source.rs").exists());
        assert!(!unit.exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn final_probe_path_replacements_never_reach_trash_or_remove_substitutes() {
        for replacement in ["root", "stage", "ancestor", "leaf"] {
            let fixture = tempfile::tempdir().unwrap();
            let base = fixture.path().canonicalize().unwrap();
            let root = base.join("temporary");
            let project = root.join("project");
            let unit = if replacement == "ancestor" {
                project.join("target")
            } else {
                root.join("unit")
            };
            std::fs::create_dir_all(&unit).unwrap();
            std::fs::write(unit.join("payload"), b"accepted fixture").unwrap();
            let environment = fixture_environment(&root);
            let reviewed =
                snapshot_temporary_unit(&environment, &root, &unit, &AtomicBool::new(false));
            assert!(reviewed.blocked_reason.is_none());
            let backend = crate::MockTrashBackend::new();
            let changed = AtomicBool::new(false);
            let substitute_stage = std::cell::RefCell::new(None::<PathBuf>);
            let probe = |_: &PlatformEnvironment, staged: &Path, _: &[i32]| {
                if !changed.swap(true, Ordering::SeqCst) {
                    match replacement {
                        "root" => {
                            std::fs::rename(&root, base.join("held-root")).unwrap();
                            std::fs::create_dir(&root).unwrap();
                            std::fs::create_dir(root.join("unit")).unwrap();
                            std::fs::write(root.join("unit/payload"), b"substitute").unwrap();
                            let fake_stage = staged.parent().unwrap();
                            std::fs::create_dir(fake_stage).unwrap();
                            *substitute_stage.borrow_mut() = Some(fake_stage.into());
                        }
                        "stage" => {
                            let original_stage = staged.parent().unwrap();
                            std::fs::rename(original_stage, root.join("held-stage")).unwrap();
                            std::fs::create_dir(original_stage).unwrap();
                            *substitute_stage.borrow_mut() = Some(original_stage.into());
                        }
                        "ancestor" => {
                            std::fs::rename(&project, root.join("held-project")).unwrap();
                            std::fs::create_dir_all(&unit).unwrap();
                            std::fs::write(unit.join("payload"), b"substitute").unwrap();
                        }
                        "leaf" => {
                            std::fs::rename(
                                staged,
                                staged.parent().unwrap().join("accepted-original"),
                            )
                            .unwrap();
                            std::fs::create_dir(staged).unwrap();
                            std::fs::write(staged.join("payload"), b"substitute").unwrap();
                            *substitute_stage.borrow_mut() = Some(staged.into());
                        }
                        _ => panic!("Unexpected fixture replacement"),
                    }
                }
                let mut observation = unknown_usage("Completed fixture probe");
                observation.state = TemporaryUsageState::NoUseDetected;
                observation
            };
            let outcome = unix::move_to_trash_with_probe(
                &environment,
                &reviewed,
                true,
                (replacement == "ancestor").then_some(project.as_path()),
                &backend,
                &probe,
            );
            assert!(outcome.is_err(), "{replacement}");
            assert!(backend.moved().is_empty(), "{replacement}");
            let accepted = match replacement {
                "root" => base.join("held-root/unit"),
                "ancestor" => root.join("held-project/target"),
                _ => unit.clone(),
            };
            assert_eq!(
                std::fs::read(accepted.join("payload")).unwrap(),
                b"accepted fixture",
                "{replacement}"
            );
            if let Some(substitute) = substitute_stage.borrow().as_ref() {
                assert!(
                    substitute.exists(),
                    "The substitute must remain: {replacement}"
                );
            }
            if replacement == "root" || replacement == "ancestor" {
                assert_eq!(std::fs::read(unit.join("payload")).unwrap(), b"substitute");
            }
            if replacement == "leaf" {
                assert_eq!(
                    std::fs::read(substitute_stage.borrow().as_ref().unwrap().join("payload"))
                        .unwrap(),
                    b"substitute"
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generated_final_use_keeps_project_cwd_outside_the_staged_subtree() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        let target = project.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("payload"), b"generated fixture").unwrap();
        let environment = fixture_environment(&root);
        let reviewed =
            snapshot_temporary_unit(&environment, &root, &target, &AtomicBool::new(false));
        let mut owner = std::process::Command::new("/bin/sleep")
            .arg("10")
            .current_dir(&project)
            .spawn()
            .unwrap();
        let usage = observe_temporary_use(&environment, &project);
        let backend = crate::MockTrashBackend::new();
        let outcome = move_reviewed_unit(&environment, &reviewed, true, Some(&project), &backend);
        let _ = owner.kill();
        owner.wait().unwrap();
        assert_eq!(usage.state, TemporaryUsageState::InUse);
        assert!(outcome.unwrap_err().contains("Generated project use"));
        assert!(backend.moved().is_empty());
        assert!(target.join("payload").exists());
        struct FixtureTrash(PathBuf);
        impl crate::TrashBackend for FixtureTrash {
            fn move_to_trash(&self, path: &Path) -> Result<(), String> {
                std::fs::rename(path, &self.0).map_err(|error| error.to_string())
            }
        }
        let idle_review =
            snapshot_temporary_unit(&environment, &root, &target, &AtomicBool::new(false));
        move_reviewed_unit(
            &environment,
            &idle_review,
            false,
            Some(&project),
            &FixtureTrash(root.join("fixture-trash-target")),
        )
        .unwrap();
        assert!(!target.exists());
        assert!(root.join("fixture-trash-target/payload").exists());
    }
}
