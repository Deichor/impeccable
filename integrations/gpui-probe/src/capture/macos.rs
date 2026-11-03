//! macOS. ScreenCaptureKit captures the window as it was last presented. It is
//! loaded at run time, so systems without it still launch; capture needs
//! macOS 14. Its callbacks run on its own queue, so the main thread never
//! waits. Pointer input is posted to the app's own event queue as `NSEvent`s,
//! so picking needs no permission.

use std::ffi::{c_char, c_int, c_void};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

use block2::RcBlock;
use gpui::Window;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSView, NSWindow, NSWindowOcclusionState};
use objc2_core_foundation::{CFDictionary, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
    CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSError, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{unpremultiply, Rgba, Shot};

const SCREEN_RECORDING: &str = "window capture needs the Screen Recording permission: grant it to the app that \
     launched this one (your terminal or editor) in System Settings > Privacy & Security > Screen & System Audio \
     Recording, then restart the app";
const LOCKED: &str = "the screen is locked, and macOS captures no window while it is; unlock it and retry";
#[cfg(feature = "gpui-0-2")]
const COVERED: &str = "the window is covered, minimized or on another Space, and macOS does not redraw it there; \
     bring it to the front and retry";
#[cfg(feature = "gpui-pre")]
const COVERED: &str = "the window is covered, minimized or on another Space, and macOS does not redraw it there; \
     bring it to the front, or build with the `snapshot` feature and the fork's gpui-pre-macos and gpui-pre-apple \
     to render it offscreen";
/// `SCStreamErrorUserDeclined`.
const USER_DECLINED: isize = -3801;
const TIMEOUT: Duration = Duration::from_secs(5);

type Answer = mpsc::Sender<Result<Rgba, String>>;

fn gpui_view(window: &Window) -> Result<&NSView, String> {
    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    // The view lives as long as the window.
    Ok(unsafe { h.ns_view.cast::<NSView>().as_ref() })
}

fn ns_window(window: &Window) -> Result<Retained<NSWindow>, String> {
    gpui_view(window)?.window().ok_or_else(|| "the view has no window".into())
}

/// Load ScreenCaptureKit once; `None` when the system has no screenshot API.
fn screenshot_manager() -> Option<&'static AnyClass> {
    static LOADED: OnceLock<bool> = OnceLock::new();
    extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    }
    const RTLD_LAZY: c_int = 1;
    let path = c"/System/Library/Frameworks/ScreenCaptureKit.framework/ScreenCaptureKit";
    if !*LOADED.get_or_init(|| unsafe { !dlopen(path.as_ptr(), RTLD_LAZY).is_null() }) {
        return None;
    }
    AnyClass::get(c"SCScreenshotManager")
}

/// Whether the login session shows the lock screen.
fn screen_locked() -> bool {
    let Some(session) = CGSessionCopyCurrentDictionary() else { return false };
    let dict: &NSDictionary<NSString, AnyObject> = unsafe { &*(&*session as *const CFDictionary).cast() };
    dict.objectForKey(ns_string!("CGSSessionScreenIsLocked")).is_some()
}

pub fn timeout_error() -> String {
    if screen_locked() {
        LOCKED.into()
    } else {
        format!("ScreenCaptureKit did not answer in {} seconds", TIMEOUT.as_secs())
    }
}

fn error_text(error: *mut NSError) -> String {
    match unsafe { error.as_ref() } {
        Some(e) if e.code() == USER_DECLINED => SCREEN_RECORDING.into(),
        _ if screen_locked() => LOCKED.into(),
        Some(e) => format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code()),
        None => "ScreenCaptureKit returned nothing".into(),
    }
}

/// What the capture needs from the window, read on the main thread.
#[derive(Clone, Copy)]
struct Target {
    number: u32,
    /// The content view in window points, top-left origin.
    source: CGRect,
    width: usize,
    height: usize,
}

pub fn start(window: &Window) -> Result<Shot, String> {
    let manager = screenshot_manager().ok_or("window capture without the `snapshot` feature needs macOS 14 or later")?;
    let ns_window = ns_window(window)?;
    if screen_locked() {
        return Err(LOCKED.into());
    }
    if !ns_window.occlusionState().contains(NSWindowOcclusionState::Visible) {
        return Err(COVERED.into());
    }
    let frame = ns_window.frame();
    let content = gpui_view(window)?.frame();
    let scale = ns_window.backingScaleFactor();
    let top = frame.size.height - content.origin.y - content.size.height;
    let target = Target {
        number: ns_window.windowNumber() as u32,
        source: CGRect::new(CGPoint::new(content.origin.x, top), content.size),
        width: (content.size.width * scale).round() as usize,
        height: (content.size.height * scale).round() as usize,
    };
    if target.width == 0 || target.height == 0 {
        return Err("the window has no size to capture".into());
    }

    let content_class = AnyClass::get(c"SCShareableContent").ok_or("ScreenCaptureKit is missing")?;
    let (tx, rx) = mpsc::channel();
    let on_content = RcBlock::new(move |content: *mut AnyObject, error: *mut NSError| {
        let result = match unsafe { content.as_ref() } {
            Some(content) => screenshot(manager, content, target, tx.clone()),
            None => Err(error_text(error)),
        };
        if let Err(e) = result {
            let _ = tx.send(Err(e));
        }
    });
    // The app's own windows; macOS 14.4+ lists them without asking the user.
    let own = sel!(getCurrentProcessShareableContentWithCompletionHandler:);
    unsafe {
        if content_class.metaclass().responds_to(own) {
            let _: () = msg_send![content_class, getCurrentProcessShareableContentWithCompletionHandler: &*on_content];
        } else if CGPreflightScreenCaptureAccess() {
            let _: () = msg_send![
                content_class,
                getShareableContentExcludingDesktopWindows: false,
                onScreenWindowsOnly: false,
                completionHandler: &*on_content
            ];
        } else {
            CGRequestScreenCaptureAccess();
            return Err(SCREEN_RECORDING.into());
        }
    }
    Ok(Shot::Pending(rx))
}

/// Runs on ScreenCaptureKit's queue.
fn screenshot(manager: &AnyClass, content: &AnyObject, target: Target, answer: Answer) -> Result<(), String> {
    unsafe {
        let windows: Retained<NSArray<AnyObject>> = msg_send![content, windows];
        let window = windows
            .iter()
            .find(|w| {
                let id: u32 = msg_send![&**w, windowID];
                id == target.number
            })
            .ok_or("ScreenCaptureKit does not list this window")?;
        let filter_class = AnyClass::get(c"SCContentFilter").ok_or("ScreenCaptureKit is missing")?;
        let config_class = AnyClass::get(c"SCStreamConfiguration").ok_or("ScreenCaptureKit is missing")?;
        let filter: Allocated<AnyObject> = msg_send![filter_class, alloc];
        let filter: Retained<AnyObject> = msg_send![filter, initWithDesktopIndependentWindow: &*window];
        let config: Retained<AnyObject> = msg_send![config_class, new];
        let _: () = msg_send![&config, setSourceRect: target.source];
        let _: () = msg_send![&config, setWidth: target.width];
        let _: () = msg_send![&config, setHeight: target.height];
        let _: () = msg_send![&config, setShowsCursor: false];
        let _: () = msg_send![&config, setIgnoreShadowsSingleWindow: true];
        let on_image = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            let result = match image.as_ref() {
                Some(image) => to_rgba(image),
                None => Err(error_text(error)),
            };
            let _ = answer.send(result);
        });
        let _: () = msg_send![
            manager,
            captureImageWithFilter: &*filter,
            configuration: &*config,
            completionHandler: &*on_image
        ];
    }
    Ok(())
}

/// Draw the image into an sRGB bitmap we own, then un-premultiply.
fn to_rgba(image: &CGImage) -> Result<Rgba, String> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let mut pixels = vec![0u8; width * height * 4];
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or("no sRGB color space")?;
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    let context = unsafe {
        CGBitmapContextCreate(pixels.as_mut_ptr().cast::<c_void>(), width, height, 8, width * 4, Some(&space), info)
    }
    .ok_or("could not create a bitmap context")?;
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    drop(context);
    unpremultiply(&mut pixels);
    Ok(Rgba { width: width as u32, height: height as u32, pixels })
}

#[cfg(feature = "gpui-0-2")]
/// Post a pointer move (and, with `click`, a left click) at a point in
/// logical pixels to the app's own event queue, as AppKit would deliver it.
pub fn post_pointer(window: &Window, x: f32, y: f32, click: bool) -> Result<(), String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::{NSPoint, NSProcessInfo};

    let mtm = MainThreadMarker::new().ok_or("pointer events must be posted on the main thread")?;
    let ns_window = ns_window(window)?;
    let content = ns_window.contentView().ok_or("the window has no content view")?.frame();
    // GPUI reads y from the bottom of the content view.
    let location = NSPoint::new(x as f64, content.size.height - y as f64);
    let number = ns_window.windowNumber();
    let time = NSProcessInfo::processInfo().systemUptime();
    let app = NSApplication::sharedApplication(mtm);
    let post = |kind: NSEventType, clicks: isize, pressure: f32| -> Result<(), String> {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            location,
            NSEventModifierFlags::empty(),
            time,
            number,
            None,
            0,
            clicks,
            pressure,
        )
        .ok_or("AppKit refused the mouse event")?;
        app.postEvent_atStart(&event, false);
        Ok(())
    };
    post(NSEventType::MouseMoved, 0, 0.0)?;
    if click {
        post(NSEventType::LeftMouseDown, 1, 1.0)?;
        post(NSEventType::LeftMouseUp, 1, 0.0)?;
    }
    Ok(())
}

