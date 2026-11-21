//! `agentero ui screenshot`: capture a native Agentero window to PNG.
//!
//! The Host captures the whole native window, so the PDF canvas and child
//! WebViews (ChatGPT) are included. Capturing in-process uses the app's own
//! per-monitor DPI awareness, so mixed-DPI monitors yield physical pixels.

use super::{local_ui_screenshot_path, trunc, write_local_ui_response, UiRequestPayload};
use std::time::Instant;
use tauri::{AppHandle, Manager, Runtime};

/// Capture the requested window and answer the CLI request.
pub fn handle<R: Runtime>(app: &AppHandle<R>, req: &UiRequestPayload) {
    let Some(id) = req.reference.as_deref() else {
        return;
    };
    let label = req.window.as_deref().unwrap_or("main");
    let started = Instant::now();
    log::info!(
        target: "agentero::op",
        "op start ui_action id={id} action=screenshot window={}",
        trunc(label)
    );
    let result = local_ui_screenshot_path(id)
        .map_err(|error| error.to_string())
        .and_then(|path| {
            let window = app
                .get_window(label)
                .ok_or_else(|| format!("Agentero window {label} is not open"))?;
            let (width, height) = capture(&window, &path)?;
            Ok((width, height, path))
        });
    let response = match &result {
        Ok((width, height, path)) => serde_json::json!({
            "action": "screenshot",
            "window": label,
            "width": width,
            "height": height,
            "image": path,
        }),
        Err(error) => serde_json::json!({ "error": error }),
    };
    let duration_ms = started.elapsed().as_millis();
    match &result {
        Ok(_) => log::info!(
            target: "agentero::op",
            "op end ui_action ok=true duration_ms={duration_ms} id={id} action=screenshot window={}",
            trunc(label)
        ),
        Err(error) => log::warn!(
            target: "agentero::op",
            "op end ui_action ok=false duration_ms={duration_ms} id={id} action=screenshot window={} error={}",
            trunc(label),
            trunc(error)
        ),
    }
    let _ = write_local_ui_response(id, &response.to_string());
}

#[cfg(windows)]
fn capture<R: Runtime>(
    window: &tauri::Window<R>,
    path: &std::path::Path,
) -> Result<(u32, u32), String> {
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

    let hwnd = window.hwnd().map_err(|error| error.to_string())?.0;
    // SAFETY: `hwnd` is a live top-level window owned by this process; every
    // GDI handle created below is released before returning.
    unsafe {
        if IsIconic(hwnd) != 0 {
            return Err("The window is minimized. Restore it before taking a screenshot.".into());
        }
        if IsWindowVisible(hwnd) == 0 {
            return Err("The window is hidden.".into());
        }
        let mut outer: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut outer) == 0 {
            return Err("Could not read the window bounds.".into());
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
            return Err("The window has no visible area.".into());
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
            return Err("Windows could not render the window for capture.".into());
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
            return Err(
                "The captured image is empty. Bring the window on screen and retry.".into(),
            );
        }
        write_png(path, width, height, &rgb)?;
        Ok((width, height))
    }
}

#[cfg(not(windows))]
fn capture<R: Runtime>(
    _window: &tauri::Window<R>,
    _path: &std::path::Path,
) -> Result<(u32, u32), String> {
    Err("ui screenshot is only implemented on Windows. Use the system screenshot tool.".into())
}

#[cfg(windows)]
fn write_png(path: &std::path::Path, width: u32, height: u32, rgb: &[u8]) -> Result<(), String> {
    let failed = |error: &dyn std::fmt::Display| format!("failed to write screenshot: {error}");
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
