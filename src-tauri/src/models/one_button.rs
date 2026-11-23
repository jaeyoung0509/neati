//! The window's projection of the one-button Clean use case (#500).
//!
//! The use case's own types carry backend ids and whole inventories; the
//! interface receives only what it shows and the opaque preview id it may
//! submit back. Every `u64` crosses IPC through the shared numeric adapter.

use serde::{Deserialize, Serialize};

use crate::services::one_button_clean::ports::{CleanOutcome, CleanPreview};
use crate::services::one_button_clean::selection::Source;
use crate::services::ports::Activity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OneButtonSource {
    Cache,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OneButtonItem {
    /// A display key; the window never submits it back.
    pub reference: String,
    pub source: OneButtonSource,
    pub name: String,
    pub kind: String,
    pub path: String,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OneButtonPreview {
    /// The one-shot id `execute_one_button_clean` accepts.
    pub id: uuid::Uuid,
    pub items: Vec<OneButtonItem>,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub total_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub cache_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub project_bytes: u64,
    /// Found but left for review in the main window.
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub review_items: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub review_bytes: u64,
    /// Saved project folders, so the interface can say build output was not
    /// checked rather than that there is none.
    pub project_folders: u32,
}

impl OneButtonPreview {
    pub fn project(id: uuid::Uuid, preview: &CleanPreview) -> Self {
        let bytes_of = |source: Source| -> u64 {
            preview
                .chosen
                .iter()
                .filter(|candidate| candidate.source == source)
                .map(|candidate| candidate.bytes)
                .sum()
        };
        Self {
            id,
            items: preview
                .chosen
                .iter()
                .map(|candidate| OneButtonItem {
                    reference: candidate.reference.clone(),
                    source: match candidate.source {
                        Source::Cache => OneButtonSource::Cache,
                        Source::Project => OneButtonSource::Project,
                    },
                    name: candidate.name.clone(),
                    kind: candidate.kind.clone(),
                    path: candidate.path.clone(),
                    bytes: candidate.bytes,
                })
                .collect(),
            total_bytes: preview.total_bytes,
            cache_bytes: bytes_of(Source::Cache),
            project_bytes: bytes_of(Source::Project),
            review_items: preview.review.items,
            review_bytes: preview.review.bytes,
            project_folders: preview
                .projects
                .as_ref()
                .map(|inventory| u32::try_from(inventory.folders.len()).unwrap_or(u32::MAX))
                .unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OneButtonFailure {
    pub reference: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OneButtonOutcome {
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub reclaimed_bytes: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub removed: u64,
    #[serde(with = "crate::ipc_numeric::u64")]
    #[specta(type = u64)]
    pub skipped: u64,
    pub failed: Vec<OneButtonFailure>,
}

impl From<CleanOutcome> for OneButtonOutcome {
    fn from(outcome: CleanOutcome) -> Self {
        Self {
            reclaimed_bytes: outcome.reclaimed_bytes,
            removed: outcome.removed,
            skipped: outcome.skipped,
            failed: outcome
                .failed
                .into_iter()
                .map(|failure| OneButtonFailure {
                    reference: failure.reference,
                    name: failure.name,
                    message: failure.message,
                })
                .collect(),
        }
    }
}

/// What the use case is doing, streamed to the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OneButtonActivity {
    Scanning {
        location: String,
    },
    Measuring {
        project: String,
        kind: String,
    },
    Removing {
        name: String,
        index: u32,
        total: u32,
    },
    Checking {
        source: String,
    },
}

impl From<Activity> for OneButtonActivity {
    fn from(activity: Activity) -> Self {
        let small = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        match activity {
            Activity::Scanning { location } => Self::Scanning { location },
            Activity::Measuring { project, kind } => Self::Measuring { project, kind },
            Activity::Removing { name, index, total } => Self::Removing {
                name,
                index: small(index),
                total: small(total),
            },
            Activity::Checking { source } => Self::Checking { source },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::one_button_clean::ports::{ReviewSummary, UnitFailure};
    use crate::services::one_button_clean::selection::Candidate;

    fn candidate(source: Source, bytes: u64) -> Candidate {
        Candidate {
            reference: format!("{source:?}"),
            source,
            name: "unit".to_string(),
            kind: "kind".to_string(),
            path: "/Users/me/unit".to_string(),
            bytes,
            backend_id: "backend-only".to_string(),
        }
    }

    #[test]
    fn a_preview_projects_totals_by_source_and_hides_backend_ids() {
        let preview = CleanPreview {
            chosen: vec![candidate(Source::Cache, 10), candidate(Source::Project, 32)],
            total_bytes: 42,
            review: ReviewSummary { items: 1, bytes: 7 },
            caches: None,
            projects: None,
        };
        let projected = OneButtonPreview::project(uuid::Uuid::nil(), &preview);
        assert_eq!((projected.cache_bytes, projected.project_bytes), (10, 32));
        assert_eq!(projected.total_bytes, 42);
        assert_eq!(projected.project_folders, 0);
        let json = serde_json::to_string(&projected).expect("serializes");
        assert!(
            !json.contains("backend-only"),
            "backend ids never reach the window"
        );
    }

    #[test]
    fn integers_beyond_javascript_precision_are_refused() {
        let outcome = OneButtonOutcome::from(CleanOutcome {
            reclaimed_bytes: crate::ipc_numeric::MAX_SAFE_INTEGER + 1,
            removed: 1,
            skipped: 0,
            failed: vec![],
        });
        assert!(serde_json::to_string(&outcome).is_err());
        let safe = OneButtonOutcome::from(CleanOutcome {
            reclaimed_bytes: crate::ipc_numeric::MAX_SAFE_INTEGER,
            removed: 1,
            skipped: 0,
            failed: vec![UnitFailure {
                reference: "r".to_string(),
                name: "n".to_string(),
                message: "m".to_string(),
            }],
        });
        let json = serde_json::to_value(&safe).expect("safe integers serialize");
        assert_eq!(
            json["reclaimed_bytes"],
            crate::ipc_numeric::MAX_SAFE_INTEGER
        );
        assert_eq!(json["failed"][0]["reference"], "r");
    }

    #[test]
    fn activity_is_tagged_for_the_window() {
        let json = serde_json::to_value(OneButtonActivity::from(Activity::Removing {
            name: "npm cache".to_string(),
            index: 2,
            total: 5,
        }))
        .expect("serializes");
        assert_eq!(json["type"], "removing");
        assert_eq!(json["index"], 2);
    }
}
