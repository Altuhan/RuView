//! The counter example running in the browser. See the README for build steps.

kite::include_ui!("counter");

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

thread_local! {
    static APP: RefCell<Option<Counter>> = const { RefCell::new(None) };
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let app = Counter::new();
    app.on_request_increase(|app| app.set_counter((app.get_counter() + app.get_step()).min(10)));
    std::mem::forget(kite_web::mount_by_id(app.instance(), "app")?);
    APP.with(|a| *a.borrow_mut() = Some(app));
    Ok(())
}

/// Current counter value (lets the page and browser tests observe state).
#[wasm_bindgen]
pub fn counter_value() -> i32 {
    APP.with(|a| {
        a.borrow()
            .as_ref()
            .map(|app| app.get_counter())
            .unwrap_or(-1)
    })
}

/// Geometry `[x, y, width, height]` of the element with `id` (for tests).
#[wasm_bindgen]
pub fn element_rect(id: &str) -> Vec<f32> {
    APP.with(|a| {
        a.borrow()
            .as_ref()
            .and_then(|app| app.instance().element_geometry(id).ok())
            .map(|g| vec![g.x, g.y, g.width, g.height])
            .unwrap_or_default()
    })
}
