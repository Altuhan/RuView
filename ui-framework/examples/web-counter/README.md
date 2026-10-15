# Kite in the browser

The counter example compiled to WebAssembly, drawing into a `<canvas>`.

```bash
# one-time setup
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # must match Cargo.lock

# build (from ui-framework/)
cargo build -p kite-example-web-counter --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir examples/web-counter/pkg \
    target/wasm32-unknown-unknown/release/kite_example_web_counter.wasm

# serve and open http://localhost:8000
python3 -m http.server -d examples/web-counter 8000
```

`smoke-test.mjs` checks the page in headless Chromium with Playwright: HiDPI
canvas size, pointer clicks, keyboard focus and pixel colors.

```bash
cd examples/web-counter && node smoke-test.mjs
```
