//! A small anti-aliased software rasterizer that works without `std`.
//!
//! Drawing goes through [`Target`], so the same code renders into a desktop
//! window buffer, a WASM canvas, or an MCU framebuffer (optionally a band of
//! a few lines at a time to save RAM).

use alloc::vec;
use alloc::vec::Vec;

use crate::color::Color;
use crate::font;
use crate::geometry::{Point, Rect};

/// A pixel surface the rasterizer can blend into.
pub trait Target {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    /// Source-over blend `color` with additional `coverage` (0..=255) at (x, y).
    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: u8);
    /// Fill a horizontal span `[x0, x1)` on row `y` with full coverage.
    fn fill_span(&mut self, x0: u32, x1: u32, y: u32, color: Color) {
        for x in x0..x1 {
            self.blend(x, y, color, 255);
        }
    }
}

/// An owned 32-bit `0x00RRGGBB` buffer (the layout softbuffer and most canvases use).
#[derive(Clone)]
pub struct RgbBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl RgbBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height) as usize],
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Color {
        Color::from_rgb_u32(self.pixels[(y * self.width + x) as usize])
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels.clear();
        self.pixels.resize((width * height) as usize, 0);
    }

    /// RGBA8 bytes (e.g. for PNG encoding or `ImageData`).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for p in &self.pixels {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
        }
        out
    }
}

impl Target for RgbBuffer {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: u8) {
        let i = (y * self.width + x) as usize;
        let dst = Color::from_rgb_u32(self.pixels[i]);
        self.pixels[i] = dst.blend_under(color, coverage).to_argb_u32() & 0x00ff_ffff;
    }
    fn fill_span(&mut self, x0: u32, x1: u32, y: u32, color: Color) {
        if color.a == 255 {
            let row = (y * self.width) as usize;
            let v = color.to_argb_u32() & 0x00ff_ffff;
            self.pixels[row + x0 as usize..row + x1 as usize].fill(v);
        } else {
            for x in x0..x1 {
                self.blend(x, y, color, 255);
            }
        }
    }
}

/// A borrowed `0x00RRGGBB` buffer (e.g. a softbuffer surface).
pub struct Rgb32Buffer<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u32],
}

impl Target for Rgb32Buffer<'_> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: u8) {
        let i = (y * self.width + x) as usize;
        let dst = Color::from_rgb_u32(self.pixels[i]);
        self.pixels[i] = dst.blend_under(color, coverage).to_argb_u32() & 0x00ff_ffff;
    }
    fn fill_span(&mut self, x0: u32, x1: u32, y: u32, color: Color) {
        let row = (y * self.width) as usize;
        if color.a == 255 {
            self.pixels[row + x0 as usize..row + x1 as usize]
                .fill(color.to_argb_u32() & 0x00ff_ffff);
        } else {
            for x in x0..x1 {
                self.blend(x, y, color, 255);
            }
        }
    }
}

/// A borrowed RGB565 framebuffer, the common format of small SPI displays.
pub struct Rgb565Buffer<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u16],
}

impl Target for Rgb565Buffer<'_> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    fn blend(&mut self, x: u32, y: u32, color: Color, coverage: u8) {
        let i = (y * self.width + x) as usize;
        let dst = Color::from_rgb565(self.pixels[i]);
        self.pixels[i] = dst.blend_under(color, coverage).to_rgb565();
    }
}

/// Draws primitives onto a [`Target`], with clipping and a vertical offset
/// (so a target can hold just a band of the full frame).
pub struct Canvas<'a> {
    target: &'a mut dyn Target,
    /// Global y coordinate (in device pixels) of the target's first row.
    offset_y: f32,
    /// Device pixels per logical pixel.
    scale: f32,
    /// Clip in device pixels.
    clip: Rect,
    clip_stack: Vec<Rect>,
}

#[inline]
fn cov(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Signed distance from `p` to a rounded box centered at `c` with half extents `h`.
#[inline]
fn sd_round_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = libm::fabsf(px - cx) - (hx - r);
    let qy = libm::fabsf(py - cy) - (hy - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    libm::sqrtf(ox * ox + oy * oy) + qx.max(qy).min(0.0) - r
}

impl<'a> Canvas<'a> {
    pub fn new(target: &'a mut dyn Target) -> Self {
        Self::with_offset(target, 0.0)
    }

    /// The target represents rows `offset_y .. offset_y + target.height()` of the frame.
    pub fn with_offset(target: &'a mut dyn Target, offset_y: f32) -> Self {
        Self::with_transform(target, offset_y, 1.0)
    }

    /// Like [`with_offset`](Self::with_offset), drawing logical coordinates at
    /// `scale` device pixels each (HiDPI). `offset_y` is in device pixels.
    pub fn with_transform(target: &'a mut dyn Target, offset_y: f32, scale: f32) -> Self {
        let clip = Rect::new(0.0, offset_y, target.width() as f32, target.height() as f32);
        Self {
            target,
            offset_y,
            scale: scale.max(0.01),
            clip,
            clip_stack: Vec::new(),
        }
    }

    #[inline]
    fn dev(&self, r: Rect) -> Rect {
        let s = self.scale;
        Rect::new(r.x * s, r.y * s, r.width * s, r.height * s)
    }

    /// The part of the frame this canvas can draw on, in logical coordinates.
    pub fn bounds(&self) -> Rect {
        let s = self.scale;
        Rect::new(
            self.clip.x / s,
            self.clip.y / s,
            self.clip.width / s,
            self.clip.height / s,
        )
    }

    pub fn push_clip(&mut self, r: Rect) {
        self.clip_stack.push(self.clip);
        self.clip = self.clip.intersect(&self.dev(r));
    }

    pub fn pop_clip(&mut self) {
        if let Some(c) = self.clip_stack.pop() {
            self.clip = c;
        }
    }

    /// Integer pixel range of `r` intersected with the clip.
    fn pixel_range(&self, r: Rect) -> Option<(u32, u32, u32, u32)> {
        let c = self.clip.intersect(&r);
        if c.is_empty() {
            return None;
        }
        let x0 = libm::floorf(c.x).max(0.0) as u32;
        let x1 = (libm::ceilf(c.right()) as u32).min(self.target.width());
        let y0 = libm::floorf(c.y - self.offset_y).max(0.0) as u32;
        let y1 =
            (libm::ceilf(c.bottom() - self.offset_y).max(0.0) as u32).min(self.target.height());
        (x0 < x1 && y0 < y1).then_some((x0, x1, y0, y1))
    }

    /// Fraction of pixel `[p, p+1)` covered by the clip interval (for soft clip edges).
    fn clip_cov(&self, x: u32, y: u32) -> f32 {
        let gx = x as f32;
        let gy = y as f32 + self.offset_y;
        let fx = cov(self.clip.right() - gx).min(cov(gx + 1.0 - self.clip.x));
        let fy = cov(self.clip.bottom() - gy).min(cov(gy + 1.0 - self.clip.y));
        fx * fy
    }

    #[inline]
    fn put(&mut self, x: u32, y: u32, color: Color, c: f32) {
        let c = c * self.clip_cov(x, y);
        if c > 0.002 {
            self.target.blend(x, y, color, (c * 255.0 + 0.5) as u8);
        }
    }

    /// Fill a (rounded) rectangle with anti-aliased edges.
    pub fn fill_rect(&mut self, r: Rect, radius: f32, color: Color) {
        if color.a == 0 || r.is_empty() {
            return;
        }
        let (r, radius) = (self.dev(r), radius * self.scale);
        let Some((x0, x1, y0, y1)) = self.pixel_range(r) else {
            return;
        };
        let radius = radius.clamp(0.0, r.width.min(r.height) / 2.0);
        let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
        let (hx, hy) = (r.width / 2.0, r.height / 2.0);
        for y in y0..y1 {
            let gy = y as f32 + self.offset_y;
            let in_corner_band = gy < r.y + radius || gy + 1.0 > r.bottom() - radius;
            let row_cov = cov(gy + 1.0 - r.y).min(cov(r.bottom() - gy));
            if in_corner_band && radius > 0.5 {
                for x in x0..x1 {
                    let d = sd_round_box(x as f32 + 0.5, gy + 0.5, cx, cy, hx, hy, radius);
                    self.put(x, y, color, cov(0.5 - d));
                }
                continue;
            }
            // Straight band: only edge pixels are partial; fill full runs as spans.
            let pixel_cov = |s: &Self, x: u32| {
                let gx = x as f32;
                cov(gx + 1.0 - r.x).min(cov(r.right() - gx)) * row_cov * s.clip_cov(x, y)
            };
            let mut x = x0;
            while x < x1 {
                let c = pixel_cov(self, x);
                if c >= 1.0 {
                    let mut end = x + 1;
                    while end < x1 && pixel_cov(self, end) >= 1.0 {
                        end += 1;
                    }
                    self.target.fill_span(x, end, y, color);
                    x = end;
                } else {
                    if c > 0.002 {
                        self.target.blend(x, y, color, (c * 255.0 + 0.5) as u8);
                    }
                    x += 1;
                }
            }
        }
    }

    /// Stroke the inside of a (rounded) rectangle's outline.
    pub fn stroke_rect(&mut self, r: Rect, radius: f32, width: f32, color: Color) {
        if color.a == 0 || width <= 0.0 || r.is_empty() {
            return;
        }
        let (r, radius, width) = (self.dev(r), radius * self.scale, width * self.scale);
        let Some((x0, x1, y0, y1)) = self.pixel_range(r) else {
            return;
        };
        let radius = radius.clamp(0.0, r.width.min(r.height) / 2.0);
        let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
        let (hx, hy) = (r.width / 2.0, r.height / 2.0);
        for y in y0..y1 {
            let gy = y as f32 + self.offset_y + 0.5;
            for x in x0..x1 {
                let d = sd_round_box(x as f32 + 0.5, gy, cx, cy, hx, hy, radius);
                let c = cov(0.5 - d) * cov(d + width + 0.5);
                if c > 0.0 {
                    self.put(x, y, color, c);
                }
            }
        }
    }

    /// An anti-aliased line with round caps.
    pub fn line(&mut self, a: Point, b: Point, width: f32, color: Color) {
        let s = self.scale;
        let (a, b, width) = (
            Point::new(a.x * s, a.y * s),
            Point::new(b.x * s, b.y * s),
            width * s,
        );
        let hw = width / 2.0;
        let bounds = Rect::new(
            a.x.min(b.x) - hw - 1.0,
            a.y.min(b.y) - hw - 1.0,
            libm::fabsf(a.x - b.x) + width + 2.0,
            libm::fabsf(a.y - b.y) + width + 2.0,
        );
        let Some((x0, x1, y0, y1)) = self.pixel_range(bounds) else {
            return;
        };
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in y0..y1 {
            let py = y as f32 + self.offset_y + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let t = (((px - a.x) * dx + (py - a.y) * dy) / len2).clamp(0.0, 1.0);
                let (ex, ey) = (px - (a.x + t * dx), py - (a.y + t * dy));
                let d = libm::sqrtf(ex * ex + ey * ey) - hw;
                self.put(x, y, color, cov(0.5 - d));
            }
        }
    }

    /// Draw one line of text with its baseline at `baseline`; returns the advance.
    pub fn text(&mut self, x: f32, baseline: f32, text: &str, size: f32, color: Color) -> f32 {
        let s = self.scale;
        let logical_width = font::line_width(text, size);
        let (x, baseline, size) = (x * s, baseline * s, size * s);
        self.text_device(x, baseline, text, size, color);
        logical_width
    }

    fn text_device(&mut self, x: f32, baseline: f32, text: &str, size: f32, color: Color) {
        let scale = size / font::BASE_SIZE;
        let samples = libm::ceilf(1.0 / scale).clamp(1.0, 4.0) as u32;
        let inv = 1.0 / samples as f32;
        let mut pen = x;
        let line_rect = Rect::new(
            x,
            baseline - font::ascent(size) - 2.0,
            1e9,
            font::line_height(size) + 4.0,
        );
        if self.clip.intersect(&line_rect).is_empty() {
            return;
        }
        for ch in text.chars() {
            let g = font::glyph(ch);
            if g.width > 0 {
                let ox = pen + g.bearing_x as f32 * scale;
                let oy = baseline + g.bearing_y as f32 * scale;
                let pad = if samples == 1 { 1.0 } else { 0.0 };
                let dest = Rect::new(
                    ox - pad,
                    oy - pad,
                    g.width as f32 * scale + 2.0 * pad,
                    g.height as f32 * scale + 2.0 * pad,
                );
                if let Some((x0, x1, y0, y1)) = self.pixel_range(dest) {
                    for y in y0..y1 {
                        let gy = y as f32 + self.offset_y;
                        for x in x0..x1 {
                            if samples == 1 {
                                // Upscaling: bilinear interpolation of the atlas.
                                let u = (x as f32 + 0.5 - ox) / scale - 0.5;
                                let v = (gy + 0.5 - oy) / scale - 0.5;
                                // Re-sharpen: bilinear alpha ramps over `scale` device pixels;
                                // remap so edges stay ~1 device pixel wide.
                                let a = ((bilinear(&g, u, v) - 0.5) * scale + 0.5).clamp(0.0, 1.0);
                                if a > 0.0 {
                                    self.put(x, y, color, a);
                                }
                                continue;
                            }
                            let mut acc = 0u32;
                            for sy in 0..samples {
                                let v = ((gy + (sy as f32 + 0.5) * inv - oy) / scale) as i32;
                                if v < 0 || v >= g.height as i32 {
                                    continue;
                                }
                                for sx in 0..samples {
                                    let u =
                                        ((x as f32 + (sx as f32 + 0.5) * inv - ox) / scale) as i32;
                                    if u >= 0 && u < g.width as i32 {
                                        acc += g.bitmap[(v as u32 * g.width + u as u32) as usize]
                                            as u32;
                                    }
                                }
                            }
                            let a = acc as f32 / (samples * samples * 255) as f32;
                            if a > 0.0 {
                                self.put(x, y, color, a);
                            }
                        }
                    }
                }
            }
            pen += g.advance * scale;
        }
    }
}

fn bilinear(g: &font::Glyph, u: f32, v: f32) -> f32 {
    let (x0, y0) = (libm::floorf(u), libm::floorf(v));
    let (fx, fy) = (u - x0, v - y0);
    let at = |x: f32, y: f32| -> f32 {
        if x < 0.0 || y < 0.0 || x >= g.width as f32 || y >= g.height as f32 {
            0.0
        } else {
            g.bitmap[(y as u32 * g.width + x as u32) as usize] as f32 / 255.0
        }
    };
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
    let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
    top * (1.0 - fy) + bottom * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_and_clips() {
        let mut buf = RgbBuffer::new(20, 20);
        {
            let mut c = Canvas::new(&mut buf);
            c.fill_rect(Rect::new(0.0, 0.0, 20.0, 20.0), 0.0, Color::WHITE);
            c.push_clip(Rect::new(0.0, 0.0, 10.0, 20.0));
            c.fill_rect(Rect::new(5.0, 5.0, 10.0, 10.0), 0.0, Color::rgb(255, 0, 0));
            c.pop_clip();
        }
        assert_eq!(buf.pixel(7, 7), Color::rgb(255, 0, 0));
        assert_eq!(buf.pixel(12, 7), Color::WHITE, "clipped");
        assert_eq!(buf.pixel(2, 2), Color::WHITE);
    }

    #[test]
    fn rounded_corners_are_antialiased() {
        let mut buf = RgbBuffer::new(20, 20);
        Canvas::new(&mut buf).fill_rect(Rect::new(0.0, 0.0, 20.0, 20.0), 8.0, Color::WHITE);
        assert_eq!(buf.pixel(0, 0), Color::BLACK);
        assert_eq!(buf.pixel(10, 10), Color::WHITE);
        let partial = (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let p = buf.pixel(*x, *y);
                p.r > 0 && p.r < 255
            });
        assert!(
            partial.count() >= 4,
            "corner should have anti-aliased pixels"
        );
    }

    #[test]
    fn band_rendering_matches_full_frame() {
        let draw = |c: &mut Canvas| {
            c.fill_rect(Rect::new(2.5, 3.0, 13.0, 9.0), 4.0, Color::rgb(10, 200, 30));
            c.text(1.0, 15.0, "Hi", 12.0, Color::WHITE);
        };
        let mut full = RgbBuffer::new(16, 16);
        draw(&mut Canvas::new(&mut full));
        for band_start in (0..16).step_by(4) {
            let mut band = RgbBuffer::new(16, 4);
            draw(&mut Canvas::with_offset(&mut band, band_start as f32));
            for y in 0..4 {
                for x in 0..16 {
                    assert_eq!(band.pixel(x, y), full.pixel(x, band_start + y));
                }
            }
        }
    }

    #[test]
    fn scaled_drawing_covers_device_pixels() {
        let mut buf = RgbBuffer::new(20, 20);
        Canvas::with_transform(&mut buf, 0.0, 2.0).fill_rect(
            Rect::new(1.0, 1.0, 4.0, 4.0),
            0.0,
            Color::WHITE,
        );
        assert_eq!(buf.pixel(2, 2), Color::WHITE);
        assert_eq!(buf.pixel(9, 9), Color::WHITE);
        assert_eq!(buf.pixel(10, 10), Color::BLACK);
    }

    #[test]
    fn text_draws_pixels() {
        let mut buf = RgbBuffer::new(40, 20);
        let adv = Canvas::new(&mut buf).text(2.0, 15.0, "Kite", 14.0, Color::WHITE);
        assert!(adv > 15.0);
        assert!(buf.pixels.iter().any(|p| *p != 0));
    }
}
