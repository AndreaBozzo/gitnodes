# GitNodes dev workflow. Install `just` (https://github.com/casey/just) to use.

# Use native PowerShell when `just` runs on Windows; Unix keeps its default shell.
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

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
    cargo leptos watch -p gitnodes-app

# Release build (SSR binary + hydrate WASM).
build:
    cargo leptos build --release -p gitnodes-app

# Checks — same set CI runs.
fmt:
    cargo fmt --all -- --check

fmt-fix:
    cargo fmt --all

lint:
    cargo clippy -p gitnodes-app --no-default-features --features ssr -- -D warnings
    cargo clippy -p gitnodes-app --no-default-features --features hydrate --target wasm32-unknown-unknown -- -D warnings
    cargo clippy --workspace --exclude gitnodes-app -- -D warnings

test:
    cargo test -p gitnodes-app --no-default-features --features ssr
    cargo test --workspace --exclude gitnodes-app

check: fmt lint test

# Supply-chain policy: accepted advisories, licenses, and registry sources.
deny:
    cargo deny check advisories licenses sources

# CI-equivalent release artifact rehearsal (requires target/site from `just build`).
embed-check:
    cargo clippy -p gitnodes-app --bin gitnodes-app --no-default-features --features embed-assets -- -D warnings
    cargo build --release -p gitnodes-app --bin gitnodes-app --no-default-features --features embed-assets
    target/release/gitnodes-app --version
    target/release/gitnodes-app --help

# Docker build (multi-stage: Node CSS → Rust → debian slim).
docker:
    docker build -t gitnodes .
