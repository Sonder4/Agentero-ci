//! Reload `agents.json` edits made by the headless CLI.

use super::AgentRegistry;
use tauri::{AppHandle, Manager};

const POLL_INTERVAL_MS: u64 = 400;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
            match app.state::<AgentRegistry>().reload_if_changed() {
                Ok(true) => crate::features::agent::service::emit_registry_changed(&app),
                Ok(false) => {}
                Err(error) => {
                    log::warn!(
                        target: "agentero::agent",
                        "external agents.json reload failed: {error}"
                    );
                }
            }
        }
    });
}
