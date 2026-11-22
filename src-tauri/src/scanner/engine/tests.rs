use super::{
    add_scan_gap, aggregate_quality, apply_verified_cache_owner_state,
    route_exact_owner_observations, scan_gap_kind, CategoryAccumulator, ScanEngine,
};
use crate::cache_providers::{CacheProviderFailure, CacheProviderScan, CacheProviderScanner};
use crate::cleaner::LifecycleProviderRegistry;
use crate::models::{
    Category, CleanStrategy, FileSize, NeverCancelled, ObservationQuality, PathIdentity, RiskTier,
    ScanEvent, ScanGapKind, ScanItem, Signature,
};
use crate::scanner::relationship::{same_directory_entry, unit_relationship};
use crate::scanner::{ScanLimits, TraversalCounters};
use crate::signatures::SignatureRegistry;
use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct FixedCacheProviders {
    result: CacheProviderScan,
    excluded: Mutex<Vec<Vec<String>>>,
}

impl FixedCacheProviders {
    fn new(result: CacheProviderScan) -> Self {
        Self {
            result,
            excluded: Mutex::new(Vec::new()),
        }
    }
}

impl CacheProviderScanner for FixedCacheProviders {
    fn scan_items(
        &self,
        _registry: &SignatureRegistry,
        excluded_signatures: &[String],
        _environment: &PlatformEnvironment,
        _cancellation: &dyn crate::models::CancellationProbe,
        _limits: ScanLimits,
        _counters: &TraversalCounters,
        _progress: &dyn crate::scanner::RootProgressSink,
    ) -> CacheProviderScan {
        self.excluded
            .lock()
            .unwrap()
            .push(excluded_signatures.to_vec());
        self.result.clone()
    }
}

/// A scan environment with no tools and a synthetic profile, so a scan in
/// a test only reports the fixture signatures it was given.
fn scan_environment() -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(if PathFlavor::current().is_windows() {
            r"Z:\NeatiFixtureHome"
        } else {
            "/neati-fixture-home"
        })
        .with_missing_tool("docker")
        .with_missing_tool("npm")
        .with_missing_tool("pnpm")
        .with_missing_tool("uv")
}

#[test]
fn scan_gaps_depend_on_typed_facts_not_diagnostic_language() {
    let mut item = ScanItem::mock(
        "gap",
        "gap",
        "Fixture",
        Category::System,
        RiskTier::Safe,
        "/fixture/Library/Mail",
        FileSize::default(),
        0,
    );
    item.quality = ObservationQuality::Unavailable;
    item.rederive_disposition();
    for kind in [
        ScanGapKind::PermissionDenied,
        ScanGapKind::FullDiskAccess,
        ScanGapKind::ToolMissing,
        ScanGapKind::UnsupportedAdapter,
        ScanGapKind::Cancelled,
        ScanGapKind::DepthLimit,
        ScanGapKind::IoError,
    ] {
        item.inspection_issue = Some(kind);
        for message in [
            "알 수 없는 오류",
            "cancel: permission denied, depth limit",
            "",
        ] {
            item.incomplete_reason = Some(message.into());
            assert_eq!(scan_gap_kind(&item), Some(kind));
            assert!(!item.allows_cleanup());
        }
    }
    item.inspection_issue = None;
    assert_eq!(scan_gap_kind(&item), Some(ScanGapKind::Unknown));
    item.quality = ObservationQuality::Fresh;
    assert_eq!(scan_gap_kind(&item), None);
}

#[test]
fn verified_cache_process_state_is_recorded_before_selection() {
    let mut item = ScanItem::mock(
        "browser.cache",
        "browser.cache",
        "Browser cache",
        Category::System,
        RiskTier::Rebuild,
        "/fixture/Cache",
        FileSize::new(4096, Some(4096)),
        1,
    );
    apply_verified_cache_owner_state(&mut item, Some(true));
    assert!(item.owner_running);
    assert!(!item.is_selected);

    let mut unknown = item.clone();
    apply_verified_cache_owner_state(&mut unknown, None);
    assert_eq!(unknown.quality, ObservationQuality::Unavailable);
    assert_eq!(unknown.cleanable_bytes(), 0);
    assert_eq!(unknown.observed_bytes(), 4096);
}

#[test]
fn scan_gap_counts_merge_without_requiring_frontend_prose_parsing() {
    let mut gaps = Vec::new();
    add_scan_gap(&mut gaps, ScanGapKind::DepthLimit, 1);
    add_scan_gap(&mut gaps, ScanGapKind::DepthLimit, 2);
    add_scan_gap(&mut gaps, ScanGapKind::PermissionDenied, 1);
    assert_eq!(
        gaps,
        vec![
            crate::models::ScanGap {
                kind: ScanGapKind::DepthLimit,
                count: 3,
            },
            crate::models::ScanGap {
                kind: ScanGapKind::PermissionDenied,
                count: 1,
            },
        ]
    );
}

#[test]
fn provider_exclusions_reach_the_scanner_and_empty_cancellation_stays_partial() {
    let cache_providers = FixedCacheProviders::new(CacheProviderScan {
        cancelled: true,
        ..CacheProviderScan::default()
    });
    let registry = SignatureRegistry::new();
    let lifecycle = LifecycleProviderRegistry::new(Vec::new());
    let owners = crate::cleaner::OwnerProviderRegistry::new(Vec::new());
    let excluded = vec!["dev.uv.cache".to_string()];
    let result = ScanEngine::scan_with_cache_providers(
        &registry,
        &lifecycle,
        &owners,
        Some(&[Category::Developer]),
        &excluded,
        false,
        &scan_environment(),
        &NeverCancelled,
        &cache_providers,
        |_| {},
    );

    assert_eq!(
        cache_providers.excluded.lock().unwrap().as_slice(),
        &[excluded]
    );
    assert!(result.cancelled);
    assert_eq!(result.quality, ObservationQuality::Partial);
    assert!(result.categories[0].items.is_empty());
    assert!(result
        .gaps
        .iter()
        .any(|gap| gap.kind == ScanGapKind::Cancelled));
}

#[test]
fn provider_failure_without_a_row_is_a_typed_scan_level_gap() {
    let cache_providers = FixedCacheProviders::new(CacheProviderScan {
        failures: vec![CacheProviderFailure {
            kind: ScanGapKind::IoError,
            reason: "uv cache discovery timed out".to_string(),
        }],
        ..CacheProviderScan::default()
    });
    let result = ScanEngine::scan_with_cache_providers(
        &SignatureRegistry::new(),
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &scan_environment(),
        &NeverCancelled,
        &cache_providers,
        |_| {},
    );

    assert!(!result.cancelled);
    assert_eq!(result.quality, ObservationQuality::Partial);
    assert!(result.categories[0].items.is_empty());
    assert_eq!(result.categories[0].quality, ObservationQuality::Partial);
    assert_eq!(
        result
            .gaps
            .iter()
            .find(|gap| gap.kind == ScanGapKind::IoError)
            .map(|gap| gap.count),
        Some(1)
    );
    assert_eq!(
        result.incomplete_reasons,
        vec!["uv cache discovery timed out".to_string()]
    );
}

fn signature(
    id: &str,
    name: &str,
    category: Category,
    path: &Path,
    exclusions: Vec<String>,
    min_age_days: Option<u32>,
) -> Signature {
    Signature {
        deletion_disposition: None,
        id: id.to_string(),
        name: name.to_string(),
        category,
        family: Default::default(),
        risk: RiskTier::Safe,
        strategy: CleanStrategy::DeleteContents,
        paths: vec![path.to_string_lossy().into_owned()],
        exclusions,
        description: "test-only signature".into(),
        min_age_days,
        include_prefixes: vec![],
        exclude_prefixes: vec![],
        intensive_only: false,
        platforms: vec![],
        discovery: Default::default(),
        unit: None,
        owner: String::new(),
        priority: 0,
        fail_if_running: Vec::new(),
        provider: String::new(),
        provider_id: None,
        artifact_kind: Default::default(),
        consequence: String::new(),
    }
}

/// A lifecycle provider's candidate reaches the scan through the category
/// its catalog entry declares: discovered with the provider's own
/// measurement, offered for explicit selection, and never pre-selected —
/// while nothing about the underlying store becomes generically deletable.
#[test]
fn a_lifecycle_provider_candidate_is_discovered_and_never_pre_selected() {
    use crate::cleaner::providers::test_support::StatedProvider;
    use crate::models::{CleanupUnitKind, PlatformKind};

    let mut registry = SignatureRegistry::new();
    let mut entry = signature(
        "test.stated.store",
        "Stated Store",
        Category::System,
        Path::new("/unused"),
        vec![],
        None,
    );
    entry.risk = RiskTier::Manual;
    entry.strategy = CleanStrategy::LifecycleProvider;
    entry.provider_id = Some("test.stated".to_string());
    entry.platforms = vec![PlatformKind::current()];
    entry.paths = Vec::new();
    registry.register(entry);

    let providers =
        LifecycleProviderRegistry::new(vec![StatedProvider::holding(6_000, 3).shared()]);
    let result = ScanEngine::scan(
        &registry,
        &providers,
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::System]),
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let items = &result.categories[0].items;
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.id, "test.stated.store");
    assert_eq!(item.unit.kind, CleanupUnitKind::ProviderAction);
    assert_eq!(item.size.observed_bytes(), 6_000);
    assert_eq!(item.file_count, 3);
    assert!(item.disposition.is_cleanable());
    assert!(
        !item.is_selected,
        "a scan never pre-selects a provider action"
    );
    assert_eq!(result.total_bytes, 6_000);
    assert_eq!(
        result.cleanable_bytes, 6_000,
        "the bytes are cleanable, through the provider that reported them"
    );
}

/// Three entries a measurement must not account for: the named exclusion,
/// the protected `.git` directory, and a signed app bundle.
fn cache_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("keep")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("Tool.app/Contents")).unwrap();
    std::fs::write(root.join("data.bin"), vec![1u8; 4_096]).unwrap();
    std::fs::write(root.join("keep/payload.bin"), vec![2u8; 2_048]).unwrap();
    std::fs::write(root.join(".git/objects"), vec![3u8; 1_024]).unwrap();
    std::fs::write(root.join("Tool.app/Contents/payload"), vec![4u8; 512]).unwrap();
}

/// Two signatures that name the same location are one unit: the bytes are
/// counted once, and the suppressed duplicate is reported rather than
/// dropped silently.
#[test]
fn overlapping_signatures_count_one_unit_and_report_the_suppression() {
    let fixture = tempfile::tempdir().unwrap();
    let shared = fixture.path().join("shared-cache");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("data.bin"), vec![1u8; 8_192]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.overlap.specific",
        "Specific cache",
        Category::Developer,
        &shared,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.overlap.broad",
        "Broad cache",
        Category::Developer,
        &shared,
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let developer = result
        .categories
        .iter()
        .find(|category| category.category == Category::Developer)
        .expect("the developer category is scanned");
    assert_eq!(
        developer.items.len(),
        1,
        "one location is one unit however many signatures name it"
    );
    assert_eq!(developer.total_bytes, 8_192);
    assert_eq!(developer.suppressed_duplicate_count, 1);
    assert_eq!(developer.suppressed_duplicate_bytes, 8_192);
    assert_eq!(result.suppressed_duplicate_count, 1);
    assert_eq!(result.suppressed_duplicate_bytes, 8_192);
    let provenance: Vec<&str> = developer.items[0]
        .overlaps
        .iter()
        .map(|overlap| overlap.signature_id.as_str())
        .collect();
    let suppressed = if developer.items[0].signature_id == "test.overlap.specific" {
        "test.overlap.broad"
    } else {
        "test.overlap.specific"
    };
    assert_eq!(
        provenance,
        vec![suppressed],
        "the rule that did not count the bytes is still named on the unit that did"
    );
}

#[test]
fn observation_routing_requires_registered_owner_contracts_and_existing_entry_identity() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("provider-cache");
    std::fs::create_dir(&path).unwrap();
    let mut observation_signature = signature(
        "test.observation",
        "Observation",
        Category::Developer,
        &path,
        vec![],
        None,
    );
    observation_signature.strategy = CleanStrategy::Manual;
    observation_signature.risk = RiskTier::Manual;
    observation_signature.owner = "Cache owner".into();
    observation_signature.provider = "Cache owner".into();
    let mut owner_signature = observation_signature.clone();
    owner_signature.id = "test.owner".into();
    owner_signature.strategy = CleanStrategy::OwnerProvider;
    owner_signature.risk = RiskTier::Rebuild;
    owner_signature.paths.clear();
    owner_signature.provider_id = Some("test.owner-operation".into());

    for boundary in [
        "verified",
        "unregistered owner",
        "unregistered observation",
        "missing provider id",
        "generic filesystem contract",
        "non-manual observation",
        "missing entry",
    ] {
        let mut observation = ScanItem::mock(
            "observation",
            &observation_signature.id,
            "Observation",
            Category::Developer,
            RiskTier::Manual,
            path.to_string_lossy(),
            FileSize::new(1_000, Some(1_000)),
            1,
        );
        observation.cache_metadata = observation_signature.cache_metadata();
        observation.ownership = observation_signature.ownership();
        observation.rederive_disposition();
        let mut owner = ScanItem::mock(
            "opaque-owner-unit",
            &owner_signature.id,
            "Owner operation",
            Category::Developer,
            RiskTier::Rebuild,
            path.to_string_lossy(),
            FileSize::new(1_000, Some(1_000)),
            1,
        );
        owner.cache_metadata = owner_signature.cache_metadata();
        owner.ownership = owner_signature.ownership();
        owner.unit.kind = crate::models::CleanupUnitKind::ProviderAction;
        owner.lifecycle_provider_action = true;
        owner.requires_confirmation = true;
        owner.rederive_disposition();
        let mut observation_contract = observation_signature.clone();
        let mut owner_contract = owner_signature.clone();
        match boundary {
            "missing provider id" => owner_contract.provider_id = None,
            "generic filesystem contract" => {
                owner_contract.strategy = CleanStrategy::DeleteDirectory
            }
            "non-manual observation" => {
                observation_contract.strategy = CleanStrategy::DeleteDirectory
            }
            "missing entry" => {
                let missing = path.join("absent").to_string_lossy().into_owned();
                for item in [&mut observation, &mut owner] {
                    item.path = missing.clone();
                    item.unit.path = missing.clone();
                    item.unit.root = missing.clone();
                }
            }
            _ => {}
        }
        let mut registry = SignatureRegistry::new();
        if boundary != "unregistered observation" {
            registry.register(observation_contract);
        }
        if boundary != "unregistered owner" {
            registry.register(owner_contract);
        }
        let original = vec![observation, owner.clone()];
        let mut accumulator = CategoryAccumulator::new();
        for item in &original {
            accumulator.push(item.clone());
        }
        let mut categories = vec![accumulator.finalize(Category::Developer, None)];
        route_exact_owner_observations(
            &mut categories,
            &registry,
            &std::cell::RefCell::new(crate::scanner::relationship::ScanRelationships::default()),
        );
        if boundary == "verified" {
            assert_eq!(categories[0].items, [owner]);
            assert_eq!(categories[0].total_bytes, 1_000);
        } else {
            assert_eq!(categories[0].items, original, "{boundary}");
            assert_eq!(categories[0].suppressed_duplicate_count, 0, "{boundary}");
        }
    }
}

#[test]
fn exact_provider_and_filesystem_duplicates_retain_provider_authority_in_either_order() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("provider-cache");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("data.bin"), vec![1u8; 4_096]).unwrap();

    let filesystem = ScanItem::mock(
        "filesystem",
        "test.filesystem",
        "Filesystem cache",
        Category::Developer,
        RiskTier::Safe,
        path.to_string_lossy(),
        FileSize::new(4_096, Some(4_096)),
        1,
    );
    let mut provider = ScanItem::mock(
        "provider",
        "test.provider",
        "Provider cache",
        Category::Developer,
        RiskTier::Rebuild,
        path.to_string_lossy(),
        FileSize::new(4_096, Some(4_096)),
        1,
    );
    provider.unit.kind = crate::models::CleanupUnitKind::ProviderAction;

    for items in [
        vec![filesystem.clone(), provider.clone()],
        vec![provider.clone(), filesystem.clone()],
    ] {
        let mut accumulator = CategoryAccumulator::new();
        for item in items {
            accumulator.push(item);
        }
        let mut categories = vec![accumulator.finalize(Category::Developer, None)];
        crate::models::resolve_unit_overlaps(&mut categories, &[], PathIdentity::CaseSensitive);
        assert_eq!(categories[0].items.len(), 1);
        assert_eq!(categories[0].items[0].signature_id, "test.provider");
    }
}

/// A unit inside a broader one is that unit's bytes: the scan counts them
/// once, keeps the narrower rule as provenance, and states how much was
/// folded away.
#[test]
fn a_child_unit_is_accounted_by_the_broader_unit_that_contains_it() {
    let fixture = tempfile::tempdir().unwrap();
    let parent = fixture.path().join("Cache");
    let child = parent.join("Service Worker");
    std::fs::create_dir_all(&child).unwrap();
    // Both files are whole allocation blocks, so the expected totals state
    // the same thing on every filesystem a scan runs against.
    std::fs::write(parent.join("data.bin"), vec![1u8; 8_192]).unwrap();
    std::fs::write(child.join("worker.bin"), vec![2u8; 8_192]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.broad",
        "Broad cache",
        Category::Developer,
        &parent,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.narrow",
        "Service worker cache",
        Category::Developer,
        &child,
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let developer = result
        .categories
        .iter()
        .find(|category| category.category == Category::Developer)
        .expect("the developer category is scanned");
    assert_eq!(developer.items.len(), 1, "one location is one unit");
    assert_eq!(developer.items[0].path, parent.to_string_lossy());
    assert_eq!(
        developer.total_bytes, 16_384,
        "the broader unit reports the bytes its subtree holds, once"
    );
    assert_eq!(developer.suppressed_overlap_count, 1);
    assert_eq!(developer.suppressed_overlap_bytes, 8_192);
    assert_eq!(result.suppressed_overlap_count, 1);
    assert_eq!(result.suppressed_overlap_bytes, 8_192);
    assert_eq!(
        result.total_bytes, 16_384,
        "no total counts the contained unit a second time"
    );
    let provenance: Vec<&str> = developer.items[0]
        .overlaps
        .iter()
        .map(|overlap| overlap.signature_id.as_str())
        .collect();
    assert_eq!(provenance, vec!["test.narrow"]);
}

/// The strictest rule that described a location decides whether it may be
/// cleaned, even when the broader rule found it first.
#[test]
fn a_stricter_child_rule_withholds_the_overlapping_unit() {
    let fixture = tempfile::tempdir().unwrap();
    let parent = fixture.path().join("Cache");
    let child = parent.join("local-state");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(parent.join("data.bin"), vec![1u8; 8_192]).unwrap();
    std::fs::write(child.join("state.bin"), vec![2u8; 8_192]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.broad",
        "Broad cache",
        Category::Developer,
        &parent,
        vec![],
        None,
    ));
    let mut manual = signature(
        "test.guarded",
        "Application state",
        Category::Developer,
        &child,
        vec![],
        None,
    );
    manual.risk = RiskTier::Manual;
    registry.register(manual);

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let developer = result
        .categories
        .iter()
        .find(|category| category.category == Category::Developer)
        .expect("the developer category is scanned");
    assert_eq!(developer.items.len(), 2);
    let surviving = developer
        .items
        .iter()
        .find(|item| item.signature_id == "test.broad")
        .expect("the broader observation remains visible but blocked");
    assert_eq!(
        surviving.disposition.eligibility,
        crate::models::CleanupEligibility::Blocked,
        "the stricter rule keeps the bytes out of the automatic total"
    );
    assert!(!surviving.is_selected);
    assert_eq!(developer.cleanable_bytes, 0);
    assert_eq!(
        developer.total_bytes, 24_576,
        "an unresolved authority conflict preserves both observations instead of claiming coverage"
    );
}

#[test]
fn differing_process_guards_prevent_containment_from_merging_authority() {
    let fixture = tempfile::tempdir().unwrap();
    let parent = fixture.path().join("Cache");
    let child = parent.join("guarded");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(parent.join("data.bin"), vec![1u8; 8_192]).unwrap();
    std::fs::write(child.join("guarded.bin"), vec![2u8; 8_192]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.guard.parent",
        "Parent cache",
        Category::Developer,
        &parent,
        vec![],
        None,
    ));
    let mut guarded = signature(
        "test.guard.child",
        "Guarded cache",
        Category::Developer,
        &child,
        vec![],
        None,
    );
    guarded.fail_if_running = vec!["guarded-tool".into()];
    registry.register(guarded);

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );
    let developer = result
        .categories
        .iter()
        .find(|category| category.category == Category::Developer)
        .expect("the developer category is scanned");

    assert_eq!(developer.items.len(), 2);
    let parent = developer
        .items
        .iter()
        .find(|item| item.signature_id == "test.guard.parent")
        .expect("the broader observation remains visible");
    assert_eq!(
        parent.disposition.eligibility,
        crate::models::CleanupEligibility::Blocked
    );
    assert!(parent
        .overlaps
        .iter()
        .any(|overlap| overlap.authority_conflict));
}

/// When no filesystem object exists, text is a conservative case-sensitive
/// fallback rather than an OS-wide guess about the volume.
#[test]
fn unresolved_case_variant_units_remain_distinct() {
    let upper = ScanItem::mock(
        "case-upper",
        "test.case",
        "Cache",
        Category::Developer,
        RiskTier::Safe,
        r"C:\Users\tester\Cache",
        FileSize::new(1_024, Some(1_024)),
        1,
    );
    let lower = ScanItem::mock(
        "case-lower",
        "test.case",
        "Cache",
        Category::Developer,
        RiskTier::Safe,
        r"c:\users\tester\cache",
        FileSize::new(1_024, Some(1_024)),
        1,
    );

    let mut accumulator = CategoryAccumulator::new();
    for item in [upper, lower] {
        accumulator.push(item);
    }
    let mut categories = vec![accumulator.finalize(Category::Developer, None)];
    crate::models::resolve_unit_overlaps_with(
        &mut categories,
        &[],
        PathIdentity::CaseSensitive,
        |candidate, container| Some(unit_relationship(candidate, container)),
    );
    assert_eq!(categories[0].items.len(), 2);
    assert_eq!(categories[0].suppressed_duplicate_count, 0);
}

/// Two scans of one filesystem state state the same totals in the same
/// order: nothing about the accounting may depend on map iteration or on
/// the order a directory listing happened to answer in.
#[test]
fn repeated_scans_state_the_same_totals_in_the_same_order() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("Cache");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(root.join("data.bin"), vec![1u8; 8_192]).unwrap();
    std::fs::write(nested.join("state.bin"), vec![2u8; 8_192]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.parent",
        "Parent cache",
        Category::Developer,
        &root,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.child",
        "Nested cache",
        Category::Developer,
        &nested,
        vec![],
        None,
    ));

    let scan = || {
        ScanEngine::scan(
            &registry,
            &LifecycleProviderRegistry::new(Vec::new()),
            &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
            None,
            &[],
            false,
            &scan_environment(),
            &crate::models::NeverCancelled,
            |_| {},
        )
    };
    let first = scan();
    let second = scan();

    assert_eq!(first.total_bytes, second.total_bytes);
    assert_eq!(first.cleanable_bytes, second.cleanable_bytes);
    assert_eq!(
        first.suppressed_overlap_count,
        second.suppressed_overlap_count
    );
    assert_eq!(
        first.suppressed_overlap_bytes,
        second.suppressed_overlap_bytes
    );
    assert_eq!(first.eligibility, second.eligibility);
    let describe = |result: &crate::models::ScanResult| -> Vec<(String, u64, u64)> {
        result
            .categories
            .iter()
            .flat_map(|category| {
                category.items.iter().map(|item| {
                    (
                        item.id.clone(),
                        item.observed_bytes(),
                        item.cleanable_bytes(),
                    )
                })
            })
            .collect()
    };
    assert_eq!(describe(&first), describe(&second));
}

#[test]
fn distinct_hardlinks_remain_distinct_cleanup_units() {
    let fixture = tempfile::tempdir().unwrap();
    let first = fixture.path().join("Pip");
    let alias = fixture.path().join("pip-alias");
    std::fs::write(&first, b"shared bytes").unwrap();
    std::fs::hard_link(&first, &alias).unwrap();

    let mut accumulator = CategoryAccumulator::new();
    for (index, path) in [&first, &alias].iter().enumerate() {
        let mut item = ScanItem::mock(
            format!("duplicate-{index}"),
            "test.duplicate",
            "Cache",
            Category::Developer,
            RiskTier::Safe,
            path.to_string_lossy().into_owned(),
            FileSize::new(12, Some(12)),
            1,
        );
        item.entry_kind = crate::models::EntryKind::File;
        accumulator.push(item);
    }
    let mut categories = vec![accumulator.finalize(Category::Developer, None)];
    crate::models::resolve_unit_overlaps_with(
        &mut categories,
        &[],
        PathIdentity::CaseSensitive,
        |candidate, container| Some(unit_relationship(candidate, container)),
    );
    assert_eq!(categories[0].items.len(), 2);
    assert_eq!(categories[0].suppressed_duplicate_count, 0);
}

#[test]
fn directory_entry_identity_follows_the_actual_volume() {
    let fixture = tempfile::tempdir().unwrap();
    let stored = fixture.path().join("Pip");
    let alternate = fixture.path().join("pip");
    std::fs::create_dir(&stored).unwrap();
    let entity = crate::safety::ToctouGuard::capture(&stored)
        .expect("directory identity")
        .entity();

    if alternate.exists() {
        assert!(same_directory_entry(&stored, &alternate, entity));
    } else {
        std::fs::create_dir(&alternate).unwrap();
        assert!(!same_directory_entry(&stored, &alternate, entity));
    }

    let mut accumulator = CategoryAccumulator::new();
    for (index, path) in [&stored, &alternate].iter().enumerate() {
        let item = ScanItem::mock(
            format!("case-{index}"),
            "test.case",
            "Cache",
            Category::Developer,
            RiskTier::Safe,
            path.to_string_lossy().into_owned(),
            FileSize::new(12, Some(12)),
            1,
        );
        accumulator.push(item);
    }
    let mut categories = vec![accumulator.finalize(Category::Developer, None)];
    crate::models::resolve_unit_overlaps_with(
        &mut categories,
        &[],
        PathIdentity::CaseSensitive,
        |candidate, container| Some(unit_relationship(candidate, container)),
    );
    let expected_units = if entity.same_entity(
        crate::safety::ToctouGuard::capture(&alternate)
            .expect("alternate identity")
            .entity(),
    ) {
        1
    } else {
        2
    };
    assert_eq!(categories[0].items.len(), expected_units);
}

/// The breakdown explains the total: every observed byte lands in exactly
/// one eligibility bucket, and only the cleanable states carry cleanable
/// bytes.
#[test]
fn the_scan_reports_an_eligibility_breakdown_that_sums_to_the_total() {
    let fixture = tempfile::tempdir().unwrap();
    let cleanable = fixture.path().join("cleanable-cache");
    let blocked = fixture.path().join("blocked-cache");
    std::fs::create_dir_all(&cleanable).unwrap();
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(cleanable.join("data.bin"), vec![1u8; 4_096]).unwrap();
    std::fs::write(blocked.join("session.sqlite"), vec![2u8; 2_048]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.breakdown.cleanable",
        "Cleanable cache",
        Category::Developer,
        &cleanable,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.breakdown.blocked",
        "Structured state",
        Category::Developer,
        &blocked.join("session.sqlite"),
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let developer = result
        .categories
        .iter()
        .find(|category| category.category == Category::Developer)
        .expect("the developer category is scanned");

    let bucket_total: u64 = developer
        .eligibility
        .buckets
        .iter()
        .map(|bucket| bucket.observed_bytes)
        .sum();
    assert_eq!(bucket_total, developer.total_bytes);

    let auto = crate::models::CleanupEligibility::AutoCleanable;
    let blocked_state = crate::models::CleanupEligibility::Blocked;
    // The buckets carry exactly what the items reported: an allocated size
    // is a filesystem fact, so the assertion is against the items rather
    // than against a literal the block size could change.
    let cleanable_item = developer
        .items
        .iter()
        .find(|item| item.disposition.eligibility == auto)
        .expect("the cleanable cache is auto-cleanable");
    let blocked_item = developer
        .items
        .iter()
        .find(|item| item.disposition.eligibility == blocked_state)
        .expect("the structured file is blocked");
    assert_eq!(
        developer.eligibility_observed_bytes(auto),
        cleanable_item.observed_bytes()
    );
    assert_eq!(
        developer.eligibility_observed_bytes(blocked_state),
        blocked_item.observed_bytes()
    );
    assert_eq!(
        blocked_item.structured_state,
        Some(crate::models::StructuredStateKind::Database)
    );
    assert_eq!(cleanable_item.structured_state, None);
    assert_eq!(
        developer.eligibility.cleanable_bytes(blocked_state),
        0,
        "a blocked state never reports cleanable bytes"
    );
    assert_eq!(developer.eligibility_items(auto), 1);
    assert_eq!(developer.eligibility_items(blocked_state), 1);
    assert_eq!(
        result.eligibility_observed_bytes(auto),
        cleanable_item.observed_bytes()
    );
    assert_eq!(
        result.eligibility_observed_bytes(blocked_state),
        blocked_item.observed_bytes()
    );
}

/// The scan reports how much of each tree it could not measure, so a total
/// that skipped entries is never presented as a complete one.
#[test]
fn scan_counts_skipped_entries_and_incomplete_items() {
    let fixture = tempfile::tempdir().unwrap();
    let plain_root = fixture.path().join("plain-cache");
    let aged_root = fixture.path().join("aged-cache");
    cache_fixture(&plain_root);
    cache_fixture(&aged_root.join("candidate.cache"));

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.counts.plain",
        "Plain cache",
        Category::Developer,
        &plain_root,
        vec!["keep".to_string()],
        None,
    ));
    registry.register(signature(
        "test.counts.aged",
        "Aged cache",
        Category::System,
        &aged_root,
        vec!["keep".to_string()],
        Some(0),
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let developer = result
        .categories
        .iter()
        .find(|category| category.category == Category::Developer)
        .expect("the developer category is scanned");
    let plain = developer
        .items
        .iter()
        .find(|item| item.signature_id == "test.counts.plain")
        .expect("the plain cache is retained");
    assert_eq!(plain.quality, ObservationQuality::Partial);
    assert_eq!(
        plain.skipped_entry_count, 3,
        "the exclusion, `.git`, and the app bundle were not measured"
    );
    assert!(plain.size.reclaimable() > 0);
    assert!(
        !plain.is_selected,
        "a partial observation is never opted in"
    );

    let system = result
        .categories
        .iter()
        .find(|category| category.category == Category::System)
        .expect("the system category is scanned");
    let aged = system
        .items
        .iter()
        .find(|item| item.signature_id == "test.counts.aged")
        .expect("the incomplete aged candidate is retained for observability");
    assert_eq!(aged.quality, ObservationQuality::Unavailable);
    assert_eq!(aged.skipped_entry_count, 3);

    assert_eq!(result.skipped_entry_count, 6);
    assert_eq!(
        result.incomplete_item_count, 2,
        "both partial and unavailable observations are incomplete, regardless \
             of whether the partial item remains reviewable for cleanup"
    );
}

/// The per-category counters are what the category cards and the freshness
/// notice render, so they have to explain the scan-level counters.
#[test]
fn category_counts_sum_to_the_scan_counts() {
    let fixture = tempfile::tempdir().unwrap();
    let developer_root = fixture.path().join("dev-cache");
    let system_root = fixture.path().join("system-cache");
    for root in [&developer_root, &system_root] {
        std::fs::create_dir(root).unwrap();
        std::fs::write(root.join("data.bin"), vec![1u8; 4_096]).unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/objects"), vec![2u8; 1_024]).unwrap();
    }

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.sum.developer",
        "Developer cache",
        Category::Developer,
        &developer_root,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.sum.system",
        "System cache",
        Category::System,
        &system_root,
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    let category_skipped: u64 = result
        .categories
        .iter()
        .map(|category| category.skipped_entry_count)
        .sum();
    let category_incomplete: u64 = result
        .categories
        .iter()
        .map(|category| category.incomplete_item_count)
        .sum();
    assert_eq!(result.skipped_entry_count, category_skipped);
    assert_eq!(result.incomplete_item_count, category_incomplete);
    assert_eq!(
        result.skipped_entry_count, 2,
        "each tree protects its own `.git` directory"
    );
    assert_eq!(
        result.incomplete_item_count, 0,
        "a deliberate exclusion does not make the observation incomplete"
    );
}

#[test]
fn observation_quality_aggregation_distinguishes_total_and_partial_failure() {
    assert_eq!(aggregate_quality([]), ObservationQuality::Fresh);
    assert_eq!(
        aggregate_quality([ObservationQuality::Fresh, ObservationQuality::Fresh]),
        ObservationQuality::Fresh
    );
    assert_eq!(
        aggregate_quality([
            ObservationQuality::Unavailable,
            ObservationQuality::Unavailable,
        ]),
        ObservationQuality::Unavailable
    );
    assert_eq!(
        aggregate_quality([ObservationQuality::Fresh, ObservationQuality::Unavailable]),
        ObservationQuality::Partial
    );
}

/// Observed and cleanable totals remain distinct, so an uninspectable item
/// stays visible without becoming actionable or a second error surface.
#[test]
fn category_totals_separate_observed_and_cleanable_bytes_and_emit_no_error_event() {
    let fixture = tempfile::tempdir().unwrap();
    let plain_root = fixture.path().join("plain-cache");
    let aged_root = fixture.path().join("aged-cache");
    std::fs::create_dir(&plain_root).unwrap();
    std::fs::write(plain_root.join("data.bin"), vec![1u8; 4_096]).unwrap();
    cache_fixture(&aged_root.join("candidate.cache"));

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.consistency.plain",
        "Plain cache",
        Category::System,
        &plain_root,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.consistency.aged",
        "Aged cache",
        Category::System,
        &aged_root,
        vec![],
        Some(0),
    ));

    let mut events = Vec::new();
    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::System]),
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |event| events.push(event),
    );

    let system = result
        .categories
        .iter()
        .find(|category| category.category == Category::System)
        .expect("the system category is scanned");
    let cleanable = system
        .items
        .iter()
        .find(|item| item.signature_id == "test.consistency.plain")
        .expect("the cleanable cache is retained");
    let blocked = system
        .items
        .iter()
        .find(|item| item.signature_id == "test.consistency.aged")
        .expect("the uninspectable aged candidate is retained for observability");

    assert!(
        blocked.size.observed_bytes() > 0,
        "the blocked item still reports what it could measure"
    );
    assert!(!blocked.allows_cleanup());
    assert_eq!(blocked.cleanable_bytes(), 0);
    assert_eq!(
        blocked.disposition.eligibility,
        crate::models::CleanupEligibility::Blocked
    );

    assert_eq!(
        system.total_bytes,
        cleanable.size.observed_bytes() + blocked.size.observed_bytes(),
        "detected bytes include retained blocked observations"
    );
    assert_eq!(system.cleanable_bytes, cleanable.size.observed_bytes());
    assert_eq!(system.safe_bytes, cleanable.size.observed_bytes());
    assert_eq!(system.rebuild_bytes, 0);
    assert_eq!(system.manual_bytes, 0);
    assert_eq!(system.incomplete_item_count, 1);

    let kinds: Vec<&str> = events
        .iter()
        .map(|event| match event {
            ScanEvent::RootStarted { .. } => "root_started",
            ScanEvent::Started { .. } => "started",
            ScanEvent::CategoryStarted { .. } => "category_started",
            ScanEvent::ItemFound { .. } => "item_found",
            ScanEvent::CategoryFinished { .. } => "category_finished",
            ScanEvent::Finished { .. } => "finished",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "started",
            "category_started",
            // Each root is named before it is read, which is what lets the
            // interface show where a long scan is.
            "root_started",
            "item_found",
            "root_started",
            "item_found",
            "category_finished",
            "finished",
        ],
        "an item-level measurement gap is never a destructive scan event"
    );
}

/// A subtree the walk cannot read is reported rather than silently
/// absent: what it could not measure stays visible as a skipped entry with
/// a reason, and the scan does not claim a complete observation.
#[cfg(unix)]
#[test]
fn an_unreadable_subtree_is_reported_rather_than_silently_missing() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("partial-cache");
    std::fs::create_dir_all(root.join("open")).unwrap();
    std::fs::write(root.join("open/data.bin"), vec![1u8; 4_096]).unwrap();
    let closed = root.join("closed");
    std::fs::create_dir_all(closed.join("inner")).unwrap();
    std::fs::write(closed.join("inner/hidden.bin"), vec![2u8; 4_096]).unwrap();
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.partial",
        "Partly readable cache",
        Category::Developer,
        &root,
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    // Restore before the fixture is dropped so the temporary tree can be
    // removed with it.
    let _ = std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755));

    assert!(
        result.skipped_entry_count >= 1,
        "the unreadable directory is counted, not dropped: {result:?}"
    );
    assert_eq!(
        result.quality,
        ObservationQuality::Partial,
        "a scan that could not read part of a tree says so"
    );
    assert!(
        !result.gaps.is_empty(),
        "a partial observation carries a typed gap instead of only prose"
    );
    assert!(
        !result.cancelled,
        "an unreadable subtree is not a cancellation"
    );
    let item = result.categories[0]
        .items
        .iter()
        .find(|item| item.signature_id == "test.partial")
        .expect("the partially readable root is still reported");
    assert!(
        item.incomplete_reason.is_some(),
        "the item states why it is incomplete: {item:?}"
    );
}

#[test]
fn cancellation_inside_the_final_signature_stops_before_the_next_root_and_sets_the_flag() {
    struct Probe {
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl crate::models::CancellationProbe for Probe {
        fn is_cancelled(&self) -> bool {
            self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    let fixture = tempfile::tempdir().unwrap();
    let first_root = fixture.path().join("first-root");
    let second_root = fixture.path().join("second-root");
    std::fs::create_dir_all(&first_root).unwrap();
    std::fs::create_dir_all(&second_root).unwrap();
    std::fs::write(first_root.join("data.bin"), vec![1u8; 128]).unwrap();
    std::fs::write(second_root.join("data.bin"), vec![2u8; 128]).unwrap();

    let mut sig = signature(
        "test.cancel.final-signature",
        "Final signature",
        Category::Developer,
        &first_root,
        vec![],
        None,
    );
    sig.paths = vec![
        first_root.to_string_lossy().into_owned(),
        second_root.to_string_lossy().into_owned(),
    ];

    let mut registry = SignatureRegistry::new();
    registry.register(sig);

    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let probe = Probe {
        cancelled: cancelled.clone(),
    };
    let mut roots = Vec::new();

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &scan_environment(),
        &probe,
        |event| {
            if let ScanEvent::RootStarted { root, .. } = event {
                roots.push(root);
                cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        },
    );

    assert!(
        result.cancelled,
        "a stop inside the final signature must survive into the final result"
    );
    assert_eq!(
        roots.len(),
        1,
        "once cancellation is observed, the signature must not start another root"
    );
}

#[test]
fn traversal_metrics_count_each_root_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("one-root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("one-file.bin"), vec![1u8; 128]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.metrics.single-root",
        "Single root",
        Category::Developer,
        &root,
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    assert_eq!(
        result.metrics.visited_entries, 2,
        "one directory root plus one file is two visited filesystem entries"
    );
    assert_eq!(result.metrics.directories_read, 1);
    assert_eq!(
        result
            .spans
            .iter()
            .filter(|span| span.source_id == "scan.overlap_resolution")
            .count(),
        1
    );
    let merged = ScanEngine::merge_slices(
        &registry,
        &scan_environment(),
        std::slice::from_ref(&result),
    )
    .expect("merged scan");
    assert_eq!(
        merged
            .spans
            .iter()
            .filter(|span| span.source_id == "scan.overlap_merge")
            .count(),
        1
    );
    assert_eq!(merged.total_bytes, result.total_bytes);
    assert_eq!(
        merged.metrics.visited_entries,
        result.metrics.visited_entries
    );
    assert!(merged.metrics.duration_ms >= result.metrics.duration_ms);
    assert!(result
        .spans
        .iter()
        .any(|span| span.source_id == "test.metrics.single-root"));
    assert!(result
        .spans
        .iter()
        .all(|span| !span.source_id.contains('/')));
}

#[test]
fn aged_traversal_metrics_count_each_entry_once() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("aged-root");
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(child.join("one-file.bin"), vec![1u8; 128]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.metrics.aged-root",
        "Aged root",
        Category::Developer,
        &root,
        vec![],
        Some(0),
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &scan_environment(),
        &crate::models::NeverCancelled,
        |_| {},
    );

    assert_eq!(
        result.metrics.visited_entries, 3,
        "one root, one child directory, and one file are three visited entries"
    );
    assert_eq!(result.metrics.directories_read, 2);
}

#[test]
fn scan_engine_honors_cancellation_probe() {
    struct AlwaysCancelled;
    impl crate::models::CancellationProbe for AlwaysCancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    let fixture = tempfile::tempdir().unwrap();
    let dev_root = fixture.path().join("dev-cache");
    std::fs::create_dir_all(&dev_root).unwrap();
    std::fs::write(dev_root.join("data.bin"), vec![1u8; 100]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.cancel",
        "Cancelled cache",
        Category::Developer,
        &dev_root,
        vec![],
        None,
    ));

    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        false,
        &scan_environment(),
        &AlwaysCancelled,
        |_| {},
    );

    assert_eq!(result.quality, ObservationQuality::Partial);
    assert!(
        result.cancelled,
        "a cancelled scan says so as a fact, not only in prose"
    );
    assert!(result
        .gaps
        .iter()
        .any(|gap| gap.kind == crate::models::ScanGapKind::Cancelled));
    assert!(result
        .incomplete_reasons
        .iter()
        .any(|reason| reason.contains("cancelled")));
}

/// A wide tree scans to the same result twice, and the run states what its
/// traversal did: entries visited, directories read, and the peak number of
/// directory tasks it kept outstanding, which never exceeds the stated
/// bound.
#[test]
fn a_wide_tree_scans_deterministically_within_its_stated_bound() {
    const DIRECTORIES: usize = 32;
    const FILES_PER_DIRECTORY: usize = 2;

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("wide-cache");
    for directory in 0..DIRECTORIES {
        let path = root.join(format!("namespace-{directory:03}"));
        std::fs::create_dir_all(&path).unwrap();
        for file in 0..FILES_PER_DIRECTORY {
            std::fs::write(path.join(format!("entry-{file}.bin")), vec![4u8; 2_048]).unwrap();
        }
    }

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.wide",
        "Wide cache",
        Category::Developer,
        &root,
        vec![],
        None,
    ));

    let providers = LifecycleProviderRegistry::new(Vec::new());
    let scan = || {
        ScanEngine::scan(
            &registry,
            &providers,
            &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
            Some(&[Category::Developer]),
            &[],
            false,
            &scan_environment(),
            &crate::models::NeverCancelled,
            |_| {},
        )
    };
    let first = scan();
    let second = scan();

    let summary = |result: &crate::models::ScanResult| {
        result.categories[0]
            .items
            .iter()
            .map(|item| (item.id.clone(), item.observed_bytes()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        summary(&first),
        summary(&second),
        "two scans of one tree state the same items in the same order"
    );
    assert_eq!(first.total_bytes, second.total_bytes);
    // The counts describe the tree, so they repeat; the duration and the
    // peak task count describe one run and are compared to the bound
    // instead of to each other.
    assert_eq!(
        first.metrics.visited_entries,
        second.metrics.visited_entries
    );
    assert_eq!(
        first.metrics.directories_read,
        second.metrics.directories_read
    );
    assert!(
        !first.cancelled && !second.cancelled,
        "nothing cancelled these scans, and the result says so"
    );

    let limits = crate::scanner::ScanLimits::default();
    assert!(first.metrics.visited_entries > 0);
    assert!(first.metrics.directories_read > 0);
    assert!(
        first.metrics.peak_outstanding_directory_tasks
            <= limits.max_concurrent_directory_reads as u64,
        "the traversal stays inside its stated bound: {:?}",
        first.metrics
    );
}

#[test]
fn cancellation_between_signatures_preserves_global_accounting() {
    // Cancellation is observed through the progress stream rather than by
    // counting probe calls: the traversal consults the probe at every
    // directory boundary, so a call count would describe the walk instead
    // of the behaviour this test is about. The probe reports cancellation
    // as soon as the first item has been observed, which stops the run
    // between signatures exactly as a user cancel does.
    struct CancelAfterFirstItem {
        cancelled: AtomicBool,
    }

    impl crate::models::CancellationProbe for CancelAfterFirstItem {
        fn is_cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }
    }

    let fixture = tempfile::tempdir().unwrap();
    let first_root = fixture.path().join("first-cache");
    let second_root = fixture.path().join("second-cache");
    std::fs::create_dir_all(&first_root).unwrap();
    std::fs::create_dir_all(&second_root).unwrap();
    std::fs::write(first_root.join("data.bin"), vec![1u8; 100]).unwrap();
    std::fs::write(second_root.join("data.bin"), vec![2u8; 200]).unwrap();

    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "test.cancel.first",
        "First cache",
        Category::Developer,
        &first_root,
        vec![],
        None,
    ));
    registry.register(signature(
        "test.cancel.second",
        "Second cache",
        Category::Developer,
        &second_root,
        vec![],
        None,
    ));

    let cancellation = CancelAfterFirstItem {
        cancelled: AtomicBool::new(false),
    };
    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
        Some(&[Category::Developer]),
        &[],
        false,
        &scan_environment(),
        &cancellation,
        |event| {
            if matches!(event, ScanEvent::ItemFound { .. }) {
                cancellation.cancelled.store(true, Ordering::SeqCst);
            }
        },
    );

    assert_eq!(result.quality, ObservationQuality::Partial);
    assert_eq!(result.categories.len(), 1);
    assert!(result.categories[0].total_bytes > 0);
    assert_eq!(
        result.total_bytes,
        result
            .categories
            .iter()
            .map(|category| category.total_bytes)
            .sum::<u64>()
    );
    assert_eq!(
        result.cleanable_bytes,
        result
            .categories
            .iter()
            .map(|category| category.cleanable_bytes)
            .sum::<u64>()
    );
    assert_eq!(
        result.skipped_entry_count,
        result
            .categories
            .iter()
            .map(|category| category.skipped_entry_count)
            .sum::<u64>()
    );
    assert_eq!(
        result.incomplete_item_count,
        result
            .categories
            .iter()
            .map(|category| category.incomplete_item_count)
            .sum::<u64>()
    );
}
