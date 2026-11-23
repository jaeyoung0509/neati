//! Agent sessions: the use case behind the inventory (#508).
//!
//! [`AgentSessionLibrary`] serves the [`ports::SessionLibrary`] driving port
//! from the bounded context in `crate::agent_sessions`, with the home folder
//! taken from the injected `PlatformEnvironment`. [`desk::SessionDesk`] serves
//! the window and owns the stop signal of the running read.
//!
//! Invariants: the inventory is read-only; nothing reaches the network; no
//! session file is ever changed or deleted.

pub mod desk;
pub mod ports;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use neati_platform::PlatformEnvironment;

use crate::agent_sessions::inventory::{
    discover, native_folder_state, project_inventory, InventoryLimits,
};
use crate::agent_sessions::{SessionCancel, SessionRoots};
use crate::models::{AgentSessionInventory, ProjectFolderState};
use crate::services::ports::{unix_now, PortFuture};

use ports::SessionLibrary;

type FolderState = Arc<dyn Fn(&str) -> ProjectFolderState + Send + Sync>;

pub struct AgentSessionLibrary {
    environment: Arc<PlatformEnvironment>,
    limits: InventoryLimits,
    folder_state: FolderState,
}

impl AgentSessionLibrary {
    pub fn new(environment: Arc<PlatformEnvironment>) -> Self {
        Self {
            environment,
            limits: InventoryLimits::default(),
            folder_state: Arc::new(native_folder_state),
        }
    }

    /// The same library with other read limits (tests, fixtures).
    pub fn with_limits(mut self, limits: InventoryLimits) -> Self {
        self.limits = limits;
        self
    }

    fn roots(&self) -> Result<SessionRoots, String> {
        SessionRoots::from_environment(&self.environment).ok_or_else(|| {
            "The home folder is unknown, so agent sessions cannot be found.".to_string()
        })
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    context: &'static str,
) -> Result<T, String> {
    crate::blocking::run_blocking(work, context).await
}

impl SessionLibrary for AgentSessionLibrary {
    fn inventory(
        &self,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<AgentSessionInventory, String>> {
        Box::pin(async move {
            let roots = self.roots()?;
            let limits = self.limits;
            let folder_state = self.folder_state.clone();
            blocking(
                move || {
                    let discovery = discover(&roots, &limits, &cancel);
                    Ok(project_inventory(&discovery, unix_now(), &*folder_state))
                },
                "agent session inventory",
            )
            .await
        })
    }
}
