# Frontend unit tests

Executable unit tests live **inside the crate** as `#[cfg(test)]` modules that
use [`wasm-bindgen-test`](https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/usage.html).
They run in a real browser (Chrome/Firefox) via `wasm-pack`.

## Run

From the repo root:

```bash
wasm-pack test --headless --chrome frontend
```

Or, without a browser, against Node:

```bash
wasm-pack test --node frontend
```

## Where the tests live

- `frontend/src/types.rs` — role/state/status label + serde wire-tag guards.
- `frontend/src/offline.rs` — mutation-queue enqueue/read, dead-letter flagging,
  URI-encoding helpers used by the offline sync cursor.

Add new tests alongside the module they cover so they run with the rest of the
suite. For DOM-dependent tests keep `wasm_bindgen_test_configure!(run_in_browser)`
at the top of the test module; for pure logic `--node` also works.
