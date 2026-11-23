//! Which project artifacts one-button Clean removes without another choice.
//!
//! Compiler build output qualifies by default (#494, owner direction
//! 2026-11-22): the project's own build recreates it and nobody edits it by
//! hand. Dependency stores qualify only through the person's dependency rule
//! (#504, owner direction 2026-11-24): the project must have been inactive for
//! the rule's period and the ecosystem's lockfile must sit beside the store, so
//! a reinstall reproduces it. Framework output, global stores and Terraform
//! state stay reviewable.
use super::activity::{has_working_process, project_last_activity, running_working_directories};
use super::DeveloperArtifactRecord;
use crate::models::{
    ArtifactOwnershipEvidence, CleanInclusion, CleanupRules, DeveloperArtifactKind,
    DeveloperArtifactStatus,
};
use neati_platform::open_files::OpenFileState;
use neati_platform::PlatformEnvironment;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DAY_SECONDS: u64 = 24 * 60 * 60;

/// Observes the open-file state of many folders, one verdict per folder.
pub type UseProbe<'a> = dyn Fn(&[PathBuf], &dyn Fn() -> bool) -> Vec<OpenFileState> + Sync + 'a;

/// Reports when the project at a root was last worked on, in epoch seconds.
pub type ActivityProbe<'a> = dyn Fn(&Path) -> Option<u64> + Sync + 'a;

/// Lists the working directories of running processes. Asked at most once per
/// scan, and only when a rule needs project activity.
pub type WorkingDirectories<'a> = dyn Fn() -> Vec<PathBuf> + Sync + 'a;

/// Compiler build outputs, recreated by the project's own build.
pub fn is_build_output(kind: DeveloperArtifactKind) -> bool {
    use DeveloperArtifactKind as Kind;
    matches!(
        kind,
        Kind::CargoTarget
            | Kind::GradleBuild
            | Kind::GradleCache
            | Kind::MavenTarget
            | Kind::SbtTarget
            | Kind::ClojureTarget
            | Kind::CMakeBuild
            | Kind::SwiftBuild
            | Kind::ZigCache
            | Kind::HaskellStackWork
            | Kind::HaskellDistNewstyle
            | Kind::ElixirBuild
            | Kind::ErlangBuild
            | Kind::DotnetObj
    )
}

/// Dependency stores a lockfile reproduces (#504). Global stores (Go module
/// cache, Flutter tooling) and environments a tool manages (tox, nox) are not
/// project dependency stores and stay reviewable.
pub fn is_dependency_store(kind: DeveloperArtifactKind) -> bool {
    !lockfiles(kind).is_empty()
}

/// The lockfiles that reproduce a dependency store, any one of which must sit
/// in the project folder beside it.
pub fn lockfiles(kind: DeveloperArtifactKind) -> &'static [&'static str] {
    use DeveloperArtifactKind as Kind;
    match kind {
        Kind::NodeModules => &[
            "package-lock.json",
            "npm-shrinkwrap.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "bun.lock",
            "bun.lockb",
        ],
        Kind::PythonVenv => &[
            "uv.lock",
            "poetry.lock",
            "Pipfile.lock",
            "pdm.lock",
            "pylock.toml",
        ],
        Kind::ComposerVendor => &["composer.lock"],
        Kind::RubyBundle => &["Gemfile.lock"],
        Kind::ElixirDeps => &["mix.lock"],
        _ => &[],
    }
}

/// Kinds whose removal repeats an exact open-file check immediately before
/// running, whichever workflow asked for it.
pub fn checks_use_before_removal(kind: DeveloperArtifactKind) -> bool {
    is_build_output(kind) || is_dependency_store(kind)
}

fn has_lockfile(project_root: &Path, kind: DeveloperArtifactKind) -> bool {
    lockfiles(kind).iter().any(|name| {
        std::fs::symlink_metadata(project_root.join(name))
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    })
}

/// Every fact except current use: a complete measurement of a build output
/// whose ownership was verified as generated.
pub fn is_candidate(
    kind: DeveloperArtifactKind,
    status: DeveloperArtifactStatus,
    ownership: ArtifactOwnershipEvidence,
) -> bool {
    is_build_output(kind)
        && status == DeveloperArtifactStatus::Complete
        && ownership == ArtifactOwnershipEvidence::VerifiedGenerated
}

/// What the selection reads besides the inventory: the person's rules, the
/// clock, and how activity and use are observed.
pub struct Selection<'a> {
    pub rules: CleanupRules,
    pub now: u64,
    pub use_probe: &'a UseProbe<'a>,
    pub activity: &'a ActivityProbe<'a>,
    /// Working directories of running processes, observed lazily, once.
    pub working_directories: &'a WorkingDirectories<'a>,
}

impl<'a> Selection<'a> {
    /// The native observations with the person's rules.
    pub fn native(rules: CleanupRules, activity: &'a ActivityProbe<'a>) -> Self {
        Self {
            rules,
            now: super::unix_timestamp(),
            use_probe: &neati_platform::open_files::observe_open_files_batch,
            activity,
            working_directories: &running_working_directories,
        }
    }
}

/// The native activity observation for `environment`.
pub fn native_activity(
    environment: &PlatformEnvironment,
) -> impl Fn(&Path) -> Option<u64> + Sync + '_ {
    move |root: &Path| project_last_activity(environment, root)
}

/// The decision before the use check: `Ok` with the idle period that applied,
/// or the reason the artifact stays for review.
fn rule_verdict(
    record: &mut DeveloperArtifactRecord,
    selection: &Selection<'_>,
    activity: &mut HashMap<PathBuf, Option<u64>>,
    working: &mut Option<Vec<PathBuf>>,
) -> Result<(), CleanInclusion> {
    let kind = record.artifact.kind;
    let dependency = is_dependency_store(kind);
    if !is_build_output(kind) && !dependency {
        return Err(CleanInclusion::NotOffered);
    }
    if record.artifact.status != DeveloperArtifactStatus::Complete
        || record.artifact.ownership != ArtifactOwnershipEvidence::VerifiedGenerated
    {
        return Err(CleanInclusion::NotVerified);
    }
    let idle_days = if dependency {
        if !selection.rules.dependency_stores.enabled {
            return Err(CleanInclusion::RuleOff);
        }
        if !has_lockfile(&record.project_root, kind) {
            return Err(CleanInclusion::NoLockfile);
        }
        selection.rules.dependency_stores.idle_days
    } else {
        selection.rules.build_output_min_idle_days
    };
    if idle_days == 0 {
        return Ok(());
    }
    let working = working.get_or_insert_with(|| (selection.working_directories)());
    if has_working_process(&record.project_root, working) {
        return Err(CleanInclusion::RecentlyActive);
    }
    let last = *activity
        .entry(record.project_root.clone())
        .or_insert_with(|| (selection.activity)(&record.project_root));
    record.artifact.project_last_activity = last;
    let Some(last) = last else {
        return Err(CleanInclusion::ActivityUnknown);
    };
    let idle_for = selection.now.saturating_sub(last);
    if idle_for < u64::from(idle_days) * DAY_SECONDS {
        return Err(CleanInclusion::RecentlyActive);
    }
    Ok(())
}

/// Decides every artifact's inclusion. Candidates that pass the person's rules
/// are then checked for current use in one batch; only folders the check
/// proved idle are preselected. In use or unknown stays a reviewable item.
pub fn apply(
    records: &mut HashMap<String, DeveloperArtifactRecord>,
    selection: &Selection<'_>,
    is_cancelled: &dyn Fn() -> bool,
) {
    let mut activity = HashMap::new();
    let mut working = None;
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    let mut keys: Vec<String> = records.keys().cloned().collect();
    keys.sort();
    for id in keys {
        if is_cancelled() {
            return;
        }
        let Some(record) = records.get_mut(&id) else {
            continue;
        };
        record.artifact.selected_by_default = false;
        match rule_verdict(record, selection, &mut activity, &mut working) {
            Ok(()) => candidates.push((id, record.path.clone())),
            Err(reason) => record.artifact.clean_inclusion = reason,
        }
    }
    if candidates.is_empty() || is_cancelled() {
        return;
    }
    let paths: Vec<PathBuf> = candidates.iter().map(|(_, path)| path.clone()).collect();
    let states = (selection.use_probe)(&paths, is_cancelled);
    let verdicts: Vec<OpenFileState> = if is_cancelled() || states.len() != candidates.len() {
        vec![OpenFileState::Unknown; candidates.len()]
    } else {
        states
    };
    for ((id, _), state) in candidates.iter().zip(verdicts) {
        if let Some(record) = records.get_mut(id) {
            record.artifact.selected_by_default = state == OpenFileState::Idle;
            record.artifact.clean_inclusion = match state {
                OpenFileState::Idle => CleanInclusion::Included,
                OpenFileState::InUse => CleanInclusion::InUse,
                OpenFileState::Unknown => CleanInclusion::UseUnknown,
            };
        }
    }
}

/// The use check repeated immediately before a build output or dependency
/// store is removed.
pub fn refusal_before_removal(state: OpenFileState) -> Option<&'static str> {
    match state {
        OpenFileState::Idle => None,
        OpenFileState::InUse => {
            Some("Skipped because a running process has files open in this folder.")
        }
        OpenFileState::Unknown => {
            Some("Skipped because neati could not confirm that nothing is using this folder. Close tools that may use it and try again.")
        }
    }
}
