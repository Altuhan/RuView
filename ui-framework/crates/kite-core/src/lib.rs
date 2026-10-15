//! # kite-core
//!
//! The runtime of the Kite UI framework. It is `no_std` (with `alloc`) so the
//! same compiled UI runs on desktops, in the browser and on microcontrollers.
//!
//! * [`ir`] — the compiled component description produced by `kite-compiler`
//! * [`ComponentInstance`] — reactive properties, callbacks, layout and input
//! * [`raster`] — an anti-aliased software renderer drawing into any [`raster::Target`]
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod color;
pub mod elements;
pub mod font;
pub mod geometry;
pub mod instance;
pub mod ir;
pub mod raster;
pub mod value;

pub use color::Color;
pub use elements::ElementKind;
pub use geometry::{Point, Rect, Size};
pub use instance::{ComponentInstance, Error, Event, Key, NativeHandler, PointerButton};
pub use ir::ComponentDef;
pub use raster::{Canvas, Rgb565Buffer, RgbBuffer, Target};
pub use value::{EnumType, Type, Value};

/// Implemented by components generated from `.ui` files.
pub trait Component {
    /// The underlying dynamic instance.
    fn instance(&self) -> &ComponentInstance;
}

impl Component for ComponentInstance {
    fn instance(&self) -> &ComponentInstance {
        self
    }
}

/// Re-exports used by generated code. Not a stable API.
#[doc(hidden)]
pub mod __private {
    pub use alloc::boxed::Box;
    pub use alloc::rc::Rc;
    pub use alloc::string::String;
    pub use alloc::vec;
    pub use alloc::vec::Vec;
}
