//! gpui-pre: draw a fresh frame and render it offscreen through GPUI's own
//! renderer. Needs no screen permission and works for covered or hidden
//! windows, whose display link may be stopped. `Window::render_to_image` is
//! ungated in the new-horizon-code-repo/gpui fork.

use gpui::{App, Window};

use super::Rgba;

pub fn window_rgba(window: &mut Window, cx: &mut App) -> Result<Rgba, String> {
    // The next display-link tick presents this frame, so the screen catches up.
    window.draw(cx).clear(cx);
    let image = window.render_to_image().map_err(|e| format!("GPUI could not render the window: {e}"))?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("the window has no size to capture".into());
    }
    Ok(Rgba { width, height, pixels: image.into_raw() })
}
