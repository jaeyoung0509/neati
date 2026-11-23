use super::schedule::{decide, next_due_at, MIN_IDLE_SECONDS};
use super::*;
use crate::models::{AutomaticCleanInterval, AutomaticCleanSettings, NeatiSettings};
use crate::services::one_button_clean::ports::{
    CleanOutcome, CleanPreview, OneButtonError, ReviewSummary,
};
use crate::services::one_button_clean::selection::{Candidate, Source};
use crate::services::ports::{ActivityPort, PortFuture};
use std::sync::atomic::AtomicU32;

const DAY: u64 = 86_400;
const NOW: u64 = 2_000_000_000;

fn on(interval: AutomaticCleanInterval) -> AutomaticCleanSettings {
    AutomaticCleanSettings {
        enabled: true,
        interval,
    }
}

#[test]
fn the_schedule_waits_for_every_condition() {
    let never =
        || -> (Option<bool>, Option<u64>) { panic!("a disabled or early check observes nothing") };
    assert_eq!(
        decide(NOW, AutomaticCleanSettings::default(), None, never),
        Err(AutomaticCleanWait::Disabled)
    );
    let weekly = on(AutomaticCleanInterval::Weekly);
    assert_eq!(
        decide(NOW, weekly, Some(NOW - 2 * DAY), never),
        Err(AutomaticCleanWait::NotDue)
    );
    let idle = MIN_IDLE_SECONDS;
    let due = Some(NOW - 8 * DAY);
    assert_eq!(
        decide(NOW, weekly, due, || (Some(false), Some(idle))),
        Err(AutomaticCleanWait::OnBattery)
    );
    assert_eq!(
        decide(NOW, weekly, due, || (None, Some(idle))),
        Err(AutomaticCleanWait::ConditionsUnknown)
    );
    assert_eq!(
        decide(NOW, weekly, due, || (Some(true), Some(idle - 1))),
        Err(AutomaticCleanWait::PersonActive)
    );
    assert_eq!(
        decide(NOW, weekly, due, || (Some(true), None)),
        Err(AutomaticCleanWait::ConditionsUnknown)
    );
    assert_eq!(
        decide(NOW, weekly, due, || (Some(true), Some(idle))),
        Ok(())
    );
    // Never ran: due at once.
    assert_eq!(
        decide(NOW, weekly, None, || (Some(true), Some(idle))),
        Ok(())
    );
    assert_eq!(next_due_at(weekly, Some(NOW)), Some(NOW + 7 * DAY));
    // The clock moved back after a run: due now rather than weeks away.
    assert_eq!(
        decide(NOW, weekly, Some(NOW + 30 * DAY), || (
            Some(true),
            Some(idle)
        )),
        Ok(())
    );
    assert_eq!(
        next_due_at(AutomaticCleanSettings::default(), Some(NOW)),
        None
    );
}

struct FakeClean {
    previews: AtomicU32,
    executes: AtomicU32,
    chosen: Vec<Candidate>,
    refuse: Option<String>,
}

impl CleanUseCase for FakeClean {
    fn preview(
        &self,
        _request: CleanRequest,
        _activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<CleanPreview, OneButtonError>> {
        Box::pin(async move {
            self.previews.fetch_add(1, Ordering::SeqCst);
            if let Some(message) = &self.refuse {
                return Err(OneButtonError::Unavailable(message.clone()));
            }
            Ok(CleanPreview {
                total_bytes: self.chosen.iter().map(|candidate| candidate.bytes).sum(),
                chosen: self.chosen.clone(),
                review: ReviewSummary::default(),
                caches: None,
                projects: None,
                worktrees: None,
            })
        })
    }

    fn execute<'a>(
        &'a self,
        preview: &'a CleanPreview,
        _activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'a, Result<CleanOutcome, OneButtonError>> {
        Box::pin(async move {
            self.executes.fetch_add(1, Ordering::SeqCst);
            Ok(CleanOutcome {
                reclaimed_bytes: preview.total_bytes,
                removed: preview.chosen.len() as u64,
                skipped: 0,
                failed: Vec::new(),
            })
        })
    }
}

struct Ready;

impl Conditions for Ready {
    fn on_ac_power(&self) -> Option<bool> {
        Some(true)
    }

    fn idle_seconds(&self) -> Option<u64> {
        Some(MIN_IDLE_SECONDS * 2)
    }
}

/// Idle at the first look, back at the Mac by the second.
struct Returns(AtomicU32);

impl Conditions for Returns {
    fn on_ac_power(&self) -> Option<bool> {
        Some(true)
    }

    fn idle_seconds(&self) -> Option<u64> {
        let look = self.0.fetch_add(1, Ordering::SeqCst);
        Some(if look == 0 { MIN_IDLE_SECONDS * 2 } else { 5 })
    }
}

#[derive(Default)]
struct Recorded(Mutex<Vec<(String, String)>>);

impl Notifier for Recorded {
    fn notify(&self, title: &str, body: &str) {
        self.0
            .lock()
            .unwrap()
            .push((title.to_string(), body.to_string()));
    }
}

fn candidate(bytes: u64) -> Candidate {
    Candidate {
        reference: "cache-a".into(),
        source: Source::Cache,
        name: "Cache".into(),
        kind: "Developer tools".into(),
        path: "/Users/me/Library/Caches/a".into(),
        bytes,
        backend_id: "cache-a".into(),
    }
}

fn automatic(
    enabled: bool,
    chosen: Vec<Candidate>,
    refuse: Option<&str>,
    state_file: Option<PathBuf>,
) -> (AutomaticClean, Arc<FakeClean>, Arc<Recorded>) {
    let settings = Arc::new(SettingsAuthority::new(NeatiSettings {
        automatic_clean: AutomaticCleanSettings {
            enabled,
            interval: AutomaticCleanInterval::Weekly,
        },
        ..NeatiSettings::default()
    }));
    let clean = Arc::new(FakeClean {
        previews: AtomicU32::new(0),
        executes: AtomicU32::new(0),
        chosen,
        refuse: refuse.map(str::to_string),
    });
    let notes = Arc::new(Recorded::default());
    (
        AutomaticClean::new(
            clean.clone(),
            settings,
            Arc::new(Ready),
            notes.clone(),
            state_file,
        ),
        clean,
        notes,
    )
}

fn tick(automatic: &AutomaticClean, now: u64) -> Option<AutomaticCleanRun> {
    tauri::async_runtime::block_on(automatic.tick(now))
}

#[test]
fn nothing_runs_while_it_is_off() {
    let (automatic, clean, notes) = automatic(false, vec![candidate(10)], None, None);
    assert!(tick(&automatic, NOW).is_none());
    assert_eq!(clean.previews.load(Ordering::SeqCst), 0);
    assert_eq!(
        automatic.status().waiting,
        Some(AutomaticCleanWait::Disabled)
    );
    assert!(notes.0.lock().unwrap().is_empty());
}

#[test]
fn a_due_run_executes_exactly_its_preview_once_per_interval() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join(STATE_FILE_NAME);
    let (automatic, clean, notes) = automatic(
        true,
        vec![candidate(5 * 1024 * 1024)],
        None,
        Some(state.clone()),
    );
    let run = tick(&automatic, NOW).expect("a run");
    assert_eq!(run.removed, 1);
    assert_eq!(run.reclaimed_bytes, 5 * 1024 * 1024);
    assert_eq!(clean.previews.load(Ordering::SeqCst), 1);
    assert_eq!(clean.executes.load(Ordering::SeqCst), 1);
    assert_eq!(notes.0.lock().unwrap().len(), 1);
    assert!(notes.0.lock().unwrap()[0].1.contains("5.0 MB"));

    // The next check in the same week does nothing.
    assert!(tick(&automatic, NOW + DAY).is_none());
    assert_eq!(clean.previews.load(Ordering::SeqCst), 1);
    assert_eq!(automatic.status().waiting, Some(AutomaticCleanWait::NotDue));
    assert_eq!(automatic.status().next_due_at, Some(NOW + 7 * DAY));

    // The record survives a restart.
    let (restarted, _, _) = automatic_with_state(Some(state));
    assert_eq!(restarted.status().last_run, Some(run));
}

fn automatic_with_state(state: Option<PathBuf>) -> (AutomaticClean, Arc<FakeClean>, Arc<Recorded>) {
    automatic(true, Vec::new(), None, state)
}

#[test]
fn an_empty_set_or_a_refusal_still_counts_as_the_intervals_run() {
    let (empty, clean, notes) = automatic(true, Vec::new(), None, None);
    let run = tick(&empty, NOW).expect("a run");
    assert_eq!(run.removed, 0);
    assert_eq!(clean.executes.load(Ordering::SeqCst), 0);
    assert!(notes.0.lock().unwrap().is_empty(), "nothing to say");

    let (refused, clean, notes) =
        automatic(true, vec![candidate(1)], Some("The scan is busy."), None);
    let run = tick(&refused, NOW).expect("a run");
    assert_eq!(run.error.as_deref(), Some("The scan is busy."));
    assert_eq!(clean.executes.load(Ordering::SeqCst), 0);
    assert_eq!(notes.0.lock().unwrap()[0].0, "Automatic Clean did not run");
    assert!(tick(&refused, NOW + 60).is_none(), "no retry loop");
}

#[test]
fn a_person_who_returns_during_the_scan_stops_the_removal() {
    let settings = Arc::new(SettingsAuthority::new(NeatiSettings {
        automatic_clean: on(AutomaticCleanInterval::Weekly),
        ..NeatiSettings::default()
    }));
    let clean = Arc::new(FakeClean {
        previews: AtomicU32::new(0),
        executes: AtomicU32::new(0),
        chosen: vec![candidate(10)],
        refuse: None,
    });
    let automatic = AutomaticClean::new(
        clean.clone(),
        settings,
        Arc::new(Returns(AtomicU32::new(0))),
        Arc::new(NoNotifier),
        None,
    );
    assert!(tick(&automatic, NOW).is_none());
    assert_eq!(clean.previews.load(Ordering::SeqCst), 1);
    assert_eq!(clean.executes.load(Ordering::SeqCst), 0);
    assert_eq!(
        automatic.status().waiting,
        Some(AutomaticCleanWait::PersonActive)
    );
    assert!(
        automatic.status().last_run.is_none(),
        "not counted as a run"
    );
}
