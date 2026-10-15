//! `kite` — the Kite command-line tool.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
kite — tools for Kite .ui files

USAGE:
    kite check <file.ui>...
    kite render <file.ui> -o <out.png> [--size WxH] [--scale N] [--set name=value]...
    kite codegen <file.ui> [-o <out.rs>] [--runtime <path>]
    kite preview <file.ui>          (live window; reloads on save)

`render` uses the component's preferred size unless --size is given.
`--set` assigns public properties before rendering, e.g. --set name=Ada.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        Some("render") => render(&args[1..]),
        Some("codegen") => codegen(&args[1..]),
        Some("preview") => preview(&args[1..]),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn compile(path: &Path) -> Result<kite_compiler::Compiled, String> {
    match kite_compiler::compile_file(path) {
        Ok(c) => {
            for w in c.diagnostics.warnings() {
                eprint!("{}", c.diagnostics.render(w));
            }
            Ok(c)
        }
        Err(d) => Err(format!("{d}error: could not compile `{}`", path.display())),
    }
}

fn check(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: kite check <file.ui>...".into());
    }
    let mut failed = false;
    for f in args {
        match compile(Path::new(f)) {
            Ok(c) => {
                let names: Vec<&str> = c.components.iter().map(|c| c.name.as_str()).collect();
                println!("ok: {f} ({})", names.join(", "));
            }
            Err(e) => {
                eprintln!("{e}");
                failed = true;
            }
        }
    }
    if failed { Err("check failed".into()) } else { Ok(()) }
}

/// Parses `value` for a property of type `ty`.
fn parse_value(ty: kite_core::Type, value: &str) -> Result<kite_core::Value, String> {
    use kite_core::{Type, Value};
    Ok(match ty {
        Type::Bool => Value::Bool(value.parse().map_err(|_| format!("`{value}` is not a bool"))?),
        Type::Int => Value::Int(value.parse().map_err(|_| format!("`{value}` is not an int"))?),
        Type::Float => Value::Float(value.parse().map_err(|_| format!("`{value}` is not a number"))?),
        Type::String => Value::String(value.into()),
        Type::Color => Value::Color(kite_core::Color::from_hex(value).ok_or(format!("`{value}` is not a color"))?),
        Type::Enum(e) => Value::Enum(e, e.variant_index(value).ok_or(format!("`{value}` is not a {}", e.name()))?),
        Type::Void => Value::Void,
    })
}

fn render(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut out = None;
    let mut size = None;
    let mut scale = 1.0f32;
    let mut sets = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => out = it.next().cloned(),
            "--size" => {
                let s = it.next().ok_or("--size needs WxH")?;
                let (w, h) = s.split_once('x').ok_or("--size needs WxH")?;
                size = Some((w.parse::<f32>().map_err(|e| e.to_string())?, h.parse::<f32>().map_err(|e| e.to_string())?));
            }
            "--scale" => scale = it.next().ok_or("--scale needs a number")?.parse().map_err(|_| "bad --scale")?,
            "--set" => sets.push(it.next().ok_or("--set needs name=value")?.clone()),
            _ => input = Some(a.clone()),
        }
    }
    let input = input.ok_or("usage: kite render <file.ui> -o <out.png>")?;
    let out = out.unwrap_or_else(|| Path::new(&input).with_extension("png").to_string_lossy().into_owned());
    let compiled = compile(Path::new(&input))?;
    let app = compiled.instantiate();
    for s in sets {
        let (name, value) = s.split_once('=').ok_or("--set needs name=value")?;
        let def = app.definition();
        let p = def.public_property(name).ok_or(format!("no public property `{name}`"))?;
        app.set_property(name, parse_value(p.ty, value)?).map_err(|e| e.to_string())?;
    }
    if let Some((w, h)) = size {
        app.set_size(kite_core::Size::new(w, h));
    }
    let img = kite_headless::render(&app, scale);
    img.save_png(&out).map_err(|e| format!("cannot write `{out}`: {e}"))?;
    println!("wrote {out} ({}x{})", img.width(), img.height());
    Ok(())
}

fn codegen(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut out: Option<PathBuf> = None;
    let mut runtime = "::kite".to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = it.next().map(PathBuf::from),
            "--runtime" => runtime = it.next().ok_or("--runtime needs a path")?.clone(),
            _ => input = Some(a.clone()),
        }
    }
    let input = input.ok_or("usage: kite codegen <file.ui>")?;
    let compiled = compile(Path::new(&input))?;
    let code = kite_compiler::generate_rust(&compiled, &runtime, &input);
    match out {
        Some(p) => std::fs::write(&p, code).map_err(|e| e.to_string()),
        None => {
            print!("{code}");
            Ok(())
        }
    }
}

#[cfg(feature = "preview")]
fn preview(args: &[String]) -> Result<(), String> {
    let input = PathBuf::from(args.first().ok_or("usage: kite preview <file.ui>")?);
    let path = input.clone();
    let load = move || -> Result<kite_core::ComponentInstance, String> { compile(&path).map(|c| c.instantiate()) };
    kite_winit::run_live(input, Box::new(load)).map_err(|e| e.to_string())
}

#[cfg(not(feature = "preview"))]
fn preview(_: &[String]) -> Result<(), String> {
    Err("this build of `kite` has no preview support (enable the `preview` feature)".into())
}
