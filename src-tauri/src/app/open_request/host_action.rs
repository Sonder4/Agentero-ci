//! Local `ui` actions the Host answers itself (screenshot, idle wait).
//!
//! They log the same `op start/end ui_action id=…` lines as renderer actions
//! and answer with `{ "error", "code" }` on failure so the CLI can report a
//! stable `error.code`.

use super::{trunc, write_local_ui_response, UiRequestPayload};
use std::time::Instant;

/// A failed Host action; `code` is part of the CLI contract (docs/backend/cli.md).
#[derive(Debug)]
pub(super) struct HostActionError {
    pub code: &'static str,
    pub message: String,
}

impl HostActionError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Logs the start on creation; [`HostAction::finish`] logs the end and
/// writes the CLI response.
pub(super) struct HostAction {
    id: String,
    action: String,
    window: String,
    started: Instant,
}

impl HostAction {
    pub fn start(req: &UiRequestPayload) -> Option<Self> {
        let id = req.reference.clone()?;
        let action = Self {
            id,
            action: req.action.clone(),
            window: req.window.clone().unwrap_or_else(|| "main".into()),
            started: Instant::now(),
        };
        log::info!(
            target: "agentero::op",
            "op start ui_action id={} action={} window={}",
            action.id,
            action.action,
            trunc(&action.window)
        );
        Some(action)
    }

    pub fn window(&self) -> &str {
        &self.window
    }

    pub fn finish(self, result: Result<serde_json::Value, HostActionError>) {
        let duration_ms = self.started.elapsed().as_millis();
        let response = match &result {
            Ok(value) => {
                log::info!(
                    target: "agentero::op",
                    "op end ui_action ok=true duration_ms={duration_ms} id={} action={} window={}",
                    self.id,
                    self.action,
                    trunc(&self.window)
                );
                value.clone()
            }
            Err(error) => {
                log::warn!(
                    target: "agentero::op",
                    "op end ui_action ok=false duration_ms={duration_ms} id={} action={} window={} code={} error={}",
                    self.id,
                    self.action,
                    trunc(&self.window),
                    error.code,
                    trunc(&error.message)
                );
                serde_json::json!({ "error": error.message, "code": error.code })
            }
        };
        let _ = write_local_ui_response(&self.id, &response.to_string());
    }
}
