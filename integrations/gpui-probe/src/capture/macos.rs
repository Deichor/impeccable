//! macOS on gpui 0.2, which has no offscreen render. ScreenCaptureKit
//! captures the window alone, covered or not. Pointer input is posted to the
//! app's own event queue as `NSEvent`s, so picking needs no permission.

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use gpui::Window;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{sel, AllocAnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSView, NSWindow};
use objc2_core_foundation::{CFDictionary, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
    CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{ns_string, NSDictionary, NSError, NSPoint, NSProcessInfo, NSString};
use objc2::runtime::AnyObject;
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration, SCWindow,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::Rgba;

const SCREEN_RECORDING: &str = "window capture needs the Screen Recording permission: grant it to the app that \
     launched this one (your terminal or editor) in System Settings > Privacy & Security > Screen & System Audio \
     Recording, then restart the app";
const LOCKED: &str = "the screen is locked, and macOS captures no window while it is; unlock it and retry";
const TIMEOUT: Duration = Duration::from_secs(5);
/// `SCStreamErrorUserDeclined`.
const USER_DECLINED: isize = -3801;

fn ns_window(window: &Window) -> Result<Retained<NSWindow>, String> {
    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    view.window().ok_or_else(|| "the view has no window".into())
}

/// Lets a retained object cross from ScreenCaptureKit's callback queue.
struct Sent<T>(Retained<T>);
unsafe impl<T> Send for Sent<T> {}

type Reply<T> = Result<Sent<T>, String>;

fn error_text(error: *mut NSError) -> String {
    match unsafe { error.as_ref() } {
        Some(e) if e.code() == USER_DECLINED => SCREEN_RECORDING.into(),
        Some(_) if screen_locked() => LOCKED.into(),
        Some(e) => format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code()),
        None => "no result".into(),
    }
}

/// Whether the login session shows the lock screen.
fn screen_locked() -> bool {
    let Some(session) = CGSessionCopyCurrentDictionary() else { return false };
    let dict: &NSDictionary<NSString, AnyObject> = unsafe { &*(&*session as *const CFDictionary).cast() };
    dict.objectForKey(ns_string!("CGSSessionScreenIsLocked")).is_some()
}

fn reply<T>(object: *mut T, error: *mut NSError) -> Reply<T>
where
    T: objc2::Message,
{
    match unsafe { Retained::retain(object) } {
        Some(o) => Ok(Sent(o)),
        None => Err(error_text(error)),
    }
}

/// The app's own windows. macOS 14.4+ answers that without asking the user;
/// older systems need the Screen Recording permission.
fn own_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel::<Reply<SCShareableContent>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let _ = tx.send(reply(content, error));
    });
    let own = sel!(getCurrentProcessShareableContentWithCompletionHandler:);
    unsafe {
        if SCShareableContent::class().metaclass().responds_to(own) {
            SCShareableContent::getCurrentProcessShareableContentWithCompletionHandler(&block);
        } else {
            if !CGPreflightScreenCaptureAccess() {
                CGRequestScreenCaptureAccess();
                return Err(SCREEN_RECORDING.into());
            }
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                false, false, &block,
            );
        }
    }
    let content = rx.recv_timeout(TIMEOUT).map_err(|_| "ScreenCaptureKit did not answer".to_string())??;
    Ok(content.0)
}

pub fn window_rgba(window: &Window) -> Result<Rgba, String> {
    if AnyClass::get(c"SCScreenshotManager").is_none() {
        return Err("window capture on gpui 0.2 needs macOS 14 or later".into());
    }
    let ns_window = ns_window(window)?;
    let number = ns_window.windowNumber() as u32;
    let frame = ns_window.frame();
    let content_view = ns_window.contentView().ok_or("the window has no content view")?;
    let content = content_view.frame();
    let scale = ns_window.backingScaleFactor();

    let shareable = own_content()?;
    let windows = unsafe { shareable.windows() };
    let target: Retained<SCWindow> = windows
        .iter()
        .find(|w| unsafe { w.windowID() } == number)
        .ok_or("ScreenCaptureKit does not list this window (is it minimized?)")?;

    let (width, height) = ((content.size.width * scale).round() as usize, (content.size.height * scale).round() as usize);
    if width == 0 || height == 0 {
        return Err("the window has no size to capture".into());
    }
    let (tx, rx) = mpsc::channel::<Reply<CGImage>>();
    let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let _ = tx.send(reply(image, error));
    });
    unsafe {
        let filter = SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &target);
        let config = SCStreamConfiguration::new();
        // The content view only, in window points with a top-left origin.
        let top = frame.size.height - content.origin.y - content.size.height;
        config.setSourceRect(CGRect::new(CGPoint::new(content.origin.x, top), CGSize::new(content.size.width, content.size.height)));
        config.setWidth(width);
        config.setHeight(height);
        config.setShowsCursor(false);
        config.setIgnoreShadowsSingleWindow(true);
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &config, Some(&block));
    }
    let image = rx.recv_timeout(TIMEOUT).map_err(|_| "ScreenCaptureKit did not answer".to_string())??;
    to_rgba(&image.0)
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

/// Post a pointer move (and, with `click`, a left click) at a point in
/// logical pixels to the app's own event queue, as AppKit would deliver it.
pub fn post_pointer(window: &Window, x: f32, y: f32, click: bool) -> Result<(), String> {
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
