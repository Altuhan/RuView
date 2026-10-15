//! Desktop counter: `cargo run -p kite-example-counter`

kite::include_ui!("counter");

use kite::prelude::*;

/// Wires the Rust side of the UI.
fn setup() -> Counter {
    let app = Counter::new();
    app.set_step(2);
    app.on_request_increase(|app| {
        // Business logic lives in Rust; the UI only asks for it.
        let next = (app.get_counter() + app.get_step()).min(10);
        app.set_counter(next);
    });
    app
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup().run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kite::headless::Harness;

    #[test]
    fn increments_through_rust_and_ui_logic() {
        let app = setup();
        let mut ui = Harness::new(app.instance().clone(), 320.0, 200.0);
        ui.click("plus");
        ui.click("plus");
        assert_eq!(app.get_counter(), 4);
        ui.click("minus");
        assert_eq!(app.get_counter(), 2);
        for _ in 0..10 {
            ui.click("plus");
        }
        assert_eq!(app.get_counter(), 10);
        assert!(app.get_at_limit());
        ui.click("reset");
        assert_eq!(app.get_counter(), 0);
        // Disabled "-" at zero does nothing.
        ui.click("minus");
        assert_eq!(app.get_counter(), 0);
    }

    #[test]
    fn renders() {
        let app = setup();
        app.set_counter(7);
        let ui = Harness::new(app.instance().clone(), 320.0, 200.0);
        let img = ui.screenshot();
        assert_eq!((img.width(), img.height()), (320, 200));
    }
}
