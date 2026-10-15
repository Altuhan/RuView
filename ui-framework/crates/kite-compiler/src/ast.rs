//! Syntax tree of a `.ui` file.

use crate::diagnostics::Span;
use crate::lexer::StrPart;

#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub imports: Vec<Import>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub names: Vec<Ident>,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub name: Ident,
    pub exported: bool,
    /// The component's root element; its `base` is the `inherits` type (or `Empty`).
    pub root: Element,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    In,
    Out,
    InOut,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDecl {
    pub visibility: Visibility,
    pub ty: Ident,
    pub name: Ident,
    pub init: Option<Binding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallbackDecl {
    pub name: Ident,
    pub params: Vec<Ident>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    Expr(Expr),
    TwoWay(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Handler {
    pub name: Ident,
    pub params: Vec<Ident>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    Element(Element),
    /// `@children`: where an instance's children are inserted.
    Placeholder(Span),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub id: Option<Ident>,
    pub base: Option<Ident>,
    pub span: Span,
    pub properties: Vec<PropertyDecl>,
    pub callbacks: Vec<CallbackDecl>,
    pub bindings: Vec<(Ident, Binding)>,
    pub handlers: Vec<Handler>,
    pub children: Vec<Child>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(f64, bool, Option<String>),
    Str(Vec<StrPart>),
    Color(String),
    Bool(bool),
    Ident(String),
    Member(Box<Expr>, Ident),
    Call(Box<Expr>, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Placeholder produced after a parse error.
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `target op value;` where op is `=`, `+=`, `-=`, `*=` or `/=`.
    Assign { target: Expr, op: &'static str, value: Expr },
    Expr(Expr),
    If { cond: Expr, then: Vec<Stmt>, otherwise: Vec<Stmt> },
}
