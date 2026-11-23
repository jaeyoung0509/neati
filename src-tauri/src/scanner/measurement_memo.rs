//! Measurements one scan has already taken (#493).
//!
//! Two catalog rules can name the same root: an observation-only entry and a
//! second entry for the same tool's store, for example. Both used to walk the
//! whole tree. When the root and every parameter that decides the measurement
//! are the same — exclusions, the application-bundle rule, the depth bound —
//! the second rule reads what the first one measured instead. The overlap
//! pass still sees both rows and still folds them exactly as before.
//!
//! The memo belongs to one scan and is dropped with it. It holds observations,
//! never authorization: planning and execution measure and verify the live
//! filesystem themselves. A measurement the scan's cancellation cut short is
//! not kept, so a stopped scan never hands its partial walk to another rule.

use super::observation::ScanLimits;
use super::size::PathMeasurement;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The most roots one scan remembers. A scan names a few hundred fixed roots;
/// past the cap a root is simply measured again.
const MAX_REMEMBERED_ROOTS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MeasurementKey {
    path: PathBuf,
    exclusions: Vec<String>,
    permit_application_bundles: bool,
    max_depth: usize,
}

#[derive(Debug, Default)]
pub struct MeasurementMemo {
    measured: Mutex<HashMap<MeasurementKey, PathMeasurement>>,
}

impl MeasurementMemo {
    /// The measurement of `path` under these parameters: the one this scan
    /// already took, or `measure()`'s, remembered when it ran to completion
    /// of its walk.
    pub(crate) fn measure_path(
        &self,
        path: &Path,
        exclusions: &[String],
        permit_application_bundles: bool,
        limits: ScanLimits,
        cancellation: &dyn crate::models::CancellationProbe,
        measure: impl FnOnce() -> PathMeasurement,
    ) -> PathMeasurement {
        let key = MeasurementKey {
            path: path.to_path_buf(),
            exclusions: exclusions.to_vec(),
            permit_application_bundles,
            max_depth: limits.max_depth,
        };
        if let Some(measured) = self
            .measured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return measured.clone();
        }
        let measurement = measure();
        if !cancellation.is_cancelled() {
            let mut measured = self
                .measured
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if measured.len() < MAX_REMEMBERED_ROOTS {
                measured.insert(key, measurement.clone());
            }
        }
        measurement
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.measured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::MeasurementMemo;
    use crate::applications::RunningApplications;
    use crate::models::{
        CancellationProbe, Category, CleanStrategy, EligibilityGate, NeverCancelled, RiskTier,
        ScanItem, Signature,
    };
    use crate::scanner::WalkContext;
    use crate::scanner::{DirectoryScanner, NoRootProgress, ScanLimits, TraversalCounters};
    use neati_platform::path_algebra::PathFlavor;
    use neati_platform::PlatformEnvironment;
    use std::path::Path;

    fn environment() -> PlatformEnvironment {
        PlatformEnvironment::simulated(PathFlavor::current()).with_home(
            if PathFlavor::current().is_windows() {
                r"Z:\NeatiFixtureHome"
            } else {
                "/neati-fixture-home"
            },
        )
    }

    fn observation(id: &str, root: &Path, exclusions: Vec<String>) -> Signature {
        Signature {
            deletion_disposition: None,
            id: id.into(),
            name: id.into(),
            category: Category::Developer,
            family: Default::default(),
            risk: RiskTier::Manual,
            strategy: CleanStrategy::Manual,
            paths: vec![root.to_string_lossy().into_owned()],
            exclusions,
            description: String::new(),
            min_age_days: None,
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

    fn walk(
        signature: &Signature,
        memo: Option<&MeasurementMemo>,
        counters: &TraversalCounters,
        cancellation: &dyn CancellationProbe,
    ) -> Vec<ScanItem> {
        let environment = environment();
        let mut context = WalkContext::new(
            &environment,
            cancellation,
            ScanLimits::default(),
            counters,
            &NoRootProgress,
        );
        if let Some(memo) = memo {
            context = context.with_measurement_memo(memo);
        }
        DirectoryScanner::scan_signature_with_context(
            signature,
            None,
            &context,
            EligibilityGate::Open,
            &RunningApplications::from_process_names(["fixture-idle".into()]),
        )
        .items
    }

    /// A root two rules name with the same parameters is walked once, and the
    /// second rule's row is the row it would have measured itself. A rule with
    /// different exclusions measures its own walk.
    #[test]
    fn a_shared_root_is_walked_once_and_reported_unchanged() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("ms-playwright");
        for browser in 0..4 {
            let directory = root.join(format!("browser-{browser}/bin"));
            std::fs::create_dir_all(&directory).unwrap();
            for file in 0..16 {
                std::fs::write(directory.join(format!("part-{file}")), vec![7; 1_024]).unwrap();
            }
        }
        let first = observation("dev.first", &root, Vec::new());
        let second = observation("dev.second", &root, Vec::new());
        let narrower = observation("dev.narrower", &root, vec!["browser-0".into()]);

        let memo = MeasurementMemo::default();
        let counters = TraversalCounters::default();
        walk(&first, Some(&memo), &counters, &NeverCancelled);
        let one_walk = counters.visited_entries();
        assert!(one_walk > 64);
        let reused = walk(&second, Some(&memo), &counters, &NeverCancelled);
        assert_eq!(
            counters.visited_entries(),
            one_walk,
            "the second rule reads the first rule's measurement"
        );
        let fresh = walk(
            &second,
            None,
            &TraversalCounters::default(),
            &NeverCancelled,
        );
        assert_eq!(
            reused, fresh,
            "the reused row is the row a fresh walk reports"
        );
        assert_eq!(reused[0].size.logical, 4 * 16 * 1_024);

        let narrowed = walk(&narrower, Some(&memo), &counters, &NeverCancelled);
        assert!(
            counters.visited_entries() > one_walk,
            "other exclusions walk again"
        );
        assert_eq!(narrowed[0].size.logical, 3 * 16 * 1_024);
        assert_eq!(memo.len(), 2);
    }

    /// A walk the scan's cancellation stopped is not handed to another rule.
    #[test]
    fn a_cancelled_measurement_is_not_remembered() {
        struct Stopped;
        impl CancellationProbe for Stopped {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("store");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/payload"), vec![1; 4_096]).unwrap();
        let memo = MeasurementMemo::default();
        let signature = observation("dev.store", &root, Vec::new());
        walk(
            &signature,
            Some(&memo),
            &TraversalCounters::default(),
            &Stopped,
        );
        assert_eq!(memo.len(), 0);
        let measured = walk(
            &signature,
            Some(&memo),
            &TraversalCounters::default(),
            &NeverCancelled,
        );
        assert_eq!(measured[0].size.logical, 4_096);
        assert_eq!(memo.len(), 1);
    }
}
