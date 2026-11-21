//! `agentero ui screenshot`: capture an Agentero window to PNG.
//!
//! - Windows: the Host prints the whole native window (title bar, PDF canvas,
//!   ChatGPT child WebViews) with `PrintWindow(PW_RENDERFULLCONTENT)`, in
//!   physical pixels thanks to the app's per-monitor DPI awareness.
//! - macOS: `screencapture -l <window number>` captures the native window.
//!   macOS asks for Screen Recording permission on first use.
//! - Linux (X11 and Wayland): WebKitGTK snapshots the visible page of the
//!   window's main webview; the title bar and child webviews are not included.

use super::host_action::{HostAction, HostActionError};
use super::{local_ui_screenshot_path, UiRequestPayload};
use std::path::Path;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use tauri::Manager;
use tauri::{AppHandle, Runtime};

/// Capture the requested window and answer the CLI request.
pub fn handle<R: Runtime>(app: &AppHandle<R>, req: &UiRequestPayload) {
    let Some(action) = HostAction::start(req) else {
        return;
    };
    let id = req.reference.as_deref().unwrap_or_default();
    let label = action.window().to_owned();
    let result = local_ui_screenshot_path(id)
        .map_err(|error| HostActionError::new("action_failed", error.to_string()))
        .and_then(|path| {
            let (width, height) = capture(app, &label, &path)?;
            Ok(serde_json::json!({
                "action": "screenshot",
                "window": label,
                "width": width,
                "height": height,
                "image": path,
            }))
        });
    action.finish(result);
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn not_open(label: &str) -> HostActionError {
    HostActionError::new(
        "window_not_open",
        format!("Agentero window {label} is not open"),
    )
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn minimized() -> HostActionError {
    HostActionError::new(
        "window_minimized",
        "The window is minimized. Restore it before taking a screenshot.",
    )
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn hidden() -> HostActionError {
    HostActionError::new("window_hidden", "The window is hidden.")
}

#[cfg(any(windows, target_os = "linux"))]
fn blank() -> HostActionError {
    HostActionError::new(
        "capture_blank",
        "The captured image is empty. Bring the window on screen and retry.",
    )
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn capture_failed(error: impl std::fmt::Display) -> HostActionError {
    HostActionError::new(
        "capture_failed",
        format!("Could not capture the window: {error}"),
    )
}

#[cfg(windows)]
fn capture<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    path: &Path,
) -> Result<(u32, u32), HostActionError> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic, IsWindowVisible};

    // Undocumented in the SDK headers; renders DirectComposition (WebView2) content.
    const PW_RENDERFULLCONTENT: u32 = 2;

    let window = app.get_window(label).ok_or_else(|| not_open(label))?;
    let hwnd = window.hwnd().map_err(capture_failed)?.0;
    // SAFETY: `hwnd` is a live top-level window owned by this process; every
    // GDI handle created below is released before returning.
    unsafe {
        if IsIconic(hwnd) != 0 {
            return Err(minimized());
        }
        if IsWindowVisible(hwnd) == 0 {
            return Err(hidden());
        }
        let mut outer: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut outer) == 0 {
            return Err(capture_failed("could not read the window bounds"));
        }
        // The visible frame excludes the invisible resize borders.
        let mut frame: RECT = outer;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut frame as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        ) != 0
        {
            frame = outer;
        }
        let full_width = outer.right - outer.left;
        let full_height = outer.bottom - outer.top;
        if full_width <= 0 || full_height <= 0 {
            return Err(blank());
        }

        let screen = GetDC(std::ptr::null_mut());
        let memory = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, full_width, full_height);
        let previous = SelectObject(memory, bitmap);
        let printed = PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT) != 0;
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: full_width,
            // Negative height: rows top-down.
            biHeight: -full_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bgra = vec![0u8; full_width as usize * full_height as usize * 4];
        let rows = if printed {
            GetDIBits(
                memory,
                bitmap,
                0,
                full_height as u32,
                bgra.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        SelectObject(memory, previous);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
        if !printed || rows != full_height {
            return Err(capture_failed("Windows could not render the window"));
        }

        let left = (frame.left - outer.left).clamp(0, full_width);
        let top = (frame.top - outer.top).clamp(0, full_height);
        let right = (frame.right - outer.left).clamp(left, full_width);
        let bottom = (frame.bottom - outer.top).clamp(top, full_height);
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for y in top..bottom {
            let row = y as usize * full_width as usize * 4;
            for x in left..right {
                let pixel = row + x as usize * 4;
                rgb.extend_from_slice(&[bgra[pixel + 2], bgra[pixel + 1], bgra[pixel]]);
            }
        }
        if width == 0 || height == 0 || rgb.iter().all(|&byte| byte == 0) {
            return Err(blank());
        }
        write_png(path, width, height, &rgb)?;
        Ok((width, height))
    }
}

#[cfg(target_os = "macos")]
fn capture<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    path: &Path,
) -> Result<(u32, u32), HostActionError> {
    let window = app.get_window(label).ok_or_else(|| not_open(label))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let target = window.clone();
    // AppKit window state must be read on the main thread.
    window
        .run_on_main_thread(move || {
            use objc2_app_kit::NSWindow;
            let state = target.ns_window().ok().and_then(|pointer| {
                // SAFETY: Tauri returns this window's live NSWindow, and the
                // closure runs on the main thread.
                unsafe { (pointer as *mut NSWindow).as_ref() }.map(|window| {
                    (
                        window.windowNumber(),
                        window.isMiniaturized(),
                        window.isVisible(),
                    )
                })
            });
            let _ = sender.send(state);
        })
        .map_err(capture_failed)?;
    let (number, is_minimized, is_visible) = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .ok()
        .flatten()
        .ok_or_else(|| capture_failed("the window did not answer"))?;
    if is_minimized {
        return Err(minimized());
    }
    if !is_visible {
        return Err(hidden());
    }
    let output = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", "-t", "png", "-l"])
        .arg(number.to_string())
        .arg(path)
        .output()
        .map_err(capture_failed)?;
    if !output.status.success() || !path.is_file() {
        return Err(HostActionError::new(
            "capture_failed",
            "macOS could not capture the window. Allow Screen Recording for Agentero \
             (or the terminal that started it) in System Settings > Privacy & Security, then retry.",
        ));
    }
    png_size(path).ok_or_else(|| capture_failed("screencapture did not write a PNG"))
}

/// Width and height from the PNG IHDR chunk.
#[cfg(target_os = "macos")]
fn png_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut header = [0u8; 24];
    std::fs::File::open(path)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

#[cfg(target_os = "linux")]
fn capture<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    path: &Path,
) -> Result<(u32, u32), HostActionError> {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};

    let webview = app.get_webview(label).ok_or_else(|| not_open(label))?;
    let window = webview.window();
    if window.is_minimized().unwrap_or(false) {
        return Err(minimized());
    }
    if !window.is_visible().unwrap_or(true) {
        return Err(hidden());
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    // with_webview runs on the GTK main thread, where WebKit must be called.
    webview
        .with_webview(move |platform| {
            platform.inner().snapshot(
                SnapshotRegion::Visible,
                SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    let _ = sender.send(
                        result
                            .map_err(|error| error.to_string())
                            .and_then(surface_rgb),
                    );
                },
            );
        })
        .map_err(capture_failed)?;
    let (width, height, rgb) = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| capture_failed("WebKit did not return a snapshot"))?
        .map_err(capture_failed)?;
    if width == 0 || height == 0 || rgb.iter().all(|&byte| byte == 0) {
        return Err(blank());
    }
    write_png(path, width, height, &rgb)?;
    Ok((width, height))
}

/// Cairo ARGB32 / RGB24 surface (native-endian `u32` pixels) to RGB bytes.
#[cfg(target_os = "linux")]
fn surface_rgb(surface: cairo::Surface) -> Result<(u32, u32, Vec<u8>), String> {
    let mut image = cairo::ImageSurface::try_from(surface)
        .map_err(|_| "the snapshot is not an image surface".to_string())?;
    if !matches!(image.format(), cairo::Format::ARgb32 | cairo::Format::Rgb24) {
        return Err(format!("unsupported snapshot format {:?}", image.format()));
    }
    let width = image.width().max(0) as usize;
    let height = image.height().max(0) as usize;
    let stride = image.stride().max(0) as usize;
    let data = image.data().map_err(|error| error.to_string())?;
    if data.len() < stride * height || stride < width * 4 {
        return Err("short snapshot buffer".into());
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let start = y * stride + x * 4;
            let pixel = u32::from_ne_bytes([
                data[start],
                data[start + 1],
                data[start + 2],
                data[start + 3],
            ]);
            // 0xAARRGGBB; alpha is ignored (page content is opaque).
            rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
    }
    Ok((width as u32, height as u32, rgb))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn capture<R: Runtime>(
    _app: &AppHandle<R>,
    _label: &str,
    _path: &Path,
) -> Result<(u32, u32), HostActionError> {
    Err(HostActionError::new(
        "unsupported_platform",
        "ui screenshot is not available on this platform. Use the system screenshot tool.",
    ))
}

#[cfg(any(windows, target_os = "linux"))]
fn write_png(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<(), HostActionError> {
    let failed = |error: &dyn std::fmt::Display| {
        HostActionError::new(
            "action_failed",
            format!("failed to write screenshot: {error}"),
        )
    };
    let file = std::fs::File::create(path).map_err(|error| failed(&error))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| failed(&error))?;
    writer
        .write_image_data(rgb)
        .map_err(|error| failed(&error))?;
    writer.finish().map_err(|error| failed(&error))
}
