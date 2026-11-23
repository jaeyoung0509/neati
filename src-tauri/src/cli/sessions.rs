//! `neati sessions`: the agent session inventory (#508).
//!
//! The listing is read-only. Titles are the person's own prompts, printed to
//! their terminal and never logged. No command changes or deletes a session
//! file.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::agent_sessions::SessionCancel;
use crate::models::{AgentSessionInventory, AgentSessionSummary, ProjectFolderState};
use crate::services::agent_sessions::ports::SessionLibrary;

use super::output::{display_path, format_bytes, print_json, Progress};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_print_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00Z");
        assert_eq!(utc(1_795_515_072), "2026-11-24 10:11Z");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00Z");
    }
}
