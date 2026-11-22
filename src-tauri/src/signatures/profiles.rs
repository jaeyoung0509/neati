//! Extra AI tool profiles (#451).
//!
//! People who use several accounts keep one configuration directory per
//! account (`~/.claude-work`, `~/.codex-personal`, …). The catalog only names
//! the default one. A profile the person confirmed in Settings gets the *same
//! reviewed sub-paths* as the default profile, by projecting the tool's
//! observation-only catalog entries onto it.
//!
//! Three rules keep this from becoming cleanup authority:
//!
//! 1. Only the ids in [`PROFILE_SIGNATURES`] are projected, and each must be
//!    observation-only (`risk = manual`, `strategy = manual`) at projection
//!    time. A deletable entry is skipped even if it is listed by mistake.
//! 2. A projected entry gets a synthetic id that is not registered, so the
//!    planner cannot build a plan for it.
//! 3. A profile path is accepted only when it is a real directory inside the
//!    home folder that is not a link and not the default profile.

use std::path::{Component, Path, PathBuf};

use neati_platform::PlatformEnvironment;

use super::SignatureRegistry;
use crate::models::{
    AiProfileTool, AiToolProfile, AiToolProfileSuggestion, CleanStrategy, RiskTier, Signature,
};

/// The most extra profiles kept across all tools.
pub const MAX_AI_TOOL_PROFILES: usize = 8;

/// Catalog entries that describe a tool's default profile directory.
const PROFILE_SIGNATURES: &[(AiProfileTool, &str)] = &[
    (AiProfileTool::Claude, "ai.claude.logs"),
    (AiProfileTool::Claude, "ai.claude.cache"),
    (AiProfileTool::Codex, "ai.codex.cache"),
    (AiProfileTool::Gemini, "ai.gemini.cache"),
    (AiProfileTool::Gemini, "ai.gemini.logs"),
    (AiProfileTool::Opencode, "ai.opencode.cache"),
    (AiProfileTool::Cursor, "ai.cursor.extensions.cache"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileRefusal {
    NotAbsolute,
    OutsideHome,
    DefaultProfile,
    NotADirectory,
    IsALink,
    Unreadable(String),
}

impl std::fmt::Display for ProfileRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAbsolute => formatter.write_str("Enter the full path of the profile folder."),
            Self::OutsideHome => {
                formatter.write_str("A profile folder must be inside your home folder.")
            }
            Self::DefaultProfile => {
                formatter.write_str("That is the default profile, which neati already measures.")
            }
            Self::NotADirectory => formatter.write_str("That path is not a folder."),
            Self::IsALink => formatter
                .write_str("That path is a link. Add the real folder so neati never follows it."),
            Self::Unreadable(reason) => write!(formatter, "The folder could not be read: {reason}"),
        }
    }
}

/// Checks one profile path against the described home and the filesystem.
pub fn validate_profile(
    profile: &AiToolProfile,
    environment: &PlatformEnvironment,
) -> Result<PathBuf, ProfileRefusal> {
    let path = PathBuf::from(profile.path.trim());
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(ProfileRefusal::NotAbsolute);
    }
    let home = environment.user_home().ok_or(ProfileRefusal::OutsideHome)?;
    let relative = path
        .strip_prefix(&home)
        .map_err(|_| ProfileRefusal::OutsideHome)?;
    if relative.as_os_str().is_empty() {
        return Err(ProfileRefusal::OutsideHome);
    }
    if path == home.join(profile.tool.default_dir_name()) {
        return Err(ProfileRefusal::DefaultProfile);
    }
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|error| ProfileRefusal::Unreadable(error.kind().to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(ProfileRefusal::IsALink);
    }
    if !meta.is_dir() {
        return Err(ProfileRefusal::NotADirectory);
    }
    Ok(path)
}

/// Keeps the valid profiles, once each, up to the cap. Used both when a save
/// is validated (to report the first refusal) and when settings load (where a
/// profile that vanished is dropped from measurement, not from the file).
pub fn valid_profiles(
    profiles: &[AiToolProfile],
    environment: &PlatformEnvironment,
) -> Vec<(AiProfileTool, PathBuf)> {
    let mut seen = std::collections::HashSet::new();
    profiles
        .iter()
        .filter_map(|profile| {
            validate_profile(profile, environment)
                .ok()
                .map(|path| (profile.tool, path))
        })
        .filter(|entry| seen.insert(entry.clone()))
        .take(MAX_AI_TOOL_PROFILES)
        .collect()
}

/// Projects the observation-only entries of each tool onto its extra profiles.
pub fn derive_profile_signatures(
    registry: &SignatureRegistry,
    profiles: &[(AiProfileTool, PathBuf)],
) -> Vec<Signature> {
    let mut derived = Vec::new();
    for (tool, profile_dir) in profiles {
        let default_prefix = format!("~/{}/", tool.default_dir_name());
        let label = profile_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| profile_dir.to_string_lossy().into_owned());
        for (_, id) in PROFILE_SIGNATURES
            .iter()
            .filter(|(candidate, _)| candidate == tool)
        {
            let Some(base) = registry.get(id) else {
                continue;
            };
            // Defense in depth: a profile never inherits deletion.
            if base.risk != RiskTier::Manual || base.strategy != CleanStrategy::Manual {
                continue;
            }
            let paths: Vec<String> = base
                .paths
                .iter()
                .filter_map(|pattern| pattern.strip_prefix(&default_prefix))
                .filter(|rest| !rest.is_empty() && is_plain_relative(rest))
                .map(|rest| profile_dir.join(rest).to_string_lossy().into_owned())
                .collect();
            if paths.is_empty() {
                continue;
            }
            let mut signature = base.clone();
            signature.id = profile_signature_id(id, profile_dir);
            signature.name = format!("{} · {label}", base.name);
            signature.paths = paths;
            signature.exclusions = Vec::new();
            derived.push(signature);
        }
    }
    derived
}

/// `cache`, `antigravity-cli/tmp`: literal components only, no selector syntax.
fn is_plain_relative(rest: &str) -> bool {
    Path::new(rest)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        && !rest.contains(['*', '{', '}', '$'])
}

/// A stable id that can never collide with a catalog id (`@` is not used
/// there) and that differs per profile directory.
pub fn profile_signature_id(base_id: &str, profile_dir: &Path) -> String {
    format!(
        "{base_id}@profile:{}",
        &crate::hash::sha256_hex(profile_dir.to_string_lossy().as_bytes())[..16]
    )
}

/// Direct children of the home folder that look like another profile
/// (`.claude-work`, `.codex_personal`). Names only; nothing inside is read.
pub fn suggest_profiles(
    environment: &PlatformEnvironment,
    existing: &[AiToolProfile],
) -> Vec<AiToolProfileSuggestion> {
    let Some(home) = environment.user_home() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&home) else {
        return Vec::new();
    };
    let mut suggestions: Vec<AiToolProfileSuggestion> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let tool = AiProfileTool::ALL.into_iter().find(|tool| {
                let default = tool.default_dir_name();
                name.strip_prefix(default)
                    .is_some_and(|rest| rest.starts_with(['-', '_', '.']) && rest.len() > 1)
            })?;
            // `file_type` does not follow links, so a link is not suggested.
            entry.file_type().ok().filter(|kind| kind.is_dir())?;
            let path = entry.path().to_string_lossy().into_owned();
            Some(AiToolProfileSuggestion { tool, path })
        })
        .filter(|suggestion| {
            !existing
                .iter()
                .any(|profile| profile.tool == suggestion.tool && profile.path == suggestion.path)
        })
        .collect();
    suggestions.sort_by(|left, right| left.path.cmp(&right.path));
    suggestions.truncate(MAX_AI_TOOL_PROFILES);
    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;
    use neati_core::domain::platform::PlatformKind;
    use neati_platform::PathFlavor;

    fn environment(home: &Path) -> PlatformEnvironment {
        PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_home(home)
    }

    fn profile(tool: AiProfileTool, path: &Path) -> AiToolProfile {
        AiToolProfile {
            tool,
            path: path.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn every_projected_catalog_entry_exists_and_is_observation_only() {
        let home = tempfile::tempdir().unwrap();
        let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
        for (_, id) in PROFILE_SIGNATURES {
            let signature = registry
                .get(id)
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(signature.risk, RiskTier::Manual, "{id}");
            assert_eq!(signature.strategy, CleanStrategy::Manual, "{id}");
        }
    }

    #[test]
    fn a_profile_gets_the_default_profiles_sub_paths_under_unregistered_ids() {
        let home = tempfile::tempdir().unwrap();
        let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
        let work = home.path().join(".claude-work");
        let derived =
            derive_profile_signatures(&registry, &[(AiProfileTool::Claude, work.clone())]);

        let mut paths: Vec<String> = derived.iter().flat_map(|s| s.paths.clone()).collect();
        paths.sort();
        let expected: Vec<String> = ["cache", "logs", "mcp-logs", "tmp"]
            .iter()
            .map(|leaf| work.join(leaf).to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, expected);
        for signature in &derived {
            assert!(
                signature.name.ends_with("· .claude-work"),
                "{}",
                signature.name
            );
            assert!(signature.id.contains("@profile:"));
            assert!(
                registry.get(&signature.id).is_none(),
                "synthetic ids stay unregistered"
            );
            assert_eq!(signature.risk, RiskTier::Manual);
            assert_eq!(signature.strategy, CleanStrategy::Manual);
        }
    }

    #[test]
    fn a_profile_never_inherits_a_deletable_entry() {
        let home = tempfile::tempdir().unwrap();
        let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
        for tool in AiProfileTool::ALL {
            let dir = home.path().join(format!("{}-x", tool.default_dir_name()));
            for signature in derive_profile_signatures(&registry, &[(tool, dir.clone())]) {
                assert_eq!(
                    signature.strategy,
                    CleanStrategy::Manual,
                    "{}",
                    signature.id
                );
                assert_eq!(signature.risk, RiskTier::Manual, "{}", signature.id);
                for path in &signature.paths {
                    assert!(
                        Path::new(path).starts_with(&dir),
                        "{path} escapes the profile"
                    );
                }
            }
        }
    }

    #[test]
    fn two_profiles_of_one_tool_get_distinct_ids() {
        let home = tempfile::tempdir().unwrap();
        let registry = SignatureRegistry::load_embedded_with(&environment(home.path())).unwrap();
        let derived = derive_profile_signatures(
            &registry,
            &[
                (AiProfileTool::Codex, home.path().join(".codex-a")),
                (AiProfileTool::Codex, home.path().join(".codex-b")),
            ],
        );
        assert_eq!(derived.len(), 2);
        assert_ne!(derived[0].id, derived[1].id);
    }

    #[test]
    fn validation_accepts_a_real_folder_in_home_and_names_each_refusal() {
        let home = tempfile::tempdir().unwrap();
        let env = environment(home.path());
        let work = home.path().join(".claude-work");
        std::fs::create_dir(&work).unwrap();
        assert_eq!(
            validate_profile(&profile(AiProfileTool::Claude, &work), &env),
            Ok(work.clone())
        );

        let default = home.path().join(".claude");
        std::fs::create_dir(&default).unwrap();
        assert_eq!(
            validate_profile(&profile(AiProfileTool::Claude, &default), &env),
            Err(ProfileRefusal::DefaultProfile)
        );
        assert_eq!(
            validate_profile(&profile(AiProfileTool::Claude, home.path()), &env),
            Err(ProfileRefusal::OutsideHome)
        );
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(
            validate_profile(&profile(AiProfileTool::Claude, outside.path()), &env),
            Err(ProfileRefusal::OutsideHome)
        );
        assert_eq!(
            validate_profile(
                &AiToolProfile {
                    tool: AiProfileTool::Claude,
                    path: ".claude-work".into()
                },
                &env
            ),
            Err(ProfileRefusal::NotAbsolute)
        );
        assert_eq!(
            validate_profile(
                &AiToolProfile {
                    tool: AiProfileTool::Claude,
                    path: format!("{}/.claude-work/../.ssh", home.path().display()),
                },
                &env
            ),
            Err(ProfileRefusal::NotAbsolute)
        );
        let file = home.path().join(".claude-file");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            validate_profile(&profile(AiProfileTool::Claude, &file), &env),
            Err(ProfileRefusal::NotADirectory)
        );
        assert!(matches!(
            validate_profile(
                &profile(AiProfileTool::Claude, &home.path().join(".missing")),
                &env
            ),
            Err(ProfileRefusal::Unreadable(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_profile_is_refused_and_never_suggested() {
        let home = tempfile::tempdir().unwrap();
        let env = environment(home.path());
        let target = tempfile::tempdir().unwrap();
        let link = home.path().join(".claude-link");
        std::os::unix::fs::symlink(target.path(), &link).unwrap();
        assert_eq!(
            validate_profile(&profile(AiProfileTool::Claude, &link), &env),
            Err(ProfileRefusal::IsALink)
        );
        assert!(suggest_profiles(&env, &[]).is_empty());
    }

    #[test]
    fn valid_profiles_drops_invalid_and_duplicate_entries_and_caps_the_list() {
        let home = tempfile::tempdir().unwrap();
        let env = environment(home.path());
        let mut profiles = Vec::new();
        for index in 0..(MAX_AI_TOOL_PROFILES + 3) {
            let dir = home.path().join(format!(".codex-{index}"));
            std::fs::create_dir(&dir).unwrap();
            profiles.push(profile(AiProfileTool::Codex, &dir));
        }
        profiles.push(profiles[0].clone());
        profiles.push(profile(AiProfileTool::Codex, &home.path().join(".gone")));
        assert_eq!(valid_profiles(&profiles, &env).len(), MAX_AI_TOOL_PROFILES);
        assert_eq!(valid_profiles(&profiles[..2], &env).len(), 2);
    }

    #[test]
    fn suggestions_name_lookalike_folders_but_not_the_default_or_added_ones() {
        let home = tempfile::tempdir().unwrap();
        let env = environment(home.path());
        for name in [
            ".claude",
            ".claude-work",
            ".codex_personal",
            ".claudette",
            "Documents",
        ] {
            std::fs::create_dir(home.path().join(name)).unwrap();
        }
        let all = suggest_profiles(&env, &[]);
        let names: Vec<(AiProfileTool, String)> = all
            .iter()
            .map(|s| {
                (
                    s.tool,
                    Path::new(&s.path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect();
        assert_eq!(
            names,
            vec![
                (AiProfileTool::Claude, ".claude-work".to_string()),
                (AiProfileTool::Codex, ".codex_personal".to_string()),
            ]
        );
        let added = profile(AiProfileTool::Claude, &home.path().join(".claude-work"));
        assert_eq!(suggest_profiles(&env, &[added]).len(), 1);
    }

    /// End to end: a confirmed profile's cache is measured by a real scan,
    /// named after its folder, and stays outside anything a plan can remove.
    #[test]
    fn a_scan_measures_a_confirmed_profile_as_observation_only() {
        use crate::models::{Category, NeverCancelled};
        use crate::scanner::engine::ScanEngine;

        let home = tempfile::tempdir().unwrap();
        let env = environment(home.path());
        let work = home.path().join(".claude-work");
        std::fs::create_dir_all(work.join("cache")).unwrap();
        std::fs::write(work.join("cache/blob.bin"), vec![7u8; 64 * 1024]).unwrap();
        // A sibling the catalog does not name must stay invisible.
        std::fs::create_dir_all(work.join("projects")).unwrap();
        std::fs::write(work.join("projects/session.jsonl"), b"private").unwrap();

        let registry = SignatureRegistry::load_embedded_with(&env).unwrap();
        let scan = |registry: &SignatureRegistry| {
            ScanEngine::scan(
                registry,
                &crate::cleaner::LifecycleProviderRegistry::new(Vec::new()),
                &crate::cleaner::OwnerProviderRegistry::new(Vec::new()),
                Some(&[Category::Ai]),
                &[],
                true,
                &env,
                &NeverCancelled,
                |_| {},
            )
        };
        let profile_items = |result: &crate::models::ScanResult| -> Vec<crate::models::ScanItem> {
            result
                .categories
                .iter()
                .flat_map(|category| category.items.clone())
                .filter(|item| item.signature_id.contains("@profile:"))
                .collect()
        };

        assert!(
            profile_items(&scan(&registry)).is_empty(),
            "nothing before the profile is confirmed"
        );

        let valid = valid_profiles(&[profile(AiProfileTool::Claude, &work)], &env);
        registry.set_profile_overlay(derive_profile_signatures(&registry, &valid));
        let items = profile_items(&scan(&registry));
        assert_eq!(items.len(), 1, "{items:?}");
        let item = &items[0];
        assert!(item.name.contains(".claude-work"), "{}", item.name);
        assert!(item.size.observed_bytes() >= 64 * 1024);
        assert!(
            !item.disposition.is_cleanable(),
            "a profile is measured, never cleanable"
        );
        assert!(registry.get(&item.signature_id).is_none());
        assert!(!item.path.contains("projects"));

        registry.set_profile_overlay(Vec::new());
        assert!(
            profile_items(&scan(&registry)).is_empty(),
            "removing the profile stops measuring it"
        );
    }
}
