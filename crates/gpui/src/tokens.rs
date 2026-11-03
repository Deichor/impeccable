//! Project design tokens: the zero-argument color functions and constants a
//! GPUI app defines (`pub fn accent() -> Rgba { pick(0x9c86ff, 0x5e40dc) }`,
//! `pub const SANS: &str = "Inter"`). The rules resolve `theme::accent()`
//! through them. An `if .. { a } else { b }` in a token (a dark/light switch)
//! yields one variant per branch, labelled with its condition.

use crate::lex::{lex, matching, Tok};
use crate::value::{eval_color, split_args};
use impeccable_core::color::Rgba;
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct FnDef {
    module: String,
    params: Vec<String>,
    body: String,
}

#[derive(Debug, Clone)]
struct ConstDef {
    module: String,
    expr: String,
}

#[derive(Debug, Default)]
pub struct Tokens {
    fns: HashMap<String, Vec<FnDef>>,
    consts: HashMap<String, Vec<ConstDef>>,
}

/// One evaluated color and the branch it came from (`""` when unconditional).
#[derive(Debug, Clone)]
pub struct Variant {
    pub color: Rgba,
    pub when: String,
}

const MAX_DEPTH: usize = 6;
const MAX_FILES: usize = 4000;

fn module_of(path: &str) -> String {
    let p = path.replace('\\', "/");
    let file = p.rsplit('/').next().unwrap_or("");
    let stem = file.trim_end_matches(".rs");
    if stem == "mod" || stem == "lib" {
        p.rsplit('/').nth(1).unwrap_or(stem).to_string()
    } else {
        stem.to_string()
    }
}

impl Tokens {
    pub fn from_sources<'a>(sources: impl IntoIterator<Item = (&'a str, &'a str)>) -> Tokens {
        let mut t = Tokens::default();
        for (path, content) in sources {
            t.add(path, content);
        }
        t
    }

    fn add(&mut self, path: &str, content: &str) {
        let module = module_of(path);
        let lexed = lex(content);
        let toks = &lexed.tokens;
        let src = &lexed.code;
        let mut i = 0;
        while i + 2 < toks.len() {
            let kw = toks[i].ident();
            if kw == Some("fn") {
                if let (Some(name), true) = (toks[i + 1].ident(), toks[i + 2].is_punct('(')) {
                    let close = matching(toks, i + 2);
                    let open = (close + 1..toks.len()).find(|k| toks[*k].is_punct('{') || toks[*k].is_punct(';'));
                    if let Some(open) = open.filter(|k| toks[*k].is_punct('{')) {
                        let end = matching(toks, open);
                        if end < toks.len() && end > open + 1 {
                            let body_toks = &toks[open + 1..end];
                            let simple = !body_toks.iter().any(|t| t.is_punct(';'))
                                && body_toks.first().and_then(|t| t.ident()) != Some("let");
                            if simple {
                                let params = split_args(&src[toks[i + 2].end..toks[close].start])
                                    .iter()
                                    .filter_map(|p| {
                                        let p = p.trim().trim_start_matches("mut ").trim();
                                        let (n, _) = p.split_once(':')?;
                                        Some(n.trim().to_string())
                                    })
                                    .filter(|n| n != "self" && n != "&self")
                                    .collect();
                                let body = src[toks[open].end..toks[end].start].trim().to_string();
                                self.fns.entry(name.to_string()).or_default().push(FnDef { module: module.clone(), params, body });
                            }
                        }
                        i = end.max(i + 1);
                        continue;
                    }
                }
            } else if matches!(kw, Some("const") | Some("static")) {
                if let (Some(name), true) = (toks[i + 1].ident(), toks[i + 2].is_punct(':')) {
                    let eq = (i + 3..toks.len()).find(|k| toks[*k].is_punct('=') || toks[*k].is_punct(';'));
                    if let Some(eq) = eq.filter(|k| toks[*k].is_punct('=')) {
                        let mut depth = 0i32;
                        let mut end = eq + 1;
                        while end < toks.len() {
                            match toks[end].tok {
                                Tok::Punct('(' | '[' | '{') => depth += 1,
                                Tok::Punct(')' | ']' | '}') => depth -= 1,
                                Tok::Punct(';') if depth == 0 => break,
                                _ => {}
                            }
                            end += 1;
                        }
                        if end < toks.len() && end > eq + 1 {
                            let expr = src[toks[eq + 1].start..toks[end - 1].end].to_string();
                            self.consts.entry(name.to_string()).or_default().push(ConstDef { module: module.clone(), expr });
                        }
                        i = end;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    fn pick<'a, T>(defs: Option<&'a Vec<T>>, qualifier: Option<&str>, module: impl Fn(&T) -> &str) -> Option<&'a T> {
        let defs = defs?;
        if let Some(q) = qualifier {
            if let Some(d) = defs.iter().find(|d| module(d) == q) {
                return Some(d);
            }
        }
        (defs.len() == 1).then(|| &defs[0])
    }

    /// Every color an expression can evaluate to. Empty when unknown.
    pub fn colors(&self, expr: &str) -> Vec<Variant> {
        self.colors_at(expr, 0)
    }

    fn colors_at(&self, expr: &str, depth: usize) -> Vec<Variant> {
        if depth > MAX_DEPTH {
            return vec![];
        }
        let expr = expr.trim();
        if let Some((cond, a, b)) = split_if(expr) {
            let mut out = Vec::new();
            for (branch, when) in [(a, cond.clone()), (b, format!("!({cond})"))] {
                let vs = self.colors_at(&branch, depth + 1);
                if vs.is_empty() {
                    return vec![];
                }
                out.extend(vs.into_iter().map(|v| Variant { when: if v.when.is_empty() { when.clone() } else { v.when }, ..v }));
            }
            return out;
        }
        if let Some(c) = eval_color(expr) {
            return vec![Variant { color: c, when: String::new() }];
        }
        let squashed: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
        let squashed = squashed.trim_end_matches(".into()").trim_end_matches(".clone()");
        // `token().opacity(x)` / `.alpha(x)`.
        for m in [".opacity(", ".alpha("] {
            if let Some(pos) = squashed.rfind(m) {
                if squashed.ends_with(')') {
                    let factor = crate::value::eval_number(&squashed[pos + m.len()..squashed.len() - 1]);
                    let Some(f) = factor else { return vec![] };
                    return self
                        .colors_at(&squashed[..pos], depth + 1)
                        .into_iter()
                        .map(|mut v| {
                            let a = if m == ".opacity(" { v.color.alpha_or_one() * f } else { f };
                            v.color.a = Some(a);
                            v
                        })
                        .collect();
                }
            }
        }
        let (path, args) = match squashed.find('(') {
            Some(open) if squashed.ends_with(')') => (&squashed[..open], Some(&squashed[open + 1..squashed.len() - 1])),
            _ => (squashed, None),
        };
        if !path.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '.') {
            return vec![];
        }
        let mut segs = path.rsplit(|c| c == ':' || c == '.').filter(|s| !s.is_empty());
        let Some(name) = segs.next() else { return vec![] };
        let qualifier = segs.next();
        match args {
            Some(args) => {
                let Some(def) = Self::pick(self.fns.get(name), qualifier, |d: &FnDef| &d.module) else { return vec![] };
                let args = split_args(args);
                if args.len() != def.params.len() {
                    return vec![];
                }
                let mut body = def.body.clone();
                for (p, a) in def.params.iter().zip(args.iter()) {
                    let re = Regex::new(&format!(r"\b{}\b", regex::escape(p))).unwrap();
                    body = re.replace_all(&body, regex::NoExpand(a)).into_owned();
                }
                self.colors_at(&body, depth + 1)
            }
            None => match Self::pick(self.consts.get(name), qualifier, |d: &ConstDef| &d.module) {
                Some(c) => self.colors_at(&c.expr.clone(), depth + 1),
                None => vec![],
            },
        }
    }

    /// A string an expression names: a literal, or a `const` holding one.
    pub fn string(&self, expr: &str) -> Option<String> {
        let s: String = expr.trim().to_string();
        let s = s.trim_end_matches(".into()").trim_end_matches(".to_string()").trim();
        let s = s.strip_prefix("SharedString::from(").and_then(|r| r.strip_suffix(')')).unwrap_or(s).trim();
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            return Some(s[1..s.len() - 1].to_string());
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':') {
            return None;
        }
        let mut segs = s.rsplit(':').filter(|x| !x.is_empty());
        let name = segs.next()?;
        let def = Self::pick(self.consts.get(name), segs.next(), |d: &ConstDef| &d.module)?;
        let e = def.expr.trim();
        (e.len() >= 2 && e.starts_with('"') && e.ends_with('"')).then(|| e[1..e.len() - 1].to_string())
    }
}

/// `if COND { A } else { B }` anywhere in `expr`: (COND, expr with A, expr
/// with B). `else if` chains are not expanded.
fn split_if(expr: &str) -> Option<(String, String, String)> {
    let bytes = expr.as_bytes();
    let mut i = 0;
    while let Some(pos) = expr[i..].find("if") {
        let at = i + pos;
        let before_ok = at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
        let after_ok = bytes.get(at + 2).is_some_and(|b| b.is_ascii_whitespace() || *b == b'!' || *b == b'(');
        if !(before_ok && after_ok) {
            i = at + 2;
            continue;
        }
        let open_a = at + expr[at..].find('{')?;
        let close_a = close_brace(expr, open_a)?;
        let rest = expr[close_a + 1..].trim_start();
        let after_else = rest.strip_prefix("else")?.trim_start();
        if !after_else.starts_with('{') {
            return None;
        }
        let open_b = expr.len() - after_else.len();
        let close_b = close_brace(expr, open_b)?;
        let cond = expr[at + 2..open_a].trim().to_string();
        let a = expr[open_a + 1..close_a].trim();
        let b = expr[open_b + 1..close_b].trim();
        let head = &expr[..at];
        let tail = &expr[close_b + 1..];
        return Some((cond, format!("{head}{a}{tail}"), format!("{head}{b}{tail}")));
    }
    None
}

fn close_brace(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (k, c) in s[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + k);
                }
            }
            _ => {}
        }
    }
    None
}

// ─── project discovery ─────────────────────────────────────────────────────

/// The outermost directory above `file_path` that holds a `Cargo.toml`
/// (the workspace root), stopping at a repository root.
fn workspace_root(file_path: &str) -> Option<std::path::PathBuf> {
    let mut dir = std::path::Path::new(file_path).parent()?.to_path_buf();
    if !dir.is_absolute() {
        dir = std::env::current_dir().ok()?.join(dir);
    }
    let mut best = None;
    loop {
        if dir.join("Cargo.toml").is_file() {
            best = Some(dir.clone());
        }
        if dir.join(".git").exists() {
            break;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    best
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            collect_rs(&path, out);
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

static CACHE: Mutex<Option<HashMap<std::path::PathBuf, Arc<Tokens>>>> = Mutex::new(None);

/// The tokens of the Cargo workspace `file_path` belongs to, read once per
/// process. Empty for a file outside any Cargo project.
pub fn for_file(file_path: &str) -> Arc<Tokens> {
    let Some(root) = workspace_root(file_path) else { return Arc::new(Tokens::default()) };
    let mut guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(t) = cache.get(&root) {
        return t.clone();
    }
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    let mut tokens = Tokens::default();
    for f in files {
        let Ok(content) = std::fs::read_to_string(&f) else { continue };
        if content.contains("Rgba") || content.contains("Hsla") || content.contains("const ") || content.contains("static ") {
            tokens.add(&f.to_string_lossy(), &content);
        }
    }
    let tokens = Arc::new(tokens);
    cache.insert(root, tokens.clone());
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME: &str = r#"
use gpui::{Rgba, rgb};
pub const SANS: &str = "Inter";
fn pick(dark: u32, light: u32) -> Rgba {
    rgb(if is_dark() { dark } else { light })
}
pub fn accent() -> Rgba { pick(0x9c86ff, 0x5e40dc) }
pub fn brand() -> Rgba { rgb(0x7b5cfa) }
pub fn tint(i: usize) -> Rgba { let x = i; rgb(0) }
"#;

    #[test]
    fn resolves_dark_light_tokens() {
        let t = Tokens::from_sources([("src/theme.rs", THEME)]);
        let v = t.colors("theme::accent()");
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].color.r, v[0].color.g, v[0].color.b), (156.0, 134.0, 255.0));
        assert_eq!(v[0].when, "is_dark()");
        assert_eq!(v[1].when, "!(is_dark())");
        assert_eq!(t.colors("brand()").len(), 1);
        assert_eq!(t.colors("theme::brand().opacity(0.5)")[0].color.alpha_or_one(), 0.5);
        assert!(t.colors("tint(1)").is_empty());
        assert_eq!(t.string("theme::SANS").as_deref(), Some("Inter"));
        assert_eq!(t.string("\"Lilex\"").as_deref(), Some("Lilex"));
    }
}
