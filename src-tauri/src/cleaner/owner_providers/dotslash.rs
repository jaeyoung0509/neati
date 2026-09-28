//! Completed DotSlash artifact directories available for direct cleanup.
//!
//! The owner has a whole-cache `clean` action but no selective age contract.
//! This adapter recognizes only complete hash-addressed artifact directories;
//! it never removes `locks`, temporary downloads, or arbitrary cache children.
//! Missing owner locks are created only at execution, not during discovery.
//! A modification timestamp means "unchanged", not "unused".

use super::OwnerScopedProvider;
use crate::models::{
    CleanFailureReason, OwnerProviderRefusal, OwnerProviderSelection, OwnerProviderUnit,
    OwnerStoreObservation, OwnerUnitObservation, OwnerUnitOutcome, OwnerUnitState, PlatformKind,
    ProviderStatus,
};
use crate::safety::owner_lock::OwnerFileLock;
use crate::safety::{Blacklist, SymlinkGuard, ToctouGuard};
use std::fs;
#[cfg(test)]
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, SystemTime};
use sysinfo::{ProcessesToUpdate, System};
use zenith_core::domain::cleanup::{
    OwnerProviderAuthorization, OwnerProviderExecution, OwnerUnitMeasurer, RunningProcessPolicy,
    RunningProcessProbe,
};
use zenith_core::domain::CleanupIdentity;
use zenith_platform::{PlatformEnvironment, TrashBackend};

const MAX_ARTIFACT_ENTRIES: usize = 100_000;
const MAX_ARTIFACT_DEPTH: usize = 64;

pub struct DotSlashArtifactsProvider {
    process: Arc<dyn RunningProcessProbe>,
    measuring: Arc<dyn OwnerUnitMeasurer>,
    trash: Arc<dyn TrashBackend>,
}

impl DotSlashArtifactsProvider {
    pub fn new(
        process: Arc<dyn RunningProcessProbe>,
        measuring: Arc<dyn OwnerUnitMeasurer>,
        trash: Arc<dyn TrashBackend>,
    ) -> Self {
        Self {
            process,
            measuring,
            trash,
        }
    }

    fn root(environment: &PlatformEnvironment) -> Option<PathBuf> {
        // An owner override can point anywhere, including a protected or
        // shared volume. Never inspect the default root as if it were active.
        if environment.cache_path_override("DOTSLASH_CACHE").is_some() {
            return None;
        }
        environment
            .user_home()
            .map(|home| home.join("Library/Caches/dotslash"))
    }

    fn read_store(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
    ) -> OwnerStoreObservation {
        let Some(root) = Self::root(environment) else {
            return OwnerStoreObservation::refused(
                ProviderStatus::Unsupported, None, "The profile has no supported default DotSlash cache root, or DOTSLASH_CACHE overrides it",
            );
        };
        match self.process.running(guard) {
            None => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    Some(root),
                    "The process table could not prove DotSlash is idle",
                )
            }
            Some(running) if !running.is_empty() => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::PrerequisiteNotMet,
                    Some(root),
                    "Close DotSlash before reviewing its cache",
                )
            }
            Some(_) => {}
        }
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return OwnerStoreObservation::ready(Some(root), Vec::new());
            }
            Err(error) => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    Some(root),
                    format!("The DotSlash cache could not be inspected: {error}"),
                )
            }
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    Some(root),
                    "The DotSlash cache root is not an ordinary directory",
                )
            }
            Ok(_) => {}
        }
        if SymlinkGuard::validate_anchored_path(&root, environment).is_err() {
            return OwnerStoreObservation::refused(
                ProviderStatus::Blocked,
                Some(root),
                "The DotSlash cache root cannot be safely resolved",
            );
        }
        let executables = match running_executables() {
            Some(paths) => paths,
            None => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    Some(root),
                    "Running executables could not be inspected",
                )
            }
        };
        let mut units = Vec::new();
        let prefixes = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    Some(root),
                    format!("The DotSlash cache could not be listed: {error}"),
                )
            }
        };
        for prefix in prefixes {
            let prefix = match prefix {
                Ok(entry) => entry,
                Err(error) => {
                    return OwnerStoreObservation::refused(
                        ProviderStatus::Blocked,
                        Some(root),
                        format!("The DotSlash cache listing was incomplete: {error}"),
                    )
                }
            };
            let prefix_name = prefix.file_name().to_string_lossy().into_owned();
            if prefix_name == "locks" {
                continue;
            }
            let prefix_path = prefix.path();
            if !hex_name(&prefix_name, 2) {
                // Downloads in progress and foreign entries are outside the
                // artifact contract. They remain visible as advisory bytes.
                let measurement = self.measuring.measure(&prefix_path);
                units.push(OwnerUnitObservation::advisory(
                    format!("other:{prefix_name}"),
                    prefix_path,
                    measurement.logical_bytes,
                    measurement.allocated_bytes,
                    measurement.entry_count,
                    "This cache entry is not a completed DotSlash artifact prefix",
                ));
                continue;
            }
            if !ordinary_directory(&prefix_path) {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    Some(root),
                    "A DotSlash artifact prefix is unreadable or is a link",
                );
            }
            let artifacts = match fs::read_dir(&prefix_path) {
                Ok(entries) => entries,
                Err(error) => {
                    return OwnerStoreObservation::refused(
                        ProviderStatus::Blocked,
                        Some(root),
                        format!("A DotSlash artifact prefix could not be listed: {error}"),
                    )
                }
            };
            for artifact in artifacts {
                let artifact = match artifact {
                    Ok(entry) => entry,
                    Err(error) => {
                        return OwnerStoreObservation::refused(
                            ProviderStatus::Blocked,
                            Some(root),
                            format!("A DotSlash artifact listing was incomplete: {error}"),
                        )
                    }
                };
                let name = artifact.file_name().to_string_lossy().into_owned();
                let path = artifact.path();
                let key = format!("{prefix_name}/{name}");
                let measurement = self.measuring.measure(&path);
                let result = if !hex_name(&name, 38) || !ordinary_directory(&path) {
                    Err(ArtifactCheck::Refused(
                        "This entry is not a complete hash-addressed artifact directory",
                    ))
                } else if artifact_is_running(&path, &executables) {
                    Err(ArtifactCheck::Refused(
                        "An executable inside this artifact is running",
                    ))
                } else if OwnerFileLock::inspect(
                    &root,
                    &["locks", &prefix_name, &name],
                    environment,
                )
                .is_err()
                {
                    Err(ArtifactCheck::Refused(
                        "The artifact's owner lock path contains an unsafe or unreadable entry",
                    ))
                } else {
                    inspect_artifact(&path)
                };
                let unit = if !measurement.complete {
                    OwnerUnitObservation::blocked(
                        &key,
                        path,
                        measurement.logical_bytes,
                        measurement.allocated_bytes,
                        measurement.entry_count,
                        measurement.detail.unwrap_or_else(|| {
                            "This artifact could not be completely measured".into()
                        }),
                    )
                } else {
                    match result {
                        Ok(()) if measurement.allocated_bytes > 0 => OwnerUnitObservation::ready(
                            &key,
                            path,
                            measurement.logical_bytes,
                            measurement.allocated_bytes,
                            measurement.entry_count,
                        ),
                        Ok(()) => OwnerUnitObservation::refused(
                            &key,
                            path,
                            measurement.logical_bytes,
                            measurement.allocated_bytes,
                            measurement.entry_count,
                            "This artifact has no measured allocated bytes",
                        ),
                        Err(ArtifactCheck::Refused(detail)) => OwnerUnitObservation::refused(
                            &key,
                            path,
                            measurement.logical_bytes,
                            measurement.allocated_bytes,
                            measurement.entry_count,
                            detail,
                        ),
                        Err(ArtifactCheck::Incomplete(detail)) => OwnerUnitObservation::blocked(
                            &key,
                            path,
                            measurement.logical_bytes,
                            measurement.allocated_bytes,
                            measurement.entry_count,
                            detail,
                        ),
                    }
                };
                units.push(unit);
            }
        }
        OwnerStoreObservation::ready(Some(root), units)
    }

    fn prepare_units(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
        selections: &[OwnerProviderSelection],
    ) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
        let observed = self.read_store(environment, guard);
        if !observed.status.is_ready() {
            return Err(OwnerProviderRefusal::for_selections(
                observed.status,
                observed
                    .detail
                    .unwrap_or_else(|| "DotSlash cache unavailable".into()),
                Vec::new(),
            ));
        }
        let root = observed.root.expect("ready DotSlash observation has root");
        let mut plan = OwnerProviderAuthorization {
            deletion_disposition: zenith_core::domain::cleanup::DeletionDisposition::Trash,
            signature_id: String::new(),
            provider_id: self.id().into(),
            risk: crate::models::RiskTier::Rebuild,
            units: Vec::new(),
            refusals: Vec::new(),
            process_guard: guard.clone(),
            requires_confirmation: false,
        };
        for selection in selections {
            let found = observed.units.iter().find(|unit| {
                unit.path == selection.path
                    && unit.state == OwnerUnitState::Ready
                    && unit.allocated_bytes == selection.expected_bytes
            });
            let Some(unit) = found else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "The DotSlash artifact changed or is no longer eligible; scan again"
                        .into(),
                });
                continue;
            };
            let Some(identity) = ToctouGuard::capture(&unit.path) else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "The artifact's filesystem identity could not be captured".into(),
                });
                continue;
            };
            plan.units.push(OwnerProviderUnit {
                item_id: selection.item_id.clone(),
                unit_key: unit.unit_key.clone(),
                name: selection.name.clone(),
                root: root.clone(),
                path: unit.path.clone(),
                identity,
                expected_bytes: unit.allocated_bytes,
                entry_count: unit.entry_count,
            });
        }
        if plan.units.is_empty() {
            return Err(OwnerProviderRefusal::for_selections(
                ProviderStatus::Blocked,
                "No selected DotSlash artifact is still eligible",
                plan.refusals,
            ));
        }
        Ok(plan)
    }

    fn move_unit(
        &self,
        environment: &PlatformEnvironment,
        unit: &OwnerProviderUnit,
    ) -> OwnerUnitOutcome {
        let refuse = |detail: String| {
            OwnerUnitOutcome::refused(
                unit.item_id.clone(),
                unit.unit_key.clone(),
                ProviderStatus::Blocked,
                detail,
            )
        };
        let Some(root) = Self::root(environment) else {
            return refuse("The DotSlash root is unavailable".into());
        };
        let Some((prefix, artifact)) = unit.unit_key.split_once('/') else {
            return refuse("The artifact key is invalid".into());
        };
        if !hex_name(prefix, 2)
            || !hex_name(artifact, 38)
            || unit.root != root
            || unit.path != root.join(prefix).join(artifact)
        {
            return refuse("The artifact is outside DotSlash's verified cache layout".into());
        }
        if let Err(error) = Blacklist::validate_with(&unit.path, environment) {
            return refuse(format!("The artifact path is protected: {error}"));
        }
        if let Err(error) = SymlinkGuard::validate_anchored_path(&unit.path, environment) {
            return refuse(format!("The artifact path is not safe: {error}"));
        }
        if let Err(error) =
            SymlinkGuard::validate_canonical_blacklist_strict(&unit.path, environment)
        {
            return refuse(format!("The resolved artifact path is protected: {error}"));
        }
        let lock = match OwnerFileLock::acquire(&root, &["locks", prefix, artifact], environment) {
            Ok(lock) => lock,
            Err(error) => {
                return refuse(format!("The DotSlash lock could not be acquired: {error}"))
            }
        };
        if let Err(error) = ToctouGuard::verify(&unit.path, &unit.identity) {
            return refuse(format!("The artifact changed since review: {error}"));
        }
        if let Err(detail) = inspect_artifact(&unit.path) {
            return refuse(detail.detail());
        }
        let measurement = self.measuring.measure(&unit.path);
        if !measurement.complete || measurement.allocated_bytes != unit.expected_bytes {
            return refuse("The artifact measurement changed; scan again".into());
        }
        match running_executables() {
            None => return refuse("Running executables could not be inspected".into()),
            Some(paths) if artifact_is_running(&unit.path, &paths) => {
                return refuse("An executable inside this artifact is running".into())
            }
            Some(_) => {}
        }
        if let Err(error) = lock.verify(environment) {
            return refuse(format!(
                "The DotSlash lock changed before the move: {error}"
            ));
        }
        if let Err(error) = ToctouGuard::verify(&unit.path, &unit.identity) {
            return refuse(format!("The artifact changed before the move: {error}"));
        }
        let mut permissions = match ArtifactTrashPermissions::prepare(&unit.path, &unit.identity) {
            Ok(permissions) => permissions,
            Err(error) => return refuse(error),
        };
        let move_result = self.trash.move_to_trash(&unit.path);
        let restore_result = permissions.restore();
        if let Err(error) = move_result {
            let restore_detail = restore_result
                .err()
                .map(|restore| {
                    format!("; the original permissions could not be restored: {restore}")
                })
                .unwrap_or_default();
            return refuse(format!(
                "The artifact could not be moved to Trash: {error}{restore_detail}"
            ));
        }
        if let Err(error) = restore_result {
            crate::diagnostics::log_error(
                "dotslash-cleanup",
                &format!(
                    "The trashed DotSlash artifact permissions could not be restored: {error}"
                ),
            );
        }
        match fs::symlink_metadata(&unit.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                OwnerUnitOutcome::trashed(
                    unit.item_id.clone(),
                    unit.unit_key.clone(),
                    measurement.allocated_bytes,
                )
            }
            _ => OwnerUnitOutcome::partially_cleaned(
                unit.item_id.clone(),
                unit.unit_key.clone(),
                0,
                None,
                "The artifact still exists after the Trash move; a replacement was left in place",
            ),
        }
    }
}

impl OwnerScopedProvider for DotSlashArtifactsProvider {
    fn id(&self) -> &'static str {
        "dotslash.stale_artifacts"
    }
    fn platforms(&self) -> &'static [PlatformKind] {
        &[PlatformKind::Macos]
    }
    fn consequence(&self) -> &'static str {
        "The selected artifact moves to Trash. Empty Trash to free disk space. DotSlash may download and unpack it again when needed."
    }
    fn requires_confirmation(&self) -> bool {
        false
    }
    fn deletion_disposition(&self) -> zenith_core::domain::cleanup::DeletionDisposition {
        zenith_core::domain::cleanup::DeletionDisposition::Trash
    }
    fn unit_label(&self, unit: &OwnerUnitObservation) -> String {
        let key = unit.unit_key.as_str();
        if key.len() > 18 && !key.starts_with("other:") {
            format!("{}…", &key[..18])
        } else {
            key.to_string()
        }
    }
    fn scan(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
    ) -> OwnerStoreObservation {
        self.read_store(environment, guard)
    }
    fn prepare(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
        selections: &[OwnerProviderSelection],
    ) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
        self.prepare_units(environment, guard, selections)
    }
    fn execute(
        &self,
        environment: &PlatformEnvironment,
        authorization: &OwnerProviderAuthorization,
    ) -> OwnerProviderExecution {
        let running = self.process.running(&authorization.process_guard);
        let refusal = match running {
            None => Some("The process table could not prove DotSlash is idle".to_string()),
            Some(running) if !running.is_empty() => {
                Some("Close DotSlash before moving its artifacts".to_string())
            }
            Some(_) => None,
        };
        OwnerProviderExecution {
            units: authorization
                .units
                .iter()
                .map(|unit| match &refusal {
                    Some(detail) => OwnerUnitOutcome::refused(
                        unit.item_id.clone(),
                        unit.unit_key.clone(),
                        ProviderStatus::PrerequisiteNotMet,
                        detail.clone(),
                    ),
                    None => self.move_unit(environment, unit),
                })
                .collect(),
        }
    }
}

fn hex_name(name: &str, length: usize) -> bool {
    name.len() == length
        && name
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn ordinary_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
}

/// Refuses unreadable, escaping, special, empty, or oversized artifact trees.
#[derive(Debug, Clone, Copy)]
enum ArtifactCheck {
    Refused(&'static str),
    Incomplete(&'static str),
}

impl ArtifactCheck {
    fn detail(self) -> String {
        match self {
            Self::Refused(detail) | Self::Incomplete(detail) => detail.to_string(),
        }
    }
}

fn inspect_artifact(path: &Path) -> Result<(), ArtifactCheck> {
    let mut seen = 0usize;
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .max_depth(MAX_ARTIFACT_DEPTH)
    {
        let entry = entry.map_err(|_| {
            ArtifactCheck::Incomplete("The artifact could not be completely inspected")
        })?;
        seen += 1;
        if seen > MAX_ARTIFACT_ENTRIES {
            return Err(ArtifactCheck::Incomplete(
                "The artifact exceeds the inspection limit",
            ));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| ArtifactCheck::Incomplete("An artifact entry could not be inspected"))?;
        if metadata.file_type().is_symlink() {
            if entry.depth() == 0 {
                return Err(ArtifactCheck::Refused("The artifact root is a link"));
            }
            // Whole-artifact Trash preserves the link itself. Never follow it.
            // Forward-only relative archive links (framework Versions/Current)
            // stay confined even in a chain; parent traversal is not supported.
            let target = fs::read_link(entry.path())
                .map_err(|_| ArtifactCheck::Incomplete("An archive link could not be inspected"))?;
            if target.as_os_str().is_empty()
                || target
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
            {
                return Err(ArtifactCheck::Refused(
                    "The artifact contains an absolute or parent-traversing archive link",
                ));
            }
            continue;
        }
        if !(metadata.is_dir() || metadata.is_file()) {
            return Err(ArtifactCheck::Refused(
                "The artifact contains a special entry",
            ));
        }
        if metadata.is_file() && !single_link(&metadata) {
            return Err(ArtifactCheck::Refused(
                "The artifact contains a file linked outside this object",
            ));
        }
        if entry.depth() == MAX_ARTIFACT_DEPTH && metadata.is_dir() {
            return Err(ArtifactCheck::Incomplete(
                "The artifact exceeds the depth limit",
            ));
        }
    }
    if seen <= 1 {
        return Err(ArtifactCheck::Refused(
            "The artifact directory has no measured contents",
        ));
    }
    Ok(())
}

fn running_executables() -> Option<Vec<PathBuf>> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    if system.processes().is_empty() {
        return None;
    }
    Some(
        system
            .processes()
            .values()
            .filter_map(|process| process.exe().map(Path::to_path_buf))
            .collect(),
    )
}

fn artifact_is_running(path: &Path, executables: &[PathBuf]) -> bool {
    executables.iter().any(|exe| exe.starts_with(path))
}

/// DotSlash deliberately publishes immutable artifact trees. macOS Trash APIs
/// require the moved directory itself to be owner-writable, so hold the exact
/// directory open while temporarily adding that one permission bit. Restoring
/// through the handle also restores the moved object after its pathname has
/// changed to the Trash location.
struct ArtifactTrashPermissions {
    #[cfg(unix)]
    directory: fs::File,
    #[cfg(unix)]
    original_mode: u32,
    #[cfg(unix)]
    changed: bool,
}

impl ArtifactTrashPermissions {
    fn prepare(path: &Path, expected: &CleanupIdentity) -> Result<Self, String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};

            let directory = fs::File::open(path).map_err(|error| {
                format!("The artifact could not be opened before the Trash move: {error}")
            })?;
            let metadata = directory.metadata().map_err(|error| {
                format!("The artifact identity could not be read before the Trash move: {error}")
            })?;
            let expected_entity = expected.entity();
            if !metadata.is_dir()
                || metadata.dev() != expected_entity.device()
                || metadata.ino() != expected_entity.inode()
            {
                return Err(
                    "The artifact changed before its Trash permissions were prepared".into(),
                );
            }

            let original_mode = metadata.permissions().mode();
            let changed = original_mode & 0o200 == 0;
            let mut permissions = Self {
                directory,
                original_mode,
                changed,
            };
            if changed {
                permissions
                    .directory
                    .set_permissions(fs::Permissions::from_mode(original_mode | 0o200))
                    .map_err(|error| {
                        format!(
                            "The read-only artifact could not be prepared for the Trash move: {error}"
                        )
                    })?;
            }
            if let Err(error) = ToctouGuard::verify_entity(path, expected) {
                let _ = permissions.restore();
                return Err(format!(
                    "The artifact changed while its Trash permissions were prepared: {error}"
                ));
            }
            Ok(permissions)
        }

        #[cfg(not(unix))]
        {
            ToctouGuard::verify_entity(path, expected)
                .map_err(|error| format!("The artifact changed before the Trash move: {error}"))?;
            Ok(Self {})
        }
    }

    fn restore(&mut self) -> Result<(), String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if self.changed {
                self.directory
                    .set_permissions(fs::Permissions::from_mode(self.original_mode))
                    .map_err(|error| error.to_string())?;
                self.changed = false;
            }
        }
        Ok(())
    }
}

impl Drop for ArtifactTrashPermissions {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(unix)]
fn single_link(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() == 1
}

#[cfg(not(unix))]
fn single_link(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::SizeCalculatorMeasurement;

    struct Idle;
    impl RunningProcessProbe for Idle {
        fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(Vec::new())
        }
    }

    struct Busy;
    impl RunningProcessProbe for Busy {
        fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(vec!["dotslash".into()])
        }
    }

    struct FixtureTrash(PathBuf);
    impl TrashBackend for FixtureTrash {
        fn move_to_trash(&self, path: &Path) -> Result<(), String> {
            fs::rename(path, self.0.join(path.file_name().unwrap()))
                .map_err(|error| error.to_string())
        }
    }

    fn fixture() -> (
        tempfile::TempDir,
        PlatformEnvironment,
        DotSlashArtifactsProvider,
        RunningProcessPolicy,
        PathBuf,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let environment = PlatformEnvironment::native().with_home(temp.path());
        let trashed = temp.path().join("trashed");
        fs::create_dir(&trashed).unwrap();
        let provider = DotSlashArtifactsProvider::new(
            Arc::new(Idle),
            Arc::new(SizeCalculatorMeasurement),
            Arc::new(FixtureTrash(trashed.clone())),
        );
        let guard = RunningProcessPolicy::guarding(vec!["dotslash".into()]);
        (temp, environment, provider, guard, trashed)
    }

    fn artifact(environment: &PlatformEnvironment, prefix: &str, rest: &str, old: bool) -> PathBuf {
        let root = DotSlashArtifactsProvider::root(environment).unwrap();
        let path = root.join(prefix).join(rest);
        let lock = root.join("locks").join(prefix).join(rest);
        fs::create_dir_all(&path).unwrap();
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, b"").unwrap();
        let payload = path.join("tool");
        fs::write(&payload, b"downloaded executable").unwrap();
        if old {
            let timestamp = SystemTime::now() - Duration::from_secs(31 * 86_400);
            File::open(&payload)
                .unwrap()
                .set_modified(timestamp)
                .unwrap();
            File::open(&path).unwrap().set_modified(timestamp).unwrap();
        }
        path
    }

    #[test]
    fn complete_artifacts_are_eligible_regardless_of_age_and_only_selected_objects_move() {
        let (_temp, environment, provider, guard, trashed) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let recent = artifact(&environment, "ab", &"d".repeat(38), false);
        let observed = provider.scan(&environment, &guard);
        let ready = observed.units.iter().find(|unit| unit.path == old).unwrap();
        assert_eq!(ready.state, OwnerUnitState::Ready);
        assert_eq!(
            observed
                .units
                .iter()
                .find(|unit| unit.path == recent)
                .unwrap()
                .state,
            OwnerUnitState::Ready
        );
        let selection = OwnerProviderSelection {
            item_id: "old".into(),
            name: "old".into(),
            path: old.clone(),
            expected_bytes: ready.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
        assert_eq!(plan.deletion_disposition, provider.deletion_disposition());
        assert_eq!(result.units[0].reclaimed_bytes, 0);
        assert_eq!(result.units[0].moved_to_trash_bytes, ready.allocated_bytes);
        assert_eq!(result.reclaimed_bytes(), 0);
        assert!(!old.exists());
        assert!(trashed.join("c".repeat(38)).exists());
        assert!(recent.exists());
    }

    #[cfg(unix)]
    #[test]
    fn read_only_artifact_is_restored_after_the_trash_move() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp, environment, provider, guard, trashed) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        fs::set_permissions(&old, fs::Permissions::from_mode(0o555)).unwrap();
        let ready = provider.scan(&environment, &guard).units.remove(0);
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &[OwnerProviderSelection {
                    item_id: "read-only".into(),
                    name: "read-only".into(),
                    path: old.clone(),
                    expected_bytes: ready.allocated_bytes,
                }],
            )
            .unwrap();

        let result = provider.execute(&environment, &plan);

        assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
        assert!(!old.exists());
        let moved = trashed.join("c".repeat(38));
        assert_eq!(
            fs::metadata(moved).unwrap().permissions().mode() & 0o777,
            0o555
        );
    }

    #[test]
    fn active_owner_and_active_artifact_executable_are_refused() {
        let (_temp, environment, provider, guard, _) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let busy = DotSlashArtifactsProvider::new(
            Arc::new(Busy),
            Arc::new(SizeCalculatorMeasurement),
            provider.trash.clone(),
        );
        assert_eq!(
            busy.scan(&environment, &guard).status,
            ProviderStatus::PrerequisiteNotMet
        );
        assert!(artifact_is_running(&old, &[old.join("tool")]));
        assert!(!artifact_is_running(
            &old,
            &[old.with_file_name("other").join("tool")]
        ));
    }

    #[test]
    fn linked_and_changed_artifacts_are_not_offered() {
        let (_temp, environment, provider, guard, _) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        std::os::unix::fs::symlink(old.join("tool"), old.join("link")).unwrap();
        File::open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(31 * 86_400))
            .unwrap();
        let observed = provider.scan(&environment, &guard);
        assert_eq!(observed.units[0].state, OwnerUnitState::Refused);
        fs::remove_file(old.join("link")).unwrap();
        File::open(&old)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(31 * 86_400))
            .unwrap();
        let ready = provider.scan(&environment, &guard).units.remove(0);
        assert_eq!(ready.state, OwnerUnitState::Ready);
        let selection = OwnerProviderSelection {
            item_id: "old".into(),
            name: "old".into(),
            path: old.clone(),
            expected_bytes: ready.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        fs::write(old.join("new"), b"recent").unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Blocked);
        assert!(old.exists());
    }

    #[test]
    fn absent_owner_metadata_is_read_only_during_scan_and_created_before_the_move() {
        for missing_tree in [false, true] {
            let (_temp, environment, provider, guard, trashed) = fixture();
            let old = artifact(&environment, "ab", &"c".repeat(38), true);
            let root = DotSlashArtifactsProvider::root(&environment).unwrap();
            let lock = root.join("locks/ab").join("c".repeat(38));
            if missing_tree {
                fs::remove_dir_all(root.join("locks")).unwrap();
            } else {
                fs::remove_file(&lock).unwrap();
            }
            let observed = provider.scan(&environment, &guard);
            let ready = observed.units.iter().find(|unit| unit.path == old).unwrap();
            assert_eq!(ready.state, OwnerUnitState::Ready);
            let plan = provider
                .prepare(
                    &environment,
                    &guard,
                    &[OwnerProviderSelection {
                        item_id: "missing-lock".into(),
                        name: "artifact".into(),
                        path: old.clone(),
                        expected_bytes: ready.allocated_bytes,
                    }],
                )
                .unwrap();
            assert!(!lock.exists());
            if missing_tree {
                assert!(!root.join("locks").exists());
            }
            let result = provider.execute(&environment, &plan);
            assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
            assert!(lock.is_file());
            assert!(!old.exists());
            assert!(trashed.join("c".repeat(38)).join("tool").exists());
        }
    }

    #[test]
    fn missing_lock_does_not_allow_linked_metadata_or_payloads() {
        let (temp, environment, provider, guard, _) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let root = DotSlashArtifactsProvider::root(&environment).unwrap();
        fs::remove_dir_all(root.join("locks")).unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("locks")).unwrap();
        assert_eq!(
            provider.scan(&environment, &guard).units[0].state,
            OwnerUnitState::Refused
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_file(root.join("locks")).unwrap();
        fs::hard_link(old.join("tool"), outside.join("payload")).unwrap();
        assert_eq!(
            provider.scan(&environment, &guard).units[0].state,
            OwnerUnitState::Refused
        );
        assert!(!root.join("locks").exists());
    }

    #[test]
    fn an_owner_starting_after_planning_still_blocks_missing_lock_cleanup() {
        let (_temp, environment, provider, guard, _) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let root = DotSlashArtifactsProvider::root(&environment).unwrap();
        fs::remove_dir_all(root.join("locks")).unwrap();
        let ready = provider.scan(&environment, &guard).units.remove(0);
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &[OwnerProviderSelection {
                    item_id: "running-owner".into(),
                    name: "artifact".into(),
                    path: old.clone(),
                    expected_bytes: ready.allocated_bytes,
                }],
            )
            .unwrap();
        let busy =
            DotSlashArtifactsProvider::new(Arc::new(Busy), provider.measuring, provider.trash);
        let result = busy.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::PrerequisiteNotMet);
        assert!(old.exists());
        assert!(!root.join("locks").exists());
    }

    #[test]
    fn whole_artifact_trash_preserves_forward_only_framework_links_without_following_them() {
        let (_temp, environment, provider, guard, trashed) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let framework = old.join("Tool.app/Contents/Frameworks/Tool.framework");
        fs::create_dir_all(framework.join("Versions/A/Resources")).unwrap();
        fs::write(framework.join("Versions/A/Tool"), b"framework executable").unwrap();
        std::os::unix::fs::symlink("A", framework.join("Versions/Current")).unwrap();
        std::os::unix::fs::symlink("Versions/Current/Tool", framework.join("Tool")).unwrap();
        std::os::unix::fs::symlink("Versions/Current/Resources", framework.join("Resources"))
            .unwrap();
        // A cycle and a dangling forward link cannot cause traversal or a hang.
        std::os::unix::fs::symlink("cycle", old.join("cycle")).unwrap();
        std::os::unix::fs::symlink("missing", old.join("dangling")).unwrap();
        let ready = provider.scan(&environment, &guard).units.remove(0);
        assert_eq!(ready.state, OwnerUnitState::Ready);
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &[OwnerProviderSelection {
                    item_id: "framework".into(),
                    name: "artifact".into(),
                    path: old.clone(),
                    expected_bytes: ready.allocated_bytes,
                }],
            )
            .unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
        assert!(!old.exists());
        let moved = trashed.join("c".repeat(38));
        let framework = moved.join("Tool.app/Contents/Frameworks/Tool.framework");
        assert_eq!(
            fs::read_link(framework.join("Tool")).unwrap(),
            Path::new("Versions/Current/Tool")
        );
        assert_eq!(
            fs::read_link(framework.join("Versions/Current")).unwrap(),
            Path::new("A")
        );
        assert_eq!(
            fs::read_link(moved.join("cycle")).unwrap(),
            Path::new("cycle")
        );
        assert_eq!(
            fs::read_link(moved.join("dangling")).unwrap(),
            Path::new("missing")
        );
    }

    #[test]
    fn a_linked_artifact_root_is_never_followed_by_the_inspector() {
        let (temp, environment, _provider, _guard, _) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let link = temp.path().join("root-link");
        std::os::unix::fs::symlink(&old, &link).unwrap();
        assert!(matches!(
            inspect_artifact(&link),
            Err(ArtifactCheck::Refused("The artifact root is a link"))
        ));
    }

    #[test]
    fn external_absolute_and_parent_traversing_links_added_after_planning_block_the_move() {
        for target in [
            "/outside",
            "../outside",
            "nested/../../outside",
            "nested/../tool",
        ] {
            let (temp, environment, provider, guard, trashed) = fixture();
            let old = artifact(&environment, "ab", &"c".repeat(38), true);
            let outside = temp.path().join("outside");
            fs::write(&outside, b"must remain").unwrap();
            std::os::unix::fs::symlink("tool", old.join("link")).unwrap();
            let ready = provider.scan(&environment, &guard).units.remove(0);
            let plan = provider
                .prepare(
                    &environment,
                    &guard,
                    &[OwnerProviderSelection {
                        item_id: "changed-link".into(),
                        name: "artifact".into(),
                        path: old.clone(),
                        expected_bytes: ready.allocated_bytes,
                    }],
                )
                .unwrap();
            fs::remove_file(old.join("link")).unwrap();
            std::os::unix::fs::symlink(target, old.join("link")).unwrap();
            assert_eq!(
                provider.scan(&environment, &guard).units[0].state,
                OwnerUnitState::Refused
            );
            assert_eq!(
                provider.execute(&environment, &plan).units[0].status,
                ProviderStatus::Blocked
            );
            assert!(old.join("tool").exists());
            assert_eq!(fs::read(&outside).unwrap(), b"must remain");
            assert_eq!(fs::read_dir(&trashed).unwrap().count(), 0);
        }
    }

    #[test]
    fn an_owner_lock_acquired_after_review_blocks_the_trash_move() {
        let (_temp, environment, provider, guard, _) = fixture();
        let old = artifact(&environment, "ab", &"c".repeat(38), true);
        let ready = provider.scan(&environment, &guard).units.remove(0);
        let selection = OwnerProviderSelection {
            item_id: "old".into(),
            name: "old".into(),
            path: old.clone(),
            expected_bytes: ready.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        let lock_path = DotSlashArtifactsProvider::root(&environment)
            .unwrap()
            .join("locks/ab")
            .join("c".repeat(38));
        let lock = File::options()
            .read(true)
            .write(true)
            .open(lock_path)
            .unwrap();
        lock.lock().unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Blocked);
        assert!(old.exists());
    }

    #[test]
    fn incomplete_measurement_is_visible_but_not_reviewable() {
        struct Partial;
        impl OwnerUnitMeasurer for Partial {
            fn measure(&self, _path: &Path) -> zenith_core::domain::cleanup::OwnerUnitMeasurement {
                zenith_core::domain::cleanup::OwnerUnitMeasurement::partial(
                    10,
                    10,
                    1,
                    "An entry could not be measured",
                )
            }
        }
        let (_temp, environment, provider, guard, _) = fixture();
        artifact(&environment, "ab", &"c".repeat(38), true);
        let provider =
            DotSlashArtifactsProvider::new(Arc::new(Idle), Arc::new(Partial), provider.trash);
        let observed = provider.scan(&environment, &guard);
        assert_eq!(observed.units[0].state, OwnerUnitState::Blocked);
        assert_eq!(observed.units[0].allocated_bytes, 10);
    }

    #[test]
    fn a_custom_cache_root_is_not_mistaken_for_the_default_store() {
        let (_temp, environment, provider, guard, _) = fixture();
        let overridden = environment.with_cache_path_override("DOTSLASH_CACHE", "/other/cache");
        assert_eq!(
            provider.scan(&overridden, &guard).status,
            ProviderStatus::Unsupported
        );
    }

    #[test]
    fn completed_artifacts_are_directly_cleanable_without_intensive_mode() {
        let (_temp, environment, provider, _guard, _) = fixture();
        artifact(&environment, "ab", &"c".repeat(38), true);
        artifact(&environment, "ab", &"d".repeat(38), false);
        let registry = crate::signatures::SignatureRegistry::load_embedded().unwrap();
        let providers = super::super::OwnerProviderRegistry::new(vec![Arc::new(provider)]);
        let off = providers.scan_items(
            &registry,
            crate::models::Category::Developer,
            false,
            &[],
            &environment,
        );
        let on_items = providers.scan_items(
            &registry,
            crate::models::Category::Developer,
            true,
            &[],
            &environment,
        );
        let off = off
            .iter()
            .find(|item| item.signature_id == "dev.dotslash.stale_artifacts")
            .unwrap();
        let on = on_items
            .iter()
            .find(|item| item.id.ends_with(&"c".repeat(38)))
            .unwrap();
        assert!(off.disposition.eligibility.is_auto_cleanable());
        assert_eq!(
            on.disposition.eligibility,
            crate::models::CleanupEligibility::AutoCleanable
        );
        assert!(off.has_current_disposition());
        assert!(on.has_current_disposition());
        assert!(on_items.iter().all(|item| item.has_current_disposition()));
        assert!(on_items
            .iter()
            .all(|item| item.disposition.eligibility.is_auto_cleanable()));
        assert!(on.is_selected);
    }
}
