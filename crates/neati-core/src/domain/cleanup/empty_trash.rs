//! Private evidence for an explicitly reviewed home-Trash operation.
use crate::domain::identity::FileIdentity;
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashEntry {
    pub relative: PathBuf,
    pub identity: FileIdentity,
    pub directory: bool,
    pub mode: u32,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanos: i64,
    pub allocated_bytes: u64,
    pub links: u64,
}
#[derive(Clone, Debug)]
pub struct TrashSnapshot {
    pub home: PathBuf,
    pub root_identity: FileIdentity,
    pub entries: Vec<TrashEntry>,
}
#[derive(Clone, Debug, Default)]
pub struct EmptyTrashOutcome {
    pub removed_bytes: u64,
    pub removed_entries: usize,
    pub cancelled: bool,
    pub items: Vec<(String, bool, String)>,
}
