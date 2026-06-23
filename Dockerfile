# ---- CSS build stage (Tailwind + typography plugin) ----
FROM node:20-alpine AS css-builder
WORKDIR /app
COPY package.json ./
RUN npm install --no-audit --no-fund
COPY tailwind.config.js ./
COPY crates/gitnodes-app/style ./crates/gitnodes-app/style
COPY crates/gitnodes-app/src ./crates/gitnodes-app/src
RUN npx tailwindcss -i crates/gitnodes-app/style/tailwind.css -o crates/gitnodes-app/style/main.css --minify

# ---- Rust build stage ----
FROM rust:1.95-slim AS builder

ARG CARGO_LEPTOS_VERSION=0.3.6
ARG TARGETARCH

RUN apt-get update && apt-get install -y pkg-config libssl-dev curl perl make && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown

# Download a pinned prebuilt cargo-leptos binary. Compiling it from source adds
# several minutes to cold builds and can starve the actual project build when
# the Docker layer cache is unavailable.
RUN set -eux; \
    case "${TARGETARCH:-amd64}" in \
        amd64) leptos_target='x86_64-unknown-linux-gnu' ;; \
        arm64) leptos_target='aarch64-unknown-linux-gnu' ;; \
        *) echo "Unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl --proto '=https' --tlsv1.2 -fsSL \
        -o /tmp/cargo-leptos.tar.gz \
        "https://github.com/leptos-rs/cargo-leptos/releases/download/v${CARGO_LEPTOS_VERSION}/cargo-leptos-${leptos_target}.tar.gz"; \
    tar -xzf /tmp/cargo-leptos.tar.gz -C /tmp; \
    install -m 0755 "/tmp/cargo-leptos-${leptos_target}/cargo-leptos" /usr/local/cargo/bin/cargo-leptos; \
    rm -rf "/tmp/cargo-leptos-${leptos_target}" /tmp/cargo-leptos.tar.gz

WORKDIR /app

# Copy only dependency manifests first to cache dep compilation
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/gitnodes-app/Cargo.toml crates/gitnodes-app/
COPY crates/gitnodes-auth/Cargo.toml crates/gitnodes-auth/
COPY crates/gitnodes-domain/Cargo.toml crates/gitnodes-domain/
COPY crates/gitnodes-graph/Cargo.toml crates/gitnodes-graph/
COPY crates/gitnodes-storage/Cargo.toml crates/gitnodes-storage/

RUN mkdir -p crates/gitnodes-app/src && echo 'fn main(){}' > crates/gitnodes-app/src/main.rs && echo '' > crates/gitnodes-app/src/lib.rs
RUN mkdir -p crates/gitnodes-auth/src && echo '' > crates/gitnodes-auth/src/lib.rs
RUN mkdir -p crates/gitnodes-domain/src && echo '' > crates/gitnodes-domain/src/lib.rs
RUN mkdir -p crates/gitnodes-graph/src && echo '' > crates/gitnodes-graph/src/lib.rs
RUN mkdir -p crates/gitnodes-storage/src && echo '' > crates/gitnodes-storage/src/lib.rs

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p gitnodes-app --features ssr 2>/dev/null || true

# Now copy real sources
COPY . .
COPY --from=css-builder /app/crates/gitnodes-app/style/main.css ./crates/gitnodes-app/style/main.css

RUN find crates -type f -name "*.rs" -exec touch {} +

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    LEPTOS_SITE_ROOT=/app/.docker-site LEPTOS_SITE_PKG_DIR=pkg cargo leptos build --release -p gitnodes-app && \
    cp target/release/gitnodes-app /app/gitnodes_bin && \
    cp -r /app/.docker-site /app/site_out

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/gitnodes_bin ./gitnodes-app
COPY --from=builder /app/site_out ./target/site

# Bundled demo brain for the public read-only demo (see the demo mode below).
# It is inert in a normal deployment — nothing reads it unless GITNODES_PREVIEW_DIR
# is set, so the production image simply carries a few KB it never touches.
COPY examples/demo-brain ./demo-brain

# Railway provides PORT at runtime; default to 3000 for local docker builds.
ENV LEPTOS_OUTPUT_NAME="gitnodes"
ENV LEPTOS_SITE_ROOT="target/site"
ENV LEPTOS_SITE_PKG_DIR="pkg"
# Cookie must be Secure on Railway (HTTPS).
ENV SESSION_COOKIE_SECURE="1"
EXPOSE 3000

# Default: run the full GitHub-backed server (`serve`).
# Read-only demo: set GITNODES_PREVIEW_DIR=/app/demo-brain and
# GITNODES_ALLOW_REMOTE_PREVIEW=1 to serve a brain read-only with no auth,
# no GitHub credential, and an in-memory database. See docs/guides/DEMO_DEPLOY.md.
CMD sh -c 'if [ -n "$GITNODES_PREVIEW_DIR" ]; then exec ./gitnodes-app preview "$GITNODES_PREVIEW_DIR"; else exec ./gitnodes-app; fi'
