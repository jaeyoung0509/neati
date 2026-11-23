//! `neati sessions`: the agent session inventory and bundles (#508, #509).
//!
//! The listing is read-only. `export` writes one bundle file that must not
//! exist and refuses when likely secrets were found unless `--allow-secrets`
//! is given; the findings are printed as counts per kind, never values.
//! `import` verifies the bundle first and never replaces a file. No command
//! deletes a session file.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::agent_sessions::SessionCancel;
use crate::models::{
    AgentSessionInventory, AgentSessionSummary, ProjectFolderState, SecretFinding, SessionRemap,
};
use crate::services::agent_sessions::ports::{SessionLibrary, SessionSelection};

use super::args::{SessionExportOptions, SessionImportOptions};
use super::output::{display_path, format_bytes, print_json, Progress};

/// A path the person typed, made absolute against the working folder.
pub fn absolute(path: &str, working_directory: Option<&Path>) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    working_directory
        .map(|folder| folder.join(path))
        .ok_or_else(|| {
            format!(
                "{} is relative and the working folder is unknown.",
                path.display()
            )
        })
}

pub fn list(
    library: &dyn SessionLibrary,
    progress: Arc<Progress>,
    home: Option<&Path>,
    json: bool,
) -> Result<i32, String> {
    progress.update("Reading agent sessions…");
    let inventory = tauri::async_runtime::block_on(library.inventory(SessionCancel::new()));
    progress.clear();
    let inventory = inventory?;
    if json {
        print_json(
            "sessions",
            Inventory {
                inventory: &inventory,
            },
        )?;
        return Ok(0);
    }
    print_inventory(&inventory, home);
    Ok(0)
}

#[derive(Serialize)]
struct Inventory<'a> {
    #[serde(flatten)]
    inventory: &'a AgentSessionInventory,
}

/// `2026-11-24 10:11Z` from Unix seconds.
fn utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil date from days since 1970-01-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60
    )
}

fn print_inventory(inventory: &AgentSessionInventory, home: Option<&Path>) {
    let totals: Vec<String> = inventory
        .tools
        .iter()
        .filter(|tool| tool.present)
        .map(|tool| {
            format!(
                "{} {} {} · {}",
                tool.tool.label(),
                tool.sessions,
                if tool.sessions == 1 {
                    "session"
                } else {
                    "sessions"
                },
                format_bytes(tool.bytes)
            )
        })
        .collect();
    if totals.is_empty() {
        println!("No Claude Code or Codex session folders were found.");
        return;
    }
    println!("{}", totals.join("   "));
    for project in &inventory.projects {
        let location = project
            .cwd
            .as_deref()
            .map(|cwd| display_path(cwd, home))
            .unwrap_or_else(|| project.display_name.clone());
        let state = match project.folder {
            ProjectFolderState::Missing => "  (folder gone)",
            ProjectFolderState::Present | ProjectFolderState::Unknown => "",
        };
        println!(
            "\n{location}{state} · {} · {}",
            project.sessions.len(),
            format_bytes(project.total_bytes)
        );
        for session in &project.sessions {
            print_session(session);
        }
    }
    for reason in &inventory.incomplete_reasons {
        println!("  · {reason}");
    }
    println!("\nExport with `neati sessions export --out FILE --only KEY,…`.");
}

fn print_session(session: &AgentSessionSummary) {
    let when = session
        .last_activity_at
        .map(utc)
        .unwrap_or_else(|| "unknown date     ".to_string());
    println!(
        "  {:<6} {}  {:>9}  {}  {}",
        session.tool.slug(),
        when,
        format_bytes(session.size_bytes),
        session.key,
        session.title.as_deref().unwrap_or("(no prompt found)")
    );
}

#[derive(Serialize)]
struct ExportResult<'a> {
    action: &'static str,
    /// The bundle was not written because likely secrets were found.
    refused: bool,
    path: Option<&'a str>,
    sessions: u32,
    files: u32,
    bytes: u64,
    archive_bytes: Option<u64>,
    findings: &'a [SecretFinding],
}

fn print_findings(findings: &[SecretFinding]) {
    for finding in findings {
        println!("  {:>4} × {}", finding.count, finding.kind.label());
    }
}

pub fn export(
    library: &dyn SessionLibrary,
    progress: Arc<Progress>,
    options: &SessionExportOptions,
    destination: PathBuf,
    json: bool,
) -> Result<i32, String> {
    let selection = if options.only.is_empty() {
        SessionSelection::All
    } else {
        SessionSelection::Only(options.only.clone())
    };
    progress.update("Checking sessions for likely secrets…");
    let prepared =
        tauri::async_runtime::block_on(library.prepare_export(selection, SessionCancel::new()));
    progress.clear();
    let prepared = prepared?;
    let sessions = u32::try_from(prepared.sessions.len()).unwrap_or(u32::MAX);
    if !prepared.findings.is_empty() && !options.allow_secrets {
        if json {
            print_json(
                "sessions",
                ExportResult {
                    action: "export",
                    refused: true,
                    path: None,
                    sessions,
                    files: prepared.files,
                    bytes: prepared.bytes,
                    archive_bytes: None,
                    findings: &prepared.findings,
                },
            )?;
        } else {
            println!("Likely secrets were found in the chosen sessions:");
            print_findings(&prepared.findings);
            println!(
                "Nothing was written. Remove them, choose other sessions, or pass --allow-secrets."
            );
        }
        return Ok(1);
    }
    progress.update("Writing the bundle…");
    let findings = prepared.findings.clone();
    let outcome = tauri::async_runtime::block_on(library.export(
        prepared,
        destination,
        options.allow_secrets,
        SessionCancel::new(),
    ));
    progress.clear();
    let outcome = outcome?;
    if json {
        print_json(
            "sessions",
            ExportResult {
                action: "export",
                refused: false,
                path: Some(&outcome.path),
                sessions: outcome.sessions,
                files: outcome.files,
                bytes: outcome.bytes,
                archive_bytes: Some(outcome.archive_bytes),
                findings: &outcome.findings,
            },
        )?;
        return Ok(0);
    }
    println!(
        "Wrote {} {} ({} files, {}) to {} · {}",
        outcome.sessions,
        if outcome.sessions == 1 {
            "session"
        } else {
            "sessions"
        },
        outcome.files,
        format_bytes(outcome.bytes),
        outcome.path,
        format_bytes(outcome.archive_bytes)
    );
    if !findings.is_empty() {
        println!("It includes likely secrets, as allowed:");
        print_findings(&outcome.findings);
    }
    Ok(0)
}

#[derive(Serialize)]
struct ImportResult<'a> {
    action: &'static str,
    bundle: String,
    neati_version: &'a str,
    sessions: u32,
    written_files: u32,
    kept_existing: &'a [String],
    rewritten_files: u32,
}

pub fn import(
    library: &dyn SessionLibrary,
    progress: Arc<Progress>,
    options: &SessionImportOptions,
    bundle: PathBuf,
    working_directory: Option<&Path>,
    json: bool,
) -> Result<i32, String> {
    let mut remaps = Vec::new();
    for (old, new) in &options.remaps {
        if !Path::new(old).is_absolute() {
            return Err(format!(
                "--remap {old}: the old folder must be an absolute path."
            ));
        }
        remaps.push(SessionRemap {
            from: old.clone(),
            to: absolute(new, working_directory)?
                .to_string_lossy()
                .into_owned(),
        });
    }
    progress.update("Verifying the bundle…");
    let review = tauri::async_runtime::block_on(library.open_bundle(bundle, SessionCancel::new()));
    progress.clear();
    let review = review?;
    let name = review.bundle.name();
    let version = review.bundle.manifest.neati_version.clone();
    let missing: Vec<String> = review
        .projects
        .iter()
        .filter(|project| project.folder == ProjectFolderState::Missing)
        .map(|project| project.original_cwd.clone())
        .collect();
    progress.update("Importing sessions…");
    let outcome = tauri::async_runtime::block_on(library.import(
        review,
        remaps.clone(),
        SessionCancel::new(),
    ));
    progress.clear();
    let outcome = outcome?;
    if json {
        print_json(
            "sessions",
            ImportResult {
                action: "import",
                bundle: name,
                neati_version: &version,
                sessions: outcome.sessions,
                written_files: outcome.written_files,
                kept_existing: &outcome.kept_existing,
                rewritten_files: outcome.rewritten_files,
            },
        )?;
        return Ok(0);
    }
    println!(
        "Verified {name} (neati {version}). Wrote {} files for {} {}.",
        outcome.written_files,
        outcome.sessions,
        if outcome.sessions == 1 {
            "session"
        } else {
            "sessions"
        }
    );
    if outcome.rewritten_files > 0 {
        println!(
            "Rewrote the project folder in {} files.",
            outcome.rewritten_files
        );
    }
    if !outcome.kept_existing.is_empty() {
        println!(
            "Kept {} existing files; neati never replaces one:",
            outcome.kept_existing.len()
        );
        for path in outcome.kept_existing.iter().take(20) {
            println!("  {path}");
        }
    }
    let unmapped: Vec<&String> = missing
        .iter()
        .filter(|cwd| {
            !remaps
                .iter()
                .any(|remap| remap.from.trim_end_matches('/') == cwd.as_str())
        })
        .collect();
    for cwd in unmapped {
        println!(
            "Note: {cwd} is not on this Mac; its sessions keep that folder. \
             --remap {cwd}=NEW moves sessions on import."
        );
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_print_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00Z");
        assert_eq!(utc(1_795_515_072), "2026-11-24 10:11Z");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00Z");
    }

    #[test]
    fn relative_paths_resolve_against_the_working_folder() {
        let folder = Path::new("/work");
        assert_eq!(
            absolute("b.zip", Some(folder)).unwrap(),
            PathBuf::from("/work/b.zip")
        );
        assert_eq!(
            absolute("/x/b.zip", None).unwrap(),
            PathBuf::from("/x/b.zip")
        );
        assert!(absolute("b.zip", None).is_err());
    }
}
