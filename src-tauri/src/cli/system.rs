//! `neati status`, `neati ports` and `neati history`: read-only views.

use serde::Serialize;

use crate::models::{CleanupHistoryEntry, DevelopmentListener, DiskMetrics, MemoryPressure};
use crate::services::insights::InsightQueries;

use super::output::{display_path, format_bytes, print_json};

#[derive(Serialize)]
struct StatusReport {
    disk: DiskMetrics,
    memory: MemorySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_cleanup: Option<CleanupHistoryEntry>,
}

#[derive(Serialize)]
struct MemorySummary {
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
    pressure: MemoryPressure,
    swap_used_bytes: u64,
}

pub fn status(insights: &dyn InsightQueries, json: bool) -> Result<i32, String> {
    let status = tauri::async_runtime::block_on(insights.status())?;
    let memory = status.memory;
    let report = StatusReport {
        memory: MemorySummary {
            total_bytes: memory.total_bytes,
            used_bytes: memory.used_bytes,
            available_bytes: memory.available_bytes,
            pressure: memory.pressure,
            swap_used_bytes: memory.swap_used_bytes,
        },
        last_cleanup: status.last_cleanup,
        disk: status.disk,
    };
    if json {
        print_json("status", report)?;
        return Ok(0);
    }
    println!(
        "Disk    {} free of {} ({:.0}% used)",
        format_bytes(report.disk.available_bytes),
        format_bytes(report.disk.total_bytes),
        report.disk.percent_used
    );
    println!(
        "Memory  {} available of {} · pressure {}",
        format_bytes(report.memory.available_bytes),
        format_bytes(report.memory.total_bytes),
        pressure_label(report.memory.pressure)
    );
    if report.memory.swap_used_bytes > 0 {
        println!(
            "        swap {}",
            format_bytes(report.memory.swap_used_bytes)
        );
    }
    match &report.last_cleanup {
        Some(entry) => println!(
            "Clean   last run removed {} in {} {} ({})",
            format_bytes(entry.removed_bytes),
            entry.item_count,
            if entry.item_count == 1 {
                "item"
            } else {
                "items"
            },
            ago(entry.finished_at)
        ),
        None => println!("Clean   no cleanup yet · `neati clean` shows what is ready"),
    }
    Ok(0)
}

#[derive(Serialize)]
struct PortsReport {
    listeners: Vec<DevelopmentListener>,
}

pub fn ports(
    insights: &dyn InsightQueries,
    home: Option<&std::path::Path>,
    json: bool,
) -> Result<i32, String> {
    let listeners = tauri::async_runtime::block_on(insights.listeners())?;
    if json {
        print_json("ports", PortsReport { listeners })?;
        return Ok(0);
    }
    if listeners.is_empty() {
        println!("No development servers are listening.");
        return Ok(0);
    }
    for listener in &listeners {
        let project = listener
            .project_name
            .clone()
            .or_else(|| {
                listener
                    .working_directory
                    .as_deref()
                    .map(|directory| display_path(directory, home))
            })
            .unwrap_or_default();
        println!(
            "  :{:<6} {:<18} pid {:<7} {}",
            listener.port, listener.server_name, listener.pid, project
        );
    }
    println!("\nStop a server from neati (System → Ports); the CLI only lists them.");
    Ok(0)
}

#[derive(Serialize)]
struct HistoryReport {
    runs: Vec<CleanupHistoryEntry>,
}

pub fn history(insights: &dyn InsightQueries, json: bool) -> Result<i32, String> {
    let runs = tauri::async_runtime::block_on(insights.history())?;
    if json {
        print_json("history", HistoryReport { runs })?;
        return Ok(0);
    }
    if runs.is_empty() {
        println!("No cleanup has run yet.");
        return Ok(0);
    }
    for run in &runs {
        let mut line = format!(
            "  {:<14} {:>9} in {} {}",
            ago(run.finished_at),
            format_bytes(run.removed_bytes),
            run.item_count,
            if run.item_count == 1 { "item" } else { "items" }
        );
        if run.failed_count > 0 {
            line.push_str(&format!(" · {} not removed", run.failed_count));
        }
        println!("{line}");
    }
    Ok(0)
}

fn pressure_label(pressure: MemoryPressure) -> &'static str {
    match pressure {
        MemoryPressure::Normal => "normal",
        MemoryPressure::Warning => "elevated",
        MemoryPressure::Critical => "critical",
    }
}

fn ago(finished_at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(finished_at);
    let seconds = now.saturating_sub(finished_at);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}
