//! The scan's indexed overlap pass reaches the pairwise verdicts on a real
//! filesystem (#493): nested directories, hard links, case variants, missing
//! paths and coverage observations, through the scan's own relationship
//! snapshot and authority policy.

use super::{scan_unit_relationship, CategoryAccumulator};
use crate::models::{
    resolve_unit_overlaps_indexed, resolve_unit_overlaps_with, Category, CleanStrategy,
    CleanupUnitKind, FileSize, ObservationQuality, PathIdentity, RiskTier, ScanItem, Signature,
};
use crate::scanner::relationship::{ScanAnchors, ScanRelationships};
use crate::signatures::SignatureRegistry;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

struct Sequence(u64);

impl Sequence {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

fn registered(id: &str, strategy: CleanStrategy, min_age_days: Option<u32>) -> Signature {
    Signature {
        deletion_disposition: None,
        id: id.into(),
        name: id.into(),
        category: Category::Developer,
        family: Default::default(),
        risk: RiskTier::Safe,
        strategy,
        paths: Vec::new(),
        exclusions: Vec::new(),
        description: String::new(),
        min_age_days,
        include_prefixes: Vec::new(),
        exclude_prefixes: Vec::new(),
        intensive_only: false,
        platforms: Vec::new(),
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

/// A tree with every relationship the filesystem can state.
fn fixture(root: &Path) -> Vec<PathBuf> {
    let cache = root.join("Cache");
    std::fs::create_dir_all(cache.join("a/b")).unwrap();
    std::fs::create_dir_all(root.join("Other/b")).unwrap();
    std::fs::write(cache.join("x.bin"), b"payload").unwrap();
    std::fs::hard_link(cache.join("x.bin"), cache.join("x-link.bin")).unwrap();
    std::fs::hard_link(cache.join("x.bin"), root.join("Other/x.bin")).unwrap();
    vec![
        root.to_path_buf(),
        cache.clone(),
        cache.join("a"),
        cache.join("a/b"),
        cache.join("x.bin"),
        cache.join("x-link.bin"),
        // A case variant: one entry on a folding volume, absent otherwise.
        root.join("cache"),
        cache.join("A"),
        cache.join("A/b"),
        root.join("Other"),
        root.join("Other/b"),
        root.join("Other/x.bin"),
        root.join("missing"),
        root.join("missing/inside"),
    ]
}

fn random_categories(
    paths: &[PathBuf],
    sequence: &mut Sequence,
) -> Vec<crate::models::CategoryResult> {
    let signatures = [
        "test.contents",
        "test.directory",
        "test.aged",
        "test.manual",
        "test.unregistered",
        "coverage:test.contents",
    ];
    let categories = [Category::Developer, Category::System];
    let mut accumulators = [CategoryAccumulator::new(), CategoryAccumulator::new()];
    for index in 0..1 + sequence.below(28) {
        let path = &paths[sequence.below(paths.len() as u64) as usize];
        let signature = signatures[sequence.below(signatures.len() as u64) as usize];
        let risk = if signature == "test.manual" || signature.starts_with("coverage:") {
            RiskTier::Manual
        } else {
            [RiskTier::Safe, RiskTier::Rebuild][sequence.below(2) as usize]
        };
        let which = sequence.below(2) as usize;
        let bytes = 1 + sequence.below(1 << 20);
        let mut item = ScanItem::mock(
            format!("item-{index}"),
            signature,
            format!("unit-{index}"),
            categories[which],
            risk,
            path.to_string_lossy(),
            FileSize::new(bytes, Some(bytes)),
            1,
        );
        match sequence.below(7) {
            0 => item.quality = ObservationQuality::Partial,
            1 => item.unit.kind = CleanupUnitKind::ProviderAction,
            2 => item.gate = crate::models::EligibilityGate::IntensiveCleanupDisabled,
            3 => item.skipped_entry_count = 1,
            _ => {}
        }
        item.rederive_disposition();
        accumulators[which].push(item);
    }
    let [developer, system] = accumulators;
    vec![
        developer.finalize(Category::Developer, None),
        system.finalize(Category::System, None),
    ]
}

#[test]
fn the_indexed_scan_overlap_pass_reaches_the_pairwise_verdicts() {
    let fixture_root = tempfile::tempdir().unwrap();
    let paths = fixture(fixture_root.path());
    let mut registry = SignatureRegistry::new();
    registry.register(registered(
        "test.contents",
        CleanStrategy::DeleteContents,
        None,
    ));
    registry.register(registered(
        "test.directory",
        CleanStrategy::DeleteDirectory,
        None,
    ));
    registry.register(registered(
        "test.aged",
        CleanStrategy::DeleteContents,
        Some(7),
    ));
    registry.register(registered("test.manual", CleanStrategy::Manual, None));

    let mut sequence = Sequence(0x51_7cc1_b727_220a);
    let mut related = 0u64;
    for case in 0..300 {
        let original = random_categories(&paths, &mut sequence);

        let mut pairwise = original.clone();
        let relationships = RefCell::new(ScanRelationships::default());
        let expected = resolve_unit_overlaps_with(
            &mut pairwise,
            &[],
            PathIdentity::CaseSensitive,
            |candidate, container| {
                scan_unit_relationship(candidate, container, &registry, &relationships)
            },
        );

        let mut indexed = original.clone();
        let relationships = RefCell::new(ScanRelationships::default());
        let actual = resolve_unit_overlaps_indexed(
            &mut indexed,
            &[],
            PathIdentity::CaseSensitive,
            |candidate, container| {
                scan_unit_relationship(candidate, container, &registry, &relationships)
            },
            &ScanAnchors(&relationships),
        );

        assert_eq!(actual, expected, "report, case {case}");
        assert_eq!(indexed, pairwise, "verdicts, case {case}");
        related += expected.suppressed_count + expected.ambiguous_count;
    }
    assert!(
        related > 300,
        "the generated scans must overlap for the comparison to mean anything: {related}"
    );
}

/// Paired timing of the pairwise and indexed overlap passes on a fixture shaped
/// like a full scan: many enumerated children under a few broad roots, and
/// narrower rules that name some of them again. Run with
/// `cargo test -p neati-desktop --lib overlap_pass_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing evidence, not a CI threshold"]
fn overlap_pass_timing() {
    let fixture_root = tempfile::tempdir().unwrap();
    let mut registry = SignatureRegistry::new();
    registry.register(registered(
        "test.contents",
        CleanStrategy::DeleteContents,
        None,
    ));
    registry.register(registered("test.manual", CleanStrategy::Manual, None));
    for units in [500usize, 1_000, 2_000] {
        let mut accumulator = CategoryAccumulator::new();
        for index in 0..units {
            let root = fixture_root.path().join(format!("root-{}", index % 8));
            let child = root.join(format!("unit-{index}"));
            std::fs::create_dir_all(&child).unwrap();
            accumulator.push(ScanItem::mock(
                format!("child-{index}"),
                "test.contents",
                format!("unit-{index}"),
                Category::System,
                RiskTier::Safe,
                child.to_string_lossy(),
                FileSize::new(4_096, Some(4_096)),
                1,
            ));
            if index % 10 == 0 {
                // A narrower observation-only rule names the same unit again.
                accumulator.push(ScanItem::mock(
                    format!("named-{index}"),
                    "test.manual",
                    format!("named-{index}"),
                    Category::System,
                    RiskTier::Manual,
                    child.join("inner").to_string_lossy(),
                    FileSize::new(1_024, Some(1_024)),
                    1,
                ));
            }
        }
        let original = vec![accumulator.finalize(Category::System, None)];
        let mut pairwise = original.clone();
        let started = std::time::Instant::now();
        let relationships = RefCell::new(ScanRelationships::default());
        let expected = resolve_unit_overlaps_with(
            &mut pairwise,
            &[],
            PathIdentity::CaseSensitive,
            |candidate, container| {
                scan_unit_relationship(candidate, container, &registry, &relationships)
            },
        );
        let pairwise_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let mut indexed = original.clone();
        let started = std::time::Instant::now();
        let relationships = RefCell::new(ScanRelationships::default());
        let actual = resolve_unit_overlaps_indexed(
            &mut indexed,
            &[],
            PathIdentity::CaseSensitive,
            |candidate, container| {
                scan_unit_relationship(candidate, container, &registry, &relationships)
            },
            &ScanAnchors(&relationships),
        );
        let indexed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        assert_eq!(actual, expected);
        assert_eq!(indexed, pairwise);
        println!(
            "{{\"overlap_units\":{},\"pairwise_ms\":{pairwise_ms:.1},\"indexed_ms\":{indexed_ms:.1}}}",
            original[0].items.len()
        );
    }
}
