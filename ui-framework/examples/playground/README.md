# Kite Playground

A browser page with a `.ui` editor and a live preview. The Kite compiler and
runtime are both compiled to WebAssembly, so every edit recompiles in the
page. It also shows the public properties (editable, like Rust's `set_*`),
a log of callbacks handled by Rust, and the typed Rust API that `kite-build`
would generate.

```bash
# from ui-framework/
cargo build -p kite-example-playground --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir examples/playground/pkg \
    target/wasm32-unknown-unknown/release/kite_example_playground.wasm
python3 -m http.server -d examples/playground 8000   # open http://localhost:8000
```
