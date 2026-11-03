# GPUI toolkit

For desktop apps built with GPUI (Zed's UI framework), including apps on gpui-component. Loads with [desktop.md](desktop.md); the platform conventions there still decide structure, this file says how to express them in GPUI.

## Where design lives in GPUI code

- **Styles are builder methods.** `div().flex().gap_2().px_3().rounded_md().text_sm()` reads like Tailwind: one spacing step is 4 px (`p_2` = 8 px), a rem is 16 px. Keep the steps; reach for `px(..)` only for values the scale cannot express. <!-- rule:gpui-style-scale -->
- **Colors and fonts come from a theme, never from the view.** Put tokens in one place (a `theme` module of zero-argument functions and constants, or a `Global` read through `cx`), and call them from views: `.bg(theme::surface())`, not `.bg(rgb(0x161618))`. An inline `rgb(..)` in a view is a token that escaped. <!-- rule:gpui-theme-tokens -->
- **Appearance switches the tokens, not the views.** Read `window.appearance()` and re-resolve on `cx.observe_window_appearance(..)`; token functions return the light or dark value. Views never branch on dark mode themselves. <!-- rule:gpui-appearance -->
- **Set type once at the root.** Font family, base size, and line height go on the window's root element and inherit; children change only what differs. Dense lists set an explicit `line_height`. <!-- rule:gpui-root-type -->

## Keyboard, focus, and commands

- **Commands are actions.** Declare them with `actions!`, bind them with `KeyBinding::new(..)` under a `key_context(..)`, and handle them with `.on_action(..)`. That makes every command rebindable, menu-able (`cx.set_menus(..)`), and listable in a palette. A click handler alone is a command only the mouse can run. <!-- rule:gpui-actions-keybindings -->
- **Focusable views track a handle.** Interactive regions own a `FocusHandle` and call `.track_focus(&handle)`; give them a visible `.focus(|s| ..)` style. Restore focus to the opener when a popover or modal closes. <!-- rule:gpui-focus -->
- **Icon-only buttons get tooltips.** Every icon-only control sets `.tooltip(..)` with its name and shortcut. <!-- rule:gpui-icon-tooltips -->
- **Stateful interaction needs an id.** `.hover(..)`, `.active(..)`, `.on_click(..)`, and tooltips need `.id(..)` on the element; stable ids keep that state across renders. <!-- rule:gpui-element-ids -->

## Layout and performance

- **Long content is virtualized.** Use `uniform_list` (same-height rows) or `list` for anything that can grow past a screen; a `children(..)` over thousands of items lays all of them out every frame. <!-- rule:gpui-virtualized-lists -->
- **Panes resize, text truncates.** Give flexible children `.flex_1().min_w_0()` so long text truncates or wraps instead of pushing the pane wider; `.overflow_hidden()` on the pane, `.text_ellipsis()` on single-line labels. <!-- rule:gpui-flex-overflow -->
- **Custom title bars keep window behavior.** With `TitlebarOptions { appears_transparent: true, .. }`, leave the drag area free (`window_control_area` on Windows) and position content around `traffic_light_position` on macOS. <!-- rule:gpui-titlebar -->

## Motion

- **Use the shipped easings.** `.with_animation(id, Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()), |el, t| ..)`. Skip `bounce(..)`; reserve `pulsating_between(..)` for indicators tied to live state. <!-- rule:gpui-motion -->

## What the detector reads

The design hook and `impeccable detect` read GPUI builder chains in `.rs` files and resolve theme functions and constants across the Cargo workspace, both branches of an `if is_dark() { .. } else { .. }` included. They report `overused-font`, `side-tab`, `border-accent-on-rounded`, `low-contrast`, `gray-on-color`, `tiny-text`, `dark-glow`, `ai-color-palette`, `bounce-easing`, `pulsing-dot`, `nested-cards`, and `gpt-thin-border-wide-shadow`. Styles inside closures (`.when(..)`, `.hover(..)`) are conditional and not read, and values that only exist at runtime stay unknown. An empty result is not proof of a clean window: check contrast, overflow, and density on a screenshot.

## Verifying the build

- **Capture the GPUI window itself.** GPUI draws with Metal, DirectX, or Vulkan, so a plain window grab can come back blank. When the app runs a debug build with `impeccable-gpui-probe` attached (`{{scripts_path}}/impeccable gpui status` answers), capture the set in one call: `{{scripts_path}}/impeccable gpui capture --out .impeccable/review --sizes <min>,<large> --appearances dark,light`. Without the probe: on Windows `PrintWindow` with `PW_RENDERFULLCONTENT`, on macOS `screencapture -l <window-id>`. <!-- rule:gpui-verify-capture -->

## Live variants

With the probe attached, `live` and `generate` run on the window instead of a browser:

1. Ask the user to press `ctrl-alt-i` in the app and click the element (or run `{{scripts_path}}/impeccable gpui pick --x <x> --y <y>` from a capture's coordinates, in logical pixels). `{{scripts_path}}/impeccable gpui selection --wait` returns its source line, style, and bounds.
2. Write 2–4 distinct directions as style patches in the JSON shape the selection printed (`"background": "#rrggbbaa"` is accepted as a solid fill), label each in a few words, and send them: `{{scripts_path}}/impeccable gpui variants --file <json>`. They show at once; nothing rebuilds.
3. The user switches with `ctrl-alt-←/→` and accepts with `ctrl-alt-enter`. `{{scripts_path}}/impeccable gpui decision --wait` returns the accepted patch.
4. Write the accepted patch into the builder chain at the source line, in the app's own vocabulary: scale methods (`.p_3()`), theme tokens for colors, not raw values. Rebuild and capture to confirm.

Patches cover style only. A variant that changes structure, children, or copy is a code edit: build the alternatives behind one `match` on a debug-only index, rebuild once, and let the user compare. <!-- rule:gpui-live-variants -->

## Window snapshot

When the probe is built with its `snapshot` feature (gpui-pre with the new-horizon-code-repo/gpui patch), `{{scripts_path}}/impeccable gpui snapshot` returns what the window actually painted and checks it: `low-contrast` against the composited fill behind each text, `tiny-text`, and `text-overflow` for text cut off by its clip, each at the source line of the element holding the text. Run it in both appearances (`POST /appearance` through `capture --appearances`, or the app's own switch) and at the smallest supported size; it sees resolved theme branches and `.when(..)` styles that the source scan cannot. A probe without the feature answers 501; fall back to captures. <!-- rule:gpui-snapshot -->
