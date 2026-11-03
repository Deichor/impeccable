> **Additional context needed**: target operating systems, window sizes, and input setup (keyboard and mouse, trackpad, touch screen).

Adapt an existing **desktop** design (`desktop`) to a different context: another window size, another OS, a touch-capable laptop, or an origin outside the desktop (a website or a phone app). The trap is treating adaptation as scaling. The job is rethinking the experience for the new context, inside the conventions of [desktop.md](desktop.md) (and [gpui.md](gpui.md) on GPUI); read them before planning if Setup hasn't already.

## Assess Adaptation Challenge

1. **Source context**: what was it designed for, and what assumptions did it make? (One window size? One OS's chrome and shortcuts? A mouse? A website?)
2. **Target context**: which window range (minimum size to ultrawide), which OSes, which input (keyboard and mouse, trackpad gestures, touch), and which posture (focused work in one window vs many windows side by side)?
3. **What breaks**: panes that cannot fit, toolbars that overflow, shortcuts that collide with the OS, hover-only controls on touch?

## Adaptation Strategies

### Narrow ↔ wide windows

- **Restructure, don't stretch.** At narrow widths, collapse the sidebar to icons or an overlay and stack the inspector under the content. At wide widths, add a pane or cap the line length; never stretch one column across an ultrawide window.
- **Toolbars overflow into a menu**, in priority order, instead of wrapping or clipping.
- **Splitters keep user intent.** Remember pane sizes per window size class, and never let a resize hide a pane the user opened.

### OS → OS (macOS ↔ Windows ↔ Linux)

Translate conventions; never transplant them:

| macOS | Windows | Linux (GNOME / KDE) |
|---|---|---|
| Cmd shortcuts | Ctrl shortcuts | Ctrl shortcuts |
| Global menu bar | In-window menu or command bar | Header bar (GNOME) / menu bar (KDE) |
| Traffic lights, left | Caption buttons and Snap Layouts, right | Desktop-dependent, usually right |
| Dialog primary on the right | Primary first, then Cancel | Desktop-dependent |
| Settings: Cmd+, | Settings: Ctrl+, or in a menu | Preferences in the app menu |
| SF Pro, vibrancy | Segoe UI Variable, Mica / Acrylic | The desktop's UI font, flat surfaces |

Rebuild chrome, shortcuts, and dialogs in each OS's vocabulary; carry over the brand's expressive layer (accent, type accent, iconography, motion) through the theme.

### Mouse → touch-capable laptops

Every hover-only control gets a visible or long-press path, targets near frequent actions grow toward 40 px, and scrolling works with touch and pen. Keep the pointer density everywhere else.

### Web or phone → desktop

Reconform, don't reflow. Replace page navigation with panes, add shortcuts and context menus, shrink touch padding to pointer density, move errors next to their cause, and give the window a real minimum size. Then treat the result to the full desktop slop test.

## Implement & Verify

- Drive structure from the **window size**, never from the screen or OS version alone.
- Test the minimum window size, a typical laptop window, and an ultrawide one, at 100% and 200% display scale, in both appearances.
- Confirm each OS's conventions on that OS, and say which one produced the evidence.

When the adaptation feels native to each context, hand off to `{{command_prefix}}impeccable polish` for the final pass.

**Avoid**:
- Stretch a layout across a wide window instead of restructuring it
- Ship one OS's shortcuts, chrome, or button order on another
- Hide core functionality at narrow window sizes (if it matters, make it work)
- Leave hover as the only path to an action on touch-capable devices
- Verify on one OS and report all three as checked
