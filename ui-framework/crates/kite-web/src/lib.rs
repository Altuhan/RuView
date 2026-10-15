//! # kite-web
//!
//! Runs Kite components in the browser. The software renderer draws into a
//! pixel buffer that is copied into a `<canvas>` with `putImageData`, so the
//! output matches the desktop and embedded backends pixel for pixel.
//!
//! ```ignore
//! #[wasm_bindgen(start)]
//! pub fn start() -> Result<(), JsValue> {
//!     let app = Counter::new();
//!     kite_web::mount_by_id(app.instance(), "app")?;
//!     Ok(())
//! }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use kite_core::{ComponentInstance, Event, Key, Point, PointerButton, RgbBuffer, Size};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, ImageData, KeyboardEvent, PointerEvent,
};

struct State {
    instance: ComponentInstance,
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    buffer: RgbBuffer,
    scale: f32,
    frame_pending: bool,
}

/// Keeps the event listeners alive. Dropping it detaches nothing (listeners
/// live as long as the page), so it is usually just ignored.
pub struct Mounted {
    _state: Rc<RefCell<State>>,
}

/// Mounts `instance` into the canvas with id `canvas_id`.
pub fn mount_by_id(instance: &ComponentInstance, canvas_id: &str) -> Result<Mounted, JsValue> {
    let document = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;
    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str(&format!("no element with id `{canvas_id}`")))?
        .dyn_into::<HtmlCanvasElement>()?;
    mount(instance, canvas)
}

/// Mounts `instance` into `canvas`. If the canvas has no CSS size, it is
/// sized to the component's preferred size; otherwise the UI follows the
/// canvas's CSS size.
pub fn mount(instance: &ComponentInstance, canvas: HtmlCanvasElement) -> Result<Mounted, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let ctx = canvas
        .get_context("2d")?
        .ok_or("2d context unavailable")?
        .dyn_into::<CanvasRenderingContext2d>()?;
    if canvas.client_width() == 0 || canvas.client_height() == 0 {
        let pref = instance.preferred_size();
        let style = canvas.style();
        style.set_property("width", &format!("{}px", pref.width))?;
        style.set_property("height", &format!("{}px", pref.height))?;
    }
    canvas.set_tab_index(0);
    let state = Rc::new(RefCell::new(State {
        instance: instance.clone(),
        canvas: canvas.clone(),
        ctx,
        buffer: RgbBuffer::new(1, 1),
        scale: window.device_pixel_ratio() as f32,
        frame_pending: false,
    }));
    resize(&state);
    render(&state);

    let pos = |e: &PointerEvent| Point::new(e.offset_x() as f32, e.offset_y() as f32);
    let button = |b: i16| match b {
        1 => PointerButton::Middle,
        2 => PointerButton::Right,
        _ => PointerButton::Left,
    };
    listen(&canvas, "pointermove", &state, move |s, e: PointerEvent| {
        s.instance
            .dispatch_event(Event::PointerMoved { position: pos(&e) });
    })?;
    listen(&canvas, "pointerdown", &state, move |s, e: PointerEvent| {
        let _ = s.canvas.focus();
        let _ = s.canvas.set_pointer_capture(e.pointer_id());
        s.instance.dispatch_event(Event::PointerPressed {
            position: pos(&e),
            button: button(e.button()),
        });
        e.prevent_default();
    })?;
    listen(&canvas, "pointerup", &state, move |s, e: PointerEvent| {
        s.instance.dispatch_event(Event::PointerReleased {
            position: pos(&e),
            button: button(e.button()),
        });
    })?;
    listen(&canvas, "pointerleave", &state, |s, _e: PointerEvent| {
        s.instance.dispatch_event(Event::PointerExited);
    })?;
    listen(&canvas, "keydown", &state, |s, e: KeyboardEvent| {
        let key = match e.key().as_str() {
            "Backspace" => Some(Key::Backspace),
            "Delete" => Some(Key::Delete),
            "ArrowLeft" => Some(Key::Left),
            "ArrowRight" => Some(Key::Right),
            "ArrowUp" => Some(Key::Up),
            "ArrowDown" => Some(Key::Down),
            "Home" => Some(Key::Home),
            "End" => Some(Key::End),
            "Enter" => Some(Key::Enter),
            " " => Some(Key::Space),
            "Tab" if e.shift_key() => Some(Key::BackTab),
            "Tab" => Some(Key::Tab),
            "Escape" => Some(Key::Escape),
            _ => None,
        };
        let mut handled = false;
        if let Some(k) = key {
            s.instance.dispatch_event(Event::KeyPressed(k));
            handled = true;
        }
        let text = e.key();
        if text.chars().count() == 1 && !e.ctrl_key() && !e.meta_key() {
            s.instance.dispatch_event(Event::Text(text));
            handled = true;
        }
        if handled {
            e.prevent_default();
        }
    })?;
    let resize_state = state.clone();
    let on_resize = Closure::<dyn FnMut()>::new(move || {
        resize(&resize_state);
        schedule(&resize_state);
    });
    window.add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref())?;
    on_resize.forget();
    Ok(Mounted { _state: state })
}

fn listen<E: JsCast + 'static>(
    canvas: &HtmlCanvasElement,
    name: &str,
    state: &Rc<RefCell<State>>,
    mut f: impl FnMut(&State, E) + 'static,
) -> Result<(), JsValue> {
    let st = state.clone();
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        if let Ok(e) = e.dyn_into::<E>() {
            f(&st.borrow(), e);
        }
        schedule(&st);
    });
    canvas.add_event_listener_with_callback(name, cb.as_ref().unchecked_ref())?;
    cb.forget();
    Ok(())
}

fn resize(state: &Rc<RefCell<State>>) {
    let mut s = state.borrow_mut();
    let scale = web_sys::window()
        .map(|w| w.device_pixel_ratio() as f32)
        .unwrap_or(1.0);
    let (w, h) = (
        s.canvas.client_width().max(1) as f32,
        s.canvas.client_height().max(1) as f32,
    );
    s.scale = scale;
    s.instance.set_size(Size::new(w, h));
    let (pw, ph) = ((w * scale).round() as u32, (h * scale).round() as u32);
    s.canvas.set_width(pw);
    s.canvas.set_height(ph);
    s.buffer.resize(pw, ph);
}

/// Requests an animation frame if the UI needs repainting.
fn schedule(state: &Rc<RefCell<State>>) {
    {
        let mut s = state.borrow_mut();
        if s.frame_pending || !s.instance.needs_redraw() {
            return;
        }
        s.frame_pending = true;
    }
    let st = state.clone();
    let cb = Closure::once_into_js(move || {
        st.borrow_mut().frame_pending = false;
        render(&st);
    });
    if let Some(w) = web_sys::window() {
        let _ = w.request_animation_frame(cb.unchecked_ref());
    }
}

fn render(state: &Rc<RefCell<State>>) {
    let mut s = state.borrow_mut();
    let State {
        instance,
        ctx,
        buffer,
        scale,
        ..
    } = &mut *s;
    instance.render_scaled(buffer, *scale);
    let rgba = buffer.to_rgba8();
    if let Ok(img) =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), buffer.width, buffer.height)
    {
        let _ = ctx.put_image_data(&img, 0.0, 0.0);
    }
}
