//! Shared desktop WebView environment settings.

use tauri::{webview::WebviewBuilder, Manager, Runtime, Url, WebviewWindowBuilder};

/// The proxy snapshot applied to the shared WebView2 environment.
///
/// WebView2 rejects a new WebView if its environment options differ from the
/// running environment that uses the same data directory. Keep one snapshot
/// for the process and apply it to every configured and dynamically created
/// WebView, while retaining the existing persistent profile.
#[derive(Clone, Default)]
pub struct WebviewProxyConfig {
    proxy_url: Option<Url>,
}

impl WebviewProxyConfig {
    pub fn new(proxy_url: Option<Url>) -> Self {
        Self { proxy_url }
    }

    pub fn apply<'a, R: Runtime, M: Manager<R>>(
        &self,
        mut builder: WebviewWindowBuilder<'a, R, M>,
    ) -> WebviewWindowBuilder<'a, R, M> {
        if let Some(proxy_url) = &self.proxy_url {
            builder = builder.proxy_url(proxy_url.clone());
        }
        builder
    }

    /// Apply the same snapshot to an embedded child WebView.
    pub fn apply_webview<R: Runtime>(&self, mut builder: WebviewBuilder<R>) -> WebviewBuilder<R> {
        if let Some(proxy_url) = &self.proxy_url {
            builder = builder.proxy_url(proxy_url.clone());
        }
        builder
    }
}
