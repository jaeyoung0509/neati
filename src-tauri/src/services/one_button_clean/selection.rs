//! Membership and naming of the one-button set. Pure: no I/O, no ports.
//!
//! The backend decides membership — `select_quick_clean_safe_candidates` for
//! caches and `developer_artifacts::preselection` for project build output.
//! This module only names each member with a reference a person or an agent
//! can repeat, and narrows the set. A reference never adds anything: one that
//! is not in this run's set refuses the whole run, because a mistyped `--skip`
//! would otherwise remove the very thing someone asked to keep.

use std::collections::{BTreeSet, HashSet};

use crate::models::{DeveloperArtifactScanResult, ScanResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cache,
    Project,
}

/// One unit the run would remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// What `--only` and `--skip` accept. Stable across runs for the same
    /// location; it is a name for a backend-chosen unit, never a path.
    pub reference: String,
    pub source: Source,
    pub name: String,
    /// The cache category or the build-output kind.
    pub kind: String,
    pub path: String,
    pub bytes: u64,
    /// The identity the backend plan is built from.
    pub backend_id: String,
}

/// The members of `scan` the backend chose (`chosen_ids`), in scan order.
pub fn cache_candidates(scan: &ScanResult, chosen_ids: &[String]) -> Vec<Candidate> {
    let chosen: HashSet<&String> = chosen_ids.iter().collect();
    scan.categories
        .iter()
        .flat_map(|category| category.items.iter())
        .filter(|item| chosen.contains(&item.id))
        .map(|item| Candidate {
            reference: item.id.clone(),
            source: Source::Cache,
            name: item.name.clone(),
            kind: category_label(item.category).to_string(),
            path: item.path.clone(),
            bytes: item.cleanable_bytes(),
            backend_id: item.id.clone(),
        })
        .collect()
}

/// The build output the backend preselected, without nested duplicates: when
/// one unit contains another, only the outer one is removed.
pub fn project_candidates(result: &DeveloperArtifactScanResult) -> Vec<Candidate> {
    let mut preselected: Vec<_> = result
        .items
        .iter()
        .filter(|item| item.selected_by_default)
        .collect();
    preselected.sort_by_key(|item| path_key(&item.path).len());
    let mut kept: Vec<&crate::models::DeveloperArtifact> = Vec::new();
    for item in preselected {
        if !kept
            .iter()
            .any(|other| paths_overlap(&item.path, &other.path))
        {
            kept.push(item);
        }
    }
    kept.into_iter()
        .map(|item| Candidate {
            reference: project_reference(&item.path),
            source: Source::Project,
            name: item.project_name.clone(),
            kind: kind_label(&item.kind),
            path: item.path.clone(),
            bytes: item.allocated_bytes,
            backend_id: item.id.clone(),
        })
        .collect()
}

/// A short, repeatable name for a project unit. Scan IDs for project output
/// are per scan, so the reference is derived from the location instead.
pub fn project_reference(path: &str) -> String {
    let digest = crate::hash::sha256_hex(path_key(path).as_bytes());
    format!("project:{}", &digest[..12])
}

/// Applies `only` and `skip`. Every reference must name a member of
/// `candidates`; otherwise the unknown references come back and nothing is
/// chosen.
pub fn narrow(
    candidates: Vec<Candidate>,
    only: &[String],
    skip: &[String],
) -> Result<Vec<Candidate>, Vec<String>> {
    let known: HashSet<&str> = candidates
        .iter()
        .map(|candidate| candidate.reference.as_str())
        .collect();
    let unknown: BTreeSet<&str> = only
        .iter()
        .chain(skip)
        .map(String::as_str)
        .filter(|reference| !known.contains(reference))
        .collect();
    if !unknown.is_empty() {
        return Err(unknown.into_iter().map(str::to_string).collect());
    }
    let only: HashSet<&str> = only.iter().map(String::as_str).collect();
    let skip: HashSet<&str> = skip.iter().map(String::as_str).collect();
    Ok(candidates
        .into_iter()
        .filter(|candidate| only.is_empty() || only.contains(candidate.reference.as_str()))
        .filter(|candidate| !skip.contains(candidate.reference.as_str()))
        .collect())
}

pub fn category_label(category: crate::models::Category) -> &'static str {
    match category {
        crate::models::Category::Ai => "AI tools",
        crate::models::Category::Developer => "Developer tools",
        crate::models::Category::Container => "Containers",
        crate::models::Category::Model => "Models",
        crate::models::Category::System => "System",
    }
}

/// `cargo_target` → `cargo target`: the serialized kind, readable.
pub fn kind_label(kind: &crate::models::DeveloperArtifactKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(|name| name.replace('_', " ")))
        .unwrap_or_else(|| "build output".to_string())
}

fn path_key(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn paths_overlap(left: &str, right: &str) -> bool {
    let (left, right) = (path_key(left), path_key(right));
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(reference: &str) -> Candidate {
        Candidate {
            reference: reference.to_string(),
            source: Source::Cache,
            name: reference.to_string(),
            kind: "System".to_string(),
            path: format!("/Users/me/Library/Caches/{reference}"),
            bytes: 1,
            backend_id: reference.to_string(),
        }
    }

    fn references(candidates: &[Candidate]) -> Vec<&str> {
        candidates
            .iter()
            .map(|candidate| candidate.reference.as_str())
            .collect()
    }

    #[test]
    fn no_filters_keep_the_whole_backend_set() {
        let kept = narrow(vec![candidate("a"), candidate("b")], &[], &[]).expect("no filters");
        assert_eq!(references(&kept), ["a", "b"]);
    }

    #[test]
    fn only_and_skip_narrow_and_never_widen() {
        let set = || vec![candidate("a"), candidate("b"), candidate("c")];
        let only = narrow(set(), &["a".into(), "c".into()], &[]).expect("known refs");
        assert_eq!(references(&only), ["a", "c"]);
        let skipped = narrow(set(), &[], &["b".into()]).expect("known ref");
        assert_eq!(references(&skipped), ["a", "c"]);
        let both = narrow(set(), &["a".into(), "b".into()], &["b".into()]).expect("known refs");
        assert_eq!(references(&both), ["a"]);
    }

    #[test]
    fn an_unknown_reference_refuses_the_whole_run() {
        // A path or a reviewable item's id is not in the set, so it cannot be
        // smuggled in through --only.
        let refused = narrow(vec![candidate("a")], &["/Users/me/Documents".into()], &[]);
        assert!(refused.is_err());
        // A mistyped --skip must not quietly remove what it meant to keep.
        let refused = narrow(vec![candidate("a"), candidate("b")], &[], &["bb".into()]);
        assert_eq!(refused.expect_err("unknown skip"), ["bb"]);
    }

    #[test]
    fn project_references_are_stable_and_path_shaped_only_by_hash() {
        let first = project_reference("/Users/me/code/app/target");
        assert_eq!(first, project_reference("/Users/me/code/app/target/"));
        assert_ne!(first, project_reference("/Users/me/code/other/target"));
        assert!(first.starts_with("project:"));
        assert!(!first.contains('/'));
    }

    fn artifact(path: &str, selected_by_default: bool) -> crate::models::DeveloperArtifact {
        crate::models::DeveloperArtifact {
            id: format!("scan-local-{path}"),
            workspace_id: "workspace".to_string(),
            project_name: "app".to_string(),
            ecosystem: crate::models::DeveloperEcosystem::Rust,
            kind: crate::models::DeveloperArtifactKind::CargoTarget,
            path: path.to_string(),
            logical_bytes: 10,
            allocated_bytes: 10,
            file_count: 1,
            newest_mtime: None,
            rebuild_hint: None,
            evidence: Vec::new(),
            ownership: crate::models::ArtifactOwnershipEvidence::VerifiedGenerated,
            status: crate::models::DeveloperArtifactStatus::Complete,
            incomplete_reason: None,
            selected_by_default,
        }
    }

    #[test]
    fn only_backend_preselected_build_output_is_offered_once() {
        let result = DeveloperArtifactScanResult {
            scan_id: "scan".to_string(),
            items: vec![
                artifact("/code/app/target/debug", true),
                artifact("/code/app/target", true),
                artifact("/code/app/node_modules", false),
                artifact("/code/lib/target", true),
            ],
            discovered_count: 4,
            measured_count: 4,
            skipped_entries: 0,
            cancelled: false,
            truncated: false,
            uninspected: Vec::new(),
        };
        let offered = project_candidates(&result);
        let paths: Vec<&str> = offered.iter().map(|item| item.path.as_str()).collect();
        assert_eq!(paths, ["/code/app/target", "/code/lib/target"]);
        assert_eq!(offered[0].kind, "cargo target");
        assert_eq!(offered[0].backend_id, "scan-local-/code/app/target");
        assert_eq!(offered[0].reference, project_reference("/code/app/target"));
    }

    #[test]
    fn nested_project_units_collapse_to_the_outer_one() {
        assert!(paths_overlap("/p/target", "/p/target/debug"));
        assert!(paths_overlap("/p/target/", "/p/target"));
        assert!(!paths_overlap("/p/target", "/p/target-old"));
    }
}
