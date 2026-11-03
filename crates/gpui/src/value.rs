//! Static evaluation of the GPUI expressions the rules read: colors
//! (`rgb(0x..)`, `hsla(..)`, `white().opacity(..)`) and lengths (`px(..)`,
//! `rems(..)`). Anything that needs runtime state (a theme lookup, a
//! variable) evaluates to `None` and the rules stay quiet about it.

use impeccable_core::color::{hsl_to_rgb, Rgba};

/// GPUI's default rem size.
pub const REM_PX: f64 = 16.0;

fn squash(expr: &str) -> String {
    let mut s: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    for prefix in ["gpui::", "::gpui::"] {
        while let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    for suffix in [".into()", ".to_rgb()", ".clone()"] {
        while let Some(rest) = s.strip_suffix(suffix) {
            s = rest.to_string();
        }
    }
    s
}

/// `name(args)` when `s` is exactly one call; returns (name, args).
fn split_call(s: &str) -> Option<(&str, &str)> {
    let open = s.find('(')?;
    if !s.ends_with(')') {
        return None;
    }
    let name = &s[..open];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') {
        return None;
    }
    let args = &s[open + 1..s.len() - 1];
    // The closing paren must close the opening one.
    let mut depth = 0;
    for c in args.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some((name.rsplit("::").next().unwrap_or(name), args))
}

pub fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in args.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// A numeric literal or a simple `a / b`, `a * b`, `a + b`, `a - b`.
pub fn eval_number(expr: &str) -> Option<f64> {
    let s = squash(expr);
    let s = s.trim_matches(|c| c == '(' || c == ')');
    for op in ['+', '-', '*', '/'] {
        // Split on the last operator that is not a leading sign.
        if let Some(pos) = s.rfind(op).filter(|p| *p > 0) {
            let (a, b) = (eval_number(&s[..pos])?, eval_number(&s[pos + 1..])?);
            return Some(match op {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                _ if b != 0.0 => a / b,
                _ => return None,
            });
        }
    }
    let lit = s.trim_end_matches("f32").trim_end_matches("f64").trim_end_matches('_');
    let lit = lit.strip_suffix('.').unwrap_or(lit).replace('_', "");
    lit.parse::<f64>().ok()
}

/// A length in pixels: `px(3.)`, `rems(0.5)`, `3.`.
pub fn eval_length(expr: &str) -> Option<f64> {
    let s = squash(expr);
    if let Some((name, args)) = split_call(&s) {
        return match name {
            "px" => eval_number(args),
            "rems" => eval_number(args).map(|v| v * REM_PX),
            "relative" | "DefiniteLength" | "Length" => None,
            _ => None,
        };
    }
    eval_number(&s)
}

fn hex_channels(hex: &str) -> Option<u32> {
    let h = hex.trim().trim_start_matches("0x").trim_start_matches("0X").replace('_', "");
    u32::from_str_radix(&h, 16).ok()
}

fn hsla(h: f64, s: f64, l: f64, a: f64) -> Rgba {
    let mut c = hsl_to_rgb(h * 360.0, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    c.a = Some(a.clamp(0.0, 1.0));
    c
}

/// Evaluate a GPUI color expression.
pub fn eval_color(expr: &str) -> Option<Rgba> {
    let s = squash(expr);
    // `color.opacity(x)` / `color.alpha(x)`.
    for m in [".opacity(", ".alpha("] {
        if let Some(pos) = s.rfind(m) {
            if s.ends_with(')') {
                let base = eval_color(&s[..pos])?;
                let a = eval_number(&s[pos + m.len()..s.len() - 1])?;
                let mut c = base;
                c.a = Some(if m == ".opacity(" { base.alpha_or_one() * a } else { a });
                return Some(c);
            }
        }
    }
    let (name, args) = split_call(&s)?;
    let a = split_args(args);
    match (name, a.len()) {
        ("rgb", 1) => {
            let v = hex_channels(&a[0])?;
            Some(Rgba::new(((v >> 16) & 0xff) as f64, ((v >> 8) & 0xff) as f64, (v & 0xff) as f64, 1.0))
        }
        ("rgba", 1) => {
            let v = hex_channels(&a[0])?;
            Some(Rgba::new(
                ((v >> 24) & 0xff) as f64,
                ((v >> 16) & 0xff) as f64,
                ((v >> 8) & 0xff) as f64,
                (v & 0xff) as f64 / 255.0,
            ))
        }
        ("hsla", 4) => Some(hsla(eval_number(&a[0])?, eval_number(&a[1])?, eval_number(&a[2])?, eval_number(&a[3])?)),
        ("opaque_grey", 2) => Some(hsla(0.0, 0.0, eval_number(&a[0])?, eval_number(&a[1])?)),
        ("black", 0) => Some(hsla(0.0, 0.0, 0.0, 1.0)),
        ("white", 0) => Some(hsla(0.0, 0.0, 1.0, 1.0)),
        ("transparent_black", 0) => Some(hsla(0.0, 0.0, 0.0, 0.0)),
        ("transparent_white", 0) => Some(hsla(0.0, 0.0, 1.0, 0.0)),
        ("red", 0) => Some(hsla(0.0, 1.0, 0.5, 1.0)),
        ("blue", 0) => Some(hsla(0.6, 1.0, 0.5, 1.0)),
        ("green", 0) => Some(hsla(0.33, 1.0, 0.5, 1.0)),
        ("yellow", 0) => Some(hsla(0.16, 1.0, 0.5, 1.0)),
        _ => None,
    }
}

/// The last identifier of an expression the rules cannot evaluate, as a
/// naming hint: `cx.theme().colors().text_accent` -> `text_accent`.
pub fn name_hint(expr: &str) -> String {
    let s = squash(expr);
    let trimmed = s.trim_end_matches("()");
    trimmed
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|p| !p.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Whether an unresolved color's name says it is an accent, not a neutral.
pub fn hint_is_accent(expr: &str) -> bool {
    let h = name_hint(expr);
    ["accent", "primary", "brand", "highlight", "purple", "violet", "indigo", "pink", "cyan", "teal"]
        .iter()
        .any(|w| h.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        let c = eval_color("rgb(0x7c3aed)").unwrap();
        assert_eq!((c.r, c.g, c.b), (124.0, 58.0, 237.0));
        let c = eval_color("gpui::rgba(0xff000080)").unwrap();
        assert!((c.alpha_or_one() - 128.0 / 255.0).abs() < 1e-9);
        let c = eval_color("hsla(270. / 360., 0.8, 0.5, 1.)").unwrap();
        assert!(c.b > c.g);
        let c = eval_color("white().opacity(0.5)").unwrap();
        assert_eq!(c.alpha_or_one(), 0.5);
        assert!(eval_color("cx.theme().colors().text").is_none());
    }

    #[test]
    fn lengths() {
        assert_eq!(eval_length("px(3.)"), Some(3.0));
        assert_eq!(eval_length("rems(0.5)"), Some(8.0));
        assert_eq!(eval_length("px(10.0_f32)"), Some(10.0));
        assert_eq!(eval_length("relative(0.5)"), None);
    }

    #[test]
    fn hints() {
        assert_eq!(name_hint("cx.theme().colors().text_accent"), "text_accent");
        assert!(hint_is_accent("theme.colors.primary"));
        assert!(!hint_is_accent("cx.theme().colors().border"));
    }
}
