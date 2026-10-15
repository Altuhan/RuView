//! Semantic analysis: expands components into a flat element list, resolves
//! names, type-checks bindings and handlers, and produces the IR.

use std::collections::HashMap;
use std::rc::Rc;

use kite_core::elements::{self as el, ElementKind};
use kite_core::ir::{
    Access, BinaryOp, BuiltinFn, CallbackDef, CallbackRef, ComponentDef, ElementDef, Expr, PropRef,
    PropertyDef, PropertyInit, PublicCallback, PublicProperty, Stmt, UnaryOp,
};
use kite_core::{Color, EnumType, Type, Value};

use crate::ast::{self, Binding, Child, ExprKind, Visibility};
use crate::diagnostics::{suggest, Diagnostic, Level, Span};
use crate::lexer::StrPart;
use crate::parser::parse_embedded_expr;

/// Components visible to the compilation, by name.
pub type Registry = HashMap<String, Rc<ast::Component>>;

struct ElemInfo {
    def: ElementDef,
    parent: Option<u32>,
    /// Scope in which the element was written (where its id lives).
    scope: u32,
    /// Declared (non built-in) properties: index -> (visibility, declaring scope).
    declared: HashMap<u32, (Visibility, u32)>,
    /// Declared callbacks: index -> declaring scope.
    declared_callbacks: HashMap<u32, u32>,
    /// Bindings/handlers already applied: (is_handler, index) -> (scope, span).
    bound: HashMap<(bool, u32), (u32, Span)>,
    type_name: String,
}

struct Scope {
    root: u32,
    prefix: String,
    ids: HashMap<String, u32>,
}

enum Pending {
    Binding { elem: u32, name: ast::Ident, binding: Binding, scope: u32 },
    Handler { elem: u32, handler: ast::Handler, scope: u32 },
}

struct Frame {
    /// `@children` location: (element, insertion index).
    placeholder: Option<(u32, usize)>,
}

pub struct Lowerer<'a> {
    reg: &'a Registry,
    elems: Vec<ElemInfo>,
    scopes: Vec<Scope>,
    pending: Vec<Pending>,
    frames: Vec<Frame>,
    stack: Vec<String>,
    diags: &'a mut Vec<Diagnostic>,
}

/// Context for lowering an expression.
struct Ctx<'c> {
    scope: u32,
    self_elem: u32,
    args: &'c [(String, Type)],
}

fn error(diags: &mut Vec<Diagnostic>, span: Span, message: String, label: Option<String>) {
    diags.push(Diagnostic { level: Level::Error, message, span, label });
}

const VOID: (Expr, Type) = (Expr::Literal(Value::Void), Type::Void);

impl<'a> Lowerer<'a> {
    pub fn new(reg: &'a Registry, diags: &'a mut Vec<Diagnostic>) -> Self {
        Self { reg, elems: Vec::new(), scopes: Vec::new(), pending: Vec::new(), frames: Vec::new(), stack: Vec::new(), diags }
    }

    fn err(&mut self, span: Span, message: impl Into<String>) {
        error(self.diags, span, message.into(), None);
    }

    fn err_hint(&mut self, span: Span, message: impl Into<String>, hint: Option<&str>) {
        error(self.diags, span, message.into(), hint.map(|h| format!("did you mean `{h}`?")));
    }

    fn warn(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic { level: Level::Warning, message: message.into(), span, label: None });
    }

    /// Compiles component `name` into a flat [`ComponentDef`].
    pub fn lower(mut self, name: &str) -> ComponentDef {
        self.scopes.push(Scope { root: 0, prefix: String::new(), ids: HashMap::new() });
        let span = self.reg.get(name).map(|c| c.name.span).unwrap_or_default();
        let (root, _) = self.instantiate(name, span, None, 0, None);
        debug_assert_eq!(root, 0);
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            self.apply_pending(p);
        }
        self.check_layout_positions();
        let root_info = &self.elems[0];
        let mut public_properties = Vec::new();
        let mut decl: Vec<_> = root_info.declared.iter().collect();
        decl.sort_by_key(|(i, _)| **i);
        for (idx, (vis, _)) in decl {
            let access = match vis {
                Visibility::In => Access::In,
                Visibility::Out => Access::Out,
                Visibility::InOut => Access::InOut,
                Visibility::Private => continue,
            };
            let p = &root_info.def.properties[*idx as usize];
            public_properties.push(PublicProperty {
                name: p.name.clone(),
                prop: PropRef { element: 0, property: *idx },
                ty: p.ty,
                access,
            });
        }
        let mut cbs: Vec<_> = root_info.declared_callbacks.keys().copied().collect();
        cbs.sort();
        let public_callbacks = cbs
            .into_iter()
            .map(|i| {
                let c = &root_info.def.callbacks[i as usize];
                PublicCallback { name: c.name.clone(), callback: CallbackRef { element: 0, callback: i }, params: c.params.clone() }
            })
            .collect();
        ComponentDef {
            name: name.to_string(),
            elements: self.elems.into_iter().map(|e| e.def).collect(),
            public_properties,
            public_callbacks,
        }
    }

    fn new_element(&mut self, kind: ElementKind, type_name: &str, parent: Option<u32>, scope: u32) -> u32 {
        let idx = self.elems.len() as u32;
        self.elems.push(ElemInfo {
            def: ElementDef::new(kind, ""),
            parent,
            scope,
            declared: HashMap::new(),
            declared_callbacks: HashMap::new(),
            bound: HashMap::new(),
            type_name: type_name.to_string(),
        });
        idx
    }

    /// Creates the element(s) for type `name`. Returns the element and where
    /// children written at the usage site should go: (element, insert position).
    fn instantiate(
        &mut self,
        name: &str,
        span: Span,
        parent: Option<u32>,
        scope: u32,
        usage_id: Option<&str>,
    ) -> (u32, (u32, Option<usize>)) {
        if let Some(kind) = ElementKind::from_name(name) {
            let idx = self.new_element(kind, name, parent, scope);
            return (idx, (idx, None));
        }
        let Some(comp) = self.reg.get(name).cloned() else {
            let candidates: Vec<&str> = ElementKind::ALL
                .iter()
                .map(|k| k.name())
                .chain(self.reg.keys().map(|s| s.as_str()))
                .collect();
            let hint = suggest(name, candidates);
            self.err_hint(span, format!("unknown element type `{name}`"), hint);
            let idx = self.new_element(ElementKind::Empty, name, parent, scope);
            return (idx, (idx, None));
        };
        if self.stack.iter().any(|s| s == name) {
            self.err(span, format!("component `{name}` instantiates itself recursively"));
            let idx = self.new_element(ElementKind::Empty, name, parent, scope);
            return (idx, (idx, None));
        }
        self.stack.push(name.to_string());
        let base = comp.root.base.as_ref().map(|b| b.name.clone()).unwrap_or_else(|| "Empty".into());
        let base_span = comp.root.base.as_ref().map(|b| b.span).unwrap_or(comp.name.span);
        let (idx, base_target) = self.instantiate(&base, base_span, parent, scope, usage_id);
        self.elems[idx as usize].type_name = name.to_string();
        let parent_prefix = self.scopes[scope as usize].prefix.clone();
        let prefix = match usage_id {
            _ if parent.is_none() => String::new(),
            Some(id) => format!("{parent_prefix}{id}."),
            None => format!("{parent_prefix}{name}#{idx}."),
        };
        self.scopes.push(Scope { root: idx, prefix, ids: HashMap::new() });
        let inner = (self.scopes.len() - 1) as u32;
        self.frames.push(Frame { placeholder: None });
        self.apply_body(&comp.root, idx, inner, base_target);
        let frame = self.frames.pop().expect("frame");
        self.stack.pop();
        let target = frame.placeholder.map(|(e, pos)| (e, Some(pos))).unwrap_or(base_target);
        (idx, target)
    }

    fn attach(&mut self, parent: u32, child: u32, pos: &mut Option<usize>) {
        self.elems[child as usize].parent = Some(parent);
        let children = &mut self.elems[parent as usize].def.children;
        match pos {
            Some(p) => {
                children.insert((*p).min(children.len()), child);
                *p += 1;
            }
            None => children.push(child),
        }
    }

    /// Expands an element written in source within `scope`.
    fn expand(&mut self, e: &ast::Element, scope: u32, parent: u32, pos: &mut Option<usize>) -> u32 {
        let base = e.base.as_ref().expect("child elements have a type");
        let id = e.id.as_ref().map(|i| i.name.as_str());
        let (idx, target) = self.instantiate(&base.name, base.span, Some(parent), scope, id);
        self.attach(parent, idx, pos);
        if let Some(id) = &e.id {
            let reserved = ["root", "self", "parent", "true", "false"];
            if reserved.contains(&id.name.as_str()) {
                self.err(id.span, format!("`{}` is reserved and cannot be used as an id", id.name));
            } else if self.scopes[scope as usize].ids.contains_key(&id.name) {
                self.err(id.span, format!("duplicate element id `{}`", id.name));
            } else {
                self.scopes[scope as usize].ids.insert(id.name.clone(), idx);
            }
            self.elems[idx as usize].def.id = format!("{}{}", self.scopes[scope as usize].prefix, id.name);
        }
        self.apply_body(e, idx, scope, target);
        idx
    }

    /// Applies declarations, bindings, handlers and children of `body` to element `idx`.
    fn apply_body(&mut self, body: &ast::Element, idx: u32, scope: u32, target: (u32, Option<usize>)) {
        for p in &body.properties {
            let Some(ty) = Type::from_name(&p.ty.name) else {
                let names = ["int", "float", "length", "bool", "string", "color", "brush"];
                let hint = suggest(&p.ty.name, names);
                self.err_hint(p.ty.span, format!("unknown type `{}`", p.ty.name), hint);
                continue;
            };
            let info = &mut self.elems[idx as usize];
            if info.def.properties.iter().any(|q| q.name == p.name.name) {
                let msg = format!("property `{}` is already declared on `{}`", p.name.name, info.type_name);
                self.err(p.name.span, msg);
                continue;
            }
            let pi = info.def.properties.len() as u32;
            info.def.properties.push(PropertyDef { name: p.name.name.clone(), ty, init: PropertyInit::Default(ty.default_value()) });
            info.declared.insert(pi, (p.visibility, scope));
            if let Some(b) = &p.init {
                self.pending.push(Pending::Binding { elem: idx, name: p.name.clone(), binding: b.clone(), scope });
            }
        }
        for c in &body.callbacks {
            let mut params = Vec::new();
            for t in &c.params {
                match Type::from_name(&t.name) {
                    Some(ty) => params.push(ty),
                    None => self.err(t.span, format!("unknown type `{}`", t.name)),
                }
            }
            let info = &mut self.elems[idx as usize];
            if info.def.callbacks.iter().any(|q| q.name == c.name.name) {
                let msg = format!("callback `{}` is already declared on `{}`", c.name.name, info.type_name);
                self.err(c.name.span, msg);
                continue;
            }
            let ci = info.def.callbacks.len() as u32;
            info.def.callbacks.push(CallbackDef { name: c.name.name.clone(), params, handler: None });
            info.declared_callbacks.insert(ci, scope);
        }
        for (name, b) in &body.bindings {
            self.pending.push(Pending::Binding { elem: idx, name: name.clone(), binding: b.clone(), scope });
        }
        for h in &body.handlers {
            self.pending.push(Pending::Handler { elem: idx, handler: h.clone(), scope });
        }
        let (target_elem, mut pos) = target;
        for child in &body.children {
            match child {
                Child::Element(c) => {
                    self.expand(c, scope, target_elem, &mut pos);
                }
                Child::Placeholder(span) => {
                    let frame_ok = !self.frames.is_empty();
                    if !frame_ok {
                        self.err(*span, "`@children` can only be used inside a component definition");
                        continue;
                    }
                    let frame = self.frames.last_mut().expect("frame");
                    if frame.placeholder.is_some() {
                        self.err(*span, "`@children` can only appear once per component");
                        continue;
                    }
                    let at = pos.unwrap_or(self.elems[target_elem as usize].def.children.len());
                    self.frames.last_mut().expect("frame").placeholder = Some((target_elem, at));
                }
            }
        }
    }

    // ----- name resolution -------------------------------------------------

    fn element_ref(&self, name: &str, ctx: &Ctx) -> Option<u32> {
        match name {
            "self" => Some(ctx.self_elem),
            "root" => Some(self.scopes[ctx.scope as usize].root),
            "parent" => self.elems[ctx.self_elem as usize].parent,
            _ => self.scopes[ctx.scope as usize].ids.get(name).copied(),
        }
    }

    /// Finds property `name` on element `e`, honoring visibility from `scope`.
    /// Returns Err(reason) if it exists but is not accessible.
    fn find_prop(&self, e: u32, name: &str, scope: u32) -> Option<Result<(u32, Type), String>> {
        let info = &self.elems[e as usize];
        let idx = info.def.properties.iter().position(|p| p.name == name)? as u32;
        if let Some((Visibility::Private, decl_scope)) = info.declared.get(&idx) {
            if *decl_scope != scope {
                return Some(Err(format!("property `{name}` of `{}` is private", info.type_name)));
            }
        }
        Some(Ok((idx, info.def.properties[idx as usize].ty)))
    }

    fn find_callback(&self, e: u32, name: &str) -> Option<(u32, Vec<Type>)> {
        let info = &self.elems[e as usize];
        let idx = info.def.callbacks.iter().position(|c| c.name == name)?;
        Some((idx as u32, info.def.callbacks[idx].params.clone()))
    }

    /// Elements searched for unqualified names: self, then ancestors in the same scope.
    fn lookup_chain(&self, ctx: &Ctx) -> Vec<u32> {
        let mut out = Vec::new();
        let root = self.scopes[ctx.scope as usize].root;
        let mut cur = Some(ctx.self_elem);
        while let Some(e) = cur {
            if self.elems[e as usize].scope == ctx.scope || e == root || e == ctx.self_elem {
                out.push(e);
            }
            if e == root {
                break;
            }
            cur = self.elems[e as usize].parent;
        }
        out
    }

    fn prop_candidates(&self, e: u32) -> Vec<String> {
        self.elems[e as usize].def.properties.iter().map(|p| p.name.clone()).collect()
    }

    /// Resolves an lvalue / property path expression to a property.
    fn resolve_prop_path(&mut self, e: &ast::Expr, ctx: &Ctx) -> Option<(PropRef, Type)> {
        match &e.kind {
            ExprKind::Ident(name) => {
                for el in self.lookup_chain(ctx) {
                    match self.find_prop(el, name, ctx.scope) {
                        Some(Ok((i, t))) => return Some((PropRef { element: el, property: i }, t)),
                        Some(Err(msg)) => {
                            self.err(e.span, msg);
                            return None;
                        }
                        None => {}
                    }
                }
                let cands = self.prop_candidates(ctx.self_elem);
                let hint = suggest(name, cands.iter().map(|s| s.as_str()));
                self.err_hint(e.span, format!("unknown property `{name}`"), hint);
                None
            }
            ExprKind::Member(obj, field) => {
                let ExprKind::Ident(obj_name) = &obj.kind else {
                    self.err(e.span, "expected a property reference such as `root.value` or `id.text`");
                    return None;
                };
                let Some(target) = self.element_ref(obj_name, ctx) else {
                    let ids: Vec<String> = self.scopes[ctx.scope as usize].ids.keys().cloned().collect();
                    let hint = suggest(obj_name, ids.iter().map(|s| s.as_str()).chain(["root", "self", "parent"]));
                    self.err_hint(obj.span, format!("unknown element `{obj_name}`"), hint);
                    return None;
                };
                match self.find_prop(target, &field.name, ctx.scope) {
                    Some(Ok((i, t))) => Some((PropRef { element: target, property: i }, t)),
                    Some(Err(msg)) => {
                        self.err(field.span, msg);
                        None
                    }
                    None => {
                        let cands = self.prop_candidates(target);
                        let hint = suggest(&field.name, cands.iter().map(|s| s.as_str()));
                        let tn = self.elems[target as usize].type_name.clone();
                        self.err_hint(field.span, format!("`{tn}` has no property `{}`", field.name), hint);
                        None
                    }
                }
            }
            _ => {
                self.err(e.span, "expected a property reference");
                None
            }
        }
    }

    /// Checks that `r` may be written from `scope`.
    fn check_writable(&mut self, r: PropRef, scope: u32, span: Span) -> bool {
        let info = &self.elems[r.element as usize];
        let name = info.def.properties[r.property as usize].name.clone();
        if let Some(spec) = info.def.kind.property(r.property) {
            if (r.property as usize) < info.def.kind.property_count() && spec.output {
                self.err(span, format!("`{name}` is an output property of `{}` and cannot be set", info.type_name));
                return false;
            }
        }
        if let Some((Visibility::Out, decl_scope)) = info.declared.get(&r.property) {
            if *decl_scope != scope {
                let tn = info.type_name.clone();
                self.err(span, format!("`{name}` is declared `out` in `{tn}` and cannot be set from outside"));
                return false;
            }
        }
        true
    }

    // ----- bindings and handlers ------------------------------------------

    fn apply_pending(&mut self, p: Pending) {
        match p {
            Pending::Binding { elem, name, binding, scope } => {
                let ctx = Ctx { scope, self_elem: elem, args: &[] };
                let found = match self.find_prop(elem, &name.name, scope) {
                    Some(Ok(v)) => v,
                    Some(Err(msg)) => return self.err(name.span, msg),
                    None => {
                        if self.find_callback(elem, &name.name).is_some() {
                            return self.err(name.span, format!("`{}` is a callback; use `{} => {{ ... }}`", name.name, name.name));
                        }
                        let cands = self.prop_candidates(elem);
                        let hint = suggest(&name.name, cands.iter().map(|s| s.as_str()));
                        let tn = self.elems[elem as usize].type_name.clone();
                        return self.err_hint(name.span, format!("`{tn}` has no property `{}`", name.name), hint);
                    }
                };
                let (pi, ty) = found;
                let r = PropRef { element: elem, property: pi };
                if !self.check_writable(r, scope, name.span) {
                    return;
                }
                if let Some((prev_scope, _)) = self.elems[elem as usize].bound.get(&(false, pi)) {
                    if *prev_scope == scope {
                        return self.err(name.span, format!("property `{}` is bound more than once", name.name));
                    }
                }
                self.elems[elem as usize].bound.insert((false, pi), (scope, name.span));
                let init = match &binding {
                    Binding::Expr(e) => {
                        let lowered = self.percent_binding(&name.name, e, &ctx).unwrap_or_else(|| {
                            let (x, t) = self.expr(e, &ctx, Some(ty));
                            self.coerce(x, t, ty, e.span)
                        });
                        PropertyInit::Binding(lowered)
                    }
                    Binding::TwoWay(e) => {
                        let Some((target, tty)) = self.resolve_prop_path(e, &ctx) else { return };
                        if target == r {
                            return self.err(e.span, "a property cannot be bound to itself");
                        }
                        if tty != ty {
                            return self.err(e.span, format!("two-way binding between `{ty}` and `{tty}`"));
                        }
                        if !self.check_writable(target, scope, e.span) {
                            return;
                        }
                        PropertyInit::Alias(target)
                    }
                };
                self.elems[elem as usize].def.properties[pi as usize].init = init;
            }
            Pending::Handler { elem, handler, scope } => {
                let Some((ci, params)) = self.find_callback(elem, &handler.name.name) else {
                    let names: Vec<String> = self.elems[elem as usize].def.callbacks.iter().map(|c| c.name.clone()).collect();
                    let hint = suggest(&handler.name.name, names.iter().map(|s| s.as_str()));
                    let tn = self.elems[elem as usize].type_name.clone();
                    return self.err_hint(handler.name.span, format!("`{tn}` has no callback `{}`", handler.name.name), hint);
                };
                if handler.params.len() > params.len() {
                    return self.err(
                        handler.name.span,
                        format!("callback `{}` has {} parameter(s), but the handler names {}", handler.name.name, params.len(), handler.params.len()),
                    );
                }
                if let Some((prev, _)) = self.elems[elem as usize].bound.get(&(true, ci)) {
                    if *prev == scope {
                        return self.err(handler.name.span, format!("callback `{}` already has a handler", handler.name.name));
                    }
                }
                self.elems[elem as usize].bound.insert((true, ci), (scope, handler.name.span));
                let args: Vec<(String, Type)> = handler.params.iter().map(|p| p.name.clone()).zip(params).collect();
                let ctx = Ctx { scope, self_elem: elem, args: &args };
                let body = self.stmts(&handler.body, &ctx);
                self.elems[elem as usize].def.callbacks[ci as usize].handler = Some(body);
            }
        }
    }

    /// `width: 50%` means half of the parent's width.
    fn percent_binding(&mut self, prop: &str, e: &ast::Expr, ctx: &Ctx) -> Option<Expr> {
        let ExprKind::Number(v, _, Some(unit)) = &e.kind else { return None };
        if unit != "%" {
            return None;
        }
        let axis = match prop {
            "width" | "x" | "min-width" | "max-width" | "preferred-width" => el::WIDTH,
            "height" | "y" | "min-height" | "max-height" | "preferred-height" => el::HEIGHT,
            _ => return None,
        };
        let Some(parent) = self.elems[ctx.self_elem as usize].parent else {
            self.err(e.span, "percentages need a parent element");
            return Some(VOID.0);
        };
        Some(Expr::Binary(
            BinaryOp::Mul,
            Box::new(Expr::Prop(PropRef { element: parent, property: axis })),
            Box::new(Expr::Literal(Value::Float((*v / 100.0) as f32))),
        ))
    }

    fn check_layout_positions(&mut self) {
        let mut warns = Vec::new();
        for info in &self.elems {
            let Some(p) = info.parent else { continue };
            if !self.elems[p as usize].def.kind.is_layout() {
                continue;
            }
            for prop in [el::X, el::Y] {
                if let Some((_, span)) = info.bound.get(&(false, prop)) {
                    warns.push(*span);
                }
            }
        }
        for span in warns {
            self.warn(span, "`x`/`y` are ignored for elements inside a layout");
        }
    }

    // ----- statements -------------------------------------------------------

    fn stmts(&mut self, body: &[ast::Stmt], ctx: &Ctx) -> Vec<Stmt> {
        let mut out = Vec::new();
        for s in body {
            match s {
                ast::Stmt::Assign { target, op, value } => {
                    let Some((r, ty)) = self.resolve_prop_path(target, ctx) else { continue };
                    if !self.check_writable(r, ctx.scope, target.span) {
                        continue;
                    }
                    let rhs = if *op == "=" {
                        let (v, t) = self.expr(value, ctx, Some(ty));
                        self.coerce(v, t, ty, value.span)
                    } else {
                        let bop = &op[..1];
                        let (v, t) = self.expr(value, ctx, None);
                        let (e, rt) = self.binary(bop, (Expr::Prop(r), ty), (v, t), value.span);
                        self.coerce(e, rt, ty, value.span)
                    };
                    out.push(Stmt::Assign(r, rhs));
                }
                ast::Stmt::Expr(e) => {
                    if let ExprKind::Call(callee, args) = &e.kind {
                        if let Some((cb, params)) = self.resolve_callback(callee, ctx) {
                            if args.len() != params.len() {
                                self.err(e.span, format!("expected {} argument(s), found {}", params.len(), args.len()));
                                continue;
                            }
                            let lowered = args
                                .iter()
                                .zip(&params)
                                .map(|(a, t)| {
                                    let (x, at) = self.expr(a, ctx, Some(*t));
                                    self.coerce(x, at, *t, a.span)
                                })
                                .collect();
                            out.push(Stmt::Call(cb, lowered));
                            continue;
                        }
                    }
                    let (_, t) = self.expr(e, ctx, None);
                    if t != Type::Void {
                        self.warn(e.span, "expression statement has no effect");
                    }
                }
                ast::Stmt::If { cond, then, otherwise } => {
                    let (c, t) = self.expr(cond, ctx, Some(Type::Bool));
                    let c = self.coerce(c, t, Type::Bool, cond.span);
                    let a = self.stmts(then, ctx);
                    let b = self.stmts(otherwise, ctx);
                    out.push(Stmt::If(c, a, b));
                }
            }
        }
        out
    }

    /// If `callee` names a callback, returns it.
    fn resolve_callback(&mut self, callee: &ast::Expr, ctx: &Ctx) -> Option<(CallbackRef, Vec<Type>)> {
        match &callee.kind {
            ExprKind::Ident(name) => {
                if BuiltinFn::from_name(name).is_some() {
                    return None;
                }
                for e in self.lookup_chain(ctx) {
                    if let Some((i, p)) = self.find_callback(e, name) {
                        return Some((CallbackRef { element: e, callback: i }, p));
                    }
                }
                None
            }
            ExprKind::Member(obj, field) => {
                let ExprKind::Ident(obj_name) = &obj.kind else { return None };
                let target = self.element_ref(obj_name, ctx)?;
                let (i, p) = self.find_callback(target, &field.name)?;
                Some((CallbackRef { element: target, callback: i }, p))
            }
            _ => None,
        }
    }

    // ----- expressions ------------------------------------------------------

    fn coerce(&mut self, e: Expr, from: Type, to: Type, span: Span) -> Expr {
        if from == to || from == Type::Void || to == Type::Void {
            return e;
        }
        match (from, to) {
            (Type::Int, Type::Float) => Expr::Convert(Type::Float, Box::new(e)),
            (Type::Int | Type::Float | Type::Bool, Type::String) => Expr::Convert(Type::String, Box::new(e)),
            (Type::Float, Type::Int) => {
                error(
                    self.diags,
                    span,
                    "expected `int`, found `float`".into(),
                    Some("convert with round(), floor() or ceil()".into()),
                );
                e
            }
            _ => {
                self.err(span, format!("expected `{to}`, found `{from}`"));
                e
            }
        }
    }

    fn expr(&mut self, e: &ast::Expr, ctx: &Ctx, expected: Option<Type>) -> (Expr, Type) {
        match &e.kind {
            ExprKind::Error => VOID,
            ExprKind::Number(v, dot, unit) => match unit.as_deref() {
                None if !dot && *v <= i32::MAX as f64 => (Expr::Literal(Value::Int(*v as i32)), Type::Int),
                None | Some("px") => (Expr::Literal(Value::Float(*v as f32)), Type::Float),
                Some("%") => (Expr::Literal(Value::Float((*v / 100.0) as f32)), Type::Float),
                Some(u) => {
                    self.err(e.span, format!("unknown unit `{u}`; supported units are `px` and `%`"));
                    VOID
                }
            },
            ExprKind::Bool(b) => (Expr::Literal(Value::Bool(*b)), Type::Bool),
            ExprKind::Color(c) => match Color::from_hex(c) {
                Some(col) => (Expr::Literal(Value::Color(col)), Type::Color),
                None => {
                    self.err(e.span, format!("invalid color `{c}`; use #rgb, #rgba, #rrggbb or #rrggbbaa"));
                    VOID
                }
            },
            ExprKind::Str(parts) => self.template(parts, ctx, e.span),
            ExprKind::Ident(name) => self.ident(name, e.span, ctx, expected),
            ExprKind::Member(obj, field) => {
                if let ExprKind::Ident(obj_name) = &obj.kind {
                    if let Some(en) = EnumType::ALL.iter().find(|en| en.name() == obj_name) {
                        return match en.variant_index(&field.name) {
                            Some(i) => (Expr::Literal(Value::Enum(*en, i)), Type::Enum(*en)),
                            None => {
                                let hint = suggest(&field.name, en.variants().iter().copied());
                                self.err_hint(field.span, format!("`{}` has no variant `{}`", en.name(), field.name), hint);
                                VOID
                            }
                        };
                    }
                    if self.element_ref(obj_name, ctx).is_some() && !ctx.args.iter().any(|(n, _)| n == obj_name) {
                        return match self.resolve_prop_path(e, ctx) {
                            Some((r, t)) => (Expr::Prop(r), t),
                            None => VOID,
                        };
                    }
                }
                let (_, t) = self.expr(obj, ctx, None);
                if t != Type::Void {
                    self.err(field.span, format!("`{t}` has no member `{}`", field.name));
                }
                VOID
            }
            ExprKind::Call(callee, args) => self.call(callee, args, e.span, ctx),
            ExprKind::Unary(op, a) => {
                let (x, t) = self.expr(a, ctx, None);
                match (*op, t) {
                    (_, Type::Void) => VOID,
                    ("-", Type::Int | Type::Float) => (Expr::Unary(UnaryOp::Neg, Box::new(x)), t),
                    ("!", Type::Bool) => (Expr::Unary(UnaryOp::Not, Box::new(x)), t),
                    _ => {
                        self.err(e.span, format!("cannot apply `{op}` to `{t}`"));
                        VOID
                    }
                }
            }
            ExprKind::Binary(op, a, b) => {
                let l = self.expr(a, ctx, None);
                let r = self.expr(b, ctx, None);
                self.binary(op, l, r, e.span)
            }
            ExprKind::Ternary(c, a, b) => {
                let (cx, ct) = self.expr(c, ctx, Some(Type::Bool));
                let cx = self.coerce(cx, ct, Type::Bool, c.span);
                let (ax, at) = self.expr(a, ctx, expected);
                let (bx, bt) = self.expr(b, ctx, expected);
                let ty = match (at, bt) {
                    (x, y) if x == y => x,
                    (Type::Void, y) => y,
                    (x, Type::Void) => x,
                    (Type::Int | Type::Float, Type::Int | Type::Float) => Type::Float,
                    (x, y) => {
                        self.err(e.span, format!("branches have different types: `{x}` and `{y}`"));
                        return VOID;
                    }
                };
                let ax = self.coerce(ax, at, ty, a.span);
                let bx = self.coerce(bx, bt, ty, b.span);
                (Expr::Conditional(Box::new(cx), Box::new(ax), Box::new(bx)), ty)
            }
        }
    }

    fn template(&mut self, parts: &[StrPart], ctx: &Ctx, span: Span) -> (Expr, Type) {
        let mut acc: Option<Expr> = None;
        for part in parts {
            let piece = match part {
                StrPart::Lit(s) => Expr::Literal(Value::String(s.clone())),
                StrPart::Code(code, offset) => {
                    let parsed = parse_embedded_expr(code, span.file, *offset, self.diags);
                    let (x, t) = self.expr(&parsed, ctx, None);
                    if t == Type::String { x } else { self.coerce(x, t, Type::String, parsed.span) }
                }
            };
            acc = Some(match acc {
                None => piece,
                Some(prev) => Expr::Binary(BinaryOp::Add, Box::new(prev), Box::new(piece)),
            });
        }
        (acc.unwrap_or(Expr::Literal(Value::String(String::new()))), Type::String)
    }

    fn ident(&mut self, name: &str, span: Span, ctx: &Ctx, expected: Option<Type>) -> (Expr, Type) {
        if let Some(i) = ctx.args.iter().position(|(n, _)| n == name) {
            return (Expr::Arg(i as u32), ctx.args[i].1);
        }
        if let Some(Type::Enum(en)) = expected {
            if let Some(i) = en.variant_index(name) {
                return (Expr::Literal(Value::Enum(en, i)), Type::Enum(en));
            }
        }
        for e in self.lookup_chain(ctx) {
            match self.find_prop(e, name, ctx.scope) {
                Some(Ok((i, t))) => return (Expr::Prop(PropRef { element: e, property: i }), t),
                Some(Err(msg)) => {
                    self.err(span, msg);
                    return VOID;
                }
                None => {}
            }
        }
        if self.element_ref(name, ctx).is_some() {
            self.err(span, format!("`{name}` is an element; access one of its properties, e.g. `{name}.width`"));
            return VOID;
        }
        let mut cands: Vec<String> = ctx.args.iter().map(|(n, _)| n.clone()).collect();
        for e in self.lookup_chain(ctx) {
            cands.extend(self.prop_candidates(e));
        }
        if let Some(Type::Enum(en)) = expected {
            cands.extend(en.variants().iter().map(|s| s.to_string()));
        }
        let hint = suggest(name, cands.iter().map(|s| s.as_str()));
        self.err_hint(span, format!("unknown identifier `{name}`"), hint);
        VOID
    }

    fn call(&mut self, callee: &ast::Expr, args: &[ast::Expr], span: Span, ctx: &Ctx) -> (Expr, Type) {
        let (f, mut lowered) = match &callee.kind {
            ExprKind::Ident(name) => match BuiltinFn::from_name(name) {
                Some(f) => (f, Vec::new()),
                None => {
                    if self.resolve_callback(callee, ctx).is_some() {
                        self.err(span, "callbacks can only be called as a statement, e.g. `root.clicked();`");
                    } else {
                        let names = BuiltinFn::ALL.iter().map(|f| f.name());
                        let hint = suggest(name, names);
                        self.err_hint(callee.span, format!("unknown function `{name}`"), hint);
                    }
                    return VOID;
                }
            },
            ExprKind::Member(obj, method) => {
                if self.resolve_callback(callee, ctx).is_some() {
                    self.err(span, "callbacks can only be called as a statement, e.g. `root.clicked();`");
                    return VOID;
                }
                match BuiltinFn::from_name(&method.name) {
                    Some(f) => (f, vec![self.expr(obj, ctx, None)]),
                    None => {
                        self.err(method.span, format!("unknown method `{}`", method.name));
                        return VOID;
                    }
                }
            }
            _ => {
                self.err(callee.span, "this expression cannot be called");
                return VOID;
            }
        };
        for a in args {
            lowered.push(self.expr(a, ctx, None));
        }
        if lowered.iter().any(|(_, t)| *t == Type::Void) {
            return VOID;
        }
        let num = |t: Type| t.is_numeric();
        let types: Vec<Type> = lowered.iter().map(|(_, t)| *t).collect();
        use BuiltinFn::*;
        let (params, ret): (Vec<Type>, Type) = match f {
            Min | Max | Clamp => {
                let n = if f == Clamp { 3 } else { 2 };
                let t = if types.iter().all(|t| *t == Type::Int) { Type::Int } else { Type::Float };
                (vec![t; n], t)
            }
            Abs => (vec![if types.first() == Some(&Type::Int) { Type::Int } else { Type::Float }], types.first().copied().filter(|t| *t == Type::Int).unwrap_or(Type::Float)),
            Round | Floor | Ceil => (vec![Type::Float], Type::Int),
            Sqrt => (vec![Type::Float], Type::Float),
            Rgb => (vec![Type::Float; 3], Type::Color),
            Rgba => (vec![Type::Float; 4], Type::Color),
            Darker | Brighter | Fade => (vec![Type::Color, Type::Float], Type::Color),
            Mix => (vec![Type::Color, Type::Color, Type::Float], Type::Color),
        };
        if lowered.len() != params.len() {
            self.err(span, format!("`{}` takes {} argument(s), found {}", f.name(), params.len(), lowered.len()));
            return VOID;
        }
        let mut out = Vec::new();
        for ((x, t), p) in lowered.into_iter().zip(&params) {
            if (num(*p) && !num(t)) || (!num(*p) && t != *p) {
                self.err(span, format!("`{}` expects `{p}` arguments, found `{t}`", f.name()));
                return VOID;
            }
            out.push(self.coerce(x, t, *p, span));
        }
        (Expr::Builtin(f, out), ret)
    }

    fn binary(&mut self, op: &str, (a, at): (Expr, Type), (b, bt): (Expr, Type), span: Span) -> (Expr, Type) {
        if at == Type::Void || bt == Type::Void {
            return VOID;
        }
        let bop = match op {
            "+" => BinaryOp::Add,
            "-" => BinaryOp::Sub,
            "*" => BinaryOp::Mul,
            "/" => BinaryOp::Div,
            "%" => BinaryOp::Rem,
            "==" => BinaryOp::Eq,
            "!=" => BinaryOp::Ne,
            "<" => BinaryOp::Lt,
            "<=" => BinaryOp::Le,
            ">" => BinaryOp::Gt,
            ">=" => BinaryOp::Ge,
            "&&" => BinaryOp::And,
            _ => BinaryOp::Or,
        };
        let mk = |a: Expr, b: Expr| Expr::Binary(bop, Box::new(a), Box::new(b));
        let numeric = at.is_numeric() && bt.is_numeric();
        let both_int = at == Type::Int && bt == Type::Int;
        let to_f = |e: Expr, t: Type| if t == Type::Int { Expr::Convert(Type::Float, Box::new(e)) } else { e };
        match bop {
            BinaryOp::Add if at == Type::String || bt == Type::String => {
                let a = if at == Type::String { a } else { Expr::Convert(Type::String, Box::new(a)) };
                let b = if bt == Type::String { b } else { Expr::Convert(Type::String, Box::new(b)) };
                (mk(a, b), Type::String)
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Rem if numeric => {
                if both_int { (mk(a, b), Type::Int) } else { (mk(to_f(a, at), to_f(b, bt)), Type::Float) }
            }
            BinaryOp::Div if numeric => (mk(to_f(a, at), to_f(b, bt)), Type::Float),
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge if numeric => {
                (mk(to_f(a, at), to_f(b, bt)), Type::Bool)
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge if at == Type::String && bt == Type::String => {
                (mk(a, b), Type::Bool)
            }
            BinaryOp::Eq | BinaryOp::Ne if numeric || at == bt => (mk(a, b), Type::Bool),
            BinaryOp::And | BinaryOp::Or if at == Type::Bool && bt == Type::Bool => (mk(a, b), Type::Bool),
            _ => {
                self.err(span, format!("cannot apply `{op}` to `{at}` and `{bt}`"));
                VOID
            }
        }
    }
}
