//! Painting of the built-in elements (the default "Kite" style).

use super::input::INPUT_PADDING;
use super::State;
use crate::color::Color;
use crate::elements::{self as el, ElementKind};
use crate::font;
use crate::geometry::{Point, Rect};
use crate::raster::Canvas;

const ACCENT: Color = Color::from_rgb_u32(0x2563eb);
const BORDER: Color = Color::from_rgb_u32(0xc9c9d1);
const SURFACE: Color = Color::WHITE;
const TEXT: Color = Color::from_rgb_u32(0x1f1f1f);
const MUTED: Color = Color::from_rgb_u32(0x9a9aa5);
const DISABLED_BG: Color = Color::from_rgb_u32(0xe6e6ea);
const RADIUS: f32 = 6.0;

fn focus_ring(c: &mut Canvas, r: Rect, radius: f32, op: f32) {
    let ring = Rect::new(r.x - 3.0, r.y - 3.0, r.width + 6.0, r.height + 6.0);
    c.stroke_rect(ring, radius + 3.0, 2.0, ACCENT.fade(0.55 * op));
}

impl State {
    pub(crate) fn paint(&mut self, c: &mut Canvas) {
        let bg = if self.kind(0) == ElementKind::Window {
            self.named(0, "background")
                .as_color()
                .unwrap_or(Color::WHITE)
        } else {
            Color::WHITE
        };
        let b = c.bounds();
        c.fill_rect(b, 0.0, bg.with_alpha(255));
        self.paint_element(0, c, 1.0);
    }

    fn paint_element(&mut self, e: u32, c: &mut Canvas, opacity: f32) {
        if !self.get_b(e, el::VISIBLE) {
            return;
        }
        let op = opacity * self.get_f(e, el::OPACITY).clamp(0.0, 1.0);
        if op <= 0.0 {
            return;
        }
        let r = self.geometry[e as usize];
        if r.intersect(&c.bounds()).is_empty() && !self.kind(e).fills_parent() {
            // Off-band leaf content; containers may still have overflowing children.
            if self.def.elements[e as usize].children.is_empty() {
                return;
            }
        }
        let mut clipped = false;
        let focused = self.focused == Some(e);
        match self.kind(e) {
            ElementKind::Rectangle => {
                let bg = self.named(e, "background").as_color().unwrap_or_default();
                let radius = self.named_f(e, "border-radius");
                let bw = self.named_f(e, "border-width");
                let bc = self.named(e, "border-color").as_color().unwrap_or_default();
                c.fill_rect(r, radius, bg.fade(op));
                c.stroke_rect(r, radius, bw, bc.fade(op));
                if self.named_b(e, "clip") {
                    c.push_clip(r);
                    clipped = true;
                }
            }
            ElementKind::Text => self.paint_text(e, c, r, op),
            ElementKind::Button => {
                let enabled = self.named_b(e, "enabled");
                let primary = self.named_b(e, "primary");
                let pressed = self.named_b(e, "pressed");
                let hover = self.named_b(e, "has-hover");
                let (mut bg, fg) = match (enabled, primary) {
                    (false, _) => (DISABLED_BG, MUTED),
                    (true, true) => (ACCENT, Color::WHITE),
                    (true, false) => (SURFACE, TEXT),
                };
                if enabled && pressed {
                    bg = bg.darker(0.15);
                } else if enabled && hover {
                    bg = if primary {
                        bg.brighter(0.12)
                    } else {
                        bg.darker(0.04)
                    };
                }
                c.fill_rect(r, RADIUS, bg.fade(op));
                if !primary || !enabled {
                    c.stroke_rect(r, RADIUS, 1.0, BORDER.fade(op));
                }
                let size = self.named_f(e, "font-size");
                let text = self.named_s(e, "text");
                let w = font::line_width(&text, size);
                let base = r.y + (r.height - font::line_height(size)) / 2.0 + font::ascent(size);
                c.text(r.x + (r.width - w) / 2.0, base, &text, size, fg.fade(op));
                if focused {
                    focus_ring(c, r, RADIUS, op);
                }
            }
            ElementKind::CheckBox => {
                let enabled = self.named_b(e, "enabled");
                let checked = self.named_b(e, "checked");
                let hover = self.named_b(e, "has-hover");
                let bx = Rect::new(r.x, r.y + (r.height - 18.0) / 2.0, 18.0, 18.0);
                if checked {
                    let fill = if enabled { ACCENT } else { MUTED };
                    c.fill_rect(bx, 4.0, fill.fade(op));
                    let w = Color::WHITE.fade(op);
                    c.line(
                        Point::new(bx.x + 4.5, bx.y + 9.5),
                        Point::new(bx.x + 7.5, bx.y + 12.5),
                        2.0,
                        w,
                    );
                    c.line(
                        Point::new(bx.x + 7.5, bx.y + 12.5),
                        Point::new(bx.x + 13.5, bx.y + 5.5),
                        2.0,
                        w,
                    );
                } else {
                    c.fill_rect(
                        bx,
                        4.0,
                        if enabled { SURFACE } else { DISABLED_BG }.fade(op),
                    );
                    let border = if hover && enabled {
                        ACCENT
                    } else {
                        BORDER.darker(0.2)
                    };
                    c.stroke_rect(bx, 4.0, 1.5, border.fade(op));
                }
                let size = self.named_f(e, "font-size");
                let text = self.named_s(e, "text");
                let base = r.y + (r.height - font::line_height(size)) / 2.0 + font::ascent(size);
                let color = self.named(e, "color").as_color().unwrap_or(TEXT);
                c.text(
                    r.x + 26.0,
                    base,
                    &text,
                    size,
                    if enabled { color } else { MUTED }.fade(op),
                );
                if focused {
                    focus_ring(c, bx, 4.0, op);
                }
            }
            ElementKind::Slider => {
                let enabled = self.named_b(e, "enabled");
                let (min, max, v) = (
                    self.named_f(e, "minimum"),
                    self.named_f(e, "maximum"),
                    self.named_f(e, "value"),
                );
                let t = if max > min {
                    ((v - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let hr = 9.0;
                let cy = r.y + r.height / 2.0;
                let track = Rect::new(r.x + hr, cy - 2.0, (r.width - 2.0 * hr).max(0.0), 4.0);
                c.fill_rect(track, 2.0, BORDER.fade(op));
                let fill = if enabled { ACCENT } else { MUTED };
                c.fill_rect(
                    Rect::new(track.x, track.y, track.width * t, 4.0),
                    2.0,
                    fill.fade(op),
                );
                let hx = track.x + track.width * t;
                let handle = Rect::new(hx - hr, cy - hr, 2.0 * hr, 2.0 * hr);
                c.fill_rect(handle, hr, SURFACE.fade(op));
                c.stroke_rect(handle, hr, 2.0, fill.fade(op));
                if focused {
                    focus_ring(c, handle, hr, op);
                }
            }
            ElementKind::TextInput => {
                let enabled = self.named_b(e, "enabled");
                c.fill_rect(
                    r,
                    RADIUS,
                    if enabled { SURFACE } else { DISABLED_BG }.fade(op),
                );
                if focused {
                    c.stroke_rect(r, RADIUS, 2.0, ACCENT.fade(op));
                } else {
                    c.stroke_rect(r, RADIUS, 1.0, BORDER.fade(op));
                }
                let size = self.named_f(e, "font-size");
                let inner = Rect::new(
                    r.x + INPUT_PADDING,
                    r.y,
                    (r.width - 2.0 * INPUT_PADDING).max(0.0),
                    r.height,
                );
                let base = r.y + (r.height - font::line_height(size)) / 2.0 + font::ascent(size);
                let shown = self.input_display_text(e);
                let scroll = self.input_scroll(e);
                c.push_clip(Rect::new(
                    inner.x - 1.0,
                    inner.y,
                    inner.width + 2.0,
                    inner.height,
                ));
                if shown.is_empty() {
                    let ph = self.named_s(e, "placeholder-text");
                    c.text(inner.x, base, &ph, size, MUTED.fade(op));
                } else {
                    let color = self.named(e, "color").as_color().unwrap_or(TEXT);
                    c.text(inner.x - scroll, base, &shown, size, color.fade(op));
                }
                if focused {
                    let x = inner.x - scroll + font::caret_x(&shown, self.cursor, size);
                    let top = base - font::ascent(size);
                    c.fill_rect(
                        Rect::new(x, top, 1.5, font::line_height(size)),
                        0.0,
                        TEXT.fade(op),
                    );
                }
                c.pop_clip();
            }
            _ => {}
        }
        for child in self.def.elements[e as usize].children.clone() {
            self.paint_element(child, c, op);
        }
        if clipped {
            c.pop_clip();
        }
    }

    fn paint_text(&mut self, e: u32, c: &mut Canvas, r: Rect, op: f32) {
        let text = self.named_s(e, "text");
        let size = self.named_f(e, "font-size");
        let color = self.named(e, "color").as_color().unwrap_or(TEXT).fade(op);
        let halign = self.named(e, "horizontal-alignment").as_enum().unwrap_or(0);
        let valign = self.named(e, "vertical-alignment").as_enum().unwrap_or(0);
        let lh = font::line_height(size);
        let total = lh * text.split('\n').count() as f32;
        let mut y = match valign {
            1 => r.y + (r.height - total) / 2.0,
            2 => r.bottom() - total,
            _ => r.y,
        };
        for line in text.split('\n') {
            let w = font::line_width(line, size);
            let x = match halign {
                1 => r.x + (r.width - w) / 2.0,
                2 => r.right() - w,
                _ => r.x,
            };
            c.text(x, y + font::ascent(size), line, size, color);
            y += lh;
        }
    }
}
