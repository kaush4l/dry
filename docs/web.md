# The web UI

The same application in a browser: `crates/server` (actix) serves every
route of the app as HTML. It draws each page on the server from the same page
files and core view models that the GPUI window draws from. This is the first
client of the host/client split ([`docs/parity-and-features.md`, Decided](parity-and-features.md#6-decided-owner-2026-11-22)). It is
also the **host**: the one writer of the store, running the engines' turns;
its pages and the GPUI window send it intents (see "The send path").

## Run it

```sh
cargo run -p dry-server                 # http://127.0.0.1:8090, the host of the store
cargo run -p dry-server -- --port 8091  # or DRY_SERVER_PORT=8091
cargo run -p dry-server -- --read-only  # serve pages only: no turns, no writes
cargo run -p dry-server -- --bind 0.0.0.0 --token "$(openssl rand -hex 16)"   # other machines too
scripts/server.sh                       # 8090, restarts on every save
```

As a service (launchd, systemd), behind TLS, and what was proved from
another machine: [server.md](server.md).

8080 stays reserved for `dx serve`. The server reads the repo root the same
way `dry-tools` does: `DRY_ROOT`, else this checkout. It reads the store the
same way the window does: `DRY_STORE`, else `md:state`, else the old
`state.txt`.

## What is served

| Path | What |
|---|---|
| any route | The page: Home, `/desk`, `/desk/:id`, `/desk/:id/artifacts/:name`, `/analytics`, `/settings`, `/settings/ui`, `/code`, and any page you add under `routes/`. The old `/engines/...` addresses answer 302 to the same place under `/desk` (`routes/engines/page.json` says `"moved": "/desk"`). A path with no route gets 404 and the "Not found" view. |
| `/events` | Server-sent events that tell open pages when something changed (see below). |
| `/assets/app.css` | The look: `@font-face` rules, the layout frame, then `crates/html/assets/app.css`. |
| `/assets/app.js` | The client script: the navigation button, soft navigation, and live updates. |
| `/assets/scene.js`, `/assets/scene.wasm` | The scene behind the glass (see "The scene"). |
| `/assets/scenes/<name>.md` | A scene's main file, `scenes/<name>/scene.md`. |
| `/assets/fonts/<file>` | A font shipped in `assets/fonts/` (Amarante). Only files that exist there are served. |
| `GET /api/status` | The host: its generation, the store, and per engine its index, id, name, whether it is in a turn and its activity. |
| `POST /api/…` | The intents (see "The send path"): messages, answers, new and deleted engines, the format, the mode, Detect and Test. |

### How a page is drawn

1. `load::world()` (`crates/server/src/load.rs`) reads everything once per request into a `dry_html::World`:
   - the routes and components (`dry_tools::load`);
   - the theme, `state/local.md`, the store, `models.json` and `sessions.json`.
2. The route's view is expanded (`pages::expand`).
3. `render::component` maps each `Component` to HTML (`crates/html/src/render/`: the renderer the browser build shares, docs/browser.md).
   It reads the same core view models as `crates/gpui/src/views` and matches
   them:
   - the dashboard's tiles;
   - cards;
   - activities and their tints;
   - `inspect` (artifacts, tool groups, prompt slots);
   - ask components;
   - `workbench` (the explorer and the editor);
   - `pages::Pane` sizes and token colours.

   Each GPUI view file has a matching module: `engine.rs`, `ask.rs`,
   `settings.rs`, `code.rs`.
4. `page::document` wraps the view in the layout frame:
   - the backdrop;
   - the navigation: the fixed routes, then LIVE with one entry per engine,
     showing its activity's tint and a status dot;
   - the glass pane: the header (menu button, Home › … trail), the view, and
     the footer.

Things that can't be drawn still show:

- **Unknown components.** If a page file names a component this build does
  not know, the page still loads. That component becomes a placeholder naming
  its `type`, and the file still appears in Settings' list of files that did
  not read.
- **Newer variants.** A `Component` variant added after `render.rs` was last
  edited gets the same placeholder. There is no crash in either case.

### The same look

- **Tokens become CSS custom properties.** `css::tokens` writes
  `--background_body`, `--space_2`, `--font` and so on.
  - Colours are written once per mode, under `:root[data-mode=light]` and
    `:root[data-mode=dark]`. The page follows the app's mode (`State::mode`)
    by setting that one attribute.
  - The tokens are written for each page. A route's `overrides.md` and its
    chosen theme are laid on first, then this machine's `state/local.md`
    (for example the Amarante font). The order is the window's `apply_theme`.
- **The frame comes from `dry_core::layout`.** `css::frame` uses its numbers:
  - the insets, the navigation's width and the header and footer heights;
  - the content box's cap and gutter for each screen class (compact, regular,
    wide);
  - `WIDE_PAGE` for pages with two columns.

  The navigation follows the same rule as `layout::frame`:
  - with `nav_pinned` it is a sidebar;
  - otherwise the header button opens it as a sidebar where there is room, or
    as an overlay below 420 px.

  Before you choose, it starts open on regular and wide screens and closed on
  compact ones. Your choice is kept in the browser (`localStorage`).
- **Columns follow the window's rule.** They wrap to one column exactly when
  the main column would get less than `MAIN_MIN` beside the side column.
- **Liquid glass.** Each pane has a translucent fill: the sheen at the top,
  `glass_fill` lower down. It also has a rim, a highlight, two shadows, and
  `backdrop-filter: blur() saturate()`.
- **The moving backdrop.** Each layer of `dry_core::backdrop` is a blurred
  radial blob. `css::frame` samples the layer's closed loop (`Layer::at`) into
  CSS keyframes. The motion setting (`app.md`'s `motion:`, a `## Scene`
  row; docs/scenes.md) sets each loop's duration
  (`Layer::period`); 0 keeps the backdrop still, and so does
  `prefers-reduced-motion`.
- **No CDN.** The CSS and the script are served from the crate, and the font
  from `assets/fonts/`.

### Looking needs no host

Plain links do everything that only looks:

- navigating (soft: the script swaps the page in place);
- folds (`<details>`; their open state survives live updates);
- the Code page's explorer and editor (`/code?open=crates,crates/core&file=README.md`).

Light/Dark, the app's themes and the Code page's own themes (in the theme
picker and the status bar) are previews: `?mode=light`, `?theme=light-plus`.
They change only what this page shows. Nothing is saved.

## The scene

The scene setting's scene (`scenes/<name>/scene.md`, docs/scenes.md) moves behind the
glass on every page. It is drawn in the browser by the core's own scene
code: `crates/scene` puts `dry_core::scene` (`start`, `step`, `frame`)
behind a small C ABI, built to `crates/server/assets/scene.wasm`
(`scripts/scene-wasm.sh`, about 120 KB, no wasm-bindgen). The page's
`scene.js` feeds it inputs and paints its rectangles, as the browser
build's `crates/web/src/scene.rs` does.

- **Why it runs in the browser.** The picture is pure in time and in its
  inputs, and the inputs include the pointer. It has to be computed where
  the pointer is: frames from the server would lag behind the mouse, and
  a JavaScript port would be a second cat to keep in step with the first.
- **What the page says.** `<canvas id="scene" data-scene="cat">` names the
  app's scene setting, and `<html data-mood>` gives the engines' mood
  (Home's tiles' rule). Both are drawn by the server and refreshed by
  every live update. Colours are resolved by the server for each mode
  (the scene's own, or the theme's for a key with a role) into the page's
  tokens as `--scene-<key>` (`css::scene`); the module's `scene_palette`
  names each key's property, so the module and `scene.js` only draw.
- **The floor** is the footer's top edge, so the cat stands on the footer,
  as in the window. The browser build uses the same rule.
- **Staleness.** The module carries the hash of the scene code it was
  built from. `cargo test -p dry-server` fails until
  `scripts/scene-wasm.sh` is run after a change to
  `crates/core/src/scene/` or `crates/scene/`.

## Live updates

- **The watcher (`live.rs`).** Twice a second it takes a fingerprint (path,
  size and modified time) of two sets of files, so new, changed and removed
  files all count:
  - `store`: the store (the `md:state` folder, or the SQLite file and its
    `-wal`) and `state.txt`. The GPUI window writes these on Save, and a
    terminal artifact's logs live there too.
  - `files`: `routes/`, `components/`, `themes/`, `scenes/`, `assets/fonts/`,
    `state/local.md`, `models.json` and `sessions.json`.
- **The stream (`/events`).** Each change bumps a generation number: the
  watcher's, and every write the host makes. Every event but `hello` has an
  `id:`, kept in a log of the last 1000, so a client that drops resumes:
  the browser sends `Last-Event-ID` on reconnect (or `?since=ID`), and the
  stream replays what it missed. When the log no longer reaches back, `hello`
  says `"reset":true` and the page draws itself again.

  ```text
  hello     {"gen":N,"last":ID}                    on connect (no id)
  change    {"gen":N,"what":["store","files"]}     the store or the files changed
  activity  {"engine":"<id>","index":i,"kind":"llm|tools|compress|wait|working|needs-you|failed|idle","label":"…","short":"…","token":"activity_llm"}
  message   {"engine":"<id>","index":i,"n":k,"role":"user|assistant","gen":N}
  turn      {"engine":"<id>","index":i,"turn":"<id>/<n>","status":"ready|failed|missing|waiting","line":"…","gen":N}
  ```

  A comment every 15 s keeps the connection open.
- **The host's own writes are not "outside" changes.** The host takes the
  store's fingerprint under the same lock it saves under, so the watcher
  only reports a store change somebody else made. Then the host reads the
  store again (when no turn runs), so an edit by hand is picked up.
- **The page (`app.js`).** On `change`, `activity`, `message` or `turn`, the
  page fetches itself again, swaps in the new `#tokens` (the theme) and
  morphs `#nav` and `#pane` into the fresh copy node by node: only what
  changed is touched (elements are matched by `id`, `data-key` or
  `data-session`), so nothing flashes and nothing jumps. A streaming answer
  and a terminal's output grow in place; open folds, scroll positions, a
  selection and focus stay. What is typed survives it: the composer's text,
  focus and caret (`data-keep`), and an open ask box with its picks.
  - **Boxes that keep to their newest line** (`data-stick`): a terminal's
    output and the live view's chat are reversed flex columns, so the
    browser opens them at the bottom and keeps them there with no script.
    A terminal the reader scrolled up in holds still (`paused`, an accent
    line at its foot) until they are back at the bottom; the answer being
    written stays in view while it grows, if it was.
  - **Terminal output in colour.** `dry_html::ansi` turns a program's ANSI
    codes (SGR: colours, bold, dim; 16, 256 and true colour) into spans the
    stylesheet colours per mode (`--ansi0`…`--ansi15`); other escapes are
    dropped and a line rewritten with `\r` shows its last state. Used by the
    terminal artifact's page, the live view's panel and tool results in the
    chat (`render/term.rs`: a spinner while it runs, ✓/✗ and an exit badge
    when it ends, the command after a `$`, Copy).
  - **Narrow screens.** Below 760 px a page of panes (the Code page, the live
    view) stacks its panes and scrolls; status strips stay one line. In a live
    view scoped to one artifact, a part it does not attach gives its room to
    the rest.
  - If the connection drops, the browser reconnects. On `hello`, a page drawn
    at an older generation (`data-gen`) refreshes.
  - Activity tints, status dots and the context gauge all come from the store,
    so they update when it changes.
  - The footer's dot says `live`, `connecting` or `offline`.

Polling is used rather than `notify` because it needs no new dependency and
matches the window, which checks every second. A watcher would cut the delay
from 0.5 s to near zero; it can replace the poll without changing the stream.

## The send path (built)

The server is the host: it runs the turns and is the only writer of the
store (`crates/server/src/host.rs`). Clients send intents, never state.

```text
POST /api/engines/:id/messages
  {"text": "…", "client": "<id>", "base": <generation>}
  → 202 {"turn": "<engine id>/<n>", "gen": <generation after the message was added>, "index": i, "id": "<engine id>"}
  → 409 {"error": "engine is in a turn", "gen": N}
  → 409 {"error": "stale: … changed since your view …", "gen": N}

POST /api/engines/:id/answers
  {"message": <index of the reply that asked>, "answer": "<ask::Ask::answer text>", "base": <generation>}
  or {"message": …, "pick": {"input": {"picked": [], "day": d, "minute": m, "typed": ""}} | {"inputs": [...]} | {"rows": [[k, v]]} | {"reject": true}}
  → 202 {"turn": "…", "gen": N, …}
  → 409 {"error": "already answered" | "engine is in a turn" | "stale: …"}

POST /api/engines                  {} | {"kind": "claude"} (a session) → 201 {"index": i, "id": "…", "gen": N}
POST /api/engines/:id/delete       {"base": N}            → 200 {"gen": N} | 409 (in a turn, stale)
POST /api/engines/:id/format       {"value": "json" | "toon" | "toon=" | ""}   ("": the other one)
POST /api/mode                     {"value": "light" | "dark"}
POST /api/models/:name/detect      the setting's server lists its models  → {"ok", "line", "models"}
POST /api/models/:name/test        one short message to it                → {"ok", "line"}
POST /api/models/:name/default     the default setting (models.json)      → {"gen", "default"}
POST /api/theme                    {"value": "<theme>"}  app.md theme:     → {"gen"}
POST /api/looks/:kind/new|set|rm   {"name", "from"? | "key", "value", "mode"?}  themes/, scenes/ → {"gen"}
POST /api/looks/import             {"text", "replace"?}  a shared look as its folder → {"gen", "kind", "name"}
GET  /api/looks/:kind/:name/export a look's folder as one file (a download)
POST /api/theme/reset              every token of the theme to its default → {"gen"}
POST /api/routes/hide|off|delete   {"value": "<pattern>"}  state/local.md, page.json → {"gen", "pattern"}
POST /api/routes/create            {"path", "title", "icon"?}             → {"gen", "pattern"}
POST /api/engines/:id/reload       {"what": "responses" | "spec", "base"} → {"gen"} | 409 in a turn
POST /api/terminal                 {"line"}  the Code page's terminal      → {"gen"}

# a worker's (docs/sessions.md): its MCP server and hooks read and write here
GET  /api/engines/:id/read         ?what=start|turn|artifact|thread&name=&which=&since= → {"text", "cursor"} (`dashboard`: the old name)
POST /api/engines/:id/posts        {"text", "to"?}                         → {"line", "gen"}
POST /api/threads/:root/posts      {"text", "to"?}  your guided post on a root's thread (`top`: the first; no `to`: kept, for nobody; docs/desk.md) → {"line", "to", "busy", "gen"}
POST /api/boards/:level/posts      the same (its old name, kept for open pages)
POST /api/engines/:id/children     {"goal", "name"?, "template"?, "text"?} | {"child", "text"} → {"line", "child", "replies"}
POST /api/engines/:id/children/:child/take {"after": n}                   → {"reply"} | {"busy"} | {"failed"}
GET  /api/approvals                pending first, then the newest decided  → {"approvals": [...]}
POST /api/approvals                {"engine", "tool", "input"}             → 201 the approval (a rule decides at once)
GET  /api/approvals/:id            → the approval | 404
POST /api/approvals/:id            {"decision": "allow" | "deny", "reason"?} → the approval | 409 decided
```

Events: `tool` (a session's live rows moved) and `approval` (asked or
decided) join `change`, `activity`, `message`, `turn` and `reply`.

- **`:id`** is an engine's index (what the pages use) or its id (what the
  window uses). `new` on `messages` makes a new thread first (the composer
  on a page with no engine). `POST /api/engines {kind}` makes a new thread
  (`kind: thread`) or session (a program's name), a root either way.
- **One writer, one generation.** Every write goes through `Host::commit`:
  the store is saved, the generation bumps, the engines whose texts changed
  are marked with it, a `message` event goes out per message added, and the
  lock file beside the store carries the new generation.
- **`base`** is the generation the client's view was drawn at (the page's
  `data-gen`). A send is refused when its engine changed after that, or when
  it names the engine by index and engines were added or removed after that
  (the index may be another engine now). It is optional: a client that
  sends none is not checked.
- **Answers.** `answer` is the text `ask::Ask::answer` makes (the window
  sends that). A page sends what it gathered (`pick`), and the host makes
  the same text with `Ask::read_input` / `Ask::submit`, so every shell sends
  the same words. The host adds it as the user's message, so `ask::state`
  reads the ask as answered; a second answer gets 409.
- **Turns.** One thread per turn, driving `dry_core::runner::Runner` (the
  machine the browser drives) with the native transport
  (`dry_tools::net::Native`, the window's HTTP) and the real disk
  (`dry_tools::live::Disk`): file and terminal artifacts work, and a turn's
  waiting calls run at once (`run_waits`). Each move borrows the state under
  the host's lock only while the runner decides. Children a step gave a
  message start their own turns; callbacks settle after every turn and on a
  two-second tick, as in the window. No step limit; the history compresses
  at 75% (`context::COMPRESS_AT`), in the core.
- **A missing model** still adds the message; the turn ends at once with
  the reason (`turn` event, status `missing`), shown on the engine's page.

### The page

`Access::Host` (`dry_html::world`): the composer and every open ask are
live, and so is every control in `HOST_ACTS`. `app.js` sends each with
`fetch`, with the page's `data-gen` as `base`. A refusal shows under the
composer in red (`data-composer-note`), as the window shows its note, and
the text stays. A stale refusal also draws the page again. Other refusals
show as a toast. The controls `HOST_ACTS` lists:

- Send and Clear, the ask controls;
- New thread, New session, Delete engine (`new-engine` is the old name of
  `new-thread`);
- the format, Light/Dark;
- Detect, Test, Make default;
- Refresh, approvals, peers.

The app's own files are written by the host too (`crates/server/src/files.rs`,
one writer at a time; the desktop app's writes, through `dry_tools`):

- the theme's Reset all (the theme file), and Copy, Remove… and Import…
  for a theme or scene folder (the choice itself is app.md's `theme:`,
  saved with the store like `scene:`);
  Reload theme and Reload routes/ redraw, since every page reads them
  anyway;
- a route's Hide/Show and Turn off/on (`state/local.md`), Delete
  (`page.json`) and New page (Create);
- Reload responses.md and Reload spec (the store's copy) on an engine;
- **the Code page's terminal**: a line typed on any page runs on the
  host in the root folder, through the terminal artifact's disk
  (`Disk::run`: `sh -c`, no input, secrets left out of its environment).
  Its output streams in on every open page, and `clear` empties it.

These are `HOST_ONLY` (`dry_html::world`): the browser build draws them
off, since it has no such files or processes.

Still drawn off on the host page: Save (there is nothing to save, the
host saves every change; the header chip says `host`), editing one
token's value, and Voice ("not checked in this runtime").

### Safety

- **Loopback by default.** It binds `127.0.0.1`. `--bind` (or
  `DRY_SERVER_BIND`) to anything else refuses to start without `--token`
  (or `DRY_SERVER_TOKEN`). `--strict` (`DRY_SERVER_STRICT=1`) asks the
  token of this machine too, for a server behind a proxy here.
- **The token.** A request from another machine needs it: `Authorization:
  Bearer <token>`, or open any page once with `?token=<token>`, which sets an
  HttpOnly, SameSite=Strict cookie and redirects to the plain address. This
  machine's own requests (a loopback peer) need none, so the window and
  `ssh -L` tunnels work as they are. A reverse proxy on this machine would
  look like loopback too: run the server `--strict`, or do not run one.
  An IPv4 peer on a dual-stack socket (`::ffff:127.0.0.1`) counts as
  loopback.
- **The terminal is the machine.** A page with the token can run
  commands on the host as the server's user (the Code page's terminal),
  as code-server's terminal can. The token is the key.
- **Writes are JSON from this server's pages.** A write without
  `Content-Type: application/json` gets 415; one whose `Origin` is not this
  server gets 403. A page on another site cannot post to it (a cross-site
  JSON post needs a preflight, which the server never grants).
- **No keys are stored.** A model's key is read from the environment
  variable its setting names, at the moment of the call. The token lives
  only in the process (and the browser's cookie).

### The host lock, and the window

`<store>.host.json` beside the store (`state.host.json` for `md:state`):
`{pid, url, gen, store}`, written when the host starts and on every write,
removed when it stops (`dry_tools::host`). A lock whose host does not
answer on its port is stale.

- A second server for the same store finds the live lock and serves
  read-only (`Access::ReadOnly`, the old behaviour), saying who hosts.
- **The GPUI window** (`crates/gpui/src/client.rs`) reads the lock every
  second. While a host answers: it reads the store again when the lock's
  generation moves, takes each engine's activity from `/api/status` for the
  tints, sends Send and an ask's answer through the host (a refusal shows
  under the composer), runs no turns and no callbacks, and Save refuses.
  The footer says `Hosted by <url>`. When the host goes, the window runs as
  it always did.

What the window does not do yet as a client: its other edits (a new
engine, the format, Delete, settings, mode) still change only its own copy,
which the next reload replaces (they need to go through the same intents);
it shows no measured prompt or compression line for the host's turns.

## History: read-only

Before the host, the GPUI window ran the turns and did not watch the store,
so a second writer could have its state overwritten by the window's next
Save. The web only read: the composer and every ask were drawn disabled
with "sending from the web arrives with the host split; send from the
desktop app for now", the two send endpoints returned 501, and the disk
handed to the core for drawing (`load::ReadOnly`) refused writes and
commands. Drawing a page still never writes or runs anything; `--read-only`
(or a second server on the same store) serves exactly that view.

## Capabilities (docs/capabilities.md)

The host keeps a capability registry: the compiled ones, this machine's
`capabilities/*.md` (probed at start; `DRY_CAPABILITIES=<dir>|none`), and
every peer that joined. Engines are equipped with it at every turn, and an
outside call (`battery.status`) is routed through it.

| Call | Body | Answer |
|---|---|---|
| `GET /api/capabilities` | | `{capabilities: [{id, provider, state, why, doc, tools, snapshot, effective}], peers, errors}` |
| `POST /api/capabilities/peers` | `{url, code, name?}` | pairs with the peer's code, the handshake, then its stream and heartbeat: `{peer, runtime, platform, capabilities}` |
| `POST /api/capabilities/peers/<name>/leave` | | its capabilities absent now |

**Settings → Capabilities** (`/settings/capabilities`, the `capabilities`
component) lists them by provider and state, with a **Join a peer** form and
**Leave** per peer on the host, and the browser's **Grant** buttons in the
web app. A change (a peer joined, left, re-registered, a snapshot came)
bumps the generation, so open pages redraw.

## The browser app and a folder on the person's computer

The wasm app (`crates/web`, docs/browser.md) has no files and no terminal.
Paired with a companion (docs/companion.md, docs/companion-protocol.md),
it gets one folder of the person's computer:

- **The live view works in the browser.** The live-view parts (`live-view`
  components: explorer, editor, panel, chat, status) are drawn by the
  shared renderer (`dry_html::render::live`, from `liveview::build`, the
  same core view the GPUI window draws), so `/desk/:id/live` and
  `/desk/:id/artifacts/:name/live` show the folder as the engine sees
  it, on the web host too. The engine page's Artifacts section links it
  first when the engine holds a file or terminal artifact.
- **Context stays in core.** `dry_core::workspace::Folder` is the
  `live::Host` over the companion's primitives (`Workspace`: read, write,
  list, stat; run, start, tail, stop), so open, focus, edit, save, the
  unsaved overlay, the open-file snapshot, sessions, tails and callbacks
  are the desktop's code. A terminal `run` waits asynchronously
  (`/v1/term/run`), so the page keeps drawing.
- **State in the folder.** With **Keep state in the folder** the engines
  are written to `<folder>/.dry/state/` through the companion (one
  writer), with `.dry/snapshot.json` at every save; IndexedDB
  (`dry-workspace`) is the offline copy, and pending changes go back when
  the page pairs again. On load, `Snapshot::restore` puts back what the
  state lost.

Proved end to end on 2026-11-23 on a scratch folder and port (the app
served by the companion, a scripted model declared as a `port` provider):
pairing, the live view listing the folder (no `.env`), an engine opening,
editing and saving `hello.txt` and running `cat hello.txt; ls` in it, a
second turn leaving an unsaved README edit; then the companion stopped and
the page closed, both started again, paired: the engines, both open files,
the unsaved edit and the session came back; a fresh browser (storage
wiped) paired with the same folder loaded the same; an artifact whose
state was emptied by hand came back from the snapshot; an engine made while
the companion was away reached `.dry/state/` when it returned.

## Not yet on the web

- **Pane edges.** Dragging the edges of resizable panes on `/code` is still
  desktop-only.
- **MCP on the host.** The host starts no MCP servers yet: an engine's MCP
  tools answer that nothing knows the call. The window still starts them
  for its own turns when no host runs.
- **Voice.** Voice is shown as "not checked in this runtime", the way it
  shows in any runtime that hasn't checked for it.

## Tests

- **Rendering.** `cargo test -p dry-server` covers the token → CSS output,
  query parsing and escaping, the unknown-component fallback and the
  watcher's fingerprint.
- **The host, end to end (`crates/server/tests/host.rs`).** The real binary
  on a scratch root whose engines talk to a fake model in the test: a send
  is taken (202), the turn's `activity`, `message`, `turn` and `change`
  events come with rising ids, the file tool wrote through the host, the
  reply is in the store and on the page, the lock carries the generation, a
  dropped stream resumes with `Last-Event-ID`; a busy engine and a stale
  view get 409, a non-JSON write 415 and a foreign `Origin` 403; an ask is
  answered once (from a page's `pick`, and as text through the window's
  client); new and deleted engines, the format and the mode; `--bind
  0.0.0.0` without a token refuses to start. Unit tests: the generation and
  stale checks (`host.rs`), the event log and resume (`live.rs`), the flags
  (`main.rs`), the lock and the client's reload rule (`dry_tools::host`),
  the window's open-ask pick (`crates/gpui/src/client.rs`).
- **End to end (`crates/server/tests/web.rs`).** These tests run the real
  binary on a free port against a scratch `DRY_ROOT`, built from this repo's
  page files and `dry_core::files::sample()`. They check that:
  - every route returns 200 with its key content;
  - an unknown component shows as a placeholder;
  - `/events` sends `change` when the store is saved and when a theme file is
    edited.
