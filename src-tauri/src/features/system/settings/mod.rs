//! Application UI settings — XDG config file `settings.json`.
//!
//! Frontend `AppSettings` (camelCase JSON) is the source of shape; Host owns
//! the durable file under [`crate::core::paths::settings_path`].
//!
//! Coupling contract: settings provides read/write/persist/broadcast only and
//! never calls into domain features. Domains that must react to changes
//! (connector port, agent proxy, import parser, jobs layout cap) register
//! [`AppSettingsStore::subscribe`] listeners at app assembly; domain defaults
//! needed at deserialize time (e.g. [`DEFAULT_CONNECTOR_PORT`]) live here and
//! are re-exported by the owning domain.

use crate::core::error::AppError;
use crate::core::paths::{self, settings_path};
use crate::features::system::builtin;
use agentero_core::decision::SystemOneCredentials;
pub use agentero_core::settings_schema::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Upper bound for `auto_translate_import_concurrency` (the settings UI offers
/// 1–5; larger values would hammer translation providers and disk).
pub const MAX_AUTO_TRANSLATE_IMPORT_CONCURRENCY: u32 = 5;

/// Upper bound for the OpenAI-compatible full-document translation batch pool
/// (the settings UI offers 1–8; higher values trigger provider rate limits).
pub const MAX_TRANSLATE_CONCURRENCY: u32 = 8;

/// Built-in provider id, re-exported so the Host never re-types the literal.
pub use agentero_core::features::translate::BUILTIN_PROVIDER_ID;

/// `EmbeddingSettings::source` vocabulary. Empty means "unset" and is resolved
/// from the stored BYOK fields by [`normalize`], so a legacy `settings.json`
/// with a populated endpoint keeps working without a `source` key.
const EMBEDDING_SOURCE_BUILTIN: &str = "builtin";
const EMBEDDING_SOURCE_CUSTOM: &str = "custom";

/// Whether `provider` names the built-in provider (credentials come from the
/// build, never from `providerConfigs`).
fn is_builtin_provider(provider: &str) -> bool {
    provider.trim().eq_ignore_ascii_case(BUILTIN_PROVIDER_ID)
}

/// True when `key` is a UI mask of only `*` (length mirrors the real secret).
/// Real secrets stay in the Host process / settings file; `settings_set` treats
/// an all-asterisk value as “keep previous key”.
pub fn is_translate_api_key_mask(key: &str) -> bool {
    let t = key.trim();
    !t.is_empty() && t.chars().all(|c| c == '*')
}

/// Replace a non-empty secret with the same number of `*` characters.
pub fn mask_translate_api_key(key: &str) -> String {
    let n = key.trim().chars().count();
    if n == 0 {
        String::new()
    } else {
        "*".repeat(n)
    }
}

/// Domain reaction fired after settings are persisted. Receives the same
/// redacted snapshot that `settings_set` returns and broadcasts.
pub type SettingsListener = Arc<dyn Fn(&AppSettings) + Send + Sync>;

/// In-memory + file-backed settings store.
pub struct AppSettingsStore {
    inner: Mutex<AppSettings>,
    path: PathBuf,
    /// Content revision last observed by Host writes or an external-file reload.
    last_disk_revision: Mutex<Option<String>>,
    /// Domain reactions registered by the app assembly; fired after every
    /// successful [`set`](Self::set). Registering from the assembly (instead
    /// of calling domains from here) keeps settings schema-agnostic.
    listeners: Mutex<Vec<SettingsListener>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsGetResult {
    pub settings: AppSettings,
    /// Absolute path to the settings file.
    pub path: String,
    /// Whether the file already existed before this read (false → first run / defaults).
    pub existed: bool,
}

/// An atomic update to one frontend preference. `expected` protects a
/// concurrent CLI or desktop edit; migration can insert missing keys only.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FrontendPreferenceUpdate {
    pub key: String,
    #[specta(type = Option<agentero_core::json::Json>)]
    pub value: Option<serde_json::Value>,
    #[specta(type = Option<agentero_core::json::Json>)]
    pub expected: Option<serde_json::Value>,
    pub only_if_missing: bool,
}

impl AppSettingsStore {
    pub fn load() -> Self {
        let path = settings_path();
        let _lock = crate::core::fs::SidecarLock::acquire(&path).ok();
        if _lock.is_some() {
            paths::migrate_legacy_file("settings.json", &path);
        }
        let (settings, _existed) = read_file(&path);
        let revision = match fs::read(&path) {
            Ok(bytes) if serde_json::from_slice::<AppSettings>(&bytes).is_ok() => {
                Some(hex::encode(Sha256::digest(bytes)))
            }
            _ => None,
        };
        Self {
            inner: Mutex::new(settings),
            last_disk_revision: Mutex::new(revision),
            path,
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// In-memory store over `settings` for tests. The path is a scratch name
    /// under the temp dir and is only written if the test calls [`set`](Self::set).
    #[cfg(test)]
    pub fn for_tests(settings: AppSettings) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        Self {
            inner: Mutex::new(settings),
            path: std::env::temp_dir().join(format!("agentero-settings-for-tests-{nanos}.json")),
            last_disk_revision: Mutex::new(None),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Register a domain reaction fired after every successful `set`.
    /// Called by the app assembly on behalf of domains (connector port,
    /// agent proxy, import parser, jobs layout cap), so settings keeps no
    /// edges into domain features.
    pub fn subscribe(&self, listener: impl Fn(&AppSettings) + Send + Sync + 'static) {
        if let Ok(mut guard) = self.listeners.lock() {
            guard.push(Arc::new(listener));
        }
    }

    fn notify(&self, settings: &AppSettings) {
        let Ok(listeners) = self.listeners.lock().map(|guard| guard.clone()) else {
            return;
        };
        for listener in listeners.iter() {
            listener(settings);
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn get(&self) -> Result<SettingsGetResult, AppError> {
        let settings = self
            .inner
            .lock()
            .map_err(|_| AppError::message("settings lock poisoned"))?
            .clone();
        let existed = self.path.is_file();
        Ok(SettingsGetResult {
            settings: redact_secrets(settings),
            path: self.path.to_string_lossy().into_owned(),
            existed,
        })
    }

    pub fn set(&self, settings: AppSettings) -> Result<AppSettings, AppError> {
        self.set_with_base(settings, None)
    }

    /// Merge the fields changed by a WebView against the snapshot it edited.
    /// Concurrent changes to the same field are rejected instead of lost.
    pub fn set_with_base(
        &self,
        mut settings: AppSettings,
        base: Option<AppSettings>,
    ) -> Result<AppSettings, AppError> {
        let lock = crate::core::fs::SidecarLock::acquire(&self.path)?;
        self.ensure_disk_revision_is_current()?;
        ensure_settings_file_supported(&self.path)?;
        let previous_proxy;
        {
            let previous = self
                .inner
                .lock()
                .map_err(|_| AppError::message("settings lock poisoned"))?;
            previous_proxy = (
                previous.network_proxy_enabled,
                previous.network_proxy_url.clone(),
            );
            if let Some(base) = base {
                settings = merge_settings_edits(base, settings, &previous)?;
            }
            merge_secrets(&mut settings, &previous);
            // Frontend preferences have their own compare-and-swap operation.
            // A settings form can be based on an older WebView snapshot.
            settings.frontend_preferences = previous.frontend_preferences.clone();
        }
        normalize(&mut settings);
        crate::core::http::configure_proxy(
            settings.network_proxy_enabled,
            &settings.network_proxy_url,
        )?;
        if let Err(error) = persist(&self.path, &settings) {
            let _ = crate::core::http::configure_proxy(previous_proxy.0, &previous_proxy.1);
            return Err(error);
        }
        sync_institution_proxy(&settings);
        if let Ok(mut revision) = self.last_disk_revision.lock() {
            *revision = disk_revision(&self.path).ok().flatten();
        }
        // Never echo raw API keys back to the WebView / settings:changed.
        let redacted = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| AppError::message("settings lock poisoned"))?;
            *guard = settings.clone();
            redact_secrets(settings)
        };
        drop(lock);
        // Fire domain reactions with the store lock released: listeners read
        // back into the store (e.g. import::refresh_parser_config).
        self.notify(&redacted);
        Ok(redacted)
    }

    /// Apply preference changes without replacing the rest of the settings.
    pub fn update_frontend_preferences(
        &self,
        updates: Vec<FrontendPreferenceUpdate>,
    ) -> Result<AppSettings, AppError> {
        let lock = crate::core::fs::SidecarLock::acquire(&self.path)?;
        self.ensure_disk_revision_is_current()?;
        ensure_settings_file_supported(&self.path)?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| AppError::message("settings lock poisoned"))?;
        let mut settings = guard.clone();
        let mut changed = false;
        for update in updates {
            let previous = settings.frontend_preferences.get(&update.key);
            if update.only_if_missing && previous.is_some() {
                continue;
            }
            if !update.only_if_missing && previous != update.expected.as_ref() {
                return Err(AppError::message(format!(
                    "frontend preference {} changed externally; retry",
                    update.key
                )));
            }
            match update.value {
                Some(value) => {
                    if previous != Some(&value) {
                        settings.frontend_preferences.insert(update.key, value);
                        changed = true;
                    }
                }
                None => {
                    changed |= settings.frontend_preferences.remove(&update.key).is_some();
                }
            }
        }
        if changed {
            persist(&self.path, &settings)?;
            *self
                .last_disk_revision
                .lock()
                .map_err(|_| AppError::message("settings revision lock poisoned"))? =
                disk_revision(&self.path)?;
            *guard = settings.clone();
        }
        let redacted = redact_secrets(settings);
        drop(guard);
        drop(lock);
        if changed {
            self.notify(&redacted);
        }
        Ok(redacted)
    }

    /// Reload settings when another Agentero process atomically replaces the
    /// shared file. Successful reloads normalize the settings, refresh core
    /// proxy state, and fire the same domain listeners as `set`.
    pub fn reload_if_changed(&self) -> Result<Option<AppSettings>, AppError> {
        let current = disk_revision(&self.path)?;
        let last = self
            .last_disk_revision
            .lock()
            .map_err(|_| AppError::message("settings revision lock poisoned"))?
            .clone();
        if current == last {
            return Ok(None);
        }

        let _lock = crate::core::fs::SidecarLock::acquire(&self.path)?;
        let current = disk_revision(&self.path)?;
        let mut observed = self
            .last_disk_revision
            .lock()
            .map_err(|_| AppError::message("settings revision lock poisoned"))?;
        if *observed == current {
            return Ok(None);
        }

        let mut settings = match current {
            Some(_) => {
                let raw = fs::read_to_string(&self.path)?;
                serde_json::from_str::<AppSettings>(&raw).map_err(|error| {
                    AppError::message(format!(
                        "invalid settings.json at {}: {error}",
                        self.path.display()
                    ))
                })?
            }
            None => AppSettings::default(),
        };
        normalize(&mut settings);
        crate::core::http::configure_proxy(
            settings.network_proxy_enabled,
            &settings.network_proxy_url,
        )?;
        sync_institution_proxy(&settings);
        let redacted = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| AppError::message("settings lock poisoned"))?;
            *guard = settings.clone();
            redact_secrets(settings)
        };
        *observed = current;
        drop(observed);
        drop(_lock);
        self.notify(&redacted);
        Ok(Some(redacted))
    }

    fn ensure_disk_revision_is_current(&self) -> Result<(), AppError> {
        let current = disk_revision(&self.path)?;
        let observed = self
            .last_disk_revision
            .lock()
            .map_err(|_| AppError::message("settings revision lock poisoned"))?;
        if *observed != current {
            return Err(AppError::message(
                "settings changed externally; retry after they reload",
            ));
        }
        Ok(())
    }

    /// Raw network-proxy configuration for process bootstrap paths that run
    /// before the redacted settings snapshot is exposed to the WebView.
    pub fn network_proxy_config(&self) -> (bool, String) {
        self.inner
            .lock()
            .map(|settings| {
                (
                    settings.network_proxy_enabled,
                    settings.network_proxy_url.clone(),
                )
            })
            .unwrap_or_else(|_| (false, default_network_proxy_url()))
    }

    /// Resolve a commercial MT API key by provider id (case-insensitive).
    /// Used by `translate_text` so the WebView never needs the plaintext key.
    pub fn translate_api_key(&self, provider: &str) -> Option<String> {
        let key = commercial_provider_settings_key(provider)?;
        let guard = self.inner.lock().ok()?;
        let cfg = guard.translate.provider_configs.get(key)?;
        let api_key = cfg.api_key.trim();
        if api_key.is_empty() || is_translate_api_key_mask(api_key) {
            None
        } else {
            Some(api_key.to_string())
        }
    }

    /// Custom translate templates (legacy prompt, system, user). Injected by
    /// `translate_text` for the OpenAI-compatible path. The Agent path reads
    /// the same fields on the frontend.
    pub fn translate_custom_templates(&self) -> (String, String, String) {
        self.inner
            .lock()
            .ok()
            .map(|guard| {
                (
                    guard.translate.custom_prompt.trim().to_string(),
                    guard.translate.custom_system.trim().to_string(),
                    guard.translate.custom_user.trim().to_string(),
                )
            })
            .unwrap_or_default()
    }

    pub fn layout_backend(&self) -> String {
        self.inner
            .lock()
            .ok()
            .map(|guard| guard.layout.backend.clone())
            .filter(|backend| !backend.trim().is_empty())
            .unwrap_or_else(default_layout_backend)
    }

    /// Magic-wand / import parallelism, seeded into the JobCenter `Import` cap.
    pub fn batch_import_concurrency(&self) -> usize {
        self.inner
            .lock()
            .ok()
            .map(|guard| guard.batch_import_concurrency.max(1) as usize)
            .unwrap_or_else(|| default_batch_import_concurrency() as usize)
    }

    /// Whether imported papers get a background full-text translation job.
    pub fn auto_translate_import(&self) -> bool {
        self.inner
            .lock()
            .ok()
            .map(|guard| guard.translate.auto_translate_import)
            .unwrap_or(false)
    }

    /// Background import-translation parallelism, seeded into the JobCenter
    /// `LayoutTranslate` cap (1–5; out-of-range values fall back to 1).
    pub fn auto_translate_import_concurrency(&self) -> usize {
        self.inner
            .lock()
            .ok()
            .map(|guard| {
                guard
                    .translate
                    .auto_translate_import_concurrency
                    .clamp(1, MAX_AUTO_TRANSLATE_IMPORT_CONCURRENCY) as usize
            })
            .unwrap_or_else(|| default_auto_translate_import_concurrency() as usize)
    }

    /// PAPER.md body-parse engine backend (`local` when unset).
    pub fn parser_backend(&self) -> String {
        self.inner
            .lock()
            .ok()
            .map(|guard| guard.layout.parser_backend.clone())
            .filter(|backend| !backend.trim().is_empty())
            .unwrap_or_else(default_parser_backend)
    }

    /// Resolve a layout-provider API key by provider id (e.g. `paddle`).
    /// Used by the layout_remote commands so the WebView never needs the key.
    ///
    /// The built-in provider resolves here rather than in each caller: it has no
    /// `providerConfigs` card (its key must never be persistable), so without
    /// this the parser engine would see no credentials at all.
    pub fn layout_api_key(&self, provider: &str) -> Option<String> {
        if is_builtin_provider(provider) {
            return builtin::api_key().map(str::to_string);
        }
        let key = layout_provider_settings_key(provider)?;
        let guard = self.inner.lock().ok()?;
        let cfg = guard.layout.provider_configs.get(key)?;
        let api_key = cfg.api_key.trim();
        if api_key.is_empty() || is_translate_api_key_mask(api_key) {
            None
        } else {
            Some(api_key.to_string())
        }
    }

    /// Resolve a layout-provider base URL by provider id (empty → None).
    pub fn layout_base_url(&self, provider: &str) -> Option<String> {
        if is_builtin_provider(provider) {
            return Some(builtin::status().base_url);
        }
        let key = layout_provider_settings_key(provider)?;
        let guard = self.inner.lock().ok()?;
        let cfg = guard.layout.provider_configs.get(key)?;
        let base_url = cfg.base_url.trim();
        if base_url.is_empty() {
            None
        } else {
            Some(base_url.to_string())
        }
    }

    /// Resolve a layout-provider model id by provider id (empty → None).
    pub fn layout_model(&self, provider: &str) -> Option<String> {
        if is_builtin_provider(provider) {
            return Some(builtin::status().ocr_model);
        }
        let key = layout_provider_settings_key(provider)?;
        let guard = self.inner.lock().ok()?;
        let cfg = guard.layout.provider_configs.get(key)?;
        let model = cfg.model.trim();
        if model.is_empty() {
            None
        } else {
            Some(model.to_string())
        }
    }

    /// Resolve a layout-provider OCR prompt override (empty → None).
    pub fn layout_prompt(&self, provider: &str) -> Option<String> {
        let key = layout_provider_settings_key(provider)?;
        let guard = self.inner.lock().ok()?;
        let cfg = guard.layout.provider_configs.get(key)?;
        let prompt = cfg.prompt.trim();
        if prompt.is_empty() {
            None
        } else {
            Some(prompt.to_string())
        }
    }

    /// Resolve a layout-provider document language (empty → None; the MinerU
    /// engine falls back to `ch`).
    pub fn layout_language(&self, provider: &str) -> Option<String> {
        let key = layout_provider_settings_key(provider)?;
        let guard = self.inner.lock().ok()?;
        let cfg = guard.layout.provider_configs.get(key)?;
        let language = cfg.language.trim();
        if language.is_empty() {
            None
        } else {
            Some(language.to_string())
        }
    }

    /// Resolve a layout-provider force-OCR flag (default false).
    pub fn layout_is_ocr(&self, provider: &str) -> bool {
        layout_provider_settings_key(provider)
            .and_then(|key| {
                self.inner.lock().ok().map(|guard| {
                    guard
                        .layout
                        .provider_configs
                        .get(key)
                        .is_some_and(|cfg| cfg.is_ocr)
                })
            })
            .unwrap_or(false)
    }

    /// Resolve the MinerU self-hosted switch (default false). Only the MinerU
    /// engine reads it; other providers return false.
    pub fn layout_self_hosted(&self, provider: &str) -> bool {
        layout_provider_settings_key(provider)
            .and_then(|key| {
                self.inner.lock().ok().map(|guard| {
                    guard
                        .layout
                        .provider_configs
                        .get(key)
                        .is_some_and(|cfg| cfg.self_hosted)
                })
            })
            .unwrap_or(false)
    }

    /// Whether [`Self::embedding_config`] resolves to the built-in gateway.
    /// Callers that accept endpoint overrides must ignore them in this case:
    /// the key is shared rather than the user's own, so honouring an override
    /// would let any webview redirect it to an arbitrary host.
    pub fn embedding_is_builtin(&self) -> bool {
        self.inner
            .lock()
            .is_ok_and(|guard| embedding_uses_builtin(&guard.embedding))
    }

    /// Resolve the embedding endpoint (base URL, API key, model): the built-in
    /// gateway unless the source is custom. None when a custom endpoint is
    /// incomplete, which is what disables arXiv recommendations.
    pub fn embedding_config(&self) -> Option<(String, Option<String>, String)> {
        let guard = self.inner.lock().ok()?;
        if embedding_uses_builtin(&guard.embedding) {
            let status = builtin::status();
            return Some((
                status.base_url,
                builtin::api_key().map(str::to_string),
                status.embedding_model,
            ));
        }
        let base_url = guard.embedding.base_url.trim();
        let model = guard.embedding.model.trim();
        if base_url.is_empty() || model.is_empty() {
            return None;
        }
        let api_key = guard.embedding.api_key.trim();
        let key = if api_key.is_empty() || is_translate_api_key_mask(api_key) {
            None
        } else {
            Some(api_key.to_string())
        };
        Some((base_url.to_string(), key, model.to_string()))
    }

    /// Decision-layer System One credentials. Returns None when the API key is
    /// unset or is a UI mask (`*`-only), so probes never send masks.
    pub fn decision_config(&self) -> Option<SystemOneCredentials> {
        let guard = self.inner.lock().ok()?;
        let api_key = guard.decision.api_key.trim();
        if api_key.is_empty() || is_translate_api_key_mask(api_key) {
            return None;
        }
        Some(SystemOneCredentials {
            api_key: api_key.to_string(),
            base_url: guard.decision.base_url.trim().to_string(),
            model: guard.decision.model.trim().to_string(),
        })
    }

    /// Smart-highlight concurrency and per-batch timeout, after normalization.
    pub fn smart_highlight_limits(&self) -> (u32, u32) {
        let Ok(guard) = self.inner.lock() else {
            return (
                default_smart_highlight_concurrency(),
                default_smart_highlight_timeout_secs(),
            );
        };
        (
            guard.decision.smart_highlight_concurrency,
            guard.decision.smart_highlight_timeout_secs,
        )
    }

    /// Raw `(tunnel_id, api_key)` for the built-in ChatGPT tunnel supervisor.
    /// Returns None unless both are set; a UI mask counts as unset so a
    /// `settings_get` → `settings_set` round-trip never leaks or wipes the key.
    pub fn mcp_tunnel_config(&self) -> Option<(String, String)> {
        let guard = self.inner.lock().ok()?;
        let id = guard.mcp_tunnel_id.trim().to_string();
        let key = guard.mcp_tunnel_api_key.trim();
        if id.is_empty() || key.is_empty() || is_translate_api_key_mask(key) {
            return None;
        }
        Some((id, key.to_string()))
    }

    /// Resolve the configured institution proxy (prefix, cookie). None when
    /// the prefix is unset.
    pub fn institution_proxy(
        &self,
    ) -> Option<(
        agentero_core::features::paper::import::institution_proxy::InstitutionProxyKind,
        String,
        String,
    )> {
        let guard = self.inner.lock().ok()?;
        let prefix = guard.institution_proxy_prefix.trim();
        if prefix.is_empty() {
            return None;
        }
        let kind =
            agentero_core::features::paper::import::institution_proxy::InstitutionProxyKind::from_setting(
                &guard.institution_proxy_type,
            );
        Some((
            kind,
            prefix.to_string(),
            guard.institution_proxy_cookie.trim().to_string(),
        ))
    }

    /// Persist cookies captured after an interactive gateway login without
    /// overwriting unrelated settings changed by another window meanwhile.
    pub fn update_institution_proxy_cookie(
        &self,
        prefix: &str,
        cookie: String,
    ) -> Result<Option<AppSettings>, AppError> {
        let lock = crate::core::fs::SidecarLock::acquire(&self.path)?;
        self.ensure_disk_revision_is_current()?;
        let redacted = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| AppError::message("settings lock poisoned"))?;
            if guard.institution_proxy_prefix.trim() != prefix.trim() {
                return Ok(None);
            }
            let mut next = guard.clone();
            next.institution_proxy_cookie = cookie;
            persist(&self.path, &next)?;
            if let Ok(mut revision) = self.last_disk_revision.lock() {
                *revision = disk_revision(&self.path).ok().flatten();
            }
            *guard = next.clone();
            redact_secrets(next)
        };
        drop(lock);
        if let Some((kind, prefix, cookie)) = self.institution_proxy() {
            agentero_core::features::paper::import::institution_proxy::set_institution_proxy(
                kind, &prefix, &cookie,
            );
        }
        self.notify(&redacted);
        Ok(Some(redacted))
    }

    /// Resolve the configured EasyScholar key. Returns None when unset or when
    /// the stored value is a UI mask (`*`-only), so probes never send masks.
    pub fn easy_scholar_key(&self) -> Option<String> {
        let guard = self.inner.lock().ok()?;
        let key = guard.easy_scholar_key.trim();
        if key.is_empty() || is_translate_api_key_mask(key) {
            None
        } else {
            Some(key.to_string())
        }
    }

    /// 解析已配置的知乎 Access Secret。未配置或仍是 UI 掩码时返回 None。
    pub fn zhihu_access_secret(&self) -> Option<String> {
        let guard = self.inner.lock().ok()?;
        let key = guard.zhihu_access_secret.trim();
        if key.is_empty() || is_translate_api_key_mask(key) {
            None
        } else {
            Some(key.to_string())
        }
    }
}

fn ensure_settings_file_supported(path: &Path) -> Result<(), AppError> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let value = serde_json::from_slice(&raw)?;
    if let Some(key) = agentero_core::settings_schema::unsupported_settings_field(&value)? {
        return Err(AppError::domain(
            "settings_unsupported_field",
            format!("settings contain an unsupported field {key}; refusing to discard it"),
        ));
    }
    Ok(())
}

fn merge_settings_edits(
    mut base: AppSettings,
    mut incoming: AppSettings,
    previous: &AppSettings,
) -> Result<AppSettings, AppError> {
    normalize(&mut base);
    normalize(&mut incoming);
    let base = serde_json::to_value(base)?;
    let incoming = serde_json::to_value(incoming)?;
    let mut latest = serde_json::to_value(redact_secrets(previous.clone()))?;
    let target = latest
        .as_object_mut()
        .ok_or_else(|| AppError::message("invalid settings snapshot"))?;
    let input = incoming
        .as_object()
        .ok_or_else(|| AppError::message("invalid settings snapshot"))?;
    for (key, value) in input {
        if key == "frontendPreferences" || base.get(key) == Some(value) {
            continue;
        }
        if target.get(key) != base.get(key) && target.get(key) != Some(value) {
            return Err(AppError::message(format!(
                "setting {key} changed externally; retry"
            )));
        }
        target.insert(key.clone(), value.clone());
    }
    serde_json::from_value(latest).map_err(Into::into)
}

fn disk_revision(path: &Path) -> Result<Option<String>, AppError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(hex::encode(Sha256::digest(bytes)))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_file(path: &PathBuf) -> (AppSettings, bool) {
    if !path.is_file() {
        return (AppSettings::default(), false);
    }
    match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<AppSettings>(&raw) {
            Ok(mut s) => {
                normalize(&mut s);
                sync_institution_proxy(&s);
                (s, true)
            }
            Err(e) => {
                log::warn!(
                    target: "agentero::settings",
                    "invalid settings.json ({}): {e}; using defaults",
                    path.display()
                );
                (AppSettings::default(), true)
            }
        },
        Err(e) => {
            log::warn!(
                target: "agentero::settings",
                "failed to read settings.json: {e}"
            );
            (AppSettings::default(), false)
        }
    }
}

fn persist(path: &Path, settings: &AppSettings) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Owner-only (0o600 on Unix) when secrets (BYOK keys) may live in this JSON.
    crate::core::fs::json_store_with(path, settings, &crate::core::fs::AtomicOpts::OWNER_ONLY)
}

/// Map Host translate provider id (any case) → settings `providerConfigs` key.
pub fn commercial_provider_settings_key(provider: &str) -> Option<&'static str> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "deepl" => Some("deepl"),
        "azure" => Some("azure"),
        "googlecloud" => Some("googleCloud"),
        "openaicompatible" => Some("openaiCompatible"),
        _ => None,
    }
}

/// Map layout provider id (any case) → settings `layout.providerConfigs` key.
pub fn layout_provider_settings_key(provider: &str) -> Option<&'static str> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "paddle" => Some("paddle"),
        "mineru" => Some("mineru"),
        "openaicompatible" => Some("openaiCompatible"),
        // No stored card: the getters above resolve it from the build, but the
        // parser registry still needs a stable credentials-map key.
        BUILTIN_PROVIDER_ID => Some(BUILTIN_PROVIDER_ID),
        _ => None,
    }
}

/// Replace non-empty secrets with same-length `*` masks.
fn redact_secrets(mut settings: AppSettings) -> AppSettings {
    for cfg in settings.translate.provider_configs.values_mut() {
        if !cfg.api_key.trim().is_empty() {
            cfg.api_key = mask_translate_api_key(&cfg.api_key);
        }
    }
    for cfg in settings.layout.provider_configs.values_mut() {
        if !cfg.api_key.trim().is_empty() {
            cfg.api_key = mask_translate_api_key(&cfg.api_key);
        }
    }
    if !settings.embedding.api_key.trim().is_empty() {
        settings.embedding.api_key = mask_translate_api_key(&settings.embedding.api_key);
    }
    if !settings.mcp_tunnel_api_key.trim().is_empty() {
        settings.mcp_tunnel_api_key = mask_translate_api_key(&settings.mcp_tunnel_api_key);
    }
    if !settings.easy_scholar_key.trim().is_empty() {
        settings.easy_scholar_key = mask_translate_api_key(&settings.easy_scholar_key);
    }
    if !settings.zhihu_access_secret.trim().is_empty() {
        settings.zhihu_access_secret = mask_translate_api_key(&settings.zhihu_access_secret);
    }
    if !settings.decision.api_key.trim().is_empty() {
        settings.decision.api_key = mask_translate_api_key(&settings.decision.api_key);
    }
    settings
}

/// Apply incoming configs while preserving secrets when the UI sends the mask.
fn merge_secrets(incoming: &mut AppSettings, previous: &AppSettings) {
    for (id, cfg) in incoming.translate.provider_configs.iter_mut() {
        if is_translate_api_key_mask(&cfg.api_key) {
            if let Some(prev) = previous.translate.provider_configs.get(id) {
                cfg.api_key = prev.api_key.clone();
            } else {
                // Mask with no prior secret → treat as unset.
                cfg.api_key.clear();
            }
        }
    }
    for (id, cfg) in incoming.layout.provider_configs.iter_mut() {
        if is_translate_api_key_mask(&cfg.api_key) {
            if let Some(prev) = previous.layout.provider_configs.get(id) {
                cfg.api_key = prev.api_key.clone();
            } else {
                cfg.api_key.clear();
            }
        }
    }
    if is_translate_api_key_mask(&incoming.embedding.api_key) {
        incoming.embedding.api_key = previous.embedding.api_key.clone();
    }
    if is_translate_api_key_mask(&incoming.mcp_tunnel_api_key) {
        incoming.mcp_tunnel_api_key = previous.mcp_tunnel_api_key.clone();
    }
    if is_translate_api_key_mask(&incoming.easy_scholar_key) {
        incoming.easy_scholar_key = previous.easy_scholar_key.clone();
    }
    if is_translate_api_key_mask(&incoming.zhihu_access_secret) {
        incoming.zhihu_access_secret = previous.zhihu_access_secret.clone();
    }
    if is_translate_api_key_mask(&incoming.decision.api_key) {
        incoming.decision.api_key = previous.decision.api_key.clone();
    }
}

/// Cap a translate template and drop placeholders other than
/// `{{targetLang}}` / `{{sourceLang}}` / `{{text}}`. Does not trim.
fn cap_translate_template(raw: &str) -> String {
    agentero_core::features::translate::strip_unknown_translate_placeholders(raw)
        .chars()
        .take(8000)
        .collect()
}

fn sync_institution_proxy(settings: &AppSettings) {
    use agentero_core::features::paper::import::institution_proxy::{
        set_institution_proxy, InstitutionProxyKind,
    };
    set_institution_proxy(
        InstitutionProxyKind::from_setting(&settings.institution_proxy_type),
        &settings.institution_proxy_prefix,
        &settings.institution_proxy_cookie,
    );
}

fn normalize(s: &mut AppSettings) {
    if s.connector_port == 0 {
        s.connector_port = default_connector_port();
    }
    if s.mcp_port == 0 {
        s.mcp_port = default_mcp_port();
    }
    s.mcp_tunnel_id = s
        .mcp_tunnel_id
        .split_whitespace()
        .collect::<String>()
        .to_ascii_lowercase();
    s.mcp_tunnel_api_key = s.mcp_tunnel_api_key.trim().to_string();
    s.easy_scholar_key = s.easy_scholar_key.trim().to_string();
    s.zhihu_access_secret = s.zhihu_access_secret.trim().to_string();
    s.decision.api_key = s.decision.api_key.trim().to_string();
    let provider = s.decision.provider.trim().to_ascii_lowercase();
    s.decision.provider = if provider.is_empty() {
        default_decision_provider()
    } else {
        provider
    };
    let decision_url = s.decision.base_url.trim().trim_end_matches('/');
    // Only the default jEV preset backfills the endpoint; Clef/OpenAI/custom
    // keep whatever the user typed (empty = not configured yet).
    s.decision.base_url = if decision_url.is_empty() && s.decision.provider == "jev" {
        default_decision_base_url()
    } else {
        decision_url.to_string()
    };
    s.decision.model = s.decision.model.trim().to_string();
    if s.decision.model.is_empty() && s.decision.provider == "jev" {
        s.decision.model = default_decision_model();
    }
    if !SMART_HIGHLIGHT_CONCURRENCY_CHOICES.contains(&s.decision.smart_highlight_concurrency) {
        s.decision.smart_highlight_concurrency = default_smart_highlight_concurrency();
    }
    s.decision.smart_highlight_timeout_secs = s.decision.smart_highlight_timeout_secs.clamp(
        SMART_HIGHLIGHT_TIMEOUT_MIN_SECS,
        SMART_HIGHLIGHT_TIMEOUT_MAX_SECS,
    );
    if s.batch_import_concurrency < 1 || s.batch_import_concurrency > 10 {
        s.batch_import_concurrency = default_batch_import_concurrency();
    }
    if !matches!(
        s.translate.dictionary.mode.as_str(),
        "auto" | "offline-first" | "off"
    ) {
        s.translate.dictionary.mode = default_dictionary_mode();
    }
    if s.translate.auto_translate_import_concurrency < 1
        || s.translate.auto_translate_import_concurrency > MAX_AUTO_TRANSLATE_IMPORT_CONCURRENCY
    {
        s.translate.auto_translate_import_concurrency = default_auto_translate_import_concurrency();
    }
    s.network_proxy_url = s.network_proxy_url.trim().to_string();
    if s.network_proxy_url.is_empty() {
        s.network_proxy_url = default_network_proxy_url();
    }
    s.institution_proxy_prefix = s
        .institution_proxy_prefix
        .trim()
        .trim_end_matches('/')
        .to_string();
    s.institution_proxy_cookie = s.institution_proxy_cookie.trim().to_string();
    s.institution_proxy_type = if s.institution_proxy_type.eq_ignore_ascii_case("wengine") {
        "wengine".to_string()
    } else if s.institution_proxy_type.eq_ignore_ascii_case("subdomain") {
        "subdomain".to_string()
    } else if s.institution_proxy_type.eq_ignore_ascii_case("ezproxy") {
        "ezproxy".to_string()
    } else {
        "auto".to_string()
    };
    const LABEL_MODES: &[&str] = &["title-author", "title", "author-year-title", "folder"];
    if !LABEL_MODES.contains(&s.paper_tree_label_mode.as_str()) {
        s.paper_tree_label_mode = default_paper_tree_label_mode();
    }
    const SORT_MODES: &[&str] = &[
        "folder",
        "title",
        "author",
        "year-desc",
        "year-asc",
        "added-desc",
    ];
    if !SORT_MODES.contains(&s.paper_tree_sort_mode.as_str()) {
        s.paper_tree_sort_mode = default_paper_tree_sort_mode();
    }
    const NOTE_MODES: &[&str] = &["standard", "title-only", "blank", "custom"];
    if !NOTE_MODES.contains(&s.paper_note_mode.as_str()) {
        s.paper_note_mode = default_paper_note_mode();
    }
    const AUTO_UPDATE_INTERNAL_LINKS: &[&str] = &["ask", "always"];
    if !AUTO_UPDATE_INTERNAL_LINKS.contains(&s.auto_update_internal_links.as_str()) {
        s.auto_update_internal_links = default_auto_update_internal_links();
    }
    // Unknown ids are kept: the launch path falls back to the system default,
    // and the settings pane can still show the saved app as unavailable.
    s.terminal_app = s.terminal_app.trim().to_ascii_lowercase();
    const PDF_SCROLL_STRATEGIES: &[&str] = &["vertical", "horizontal", "paginated"];
    if !PDF_SCROLL_STRATEGIES.contains(&s.pdf_scroll_strategy.as_str()) {
        s.pdf_scroll_strategy = default_pdf_scroll_strategy();
    }
    const PDF_SPREAD_MODES: &[&str] = &["none", "odd", "even"];
    if !PDF_SPREAD_MODES.contains(&s.pdf_spread_mode.as_str()) {
        s.pdf_spread_mode = default_pdf_spread_mode();
    }
    if !["hover", "always", "hidden"].contains(&s.pdf_translate_tab.as_str()) {
        s.pdf_translate_tab = default_pdf_translate_tab();
    }

    // Library columns: drop unknown/duplicate keys, append missing ones
    // with their default visibility, and keep `title` visible.
    let mut seen: Vec<String> = Vec::new();
    let mut cols: Vec<LibraryColumnPref> = Vec::new();
    for col in s.library_columns.drain(..) {
        // `year` became a full publication date; keep the saved position.
        let key = if col.key == "year" {
            "date".to_string()
        } else {
            col.key
        };
        let is_custom_prop = key.starts_with("prop:");
        if !LIBRARY_COLUMN_KEYS.contains(&key.as_str()) && !is_custom_prop {
            continue;
        }
        if seen.iter().any(|k| k == &key) {
            continue;
        }
        seen.push(key.clone());
        cols.push(LibraryColumnPref {
            key,
            visible: col.visible,
            width_rem: col
                .width_rem
                .filter(|w| w.is_finite() && *w > 0.0)
                .map(|w| w.clamp(5.0, 120.0)),
        });
    }
    for fallback in default_library_columns() {
        if !seen.iter().any(|key| key == &fallback.key) {
            cols.push(fallback);
        }
    }
    for col in cols.iter_mut() {
        if col.key == "title" {
            col.visible = true;
        }
    }
    s.library_columns = cols;

    const THEMES: &[&str] = &["system", "light", "dark"];
    if !THEMES.contains(&s.theme.as_str()) {
        s.theme = default_theme();
    }
    s.ui_theme = s.ui_theme.trim().to_string();
    if s.ui_theme.is_empty() {
        s.ui_theme = default_ui_theme();
    }
    const LOCALES: &[&str] = &["system", "en", "zh-CN"];
    if !LOCALES.contains(&s.locale.as_str()) {
        s.locale = default_locale();
    }
    if s.editor_font_size < 10 || s.editor_font_size > 32 {
        s.editor_font_size = default_editor_font_size();
    }
    if !(12..=20).contains(&s.wiki_embed_font_size) {
        s.wiki_embed_font_size = default_wiki_embed_font_size();
    }
    s.markdown_max_width = normalize_markdown_max_width(s.markdown_max_width);
    s.interface_font_family = normalize_font_family_value(&s.interface_font_family);
    s.text_font_family = normalize_font_family_value(&s.text_font_family);
    s.mono_font_family = normalize_font_family_value(&s.mono_font_family);
    // Migrate deprecated editorFontFamily preset into textFontFamily once.
    let legacy = normalize_font_family_value(&s.editor_font_family);
    if s.text_font_family.is_empty() && !legacy.is_empty() {
        s.text_font_family = legacy;
    }
    s.editor_font_family.clear();
    if !s.editor_line_height.is_finite() || s.editor_line_height < 1.4 || s.editor_line_height > 2.0
    {
        s.editor_line_height = default_editor_line_height();
    } else {
        // Snap to 0.1 steps to match the frontend slider.
        s.editor_line_height = (s.editor_line_height * 10.0).round() / 10.0;
    }
    const UI_SCALE_PRESETS: &[f64] = &[0.8, 0.9, 1.0, 1.25, 1.5];
    if !s.ui_scale.is_finite() {
        s.ui_scale = default_ui_scale();
    } else {
        let mut closest = UI_SCALE_PRESETS[0];
        let mut best = f64::INFINITY;
        for &preset in UI_SCALE_PRESETS {
            let d = (preset - s.ui_scale).abs();
            if d < best {
                best = d;
                closest = preset;
            }
        }
        s.ui_scale = closest;
    }
    const PERMS: &[&str] = &["restricted", "ask", "auto"];
    if !PERMS.contains(&s.agent_permission_mode.as_str()) {
        s.agent_permission_mode = default_permission_mode();
    }
    const AI_LANGS: &[&str] = &["auto", "en", "zh-CN"];
    if !AI_LANGS.contains(&s.ai_response_language.as_str()) {
        s.ai_response_language = default_ai_response_language();
    }

    s.pdf_ask.agent_id = s.pdf_ask.agent_id.trim().to_string();
    s.pdf_ask.model_id = s.pdf_ask.model_id.trim().to_string();
    s.paper_reader.agent_id = s.paper_reader.agent_id.trim().to_string();
    s.paper_reader.model_id = s.paper_reader.model_id.trim().to_string();

    // Keep trailing slashes on base_url (same reason as translate/layout configs):
    // normalize runs on every save and echoes back into the settings UI.
    s.embedding.base_url = s.embedding.base_url.trim().to_string();
    s.embedding.api_key = s.embedding.api_key.trim().to_string();
    s.embedding.model = s.embedding.model.trim().to_string();
    s.embedding.source = resolve_embedding_source(&s.embedding);

    normalize_translate_provider_configs(&mut s.translate.provider_configs);
    s.translate.agent_id = s.translate.agent_id.trim().to_string();
    s.translate.model_id = s.translate.model_id.trim().to_string();
    const TR_TARGETS: &[&str] = &["ui", "en", "zh-CN"];
    if !TR_TARGETS.contains(&s.translate.target_lang.as_str()) {
        s.translate.target_lang = default_translate_target();
    }
    if s.translate.source_lang != "auto" {
        s.translate.source_lang = default_translate_source();
    }
    // Cap hand-edited oversized prompts and drop unknown `{{…}}` tokens. The
    // value is echoed to a billed API on every request. No trim: leading
    // whitespace may be intentional (the settings UI trims on blur).
    s.translate.custom_prompt = cap_translate_template(&s.translate.custom_prompt);
    s.translate.custom_system = cap_translate_template(&s.translate.custom_system);
    s.translate.custom_user = cap_translate_template(&s.translate.custom_user);

    const LAYOUT_BACKENDS: &[&str] = &["local", "paddle", "mineru"];
    if !LAYOUT_BACKENDS.contains(&s.layout.backend.as_str()) {
        s.layout.backend = default_layout_backend();
    }
    // The whitelist is what makes a selection stick: normalize runs on every
    // save, so an unlisted parser backend is silently reset and persisted.
    const PARSER_BACKENDS: &[&str] = &[
        "local",
        "paddle",
        "mineru",
        "openaiCompatible",
        BUILTIN_PROVIDER_ID,
    ];
    if !PARSER_BACKENDS.contains(&s.layout.parser_backend.as_str()) {
        s.layout.parser_backend = default_parser_backend();
    }
    normalize_layout_provider_configs(&mut s.layout.provider_configs);
    s.dismissed_reminders =
        normalize_dismissed_reminders(std::mem::take(&mut s.dismissed_reminders));
}

/// Known config-reminder ids; anything else is dropped so a hand-edited file
/// cannot smuggle stale ids into the frontend.
const DISMISSED_REMINDER_IDS: &[&str] = &["layout-local-model", "network-proxy"];

fn normalize_dismissed_reminders(raw: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in raw {
        let id = id.trim();
        if id.is_empty() || !DISMISSED_REMINDER_IDS.contains(&id) || out.iter().any(|k| k == id) {
            continue;
        }
        out.push(id.to_string());
    }
    out
}

/// Migration rule for `EmbeddingSettings::source`: an explicit value wins,
/// otherwise any populated BYOK field (an all-`*` mask counts as populated)
/// implies a custom endpoint. An all-empty triple uses the built-in endpoint
/// only in builds that actually carry the compiled-in key; source builds without
/// that key fall back to `custom` so the settings UI does not show an unusable
/// built-in provider.
fn resolve_embedding_source(settings: &EmbeddingSettings) -> String {
    let explicit = settings.source.trim().to_ascii_lowercase();
    if explicit == EMBEDDING_SOURCE_BUILTIN || explicit == EMBEDDING_SOURCE_CUSTOM {
        return explicit;
    }
    let configured = !settings.base_url.trim().is_empty()
        || !settings.api_key.trim().is_empty()
        || !settings.model.trim().is_empty();
    if configured || !builtin::available() {
        EMBEDDING_SOURCE_CUSTOM.to_string()
    } else {
        EMBEDDING_SOURCE_BUILTIN.to_string()
    }
}

/// Anything but an explicit custom endpoint uses the built-in gateway — but
/// only when this build actually carries a key, so a build without one keeps
/// the stored-values behaviour (and `recommend.no_embedding`).
fn embedding_uses_builtin(settings: &EmbeddingSettings) -> bool {
    settings.source.trim() != EMBEDDING_SOURCE_CUSTOM && builtin::available()
}

fn normalize_layout_provider_configs(configs: &mut HashMap<String, LayoutProviderConfig>) {
    const PROVIDERS: &[&str] = &["paddle", "mineru", "openaiCompatible"];
    // MinerU `language` vocabulary (API v4); anything else resets to the
    // Chinese-English default so the request never carries an unknown pack id.
    const MINERU_LANGUAGES: &[&str] = &[
        "ch",
        "ch_server",
        "en",
        "japan",
        "korean",
        "chinese_cht",
        "ta",
        "te",
        "ka",
        "el",
        "th",
        "latin",
        "arabic",
        "cyrillic",
        "east_slavic",
        "devanagari",
    ];
    configs.retain(|k, _| PROVIDERS.contains(&k.as_str()));
    for cfg in configs.values_mut() {
        cfg.api_key = cfg.api_key.trim().to_string();
        // Keep trailing slashes (same reason as translate configs): normalize
        // runs on every save and echoes back into the settings UI.
        cfg.base_url = cfg.base_url.trim().to_string();
        cfg.model = cfg.model.trim().to_string();
        cfg.prompt = cfg.prompt.trim().to_string();
        cfg.language = cfg.language.trim().to_string();
        if !MINERU_LANGUAGES.contains(&cfg.language.as_str()) {
            cfg.language = "ch".to_string();
        }
    }
}

fn normalize_translate_provider_configs(configs: &mut HashMap<String, TranslateProviderConfig>) {
    const COMMERCIAL: &[&str] = &["deepl", "azure", "googleCloud", "openaiCompatible"];
    configs.retain(|k, _| COMMERCIAL.contains(&k.as_str()));
    for cfg in configs.values_mut() {
        cfg.api_key = cfg.api_key.trim().to_string();
        // Keep trailing slashes: this runs on every save and the result is
        // echoed back into the settings UI, so stripping "/" would make it
        // impossible to type paths like ".../v1". Endpoints trim on use.
        cfg.base_url = cfg.base_url.trim().to_string();
        cfg.region = cfg.region.trim().to_string();
        cfg.model = cfg.model.trim().to_string();
        if cfg.concurrency < 1 || cfg.concurrency > MAX_TRANSLATE_CONCURRENCY {
            cfg.concurrency = default_translate_concurrency();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn captured_proxy_cookie_updates_only_matching_gateway() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppSettingsStore {
            inner: Mutex::new(AppSettings {
                institution_proxy_prefix: "https://vpn.example.edu".into(),
                institution_proxy_cookie: "old=1".into(),
                theme: "dark".into(),
                ..AppSettings::default()
            }),
            path: dir.path().join("settings.json"),
            last_disk_revision: Mutex::new(None),
            listeners: Mutex::new(Vec::new()),
        };
        assert!(store
            .update_institution_proxy_cookie("https://other.example.edu", "wrong=1".into())
            .unwrap()
            .is_none());
        store
            .update_institution_proxy_cookie("https://vpn.example.edu", "fresh=1".into())
            .unwrap();
        let (saved, _) = read_file(&store.path);
        assert_eq!(saved.institution_proxy_cookie, "fresh=1");
        assert_eq!(saved.theme, "dark");
    }

    #[test]
    fn roundtrip_defaults() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agentero-settings-test-{n}"));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        let s = AppSettings::default();
        persist(&path, &s).expect("write");
        let (loaded, existed) = read_file(&path);
        assert!(existed);
        assert_eq!(loaded.theme, "system");
        assert_eq!(loaded.auto_update_internal_links, "ask");
        assert_eq!(loaded.paper_note_mode, "standard");
        assert!(loaded.vim_mode);
        assert!(!loaded.onboarding_done);
        assert!(!loaded.feature_tour_done);
        // Persisted wire shape is camelCase: the settings window sends
        // `vimMode` and expects it back, otherwise the switch snaps back on.
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["vimMode"], serde_json::json!(true));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcut_overrides_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = AppSettings::default();
        settings.shortcut_overrides.insert(
            "workspace.search".into(),
            Some(ShortcutBinding {
                key: "K".into(),
                meta: Some(true),
                ctrl: Some(false),
                alt: Some(true),
                shift: Some(true),
            }),
        );
        settings
            .shortcut_overrides
            .insert("workspace.close".into(), None);
        settings.shortcut_overrides.insert(
            "future.command".into(),
            Some(ShortcutBinding {
                key: "F12".into(),
                ..Default::default()
            }),
        );
        persist(&path, &settings).expect("write");
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(json["shortcutOverrides"]["workspace.close"].is_null());
        assert_eq!(json["shortcutOverrides"]["workspace.search"]["ctrl"], false);
        assert_eq!(
            json["shortcutOverrides"]["future.command"],
            serde_json::json!({"key": "F12"})
        );
        let (loaded, existed) = read_file(&path);
        assert!(existed);
        assert_eq!(loaded.shortcut_overrides, settings.shortcut_overrides);
    }

    #[test]
    fn legacy_settings_default_to_empty_shortcut_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"theme":"dark","locale":"zh-CN"}"#).unwrap();
        let (loaded, existed) = read_file(&path);
        assert!(existed);
        assert!(loaded.shortcut_overrides.is_empty());
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.locale, "zh-CN");
    }

    #[test]
    fn onboarding_flags_roundtrip() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agentero-settings-onboarding-{n}"));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        let s = AppSettings {
            onboarding_done: true,
            feature_tour_done: true,
            ..AppSettings::default()
        };
        persist(&path, &s).expect("write");
        let raw = fs::read_to_string(&path).expect("read json");
        let json: serde_json::Value = serde_json::from_str(&raw).expect("parse json");
        assert_eq!(json["onboardingDone"], true);
        assert_eq!(json["featureTourDone"], true);
        let (loaded, existed) = read_file(&path);
        assert!(existed);
        assert!(loaded.onboarding_done);
        assert!(loaded.feature_tour_done);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_onboarding_flags_default_false() {
        let json = r#"{"theme":"dark"}"#;
        let s: AppSettings = serde_json::from_str(json).expect("deserialize");
        assert!(!s.onboarding_done);
        assert!(!s.feature_tour_done);
        assert_eq!(s.theme, "dark");
    }

    #[test]
    fn dismissed_reminders_whitelist() {
        let mut s = AppSettings {
            dismissed_reminders: vec![
                "layout-local-model".into(),
                "unknown".into(),
                "layout-local-model".into(),
                "  network-proxy ".into(),
            ],
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(
            s.dismissed_reminders,
            vec!["layout-local-model", "network-proxy"]
        );
    }

    #[test]
    fn redact_and_merge_api_keys() {
        let mut previous = AppSettings::default();
        previous.translate.provider_configs.insert(
            "deepl".into(),
            TranslateProviderConfig {
                api_key: "sk-secret".into(),
                ..Default::default()
            },
        );

        let redacted = redact_secrets(previous.clone());
        assert_eq!(
            redacted
                .translate
                .provider_configs
                .get("deepl")
                .map(|c| c.api_key.as_str()),
            Some("*********") // "sk-secret".chars().count()
        );
        assert!(is_translate_api_key_mask("*********"));
        assert!(!is_translate_api_key_mask("sk-secret"));

        let mut incoming = redacted;
        merge_secrets(&mut incoming, &previous);
        assert_eq!(
            incoming
                .translate
                .provider_configs
                .get("deepl")
                .map(|c| c.api_key.as_str()),
            Some("sk-secret")
        );

        // Explicit empty clears the secret.
        incoming
            .translate
            .provider_configs
            .get_mut("deepl")
            .unwrap()
            .api_key
            .clear();
        merge_secrets(&mut incoming, &previous);
        assert_eq!(
            incoming
                .translate
                .provider_configs
                .get("deepl")
                .map(|c| c.api_key.as_str()),
            Some("")
        );

        // MCP tunnel runtime key follows the same mask round-trip.
        let mut with_tunnel = AppSettings {
            mcp_tunnel_id: " Tunnel_ABC ".into(),
            mcp_tunnel_api_key: "  sk-tunnel-secret ".into(),
            ..AppSettings::default()
        };
        normalize(&mut with_tunnel);
        assert_eq!(with_tunnel.mcp_tunnel_id, "tunnel_abc");
        assert_eq!(with_tunnel.mcp_tunnel_api_key, "sk-tunnel-secret");

        let redacted_tunnel = redact_secrets(with_tunnel.clone());
        assert!(is_translate_api_key_mask(
            &redacted_tunnel.mcp_tunnel_api_key
        ));
        let mut echoed = redacted_tunnel;
        merge_secrets(&mut echoed, &with_tunnel);
        assert_eq!(echoed.mcp_tunnel_api_key, "sk-tunnel-secret");
    }

    #[test]
    fn translate_request_options_survive_settings_roundtrip() {
        let legacy: TranslateProviderConfig = serde_json::from_str(r#"{"model":"qwen3"}"#).unwrap();
        assert!(legacy.extra_request_body.is_empty());
        let mut settings = AppSettings::default();
        settings.translate.provider_configs.insert(
            "openaiCompatible".into(),
            TranslateProviderConfig {
                extra_request_body: r#"{"chat_template_kwargs":{"enable_thinking":false}}"#.into(),
                ..legacy
            },
        );
        normalize(&mut settings);
        let decoded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            decoded.translate.provider_configs,
            settings.translate.provider_configs
        );
    }

    #[test]
    fn commercial_provider_key_mapping() {
        assert_eq!(
            commercial_provider_settings_key("googleCloud"),
            Some("googleCloud")
        );
        assert_eq!(
            commercial_provider_settings_key("GOOGLECLOUD"),
            Some("googleCloud")
        );
        assert_eq!(
            commercial_provider_settings_key("openaiCompatible"),
            Some("openaiCompatible")
        );
        assert_eq!(commercial_provider_settings_key("cnki"), None);
    }

    #[test]
    fn normalize_parser_backend_and_openai_compatible_config() {
        let mut s = AppSettings::default();
        s.layout.parser_backend = "bogus".into();
        s.layout.provider_configs.insert(
            "openaiCompatible".into(),
            LayoutProviderConfig {
                api_key: "  sk-x  ".into(),
                base_url: " https://api.siliconflow.cn/v1 ".into(),
                model: "  deepseek-ai/DeepSeek-OCR  ".into(),
                prompt: "  Convert to markdown.  ".into(),
                ..Default::default()
            },
        );
        s.layout
            .provider_configs
            .insert("unknown".into(), LayoutProviderConfig::default());
        normalize(&mut s);
        assert_eq!(s.layout.parser_backend, "local");
        assert!(!s.layout.provider_configs.contains_key("unknown"));
        let cfg = s.layout.provider_configs.get("openaiCompatible").unwrap();
        assert_eq!(cfg.api_key, "sk-x");
        assert_eq!(cfg.base_url, "https://api.siliconflow.cn/v1");
        assert_eq!(cfg.model, "deepseek-ai/DeepSeek-OCR");
        assert_eq!(cfg.prompt, "Convert to markdown.");

        let mut ok = AppSettings::default();
        ok.layout.parser_backend = "openaiCompatible".into();
        normalize(&mut ok);
        assert_eq!(ok.layout.parser_backend, "openaiCompatible");
    }

    #[test]
    fn layout_provider_key_accepts_openai_compatible() {
        assert_eq!(
            layout_provider_settings_key("OPENAICOMPATIBLE"),
            Some("openaiCompatible")
        );
    }

    #[test]
    fn normalize_layout_language_whitelist() {
        let mut s = AppSettings::default();
        s.layout.provider_configs.insert(
            "mineru".into(),
            LayoutProviderConfig {
                language: " japan ".into(),
                ..Default::default()
            },
        );
        normalize(&mut s);
        assert_eq!(
            s.layout.provider_configs.get("mineru").unwrap().language,
            "japan"
        );

        // Unknown / legacy value resets to the Chinese-English default.
        let mut s = AppSettings::default();
        s.layout.provider_configs.insert(
            "mineru".into(),
            LayoutProviderConfig {
                language: "auto".into(),
                ..Default::default()
            },
        );
        normalize(&mut s);
        assert_eq!(
            s.layout.provider_configs.get("mineru").unwrap().language,
            "ch"
        );
    }

    #[test]
    fn normalize_rejects_unknown_internal_link_rename_policy() {
        let mut s = AppSettings {
            auto_update_internal_links: "unsafe".into(),
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.auto_update_internal_links, "ask");
    }

    #[test]
    fn normalize_caps_translate_custom_prompt() {
        // Hand-edited oversized prompt is capped; empty and normal values pass.
        let mut s = AppSettings {
            translate: TranslateSettings {
                custom_prompt: "x".repeat(9000),
                ..Default::default()
            },
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.translate.custom_prompt.chars().count(), 8000);

        let mut s = AppSettings {
            translate: TranslateSettings {
                custom_prompt: "keep me".into(),
                ..Default::default()
            },
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.translate.custom_prompt, "keep me");
    }

    #[test]
    fn normalize_strips_unknown_translate_placeholders() {
        let mut s = AppSettings {
            translate: TranslateSettings {
                custom_prompt: "{{audience}} only".into(),
                custom_system: "Style {{tone}} for {{targetLang}}.".into(),
                custom_user: "<<<{{text}}>>> {{mood}}".into(),
                ..Default::default()
            },
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.translate.custom_prompt, " only");
        assert_eq!(s.translate.custom_system, "Style  for {{targetLang}}.");
        assert_eq!(s.translate.custom_user, "<<<{{text}}>>> ");
    }

    #[test]
    fn normalize_rejects_unknown_paper_note_mode() {
        let mut s = AppSettings {
            paper_note_mode: "fancy".into(),
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.paper_note_mode, "standard");
    }

    #[test]
    fn normalize_rejects_unknown_pdf_reading_modes() {
        let mut s = AppSettings {
            pdf_scroll_strategy: "diagonal".into(),
            pdf_spread_mode: "triple".into(),
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.pdf_scroll_strategy, "vertical");
        assert_eq!(s.pdf_spread_mode, "none");
    }

    #[test]
    fn normalize_preserves_paginated_pdf_mode() {
        let mut s = AppSettings {
            pdf_scroll_strategy: "paginated".into(),
            pdf_spread_mode: "odd".into(),
            ..AppSettings::default()
        };
        normalize(&mut s);
        assert_eq!(s.pdf_scroll_strategy, "paginated");
        assert_eq!(s.pdf_spread_mode, "odd");
    }

    #[test]
    fn normalize_reading_layout_prefs() {
        let mut invalid = AppSettings {
            wiki_embed_font_size: 99,
            markdown_max_width: 500,
            ..AppSettings::default()
        };
        normalize(&mut invalid);
        assert_eq!(invalid.wiki_embed_font_size, 13);
        assert_eq!(invalid.markdown_max_width, 0);

        let mut valid = AppSettings {
            wiki_embed_font_size: 18,
            markdown_max_width: 800,
            ..AppSettings::default()
        };
        normalize(&mut valid);
        assert_eq!(valid.wiki_embed_font_size, 18);
        assert_eq!(valid.markdown_max_width, 800);
    }

    #[test]
    fn set_fires_subscribers_with_redacted_snapshot() {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agentero-settings-sub-test-{n}"));
        let _ = fs::create_dir_all(&dir);
        let store = AppSettingsStore {
            inner: Mutex::new(AppSettings::default()),
            path: dir.join("settings.json"),
            last_disk_revision: Mutex::new(None),
            listeners: Mutex::new(Vec::new()),
        };
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        {
            let seen = Arc::clone(&seen);
            store.subscribe(move |s| {
                seen.lock().unwrap().push(
                    s.translate
                        .provider_configs
                        .get("deepl")
                        .map_or(String::new(), |c| c.api_key.clone()),
                );
            });
        }

        let mut incoming = AppSettings::default();
        incoming.translate.provider_configs.insert(
            "deepl".into(),
            TranslateProviderConfig {
                api_key: "sk-secret".into(),
                ..Default::default()
            },
        );
        let out = store.set(incoming).expect("set");

        // Listener fired exactly once with the redacted snapshot.
        assert_eq!(*seen.lock().unwrap(), vec!["*********".to_string()]);
        assert_eq!(
            out.translate
                .provider_configs
                .get("deepl")
                .map(|c| c.api_key.as_str()),
            Some("*********")
        );
        // The plaintext secret is what lands on disk.
        let (loaded, _) = read_file(&store.path);
        assert_eq!(
            loaded
                .translate
                .provider_configs
                .get("deepl")
                .map(|c| c.api_key.as_str()),
            Some("sk-secret")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_column_widths_roundtrip_and_normalize() {
        let old: LibraryColumnPref =
            serde_json::from_str(r#"{"key":"title","visible":true}"#).unwrap();
        assert_eq!(old.width_rem, None);
        let mut s = AppSettings::default();
        s.library_columns[0].width_rem = Some(25.5);
        s.library_columns[1].width_rem = Some(-2.0);
        s.library_columns[2].width_rem = Some(999.0);
        normalize(&mut s);
        let encoded = serde_json::to_string(&s).unwrap();
        let loaded: AppSettings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(loaded.library_columns[0].width_rem, Some(25.5));
        assert_eq!(loaded.library_columns[1].width_rem, None);
        assert_eq!(loaded.library_columns[2].width_rem, Some(120.0));
    }

    #[test]
    fn normalize_reconciles_library_columns() {
        let mut s = AppSettings {
            library_columns: vec![
                LibraryColumnPref {
                    key: "bogus".into(),
                    width_rem: None,
                    visible: true,
                },
                LibraryColumnPref {
                    key: "title".into(),
                    width_rem: None,
                    visible: false,
                },
                LibraryColumnPref {
                    key: "year".into(),
                    width_rem: None,
                    visible: false,
                },
                LibraryColumnPref {
                    key: "prop:test_prop".into(),
                    width_rem: Some(9.0),
                    visible: true,
                },
            ],
            ..AppSettings::default()
        };
        normalize(&mut s);
        let keys: Vec<&str> = s.library_columns.iter().map(|c| c.key.as_str()).collect();
        // Unknown dropped; `year` renamed in place; custom prop kept; missing columns appended.
        assert_eq!(
            keys,
            vec![
                "title",
                "date",
                "prop:test_prop",
                "authors",
                "publication",
                "tags",
                "id",
                "citations",
                "addedAt"
            ]
        );
        // Title forced visible even though stored hidden.
        let title = s.library_columns.iter().find(|c| c.key == "title").unwrap();
        assert!(title.visible);
        // Non-title hidden preference preserved through the rename.
        let date = s.library_columns.iter().find(|c| c.key == "date").unwrap();
        assert!(!date.visible);
        // Appended column defaults to visible.
        let authors = s
            .library_columns
            .iter()
            .find(|c| c.key == "authors")
            .unwrap();
        assert!(authors.visible);
    }

    #[test]
    fn library_columns_added_date_persists_through_host_store() {
        let store = AppSettingsStore::for_tests(AppSettings::default());
        let mut legacy = AppSettings::default();
        legacy.library_columns.retain(|col| col.key != "addedAt");
        legacy.library_columns.reverse();
        legacy.library_columns[0].visible = false;
        let existing: Vec<_> = legacy
            .library_columns
            .iter()
            .map(|col| (col.key.clone(), col.visible))
            .collect();
        // Simulate an older settings.json, exercising the real disk load path.
        persist(&store.path, &legacy).expect("write legacy settings");
        let mut loaded = store
            .reload_if_changed()
            .expect("reload legacy settings through Host")
            .expect("observe the newly written settings file");
        let added = loaded.library_columns.last().unwrap();
        assert_eq!(added.key, "addedAt");
        assert!(!added.visible);
        assert_eq!(
            loaded.library_columns[..existing.len()]
                .iter()
                .map(|col| (col.key.clone(), col.visible))
                .collect::<Vec<_>>(),
            existing
        );

        // Enable and move the column, then save via the desktop Host store.
        let mut added = loaded.library_columns.pop().unwrap();
        added.visible = true;
        loaded.library_columns.insert(0, added);
        let expected = serde_json::to_value(&loaded.library_columns).unwrap();
        let returned = store.set(loaded).expect("save settings through Host");
        assert_eq!(
            serde_json::to_value(&returned.library_columns).unwrap(),
            expected
        );
        let (reloaded, _) = read_file(&store.path);
        assert_eq!(
            serde_json::to_value(&reloaded.library_columns).unwrap(),
            expected
        );
        fs::remove_file(&store.path).expect("remove test settings");
    }

    /// `agentero` is a parser (body-text) backend only. Layout analysis stays on
    /// the bundled offline PP-DocLayoutV3 model, so `LAYOUT_BACKENDS` must not
    /// grow the built-in id and `default_layout_backend` must not consult it.
    #[test]
    fn layout_backend_never_becomes_builtin() {
        assert_eq!(default_layout_backend(), "local");
        assert_eq!(AppSettings::default().layout.backend, "local");

        let mut s = AppSettings::default();
        s.layout.backend = BUILTIN_PROVIDER_ID.into();
        normalize(&mut s);
        assert_eq!(s.layout.backend, "local");
    }

    #[test]
    fn builtin_parser_backend_sticks_but_never_stores_a_key() {
        let mut s = AppSettings::default();
        s.layout.parser_backend = BUILTIN_PROVIDER_ID.into();
        s.layout.provider_configs.insert(
            BUILTIN_PROVIDER_ID.into(),
            LayoutProviderConfig {
                api_key: "sk-must-not-be-written".into(),
                base_url: "https://gateway.test/v1".into(),
                model: "some/ocr-model".into(),
                ..Default::default()
            },
        );
        normalize(&mut s);
        // Listed in PARSER_BACKENDS, so normalize does not reset it.
        assert_eq!(s.layout.parser_backend, BUILTIN_PROVIDER_ID);
        // Unlisted in the provider-config whitelist, so the card is dropped.
        assert!(s.layout.provider_configs.is_empty());

        let store = AppSettingsStore::for_tests(AppSettings::default());
        let mut incoming = AppSettings::default();
        incoming.layout.parser_backend = BUILTIN_PROVIDER_ID.into();
        incoming.layout.provider_configs.insert(
            BUILTIN_PROVIDER_ID.into(),
            LayoutProviderConfig {
                api_key: "sk-must-not-be-written".into(),
                ..Default::default()
            },
        );
        store.set(incoming).expect("set");
        let raw = fs::read_to_string(store.path()).expect("settings written");
        assert!(!raw.contains("sk-must-not-be-written"));
        // The secret that actually matters is the compiled-in one; guarded
        // because it exists only in a keyed build.
        if let Some(key) = builtin::api_key() {
            assert!(!raw.contains(key), "built-in key reached settings.json");
        }
        assert!(raw.contains(BUILTIN_PROVIDER_ID), "parserBackend kept");
        let (reloaded, existed) = read_file(store.path());
        assert!(existed);
        assert_eq!(reloaded.layout.parser_backend, BUILTIN_PROVIDER_ID);
        let _ = fs::remove_file(store.path());
    }

    #[test]
    fn layout_getters_resolve_builtin_without_a_provider_card() {
        let store = AppSettingsStore::for_tests(AppSettings::default());
        let status = builtin::status();
        assert_eq!(
            store.layout_base_url(BUILTIN_PROVIDER_ID).as_deref(),
            Some(status.base_url.as_str())
        );
        assert_eq!(
            store.layout_model(BUILTIN_PROVIDER_ID).as_deref(),
            Some(status.ocr_model.as_str())
        );
        assert_eq!(
            store.layout_api_key(BUILTIN_PROVIDER_ID).is_some(),
            builtin::available()
        );
        // Prompt/language/force-OCR stay unset: the VLM engine derives the
        // prompt from the model id, and the other two are MinerU-only.
        assert!(store.layout_prompt(BUILTIN_PROVIDER_ID).is_none());
        assert!(store.layout_language(BUILTIN_PROVIDER_ID).is_none());
        assert!(!store.layout_is_ocr(BUILTIN_PROVIDER_ID));
        assert_eq!(
            layout_provider_settings_key("Agentero"),
            Some(BUILTIN_PROVIDER_ID)
        );
    }

    #[test]
    fn embedding_source_is_inferred_only_when_unset() {
        let empty_default_source = if builtin::available() {
            "builtin"
        } else {
            "custom"
        };
        let mut fresh = AppSettings::default();
        normalize(&mut fresh);
        assert_eq!(fresh.embedding.source, empty_default_source);

        // Legacy file: BYOK fields populated, no `source` key at all.
        let mut legacy: AppSettings = serde_json::from_str(
            r#"{"embedding":{"baseUrl":"https://embed.test/v1","model":"bge-m3"}}"#,
        )
        .expect("deserialize");
        assert_eq!(legacy.embedding.source, "");
        normalize(&mut legacy);
        assert_eq!(legacy.embedding.source, "custom");

        // An explicit choice wins over inference from the other fields.
        let mut explicit = AppSettings::default();
        explicit.embedding.source = " BUILTIN ".into();
        explicit.embedding.model = "bge-m3".into();
        normalize(&mut explicit);
        assert_eq!(explicit.embedding.source, "builtin");

        // A `*` mask counts as configured, so a redacted round-trip stays custom.
        let mut masked = AppSettings::default();
        masked.embedding.api_key = "****".into();
        normalize(&mut masked);
        assert_eq!(masked.embedding.source, "custom");

        // Unknown values are re-inferred.
        let mut unknown = AppSettings::default();
        unknown.embedding.source = "gateway".into();
        normalize(&mut unknown);
        assert_eq!(unknown.embedding.source, empty_default_source);
    }

    #[test]
    fn embedding_config_prefers_builtin_unless_custom() {
        let status = builtin::status();
        let store = AppSettingsStore::for_tests(AppSettings::default());
        match store.embedding_config() {
            Some((base_url, api_key, model)) => {
                assert!(builtin::available(), "no key compiled in → no config");
                assert_eq!(base_url, status.base_url);
                assert_eq!(model, status.embedding_model);
                assert!(api_key.is_some());
            }
            None => {
                assert!(!builtin::available());
            }
        }

        let store = AppSettingsStore::for_tests(AppSettings {
            embedding: EmbeddingSettings {
                source: "custom".into(),
                base_url: "https://embed.test/v1".into(),
                api_key: "sk-custom-test".into(),
                model: "bge-m3".into(),
            },
            ..AppSettings::default()
        });
        assert_eq!(
            store.embedding_config(),
            Some((
                "https://embed.test/v1".into(),
                Some("sk-custom-test".into()),
                "bge-m3".into()
            ))
        );
    }

    #[test]
    fn embedding_is_builtin_tracks_the_resolved_endpoint() {
        // `probe_embedding` suppresses webview-supplied endpoint overrides
        // exactly when this is true; the two must never disagree.
        let store = AppSettingsStore::for_tests(AppSettings::default());
        assert_eq!(store.embedding_is_builtin(), builtin::available());

        let custom = AppSettingsStore::for_tests(AppSettings {
            embedding: EmbeddingSettings {
                source: "custom".into(),
                base_url: "https://embed.test/v1".into(),
                api_key: "sk-custom-test".into(),
                model: "bge-m3".into(),
            },
            ..AppSettings::default()
        });
        assert!(!custom.embedding_is_builtin());
        assert_eq!(
            custom.embedding_config().map(|(base, _, _)| base),
            Some("https://embed.test/v1".into())
        );
    }

    #[test]
    fn smart_highlight_limits_keep_supported_values() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.decision.smart_highlight_concurrency, 16);
        assert_eq!(settings.decision.smart_highlight_timeout_secs, 120);

        settings.decision.smart_highlight_concurrency = 3;
        settings.decision.smart_highlight_timeout_secs = 9;
        normalize(&mut settings);
        assert_eq!(settings.decision.smart_highlight_concurrency, 16);
        assert_eq!(settings.decision.smart_highlight_timeout_secs, 30);

        settings.decision.smart_highlight_concurrency = 4;
        settings.decision.smart_highlight_timeout_secs = 300;
        normalize(&mut settings);
        assert_eq!(settings.decision.smart_highlight_concurrency, 4);
        assert_eq!(settings.decision.smart_highlight_timeout_secs, 300);

        settings.decision.smart_highlight_timeout_secs = 9_000;
        normalize(&mut settings);
        assert_eq!(settings.decision.smart_highlight_timeout_secs, 600);
    }

    #[test]
    fn decision_config_never_returns_masked_key() {
        let store = AppSettingsStore::for_tests(AppSettings {
            decision: DecisionSettings {
                provider: "jev".into(),
                api_key: "sk-jev-secret".into(),
                base_url: "https://jev.test/v1".into(),
                model: "jev-latest".into(),
                smart_highlight: false,
                smart_highlight_concurrency: 16,
                smart_highlight_timeout_secs: 120,
            },
            ..AppSettings::default()
        });
        assert_eq!(
            store.decision_config(),
            Some(SystemOneCredentials {
                api_key: "sk-jev-secret".into(),
                base_url: "https://jev.test/v1".into(),
                model: "jev-latest".into(),
            })
        );

        // After redaction the getter must treat the mask as unset.
        let redacted = redact_secrets(AppSettings {
            decision: DecisionSettings {
                provider: "jev".into(),
                api_key: "sk-jev-secret".into(),
                base_url: "https://jev.test/v1".into(),
                model: "jev-latest".into(),
                smart_highlight: false,
                smart_highlight_concurrency: 16,
                smart_highlight_timeout_secs: 120,
            },
            ..AppSettings::default()
        });
        let masked_store = AppSettingsStore::for_tests(redacted);
        assert!(masked_store.decision_config().is_none());
    }

    #[test]
    fn fresh_install_defaults_use_local_parser() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (s, existed) = read_file(&path);
        assert!(!existed);
        assert_eq!(s.translate.provider, "tencenttransmart");
        assert_eq!(s.layout.parser_backend, "local");
        assert_eq!(s.layout.backend, "local");

        persist(&path, &s).expect("write");
        let (reloaded, existed) = read_file(&path);
        assert!(existed);
        assert_eq!(reloaded.layout, s.layout);
    }

    #[test]
    fn legacy_settings_without_parser_backend_use_local() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for (json, layout_backend) in [
            (r#"{}"#, "local"),
            (r#"{"layout":{"backend":"paddle"}}"#, "paddle"),
        ] {
            fs::write(&path, json).unwrap();
            let (loaded, existed) = read_file(&path);
            assert!(existed);
            assert_eq!(loaded.layout.parser_backend, "local");
            assert_eq!(loaded.layout.backend, layout_backend);
        }
    }

    #[test]
    fn dictionary_modes_preserve_settings_and_default_legacy_values() {
        for (raw, expected) in [
            (serde_json::json!({}), "auto"),
            (
                serde_json::json!({"dictionary": {"mode": "unknown"}}),
                "auto",
            ),
            (
                serde_json::json!({"dictionary": {"mode": "offline-first"}}),
                "offline-first",
            ),
            (serde_json::json!({"dictionary": {"mode": "off"}}), "off"),
        ] {
            let mut app = AppSettings {
                translate: serde_json::from_value(raw).unwrap(),
                ..AppSettings::default()
            };
            normalize(&mut app);
            let settings = app.translate;
            assert_eq!(settings.dictionary.mode, expected);
            let encoded = serde_json::to_value(&settings).unwrap();
            let restored: TranslateSettings = serde_json::from_value(encoded).unwrap();
            assert_eq!(restored, settings);
        }
    }

    #[test]
    fn translate_default_survives_settings_roundtrip() {
        for raw in [
            None,
            Some("{}"),
            Some(r#"{"translate":{"targetLang":"en"}}"#),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            if let Some(raw) = raw {
                fs::write(&path, raw).unwrap();
            }
            let (settings, existed) = read_file(&path);
            assert_eq!(existed, raw.is_some());
            assert_eq!(settings.translate.provider, "tencenttransmart");
            persist(&path, &settings).unwrap();
            let (reloaded, existed) = read_file(&path);
            assert!(existed);
            assert_eq!(reloaded.translate, settings.translate);
        }
    }

    #[test]
    fn explicit_translate_provider_survives_settings_roundtrip() {
        for provider in [BUILTIN_PROVIDER_ID, "deepl"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            fs::write(
                &path,
                serde_json::json!({"translate": {"provider": provider}}).to_string(),
            )
            .unwrap();
            let (settings, existed) = read_file(&path);
            assert!(existed);
            assert_eq!(settings.translate.provider, provider);
            persist(&path, &settings).unwrap();
            let (reloaded, _) = read_file(&path);
            assert_eq!(reloaded.translate.provider, provider);
        }
    }
}

/// Tauri command shells for this feature.
pub mod commands;
#[cfg(feature = "desktop")]
pub mod watcher;
