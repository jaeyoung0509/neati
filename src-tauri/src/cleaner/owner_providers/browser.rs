//! Owner-scoped cleanup for Chromium-managed download and offline caches.
//!
//! Chromium's component updater keeps one metadata-coupled store at
//! `component_crx_cache`; deleting individual payloads would leave that index
//! inconsistent. CacheStorage likewise owns indexes below one exact profile
//! subtree. These adapters therefore move only the complete owner store to the
//! operating system Trash, and only while every supported browser process is
//! known to be stopped. Profile databases, extensions, installed components,
//! cookies, history, and Service Worker registration data are siblings and are
//! never part of either unit.

use super::OwnerScopedProvider;
use crate::models::{
    CleanFailureReason, OwnerProviderRefusal, OwnerProviderSelection, OwnerProviderUnit,
    OwnerStoreObservation, OwnerUnitObservation, OwnerUnitOutcome, OwnerUnitState, PlatformKind,
    ProviderStatus,
};
use crate::safety::{Blacklist, SymlinkGuard, ToctouGuard};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zenith_core::domain::cleanup::{
    OwnerProviderAuthorization, OwnerProviderExecution, OwnerUnitMeasurer, RunningProcessPolicy,
    RunningProcessProbe,
};
use zenith_platform::{PlatformEnvironment, TrashBackend};

const MAX_STORE_ENTRIES: usize = 200_000;
const MAX_STORE_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserCacheKind {
    ComponentDownloads,
    OfflineCacheStorage,
}

impl BrowserCacheKind {
    fn provider_id(self) -> &'static str {
        match self {
            Self::ComponentDownloads => "chromium.component_downloads",
            Self::OfflineCacheStorage => "chromium.offline_cache_storage",
        }
    }

    fn consequence(self) -> &'static str {
        match self {
            Self::ComponentDownloads => {
                "Downloaded component update archives move to Trash. Empty Trash to free disk space; the browser may download them again when an update needs them."
            }
            Self::OfflineCacheStorage => {
                "Offline website assets move to Trash. Sites and installed web apps may need a network connection to download them again; cookies, passwords, history, local storage, IndexedDB, sessions, and Service Worker registrations stay intact."
            }
        }
    }

    fn needs_confirmation(self) -> bool {
        matches!(self, Self::OfflineCacheStorage)
    }
}

#[derive(Clone, Copy)]
struct BrowserLayout {
    key: &'static str,
    label: &'static str,
    relative: &'static str,
}

const MAC_LAYOUTS: &[BrowserLayout] = &[
    BrowserLayout {
        key: "chrome",
        label: "Chrome",
        relative: "Google/Chrome",
    },
    BrowserLayout {
        key: "chrome-canary",
        label: "Chrome Canary",
        relative: "Google/Chrome Canary",
    },
    BrowserLayout {
        key: "brave",
        label: "Brave",
        relative: "BraveSoftware/Brave-Browser",
    },
    BrowserLayout {
        key: "edge",
        label: "Microsoft Edge",
        relative: "Microsoft Edge",
    },
    BrowserLayout {
        key: "edge-beta",
        label: "Microsoft Edge Beta",
        relative: "Microsoft Edge Beta",
    },
    BrowserLayout {
        key: "edge-dev",
        label: "Microsoft Edge Dev",
        relative: "Microsoft Edge Dev",
    },
    BrowserLayout {
        key: "edge-canary",
        label: "Microsoft Edge Canary",
        relative: "Microsoft Edge Canary",
    },
    BrowserLayout {
        key: "chromium",
        label: "Chromium",
        relative: "Chromium",
    },
    BrowserLayout {
        key: "vivaldi",
        label: "Vivaldi",
        relative: "Vivaldi",
    },
    BrowserLayout {
        key: "arc",
        label: "Arc",
        relative: "company.thebrowser.Browser",
    },
];

const WINDOWS_LAYOUTS: &[BrowserLayout] = &[
    BrowserLayout {
        key: "chrome",
        label: "Chrome",
        relative: "Google/Chrome/User Data",
    },
    BrowserLayout {
        key: "chrome-canary",
        label: "Chrome Canary",
        relative: "Google/Chrome SxS/User Data",
    },
    BrowserLayout {
        key: "brave",
        label: "Brave",
        relative: "BraveSoftware/Brave-Browser/User Data",
    },
    BrowserLayout {
        key: "edge",
        label: "Microsoft Edge",
        relative: "Microsoft/Edge/User Data",
    },
    BrowserLayout {
        key: "edge-beta",
        label: "Microsoft Edge Beta",
        relative: "Microsoft/Edge Beta/User Data",
    },
    BrowserLayout {
        key: "edge-dev",
        label: "Microsoft Edge Dev",
        relative: "Microsoft/Edge Dev/User Data",
    },
    BrowserLayout {
        key: "edge-canary",
        label: "Microsoft Edge Canary",
        relative: "Microsoft/Edge SxS/User Data",
    },
    BrowserLayout {
        key: "chromium",
        label: "Chromium",
        relative: "Chromium/User Data",
    },
    BrowserLayout {
        key: "vivaldi",
        label: "Vivaldi",
        relative: "Vivaldi/User Data",
    },
    BrowserLayout {
        key: "arc",
        label: "Arc",
        relative: "TheBrowserCompany/Arc/User Data",
    },
];

pub struct ChromiumCacheProvider {
    kind: BrowserCacheKind,
    process: Arc<dyn RunningProcessProbe>,
    measuring: Arc<dyn OwnerUnitMeasurer>,
    trash: Arc<dyn TrashBackend>,
}

impl ChromiumCacheProvider {
    pub fn new(
        kind: BrowserCacheKind,
        process: Arc<dyn RunningProcessProbe>,
        measuring: Arc<dyn OwnerUnitMeasurer>,
        trash: Arc<dyn TrashBackend>,
    ) -> Self {
        Self {
            kind,
            process,
            measuring,
            trash,
        }
    }

    fn layouts(environment: &PlatformEnvironment) -> &'static [BrowserLayout] {
        match environment.platform() {
            PlatformKind::Windows => WINDOWS_LAYOUTS,
            PlatformKind::Macos => MAC_LAYOUTS,
            _ => &[],
        }
    }

    fn base(environment: &PlatformEnvironment) -> Option<PathBuf> {
        match environment.platform() {
            PlatformKind::Macos => environment
                .user_home()
                .map(|home| home.join("Library/Application Support")),
            PlatformKind::Windows => environment.local_app_data(),
            _ => None,
        }
    }

    fn read_store(
        &self,
        environment: &PlatformEnvironment,
        _guard: &RunningProcessPolicy,
    ) -> OwnerStoreObservation {
        let Some(base) = Self::base(environment) else {
            return OwnerStoreObservation::refused(
                ProviderStatus::Unsupported,
                None,
                "This environment has no supported browser profile root",
            );
        };
        let mut units = Vec::new();
        for layout in Self::layouts(environment) {
            let browser_root = base.join(layout.relative);
            match self.kind {
                BrowserCacheKind::ComponentDownloads => {
                    let path = browser_root.join("component_crx_cache");
                    if fs::symlink_metadata(&path)
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                    {
                        continue;
                    }
                    let owner = self.owner_state(layout);
                    if let Some(unit) =
                        self.observe_unit(environment, layout, None, &browser_root, path, &owner)
                    {
                        units.push(unit);
                    }
                }
                BrowserCacheKind::OfflineCacheStorage => {
                    let profiles = match direct_profiles(&browser_root) {
                        Ok(profiles) => profiles,
                        Err(ProfileRead::Missing) => continue,
                        Err(ProfileRead::Unsafe(detail)) => {
                            return OwnerStoreObservation::refused(
                                ProviderStatus::Blocked,
                                Some(browser_root),
                                detail,
                            )
                        }
                    };
                    let paths: Vec<(String, PathBuf)> = profiles
                        .into_iter()
                        .map(|profile| {
                            let path = browser_root
                                .join(&profile)
                                .join("Service Worker")
                                .join("CacheStorage");
                            (profile, path)
                        })
                        .filter(|(_, path)| {
                            !fs::symlink_metadata(path)
                                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                        })
                        .collect();
                    if paths.is_empty() {
                        continue;
                    }
                    let owner = self.owner_state(layout);
                    for (profile, path) in paths {
                        if let Some(unit) = self.observe_unit(
                            environment,
                            layout,
                            Some(&profile),
                            &browser_root,
                            path,
                            &owner,
                        ) {
                            units.push(unit);
                        }
                    }
                }
            }
        }
        OwnerStoreObservation::ready(Some(base), units)
    }

    fn owner_state(&self, layout: &BrowserLayout) -> BrowserOwnerState {
        match self.process.running(&browser_guard(layout)) {
            Some(running) if running.is_empty() => BrowserOwnerState::Idle,
            Some(running) => BrowserOwnerState::Running(running),
            None => BrowserOwnerState::Unknown,
        }
    }

    fn observe_unit(
        &self,
        environment: &PlatformEnvironment,
        layout: &BrowserLayout,
        profile: Option<&str>,
        browser_root: &Path,
        path: PathBuf,
        owner: &BrowserOwnerState,
    ) -> Option<OwnerUnitObservation> {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                return Some(OwnerUnitObservation::blocked(
                    unit_key(layout.key, profile),
                    path,
                    0,
                    0,
                    0,
                    format!("The browser cache could not be inspected: {error}"),
                ))
            }
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                return Some(OwnerUnitObservation::blocked(
                    unit_key(layout.key, profile),
                    path,
                    0,
                    0,
                    0,
                    "The browser cache is not an ordinary directory",
                ))
            }
            Ok(_) => {}
        }
        if SymlinkGuard::validate_anchored_path(browser_root, environment).is_err() {
            return Some(OwnerUnitObservation::blocked(
                unit_key(layout.key, profile),
                path,
                0,
                0,
                0,
                "The browser profile root cannot be safely resolved",
            ));
        }
        let measurement = self.measuring.measure(&path);
        if !measurement.complete {
            return Some(OwnerUnitObservation::blocked(
                unit_key(layout.key, profile),
                path,
                measurement.logical_bytes,
                measurement.allocated_bytes,
                measurement.entry_count,
                measurement
                    .detail
                    .unwrap_or_else(|| "The browser cache could not be measured completely".into()),
            ));
        }
        if let Err(detail) = inspect_store(&path) {
            return Some(OwnerUnitObservation::blocked(
                unit_key(layout.key, profile),
                path,
                measurement.logical_bytes,
                measurement.allocated_bytes,
                measurement.entry_count,
                detail,
            ));
        }
        if measurement.allocated_bytes == 0 {
            return None;
        }
        let key = unit_key(layout.key, profile);
        Some(match owner {
            BrowserOwnerState::Idle => OwnerUnitObservation::ready(
                key,
                path,
                measurement.logical_bytes,
                measurement.allocated_bytes,
                measurement.entry_count,
            ),
            BrowserOwnerState::Running(running) => OwnerUnitObservation::refused(
                key,
                path,
                measurement.logical_bytes,
                measurement.allocated_bytes,
                measurement.entry_count,
                format!(
                    "Close {} before cleaning this browser-owned cache",
                    running.join(", ")
                ),
            ),
            BrowserOwnerState::Unknown => OwnerUnitObservation::blocked(
                key,
                path,
                measurement.logical_bytes,
                measurement.allocated_bytes,
                measurement.entry_count,
                "The process table could not prove this browser is stopped",
            ),
        })
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
                    .unwrap_or_else(|| "Browser cache is unavailable".into()),
                Vec::new(),
            ));
        }
        let mut plan = OwnerProviderAuthorization {
            deletion_disposition:
                zenith_core::domain::cleanup::DeletionDisposition::PermanentDelete,
            signature_id: String::new(),
            provider_id: self.id().into(),
            risk: crate::models::RiskTier::Rebuild,
            units: Vec::new(),
            refusals: Vec::new(),
            process_guard: guard.clone(),
            requires_confirmation: self.requires_confirmation(),
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
                    detail:
                        "The selected browser cache changed or is no longer eligible; scan again"
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
                    detail: "The browser cache identity could not be captured".into(),
                });
                continue;
            };
            let Some(root) = self.browser_root_for(environment, unit) else {
                plan.refusals.push(crate::models::OwnerUnitRefusal {
                    item_id: selection.item_id.clone(),
                    item_name: selection.name.clone(),
                    status: ProviderStatus::Blocked,
                    reason: CleanFailureReason::ProviderRefused,
                    detail: "The browser cache no longer matches a supported profile layout".into(),
                });
                continue;
            };
            plan.units.push(OwnerProviderUnit {
                item_id: selection.item_id.clone(),
                unit_key: unit.unit_key.clone(),
                name: selection.name.clone(),
                root,
                path: unit.path.clone(),
                identity,
                expected_bytes: unit.allocated_bytes,
                entry_count: unit.entry_count,
            });
        }
        if plan.units.is_empty() {
            return Err(OwnerProviderRefusal::for_selections(
                ProviderStatus::Blocked,
                "No selected browser cache is still eligible",
                plan.refusals,
            ));
        }
        Ok(plan)
    }

    fn browser_root_for(
        &self,
        environment: &PlatformEnvironment,
        unit: &OwnerUnitObservation,
    ) -> Option<PathBuf> {
        let base = Self::base(environment)?;
        let browser_key = unit.unit_key.split('/').next()?;
        let layout = Self::layouts(environment)
            .iter()
            .find(|layout| layout.key == browser_key)?;
        let root = base.join(layout.relative);
        let expected = match self.kind {
            BrowserCacheKind::ComponentDownloads => root.join("component_crx_cache"),
            BrowserCacheKind::OfflineCacheStorage => {
                let profile = unit.unit_key.split_once('/')?.1;
                if !profile_name(profile) {
                    return None;
                }
                root.join(profile)
                    .join("Service Worker")
                    .join("CacheStorage")
            }
        };
        (unit.path == expected).then_some(root)
    }

    fn move_unit(
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
        let observation = self.read_store(environment, &authorization.process_guard);
        if !observation.status.is_ready() {
            return refuse(
                observation
                    .detail
                    .unwrap_or_else(|| "The browser cache is unavailable".into()),
            );
        }
        let current = observation.units.iter().find(|candidate| {
            candidate.unit_key == unit.unit_key
                && candidate.path == unit.path
                && candidate.state == OwnerUnitState::Ready
                && candidate.allocated_bytes == unit.expected_bytes
        });
        if current.is_none() {
            return refuse("The browser cache changed since review; scan again".into());
        }
        if let Err(error) = Blacklist::validate_with(&unit.path, environment) {
            return refuse(format!("The browser cache path is protected: {error}"));
        }
        if let Err(error) = SymlinkGuard::validate_anchored_path(&unit.path, environment) {
            return refuse(format!("The browser cache path is unsafe: {error}"));
        }
        if let Err(error) =
            SymlinkGuard::validate_canonical_blacklist_strict(&unit.path, environment)
        {
            return refuse(format!(
                "The resolved browser cache path is protected: {error}"
            ));
        }
        if let Err(error) = ToctouGuard::verify(&unit.path, &unit.identity) {
            return refuse(format!("The browser cache changed since review: {error}"));
        }
        if let Err(detail) = inspect_store(&unit.path) {
            return refuse(detail);
        }
        if let Err(error) = self.trash.move_to_trash(&unit.path) {
            return refuse(format!(
                "The browser cache could not be moved to Trash: {error}"
            ));
        }
        match fs::symlink_metadata(&unit.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => OwnerUnitOutcome::cleaned(
                unit.item_id.clone(),
                unit.unit_key.clone(),
                unit.expected_bytes,
            ),
            _ => OwnerUnitOutcome::partially_cleaned(
                unit.item_id.clone(),
                unit.unit_key.clone(),
                0,
                None,
                "The browser cache still exists after the Trash move; a replacement was left in place",
            ),
        }
    }
}

impl OwnerScopedProvider for ChromiumCacheProvider {
    fn id(&self) -> &'static str {
        self.kind.provider_id()
    }

    fn platforms(&self) -> &'static [PlatformKind] {
        &[PlatformKind::Macos, PlatformKind::Windows]
    }

    fn consequence(&self) -> &'static str {
        self.kind.consequence()
    }

    fn requires_confirmation(&self) -> bool {
        self.kind.needs_confirmation()
    }

    fn unit_label(&self, unit: &OwnerUnitObservation) -> String {
        let (browser_key, profile) = unit
            .unit_key
            .split_once('/')
            .map_or((unit.unit_key.as_str(), None), |(browser, profile)| {
                (browser, Some(profile))
            });
        let browser = MAC_LAYOUTS
            .iter()
            .chain(WINDOWS_LAYOUTS.iter())
            .find(|layout| layout.key == browser_key)
            .map_or(browser_key, |layout| layout.label);
        match (self.kind, profile) {
            (BrowserCacheKind::ComponentDownloads, _) => format!("{browser} component downloads"),
            (BrowserCacheKind::OfflineCacheStorage, Some(profile)) => {
                format!("{browser} {profile} offline cache")
            }
            (BrowserCacheKind::OfflineCacheStorage, None) => format!("{browser} offline cache"),
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
        OwnerProviderExecution {
            units: authorization
                .units
                .iter()
                .map(|unit| self.move_unit(environment, authorization, unit))
                .collect(),
        }
    }
}

fn unit_key(browser: &str, profile: Option<&str>) -> String {
    profile.map_or_else(
        || browser.to_string(),
        |profile| format!("{browser}/{profile}"),
    )
}

enum ProfileRead {
    Missing,
    Unsafe(String),
}

enum BrowserOwnerState {
    Idle,
    Running(Vec<String>),
    Unknown,
}

fn browser_guard(layout: &BrowserLayout) -> RunningProcessPolicy {
    let names: &[&str] = match layout.key {
        "chrome" | "chrome-canary" => &[
            "Google Chrome",
            "Google Chrome Helper",
            "Google Chrome Helper (Renderer)",
            "Google Chrome Helper (GPU)",
            "chrome",
            "chrome.exe",
        ],
        "brave" => &[
            "Brave Browser",
            "Brave Browser Helper",
            "Brave Browser Helper (Renderer)",
            "Brave Browser Helper (GPU)",
            "brave",
            "brave.exe",
        ],
        "edge" | "edge-beta" | "edge-dev" | "edge-canary" => &[
            "Microsoft Edge",
            "Microsoft Edge Helper",
            "msedge",
            "msedge.exe",
        ],
        "chromium" => &["Chromium", "Chromium Helper", "chromium", "chromium.exe"],
        "vivaldi" => &["Vivaldi", "Vivaldi Helper", "vivaldi", "vivaldi.exe"],
        "arc" => &["Arc", "Arc Helper", "arc", "arc.exe"],
        _ => &[],
    };
    RunningProcessPolicy::guarding(names.iter().map(|name| (*name).to_string()).collect())
}

fn direct_profiles(root: &Path) -> Result<Vec<String>, ProfileRead> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProfileRead::Missing
        } else {
            ProfileRead::Unsafe(format!(
                "A browser profile root could not be inspected: {error}"
            ))
        }
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ProfileRead::Unsafe(
            "A browser profile root is not an ordinary directory".into(),
        ));
    }
    let entries = fs::read_dir(root).map_err(|error| {
        ProfileRead::Unsafe(format!(
            "A browser profile root could not be listed: {error}"
        ))
    })?;
    let mut profiles = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            ProfileRead::Unsafe(format!("A browser profile listing was incomplete: {error}"))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if profile_name(&name) {
            profiles.push(name);
        }
    }
    profiles.sort();
    Ok(profiles)
}

fn profile_name(name: &str) -> bool {
    matches!(name, "Default" | "Guest Profile" | "System Profile")
        || name.strip_prefix("Profile ").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn inspect_store(root: &Path) -> Result<(), String> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((path, depth)) = stack.pop() {
        if depth > MAX_STORE_DEPTH {
            return Err("The browser cache exceeds the supported directory depth".into());
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("A browser cache entry could not be inspected: {error}"))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err("The browser cache contains a symbolic link".into());
        }
        if metadata.is_file() {
            continue;
        }
        if !metadata.is_dir() {
            return Err("The browser cache contains an unsupported filesystem object".into());
        }
        let entries = fs::read_dir(&path)
            .map_err(|error| format!("A browser cache directory could not be listed: {error}"))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("A browser cache listing was incomplete: {error}"))?;
            seen += 1;
            if seen > MAX_STORE_ENTRIES {
                return Err("The browser cache exceeds the supported entry limit".into());
            }
            stack.push((entry.path(), depth + 1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::SizeCalculatorMeasurement;
    use std::sync::Mutex;

    struct Idle;
    impl RunningProcessProbe for Idle {
        fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(Vec::new())
        }
    }

    struct Busy;
    impl RunningProcessProbe for Busy {
        fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(vec!["Google Chrome".into()])
        }
    }

    struct Unknown;
    impl RunningProcessProbe for Unknown {
        fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            None
        }
    }

    struct StartsAfterReview {
        chrome_checks: std::sync::atomic::AtomicUsize,
    }
    impl RunningProcessProbe for StartsAfterReview {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            let is_chrome = guard
                .executables()
                .iter()
                .any(|name| name == "Google Chrome");
            if is_chrome
                && self
                    .chrome_checks
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    >= 2
            {
                Some(vec!["Google Chrome".into()])
            } else {
                Some(Vec::new())
            }
        }
    }

    #[derive(Default)]
    struct FixtureTrash {
        moved: Mutex<Vec<PathBuf>>,
    }
    impl TrashBackend for FixtureTrash {
        fn move_to_trash(&self, path: &Path) -> Result<(), String> {
            self.moved.lock().unwrap().push(path.to_path_buf());
            fs::remove_dir_all(path).map_err(|error| error.to_string())
        }
    }

    fn provider(
        kind: BrowserCacheKind,
        process: Arc<dyn RunningProcessProbe>,
        trash: Arc<dyn TrashBackend>,
    ) -> ChromiumCacheProvider {
        ChromiumCacheProvider::new(kind, process, Arc::new(SizeCalculatorMeasurement), trash)
    }

    fn mac_fixture() -> (tempfile::TempDir, PlatformEnvironment, RunningProcessPolicy) {
        let temp = tempfile::tempdir().unwrap();
        let environment = PlatformEnvironment::native()
            .with_platform(PlatformKind::Macos)
            .with_home(temp.path());
        let guard = RunningProcessPolicy::guarding(vec!["Google Chrome".into()]);
        (temp, environment, guard)
    }

    #[test]
    fn windows_layouts_resolve_from_the_stated_local_app_data() {
        let local = PathBuf::from(r"D:\Users\tester\AppData\Local");
        let roots = zenith_platform::paths::SimulatedPaths::new()
            .with_flavor(zenith_platform::PathFlavor::Windows)
            .with_local_app_data(local.clone());
        let environment = PlatformEnvironment::simulated(zenith_platform::PathFlavor::Windows)
            .with_platform(PlatformKind::Windows)
            .with_roots(Arc::new(roots));
        assert_eq!(
            ChromiumCacheProvider::base(&environment),
            Some(local.clone())
        );
        assert_eq!(
            local.join(WINDOWS_LAYOUTS[0].relative),
            local.join("Google/Chrome/User Data")
        );
        assert!(WINDOWS_LAYOUTS
            .iter()
            .all(|layout| !layout.relative.contains('*') && !layout.relative.contains("..")));
    }

    fn selection(unit: &OwnerUnitObservation) -> OwnerProviderSelection {
        OwnerProviderSelection {
            item_id: "browser-item".into(),
            name: "Browser cache".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        }
    }

    #[test]
    fn component_provider_moves_only_the_complete_component_store() {
        let (_temp, environment, guard) = mac_fixture();
        let user_data = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome");
        let component = user_data.join("component_crx_cache");
        fs::create_dir_all(&component).unwrap();
        fs::write(component.join("metadata.json"), b"not parsed independently").unwrap();
        fs::write(component.join("payload.crx3"), vec![1; 8192]).unwrap();
        let extension = user_data.join("Default/Extensions/keep");
        fs::create_dir_all(&extension).unwrap();
        fs::write(extension.join("manifest.json"), b"keep").unwrap();
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(Idle),
            trash.clone(),
        );

        let observation = provider.scan(&environment, &guard);
        assert_eq!(observation.units.len(), 1);
        assert_eq!(observation.units[0].state, OwnerUnitState::Ready);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&observation.units[0])])
            .unwrap();
        let result = provider.execute(&environment, &plan);

        assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
        assert!(!component.exists());
        assert!(extension.exists());
        assert_eq!(trash.moved.lock().unwrap().as_slice(), &[component]);
    }

    #[test]
    fn offline_provider_removes_cache_storage_but_preserves_profile_state() {
        let (_temp, environment, guard) = mac_fixture();
        let profile = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default");
        let cache = profile.join("Service Worker/CacheStorage");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("index.txt"), vec![2; 8192]).unwrap();
        for sibling in [
            profile.join("Cookies"),
            profile.join("History"),
            profile.join("Local Storage/state"),
            profile.join("IndexedDB/state"),
            profile.join("Service Worker/Database/registration"),
        ] {
            fs::create_dir_all(sibling.parent().unwrap()).unwrap();
            fs::write(sibling, b"keep").unwrap();
        }
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Idle),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        assert_eq!(observation.units.len(), 1);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&observation.units[0])])
            .unwrap();
        assert!(plan.requires_confirmation);
        provider.execute(&environment, &plan);

        assert!(!cache.exists());
        assert!(profile.join("Cookies").exists());
        assert!(profile.join("History").exists());
        assert!(profile.join("Local Storage/state").exists());
        assert!(profile.join("IndexedDB/state").exists());
        assert!(profile
            .join("Service Worker/Database/registration")
            .exists());
    }

    #[test]
    fn multiple_profiles_are_enumerated_without_recursive_discovery() {
        let (_temp, environment, guard) = mac_fixture();
        let root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome");
        for profile in ["Default", "Profile 2"] {
            let cache = root.join(profile).join("Service Worker/CacheStorage");
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join("entry"), vec![3; 4096]).unwrap();
        }
        let nested = root.join("Archive/Profile 9/Service Worker/CacheStorage");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("entry"), vec![4; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Idle),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        assert_eq!(observation.units.len(), 2);
        assert!(observation
            .units
            .iter()
            .any(|unit| unit.unit_key == "chrome/Default"));
        assert!(observation
            .units
            .iter()
            .any(|unit| unit.unit_key == "chrome/Profile 2"));
    }

    #[test]
    fn running_browser_keeps_its_measured_units_visible_but_refused() {
        let (_temp, environment, guard) = mac_fixture();
        let cache = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/component_crx_cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), vec![1; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(Busy),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        assert_eq!(observation.status, ProviderStatus::Ready);
        assert_eq!(observation.units[0].state, OwnerUnitState::Refused);
        assert!(observation.units[0].allocated_bytes > 0);
    }

    #[test]
    fn unknown_process_state_keeps_measured_units_visible_but_blocked() {
        let (_temp, environment, guard) = mac_fixture();
        let cache = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/component_crx_cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), vec![1; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(Unknown),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        assert_eq!(observation.units[0].state, OwnerUnitState::Blocked);
        assert!(observation.units[0].allocated_bytes > 0);
    }

    #[test]
    fn browser_starting_after_review_blocks_execution() {
        let (_temp, environment, guard) = mac_fixture();
        let cache = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/component_crx_cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), vec![1; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(StartsAfterReview {
                chrome_checks: std::sync::atomic::AtomicUsize::new(0),
            }),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&observation.units[0])])
            .unwrap();
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Blocked);
        assert!(cache.exists());
    }

    #[cfg(unix)]
    #[test]
    fn link_inside_store_blocks_the_unit() {
        use std::os::unix::fs::symlink;
        let (_temp, environment, guard) = mac_fixture();
        let cache = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/component_crx_cache");
        fs::create_dir_all(&cache).unwrap();
        symlink("/tmp", cache.join("escape")).unwrap();
        let provider = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(Idle),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        assert_eq!(observation.units[0].state, OwnerUnitState::Blocked);
    }

    #[test]
    fn replacement_after_review_is_refused() {
        let (_temp, environment, guard) = mac_fixture();
        let cache = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/component_crx_cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), vec![5; 8192]).unwrap();
        let provider = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(Idle),
            Arc::new(FixtureTrash::default()),
        );
        let observation = provider.scan(&environment, &guard);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&observation.units[0])])
            .unwrap();
        fs::remove_dir_all(&cache).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), vec![5; 8192]).unwrap();

        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Blocked);
        assert!(cache.exists());
    }
}
