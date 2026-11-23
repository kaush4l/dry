#!/usr/bin/env bash
# Build dry for the browser into dist/, ready for GitHub Pages:
#
#   dist/index.html      the landing page (crates/web/landing/)
#   dist/app/            the wasm app (crates/web, built with trunk)
#   dist/404.html        sends a stray path to the landing page
#   dist/.nojekyll       Pages serves files as they are
#
# Usage: scripts/web-build.sh [BASE]   BASE is where the site is served,
# default /dry/ (https://kaush4l.github.io/dry/). Use / for a site served
# at a domain's root. Needs: rustup target add wasm32-unknown-unknown;
# cargo install trunk (it fetches wasm-bindgen and wasm-opt if missing).
set -euo pipefail
BASE="${1:-/dry/}"
case "$BASE" in */) ;; *) BASE="$BASE/" ;; esac
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
rm -rf "$ROOT/dist"
cd "$ROOT/crates/web"
trunk build --release --public-url "${BASE}app/" --dist "$ROOT/dist/app" --filehash false index.html
# The container route's Alpine image and apk packages (docs/container.md):
# kept in git under vm/ for now; vendor-dl/ (scripts/vm-fetch.py and
# scripts/vm-image.py) remakes them.
if [ -f "$ROOT/vm/image/vmlinuz-virt" ]; then VM="$ROOT/vm/image"; APK="$ROOT/vm/apk/alpine"
else VM="$ROOT/vendor-dl/out"; APK="$ROOT/vendor-dl/apk-mirror/alpine"; fi
if [ -f "$VM/vmlinuz-virt" ]; then
  cp "$VM/vmlinuz-virt" "$VM/alpine-dry.cpio.gz" "$ROOT/dist/app/vm/"
  [ -d "$APK" ] && cp -R "$APK" "$ROOT/dist/app/vm/"
else
  echo "no VM image (python3 -I scripts/vm-fetch.py && python3 -I scripts/vm-image.py): the container route will say so"
fi
cp "$ROOT/crates/web/landing/index.html" "$ROOT/dist/index.html"
touch "$ROOT/dist/.nojekyll"
cat > "$ROOT/dist/404.html" <<HTML
<!doctype html><meta charset="utf-8"><title>dry</title>
<script>location.replace("${BASE}")</script>
<p>Not here: <a href="${BASE}">dry</a></p>
HTML
echo "built $ROOT/dist for ${BASE}"
