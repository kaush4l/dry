#!/usr/bin/env bash
# Build the scene module the server's pages draw the scene with
# (crates/scene: dry_core::scene behind a C ABI) into
# crates/server/assets/scene.wasm, which dry-server embeds and serves as
# /assets/scene.wasm. Run it after changing crates/core/src/scene/ or
# crates/scene/: `cargo test -p dry-server` fails while the file is stale.
#
# Needs: rustup target add wasm32-unknown-unknown; wasm-opt (binaryen) if
# present makes it smaller. Its own target folder, so it never waits on
# (or invalidates) the workspace's.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/crates/server/assets/scene.wasm"
CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_LTO=true CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
CARGO_PROFILE_RELEASE_PANIC=abort CARGO_PROFILE_RELEASE_STRIP=true \
  cargo build --manifest-path "$ROOT/Cargo.toml" -p dry-scene --lib --release --target wasm32-unknown-unknown \
  --target-dir "$ROOT/target/scene-wasm"
BUILT="$ROOT/target/scene-wasm/wasm32-unknown-unknown/release/dry_scene.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals "$BUILT" -o "$OUT"
else
  cp "$BUILT" "$OUT"
fi
echo "built $OUT ($(wc -c < "$OUT" | tr -d ' ') bytes)"
