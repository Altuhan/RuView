//! In-browser Kite playground: the `.ui` compiler and runtime both run as
//! WebAssembly. The page sends source text, this crate compiles it and shows
//! the result on a canvas, and the page polls public state and callback calls.

use std::cell::RefCell;

use kite_core::{ir::Access, ComponentInstance, Type, Value};
use wasm_bindgen::prelude::*;

thread_local! {
    static MOUNTED: RefCell<Option<kite_web::Mounted>> = const { RefCell::new(None) };
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn current() -> Option<ComponentInstance> {
    MOUNTED.with(|m| m.borrow().as_ref().map(|m| m.instance()))
}

/// Compiles `source` and shows it in the canvas `canvas_id`.
/// Returns "" on success or the rendered compiler diagnostics.
#[wasm_bindgen]
pub fn compile(source: &str, canvas_id: &str) -> String {
    let compiled = match kite_compiler::compile_source("app.ui", source) {
        Ok(c) => c,
        Err(d) => return d.to_string(),
    };
    let app = compiled.instantiate();
    // Route every public callback to a log, as a Rust handler would.
    for cb in &app.definition().public_callbacks {
        let name = cb.name.clone();
        let _ = app.on_callback(&cb.name, move |_, args| {
            let args: Vec<String> = args
                .iter()
                .map(|a| match a {
                    Value::String(s) => format!("{s:?}"),
                    v => v.to_string(),
                })
                .collect();
            LOG.with(|l| l.borrow_mut().push(format!("{name}({})", args.join(", "))));
        });
    }
    let result = MOUNTED.with(|m| {
        let mut m = m.borrow_mut();
        match &*m {
            Some(mounted) => mounted.replace(&app),
            None => kite_web::mount_by_id(&app, canvas_id).and_then(|mounted| {
                // Size the canvas to the component (a bare canvas defaults to 300x150).
                mounted.replace(&app)?;
                *m = Some(mounted);
                Ok(())
            }),
        }
    });
    let warnings: Vec<String> = compiled
        .diagnostics
        .warnings()
        .map(|w| compiled.diagnostics.render(w))
        .collect();
    match result {
        Ok(()) if warnings.is_empty() => String::new(),
        Ok(()) => format!("(compiled with warnings)\n{}", warnings.join("\n")),
        Err(e) => format!("failed to mount: {e:?}"),
    }
}

/// Public properties of the running component as JSON:
/// `[{"name","type","access","value"}]`.
#[wasm_bindgen]
pub fn state_json() -> String {
    let Some(app) = current() else {
        return "[]".into();
    };
    let def = app.definition();
    let items: Vec<String> = def
        .public_properties
        .iter()
        .map(|p| {
            let access = match p.access {
                Access::In => "in",
                Access::Out => "out",
                Access::InOut => "in-out",
            };
            let value = app
                .get_property(&p.name)
                .map(|v| v.to_string())
                .unwrap_or_default();
            format!(
                "{{\"name\":{},\"type\":{},\"access\":\"{access}\",\"value\":{}}}",
                json_str(&p.name),
                json_str(p.ty.name()),
                json_str(&value)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Callback invocations since the last call, as a JSON array of strings.
#[wasm_bindgen]
pub fn take_log() -> String {
    let lines = LOG.with(|l| std::mem::take(&mut *l.borrow_mut()));
    let items: Vec<String> = lines.iter().map(|s| json_str(s)).collect();
    format!("[{}]", items.join(","))
}

/// Sets a public property from outside the UI (like Rust's `set_<name>`).
/// Returns "" on success or an error message.
#[wasm_bindgen]
pub fn set_property(name: &str, text: &str) -> String {
    let Some(app) = current() else {
        return "no component".into();
    };
    let def = app.definition();
    let Some(p) = def.public_property(name) else {
        return format!("no property `{name}`");
    };
    let value = match p.ty {
        Type::Bool => text
            .trim()
            .parse()
            .map(Value::Bool)
            .map_err(|_| "expected true or false"),
        Type::Int => text
            .trim()
            .parse()
            .map(Value::Int)
            .map_err(|_| "expected an integer"),
        Type::Float => text
            .trim()
            .parse()
            .map(Value::Float)
            .map_err(|_| "expected a number"),
        Type::String => Ok(Value::String(text.into())),
        Type::Color => kite_core::Color::from_hex(text.trim())
            .map(Value::Color)
            .ok_or("expected #rrggbb"),
        Type::Enum(e) => e
            .variant_index(text.trim())
            .map(|i| Value::Enum(e, i))
            .ok_or("unknown variant"),
        Type::Void => Err("void"),
    };
    match value {
        Ok(v) => app
            .set_property(name, v)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default(),
        Err(e) => e.into(),
    }
}

/// The typed Rust API `kite-build` would generate for `source` (signatures only).
#[wasm_bindgen]
pub fn rust_api(source: &str) -> String {
    let Ok(compiled) = kite_compiler::compile_source("app.ui", source) else {
        return String::new();
    };
    let code = kite_compiler::generate_rust(&compiled, "::kite", "app.ui");
    let mut out = Vec::new();
    for line in code.lines() {
        let t = line.trim_start();
        if t.starts_with("pub struct") {
            out.push(format!("{} {{ .. }}", t.trim_end_matches(" {")));
        } else if t.starts_with("impl ") && !t.contains(" for ") {
            out.push(String::new());
            out.push(t.into());
        } else if t.starts_with("pub fn")
            && !t.starts_with("pub fn definition")
            && !t.starts_with("pub fn instance")
        {
            out.push(format!("    {};", t.trim_end_matches(" {")));
        }
    }
    if !out.is_empty() {
        out.push("}".into());
    }
    out.join("\n")
}
