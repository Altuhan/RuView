# Kite

Kite is a declarative UI framework in the spirit of [Slint](https://slint.dev):
you describe the UI in `.ui` files using a small, typed markup language, and
Kite compiles them to Rust at build time or loads them at runtime. The same UI
runs in a desktop window, in the browser (WebAssembly), and on
microcontrollers (`no_std`).

```
export component Counter inherits Window {
    in-out property <int> counter;
    callback request-increase;

    VerticalLayout {
        padding: 20px;
        spacing: 12px;
        Text { text: "Clicked \{counter} times"; font-size: 24px; }
        Button {
            text: "+1";
            primary: true;
            clicked => { root.request-increase(); }
        }
    }
}
```

```rust
kite::include_ui!("counter");
use kite::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Counter::new();
    app.on_request_increase(|app| app.set_counter(app.get_counter() + 1));
    app.run()
}
```

## Quick start

```bash
cd ui-framework
cargo install --path crates/kite-cli     # installs the `kite` tool

kite new my-app                          # scaffold a ready-to-run project
cd my-app
cargo run                                # build & open the window
kite preview ui/app.ui                   # live preview, reloads on save
```

Other `kite` commands:

| Command | What it does |
|---|---|
| `kite check ui/app.ui` | Type-check files and print diagnostics |
| `kite render ui/app.ui -o shot.png --scale 2 --set name=Ada` | Headless screenshot |
| `kite codegen ui/app.ui` | Print the generated Rust |
| `kite elements` | Print the built-in element reference |

## What's in the box

- **A real language**: components, typed properties with `in`/`out`/`in-out`
  visibility, reactive bindings, two-way bindings (`<=>`), callbacks and
  handlers, string interpolation (`"\{expr}"`), units (`px`, `%`), imports,
  inheritance, `@children` slots, and error messages that point at the source
  with "did you mean" hints.
- **Reactivity**: bindings are lazy and re-evaluate only when a dependency
  changes. Assigning a property replaces its binding, as in Slint.
- **Layouts**: `HorizontalLayout` and `VerticalLayout` with
  min/preferred/max sizes, stretch factors, padding, spacing and alignment.
  Absolute positioning is available everywhere else.
- **Widgets**: `Text`, `Rectangle`, `TouchArea`, `Button`, `CheckBox`,
  `Slider`, `TextInput` (caret, password mode, keyboard editing), with focus
  and Tab navigation.
- **One renderer everywhere**: an anti-aliased software rasterizer with a
  built-in font. It is `no_std` and needs no GPU, so desktop, web and MCU
  output match pixel for pixel. HiDPI is supported, and band rendering lets
  small devices draw a few rows at a time.
- **Two ways to run a UI**: `kite-build` generates typed Rust APIs
  (`get_x`/`set_x`/`on_x`/`invoke_x`), and `kite-compiler` interprets `.ui`
  files at runtime for tools and live preview.

## Crates

| Crate | Role | `no_std` |
|---|---|---|
| `kite-core` | Runtime: IR, reactive properties, layout, input, software renderer | yes |
| `kite-compiler` | `.ui` lexer, parser, type checker, IR lowering, Rust codegen | — |
| `kite-build` | Use from `build.rs` to compile `.ui` files | — |
| `kite` | Facade: `include_ui!`, `RunExt::run`, optional interpreter/headless | yes (no default features) |
| `kite-winit` | Desktop backend (winit + softbuffer; X11, Wayland, Windows, macOS) | — |
| `kite-web` | Browser backend (WASM, `<canvas>`) | — |
| `kite-embedded` | MCU backend (`embedded-graphics` `DrawTarget`, touch input) | yes |
| `kite-headless` | PNG rendering and input simulation for tests | — |
| `kite-cli` | The `kite` command | — |

## Examples

| Example | Shows | Run |
|---|---|---|
| `examples/playground` | Live `.ui` editor in the browser (compiler runs as WASM) | see [its README](examples/playground/README.md) |
| `examples/ui/gallery.ui` | All widgets, components, `@children`, two-way bindings | `kite preview examples/ui/gallery.ui` |
| `examples/counter` | Build-time codegen, Rust callbacks, headless tests | `cargo run -p kite-example-counter` |
| `examples/web-counter` | The counter in the browser | see [its README](examples/web-counter/README.md) |
| `examples/embedded-thermostat` | `no_std` UI for a 320×240 RGB565 display | `cargo test -p kite-example-thermostat` |

## Status and limits

This is an MVP. What has been verified, and how:

- Unit and integration tests cover the compiler, reactivity, layout, input,
  rendering, codegen and the embedded band renderer (`cargo test`).
- The `no_std` crates cross-compile for `riscv32imac-unknown-none-elf` and
  `thumbv7em-none-eabihf`. **Not yet run on real hardware**: no device
  boot or runtime log has been captured.
- The web backend builds for `wasm32-unknown-unknown`. A Playwright smoke
  test drives it in headless Chromium (see the web example's README).
- The desktop backend compiles, but no automated test opens a real window,
  because CI has no display.

No performance numbers are claimed. Rendering is a full CPU repaint whenever
anything changes.

Not implemented yet (roadmap): `for`/`if` repeaters and models, animations and
states, images, `GridLayout`, scrolling (`Flickable`/`ListView`), clipboard and
IME composition, accessibility, dirty-region repainting, GPU renderers, native
code generation of bindings (the generated code currently builds an IR that a
small interpreter evaluates), and a language server.

See [docs/LANGUAGE.md](docs/LANGUAGE.md) for the language reference and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it works.

## License

Code: MIT OR Apache-2.0. The built-in font atlas is rasterized from DejaVu Sans
(Bitstream Vera license, see `crates/kite-core/src/font/LICENSE-FONT`).
