//! Window capture. GPUI draws through the GPU, so a plain screen grab of a
//! covered or off-screen window comes back blank. Each platform asks for the
//! window itself: the compositor on Windows, GPUI's own renderer on gpui-pre
//! (macOS), ScreenCaptureKit on gpui 0.2 (macOS).

use gpui::{App, Window};

#[cfg(all(target_os = "macos", feature = "gpui-0-2"))]
mod macos;
#[cfg(all(target_os = "macos", feature = "gpui-pre"))]
mod render;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(all(target_os = "macos", feature = "gpui-0-2"))]
pub use macos::post_pointer;
#[cfg(all(target_os = "windows", feature = "gpui-0-2"))]
pub use windows::post_pointer;

#[cfg(all(target_os = "macos", feature = "gpui-0-2"))]
use macos::window_rgba;
#[cfg(target_os = "windows")]
use windows::window_rgba;

pub fn window_png(window: &mut Window, cx: &mut App) -> Result<Vec<u8>, String> {
    #[cfg(all(target_os = "macos", feature = "gpui-pre"))]
    let rgba = render::window_rgba(window, cx)?;
    #[cfg(not(all(target_os = "macos", feature = "gpui-pre")))]
    let rgba = {
        let _ = cx;
        window_rgba(window)?
    };
    encode_png(&rgba.pixels, rgba.width, rgba.height)
}

/// Straight RGBA, row-major, top row first.
#[allow(dead_code)]
pub(crate) struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn encode_png(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(pixels).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(all(not(any(target_os = "windows", target_os = "macos")), feature = "gpui-0-2"))]
pub fn post_pointer(_window: &Window, _x: f32, _y: f32, _click: bool) -> Result<(), String> {
    Err("picking by point is implemented for Windows and macOS only so far; pick in the window instead".into())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn window_rgba(_window: &Window) -> Result<Rgba, String> {
    Err("window capture is implemented for Windows and macOS only so far".into())
}
