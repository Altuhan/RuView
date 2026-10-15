//! A live instance of a [`ComponentDef`]: reactive properties, callbacks,
//! layout, input handling and painting.
//!
//! [`ComponentInstance`] is a cheap, clonable handle (`Rc` inside). Callbacks
//! receive a handle to the instance, so Rust code can read and write
//! properties from inside a handler without capturing anything.

mod eval;
mod input;
mod layout;
mod paint;

pub use input::{Event, Key, PointerButton};

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use crate::elements::{self, ElementKind};
use crate::geometry::{Rect, Size};
use crate::ir::{CallbackRef, ComponentDef, PropRef, PropertyInit, Stmt};
use crate::raster::{Canvas, Target};
use crate::value::{Type, Value};

/// A callback implemented in Rust. It receives the instance and the arguments.
pub type NativeHandler = Box<dyn FnMut(&ComponentInstance, &[Value])>;

/// Errors from the dynamic property/callback API.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    UnknownProperty(String),
    ReadOnlyProperty(String),
    TypeMismatch {
        name: String,
        expected: Type,
        found: Type,
    },
    UnknownCallback(String),
    ArgumentCount {
        name: String,
        expected: usize,
        found: usize,
    },
    UnknownElement(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownProperty(n) => write!(f, "unknown public property `{n}`"),
            Error::ReadOnlyProperty(n) => write!(f, "property `{n}` is `out` and cannot be set"),
            Error::TypeMismatch {
                name,
                expected,
                found,
            } => {
                write!(f, "property `{name}` has type {expected}, got {found}")
            }
            Error::UnknownCallback(n) => write!(f, "unknown public callback `{n}`"),
            Error::ArgumentCount {
                name,
                expected,
                found,
            } => {
                write!(
                    f,
                    "callback `{name}` takes {expected} argument(s), got {found}"
                )
            }
            Error::UnknownElement(n) => write!(f, "no element with id `{n}`"),
        }
    }
}

#[cfg(feature = "std")]
extern crate std;
#[cfg(feature = "std")]
impl std::error::Error for Error {}

pub(crate) struct Slot {
    value: Value,
    ty: Type,
    /// Location of the binding expression in the definition, if bound.
    binding: Option<PropRef>,
    dirty: bool,
    /// Set by `.ui` code or from Rust (as opposed to defaulted/computed by layout).
    explicit: bool,
    evaluating: bool,
    dependents: Vec<u32>,
}

/// Mutable state of an instance; always accessed through one `RefCell` borrow
/// that is released before any callback runs.
pub(crate) struct State {
    pub(crate) def: Rc<ComponentDef>,
    prop_offset: Vec<u32>,
    cb_offset: Vec<u32>,
    /// `alias[slot]` is the slot a two-way-bound property forwards to.
    alias: Vec<u32>,
    slots: Vec<Slot>,
    pub(crate) geometry: Vec<Rect>,
    eval_stack: Vec<u32>,
    pub(crate) size: Size,
    pub(crate) layout_dirty: bool,
    pub(crate) needs_redraw: bool,
    pub(crate) hovered: Option<u32>,
    pub(crate) pressed: Option<u32>,
    pub(crate) focused: Option<u32>,
    /// Caret position (in chars) in the focused text input.
    pub(crate) cursor: usize,
    /// Callbacks queued during event handling, run after the borrow ends.
    pub(crate) pending: Vec<(u32, Vec<Value>)>,
    /// Per-pass memo of layout constraints: `[horizontal, vertical]` per element.
    pub(crate) constraint_cache: Vec<[Option<layout::Constraint>; 2]>,
}

impl State {
    fn new(def: Rc<ComponentDef>) -> Self {
        let n = def.elements.len();
        let mut prop_offset = Vec::with_capacity(n);
        let mut cb_offset = Vec::with_capacity(n);
        let (mut np, mut nc) = (0u32, 0u32);
        for e in def.elements.iter() {
            prop_offset.push(np);
            cb_offset.push(nc);
            np += e.properties.len() as u32;
            nc += e.callbacks.len() as u32;
        }
        let mut slots = Vec::with_capacity(np as usize);
        let mut alias = Vec::with_capacity(np as usize);
        for (ei, e) in def.elements.iter().enumerate() {
            for (pi, p) in e.properties.iter().enumerate() {
                let here = PropRef {
                    element: ei as u32,
                    property: pi as u32,
                };
                let (value, binding, explicit) = match &p.init {
                    PropertyInit::Default(v) => (v.clone(), None, false),
                    PropertyInit::Binding(_) => (p.ty.default_value(), Some(here), true),
                    PropertyInit::Alias(_) => (p.ty.default_value(), None, true),
                };
                alias.push(slots.len() as u32);
                slots.push(Slot {
                    value,
                    ty: p.ty,
                    dirty: binding.is_some(),
                    binding,
                    explicit,
                    evaluating: false,
                    dependents: Vec::new(),
                });
            }
        }
        let mut st = State {
            def: def.clone(),
            prop_offset,
            cb_offset,
            alias,
            slots,
            geometry: vec![Rect::default(); n],
            eval_stack: Vec::new(),
            size: Size::default(),
            layout_dirty: true,
            needs_redraw: true,
            hovered: None,
            pressed: None,
            focused: None,
            cursor: 0,
            pending: Vec::new(),
            constraint_cache: vec![[None, None]; n],
        };
        // Resolve two-way bindings to their final target (following chains).
        for (ei, e) in def.elements.iter().enumerate() {
            for (pi, p) in e.properties.iter().enumerate() {
                if let PropertyInit::Alias(target) = p.init {
                    let me = st.slot_of(PropRef {
                        element: ei as u32,
                        property: pi as u32,
                    });
                    let t = st.slot_of(target);
                    st.alias[me as usize] = t;
                }
            }
        }
        for i in 0..st.alias.len() {
            let mut t = st.alias[i];
            let mut hops = 0;
            while st.alias[t as usize] != t && hops < 64 {
                t = st.alias[t as usize];
                hops += 1;
            }
            st.alias[i] = t;
        }
        st
    }

    #[inline]
    pub(crate) fn slot_of(&self, r: PropRef) -> u32 {
        self.prop_offset[r.element as usize] + r.property
    }

    #[inline]
    pub(crate) fn cb_slot(&self, r: CallbackRef) -> u32 {
        self.cb_offset[r.element as usize] + r.callback
    }

    pub(crate) fn kind(&self, e: u32) -> ElementKind {
        self.def.elements[e as usize].kind
    }

    /// Reads a property, re-evaluating its binding if needed and recording
    /// a dependency for the binding currently being evaluated.
    pub(crate) fn get_slot(&mut self, slot: u32) -> Value {
        let s = self.alias[slot as usize] as usize;
        if let Some(&reader) = self.eval_stack.last() {
            let deps = &mut self.slots[s].dependents;
            if !deps.contains(&reader) {
                deps.push(reader);
            }
        }
        if self.slots[s].dirty && !self.slots[s].evaluating {
            if let Some(b) = self.slots[s].binding {
                let def = self.def.clone();
                if let PropertyInit::Binding(expr) = &def.property(b).init {
                    self.slots[s].evaluating = true;
                    self.eval_stack.push(s as u32);
                    let v = self.eval(expr, &[]);
                    self.eval_stack.pop();
                    let ty = self.slots[s].ty;
                    let slot = &mut self.slots[s];
                    slot.value = v.convert(ty);
                    slot.evaluating = false;
                }
            }
            self.slots[s].dirty = false;
        }
        self.slots[s].value.clone()
    }

    pub(crate) fn get(&mut self, e: u32, prop: u32) -> Value {
        let s = self.slot_of(PropRef {
            element: e,
            property: prop,
        });
        self.get_slot(s)
    }

    pub(crate) fn get_f(&mut self, e: u32, prop: u32) -> f32 {
        self.get(e, prop).as_float().unwrap_or(0.0)
    }

    pub(crate) fn get_b(&mut self, e: u32, prop: u32) -> bool {
        self.get(e, prop).as_bool().unwrap_or(false)
    }

    /// Built-in property by name (used for kind-specific properties).
    pub(crate) fn named(&mut self, e: u32, name: &str) -> Value {
        match self.kind(e).property_index(name) {
            Some(i) => self.get(e, i),
            None => Value::Void,
        }
    }

    pub(crate) fn named_f(&mut self, e: u32, name: &str) -> f32 {
        self.named(e, name).as_float().unwrap_or(0.0)
    }

    pub(crate) fn named_b(&mut self, e: u32, name: &str) -> bool {
        self.named(e, name).as_bool().unwrap_or(false)
    }

    pub(crate) fn named_s(&mut self, e: u32, name: &str) -> String {
        match self.named(e, name) {
            Value::String(s) => s,
            v => v.to_string(),
        }
    }

    pub(crate) fn is_explicit(&self, e: u32, prop: u32) -> bool {
        let s = self.alias[self.slot_of(PropRef {
            element: e,
            property: prop,
        }) as usize];
        self.slots[s as usize].explicit
    }

    /// Assigns a property. Like Slint, assigning removes any binding.
    pub(crate) fn set_slot(&mut self, slot: u32, v: Value) {
        let s = self.alias[slot as usize] as usize;
        let v = v.convert(self.slots[s].ty);
        let slot = &mut self.slots[s];
        slot.binding = None;
        slot.dirty = false;
        slot.explicit = true;
        if slot.value != v {
            slot.value = v;
            self.invalidate(s as u32);
        }
    }

    pub(crate) fn set(&mut self, e: u32, prop: u32, v: Value) {
        let s = self.slot_of(PropRef {
            element: e,
            property: prop,
        });
        self.set_slot(s, v);
    }

    pub(crate) fn set_named(&mut self, e: u32, name: &str, v: Value) {
        if let Some(i) = self.kind(e).property_index(name) {
            self.set(e, i, v);
        }
    }

    /// Written by layout/input: keeps the property non-explicit. Returns whether it changed.
    pub(crate) fn set_computed(&mut self, e: u32, prop: u32, v: Value) -> bool {
        let s = self.alias[self.slot_of(PropRef {
            element: e,
            property: prop,
        }) as usize] as usize;
        if self.slots[s].explicit || self.slots[s].value == v {
            return false;
        }
        self.slots[s].value = v;
        self.invalidate(s as u32);
        true
    }

    /// The root's size as written in `.ui` code. Layout overwrites the root's
    /// `width`/`height` with the viewport size, so evaluate the binding itself.
    pub(crate) fn declared_size(&mut self, prop: u32) -> Option<f32> {
        if !self.is_explicit(0, prop) {
            return None;
        }
        let def = self.def.clone();
        match &def
            .property(PropRef {
                element: 0,
                property: prop,
            })
            .init
        {
            PropertyInit::Binding(expr) => self.eval(expr, &[]).as_float(),
            _ => Some(self.get_f(0, prop)),
        }
    }

    /// Overrides a property's current value even if it is explicitly set
    /// (used for the root's geometry, which always matches the viewport).
    pub(crate) fn force_computed(&mut self, e: u32, prop: u32, v: Value) -> bool {
        let s = self.alias[self.slot_of(PropRef {
            element: e,
            property: prop,
        }) as usize] as usize;
        let slot = &mut self.slots[s];
        slot.dirty = false;
        if slot.value == v {
            return false;
        }
        slot.value = v;
        self.invalidate(s as u32);
        true
    }

    /// Marks everything that depends on `slot` as dirty.
    fn invalidate(&mut self, slot: u32) {
        self.layout_dirty = true;
        self.needs_redraw = true;
        let mut stack = core::mem::take(&mut self.slots[slot as usize].dependents);
        while let Some(d) = stack.pop() {
            let s = &mut self.slots[d as usize];
            if !s.dirty && s.binding.is_some() {
                s.dirty = true;
                stack.append(&mut s.dependents);
            }
        }
    }

    pub(crate) fn element_by_id(&self, id: &str) -> Option<u32> {
        self.def
            .elements
            .iter()
            .position(|e| e.id == id)
            .map(|i| i as u32)
    }

    pub(crate) fn queue(&mut self, e: u32, callback: &str, args: Vec<Value>) {
        if let Some(i) = self.kind(e).callback_index(callback) {
            let slot = self.cb_slot(CallbackRef {
                element: e,
                callback: i,
            });
            self.pending.push((slot, args));
        }
    }
}

struct Inner {
    state: RefCell<State>,
    native: RefCell<Vec<Option<NativeHandler>>>,
    /// IR handler of each callback slot.
    handlers: Vec<Option<CallbackRef>>,
}

/// A running component. Clone it freely; clones share the same instance.
#[derive(Clone)]
pub struct ComponentInstance {
    inner: Rc<Inner>,
}

impl fmt::Debug for ComponentInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let st = self.inner.state.borrow();
        f.debug_struct("ComponentInstance")
            .field("component", &st.def.name)
            .finish()
    }
}

const MAX_CALL_DEPTH: u32 = 64;

impl ComponentInstance {
    pub fn new(def: Rc<ComponentDef>) -> Self {
        let mut handlers = Vec::new();
        for (ei, e) in def.elements.iter().enumerate() {
            for (ci, c) in e.callbacks.iter().enumerate() {
                handlers.push(c.handler.as_ref().map(|_| CallbackRef {
                    element: ei as u32,
                    callback: ci as u32,
                }));
            }
        }
        let native = (0..handlers.len()).map(|_| None).collect();
        let state = State::new(def);
        Self {
            inner: Rc::new(Inner {
                state: RefCell::new(state),
                native: RefCell::new(native),
                handlers,
            }),
        }
    }

    pub fn from_def(def: ComponentDef) -> Self {
        Self::new(Rc::new(def))
    }

    pub fn definition(&self) -> Rc<ComponentDef> {
        self.inner.state.borrow().def.clone()
    }

    /// Reads a public (`in`, `out` or `in-out`) property of the root component.
    pub fn get_property(&self, name: &str) -> Result<Value, Error> {
        let mut st = self.inner.state.borrow_mut();
        let p = st
            .def
            .public_property(name)
            .ok_or_else(|| Error::UnknownProperty(name.into()))?;
        let slot = st.slot_of(p.prop);
        Ok(st.get_slot(slot))
    }

    /// Writes a public `in` or `in-out` property.
    pub fn set_property(&self, name: &str, value: impl Into<Value>) -> Result<(), Error> {
        let value = value.into();
        let mut st = self.inner.state.borrow_mut();
        let p = st
            .def
            .public_property(name)
            .ok_or_else(|| Error::UnknownProperty(name.into()))?;
        if !p.access.writable() {
            return Err(Error::ReadOnlyProperty(name.into()));
        }
        let ok = value.ty() == p.ty || (p.ty == Type::Float && value.ty() == Type::Int);
        if !ok {
            return Err(Error::TypeMismatch {
                name: name.into(),
                expected: p.ty,
                found: value.ty(),
            });
        }
        let slot = st.slot_of(p.prop);
        st.set_slot(slot, value);
        Ok(())
    }

    /// Reads any property of any element by its `id` (useful in tests and tools).
    pub fn element_property(&self, id: &str, property: &str) -> Result<Value, Error> {
        let mut st = self.inner.state.borrow_mut();
        let e = st
            .element_by_id(id)
            .ok_or_else(|| Error::UnknownElement(id.into()))?;
        let idx = st.def.elements[e as usize]
            .properties
            .iter()
            .position(|p| p.name == property)
            .ok_or_else(|| Error::UnknownProperty(property.into()))?;
        Ok(st.get(e, idx as u32))
    }

    /// Absolute geometry of the element with `id`, after layout.
    pub fn element_geometry(&self, id: &str) -> Result<Rect, Error> {
        self.ensure_layout();
        let st = self.inner.state.borrow();
        let e = st
            .element_by_id(id)
            .ok_or_else(|| Error::UnknownElement(id.into()))?;
        Ok(st.geometry[e as usize])
    }

    /// Sets a Rust handler for a public callback, replacing any `.ui` handler.
    pub fn on_callback(
        &self,
        name: &str,
        f: impl FnMut(&ComponentInstance, &[Value]) + 'static,
    ) -> Result<(), Error> {
        let slot = {
            let st = self.inner.state.borrow();
            let c = st
                .def
                .public_callback(name)
                .ok_or_else(|| Error::UnknownCallback(name.into()))?;
            st.cb_slot(c.callback)
        };
        self.inner.native.borrow_mut()[slot as usize] = Some(Box::new(f));
        Ok(())
    }

    /// Invokes a public callback.
    pub fn invoke(&self, name: &str, args: &[Value]) -> Result<(), Error> {
        let (slot, params) = {
            let st = self.inner.state.borrow();
            let c = st
                .def
                .public_callback(name)
                .ok_or_else(|| Error::UnknownCallback(name.into()))?;
            (st.cb_slot(c.callback), c.params.clone())
        };
        if params.len() != args.len() {
            return Err(Error::ArgumentCount {
                name: name.into(),
                expected: params.len(),
                found: args.len(),
            });
        }
        let args = args
            .iter()
            .cloned()
            .zip(params)
            .map(|(v, t)| v.convert(t))
            .collect();
        self.invoke_slot(slot, args, 0);
        Ok(())
    }

    fn invoke_slot(&self, slot: u32, args: Vec<Value>, depth: u32) {
        if depth > MAX_CALL_DEPTH {
            return;
        }
        let native = self.inner.native.borrow_mut()[slot as usize].take();
        if let Some(mut f) = native {
            f(self, &args);
            let mut natives = self.inner.native.borrow_mut();
            if natives[slot as usize].is_none() {
                natives[slot as usize] = Some(f);
            }
            return;
        }
        if let Some(r) = self.inner.handlers[slot as usize] {
            let def = self.definition();
            if let Some(stmts) =
                &def.elements[r.element as usize].callbacks[r.callback as usize].handler
            {
                self.run(stmts, &args, depth + 1);
            }
        }
    }

    fn run(&self, stmts: &[Stmt], args: &[Value], depth: u32) {
        for stmt in stmts {
            match stmt {
                Stmt::Assign(target, e) => {
                    let mut st = self.inner.state.borrow_mut();
                    let v = st.eval(e, args);
                    let slot = st.slot_of(*target);
                    st.set_slot(slot, v);
                }
                Stmt::Call(cb, exprs) => {
                    let (slot, vals) = {
                        let mut st = self.inner.state.borrow_mut();
                        let vals: Vec<Value> = exprs.iter().map(|e| st.eval(e, args)).collect();
                        (st.cb_slot(*cb), vals)
                    };
                    self.invoke_slot(slot, vals, depth + 1);
                }
                Stmt::If(c, then, otherwise) => {
                    let cond = self
                        .inner
                        .state
                        .borrow_mut()
                        .eval(c, args)
                        .as_bool()
                        .unwrap_or(false);
                    self.run(if cond { then } else { otherwise }, args, depth + 1);
                }
            }
        }
    }

    fn run_pending(&self) {
        loop {
            let pending = core::mem::take(&mut self.inner.state.borrow_mut().pending);
            if pending.is_empty() {
                break;
            }
            for (slot, args) in pending {
                self.invoke_slot(slot, args, 0);
            }
        }
    }

    /// Feeds an input event; runs any callbacks it triggers.
    pub fn dispatch_event(&self, event: Event) {
        self.ensure_layout();
        self.inner.state.borrow_mut().handle_event(event);
        self.run_pending();
    }

    /// Sets the window (viewport) size in logical pixels.
    pub fn set_size(&self, size: Size) {
        let mut st = self.inner.state.borrow_mut();
        if st.size != size {
            st.size = size;
            st.layout_dirty = true;
            st.needs_redraw = true;
        }
    }

    pub fn size(&self) -> Size {
        self.inner.state.borrow().size
    }

    /// Preferred size of the root element (its explicit size, or the size of its content).
    pub fn preferred_size(&self) -> Size {
        let mut st = self.inner.state.borrow_mut();
        st.clear_constraint_cache();
        let w = st
            .declared_size(elements::WIDTH)
            .unwrap_or_else(|| st.constraints(0, true).1);
        let h = st
            .declared_size(elements::HEIGHT)
            .unwrap_or_else(|| st.constraints(0, false).1);
        Size::new(libm::ceilf(w.max(1.0)), libm::ceilf(h.max(1.0)))
    }

    /// The root window's `title` (empty if the root is not a `Window`).
    pub fn window_title(&self) -> String {
        let mut st = self.inner.state.borrow_mut();
        if st.kind(0) == ElementKind::Window {
            st.named_s(0, "title")
        } else {
            String::new()
        }
    }

    /// Whether something changed since the last [`render`](Self::render).
    pub fn needs_redraw(&self) -> bool {
        self.inner.state.borrow().needs_redraw
    }

    /// Element id that currently has keyboard focus.
    pub fn focused_element(&self) -> Option<String> {
        let st = self.inner.state.borrow();
        st.focused.map(|e| st.def.elements[e as usize].id.clone())
    }

    fn ensure_layout(&self) {
        let mut st = self.inner.state.borrow_mut();
        if st.size == Size::default() {
            drop(st);
            let s = self.preferred_size();
            st = self.inner.state.borrow_mut();
            st.size = s;
        }
        if st.layout_dirty {
            st.layout();
        }
    }

    /// Renders the whole frame into `target` (1 logical pixel = 1 device pixel).
    pub fn render(&self, target: &mut dyn Target) {
        self.render_band(target, 0.0, 1.0);
    }

    /// Renders with `scale` device pixels per logical pixel (HiDPI screens).
    pub fn render_scaled(&self, target: &mut dyn Target, scale: f32) {
        self.render_band(target, 0.0, scale);
    }

    /// Renders device rows `offset_y .. offset_y + target.height()` of the
    /// frame, for devices without RAM for a full framebuffer.
    pub fn render_band(&self, target: &mut dyn Target, offset_y: f32, scale: f32) {
        self.ensure_layout();
        let mut st = self.inner.state.borrow_mut();
        let mut canvas = Canvas::with_transform(target, offset_y, scale);
        st.paint(&mut canvas);
        st.needs_redraw = false;
    }
}

#[cfg(test)]
mod tests;
