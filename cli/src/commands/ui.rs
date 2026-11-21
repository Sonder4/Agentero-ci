//! `agentero ui *` — workspace / window management from the CLI.
//!
//! Mirrors the desktop workspace primitives (open-or-focus a document or a
//! native child window, close a document) so scripts and external agents can
//! drive the running App without touching its internals.

use crate::error::{CliError, ExitCode};
use crate::resolve::{resolve_vault, GlobalOpts};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::{Subcommand, ValueEnum, ValueHint};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agentero_core::features::open_request::{
    local_ui_screenshot_path, write_local_ui_request, UiIdleSpec, UiRequestPayload, UiSlideSpec,
    UiWaitSpec, UI_ACTION_CHATGPT, UI_ACTION_SCREENSHOT, UI_ACTION_SLIDE, UI_ACTION_WAIT,
    UI_ACTION_WAIT_IDLE, UI_WAIT_MAX_MS,
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
    /// Wait until a control (action id or label) is present or gone, or the app is idle.
    Wait {
        /// Action id or label; omit with --idle.
        #[arg(required_unless_present = "idle", conflicts_with = "idle")]
        target: Option<String>,
        /// Wait until no visible control matches instead.
        #[arg(long, conflicts_with = "idle")]
        gone: bool,
        /// Wait until no background job (import, parse, layout, translate,
        /// sync, download, compile) has been queued or running for 0.75 s.
        #[arg(long)]
        idle: bool,
        /// Seconds before giving up (at most 120).
        #[arg(long, default_value_t = 10.0)]
        timeout: f64,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Set a slider (action id or label) to a value, `min`, `max`, or `+N`/`-N` steps.
    Slide {
        target: String,
        #[arg(allow_hyphen_values = true)]
        value: String,
        #[arg(long)]
        occurrence: Option<usize>,
        #[arg(long, default_value = "main")]
        window: String,
    },
    /// Save a PNG of a running Agentero window (Linux: page content only).
    Screenshot {
        #[arg(long, default_value = "main")]
        window: String,
        /// Output PNG (default: agentero-<window>-<time>.png in the current directory).
        // `-o` / `--out`, like `export bib`, so it does not clash with global `--output`.
        #[arg(short = 'o', long = "out", value_name = "FILE", value_hint = ValueHint::FilePath)]
        out: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        overwrite: bool,
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
        UiCmd::Wait {
            target,
            gone,
            idle,
            timeout,
            window,
        } => run_wait(target, gone, idle, timeout, window),
        UiCmd::Slide {
            target,
            value,
            occurrence,
            window,
        } => {
            let spec = UiSlideSpec {
                target,
                occurrence,
                value,
            };
            let spec = serde_json::to_string(&spec).expect("slide spec serializes");
            let request = UiRequestPayload::local_action(UI_ACTION_SLIDE, Some(spec), window, None);
            request_local(request, Duration::from_secs(10))
        }
        UiCmd::Screenshot {
            window,
            out,
            overwrite,
        } => run_screenshot(window, out, overwrite),
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

fn run_wait(
    target: Option<String>,
    gone: bool,
    idle: bool,
    timeout: f64,
    window: String,
) -> Result<Value, CliError> {
    let timeout_ms = (timeout * 1000.0).round();
    if !timeout_ms.is_finite() || timeout_ms < 1.0 || timeout_ms > UI_WAIT_MAX_MS as f64 {
        return Err(CliError::usage(format!(
            "--timeout must be between 0.001 and {} seconds",
            UI_WAIT_MAX_MS / 1000
        )));
    }
    let timeout_ms = timeout_ms as u64;
    let request = match target {
        // Answered by the Host from its job queue; any window works.
        _ if idle => UiRequestPayload::local_action(
            UI_ACTION_WAIT_IDLE,
            Some(serde_json::to_string(&UiIdleSpec { timeout_ms }).expect("idle spec serializes")),
            window,
            None,
        ),
        Some(target) => {
            let spec = serde_json::to_string(&UiWaitSpec {
                target,
                gone,
                timeout_ms,
            })
            .expect("wait spec serializes");
            UiRequestPayload::local_action(UI_ACTION_WAIT, Some(spec), window, None)
        }
        None => {
            return Err(CliError::usage(
                "Name a control to wait for, or pass --idle",
            ))
        }
    };
    // The desktop answers when the wait ends; allow for request pickup on top.
    request_local(
        request,
        Duration::from_millis(timeout_ms) + Duration::from_secs(10),
    )
}

fn run_screenshot(
    window: String,
    output: Option<PathBuf>,
    overwrite: bool,
) -> Result<Value, CliError> {
    let output = match output {
        Some(path) => path,
        None => {
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis());
            PathBuf::from(format!("agentero-{window}-{millis}.png"))
        }
    };
    if !output
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        return Err(CliError::usage("The screenshot output must be a .png file"));
    }
    if output.exists() && !overwrite {
        return Err(CliError::usage(
            "The output file already exists. Pass --overwrite to replace it.",
        ));
    }
    let directory = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !directory.is_dir() {
        return Err(CliError::usage("The output directory does not exist"));
    }
    let request = UiRequestPayload::local_action(UI_ACTION_SCREENSHOT, None, window, None);
    let id = request.reference.clone().unwrap_or_default();
    let mut result = request_local(request, Duration::from_secs(20))?;
    let captured =
        local_ui_screenshot_path(&id).map_err(|error| CliError::message(error.to_string()))?;
    if std::fs::rename(&captured, &output).is_err() {
        // Different volume: copy, then drop the Host's file.
        std::fs::copy(&captured, &output)
            .map_err(|error| CliError::message(format!("failed to save screenshot: {error}")))?;
        let _ = std::fs::remove_file(&captured);
    }
    let saved = output.canonicalize().unwrap_or(output);
    let line = format!(
        "Saved {}x{} screenshot to {}",
        result["width"],
        result["height"],
        saved.display()
    );
    if let Some(object) = result.as_object_mut() {
        object.insert("path".into(), Value::String(saved.display().to_string()));
        object.insert("lines".into(), serde_json::json!([line]));
    }
    Ok(result)
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
    // The same id appears in the app log (`op … ui_action id=…`).
    let action_id = request.reference.clone().unwrap_or_default();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match std::fs::read_to_string(&response_path) {
            Ok(body) => {
                let _ = std::fs::remove_file(&response_path);
                let mut response: Value = serde_json::from_str(&body)
                    .map_err(|error| CliError::message(error.to_string()))?;
                if let Some(error) = response.get("error").and_then(Value::as_str) {
                    let code = response.get("code").and_then(Value::as_str);
                    return Err(action_error(
                        action_code(code),
                        format!("{error} (action {action_id})"),
                        &action_id,
                    ));
                }
                // ChatGPT exports write the response verbatim; keep them clean.
                if request.action != UI_ACTION_CHATGPT {
                    if let Some(object) = response.as_object_mut() {
                        object.insert("actionId".into(), Value::String(action_id));
                    }
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
    Err(action_error(
        "no_ack",
        format!(
            "Agentero did not acknowledge the action. Check that the app is running and the requested window is open. (action {action_id})"
        ),
        &action_id,
    ))
}

/// Error codes the desktop may report for a dispatched local action
/// (documented in docs/backend/cli.md). Unknown or missing codes, such as
/// failures from older apps, become `action_failed`.
const ACTION_ERROR_CODES: &[&str] = &[
    "control_not_found",
    "control_ambiguous",
    "control_disabled",
    "control_obscured",
    "occurrence_out_of_range",
    "not_a_slider",
    "value_out_of_range",
    "slider_unresponsive",
    "command_unavailable",
    "shortcut_unavailable",
    "invalid_key_chord",
    "focus_in_frame",
    "no_editable_focus",
    "wait_timeout",
    "window_not_open",
    "window_minimized",
    "window_hidden",
    "capture_failed",
    "capture_blank",
    "unsupported_platform",
    "unsupported_action",
    "invalid_argument",
    "action_failed",
];

fn action_code(code: Option<&str>) -> &'static str {
    code.and_then(|code| ACTION_ERROR_CODES.iter().find(|known| **known == code))
        .copied()
        .unwrap_or("action_failed")
}

/// A failed dispatched action; `details.actionId` matches the app log.
fn action_error(code: &'static str, message: String, action_id: &str) -> CliError {
    CliError::with_details(
        code,
        message,
        serde_json::json!({ "actionId": action_id }),
        ExitCode::Business,
    )
}
