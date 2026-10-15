//! Runtime tests on hand-built IR (no compiler involved), the way generated
//! code drives the runtime on embedded targets.

use alloc::rc::Rc;
use alloc::vec;
use core::cell::Cell;

use crate::elements::{ElementKind, COMMON};
use crate::ir::build::*;
use crate::ir::*;
use crate::{ComponentInstance, Type, Value};

/// Root with `in-out property <int> n` and `out property <int> doubled: n * 2`,
/// and a Button child whose `clicked` handler does `n = n + 1`.
fn def() -> ComponentDef {
    let mut root = ElementDef::new(ElementKind::Window, "root");
    let n = root.properties.len() as u32;
    root.properties
        .push(default_prop("n", Type::Int, Value::Int(1)));
    root.properties.push(bound_prop(
        "doubled",
        Type::Int,
        bin(BinaryOp::Mul, prop(0, n), lit(2)),
    ));
    root.callbacks.push(CallbackDef {
        name: "bump".into(),
        params: vec![],
        handler: None,
    });
    root.children = vec![1];
    let mut button = ElementDef::new(ElementKind::Button, "b");
    let text = ElementKind::Button.property_index("text").unwrap();
    button.properties[text as usize].init = PropertyInit::Binding(conv(Type::String, prop(0, n)));
    button.callbacks[0].handler = Some(vec![
        Stmt::Assign(pref(0, n), bin(BinaryOp::Add, prop(0, n), lit(1))),
        Stmt::Call(cref(0, 0), vec![]),
    ]);
    ComponentDef {
        name: "T".into(),
        elements: vec![root, button],
        public_properties: vec![
            PublicProperty {
                name: "n".into(),
                prop: pref(0, n),
                ty: Type::Int,
                access: Access::InOut,
            },
            PublicProperty {
                name: "doubled".into(),
                prop: pref(0, n + 1),
                ty: Type::Int,
                access: Access::Out,
            },
        ],
        public_callbacks: vec![PublicCallback {
            name: "bump".into(),
            callback: cref(0, 0),
            params: vec![],
        }],
    }
}

#[test]
fn hand_built_ir_runs() {
    let app = ComponentInstance::from_def(def());
    assert_eq!(app.get_property("doubled").unwrap(), Value::Int(2));
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    app.on_callback("bump", move |_, _| c.set(c.get() + 1))
        .unwrap();
    app.set_size(crate::Size::new(100.0, 50.0));
    let g = app.element_geometry("b").unwrap();
    let p = crate::Point::new(g.x + g.width / 2.0, g.y + g.height / 2.0);
    app.dispatch_event(crate::Event::PointerPressed {
        position: p,
        button: crate::PointerButton::Left,
    });
    app.dispatch_event(crate::Event::PointerReleased {
        position: p,
        button: crate::PointerButton::Left,
    });
    assert_eq!(app.get_property("doubled").unwrap(), Value::Int(4));
    assert_eq!(app.element_property("b", "text").unwrap(), Value::from("2"));
    assert_eq!(calls.get(), 1);
    assert_eq!(COMMON.len(), 14);
}

#[test]
fn assignment_breaks_binding_and_setters_validate() {
    let app = ComponentInstance::from_def(def());
    assert!(app.set_property("doubled", 5).is_err());
    assert!(app.set_property("missing", 5).is_err());
    assert!(app.invoke("bump", &[Value::Int(1)]).is_err());
    app.set_property("n", 10).unwrap();
    assert_eq!(app.get_property("doubled").unwrap(), Value::Int(20));
}
