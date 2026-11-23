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
    Scan { all: bool },
    Projects { folders: Vec<String>, all: bool },
    Clean(CleanOptions),
    Ports,
    Ai { refresh: bool },
    History,
    Doctor,
    SkillPrint,
    SkillInstall { target: SkillTarget, force: bool },
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
    fn global_flags_apply_to_every_command() {
        let invocation = run(&["ports", "--json", "-q"]);
        assert_eq!(invocation.command, Command::Ports);
        assert!(invocation.json && invocation.quiet);
    }
}
