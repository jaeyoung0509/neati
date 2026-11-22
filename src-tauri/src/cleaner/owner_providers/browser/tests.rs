use super::*;
use crate::scanner::SizeCalculatorMeasurement;
use std::sync::Mutex;

fn arc_bundle(environment: &PlatformEnvironment) -> PathBuf {
    let app = environment
        .user_home()
        .unwrap()
        .join("Applications/Arc.app");
    let fields = [
        (
            "Contents/Info.plist",
            vec![
                ("CFBundleName", "Arc"),
                ("CFBundleIdentifier", "company.thebrowser.Browser"),
                ("CFBundleShortVersionString", "1.166.0"),
                ("CFBundleVersion", "87668"),
                ("CFBundleExecutable", "Arc"),
            ],
        ),
        (
            "Contents/Frameworks/ArcCore.framework/Versions/A/Resources/Info.plist",
            vec![
                ("CFBundleIdentifier", "company.thebrowser.browser.framework"),
                ("CFBundleShortVersionString", "154.0.8037.58"),
            ],
        ),
    ];
    for (name, values) in fields {
        let path = app.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let dictionary = values
            .into_iter()
            .map(|(name, value)| (name.to_string(), plist::Value::String(value.to_string())))
            .collect();
        plist::Value::Dictionary(dictionary)
            .to_file_xml(path)
            .unwrap();
    }
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::write(
        app.join("Contents/MacOS/Arc"),
        b"fixture metadata identity; never executed",
    )
    .unwrap();
    app
}

fn isolated_mac_fixture() -> (tempfile::TempDir, PlatformEnvironment, RunningProcessPolicy) {
    let temp = tempfile::tempdir().unwrap();
    let paths = neati_platform::paths::SimulatedPaths::new()
        .with_flavor(neati_platform::PathFlavor::current())
        .with_home(temp.path())
        .with_program_files(temp.path().join("system-applications"));
    // The catalog is macOS, but these filesystem fixtures run on each host.
    // Its temporary paths must retain the host's drive/separator semantics.
    let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
        .with_platform(PlatformKind::Macos)
        .with_roots(Arc::new(paths));
    (temp, environment, RunningProcessPolicy::none())
}

#[test]
fn arc_public_contract_enables_only_user_data_and_refuses_unknown_bundle_versions() {
    let (_temp, environment, guard) = isolated_mac_fixture();
    let app = arc_bundle(&environment);
    let base = ChromiumCacheProvider::base(&environment).unwrap();
    for root in ["Arc/User Data", "Arc", "company.thebrowser.Browser"] {
        let path = base
            .join(root)
            .join("Default/Service Worker/CacheStorage/origin/unit");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), vec![1; 4096]).unwrap();
    }
    let trash = Arc::new(FixtureTrash::default());
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Idle),
        trash.clone(),
    );
    let scan = provider.scan(&environment, &guard);
    assert_eq!(scan.units.len(), 1);
    assert!(scan.units[0].path.starts_with(base.join("Arc/User Data")));
    assert_eq!(scan.units[0].state, OwnerUnitState::Ready);
    let plan = provider
        .prepare(&environment, &guard, &[selection(&scan.units[0])])
        .unwrap();
    let info = app.join("Contents/Info.plist");
    let mut value = plist::Value::from_file(&info).unwrap();
    value.as_dictionary_mut().unwrap().insert(
        "CFBundleShortVersionString".into(),
        plist::Value::String("1.167.0".into()),
    );
    value.to_file_xml(info).unwrap();
    assert_eq!(
        provider.execute(&environment, &plan).units[0].status,
        ProviderStatus::Blocked
    );
    assert!(trash.moved.lock().unwrap().is_empty());
    let scan = provider.scan(&environment, &guard);
    assert_eq!(scan.units.len(), 1);
    assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
    assert!(scan.units[0].allocated_bytes > 0);
    for root in ["Arc", "company.thebrowser.Browser"] {
        assert!(base
            .join(root)
            .join("Default/Service Worker/CacheStorage/origin/unit/payload")
            .exists());
    }
}

#[test]
fn renderer_matrix_keeps_profiles_channels_and_all_adjacent_state_separate() {
    let (_temp, environment, guard) = isolated_mac_fixture();
    arc_bundle(&environment);
    let home = environment.user_home().unwrap();
    let arc = home.join("Library/Application Support/Arc/User Data");
    let mcp = home.join(".cache/chrome-devtools-mcp-cli/chrome-profile-beta");
    let beta = home.join("Library/Application Support/Google/Chrome Beta");
    for root in [&arc, &mcp, &beta] {
        for profile in ["Default", "Profile 2"] {
            fs::create_dir_all(root.join(profile)).unwrap();
            for state in [
                "Cookies",
                "History",
                "Login Data",
                "Preferences",
                "IndexedDB/db",
                "Service Worker/ScriptCache/script",
                "Service Worker/Database/db",
                "Extensions/installed",
                "OptGuideOnDeviceModel/model",
            ] {
                let path = root.join(profile).join(state);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"protected").unwrap();
            }
        }
    }
    let paths = [
        home.join("Library/Caches/Arc/User Data/Default/Cache"),
        arc.join("Profile 2/GPUCache"),
        mcp.join("Default/Cache"),
        mcp.join("Profile 2/Code Cache"),
        home.join("Library/Caches/Google/Chrome Beta/Default/Cache"),
    ];
    for path in &paths {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("data_0"), vec![1; 4096]).unwrap();
    }
    let unknown = mcp.join("Custom/Cache");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("data_0"), vec![1; 4096]).unwrap();
    let trash = Arc::new(FixtureTrash::default());
    let provider = provider(
        BrowserCacheKind::RendererCaches,
        Arc::new(Idle),
        trash.clone(),
    );
    let scan = provider.scan(&environment, &guard);
    assert_eq!(scan.units.len(), 5);
    assert!(scan
        .units
        .iter()
        .all(|unit| unit.state == OwnerUnitState::Ready));
    let plan = provider
        .prepare(
            &environment,
            &guard,
            &scan.units.iter().map(selection).collect::<Vec<_>>(),
        )
        .unwrap();
    let outcome = provider.execute(&environment, &plan);
    assert!(outcome
        .units
        .iter()
        .all(|unit| unit.status == ProviderStatus::Cleaned));
    assert_eq!(trash.moved.lock().unwrap().len(), 5);
    assert!(unknown.join("data_0").exists());
    for root in [&arc, &mcp, &beta] {
        for profile in ["Default", "Profile 2"] {
            for state in [
                "Cookies",
                "History",
                "Login Data",
                "Preferences",
                "IndexedDB/db",
                "Service Worker/ScriptCache/script",
                "Service Worker/Database/db",
                "Extensions/installed",
                "OptGuideOnDeviceModel/model",
            ] {
                assert_eq!(
                    fs::read(root.join(profile).join(state)).unwrap(),
                    b"protected"
                );
            }
        }
    }
}

#[test]
fn final_owner_boundary_renderer_state_and_forged_advisory_selection_never_reach_trash() {
    let (_temp, environment, guard) = isolated_mac_fixture();
    let home = environment.user_home().unwrap();
    let cache = home.join(".cache/chrome-devtools-mcp/chrome-profile/Default/Cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("data_0"), vec![1; 4096]).unwrap();
    let trash = Arc::new(FixtureTrash::default());
    let provider = provider(
        BrowserCacheKind::RendererCaches,
        Arc::new(Idle),
        trash.clone(),
    );
    for name in [
        "Cookies",
        "History",
        "credentials.json",
        "index.db",
        "index.db-wal",
        "index.db-shm",
        "settings.json",
        "Model.app/payload",
        "OptGuideOnDeviceModel/model",
        "worker.js",
        "Worker.JS",
        "runtime.wasm",
    ] {
        let path = cache.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![2; 4096]).unwrap();
        let scan = provider.scan(&environment, &guard);
        assert_eq!(scan.units[0].state, OwnerUnitState::Blocked, "{name}");
        assert!(scan.units[0].allocated_bytes > 0);
        assert!(provider
            .prepare(&environment, &guard, &[selection(&scan.units[0])])
            .is_err());
        if path.parent() != Some(cache.as_path()) {
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
    let scan = provider.scan(&environment, &guard);
    let mut forged = selection(&scan.units[0]);
    forged.item_id = "manual-observation".into();
    assert!(provider.prepare(&environment, &guard, &[forged]).is_err());
    assert!(trash.moved.lock().unwrap().is_empty());
    assert!(cache.join("data_0").exists());
}

#[cfg(unix)]
#[test]
fn browser_final_callback_ancestor_and_equal_size_entry_replacements_are_refused() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Replace {
        root: PathBuf,
        ancestor: bool,
        armed: AtomicBool,
        calls: AtomicUsize,
    }
    impl RunningProcessProbe for Replace {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            if self.armed.load(Ordering::SeqCst)
                && guard.open_file_path().is_some()
                && self.calls.fetch_add(1, Ordering::SeqCst) == 1
            {
                if self.ancestor {
                    fs::rename(&self.root, self.root.with_extension("reviewed")).unwrap();
                    std::os::unix::fs::symlink(self.root.with_extension("outside"), &self.root)
                        .unwrap();
                } else {
                    let unit = guard.open_file_path().unwrap();
                    let file = unit.join("payload");
                    let bytes = fs::read(&file).unwrap();
                    let modified = fs::metadata(&file).unwrap().modified().unwrap();
                    fs::rename(&file, unit.join("reviewed-payload")).unwrap();
                    fs::write(&file, bytes).unwrap();
                    fs::File::options()
                        .write(true)
                        .open(file)
                        .unwrap()
                        .set_modified(modified)
                        .unwrap();
                }
            }
            Some(vec![])
        }
    }
    for ancestor in [false, true] {
        let (_temp, environment, guard) = isolated_mac_fixture();
        let root = ChromiumCacheProvider::base(&environment)
            .unwrap()
            .join("Google/Chrome");
        let unit = root.join("Default/Service Worker/CacheStorage/origin/unit");
        fs::create_dir_all(&unit).unwrap();
        fs::write(unit.join("payload"), vec![1; 4096]).unwrap();
        let outside = root
            .with_extension("outside")
            .join("Default/Service Worker/CacheStorage/origin/unit");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("private"), b"keep").unwrap();
        let probe = Arc::new(Replace {
            root: root.clone(),
            ancestor,
            armed: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
        });
        let trash = Arc::new(FixtureTrash::default());
        let provider = provider(
            BrowserCacheKind::OfflineCacheStorage,
            probe.clone(),
            trash.clone(),
        );
        let scan = provider.scan(&environment, &guard);
        let plan = provider
            .prepare(&environment, &guard, &[selection(&scan.units[0])])
            .unwrap();
        probe.armed.store(true, Ordering::SeqCst);
        let result = provider.execute(&environment, &plan);
        assert_eq!(result.units[0].status, ProviderStatus::Blocked);
        assert_eq!(result.reclaimed_bytes(), 0);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        assert!(trash.moved.lock().unwrap().is_empty());
        assert_eq!(fs::read(outside.join("private")).unwrap(), b"keep");
    }
}

#[cfg(unix)]
#[test]
fn browser_hardlinks_and_stale_same_byte_selections_are_refused() {
    let (temp, environment, guard) = isolated_mac_fixture();
    let unit = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/unit");
    fs::create_dir_all(&unit).unwrap();
    let file = unit.join("payload");
    fs::write(&file, vec![1; 4096]).unwrap();
    let trash = Arc::new(FixtureTrash::default());
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Idle),
        trash.clone(),
    );
    let scan = provider.scan(&environment, &guard);
    let selected = selection(&scan.units[0]);
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    fs::rename(&file, temp.path().join("old-payload")).unwrap();
    fs::write(&file, vec![1; 4096]).unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert!(provider.prepare(&environment, &guard, &[selected]).is_err());
    fs::hard_link(&file, temp.path().join("shared")).unwrap();
    let scan = provider.scan(&environment, &guard);
    assert_eq!(scan.units[0].state, OwnerUnitState::Blocked);
    assert!(scan.units[0].allocated_bytes > 0);
    assert!(trash.moved.lock().unwrap().is_empty());
}

#[test]
fn cancelled_browser_inventory_starts_no_use_probes_and_qualifies_unknown_scope() {
    struct Cancelled;
    impl crate::models::CancellationProbe for Cancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }
    struct NoProbe;
    impl RunningProcessProbe for NoProbe {
        fn running(&self, _: &RunningProcessPolicy) -> Option<Vec<String>> {
            panic!("cancelled inventory must not launch a process/handle probe")
        }
    }
    let (_temp, environment, _guard) = isolated_mac_fixture();
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(NoProbe),
        Arc::new(FixtureTrash::default()),
    );
    let result = provider.read_store_with_cancellation(&environment, &Cancelled);
    assert!(result.units.is_empty());
    assert_eq!(
        result.inspection_issue,
        Some(crate::models::ScanGapKind::Cancelled)
    );
    assert!(result.detail.unwrap().contains("unknown"));
}

struct Idle;
impl RunningProcessProbe for Idle {
    fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
        Some(Vec::new())
    }
}

struct Busy;
impl RunningProcessProbe for Busy {
    fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
        Some(vec!["Google Chrome".into()])
    }
}

struct Unknown;
impl RunningProcessProbe for Unknown {
    fn running(&self, _guard: &RunningProcessPolicy) -> Option<Vec<String>> {
        None
    }
}

struct StartsAfterReview {
    chrome_checks: std::sync::atomic::AtomicUsize,
}
impl RunningProcessProbe for StartsAfterReview {
    fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
        let is_chrome = guard
            .executables()
            .iter()
            .any(|name| name == "Google Chrome");
        if is_chrome
            && self
                .chrome_checks
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                >= 2
        {
            Some(vec!["Google Chrome".into()])
        } else {
            Some(Vec::new())
        }
    }
}

#[derive(Default)]
struct FixtureTrash {
    moved: Mutex<Vec<PathBuf>>,
}
impl RemovalBackend for FixtureTrash {
    fn remove(
        &self,
        path: &Path,
        _: neati_core::domain::identity::FileIdentity,
    ) -> Result<(), String> {
        self.moved.lock().unwrap().push(path.to_path_buf());
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    }
}

fn provider(
    kind: BrowserCacheKind,
    process: Arc<dyn RunningProcessProbe>,
    removal: Arc<dyn RemovalBackend>,
) -> ChromiumCacheProvider {
    ChromiumCacheProvider::new(kind, process, Arc::new(SizeCalculatorMeasurement), removal)
}

fn mac_fixture() -> (tempfile::TempDir, PlatformEnvironment, RunningProcessPolicy) {
    let temp = tempfile::tempdir().unwrap();
    let environment = PlatformEnvironment::native()
        .with_platform(PlatformKind::Macos)
        .with_home(temp.path());
    let guard = RunningProcessPolicy::guarding(vec!["Google Chrome".into()]);
    (temp, environment, guard)
}

#[test]
fn offline_probe_fanout_is_bounded_and_keeps_each_unit_verdict() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Default)]
    struct Probe {
        active: AtomicUsize,
        peak: AtomicUsize,
        calls: AtomicUsize,
    }
    impl RunningProcessProbe for Probe {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            let path = guard.open_file_path().expect("exact unit probe");
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.active.fetch_sub(1, Ordering::SeqCst);
            match path.file_name().unwrap().to_str().unwrap() {
                "unit-0" => None,
                "unit-1" => Some(vec!["fixture reader".into()]),
                _ => Some(vec![]),
            }
        }
    }
    let (_temp, environment, guard) = mac_fixture();
    let root = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default/Service Worker/CacheStorage/origin");
    for index in 0..12 {
        let path = root.join(format!("unit-{index}"));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("payload"), vec![1; 4096]).unwrap();
    }
    let probe = Arc::new(Probe::default());
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        probe.clone(),
        Arc::new(FixtureTrash::default()),
    );
    let start = std::time::Instant::now();
    let observed = provider.scan(&environment, &guard);
    assert_eq!(observed.units.len(), 12);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 12);
    assert!((1..=4).contains(&probe.peak.load(Ordering::SeqCst)));
    assert_eq!(
        observed
            .units
            .iter()
            .filter(|unit| unit.state == OwnerUnitState::Ready)
            .count(),
        10
    );
    assert_eq!(
        observed
            .units
            .iter()
            .find(|unit| unit.path.ends_with("unit-1"))
            .unwrap()
            .state,
        OwnerUnitState::InUse
    );
    assert_ne!(
        observed
            .units
            .iter()
            .find(|unit| unit.path.ends_with("unit-0"))
            .unwrap()
            .state,
        OwnerUnitState::Ready
    );
    let parallel_ms = start.elapsed().as_millis();
    let serial_started = std::time::Instant::now();
    let layout = MAC_LAYOUTS
        .iter()
        .find(|layout| layout.key == "chrome")
        .unwrap();
    let browser_root = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join(layout.relative);
    let serial: Vec<_> = observed
        .units
        .iter()
        .map(|unit| {
            provider
                .observe_unit(
                    &environment,
                    layout,
                    Some("fixture"),
                    &browser_root,
                    unit.path.clone(),
                    &BrowserOwnerState::Idle,
                )
                .unwrap()
        })
        .collect();
    for (parallel, serial) in observed.units.iter().zip(&serial) {
        assert_eq!(parallel.path, serial.path);
        assert_eq!(parallel.state, serial.state);
        assert_eq!(parallel.allocated_bytes, serial.allocated_bytes);
    }
    println!(
        "12 exact-unit probes: parallel {} ms, serial {} ms, peak {}",
        parallel_ms,
        serial_started.elapsed().as_millis(),
        probe.peak.load(Ordering::SeqCst)
    );
}

#[test]
fn windows_layouts_resolve_from_the_stated_local_app_data() {
    let local = PathBuf::from(r"D:\Users\tester\AppData\Local");
    let roots = neati_platform::paths::SimulatedPaths::new()
        .with_flavor(neati_platform::PathFlavor::Windows)
        .with_local_app_data(local.clone());
    let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::Windows)
        .with_platform(PlatformKind::Windows)
        .with_roots(Arc::new(roots));
    assert_eq!(
        ChromiumCacheProvider::base(&environment),
        Some(local.clone())
    );
    assert_eq!(
        local.join(WINDOWS_LAYOUTS[0].relative),
        local.join("Google/Chrome/User Data")
    );
    assert!(WINDOWS_LAYOUTS
        .iter()
        .all(|layout| !layout.relative.contains('*') && !layout.relative.contains("..")));
}

fn selection(unit: &OwnerUnitObservation) -> OwnerProviderSelection {
    OwnerProviderSelection {
        item_id: format!("browser-item.{}", unit.unit_key),
        name: "Browser cache".into(),
        path: unit.path.clone(),
        expected_bytes: unit.allocated_bytes,
    }
}

#[test]
fn component_provider_moves_only_the_complete_component_store() {
    let (_temp, environment, guard) = mac_fixture();
    let user_data = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome");
    let component = user_data.join("component_crx_cache");
    fs::create_dir_all(&component).unwrap();
    fs::write(component.join("metadata.json"), b"not parsed independently").unwrap();
    fs::write(component.join("payload.crx3"), vec![1; 8192]).unwrap();
    let extension = user_data.join("Default/Extensions/keep");
    fs::create_dir_all(&extension).unwrap();
    fs::write(extension.join("manifest.json"), b"keep").unwrap();
    let trash = Arc::new(FixtureTrash::default());
    let provider = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(Idle),
        trash.clone(),
    );

    let observation = provider.scan(&environment, &guard);
    assert_eq!(observation.units.len(), 1);
    assert_eq!(observation.units[0].state, OwnerUnitState::Ready);
    let plan = provider
        .prepare(&environment, &guard, &[selection(&observation.units[0])])
        .unwrap();
    let result = provider.execute(&environment, &plan);

    assert_eq!(result.units[0].status, ProviderStatus::Cleaned);
    assert!(!component.exists());
    assert!(extension.exists());
    assert_eq!(trash.moved.lock().unwrap().as_slice(), &[component]);
}

#[test]
fn offline_provider_removes_cache_storage_but_preserves_profile_state() {
    let (_temp, environment, guard) = mac_fixture();
    let profile = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default");
    let cache = profile.join("Service Worker/CacheStorage/origin/cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("index.txt"), vec![2; 8192]).unwrap();
    for sibling in [
        profile.join("Cookies"),
        profile.join("History"),
        profile.join("Local Storage/state"),
        profile.join("IndexedDB/state"),
        profile.join("Service Worker/Database/registration"),
    ] {
        fs::create_dir_all(sibling.parent().unwrap()).unwrap();
        fs::write(sibling, b"keep").unwrap();
    }
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Idle),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    assert_eq!(observation.units.len(), 1);
    let plan = provider
        .prepare(&environment, &guard, &[selection(&observation.units[0])])
        .unwrap();
    assert!(plan.requires_confirmation);
    provider.execute(&environment, &plan);

    assert!(!cache.exists());
    assert!(profile.join("Cookies").exists());
    assert!(profile.join("History").exists());
    assert!(profile.join("Local Storage/state").exists());
    assert!(profile.join("IndexedDB/state").exists());
    assert!(profile
        .join("Service Worker/Database/registration")
        .exists());
}

#[test]
fn offline_units_preserve_origin_indexes_and_unselected_cache_siblings() {
    let (_temp, environment, guard) = mac_fixture();
    let root = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default/Service Worker/CacheStorage");
    for relative in [
        "origin/first/payload",
        "origin/second/payload",
        "origin/index.txt",
        "index.txt",
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![1; 4096]).unwrap();
    }
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Idle),
        Arc::new(FixtureTrash::default()),
    );
    let observed = provider.scan(&environment, &guard);
    assert_eq!(observed.units.len(), 4);
    assert_eq!(
        observed
            .units
            .iter()
            .filter(|unit| unit.state == OwnerUnitState::Advisory)
            .count(),
        2
    );
    let selected = observed
        .units
        .iter()
        .find(|unit| unit.path.ends_with("origin/first"))
        .unwrap();
    let plan = provider
        .prepare(&environment, &guard, &[selection(selected)])
        .unwrap();
    assert_eq!(
        plan.deletion_disposition,
        neati_core::domain::cleanup::DeletionDisposition::PermanentDelete
    );
    assert_eq!(
        provider.execute(&environment, &plan).units[0].status,
        ProviderStatus::Cleaned
    );
    assert!(!root.join("origin/first").exists());
    for retained in ["origin/second/payload", "origin/index.txt", "index.txt"] {
        assert!(root.join(retained).exists());
    }
}

#[test]
fn running_browser_allows_idle_offline_units_but_not_component_stores() {
    struct BrowserOnly;
    impl RunningProcessProbe for BrowserOnly {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(if guard.open_file_path().is_some() {
                Vec::new()
            } else {
                vec!["Google Chrome".into()]
            })
        }
    }
    let (_temp, environment, guard) = mac_fixture();
    let root = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome");
    for relative in [
        "Default/Service Worker/CacheStorage/origin/cache",
        "component_crx_cache",
    ] {
        fs::create_dir_all(root.join(relative)).unwrap();
        fs::write(root.join(relative).join("payload"), vec![1; 4096]).unwrap();
    }
    let trash = Arc::new(FixtureTrash::default());
    let offline = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(BrowserOnly),
        trash.clone(),
    );
    let observed = offline.scan(&environment, &guard);
    assert_eq!(observed.units[0].state, OwnerUnitState::Ready);
    let plan = offline
        .prepare(&environment, &guard, &[selection(&observed.units[0])])
        .unwrap();
    assert_eq!(
        offline.execute(&environment, &plan).units[0].status,
        ProviderStatus::Cleaned
    );
    let components = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(BrowserOnly),
        trash,
    );
    assert_eq!(
        components.scan(&environment, &guard).units[0].state,
        OwnerUnitState::InUse
    );
}

#[test]
fn offline_handle_opened_at_final_check_prevents_trash() {
    struct OpensAtMutation(std::sync::atomic::AtomicUsize);
    impl RunningProcessProbe for OpensAtMutation {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(
                if guard.open_file_path().is_some()
                    && self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3
                {
                    vec!["cache reader".into()]
                } else {
                    Vec::new()
                },
            )
        }
    }
    let (_temp, environment, guard) = mac_fixture();
    let path = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/cache");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("payload"), vec![1; 4096]).unwrap();
    let trash = Arc::new(FixtureTrash::default());
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(OpensAtMutation(std::sync::atomic::AtomicUsize::new(0))),
        trash.clone(),
    );
    let observed = provider.scan(&environment, &guard);
    let plan = provider
        .prepare(&environment, &guard, &[selection(&observed.units[0])])
        .unwrap();
    assert_eq!(
        provider.execute(&environment, &plan).units[0].status,
        ProviderStatus::Blocked
    );
    assert!(path.exists());
    assert!(trash.moved.lock().unwrap().is_empty());
}

#[test]
fn unknown_offline_handle_state_never_authorizes_a_unit() {
    let (_temp, environment, guard) = mac_fixture();
    let path = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/cache");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("payload"), vec![1; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Unknown),
        Arc::new(FixtureTrash::default()),
    );
    let observed = provider.scan(&environment, &guard);
    assert_eq!(observed.units[0].state, OwnerUnitState::Blocked);
    assert!(provider
        .prepare(&environment, &guard, &[selection(&observed.units[0])])
        .is_err());
    assert!(path.exists());
}

#[test]
fn windows_offline_units_keep_the_browser_exit_requirement() {
    let temp = tempfile::tempdir().unwrap();
    let roots = neati_platform::paths::SimulatedPaths::new()
        .with_flavor(neati_platform::PathFlavor::current())
        .with_local_app_data(temp.path().to_path_buf());
    let environment = PlatformEnvironment::simulated(neati_platform::PathFlavor::current())
        .with_platform(PlatformKind::Windows)
        .with_roots(Arc::new(roots));
    let path = temp
        .path()
        .join("Google/Chrome/User Data/Default/Service Worker/CacheStorage/origin/cache");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("payload"), vec![1; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Busy),
        Arc::new(FixtureTrash::default()),
    );
    let observed = provider.scan(&environment, &RunningProcessPolicy::none());
    assert_eq!(observed.units[0].state, OwnerUnitState::InUse);
    assert!(path.exists());
}

#[test]
fn open_file_probe_can_refuse_an_otherwise_idle_browser_unit() {
    struct OpenFile;
    impl RunningProcessProbe for OpenFile {
        fn running(&self, guard: &RunningProcessPolicy) -> Option<Vec<String>> {
            Some(if guard.open_file_path().is_some() {
                vec!["cache reader".into()]
            } else {
                vec![]
            })
        }
    }
    let (_temp, environment, guard) = mac_fixture();
    let path = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/Default/Service Worker/CacheStorage/origin/cache");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("payload"), vec![1; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(OpenFile),
        Arc::new(FixtureTrash::default()),
    );
    let observed = provider.scan(&environment, &guard);
    assert_eq!(observed.units[0].state, OwnerUnitState::InUse);
    assert!(provider
        .prepare(&environment, &guard, &[selection(&observed.units[0])])
        .is_err());
    assert!(path.exists());
}

#[test]
fn multiple_profiles_are_enumerated_without_recursive_discovery() {
    let (_temp, environment, guard) = mac_fixture();
    let root = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome");
    for profile in ["Default", "Profile 2"] {
        let cache = root
            .join(profile)
            .join("Service Worker/CacheStorage/origin/cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("entry"), vec![3; 4096]).unwrap();
    }
    let nested = root.join("Archive/Profile 9/Service Worker/CacheStorage");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("entry"), vec![4; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::OfflineCacheStorage,
        Arc::new(Idle),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    assert_eq!(observation.units.len(), 2);
    assert!(observation
        .units
        .iter()
        .any(|unit| unit.unit_key.starts_with("chrome/Default/origin/cache@")));
    assert!(observation
        .units
        .iter()
        .any(|unit| unit.unit_key.starts_with("chrome/Profile 2/origin/cache@")));
}

#[test]
fn running_browser_keeps_its_measured_units_visible_but_refused() {
    let (_temp, environment, guard) = mac_fixture();
    let cache = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/component_crx_cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("payload"), vec![1; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(Busy),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    assert_eq!(observation.status, ProviderStatus::Ready);
    assert_eq!(observation.units[0].state, OwnerUnitState::InUse);
    assert!(observation.units[0].allocated_bytes > 0);
}

#[test]
fn unknown_process_state_keeps_measured_units_visible_but_blocked() {
    let (_temp, environment, guard) = mac_fixture();
    let cache = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/component_crx_cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("payload"), vec![1; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(Unknown),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    assert_eq!(observation.units[0].state, OwnerUnitState::Blocked);
    assert!(observation.units[0].allocated_bytes > 0);
}

#[test]
fn browser_starting_after_review_blocks_execution() {
    let (_temp, environment, guard) = mac_fixture();
    let cache = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/component_crx_cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("payload"), vec![1; 4096]).unwrap();
    let provider = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(StartsAfterReview {
            chrome_checks: std::sync::atomic::AtomicUsize::new(0),
        }),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    let plan = provider
        .prepare(&environment, &guard, &[selection(&observation.units[0])])
        .unwrap();
    let result = provider.execute(&environment, &plan);
    assert_eq!(result.units[0].status, ProviderStatus::Blocked);
    assert!(cache.exists());
}

#[cfg(unix)]
#[test]
fn link_inside_store_blocks_the_unit() {
    use std::os::unix::fs::symlink;
    let (_temp, environment, guard) = mac_fixture();
    let cache = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/component_crx_cache");
    fs::create_dir_all(&cache).unwrap();
    symlink("/tmp", cache.join("escape")).unwrap();
    let provider = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(Idle),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    assert_eq!(observation.units[0].state, OwnerUnitState::Blocked);
}

#[test]
fn replacement_after_review_is_refused() {
    let (_temp, environment, guard) = mac_fixture();
    let cache = ChromiumCacheProvider::base(&environment)
        .unwrap()
        .join("Google/Chrome/component_crx_cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("payload"), vec![5; 8192]).unwrap();
    let provider = provider(
        BrowserCacheKind::ComponentDownloads,
        Arc::new(Idle),
        Arc::new(FixtureTrash::default()),
    );
    let observation = provider.scan(&environment, &guard);
    let plan = provider
        .prepare(&environment, &guard, &[selection(&observation.units[0])])
        .unwrap();
    fs::remove_dir_all(&cache).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("payload"), vec![5; 8192]).unwrap();

    let result = provider.execute(&environment, &plan);
    assert_eq!(result.units[0].status, ProviderStatus::Blocked);
    assert!(cache.exists());
}
