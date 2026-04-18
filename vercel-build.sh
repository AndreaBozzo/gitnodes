#!/usr/bin/env bash
set -euo pipefail

# Vercel build: install a minimal Rust toolchain + Trunk, then produce the dist/.
# Caches go under /vercel/.cache which Vercel persists between builds.

export CARGO_HOME="${CARGO_HOME:-/vercel/.cache/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-/vercel/.cache/rustup}"
export PATH="$CARGO_HOME/bin:$PATH"

if ! command -v rustc >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal
fi

rustup target add wasm32-unknown-unknown

TRUNK_VERSION="0.21.1"
if ! command -v trunk >/dev/null 2>&1; then
  mkdir -p "$CARGO_HOME/bin"
  curl -fsSL \
    "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C "$CARGO_HOME/bin"
  chmod +x "$CARGO_HOME/bin/trunk"
fi

trunk build --release
