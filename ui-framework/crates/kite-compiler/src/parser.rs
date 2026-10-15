//! Recursive-descent parser.
//!
//! ```text
//! file      := (import | component)*
//! import    := 'import' '{' Ident (',' Ident)* '}' 'from' String ';'
//! component := ['export'] 'component' Ident ['inherits' Ident] '{' body '}'
//! body      := ( property | callback | binding | two-way | handler | element | '@children' )*
//! property  := [in|out|in-out|private] 'property' '<' type '>' Ident [':' expr | '<=>' expr] ';'
//! callback  := 'callback' Ident ['(' types ')'] ';'
//! binding   := Ident ':' expr ';'          two-way := Ident '<=>' expr ';'
//! handler   := Ident ['(' idents ')'] '=>' block
//! element   := [Ident ':='] Ident '{' body '}'
//! ```

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Level, Span};
use crate::lexer::{lex, Tok, Token};

pub struct Parser<'d> {
    toks: Vec<Token>,
    pos: usize,
    diags: &'d mut Vec<Diagnostic>,
}

pub fn parse_file(src: &str, file: u32, diags: &mut Vec<Diagnostic>) -> File {
    let toks = lex(src, file, 0, diags);
    let mut p = Parser {
        toks,
        pos: 0,
        diags,
    };
    p.file()
}

/// Parses an expression embedded in a string (`\{...}`) starting at `offset`.
pub fn parse_embedded_expr(
    src: &str,
    file: u32,
    offset: usize,
    diags: &mut Vec<Diagnostic>,
) -> Expr {
    let toks = lex(src, file, offset, diags);
    let mut p = Parser {
        toks,
        pos: 0,
        diags,
    };
    let e = p.expr();
    if !matches!(p.peek(), Tok::Eof) {
        p.error_here("unexpected tokens after interpolated expression");
    }
    e
}

type PResult<T> = Result<T, ()>;

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == k)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn describe(t: &Tok) -> String {
        match t {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Number(..) => "a number".into(),
            Tok::Str(_) => "a string".into(),
            Tok::Color(c) => format!("`{c}`"),
            Tok::Punct(p) => format!("`{p}`"),
            Tok::Eof => "end of file".into(),
        }
    }

    fn error_here(&mut self, msg: &str) {
        let span = self.span();
        self.diags.push(Diagnostic {
            level: Level::Error,
            message: msg.into(),
            span,
            label: None,
        });
    }

    fn expect(&mut self, p: &str) -> PResult<Span> {
        if self.is_punct(p) {
            Ok(self.bump().span)
        } else {
            let found = Self::describe(self.peek());
            self.error_here(&format!("expected `{p}`, found {found}"));
            Err(())
        }
    }

    fn ident(&mut self, what: &str) -> PResult<Ident> {
        match self.peek().clone() {
            Tok::Ident(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            t => {
                let found = Self::describe(&t);
                self.error_here(&format!("expected {what}, found {found}"));
                Err(())
            }
        }
    }

    /// Skips to just after the next `;` or `}` at the current nesting level.
    fn recover(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Punct("{") => depth += 1,
                Tok::Punct("}") => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                    if depth == 0 {
                        self.bump();
                        return;
                    }
                }
                Tok::Punct(";") if depth == 0 => {
                    self.bump();
                    return;
                }
                _ => {}
            }
            self.bump();
        }
    }

    fn file(&mut self) -> File {
        let mut file = File {
            imports: Vec::new(),
            components: Vec::new(),
        };
        while !matches!(self.peek(), Tok::Eof) {
            let r = if self.is_kw("import") {
                self.import().map(|i| file.imports.push(i))
            } else if self.is_kw("export") || self.is_kw("component") {
                self.component().map(|c| file.components.push(c))
            } else {
                let found = Self::describe(self.peek());
                self.error_here(&format!("expected `component` or `import`, found {found}"));
                Err(())
            };
            if r.is_err() {
                // Skip to the next top-level item.
                while !matches!(self.peek(), Tok::Eof)
                    && !self.is_kw("component")
                    && !self.is_kw("export")
                    && !self.is_kw("import")
                {
                    self.bump();
                }
            }
        }
        file
    }

    fn import(&mut self) -> PResult<Import> {
        let start = self.bump().span;
        self.expect("{")?;
        let mut names = Vec::new();
        while !self.is_punct("}") {
            names.push(self.ident("component name")?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect("}")?;
        if !self.is_kw("from") {
            self.error_here("expected `from`");
            return Err(());
        }
        self.bump();
        let path = match self.peek().clone() {
            Tok::Str(parts) => {
                self.bump();
                parts
                    .into_iter()
                    .map(|p| match p {
                        crate::lexer::StrPart::Lit(s) => s,
                        crate::lexer::StrPart::Code(..) => String::new(),
                    })
                    .collect()
            }
            _ => {
                self.error_here("expected a file path string");
                return Err(());
            }
        };
        self.expect(";")?;
        Ok(Import {
            names,
            path,
            span: start.to(self.prev_span()),
        })
    }

    fn component(&mut self) -> PResult<Component> {
        let exported = if self.is_kw("export") {
            self.bump();
            true
        } else {
            false
        };
        if !self.is_kw("component") {
            self.error_here("expected `component`");
            return Err(());
        }
        let start = self.bump().span;
        let name = self.ident("component name")?;
        let base = if self.is_kw("inherits") {
            self.bump();
            Some(self.ident("base type")?)
        } else {
            None
        };
        let mut root = Element {
            base,
            span: start.to(name.span),
            ..Default::default()
        };
        self.expect("{")?;
        self.body(&mut root);
        self.expect("}")?;
        Ok(Component {
            name,
            exported,
            root,
        })
    }

    fn body(&mut self, el: &mut Element) {
        while !self.is_punct("}") && !matches!(self.peek(), Tok::Eof) {
            let before = self.pos;
            if self.member(el).is_err() {
                self.recover();
            }
            if self.pos == before {
                self.bump();
            }
        }
    }

    fn visibility(&self) -> Option<Visibility> {
        match self.peek() {
            Tok::Ident(s) if s == "in" => Some(Visibility::In),
            Tok::Ident(s) if s == "out" => Some(Visibility::Out),
            Tok::Ident(s) if s == "in-out" => Some(Visibility::InOut),
            Tok::Ident(s) if s == "private" => Some(Visibility::Private),
            _ => None,
        }
    }

    fn member(&mut self, el: &mut Element) -> PResult<()> {
        if self.eat("@") {
            let id = self.ident("`children`")?;
            if id.name != "children" {
                self.diags.push(Diagnostic {
                    level: Level::Error,
                    message: format!("unknown `@{}`; only `@children` is supported", id.name),
                    span: id.span,
                    label: None,
                });
            }
            el.children.push(Child::Placeholder(id.span));
            return Ok(());
        }
        let vis = self.visibility();
        let is_prop_kw = |t: &Tok| matches!(t, Tok::Ident(s) if s == "property");
        if (vis.is_some() && is_prop_kw(self.peek_at(1)))
            || (vis.is_none()
                && is_prop_kw(self.peek())
                && matches!(self.peek_at(1), Tok::Punct("<")))
        {
            if vis.is_some() {
                self.bump();
            }
            self.bump();
            self.expect("<")?;
            let ty = self.ident("type name")?;
            self.expect(">")?;
            let name = self.ident("property name")?;
            let init = if self.eat(":") {
                Some(Binding::Expr(self.expr()))
            } else if self.eat("<=>") {
                Some(Binding::TwoWay(self.expr()))
            } else {
                None
            };
            self.expect(";")?;
            el.properties.push(PropertyDecl {
                visibility: vis.unwrap_or(Visibility::Private),
                ty,
                name,
                init,
            });
            return Ok(());
        }
        if self.is_kw("callback") && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.bump();
            let name = self.ident("callback name")?;
            let mut params = Vec::new();
            if self.eat("(") {
                while !self.is_punct(")") {
                    params.push(self.ident("parameter type")?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(")")?;
            }
            self.expect(";")?;
            el.callbacks.push(CallbackDecl { name, params });
            return Ok(());
        }
        let first = self.ident("a property, callback or element")?;
        match self.peek() {
            Tok::Punct(":=") => {
                self.bump();
                let base = self.ident("element type")?;
                let child = self.element(Some(first), base)?;
                el.children.push(Child::Element(Box::new(child)));
            }
            Tok::Punct("{") => {
                let child = self.element(None, first)?;
                el.children.push(Child::Element(Box::new(child)));
            }
            Tok::Punct(":") => {
                self.bump();
                let e = self.expr();
                // Allow `prop: { ... }`-less forms only; require the semicolon.
                self.expect(";")?;
                el.bindings.push((first, Binding::Expr(e)));
            }
            Tok::Punct("<=>") => {
                self.bump();
                let e = self.expr();
                self.expect(";")?;
                el.bindings.push((first, Binding::TwoWay(e)));
            }
            Tok::Punct("=>") | Tok::Punct("(") => {
                let mut params = Vec::new();
                if self.eat("(") {
                    while !self.is_punct(")") {
                        params.push(self.ident("parameter name")?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                self.expect("=>")?;
                let body = self.block()?;
                el.handlers.push(Handler {
                    name: first,
                    params,
                    body,
                });
            }
            t => {
                let found = Self::describe(t);
                self.error_here(&format!(
                    "expected `:`, `<=>`, `=>` or `{{` after `{}`, found {found}",
                    first.name
                ));
                return Err(());
            }
        }
        Ok(())
    }

    fn element(&mut self, id: Option<Ident>, base: Ident) -> PResult<Element> {
        let start = id.as_ref().map(|i| i.span).unwrap_or(base.span);
        let mut el = Element {
            id,
            base: Some(base),
            span: start,
            ..Default::default()
        };
        self.expect("{")?;
        self.body(&mut el);
        let end = self.expect("}")?;
        el.span = start.to(end);
        Ok(el)
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect("{")?;
        let mut stmts = Vec::new();
        while !self.is_punct("}") && !matches!(self.peek(), Tok::Eof) {
            let before = self.pos;
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(()) => self.recover(),
            }
            if self.pos == before {
                self.bump();
            }
        }
        self.expect("}")?;
        Ok(stmts)
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        if self.is_kw("if") {
            self.bump();
            let cond = self.expr();
            let then = self.block()?;
            let otherwise = if self.is_kw("else") {
                self.bump();
                if self.is_kw("if") {
                    vec![self.stmt()?]
                } else {
                    self.block()?
                }
            } else {
                Vec::new()
            };
            return Ok(Stmt::If {
                cond,
                then,
                otherwise,
            });
        }
        let target = self.expr();
        for op in ["=", "+=", "-=", "*=", "/="] {
            if self.is_punct(op) {
                let op = match self.bump().tok {
                    Tok::Punct(p) => p,
                    _ => unreachable!(),
                };
                let value = self.expr();
                self.expect(";")?;
                return Ok(Stmt::Assign { target, op, value });
            }
        }
        self.expect(";")?;
        Ok(Stmt::Expr(target))
    }

    pub fn expr(&mut self) -> Expr {
        self.ternary()
    }

    fn ternary(&mut self) -> Expr {
        let cond = self.binary(0);
        if self.eat("?") {
            let a = self.ternary();
            if self.expect(":").is_err() {
                return Expr {
                    kind: ExprKind::Error,
                    span: cond.span,
                };
            }
            let b = self.ternary();
            let span = cond.span.to(b.span);
            return Expr {
                kind: ExprKind::Ternary(Box::new(cond), Box::new(a), Box::new(b)),
                span,
            };
        }
        cond
    }

    fn binary(&mut self, min_prec: u8) -> Expr {
        const OPS: [(&str, u8); 13] = [
            ("||", 1),
            ("&&", 2),
            ("==", 3),
            ("!=", 3),
            ("<", 4),
            ("<=", 4),
            (">", 4),
            (">=", 4),
            ("+", 5),
            ("-", 5),
            ("*", 6),
            ("/", 6),
            ("%", 6),
        ];
        let mut lhs = self.unary();
        loop {
            let op = match self.peek() {
                Tok::Punct(p) => OPS.iter().find(|(o, _)| o == p).copied(),
                _ => None,
            };
            let Some((op, prec)) = op else { break };
            if prec < min_prec || prec == 0 {
                break;
            }
            self.bump();
            let rhs = self.binary(prec + 1);
            let span = lhs.span.to(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        lhs
    }

    fn unary(&mut self) -> Expr {
        for op in ["-", "!", "+"] {
            if self.is_punct(op) {
                let start = self.bump().span;
                let e = self.unary();
                let span = start.to(e.span);
                if op == "+" {
                    return e;
                }
                let op = if op == "-" { "-" } else { "!" };
                return Expr {
                    kind: ExprKind::Unary(op, Box::new(e)),
                    span,
                };
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Expr {
        let mut e = self.primary();
        loop {
            if self.eat(".") {
                match self.ident("member name") {
                    Ok(id) => {
                        let span = e.span.to(id.span);
                        e = Expr {
                            kind: ExprKind::Member(Box::new(e), id),
                            span,
                        };
                    }
                    Err(()) => {
                        return Expr {
                            kind: ExprKind::Error,
                            span: e.span,
                        }
                    }
                }
            } else if self.is_punct("(") {
                self.bump();
                let mut args = Vec::new();
                while !self.is_punct(")") && !matches!(self.peek(), Tok::Eof) {
                    args.push(self.expr());
                    if !self.eat(",") {
                        break;
                    }
                }
                let end = self.expect(")").unwrap_or(e.span);
                let span = e.span.to(end);
                e = Expr {
                    kind: ExprKind::Call(Box::new(e), args),
                    span,
                };
            } else {
                break;
            }
        }
        e
    }

    fn primary(&mut self) -> Expr {
        let t = self.bump();
        let kind = match t.tok {
            Tok::Number(v, dot, unit) => ExprKind::Number(v, dot, unit),
            Tok::Str(parts) => ExprKind::Str(parts),
            Tok::Color(c) => ExprKind::Color(c),
            Tok::Ident(s) if s == "true" => ExprKind::Bool(true),
            Tok::Ident(s) if s == "false" => ExprKind::Bool(false),
            Tok::Ident(s) => ExprKind::Ident(s),
            Tok::Punct("(") => {
                let e = self.expr();
                let end = self.expect(")").unwrap_or(e.span);
                return Expr {
                    kind: e.kind,
                    span: t.span.to(end),
                };
            }
            other => {
                let found = Self::describe(&other);
                self.diags.push(Diagnostic {
                    level: Level::Error,
                    message: format!("expected an expression, found {found}"),
                    span: t.span,
                    label: None,
                });
                // Don't consume structural tokens so recovery can see them.
                if matches!(other, Tok::Punct(";") | Tok::Punct("}") | Tok::Eof) {
                    self.pos = self.pos.saturating_sub(1);
                }
                ExprKind::Error
            }
        };
        Expr { kind, span: t.span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> (File, Vec<Diagnostic>) {
        let mut d = Vec::new();
        let f = parse_file(src, 0, &mut d);
        (f, d)
    }

    #[test]
    fn parses_component_tree() {
        let (f, d) = parse(
            r#"
            import { Card } from "card.ui";
            export component App inherits Window {
                in-out property <int> counter: 0;
                callback clicked(int);
                title: "Hi";
                width <=> root.w;
                VerticalLayout {
                    label := Text { text: "n = \{counter}"; }
                    Button {
                        clicked => { counter += 1; if counter > 3 { counter = 0; } else { root.clicked(counter); } }
                    }
                    @children
                }
            }
        "#,
        );
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(f.imports.len(), 1);
        let c = &f.components[0];
        assert!(c.exported);
        assert_eq!(c.root.base.as_ref().unwrap().name, "Window");
        assert_eq!(c.root.properties[0].visibility, Visibility::InOut);
        assert_eq!(c.root.callbacks[0].params.len(), 1);
        assert_eq!(c.root.bindings.len(), 2);
        let Child::Element(layout) = &c.root.children[0] else {
            panic!()
        };
        assert_eq!(layout.children.len(), 3);
    }

    #[test]
    fn precedence() {
        let mut d = Vec::new();
        let e = parse_embedded_expr("1 + 2 * 3 == 7 && !b ? x : y", 0, 0, &mut d);
        assert!(d.is_empty());
        let ExprKind::Ternary(c, _, _) = e.kind else {
            panic!()
        };
        let ExprKind::Binary("&&", l, _) = c.kind else {
            panic!()
        };
        let ExprKind::Binary("==", l, _) = l.kind else {
            panic!()
        };
        assert!(matches!(l.kind, ExprKind::Binary("+", _, _)));
    }

    #[test]
    fn recovers_from_errors() {
        let (f, d) = parse("component A { width: ; height: 10px; Text { text: \"ok\"; } }");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(f.components[0].root.children.len(), 1);
    }
}
