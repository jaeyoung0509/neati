//! Accounting-only hard-link audit. No observation here grants deletion authority.
use crate::models::{CategoryResult, FileIdentity};
use crate::safety::{SymlinkGuard, ToctouGuard};
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct SharedStorageReport {
    pub bytes: u64,
    pub count: u64,
    pub incomplete: bool,
}

pub(crate) fn audit(
    categories: &mut [CategoryResult],
    environment: &neati_platform::PlatformEnvironment,
) -> SharedStorageReport {
    audit_bounded(categories, 50_000, Duration::from_millis(500), environment)
}

fn audit_bounded(
    categories: &mut [CategoryResult],
    limit: usize,
    time: Duration,
    environment: &neati_platform::PlatformEnvironment,
) -> SharedStorageReport {
    let started = Instant::now();
    let mut visited = 0;
    let mut seen = HashSet::<FileIdentity>::new();
    let mut report = SharedStorageReport::default();
    for category in categories {
        for item in &category.items {
            if item.observed_bytes() == 0 || !item.unit.is_declared() {
                continue;
            }
            let root = std::path::Path::new(&item.unit.path);
            // Direct hard-link entries are handled by the unit relationship pass.
            if !root.is_dir() || SymlinkGuard::is_symlink(root) {
                continue;
            }
            let safe_root = SymlinkGuard::validate_anchored_path(root, environment).is_ok();
            let mut pending = if safe_root {
                vec![(root.to_path_buf(), 0)]
            } else {
                vec![]
            };
            let mut possible_duplicate = 0u64;
            let mut unknown = !safe_root;
            while let Some((path, depth)) = pending.pop() {
                if visited >= limit || started.elapsed() >= time {
                    unknown = true;
                    break;
                }
                visited += 1;
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    unknown = true;
                    continue;
                };
                if SymlinkGuard::is_symlink(&path) {
                    continue;
                }
                if metadata.is_file() {
                    #[cfg(unix)]
                    let shared = {
                        use std::os::unix::fs::MetadataExt;
                        metadata.nlink() > 1
                    };
                    #[cfg(not(unix))]
                    let shared = true;
                    if shared {
                        match ToctouGuard::capture(&path).map(|value| value.entity()) {
                            Some(identity) if !identity.is_unknown() => {
                                if !seen.insert(identity) {
                                    #[cfg(unix)]
                                    let bytes = {
                                        use std::os::unix::fs::MetadataExt;
                                        metadata.blocks().saturating_mul(512)
                                    };
                                    #[cfg(not(unix))]
                                    let bytes =
                                        super::get_allocated_size(&path).unwrap_or(metadata.len());
                                    possible_duplicate = possible_duplicate.saturating_add(bytes);
                                }
                            }
                            _ => unknown = true,
                        }
                    }
                } else if metadata.is_dir() {
                    if depth >= 32 {
                        unknown = true;
                        continue;
                    }
                    match std::fs::read_dir(&path) {
                        Ok(entries) => {
                            for entry in entries {
                                if pending.len() + visited >= limit {
                                    unknown = true;
                                    break;
                                }
                                match entry {
                                    Ok(entry) => pending.push((entry.path(), depth + 1)),
                                    Err(_) => unknown = true,
                                }
                            }
                        }
                        Err(_) => unknown = true,
                    }
                }
            }
            // Measurement may prune entries the identity audit sees. State a range,
            // never subtract an unproven contribution from a measured item/plan.
            // These explicitly observed application snapshots use APFS cloning.
            // Inode/link counts cannot establish their private physical blocks.
            let cloned_snapshot = item.signature_id == "system.code_sign_clones.observation";
            let uncertain = if unknown || cloned_snapshot {
                item.observed_bytes()
            } else {
                possible_duplicate.min(item.observed_bytes())
            };
            let uncertain = uncertain.min(
                category
                    .total_bytes
                    .saturating_sub(category.ambiguous_overlap_bytes),
            );
            if uncertain > 0 {
                category.ambiguous_overlap_count += 1;
                category.ambiguous_overlap_bytes =
                    category.ambiguous_overlap_bytes.saturating_add(uncertain);
                report.count += 1;
                report.bytes = report.bytes.saturating_add(uncertain);
            }
            report.incomplete |= unknown || cloned_snapshot;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, FileSize, ObservationQuality, RiskTier, ScanItem};

    #[test]
    fn application_clone_footprint_never_implies_unique_physical_recovery() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("clone");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("payload"), vec![1; 8192]).unwrap();
        let mut categories = vec![category(&root, "snapshot")];
        let item = &mut categories[0].items[0];
        item.signature_id = "system.code_sign_clones.observation".into();
        item.risk = RiskTier::Manual;
        item.rederive_disposition();
        let report = audit_bounded(
            &mut categories,
            100,
            Duration::from_secs(5),
            &neati_platform::PlatformEnvironment::simulated(
                neati_platform::path_algebra::PathFlavor::current(),
            )
            .with_home(fixture.path()),
        );
        assert_eq!(report.bytes, 8192);
        assert!(report.incomplete);
        assert_eq!(categories[0].ambiguous_overlap_bytes, 8192);
        assert!(!categories[0].items[0].allows_cleanup());
        assert!(root.exists());
    }

    fn category(path: &std::path::Path, name: &str) -> CategoryResult {
        let item = ScanItem::mock(
            name,
            name,
            name,
            Category::System,
            RiskTier::Safe,
            path.to_string_lossy(),
            FileSize::new(8192, Some(8192)),
            1,
        );
        CategoryResult {
            category: Category::System,
            display_name: name.into(),
            items: vec![item],
            total_bytes: 8192,
            cleanable_bytes: 8192,
            safe_bytes: 8192,
            rebuild_bytes: 0,
            manual_bytes: 0,
            quality: ObservationQuality::Fresh,
            skipped_entry_count: 0,
            incomplete_item_count: 0,
            eligibility: Default::default(),
            suppressed_duplicate_count: 0,
            suppressed_duplicate_bytes: 0,
            suppressed_overlap_count: 0,
            suppressed_overlap_bytes: 0,
            ambiguous_overlap_count: 0,
            ambiguous_overlap_bytes: 0,
        }
    }

    #[test]
    fn nested_hardlinks_across_categories_keep_actions_and_qualify_storage() {
        let fixture = tempfile::tempdir().unwrap();
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::fs::write(first.join("payload"), vec![1; 8192]).unwrap();
        std::fs::hard_link(first.join("payload"), second.join("alias")).unwrap();
        let mut categories = vec![category(&first, "one"), category(&second, "two")];
        categories[1].category = Category::Developer;
        let report = audit_bounded(
            &mut categories,
            100,
            Duration::from_secs(5),
            &neati_platform::PlatformEnvironment::native(),
        );
        assert!(!report.incomplete);
        assert_eq!(report.count, 1);
        assert_eq!(report.bytes, 8192);
        assert_eq!(categories[1].ambiguous_overlap_bytes, 8192);
        assert_eq!(categories.iter().map(|c| c.items.len()).sum::<usize>(), 2);
        assert!(categories.iter().all(|c| c.items[0].allows_cleanup()));
        assert!(first.join("payload").exists() && second.join("alias").exists());
    }

    #[test]
    fn exhausted_budget_reports_uncertainty_without_changing_authority() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("payload"), vec![1; 8192]).unwrap();
        let mut categories = vec![category(fixture.path(), "one")];
        let report = audit_bounded(
            &mut categories,
            0,
            Duration::from_secs(5),
            &neati_platform::PlatformEnvironment::native(),
        );
        assert!(report.incomplete);
        assert_eq!(report.bytes, 8192);
        assert_eq!(categories[0].total_bytes, 8192);
        assert_eq!(categories[0].cleanable_bytes, 8192);
        assert!(categories[0].items[0].is_selected);
    }
}
