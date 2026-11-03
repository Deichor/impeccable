//! Linux capture and picking.
//!
//! Capture prefers GPUI's own renderer (`Window::render_to_image`, `snapshot`
//! feature with the patched GPUI): it draws the last frame offscreen, so it
//! works the same on X11 and Wayland, covered or not. Without it, X11 reads the
//! window from the server (the compositor's pixmap when one runs, else the
//! window itself). Wayland gives no client another window's pixels.
//!
//! Picking by point is only needed on gpui 0.2 (gpui-pre dispatches events
//! itself); X11 gets it through XTEST, Wayland has no way to inject input.

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use gpui::Window;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder};
use x11rb::rust_connection::RustConnection;

use super::{Rgba, Shot};

enum Session {
    X11(u32),
    Wayland,
}

/// One X connection for the probe's lifetime; dropped on any X error so the
/// next request reconnects.
struct X {
    conn: RustConnection,
    #[cfg_attr(not(feature = "gpui-0-2"), allow(dead_code))]
    screen: usize,
}

static X_CONN: Mutex<Option<Arc<X>>> = Mutex::new(None);
/// gpui 0.2 window id to X11 window, found once per window.
#[cfg(feature = "gpui-0-2")]
static X_WINDOWS: Mutex<Vec<(u64, u32)>> = Mutex::new(Vec::new());

fn xconn() -> Result<Arc<X>, String> {
    let mut slot = X_CONN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = slot.as_ref() {
        return Ok(x.clone());
    }
    let (conn, screen) =
        x11rb::connect(None).map_err(|e| format!("cannot reach the X server named by DISPLAY: {e}"))?;
    let x = Arc::new(X { conn, screen });
    *slot = Some(x.clone());
    Ok(x)
}

/// Drop the cached connection and window ids after an X error.
fn forget() {
    *X_CONN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    #[cfg(feature = "gpui-0-2")]
    X_WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn device_size(window: &Window) -> (u16, u16) {
    let scale = window.scale_factor();
    let size = window.viewport_size();
    ((f32::from(size.width) * scale).round() as u16, (f32::from(size.height) * scale).round() as u16)
}

#[cfg(feature = "gpui-pre")]
fn session(window: &Window) -> Result<Session, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|e| {
        format!(
            "the window has no X11 or Wayland handle ({e}); GPUI may have found no display and run headless. \
             Check DISPLAY or WAYLAND_DISPLAY (an empty WAYLAND_DISPLAY still counts as set)"
        )
    })?;
    match handle.as_raw() {
        RawWindowHandle::Xcb(h) => Ok(Session::X11(h.window.get())),
        RawWindowHandle::Xlib(h) => Ok(Session::X11(h.window as u32)),
        RawWindowHandle::Wayland(_) => Ok(Session::Wayland),
        _ => Err("not an X11 or Wayland window".into()),
    }
}

/// gpui 0.2's X11 window panics on `window_handle`, so the backend comes from
/// GPUI's own choice and the X11 window from its `_NET_WM_PID`.
#[cfg(feature = "gpui-0-2")]
fn session(window: &Window) -> Result<Session, String> {
    match gpui::guess_compositor() {
        "Wayland" => Ok(Session::Wayland),
        "X11" => {
            let key = window.window_handle().window_id().as_u64();
            let mut known = X_WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(&(_, id)) = known.iter().find(|(k, _)| *k == key) {
                return Ok(Session::X11(id));
            }
            let id = x11_window_of_this_process(&*xconn()?, device_size(window))?;
            known.push((key, id));
            Ok(Session::X11(id))
        }
        _ => Err("GPUI runs headless: neither DISPLAY nor WAYLAND_DISPLAY is set".into()),
    }
}

/// This process's top-level X11 window, preferring one of `size` (device pixels).
#[cfg(feature = "gpui-0-2")]
fn x11_window_of_this_process(x: &X, size: (u16, u16)) -> Result<u32, String> {
    let conn = &x.conn;
    let e = |e: &dyn std::fmt::Display| format!("X11 window lookup failed: {e}");
    let pid_atom = conn.intern_atom(true, b"_NET_WM_PID").map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?.atom;
    let pid = std::process::id();
    let mut found = Vec::new();
    // Breadth-first under the root, deep enough for a window manager's frames.
    let mut level = vec![conn.setup().roots[x.screen].root];
    for _ in 0..4 {
        let mut next = Vec::new();
        for parent in level {
            let Ok(Ok(tree)) = conn.query_tree(parent).map(|c| c.reply()) else { continue };
            for w in tree.children {
                let owner = conn
                    .get_property(false, w, pid_atom, x11rb::protocol::xproto::AtomEnum::CARDINAL, 0, 1)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .and_then(|p| p.value32().and_then(|mut v| v.next()));
                if owner == Some(pid) {
                    found.push(w);
                } else {
                    next.push(w);
                }
            }
        }
        level = next;
    }
    let sized = found.iter().copied().find(|&w| {
        conn.get_geometry(w).ok().and_then(|c| c.reply().ok()).is_some_and(|g| (g.width, g.height) == size)
    });
    sized
        .or(found.first().copied())
        .ok_or_else(|| "found no X11 window carrying this process's _NET_WM_PID".into())
}

/// Start a capture. The X11 read runs off the main thread, since it waits for
/// the last present to land.
pub fn start(window: &Window) -> Result<Shot, String> {
    #[cfg(feature = "snapshot")]
    let rendered = match super::render::window_rgba(window) {
        Ok(rgba) => return Ok(Shot::Done(Ok(rgba))),
        Err(e) => Some(e),
    };
    #[cfg(not(feature = "snapshot"))]
    let rendered: Option<String> = None;

    let first = rendered.clone();
    let also = move |e: String| match &first {
        Some(r) => format!("{e}. The renderer route failed too: {r}"),
        None => e,
    };
    let session = session(window).inspect_err(|_| forget()).map_err(&also)?;
    match session {
        Session::X11(id) => {
            let want = device_size(window);
            let x = xconn().map_err(&also)?;
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let shot = settled_rgba(&x, id, want).inspect_err(|_| forget()).map_err(also);
                let _ = tx.send(shot);
            });
            Ok(Shot::Pending(rx))
        }
        Session::Wayland => Err(match rendered {
            Some(r) => format!(
                "on Wayland the probe captures through GPUI's renderer only, and it failed ({r}). \
                 Patch gpui-pre-wgpu and gpui-pre-linux from new-horizon-code-repo/gpui as well as gpui-pre \
                 (see the probe README), or start the app with WAYLAND_DISPLAY unset to run it under X11"
            ),
            None => "on Wayland no app can read another window's pixels, so the probe captures through \
                     GPUI's renderer, which needs gpui-pre with the `snapshot` feature and the \
                     new-horizon-code-repo/gpui patch (see the probe README). Or start the app with \
                     WAYLAND_DISPLAY unset to run it under X11"
                .into(),
        }),
    }
}

/// A frame callback fires before the GPU present reaches the X server, so a
/// single read can still hold the previous size or appearance. Read until two
/// reads 40 ms apart agree at the size GPUI laid out, for at most about 0.6 s.
fn settled_rgba(x: &X, window: u32, want: (u16, u16)) -> Result<Rgba, String> {
    let mut last: Option<Read> = None;
    for _ in 0..12 {
        let read = x11_read(x, window)?;
        let sized = (read.rgba.width, read.rgba.height) == (u32::from(want.0), u32::from(want.1));
        let same = last.as_ref().is_some_and(|l| l.rgba.pixels == read.rgba.pixels);
        if sized && same && !read.blank {
            return Ok(read.rgba);
        }
        last = Some(read);
        std::thread::sleep(Duration::from_millis(40));
    }
    let read = last.expect("at least one read");
    if read.blank {
        return Err("the X server holds only black for this window. Either the app's GPU frames never reached \
                    it (Xvfb with Mesa's software Vulkan driver before Mesa 25 does this) or the window really is all \
                    black. Run the app on a GPU-backed X server, or move it to gpui-pre with the probe's \
                    `snapshot` feature, which captures through GPUI's renderer"
            .into());
    }
    Ok(read.rgba)
}

struct Read {
    rgba: Rgba,
    /// Every pixel black. A window that never presented reads as opaque black
    /// too, so the two cannot be told apart; a real UI is rarely all black.
    blank: bool,
}

fn x11_read(x: &X, window: u32) -> Result<Read, String> {
    let conn = &x.conn;
    let e = |e: &dyn std::fmt::Display| format!("X11 capture failed: {e}");
    let geo = conn.get_geometry(window).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?;
    let visual = conn
        .get_window_attributes(window)
        .map_err(|x| e(&x))?
        .reply()
        .map_err(|x| e(&x))?
        .visual;
    let pixmap = composite_pixmap(conn, window);
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, pixmap.unwrap_or(window), 0, 0, geo.width, geo.height, !0)
        .map_err(|x| e(&x))?
        .reply();
    if let Some(p) = pixmap {
        let _ = conn.free_pixmap(p);
        let _ = conn.flush();
    }
    let image = reply.map_err(|x| {
        format!("X11 capture failed: {x}; the window must be mapped (not minimized) and on screen")
    })?;
    let count = geo.width as usize * geo.height as usize;
    let pixels = to_rgba(conn, &image.data, image.depth, visual, count)?;
    let blank = pixels.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]);
    Ok(Read { rgba: Rgba { width: geo.width as u32, height: geo.height as u32, pixels }, blank })
}

/// The window's offscreen pixmap, kept only while a compositing manager
/// redirects the window. Covered parts are then still readable.
fn composite_pixmap(conn: &RustConnection, window: u32) -> Option<u32> {
    conn.extension_information(composite::X11_EXTENSION_NAME).ok()??;
    conn.composite_query_version(0, 2).ok()?.reply().ok()?;
    let pixmap = conn.generate_id().ok()?;
    conn.composite_name_window_pixmap(window, pixmap).ok()?.check().ok()?;
    Some(pixmap)
}

/// Shift of an 8-bit color channel mask.
fn channel(mask: u32) -> Result<u32, String> {
    let shift = mask.trailing_zeros();
    if mask != 0 && mask >> shift == 0xff {
        Ok(shift)
    } else {
        Err(format!("X11 capture: unsupported color mask {mask:#x}"))
    }
}

/// Z-pixmap data at 32 bits per pixel to opaque RGBA, through the visual's masks.
fn to_rgba(conn: &RustConnection, data: &[u8], depth: u8, visual: u32, count: usize) -> Result<Vec<u8>, String> {
    let setup = conn.setup();
    let bpp = setup.pixmap_formats.iter().find(|f| f.depth == depth).map(|f| f.bits_per_pixel);
    if bpp != Some(32) || data.len() < count * 4 {
        return Err(format!("X11 capture: unsupported pixel format (depth {depth}, {bpp:?} bits per pixel)"));
    }
    let vt = setup
        .roots
        .iter()
        .flat_map(|s| &s.allowed_depths)
        .flat_map(|d| &d.visuals)
        .find(|v| v.visual_id == visual)
        .ok_or("X11 capture: the window's visual is unknown")?;
    let (r, g, b) = (channel(vt.red_mask)?, channel(vt.green_mask)?, channel(vt.blue_mask)?);
    let big = setup.image_byte_order == ImageOrder::MSB_FIRST;
    let mut out = Vec::with_capacity(count * 4);
    for px in data.chunks_exact(4).take(count) {
        let px = [px[0], px[1], px[2], px[3]];
        let v = if big { u32::from_be_bytes(px) } else { u32::from_le_bytes(px) };
        out.extend_from_slice(&[(v >> r) as u8, (v >> g) as u8, (v >> b) as u8, 255]);
    }
    Ok(out)
}

/// Where the real pointer was before the first synthetic move of a pick.
#[cfg(feature = "gpui-0-2")]
static POINTER_HOME: Mutex<Option<(u32, i16, i16)>> = Mutex::new(None);

/// Move the real pointer to a window point (logical pixels) and, with
/// `click`, press and release the left button, through XTEST. GPUI reads
/// XInput2 events, which a synthetic `SendEvent` does not produce. XTEST acts
/// on whatever is on top, so a point another window covers is refused. The
/// pointer goes back where it was after the click; a move alone leaves it, so
/// the hover it causes lasts until the click.
#[cfg(feature = "gpui-0-2")]
pub fn post_pointer(window: &Window, x: f32, y: f32, click: bool) -> Result<(), String> {
    let id = match session(window).inspect_err(|_| forget())? {
        Session::X11(id) => id,
        Session::Wayland => {
            return Err("on Wayland no app can send input to another window, so a gpui 0.2 app picks by hand: \
                        ctrl-alt-i in the app, hover, click. gpui-pre apps pick by point on Wayland too"
                .into())
        }
    };
    let scale = window.scale_factor();
    let point = ((x * scale).round() as i16, (y * scale).round() as i16);
    xtest_pointer(&*xconn()?, id, point, click).inspect_err(|_| forget())
}

#[cfg(feature = "gpui-0-2")]
fn xtest_pointer(x: &X, id: u32, (px, py): (i16, i16), click: bool) -> Result<(), String> {
    use x11rb::protocol::{xproto, xtest, xtest::ConnectionExt as _};

    let conn = &x.conn;
    let e = |e: &dyn std::fmt::Display| format!("X11 pointer failed: {e}");
    conn.extension_information(xtest::X11_EXTENSION_NAME)
        .map_err(|x| e(&x))?
        .ok_or("the X server has no XTEST extension, so the probe cannot move the pointer; pick in the window instead")?;
    let geo = conn.get_geometry(id).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?;
    if px < 0 || py < 0 || px as u16 >= geo.width || py as u16 >= geo.height {
        return Err(format!("the point is outside the window ({}x{} device pixels)", geo.width, geo.height));
    }
    let root = geo.root;
    let at = conn.translate_coordinates(id, root, px, py).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?;
    // The root's child at that point is the topmost window there.
    let top = conn
        .translate_coordinates(root, root, at.dst_x, at.dst_y)
        .map_err(|x| e(&x))?
        .reply()
        .map_err(|x| e(&x))?
        .child;
    if top != toplevel(conn, id, root)? {
        return Err("another window covers the app at that point, and XTEST would click it instead. \
                    Bring the app to the front, or pick in the window (ctrl-alt-i)"
            .into());
    }
    let mut home = POINTER_HOME.lock().unwrap_or_else(|e| e.into_inner());
    if home.is_none() {
        let p = conn.query_pointer(root).map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?;
        *home = Some((root, p.root_x, p.root_y));
    }
    let fake = |kind: u8, detail: u8, (x, y): (i16, i16)| {
        conn.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, root, x, y, 0)
            .map(drop)
            .map_err(|x| e(&x))
    };
    fake(xproto::MOTION_NOTIFY_EVENT, 0, (at.dst_x, at.dst_y))?;
    if click {
        fake(xproto::BUTTON_PRESS_EVENT, 1, (0, 0))?;
        fake(xproto::BUTTON_RELEASE_EVENT, 1, (0, 0))?;
        if let Some((home_root, hx, hy)) = home.take().filter(|h| h.0 == root) {
            conn.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, x11rb::CURRENT_TIME, home_root, hx, hy, 0)
                .map(drop)
                .map_err(|x| e(&x))?;
        }
    }
    // A round trip, so the server has queued the events before we return.
    conn.get_input_focus().map_err(|x| e(&x))?.reply().map_err(|x| e(&x))?;
    Ok(())
}

/// The root's child that holds `window`: the window itself, or its window manager frame.
#[cfg(feature = "gpui-0-2")]
fn toplevel(conn: &RustConnection, window: u32, root: u32) -> Result<u32, String> {
    let mut w = window;
    for _ in 0..8 {
        let parent = conn
            .query_tree(w)
            .map_err(|x| format!("X11 pointer failed: {x}"))?
            .reply()
            .map_err(|x| format!("X11 pointer failed: {x}"))?
            .parent;
        if parent == root || parent == x11rb::NONE {
            return Ok(w);
        }
        w = parent;
    }
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

    #[test]
    fn a_zero_or_odd_color_mask_is_an_error_not_a_panic() {
        assert_eq!(channel(0xff0000), Ok(16));
        assert!(channel(0).is_err());
        assert!(channel(0x3ff).is_err());
    }

    /// Reads a plain X window of a known color. Runs only with a DISPLAY (an X server such as Xvfb).
    #[test]
    fn x11_capture_reads_a_window_through_its_visual() {
        if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
            return;
        }
        let x = xconn().unwrap();
        let conn = &x.conn;
        let root = &conn.setup().roots[x.screen];
        let window = conn.generate_id().unwrap();
        let aux = CreateWindowAux::new().background_pixel(0x3366cc).override_redirect(1);
        conn.create_window(0, window, root.root, 10, 10, 64, 48, 0, WindowClass::INPUT_OUTPUT, 0, &aux).unwrap();
        conn.map_window(window).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
        std::thread::sleep(Duration::from_millis(200));

        let image = settled_rgba(&x, window, (64, 48)).unwrap();
        assert_eq!((image.width, image.height), (64, 48));
        let center = ((24 * 64 + 32) * 4) as usize;
        assert_eq!(&image.pixels[center..center + 4], &[0x33, 0x66, 0xcc, 0xff]);

        conn.destroy_window(window).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
    }
}
