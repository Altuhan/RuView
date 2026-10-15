//! # kite-compiler
//!
//! Compiles Kite `.ui` files. Use it in three ways:
//!
//! * **Interpreter**: [`compile_source`] / [`compile_file`] → [`kite_core::ComponentDef`]
//!   → [`kite_core::ComponentInstance`] (live preview, tools, scripting).
//! * **Code generation**: [`generate_rust`] (used by `kite-build` from `build.rs`).
//! * **Checking**: inspect [`Diagnostics`] for errors and warnings.

pub mod ast;
pub mod codegen;
pub mod diagnostics;
pub mod lexer;
pub mod lower;
pub mod parser;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub use diagnostics::{Diagnostic, Diagnostics, Level, SourceMap, Span};
use kite_core::ComponentDef;

/// Result of a successful compilation.
#[derive(Debug, Clone)]
pub struct Compiled {
    /// Exported components of the main file, in source order.
    pub components: Vec<ComponentDef>,
    /// Warnings (errors make compilation fail).
    pub diagnostics: Diagnostics,
}

impl Compiled {
    /// The main component: the last exported one.
    pub fn main(&self) -> &ComponentDef {
        self.components.last().expect("compilation produces at least one component")
    }

    pub fn component(&self, name: &str) -> Option<&ComponentDef> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Instantiates the main component.
    pub fn instantiate(&self) -> kite_core::ComponentInstance {
        kite_core::ComponentInstance::from_def(self.main().clone())
    }
}

/// Loads imported files. The default reads from the filesystem.
pub type Loader<'a> = dyn FnMut(&Path) -> std::io::Result<String> + 'a;

/// Compiles source text. `path` names the file (for diagnostics and relative imports).
pub fn compile_source(path: impl AsRef<Path>, src: &str) -> Result<Compiled, Diagnostics> {
    compile_with_loader(path.as_ref(), src.to_string(), &mut |p| std::fs::read_to_string(p))
}

/// Reads and compiles a `.ui` file.
pub fn compile_file(path: impl AsRef<Path>) -> Result<Compiled, Diagnostics> {
    let path = path.as_ref();
    match std::fs::read_to_string(path) {
        Ok(src) => compile_source(path, &src),
        Err(e) => {
            let mut d = Diagnostics::default();
            d.items.push(Diagnostic {
                level: Level::Error,
                message: format!("cannot read `{}`: {e}", path.display()),
                span: Span { file: u32::MAX, start: 0, end: 0 },
                label: None,
            });
            Err(d)
        }
    }
}

/// Compiles `src` using `loader` to read imported files.
pub fn compile_with_loader(path: &Path, src: String, loader: &mut Loader) -> Result<Compiled, Diagnostics> {
    let mut sources = SourceMap::default();
    let mut items = Vec::new();
    let main_id = sources.add(path, src.clone());
    let main_ast = parser::parse_file(&src, main_id, &mut items);

    let mut registry: lower::Registry = HashMap::new();
    let mut origin: HashMap<String, Span> = HashMap::new();
    let mut register = |c: &ast::Component, items: &mut Vec<Diagnostic>| {
        if let Some(prev) = origin.get(&c.name.name) {
            if *prev != c.name.span {
                items.push(Diagnostic {
                    level: Level::Error,
                    message: format!("component `{}` is defined more than once", c.name.name),
                    span: c.name.span,
                    label: None,
                });
            }
            return;
        }
        origin.insert(c.name.name.clone(), c.name.span);
        registry.insert(c.name.name.clone(), Rc::new(c.clone()));
    };

    // Load imports breadth-first, each file once.
    let mut seen: HashSet<PathBuf> = HashSet::new();
    seen.insert(normalize(path));
    let mut queue: Vec<(PathBuf, ast::File)> = vec![(path.to_path_buf(), main_ast.clone())];
    let mut files: Vec<(PathBuf, ast::File)> = Vec::new();
    while let Some((file_path, file)) = queue.pop() {
        for imp in &file.imports {
            let dir = file_path.parent().unwrap_or(Path::new("."));
            let target = dir.join(&imp.path);
            let key = normalize(&target);
            if !seen.insert(key) {
                continue;
            }
            match loader(&target) {
                Ok(text) => {
                    let id = sources.add(&target, text.clone());
                    let parsed = parser::parse_file(&text, id, &mut items);
                    for name in &imp.names {
                        if !parsed.components.iter().any(|c| c.name.name == name.name && c.exported) {
                            items.push(Diagnostic {
                                level: Level::Error,
                                message: format!("`{}` does not export a component named `{}`", imp.path, name.name),
                                span: name.span,
                                label: None,
                            });
                        }
                    }
                    queue.push((target, parsed));
                }
                Err(e) => items.push(Diagnostic {
                    level: Level::Error,
                    message: format!("cannot import `{}`: {e}", imp.path),
                    span: imp.span,
                    label: None,
                }),
            }
        }
        files.push((file_path, file));
    }
    for (_, f) in &files {
        for c in &f.components {
            register(c, &mut items);
        }
    }

    let mut outputs: Vec<&ast::Component> = main_ast.components.iter().filter(|c| c.exported).collect();
    if outputs.is_empty() {
        if let Some(last) = main_ast.components.last() {
            items.push(Diagnostic {
                level: Level::Warning,
                message: format!("no exported component; using `{}` (add `export` to silence)", last.name.name),
                span: last.name.span,
                label: None,
            });
            outputs.push(last);
        } else if !items.iter().any(|d| d.level == Level::Error) {
            items.push(Diagnostic {
                level: Level::Error,
                message: "file contains no component".into(),
                span: Span { file: main_id, start: 0, end: 0 },
                label: None,
            });
        }
    }

    let mut components = Vec::new();
    if !items.iter().any(|d| d.level == Level::Error) {
        for c in outputs {
            let lowerer = lower::Lowerer::new(&registry, &mut items);
            components.push(lowerer.lower(&c.name.name));
        }
    }
    let diagnostics = Diagnostics { items, sources };
    if diagnostics.has_errors() {
        Err(diagnostics)
    } else {
        Ok(Compiled { components, diagnostics })
    }
}

fn normalize(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Generates Rust code for every exported component. `runtime_path` is the
/// path of the runtime crate in the generated code, e.g. `::kite` or `::kite_core`.
pub fn generate_rust(compiled: &Compiled, runtime_path: &str, source_name: &str) -> String {
    codegen::generate(&compiled.components, runtime_path, source_name)
}

#[cfg(test)]
mod tests;
