# dry in the browser

The whole app as WebAssembly in a page: the core's state and turn loop, the
same renderer as the server, the state kept in the browser, each step sent
to a model server with `fetch`. Hosted as static files (GitHub Pages), it
needs no server of its own.

```text
dist/
  index.html     the landing page (crates/web/landing/index.html), the abyss scene behind it
  app/           the app (crates/web, built by trunk): index.html, dry-web.js, dry-web_bg.wasm, fonts/
  404.html       sends a stray path back to the landing page
  .nojekyll
```

## What runs, and what is refused

| In the browser | How |
|---|---|
| Engines, their turns, no step limit | `dry_core::runner::Runner`, fed by `fetch` (below) |
| Compression at 75% of the window | the runner, as the window does; the window is asked of LM Studio once per model (`/api/v0/models`) |
| The asks (choice, multi, confirm, date, date and time, number, text, card) | drawn live; the answer text is the core's (`Ask::answer`) |
| Child engines (`engine_call`, `engine_start`) | each child runs its own turn at the same time; replies come back through `turn::callbacks` |
| Artifacts: text, shared, learning, engines, clock | as everywhere |
| Home's dashboard, Desk, an engine's page, artifact pages, Settings, UI tokens, Code | the shared renderer (`crates/html`) |
| The scene (an app setting) | painted on a `<canvas>` from `dry_core::scene` (the window's scene; the server's pages use scene.js) |
| State across reloads | IndexedDB (database `dry`, store `texts`) |

| Refused | Why, as the page says it |
|---|---|
| `file` artifacts (open, read, write files) | "no files in the browser: a file artifact reads and writes a folder on a computer" |
| `terminal` artifacts (`run`, `start`, `background`) | "no terminal in the browser: commands run on a computer" |
| MCP tools | none are started: they are programs; a call to one gets the built-ins' "no such tool" |
| Session engines (`claude`, `codex` …) | "a … session runs its program on a computer; open this engine in the desktop app" |
| Editing routes, themes, tokens; the Code page's terminal | drawn off with "not in the browser: this needs the desktop app" |

A Linux to run commands in is on its own route, `#/container`: Alpine in
v86, with exec, a port's page and saved machines (docs/container.md). It
is not wired to `terminal` artifacts yet.

The Code page browses the app's own bundled files (routes, components,
themes, scenes, `responses.md`, `models.json`, `sessions.json`), read-only.

## The seams: traits and their implementations

All in `dry-core`, pure (no I/O); a shell implements them.

| Trait | What it is | Implementations |
|---|---|---|
| `store::Store` | whole states, blocking: `describe`, `exists`, `load`, `save` | `dry_tools::store::MdStore` (markdown folder), `SqliteStore`, `store::Memory` |
| `store::TextStore` | texts by path, async: `read_all`, `write(Changes)` | `dry_web::idb::Idb` (IndexedDB), `Local` (localStorage), `store::Memory`; `Backend` picks the first that opens |
| `runner::Transport` | reach a model server: `post(Request)`, `get(url)` | `dry_web::net::Fetch` (`fetch`, a key as `Bearer` when set); tests' fake model |
| `live::Host` | files and processes for artifacts | `dry_tools::live::Disk`, the server's `ReadOnly`, `turn::NoHost`, `dry_web::env::VmHost` (the browser VM, docs/container.md), `dry_web::host::Browser` (refuses, saying why) |
| `live::Time` | the clock | `dry_web::host::Browser` (`Date`, with its offset), a fixed `Clock` |
| `dry_html::world::Repo` | the Code page's folder tree | the server's `Checkout`, `MemoryRepo` (the bundle), `NoRepo` |

- **The texts.** `store::to_texts` / `from_texts` turn the state into the
  same files a markdown folder holds (`app.md`, `engines/<id>/spec/…`,
  `convo/messages.md`, `artifacts/<name>.md` …). A test checks that
  `MdStore` writes exactly these, so a browser's store can be written out
  as a `state/` folder and back. `Saver` remembers what was written, so a
  save writes only what changed; `local/…` paths (this browser's settings)
  travel in the same store and are never touched by a state save.
- **The loop.** `runner::Runner` is the turn as a machine: `next(state,
  came)` returns a `Next` (`Probe`, `Post`, `Run`, `Waiting`, `Done`) and
  the shell brings back what it came to. It borrows the state only while
  it decides, so the page draws and takes clicks while the model thinks.
  `runner::io` does one move's I/O with any `Transport`. A test runs the
  same fake-model turn through the runner (async) and through
  `turn::step_in` (blocking): the same log, artifacts and requests, byte
  for byte.

### Plugging in another backend

- **OPFS** (the browser's private file system): a `TextStore` that keeps
  one file per path; add an arm to `idb::Backend` and try it before
  IndexedDB.
- **A remote host** (option 3, later): a `TextStore` whose `read_all` is
  `GET /api/texts` and `write` is `POST /api/texts` with the `Changes`, and
  a `Transport` that posts to the host, which calls the model. The page,
  the renderer and the runner stay as they are; the host becomes the only
  writer (docs/web.md, "The planned send path"), and its `/events` stream
  tells the page when to draw again.

## The renderer, shared

`crates/html` (`dry-html`) is the renderer that used to live in
`crates/server`: every route and component as HTML from the core's view
models, the CSS from the tokens and the layout standard. Pure; the server
and the browser fill a `World` and call `page::document` (the server) or
`page::parts` (the browser sets `#tokens`, `#nav` and `#pane`).

- `Access::ReadOnly` (the server): what would change the app is drawn
  disabled with the reason.
- `Access::Live` (the browser): those are buttons with `data-act` and
  their values; one listener per event kind on the document handles them
  (`crates/web/src/app.rs`, `act`). No other script.
- Links stay `/desk/0`; the browser turns them into hash routes
  (`#/desk/0`), since Pages has no server rewrites.

## Build and serve locally

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked           # 0.21; it uses wasm-bindgen 0.2.130 and wasm-opt from PATH, or fetches them
scripts/web-build.sh                   # into dist/, for https://kaush4l.github.io/dry/ (base /dry/)
scripts/web-build.sh /                 # for a site at a domain's root
```

To see it under `/dry/` as Pages will serve it, serve the folder above
`dist`, with `dist` as `dry`:

```sh
mkdir -p /tmp/site && ln -sfn "$PWD/dist" /tmp/site/dry
python3 -m http.server 8765 --bind 127.0.0.1 --directory /tmp/site
# http://localhost:8765/dry/      the landing page
# http://localhost:8765/dry/app/  the app
```

While working on the app alone, `cd crates/web && trunk serve --port 8765`
rebuilds on save (then the app is at `/`).

## LM Studio

1. Load a model in LM Studio's **Developer** tab and start the server
   (port 1234).
2. In the server's **Settings**, turn on **Enable CORS**. Check it:

   ```sh
   curl -si -X OPTIONS http://localhost:1234/v1/chat/completions \
     -H 'Origin: https://kaush4l.github.io' -H 'Access-Control-Request-Method: POST' \
     -H 'Access-Control-Request-Headers: content-type' | grep -i access-control
   # Access-Control-Allow-Origin: *
   # Access-Control-Allow-Headers: *
   # Access-Control-Allow-Methods: PUT, POST, PATCH, DELETE, GET
   ```

3. In the app: **Settings → Models → Detect**. It lists the server's
   models, the loaded ones first, and picks the first loaded one. **Test**
   asks it for one word. New engines use this endpoint and model; engines
   that had none follow when you change it.

The endpoint is `http://localhost:1234/v1` unless you change it. A key
field is there for a cloud server only: what you type stays in this
browser's IndexedDB, is sent as `Authorization: Bearer …` to that
endpoint only, and is never drawn back. Never type one on a shared
computer.

### What each browser needs (checked 2026-11-23)

What was measured here, with LM Studio's CORS on (`Access-Control-Allow-
Origin: *`, no `Access-Control-Allow-Private-Network` header in its
preflight answer):

| Page origin → `http://localhost:1234` | Chrome 157 |
|---|---|
| `http://localhost:8765` (a local copy) | works: GET and the JSON POST (with its preflight) |
| `https://` on this machine | works |
| `http://192.168.x.x` (another machine on the LAN, plain http) | works |
| `https://` treated as public (GitHub Pages; tested with `--ip-address-space-overrides=…=public`) | **blocked** until the site is allowed to reach local apps |
| the same with Local Network Access checks off | works: CORS is not the problem |

So, from GitHub Pages:

- **Chrome / Edge**: Chrome's **Local Network Access** (2025 onward)
  replaced the old private-network preflight. The first call from the
  hosted page shows a permission prompt asking to let the site connect to
  devices and apps on your local network; choose **Allow**. If it was blocked, allow it in
  the site's settings (the icon left of the address → Site settings →
  Local network access). No server header is needed for this; mixed
  content is not an issue (`http://localhost` counts as secure).
- **Firefox**: not tested here (not installed). Firefox lets an https page
  call `http://localhost` and `http://127.0.0.1`; it is rolling out its own
  local network prompt, which works the same way: allow it.
- **Safari**: not tested here. Safari blocks an https page from calling
  `http://localhost` as mixed content, and LM Studio serves plain http.
  Use the workaround.

**Workaround for any browser** (and the most private way): run the local
copy. Build it (`scripts/web-build.sh`) and serve `dist` from this machine
as above, or `cd crates/web && trunk serve`, and open it on
`http://localhost:…`. A page on `localhost` calling `localhost` needs only
LM Studio's CORS. (Each origin has its own IndexedDB: the hosted page and
the local copy keep separate engines.)

## First visit and the demo

With nothing stored, the app offers **Start empty** or **Load demo**. The
demo (`crates/web/src/demo.rs`) is four engines: a trip planner with a
plan artifact and an open choice, a release with a child engine
(`engine_call` and its reply), an answered date ask and notes, and an empty
one. It needs no model server, so the hosted page shows the app to anyone.
Home's dashboard has **New thread** and **Load demo** too (the demo
replaces what is kept, after asking).

## Publish on GitHub Pages

The workflow is `.github/workflows/pages.yml`: on every push to `main` it
runs the tests of the browser's crates, builds with
`scripts/web-build.sh "/<repo name>/"`, and deploys `dist/` with
`actions/deploy-pages`. The owner does these steps, once:

```sh
gh auth login                                    # GitHub.com, HTTPS, log in with a browser
cd /Users/kaush/Downloads/Dev/dry-learn
gh repo create kaush4l/dry --public --source . --remote origin
git push -u origin main
gh api -X POST repos/kaush4l/dry/pages -f build_type=workflow   # Settings → Pages → Source: GitHub Actions
gh run watch                                     # the first deploy
# https://kaush4l.github.io/dry/        the landing page
# https://kaush4l.github.io/dry/app/    the app
```

If the repo has another name, the workflow follows it (the base path is
the repo's name).

## Tests

- `cargo test -p dry-core`: `store` (the texts, `Memory` as both kinds of
  store, saving only changes), `runner` (the async turn equals the
  blocking one; a failed call; the window probe; what cannot run),
  `dashboard::board`.
- `cargo test -p dry-tools`: `MdStore` writes exactly the core's texts.
- `cargo test -p dry-html`: the bundle loader reads the repo's files as
  `dry_tools::load` does; the same page is live in the browser and
  read-only on the server; every standard route draws both ways.
- `cargo test -p dry-server`: unchanged, on the moved renderer, including
  the end-to-end tests.
- `cargo test -p dry-web`: the settings and the demo (native).
- IndexedDB and localStorage in a real browser (`crates/web/tests/idb.rs`):

  ```sh
  cd crates/web
  NO_HEADLESS=1 WASM_BINDGEN_TEST_ADDRESS=127.0.0.1:8766 \
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
    cargo test --target wasm32-unknown-unknown --test idb
  # open http://127.0.0.1:8766: 3 passed
  ```

  Headless needs a chromedriver of the installed Chrome's version (this
  machine's is 151 against Chrome 157), or geckodriver.

## Not yet

- Option 3, a client for a remote host: the `TextStore` and `Transport`
  over HTTP above, the host's write endpoints (docs/web.md), and a mobile
  shell on the same page.
- Files for `file` artifacts in the browser (an OPFS-backed `Host`).
- Streaming replies (the reply arrives whole; the tint says the call is out).
- Voice.
