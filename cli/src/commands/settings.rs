//! `agentero settings` — complete, scriptable access to the shared settings file.
//!
//! The desktop Host and the headless CLI use the same
//! `$XDG_CONFIG_HOME/agentero/settings.json`. Commands in this module operate on
//! JSON Pointer paths so supported top-level, nested, map, and array fields
//! remain reachable without adding one clap flag per UI control.

use crate::error::{CliError, ExitCode};
use crate::output::OutputFormat;
use crate::resolve::{resolve_vault, GlobalOpts};
use agentero_core::fs::{AtomicOpts, SidecarLock};
use agentero_core::paths::{agents_path, plugins_path, settings_path};
use agentero_core::plugins::PluginsDocument;
use clap::{Subcommand, ValueEnum, ValueHint};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConfigScope {
    /// Desktop and CLI preferences in `settings.json`.
    Settings,
    /// Agent registry, commands, args, environment, proxy and default agent.
    Agents,
    /// Installed plugin packages, per-Vault grants and plugin settings.
    Plugins,
    /// Current Vault's `.agentero/doctor.json`.
    Doctor,
    /// CLI defaults and known Vaults in `config.toml`.
    Cli,
}

#[derive(Debug, Subcommand)]
pub enum SettingsCmd {
    /// Print the canonical settings file path and current revision.
    Path,
    /// Read one JSON Pointer value. Defaults to the complete settings object.
    Get {
        /// JSON Pointer (`/translate/provider`) or dotted path (`translate.provider`).
        #[arg(default_value = "/")]
        pointer: String,
        /// Include credential and cookie values instead of masking them.
        #[arg(long = "reveal-secrets")]
        reveal_secrets: bool,
    },
    /// Set one value. Missing parent objects are created.
    Set {
        /// JSON Pointer (`/translate/provider`) or dotted path (`translate.provider`).
        pointer: String,
        /// Bare values become strings; JSON object/array/boolean/number/null syntax is parsed.
        #[arg(conflicts_with_all = ["file", "stdin"])]
        value: Option<String>,
        /// Read the value from a UTF-8 file.
        #[arg(long = "file", value_name = "PATH", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
        /// Read the value from stdin. Prefer this for credentials.
        #[arg(long, conflicts_with = "file")]
        stdin: bool,
        /// Store the complete input as a JSON string, even when it looks like JSON.
        #[arg(long, conflicts_with = "parse_json")]
        string: bool,
        /// Require valid JSON syntax instead of falling back to a string.
        #[arg(long = "parse-json", conflicts_with = "string")]
        parse_json: bool,
        /// Trim leading and trailing whitespace from the supplied value.
        #[arg(long)]
        trim: bool,
        /// Reject the update unless the current file has this revision.
        #[arg(long = "expected-revision")]
        expected_revision: Option<String>,
    },
    /// Remove one value and let Agentero's defaults apply on the next read.
    Unset {
        /// JSON Pointer or dotted path. The settings root cannot be removed.
        pointer: String,
        /// Reject the update unless the current file has this revision.
        #[arg(long = "expected-revision")]
        expected_revision: Option<String>,
    },
    /// Deep-merge a JSON object into the current settings tree.
    Patch {
        /// Inline JSON object. Prefer --file or --stdin for large or secret input.
        #[arg(long, value_name = "JSON", conflicts_with = "file")]
        data: Option<String>,
        /// Read a JSON object from a file.
        #[arg(long = "file", value_name = "PATH", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
        /// Read a JSON object from stdin.
        #[arg(long, conflicts_with = "file")]
        stdin: bool,
        /// Reject the update unless the current file has this revision.
        #[arg(long = "expected-revision")]
        expected_revision: Option<String>,
    },
    /// Replace the complete settings object. Requires --yes.
    Replace {
        /// Inline JSON object. Prefer --file or --stdin for large or secret input.
        #[arg(long, value_name = "JSON", conflicts_with = "file")]
        data: Option<String>,
        /// Read a JSON object from a file.
        #[arg(long = "file", value_name = "PATH", value_hint = ValueHint::FilePath)]
        file: Option<PathBuf>,
        /// Read a JSON object from stdin.
        #[arg(long, conflicts_with = "file")]
        stdin: bool,
        /// Reject the replacement unless the current file has this revision.
        #[arg(long = "expected-revision")]
        expected_revision: Option<String>,
    },
    /// Remove all persisted overrides and return to Agentero defaults. Requires --yes.
    Reset {
        /// Reject the reset unless the current file has this revision.
        #[arg(long = "expected-revision")]
        expected_revision: Option<String>,
    },
    /// List leaf JSON Pointers under an optional prefix.
    Keys {
        /// JSON Pointer or dotted path used as the listing root.
        #[arg(default_value = "/")]
        pointer: String,
    },
}

struct SettingsDocument {
    value: Value,
    revision: Option<String>,
}

struct ConfigTarget {
    scope: ConfigScope,
    path: PathBuf,
    default_value: Value,
}

pub fn run(scope: ConfigScope, cmd: SettingsCmd, globals: &GlobalOpts) -> Result<Value, CliError> {
    let target = resolve_target(scope, globals)?;
    let default_value = &target.default_value;
    match cmd {
        SettingsCmd::Path => path_result(&target),
        SettingsCmd::Get {
            pointer,
            reveal_secrets,
        } => get(&target, &pointer, reveal_secrets, globals),
        SettingsCmd::Keys { pointer } => keys(&target, &pointer),
        SettingsCmd::Set {
            pointer,
            value,
            file,
            stdin,
            string,
            parse_json,
            trim,
            expected_revision,
        } => {
            let raw = read_set_input(value.as_deref(), file.as_deref(), stdin)?;
            let raw = if trim { raw.trim().to_string() } else { raw };
            let parsed = parse_value(&raw, string, parse_json)?;
            mutate(&target, expected_revision.as_deref(), globals, |root| {
                set_pointer(root, &pointer, parsed.clone())?;
                Ok(serde_json::json!({
                    "pointer": normalize_pointer(&pointer)?,
                    "value": redact_for_pointer(&parsed, &pointer)?,
                }))
            })
        }
        SettingsCmd::Unset {
            pointer,
            expected_revision,
        } => mutate(&target, expected_revision.as_deref(), globals, |root| {
            let removed = unset_pointer(root, &pointer)?;
            Ok(serde_json::json!({
                "pointer": normalize_pointer(&pointer)?,
                "removed": redact_for_pointer(&removed, &pointer)?,
            }))
        }),
        SettingsCmd::Patch {
            data,
            file,
            stdin,
            expected_revision,
        } => {
            let parsed = read_object_input(data.as_deref(), file.as_deref(), stdin)?;
            mutate(&target, expected_revision.as_deref(), globals, |root| {
                deep_merge(root, &parsed)?;
                Ok(serde_json::json!({
                    "patched": object_leaf_count(&parsed),
                }))
            })
        }
        SettingsCmd::Replace {
            data,
            file,
            stdin,
            expected_revision,
        } => {
            require_yes(globals, "replacing the complete settings file")?;
            let parsed = read_object_input(data.as_deref(), file.as_deref(), stdin)?;
            mutate(&target, expected_revision.as_deref(), globals, |root| {
                *root = parsed.clone();
                Ok(serde_json::json!({
                    "replaced": object_leaf_count(&parsed),
                }))
            })
        }
        SettingsCmd::Reset { expected_revision } => {
            require_yes(globals, "resetting all persisted settings")?;
            mutate(&target, expected_revision.as_deref(), globals, |root| {
                *root = default_value.clone();
                Ok(serde_json::json!({ "reset": true }))
            })
        }
    }
}

fn resolve_target(scope: ConfigScope, globals: &GlobalOpts) -> Result<ConfigTarget, CliError> {
    let (path, default_value) = match scope {
        ConfigScope::Settings => (settings_path(), Value::Object(Map::new())),
        ConfigScope::Agents => (agents_path(), Value::Object(Map::new())),
        ConfigScope::Plugins => (plugins_path(), PluginsDocument::empty().as_value().clone()),
        ConfigScope::Cli => {
            // Apply the normal, locked legacy migration before touching the
            // new config location; otherwise a first edit could shadow it.
            crate::config::load()?;
            (crate::config::config_path()?, Value::Object(Map::new()))
        }
        ConfigScope::Doctor => {
            let path = resolve_vault(globals)?
                .join(".agentero")
                .join("doctor.json");
            (path, serde_json::json!({ "ignoredAliasPaths": [] }))
        }
    };
    if matches!(scope, ConfigScope::Settings | ConfigScope::Agents) {
        let _lock = SidecarLock::acquire(&path)?;
        let name = match scope {
            ConfigScope::Settings => "settings.json",
            ConfigScope::Agents => "agents.json",
            _ => unreachable!(),
        };
        migrate_legacy_settings(name, &path)?;
    }
    Ok(ConfigTarget {
        scope,
        path,
        default_value,
    })
}

/// Preserve the old copy and publish the new one only after the entire read
/// succeeds. A failed migration must not let a later write shadow old data.
fn migrate_legacy_settings(name: &str, target: &Path) -> Result<(), CliError> {
    if target.try_exists()? {
        return Ok(());
    }
    let Some(legacy_dir) = agentero_core::paths::legacy_config_dir() else {
        return Ok(());
    };
    if legacy_dir == agentero_core::paths::agentero_config_dir() {
        return Ok(());
    }
    migrate_legacy_settings_from(&legacy_dir.join(name), target)
}

fn migrate_legacy_settings_from(source: &Path, target: &Path) -> Result<(), CliError> {
    if target.try_exists()? {
        return Ok(());
    }
    let bytes = match fs::read(source) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(CliError::message(format!(
                "failed to read legacy configuration {}: {error}",
                source.display()
            )));
        }
    };
    agentero_core::fs::atomic_write_with(
        target,
        &bytes,
        &AtomicOpts {
            sync_file: true,
            ..AtomicOpts::OWNER_ONLY
        },
    )?;
    Ok(())
}

fn scope_name(scope: ConfigScope) -> &'static str {
    match scope {
        ConfigScope::Settings => "settings",
        ConfigScope::Agents => "agents",
        ConfigScope::Plugins => "plugins",
        ConfigScope::Doctor => "doctor",
        ConfigScope::Cli => "cli",
    }
}

fn validate_document(scope: ConfigScope, value: &Value) -> Result<(), CliError> {
    if scope == ConfigScope::Settings {
        let settings =
            serde_json::from_value::<agentero_core::settings_schema::AppSettings>(value.clone())
                .map_err(|_| {
                    CliError::new(
                        "settings_invalid_schema",
                        "application settings are invalid; check field types and values",
                        ExitCode::Business,
                    )
                })?;
        if let Some(key) = agentero_core::settings_schema::unsupported_settings_field(value)? {
            return Err(CliError::new(
                "settings_invalid_schema",
                format!("unknown application settings field {key}; the desktop would discard it"),
                ExitCode::Business,
            ));
        }
        agentero_core::http::configure_proxy(
            settings.network_proxy_enabled,
            &settings.network_proxy_url,
        )
        .map_err(|_| {
            CliError::new(
                "settings_invalid_schema",
                "network proxy configuration is invalid",
                ExitCode::Business,
            )
        })?;
    } else if scope == ConfigScope::Doctor {
        serde_json::from_value::<agentero_core::features::vault::doctor::DoctorVaultState>(
            value.clone(),
        )
        .map_err(|_| {
            CliError::new(
                "settings_invalid_schema",
                "Vault Doctor configuration is invalid; check field types and values",
                ExitCode::Business,
            )
        })?;
    } else if scope == ConfigScope::Agents {
        let registry = serde_json::from_value::<agentero_core::agent_schema::AgentRegistryState>(
            value.clone(),
        )
        .map_err(|_| {
            CliError::new(
                "settings_invalid_schema",
                "agent registry is invalid; check field types and values",
                ExitCode::Business,
            )
        })?;
        if let Some(key) = unknown_field(value, &serde_json::to_value(registry)?, "") {
            return Err(CliError::new(
                "settings_invalid_schema",
                format!("unknown agent registry field {key}; the desktop would discard it"),
                ExitCode::Business,
            ));
        }
    } else if scope == ConfigScope::Cli {
        serde_json::from_value::<crate::config::CliConfig>(value.clone()).map_err(|_| {
            CliError::new(
                "settings_invalid_schema",
                "CLI configuration is invalid; check field types and values",
                ExitCode::Business,
            )
        })?;
    } else if scope == ConfigScope::Plugins {
        PluginsDocument::from_value(value.clone()).map_err(|error| {
            CliError::new(
                "settings_invalid_schema",
                format!("plugins configuration is invalid: {error}"),
                ExitCode::Business,
            )
        })?;
    }
    Ok(())
}

fn unknown_field(input: &Value, serialized: &Value, path: &str) -> Option<String> {
    match (input, serialized) {
        (Value::Object(input), Value::Object(serialized)) => {
            for (key, value) in input {
                let child = format!("{path}/{key}");
                match serialized.get(key) {
                    Some(expected) => {
                        if let Some(unknown) = unknown_field(value, expected, &child) {
                            return Some(unknown);
                        }
                    }
                    // serde skips optional None fields when writing.
                    None if value.is_null() => {}
                    None => return Some(child),
                }
            }
            None
        }
        (Value::Array(input), Value::Array(serialized)) => input
            .iter()
            .zip(serialized)
            .enumerate()
            .find_map(|(i, (value, expected))| {
                unknown_field(value, expected, &format!("{path}/{i}"))
            }),
        _ => None,
    }
}

fn path_result(target: &ConfigTarget) -> Result<Value, CliError> {
    let doc = read_document(&target.path, &target.default_value, target.scope)?;
    Ok(serde_json::json!({
        "scope": scope_name(target.scope),
        "path": target.path,
        "exists": doc.revision.is_some(),
        "revision": doc.revision,
    }))
}

fn get(
    target: &ConfigTarget,
    pointer: &str,
    reveal_secrets: bool,
    globals: &GlobalOpts,
) -> Result<Value, CliError> {
    let mut doc = read_document(&target.path, &target.default_value, target.scope)?;
    include_settings_defaults(target.scope, &mut doc.value)?;
    let normalized = normalize_pointer(pointer)?;
    let found = get_pointer(&doc.value, &normalized);
    let value = found
        .map(|value| {
            if reveal_secrets {
                value.clone()
            } else {
                redact_for_pointer(value, &normalized).unwrap_or(Value::Null)
            }
        })
        .unwrap_or(Value::Null);
    let mut result = serde_json::json!({
        "scope": scope_name(target.scope),
        "pointer": normalized,
        "found": found.is_some(),
        "value": value,
        "path": target.path,
        "revision": doc.revision,
        "maskedSecrets": !reveal_secrets,
    });
    if globals.format == OutputFormat::Text && !globals.quiet {
        let rendered = match &result["value"] {
            Value::String(text) => text.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_default(),
        };
        result
            .as_object_mut()
            .expect("settings get result is an object")
            .insert(
                "lines".to_string(),
                serde_json::json!([format!("{} {}", globals.style.key(&normalized), rendered)]),
            );
    }
    Ok(result)
}

fn keys(target: &ConfigTarget, pointer: &str) -> Result<Value, CliError> {
    let mut doc = read_document(&target.path, &target.default_value, target.scope)?;
    include_settings_defaults(target.scope, &mut doc.value)?;
    let normalized = normalize_pointer(pointer)?;
    let value =
        get_pointer(&doc.value, &normalized).ok_or_else(|| pointer_not_found(&normalized))?;
    let mut out = Vec::new();
    collect_leaf_pointers(&normalized, value, &mut out);
    out.sort();
    Ok(Value::Array(out.into_iter().map(Value::String).collect()))
}

fn include_settings_defaults(scope: ConfigScope, value: &mut Value) -> Result<(), CliError> {
    if scope == ConfigScope::Settings {
        // Use the same serde defaults as the Host, while preserving unknown
        // future fields from the on-disk document for round-tripping.
        let mut defaults =
            serde_json::to_value(agentero_core::settings_schema::AppSettings::default())?;
        deep_merge(&mut defaults, value)?;
        *value = defaults;
    }
    Ok(())
}

fn mutate<F>(
    target: &ConfigTarget,
    expected_revision: Option<&str>,
    globals: &GlobalOpts,
    operation: F,
) -> Result<Value, CliError>
where
    F: FnOnce(&mut Value) -> Result<Value, CliError>,
{
    let _lock = SidecarLock::acquire(&target.path)?;
    let mut doc = read_document(&target.path, &target.default_value, target.scope)?;
    if let Some(expected) = expected_revision {
        let actual = doc.revision.as_deref().unwrap_or("");
        if expected != actual {
            return Err(CliError::with_details(
                "settings_revision_conflict",
                "settings changed since the expected revision",
                serde_json::json!({
                    "expectedRevision": expected,
                    "actualRevision": actual,
                    "scope": scope_name(target.scope),
                    "path": target.path,
                }),
                ExitCode::Business,
            ));
        }
    }
    ensure_object_root(&mut doc.value)?;
    let details = operation(&mut doc.value)?;
    ensure_object_root(&mut doc.value)?;
    validate_document(target.scope, &doc.value)?;
    let raw = if target.scope == ConfigScope::Cli {
        let toml_value = toml::Value::try_from(doc.value.clone()).map_err(|error| {
            CliError::usage(format!(
                "CLI configuration cannot be represented in TOML: {error}"
            ))
        })?;
        toml::to_string_pretty(&toml_value)
            .map_err(|error| CliError::message(error.to_string()))?
            .into_bytes()
    } else {
        serde_json::to_vec_pretty(&doc.value)?
    };
    agentero_core::fs::atomic_write_with(
        &target.path,
        &raw,
        &AtomicOpts {
            sync_file: true,
            ..AtomicOpts::OWNER_ONLY
        },
    )?;
    let revision = digest(&raw);
    let mut result = serde_json::json!({
        "scope": scope_name(target.scope),
        "path": target.path,
        "revision": revision,
        "changed": doc.revision.as_deref() != Some(revision.as_str()),
        "result": details,
    });
    if globals.format == OutputFormat::Text && !globals.quiet {
        result
            .as_object_mut()
            .expect("settings mutation result is an object")
            .insert(
                "lines".to_string(),
                serde_json::json!([format!(
                    "{} {} {}",
                    globals.style.ok("saved"),
                    globals.style.path(&target.path.to_string_lossy()),
                    globals.style.dim(&format!("revision {revision}"))
                )]),
            );
    }
    Ok(result)
}

fn read_document(
    path: &Path,
    default_value: &Value,
    scope: ConfigScope,
) -> Result<SettingsDocument, CliError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SettingsDocument {
                value: default_value.clone(),
                revision: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let value = if scope == ConfigScope::Cli {
        let raw =
            std::str::from_utf8(&bytes).map_err(|error| CliError::message(error.to_string()))?;
        let parsed: toml::Value = toml::from_str(raw).map_err(|error| {
            CliError::message(format!(
                "invalid CLI configuration at {}: {error}",
                path.display()
            ))
        })?;
        serde_json::to_value(parsed)?
    } else {
        serde_json::from_slice::<Value>(&bytes).map_err(|error| {
            CliError::new(
                "settings_invalid_json",
                format!("invalid settings JSON at {}: {error}", path.display()),
                ExitCode::Business,
            )
        })?
    };
    if !value.is_object() {
        return Err(CliError::new(
            "settings_invalid_root",
            format!("settings root must be a JSON object: {}", path.display()),
            ExitCode::Business,
        ));
    }
    Ok(SettingsDocument {
        value,
        revision: Some(digest(&bytes)),
    })
}

fn read_set_input(
    positional: Option<&str>,
    file: Option<&Path>,
    stdin: bool,
) -> Result<String, CliError> {
    match (positional, file) {
        (Some(value), None) => Ok(value.to_string()),
        (None, Some(path)) => Ok(fs::read_to_string(path)?),
        (None, None) if stdin => read_stdin(),
        (Some(_), Some(_)) => Err(CliError::usage(
            "settings set accepts a positional value, --file, or --stdin, not more than one",
        )),
        (None, None) => Err(CliError::usage(
            "settings set requires a value, --file, or --stdin",
        )),
    }
}

fn read_object_input(
    data: Option<&str>,
    file: Option<&Path>,
    stdin: bool,
) -> Result<Value, CliError> {
    let raw = match (data, file) {
        (Some(data), None) => data.to_string(),
        (None, Some(path)) => fs::read_to_string(path)?,
        (None, None) if stdin => read_stdin()?,
        (Some(_), Some(_)) => {
            return Err(CliError::usage(
                "provide JSON through one of --data, --file, or --stdin",
            ));
        }
        (None, None) => {
            return Err(CliError::usage(
                "JSON input is required through --data, --file, or --stdin",
            ));
        }
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|error| {
        CliError::new(
            "settings_invalid_json",
            format!("settings input is not valid JSON: {error}"),
            ExitCode::Business,
        )
    })?;
    if !value.is_object() {
        return Err(CliError::usage(
            "settings patch/replace input must be a JSON object",
        ));
    }
    Ok(value)
}

fn read_stdin() -> Result<String, CliError> {
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    Ok(raw)
}

fn parse_value(raw: &str, string: bool, parse_json: bool) -> Result<Value, CliError> {
    if string {
        return Ok(Value::String(raw.to_string()));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value) => Ok(value),
        Err(error) if parse_json => Err(CliError::new(
            "settings_invalid_json",
            format!("settings value is not valid JSON: {error}"),
            ExitCode::Business,
        )),
        Err(_) => Ok(Value::String(raw.to_string())),
    }
}

fn require_yes(globals: &GlobalOpts, operation: &str) -> Result<(), CliError> {
    if globals.yes {
        Ok(())
    } else {
        Err(CliError::needs_confirmation(format!(
            "{operation} requires --yes"
        )))
    }
}

fn ensure_object_root(value: &mut Value) -> Result<(), CliError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(CliError::new(
            "settings_invalid_root",
            "settings root must remain a JSON object",
            ExitCode::Business,
        ))
    }
}

fn normalize_pointer(raw: &str) -> Result<String, CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" || trimmed == "." {
        return Ok("/".to_string());
    }
    if trimmed.starts_with('/') {
        validate_pointer_syntax(trimmed)?;
        return Ok(trimmed.to_string());
    }
    let segments: Vec<String> = trimmed
        .split('.')
        .map(|part| part.replace('~', "~0").replace('/', "~1"))
        .collect();
    if segments.iter().any(|part| part.is_empty()) {
        return Err(CliError::usage(format!(
            "invalid dotted settings path `{raw}`"
        )));
    }
    Ok(format!("/{}", segments.join("/")))
}

fn validate_pointer_syntax(pointer: &str) -> Result<(), CliError> {
    if pointer == "/" {
        return Ok(());
    }
    for token in pointer.trim_start_matches('/').split('/') {
        let mut chars = token.chars();
        while let Some(ch) = chars.next() {
            if ch == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err(CliError::usage(format!(
                    "invalid JSON Pointer escape in `{pointer}`"
                )));
            }
        }
    }
    Ok(())
}

fn pointer_tokens(pointer: &str) -> Result<Vec<String>, CliError> {
    let normalized = normalize_pointer(pointer)?;
    if normalized == "/" {
        return Ok(Vec::new());
    }
    Ok(normalized
        .trim_start_matches('/')
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect())
}

fn get_pointer<'a>(root: &'a Value, pointer: &str) -> Option<&'a Value> {
    pointer_tokens(pointer)
        .ok()?
        .iter()
        .try_fold(root, |current, token| match current {
            Value::Object(map) => map.get(token),
            Value::Array(items) => token
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => None,
        })
}

fn set_pointer(root: &mut Value, pointer: &str, value: Value) -> Result<(), CliError> {
    let tokens = pointer_tokens(pointer)?;
    if tokens.is_empty() {
        return Err(CliError::usage(
            "cannot set the settings root; use `settings replace --yes`",
        ));
    }
    let mut current = root;
    for (index, token) in tokens.iter().enumerate() {
        let last = index + 1 == tokens.len();
        match current {
            Value::Object(map) => {
                if last {
                    map.insert(token.clone(), value);
                    return Ok(());
                }
                if !map.contains_key(token) {
                    map.insert(token.clone(), Value::Object(Map::new()));
                }
                let next = map.get_mut(token).expect("inserted settings parent");
                if !next.is_object() && !next.is_array() {
                    return Err(pointer_conflict(pointer, token));
                }
                current = next;
            }
            Value::Array(items) => {
                let array_index = token
                    .parse::<usize>()
                    .map_err(|_| pointer_conflict(pointer, token))?;
                if last {
                    if array_index < items.len() {
                        items[array_index] = value;
                    } else if array_index == items.len() {
                        items.push(value);
                    } else {
                        return Err(pointer_conflict(pointer, token));
                    }
                    return Ok(());
                }
                current = items
                    .get_mut(array_index)
                    .ok_or_else(|| pointer_conflict(pointer, token))?;
            }
            _ => return Err(pointer_conflict(pointer, token)),
        }
    }
    Ok(())
}

fn unset_pointer(root: &mut Value, pointer: &str) -> Result<Value, CliError> {
    let tokens = pointer_tokens(pointer)?;
    if tokens.is_empty() {
        return Err(CliError::usage(
            "cannot unset the settings root; use `agentero settings reset --yes`",
        ));
    }
    let (parents, leaf) = tokens.split_at(tokens.len() - 1);
    let leaf = &leaf[0];
    let mut current = root;
    for token in parents {
        current = match current {
            Value::Object(map) => map
                .get_mut(token)
                .ok_or_else(|| pointer_not_found(pointer))?,
            Value::Array(items) => items
                .get_mut(
                    token
                        .parse::<usize>()
                        .map_err(|_| pointer_conflict(pointer, token))?,
                )
                .ok_or_else(|| pointer_not_found(pointer))?,
            _ => return Err(pointer_not_found(pointer)),
        };
    }
    match current {
        Value::Object(map) => map.remove(leaf).ok_or_else(|| pointer_not_found(pointer)),
        Value::Array(items) => {
            let index = leaf
                .parse::<usize>()
                .map_err(|_| pointer_conflict(pointer, leaf))?;
            if index >= items.len() {
                Err(pointer_not_found(pointer))
            } else {
                Ok(items.remove(index))
            }
        }
        _ => Err(pointer_not_found(pointer)),
    }
}

fn deep_merge(target: &mut Value, patch: &Value) -> Result<(), CliError> {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                if value.is_object() {
                    if !target.get(key).is_some_and(Value::is_object) {
                        target.insert(key.clone(), Value::Object(Map::new()));
                    }
                    deep_merge(target.get_mut(key).expect("settings patch parent"), value)?;
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
            Ok(())
        }
        _ => Err(CliError::usage("settings patch root must be a JSON object")),
    }
}

fn collect_leaf_pointers(pointer: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                let token = key.replace('~', "~0").replace('/', "~1");
                let child_pointer = if pointer == "/" {
                    format!("/{token}")
                } else {
                    format!("{pointer}/{token}")
                };
                collect_leaf_pointers(&child_pointer, child, out);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, child) in items.iter().enumerate() {
                let child_pointer = if pointer == "/" {
                    format!("/{index}")
                } else {
                    format!("{pointer}/{index}")
                };
                collect_leaf_pointers(&child_pointer, child, out);
            }
        }
        _ => out.push(pointer.to_string()),
    }
}

fn object_leaf_count(value: &Value) -> usize {
    let mut pointers = Vec::new();
    collect_leaf_pointers("/", value, &mut pointers);
    pointers.len()
}

fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let redacted = if is_secret_key(key) {
                        redact_leaf(value)
                    } else {
                        redact_value(value)
                    };
                    (key.clone(), redacted)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::String(text) => redact_text_with_url_credentials(text),
        other => other.clone(),
    }
}

fn redact_for_pointer(value: &Value, pointer: &str) -> Result<Value, CliError> {
    let tokens = pointer_tokens(pointer)?;
    if tokens.iter().any(|token| is_secret_key(token)) {
        Ok(redact_leaf(value))
    } else {
        Ok(redact_value(value))
    }
}

fn redact_leaf(value: &Value) -> Value {
    match value {
        Value::String(secret) if !secret.is_empty() => {
            Value::String("*".repeat(secret.chars().count()))
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_leaf(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_leaf).collect()),
        other => other.clone(),
    }
}

fn is_secret_key(key: &str) -> bool {
    let key: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    key == "env"
        || key == "args"
        || key == "easyscholarkey"
        || key == "zhihuaccesssecret"
        || key == "institutionproxycookie"
        || key.ends_with("apikey")
        || key.contains("accesskey")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("token")
        || key.contains("cookie")
        || key.contains("privatekey")
        || key.contains("credential")
        || key.contains("authorization")
}

fn redact_text_with_url_credentials(text: &str) -> Value {
    let Some((_, rest)) = text.split_once("://") else {
        return Value::String(text.to_string());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let query = rest.split_once('?').map(|(_, query)| query).unwrap_or("");
    let has_secret_query = query.split('&').any(|part| {
        part.split_once('=')
            .is_some_and(|(key, _)| is_secret_key(key) || key.eq_ignore_ascii_case("key"))
    });
    if authority.contains('@') || has_secret_query {
        redact_leaf(&Value::String(text.to_string()))
    } else {
        Value::String(text.to_string())
    }
}

fn pointer_conflict(pointer: &str, token: &str) -> CliError {
    CliError::new(
        "settings_path_conflict",
        format!("cannot create settings pointer `{pointer}` at `{token}`"),
        ExitCode::Business,
    )
}

fn pointer_not_found(pointer: &str) -> CliError {
    CliError::new(
        "settings_path_not_found",
        format!("settings pointer not found: {pointer}"),
        ExitCode::Business,
    )
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
