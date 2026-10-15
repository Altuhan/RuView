//! Times compile, layout and paint of the POS example: `cargo run --release -p kite-headless --example bench_pos`
use std::time::Instant;

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/ui/pos.ui");
    let t = Instant::now();
    let compiled = kite_compiler::compile_file(path).unwrap_or_else(|d| panic!("{d}"));
    println!("compile        {:>8.2} ms", t.elapsed().as_secs_f64() * 1e3);
    let app = compiled.instantiate();
    app.set_size(kite_core::Size::new(980.0, 720.0));
    let t = Instant::now();
    let _ = app.element_geometry("charge");
    println!("first layout   {:>8.2} ms", t.elapsed().as_secs_f64() * 1e3);
    for scale in [1.0f32, 2.0] {
        let mut buf = kite_core::RgbBuffer::new((980.0 * scale) as u32, (720.0 * scale) as u32);
        let t = Instant::now();
        app.render_scaled(&mut buf, scale);
        println!(
            "paint @{scale}x     {:>8.2} ms",
            t.elapsed().as_secs_f64() * 1e3
        );
    }
    let g = app.element_geometry("milk").unwrap();
    let p = kite_core::Point::new(g.x + 5.0, g.y + 5.0);
    let t = Instant::now();
    app.dispatch_event(kite_core::Event::PointerPressed {
        position: p,
        button: kite_core::PointerButton::Left,
    });
    app.dispatch_event(kite_core::Event::PointerReleased {
        position: p,
        button: kite_core::PointerButton::Left,
    });
    let _ = app.element_geometry("charge");
    println!("tap + relayout {:>8.2} ms", t.elapsed().as_secs_f64() * 1e3);
}
