//! Pointer and keyboard handling for the built-in widgets.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::State;
use crate::elements::{self as el, ElementKind};
use crate::font;
use crate::geometry::Point;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Enter,
    Space,
    Tab,
    BackTab,
    Escape,
}

/// Platform-independent input events, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PointerMoved {
        position: Point,
    },
    PointerPressed {
        position: Point,
        button: PointerButton,
    },
    PointerReleased {
        position: Point,
        button: PointerButton,
    },
    PointerExited,
    KeyPressed(Key),
    /// Committed text (typed characters, IME output or paste).
    Text(String),
}

/// Inner padding of text inputs; shared with painting.
pub(crate) const INPUT_PADDING: f32 = 8.0;

impl State {
    fn enabled(&mut self, e: u32) -> bool {
        match self.kind(e).property_index("enabled") {
            Some(i) => self.get_b(e, i),
            None => true,
        }
    }

    fn hit(&mut self, e: u32, p: Point) -> Option<u32> {
        if !self.get_b(e, el::VISIBLE) {
            return None;
        }
        let children = self.def.elements[e as usize].children.clone();
        for c in children.into_iter().rev() {
            if let Some(h) = self.hit(c, p) {
                return Some(h);
            }
        }
        let kind = self.kind(e);
        (kind.interactive() && self.geometry[e as usize].contains(p) && self.enabled(e))
            .then_some(e)
    }

    fn set_hover(&mut self, e: Option<u32>, value: bool) {
        if let Some(e) = e {
            self.set_out(e, "has-hover", Value::Bool(value));
        }
    }

    /// Writes an output property (these are never bound, so assignment keeps semantics).
    fn set_out(&mut self, e: u32, name: &str, v: Value) {
        if let Some(i) = self.kind(e).property_index(name) {
            if self.get(e, i) != v {
                self.set(e, i, v);
            }
        }
    }

    pub(crate) fn set_focus(&mut self, e: Option<u32>) {
        if self.focused == e {
            return;
        }
        if let Some(old) = self.focused {
            self.set_out(old, "has-focus", Value::Bool(false));
        }
        self.focused = e;
        if let Some(new) = e {
            self.set_out(new, "has-focus", Value::Bool(true));
            if self.kind(new) == ElementKind::TextInput {
                self.cursor = self.named_s(new, "text").chars().count();
            }
        }
        self.needs_redraw = true;
    }

    fn focus_chain(&mut self, e: u32, out: &mut Vec<u32>) {
        if !self.get_b(e, el::VISIBLE) {
            return;
        }
        if self.kind(e).focusable() && self.enabled(e) {
            out.push(e);
        }
        for c in self.def.elements[e as usize].children.clone() {
            self.focus_chain(c, out);
        }
    }

    fn move_focus(&mut self, forward: bool) {
        let mut chain = Vec::new();
        self.focus_chain(0, &mut chain);
        if chain.is_empty() {
            return;
        }
        let n = chain.len();
        let next = match self
            .focused
            .and_then(|f| chain.iter().position(|c| *c == f))
        {
            Some(i) if forward => chain[(i + 1) % n],
            Some(i) => chain[(i + n - 1) % n],
            None if forward => chain[0],
            None => chain[n - 1],
        };
        self.set_focus(Some(next));
    }

    fn slider_set_from_x(&mut self, e: u32, x: f32) {
        let g = self.geometry[e as usize];
        let (min, max) = (self.named_f(e, "minimum"), self.named_f(e, "maximum"));
        let r = g.height / 2.0;
        let t = ((x - g.x - r) / (g.width - 2.0 * r).max(1.0)).clamp(0.0, 1.0);
        self.slider_set(e, min + t * (max - min));
    }

    fn slider_set(&mut self, e: u32, v: f32) {
        let (min, max, step) = (
            self.named_f(e, "minimum"),
            self.named_f(e, "maximum"),
            self.named_f(e, "step"),
        );
        let mut v = v.clamp(min.min(max), max.max(min));
        if step > 0.0 {
            v = min + libm::roundf((v - min) / step) * step;
        }
        if self.named_f(e, "value") != v {
            self.set_named(e, "value", Value::Float(v));
            self.queue(e, "changed", vec![Value::Float(v)]);
        }
    }

    fn activate(&mut self, e: u32) {
        match self.kind(e) {
            ElementKind::Button | ElementKind::TouchArea => self.queue(e, "clicked", Vec::new()),
            ElementKind::CheckBox => {
                let v = !self.named_b(e, "checked");
                self.set_named(e, "checked", Value::Bool(v));
                self.queue(e, "toggled", Vec::new());
            }
            _ => {}
        }
    }

    fn edit_text(&mut self, e: u32, f: impl FnOnce(&mut Vec<char>, &mut usize)) {
        let mut chars: Vec<char> = self.named_s(e, "text").chars().collect();
        let before = chars.clone();
        let mut cursor = self.cursor.min(chars.len());
        f(&mut chars, &mut cursor);
        self.cursor = cursor;
        self.needs_redraw = true;
        if chars != before {
            let s: String = chars.into_iter().collect();
            self.set_named(e, "text", Value::String(s.clone()));
            self.queue(e, "edited", vec![Value::String(s)]);
        }
    }

    pub(crate) fn handle_event(&mut self, event: Event) {
        match event {
            Event::PointerMoved { position } => {
                if let Some(p) = self.pressed {
                    match self.kind(p) {
                        ElementKind::Slider => self.slider_set_from_x(p, position.x),
                        ElementKind::TextInput => {
                            self.cursor = self.text_hit(p, position.x);
                            self.needs_redraw = true;
                        }
                        _ => {}
                    }
                }
                let target = self.hit(0, position);
                if target != self.hovered {
                    let old = self.hovered;
                    self.set_hover(old, false);
                    self.hovered = target;
                    self.set_hover(target, true);
                }
                if let Some(t) = target.or(self.pressed) {
                    if self.kind(t) == ElementKind::TouchArea {
                        let g = self.geometry[t as usize];
                        self.set_out(t, "mouse-x", Value::Float(position.x - g.x));
                        self.set_out(t, "mouse-y", Value::Float(position.y - g.y));
                    }
                }
            }
            Event::PointerPressed {
                position,
                button: PointerButton::Left,
            } => {
                let target = self.hit(0, position);
                self.pressed = target;
                match target {
                    Some(t) => {
                        self.set_out(t, "pressed", Value::Bool(true));
                        if self.kind(t).focusable() {
                            self.set_focus(Some(t));
                        }
                        match self.kind(t) {
                            ElementKind::Slider => self.slider_set_from_x(t, position.x),
                            ElementKind::TextInput => {
                                self.cursor = self.text_hit(t, position.x);
                                self.needs_redraw = true;
                            }
                            ElementKind::TouchArea => {
                                let g = self.geometry[t as usize];
                                self.set_out(t, "mouse-x", Value::Float(position.x - g.x));
                                self.set_out(t, "mouse-y", Value::Float(position.y - g.y));
                            }
                            _ => {}
                        }
                    }
                    None => self.set_focus(None),
                }
            }
            Event::PointerReleased {
                position,
                button: PointerButton::Left,
            } => {
                if let Some(p) = self.pressed.take() {
                    self.set_out(p, "pressed", Value::Bool(false));
                    if self.geometry[p as usize].contains(position) && self.enabled(p) {
                        self.activate(p);
                    }
                }
            }
            Event::PointerPressed { .. } | Event::PointerReleased { .. } => {}
            Event::PointerExited => {
                let old = self.hovered.take();
                self.set_hover(old, false);
            }
            Event::KeyPressed(Key::Tab) => self.move_focus(true),
            Event::KeyPressed(Key::BackTab) => self.move_focus(false),
            Event::KeyPressed(key) => {
                let Some(f) = self.focused else { return };
                match self.kind(f) {
                    ElementKind::Button | ElementKind::CheckBox
                        if matches!(key, Key::Enter | Key::Space) =>
                    {
                        self.activate(f)
                    }
                    ElementKind::Slider => {
                        let (min, max, step) = (
                            self.named_f(f, "minimum"),
                            self.named_f(f, "maximum"),
                            self.named_f(f, "step"),
                        );
                        let step = if step > 0.0 {
                            step
                        } else {
                            (max - min) / 100.0
                        };
                        let v = self.named_f(f, "value");
                        match key {
                            Key::Left | Key::Down => self.slider_set(f, v - step),
                            Key::Right | Key::Up => self.slider_set(f, v + step),
                            Key::Home => self.slider_set(f, min),
                            Key::End => self.slider_set(f, max),
                            _ => {}
                        }
                    }
                    ElementKind::TextInput => match key {
                        Key::Backspace => self.edit_text(f, |t, c| {
                            if *c > 0 {
                                t.remove(*c - 1);
                                *c -= 1;
                            }
                        }),
                        Key::Delete => self.edit_text(f, |t, c| {
                            if *c < t.len() {
                                t.remove(*c);
                            }
                        }),
                        Key::Left => self.edit_text(f, |_, c| *c = c.saturating_sub(1)),
                        Key::Right => self.edit_text(f, |t, c| *c = (*c + 1).min(t.len())),
                        Key::Home => self.edit_text(f, |_, c| *c = 0),
                        Key::End => self.edit_text(f, |t, c| *c = t.len()),
                        Key::Enter => {
                            let text = self.named_s(f, "text");
                            self.queue(f, "accepted", vec![Value::String(text)]);
                        }
                        Key::Escape => self.set_focus(None),
                        _ => {}
                    },
                    _ => {}
                }
            }
            Event::Text(s) => {
                let Some(f) = self.focused else { return };
                if self.kind(f) != ElementKind::TextInput || !self.enabled(f) {
                    return;
                }
                let insert: Vec<char> = s.chars().filter(|c| !c.is_control()).collect();
                if !insert.is_empty() {
                    self.edit_text(f, |t, c| {
                        for (i, ch) in insert.iter().enumerate() {
                            t.insert(*c + i, *ch);
                        }
                        *c += insert.len();
                    });
                }
            }
        }
    }

    /// Displayed text of a text input (masked for passwords).
    pub(crate) fn input_display_text(&mut self, e: u32) -> String {
        let text = self.named_s(e, "text");
        if self.named_b(e, "password") {
            text.chars().map(|_| '•').collect()
        } else {
            text
        }
    }

    /// Horizontal scroll offset that keeps the caret visible.
    pub(crate) fn input_scroll(&mut self, e: u32) -> f32 {
        if self.focused != Some(e) {
            return 0.0;
        }
        let size = self.named_f(e, "font-size");
        let shown = self.input_display_text(e);
        let caret = font::caret_x(&shown, self.cursor, size);
        let visible = self.geometry[e as usize].width - 2.0 * INPUT_PADDING;
        (caret - visible + 2.0).max(0.0)
    }

    fn text_hit(&mut self, e: u32, x: f32) -> usize {
        let size = self.named_f(e, "font-size");
        let scroll = self.input_scroll(e);
        let shown = self.input_display_text(e);
        let local = x - self.geometry[e as usize].x - INPUT_PADDING + scroll;
        font::hit_test(&shown, local, size)
    }
}
