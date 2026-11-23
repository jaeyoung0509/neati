//! Aged children measured on the shared pool are the children a serial walk
//! measures (#493): the same items, in the same order, with the same counts.

use super::DirectoryScanner;
use crate::applications::RunningApplications;
use crate::models::{
    Category, CleanStrategy, EligibilityGate, NeverCancelled, RiskTier, ScanItem, Signature,
};
use crate::scanner::observation::{NoRootProgress, ScanLimits, TraversalCounters, WalkContext};
use neati_platform::open_files::{FixedOpenFileProbe, OpenFileState};
use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;
use std::path::Path;
use std::sync::Arc;

fn backdate(path: &Path, days: u64) {
    super::tests::age_entry(path, days);
}

fn aged_signature(root: &Path, strategy: CleanStrategy) -> Signature {
    Signature {
        deletion_disposition: None,
        id: "system.test.pooled".into(),
        name: "Pooled aged caches".into(),
        category: Category::System,
        family: Default::default(),
        risk: RiskTier::Safe,
        strategy,
        paths: vec![root.to_string_lossy().into_owned()],
        exclusions: vec![root.join("excluded-by-path").to_string_lossy().into_owned()],
        description: "Pooled aged child fixture".into(),
        min_age_days: Some(3),
        include_prefixes: vec![],
        exclude_prefixes: vec!["skip-".into()],
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

/// Children of every shape the classification and the measurement decide:
/// stale and fresh trees, empty ones, nested ones, a protected bundle, a
/// structured-state name, links, and namespaces excluded by prefix and path.
fn fixture(root: &Path) {
    for index in 0..48 {
        let child = root.join(format!("cache-{index:02}"));
        for nested in 0..(index % 5) {
            let directory = child.join(format!("level-{nested}"));
            std::fs::create_dir_all(&directory).unwrap();
            for file in 0..(1 + index % 7) {
                let path = directory.join(format!("blob-{file}.bin"));
                std::fs::write(&path, vec![index as u8; 512 * (1 + file)]).unwrap();
                if index % 3 != 0 {
                    backdate(&path, 10);
                }
            }
            if index % 3 != 0 {
                backdate(&directory, 10);
            }
        }
        std::fs::create_dir_all(&child).unwrap();
        if index % 3 != 0 {
            backdate(&child, 10);
        }
    }
    std::fs::create_dir_all(root.join("cache-bundle/Tool.app/Contents")).unwrap();
    std::fs::write(root.join("cache-bundle/Tool.app/Contents/Info.plist"), b"x").unwrap();
    std::fs::create_dir_all(root.join("cache-db")).unwrap();
    std::fs::write(root.join("cache-db/index.sqlite"), vec![1; 2048]).unwrap();
    std::fs::create_dir_all(root.join("skip-owned/data")).unwrap();
    std::fs::write(root.join("skip-owned/data/payload"), vec![2; 4096]).unwrap();
    std::fs::create_dir_all(root.join("excluded-by-path")).unwrap();
    std::fs::write(root.join("excluded-by-path/payload"), vec![3; 4096]).unwrap();
    std::fs::write(root.join("loose-file.bin"), vec![4; 1024]).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("cache-01"), root.join("cache-link")).unwrap();
        std::os::unix::fs::symlink(root.join("missing"), root.join("cache-dangling")).unwrap();
    }
}

fn walk(signature: &Signature, pool: Option<&rayon::ThreadPool>) -> (Vec<ScanItem>, u64, u64) {
    // A stated home: the safety blacklist refuses every path when the
    // environment names none.
    let environment = PlatformEnvironment::simulated(PathFlavor::current()).with_home(
        if PathFlavor::current().is_windows() {
            r"Z:\NeatiFixtureHome"
        } else {
            "/neati-fixture-home"
        },
    );
    let counters = TraversalCounters::default();
    let context = WalkContext::new(
        &environment,
        &NeverCancelled,
        ScanLimits::default(),
        &counters,
        &NoRootProgress,
    );
    let running = RunningApplications::from_process_names(["fixture-idle".into()])
        .with_open_file_probe(Arc::new(FixedOpenFileProbe(OpenFileState::Idle)));
    let scan = DirectoryScanner::scan_signature_with_context(
        signature,
        pool,
        &context,
        EligibilityGate::Open,
        &running,
    );
    (
        scan.items,
        counters.visited_entries(),
        counters.directories_read(),
    )
}

/// The pooled walk reports exactly what the serial walk reports, for whole
/// tree age and for per-entry stale accounting alike.
#[test]
fn pooled_aged_children_match_the_serial_walk() {
    let fixture_root = tempfile::tempdir().unwrap();
    let root = fixture_root.path().join("aged-root");
    std::fs::create_dir_all(&root).unwrap();
    fixture(&root);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    for strategy in [
        CleanStrategy::DeleteDirectory,
        CleanStrategy::DeleteStaleContents,
    ] {
        let signature = aged_signature(&root, strategy);
        let serial = walk(&signature, None);
        let pooled = walk(&signature, Some(&pool));
        assert!(
            serial.0.len() > 48,
            "every child is inventoried: {}",
            serial.0.len()
        );
        assert!(serial.0.iter().any(|item| item.disposition.is_cleanable()));
        assert!(serial.0.iter().any(|item| !item.disposition.is_cleanable()));
        assert_eq!(pooled.0, serial.0, "{strategy:?} items and order");
        assert_eq!(
            (pooled.1, pooled.2),
            (serial.1, serial.2),
            "{strategy:?} visited entries and directory reads"
        );
    }
}

/// Paired timing of serial and pooled aged-child measurement. Run with
/// `cargo test -p neati-desktop --lib aged_children_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing evidence, not a CI threshold"]
fn aged_children_timing() {
    let fixture_root = tempfile::tempdir().unwrap();
    let root = fixture_root.path().join("aged-root");
    for child in 0..64 {
        for directory in 0..20 {
            let path = root.join(format!("cache-{child:02}/d{directory:02}"));
            std::fs::create_dir_all(&path).unwrap();
            for file in 0..20 {
                std::fs::write(path.join(format!("f{file:02}")), b"cache payload").unwrap();
            }
        }
    }
    let pool = crate::execution_budget::shared_scan_pool();
    let signature = aged_signature(&root, CleanStrategy::DeleteStaleContents);
    // One untimed walk so both measurements read a warm metadata cache.
    let expected = walk(&signature, None);
    for _ in 0..3 {
        let started = std::time::Instant::now();
        let serial = walk(&signature, None);
        let serial_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let started = std::time::Instant::now();
        let pooled = walk(&signature, pool);
        let pooled_ms = started.elapsed().as_secs_f64() * 1_000.0;
        assert_eq!(serial.0, expected.0);
        assert_eq!(pooled.0, expected.0);
        println!(
            "{{\"aged_children\":64,\"entries\":{},\"workers\":{},\"serial_ms\":{serial_ms:.1},\"pooled_ms\":{pooled_ms:.1}}}",
            serial.1,
            pool.map_or(1, rayon::ThreadPool::current_num_threads),
        );
    }
}
