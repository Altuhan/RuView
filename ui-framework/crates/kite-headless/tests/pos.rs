//! Drives the POS example (examples/ui/pos.ui) through a complete sale.

use kite_core::{Key, Value};
use kite_headless::Harness;

fn float(ui: &Harness, name: &str) -> f32 {
    ui.instance.get_property(name).unwrap().as_float().unwrap()
}

#[test]
fn rings_up_a_cash_sale() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/ui/pos.ui");
    let compiled = kite_compiler::compile_file(path).unwrap_or_else(|d| panic!("{d}"));
    let app = compiled.instantiate();
    let sales = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = sales.clone();
    app.on_callback("checkout", move |_, args| {
        log.borrow_mut().push(args.to_vec())
    })
    .unwrap();
    let mut ui = Harness::auto_sized(app);
    let shots = std::env::var("KITE_POS_SHOTS").ok();
    let shot = |ui: &Harness, name: &str| {
        if let Some(dir) = &shots {
            ui.screenshot()
                .save_png(format!("{dir}/pos-{name}.png"))
                .unwrap();
        }
    };
    shot(&ui, "0-empty");

    // Two sourdough loaves and a coffee by tapping tiles.
    ui.click("sourdough");
    ui.click("sourdough");
    ui.click("coffee");
    // Bananas by PLU code.
    ui.click_at(kite_core::Point::new(150.0, 76.0));
    ui.type_text("4011");
    ui.press(Key::Enter);
    assert_eq!(ui.instance.get_property("items").unwrap(), Value::Int(4));
    let subtotal = float(&ui, "subtotal");
    assert!(
        (subtotal - (2.0 * 5.50 + 11.99 + 0.69)).abs() < 0.001,
        "{subtotal}"
    );

    ui.instance.set_property("member", true).unwrap();
    let total = float(&ui, "total");
    let expected = ((subtotal * 0.9 * 1.08) * 100.0).round() / 100.0;
    assert!((total - expected).abs() < 0.001, "{total} vs {expected}");
    shot(&ui, "1-cart");

    ui.click("charge");
    assert_eq!(
        ui.instance.get_property("screen").unwrap(),
        Value::from("pay")
    );
    shot(&ui, "2-pay");
    // Pay cash with $50.
    ui.click("cash-chip");
    ui.click("tender-fifty");
    let change = float(&ui, "change");
    assert!((change - (50.0 - total)).abs() < 0.001);
    shot(&ui, "3-cash");
    ui.click("complete");
    assert_eq!(
        ui.instance.get_property("screen").unwrap(),
        Value::from("done")
    );
    assert_eq!(sales.borrow().len(), 1);
    assert_eq!(sales.borrow()[0][1], Value::from("cash"));
    shot(&ui, "4-done");

    ui.click("new-sale");
    assert_eq!(ui.instance.get_property("items").unwrap(), Value::Int(0));
    assert_eq!(
        ui.instance.get_property("screen").unwrap(),
        Value::from("sale")
    );
}
