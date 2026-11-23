//! Recognising generated artifacts inside a project from its markers.

use super::*;

pub(super) fn recognize_artifact(
    workspace: &DeveloperWorkspaceRecord,
    project_root: &Path,
    child_name: &str,
) -> Option<Candidate> {
    let artifact_match = rules::recognize(project_root, child_name)?;
    let candidate_path = project_root.join(&artifact_match.artifact_relative);
    if !candidate_path.is_dir() {
        return None;
    }
    Some(candidate_from_match(
        workspace,
        candidate_path,
        project_root,
        child_name,
        artifact_match,
    ))
}

pub(super) fn validate_observed_components(root: &Path, path: &Path) -> Result<(), ()> {
    let relative = path.strip_prefix(root).map_err(|_| ())?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(());
        };
        cursor.push(name);
        if SymlinkGuard::is_symlink(&cursor) {
            return Err(());
        }
    }
    Ok(())
}

pub(super) fn global_go_module_candidate(
    environment: &PlatformEnvironment,
    workspace: &DeveloperWorkspaceRecord,
    seen_paths: &mut HashSet<PathBuf>,
) -> Option<Candidate> {
    let home = environment.user_home()?;
    let canonical_home = fs::canonicalize(&home).ok()?;
    let expected_root = fs::canonicalize(home.join("go")).ok()?;
    if workspace.path != expected_root && workspace.path != canonical_home {
        return None;
    }
    let path = expected_root.join("pkg/mod");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_dir() || SymlinkGuard::is_symlink(&path) || !seen_paths.insert(path.clone()) {
        return None;
    }
    Some(Candidate {
        id: Uuid::new_v4().to_string(),
        workspace: workspace.clone(),
        project_name: "Go module cache".to_string(),
        ecosystem: DeveloperEcosystem::Go,
        kind: DeveloperArtifactKind::GoModuleCache,
        path,
        project_root: expected_root,
        artifact_relative: PathBuf::from("pkg/mod"),
        marker_paths: Vec::new(),
        evidence: vec![if workspace.whole_home {
            "Built-in whole-home scan scope".to_string()
        } else {
            "Explicitly selected ~/go workspace".to_string()
        }],
        rebuild_hint: Some("go mod download".to_string()),
    })
}

pub(super) fn candidate_from_match(
    workspace: &DeveloperWorkspaceRecord,
    path: PathBuf,
    project_root: &Path,
    child_name: &str,
    artifact_match: ArtifactMatch,
) -> Candidate {
    let project_name = artifact_match
        .project_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(project_root.to_string_lossy().as_ref())
        .to_string();
    let artifact_relative = if artifact_match.artifact_relative.as_os_str().is_empty() {
        PathBuf::from(child_name)
    } else {
        artifact_match.artifact_relative
    };
    Candidate {
        id: Uuid::new_v4().to_string(),
        workspace: workspace.clone(),
        project_name,
        ecosystem: artifact_match.ecosystem,
        kind: artifact_match.kind,
        path,
        project_root: artifact_match.project_root,
        artifact_relative,
        marker_paths: artifact_match.marker_paths,
        evidence: artifact_match.evidence,
        rebuild_hint: artifact_match.rebuild_hint,
    }
}

pub(super) fn recognize_node(project_root: &Path) -> Option<ArtifactMatch> {
    let package_json = find_named_marker(project_root, &["package.json"])?;
    let mut marker_paths = vec![package_json.clone()];
    let mut evidence = vec!["package.json".to_string()];
    let lockfiles = [
        ("pnpm-lock.yaml", "pnpm install"),
        ("package-lock.json", "npm ci"),
        ("yarn.lock", "yarn install"),
        ("bun.lock", "bun install"),
        ("bun.lockb", "bun install"),
    ];
    let package_root = package_json.parent()?;
    let mut hint = "npm install".to_string();
    for (lockfile, lock_hint) in lockfiles {
        let path = package_root.join(lockfile);
        if is_regular_file(&path) {
            marker_paths.push(path);
            evidence.push(lockfile.to_string());
            hint = lock_hint.to_string();
            break;
        }
    }
    Some(ArtifactMatch {
        ecosystem: DeveloperEcosystem::Node,
        kind: DeveloperArtifactKind::NodeModules,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from("node_modules"),
        marker_paths,
        evidence,
        rebuild_hint: Some(hint),
    })
}

pub(super) fn recognize_python(project_root: &Path, child_name: &str) -> Option<ArtifactMatch> {
    let names = [
        "pyproject.toml",
        "uv.lock",
        "poetry.lock",
        "requirements.txt",
        "Pipfile.lock",
    ];
    let marker = find_named_marker(project_root, &names)?;
    if !is_regular_file(&project_root.join(child_name).join("pyvenv.cfg")) {
        return None;
    }
    let marker_name = marker.file_name()?.to_string_lossy();
    let hint = match marker_name.as_ref() {
        "uv.lock" => "uv sync",
        "poetry.lock" => "poetry install",
        "requirements.txt" => "python -m pip install -r requirements.txt",
        "Pipfile.lock" => "pipenv sync",
        _ => "python -m pip install -e .",
    };
    Some(ArtifactMatch {
        ecosystem: DeveloperEcosystem::Python,
        kind: DeveloperArtifactKind::PythonVenv,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from(child_name),
        marker_paths: vec![marker.clone()],
        evidence: vec![marker_name.into_owned()],
        rebuild_hint: Some(hint.to_string()),
    })
}

pub(super) fn recognize_vendor(project_root: &Path) -> Option<ArtifactMatch> {
    if let Some(marker) = find_named_marker(project_root, &["composer.json"]) {
        let vendor = project_root.join("vendor");
        let installed = vendor.join("composer/installed.json");
        let autoload = vendor.join("autoload.php");
        let generated_evidence = if is_regular_file(&installed) {
            installed
        } else if is_regular_file(&autoload) {
            autoload
        } else {
            return None;
        };
        let mut marker_paths = vec![marker];
        let lock = project_root.join("composer.lock");
        let mut evidence = vec!["composer.json".to_string()];
        if is_regular_file(&lock) {
            marker_paths.push(lock);
            evidence.push("composer.lock".to_string());
        }
        marker_paths.push(generated_evidence.clone());
        evidence.push(
            generated_evidence
                .strip_prefix(project_root)
                .unwrap_or(&generated_evidence)
                .to_string_lossy()
                .into_owned(),
        );
        return Some(ArtifactMatch {
            ecosystem: DeveloperEcosystem::Php,
            kind: DeveloperArtifactKind::ComposerVendor,
            project_root: project_root.to_path_buf(),
            artifact_relative: PathBuf::from("vendor"),
            marker_paths,
            evidence,
            rebuild_hint: Some("composer install".to_string()),
        });
    }

    let marker = find_named_marker(project_root, &["Gemfile"])?;
    let bundle = project_root.join("vendor/bundle");
    if !bundle.is_dir() {
        return None;
    }
    let mut marker_paths = vec![marker];
    let lock = project_root.join("Gemfile.lock");
    let mut evidence = vec!["Gemfile".to_string()];
    if is_regular_file(&lock) {
        marker_paths.push(lock);
        evidence.push("Gemfile.lock".to_string());
    }
    Some(ArtifactMatch {
        ecosystem: DeveloperEcosystem::Ruby,
        kind: DeveloperArtifactKind::RubyBundle,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from("vendor/bundle"),
        marker_paths,
        evidence,
        rebuild_hint: Some("bundle install".to_string()),
    })
}

pub(super) fn recognize_build(project_root: &Path, child_name: &str) -> Option<ArtifactMatch> {
    let gradle = find_named_marker(
        project_root,
        &[
            "build.gradle.kts",
            "build.gradle",
            "settings.gradle.kts",
            "settings.gradle",
            "gradlew",
        ],
    );
    if let Some(marker) = gradle {
        let is_kotlin = marker
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".kts"));
        return Some(ArtifactMatch {
            ecosystem: if is_kotlin {
                DeveloperEcosystem::Kotlin
            } else {
                DeveloperEcosystem::Java
            },
            kind: DeveloperArtifactKind::GradleBuild,
            project_root: project_root.to_path_buf(),
            artifact_relative: PathBuf::from(child_name),
            marker_paths: vec![marker.clone()],
            evidence: vec![marker.file_name()?.to_string_lossy().into_owned()],
            rebuild_hint: Some("./gradlew build".to_string()),
        });
    }
    let marker = find_named_marker(project_root, &["CMakeLists.txt"])?;
    let cache = project_root.join(child_name).join("CMakeCache.txt");
    if !is_regular_file(&cache) {
        return None;
    }
    Some(ArtifactMatch {
        ecosystem: native_cmake_ecosystem(project_root),
        kind: DeveloperArtifactKind::CMakeBuild,
        project_root: project_root.to_path_buf(),
        artifact_relative: PathBuf::from(child_name),
        marker_paths: vec![marker, cache],
        evidence: vec![
            "CMakeLists.txt".to_string(),
            "build/CMakeCache.txt".to_string(),
        ],
        rebuild_hint: Some("cmake --build build".to_string()),
    })
}

pub(super) fn native_cmake_ecosystem(project_root: &Path) -> DeveloperEcosystem {
    let Ok(entries) = fs::read_dir(project_root) else {
        return DeveloperEcosystem::Cpp;
    };
    let mut has_c = false;
    for path in entries.flatten().map(|entry| entry.path()) {
        match path.extension().and_then(|value| value.to_str()) {
            Some("cc" | "cpp" | "cxx" | "mm") => return DeveloperEcosystem::Cpp,
            Some("c") => has_c = true,
            _ => {}
        }
    }
    if has_c {
        DeveloperEcosystem::C
    } else {
        DeveloperEcosystem::Cpp
    }
}

pub(super) fn find_named_marker(project_root: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| project_root.join(name))
        .find(|path| is_regular_file(path))
}

pub(super) fn find_project_extension_marker(
    project_root: &Path,
    extensions: &[&str],
) -> Option<PathBuf> {
    if extensions.is_empty() {
        return None;
    }
    let entries = fs::read_dir(project_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_regular_file(&path) {
            continue;
        }
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            return Some(path);
        }
    }
    None
}

pub(super) fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}
