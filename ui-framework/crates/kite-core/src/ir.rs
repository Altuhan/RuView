//! The compiled form of a component.
//!
//! The compiler flattens a `.ui` component (including instantiated
//! sub-components) into a list of elements whose properties and callbacks are
//! addressed by index. The runtime instantiates this description; generated
//! Rust code builds it with plain constructors, so no parser is needed on the
//! target.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::elements::ElementKind;
use crate::value::{Type, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PropRef {
    pub element: u32,
    pub property: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallbackRef {
    pub element: u32,
    pub callback: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFn {
    Min,
    Max,
    Abs,
    Round,
    Floor,
    Ceil,
    Sqrt,
    Clamp,
    Rgb,
    Rgba,
    Darker,
    Brighter,
    Fade,
    Mix,
}

impl BuiltinFn {
    pub const ALL: [BuiltinFn; 14] = [
        BuiltinFn::Min,
        BuiltinFn::Max,
        BuiltinFn::Abs,
        BuiltinFn::Round,
        BuiltinFn::Floor,
        BuiltinFn::Ceil,
        BuiltinFn::Sqrt,
        BuiltinFn::Clamp,
        BuiltinFn::Rgb,
        BuiltinFn::Rgba,
        BuiltinFn::Darker,
        BuiltinFn::Brighter,
        BuiltinFn::Fade,
        BuiltinFn::Mix,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BuiltinFn::Min => "min",
            BuiltinFn::Max => "max",
            BuiltinFn::Abs => "abs",
            BuiltinFn::Round => "round",
            BuiltinFn::Floor => "floor",
            BuiltinFn::Ceil => "ceil",
            BuiltinFn::Sqrt => "sqrt",
            BuiltinFn::Clamp => "clamp",
            BuiltinFn::Rgb => "rgb",
            BuiltinFn::Rgba => "rgba",
            BuiltinFn::Darker => "darker",
            BuiltinFn::Brighter => "brighter",
            BuiltinFn::Fade => "fade",
            BuiltinFn::Mix => "mix",
        }
    }

    pub fn from_name(name: &str) -> Option<BuiltinFn> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Prop(PropRef),
    /// Argument `n` of the callback handler being executed.
    Arg(u32),
    Unary(UnaryOp, Box<Expr>),
    /// Operands are already converted to a common type by the compiler.
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    Builtin(BuiltinFn, Vec<Expr>),
    Convert(Type, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Assign(PropRef, Expr),
    Call(CallbackRef, Vec<Expr>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyInit {
    /// Not set in `.ui` code; layout may compute it.
    Default(Value),
    /// Explicitly bound (constant bindings are literal expressions).
    Binding(Expr),
    /// Two-way binding: this property is the target property.
    Alias(PropRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDef {
    pub name: String,
    pub ty: Type,
    pub init: PropertyInit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallbackDef {
    pub name: String,
    pub params: Vec<Type>,
    pub handler: Option<Vec<Stmt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElementDef {
    pub kind: ElementKind,
    /// The `id :=` given in source, or a generated debug name.
    pub id: String,
    pub children: Vec<u32>,
    /// Built-in properties first (in [`ElementKind::property`] order), then declared ones.
    pub properties: Vec<PropertyDef>,
    /// Built-in callbacks first, then declared ones.
    pub callbacks: Vec<CallbackDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    In,
    Out,
    InOut,
}

impl Access {
    pub fn readable(self) -> bool {
        true
    }

    pub fn writable(self) -> bool {
        matches!(self, Access::In | Access::InOut)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicProperty {
    pub name: String,
    pub prop: PropRef,
    pub ty: Type,
    pub access: Access,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicCallback {
    pub name: String,
    pub callback: CallbackRef,
    pub params: Vec<Type>,
}

/// A fully compiled, flattened component. Element 0 is the root.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDef {
    pub name: String,
    pub elements: Vec<ElementDef>,
    pub public_properties: Vec<PublicProperty>,
    pub public_callbacks: Vec<PublicCallback>,
}

impl ComponentDef {
    pub fn property(&self, r: PropRef) -> &PropertyDef {
        &self.elements[r.element as usize].properties[r.property as usize]
    }

    pub fn public_property(&self, name: &str) -> Option<&PublicProperty> {
        self.public_properties.iter().find(|p| p.name == name)
    }

    pub fn public_callback(&self, name: &str) -> Option<&PublicCallback> {
        self.public_callbacks.iter().find(|c| c.name == name)
    }
}

/// Shorthand constructors used by generated code to keep it compact.
pub mod build {
    use super::*;

    pub fn lit(v: impl Into<Value>) -> Expr {
        Expr::Literal(v.into())
    }

    pub fn prop(element: u32, property: u32) -> Expr {
        Expr::Prop(PropRef { element, property })
    }

    pub fn pref(element: u32, property: u32) -> PropRef {
        PropRef { element, property }
    }

    pub fn cref(element: u32, callback: u32) -> CallbackRef {
        CallbackRef { element, callback }
    }

    pub fn un(op: UnaryOp, e: Expr) -> Expr {
        Expr::Unary(op, Box::new(e))
    }

    pub fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    pub fn cond(c: Expr, a: Expr, b: Expr) -> Expr {
        Expr::Conditional(Box::new(c), Box::new(a), Box::new(b))
    }

    pub fn conv(t: Type, e: Expr) -> Expr {
        Expr::Convert(t, Box::new(e))
    }

    pub fn default_prop(name: &str, ty: Type, v: Value) -> PropertyDef {
        PropertyDef { name: name.into(), ty, init: PropertyInit::Default(v) }
    }

    pub fn bound_prop(name: &str, ty: Type, e: Expr) -> PropertyDef {
        PropertyDef { name: name.into(), ty, init: PropertyInit::Binding(e) }
    }

    pub fn alias_prop(name: &str, ty: Type, target: PropRef) -> PropertyDef {
        PropertyDef { name: name.into(), ty, init: PropertyInit::Alias(target) }
    }
}

impl ElementDef {
    /// An element of `kind` with every built-in property at its default and
    /// no callback handlers. Generated code starts from this and overrides.
    pub fn new(kind: ElementKind, id: &str) -> Self {
        let properties = (0..kind.property_count() as u32)
            .map(|i| {
                let spec = kind.property(i).expect("index in range");
                PropertyDef {
                    name: spec.name.into(),
                    ty: spec.ty,
                    init: PropertyInit::Default(kind.default_value(i)),
                }
            })
            .collect();
        let callbacks = kind
            .callbacks()
            .iter()
            .map(|c| CallbackDef { name: c.name.into(), params: c.params.to_vec(), handler: None })
            .collect();
        Self { kind, id: id.into(), children: Vec::new(), properties, callbacks }
    }
}
