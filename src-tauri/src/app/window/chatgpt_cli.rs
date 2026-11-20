//! CLI access to the visible ChatGPT page in the existing sidebar WebView.

use agentero_core::features::open_request::{write_local_ui_response, UiRequestPayload};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, Url, Webview};

use super::commands::{eval_chatgpt_script, CHATGPT_SIDEBAR_WEBVIEW_LABEL};

const ORIGIN: &str = "https://chatgpt.com";
const SCRIPT: &str = include_str!("chatgpt-cli.js");
static PAGE_ACTION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn handle<R: Runtime>(app: &AppHandle<R>, request: UiRequestPayload) {
    let Some(id) = request.reference.as_deref() else {
        return;
    };
    let result = dispatch(app, id, request.path.as_deref()).await;
    let mut reply = match result {
        Ok(result) => result,
        Err(error) => json!({ "error": error }),
    };
    if !reply.is_object() {
        reply = json!({ "error": "Unexpected ChatGPT page response" });
    }
    let payload = reply.to_string();
    let payload = if payload.len() > 1024 * 1024 {
        r#"{"error":"The ChatGPT response exceeds the CLI transfer limit of 1 MiB"}"#
    } else {
        &payload
    };
    let _ = write_local_ui_response(id, payload);
}

async fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    raw: Option<&str>,
) -> Result<Value, &'static str> {
    let _lock = PAGE_ACTION
        .try_lock()
        .map_err(|_| "Another ChatGPT CLI action is running")?;
    let request: Value =
        serde_json::from_str(raw.ok_or("Missing ChatGPT action")?).map_err(|_| "Invalid action")?;
    let webview = app
        .get_webview(CHATGPT_SIDEBAR_WEBVIEW_LABEL)
        .ok_or("Open the ChatGPT sidebar first with agentero ui command chatgpt.open")?;
    check_origin(&webview)?;
    if matches!(
        request["operation"].as_str(),
        Some(
            "open"
                | "project-chats"
                | "project-sources"
                | "rename-project"
                | "project-instructions"
                | "project-source"
                | "export"
        )
    ) {
        if let Some(target) = request["target"].as_str().and_then(chatgpt_route) {
            navigate(&webview, target).await?;
        }
    }
    let slot = json!(format!("__agentero_chatgpt_cli_{id}"));
    let started: Option<bool> =
        eval_chatgpt_script(&webview, format!("{SCRIPT}({slot}, {request});")).await;
    if started != Some(true) {
        return Err("The ChatGPT page did not start the action");
    }

    let deadline = Instant::now() + Duration::from_secs(37);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(160)).await;
        check_origin(&webview)?;
        let poll = format!(
            "(() => {{ if (location.origin !== {ORIGIN:?}) return null; \
             if (!Object.hasOwn(globalThis, {slot})) return {{error: 'The ChatGPT page changed during the action'}}; \
             const result = globalThis[{slot}]; if (result) delete globalThis[{slot}]; \
             return result; }})()"
        );
        if let Some(Some(result)) = eval_chatgpt_script::<R, Option<Value>>(&webview, poll).await {
            return Ok(result);
        }
    }
    Err("Timed out waiting for the ChatGPT page. Check its current state before retrying.")
}

fn chatgpt_route(path: &str) -> Option<Url> {
    let path = path.strip_prefix('/')?.trim_end_matches('/');
    let segments = path.split('/').collect::<Vec<_>>();
    let valid_id = |id: &str| {
        !id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    };
    let valid = match segments.as_slice() {
        ["c", id] => valid_id(id),
        ["g", project] => project.starts_with("g-") && valid_id(project),
        ["g", project, "project"] => project.starts_with("g-") && valid_id(project),
        ["g", project, "c", id] => project.starts_with("g-") && valid_id(project) && valid_id(id),
        _ => false,
    };
    valid
        .then(|| Url::parse(&format!("{ORIGIN}/{path}")).ok())
        .flatten()
}

async fn navigate<R: Runtime>(webview: &Webview<R>, mut target: Url) -> Result<(), &'static str> {
    let mut path = target.path().to_owned();
    if path.starts_with("/g/") && path.matches('/').count() == 2 {
        path.push_str("/project");
        target.set_path(&path);
    }
    if webview.url().is_ok_and(|url| url.path() == path) {
        return Ok(());
    }
    webview
        .navigate(target)
        .map_err(|_| "ChatGPT navigation failed")?;
    let deadline = Instant::now() + Duration::from_secs(13);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if webview.url().is_ok_and(|url| url.path() == path) {
            let ready: Option<bool> = eval_chatgpt_script(
                webview,
                format!("location.origin === {ORIGIN:?} && document.readyState !== 'loading' && !!document.querySelector('main')"),
            ).await;
            if ready == Some(true) {
                return Ok(());
            }
        }
    }
    Err("The requested ChatGPT page did not load")
}

fn check_origin<R: Runtime>(webview: &Webview<R>) -> Result<(), &'static str> {
    if webview
        .url()
        .is_ok_and(|url| url.origin().ascii_serialization() == ORIGIN)
    {
        Ok(())
    } else {
        Err("The sidebar must be on https://chatgpt.com")
    }
}
