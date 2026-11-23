//! `neati scan` and `neati projects`: read-only views of what Clean sees.
//!
//! Both are a preview of the one-button use case, restricted to one source,
//! rendered with the inventory the set was chosen from. They never execute.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;

use crate::models::CleanupEligibility;
use crate::services::one_button_clean::ports::{
    CleanPreview, CleanRequest, CleanUseCase, ProjectInventory,
};
use crate::services::one_button_clean::selection::{category_label, kind_label, project_reference};

use super::clean::{plural, RefusalJson};
use super::output::{display_path, format_bytes, print_json, Progress};

pub struct Inventory<'a> {
    pub use_case: &'a dyn CleanUseCase,
    pub progress: Arc<Progress>,
    pub home: Option<&'a std::path::Path>,
    pub json: bool,
}

impl Inventory<'_> {
    fn preview(&self, caches: bool) -> Result<CleanPreview, String> {
        self.preview_parts(CleanRequest {
            caches,
            projects: !caches,
            worktrees: false,
            ..CleanRequest::default()
        })
    }

    fn preview_parts(&self, request: CleanRequest) -> Result<CleanPreview, String> {
        let preview =
            tauri::async_runtime::block_on(self.use_case.preview(request, self.progress.clone()));
        self.progress.clear();
        preview.map_err(|error| error.to_string())
    }

    pub fn caches(&self, all: bool) -> Result<i32, String> {
        let preview = self.preview(true)?;
        let Some(scan) = &preview.caches else {
            return Err("The cache scan did not run.".to_string());
        };
        let ready: HashSet<&str> = preview
            .chosen
            .iter()
            .map(|candidate| candidate.reference.as_str())
            .collect();
        let mut items: Vec<ScanEntry> = scan
            .categories
            .iter()
            .flat_map(|category| category.items.iter())
            .filter(|item| all || ready.contains(item.id.as_str()))
            .map(|item| ScanEntry {
                reference: item.id.clone(),
                name: item.name.clone(),
                category: category_label(item.category),
                path: item.path.clone(),
                observed_bytes: item.observed_bytes(),
                cleanable_bytes: item.cleanable_bytes(),
                eligibility: item.disposition.eligibility,
                ready: ready.contains(item.id.as_str()),
                reason: item.disposition.reason.clone(),
            })
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.observed_bytes));
        let report = ScanJson {
            scan_id: scan.scan_id.clone(),
            observed_bytes: scan.total_bytes,
            ready_bytes: preview.total_bytes,
            ready_items: preview.chosen.len(),
            reviewable_bytes: preview.review.bytes,
            reviewable_items: preview.review.items,
            complete: scan.incomplete_reasons.is_empty(),
            incomplete_reasons: scan.incomplete_reasons.clone(),
            items,
        };
        if self.json {
            print_json("scan", report)?;
        } else {
            print_caches(&report, all, self.home);
        }
        Ok(0)
    }

    pub fn projects(&self, all: bool) -> Result<i32, String> {
        let preview = self.preview(false)?;
        let inventory = preview.projects.clone().unwrap_or_default();
        let ready: HashSet<&str> = preview
            .chosen
            .iter()
            .map(|candidate| candidate.reference.as_str())
            .collect();
        let mut items: Vec<ProjectEntry> = inventory
            .result
            .iter()
            .flat_map(|result| result.items.iter())
            .map(|item| {
                let reference = project_reference(&item.path);
                ProjectEntry {
                    ready: ready.contains(reference.as_str()),
                    reference,
                    project: item.project_name.clone(),
                    kind: kind_label(&item.kind),
                    path: item.path.clone(),
                    bytes: item.allocated_bytes,
                    clean_inclusion: item.clean_inclusion,
                    project_last_activity: item.project_last_activity,
                    status: item.status,
                    rebuild_hint: item.rebuild_hint.clone(),
                }
            })
            .filter(|entry| all || entry.ready)
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
        let report = ProjectsJson {
            folders: &inventory.folders,
            refused: inventory.refused.iter().map(RefusalJson::from).collect(),
            ready_bytes: preview.total_bytes,
            ready_items: preview.chosen.len(),
            items,
        };
        if self.json {
            print_json("projects", report)?;
        } else {
            print_projects(&report, &inventory, all, self.home);
        }
        Ok(0)
    }
}

#[derive(Serialize)]
struct ScanEntry {
    #[serde(rename = "ref")]
    reference: String,
    name: String,
    category: &'static str,
    path: String,
    observed_bytes: u64,
    cleanable_bytes: u64,
    eligibility: CleanupEligibility,
    /// In the one-button Clean set.
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize)]
struct ScanJson {
    scan_id: String,
    observed_bytes: u64,
    ready_bytes: u64,
    ready_items: usize,
    reviewable_bytes: u64,
    reviewable_items: u64,
    complete: bool,
    incomplete_reasons: Vec<String>,
    items: Vec<ScanEntry>,
}

#[derive(Serialize)]
struct ProjectEntry {
    #[serde(rename = "ref")]
    reference: String,
    project: String,
    kind: String,
    path: String,
    bytes: u64,
    /// In the one-button Clean set: what the person's cleanup rules include
    /// and an exact check proved idle (#494, #504).
    ready: bool,
    /// Why it is in the set or left for review.
    clean_inclusion: crate::models::CleanInclusion,
    /// When its project was last worked on (seconds since the epoch), when a
    /// rule needed to know.
    #[serde(skip_serializing_if = "Option::is_none")]
    project_last_activity: Option<u64>,
    status: crate::models::DeveloperArtifactStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    rebuild_hint: Option<String>,
}

#[derive(Serialize)]
struct ProjectsJson<'a> {
    folders: &'a [String],
    refused: Vec<RefusalJson<'a>>,
    ready_bytes: u64,
    ready_items: usize,
    items: Vec<ProjectEntry>,
}

fn print_caches(report: &ScanJson, all: bool, home: Option<&std::path::Path>) {
    println!(
        "Caches: {} found, {} ready to clean in {} {}.",
        format_bytes(report.observed_bytes),
        format_bytes(report.ready_bytes),
        report.ready_items,
        plural(report.ready_items as u64, "item", "items")
    );
    if report.reviewable_items > 0 {
        println!(
            "{} more ({}) need review in the app.",
            report.reviewable_items,
            format_bytes(report.reviewable_bytes)
        );
    }
    if !report.complete {
        println!("Partial scan: some locations could not be fully inspected.");
        for reason in &report.incomplete_reasons {
            println!("  · {reason}");
        }
    }
    if !report.items.is_empty() {
        println!();
    }
    for item in &report.items {
        println!(
            "  {:>9}  {:<10} {} · {}",
            format_bytes(item.observed_bytes),
            if item.ready {
                "ready"
            } else {
                eligibility_label(item.eligibility)
            },
            item.name,
            item.category
        );
        println!(
            "                        {}  [{}]",
            display_path(&item.path, home),
            item.reference
        );
        if all && !item.ready {
            if let Some(reason) = &item.reason {
                println!("                        {reason}");
            }
        }
    }
    if !all {
        println!(
            "\n`neati scan --all` lists every location found, with the reason it is not ready."
        );
    }
}

fn print_projects(
    report: &ProjectsJson,
    inventory: &ProjectInventory,
    all: bool,
    home: Option<&std::path::Path>,
) {
    if report.folders.is_empty() {
        println!(
            "No project folder is saved. Add one in neati (Clean → Projects), or pass --folder ~/code."
        );
        return;
    }
    println!(
        "Project folders: {}",
        report
            .folders
            .iter()
            .map(|folder| display_path(folder, home))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for refusal in &inventory.refused {
        println!(
            "  not scanned: {} — {}",
            display_path(&refusal.path, home),
            refusal.reason
        );
    }
    println!(
        "Ready to clean under your rules: {} in {} {}.\n",
        format_bytes(report.ready_bytes),
        report.ready_items,
        plural(report.ready_items as u64, "folder", "folders")
    );
    for item in &report.items {
        println!(
            "  {:>9}  {:<8} {} · {}",
            format_bytes(item.bytes),
            if item.ready { "ready" } else { "review" },
            item.project,
            item.kind
        );
        println!(
            "                      {}  [{}]",
            display_path(&item.path, home),
            item.reference
        );
        if all && !item.ready {
            println!(
                "                      {}",
                inclusion_reason(item.clean_inclusion)
            );
        }
    }
    if !all {
        println!("\n`neati projects --all` also lists dependency folders and other output that needs review in the app, with the reason.");
    }
}

fn eligibility_label(eligibility: CleanupEligibility) -> &'static str {
    match eligibility {
        CleanupEligibility::AutoCleanable => "ready",
        CleanupEligibility::Reviewable => "review",
        CleanupEligibility::Recent => "recent",
        CleanupEligibility::PolicyGated => "off",
        CleanupEligibility::Advisory => "advisory",
        CleanupEligibility::Blocked => "protected",
    }
}

/// Why a project item is or is not in the one-button set, for the terminal.
fn inclusion_reason(inclusion: crate::models::CleanInclusion) -> &'static str {
    use crate::models::CleanInclusion as Inclusion;
    match inclusion {
        Inclusion::Included => "Included by your cleanup rules.",
        Inclusion::NotOffered => "Always reviewed in the app.",
        Inclusion::NotVerified => "Its size or generated-only ownership was not fully verified.",
        Inclusion::RuleOff => "Dependency folders of inactive projects are off in Settings.",
        Inclusion::NoLockfile => "No lockfile beside it, so a reinstall might not reproduce it.",
        Inclusion::RecentlyActive => "The project was worked on within your rule's period.",
        Inclusion::ActivityUnknown => "When the project was last worked on could not be observed.",
        Inclusion::InUse => "A running process has files open in it.",
        Inclusion::UseUnknown => "neati could not confirm that nothing is using it.",
    }
}

#[derive(Serialize)]
struct WorktreesJson<'a> {
    repositories: u32,
    idle_days: u16,
    /// Whether finished worktrees are in `neati clean`.
    in_clean: bool,
    refused: &'a [String],
    items: &'a [crate::models::WorktreeItem],
}

impl Inventory<'_> {
    /// Linked worktrees and their state. Removal stays in the app or in
    /// `neati clean` when the person's worktree rule is on.
    pub fn worktrees(&self) -> Result<i32, String> {
        let preview = self.preview_parts(CleanRequest {
            caches: false,
            projects: false,
            worktrees: true,
            worktree_inventory: true,
            ..CleanRequest::default()
        })?;
        let inventory = preview
            .worktrees
            .ok_or_else(|| "The worktree check did not run.".to_string())?;
        if self.json {
            print_json(
                "worktrees",
                WorktreesJson {
                    repositories: inventory.repositories,
                    idle_days: inventory.idle_days,
                    in_clean: inventory.rule_enabled,
                    refused: &inventory.refused,
                    items: &inventory.items,
                },
            )?;
            return Ok(0);
        }
        if inventory.repositories == 0 {
            println!(
                "No repository found in the saved project folders. Add one in neati (Clean → Projects), or pass --folder ~/code."
            );
            return Ok(0);
        }
        let finished: Vec<_> = inventory
            .items
            .iter()
            .filter(|item| item.is_removable())
            .collect();
        println!(
            "{} {} in {} {}; {} finished ({}).",
            inventory.items.len(),
            plural(inventory.items.len() as u64, "worktree", "worktrees"),
            inventory.repositories,
            plural(
                u64::from(inventory.repositories),
                "repository",
                "repositories"
            ),
            finished.len(),
            format_bytes(finished.iter().map(|item| item.allocated_bytes).sum()),
        );
        for reason in &inventory.refused {
            println!("  not read: {reason}");
        }
        if !inventory.items.is_empty() {
            println!();
        }
        for item in &inventory.items {
            println!(
                "  {:>9}  {:<17} {} · {}",
                format_bytes(item.allocated_bytes),
                worktree_state_label(item.state),
                item.name,
                item.branch.as_deref().unwrap_or("detached"),
            );
            println!(
                "                               {}  [{}]",
                display_path(&item.path, self.home),
                item.id
            );
        }
        println!(
            "\nFinished means no uncommitted or untracked change, a reachable commit, nothing using it, and no work for {} days.",
            inventory.idle_days
        );
        if inventory.rule_enabled {
            println!("Your worktree rule is on: `neati clean` includes finished worktrees.");
        } else {
            println!("Remove finished worktrees in the app (Clean → Projects), or turn on the worktree rule in Settings.");
        }
        Ok(0)
    }
}

fn worktree_state_label(state: crate::models::WorktreeState) -> &'static str {
    use crate::models::WorktreeState as State;
    match state {
        State::Finished => "finished",
        State::RecentlyActive => "recently active",
        State::ActivityUnknown => "activity unknown",
        State::HasChanges => "has changes",
        State::HasSubmodules => "has submodules",
        State::Locked => "locked",
        State::CommitsOnlyHere => "commits only here",
        State::InUse => "in use",
        State::UseUnknown => "use unknown",
        State::Missing => "missing",
        State::Refused => "not checked",
    }
}
