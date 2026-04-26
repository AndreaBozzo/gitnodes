# ---- CSS build stage (Tailwind + typography plugin) ----
FROM node:20-alpine AS css-builder
WORKDIR /app
COPY package.json ./
RUN npm install --no-audit --no-fund
COPY tailwind.config.js ./
COPY crates/brain-app/style ./crates/brain-app/style
COPY crates/brain-app/src ./crates/brain-app/src
RUN npx tailwindcss -i crates/brain-app/style/tailwind.css -o crates/brain-app/style/main.css --minify

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
    install -m 0755 /tmp/cargo-leptos /usr/local/cargo/bin/cargo-leptos; \
    rm -rf /tmp/cargo-leptos /tmp/cargo-leptos.tar.gz

WORKDIR /app

# Copy only dependency manifests first to cache dep compilation
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/brain-app/Cargo.toml crates/brain-app/
COPY crates/brain-auth/Cargo.toml crates/brain-auth/
COPY crates/brain-domain/Cargo.toml crates/brain-domain/
COPY crates/brain-graph/Cargo.toml crates/brain-graph/
COPY crates/brain-storage/Cargo.toml crates/brain-storage/

RUN mkdir -p crates/brain-app/src && echo 'fn main(){}' > crates/brain-app/src/main.rs && echo '' > crates/brain-app/src/lib.rs
RUN mkdir -p crates/brain-auth/src && echo '' > crates/brain-auth/src/lib.rs
RUN mkdir -p crates/brain-domain/src && echo '' > crates/brain-domain/src/lib.rs
RUN mkdir -p crates/brain-graph/src && echo '' > crates/brain-graph/src/lib.rs
RUN mkdir -p crates/brain-storage/src && echo '' > crates/brain-storage/src/lib.rs

RUN --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-v2-/usr/local/cargo/registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-v2-/usr/local/cargo/git,target=/usr/local/cargo/git \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-v2-/app/target,target=/app/target \
    cargo build --release -p brain-app --features ssr 2>/dev/null || true

# Now copy real sources
COPY . .
COPY --from=css-builder /app/crates/brain-app/style/main.css ./crates/brain-app/style/main.css

RUN find crates -type f -name "*.rs" -exec touch {} +

RUN --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-v2-/usr/local/cargo/registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-v2-/usr/local/cargo/git,target=/usr/local/cargo/git \
    --mount=type=cache,id=s/1f4c0640-e2bb-448a-8b76-62e3566c4420-v2-/app/target,target=/app/target \
    LEPTOS_SITE_ROOT=/app/.docker-site LEPTOS_SITE_PKG_DIR=pkg cargo leptos build --release -p brain-app && \
    cp target/release/brain-app /app/brain_ui_bin && \
    cp -r /app/.docker-site /app/site_out

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/brain_ui_bin ./brain-app
COPY --from=builder /app/site_out ./target/site

# Railway provides PORT at runtime; default to 3000 for local docker builds.
ENV LEPTOS_OUTPUT_NAME="brain_ui"
ENV LEPTOS_SITE_ROOT="target/site"
ENV LEPTOS_SITE_PKG_DIR="pkg"
# Cookie must be Secure on Railway (HTTPS).
ENV SESSION_COOKIE_SECURE="1"
EXPOSE 3000

CMD sh -c "./brain-app"
