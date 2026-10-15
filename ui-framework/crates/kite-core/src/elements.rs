//! The built-in element vocabulary shared by the compiler and the runtime.
//!
//! Every element has the [`COMMON`] geometry properties followed by its
//! kind-specific properties. Property indices are stable, so compiled code can
//! refer to `(element, index)` pairs instead of names.

use crate::color::Color;
use crate::value::{EnumType, Type, Value};
use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementKind {
    /// Invisible container; the base of components without `inherits`.
    Empty,
    Window,
    Rectangle,
    Text,
    TouchArea,
    Button,
    CheckBox,
    Slider,
    TextInput,
    HorizontalLayout,
    VerticalLayout,
}

/// Default value of a built-in property.
#[derive(Debug, Clone, Copy)]
pub enum Default {
    Bool(bool),
    Int(i32),
    Float(f32),
    Str(&'static str),
    Color(u32),
    Enum(EnumType, u8),
}

impl Default {
    pub fn to_value(self) -> Value {
        match self {
            Default::Bool(b) => Value::Bool(b),
            Default::Int(i) => Value::Int(i),
            Default::Float(f) => Value::Float(f),
            Default::Str(s) => Value::String(String::from(s)),
            Default::Color(c) => Value::Color(Color::from_argb_u32(c)),
            Default::Enum(e, v) => Value::Enum(e, v),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PropSpec {
    pub name: &'static str,
    pub ty: Type,
    pub default: Default,
    /// Output-only properties are written by the runtime (e.g. `pressed`)
    /// and cannot be bound from `.ui` code.
    pub output: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CallbackSpec {
    pub name: &'static str,
    pub params: &'static [Type],
}

const fn p(name: &'static str, ty: Type, default: Default) -> PropSpec {
    PropSpec { name, ty, default, output: false }
}

const fn out(name: &'static str, ty: Type, default: Default) -> PropSpec {
    PropSpec { name, ty, default, output: true }
}

const F: Type = Type::Float;
const B: Type = Type::Bool;
const S: Type = Type::String;
const C: Type = Type::Color;

/// Sentinel for "unbounded" maximum sizes.
pub const UNBOUNDED: f32 = 1.0e9;

pub const X: u32 = 0;
pub const Y: u32 = 1;
pub const WIDTH: u32 = 2;
pub const HEIGHT: u32 = 3;
pub const MIN_WIDTH: u32 = 4;
pub const MIN_HEIGHT: u32 = 5;
pub const MAX_WIDTH: u32 = 6;
pub const MAX_HEIGHT: u32 = 7;
pub const PREFERRED_WIDTH: u32 = 8;
pub const PREFERRED_HEIGHT: u32 = 9;
pub const HORIZONTAL_STRETCH: u32 = 10;
pub const VERTICAL_STRETCH: u32 = 11;
pub const VISIBLE: u32 = 12;
pub const OPACITY: u32 = 13;

pub static COMMON: [PropSpec; 14] = [
    p("x", F, Default::Float(0.0)),
    p("y", F, Default::Float(0.0)),
    p("width", F, Default::Float(0.0)),
    p("height", F, Default::Float(0.0)),
    p("min-width", F, Default::Float(0.0)),
    p("min-height", F, Default::Float(0.0)),
    p("max-width", F, Default::Float(UNBOUNDED)),
    p("max-height", F, Default::Float(UNBOUNDED)),
    p("preferred-width", F, Default::Float(0.0)),
    p("preferred-height", F, Default::Float(0.0)),
    p("horizontal-stretch", F, Default::Float(0.0)),
    p("vertical-stretch", F, Default::Float(0.0)),
    p("visible", B, Default::Bool(true)),
    p("opacity", F, Default::Float(1.0)),
];

const TEXT_COLOR: u32 = 0xff1f1f1f;
const HALIGN: Type = Type::Enum(EnumType::TextHorizontalAlignment);
const VALIGN: Type = Type::Enum(EnumType::TextVerticalAlignment);
const LALIGN: Type = Type::Enum(EnumType::LayoutAlignment);

static WINDOW: [PropSpec; 2] = [
    p("title", S, Default::Str("Kite")),
    p("background", C, Default::Color(0xfff5f5f7)),
];
static RECTANGLE: [PropSpec; 5] = [
    p("background", C, Default::Color(0)),
    p("border-radius", F, Default::Float(0.0)),
    p("border-width", F, Default::Float(0.0)),
    p("border-color", C, Default::Color(0)),
    p("clip", B, Default::Bool(false)),
];
static TEXT: [PropSpec; 5] = [
    p("text", S, Default::Str("")),
    p("color", C, Default::Color(TEXT_COLOR)),
    p("font-size", F, Default::Float(14.0)),
    p("horizontal-alignment", HALIGN, Default::Enum(EnumType::TextHorizontalAlignment, 0)),
    p("vertical-alignment", VALIGN, Default::Enum(EnumType::TextVerticalAlignment, 0)),
];
static TOUCH_AREA: [PropSpec; 5] = [
    p("enabled", B, Default::Bool(true)),
    out("pressed", B, Default::Bool(false)),
    out("has-hover", B, Default::Bool(false)),
    out("mouse-x", F, Default::Float(0.0)),
    out("mouse-y", F, Default::Float(0.0)),
];
static BUTTON: [PropSpec; 7] = [
    p("text", S, Default::Str("")),
    p("enabled", B, Default::Bool(true)),
    p("primary", B, Default::Bool(false)),
    p("font-size", F, Default::Float(14.0)),
    out("pressed", B, Default::Bool(false)),
    out("has-hover", B, Default::Bool(false)),
    out("has-focus", B, Default::Bool(false)),
];
static CHECK_BOX: [PropSpec; 6] = [
    p("text", S, Default::Str("")),
    p("checked", B, Default::Bool(false)),
    p("enabled", B, Default::Bool(true)),
    p("font-size", F, Default::Float(14.0)),
    out("has-hover", B, Default::Bool(false)),
    out("has-focus", B, Default::Bool(false)),
];
static SLIDER: [PropSpec; 7] = [
    p("value", F, Default::Float(0.0)),
    p("minimum", F, Default::Float(0.0)),
    p("maximum", F, Default::Float(100.0)),
    p("step", F, Default::Float(1.0)),
    p("enabled", B, Default::Bool(true)),
    out("pressed", B, Default::Bool(false)),
    out("has-focus", B, Default::Bool(false)),
];
static TEXT_INPUT: [PropSpec; 7] = [
    p("text", S, Default::Str("")),
    p("placeholder-text", S, Default::Str("")),
    p("font-size", F, Default::Float(14.0)),
    p("color", C, Default::Color(TEXT_COLOR)),
    p("enabled", B, Default::Bool(true)),
    p("password", B, Default::Bool(false)),
    out("has-focus", B, Default::Bool(false)),
];
static LAYOUT: [PropSpec; 3] = [
    p("spacing", F, Default::Float(0.0)),
    p("padding", F, Default::Float(0.0)),
    p("alignment", LALIGN, Default::Enum(EnumType::LayoutAlignment, 0)),
];

static CLICKED: [CallbackSpec; 1] = [CallbackSpec { name: "clicked", params: &[] }];
static TOGGLED: [CallbackSpec; 1] = [CallbackSpec { name: "toggled", params: &[] }];
static CHANGED: [CallbackSpec; 1] = [CallbackSpec { name: "changed", params: &[Type::Float] }];
static TEXT_INPUT_CB: [CallbackSpec; 2] = [
    CallbackSpec { name: "edited", params: &[Type::String] },
    CallbackSpec { name: "accepted", params: &[Type::String] },
];

impl ElementKind {
    pub const ALL: [ElementKind; 11] = [
        ElementKind::Empty,
        ElementKind::Window,
        ElementKind::Rectangle,
        ElementKind::Text,
        ElementKind::TouchArea,
        ElementKind::Button,
        ElementKind::CheckBox,
        ElementKind::Slider,
        ElementKind::TextInput,
        ElementKind::HorizontalLayout,
        ElementKind::VerticalLayout,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ElementKind::Empty => "Empty",
            ElementKind::Window => "Window",
            ElementKind::Rectangle => "Rectangle",
            ElementKind::Text => "Text",
            ElementKind::TouchArea => "TouchArea",
            ElementKind::Button => "Button",
            ElementKind::CheckBox => "CheckBox",
            ElementKind::Slider => "Slider",
            ElementKind::TextInput => "TextInput",
            ElementKind::HorizontalLayout => "HorizontalLayout",
            ElementKind::VerticalLayout => "VerticalLayout",
        }
    }

    pub fn from_name(name: &str) -> Option<ElementKind> {
        Self::ALL.iter().copied().find(|k| k.name() == name)
    }

    /// Kind-specific properties, which follow the [`COMMON`] ones.
    pub fn own_properties(self) -> &'static [PropSpec] {
        match self {
            ElementKind::Empty => &[],
            ElementKind::Window => &WINDOW,
            ElementKind::Rectangle => &RECTANGLE,
            ElementKind::Text => &TEXT,
            ElementKind::TouchArea => &TOUCH_AREA,
            ElementKind::Button => &BUTTON,
            ElementKind::CheckBox => &CHECK_BOX,
            ElementKind::Slider => &SLIDER,
            ElementKind::TextInput => &TEXT_INPUT,
            ElementKind::HorizontalLayout | ElementKind::VerticalLayout => &LAYOUT,
        }
    }

    pub fn callbacks(self) -> &'static [CallbackSpec] {
        match self {
            ElementKind::TouchArea | ElementKind::Button => &CLICKED,
            ElementKind::CheckBox => &TOGGLED,
            ElementKind::Slider => &CHANGED,
            ElementKind::TextInput => &TEXT_INPUT_CB,
            _ => &[],
        }
    }

    pub fn property_count(self) -> usize {
        COMMON.len() + self.own_properties().len()
    }

    /// Spec of built-in property number `index`.
    pub fn property(self, index: u32) -> Option<&'static PropSpec> {
        let i = index as usize;
        COMMON.get(i).or_else(|| self.own_properties().get(i - COMMON.len()))
    }

    pub fn property_index(self, name: &str) -> Option<u32> {
        (0..self.property_count() as u32).find(|i| self.property(*i).map(|p| p.name) == Some(name))
    }

    pub fn callback_index(self, name: &str) -> Option<u32> {
        self.callbacks().iter().position(|c| c.name == name).map(|i| i as u32)
    }

    /// Default value of property `index`, including per-kind overrides.
    pub fn default_value(self, index: u32) -> Value {
        let stretchy = matches!(
            self,
            ElementKind::Rectangle
                | ElementKind::Empty
                | ElementKind::TouchArea
                | ElementKind::HorizontalLayout
                | ElementKind::VerticalLayout
        );
        match index {
            HORIZONTAL_STRETCH
                if stretchy || matches!(self, ElementKind::Slider | ElementKind::TextInput) =>
            {
                Value::Float(1.0)
            }
            VERTICAL_STRETCH if stretchy => Value::Float(1.0),
            _ => self.property(index).map(|p| p.default.to_value()).unwrap_or_default(),
        }
    }

    pub fn is_layout(self) -> bool {
        matches!(self, ElementKind::HorizontalLayout | ElementKind::VerticalLayout)
    }

    /// Whether an unsized element fills its parent (otherwise it takes its preferred size).
    pub fn fills_parent(self) -> bool {
        matches!(
            self,
            ElementKind::Empty
                | ElementKind::Rectangle
                | ElementKind::TouchArea
                | ElementKind::HorizontalLayout
                | ElementKind::VerticalLayout
                | ElementKind::Window
        )
    }

    /// Whether the element can receive keyboard focus.
    pub fn focusable(self) -> bool {
        matches!(
            self,
            ElementKind::Button | ElementKind::CheckBox | ElementKind::Slider | ElementKind::TextInput
        )
    }

    pub fn interactive(self) -> bool {
        self.focusable() || self == ElementKind::TouchArea
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_lookup_is_stable() {
        assert_eq!(ElementKind::Rectangle.property_index("x"), Some(X));
        assert_eq!(ElementKind::Rectangle.property_index("visible"), Some(VISIBLE));
        assert_eq!(ElementKind::Rectangle.property_index("background"), Some(COMMON.len() as u32));
        assert_eq!(ElementKind::Text.property_index("background"), None);
        assert_eq!(ElementKind::Button.callback_index("clicked"), Some(0));
    }
}
