//! The GPUI source engine: design rules over Rust UI code written with
//! GPUI's builder API (`div().p_2().bg(..)`).
//!
//! It is a static reader. It follows builder chains and evaluates literal
//! colors and lengths; theme lookups and variables are unknown, and a rule
//! that would need one stays quiet. Rendered checks (what the window
//! actually paints) are out of reach here.

mod lex;
mod model;
mod pack;
mod rules;
mod snapshot;
mod tokens;
mod value;

pub use pack::register as register_rules;
pub use snapshot::{detect_snapshot, detect_sweep};
pub use tokens::Tokens;

use impeccable_core::findings::{try_finding, Finding};
use impeccable_core::inline_ignores::apply_inline_ignores;
use serde_json::Value;

/// Whether a `.rs` file is GPUI UI code worth scanning.
pub fn is_gpui_source(content: &str) -> bool {
    content.contains("gpui") || content.contains("IntoElement") || content.contains("RenderOnce")
}

/// Whether a path is a Rust source file the GPUI engine may read. Build
/// output under `target/` never is.
pub fn is_rust_path(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/").to_ascii_lowercase();
    p.ends_with(".rs") && !p.contains("/target/") && !p.starts_with("target/")
}

/// Scan one file, resolving theme tokens from the Cargo workspace it is in.
pub fn detect_gpui(content: &str, file_path: &str, inline_ignores: bool) -> Vec<Finding> {
    if !is_gpui_source(content) {
        return Vec::new();
    }
    detect_gpui_with(content, file_path, inline_ignores, &tokens::for_file(file_path))
}

pub fn detect_gpui_with(content: &str, file_path: &str, inline_ignores: bool, tokens: &Tokens) -> Vec<Finding> {
    if !is_gpui_source(content) {
        return Vec::new();
    }
    pack::register();
    let lexed = lex::lex(content);
    let chains = model::chains(&lexed.tokens, &lexed.code);
    let mut seen = std::collections::HashSet::new();
    let mut findings: Vec<Finding> = rules::run(&lexed, &chains, tokens)
        .into_iter()
        .filter(|h| seen.insert((h.id, h.line, h.snippet.clone())))
        .filter_map(|h| {
            let mut f = try_finding(h.id, file_path, &h.snippet, h.line as f64)?;
            if let Some(v) = h.value {
                f.extras.insert("ignoreValue".into(), Value::String(v));
            }
            Some(f)
        })
        .collect();
    findings.sort_by(|a, b| a.line.partial_cmp(&b.line).unwrap_or(std::cmp::Ordering::Equal));
    if inline_ignores {
        findings = apply_inline_ignores(findings, Some(content));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(src: &str) -> Vec<String> {
        detect_gpui_with(src, "view.rs", true, &Tokens::default()).into_iter().map(|f| f.antipattern).collect()
    }

    #[test]
    fn ignores_non_gpui_rust() {
        assert!(ids("fn main() { println!(\"font_family(\\\"Inter\\\")\"); }").is_empty());
    }

    #[test]
    fn overused_font() {
        let f = detect_gpui_with("use gpui::*;\nfn r() { div().font_family(\"Inter\") }", "a.rs", true, &Tokens::default());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].antipattern, "overused-font");
        assert_eq!(f[0].line, 2.0);
        assert!(ids("use gpui::*; fn r() { div().font_family(\"Berkeley Mono\") }").is_empty());
    }

    #[test]
    fn side_tab_needs_accent_color() {
        let accent = "use gpui::*; fn r() { div().p_4().bg(white()).rounded_lg().border_l_4().border_color(rgb(0x7c3aed)) }";
        assert_eq!(ids(accent), vec!["side-tab"]);
        let neutral = "use gpui::*; fn r() { div().rounded_lg().border_l_4().border_color(rgb(0xdddddd)) }";
        assert!(ids(neutral).is_empty());
        let themed = "use gpui::*; fn r() { div().rounded_lg().border_l_4().border_color(cx.theme().colors().border) }";
        assert!(ids(themed).is_empty());
        let conditional = "use gpui::*; fn r() { div().rounded_lg().when(sel, |d| d.border_l_4().border_color(rgb(0x7c3aed))) }";
        assert!(ids(conditional).is_empty());
    }

    #[test]
    fn low_contrast_uses_ancestor_bg() {
        let src = "use gpui::*; fn r() { div().bg(rgb(0xffffff)).child(div().text_color(rgb(0xcccccc)).child(\"hi\")) }";
        assert_eq!(ids(src), vec!["low-contrast"]);
        let ok = "use gpui::*; fn r() { div().bg(rgb(0xffffff)).child(div().text_color(rgb(0x333333))) }";
        assert!(ids(ok).is_empty());
        let unknown = "use gpui::*; fn r() { div().bg(theme.bg).child(div().text_color(rgb(0xcccccc))) }";
        assert!(ids(unknown).is_empty());
    }

    #[test]
    fn glow_gradient_bounce_tiny_pulse() {
        let glow = "use gpui::*; fn r() { div().shadow(vec![BoxShadow { color: hsla(0.75, 0.9, 0.6, 0.6), offset: point(px(0.), px(0.)), blur_radius: px(24.), spread_radius: px(0.) }]) }";
        assert_eq!(ids(glow), vec!["dark-glow"]);
        let grad = "use gpui::*; fn r() { div().bg(linear_gradient(90., linear_color_stop(rgb(0x7c3aed), 0.), linear_color_stop(rgb(0x2563eb), 1.))) }";
        assert_eq!(ids(grad), vec!["ai-color-palette"]);
        let easing = "use gpui::*; fn r() { Animation::new(d).with_easing(bounce(ease_in_out)) }";
        assert_eq!(ids(easing), vec!["bounce-easing"]);
        assert_eq!(ids("use gpui::*; fn r() { div().text_size(px(9.)) }"), vec!["tiny-text"]);
        assert!(ids("use gpui::*; fn r() { div().text_xs() }").is_empty());
        let pulse = "use gpui::*; fn r() { div().size_2().rounded_full().bg(green()).with_animation(\"p\", Animation::new(d).repeat().with_easing(pulsating_between(0.4, 1.)), |d, t| d.opacity(t)) }";
        assert_eq!(ids(pulse), vec!["pulsing-dot"]);
    }

    #[test]
    fn nested_cards() {
        let src = "use gpui::*; fn r() { div().bg(white()).rounded_lg().border_1().p_4().child(div().bg(white()).rounded_md().shadow_sm().p_2().child(a).child(b)) }";
        assert_eq!(ids(src), vec!["nested-cards"]);
        let input = "use gpui::*; fn r() { div().bg(white()).rounded_lg().border_1().child(div().bg(white()).rounded_md().border_1().child(editor)) }";
        assert!(ids(input).is_empty());
    }

    #[test]
    fn inline_ignore() {
        let src = "use gpui::*;\n// impeccable-disable-next-line overused-font\nfn r() { div().font_family(\"Inter\") }";
        assert!(ids(src).is_empty());
    }

    #[test]
    fn rust_paths() {
        assert!(is_rust_path("src/ui/view.rs"));
        assert!(!is_rust_path("C:\\app\\target\\debug\\build\\x\\out\\gen.rs"));
        assert!(!is_rust_path("src/app.tsx"));
    }

    #[test]
    fn icon_buttons_need_a_name_and_focusable_controls_a_focus_style() {
        let src = r#"use gpui::*;
fn bar(cx: &mut App) -> impl IntoElement {
    div()
        .child(div().id("close").size_6().on_click(|_, _, _| {}).child(svg().path("icons/x.svg")))
        .child(div().id("named").on_click(|_, _, _| {}).tooltip(Tooltip::text("Close")).child(svg().path("icons/x.svg")))
        .child(div().id("save").track_focus(&handle).on_click(|_, _, _| {}).child("Save"))
        .child(div().id("ok").track_focus(&handle).focus(|s| s.border_1()).on_click(|_, _, _| {}).child("Ok"))
}
"#;
        let found = detect_gpui_with(src, "ui/bar.rs", false, &Tokens::default());
        let ids: Vec<(&str, f64)> = found.iter().map(|f| (f.antipattern.as_str(), f.line)).collect();
        assert_eq!(ids, vec![("gpui/unnamed-icon-button", 4.0), ("gpui/no-focus-style", 6.0)], "{found:#?}");
    }

}
