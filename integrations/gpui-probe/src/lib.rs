//! Debug-build bridge between a GPUI app and Impeccable.
//!
//! ```ignore
//! impeccable_gpui_probe::attach(cx, impeccable_gpui_probe::Options {
//!     on_appearance: Some(Box::new(|dark, _cx| theme::set_dark(dark))),
//!     ..Default::default()
//! });
//! ```
//!
//! In a debug build this serves a localhost API (port in
//! `.impeccable/gpui-probe.json`) that `impeccable gpui` talks to:
//! window captures, resizing and appearance switching (Phase 3a), and live
//! style variants on the element picked in GPUI's inspector (Phase 4). In a
//! release build `attach` does nothing.

#[cfg(all(feature = "gpui-0-2", feature = "gpui-pre"))]
compile_error!("pick one GPUI: `gpui-0-2` (default) or `gpui-pre` (with default-features = false)");
#[cfg(not(any(feature = "gpui-0-2", feature = "gpui-pre")))]
compile_error!("enable `gpui-0-2` or `gpui-pre`");

use gpui::App;

#[cfg(debug_assertions)]
mod capture;
#[cfg(debug_assertions)]
mod imp;
#[cfg(debug_assertions)]
mod server;

/// Called with `true` for dark and `false` for light; the app switches its
/// theme tokens. Without it the probe cannot change the appearance.
pub type AppearanceHook = Box<dyn Fn(bool, &mut App)>;

#[derive(Default)]
pub struct Options {
    /// Port to bind on 127.0.0.1; 0 picks a free one.
    pub port: u16,
    pub on_appearance: Option<AppearanceHook>,
}

/// Start the probe. Call once, after the app's own setup.
pub fn attach(cx: &mut App, options: Options) {
    #[cfg(debug_assertions)]
    imp::attach(cx, options);
    #[cfg(not(debug_assertions))]
    let _ = (cx, options);
}
