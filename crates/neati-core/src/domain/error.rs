use std::fmt;

use crate::domain::cleanup::PlanItemRefusal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeatiError {
    PermissionDenied(String),
    PathNotAllowed(String),
    SymlinkEscape(String),
    ChangedSinceScan(String),
    /// The path no longer exists. Absence is deliberately not
    /// [`NeatiError::ChangedSinceScan`]: the caller's postcondition already
    /// holds, so it is classified as already-absent rather than as a mutation
    /// failure. Every other failure still fails closed.
    Missing(String),
    SignatureMismatch(String),
    ToolUnavailable(String),
    ExternalCommandFailed(String),
    BlacklistedPath(String),
    InvalidPlan(String),
    UnsupportedManualOperation(String),
    /// Nothing the caller selected could be authorized, and every item states
    /// why.
    ///
    /// This is an answer about the items rather than a failure of the scan: a
    /// current policy refused each of them, so the inventory the caller holds
    /// is still the truth about the machine and the refusal is stated per item
    /// instead of discarding the selection.
    RefusedSelection(Vec<PlanItemRefusal>),
    Io(String),
}

impl fmt::Display for NeatiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NeatiError::PermissionDenied(p) => write!(f, "Permission denied for path: {}", p),
            NeatiError::PathNotAllowed(p) => write!(f, "Path is not allowed: {}", p),
            NeatiError::SymlinkEscape(p) => write!(f, "Symlink escape attempt rejected: {}", p),
            NeatiError::ChangedSinceScan(p) => write!(f, "File changed since scan: {}", p),
            NeatiError::Missing(p) => write!(f, "Path no longer exists: {}", p),
            NeatiError::SignatureMismatch(id) => write!(f, "Signature mismatch: {}", id),
            NeatiError::ToolUnavailable(t) => write!(f, "Tool unavailable: {}", t),
            NeatiError::ExternalCommandFailed(e) => write!(f, "External command failed: {}", e),
            NeatiError::BlacklistedPath(p) => {
                write!(f, "Attempted operation on blacklisted path: {}", p)
            }
            NeatiError::InvalidPlan(msg) => write!(f, "Invalid delete plan: {}", msg),
            NeatiError::UnsupportedManualOperation(name) => {
                write!(f, "Manual item requires a dedicated adapter: {}", name)
            }
            NeatiError::RefusedSelection(refusals) => {
                let first = refusals
                    .first()
                    .map(|refusal| refusal.message.as_str())
                    .unwrap_or("no item in the selection can be cleaned");
                write!(
                    f,
                    "Nothing in the selection can be cleaned ({} item(s) refused): {}",
                    refusals.len(),
                    first
                )
            }
            NeatiError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for NeatiError {}

impl From<std::io::Error> for NeatiError {
    fn from(err: std::io::Error) -> Self {
        NeatiError::Io(err.to_string())
    }
}

pub type NeatiResult<T> = Result<T, NeatiError>;
