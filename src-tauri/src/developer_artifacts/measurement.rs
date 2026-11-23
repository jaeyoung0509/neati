//! Measuring candidate artifacts and turning measurements into records.

use super::*;

pub(super) fn project_measurement_groups(mut candidates: Vec<Candidate>) -> Vec<Vec<Candidate>> {
    // Place ancestors first so each group's first root covers every member's
    // project-use scope. Discovery already bounds the number of candidates.
    candidates.sort_by_key(|candidate| candidate.project_root.components().count());
    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    for candidate in candidates {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| candidate.project_root.starts_with(&group[0].project_root))
        {
            group.push(candidate);
        } else {
            groups.push(vec![candidate]);
        }
    }
    groups
}

pub(super) fn measure_candidate(
    environment: &PlatformEnvironment,
    candidate: Candidate,
    tx: &mpsc::Sender<MeasurementMessage>,
    cancel: &AtomicBool,
) {
    let _ = tx.send(MeasurementMessage::Started {
        artifact_id: candidate.id.clone(),
        project_name: candidate.project_name.clone(),
        kind: candidate.kind,
    });
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let stats = measure_tree(
        &candidate.path,
        candidate.workspace.identity.device(),
        cancel,
        0,
    );
    let ownership = neati_platform::artifact_ownership::probe_artifact_ownership(
        environment,
        &candidate.project_root,
        &candidate.path,
        cancel,
    );
    let mut candidate = candidate;
    let mut stats = stats;
    if framework_generated_kind(candidate.kind).is_some() {
        if let Some(format) = framework_generated_kind(candidate.kind) {
            if let Ok(metadata) =
                neati_platform::framework_metadata::observe(&candidate.project_root, format)
            {
                candidate.evidence.extend(metadata.evidence);
            }
        }
        let contract = verify_framework_generated_contract(
            environment,
            &candidate.project_root,
            &candidate.path,
            candidate.kind,
            cancel,
        );
        if let Err(reason) = contract {
            if artifact_is_framework_parent(candidate.kind) {
                stats.framework_advisory = Some(reason.clone());
            } else {
                stats.safety_blocked = true;
            }
            candidate.evidence.push(reason);
        } else if neati_platform::temporary_storage::observe_temporary_use(
            environment,
            &candidate.project_root,
        )
        .state
            != neati_core::domain::storage::TemporaryUsageState::NoUseDetected
        {
            stats.safety_blocked = true;
            candidate.evidence.push("Framework project use is active or unknown; cleanup is unavailable until a complete idle observation.".into());
        } else {
            stats.framework_verified = true;
            candidate.evidence.push("A completed macOS use probe detected no current use. This does not establish future abandonment.".into());
        }
    }
    let _ = tx.send(MeasurementMessage::Finished {
        candidate: Box::new(candidate),
        stats,
        ownership,
    });
}

pub(super) fn measure_tree(
    path: &Path,
    _root_device: u64,
    cancel: &AtomicBool,
    depth: usize,
) -> TreeStats {
    measure_tree_bounded(
        path,
        _root_device,
        cancel,
        depth,
        &mut (0usize, std::time::Instant::now()),
    )
}

pub(super) fn measure_tree_bounded(
    path: &Path,
    _root_device: u64,
    cancel: &AtomicBool,
    depth: usize,
    budget: &mut (usize, std::time::Instant),
) -> TreeStats {
    let mut stats = TreeStats::new();
    budget.0 += 1;
    if budget.0 > MAX_DISCOVERY_ENTRIES as usize
        || budget.1.elapsed() > std::time::Duration::from_secs(5)
    {
        stats.complete = false;
        return stats;
    }
    if cancel.load(Ordering::Relaxed) {
        stats.complete = false;
        stats.cancelled = true;
        return stats;
    }
    if depth > MAX_MEASUREMENT_DEPTH {
        stats.complete = false;
        return stats;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            stats.complete = false;
            return stats;
        }
    };
    #[cfg(unix)]
    if metadata.dev() != _root_device {
        stats.complete = false;
        stats.safety_blocked = true;
        return stats;
    }
    if SymlinkGuard::is_symlink(path) {
        stats.complete = false;
        // A nested link is not followed and only makes aggregate size/count
        // partial. The reviewed artifact root itself is still rejected by
        // ReviewedFileIdentity before it can become a cleanup record.
        stats.safety_blocked = depth == 0;
        merge_mtime(&mut stats.newest_mtime, metadata.modified().ok());
        stats.logical_bytes = metadata.len();
        stats.allocated_bytes = allocated_bytes(&metadata);
        stats.file_count = 1;
        return stats;
    }
    merge_mtime(&mut stats.newest_mtime, metadata.modified().ok());
    if metadata.is_file() {
        stats.logical_bytes = metadata.len();
        stats.allocated_bytes = allocated_bytes(&metadata);
        stats.file_count = 1;
        return stats;
    }
    if !metadata.is_dir() {
        return stats;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            stats.complete = false;
            return stats;
        }
    };
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            stats.complete = false;
            stats.cancelled = true;
            break;
        }
        let Ok(entry) = entry else {
            stats.complete = false;
            continue;
        };
        let child = entry.path();
        let child_stats = measure_tree_bounded(&child, _root_device, cancel, depth + 1, budget);
        stats.logical_bytes = stats
            .logical_bytes
            .saturating_add(child_stats.logical_bytes);
        stats.allocated_bytes = stats
            .allocated_bytes
            .saturating_add(child_stats.allocated_bytes);
        stats.file_count = stats.file_count.saturating_add(child_stats.file_count);
        merge_mtime(&mut stats.newest_mtime, child_stats.newest_mtime);
        if !child_stats.complete {
            stats.complete = false;
        }
        if child_stats.cancelled {
            stats.cancelled = true;
            break;
        }
        if child_stats.safety_blocked {
            stats.safety_blocked = true;
        }
        if budget.0 > MAX_DISCOVERY_ENTRIES as usize
            || budget.1.elapsed() > std::time::Duration::from_secs(5)
        {
            stats.complete = false;
            break;
        }
    }
    stats
}

pub(super) fn record_from_measurement(
    candidate: Candidate,
    stats: TreeStats,
    ownership: ArtifactOwnershipEvidence,
) -> Option<DeveloperArtifactRecord> {
    if stats.complete
        && !stats.safety_blocked
        && !stats.cancelled
        && ownership.allows_cleanup()
        && stats.logical_bytes == 0
        && stats.allocated_bytes == 0
    {
        return None;
    }
    let identity = identity_from_path(&candidate.path)?;
    let project_identity = identity_from_path(&candidate.project_root)?;
    let marker_identities = candidate
        .marker_paths
        .iter()
        .filter_map(|path| identity_from_path(path).map(|identity| (path.clone(), identity)))
        .collect::<Vec<_>>();
    let status = if stats.cancelled {
        DeveloperArtifactStatus::ScanCancelled
    } else if stats.safety_blocked
        || marker_identities.len() != candidate.marker_paths.len()
        || !ownership.allows_cleanup()
    {
        DeveloperArtifactStatus::SafetyBlocked
    } else if artifact_is_framework_parent(candidate.kind) && !stats.framework_verified {
        DeveloperArtifactStatus::ObservationOnly
    } else if stats.complete {
        DeveloperArtifactStatus::Complete
    } else {
        DeveloperArtifactStatus::MeasurementIncomplete
    };
    let incomplete_reason = match status {
        DeveloperArtifactStatus::Complete => None,
        DeveloperArtifactStatus::MeasurementIncomplete => Some(
            "Some entries could not be measured; displayed size and file counts may be partial."
                .to_string(),
        ),
        DeveloperArtifactStatus::SafetyBlocked => Some(if let Some(reason) = ownership.refusal_message() {
            reason.to_string()
        } else if stats.safety_blocked && framework_generated_kind(candidate.kind).is_some() {
            candidate.evidence.last().cloned().unwrap_or_else(|| "Generated framework evidence could not be verified.".into())
        } else if stats.safety_blocked {
            "A symbolic link or filesystem boundary could not be verified; cleanup is blocked."
                .to_string()
        } else {
            "Project markers could not be verified; cleanup is blocked.".to_string()
        }),
        DeveloperArtifactStatus::ScanCancelled => Some(
            "The scan was cancelled before this artifact could be fully verified. Scan again before cleanup."
                .to_string(),
        ),
        DeveloperArtifactStatus::ObservationOnly => Some(stats.framework_advisory.unwrap_or_else(|| "Framework output is observed only; its complete generated-only ownership could not be verified.".into())),
    };
    let artifact = DeveloperArtifact {
        id: candidate.id,
        workspace_id: candidate.workspace.workspace.id.clone(),
        project_name: candidate.project_name,
        ecosystem: candidate.ecosystem,
        kind: candidate.kind,
        path: candidate.path.to_string_lossy().into_owned(),
        logical_bytes: stats.logical_bytes,
        allocated_bytes: stats.allocated_bytes,
        file_count: stats.file_count,
        newest_mtime: stats
            .newest_mtime
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs()),
        rebuild_hint: candidate.rebuild_hint,
        evidence: candidate.evidence,
        ownership,
        status,
        incomplete_reason,
        selected_by_default: false,
    };
    Some(DeveloperArtifactRecord {
        artifact,
        path: candidate.path,
        identity,
        workspace_path: candidate.workspace.path,
        workspace_identity: candidate.workspace.identity,
        project_root: candidate.project_root,
        project_identity,
        artifact_relative: candidate.artifact_relative,
        marker_identities,
    })
}

pub(super) fn merge_mtime(current: &mut Option<SystemTime>, candidate: Option<SystemTime>) {
    if let Some(candidate) = candidate {
        *current = Some(match *current {
            Some(existing) => existing.max(candidate),
            None => candidate,
        });
    }
}

pub(super) fn allocated_bytes(metadata: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        metadata.blocks().saturating_mul(512)
    }
    #[cfg(not(unix))]
    {
        metadata.len()
    }
}
