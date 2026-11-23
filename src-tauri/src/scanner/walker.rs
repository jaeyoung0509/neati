use super::observation::{NoRootProgress, ScanLimits, TraversalCounters};
use super::observation::{SignatureScan, WalkContext};
use super::size::{describe_inspection_error, inspection_issue_for_io};
use crate::models::ScanGapKind;
use crate::models::{
    classify_structured_state, derive_cleanup_disposition, AgeObservation, CacheSizeSemantics,
    CleanStrategy, CleanupEligibility, CleanupOwnership, CleanupUnit, DispositionFacts,
    EligibilityGate, EntryKind, FileSize, NeatiError, ObservationQuality, PathFacts, ScanItem,
    Signature, StaleEntryObservation, StructuredStateKind,
};
use crate::safety::SymlinkGuard;
use crate::scanner::{PathMeasurement, SizeCalculator};
use crate::signatures::SignatureLoader;
use neati_platform::PlatformEnvironment;
use rayon::ThreadPool;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub struct DirectoryScanner;

/// Pure path-policy vocabulary reused during one tree observation. Fresh
/// metadata and cancellation checks stay at every recursive entry; this value
/// has no filesystem identity or cleanup authorization in it.
struct TreeObservationPolicy {
    blacklist: crate::safety::blacklist::PreparedBlacklist,
    exclusions: crate::signatures::exclusions::PreparedExclusions,
}

impl TreeObservationPolicy {
    fn new(exclusions: &[String], environment: &PlatformEnvironment) -> Self {
        Self {
            blacklist: crate::safety::Blacklist::prepare(environment),
            exclusions: crate::signatures::exclusions::PreparedExclusions::new(
                exclusions,
                environment,
            ),
        }
    }
}

#[derive(Default)]
struct SignatureTimings {
    root_expansion: Duration,
    policy_preparation: Duration,
    tree_measurement: Duration,
    child_enumeration: Duration,
    metadata_and_links: Duration,
    age_evaluation: Duration,
    size_accounting: Duration,
    use_checks: Duration,
    child_classification: Duration,
    traversal: Duration,
    measured_aged_children: bool,
}

/// Exclusive local work inside the already single-pass age/size traversal.
/// One aggregate per signature is emitted; paths and per-entry events are not.
#[derive(Default)]
struct TreePhaseTimings {
    metadata_and_links: Duration,
    age_evaluation: Duration,
    size_accounting: Duration,
}

impl SignatureTimings {
    fn spans(self, signature: &Signature) -> Vec<neati_core::domain::scan::ScanSpan> {
        let mut phases = vec![
            ("root_expansion", self.root_expansion),
            ("use_checks", self.use_checks),
        ];
        if self.measured_aged_children {
            phases.extend([
                ("aged.policy_preparation", self.policy_preparation),
                ("aged.tree_measurement", self.tree_measurement),
                ("aged.metadata_and_links", self.metadata_and_links),
                ("aged.age_evaluation", self.age_evaluation),
                ("aged.size_accounting", self.size_accounting),
                ("aged.traversal_and_policy", self.traversal),
                ("aged.child_classification", self.child_classification),
                (
                    "aged.enumeration_and_classification",
                    self.child_enumeration,
                ),
            ]);
        }
        phases
            .into_iter()
            .map(|(phase, duration)| neati_core::domain::scan::ScanSpan {
                source_id: format!("{}.{}", signature.id, phase),
                duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            })
            .collect()
    }
}

/// One concrete root a catalog pattern authorizes.
///
/// `key` is the part of the path the pattern did not spell out — the matched
/// application, package, or profile — so two matches of one pattern never share
/// an item identity. A literal pattern produces one root with no key, and its
/// identity is exactly what it has always been.
struct SignatureRoot {
    path: std::path::PathBuf,
    key: Option<String>,
}

mod aged_measurement;

/// One enumerated child of an aged root, in enumeration order: either already
/// decided while classifying (a coverage observation, a link, an unreadable
/// entry) or waiting for its tree measurement.
enum AgedSlot {
    Ready(Box<ScanItem>),
    Measure(AgedChild),
}

impl AgedSlot {
    fn ready(item: ScanItem) -> Self {
        Self::Ready(Box::new(item))
    }
}

/// A classified child whose tree is measured before its item is built.
struct AgedChild {
    path: std::path::PathBuf,
    name: String,
    facts: CandidateFacts,
    path_text: String,
    unit: CleanupUnit,
    ownership: CleanupOwnership,
}

/// What the filesystem states about a candidate, in the terms the shared rule
/// uses: the entry kind, whether it is executable, and whether the name alone
/// already marks it as structured state.
struct CandidateFacts {
    entry_kind: EntryKind,
    structured_state: Option<StructuredStateKind>,
}

impl CandidateFacts {
    fn read(path: &std::path::Path, metadata: &fs::Metadata) -> Self {
        let entry_kind = if metadata.is_dir() {
            EntryKind::Directory
        } else if metadata.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let structured_state = classify_structured_state(
            PathFacts::new(&name, entry_kind).executable(is_executable(metadata)),
        );
        Self {
            entry_kind,
            structured_state,
        }
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

/// Windows may report a missing child when its existing parent is a file.
/// Keep that blocked namespace observable instead of treating it as absent.
fn qualify_manual_ancestor_error(error: NeatiError) -> Option<NeatiError> {
    let NeatiError::Missing(missing_path) = &error else {
        return Some(error);
    };
    let Some(parent) = Path::new(missing_path).parent() else {
        return Some(error);
    };
    let io_error = |error: std::io::Error| match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            NeatiError::PermissionDenied(parent.display().to_string())
        }
        _ => NeatiError::Io(error.to_string()),
    };
    let metadata = match fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => return Some(io_error(error)),
    };
    match SymlinkGuard::is_symlink_from_metadata(parent, &metadata) {
        Ok(true) => Some(NeatiError::SymlinkEscape(format!(
            "Path ancestor is a symlink or reparse escape: {}",
            parent.display()
        ))),
        Ok(false) if metadata.is_dir() => None,
        Ok(false) => Some(NeatiError::Io(format!(
            "Path ancestor is not a directory: {}",
            parent.display()
        ))),
        Err(error) => Some(io_error(error)),
    }
}

impl DirectoryScanner {
    /// Scan with explicit owner observations and default traversal bounds.
    pub fn scan_signature(
        signature: &Signature,
        environment: &PlatformEnvironment,
        cancellation: &dyn crate::models::CancellationProbe,
        running_apps: &crate::applications::RunningApplications,
    ) -> Vec<ScanItem> {
        // A walk nobody is watching: default bounds, counters nobody reads, no
        // progress listener. It behaves exactly like a watched walk.
        let counters = TraversalCounters::default();
        let context = WalkContext::new(
            environment,
            cancellation,
            ScanLimits::default(),
            &counters,
            &NoRootProgress,
        );
        Self::scan_signature_with_context(
            signature,
            None,
            &context,
            EligibilityGate::Open,
            running_apps,
        )
        .items
    }

    pub fn scan_signature_with_context(
        signature: &Signature,
        pool: Option<&ThreadPool>,
        context: &WalkContext<'_>,
        gate: EligibilityGate,
        running_apps: &crate::applications::RunningApplications,
    ) -> SignatureScan {
        let mut items = Vec::new();
        let mut scanned_roots = Vec::new();
        let mut seen_roots = std::collections::HashSet::new();
        let mut selector_incomplete = false;
        let mut timings = SignatureTimings::default();

        // If signature has no explicit file paths (e.g. Docker commands), return early or handle in Docker adapter
        if signature.paths.is_empty() {
            return SignatureScan::new(items);
        }

        let unit_is_root = !signature.unit_kind().is_enumerated_child();

        for (idx, pattern) in signature.paths.iter().enumerate() {
            if context.cancellation.is_cancelled() {
                break;
            }
            let expansion_started = Instant::now();
            let Some(path_buf) = SignatureLoader::expand_path(pattern, context.environment) else {
                timings.root_expansion += expansion_started.elapsed();
                continue;
            };

            // The pattern's traversal is consumed page by page: a namespace
            // larger than one page is walked across pages rather than cut off
            // at the first one, so every parent is evaluated and the tail of a
            // large namespace is reachable. Cancellation stops between pages,
            // and what was covered before it did stays a lower bound the scan
            // reports as one.
            let (roots, failures, incomplete) =
                Self::signature_roots(&path_buf, context.environment, context.cancellation);
            timings.root_expansion += expansion_started.elapsed();
            selector_incomplete |= incomplete;

            for (fail_idx, failure) in failures.iter().enumerate() {
                let reason = format!(
                    "Could not inspect {}: {}",
                    failure.path.display(),
                    failure.error
                );
                items.push(
                    Self::unavailable_selector_item(
                        signature,
                        &failure.path,
                        idx,
                        fail_idx,
                        failures.len(),
                        reason,
                        gate,
                    )
                    .with_inspection_issue(failure.error_kind.map(|kind| {
                        inspection_issue_for_io(
                            context.environment,
                            &failure.path,
                            &std::io::Error::from(kind),
                        )
                    })),
                );
            }

            // One pattern can name many roots (`Application Support/*/GPUCache`),
            // and each root is scanned on its own so nothing about one match
            // decides another. A pattern with no selector yields exactly one
            // root and keeps its historical identity.
            for root in roots {
                if context.cancellation.is_cancelled() {
                    break;
                }
                let root_path = root.path.clone();
                // Catalog exclusions apply to selected roots as well as their
                // contents, so advisory discovery can defer to an exact owner.
                if crate::signatures::exclusions::is_excluded(
                    &root_path,
                    &signature.exclusions,
                    context.environment,
                ) {
                    continue;
                }
                let flavor = context.environment.flavor();
                let key = neati_platform::path_algebra::fold(
                    &neati_platform::path_algebra::normalize(&root_path.to_string_lossy(), flavor),
                    flavor,
                );
                if !seen_roots.insert(key) {
                    continue;
                }
                // The root is reported before it is read: a scan spends its
                // time inside one root, so this is what lets the interface say
                // where it is rather than only that it is running.
                context.progress.root_started(signature, &root_path);
                if context.cancellation.is_cancelled() {
                    break;
                }
                scanned_roots.push(root_path.clone());
                if let Some(min_age_days) = signature.min_age_days {
                    if !unit_is_root {
                        let started = Instant::now();
                        let previous_measurement = timings.tree_measurement;
                        let previous_preparation = timings.policy_preparation;
                        timings.measured_aged_children = true;
                        items.extend(Self::scan_aged_children(
                            context,
                            pool,
                            signature,
                            &root_path,
                            idx,
                            min_age_days,
                            gate,
                            root.key.as_deref(),
                            &mut timings,
                        ));
                        timings.child_enumeration += started
                            .elapsed()
                            .saturating_sub(timings.tree_measurement - previous_measurement)
                            .saturating_sub(timings.policy_preparation - previous_preparation);
                        continue;
                    }
                    items.push(Self::scan_aged_unit(
                        context,
                        signature,
                        &root_path,
                        idx,
                        min_age_days,
                        gate,
                        root.key.as_deref(),
                    ));
                    continue;
                }

                if let Some(item) = Self::scan_fixed_unit(
                    signature,
                    &root_path,
                    idx,
                    pool,
                    context,
                    gate,
                    root.key.as_deref(),
                ) {
                    items.push(item);
                }
            }
        }

        // A selection is derived from the facts above, so it is applied after
        // every field is set: only an auto-cleanable unit with reclaimable
        // bytes may be pre-selected.
        let use_started = Instant::now();
        // One batched open-file observation for the whole signature instead
        // of one lsof per unit (#493); each unit still gets its own verdict.
        let open_file_paths: Vec<std::path::PathBuf> = items
            .iter()
            .filter(|item| item.risk != crate::models::RiskTier::Manual)
            .filter_map(|item| {
                signature
                    .process_guard_for(Path::new(&item.path), context.environment)
                    .open_file_path()
                    .map(Path::to_path_buf)
            })
            .collect();
        if open_file_paths.len() > 1 {
            running_apps
                .prefetch_open_files(&open_file_paths, &|| context.cancellation.is_cancelled());
        }
        for item in &mut items {
            if item.risk != crate::models::RiskTier::Manual {
                let guard = signature.process_guard_for(Path::new(&item.path), context.environment);
                if !guard.is_empty() {
                    match running_apps.running_executables_with_cancellation(&guard, &|| {
                        context.cancellation.is_cancelled()
                    }) {
                        Some(names) => item.owner_running |= !names.is_empty(),
                        None => {
                            item.quality = ObservationQuality::Unavailable;
                            item.incomplete_reason =
                                Some("Owner process state could not be verified".into());
                            item.inspection_issue = Some(ScanGapKind::OwnerStateUnknown);
                        }
                    }
                }
            }
            let disposition = item.derive_disposition();
            item.disposition = disposition;
            item.is_selected = item.is_pre_selectable();
        }
        timings.use_checks += use_started.elapsed();

        SignatureScan {
            items,
            roots: scanned_roots,
            selector_incomplete,
            spans: timings.spans(signature),
        }
    }

    /// The concrete roots one pattern authorizes, alongside any branches that
    /// failed to enumerate due to permission or I/O errors.
    ///
    /// A literal pattern is its own root; a selector pattern is expanded once,
    /// deterministically, and produces one entry per match with the key that
    /// distinguishes them.
    fn signature_roots(
        expanded: &std::path::Path,
        environment: &PlatformEnvironment,
        cancellation: &dyn crate::models::CancellationProbe,
    ) -> (
        Vec<SignatureRoot>,
        Vec<neati_platform::selector::SelectionFailure>,
        bool,
    ) {
        let text = expanded.to_string_lossy().into_owned();
        if !neati_platform::selector::PathSelector::is_pattern(&text) {
            return (
                vec![SignatureRoot {
                    path: expanded.to_path_buf(),
                    key: None,
                }],
                Vec::new(),
                false,
            );
        }

        let Ok(selector) =
            neati_platform::selector::PathSelector::parse(&text, environment.flavor())
        else {
            // The catalog refuses a malformed selector at load time; a pattern
            // that reaches here is refused rather than treated as a literal
            // directory name.
            crate::diagnostics::log_error(
                "scanner",
                &format!("Refused a malformed catalog pattern: {text}"),
            );
            return (Vec::new(), Vec::new(), false);
        };

        let mut matches = Vec::new();
        let mut failures = Vec::new();
        let mut cursor = None;
        let mut incomplete = false;
        loop {
            let page = match selector.expand_page(environment.flavor(), cursor.as_ref()) {
                Ok(page) => page,
                Err(lost) => {
                    // A position that no longer names the same enumeration is
                    // not resumed into: the scan reports what it could not
                    // cover instead of claiming roots it never inspected.
                    crate::diagnostics::log_error(
                        "scanner",
                        &format!("Could not continue the traversal of `{text}`: {lost}"),
                    );
                    incomplete = true;
                    break;
                }
            };
            matches.extend(page.matches);
            failures.extend(page.failures);
            match page.continuation {
                Some(next) => {
                    cursor = Some(next);
                    if cancellation.is_cancelled() {
                        // Stopping between pages is the only way a traversal
                        // ends early, and the scan says so rather than
                        // presenting a partial enumeration as the whole one.
                        incomplete = true;
                        break;
                    }
                }
                None => break,
            }
        }
        let roots = matches
            .into_iter()
            .map(|path| {
                let key = selector
                    .match_key(&path.to_string_lossy(), environment.flavor())
                    .unwrap_or_default();
                SignatureRoot {
                    path,
                    key: Some(key),
                }
            })
            .collect();
        (roots, failures, incomplete)
    }

    /// Scans one root when the root itself is the cleanup unit.
    ///
    /// The root's whole tree is the unit, so its identity, its entry kind, and
    /// its structured-state classification describe exactly what a plan would
    /// delete.
    #[allow(clippy::too_many_arguments)]
    fn scan_fixed_unit(
        signature: &Signature,
        path_buf: &std::path::Path,
        idx: usize,
        pool: Option<&ThreadPool>,
        context: &WalkContext<'_>,
        gate: EligibilityGate,
        root_key: Option<&str>,
    ) -> Option<ScanItem> {
        let environment = context.environment;
        // A symlink, junction, or mount point is a boundary, not a target: the
        // guard classifies the reparse tag, so a cloud placeholder stays an
        // ordinary entry while an indirection is refused. `Path::exists()`
        // collapses every metadata error into `false`, so permission and I/O
        // failures stay observable instead of reading as an absent path.
        // Fixed advisory roots can be outside a standard cache directory (for
        // example an injected XDG root). Refuse indirection in their ancestors
        // before measuring anything there. A missing namespace is still
        // absent, while access and inspection failures remain explicit gaps.
        let ancestor_error = (signature.strategy == CleanStrategy::Manual)
            .then(|| SymlinkGuard::validate_anchored_path(path_buf, environment).err())
            .flatten()
            .and_then(qualify_manual_ancestor_error);
        let is_link = ancestor_error.is_none() && SymlinkGuard::is_symlink(path_buf);
        let (exists, measurement, facts) = if let Some(error) = ancestor_error {
            let issue = match &error {
                NeatiError::SymlinkEscape(_) => ScanGapKind::SafetyProtected,
                NeatiError::PermissionDenied(_) => inspection_issue_for_io(
                    environment,
                    path_buf,
                    &std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                ),
                _ => ScanGapKind::Unknown,
            };
            (
                true,
                PathMeasurement::unavailable(format!(
                    "Could not inspect configured path {}: {}",
                    path_buf.display(),
                    error
                ))
                .with_inspection_issue(issue),
                None,
            )
        } else {
            match fs::symlink_metadata(path_buf) {
                Ok(_) if is_link => (
                    true,
                    PathMeasurement::unavailable(format!(
                    "Configured path {} is a link, junction, or mount point; cleanup is blocked",
                    path_buf.display()
                ))
                    .with_inspection_issue(ScanGapKind::SafetyProtected),
                    None,
                ),
                Ok(metadata) => {
                    let facts = CandidateFacts::read(path_buf, &metadata);
                    let measure = || {
                        SizeCalculator::measure_path_with_bundle_rule(
                            path_buf,
                            &signature.exclusions,
                            pool,
                            context.environment,
                            context.cancellation,
                            context.limits,
                            context.counters,
                            context.permit_application_bundles,
                        )
                    };
                    // Another rule may have named this root with the same
                    // measurement parameters already in this scan (#493).
                    let measurement = match context.measurements {
                        Some(memo) => memo.measure_path(
                            path_buf,
                            &signature.exclusions,
                            context.permit_application_bundles,
                            context.limits,
                            context.cancellation,
                            measure,
                        ),
                        None => measure(),
                    };
                    (true, measurement, Some(facts))
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => (
                    false,
                    PathMeasurement::complete(FileSize::default(), 0),
                    None,
                ),
                Err(err) => (
                    true,
                    PathMeasurement::unavailable(format!(
                        "Could not inspect configured path {}: {}",
                        path_buf.display(),
                        describe_inspection_error(environment, path_buf, &err)
                    ))
                    .with_inspection_issue(inspection_issue_for_io(
                        environment,
                        path_buf,
                        &err,
                    )),
                    None,
                ),
            }
        };

        let size = measurement.size;
        let file_count = measurement.file_count;
        let skipped_entry_count = measurement.skipped_entries;
        let (quality, incomplete_reason) = if !exists || measurement.complete {
            (ObservationQuality::Fresh, None)
        } else if size.observed_bytes() == 0 {
            (
                ObservationQuality::Unavailable,
                measurement
                    .incomplete_reason
                    .or_else(|| Some("Inaccessible path; read failed".into())),
            )
        } else {
            (
                ObservationQuality::Partial,
                measurement
                    .incomplete_reason
                    .or_else(|| Some("Incomplete scan; some entries could not be read".into())),
            )
        };

        let mut cache_metadata = signature.cache_metadata();
        if quality == ObservationQuality::Partial {
            cache_metadata.size_semantics = CacheSizeSemantics::ConservativeLowerBound;
        } else if quality == ObservationQuality::Unavailable {
            cache_metadata.size_semantics = CacheSizeSemantics::Informational;
        }

        let last_modified = if exists && facts.is_some() {
            fs::metadata(path_buf)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        } else {
            None
        };

        let item_id = match (signature.paths.len() > 1, root_key) {
            (multi, Some(key)) => {
                let index = if multi {
                    format!(".{idx}")
                } else {
                    String::new()
                };
                format!("{}{}.{}", signature.id, index, key)
            }
            (true, None) => format!("{}.{}", signature.id, idx),
            (false, None) => signature.id.clone(),
        };

        let display_name = match root_key {
            // A selected root is one of several: the row names which one, so
            // two matches of the same pattern never look like one another.
            Some(key) => format!("{} ({})", signature.name, key),
            None if signature.paths.len() > 1 => format!(
                "{} ({})",
                signature.name,
                path_buf
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
            ),
            None => signature.name.clone(),
        };

        let path_text = path_buf.to_string_lossy().to_string();
        let disposition = derive_cleanup_disposition(
            DispositionFacts::new(
                signature.risk,
                quality,
                &cache_metadata,
                &size,
                incomplete_reason.as_deref(),
            )
            .with_gate(gate)
            .with_stale_entries(None)
            .with_structured_state(facts.as_ref().and_then(|facts| facts.structured_state)),
        );

        Some(ScanItem {
            id: item_id,
            signature_id: signature.id.clone(),
            name: display_name,
            category: signature.category,
            risk: signature.risk,
            path: path_text.clone(),
            size,
            file_count,
            description: signature.description.clone(),
            cache_metadata,
            disposition,
            unit: CleanupUnit::new(signature.unit_kind(), path_text.clone(), path_text),
            ownership: signature.ownership(),
            age: None,
            stale: None,
            structured_state: facts.as_ref().and_then(|facts| facts.structured_state),
            provider_restriction: None,
            entry_kind: facts
                .as_ref()
                .map(|facts| facts.entry_kind)
                .unwrap_or(EntryKind::Directory),
            gate,
            owner_running: false,
            lifecycle_provider_action: false,
            requires_confirmation: false,
            overlaps: Vec::new(),
            is_selected: false,
            last_modified,
            exists,
            quality,
            inspection_issue: measurement.inspection_issue,
            incomplete_reason,
            skipped_entry_count,
        })
    }

    /// Scans one root as a unit whose whole tree carries the age policy.
    ///
    /// A named cache subtree is deleted whole or not at all, so its freshness
    /// is the freshness of everything inside it. This is the shape the generic
    /// aged-child rule cannot cover: the root is the unit, not its children.
    #[allow(clippy::too_many_arguments)]
    fn scan_aged_unit(
        context: &WalkContext<'_>,
        signature: &Signature,
        path_buf: &std::path::Path,
        idx: usize,
        min_age_days: u32,
        gate: EligibilityGate,
        root_key: Option<&str>,
    ) -> ScanItem {
        let environment = context.environment;
        let is_link = SymlinkGuard::is_symlink(path_buf);
        let stats = fs::symlink_metadata(path_buf);
        let stats = match stats {
            Ok(_) if is_link => {
                return Self::unavailable_aged_item(
                    signature,
                    path_buf,
                    Self::item_id_for(signature, idx, root_key, None),
                    Self::display_name_for(signature, path_buf, root_key),
                    FileSize::default(),
                    0,
                    None,
                    1,
                    format!(
                        "Configured path {} is a link, junction, or mount point; cleanup is blocked",
                        path_buf.display()
                    ),
                    gate,
                    false,
                ).with_inspection_issue(Some(ScanGapKind::SafetyProtected));
            }
            Ok(_) => Self::measure_tree_stats(context, path_buf, &signature.exclusions, 0, None),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Self::unavailable_aged_item(
                    signature,
                    path_buf,
                    Self::item_id_for(signature, idx, root_key, None),
                    Self::display_name_for(signature, path_buf, root_key),
                    FileSize::default(),
                    0,
                    None,
                    0,
                    format!("Configured path {} does not exist", path_buf.display()),
                    gate,
                    false,
                )
                .with_inspection_issue(None);
            }
            Err(err) => {
                return Self::unavailable_aged_item(
                    signature,
                    path_buf,
                    Self::item_id_for(signature, idx, root_key, None),
                    Self::display_name_for(signature, path_buf, root_key),
                    FileSize::default(),
                    0,
                    None,
                    1,
                    format!(
                        "Could not inspect {}: {}",
                        path_buf.display(),
                        describe_inspection_error(environment, path_buf, &err)
                    ),
                    gate,
                    false,
                )
                .with_inspection_issue(Some(inspection_issue_for_io(
                    environment,
                    path_buf,
                    &err,
                )));
            }
        };

        let size = FileSize::new(stats.logical, Some(stats.allocated));
        let newest_modified = stats
            .newest_mtime
            .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = AgeObservation::evaluate(min_age_days, newest_modified, now_secs);

        if !stats.complete {
            let reason = stats
                .incomplete_reason
                .clone()
                .unwrap_or_else(|| format!("Could not completely inspect {}", path_buf.display()));
            return Self::unavailable_aged_item(
                signature,
                path_buf,
                Self::item_id_for(signature, idx, root_key, None),
                Self::display_name_for(signature, path_buf, root_key),
                size,
                stats.file_count,
                newest_modified,
                stats.skipped_entries,
                reason,
                gate,
                false,
            )
            .with_inspection_issue(stats.inspection_issue);
        }

        let facts = match fs::symlink_metadata(path_buf) {
            Ok(metadata) => CandidateFacts::read(path_buf, &metadata),
            Err(_) => CandidateFacts {
                entry_kind: EntryKind::Directory,
                structured_state: None,
            },
        };

        let description = if age.satisfied {
            format!(
                "{} (unchanged for at least {} days)",
                signature.description, min_age_days
            )
        } else {
            format!(
                "{} (used within the last {} days; kept until it is stale)",
                signature.description, min_age_days
            )
        };

        let path_text = path_buf.to_string_lossy().to_string();
        let cache_metadata = signature.cache_metadata();
        let disposition = derive_cleanup_disposition(
            DispositionFacts::new(
                signature.risk,
                ObservationQuality::Fresh,
                &cache_metadata,
                &size,
                None,
            )
            .with_gate(gate)
            .with_age(Some(&age))
            .with_structured_state(facts.structured_state),
        );

        ScanItem {
            id: Self::item_id_for(signature, idx, root_key, None),
            signature_id: signature.id.clone(),
            name: Self::display_name_for(signature, path_buf, root_key),
            category: signature.category,
            risk: signature.risk,
            path: path_text.clone(),
            size,
            file_count: stats.file_count,
            description,
            cache_metadata,
            disposition,
            unit: CleanupUnit::new(signature.unit_kind(), path_text.clone(), path_text),
            ownership: signature.ownership(),
            age: Some(age),
            stale: None,
            structured_state: facts.structured_state,
            provider_restriction: None,
            entry_kind: facts.entry_kind,
            gate,
            owner_running: false,
            lifecycle_provider_action: false,
            requires_confirmation: false,
            overlaps: Vec::new(),
            is_selected: false,
            last_modified: newest_modified,
            exists: true,
            quality: ObservationQuality::Fresh,
            inspection_issue: None,
            incomplete_reason: None,
            skipped_entry_count: stats.skipped_entries,
        }
    }

    /// The item identity for one root: signature, pattern, selected key, child.
    fn item_id_for(
        signature: &Signature,
        idx: usize,
        root_key: Option<&str>,
        child: Option<&str>,
    ) -> String {
        let mut id = signature.id.clone();
        if signature.paths.len() > 1 || root_key.is_some() {
            id.push_str(&format!(".{idx}"));
        }
        if let Some(key) = root_key {
            id.push('.');
            id.push_str(key);
        }
        if let Some(child) = child {
            id.push('.');
            id.push_str(child);
        }
        id
    }

    /// The display name for one root.
    fn display_name_for(
        signature: &Signature,
        path_buf: &std::path::Path,
        root_key: Option<&str>,
    ) -> String {
        match root_key {
            Some(key) => format!("{} ({})", signature.name, key),
            None if signature.paths.len() > 1 => format!(
                "{} ({})",
                signature.name,
                path_buf
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
            ),
            None => signature.name.clone(),
        }
    }

    /// Enumerates the children of an aged root and reports each as its own unit.
    ///
    /// A child is inventoried whatever its age: the age policy decides
    /// eligibility, not visibility. That is the difference between "this cache
    /// is 4 GB and it is being written right now" and a namespace that silently
    /// vanishes from the totals.
    #[allow(clippy::too_many_arguments)]
    fn scan_aged_children(
        context: &WalkContext<'_>,
        pool: Option<&ThreadPool>,
        signature: &Signature,
        root: &std::path::Path,
        path_index: usize,
        min_age_days: u32,
        gate: EligibilityGate,
        root_key: Option<&str>,
        timings: &mut SignatureTimings,
    ) -> Vec<ScanItem> {
        let environment = context.environment;
        if context.cancellation.is_cancelled() {
            return Vec::new();
        }
        // This function reads the namespace root directly. Count that root here
        // instead of in the generic signature loop so every filesystem entry is
        // represented exactly once in traversal metrics.
        context.counters.visit_entry();
        // A signature that removes stale entries ages each entry of a namespace;
        // every other aged signature removes a child whole and ages that child's
        // whole tree. The strategy is the difference, and it decides which
        // verdict the scan reports.
        let stale_policy = (signature.strategy
            == crate::models::CleanStrategy::DeleteStaleContents)
            .then(|| crate::safety::StaleEntryPolicy::from_days(min_age_days));
        let root_is_link = SymlinkGuard::is_symlink(root);
        match fs::symlink_metadata(root) {
            Ok(_) if root_is_link => {
                return vec![Self::unavailable_aged_item(
                    signature,
                    root,
                    format!(
                        "{}.unavailable",
                        Self::item_id_for(signature, path_index, root_key, None)
                    ),
                    Self::display_name_for(signature, root, root_key),
                    FileSize::default(),
                    0,
                    None,
                    1,
                    format!(
                        "Configured path {} is a link, junction, or mount point; cleanup is blocked",
                        root.display()
                    ),
                    gate,
                    false,
                ).with_inspection_issue(Some(ScanGapKind::SafetyProtected))];
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return vec![],
            Err(err) => {
                return vec![Self::unavailable_aged_item(
                    signature,
                    root,
                    format!(
                        "{}.unavailable",
                        Self::item_id_for(signature, path_index, root_key, None)
                    ),
                    Self::display_name_for(signature, root, root_key),
                    FileSize::default(),
                    0,
                    None,
                    1,
                    format!(
                        "Could not inspect {}: {}",
                        root.display(),
                        describe_inspection_error(environment, root, &err)
                    ),
                    gate,
                    false,
                )
                .with_inspection_issue(Some(inspection_issue_for_io(
                    environment,
                    root,
                    &err,
                )))];
            }
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => {
                context.counters.directory_read();
                entries
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return vec![],
            Err(err) => {
                return vec![Self::unavailable_aged_item(
                    signature,
                    root,
                    format!(
                        "{}.unavailable",
                        Self::item_id_for(signature, path_index, root_key, None)
                    ),
                    Self::display_name_for(signature, root, root_key),
                    FileSize::default(),
                    0,
                    None,
                    1,
                    format!(
                        "Could not inspect {}: {}",
                        root.display(),
                        describe_inspection_error(environment, root, &err)
                    ),
                    gate,
                    false,
                )
                .with_inspection_issue(Some(inspection_issue_for_io(
                    environment,
                    root,
                    &err,
                )))];
            }
        };
        let now = SystemTime::now();
        let now_secs = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let root_text = root.to_string_lossy().to_string();
        let unit_kind = signature.unit_kind();
        let mut items = Vec::new();
        // Classification is sequential and keeps enumeration order; the
        // children it admits are measured afterwards, side by side.
        let mut slots: Vec<AgedSlot> = Vec::new();
        let mut entry_failure = None;
        let mut entry_issue = None;
        let mut coverage_count = 0usize;
        let preparation_started = Instant::now();
        let observation_policy = TreeObservationPolicy::new(&signature.exclusions, environment);
        timings.policy_preparation += preparation_started.elapsed();
        // Temp roots are deliberately a prefix-only catalog, even for read-only
        // accounting. Unrelated user/session files are outside this inventory.
        let observe_include_misses = ![
            "${TEMP}",
            "$TMPDIR",
            "${SHARED_TEMP}",
            "/tmp",
            "/private/tmp",
        ]
        .iter()
        .filter_map(|pattern| SignatureLoader::expand_path(pattern, environment))
        .any(|temp| temp == root);

        for entry in entries {
            if context.cancellation.is_cancelled() {
                break;
            }
            let classification_started = Instant::now();
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    if entry_failure.is_none() {
                        entry_issue = Some(inspection_issue_for_io(environment, root, &err));
                        entry_failure = Some(format!(
                            "Could not read an entry in {}: {}",
                            root.display(),
                            err
                        ));
                    }
                    timings.child_classification += classification_started.elapsed();
                    continue;
                }
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !crate::signatures::exclusions::is_included_namespace(
                &name,
                &signature.include_prefixes,
            ) {
                if !observe_include_misses {
                    timings.child_classification += classification_started.elapsed();
                    continue;
                }
                coverage_count += 1;
                if coverage_count > 64 {
                    entry_failure =
                        Some("Excluded namespace observation reached its 64-root limit".into());
                    timings.child_classification += classification_started.elapsed();
                    continue;
                }
                slots.push(AgedSlot::ready(super::coverage::observe(signature, &path,
                    "Outside this signature's included namespaces; no cleanup operation is authorized", context)));
                timings.child_classification += classification_started.elapsed();
                continue;
            }
            // Use one no-follow metadata observation for classification. The
            // Windows reparse-tag query remains fresh and fail-closed.
            let metadata = match fs::symlink_metadata(&path).map(|metadata| {
                let is_link =
                    SymlinkGuard::is_symlink_from_metadata(&path, &metadata).unwrap_or(true);
                (metadata, is_link)
            }) {
                Ok((_, true)) => {
                    slots.push(AgedSlot::ready(
                        Self::unavailable_aged_item(
                            signature,
                            &path,
                            Self::item_id_for(signature, path_index, root_key, Some(&name)),
                            name,
                            FileSize::default(),
                            0,
                            None,
                            1,
                            format!(
                            "Candidate {} is a link, junction, or mount point; cleanup is blocked",
                            path.display()
                        ),
                            gate,
                            false,
                        )
                        .with_inspection_issue(Some(ScanGapKind::SafetyProtected)),
                    ));
                    timings.child_classification += classification_started.elapsed();
                    continue;
                }
                Ok((metadata, false)) => metadata,
                Err(err) => {
                    slots.push(AgedSlot::ready(
                        Self::unavailable_aged_item(
                            signature,
                            &path,
                            Self::item_id_for(signature, path_index, root_key, Some(&name)),
                            name,
                            FileSize::default(),
                            0,
                            None,
                            1,
                            format!(
                                "Could not inspect {}: {}",
                                path.display(),
                                describe_inspection_error(environment, &path, &err)
                            ),
                            gate,
                            false,
                        )
                        .with_inspection_issue(Some(
                            inspection_issue_for_io(environment, &path, &err),
                        )),
                    ));
                    timings.child_classification += classification_started.elapsed();
                    continue;
                }
            };
            if crate::signatures::exclusions::is_excluded_namespace(
                &name,
                &signature.exclude_prefixes,
            ) || crate::signatures::exclusions::is_excluded(
                &path,
                &signature.exclusions,
                environment,
            ) {
                coverage_count += 1;
                if coverage_count > 64 {
                    entry_failure =
                        Some("Excluded namespace observation reached its 64-root limit".into());
                    timings.child_classification += classification_started.elapsed();
                    continue;
                }
                slots.push(AgedSlot::ready(super::coverage::observe(
                    signature,
                    &path,
                    "Excluded from generic cleanup; use its dedicated owner when available",
                    context,
                )));
                timings.child_classification += classification_started.elapsed();
                continue;
            }

            let facts = CandidateFacts::read(&path, &metadata);
            let path_text = path.to_string_lossy().to_string();
            let unit = CleanupUnit::new(unit_kind, root_text.clone(), path_text.clone());
            let ownership = if signature.ownership().is_known() {
                signature.ownership()
            } else {
                // A namespace enumerated under a broad root has no catalog
                // owner: its own name is the only statement about who it
                // belongs to, and it is reported as an inference.
                CleanupOwnership::inferred(name.clone())
            };
            timings.child_classification += classification_started.elapsed();
            slots.push(AgedSlot::Measure(AgedChild {
                path,
                name,
                facts,
                path_text,
                unit,
                ownership,
            }));
        }

        // Single-pass fail-closed tree measurement of every classified child,
        // side by side on the shared pool (#493). Results keep enumeration
        // order, so the items below are the ones a serial walk builds.
        let children: Vec<&Path> = slots
            .iter()
            .filter_map(|slot| match slot {
                AgedSlot::Measure(child) => Some(child.path.as_path()),
                AgedSlot::Ready(_) => None,
            })
            .collect();
        let (measured, measured_timings) = aged_measurement::measure_children(
            context,
            pool,
            &observation_policy,
            stale_policy,
            &children,
        );
        timings.tree_measurement += measured_timings.tree_measurement;
        timings.metadata_and_links += measured_timings.metadata_and_links;
        timings.age_evaluation += measured_timings.age_evaluation;
        timings.size_accounting += measured_timings.size_accounting;
        timings.traversal += measured_timings.traversal;
        let mut measured = measured.into_iter();
        for slot in slots {
            let child = match slot {
                AgedSlot::Ready(item) => {
                    items.push(*item);
                    continue;
                }
                AgedSlot::Measure(child) => child,
            };
            // A child the cancelled scan never reached ends the walk here, as
            // the serial walk stopped before it; a child measured while the
            // scan was stopped is the last one it reports.
            let Some(aged_measurement::MeasuredChild { stats, stopped }) =
                measured.next().flatten()
            else {
                break;
            };
            items.extend(Self::aged_child_item(
                signature,
                child,
                stats,
                path_index,
                root_key,
                gate,
                min_age_days,
                now_secs,
                stale_policy,
            ));
            if stopped {
                break;
            }
        }

        if let Some(reason) = entry_failure {
            items.push(
                Self::unavailable_aged_item(
                    signature,
                    root,
                    format!(
                        "{}.unavailable",
                        Self::item_id_for(signature, path_index, root_key, None)
                    ),
                    format!(
                        "{} (scan incomplete)",
                        Self::display_name_for(signature, root, root_key)
                    ),
                    FileSize::default(),
                    0,
                    None,
                    1,
                    reason,
                    gate,
                    false,
                )
                .with_inspection_issue(entry_issue),
            );
        }

        items
    }

    /// The item one measured child of an aged root is reported as, or nothing
    /// for a complete observation of an empty, satisfied child.
    #[allow(clippy::too_many_arguments)]
    fn aged_child_item(
        signature: &Signature,
        child: AgedChild,
        stats: TreeStats,
        path_index: usize,
        root_key: Option<&str>,
        gate: EligibilityGate,
        min_age_days: u32,
        now_secs: u64,
        stale_policy: Option<crate::safety::StaleEntryPolicy>,
    ) -> Option<ScanItem> {
        let AgedChild {
            path,
            name,
            facts,
            path_text,
            unit,
            ownership,
        } = child;
        // An incomplete tree cannot prove the candidate's newest timestamp,
        // so retain it for observability but block cleanup.
        if !stats.complete {
            let size = FileSize::new(stats.logical, Some(stats.allocated));
            let last_modified = stats
                .newest_mtime
                .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs());
            return Some(
                Self::unavailable_aged_item(
                    signature,
                    &path,
                    Self::item_id_for(signature, path_index, root_key, Some(&name)),
                    name,
                    size,
                    stats.file_count,
                    last_modified,
                    stats.skipped_entries,
                    stats.incomplete_reason.unwrap_or_else(|| {
                        format!("Could not completely inspect {}", path.display())
                    }),
                    gate,
                    false,
                )
                .with_inspection_issue(stats.inspection_issue),
            );
        }

        let size = FileSize::new(stats.logical, Some(stats.allocated));
        let newest_modified = stats
            .newest_mtime
            .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        let age = AgeObservation::evaluate(min_age_days, newest_modified, now_secs);
        let stale = stale_policy.map(|_| {
            StaleEntryObservation::new(min_age_days, stats.stale_bytes, stats.stale_file_count)
        });

        // An empty fresh observation carries nothing a user could act on.
        if size.observed_bytes() == 0 && age.satisfied {
            return None;
        }

        let description = match stale {
            Some(stale) if stale.nothing_is_stale() => format!(
                "{} (nothing inside has been inactive for {} days)",
                signature.description, min_age_days
            ),
            Some(stale) => format!(
                "{} ({} of it has been inactive for at least {} days)",
                signature.description,
                format_bytes(stale.stale_bytes),
                min_age_days
            ),
            None if age.satisfied => format!(
                "{} (unchanged for at least {} days)",
                signature.description, min_age_days
            ),
            None => format!(
                "{} (used within the last {} days; kept until it is stale)",
                signature.description, min_age_days
            ),
        };

        // The signature's resolved guard below is the single owner policy.
        // Do not pre-mark a whole namespace busy before its unit check.
        let owner_running = false;

        let cache_metadata = signature.cache_metadata();
        let disposition = derive_cleanup_disposition(
            DispositionFacts::new(
                signature.risk,
                ObservationQuality::Fresh,
                &cache_metadata,
                &size,
                None,
            )
            .with_gate(gate)
            .with_running_owner(owner_running)
            .with_age(stale.is_none().then_some(&age))
            .with_stale_entries(stale.as_ref())
            .with_structured_state(facts.structured_state),
        );
        let is_selected = disposition.eligibility == CleanupEligibility::AutoCleanable
            && disposition.cleanable_bytes.unwrap_or(0) > 0;

        let display_name = match root_key {
            Some(key) => format!("{name} ({key})"),
            None => name.clone(),
        };
        Some(ScanItem {
            id: Self::item_id_for(signature, path_index, root_key, Some(&name)),
            signature_id: signature.id.clone(),
            name: display_name,
            category: signature.category,
            risk: signature.risk,
            path: path_text,
            size,
            file_count: stats.file_count,
            description,
            cache_metadata,
            disposition,
            unit,
            ownership,
            age: stale.is_none().then_some(age),
            stale,
            structured_state: facts.structured_state,
            provider_restriction: None,
            entry_kind: facts.entry_kind,
            gate,
            owner_running,
            lifecycle_provider_action: false,
            requires_confirmation: false,
            overlaps: Vec::new(),
            is_selected,
            last_modified: newest_modified,
            exists: true,
            quality: ObservationQuality::Fresh,
            inspection_issue: None,
            incomplete_reason: None,
            skipped_entry_count: stats.skipped_entries,
        })
    }

    fn unavailable_selector_item(
        signature: &Signature,
        path: &Path,
        idx: usize,
        fail_idx: usize,
        fail_count: usize,
        reason: String,
        gate: EligibilityGate,
    ) -> ScanItem {
        let failure_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let display_name = if failure_name.is_empty() || failure_name == signature.name {
            signature.name.clone()
        } else {
            format!("{} ({failure_name})", signature.name)
        };
        let item_id = if fail_count > 1 {
            format!("{}.{idx}.unavailable.{fail_idx}", signature.id)
        } else if signature.paths.len() > 1 {
            format!("{}.{idx}.unavailable", signature.id)
        } else {
            format!("{}.unavailable", signature.id)
        };
        let description = if signature.min_age_days.is_some() {
            format!(
                "{} Age eligibility could not be verified, so cleanup is blocked.",
                signature.description
            )
        } else {
            signature.description.clone()
        };
        let mut cache_metadata = signature.cache_metadata();
        cache_metadata.size_semantics = CacheSizeSemantics::Informational;
        let disposition = derive_cleanup_disposition(
            DispositionFacts::new(
                signature.risk,
                ObservationQuality::Unavailable,
                &cache_metadata,
                &FileSize::default(),
                Some(&reason),
            )
            .with_gate(gate),
        );
        let path_text = path.to_string_lossy().to_string();
        ScanItem {
            id: item_id,
            signature_id: signature.id.clone(),
            name: display_name,
            category: signature.category,
            risk: signature.risk,
            path: path_text.clone(),
            size: FileSize::default(),
            file_count: 0,
            description,
            cache_metadata,
            disposition,
            unit: CleanupUnit::new(signature.unit_kind(), path_text.clone(), path_text),
            ownership: signature.ownership(),
            age: None,
            stale: None,
            structured_state: None,
            provider_restriction: None,
            entry_kind: EntryKind::Directory,
            gate,
            owner_running: false,
            lifecycle_provider_action: false,
            requires_confirmation: false,
            overlaps: Vec::new(),
            is_selected: false,
            last_modified: None,
            exists: true,
            quality: ObservationQuality::Unavailable,
            inspection_issue: None,
            incomplete_reason: Some(reason),
            skipped_entry_count: 1,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn unavailable_aged_item(
        signature: &Signature,
        path: &Path,
        id: String,
        name: String,
        size: FileSize,
        file_count: usize,
        last_modified: Option<u64>,
        skipped_entry_count: u64,
        reason: String,
        gate: EligibilityGate,
        owner_running: bool,
    ) -> ScanItem {
        let mut cache_metadata = signature.cache_metadata();
        // An uninspectable candidate's size is informational: it can never be
        // cleaned, so it must not render as a reclaimable lower bound.
        cache_metadata.size_semantics = CacheSizeSemantics::Informational;
        let disposition = derive_cleanup_disposition(
            DispositionFacts::new(
                signature.risk,
                ObservationQuality::Unavailable,
                &cache_metadata,
                &size,
                Some(&reason),
            )
            .with_gate(gate),
        );
        let path_text = path.to_string_lossy().to_string();
        ScanItem {
            id,
            signature_id: signature.id.clone(),
            name,
            category: signature.category,
            risk: signature.risk,
            path: path_text.clone(),
            size,
            file_count,
            description: format!(
                "{} Age eligibility could not be verified, so cleanup is blocked.",
                signature.description
            ),
            cache_metadata,
            disposition,
            unit: CleanupUnit::new(signature.unit_kind(), path_text.clone(), path_text),
            ownership: signature.ownership(),
            age: None,
            stale: None,
            structured_state: None,
            provider_restriction: None,
            entry_kind: EntryKind::Directory,
            gate,
            owner_running,
            lifecycle_provider_action: false,
            requires_confirmation: false,
            overlaps: Vec::new(),
            is_selected: false,
            last_modified,
            exists: true,
            quality: ObservationQuality::Unavailable,
            inspection_issue: None,
            incomplete_reason: Some(reason),
            skipped_entry_count,
        }
    }

    /// Measures directory statistics (size, count, newest mtime) in a single recursive pass.
    /// Marks complete = false if any error, symlink escape, or depth cutoff occurs.
    pub fn measure_tree_stats(
        context: &WalkContext<'_>,
        path: &Path,
        exclusions: &[String],
        current_depth: usize,
        stale_policy: Option<crate::safety::StaleEntryPolicy>,
    ) -> TreeStats {
        let policy = TreeObservationPolicy::new(exclusions, context.environment);
        Self::measure_tree_stats_with_policy(context, path, &policy, current_depth, stale_policy)
    }

    fn measure_tree_stats_with_policy(
        context: &WalkContext<'_>,
        path: &Path,
        policy: &TreeObservationPolicy,
        current_depth: usize,
        stale_policy: Option<crate::safety::StaleEntryPolicy>,
    ) -> TreeStats {
        Self::measure_tree_stats_profiled(
            context,
            path,
            policy,
            current_depth,
            stale_policy,
            &mut TreePhaseTimings::default(),
        )
    }

    fn measure_tree_stats_profiled(
        context: &WalkContext<'_>,
        path: &Path,
        policy: &TreeObservationPolicy,
        current_depth: usize,
        stale_policy: Option<crate::safety::StaleEntryPolicy>,
        phases: &mut TreePhaseTimings,
    ) -> TreeStats {
        let environment = context.environment;
        let cancellation = context.cancellation;
        let max_depth = context.limits.max_depth;
        let mut stats = TreeStats {
            stale_bytes: 0,
            stale_file_count: 0,
            logical: 0,
            allocated: 0,
            file_count: 0,
            newest_mtime: None,
            complete: true,
            incomplete_reason: None,
            inspection_issue: None,
            skipped_entries: 0,
        };

        if cancellation.is_cancelled() {
            stats.complete = false;
            stats.skipped_entries = 1;
            stats.incomplete_reason =
                Some(format!("Scan cancelled while measuring {}", path.display()));
            stats.inspection_issue = Some(ScanGapKind::Cancelled);
            return stats;
        }

        context.counters.visit_entry();
        if current_depth > max_depth {
            stats.complete = false;
            stats.skipped_entries += 1;
            stats.incomplete_reason = Some(format!(
                "Directory depth limit of {} exceeded at {}",
                max_depth,
                path.display()
            ));
            stats.inspection_issue = Some(ScanGapKind::DepthLimit);
            return stats;
        }

        let metadata_started = Instant::now();
        let observation = fs::symlink_metadata(path).and_then(|meta| {
            SymlinkGuard::is_symlink_from_metadata(path, &meta).map(|is_link| (meta, is_link))
        });
        phases.metadata_and_links += metadata_started.elapsed();
        let (meta, is_link) = match observation {
            Ok(observation) => observation,
            Err(err) => {
                stats.complete = false;
                stats.skipped_entries += 1;
                stats.incomplete_reason = Some(format!(
                    "Failed to read metadata for {}: {}",
                    path.display(),
                    err
                ));
                stats.inspection_issue = Some(inspection_issue_for_io(environment, path, &err));
                return stats;
            }
        };

        // Signed app bundles are protected by macOS App Management (TCC):
        // chmod and deletion inside them fail with EPERM even for the owning
        // user, so generic cleanup can never succeed. Treat any bundle in the
        // tree like an unreadable subtree and fail closed.
        //
        // A signature whose policy permits bundles (owner-guarded build
        // output) measures built products as content. If macOS still refuses
        // one at removal, execution reports that entry as a failure.
        if !context.permit_application_bundles
            && meta.is_dir()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        {
            stats.complete = false;
            stats.skipped_entries += 1;
            stats.incomplete_reason = Some(format!(
                "Protected application bundle encountered in {}",
                path.display()
            ));
            stats.inspection_issue = Some(ScanGapKind::SafetyProtected);
            return stats;
        }

        let age_started = Instant::now();
        let modified = meta.modified().ok();
        if let Some(modified) = modified {
            stats.newest_mtime = Some(match stats.newest_mtime {
                Some(existing) => existing.max(modified),
                None => modified,
            });
        }

        phases.age_evaluation += age_started.elapsed();

        // The walk refuses an indirection and accounts for the link itself;
        // `SymlinkGuard` is the same classifier size.rs uses, so a junction is
        // a boundary in both walks rather than only in one.
        if is_link || meta.is_file() {
            let size_started = Instant::now();
            let len = meta.len();
            stats.logical = len;
            #[cfg(unix)]
            {
                stats.allocated = meta.blocks() * 512;
            }
            #[cfg(windows)]
            {
                stats.allocated = crate::scanner::get_allocated_size(path).unwrap_or(len);
            }
            #[cfg(not(any(unix, windows)))]
            {
                stats.allocated = len;
            }
            stats.file_count = 1;
            phases.size_accounting += size_started.elapsed();

            // The entry's own verdict, from the entry's own facts. The
            // execution guard evaluates the same predicate for the same file,
            // so the estimate and the deletion agree by construction.
            if let Some(policy) = stale_policy {
                let age_started = Instant::now();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let now = SystemTime::now();
                if policy.allows(
                    &name,
                    crate::safety::stale::entry_kind(&meta),
                    crate::safety::stale::is_executable(&meta),
                    modified,
                    now,
                ) {
                    stats.stale_bytes += stats.allocated;
                    stats.stale_file_count = 1;
                }
                phases.age_evaluation += age_started.elapsed();
            }
            return stats;
        }

        if !meta.is_dir() {
            return stats;
        }
        let stale_policy = if CandidateFacts::read(path, &meta).structured_state.is_some() {
            None
        } else {
            stale_policy
        };

        context.counters.directory_read();
        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(err) => {
                stats.complete = false;
                stats.skipped_entries += 1;
                stats.incomplete_reason = Some(format!(
                    "Failed to read directory {}: {}",
                    path.display(),
                    describe_inspection_error(environment, path, &err)
                ));
                stats.inspection_issue = Some(inspection_issue_for_io(environment, path, &err));
                return stats;
            }
        };

        for entry in entries {
            if cancellation.is_cancelled() {
                stats.complete = false;
                stats.skipped_entries += 1;
                if stats.incomplete_reason.is_none() {
                    stats.incomplete_reason =
                        Some(format!("Scan cancelled while measuring {}", path.display()));
                    stats.inspection_issue = Some(ScanGapKind::Cancelled);
                }
                break;
            }
            let ent = match entry {
                Ok(e) => e,
                Err(err) => {
                    stats.complete = false;
                    stats.skipped_entries += 1;
                    if stats.incomplete_reason.is_none() {
                        stats.incomplete_reason = Some(format!(
                            "Failed to read entry in {}: {}",
                            path.display(),
                            describe_inspection_error(environment, path, &err)
                        ));
                        stats.inspection_issue =
                            Some(inspection_issue_for_io(environment, path, &err));
                    }
                    continue;
                }
            };
            let child_path = ent.path();

            if policy.blacklist.is_blacklisted(&child_path) {
                stats.skipped_entries += 1;
                continue;
            }

            if policy.exclusions.is_excluded(&child_path) {
                stats.skipped_entries += 1;
                continue;
            }

            let sub_stats = Self::measure_tree_stats_profiled(
                context,
                &child_path,
                policy,
                current_depth + 1,
                stale_policy,
                phases,
            );
            if !sub_stats.complete {
                stats.complete = false;
                if stats.incomplete_reason.is_none() {
                    stats.incomplete_reason = sub_stats.incomplete_reason;
                    stats.inspection_issue = sub_stats.inspection_issue;
                }
            }
            stats.logical += sub_stats.logical;
            stats.allocated += sub_stats.allocated;
            stats.file_count += sub_stats.file_count;
            stats.stale_bytes += sub_stats.stale_bytes;
            stats.stale_file_count += sub_stats.stale_file_count;
            stats.skipped_entries += sub_stats.skipped_entries;
            if let Some(sub_mtime) = sub_stats.newest_mtime {
                stats.newest_mtime = Some(match stats.newest_mtime {
                    Some(existing) => existing.max(sub_mtime),
                    None => sub_mtime,
                });
            }
        }

        stats
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeStats {
    /// Bytes in files whose *own* age satisfies the stale-entry policy, when the
    /// caller supplied one. Zero when it did not.
    pub stale_bytes: u64,
    /// How many files that subset holds.
    pub stale_file_count: u64,
    pub logical: u64,
    pub allocated: u64,
    pub file_count: usize,
    pub newest_mtime: Option<SystemTime>,
    pub complete: bool,
    pub incomplete_reason: Option<String>,
    pub inspection_issue: Option<ScanGapKind>,
    /// Entries the walk did not account for: excluded, blacklisted, protected,
    /// unreadable, or beyond the depth limit.
    pub skipped_entries: u64,
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * KIB;
    const GIB: f64 = 1024.0 * MIB;
    const TIB: f64 = 1024.0 * GIB;

    let b = bytes as f64;
    let (val, unit) = if b >= TIB {
        (b / TIB, "TB")
    } else if b >= GIB {
        (b / GIB, "GB")
    } else if b >= MIB {
        (b / MIB, "MB")
    } else if b >= KIB {
        (b / KIB, "KB")
    } else {
        return format!("{bytes} B");
    };

    let formatted = format!("{val:.1}");
    let trimmed = formatted.strip_suffix(".0").unwrap_or(&formatted);
    format!("{trimmed} {unit}")
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod aged_pool_tests;
