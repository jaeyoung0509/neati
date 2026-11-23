//! Automatic Clean (#507, owner direction 2026-11-24).
//!
//! A driving adapter, like the CLI and the Quick Panel: it runs the same
//! one-button `CleanUseCase` (preview, then exactly that preview) and owns no
//! cleanup rule of its own. What it adds is *when*: only while the person has
//! turned it on, once per chosen interval, on AC power, and after the person
//! has left the Mac alone for a while. Every removal still goes through the
//! use case's own checks (open files, ownership, the person's cleanup rules).
//!
//! A run that fails or removes nothing still counts as the interval's run, so
//! a failing condition never turns into a retry loop. The last run is kept in
//! a small state file next to the settings, written atomically.

pub mod schedule;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::models::{AutomaticCleanRun, AutomaticCleanStatus, AutomaticCleanWait};
use crate::services::one_button_clean::ports::{CleanRequest, CleanUseCase};
use crate::services::ports::Silent;
use crate::services::SettingsAuthority;

/// The facts a run waits for, observed when a check happens.
pub trait Conditions: Send + Sync {
    /// `Some(true)` on AC power; `None` when the source is unknown.
    fn on_ac_power(&self) -> Option<bool>;
    /// Seconds since the last keyboard, mouse or trackpad input.
    fn idle_seconds(&self) -> Option<u64>;
}

/// The native observations.
pub struct NativeConditions {
    battery: Arc<dyn crate::power::BatteryProvider>,
}

impl NativeConditions {
    pub fn new(battery: Arc<dyn crate::power::BatteryProvider>) -> Self {
        Self { battery }
    }
}

impl Conditions for NativeConditions {
    fn on_ac_power(&self) -> Option<bool> {
        match self.battery.read().power_source {
            crate::models::PowerSourceType::Ac => Some(true),
            crate::models::PowerSourceType::Battery => Some(false),
            crate::models::PowerSourceType::Unknown => None,
        }
    }

    fn idle_seconds(&self) -> Option<u64> {
        neati_platform::input_idle::seconds_since_last_input()
    }
}

/// Where the person hears what an automatic run did.
pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str);
}

/// A notifier that says nothing, for a process without a desktop.
pub struct NoNotifier;

impl Notifier for NoNotifier {
    fn notify(&self, _title: &str, _body: &str) {}
}

#[derive(Default)]
struct Inner {
    last_run: Option<AutomaticCleanRun>,
    waiting: Option<AutomaticCleanWait>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StateFile {
    last_run: Option<AutomaticCleanRun>,
}

pub struct AutomaticClean {
    clean: Arc<dyn CleanUseCase>,
    settings: Arc<SettingsAuthority>,
    conditions: Arc<dyn Conditions>,
    notifier: Arc<dyn Notifier>,
    state_file: Option<PathBuf>,
    inner: Mutex<Inner>,
    running: AtomicBool,
}

/// Name of the state file in the app's configuration directory.
pub const STATE_FILE_NAME: &str = "automatic-clean.json";

impl AutomaticClean {
    pub fn new(
        clean: Arc<dyn CleanUseCase>,
        settings: Arc<SettingsAuthority>,
        conditions: Arc<dyn Conditions>,
        notifier: Arc<dyn Notifier>,
        state_file: Option<PathBuf>,
    ) -> Self {
        let last_run = state_file
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<StateFile>(&bytes).ok())
            .and_then(|state| state.last_run);
        Self {
            clean,
            settings,
            conditions,
            notifier,
            state_file,
            inner: Mutex::new(Inner {
                last_run,
                waiting: None,
            }),
            running: AtomicBool::new(false),
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn status(&self) -> AutomaticCleanStatus {
        let settings = self
            .settings
            .snapshot()
            .map(|settings| settings.automatic_clean)
            .unwrap_or_default();
        let inner = self.inner();
        AutomaticCleanStatus {
            settings,
            next_due_at: schedule::next_due_at(settings, inner.last_run.as_ref().map(|run| run.at)),
            last_run: inner.last_run.clone(),
            waiting: inner.waiting,
            available: cfg!(target_os = "macos"),
        }
    }

    /// One check: runs Clean when every condition holds, otherwise records
    /// why it waited. Returns the run, when there was one.
    pub async fn tick(&self, now: u64) -> Option<AutomaticCleanRun> {
        let settings = self.settings.snapshot().ok()?.automatic_clean;
        let last = self.inner().last_run.as_ref().map(|run| run.at);
        let verdict = if self.running.load(Ordering::SeqCst) {
            Err(AutomaticCleanWait::Running)
        } else {
            schedule::decide(now, settings, last, || {
                (
                    self.conditions.on_ac_power(),
                    self.conditions.idle_seconds(),
                )
            })
        };
        if let Err(wait) = verdict {
            self.inner().waiting = Some(wait);
            return None;
        }
        if self.running.swap(true, Ordering::SeqCst) {
            self.inner().waiting = Some(AutomaticCleanWait::Running);
            return None;
        }
        let run = self.run(now).await;
        self.running.store(false, Ordering::SeqCst);
        let run = match run {
            Ok(run) => run,
            Err(wait) => {
                // The person came back or unplugged during the scan: nothing
                // was removed, and the next check tries again.
                self.inner().waiting = Some(wait);
                return None;
            }
        };
        {
            let mut inner = self.inner();
            inner.last_run = Some(run.clone());
            inner.waiting = None;
        }
        self.persist(&run);
        self.announce(&run);
        Some(run)
    }

    async fn run(&self, now: u64) -> Result<AutomaticCleanRun, AutomaticCleanWait> {
        let request = CleanRequest::default();
        let preview = match self.clean.preview(request, Arc::new(Silent)).await {
            Ok(preview) => preview,
            Err(error) => return Ok(failed_run(now, error.to_string())),
        };
        if preview.chosen.is_empty() {
            return Ok(AutomaticCleanRun {
                at: now,
                reclaimed_bytes: 0,
                removed: 0,
                failed: 0,
                error: None,
            });
        }
        // A scan takes minutes: the conditions must still hold right before
        // anything is removed.
        schedule::ready((
            self.conditions.on_ac_power(),
            self.conditions.idle_seconds(),
        ))?;
        Ok(match self.clean.execute(&preview, Arc::new(Silent)).await {
            Ok(outcome) => AutomaticCleanRun {
                at: now,
                reclaimed_bytes: outcome.reclaimed_bytes,
                removed: outcome.removed,
                failed: outcome.failed.len() as u64,
                error: None,
            },
            Err(error) => failed_run(now, error.to_string()),
        })
    }

    fn persist(&self, run: &AutomaticCleanRun) {
        let Some(path) = &self.state_file else {
            return;
        };
        let state = StateFile {
            last_run: Some(run.clone()),
        };
        let written = serde_json::to_vec_pretty(&state)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                neati_platform::file_ops::atomic_write(path, &bytes)
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = written {
            crate::diagnostics::log_error(
                "automatic-clean",
                &format!("The automatic Clean record could not be saved: {error}"),
            );
        }
    }

    fn announce(&self, run: &AutomaticCleanRun) {
        if let Some(error) = &run.error {
            self.notifier.notify(
                "Automatic Clean did not run",
                &crate::privacy::paths::mask_paths_in_text(error),
            );
            return;
        }
        if run.removed == 0 && run.failed == 0 {
            return;
        }
        let mut body = format!(
            "Permanently removed {} {} ({}).",
            run.removed,
            if run.removed == 1 { "item" } else { "items" },
            format_bytes(run.reclaimed_bytes)
        );
        if run.failed > 0 {
            body.push_str(&format!(
                " {} could not be removed; open neati for details.",
                run.failed
            ));
        }
        self.notifier
            .notify("neati cleaned up automatically", &body);
    }
}

fn failed_run(now: u64, error: String) -> AutomaticCleanRun {
    AutomaticCleanRun {
        at: now,
        reclaimed_bytes: 0,
        removed: 0,
        failed: 0,
        error: Some(error),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// How often the app checks. A check reads the settings and, while Automatic
/// Clean is off, does nothing else.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// The first check waits until launch work has settled.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5 * 60);

/// Starts the app's periodic check. Only the desktop app calls this; the CLI
/// never schedules anything.
pub fn spawn(automatic: Arc<AutomaticClean>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            automatic.tick(crate::services::ports::unix_now()).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests;
