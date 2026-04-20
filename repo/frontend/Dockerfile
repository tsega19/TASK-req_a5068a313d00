# -----------------------------------------------------------------------------
# Stage 1: compile Yew → WASM with trunk
# -----------------------------------------------------------------------------
FROM rust:1.88-bookworm AS builder
WORKDIR /app

RUN rustup target add wasm32-unknown-unknown
# Install Trunk from the pre-built release tarball. Compiling Trunk from
# source pulls in minify-html-common, which requires more memory than CI
# provides and gets OOM-killed (SIGKILL) mid-build.
RUN set -eux; \
    curl -fsSL --retry 5 --retry-delay 5 --retry-all-errors \
      https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-gnu.tar.gz \
    | tar -xzC /usr/local/bin; \
    chmod +x /usr/local/bin/trunk; \
    trunk --version
# Pre-install the wasm-bindgen CLI that matches the version our Cargo.lock
# resolves to. Keeping this in sync with the library version is required —
# Trunk would otherwise try to download the musl binary from GitHub Releases,
# which has been observed to 504 intermittently.
RUN cargo install --locked --version 0.2.100 wasm-bindgen-cli

# Dependency pre-compilation layer
COPY Cargo.toml ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
 && cargo build --release --target wasm32-unknown-unknown \
 && rm -rf src target/wasm32-unknown-unknown/release/.fingerprint/fieldops_frontend-*

COPY Trunk.toml index.html ./
COPY src ./src
COPY styles ./styles
RUN trunk build --release

# -----------------------------------------------------------------------------
# Stage 2: nginx serving static dist + proxying /api to backend container
# -----------------------------------------------------------------------------
FROM nginx:1.27-alpine AS runtime
COPY --from=builder /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80

# -----------------------------------------------------------------------------
# Stage 3 (optional): Dockerized wasm-bindgen-test runner. Used by the
# `frontend-test` service under the `tests` profile in docker-compose.
# This keeps the frontend unit-test pipeline fully inside Docker — no host
# toolchain required.
# -----------------------------------------------------------------------------
FROM builder AS tester
# `wasm_bindgen_test_configure!(run_in_browser)` (used by the localStorage-
# backed tests in offline.rs) requires a real browser. Install Chromium +
# its driver so the test runner can drive it headless; fonts + minimal deps
# are what puppeteer-style tooling typically installs.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        chromium chromium-driver \
        ca-certificates fonts-liberation \
 && rm -rf /var/lib/apt/lists/*
# wasm-bindgen-test-runner ships with the wasm-bindgen-cli we pre-installed
# at the pinned 0.2.100 version — no extra install needed.
ENV CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
    WASM_BINDGEN_TEST_TIMEOUT=60 \
    CHROMEDRIVER=/usr/bin/chromedriver
# NOTE: intentionally NOT setting NO_HEADLESS. wasm-bindgen-test-runner
# treats *any* value of NO_HEADLESS (including "0" or empty) as "go
# interactive", which hangs CI waiting for a human to hit Ctrl-C.
# Default (env var unset) runs Chromium headless. See:
# https://github.com/rustwasm/wasm-bindgen/issues/2540
# Run the crate's #[wasm_bindgen_test] cases through the configured runner.
# The frontend is a bin-only crate (no [lib]) so we target the bin test
# harness that contains every `#[cfg(test)] mod tests` in the source tree.
CMD ["cargo", "test", "--release", "--target", "wasm32-unknown-unknown", "--bins", "--", "--nocapture"]
