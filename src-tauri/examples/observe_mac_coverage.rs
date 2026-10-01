//! Bounded current-machine metadata evidence for the added named observations.
//! No owner CLI, cleanup plan, executor, private payload or path is reported.
use neati_lib::cleaner::{LifecycleProviderRegistry, OwnerProviderRegistry};
use neati_lib::models::{CancellationProbe, CleanStrategy, ObservationQuality, PlatformKind};
use neati_lib::scanner::ScanEngine;
use neati_lib::signatures::SignatureRegistry;
use neati_platform::PlatformEnvironment;
use std::time::{Duration, Instant};

struct Deadline(Instant);
impl CancellationProbe for Deadline {
    fn is_cancelled(&self) -> bool {
        self.0.elapsed() >= Duration::from_secs(30)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment = PlatformEnvironment::native();
    if environment.platform() != PlatformKind::Macos {
        return Err("This observation evidence has a macOS adapter only".into());
    }
    let embedded = SignatureRegistry::load_embedded_with(&environment)?;
    let ids = [
        "dev.oh_my_zsh.cache",
        "dev.github_cli.http_cache",
        "dev.kubernetes.cached_metadata",
        "dev.pyenv.download_observation",
        "dev.precommit.environment_observation",
        "dev.pyinstaller.binary_observation",
        "system.arc.profile_cache_observations",
        "system.chrome_devtools.profile_cache_observations",
    ];
    let mut registry = SignatureRegistry::new();
    for id in ids {
        let signature = embedded.get(id).ok_or("Missing observation signature")?;
        if signature.strategy != CleanStrategy::Manual {
            return Err("Observation evidence must not acquire mutation authority".into());
        }
        registry.register(signature.clone());
    }
    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(vec![]),
        &OwnerProviderRegistry::new(vec![]),
        None,
        &[],
        true,
        &environment,
        &Deadline(Instant::now()),
        |_| {},
    );
    assert_eq!(result.cleanable_bytes, 0);
    let rows: Vec<_> = ids.iter().map(|id| {
        let items: Vec<_> = result.categories.iter().flat_map(|c| &c.items)
            .filter(|item| item.signature_id == *id).collect();
        assert!(items.iter().all(|item| !item.allows_cleanup() && !item.is_selected));
        serde_json::json!({
            "signature_id": id,
            "items": items.len(),
            "observed_bytes": items.iter().map(|item| item.observed_bytes()).sum::<u64>(),
            "incomplete_items": items.iter().filter(|item| item.quality != ObservationQuality::Fresh).count(),
        })
    }).collect();
    let resources = neati_lib::applications::leftovers::scan(&environment);
    let mut classifications = std::collections::BTreeMap::<String, usize>::new();
    for item in &resources.items {
        let label = serde_json::to_value(item.classification)?;
        let label = label
            .as_str()
            .ok_or("Classification is not a typed string")?;
        *classifications.entry(label.to_string()).or_default() += 1;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "application_version": env!("CARGO_PKG_VERSION"),
            "platform": environment.platform(),
            "quality": result.quality,
            "cancelled": result.cancelled,
            "observed_bytes": result.total_bytes,
            "ambiguous_overlap_bytes": result.ambiguous_overlap_bytes,
            "cleanable_bytes": result.cleanable_bytes,
            "actual_cleanup_bytes": 0,
            "free_space_delta_measured": false,
            "signatures": rows,
            "read_only_application_review": {
                "quality": resources.quality,
                "items": resources.items.len(),
                "checked_locations": resources.observed_roots,
                "classifications": classifications,
                "incomplete_reason_count": resources.incomplete_reasons.len(),
                "cleanup_authority": false,
                "gui_tcc_evidence": false,
            },
        }))?
    );
    Ok(())
}
