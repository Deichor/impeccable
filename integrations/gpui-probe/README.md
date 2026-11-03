# impeccable-gpui-probe

A debug-build bridge between a GPUI app and Impeccable. It lets the agent capture the real window (both appearances, several sizes) and try style variants live on an element you pick, without recompiling.

Release builds compile it to nothing.

## Setup

```toml
# Cargo.toml of the app
[dependencies]
impeccable-gpui-probe = { git = "https://github.com/new-horizon-code-repo/impeccable", branch = "main" }
```

```rust
// After the app's own setup, inside Application::run
impeccable_gpui_probe::attach(cx, impeccable_gpui_probe::Options {
    // Switch the app's theme tokens; without it captures use the current appearance only.
    on_appearance: Some(Box::new(|dark, _cx| theme::set_dark(dark))),
    ..Default::default()
});
```

The default build uses `gpui` 0.2 from crates.io. An app on Zed's `gpui-pre` snapshots turns that off and picks one of two features:

```toml
# gpui-pre 0.3.11: captures, picking and live variants
impeccable-gpui-probe = { git = "...", branch = "...", default-features = false, features = ["gpui-pre"] }

# the same plus window snapshots; needs the patched GPUI at the workspace root
impeccable-gpui-probe = { git = "...", branch = "...", default-features = false, features = ["snapshot"] }

[patch.crates-io]
gpui-pre = { git = "https://github.com/new-horizon-code-repo/gpui", branch = "main" }
# macOS: the Metal renderer with offscreen capture
gpui-pre-macos = { git = "https://github.com/new-horizon-code-repo/gpui", branch = "main" }
gpui-pre-apple = { git = "https://github.com/new-horizon-code-repo/gpui", branch = "main" }
```

Run a debug build from the project root. The probe serves only this machine's own clients: requests from a browser page (any `Origin` header) or for a foreign `Host` get 403. The probe listens on 127.0.0.1 and writes its port to `.impeccable/gpui-probe.json`, which `impeccable gpui` reads. Add `.impeccable/gpui-probe.json` to `.gitignore`.

## What it does

| | Command | Notes |
|---|---|---|
| Window captures | `impeccable gpui capture --sizes 960x640,1440x900 --appearances dark,light` | One PNG per appearance × size, then restores the window size. Windows and macOS. |
| Pick an element | `ctrl-alt-i` in the app, hover, click; or `impeccable gpui pick --x --y` | The panel shows the element's source line. |
| Live variants | `impeccable gpui variants --file v.json` | Style patches applied to the picked element at once; no rebuild. |
| Decide | `ctrl-alt-←/→` switch, `ctrl-alt-enter` accept, `ctrl-alt-backspace` original | `impeccable gpui decision --wait` returns the accepted patch and source line. |
| Structure variants | `match impeccable_gpui_probe::slot("id", &["Card", "Row"], cx) { .. }` in `render`; `impeccable gpui slots`, `impeccable gpui slot --id id --index 1` | Layout, children and copy alternatives after one rebuild. `ctrl-alt-←/→` switches arms when no style variants are shown, `ctrl-alt-enter` accepts. Release builds take arm 0. |
| State sweep | `impeccable gpui sweep` | `snapshot` feature only. Hovers each hover-reactive element and steps focus through the Tab order, so `.on_hover(..)` and focus code run, and reports what only those states show plus the Tab order. |
| Window snapshot | `impeccable gpui snapshot [--out snap.json]` | `snapshot` feature only. Every painted element and text with resolved colors, sizes and clips, checked by the same rules as the source scan plus `text-overflow`, hover and active contrast, animation curves, and keyboard reach, focus styles, icon-button names and long unvirtualized lists, at the source line. |

A variant is a partial GPUI `StyleRefinement` in JSON, the same shape `impeccable gpui selection` prints for the picked element. `"background": "#rrggbbaa"` is accepted as a solid fill. Invalid patches are rejected before any is shown.

Variants change the picked instance only, and only until the inspector closes; the agent writes the accepted one into the source line.

## Smoke test

`node scripts/smoke.mjs` starts `examples/demo` (build it with `--no-default-features --features snapshot` first) and drives every route on the real window: status, snapshot, slots, sweep, pick, capture, and the browser-origin refusal. With `IMPECCABLE_BIN` set it also checks the engine's `gpui snapshot` and `gpui sweep` findings. CI runs it on Windows.

## Limits

- Window capture: Win32 `PrintWindow` on Windows. On macOS, gpui-pre renders the last frame offscreen through GPUI's Metal renderer, which needs no permission and works for covered windows and a locked screen; gpui 0.2 has no such API, so it captures through ScreenCaptureKit (macOS 14 or later). It asks only for the app's own windows, which macOS 14.4 and later list without consent; if macOS still asks for Screen Recording, grant it to the terminal or editor that launched the app in System Settings > Privacy & Security > Screen & System Audio Recording. ScreenCaptureKit captures nothing while the screen is locked. Linux returns an error for now.
- Point picking uses GPUI's own event dispatch on gpui-pre, where it works on every platform. On gpui 0.2 it posts native mouse events to the app's own queue: Win32 messages on Windows, `NSEvent`s on macOS (no Accessibility permission needed). Linux returns an error for now.
- On macOS a covered window, or every window while the screen is locked, gets no display-link frames. On gpui-pre the probe draws a frame itself for captures, picks and snapshots; on gpui 0.2 picking needs the window on screen.
- Style variants change style only; structure goes through slots, which need one rebuild to add and one to remove.
- The inspector exists in debug builds (or with GPUI's `inspector` feature).
- Snapshot findings sit on the element that holds the text; text inside an element built in a library (gpui-component) points into that library.
