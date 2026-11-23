#!/bin/sh
# Dev server for dry-server: restarts the HTTP server on every save under
# crates/ or of Cargo.toml. Serves http://127.0.0.1:8090.
cd "$(dirname "$0")/.." || exit 1
exec watchexec --restart --clear --watch crates --watch Cargo.toml -- cargo run -p dry-server
