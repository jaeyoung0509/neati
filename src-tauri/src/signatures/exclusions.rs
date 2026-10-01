//! One exclusion contract for discovery, measurement, and execution.
use super::SignatureLoader;
use crate::models::PlatformKind;
use neati_platform::path_algebra::{self, PathFlavor};
use neati_platform::selector::PathSelector;
use neati_platform::PlatformEnvironment;
use std::path::Path;

/// Whether a cache namespace name is excluded by the signature's prefix list.
///
/// Exclusion is case-insensitive: a cache namespace's on-disk casing is not
/// stable (APFS is case-insensitive by default, so `familycircled` and
/// `FamilyCircle` resolve to the same directory), and an exclusion that matches
/// more is the fail-safe direction. `include_prefixes` stays case-sensitive,
/// because widening an inclusion widens the cleanup surface.
pub fn is_excluded_namespace(name: &str, exclude_prefixes: &[String]) -> bool {
    let lowered = name.to_lowercase();
    exclude_prefixes
        .iter()
        .any(|prefix| lowered.starts_with(&prefix.to_lowercase()))
}

/// Inclusion prefixes keep their declared casing because widening inclusion
/// would widen deletion authority.
pub fn is_included_namespace(name: &str, prefixes: &[String]) -> bool {
    prefixes.is_empty() || prefixes.iter().any(|prefix| name.starts_with(prefix))
}

// A bare protected name must survive alternate casing on macOS too. Folding
// exclusions only narrows authority; inclusions and rooted path algebra retain
// their existing semantics, including on case-sensitive filesystems.
fn excluded_name_matches(
    candidate: &str,
    name: &str,
    flavor: PathFlavor,
    fold_names: bool,
) -> bool {
    if fold_names {
        candidate.to_lowercase() == name.to_lowercase()
    } else {
        path_algebra::equal(candidate, name, flavor)
    }
}

pub fn is_excluded(path: &Path, exclusions: &[String], environment: &PlatformEnvironment) -> bool {
    exclusions.iter().any(|exclusion| {
        if let Some(expanded) = SignatureLoader::expand_exclusion(exclusion, environment) {
            return path_algebra::contains(
                &expanded.to_string_lossy(),
                &path.to_string_lossy(),
                environment.flavor(),
            );
        }
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                excluded_name_matches(
                    name,
                    exclusion,
                    environment.flavor(),
                    environment.platform() == PlatformKind::Macos,
                )
            })
    })
}

/// Expanded exclusion vocabulary for one observation walk. Expansion depends
/// only on the stated environment; each visited candidate is still compared
/// through the same path algebra, never through a cached filesystem verdict.
pub(crate) struct PreparedExclusions {
    entries: Vec<PreparedExclusion>,
    flavor: PathFlavor,
    fold_names: bool,
}

enum PreparedExclusion {
    Root(String),
    Name(String),
}

impl PreparedExclusions {
    pub(crate) fn new(exclusions: &[String], environment: &PlatformEnvironment) -> Self {
        Self {
            entries: exclusions
                .iter()
                .map(
                    |exclusion| match SignatureLoader::expand_exclusion(exclusion, environment) {
                        Some(root) => PreparedExclusion::Root(root.to_string_lossy().into_owned()),
                        None => PreparedExclusion::Name(exclusion.clone()),
                    },
                )
                .collect(),
            flavor: environment.flavor(),
            fold_names: environment.platform() == PlatformKind::Macos,
        }
    }

    pub(crate) fn is_excluded(&self, path: &Path) -> bool {
        self.entries.iter().any(|entry| match entry {
            PreparedExclusion::Root(root) => {
                path_algebra::contains(root, &path.to_string_lossy(), self.flavor)
            }
            PreparedExclusion::Name(name) => path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|candidate| {
                    excluded_name_matches(candidate, name, self.flavor, self.fold_names)
                }),
        })
    }
}

/// Whether a concrete path lies at or below a root selected by a pattern.
/// This uses the same selector parser as traversal, without touching the disk.
pub(crate) fn reachable_from(pattern: &str, path: &str, flavor: PathFlavor) -> bool {
    if !PathSelector::is_pattern(pattern) {
        return path_algebra::contains(pattern, path, flavor);
    }
    let Ok(selector) = PathSelector::parse(pattern, flavor) else {
        return false;
    };
    let mut candidate = path_algebra::split_path(path, flavor);
    loop {
        if selector.matches(&path_algebra::join_parts(&candidate, flavor), flavor) {
            return true;
        }
        if candidate.components.pop().is_none() {
            return false;
        }
    }
}

/// Keep placeholders symbolic at authoring time: no host filesystem or profile
/// may decide whether a portable catalog loads. Equivalent spellings share a
/// symbol; different roots must be named consistently by the author.
fn symbolic(value: &str) -> String {
    let mut value = SignatureLoader::normalize_pattern(value).replace('\\', "/");
    if value == "~" || value.starts_with("~/") {
        value = format!("${{USER_HOME}}{}", &value[1..]);
    }
    if value == "$TMPDIR" || value.starts_with("$TMPDIR/") {
        value = value.replacen("$TMPDIR", "${TEMP}", 1);
    }
    if let Some(rest) = value.strip_prefix("${") {
        if let Some((name, tail)) = rest.split_once('}') {
            return format!("/__neati_placeholder/{name}{tail}");
        }
    }
    value
}

pub(crate) fn validate_reachability(
    signature: &crate::models::Signature,
) -> Result<(), crate::models::NeatiError> {
    for exclusion in &signature.exclusions {
        let path_shaped = exclusion.starts_with(['~', '$', '/', '\\'])
            || path_algebra::is_absolute(exclusion, PathFlavor::Windows);
        if !path_shaped {
            if exclusion.is_empty() || exclusion.contains(['/', '\\']) {
                return Err(crate::models::NeatiError::InvalidPlan(format!(
                    "Signature `{}` exclusion `{exclusion}` must be a bare entry name or rooted path",
                    signature.id,
                )));
            }
            continue;
        }
        let excluded = symbolic(exclusion);
        let reachable = signature.paths.iter().any(|root| {
            let root = symbolic(root);
            let flavor = if signature.platforms == [crate::models::PlatformKind::Windows]
                || (path_algebra::is_absolute(&root, PathFlavor::Windows) && !root.starts_with('/'))
            {
                PathFlavor::Windows
            } else {
                PathFlavor::Posix
            };
            reachable_from(&root, &excluded, flavor)
        });
        if !reachable {
            return Err(crate::models::NeatiError::InvalidPlan(format!(
                "Signature `{}` has unreachable exclusion `{exclusion}`",
                signature.id,
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_reachability_obeys_components_and_alternatives() {
        let root = symbolic("${LOCAL_APP_DATA}/Packages/*/{TempState,Temp}");
        assert!(reachable_from(
            &root,
            &symbolic("${LOCAL_APP_DATA}/Packages/app/TempState/keep"),
            PathFlavor::Posix
        ));
        assert!(!reachable_from(
            &root,
            &symbolic("${LOCAL_APP_DATA}/Packages/app/LocalState/keep"),
            PathFlavor::Posix
        ));
        assert!(!reachable_from(
            &symbolic("~/cache"),
            &symbolic("~/cache-neighbor/file"),
            PathFlavor::Posix
        ));
        assert!(reachable_from(
            &symbolic("~/cache"),
            &symbolic("${USER_HOME}/cache/file"),
            PathFlavor::Posix
        ));
        assert!(reachable_from(
            &symbolic("$TMPDIR"),
            &symbolic("${TEMP}/keep"),
            PathFlavor::Posix
        ));
    }

    #[test]
    fn filename_exclusions_are_exact_and_windows_paths_fold_case() {
        let posix = PlatformEnvironment::simulated(PathFlavor::Posix);
        let exclusions = vec!["onboarding.json".into()];
        assert!(is_excluded(
            Path::new("/cache/onboarding.json"),
            &exclusions,
            &posix
        ));
        assert!(!is_excluded(
            Path::new("/cache/onboarding.json.bak"),
            &exclusions,
            &posix
        ));
        let windows = PlatformEnvironment::simulated(PathFlavor::Windows);
        assert!(is_excluded(
            Path::new("C:/cache/ONBOARDING.JSON"),
            &exclusions,
            &windows
        ));
        assert!(is_excluded(
            Path::new("C:/CACHE/keep/file"),
            &["c:/cache/keep".into()],
            &windows
        ));
        assert!(!is_excluded(
            Path::new("C:/CACHE/keep-other/file"),
            &["c:/cache/keep".into()],
            &windows
        ));
    }

    #[test]
    fn protected_names_fold_case_on_macos_for_scan_and_execution_only() {
        let exclusions = vec!["CloudKit".into(), "com.apple.Safari".into()];
        let macos =
            PlatformEnvironment::simulated(PathFlavor::Posix).with_platform(PlatformKind::Macos);
        let prepared = PreparedExclusions::new(&exclusions, &macos);
        for (name, denied) in [
            ("cLoUdKiT", true),
            ("cOm.ApPlE.SaFaRi", true),
            ("CloudKit-neighbor", false),
            ("com.apple.Safari-notes", false),
        ] {
            let path = std::path::PathBuf::from("/cache/nested").join(name);
            assert_eq!(prepared.is_excluded(&path), denied, "{name}");
            assert_eq!(is_excluded(&path, &exclusions, &macos), denied, "{name}");
        }
        let linux =
            PlatformEnvironment::simulated(PathFlavor::Posix).with_platform(PlatformKind::Linux);
        assert!(!is_excluded(
            Path::new("/cache/cLoUdKiT"),
            &exclusions,
            &linux
        ));
        assert!(!is_included_namespace("cloudkit", &["CloudKit".into()]));
    }

    #[test]
    fn prepared_observation_exclusions_match_fresh_expansion() {
        for flavor in [PathFlavor::Posix, PathFlavor::Windows] {
            let home = if flavor.is_windows() {
                r"D:\Users\me"
            } else {
                "/Users/me"
            };
            let environment = PlatformEnvironment::simulated(flavor)
                .with_home(home)
                .with_temp_dir(if flavor.is_windows() {
                    r"E:\Temp"
                } else {
                    "/private/tmp"
                });
            let exclusions = vec![
                "onboarding.json".into(),
                "~/cache/keep".into(),
                "$TMPDIR/node-compile-cache".into(),
            ];
            let prepared = PreparedExclusions::new(&exclusions, &environment);
            for (path, denied) in [
                (format!("{home}/cache/keep/data"), true),
                (format!("{home}/cache/keep-other/data"), false),
                (format!("{home}/cache/onboarding.json"), true),
                (format!("{home}/cache/onboarding.json.bak"), false),
                (
                    format!(
                        "{}/node-compile-cache/data",
                        environment.temp_dir().display()
                    ),
                    true,
                ),
                (
                    format!(
                        "{}/node-compile-cache-other/data",
                        environment.temp_dir().display()
                    ),
                    false,
                ),
            ] {
                assert_eq!(
                    prepared.is_excluded(Path::new(&path)),
                    denied,
                    "{flavor:?}: {path}"
                );
                assert_eq!(
                    prepared.is_excluded(Path::new(&path)),
                    is_excluded(Path::new(&path), &exclusions, &environment)
                );
            }
        }
        let no_home = PlatformEnvironment::simulated(PathFlavor::Posix);
        let exclusions = vec!["${USER_HOME}/cache/keep".into()];
        let prepared = PreparedExclusions::new(&exclusions, &no_home);
        assert!(!prepared.is_excluded(Path::new("/cache/keep/data")));
        assert!(!is_excluded(
            Path::new("/cache/keep/data"),
            &exclusions,
            &no_home
        ));
    }
}
