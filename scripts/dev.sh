#!/bin/sh
# Dev server for the GPUI window: on every save under crates/, default/
# (the default under the store, docs/default.md §1) or of Cargo.toml,
# rebuild and reopen the window. GPUI has no hot reload of its
# own, so this is "restart on save". state/ is not watched, so pressing
# Save in the app does not restart it. Needs watchexec (brew install watchexec).
cd "$(dirname "$0")/.." || exit 1
exec watchexec --restart --clear --watch crates --watch default --watch Cargo.toml -- cargo run -p dry-gpui
