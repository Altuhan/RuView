//! Source spans and human-readable error reporting.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Span {
    pub file: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: u32, start: usize, end: usize) -> Self {
        Self { file, start: start as u32, end: end as u32 }
    }

    pub fn to(self, other: Span) -> Span {
        Span { file: self.file, start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub span: Span,
    /// Short note printed under the caret, e.g. a suggestion.
    pub label: Option<String>,
}

/// The source files of one compilation, for rendering diagnostics.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    pub files: Vec<(PathBuf, String)>,
}

impl SourceMap {
    pub fn add(&mut self, path: impl Into<PathBuf>, src: String) -> u32 {
        self.files.push((path.into(), src));
        (self.files.len() - 1) as u32
    }

    pub fn source(&self, file: u32) -> &str {
        &self.files[file as usize].1
    }

    /// 1-based line and column of a byte offset.
    pub fn line_col(&self, file: u32, offset: u32) -> (usize, usize) {
        let src = self.source(file);
        let offset = (offset as usize).min(src.len());
        let before = &src[..offset];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
        (line, col)
    }
}

/// A list of diagnostics plus the sources needed to print them.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
    pub sources: SourceMap,
}

impl Diagnostics {
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.level == Level::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.level == Level::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.level == Level::Warning)
    }

    /// Renders one diagnostic in a rustc-like format.
    pub fn render(&self, d: &Diagnostic) -> String {
        let mut out = String::new();
        let level = match d.level {
            Level::Error => "error",
            Level::Warning => "warning",
        };
        out.push_str(&format!("{level}: {}\n", d.message));
        if (d.span.file as usize) < self.sources.files.len() {
            let (line, col) = self.sources.line_col(d.span.file, d.span.start);
            let path = self.sources.files[d.span.file as usize].0.display();
            let gutter = line.to_string().len();
            let text = self.sources.source(d.span.file).lines().nth(line - 1).unwrap_or("");
            let span_len = (d.span.end.saturating_sub(d.span.start) as usize).max(1);
            let width = span_len.min(text.chars().count().saturating_sub(col - 1).max(1));
            out.push_str(&format!("{:gutter$}--> {path}:{line}:{col}\n", ""));
            out.push_str(&format!("{:gutter$} |\n", ""));
            out.push_str(&format!("{line} | {text}\n"));
            out.push_str(&format!("{:gutter$} | {:pad$}{}", "", "", "^".repeat(width), pad = col - 1));
            if let Some(label) = &d.label {
                out.push(' ');
                out.push_str(label);
            }
            out.push('\n');
        }
        out
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for d in &self.items {
            writeln!(f, "{}", self.render(d))?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostics {}

/// Closest candidate by edit distance, for "did you mean" hints.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = edit_distance(name, c);
        if d <= (name.len().max(3) / 3).max(1) && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_names() {
        assert_eq!(suggest("colour", ["color", "text", "font-size"]), Some("color"));
        assert_eq!(suggest("zzz", ["color", "text"]), None);
    }
}
