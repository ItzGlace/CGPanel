#!/usr/bin/env bash
# Build the real pre-update 0.5.2 panel while preserving the current checkout.
set -euo pipefail
cg_upgrade_repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
cg_upgrade_source=$(mktemp -d)
trap 'rm -rf "$cg_upgrade_source"' EXIT
cg_upgrade_commit=ea245c04c3397f9e5006e710cd45ae8bd08bcd61
if ! git -C "$cg_upgrade_repo" cat-file -e "$cg_upgrade_commit^{commit}" 2>/dev/null; then
  git -C "$cg_upgrade_repo" fetch --no-tags --depth=1 origin "$cg_upgrade_commit"
fi
git -C "$cg_upgrade_repo" archive "$cg_upgrade_commit" | tar -x -C "$cg_upgrade_source"
export CARGO_TARGET_DIR="$cg_upgrade_repo/target"
cargo build --locked --manifest-path "$cg_upgrade_source/Cargo.toml" --bin cgpanel
cp "$CARGO_TARGET_DIR/debug/cgpanel" "$CARGO_TARGET_DIR/upgrade-baseline-cgpanel"
cargo build --locked --manifest-path "$cg_upgrade_repo/Cargo.toml" --bins
