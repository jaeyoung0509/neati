//! The agent session library as a driving port (#508).
//!
//! An interface adapter (the CLI, the window's [`super::desk::SessionDesk`])
//! lists sessions through [`SessionLibrary`] only. Nothing here names Tauri, a
//! terminal or stdout, and nothing here deletes a session file.

use crate::agent_sessions::SessionCancel;
use crate::models::AgentSessionInventory;
use crate::services::ports::PortFuture;

pub trait SessionLibrary: Send + Sync {
    /// The read-only inventory, grouped by project.
    fn inventory(
        &self,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<AgentSessionInventory, String>>;
}
