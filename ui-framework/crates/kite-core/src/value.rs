//! Dynamically typed values flowing through properties, bindings and callbacks.

use alloc::string::{String, ToString};
use core::fmt;

use crate::color::Color;

/// Built-in enumerations usable as property types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumType {
    TextHorizontalAlignment,
    TextVerticalAlignment,
    LayoutAlignment,
}

impl EnumType {
    pub const ALL: [EnumType; 3] = [
        EnumType::TextHorizontalAlignment,
        EnumType::TextVerticalAlignment,
        EnumType::LayoutAlignment,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EnumType::TextHorizontalAlignment => "TextHorizontalAlignment",
            EnumType::TextVerticalAlignment => "TextVerticalAlignment",
            EnumType::LayoutAlignment => "LayoutAlignment",
        }
    }

    pub fn variants(self) -> &'static [&'static str] {
        match self {
            EnumType::TextHorizontalAlignment => &["left", "center", "right"],
            EnumType::TextVerticalAlignment => &["top", "center", "bottom"],
            EnumType::LayoutAlignment => &["stretch", "start", "center", "end", "space-between"],
        }
    }

    pub fn variant_index(self, name: &str) -> Option<u8> {
        self.variants()
            .iter()
            .position(|v| *v == name)
            .map(|i| i as u8)
    }
}

/// Static types of the `.ui` language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Bool,
    Int,
    /// `float` and `length` (logical pixels) share a representation.
    Float,
    String,
    Color,
    Enum(EnumType),
}

impl Type {
    pub fn name(self) -> &'static str {
        match self {
            Type::Void => "void",
            Type::Bool => "bool",
            Type::Int => "int",
            Type::Float => "float",
            Type::String => "string",
            Type::Color => "color",
            Type::Enum(e) => e.name(),
        }
    }

    /// Parses a type name as written in `property <T>` declarations.
    pub fn from_name(name: &str) -> Option<Type> {
        Some(match name {
            "bool" => Type::Bool,
            "int" => Type::Int,
            "float" | "length" => Type::Float,
            "string" => Type::String,
            "color" | "brush" => Type::Color,
            _ => {
                return EnumType::ALL
                    .iter()
                    .find(|e| e.name() == name)
                    .map(|e| Type::Enum(*e))
            }
        })
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    pub fn default_value(self) -> Value {
        match self {
            Type::Void => Value::Void,
            Type::Bool => Value::Bool(false),
            Type::Int => Value::Int(0),
            Type::Float => Value::Float(0.0),
            Type::String => Value::String(String::new()),
            Type::Color => Value::Color(Color::TRANSPARENT),
            Type::Enum(e) => Value::Enum(e, 0),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Void,
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
    Color(Color),
    Enum(EnumType, u8),
}

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Value::Void => Type::Void,
            Value::Bool(_) => Type::Bool,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::String(_) => Type::String,
            Value::Color(_) => Type::Color,
            Value::Enum(e, _) => Type::Enum(*e),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i32),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match self {
            Value::Int(i) => Some(*i as f32),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<Color> {
        match self {
            Value::Color(c) => Some(*c),
            _ => None,
        }
    }

    pub fn as_enum(&self) -> Option<u8> {
        match self {
            Value::Enum(_, v) => Some(*v),
            _ => None,
        }
    }

    /// Converts to `ty`, applying the language's implicit/explicit conversions.
    pub fn convert(self, ty: Type) -> Value {
        match (self, ty) {
            (v, t) if v.ty() == t => v,
            (Value::Int(i), Type::Float) => Value::Float(i as f32),
            (Value::Float(f), Type::Int) => Value::Int(f as i32),
            (v, Type::String) => Value::String(v.to_string()),
            (_, t) => t.default_value(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Void => Ok(()),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(v) => {
                // Print at most 3 decimals and drop trailing zeros, e.g. 2.5, 0.333, 4.
                let r = libm::roundf(*v * 1000.0) / 1000.0;
                write!(f, "{r}")
            }
            Value::String(s) => f.write_str(s),
            Value::Color(c) => write!(f, "#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a),
            Value::Enum(e, i) => f.write_str(e.variants().get(*i as usize).copied().unwrap_or("?")),
        }
    }
}

macro_rules! from_impl {
    ($t:ty, $v:ident) => {
        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Value::$v(v.into())
            }
        }
    };
}
from_impl!(bool, Bool);
from_impl!(i32, Int);
from_impl!(f32, Float);
from_impl!(String, String);
from_impl!(&str, String);
from_impl!(Color, Color);

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn display_trims_floats() {
        assert_eq!(Value::Float(2.5).to_string(), "2.5");
        assert_eq!(Value::Float(4.0).to_string(), "4");
        assert_eq!(Value::Float(1.0 / 3.0).to_string(), "0.333");
    }

    #[test]
    fn conversions() {
        assert_eq!(Value::Int(3).convert(Type::Float), Value::Float(3.0));
        assert_eq!(
            Value::Int(3).convert(Type::String),
            Value::String("3".into())
        );
        assert_eq!(Type::from_name("length"), Some(Type::Float));
        assert_eq!(
            Type::from_name("LayoutAlignment"),
            Some(Type::Enum(EnumType::LayoutAlignment))
        );
    }
}
