//! RGBA colors.

/// A non-premultiplied 8-bit RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `0xRRGGBB`, fully opaque.
    pub const fn from_rgb_u32(v: u32) -> Self {
        Self::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// `0xAARRGGBB`.
    pub const fn from_argb_u32(v: u32) -> Self {
        Self::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8)
    }

    pub const fn to_argb_u32(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// Parses `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` (the `#` is optional).
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        let digit = |c: u8| -> Option<u8> { (c as char).to_digit(16).map(|d| d as u8) };
        let b = s.as_bytes();
        let pair = |i: usize| -> Option<u8> { Some(digit(b[i])? * 16 + digit(b[i + 1])?) };
        match b.len() {
            3 | 4 => {
                let mut c = [255u8; 4];
                for (i, ch) in b.iter().enumerate() {
                    c[i] = digit(*ch)? * 17;
                }
                Some(Self::rgba(c[0], c[1], c[2], c[3]))
            }
            6 => Some(Self::rgb(pair(0)?, pair(2)?, pair(4)?)),
            8 => Some(Self::rgba(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
            _ => None,
        }
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Multiplies the alpha channel by `f` in `0..=1`.
    pub fn fade(self, f: f32) -> Self {
        self.with_alpha((self.a as f32 * f.clamp(0.0, 1.0) + 0.5) as u8)
    }

    /// Mixes towards black by `f` in `0..=1`.
    pub fn darker(self, f: f32) -> Self {
        self.mix(Color::BLACK.with_alpha(self.a), f)
    }

    /// Mixes towards white by `f` in `0..=1`.
    pub fn brighter(self, f: f32) -> Self {
        self.mix(Color::WHITE.with_alpha(self.a), f)
    }

    /// Linear interpolation: `t = 0` gives `self`, `t = 1` gives `other`.
    pub fn mix(self, other: Color, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t + 0.5) as u8;
        Self::rgba(
            l(self.r, other.r),
            l(self.g, other.g),
            l(self.b, other.b),
            l(self.a, other.a),
        )
    }

    /// Source-over blend of `src` with extra coverage `cov` (0..=255) onto an opaque `self`.
    #[inline]
    pub fn blend_under(self, src: Color, cov: u8) -> Color {
        let a = (src.a as u32 * cov as u32 + 127) / 255;
        if a == 0 {
            return self;
        }
        if a >= 255 {
            return src.with_alpha(255);
        }
        let inv = 255 - a;
        let ch = |d: u8, s: u8| ((s as u32 * a + d as u32 * inv + 127) / 255) as u8;
        Color::rgba(ch(self.r, src.r), ch(self.g, src.g), ch(self.b, src.b), 255)
    }

    pub fn to_rgb565(self) -> u16 {
        ((self.r as u16 & 0xf8) << 8) | ((self.g as u16 & 0xfc) << 3) | (self.b as u16 >> 3)
    }

    pub fn from_rgb565(v: u16) -> Self {
        let r = ((v >> 11) & 0x1f) as u8;
        let g = ((v >> 5) & 0x3f) as u8;
        let b = (v & 0x1f) as u8;
        Self::rgb(
            (r << 3) | (r >> 2),
            (g << 2) | (g >> 4),
            (b << 3) | (b >> 2),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(Color::from_hex("#fff"), Some(Color::WHITE));
        assert_eq!(
            Color::from_hex("#11223344"),
            Some(Color::rgba(0x11, 0x22, 0x33, 0x44))
        );
        assert_eq!(
            Color::from_hex("#123456"),
            Some(Color::rgb(0x12, 0x34, 0x56))
        );
        assert_eq!(Color::from_hex("#12345"), None);
        assert_eq!(Color::from_hex("#zzz"), None);
    }

    #[test]
    fn rgb565_roundtrip_is_close() {
        let c = Color::rgb(200, 100, 50);
        let back = Color::from_rgb565(c.to_rgb565());
        assert!((back.r as i32 - 200).abs() < 8 && (back.g as i32 - 100).abs() < 4);
    }
}
