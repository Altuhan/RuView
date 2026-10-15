# Kite architecture

```
 .ui source ──► kite-compiler ─────────────────────────────┐
               lexer → parser → AST                        │
               lower: expand components, resolve names,    │
                      type-check → ComponentDef (IR)        │
                 │                          │               │
                 │ codegen (kite-build)     │ interpreter   │
                 ▼                          ▼               │
          Rust: typed struct +        ComponentInstance ◄───┘
          IR built by constructors          │
                 │                          │
                 └────────► kite-core ◄─────┘   (no_std + alloc)
                            reactive store · layout · input · paint
                                            │ raster::Target
              ┌──────────────┬──────────────┼───────────────┐
          kite-winit      kite-web     kite-embedded     kite-headless
          softbuffer      <canvas>     DrawTarget bands  PNG / tests
```

## IR

`ComponentDef` is a flat list of `ElementDef`s (element 0 is the root). Each
element has its built-in properties at fixed indices (the `COMMON` geometry
properties, then kind-specific ones), followed by declared properties.
Expressions refer to properties as `(element, index)` pairs, so the
runtime never looks names up in its hot paths. Instantiating a sub-component
copies its elements into the list. The compiler tracks which component scope
each element came from, to enforce `private`/`out` visibility and to resolve
ids.

## Reactive store

Each property is a slot holding a value, an optional binding, a dirty flag
and a dependents list:

- **Read**: if the slot is dirty, re-evaluate its binding with the slot on an
  evaluation stack. Every slot read during evaluation records the evaluating
  slot as a dependent.
- **Write**: drop the binding, store the value, and mark the dependents
  dirty, transitively. Dependent lists are cleared as they are walked and
  rebuilt on the next evaluation, so stale edges disappear.
- Two-way bindings are slot aliases, resolved once when the instance is created.
- A cycle (a binding that reads itself) returns the last value instead of recursing.

The instance state lives in one `RefCell`. Every borrow is released before
user callbacks run, so Rust handlers can freely read and write properties.
Input handling queues callback invocations and runs them after the borrow ends.

## Layout

Layout is a top-down pass. Constraints (min, preferred, max, stretch) are
computed recursively from widget content (text metrics) and explicit
properties. Computed geometry is written back into `x`/`y`/`width`/`height`
without marking them explicit, so bindings can depend on layout. Layout
repeats while values change (at most 4 passes).

## Rendering

`raster::Canvas` draws anti-aliased rounded rectangles and strokes (signed
distance), lines (capsule distance) and text, onto any `raster::Target`:

- `RgbBuffer` / `Rgb32Buffer` — `0x00RRGGBB` (desktop, web, PNG)
- `Rgb565Buffer` — MCU framebuffers
- any `embedded-graphics` `DrawTarget`, through `kite-embedded`

The canvas supports a scale factor (HiDPI) and a vertical offset, so a frame
can be produced as horizontal bands into a small buffer. A test checks that
banded output is pixel-identical to full-frame output.

Text uses a 107-glyph 8-bit alpha atlas (ASCII plus a few symbols),
rasterized at 32 px from DejaVu Sans by `tools/gen_font.py`. Smaller sizes
are supersampled. Larger sizes are bilinearly upsampled and re-thresholded
to keep edges sharp.

## Code generation

The generated Rust contains a struct wrapping `ComponentInstance`, typed
accessors, and `definition()`, which builds the IR with plain constructors.
It doesn't parse anything at runtime and only needs `alloc`. Only properties
that differ from their defaults are emitted. The runtime crate path is
configurable (`::kite` or `::kite_core`), so `no_std` crates can depend on
`kite-core` alone.
