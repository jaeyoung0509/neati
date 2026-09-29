//! Fixed-argument Tool cleanup. The preview is bound to exact candidate
//! identities and re-read at execution. There is no filesystem deletion path.
use super::OwnerScopedProvider;
use crate::models::{
    CleanFailureReason, OwnerProviderRefusal, OwnerProviderSelection, OwnerProviderUnit,
    OwnerStoreObservation, OwnerUnitObservation, OwnerUnitOutcome, OwnerUnitState, PlatformKind,
    ProviderStatus,
};
use crate::safety::{Blacklist, SymlinkGuard, ToctouGuard};
use neati_core::domain::cleanup::{
    OwnerProviderAuthorization, OwnerProviderExecution, OwnerUnitMeasurer, RunningProcessPolicy,
    RunningProcessProbe,
};
use neati_platform::PlatformEnvironment;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

const CLEAN_ARGS: &[&str] = &[
    "clean",
    "--yes",
    "--index-cache",
    "--tarballs",
    "--logfiles",
    "--json",
];
const MAX_CANDIDATES: usize = 10_000;
#[derive(Clone, Debug)]
struct ToolPreview {
    executable: PathBuf,
    executable_identity: crate::models::CleanupIdentity,
    prefix: PathBuf,
    candidates: Vec<PathBuf>,
    estimated_bytes: u64,
    fingerprint: String,
}
impl ToolPreview {
    fn key(&self) -> String {
        self.fingerprint.clone()
    }
}
trait ToolCommandRunner: Send + Sync {
    fn preview(&self, environment: &PlatformEnvironment) -> Result<ToolPreview, String>;
    fn cleanup(
        &self,
        environment: &PlatformEnvironment,
        preview: &ToolPreview,
        processes: &dyn RunningProcessProbe,
        guard: &RunningProcessPolicy,
    ) -> Result<(), String>;
}
#[derive(Clone, Copy, Debug)]
pub enum ToolCacheKind {
    Conda,
    Mise,
    Swiftpm,
}
impl ToolCacheKind {
    fn executable(self) -> &'static str {
        match self {
            Self::Conda => "conda",
            Self::Mise => "mise",
            Self::Swiftpm => "swift-package",
        }
    }
}
struct NativeToolCommandRunner {
    kind: ToolCacheKind,
}
impl NativeToolCommandRunner {
    fn executable(&self, environment: &PlatformEnvironment) -> Result<PathBuf, String> {
        let path = if matches!(self.kind, ToolCacheKind::Swiftpm) {
            swiftpm_executable(environment)?
        } else {
            crate::tooling::resolve_with(self.kind.executable(), environment)
                .ok_or("Tool is not installed")?
        };
        let canonical = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
        let mut roots = vec![
            PathBuf::from("/opt/homebrew"),
            PathBuf::from("/opt/anaconda3"),
            PathBuf::from("/opt/miniconda3"),
        ];
        if let Some(home) = environment.user_home() {
            roots.extend(
                ["miniconda3", "anaconda3", "miniforge3", "mambaforge"].map(|name| home.join(name)),
            );
        }
        if matches!(self.kind, ToolCacheKind::Mise) {
            roots = vec![
                PathBuf::from("/opt/homebrew"),
                PathBuf::from("/usr/local/bin"),
            ];
            if let Some(home) = environment.user_home() {
                roots.push(home.join(".local/bin"));
            }
        }
        if matches!(self.kind, ToolCacheKind::Swiftpm) {
            roots = vec![PathBuf::from("/Library/Developer/CommandLineTools/usr/bin"), PathBuf::from("/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin")];
        }
        if !roots.iter().any(|root| {
            canonical.starts_with(std::fs::canonicalize(root).unwrap_or_else(|_| root.clone()))
        }) || !canonical.is_file()
        {
            return Err("Tool executable is outside supported installation roots".into());
        }
        Ok(canonical)
    }
    fn command(
        &self,
        environment: &PlatformEnvironment,
        executable: &Path,
        dry_run: bool,
    ) -> Result<Command, String> {
        let home = environment.user_home().ok_or("No user home for Conda")?;
        let mut command = Command::new(executable);
        match self.kind {
            ToolCacheKind::Swiftpm => {
                command.arg("--version");
            }
            ToolCacheKind::Conda => {
                command.args(CLEAN_ARGS);
                if dry_run {
                    command.arg("--dry-run");
                }
            }
            ToolCacheKind::Mise => {
                // Inventory must not refill the update cache or prune unrelated
                // entries as a side effect of doctor. Keep preview and mutation
                // in the same explicit environment.
                command.args(if dry_run {
                    &["doctor", "--json"][..]
                } else {
                    &["cache", "clear"][..]
                });
            }
        }
        command
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("CONDA_NO_PLUGINS", "true")
            .env("NO_COLOR", "1")
            .current_dir(&home);
        if matches!(self.kind, ToolCacheKind::Mise) {
            command
                .env("MISE_DISABLE_UPDATE_WARNING", "1")
                .env("MISE_AUTO_UPDATE", "0")
                .env("MISE_CACHE_PRUNE_AGE", "0s");
        }
        for name in [
            "CONDA_PKGS_DIRS",
            "CONDARC",
            "MISE_CACHE_DIR",
            "MISE_STATE_DIR",
            "MISE_DATA_DIR",
            "MISE_CONFIG_DIR",
            "MISE_CONFIG_FILE",
            "MISE_GLOBAL_CONFIG_FILE",
            "MISE_TASK_CACHE_DIR",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
        ] {
            if let Some(value) = environment.cache_path_override(name) {
                command.env(name, value);
            }
        }
        Ok(command)
    }
    fn run(
        &self,
        environment: &PlatformEnvironment,
        dry_run: bool,
        reviewed: Option<&ToolPreview>,
    ) -> Result<(PathBuf, Vec<u8>), String> {
        let executable = self.executable(environment)?;
        if let Some(reviewed) = reviewed {
            if executable != reviewed.executable {
                return Err("The provider executable changed".into());
            }
            ToctouGuard::verify(&executable, &reviewed.executable_identity)
                .map_err(|error| error.to_string())?;
        }
        let scratch = if matches!(self.kind, ToolCacheKind::Swiftpm) {
            Some(
                tempfile::Builder::new()
                    .prefix("neati-swiftpm-")
                    .tempdir_in(environment.temp_dir())
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let command = if let Some(scratch) = &scratch {
            if dry_run {
                let mut command = Command::new(&executable);
                command
                    .arg("--version")
                    .current_dir(scratch.path())
                    .env_clear()
                    .env("HOME", scratch.path())
                    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
                command
            } else {
                swiftpm_command(
                    &executable,
                    &swiftpm_cache_root(environment)?,
                    scratch.path(),
                )
            }
        } else {
            self.command(environment, &executable, dry_run)?
        };
        let output = neati_platform::subprocess::run_with_timeout(
            command,
            Duration::from_secs(if dry_run { 30 } else { 120 }),
        )
        .map_err(|error| error.to_string())?;
        if !output.status.success() || (dry_run && !output.stderr.is_empty()) {
            return Err("Tool returned an error or warning; no cleanup result was verified".into());
        }
        Ok((executable, output.stdout))
    }
}
impl ToolCommandRunner for NativeToolCommandRunner {
    fn preview(&self, environment: &PlatformEnvironment) -> Result<ToolPreview, String> {
        let (executable, output) = self.run(environment, true, None)?;
        let candidates = match self.kind {
            ToolCacheKind::Conda => parse_candidates(&output)?,
            ToolCacheKind::Mise => parse_mise_roots(&output, environment)?,
            ToolCacheKind::Swiftpm => swiftpm_candidates(&output, environment)?,
        };
        let mut digest = Sha256::new();
        digest.update(executable.as_os_str().as_encoded_bytes());
        let executable_identity =
            ToctouGuard::capture(&executable).ok_or("Tool executable identity unavailable")?;
        digest.update(format!("{executable_identity:?}"));
        let measurer = crate::scanner::SizeCalculatorMeasurement;
        let mut estimated_bytes = 0u64;
        for path in &candidates {
            if let Some(ancestor) = path
                .ancestors()
                .find(|candidate| std::fs::symlink_metadata(candidate).is_ok())
            {
                SymlinkGuard::validate_anchored_path(ancestor, environment)
                    .map_err(|error| error.to_string())?;
            }
            if std::fs::symlink_metadata(path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                digest.update(path.as_os_str().as_encoded_bytes());
                digest.update(b"missing");
                continue;
            }
            Blacklist::validate_with(path, environment).map_err(|e| e.to_string())?;
            SymlinkGuard::validate_anchored_path(path, environment).map_err(|e| e.to_string())?;
            super::browser::inspect_store(path)?;
            let identity =
                ToctouGuard::capture(path).ok_or("Tool candidate identity unavailable")?;
            let measurement = measurer.measure(path);
            if !measurement.complete {
                return Err("Tool candidates could not be completely measured".into());
            }
            estimated_bytes = estimated_bytes
                .checked_add(measurement.allocated_bytes)
                .ok_or("Tool estimate overflow")?;
            digest.update(path.as_os_str().as_encoded_bytes());
            digest.update(format!("{identity:?}"));
            digest.update(measurement.allocated_bytes.to_le_bytes());
            if matches!(self.kind, ToolCacheKind::Swiftpm) {
                fingerprint_tree(path, &mut digest)?;
            }
        }
        Ok(ToolPreview {
            prefix: executable
                .parent()
                .ok_or("Tool has no installation parent")?
                .to_path_buf(),
            executable,
            executable_identity,
            candidates,
            estimated_bytes,
            fingerprint: format!(
                "inventory-{}",
                digest
                    .finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
        })
    }
    fn cleanup(
        &self,
        environment: &PlatformEnvironment,
        preview: &ToolPreview,
        processes: &dyn RunningProcessProbe,
        guard: &RunningProcessPolicy,
    ) -> Result<(), String> {
        let current = self.preview(environment)?;
        if current.key() != preview.key() {
            return Err("The tool or cache inventory changed before execution".into());
        }
        match processes.running(guard) {
            Some(running) if running.is_empty() => {}
            _ => return Err("The owner is running or its state is unknown".into()),
        }
        if matches!(self.kind, ToolCacheKind::Swiftpm) {
            verify_swiftpm_idle(&current.candidates, processes, guard)?;
        }
        let (_, output) = self.run(environment, false, Some(&current))?;
        match self.kind {
            ToolCacheKind::Conda => parse_candidates(&output).map(|_| ()),
            ToolCacheKind::Mise | ToolCacheKind::Swiftpm => Ok(()),
        }
    }
}

fn verify_swiftpm_idle(
    paths: &[PathBuf],
    processes: &dyn RunningProcessProbe,
    guard: &RunningProcessPolicy,
) -> Result<(), String> {
    for path in paths {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        if !matches!(processes.running(&guard.clone().with_open_files(path.clone())), Some(names) if names.is_empty())
        {
            return Err("SwiftPM cache handles are busy or unknown".into());
        }
    }
    Ok(())
}

fn swiftpm_executable(environment: &PlatformEnvironment) -> Result<PathBuf, String> {
    if let Some(tool) = environment.tool("swift-package") {
        return tool
            .path()
            .map(Path::to_path_buf)
            .ok_or("SwiftPM is unavailable".into());
    }
    let mut command = Command::new("/usr/bin/xcrun");
    command
        .args(["--find", "swift-package"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    let result = neati_platform::subprocess::run_with_timeout(command, Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(
            "Select an installed Apple developer toolchain before reviewing SwiftPM cleanup".into(),
        );
    }
    let path = String::from_utf8(result.stdout).map_err(|e| e.to_string())?;
    Ok(PathBuf::from(path.trim()))
}
fn swiftpm_cache_root(environment: &PlatformEnvironment) -> Result<PathBuf, String> {
    Ok(environment
        .user_home()
        .ok_or("User home unavailable")?
        .join("Library/Caches/org.swift.swiftpm"))
}
fn swiftpm_candidates(
    version: &[u8],
    environment: &PlatformEnvironment,
) -> Result<Vec<PathBuf>, String> {
    // This is the owner version exercised by the real disposable-home validation.
    // Unknown command semantics cannot authorize a broader package-store purge.
    if version != b"Swift Package Manager - Swift 6.4.0-dev\n" {
        return Err("SwiftPM cleanup is validated for Swift 6.4.0-dev only; this toolchain needs compatibility validation".into());
    }
    let root = swiftpm_cache_root(environment)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        match std::fs::symlink_metadata(root.join(format!("manifests/manifest.db{suffix}"))) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("SwiftPM manifest database has a journal or an unreadable companion; close its owner and review again".into()),
        }
    }
    Ok([
        "repositories",
        "registry/downloads",
        "manifests/manifest.db",
    ]
    .map(|name| root.join(name))
    .to_vec())
}
fn swiftpm_command(executable: &Path, cache: &Path, scratch: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--cache-path")
        .arg(cache)
        .arg("--config-path")
        .arg(scratch.join("configuration"))
        .arg("--security-path")
        .arg(scratch.join("security"))
        .arg("--scratch-path")
        .arg(scratch.join("build"))
        .arg("purge-cache")
        .current_dir(scratch)
        .env_clear()
        .env("HOME", scratch)
        .env("CFFIXED_USER_HOME", scratch)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    command
}
fn fingerprint_tree(root: &Path, digest: &mut Sha256) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    let mut remaining = MAX_CANDIDATES;
    let start = std::time::Instant::now();
    while let Some(path) = stack.pop() {
        remaining = remaining
            .checked_sub(1)
            .ok_or("SwiftPM inventory exceeds its entry limit")?;
        if start.elapsed() > Duration::from_secs(10) {
            return Err("SwiftPM inventory exceeded its time limit".into());
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            return Err("SwiftPM inventory contains unsupported entries".into());
        }
        let identity = ToctouGuard::capture(&path).ok_or("SwiftPM entry identity unavailable")?;
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update(format!("{identity:?}"));
        if metadata.is_dir() {
            let mut children = Vec::new();
            for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
                if children.len() + stack.len() >= remaining {
                    return Err("SwiftPM inventory exceeds its entry limit".into());
                }
                children.push(entry.map_err(|e| e.to_string())?.path());
            }
            children.sort();
            stack.extend(children);
        }
    }
    Ok(())
}

fn parse_candidates(bytes: &[u8]) -> Result<Vec<PathBuf>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Tool returned invalid JSON")?;
    let object = value.as_object().ok_or("Tool preview is not an object")?;
    if value.get("success").and_then(|v| v.as_bool()) != Some(true)
        || object
            .keys()
            .any(|key| !["success", "tarballs", "index_cache", "logfiles"].contains(&key.as_str()))
    {
        return Err("Tool preview contains unsupported cleanup targets or errors".into());
    }
    let mut candidates = Vec::new();
    let mut add = |path: PathBuf| -> Result<(), String> {
        if !path.is_absolute()
            || path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err("Tool returned an ambiguous candidate path".into());
        }
        if candidates.len() >= MAX_CANDIDATES {
            return Err("Tool preview exceeds its candidate limit".into());
        }
        candidates.push(path);
        Ok(())
    };
    let archives = value
        .get("tarballs")
        .ok_or("Tool omitted archive preview")?;
    if !archives
        .get("warnings")
        .and_then(|v| v.as_array())
        .ok_or("Missing archive warnings")?
        .is_empty()
    {
        return Err("Tool could not completely inspect its archives".into());
    }
    let directories = archives
        .get("pkgs_dirs")
        .and_then(|v| v.as_object())
        .ok_or("Missing archive paths")?;
    for (root, names) in directories {
        for name in names.as_array().ok_or("Invalid archive list")? {
            let name = name.as_str().ok_or("Invalid archive name")?;
            let archive = name.strip_suffix(".partial").unwrap_or(name);
            if name.contains(['/', '\\'])
                || !(archive.ends_with(".conda") || archive.ends_with(".tar.bz2"))
            {
                return Err("Tool returned an unsupported archive name".into());
            }
            add(Path::new(root).join(name))?;
        }
    }
    for path in value
        .pointer("/index_cache/files")
        .and_then(|v| v.as_array())
        .ok_or("Missing index cache preview")?
    {
        let path = PathBuf::from(path.as_str().ok_or("Invalid index path")?);
        if path.file_name().and_then(|v| v.to_str()) != Some("cache") {
            return Err("Unsupported index cache path".into());
        }
        add(path)?;
    }
    for path in value
        .get("logfiles")
        .and_then(|v| v.as_array())
        .ok_or("Missing logfile preview")?
    {
        let path = PathBuf::from(path.as_str().ok_or("Invalid logfile path")?);
        if path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|v| v.to_str())
            != Some(".logs")
        {
            return Err("Unsupported logfile path".into());
        }
        add(path)?;
    }
    candidates.sort();
    candidates.dedup();
    if candidates
        .windows(2)
        .any(|pair| pair[1].starts_with(&pair[0]))
    {
        return Err("Tool preview contains overlapping candidates".into());
    }
    Ok(candidates)
}

pub struct ToolCleanupProvider {
    process: Arc<dyn RunningProcessProbe>,
    runner: Arc<dyn ToolCommandRunner>,
    kind: ToolCacheKind,
}

impl ToolCleanupProvider {
    pub fn native(kind: ToolCacheKind, process: Arc<dyn RunningProcessProbe>) -> Self {
        Self {
            process,
            runner: Arc::new(NativeToolCommandRunner { kind }),
            kind,
        }
    }

    #[cfg(test)]
    fn with_runner(
        process: Arc<dyn RunningProcessProbe>,
        runner: Arc<dyn ToolCommandRunner>,
    ) -> Self {
        Self {
            process,
            runner,
            kind: ToolCacheKind::Conda,
        }
    }

    fn read_store(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
    ) -> OwnerStoreObservation {
        if !matches!(self.kind, ToolCacheKind::Swiftpm)
            && crate::tooling::resolve_with(self.kind.executable(), environment).is_none()
        {
            return OwnerStoreObservation::ready(None, Vec::new());
        }
        match self.process.running(guard) {
            None => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    None,
                    "The process table could not prove the tool is idle",
                )
            }
            Some(running) if !running.is_empty() => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::PrerequisiteNotMet,
                    None,
                    format!(
                        "Close {} before previewing tool cleanup",
                        running.join(", ")
                    ),
                )
            }
            Some(_) => {}
        }
        let preview = match self.runner.preview(environment) {
            Ok(preview) => preview,
            Err(error) => {
                return OwnerStoreObservation::refused(ProviderStatus::Blocked, None, error)
            }
        };
        if matches!(self.kind, ToolCacheKind::Swiftpm) {
            if let Err(error) =
                verify_swiftpm_idle(&preview.candidates, self.process.as_ref(), guard)
            {
                return OwnerStoreObservation::refused(ProviderStatus::Blocked, None, error);
            }
        }
        let root = preview.prefix.clone();
        if preview.candidates.is_empty() || preview.estimated_bytes == 0 {
            return OwnerStoreObservation::ready(Some(root), Vec::new());
        }
        let unit = OwnerUnitObservation::ready(
            preview.key(),
            preview.executable,
            preview.estimated_bytes,
            preview.estimated_bytes,
            preview.candidates.len() as u64,
        );
        OwnerStoreObservation::ready(Some(root), vec![unit])
    }

    fn prepare_units(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
        selections: &[OwnerProviderSelection],
    ) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
        let observation = self.read_store(environment, guard);
        if !observation.status.is_ready() {
            return Err(OwnerProviderRefusal::for_selections(
                observation.status,
                observation
                    .detail
                    .unwrap_or_else(|| "Tool cleanup is unavailable".into()),
                Vec::new(),
            ));
        }
        let Some(root) = observation.root else {
            return Err(OwnerProviderRefusal::for_selections(
                ProviderStatus::Blocked,
                "The tool is no longer installed; scan again",
                Vec::new(),
            ));
        };
        let mut plan = OwnerProviderAuthorization {
            deletion_disposition: neati_core::domain::cleanup::DeletionDisposition::PermanentDelete,
            signature_id: String::new(),
            provider_id: self.id().into(),
            risk: crate::models::RiskTier::Rebuild,
            units: Vec::new(),
            refusals: Vec::new(),
            process_guard: guard.clone(),
            requires_confirmation: true,
        };
        for selection in selections {
            let found = observation.units.iter().find(|unit| {
                unit.path == selection.path
                    && selection.item_id.ends_with(&unit.unit_key)
                    && unit.state == OwnerUnitState::Ready
                    && unit.allocated_bytes == selection.expected_bytes
            });
            let Some(unit) = found else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "The tool's cleanup candidates changed; scan again".into(),
                });
                continue;
            };
            let Some(identity) = ToctouGuard::capture(&unit.path) else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "The tool executable identity could not be captured".into(),
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
                "No reviewed tool cleanup is still eligible",
                plan.refusals,
            ));
        }
        Ok(plan)
    }

    fn execute_unit(
        &self,
        environment: &PlatformEnvironment,
        authorization: &OwnerProviderAuthorization,
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
        match self.process.running(&authorization.process_guard) {
            None => return refuse("The process table could not prove the tool is idle".into()),
            Some(running) if !running.is_empty() => {
                return refuse(format!("The tool is running: {}", running.join(", ")))
            }
            Some(_) => {}
        }
        if let Err(error) = ToctouGuard::verify(&unit.path, &unit.identity) {
            return refuse(format!("The tool executable changed since review: {error}"));
        }
        let preview = match self.runner.preview(environment) {
            Ok(preview) => preview,
            Err(error) => return refuse(error),
        };
        if preview.key() != unit.unit_key
            || preview.executable != unit.path
            || preview.prefix != unit.root
            || preview.estimated_bytes != unit.expected_bytes
            || preview.candidates.len() as u64 != unit.entry_count
        {
            return refuse(
                "The tool's exact cleanup candidates changed since review; scan again".into(),
            );
        }
        let command_result = self.runner.cleanup(
            environment,
            &preview,
            self.process.as_ref(),
            &authorization.process_guard,
        );
        let remaining = match self.runner.preview(environment) {
            Ok(preview) => preview.estimated_bytes.min(unit.expected_bytes),
            Err(error) => {
                return OwnerUnitOutcome::partially_cleaned(
                    unit.item_id.clone(),
                    unit.unit_key.clone(),
                    0,
                    None,
                    format!("Tool ran, but its post-cleanup state could not be verified: {error}"),
                )
            }
        };
        let removed = unit.expected_bytes.saturating_sub(remaining);
        match command_result {
            Ok(()) if remaining == 0 => {
                OwnerUnitOutcome::cleaned(unit.item_id.clone(), unit.unit_key.clone(), removed)
            }
            Ok(()) => OwnerUnitOutcome::partially_cleaned(
                unit.item_id.clone(),
                unit.unit_key.clone(),
                removed,
                None,
                "Tool completed but still reports reviewed cleanup candidates",
            ),
            Err(error) if removed > 0 => OwnerUnitOutcome::partially_cleaned(
                unit.item_id.clone(),
                unit.unit_key.clone(),
                removed,
                Some(remaining),
                error,
            ),
            Err(error) => refuse(error),
        }
    }
}

impl OwnerScopedProvider for ToolCleanupProvider {
    fn id(&self) -> &'static str {
        match self.kind {
            ToolCacheKind::Conda => "conda.disposable_cache",
            ToolCacheKind::Mise => "mise.cache_clear",
            ToolCacheKind::Swiftpm => "swiftpm.purge_cache",
        }
    }

    fn platforms(&self) -> &'static [PlatformKind] {
        &[PlatformKind::Macos]
    }

    fn consequence(&self) -> &'static str {
        match self.kind {
            ToolCacheKind::Swiftpm => "SwiftPM purges global repository downloads, registry downloads and its manifest cache. Dependencies may need downloading again. Project builds, installed toolchains, artifacts, configuration and security state stay intact.",
            ToolCacheKind::Conda => "Conda removes downloaded package archives, index caches and logs. Extracted packages and installed environments remain intact.",
            ToolCacheKind::Mise => "mise clears tool metadata, task output caches and cached environments using its own command. Installed tools, configuration and trust records remain intact; tasks may run again.",
        }
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn unit_label(&self, unit: &OwnerUnitObservation) -> String {
        format!(
            "{} cleanup ({} locations)",
            self.kind.executable(),
            unit.entry_count
        )
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
        OwnerProviderExecution {
            units: authorization
                .units
                .iter()
                .map(|unit| self.execute_unit(environment, authorization, unit))
                .collect(),
        }
    }
}

/// mise cache clear also covers external task caches and state/env-cache.
/// Read those roots from the owner's JSON rather than measuring one cache path.
fn parse_mise_roots(
    bytes: &[u8],
    environment: &PlatformEnvironment,
) -> Result<Vec<PathBuf>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "mise returned invalid JSON")?;
    let home = environment.user_home().ok_or("No user home for mise")?;
    let directory = |name: &str| -> Result<PathBuf, String> {
        value
            .get("dirs")
            .and_then(|v| v.get(name))
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .ok_or_else(|| format!("mise did not report its {name} directory"))
    };
    let cache = directory("cache")?;
    let state = directory("state")?;
    let data = directory("data")?;
    let config = directory("config")?;
    // doctor emits only non-default settings. An empty settings object means
    // the documented default task cache; malformed settings still fail closed.
    let settings = value
        .get("settings")
        .and_then(|v| v.as_object())
        .ok_or("mise did not report its settings")?;
    let task_setting = match settings.get("task") {
        None => &serde_json::Value::Null,
        Some(task) => task
            .as_object()
            .ok_or("mise returned invalid task settings")?
            .get("cache_dir")
            .unwrap_or(&serde_json::Value::Null),
    };
    let task = match task_setting {
        serde_json::Value::Null => cache.join("task-artifacts/v2"),
        serde_json::Value::String(path) if !path.is_empty() => PathBuf::from(path).join("v2"),
        _ => return Err("mise returned an unsupported task cache setting".into()),
    };
    let mut roots = vec![
        cache,
        task,
        state.join("env-cache"),
        state.join("task-artifacts"),
    ];
    for root in &roots {
        if !root.is_absolute()
            || !root.starts_with(&home)
            || root == &home
            || root.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
            || [
                home.join("Documents"),
                home.join("Desktop"),
                home.join("Downloads"),
                data.clone(),
                config.clone(),
            ]
            .iter()
            .any(|protected| root.starts_with(protected) || protected.starts_with(root))
        {
            return Err(
                "mise cache scope overlaps installed tools, configuration or user content".into(),
            );
        }
    }
    roots.sort();
    roots.dedup();
    let all = roots.clone();
    roots.retain(|root| {
        !all.iter()
            .any(|parent| parent != root && root.starts_with(parent))
    });
    Ok(roots)
}

#[cfg(test)]
mod tests {
    #[test]
    fn swiftpm_scope_refuses_unknown_versions_journals_and_covers_every_handle() {
        let root = tempfile::tempdir().unwrap();
        let env =
            neati_platform::PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
                .with_home(root.path());
        assert!(super::swiftpm_candidates(b"Swift Package Manager - Swift 9.0\n", &env).is_err());
        let version = b"Swift Package Manager - Swift 6.4.0-dev\n";
        let paths = super::swiftpm_candidates(version, &env).unwrap();
        assert_eq!(paths.len(), 3);
        for path in &paths {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if path.ends_with("manifest.db") {
                std::fs::write(path, b"fixture").unwrap();
            } else {
                std::fs::create_dir(path).unwrap();
            }
        }
        struct FirstBusy;
        impl super::RunningProcessProbe for FirstBusy {
            fn running(&self, policy: &super::RunningProcessPolicy) -> Option<Vec<String>> {
                Some(
                    if policy
                        .open_file_path()
                        .is_some_and(|p| p.ends_with("repositories"))
                    {
                        vec!["busy".into()]
                    } else {
                        vec![]
                    },
                )
            }
        }
        assert!(super::verify_swiftpm_idle(
            &paths,
            &FirstBusy,
            &super::RunningProcessPolicy::none()
        )
        .is_err());
        std::fs::write(paths[2].with_extension("db-wal"), b"journal").unwrap();
        assert!(super::swiftpm_candidates(version, &env).is_err());
    }
    #[test]
    fn swiftpm_review_fingerprint_changes_when_only_a_descendant_is_replaced() {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("nested/payload");
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"same size").unwrap();
        let mut before = Sha256::new();
        super::fingerprint_tree(root.path(), &mut before).unwrap();
        std::fs::rename(&path, root.path().join("keep")).unwrap();
        std::fs::write(&path, b"same size").unwrap();
        let mut after = Sha256::new();
        super::fingerprint_tree(root.path(), &mut after).unwrap();
        assert_ne!(before.finalize(), after.finalize());
    }

    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    struct Idle;
    impl RunningProcessProbe for Idle {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(vec![])
        }
    }
    struct Runner {
        preview: Mutex<ToolPreview>,
        calls: AtomicUsize,
    }
    impl ToolCommandRunner for Runner {
        fn preview(&self, _: &PlatformEnvironment) -> Result<ToolPreview, String> {
            Ok(self.preview.lock().unwrap().clone())
        }
        fn cleanup(
            &self,
            _: &PlatformEnvironment,
            _: &ToolPreview,
            _: &dyn RunningProcessProbe,
            _: &RunningProcessPolicy,
        ) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.preview.lock().unwrap().estimated_bytes = 0;
            Ok(())
        }
    }
    fn conda_json() -> serde_json::Value {
        serde_json::json!({"success":true,"tarballs":{"warnings":[],"pkgs_dirs":{"/profile/miniconda3/pkgs":["a.conda","b.tar.bz2.partial"]}},"index_cache":{"files":["/profile/miniconda3/pkgs/cache"]},"logfiles":["/profile/miniconda3/pkgs/.logs/log"]})
    }
    #[test]
    fn missing_tool_refuses_planning_without_panicking() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current());
        let provider = ToolCleanupProvider::native(ToolCacheKind::Conda, Arc::new(Idle));
        assert!(provider
            .prepare_units(&environment, &RunningProcessPolicy::default(), &[])
            .is_err());
    }
    #[test]
    fn conda_preview_is_narrow_and_rejects_incomplete_or_expanded_targets() {
        let good = conda_json();
        assert_eq!(
            parse_candidates(&serde_json::to_vec(&good).unwrap())
                .unwrap()
                .len(),
            4
        );
        let mut changed = good.clone();
        changed["packages"] = serde_json::json!({});
        assert!(parse_candidates(&serde_json::to_vec(&changed).unwrap()).is_err());
        let mut warning = good.clone();
        warning["tarballs"]["warnings"] = serde_json::json!(["permission denied"]);
        assert!(parse_candidates(&serde_json::to_vec(&warning).unwrap()).is_err());
        let mut traversal = good;
        traversal["tarballs"]["pkgs_dirs"]["/profile/miniconda3/pkgs"] =
            serde_json::json!(["../important.conda"]);
        assert!(parse_candidates(&serde_json::to_vec(&traversal).unwrap()).is_err());
        assert!(!CLEAN_ARGS.iter().any(|arg| [
            "--all",
            "--packages",
            "--force-pkgs-dirs",
            "--tempfiles"
        ]
        .contains(arg)));
    }
    #[test]
    fn mise_preview_and_cleanup_disable_incidental_updates_and_pruning() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home("/profile");
        let runner = NativeToolCommandRunner {
            kind: ToolCacheKind::Mise,
        };
        for dry_run in [true, false] {
            let command = runner
                .command(&environment, Path::new("/profile/.local/bin/mise"), dry_run)
                .unwrap();
            let variables: std::collections::HashMap<_, _> = command.get_envs().collect();
            for (key, expected) in [
                ("MISE_DISABLE_UPDATE_WARNING", "1"),
                ("MISE_AUTO_UPDATE", "0"),
                ("MISE_CACHE_PRUNE_AGE", "0s"),
            ] {
                assert_eq!(
                    variables.get(std::ffi::OsStr::new(key)).copied().flatten(),
                    Some(std::ffi::OsStr::new(expected))
                );
            }
        }
    }

    #[test]
    fn mise_default_settings_match_real_doctor_output() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home("/profile");
        let report = serde_json::json!({"dirs": {"cache":"/profile/Library/Caches/mise", "state":"/profile/.local/state/mise", "data":"/profile/.local/share/mise", "config":"/profile/.config/mise"}, "settings": {}});
        let roots = parse_mise_roots(&serde_json::to_vec(&report).unwrap(), &environment).unwrap();
        assert_eq!(
            roots,
            vec![
                PathBuf::from("/profile/.local/state/mise/env-cache"),
                PathBuf::from("/profile/.local/state/mise/task-artifacts"),
                PathBuf::from("/profile/Library/Caches/mise")
            ]
        );
        let mut malformed = report;
        malformed["settings"]["task"] = serde_json::json!("unknown");
        assert!(parse_mise_roots(&serde_json::to_vec(&malformed).unwrap(), &environment).is_err());
    }

    #[test]
    fn mise_scope_includes_external_tasks_and_environment_caches_without_overlap() {
        let environment =
            PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix).with_home("/profile");
        let mut report = serde_json::json!({"dirs":{"cache":"/profile/.cache/mise", "state":"/profile/.local/state/mise", "data":"/profile/.local/share/mise", "config":"/profile/.config/mise"},"settings":{"task":{"cache_dir":"/profile/.cache/tasks"}}});
        let roots = parse_mise_roots(&serde_json::to_vec(&report).unwrap(), &environment).unwrap();
        assert_eq!(roots.len(), 4);
        assert!(roots.contains(&PathBuf::from("/profile/.cache/tasks/v2")));
        assert!(roots.contains(&PathBuf::from("/profile/.local/state/mise/env-cache")));
        report["settings"]["task"]["cache_dir"] = serde_json::Value::Null;
        assert_eq!(
            parse_mise_roots(&serde_json::to_vec(&report).unwrap(), &environment)
                .unwrap()
                .len(),
            3
        );
        report["dirs"]["cache"] = serde_json::json!("/profile/.local/share/mise");
        assert!(parse_mise_roots(&serde_json::to_vec(&report).unwrap(), &environment).is_err());
    }
    #[test]
    fn reviewed_inventory_and_executable_are_rechecked_before_the_fixed_command() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("conda");
        std::fs::write(&executable, b"fixture").unwrap();
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home(temp.path())
            .with_tool("conda", &executable);
        let runner = Arc::new(Runner {
            preview: Mutex::new(ToolPreview {
                executable: executable.clone(),
                executable_identity: ToctouGuard::capture(&executable).unwrap(),
                prefix: temp.path().to_path_buf(),
                candidates: vec![temp.path().join("archive.conda")],
                estimated_bytes: 4096,
                fingerprint: "fixture-1".into(),
            }),
            calls: AtomicUsize::new(0),
        });
        let provider = ToolCleanupProvider::with_runner(Arc::new(Idle), runner.clone());
        let guard = RunningProcessPolicy::guarding(vec!["conda".into()]);
        let observed = provider.scan(&environment, &guard);
        let unit = &observed.units[0];
        let selection = OwnerProviderSelection {
            item_id: format!("dev.conda.{}", unit.unit_key),
            name: "Conda".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        runner.preview.lock().unwrap().fingerprint = "fixture-2".into();
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Blocked
        );
        assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
        runner.preview.lock().unwrap().fingerprint = "fixture-1".into();
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Cleaned
        );
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        assert!(executable.exists());
    }
    #[test]
    fn native_conda_adapter_runs_only_the_reviewed_command_against_a_fixture_store() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("miniconda3/bin/conda");
        let archive = temp.path().join("miniconda3/pkgs/payload.conda");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::write(&archive, vec![3; 8192]).unwrap();
        let report = serde_json::json!({"success":true,"tarballs":{"warnings":[],"pkgs_dirs":{archive.parent().unwrap().to_str().unwrap():["payload.conda"]}},"index_cache":{"files":[]},"logfiles":[]}).to_string();
        let empty = serde_json::json!({"success":true,"tarballs":{"warnings":[],"pkgs_dirs":{}},"index_cache":{"files":[]},"logfiles":[]}).to_string();
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
        let script = format!("#!/bin/sh\n[ \"$1 $2 $3 $4 $5 $6\" = 'clean --yes --index-cache --tarballs --logfiles --json' ] || exit 9\nif [ \"$#\" -eq 6 ]; then /bin/rm -f -- \"$HOME/miniconda3/pkgs/payload.conda\"; fi\nif [ -f \"$HOME/miniconda3/pkgs/payload.conda\" ]; then printf '%s\\n' {}; else printf '%s\\n' {}; fi\n", quote(&report), quote(&empty));
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home(temp.path())
            .with_tool("conda", &executable);
        let provider = ToolCleanupProvider::native(ToolCacheKind::Conda, Arc::new(Idle));
        let guard = RunningProcessPolicy::guarding(vec!["conda".into()]);
        let observation = provider.scan(&environment, &guard);
        assert_eq!(
            observation.status,
            ProviderStatus::Ready,
            "{:?}",
            observation.detail
        );
        let unit = &observation.units[0];
        let selection = OwnerProviderSelection {
            item_id: format!("conda.{}", unit.unit_key),
            name: "Conda fixture".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Cleaned
        );
        assert!(!archive.exists());
        assert!(executable.exists());
    }

    #[test]
    fn native_mise_adapter_measures_state_and_task_scopes_and_verifies_the_command() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join(".local/bin/mise");
        let cache = temp.path().join(".cache/mise");
        let task = temp.path().join(".cache/task-output/v2");
        let state = temp.path().join(".local/state/mise");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        for root in [
            &cache,
            &task,
            &state.join("env-cache"),
            &state.join("task-artifacts"),
        ] {
            std::fs::create_dir_all(root).unwrap();
            std::fs::write(root.join("payload"), vec![3; 4096]).unwrap();
        }
        let installed = temp.path().join(".local/share/mise/installs/tool");
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
        std::fs::write(&installed, b"keep").unwrap();
        let report = serde_json::json!({"dirs":{"cache":cache,"state":state,"data":temp.path().join(".local/share/mise"),"config":temp.path().join(".config/mise")},"settings":{"task":{"cache_dir":task.parent().unwrap()}}}).to_string();
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
        let script = format!("#!/bin/sh\nif [ \"$1 $2\" = 'doctor --json' ]; then printf '%s\\n' {}; elif [ \"$1 $2\" = 'cache clear' ]; then /bin/rm -rf -- \"$HOME/.cache/mise\" \"$HOME/.cache/task-output/v2\" \"$HOME/.local/state/mise/env-cache\" \"$HOME/.local/state/mise/task-artifacts\"; else exit 9; fi\n", quote(&report));
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home(temp.path())
            .with_tool("mise", &executable);
        let provider = ToolCleanupProvider::native(ToolCacheKind::Mise, Arc::new(Idle));
        let guard = RunningProcessPolicy::guarding(vec!["mise".into()]);
        let observation = provider.scan(&environment, &guard);
        assert_eq!(
            observation.status,
            ProviderStatus::Ready,
            "{:?}",
            observation.detail
        );
        let unit = &observation.units[0];
        assert_eq!(unit.allocated_bytes, 4 * 4096);
        let selected = OwnerProviderSelection {
            item_id: format!("mise.{}", unit.unit_key),
            name: "mise fixture".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        };
        let plan = provider.prepare(&environment, &guard, &[selected]).unwrap();
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Cleaned
        );
        assert!(installed.exists());
        assert!(!cache.exists());
        assert!(!task.exists());
    }

    #[test]
    fn native_commands_use_the_stated_home_and_fixed_owner_arguments() {
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_home("/profile");
        for kind in [
            ToolCacheKind::Conda,
            ToolCacheKind::Mise,
            ToolCacheKind::Swiftpm,
        ] {
            let runner = NativeToolCommandRunner { kind };
            let command = runner
                .command(&environment, Path::new("/fixture/tool"), false)
                .unwrap();
            let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
            match kind {
                ToolCacheKind::Conda => assert_eq!(args, CLEAN_ARGS),
                ToolCacheKind::Mise => assert_eq!(args, ["cache", "clear"]),
                ToolCacheKind::Swiftpm => assert_eq!(args, ["--version"]),
            }
            assert_eq!(command.get_current_dir(), Some(Path::new("/profile")));
        }
    }
}
