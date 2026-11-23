//! One-button Clean activity adapted onto a Tauri `Channel` (#500).

use tauri::ipc::Channel;

use crate::models::OneButtonActivity;
use crate::services::ports::{Activity, ActivityPort};

/// The window's rendering of the use case's activity port.
pub struct TauriOneButtonActivity {
    channel: Channel<OneButtonActivity>,
}

impl TauriOneButtonActivity {
    pub fn new(channel: Channel<OneButtonActivity>) -> Self {
        Self { channel }
    }
}

impl ActivityPort for TauriOneButtonActivity {
    fn report(&self, activity: Activity) {
        let _ = self.channel.send(activity.into());
    }
}
