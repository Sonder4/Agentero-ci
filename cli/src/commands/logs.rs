//! `agentero logs` — read the desktop app log without knowing where it lives.
//!
//! The desktop Host writes `agentero.log` (rotated to `agentero_<stamp>.log`,
//! 2 MB each, five kept) under the Tauri app log directory. `logs tail
//! --since <actionId>` starts at the first line that mentions a `ui` action
//! id, so a test script can read exactly what one step logged.
//!
//! @see docs/backend/cli.md

use crate::error::{CliError, ExitCode};
use clap::{Subcommand, ValueEnum, ValueHint};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Tauri bundle identifier (`src-tauri/tauri.conf.json`); names the log dir.
const APP_IDENTIFIER: &str = "com.poco-ai.agentero";

#[derive(Debug, Subcommand)]
pub enum LogsCmd {
    /// Print the desktop app log directory and its log files.
    Path {
        /// Read logs from this directory instead.
        #[arg(long = "log-dir", env = "AGENTERO_LOG_DIR", value_hint = ValueHint::DirPath)]
        log_dir: Option<PathBuf>,
    },
    /// Show recent desktop app log entries.
    Tail {
        /// Start at the first entry mentioning this `ui` actionId.
        #[arg(long, value_name = "ACTION_ID")]
        since: Option<String>,
        /// Only entries at this level or more severe.
        #[arg(long, value_enum)]
        level: Option<LogLevel>,
        /// Only entries containing this text (case-insensitive).
        #[arg(long, value_name = "TEXT")]
        grep: Option<String>,
        /// Keep the last N matching entries (0 = all).
        #[arg(short = 'n', long = "lines", default_value_t = 200)]
        lines: usize,
        /// Read logs from this directory instead.
        #[arg(long = "log-dir", env = "AGENTERO_LOG_DIR", value_hint = ValueHint::DirPath)]
        log_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "TRACE" => Some(Self::Trace),
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" => Some(Self::Warn),
            "ERROR" => Some(Self::Error),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// One log record; continuation lines are folded into `message`.
struct Entry {
    date: String,
    time: String,
    level: Option<LogLevel>,
    target: String,
    message: String,
    raw: String,
}

pub fn run(cmd: LogsCmd) -> Result<Value, CliError> {
    match cmd {
        LogsCmd::Path { log_dir } => {
            let dir = resolve_dir(log_dir)?;
            let files = log_files(&dir);
            Ok(json!({
                "logDir": dir,
                "exists": dir.is_dir(),
                "files": files.iter().map(|path| json!({
                    "name": path.file_name().map(|name| name.to_string_lossy()),
                    "bytes": std::fs::metadata(path).map(|meta| meta.len()).ok(),
                })).collect::<Vec<_>>(),
                "lines": [dir.display().to_string()],
            }))
        }
        LogsCmd::Tail {
            since,
            level,
            grep,
            lines,
            log_dir,
        } => tail(resolve_dir(log_dir)?, since, level, grep, lines),
    }
}

fn tail(
    dir: PathBuf,
    since: Option<String>,
    level: Option<LogLevel>,
    grep: Option<String>,
    limit: usize,
) -> Result<Value, CliError> {
    let files = log_files(&dir);
    if files.is_empty() {
        return Err(CliError::new(
            "log_not_found",
            format!(
                "No Agentero log files in {}. Start the desktop app, or pass --log-dir.",
                dir.display()
            ),
            ExitCode::Business,
        ));
    }
    let mut entries = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path).map_err(|error| {
            CliError::message(format!("failed to read {}: {error}", path.display()))
        })?;
        parse_into(&String::from_utf8_lossy(&bytes), &mut entries);
    }

    if let Some(id) = since.as_deref().map(str::trim) {
        if id.len() < 8 || id.chars().any(char::is_whitespace) {
            return Err(CliError::usage(
                "--since expects an actionId returned by an agentero ui command",
            ));
        }
        let start = entries
            .iter()
            .position(|entry| entry.message.contains(id))
            .ok_or_else(|| {
                CliError::with_details(
                    "action_not_found",
                    format!("No log entry mentions action {id}. The log may have rotated past it."),
                    json!({ "actionId": id }),
                    ExitCode::Business,
                )
            })?;
        entries = entries.split_off(start);
    }
    let needle = grep.as_deref().map(str::to_lowercase);
    entries.retain(|entry| {
        level.is_none_or(|min| entry.level.is_some_and(|level| level >= min))
            && needle
                .as_deref()
                .is_none_or(|needle| entry.raw.to_lowercase().contains(needle))
    });
    let matched = entries.len();
    if limit > 0 && entries.len() > limit {
        entries = entries.split_off(entries.len() - limit);
    }

    Ok(json!({
        "logDir": dir,
        "files": files,
        "since": since,
        "matched": matched,
        "truncated": matched > entries.len(),
        "entries": entries.iter().map(|entry| json!({
            "date": entry.date,
            "time": entry.time,
            "level": entry.level.map(LogLevel::as_str),
            "target": entry.target,
            "message": entry.message,
        })).collect::<Vec<_>>(),
        "lines": entries.iter().map(|entry| entry.raw.as_str()).collect::<Vec<_>>(),
    }))
}

/// Parse `[date][time][LEVEL][target] message` records.
fn parse_into(text: &str, entries: &mut Vec<Entry>) {
    for line in text.lines() {
        match parse_header(line) {
            Some((date, time, level, target, message)) => entries.push(Entry {
                date: date.into(),
                time: time.into(),
                level: LogLevel::parse(level),
                target: target.into(),
                message: message.into(),
                raw: line.into(),
            }),
            None => {
                if let Some(entry) = entries.last_mut() {
                    entry.message.push('\n');
                    entry.message.push_str(line);
                    entry.raw.push('\n');
                    entry.raw.push_str(line);
                }
            }
        }
    }
}

fn parse_header(line: &str) -> Option<(&str, &str, &str, &str, &str)> {
    let rest = line.strip_prefix('[')?;
    let (date, rest) = rest.split_once("][")?;
    let (time, rest) = rest.split_once("][")?;
    let (level, rest) = rest.split_once("][")?;
    let (target, message) = rest.split_once(']')?;
    LogLevel::parse(level)?;
    Some((
        date,
        time,
        level,
        target,
        message.strip_prefix(' ').unwrap_or(message),
    ))
}

/// Rotated files oldest first, then the active `agentero.log`.
fn log_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut rotated = Vec::new();
    let mut active = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "agentero.log" {
            active = Some(path);
        } else if name.starts_with("agentero_") && name.ends_with(".log") {
            rotated.push(path);
        }
    }
    // `agentero_<YYYY-MM-DD_HH-MM-SS>.log` sorts chronologically by name.
    rotated.sort();
    rotated.extend(active);
    rotated
}

fn resolve_dir(flag: Option<PathBuf>) -> Result<PathBuf, CliError> {
    flag.or_else(default_log_dir)
        .ok_or_else(|| CliError::usage("Could not locate the app log directory; pass --log-dir"))
}

/// Tauri `app_log_dir()` for the desktop app.
fn default_log_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|home| home.join("Library").join("Logs").join(APP_IDENTIFIER))
    }
    #[cfg(not(target_os = "macos"))]
    {
        dirs::data_local_dir().map(|dir| dir.join(APP_IDENTIFIER).join("logs"))
    }
}
