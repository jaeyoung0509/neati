//! The use case against fake driven ports: no filesystem, no services.

use std::sync::{Arc, Mutex};

use crate::models::{
    ArtifactOwnershipEvidence, Category, CategoryResult, DeveloperArtifact, DeveloperArtifactKind,
    DeveloperArtifactScanResult, DeveloperArtifactStatus, DeveloperEcosystem, FileSize,
    ObservationQuality, RiskTier, ScanItem, ScanResult,
};
use crate::services::ports::{ActivityPort, PortFuture, Silent};

use super::ports::*;
use super::selection::project_reference;
use super::OneButtonClean;

type Calls = Mutex<Vec<(String, Vec<String>)>>;

struct FakeCaches {
    current: Option<ScanResult>,
    scans: Mutex<u32>,
    scan: ScanResult,
    chosen: Vec<String>,
    report: RemovalReport,
    removals: Calls,
}

impl CacheCleanup for FakeCaches {
    fn current(&self) -> Option<ScanResult> {
        self.current.clone()
    }

    fn scan(&self, _: Arc<dyn ActivityPort>) -> PortFuture<'_, Result<ScanResult, String>> {
        Box::pin(async move {
            *self.scans.lock().unwrap() += 1;
            Ok(self.scan.clone())
        })
    }

    fn one_button_ids(&self, _: &ScanResult) -> Result<Vec<String>, String> {
        Ok(self.chosen.clone())
    }

    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        _: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>> {
        Box::pin(async move {
            self.removals.lock().unwrap().push((scan_id, ids));
            Ok(self.report.clone())
        })
    }
}

struct FakeWorktrees {
    inventory: crate::models::WorktreeInventory,
    removals: Calls,
}

impl WorktreeCleanup for FakeWorktrees {
    fn current(&self) -> Option<crate::models::WorktreeInventory> {
        None
    }

    fn scan(
        &self,
        _: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<crate::models::WorktreeInventory, String>> {
        Box::pin(async move { Ok(self.inventory.clone()) })
    }

    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        _: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>> {
        Box::pin(async move {
            self.removals.lock().unwrap().push((scan_id, ids.clone()));
            Ok(RemovalReport {
                reclaimed_bytes: 5000,
                removed: ids,
                skipped: 0,
                failed: Vec::new(),
            })
        })
    }
}

fn worktree(
    id: &str,
    state: crate::models::WorktreeState,
    selected: bool,
) -> crate::models::WorktreeItem {
    crate::models::WorktreeItem {
        id: id.to_string(),
        repository: "/Users/me/code/app".into(),
        path: format!("/Users/me/code/{id}"),
        name: id.to_string(),
        branch: Some("feature".into()),
        head: "abc1234".into(),
        agent: None,
        allocated_bytes: 5000,
        file_count: 10,
        measurement_complete: true,
        last_activity: None,
        state,
        reason: None,
        selected_by_default: selected,
    }
}

fn no_worktrees() -> Arc<FakeWorktrees> {
    Arc::new(FakeWorktrees {
        inventory: crate::models::WorktreeInventory {
            scan_id: "worktrees-1".into(),
            repositories: 0,
            items: Vec::new(),
            refused: Vec::new(),
            idle_days: 14,
            rule_enabled: false,
        },
        removals: Mutex::new(Vec::new()),
    })
}

struct FakeProjects {
    inventory: ProjectInventory,
    report: RemovalReport,
    scans: Mutex<u32>,
    removals: Calls,
}

impl ProjectCleanup for FakeProjects {
    fn current(&self) -> Option<ProjectInventory> {
        None
    }

    fn scan(&self, _: Arc<dyn ActivityPort>) -> PortFuture<'_, Result<ProjectInventory, String>> {
        Box::pin(async move {
            *self.scans.lock().unwrap() += 1;
            Ok(self.inventory.clone())
        })
    }

    fn remove(
        &self,
        scan_id: String,
        ids: Vec<String>,
        _: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<RemovalReport, String>> {
        Box::pin(async move {
            self.removals.lock().unwrap().push((scan_id, ids));
            Ok(self.report.clone())
        })
    }
}

fn item(id: &str, bytes: u64) -> ScanItem {
    let mut item = ScanItem::mock(
        id,
        "test_sig",
        id,
        Category::System,
        RiskTier::Safe,
        format!("/Users/me/Library/Caches/{id}"),
        FileSize::new(bytes, Some(bytes)),
        1,
    );
    item.disposition = item.derive_disposition();
    item
}

fn scan(items: Vec<ScanItem>) -> ScanResult {
    let mut category = CategoryResult {
        category: Category::System,
        display_name: Category::System.display_name().to_string(),
        total_bytes: 0,
        cleanable_bytes: 0,
        safe_bytes: 0,
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
        items: Vec::new(),
    };
    for item in &items {
        category.eligibility.add(item);
    }
    let eligibility = category.eligibility.clone();
    category.items = items;
    ScanResult {
        cancelled: false,
        metrics: Default::default(),
        scan_id: "cache-scan".to_string(),
        valid_for_seconds: ScanResult::VALID_FOR_SECONDS,
        started_at: 0,
        finished_at: 0,
        total_bytes: 0,
        cleanable_bytes: 0,
        safe_bytes: 0,
        rebuild_bytes: 0,
        manual_bytes: 0,
        categories: vec![category],
        incomplete_reasons: Vec::new(),
        gaps: Vec::new(),
        spans: Vec::new(),
        quality: ObservationQuality::Fresh,
        skipped_entry_count: 0,
        incomplete_item_count: 0,
        eligibility,
        suppressed_duplicate_count: 0,
        suppressed_duplicate_bytes: 0,
        suppressed_overlap_count: 0,
        suppressed_overlap_bytes: 0,
        ambiguous_overlap_count: 0,
        ambiguous_overlap_bytes: 0,
    }
}

fn artifact(id: &str, path: &str, bytes: u64, selected_by_default: bool) -> DeveloperArtifact {
    DeveloperArtifact {
        id: id.to_string(),
        workspace_id: "workspace".to_string(),
        project_name: "app".to_string(),
        ecosystem: DeveloperEcosystem::Rust,
        kind: DeveloperArtifactKind::CargoTarget,
        path: path.to_string(),
        logical_bytes: bytes,
        allocated_bytes: bytes,
        file_count: 1,
        newest_mtime: None,
        rebuild_hint: None,
        evidence: Vec::new(),
        ownership: ArtifactOwnershipEvidence::VerifiedGenerated,
        status: DeveloperArtifactStatus::Complete,
        incomplete_reason: None,
        selected_by_default,
        project_last_activity: None,
        clean_inclusion: if selected_by_default {
            crate::models::CleanInclusion::Included
        } else {
            crate::models::CleanInclusion::NotOffered
        },
    }
}

struct World {
    caches: Arc<FakeCaches>,
    projects: Arc<FakeProjects>,
    use_case: OneButtonClean,
}

/// Caches `a`, `b`, `c` with the backend choosing `a` and `c`; one idle build
/// output and one dependency folder the backend did not preselect.
fn world() -> World {
    let caches = Arc::new(FakeCaches {
        current: None,
        scans: Mutex::new(0),
        scan: scan(vec![item("a", 100), item("b", 200), item("c", 300)]),
        chosen: vec!["a".to_string(), "c".to_string()],
        report: RemovalReport {
            reclaimed_bytes: 100,
            removed: vec!["a".to_string()],
            skipped: 0,
            failed: Vec::new(),
        },
        removals: Mutex::new(Vec::new()),
    });
    let projects = Arc::new(FakeProjects {
        inventory: ProjectInventory {
            folders: vec!["/Users/me/code".to_string()],
            refused: Vec::new(),
            result: Some(DeveloperArtifactScanResult {
                scan_id: "project-scan".to_string(),
                items: vec![
                    artifact("t1", "/Users/me/code/app/target", 1000, true),
                    artifact("n1", "/Users/me/code/app/node_modules", 50, false),
                ],
                discovered_count: 2,
                measured_count: 2,
                skipped_entries: 0,
                cancelled: false,
                truncated: false,
                uninspected: Vec::new(),
            }),
        },
        report: RemovalReport {
            reclaimed_bytes: 1000,
            removed: vec!["t1".to_string()],
            skipped: 0,
            failed: Vec::new(),
        },
        scans: Mutex::new(0),
        removals: Mutex::new(Vec::new()),
    });
    let use_case = OneButtonClean::new(caches.clone(), projects.clone(), no_worktrees());
    World {
        caches,
        projects,
        use_case,
    }
}

fn preview(world: &World, request: CleanRequest) -> Result<CleanPreview, OneButtonError> {
    tauri::async_runtime::block_on(world.use_case.preview(request, Arc::new(Silent)))
}

fn references(preview: &CleanPreview) -> Vec<&str> {
    preview
        .chosen
        .iter()
        .map(|candidate| candidate.reference.as_str())
        .collect()
}

#[test]
fn the_preview_is_exactly_the_backend_set_and_removes_nothing() {
    let world = world();
    let preview = preview(&world, CleanRequest::default()).expect("preview");
    let target = project_reference("/Users/me/code/app/target");
    assert_eq!(references(&preview), ["a", "c", target.as_str()]);
    assert_eq!(preview.total_bytes, 100 + 300 + 1000);
    // The dependency folder is reported for review, never chosen.
    assert_eq!(preview.review.items, 1);
    assert_eq!(preview.review.bytes, 50);
    assert!(world.caches.removals.lock().unwrap().is_empty());
    assert!(world.projects.removals.lock().unwrap().is_empty());
}

#[test]
fn an_unknown_reference_refuses_before_any_removal() {
    let world = world();
    let refused = preview(
        &world,
        CleanRequest {
            only: vec!["b".to_string()],
            ..CleanRequest::default()
        },
    );
    // `b` exists in the scan, but the backend did not choose it.
    assert_eq!(
        refused.expect_err("not in the set"),
        OneButtonError::UnknownReferences(vec!["b".to_string()])
    );
    assert!(world.caches.removals.lock().unwrap().is_empty());
}

#[test]
fn execution_submits_only_the_chosen_ids_with_their_own_scan() {
    let world = world();
    let preview = preview(
        &world,
        CleanRequest {
            skip: vec!["c".to_string()],
            ..CleanRequest::default()
        },
    )
    .expect("preview");
    let outcome =
        tauri::async_runtime::block_on(world.use_case.execute(&preview, Arc::new(Silent)))
            .expect("execute");
    assert_eq!(
        *world.caches.removals.lock().unwrap(),
        [("cache-scan".to_string(), vec!["a".to_string()])]
    );
    assert_eq!(
        *world.projects.removals.lock().unwrap(),
        [("project-scan".to_string(), vec!["t1".to_string()])]
    );
    assert_eq!(outcome.reclaimed_bytes, 1100);
    assert_eq!(outcome.removed, 2);
    assert!(outcome.failed.is_empty());
}

#[test]
fn failures_come_back_named_by_reference() {
    let mut world = world();
    let caches = Arc::new(FakeCaches {
        report: RemovalReport {
            reclaimed_bytes: 0,
            removed: Vec::new(),
            skipped: 1,
            failed: vec![("a".to_string(), "Locked by its owner.".to_string())],
        },
        current: None,
        scans: Mutex::new(0),
        scan: world.caches.scan.clone(),
        chosen: vec!["a".to_string()],
        removals: Mutex::new(Vec::new()),
    });
    world.use_case = OneButtonClean::new(caches, world.projects.clone(), no_worktrees());
    let preview = preview(
        &world,
        CleanRequest {
            projects: false,
            ..CleanRequest::default()
        },
    )
    .expect("preview");
    let outcome =
        tauri::async_runtime::block_on(world.use_case.execute(&preview, Arc::new(Silent)))
            .expect("execute");
    assert_eq!(outcome.skipped, 1);
    assert_eq!(
        outcome.failed,
        [UnitFailure {
            reference: "a".to_string(),
            name: "a".to_string(),
            message: "Locked by its owner.".to_string(),
        }]
    );
}

#[test]
fn a_source_left_out_is_never_scanned_or_touched() {
    let world = world();
    let preview = preview(
        &world,
        CleanRequest {
            projects: false,
            ..CleanRequest::default()
        },
    )
    .expect("preview");
    assert_eq!(*world.projects.scans.lock().unwrap(), 0);
    assert!(preview.projects.is_none());
    tauri::async_runtime::block_on(world.use_case.execute(&preview, Arc::new(Silent)))
        .expect("execute");
    assert!(world.projects.removals.lock().unwrap().is_empty());
}

#[test]
fn an_empty_selection_executes_nothing() {
    let world = world();
    let target = project_reference("/Users/me/code/app/target");
    let preview = preview(
        &world,
        CleanRequest {
            skip: vec!["a".to_string(), "c".to_string(), target],
            ..CleanRequest::default()
        },
    )
    .expect("preview");
    assert!(preview.chosen.is_empty());
    let outcome =
        tauri::async_runtime::block_on(world.use_case.execute(&preview, Arc::new(Silent)))
            .expect("execute");
    assert_eq!(outcome, CleanOutcome::default());
    assert!(world.caches.removals.lock().unwrap().is_empty());
    assert!(world.projects.removals.lock().unwrap().is_empty());
}

#[test]
fn reuse_takes_the_current_scan_and_only_then() {
    let mut world = world();
    let mut held = world.caches.scan.clone();
    held.scan_id = "held-scan".to_string();
    let caches = Arc::new(FakeCaches {
        current: Some(held),
        scans: Mutex::new(0),
        scan: world.caches.scan.clone(),
        chosen: vec!["a".to_string()],
        report: RemovalReport::default(),
        removals: Mutex::new(Vec::new()),
    });
    world.use_case = OneButtonClean::new(caches.clone(), world.projects.clone(), no_worktrees());

    let reused = preview(
        &world,
        CleanRequest {
            reuse_current: true,
            ..CleanRequest::default()
        },
    )
    .expect("preview");
    assert_eq!(reused.caches.expect("caches").scan_id, "held-scan");
    assert_eq!(
        *caches.scans.lock().unwrap(),
        0,
        "a valid held scan is reused"
    );

    let fresh = preview(&world, CleanRequest::default()).expect("preview");
    assert_eq!(fresh.caches.expect("caches").scan_id, "cache-scan");
    assert_eq!(*caches.scans.lock().unwrap(), 1, "without reuse it scans");
}

fn sessions(ttl: std::time::Duration) -> (Arc<FakeCaches>, super::sessions::OneButtonSessions) {
    let World {
        caches, use_case, ..
    } = world();
    (
        caches,
        super::sessions::OneButtonSessions::with_ttl(Arc::new(use_case), ttl),
    )
}

fn caches_request() -> CleanRequest {
    CleanRequest {
        projects: false,
        ..CleanRequest::default()
    }
}

#[test]
fn a_held_preview_runs_once() {
    let (caches, sessions) = sessions(super::sessions::PREVIEW_TTL);
    let (id, shown) =
        tauri::async_runtime::block_on(sessions.preview(caches_request(), Arc::new(Silent)))
            .expect("preview");
    assert_eq!(shown.chosen.len(), 2);
    tauri::async_runtime::block_on(sessions.execute(id, Arc::new(Silent))).expect("first run");
    assert_eq!(
        tauri::async_runtime::block_on(sessions.execute(id, Arc::new(Silent))),
        Err(OneButtonError::PreviewExpired),
        "a replayed id must not run again"
    );
    assert_eq!(caches.removals.lock().unwrap().len(), 1);
    assert_eq!(sessions.held_count(), 0);
}

#[test]
fn an_expired_or_unknown_preview_removes_nothing() {
    let (caches, sessions) = sessions(std::time::Duration::ZERO);
    let (id, _) =
        tauri::async_runtime::block_on(sessions.preview(caches_request(), Arc::new(Silent)))
            .expect("preview");
    for submitted in [id, uuid::Uuid::new_v4()] {
        assert_eq!(
            tauri::async_runtime::block_on(sessions.execute(submitted, Arc::new(Silent))),
            Err(OneButtonError::PreviewExpired)
        );
    }
    assert!(caches.removals.lock().unwrap().is_empty());
}

#[test]
fn the_preview_store_is_capped() {
    let (_, sessions) = sessions(super::sessions::PREVIEW_TTL);
    for _ in 0..(super::sessions::PREVIEW_CAP + 3) {
        tauri::async_runtime::block_on(sessions.preview(caches_request(), Arc::new(Silent)))
            .expect("preview");
    }
    assert_eq!(sessions.held_count(), super::sessions::PREVIEW_CAP);
}

#[test]
fn finished_worktrees_join_the_set_only_when_the_rule_selected_them() {
    use crate::models::WorktreeState;
    let mut world = world();
    let worktrees = Arc::new(FakeWorktrees {
        inventory: crate::models::WorktreeInventory {
            scan_id: "worktrees-1".into(),
            repositories: 1,
            items: vec![
                worktree("worktree:aaaaaaaaaaaa", WorktreeState::Finished, true),
                worktree("worktree:bbbbbbbbbbbb", WorktreeState::Finished, false),
                worktree("worktree:cccccccccccc", WorktreeState::HasChanges, false),
            ],
            refused: Vec::new(),
            idle_days: 14,
            rule_enabled: true,
        },
        removals: Mutex::new(Vec::new()),
    });
    world.use_case = OneButtonClean::new(
        world.caches.clone(),
        world.projects.clone(),
        worktrees.clone(),
    );
    let preview = preview(
        &world,
        CleanRequest {
            caches: false,
            projects: false,
            ..CleanRequest::default()
        },
    )
    .expect("preview");
    assert_eq!(references(&preview), ["worktree:aaaaaaaaaaaa"]);
    // The finished one the rule left out waits for review; a dirty one is
    // not a pending decision.
    assert_eq!(preview.review.items, 1);
    assert_eq!(preview.review.bytes, 5000);
    assert!(worktrees.removals.lock().unwrap().is_empty());

    let outcome =
        tauri::async_runtime::block_on(world.use_case.execute(&preview, Arc::new(Silent)))
            .expect("execute");
    assert_eq!(outcome.removed, 1);
    assert_eq!(outcome.reclaimed_bytes, 5000);
    assert_eq!(
        worktrees.removals.lock().unwrap().as_slice(),
        [(
            "worktrees-1".to_string(),
            vec!["worktree:aaaaaaaaaaaa".to_string()]
        )]
    );

    let skipped = preview_with_skip(&world, "worktree:aaaaaaaaaaaa");
    assert!(skipped.chosen.is_empty());
}

fn preview_with_skip(world: &World, reference: &str) -> CleanPreview {
    preview(
        world,
        CleanRequest {
            caches: false,
            projects: false,
            skip: vec![reference.to_string()],
            ..CleanRequest::default()
        },
    )
    .expect("narrowed preview")
}
