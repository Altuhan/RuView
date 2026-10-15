//! Size negotiation and placement.
//!
//! Elements inside `HorizontalLayout`/`VerticalLayout` are sized from their
//! min/preferred/max constraints and stretch factors. Other elements use
//! their explicit geometry; unset sizes fill the parent (containers) or use
//! the preferred size (widgets), and unset positions center the element.
//! Computed geometry is written back into the `x`/`y`/`width`/`height`
//! properties so bindings can depend on it; layout repeats until stable.

use alloc::vec::Vec;

use super::State;
use crate::elements::{self as el, ElementKind};
use crate::font;
use crate::geometry::Rect;
use crate::value::Value;

/// (min, preferred, max, stretch) along one axis.
pub(crate) type Constraint = (f32, f32, f32, f32);

const MAX_PASSES: usize = 4;

/// Distributes `avail` among items, honoring min/max and stretch weights.
fn distribute(avail: f32, items: &[Constraint]) -> Vec<f32> {
    let mut sizes: Vec<f32> = items.iter().map(|c| c.1.clamp(c.0, c.2.max(c.0))).collect();
    let total: f32 = sizes.iter().sum();
    if total < avail {
        let mut extra = avail - total;
        for _ in 0..8 {
            let wsum: f32 = items
                .iter()
                .zip(&sizes)
                .filter(|(c, s)| c.3 > 0.0 && **s < c.2)
                .map(|(c, _)| c.3)
                .sum();
            if wsum <= 0.0 || extra < 0.01 {
                break;
            }
            let mut used = 0.0;
            for (c, s) in items.iter().zip(sizes.iter_mut()) {
                if c.3 > 0.0 && *s < c.2 {
                    let n = (*s + extra * c.3 / wsum).min(c.2);
                    used += n - *s;
                    *s = n;
                }
            }
            extra -= used;
        }
    } else if total > avail {
        let shrinkable: f32 = items.iter().zip(&sizes).map(|(c, s)| s - c.0).sum();
        if shrinkable > 0.0 {
            let f = ((total - avail) / shrinkable).min(1.0);
            for (c, s) in items.iter().zip(sizes.iter_mut()) {
                *s -= (*s - c.0) * f;
            }
        }
    }
    sizes
}

impl State {
    fn children(&self, e: u32) -> Vec<u32> {
        self.def.elements[e as usize].children.clone()
    }

    fn visible_children(&mut self, e: u32) -> Vec<u32> {
        self.children(e)
            .into_iter()
            .filter(|c| self.get_b(*c, el::VISIBLE))
            .collect()
    }

    /// Intrinsic (min, preferred) size of an element's content along one axis.
    fn intrinsic(&mut self, e: u32, horizontal: bool) -> (f32, f32) {
        let kind = self.kind(e);
        match kind {
            ElementKind::Text => {
                let size = self.named_f(e, "font-size");
                let (w, h) = font::measure(&self.named_s(e, "text"), size);
                let v = if horizontal { w } else { h };
                (v, v)
            }
            ElementKind::Button => {
                let size = self.named_f(e, "font-size");
                let v = if horizontal {
                    font::line_width(&self.named_s(e, "text"), size) + 32.0
                } else {
                    (font::line_height(size) + 14.0).max(32.0)
                };
                (v, v)
            }
            ElementKind::CheckBox => {
                let size = self.named_f(e, "font-size");
                let v = if horizontal {
                    let t = self.named_s(e, "text");
                    26.0 + font::line_width(&t, size)
                } else {
                    font::line_height(size).max(18.0) + 6.0
                };
                (v, v)
            }
            ElementKind::Slider => {
                if horizontal {
                    (60.0, 160.0)
                } else {
                    (24.0, 24.0)
                }
            }
            ElementKind::TextInput => {
                let size = self.named_f(e, "font-size");
                if horizontal {
                    (50.0, 160.0)
                } else {
                    let h = font::line_height(size) + 16.0;
                    (h, h)
                }
            }
            ElementKind::HorizontalLayout | ElementKind::VerticalLayout => {
                let main = (kind == ElementKind::HorizontalLayout) == horizontal;
                let padding = self.named_f(e, "padding");
                let spacing = self.named_f(e, "spacing");
                let kids = self.visible_children(e);
                let (mut min, mut pref) = (0.0f32, 0.0f32);
                for c in &kids {
                    let (cmin, cpref, _, _) = self.constraints(*c, horizontal);
                    if main {
                        min += cmin;
                        pref += cpref;
                    } else {
                        min = min.max(cmin);
                        pref = pref.max(cpref);
                    }
                }
                if main && kids.len() > 1 {
                    let gaps = spacing * (kids.len() - 1) as f32;
                    min += gaps;
                    pref += gaps;
                }
                (min + 2.0 * padding, pref + 2.0 * padding)
            }
            _ => {
                // Containers: big enough for their children.
                let (mut min, mut pref) = (0.0f32, 0.0f32);
                let pos = if horizontal { el::X } else { el::Y };
                for c in self.visible_children(e) {
                    let (cmin, cpref, _, _) = self.constraints(c, horizontal);
                    let off = if self.is_explicit(c, pos) {
                        self.get_f(c, pos).max(0.0)
                    } else {
                        0.0
                    };
                    min = min.max(off + cmin);
                    pref = pref.max(off + cpref);
                }
                (min, pref)
            }
        }
    }

    pub(crate) fn clear_constraint_cache(&mut self) {
        for c in &mut self.constraint_cache {
            *c = [None, None];
        }
    }

    /// Layout constraints of `e` along one axis (memoized per layout pass).
    pub(crate) fn constraints(&mut self, e: u32, horizontal: bool) -> (f32, f32, f32, f32) {
        let axis = usize::from(!horizontal);
        if let Some(c) = self.constraint_cache[e as usize][axis] {
            return c;
        }
        let c = self.compute_constraints(e, horizontal);
        self.constraint_cache[e as usize][axis] = Some(c);
        c
    }

    fn compute_constraints(&mut self, e: u32, horizontal: bool) -> (f32, f32, f32, f32) {
        let (size, min_p, max_p, pref_p, stretch_p) = if horizontal {
            (
                el::WIDTH,
                el::MIN_WIDTH,
                el::MAX_WIDTH,
                el::PREFERRED_WIDTH,
                el::HORIZONTAL_STRETCH,
            )
        } else {
            (
                el::HEIGHT,
                el::MIN_HEIGHT,
                el::MAX_HEIGHT,
                el::PREFERRED_HEIGHT,
                el::VERTICAL_STRETCH,
            )
        };
        if self.is_explicit(e, size) {
            let v = self.get_f(e, size).max(0.0);
            return (v, v, v, 0.0);
        }
        let kind = self.kind(e);
        let (imin, ipref) = self.intrinsic(e, horizontal);
        let min = if self.is_explicit(e, min_p) {
            self.get_f(e, min_p)
        } else {
            imin
        };
        let pref = if self.is_explicit(e, pref_p) {
            self.get_f(e, pref_p)
        } else {
            ipref
        };
        // Fixed-height widgets don't grow vertically unless asked to.
        let fixed_height = !horizontal
            && matches!(
                kind,
                ElementKind::Button
                    | ElementKind::CheckBox
                    | ElementKind::Slider
                    | ElementKind::TextInput
            );
        let max = if self.is_explicit(e, max_p) || !fixed_height {
            self.get_f(e, max_p)
        } else {
            pref
        };
        let max = max.max(min);
        // A layout stretches only if one of its children wants to.
        let stretch = if kind.is_layout() && !self.is_explicit(e, stretch_p) {
            let kids = self.visible_children(e);
            kids.into_iter()
                .map(|c| self.constraints(c, horizontal).3)
                .fold(0.0, f32::max)
        } else {
            self.get_f(e, stretch_p).max(0.0)
        };
        (min, pref.clamp(min, max), max, stretch)
    }

    pub(crate) fn layout(&mut self) {
        for _ in 0..MAX_PASSES {
            self.layout_dirty = false;
            self.clear_constraint_cache();
            let root = Rect::new(0.0, 0.0, self.size.width, self.size.height);
            self.place(0, root, None);
            if !self.layout_dirty {
                break;
            }
        }
        self.layout_dirty = false;
    }

    /// Assigns `rect` (absolute) to `e`, writes its geometry properties, and lays out children.
    fn place(&mut self, e: u32, rect: Rect, parent: Option<Rect>) {
        self.geometry[e as usize] = rect;
        let (px, py) = parent.map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
        self.set_computed(e, el::X, Value::Float(rect.x - px));
        self.set_computed(e, el::Y, Value::Float(rect.y - py));
        if e == 0 {
            self.force_computed(e, el::WIDTH, Value::Float(rect.width));
            self.force_computed(e, el::HEIGHT, Value::Float(rect.height));
        } else {
            self.set_computed(e, el::WIDTH, Value::Float(rect.width));
            self.set_computed(e, el::HEIGHT, Value::Float(rect.height));
        }
        let kind = self.kind(e);
        if kind.is_layout() {
            self.place_box(e, rect, kind == ElementKind::HorizontalLayout);
            return;
        }
        for c in self.children(e) {
            let r = self.free_rect(c, rect);
            self.place(c, r, Some(rect));
        }
    }

    /// Geometry of a child that is not managed by a layout.
    fn free_rect(&mut self, c: u32, parent: Rect) -> Rect {
        let kind = self.kind(c);
        let axis = |s: &mut State, horizontal: bool| {
            let (pos, size, avail) = if horizontal {
                (el::X, el::WIDTH, parent.width)
            } else {
                (el::Y, el::HEIGHT, parent.height)
            };
            let explicit_pos = s.is_explicit(c, pos);
            let p = if explicit_pos { s.get_f(c, pos) } else { 0.0 };
            let len = if s.is_explicit(c, size) {
                s.get_f(c, size)
            } else {
                let (min, pref, max, _) = s.constraints(c, horizontal);
                if kind.fills_parent() {
                    (avail - p).clamp(min, max.max(min))
                } else {
                    pref
                }
            };
            let p = if explicit_pos { p } else { (avail - len) / 2.0 };
            (p, len.max(0.0))
        };
        let (x, w) = axis(self, true);
        let (y, h) = axis(self, false);
        Rect::new(parent.x + x, parent.y + y, w, h)
    }

    fn place_box(&mut self, e: u32, rect: Rect, horizontal: bool) {
        let padding = self.named_f(e, "padding");
        let spacing = self.named_f(e, "spacing");
        let alignment = self.named(e, "alignment").as_enum().unwrap_or(0);
        let inner = rect.inset(padding);
        let all = self.children(e);
        let kids: Vec<u32> = self.visible_children(e);
        for c in &all {
            if !kids.contains(c) {
                let r = Rect::new(inner.x, inner.y, 0.0, 0.0);
                self.place(*c, r, Some(rect));
            }
        }
        if kids.is_empty() {
            return;
        }
        let (main_len, cross_len) = if horizontal {
            (inner.width, inner.height)
        } else {
            (inner.height, inner.width)
        };
        let gaps = spacing * (kids.len() - 1) as f32;
        let mut mains: Vec<Constraint> = kids
            .iter()
            .map(|c| self.constraints(*c, horizontal))
            .collect();
        // With `alignment: stretch` and no stretchable child, all children share the space.
        if alignment == 0 && mains.iter().all(|c| c.3 <= 0.0) {
            for c in &mut mains {
                c.3 = 1.0;
            }
        }
        let sizes = distribute((main_len - gaps).max(0.0), &mains);
        let leftover = (main_len - gaps - sizes.iter().sum::<f32>()).max(0.0);
        // LayoutAlignment: stretch, start, center, end, space-between
        let (mut pos, extra_gap) = match alignment {
            2 => (leftover / 2.0, 0.0),
            3 => (leftover, 0.0),
            4 if kids.len() > 1 => (0.0, leftover / (kids.len() - 1) as f32),
            4 => (leftover / 2.0, 0.0),
            _ => (0.0, 0.0),
        };
        for (i, c) in kids.iter().enumerate() {
            let (cmin, _, cmax, _) = self.constraints(*c, !horizontal);
            let cross_explicit =
                self.is_explicit(*c, if horizontal { el::HEIGHT } else { el::WIDTH });
            let cross = if cross_explicit {
                cmin
            } else {
                cross_len.clamp(cmin, cmax.max(cmin))
            };
            let cross_off = (cross_len - cross) / 2.0;
            let r = if horizontal {
                Rect::new(inner.x + pos, inner.y + cross_off, sizes[i], cross)
            } else {
                Rect::new(inner.x + cross_off, inner.y + pos, cross, sizes[i])
            };
            self.place(*c, r, Some(rect));
            pos += sizes[i] + spacing + extra_gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::distribute;

    #[test]
    fn distributes_extra_by_stretch() {
        let s = distribute(
            300.0,
            &[
                (0.0, 50.0, 1e9, 1.0),
                (0.0, 50.0, 1e9, 2.0),
                (0.0, 50.0, 1e9, 0.0),
            ],
        );
        assert!(
            (s[0] - 100.0).abs() < 0.1 && (s[1] - 150.0).abs() < 0.1 && (s[2] - 50.0).abs() < 0.1,
            "{s:?}"
        );
    }

    #[test]
    fn respects_max_and_shrinks_to_min() {
        let s = distribute(300.0, &[(0.0, 50.0, 80.0, 1.0), (0.0, 50.0, 1e9, 1.0)]);
        assert!(
            (s[0] - 80.0).abs() < 0.1 && (s[1] - 220.0).abs() < 0.1,
            "{s:?}"
        );
        let s = distribute(60.0, &[(20.0, 50.0, 1e9, 1.0), (40.0, 50.0, 1e9, 1.0)]);
        assert!(
            (s.iter().sum::<f32>() - 60.0).abs() < 0.1 && s[0] >= 20.0 && s[1] >= 40.0,
            "{s:?}"
        );
    }
}
