//! When automatic Clean may run. Pure: the clock and the observations are
//! arguments, so the rule is tested without waiting or a real machine.

use crate::models::{AutomaticCleanSettings, AutomaticCleanWait};

/// The person must have left the Mac alone at least this long.
pub const MIN_IDLE_SECONDS: u64 = 10 * 60;

/// The earliest time the next run may start, while Automatic Clean is on.
pub fn next_due_at(settings: AutomaticCleanSettings, last_run_at: Option<u64>) -> Option<u64> {
    if !settings.enabled {
        return None;
    }
    Some(last_run_at.map_or(0, |last| last.saturating_add(settings.interval.seconds())))
}

/// `Ok` when a run may start now. `observe` is only called once the cheap
/// checks pass, so a disabled or not-yet-due schedule observes nothing.
pub fn decide(
    now: u64,
    settings: AutomaticCleanSettings,
    last_run_at: Option<u64>,
    observe: impl FnOnce() -> (Option<bool>, Option<u64>),
) -> Result<(), AutomaticCleanWait> {
    if !settings.enabled {
        return Err(AutomaticCleanWait::Disabled);
    }
    if let Some(due) = next_due_at(settings, last_run_at) {
        if now < due {
            return Err(AutomaticCleanWait::NotDue);
        }
    }
    let (on_ac, idle) = observe();
    match on_ac {
        Some(true) => {}
        Some(false) => return Err(AutomaticCleanWait::OnBattery),
        None => return Err(AutomaticCleanWait::ConditionsUnknown),
    }
    match idle {
        Some(seconds) if seconds >= MIN_IDLE_SECONDS => Ok(()),
        Some(_) => Err(AutomaticCleanWait::PersonActive),
        None => Err(AutomaticCleanWait::ConditionsUnknown),
    }
}
