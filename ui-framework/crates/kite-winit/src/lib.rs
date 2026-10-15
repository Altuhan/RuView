//! # kite-winit
//!
//! Runs Kite components in a native desktop window (Linux X11/Wayland,
//! Windows, macOS) using winit for windowing and softbuffer to present the
//! software-rendered frame.

use std::error::Error;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use kite_core::{ComponentInstance, Event, Key, Point, PointerButton, Rgb32Buffer, Size};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// Reloads a component (used by live preview).
pub type Loader = Box<dyn FnMut() -> Result<ComponentInstance, String>>;

/// Opens a window showing `component` and runs until it is closed.
pub fn run(component: &(impl kite_core::Component + ?Sized)) -> Result<(), Box<dyn Error>> {
    run_app(App::new(component.instance().clone(), None))
}

/// Like [`run`], but reloads via `loader` whenever `watch` changes on disk.
pub fn run_live(watch: PathBuf, mut loader: Loader) -> Result<(), Box<dyn Error>> {
    let first = loader().map_err(|e| -> Box<dyn Error> { e.into() })?;
    let mtime = modified(&watch);
    run_app(App::new(first, Some(Live { path: watch, loader, mtime, next_check: Instant::now() })))
}

fn run_app(mut app: App) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;
    match app.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn modified(p: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

struct Live {
    path: PathBuf,
    loader: Loader,
    mtime: Option<SystemTime>,
    next_check: Instant,
}

struct Gfx {
    window: Rc<Window>,
    surface: Surface,
    _context: softbuffer::Context<Rc<Window>>,
}

struct App {
    instance: ComponentInstance,
    gfx: Option<Gfx>,
    live: Option<Live>,
    cursor: Point,
    modifiers: ModifiersState,
    title: String,
    error: Option<Box<dyn Error>>,
}

impl App {
    fn new(instance: ComponentInstance, live: Option<Live>) -> Self {
        Self { instance, gfx: None, live, cursor: Point::default(), modifiers: ModifiersState::empty(), title: String::new(), error: None }
    }

    fn scale(&self) -> f32 {
        self.gfx.as_ref().map(|g| g.window.scale_factor() as f32).unwrap_or(1.0)
    }

    fn sync_size(&mut self) {
        if let Some(g) = &self.gfx {
            let s = g.window.inner_size();
            let f = g.window.scale_factor() as f32;
            self.instance.set_size(Size::new(s.width as f32 / f, s.height as f32 / f));
        }
    }

    fn after_input(&mut self) {
        if let Some(g) = &self.gfx {
            let title = self.instance.window_title();
            if title != self.title {
                g.window.set_title(&title);
                self.title = title;
            }
            if self.instance.needs_redraw() {
                g.window.request_redraw();
            }
        }
    }

    fn fail(&mut self, el: &ActiveEventLoop, e: impl Into<Box<dyn Error>>) {
        self.error = Some(e.into());
        el.exit();
    }

    fn redraw(&mut self) -> Result<(), Box<dyn Error>> {
        let scale = self.scale();
        let Some(g) = &mut self.gfx else { return Ok(()) };
        let PhysicalSize { width, height } = g.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else { return Ok(()) };
        g.surface.resize(w, h)?;
        let mut buffer = g.surface.buffer_mut()?;
        let mut target = Rgb32Buffer { width, height, pixels: &mut buffer };
        self.instance.render_scaled(&mut target, scale);
        buffer.present()?;
        Ok(())
    }

    fn check_reload(&mut self) {
        let Some(live) = &mut self.live else { return };
        if Instant::now() < live.next_check {
            return;
        }
        live.next_check = Instant::now() + Duration::from_millis(250);
        let m = modified(&live.path);
        if m == live.mtime {
            return;
        }
        live.mtime = m;
        match (live.loader)() {
            Ok(new) => {
                eprintln!("[kite] reloaded {}", live.path.display());
                self.instance = new;
                self.sync_size();
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            Err(e) => eprintln!("{e}\n[kite] keeping the previous version"),
        }
    }
}

fn map_key(key: &WKey, shift: bool) -> Option<Key> {
    let WKey::Named(n) = key else { return None };
    Some(match n {
        NamedKey::Backspace => Key::Backspace,
        NamedKey::Delete => Key::Delete,
        NamedKey::ArrowLeft => Key::Left,
        NamedKey::ArrowRight => Key::Right,
        NamedKey::ArrowUp => Key::Up,
        NamedKey::ArrowDown => Key::Down,
        NamedKey::Home => Key::Home,
        NamedKey::End => Key::End,
        NamedKey::Enter => Key::Enter,
        NamedKey::Space => Key::Space,
        NamedKey::Tab if shift => Key::BackTab,
        NamedKey::Tab => Key::Tab,
        NamedKey::Escape => Key::Escape,
        _ => return None,
    })
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let pref = self.instance.preferred_size();
        self.title = self.instance.window_title();
        let attrs = Window::default_attributes()
            .with_title(if self.title.is_empty() { "Kite" } else { &self.title })
            .with_inner_size(LogicalSize::new(pref.width.max(64.0) as f64, pref.height.max(48.0) as f64));
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => return self.fail(el, e),
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => return self.fail(el, e.to_string()),
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => return self.fail(el, e.to_string()),
        };
        self.gfx = Some(Gfx { window, surface, _context: context });
        self.sync_size();
        if self.live.is_some() {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250)));
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let scale = self.scale();
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.sync_size();
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(el, e.to_string());
                }
            }
            WindowEvent::CursorMoved { position: PhysicalPosition { x, y }, .. } => {
                self.cursor = Point::new(x as f32 / scale, y as f32 / scale);
                self.instance.dispatch_event(Event::PointerMoved { position: self.cursor });
            }
            WindowEvent::CursorLeft { .. } => self.instance.dispatch_event(Event::PointerExited),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return,
                };
                let position = self.cursor;
                self.instance.dispatch_event(match state {
                    ElementState::Pressed => Event::PointerPressed { position, button },
                    ElementState::Released => Event::PointerReleased { position, button },
                });
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(k) = map_key(&event.logical_key, self.modifiers.shift_key()) {
                    self.instance.dispatch_event(Event::KeyPressed(k));
                }
                let ctrl = self.modifiers.control_key() || self.modifiers.super_key();
                if let (Some(text), false) = (&event.text, ctrl) {
                    if text.chars().any(|c| !c.is_control()) {
                        self.instance.dispatch_event(Event::Text(text.to_string()));
                    }
                }
            }
            _ => {}
        }
        self.after_input();
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.live.is_some() {
            self.check_reload();
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250)));
        }
    }
}
