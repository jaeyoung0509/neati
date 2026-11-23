//! The session use case through its port, in a fixture home (#508).

use std::path::Path;
use std::sync::Arc;

use neati_platform::path_algebra::PathFlavor;
use neati_platform::PlatformEnvironment;

use super::desk::SessionDesk;
use super::ports::SessionLibrary;
use super::AgentSessionLibrary;
use crate::agent_sessions::SessionCancel;
use crate::models::PlatformKind;

const ID: &str = "0b5c7e2a-1111-4c3e-9a1b-2c3d4e5f6a7b";

fn environment(home: &Path) -> Arc<PlatformEnvironment> {
    Arc::new(
        PlatformEnvironment::simulated(PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_home(home),
    )
}

fn home_with_session() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join(".claude/projects/-work-app");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{ID}.jsonl")),
        format!(
            "{{\"type\":\"user\",\"cwd\":\"/work/app\",\"sessionId\":\"{ID}\",\"timestamp\":\"2026-11-20T09:00:00Z\",\"message\":{{\"role\":\"user\",\"content\":\"hello\"}}}}\n"
        ),
    )
    .unwrap();
    home
}

#[test]
fn the_library_reads_the_stated_home_only() {
    let home = home_with_session();
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
    let home = home_with_session();
    let library: Arc<dyn SessionLibrary> =
        Arc::new(AgentSessionLibrary::new(environment(home.path())));
    let desk = SessionDesk::new(library);
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
