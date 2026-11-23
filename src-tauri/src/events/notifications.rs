//! Native notifications adapted onto the Tauri notification plugin.

use tauri::AppHandle;

use crate::agent_activity::notifications::NotificationFilter;
use crate::models::{AgentActivitySnapshot, AgentNotificationPreferences, Recommendation};
use crate::services::desktop_notifications::DesktopNotifications;

/// Delivers notifications through the plugin registered by the desktop shell.
pub struct TauriNotifications {
    app: AppHandle,
}

impl TauriNotifications {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl DesktopNotifications for TauriNotifications {
    fn request_permission(&self) -> Result<(), String> {
        crate::ai_control_center::notifications::request_permission_if_needed(&self.app)
    }

    fn emit_recommendations(&self, recommendations: &[Recommendation]) -> Vec<String> {
        crate::ai_control_center::notifications::emit_advisories(&self.app, recommendations)
    }

    fn emit_process_advisories(
        &self,
        snapshot: &AgentActivitySnapshot,
        preferences: &AgentNotificationPreferences,
        filter: &mut NotificationFilter,
    ) -> Vec<String> {
        crate::agent_activity::notifications::emit_process_advisories(
            &self.app,
            snapshot,
            preferences,
            filter,
        )
    }
}

/// What automatic Clean (#507) tells the person, through the same plugin.
pub struct TauriAutomaticCleanNotifier {
    app: AppHandle,
}

impl TauriAutomaticCleanNotifier {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl crate::automatic_clean::Notifier for TauriAutomaticCleanNotifier {
    fn notify(&self, title: &str, body: &str) {
        use tauri_plugin_notification::{NotificationExt, PermissionState};
        let notification = self.app.notification();
        if notification.permission_state().ok() != Some(PermissionState::Granted) {
            return;
        }
        if let Err(error) = notification.builder().title(title).body(body).show() {
            crate::diagnostics::log_error(
                "automatic-clean",
                &format!("Native notification failed: {error}"),
            );
        }
    }
}
