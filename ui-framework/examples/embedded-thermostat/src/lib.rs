//! A `no_std` thermostat UI for a 320×240 display.
//!
//! The firmware owns the display driver and touch controller; this crate
//! only needs a `DrawTarget`. A typical main loop:
//!
//! ```ignore
//! let ui = kite_example_thermostat::Thermostat::new();
//! let mut touch = kite_embedded::Touch::new();
//! loop {
//!     ui.set_current(sensor.read_celsius());
//!     touch.update(ui.instance(), touch_controller.sample());
//!     kite_embedded::render(ui.instance(), &mut display, 16, false)?;
//!     if ui.get_heating() { relay.set_high() } else { relay.set_low() }
//!     delay.delay_ms(16);
//! }
//! ```
#![no_std]

include!(concat!(env!("OUT_DIR"), "/thermostat.rs"));

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use embedded_graphics_core::draw_target::DrawTarget;
    use embedded_graphics_core::geometry::{OriginDimensions, Size};
    use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
    use embedded_graphics_core::Pixel;
    use std::vec::Vec;

    /// In-memory RGB565 display, like an ST7789 framebuffer.
    struct FakeDisplay {
        pixels: Vec<Rgb565>,
        writes: usize,
    }

    impl OriginDimensions for FakeDisplay {
        fn size(&self) -> Size {
            Size::new(320, 240)
        }
    }

    impl DrawTarget for FakeDisplay {
        type Color = Rgb565;
        type Error = core::convert::Infallible;
        fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
            &mut self,
            pixels: I,
        ) -> Result<(), Self::Error> {
            for Pixel(p, c) in pixels {
                self.pixels[(p.y * 320 + p.x) as usize] = c;
                self.writes += 1;
            }
            Ok(())
        }
    }

    #[test]
    fn renders_in_bands_and_reacts_to_touch() {
        let ui = Thermostat::new();
        let mut display = FakeDisplay {
            pixels: std::vec![Rgb565::BLACK; 320 * 240],
            writes: 0,
        };
        assert!(kite_embedded::render(ui.instance(), &mut display, 16, false).unwrap());
        assert_eq!(display.writes, 320 * 240);
        // Background is the dark slate rectangle.
        let bg = display.pixels[5 * 320 + 5];
        assert!(bg.b() > bg.r(), "{bg:?}");
        // Nothing changed: no redraw.
        assert!(!kite_embedded::render(ui.instance(), &mut display, 16, false).unwrap());

        let up = ui.instance().element_geometry("up").unwrap();
        let mut touch = kite_embedded::Touch::new();
        touch.update(ui.instance(), Some((up.x + 5.0, up.y + 5.0)));
        touch.update(ui.instance(), None);
        assert_eq!(ui.get_target(), 22.5);
        ui.set_current(18.0);
        assert!(ui.get_heating());
        assert!(kite_embedded::render(ui.instance(), &mut display, 16, false).unwrap());
    }
}
