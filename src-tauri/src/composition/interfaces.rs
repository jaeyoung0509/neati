//! Binding the interface-agnostic use cases to the services (#498).
//!
//! The driving ports an adapter may call are built here, from the same
//! `DesktopState` the window uses, and each driven port is bound to its
//! in-process adapter here and nowhere else. An adapter receives only the
//! ports; it never sees a service or reaches into `DesktopState`.

use std::sync::Arc;

use crate::commands::DesktopState;
use crate::services::command_line::CommandLineService;
use crate::services::insights::{InsightQueries, MachineInsights};
use crate::services::one_button_clean::adapters::{
    ServiceCacheCleanup, ServiceProjectCleanup, ServiceWorktreeCleanup,
};
use crate::services::one_button_clean::ports::CleanUseCase;
use crate::services::one_button_clean::sessions::OneButtonSessions;
use crate::services::one_button_clean::OneButtonClean;

/// The driving ports an interface adapter is given.
#[derive(Clone)]
pub struct ApplicationPorts {
    pub clean: Arc<dyn CleanUseCase>,
    /// The same use case for an interface that shows a preview and executes
    /// it later by id (the window and the Quick Panel).
    pub clean_sessions: Arc<OneButtonSessions>,
    pub insights: Arc<dyn InsightQueries>,
    /// The `neati` command and the agent skill (#500).
    pub command_line: Arc<CommandLineService>,
}

pub fn application_ports(state: &DesktopState) -> ApplicationPorts {
    let caches = Arc::new(ServiceCacheCleanup::new(
        state.cleanup.clone(),
        state.settings.clone(),
        state.catalog_failure(),
    ));
    let projects = Arc::new(ServiceProjectCleanup::new(
        state.storage.clone(),
        state.settings.clone(),
    ));
    let worktrees = Arc::new(ServiceWorktreeCleanup::new(state.worktrees.clone()));
    let clean: Arc<dyn CleanUseCase> = Arc::new(OneButtonClean::new(caches, projects, worktrees));
    ApplicationPorts {
        clean_sessions: Arc::new(OneButtonSessions::new(clean.clone())),
        clean,
        insights: Arc::new(MachineInsights::new(
            state.system.clone(),
            state.cleanup.clone(),
            state.storage.clone(),
            state.ai.clone(),
        )),
        // The running executable is a process fact observed here, once, as
        // the login item observes it.
        command_line: Arc::new(CommandLineService::new(
            state.environment.clone(),
            std::env::current_exe().ok(),
        )),
    }
}
