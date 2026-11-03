//! Rules over a painted window: the JSON that impeccable-gpui-probe serves at
//! `GET /snapshot` (gpui-pre with the new-horizon-code-repo/gpui patch).
//! Unlike the source rules, these see resolved values: theme branches,
//! `.when(..)` styles, real text and the clip each text paints into.

use impeccable_core::color::{color_to_hex, composite_color_over, contrast_ratio, Rgba};
use impeccable_core::findings::{try_finding, Finding};
use serde_json::Value;

/// Desktop floor shared with the source rule.
const TINY_TEXT_PX: f64 = 11.0;
/// Slack for sub-pixel layout before a spill counts as overflow.
const OVERFLOW_SLACK_PX: f64 = 1.0;

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

/// Findings for one snapshot, at the source line of the element holding each text.
pub fn detect_snapshot(snapshot: &Value) -> Vec<Finding> {
    let nodes = nodes(snapshot);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut push = |id: &str, at: Option<(String, f64)>, snippet: String| {
        let (file, line) = at.unwrap_or_else(|| ("<window>".into(), 0.0));
        if seen.insert((id.to_string(), file.clone(), line as i64, snippet.clone())) {
            out.extend(try_finding(id, &file, &snippet, line));
        }
    };

    for (idx, node) in nodes.iter().enumerate() {
        let v = node.v;
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

        if size > 0.0 && size < TINY_TEXT_PX {
            push("tiny-text", at.clone(), format!("{} at {size}px (desktop floor is {TINY_TEXT_PX}px)", quote(text)));
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
            if let Some((ratio, ink)) = worst.filter(|(r, _)| *r < floor) {
                push(
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
                push("text-overflow", at.clone(), format!("{} is cut off: {what}", quote(text)));
            }
        }
    }
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
}
