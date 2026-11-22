//! Which project build outputs one-button Clean removes without another
//! choice (#494, owner direction 2026-11-22).
//!
//! Only compiler build outputs qualify: the project's own build recreates them
//! and nobody edits them by hand. Dependency stores, framework outputs and
//! Terraform state stay reviewable, because removing them costs a download,
//! may lose local changes, or changes which workspace a tool targets.
use super::DeveloperArtifactRecord;
use crate::models::{ArtifactOwnershipEvidence, DeveloperArtifactKind, DeveloperArtifactStatus};
use neati_platform::open_files::OpenFileState;
use std::collections::HashMap;
use std::path::PathBuf;

/// Observes the open-file state of many folders, one verdict per folder.
pub type UseProbe<'a> = dyn Fn(&[PathBuf], &dyn Fn() -> bool) -> Vec<OpenFileState> + Sync + 'a;

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

/// Marks the candidates an exact open-file check proved idle. A folder that is
/// in use, or whose use could not be determined, stays a reviewable item.
pub fn apply(
    records: &mut HashMap<String, DeveloperArtifactRecord>,
    probe: &UseProbe<'_>,
    is_cancelled: &dyn Fn() -> bool,
) {
    let candidates: Vec<(String, PathBuf)> = records
        .iter()
        .filter(|(_, record)| {
            let artifact = &record.artifact;
            is_candidate(artifact.kind, artifact.status, artifact.ownership)
        })
        .map(|(id, record)| (id.clone(), record.path.clone()))
        .collect();
    if candidates.is_empty() || is_cancelled() {
        return;
    }
    let paths: Vec<PathBuf> = candidates.iter().map(|(_, path)| path.clone()).collect();
    let states = probe(&paths, is_cancelled);
    if is_cancelled() || states.len() != candidates.len() {
        return;
    }
    for ((id, _), state) in candidates.iter().zip(states) {
        if let Some(record) = records.get_mut(id) {
            record.artifact.selected_by_default = state == OpenFileState::Idle;
        }
    }
}

/// The use check repeated immediately before a build output is removed.
pub fn refusal_before_removal(state: OpenFileState) -> Option<&'static str> {
    match state {
        OpenFileState::Idle => None,
        OpenFileState::InUse => {
            Some("Skipped because a running process has files open in this build folder.")
        }
        OpenFileState::Unknown => {
            Some("Skipped because neati could not confirm that no build is using this folder.")
        }
    }
}
