//! # kite-build
//!
//! Compiles `.ui` files to Rust at build time. In `build.rs`:
//!
//! ```no_run
//! fn main() {
//!     kite_build::compile("ui/app.ui");
//! }
//! ```
//!
//! and in your crate: `kite::include_ui!("app");` (the file stem).
//! Diagnostics are reported as build errors with source snippets.

use std::path::{Path, PathBuf};

/// Code generation options.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path of the runtime crate in generated code: `::kite` (default) or
    /// `::kite_core` for `no_std` crates that depend on the core directly.
    pub runtime_path: String,
    /// Output directory (defaults to `$OUT_DIR`).
    pub out_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self { runtime_path: "::kite".into(), out_dir: None }
    }
}

impl Config {
    pub fn runtime_path(mut self, path: &str) -> Self {
        self.runtime_path = path.into();
        self
    }
}

/// Error from [`try_compile`]: rendered diagnostics or an I/O problem.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Compiles `path` with default settings; on error prints diagnostics and fails the build.
pub fn compile(path: impl AsRef<Path>) {
    compile_with_config(path, Config::default())
}

pub fn compile_with_config(path: impl AsRef<Path>, config: Config) {
    if let Err(e) = try_compile(path.as_ref(), &config) {
        eprintln!("{e}");
        // A panic message is shown by cargo; keep it short since details are above.
        panic!("kite-build: failed to compile `{}` (see errors above)", path.as_ref().display());
    }
}

/// Compiles `path` and writes `<out_dir>/<stem>.rs`. Returns the output path.
pub fn try_compile(path: &Path, config: &Config) -> Result<PathBuf, Error> {
    println!("cargo:rerun-if-changed={}", path.display());
    let compiled = kite_compiler::compile_file(path).map_err(|d| {
        for (file, _) in &d.sources.files {
            println!("cargo:rerun-if-changed={}", file.display());
        }
        Error(d.to_string())
    })?;
    for (file, _) in &compiled.diagnostics.sources.files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    for w in compiled.diagnostics.warnings() {
        for line in compiled.diagnostics.render(w).lines() {
            println!("cargo:warning={line}");
        }
    }
    let out_dir = match &config.out_dir {
        Some(d) => d.clone(),
        None => PathBuf::from(std::env::var("OUT_DIR").map_err(|_| Error("OUT_DIR is not set; call from build.rs or set Config::out_dir".into()))?),
    };
    let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| Error(format!("bad file name `{}`", path.display())))?;
    let code = kite_compiler::generate_rust(&compiled, &config.runtime_path, &path.display().to_string());
    let out = out_dir.join(format!("{stem}.rs"));
    std::fs::write(&out, code).map_err(|e| Error(format!("cannot write `{}`: {e}", out.display())))?;
    Ok(out)
}
