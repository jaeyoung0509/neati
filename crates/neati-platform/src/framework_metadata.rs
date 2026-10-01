//! Inert bounded project metadata. Nothing here imports or evaluates project JS.
use neati_core::domain::storage::{static_framework_config, FrameworkGeneratedKind};
use sha1::{Digest, Sha1};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FrameworkMetadata {
    pub marker_paths: Vec<PathBuf>,
    pub fingerprint: String,
    pub whole_default: Result<(), String>,
    /// These paths are metadata observations, never authorization.
    pub custom_outputs: Vec<PathBuf>,
    pub evidence: Vec<String>,
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    read_regular_bound(path, limit).map(|(bytes, _)| bytes)
}

fn read_regular_bound(path: &Path, limit: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
    let before = fs::symlink_metadata(path).map_err(|_| "Framework metadata is unreadable")?;
    if !before.is_file() || is_link(&before) || before.len() > limit {
        return Err("Framework metadata must be a bounded regular file.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "Framework metadata cannot be opened without following links")?;
    let held = file
        .metadata()
        .map_err(|_| "Framework metadata handle is unreadable")?;
    if !held.is_file()
        || held.len() != before.len()
        || held.modified().ok() != before.modified().ok()
    {
        return Err("Framework metadata changed while opening.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if held.dev() != before.dev() || held.ino() != before.ino() || held.nlink() != 1 {
            return Err("Framework metadata identity or link count is unverified.".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Framework metadata read failed")?;
    let after = fs::symlink_metadata(path).map_err(|_| "Framework metadata disappeared")?;
    if bytes.len() as u64 != before.len()
        || is_link(&after)
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("Framework metadata changed while reading.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err("Framework metadata was replaced.".into());
        }
    }
    let mut stamp = Vec::new();
    stamp.extend(after.len().to_le_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for field in [
            after.dev(),
            after.ino(),
            after.mode() as u64,
            after.uid() as u64,
            after.nlink(),
            after.mtime() as u64,
            after.mtime_nsec() as u64,
            after.ctime() as u64,
            after.ctime_nsec() as u64,
        ] {
            stamp.extend(field.to_le_bytes());
        }
    }
    #[cfg(not(unix))]
    {
        if let Ok(modified) = after.modified() {
            stamp.extend(format!("{modified:?}").as_bytes());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        stamp.extend(after.creation_time().to_le_bytes());
        stamp.extend(after.file_attributes().to_le_bytes());
    }
    Ok((bytes, stamp))
}

pub fn observe(project: &Path, kind: FrameworkGeneratedKind) -> Result<FrameworkMetadata, String> {
    let marker = project.join("package.json");
    let (bytes, marker_stamp) = read_regular_bound(&marker, 256 * 1024)?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| "Framework manifest is not JSON")?;
    let manifest = static_framework_config(
        std::str::from_utf8(&bytes).map_err(|_| "Framework manifest is not UTF-8")?,
    )
    .ok_or("Framework manifest is not bounded unique-key JSON")?;
    let versions: Vec<_> = [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ]
    .iter()
    .filter_map(|section| {
        manifest
            .get(section)?
            .as_object()?
            .get(kind.dependency())?
            .as_str()
    })
    .filter(|version| !version.trim().is_empty() && !version.trim().starts_with("npm:"))
    .collect();
    if versions.is_empty() {
        return Err("No direct matching framework dependency was established.".into());
    }
    let svelte = kind.dependency() == "@sveltejs/kit";
    let supported = if svelte { "2.37.1" } else { "15.5.14" };
    let mut whole_default = if versions.iter().all(|v| *v == supported) {
        Ok(())
    } else {
        Err(format!("Whole-output format is recorded for {} {supported}; other or ranged versions remain observed.", kind.dependency()))
    };
    let config_names: &[&str] = if svelte {
        &[
            "svelte.config.js",
            "svelte.config.mjs",
            "svelte.config.cjs",
            "svelte.config.ts",
        ]
    } else {
        &[
            "next.config.js",
            "next.config.mjs",
            "next.config.cjs",
            "next.config.ts",
            "next.config.cts",
            "next.config.mts",
            "next.config.json",
            "next.config.jsx",
            "next.config.tsx",
        ]
    };
    let mut hash = Sha1::new();
    hash.update(&bytes);
    hash.update(marker_stamp);
    let mut marker_paths = vec![marker];
    let mut custom_outputs = Vec::new();
    let mut configs = 0;
    let mut direct_loader_config = false;
    let mut evidence = vec![format!(
        "Direct dependency: {} {}",
        kind.dependency(),
        versions.join(", ")
    )];
    for name in config_names {
        let config = project.join(name);
        let metadata = match fs::symlink_metadata(&config) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hash.update(name.as_bytes());
                hash.update(b"absent");
                continue;
            }
            Err(_) => {
                whole_default = Err(
                    "Configuration presence is unreadable; default resolution is unknown.".into(),
                );
                continue;
            }
        };
        configs += 1;
        direct_loader_config |=
            ["next.config.js", "next.config.mjs", "next.config.ts"].contains(name);
        marker_paths.push(config.clone());
        if is_link(&metadata) {
            whole_default = Err("Linked framework configuration is protected.".into());
            continue;
        }
        let (bytes, stamp) = match read_regular_bound(&config, 64 * 1024) {
            Ok(bound) => bound,
            Err(reason) => {
                whole_default = Err(reason);
                continue;
            }
        };
        hash.update(name.as_bytes());
        hash.update(&bytes);
        hash.update(stamp);
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => {
                whole_default = Err("Framework configuration is not UTF-8 metadata.".into());
                continue;
            }
        };
        // JSON by itself is not a JavaScript module export. Match only the
        // recorded loader's inert module syntax and package mode.
        let module = manifest.get("type").and_then(serde_json::Value::as_str) == Some("module");
        let source = text.trim();
        let loader_supported = if svelte {
            *name == "svelte.config.js"
        } else {
            ["next.config.js", "next.config.mjs"].contains(name)
        };
        let export = source.starts_with("export default ")
            && ((*name).ends_with(".mjs") || (*name).ends_with(".js") && module);
        let commonjs =
            source.starts_with("module.exports = ") && (*name).ends_with(".js") && !module;
        let parsed = (loader_supported && (export || commonjs))
            .then(|| static_framework_config(text))
            .flatten();
        if let Some(value) = &parsed {
            let output = if svelte {
                value.pointer("/kit/outDir")
            } else {
                value.get("distDir")
            };
            if let Some(output) = output {
                if let Some(relative) = output.as_str().and_then(observed_relative) {
                    if relative != Path::new(if svelte { ".svelte-kit" } else { ".next" }) {
                        custom_outputs.push(relative);
                    }
                }
            }
            let allowed = if svelte {
                value
                    .as_object()
                    .is_some_and(|o| o.keys().all(|k| k == "kit"))
                    && value.get("kit").is_none_or(|kit| {
                        kit.as_object()
                            .is_some_and(|o| o.keys().all(|k| k == "outDir"))
                    })
            } else {
                value
                    .as_object()
                    .is_some_and(|o| o.keys().all(|k| k == "distDir"))
            };
            let default = output
                .is_none_or(|v| v.as_str() == Some(if svelte { ".svelte-kit" } else { ".next" }));
            if !allowed || !default {
                whole_default =
                    Err("Custom or unsupported static configuration remains advisory.".into());
            }
            evidence.push(format!("{name}: bounded static JSON export; custom configuration grants no path authority."));
        } else {
            // A literal in arbitrary JS is only a hint. It is never treated as
            // the configuration's result (comments, conditions and spreads can
            // change what the framework actually evaluates).
            custom_outputs.extend(literal_hints(
                text,
                if svelte { "outDir" } else { "distDir" },
            ));
            whole_default = Err("Dynamic or unknown configuration syntax was not evaluated; whole-output cleanup is unavailable.".into());
            evidence.push(format!(
                "{name}: unknown JavaScript/TypeScript configuration; not executed."
            ));
        }
    }
    if configs > 1 {
        whole_default =
            Err("Multiple framework configuration files have ambiguous precedence.".into());
    }
    if configs == 0 {
        evidence.push("No direct framework config file exists.".into());
    }
    if !svelte && !direct_loader_config {
        // Next 15.5.14 searches ancestors with find-up. An inherited config is
        // not direct project authority. Bind absence through the same bounded
        // final metadata recheck; never load or evaluate an ancestor config.
        let mut reached_root = false;
        for ancestor in project.ancestors().skip(1).take(32) {
            for name in config_names {
                let path = ancestor.join(name);
                hash.update(path.to_string_lossy().as_bytes());
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        hash.update(b"present");
                        hash.update(metadata.len().to_le_bytes());
                        hash.update(format!("{:?}", metadata.modified().ok()).as_bytes());
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            for field in [
                                metadata.dev(),
                                metadata.ino(),
                                metadata.ctime() as u64,
                                metadata.ctime_nsec() as u64,
                            ] {
                                hash.update(field.to_le_bytes());
                            }
                        }
                        whole_default = Err("An ancestor Next configuration prevents verified direct default resolution; the whole output stays observed.".into());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        hash.update(b"absent");
                    }
                    Err(_) => {
                        hash.update(b"unreadable");
                        whole_default = Err("Ancestor Next configuration presence is unreadable; default resolution is unknown.".into());
                    }
                }
            }
            if ancestor.parent().is_none() {
                reached_root = true;
            }
        }
        if !reached_root {
            whole_default = Err("Ancestor Next configuration search reached its 32-level budget; default resolution is unknown.".into());
        }
        evidence.push(
            "Next ancestor config presence was inspected without reading or executing it.".into(),
        );
    }
    custom_outputs.sort();
    custom_outputs.dedup();
    let default = Path::new(if svelte { ".svelte-kit" } else { ".next" });
    custom_outputs.retain(|path| path != default);
    custom_outputs.truncate(16);
    Ok(FrameworkMetadata {
        marker_paths,
        fingerprint: format!("{:x}", hash.finalize()),
        whole_default,
        custom_outputs,
        evidence,
    })
}

fn observed_relative(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > 256 || value.contains(['\\', ':', '*', '?', '\0']) {
        return None;
    }
    let path = PathBuf::from(value);
    (path.components().count() <= 8 && path.components().all(|c| matches!(c, Component::Normal(_))))
        .then_some(path)
}

fn literal_hints(text: &str, key: &str) -> Vec<PathBuf> {
    // Deliberately non-authoritative, bounded to direct relative strings.
    text.match_indices(key)
        .take(16)
        .filter_map(|(index, _)| {
            let rest = text[index + key.len()..]
                .trim_start()
                .trim_start_matches(['\'', '"'])
                .trim_start();
            let rest = rest.strip_prefix(':')?.trim_start();
            let quote = rest.chars().next()?;
            if quote != '\'' && quote != '"' {
                return None;
            }
            let value = rest[1..].split(quote).next()?;
            observed_relative(value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_defaults_custom_dynamic_and_manifest_versions_never_execute_js() {
        let fixture = tempfile::tempdir().unwrap();
        let project = fixture.path();
        let kind = FrameworkGeneratedKind::NextOutput;
        fs::write(
            project.join("package.json"),
            r#"{"dependencies":{"next":"15.5.14"}}"#,
        )
        .unwrap();
        let first = observe(project, kind).unwrap();
        assert!(first.whole_default.is_ok());
        fs::write(
            project.join("next.config.js"),
            "module.exports = {\"distDir\":\"custom/nested\"};",
        )
        .unwrap();
        let custom = observe(project, kind).unwrap();
        assert!(custom.whole_default.is_err());
        assert_eq!(custom.custom_outputs, [PathBuf::from("custom/nested")]);
        assert_ne!(first.fingerprint, custom.fingerprint);
        fs::write(project.join("next.config.js"), "require('fs').writeFileSync('EXECUTED','bad'); module.exports = {distDir: 'unverified'}").unwrap();
        let dynamic = observe(project, kind).unwrap();
        assert!(dynamic.whole_default.unwrap_err().contains("not evaluated"));
        assert_eq!(dynamic.custom_outputs, [PathBuf::from("unverified")]);
        assert!(!project.join("EXECUTED").exists());
        fs::write(
            project.join("next.config.js"),
            "module.exports = {\"distDir\":\".next\"};",
        )
        .unwrap();
        assert!(observe(project, kind).unwrap().whole_default.is_ok());
        let before_replacement = observe(project, kind).unwrap().fingerprint;
        fs::rename(
            project.join("next.config.js"),
            project.join("previous-config"),
        )
        .unwrap();
        fs::write(
            project.join("next.config.js"),
            "module.exports = {\"distDir\":\".next\"};",
        )
        .unwrap();
        assert_ne!(
            observe(project, kind).unwrap().fingerprint,
            before_replacement
        );
        for source in ["{}", "export default {};"] {
            fs::write(project.join("next.config.js"), source).unwrap();
            assert!(observe(project, kind).unwrap().whole_default.is_err());
        }
        fs::write(
            project.join("next.config.js"),
            "module.exports = {\"distDir\":\".next\"};",
        )
        .unwrap();
        fs::write(project.join("next.config.ts"), "export default {};").unwrap();
        assert!(observe(project, kind)
            .unwrap()
            .whole_default
            .unwrap_err()
            .contains("Multiple"));
        fs::remove_file(project.join("next.config.ts")).unwrap();
        for dependency in ["^15.5.14", "16.0.0", "npm:other@15.5.14", ""] {
            fs::write(
                project.join("package.json"),
                serde_json::json!({"dependencies":{"next":dependency}}).to_string(),
            )
            .unwrap();
            if dependency.starts_with("npm:") || dependency.is_empty() {
                assert!(observe(project, kind).is_err());
            } else {
                assert!(observe(project, kind).unwrap().whole_default.is_err());
            }
        }
        fs::write(
            project.join("package.json"),
            r#"{"dependencies":{"svelte":"5","vite":"6"}}"#,
        )
        .unwrap();
        assert!(observe(project, FrameworkGeneratedKind::SvelteKitOutput).is_err());
    }

    #[test]
    fn ambiguous_metadata_and_unsafe_custom_paths_fail_closed() {
        let fixture = tempfile::tempdir().unwrap();
        fs::write(
            fixture.path().join("package.json"),
            r#"{"dependencies":{"next":"15.5.14"},"dependencies":{}}"#,
        )
        .unwrap();
        assert!(observe(fixture.path(), FrameworkGeneratedKind::NextOutput).is_err());
        for value in [
            "",
            "../source",
            "/outside",
            ".",
            "C:/outside",
            "src/../source",
            "cache/*",
            "a\\b",
        ] {
            assert!(observed_relative(value).is_none());
        }
        assert_eq!(
            observed_relative("build/next"),
            Some(PathBuf::from("build/next"))
        );
        fs::write(fixture.path().join("huge"), vec![0; 65_537]).unwrap();
        assert!(read_regular(&fixture.path().join("huge"), 65_536).is_err());
    }

    #[test]
    fn next_ancestor_and_unsupported_configs_never_establish_default_authority() {
        let fixture = tempfile::tempdir().unwrap();
        let project = fixture.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(
            project.join("package.json"),
            r#"{"dependencies":{"next":"15.5.14"}}"#,
        )
        .unwrap();
        let kind = FrameworkGeneratedKind::NextOutput;
        let absent = observe(&project, kind).unwrap();
        assert!(absent.whole_default.is_ok());
        fs::write(
            fixture.path().join("next.config.js"),
            "throw new Error('never evaluate ancestor');",
        )
        .unwrap();
        let inherited = observe(&project, kind).unwrap();
        assert!(inherited.whole_default.unwrap_err().contains("ancestor"));
        assert_ne!(inherited.fingerprint, absent.fingerprint);
        // A complete supported direct export outranks find-up ancestors.
        fs::write(project.join("next.config.js"), "module.exports = {};").unwrap();
        assert!(observe(&project, kind).unwrap().whole_default.is_ok());
        fs::remove_file(project.join("next.config.js")).unwrap();
        fs::remove_file(fixture.path().join("next.config.js")).unwrap();
        for name in [
            "next.config.cjs",
            "next.config.cts",
            "next.config.mts",
            "next.config.json",
            "next.config.jsx",
            "next.config.tsx",
        ] {
            fs::write(
                project.join(name),
                "module.exports = {\"distDir\":\"custom\"};",
            )
            .unwrap();
            let metadata = observe(&project, kind).unwrap();
            assert!(metadata.whole_default.is_err(), "{name}");
            assert_eq!(metadata.custom_outputs, [PathBuf::from("custom")]);
            fs::remove_file(project.join(name)).unwrap();
        }
        let deep = fixture
            .path()
            .join((0..33).map(|_| "n").collect::<PathBuf>());
        fs::create_dir_all(&deep).unwrap();
        fs::write(
            deep.join("package.json"),
            r#"{"dependencies":{"next":"15.5.14"}}"#,
        )
        .unwrap();
        assert!(observe(&deep, kind)
            .unwrap()
            .whole_default
            .unwrap_err()
            .contains("budget"));
    }

    #[cfg(unix)]
    #[test]
    fn linked_and_replaced_config_identity_cannot_grant_whole_authority() {
        use std::os::unix::fs::symlink;
        let fixture = tempfile::tempdir().unwrap();
        fs::write(
            fixture.path().join("package.json"),
            r#"{"devDependencies":{"@sveltejs/kit":"2.37.1"}}"#,
        )
        .unwrap();
        let kind = FrameworkGeneratedKind::SvelteKitOutput;
        let first = observe(fixture.path(), kind).unwrap();
        fs::write(fixture.path().join("outside"), "export default {};").unwrap();
        symlink(
            fixture.path().join("outside"),
            fixture.path().join("svelte.config.js"),
        )
        .unwrap();
        assert!(observe(fixture.path(), kind)
            .unwrap()
            .whole_default
            .is_err());
        assert!(read_regular(&fixture.path().join("svelte.config.js"), 65536).is_err());
        fs::remove_file(fixture.path().join("svelte.config.js")).unwrap();
        fs::write(
            fixture.path().join("svelte.config.js"),
            "export default {\"kit\":{\"outDir\":\"elsewhere\"}};",
        )
        .unwrap();
        assert_ne!(
            observe(fixture.path(), kind).unwrap().fingerprint,
            first.fingerprint
        );
    }
}
