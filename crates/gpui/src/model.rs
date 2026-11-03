//! Builder chains: `div().flex().bg(..).child(div()...)`. Each chain is one
//! element as the source spells it, with the chain it was passed into as its
//! parent. Styles set inside closures (`.when(..)`, `.hover(..)`) are not
//! base styles and are left out, so conditional state never counts.

use crate::lex::{matching, Tok, Token};

#[derive(Debug, Clone)]
pub struct Call {
    pub name: String,
    pub args: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Chain {
    #[cfg_attr(not(test), allow(dead_code))]
    pub root: String,
    pub line: usize,
    pub calls: Vec<Call>,
    pub parent: Option<usize>,
}

impl Chain {
    pub fn call(&self, name: &str) -> Option<&Call> {
        self.calls.iter().find(|c| c.name == name)
    }
    #[cfg(test)]
    pub fn has(&self, name: &str) -> bool {
        self.call(name).is_some()
    }
}

const NOT_ROOTS: &[&str] = &["if", "while", "match", "for", "return", "fn", "loop", "in", "as", "move"];

pub fn chains(tokens: &[Token], src: &str) -> Vec<Chain> {
    let mut out = Vec::new();
    scan(tokens, src, 0, tokens.len(), None, &mut out);
    out
}

fn text(tokens: &[Token], src: &str, from: usize, to: usize) -> String {
    if from >= to || to > tokens.len() {
        return String::new();
    }
    src[tokens[from].start..tokens[to - 1].end].to_string()
}

fn scan(tokens: &[Token], src: &str, from: usize, to: usize, parent: Option<usize>, out: &mut Vec<Chain>) {
    let mut i = from;
    while i < to {
        let t = &tokens[i];
        let is_root = matches!(&t.tok, Tok::Ident(name) if !NOT_ROOTS.contains(&name.as_str()))
            && tokens.get(i + 1).is_some_and(|n| n.is_punct('('))
            && !(i > 0 && tokens[i - 1].is_punct('.'))
            && !(i > 0 && tokens[i - 1].ident() == Some("fn"));
        if !is_root {
            i += 1;
            continue;
        }
        let close = matching(tokens, i + 1).min(to);
        let idx = out.len();
        out.push(Chain { root: t.ident().unwrap_or("").to_string(), line: t.line, calls: Vec::new(), parent });
        scan(tokens, src, i + 2, close, parent, out);
        let mut k = close + 1;
        while k + 2 < to
            && tokens.get(k).is_some_and(|d| d.is_punct('.'))
            && tokens.get(k + 1).and_then(|n| n.ident()).is_some()
            && tokens.get(k + 2).is_some_and(|p| p.is_punct('('))
        {
            let name = tokens[k + 1].ident().unwrap().to_string();
            let line = tokens[k + 1].line;
            let open = k + 2;
            let end = matching(tokens, open).min(to);
            let args = text(tokens, src, open + 1, end);
            out[idx].calls.push(Call { name, args, line });
            scan(tokens, src, open + 1, end, Some(idx), out);
            k = end + 1;
        }
        i = k.max(i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lex;

    #[test]
    fn nests_children() {
        let src = "fn render() -> impl IntoElement { div().p_2().child(div().bg(rgb(0xff0000)).child(\"x\")).when(a, |this| this.bg(red())) }";
        let l = lex(src);
        let cs = chains(&l.tokens, &l.code);
        let outer = cs.iter().position(|c| c.root == "div" && c.has("p_2")).unwrap();
        let inner = cs.iter().position(|c| c.root == "div" && c.has("bg")).unwrap();
        assert_eq!(cs[inner].parent, Some(outer));
        // `this.bg(..)` inside the closure is not a base style of the outer div.
        assert!(!cs[outer].has("bg"));
        assert!(cs[outer].has("when"));
    }
}
