//! The GPUI rules. Each one reuses a built-in rule id, so names,
//! descriptions, severities, ignores and `--list-rules` stay as they are; only
//! the way the pattern is found differs.

use crate::lex::{matching, Lexed, Tok};
use crate::model::Chain;
use crate::tokens::{Tokens, Variant};
use crate::value::{eval_length, hint_is_accent, split_args};
use impeccable_core::color::{color_to_hex, composite_color_over, contrast_ratio, get_hue, has_chroma, is_gray_ink, Rgba};
use impeccable_core::constants::overused_font_name;
use once_cell::sync::Lazy;
use regex::Regex;

pub struct Hit {
    pub id: &'static str,
    pub line: usize,
    pub snippet: String,
    pub value: Option<String>,
}

fn hit(id: &'static str, line: usize, snippet: String) -> Hit {
    Hit { id, line, snippet, value: None }
}

#[derive(Default)]
pub struct Style {
    /// top, right, bottom, left
    pub widths: [f64; 4],
    pub border_call_line: usize,
    pub border_call: String,
    pub border_color: Option<String>,
    pub radius: f64,
    pub radius_call: String,
    pub bg: Option<String>,
    pub text_color: Option<String>,
    pub text_size: Option<f64>,
    pub text_size_line: usize,
    pub shadow: Option<String>,
    pub size: Option<f64>,
}

static RE_BORDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^border(?:_(t|r|b|l|x|y))?(?:_(\d+))?$").unwrap());
static RE_ROUNDED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^rounded(?:_(?:t|b|l|r|tl|tr|bl|br))?(?:_(none|xs|sm|md|lg|xl|2xl|3xl|full))?$").unwrap());
static RE_SIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?:size|w|h)_(\d+)(?:p(\d+))?$").unwrap());
static RE_SHADOW: Lazy<Regex> = Lazy::new(|| Regex::new(r"^shadow_(xs|sm|md|lg|xl|2xl)$").unwrap());
const PX_PER_STEP: f64 = 4.0;

pub fn style_of(chain: &Chain) -> Style {
    let mut s = Style::default();
    for c in &chain.calls {
        let name = c.name.as_str();
        if let Some(m) = RE_BORDER.captures(name) {
            let width = match m.get(2) {
                Some(d) => d.as_str().parse::<f64>().ok(),
                None if !c.args.trim().is_empty() => eval_length(&c.args),
                None => None,
            };
            let Some(w) = width else { continue };
            let sides: &[usize] = match m.get(1).map(|x| x.as_str()) {
                None => &[0, 1, 2, 3],
                Some("t") => &[0],
                Some("r") => &[1],
                Some("b") => &[2],
                Some("l") => &[3],
                Some("x") => &[1, 3],
                _ => &[0, 2],
            };
            for side in sides {
                s.widths[*side] = w;
            }
            s.border_call_line = c.line;
            s.border_call = format!("{}({})", c.name, c.args.trim());
        } else if name == "border_color" {
            s.border_color = Some(c.args.clone());
        } else if let Some(m) = RE_ROUNDED.captures(name) {
            let r = match m.get(1).map(|x| x.as_str()) {
                Some("none") => Some(0.0),
                Some("xs") => Some(2.0),
                Some("sm") => Some(4.0),
                Some("md") => Some(6.0),
                Some("lg") => Some(8.0),
                Some("xl") => Some(12.0),
                Some("2xl") => Some(16.0),
                Some("3xl") => Some(24.0),
                Some(_) => Some(9999.0),
                None => eval_length(&c.args),
            };
            if let Some(r) = r {
                if r >= s.radius {
                    s.radius = r;
                    s.radius_call = format!("{}({})", c.name, c.args.trim());
                }
            }
        } else if name == "bg" {
            s.bg = Some(c.args.clone());
        } else if name == "text_color" {
            s.text_color = Some(c.args.clone());
        } else if name == "text_size" {
            s.text_size = eval_length(&c.args);
            s.text_size_line = c.line;
        } else if let Some(px) = text_step(name) {
            s.text_size = Some(px);
            s.text_size_line = c.line;
        } else if let Some(m) = RE_SHADOW.captures(name) {
            s.shadow = Some(m[1].to_string());
        } else if name == "shadow" {
            s.shadow = Some("custom".to_string());
        } else if let Some(m) = RE_SIZE.captures(name) {
            let whole: f64 = m[1].parse().unwrap_or(0.0);
            let frac = m.get(2).map(|f| format!("0.{}", f.as_str()).parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0);
            let v = (whole + frac) * PX_PER_STEP;
            s.size = Some(s.size.map_or(v, |o: f64| o.max(v)));
        } else if matches!(name, "size" | "w" | "h") {
            if let Some(v) = eval_length(&c.args) {
                s.size = Some(s.size.map_or(v, |o: f64| o.max(v)));
            }
        }
    }
    s
}

fn text_step(name: &str) -> Option<f64> {
    Some(match name {
        "text_xs" => 12.0,
        "text_sm" => 14.0,
        "text_base" => 16.0,
        "text_lg" => 18.0,
        "text_xl" => 20.0,
        "text_2xl" => 24.0,
        "text_3xl" => 30.0,
        _ => return None,
    })
}

fn is_card(s: &Style) -> bool {
    let all_sides = s.widths.iter().all(|w| *w >= 1.0);
    s.radius >= 4.0 && s.bg.is_some() && (all_sides || s.shadow.is_some())
}

/// Whether a border color is an accent (not a neutral). `None` when the
/// expression cannot be read either way.
fn border_is_accent(tokens: &Tokens, expr: Option<&str>) -> Option<bool> {
    let expr = expr?;
    let vs = tokens.colors(expr);
    if !vs.is_empty() {
        return Some(vs.iter().any(|v| has_chroma(Some(&v.color), Some(30.0))));
    }
    hint_is_accent(expr).then_some(true)
}

fn side_stripes(tokens: &Tokens, s: &Style) -> Vec<Hit> {
    let mut out = Vec::new();
    if s.radius >= 9999.0 || border_is_accent(tokens, s.border_color.as_deref()) != Some(true) {
        return out;
    }
    let names = ["top", "right", "bottom", "left"];
    for i in 0..4 {
        let w = s.widths[i];
        let max_other = (0..4).filter(|j| *j != i).map(|j| s.widths[j]).fold(0.0, f64::max);
        if !(w >= 2.0 && (max_other <= 1.0 || w >= max_other * 2.0)) {
            continue;
        }
        let side = i == 1 || i == 3;
        let detail = if s.radius > 0.0 {
            format!("{} + {} ({}: {}px stripe on a {}px-rounded box)", s.border_call, s.radius_call, names[i], w, s.radius)
        } else {
            format!("{} ({}: {}px stripe)", s.border_call, names[i], w)
        };
        if side && (s.radius > 0.0 || w >= 3.0) {
            out.push(hit("side-tab", s.border_call_line, detail));
        } else if !side && s.radius > 0.0 {
            out.push(hit("border-accent-on-rounded", s.border_call_line, detail));
        }
    }
    out
}

/// The nearest background behind a chain: its own, else the closest
/// ancestor's. `None` when that background cannot be evaluated or is not
/// opaque, since the rules then cannot know what the text sits on.
fn backdrop(tokens: &Tokens, chains: &[Chain], styles: &[Style], idx: usize) -> Option<Vec<Variant>> {
    let mut cur = Some(idx);
    while let Some(i) = cur {
        if let Some(bg) = styles[i].bg.as_deref() {
            let vs = tokens.colors(bg);
            let opaque = !vs.is_empty() && vs.iter().all(|v| v.color.alpha_or_one() >= 0.99);
            return opaque.then_some(vs);
        }
        cur = chains[i].parent;
    }
    None
}

/// Ink and background variants from the same branch: equal conditions, or
/// an unconditional side against every branch of the other.
fn pairs<'a>(ink: &'a [Variant], bg: &'a [Variant]) -> Vec<(&'a Variant, &'a Variant)> {
    let mut out = Vec::new();
    for i in ink {
        for b in bg {
            if i.when.is_empty() || b.when.is_empty() || i.when == b.when {
                out.push((i, b));
            }
        }
    }
    out
}

fn when_label(i: &Variant, b: &Variant) -> String {
    let w = if i.when.is_empty() { &b.when } else { &i.when };
    if w.is_empty() {
        String::new()
    } else {
        format!(" when {w}")
    }
}

fn contrast(tokens: &Tokens, chains: &[Chain], styles: &[Style], idx: usize) -> Vec<Hit> {
    let s = &styles[idx];
    let Some(expr) = s.text_color.as_deref() else { return vec![] };
    let ink = tokens.colors(expr);
    if ink.is_empty() {
        return vec![];
    }
    let Some(bg) = backdrop(tokens, chains, styles, idx) else { return vec![] };
    let line = chains[idx].calls.iter().find(|c| c.name == "text_color").map(|c| c.line).unwrap_or(chains[idx].line);
    let large = s.text_size.is_some_and(|px| px >= 24.0);
    let floor = if large { 3.0 } else { 4.5 };
    let mut worst: Option<(f64, String)> = None;
    let mut gray: Option<String> = None;
    for (i, b) in pairs(&ink, &bg) {
        let c = if i.color.alpha_or_one() < 1.0 { composite_color_over(&i.color, &b.color) } else { i.color };
        let ratio = contrast_ratio(&c, &b.color);
        let pair = format!("text_color {} on bg {}{}", color_to_hex(Some(&c)), color_to_hex(Some(&b.color)), when_label(i, b));
        if ratio < floor && worst.as_ref().is_none_or(|(r, _)| ratio < *r) {
            worst = Some((ratio, pair.clone()));
        }
        if gray.is_none() && is_gray_ink(&c) && has_chroma(Some(&b.color), Some(50.0)) {
            gray = Some(pair);
        }
    }
    let mut out = Vec::new();
    if let Some((ratio, pair)) = worst {
        out.push(hit("low-contrast", line, format!("{pair}: {:.1}:1 (needs {floor}:1)", ratio)));
    }
    if let Some(pair) = gray {
        out.push(hit("gray-on-color", line, pair));
    }
    out
}

fn is_purple(c: &Rgba) -> bool {
    let hue = get_hue(Some(c));
    has_chroma(Some(c), Some(50.0)) && (260.0..=310.0).contains(&hue)
}

/// File-level scans over tokens: gradients, box shadows, easings.
fn first_color(tokens: &Tokens, expr: &str) -> Vec<Rgba> {
    tokens.colors(expr).into_iter().map(|v| v.color).collect()
}

fn token_rules(lexed: &Lexed, tokens: &Tokens) -> Vec<Hit> {
    let t = &lexed.tokens;
    let src = &lexed.code;
    let mut out = Vec::new();
    for i in 0..t.len() {
        let Some(name) = t[i].ident() else { continue };
        let after_dot = i > 0 && t[i - 1].is_punct('.');
        let called = t.get(i + 1).is_some_and(|n| n.is_punct('('));
        if name == "linear_gradient" && called {
            let close = matching(t, i + 1);
            if close >= t.len() {
                continue;
            }
            let args = &src[t[i + 1].end..t[close].start];
            let stops: Vec<Rgba> = split_args(args)
                .iter()
                .skip(1)
                .flat_map(|stop| {
                    let inner = stop.trim();
                    let color = inner
                        .strip_prefix("linear_color_stop(")
                        .and_then(|r| r.strip_suffix(')'))
                        .map(|r| split_args(r).first().cloned().unwrap_or_default())
                        .unwrap_or_else(|| inner.to_string());
                    first_color(tokens, &color)
                })
                .collect();
            if let Some(p) = stops.iter().find(|c| is_purple(c)) {
                out.push(hit("ai-color-palette", t[i].line, format!("Purple/violet gradient (linear_gradient, {})", color_to_hex(Some(p)))));
            }
        } else if name == "BoxShadow" && t.get(i + 1).is_some_and(|n| n.is_punct('{')) {
            let close = matching(t, i + 1);
            if close >= t.len() {
                continue;
            }
            let body = &src[t[i + 1].end..t[close].start];
            let mut color: Vec<Rgba> = Vec::new();
            let mut zero_offset = false;
            let mut blur = 0.0;
            for field in split_args(body) {
                let Some((k, v)) = field.split_once(':') else { continue };
                match k.trim() {
                    "color" => color = first_color(tokens, v),
                    "offset" => {
                        let v = v.trim();
                        zero_offset = v.strip_prefix("point(").and_then(|r| r.strip_suffix(')')).is_some_and(|r| {
                            split_args(r).iter().all(|a| eval_length(a) == Some(0.0))
                        }) || v.contains("Point::default()")
                            || v.contains("Point::zero()");
                    }
                    "blur_radius" => blur = eval_length(v).unwrap_or(0.0),
                    _ => {}
                }
            }
            if let Some(c) = color.into_iter().find(|c| has_chroma(Some(c), Some(50.0))) {
                if zero_offset && blur >= 6.0 && c.alpha_or_one() >= 0.2 && has_chroma(Some(&c), Some(50.0)) {
                    out.push(hit("dark-glow", t[i].line, format!("BoxShadow {} with zero offset, {}px blur", color_to_hex(Some(&c)), blur)));
                }
            }
        } else if (name == "bounce" || name == "elastic") && called && !after_dot {
            let prev_fn = i > 0 && t[i - 1].ident() == Some("fn");
            if !prev_fn {
                let mut h = hit("bounce-easing", t[i].line, format!("{name}(..) easing"));
                h.value = Some(name.to_string());
                out.push(h);
            }
        }
    }
    out
}

/// `.font_family(X)` and `font_family: X`, with X a literal or a constant.
fn font_rules(lexed: &Lexed, tokens: &Tokens) -> Vec<Hit> {
    let t = &lexed.tokens;
    let src = &lexed.code;
    let mut out = Vec::new();
    for i in 0..t.len() {
        if t[i].ident() != Some("font_family") {
            continue;
        }
        let expr = match t.get(i + 1).map(|n| &n.tok) {
            Some(Tok::Punct('(')) => {
                let close = matching(t, i + 1);
                if close >= t.len() {
                    continue;
                }
                src[t[i + 1].end..t[close].start].to_string()
            }
            Some(Tok::Punct(':')) if !t.get(i + 2).is_some_and(|n| n.is_punct(':')) => {
                let mut depth = 0i32;
                let mut end = i + 2;
                while end < t.len() {
                    match t[end].tok {
                        Tok::Punct('(' | '[' | '{') => depth += 1,
                        Tok::Punct(')' | ']' | '}') if depth == 0 => break,
                        Tok::Punct(')' | ']' | '}') => depth -= 1,
                        Tok::Punct(',' | ';') if depth == 0 => break,
                        _ => {}
                    }
                    end += 1;
                }
                if end <= i + 2 || end > t.len() {
                    continue;
                }
                src[t[i + 2].start..t[end - 1].end].to_string()
            }
            _ => continue,
        };
        let Some(face) = tokens.string(&expr) else { continue };
        if overused_font_name(&face.to_lowercase()).is_none() {
            continue;
        }
        let shown = expr.trim();
        let snippet = if shown.starts_with('"') {
            format!("font_family({shown})")
        } else {
            format!("font_family({shown}) = \"{face}\"")
        };
        let mut h = hit("overused-font", t[i].line, snippet);
        h.value = Some(face);
        out.push(h);
    }
    out
}

/// An icon-only child: `svg()`, `img(..)`, or an icon component.
fn is_icon(expr: &str) -> bool {
    let e = expr.trim();
    e.starts_with("svg()") || e.starts_with("img(") || e.starts_with("Icon::") || e.starts_with("icon(") || e.contains("IconName::")
}

/// Keyboard rules a source chain can answer on its own. Reachability needs
/// the painted tree (`impeccable gpui snapshot`): a focusable container
/// built elsewhere may handle the row's command.
fn input_rules(chain: &Chain) -> Vec<Hit> {
    let mut out = Vec::new();
    // A helper such as `button(id, label, ..)` may name and focus itself.
    if !matches!(chain.root.as_str(), "div" | "h_flex" | "v_flex") {
        return out;
    }
    let Some(click) = chain.call("on_click").or_else(|| chain.call("on_mouse_down")) else { return out };
    let kids: Vec<&str> = chain.calls.iter().filter(|c| c.name == "child").map(|c| c.args.as_str()).collect();
    let icon_only = !kids.is_empty() && chain.call("children").is_none() && kids.iter().all(|k| is_icon(k));
    let named = ["tooltip", "aria_label", "label"].iter().any(|n| chain.call(n).is_some());
    if icon_only && !named {
        out.push(hit(crate::pack::UNNAMED_ICON_BUTTON, click.line, format!("{} with only an icon child and no .tooltip(..)", click.name)));
    }
    let focus_styled = ["focus", "focus_visible", "in_focus"].iter().any(|n| chain.call(n).is_some());
    if chain.call("track_focus").is_some() && chain.call("key_context").is_none() && !focus_styled {
        out.push(hit(crate::pack::NO_FOCUS_STYLE, click.line, "track_focus + on_click with no .focus(..) or .focus_visible(..) style".into()));
    }
    out
}

pub fn run(lexed: &Lexed, chains: &[Chain], tokens: &Tokens) -> Vec<Hit> {
    let styles: Vec<Style> = chains.iter().map(style_of).collect();
    let mut out = font_rules(lexed, tokens);
    out.extend(token_rules(lexed, tokens));
    for (idx, chain) in chains.iter().enumerate() {
        let s = &styles[idx];
        out.extend(side_stripes(tokens, s));
        out.extend(contrast(tokens, chains, &styles, idx));
        if let Some(px) = s.text_size.filter(|px| *px < 11.0) {
            out.push(hit("tiny-text", s.text_size_line, format!("{px}px text (desktop floor is 11px)")));
        }
        if s.radius >= 9999.0
            && s.size.is_some_and(|px| px <= 12.0)
            && chain.call("with_animation").is_some_and(|c| c.args.contains("pulsating_between"))
        {
            out.push(hit("pulsing-dot", chain.line, "pulsating_between on a small rounded_full dot".to_string()));
        }
        if s.widths.iter().all(|w| *w == 1.0) && matches!(s.shadow.as_deref(), Some("xl" | "2xl")) {
            out.push(hit("gpt-thin-border-wide-shadow", s.border_call_line, format!("border_1 + shadow_{}", s.shadow.as_deref().unwrap())));
        }
        out.extend(input_rules(chain));
        // An input or a chip is a framed box too; a nested card holds content.
        let holds_content = chain.calls.iter().filter(|c| c.name == "child" || c.name == "children").count() >= 2;
        if is_card(s) && holds_content {
            let mut p = chain.parent;
            while let Some(pi) = p {
                if is_card(&styles[pi]) {
                    out.push(hit("nested-cards", chain.line, format!("card (line {}) inside card (line {})", chain.line, chains[pi].line)));
                    break;
                }
                p = chains[pi].parent;
            }
        }
    }
    out
}
