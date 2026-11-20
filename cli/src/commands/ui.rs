//! `agentero ui *` — workspace / window management from the CLI.
//!
//! Mirrors the desktop workspace primitives (open-or-focus a document or a
//! native child window, close a document) so scripts and external agents can
//! drive the running App without touching its internals.

use crate::error::CliError;
use crate::resolve::{resolve_vault, GlobalOpts};
use clap::{Subcommand, ValueHint};
use serde_json::Value;
use std::time::{Duration, Instant};

use agentero_core::features::open_request::{write_local_ui_request, UiRequestPayload};

/// Window ids `agentero ui window` may target (mirrors the desktop frontend's
/// openable windows).
const UI_WINDOWS: &[&str] = &["settings", "agent", "annotations"];

#[derive(Debug, Subcommand)]
pub enum UiCmd {
    /// List actionable controls in a running Agentero window.
    Actions {
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Invoke a global shortcut by its stable id.
    Shortcut {
        id: String,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Run a command by its command palette id.
    Command {
        id: String,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Click a visible control by action id or label (see ui actions).
    Click {
        label: String,
        #[arg(long)]
        occurrence: Option<usize>,
        /// Open the control's in-app context menu.
        #[arg(long)]
        right: bool,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Dispatch a key chord to the focused Agentero control, e.g. Ctrl+f.
    Key {
        chord: String,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Replace text in the focused Agentero input or textarea.
    Fill {
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Open (or focus) a Vault-relative document / folder / paper in the workspace.
    Open {
        /// Vault-relative path (e.g. `notes/idea.md`, `papers/demo`).
        #[arg(value_hint = ValueHint::DirPath)]
        path: String,
    },
    /// Close the workspace panel(s) for a Vault-relative path.
    Close {
        /// Vault-relative path (e.g. `notes/idea.md`, `papers/demo`).
        #[arg(value_hint = ValueHint::DirPath)]
        path: String,
    },
    /// Open (or focus) a native child window.
    ///
    /// `view` is `settings` or a right-rail feature: agent / annotations.
    Window {
        /// Window id (`settings`, `agent`, `annotations`).
        view: String,
        /// Settings section (only meaningful for `settings`).
        #[arg(long = "section")]
        section: Option<String>,
    },
    /// Split the workspace: open a path beside a reference panel, or split the
    /// active pane when no path is given (⌘\ semantics).
    Split {
        /// Vault-relative path to open beside the reference (omit to split the
        /// active pane).
        #[arg(value_hint = ValueHint::DirPath)]
        path: Option<String>,
        /// Split direction: right (default) | left | above | below | within.
        /// `up`/`down` are accepted as aliases for above/below.
        #[arg(long = "direction", value_name = "DIR")]
        direction: Option<String>,
        /// Reference panel path (default: the active pane).
        #[arg(long = "reference", value_name = "PATH")]
        reference: Option<String>,
    },
}

pub fn run(cmd: UiCmd, globals: &GlobalOpts) -> Result<Value, CliError> {
    match cmd {
        UiCmd::Actions { window } => run_local("actions", None, window, None, false),
        UiCmd::Shortcut { id, window } => run_local("shortcut", Some(id), window, None, false),
        UiCmd::Command { id, window } => run_local("command", Some(id), window, None, false),
        UiCmd::Click {
            label,
            occurrence,
            right,
            window,
        } => run_local("click", Some(label), window, occurrence, right),
        UiCmd::Key { chord, window } => run_local("key", Some(chord), window, None, false),
        UiCmd::Fill { value, window } => run_local("fill", Some(value), window, None, false),
        UiCmd::Open { path } => {
            let vault = resolve_vault(globals)?;
            crate::commands::open::open_path_in_app(&vault, &path, globals)
        }
        UiCmd::Close { path } => {
            let vault = resolve_vault(globals)?;
            crate::commands::open::close_path_in_app(&vault, &path, globals)
        }
        UiCmd::Window { view, section } => {
            let view = view.trim().to_ascii_lowercase();
            if !UI_WINDOWS.contains(&view.as_str()) {
                return Err(CliError::usage(format!(
                    "unknown window: {view} (expected one of {})",
                    UI_WINDOWS.join(", ")
                )));
            }
            crate::commands::open::open_window_in_app(&view, section, globals)
        }
        UiCmd::Split {
            path,
            direction,
            reference,
        } => {
            let vault = resolve_vault(globals)?;
            crate::commands::open::split_in_app(
                &vault,
                path.as_deref(),
                reference.as_deref(),
                direction,
                globals,
            )
        }
    }
}

fn run_local(
    action: &str,
    target: Option<String>,
    window: String,
    occurrence: Option<usize>,
    right: bool,
) -> Result<Value, CliError> {
    let mut request = UiRequestPayload::local_action(action, target, window, occurrence);
    request.direction = right.then(|| "right".to_owned());
    let path =
        write_local_ui_request(&request).map_err(|error| CliError::usage(error.to_string()))?;
    let response_path = path.with_extension("response");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match std::fs::read_to_string(&response_path) {
            Ok(body) => {
                let _ = std::fs::remove_file(&response_path);
                let response: Value = serde_json::from_str(&body)
                    .map_err(|error| CliError::message(error.to_string()))?;
                if let Some(error) = response.get("error").and_then(Value::as_str) {
                    return Err(CliError::message(error.to_owned()));
                }
                return Ok(response);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(CliError::message(error.to_string())),
        }
    }
    let _ = std::fs::remove_file(&path);
    Err(CliError::message(
        "Agentero did not acknowledge the action. Check that the requested window is open.",
    ))
}
