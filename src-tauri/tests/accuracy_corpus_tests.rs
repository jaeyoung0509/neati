//! Labelled accuracy corpus (#431, proposal A).
//!
//! The safety tests prove individual guards. This suite measures the result:
//! it builds a home directory whose every file carries a ground-truth label,
//! runs the real scan, plan and execution path over it, and counts
//!
//! - **false positives**: a `MustKeep` file that cleanup removed, separately for
//!   the preselected set (what Clean does with no further choice) and for the
//!   wider offered set; and
//! - **false negatives**: `Regenerable` bytes still on disk afterwards, with
//!   the reason neati left them.
//!
//! The preselected false-positive count is enforced to be zero. The other
//! numbers are pinned to the baselines below, so a catalog or policy change
//! has to state its effect on them in the same commit.
//!
//! Every owner in the corpus is fictitious (`com.example.*`) or a tool that is
//! declared missing, so the verdicts do not depend on what happens to be
//! running on the machine that executes the test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use neati_core::domain::platform::PlatformKind;
use neati_lib::cleaner::{CleanExecutor, LifecycleProviderRegistry, OwnerProviderRegistry};
use neati_lib::models::{NeverCancelled, ScanItem, ScanResult};
use neati_lib::safety::SafetyPlanner;
use neati_lib::scanner::ScanEngine;
use neati_lib::signatures::SignatureRegistry;
use neati_platform::{PathFlavor, PlatformEnvironment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Truth {
    /// User state, credentials, source, or anything not regenerable.
    MustKeep,
    /// Safe to remove at any time; its owner rebuilds or re-downloads it.
    Regenerable,
}

struct Entry {
    /// Path relative to the fixture home.
    path: &'static str,
    bytes: usize,
    truth: Truth,
    /// Why the label is what it is.
    #[allow(dead_code)]
    note: &'static str,
}

const fn keep(path: &'static str, note: &'static str) -> Entry {
    Entry {
        path,
        bytes: 4096,
        truth: Truth::MustKeep,
        note,
    }
}

const fn regen(path: &'static str, bytes: usize, note: &'static str) -> Entry {
    Entry {
        path,
        bytes,
        truth: Truth::Regenerable,
        note,
    }
}

const KB: usize = 1024;

/// The corpus. Add a row when a layout is found in the wild that neati gets
/// wrong in either direction; the baselines below then move with it.
const CORPUS: &[Entry] = &[
    // --- User content and credentials -------------------------------------
    keep("Documents/thesis.docx", "user document"),
    keep("Desktop/notes.txt", "user document"),
    keep("Downloads/installer.dmg", "user download"),
    keep(".ssh/id_ed25519", "credential"),
    keep(".gitconfig", "configuration"),
    keep("Projects/app/src/main.rs", "source code"),
    keep("Projects/app/.git/config", "repository metadata"),
    // --- AI tool state beside its caches ----------------------------------
    keep(".claude/settings.json", "tool configuration"),
    keep(".claude/projects/app/session.jsonl", "session transcript"),
    keep(".codex/auth.json", "credential"),
    keep(".codex/sessions/2026/rollout.jsonl", "session transcript"),
    regen(
        ".claude/cache/blob.bin",
        64 * KB,
        "CLI cache, reported for review only",
    ),
    regen(
        ".codex/cache/blob.bin",
        64 * KB,
        "CLI cache, reported for review only",
    ),
    // --- Application state that must survive a cache clean ----------------
    keep(
        "Library/Application Support/com.example.notes/notes.sqlite",
        "application database",
    ),
    keep(
        "Library/Preferences/com.example.editor.plist",
        "preferences",
    ),
    keep("Library/Keychains/login.keychain-db", "keychain"),
    // --- Generic application caches ---------------------------------------
    regen(
        "Library/Caches/com.example.editor/blobs/a.bin",
        256 * KB,
        "idle third-party cache payload",
    ),
    regen(
        "Library/Caches/com.example.viewer/thumbnails/t.bin",
        128 * KB,
        "idle third-party cache payload",
    ),
    keep(
        "Library/Caches/com.example.editor/state.sqlite",
        "structured state kept inside a cache namespace",
    ),
    keep(
        "Library/Caches/com.example.editor/state.sqlite-wal",
        "database companion",
    ),
    keep("Library/Caches/com.example.editor/app.lock", "lock file"),
    keep(
        "Library/Caches/CloudKit/com.example.sync/Records.db",
        "protected Apple namespace",
    ),
    // --- Logs ---------------------------------------------------------------
    regen(
        "Library/Logs/com.example.editor/app.log",
        32 * KB,
        "ordinary application log",
    ),
    // --- Developer stores ---------------------------------------------------
    keep(
        "Library/Developer/Xcode/Archives/2026-11-01/App.xcarchive/Info.plist",
        "release archive, inventory only",
    ),
    regen(
        "Library/Developer/Xcode/DerivedData/App-abc/Build/Intermediates/a.o",
        512 * KB,
        "rebuildable build output",
    ),
    regen(
        "Library/Developer/Xcode/DerivedData/App-abc/Build/Products/Debug/App.app/Contents/MacOS/App",
        128 * KB,
        "built product, rebuilt by Xcode",
    ),
    regen(
        "Library/Developer/Xcode/DerivedData/App-abc/Index.noindex/DataStore/index.sqlite",
        64 * KB,
        "index database, rebuilt by Xcode",
    ),
    regen(
        "Library/Developer/Xcode/iOS DeviceSupport/17.0 (21A329)/Symbols/usr/lib/libobjc.A.dylib",
        256 * KB,
        "device symbol dump, re-downloaded when the device connects",
    ),
    keep(
        "Library/Developer/Xcode/UserData/FontAndColorThemes/Mine.xccolortheme",
        "Xcode user data beside DerivedData",
    ),
];

struct Fixture {
    home: tempfile::TempDir,
    environment: PlatformEnvironment,
    registry: SignatureRegistry,
}

fn fixture() -> Fixture {
    let home = tempfile::tempdir().expect("fixture home");
    // Old enough that no age gate is the reason a file is kept.
    let old = SystemTime::now() - Duration::from_secs(120 * 24 * 60 * 60);
    for entry in CORPUS {
        let path = home.path().join(entry.path);
        fs::create_dir_all(path.parent().expect("entry has a parent")).expect("fixture dir");
        fs::write(&path, vec![0xA5u8; entry.bytes]).expect("fixture file");
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(old))
            .expect("fixture mtime");
    }
    age_directories(home.path(), old);
    let temp = home.path().join("private-tmp");
    fs::create_dir_all(&temp).expect("fixture temp");

    let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
        .with_platform(PlatformKind::Macos)
        .with_home(home.path())
        .with_temp_dir(&temp)
        .with_missing_tool("docker")
        .with_missing_tool("npm")
        .with_missing_tool("pnpm")
        .with_missing_tool("uv")
        .with_missing_tool("brew")
        .with_missing_tool("cargo")
        .with_missing_tool("go")
        .with_missing_tool("pip")
        .with_missing_tool("pod")
        .with_missing_tool("xcrun");
    let registry = SignatureRegistry::load_embedded_with(&environment).expect("embedded catalog");
    Fixture {
        home,
        environment,
        registry,
    }
}

/// Directory mtimes feed whole-tree inactivity rules, so they are aged too.
fn age_directories(root: &Path, when: SystemTime) {
    let mut stack = vec![root.to_path_buf()];
    let mut directories = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(entry.path());
            }
        }
        directories.push(dir);
    }
    for dir in directories {
        if let Ok(handle) = fs::File::open(&dir) {
            let _ = handle.set_modified(when);
        }
    }
}

fn scan(fixture: &Fixture) -> ScanResult {
    ScanEngine::scan(
        &fixture.registry,
        &LifecycleProviderRegistry::new(Vec::new()),
        &OwnerProviderRegistry::new(Vec::new()),
        None,
        &[],
        true,
        &fixture.environment,
        &NeverCancelled,
        |_| {},
    )
}

fn items(result: &ScanResult) -> Vec<ScanItem> {
    result
        .categories
        .iter()
        .flat_map(|category| category.items.clone())
        .collect()
}

/// Plans and executes exactly `selected`, as the application would.
///
/// Returns the signature ids whose unit the planner refused. A refusal is the
/// system working, but when the scan offered that unit it is also a
/// disagreement the person sees as "ready to clean" bytes that Clean leaves.
fn clean(fixture: &Fixture, selected: &[ScanItem]) -> Vec<String> {
    let owner_providers = OwnerProviderRegistry::new(Vec::new());
    let mut refused = Vec::new();
    for item in selected {
        let plan = match SafetyPlanner::create_plan_with_environment(
            std::slice::from_ref(item),
            &fixture.registry,
            &fixture.environment,
            &owner_providers,
        ) {
            Ok(plan) => plan,
            Err(_) => {
                refused.push(item.signature_id.clone());
                continue;
            }
        };
        CleanExecutor::execute(
            plan,
            &fixture.environment,
            &LifecycleProviderRegistry::new(Vec::new()),
            &owner_providers,
            &neati_platform::MockTrashBackend::new(),
            |_| {},
        );
    }
    refused
}

fn present(fixture: &Fixture, entry: &Entry) -> bool {
    fixture.home.path().join(entry.path).is_file()
}

fn removed(fixture: &Fixture, truth: Truth) -> Vec<&'static str> {
    CORPUS
        .iter()
        .filter(|entry| entry.truth == truth && !present(fixture, entry))
        .map(|entry| entry.path)
        .collect()
}

/// Why a regenerable file is still on disk, from the scan that saw it.
fn retention_reason(
    fixture: &Fixture,
    entry: &Entry,
    scanned: &[ScanItem],
    refused: &[String],
) -> String {
    let path = fixture.home.path().join(entry.path);
    let covering: Vec<&ScanItem> = scanned
        .iter()
        .filter(|item| path.starts_with(PathBuf::from(&item.path)))
        .collect();
    match covering
        .iter()
        .max_by_key(|item| PathBuf::from(&item.path).components().count())
    {
        None => "no catalog entry".to_string(),
        // The eligibility is left out of this key: a running owner turns
        // `AutoCleanable` into `Reviewable` on the host, and both are offered.
        Some(item) if refused.contains(&item.signature_id) => format!(
            "{} · offered in the scan, refused by the planner",
            item.signature_id
        ),
        Some(item) => format!("{} · {:?}", item.signature_id, item.disposition.eligibility),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Accuracy {
    preselected_false_positives: Vec<&'static str>,
    offered_false_positives: Vec<&'static str>,
    /// Regenerable bytes left on disk, by reason.
    false_negative_bytes: BTreeMap<String, usize>,
    /// Bytes the scan offered as cleanable whose unit the planner then
    /// refused. The interface counted them as ready; Clean left them.
    offered_but_refused_bytes: usize,
    reclaimed_regenerable_bytes: usize,
}

fn measure() -> Accuracy {
    let fixture = fixture();
    let scanned = items(&scan(&fixture));

    // Phase 1: what Clean does with no further choice.
    let preselected: Vec<ScanItem> = scanned
        .iter()
        .filter(|item| item.disposition.eligibility.is_auto_cleanable())
        .cloned()
        .collect();
    let mut refused = clean(&fixture, &preselected);
    let preselected_false_positives = removed(&fixture, Truth::MustKeep);

    // Phase 2: everything else neati offers when the person selects it.
    let offered: Vec<ScanItem> = items(&scan(&fixture))
        .into_iter()
        .filter(|item| item.disposition.is_cleanable())
        .collect();
    refused.extend(clean(&fixture, &offered));
    let offered_false_positives: Vec<&'static str> = removed(&fixture, Truth::MustKeep)
        .into_iter()
        .filter(|path| !preselected_false_positives.contains(path))
        .collect();

    let mut false_negative_bytes = BTreeMap::new();
    let mut reclaimed_regenerable_bytes = 0;
    let mut offered_but_refused_bytes = 0;
    for entry in CORPUS
        .iter()
        .filter(|entry| entry.truth == Truth::Regenerable)
    {
        if present(&fixture, entry) {
            let reason = retention_reason(&fixture, entry, &scanned, &refused);
            if reason.ends_with("refused by the planner") {
                offered_but_refused_bytes += entry.bytes;
            }
            *false_negative_bytes.entry(reason).or_insert(0) += entry.bytes;
        } else {
            reclaimed_regenerable_bytes += entry.bytes;
        }
    }
    Accuracy {
        preselected_false_positives,
        offered_false_positives,
        false_negative_bytes,
        offered_but_refused_bytes,
        reclaimed_regenerable_bytes,
    }
}

#[test]
fn preselected_cleanup_never_removes_a_must_keep_file() {
    let accuracy = measure();
    assert!(
        accuracy.preselected_false_positives.is_empty(),
        "Clean removed files labelled MustKeep without any selection: {:?}",
        accuracy.preselected_false_positives
    );
}

#[test]
fn accuracy_matches_the_recorded_baseline() {
    let accuracy = measure();
    eprintln!("accuracy corpus: {accuracy:#?}");
    assert_eq!(accuracy, baseline());
}

/// The numbers a catalog or policy change must restate.
///
/// - No `MustKeep` file is removed, preselected or offered.
/// - The two generic application caches and the application log are reclaimed.
///   The database, its WAL and the lock file inside the cache namespace stay.
/// - Claude and Codex CLI caches are advisory by design (review only).
/// - 2026-11-22, #350: Xcode DerivedData is reclaimed, including object files,
///   a built app bundle and the index database. Before that change the scan
///   offered it and the planner refused it (512 KB offered but refused).
/// - **Known disagreement:** an Xcode DeviceSupport symbol set is offered by
///   the scan but refused by the planner, because it contains `.dylib` files
///   and its signature keeps the default structured-state policy. The owner
///   approved the build-output policy for DerivedData only; extending it to
///   DeviceSupport is a separate decision (#350).
fn baseline() -> Accuracy {
    Accuracy {
        preselected_false_positives: Vec::new(),
        offered_false_positives: Vec::new(),
        false_negative_bytes: BTreeMap::from([
            ("ai.claude.cache · Advisory".to_string(), 64 * KB),
            ("ai.codex.cache · Advisory".to_string(), 64 * KB),
            (
                "system.xcode_device_support · offered in the scan, refused by the planner"
                    .to_string(),
                256 * KB,
            ),
        ]),
        offered_but_refused_bytes: 256 * KB,
        reclaimed_regenerable_bytes: (256 + 128 + 32 + 512 + 128 + 64) * KB,
    }
}
