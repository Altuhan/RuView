//! End-to-end tests: `.ui` source → IR → running instance.

use kite_core::{ComponentInstance, Event, Key, Point, PointerButton, Size, Value};

use crate::{compile_source, Diagnostics};

fn run(src: &str) -> ComponentInstance {
    match compile_source("test.ui", src) {
        Ok(c) => c.instantiate(),
        Err(d) => panic!("compile failed:\n{d}"),
    }
}

fn errors(src: &str) -> Diagnostics {
    match compile_source("test.ui", src) {
        Ok(_) => panic!("expected compile errors"),
        Err(d) => d,
    }
}

fn assert_error(src: &str, needle: &str) {
    let d = errors(src);
    let text = d.to_string();
    assert!(text.contains(needle), "expected `{needle}` in:\n{text}");
}

fn click(app: &ComponentInstance, x: f32, y: f32) {
    let position = Point::new(x, y);
    app.dispatch_event(Event::PointerMoved { position });
    app.dispatch_event(Event::PointerPressed {
        position,
        button: PointerButton::Left,
    });
    app.dispatch_event(Event::PointerReleased {
        position,
        button: PointerButton::Left,
    });
}

fn center(app: &ComponentInstance, id: &str) -> (f32, f32) {
    let g = app.element_geometry(id).unwrap();
    (g.x + g.width / 2.0, g.y + g.height / 2.0)
}

const COUNTER: &str = r#"
export component Counter inherits Window {
    in-out property <int> counter: 0;
    out property <string> label: "Clicked \{counter} times";
    callback reset;
    reset => { counter = 0; }
    VerticalLayout {
        padding: 10px;
        spacing: 8px;
        Text { text: label; }
        btn := Button {
            text: "Increment";
            clicked => { counter += 1; }
        }
    }
}
"#;

#[test]
fn counter_reacts_to_clicks() {
    let app = run(COUNTER);
    app.set_size(Size::new(200.0, 120.0));
    assert_eq!(
        app.get_property("label").unwrap(),
        Value::from("Clicked 0 times")
    );
    let (x, y) = center(&app, "btn");
    click(&app, x, y);
    click(&app, x, y);
    assert_eq!(app.get_property("counter").unwrap(), Value::Int(2));
    assert_eq!(
        app.get_property("label").unwrap(),
        Value::from("Clicked 2 times")
    );
    app.invoke("reset", &[]).unwrap();
    assert_eq!(
        app.get_property("label").unwrap(),
        Value::from("Clicked 0 times")
    );
}

#[test]
fn rust_handlers_override_and_can_mutate() {
    let app = run(COUNTER);
    app.set_property("counter", 5).unwrap();
    app.on_callback("reset", |inst, _| {
        let n = inst.get_property("counter").unwrap().as_int().unwrap();
        inst.set_property("counter", n * 10).unwrap();
    })
    .unwrap();
    app.invoke("reset", &[]).unwrap();
    assert_eq!(app.get_property("counter").unwrap(), Value::Int(50));
    assert!(
        app.set_property("label", "x").is_err(),
        "out property is read-only"
    );
    assert!(app.set_property("counter", "x").is_err(), "type checked");
}

#[test]
fn bindings_track_dependencies_transitively() {
    let app = run(r#"
        export component A {
            in property <int> a: 1;
            out property <int> b: a * 2;
            out property <int> c: b + a;
            out property <bool> big: c > 10;
        }
    "#);
    assert_eq!(app.get_property("c").unwrap(), Value::Int(3));
    app.set_property("a", 5).unwrap();
    assert_eq!(app.get_property("b").unwrap(), Value::Int(10));
    assert_eq!(app.get_property("c").unwrap(), Value::Int(15));
    assert_eq!(app.get_property("big").unwrap(), Value::Bool(true));
}

#[test]
fn components_children_and_two_way_bindings() {
    let app = run(r#"
        component Card inherits Rectangle {
            in property <string> title;
            in-out property <string> value;
            background: #ffffff;
            VerticalLayout {
                Text { text: root.title; }
                input := TextInput { text <=> root.value; }
                @children
            }
        }
        export component App inherits Window {
            in-out property <string> name: "Ada";
            width: 300px;
            height: 200px;
            c1 := Card {
                title: "Name";
                value <=> root.name;
                extra := Text { text: "below"; }
            }
        }
    "#);
    assert_eq!(
        app.element_property("c1.input", "text").unwrap(),
        Value::from("Ada")
    );
    app.set_property("name", "Grace").unwrap();
    assert_eq!(
        app.element_property("c1.input", "text").unwrap(),
        Value::from("Grace")
    );
    // Typing into the input flows back through both two-way bindings.
    let (x, y) = center(&app, "c1.input");
    click(&app, x, y);
    assert_eq!(app.focused_element().as_deref(), Some("c1.input"));
    app.dispatch_event(Event::KeyPressed(Key::Backspace));
    app.dispatch_event(Event::Text("!".into()));
    assert_eq!(app.get_property("name").unwrap(), Value::from("Grac!"));
    // `@children` content is placed inside the card's layout, after the input.
    let input = app.element_geometry("c1.input").unwrap();
    let extra = app.element_geometry("extra").unwrap();
    assert!(extra.y >= input.bottom() - 0.01, "{extra:?} {input:?}");
}

#[test]
fn layout_distributes_space() {
    let app = run(r#"
        export component L inherits Window {
            width: 400px;
            height: 100px;
            HorizontalLayout {
                padding: 10px;
                spacing: 10px;
                a := Rectangle { horizontal-stretch: 1; }
                b := Rectangle { horizontal-stretch: 3; }
                c := Rectangle { width: 50px; }
            }
        }
    "#);
    app.set_size(Size::new(400.0, 100.0));
    let (a, b, c) = (
        app.element_geometry("a").unwrap(),
        app.element_geometry("b").unwrap(),
        app.element_geometry("c").unwrap(),
    );
    // 400 - 2*10 padding - 2*10 spacing - 50 fixed = 310, split 1:3
    assert!((a.width - 77.5).abs() < 0.5, "{a:?}");
    assert!((b.width - 232.5).abs() < 0.5, "{b:?}");
    assert_eq!(c.width, 50.0);
    assert_eq!(c.x, 340.0);
    assert_eq!(a.height, 80.0);
    // Geometry is visible to bindings.
    assert_eq!(app.element_property("c", "x").unwrap(), Value::Float(340.0));
}

#[test]
fn geometry_bindings_and_percentages() {
    let app = run(r#"
        export component P inherits Window {
            width: 200px;
            height: 100px;
            half := Rectangle { width: 50%; height: parent.height / 4; x: 0; y: 0; }
            follow := Rectangle { x: half.width + 10px; width: 20px; height: 20px; y: 0; }
        }
    "#);
    app.set_size(Size::new(200.0, 100.0));
    assert_eq!(app.element_geometry("half").unwrap().width, 100.0);
    assert_eq!(app.element_geometry("half").unwrap().height, 25.0);
    assert_eq!(app.element_geometry("follow").unwrap().x, 110.0);
    app.set_size(Size::new(300.0, 100.0));
    assert_eq!(app.element_geometry("follow").unwrap().x, 160.0);
}

#[test]
fn widgets_respond_to_input() {
    let app = run(r#"
        export component W inherits Window {
            out property <float> level: s.value;
            out property <bool> on: cb.checked;
            in-out property <string> log;
            VerticalLayout {
                padding: 10px;
                cb := CheckBox { text: "Enable"; toggled => { log += "t"; } }
                s := Slider { maximum: 10; changed(v) => { log += "s" + v; } }
                t := TextInput { accepted(text) => { log += "[" + text + "]"; } }
            }
        }
    "#);
    app.set_size(Size::new(220.0, 160.0));
    let (x, y) = center(&app, "cb");
    click(&app, x, y);
    assert_eq!(app.get_property("on").unwrap(), Value::Bool(true));
    let g = app.element_geometry("s").unwrap();
    click(&app, g.right() - 1.0, g.y + g.height / 2.0);
    assert_eq!(app.get_property("level").unwrap(), Value::Float(10.0));
    app.dispatch_event(Event::KeyPressed(Key::Left));
    assert_eq!(app.get_property("level").unwrap(), Value::Float(9.0));
    app.dispatch_event(Event::KeyPressed(Key::Tab));
    assert_eq!(app.focused_element().as_deref(), Some("t"));
    app.dispatch_event(Event::Text("hi".into()));
    app.dispatch_event(Event::KeyPressed(Key::Enter));
    assert_eq!(app.get_property("log").unwrap(), Value::from("ts10s9[hi]"));
}

#[test]
fn disabled_and_hidden_widgets_ignore_clicks() {
    let app = run(r#"
        export component D inherits Window {
            in-out property <int> n;
            width: 200px; height: 100px;
            b := Button { enabled: false; clicked => { n += 1; } }
            h := Button { visible: false; clicked => { n += 10; } }
        }
    "#);
    let (x, y) = center(&app, "b");
    click(&app, x, y);
    assert_eq!(app.get_property("n").unwrap(), Value::Int(0));
}

#[test]
fn expressions_and_builtins() {
    let app = run(r#"
        export component E {
            in property <float> t: 0.25;
            out property <int> r: round(t * 10);
            out property <float> m: max(1, t);
            out property <string> s: t > 0.5 ? "high" : "low";
            out property <color> c: #ff0000.darker(0.5);
            out property <int> i: 7 % 3 + abs(-2);
            out property <float> d: 7 / 2;
            out property <bool> eq: "a" + 1 == "a1";
        }
    "#);
    assert_eq!(app.get_property("r").unwrap(), Value::Int(3));
    assert_eq!(app.get_property("m").unwrap(), Value::Float(1.0));
    assert_eq!(app.get_property("s").unwrap(), Value::from("low"));
    assert_eq!(
        app.get_property("c").unwrap(),
        Value::Color(kite_core::Color::rgb(128, 0, 0))
    );
    assert_eq!(app.get_property("i").unwrap(), Value::Int(3));
    assert_eq!(app.get_property("d").unwrap(), Value::Float(3.5));
    assert_eq!(app.get_property("eq").unwrap(), Value::Bool(true));
}

#[test]
fn binding_cycles_do_not_hang() {
    let app = run(r#"
        export component C {
            out property <int> a: b + 1;
            out property <int> b: a + 1;
        }
    "#);
    let _ = app.get_property("a").unwrap();
}

#[test]
fn reports_helpful_errors() {
    assert_error(
        "export component A { Text { colour: red; } }",
        "`Text` has no property `colour`",
    );
    assert_error(
        "export component A { Text { colour: red; } }",
        "did you mean `color`?",
    );
    assert_error("export component A { Buton { } }", "did you mean `Button`?");
    assert_error(
        "export component A { in property <int> n: 1.5; }",
        "expected `int`, found `float`",
    );
    assert_error(
        "export component A { Button { pressed: true; } }",
        "output property",
    );
    assert_error(
        "export component A { Text { text: missing; } }",
        "unknown identifier `missing`",
    );
    assert_error(
        "export component A { b := Button {} b := Text {} }",
        "duplicate element id `b`",
    );
    assert_error(
        "component Inner { out property <int> v; }\nexport component A { Inner { v: 3; } }",
        "declared `out`",
    );
    assert_error(
        "component Inner { property <int> secret; }\nexport component A { i := Inner {} Text { text: i.secret; } }",
        "is private",
    );
    assert_error(
        "export component A { callback go; Text { text: go(); } }",
        "only be called as a statement",
    );
    assert_error(
        "component A { A {} }\nexport component B { A {} }",
        "recursively",
    );
    let d = errors("export component A {\n    Text { text: 1 +; }\n}");
    let rendered = d.to_string();
    assert!(rendered.contains("test.ui:2:"), "{rendered}");
}

#[test]
fn warns_about_positions_in_layouts() {
    let c = compile_source(
        "test.ui",
        "export component A { VerticalLayout { Text { x: 5px; text: \"hi\"; } } }",
    )
    .unwrap();
    assert_eq!(c.diagnostics.warnings().count(), 1);
}

#[test]
fn imports_resolve_through_loader() {
    use std::path::Path;
    let mut loader = |p: &Path| -> std::io::Result<String> {
        assert!(p.ends_with("widgets/badge.ui"), "{p:?}");
        Ok("export component Badge inherits Rectangle { in property <string> label; Text { text: root.label; } }".into())
    };
    let c = crate::compile_with_loader(
        Path::new("app/main.ui"),
        r#"import { Badge } from "../widgets/badge.ui";
           export component App { b := Badge { label: "new"; } }"#
            .into(),
        &mut loader,
    )
    .unwrap_or_else(|d| panic!("{d}"));
    let app = c.instantiate();
    assert_eq!(
        app.element_property("b", "label").unwrap(),
        Value::from("new")
    );
}

#[test]
fn codegen_emits_typed_api() {
    let c = compile_source("counter.ui", COUNTER).unwrap();
    let code = crate::generate_rust(&c, "::kite", "counter.ui");
    assert!(code.contains("pub struct Counter"));
    assert!(code.contains("pub fn get_counter(&self) -> i32"));
    assert!(code.contains("pub fn set_counter(&self, value: i32)"));
    assert!(code.contains("pub fn get_label(&self)"));
    assert!(
        !code.contains("pub fn set_label"),
        "out properties have no setter"
    );
    assert!(code.contains("pub fn on_reset(&self"));
    assert!(code.contains("pub fn invoke_reset(&self)"));
}
