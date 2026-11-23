//! Keep-newest retention among enumerated sibling units (#480).
//!
//! Some stores hold one unit per tool or OS version, and the newest one is the
//! one in use: Xcode keeps one DeviceSupport symbol set per connected OS
//! version and needs the newest to debug the device in hand. A unit the rule
//! keeps is still discovered and measured; it is never offered for removal.
//!
//! The decision is a pure function of the sibling names and their own
//! modification times, so the scan, the planner and the execution guard ask
//! the same question of the same facts and cannot drift.

use serde::{Deserialize, Serialize};

/// Why the keep-newest rule retains a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RetentionReason {
    /// Its name carries the newest version among its siblings.
    NewestVersion,
    /// A sibling name carries no version, so the most recently modified unit
    /// is kept as well.
    NewestModified,
    /// The siblings could not be listed completely, so no unit can be shown
    /// not to be the newest.
    SiblingsUnverified,
}

impl RetentionReason {
    /// The reason the interface shows for a retained unit.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::NewestVersion => {
                "Kept: this is the newest version in its folder. Older versions beside it can be cleaned."
            }
            Self::NewestModified => {
                "Kept: a version in this folder could not be read, so the most recently modified entry is kept as well."
            }
            Self::SiblingsUnverified => {
                "Kept: the folder could not be listed completely, so neati cannot show this is not the newest version."
            }
        }
    }
}

/// One sibling unit as the rule sees it: its name and its own modification
/// time in seconds since the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SiblingObservation<'a> {
    pub name: &'a str,
    pub modified: Option<u64>,
}

/// The leading dotted version in a unit name.
///
/// Xcode names symbol sets `17.4 (21E213)`, `17.4.1 (21E236) arm64e`, or, since
/// Xcode 15, with the device model first: `iPhone15,2 17.4 (21E219)`. The
/// version is the first whitespace-separated token made only of dot-separated
/// decimal numbers. Trailing zero components are dropped so `17.4` and
/// `17.4.0` compare equal.
pub fn leading_version(name: &str) -> Option<Vec<u64>> {
    name.split_whitespace().find_map(|token| {
        let mut components = Vec::new();
        for part in token.split('.') {
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            components.push(part.parse::<u64>().ok()?);
        }
        while components.len() > 1 && components.last() == Some(&0) {
            components.pop();
        }
        Some(components)
    })
}

/// Which siblings the keep-newest rule retains, one verdict per sibling.
///
/// - Every sibling whose version equals the highest version is kept: the same
///   OS version built for two architectures is one version.
/// - When any sibling name carries no version, the most recently modified
///   sibling is kept too, and so is any sibling whose time is unknown: neither
///   can be shown not to be the newest.
pub fn retained_siblings(siblings: &[SiblingObservation<'_>]) -> Vec<Option<RetentionReason>> {
    let versions: Vec<Option<Vec<u64>>> = siblings
        .iter()
        .map(|sibling| leading_version(sibling.name))
        .collect();
    let newest_version = versions.iter().flatten().max().cloned();
    let unversioned = versions.iter().any(Option::is_none);
    let newest_modified = siblings.iter().filter_map(|sibling| sibling.modified).max();
    siblings
        .iter()
        .zip(&versions)
        .map(|(sibling, version)| {
            if version.is_some() && *version == newest_version {
                return Some(RetentionReason::NewestVersion);
            }
            if unversioned && (sibling.modified.is_none() || sibling.modified == newest_modified) {
                return Some(RetentionReason::NewestModified);
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sibling(name: &str, modified: u64) -> SiblingObservation<'_> {
        SiblingObservation {
            name,
            modified: Some(modified),
        }
    }

    #[test]
    fn reads_the_leading_version_with_or_without_a_device_model() {
        assert_eq!(leading_version("17.4 (21E213)"), Some(vec![17, 4]));
        assert_eq!(
            leading_version("17.4.1 (21E236) arm64e"),
            Some(vec![17, 4, 1])
        );
        assert_eq!(
            leading_version("iPhone15,2 17.4 (21E219)"),
            Some(vec![17, 4])
        );
        assert_eq!(leading_version("Watch6,2 10.4 (21T216)"), Some(vec![10, 4]));
        assert_eq!(leading_version("18.0.0 (22A3354)"), Some(vec![18]));
        assert_eq!(leading_version("18"), Some(vec![18]));
        for unversioned in ["", "Latest", "(21E213)", "17..4", ".17", "17.", "v17.4"] {
            assert_eq!(leading_version(unversioned), None, "{unversioned}");
        }
    }

    #[test]
    fn keeps_the_highest_version_not_the_latest_written() {
        let siblings = [
            sibling("16.7 (20H19)", 300),
            sibling("17.4.1 (21E236)", 100),
            sibling("17.10 (21H1)", 50),
            sibling("17.4 (21E213)", 200),
        ];
        assert_eq!(
            retained_siblings(&siblings),
            [None, None, Some(RetentionReason::NewestVersion), None]
        );
    }

    #[test]
    fn keeps_every_build_of_the_newest_version() {
        let siblings = [
            sibling("iPhone15,2 17.4 (21E219)", 1),
            sibling("iPhone14,5 17.4.0 (21E219)", 2),
            sibling("17.4 (21E219) arm64e", 3),
            sibling("17.3 (21D50)", 4),
        ];
        let newest = Some(RetentionReason::NewestVersion);
        assert_eq!(retained_siblings(&siblings), [newest, newest, newest, None]);
    }

    #[test]
    fn an_unversioned_name_also_keeps_the_most_recently_modified_sibling() {
        let siblings = [
            sibling("17.0 (21A329)", 10),
            sibling("Latest", 30),
            sibling("18.1 (22B83)", 20),
            SiblingObservation {
                name: "16.0 (20A362)",
                modified: None,
            },
        ];
        assert_eq!(
            retained_siblings(&siblings),
            [
                None,
                Some(RetentionReason::NewestModified),
                Some(RetentionReason::NewestVersion),
                Some(RetentionReason::NewestModified),
            ]
        );
        let unversioned = [sibling("a", 1), sibling("b", 2)];
        assert_eq!(
            retained_siblings(&unversioned),
            [None, Some(RetentionReason::NewestModified)]
        );
    }

    #[test]
    fn a_single_sibling_is_always_the_newest() {
        assert_eq!(
            retained_siblings(&[sibling("17.0 (21A329)", 1)]),
            [Some(RetentionReason::NewestVersion)]
        );
        assert!(retained_siblings(&[]).is_empty());
    }
}
