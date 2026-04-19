# Brain_UI dev workflow. Install `just` (https://github.com/casey/just) to use.

default:
    @just --list

# Install Node deps for the Tailwind pipeline (one-off).
setup:
    npm ci

# Watch-rebuild CSS into style/main.css.
css-watch:
    npm run watch:css

# One-shot minified CSS build.
css:
    npm run build:css

# Full dev loop: assumes `just css-watch` is running in another terminal.
dev:
    cargo leptos watch -p brain-app

# Release build (SSR binary + hydrate WASM).
build:
    cargo leptos build --release -p brain-app

# Checks — same set CI runs.
fmt:
    cargo fmt --all -- --check

fmt-fix:
    cargo fmt --all

lint:
    cargo clippy -p brain-app --no-default-features --features ssr -- -D warnings
    cargo clippy -p brain-app --no-default-features --features hydrate --target wasm32-unknown-unknown -- -D warnings
    cargo clippy --workspace --exclude brain-app -- -D warnings

test:
    cargo test -p brain-app --no-default-features --features ssr
    cargo test --workspace --exclude brain-app

check: fmt lint test

# Docker build (multi-stage: Node CSS → Rust → debian slim).
docker:
    docker build -t brain_ui .
