//! A small Rust tokenizer: enough to follow GPUI builder chains without a
//! full parser. Comments are dropped, string and char literals stay opaque.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Punct(char),
    Str(String),
    Num(String),
    Char,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

impl Token {
    pub fn is_punct(&self, c: char) -> bool {
        self.tok == Tok::Punct(c)
    }
    pub fn ident(&self) -> Option<&str> {
        match &self.tok {
            Tok::Ident(s) => Some(s),
            _ => None,
        }
    }
}

/// Tokens plus a copy of the source with every comment blanked to spaces
/// (newlines kept), so byte offsets and line numbers still line up.
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub code: String,
}

pub fn lex(src: &str) -> Lexed {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut code = bytes.to_vec();
    let mut i = 0;
    let mut line = 1;
    let blank = |code: &mut Vec<u8>, from: usize, to: usize| {
        for b in code.iter_mut().take(to).skip(from) {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    let count_lines = |from: usize, to: usize| bytes[from..to].iter().filter(|b| **b == b'\n').count();

    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // Comments.
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let end = bytes[i..].iter().position(|b| *b == b'\n').map(|p| i + p).unwrap_or(bytes.len());
            blank(&mut code, i, end);
            i = end;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            let mut depth = 0;
            while i < bytes.len() {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            line += count_lines(start, i.min(bytes.len()));
            blank(&mut code, start, i.min(bytes.len()));
            continue;
        }
        // Raw and byte strings: r"..", r#".."#, b"..", br#".."#.
        if (c == b'r' || c == b'b') && is_raw_or_byte_string(bytes, i) {
            let start = i;
            let mut j = i;
            if bytes[j] == b'b' {
                j += 1;
            }
            let raw = bytes.get(j) == Some(&b'r');
            if raw {
                j += 1;
            }
            let mut hashes = 0;
            while bytes.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            j += 1; // opening quote
            let body_start = j;
            let body_end;
            if raw {
                loop {
                    if j >= bytes.len() {
                        body_end = bytes.len();
                        break;
                    }
                    if bytes[j] == b'"' && bytes[j + 1..].iter().take(hashes).filter(|b| **b == b'#').count() == hashes {
                        body_end = j;
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
            } else {
                let (e, next) = scan_quoted(bytes, j);
                body_end = e;
                j = next;
            }
            tokens.push(Token { tok: Tok::Str(src[body_start..body_end].to_string()), start, end: j, line });
            line += count_lines(start, j.min(bytes.len()));
            i = j;
            continue;
        }
        if c == b'"' {
            let start = i;
            let (body_end, next) = scan_quoted(bytes, i + 1);
            tokens.push(Token { tok: Tok::Str(src[i + 1..body_end].to_string()), start, end: next, line });
            line += count_lines(start, next.min(bytes.len()));
            i = next;
            continue;
        }
        if c == b'\'' {
            // Char literal or lifetime.
            if bytes.get(i + 1) == Some(&b'\\') {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j] != b'\'' {
                    j += 1;
                }
                tokens.push(Token { tok: Tok::Char, start: i, end: j + 1, line });
                i = j + 1;
                continue;
            }
            let ch_len = src[i + 1..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            if bytes.get(i + 1 + ch_len) == Some(&b'\'') {
                tokens.push(Token { tok: Tok::Char, start: i, end: i + 2 + ch_len, line });
                i += 2 + ch_len;
                continue;
            }
            tokens.push(Token { tok: Tok::Punct('\''), start: i, end: i + 1, line });
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_alphanumeric() || b == b'_' {
                    i += 1;
                } else if b == b'.' {
                    // `1.0`, `1.` are numbers; `1..2` and `1.max(..)` are not.
                    let next = bytes.get(i + 1).copied().unwrap_or(b' ');
                    if next == b'.' || next.is_ascii_alphabetic() || next == b'_' {
                        break;
                    }
                    i += 1;
                } else {
                    break;
                }
            }
            tokens.push(Token { tok: Tok::Num(src[start..i].to_string()), start, end: i, line });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) {
                i += 1;
            }
            tokens.push(Token { tok: Tok::Ident(src[start..i].to_string()), start, end: i, line });
            continue;
        }
        tokens.push(Token { tok: Tok::Punct(c as char), start: i, end: i + 1, line });
        i += 1;
    }
    Lexed { tokens, code: String::from_utf8(code).unwrap_or_else(|_| src.to_string()) }
}

fn is_raw_or_byte_string(bytes: &[u8], i: usize) -> bool {
    // Must not be the tail of a longer identifier.
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        return false;
    }
    let mut j = i;
    if bytes[j] == b'b' {
        j += 1;
    }
    if bytes.get(j) == Some(&b'r') {
        j += 1;
        while bytes.get(j) == Some(&b'#') {
            j += 1;
        }
        return bytes.get(j) == Some(&b'"');
    }
    j > i && bytes.get(j) == Some(&b'"')
}

/// From just after an opening quote: (end of body, index after closing quote).
fn scan_quoted(bytes: &[u8], mut j: usize) -> (usize, usize) {
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 2,
            b'"' => return (j, j + 1),
            _ => j += 1,
        }
    }
    (bytes.len(), bytes.len())
}

/// Index of the token closing the group opened at `open` (`(`, `[` or `{`).
pub fn matching(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0i32;
    for (k, t) in tokens.iter().enumerate().skip(open) {
        if let Tok::Punct(c) = t.tok {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return k;
                    }
                }
                _ => {}
            }
        }
    }
    tokens.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_keeps_lines() {
        let l = lex("a // x(\n/* y( */ b(\"c)\")");
        let idents: Vec<_> = l.tokens.iter().filter_map(|t| t.ident()).collect();
        assert_eq!(idents, vec!["a", "b"]);
        assert_eq!(l.tokens.last().unwrap().line, 2);
        assert!(!l.code.contains('x'));
    }

    #[test]
    fn numbers_and_lifetimes() {
        let l = lex("px(1.) 0..2 'a 'b' r#\"q\"#");
        let toks: Vec<_> = l.tokens.iter().map(|t| t.tok.clone()).collect();
        assert!(toks.contains(&Tok::Num("1.".into())));
        assert!(toks.contains(&Tok::Num("0".into())));
        assert!(toks.contains(&Tok::Char));
        assert!(toks.contains(&Tok::Str("q".into())));
    }
}
