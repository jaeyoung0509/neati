//! The `neati <command>` grammar (#498).
//!
//! Only a recognised first word makes an invocation a CLI one. Anything else,
//! including no arguments and the `--background` the login item passes, is
//! the desktop app's, so launching neati from Finder never changes behaviour.

use std::path::PathBuf;

/// What the parsed arguments ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Not a CLI invocation: start the desktop app.
    NotCli,
    Run(Invocation),
    /// A CLI command with arguments it does not accept.
    Usage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub command: Command,
    /// Print one JSON object on stdout instead of text.
    pub json: bool,
    /// Never write progress to stderr.
    pub quiet: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Status,
    Scan {
        all: bool,
    },
    Projects {
        folders: Vec<String>,
        all: bool,
    },
    /// Linked Git worktrees in the saved project folders (#505), read-only.
    Worktrees {
        folders: Vec<String>,
    },
    /// Saves a project folder in the app's settings.
    ProjectsAdd {
        path: String,
    },
    /// Forgets a saved project folder.
    ProjectsRemove {
        path: String,
    },
    LargeFiles(LargeFileOptions),
    Apps,
    Docker,
    Clean(CleanOptions),
    Ports,
    Ai {
        refresh: bool,
    },
    History,
    Doctor,
    SkillPrint,
    SkillInstall {
        target: SkillTarget,
        force: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanOptions {
    /// Remove without asking. Without it nothing is removed unless a person
    /// answers the terminal prompt.
    pub yes: bool,
    /// Never remove, never ask.
    pub dry_run: bool,
    /// Restrict the run to these references from the same run's scan.
    pub only: Vec<String>,
    /// Leave these references out.
    pub skip: Vec<String>,
    /// Project folders to include for this run, beyond the saved ones.
    pub folders: Vec<String>,
    pub caches: bool,
    pub projects: bool,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            yes: false,
            dry_run: false,
            only: Vec::new(),
            skip: Vec::new(),
            folders: Vec::new(),
            caches: true,
            projects: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargeFileOptions {
    /// Smallest file reported, in bytes. The backend clamps it to its own
    /// minimum for the chosen filter.
    pub min_size_bytes: u64,
    /// `downloads`, `desktop`, `documents`, `movies`; empty means all four.
    pub roots: Vec<String>,
    pub installers: bool,
}

impl Default for LargeFileOptions {
    fn default() -> Self {
        Self {
            // The app's default threshold.
            min_size_bytes: 500 * 1024 * 1024,
            roots: Vec::new(),
            installers: false,
        }
    }
}

/// The large-file roots the backend recognises.
pub const LARGE_FILE_ROOTS: [&str; 4] = ["downloads", "desktop", "documents", "movies"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillTarget {
    /// `~/.claude/skills`
    Claude,
    /// `~/.codex/skills`
    Codex,
    /// A skills directory the person named.
    Directory(PathBuf),
}

pub fn parse(args: &[String]) -> Parsed {
    let Some(first) = args.first().map(String::as_str) else {
        return Parsed::NotCli;
    };
    let rest = &args[1..];
    let result = match first {
        "help" | "--help" | "-h" => no_options(rest, Command::Help),
        "version" | "--version" | "-V" => no_options(rest, Command::Version),
        "status" => no_options(rest, Command::Status),
        "scan" => parse_scan(rest),
        "projects" => parse_projects(rest),
        "worktrees" => parse_worktrees(rest),
        "large-files" => parse_large_files(rest),
        "apps" => no_options(rest, Command::Apps),
        "docker" => no_options(rest, Command::Docker),
        "clean" => parse_clean(rest),
        "ports" => no_options(rest, Command::Ports),
        "ai" => parse_ai(rest),
        "history" => no_options(rest, Command::History),
        "doctor" => no_options(rest, Command::Doctor),
        "skill" => parse_skill(rest),
        _ => return Parsed::NotCli,
    };
    match result {
        Ok(invocation) => Parsed::Run(invocation),
        Err(message) => Parsed::Usage(message),
    }
}

/// Global flags every command accepts.
#[derive(Default)]
struct Globals {
    json: bool,
    quiet: bool,
}

impl Globals {
    fn take(&mut self, argument: &str) -> bool {
        match argument {
            "--json" => self.json = true,
            "--quiet" | "-q" => self.quiet = true,
            _ => return false,
        }
        true
    }

    fn finish(self, command: Command) -> Invocation {
        Invocation {
            command,
            json: self.json,
            quiet: self.quiet,
        }
    }
}

fn no_options(rest: &[String], command: Command) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    for argument in rest {
        if !globals.take(argument) {
            return Err(unknown(argument));
        }
    }
    Ok(globals.finish(command))
}

fn parse_scan(rest: &[String]) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    let mut all = false;
    for argument in rest {
        match argument.as_str() {
            "--all" => all = true,
            other if globals.take(other) => {}
            other => return Err(unknown(other)),
        }
    }
    Ok(globals.finish(Command::Scan { all }))
}

fn parse_projects(rest: &[String]) -> Result<Invocation, String> {
    match rest.first().map(String::as_str) {
        Some("add") | Some("remove") => return parse_project_edit(rest),
        _ => {}
    }
    let mut globals = Globals::default();
    let mut all = false;
    let mut folders = Vec::new();
    let mut arguments = rest.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--all" => all = true,
            "--folder" => folders.push(value(&mut arguments, "--folder")?),
            other if globals.take(other) => {}
            other => return Err(unknown(other)),
        }
    }
    Ok(globals.finish(Command::Projects { folders, all }))
}

fn parse_worktrees(rest: &[String]) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    let mut folders = Vec::new();
    let mut arguments = rest.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--folder" => folders.push(value(&mut arguments, "--folder")?),
            other if globals.take(other) => {}
            other => return Err(unknown(other)),
        }
    }
    Ok(globals.finish(Command::Worktrees { folders }))
}

/// `projects add PATH` / `projects remove PATH`.
fn parse_project_edit(rest: &[String]) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    let mut path = None;
    for argument in &rest[1..] {
        if globals.take(argument) {
            continue;
        }
        if argument.starts_with("--") || path.is_some() {
            return Err(unknown(argument));
        }
        path = Some(argument.clone());
    }
    let path = path.ok_or_else(|| format!("projects {} needs a folder path.", rest[0]))?;
    Ok(globals.finish(if rest[0] == "add" {
        Command::ProjectsAdd { path }
    } else {
        Command::ProjectsRemove { path }
    }))
}

fn parse_large_files(rest: &[String]) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    let mut options = LargeFileOptions::default();
    let mut arguments = rest.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--min-size" => {
                options.min_size_bytes = parse_size(&value(&mut arguments, "--min-size")?)?
            }
            "--root" => {
                let root = value(&mut arguments, "--root")?.to_lowercase();
                if !LARGE_FILE_ROOTS.contains(&root.as_str()) {
                    return Err(format!(
                        "Unknown root {root}. Use one of: {}.",
                        LARGE_FILE_ROOTS.join(", ")
                    ));
                }
                if !options.roots.contains(&root) {
                    options.roots.push(root);
                }
            }
            "--installers" => options.installers = true,
            other if globals.take(other) => {}
            other => return Err(unknown(other)),
        }
    }
    Ok(globals.finish(Command::LargeFiles(options)))
}

/// `500M`, `1G`, `1.5GB`, `750mb` or a byte count.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    let split = trimmed
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    let number: f64 = number
        .parse()
        .map_err(|_| format!("{text} is not a size. Try 500M or 1G."))?;
    let multiplier: u64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        _ => return Err(format!("{text} is not a size. Try 500M or 1G.")),
    };
    let bytes = number * multiplier as f64;
    if !bytes.is_finite() || bytes < 0.0 || bytes > u64::MAX as f64 {
        return Err(format!("{text} is not a size. Try 500M or 1G."));
    }
    Ok(bytes as u64)
}

fn parse_clean(rest: &[String]) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    let mut options = CleanOptions::default();
    let mut caches_only = false;
    let mut projects_only = false;
    let mut arguments = rest.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--yes" | "-y" => options.yes = true,
            "--dry-run" | "-n" => options.dry_run = true,
            "--only" => options.only.extend(references(&mut arguments, "--only")?),
            "--skip" => options.skip.extend(references(&mut arguments, "--skip")?),
            "--folder" => options.folders.push(value(&mut arguments, "--folder")?),
            "--caches-only" => caches_only = true,
            "--projects-only" => projects_only = true,
            other if globals.take(other) => {}
            other => return Err(unknown(other)),
        }
    }
    if options.yes && options.dry_run {
        return Err("--yes and --dry-run cannot be used together.".to_string());
    }
    if caches_only && projects_only {
        return Err("--caches-only and --projects-only cannot be used together.".to_string());
    }
    options.caches = !projects_only;
    options.projects = !caches_only;
    Ok(globals.finish(Command::Clean(options)))
}

fn parse_ai(rest: &[String]) -> Result<Invocation, String> {
    let mut globals = Globals::default();
    let mut refresh = false;
    for argument in rest {
        match argument.as_str() {
            "--refresh" => refresh = true,
            other if globals.take(other) => {}
            other => return Err(unknown(other)),
        }
    }
    Ok(globals.finish(Command::Ai { refresh }))
}

fn parse_skill(rest: &[String]) -> Result<Invocation, String> {
    let Some(action) = rest.first().map(String::as_str) else {
        return Err("skill needs an action: print or install.".to_string());
    };
    let mut globals = Globals::default();
    let mut arguments = rest[1..].iter();
    match action {
        "print" => {
            for argument in arguments {
                if !globals.take(argument) {
                    return Err(unknown(argument));
                }
            }
            Ok(globals.finish(Command::SkillPrint))
        }
        "install" => {
            let mut target = SkillTarget::Claude;
            let mut force = false;
            while let Some(argument) = arguments.next() {
                match argument.as_str() {
                    "--target" => {
                        target = match value(&mut arguments, "--target")?.as_str() {
                            "claude" => SkillTarget::Claude,
                            "codex" => SkillTarget::Codex,
                            other => {
                                return Err(format!(
                                    "Unknown skill target {other}. Use claude, codex or --dir PATH."
                                ))
                            }
                        }
                    }
                    "--dir" => {
                        target =
                            SkillTarget::Directory(PathBuf::from(value(&mut arguments, "--dir")?))
                    }
                    "--force" => force = true,
                    other if globals.take(other) => {}
                    other => return Err(unknown(other)),
                }
            }
            Ok(globals.finish(Command::SkillInstall { target, force }))
        }
        other => Err(format!(
            "Unknown skill action {other}. Use print or install."
        )),
    }
}

fn value<'a>(
    arguments: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<String, String> {
    match arguments.next() {
        Some(value) if !value.starts_with("--") && !value.is_empty() => Ok(value.clone()),
        _ => Err(format!("{flag} needs a value.")),
    }
}

/// `--only a,b --only c` all name references.
fn references<'a>(
    arguments: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<Vec<String>, String> {
    let raw = value(arguments, flag)?;
    let references: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if references.is_empty() {
        return Err(format!("{flag} needs at least one reference."));
    }
    Ok(references)
}

fn unknown(argument: &str) -> String {
    format!("Unknown option {argument}. Run `neati help` for usage.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(words: &[&str]) -> Parsed {
        parse(
            &words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn run(words: &[&str]) -> Invocation {
        match parse_words(words) {
            Parsed::Run(invocation) => invocation,
            other => panic!("{words:?} did not parse: {other:?}"),
        }
    }

    #[test]
    fn the_app_keeps_every_launch_that_names_no_command() {
        assert_eq!(parse_words(&[]), Parsed::NotCli);
        assert_eq!(parse_words(&["--background"]), Parsed::NotCli);
        assert_eq!(parse_words(&["-psn_0_12345"]), Parsed::NotCli);
        assert_eq!(parse_words(&["--doctor"]), Parsed::NotCli);
    }

    #[test]
    fn clean_is_a_dry_run_unless_yes_is_given() {
        let Command::Clean(options) = run(&["clean"]).command else {
            panic!("clean did not parse as clean");
        };
        assert!(!options.yes);
        assert!(options.caches && options.projects);

        let Command::Clean(options) = run(&["clean", "--yes", "--json"]).command else {
            panic!("clean --yes did not parse as clean");
        };
        assert!(options.yes);
    }

    #[test]
    fn clean_rejects_contradictory_flags() {
        assert!(matches!(
            parse_words(&["clean", "--yes", "--dry-run"]),
            Parsed::Usage(_)
        ));
        assert!(matches!(
            parse_words(&["clean", "--caches-only", "--projects-only"]),
            Parsed::Usage(_)
        ));
    }

    #[test]
    fn references_accept_commas_and_repeats() {
        let Command::Clean(options) =
            run(&["clean", "--only", "a, b", "--only", "c", "--skip", "d"]).command
        else {
            panic!("clean did not parse as clean");
        };
        assert_eq!(options.only, ["a", "b", "c"]);
        assert_eq!(options.skip, ["d"]);
        assert!(matches!(
            parse_words(&["clean", "--only"]),
            Parsed::Usage(_)
        ));
        assert!(matches!(
            parse_words(&["clean", "--only", ","]),
            Parsed::Usage(_)
        ));
    }

    #[test]
    fn a_flag_value_cannot_be_another_flag() {
        assert!(matches!(
            parse_words(&["projects", "--folder", "--json"]),
            Parsed::Usage(_)
        ));
    }

    #[test]
    fn unknown_options_are_usage_errors_not_app_launches() {
        assert!(matches!(
            parse_words(&["status", "--verbose"]),
            Parsed::Usage(_)
        ));
        assert!(matches!(parse_words(&["skill"]), Parsed::Usage(_)));
        assert!(matches!(
            parse_words(&["skill", "install", "--target", "vim"]),
            Parsed::Usage(_)
        ));
    }

    #[test]
    fn skill_install_defaults_to_claude() {
        assert_eq!(
            run(&["skill", "install"]).command,
            Command::SkillInstall {
                target: SkillTarget::Claude,
                force: false
            }
        );
        assert_eq!(
            run(&["skill", "install", "--target", "codex", "--force"]).command,
            Command::SkillInstall {
                target: SkillTarget::Codex,
                force: true
            }
        );
    }

    #[test]
    fn project_folders_are_added_and_removed_by_path() {
        assert_eq!(
            run(&["projects", "add", "~/code", "--json"]).command,
            Command::ProjectsAdd {
                path: "~/code".to_string()
            }
        );
        assert_eq!(
            run(&["projects", "remove", "/Users/me/code"]).command,
            Command::ProjectsRemove {
                path: "/Users/me/code".to_string()
            }
        );
        assert!(matches!(
            parse_words(&["projects", "add"]),
            Parsed::Usage(_)
        ));
        assert!(matches!(
            parse_words(&["projects", "add", "a", "b"]),
            Parsed::Usage(_)
        ));
    }

    #[test]
    fn large_file_options_parse_sizes_and_known_roots() {
        let Command::LargeFiles(options) = run(&[
            "large-files",
            "--min-size",
            "1.5G",
            "--root",
            "Downloads",
            "--installers",
        ])
        .command
        else {
            panic!("large-files did not parse");
        };
        assert_eq!(options.min_size_bytes, 1024 * 1024 * 1024 * 3 / 2);
        assert_eq!(options.roots, ["downloads"]);
        assert!(options.installers);
        assert_eq!(parse_size("500M"), Ok(500 * 1024 * 1024));
        assert_eq!(parse_size("2048"), Ok(2048));
        assert!(parse_size("lots").is_err());
        assert!(matches!(
            parse_words(&["large-files", "--root", "/"]),
            Parsed::Usage(_)
        ));
    }

    #[test]
    fn global_flags_apply_to_every_command() {
        let invocation = run(&["ports", "--json", "-q"]);
        assert_eq!(invocation.command, Command::Ports);
        assert!(invocation.json && invocation.quiet);
    }
}
