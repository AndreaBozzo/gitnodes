#!/usr/bin/env bash
# Layer 2 rename: brain-* crates -> gitnodes-* crates (+ build/asset glue).
# Pure mechanical, fully compiler-checked. Does NOT touch Layer 1 docs
# (README/package.json/ROADMAP) or Layer 3 identifiers (BrainConfig/BrainError)
# or Layer 4 contract (.brain-config.yml). Those are separate passes.
#
# Usage:
#   scripts/rename-crates.sh            # dry run: report planned changes
#   scripts/rename-crates.sh --apply    # perform the rename (git mv + sed -i)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

APPLY=0
[[ "${1:-}" == "--apply" ]] && APPLY=1

CRATES=(app domain graph auth storage)

# token replacements: "from|to"
UNDERSCORE=(
  "brain_app|gitnodes_app"
  "brain_domain|gitnodes_domain"
  "brain_graph|gitnodes_graph"
  "brain_auth|gitnodes_auth"
  "brain_storage|gitnodes_storage"
  "brain_ui|gitnodes"          # leptos output-name + asset/bin/volume names
)
HYPHEN=(
  "brain-app|gitnodes-app"
  "brain-domain|gitnodes-domain"
  "brain-graph|gitnodes-graph"
  "brain-auth|gitnodes-auth"
  "brain-storage|gitnodes-storage"
)

# Files whose CONTENT we rewrite (build + source glue, not docs)
mapfile -t CONTENT_FILES < <(git ls-files \
  'crates/**/*.rs' 'crates/**/Cargo.toml' Dockerfile justfile)

echo "### Layer 2 rename dry-run (APPLY=$APPLY)"
echo
echo "## 1. Directory renames (git mv)"
for c in "${CRATES[@]}"; do
  echo "  crates/brain-$c -> crates/gitnodes-$c"
done
echo
echo "## 2. Content rewrites — hits per file"
total=0
for f in "${CONTENT_FILES[@]}"; do
  [[ -f "$f" ]] || continue
  n=0
  for pair in "${UNDERSCORE[@]}" "${HYPHEN[@]}"; do
    from="${pair%%|*}"
    c=$(grep -oE "${from//_/_}" "$f" 2>/dev/null | wc -l || true)
    n=$((n + c))
  done
  if [[ "$n" -gt 0 ]]; then
    printf "  %4d  %s\n" "$n" "$f"
    total=$((total + n))
  fi
done
echo "  ----"
printf "  %4d  TOTAL token replacements across %d files\n" "$total" "${#CONTENT_FILES[@]}"

if [[ "$APPLY" -eq 0 ]]; then
  echo
  echo "Dry run only. Re-run with --apply to perform the rename."
  exit 0
fi

echo
echo "## Applying..."
# 1. rewrite content first (paths still under old names; sed matches contents)
for f in "${CONTENT_FILES[@]}"; do
  [[ -f "$f" ]] || continue
  for pair in "${UNDERSCORE[@]}" "${HYPHEN[@]}"; do
    from="${pair%%|*}"; to="${pair##*|}"
    sed -i "s/${from}/${to}/g" "$f"
  done
done
# 2. move directories
for c in "${CRATES[@]}"; do
  git mv "crates/brain-$c" "crates/gitnodes-$c"
done
echo "Done. Now run: cargo check --workspace  (and the CI gate set)."
