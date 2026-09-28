//! Reviewed `brew cleanup` execution through Homebrew's own public CLI.
//!
//! The provider never guesses which old kegs or cached downloads are stale.
//! It parses the exact fixed-argument dry-run, binds the review to a digest of
//! those candidates, repeats that preview immediately before execution, and
//! refuses the action if Homebrew's answer changed.

use super::OwnerScopedProvider;
use crate::models::{
    CleanFailureReason, OwnerProviderRefusal, OwnerProviderSelection, OwnerProviderUnit,
    OwnerStoreObservation, OwnerUnitObservation, OwnerUnitOutcome, OwnerUnitState, PlatformKind,
    ProviderStatus,
};
use crate::safety::ToctouGuard;
use neati_core::domain::cleanup::{
    OwnerProviderAuthorization, OwnerProviderExecution, RunningProcessPolicy, RunningProcessProbe,
};
use neati_platform::PlatformEnvironment;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

const PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq)]
struct BrewPreview {
    executable: PathBuf,
    prefix: PathBuf,
    version: String,
    candidates: Vec<PathBuf>,
    estimated_bytes: u64,
}

impl BrewPreview {
    fn key(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.executable.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(self.version.as_bytes());
        digest.update([0]);
        digest.update(self.estimated_bytes.to_le_bytes());
        for candidate in &self.candidates {
            digest.update([0]);
            digest.update(candidate.to_string_lossy().as_bytes());
        }
        let bytes = digest.finalize();
        let mut key = String::with_capacity(8 + bytes.len() * 2);
        key.push_str("cleanup-");
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(key, "{byte:02x}");
        }
        key
    }
}

trait BrewCommandRunner: Send + Sync {
    fn preview(&self, environment: &PlatformEnvironment) -> Result<BrewPreview, String>;
    fn cleanup(&self, environment: &PlatformEnvironment) -> Result<(), String>;
}

struct NativeBrewCommandRunner;

impl NativeBrewCommandRunner {
    fn executable(environment: &PlatformEnvironment) -> Result<PathBuf, String> {
        let executable = crate::tooling::resolve_with("brew", environment)
            .ok_or_else(|| "Homebrew was not found in the stated environment".to_string())?;
        trusted_brew(&executable)
    }

    fn command(executable: &Path, dry_run: bool) -> Command {
        let mut command = Command::new(executable);
        command.arg("cleanup");
        if dry_run {
            command.arg("--dry-run");
        }
        command.arg("--prune=30");
        command.env("HOMEBREW_NO_AUTO_UPDATE", "1");
        command.env("HOMEBREW_NO_COLOR", "1");
        command.env("HOMEBREW_NO_ENV_HINTS", "1");
        command.env("NO_COLOR", "1");
        neati_platform::subprocess::configure_background_command(&mut command);
        command
    }

    fn version(executable: &Path) -> Result<String, String> {
        let mut command = Command::new(executable);
        command.arg("--version");
        neati_platform::subprocess::configure_background_command(&mut command);
        let output = Self::run(command, Duration::from_secs(5))?;
        if !output.status.success() {
            return Err("Homebrew version probe failed".into());
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| line.starts_with("Homebrew ") && line.len() <= 128)
            .map(str::to_string)
            .ok_or_else(|| "Homebrew returned an unrecognized version".to_string())
    }

    fn prefix(executable: &Path) -> Result<PathBuf, String> {
        let mut command = Command::new(executable);
        command.arg("--prefix");
        neati_platform::subprocess::configure_background_command(&mut command);
        let output = Self::run(command, Duration::from_secs(5))?;
        if !output.status.success() {
            return Err("Homebrew prefix probe failed".into());
        }
        let prefix = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        if !matches!(prefix.to_str(), Some("/opt/homebrew" | "/usr/local")) {
            return Err(format!(
                "Homebrew reported an untrusted prefix `{}`",
                prefix.display()
            ));
        }
        Ok(prefix)
    }

    fn run(command: Command, timeout: Duration) -> Result<std::process::Output, String> {
        neati_platform::subprocess::run_with_timeout(command, timeout)
            .map_err(|error| format!("Homebrew command failed to run: {error}"))
    }
}

impl BrewCommandRunner for NativeBrewCommandRunner {
    fn preview(&self, environment: &PlatformEnvironment) -> Result<BrewPreview, String> {
        let executable = Self::executable(environment)?;
        let prefix = Self::prefix(&executable)?;
        let version = Self::version(&executable)?;
        let output = Self::run(Self::command(&executable, true), PREVIEW_TIMEOUT)?;
        if !output.status.success() {
            return Err(format!(
                "Homebrew cleanup preview failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        parse_preview(
            &String::from_utf8_lossy(&output.stdout),
            executable,
            prefix,
            version,
            environment,
        )
    }

    fn cleanup(&self, environment: &PlatformEnvironment) -> Result<(), String> {
        let executable = Self::executable(environment)?;
        let output = Self::run(Self::command(&executable, false), CLEANUP_TIMEOUT)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Homebrew cleanup exited unsuccessfully: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

pub struct HomebrewCleanupProvider {
    process: Arc<dyn RunningProcessProbe>,
    runner: Arc<dyn BrewCommandRunner>,
}

impl HomebrewCleanupProvider {
    pub fn native(process: Arc<dyn RunningProcessProbe>) -> Self {
        Self {
            process,
            runner: Arc::new(NativeBrewCommandRunner),
        }
    }

    #[cfg(test)]
    fn with_runner(
        process: Arc<dyn RunningProcessProbe>,
        runner: Arc<dyn BrewCommandRunner>,
    ) -> Self {
        Self { process, runner }
    }

    fn read_store(
        &self,
        environment: &PlatformEnvironment,
        guard: &RunningProcessPolicy,
    ) -> OwnerStoreObservation {
        match self.process.running(guard) {
            None => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::Blocked,
                    None,
                    "The process table could not prove Homebrew is idle",
                )
            }
            Some(running) if !running.is_empty() => {
                return OwnerStoreObservation::refused(
                    ProviderStatus::PrerequisiteNotMet,
                    None,
                    format!(
                        "Close {} before previewing Homebrew cleanup",
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
                    .unwrap_or_else(|| "Homebrew cleanup is unavailable".into()),
                Vec::new(),
            ));
        }
        let root = observation.root.expect("ready Homebrew preview has a root");
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
                    && unit.state == OwnerUnitState::Ready
                    && unit.allocated_bytes == selection.expected_bytes
            });
            let Some(unit) = found else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "Homebrew's cleanup candidates changed; scan again".into(),
                });
                continue;
            };
            let Some(identity) = ToctouGuard::capture(&unit.path) else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "The Homebrew executable identity could not be captured".into(),
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
                "No reviewed Homebrew cleanup is still eligible",
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
            None => return refuse("The process table could not prove Homebrew is idle".into()),
            Some(running) if !running.is_empty() => {
                return refuse(format!("Homebrew is running: {}", running.join(", ")))
            }
            Some(_) => {}
        }
        if let Err(error) = ToctouGuard::verify(&unit.path, &unit.identity) {
            return refuse(format!(
                "The Homebrew executable changed since review: {error}"
            ));
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
                "Homebrew's exact cleanup candidates changed since review; scan again".into(),
            );
        }
        let command_result = self.runner.cleanup(environment);
        let remaining = match self.runner.preview(environment) {
            Ok(preview) => preview.estimated_bytes.min(unit.expected_bytes),
            Err(error) => {
                return OwnerUnitOutcome::partially_cleaned(
                    unit.item_id.clone(),
                    unit.unit_key.clone(),
                    0,
                    None,
                    format!(
                        "Homebrew ran, but its post-cleanup state could not be verified: {error}"
                    ),
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
                "Homebrew completed but still reports reviewed cleanup candidates",
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

impl OwnerScopedProvider for HomebrewCleanupProvider {
    fn id(&self) -> &'static str {
        "homebrew.cleanup"
    }

    fn platforms(&self) -> &'static [PlatformKind] {
        &[PlatformKind::Macos]
    }

    fn consequence(&self) -> &'static str {
        "Homebrew removes exactly the old formula versions, stale lock files, and outdated downloads shown by its 30-day dry-run. Needed downloads may be fetched again; current installed versions remain."
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn unit_label(&self, unit: &OwnerUnitObservation) -> String {
        format!("Homebrew cleanup ({} candidates)", unit.entry_count)
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

fn trusted_brew(path: &Path) -> Result<PathBuf, String> {
    let trusted = [
        Path::new("/opt/homebrew/bin/brew"),
        Path::new("/usr/local/bin/brew"),
    ];
    if !trusted.contains(&path) {
        return Err(format!(
            "Refusing untrusted Homebrew executable `{}`",
            path.display()
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("Homebrew executable could not be resolved: {error}"))?;
    let canonical_trusted = [
        Path::new("/opt/homebrew/bin/brew"),
        Path::new("/usr/local/bin/brew"),
        Path::new("/usr/local/Homebrew/bin/brew"),
    ];
    if !canonical_trusted.contains(&canonical.as_path()) {
        return Err(format!(
            "Refusing Homebrew executable target `{}`",
            canonical.display()
        ));
    }
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| format!("Homebrew executable could not be inspected: {error}"))?;
    if !metadata.is_file() {
        return Err("The Homebrew executable is not an ordinary file".into());
    }
    Ok(canonical)
}

fn parse_preview(
    output: &str,
    executable: PathBuf,
    prefix: PathBuf,
    version: String,
    environment: &PlatformEnvironment,
) -> Result<BrewPreview, String> {
    let mut candidates = Vec::new();
    let mut estimated_bytes = None;
    for line in output.lines().map(str::trim) {
        let candidate = line
            .strip_prefix("Would remove: ")
            .or_else(|| line.strip_prefix("Would remove (empty directory): "));
        if let Some(value) = candidate {
            let raw_path = value.rsplit_once(" (").map_or(value, |(path, _)| path);
            let path = PathBuf::from(raw_path);
            validate_candidate(&path, &prefix, environment)?;
            candidates.push(path);
            continue;
        }
        if let Some(value) = line
            .strip_prefix("==> This operation would free approximately ")
            .and_then(|line| line.strip_suffix(" of disk space."))
        {
            estimated_bytes = Some(parse_size(value)?);
            continue;
        }
        if !line.is_empty() {
            return Err(format!(
                "Homebrew cleanup preview contained an unrecognized line: {line}"
            ));
        }
    }
    candidates.sort();
    candidates.dedup();
    let estimated_bytes = if candidates.is_empty() {
        0
    } else {
        estimated_bytes.ok_or_else(|| {
            "Homebrew listed cleanup candidates without a parseable size summary".to_string()
        })?
    };
    Ok(BrewPreview {
        executable,
        prefix,
        version,
        candidates,
        estimated_bytes,
    })
}

fn validate_candidate(
    path: &Path,
    prefix: &Path,
    environment: &PlatformEnvironment,
) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Homebrew reported a non-absolute cleanup candidate".into());
    }
    let in_prefix = path.starts_with(prefix);
    let in_cache = environment
        .user_home()
        .map(|home| path.starts_with(home.join("Library/Caches/Homebrew")))
        .unwrap_or(false);
    if !in_prefix && !in_cache {
        return Err(format!(
            "Homebrew reported a candidate outside its prefix and cache: {}",
            path.display()
        ));
    }
    Ok(())
}

fn parse_size(value: &str) -> Result<u64, String> {
    let split = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii_digit() && *character != '.')
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    let number: f64 = value[..split]
        .parse()
        .map_err(|_| format!("Homebrew reported an invalid cleanup size `{value}`"))?;
    let unit = value[split..].trim();
    let multiplier = match unit {
        "B" => 1f64,
        "KB" => 1_000f64,
        "MB" => 1_000_000f64,
        "GB" => 1_000_000_000f64,
        "KiB" => 1_024f64,
        "MiB" => 1_048_576f64,
        "GiB" => 1_073_741_824f64,
        _ => {
            return Err(format!(
                "Homebrew reported an unsupported cleanup size `{value}`"
            ))
        }
    };
    if !number.is_finite() || number < 0.0 {
        return Err(format!(
            "Homebrew reported an invalid cleanup size `{value}`"
        ));
    }
    Ok((number * multiplier).round().min(u64::MAX as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::Mutex;

    struct Idle;
    impl RunningProcessProbe for Idle {
        fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(Vec::new())
        }
    }

    struct FakeRunner {
        previews: Mutex<VecDeque<BrewPreview>>,
        cleanup_calls: Mutex<usize>,
        cleanup_error: Option<String>,
    }

    impl BrewCommandRunner for FakeRunner {
        fn preview(&self, _environment: &PlatformEnvironment) -> Result<BrewPreview, String> {
            self.previews
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no preview".into())
        }

        fn cleanup(&self, _environment: &PlatformEnvironment) -> Result<(), String> {
            *self.cleanup_calls.lock().unwrap() += 1;
            self.cleanup_error.clone().map_or(Ok(()), Err)
        }
    }

    fn fixture() -> (tempfile::TempDir, PlatformEnvironment, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("homebrew");
        let executable = prefix.join("bin/brew");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"brew").unwrap();
        let environment = PlatformEnvironment::native()
            .with_platform(PlatformKind::Macos)
            .with_home(temp.path());
        (temp, environment, executable, prefix)
    }

    fn preview(executable: &Path, prefix: &Path, candidates: &[&str], bytes: u64) -> BrewPreview {
        BrewPreview {
            executable: executable.to_path_buf(),
            prefix: prefix.to_path_buf(),
            version: "Homebrew 5.0.0".into(),
            candidates: candidates
                .iter()
                .map(|candidate| prefix.join(candidate))
                .collect(),
            estimated_bytes: bytes,
        }
    }

    #[test]
    fn parses_exact_candidates_and_decimal_summary() {
        let (_temp, environment, executable, prefix) = fixture();
        let output = "Would remove: PREFIX/Cellar/glib/2.86.0 (504 files, 39.5MB)\nWould remove (empty directory): PREFIX/lib/gio\n==> This operation would free approximately 39.5MB of disk space.\n"
            .replace("PREFIX", &prefix.to_string_lossy());
        let parsed = parse_preview(
            &output,
            executable,
            prefix.clone(),
            "Homebrew 5.0.0".into(),
            &environment,
        )
        .unwrap();
        assert_eq!(parsed.candidates.len(), 2);
        assert_eq!(parsed.estimated_bytes, 39_500_000);
        assert!(parsed
            .candidates
            .iter()
            .all(|path| path.starts_with(&prefix)));
    }

    #[test]
    fn reviewed_preview_is_rechecked_before_fixed_cleanup() {
        let (_temp, environment, executable, prefix) = fixture();
        let first = preview(&executable, &prefix, &["Cellar/tool/1.0"], 8_000_000);
        let after = preview(&executable, &prefix, &[], 0);
        let runner = Arc::new(FakeRunner {
            previews: Mutex::new(VecDeque::from([first.clone(), first.clone(), first, after])),
            cleanup_calls: Mutex::new(0),
            cleanup_error: None,
        });
        let provider = HomebrewCleanupProvider::with_runner(Arc::new(Idle), runner.clone());
        let guard = RunningProcessPolicy::guarding(vec!["brew".into()]);
        let observation = provider.scan(&environment, &guard);
        let unit = &observation.units[0];
        let selection = OwnerProviderSelection {
            item_id: "brew-cleanup".into(),
            name: "Homebrew cleanup".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(*runner.cleanup_calls.lock().unwrap(), 1);
        assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
        assert_eq!(result.units[0].reclaimed_bytes, 8_000_000);
    }

    #[test]
    fn candidate_change_before_execution_is_refused() {
        let (_temp, environment, executable, prefix) = fixture();
        let reviewed = preview(&executable, &prefix, &["Cellar/tool/1.0"], 8_000_000);
        let changed = preview(&executable, &prefix, &["Cellar/tool/1.1"], 9_000_000);
        let runner = Arc::new(FakeRunner {
            previews: Mutex::new(VecDeque::from([reviewed.clone(), reviewed, changed])),
            cleanup_calls: Mutex::new(0),
            cleanup_error: None,
        });
        let provider = HomebrewCleanupProvider::with_runner(Arc::new(Idle), runner.clone());
        let guard = RunningProcessPolicy::guarding(vec!["brew".into()]);
        let observation = provider.scan(&environment, &guard);
        let unit = &observation.units[0];
        let selection = OwnerProviderSelection {
            item_id: "brew-cleanup".into(),
            name: "Homebrew cleanup".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        };
        let plan = provider
            .prepare(&environment, &guard, &[selection])
            .unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(*runner.cleanup_calls.lock().unwrap(), 0);
        assert_eq!(result.units[0].status, ProviderStatus::Blocked);
    }

    #[test]
    fn candidate_outside_owner_roots_is_rejected() {
        let (_temp, environment, executable, prefix) = fixture();
        let output = "Would remove: /Users/shared/not-homebrew (1 file, 1MB)\n==> This operation would free approximately 1MB of disk space.\n";
        assert!(parse_preview(
            output,
            executable,
            prefix,
            "Homebrew 5.0.0".into(),
            &environment,
        )
        .is_err());
    }

    #[test]
    fn unknown_preview_stdout_is_rejected() {
        let (_temp, environment, executable, prefix) = fixture();
        let output = "Would maybe remove something later\n";
        assert!(parse_preview(
            output,
            executable,
            prefix,
            "Homebrew 5.0.0".into(),
            &environment,
        )
        .is_err());
    }

    #[test]
    fn command_failure_reports_verified_partial_progress() {
        let (_temp, environment, executable, prefix) = fixture();
        let reviewed = preview(&executable, &prefix, &["Cellar/tool/1.0"], 8_000_000);
        let remaining = preview(&executable, &prefix, &["Cellar/tool/1.0"], 4_000_000);
        let runner = Arc::new(FakeRunner {
            previews: Mutex::new(VecDeque::from([
                reviewed.clone(),
                reviewed.clone(),
                reviewed,
                remaining,
            ])),
            cleanup_calls: Mutex::new(0),
            cleanup_error: Some("cleanup stopped after one candidate".into()),
        });
        let provider = HomebrewCleanupProvider::with_runner(Arc::new(Idle), runner);
        let guard = RunningProcessPolicy::guarding(vec!["brew".into()]);
        let observation = provider.scan(&environment, &guard);
        let unit = &observation.units[0];
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &[OwnerProviderSelection {
                    item_id: "brew-cleanup".into(),
                    name: "Homebrew cleanup".into(),
                    path: unit.path.clone(),
                    expected_bytes: unit.allocated_bytes,
                }],
            )
            .unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::PartiallyCleaned);
        assert_eq!(result.units[0].reclaimed_bytes, 4_000_000);
        assert_eq!(result.units[0].remaining_bytes, Some(4_000_000));
    }
}
