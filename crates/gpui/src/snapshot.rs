//! Rules over a painted window: the JSON that impeccable-gpui-probe serves at
//! `GET /snapshot` (gpui-pre with the new-horizon-code-repo/gpui patch).
//! Unlike the source rules, these see resolved values: theme branches,
//! `.when(..)` styles, real text and the clip each text paints into.

use impeccable_core::color::{color_to_hex, composite_color_over, contrast_ratio, get_hue, has_chroma, is_gray_ink, Rgba};
use impeccable_core::constants::overused_font_name;
use impeccable_core::findings::{try_finding, Finding};
use serde_json::Value;

/// Desktop floor shared with the source rule.
const TINY_TEXT_PX: f64 = 11.0;
/// Slack for sub-pixel layout before a spill counts as overflow.
const OVERFLOW_SLACK_PX: f64 = 1.0;
/// `shadow_xl` and wider.
const WIDE_SHADOW_BLUR_PX: f64 = 25.0;

fn hex(v: &Value) -> Option<Rgba> {
    let s = v.as_str()?.strip_prefix('#')?;
    let n = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok().map(f64::from);
    let a = if s.len() >= 8 { n(6)? / 255.0 } else { 1.0 };
    Some(Rgba::new(n(0)?, n(2)?, n(4)?, a))
}

fn rect(v: &Value) -> Option<[f64; 4]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?])
}

/// A solid fill, `None` for no background, `Err` for a fill we cannot reduce
/// to one color (gradient, pattern).
fn fill(node: &Value) -> Result<Option<Rgba>, ()> {
    let bg = &node["style"]["background"];
    if bg.is_null() {
        return Ok(None);
    }
    let color = &bg["Color"];
    if color["tag"].as_str() != Some("Solid") {
        return Err(());
    }
    let c = hex(&color["solid"]).ok_or(())?;
    Ok((c.alpha_or_one() > 0.0).then_some(c))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn sides(v: &Value) -> [f64; 4] {
    let a = v.as_array();
    let at = |i: usize| a.and_then(|a| a.get(i)).map(num).unwrap_or(0.0);
    [at(0), at(1), at(2), at(3)]
}

fn is_purple(c: &Rgba) -> bool {
    has_chroma(Some(c), Some(50.0)) && (260.0..=310.0).contains(&get_hue(Some(c)))
}

/// A framed, filled, rounded box, like the source rule's card.
fn is_card(style: &Value) -> bool {
    let widths = sides(&style["borderWidths"]);
    let radius = sides(&style["cornerRadii"]).into_iter().fold(0.0, f64::max);
    let filled = !style["background"].is_null();
    let shadow = style["boxShadow"].as_array().is_some_and(|a| !a.is_empty());
    radius >= 4.0 && filled && (widths.iter().all(|w| *w >= 1.0) || shadow)
}

/// One accent stripe on one side of a box, as the source rule reads it.
fn stripe(style: &Value) -> Option<(&'static str, String)> {
    let color = hex(&style["borderColor"])?;
    if color.alpha_or_one() <= 0.0 || !has_chroma(Some(&color), Some(30.0)) {
        return None;
    }
    let widths = sides(&style["borderWidths"]);
    let radius = sides(&style["cornerRadii"]).into_iter().fold(0.0, f64::max);
    if radius >= 9999.0 {
        return None;
    }
    let names = ["top", "right", "bottom", "left"];
    (0..4).find_map(|i| {
        let w = widths[i];
        let max_other = (0..4).filter(|j| *j != i).map(|j| widths[j]).fold(0.0, f64::max);
        if !(w >= 2.0 && (max_other <= 1.0 || w >= max_other * 2.0)) {
            return None;
        }
        let side = i == 1 || i == 3;
        let detail = if radius > 0.0 {
            format!("{}: {w}px {} stripe on a {radius}px-rounded box", names[i], color_to_hex(Some(&color)))
        } else {
            format!("{}: {w}px {} stripe", names[i], color_to_hex(Some(&color)))
        };
        if side && (radius > 0.0 || w >= 3.0) {
            Some(("side-tab", detail))
        } else if !side && radius > 0.0 {
            Some(("border-accent-on-rounded", detail))
        } else {
            None
        }
    })
}

struct Node<'a> {
    v: &'a Value,
    parent: Option<usize>,
}

fn nodes(snapshot: &Value) -> Vec<Node<'_>> {
    snapshot["nodes"]
        .as_array()
        .map(|a| a.iter().map(|v| Node { v, parent: v["parent"].as_u64().map(|p| p as usize) }).collect())
        .unwrap_or_default()
}

fn ancestors<'a>(nodes: &'a [Node<'a>], idx: usize) -> impl Iterator<Item = usize> + 'a {
    std::iter::successors(nodes[idx].parent, |&i| nodes.get(i).and_then(|n| n.parent))
}

/// The color behind a node: ancestor fills composited bottom-up. `None` when
/// a gradient is in the way or nothing opaque is found.
fn backdrop(nodes: &[Node], idx: usize) -> Option<Rgba> {
    let mut layers = Vec::new();
    for i in ancestors(nodes, idx) {
        if let Some(c) = fill(nodes[i].v).ok()? {
            let opaque = c.alpha_or_one() >= 1.0;
            layers.push(c);
            if opaque {
                let mut color = layers.pop()?;
                while let Some(top) = layers.pop() {
                    color = composite_color_over(&top, &color);
                }
                return Some(color);
            }
        }
    }
    None
}

/// The source line of the element that holds a node.
fn source(nodes: &[Node], idx: usize) -> Option<(String, f64)> {
    std::iter::once(idx).chain(ancestors(nodes, idx)).find_map(|i| {
        let s = nodes[i].v["source"].as_str()?;
        let mut parts = s.rsplitn(3, ':');
        let _col = parts.next()?;
        let line: f64 = parts.next()?.parse().ok()?;
        Some((parts.next()?.replace('\\', "/"), line))
    })
}

fn quote(text: &str) -> String {
    let t: String = text.chars().take(40).collect();
    if t.len() < text.len() {
        format!("\"{t}…\"")
    } else {
        format!("\"{t}\"")
    }
}

#[derive(Default)]
struct Sink {
    seen: std::collections::HashSet<(String, String, i64, String)>,
    out: Vec<Finding>,
}

impl Sink {
    fn push(&mut self, id: &str, at: Option<(String, f64)>, snippet: String) {
        self.push_value(id, at, snippet, None);
    }

    /// Repeated instances of one source line report once.
    fn push_value(&mut self, id: &str, at: Option<(String, f64)>, snippet: String, value: Option<String>) {
        let (file, line) = at.unwrap_or_else(|| ("<window>".into(), 0.0));
        if !self.seen.insert((id.to_string(), file.clone(), line as i64, snippet.clone())) {
            return;
        }
        if let Some(mut f) = try_finding(id, &file, &snippet, line) {
            if let Some(v) = value {
                f.extras.insert("ignoreValue".into(), Value::String(v));
            }
            self.out.push(f);
        }
    }
}

/// Findings for one snapshot, at the source line of the element holding each text.
pub fn detect_snapshot(snapshot: &Value) -> Vec<Finding> {
    let nodes = nodes(snapshot);
    let mut sink = Sink::default();
    let mut fonts = std::collections::HashSet::new();
    let mut children = vec![0usize; nodes.len()];
    for n in &nodes {
        if let Some(c) = n.parent.and_then(|p| children.get_mut(p)) {
            *c += 1;
        }
    }

    for (idx, node) in nodes.iter().enumerate() {
        let v = node.v;
        if v["kind"].as_str() == Some("element") {
            let style = &v["style"];
            let at = source(&nodes, idx);
            if let Some((id, detail)) = stripe(style) {
                sink.push(id, at.clone(), detail);
            }
            if is_card(style) && children[idx] >= 2 {
                if let Some(outer) = ancestors(&nodes, idx).find(|&i| is_card(&nodes[i].v["style"])) {
                    let line = |i: usize| source(&nodes, i).map_or(0.0, |(_, l)| l);
                    sink.push("nested-cards", at.clone(), format!("card (line {}) inside card (line {})", line(idx), line(outer)));
                }
            }
            let shadows = style["boxShadow"].as_array().map(Vec::as_slice).unwrap_or_default();
            for sh in shadows {
                let Some(c) = hex(&sh["color"]) else { continue };
                let blur = num(&sh["blur_radius"]);
                let still = num(&sh["offset"]["x"]) == 0.0 && num(&sh["offset"]["y"]) == 0.0;
                if still && blur >= 6.0 && c.alpha_or_one() >= 0.2 && has_chroma(Some(&c), Some(50.0)) {
                    sink.push("dark-glow", at.clone(), format!("shadow {} with zero offset, {blur}px blur", color_to_hex(Some(&c))));
                }
            }
            let widths = sides(&style["borderWidths"]);
            if let Some(blur) = shadows.iter().map(|sh| num(&sh["blur_radius"])).reduce(f64::max) {
                if widths.iter().all(|w| *w == 1.0) && blur >= WIDE_SHADOW_BLUR_PX {
                    sink.push("gpt-thin-border-wide-shadow", at.clone(), format!("1px border + {blur}px shadow blur"));
                }
            }
            let bg = &style["background"]["Color"];
            if bg["tag"].as_str() == Some("LinearGradient") {
                let stops = bg["colors"].as_array().map(Vec::as_slice).unwrap_or_default();
                if let Some(p) = stops.iter().filter_map(|stop| hex(&stop["color"])).find(is_purple) {
                    sink.push("ai-color-palette", at.clone(), format!("Purple/violet gradient ({})", color_to_hex(Some(&p))));
                }
            }
            continue;
        }
        if v["kind"].as_str() != Some("text") {
            continue;
        }
        let text = v["text"].as_str().unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let at = source(&nodes, idx);
        let size = v["fontSize"].as_f64().unwrap_or(0.0);
        let weight = v["fontWeight"].as_f64().unwrap_or(400.0);

        if let Some(face) = v["fontFamily"].as_str().filter(|f| overused_font_name(&f.to_lowercase()).is_some()) {
            if fonts.insert(face.to_string()) {
                sink.push_value("overused-font", at.clone(), format!("font_family \"{face}\" (painted)"), Some(face.to_string()));
            }
        }

        if size > 0.0 && size < TINY_TEXT_PX {
            sink.push("tiny-text", at.clone(), format!("{} at {size}px (desktop floor is {TINY_TEXT_PX}px)", quote(text)));
        }

        if let Some(bg) = backdrop(&nodes, idx) {
            let large = size >= 24.0 || (size >= 18.66 && weight >= 700.0);
            let floor = if large { 3.0 } else { 4.5 };
            let worst = v["runs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|run| hex(&run["color"]))
                .map(|ink| {
                    let ink = if ink.alpha_or_one() < 1.0 { composite_color_over(&ink, &bg) } else { ink };
                    (contrast_ratio(&ink, &bg), ink)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, ink)) = worst.filter(|(_, ink)| is_gray_ink(ink) && has_chroma(Some(&bg), Some(50.0))) {
                sink.push(
                    "gray-on-color",
                    at.clone(),
                    format!("{} {} on {}", quote(text), color_to_hex(Some(&ink)), color_to_hex(Some(&bg))),
                );
            }
            if let Some((ratio, ink)) = worst.filter(|(r, _)| *r < floor) {
                sink.push(
                    "low-contrast",
                    at.clone(),
                    format!(
                        "{} {} on {}: {ratio:.1}:1 (needs {floor}:1)",
                        quote(text),
                        color_to_hex(Some(&ink)),
                        color_to_hex(Some(&bg))
                    ),
                );
            }
        }

        if let (Some(b), Some(clip), Some(s)) = (rect(&v["bounds"]), rect(&v["clip"]), v["size"].as_array()) {
            let w = s.first().and_then(Value::as_f64).unwrap_or(0.0);
            let h = s.get(1).and_then(Value::as_f64).unwrap_or(0.0);
            let spill_x = b[0] + w - (clip[0] + clip[2]);
            let spill_y = b[1] + h - (clip[1] + clip[3]);
            if spill_x > OVERFLOW_SLACK_PX || spill_y > OVERFLOW_SLACK_PX {
                let what = if spill_x > OVERFLOW_SLACK_PX {
                    format!("{w:.0}px wide in a {:.0}px clip", clip[2])
                } else {
                    format!("{h:.0}px tall in a {:.0}px clip", clip[3])
                };
                sink.push("text-overflow", at.clone(), format!("{} is cut off: {what}", quote(text)));
            }
        }
    }
    let mut out = sink.out;
    out.sort_by(|a, b| (&a.file, a.line).partial_cmp(&(&b.file, b.line)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn solid(hex: &str) -> Value {
        json!({ "Color": { "tag": "Solid", "solid": hex } })
    }

    fn snapshot() -> Value {
        json!({ "nodes": [
            { "kind": "element", "parent": null, "source": "examples\\demo.rs:47:9", "bounds": [0, 0, 720, 480], "clip": [0, 0, 720, 480], "style": { "background": solid("#111214ff") } },
            { "kind": "element", "parent": 0, "source": "examples\\demo.rs:35:13", "bounds": [24, 72, 672, 76], "clip": [0, 0, 720, 480], "style": { "background": solid("#1c1d21ff") } },
            { "kind": "element", "parent": 1, "source": "examples\\demo.rs:45:24", "bounds": [44, 116, 636, 16], "clip": [0, 0, 720, 480], "style": {} },
            { "kind": "text", "parent": 2, "text": "Finished 2 minutes ago", "fontSize": 10.0, "fontWeight": 400.0, "bounds": [44, 116, 103, 16], "clip": [0, 0, 720, 480], "size": [103, 16], "runs": [{ "len": 22, "color": "#7c7c86ff" }] },
            { "kind": "element", "parent": 0, "source": "examples\\demo.rs:58:17", "bounds": [24, 248, 160, 21], "clip": [0, 0, 720, 480], "style": { "overflow": { "x": "Hidden" } } },
            { "kind": "text", "parent": 4, "text": "Deploy to eu-west-1 failed after 3 retries", "fontSize": 13.0, "bounds": [24, 248, 233, 21], "clip": [24, 248, 160, 21], "size": [233, 21], "runs": [{ "len": 42, "color": "#ececeeff" }] },
            { "kind": "text", "parent": 0, "text": "Builds", "fontSize": 22.0, "bounds": [24, 24, 59, 36], "clip": [0, 0, 720, 480], "size": [59, 36], "runs": [{ "len": 6, "color": "#ececeeff" }] }
        ]})
    }

    #[test]
    fn finds_tiny_low_contrast_and_cut_off_text_at_their_source_lines() {
        let found = detect_snapshot(&snapshot());
        let ids: Vec<(&str, f64)> = found.iter().map(|f| (f.antipattern.as_str(), f.line)).collect();
        assert_eq!(ids, vec![("tiny-text", 45.0), ("low-contrast", 45.0), ("text-overflow", 58.0)], "{found:#?}");
        assert!(found.iter().all(|f| f.file == "examples/demo.rs"));
        assert!(found[1].snippet.contains("4.1:1"), "{}", found[1].snippet);
        assert!(found[2].snippet.contains("233px wide in a 160px clip"), "{}", found[2].snippet);
    }

    #[test]
    fn translucent_fills_composite_and_gradients_skip_contrast() {
        let mut s = snapshot();
        s["nodes"][1]["style"]["background"] = solid("#ffffff10");
        let found = detect_snapshot(&s);
        assert!(found.iter().any(|f| f.antipattern == "low-contrast"));

        s["nodes"][1]["style"]["background"] = json!({ "Color": { "tag": "LinearGradient" } });
        let found = detect_snapshot(&s);
        assert!(!found.iter().any(|f| f.antipattern == "low-contrast"));
    }

    fn el(parent: Option<usize>, line: u32, style: Value) -> Value {
        json!({ "kind": "element", "parent": parent, "source": format!("src/ui.rs:{line}:9"), "bounds": [0, 0, 400, 200], "clip": [0, 0, 400, 200], "style": style })
    }

    fn txt(parent: usize, text: &str, color: &str, family: &str) -> Value {
        json!({ "kind": "text", "parent": parent, "text": text, "fontSize": 14.0, "fontFamily": family, "bounds": [0, 0, 80, 20], "clip": [0, 0, 400, 200], "size": [80, 20], "runs": [{ "len": text.len(), "color": color }] })
    }

    #[test]
    fn element_rules_read_resolved_styles() {
        let card = |line| el(Some(0), line, json!({ "background": solid("#ffffffff"), "borderWidths": [1, 1, 1, 1], "cornerRadii": [8, 8, 8, 8], "borderColor": "#e4e4e7ff" }));
        let mut inner = card(8);
        inner["parent"] = json!(6);
        let snap = json!({ "nodes": [
            el(None, 1, json!({ "background": solid("#2563ebff") })),
            el(Some(0), 2, json!({ "borderWidths": [0, 0, 0, 4], "cornerRadii": [8, 8, 8, 8], "borderColor": "#7c3aedff" })),
            el(Some(0), 3, json!({ "boxShadow": [{ "color": "#7c3aedcc", "offset": { "x": 0, "y": 0 }, "blur_radius": 12, "spread_radius": 0 }] })),
            el(Some(0), 4, json!({ "borderWidths": [1, 1, 1, 1], "boxShadow": [{ "color": "#0000001a", "offset": { "x": 0, "y": 20 }, "blur_radius": 25, "spread_radius": -5 }] })),
            el(Some(0), 5, json!({ "background": { "Color": { "tag": "LinearGradient", "colors": [{ "color": "#7c3aedff" }, { "color": "#2563ebff" }] } } })),
            txt(0, "Muted", "#9a9a9aff", "Inter"),
            card(7),
            inner,
            txt(7, "Title", "#18181bff", "Inter"),
            txt(7, "Body", "#18181bff", "Inter"),
        ]});
        let found = detect_snapshot(&snap);
        let ids: Vec<(&str, f64)> = found.iter().map(|f| (f.antipattern.as_str(), f.line)).collect();
        for expected in [
            ("side-tab", 2.0),
            ("dark-glow", 3.0),
            ("gpt-thin-border-wide-shadow", 4.0),
            ("ai-color-palette", 5.0),
            ("gray-on-color", 1.0),
            ("overused-font", 1.0),
            ("nested-cards", 8.0),
        ] {
            assert!(ids.contains(&expected), "missing {expected:?} in {ids:?}");
        }
        let font = found.iter().find(|f| f.antipattern == "overused-font").unwrap();
        assert_eq!(font.extras.get("ignoreValue"), Some(&json!("Inter")));
        assert_eq!(found.iter().filter(|f| f.antipattern == "overused-font").count(), 1);
    }

    #[test]
    fn neutral_borders_and_fully_rounded_pills_are_not_stripes() {
        let snap = json!({ "nodes": [
            el(None, 1, json!({ "borderWidths": [0, 0, 0, 4], "cornerRadii": [8, 8, 8, 8], "borderColor": "#d4d4d8ff" })),
            el(None, 2, json!({ "borderWidths": [0, 0, 0, 4], "cornerRadii": [9999, 9999, 9999, 9999], "borderColor": "#7c3aedff" })),
        ]});
        assert!(detect_snapshot(&snap).is_empty());
    }

}
