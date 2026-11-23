//! The location index must reach every verdict the pairwise pass reaches.

use super::super::{
    resolve_unit_overlaps_indexed, resolve_unit_overlaps_with, CategoryResult, OverlapAnchors,
    PathIdentity, UnitRelationship,
};
use crate::domain::scan::{
    CleanupUnitKind, EligibilityGate, EligibilitySummary, FileSize, ScanItem,
};
use crate::domain::{Category, ObservationQuality, RiskTier};
use std::collections::HashMap;
use std::path::Path;

/// A deterministic generator, so every failing case can be replayed.
struct Sequence(u64);

impl Sequence {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

fn category(category: Category, items: Vec<ScanItem>) -> CategoryResult {
    let mut result = CategoryResult {
        category,
        display_name: category.display_name().to_string(),
        items,
        total_bytes: 0,
        cleanable_bytes: 0,
        safe_bytes: 0,
        rebuild_bytes: 0,
        manual_bytes: 0,
        quality: ObservationQuality::Fresh,
        skipped_entry_count: 0,
        incomplete_item_count: 0,
        eligibility: EligibilitySummary::default(),
        suppressed_duplicate_count: 0,
        suppressed_duplicate_bytes: 0,
        suppressed_overlap_count: 0,
        suppressed_overlap_bytes: 0,
        ambiguous_overlap_count: 0,
        ambiguous_overlap_bytes: 0,
    };
    result.recompute_accounting();
    result
}

/// A small tree, so duplicates, ancestors, case variants and siblings are all
/// common.
fn random_path(sequence: &mut Sequence) -> String {
    let depth = 1 + sequence.below(4);
    let mut path = String::from("/r");
    for _ in 0..depth {
        path.push('/');
        path.push_str(["a", "b", "c", "A"][sequence.below(4) as usize]);
    }
    if sequence.below(8) == 0 {
        path.push('/');
    }
    path
}

fn random_categories(sequence: &mut Sequence) -> Vec<CategoryResult> {
    let categories = [Category::System, Category::Developer, Category::Ai];
    let mut items: Vec<Vec<ScanItem>> = vec![Vec::new(); categories.len()];
    let count = 1 + sequence.below(48);
    for index in 0..count {
        let path = random_path(sequence);
        let risk =
            [RiskTier::Safe, RiskTier::Rebuild, RiskTier::Manual][sequence.below(3) as usize];
        let bytes = 1 + sequence.below(4_096);
        let which = sequence.below(categories.len() as u64) as usize;
        let mut item = ScanItem::mock(
            format!("item-{index}"),
            format!("signature-{}", sequence.below(5)),
            format!("name-{index}"),
            categories[which],
            risk,
            path,
            FileSize::new(bytes, Some(bytes)),
            1,
        );
        match sequence.below(6) {
            0 => item.quality = ObservationQuality::Partial,
            1 => item.skipped_entry_count = 1,
            2 => item.unit.kind = CleanupUnitKind::ProviderAction,
            3 => item.gate = EligibilityGate::IntensiveCleanupDisabled,
            _ => {}
        }
        if sequence.below(16) == 0 {
            // An undeclared unit is never a candidate.
            item.unit.root.clear();
        }
        item.rederive_disposition();
        items[which].push(item);
    }
    categories
        .iter()
        .zip(items)
        .map(|(name, items)| category(*name, items))
        .collect()
}

/// Stable identities for one generated tree. Some spellings alias an earlier
/// path (a folding volume, a second hard link) and some paths have no identity
/// at all, so the resolver falls back to text for them.
struct Entities(HashMap<String, Option<u64>>);

impl Entities {
    fn of(categories: &[CategoryResult], sequence: &mut Sequence) -> Self {
        let mut paths: Vec<String> = categories
            .iter()
            .flat_map(|category| &category.items)
            .flat_map(|item| {
                Path::new(&item.unit.path)
                    .ancestors()
                    .map(|ancestor| ancestor.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
            .collect();
        paths.sort();
        paths.dedup();
        let mut entities: HashMap<String, Option<u64>> = HashMap::new();
        for (index, path) in paths.iter().enumerate() {
            let entity = match sequence.below(10) {
                0 => None,
                1 | 2 if index > 0 => {
                    let earlier = &paths[sequence.below(index as u64) as usize];
                    entities.get(earlier).copied().flatten()
                }
                _ => Some(index as u64 + 1),
            };
            entities.insert(path.clone(), entity);
        }
        Self(entities)
    }

    fn entity(&self, path: &Path) -> Option<u64> {
        self.0
            .get(path.to_string_lossy().as_ref())
            .copied()
            .flatten()
    }

    /// What a filesystem with these identities states, with a policy layer
    /// like the scan's on top. `None` is the textual fallback when either side
    /// has no identity.
    fn relationship(&self, candidate: &ScanItem, container: &ScanItem) -> Option<UnitRelationship> {
        let own = self.entity(Path::new(&candidate.unit.path))?;
        let outer = self.entity(Path::new(&container.unit.path))?;
        let structural = if own == outer {
            UnitRelationship::Equivalent
        } else if Path::new(&candidate.unit.path)
            .ancestors()
            .skip(1)
            .any(|ancestor| self.entity(ancestor) == Some(outer))
        {
            UnitRelationship::Contained
        } else {
            return Some(UnitRelationship::Distinct);
        };
        Some(match structural {
            UnitRelationship::Equivalent if candidate.risk != container.risk => {
                UnitRelationship::EquivalentConflict
            }
            UnitRelationship::Equivalent if candidate.signature_id == "signature-4" => {
                UnitRelationship::SharedStorage
            }
            UnitRelationship::Contained if container.risk == RiskTier::Manual => {
                UnitRelationship::AuthorityConflict
            }
            other => other,
        })
    }
}

impl OverlapAnchors for Entities {
    type Key = u64;

    fn anchors(&self, container: &ScanItem) -> Vec<u64> {
        self.entity(Path::new(&container.unit.path))
            .into_iter()
            .collect()
    }

    fn reach(&self, candidate: &ScanItem) -> Vec<u64> {
        Path::new(&candidate.unit.path)
            .ancestors()
            .filter_map(|ancestor| self.entity(ancestor))
            .collect()
    }
}

struct TextOnly;

impl OverlapAnchors for TextOnly {
    type Key = u64;

    fn anchors(&self, _container: &ScanItem) -> Vec<u64> {
        Vec::new()
    }

    fn reach(&self, _candidate: &ScanItem) -> Vec<u64> {
        Vec::new()
    }
}

/// The location index gives every verdict the pairwise pass gives: the same
/// retained rows in the same order, the same provenance, conflicts and
/// ambiguities, and the same totals, for textual and identity resolvers alike
/// and under both path identities.
#[test]
fn the_location_index_reaches_the_pairwise_verdicts() {
    let mut sequence = Sequence(0x9e37_79b9_7f4a_7c15);
    let mut related = 0u64;
    for case in 0..600 {
        let original = random_categories(&mut sequence);
        let entities = Entities::of(&original, &mut sequence);
        for identity in [PathIdentity::CaseSensitive, PathIdentity::CaseInsensitive] {
            let mut pairwise = original.clone();
            let mut indexed = original.clone();
            let expected = resolve_unit_overlaps_with(&mut pairwise, &[], identity, |_, _| None);
            let actual =
                resolve_unit_overlaps_indexed(&mut indexed, &[], identity, |_, _| None, &TextOnly);
            assert_eq!(actual, expected, "textual report, case {case}");
            assert_eq!(indexed, pairwise, "textual verdicts, case {case}");

            let mut pairwise = original.clone();
            let mut indexed = original.clone();
            let expected = resolve_unit_overlaps_with(&mut pairwise, &[], identity, |c, o| {
                entities.relationship(c, o)
            });
            let actual = resolve_unit_overlaps_indexed(
                &mut indexed,
                &[],
                identity,
                |c, o| entities.relationship(c, o),
                &entities,
            );
            assert_eq!(actual, expected, "identity report, case {case}");
            assert_eq!(indexed, pairwise, "identity verdicts, case {case}");
            related += expected.suppressed_count + expected.ambiguous_count;
        }
    }
    assert!(
        related > 1_000,
        "the generated trees must overlap for the comparison to mean anything: {related}"
    );
}

/// The folded-key lookup answers exactly what comparing against every folded
/// key answered, including the empty key that names no location.
#[test]
fn folded_keys_answer_like_the_pairwise_containment_check() {
    let mut sequence = Sequence(0x2545_f491_4f6c_dd1d);
    for _ in 0..400 {
        let keys: Vec<_> = (0..1 + sequence.below(12))
            .map(|_| {
                let mut item = ScanItem::mock(
                    "id",
                    "signature",
                    "name",
                    Category::System,
                    RiskTier::Safe,
                    random_path(&mut sequence),
                    FileSize::default(),
                    0,
                );
                if sequence.below(10) == 0 {
                    item.unit.path = "/".into();
                }
                item.unit_identity(PathIdentity::CaseSensitive)
            })
            .collect();
        let (folded, probes) = keys.split_at(keys.len() / 2);
        let mut index = super::FoldedKeys::default();
        for key in folded {
            index.insert(key.clone());
        }
        for probe in probes.iter().chain(folded) {
            assert_eq!(
                index.contains_within(probe),
                folded.iter().any(|outer| probe.is_within(outer)),
                "{probe} against {folded:?}"
            );
        }
    }
}

/// The ancestors of a key are exactly the keys it is strictly inside.
#[test]
fn key_ancestors_are_the_strict_containers() {
    let item = ScanItem::mock(
        "id",
        "signature",
        "name",
        Category::System,
        RiskTier::Safe,
        "/Users/a/Library/Caches/",
        FileSize::default(),
        0,
    );
    let key = item.unit_identity(PathIdentity::CaseSensitive);
    let ancestors: Vec<String> = key.ancestors().map(|key| key.to_string()).collect();
    assert_eq!(
        ancestors,
        ["Users/a/Library", "Users/a", "Users"],
        "nearest first, never the key itself"
    );
    assert!(key.ancestors().all(|ancestor| key.is_within(&ancestor)));
}
