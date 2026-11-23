//! One-shot session bundle workflows for the window (#509).
//!
//! A terminal run prepares and writes a bundle in one process. The window
//! shows what an export or import would do, waits for a click, and then asks
//! for exactly that to happen: the prepared export or verified bundle stays
//! here behind an opaque id, and the window submits only the id and the
//! person's answers. Paths come from system dialogs opened here, never from
//! the window. A finished export or import consumes its id; ids expire, and
//! the store is capped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::agent_sessions::picker::BundlePicker;
use crate::agent_sessions::SessionCancel;
use crate::models::{
    AgentSessionInventory, SessionBundlePreview, SessionExportOutcome, SessionExportPreview,
    SessionImportOutcome, SessionRemap,
};

use super::ports::{BundleReview, ExportSelection, SessionLibrary, SessionSelection};

/// How long a shown preview may be acted on.
pub const HELD_TTL: Duration = Duration::from_secs(15 * 60);
/// Previews of each kind held at once.
pub const HELD_CAP: usize = 4;

struct Held<T> {
    entries: Mutex<HashMap<Uuid, (Instant, T)>>,
    ttl: Duration,
}

impl<T: Clone> Held<T> {
    fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, (Instant, T)>> {
        // The entries are disposable previews; a poisoned lock is recovered.
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn insert(&self, value: T) -> Uuid {
        let mut entries = self.lock();
        let now = Instant::now();
        entries.retain(|_, (created, _)| now.duration_since(*created) < self.ttl);
        while entries.len() >= HELD_CAP {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (created, _))| *created)
                .map(|(id, _)| *id)
            else {
                break;
            };
            entries.remove(&oldest);
        }
        let id = Uuid::new_v4();
        entries.insert(id, (now, value));
        id
    }

    fn peek(&self, id: &Uuid) -> Option<T> {
        let entries = self.lock();
        entries
            .get(id)
            .filter(|(created, _)| created.elapsed() < self.ttl)
            .map(|(_, value)| value.clone())
    }

    fn take(&self, id: &Uuid) -> Option<T> {
        let mut entries = self.lock();
        entries
            .remove(id)
            .filter(|(created, _)| created.elapsed() < self.ttl)
            .map(|(_, value)| value)
    }
}

fn expired() -> String {
    "This review expired or was already used. Prepare it again.".to_string()
}

pub struct SessionDesk {
    library: Arc<dyn SessionLibrary>,
    picker: Arc<dyn BundlePicker>,
    exports: Held<ExportSelection>,
    imports: Held<BundleReview>,
    running: Mutex<Option<SessionCancel>>,
}

impl SessionDesk {
    pub fn new(library: Arc<dyn SessionLibrary>, picker: Arc<dyn BundlePicker>) -> Self {
        Self::with_ttl(library, picker, HELD_TTL)
    }

    pub fn with_ttl(
        library: Arc<dyn SessionLibrary>,
        picker: Arc<dyn BundlePicker>,
        ttl: Duration,
    ) -> Self {
        Self {
            library,
            picker,
            exports: Held::new(ttl),
            imports: Held::new(ttl),
            running: Mutex::new(None),
        }
    }

    /// A fresh stop signal for the operation that starts now.
    fn begin(&self) -> SessionCancel {
        let cancel = SessionCancel::new();
        *self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(cancel.clone());
        cancel
    }

    /// Forgets `cancel` once its operation ended, unless a newer operation
    /// already replaced it, so a finished operation leaves no handle behind.
    fn finish(&self, cancel: &SessionCancel) {
        let mut running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if running
            .as_ref()
            .is_some_and(|current| current.is_same(cancel))
        {
            *running = None;
        }
    }

    /// Whether no session operation holds a stop signal.
    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none()
    }

    /// Stops the running session operation, if any.
    pub fn cancel(&self) {
        if let Some(cancel) = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            cancel.cancel();
        }
    }

    pub async fn inventory(&self) -> Result<AgentSessionInventory, String> {
        let cancel = self.begin();
        let result = self.library.inventory(cancel.clone()).await;
        self.finish(&cancel);
        result
    }

    /// Resolves the chosen keys and counts likely secrets.
    pub async fn prepare_export(&self, keys: Vec<String>) -> Result<SessionExportPreview, String> {
        if keys.is_empty() {
            return Err("Choose at least one session to export.".to_string());
        }
        let cancel = self.begin();
        let selection = self
            .library
            .prepare_export(SessionSelection::Only(keys), cancel.clone())
            .await;
        self.finish(&cancel);
        let selection = selection?;
        let preview = SessionExportPreview {
            id: Uuid::nil(),
            sessions: u32::try_from(selection.sessions.len()).unwrap_or(u32::MAX),
            files: selection.files,
            bytes: selection.bytes,
            findings: selection.findings.clone(),
        };
        let id = self.exports.insert(selection);
        Ok(SessionExportPreview { id, ..preview })
    }

    /// Asks where to save, then writes the held export. `None` when the
    /// person closed the dialog; the preview stays available.
    pub async fn export(
        &self,
        id: Uuid,
        allow_secrets: bool,
    ) -> Result<Option<SessionExportOutcome>, String> {
        let selection = self.exports.peek(&id).ok_or_else(expired)?;
        if !selection.findings.is_empty() && !allow_secrets {
            return Err(
                "Likely secrets were found. Confirm that the bundle may include them.".to_string(),
            );
        }
        let picker = self.picker.clone();
        let Some(destination) = crate::blocking::run_blocking(
            move || picker.export_destination(),
            "session bundle dialog",
        )
        .await?
        else {
            return Ok(None);
        };
        let selection = self.exports.take(&id).ok_or_else(expired)?;
        let cancel = self.begin();
        let outcome = self
            .library
            .export(selection, destination, allow_secrets, cancel.clone())
            .await;
        self.finish(&cancel);
        outcome.map(Some)
    }

    /// Asks for a bundle and verifies it. `None` when the dialog was closed.
    pub async fn open_bundle(&self) -> Result<Option<SessionBundlePreview>, String> {
        let picker = self.picker.clone();
        let Some(path) = crate::blocking::run_blocking(
            move || picker.bundle_to_import(),
            "session bundle dialog",
        )
        .await?
        else {
            return Ok(None);
        };
        let cancel = self.begin();
        let review = self.library.open_bundle(path, cancel.clone()).await;
        self.finish(&cancel);
        let review = review?;
        let preview = SessionBundlePreview {
            id: Uuid::nil(),
            name: review.bundle.name(),
            neati_version: review.bundle.manifest.neati_version.clone(),
            sessions: review.entries.clone(),
            projects: review.projects.clone(),
        };
        let id = self.imports.insert(review);
        Ok(Some(SessionBundlePreview { id, ..preview }))
    }

    /// A project folder on this Mac for a remap. `None` when the dialog was
    /// closed.
    pub async fn pick_project_folder(&self) -> Result<Option<String>, String> {
        let picker = self.picker.clone();
        let folder =
            crate::blocking::run_blocking(move || picker.project_folder(), "project folder dialog")
                .await?;
        Ok(folder.map(|path| {
            let text = path.to_string_lossy().into_owned();
            let trimmed = text.trim_end_matches('/');
            if trimmed.is_empty() {
                text
            } else {
                trimmed.to_string()
            }
        }))
    }

    /// Imports exactly the held bundle. A completed import consumes the id; a
    /// refused one (a remap the backend would not accept) keeps it, and a
    /// retry still never replaces a file the first attempt wrote.
    pub async fn import(
        &self,
        id: Uuid,
        remaps: Vec<SessionRemap>,
    ) -> Result<SessionImportOutcome, String> {
        let review = self.imports.peek(&id).ok_or_else(expired)?;
        let cancel = self.begin();
        let outcome = self.library.import(review, remaps, cancel.clone()).await;
        self.finish(&cancel);
        let outcome = outcome?;
        self.imports.take(&id);
        Ok(outcome)
    }
}
