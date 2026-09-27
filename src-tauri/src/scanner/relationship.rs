//! Establishing which two discovered units describe the same location.
//!
//! The scan and the planner both need this answer, and they must not derive it
//! differently: a scan states what it found, and a plan authorizes a subset of
//! what that scan reported. Deriving it from the OS family instead — "Windows
//! folds case, POSIX does not" — gets it wrong in both directions, because case
//! behaviour is a property of the volume and not of the platform: a Windows
//! volume with per-directory case sensitivity holds two distinct entries, and a
//! case-folding APFS volume holds one.
//!
//! The objects answer first: stable filesystem identity decides whether two
//! units are the same entry, and the identities of the ancestor entries decide
//! containment. Only when an identity cannot be obtained does the comparison
//! fall back to path text, conservatively case-sensitive.

use crate::models::{FileIdentity, PathIdentity, ScanItem, UnitRelationship};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

trait RelationshipProbe {
    fn identity(&mut self, path: &Path) -> Option<FileIdentity>;
    fn entry_name(&mut self, path: &Path, entity: FileIdentity) -> Option<OsString>;
}

struct FilesystemProbe;

impl RelationshipProbe for FilesystemProbe {
    fn identity(&mut self, path: &Path) -> Option<FileIdentity> {
        crate::safety::ToctouGuard::capture(path).map(|identity| identity.entity())
    }

    fn entry_name(&mut self, path: &Path, entity: FileIdentity) -> Option<OsString> {
        actual_entry_name_with(path, entity, self)
    }
}

/// Observations for one overlap pass only, never cleanup authorization.
/// The planner and executor must continue to inspect the live filesystem.
#[derive(Default)]
pub(crate) struct ScanRelationships {
    identities: HashMap<PathBuf, Option<FileIdentity>>,
    entry_names: HashMap<(PathBuf, FileIdentity), Option<OsString>>,
    #[cfg(test)]
    identity_reads: usize,
    #[cfg(test)]
    directory_reads: usize,
}

impl ScanRelationships {
    pub(crate) fn relationship(
        &mut self,
        candidate: &ScanItem,
        container: &ScanItem,
    ) -> UnitRelationship {
        unit_relationship_with(candidate, container, self)
    }
}

impl RelationshipProbe for ScanRelationships {
    fn identity(&mut self, path: &Path) -> Option<FileIdentity> {
        if let Some(identity) = self.identities.get(path) {
            return *identity;
        }
        #[cfg(test)]
        {
            self.identity_reads += 1;
        }
        let identity = FilesystemProbe.identity(path);
        self.identities.insert(path.to_path_buf(), identity);
        identity
    }

    fn entry_name(&mut self, path: &Path, entity: FileIdentity) -> Option<OsString> {
        let key = (path.to_path_buf(), entity);
        if let Some(name) = self.entry_names.get(&key) {
            return name.clone();
        }
        #[cfg(test)]
        {
            self.directory_reads += 1;
        }
        let name = actual_entry_name_with(path, entity, self);
        self.entry_names.insert(key, name.clone());
        name
    }
}

/// Resolve the actual directory entry for a spelling that reached `entity`.
/// On a case-folding volume, `pip` can open an entry stored as `Pip`; on a
/// case-sensitive volume both spellings may be separate hardlinks to one inode.
fn actual_entry_name_with(
    path: &Path,
    entity: FileIdentity,
    probe: &mut impl RelationshipProbe,
) -> Option<OsString> {
    let requested = path.file_name()?;
    let mut folded_match = None;
    for entry in std::fs::read_dir(path.parent()?).ok()?.flatten() {
        let name = entry.file_name();
        if name != requested
            && !name
                .to_string_lossy()
                .eq_ignore_ascii_case(&requested.to_string_lossy())
        {
            continue;
        }
        let matches_entity = probe
            .identity(&entry.path())
            .is_some_and(|identity| identity == entity);
        if !matches_entity {
            continue;
        }
        if name == requested {
            return Some(name);
        }
        if folded_match.is_some() {
            return None;
        }
        folded_match = Some(name);
    }
    folded_match
}

/// Whether both paths name one directory entry, whatever their spelling.
#[cfg(test)]
pub(crate) fn same_directory_entry(first: &Path, second: &Path, entity: FileIdentity) -> bool {
    same_directory_entry_with(first, second, entity, &mut FilesystemProbe)
}

fn same_directory_entry_with(
    first: &Path,
    second: &Path,
    entity: FileIdentity,
    probe: &mut impl RelationshipProbe,
) -> bool {
    let parents_match = first
        .parent()
        .and_then(|path| probe.identity(path))
        .zip(second.parent().and_then(|path| probe.identity(path)))
        .is_some_and(|(left, right)| left.same_entity(right));
    parents_match
        && probe
            .entry_name(first, entity)
            .zip(probe.entry_name(second, entity))
            .is_some_and(|(left, right)| left == right)
}

/// The relationship the filesystem establishes between two units, without
/// policy: whether they name one object, one contains the other, or they are
/// separate locations.
pub(crate) fn unit_relationship(candidate: &ScanItem, container: &ScanItem) -> UnitRelationship {
    unit_relationship_with(candidate, container, &mut FilesystemProbe)
}

fn unit_relationship_with(
    candidate: &ScanItem,
    container: &ScanItem,
    probe: &mut impl RelationshipProbe,
) -> UnitRelationship {
    if let Some(relationship) = filesystem_relationship(candidate, container, probe) {
        return relationship;
    }
    // No stable identity is available (the path is gone, or the platform
    // cannot state one). Text is the conservative fallback: case-sensitive, so
    // two spellings are treated as two locations rather than one.
    let candidate_key = candidate.unit_identity(PathIdentity::CaseSensitive);
    let container_key = container.unit_identity(PathIdentity::CaseSensitive);
    if candidate_key == container_key {
        UnitRelationship::Equivalent
    } else if candidate_key.is_within(&container_key) {
        UnitRelationship::Contained
    } else {
        UnitRelationship::Distinct
    }
}

/// The relationship stable filesystem identity establishes, when it can.
fn filesystem_relationship(
    candidate: &ScanItem,
    container: &ScanItem,
    probe: &mut impl RelationshipProbe,
) -> Option<UnitRelationship> {
    let candidate_path = Path::new(&candidate.unit.path);
    let container_path = Path::new(&container.unit.path);
    let candidate_entity = probe.identity(candidate_path)?;
    let container_entity = probe.identity(container_path)?;
    if candidate_entity.is_unknown() || container_entity.is_unknown() {
        return None;
    }
    if candidate_entity.same_entity(container_entity)
        && same_directory_entry_with(candidate_path, container_path, candidate_entity, probe)
    {
        return Some(UnitRelationship::Equivalent);
    }
    if candidate_path.ancestors().skip(1).any(|ancestor| {
        probe
            .identity(ancestor)
            .is_some_and(|identity| identity.same_entity(container_entity))
    }) {
        return Some(UnitRelationship::Contained);
    }
    Some(UnitRelationship::Distinct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, CleanupUnit, FileSize, RiskTier, ScanItem, UnitRelationship};

    fn item(signature: &str, path: &str) -> ScanItem {
        ScanItem::mock(
            format!("{signature}:{path}"),
            signature,
            signature,
            Category::System,
            RiskTier::Safe,
            path,
            FileSize::new(1_024, Some(1_024)),
            1,
        )
    }

    #[derive(Default)]
    struct CountingProbe {
        identity_reads: usize,
    }

    impl RelationshipProbe for CountingProbe {
        fn identity(&mut self, path: &Path) -> Option<FileIdentity> {
            self.identity_reads += 1;
            FilesystemProbe.identity(path)
        }

        fn entry_name(&mut self, path: &Path, entity: FileIdentity) -> Option<OsString> {
            actual_entry_name_with(path, entity, self)
        }
    }

    #[test]
    fn overlap_pass_reads_each_identity_once_instead_of_once_per_pair() {
        let fixture = tempfile::tempdir().expect("fixture");
        let items = (0..64)
            .map(|index| {
                let path = fixture.path().join(format!("cache-{index}"));
                std::fs::create_dir(&path).expect("fixture");
                item("test.cache", &path.to_string_lossy())
            })
            .collect::<Vec<_>>();
        let unique_paths = items
            .iter()
            .flat_map(|item| {
                Path::new(&item.unit.path)
                    .ancestors()
                    .map(Path::to_path_buf)
            })
            .collect::<std::collections::HashSet<_>>();
        let mut snapshot = ScanRelationships::default();
        let mut live = CountingProbe::default();
        for (left, candidate) in items.iter().enumerate() {
            for (right, container) in items.iter().enumerate() {
                let expected = if left == right {
                    UnitRelationship::Equivalent
                } else {
                    UnitRelationship::Distinct
                };
                assert_eq!(
                    unit_relationship_with(candidate, container, &mut live),
                    expected
                );
                assert_eq!(snapshot.relationship(candidate, container), expected);
            }
        }
        assert_eq!(snapshot.identity_reads, unique_paths.len());
        assert_eq!(snapshot.directory_reads, items.len());
        assert!(live.identity_reads > items.len() * items.len());
        let first_pass_reads = (snapshot.identity_reads, snapshot.directory_reads);
        for candidate in &items {
            for container in &items {
                snapshot.relationship(candidate, container);
            }
        }
        assert_eq!(
            (snapshot.identity_reads, snapshot.directory_reads),
            first_pass_reads
        );
        println!(
            "64-unit overlap identity probes: live={}, snapshot={}",
            live.identity_reads, snapshot.identity_reads
        );
    }

    #[test]
    fn snapshot_preserves_containment_and_distinct_hardlink_entries() {
        let fixture = tempfile::tempdir().expect("fixture");
        let first = fixture.path().join("cache.bin");
        let second = fixture.path().join("other.bin");
        std::fs::write(&first, b"cache").expect("fixture");
        std::fs::hard_link(&first, &second).expect("hardlink fixture");
        let first = item("test.first", &first.to_string_lossy());
        let second = item("test.second", &second.to_string_lossy());
        let parent = item("test.parent", &fixture.path().to_string_lossy());
        let mut snapshot = ScanRelationships::default();
        for (candidate, container, expected) in [
            (&first, &second, UnitRelationship::Distinct),
            (&second, &first, UnitRelationship::Distinct),
            (&first, &first, UnitRelationship::Equivalent),
            (&first, &parent, UnitRelationship::Contained),
        ] {
            assert_eq!(snapshot.relationship(candidate, container), expected);
            assert_eq!(unit_relationship(candidate, container), expected);
        }
    }

    #[test]
    fn snapshot_is_disposable_and_does_not_cache_authorization_checks() {
        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("cache.bin");
        let mut missing = ScanRelationships::default();
        assert_eq!(missing.identity(&path), None);
        std::fs::write(&path, b"original").expect("fixture");
        assert_eq!(missing.identity(&path), None);
        assert_eq!(missing.identity_reads, 1, "absence is local to one pass");

        let mut first = ScanRelationships::default();
        let original = first.identity(&path).expect("original identity");
        assert!(same_directory_entry(&path, &path, original));
        std::fs::rename(&path, fixture.path().join("held.bin")).expect("retain original inode");
        std::fs::write(&path, b"replacement").expect("replacement");
        let replacement = ScanRelationships::default()
            .identity(&path)
            .expect("new identity");
        assert!(!original.same_entity(replacement));
        assert_eq!(first.identity(&path), Some(original));
        assert!(
            !same_directory_entry(&path, &path, original),
            "authorization re-reads the directory entry"
        );
        assert!(same_directory_entry(&path, &path, replacement));
    }

    /// Identity decides first: two spellings that reach the same directory
    /// entry are one unit, whatever their text.
    #[test]
    fn stable_identity_finds_one_entry_under_two_spellings() {
        let fixture = tempfile::tempdir().expect("fixture");
        let stored = fixture.path().join("Cache");
        std::fs::create_dir(&stored).expect("fixture");
        let alternate = fixture.path().join("cache");

        let candidate = item("test.stored", &stored.to_string_lossy());
        let container = item("test.alternate", &alternate.to_string_lossy());

        let mut snapshot = ScanRelationships::default();
        assert_eq!(
            snapshot.relationship(&candidate, &container),
            unit_relationship(&candidate, &container)
        );

        if alternate.exists() {
            // A folding volume answers to both spellings: one entry, one unit.
            assert_eq!(
                unit_relationship(&candidate, &container),
                UnitRelationship::Equivalent
            );
        } else {
            // A case-sensitive volume holds one entry under one spelling: the
            // missing one is not that entry.
            assert_eq!(
                unit_relationship(&candidate, &container),
                UnitRelationship::Distinct
            );
        }
    }

    /// Containment is established by the identities of the actual ancestor
    /// entries, not by string prefixes: a sibling whose name merely starts the
    /// same way is not inside.
    #[test]
    fn containment_follows_the_real_ancestors() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = fixture.path().join("Cache");
        let child = parent.join("nested");
        let sibling = fixture.path().join("CacheX");
        std::fs::create_dir_all(&child).expect("fixture");
        std::fs::create_dir_all(&sibling).expect("fixture");

        let parent_item = item("test.parent", &parent.to_string_lossy());
        let child_item = item("test.child", &child.to_string_lossy());
        let sibling_item = item("test.sibling", &sibling.to_string_lossy());

        assert_eq!(
            unit_relationship(&child_item, &parent_item),
            UnitRelationship::Contained
        );
        assert_eq!(
            unit_relationship(&sibling_item, &parent_item),
            UnitRelationship::Distinct,
            "a shared name prefix is not containment"
        );
    }

    /// When no stable identity can be obtained, the fallback is text, and it is
    /// conservative: two spellings are two locations rather than one.
    #[test]
    fn missing_identity_falls_back_to_case_sensitive_text() {
        let upper = item("test.upper", r"C:\work\Cache");
        let lower = item("test.lower", r"C:\work\cache");
        let nested = item("test.nested", r"C:\work\Cache\nested");

        assert_eq!(
            unit_relationship(&upper, &lower),
            UnitRelationship::Distinct,
            "an unavailable identity does not fold two spellings into one"
        );
        assert_eq!(
            unit_relationship(&nested, &upper),
            UnitRelationship::Contained
        );
    }

    /// The unit kind does not change the structural answer: this function
    /// states what the filesystem shows, and policy is decided by the caller.
    #[test]
    fn the_structural_answer_ignores_authority() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = fixture.path().join("Cache");
        let child = parent.join("nested");
        std::fs::create_dir_all(&child).expect("fixture");

        let mut child_item = item("test.child", &child.to_string_lossy());
        child_item.unit = CleanupUnit::new(
            crate::models::CleanupUnitKind::ProviderAction,
            child.to_string_lossy().into_owned(),
            child.to_string_lossy().into_owned(),
        );
        let parent_item = item("test.parent", &parent.to_string_lossy());

        assert_eq!(
            unit_relationship(&child_item, &parent_item),
            UnitRelationship::Contained
        );
    }
}
