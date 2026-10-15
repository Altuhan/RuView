//! # kite-android
//!
//! Embeds Kite components in Android apps as a regular `View`.
//!
//! The Kotlin side is `dev.kite.KiteView` (see `examples/android-pos`). It
//! owns a `Bitmap` and forwards size changes, touches and keyboard input;
//! this crate renders frames into the bitmap's pixel buffer and routes
//! component callbacks back to Kotlin.
//!
//! [`Host`] holds all platform-independent logic and is unit-tested on the
//! desktop. The JNI entry points are generated in the app's `cdylib` with
//! [`kite_view_jni!`], which ties a concrete component type to `KiteView`.

use kite_core::{ComponentInstance, Event, Key, Point, PointerButton, Size, Target, Type, Value};

#[doc(hidden)]
pub use jni;

/// Android `MotionEvent` action codes used by [`Host::touch`].
pub mod action {
    pub const DOWN: i32 = 0;
    pub const UP: i32 = 1;
    pub const MOVE: i32 = 2;
    pub const CANCEL: i32 = 3;
}

/// Platform-independent state behind one `KiteView`.
pub struct Host {
    pub instance: ComponentInstance,
    /// Physical pixels per logical pixel (Android's `displayMetrics.density`).
    pub density: f32,
    width: u32,
    height: u32,
    touching: bool,
    last: Point,
}

impl Host {
    pub fn new(instance: ComponentInstance) -> Self {
        Self {
            instance,
            density: 1.0,
            width: 0,
            height: 0,
            touching: false,
            last: Point::default(),
        }
    }

    /// The view's size in physical pixels changed.
    pub fn resize(&mut self, width: u32, height: u32, density: f32) {
        self.width = width;
        self.height = height;
        self.density = if density > 0.0 { density } else { 1.0 };
        self.instance.set_size(Size::new(
            width as f32 / self.density,
            height as f32 / self.density,
        ));
    }

    fn logical(&self, x: f32, y: f32) -> Point {
        Point::new(x / self.density, y / self.density)
    }

    /// A touch event at physical pixel coordinates. Returns whether to redraw.
    pub fn touch(&mut self, action: i32, x: f32, y: f32) -> bool {
        let position = self.logical(x, y);
        let left = PointerButton::Left;
        match action {
            action::DOWN => {
                self.touching = true;
                self.instance
                    .dispatch_event(Event::PointerMoved { position });
                self.instance.dispatch_event(Event::PointerPressed {
                    position,
                    button: left,
                });
            }
            action::MOVE if self.touching => self
                .instance
                .dispatch_event(Event::PointerMoved { position }),
            action::UP if self.touching => {
                self.touching = false;
                self.instance.dispatch_event(Event::PointerReleased {
                    position,
                    button: left,
                });
                // Touch screens have no hover: clear it once the finger lifts.
                self.instance.dispatch_event(Event::PointerExited);
            }
            action::CANCEL if self.touching => {
                self.touching = false;
                // Release outside every element so nothing activates.
                let outside = Point::new(-1.0, -1.0);
                self.instance.dispatch_event(Event::PointerReleased {
                    position: outside,
                    button: left,
                });
                self.instance.dispatch_event(Event::PointerExited);
            }
            _ => {}
        }
        self.last = position;
        self.instance.needs_redraw()
    }

    /// Committed text from the soft keyboard or a hardware key.
    pub fn text(&mut self, text: &str) -> bool {
        self.instance.dispatch_event(Event::Text(text.into()));
        self.instance.needs_redraw()
    }

    /// An Android `KeyEvent` key code. Returns whether it was handled.
    pub fn key(&mut self, code: i32, shift: bool) -> bool {
        let key = match code {
            67 => Key::Backspace,        // KEYCODE_DEL
            112 => Key::Delete,          // KEYCODE_FORWARD_DEL
            21 => Key::Left,             // KEYCODE_DPAD_LEFT
            22 => Key::Right,            // KEYCODE_DPAD_RIGHT
            19 => Key::Up,               // KEYCODE_DPAD_UP
            20 => Key::Down,             // KEYCODE_DPAD_DOWN
            122 => Key::Home,            // KEYCODE_MOVE_HOME
            123 => Key::End,             // KEYCODE_MOVE_END
            66 | 160 => Key::Enter,      // KEYCODE_ENTER, KEYCODE_NUMPAD_ENTER
            62 => Key::Space,            // KEYCODE_SPACE
            61 if shift => Key::BackTab, // KEYCODE_TAB
            61 => Key::Tab,
            111 => Key::Escape, // KEYCODE_ESCAPE
            _ => return false,
        };
        self.instance.dispatch_event(Event::KeyPressed(key));
        if key == Key::Space {
            // Buttons react to the key; text fields need the character.
            self.instance.dispatch_event(Event::Text(" ".into()));
        }
        true
    }

    pub fn wants_keyboard(&self) -> bool {
        self.instance.text_input_focused()
    }

    /// Renders the frame into an Android `ARGB_8888` bitmap buffer (RGBA bytes in memory).
    pub fn render_rgba(&mut self, pixels: &mut [u8], width: u32, height: u32) {
        if pixels.len() < (width * height * 4) as usize {
            return;
        }
        let mut target = RgbaBytes {
            width,
            height,
            pixels,
        };
        self.instance.render_scaled(&mut target, self.density);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// A public property as display text (`""` if unknown).
    pub fn property(&self, name: &str) -> String {
        self.instance
            .get_property(name)
            .map(|v| v.to_string())
            .unwrap_or_default()
    }

    /// Sets a public property from text, converted to its declared type.
    pub fn set_property(&mut self, name: &str, text: &str) -> Result<(), String> {
        let def = self.instance.definition();
        let p = def
            .public_property(name)
            .ok_or_else(|| format!("no public property `{name}`"))?;
        let value = match p.ty {
            Type::Bool => Value::Bool(text.trim().parse().map_err(|_| "expected true or false")?),
            Type::Int => Value::Int(text.trim().parse().map_err(|_| "expected an integer")?),
            Type::Float => Value::Float(text.trim().parse().map_err(|_| "expected a number")?),
            Type::String => Value::String(text.into()),
            Type::Color => {
                Value::Color(kite_core::Color::from_hex(text.trim()).ok_or("expected #rrggbb")?)
            }
            Type::Enum(e) => Value::Enum(e, e.variant_index(text.trim()).ok_or("unknown variant")?),
            Type::Void => return Err("void property".into()),
        };
        self.instance
            .set_property(name, value)
            .map_err(|e| e.to_string())
    }

    /// `[x, y, width, height]` of element `id` in physical pixels (for UI tests).
    pub fn element_rect(&self, id: &str) -> Option<[f32; 4]> {
        let g = self.instance.element_geometry(id).ok()?;
        let d = self.density;
        Some([g.x * d, g.y * d, g.width * d, g.height * d])
    }
}

/// A borrowed RGBA8888 buffer (Android `Bitmap.Config.ARGB_8888` memory layout).
pub struct RgbaBytes<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u8],
}

impl Target for RgbaBytes<'_> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    #[inline]
    fn blend(&mut self, x: u32, y: u32, color: kite_core::Color, coverage: u8) {
        let i = ((y * self.width + x) * 4) as usize;
        let p = &mut self.pixels[i..i + 4];
        let dst = kite_core::Color::rgb(p[0], p[1], p[2]);
        let c = dst.blend_under(color, coverage);
        p.copy_from_slice(&[c.r, c.g, c.b, 255]);
    }
    fn fill_span(&mut self, x0: u32, x1: u32, y: u32, color: kite_core::Color) {
        if color.a == 255 {
            let row = ((y * self.width + x0) * 4) as usize;
            let px = [color.r, color.g, color.b, 255];
            for chunk in self.pixels[row..row + ((x1 - x0) * 4) as usize].chunks_exact_mut(4) {
                chunk.copy_from_slice(&px);
            }
        } else {
            for x in x0..x1 {
                self.blend(x, y, color, 255);
            }
        }
    }
}

/// Callback arguments as display strings, as passed to Kotlin.
pub fn callback_args(args: &[Value]) -> Vec<String> {
    args.iter().map(|v| v.to_string()).collect()
}

/// Generates the JNI functions behind `dev.kite.KiteView` for component type
/// `$component` (a struct generated by `kite-build`). Use it once in the
/// app's `cdylib` crate:
///
/// ```ignore
/// kite::include_ui!("pos");
/// kite_android::kite_view_jni!(Register);
/// ```
#[macro_export]
macro_rules! kite_view_jni {
    ($component:ty) => {
        mod __kite_view_jni {
            use super::*;
            use $crate::jni::objects::{JByteBuffer, JClass, JObject, JString, JValue};
            use $crate::jni::sys::{jboolean, jfloat, jfloatArray, jint, jlong, jstring};
            use $crate::jni::JNIEnv;
            use $crate::Host;

            fn host<'a>(handle: jlong) -> &'a mut Host {
                // SAFETY: `handle` comes from `nativeCreate` and stays valid
                // until `nativeDestroy`; KiteView only calls on the UI thread.
                unsafe { &mut *(handle as *mut Host) }
            }

            fn jstr(env: &mut JNIEnv, s: &JString) -> String {
                env.get_string(s).map(|s| s.into()).unwrap_or_default()
            }

            /// Creates the component; `view` receives `onKiteCallback(name, args)`.
            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeCreate(
                mut env: JNIEnv,
                _class: JClass,
                view: JObject,
            ) -> jlong {
                let component = <$component>::new();
                let instance = kite::Component::instance(&component).clone();
                if let (Ok(vm), Ok(view)) = (env.get_java_vm(), env.new_global_ref(view)) {
                    let vm = std::sync::Arc::new(vm);
                    for cb in &instance.definition().public_callbacks {
                        let (vm, view, name) = (vm.clone(), view.clone(), cb.name.clone());
                        let _ = instance.on_callback(&cb.name, move |_, args| {
                            let Ok(mut env) = vm.attach_current_thread() else {
                                return;
                            };
                            let args = $crate::callback_args(args);
                            let Ok(jname) = env.new_string(&name) else {
                                return;
                            };
                            let Ok(array) = env.new_object_array(
                                args.len() as i32,
                                "java/lang/String",
                                JObject::null(),
                            ) else {
                                return;
                            };
                            for (i, a) in args.iter().enumerate() {
                                if let Ok(s) = env.new_string(a) {
                                    let _ = env.set_object_array_element(&array, i as i32, s);
                                }
                            }
                            let _ = env.call_method(
                                view.as_obj(),
                                "onKiteCallback",
                                "(Ljava/lang/String;[Ljava/lang/String;)V",
                                &[JValue::Object(&jname), JValue::Object(&array)],
                            );
                            if env.exception_check().unwrap_or(false) {
                                let _ = env.exception_describe();
                                let _ = env.exception_clear();
                            }
                        });
                    }
                }
                Box::into_raw(Box::new(Host::new(instance))) as jlong
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeDestroy(
                _env: JNIEnv,
                _class: JClass,
                handle: jlong,
            ) {
                if handle != 0 {
                    // SAFETY: created by `Box::into_raw` in nativeCreate; destroyed once.
                    drop(unsafe { Box::from_raw(handle as *mut Host) });
                }
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeResize(
                _env: JNIEnv,
                _class: JClass,
                handle: jlong,
                width: jint,
                height: jint,
                density: jfloat,
            ) {
                host(handle).resize(width.max(0) as u32, height.max(0) as u32, density);
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeTouch(
                _env: JNIEnv,
                _class: JClass,
                handle: jlong,
                action: jint,
                x: jfloat,
                y: jfloat,
            ) -> jboolean {
                host(handle).touch(action, x, y) as jboolean
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeText(
                mut env: JNIEnv,
                _class: JClass,
                handle: jlong,
                text: JString,
            ) -> jboolean {
                let text = jstr(&mut env, &text);
                host(handle).text(&text) as jboolean
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeKey(
                _env: JNIEnv,
                _class: JClass,
                handle: jlong,
                code: jint,
                shift: jboolean,
            ) -> jboolean {
                host(handle).key(code, shift != 0) as jboolean
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeWantsKeyboard(
                _env: JNIEnv,
                _class: JClass,
                handle: jlong,
            ) -> jboolean {
                host(handle).wants_keyboard() as jboolean
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeNeedsRedraw(
                _env: JNIEnv,
                _class: JClass,
                handle: jlong,
            ) -> jboolean {
                host(handle).instance.needs_redraw() as jboolean
            }

            /// Renders into a direct ByteBuffer of `width * height * 4` bytes.
            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeRender(
                env: JNIEnv,
                _class: JClass,
                handle: jlong,
                buffer: JByteBuffer,
                width: jint,
                height: jint,
            ) {
                let (Ok(ptr), Ok(len)) = (
                    env.get_direct_buffer_address(&buffer),
                    env.get_direct_buffer_capacity(&buffer),
                ) else {
                    return;
                };
                if ptr.is_null() {
                    return;
                }
                // SAFETY: the direct buffer is valid for `len` bytes and not
                // accessed by Java while this call runs on the UI thread.
                let pixels = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
                host(handle).render_rgba(pixels, width.max(0) as u32, height.max(0) as u32);
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeGetProperty(
                mut env: JNIEnv,
                _class: JClass,
                handle: jlong,
                name: JString,
            ) -> jstring {
                let name = jstr(&mut env, &name);
                let value = host(handle).property(&name);
                env.new_string(value)
                    .map(|s| s.into_raw())
                    .unwrap_or(std::ptr::null_mut())
            }

            /// Returns "" on success or an error message.
            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeSetProperty(
                mut env: JNIEnv,
                _class: JClass,
                handle: jlong,
                name: JString,
                value: JString,
            ) -> jstring {
                let (name, value) = (jstr(&mut env, &name), jstr(&mut env, &value));
                let msg = host(handle)
                    .set_property(&name, &value)
                    .err()
                    .unwrap_or_default();
                env.new_string(msg)
                    .map(|s| s.into_raw())
                    .unwrap_or(std::ptr::null_mut())
            }

            #[no_mangle]
            pub extern "system" fn Java_dev_kite_KiteView_nativeElementRect(
                mut env: JNIEnv,
                _class: JClass,
                handle: jlong,
                id: JString,
            ) -> jfloatArray {
                let id = jstr(&mut env, &id);
                let rect = host(handle).element_rect(&id).unwrap_or([0.0; 4]);
                match env.new_float_array(4) {
                    Ok(arr) => {
                        let _ = env.set_float_array_region(&arr, 0, &rect);
                        arr.into_raw()
                    }
                    Err(_) => std::ptr::null_mut(),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos() -> Host {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/ui/pos.ui");
        let c = kite_compiler::compile_file(path).unwrap_or_else(|d| panic!("{d}"));
        Host::new(c.instantiate())
    }

    fn tap(h: &mut Host, id: &str) {
        let [x, y, w, hh] = h.element_rect(id).unwrap();
        h.touch(action::DOWN, x + w / 2.0, y + hh / 2.0);
        h.touch(action::UP, x + w / 2.0, y + hh / 2.0);
    }

    #[test]
    fn tablet_sale_with_density_and_keyboard() {
        let mut h = pos();
        // Pixel Tablet landscape: 2560x1600 px at density 2.0 (1280x800 dp).
        h.resize(2560, 1600, 2.0);
        tap(&mut h, "sourdough");
        tap(&mut h, "sourdough");
        tap(&mut h, "coffee");
        // Focus the PLU field: the host should ask for the soft keyboard.
        let field = h.element_rect("plu").unwrap();
        h.touch(action::DOWN, field[0] + 10.0, field[1] + 10.0);
        h.touch(action::UP, field[0] + 10.0, field[1] + 10.0);
        assert!(h.wants_keyboard());
        h.text("4011");
        assert!(h.key(66, false));
        assert_eq!(h.property("items"), "4");
        assert_eq!(h.property("total"), "25.57");
        let mut px = vec![0u8; 2560 * 1600 * 4];
        h.render_rgba(&mut px, 2560, 1600);
        assert!(px.chunks_exact(4).all(|p| p[3] == 255), "opaque output");
        // Top-left is the window background #f1f5f9.
        assert_eq!(&px[..4], &[0xf1, 0xf5, 0xf9, 0xff]);
    }

    #[test]
    fn cancelled_touch_does_not_click() {
        let mut h = pos();
        h.resize(1280, 800, 1.0);
        let [x, y, w, hh] = h.element_rect("milk").unwrap();
        h.touch(action::DOWN, x + w / 2.0, y + hh / 2.0);
        h.touch(action::CANCEL, x + w / 2.0, y + hh / 2.0);
        assert_eq!(h.property("items"), "0");
        assert!(h.set_property("member", "yes").is_err());
        assert!(h.set_property("member", "true").is_ok());
        assert!(!h.key(9999, false));
    }
}
