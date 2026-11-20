//! Desktop window helpers (new window, settings window).

#[cfg(feature = "desktop")]
pub(crate) mod chatgpt_cli;
pub mod commands;
#[cfg(windows)]
pub mod pinch_zoom;
