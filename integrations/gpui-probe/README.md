# impeccable-gpui-probe

A debug-build bridge between a GPUI app and Impeccable. It lets the agent capture the real window (both appearances, several sizes) and try style variants live on an element you pick, without recompiling.

Release builds compile it to nothing.

## Setup

```toml
# Cargo.toml of the app
[dependencies]
impeccable-gpui-probe = { git = "https://github.com/new-horizon-code-repo/impeccable", branch = "feat/desktop-gpui-platform" }
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
```

Run a debug build from the project root. The probe listens on 127.0.0.1 and writes its port to `.impeccable/gpui-probe.json`, which `impeccable gpui` reads. Add `.impeccable/gpui-probe.json` to `.gitignore`.

## What it does

| | Command | Notes |
|---|---|---|
| Window captures | `impeccable gpui capture --sizes 960x640,1440x900 --appearances dark,light` | One PNG per appearance × size, then restores the window size. Windows only for now. |
| Pick an element | `ctrl-alt-i` in the app, hover, click; or `impeccable gpui pick --x --y` | The panel shows the element's source line. |
| Live variants | `impeccable gpui variants --file v.json` | Style patches applied to the picked element at once; no rebuild. |
| Decide | `ctrl-alt-←/→` switch, `ctrl-alt-enter` accept, `ctrl-alt-backspace` original | `impeccable gpui decision --wait` returns the accepted patch and source line. |
| Structure variants | `match impeccable_gpui_probe::slot("id", &["Card", "Row"], cx) { .. }` in `render`; `impeccable gpui slots`, `impeccable gpui slot --id id --index 1` | Layout, children and copy alternatives after one rebuild. `ctrl-alt-←/→` switches arms when no style variants are shown, `ctrl-alt-enter` accepts. Release builds take arm 0. |
| Window snapshot | `impeccable gpui snapshot [--out snap.json]` | `snapshot` feature only. Every painted element and text with resolved colors, sizes and clips, checked by the same rules as the source scan plus `text-overflow`, at the source line. |

A variant is a partial GPUI `StyleRefinement` in JSON, the same shape `impeccable gpui selection` prints for the picked element. `"background": "#rrggbbaa"` is accepted as a solid fill. Invalid patches are rejected before any is shown.

Variants change the picked instance only, and only until the inspector closes; the agent writes the accepted one into the source line.

## Limits

- Window capture and point picking use Win32 (`PrintWindow`, posted mouse messages). macOS and Linux return an error for those two calls; picking in the window works everywhere.
- Style variants change style only; structure goes through slots, which need one rebuild to add and one to remove.
- The inspector exists in debug builds (or with GPUI's `inspector` feature).
- Snapshot findings sit on the element that holds the text; text inside an element built in a library (gpui-component) points into that library.
