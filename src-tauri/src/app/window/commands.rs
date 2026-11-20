//! Multi-window helpers.

use sha2::{Digest, Sha256};
use tauri::{
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl,
    WebviewWindowBuilder,
};

use crate::core::log_util::OpTimer;

// Settings store is only used for macOS traffic-light y scaling.
#[cfg(target_os = "macos")]
use crate::features::system::settings::AppSettingsStore;

/// Default traffic-light y position at 100% UI scale. Matches tauri.conf.json.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_Y_DEFAULT: f64 = 18.0;
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_X: f64 = 14.0;

/// Top-left y for the macOS traffic-light buttons in the 32px Settings header.
/// 9 + 14 (button height) = 23, leaving 9px above and 9px below for vertical centering.
#[cfg(target_os = "macos")]
const SETTINGS_TRAFFIC_LIGHT_Y: f64 = 16.0;

pub const SETTINGS_WINDOW_LABEL: &str = "settings";
pub const CHATGPT_SIDEBAR_WEBVIEW_LABEL: &str = "chatgpt-sidebar";
pub const SETTINGS_SECTION_REQUESTED_EVENT: &str = "settings:section-requested";
pub const SETTINGS_VAULT_REQUESTED_EVENT: &str = "settings:vault-requested";

const CHATGPT_URL: &str = "https://chatgpt.com/";
const CHATGPT_QUERY_MAX_BYTES: usize = 32 * 1024;
static CHATGPT_WEBVIEW_OPEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Copy, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptSidebarBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Broadcast when a singleton child window closes (or fails to build).
/// Wire naming follows docs/architecture.md (`domain:event`);
/// this is a window signal, not a lifecycle fact event.
pub const WINDOW_CLOSED_EVENT: &str = "window:closed";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowClosedPayload<'a> {
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    view: Option<&'a str>,
}

/// Emit `window:closed` with `kind: "settings" | "feature"` (+ `view` for feature).
pub fn emit_window_closed(app: &AppHandle, kind: &str, view: Option<&str>) {
    let _ = app.emit(WINDOW_CLOSED_EVENT, WindowClosedPayload { kind, view });
}

/// Bring an existing singleton window back to the front, restoring it if the
/// user minimized it.
///
/// `set_focus` alone is not enough on Windows: tao only calls
/// `SetForegroundWindow` for a window that is visible and not minimized, so a
/// minimized Settings / feature / document window silently ignores the click.
/// `unminimize` and `show` are queued onto the event loop when called from an
/// async command, so a `set_focus` right after them would still read the stale
/// minimized state. Run the sequence on the main thread, where each call is
/// applied to the window state before the next one reads it. macOS and Linux
/// keep plain `set_focus` — their implementations already deminiaturize and
/// raise the window.
fn raise_existing_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        let win = window.clone();
        let _ = window.run_on_main_thread(move || {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        });
    }
    #[cfg(not(windows))]
    let _ = window.set_focus();
}

fn chatgpt_url(query: Option<&str>) -> Result<Url, String> {
    let mut url = Url::parse(CHATGPT_URL).map_err(|_| "chatgpt_url_invalid".to_string())?;
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return Ok(url);
    };
    if query.len() > CHATGPT_QUERY_MAX_BYTES {
        return Err("chatgpt_query_too_large".into());
    }
    url.query_pairs_mut().append_pair("q", query);
    Ok(url)
}

fn is_safe_chatgpt_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "about")
}

fn chatgpt_sidebar_geometry(
    bounds: ChatGptSidebarBounds,
) -> Result<(LogicalPosition<f64>, LogicalSize<f64>), String> {
    let values = [bounds.x, bounds.y, bounds.width, bounds.height];
    if values.iter().any(|value| !value.is_finite())
        || bounds.x < 0.0
        || bounds.y < 0.0
        || bounds.width < 1.0
        || bounds.height < 1.0
    {
        return Err("invalid_chatgpt_sidebar_bounds".into());
    }
    Ok((
        LogicalPosition::new(bounds.x, bounds.y),
        LogicalSize::new(bounds.width, bounds.height),
    ))
}

/// Enables macOS native fullscreen space and window tiling (Move & Resize).
///
/// `setCollectionBehavior:` must run on the main thread: AppKit routes it
/// through its window-management coordinator, which traps (`EXC_BREAKPOINT`)
/// when called from an async command thread and takes the whole app down
/// before the new window ever shows. `window_new` / `doc_window_open` are
/// `async` (see [`window_new`]), so hop back to the main thread with
/// `run_on_main_thread`.
#[cfg(target_os = "macos")]
pub fn enable_window_tiling(window: &tauri::WebviewWindow) {
    let window = window.clone();
    let scheduler = window.clone();
    let _ = scheduler.run_on_main_thread(move || {
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        let Ok(ptr) = window.ns_window() else {
            return;
        };
        let ns_win = unsafe { (ptr as *mut NSWindow).as_ref() };
        if let Some(ns_win) = ns_win {
            let mut behavior = ns_win.collectionBehavior();
            behavior |= NSWindowCollectionBehavior::FullScreenPrimary
                | NSWindowCollectionBehavior::FullScreenAllowsTiling;
            ns_win.setCollectionBehavior(behavior);
        }
    });
}

/// Open a fresh Agentero window without restoring the last vault (`?fresh=1`).
///
/// `async` is load-bearing: sync command handlers run on the main thread inside
/// the calling webview's IPC callback, and building a webview from there hangs
/// on Windows — wry waits in a nested message loop for the WebView2 controller
/// callback, which WebView2 only runs once the current handler returns, so
/// `build()` never comes back and the new window stays blank. Async handlers run
/// off the main thread, so window creation is queued onto the event loop
/// instead of nested inside another handler.
#[tauri::command]
#[specta::specta]
pub async fn window_new(app: AppHandle) -> Result<(), String> {
    let op = OpTimer::start("window_new");
    let label = format!("agentero-{}", uuid::Uuid::new_v4().simple());

    // Windows/macOS main windows use `dragDropEnabled: false` for HTML5
    // DnD (vault moves, tab reorder, Library/composer file drops). Linux
    // enables native GTK file paths as a fallback for empty WebKit FileLists.
    // On Windows, Tauri native drag-drop swallows HTML5 dragover/drop.
    // Frontend preventDefaults leftover file drops so the webview never
    // navigates to a dropped PDF.
    // Secondary windows inherit platform defaults — frontend still preventDefaults.
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut builder = app
        .state::<crate::core::webview::WebviewProxyConfig>()
        .apply(WebviewWindowBuilder::new(
            &app,
            &label,
            WebviewUrl::App("index.html?fresh=1".into()),
        ))
        .title("Agentero")
        .inner_size(1280.0, 800.0)
        .min_inner_size(700.0, 520.0)
        .resizable(true);

    #[cfg(target_os = "macos")]
    {
        let scale = app
            .state::<AppSettingsStore>()
            .get()
            .map(|r| r.settings.ui_scale)
            .unwrap_or(1.0);
        let y = if scale.is_finite() && (0.8..=1.5).contains(&scale) {
            TRAFFIC_LIGHT_Y_DEFAULT * scale
        } else {
            TRAFFIC_LIGHT_Y_DEFAULT
        };
        builder = builder
            .visible(false)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .traffic_light_position(tauri::LogicalPosition::new(TRAFFIC_LIGHT_X, y));
    }

    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            op.finish_err_msg("window", &e);
            return Err(e.to_string());
        }
    };
    #[cfg(target_os = "macos")]
    enable_window_tiling(&window);
    let _ = window.set_focus();

    // Native menu is macOS-only; other platforms drive actions from the React
    // title bar + keyboard shortcuts, so no window menu is attached.
    #[cfg(target_os = "macos")]
    if let Some(menu) = app.menu() {
        let _ = window.set_menu(menu);
    }

    op.finish_ok_extra(format!("label={label}"));
    Ok(())
}

/// Open, position, and focus the ChatGPT child WebView embedded in Agentero's
/// right rail. The child reuses the main window's persistent WebView2
/// environment and proxy snapshot. Its webview label is absent from Tauri
/// capabilities, so the remote origin has no Agentero IPC, Vault, or document
/// access. A query navigates the child through ChatGPT Web's `?q=` entry point.
/// Keep this async: creating a child WebView inside a synchronous command can
/// deadlock Windows WebView2 when called from another WebView's IPC callback.
#[tauri::command]
#[specta::specta]
pub async fn chatgpt_sidebar_open(
    app: AppHandle,
    query: Option<String>,
    bounds: ChatGptSidebarBounds,
) -> Result<(), String> {
    let op = OpTimer::start("chatgpt_sidebar_open");
    let url = match chatgpt_url(query.as_deref()) {
        Ok(url) => url,
        Err(error) => {
            op.finish_err_msg("url", &error);
            return Err(error);
        }
    };
    let has_query = url.query().is_some();
    let (position, size) = match chatgpt_sidebar_geometry(bounds) {
        Ok(geometry) => geometry,
        Err(error) => {
            op.finish_err_msg("bounds", &error);
            return Err(error);
        }
    };
    let _guard = CHATGPT_WEBVIEW_OPEN_LOCK.lock().await;

    if let Some(webview) = app.get_webview(CHATGPT_SIDEBAR_WEBVIEW_LABEL) {
        webview
            .set_position(position)
            .map_err(|error| error.to_string())?;
        webview.set_size(size).map_err(|error| error.to_string())?;
        if has_query {
            webview.navigate(url).map_err(|error| error.to_string())?;
        }
        webview.show().map_err(|error| error.to_string())?;
        if has_query {
            webview.set_focus().map_err(|error| error.to_string())?;
        }
        op.finish_ok_extra("existing=true");
        return Ok(());
    }

    let window = app
        .get_window("main")
        .ok_or_else(|| "main_window_missing".to_string())?;
    let builder = app
        .state::<crate::core::webview::WebviewProxyConfig>()
        .apply_webview(WebviewBuilder::new(
            CHATGPT_SIDEBAR_WEBVIEW_LABEL,
            WebviewUrl::External(url),
        ))
        // ChatGPT sign-in providers use window.open for OAuth. The default
        // popup path uses the same WebView environment and has no Agentero
        // capability. Restrict both paths to secure navigation.
        .on_navigation(is_safe_chatgpt_navigation)
        .on_new_window(|url, _features| {
            if is_safe_chatgpt_navigation(&url) {
                NewWindowResponse::Allow
            } else {
                NewWindowResponse::Deny
            }
        });
    let webview = match window.add_child(builder, position, size) {
        Ok(webview) => webview,
        Err(error) => {
            op.finish_err_msg("webview", &error);
            return Err(error.to_string());
        }
    };
    webview.show().map_err(|error| error.to_string())?;
    if has_query {
        webview.set_focus().map_err(|error| error.to_string())?;
    }
    op.finish_ok_extra("existing=false");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn chatgpt_sidebar_attach_quote(
    app: AppHandle,
    caller: tauri::Webview,
    preview: String,
    quote: String,
) -> Result<(), String> {
    if caller.label() != "main" {
        return Err("chatgpt_sidebar_quote_requires_main_window".into());
    }
    if preview.len() > 1024 || quote.trim().is_empty() || quote.len() > 128 * 1024 {
        return Err("invalid_chatgpt_quote".into());
    }
    let webview = app
        .get_webview(CHATGPT_SIDEBAR_WEBVIEW_LABEL)
        .ok_or_else(|| "chatgpt_sidebar_unavailable".to_string())?;
    let url = webview.url().map_err(|error| error.to_string())?;
    let filename = chatgpt_quote_filename(&preview, &quote);
    let op = OpTimer::start("chatgpt_sidebar_attach_quote");
    let result = attach_chatgpt_quote(&webview, url.as_str(), &filename, &preview, &quote).await;
    match result {
        Ok(()) => {
            op.finish_ok();
            let _ = webview.set_focus();
            Ok(())
        }
        Err((code, status)) => {
            op.finish_err_msg(code, status);
            Err(code.into())
        }
    }
}

fn chatgpt_quote_filename(preview: &str, quote: &str) -> String {
    let title: String = preview
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect();
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.trim_matches(['.', ' ']);
    let mut prefix: String = title.chars().take(36).collect();
    if prefix.len() < title.len() {
        prefix.truncate(prefix.trim_end().len());
        prefix.push('…');
    }
    let title = if prefix.is_empty() {
        "Excerpt"
    } else {
        &prefix
    };
    let hash = hex::encode(Sha256::digest(quote.as_bytes()));
    format!("{title} · {}.md", &hash[..8])
}

async fn attach_chatgpt_quote(
    webview: &tauri::Webview,
    url: &str,
    filename: &str,
    preview: &str,
    quote: &str,
) -> Result<(), (&'static str, &'static str)> {
    let request = serde_json::json!({
        "url": url, "filename": filename, "preview": preview, "text": quote
    });
    match eval_chatgpt_quote(webview, &request).await.as_deref() {
        Some("attached") => return Ok(()),
        Some("queued" | "processing") => (),
        Some(
            "wrong_origin"
            | "conversation_changed"
            | "composer_missing"
            | "attachment_unavailable"
            | "attachment_failed",
        ) => {
            return Err(("attachment_unavailable", "composer_rejected_attachment"));
        }
        _ => return Err(("attachment_unconfirmed", "upload_dispatch_unconfirmed")),
    }

    let check = serde_json::json!({ "url": url, "filename": filename, "preview": preview });
    let mut status = "card_not_visible";
    for _ in 0..48 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        match eval_chatgpt_quote(webview, &check).await.as_deref() {
            Some("attached") => return Ok(()),
            Some("waiting") => status = "card_not_visible",
            Some("processing") => status = "card_processing",
            Some("attachment_failed") => {
                return Err(("attachment_unavailable", "upload_failed"));
            }
            Some("conversation_changed") => {
                return Err(("attachment_unconfirmed", "conversation_changed"));
            }
            _ => break,
        }
    }
    Err(("attachment_unconfirmed", status))
}

async fn eval_chatgpt_quote(
    webview: &tauri::Webview,
    request: &serde_json::Value,
) -> Option<String> {
    let script = include_str!("chatgpt-quote-attachment.js");
    eval_chatgpt_script(webview, format!("{script}({request});")).await
}

pub(super) async fn eval_chatgpt_script<R: tauri::Runtime, T: serde::de::DeserializeOwned>(
    webview: &tauri::Webview<R>,
    script: String,
) -> Option<T> {
    let (send, mut receive) = tokio::sync::mpsc::channel(1);
    webview
        .eval_with_callback(script, move |result| {
            let _ = send.try_send(result);
        })
        .ok()?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), receive.recv())
        .await
        .ok()??;
    serde_json::from_str(&result).ok()
}

/// Hide the embedded ChatGPT WebView when the right rail is closed or another
/// rail surface is active. The WebView and its page state remain mounted.
#[tauri::command]
#[specta::specta]
pub fn chatgpt_sidebar_hide(app: AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview(CHATGPT_SIDEBAR_WEBVIEW_LABEL) {
        webview.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Destroy the embedded ChatGPT WebView. Used only when the main page unloads.
#[tauri::command]
#[specta::specta]
pub fn chatgpt_sidebar_close(app: AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview(CHATGPT_SIDEBAR_WEBVIEW_LABEL) {
        webview.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Open a singleton native Settings window, or focus it if already open.
///
/// See [`window_new`] for why this must stay `async`.
#[tauri::command]
#[specta::specta]
pub async fn settings_window_open(
    app: AppHandle,
    section: Option<String>,
    vault_path: Option<String>,
) -> Result<(), String> {
    let op = OpTimer::start("settings_window_open");

    if let Some(win) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        if let Some(section) = section {
            win.emit_to(
                EventTarget::webview_window(SETTINGS_WINDOW_LABEL),
                SETTINGS_SECTION_REQUESTED_EVENT,
                section,
            )
            .map_err(|error| error.to_string())?;
        }
        // The window is a singleton; reuse must retarget it at the Vault that
        // asked for it, otherwise the sync pane keeps editing the first vault.
        if let Some(path) = vault_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            win.emit_to(
                EventTarget::webview_window(SETTINGS_WINDOW_LABEL),
                SETTINGS_VAULT_REQUESTED_EVENT,
                path,
            )
            .map_err(|error| error.to_string())?;
        }
        raise_existing_window(&win);
        op.finish_ok_extra("existing");
        return Ok(());
    }

    let mut url = format!(
        "index.html?window=settings&section={}",
        urlencoding::encode(section.as_deref().unwrap_or("general"))
    );
    if let Some(path) = vault_path {
        url.push_str(&format!("&vault_path={}", urlencoding::encode(&path)));
    }

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut builder = app
        .state::<crate::core::webview::WebviewProxyConfig>()
        .apply(WebviewWindowBuilder::new(
            &app,
            SETTINGS_WINDOW_LABEL,
            WebviewUrl::App(url.into()),
        ))
        .title("Settings")
        .inner_size(880.0, 640.0)
        .min_inner_size(760.0, 520.0)
        .resizable(true);

    #[cfg(target_os = "macos")]
    {
        // The 32px header (Tailwind h-8) does not scale with `ui_scale`, so the
        // y position stays at the constant that vertically centers the buttons.
        builder = builder
            .visible(false)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .traffic_light_position(tauri::LogicalPosition::new(
                TRAFFIC_LIGHT_X,
                SETTINGS_TRAFFIC_LIGHT_Y,
            ));
    }

    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            op.finish_err_msg("settings", &e);
            // Keep the main window's `⌘,` toggle out of a stuck "open" state.
            emit_window_closed(&app, "settings", None);
            return Err(e.to_string());
        }
    };
    let _ = window.set_focus();

    op.finish_ok_extra("new");
    Ok(())
}

/// Valid right-rail feature views that may open as a singleton native window.
pub(crate) const FEATURE_VIEWS: &[&str] =
    &["agent", "backlinks", "annotations", "references", "figures"];

/// Feature views the desktop frontend can actually pop out today (see
/// `FeatureViewType` in `lib/shell/ui-store`).
pub(crate) const UI_OPENABLE_FEATURE_VIEWS: &[&str] = &["agent", "annotations"];

/// True for a window id the `ui:request` `open-window` action may target: the
/// singleton Settings window or an openable right-rail feature window.
pub(crate) fn is_known_window(view: &str) -> bool {
    view == "settings" || UI_OPENABLE_FEATURE_VIEWS.contains(&view)
}

pub fn feature_window_label(view: &str) -> String {
    format!("feature-{view}")
}

/// Parse `feature-{view}` labels; used when emitting close events.
pub fn feature_view_from_label(label: &str) -> Option<&str> {
    label
        .strip_prefix("feature-")
        .filter(|v| FEATURE_VIEWS.contains(v))
}

fn validate_feature_view(view: &str) -> Result<(), String> {
    if FEATURE_VIEWS.contains(&view) {
        Ok(())
    } else {
        Err(format!("unknown feature view: {view}"))
    }
}

/// Stable Webview label for a document path (`doc-` + short sha256 hex).
pub fn doc_window_label(path: &str) -> String {
    let hash = Sha256::digest(path.as_bytes());
    let hex = hex::encode(hash);
    // 16 hex chars is enough to avoid collisions for open doc windows.
    format!("doc-{}", &hex[..16])
}

/// Open (or focus) a singleton feature window for a right-rail view.
///
/// See [`window_new`] for why this must stay `async`.
#[tauri::command]
#[specta::specta]
pub async fn feature_window_open(
    app: AppHandle,
    view: String,
    vault_path: Option<String>,
    active_path: Option<String>,
    paper_title: Option<String>,
    // Localized caption from the frontend (`t()`); English fallbacks below.
    title: Option<String>,
) -> Result<(), String> {
    let op = OpTimer::start("feature_window_open");
    validate_feature_view(&view)?;
    let label = feature_window_label(&view);

    if let Some(win) = app.get_webview_window(&label) {
        raise_existing_window(&win);
        op.finish_ok_extra(format!("existing view={view}"));
        return Ok(());
    }

    let mut url = format!(
        "index.html?window=feature&view={}",
        urlencoding::encode(&view)
    );
    if let Some(path) = vault_path {
        url.push_str(&format!("&vault_path={}", urlencoding::encode(&path)));
    }
    if let Some(path) = active_path.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&active_path={}", urlencoding::encode(&path)));
    }
    if let Some(pt) = paper_title.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&paper_title={}", urlencoding::encode(&pt)));
    }

    let window_title =
        title
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| match view.as_str() {
                "agent" => "Agent".into(),
                "backlinks" => "Backlinks".into(),
                "annotations" => "Annotations".into(),
                "references" => "References".into(),
                "figures" => "Figures".into(),
                other => other.to_string(),
            });

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut builder = app
        .state::<crate::core::webview::WebviewProxyConfig>()
        .apply(WebviewWindowBuilder::new(
            &app,
            &label,
            WebviewUrl::App(url.into()),
        ))
        .title(window_title)
        .inner_size(420.0, 720.0)
        .min_inner_size(320.0, 400.0)
        .resizable(true);

    #[cfg(target_os = "macos")]
    {
        let scale = app
            .state::<AppSettingsStore>()
            .get()
            .map(|r| r.settings.ui_scale)
            .unwrap_or(1.0);
        let y = if scale.is_finite() && (0.8..=1.5).contains(&scale) {
            TRAFFIC_LIGHT_Y_DEFAULT * scale
        } else {
            TRAFFIC_LIGHT_Y_DEFAULT
        };
        builder = builder
            .visible(false)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .traffic_light_position(tauri::LogicalPosition::new(TRAFFIC_LIGHT_X, y));
    }

    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            op.finish_err_msg("feature", &e);
            emit_window_closed(&app, "feature", Some(&view));
            return Err(e.to_string());
        }
    };
    let _ = window.set_focus();

    op.finish_ok_extra(format!("new view={view}"));
    Ok(())
}

/// Open (or focus) a per-path document window.
///
/// See [`window_new`] for why this must stay `async`.
#[tauri::command]
#[specta::specta]
pub async fn doc_window_open(
    app: AppHandle,
    path: String,
    mode: Option<String>,
    vault_path: Option<String>,
    // Localized caption from the frontend when available.
    title: Option<String>,
) -> Result<(), String> {
    let op = OpTimer::start("doc_window_open");
    if path.trim().is_empty() {
        return Err("path is required".into());
    }
    let label = doc_window_label(&path);

    if let Some(win) = app.get_webview_window(&label) {
        raise_existing_window(&win);
        op.finish_ok_extra(format!("existing label={label}"));
        return Ok(());
    }

    let mut url = format!("index.html?window=doc&path={}", urlencoding::encode(&path));
    if let Some(m) = mode.as_ref().filter(|s| !s.is_empty()) {
        url.push_str(&format!("&mode={}", urlencoding::encode(m)));
    }
    if let Some(vp) = vault_path {
        url.push_str(&format!("&vault_path={}", urlencoding::encode(&vp)));
    }

    let window_title = title.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        std::path::Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Document")
            .to_string()
    });

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut builder = app
        .state::<crate::core::webview::WebviewProxyConfig>()
        .apply(WebviewWindowBuilder::new(
            &app,
            &label,
            WebviewUrl::App(url.into()),
        ))
        .title(window_title)
        .inner_size(960.0, 720.0)
        .min_inner_size(480.0, 360.0)
        .resizable(true);

    #[cfg(target_os = "macos")]
    {
        let scale = app
            .state::<AppSettingsStore>()
            .get()
            .map(|r| r.settings.ui_scale)
            .unwrap_or(1.0);
        let y = if scale.is_finite() && (0.8..=1.5).contains(&scale) {
            TRAFFIC_LIGHT_Y_DEFAULT * scale
        } else {
            TRAFFIC_LIGHT_Y_DEFAULT
        };
        builder = builder
            .visible(false)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .traffic_light_position(tauri::LogicalPosition::new(TRAFFIC_LIGHT_X, y));
    }

    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            op.finish_err_msg("doc", &e);
            return Err(e.to_string());
        }
    };
    #[cfg(target_os = "macos")]
    enable_window_tiling(&window);
    let _ = window.set_focus();

    op.finish_ok_extra(format!("new label={label}"));
    Ok(())
}

/// Anti-drift: bind the owned `WindowClosedEvent` mirror (in
/// `app::events_contract`, feeding the `window:closed` payload type in
/// bindings.ts) to the private borrowed `WindowClosedPayload` actually emitted
/// here. Borrowed fields (`&str`) cannot share compile-time type identity with
/// the owned mirror, so the binding is the serde shape (both `view` variants,
/// covering `rename_all` + `skip_serializing_if`).
#[cfg(test)]
mod events_contract_shape_tests {
    use super::WindowClosedPayload;
    use crate::app::events_contract::WindowClosedEvent as MirrorWindowClosed;

    #[test]
    fn window_closed_mirror_matches_emit_payload_shape() {
        let real = WindowClosedPayload {
            kind: "feature",
            view: Some("chat"),
        };
        let mirror = MirrorWindowClosed {
            kind: "feature".to_string(),
            view: Some("chat".to_string()),
        };
        assert_eq!(
            serde_json::to_value(&real).unwrap(),
            serde_json::to_value(&mirror).unwrap(),
            "WindowClosedEvent mirror drifted from WindowClosedPayload"
        );
        let real_none = WindowClosedPayload {
            kind: "settings",
            view: None,
        };
        let mirror_none = MirrorWindowClosed {
            kind: "settings".to_string(),
            view: None,
        };
        assert_eq!(
            serde_json::to_value(&real_none).unwrap(),
            serde_json::to_value(&mirror_none).unwrap(),
            "WindowClosedEvent mirror drifted from WindowClosedPayload (None variant)"
        );
    }
}
