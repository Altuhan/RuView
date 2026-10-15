//! Generates Rust source for compiled components.
//!
//! For each exported component the output contains a struct with typed
//! getters/setters for its public properties, `on_*`/`invoke_*` methods for
//! its callbacks, and a `definition()` function that builds the IR with plain
//! constructors (no parsing at runtime, works in `no_std`).

use std::fmt::Write;

use kite_core::ir::{Access, ComponentDef, ElementDef, Expr, PropertyInit, Stmt};
use kite_core::{Type, Value};

fn rust_ident(name: &str) -> String {
    let s: String = name.chars().map(|c| if c == '-' { '_' } else { c }).collect();
    const KEYWORDS: [&str; 12] = ["type", "self", "super", "crate", "fn", "struct", "enum", "match", "loop", "move", "ref", "box"];
    if KEYWORDS.contains(&s.as_str()) { format!("r#{s}") } else { s }
}

fn f32_lit(v: f32) -> String {
    let v = if v.is_finite() { v } else { f32::MAX };
    format!("{v:?}f32")
}

struct Gen<'a> {
    rt: &'a str,
    out: String,
}

impl Gen<'_> {
    fn ty(&self, t: Type) -> String {
        let rt = self.rt;
        match t {
            Type::Void => "()".into(),
            Type::Bool => "bool".into(),
            Type::Int => "i32".into(),
            Type::Float => "f32".into(),
            Type::String => format!("{rt}::__private::String"),
            Type::Color => format!("{rt}::Color"),
            Type::Enum(_) => format!("{rt}::Value"),
        }
    }

    fn type_expr(&self, t: Type) -> String {
        let rt = self.rt;
        match t {
            Type::Enum(e) => format!("{rt}::Type::Enum({rt}::EnumType::{e:?})"),
            t => format!("{rt}::Type::{t:?}"),
        }
    }

    fn value(&self, v: &Value) -> String {
        let rt = self.rt;
        match v {
            Value::Void => format!("{rt}::Value::Void"),
            Value::Bool(b) => format!("{rt}::Value::Bool({b})"),
            Value::Int(i) => format!("{rt}::Value::Int({i})"),
            Value::Float(f) => format!("{rt}::Value::Float({})", f32_lit(*f)),
            Value::String(s) => format!("{rt}::Value::String({s:?}.into())"),
            Value::Color(c) => format!("{rt}::Value::Color({rt}::Color::from_argb_u32({:#010x}))", c.to_argb_u32()),
            Value::Enum(e, i) => format!("{rt}::Value::Enum({rt}::EnumType::{e:?}, {i})"),
        }
    }

    fn expr(&self, e: &Expr) -> String {
        let rt = self.rt;
        match e {
            Expr::Literal(v) => format!("Expr::Literal({})", self.value(v)),
            Expr::Prop(r) => format!("prop({}, {})", r.element, r.property),
            Expr::Arg(i) => format!("Expr::Arg({i})"),
            Expr::Unary(op, a) => format!("un(UnaryOp::{op:?}, {})", self.expr(a)),
            Expr::Binary(op, a, b) => format!("bin(BinaryOp::{op:?}, {}, {})", self.expr(a), self.expr(b)),
            Expr::Conditional(c, a, b) => format!("cond({}, {}, {})", self.expr(c), self.expr(a), self.expr(b)),
            Expr::Builtin(f, args) => {
                let a: Vec<String> = args.iter().map(|x| self.expr(x)).collect();
                format!("Expr::Builtin(BuiltinFn::{f:?}, {rt}::__private::vec![{}])", a.join(", "))
            }
            Expr::Convert(t, a) => format!("conv({}, {})", self.type_expr(*t), self.expr(a)),
        }
    }

    fn stmts(&self, s: &[Stmt]) -> String {
        let rt = self.rt;
        let items: Vec<String> = s
            .iter()
            .map(|s| match s {
                Stmt::Assign(r, e) => format!("Stmt::Assign(pref({}, {}), {})", r.element, r.property, self.expr(e)),
                Stmt::Call(c, args) => {
                    let a: Vec<String> = args.iter().map(|x| self.expr(x)).collect();
                    format!("Stmt::Call(cref({}, {}), {rt}::__private::vec![{}])", c.element, c.callback, a.join(", "))
                }
                Stmt::If(c, a, b) => format!("Stmt::If({}, {}, {})", self.expr(c), self.stmts(a), self.stmts(b)),
            })
            .collect();
        format!("{rt}::__private::vec![{}]", items.join(", "))
    }

    fn element(&mut self, i: usize, e: &ElementDef) {
        let rt = self.rt;
        let fresh = ElementDef::new(e.kind, "");
        let _ = writeln!(self.out, "        // [{i}] {}{}", e.kind.name(), if e.id.is_empty() { String::new() } else { format!(" `{}`", e.id) });
        let _ = writeln!(self.out, "        let mut e = ElementDef::new(ElementKind::{:?}, {:?});", e.kind, e.id);
        if !e.children.is_empty() {
            let c: Vec<String> = e.children.iter().map(|c| c.to_string()).collect();
            let _ = writeln!(self.out, "        e.children = {rt}::__private::vec![{}];", c.join(", "));
        }
        for (pi, p) in e.properties.iter().enumerate() {
            let init = match &p.init {
                PropertyInit::Default(v) => format!("PropertyInit::Default({})", self.value(v)),
                PropertyInit::Binding(x) => format!("PropertyInit::Binding({})", self.expr(x)),
                PropertyInit::Alias(r) => format!("PropertyInit::Alias(pref({}, {}))", r.element, r.property),
            };
            match fresh.properties.get(pi) {
                Some(f) if f.init == p.init => {}
                Some(_) => {
                    let _ = writeln!(self.out, "        e.properties[{pi}].init = {init}; // {}", p.name);
                }
                None => {
                    let _ = writeln!(
                        self.out,
                        "        e.properties.push(PropertyDef {{ name: {:?}.into(), ty: {}, init: {init} }});",
                        p.name,
                        self.type_expr(p.ty)
                    );
                }
            }
        }
        for (ci, c) in e.callbacks.iter().enumerate() {
            let handler = match &c.handler {
                Some(s) => format!("Some({})", self.stmts(s)),
                None => "None".into(),
            };
            if ci < fresh.callbacks.len() {
                if c.handler.is_some() {
                    let _ = writeln!(self.out, "        e.callbacks[{ci}].handler = {handler}; // {}", c.name);
                }
            } else {
                let params: Vec<String> = c.params.iter().map(|t| self.type_expr(*t)).collect();
                let _ = writeln!(
                    self.out,
                    "        e.callbacks.push(CallbackDef {{ name: {:?}.into(), params: {rt}::__private::vec![{}], handler: {handler} }});",
                    c.name,
                    params.join(", ")
                );
            }
        }
        let _ = writeln!(self.out, "        elements.push(e);");
    }

    fn from_value(&self, t: Type, v: &str) -> String {
        match t {
            Type::Bool => format!("{v}.as_bool().unwrap_or_default()"),
            Type::Int => format!("{v}.as_int().unwrap_or_default()"),
            Type::Float => format!("{v}.as_float().unwrap_or_default()"),
            Type::String => format!("{v}.as_str().map({}::__private::String::from).unwrap_or_default()", self.rt),
            Type::Color => format!("{v}.as_color().unwrap_or_default()"),
            Type::Enum(_) | Type::Void => format!("{v}.clone()"),
        }
    }

    fn component(&mut self, def: &ComponentDef) {
        let rt = self.rt;
        let name = rust_ident(&def.name);
        let _ = writeln!(self.out, "/// Generated from the `{}` component.", def.name);
        let _ = writeln!(self.out, "#[derive(Clone)]\npub struct {name} {{\n    instance: {rt}::ComponentInstance,\n}}\n");
        let _ = writeln!(self.out, "#[allow(dead_code, clippy::all)]\nimpl {name} {{");
        let _ = writeln!(self.out, "    /// Creates a new instance.\n    pub fn new() -> Self {{\n        Self {{ instance: {rt}::ComponentInstance::new({rt}::__private::Rc::new(Self::definition())) }}\n    }}\n");
        let _ = writeln!(self.out, "    /// The underlying dynamic instance.\n    pub fn instance(&self) -> &{rt}::ComponentInstance {{\n        &self.instance\n    }}\n");
        for p in &def.public_properties {
            let id = rust_ident(&p.name);
            let ty = self.ty(p.ty);
            let conv = self.from_value(p.ty, "v");
            let _ = writeln!(
                self.out,
                "    pub fn get_{id}(&self) -> {ty} {{\n        match self.instance.get_property({:?}) {{ Ok(v) => {conv}, Err(_) => unreachable!() }}\n    }}\n",
                p.name
            );
            if p.access != Access::Out {
                let _ = writeln!(
                    self.out,
                    "    pub fn set_{id}(&self, value: {ty}) {{\n        let _ = self.instance.set_property({:?}, {rt}::Value::from(value));\n    }}\n",
                    p.name
                );
            }
        }
        for c in &def.public_callbacks {
            let id = rust_ident(&c.name);
            let params: Vec<String> = c.params.iter().map(|t| self.ty(*t)).collect();
            let args: Vec<String> = c.params.iter().enumerate().map(|(i, t)| self.from_value(*t, &format!("args[{i}]"))).collect();
            let _ = writeln!(
                self.out,
                "    /// Sets the Rust handler for `{}`; it receives the component.\n    pub fn on_{id}(&self, mut handler: impl FnMut(&{name}{}) + 'static) {{\n        let _ = self.instance.on_callback({:?}, move |instance, args| {{\n            let _ = &args;\n            let this = {name} {{ instance: instance.clone() }};\n            handler(&this{})\n        }});\n    }}\n",
                c.name,
                params.iter().map(|p| format!(", {p}")).collect::<String>(),
                c.name,
                args.iter().map(|a| format!(", {a}")).collect::<String>(),
            );
            let sig: Vec<String> = params.iter().enumerate().map(|(i, p)| format!(", arg{i}: {p}")).collect();
            let vals: Vec<String> = (0..params.len()).map(|i| format!("{rt}::Value::from(arg{i})")).collect();
            let _ = writeln!(
                self.out,
                "    pub fn invoke_{id}(&self{}) {{\n        let _ = self.instance.invoke({:?}, &[{}]);\n    }}\n",
                sig.concat(),
                c.name,
                vals.join(", ")
            );
        }
        let _ = writeln!(self.out, "    /// The compiled component description.\n    pub fn definition() -> {rt}::ir::ComponentDef {{");
        let _ = writeln!(self.out, "        #[allow(unused_imports)]\n        use {rt}::ir::{{build::*, *}};\n        use {rt}::elements::ElementKind;");
        let _ = writeln!(self.out, "        let mut elements = {rt}::__private::Vec::with_capacity({});", def.elements.len());
        for (i, e) in def.elements.iter().enumerate() {
            self.element(i, e);
        }
        let props: Vec<String> = def
            .public_properties
            .iter()
            .map(|p| {
                format!(
                    "PublicProperty {{ name: {:?}.into(), prop: pref({}, {}), ty: {}, access: Access::{:?} }}",
                    p.name, p.prop.element, p.prop.property, self.type_expr(p.ty), p.access
                )
            })
            .collect();
        let cbs: Vec<String> = def
            .public_callbacks
            .iter()
            .map(|c| {
                let params: Vec<String> = c.params.iter().map(|t| self.type_expr(*t)).collect();
                format!(
                    "PublicCallback {{ name: {:?}.into(), callback: cref({}, {}), params: {rt}::__private::vec![{}] }}",
                    c.name, c.callback.element, c.callback.callback, params.join(", ")
                )
            })
            .collect();
        let _ = writeln!(
            self.out,
            "        ComponentDef {{\n            name: {:?}.into(),\n            elements,\n            public_properties: {rt}::__private::vec![{}],\n            public_callbacks: {rt}::__private::vec![{}],\n        }}\n    }}\n}}\n",
            def.name,
            props.join(", "),
            cbs.join(", ")
        );
        let _ = writeln!(self.out, "impl Default for {name} {{\n    fn default() -> Self {{\n        Self::new()\n    }}\n}}\n");
        let _ = writeln!(self.out, "impl {rt}::Component for {name} {{\n    fn instance(&self) -> &{rt}::ComponentInstance {{\n        &self.instance\n    }}\n}}\n");
    }
}

/// Rust source for `components`, referring to the runtime crate at `runtime_path` (e.g. `::kite`).
pub fn generate(components: &[ComponentDef], runtime_path: &str, source_name: &str) -> String {
    let mut g = Gen { rt: runtime_path, out: String::new() };
    let _ = writeln!(g.out, "// @generated by kite-compiler from `{source_name}`. Do not edit.\n");
    for c in components {
        g.component(c);
    }
    g.out
}
