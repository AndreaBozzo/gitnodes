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
RUN cargo install cargo-leptos --locked

WORKDIR /app
COPY . .
# Overwrite the source main.css with the compiled Tailwind output.
COPY --from=css-builder /app/style/main.css ./style/main.css

RUN cargo leptos build --release

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/brain_ui .
COPY --from=builder /app/target/site ./target/site

# Railway provides PORT at runtime; default to 3000 for local docker builds.
ENV LEPTOS_OUTPUT_NAME="brain_ui"
ENV LEPTOS_SITE_ROOT="target/site"
ENV LEPTOS_SITE_PKG_DIR="pkg"
# Cookie must be Secure on Railway (HTTPS).
ENV SESSION_COOKIE_SECURE="1"
EXPOSE 3000

# Use a shell entrypoint so $LEPTOS_SITE_ADDR is expanded at runtime.
CMD sh -c "./brain_ui"
