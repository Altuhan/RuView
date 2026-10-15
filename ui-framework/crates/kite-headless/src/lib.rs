//! # kite-headless
//!
//! Renders Kite components without a window — for screenshot tests, CI,
//! documentation images and server-side rendering — and provides helpers to
//! drive components with simulated input.
//!
//! ```no_run
//! # fn demo(app: &kite_core::ComponentInstance) -> std::io::Result<()> {
//! let mut ui = kite_headless::Harness::new(app.clone(), 320.0, 240.0);
//! ui.click("increment");
//! ui.screenshot().save_png("counter.png")?;
//! # Ok(()) }
//! ```

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use kite_core::{Color, ComponentInstance, Event, Key, Point, PointerButton, RgbBuffer, Size};

/// A rendered frame.
pub struct Image {
    pub buffer: RgbBuffer,
}

impl Image {
    pub fn width(&self) -> u32 {
        self.buffer.width
    }

    pub fn height(&self) -> u32 {
        self.buffer.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> Color {
        self.buffer.pixel(x, y)
    }

    /// Encodes the image as PNG bytes.
    pub fn to_png(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.buffer.width, self.buffer.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(io::Error::other)?;
            w.write_image_data(&self.buffer.to_rgba8()).map_err(io::Error::other)?;
        }
        Ok(out)
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let file = BufWriter::new(File::create(path)?);
        let mut enc = png::Encoder::new(file, self.buffer.width, self.buffer.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(io::Error::other)?;
        w.write_image_data(&self.buffer.to_rgba8()).map_err(io::Error::other)
    }
}

/// Renders `instance` at its current size (or preferred size if unsized) and `scale`.
pub fn render(instance: &ComponentInstance, scale: f32) -> Image {
    let mut size = instance.size();
    if size.width <= 0.0 || size.height <= 0.0 {
        size = instance.preferred_size();
        instance.set_size(size);
    }
    let w = (size.width * scale).ceil().max(1.0) as u32;
    let h = (size.height * scale).ceil().max(1.0) as u32;
    let mut buffer = RgbBuffer::new(w, h);
    instance.render_scaled(&mut buffer, scale);
    Image { buffer }
}

/// Drives a component like a user would.
pub struct Harness {
    pub instance: ComponentInstance,
    pub scale: f32,
}

impl Harness {
    pub fn new(instance: ComponentInstance, width: f32, height: f32) -> Self {
        instance.set_size(Size::new(width, height));
        Self { instance, scale: 1.0 }
    }

    /// Uses the component's preferred size.
    pub fn auto_sized(instance: ComponentInstance) -> Self {
        let s = instance.preferred_size();
        Self::new(instance, s.width, s.height)
    }

    fn center(&self, id: &str) -> Point {
        let g = self
            .instance
            .element_geometry(id)
            .unwrap_or_else(|e| panic!("cannot find element `{id}`: {e}"));
        Point::new(g.x + g.width / 2.0, g.y + g.height / 2.0)
    }

    pub fn click_at(&mut self, position: Point) {
        self.instance.dispatch_event(Event::PointerMoved { position });
        self.instance.dispatch_event(Event::PointerPressed { position, button: PointerButton::Left });
        self.instance.dispatch_event(Event::PointerReleased { position, button: PointerButton::Left });
    }

    /// Clicks the center of the element with `id`.
    pub fn click(&mut self, id: &str) {
        let p = self.center(id);
        self.click_at(p);
    }

    pub fn hover(&mut self, id: &str) {
        let position = self.center(id);
        self.instance.dispatch_event(Event::PointerMoved { position });
    }

    /// Types text into the focused element.
    pub fn type_text(&mut self, text: &str) {
        self.instance.dispatch_event(Event::Text(text.into()));
    }

    pub fn press(&mut self, key: Key) {
        self.instance.dispatch_event(Event::KeyPressed(key));
    }

    pub fn screenshot(&self) -> Image {
        render(&self.instance, self.scale)
    }
}
