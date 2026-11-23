//! Which retained units the overlap resolution compares a candidate against.
//!
//! The resolution asks, for each candidate, which earlier retained unit it is
//! the same as or inside, and takes the first answer in retention order. Asking
//! every retained unit makes a scan of `n` units cost `n²` relationship
//! questions (#493). A unit can only be related to another through a shared
//! location — its own path text or one of its ancestors', or a stable identity
//! the resolver can state — so the retained units are indexed by those keys and
//! only the ones a candidate reaches are asked, still in retention order.
//!
//! Skipping a retained unit is sound only if the resolver would have called it
//! `Distinct`: a `Distinct` answer changes nothing in the resolution. The text
//! index covers the resolution's own textual fallback exactly; anything else a
//! resolver can relate is its own [`OverlapAnchors`] contract.

use super::super::{CleanupUnitIdentity, ScanItem};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// The keys, besides path text, through which a relationship resolver can find
/// two units related.
///
/// Contract: whenever the resolver answers anything other than `Distinct` for
/// a candidate and a container — and its answer is not the textual one (the
/// same key, or the candidate's key inside the container's) — one of
/// `anchors(container)` is among `reach(candidate)`. A resolver that relates
/// units only through their text states no keys at all.
pub trait OverlapAnchors {
    type Key: Hash + Eq;

    /// The keys a retained unit can be found at.
    fn anchors(&self, container: &ScanItem) -> Vec<Self::Key>;

    /// The keys a candidate reaches: its own, and its ancestors'.
    fn reach(&self, candidate: &ScanItem) -> Vec<Self::Key>;
}

/// How the resolution chooses the retained units it compares a candidate to.
pub(super) trait ContainerLookup {
    /// The unit at `position` in retention order is retained from now on.
    fn retain(&mut self, position: usize, key: &CleanupUnitIdentity, item: &ScanItem);

    /// Writes the retention positions `candidate` may be related to, in
    /// ascending order and without repeats.
    fn candidates(
        &mut self,
        key: &CleanupUnitIdentity,
        candidate: &ScanItem,
        retained: usize,
        out: &mut Vec<usize>,
    );
}

/// Every retained unit, for a resolver that states no anchors.
pub(super) struct EveryRetained;

impl ContainerLookup for EveryRetained {
    fn retain(&mut self, _position: usize, _key: &CleanupUnitIdentity, _item: &ScanItem) {}

    fn candidates(
        &mut self,
        _key: &CleanupUnitIdentity,
        _candidate: &ScanItem,
        retained: usize,
        out: &mut Vec<usize>,
    ) {
        out.clear();
        out.extend(0..retained);
    }
}

/// The retained units a candidate shares a location key with.
pub(super) struct ByLocation<'a, A: OverlapAnchors> {
    anchors: &'a A,
    by_text: HashMap<CleanupUnitIdentity, Vec<usize>>,
    by_anchor: HashMap<A::Key, Vec<usize>>,
}

impl<'a, A: OverlapAnchors> ByLocation<'a, A> {
    pub(super) fn new(anchors: &'a A) -> Self {
        Self {
            anchors,
            by_text: HashMap::new(),
            by_anchor: HashMap::new(),
        }
    }
}

impl<A: OverlapAnchors> ContainerLookup for ByLocation<'_, A> {
    fn retain(&mut self, position: usize, key: &CleanupUnitIdentity, item: &ScanItem) {
        self.by_text.entry(key.clone()).or_default().push(position);
        for anchor in self.anchors.anchors(item) {
            self.by_anchor.entry(anchor).or_default().push(position);
        }
    }

    fn candidates(
        &mut self,
        key: &CleanupUnitIdentity,
        candidate: &ScanItem,
        _retained: usize,
        out: &mut Vec<usize>,
    ) {
        out.clear();
        // The textual fallback relates equal keys and a key inside another.
        // Equality is looked up as the key itself, so two empty keys still
        // meet the way the fallback compares them.
        for text in std::iter::once(key.clone()).chain(key.ancestors()) {
            if let Some(positions) = self.by_text.get(&text) {
                out.extend_from_slice(positions);
            }
        }
        for anchor in self.anchors.reach(candidate) {
            if let Some(positions) = self.by_anchor.get(&anchor) {
                out.extend_from_slice(positions);
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// The keys already folded into a container, answering "is this key inside
/// one of them?" by looking its ancestors up instead of comparing every key.
#[derive(Default)]
pub(super) struct FoldedKeys(HashSet<CleanupUnitIdentity>);

impl FoldedKeys {
    pub(super) fn insert(&mut self, key: CleanupUnitIdentity) {
        self.0.insert(key);
    }

    /// The same answer as `keys.iter().any(|outer| key.is_within(outer))`.
    pub(super) fn contains_within(&self, key: &CleanupUnitIdentity) -> bool {
        if key.components().next().is_none() {
            // An empty key is inside nothing.
            return false;
        }
        self.0.contains(key) || key.ancestors().any(|ancestor| self.0.contains(&ancestor))
    }
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
