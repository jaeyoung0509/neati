//! Observes the siblings a keep-newest rule compares (#480).
//!
//! The scan, the planner and the execution guard each call [`retention_of`]
//! for the path they are about to report, plan or remove, so the newest unit
//! is decided by one function over a listing taken at that boundary. What the
//! scan displayed is never trusted later.

use crate::models::{RetentionReason, ScanItem};
use neati_core::domain::scan::{retained_siblings, SiblingObservation};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// More sibling folders than a version store holds. A larger listing is not
/// compared, so every unit in it is kept.
const MAX_SIBLINGS: usize = 4096;

/// The units directly inside `parent` that the rule keeps, by name.
///
/// Only real directories compete: a file or a link is not a versioned unit.
/// `None` when the folder cannot be listed completely, so a caller keeps the
/// unit rather than guess which one is newest.
pub fn retained_children(parent: &Path) -> Option<HashMap<OsString, RetentionReason>> {
    let mut children = Vec::new();
    for entry in fs::read_dir(parent).ok()? {
        let entry = entry.ok()?;
        let metadata = fs::symlink_metadata(entry.path()).ok()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        if children.len() == MAX_SIBLINGS {
            return None;
        }
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs());
        children.push((entry.file_name(), modified));
    }
    let names: Vec<String> = children
        .iter()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    let siblings: Vec<SiblingObservation<'_>> = names
        .iter()
        .zip(&children)
        .map(|(name, (_, modified))| SiblingObservation {
            name,
            modified: *modified,
        })
        .collect();
    let verdicts = retained_siblings(&siblings);
    Some(
        children
            .into_iter()
            .zip(verdicts)
            .filter_map(|((name, _), verdict)| verdict.map(|reason| (name, reason)))
            .collect(),
    )
}

/// Whether the rule keeps `path` among its siblings, observed now.
pub fn retention_of(path: &Path) -> Option<RetentionReason> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Some(RetentionReason::SiblingsUnverified);
    };
    match retained_children(parent) {
        Some(retained) => retained.get(name).copied(),
        None => Some(RetentionReason::SiblingsUnverified),
    }
}

/// Records the rule's verdict on every item of one signature's scan, listing
/// each parent folder once.
pub fn annotate(items: &mut [ScanItem]) {
    let mut parents: HashMap<PathBuf, Option<HashMap<OsString, RetentionReason>>> = HashMap::new();
    for item in items {
        let path = Path::new(&item.path);
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            item.retained = Some(RetentionReason::SiblingsUnverified);
            continue;
        };
        let retained = parents
            .entry(parent.to_path_buf())
            .or_insert_with(|| retained_children(parent));
        item.retained = match retained {
            Some(retained) => retained.get(name).copied(),
            None => Some(RetentionReason::SiblingsUnverified),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_sibling_directories_compete() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for name in ["16.0 (20A362)", "17.0 (21A329)"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        // A newer-looking file is not a symbol set.
        fs::write(root.join("18.0 (22A3354)"), b"not a directory").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("16.0 (20A362)"), root.join("19.0 (23A1)")).unwrap();

        assert_eq!(retention_of(&root.join("16.0 (20A362)")), None);
        assert_eq!(
            retention_of(&root.join("17.0 (21A329)")),
            Some(RetentionReason::NewestVersion)
        );
        assert_eq!(retention_of(&root.join("18.0 (22A3354)")), None);
    }

    #[test]
    fn an_unlistable_parent_keeps_the_unit() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            retention_of(&temp.path().join("missing/17.0 (21A329)")),
            Some(RetentionReason::SiblingsUnverified)
        );
        assert_eq!(
            retention_of(Path::new("/")),
            Some(RetentionReason::SiblingsUnverified)
        );
    }
}
