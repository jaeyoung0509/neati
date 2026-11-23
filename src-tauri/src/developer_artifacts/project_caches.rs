//! Project-local caches that kondo and npkill find (#479).
//!
//! Each rule needs direct project evidence: a marker in the folder that holds
//! the cache, or, for the Python tool caches, a valid Cache Directory Tagging
//! Standard tag inside the cache itself. Git-tracked descendants still revoke
//! authority through the shared ownership probe, and a recognized folder is
//! removed as one whole unit. None of these kinds is a one-button build output
//! (`preselection::is_build_output`): they stay reviewable in the Projects
//! view, because removing one costs a re-download, a re-import or a cold tool
//! cache.
//!
//! Deliberately absent: `.ipynb_checkpoints` (the only recovery copy of a
//! notebook), Unreal `Saved` (project settings, logs and autosaves) and Unity
//! `Assets`/`ProjectSettings` (the project itself). `.dart_tool` with
//! `pubspec.yaml` is the existing `FlutterTooling` rule, so it is not repeated.
use super::rules::{Recognition, Rule};
use super::*;
use DeveloperArtifactKind as Kind;
use DeveloperEcosystem as Ecosystem;

/// A cache folder recognized by a marker file beside it.
const fn marker(
    directory: &'static str,
    kind: DeveloperArtifactKind,
    names: &'static [&'static str],
    extensions: &'static [&'static str],
    ecosystem: DeveloperEcosystem,
    hint: &'static str,
) -> Rule {
    Rule {
        discovery_name: Some(directory),
        relative: directory,
        kind,
        recognition: Recognition::Marker {
            names,
            extensions,
            ecosystem,
            hint,
        },
    }
}

/// A cache folder whose evidence needs more than one marker file.
const fn custom(
    directory: &'static str,
    kind: DeveloperArtifactKind,
    recognize: fn(&Path, &str) -> Option<ArtifactMatch>,
) -> Rule {
    Rule {
        discovery_name: Some(directory),
        relative: directory,
        kind,
        recognition: Recognition::Custom(recognize),
    }
}

pub(super) const RULES: &[Rule] = &[
    custom(
        "__pycache__",
        Kind::PythonBytecodeCache,
        recognize_python_cache,
    ),
    custom(".pytest_cache", Kind::PytestCache, recognize_python_cache),
    custom(".mypy_cache", Kind::MypyCache, recognize_python_cache),
    custom(".ruff_cache", Kind::RuffCache, recognize_python_cache),
    marker(
        ".tox",
        Kind::ToxEnvironments,
        &["tox.ini"],
        &[],
        Ecosystem::Python,
        "tox -r",
    ),
    marker(
        ".nox",
        Kind::NoxEnvironments,
        &["noxfile.py"],
        &[],
        Ecosystem::Python,
        "nox",
    ),
    marker(
        ".turbo",
        Kind::TurboCache,
        &["turbo.json"],
        &[],
        Ecosystem::Node,
        "turbo run build",
    ),
    marker(
        ".angular",
        Kind::AngularCache,
        &["angular.json"],
        &[],
        Ecosystem::Node,
        "ng build",
    ),
    marker(
        ".nx",
        Kind::NxCache,
        &["nx.json"],
        &[],
        Ecosystem::Node,
        "nx run-many -t build",
    ),
    custom("Library", Kind::UnityLibrary, recognize_unity),
    custom("Temp", Kind::UnityTemp, recognize_unity),
    custom("Obj", Kind::UnityObj, recognize_unity),
    custom("Logs", Kind::UnityLogs, recognize_unity),
    marker(
        "Intermediate",
        Kind::UnrealIntermediate,
        &[],
        &["uproject"],
        Ecosystem::Unreal,
        "Open the project in Unreal Editor to rebuild it",
    ),
    marker(
        "DerivedDataCache",
        Kind::UnrealDerivedDataCache,
        &[],
        &["uproject"],
        Ecosystem::Unreal,
        "Open the project in Unreal Editor to rebuild it",
    ),
    marker(
        ".godot",
        Kind::GodotCache,
        &["project.godot"],
        &[],
        Ecosystem::Godot,
        "Open the project in Godot to re-import its assets",
    ),
    marker(
        "elm-stuff",
        Kind::ElmStuff,
        &["elm.json"],
        &[],
        Ecosystem::Elm,
        "elm make",
    ),
    marker(
        ".cpcache",
        Kind::ClojureCpcache,
        &["deps.edn"],
        &[],
        Ecosystem::Clojure,
        "clojure -P",
    ),
    marker(
        ".bloop",
        Kind::BloopCache,
        &["build.sbt"],
        &[],
        Ecosystem::Scala,
        "sbt bloopInstall",
    ),
    marker(
        ".metals",
        Kind::MetalsCache,
        &["build.sbt"],
        &[],
        Ecosystem::Scala,
        "Metals re-indexes the build on the next import",
    ),
];

/// Names generic enough that a folder outside any project may carry them. The
/// walk still descends into one no rule recognized, so a nested project inside
/// a folder named `Library` or `Logs` is not hidden.
pub(super) const TRAVERSE_UNLESS_RECOGNIZED: &[&str] = &[
    "Library",
    "Temp",
    "Obj",
    "Logs",
    "Intermediate",
    "DerivedDataCache",
];

/// The tag file the Cache Directory Tagging Standard places in a cache.
pub(super) const CACHE_DIRECTORY_TAG: &str = "CACHEDIR.TAG";

/// The exact first line the standard requires; anything else is not a tag.
const CACHE_DIRECTORY_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Whether `marker` is the tag inside `artifact` that evidences a Python tool
/// cache. The removal scope check accepts exactly that file in place of a
/// marker beside the artifact; every other kind needs a marker beside it.
pub(crate) fn is_inner_cache_tag(
    kind: DeveloperArtifactKind,
    artifact: &Path,
    marker: &Path,
) -> bool {
    matches!(
        kind,
        Kind::PythonBytecodeCache | Kind::PytestCache | Kind::MypyCache | Kind::RuffCache
    ) && marker == artifact.join(CACHE_DIRECTORY_TAG)
}

/// Python tool caches: a project file beside the cache, or a valid tag inside
/// it (pytest, mypy and Ruff write one).
fn recognize_python_cache(root: &Path, name: &str) -> Option<ArtifactMatch> {
    let (kind, hint) = match name {
        "__pycache__" => (
            Kind::PythonBytecodeCache,
            "Python recompiles bytecode on the next import",
        ),
        ".pytest_cache" => (
            Kind::PytestCache,
            "pytest (the last-failed record starts over)",
        ),
        ".mypy_cache" => (Kind::MypyCache, "mypy ."),
        ".ruff_cache" => (Kind::RuffCache, "ruff check ."),
        _ => return None,
    };
    let (marker, evidence) =
        match find_named_marker(root, &["pyproject.toml", "setup.py", "setup.cfg"]) {
            Some(marker) => {
                let evidence = marker.file_name()?.to_string_lossy().into_owned();
                (marker, evidence)
            }
            None => {
                let tag = root.join(name).join(CACHE_DIRECTORY_TAG);
                if !has_cache_directory_signature(&tag) {
                    return None;
                }
                (tag, format!("{name}/{CACHE_DIRECTORY_TAG}"))
            }
        };
    Some(ArtifactMatch {
        ecosystem: Ecosystem::Python,
        kind,
        project_root: root.to_path_buf(),
        artifact_relative: PathBuf::from(name),
        marker_paths: vec![marker],
        evidence: vec![evidence],
        rebuild_hint: Some(hint.to_string()),
    })
}

/// Unity's generated folders, only beside both `Assets` and `ProjectSettings`.
fn recognize_unity(root: &Path, name: &str) -> Option<ArtifactMatch> {
    let kind = match name {
        "Library" => Kind::UnityLibrary,
        "Temp" => Kind::UnityTemp,
        "Obj" => Kind::UnityObj,
        "Logs" => Kind::UnityLogs,
        _ => return None,
    };
    let markers = ["Assets", "ProjectSettings"].map(|marker| root.join(marker));
    if !markers.iter().all(|marker| is_real_directory(marker)) {
        return None;
    }
    Some(ArtifactMatch {
        ecosystem: Ecosystem::Unity,
        kind,
        project_root: root.to_path_buf(),
        artifact_relative: PathBuf::from(name),
        marker_paths: markers.to_vec(),
        evidence: vec!["Assets/".into(), "ProjectSettings/".into()],
        rebuild_hint: Some("Open the project in Unity to re-import it".into()),
    })
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

/// Reads at most the signature's length from a regular, unlinked file.
fn has_cache_directory_signature(path: &Path) -> bool {
    use std::io::Read;
    if !is_regular_file(path) {
        return false;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let Ok(file) = options.open(path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let mut prefix = Vec::with_capacity(CACHE_DIRECTORY_SIGNATURE.len());
    file.take(CACHE_DIRECTORY_SIGNATURE.len() as u64)
        .read_to_end(&mut prefix)
        .is_ok()
        && prefix == CACHE_DIRECTORY_SIGNATURE
}
