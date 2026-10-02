//! Owner-scoped cleanup for Chromium-managed download and offline caches.
//!
//! Component downloads are one metadata-coupled store. Offline CacheStorage
//! uses depth-two cache directories, matching Mole's deletion granularity.
//! Origin indexes and Service Worker registrations remain outside those units.
//! Every unit is re-enumerated before Trash. On macOS, disposable offline
//! units use exact open-file checks; coupled component stores require owner exit.

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
use neati_platform::{PlatformEnvironment, TrashBackend};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_STORE_ENTRIES: usize = 200_000;
const MAX_STORE_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserCacheKind {
    ComponentDownloads,
    OfflineCacheStorage,
    RendererCaches,
}

impl BrowserCacheKind {
    fn provider_id(self) -> &'static str {
        match self {
            Self::ComponentDownloads => "chromium.component_downloads",
            Self::OfflineCacheStorage => "chromium.offline_cache_storage",
            Self::RendererCaches => "chromium.additional_renderer_caches",
        }
    }

    fn consequence(self) -> &'static str {
        match self {
            Self::ComponentDownloads => {
                "Downloaded component update archives move to Trash. Empty Trash to free disk space; the browser may download them again when an update needs them."
            }
            Self::RendererCaches => "Named HTTP, code and GPU caches move to Trash and are downloaded or rebuilt again. Profile state, offline websites, extensions, credentials, settings and models remain intact.",
            Self::OfflineCacheStorage => {
                "Offline website assets move to Trash. Sites and installed web apps may need a network connection to download them again; cookies, passwords, history, local storage, IndexedDB, sessions, and Service Worker registrations stay intact."
            }
        }
    }

    fn needs_confirmation(self) -> bool {
        matches!(self, Self::OfflineCacheStorage)
    }
}

/// Narrows the catalog's multi-browser guard to the browser owning this unit.
pub(crate) fn cache_owner_guard(
    path: &Path,
    environment: &PlatformEnvironment,
) -> Option<RunningProcessPolicy> {
    let base = ChromiumCacheProvider::base(environment)?;
    ChromiumCacheProvider::layouts(environment)
        .iter()
        // Variant channels can share helper names with stable builds. Until
        // shutdown maps their bundle identity, leave those apps for manual exit.
        .find(|layout| !layout.key.contains('-') && path.starts_with(base.join(layout.relative)))
        .map(browser_guard)
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
        relative: "Arc/User Data",
    },
    BrowserLayout {
        key: "chrome-beta",
        label: "Chrome Beta",
        relative: "Google/Chrome Beta",
    },
    BrowserLayout {
        key: "chrome-dev",
        label: "Chrome Dev",
        relative: "Google/Chrome Dev",
    },
    BrowserLayout {
        key: "mcp-stable",
        label: "Chrome DevTools MCP",
        relative: "chrome-devtools-mcp/chrome-profile",
    },
    BrowserLayout {
        key: "mcp-beta",
        label: "Chrome DevTools MCP Beta",
        relative: "chrome-devtools-mcp/chrome-profile-beta",
    },
    BrowserLayout {
        key: "mcp-dev",
        label: "Chrome DevTools MCP Dev",
        relative: "chrome-devtools-mcp/chrome-profile-dev",
    },
    BrowserLayout {
        key: "mcp-canary",
        label: "Chrome DevTools MCP Canary",
        relative: "chrome-devtools-mcp/chrome-profile-canary",
    },
    BrowserLayout {
        key: "mcp-cli-stable",
        label: "Chrome DevTools MCP CLI",
        relative: "chrome-devtools-mcp-cli/chrome-profile",
    },
    BrowserLayout {
        key: "mcp-cli-beta",
        label: "Chrome DevTools MCP CLI Beta",
        relative: "chrome-devtools-mcp-cli/chrome-profile-beta",
    },
    BrowserLayout {
        key: "mcp-cli-dev",
        label: "Chrome DevTools MCP CLI Dev",
        relative: "chrome-devtools-mcp-cli/chrome-profile-dev",
    },
    BrowserLayout {
        key: "mcp-cli-canary",
        label: "Chrome DevTools MCP CLI Canary",
        relative: "chrome-devtools-mcp-cli/chrome-profile-canary",
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

    fn root(environment: &PlatformEnvironment, layout: &BrowserLayout) -> Option<PathBuf> {
        if layout.key.starts_with("mcp-") {
            environment
                .user_home()
                .map(|home| home.join(".cache").join(layout.relative))
        } else {
            Self::base(environment).map(|base| base.join(layout.relative))
        }
    }

    fn read_store(
        &self,
        environment: &PlatformEnvironment,
        _guard: &RunningProcessPolicy,
    ) -> OwnerStoreObservation {
        self.read_store_with_cancellation(environment, &crate::models::NeverCancelled)
    }

    fn read_store_with_cancellation(
        &self,
        environment: &PlatformEnvironment,
        cancellation: &dyn crate::models::CancellationProbe,
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
            if cancellation.is_cancelled() {
                break;
            }
            if layout.key.starts_with("mcp-") && self.kind != BrowserCacheKind::RendererCaches {
                continue;
            }
            let browser_root = Self::root(environment, layout).expect("resolved browser base");
            match self.kind {
                BrowserCacheKind::RendererCaches => {
                    // Stable Chrome/Brave leaves retain their existing registered pipeline.
                    if environment.platform() != PlatformKind::Macos
                        || !(layout.key == "arc"
                            || layout.key.starts_with("mcp-")
                            || matches!(layout.key, "chrome-beta" | "chrome-dev" | "chrome-canary"))
                    {
                        continue;
                    }
                    let profiles = match direct_profiles(&browser_root) {
                        Ok(profiles) => profiles,
                        Err(ProfileRead::Missing) => continue,
                        Err(ProfileRead::Unsafe(detail)) => {
                            units.push(OwnerUnitObservation::blocked(
                                unit_key(layout.key, None),
                                browser_root,
                                0,
                                0,
                                0,
                                detail,
                            ));
                            continue;
                        }
                    };
                    for profile in profiles {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let cache_profile = if layout.key.starts_with("mcp-") {
                            browser_root.join(&profile)
                        } else {
                            environment
                                .user_home()
                                .expect("resolved home")
                                .join("Library/Caches")
                                .join(layout.relative)
                                .join(&profile)
                        };
                        for (leaf, path) in [
                            ("Cache", cache_profile.join("Cache")),
                            ("Code Cache", cache_profile.join("Code Cache")),
                            ("GPUCache", browser_root.join(&profile).join("GPUCache")),
                        ] {
                            if cancellation.is_cancelled() {
                                break;
                            }
                            if let Some(unit) = self.observe_unit(
                                environment,
                                layout,
                                Some(&format!("{profile}/{leaf}")),
                                &browser_root,
                                path,
                                &BrowserOwnerState::Idle,
                            ) {
                                units.push(unit);
                            }
                        }
                    }
                }
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
                    let owner = if environment.platform() == PlatformKind::Macos {
                        // Offline payload units do not share the component store's
                        // browser-wide lock. Check each unit's handles below.
                        BrowserOwnerState::Idle
                    } else {
                        self.owner_state(layout)
                    };
                    for (profile, path) in paths {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let entries = SymlinkGuard::validate_anchored_path(&path, environment)
                            .map_err(|error| error.to_string())
                            .and_then(|()| offline_entries(&path));
                        match entries {
                            Ok(entries) => {
                                let observe = |(entry, payload): &(PathBuf, bool)| {
                                    if cancellation.is_cancelled() {
                                        return None;
                                    }
                                    let relative =
                                        entry.strip_prefix(&path).expect("enumerated child");
                                    let key = format!(
                                        "{profile}/{}",
                                        relative.to_string_lossy().replace('\\', "/")
                                    );
                                    if *payload {
                                        self.observe_unit(
                                            environment,
                                            layout,
                                            Some(&key),
                                            &browser_root,
                                            entry.clone(),
                                            &owner,
                                        )
                                    } else {
                                        let measured = self.measuring.measure(entry);
                                        if measured.allocated_bytes == 0 && measured.complete {
                                            return None;
                                        }
                                        let mut unit = OwnerUnitObservation::advisory(
                                            unit_key(layout.key, Some(&key)),
                                            entry.clone(),
                                            measured.logical_bytes,
                                            measured.allocated_bytes,
                                            measured.entry_count,
                                            "CacheStorage origin metadata stays with the browser",
                                        );
                                        if !measured.complete {
                                            unit.state = OwnerUnitState::Blocked;
                                            unit.detail = measured.detail;
                                        }
                                        Some(unit)
                                    }
                                };
                                // Exact unit probes are independent. Reuse the application-owned
                                // four-worker budget; no process observation survives this read.
                                let observed: Vec<_> = crate::execution_budget::install_shared(
                                    || {
                                        use rayon::prelude::*;
                                        entries.par_iter().map(observe).collect()
                                    },
                                    || entries.iter().map(observe).collect(),
                                );
                                units.extend(observed.into_iter().flatten());
                            }
                            Err(detail) => units.push(OwnerUnitObservation::blocked(
                                unit_key(layout.key, Some(&profile)),
                                path,
                                0,
                                0,
                                0,
                                detail,
                            )),
                        }
                    }
                }
            }
        }
        let mut observation = OwnerStoreObservation::ready(Some(base), units);
        if cancellation.is_cancelled() {
            observation.detail =
                Some("Browser inventory was cancelled; unvisited scopes remain unknown".into());
            observation.inspection_issue = Some(crate::models::ScanGapKind::Cancelled);
        }
        observation
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
        if SymlinkGuard::validate_anchored_path(browser_root, environment).is_err()
            || SymlinkGuard::validate_anchored_path(&path, environment).is_err()
        {
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
        let fingerprint =
            match unit_fingerprint(&path, browser_root, layout.key, environment, self.kind) {
                Ok(fingerprint) => fingerprint,
                Err(detail) => {
                    return Some(OwnerUnitObservation::blocked(
                        unit_key(layout.key, profile),
                        path,
                        measurement.logical_bytes,
                        measurement.allocated_bytes,
                        measurement.entry_count,
                        detail,
                    ))
                }
            };
        let key = format!("{}@{fingerprint}", unit_key(layout.key, profile));
        let file_state;
        let owner = if matches!(owner, BrowserOwnerState::Idle)
            && environment.platform() == PlatformKind::Macos
        {
            file_state = match self
                .process
                .running(&RunningProcessPolicy::none().with_open_files(path.clone()))
            {
                Some(names) if names.is_empty() => BrowserOwnerState::Idle,
                Some(names) => BrowserOwnerState::Running(names),
                None => BrowserOwnerState::Unknown,
            };
            &file_state
        } else {
            owner
        };
        Some(match owner {
            BrowserOwnerState::Idle => OwnerUnitObservation::ready(
                key,
                path,
                measurement.logical_bytes,
                measurement.allocated_bytes,
                measurement.entry_count,
            ),
            BrowserOwnerState::Running(running) => OwnerUnitObservation::in_use(
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
                "Cache use could not be verified; scan again",
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
            deletion_disposition: self.deletion_disposition(),
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
        let key = unit.unit_key.split('@').next()?;
        let browser_key = key.split('/').next()?;
        let layout = Self::layouts(environment)
            .iter()
            .find(|layout| layout.key == browser_key)?;
        let root = Self::root(environment, layout)?;
        let expected = match self.kind {
            BrowserCacheKind::RendererCaches => {
                let mut parts = key.split('/').skip(1);
                let profile = parts.next()?;
                let leaf = parts.next()?;
                if !profile_name(profile)
                    || parts.next().is_some()
                    || !matches!(leaf, "Cache" | "Code Cache" | "GPUCache")
                {
                    return None;
                }
                if leaf == "GPUCache" || layout.key.starts_with("mcp-") {
                    root.join(profile).join(leaf)
                } else {
                    environment
                        .user_home()?
                        .join("Library/Caches")
                        .join(layout.relative)
                        .join(profile)
                        .join(leaf)
                }
            }
            BrowserCacheKind::ComponentDownloads => root.join("component_crx_cache"),
            BrowserCacheKind::OfflineCacheStorage => {
                let mut parts = key.split('/').skip(1);
                let profile = parts.next()?;
                let origin = parts.next()?;
                let cache = parts.next()?;
                if !profile_name(profile)
                    || parts.next().is_some()
                    || !ordinary_component(origin)
                    || !ordinary_component(cache)
                {
                    return None;
                }
                root.join(profile)
                    .join("Service Worker")
                    .join("CacheStorage")
                    .join(origin)
                    .join(cache)
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
        // Recheck after enumeration and identity validation, immediately before
        // the mutation. A handle opened since the preview revokes permission.
        if environment.platform() == PlatformKind::Macos
            && !matches!(self.process.running(
                &RunningProcessPolicy::none().with_open_files(unit.path.clone())
            ), Some(names) if names.is_empty())
        {
            return refuse(
                "This cache is in use or its use could not be verified; scan again".into(),
            );
        }
        let browser_key = unit
            .unit_key
            .split('/')
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("");
        let Some(layout) = Self::layouts(environment)
            .iter()
            .find(|layout| layout.key == browser_key)
        else {
            return refuse("Browser owner contract disappeared".into());
        };
        if self.kind == BrowserCacheKind::ComponentDownloads
            && !matches!(self.owner_state(layout), BrowserOwnerState::Idle)
        {
            return refuse("The browser restarted or its state is unknown; scan again".into());
        }
        if SymlinkGuard::validate_anchored_path(&unit.root, environment).is_err()
            || SymlinkGuard::validate_anchored_path(&unit.path, environment).is_err()
            || ToctouGuard::verify(&unit.path, &unit.identity).is_err()
        {
            return refuse("Browser root or unit changed after the final use check".into());
        }
        match unit_fingerprint(&unit.path, &unit.root, layout.key, environment, self.kind) {
            Ok(fingerprint)
                if unit
                    .unit_key
                    .rsplit_once('@')
                    .is_some_and(|(_, reviewed)| reviewed == fingerprint) => {}
            _ => {
                return refuse(
                    "Browser entry layout or runtime changed after the final use check".into(),
                )
            }
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
        if self.kind == BrowserCacheKind::RendererCaches {
            &[PlatformKind::Macos]
        } else {
            &[PlatformKind::Macos, PlatformKind::Windows]
        }
    }

    fn consequence(&self) -> &'static str {
        self.kind.consequence()
    }

    fn requires_confirmation(&self) -> bool {
        self.kind.needs_confirmation()
    }

    fn deletion_disposition(&self) -> neati_core::domain::cleanup::DeletionDisposition {
        neati_core::domain::cleanup::DeletionDisposition::Trash
    }

    fn unit_label(&self, unit: &OwnerUnitObservation) -> String {
        let key = unit.unit_key.split('@').next().unwrap_or(&unit.unit_key);
        let (browser_key, profile) = key
            .split_once('/')
            .map_or((key, None), |(browser, profile)| (browser, Some(profile)));
        let browser = MAC_LAYOUTS
            .iter()
            .chain(WINDOWS_LAYOUTS.iter())
            .find(|layout| layout.key == browser_key)
            .map_or(browser_key, |layout| layout.label);
        match (self.kind, profile) {
            (BrowserCacheKind::RendererCaches, _) => format!(
                "{browser} {}",
                unit.path.file_name().unwrap_or_default().to_string_lossy()
            ),
            (BrowserCacheKind::ComponentDownloads, _) => format!("{browser} component downloads"),
            (BrowserCacheKind::OfflineCacheStorage, Some(profile)) => {
                format!(
                    "{browser} {} offline cache · {}",
                    profile.split('/').next().unwrap_or(profile),
                    unit.path.file_name().unwrap_or_default().to_string_lossy()
                )
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

    fn scan_with_spans(
        &self,
        environment: &PlatformEnvironment,
        _guard: &RunningProcessPolicy,
        _source_id: &str,
        cancellation: &dyn crate::models::CancellationProbe,
        _spans: &mut Vec<neati_core::domain::scan::ScanSpan>,
    ) -> OwnerStoreObservation {
        self.read_store_with_cancellation(environment, cancellation)
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
        "chrome" | "chrome-canary" | "chrome-beta" | "chrome-dev" => &[
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
        key if key.starts_with("mcp-") => &[
            "Google Chrome",
            "Google Chrome for Testing",
            "Google Chrome Helper",
            "chrome",
            "node",
            "chrome-devtools-mcp",
        ],
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
    for (index, entry) in entries.enumerate() {
        if index >= MAX_STORE_ENTRIES {
            return Err(ProfileRead::Unsafe(
                "Browser profile listing exceeds its entry limit".into(),
            ));
        }
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

fn ordinary_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

/// The public Arc 1.166.0 bundle establishes the macOS root contract. Only
/// that reviewed installed identity enables its actionable layout; legacy
/// spellings and another bundle/version do not inherit it.
fn arc_contract_files(environment: &PlatformEnvironment) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    if let Some(root) = environment.program_files() {
        roots.push(root);
    }
    if let Some(home) = environment.user_home() {
        roots.push(home.join("Applications"));
    }
    let mut proofs = Vec::new();
    for root in roots {
        let app = root.join("Arc.app");
        if fs::symlink_metadata(&app)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let files = [
            app.join("Contents/Info.plist"),
            app.join("Contents/Frameworks/ArcCore.framework/Versions/A/Resources/Info.plist"),
        ];
        let mut values = Vec::new();
        for path in &files {
            SymlinkGuard::validate_anchored_path(path, environment).map_err(|e| e.to_string())?;
            let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            if !metadata.is_file() || metadata.len() > 262_144 {
                return Err("Arc bundle metadata is unsafe or exceeds its limit".into());
            }
            values
                .push(plist::Value::from_file(path).map_err(|_| "Arc bundle metadata is invalid")?);
        }
        let value = |index: usize, name: &str| {
            values[index]
                .as_dictionary()
                .and_then(|dictionary| dictionary.get(name))
                .and_then(plist::Value::as_string)
        };
        if value(0, "CFBundleName") != Some("Arc")
            || value(0, "CFBundleIdentifier") != Some("company.thebrowser.Browser")
            || value(0, "CFBundleShortVersionString") != Some("1.166.0")
            || value(0, "CFBundleVersion") != Some("87668")
            || value(0, "CFBundleExecutable") != Some("Arc")
            || value(1, "CFBundleIdentifier") != Some("company.thebrowser.browser.framework")
            || value(1, "CFBundleShortVersionString") != Some("154.0.8037.58")
        {
            return Err("Arc's installed bundle/version does not match the verified 1.166.0 / ArcCore154 root contract; cache observations remain unavailable".into());
        }
        let executable = app.join("Contents/MacOS/Arc");
        SymlinkGuard::validate_anchored_path(&executable, environment)
            .map_err(|e| e.to_string())?;
        if !fs::symlink_metadata(&executable).is_ok_and(|metadata| metadata.is_file()) {
            return Err("Arc bundle executable identity is unavailable".into());
        }
        proofs.push(
            files
                .into_iter()
                .chain(std::iter::once(executable))
                .collect::<Vec<_>>(),
        );
    }
    if proofs.len() != 1 {
        return Err("Exactly one verified Arc bundle is required in the platform-stated application roots; no installed or ambiguous identity grants cleanup".into());
    }
    Ok(proofs.remove(0))
}

fn unit_fingerprint(
    path: &Path,
    root: &Path,
    browser: &str,
    environment: &PlatformEnvironment,
    kind: BrowserCacheKind,
) -> Result<String, String> {
    let mut digest = Sha256::new();
    let root_identity = ToctouGuard::capture(root).ok_or("Browser root identity is unavailable")?;
    digest.update(format!("{root_identity:?}"));
    if browser == "arc" && environment.platform() == PlatformKind::Macos {
        for file in arc_contract_files(environment)? {
            digest.update(file.as_os_str().as_encoded_bytes());
            digest.update(format!(
                "{:?}",
                ToctouGuard::capture(&file).ok_or("Arc runtime identity is unavailable")?
            ));
            if file
                .extension()
                .is_some_and(|extension| extension == "plist")
            {
                digest.update(fs::read(file).map_err(|e| e.to_string())?);
            }
        }
    }
    let started = std::time::Instant::now();
    let mut pending = vec![(path.to_path_buf(), 0usize)];
    let mut count = 0usize;
    while let Some((entry, depth)) = pending.pop() {
        count += 1;
        if count > MAX_STORE_ENTRIES
            || depth > MAX_STORE_DEPTH
            || started.elapsed() > std::time::Duration::from_secs(10)
        {
            return Err("Browser identity inventory exceeded its bounded budget".into());
        }
        let metadata = fs::symlink_metadata(&entry).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err("Browser unit contains a link or unsupported filesystem entry".into());
        }
        #[cfg(unix)]
        if metadata.is_file() {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err("Browser unit contains a shared hard link".into());
            }
        }
        if kind == BrowserCacheKind::RendererCaches {
            let name = entry
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Browser cache entry name is invalid")?;
            let lower = name.to_ascii_lowercase();
            let executable = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    metadata.is_file() && metadata.mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    false
                }
            };
            let facts = crate::models::PathFacts::new(
                name,
                if metadata.is_dir() {
                    crate::models::EntryKind::Directory
                } else {
                    crate::models::EntryKind::File
                },
            )
            .executable(executable);
            if crate::models::classify_structured_state(facts).is_some()
                || (metadata.is_file()
                    && matches!(
                        entry
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .map(str::to_ascii_lowercase)
                            .as_deref(),
                        Some(
                            "js" | "mjs"
                                | "cjs"
                                | "ts"
                                | "tsx"
                                | "jsx"
                                | "py"
                                | "rb"
                                | "sh"
                                | "wasm"
                                | "node"
                                | "dylib"
                        )
                    ))
                || matches!(
                    lower.as_str(),
                    "cookies"
                        | "history"
                        | "login data"
                        | "web data"
                        | "local state"
                        | "preferences"
                        | "secure preferences"
                        | "bookmarks"
                        | "sessions"
                        | "session storage"
                        | "indexeddb"
                        | "local storage"
                        | "extensions"
                        | "extension state"
                        | "scriptcache"
                        | "service worker"
                        | "optguideondevicemodel"
                        | "optimizationguidepredictionmodels"
                )
            {
                return Err("Named renderer cache contains protected profile state, a database, configuration, credential, model or executable".into());
            }
        }
        digest.update(entry.as_os_str().as_encoded_bytes());
        digest.update(format!(
            "{:?}",
            ToctouGuard::capture(&entry).ok_or("Browser entry identity is unavailable")?
        ));
        if metadata.is_dir() {
            let mut children = Vec::new();
            for child in fs::read_dir(entry).map_err(|e| e.to_string())? {
                if children.len() + pending.len() + count >= MAX_STORE_ENTRIES {
                    return Err("Browser identity inventory exceeded its entry limit".into());
                }
                children.push(child.map_err(|e| e.to_string())?.path());
            }
            children.sort();
            pending.extend(children.into_iter().map(|child| (child, depth + 1)));
        }
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Disjoint observations: only directories exactly two levels below the store
/// are removable. Every other entry is retained and measured once.
fn offline_entries(root: &Path) -> Result<Vec<(PathBuf, bool)>, String> {
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut entries = Vec::new();
    let mut count = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("CacheStorage contains an unsafe directory".into());
        }
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            count += 1;
            if count > MAX_STORE_ENTRIES {
                return Err("CacheStorage exceeds its enumeration limit".into());
            }
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("CacheStorage contains a symbolic link".into());
            }
            if metadata.is_dir() && depth == 0 {
                pending.push((entry.path(), 1));
            } else {
                entries.push((entry.path(), metadata.is_dir() && depth == 1));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

pub(super) fn inspect_store(root: &Path) -> Result<(), String> {
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

    fn arc_bundle(environment: &PlatformEnvironment) -> PathBuf {
        let app = environment
            .user_home()
            .unwrap()
            .join("Applications/Arc.app");
        let fields = [
            (
                "Contents/Info.plist",
                vec![
                    ("CFBundleName", "Arc"),
                    ("CFBundleIdentifier", "company.thebrowser.Browser"),
                    ("CFBundleShortVersionString", "1.166.0"),
                    ("CFBundleVersion", "87668"),
                    ("CFBundleExecutable", "Arc"),
                ],
            ),
            (
                "Contents/Frameworks/ArcCore.framework/Versions/A/Resources/Info.plist",
                vec![
                    ("CFBundleIdentifier", "company.thebrowser.browser.framework"),
                    ("CFBundleShortVersionString", "154.0.8037.58"),
                ],
            ),
        ];
        for (name, values) in fields {
            let path = app.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let dictionary = values
                .into_iter()
                .map(|(name, value)| (name.to_string(), plist::Value::String(value.to_string())))
                .collect();
            plist::Value::Dictionary(dictionary)
                .to_file_xml(path)
                .unwrap();
        }
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(
            app.join("Contents/MacOS/Arc"),
            b"fixture metadata identity; never executed",
        )
        .unwrap();
        app
    }

    fn isolated_mac_fixture() -> (tempfile::TempDir, PlatformEnvironment, RunningProcessPolicy) {
        let temp = tempfile::tempdir().unwrap();
        let paths = neati_platform::paths::SimulatedPaths::new()
            .with_flavor(neati_platform::PathFlavor::current())
            .with_home(temp.path())
            .with_program_files(temp.path().join("system-applications"));
        // The catalog is macOS, but these filesystem fixtures run on each host.
        // Its temporary paths must retain the host's drive/separator semantics.
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_platform(PlatformKind::Macos)
            .with_roots(Arc::new(paths));
        (temp, environment, RunningProcessPolicy::none())
    }

    #[test]
    fn arc_public_contract_enables_only_user_data_and_refuses_unknown_bundle_versions() {
        let (_temp, environment, guard) = isolated_mac_fixture();
        let app = arc_bundle(&environment);
        let base = ChromiumCacheProvider::base(&environment).unwrap();
        for root in ["Arc/User Data", "Arc", "company.thebrowser.Browser"] {
            let path = base
                .join(root)
                .join("Default/Service Worker/CacheStorage/origin/unit");
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("payload"), vec![1; 4096]).unwrap();
        }
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Idle),
            trash.clone(),
        );
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.units.len(), 1);
        assert!(scan.units[0].path.starts_with(base.join("Arc/User Data")));
        assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&scan.units[0])])
            .unwrap();
        let info = app.join("Contents/Info.plist");
        let mut value = plist::Value::from_file(&info).unwrap();
        value.as_dictionary_mut().unwrap().insert(
            "CFBundleShortVersionString".into(),
            plist::Value::String("1.167.0".into()),
        );
        value.to_file_xml(info).unwrap();
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Blocked
        );
        assert!(trash.moved.lock().unwrap().is_empty());
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.units.len(), 1);
        assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
        assert!(scan.units[0].allocated_bytes > 0);
        for root in ["Arc", "company.thebrowser.Browser"] {
            assert!(base
                .join(root)
                .join("Default/Service Worker/CacheStorage/origin/unit/payload")
                .exists());
        }
    }

    #[test]
    fn renderer_matrix_keeps_profiles_channels_and_all_adjacent_state_separate() {
        let (_temp, environment, guard) = isolated_mac_fixture();
        arc_bundle(&environment);
        let home = environment.user_home().unwrap();
        let arc = home.join("Library/Application Support/Arc/User Data");
        let mcp = home.join(".cache/chrome-devtools-mcp-cli/chrome-profile-beta");
        let beta = home.join("Library/Application Support/Google/Chrome Beta");
        for root in [&arc, &mcp, &beta] {
            for profile in ["Default", "Profile 2"] {
                fs::create_dir_all(root.join(profile)).unwrap();
                for state in [
                    "Cookies",
                    "History",
                    "Login Data",
                    "Preferences",
                    "IndexedDB/db",
                    "Service Worker/ScriptCache/script",
                    "Service Worker/Database/db",
                    "Extensions/installed",
                    "OptGuideOnDeviceModel/model",
                ] {
                    let path = root.join(profile).join(state);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, b"protected").unwrap();
                }
            }
        }
        let paths = [
            home.join("Library/Caches/Arc/User Data/Default/Cache"),
            arc.join("Profile 2/GPUCache"),
            mcp.join("Default/Cache"),
            mcp.join("Profile 2/Code Cache"),
            home.join("Library/Caches/Google/Chrome Beta/Default/Cache"),
        ];
        for path in &paths {
            fs::create_dir_all(path).unwrap();
            fs::write(path.join("data_0"), vec![1; 4096]).unwrap();
        }
        let unknown = mcp.join("Custom/Cache");
        fs::create_dir_all(&unknown).unwrap();
        fs::write(unknown.join("data_0"), vec![1; 4096]).unwrap();
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::RendererCaches,
            Arc::new(Idle),
            trash.clone(),
        );
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.units.len(), 5);
        assert!(scan
            .units
            .iter()
            .all(|unit| unit.state == OwnerUnitState::Ready));
        let plan = provider
            .prepare(
                &environment,
                &guard,
                &scan.units.iter().map(selection).collect::<Vec<_>>(),
            )
            .unwrap();
        let outcome = provider.execute(&environment, &plan);
        assert!(outcome
            .units
            .iter()
            .all(|unit| unit.status == ProviderStatus::Cleaned));
        assert_eq!(trash.moved.lock().unwrap().len(), 5);
        assert!(unknown.join("data_0").exists());
        for root in [&arc, &mcp, &beta] {
            for profile in ["Default", "Profile 2"] {
                for state in [
                    "Cookies",
                    "History",
                    "Login Data",
                    "Preferences",
                    "IndexedDB/db",
                    "Service Worker/ScriptCache/script",
                    "Service Worker/Database/db",
                    "Extensions/installed",
                    "OptGuideOnDeviceModel/model",
                ] {
                    assert_eq!(
                        fs::read(root.join(profile).join(state)).unwrap(),
                        b"protected"
                    );
                }
            }
        }
    }

    #[test]
    fn final_owner_boundary_renderer_state_and_forged_advisory_selection_never_reach_trash() {
        let (_temp, environment, guard) = isolated_mac_fixture();
        let home = environment.user_home().unwrap();
        let cache = home.join(".cache/chrome-devtools-mcp/chrome-profile/Default/Cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("data_0"), vec![1; 4096]).unwrap();
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::RendererCaches,
            Arc::new(Idle),
            trash.clone(),
        );
        for name in [
            "Cookies",
            "History",
            "credentials.json",
            "index.db",
            "index.db-wal",
            "index.db-shm",
            "settings.json",
            "Model.app/payload",
            "OptGuideOnDeviceModel/model",
            "worker.js",
            "Worker.JS",
            "runtime.wasm",
        ] {
            let path = cache.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![2; 4096]).unwrap();
            let scan = provider.scan(&environment, &guard);
            assert_eq!(scan.units[0].state, OwnerUnitState::Blocked, "{name}");
            assert!(scan.units[0].allocated_bytes > 0);
            assert!(provider
                .prepare(&environment, &guard, &[selection(&scan.units[0])])
                .is_err());
            if path.parent() != Some(cache.as_path()) {
                fs::remove_dir_all(path.parent().unwrap()).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }
        let scan = provider.scan(&environment, &guard);
        let mut forged = selection(&scan.units[0]);
        forged.item_id = "manual-observation".into();
        assert!(provider.prepare(&environment, &guard, &[forged]).is_err());
        assert!(trash.moved.lock().unwrap().is_empty());
        assert!(cache.join("data_0").exists());
    }

    #[cfg(unix)]
    #[test]
    fn browser_final_callback_ancestor_and_equal_size_entry_replacements_are_refused() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct Replace {
            root: PathBuf,
            ancestor: bool,
            armed: AtomicBool,
            calls: AtomicUsize,
        }
        impl RunningProcessProbe for Replace {
            fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
                if self.armed.load(Ordering::SeqCst)
                    && guard.open_file_path().is_some()
                    && self.calls.fetch_add(1, Ordering::SeqCst) == 1
                {
                    if self.ancestor {
                        fs::rename(&self.root, self.root.with_extension("reviewed")).unwrap();
                        std::os::unix::fs::symlink(self.root.with_extension("outside"), &self.root)
                            .unwrap();
                    } else {
                        let unit = guard.open_file_path().unwrap();
                        let file = unit.join("payload");
                        let bytes = fs::read(&file).unwrap();
                        let modified = fs::metadata(&file).unwrap().modified().unwrap();
                        fs::rename(&file, unit.join("reviewed-payload")).unwrap();
                        fs::write(&file, bytes).unwrap();
                        fs::File::options()
                            .write(true)
                            .open(file)
                            .unwrap()
                            .set_modified(modified)
                            .unwrap();
                    }
                }
                Some(vec![])
            }
        }
        for ancestor in [false, true] {
            let (_temp, environment, guard) = isolated_mac_fixture();
            let root = ChromiumCacheProvider::base(&environment)
                .unwrap()
                .join("Google/Chrome");
            let unit = root.join("Default/Service Worker/CacheStorage/origin/unit");
            fs::create_dir_all(&unit).unwrap();
            fs::write(unit.join("payload"), vec![1; 4096]).unwrap();
            let outside = root
                .with_extension("outside")
                .join("Default/Service Worker/CacheStorage/origin/unit");
            fs::create_dir_all(&outside).unwrap();
            fs::write(outside.join("private"), b"keep").unwrap();
            let probe = Arc::new(Replace {
                root: root.clone(),
                ancestor,
                armed: AtomicBool::new(false),
                calls: AtomicUsize::new(0),
            });
            let trash = Arc::new(FixtureTrash::default());
            let provider = provider(
                BrowserCacheKind::OfflineCacheStorage,
                probe.clone(),
                trash.clone(),
            );
            let scan = provider.scan(&environment, &guard);
            let plan = provider
                .prepare(&environment, &guard, &[selection(&scan.units[0])])
                .unwrap();
            probe.armed.store(true, Ordering::SeqCst);
            let result = provider.execute(&environment, &plan);
            assert_eq!(result.units[0].status, ProviderStatus::Blocked);
            assert_eq!(result.reclaimed_bytes(), 0);
            assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
            assert!(trash.moved.lock().unwrap().is_empty());
            assert_eq!(fs::read(outside.join("private")).unwrap(), b"keep");
        }
    }

    #[cfg(unix)]
    #[test]
    fn browser_hardlinks_and_stale_same_byte_selections_are_refused() {
        let (temp, environment, guard) = isolated_mac_fixture();
        let unit = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/unit");
        fs::create_dir_all(&unit).unwrap();
        let file = unit.join("payload");
        fs::write(&file, vec![1; 4096]).unwrap();
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Idle),
            trash.clone(),
        );
        let scan = provider.scan(&environment, &guard);
        let selected = selection(&scan.units[0]);
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        fs::rename(&file, temp.path().join("old-payload")).unwrap();
        fs::write(&file, vec![1; 4096]).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(provider.prepare(&environment, &guard, &[selected]).is_err());
        fs::hard_link(&file, temp.path().join("shared")).unwrap();
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
        assert!(scan.units[0].allocated_bytes > 0);
        assert!(trash.moved.lock().unwrap().is_empty());
    }

    #[test]
    fn cancelled_browser_inventory_starts_no_use_probes_and_qualifies_unknown_scope() {
        struct Cancelled;
        impl crate::models::CancellationProbe for Cancelled {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        struct NoProbe;
        impl RunningProcessProbe for NoProbe {
            fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
                panic!("cancelled inventory must not launch a process/handle probe")
            }
        }
        let (_temp, environment, _guard) = isolated_mac_fixture();
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(NoProbe),
            Arc::new(FixtureTrash::default()),
        );
        let result = provider.read_store_with_cancellation(&environment, &Cancelled);
        assert!(result.units.is_empty());
        assert_eq!(
            result.inspection_issue,
            Some(crate::models::ScanGapKind::Cancelled)
        );
        assert!(result.detail.unwrap().contains("unknown"));
    }

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
    fn offline_probe_fanout_is_bounded_and_keeps_each_unit_verdict() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        #[derive(Default)]
        struct Probe {
            active: AtomicUsize,
            peak: AtomicUsize,
            calls: AtomicUsize,
        }
        impl RunningProcessProbe for Probe {
            fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
                let path = guard.open_file_path().expect("exact unit probe");
                self.calls.fetch_add(1, Ordering::SeqCst);
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(5));
                self.active.fetch_sub(1, Ordering::SeqCst);
                match path.file_name().unwrap().to_str().unwrap() {
                    "unit-0" => None,
                    "unit-1" => Some(vec!["fixture reader".into()]),
                    _ => Some(vec![]),
                }
            }
        }
        let (_temp, environment, guard) = mac_fixture();
        let root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default/Service Worker/CacheStorage/origin");
        for index in 0..12 {
            let path = root.join(format!("unit-{index}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("payload"), vec![1; 4096]).unwrap();
        }
        let probe = Arc::new(Probe::default());
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            probe.clone(),
            Arc::new(FixtureTrash::default()),
        );
        let start = std::time::Instant::now();
        let observed = provider.scan(&environment, &guard);
        assert_eq!(observed.units.len(), 12);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 12);
        assert!((1..=4).contains(&probe.peak.load(Ordering::SeqCst)));
        assert_eq!(
            observed
                .units
                .iter()
                .filter(|unit| unit.state == OwnerUnitState::Ready)
                .count(),
            10
        );
        assert_eq!(
            observed
                .units
                .iter()
                .find(|unit| unit.path.ends_with("unit-1"))
                .unwrap()
                .state,
            OwnerUnitState::InUse
        );
        assert_ne!(
            observed
                .units
                .iter()
                .find(|unit| unit.path.ends_with("unit-0"))
                .unwrap()
                .state,
            OwnerUnitState::Ready
        );
        let parallel_ms = start.elapsed().as_millis();
        let serial_started = std::time::Instant::now();
        let layout = MAC_LAYOUTS
            .iter()
            .find(|layout| layout.key == "chrome")
            .unwrap();
        let browser_root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join(layout.relative);
        let serial: Vec<_> = observed
            .units
            .iter()
            .map(|unit| {
                provider
                    .observe_unit(
                        &environment,
                        layout,
                        Some("fixture"),
                        &browser_root,
                        unit.path.clone(),
                        &BrowserOwnerState::Idle,
                    )
                    .unwrap()
            })
            .collect();
        for (parallel, serial) in observed.units.iter().zip(&serial) {
            assert_eq!(parallel.path, serial.path);
            assert_eq!(parallel.state, serial.state);
            assert_eq!(parallel.allocated_bytes, serial.allocated_bytes);
        }
        println!(
            "12 exact-unit probes: parallel {} ms, serial {} ms, peak {}",
            parallel_ms,
            serial_started.elapsed().as_millis(),
            probe.peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn windows_layouts_resolve_from_the_stated_local_app_data() {
        let local = PathBuf::from(r"D:\Users\tester\AppData\Local");
        let roots = neati_platform::paths::SimulatedPaths::new()
            .with_flavor(neati_platform::PathFlavor::Windows)
            .with_local_app_data(local.clone());
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::Windows)
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
            item_id: format!("browser-item.{}", unit.unit_key),
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
        let cache = profile.join("Service Worker/CacheStorage/origin/cache");
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
    fn offline_units_preserve_origin_indexes_and_unselected_cache_siblings() {
        let (_temp, environment, guard) = mac_fixture();
        let root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default/Service Worker/CacheStorage");
        for relative in [
            "origin/first/payload",
            "origin/second/payload",
            "origin/index.txt",
            "index.txt",
        ] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, vec![1; 4096]).unwrap();
        }
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Idle),
            Arc::new(FixtureTrash::default()),
        );
        let observed = provider.scan(&environment, &guard);
        assert_eq!(observed.units.len(), 4);
        assert_eq!(
            observed
                .units
                .iter()
                .filter(|unit| unit.state == OwnerUnitState::Advisory)
                .count(),
            2
        );
        let selected = observed
            .units
            .iter()
            .find(|unit| unit.path.ends_with("origin/first"))
            .unwrap();
        let plan = provider
            .prepare(&environment, &guard, &[selection(selected)])
            .unwrap();
        assert_eq!(
            plan.deletion_disposition,
            neati_core::domain::cleanup::DeletionDisposition::Trash
        );
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Cleaned
        );
        assert!(!root.join("origin/first").exists());
        for retained in ["origin/second/payload", "origin/index.txt", "index.txt"] {
            assert!(root.join(retained).exists());
        }
    }

    #[test]
    fn running_browser_allows_idle_offline_units_but_not_component_stores() {
        struct BrowserOnly;
        impl RunningProcessProbe for BrowserOnly {
            fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
                Some(if guard.open_file_path().is_some() {
                    Vec::new()
                } else {
                    vec!["Google Chrome".into()]
                })
            }
        }
        let (_temp, environment, guard) = mac_fixture();
        let root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome");
        for relative in [
            "Default/Service Worker/CacheStorage/origin/cache",
            "component_crx_cache",
        ] {
            fs::create_dir_all(root.join(relative)).unwrap();
            fs::write(root.join(relative).join("payload"), vec![1; 4096]).unwrap();
        }
        let trash = Arc::new(FixtureTrash::default());
        let offline = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(BrowserOnly),
            trash.clone(),
        );
        let observed = offline.scan(&environment, &guard);
        assert_eq!(observed.units[0].state, OwnerUnitState::Ready);
        let plan = offline
            .prepare(&environment, &guard, &[selection(&observed.units[0])])
            .unwrap();
        assert_eq!(
            offline.execute(&environment, &plan).units[0].status,
            ProviderStatus::Cleaned
        );
        let components = provider(
            BrowserCacheKind::ComponentDownloads,
            Arc::new(BrowserOnly),
            trash,
        );
        assert_eq!(
            components.scan(&environment, &guard).units[0].state,
            OwnerUnitState::InUse
        );
    }

    #[test]
    fn offline_handle_opened_at_final_check_prevents_trash() {
        struct OpensAtMutation(std::sync::atomic::AtomicUsize);
        impl RunningProcessProbe for OpensAtMutation {
            fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
                Some(
                    if guard.open_file_path().is_some()
                        && self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3
                    {
                        vec!["cache reader".into()]
                    } else {
                        Vec::new()
                    },
                )
            }
        }
        let (_temp, environment, guard) = mac_fixture();
        let path = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/cache");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), vec![1; 4096]).unwrap();
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(OpensAtMutation(std::sync::atomic::AtomicUsize::new(0))),
            trash.clone(),
        );
        let observed = provider.scan(&environment, &guard);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&observed.units[0])])
            .unwrap();
        assert_eq!(
            provider.execute(&environment, &plan).units[0].status,
            ProviderStatus::Blocked
        );
        assert!(path.exists());
        assert!(trash.moved.lock().unwrap().is_empty());
    }

    #[test]
    fn unknown_offline_handle_state_never_authorizes_a_unit() {
        let (_temp, environment, guard) = mac_fixture();
        let path = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/cache");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), vec![1; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Unknown),
            Arc::new(FixtureTrash::default()),
        );
        let observed = provider.scan(&environment, &guard);
        assert_eq!(observed.units[0].state, OwnerUnitState::Blocked);
        assert!(provider
            .prepare(&environment, &guard, &[selection(&observed.units[0])])
            .is_err());
        assert!(path.exists());
    }

    #[test]
    fn windows_offline_units_keep_the_browser_exit_requirement() {
        let temp = tempfile::tempdir().unwrap();
        let roots = neati_platform::paths::SimulatedPaths::new()
            .with_flavor(neati_platform::PathFlavor::current())
            .with_local_app_data(temp.path().to_path_buf());
        let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_platform(PlatformKind::Windows)
            .with_roots(Arc::new(roots));
        let path = temp
            .path()
            .join("Google/Chrome/User Data/Default/Service Worker/CacheStorage/origin/cache");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), vec![1; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(Busy),
            Arc::new(FixtureTrash::default()),
        );
        let observed = provider.scan(&environment, &RunningProcessPolicy::none());
        assert_eq!(observed.units[0].state, OwnerUnitState::InUse);
        assert!(path.exists());
    }

    #[test]
    fn open_file_probe_can_refuse_an_otherwise_idle_browser_unit() {
        struct OpenFile;
        impl RunningProcessProbe for OpenFile {
            fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
                Some(if guard.open_file_path().is_some() {
                    vec!["cache reader".into()]
                } else {
                    vec![]
                })
            }
        }
        let (_temp, environment, guard) = mac_fixture();
        let path = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/cache");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), vec![1; 4096]).unwrap();
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            Arc::new(OpenFile),
            Arc::new(FixtureTrash::default()),
        );
        let observed = provider.scan(&environment, &guard);
        assert_eq!(observed.units[0].state, OwnerUnitState::InUse);
        assert!(provider
            .prepare(&environment, &guard, &[selection(&observed.units[0])])
            .is_err());
        assert!(path.exists());
    }

    #[test]
    fn multiple_profiles_are_enumerated_without_recursive_discovery() {
        let (_temp, environment, guard) = mac_fixture();
        let root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome");
        for profile in ["Default", "Profile 2"] {
            let cache = root
                .join(profile)
                .join("Service Worker/CacheStorage/origin/cache");
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
            .any(|unit| unit.unit_key.starts_with("chrome/Default/origin/cache@")));
        assert!(observation
            .units
            .iter()
            .any(|unit| unit.unit_key.starts_with("chrome/Profile 2/origin/cache@")));
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
        assert_eq!(observation.units[0].state, OwnerUnitState::InUse);
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
