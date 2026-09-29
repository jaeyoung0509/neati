//! Native evidence for the core's reviewed cache operations.
use super::OwnerScopedProvider;
use crate::models::*;
use crate::safety::{SymlinkGuard, ToctouGuard};
use neati_core::domain::cleanup::{
    reviewed_cache::{self, ReviewedCacheKind, ReviewedCachePort},
    OwnerProviderAuthorization, OwnerProviderExecution, OwnerUnitMeasurer, RunningProcessProbe,
};
use neati_platform::{PlatformEnvironment, TrashBackend};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

const MAX_ENTRIES: usize = 50_000;
const MAX_DEPTH: usize = 48;
const EDITORS: &[(&str, &[&str])] = &[
    (
        "Code",
        &[
            "Code",
            "Code Helper",
            "Code Helper (Plugin)",
            "Code Helper (Renderer)",
        ],
    ),
    (
        "Code - Insiders",
        &["Code - Insiders", "Code - Insiders Helper"],
    ),
    (
        "Cursor",
        &[
            "Cursor",
            "Cursor Helper",
            "Cursor Helper (Plugin)",
            "Cursor Helper (Renderer)",
        ],
    ),
];
const IDE_OWNERS: &[&str] = &[
    "idea",
    "idea64",
    "pycharm",
    "webstorm",
    "clion",
    "rider",
    "goland",
    "datagrip",
    "rubymine",
    "rustrover",
    "phpstorm",
    "studio",
    "studio64",
];
#[derive(Clone)]
struct Candidate {
    path: PathBuf,
    owners: Vec<String>,
    min_days: u64,
    label: String,
    retained_reason: Option<String>,
}

pub struct ReviewedCacheProvider {
    kind: ReviewedCacheKind,
    process: Arc<dyn RunningProcessProbe>,
    measuring: Arc<dyn OwnerUnitMeasurer>,
    trash: Arc<dyn TrashBackend>,
}
impl ReviewedCacheProvider {
    pub fn new(
        kind: ReviewedCacheKind,
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
    fn port<'a>(&'a self, environment: &'a PlatformEnvironment) -> NativePort<'a> {
        NativePort {
            provider: self,
            environment,
            now: SystemTime::now(),
        }
    }
}
impl OwnerScopedProvider for ReviewedCacheProvider {
    fn id(&self) -> &'static str {
        self.kind.id()
    }
    fn platforms(&self) -> &'static [PlatformKind] {
        &[PlatformKind::Macos]
    }
    fn consequence(&self) -> &'static str {
        self.kind.consequence()
    }
    fn requires_confirmation(&self) -> bool {
        self.kind.requires_confirmation()
    }
    fn deletion_disposition(&self) -> neati_core::domain::cleanup::DeletionDisposition {
        neati_core::domain::cleanup::DeletionDisposition::Trash
    }
    fn scan(&self, env: &PlatformEnvironment, _: &RunningProcessPolicy) -> OwnerStoreObservation {
        self.port(env).inventory(self.kind)
    }
    fn prepare(
        &self,
        env: &PlatformEnvironment,
        _: &RunningProcessPolicy,
        selections: &[OwnerProviderSelection],
    ) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
        reviewed_cache::prepare(self.kind, &self.port(env), selections)
    }
    fn execute(
        &self,
        env: &PlatformEnvironment,
        plan: &OwnerProviderAuthorization,
    ) -> OwnerProviderExecution {
        reviewed_cache::execute(self.kind, &self.port(env), plan)
    }
}
struct NativePort<'a> {
    provider: &'a ReviewedCacheProvider,
    environment: &'a PlatformEnvironment,
    now: SystemTime,
}
impl NativePort<'_> {
    fn candidates(&self, kind: ReviewedCacheKind) -> Result<Vec<Candidate>, String> {
        if self.environment.platform() != PlatformKind::Macos {
            return Err("This owner operation is available on macOS only".into());
        }
        let home = self
            .environment
            .user_home()
            .ok_or("The user home is unavailable")?;
        let mut result = Vec::new();
        let mut budget = MAX_ENTRIES;
        let mut children = |root: &Path| -> Result<Vec<PathBuf>, String> {
            match fs::symlink_metadata(root) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(e.to_string()),
                Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
                    return Err("An owner root is not an ordinary directory".into())
                }
                Ok(_) => {}
            }
            SymlinkGuard::validate_anchored_path(root, self.environment)
                .map_err(|e| e.to_string())?;
            let mut entries = Vec::new();
            for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
                budget = budget
                    .checked_sub(1)
                    .ok_or("Owner discovery exceeded its entry limit")?;
                entries.push(entry.map_err(|e| e.to_string())?.path());
            }
            entries.sort();
            Ok(entries)
        };
        let candidate = |path, owners: &[&str], min_days, label: &str| Candidate {
            path,
            owners: owners.iter().map(|s| (*s).into()).collect(),
            min_days,
            label: label.into(),
            retained_reason: None,
        };
        match kind {
            ReviewedCacheKind::CodexStaging => {
                let root = home.join(".cache/codex-runtimes");
                match fs::symlink_metadata(&root) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
                    Err(e) => return Err(e.to_string()),
                    Ok(_) => {}
                }
                // The installer activates by renaming payload/codex-primary-runtime
                // into this independent sibling, then removes its staging directory.
                let installed = root.join("codex-primary-runtime");
                SymlinkGuard::validate_anchored_path(&installed, self.environment)
                    .map_err(|e| e.to_string())?;
                let manifest =
                    read_owner_manifest(&installed.join("runtime.json"), self.environment)?.ok_or(
                        "An independently activated runtime is required before staging cleanup",
                    )?;
                if manifest.get("bundleFormatVersion").and_then(|v| v.as_u64()) != Some(2)
                    || manifest.get("targetPlatform").and_then(|v| v.as_str()) != Some("darwin")
                    || manifest
                        .get("bundleVersion")
                        .and_then(|v| v.as_str())
                        .is_none_or(|v| v.is_empty())
                {
                    return Err("Runtime installation format is unsupported; staging references cannot be verified".into());
                }
                for path in children(&root)? {
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                    let Some(suffix) = name.strip_prefix("codex-runtime-install-") else {
                        continue;
                    };
                    if suffix.len() != 6 || !suffix.bytes().all(|c| c.is_ascii_alphanumeric()) {
                        continue;
                    }
                    let mut unit = candidate(
                        path.clone(),
                        &[
                            "ChatGPT",
                            "Codex",
                            "Codex (Renderer)",
                            "Codex (Service)",
                            "codex",
                            "node",
                            "python",
                            "python3",
                            "tar",
                            "unzip",
                        ],
                        7,
                        "Runtime installation staging",
                    );
                    let entries = children(&path)?;
                    if !fs::symlink_metadata(path.join("payload"))
                        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
                        || entries.iter().any(|p| {
                            ![
                                "payload",
                                "node-runtime.tar.xz",
                                "node-runtime.tar.gz",
                                "node-runtime.zip",
                            ]
                            .iter()
                            .any(|name| p.file_name().is_some_and(|leaf| leaf == *name))
                        })
                    {
                        unit.retained_reason = Some(
                            "Unrecognized staging layout; its ownership is not established".into(),
                        );
                    }
                    result.push(unit);
                }
            }
            ReviewedCacheKind::GradleMarkers => {
                let root = self
                    .environment
                    .expand_placeholder("${GRADLE_USER_HOME}/notifications")
                    .ok_or("Gradle User Home is ambiguous")?;
                for version in children(&root)? {
                    let name = version.file_name().and_then(|v| v.to_str()).unwrap_or("");
                    if !name.starts_with(|c: char| c.is_ascii_digit())
                        || !name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                    {
                        continue;
                    }
                    let path = version.join("release-features.rendered");
                    if fs::symlink_metadata(&path)
                        .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
                    {
                        result.push(candidate(
                            path,
                            &["gradle", "gradlew"],
                            0,
                            "Gradle release marker",
                        ));
                    }
                }
            }
            ReviewedCacheKind::GoogleUpdaterLogs => {
                let root = home.join("Library/Application Support/Google/GoogleUpdater");
                for name in ["updater.log", "updater.log.old"] {
                    let path = root.join(name);
                    match fs::symlink_metadata(&path) {
                        Ok(m) if m.is_file() && !m.file_type().is_symlink() => {
                            result.push(candidate(
                                path,
                                &["GoogleUpdater", "GoogleUpdater Helper"],
                                0,
                                "Google Updater log",
                            ))
                        }
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                            return Err(e.to_string())
                        }
                        _ => {}
                    }
                }
            }
            ReviewedCacheKind::NodeHeaders => {
                for key in ["npm_config_devdir", "npm_package_config_node_gyp_devdir"] {
                    if self.environment.cache_path_override(key).is_some() {
                        return Err(
                            "Custom node-gyp SDK roots require owner review outside this operation"
                                .into(),
                        );
                    }
                }
                for relative in ["Library/Caches/node-gyp", ".cache/node-gyp", ".node-gyp"] {
                    for path in children(&home.join(relative))? {
                        let version = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                        if numeric_version(version)
                            && fs::symlink_metadata(path.join("installVersion")).is_ok()
                        {
                            result.push(candidate(
                                path,
                                &["node", "bun", "clang", "clang++", "make", "ninja"],
                                0,
                                "Node development headers",
                            ));
                        }
                    }
                }
            }
            ReviewedCacheKind::ElectronArchives => {
                if self
                    .environment
                    .cache_path_override("ELECTRON_CACHE")
                    .is_some()
                    || self
                        .environment
                        .cache_path_override("electron_config_cache")
                        .is_some()
                {
                    return Err(
                        "Custom Electron download caches are outside the default-root operation"
                            .into(),
                    );
                }
                for relative in ["Library/Caches/electron", ".cache/electron"] {
                    let root = home.join(relative);
                    for path in children(&root)? {
                        if fs::symlink_metadata(&path)
                            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
                        {
                            for archive in children(&path)? {
                                if electron_archive(&archive) {
                                    result.push(candidate(
                                        archive,
                                        &["node", "bun"],
                                        0,
                                        "Electron download",
                                    ));
                                }
                            }
                        } else if electron_archive(&path) {
                            result.push(candidate(path, &["node", "bun"], 0, "Electron download"));
                        }
                    }
                }
            }
            ReviewedCacheKind::ShellCompletions => {
                for path in children(&home)? {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if (name == ".zcompdump"
                        || name == ".zcompdump.zwc"
                        || name.starts_with(".zcompdump-"))
                        && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
                    {
                        result.push(candidate(path, &[], 0, "Zsh completion dump"));
                    }
                }
            }
            ReviewedCacheKind::UpdaterStaging => {
                for (relative, owners) in [
                    (
                        "Library/Caches/cursor-updater/pending",
                        &["Cursor", "Cursor Helper", "ShipIt", "installer"][..],
                    ),
                    (
                        "Library/Caches/notion-updater/pending",
                        &["Notion", "Notion Helper", "ShipIt", "installer"][..],
                    ),
                ] {
                    let root = home.join(relative);
                    let entries = children(&root)?;
                    if entries.is_empty() {
                        continue;
                    }
                    let referenced = updater_reference(&root, self.environment)?;
                    for path in entries {
                        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                        if name.ends_with(".zip") || name.ends_with(".dmg") {
                            let mut unit =
                                candidate(path.clone(), owners, 7, "Unreferenced updater archive");
                            unit.retained_reason = referenced
                                .as_ref()
                                .filter(|current| *current == name)
                                .map(|_| {
                                    "This archive is referenced by the updater's current manifest"
                                        .to_string()
                                });
                            result.push(unit);
                        }
                    }
                }
            }
            ReviewedCacheKind::EditorOffline => {
                for (name, owners) in EDITORS {
                    let editor = home.join("Library/Application Support").join(name);
                    let mut roots = vec![editor.join("Service Worker/CacheStorage")];
                    for slot in children(&editor.join("WebStorage"))? {
                        if fs::symlink_metadata(&slot)
                            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
                        {
                            roots.push(slot.join("CacheStorage"));
                        }
                    }
                    for root in roots {
                        for origin in children(&root)? {
                            if fs::symlink_metadata(&origin)
                                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
                            {
                                for unit in children(&origin)? {
                                    if fs::symlink_metadata(&unit).is_ok_and(|m| m.is_dir()) {
                                        result.push(candidate(unit, owners, 0, name));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ReviewedCacheKind::IdeIndexes => {
                let mut versions = children(&home.join("Library/Caches/JetBrains"))?;
                versions.extend(
                    children(&home.join("Library/Caches/Google"))?
                        .into_iter()
                        .filter(|p| {
                            p.file_name()
                                .is_some_and(|s| s.to_string_lossy().starts_with("AndroidStudio"))
                        }),
                );
                for version in versions {
                    if !supported_ide_version(&version) {
                        continue;
                    }
                    if !fs::symlink_metadata(&version)
                        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
                    {
                        continue;
                    }
                    for name in ["index", "caches", "vcs-log"] {
                        let path = version.join(name);
                        if fs::symlink_metadata(&path).is_ok() {
                            result.push(candidate(path, IDE_OWNERS, 0, "IDE indexes"));
                        }
                    }
                }
            }
            ReviewedCacheKind::MessagesPreviews => {
                for relative in [
                    "Library/Messages/StickerCache",
                    "Library/Messages/Caches/Previews/Attachments",
                    "Library/Messages/Caches/Previews/StickerCache",
                ] {
                    let mut pending: Vec<_> = children(&home.join(relative))?
                        .into_iter()
                        .map(|p| (p, 0))
                        .collect();
                    while let Some((path, depth)) = pending.pop() {
                        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                        if metadata.file_type().is_symlink() {
                            continue;
                        }
                        if metadata.is_dir() {
                            if depth >= 4 {
                                return Err(
                                    "Messages preview directory exceeds its supported depth".into(),
                                );
                            }
                            pending.extend(children(&path)?.into_iter().map(|p| (p, depth + 1)));
                        } else if metadata.is_file()
                            && path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
                                ["png", "jpg", "jpeg", "heic", "gif", "webp", "tiff"]
                                    .contains(&v.to_ascii_lowercase().as_str())
                            })
                        {
                            result.push(candidate(
                                path,
                                &["Messages", "imagent"],
                                0,
                                "Messages preview image",
                            ));
                        }
                    }
                }
            }
            ReviewedCacheKind::MailDownloads => {
                for relative in [
                    "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
                    "Library/Mail Downloads",
                ] {
                    for path in children(&home.join(relative))? {
                        result.push(candidate(
                            path,
                            &["Mail", "Mail Web Content", "maild"],
                            30,
                            "Mail attachment copy",
                        ));
                    }
                }
            }
            ReviewedCacheKind::AbandonedDownloads => {
                // Use the platform's actual Downloads location, including redirection.
                let root = self
                    .environment
                    .known_folder(neati_platform::KnownFolder::Downloads)
                    .ok_or("The Downloads folder is unavailable")?;
                for path in children(root)? {
                    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if ["download", "crdownload", "part"].contains(&extension) {
                        result.push(candidate(
                            path,
                            &[
                                "Google Chrome",
                                "Safari",
                                "Firefox",
                                "Brave Browser",
                                "Microsoft Edge",
                                "Arc",
                                "curl",
                                "wget",
                                "aria2c",
                            ],
                            7,
                            "Incomplete download",
                        ));
                    }
                }
            }
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        result.dedup_by(|a, b| a.path == b.path);
        Ok(result)
    }
    fn inspect(&self, candidate: &Candidate) -> Result<SystemTime, String> {
        SymlinkGuard::validate_anchored_path(&candidate.path, self.environment)
            .map_err(|e| e.to_string())?;
        if self.provider.kind == ReviewedCacheKind::NodeHeaders {
            let entries: Vec<_> = fs::read_dir(&candidate.path)
                .map_err(|e| e.to_string())?
                .take(4)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            if entries.len() != 2
                || entries.iter().any(|e| {
                    !["include", "installVersion"]
                        .contains(&e.file_name().to_string_lossy().as_ref())
                })
                || !fs::symlink_metadata(candidate.path.join("include/node/node.h"))
                    .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                return Err("This directory does not match a downloaded Node headers unit".into());
            }
        }
        let mut latest = SystemTime::UNIX_EPOCH;
        let mut stack = vec![(candidate.path.clone(), 0)];
        let mut remaining = MAX_ENTRIES;
        let started = std::time::Instant::now();
        while let Some((path, depth)) = stack.pop() {
            if started.elapsed() > Duration::from_secs(10) {
                return Err("Unit inspection exceeded its time limit".into());
            }
            remaining = remaining
                .checked_sub(1)
                .ok_or("Unit exceeds its entry limit")?;
            if depth > MAX_DEPTH {
                return Err("Unit exceeds its depth limit".into());
            }
            let m = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() && self.provider.kind == ReviewedCacheKind::CodexStaging {
                // A whole staging directory moves without dereferencing internal
                // relative links. Absolute or escaping links fail the contract.
                let destination = fs::read_link(&path).map_err(|e| e.to_string())?;
                if !staging_link_is_internal(&candidate.path, &path, &destination) {
                    return Err("Staging link escapes the reviewed unit".into());
                }
                latest = latest.max(m.modified().map_err(|e| e.to_string())?);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if m.ctime() < 0 {
                        return Err("Link has an invalid change timestamp".into());
                    }
                    latest = latest.max(
                        SystemTime::UNIX_EPOCH
                            + Duration::new(m.ctime() as u64, m.ctime_nsec().max(0) as u32),
                    );
                }
                continue;
            }
            if m.file_type().is_symlink() || (!m.is_dir() && !m.is_file()) {
                return Err("Unit contains a link or unsupported filesystem object".into());
            }
            latest = latest.max(m.modified().map_err(|e| e.to_string())?);
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if m.ctime() < 0 {
                    return Err("Unit has an invalid change timestamp".into());
                }
                latest = latest.max(
                    SystemTime::UNIX_EPOCH
                        + Duration::new(m.ctime() as u64, m.ctime_nsec().max(0) as u32),
                );
            }
            if m.is_dir() {
                for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                    if stack.len() >= remaining {
                        return Err("Unit exceeds its entry limit".into());
                    }
                    stack.push((entry.map_err(|e| e.to_string())?.path(), depth + 1));
                }
            }
        }
        Ok(latest)
    }
    fn observe(&self, candidate: &Candidate) -> OwnerUnitObservation {
        let path = candidate.path.clone();
        let key = path.to_string_lossy().into_owned();
        let latest = match self.inspect(candidate) {
            Ok(latest) => latest,
            Err(detail) => {
                let mut unit = OwnerUnitObservation::ready(key, path, 0, 0, 0);
                unit.state = OwnerUnitState::Blocked;
                unit.detail = Some(detail);
                return unit;
            }
        };
        let measurement = self.provider.measuring.measure(&path);
        let mut unit = OwnerUnitObservation::ready(
            key,
            path.clone(),
            measurement.logical_bytes,
            measurement.allocated_bytes,
            measurement.entry_count,
        );
        if !measurement.complete {
            unit.state = OwnerUnitState::Blocked;
            unit.detail = Some(
                measurement
                    .detail
                    .unwrap_or_else(|| "Measurement is incomplete".into()),
            );
            return unit;
        }
        if let Some(reason) = &candidate.retained_reason {
            unit.state = OwnerUnitState::Advisory;
            unit.detail = Some(reason.clone());
            return unit;
        }
        let mut guard =
            RunningProcessPolicy::guarding(candidate.owners.clone()).with_open_files(path);
        if self.provider.kind == ReviewedCacheKind::GradleMarkers {
            guard = guard.with_gradle_owner();
        }
        match self.provider.process.running(&guard) {
            None => {
                unit.state = OwnerUnitState::Blocked;
                unit.detail = Some("Owner or open-file state is unknown".into());
            }
            Some(names) if !names.is_empty() => {
                unit.state = OwnerUnitState::InUse;
                unit.detail = Some(format!(
                    "Close {} before reviewing {}",
                    names.join(", "),
                    candidate.label
                ));
            }
            Some(_) => {
                if !self
                    .now
                    .duration_since(latest)
                    .is_ok_and(|age| age >= Duration::from_secs(candidate.min_days * 86400))
                {
                    unit.state = OwnerUnitState::Recent;
                    unit.detail = Some(format!(
                        "Retained until every entry has been inactive for {} days",
                        candidate.min_days
                    ));
                }
            }
        }
        unit
    }
}
impl ReviewedCachePort for NativePort<'_> {
    fn inventory(&self, kind: ReviewedCacheKind) -> OwnerStoreObservation {
        match self.candidates(kind) {
            Err(detail) => OwnerStoreObservation::refused(ProviderStatus::Blocked, None, detail),
            Ok(candidates) => {
                let start = std::time::Instant::now();
                let mut units = Vec::new();
                for candidate in candidates {
                    if units.len() >= 512 || start.elapsed() > Duration::from_secs(20) {
                        return OwnerStoreObservation::refused(ProviderStatus::Blocked, None,
                            "Owner observation exceeded its bounded review budget; coverage is incomplete");
                    }
                    let unit = self.observe(&candidate);
                    if unit.allocated_bytes > 0 || unit.state == OwnerUnitState::Blocked {
                        units.push(unit);
                    }
                }
                OwnerStoreObservation::ready(None, units)
            }
        }
    }
    fn identity(&self, path: &Path) -> Option<CleanupIdentity> {
        ToctouGuard::capture(path)
    }
    fn move_verified(
        &self,
        kind: ReviewedCacheKind,
        unit: &OwnerProviderUnit,
    ) -> Result<(), String> {
        let candidates = self.candidates(kind)?;
        let candidate = candidates
            .iter()
            .find(|c| c.path == unit.path)
            .ok_or("Unit is outside the registered owner scope")?;
        let current = self.observe(candidate);
        if current.state != OwnerUnitState::Ready || current.allocated_bytes != unit.expected_bytes
        {
            return Err("Unit is no longer eligible; scan again".into());
        }
        ToctouGuard::verify(&unit.path, &unit.identity).map_err(|e| e.to_string())?;
        // Final use check follows the potentially long tree inspection.
        let mut guard = RunningProcessPolicy::guarding(candidate.owners.clone())
            .with_open_files(unit.path.clone());
        if kind == ReviewedCacheKind::GradleMarkers {
            guard = guard.with_gradle_owner();
        }
        if !matches!(self.provider.process.running(&guard), Some(names) if names.is_empty()) {
            return Err("The owner restarted or a handle opened before the Trash move".into());
        }
        if kind == ReviewedCacheKind::UpdaterStaging {
            let parent = unit.path.parent().ok_or("Updater unit has no parent")?;
            let referenced = updater_reference(parent, self.environment)?;
            if referenced
                .as_deref()
                .is_some_and(|name| unit.path.file_name().is_some_and(|leaf| leaf == name))
            {
                return Err("The updater now references this archive; review again".into());
            }
        }
        ToctouGuard::verify(&unit.path, &unit.identity).map_err(|error| error.to_string())?;
        self.provider.trash.move_to_trash(&unit.path)
    }
    fn is_absent(&self, path: &Path) -> bool {
        fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    }
}

fn staging_link_is_internal(root: &Path, link: &Path, destination: &Path) -> bool {
    if destination.is_absolute() || !link.starts_with(root) {
        return false;
    }
    let Some(parent) = link.parent() else {
        return false;
    };
    let mut target = parent.to_path_buf();
    for component in destination.components() {
        match component {
            std::path::Component::Normal(part) => target.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if target == root || !target.pop() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    target.starts_with(root)
}

fn supported_ide_version(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    [
        "IntelliJIdea",
        "IdeaIC",
        "PyCharm",
        "PyCharmCE",
        "WebStorm",
        "CLion",
        "Rider",
        "GoLand",
        "DataGrip",
        "RubyMine",
        "RustRover",
        "PhpStorm",
        "AndroidStudio",
        "AndroidStudioPreview",
    ]
    .iter()
    .any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|version| {
            version.chars().next().is_some_and(|c| c.is_ascii_digit())
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
    })
}

fn read_owner_manifest(
    path: &Path,
    environment: &PlatformEnvironment,
) -> Result<Option<serde_json::Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) => metadata,
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 65536 {
        return Err("Owner manifest has an unsupported shape".into());
    }
    SymlinkGuard::validate_anchored_path(path, environment).map_err(|e| e.to_string())?;
    let identity = ToctouGuard::capture(path).ok_or("Owner manifest identity is unavailable")?;
    use std::io::Read;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?.take(65537);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Owner manifest exceeded its size limit".into());
    }
    ToctouGuard::verify(path, &identity).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(Some(json))
}

fn updater_reference(
    root: &Path,
    environment: &PlatformEnvironment,
) -> Result<Option<String>, String> {
    let Some(json) = read_owner_manifest(&root.join("update-info.json"), environment)? else {
        return Ok(None);
    };
    let object = json
        .as_object()
        .ok_or("Updater manifest is not an object")?;
    if object.keys().any(|key| {
        ![
            "fileName",
            "sha512",
            "isAdminRightsRequired",
            "installOnNextLaunch",
        ]
        .contains(&key.as_str())
    }) {
        return Err(
            "Updater manifest has unknown fields; its full reference scope is unavailable".into(),
        );
    }
    let name = object
        .get("fileName")
        .and_then(|v| v.as_str())
        .ok_or("Updater manifest has no installer filename")?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err("Updater manifest filename is not a direct child".into());
    }
    Ok(Some(name.into()))
}

fn numeric_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}
fn electron_archive(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.starts_with("electron-v")
        && name.ends_with(".zip")
        && [
            "-darwin-arm64.zip",
            "-darwin-x64.zip",
            "-darwin-universal.zip",
        ]
        .iter()
        .any(|suffix| {
            name.strip_prefix("electron-v")
                .and_then(|tail| tail.strip_suffix(suffix))
                .is_some_and(numeric_version)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::SizeCalculatorMeasurement;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct UseState(Option<Vec<String>>);
    impl RunningProcessProbe for UseState {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            self.0.clone()
        }
    }
    struct Starts(AtomicUsize);
    impl RunningProcessProbe for Starts {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(if self.0.fetch_add(1, Ordering::SeqCst) >= 3 {
                vec!["Restarted owner".into()]
            } else {
                vec![]
            })
        }
    }
    struct FixtureTrash(PathBuf);
    impl TrashBackend for FixtureTrash {
        fn move_to_trash(&self, path: &Path) -> Result<(), String> {
            fs::create_dir_all(&self.0).map_err(|e| e.to_string())?;
            fs::rename(path, self.0.join("moved")).map_err(|e| e.to_string())
        }
    }
    fn fixture(
        kind: ReviewedCacheKind,
        processes: Arc<dyn RunningProcessProbe>,
    ) -> (
        tempfile::TempDir,
        PlatformEnvironment,
        ReviewedCacheProvider,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let root = neati_platform::NativePlatformPaths::normalize_verbatim_path(
            &dir.path().canonicalize().unwrap(),
        );
        let home = root.join("home");
        fs::create_dir(&home).unwrap();
        let env = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
            .with_platform(PlatformKind::Macos)
            .with_home(&home)
            .with_known_folder(
                neati_platform::KnownFolder::Downloads,
                home.join("Downloads"),
            );
        let provider = ReviewedCacheProvider::new(
            kind,
            processes,
            Arc::new(SizeCalculatorMeasurement),
            Arc::new(FixtureTrash(root.join("fixture-trash"))),
        );
        (dir, env, provider)
    }
    fn file(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![42; 8192]).unwrap();
    }
    fn selection(unit: &OwnerUnitObservation) -> OwnerProviderSelection {
        OwnerProviderSelection {
            item_id: "reviewed".into(),
            name: "reviewed".into(),
            path: unit.path.clone(),
            expected_bytes: unit.allocated_bytes,
        }
    }
    #[test]
    fn editor_units_leave_origin_metadata_profile_state_and_other_stores_intact() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::EditorOffline,
            Arc::new(UseState(Some(vec![]))),
        );
        let root = env
            .user_home()
            .unwrap()
            .join("Library/Application Support/Code");
        let unit = root.join("WebStorage/slot/CacheStorage/origin/unit");
        file(&unit.join("payload"));
        for name in [
            "WebStorage/slot/CacheStorage/origin/index.txt",
            "WebStorage/slot/IndexedDB/user.db",
            "User/settings.json",
            "Service Worker/Database/user.db",
        ] {
            file(&root.join(name));
        }
        let inventory = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(inventory.units.len(), 1);
        let plan = provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&inventory.units[0])],
            )
            .unwrap();
        provider.execute(&env, &plan);
        assert!(!unit.exists());
        for name in [
            "WebStorage/slot/CacheStorage/origin/index.txt",
            "WebStorage/slot/IndexedDB/user.db",
            "User/settings.json",
            "Service Worker/Database/user.db",
        ] {
            assert!(root.join(name).exists());
        }
    }
    #[test]
    fn ide_index_scope_preserves_history_settings_and_foreign_directories() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::IdeIndexes,
            Arc::new(UseState(Some(vec![]))),
        );
        let root = env.user_home().unwrap().join("Library/Caches/JetBrains");
        for relative in [
            "IntelliJIdea2026.2/index/data.db",
            "IntelliJIdea2026.2/index/data.db-wal",
            "IntelliJIdea2026.2/index/data.db-shm",
            "IntelliJIdea2026.2/LocalHistory/history.db",
            "OtherPersonalStore/index/keep.db",
        ] {
            file(&root.join(relative));
        }
        let inventory = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(inventory.units.len(), 1);
        let plan = provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&inventory.units[0])],
            )
            .unwrap();
        provider.execute(&env, &plan);
        assert!(!root.join("IntelliJIdea2026.2/index").exists());
        assert!(root
            .join("IntelliJIdea2026.2/LocalHistory/history.db")
            .exists());
        assert!(root.join("OtherPersonalStore/index/keep.db").exists());
    }
    #[test]
    fn active_and_unknown_owners_never_authorize_editor_cleanup() {
        for (process, expected) in [
            (Some(vec!["Code".into()]), OwnerUnitState::InUse),
            (None, OwnerUnitState::Blocked),
        ] {
            let (_dir, env, provider) = fixture(
                ReviewedCacheKind::EditorOffline,
                Arc::new(UseState(process)),
            );
            file(&env.user_home().unwrap().join(
                "Library/Application Support/Code/Service Worker/CacheStorage/origin/unit/data",
            ));
            let inventory = provider.scan(&env, &RunningProcessPolicy::none());
            assert_eq!(inventory.units[0].state, expected);
            assert!(provider
                .prepare(
                    &env,
                    &RunningProcessPolicy::none(),
                    &[selection(&inventory.units[0])]
                )
                .is_err());
        }
    }
    #[test]
    fn owner_restarting_at_last_check_revokes_move() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::EditorOffline,
            Arc::new(Starts(AtomicUsize::new(0))),
        );
        let path = env
            .user_home()
            .unwrap()
            .join("Library/Application Support/Code/Service Worker/CacheStorage/origin/unit/data");
        file(&path);
        let scan = provider.scan(&env, &RunningProcessPolicy::none());
        let plan = provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&scan.units[0])],
            )
            .unwrap();
        provider.execute(&env, &plan);
        assert!(path.exists());
    }
    #[test]
    fn reviewed_downloads_are_age_gated_and_completed_files_are_outside_scope() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::AbandonedDownloads,
            Arc::new(UseState(Some(vec![]))),
        );
        let root = env
            .known_folder(neati_platform::KnownFolder::Downloads)
            .unwrap();
        file(&root.join("partial.zip.crdownload"));
        file(&root.join("complete.zip"));
        let inventory = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(inventory.units.len(), 1);
        assert_eq!(inventory.units[0].state, OwnerUnitState::Recent);
        let port = NativePort {
            provider: &provider,
            environment: &env,
            now: SystemTime::now() + Duration::from_secs(8 * 86400),
        };
        let old = port.inventory(ReviewedCacheKind::AbandonedDownloads);
        assert_eq!(old.units[0].state, OwnerUnitState::Ready);
        let plan = reviewed_cache::prepare(
            ReviewedCacheKind::AbandonedDownloads,
            &port,
            &[selection(&old.units[0])],
        )
        .unwrap();
        reviewed_cache::execute(ReviewedCacheKind::AbandonedDownloads, &port, &plan);
        assert!(!root.join("partial.zip.crdownload").exists());
        assert!(root.join("complete.zip").exists());
    }
    #[test]
    fn updater_keeps_referenced_archives_and_rechecks_changed_manifests() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::UpdaterStaging,
            Arc::new(UseState(Some(vec![]))),
        );
        let root = env
            .user_home()
            .unwrap()
            .join("Library/Caches/cursor-updater/pending");
        file(&root.join("current.zip"));
        file(&root.join("old.zip"));
        fs::write(
            root.join("update-info.json"),
            r#"{"fileName":"current.zip","sha512":"hash"}"#,
        )
        .unwrap();
        let port = NativePort {
            provider: &provider,
            environment: &env,
            now: SystemTime::now() + Duration::from_secs(8 * 86400),
        };
        let scan = port.inventory(ReviewedCacheKind::UpdaterStaging);
        assert_eq!(scan.units.len(), 2);
        assert_eq!(
            scan.units
                .iter()
                .find(|u| u.path.ends_with("current.zip"))
                .unwrap()
                .state,
            OwnerUnitState::Advisory
        );
        let old = scan
            .units
            .iter()
            .find(|u| u.path.ends_with("old.zip"))
            .unwrap();
        let plan =
            reviewed_cache::prepare(ReviewedCacheKind::UpdaterStaging, &port, &[selection(old)])
                .unwrap();
        fs::write(
            root.join("update-info.json"),
            r#"{"fileName":"old.zip","sha512":"hash"}"#,
        )
        .unwrap();
        reviewed_cache::execute(ReviewedCacheKind::UpdaterStaging, &port, &plan);
        assert!(root.join("old.zip").exists());
        assert!(root.join("current.zip").exists());
        fs::write(root.join("update-info.json"), b"not valid JSON").unwrap();
        assert_eq!(
            port.inventory(ReviewedCacheKind::UpdaterStaging).status,
            ProviderStatus::Blocked
        );
    }

    #[test]
    fn mail_plan_never_reaches_message_store_or_forged_selection() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::MailDownloads,
            Arc::new(UseState(Some(vec![]))),
        );
        let home = env.user_home().unwrap();
        let copy = home.join(
            "Library/Containers/com.apple.mail/Data/Library/Mail Downloads/attachment/report.pdf",
        );
        let message = home.join("Library/Mail/V10/message.emlx");
        file(&copy);
        file(&message);
        let port = NativePort {
            provider: &provider,
            environment: &env,
            now: SystemTime::now() + Duration::from_secs(31 * 86400),
        };
        let scan = port.inventory(ReviewedCacheKind::MailDownloads);
        assert_eq!(scan.units.len(), 1);
        let mut forged = selection(&scan.units[0]);
        forged.path = message.clone();
        assert!(
            reviewed_cache::prepare(ReviewedCacheKind::MailDownloads, &port, &[forged]).is_err()
        );
        let plan = reviewed_cache::prepare(
            ReviewedCacheKind::MailDownloads,
            &port,
            &[selection(&scan.units[0])],
        )
        .unwrap();
        reviewed_cache::execute(ReviewedCacheKind::MailDownloads, &port, &plan);
        assert!(!copy.exists());
        assert!(message.exists());
    }
    #[test]
    fn small_cache_adapters_preserve_installed_software_and_settings() {
        for (kind, target, sentinels) in [
            (ReviewedCacheKind::GradleMarkers, ".gradle/notifications/9.0/release-features.rendered", vec![".gradle/notifications/9.0/preferences.json", ".gradle/wrapper/dists/installed"]),
            (ReviewedCacheKind::GoogleUpdaterLogs, "Library/Application Support/Google/GoogleUpdater/updater.log.old", vec!["Library/Application Support/Google/GoogleUpdater/prefs.json", "Library/Application Support/Google/GoogleUpdater/1.2/GoogleUpdater.app/Contents/MacOS/GoogleUpdater"]),
            (
                ReviewedCacheKind::NodeHeaders,
                "Library/Caches/node-gyp/22.1.0",
                vec![
                    ".local/bin/node",
                    "Library/Caches/node-gyp/current/settings.json",
                ],
            ),
            (
                ReviewedCacheKind::ElectronArchives,
                "Library/Caches/electron/hash/electron-v30.1.0-darwin-arm64.zip",
                vec![
                    "Library/Caches/electron/hash/Electron.app/Contents/MacOS/Electron",
                    ".cache/ms-playwright/chromium/chrome",
                ],
            ),
            (
                ReviewedCacheKind::ShellCompletions,
                ".zcompdump-host-5.9",
                vec![".zshrc", ".zsh_history"],
            ),
        ] {
            let (_dir, env, provider) = fixture(kind, Arc::new(UseState(Some(vec![]))));
            let home = env.user_home().unwrap();
            let target = home.join(target);
            if kind == ReviewedCacheKind::NodeHeaders {
                file(&target.join("include/node/node.h"));
                file(&target.join("installVersion"));
            } else {
                file(&target);
            }
            for sentinel in &sentinels {
                file(&home.join(sentinel));
            }
            let scan = provider.scan(&env, &RunningProcessPolicy::none());
            assert_eq!(scan.units.len(), 1);
            assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
            let plan = provider
                .prepare(
                    &env,
                    &RunningProcessPolicy::none(),
                    &[selection(&scan.units[0])],
                )
                .unwrap();
            assert!(!plan.requires_confirmation);
            provider.execute(&env, &plan);
            assert!(!target.exists());
            for sentinel in sentinels {
                assert!(home.join(sentinel).exists());
            }
        }
    }
    #[test]
    fn replaced_unit_and_failed_trash_move_never_report_success() {
        let (_dir, env, mut provider) = fixture(
            ReviewedCacheKind::ShellCompletions,
            Arc::new(UseState(Some(vec![]))),
        );
        let path = env.user_home().unwrap().join(".zcompdump");
        file(&path);
        let scan = provider.scan(&env, &RunningProcessPolicy::none());
        let plan = provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&scan.units[0])],
            )
            .unwrap();
        fs::rename(&path, path.with_extension("old")).unwrap();
        file(&path);
        provider.execute(&env, &plan);
        assert!(path.exists());
        struct Denied;
        impl TrashBackend for Denied {
            fn move_to_trash(&self, _: &Path) -> Result<(), String> {
                Err("Permission denied by fixture".into())
            }
        }
        provider.trash = Arc::new(Denied);
        let fresh = provider.scan(&env, &RunningProcessPolicy::none());
        let plan = provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&fresh.units[0])],
            )
            .unwrap();
        let result = provider.execute(&env, &plan);
        assert!(format!("{:?}", result.units).contains("Permission denied by fixture"));
        assert!(path.exists());
    }
    #[test]
    fn messages_preview_units_preserve_original_attachments_and_databases() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::MessagesPreviews,
            Arc::new(UseState(Some(vec![]))),
        );
        let home = env.user_home().unwrap();
        let preview = home.join("Library/Messages/Caches/Previews/Attachments/slot/thumb.jpg");
        let original = home.join("Library/Messages/Attachments/slot/photo.jpg");
        let database = home.join("Library/Messages/Caches/Previews/Attachments/index.db");
        file(&preview);
        file(&original);
        file(&database);
        let scan = provider.scan(&env, &RunningProcessPolicy::none());
        assert_eq!(scan.units.len(), 1);
        let plan = provider
            .prepare(
                &env,
                &RunningProcessPolicy::none(),
                &[selection(&scan.units[0])],
            )
            .unwrap();
        assert!(plan.requires_confirmation);
        provider.execute(&env, &plan);
        assert!(!preview.exists());
        assert!(original.exists());
        assert!(database.exists());
    }
    #[test]
    fn runtime_staging_requires_an_independent_activated_runtime_and_known_layout() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::CodexStaging,
            Arc::new(UseState(Some(vec![]))),
        );
        let root = env.user_home().unwrap().join(".cache/codex-runtimes");
        let stage = root.join("codex-runtime-install-Ab12Cd");
        file(&stage.join("node-runtime.tar.xz"));
        fs::create_dir(stage.join("payload")).unwrap();
        assert_eq!(
            provider.scan(&env, &RunningProcessPolicy::none()).status,
            ProviderStatus::Blocked
        );
        let installed = root.join("codex-primary-runtime");
        file(&installed.join("dependencies/node/bin/node"));
        fs::write(
            installed.join("runtime.json"),
            r#"{"bundleFormatVersion":2,"targetPlatform":"darwin","bundleVersion":"fixture"}"#,
        )
        .unwrap();
        let port = NativePort {
            provider: &provider,
            environment: &env,
            now: SystemTime::now() + Duration::from_secs(8 * 86400),
        };
        let scan = port.inventory(ReviewedCacheKind::CodexStaging);
        assert_eq!(scan.units.len(), 1);
        assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
        let plan = reviewed_cache::prepare(
            ReviewedCacheKind::CodexStaging,
            &port,
            &[selection(&scan.units[0])],
        )
        .unwrap();
        reviewed_cache::execute(ReviewedCacheKind::CodexStaging, &port, &plan);
        assert!(!stage.exists());
        assert!(installed.join("dependencies/node/bin/node").exists());
        file(&stage.join("unknown-personal-data"));
        assert_eq!(
            port.inventory(ReviewedCacheKind::CodexStaging).units[0].state,
            OwnerUnitState::Advisory
        );
    }
    #[test]
    fn staging_link_scope_rejects_absolute_and_escaping_targets() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("stage");
        let link = root.join("payload/bin/tool");
        assert!(staging_link_is_internal(
            &root,
            &link,
            Path::new("../lib/tool")
        ));
        assert!(!staging_link_is_internal(
            &root,
            &link,
            Path::new("../../../outside")
        ));
        assert!(!staging_link_is_internal(
            &root,
            &link,
            &fixture.path().join("outside")
        ));
        assert!(!staging_link_is_internal(
            &root,
            &fixture.path().join("outside/link"),
            Path::new("file")
        ));
    }
    #[test]
    fn empty_gradle_markers_are_not_reported_as_reclaimable_bytes() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::GradleMarkers,
            Arc::new(UseState(Some(vec![]))),
        );
        let path = env
            .user_home()
            .unwrap()
            .join(".gradle/notifications/9.0/release-features.rendered");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, []).unwrap();
        assert!(provider
            .scan(&env, &RunningProcessPolicy::none())
            .units
            .is_empty());
        assert!(path.exists());
    }
    #[cfg(unix)]
    #[test]
    fn staged_links_move_without_following_and_escape_links_revoke_execution() {
        let (_dir, env, provider) = fixture(
            ReviewedCacheKind::CodexStaging,
            Arc::new(UseState(Some(vec![]))),
        );
        let root = env.user_home().unwrap().join(".cache/codex-runtimes");
        let installed = root.join("codex-primary-runtime");
        file(&installed.join("keep"));
        fs::write(
            installed.join("runtime.json"),
            r#"{"bundleFormatVersion":2,"targetPlatform":"darwin","bundleVersion":"fixture"}"#,
        )
        .unwrap();
        let stage = root.join("codex-runtime-install-Ab12Cd");
        file(&stage.join("payload/runtime/lib/tool"));
        file(&stage.join("node-runtime.tar.xz"));
        fs::create_dir(stage.join("payload/runtime/bin")).unwrap();
        let link = stage.join("payload/runtime/bin/tool");
        std::os::unix::fs::symlink("../lib/tool", &link).unwrap();
        let port = NativePort {
            provider: &provider,
            environment: &env,
            now: SystemTime::now() + Duration::from_secs(8 * 86400),
        };
        let scan = port.inventory(ReviewedCacheKind::CodexStaging);
        assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
        let plan = reviewed_cache::prepare(
            ReviewedCacheKind::CodexStaging,
            &port,
            &[selection(&scan.units[0])],
        )
        .unwrap();
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&installed, &link).unwrap();
        reviewed_cache::execute(ReviewedCacheKind::CodexStaging, &port, &plan);
        assert!(stage.exists());
        assert!(installed.join("keep").exists());
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink("../lib/tool", &link).unwrap();
        let fresh = port.inventory(ReviewedCacheKind::CodexStaging);
        let plan = reviewed_cache::prepare(
            ReviewedCacheKind::CodexStaging,
            &port,
            &[selection(&fresh.units[0])],
        )
        .unwrap();
        reviewed_cache::execute(ReviewedCacheKind::CodexStaging, &port, &plan);
        assert!(!stage.exists());
        assert!(installed.join("keep").exists());
    }
}
