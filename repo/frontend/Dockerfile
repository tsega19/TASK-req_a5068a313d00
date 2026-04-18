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
FROM nginx:1.27-alpine
COPY --from=builder /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
