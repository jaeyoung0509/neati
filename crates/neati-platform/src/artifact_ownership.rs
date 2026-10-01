//! Bounded, read-only ownership observations for an exact project artifact.
//!
//! This adapter has no command argument or IPC surface. It inspects names and
//! Git's index, never deployment key contents or project scripts. Absence of a
//! repository is established by a bounded ancestor walk; a broken Git marker
//! cannot be mistaken for that absence.
use crate::{run_with_timeout_cancellable, PlatformEnvironment, SubprocessError};
use neati_core::domain::storage::{
    ArtifactOwnershipEvidence as Evidence, ArtifactOwnershipUncertainty as Uncertainty,
};
use std::fs::{self, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

const MAX_ENTRIES: usize = 250_000;
const MAX_DEPTH: usize = 64;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const GIT_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(windows)]
const INERT_PATH: &str = "NUL";
#[cfg(not(windows))]
const INERT_PATH: &str = "/dev/null";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    entity: (u64, u64),
    #[cfg(windows)]
    entity: WindowsEntity,
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct WindowsEntity {
    volume: u64,
    file_id: [u8; 16],
}

fn stamp(path: &Path, metadata: &Metadata) -> Result<Stamp, Uncertainty> {
    #[cfg(unix)]
    let _ = path;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let file = OpenOptions::new()
            .read(true)
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| Uncertainty::UnreadableMetadata)?;
        let opened = file
            .metadata()
            .map_err(|_| Uncertainty::UnreadableMetadata)?;
        if is_link(&opened)
            || metadata.file_type() != opened.file_type()
            || metadata.len() != opened.len()
            || metadata.modified().ok() != opened.modified().ok()
        {
            return Err(Uncertainty::ChangedDuringProbe);
        }
        stamp_opened(&file, &opened)
    }
    #[cfg(unix)]
    Ok(Stamp {
        size: metadata.len(),
        modified: metadata
            .modified()
            .map_err(|_| Uncertainty::UnreadableMetadata)?,
        #[cfg(unix)]
        entity: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        },
    })
}

#[cfg(windows)]
fn stamp_opened(file: &fs::File, metadata: &Metadata) -> Result<Stamp, Uncertainty> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    // Compare the full native identifier. Creation time is user-writable and
    // is never an identity fallback. Unsupported filesystems stay uncertain.
    let observed = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if observed == 0 || info.VolumeSerialNumber == 0 || info.FileId.Identifier == [0; 16] {
        return Err(Uncertainty::UnreadableMetadata);
    }
    Ok(Stamp {
        size: metadata.len(),
        modified: metadata
            .modified()
            .map_err(|_| Uncertainty::UnreadableMetadata)?,
        entity: WindowsEntity {
            volume: info.VolumeSerialNumber,
            file_id: info.FileId.Identifier,
        },
    })
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn same_entity(left: &Stamp, right: &Stamp) -> bool {
    left.entity == right.entity
}

fn is_platform_alias(path: &Path) -> bool {
    cfg!(target_os = "macos") && [Path::new("/tmp"), Path::new("/var")].contains(&path)
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    started: Instant,
    entries: usize,
    max_entries: usize,
    stamps: Vec<(PathBuf, Stamp)>,
    component_entities: Vec<(PathBuf, Stamp)>,
    repository_absences: Vec<(PathBuf, Stamp)>,
    metadata_absences: Vec<PathBuf>,
    configurations: Vec<String>,
}

impl Budget<'_> {
    fn check(&self) -> Result<(), Uncertainty> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Uncertainty::Cancelled)
        } else if self.started.elapsed() >= PROBE_TIMEOUT {
            Err(Uncertainty::TimedOut)
        } else {
            Ok(())
        }
    }

    fn visit(&mut self) -> Result<(), Uncertainty> {
        self.check()?;
        self.entries += 1;
        if self.entries > self.max_entries {
            return Err(Uncertainty::BudgetExceeded);
        }
        Ok(())
    }

    fn record(&mut self, path: &Path, metadata: &Metadata) -> Result<(), Uncertainty> {
        self.stamps
            .push((path.to_path_buf(), stamp(path, metadata)?));
        Ok(())
    }

    fn unchanged(&mut self) -> Result<(), Uncertainty> {
        for (path, expected) in &self.component_entities {
            self.check()?;
            let metadata =
                fs::symlink_metadata(path).map_err(|_| Uncertainty::ChangedDuringProbe)?;
            if is_link(&metadata)
                || !metadata.is_dir()
                || !same_entity(&stamp(path, &metadata)?, expected)
            {
                return Err(Uncertainty::ChangedDuringProbe);
            }
        }
        for (path, expected) in &self.stamps {
            self.check()?;
            let metadata =
                fs::symlink_metadata(path).map_err(|_| Uncertainty::ChangedDuringProbe)?;
            if is_link(&metadata) || stamp(path, &metadata)? != *expected {
                return Err(Uncertainty::ChangedDuringProbe);
            }
        }
        for (path, expected) in self.repository_absences.clone() {
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| Uncertainty::ChangedDuringProbe)?;
            let current = stamp(&path, &metadata)?;
            if is_link(&metadata) || !metadata.is_dir() || !same_entity(&current, &expected) {
                return Err(Uncertainty::ChangedDuringProbe);
            }
            for entry in fs::read_dir(&path).map_err(|_| Uncertainty::ChangedDuringProbe)? {
                self.visit()?;
                let entry = entry.map_err(|_| Uncertainty::ChangedDuringProbe)?;
                if os_bytes(&entry.file_name())
                    .ok_or(Uncertainty::MalformedMetadata)?
                    .eq_ignore_ascii_case(b".git")
                {
                    return Err(Uncertainty::ChangedDuringProbe);
                }
            }
        }
        for path in &self.metadata_absences {
            self.check()?;
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(Uncertainty::ChangedDuringProbe),
            }
        }
        Ok(())
    }
}

/// Complete ownership evidence is required separately from byte measurement.
pub fn probe_artifact_ownership(
    environment: &PlatformEnvironment,
    project_root: &Path,
    artifact: &Path,
    cancel: &AtomicBool,
) -> Evidence {
    probe_with(
        environment,
        project_root,
        artifact,
        cancel,
        MAX_ENTRIES,
        &|command, timeout, cancel| {
            run_with_timeout_cancellable(command, timeout, &|| cancel.load(Ordering::Relaxed))
        },
        &|| {},
    )
}

type GitRunner<'a> = dyn Fn(Command, Duration, &AtomicBool) -> Result<Output, SubprocessError> + 'a;

fn probe_with(
    environment: &PlatformEnvironment,
    project_root: &Path,
    artifact: &Path,
    cancel: &AtomicBool,
    max_entries: usize,
    run: &GitRunner<'_>,
    before_recheck: &dyn Fn(),
) -> Evidence {
    let mut budget = Budget {
        cancel,
        started: Instant::now(),
        entries: 0,
        max_entries,
        stamps: Vec::new(),
        component_entities: Vec::new(),
        repository_absences: Vec::new(),
        metadata_absences: Vec::new(),
        configurations: Vec::new(),
    };
    let result = (|| {
        budget.check()?;
        if !project_root.is_absolute()
            || artifact == project_root
            || !artifact.starts_with(project_root)
            || project_root
                .components()
                .chain(artifact.components())
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(Uncertainty::OutsideScope);
        }
        for path in artifact
            .ancestors()
            .take_while(|path| path.starts_with(project_root))
        {
            budget.check()?;
            let metadata =
                fs::symlink_metadata(path).map_err(|_| Uncertainty::UnreadableMetadata)?;
            if is_link(&metadata) || !metadata.is_dir() {
                return Err(Uncertainty::OutsideScope);
            }
            budget
                .component_entities
                .push((path.to_path_buf(), stamp(path, &metadata)?));
        }
        // The caller's trusted root may have a platform alias above it
        // (/var and /tmp on macOS). No link below that root is accepted.
        let project_root =
            fs::canonicalize(project_root).map_err(|_| Uncertainty::UnreadableMetadata)?;
        let artifact = fs::canonicalize(artifact).map_err(|_| Uncertainty::UnreadableMetadata)?;
        let project_root = project_root.as_path();
        let artifact = artifact.as_path();
        if artifact == project_root || !artifact.starts_with(project_root) {
            return Err(Uncertainty::OutsideScope);
        }
        let metadata =
            fs::symlink_metadata(project_root).map_err(|_| Uncertainty::UnreadableMetadata)?;
        budget
            .component_entities
            .push((project_root.to_path_buf(), stamp(project_root, &metadata)?));
        verify_components(artifact, &mut budget)?;
        if let Some(protected) = inspect_names(artifact, &mut budget)? {
            return Ok(protected);
        }
        let routes = repository_routes(project_root, &mut budget)?;
        let Some(outermost) = routes.last() else {
            before_recheck();
            budget.unchanged()?;
            return Ok(Evidence::VerifiedGenerated);
        };
        let git = resolve_git(environment, &outermost.root).ok_or(Uncertainty::MissingGit)?;
        // Scratch identities remain bound until every ancestor index has been
        // inspected. Dropping an earlier directory would invalidate our stamps.
        let mut snapshots = Vec::new();
        for route in &routes {
            let relative = artifact
                .strip_prefix(&route.root)
                .map_err(|_| Uncertainty::OutsideScope)?;
            let relative = path_bytes(relative).ok_or(Uncertainty::MalformedMetadata)?;
            snapshots.push(GitIndexSnapshot::capture(
                environment,
                route,
                artifact,
                &mut budget,
            )?);
            let snapshot = snapshots.last().ok_or(Uncertainty::UnreadableMetadata)?;
            snapshot.verify_configuration(&git, environment, &mut budget, run)?;
            let command = index_command(&git, snapshot, environment);
            budget.check()?;
            budget.unchanged()?;
            let timeout = GIT_TIMEOUT.min(PROBE_TIMEOUT.saturating_sub(budget.started.elapsed()));
            let output = run(command, timeout, cancel).map_err(|error| match error {
                SubprocessError::Timeout(..) => Uncertainty::TimedOut,
                SubprocessError::Cancelled(..) => Uncertainty::Cancelled,
                _ => Uncertainty::UnreadableMetadata,
            })?;
            budget.check()?;
            if !output.status.success()
                || output.stdout.len() >= crate::subprocess::MAX_CAPTURE_BYTES
                || output.stderr.len() >= crate::subprocess::MAX_CAPTURE_BYTES
            {
                return Err(Uncertainty::MalformedMetadata);
            }
            let tracked =
                index_intersects_artifact(environment.flavor(), &output.stdout, &relative)?;
            // Index and every visited directory are rebound after the command. A
            // changed descendant cannot hide behind an unchanged artifact root.
            before_recheck();
            budget.unchanged()?;
            if tracked {
                return Ok(Evidence::TrackedContent);
            }
        }
        Ok(Evidence::VerifiedGenerated)
    })();
    result.unwrap_or_else(Evidence::Incomplete)
}

fn verify_components(path: &Path, budget: &mut Budget<'_>) -> Result<(), Uncertainty> {
    for component in path.ancestors() {
        budget.check()?;
        let metadata =
            fs::symlink_metadata(component).map_err(|_| Uncertainty::UnreadableMetadata)?;
        if is_link(&metadata) && is_platform_alias(component) {
            continue;
        }
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(Uncertainty::OutsideScope);
        }
        budget
            .component_entities
            .push((component.to_path_buf(), stamp(component, &metadata)?));
        if component == path {
            budget.record(component, &metadata)?;
        }
    }
    Ok(())
}

fn inspect_names(root: &Path, budget: &mut Budget<'_>) -> Result<Option<Evidence>, Uncertainty> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        budget.visit()?;
        if depth > MAX_DEPTH {
            return Err(Uncertainty::BudgetExceeded);
        }
        let metadata =
            fs::symlink_metadata(&directory).map_err(|_| Uncertainty::UnreadableMetadata)?;
        if is_link(&metadata) || !metadata.is_dir() {
            return Err(Uncertainty::ChangedDuringProbe);
        }
        budget.record(&directory, &metadata)?;
        for entry in fs::read_dir(&directory).map_err(|_| Uncertainty::UnreadableMetadata)? {
            budget.visit()?;
            let entry = entry.map_err(|_| Uncertainty::UnreadableMetadata)?;
            let name = entry.file_name();
            let bytes = os_bytes(&name).ok_or(Uncertainty::MalformedMetadata)?;
            if bytes.eq_ignore_ascii_case(b".git") {
                return Ok(Some(Evidence::NestedRepository));
            }
            if bytes.to_ascii_lowercase().ends_with(b"-keypair.json") {
                return Ok(Some(Evidence::DeploymentKeyMaterial));
            }
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|_| Uncertainty::UnreadableMetadata)?;
            // Whole-unit Trash moves the link itself, never its destination.
            // Retain the existing partial measurement contract for such links.
            if !is_link(&metadata) && metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if !is_link(&metadata) && !metadata.is_file() {
                return Err(Uncertainty::UnreadableMetadata);
            }
        }
    }
    Ok(None)
}

struct RepositoryRoute {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
    configurations: Vec<String>,
}

/// Git never receives a live repository metadata path. A split-index or config
/// replacement during child startup can therefore affect only this private copy.
struct GitIndexSnapshot {
    _directory: tempfile::TempDir,
    route: RepositoryRoute,
    configurations: Vec<PathBuf>,
}

impl GitIndexSnapshot {
    fn capture(
        environment: &PlatformEnvironment,
        source: &RepositoryRoute,
        artifact: &Path,
        budget: &mut Budget<'_>,
    ) -> Result<Self, Uncertainty> {
        let index = read_metadata_bytes(&source.git_dir.join("index"), budget, MAX_INDEX_BYTES)?;
        validate_index_snapshot(&index)?;
        let temp_root = fs::canonicalize(environment.temp_dir())
            .map_err(|_| Uncertainty::UnreadableMetadata)?;
        if temp_root.starts_with(artifact) {
            return Err(Uncertainty::OutsideScope);
        }
        budget.check()?;
        let directory = tempfile::Builder::new()
            .prefix("neati-artifact-index-")
            .tempdir_in(&temp_root)
            .map_err(|_| Uncertainty::UnreadableMetadata)?;
        let git_dir = directory.path().to_path_buf();
        for child in ["objects", "refs"] {
            fs::create_dir(git_dir.join(child)).map_err(|_| Uncertainty::UnreadableMetadata)?;
        }
        fs::write(
            git_dir.join("HEAD"),
            "ref: refs/heads/neati-artifact-probe\n",
        )
        .map_err(|_| Uncertainty::UnreadableMetadata)?;
        fs::write(
            git_dir.join("config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .map_err(|_| Uncertainty::UnreadableMetadata)?;
        fs::write(git_dir.join("index"), index).map_err(|_| Uncertainty::UnreadableMetadata)?;
        let mut configurations = Vec::new();
        for (number, bytes) in source.configurations.iter().enumerate() {
            let path = git_dir.join(format!("reviewed-config-{number}"));
            fs::write(&path, bytes).map_err(|_| Uncertainty::UnreadableMetadata)?;
            configurations.push(path);
        }
        verify_components(&git_dir, budget)?;
        Ok(Self {
            _directory: directory,
            route: RepositoryRoute {
                root: source.root.clone(),
                common_dir: git_dir.clone(),
                git_dir,
                configurations: Vec::new(),
            },
            configurations,
        })
    }

    fn verify_configuration(
        &self,
        git: &Path,
        environment: &PlatformEnvironment,
        budget: &mut Budget<'_>,
        run: &GitRunner<'_>,
    ) -> Result<(), Uncertainty> {
        for path in &self.configurations {
            budget.unchanged()?;
            let mut command = Command::new(git);
            configure_git_command(&mut command, &self.route, environment);
            command
                .args(["config", "--no-includes", "--file"])
                .arg(path)
                .args(["--null", "--list"]);
            let timeout = GIT_TIMEOUT.min(PROBE_TIMEOUT.saturating_sub(budget.started.elapsed()));
            let output = run(command, timeout, budget.cancel).map_err(|error| match error {
                SubprocessError::Timeout(..) => Uncertainty::TimedOut,
                SubprocessError::Cancelled(..) => Uncertainty::Cancelled,
                _ => Uncertainty::UnreadableMetadata,
            })?;
            budget.check()?;
            if !output.status.success()
                || output.stdout.len() >= crate::subprocess::MAX_CAPTURE_BYTES
                || output.stderr.len() >= crate::subprocess::MAX_CAPTURE_BYTES
                || (!output.stdout.is_empty() && output.stdout.last() != Some(&0))
            {
                return Err(Uncertainty::MalformedMetadata);
            }
            for record in output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|record| !record.is_empty())
            {
                let (key, value) = match record.iter().position(|byte| *byte == b'\n') {
                    Some(split) => (&record[..split], &record[split + 1..]),
                    None => (record, b"".as_slice()),
                };
                if (key == b"core.repositoryformatversion"
                    && ![b"0".as_slice(), b"1"].contains(&value))
                    || (key.starts_with(b"extensions.") && key != b"extensions.worktreeconfig")
                {
                    return Err(Uncertainty::UnsupportedIndex);
                }
                if key == b"core.worktree" {
                    return Err(Uncertainty::OutsideScope);
                }
                if key == b"core.bare" {
                    let value = value.to_ascii_lowercase();
                    if [b"true".as_slice(), b"1", b"yes", b"on", b""].contains(&value.as_slice()) {
                        return Err(Uncertainty::UnsupportedIndex);
                    }
                    if ![b"false".as_slice(), b"0", b"no", b"off"].contains(&value.as_slice()) {
                        return Err(Uncertainty::MalformedMetadata);
                    }
                }
                if key == b"extensions.worktreeconfig"
                    && ![
                        b"true".as_slice(),
                        b"false",
                        b"1",
                        b"0",
                        b"yes",
                        b"no",
                        b"on",
                        b"off",
                        b"",
                    ]
                    .contains(&value.to_ascii_lowercase().as_slice())
                {
                    return Err(Uncertainty::MalformedMetadata);
                }
            }
        }
        Ok(())
    }
}

fn validate_index_snapshot(bytes: &[u8]) -> Result<(), Uncertainty> {
    use sha1::{Digest, Sha1};
    if bytes.len() < 32 || &bytes[..4] != b"DIRC" {
        return Err(Uncertainty::MalformedMetadata);
    }
    let version = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    if ![2, 3, 4].contains(&version) {
        return Err(Uncertainty::UnsupportedIndex);
    }
    let count = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
    if count as usize > MAX_ENTRIES {
        return Err(Uncertainty::BudgetExceeded);
    }
    let (contents, checksum) = bytes.split_at(bytes.len() - 20);
    if checksum == [0; 20] {
        return Err(Uncertainty::UnsupportedIndex);
    }
    let digest = Sha1::digest(contents);
    if &digest[..] != checksum {
        return Err(Uncertainty::MalformedMetadata);
    }
    Ok(())
}

fn repository_routes(
    project: &Path,
    budget: &mut Budget<'_>,
) -> Result<Vec<RepositoryRoute>, Uncertainty> {
    let mut routes = Vec::new();
    for (depth, root) in project.ancestors().enumerate() {
        if depth > MAX_DEPTH {
            return Err(Uncertainty::BudgetExceeded);
        }
        budget.check()?;
        // Enumerating names also refuses case-variant metadata on a
        // case-sensitive filesystem, where Git would otherwise miss it.
        let mut marker = None;
        for entry in fs::read_dir(root).map_err(|_| Uncertainty::UnreadableMetadata)? {
            budget.visit()?;
            let entry = entry.map_err(|_| Uncertainty::UnreadableMetadata)?;
            if os_bytes(&entry.file_name())
                .ok_or(Uncertainty::MalformedMetadata)?
                .eq_ignore_ascii_case(b".git")
            {
                if marker.is_some() || entry.file_name() != ".git" {
                    return Err(Uncertainty::MalformedMetadata);
                }
                marker = Some(entry.path());
            }
        }
        let Some(marker) = marker else {
            let metadata =
                fs::symlink_metadata(root).map_err(|_| Uncertainty::UnreadableMetadata)?;
            if is_link(&metadata) || !metadata.is_dir() {
                return Err(Uncertainty::OutsideScope);
            }
            budget
                .repository_absences
                .push((root.to_path_buf(), stamp(root, &metadata)?));
            continue;
        };
        let metadata =
            fs::symlink_metadata(&marker).map_err(|_| Uncertainty::UnreadableMetadata)?;
        if is_link(&metadata) {
            return Err(Uncertainty::MalformedMetadata);
        }
        budget.record(&marker, &metadata)?;
        let config_start = budget.configurations.len();
        let (git_dir, common_dir) = if metadata.is_dir() {
            let commondir = marker.join("commondir");
            match fs::symlink_metadata(&commondir) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    budget.metadata_absences.push(commondir)
                }
                _ => return Err(Uncertainty::MalformedMetadata),
            }
            (marker.clone(), marker)
        } else if metadata.is_file() {
            let pointer = read_metadata(&marker, budget)?;
            let pointer = pointer
                .strip_prefix("gitdir: ")
                .ok_or(Uncertainty::MalformedMetadata)?;
            let target = recorded_path(root, pointer)?;
            verify_components(&target, budget)?;
            let target = fs::canonicalize(target).map_err(|_| Uncertainty::MalformedMetadata)?;
            // External routing is accepted only for a linked worktree with
            // both reciprocal metadata and the exact common-directory shape.
            let back = recorded_path(&target, &read_metadata(&target.join("gitdir"), budget)?)?;
            if fs::canonicalize(back).map_err(|_| Uncertainty::MalformedMetadata)?
                != root.join(".git")
            {
                return Err(Uncertainty::MalformedMetadata);
            }
            let common =
                recorded_path(&target, &read_metadata(&target.join("commondir"), budget)?)?;
            // Bind the recorded route before canonicalization, then compare
            // like canonical forms. Windows canonical paths carry a verbatim
            // prefix that lexical metadata paths deliberately normalize away.
            verify_components(&common, budget)?;
            let common = fs::canonicalize(common).map_err(|_| Uncertainty::MalformedMetadata)?;
            if target.parent().and_then(Path::parent) != Some(common.as_path())
                || target.parent().and_then(Path::file_name)
                    != Some(std::ffi::OsStr::new("worktrees"))
            {
                return Err(Uncertainty::MalformedMetadata);
            }
            verify_components(&common, budget)?;
            inspect_configuration(&common.join("config"), budget, true)?;
            (target, common)
        } else {
            return Err(Uncertainty::MalformedMetadata);
        };
        validate_head(&read_metadata(&git_dir.join("HEAD"), budget)?)?;
        inspect_configuration(&git_dir.join("config"), budget, metadata.is_dir())?;
        inspect_configuration(&git_dir.join("config.worktree"), budget, false)?;
        for name in ["objects", "refs"] {
            let path = common_dir.join(name);
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| Uncertainty::UnreadableMetadata)?;
            if is_link(&metadata) || !metadata.is_dir() {
                return Err(Uncertainty::MalformedMetadata);
            }
            budget
                .component_entities
                .push((path.clone(), stamp(&path, &metadata)?));
        }
        // A missing index is uncertainty, including an index removed from an
        // otherwise valid repository. Do not reinterpret it as zero tracked files.
        let index = git_dir.join("index");
        let metadata = fs::symlink_metadata(&index).map_err(|_| Uncertainty::UnreadableMetadata)?;
        if is_link(&metadata) || !metadata.is_file() || metadata.len() < 12 {
            return Err(Uncertainty::MalformedMetadata);
        }
        budget.record(&index, &metadata)?;
        // Git refreshes shared-index mtimes even for ls-files with optional
        // locks disabled. Refuse this layout before launching a child.
        for entry in fs::read_dir(&git_dir).map_err(|_| Uncertainty::UnreadableMetadata)? {
            budget.visit()?;
            let entry = entry.map_err(|_| Uncertainty::UnreadableMetadata)?;
            if os_bytes(&entry.file_name())
                .ok_or(Uncertainty::MalformedMetadata)?
                .to_ascii_lowercase()
                .starts_with(b"sharedindex.")
            {
                return Err(Uncertainty::UnsupportedIndex);
            }
        }
        routes.push(RepositoryRoute {
            root: root.to_path_buf(),
            git_dir,
            common_dir,
            configurations: budget.configurations[config_start..].to_vec(),
        });
    }
    Ok(routes)
}

fn recorded_path(base: &Path, contents: &str) -> Result<PathBuf, Uncertainty> {
    let text = contents.strip_suffix('\n').unwrap_or(contents);
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.is_empty() || text.contains(['\n', '\r', '\0']) {
        return Err(Uncertainty::MalformedMetadata);
    }
    let path = Path::new(text);
    Ok(crate::path_algebra::normalize_lexical(
        &if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        },
    ))
}

fn validate_head(contents: &str) -> Result<(), Uncertainty> {
    let text = contents.strip_suffix('\n').unwrap_or(contents);
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    if let Some(reference) = text.strip_prefix("ref: refs/") {
        if !reference.is_empty()
            && !reference.ends_with('.')
            && !reference.contains("..")
            && !reference.contains("@{")
            && !reference
                .chars()
                .any(|ch| ch.is_control() || " ~^:?*[\\".contains(ch))
            && reference
                .split('/')
                .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
        {
            return Ok(());
        }
    }
    Err(Uncertainty::MalformedMetadata)
}

fn read_metadata(path: &Path, budget: &mut Budget<'_>) -> Result<String, Uncertainty> {
    String::from_utf8(read_metadata_bytes(path, budget, MAX_METADATA_BYTES)?)
        .map_err(|_| Uncertainty::MalformedMetadata)
}

fn read_metadata_bytes(
    path: &Path,
    budget: &mut Budget<'_>,
    max_bytes: usize,
) -> Result<Vec<u8>, Uncertainty> {
    budget.check()?;
    let metadata = fs::symlink_metadata(path).map_err(|_| Uncertainty::UnreadableMetadata)?;
    if is_link(&metadata) || !metadata.is_file() {
        return Err(Uncertainty::MalformedMetadata);
    }
    if metadata.len() > max_bytes as u64 {
        return Err(Uncertainty::BudgetExceeded);
    }
    let expected = stamp(path, &metadata)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(path)
        .map_err(|_| Uncertainty::UnreadableMetadata)?;
    let opened = file
        .metadata()
        .map_err(|_| Uncertainty::UnreadableMetadata)?;
    #[cfg(unix)]
    let opened_stamp = stamp(path, &opened)?;
    #[cfg(windows)]
    let opened_stamp = stamp_opened(&file, &opened)?;
    if is_link(&opened) || !opened.is_file() || opened_stamp != expected {
        return Err(Uncertainty::ChangedDuringProbe);
    }
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Uncertainty::UnreadableMetadata)?;
    if bytes.len() > max_bytes {
        return Err(Uncertainty::BudgetExceeded);
    }
    let current = fs::symlink_metadata(path).map_err(|_| Uncertainty::ChangedDuringProbe)?;
    if is_link(&current) || stamp(path, &current)? != expected {
        return Err(Uncertainty::ChangedDuringProbe);
    }
    budget.stamps.push((path.to_path_buf(), expected));
    Ok(bytes)
}

fn inspect_configuration(
    path: &Path,
    budget: &mut Budget<'_>,
    required: bool,
) -> Result<(), Uncertainty> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
            budget.metadata_absences.push(path.to_path_buf());
            Ok(())
        }
        Err(_) => Err(Uncertainty::UnreadableMetadata),
        Ok(_) => {
            let config = read_metadata(path, budget)?;
            // ls-files does not use filters or diff drivers. Includes could
            // route configuration reads to arbitrary files (including secrets)
            // or unbounded metadata, so this narrow adapter refuses them.
            if config.contains('\0') || config.to_ascii_lowercase().contains("include") {
                Err(Uncertainty::MalformedMetadata)
            } else if config.to_ascii_lowercase().contains("objectformat") {
                // The private index decoder has an explicit SHA-1 contract.
                // Other hash widths cannot be inferred from raw index bytes.
                Err(Uncertainty::UnsupportedIndex)
            } else {
                budget.configurations.push(config);
                Ok(())
            }
        }
    }
}

fn resolve_git(environment: &PlatformEnvironment, project: &Path) -> Option<PathBuf> {
    let candidates = if let Some(stated) = environment.tool("git") {
        vec![stated.path()?.to_path_buf()]
    } else {
        let name = if cfg!(windows) { "git.exe" } else { "git" };
        let mut paths = environment
            .path_entries()
            .iter()
            .take(128)
            .map(|root| root.join(name))
            .collect::<Vec<_>>();
        #[cfg(not(windows))]
        paths.extend([
            PathBuf::from("/usr/bin/git"),
            PathBuf::from("/opt/homebrew/bin/git"),
            PathBuf::from("/usr/local/bin/git"),
        ]);
        #[cfg(windows)]
        if let Some(root) = environment.program_files() {
            paths.push(root.join("Git/cmd/git.exe"));
        }
        paths
    };
    candidates.into_iter().find_map(|path| {
        if !path.is_absolute() || path.starts_with(project) {
            return None;
        }
        let resolved = fs::canonicalize(path).ok()?;
        if resolved.starts_with(project) || !resolved.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if resolved.metadata().ok()?.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        Some(resolved)
    })
}

fn index_command(
    git: &Path,
    snapshot: &GitIndexSnapshot,
    environment: &PlatformEnvironment,
) -> Command {
    let mut command = Command::new(git);
    configure_index_command(&mut command, &snapshot.route, environment);
    command
}

fn configure_index_command(
    command: &mut Command,
    route: &RepositoryRoute,
    environment: &PlatformEnvironment,
) {
    configure_git_command(command, route, environment);
    command.args([
        "ls-files",
        "--cached",
        "--stage",
        "--full-name",
        "--sparse",
        "-z",
    ]);
}

fn configure_git_command(
    command: &mut Command,
    route: &RepositoryRoute,
    environment: &PlatformEnvironment,
) {
    command
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", INERT_PATH)
        .env("GIT_CONFIG_GLOBAL", INERT_PATH)
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_COMMON_DIR", &route.common_dir)
        .env("GIT_INDEX_FILE", route.git_dir.join("index"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_PROTOCOL_FROM_USER", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("LC_ALL", "C")
        .current_dir(&route.root)
        .arg("--no-pager")
        .arg("--no-optional-locks")
        .arg("--literal-pathspecs")
        .arg("--git-dir")
        .arg(&route.git_dir)
        .arg("--work-tree")
        .arg(&route.root);
    #[cfg(windows)]
    if let Some(root) = environment.system_root() {
        command.env("SystemRoot", root);
    }
    #[cfg(not(windows))]
    let _ = environment;
    for setting in [
        "core.fsmonitor=",
        "core.untrackedCache=false",
        "core.pager=",
        "core.sshCommand=neati-disabled-ssh",
        "diff.external=",
        "protocol.allow=never",
    ] {
        command.arg("-c").arg(setting);
    }
    command
        .arg("-c")
        .arg(format!("core.hooksPath={INERT_PATH}"));
}

fn index_intersects_artifact(
    flavor: crate::PathFlavor,
    bytes: &[u8],
    artifact: &[u8],
) -> Result<bool, Uncertainty> {
    validate_index_path(flavor, artifact)?;
    if bytes.is_empty() {
        return Ok(false);
    }
    if bytes.last() != Some(&0) {
        return Err(Uncertainty::MalformedMetadata);
    }
    let artifact = artifact.to_ascii_lowercase();
    let mut tracked = false;
    for record in bytes[..bytes.len() - 1].split(|byte| *byte == 0) {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(Uncertainty::MalformedMetadata)?;
        let fields = record[..tab]
            .split(|byte| *byte == b' ')
            .collect::<Vec<_>>();
        let path = &record[tab + 1..];
        let component_path = path.strip_suffix(b"/").unwrap_or(path);
        validate_index_path(flavor, component_path)?;
        if fields.len() != 3
            || ![
                b"100644".as_slice(),
                b"100755",
                b"120000",
                b"160000",
                b"040000",
            ]
            .contains(&fields[0])
            || !matches!(fields[1].len(), 40 | 64)
            || !fields[1].iter().all(u8::is_ascii_hexdigit)
            || ![b"0".as_slice(), b"1", b"2", b"3"].contains(&fields[2])
            || path.is_empty()
            || path.starts_with(b"/")
            || (path.ends_with(b"/") && fields[0] != b"040000")
            || component_path
                .split(|byte| *byte == b'/')
                .any(|part| part.is_empty() || part == b".." || part == b".")
        {
            return Err(Uncertainty::MalformedMetadata);
        }
        let path = path.strip_suffix(b"/").unwrap_or(path).to_ascii_lowercase();
        if path == artifact
            || (path.starts_with(&artifact) && path.get(artifact.len()) == Some(&b'/'))
            || (artifact.starts_with(&path) && artifact.get(path.len()) == Some(&b'/'))
        {
            tracked = true;
        } else if !artifact.is_ascii() || !path.is_ascii() {
            // Filesystem Unicode case folding and normalization differ by
            // platform. A byte mismatch cannot prove disjointness there.
            return Err(Uncertainty::MalformedMetadata);
        }
    }
    Ok(tracked)
}

fn validate_index_path(flavor: crate::PathFlavor, bytes: &[u8]) -> Result<(), Uncertainty> {
    if bytes.is_empty()
        || bytes.starts_with(b"/")
        || bytes
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(Uncertainty::MalformedMetadata);
    }
    if flavor.is_windows() {
        use crate::path_algebra as paths;
        let text = std::str::from_utf8(bytes).map_err(|_| Uncertainty::MalformedMetadata)?;
        if bytes.contains(&b'\\')
            || bytes.contains(&b':')
            || paths::is_absolute(text, flavor)
            || paths::contains_short_name(text, flavor)
            || paths::has_alternate_data_stream(text, flavor)
            || paths::has_trailing_dot_or_space(text, flavor)
            || text.split('/').any(paths::is_reserved_device_name)
        {
            return Err(Uncertainty::MalformedMetadata);
        }
    }
    Ok(())
}

fn path_bytes(path: &Path) -> Option<Vec<u8>> {
    let components = path
        .components()
        .map(|component| os_bytes(component.as_os_str()))
        .collect::<Option<Vec<_>>>()?;
    Some(components.join(&b'/'))
}

fn os_bytes(value: &std::ffi::OsStr) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(value.as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        Some(value.to_str()?.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathFlavor;

    fn is_index_query(command: &Command) -> bool {
        command.get_args().any(|argument| argument == "ls-files")
    }

    fn environment(root: &Path) -> PlatformEnvironment {
        let git = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\Git\cmd\git.exe")
        } else {
            PathBuf::from("/usr/bin/git")
        };
        assert!(
            git.is_file(),
            "Git is a prerequisite for repository fixtures"
        );
        PlatformEnvironment::simulated(PathFlavor::current())
            .with_home(root)
            .with_temp_dir(root)
            .with_tool("git", git)
    }

    fn git(root: &Path, arguments: &[&str]) {
        let environment = environment(root);
        let git = environment.tool("git").unwrap().path().unwrap();
        let output = Command::new(git)
            .current_dir(root)
            .args([
                "-c",
                "core.fsmonitor=",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(arguments)
            .env("GIT_CONFIG_GLOBAL", INERT_PATH)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap().join("repo");
        let artifact = root.join("target");
        fs::create_dir_all(artifact.join("assets")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(artifact.join("assets/generated.bin"), b"generated").unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["add", "--", "Cargo.toml"]);
        (temp, root, artifact)
    }

    fn probe(root: &Path, artifact: &Path) -> Evidence {
        probe_artifact_ownership(&environment(root), root, artifact, &AtomicBool::new(false))
    }

    #[test]
    fn generated_untracked_payload_is_verified_and_new_tracked_content_is_protected() {
        let (_temp, root, artifact) = repository();
        assert_eq!(probe(&root, &artifact), Evidence::VerifiedGenerated);
        git(&root, &["add", "--", "target/assets/generated.bin"]);
        assert_eq!(probe(&root, &artifact), Evidence::TrackedContent);
        git(
            &root,
            &[
                "update-index",
                "--skip-worktree",
                "target/assets/generated.bin",
            ],
        );
        assert_eq!(probe(&root, &artifact), Evidence::TrackedContent);
        git(&root, &["update-index", "--split-index"]);
        assert_eq!(
            probe_with(
                &environment(&root),
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("split indexes must be refused before Git runs"),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::UnsupportedIndex)
        );
    }

    #[test]
    fn names_protect_key_material_and_nested_git_directory_or_worktree_file() {
        for name in [
            ".git",
            ".GIT",
            "example-keypair.json",
            "EXAMPLE-KEYPAIR.JSON",
        ] {
            let (_temp, root, artifact) = repository();
            let path = artifact.join("assets").join(name);
            if name == ".git" {
                fs::create_dir(&path).unwrap();
            } else {
                fs::write(&path, b"this is not read").unwrap();
            }
            assert_eq!(
                probe(&root, &artifact),
                if name.eq_ignore_ascii_case(".git") {
                    Evidence::NestedRepository
                } else {
                    Evidence::DeploymentKeyMaterial
                }
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn keypair_name_is_refused_without_opening_a_fifo_or_following_a_link() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;
        let (_temp, root, artifact) = repository();
        let key = artifact.join("assets/example-keypair.json");
        let name = std::ffi::CString::new(key.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert_eq!(probe(&root, &artifact), Evidence::DeploymentKeyMaterial);
        fs::remove_file(&key).unwrap();
        symlink("/neati-outside-sentinel", &key).unwrap();
        assert_eq!(probe(&root, &artifact), Evidence::DeploymentKeyMaterial);
    }

    #[test]
    fn linked_worktree_uses_its_own_index_and_refuses_forged_backlinks() {
        let (_temp, root, _) = repository();
        git(&root, &["commit", "-qm", "fixture"]);
        let linked = root.parent().unwrap().join("linked");
        git(
            &root,
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        );
        let artifact = linked.join("target");
        fs::create_dir(&artifact).unwrap();
        fs::write(artifact.join("generated.bin"), b"generated").unwrap();
        assert_eq!(probe(&linked, &artifact), Evidence::VerifiedGenerated);
        let pointer = fs::read_to_string(linked.join(".git")).unwrap();
        let git_dir = PathBuf::from(pointer.trim().strip_prefix("gitdir: ").unwrap());
        let common_pointer = git_dir.join("commondir");
        let original_common = fs::read(&common_pointer).unwrap();
        let canonical_common = fs::canonicalize(root.join(".git")).unwrap();
        // The same native directory can be recorded with a lexical spelling
        // or the verbatim spelling returned by Windows canonicalization.
        for common in [
            crate::path_algebra::normalize_lexical(&canonical_common),
            canonical_common,
        ] {
            fs::write(&common_pointer, common.to_string_lossy().as_bytes()).unwrap();
            assert_eq!(probe(&linked, &artifact), Evidence::VerifiedGenerated);
        }
        fs::write(&common_pointer, &original_common).unwrap();
        git(&linked, &["add", "--", "target/generated.bin"]);
        assert_eq!(probe(&linked, &artifact), Evidence::TrackedContent);
        fs::write(&common_pointer, root.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(
            probe(&linked, &artifact),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
        fs::write(&common_pointer, original_common).unwrap();
        fs::write(
            git_dir.join("gitdir"),
            root.join(".git").to_string_lossy().as_bytes(),
        )
        .unwrap();
        assert_eq!(
            probe(&linked, &artifact),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
    }

    #[test]
    fn corrupt_missing_and_misrouted_repository_metadata_preserves_uncertainty() {
        let (_temp, root, artifact) = repository();
        fs::write(root.join(".git/index"), b"not an index but long enough").unwrap();
        assert_eq!(
            probe(&root, &artifact),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
        fs::remove_file(root.join(".git/index")).unwrap();
        assert_eq!(
            probe(&root, &artifact),
            Evidence::Incomplete(Uncertainty::UnreadableMetadata)
        );
        fs::remove_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git"), "gitdir: /another-repository\n").unwrap();
        assert!(!probe(&root, &artifact).allows_cleanup());
        fs::write(root.join(".git"), "broken pointer").unwrap();
        assert_eq!(
            probe(&root, &artifact),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
    }

    #[test]
    fn cancellation_timeout_output_limit_and_missing_git_cannot_authorize() {
        let (_temp, root, artifact) = repository();
        let env = environment(&root);
        assert_eq!(
            probe_artifact_ownership(&env, &root, &artifact, &AtomicBool::new(true)),
            Evidence::Incomplete(Uncertainty::Cancelled)
        );
        assert_eq!(
            probe_artifact_ownership(
                &env.clone().with_missing_tool("git"),
                &root,
                &artifact,
                &AtomicBool::new(false)
            ),
            Evidence::Incomplete(Uncertainty::MissingGit)
        );
        assert_eq!(
            probe_with(
                &env,
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, timeout, _| Err(SubprocessError::Timeout("fixture-git".into(), timeout)),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::TimedOut)
        );
        assert_eq!(
            probe_with(
                &env,
                &root,
                &artifact,
                &AtomicBool::new(false),
                1,
                &|_, _, _| panic!("an incomplete walk cannot run Git"),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::BudgetExceeded)
        );
        assert!(index_intersects_artifact(
            crate::PathFlavor::current(),
            b"unterminated metadata",
            b"target"
        )
        .is_err());
        for cap_stdout in [true, false] {
            assert_eq!(
                probe_with(
                    &env,
                    &root,
                    &artifact,
                    &AtomicBool::new(false),
                    MAX_ENTRIES,
                    &|command, timeout, cancel| {
                        let index_query = is_index_query(&command);
                        let mut output = run_with_timeout_cancellable(command, timeout, &|| {
                            cancel.load(Ordering::Relaxed)
                        })?;
                        let capped = vec![b'x'; crate::subprocess::MAX_CAPTURE_BYTES];
                        if index_query && cap_stdout {
                            output.stdout = capped;
                        } else if index_query {
                            output.stderr = capped;
                        }
                        Ok(output)
                    },
                    &|| {},
                ),
                Evidence::Incomplete(Uncertainty::MalformedMetadata)
            );
        }
    }

    #[test]
    fn nul_delimited_paths_case_differences_sparse_directories_and_unicode_are_safe() {
        let oid = "a".repeat(40);
        for path in [
            "target/assets/authored file\nwith\ttabs",
            "TARGET/assets/source.rs",
            "target/non-ASCII-é.rs",
        ] {
            let output = format!("100644 {oid} 0\t{path}\0");
            assert_eq!(
                index_intersects_artifact(
                    crate::PathFlavor::current(),
                    output.as_bytes(),
                    b"target"
                ),
                Ok(true)
            );
        }
        let sparse = format!("040000 {oid} 0\tpackages/\0");
        assert_eq!(
            index_intersects_artifact(
                crate::PathFlavor::current(),
                sparse.as_bytes(),
                b"packages/project/target"
            ),
            Ok(true)
        );
        let gitlink = format!("160000 {oid} 0\tpackages/project\0");
        assert_eq!(
            index_intersects_artifact(
                crate::PathFlavor::current(),
                gitlink.as_bytes(),
                b"packages/project/target"
            ),
            Ok(true)
        );
        for malformed in [
            "target//source.rs",
            "target/source.rs/",
            "/target/file",
            "target/../file",
        ] {
            let output = format!("100644 {oid} 0\t{malformed}\0");
            assert_eq!(
                index_intersects_artifact(
                    crate::PathFlavor::current(),
                    output.as_bytes(),
                    b"target"
                ),
                Err(Uncertainty::MalformedMetadata)
            );
        }
        for (indexed, observed) in [
            ("packages/é/target/authored.rs", "packages/É/target"),
            ("packages/e\u{301}/target/authored.rs", "packages/é/target"),
            ("K/target/source.rs", "k/target"),
        ] {
            let output = format!("100644 {oid} 0\t{indexed}\0");
            assert_eq!(
                index_intersects_artifact(
                    crate::PathFlavor::current(),
                    output.as_bytes(),
                    observed.as_bytes()
                ),
                Err(Uncertainty::MalformedMetadata)
            );
        }
        assert_eq!(
            index_intersects_artifact(
                crate::PathFlavor::current(),
                format!("100644 {oid} 0\ttarget-other/source.rs\0").as_bytes(),
                b"target"
            ),
            Ok(false)
        );
    }

    #[test]
    fn new_repository_or_new_optional_config_during_probe_revokes_authority() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap().join("repo");
        let artifact = root.join("target");
        fs::create_dir_all(&artifact).unwrap();
        fs::write(artifact.join("output.bin"), b"generated").unwrap();
        let before = stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap();
        let result = probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|_, _, _| panic!("no repository existed before the recheck"),
            &|| {
                git(&root, &["init", "-q"]);
            },
        );
        assert_eq!(
            stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap(),
            before
        );
        assert_eq!(
            result,
            Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
        );

        let (_temp, root, artifact) = repository();
        let result = probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|command, timeout, cancel| {
                run_with_timeout_cancellable(command, timeout, &|| cancel.load(Ordering::Relaxed))
            },
            &|| {
                fs::write(
                    root.join(".git/config.worktree"),
                    "[core]\nfsmonitor=bad-command\n",
                )
                .unwrap();
            },
        );
        assert_eq!(
            result,
            Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
        );
    }

    #[test]
    fn descendant_changes_during_index_probe_are_refused_with_unchanged_artifact_root() {
        let (_temp, root, artifact) = repository();
        let before = stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap();
        let result = probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|command, timeout, cancel| {
                let index_query = is_index_query(&command);
                let output = run_with_timeout_cancellable(command, timeout, &|| {
                    cancel.load(Ordering::Relaxed)
                })?;
                if index_query {
                    fs::write(
                        artifact.join("assets/new-keypair.json"),
                        b"unread key material",
                    )
                    .unwrap();
                }
                Ok(output)
            },
            &|| {},
        );
        assert_eq!(
            stamp(&artifact, &fs::metadata(&artifact).unwrap()).unwrap(),
            before
        );
        assert_eq!(
            result,
            Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
        );
    }

    #[test]
    fn repository_fsmonitor_hooks_and_config_includes_never_execute() {
        let (_temp, root, artifact) = repository();
        let sentinel = root.join("executed-sentinel");
        let program = root.join(if cfg!(windows) {
            "fsmonitor.cmd"
        } else {
            "fsmonitor.sh"
        });
        fs::write(
            &program,
            if cfg!(windows) {
                format!("@echo executed>\"{}\"\r\n", sentinel.display())
            } else {
                format!("#!/bin/sh\nprintf executed > '{}'\n", sentinel.display())
            },
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        }
        git(
            &root,
            &["config", "core.fsmonitor", program.to_str().unwrap()],
        );
        git(&root, &["config", "core.hooksPath", root.to_str().unwrap()]);
        assert_eq!(probe(&root, &artifact), Evidence::VerifiedGenerated);
        assert!(!sentinel.exists());
        // Includes are refused before Git can follow an arbitrary config path.
        git(
            &root,
            &["config", "include.path", program.to_str().unwrap()],
        );
        assert_eq!(
            probe(&root, &artifact),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
        assert!(!sentinel.exists());
    }

    #[test]
    fn inherited_git_routing_global_config_and_program_settings_are_scrubbed() {
        let (_temp, root, artifact) = repository();
        let global = root.parent().unwrap().join("global.gitconfig");
        fs::write(&global, "[invalid configuration\n").unwrap();
        let sentinel = root.parent().unwrap().join("outside-sentinel");
        fs::write(&sentinel, b"unchanged").unwrap();
        let env = environment(&root);
        let git = env.tool("git").unwrap().path().unwrap();
        let route = RepositoryRoute {
            root: root.clone(),
            git_dir: root.join(".git"),
            common_dir: root.join(".git"),
            configurations: Vec::new(),
        };
        let mut command = Command::new(git);
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_EXEC_PATH",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_0",
            "GIT_SSH_COMMAND",
            "GIT_EXTERNAL_DIFF",
        ] {
            command.env(key, &global);
        }
        configure_index_command(&mut command, &route, &env);
        let arguments = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(arguments.iter().any(|arg| arg == "core.fsmonitor="));
        assert!(!arguments.iter().any(|arg| arg == "core.fsmonitor=false"));
        assert!(arguments.ends_with(
            &[
                "ls-files",
                "--cached",
                "--stage",
                "--full-name",
                "--sparse",
                "-z"
            ]
            .map(str::to_string)
        ));
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_EXEC_PATH",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_0",
            "GIT_SSH_COMMAND",
            "GIT_EXTERNAL_DIFF",
        ] {
            assert!(!command
                .get_envs()
                .any(|(name, value)| name == key && value.is_some()));
        }
        assert!(command
            .get_envs()
            .any(|(name, value)| name == "GIT_ALLOW_PROTOCOL"
                && value == Some(std::ffi::OsStr::new(""))));
        assert!(command
            .get_envs()
            .any(|(name, value)| name == "GIT_NO_LAZY_FETCH"
                && value == Some(std::ffi::OsStr::new("1"))));
        let output = run_with_timeout_cancellable(command, GIT_TIMEOUT, &|| false).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            index_intersects_artifact(crate::PathFlavor::current(), &output.stdout, b"target"),
            Ok(false)
        );
        assert_eq!(fs::read(sentinel).unwrap(), b"unchanged");
        assert_eq!(probe(&root, &artifact), Evidence::VerifiedGenerated);
    }

    #[test]
    fn direct_marker_cannot_route_to_an_external_common_directory() {
        let (_temp, root, artifact) = repository();
        let outside = root.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(
            outside.join("config"),
            "[include]\npath=secret-keypair.json\n",
        )
        .unwrap();
        fs::write(
            root.join(".git/commondir"),
            outside.to_string_lossy().as_bytes(),
        )
        .unwrap();
        assert_eq!(
            probe_with(
                &environment(&root),
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("an unverified commondir must not run Git"),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::MalformedMetadata)
        );
    }

    #[test]
    fn case_variant_shared_index_metadata_is_refused_without_running_git() {
        let (_temp, root, artifact) = repository();
        fs::write(root.join(".git/SHAREDINDEX.fixture"), b"unsupported").unwrap();
        assert_eq!(
            probe_with(
                &environment(&root),
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("shared-index layouts must not launch Git"),
                &|| {},
            ),
            Evidence::Incomplete(Uncertainty::UnsupportedIndex)
        );
    }

    #[test]
    fn a_split_index_created_at_child_start_cannot_freshen_live_repository_metadata() {
        let (_temp, root, artifact) = repository();
        let result = probe_with(
            &environment(&root),
            &root,
            &artifact,
            &AtomicBool::new(false),
            MAX_ENTRIES,
            &|command, timeout, cancel| {
                let live_git_dir = root.join(".git");
                for name in ["GIT_COMMON_DIR", "GIT_INDEX_FILE"] {
                    let (_, value) = command.get_envs().find(|(key, _)| *key == name).unwrap();
                    assert!(!Path::new(value.unwrap()).starts_with(&live_git_dir));
                }
                let observed_shared = if is_index_query(&command) {
                    git(&root, &["update-index", "--split-index"]);
                    let shared = fs::read_dir(&live_git_dir)
                        .unwrap()
                        .map(Result::unwrap)
                        .find(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with("sharedindex.")
                        })
                        .unwrap()
                        .path();
                    let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
                    fs::File::options()
                        .write(true)
                        .open(&shared)
                        .unwrap()
                        .set_times(fs::FileTimes::new().set_modified(old_time))
                        .unwrap();
                    let before = stamp(&shared, &fs::metadata(&shared).unwrap()).unwrap();
                    Some((shared, before))
                } else {
                    None
                };
                let output = run_with_timeout_cancellable(command, timeout, &|| {
                    cancel.load(Ordering::Relaxed)
                })?;
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                if let Some((shared, before)) = observed_shared {
                    assert_eq!(
                        stamp(&shared, &fs::metadata(&shared).unwrap()).unwrap(),
                        before
                    );
                }
                Ok(output)
            },
            &|| {},
        );
        assert_eq!(
            result,
            Evidence::Incomplete(Uncertainty::ChangedDuringProbe)
        );
    }

    #[test]
    fn private_index_capture_is_bounded_and_never_writes_inside_the_artifact() {
        let (_temp, root, artifact) = repository();
        let env = environment(&root);
        assert_eq!(
            probe_with(
                &env.clone().with_temp_dir(&artifact),
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("an artifact cannot hold the probe's scratch repository"),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::OutsideScope)
        );
        fs::File::options()
            .write(true)
            .open(root.join(".git/index"))
            .unwrap()
            .set_len(MAX_INDEX_BYTES as u64 + 1)
            .unwrap();
        assert_eq!(
            probe_with(
                &env,
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("an oversized index must not reach Git"),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::BudgetExceeded)
        );
    }

    #[test]
    fn non_sha1_repositories_are_refused_before_any_git_child() {
        let (_temp, root, artifact) = repository();
        fs::remove_dir_all(root.join(".git")).unwrap();
        git(&root, &["init", "-q", "--object-format=sha256"]);
        git(
            &root,
            &["add", "--", "Cargo.toml", "target/assets/generated.bin"],
        );
        assert_eq!(
            probe_with(
                &environment(&root),
                &root,
                &artifact,
                &AtomicBool::new(false),
                MAX_ENTRIES,
                &|_, _, _| panic!("non-SHA-1 metadata must not reach the private SHA-1 decoder"),
                &|| {}
            ),
            Evidence::Incomplete(Uncertainty::UnsupportedIndex)
        );
    }

    #[test]
    fn scratch_metadata_cannot_normalize_broken_source_head_configuration_or_index() {
        for (path, contents) in [
            ("HEAD", "not a reference"),
            ("config", "[broken configuration\n"),
            ("config", "[core]\nbare=invalid\n"),
            ("config", "[extensions]\nworktreeConfig=invalid\n"),
        ] {
            let (_temp, root, artifact) = repository();
            fs::write(root.join(".git").join(path), contents).unwrap();
            assert_eq!(
                probe(&root, &artifact),
                Evidence::Incomplete(Uncertainty::MalformedMetadata),
                "{path}: {contents}"
            );
        }
        for zero_checksum in [false, true] {
            let (_temp, root, artifact) = repository();
            let path = root.join(".git/index");
            let mut bytes = fs::read(&path).unwrap();
            if zero_checksum {
                let start = bytes.len() - 20;
                bytes[start..].fill(0);
            } else {
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
            }
            fs::write(path, bytes).unwrap();
            assert_eq!(
                probe_with(
                    &environment(&root),
                    &root,
                    &artifact,
                    &AtomicBool::new(false),
                    MAX_ENTRIES,
                    &|_, _, _| panic!("unverified index integrity must not reach Git"),
                    &|| {}
                ),
                Evidence::Incomplete(if zero_checksum {
                    Uncertainty::UnsupportedIndex
                } else {
                    Uncertainty::MalformedMetadata
                })
            );
        }
    }

    #[test]
    fn original_repository_shape_and_worktree_routing_must_be_verified() {
        for name in ["objects", "refs"] {
            for replace_with_file in [false, true] {
                let (_temp, root, artifact) = repository();
                let path = root.join(".git").join(name);
                fs::remove_dir_all(&path).unwrap();
                if replace_with_file {
                    fs::write(&path, b"invalid shape").unwrap();
                }
                assert_eq!(
                    probe_with(
                        &environment(&root),
                        &root,
                        &artifact,
                        &AtomicBool::new(false),
                        MAX_ENTRIES,
                        &|_, _, _| panic!("broken repository directories must not reach Git"),
                        &|| {}
                    ),
                    Evidence::Incomplete(if replace_with_file {
                        Uncertainty::MalformedMetadata
                    } else {
                        Uncertainty::UnreadableMetadata
                    })
                );
            }
        }
        let (_temp, root, artifact) = repository();
        fs::write(root.join(".git/config"), "[core]\nworktree=/outside\n").unwrap();
        assert_eq!(
            probe(&root, &artifact),
            Evidence::Incomplete(Uncertainty::OutsideScope)
        );
    }

    #[test]
    fn nearer_valid_or_broken_repository_metadata_cannot_mask_ancestor_tracked_content() {
        let (_temp, root, _) = repository();
        let project = root.join("package");
        let artifact = project.join("target");
        fs::create_dir_all(&artifact).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname='nested'\n").unwrap();
        fs::write(artifact.join("authored.rs"), b"authored").unwrap();
        git(&root, &["add", "--", "package/target/authored.rs"]);
        git(&project, &["init", "-q"]);
        git(&project, &["add", "--", "Cargo.toml"]);
        assert_eq!(probe(&project, &artifact), Evidence::TrackedContent);
        fs::remove_dir_all(project.join(".git/objects")).unwrap();
        assert_eq!(
            probe(&project, &artifact),
            Evidence::Incomplete(Uncertainty::UnreadableMetadata)
        );
    }

    #[test]
    fn windows_index_path_ambiguities_fail_closed_on_every_runner() {
        let oid = "a".repeat(40);
        for path in [
            r"target\authored.rs",
            "target./authored.rs",
            "target /authored.rs",
            "TARGET~1/authored.rs",
            "target:stream",
            "C:target/file",
            "target/NUL",
        ] {
            let output = format!("100644 {oid} 0\t{path}\0");
            assert_eq!(
                index_intersects_artifact(crate::PathFlavor::Windows, output.as_bytes(), b"target"),
                Err(Uncertainty::MalformedMetadata)
            );
        }
        assert_eq!(
            index_intersects_artifact(crate::PathFlavor::Windows, b"", b"TARGET~1"),
            Err(Uncertainty::MalformedMetadata)
        );
        let output = format!("100644 {oid} 0\ttarget\\authored.rs\0");
        assert_eq!(
            index_intersects_artifact(crate::PathFlavor::Posix, output.as_bytes(), b"target"),
            Ok(false)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_native_file_ids_refuse_replacement_with_restored_creation_and_write_times() {
        use std::os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        };
        use windows_sys::Win32::{
            Foundation::FILETIME,
            Storage::FileSystem::{
                SetFileTime, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_WRITE_ATTRIBUTES,
            },
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("metadata");
        fs::create_dir(&path).unwrap();
        let metadata = fs::symlink_metadata(&path).unwrap();
        let before = stamp(&path, &metadata).unwrap();
        fs::rename(&path, temp.path().join("original-metadata")).unwrap();
        fs::create_dir(&path).unwrap();
        let handle = OpenOptions::new()
            .read(true)
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .unwrap();
        let time = |value: u64| FILETIME {
            dwLowDateTime: value as u32,
            dwHighDateTime: (value >> 32) as u32,
        };
        assert_ne!(
            unsafe {
                SetFileTime(
                    handle.as_raw_handle().cast(),
                    &time(metadata.creation_time()),
                    std::ptr::null(),
                    &time(metadata.last_write_time()),
                )
            },
            0
        );
        let restored = fs::symlink_metadata(&path).unwrap();
        assert_eq!(restored.creation_time(), metadata.creation_time());
        assert_eq!(restored.last_write_time(), metadata.last_write_time());
        let after = stamp(&path, &restored).unwrap();
        assert_eq!(before.size, after.size);
        assert_eq!(before.modified, after.modified);
        assert!(!same_entity(&before, &after));
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_git_metadata_ancestor_with_a_link_revokes_its_original_route() {
        let temp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let parent = base.join("metadata");
        let git_dir = parent.join("worktrees/project");
        fs::create_dir_all(&git_dir).unwrap();
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            started: Instant::now(),
            entries: 0,
            max_entries: MAX_ENTRIES,
            stamps: Vec::new(),
            component_entities: Vec::new(),
            repository_absences: Vec::new(),
            metadata_absences: Vec::new(),
            configurations: Vec::new(),
        };
        verify_components(&git_dir, &mut budget).unwrap();
        let original = stamp(&git_dir, &fs::metadata(&git_dir).unwrap()).unwrap();
        let moved = base.join("original-metadata");
        fs::rename(&parent, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &parent).unwrap();
        assert_eq!(
            stamp(&git_dir, &fs::metadata(&git_dir).unwrap()).unwrap(),
            original
        );
        assert_eq!(budget.unchanged(), Err(Uncertainty::ChangedDuringProbe));
    }
}
