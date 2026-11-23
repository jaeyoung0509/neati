//! Read-only machine insights as a driving port (#498).
//!
//! [`InsightQueries`] is what an interface adapter may read about the machine
//! without changing it: disk and memory, development listeners, completed
//! cleanups and AI tool usage. [`MachineInsights`] serves it from the existing
//! services; a test serves it from a fake.

use std::sync::Arc;

use crate::models::{
    AiProviderUsage, AiUsageSnapshot, CleanupHistoryEntry, DevelopmentListener, DiskMetrics,
    MemoryMetrics,
};
use crate::services::ports::{Activity, ActivityPort, PortFuture};
use crate::services::progress::ProviderUsageSink;
use crate::services::{AiService, CleanupService, SystemService};

/// Disk, memory and the most recent cleanup, read together.
#[derive(Debug, Clone)]
pub struct MachineStatus {
    pub disk: DiskMetrics,
    pub memory: MemoryMetrics,
    pub last_cleanup: Option<CleanupHistoryEntry>,
}

pub trait InsightQueries: Send + Sync {
    fn status(&self) -> PortFuture<'_, Result<MachineStatus, String>>;
    fn listeners(&self) -> PortFuture<'_, Result<Vec<DevelopmentListener>, String>>;
    fn history(&self) -> PortFuture<'_, Result<Vec<CleanupHistoryEntry>, String>>;
    /// Provider usage; `refresh` bypasses the bounded cache.
    fn ai_usage(
        &self,
        refresh: bool,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<AiUsageSnapshot, String>>;
}

pub struct MachineInsights {
    system: Arc<SystemService>,
    cleanup: Arc<CleanupService>,
    ai: Arc<AiService>,
}

impl MachineInsights {
    pub fn new(
        system: Arc<SystemService>,
        cleanup: Arc<CleanupService>,
        ai: Arc<AiService>,
    ) -> Self {
        Self {
            system,
            cleanup,
            ai,
        }
    }
}

impl InsightQueries for MachineInsights {
    fn status(&self) -> PortFuture<'_, Result<MachineStatus, String>> {
        Box::pin(async move {
            let disk = self.system.disk_metrics().await?;
            let memory = self.system.memory_metrics().await?;
            // History is context, not the status: an unreadable history file
            // must not turn a disk and memory reading into a failure.
            let last_cleanup = self
                .cleanup
                .cleanup_history()
                .await
                .ok()
                .and_then(|runs| runs.into_iter().next());
            Ok(MachineStatus {
                disk,
                memory,
                last_cleanup,
            })
        })
    }

    fn listeners(&self) -> PortFuture<'_, Result<Vec<DevelopmentListener>, String>> {
        Box::pin(self.system.development_listeners())
    }

    fn history(&self) -> PortFuture<'_, Result<Vec<CleanupHistoryEntry>, String>> {
        Box::pin(self.cleanup.cleanup_history())
    }

    fn ai_usage(
        &self,
        refresh: bool,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<AiUsageSnapshot, String>> {
        Box::pin(async move {
            let sink: Arc<dyn ProviderUsageSink> = Arc::new(ProviderActivity(activity));
            self.ai.usage_snapshot(refresh, sink).await
        })
    }
}

struct ProviderActivity(Arc<dyn ActivityPort>);

impl ProviderUsageSink for ProviderActivity {
    fn emit(&self, usage: AiProviderUsage) {
        self.0.report(Activity::Checking { source: usage.name });
    }
}
