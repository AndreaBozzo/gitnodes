# ---- CSS build stage (Tailwind + typography plugin) ----
FROM node:20-alpine AS css-builder
WORKDIR /app
COPY package.json ./
RUN npm install --no-audit --no-fund
COPY tailwind.config.js ./
COPY style ./style
COPY src ./src
RUN npx tailwindcss -i style/tailwind.css -o style/main.css --minify

# ---- Rust build stage ----
FROM rust:1.95-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev curl perl make && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown

# Cache cargo-leptos install across builds
RUN --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/usr/local/cargo/registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/usr/local/cargo/git,target=/usr/local/cargo/git \
    cargo install cargo-leptos --locked

WORKDIR /app

# Copy only dependency manifests first to cache dep compilation
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY build.rs ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs && echo '' > src/lib.rs
RUN --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/usr/local/cargo/registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/usr/local/cargo/git,target=/usr/local/cargo/git \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/app/target,target=/app/target \
    cargo build --release --features ssr 2>/dev/null || true

# Now copy real sources
COPY . .
COPY --from=css-builder /app/style/main.css ./style/main.css

RUN --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/usr/local/cargo/registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/usr/local/cargo/git,target=/usr/local/cargo/git \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-/app/target,target=/app/target \
    cargo leptos build --release && \
    cp target/release/brain_ui /app/brain_ui_bin && \
    cp -r target/site /app/site_out

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/brain_ui_bin ./brain_ui
COPY --from=builder /app/site_out ./target/site

# Railway provides PORT at runtime; default to 3000 for local docker builds.
ENV LEPTOS_OUTPUT_NAME="brain_ui"
ENV LEPTOS_SITE_ROOT="target/site"
ENV LEPTOS_SITE_PKG_DIR="pkg"
# Cookie must be Secure on Railway (HTTPS).
ENV SESSION_COOKIE_SECURE="1"
EXPOSE 3000

CMD sh -c "./brain_ui"
