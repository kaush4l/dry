# dry-learn

Fresh skeleton. One change at a time.

## Layout

    crates/core    dry-core    State (engines), Screen, layout standard; no UI, disk or network
    crates/gpui    dry-gpui    native window (GPUI); saves state.txt at the repo root
    crates/html    dry-html    the UI as HTML and CSS, shared by the server and the browser
    crates/server  dry-server  HTTP server (actix-web) for the web side
    crates/tools   dry-tools   the disk: stores, files, processes, MCP, the `dry` command
    crates/web     dry-web     the app in the browser (WebAssembly): docs/browser.md

## Run

GPUI window, reopens on every save:

    scripts/dev.sh

Server on http://127.0.0.1:8090, restarts on every save:

    scripts/server.sh

Both need `brew install watchexec`. Stop with Ctrl-C.

Once, without watching: `cargo run -p dry-gpui` or `cargo run -p dry-server`.

In the browser (WebAssembly, state in IndexedDB, LM Studio for the model),
built for GitHub Pages into `dist/`:

    scripts/web-build.sh        # base /dry/; see docs/browser.md
