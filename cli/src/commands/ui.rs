//! `agentero ui *` — workspace / window management from the CLI.
//!
//! Mirrors the desktop workspace primitives (open-or-focus a document or a
//! native child window, close a document) so scripts and external agents can
//! drive the running App without touching its internals.

use crate::error::CliError;
use crate::resolve::{resolve_vault, GlobalOpts};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::{Subcommand, ValueEnum, ValueHint};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agentero_core::features::open_request::{
    write_local_ui_request, UiRequestPayload, UI_ACTION_CHATGPT,
};

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
    /// Operate the signed-in ChatGPT website in the open Agentero sidebar.
    Chatgpt {
        #[arg(value_enum)]
        action: ChatGptAction,
        /// Relative ChatGPT link returned by the list commands.
        #[arg(long)]
        target: Option<String>,
        /// New name, instructions, project link, or message.
        #[arg(long, allow_hyphen_values = true)]
        value: Option<String>,
        /// Local file for project-source.
        #[arg(long, value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
        /// Create a new JSON file from export.
        #[arg(long, value_hint = ValueHint::FilePath)]
        out: Option<PathBuf>,
        /// Replace only this exact existing draft before sending.
        #[arg(long, requires = "value", allow_hyphen_values = true)]
        replace_draft: Option<String>,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ChatGptAction {
    Status,
    Projects,
    Recent,
    ProjectChats,
    ProjectSources,
    Open,
    NewChat,
    Send,
    RenameChat,
    PinChat,
    MoveChat,
    ArchiveChat,
    ShareChat,
    CreateProject,
    RenameProject,
    ProjectInstructions,
    ProjectSource,
    Export,
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
        UiCmd::Chatgpt {
            action,
            target,
            value,
            file,
            out,
            replace_draft,
        } => run_chatgpt(
            action,
            target,
            value,
            file.as_deref(),
            out.as_deref(),
            replace_draft,
        ),
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
    request_local(request, Duration::from_secs(10))
}

fn run_chatgpt(
    action: ChatGptAction,
    target: Option<String>,
    value: Option<String>,
    file: Option<&Path>,
    out: Option<&Path>,
    replace_draft: Option<String>,
) -> Result<Value, CliError> {
    if file.is_some() != matches!(action, ChatGptAction::ProjectSource) {
        return Err(CliError::usage(
            "--file is required only for project-source",
        ));
    }
    if out.is_some() && !matches!(action, ChatGptAction::Export) {
        return Err(CliError::usage("--out is available only for export"));
    }
    if replace_draft.is_some() && !matches!(action, ChatGptAction::Send) {
        return Err(CliError::usage(
            "--replace-draft is available only for send",
        ));
    }
    if out.is_some_and(Path::exists) {
        return Err(CliError::usage("The output file already exists"));
    }
    let file = file
        .map(|path| -> Result<Value, CliError> {
            const MAX_BYTES: u64 = 16 * 1024 * 1024;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| CliError::usage("A UTF-8 file name is required"))?;
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
                .map_err(|error| CliError::message(error.to_string()))?;
            if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
                return Err(CliError::usage(
                    "A nonempty project file of at most 16 MiB is required",
                ));
            }
            Ok(serde_json::json!({ "name": name, "base64": STANDARD.encode(bytes) }))
        })
        .transpose()?;
    let operation = action.to_possible_value().expect("named ChatGPT action");
    let data = serde_json::json!({
        "operation": operation.get_name(), "target": target, "value": value, "file": file,
        "replaceDraft": replace_draft,
    });
    let request = UiRequestPayload::local_action(
        UI_ACTION_CHATGPT,
        Some(data.to_string()),
        "main".into(),
        None,
    );
    let result = request_local(request, Duration::from_secs(57))?;
    if let Some(path) = out {
        let directory = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .map_err(|error| CliError::message(error.to_string()))?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), &result)
            .map_err(|error| CliError::message(error.to_string()))?;
        temporary
            .write_all(b"\n")
            .map_err(|error| CliError::message(error.to_string()))?;
        temporary
            .persist_noclobber(path)
            .map_err(|error| CliError::message(error.to_string()))?;
        return Ok(
            serde_json::json!({ "path": path, "messages": result["messages"].as_array().map_or(0, Vec::len) }),
        );
    }
    Ok(result)
}

fn request_local(request: UiRequestPayload, timeout: Duration) -> Result<Value, CliError> {
    let path =
        write_local_ui_request(&request).map_err(|error| CliError::usage(error.to_string()))?;
    let response_path = path.with_extension("response");
    let deadline = Instant::now() + timeout;
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
