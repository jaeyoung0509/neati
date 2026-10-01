//! The installed gh 2.83.1 owner contract. Only its default local HTTP cache
//! is actionable; XDG overrides remain named advisory observations.
use crate::safety::SymlinkGuard;
use neati_platform::PlatformEnvironment;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) const VERSION_OUTPUT: &[u8] =
    b"gh version 2.83.1 (2025-11-13)\nhttps://github.com/cli/cli/releases/tag/v2.83.1\n";

pub(super) fn cache_root(env: &PlatformEnvironment) -> Result<PathBuf, String> {
    let home = env.user_home().ok_or("User home is unavailable")?;
    let base = home.join(".cache");
    if env
        .cache_path_override("XDG_CACHE_HOME")
        .is_some_and(|p| p != base)
    {
        return Err("Custom GitHub CLI cache roots remain observation-only; their complete owner scope is not validated".into());
    }
    Ok(base.join("gh"))
}

pub(super) fn command(
    executable: &Path,
    env: &PlatformEnvironment,
    scratch: &Path,
    preview: bool,
) -> Result<Command, String> {
    let root = cache_root(env)?;
    let mut command = Command::new(executable);
    command.args(if preview {
        &["--version"][..]
    } else {
        &["config", "clear-cache"][..]
    });
    // Do not inherit authentication, shell, extension or alternate config roots.
    // The command needs no credentials, network request or shell configuration.
    command
        .env_clear()
        .env("HOME", scratch)
        .env("XDG_CACHE_HOME", root.parent().unwrap())
        .env("GH_CONFIG_DIR", scratch.join("configuration"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .current_dir(scratch);
    Ok(command)
}

pub(super) fn candidates(
    version: &[u8],
    env: &PlatformEnvironment,
) -> Result<Vec<PathBuf>, String> {
    if version != VERSION_OUTPUT {
        return Err("GitHub CLI cleanup is validated for installed gh 2.83.1 only; this distribution needs compatibility validation".into());
    }
    let root = cache_root(env)?;
    match std::fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let ancestor = root
                .ancestors()
                .find(|path| std::fs::symlink_metadata(path).is_ok())
                .ok_or("Cache ancestry is unavailable")?;
            SymlinkGuard::validate_anchored_path(ancestor, env).map_err(|e| e.to_string())?;
            return Ok(vec![root.clone()]);
        }
        Err(error) => return Err(error.to_string()),
        Ok(_) => {}
    }
    SymlinkGuard::validate_anchored_path(&root, env).map_err(|e| e.to_string())?;
    let uid = env
        .current_user_id()
        .ok_or("Current-user ownership is unknown")?;
    let mut stack = vec![(root.clone(), 0usize)];
    let mut count = 0usize;
    let began = std::time::Instant::now();
    while let Some((path, depth)) = stack.pop() {
        count += 1;
        if count > 10_000 || began.elapsed() > std::time::Duration::from_secs(5) {
            return Err("GitHub CLI cache inventory exceeded its budget".into());
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || meta.uid() != uid || meta.mode() & 0o022 != 0 {
            return Err(
                "GitHub CLI cache contains a link, foreign-owned or writable-by-others entry"
                    .into(),
            );
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let hexadecimal = name
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
        if meta.is_dir() && depth <= 2 && (depth == 0 || (name.len() == 2 && hexadecimal)) {
            for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
                if count + stack.len() >= 10_000 {
                    return Err("GitHub CLI cache inventory exceeded its budget".into());
                }
                stack.push((entry.map_err(|e| e.to_string())?.path(), depth + 1));
            }
        } else if !(meta.is_file()
            && depth == 3
            && name.len() == 60
            && hexadecimal
            && meta.mode() & 0o111 == 0
            && meta.nlink() == 1)
        {
            return Err("GitHub CLI cache layout is unknown or includes executable/structured state; the whole owner operation is unavailable".into());
        }
    }
    Ok(vec![root])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PlatformEnvironment, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let env = PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
            .with_home(&home)
            .with_current_user_id(std::fs::metadata(&home).unwrap().uid());
        let root = cache_root(&env).unwrap();
        std::fs::create_dir_all(root.join("ab/cd")).unwrap();
        std::fs::write(
            root.join("ab/cd").join("0".repeat(60)),
            b"private HTTP response fixture",
        )
        .unwrap();
        (temp, env, root)
    }
    #[test]
    fn complete_hash_scope_is_metadata_only_and_configuration_is_outside_it() {
        let (_temp, env, root) = fixture();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join("ab/cd").join("0".repeat(60)),
            std::fs::Permissions::from_mode(0o0),
        )
        .unwrap();
        assert_eq!(
            candidates(VERSION_OUTPUT, &env).unwrap(),
            vec![root.clone()]
        );
        let command = command(
            Path::new("/reviewed/gh"),
            &env,
            root.parent().unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["config", "clear-cache"]
        );
        assert!(!command.get_envs().any(|(key, _)| key == "GH_TOKEN"));
    }
    #[test]
    fn unknown_version_override_and_structured_sentinel_refuse_the_whole_operation() {
        let (_temp, env, root) = fixture();
        assert!(candidates(b"gh version 99.0\n", &env).is_err());
        for custom in [
            env.user_home().unwrap(),
            env.user_home().unwrap().join(".config"),
            PathBuf::from("relative"),
        ] {
            assert!(cache_root(
                &env.clone()
                    .with_cache_path_override("XDG_CACHE_HOME", custom)
            )
            .is_err());
        }
        std::fs::write(root.join("hosts.yml"), b"credential sentinel").unwrap();
        assert!(candidates(VERSION_OUTPUT, &env).is_err());
    }
    #[test]
    fn linked_executable_hardlinked_foreign_and_writable_entries_are_refused() {
        let (_temp, env, root) = fixture();
        let file = root.join("ab/cd").join("0".repeat(60));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(candidates(VERSION_OUTPUT, &env).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&file, root.parent().unwrap().join("outside-link")).unwrap();
        assert!(candidates(VERSION_OUTPUT, &env).is_err());
        std::fs::remove_file(root.parent().unwrap().join("outside-link")).unwrap();
        let foreign = env
            .clone()
            .with_current_user_id(env.current_user_id().unwrap().wrapping_add(1));
        assert!(candidates(VERSION_OUTPUT, &foreign).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o622)).unwrap();
        assert!(candidates(VERSION_OUTPUT, &env).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&file, root.join("linked")).unwrap();
        assert!(candidates(VERSION_OUTPUT, &env).is_err());
    }
}
