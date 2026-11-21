//! Desktop window helpers (new window, settings window).

#[cfg(all(
    feature = "desktop",
    not(any(target_os = "ios", target_os = "android"))
))]
pub(crate) mod chatgpt_cli;
pub mod commands;
#[cfg(windows)]
pub mod pinch_zoom;
