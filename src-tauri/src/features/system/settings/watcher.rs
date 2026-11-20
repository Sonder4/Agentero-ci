//! Reload CLI edits into the running desktop Host.

use super::AppSettingsStore;
use tauri::{AppHandle, Emitter, Manager};

const POLL_INTERVAL_MS: u64 = 400;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
            let result = app.state::<AppSettingsStore>().reload_if_changed();
            match result {
                Ok(Some(settings)) => {
                    let _ = app.emit("settings:changed", settings);
                }
                Ok(None) => {}
                Err(error) => {
                    log::warn!(
                        target: "agentero::settings",
                        "external settings reload failed: {error}"
                    );
                }
            }
        }
    });
}
