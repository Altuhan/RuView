//! # kite-embedded
//!
//! Runs Kite UIs on microcontrollers (`no_std` + `alloc`).
//!
//! * [`render`] draws into any [`embedded_graphics_core`] `DrawTarget` a band
//!   of rows at a time, so a 320×240 display needs only `width × band_rows`
//!   pixels of RAM instead of a full framebuffer.
//! * [`Touch`] turns raw touch-controller samples into Kite pointer events.
//! * For memory-mapped framebuffers use [`kite_core::Rgb565Buffer`] directly.
//!
//! You need a global allocator (e.g. `esp-alloc` or `embedded-alloc`).
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Point as EgPoint, Size as EgSize};
use embedded_graphics_core::pixelcolor::Rgb888;
use embedded_graphics_core::primitives::Rectangle;
use kite_core::{ComponentInstance, Event, Point, PointerButton, RgbBuffer, Size};

/// Renders the whole UI into `display`, `band_rows` rows at a time.
///
/// Sets the component size to the display size first. Returns early without
/// drawing when nothing changed since the last render (unless `force`).
pub fn render<D>(
    instance: &ComponentInstance,
    display: &mut D,
    band_rows: u32,
    force: bool,
) -> Result<bool, D::Error>
where
    D: DrawTarget + OriginDimensions,
    D::Color: From<Rgb888>,
{
    let EgSize { width, height } = display.size();
    instance.set_size(Size::new(width as f32, height as f32));
    if !force && !instance.needs_redraw() {
        return Ok(false);
    }
    let rows = band_rows.clamp(1, height.max(1));
    let mut band = RgbBuffer::new(width, rows);
    let mut y = 0;
    while y < height {
        let h = rows.min(height - y);
        band.resize(width, h);
        instance.render_band(&mut band, y as f32, 1.0);
        let area = Rectangle::new(EgPoint::new(0, y as i32), EgSize::new(width, h));
        let colors = band
            .pixels
            .iter()
            .map(|p| D::Color::from(Rgb888::new((p >> 16) as u8, (p >> 8) as u8, *p as u8)));
        display.fill_contiguous(&area, colors)?;
        y += h;
    }
    Ok(true)
}

/// Converts touch-controller samples (pressed + position) into pointer events.
#[derive(Debug, Default, Clone, Copy)]
pub struct Touch {
    last: Option<Point>,
}

impl Touch {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Feeds one sample: `Some((x, y))` while touched, `None` when released.
    pub fn update(&mut self, instance: &ComponentInstance, sample: Option<(f32, f32)>) {
        match (self.last, sample) {
            (None, Some((x, y))) => {
                let position = Point::new(x, y);
                instance.dispatch_event(Event::PointerMoved { position });
                instance.dispatch_event(Event::PointerPressed {
                    position,
                    button: PointerButton::Left,
                });
                self.last = Some(position);
            }
            (Some(prev), Some((x, y))) => {
                let position = Point::new(x, y);
                if position != prev {
                    instance.dispatch_event(Event::PointerMoved { position });
                    self.last = Some(position);
                }
            }
            (Some(prev), None) => {
                instance.dispatch_event(Event::PointerReleased {
                    position: prev,
                    button: PointerButton::Left,
                });
                instance.dispatch_event(Event::PointerExited);
                self.last = None;
            }
            (None, None) => {}
        }
    }
}

/// Re-export so applications don't need a separate dependency for colors.
pub use embedded_graphics_core::pixelcolor;
