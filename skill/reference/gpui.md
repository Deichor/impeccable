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

- **Capture the GPUI window itself.** GPUI draws with Metal, DirectX, or Vulkan, so a plain window grab can come back blank: on Windows use `PrintWindow` with `PW_RENDERFULLCONTENT`; on macOS `screencapture -l <window-id>`. Run a debug build when you need the GPUI inspector to find the source line of an element. <!-- rule:gpui-verify-capture -->
