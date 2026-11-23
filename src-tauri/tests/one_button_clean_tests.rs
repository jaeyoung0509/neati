//! One-button Clean through the composed ports, against a fixture home (#498).
//!
//! `composition::application_ports` binds the use case to the real services,
//! so these runs exercise the scan, the backend's selection, the reviewed
//! quick-clean intent and its execution guard end to end, inside a temporary
//! home. No real user directory is read for removal or touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use neati_lib::composition::{application_ports, desktop_state, ApplicationPorts};
use neati_lib::docker::adapter::ContainerHost;
use neati_lib::models::{NeatiSettings, PlatformKind};
use neati_lib::services::one_button_clean::ports::{CleanPreview, CleanRequest, OneButtonError};
use neati_lib::services::ports::Silent;
use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;

const STEAM: &str = "Library/Application Support/Steam/config/htmlcache/Default";

struct Fixture {
    _home: tempfile::TempDir,
    root: PathBuf,
    ports: ApplicationPorts,
}

impl Fixture {
    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(STEAM).join(relative)
    }

    fn preview(&self, request: CleanRequest) -> Result<CleanPreview, OneButtonError> {
        tauri::async_runtime::block_on(self.ports.clean.preview(request, Arc::new(Silent)))
    }
}

/// Steam's embedded browser caches, old enough for every age gate, beside the
/// browser's cookies, which are user state and must survive every run.
fn fixture() -> Fixture {
    let home = tempfile::tempdir().expect("fixture home");
    let root = fs::canonicalize(home.path()).expect("canonical fixture home");
    for relative in [
        "Cache/Cache_Data/000208583613f92a_0",
        "Code Cache/js/0003238a4bea7d7d_0",
        "GPUCache/index",
        "Cookies",
    ] {
        let path = root.join(STEAM).join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("fixture dir");
        fs::write(&path, vec![0xA5u8; 64 * 1024]).expect("fixture file");
    }
    age(
        &root,
        SystemTime::now() - Duration::from_secs(120 * 24 * 60 * 60),
    );
    let temp = root.join("private-tmp");
    fs::create_dir_all(&temp).expect("fixture temp");

    let mut environment = PlatformEnvironment::simulated(PathFlavor::Posix)
        .with_platform(PlatformKind::Macos)
        .with_home(&root)
        .with_temp_dir(&temp);
    for tool in [
        "docker", "npm", "pnpm", "uv", "brew", "cargo", "go", "pip", "pod", "xcrun",
    ] {
        environment = environment.with_missing_tool(tool);
    }
    let state = desktop_state(Arc::new(environment), ContainerHost::from_value(None));
    state
        .settings
        .replace(NeatiSettings::default().sanitize())
        .expect("fixture settings");
    Fixture {
        _home: home,
        root,
        ports: application_ports(&state),
    }
}

fn age(root: &Path, when: SystemTime) {
    let mut stack = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(entry.path());
            } else {
                entries.push(entry.path());
            }
        }
        entries.push(directory);
    }
    for entry in entries {
        if let Ok(handle) = fs::File::open(&entry) {
            let _ = handle.set_modified(when);
        }
    }
}

fn caches_only() -> CleanRequest {
    CleanRequest {
        projects: false,
        ..CleanRequest::default()
    }
}

// The open-file checks that authorize these caches have a macOS adapter only;
// elsewhere the unknown state rightly refuses them.
#[cfg(target_os = "macos")]
#[test]
fn a_preview_names_each_embedded_cache_and_removes_nothing() {
    let fixture = fixture();
    let preview = fixture.preview(caches_only()).expect("preview");
    let steam: Vec<_> = preview
        .chosen
        .iter()
        .filter(|candidate| candidate.reference.starts_with("system.steam.web_cache"))
        .collect();
    assert_eq!(steam.len(), 3, "each embedded browser cache is one unit");
    assert!(preview.total_bytes >= 3 * 64 * 1024);
    for relative in ["Cache", "Code Cache", "GPUCache", "Cookies"] {
        assert!(fixture.path(relative).exists());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn executing_a_narrowed_preview_removes_exactly_that_unit() {
    let fixture = fixture();
    let gpu = fixture
        .preview(caches_only())
        .expect("preview")
        .chosen
        .into_iter()
        .find(|candidate| candidate.path.ends_with("/GPUCache"))
        .expect("GPU cache offered")
        .reference;

    let narrowed = fixture
        .preview(CleanRequest {
            only: vec![gpu],
            ..caches_only()
        })
        .expect("narrowed preview");
    assert_eq!(narrowed.chosen.len(), 1);
    let outcome =
        tauri::async_runtime::block_on(fixture.ports.clean.execute(&narrowed, Arc::new(Silent)))
            .expect("removal");
    assert!(
        outcome.failed.is_empty(),
        "nothing failed: {:?}",
        outcome.failed
    );
    assert_eq!(outcome.removed, 1);

    assert!(!fixture.path("GPUCache/index").exists());
    assert!(fixture.path("Cache/Cache_Data/000208583613f92a_0").exists());
    assert!(fixture.path("Code Cache/js/0003238a4bea7d7d_0").exists());
    assert!(fixture.path("Cookies").exists());
}

#[test]
fn a_path_or_unknown_reference_refuses_before_anything_is_removed() {
    let fixture = fixture();
    let cookies = fixture.path("Cookies");
    for reference in [
        cookies.to_string_lossy().into_owned(),
        "system.nope".to_string(),
    ] {
        let refused = fixture.preview(CleanRequest {
            only: vec![reference.clone()],
            ..caches_only()
        });
        assert_eq!(
            refused.expect_err("refused"),
            OneButtonError::UnknownReferences(vec![reference])
        );
    }
    assert!(cookies.exists());
    assert!(fixture.path("GPUCache/index").exists());
}

#[test]
fn without_project_folders_no_build_output_is_offered() {
    let fixture = fixture();
    let preview = fixture
        .preview(CleanRequest {
            caches: false,
            ..CleanRequest::default()
        })
        .expect("preview");
    assert!(preview.chosen.is_empty());
    assert!(preview
        .projects
        .expect("projects were asked for")
        .folders
        .is_empty());
}
