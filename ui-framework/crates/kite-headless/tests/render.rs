use kite_core::Color;
use kite_headless::{render, Harness};

fn compile(src: &str) -> kite_core::ComponentInstance {
    kite_compiler::compile_source("t.ui", src).unwrap_or_else(|d| panic!("{d}")).instantiate()
}

#[test]
fn renders_rectangles_at_their_geometry() {
    let app = compile(
        r#"export component A inherits Window {
            width: 100px; height: 60px; background: #ffffff;
            r := Rectangle { x: 10px; y: 10px; width: 30px; height: 20px; background: #ff0000; }
        }"#,
    );
    let img = render(&app, 1.0);
    assert_eq!((img.width(), img.height()), (100, 60));
    assert_eq!(img.pixel(20, 20), Color::rgb(255, 0, 0));
    assert_eq!(img.pixel(50, 20), Color::WHITE);
    let img2 = render(&app, 2.0);
    assert_eq!((img2.width(), img2.height()), (200, 120));
    assert_eq!(img2.pixel(40, 40), Color::rgb(255, 0, 0));
}

#[test]
fn reactive_changes_show_up_in_the_next_frame() {
    let app = compile(
        r#"export component A inherits Window {
            in-out property <bool> on;
            width: 40px; height: 40px;
            TouchArea { clicked => { on = !on; } }
            Rectangle { background: on ? #00ff00 : #0000ff; }
        }"#,
    );
    let mut ui = Harness::new(app, 40.0, 40.0);
    assert_eq!(ui.screenshot().pixel(5, 5), Color::rgb(0, 0, 255));
    ui.click_at(kite_core::Point::new(20.0, 20.0));
    assert!(ui.instance.needs_redraw());
    assert_eq!(ui.screenshot().pixel(5, 5), Color::rgb(0, 255, 0));
}

#[test]
fn png_encoding_produces_a_valid_header() {
    let app = compile("export component A inherits Window { width: 8px; height: 8px; }");
    let png = render(&app, 1.0).to_png().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}
