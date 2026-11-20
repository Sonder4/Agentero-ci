//! Shared serialized application-settings schema and defaults.
//!
//! The desktop Host owns normalization and side effects; the CLI uses this
//! schema to validate changes before writing the same configuration file.

use crate::decision::DEFAULT_SYSTEM_ONE_MODEL;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_NETWORK_PROXY_URL: &str = "http://127.0.0.1:7890";
pub const DEFAULT_DECISION_BASE_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const DEFAULT_CONNECTOR_PORT: u16 = 23119;
pub const DEFAULT_MCP_PORT: u16 = 8765;

/// Find non-null fields this version would discard on its next settings save.
pub fn unsupported_settings_field(
    value: &serde_json::Value,
) -> Result<Option<String>, serde_json::Error> {
    let settings: AppSettings = serde_json::from_value(value.clone())?;
    let serialized = serde_json::to_value(settings)?;
    let mut input = value.clone();
    if let Some(map) = input.as_object_mut() {
        // These legacy fields are deliberately migrated by the Host.
        map.remove("editorFontFamily");
        map.remove("jev");
    }
    Ok(find_unknown_field(&input, &serialized, ""))
}

fn find_unknown_field(
    input: &serde_json::Value,
    serialized: &serde_json::Value,
    path: &str,
) -> Option<String> {
    use serde_json::Value;
    match (input, serialized) {
        (Value::Object(input), Value::Object(serialized)) => {
            for (key, value) in input {
                let child = format!("{path}/{key}");
                match serialized.get(key) {
                    Some(expected) => {
                        if let Some(unknown) = find_unknown_field(value, expected, &child) {
                            return Some(unknown);
                        }
                    }
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
            .find_map(|(index, (item, expected))| {
                find_unknown_field(item, expected, &format!("{path}/{index}"))
            }),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Preferences formerly stored only in the desktop WebView. Values are
    /// JSON objects or arrays owned by the corresponding frontend feature.
    #[serde(default)]
    #[specta(type = HashMap<String, crate::json::Json>)]
    pub frontend_preferences: HashMap<String, serde_json::Value>,
    /// Null disables a command; absent ids use their default shortcut.
    #[serde(default)]
    pub shortcut_overrides: HashMap<String, Option<ShortcutBinding>>,
    /// EasyScholar key for journal ranking and impact-factor lookups.
    #[serde(default)]
    pub easy_scholar_key: String,
    /// 知乎开放平台 Access Secret（内置「知乎」Agent 的直答 / 检索凭证）。
    #[serde(default)]
    pub zhihu_access_secret: String,
    #[serde(default)]
    pub network_proxy_enabled: bool,
    #[serde(default = "default_network_proxy_url")]
    pub network_proxy_url: String,
    /// EZProxy/WebVPN prefix for paywalled PDF fallback, e.g.
    /// `https://webvpn.example.edu/login?url=`. Empty disables the layer.
    #[serde(default)]
    pub institution_proxy_prefix: String,
    /// Session cookie sent along with institution proxy requests (pasted from
    /// the browser). Empty = no cookie.
    #[serde(default)]
    pub institution_proxy_cookie: String,
    /// Gateway flavour: `auto` (fingerprint the prefix), `ezproxy` (query
    /// passthrough), `wengine` (path-rewriting WebVPN, e.g. ZJU) or
    /// `subdomain` (Astraeus-style host mapping). Empty/unknown means `auto`.
    #[serde(default)]
    pub institution_proxy_type: String,
    #[serde(default = "default_paper_tree_label_mode")]
    pub paper_tree_label_mode: String,
    #[serde(default = "default_paper_tree_sort_mode")]
    pub paper_tree_sort_mode: String,
    /// NOTES.md shell generated on paper import:
    /// `standard` | `title-only` | `blank` | `custom` (vault template).
    #[serde(default = "default_paper_note_mode")]
    pub paper_note_mode: String,
    /// Open a paper's NOTES.md in the reading split when a paper is opened.
    /// Default on; off opens only the PDF/HTML body.
    #[serde(default = "default_true")]
    pub auto_open_paper_notes: bool,
    /// Auto-ingest: adopt bare folders created under `papers/` that hold at
    /// least one settled PDF into the library in place (catalog row + NOTES
    /// shell + background metadata recognition). Default on.
    #[serde(default = "default_true")]
    pub auto_ingest: bool,
    /// When opening a new paper, close the active tab instead of adding a new one.
    /// Default off; useful for users who prefer a single-paper-at-a-time workflow.
    #[serde(default)]
    pub replace_current_tab_on_open_paper: bool,
    #[serde(default = "default_auto_update_internal_links")]
    pub auto_update_internal_links: String,
    /// Preferred terminal for "Open in Terminal". Empty = system default.
    /// Unknown or uninstalled ids fall back at launch time.
    #[serde(default)]
    pub terminal_app: String,
    /// PDF navigation: `vertical` | `horizontal` | `paginated`.
    #[serde(default = "default_pdf_scroll_strategy")]
    pub pdf_scroll_strategy: String,
    /// Enable single-key Vim navigation on PDF reading surfaces.
    #[serde(default = "default_true")]
    pub pdf_vim_keys: bool,
    /// Master switch for Vim-style single-key navigation (PDF + list surfaces).
    /// Off keeps plain typing and arrow keys only.
    #[serde(default = "default_true")]
    pub vim_mode: bool,
    /// Enable the ephemeral F-key global element hint overlay.
    #[serde(default = "default_true")]
    pub hint_mode_enabled: bool,
    /// Subtle in-app sound cues (agent attention / turn done / failure).
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    /// PDF page layout: `none` (single page) | `odd` | `even` (two-page spread).
    #[serde(default = "default_pdf_spread_mode")]
    pub pdf_spread_mode: String,
    /// In-page translate tab: `hover` (default) | `always` | `hidden`.
    #[serde(default = "default_pdf_translate_tab")]
    pub pdf_translate_tab: String,
    #[serde(default = "default_library_columns")]
    pub library_columns: Vec<LibraryColumnPref>,
    #[serde(default)]
    pub connector_enabled: bool,
    #[serde(default = "default_connector_port")]
    pub connector_port: u16,
    /// Loopback Streamable HTTP MCP server. Default off.
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
    /// Opt-in: expose paper full text (`paper_text_get`) through the MCP
    /// server. Default off — external clients only see metadata and NOTES.
    #[serde(default)]
    pub mcp_expose_paper_text: bool,
    /// OpenAI Secure MCP Tunnel id (`tunnel_` + 32 hex) for the built-in
    /// `tunnel-client` supervisor. Empty = never configured.
    #[serde(default)]
    pub mcp_tunnel_id: String,
    /// Runtime (Restricted) control-plane API key. Masked on read like the
    /// other commercial keys; the WebView never sees the plaintext.
    #[serde(default)]
    pub mcp_tunnel_api_key: String,
    #[serde(default = "default_batch_import_concurrency")]
    pub batch_import_concurrency: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_ui_theme")]
    pub ui_theme: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: u32,
    /// UI chrome font. Empty = app default. Built-ins: system/serif/mono, else family name.
    #[serde(default)]
    pub interface_font_family: String,
    /// Markdown body font. Empty = inherit interface/default. Same vocabulary as interface.
    #[serde(default)]
    pub text_font_family: String,
    /// Monospace font for code / font-mono. Empty = app default mono stack.
    #[serde(default)]
    pub mono_font_family: String,
    /// Deprecated single editor font preset; migrated into `text_font_family` once.
    #[serde(default, skip_serializing)]
    pub editor_font_family: String,
    /// Markdown editor body line-height (unitless), typical range 1.4–2.0.
    #[serde(default = "default_editor_line_height")]
    pub editor_line_height: f64,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f64,
    #[serde(default = "default_true")]
    pub show_editor_toolbar: bool,
    /// Body size of wikilink embeds, px before UI scale. Range 12–20, default 13.
    #[serde(default = "default_wiki_embed_font_size")]
    pub wiki_embed_font_size: u32,
    /// Markdown column max width in CSS px. 0 = full pane. Presets only.
    #[serde(default = "default_markdown_max_width")]
    pub markdown_max_width: u32,
    #[serde(default = "default_permission_mode")]
    pub agent_permission_mode: String,
    #[serde(default)]
    pub auto_paper_reader: bool,
    #[serde(default = "default_ai_response_language")]
    pub ai_response_language: String,
    #[serde(default)]
    pub agent_personal_prompt: String,
    #[serde(default)]
    pub pdf_ask: PdfAskSettings,
    /// Background paper-reader (精读) Agent seat. Empty = follow the default
    /// agent and its last picked model.
    #[serde(default)]
    pub paper_reader: PaperReaderSettings,
    /// Decision-layer provider (System One: jEV / Cloudflare Clef / compatible).
    /// The legacy `jev` key is still read for backward compatibility.
    #[serde(default, alias = "jev")]
    pub decision: DecisionSettings,
    #[serde(default)]
    pub embedding: EmbeddingSettings,
    #[serde(default)]
    pub translate: TranslateSettings,
    #[serde(default)]
    pub layout: LayoutSettings,
    /// PostHog product analytics opt-out (applies from the next launch).
    #[serde(default = "default_true")]
    pub telemetry_enabled: bool,
    /// Plaza discovery sources in the sidebar. Default on.
    #[serde(default = "default_true")]
    pub plaza_enabled: bool,
    /// Plaza source ids hidden from the sidebar / Plaza home (right-click toggle).
    #[serde(default)]
    pub plaza_hidden_sources: Vec<String>,
    /// First-run setup wizard completed. Default false → auto-show on fresh installs.
    #[serde(default)]
    pub onboarding_done: bool,
    /// Post-vault feature tour completed or skipped. Default false → auto-start once.
    #[serde(default)]
    pub feature_tour_done: bool,
    /// Config reminders the user dismissed with "don't remind me again"
    /// (`layout-local-model` | `network-proxy`). Unknown ids are dropped on save.
    #[serde(default)]
    pub dismissed_reminders: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PdfAskSettings {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub model_id: String,
}

/// Agent seat for the background paper-reader workflow.
///
/// Empty fields follow the default agent and the model last picked for it;
/// pinning them keeps the workflow off the agent CLI's own default model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PaperReaderSettings {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub model_id: String,
}

/// Decision-layer provider settings.
///
/// Callers speak the System One contract (`state + questions -> answers`),
/// implemented by TypeSafe jEV, Cloudflare Clef, and compatible endpoints.
/// The OpenAI Decisions preset (`provider = "openai"`) uses the same settings
/// fields; the transport adapts request/response shapes to `/v1/decisions`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DecisionSettings {
    /// `jev` | `clef` | `openai` | `custom` (empty = `jev`).
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    /// Model id injected into every request (e.g. `jev-latest`, `clef`).
    #[serde(default)]
    pub model: String,
    /// Whether the experimental smart-highlight toolbar action is enabled.
    #[serde(default)]
    pub smart_highlight: bool,
    /// How many smart-highlight batches may be in flight. One of 1, 2, 4, 16.
    #[serde(default = "default_smart_highlight_concurrency")]
    pub smart_highlight_concurrency: u32,
    /// Seconds for one smart-highlight batch, starting when its request is sent.
    #[serde(default = "default_smart_highlight_timeout_secs")]
    pub smart_highlight_timeout_secs: u32,
}

impl Default for DecisionSettings {
    fn default() -> Self {
        Self {
            provider: default_decision_provider(),
            api_key: String::new(),
            base_url: default_decision_base_url(),
            model: default_decision_model(),
            smart_highlight: false,
            smart_highlight_concurrency: default_smart_highlight_concurrency(),
            smart_highlight_timeout_secs: default_smart_highlight_timeout_secs(),
        }
    }
}

#[doc(hidden)]
pub const SMART_HIGHLIGHT_CONCURRENCY_CHOICES: &[u32] = &[1, 2, 4, 16];
#[doc(hidden)]
pub const SMART_HIGHLIGHT_TIMEOUT_MIN_SECS: u32 = 30;
#[doc(hidden)]
pub const SMART_HIGHLIGHT_TIMEOUT_MAX_SECS: u32 = 600;

#[doc(hidden)]
pub fn default_smart_highlight_concurrency() -> u32 {
    16
}

#[doc(hidden)]
pub fn default_smart_highlight_timeout_secs() -> u32 {
    120
}

/// Embedding endpoint: the built-in provider, or a custom OpenAI-compatible
/// one (BYOK). Custom with all-empty fields = feature disabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSettings {
    /// `builtin` | `custom`; empty = unset and inferred from the fields below.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
}

/// One column in the papers Library table: array order = display order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryColumnPref {
    pub key: String,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_rem: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranslateSettings {
    #[serde(default)]
    pub dictionary: DictionarySettings,
    #[serde(default = "default_translate_provider")]
    pub provider: String,
    #[serde(default = "default_translate_target")]
    pub target_lang: String,
    #[serde(default = "default_translate_source")]
    pub source_lang: String,
    #[serde(default)]
    pub provider_configs: HashMap<String, TranslateProviderConfig>,
    #[serde(default)]
    pub auto_translate_selection: bool,
    /// Deprecated: kept for migration. Use `display_mode` + `dual_pane_source`.
    #[serde(default)]
    pub dual_pane_translate: bool,
    #[serde(default = "default_translate_display_mode")]
    pub display_mode: String,
    #[serde(default = "default_translate_dual_pane_source")]
    pub dual_pane_source: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub model_id: String,
    /// Legacy single template. Empty when `custom_system` is set. A short line
    /// is an extra preference; a longer value replaces the academic style
    /// block only. Engine rules always stay. Max 8000 chars.
    #[serde(default)]
    pub custom_prompt: String,
    /// Custom system style / preference. Wins over `custom_prompt`.
    /// Placeholders: `{{targetLang}}`, `{{sourceLang}}`, `{{text}}`.
    #[serde(default)]
    pub custom_system: String,
    /// Custom user template. Applied only when it contains `{{text}}`.
    #[serde(default)]
    pub custom_user: String,
    /// Persist extracted reflow character IR pages in the app cache (default on).
    #[serde(default = "default_true")]
    pub cache_reflow_ir: bool,
    /// Persist the finished translated PDF in the app cache (default off;
    /// each final is roughly 25 MB).
    #[serde(default)]
    pub cache_reflow_final: bool,
    /// Full-document translation runs in the background after a paper is
    /// imported and its layout lands (default off: it spends provider quota).
    #[serde(default)]
    pub auto_translate_import: bool,
    /// How many papers the background import translation runs at once.
    /// Seeded into the JobCenter `LayoutTranslate` cap (1–5, default 1).
    #[serde(default = "default_auto_translate_import_concurrency")]
    pub auto_translate_import_concurrency: u32,
    /// Close the selection translation result on a click elsewhere on the page
    /// (default off, matching the long-standing "card stays open" behavior).
    /// Scrolling, page turns and viewer toolbars never close it.
    #[serde(default)]
    pub dismiss_selection_on_outside_click: bool,
}

impl Default for TranslateSettings {
    fn default() -> Self {
        Self {
            dictionary: DictionarySettings::default(),
            provider: default_translate_provider(),
            target_lang: default_translate_target(),
            source_lang: default_translate_source(),
            provider_configs: HashMap::new(),
            auto_translate_selection: false,
            dual_pane_translate: false,
            display_mode: default_translate_display_mode(),
            dual_pane_source: default_translate_dual_pane_source(),
            agent_id: String::new(),
            model_id: String::new(),
            custom_prompt: String::new(),
            custom_system: String::new(),
            custom_user: String::new(),
            cache_reflow_ir: true,
            cache_reflow_final: false,
            auto_translate_import: false,
            auto_translate_import_concurrency: default_auto_translate_import_concurrency(),
            dismiss_selection_on_outside_click: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct DictionarySettings {
    #[serde(default = "default_dictionary_mode")]
    pub mode: String,
}

#[doc(hidden)]
pub fn default_dictionary_mode() -> String {
    "auto".into()
}

impl Default for DictionarySettings {
    fn default() -> Self {
        Self {
            mode: default_dictionary_mode(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranslateProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub extra_request_body: String,
    /// Full-document translation batch concurrency for the OpenAI-compatible
    /// path (1–8, default 2). Selection translation and the Agent path stay
    /// serial; other providers ignore it.
    #[serde(default = "default_translate_concurrency")]
    pub concurrency: u32,
}

impl Default for TranslateProviderConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: String::new(),
            region: String::new(),
            model: String::new(),
            extra_request_body: String::new(),
            concurrency: default_translate_concurrency(),
        }
    }
}

/// PDF layout-analysis backend selection.
/// - `local`: on-device PP-DocLayoutV3 (ONNX in the renderer).
/// - `paddle`: remote PP-StructureV3 async job API (`POST {base}/api/v2/ocr/jobs`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSettings {
    #[serde(default = "default_layout_backend")]
    pub backend: String,
    /// PAPER.md body-parse engine: `local` | `paddle` | `mineru` |
    /// `openaiCompatible` | `agentero` (built-in).
    #[serde(default = "default_parser_backend")]
    pub parser_backend: String,
    #[serde(default)]
    pub provider_configs: HashMap<String, LayoutProviderConfig>,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            backend: default_layout_backend(),
            parser_backend: default_parser_backend(),
            provider_configs: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// OCR prompt override; empty → the engine derives one from the model id.
    #[serde(default)]
    pub prompt: String,
    /// MinerU document language (OCR language pack); normalize() enforces the
    /// MinerU vocabulary and falls back to `ch`.
    #[serde(default)]
    pub language: String,
    /// MinerU force-OCR: run OCR on every page regardless of the PDF text layer.
    #[serde(default)]
    pub is_ocr: bool,
    /// Treat `base_url` as a self-hosted MinerU server (version-detected)
    /// instead of the official cloud API (which also covers relays).
    #[serde(default)]
    pub self_hosted: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            frontend_preferences: HashMap::new(),
            shortcut_overrides: HashMap::new(),
            easy_scholar_key: String::new(),
            zhihu_access_secret: String::new(),
            network_proxy_enabled: false,
            network_proxy_url: default_network_proxy_url(),
            institution_proxy_prefix: String::new(),
            institution_proxy_cookie: String::new(),
            institution_proxy_type: "auto".to_string(),
            paper_tree_label_mode: default_paper_tree_label_mode(),
            paper_tree_sort_mode: default_paper_tree_sort_mode(),
            paper_note_mode: default_paper_note_mode(),
            auto_open_paper_notes: default_true(),
            auto_ingest: default_true(),
            replace_current_tab_on_open_paper: false,
            auto_update_internal_links: default_auto_update_internal_links(),
            terminal_app: String::new(),
            pdf_scroll_strategy: default_pdf_scroll_strategy(),
            pdf_vim_keys: true,
            vim_mode: true,
            hint_mode_enabled: true,
            sound_enabled: true,
            pdf_spread_mode: default_pdf_spread_mode(),
            pdf_translate_tab: default_pdf_translate_tab(),
            library_columns: default_library_columns(),
            connector_enabled: false,
            connector_port: default_connector_port(),
            mcp_enabled: false,
            mcp_expose_paper_text: false,
            mcp_port: default_mcp_port(),
            mcp_tunnel_id: String::new(),
            mcp_tunnel_api_key: String::new(),
            batch_import_concurrency: default_batch_import_concurrency(),
            theme: default_theme(),
            ui_theme: default_ui_theme(),
            locale: default_locale(),
            editor_font_size: default_editor_font_size(),
            interface_font_family: String::new(),
            text_font_family: String::new(),
            mono_font_family: String::new(),
            editor_font_family: String::new(),
            editor_line_height: default_editor_line_height(),
            ui_scale: default_ui_scale(),
            show_editor_toolbar: true,
            wiki_embed_font_size: default_wiki_embed_font_size(),
            markdown_max_width: default_markdown_max_width(),
            agent_permission_mode: default_permission_mode(),
            auto_paper_reader: false,
            ai_response_language: default_ai_response_language(),
            agent_personal_prompt: String::new(),
            pdf_ask: PdfAskSettings::default(),
            paper_reader: PaperReaderSettings::default(),
            decision: DecisionSettings::default(),
            embedding: EmbeddingSettings::default(),
            translate: TranslateSettings::default(),
            layout: LayoutSettings::default(),
            telemetry_enabled: default_true(),
            plaza_enabled: default_true(),
            plaza_hidden_sources: Vec::new(),
            onboarding_done: false,
            feature_tour_done: false,
            dismissed_reminders: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}
#[doc(hidden)]
pub fn default_network_proxy_url() -> String {
    DEFAULT_NETWORK_PROXY_URL.to_string()
}
#[doc(hidden)]
pub fn default_decision_provider() -> String {
    "jev".to_string()
}
#[doc(hidden)]
pub fn default_decision_base_url() -> String {
    DEFAULT_DECISION_BASE_URL.to_string()
}
#[doc(hidden)]
pub fn default_decision_model() -> String {
    DEFAULT_SYSTEM_ONE_MODEL.to_string()
}
#[doc(hidden)]
pub fn default_paper_tree_label_mode() -> String {
    "title-author".into()
}
#[doc(hidden)]
pub fn default_paper_tree_sort_mode() -> String {
    "folder".into()
}
#[doc(hidden)]
pub fn default_paper_note_mode() -> String {
    "standard".into()
}
#[doc(hidden)]
pub fn default_auto_update_internal_links() -> String {
    "ask".into()
}
#[doc(hidden)]
pub fn default_pdf_scroll_strategy() -> String {
    "vertical".into()
}
#[doc(hidden)]
pub fn default_pdf_spread_mode() -> String {
    "none".into()
}
#[doc(hidden)]
pub fn default_pdf_translate_tab() -> String {
    "hover".into()
}
/// Canonical papers-Library column keys, in default order.
#[doc(hidden)]
pub const LIBRARY_COLUMN_KEYS: &[&str] = &[
    "title",
    "authors",
    "date",
    "publication",
    "tags",
    "id",
    "citations",
    "addedAt",
];
#[doc(hidden)]
pub fn default_library_columns() -> Vec<LibraryColumnPref> {
    LIBRARY_COLUMN_KEYS
        .iter()
        .map(|&key| LibraryColumnPref {
            key: key.to_string(),
            visible: key != "addedAt",
            width_rem: None,
        })
        .collect()
}
#[doc(hidden)]
pub fn default_theme() -> String {
    "system".into()
}
/// tweakcn preset name; the theme list lives in the frontend bundle, so the
/// Host only guarantees a non-empty value.
#[doc(hidden)]
pub fn default_ui_theme() -> String {
    "default".into()
}
#[doc(hidden)]
pub fn default_locale() -> String {
    "system".into()
}
#[doc(hidden)]
pub fn default_editor_font_size() -> u32 {
    14
}
#[doc(hidden)]
pub fn default_editor_line_height() -> f64 {
    1.6
}
#[doc(hidden)]
pub fn default_wiki_embed_font_size() -> u32 {
    13
}
fn default_markdown_max_width() -> u32 {
    0
}
#[doc(hidden)]
pub fn normalize_markdown_max_width(value: u32) -> u32 {
    const ALLOWED: &[u32] = &[0, 640, 720, 800, 960, 1120];
    if ALLOWED.contains(&value) {
        value
    } else {
        default_markdown_max_width()
    }
}

#[doc(hidden)]
pub fn normalize_font_family_value(raw: &str) -> String {
    let v = raw.trim();
    if v.is_empty() || v == "default" {
        return String::new();
    }
    v.chars().take(120).collect()
}
#[doc(hidden)]
pub fn default_ui_scale() -> f64 {
    1.0
}
#[doc(hidden)]
pub fn default_permission_mode() -> String {
    "restricted".into()
}
#[doc(hidden)]
pub fn default_ai_response_language() -> String {
    "auto".into()
}
fn default_translate_provider() -> String {
    "tencenttransmart".into()
}
#[doc(hidden)]
pub fn default_connector_port() -> u16 {
    DEFAULT_CONNECTOR_PORT
}
#[doc(hidden)]
pub fn default_mcp_port() -> u16 {
    DEFAULT_MCP_PORT
}
#[doc(hidden)]
pub fn default_batch_import_concurrency() -> u32 {
    5
}
#[doc(hidden)]
pub fn default_auto_translate_import_concurrency() -> u32 {
    1
}
#[doc(hidden)]
pub fn default_translate_concurrency() -> u32 {
    2
}
#[doc(hidden)]
pub fn default_translate_target() -> String {
    "ui".into()
}
#[doc(hidden)]
pub fn default_translate_source() -> String {
    "auto".into()
}
fn default_translate_display_mode() -> String {
    "overlay".into()
}
fn default_translate_dual_pane_source() -> String {
    "pdf".into()
}
/// Layout analysis stays on the bundled offline PP-DocLayoutV3 model: it is
/// free, local, and a cloud backend would bill every PDF for no gain. The
/// built-in provider is a *parser* (body-text) backend only.
#[doc(hidden)]
pub fn default_layout_backend() -> String {
    "local".into()
}
#[doc(hidden)]
pub fn default_parser_backend() -> String {
    "local".into()
}
