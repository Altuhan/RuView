//! Built-in proportional font (8-bit alpha atlas) and text measurement.

#[allow(clippy::all)]
mod atlas;

pub use atlas::{ASCENT, BASE_SIZE, DESCENT};

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    /// Horizontal advance in atlas pixels.
    pub advance: f32,
    pub bearing_x: i32,
    /// Offset of the bitmap top from the baseline (negative is above).
    pub bearing_y: i32,
    pub width: u32,
    pub height: u32,
    pub bitmap: &'static [u8],
}

pub fn glyph(ch: char) -> Glyph {
    let code = ch as u32;
    let idx = atlas::GLYPHS
        .binary_search_by_key(&code, |g| g.0)
        .or_else(|_| atlas::GLYPHS.binary_search_by_key(&('?' as u32), |g| g.0))
        .unwrap_or(0);
    let (_, adv, bx, by, w, h, off) = atlas::GLYPHS[idx];
    let len = w as usize * h as usize;
    Glyph {
        advance: adv as f32 / 64.0,
        bearing_x: bx as i32,
        bearing_y: by as i32,
        width: w as u32,
        height: h as u32,
        bitmap: &atlas::DATA[off as usize..off as usize + len],
    }
}

/// Height of one line of text at `size` pixels.
pub fn line_height(size: f32) -> f32 {
    (ASCENT + DESCENT) * size / BASE_SIZE
}

pub fn ascent(size: f32) -> f32 {
    ASCENT * size / BASE_SIZE
}

/// Width of a single line of text.
pub fn line_width(text: &str, size: f32) -> f32 {
    text.chars().map(|c| glyph(c).advance).sum::<f32>() * size / BASE_SIZE
}

/// Size of possibly multi-line text.
pub fn measure(text: &str, size: f32) -> (f32, f32) {
    let mut w: f32 = 0.0;
    let mut lines = 0;
    for line in text.split('\n') {
        w = w.max(line_width(line, size));
        lines += 1;
    }
    (w, lines as f32 * line_height(size))
}

/// X offset (relative to the line start) of the caret placed before char index `n`.
pub fn caret_x(text: &str, n: usize, size: f32) -> f32 {
    text.chars().take(n).map(|c| glyph(c).advance).sum::<f32>() * size / BASE_SIZE
}

/// Char index of the caret position nearest to `x` within a single line.
pub fn hit_test(text: &str, x: f32, size: f32) -> usize {
    let scale = size / BASE_SIZE;
    let mut pos = 0.0;
    for (i, c) in text.chars().enumerate() {
        let adv = glyph(c).advance * scale;
        if x < pos + adv / 2.0 {
            return i;
        }
        pos += adv;
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_exist_and_measure() {
        let a = glyph('A');
        assert!(a.width > 0 && a.height > 0 && a.advance > 0.0);
        assert_eq!(glyph('\u{1F600}').advance, glyph('?').advance);
        let (w, h) = measure("Hello\nWorld!", 16.0);
        assert!(w > 30.0 && w < 120.0, "{w}");
        assert!((h - 2.0 * line_height(16.0)).abs() < 0.01);
        assert_eq!(hit_test("abc", 0.0, 16.0), 0);
        assert_eq!(hit_test("abc", 1000.0, 16.0), 3);
    }
}
