//! The agent session library as a driving port (#508, #509).
//!
//! An interface adapter (the CLI, the window's [`super::desk::SessionDesk`])
//! lists sessions, prepares and writes a bundle, and verifies and imports one
//! through [`SessionLibrary`] only. Nothing here names Tauri, a terminal or
//! stdout, and nothing here deletes a session file.

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_sessions::bundle::{ExportSession, VerifiedBundle};
use crate::agent_sessions::SessionCancel;
use crate::models::{
    AgentSessionInventory, SecretFinding, SessionBundleEntry, SessionBundleProject,
    SessionExportOutcome, SessionImportOutcome, SessionRemap,
};
use crate::services::ports::PortFuture;

/// Which sessions an export takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSelection {
    /// Every session the inventory lists.
    All,
    /// Session keys from the inventory, or session ids. An unknown or
    /// ambiguous reference refuses the whole export.
    Only(Vec<String>),
}

/// Chosen sessions resolved to their files, and what the secret scan found.
#[derive(Debug, Clone)]
pub struct ExportSelection {
    pub sessions: Arc<Vec<ExportSession>>,
    pub files: u32,
    pub bytes: u64,
    /// Counts per kind; never a value.
    pub findings: Vec<SecretFinding>,
}

/// A verified bundle and what importing it would do on this machine.
#[derive(Debug, Clone)]
pub struct BundleReview {
    pub bundle: Arc<VerifiedBundle>,
    pub entries: Vec<SessionBundleEntry>,
    pub projects: Vec<SessionBundleProject>,
}

pub trait SessionLibrary: Send + Sync {
    /// The read-only inventory, grouped by project.
    fn inventory(
        &self,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<AgentSessionInventory, String>>;

    /// Resolves the selection and counts likely secrets. Writes nothing.
    fn prepare_export(
        &self,
        selection: SessionSelection,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<ExportSelection, String>>;

    /// Writes the bundle to `destination`, which must not exist. Refuses when
    /// secrets are found and `allow_secrets` is false.
    fn export(
        &self,
        selection: ExportSelection,
        destination: PathBuf,
        allow_secrets: bool,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<SessionExportOutcome, String>>;

    /// Reads a bundle and verifies its manifest and every checksum.
    fn open_bundle(
        &self,
        path: PathBuf,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<BundleReview, String>>;

    /// Imports a verified bundle; existing files are kept and reported.
    fn import(
        &self,
        review: BundleReview,
        remaps: Vec<SessionRemap>,
        cancel: SessionCancel,
    ) -> PortFuture<'_, Result<SessionImportOutcome, String>>;
}
