//! Corepack 0.36.0's fixed cache command owns only COREPACK_HOME/v1.
//! The distribution and Node runtime are independent of that mutable store.
use crate::safety::{SymlinkGuard, ToctouGuard};
use neati_platform::PlatformEnvironment;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const VERSION: &[u8] = b"0.36.0\n";
const NODE_VERSION: &[u8] = b"v26.7.0\n";
const FILES: &[(&str, &str, u64)] = &[
    (
        "corepack.js",
        "3655bc798f300951f2070fee411b337d626b0c3ae80c2d24c46ccac4595d4bf9",
        174,
    ),
    (
        "lib/corepack.cjs",
        "abd6f0e7b4e2af310243e202fd908ea64cda545786222f0a3d3f62426e8ad66e",
        530014,
    ),
];

pub(super) fn cache_root(env: &PlatformEnvironment) -> Result<PathBuf, String> {
    let home = env.user_home().ok_or("User home is unavailable")?;
    let cache = home.join(".cache");
    let root = cache.join("node/corepack");
    if env
        .cache_path_override("XDG_CACHE_HOME")
        .is_some_and(|p| p != cache)
        || env
            .cache_path_override("COREPACK_HOME")
            .is_some_and(|p| p != root)
    {
        return Err("Custom, relative or empty Corepack cache roots are unavailable; only the verified default v1 store is supported".into());
    }
    Ok(root.join("v1"))
}

pub(super) fn resolve_distribution(
    path: &Path,
    env: &PlatformEnvironment,
) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let mut roots = vec![PathBuf::from("/opt/homebrew"), PathBuf::from("/usr/local")];
    if let Some(home) = env.user_home() {
        roots.push(home.join(".local/lib/node_modules/corepack"));
        roots.push(home.join(".nvm/versions/node"));
    }
    if canonical.file_name().and_then(|s| s.to_str()) != Some("corepack.js")
        || canonical
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            != Some("dist")
        || !roots.iter().any(|root| canonical.starts_with(root))
    {
        return Err(
            "Corepack needs a verified installed distribution in a supported tool root".into(),
        );
    }
    distribution_files(&canonical)?;
    Ok(canonical)
}

fn distribution_files(script: &Path) -> Result<Vec<PathBuf>, String> {
    let dist = script
        .parent()
        .ok_or("Corepack distribution has no parent")?;
    for directory in [dist.to_path_buf(), dist.join("lib")] {
        let metadata = fs::symlink_metadata(directory).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.mode() & 0o022 != 0 {
            return Err("Corepack distribution ancestry is unsafe".into());
        }
    }
    let mut files = Vec::new();
    for (relative, expected, length) in FILES {
        let path = dist.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
            || metadata.mode() & 0o022 != 0
            || metadata.len() != *length
        {
            return Err(
                "Corepack's executable distribution contains an unsafe or changed file".into(),
            );
        }
        let actual = Sha256::digest(fs::read(&path).map_err(|e| e.to_string())?);
        let actual = actual
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual != *expected {
            return Err(
                "Corepack executable contents are not the verified 0.36.0 distribution".into(),
            );
        }
        files.push(path);
    }
    Ok(files)
}

fn node(env: &PlatformEnvironment) -> Result<PathBuf, String> {
    let path = crate::tooling::resolve_with("node", env)
        .ok_or("Corepack's Node runtime is unavailable")?;
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let mut roots = vec![PathBuf::from("/opt/homebrew"), PathBuf::from("/usr/local")];
    if let Some(home) = env.user_home() {
        roots.push(home.join(".nvm/versions/node"));
    }
    let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !roots.iter().any(|r| path.starts_with(r))
        || !meta.is_file()
        || meta.mode() & 0o111 == 0
        || meta.mode() & 0o022 != 0
    {
        return Err("Corepack's Node runtime is outside a verified installation root".into());
    }
    let mut command = Command::new(&path);
    command
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    let output =
        neati_platform::subprocess::run_with_timeout(command, std::time::Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
    if !output.status.success() || !output.stderr.is_empty() || output.stdout != NODE_VERSION {
        return Err(
            "Corepack cleanup is validated with Node 26.7.0 only; this runtime is unsupported"
                .into(),
        );
    }
    Ok(path)
}

pub(super) fn fingerprint_runtime(
    script: &Path,
    env: &PlatformEnvironment,
    digest: &mut Sha256,
) -> Result<(), String> {
    for file in distribution_files(script)?
        .into_iter()
        .chain(std::iter::once(node(env)?))
    {
        let identity =
            ToctouGuard::capture(&file).ok_or("Corepack runtime identity is unavailable")?;
        digest.update(file.as_os_str().as_encoded_bytes());
        digest.update(format!("{identity:?}"));
    }
    Ok(())
}

pub(super) fn command(
    script: &Path,
    env: &PlatformEnvironment,
    scratch: &Path,
    preview: bool,
) -> Result<Command, String> {
    distribution_files(script)?;
    let root = cache_root(env)?;
    let mut command = Command::new(node(env)?);
    command
        .arg(script)
        .args(if preview {
            &["--version"][..]
        } else {
            &["cache", "clean"][..]
        })
        .env_clear()
        .env("HOME", scratch)
        .env("CFFIXED_USER_HOME", scratch)
        .env("TMPDIR", scratch)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("COREPACK_HOME", root.parent().unwrap())
        .env("COREPACK_ENABLE_NETWORK", "0")
        .env("COREPACK_ENABLE_AUTO_PIN", "0")
        .env("COREPACK_DEFAULT_TO_LATEST", "0")
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("NODE_DISABLE_COMPILE_CACHE", "1")
        .current_dir(scratch);
    Ok(command)
}

// The actually generated marker uses a stable numeric version directory.
// Other prerelease/URL layouts have no recorded positive contract here.
fn marker_matches_version(reference: &str, directory: &str, hash: &str) -> bool {
    let (version, suffix) = reference
        .split_once('+')
        .map_or((reference, None), |(v, s)| (v, Some(s)));
    let components: Vec<_> = version.split('.').collect();
    components.len() == 3
        && version == directory
        && components.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u64>().is_ok()
        })
        && hash.strip_prefix("sha512.").is_some_and(|payload| {
            payload.len() == 128
                && payload
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        && suffix.is_none_or(|value| value == hash)
}

pub(super) fn candidates(
    version: &[u8],
    env: &PlatformEnvironment,
) -> Result<Vec<PathBuf>, String> {
    if version != VERSION {
        return Err("Corepack cleanup is validated for 0.36.0 only".into());
    }
    let root = cache_root(env)?;
    let Some(ancestor) = root.ancestors().find(|p| fs::symlink_metadata(p).is_ok()) else {
        return Err("Corepack cache ancestry is unavailable".into());
    };
    SymlinkGuard::validate_anchored_path(ancestor, env).map_err(|e| e.to_string())?;
    if fs::symlink_metadata(&root).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(vec![root]);
    }
    let uid = env
        .current_user_id()
        .ok_or("Current-user ownership is unknown")?;
    let started = std::time::Instant::now();
    let mut pending = vec![(root.clone(), 0usize)];
    let mut count = 0usize;
    while let Some((path, depth)) = pending.pop() {
        count += 1;
        if count > 10_000 || depth > 32 || started.elapsed() > std::time::Duration::from_secs(5) {
            return Err("Corepack cache inventory exceeded its bounded budget".into());
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink()
            || metadata.uid() != uid
            || metadata.mode() & 0o022 != 0
            || (!metadata.is_dir() && !metadata.is_file())
            || (metadata.is_file() && metadata.nlink() != 1)
        {
            return Err(
                "Corepack cache contains a link, foreign-owned, shared or unsupported entry".into(),
            );
        }
        if depth == 0 && !metadata.is_dir() {
            return Err("Corepack v1 is not an ordinary directory".into());
        }
        if depth == 1
            && (!metadata.is_dir()
                || !matches!(
                    path.file_name().and_then(|s| s.to_str()),
                    Some("npm" | "pnpm" | "yarn")
                ))
        {
            return Err("Corepack v1 contains an unknown package-manager namespace".into());
        }
        if depth == 2 {
            if !metadata.is_dir() {
                return Err("Corepack v1 contains an unknown version entry".into());
            }
            let marker = path.join(".corepack");
            let meta = fs::symlink_metadata(&marker)
                .map_err(|_| "Corepack distribution marker is unavailable")?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65_536 {
                return Err("Corepack distribution marker is unsafe".into());
            }
            let report: serde_json::Value =
                serde_json::from_slice(&fs::read(marker).map_err(|e| e.to_string())?)
                    .map_err(|_| "Corepack distribution marker is invalid")?;
            let parent_name = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|s| s.to_str());
            if report.pointer("/locator/name").and_then(|v| v.as_str()) != parent_name
                || report
                    .pointer("/locator/reference")
                    .and_then(|v| v.as_str())
                    .is_none()
                || report
                    .get("hash")
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| !s.starts_with("sha512.") || s.len() != 135)
            {
                return Err(
                    "Corepack distribution marker does not match its owned namespace".into(),
                );
            }
            let reference = report
                .pointer("/locator/reference")
                .and_then(|value| value.as_str())
                .ok_or("Corepack marker has no version reference")?;
            let directory = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or("Corepack version directory is invalid")?;
            let hash = report
                .get("hash")
                .and_then(|value| value.as_str())
                .ok_or("Corepack marker has no hash")?;
            if !marker_matches_version(reference, directory, hash) {
                return Err("Corepack marker does not match a verified stable version directory and hash; unknown/prerelease/URL layouts remain unavailable".into());
            }
            let valid_path = |v: &serde_json::Value| {
                v.as_str().is_some_and(|s| {
                    !s.is_empty()
                        && Path::new(s)
                            .components()
                            .any(|c| matches!(c, Component::Normal(_)))
                        && Path::new(s)
                            .components()
                            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
                })
            };
            let bins = report
                .get("bin")
                .ok_or("Corepack distribution has no binary map")?;
            if !(bins
                .as_object()
                .is_some_and(|m| !m.is_empty() && m.values().all(valid_path))
                || bins
                    .as_array()
                    .is_some_and(|m| !m.is_empty() && m.iter().all(valid_path)))
            {
                return Err("Corepack distribution binary map is unsafe".into());
            }
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                if pending.len() + count >= 10_000 {
                    return Err("Corepack inventory exceeded its entry limit".into());
                }
                pending.push((entry.map_err(|e| e.to_string())?.path(), depth + 1));
            }
        }
    }
    Ok(vec![root])
}

#[cfg(test)]
mod tests {
    use super::*;
    use neati_platform::path_algebra::PathFlavor;

    fn environment(home: &Path) -> PlatformEnvironment {
        PlatformEnvironment::simulated(PathFlavor::Posix).with_home(home)
    }

    #[test]
    fn empty_relative_and_custom_overrides_never_authorize_default_store() {
        let fixture = tempfile::tempdir().unwrap();
        for override_path in ["", "relative/cache", "/unreviewed/cache"] {
            let env = environment(fixture.path())
                .with_cache_path_override("COREPACK_HOME", override_path);
            assert!(cache_root(&env).is_err());
        }
        let env = environment(fixture.path())
            .with_cache_path_override("COREPACK_HOME", fixture.path().join(".cache/node/corepack"));
        assert_eq!(
            cache_root(&env).unwrap(),
            fixture.path().join(".cache/node/corepack/v1")
        );
    }

    #[test]
    fn whole_v1_scope_rejects_unknown_state_and_adjacent_selection() {
        let fixture = tempfile::tempdir().unwrap();
        let env = environment(fixture.path()).with_current_user_id(unsafe { libc::geteuid() });
        let root = cache_root(&env).unwrap();
        fs::create_dir_all(&root).unwrap();
        assert_eq!(candidates(VERSION, &env).unwrap(), vec![root.clone()]);
        fs::write(root.join("credentials.json"), b"retained").unwrap();
        assert!(candidates(VERSION, &env).is_err());
        assert!(candidates(b"0.37.0\n", &env).is_err());
    }

    #[test]
    fn final_owner_boundary_marker_reference_matches_stable_owned_version() {
        let fixture = tempfile::tempdir().unwrap();
        let env = environment(fixture.path()).with_current_user_id(unsafe { libc::geteuid() });
        let version = cache_root(&env).unwrap().join("pnpm/10.17.1");
        fs::create_dir_all(&version).unwrap();
        let hash = format!("sha512.{}", "a".repeat(128));
        for reference in ["10.17.1".to_string(), format!("10.17.1+{hash}")] {
            fs::write(version.join(".corepack"), serde_json::to_vec(&serde_json::json!({"locator":{"name":"pnpm","reference":reference},"hash":hash,"bin":{"pnpm":"./bin/pnpm.cjs"}})).unwrap()).unwrap();
            assert_eq!(
                candidates(VERSION, &env).unwrap(),
                vec![cache_root(&env).unwrap()]
            );
        }
        for reference in [
            "10.17.2",
            "https://example.invalid/manager.tgz",
            "10.17.1-beta.1",
            "010.17.1",
            "10.17.1+sha512.bad",
        ] {
            fs::write(version.join(".corepack"), serde_json::to_vec(&serde_json::json!({"locator":{"name":"pnpm","reference":reference},"hash":hash,"bin":{"pnpm":"./bin/pnpm.cjs"}})).unwrap()).unwrap();
            assert!(candidates(VERSION, &env).is_err(), "{reference}");
            assert!(version.join(".corepack").exists());
        }
    }

    #[test]
    fn linked_ancestor_or_hard_linked_distribution_is_refused() {
        let fixture = tempfile::tempdir().unwrap();
        let env = environment(fixture.path()).with_current_user_id(unsafe { libc::geteuid() });
        let root = cache_root(&env).unwrap();
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(fixture.path(), root.join("linked")).unwrap();
        assert!(candidates(VERSION, &env).is_err());
        fs::remove_file(root.join("linked")).unwrap();
        let manager = root.join("pnpm/11.0.0");
        fs::create_dir_all(&manager).unwrap();
        fs::write(manager.join(".corepack"), b"{}").unwrap();
        fs::hard_link(
            manager.join(".corepack"),
            fixture.path().join("outside-marker"),
        )
        .unwrap();
        assert!(candidates(VERSION, &env).is_err());
    }
}
