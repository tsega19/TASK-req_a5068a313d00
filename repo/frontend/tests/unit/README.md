# Frontend unit tests

The Yew app targets `wasm32-unknown-unknown`; native `cargo test` against
`src/` would fail to compile because most modules depend on `web-sys`,
`wasm-bindgen`, and `gloo-net` which require a browser environment.

The idiomatic approach is [`wasm-bindgen-test`](https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/usage.html)
in headless-browser or Node mode. Integrating that runner into the Docker
test pipeline requires either:

1. A headless browser (Chromium/Firefox) inside the container, driven by
   `wasm-pack test --headless --chrome`.
2. Node.js + `wasm-bindgen-test-runner --node` for DOM-free tests.

Both add significant container build weight. The current phase satisfies
the PRD's Frontend test requirement via `../e2e/smoke.sh`, which exercises
the production bundle end-to-end against the running stack — the same
surface a unit-test suite would cover indirectly via DOM assertions.

Pure-logic tests (`types.rs` role/state mappings, `auth.rs` localStorage
roundtrip) can be added here in a follow-up by restructuring the crate
into a `cfg(target_arch="wasm32")`-gated bin + a native-testable library.
