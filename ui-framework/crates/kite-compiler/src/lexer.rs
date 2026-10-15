//! Tokenizer for `.ui` source.
//!
//! Identifiers may contain `-` (as in `border-radius`), so subtraction needs
//! whitespace: `a - b`. Strings support `\{expr}` interpolation.

use crate::diagnostics::{Diagnostic, Level, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    /// Source of an interpolated expression and its byte offset in the file.
    Code(String, usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// Numeric literal: value, whether it had a decimal point, optional unit.
    Number(f64, bool, Option<String>),
    Str(Vec<StrPart>),
    Color(String),
    Punct(&'static str),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

const PUNCTS: [&str; 33] = [
    "<=>", ":=", "=>", "->", "<=", ">=", "==", "!=", "+=", "-=", "*=", "/=", "&&", "||", "{", "}", "(", ")",
    "[", "]", ";", ":", ",", ".", "<", ">", "=", "+", "-", "*", "/", "%", "!",
];
const PUNCTS_EXTRA: [&str; 2] = ["?", "@"];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn lex(src: &str, file: u32, base: usize, diags: &mut Vec<Diagnostic>) -> Vec<Token> {
    let bytes: Vec<char> = src.chars().collect();
    // Byte offset of each char index (plus one past the end).
    let mut offs = Vec::with_capacity(bytes.len() + 1);
    let mut o = 0;
    for c in &bytes {
        offs.push(o);
        o += c.len_utf8();
    }
    offs.push(o);
    let span = |a: usize, b: usize| Span::new(file, base + offs[a], base + offs[b]);
    let mut out = Vec::new();
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        let c = bytes[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < n && bytes[i + 1] == '/' {
            while i < n && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < n && bytes[i + 1] == '*' {
            let start = i;
            i += 2;
            while i + 1 < n && !(bytes[i] == '*' && bytes[i + 1] == '/') {
                i += 1;
            }
            if i + 1 >= n {
                diags.push(err(span(start, n), "unterminated block comment"));
                i = n;
            } else {
                i += 2;
            }
            continue;
        }
        let start = i;
        if is_ident_start(c) {
            i += 1;
            while i < n
                && (is_ident_char(bytes[i])
                    || (bytes[i] == '-' && i + 1 < n && is_ident_start(bytes[i + 1])))
            {
                i += 1;
            }
            let s: String = bytes[start..i].iter().collect();
            out.push(Token { tok: Tok::Ident(s), span: span(start, i) });
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < n && bytes[i + 1].is_ascii_digit()) {
            let mut dot = false;
            while i < n && (bytes[i].is_ascii_digit() || (bytes[i] == '.' && !dot && i + 1 < n && bytes[i + 1].is_ascii_digit())) {
                if bytes[i] == '.' {
                    dot = true;
                }
                i += 1;
            }
            let text: String = bytes[start..i].iter().collect();
            let unit_start = i;
            if i < n && bytes[i] == '%' {
                i += 1;
            } else {
                while i < n && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
            }
            let unit: String = bytes[unit_start..i].iter().collect();
            let value = text.parse::<f64>().unwrap_or(0.0);
            let unit = (!unit.is_empty()).then_some(unit);
            out.push(Token { tok: Tok::Number(value, dot, unit), span: span(start, i) });
            continue;
        }
        if c == '#' {
            i += 1;
            while i < n && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let s: String = bytes[start..i].iter().collect();
            out.push(Token { tok: Tok::Color(s), span: span(start, i) });
            continue;
        }
        if c == '"' {
            i += 1;
            let mut parts = Vec::new();
            let mut lit = String::new();
            let mut closed = false;
            while i < n {
                match bytes[i] {
                    '"' => {
                        i += 1;
                        closed = true;
                        break;
                    }
                    '\\' if i + 1 < n => {
                        let e = bytes[i + 1];
                        i += 2;
                        match e {
                            'n' => lit.push('\n'),
                            't' => lit.push('\t'),
                            '"' => lit.push('"'),
                            '\\' => lit.push('\\'),
                            '{' => {
                                // Interpolation: find the matching '}'.
                                let code_start = i;
                                let mut depth = 1;
                                while i < n && depth > 0 {
                                    match bytes[i] {
                                        '{' => depth += 1,
                                        '}' => depth -= 1,
                                        _ => {}
                                    }
                                    i += 1;
                                }
                                if depth != 0 {
                                    diags.push(err(span(start, n), "unterminated `\\{` in string"));
                                    break;
                                }
                                if !lit.is_empty() {
                                    parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                                }
                                let code: String = bytes[code_start..i - 1].iter().collect();
                                parts.push(StrPart::Code(code, base + offs[code_start]));
                            }
                            other => {
                                diags.push(err(span(i - 2, i), &format!("unknown escape `\\{other}`")));
                            }
                        }
                    }
                    ch => {
                        lit.push(ch);
                        i += 1;
                    }
                }
            }
            if !closed {
                diags.push(err(span(start, i.min(n)), "unterminated string literal"));
            }
            if !lit.is_empty() || parts.is_empty() {
                parts.push(StrPart::Lit(lit));
            }
            out.push(Token { tok: Tok::Str(parts), span: span(start, i.min(n)) });
            continue;
        }
        let rest: String = bytes[i..(i + 3).min(n)].iter().collect();
        if let Some(p) = PUNCTS.iter().chain(PUNCTS_EXTRA.iter()).find(|p| rest.starts_with(**p)) {
            let len = p.chars().count();
            out.push(Token { tok: Tok::Punct(p), span: span(i, i + len) });
            i += len;
            continue;
        }
        diags.push(err(span(i, i + 1), &format!("unexpected character `{c}`")));
        i += 1;
    }
    out.push(Token { tok: Tok::Eof, span: span(n, n) });
    out
}

fn err(span: Span, msg: &str) -> Diagnostic {
    Diagnostic { level: Level::Error, message: msg.into(), span, label: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        let mut d = Vec::new();
        let t = lex(s, 0, 0, &mut d);
        assert!(d.is_empty(), "{d:?}");
        t.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn hyphenated_identifiers_and_minus() {
        assert_eq!(
            toks("border-radius: a - b-c;"),
            vec![
                Tok::Ident("border-radius".into()),
                Tok::Punct(":"),
                Tok::Ident("a".into()),
                Tok::Punct("-"),
                Tok::Ident("b-c".into()),
                Tok::Punct(";"),
                Tok::Eof
            ]
        );
        assert_eq!(toks("x-1")[1], Tok::Punct("-"));
    }

    #[test]
    fn numbers_units_colors() {
        assert_eq!(toks("10px")[0], Tok::Number(10.0, false, Some("px".into())));
        assert_eq!(toks("50%")[0], Tok::Number(50.0, false, Some("%".into())));
        assert_eq!(toks("1.5")[0], Tok::Number(1.5, true, None));
        assert_eq!(toks("#ff00aa")[0], Tok::Color("#ff00aa".into()));
        assert_eq!(toks("a <=> b")[1], Tok::Punct("<=>"));
    }

    #[test]
    fn string_interpolation() {
        let t = toks(r#""Count: \{n + 1}!""#);
        assert_eq!(
            t[0],
            Tok::Str(vec![
                StrPart::Lit("Count: ".into()),
                StrPart::Code("n + 1".into(), 10),
                StrPart::Lit("!".into())
            ])
        );
    }
}
