//! Owner providers run side by side but report what one-after-another reports
//! (#493): the same items in the same order, no provider started after a stop,
//! and progress from a slow provider while it is still running.

use super::{OwnerScan, ProgressFocus, OWNER_PROVIDER_LANES};
use crate::cleaner::owner_providers::{
    OwnerProviderRegistry, OwnerScanContext, OwnerScopedProvider,
};
use crate::models::{
    CancellationProbe, Category, CleanStrategy, NeverCancelled, OwnerProviderRefusal,
    OwnerProviderSelection, OwnerStoreObservation, OwnerUnitObservation, PlatformKind, RiskTier,
    ScanItem, Signature,
};
use crate::scanner::RootProgressSink;
use crate::signatures::SignatureRegistry;
use neati_core::domain::cleanup::{
    OwnerProviderAuthorization, OwnerProviderExecution, RunningProcessPolicy,
};
use neati_core::domain::scan::ScanSpan;
use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PLATFORMS: [PlatformKind; 3] = [
    PlatformKind::Macos,
    PlatformKind::Windows,
    PlatformKind::Linux,
];

/// A bound on every wait in these tests: a defect fails the test instead of
/// hanging it.
const PATIENCE: Duration = Duration::from_secs(20);

fn wait_until(condition: impl Fn() -> bool, what: &str) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < PATIENCE, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[derive(Clone)]
struct Stop(Arc<AtomicBool>);

impl CancellationProbe for Stop {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// How the fixture provider behaves while it inventories its store.
#[derive(Clone)]
enum Behaviour {
    /// Sleeps, then reports its units.
    Sleep(Duration),
    /// Sets the stop and reports its units.
    Stop(Arc<AtomicBool>),
    /// Waits until another provider has started, then sets the stop.
    StopOnceRunning(Arc<AtomicBool>, Arc<AtomicUsize>),
    /// Waits until the stop is set, then reports its units.
    AwaitStop(Arc<AtomicBool>),
    /// Names a location and waits until the scan's thread has passed it on.
    ReportLocation(PathBuf, Arc<AtomicBool>),
}

struct Fixture {
    id: &'static str,
    behaviour: Behaviour,
    calls: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Fixture {
    fn new(id: &'static str, behaviour: Behaviour) -> Self {
        Self {
            id,
            behaviour,
            calls: Arc::default(),
            active: Arc::default(),
            peak: Arc::default(),
        }
    }

    fn sharing(mut self, active: &Arc<AtomicUsize>, peak: &Arc<AtomicUsize>) -> Self {
        self.active = Arc::clone(active);
        self.peak = Arc::clone(peak);
        self
    }

    fn store(&self) -> OwnerStoreObservation {
        let root = PathBuf::from("/store").join(self.id);
        OwnerStoreObservation::ready(
            Some(root.clone()),
            (0..3)
                .map(|unit| {
                    let bytes = 4_096 * (unit + 1);
                    OwnerUnitObservation::ready(
                        format!("unit-{unit}"),
                        root.join(format!("unit-{unit}")),
                        bytes,
                        bytes,
                        1,
                    )
                })
                .collect(),
        )
    }
}

impl OwnerScopedProvider for Fixture {
    fn id(&self) -> &'static str {
        self.id
    }
    fn platforms(&self) -> &'static [PlatformKind] {
        &PLATFORMS
    }
    fn consequence(&self) -> &'static str {
        "fixture only"
    }
    fn requires_confirmation(&self) -> bool {
        true
    }
    fn scan(&self, _: &PlatformEnvironment, _: &RunningProcessPolicy) -> OwnerStoreObservation {
        self.store()
    }
    fn scan_in(
        &self,
        _: &PlatformEnvironment,
        _: &RunningProcessPolicy,
        context: &OwnerScanContext<'_>,
        _: &mut Vec<ScanSpan>,
    ) -> OwnerStoreObservation {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        match &self.behaviour {
            Behaviour::Sleep(duration) => std::thread::sleep(*duration),
            Behaviour::Stop(stop) => stop.store(true, Ordering::SeqCst),
            Behaviour::StopOnceRunning(stop, other) => {
                wait_until(|| other.load(Ordering::SeqCst) > 0, "the other provider");
                stop.store(true, Ordering::SeqCst);
            }
            Behaviour::AwaitStop(stop) => {
                wait_until(|| stop.load(Ordering::SeqCst), "the stop");
            }
            Behaviour::ReportLocation(location, heard) => {
                context.location_started(location);
                wait_until(
                    || heard.load(Ordering::SeqCst),
                    "the location to reach the scan's thread",
                );
            }
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.store()
    }
    fn prepare(
        &self,
        _: &PlatformEnvironment,
        _: &RunningProcessPolicy,
        _: &[OwnerProviderSelection],
    ) -> Result<OwnerProviderAuthorization, OwnerProviderRefusal> {
        panic!("a scan never prepares")
    }
    fn execute(
        &self,
        _: &PlatformEnvironment,
        _: &OwnerProviderAuthorization,
    ) -> OwnerProviderExecution {
        panic!("a scan never executes")
    }
}

/// One catalog entry per provider, in the order the providers are given.
fn catalog(ids: &[&'static str]) -> SignatureRegistry {
    let mut registry = SignatureRegistry::new();
    for (index, id) in ids.iter().enumerate() {
        registry.register(Signature {
            deletion_disposition: None,
            id: format!("test.{id}"),
            name: format!("Store {id}"),
            category: Category::Developer,
            family: Default::default(),
            risk: RiskTier::Rebuild,
            strategy: CleanStrategy::OwnerProvider,
            paths: Vec::new(),
            exclusions: Vec::new(),
            description: String::new(),
            min_age_days: None,
            include_prefixes: Vec::new(),
            exclude_prefixes: Vec::new(),
            intensive_only: false,
            platforms: vec![PlatformKind::current()],
            discovery: Default::default(),
            unit: None,
            owner: String::new(),
            priority: (ids.len() - index) as i32,
            fail_if_running: Vec::new(),
            provider: String::new(),
            provider_id: Some((*id).to_string()),
            artifact_kind: Default::default(),
            consequence: String::new(),
        });
    }
    registry
}

fn environment() -> PlatformEnvironment {
    PlatformEnvironment::simulated(PathFlavor::current())
}

fn pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
}

/// Progress events the scan's thread received, in order.
#[derive(Default)]
struct Heard {
    roots: Mutex<Vec<(String, PathBuf)>>,
    location: Mutex<Option<(PathBuf, Arc<AtomicBool>)>>,
}

impl RootProgressSink for Heard {
    fn root_started(&self, signature: &Signature, root: &Path) {
        self.roots
            .lock()
            .unwrap()
            .push((signature.id.clone(), root.to_path_buf()));
        if let Some((expected, heard)) = self.location.lock().unwrap().as_ref() {
            if root == expected {
                heard.store(true, Ordering::SeqCst);
            }
        }
    }
}

fn scan(
    providers: &OwnerProviderRegistry,
    registry: &SignatureRegistry,
    scan: &OwnerScan<'_>,
    progress: &dyn RootProgressSink,
) -> (Vec<ScanItem>, Vec<String>) {
    let mut spans = Vec::new();
    let items = providers.scan_items_in(
        registry,
        Category::Developer,
        false,
        &[],
        &environment(),
        scan,
        &mut spans,
        progress,
    );
    (
        items,
        spans.into_iter().map(|span| span.source_id).collect(),
    )
}

/// Providers that finish in the reverse of catalog order still report their
/// units in catalog order, exactly as running them one after another does,
/// and no more than the lane bound ever run at once.
#[test]
fn side_by_side_providers_report_the_serial_result() {
    let ids = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let providers = OwnerProviderRegistry::new(
        ids.iter()
            .enumerate()
            .map(|(index, id)| {
                let delay = Duration::from_millis(5 * (ids.len() - index) as u64);
                Arc::new(Fixture::new(id, Behaviour::Sleep(delay)).sharing(&active, &peak))
                    as Arc<dyn OwnerScopedProvider>
            })
            .collect(),
    );
    let registry = catalog(&ids);

    let serial = scan(
        &providers,
        &registry,
        &OwnerScan::unscanned(&NeverCancelled),
        &Heard::default(),
    );
    assert_eq!(
        peak.swap(0, Ordering::SeqCst),
        1,
        "unscanned runs one at a time"
    );

    let workers = pool(OWNER_PROVIDER_LANES);
    let heard = Heard::default();
    let side_by_side = scan(
        &providers,
        &registry,
        &OwnerScan {
            snapshot: None,
            cancellation: &NeverCancelled,
            pool: Some(&workers),
            lanes: OWNER_PROVIDER_LANES,
        },
        &heard,
    );
    let concurrent = peak.load(Ordering::SeqCst);
    assert!(
        (2..=OWNER_PROVIDER_LANES).contains(&concurrent),
        "providers overlapped within the lane bound: {concurrent}"
    );
    assert_eq!(serial.0.len(), ids.len() * 3);
    assert_eq!(side_by_side.0, serial.0, "the same items in the same order");
    assert_eq!(side_by_side.1, serial.1, "the same spans in the same order");
    let started: Vec<String> = heard
        .roots
        .lock()
        .unwrap()
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        assert!(
            started.contains(&format!("test.{id}")),
            "every provider was announced when it started"
        );
    }
}

/// Once the scan is stopped no further provider starts. A provider that was
/// already running finishes, and neither it nor any provider after it in
/// catalog order reports units.
#[test]
fn a_stopped_scan_starts_no_further_provider() {
    let stop = Arc::new(AtomicBool::new(false));
    let waiting = Fixture::new("waiting", Behaviour::AwaitStop(Arc::clone(&stop)));
    // The stop comes while the first provider is running, so which providers
    // had started is settled before it.
    let stopping = Fixture::new(
        "stopping",
        Behaviour::StopOnceRunning(Arc::clone(&stop), Arc::clone(&waiting.calls)),
    );
    let later = Fixture::new("later", Behaviour::Sleep(Duration::ZERO));
    let last = Fixture::new("last", Behaviour::Sleep(Duration::ZERO));
    let calls = [
        Arc::clone(&waiting.calls),
        Arc::clone(&stopping.calls),
        Arc::clone(&later.calls),
        Arc::clone(&last.calls),
    ];
    let providers = OwnerProviderRegistry::new(vec![
        Arc::new(waiting),
        Arc::new(stopping),
        Arc::new(later),
        Arc::new(last),
    ]);
    let registry = catalog(&["waiting", "stopping", "later", "last"]);
    let workers = pool(2);
    let probe = Stop(Arc::clone(&stop));
    let (items, spans) = scan(
        &providers,
        &registry,
        &OwnerScan {
            snapshot: None,
            cancellation: &probe,
            pool: Some(&workers),
            lanes: 2,
        },
        &Heard::default(),
    );
    let calls: Vec<usize> = calls
        .iter()
        .map(|calls| calls.load(Ordering::SeqCst))
        .collect();
    assert_eq!(calls, [1, 1, 0, 0], "nothing starts after the stop");
    assert!(
        items.is_empty(),
        "a provider that ended after the stop reports nothing"
    );
    assert_eq!(
        spans,
        ["test.waiting"],
        "the first stopped provider's timing is kept and the pass ends there"
    );
}

/// One lane is the serial pass: the provider that stops the scan is the last
/// one that runs.
#[test]
fn one_lane_stops_at_the_provider_that_stopped_the_scan() {
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Fixture::new("stopping", Behaviour::Stop(Arc::clone(&stop)));
    let later = Fixture::new("later", Behaviour::Sleep(Duration::ZERO));
    let later_calls = Arc::clone(&later.calls);
    let providers = OwnerProviderRegistry::new(vec![Arc::new(stopping), Arc::new(later)]);
    let registry = catalog(&["stopping", "later"]);
    let workers = pool(2);
    let probe = Stop(Arc::clone(&stop));
    let (items, _) = scan(
        &providers,
        &registry,
        &OwnerScan {
            snapshot: None,
            cancellation: &probe,
            pool: Some(&workers),
            lanes: 1,
        },
        &Heard::default(),
    );
    assert_eq!(later_calls.load(Ordering::SeqCst), 0);
    assert!(items.is_empty());
}

/// A slow provider's progress reaches the scan's thread while the provider is
/// still running: the provider waits for its own location to be heard, so a
/// report held back until it finished would fail the wait.
#[test]
fn a_slow_provider_is_named_while_it_runs() {
    let heard_location = Arc::new(AtomicBool::new(false));
    let location = PathBuf::from("/opt/fixture-prefix");
    let slow = Fixture::new(
        "slow",
        Behaviour::ReportLocation(location.clone(), Arc::clone(&heard_location)),
    );
    let quick = Fixture::new("quick", Behaviour::Sleep(Duration::ZERO));
    let providers = OwnerProviderRegistry::new(vec![Arc::new(slow), Arc::new(quick)]);
    let registry = catalog(&["slow", "quick"]);
    let heard = Heard::default();
    *heard.location.lock().unwrap() = Some((location.clone(), heard_location));
    for workers in [None, Some(pool(2))] {
        heard.roots.lock().unwrap().clear();
        if let Some((_, flag)) = heard.location.lock().unwrap().as_ref() {
            flag.store(false, Ordering::SeqCst);
        }
        let (items, _) = scan(
            &providers,
            &registry,
            &OwnerScan {
                snapshot: None,
                cancellation: &NeverCancelled,
                pool: workers.as_ref(),
                lanes: 2,
            },
            &heard,
        );
        assert_eq!(items.len(), 6);
        let roots = heard.roots.lock().unwrap().clone();
        assert!(roots.contains(&("test.slow".into(), PathBuf::from("provider://slow"))));
        assert!(roots.contains(&("test.slow".into(), location.clone())));
        assert!(roots.contains(&("test.quick".into(), PathBuf::from("provider://quick"))));
    }
}

/// When the provider the interface was told about ends while another still
/// runs, the one running longest is named again with its latest location.
#[test]
fn the_longest_running_provider_is_named_when_the_announced_one_ends() {
    let mut focus = ProgressFocus::default();
    assert_eq!(
        focus.started(0, "provider://slow".into()),
        (0, "provider://slow".into())
    );
    assert_eq!(
        focus.moved(0, "/opt/homebrew".into()),
        (0, "/opt/homebrew".into())
    );
    assert_eq!(
        focus.started(1, "provider://quick".into()),
        (1, "provider://quick".into())
    );
    assert_eq!(
        focus.started(2, "provider://other".into()),
        (2, "provider://other".into())
    );
    assert_eq!(
        focus.finished(1),
        None,
        "an unannounced provider ending changes nothing"
    );
    assert_eq!(
        focus.finished(2),
        Some((0, PathBuf::from("/opt/homebrew"))),
        "the longest-running provider is named again with its latest location"
    );
    assert_eq!(focus.finished(0), None, "nothing is left running");
}

/// Paired timing of a provider pass run one after another and side by side on
/// the shared scan pool: one slow owner command (a stand-in for the ~7.7 s
/// `brew cleanup --dry-run`, scaled down tenfold) and seven quick stores. Run
/// with `cargo test -p neati-desktop --lib provider_pass_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing evidence, not a CI threshold"]
fn provider_pass_timing() {
    let ids = ["slow", "q1", "q2", "q3", "q4", "q5", "q6", "q7"];
    let providers = OwnerProviderRegistry::new(
        ids.iter()
            .map(|id| {
                let delay = if *id == "slow" { 770 } else { 100 };
                Arc::new(Fixture::new(
                    id,
                    Behaviour::Sleep(Duration::from_millis(delay)),
                )) as Arc<dyn OwnerScopedProvider>
            })
            .collect(),
    );
    let registry = catalog(&ids);
    let started = Instant::now();
    let serial = scan(
        &providers,
        &registry,
        &OwnerScan::unscanned(&NeverCancelled),
        &Heard::default(),
    );
    let serial_ms = started.elapsed().as_millis();
    let pool = crate::execution_budget::shared_scan_pool();
    let started = Instant::now();
    let lanes = scan(
        &providers,
        &registry,
        &OwnerScan {
            snapshot: None,
            cancellation: &NeverCancelled,
            pool,
            lanes: OWNER_PROVIDER_LANES,
        },
        &Heard::default(),
    );
    let lanes_ms = started.elapsed().as_millis();
    assert_eq!(lanes, serial);
    println!(
        "{{\"providers\":8,\"workers\":{},\"serial_ms\":{serial_ms},\"lanes_ms\":{lanes_ms}}}",
        pool.map_or(1, rayon::ThreadPool::current_num_threads)
    );
}
