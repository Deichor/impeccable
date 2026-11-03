# Desktop platform

For native desktop apps on macOS, Windows, and Linux: GPUI, Electron, Tauri, Qt, WinUI, AppKit, GTK. When the toolkit is GPUI, [gpui.md](gpui.md) loads alongside this file.

On desktop, the visitor mode narrows what expression may override. Window, input, and menu conventions govern structure in every mode; brand expresses through the layer the OS leaves open (accent, type, iconography, motion, content).

## The desktop slop test

Would someone who lives in desktop software all day trust this app, or feel they are using a website in a frame? The tells: phone-sized padding on a 27-inch screen, actions that only a mouse can reach, no shortcuts, no context menus, a hamburger menu hiding the commands, every error in a toast, one centered column wasting a wide window, custom window chrome that breaks dragging and snapping. Default to what the OS already taught the user; depart only for a reason they would thank you for.

## Window & layout

- **Resizable all the way.** Layouts hold from the minimum window size to an ultrawide monitor. Set a real minimum size; below a width, panes collapse or stack instead of shrinking their text. <!-- rule:desktop-layout-resizable -->
- **Panes, not pages.** Sidebar, content, and inspector side by side, with draggable splitters whose sizes persist between launches. Navigation keeps context visible instead of replacing the whole window. <!-- rule:desktop-layout-panes -->
- **Window chrome keeps its jobs.** A custom title bar still drags the window, double-clicks to maximize (zoom on macOS), and leaves room for the traffic lights on the left (macOS) or the caption buttons on the right (Windows), including Snap Layouts on the maximize button. <!-- rule:desktop-layout-window-chrome -->
- **Pointer density.** A precise pointer allows compact rows (24–32 px) and tight toolbars. Touch-sized padding is a phone habit; use it only when touch screens are a stated target. <!-- rule:desktop-layout-density -->

## Input

- **Keyboard first.** Every action is reachable from the keyboard. Shortcuts follow the OS: Cmd on macOS, Ctrl on Windows and Linux; Cmd/Ctrl+, opens settings, Cmd/Ctrl+W closes the window or tab, Esc dismisses, Cmd+Q quits on macOS. Menus and tooltips show the shortcut next to the command. <!-- rule:desktop-input-keyboard-first -->
- **Visible focus.** Keyboard focus draws a clear ring; Tab order follows reading order; closing a dialog or popover returns focus to what opened it. <!-- rule:desktop-input-focus -->
- **Hover informs, never gates.** Hover states and delayed tooltips are expected on desktop. An action that appears only on hover also needs a keyboard path and a context-menu entry. <!-- rule:desktop-input-hover -->
- **Right-click works.** Objects (rows, files, tabs, selections) have context menus with their actions, mirrored in the menu bar or command surface. <!-- rule:desktop-input-context-menu -->
- **Selection behaves like the OS.** Click selects, Cmd/Ctrl-click toggles, Shift-click extends, arrow keys move, Cmd/Ctrl+A selects all. Drag and drop works where objects move. <!-- rule:desktop-input-selection -->

## Menus & commands

- **macOS has a menu bar.** App, File, Edit, View, Window, Help, with the standard items in their usual places. Windows and Linux apps may use an in-window menu or a command surface, but the standard shortcuts still work. <!-- rule:desktop-menus-standard -->
- **A command palette for wide apps.** Apps with many actions get a searchable palette (Cmd/Ctrl+Shift+P or Cmd/Ctrl+K). It adds discovery; it does not replace menus or shortcuts. <!-- rule:desktop-menus-palette -->
- **Edit commands everywhere.** Undo, redo, cut, copy, paste, and select all work in every text field and on selections. <!-- rule:desktop-menus-edit-commands -->

## Typography

- **System UI faces or one deliberate brand face.** SF Pro on macOS, Segoe UI Variable on Windows, the desktop's UI font on Linux. A brand face carries display moments; a generic default carries nothing. <!-- rule:desktop-typo-faces -->
- **Desktop scale.** Body text sits at 13 px (macOS) to 14 px (Windows); secondary text never goes under 11 px. Respect the OS text-size and display-scale settings. <!-- rule:desktop-typo-scale -->

## Color & appearance

- **Follow the OS appearance.** Light and dark track the system setting by default, with an in-app override. Both appearances are designed, not inverted. <!-- rule:desktop-color-appearance -->
- **One accent, used for state.** Selection, focus, primary actions, and links carry the accent (the system accent or one brand color); large surfaces don't. <!-- rule:desktop-color-accent -->
- **Quiet text still reads.** Meta text, placeholders, and disabled labels keep 4.5:1 against their surface in both appearances; muted is a lightness step, not a fade to nothing. <!-- rule:desktop-color-quiet-text -->
- **Materials come from the platform.** Vibrancy (macOS), Mica and Acrylic (Windows) come from the OS APIs; hand-rolled blur reads as a web port. <!-- rule:desktop-color-materials -->

## Components & feedback

- **Familiar controls.** Checkboxes, radio groups, popup buttons, segmented controls, steppers, sliders, sortable tables, and tree views behave like the OS versions, including their keyboard handling. <!-- rule:desktop-components-familiar -->
- **Modal only to block.** Dialogs are for decisions the app cannot continue without. Button order follows the OS (macOS: primary on the right; Windows: primary first, then Cancel); Enter confirms the default, Esc cancels. <!-- rule:desktop-components-dialogs -->
- **Errors stay near their cause.** Validation errors sit beside the field, failed operations beside the object they failed on. Toasts are for confirmations that need no action; system notifications are for background events while the window is not focused. <!-- rule:desktop-components-feedback -->

## Motion

- **Short and functional.** 100–200 ms transitions for panes, popovers, and state changes; no page-transition theater. Honor Reduce Motion (macOS) and "Animation effects" off (Windows). <!-- rule:desktop-motion-functional -->

## Verifying the build

- **Screenshots come from the app window, never a browser.** macOS: `screencapture -l <window-id> <path>` (or `-w` to pick the window). Windows: capture the window itself (PrintWindow with full-content rendering for GPU-drawn windows). Linux: the compositor's window capture. Capture the minimum window size and a large one, in both appearances. <!-- rule:desktop-verify-window-capture -->
- **Do a keyboard pass.** Walk the main flow with the keyboard only: focus visible, nothing unreachable, shortcuts as listed. <!-- rule:desktop-verify-keyboard-pass -->
- **Check the scale factors.** Look at 100% and 200% display scale; blurry icons, 1 px lines that vanish, and clipped text show up there. <!-- rule:desktop-verify-scale -->
- **Say which OS produced the evidence.** Per-OS conventions (menu bar, chrome, button order) can only be confirmed on that OS. <!-- rule:desktop-verify-os-honesty -->
