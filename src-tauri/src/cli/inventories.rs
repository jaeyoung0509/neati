//! `neati large-files`, `neati apps`, `neati docker`: read-only views (#500).
//!
//! Removing a large file, uninstalling an app and pruning Docker data are
//! reviewed workflows with their own confirmations; they stay in the window.

use std::sync::Arc;

use serde::Serialize;

use crate::models::{
    DockerStatus, InstalledAppInventory, LargeFileFilter, LargeFileScanRequest, LargeFileScanResult,
};
use crate::services::insights::InsightQueries;

use super::args::LargeFileOptions;
use super::output::{format_bytes, print_json, Progress};

pub fn large_files(
    insights: &dyn InsightQueries,
    progress: Arc<Progress>,
    options: &LargeFileOptions,
    json: bool,
) -> Result<i32, String> {
    let request = LargeFileScanRequest {
        roots: options.roots.clone(),
        min_size_bytes: options.min_size_bytes,
        filter: if options.installers {
            LargeFileFilter::Installers
        } else {
            LargeFileFilter::All
        },
    };
    let result = tauri::async_runtime::block_on(insights.large_files(request, progress.clone()));
    progress.clear();
    let result = result?;
    if json {
        print_json("large-files", Wrapped { result: &result })?;
        return Ok(0);
    }
    print_large_files(&result);
    Ok(0)
}

#[derive(Serialize)]
struct Wrapped<'a, T: Serialize> {
    #[serde(flatten)]
    result: &'a T,
}

fn print_large_files(result: &LargeFileScanResult) {
    if result.items.is_empty() {
        println!("No large files found.");
        return;
    }
    let total: u64 = result.items.iter().map(|item| item.allocated_size).sum();
    println!(
        "{} large {} · {}",
        result.items.len(),
        if result.items.len() == 1 {
            "file"
        } else {
            "files"
        },
        format_bytes(total)
    );
    println!();
    for item in &result.items {
        println!(
            "  {:>9}  {}  ({})",
            format_bytes(item.allocated_size),
            item.name,
            item.display_parent
        );
    }
    println!("\nReview and remove large files in neati (Clean → Large Files).");
}

pub fn apps(insights: &dyn InsightQueries, json: bool) -> Result<i32, String> {
    let inventory = tauri::async_runtime::block_on(insights.installed_apps())?;
    if json {
        print_json("apps", Wrapped { result: &inventory })?;
        return Ok(0);
    }
    print_apps(&inventory);
    Ok(0)
}

fn print_apps(inventory: &InstalledAppInventory) {
    let mut apps: Vec<_> = inventory.apps.iter().collect();
    apps.sort_by_key(|app| std::cmp::Reverse(app.allocated_size));
    for app in apps {
        println!(
            "  {:>9}  {}{}",
            format_bytes(app.allocated_size),
            app.name,
            app.version
                .as_deref()
                .map(|version| format!(" {version}"))
                .unwrap_or_default()
        );
    }
    for reason in &inventory.incomplete_reasons {
        println!("  · {reason}");
    }
    println!("\nUninstall apps and their leftovers in neati (Clean → Apps).");
}

pub fn docker(insights: &dyn InsightQueries, json: bool) -> Result<i32, String> {
    let status = tauri::async_runtime::block_on(insights.docker())?;
    if json {
        print_json("docker", Wrapped { result: &status })?;
        return Ok(0);
    }
    print_docker(&status);
    Ok(0)
}

fn print_docker(status: &DockerStatus) {
    if !status.is_available {
        println!("Docker is not installed or its command was not found.");
        return;
    }
    println!(
        "Docker{} · engine {}",
        status
            .version
            .as_deref()
            .map(|version| format!(" {version}"))
            .unwrap_or_default(),
        if status.is_running {
            "running"
        } else {
            "stopped"
        }
    );
    if let Some(message) = &status.error_message {
        println!("  {message}");
    }
    if status.is_running {
        println!(
            "  {} images · {} containers · {} volumes",
            status.images.len(),
            status.containers.len(),
            status.volumes.len()
        );
    }
    println!("Prune Docker data in neati (System → Docker); `--json` has every field.");
}
