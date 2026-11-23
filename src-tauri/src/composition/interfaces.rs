//! Binding the interface-agnostic use cases to the services (#498).
//!
//! The driving ports an adapter may call are built here, from the same
//! `DesktopState` the window uses, and each driven port is bound to its
//! in-process adapter here and nowhere else. An adapter receives only the
//! ports; it never sees a service or reaches into `DesktopState`.

use std::sync::Arc;

use crate::commands::DesktopState;
use crate::services::insights::{InsightQueries, MachineInsights};
use crate::services::one_button_clean::adapters::{ServiceCacheCleanup, ServiceProjectCleanup};
use crate::services::one_button_clean::ports::CleanUseCase;
use crate::services::one_button_clean::OneButtonClean;

/// The driving ports an interface adapter is given.
#[derive(Clone)]
pub struct ApplicationPorts {
    pub clean: Arc<dyn CleanUseCase>,
    pub insights: Arc<dyn InsightQueries>,
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
    ApplicationPorts {
        clean: Arc::new(OneButtonClean::new(caches, projects)),
        insights: Arc::new(MachineInsights::new(
            state.system.clone(),
            state.cleanup.clone(),
            state.ai.clone(),
        )),
    }
}
