//! Versioned CocoaPods owner command. Inventory never constructs a downloader
//! cache: its constructor can wipe the store on a VERSION mismatch.
use crate::safety::{SymlinkGuard, ToctouGuard};
use neati_platform::PlatformEnvironment;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const VERSION: &str = "1.16.2";
// Use the installed RubyGems runtime, but bypass pod's launcher and CLAide's
// automatic plugin loading. No inherited RUBYOPT/GEM_HOME/Bundler environment.
const LAUNCHER: &str = r#"
require 'rubygems'
gem 'cocoapods', '= 1.16.2'
require 'cocoapods'
require 'cocoapods/command'
require 'cocoapods/downloader/cache'
require 'json'
abort 'Unsupported CocoaPods version' unless Pod::VERSION == '1.16.2'
Pod::Command.plugin_prefixes = []
if ARGV == ['--neati-inventory']
  puts JSON.generate(version: Pod::VERSION, runtime: File.realpath(RbConfig.ruby), files: $LOADED_FEATURES.select { |p| p.start_with?('/') }.map { |p| File.realpath(p) }.uniq.sort)
else
  abort 'Unsupported command' unless ARGV == ['cache', 'clean', '--all', '--no-ansi', '--silent']
  Pod::Command.run(ARGV)
end
"#;

pub(super) fn cocoapods_cache_root(env: &PlatformEnvironment) -> Result<PathBuf, String> {
    let home = env.user_home().ok_or("User home unavailable")?;
    let root = home.join("Library/Caches/CocoaPods");
    if env.cache_path_override("CP_HOME_DIR").is_some()
        || env
            .cache_path_override("CP_CACHE_DIR")
            .is_some_and(|p| p != root)
    {
        return Err("Custom CocoaPods cache/home roots need compatibility validation".into());
    }
    Ok(root)
}

fn ruby_for_pod(pod: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::metadata(pod).map_err(|e| e.to_string())?;
    if metadata.len() > 16_384 {
        return Err("Unsupported CocoaPods launcher".into());
    }
    let text = std::fs::read_to_string(pod).map_err(|e| e.to_string())?;
    let interpreter = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("#!"))
        .ok_or("CocoaPods needs an absolute RubyGems Ruby launcher")?
        .trim();
    let path = Path::new(interpreter);
    if !path.is_absolute()
        || interpreter.chars().any(char::is_whitespace)
        || path.file_name().and_then(|n| n.to_str()) != Some("ruby")
        || !text.contains("Gem.activate_bin_path('cocoapods', 'pod'")
    {
        return Err("Only standard RubyGems CocoaPods launchers are supported".into());
    }
    let ruby = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if ![
        "/usr/bin",
        "/System/Library/Frameworks/Ruby.framework",
        "/opt/homebrew",
        "/usr/local",
    ]
    .iter()
    .any(|root| ruby.starts_with(root))
        || !ruby.is_file()
    {
        return Err("Ruby is outside supported installation roots".into());
    }
    Ok(ruby)
}

pub(super) fn cocoapods_command(
    pod: &Path,
    cache: &Path,
    scratch: &Path,
    preview: bool,
) -> Result<Command, String> {
    let mut command = Command::new(ruby_for_pod(pod)?);
    command.args(["-e", LAUNCHER, "--"]);
    if preview {
        command.arg("--neati-inventory");
    } else {
        command.args(["cache", "clean", "--all", "--no-ansi", "--silent"]);
    }
    command
        .env_clear()
        .env("HOME", scratch)
        .env("CFFIXED_USER_HOME", scratch)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("CP_HOME_DIR", scratch.join("cocoapods"))
        .env("CP_REPOS_DIR", scratch.join("repositories"))
        .env(
            "CP_CACHE_DIR",
            if preview {
                scratch.join("cache")
            } else {
                cache.to_path_buf()
            },
        )
        // Suppress automatic network error reporting/searches.
        .env("COCOA_PODS_ENV", "development")
        .env("COCOAPODS_DISABLE_STATS", "true")
        .current_dir(scratch);
    Ok(command)
}

fn runtime_report(output: &[u8]) -> Result<serde_json::Value, String> {
    let report: serde_json::Value =
        serde_json::from_slice(output).map_err(|_| "Invalid CocoaPods runtime report")?;
    if report.get("version").and_then(|v| v.as_str()) != Some(VERSION) {
        return Err("CocoaPods cleanup is supported for 1.16.2 only".into());
    }
    Ok(report)
}

pub(super) fn fingerprint_cocoapods_runtime(
    output: &[u8],
    digest: &mut Sha256,
) -> Result<(), String> {
    let report = runtime_report(output)?;
    let runtime = report
        .get("runtime")
        .and_then(|v| v.as_str())
        .ok_or("Missing Ruby runtime")?;
    let files = report
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or("Missing Ruby loaded-file inventory")?;
    if files.is_empty() || files.len() > 2_000 {
        return Err("Ruby inventory is empty or exceeds its limit".into());
    }
    for name in std::iter::once(runtime).chain(files.iter().map(|v| v.as_str().unwrap_or(""))) {
        let path = Path::new(name);
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("Ambiguous Ruby loaded-file path".into());
        }
        let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Unsupported Ruby runtime entry".into());
        }
        digest.update(name.as_bytes());
        digest.update(format!(
            "{:?}",
            ToctouGuard::capture(path).ok_or("Ruby runtime identity unavailable")?
        ));
    }
    Ok(())
}

pub(super) fn cocoapods_candidates(
    output: &[u8],
    env: &PlatformEnvironment,
) -> Result<Vec<PathBuf>, String> {
    runtime_report(output)?;
    let root = cocoapods_cache_root(env)?;
    match std::fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![root]),
        Err(e) => return Err(e.to_string()),
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err("CocoaPods cache is not a regular directory".into())
        }
        Ok(_) => {}
    }
    SymlinkGuard::validate_anchored_path(&root, env).map_err(|e| e.to_string())?;
    // The command deletes the whole root. Unknown siblings must never be
    // silently omitted from the preview or become newly authorized payloads.
    for entry in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        match entry.file_name().to_str() {
            Some("Pods" | "Specs") if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Some("VERSION")
                if meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 64 => {}
            _ => {
                return Err(
                    "CocoaPods cache has unknown root entries; whole-cache cleanup is blocked"
                        .into(),
                )
            }
        }
    }
    if std::fs::read_to_string(root.join("VERSION"))
        .map_err(|e| e.to_string())?
        .trim()
        != VERSION
    {
        return Err(
            "CocoaPods cache version is missing or unsupported; preview leaves it untouched".into(),
        );
    }
    // Lock files inside downloaded packages are ambiguous with downloader
    // locks. Refuse them conservatively, including orphaned locks.
    let mut stack = vec![root.clone()];
    let start = std::time::Instant::now();
    let mut count = 0;
    while let Some(path) = stack.pop() {
        count += 1;
        if count > 10_000 || start.elapsed() > std::time::Duration::from_secs(10) {
            return Err("CocoaPods inventory exceeded its budget".into());
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink()
            || (!meta.is_dir() && !meta.is_file())
            || path.extension().is_some_and(|ext| ext == "lock")
        {
            return Err("CocoaPods inventory contains a link, lock or unsupported entry".into());
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
                if stack.len() + count >= 10_000 {
                    return Err("CocoaPods inventory exceeded its budget".into());
                }
                stack.push(entry.map_err(|e| e.to_string())?.path());
            }
        }
    }
    Ok(vec![root])
}

// Absence or incompatibility of the owner must not hide observed cache bytes.
// These units are always blocked and cannot match a fingerprinted command plan.
pub(super) fn blocked_observation(
    env: &PlatformEnvironment,
    refused: crate::models::OwnerStoreObservation,
) -> crate::models::OwnerStoreObservation {
    use neati_core::domain::cleanup::OwnerUnitMeasurer;
    let Some(home) = env.user_home() else {
        return refused;
    };
    let root = home.join("Library/Caches/CocoaPods");
    if SymlinkGuard::validate_anchored_path(&root, env).is_err() {
        return refused;
    }
    let measured = crate::scanner::SizeCalculatorMeasurement.measure(&root);
    if measured.allocated_bytes == 0 {
        return refused;
    }
    let detail = refused
        .detail
        .unwrap_or_else(|| "CocoaPods cleanup is unavailable".into());
    crate::models::OwnerStoreObservation::ready(
        Some(root.clone()),
        vec![crate::models::OwnerUnitObservation::blocked(
            "observed-default-cache",
            root,
            measured.allocated_bytes,
            measured.allocated_bytes,
            1,
            detail,
        )
        .with_inspection_issue(
            refused
                .inspection_issue
                .unwrap_or(crate::models::ScanGapKind::Unknown),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PlatformEnvironment, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let env = PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
            .with_home(temp.path());
        let root = cocoapods_cache_root(&env).unwrap();
        std::fs::create_dir_all(root.join("Pods/Release/Example")).unwrap();
        std::fs::create_dir(root.join("Specs")).unwrap();
        std::fs::write(root.join("VERSION"), VERSION).unwrap();
        std::fs::write(root.join("Pods/Release/Example/source.m"), b"cached source").unwrap();
        (temp, env, root)
    }
    fn report() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"version":VERSION,"runtime":"/usr/bin/ruby","files":["/usr/bin/ruby"]})).unwrap()
    }
    #[test]
    fn whole_scope_refuses_overrides_unknown_versions_and_root_entries_without_mutation() {
        let (temp, env, root) = fixture();
        assert_eq!(
            cocoapods_candidates(&report(), &env).unwrap(),
            vec![root.clone()]
        );
        assert!(cocoapods_candidates(
            &report(),
            &env.clone()
                .with_cache_path_override("CP_HOME_DIR", temp.path().join("custom"))
        )
        .is_err());
        assert!(cocoapods_candidates(
            &report(),
            &env.clone()
                .with_cache_path_override("CP_CACHE_DIR", temp.path().join("project/Pods"))
        )
        .is_err());
        std::fs::write(root.join("VERSION"), "1.15.0").unwrap();
        assert!(cocoapods_candidates(&report(), &env).is_err());
        assert!(root.join("Pods/Release/Example/source.m").exists());
        std::fs::write(root.join("VERSION"), VERSION).unwrap();
        std::fs::write(root.join("credentials.json"), b"keep").unwrap();
        assert!(cocoapods_candidates(&report(), &env).is_err());
        assert_eq!(
            std::fs::read(root.join("credentials.json")).unwrap(),
            b"keep"
        );
        let mut wrong: serde_json::Value = serde_json::from_slice(&report()).unwrap();
        wrong["version"] = "1.17.0".into();
        assert!(cocoapods_candidates(&serde_json::to_vec(&wrong).unwrap(), &env).is_err());
    }
    #[test]
    fn links_and_downloader_locks_block_the_entire_command() {
        let (temp, env, root) = fixture();
        let external = temp.path().join("project");
        std::fs::create_dir(&external).unwrap();
        std::fs::write(external.join("Podfile"), "keep").unwrap();
        let link = root.join("Pods/link");
        std::os::unix::fs::symlink(&external, &link).unwrap();
        assert!(cocoapods_candidates(&report(), &env).is_err());
        std::fs::remove_file(link).unwrap();
        std::fs::write(root.join("Pods/Release/Example.lock"), "lock").unwrap();
        assert!(cocoapods_candidates(&report(), &env).is_err());
        assert!(external.join("Podfile").exists());
    }
    #[test]
    fn unavailable_owner_keeps_bytes_visible_but_unselectable() {
        let (_temp, env, root) = fixture();
        let refused = crate::models::OwnerStoreObservation::refused(
            crate::models::ProviderStatus::Blocked,
            None,
            "Tool missing",
        );
        let observed = blocked_observation(&env, refused);
        assert_eq!(observed.root, Some(root));
        assert!(observed.units[0].allocated_bytes > 0);
        assert!(!observed.has_ready_units());
    }
    #[test]
    fn runtime_replacement_changes_review_fingerprint_and_invalid_reports_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library.rb");
        std::fs::write(&library, "before").unwrap();
        let report = serde_json::to_vec(
            &serde_json::json!({"version":VERSION,"runtime":"/usr/bin/ruby","files":[library]}),
        )
        .unwrap();
        let mut before = Sha256::new();
        fingerprint_cocoapods_runtime(&report, &mut before).unwrap();
        std::fs::write(&library, "replacement").unwrap();
        let mut after = Sha256::new();
        fingerprint_cocoapods_runtime(&report, &mut after).unwrap();
        assert_ne!(before.finalize(), after.finalize());
        assert!(fingerprint_cocoapods_runtime(b"{}", &mut Sha256::new()).is_err());
    }
    #[test]
    fn actual_ruby_command_uses_pinned_owner_code_and_isolates_repositories_projects_and_preview() {
        let (temp, _env, root) = fixture();
        let scratch = temp.path().join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        let pod = temp.path().join("pod");
        std::fs::write(
            &pod,
            "#!/usr/bin/ruby\nGem.activate_bin_path('cocoapods', 'pod', version)\n",
        )
        .unwrap();
        let gems = temp.path().join("fixture-gems");
        let library = gems.join("gems/cocoapods-1.16.2/lib/cocoapods");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir(gems.join("specifications")).unwrap();
        std::fs::write(gems.join("specifications/cocoapods-1.16.2.gemspec"), "Gem::Specification.new { |s| s.name='cocoapods'; s.version='1.16.2'; s.summary='fixture'; s.authors=['fixture']; s.files=[]; s.require_paths=['lib'] }").unwrap();
        std::fs::write(
            library.with_extension("rb"),
            "module Pod; VERSION='1.16.2'; end",
        )
        .unwrap();
        std::fs::create_dir_all(library.join("downloader")).unwrap();
        std::fs::write(
            library.join("downloader/cache.rb"),
            "# fixture cache facade is supplied by command.rb",
        )
        .unwrap();
        let owner = include_str!("../../../tests/fixtures/cocoapods-1.16.2/clean.rb");
        // Small test facade supplies only the owner command's UI/CLAide API.
        // The removal implementation below is the unmodified upstream file.
        let facade = r#"
require 'fileutils'
require 'pathname'
module CLAide; class Argument; def initialize(*); end; end; end
module Pod
  module UI; def self.message(*); yield; end; end
  class Command
    class << self; attr_accessor :plugin_prefixes, :summary, :description, :arguments; end
    def self.run(argv)
      raise 'Plugins enabled' unless plugin_prefixes == []
      command = Cache::Clean.allocate
      command.instance_variable_set(:@cache, Struct.new(:root).new(Pathname.new(ENV.fetch('CP_CACHE_DIR'))))
      command.send(:clear_cache)
    end
    class Cache < Command; end
  end
end
"#;
        std::fs::write(library.join("command.rb"), format!("{facade}\n{owner}")).unwrap();
        let sentinels = [
            ".cocoapods/repos/trunk/spec.json",
            "project/Pods/installed.m",
            "project/Podfile.lock",
            ".cocoapods/config.yaml",
            ".netrc",
        ];
        for name in sentinels {
            let path = temp.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"keep").unwrap();
        }
        for preview in [true, false] {
            let mut command = cocoapods_command(&pod, &root, &scratch, preview).unwrap();
            // Only the test process selects this disposable fixture gem set.
            command.env("GEM_HOME", &gems).env("GEM_PATH", &gems);
            let output = neati_platform::subprocess::run_with_timeout(
                command,
                std::time::Duration::from_secs(10),
            )
            .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            if preview {
                fingerprint_cocoapods_runtime(&output.stdout, &mut Sha256::new()).unwrap();
                assert!(root.join("VERSION").exists());
                assert!(root.join("Pods/Release/Example/source.m").exists());
            } else {
                assert!(!root.exists());
            }
            for name in sentinels {
                assert_eq!(std::fs::read(temp.path().join(name)).unwrap(), b"keep");
            }
        }
    }
}
