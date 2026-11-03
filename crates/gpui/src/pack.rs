//! Desktop rules the web registry has no row for, registered as a rule pack
//! under `gpui/` ids so the built-in registry and `--list-rules` stay as they
//! are. Every GPUI entry point calls [`register`] before it builds findings.

use impeccable_core::registry::{extend, Antipattern};

pub const UNREACHABLE_CONTROL: &str = "gpui/unreachable-control";
pub const NO_FOCUS_STYLE: &str = "gpui/no-focus-style";
pub const UNNAMED_ICON_BUTTON: &str = "gpui/unnamed-icon-button";
pub const UNVIRTUALIZED_LIST: &str = "gpui/unvirtualized-list";

static ROWS: [Antipattern; 4] = [
    Antipattern {
        id: UNREACHABLE_CONTROL,
        category: "quality",
        scopes: Some(&["interaction"]),
        severity: None,
        name: "Control the keyboard cannot reach",
        description: "The element handles clicks but tracks no focus handle, and no focusable ancestor handles its actions, so keyboard users cannot use it. Give it a FocusHandle with track_focus, or route the command through an action on a focused container.",
        skill_section: Some("Keyboard, focus, and commands"),
        skill_guideline: Some("focusable views track a handle"),
    },
    Antipattern {
        id: NO_FOCUS_STYLE,
        category: "quality",
        scopes: Some(&["interaction"]),
        severity: None,
        name: "Focus is invisible",
        description: "A focusable control sets no focus or focus-visible style, so a keyboard user cannot see where focus is. Add .focus(..) or .focus_visible(..) with a visible ring or fill.",
        skill_section: Some("Keyboard, focus, and commands"),
        skill_guideline: Some("visible focus style"),
    },
    Antipattern {
        id: UNNAMED_ICON_BUTTON,
        category: "quality",
        scopes: Some(&["interaction"]),
        severity: None,
        name: "Icon-only button without a name",
        description: "A clickable element shows only an icon and has no tooltip or accessible label, so its purpose is a guess. Add .tooltip(..) with the command name and shortcut.",
        skill_section: Some("Keyboard, focus, and commands"),
        skill_guideline: Some("icon-only buttons get tooltips"),
    },
    Antipattern {
        id: UNVIRTUALIZED_LIST,
        category: "quality",
        scopes: Some(&["layout"]),
        severity: Some("advisory"),
        name: "Long list laid out in full",
        description: "A scrolling container lays out every row each frame. Use uniform_list for same-height rows or list for variable ones, so only the visible rows render.",
        skill_section: Some("Layout and performance"),
        skill_guideline: Some("long content is virtualized"),
    },
];

/// Make the `gpui/` ids resolve. Idempotent.
pub fn register() {
    extend(&ROWS);
}
