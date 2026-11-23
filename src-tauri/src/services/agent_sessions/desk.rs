//! The session library as the window uses it (#508).
//!
//! The desk owns the stop signal of the running session read, so the window
//! can stop it by asking, without holding a handle itself.

use std::sync::{Arc, Mutex};

use crate::agent_sessions::SessionCancel;
use crate::models::AgentSessionInventory;

use super::ports::SessionLibrary;

pub struct SessionDesk {
    library: Arc<dyn SessionLibrary>,
    running: Mutex<Option<SessionCancel>>,
}

impl SessionDesk {
    pub fn new(library: Arc<dyn SessionLibrary>) -> Self {
        Self {
            library,
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
}
