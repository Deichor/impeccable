//! Window capture. GPUI draws through the GPU, so a plain screen grab of a
//! covered or off-screen window comes back blank. Each platform asks for the
//! window itself: the compositor on Windows, and on macOS GPUI's own renderer
//! (gpui-pre with the fork, `snapshot`) or else ScreenCaptureKit. Linux takes
//! the same renderer route, else the X server.

use std::sync::mpsc;

use gpui::Window;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(any(target_os = "macos", target_os = "linux"), feature = "snapshot"))]
mod render;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(all(target_os = "linux", feature = "gpui-0-2"))]
pub use linux::post_pointer;
#[cfg(all(target_os = "macos", feature = "gpui-0-2"))]
pub use macos::post_pointer;
#[cfg(all(target_os = "windows", feature = "gpui-0-2"))]
pub use windows::post_pointer;

/// Straight RGBA, row-major, top row first.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A capture in flight: done at once, or answered later off the main thread.
#[allow(dead_code)]
pub enum Shot {
    Done(Result<Rgba, String>),
    Pending(mpsc::Receiver<Result<Rgba, String>>),
}

/// Start capturing the window's latest frame; the caller makes sure one was drawn.
pub fn start(window: &Window) -> Result<Shot, String> {
    #[cfg(target_os = "windows")]
    return Ok(Shot::Done(windows::window_rgba(window)));
    #[cfg(all(target_os = "macos", feature = "snapshot"))]
    if let Ok(rgba) = render::window_rgba(window) {
        return Ok(Shot::Done(Ok(rgba)));
    }
    // Without the fork's macOS renderer, render_to_image is not implemented.
    #[cfg(target_os = "macos")]
    return macos::start(window);
    #[cfg(target_os = "linux")]
    return linux::start(window);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = window;
        Err("window capture is implemented for Windows, macOS and Linux only so far".into())
    }
}

/// Why a pending capture never answered.
pub fn timeout_error() -> String {
    #[cfg(target_os = "macos")]
    return macos::timeout_error();
    #[cfg(not(target_os = "macos"))]
    "the capture did not finish in time".into()
}

pub fn png(rgba: Rgba) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, rgba.width, rgba.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba.pixels).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Premultiplied RGBA to straight alpha, in place.
#[allow(dead_code)]
fn unpremultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
#[cfg(feature = "gpui-0-2")]
pub fn post_pointer(_window: &Window, _x: f32, _y: f32, _click: bool) -> Result<(), String> {
    Err("picking by point is implemented for Windows, macOS and Linux only so far; pick in the window instead".into())
}

#[cfg(test)]
mod tests {
    use super::unpremultiply;

    #[test]
    fn unpremultiply_restores_straight_alpha_and_keeps_opaque_and_clear_pixels() {
        let mut px = [64, 32, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        unpremultiply(&mut px);
        assert_eq!(px, [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }
}
