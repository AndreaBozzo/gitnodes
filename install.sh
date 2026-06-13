#!/usr/bin/env sh
# GitNodes installer for macOS and Linux.
#
#   curl -fSLo install-gitnodes.sh https://raw.githubusercontent.com/AndreaBozzo/gitnodes/master/install.sh
#   less install-gitnodes.sh
#   sh install-gitnodes.sh
#
# Downloads the prebuilt `gitnodes` binary for your platform. If the install
# directory is not already on PATH, the script prints the exact shell setup.
#
# Overridable via env:
#   GITNODES_REPO     owner/repo to download from (default: AndreaBozzo/gitnodes)
#   GITNODES_VERSION  release tag (default: latest)
#   GITNODES_BIN_DIR  install directory (default: $HOME/.local/bin)
set -eu

REPO="${GITNODES_REPO:-AndreaBozzo/gitnodes}"
VERSION="${GITNODES_VERSION:-latest}"
BIN_DIR="${GITNODES_BIN_DIR:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }

# --- detect a platform for which the release workflow publishes an asset -----
os="$(uname -s)"
arch="$(uname -m)"
case "${os}:${arch}" in
  Linux:x86_64|Linux:amd64) target="x86_64-unknown-linux-gnu" ;;
  Darwin:x86_64|Darwin:amd64) target="x86_64-apple-darwin" ;;
  Darwin:arm64|Darwin:aarch64) target="aarch64-apple-darwin" ;;
  Linux:arm64|Linux:aarch64)
    err "Linux ARM64 binaries are not published yet; see the Releases page"
    ;;
  *) err "unsupported platform '${os}/${arch}' — see the Releases page" ;;
esac
asset="gitnodes-${target}.tar.gz"

# --- resolve download URL ----------------------------------------------------
if [ "$VERSION" = "latest" ]; then
  url="https://github.com/${REPO}/releases/latest/download/${asset}"
else
  url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
fi

command -v curl >/dev/null 2>&1 || err "curl is required"
command -v tar  >/dev/null 2>&1 || err "tar is required"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

printf 'Downloading %s ...\n' "$asset"
curl -fSL --proto '=https' --tlsv1.2 "$url" -o "$tmp/$asset" \
  || err "download failed: $url"

tar -xzf "$tmp/$asset" -C "$tmp" || err "extract failed"
[ -f "$tmp/gitnodes" ] || err "archive did not contain a 'gitnodes' binary"

mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/gitnodes" "$BIN_DIR/gitnodes" 2>/dev/null \
  || { cp "$tmp/gitnodes" "$BIN_DIR/gitnodes" && chmod 0755 "$BIN_DIR/gitnodes"; }

printf '\nInstalled gitnodes to %s\n' "$BIN_DIR/gitnodes"

# --- PATH advice -------------------------------------------------------------
case ":${PATH}:" in
  *":${BIN_DIR}:"*)
    printf 'Run:  gitnodes init my-brain\n'
    ;;
  *)
    printf '\n%s is not on your PATH yet. Add it, e.g.:\n' "$BIN_DIR"
    printf '  echo '\''export PATH="%s:$PATH"'\'' >> ~/.profile && . ~/.profile\n' "$BIN_DIR"
    printf 'Then run:  gitnodes init my-brain\n'
    ;;
esac
