#!/usr/bin/env sh
set -eu

if [ "$#" -lt 5 ] || [ "$#" -gt 6 ]; then
  echo "usage: $0 VERSION LINUX_X64_SHA MACOS_X64_SHA MACOS_ARM64_SHA WINDOWS_X64_SHA [OUT_DIR]" >&2
  exit 2
fi

VERSION="${1#v}"
SHA_LINUX_X64="$2"
SHA_MACOS_X64="$3"
SHA_MACOS_ARM64="$4"
SHA_WINDOWS_X64="$5"
OUT_DIR="${6:-dist/package-manifests}"

for value in "$SHA_LINUX_X64" "$SHA_MACOS_X64" "$SHA_MACOS_ARM64" "$SHA_WINDOWS_X64"; do
  case "$value" in
    *[!0-9a-fA-F]*|"")
      echo "error: every checksum must be a 64-character hexadecimal SHA-256" >&2
      exit 1
      ;;
  esac
  if [ "${#value}" -ne 64 ]; then
    echo "error: every checksum must be a 64-character hexadecimal SHA-256" >&2
    exit 1
  fi
done

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
mkdir -p "$OUT_DIR"

render() {
  input="$1"
  output="$2"
  sed \
    -e "s/__VERSION__/$VERSION/g" \
    -e "s/__SHA_LINUX_X64__/$SHA_LINUX_X64/g" \
    -e "s/__SHA_MACOS_X64__/$SHA_MACOS_X64/g" \
    -e "s/__SHA_MACOS_ARM64__/$SHA_MACOS_ARM64/g" \
    -e "s/__SHA_WINDOWS_X64__/$SHA_WINDOWS_X64/g" \
    "$input" > "$output"
}

render "$ROOT/packaging/homebrew/gitnodes.rb.template" "$OUT_DIR/gitnodes.rb"
render \
  "$ROOT/packaging/winget/AndreaBozzo.GitNodes.installer.yaml.template" \
  "$OUT_DIR/AndreaBozzo.GitNodes.installer.yaml"
render \
  "$ROOT/packaging/winget/AndreaBozzo.GitNodes.locale.en-US.yaml.template" \
  "$OUT_DIR/AndreaBozzo.GitNodes.locale.en-US.yaml"
render \
  "$ROOT/packaging/winget/AndreaBozzo.GitNodes.yaml.template" \
  "$OUT_DIR/AndreaBozzo.GitNodes.yaml"

printf 'Rendered package manifests for GitNodes %s in %s\n' "$VERSION" "$OUT_DIR"
