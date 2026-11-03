//! gpui-pre with the fork (`snapshot`): render the window's last frame
//! offscreen through GPUI's own renderer. Needs no screen permission and works
//! for covered windows. The fork's gpui-pre-macos and gpui-pre-apple implement
//! it on macOS, gpui-pre-wgpu and gpui-pre-linux on Linux; with the crates.io
//! ones it errors and capture falls back to ScreenCaptureKit or the X server.

use gpui::Window;

use super::{unpremultiply, Rgba};

pub fn window_rgba(window: &Window) -> Result<Rgba, String> {
    let image = window.render_to_image().map_err(|e| format!("GPUI could not render the window: {e}"))?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("the window has no size to capture".into());
    }
    // Metal and wgpu both leave premultiplied color on the clear target.
    let mut pixels = image.into_raw();
    unpremultiply(&mut pixels);
    Ok(Rgba { width, height, pixels })
}
