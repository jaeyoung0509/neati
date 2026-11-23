mod catalog;

use crate::models::{
    derive_cleanup_disposition, AbsolutePath, CacheManagementMode, CacheMetadata,
    CacheSizeSemantics, CancellationProbe, CanonicalPath, Category, CleanupEligibility,
    CleanupUnit, CleanupUnitKind, DispositionFacts, EligibilityGate, EntryKind, ObservationQuality,
    RiskTier, ScanGapKind, ScanItem,
};
use crate::safety::{Blacklist, SymlinkGuard};
use crate::scanner::{
    PathMeasurement, RootProgressSink, ScanLimits, SizeCalculator, TraversalCounters,
};
use crate::signatures::SignatureRegistry;
use crate::tooling;
use catalog::{DiscoveryOutput, ProviderKind};
use neati_platform::path_algebra::{self, PathFlavor};
use neati_platform::PlatformEnvironment;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use sysinfo::{ProcessesToUpdate, System};

const PROVIDER_TIMEOUT: Duration = Duration::from_secs(15);
/// uv coordinates cache access through its own lock. Its documented lock wait
/// is five minutes, so permit that wait while retaining the same hard deadline.
const UV_PRUNE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_DISCOVERY_OUTPUT: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CacheProviderPruneFailure {
    OwnerRunning(String),
    OwnerStateUnknown(String),
    Failed(String),
}

impl std::fmt::Display for CacheProviderPruneFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OwnerRunning(executable) => {
                write!(formatter, "{executable} is currently running")
            }
            Self::OwnerStateUnknown(executable) => {
                write!(
                    formatter,
                    "{executable} process state could not be verified"
                )
            }
            Self::Failed(reason) => formatter.write_str(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheProviderFailure {
    pub kind: ScanGapKind,
    pub reason: String,
}

impl CacheProviderFailure {
    fn new(kind: ScanGapKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CacheProviderScan {
    pub items: Vec<ScanItem>,
    pub failures: Vec<CacheProviderFailure>,
    pub cancelled: bool,
    pub spans: Vec<neati_core::domain::scan::ScanSpan>,
}

pub(crate) trait CacheProviderScanner: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    fn scan_items(
        &self,
        registry: &SignatureRegistry,
        excluded_signatures: &[String],
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
        progress: &dyn RootProgressSink,
    ) -> CacheProviderScan;
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderCommandResult {
    Absent,
    Output {
        success: bool,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Failed(String),
    Cancelled,
}

trait ProviderCommandRunner: Send + Sync {
    fn discover(
        &self,
        provider: ProviderKind,
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
    ) -> ProviderCommandResult;

    fn prune(
        &self,
        provider: ProviderKind,
        environment: &PlatformEnvironment,
    ) -> ProviderCommandResult;
}

trait ProviderMeasurer: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    fn measure(
        &self,
        path: &Path,
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
    ) -> PathMeasurement;

    fn measure_for_prune(&self, path: &Path, environment: &PlatformEnvironment) -> PathMeasurement {
        let counters = TraversalCounters::default();
        self.measure(
            path,
            environment,
            &crate::models::NeverCancelled,
            ScanLimits::default(),
            &counters,
        )
    }
}

struct NativeProviderCommandRunner;

impl ProviderCommandRunner for NativeProviderCommandRunner {
    fn discover(
        &self,
        provider: ProviderKind,
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
    ) -> ProviderCommandResult {
        let Some(executable) = tooling::resolve_with(provider.executable(), environment) else {
            return ProviderCommandResult::Absent;
        };
        if let Err(error) = validate_executable(&executable, environment) {
            return ProviderCommandResult::Failed(error);
        }
        let mut command = Command::new(executable);
        command.args(provider.discovery_args());
        if let Err(error) = configure_provider_command(provider, environment, &mut command) {
            return ProviderCommandResult::Failed(error);
        }
        match neati_platform::subprocess::run_with_timeout_cancellable(
            command,
            PROVIDER_TIMEOUT,
            &|| cancellation.is_cancelled(),
        ) {
            Ok(output) => ProviderCommandResult::Output {
                success: output.status.success(),
                stdout: output.stdout,
                stderr: output.stderr,
            },
            Err(neati_platform::SubprocessError::Cancelled(_)) => ProviderCommandResult::Cancelled,
            Err(error) => ProviderCommandResult::Failed(error.to_string()),
        }
    }

    fn prune(
        &self,
        provider: ProviderKind,
        environment: &PlatformEnvironment,
    ) -> ProviderCommandResult {
        let timeout = if provider == ProviderKind::Uv {
            UV_PRUNE_TIMEOUT
        } else {
            PROVIDER_TIMEOUT
        };
        match run_provider_with_timeout(provider, provider.prune_args(), environment, timeout) {
            Ok(output) => ProviderCommandResult::Output {
                success: output.status.success(),
                stdout: output.stdout,
                stderr: output.stderr,
            },
            Err(error) => ProviderCommandResult::Failed(error),
        }
    }
}

struct NativeProviderMeasurer;

impl ProviderMeasurer for NativeProviderMeasurer {
    fn measure(
        &self,
        path: &Path,
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
    ) -> PathMeasurement {
        SizeCalculator::measure_path_bounded(path, &[], environment, cancellation, limits, counters)
    }
}

/// Backend-owned cache providers. The frontend can select only the ScanItem ID;
/// executable names, arguments, and cache paths are all rediscovered here.
pub struct CacheProviderRegistry;

impl CacheProviderRegistry {
    /// Scans every external cache provider through the described environment.
    /// The provider executables are resolved with the environment's stated tool
    /// facts and the discovered caches are approved against the environment's
    /// home, so a redirected or simulated environment cannot be answered by the
    /// runner's own profile.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scan_items(
        registry: &SignatureRegistry,
        excluded_signatures: &[String],
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
        progress: &dyn RootProgressSink,
    ) -> CacheProviderScan {
        Self::scan_items_with(
            registry,
            excluded_signatures,
            environment,
            cancellation,
            limits,
            counters,
            progress,
            &NativeProviderCommandRunner,
            &NativeProviderMeasurer,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn scan_items_with(
        registry: &SignatureRegistry,
        excluded_signatures: &[String],
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
        progress: &dyn RootProgressSink,
        runner: &dyn ProviderCommandRunner,
        measurer: &dyn ProviderMeasurer,
    ) -> CacheProviderScan {
        let mut result = CacheProviderScan::default();
        // Provider discovery is deliberately sequential. Starting every CLI
        // at once would make a user stop incapable of preventing later probes,
        // while these bounded lookups do not justify that lifecycle ambiguity.
        for provider in ProviderKind::ALL {
            if cancellation.is_cancelled() {
                result.cancelled = true;
                break;
            }
            if excluded_signatures
                .iter()
                .any(|id| id == provider.signature_id())
            {
                continue;
            }
            match Self::scan_provider(
                provider,
                registry,
                environment,
                cancellation,
                limits,
                counters,
                progress,
                runner,
                measurer,
                &mut result.spans,
            ) {
                Ok(Some(item)) => result.items.push(item),
                Ok(None) => {}
                Err(failure) if failure.kind == ScanGapKind::Cancelled => {
                    result.cancelled = true;
                    break;
                }
                Err(failure) => {
                    crate::diagnostics::log_error(
                        "cache_providers",
                        &format!(
                            "{} scan incomplete: {}",
                            provider.executable(),
                            failure.reason
                        ),
                    );
                    result.failures.push(failure);
                }
            }
            if cancellation.is_cancelled() {
                result.cancelled = true;
                break;
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn scan_provider(
        provider: ProviderKind,
        registry: &SignatureRegistry,
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
        progress: &dyn RootProgressSink,
        runner: &dyn ProviderCommandRunner,
        measurer: &dyn ProviderMeasurer,
        spans: &mut Vec<neati_core::domain::scan::ScanSpan>,
    ) -> Result<Option<ScanItem>, CacheProviderFailure> {
        let signature_id = provider.signature_id();
        let Some(signature) = registry.get(signature_id) else {
            return Ok(None);
        };
        if signature.family != provider.family() {
            return Err(CacheProviderFailure::new(
                ScanGapKind::IoError,
                format!(
                    "{} catalog ownership does not match its provider contract",
                    provider.display_name()
                ),
            ));
        }
        if !signature.supports_current_platform() {
            return Ok(None);
        }
        let discovery_started = std::time::Instant::now();
        let command = runner.discover(provider, environment, cancellation);
        spans.push(neati_core::domain::scan::ScanSpan {
            source_id: format!("{signature_id}.discovery"),
            duration_ms: discovery_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        });
        let path = match command {
            ProviderCommandResult::Absent => return Ok(None),
            ProviderCommandResult::Cancelled => {
                return Err(CacheProviderFailure::new(
                    ScanGapKind::Cancelled,
                    format!("{} cache inspection was cancelled", provider.executable()),
                ));
            }
            ProviderCommandResult::Failed(reason) => {
                return Err(CacheProviderFailure::new(
                    ScanGapKind::Unknown,
                    format!(
                        "{} cache inspection failed: {reason}",
                        provider.executable()
                    ),
                ));
            }
            ProviderCommandResult::Output {
                success,
                stdout,
                stderr,
            } => {
                if !success {
                    let reason = bounded_message(&stderr);
                    return Err(CacheProviderFailure::new(
                        ScanGapKind::Unknown,
                        format!(
                            "{} cache discovery failed: {}",
                            provider.executable(),
                            reason
                        ),
                    ));
                }
                let parsed = parse_provider_path(provider, &stdout).map_err(|reason| {
                    CacheProviderFailure::new(
                        ScanGapKind::IoError,
                        format!("{}: {reason}", provider.executable()),
                    )
                })?;
                // A CLI may report its default cache location before it has
                // created that directory. Nothing exists to measure or
                // authorize in that case; scan it again once the owner does.
                // An existing path still passes the full approval check.
                if std::fs::symlink_metadata(parsed.as_path())
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                {
                    let resolved_candidate = parsed.as_path().ancestors().find_map(|ancestor| {
                        let canonical = std::fs::canonicalize(ancestor).ok()?;
                        let remainder = parsed.as_path().strip_prefix(ancestor).ok()?;
                        Some(canonical.join(remainder))
                    });
                    let approved_default = environment
                        .user_home()
                        .and_then(|home| std::fs::canonicalize(home).ok())
                        .zip(resolved_candidate)
                        .is_some_and(|(home, candidate)| {
                            cache_location_approved(&candidate, &home, environment.flavor())
                        });
                    if approved_default {
                        return Ok(None);
                    }
                }
                validate_cache_path(parsed, environment).map_err(|reason| {
                    CacheProviderFailure::new(
                        ScanGapKind::Unknown,
                        format!("{}: {reason}", provider.executable()),
                    )
                })?
            }
        };
        progress.root_started(signature, &path);
        let measurement_started = std::time::Instant::now();
        let measurement = measurer.measure(&path, environment, cancellation, limits, counters);
        spans.push(neati_core::domain::scan::ScanSpan {
            source_id: format!("{signature_id}.measurement"),
            duration_ms: measurement_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        });
        if measurement.size.reclaimable() == 0 && measurement.complete {
            return Ok(None);
        }
        let (quality, size_semantics) = if !measurement.complete {
            let q = if measurement.size.reclaimable() == 0 {
                ObservationQuality::Unavailable
            } else {
                ObservationQuality::Partial
            };
            (q, CacheSizeSemantics::ConservativeLowerBound)
        } else {
            (ObservationQuality::Fresh, CacheSizeSemantics::Informational)
        };
        let last_modified = std::fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        let cache_metadata = CacheMetadata {
            provider: provider.executable().to_string(),
            management_mode: CacheManagementMode::ToolManaged,
            artifact_kind: provider.artifact_kind(),
            consequence: provider.consequence().to_string(),
            size_semantics,
            last_used_confidence: Default::default(),
        };
        let disposition = derive_cleanup_disposition(DispositionFacts::new(
            RiskTier::Rebuild,
            quality,
            &cache_metadata,
            &measurement.size,
            measurement.incomplete_reason.as_deref(),
        ));
        let is_selected = disposition.eligibility == CleanupEligibility::AutoCleanable;
        let path_text = path.to_string_lossy().into_owned();
        Ok(Some(ScanItem {
            id: signature_id.to_string(),
            signature_id: signature_id.to_string(),
            name: provider.display_name().to_string(),
            category: Category::Developer,
            risk: RiskTier::Rebuild,
            path: path_text.clone(),
            size: measurement.size,
            file_count: measurement.file_count,
            description: "Inspected and pruned by the owning tool.".to_string(),
            cache_metadata,
            disposition,
            // The provider owns both the discovery and the invalidation; the
            // location travels with the item as a staleness assertion only.
            unit: CleanupUnit::new(
                CleanupUnitKind::ProviderAction,
                path_text.clone(),
                path_text,
            ),
            // The planner compares this claim with the catalog. Reuse the
            // catalog projection instead of manufacturing a stronger
            // executable-name claim (for example `go` versus provider `Go`).
            ownership: signature.ownership(),
            age: None,
            stale: None,
            structured_state: None,
            provider_restriction: None,
            retained: None,
            entry_kind: EntryKind::Directory,
            gate: EligibilityGate::Open,
            owner_running: false,
            lifecycle_provider_action: false,
            requires_confirmation: false,
            overlaps: Vec::new(),
            is_selected,
            last_modified,
            exists: true,
            quality,
            inspection_issue: measurement.inspection_issue,
            incomplete_reason: measurement.incomplete_reason,
            skipped_entry_count: measurement.skipped_entries,
        }))
    }

    /// Runs the provider's own prune command.
    ///
    /// The returned amount is `None` when either measurement was incomplete:
    /// the provider still pruned, and the caller reports the target as partial
    /// instead of showing a difference between two partial numbers as exact.
    pub(crate) fn prune(
        signature_id: &str,
        planned_path: &Path,
        environment: &PlatformEnvironment,
    ) -> Result<Option<u64>, CacheProviderPruneFailure> {
        let provider = ProviderKind::for_signature(signature_id).ok_or_else(|| {
            CacheProviderPruneFailure::Failed("Unknown external cache provider".to_string())
        })?;
        Self::prune_with(
            provider,
            planned_path,
            environment,
            &NativeProviderCommandRunner,
            &NativeProviderMeasurer,
        )
    }

    fn prune_with(
        provider: ProviderKind,
        planned_path: &Path,
        environment: &PlatformEnvironment,
        runner: &dyn ProviderCommandRunner,
        measurer: &dyn ProviderMeasurer,
    ) -> Result<Option<u64>, CacheProviderPruneFailure> {
        if !provider.active_processes().is_empty() {
            if let Some(refusal) =
                owner_process_refusal(provider, matching_process_is_active(provider))
            {
                return Err(refusal);
            }
        }
        let fresh_path = discover_path_with(provider, environment, runner)
            .map_err(CacheProviderPruneFailure::Failed)?;
        if !paths_match(&fresh_path, planned_path) {
            return Err(CacheProviderPruneFailure::Failed(
                "The provider cache location changed since the scan. Scan again.".to_string(),
            ));
        }
        let before = measurer.measure_for_prune(&fresh_path, environment);
        match runner.prune(provider, environment) {
            ProviderCommandResult::Output { success: true, .. } => {}
            ProviderCommandResult::Output {
                success: false,
                stderr,
                ..
            } => {
                return Err(CacheProviderPruneFailure::Failed(format!(
                    "{} prune failed: {}",
                    provider.executable(),
                    bounded_message(&stderr)
                )));
            }
            ProviderCommandResult::Failed(reason) => {
                return Err(CacheProviderPruneFailure::Failed(format!(
                    "{} prune failed: {reason}",
                    provider.executable()
                )));
            }
            ProviderCommandResult::Absent => {
                return Err(CacheProviderPruneFailure::Failed(format!(
                    "{} was not detected in trusted tool locations",
                    provider.executable()
                )));
            }
            ProviderCommandResult::Cancelled => {
                return Err(CacheProviderPruneFailure::Failed(format!(
                    "{} prune was cancelled",
                    provider.executable()
                )));
            }
        }
        let rediscovered = discover_path_with(provider, environment, runner)
            .map_err(CacheProviderPruneFailure::Failed)?;
        if !paths_match(&rediscovered, &fresh_path) {
            return Err(CacheProviderPruneFailure::Failed(
                "The provider cache location changed during cleanup.".to_string(),
            ));
        }
        let after = measurer.measure_for_prune(&rediscovered, environment);
        Ok(crate::scanner::size::reclaimed_between(&before, &after))
    }
}

impl CacheProviderScanner for CacheProviderRegistry {
    fn scan_items(
        &self,
        registry: &SignatureRegistry,
        excluded_signatures: &[String],
        environment: &PlatformEnvironment,
        cancellation: &dyn CancellationProbe,
        limits: ScanLimits,
        counters: &TraversalCounters,
        progress: &dyn RootProgressSink,
    ) -> CacheProviderScan {
        Self::scan_items(
            registry,
            excluded_signatures,
            environment,
            cancellation,
            limits,
            counters,
            progress,
        )
    }
}

fn run_provider_with_timeout(
    provider: ProviderKind,
    args: &[&str],
    environment: &PlatformEnvironment,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let executable =
        tooling::resolve_with(provider.executable(), environment).ok_or_else(|| {
            format!(
                "{} was not detected in PATH or known tool locations",
                provider.executable()
            )
        })?;
    validate_executable(&executable, environment)?;
    let mut command = Command::new(executable);
    command.args(args);
    configure_provider_command(provider, environment, &mut command)?;
    neati_platform::subprocess::run_with_timeout(command, timeout)
        .map_err(|error| error.to_string())
}

// Discovery and mutation must see the same cache configuration. Explicitly
// remove both npm spellings: environment keys are case-sensitive on Unix.
const CACHE_PATH_ENVIRONMENT: &[&str] = &[
    "GOCACHE",
    "GOMODCACHE",
    "GOPATH",
    "GOENV",
    "UV_CACHE_DIR",
    "PIP_CACHE_DIR",
    "PIP_CONFIG_FILE",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "PNPM_HOME",
    "npm_config_store_dir",
    "NPM_CONFIG_STORE_DIR",
    "npm_config_cache",
    "NPM_CONFIG_CACHE",
    "npm_config_userconfig",
    "NPM_CONFIG_USERCONFIG",
    "npm_config_globalconfig",
    "NPM_CONFIG_GLOBALCONFIG",
    "BUN_INSTALL_CACHE_DIR",
    "BUN_RUNTIME_TRANSPILER_CACHE_PATH",
    "COMPOSER_CACHE_DIR",
    "COMPOSER_HOME",
    "NUGET_PACKAGES",
    "NUGET_HTTP_CACHE_PATH",
    "NUGET_PLUGINS_CACHE_PATH",
];

fn strip_cache_environment(command: &mut Command) {
    for variable in CACHE_PATH_ENVIRONMENT {
        command.env_remove(variable);
    }
}

fn configure_provider_command(
    provider: ProviderKind,
    environment: &PlatformEnvironment,
    command: &mut Command,
) -> Result<(), String> {
    strip_cache_environment(command);
    if let Some((name, configured_path)) = provider_cache_override(provider, environment)? {
        let path = configured_path
            .to_str()
            .ok_or_else(|| "The provider cache override is not UTF-8".to_string())?;
        let absolute = AbsolutePath::new(path)
            .map_err(|_| "The provider cache override is not absolute".to_string())?;
        let approved = validate_cache_path(absolute, environment)?;
        command.env(name, approved);
    }
    let mut dependency_dirs = Vec::new();
    for dependency in provider.runtime_dependencies() {
        let executable = tooling::resolve_with(dependency, environment).ok_or_else(|| {
            format!(
                "{} requires {}, which was not detected in trusted tool locations",
                provider.executable(),
                dependency
            )
        })?;
        validate_executable(&executable, environment)?;
        let canonical = std::fs::canonicalize(executable)
            .map_err(|_| format!("Could not validate the {dependency} runtime"))?;
        let parent = canonical
            .parent()
            .ok_or_else(|| format!("Could not resolve the {dependency} runtime directory"))?;
        dependency_dirs.push(parent.to_path_buf());
    }
    if !dependency_dirs.is_empty() {
        // Script shims commonly use `/usr/bin/env node` or `/usr/bin/env php`.
        // Give them only the verified runtime directories plus the OS command
        // roots needed by `env`; never forward an arbitrary inherited PATH.
        dependency_dirs.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
        let path = std::env::join_paths(dependency_dirs)
            .map_err(|_| "Could not construct the provider runtime PATH".to_string())?;
        command.env("PATH", path);
    }
    if provider.local_toolchain_only() {
        // Go's default `auto` toolchain mode may download a different toolchain
        // merely to answer `go env` or perform `go clean`. Inspection and
        // cleanup must not create the cache they are measuring or access the
        // network as a side effect.
        command.env("GOTOOLCHAIN", "local");
    }
    Ok(())
}

fn provider_cache_override(
    provider: ProviderKind,
    environment: &PlatformEnvironment,
) -> Result<Option<(&'static str, &Path)>, String> {
    Ok(match provider {
        ProviderKind::Uv => environment
            .cache_path_override("UV_CACHE_DIR")
            .map(|path| ("UV_CACHE_DIR", path)),
        ProviderKind::Pip => environment
            .cache_path_override("PIP_CACHE_DIR")
            .map(|path| ("PIP_CACHE_DIR", path)),
        ProviderKind::Npm => {
            npm_cache_override(environment, "npm_config_cache", "NPM_CONFIG_CACHE")?
                .map(|path| ("npm_config_cache", path))
        }
        ProviderKind::Pnpm => {
            npm_cache_override(environment, "npm_config_store_dir", "NPM_CONFIG_STORE_DIR")?
                .map(|path| ("npm_config_store_dir", path))
        }
        _ => None,
    })
}

fn npm_cache_override<'a>(
    environment: &'a PlatformEnvironment,
    lowercase: &str,
    uppercase: &str,
) -> Result<Option<&'a Path>, String> {
    let configured = |name| {
        environment
            .cache_path_override(name)
            .filter(|path| !path.as_os_str().is_empty())
    };
    let lower = configured(lowercase);
    let upper = configured(uppercase);
    if lower
        .zip(upper)
        .is_some_and(|(lower, upper)| lower != upper)
    {
        return Err(format!(
            "Conflicting {lowercase} and {uppercase} cache settings; choose one location"
        ));
    }
    Ok(lower.or(upper))
}

fn discover_path_with(
    provider: ProviderKind,
    environment: &PlatformEnvironment,
    runner: &dyn ProviderCommandRunner,
) -> Result<PathBuf, String> {
    let output = runner.discover(provider, environment, &crate::models::NeverCancelled);
    let bytes = match output {
        ProviderCommandResult::Output {
            success: true,
            stdout,
            ..
        } => stdout,
        ProviderCommandResult::Output {
            success: false,
            stderr,
            ..
        } => {
            return Err(format!(
                "{} cache discovery failed: {}",
                provider.executable(),
                bounded_message(&stderr)
            ));
        }
        ProviderCommandResult::Absent => {
            return Err(format!(
                "{} was not detected in trusted tool locations",
                provider.executable()
            ));
        }
        ProviderCommandResult::Failed(reason) => {
            return Err(format!(
                "{} cache discovery failed: {reason}",
                provider.executable()
            ));
        }
        ProviderCommandResult::Cancelled => {
            return Err(format!(
                "{} cache discovery was cancelled",
                provider.executable()
            ));
        }
    };
    parse_provider_path(provider, &bytes).and_then(|path| validate_cache_path(path, environment))
}

fn parse_provider_path(provider: ProviderKind, output: &[u8]) -> Result<AbsolutePath, String> {
    match provider.discovery_output() {
        DiscoveryOutput::BarePath => parse_discovered_path(output),
        DiscoveryOutput::LabeledPath(expected_label) => {
            if output.len() > MAX_DISCOVERY_OUTPUT {
                return Err("Cache discovery output exceeded the safety limit".to_string());
            }
            let text = std::str::from_utf8(output)
                .map_err(|_| "Cache discovery did not return UTF-8".to_string())?;
            let lines = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            if lines.len() != 1 {
                return Err("Cache discovery returned an ambiguous path".to_string());
            }
            let (label, path) = lines[0]
                .split_once(':')
                .ok_or_else(|| "Cache discovery did not return a labeled path".to_string())?;
            if !label.trim().eq_ignore_ascii_case(expected_label) {
                return Err("Cache discovery returned an unexpected resource label".to_string());
            }
            parse_discovered_path(path.trim().as_bytes())
        }
    }
}

fn parse_discovered_path(output: &[u8]) -> Result<AbsolutePath, String> {
    if output.len() > MAX_DISCOVERY_OUTPUT {
        return Err("Cache discovery output exceeded the safety limit".to_string());
    }
    let text = std::str::from_utf8(output)
        .map_err(|_| "Cache discovery did not return UTF-8".to_string())?;
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.len() != 1 || lines[0].chars().any(char::is_control) {
        return Err("Cache discovery returned an ambiguous path".to_string());
    }
    AbsolutePath::new(lines[0]).map_err(|_| "Cache discovery returned a relative path".to_string())
}

fn validate_cache_path(
    path: AbsolutePath,
    environment: &PlatformEnvironment,
) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| "The discovered cache directory is unavailable".to_string())?;
    if !metadata.is_dir() || SymlinkGuard::is_symlink(path.as_path()) {
        return Err("The discovered cache must be a real directory".to_string());
    }
    // The stated profile is the authority: the runner's own home must not
    // decide whether a cache belongs to this user.
    let home = environment
        .user_home()
        .ok_or_else(|| "Could not resolve the current user profile".to_string())?;
    let canonical_home = std::fs::canonicalize(home)
        .map_err(|_| "Could not validate the current user profile".to_string())?;
    // Every check below runs on the resolved location: an approved cache
    // reached through a link is the link's target, and that is what the
    // profile, blacklist, and symlink rules must be applied to.
    let canonical = CanonicalPath::resolve(&path)
        .map_err(|_| "Could not canonicalize the discovered cache".to_string())?;

    let flavor = environment.flavor();
    let in_profile = path_is_within(canonical.as_path(), &canonical_home, flavor);
    let mut approved =
        in_profile && cache_location_approved(canonical.as_path(), &canonical_home, flavor);
    // Stated relocated caches stay
    // supported under the same blacklist and symlink validation as in-profile
    // locations instead of being refused for being outside the profile.
    for root in relocated_cache_roots(environment) {
        approved |= path_is_same(canonical.as_path(), &root, flavor)
            || path_is_within(canonical.as_path(), &root, flavor);
    }

    if !approved {
        return Err(
            "The provider cache override is outside approved user cache locations".to_string(),
        );
    }
    Blacklist::validate_with(canonical.as_path(), environment)
        .map_err(|error| error.to_string())?;
    if in_profile {
        SymlinkGuard::validate_no_symlink_ancestors(
            canonical.as_path(),
            &canonical_home,
            environment,
        )
        .map_err(|error| error.to_string())?;
    } else {
        SymlinkGuard::validate_anchored_path(canonical.as_path(), environment)
            .map_err(|error| error.to_string())?;
    }
    Ok(canonical.into_path_buf())
}

fn relocated_cache_roots(environment: &PlatformEnvironment) -> Vec<PathBuf> {
    // The application-data containers come from the described environment;
    // supported per-tool overrides were captured in that same description at
    // composition. Discovery and cleanup thus approve the same stated root.
    let stated = [environment.local_app_data(), environment.roaming_app_data()];
    let overrides = [
        "UV_CACHE_DIR",
        "PIP_CACHE_DIR",
        "npm_config_store_dir",
        "NPM_CONFIG_STORE_DIR",
        "npm_config_cache",
        "NPM_CONFIG_CACHE",
    ]
    .into_iter()
    .filter_map(|variable| {
        environment
            .cache_path_override(variable)
            .map(Path::to_path_buf)
    });
    stated
        .into_iter()
        .flatten()
        .chain(overrides)
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .map(|root| neati_platform::NativePlatformPaths::normalize_verbatim_path(&root))
        .collect()
}

/// Same location under the environment's own flavor: case folded and
/// separator-agnostic on Windows, byte exact on POSIX. Using the flavor rather
/// than the host keeps a simulated Windows environment comparable on any
/// runner.
fn path_is_same(left: &Path, right: &Path, flavor: PathFlavor) -> bool {
    path_algebra::equal(&left.to_string_lossy(), &right.to_string_lossy(), flavor)
}

fn path_is_within(path: &Path, base: &Path, flavor: PathFlavor) -> bool {
    !path_is_same(path, base, flavor)
        && path_algebra::contains(&base.to_string_lossy(), &path.to_string_lossy(), flavor)
}

/// Pure approval check over already-canonicalized paths (no filesystem access),
/// so the trust boundary is directly unit-testable. Broad cache roots match
/// strict subdirectories; specific store roots (including the npm home
/// directory) match themselves and their contents.
fn cache_location_approved(canonical: &Path, canonical_home: &Path, flavor: PathFlavor) -> bool {
    let broad_cache_roots = [
        canonical_home.join(".cache"),
        canonical_home.join("Library/Caches"),
    ];
    let specific_store_roots = [
        canonical_home.join(".local/share/pnpm"),
        canonical_home.join("Library/pnpm"),
        canonical_home.join(".pnpm-store"),
        canonical_home.join(".npm"),
        canonical_home.join(".bun/install/cache"),
        canonical_home.join(".composer/cache"),
        canonical_home.join(".nuget/packages"),
        canonical_home.join(".local/share/NuGet"),
        canonical_home.join("go/pkg/mod"),
    ];
    broad_cache_roots.iter().any(|root| {
        !path_is_same(canonical, root, flavor) && path_is_within(canonical, root, flavor)
    }) || specific_store_roots.iter().any(|root| {
        path_is_same(canonical, root, flavor) || path_is_within(canonical, root, flavor)
    })
}

/// Home-relative install locations of version-manager-owned toolchains
/// (nvm, Volta, asdf, fnm). Narrow to each manager's own directory; the
/// caller still requires the canonical executable to live under one of these
/// roots, and execution stays limited to fixed provider arguments.
fn node_manager_roots(home: &Path) -> Vec<PathBuf> {
    [".nvm", ".volta", ".asdf", ".fnm"]
        .iter()
        .map(|dir| home.join(dir))
        .collect()
}

fn validate_executable(path: &Path, environment: &PlatformEnvironment) -> Result<(), String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| "Could not validate the provider executable".to_string())?;
    let canonical = neati_platform::NativePlatformPaths::normalize_verbatim_path(&canonical);
    let mut roots = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/usr/local/bin"),
        // Microsoft's macOS package places the real host here and commonly
        // exposes `/usr/local/bin/dotnet` as a symlink. Validation is applied
        // to the canonical executable, so the documented target must be a
        // trust root as well as the symlink directory.
        PathBuf::from("/usr/local/share/dotnet"),
        PathBuf::from("/opt/homebrew"),
    ];
    roots.extend(neati_platform::NativePlatformPaths::trusted_tool_roots(
        environment.user_home().as_deref(),
    ));
    if let Some(home) = environment.user_home() {
        roots.extend([
            home.join(".local/bin"),
            home.join(".local/share/uv"),
            home.join(".local/share/pnpm"),
            home.join(".cargo/bin"),
            home.join(".npm-global/bin"),
            home.join("Library/pnpm"),
        ]);
        roots.extend(node_manager_roots(&home));
    }

    // Compare canonical locations when a root exists. This matters on macOS,
    // where `/var` resolves to `/private/var`, and for package-manager shims
    // whose documented root may itself be reached through a link.
    let roots = roots
        .into_iter()
        .map(|root| {
            std::fs::canonicalize(&root)
                .map(|path| neati_platform::NativePlatformPaths::normalize_verbatim_path(&path))
                .unwrap_or(root)
        })
        .collect::<Vec<_>>();

    if executable_under_roots(&canonical, &roots) {
        Ok(())
    } else {
        Err("The provider executable is outside trusted install locations".to_string())
    }
}

/// Pure matcher over an already-canonicalized executable and its trust roots.
/// Kept separate so the Windows trust boundary can be tested without touching
/// the real filesystem. Windows-style paths always use the Windows comparison
/// so the fixtures remain meaningful on Unix hosts.
fn executable_under_roots(canonical: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        let norm_root = neati_platform::NativePlatformPaths::normalize_verbatim_path(root);
        if looks_like_windows_path(canonical) || looks_like_windows_path(&norm_root) {
            neati_platform::NativePlatformPaths::windows_path_starts_with(canonical, &norm_root)
        } else {
            canonical.starts_with(&norm_root)
        }
    })
}

fn looks_like_windows_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("\\\\")
        || text.starts_with("//")
        || matches!(
            text.as_bytes(),
            [drive, b':', ..] if drive.is_ascii_alphabetic()
        )
}

fn paths_match(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn matching_process_is_active(provider: ProviderKind) -> Option<bool> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let processes: Vec<_> = system.processes().values().collect();
    process_table_activity(&processes, |process| {
        process_matches_provider(provider, process.name(), process.cmd())
    })
}

fn owner_process_refusal(
    provider: ProviderKind,
    active: Option<bool>,
) -> Option<CacheProviderPruneFailure> {
    match active {
        Some(false) => None,
        Some(true) => Some(CacheProviderPruneFailure::OwnerRunning(
            provider.executable().to_string(),
        )),
        None => Some(CacheProviderPruneFailure::OwnerStateUnknown(
            provider.executable().to_string(),
        )),
    }
}

fn process_table_activity<T>(
    processes: &[T],
    mut matches_owner: impl FnMut(&T) -> bool,
) -> Option<bool> {
    if processes.is_empty() {
        None
    } else {
        Some(processes.iter().any(&mut matches_owner))
    }
}

fn process_matches_provider(
    provider: ProviderKind,
    process_name: &std::ffi::OsStr,
    command: &[std::ffi::OsString],
) -> bool {
    let name = process_name.to_string_lossy();
    if provider == ProviderKind::Pip {
        return pip_executable_name(&name)
            || command.first().is_some_and(|arg| {
                Path::new(arg)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(pip_executable_name)
            })
            || python_pip_module(command);
    }
    provider.active_processes().iter().any(|expected| {
        name.eq_ignore_ascii_case(expected)
            || name.eq_ignore_ascii_case(&format!("{expected}.exe"))
            || command.iter().take(3).any(|argument| {
                Path::new(argument)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| {
                        value.eq_ignore_ascii_case(expected)
                            || value.eq_ignore_ascii_case(&format!("{expected}.exe"))
                            || value.starts_with(&format!("{expected}."))
                    })
            })
    })
}

fn pip_executable_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "pip" | "pip3" | "pip.exe" | "pip3.exe")
        || name.strip_prefix("pip3.").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn python_pip_module(command: &[std::ffi::OsString]) -> bool {
    let Some(executable) = command.first().and_then(|arg| Path::new(arg).file_name()) else {
        return false;
    };
    let executable = executable.to_string_lossy().to_ascii_lowercase();
    if !executable.starts_with("python") && !executable.starts_with("pypy") {
        return false;
    }
    let mut args = command.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-m") => return args.next().is_some_and(|module| module == "pip"),
            Some("-c" | "--") => return false,
            Some("-W" | "-X") => {
                if args.next().is_none() {
                    return false;
                }
            }
            Some(option) if option.starts_with('-') && option != "--" => {}
            _ => return false,
        }
    }
    false
}

fn bounded_message(bytes: &[u8]) -> String {
    crate::diagnostics::sanitize_log(
        String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).trim(),
    )
}

#[cfg(test)]
mod tests;
