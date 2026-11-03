//! Windows: `PrintWindow` with full-content rendering asks the compositor for
//! the window itself, and pointer input goes to the window's message queue.

use gpui::Window;

use super::Rgba;

pub fn window_rgba(window: &Window) -> Result<Rgba, String> {
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
#[cfg(feature = "gpui-0-2")]
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
