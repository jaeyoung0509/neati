//! Bounded, de-identified cleanup operation history.
//!
//! History is operational evidence, not another inventory. It records only
//! aggregate outcomes; item names, identifiers, paths, provider output, and
//! error messages never enter the file.

use std::fs;
use std::io;
use std::path::PathBuf;

use neati_platform::PlatformEnvironment;
use serde::{Deserialize, Serialize};

use crate::models::{CleanResult, CleanupHistoryEntry};

const HISTORY_FILE_NAME: &str = "cleanup-history.jsonl";
const MAX_HISTORY_ENTRIES: usize = 200;

#[derive(Debug, Serialize, Deserialize)]
struct CleanupHistoryRecord {
    schema: u8,
    started_at: u64,
    finished_at: u64,
    target_count: usize,
    reclaimed_bytes: u64,
    moved_to_trash_bytes: u64,
    failed_bytes: u64,
    partial_count: u64,
    failed_count: u64,
    skipped_count: u64,
}

impl From<&CleanResult> for CleanupHistoryRecord {
    fn from(result: &CleanResult) -> Self {
        Self {
            schema: 1,
            started_at: result.started_at,
            finished_at: result.finished_at,
            target_count: result.items.len(),
            reclaimed_bytes: result.total_reclaimed_bytes,
            moved_to_trash_bytes: result.total_moved_to_trash_bytes,
            failed_bytes: result.total_failed_bytes,
            partial_count: result.partial_count,
            failed_count: result.failed_count,
            skipped_count: result.skipped_count,
        }
    }
}

fn history_path(environment: &PlatformEnvironment) -> PathBuf {
    crate::diagnostics::log_dir(environment).join(HISTORY_FILE_NAME)
}

pub(crate) fn record(result: &CleanResult, environment: &PlatformEnvironment) -> io::Result<()> {
    let path = history_path(environment);
    let existing = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut lines: Vec<&str> = existing.lines().collect();
    let keep = MAX_HISTORY_ENTRIES.saturating_sub(1);
    if lines.len() > keep {
        lines.drain(..lines.len() - keep);
    }

    let record = serde_json::to_string(&CleanupHistoryRecord::from(result))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut contents = lines.join("\n");
    if !contents.is_empty() {
        contents.push('\n');
    }
    contents.push_str(&record);
    contents.push('\n');
    neati_platform::file_ops::atomic_write(&path, contents.as_bytes())
}

/// The largest value the interface can show exactly; larger stored values
/// saturate instead of failing the whole list.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Reads the stored history, newest first.
///
/// A missing file is an empty history. A line that does not parse (an older
/// schema, a torn write) is skipped so one bad record cannot hide the rest.
pub(crate) fn list(environment: &PlatformEnvironment) -> io::Result<Vec<CleanupHistoryEntry>> {
    let contents = match fs::read_to_string(history_path(environment)) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut entries: Vec<CleanupHistoryEntry> = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<CleanupHistoryRecord>(line).ok())
        .map(|record| CleanupHistoryEntry {
            finished_at: record.finished_at.min(MAX_SAFE_INTEGER),
            item_count: u64::try_from(record.target_count).unwrap_or(MAX_SAFE_INTEGER),
            removed_bytes: record.reclaimed_bytes.min(MAX_SAFE_INTEGER),
            partial_count: record.partial_count.min(MAX_SAFE_INTEGER),
            failed_count: record.failed_count.min(MAX_SAFE_INTEGER),
            skipped_count: record.skipped_count.min(MAX_SAFE_INTEGER),
        })
        .collect();
    // An oversized file still returns only the newest window.
    if entries.len() > MAX_HISTORY_ENTRIES {
        entries.drain(..entries.len() - MAX_HISTORY_ENTRIES);
    }
    entries.reverse();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CleanItemResult, CleanStatus};
    use neati_platform::path_algebra::PathFlavor;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn result_with_private_item_data() -> CleanResult {
        CleanResult {
            plan_id: Uuid::new_v4(),
            started_at: 10,
            finished_at: 12,
            total_reclaimed_bytes: 42,
            total_moved_to_trash_bytes: 0,
            total_failed_bytes: 0,
            partial_count: 0,
            failed_count: 0,
            skipped_count: 0,
            items: vec![CleanItemResult {
                item_id: "private-item-id".into(),
                name: "Private cache name".into(),
                path: "/Users/example/private/cache".into(),
                status: CleanStatus::Success,
                success: true,
                estimated_bytes: 42,
                bytes_reclaimed: 42,
                moved_to_trash_bytes: 0,
                failure_reason: None,
                error_message: None,
            }],
            actual_disk_free_delta: Some(42),
        }
    }

    #[test]
    fn history_contains_aggregates_but_no_inventory_data() {
        let fixture = tempdir().unwrap();
        let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home(fixture.path())
            .with_temp_dir(fixture.path());
        record(&result_with_private_item_data(), &environment).unwrap();

        let contents = fs::read_to_string(history_path(&environment)).unwrap();
        assert!(contents.contains("\"reclaimed_bytes\":42"));
        assert!(contents.contains("\"target_count\":1"));
        assert!(!contents.contains("private-item-id"));
        assert!(!contents.contains("Private cache name"));
        assert!(!contents.contains("/Users/example"));
    }

    #[test]
    fn history_keeps_only_the_latest_bounded_window() {
        let fixture = tempdir().unwrap();
        let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home(fixture.path())
            .with_temp_dir(fixture.path());
        for _ in 0..(MAX_HISTORY_ENTRIES + 5) {
            record(&result_with_private_item_data(), &environment).unwrap();
        }

        let contents = fs::read_to_string(history_path(&environment)).unwrap();
        assert_eq!(contents.lines().count(), MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn list_returns_newest_first_and_skips_unreadable_lines() {
        let fixture = tempdir().unwrap();
        let environment = PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_home(fixture.path())
            .with_temp_dir(fixture.path());
        assert!(list(&environment).unwrap().is_empty());

        let mut older = result_with_private_item_data();
        older.finished_at = 100;
        record(&older, &environment).unwrap();
        let mut newer = result_with_private_item_data();
        newer.finished_at = 200;
        newer.failed_count = 2;
        record(&newer, &environment).unwrap();

        let path = history_path(&environment);
        let mut contents = fs::read_to_string(&path).unwrap();
        contents.push_str("{not json\n");
        fs::write(&path, contents).unwrap();

        let entries = list(&environment).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].finished_at, 200);
        assert_eq!(entries[0].failed_count, 2);
        assert_eq!(entries[1].finished_at, 100);
        assert_eq!(entries[1].removed_bytes, 42);
        assert_eq!(entries[1].item_count, 1);
    }
}
