//! Self-update: one check, one install, both started by the person using neati.
//!
//! The Tauri updater plugin downloads the manifest named in
//! `plugins.updater.endpoints`, verifies the archive against
//! `plugins.updater.pubkey` (a minisign key, independent of Apple code
//! signing), and refuses a signature whose trusted comment names a different
//! version (`requireSignedVersion`). This service owns the one thing the plugin
//! does not: which checked update an install request may act on. The webview
//! never receives a URL or signature; it names the version it was shown, and
//! the install proceeds only if that is the update this process checked.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Runtime};

use crate::models::UpdateCheck;

/// A checked update is installable for this long. After that the person must
/// check again, so an install never acts on a manifest read long ago.
pub(crate) const PENDING_UPDATE_TTL: Duration = Duration::from_secs(30 * 60);
#[cfg(target_os = "macos")]
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(target_os = "macos")]
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

#[cfg(not(target_os = "macos"))]
pub const UNAVAILABLE_PLATFORM: &str =
    "Self-update is available only in the macOS build. Download new versions from the releases page.";
pub const UNAVAILABLE_UNSIGNED: &str =
    "This build has no update signing key, so it cannot verify a download. Download new versions from the releases page.";

/// What an install request found in the pending slot.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PendingLookup {
    Ready,
    NoneChecked,
    Expired,
    VersionMismatch { checked: String },
}

/// The checked update an install may act on, independent of the plugin type so
/// the expiry and version-binding rules are testable without a network.
struct PendingSlot<T> {
    value: Option<(T, String, Instant)>,
}

impl<T> PendingSlot<T> {
    const fn new() -> Self {
        Self { value: None }
    }

    fn store(&mut self, value: T, version: String, now: Instant) {
        self.value = Some((value, version, now));
    }

    fn clear(&mut self) {
        self.value = None;
    }

    /// Takes the pending value only when it is fresh and names `version`.
    /// An expired entry is dropped; a mismatched one is kept for the version
    /// the person was actually shown.
    fn take_for(&mut self, version: &str, now: Instant) -> (PendingLookup, Option<T>) {
        let Some((_, checked, checked_at)) = &self.value else {
            return (PendingLookup::NoneChecked, None);
        };
        if now.saturating_duration_since(*checked_at) > PENDING_UPDATE_TTL {
            self.value = None;
            return (PendingLookup::Expired, None);
        }
        if checked != version {
            return (
                PendingLookup::VersionMismatch {
                    checked: checked.clone(),
                },
                None,
            );
        }
        let (value, _, _) = self.value.take().expect("checked above");
        (PendingLookup::Ready, Some(value))
    }
}

#[cfg(target_os = "macos")]
type PendingUpdate = tauri_plugin_updater::Update;
#[cfg(not(target_os = "macos"))]
type PendingUpdate = ();

pub struct UpdateService {
    pending: Mutex<PendingSlot<PendingUpdate>>,
}

impl Default for UpdateService {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateService {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(PendingSlot::new()),
        }
    }

    /// The public key this build verifies downloads against. Empty means the
    /// build was produced without one and must not offer self-update.
    pub fn configured_pubkey<R: Runtime>(app: &AppHandle<R>) -> String {
        app.config()
            .plugins
            .0
            .get("updater")
            .and_then(|updater| updater.get("pubkey"))
            .and_then(|key| key.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    }

    /// Asks the release manifest whether a newer signed build exists.
    pub async fn check<R: Runtime>(&self, app: &AppHandle<R>) -> UpdateCheck {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            self.lock().clear();
            UpdateCheck::Unavailable {
                reason: UNAVAILABLE_PLATFORM.to_string(),
            }
        }
        #[cfg(target_os = "macos")]
        {
            use tauri_plugin_updater::UpdaterExt;

            let current_version = app.package_info().version.to_string();
            if Self::configured_pubkey(app).is_empty() {
                return UpdateCheck::Unavailable {
                    reason: UNAVAILABLE_UNSIGNED.to_string(),
                };
            }
            let updater = match app.updater_builder().timeout(CHECK_TIMEOUT).build() {
                Ok(updater) => updater,
                Err(error) => {
                    return UpdateCheck::Failed {
                        message: format!("The update check could not start: {error}"),
                    }
                }
            };
            match updater.check().await {
                Ok(Some(update)) => {
                    let version = update.version.clone();
                    let notes = update.body.clone().filter(|body| !body.trim().is_empty());
                    let published_at = update
                        .raw_json
                        .get("pub_date")
                        .and_then(|date| date.as_str())
                        .map(str::to_string);
                    self.lock().store(update, version.clone(), Instant::now());
                    UpdateCheck::Available {
                        current_version,
                        version,
                        notes,
                        published_at,
                    }
                }
                Ok(None) => {
                    self.lock().clear();
                    UpdateCheck::UpToDate { current_version }
                }
                Err(error) => {
                    self.lock().clear();
                    crate::diagnostics::log_error("updater", &format!("check failed: {error}"));
                    UpdateCheck::Failed {
                        message: format!("Could not reach the release manifest: {error}"),
                    }
                }
            }
        }
    }

    /// Downloads, verifies and installs the update this process last checked,
    /// then restarts neati. `version` must be the version the person was shown.
    pub async fn install<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        version: &str,
    ) -> Result<(), String> {
        let (lookup, pending) = self.lock().take_for(version, Instant::now());
        match lookup {
            PendingLookup::Ready => {}
            PendingLookup::NoneChecked => {
                return Err("Check for updates before installing.".to_string())
            }
            PendingLookup::Expired => {
                return Err("This update check expired. Check for updates again.".to_string())
            }
            PendingLookup::VersionMismatch { checked } => {
                return Err(format!(
                    "The last check found {checked}, not {version}. Check for updates again."
                ))
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, pending);
            Err(UNAVAILABLE_PLATFORM.to_string())
        }
        #[cfg(target_os = "macos")]
        {
            let mut update = pending.expect("Ready carries the update");
            update.timeout = Some(DOWNLOAD_TIMEOUT);
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|error| {
                    crate::diagnostics::log_error("updater", &format!("install failed: {error}"));
                    format!("The update could not be installed: {error}")
                })?;
            app.restart();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PendingSlot<PendingUpdate>> {
        // The slot is disposable: a poisoned lock only loses a checked update,
        // which the person can check again.
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_requires_a_prior_check() {
        let mut slot = PendingSlot::<u8>::new();
        assert_eq!(
            slot.take_for("1.0.0", Instant::now()),
            (PendingLookup::NoneChecked, None)
        );
    }

    #[test]
    fn install_takes_the_checked_update_once() {
        let mut slot = PendingSlot::new();
        let now = Instant::now();
        slot.store(7u8, "1.2.3".to_string(), now);
        assert_eq!(slot.take_for("1.2.3", now), (PendingLookup::Ready, Some(7)));
        assert_eq!(
            slot.take_for("1.2.3", now),
            (PendingLookup::NoneChecked, None)
        );
    }

    #[test]
    fn install_refuses_a_version_the_person_was_not_shown() {
        let mut slot = PendingSlot::new();
        let now = Instant::now();
        slot.store(7u8, "1.2.3".to_string(), now);
        assert_eq!(
            slot.take_for("9.9.9", now),
            (
                PendingLookup::VersionMismatch {
                    checked: "1.2.3".to_string()
                },
                None
            )
        );
        // The shown version stays installable.
        assert_eq!(slot.take_for("1.2.3", now), (PendingLookup::Ready, Some(7)));
    }

    #[test]
    fn a_checked_update_expires_and_is_dropped() {
        let mut slot = PendingSlot::new();
        let checked_at = Instant::now();
        slot.store(7u8, "1.2.3".to_string(), checked_at);
        let later = checked_at + PENDING_UPDATE_TTL + Duration::from_secs(1);
        assert_eq!(
            slot.take_for("1.2.3", later),
            (PendingLookup::Expired, None)
        );
        assert_eq!(
            slot.take_for("1.2.3", later),
            (PendingLookup::NoneChecked, None)
        );
    }

    #[test]
    fn exactly_at_the_ttl_the_update_is_still_installable() {
        let mut slot = PendingSlot::new();
        let checked_at = Instant::now();
        slot.store(7u8, "1.2.3".to_string(), checked_at);
        assert_eq!(
            slot.take_for("1.2.3", checked_at + PENDING_UPDATE_TTL),
            (PendingLookup::Ready, Some(7))
        );
    }
}
