//! Agent sessions: the use case behind the inventory and bundles (#508, #509).
//!
//! [`AgentSessionLibrary`] serves the [`ports::SessionLibrary`] driving port
//! from the bounded context in `crate::agent_sessions`, with the home folder
//! taken from the injected `PlatformEnvironment`. [`desk::SessionDesk`] holds
//! one-shot previews for the window, which picks files with system dialogs.
//!
//! Invariants: the inventory is read-only; a bundle never overwrites a file;
//! secrets are reported as counts per kind; nothing reaches the network; no
//! session file is ever deleted.

pub mod desk;
pub mod ports;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use neati_platform::PlatformEnvironment;

use crate::agent_sessions::bundle::{self, ExportSession};
use crate::agent_sessions::inventory::{
    discover, native_folder_state, project_inventory, DiscoveredSession, InventoryLimits,
};
use crate::agent_sessions::{SessionCancel, SessionRoots};
use crate::models::{
    AgentSessionInventory, ProjectFolderState, SessionBundleEntry, SessionBundleProject,
    SessionExportOutcome, SessionImportOutcome, SessionRemap,
};
use crate::services::ports::{unix_now, PortFuture};

use ports::{BundleReview, ExportSelection, SessionLibrary, SessionSelection};

type FolderState = Arc<dyn Fn(&str) -> ProjectFolderState + Send + Sync>;

pub struct AgentSessionLibrary {
    environment: Arc<PlatformEnvironment>,
    limits: InventoryLimits,
    version: String,
    folder_state: FolderState,
}

impl AgentSessionLibrary {
    pub fn new(environment: Arc<PlatformEnvironment>) -> Self {
        Self {
            environment,
            limits: InventoryLimits::default(),
            version: env!("CARGO_PKG_VERSION").to_string(),
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

/// Resolves references to sessions: an inventory key, or a session id that
/// names exactly one session.
pub fn resolve_selection(
    sessions: &[DiscoveredSession],
    selection: &SessionSelection,
) -> Result<Vec<DiscoveredSession>, String> {
    let references = match selection {
        SessionSelection::All => {
            if sessions.is_empty() {
                return Err("There are no agent sessions to export.".to_string());
            }
            return Ok(sessions.to_vec());
        }
        SessionSelection::Only(references) => references,
    };
    if references.is_empty() {
        return Err("Choose at least one session to export.".to_string());
    }
    let mut chosen: BTreeMap<String, DiscoveredSession> = BTreeMap::new();
    for reference in references {
        let matches: Vec<&DiscoveredSession> = sessions
            .iter()
            .filter(|session| session.key == *reference || session.session_id == *reference)
            .collect();
        match matches.as_slice() {
            [] => {
                return Err(format!(
                    "{reference} is not a session in the current inventory. Nothing was exported."
                ))
            }
            [session] => {
                chosen.insert(session.key.clone(), (*session).clone());
            }
            _ => {
                return Err(format!(
                    "{reference} names more than one session; use its key. Nothing was exported."
                ))
            }
        }
    }
    Ok(chosen.into_values().collect())
}

fn export_sessions(sessions: Vec<DiscoveredSession>) -> Vec<ExportSession> {
    sessions
        .into_iter()
        .map(|session| ExportSession {
            tool: session.tool,
            session_id: session.session_id,
            original_cwd: session.cwd,
            files: session.files,
        })
        .collect()
}

fn review(
    bundle: bundle::VerifiedBundle,
    roots: &SessionRoots,
    folder_state: &dyn Fn(&str) -> ProjectFolderState,
) -> Result<BundleReview, String> {
    let planned = bundle::plan_import(&bundle, roots, &[], folder_state)?;
    let entries = planned
        .iter()
        .map(|session| SessionBundleEntry {
            tool: session.tool,
            session_id: session.session_id.clone(),
            original_cwd: session.original_cwd.clone(),
            files: u32::try_from(session.files.len()).unwrap_or(u32::MAX),
            bytes: session
                .files
                .iter()
                .map(|file| file.file.size)
                .fold(0u64, u64::saturating_add),
            existing_files: u32::try_from(session.files.iter().filter(|file| file.exists).count())
                .unwrap_or(u32::MAX),
        })
        .collect();
    let projects = bundle::bundle_projects(&bundle.manifest, folder_state)
        .into_iter()
        .map(|(original_cwd, folder, sessions)| SessionBundleProject {
            original_cwd,
            folder,
            sessions,
        })
        .collect();
    Ok(BundleReview {
        bundle: Arc::new(bundle),
        entries,
        projects,
    })
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

    fn prepare_export(
        &self,
        selection: SessionSelection,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<ExportSelection, String>> {
        Box::pin(async move {
            let roots = self.roots()?;
            let limits = self.limits;
            blocking(
                move || {
                    let discovery = discover(&roots, &limits, &cancel);
                    if discovery.cancelled {
                        return Err("The session operation was stopped.".to_string());
                    }
                    let chosen =
                        export_sessions(resolve_selection(&discovery.sessions, &selection)?);
                    let scan = bundle::scan_for_export(&chosen, &cancel)?;
                    Ok(ExportSelection {
                        sessions: Arc::new(chosen),
                        files: scan.files,
                        bytes: scan.bytes,
                        findings: scan.secrets.findings(),
                    })
                },
                "agent session export preparation",
            )
            .await
        })
    }

    fn export(
        &self,
        selection: ExportSelection,
        destination: PathBuf,
        allow_secrets: bool,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<SessionExportOutcome, String>> {
        Box::pin(async move {
            let version = self.version.clone();
            blocking(
                move || {
                    let report = bundle::write_bundle(
                        &selection.sessions,
                        &destination,
                        allow_secrets,
                        &version,
                        &cancel,
                    )?;
                    Ok(SessionExportOutcome {
                        path: report.path.to_string_lossy().into_owned(),
                        sessions: report.sessions,
                        files: report.files,
                        bytes: report.bytes,
                        archive_bytes: report.archive_bytes,
                        findings: report.secrets.findings(),
                    })
                },
                "agent session export",
            )
            .await
        })
    }

    fn open_bundle(
        &self,
        path: PathBuf,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<BundleReview, String>> {
        Box::pin(async move {
            let roots = self.roots()?;
            let folder_state = self.folder_state.clone();
            blocking(
                move || review(bundle::read_bundle(&path, &cancel)?, &roots, &*folder_state),
                "agent session bundle check",
            )
            .await
        })
    }

    fn import(
        &self,
        review: BundleReview,
        remaps: Vec<SessionRemap>,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<SessionImportOutcome, String>> {
        Box::pin(async move {
            let roots = self.roots()?;
            let folder_state = self.folder_state.clone();
            blocking(
                move || {
                    let report = bundle::import_bundle(
                        &review.bundle,
                        &roots,
                        &remaps,
                        &*folder_state,
                        &cancel,
                    )?;
                    Ok(SessionImportOutcome {
                        sessions: report.sessions,
                        written_files: report.written,
                        kept_existing: report.kept_existing,
                        rewritten_files: report.rewritten,
                    })
                },
                "agent session import",
            )
            .await
        })
    }
}
