//! `neati <command>`: the terminal and agent interface (#498).
//!
//! The CLI is a driving adapter beside the Tauri one. Its handlers receive
//! only the driving ports `composition::application_ports` binds —
//! `CleanUseCase` and `InsightQueries` — so a terminal run scans, chooses and
//! removes exactly as the window does, and a test drives the handlers with
//! fake ports. It runs before Tauri starts and only for a recognised command
//! word; every other launch is the app's.
//!
//! Layout: `args` (grammar) → `context` (CLI composition root) → one module
//! per command family (presentation and approval) → ports.

mod ai;
pub mod args;
mod clean;
pub mod context;
pub mod output;
mod scan;
pub mod skill;
mod system;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use args::{Command, Parsed};
use context::CliContext;

const USAGE: &str = "\
neati — free developer disk space on your Mac

USAGE:
  neati <command> [options] [--json] [--quiet]

COMMANDS:
  status              Disk, memory and the last cleanup
  scan [--all]        Caches neati found; --all includes ones that need review
  projects [--all]    Build output in your saved project folders
      --folder PATH   Also scan this folder for this run (repeatable)
  clean               Preview what one-button Clean removes (nothing is removed)
      --yes           Permanently remove the previewed set
      --only REF      Remove only these references (repeatable, comma-separated)
      --skip REF      Leave these references out
      --folder PATH   Include this project folder for this run
      --caches-only | --projects-only
  ports               Development servers listening on this Mac
  ai [--refresh]      AI tool usage (Claude, Codex, Gemini, …)
  history             Past cleanups
  doctor              Platform self-check (same as `Neati --doctor`)
  skill print         Print the agent skill
  skill install [--target claude|codex] [--dir PATH] [--force]
                      Install the agent skill for Claude Code or Codex
  help | version

OPTIONS:
  --json    Print one JSON object on stdout (schema neati.cli/1)
  --quiet   No progress on stderr

`neati clean` only ever removes the set the app's Clean button would: idle,
regenerable caches and idle, verified build output. Items that need review,
large files, apps and Docker data stay in the app. Removal is permanent.

EXIT CODES:
  0  success (a preview is a success)
  1  the command failed, or some items were not removed
  2  usage error";

/// Runs a CLI command when `args` name one. `None` means start the app.
pub fn run(args: &[String]) -> Option<i32> {
    let invocation = match args::parse(args) {
        Parsed::NotCli => return None,
        Parsed::Usage(message) => {
            eprintln!("neati: {message}");
            return Some(2);
        }
        Parsed::Run(invocation) => invocation,
    };
    let json = invocation.json;
    let quiet = invocation.quiet;
    let name = command_name(&invocation.command);
    let outcome = match &invocation.command {
        Command::Help => {
            println!("{USAGE}");
            Ok(0)
        }
        Command::Version => {
            if json {
                output::print_json(
                    "version",
                    serde_json::json!({ "version": env!("CARGO_PKG_VERSION") }),
                )
                .map(|()| 0)
            } else {
                println!("neati {}", env!("CARGO_PKG_VERSION"));
                Ok(0)
            }
        }
        Command::Doctor => {
            let mut doctor = vec!["--doctor".to_string()];
            if json {
                doctor.push("--json".to_string());
            }
            Ok(crate::diagnostics::doctor::run_cli(&doctor).unwrap_or(1))
        }
        Command::SkillPrint => Ok(skill::print()),
        Command::SkillInstall { target, force } => {
            let home = neati_platform::PlatformEnvironment::native().user_home();
            skill::install(home.as_deref(), target, *force, json)
        }
        command => run_folders(command)
            .and_then(CliContext::native)
            .and_then(|context| {
                dispatch(
                    command,
                    &context,
                    &clean::TerminalApproval,
                    Arc::new(output::Progress::new(quiet)),
                    json,
                )
            }),
    };
    Some(match outcome {
        Ok(code) => code,
        Err(message) => {
            output::print_error(name, json, &message);
            1
        }
    })
}

/// Project folders this run adds to the saved ones.
fn run_folders(command: &Command) -> Result<&[String], String> {
    Ok(match command {
        Command::Projects { folders, .. } => folders,
        Command::Clean(options) => &options.folders,
        _ => &[],
    })
}

/// Runs a port-backed command. Everything a handler touches comes in here.
fn dispatch(
    command: &Command,
    context: &CliContext,
    approval: &dyn clean::Approval,
    progress: Arc<output::Progress>,
    json: bool,
) -> Result<i32, String> {
    let ports = &context.ports;
    let home = context.home.as_deref();
    match command {
        Command::Status => system::status(ports.insights.as_ref(), json),
        Command::Ports => system::ports(ports.insights.as_ref(), home, json),
        Command::History => system::history(ports.insights.as_ref(), json),
        Command::Ai { refresh } => ai::ai(ports.insights.as_ref(), progress, *refresh, json),
        Command::Scan { all } => scan::Inventory {
            use_case: ports.clean.as_ref(),
            progress,
            home,
            json,
        }
        .caches(*all),
        Command::Projects { all, .. } => scan::Inventory {
            use_case: ports.clean.as_ref(),
            progress,
            home,
            json,
        }
        .projects(*all),
        Command::Clean(options) => clean::CleanRun {
            use_case: ports.clean.as_ref(),
            approval,
            progress,
            home,
            json,
        }
        .run(options),
        Command::Help
        | Command::Version
        | Command::Doctor
        | Command::SkillPrint
        | Command::SkillInstall { .. } => Ok(0),
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Help => "help",
        Command::Version => "version",
        Command::Status => "status",
        Command::Scan { .. } => "scan",
        Command::Projects { .. } => "projects",
        Command::Clean(_) => "clean",
        Command::Ports => "ports",
        Command::Ai { .. } => "ai",
        Command::History => "history",
        Command::Doctor => "doctor",
        Command::SkillPrint | Command::SkillInstall { .. } => "skill",
    }
}
