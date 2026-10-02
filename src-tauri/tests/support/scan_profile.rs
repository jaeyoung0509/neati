//! Opt-in paired measurements. Every tree belongs to this test; no catalog,
//! user cache, cleanup plan or cleanup executor is used.
use super::*;

fn normalize_inventory(value: &mut serde_json::Value, root: &str) {
    match value {
        serde_json::Value::String(text) => *text = text.replace(root, "<fixture>"),
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_inventory(value, root);
            }
        }
        serde_json::Value::Object(fields) => {
            // Keep age satisfaction/policy and every range/eligibility/reason;
            // wall timestamps and process-specific identifiers are not facts
            // shared by separately constructed fixtures.
            for key in [
                "scan_id",
                "started_at",
                "finished_at",
                "last_modified",
                "newest_modified",
            ] {
                fields.remove(key);
            }
            for value in fields.values_mut() {
                normalize_inventory(value, root);
            }
        }
        _ => {}
    }
}

fn inventory(fixture: &Fixture, result: &ScanResult) -> serde_json::Value {
    let mut value = serde_json::to_value(result).expect("scan inventory serializes");
    let fields = value.as_object_mut().expect("scan is an object");
    fields.remove("spans");
    let metrics = fields
        .get_mut("metrics")
        .expect("metrics")
        .as_object_mut()
        .expect("metrics object");
    metrics.remove("duration_ms");
    metrics.remove("peak_outstanding_directory_tasks");
    normalize_inventory(&mut value, &fixture._directory.path().to_string_lossy());
    if let Ok(canonical) = fixture._directory.path().canonicalize() {
        normalize_inventory(&mut value, &canonical.to_string_lossy());
    }
    value
}

fn profile(fixture: &Fixture) {
    let mut expected = None;
    for iteration in 0..6 {
        let before_cpu = process_cpu_us();
        let before_rss = resident_bytes();
        let started = Instant::now();
        let mut first_root_us = None;
        let mut first_item_us = None;
        let mut events = Vec::new();
        let mut encoded_bytes = 0usize;
        let result = fixture.scan_with(&NeverCancelled, |event| {
            let kind = match &event {
                ScanEvent::Started { .. } => "started",
                ScanEvent::CategoryStarted { .. } => "category_started",
                ScanEvent::RootStarted { .. } => {
                    first_root_us.get_or_insert_with(|| started.elapsed().as_micros());
                    "root_started"
                }
                ScanEvent::ItemFound { .. } => {
                    first_item_us.get_or_insert_with(|| started.elapsed().as_micros());
                    "item_found"
                }
                ScanEvent::CategoryFinished { .. } => "category_finished",
                ScanEvent::Finished { .. } => "finished",
            };
            events.push(kind);
            encoded_bytes += serde_json::to_vec(&event).expect("event encodes").len();
        });
        let wall_us = started.elapsed().as_micros();
        let cpu_us = before_cpu
            .zip(process_cpu_us())
            .map(|(before, after)| after - before);
        let after_rss = resident_bytes();
        let row = metrics_row(fixture.name, &result, 0);
        assert_common_invariants(&row, ScanLimits::default().max_concurrent_directory_reads);
        assert!(row.duration_ms <= fixture.ceiling_ms);
        assert!(!result.cancelled);
        if fixture.name == "inaccessible" {
            #[cfg(unix)]
            assert_fixture_shape_inaccessible(&result);
        } else if fixture.name == "symlink" {
            #[cfg(unix)]
            assert_fixture_shape_symlink(&result);
        } else if fixture.name != "plain_observation" {
            assert_fixture_shape(fixture, &result);
        } else {
            assert_eq!(result.metrics.visited_entries, 2305);
            assert_eq!(logical_bytes(&result), 8 * 1024 * 1024);
            assert_eq!(result.cleanable_bytes, 0);
        }
        let semantic = inventory(fixture, &result);
        if let Some(expected) = &expected {
            assert_eq!(
                &semantic, expected,
                "repeated fixture semantics and ordering"
            );
        } else {
            expected = Some(semantic.clone());
        }
        println!(
            "controlled_scan_profile {}",
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"), "fixture": fixture.name,
                "iteration": iteration, "warmup": iteration == 0,
                "wall_us": wall_us, "cpu_us": cpu_us,
                "first_root_us": first_root_us, "first_item_us": first_item_us,
                "rss_before_bytes": before_rss, "rss_after_bytes": after_rss,
                "peak_tasks": result.metrics.peak_outstanding_directory_tasks,
                "event_order": events, "encoded_event_bytes": encoded_bytes,
                "inventory": semantic, "spans": result.spans,
            })
        );
    }

    // Boundary Stop: the root is announced before any of its entries are read.
    // The separate active aged-cancellation test also stops inside a tree.
    let flag = Arc::new(AtomicBool::new(false));
    let probe = Probe {
        flag: Arc::clone(&flag),
    };
    let mut stopped_at = None;
    let stopped = fixture.scan_with(&probe, |event| {
        if matches!(event, ScanEvent::RootStarted { .. }) && stopped_at.is_none() {
            stopped_at = Some(Instant::now());
            flag.store(true, Ordering::SeqCst);
        }
    });
    let stop_us = stopped_at
        .expect("a root was announced")
        .elapsed()
        .as_micros();
    assert!(stopped.cancelled);
    assert_eq!(stopped.metrics.visited_entries, 0);
    assert!(stop_us <= u128::from(CANCELLATION_LATENCY_CEILING_MS) * 1000);
    println!(
        "controlled_scan_stop {}",
        serde_json::json!({
            "fixture": fixture.name, "stop_us": stop_us,
            "request_boundary": "first_root_before_walk", "inventory": inventory(fixture, &stopped),
        })
    );
}

macro_rules! profile_case {
    ($name:ident, $factory:ident) => {
        #[test]
        #[ignore = "paired read-only disposable-fixture profiling; no user data"]
        fn $name() {
            let _measurement = isolate_fixture_measurement();
            let fixture = $factory();
            profile(&fixture);
        }
    };
}

profile_case!(wide, wide_fixture);
profile_case!(deep, deep_fixture);
profile_case!(mixed_size, mixed_size_fixture);
profile_case!(mixed_age, mixed_age_fixture);
profile_case!(aged_observation, aged_observation_fixture);
profile_case!(plain_observation, plain_observation_fixture);
profile_case!(overlap, overlapping_roots_fixture);

#[cfg(unix)]
#[test]
#[ignore = "paired read-only disposable-fixture profiling; no user data"]
fn link() {
    let _measurement = isolate_fixture_measurement();
    let directory = tempfile::tempdir().expect("fixture");
    write_file(&directory.path().join("outside/outside.bin"), 16384);
    let root = directory.path().join("symlink");
    write_file(&root.join("real/payload.bin"), 4096);
    std::os::unix::fs::symlink("../outside", root.join("escape")).expect("fixture link");
    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "benchmark.symlink",
        "Linked cache",
        Category::Developer,
        &[root],
        None,
        None,
        CleanStrategy::DeleteContents,
    ));
    let fixture = Fixture::new("symlink", 1250, directory, registry);
    profile(&fixture);
}

#[cfg(unix)]
#[test]
#[ignore = "paired read-only disposable-fixture profiling; no user data"]
fn inaccessible() {
    let _measurement = isolate_fixture_measurement();
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().expect("fixture");
    let root = directory.path().join("inaccessible");
    write_file(&root.join("readable.bin"), 4096);
    let locked = root.join("locked");
    write_file(&locked.join("secret.bin"), 8192);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .expect("lock fixture");
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let mut registry = SignatureRegistry::new();
    registry.register(signature(
        "benchmark.inaccessible",
        "Locked cache",
        Category::System,
        &[root],
        None,
        None,
        CleanStrategy::DeleteContents,
    ));
    // Restore before TempDir drop, even when a measurement assertion unwinds.
    let fixture = Fixture::new("inaccessible", 1250, directory, registry);
    let _restore = Restore(locked);
    profile(&fixture);
}
