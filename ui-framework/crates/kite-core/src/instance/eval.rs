//! Expression evaluation.

use alloc::format;
use alloc::string::String;

use super::State;
use crate::color::Color;
use crate::ir::{BinaryOp, BuiltinFn, Expr, UnaryOp};
use crate::value::Value;

fn num(v: &Value) -> f32 {
    v.as_float().unwrap_or(0.0)
}

impl State {
    pub(crate) fn eval(&mut self, e: &Expr, args: &[Value]) -> Value {
        match e {
            Expr::Literal(v) => v.clone(),
            Expr::Prop(r) => {
                let s = self.slot_of(*r);
                self.get_slot(s)
            }
            Expr::Arg(i) => args.get(*i as usize).cloned().unwrap_or_default(),
            Expr::Unary(op, a) => {
                let a = self.eval(a, args);
                match (op, a) {
                    (UnaryOp::Neg, Value::Int(i)) => Value::Int(i.wrapping_neg()),
                    (UnaryOp::Neg, Value::Float(f)) => Value::Float(-f),
                    (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
                    (_, v) => v,
                }
            }
            Expr::Binary(BinaryOp::And, a, b) => {
                let a = self.eval(a, args).as_bool().unwrap_or(false);
                Value::Bool(a && self.eval(b, args).as_bool().unwrap_or(false))
            }
            Expr::Binary(BinaryOp::Or, a, b) => {
                let a = self.eval(a, args).as_bool().unwrap_or(false);
                Value::Bool(a || self.eval(b, args).as_bool().unwrap_or(false))
            }
            Expr::Binary(op, a, b) => {
                let a = self.eval(a, args);
                let b = self.eval(b, args);
                binary(*op, a, b)
            }
            Expr::Conditional(c, a, b) => {
                if self.eval(c, args).as_bool().unwrap_or(false) {
                    self.eval(a, args)
                } else {
                    self.eval(b, args)
                }
            }
            Expr::Builtin(f, a) => {
                let mut vals = [Value::Void, Value::Void, Value::Void, Value::Void];
                for (i, e) in a.iter().take(4).enumerate() {
                    vals[i] = self.eval(e, args);
                }
                builtin(*f, &vals[..a.len().min(4)])
            }
            Expr::Convert(t, a) => self.eval(a, args).convert(*t),
        }
    }
}

fn binary(op: BinaryOp, a: Value, b: Value) -> Value {
    use BinaryOp::*;
    match (op, &a, &b) {
        (Add, Value::String(x), _) => {
            let mut s = String::from(x.as_str());
            s.push_str(&format!("{b}"));
            Value::String(s)
        }
        (Add, _, Value::String(y)) => Value::String(format!("{a}{y}")),
        (Add | Sub | Mul | Rem, Value::Int(x), Value::Int(y)) => Value::Int(match op {
            Add => x.wrapping_add(*y),
            Sub => x.wrapping_sub(*y),
            Mul => x.wrapping_mul(*y),
            _ => {
                if *y == 0 {
                    0
                } else {
                    x.wrapping_rem(*y)
                }
            }
        }),
        (Add, _, _) => Value::Float(num(&a) + num(&b)),
        (Sub, _, _) => Value::Float(num(&a) - num(&b)),
        (Mul, _, _) => Value::Float(num(&a) * num(&b)),
        (Div, _, _) => {
            let d = num(&b);
            Value::Float(if d == 0.0 { 0.0 } else { num(&a) / d })
        }
        (Rem, _, _) => {
            let d = num(&b);
            Value::Float(if d == 0.0 {
                0.0
            } else {
                libm::fmodf(num(&a), d)
            })
        }
        (Eq, _, _) => Value::Bool(equal(&a, &b)),
        (Ne, _, _) => Value::Bool(!equal(&a, &b)),
        (Lt | Le | Gt | Ge, Value::String(x), Value::String(y)) => Value::Bool(match op {
            Lt => x < y,
            Le => x <= y,
            Gt => x > y,
            _ => x >= y,
        }),
        (Lt, _, _) => Value::Bool(num(&a) < num(&b)),
        (Le, _, _) => Value::Bool(num(&a) <= num(&b)),
        (Gt, _, _) => Value::Bool(num(&a) > num(&b)),
        (Ge, _, _) => Value::Bool(num(&a) >= num(&b)),
        (And | Or, _, _) => unreachable!("short-circuit operators are handled by eval"),
    }
}

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => num(a) == num(b),
        _ => a == b,
    }
}

fn byte(v: &Value) -> u8 {
    num(v).clamp(0.0, 255.0) as u8
}

fn builtin(f: BuiltinFn, a: &[Value]) -> Value {
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    let both_int = a.iter().all(|v| matches!(v, Value::Int(_)));
    match f {
        BuiltinFn::Min | BuiltinFn::Max => {
            let pick_first = (num(&arg(0)) < num(&arg(1))) == (f == BuiltinFn::Min);
            let v = if pick_first { arg(0) } else { arg(1) };
            if both_int {
                v
            } else {
                Value::Float(num(&v))
            }
        }
        BuiltinFn::Abs => match arg(0) {
            Value::Int(i) => Value::Int(i.wrapping_abs()),
            v => Value::Float(libm::fabsf(num(&v))),
        },
        BuiltinFn::Round => Value::Int(libm::roundf(num(&arg(0))) as i32),
        BuiltinFn::Floor => Value::Int(libm::floorf(num(&arg(0))) as i32),
        BuiltinFn::Ceil => Value::Int(libm::ceilf(num(&arg(0))) as i32),
        BuiltinFn::Sqrt => Value::Float(libm::sqrtf(num(&arg(0)).max(0.0))),
        BuiltinFn::Clamp => {
            let (v, lo, hi) = (num(&arg(0)), num(&arg(1)), num(&arg(2)));
            let r = v.max(lo).min(hi);
            if both_int {
                Value::Int(r as i32)
            } else {
                Value::Float(r)
            }
        }
        BuiltinFn::Rgb => Value::Color(Color::rgb(byte(&arg(0)), byte(&arg(1)), byte(&arg(2)))),
        BuiltinFn::Rgba => Value::Color(Color::rgba(
            byte(&arg(0)),
            byte(&arg(1)),
            byte(&arg(2)),
            (num(&arg(3)).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        )),
        BuiltinFn::Darker => {
            Value::Color(arg(0).as_color().unwrap_or_default().darker(num(&arg(1))))
        }
        BuiltinFn::Brighter => {
            Value::Color(arg(0).as_color().unwrap_or_default().brighter(num(&arg(1))))
        }
        BuiltinFn::Fade => Value::Color(arg(0).as_color().unwrap_or_default().fade(num(&arg(1)))),
        BuiltinFn::Mix => {
            let a0 = arg(0).as_color().unwrap_or_default();
            Value::Color(a0.mix(arg(1).as_color().unwrap_or_default(), num(&arg(2))))
        }
        BuiltinFn::Fixed => {
            let decimals = (num(&arg(1)) as usize).min(6);
            let v = num(&arg(0));
            // Avoid printing "-0.00".
            let v = if libm::fabsf(v) < 0.5 * libm::powf(10.0, -(decimals as f32)) {
                0.0
            } else {
                v
            };
            Value::String(format!("{v:.decimals$}"))
        }
    }
}
