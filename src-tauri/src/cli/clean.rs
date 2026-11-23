//! `neati clean`: the terminal adapter for the one-button Clean use case.
//!
//! This module decides only what an interface decides: whether this run is
//! approved (`--yes`, or a person answering the prompt), and how the preview
//! and the outcome look as text or `neati.cli/1` JSON. Membership, narrowing
//! and removal belong to `CleanUseCase`.

use std::io::{BufRead, IsTerminal, Write};
use std::sync::Arc;

use serde::Serialize;

use crate::services::one_button_clean::ports::{
    CleanOutcome, CleanPreview, CleanRequest, CleanUseCase,
};
use crate::services::one_button_clean::selection::{Candidate, Source};

use super::args::CleanOptions;
use super::output::{display_path, format_bytes, print_json, Progress};

/// How this run may be approved, beyond `--yes`.
pub trait Approval {
    /// Whether a person can answer a prompt right now.
    fn can_ask(&self) -> bool;
    /// Asks that person; `true` only for an explicit yes.
    fn ask(&self, count: usize, bytes: u64) -> Result<bool, String>;
}

/// A prompt on the controlling terminal: only when stdin and stderr are TTYs.
pub struct TerminalApproval;

impl Approval for TerminalApproval {
    fn can_ask(&self) -> bool {
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }

    fn ask(&self, count: usize, bytes: u64) -> Result<bool, String> {
        let mut stderr = std::io::stderr();
        let _ = write!(
            stderr,
            "\nPermanently remove {count} {} ({})? This cannot be undone. [y/N] ",
            plural(count as u64, "item", "items"),
            format_bytes(bytes)
        );
        let _ = stderr.flush();
        let mut answer = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut answer)
            .map_err(|error| error.to_string())?;
        Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES"))
    }
}

pub struct CleanRun<'a> {
    pub use_case: &'a dyn CleanUseCase,
    pub approval: &'a dyn Approval,
    pub progress: Arc<Progress>,
    pub home: Option<&'a std::path::Path>,
    pub json: bool,
}

impl CleanRun<'_> {
    pub fn run(&self, options: &CleanOptions) -> Result<i32, String> {
        let request = CleanRequest {
            caches: options.caches,
            projects: options.projects,
            only: options.only.clone(),
            skip: options.skip.clone(),
            // A terminal run is its own process: there is nothing to reuse,
            // and the preview it shows is the one it executes.
            reuse_current: false,
        };
        let preview =
            tauri::async_runtime::block_on(self.use_case.preview(request, self.progress.clone()));
        self.progress.clear();
        let preview = preview.map_err(|error| error.to_string())?;

        // Only a person at a terminal is asked. An agent, a pipe or `--json`
        // without `--yes` always gets the preview and nothing else.
        let interactive = !options.yes && !options.dry_run && !self.json && self.approval.can_ask();
        if !self.json {
            print_plan(&preview, self.home);
        }
        let approved = options.yes
            || (interactive
                && !preview.chosen.is_empty()
                && self
                    .approval
                    .ask(preview.chosen.len(), preview.total_bytes)?);

        if !approved {
            if self.json {
                print_json("clean", CleanJson::new(&preview, None))?;
            } else if !preview.chosen.is_empty() {
                println!(
                    "\nNothing was removed.{}",
                    if interactive {
                        ""
                    } else {
                        " Run `neati clean --yes` to permanently remove these."
                    }
                );
            }
            return Ok(0);
        }

        let outcome =
            tauri::async_runtime::block_on(self.use_case.execute(&preview, self.progress.clone()));
        self.progress.clear();
        let outcome = outcome.map_err(|error| error.to_string())?;
        let exit_code = i32::from(!outcome.failed.is_empty());
        if self.json {
            print_json("clean", CleanJson::new(&preview, Some(&outcome)))?;
        } else {
            print_outcome(&outcome);
        }
        Ok(exit_code)
    }
}

// --- `neati.cli/1` JSON: owned by this adapter, not by the use case ---------

#[derive(Serialize)]
struct CleanJson<'a> {
    dry_run: bool,
    total_bytes: u64,
    items: Vec<CandidateJson<'a>>,
    /// Found but left for review in the app; the CLI never removes these.
    review_in_app: CountJson,
    project_folders: FoldersJson<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<OutcomeJson<'a>>,
}

impl<'a> CleanJson<'a> {
    fn new(preview: &'a CleanPreview, outcome: Option<&'a CleanOutcome>) -> Self {
        let projects = preview.projects.as_ref();
        Self {
            dry_run: outcome.is_none(),
            total_bytes: preview.total_bytes,
            items: preview.chosen.iter().map(CandidateJson::from).collect(),
            review_in_app: CountJson {
                items: preview.review.items,
                bytes: preview.review.bytes,
            },
            project_folders: FoldersJson {
                saved: projects
                    .map(|inventory| inventory.folders.as_slice())
                    .unwrap_or_default(),
                refused: projects
                    .map(|inventory| inventory.refused.iter().map(RefusalJson::from).collect())
                    .unwrap_or_default(),
            },
            result: outcome.map(OutcomeJson::from),
        }
    }
}

#[derive(Serialize)]
struct CandidateJson<'a> {
    #[serde(rename = "ref")]
    reference: &'a str,
    source: &'static str,
    name: &'a str,
    kind: &'a str,
    path: &'a str,
    bytes: u64,
}

impl<'a> From<&'a Candidate> for CandidateJson<'a> {
    fn from(candidate: &'a Candidate) -> Self {
        Self {
            reference: &candidate.reference,
            source: match candidate.source {
                Source::Cache => "cache",
                Source::Project => "project",
            },
            name: &candidate.name,
            kind: &candidate.kind,
            path: &candidate.path,
            bytes: candidate.bytes,
        }
    }
}

#[derive(Serialize)]
struct CountJson {
    items: u64,
    bytes: u64,
}

#[derive(Serialize)]
struct FoldersJson<'a> {
    saved: &'a [String],
    refused: Vec<RefusalJson<'a>>,
}

#[derive(Serialize)]
pub(super) struct RefusalJson<'a> {
    path: &'a str,
    reason: &'a str,
}

impl<'a> From<&'a crate::models::DeveloperWorkspaceRefusal> for RefusalJson<'a> {
    fn from(refusal: &'a crate::models::DeveloperWorkspaceRefusal) -> Self {
        Self {
            path: &refusal.path,
            reason: &refusal.reason,
        }
    }
}

#[derive(Serialize)]
struct OutcomeJson<'a> {
    reclaimed_bytes: u64,
    removed: u64,
    skipped: u64,
    failed: Vec<FailureJson<'a>>,
}

impl<'a> From<&'a CleanOutcome> for OutcomeJson<'a> {
    fn from(outcome: &'a CleanOutcome) -> Self {
        Self {
            reclaimed_bytes: outcome.reclaimed_bytes,
            removed: outcome.removed,
            skipped: outcome.skipped,
            failed: outcome
                .failed
                .iter()
                .map(|failure| FailureJson {
                    reference: &failure.reference,
                    name: &failure.name,
                    message: &failure.message,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct FailureJson<'a> {
    #[serde(rename = "ref")]
    reference: &'a str,
    name: &'a str,
    message: &'a str,
}

// --- Text ------------------------------------------------------------------

fn print_plan(preview: &CleanPreview, home: Option<&std::path::Path>) {
    if preview.chosen.is_empty() {
        println!("Nothing is ready to clean.");
    } else {
        println!(
            "Ready to clean: {} in {} {}",
            format_bytes(preview.total_bytes),
            preview.chosen.len(),
            plural(preview.chosen.len() as u64, "item", "items")
        );
        println!("These are permanently removed; they are rebuilt or re-downloaded when needed.\n");
        for candidate in &preview.chosen {
            println!(
                "  {:>9}  {} · {}",
                format_bytes(candidate.bytes),
                candidate.name,
                candidate.kind
            );
            println!(
                "             {}  [{}]",
                display_path(&candidate.path, home),
                candidate.reference
            );
        }
    }
    if preview.review.items > 0 {
        println!(
            "\n{} more {} ({}) need review in the app and are not removed here.",
            preview.review.items,
            plural(preview.review.items, "item", "items"),
            format_bytes(preview.review.bytes)
        );
    }
    if let Some(projects) = &preview.projects {
        if projects.folders.is_empty() {
            println!("\nProject build folders are not scanned: no project folder is saved. Add one in neati (Clean → Projects) or pass --folder ~/code.");
        }
        for refusal in &projects.refused {
            println!(
                "\nProject folder {} was not scanned: {}",
                display_path(&refusal.path, home),
                refusal.reason
            );
        }
    }
}

fn print_outcome(outcome: &CleanOutcome) {
    println!(
        "\nRemoved {} {}, {} reclaimed.",
        outcome.removed,
        plural(outcome.removed, "item", "items"),
        format_bytes(outcome.reclaimed_bytes)
    );
    if outcome.skipped > 0 {
        println!("{} already gone or changed, left alone.", outcome.skipped);
    }
    for failure in &outcome.failed {
        println!(
            "Not removed: {} [{}] — {}",
            failure.name, failure.reference, failure.message
        );
    }
}

pub(super) fn plural(count: u64, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 {
        one
    } else {
        many
    }
}
