//! One-shot previews for interfaces that cannot hold a preview themselves.
//!
//! A terminal run previews and executes inside one process, so it keeps the
//! preview in hand. A window shows the preview, waits for a click, and then
//! asks for exactly that preview to run: the preview stays here, behind an
//! opaque id, and the interface submits only the id. Each id runs at most once,
//! expires, and the store is capped, as every lifecycle-owned registry is.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::services::ports::ActivityPort;

use super::ports::{CleanOutcome, CleanPreview, CleanRequest, CleanUseCase, OneButtonError};

/// How long a shown preview may be executed. The scans it was chosen from are
/// valid for cleanup for five minutes; a preview never outlives that.
pub const PREVIEW_TTL: Duration = Duration::from_secs(300);
/// Previews held at once. Two surfaces (the window and the Quick Panel) each
/// need one; the rest is room for a preview replaced mid-flight.
pub const PREVIEW_CAP: usize = 4;

struct Held {
    preview: Arc<CleanPreview>,
    created: Instant,
}

pub struct OneButtonSessions {
    use_case: Arc<dyn CleanUseCase>,
    held: Mutex<HashMap<Uuid, Held>>,
    ttl: Duration,
}

impl OneButtonSessions {
    pub fn new(use_case: Arc<dyn CleanUseCase>) -> Self {
        Self::with_ttl(use_case, PREVIEW_TTL)
    }

    pub fn with_ttl(use_case: Arc<dyn CleanUseCase>, ttl: Duration) -> Self {
        Self {
            use_case,
            held: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Previews and holds the result for one later execution.
    pub async fn preview(
        &self,
        request: CleanRequest,
        activity: Arc<dyn ActivityPort>,
    ) -> Result<(Uuid, Arc<CleanPreview>), OneButtonError> {
        let preview = Arc::new(self.use_case.preview(request, activity).await?);
        let id = Uuid::new_v4();
        let mut held = self.lock();
        let now = Instant::now();
        held.retain(|_, entry| now.duration_since(entry.created) < self.ttl);
        while held.len() >= PREVIEW_CAP {
            let Some(oldest) = held
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(id, _)| *id)
            else {
                break;
            };
            held.remove(&oldest);
        }
        held.insert(
            id,
            Held {
                preview: preview.clone(),
                created: now,
            },
        );
        Ok((id, preview))
    }

    /// Executes the preview `id` named, once. The id is consumed before the
    /// work starts, so a repeated or concurrent submit cannot run it twice,
    /// and a failed run is not retried from the same preview.
    pub async fn execute(
        &self,
        id: Uuid,
        activity: Arc<dyn ActivityPort>,
    ) -> Result<CleanOutcome, OneButtonError> {
        let held = self
            .lock()
            .remove(&id)
            .filter(|entry| entry.created.elapsed() < self.ttl)
            .ok_or(OneButtonError::PreviewExpired)?;
        self.use_case.execute(&held.preview, activity).await
    }

    /// Previews still held, for tests and diagnostics.
    pub fn held_count(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Held>> {
        // The map holds only disposable previews; a poisoned lock loses
        // nothing a person could not recreate by checking again.
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
