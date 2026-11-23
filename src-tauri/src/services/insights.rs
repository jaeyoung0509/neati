//! Read-only machine insights as a driving port (#498).
//!
//! [`InsightQueries`] is what an interface adapter may read about the machine
//! without changing it: disk and memory, development listeners, completed
//! cleanups and AI tool usage. [`MachineInsights`] serves it from the existing
//! services; a test serves it from a fake.

use std::sync::Arc;

use crate::models::{
    AiProviderUsage, AiUsageSnapshot, CleanupHistoryEntry, DevelopmentListener, DiskMetrics,
    DockerStatus, InstalledAppInventory, LargeFileScanEvent, LargeFileScanRequest,
    LargeFileScanResult, MemoryMetrics,
};
use crate::services::ports::{Activity, ActivityPort, PortFuture};
use crate::services::progress::{LargeFileScanSink, ProviderUsageSink};
use crate::services::{AiService, CleanupService, StorageService, SystemService};

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
    /// Large files under the stated roots. Reading only: removal stays a
    /// reviewed workflow in the window.
    fn large_files(
        &self,
        request: LargeFileScanRequest,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<LargeFileScanResult, String>>;
    /// Installed applications. Reading only: uninstalling stays in the window.
    fn installed_apps(&self) -> PortFuture<'_, Result<InstalledAppInventory, String>>;
    fn docker(&self) -> PortFuture<'_, Result<DockerStatus, String>>;
}

pub struct MachineInsights {
    system: Arc<SystemService>,
    cleanup: Arc<CleanupService>,
    storage: Arc<StorageService>,
    ai: Arc<AiService>,
}

impl MachineInsights {
    pub fn new(
        system: Arc<SystemService>,
        cleanup: Arc<CleanupService>,
        storage: Arc<StorageService>,
        ai: Arc<AiService>,
    ) -> Self {
        Self {
            system,
            cleanup,
            storage,
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

    fn large_files(
        &self,
        request: LargeFileScanRequest,
        activity: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<LargeFileScanResult, String>> {
        Box::pin(async move {
            let sink: Arc<dyn LargeFileScanSink> = Arc::new(LargeFileActivity(activity));
            self.storage.scan_large_files(request, sink).await
        })
    }

    fn installed_apps(&self) -> PortFuture<'_, Result<InstalledAppInventory, String>> {
        Box::pin(self.storage.installed_apps())
    }

    fn docker(&self) -> PortFuture<'_, Result<DockerStatus, String>> {
        Box::pin(self.system.docker_status())
    }
}

struct LargeFileActivity(Arc<dyn ActivityPort>);

impl LargeFileScanSink for LargeFileActivity {
    fn emit(&self, event: LargeFileScanEvent) {
        if let LargeFileScanEvent::RootStarted { root, .. } = event {
            self.0.report(Activity::Scanning { location: root });
        }
    }
}

struct ProviderActivity(Arc<dyn ActivityPort>);

impl ProviderUsageSink for ProviderActivity {
    fn emit(&self, usage: AiProviderUsage) {
        self.0.report(Activity::Checking { source: usage.name });
    }
}
