//! Wake the mounted dashboard; the one-shot backend mailbox owns the route.
pub fn notify<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Emitter;
    let _ = app.emit_to("main", "dashboard-navigation-pending", ());
}
