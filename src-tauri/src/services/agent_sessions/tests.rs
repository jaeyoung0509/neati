//! The session use case through its port, in a fixture home (#508, #509).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;

use super::desk::SessionDesk;
use super::ports::{SessionLibrary, SessionSelection};
use super::{resolve_selection, AgentSessionLibrary};
use crate::agent_sessions::inventory::{DiscoveredSession, SessionFile};
use crate::agent_sessions::picker::BundlePicker;
use crate::agent_sessions::SessionCancel;
use crate::models::{AgentSessionTool, PlatformKind};

const ID: &str = "0b5c7e2a-1111-4c3e-9a1b-2c3d4e5f6a7b";

fn environment(home: &Path) -> Arc<PlatformEnvironment> {
    Arc::new(
        PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_home(home),
    )
}

fn home_with_session(secret: bool) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(".claude/projects/-work-app");
    std::fs::create_dir_all(&folder).unwrap();
    let extra = if secret {
        format!(
            " token {}",
            crate::agent_sessions::secrets::fake_token("ghp_", 36)
        )
    } else {
        String::new()
    };
    std::fs::write(
        folder.join(format!("{ID}.jsonl")),
        format!(
            "{{\"type\":\"user\",\"cwd\":\"/work/app\",\"sessionId\":\"{ID}\",\"timestamp\":\"2026-11-20T09:00:00Z\",\"message\":{{\"role\":\"user\",\"content\":\"hello{extra}\"}}}}\n"
        ),
    )
    .unwrap();
    home
}

struct FakePicker {
    destination: Mutex<Option<PathBuf>>,
    bundle: Mutex<Option<PathBuf>>,
}

impl BundlePicker for FakePicker {
    fn export_destination(&self) -> Result<Option<PathBuf>, String> {
        Ok(self.destination.lock().unwrap().clone())
    }
    fn bundle_to_import(&self) -> Result<Option<PathBuf>, String> {
        Ok(self.bundle.lock().unwrap().clone())
    }
    fn project_folder(&self) -> Result<Option<PathBuf>, String> {
        Ok(Some(PathBuf::from("/work/new/")))
    }
}

fn desk(home: &Path) -> (SessionDesk, Arc<FakePicker>) {
    let picker = Arc::new(FakePicker {
        destination: Mutex::new(None),
        bundle: Mutex::new(None),
    });
    let library: Arc<dyn SessionLibrary> = Arc::new(AgentSessionLibrary::new(environment(home)));
    (SessionDesk::new(library, picker.clone()), picker)
}

fn session(key: &str, id: &str) -> DiscoveredSession {
    DiscoveredSession {
        tool: AgentSessionTool::Codex,
        key: key.to_string(),
        session_id: id.to_string(),
        files: vec![SessionFile {
            path: PathBuf::from("/x"),
            relative: "sessions/x.jsonl".into(),
            size: 1,
        }],
        cwd: None,
        title: None,
        started_at: None,
        last_activity_at: None,
        dates_from_records: false,
        archived: false,
        issue: None,
        claude_folder: None,
    }
}

#[test]
fn references_resolve_by_key_or_unique_id_and_refuse_otherwise() {
    let sessions = vec![session("k1", "a"), session("k2", "b"), session("k3", "b")];
    let only = |references: &[&str]| {
        SessionSelection::Only(
            references
                .iter()
                .map(|reference| reference.to_string())
                .collect(),
        )
    };
    let chosen = resolve_selection(&sessions, &only(&["k1", "k3"])).unwrap();
    assert_eq!(
        chosen.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(),
        ["k1", "k3"]
    );
    assert_eq!(
        resolve_selection(&sessions, &only(&["a"])).unwrap()[0].key,
        "k1"
    );
    assert!(resolve_selection(&sessions, &only(&["b"]))
        .unwrap_err()
        .contains("more than one"));
    assert!(resolve_selection(&sessions, &only(&["k1", "nope"]))
        .unwrap_err()
        .contains("Nothing was exported"));
    assert!(resolve_selection(&sessions, &only(&[])).is_err());
    assert_eq!(
        resolve_selection(&sessions, &SessionSelection::All)
            .unwrap()
            .len(),
        3
    );
    assert!(resolve_selection(&[], &SessionSelection::All).is_err());
}

#[test]
fn the_library_reads_the_stated_home_only() {
    let home = home_with_session(false);
    let library = AgentSessionLibrary::new(environment(home.path()));
    let inventory =
        tauri::async_runtime::block_on(library.inventory(SessionCancel::new())).unwrap();
    assert_eq!(inventory.projects.len(), 1);
    assert_eq!(inventory.projects[0].cwd.as_deref(), Some("/work/app"));
    assert_eq!(
        inventory.projects[0].sessions[0].title.as_deref(),
        Some("hello")
    );

    let homeless =
        AgentSessionLibrary::new(Arc::new(PlatformEnvironment::simulated(PathFlavor::Posix)));
    assert!(tauri::async_runtime::block_on(homeless.inventory(SessionCancel::new())).is_err());
}

#[test]
fn the_window_desk_reads_through_the_port_and_a_stop_without_a_read_is_harmless() {
    let home = home_with_session(false);
    let (desk, _) = desk(home.path());
    desk.cancel();
    let inventory = tauri::async_runtime::block_on(desk.inventory()).unwrap();
    assert_eq!(
        inventory.tools.iter().filter(|tool| tool.present).count(),
        1
    );
    assert!(!inventory.cancelled);
    assert!(
        desk.is_idle(),
        "a finished read leaves no stop signal behind"
    );
}

#[test]
fn the_window_export_needs_confirmation_for_findings_and_survives_a_closed_dialog() {
    let home = home_with_session(true);
    let (desk, picker) = desk(home.path());
    let key = tauri::async_runtime::block_on(desk.inventory())
        .unwrap()
        .projects[0]
        .sessions[0]
        .key
        .clone();
    let preview = tauri::async_runtime::block_on(desk.prepare_export(vec![key])).unwrap();
    assert_eq!(preview.sessions, 1);
    assert_eq!(preview.findings.len(), 1);

    // Findings without confirmation are refused before any dialog opens.
    assert!(tauri::async_runtime::block_on(desk.export(preview.id, false)).is_err());
    // A closed dialog writes nothing and keeps the preview.
    assert_eq!(
        tauri::async_runtime::block_on(desk.export(preview.id, true)).unwrap(),
        None
    );
    let out = tempfile::tempdir().unwrap();
    let destination = out.path().join("s.neati-sessions");
    *picker.destination.lock().unwrap() = Some(destination.clone());
    let outcome = tauri::async_runtime::block_on(desk.export(preview.id, true))
        .unwrap()
        .expect("written");
    assert_eq!(outcome.sessions, 1);
    assert!(destination.is_file());
    // The preview is consumed.
    assert!(tauri::async_runtime::block_on(desk.export(preview.id, true)).is_err());

    // The bundle opens and imports into another home, once.
    let other = tempfile::tempdir().unwrap();
    let (importer, picker) = self::desk(other.path());
    assert_eq!(
        tauri::async_runtime::block_on(importer.open_bundle()).unwrap(),
        None
    );
    *picker.bundle.lock().unwrap() = Some(destination);
    let review = tauri::async_runtime::block_on(importer.open_bundle())
        .unwrap()
        .expect("opened");
    assert_eq!(review.sessions.len(), 1);
    assert_eq!(review.sessions[0].existing_files, 0);
    let imported = tauri::async_runtime::block_on(importer.import(review.id, vec![])).unwrap();
    assert_eq!(imported.written_files, 1);
    assert!(tauri::async_runtime::block_on(importer.import(review.id, vec![])).is_err());
    assert!(
        desk.is_idle() && importer.is_idle(),
        "finished operations leave no stop signal"
    );
    assert_eq!(
        tauri::async_runtime::block_on(importer.pick_project_folder()).unwrap(),
        Some("/work/new".to_string())
    );
    assert!(tauri::async_runtime::block_on(importer.prepare_export(vec![])).is_err());
}
