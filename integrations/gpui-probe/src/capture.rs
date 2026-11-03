//! Window capture. GPUI draws through the GPU, so a plain screen grab of a
//! covered or off-screen window comes back blank; on Windows `PrintWindow`
//! with full-content rendering asks the compositor for the window itself.

use gpui::Window;

pub fn window_png(window: &Window) -> Result<Vec<u8>, String> {
    let rgba = window_rgba(window)?;
    encode_png(&rgba.pixels, rgba.width, rgba.height)
}

struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
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

#[cfg(target_os = "windows")]
fn window_rgba(window: &Window) -> Result<Rgba, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    const PW_CLIENTONLY: u32 = 1;
    const PW_RENDERFULLCONTENT: u32 = 2;

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        let (w, h) = ((rect.right - rect.left).max(1), (rect.bottom - rect.top).max(1));
        let screen = GetDC(Some(hwnd));
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_CLIENTONLY | PW_RENDERFULLCONTENT)).as_bool();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(Some(hwnd), screen);
        if !ok || lines == 0 {
            return Err("PrintWindow failed".into());
        }
        // BGRA -> RGBA, opaque.
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        Ok(Rgba { width: w as u32, height: h as u32, pixels: buf })
    }
}

/// Post a pointer move (and, with `click`, a left click) at a point in
/// logical pixels to the window's own message queue.
#[cfg(all(target_os = "windows", feature = "gpui-0-2"))]
pub fn post_pointer(window: &Window, x: f32, y: f32, click: bool) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};

    const MK_LBUTTON: usize = 1;
    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
    let scale = window.scale_factor();
    let (px, py) = ((x * scale).round() as i32, (y * scale).round() as i32);
    let lparam = LPARAM(((py as u32 as isize) << 16) | (px as u32 as isize & 0xffff));
    unsafe {
        let post = |msg, wparam| PostMessageW(Some(hwnd), msg, WPARAM(wparam), lparam).map_err(|e| e.to_string());
        post(WM_MOUSEMOVE, 0)?;
        if click {
            post(WM_LBUTTONDOWN, MK_LBUTTON)?;
            post(WM_LBUTTONUP, 0)?;
        }
    }
    Ok(())
}

#[cfg(all(not(target_os = "windows"), feature = "gpui-0-2"))]
pub fn post_pointer(_window: &Window, _x: f32, _y: f32, _click: bool) -> Result<(), String> {
    Err("picking by point is implemented for Windows only so far; pick in the window instead".into())
}

#[cfg(not(target_os = "windows"))]
fn window_rgba(_window: &Window) -> Result<Rgba, String> {
    Err("window capture is implemented for Windows only so far; on macOS use `screencapture -l <window-id>`".into())
}
