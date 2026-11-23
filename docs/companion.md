# The companion

A small local server, one file, that any computer with Node 20+ can run
(Node 22+ for the browser capability). The dry web app connects to it and
gets what a browser cannot do itself: the computer's **projects**, its
**runtimes**, **local inference** (LM Studio, Ollama, the `claude` and
`copilot` CLIs), **git**, **custom tools**, and a **headless browser** to
look at the apps it runs. It can also **serve the app itself**. So you open
the page on a computer, connect, and run your engines against that
computer's projects and models: an environment and an agent that can make
changes, apply them, test them (and look at the result), and (with your
click) push them.

```text
browser (dry web app, wasm)                      this computer
┌──────────────────────────────┐  127.0.0.1     ┌────────────────────────────────────┐
│ the app (served from here,   │ ─────────────▶ │ companion/dry-companion.mjs         │
│  or the hosted page)         │   /v1/...      │  /, /app/      the built app        │
│ Settings → Companion (pair)  │                │  projects, files (roots)            │
│ Projects view (#/projects)   │ ◀───────────── │  exec sessions (/bin/sh), event log │
│ file / terminal / browser    │  SSE events    │  tools/*.md, tools/*.json ──┐       │
│   artifacts, outside tools   │  (resumable)   │  /mcp (same tools) ◀────────┘       │
│ model calls (Transport)      │                │  /v1/chat/completions ─▶ LM Studio, Ollama, claude -p, copilot -p
└──────────────────────────────┘                │  browser ─▶ this machine's Chrome (CDP, headless)
                                                │  git (the machine's credentials)    │
                                                └────────────────────────────────────┘
```

The wire contract every companion keeps (this Node one, the Python one
`companion/dry_companion.py`, and the Rust `dry companion`) is
**docs/companion-protocol.md**, checked from outside by `conformance/`.
This page is the Node companion's own guide; the other two are in "The
Python companion" and "The Rust companion" below.

| companion | start it | needs |
|---|---|---|
| Node | `node companion/dry-companion.mjs --folder DIR` | Node 20+ (22+ for the browser) |
| Python | `python3 companion/dry_companion.py --folder DIR` | Python 3.9+ |
| Rust | `dry companion --folder DIR` (`cargo build --release --bin dry`, then `target/release/dry companion …`) | nothing: one binary |

## Run it

```sh
node companion/dry-companion.mjs --folder ~/my-project
# dry companion 0.1.0
# the page gets this folder: /Users/you/my-project
#   its files, and a terminal in it (state in .dry/). Use it? [Y/n]
#
# checks
#   ok       folder     /Users/you/my-project (state in .dry/)
#   ok       provider   lmstudio: port http://127.0.0.1:1234: 3 models
#   absent   provider   local: native: mlx_lm.server (mlx, metal) is not on this machine
#              → pip install mlx-lm   (Apple silicon; gives mlx_lm.server)
#   ok       capability battery: This Mac's battery: …
#   ok       git        git version 2.54.0
#   ok       chrome     browser: /Applications/Google Chrome.app/…
#   missing  container  no docker or podman
#              → install Docker Desktop or Podman (optional: containers)
#
# switches (saved in /Users/you/my-project/.dry/companion.json; actuators start off)
#    1  on   file             The workspace folder's files: …
#    2  off  terminal         (actuator) Commands in the workspace folder …
#    …
# flip some (their numbers, e.g. "2 5"), or Enter to go on: 2
#
# listening http://127.0.0.1:7777
# the app: http://localhost:7777/app/
#   pairing code: 6GZ5ZP   (type it in dry → Settings → Companion; it works once)
# 04:29:00  connected: p1 from http://localhost:7777 until 16:29
# 04:29:00  p1 is dry-web (Mozilla/5.0 …)
# 04:26:33  run engine-1/terminal/1: cat hello.txt; ls
# 04:26:33  exec 1 run ended 0: cat hello.txt; ls
```

The start is the person's: the folder is confirmed, the checks say what
this machine has (each found or missing, with the fix), and the switches
say what the page may use. **Actuators start off** (the terminal, and any
capability with an `approval: yes` tool); flip one by its number, or later
by typing `on terminal`. The choice is saved in `.dry/companion.json`;
`--yes` takes the saved choices without asking (and `--on` / `--off` set
them from the command line). Then the code, then a line for each pairing,
each peer's hello, and each command or call as it runs and ends (the
page's reads and saves are not lines; `--verbose` shows them). **Esc stops
everything it runs.** Plain stdout and stdin: the same in the terminals of
macOS, Linux and Windows.

| Option | Default | |
|---|---|---|
| `--folder DIR` | the current folder | the folder the page gets; dry's state goes in `DIR/.dry/` |
| `--port N` | 7777 | listens on 127.0.0.1 only |
| `--yes` | | take the saved switches and the folder without asking |
| `--on ID[,ID]`, `--off ID[,ID]` | | switch capabilities on or off (saved) |
| `--providers FILE` | `DIR/.dry/providers.json` | the declared model providers (made from `providers.json`, the template, on the first run) |
| `--config FILE` | `DIR/.dry/companion.json` | where the switches are saved |
| `--pair-code-file FILE` | | each new pairing code written there |
| `--heartbeat S` | 15 | the streams' `: ping` interval |
| `--app DIR` | `../dist` next to the script, if it is there | the built app to serve at `/` and `/app/` |
| `--no-app` | | serve no app |
| `--tools DIR` | | custom tools besides each project's `tools/` folder |
| `--allow-url URL` | | a URL prefix the browser may open besides localhost (repeatable) |
| `--dry PATH` | | the `dry` binary that runs a project's evidence |
| `--chrome PATH` | found on the machine | the Chrome / Chromium / Edge / Brave to drive |
| `--ttl HOURS` | 12 | how long a pairing lasts |
| `--capabilities DIR` | `../capabilities` next to the script | capability files this machine declares (docs/capabilities.md §3) |
| `--no-capabilities` | | declare none (its own terminal, file, browser, git, container stay) |
| `--origin URL` | | one more page origin to answer (repeatable) |
| `--root DIR` | the folder | projects mode: a folder whose projects the Projects view shows (repeatable) |
| `--lmstudio URL`, `--ollama URL` | | the same as declaring a `port` provider named `lmstudio` / `ollama` |
| `--store DIR` | `DIR/.dry/state` | where the page's state goes |
| `--quiet`, `--verbose` | | no lines; a line for every read too |

No npm install: only Node's `http`, `child_process`, `fs`, `crypto`,
`readline`, and Node 22's global `WebSocket` for the browser. While it
runs, type in its terminal:

| Command | |
|---|---|
| Esc | stop everything it runs (the same as `stop-all`) |
| `pairings` | every pairing: id, its peer's hello, page origin, since, until |
| `revoke <id>` | end one pairing now (its streams close) |
| `on <id>`, `off <id>` | switch a capability (saved; peers re-register) |
| `switches` | every capability, on or off, actuator or not |
| `providers` | probe the declared providers again: each ok, failed or absent, with why and the fix |
| `stop-all` | the kill switch: stop every command it runs, and close the browser |
| `code` | the current pairing code |
| `tools` | the custom tools it found, and the files it refused |
| `capabilities` | every capability it provides: state, why, tools; the files that did not read |

### The Python companion

For a machine with Python and no Node: `companion/dry_companion.py`, one
file, Python 3.9+, the standard library only (nothing to `pip install`).
It keeps the same contract (passes `conformance/`, 40 of 40) and starts the
same way: the same options, the same checks, numbered switches saved in
`.dry/companion.json`, the `listening` line, the code, the live lines, and
**Esc stops everything it runs** (termios on macOS and Linux, msvcrt on
Windows).

```sh
python3 companion/dry_companion.py --folder ~/my-project
python3 companion/dry_companion.py --folder ~/my-project --yes --on terminal --port 7777
node conformance/run.mjs --cmd "python3 $PWD/companion/dry_companion.py"   # the contract (an absolute path:
                                                                           #   the suite starts it from a temp folder)
python3 -m unittest companion/test_dry_companion.py                        # its parser, as dry_core's
```

It has the folder's files and terminal, the store and snapshot, providers
(cli, port, native; never installing or downloading), declared
capabilities (`capabilities/*.md`, read by the same rules as
`dry_core::capability::declared`, run as argument lists, never a shell),
MCP (the handshake, resources, the stream, heartbeats), stop-all, git
status, evidence (with the `dry` binary when there is one) and `--app`.
Not in it: the **browser** capability (the standard library has no
WebSocket client for Chrome's DevTools protocol, so `browser` is absent
with that reason: use the Node companion for it), projects mode
(`--root`), custom tools (`tools/*.md`) and the `/v1/git/*` endpoints, all
outside the contract. Measured on an M4 Max: it is listening in about
90 ms (Node: about 350 ms) and idles at about 39 MB (Node: about 64 MB).

### The Rust companion

The `dry` binary's own: `dry companion` (crates/tools/src/companion/), so a
machine with dry needs neither Node nor Python. It keeps the same contract
(passes `conformance/`, 40 of 40) and starts the same way: the same
options (the table above, less the Node-only `--tools`, `--allow-url`,
`--dry`, `--root`), the folder confirmed, the same checks, numbered
switches saved in `.dry/companion.json`, the `listening` line, the code,
the live lines, and **Esc stops everything it runs** (a key at a time via
`stty` on macOS and Linux; an arrow key is not an Esc).

```sh
cargo build --release --bin dry
target/release/dry companion --folder ~/my-project
target/release/dry companion --folder ~/my-project --yes --on terminal --port 7777
cargo build --bin dry && node conformance/run.mjs --cmd "$PWD/target/debug/dry companion"   # the contract (an absolute
                                                                                           #   path: the suite starts it
                                                                                           #   from a temp folder)
cargo test -p dry-tools companion                                                          # its rules, pure
```

It is dry's own code wherever dry has it: declared capabilities are read
by `dry_core::capability::declared` and run by the tools runner
(`dry_tools::capability::run_argv`: argument lists, never a shell); the
secret rules are `dry_core::file::secret_file` and
`dry_core::terminal::secret_env`; a CLI provider runs through
`dry_tools::cli` (its `--version` probe and its spawn, with the protocol's
argv); **evidence runs in the process** (`dry_tools::evidence::run`, no
`dry` binary to find), and the **browser** is dry's DevTools client
(`dry_tools::cdp`) with the evidence runner's steps (open, look, click,
type, screenshot; only this machine's pages, a refused URL is `403
url_refused`). HTTP is std's `TcpListener`, a thread per connection, one
request per connection; no new crate. Not in it: projects mode (`--root`,
`/v1/projects`, `/v1/files/*`, `/v1/exec/*`), custom tools (`tools/*.md`,
`/v1/tools`) and the `/v1/git/*` endpoints, all outside the contract; on
Windows, Esc (no `stty`: type `stop-all`) and SIGTERM handling are not
there yet (its Windows code paths are written but not yet built or run). `stop-all` does not interrupt an
evidence run in progress (it ends by its own timeouts). Measured on an M4
Max: listening in about 80 ms (Node: about 340 ms), idles at about 5 MB
(Node: about 72 MB); it adds about 0.8 MB to the release `dry` binary
(8.1 MB without it, 9.0 MB with it).

### The app, served by the companion

Build the app for the site root (the `/` form of `scripts/web-build.sh`:
`trunk build --release --public-url /app/ --dist <dir>/app`, plus
`crates/web/landing/index.html` as `<dir>/index.html`), and give the folder
with `--app` (or put it at `dist/` beside `companion/`). Then open
**http://localhost:7777/app/**: the landing page is at `/`, the app at
`/app/`, with the right types (`application/wasm` and so on) and
`Cross-Origin-Opener-Policy: same-origin` / `Cross-Origin-Embedder-Policy:
require-corp`, so the page is cross-origin isolated without a service
worker. Paths outside the folder are refused.

A page served by the companion is **same origin** with it: no CORS, no
local network prompt, any browser. The page sees that it is served by a
companion and uses it as the default URL. Pairing still applies.

This is the way for **Safari** (which blocks an https page from calling
`http://127.0.0.1`), and for any browser that will not let the hosted page
reach the machine. From the hosted https page, Chrome asks once whether the
site may reach apps on your local network: allow it (docs/browser.md).

## What the page can do with it

In **Settings → Companion**: the URL, the code, **Connect**; then the
computer's name and runtimes, **Use its models**, the pairings (with
**Revoke**), **Stop all**, where the state is kept, and:

- **Folder**: the workspace folder, whether files and the terminal are
  switched on, and **Give the folder to an engine** (a new one or one you
  have): it gets `files` (`kind: file`, `root: .`) and `terminal` (`kind:
  terminal`, `root: .`), and the page opens its **live view**
  (`#/desk/<i>/live`): the explorer (the folder as a tree, open files
  bright, unsaved ones ●, secrets never listed), the editor tabs (each open
  file with its unsaved edits laid over, focus lines lit), the terminal
  panel (sessions, running first, each tail), the chat and the status
  strip. It is the desktop's live view (`dry_core::liveview`), drawn by
  the shared HTML renderer (`dry_html::render::live`).
- **Keep state in the folder**: the engines go to `<folder>/.dry/state/`
  (the same layout as dry's `state/`), and every save writes
  `.dry/snapshot.json` too (open files, focus, unsaved edits, sessions).
  This browser keeps a copy in IndexedDB (`dry-workspace`): while the
  companion is away the page works from it and marks it pending; pairing
  again sends the pending texts first, then loads from the folder. Pairing
  a fresh browser with a folder that already holds dry's state loads it at
  once. So: stop the companion and close the page, start both again, pair,
  and the engines, open files and unsaved edits are back.
- **Providers**: every entry of `.dry/providers.json`, ok (its models) or
  failed / absent (why, and the fix), **Probe again**, an **Add a
  provider** form (written into the file through the companion), and the
  template to paste from.

The engine's `file` and `terminal` artifacts reach the folder through the
companion's primitives (`/v1/fs/*`, `/v1/term/*`): dry's core does the
rest (`dry_core::workspace::Folder` is the `live::Host` over them), so the
open/focus/edit/save rules, the unsaved overlay, the snapshots, sessions
and callbacks are the desktop's own code.

- **Projects** (`#/projects`, projects mode): the folders under the roots
  that have a `.git` or a manifest, two levels deep. Open one: its tree, a
  file's text, a terminal (**Run**, **Start** with a callback,
  **Background**, **Stop**, **Stop all**), its **tools**, git (status,
  diff, commit all, pull, push…, clone…), and **Give it to an engine**
  (`files`, `terminal` and `browser` rooted at the project; an absolute
  root goes through `/v1/files` and `/v1/exec`).
- **Custom tools reach every engine** as outside tools, the way MCP tools
  do (`Engine::mcp_tools`, listed in the prompt): `companion.<name>`.
- **Models**: every model of every ok provider, `<provider>/<model>`,
  through the page's usual `Transport` (**Use its models** points Settings
  → Models at the companion and lists them).
- **Pairing survives a reload** (below), and so do callbacks.

In the plain browser (not connected) file, terminal and browser artifacts
are absent, saying why.

## Custom tools

Custom tools are written down as the tool and its metadata, not as code
(docs/developer-loop.md §3, "Where tools come from"). The companion reads
`tools/*.json` and `tools/*.md` in **each project** (named
`<project>.<name>`, run in the project) and in **`--tools DIR`** (named
`<name>`, run in that folder).

```json
{
  "name": "greet",
  "description": "Greet someone the way hello-app does: one line back.",
  "inputs": [{ "name": "who", "type": "string", "required": true, "description": "who to greet" }],
  "command": ["node", "tools/greet.js", "{who}"],
  "timeout": "10s"
}
```

```markdown
# greet
Greet someone the way hello-app does: one line back.

command: node tools/greet.js {who}
timeout: 10s
inputs:
- who (string, required): who to greet
```

- **Inputs** have a name, a type (`string`, `integer`, `number`,
  `boolean`), whether they are required, and a description. A call is
  checked: unknown inputs, missing required ones, a wrong type, a zero byte
  or more than 64 KB are refused (400).
- **The command** is an argument list, the program first (in the `.md`
  form, split on spaces; use the JSON array form for arguments with
  spaces). It runs with no shell (`spawn(program, args)`), in the project,
  with the secret-looking environment variables left out.
- **Inputs go in two ways, never by string interpolation:** an argument
  that is exactly `{name}` is replaced by that input's value as one whole
  argument (left out when an optional input is not given), and every given
  input is in the environment as `DRY_INPUT_<NAME>`. A value like
  `Ada; touch /tmp/x` or `$(…)` stays one argument. An argument with a
  placeholder inside it (`"hi {who}"`) is refused, and so is a placeholder
  right after a code flag (`sh -c {x}`, `node -e {x}`, `python -c`…): such
  a tool does not load, and the reason is listed (`tool_errors`, the
  Projects view, `tools` in the terminal).
- `timeout` (default 60 s): the tool's process group is killed after it.

`GET /v1/tools` lists them (with their JSON Schema), `POST /v1/tools/call
{name, arguments}` runs one: `{ok, exit, output, timed_out, ms}`. They are
also in `/v1/capabilities` and at `/mcp`.

## MCP

`/mcp` is an MCP server over **streamable HTTP**, with the same tools
(custom tools, and `browser_open`, `browser_look`, `browser_click`,
`browser_type`, `browser_screenshot`; the screenshot comes back as an image
too). The pairing token is the bearer token:

```sh
claude mcp add --transport http dry http://localhost:7777/mcp --header "Authorization: Bearer <token>"
```

It follows the spec of 2026-07-28 (checked 2026-11-23) and serves older
clients too (dual-era):

- **Modern** requests carry `_meta["io.modelcontextprotocol/protocolVersion"]`
  (`2026-07-28`) and are stateless; `MCP-Protocol-Version`, `Mcp-Method`
  and (for `tools/call`) `Mcp-Name` must match the body, else
  `400` with `HeaderMismatch` (-32020); an unknown version gets
  `UnsupportedProtocolVersionError` (-32022, with the supported list); an
  unknown method `404` with -32601. `server/discover` lists the versions.
- **Legacy** clients (`2025-11-25`, `2025-06-18`, `2025-03-26`) send
  `initialize`; the version they ask for is answered when supported.
- Notifications get `202`; `GET` and `DELETE` get `405` (no standalone
  stream, no sessions); answers are `application/json`.
- The `Origin` header is checked as for every call (403), and the token is
  required (401 with `WWW-Authenticate: Bearer`).

Checked with Claude Code: `claude -p --mcp-config` with this server called
`hello-app.greet` and printed its line.

## Capabilities (docs/capabilities.md)

The companion is a **provider**: everything it does is described as
capabilities over its MCP server, so a host, the web app or another
device joins it the same way (the handshake, docs/capabilities.md §4).

- **Its own:** `terminal` (`terminal.run(command, cwd?)` in the folder,
  needs approval, an actuator: off until switched on), `file`
  (`file.list(path?)`, `file.read(path)` in the folder), `browser` (the
  `browser_*` tools), `evidence` (`evidence_run`, `evidence_read`), `git`
  (`git.status`), `container` (present with docker or podman; snapshot
  `ps`), and one `model.<name>` per declared provider (its snapshot: its
  models; absent with why and the fix when the provider is not ok). Each is
  absent with why where the machine lacks it (no Chrome, no dry binary, no
  git), or where the person switched it off.
- **Declared:** every `<id>.md` in `--capabilities` (the repo's
  `capabilities/` by default), probed every 30 s; a file that does not read
  is listed with its line (`/v1/capabilities` → `capabilities.errors`).
  A declared tool runs its argv with no shell, the inputs as whole
  arguments and `DRY_INPUT_<NAME>`, in the capabilities folder.
- **MCP:** `initialize` answers `_meta.hello` and keeps the peer's hello on
  its pairing (`pairings` shows it); `tools/list` carries
  `_meta.capability` (with doc, state, approval) on every capability's
  tool; `resources/list` has one `dry://capability/<id>` per capability,
  `resources/read` its snapshot now; `GET /mcp` with `Accept:
  text/event-stream` is the peer's stream: while one listens, the declared
  snapshots are read at their intervals, an event that becomes true rides
  on `notifications/resources/updated` (`_meta.event`), and a changed
  folder or probe is `notifications/tools/list_changed`. A `: ping` comment
  every 15 s keeps it open.

## The browser (UI evidence)

The machine's own Chrome (or Chromium, Edge, Brave), headless, driven over
the DevTools protocol: started on first use with
`--remote-debugging-port=0` and a temporary profile, reached over Node 22's
global `WebSocket` (no Playwright, no download). On older Node the
capability says it needs Node 22 and the rest of the companion works.

| Call | Body | Answer |
|---|---|---|
| `POST /v1/browser/open` | `{url, page?}` | the page (below), with a screenshot |
| `POST /v1/browser/look` | `{page?}` | the page now (no new screenshot) |
| `POST /v1/browser/click` | `{target, page?}` | clicks a CSS selector or the element showing that text; `did` says what |
| `POST /v1/browser/type` | `{target, text, press?, page?}` | types into a field (a selector, its label, placeholder or name); `press`: a key after (`Enter`) |
| `POST /v1/browser/screenshot` | `{page?}` | `{screenshot, jpeg}` |
| `POST /v1/browser/close` | `{page?}` | closes that page |

The page is `{url, title, text, errors, screenshot, jpeg, did}`: `text` is
its accessibility tree as lines (`heading "Todo"`, `button "Add"`,
`textbox "new item" = "milk"`), `errors` its console errors and uncaught
exceptions, `screenshot` a PNG in a temporary folder
(`$TMPDIR/dry-companion-shots/`), `jpeg` a small JPEG (0.4 scale) as
base64. `page` names a tab: each browser artifact has its own
(`<engine>/<artifact>`).

**Only this machine's pages**: `http(s)` on `localhost`, `127.0.0.1`,
`[::1]` or `*.localhost`, and the prefixes given with `--allow-url`. Other
URLs (other hosts, `file:`, `javascript:`…) get 403; after every load,
click and type the page's address is checked again, and a page that went
elsewhere is sent to `about:blank` with an error. (What a local page loads
from elsewhere, an image or a script, is not blocked.)

**The `browser` artifact kind** (`dry_core::browser`, docs/developer-loop.md
§2): `kind: browser`, its state the URL and the last screenshot's path; its
tools `browse(url)`, `click(target)`, `type(target, text, key?)`; its
snapshot, read again at every render, the page's text (cut at 6 000
characters), its console errors and the screenshot's path. So an engine's
test phase starts its app (`background`), browses it, clicks, and reads
what it shows, with the errors. A shell without a browser says so
(`Host::browser`'s default).

## Evidence (docs/evidence.md)

A project with an `evidence.md` can have its evidence run from the page:
the companion runs the project's `dry evidence` (the `dry` binary from
`--dry PATH`, `DRY_BIN`, this checkout's `target/release` or
`target/debug`, else `PATH`) and waits for it. One implementation, the
Rust runner, drives Chrome and diffs; the companion only starts it, reads
its folder and serves its images. `stop-all` stops a run.

| Call | Body / query | Answer |
|---|---|---|
| `POST /v1/evidence/run` | `{project, expect?, update?}` | `{line, summary, manifest, code, output, ms}` (`code` 1: an unexpected change) |
| `GET /v1/evidence` | `?project=` | the last run: `{line, summary, manifest}` |
| `GET /v1/evidence/file` | `?project=&name=home@1280x800.png` | one PNG of its `evidence/` (a shot, `.new` or `.diff`) |

MCP: `evidence_run` and `evidence_read` (the line, then the summary).

## The protocol (v1)

The contract every companion keeps (health, pairing, origins, the
folder's `/v1/workspace`, `/v1/fs/*` and `/v1/term/*`, the store, the
snapshot, providers, MCP, switches, stop-all) is docs/companion-protocol.md,
with its error codes. The table here is everything this Node companion
answers, the contract's calls and its own (projects mode, git, the browser,
evidence, custom tools). JSON in and out. Every call but `/v1/health`,
`/v1/pair` and the app's files needs the session token, as `x-dry-token:
<token>` or `Authorization: Bearer <token>`. An error is `{"error": "why",
"code": "path_outside"}` with a 4xx/5xx status. The pure readers of these
answers are `dry_core::companion`, `dry_core::workspace` (and
`dry_core::browser`).

| Call | Body / query | Answer |
|---|---|---|
| `GET /`, `GET /app/…` | | the app's files (`--app`) |
| `GET /v1/health` | | `{ok, name, version, protocol, paired, app}` |
| `POST /v1/pair` | `{code}` | `{token, id, expires, ttl}`; 401 for a wrong code |
| `GET /v1/workspace`, `POST /v1/workspace/snapshot` | `{snapshot}` | the folder, its switches, `.dry/snapshot.json` (protocol §6–7) |
| `GET /v1/fs/list \| read \| stat`, `POST /v1/fs/write` | `?path=` / `{path, text}` | the folder's files (protocol §6) |
| `POST /v1/term/run \| start \| stop`, `GET /v1/term/tail` | `{key, command, cwd?}` / `?key=&lines=` | commands in the folder, by key (protocol §6) |
| `GET /v1/providers`, `POST /v1/providers` | `?refresh=1` / `{provider}` | the declared providers, probed; add one (protocol §8) |
| `GET /v1/switches` | | every capability, on or off |
| `POST /v1/unpair` | | this token stops working |
| `GET /v1/pairings` | | `{pairings: [{id, origin, created, expires, seen, current}]}` |
| `POST /v1/pairings/revoke` | `{id}` | that pairing stops working |
| `POST /v1/stop-all` | | `{stopped, browser}`: every running command stopped, the browser closed |
| `GET /v1/capabilities` | `?refresh=1` | `{version, protocol, platform, hostname, roots, runtimes, providers, tools, tool_errors, browser, app, mcp, events}` |
| `GET /v1/projects` | | `{roots, projects: [{name, path, root, markers, git, branch}]}` |
| `GET /v1/files/tree` | `?path=DIR` | `{path, project, entries: [{name, path, dir, size, link}]}` |
| `GET /v1/files/list` | `?root=DIR&max=N` | `{files}` (as `Host::list`) |
| `GET /v1/files/read` | `?path=FILE&within=DIR` | `{kind: text \| binary \| too_large \| missing, …}` |
| `POST /v1/files/write` | `{path, text, within?}` | `{ok, path, bytes}` |
| `POST /v1/exec/run` | `{cwd, command, timeout_ms?, key?}` | waits: `{id, status, exit, signal, timed_out, output, ms}` |
| `POST /v1/exec/start` | `{cwd, command, mode: start \| background, key?, timeout_ms?}` | at once: the session |
| `GET /v1/exec/sessions` | | `{sessions}` |
| `GET /v1/exec/session` | `?id=` or `?key=`, `&tail=N` | the session with its last N lines |
| `POST /v1/exec/stop` | `{id}` or `{key}` | SIGTERM to its process group, SIGKILL 2 s later |
| `GET /v1/events` | `?since=ID` (or `Last-Event-ID`), `?id=`, `?all=1` | server-sent events, each with an `id:`: `hello`, `exit`, `output` |
| `GET /v1/events/log` | `?since=ID` | `{events: [{id, event, data, at}], last}` |
| `GET /v1/tools`, `POST /v1/tools/call` | `{name, arguments}` | custom tools (above) |
| `POST /v1/browser/<op>` | | the browser (above) |
| `POST /v1/evidence/run`, `GET /v1/evidence`, `GET /v1/evidence/file` | | a project's evidence (above) |
| `POST /mcp` | JSON-RPC | MCP (above): tools, `resources/list`, `resources/read`, the hello in `initialize` |
| `GET /mcp` | `Accept: text/event-stream` | a peer's stream of notifications (capabilities, above); without that Accept, 405 |
| `POST /v1/git/status \| diff \| branch \| commit \| pull` | `{path, …}` | git, as before |
| `POST /v1/git/push`, `POST /v1/git/clone` | `{…, confirmed: true}` | 428 without `confirmed` |
| `GET /v1/models`, `GET /api/v0/models` | | every model |
| `POST /v1/chat/completions` | OpenAI's | OpenAI's |
| `GET /v1/store`, `POST /v1/store` | `{put, remove}` | the state's texts, as files under `--store` |

`key` names a session the way the page's terminal artifacts do
(`<engine>/<artifact>/<session>`), so a session's status survives a page
reload. Sessions live in the companion's memory: a restarted companion has
none, and the engine's terminal reads them as stopped.

### The event log, and callbacks across a reload

The companion keeps its last 1000 events (every command's exit, with the
session) with increasing ids. A start's exit goes to every stream (it is a
callback); a run's or a background's only to a stream for that session or
one opened with `?all=1`. The page remembers the last id it saw (in the
tab's sessionStorage) and opens the stream with `?since=<id>`: whatever
ended while the tab was reloading, or away on another page, comes first,
and runs the engines' callbacks at once. (The page's 2 s tick would also
find the ended session by its key; the log makes it immediate and works
even when the page was gone for the whole run.)

## Security

This is a server on your computer that a public web page talks to, so:

- **Loopback only.** It binds `127.0.0.1`. The `Host` header must be
  `127.0.0.1:<port>`, `localhost:<port>` or `[::1]:<port>` (DNS
  rebinding is refused).
- **Origins.** A request with an `Origin` is answered only for
  `https://kaush4l.github.io`, `http://localhost[:port]`,
  `http://127.0.0.1[:port]`, `http://[::1][:port]` and each `--origin`.
  CORS preflights get that origin (never `*`), the methods, the headers
  and `Access-Control-Allow-Private-Network: true`. A page the companion
  serves is same origin.
- **Pairing.** At start it prints a 6-character code; a page sends it once
  to `/v1/pair` and gets a 256-bit token. The code works once (a new one is
  printed); five wrong codes change it. A pairing **expires** (12 h, or
  `--ttl`) and can be **revoked** (the terminal's `revoke <id>`,
  `/v1/pairings/revoke`, or the page's **Revoke**). The companion keeps
  tokens in memory only. The page keeps its token in the **tab's
  sessionStorage** (with its expiry): a reload keeps the pairing, closing
  the tab ends it, and it is never in IndexedDB, localStorage or a URL.
  (sessionStorage is readable by scripts on the same origin; the app runs
  no third-party scripts.)
- **Paths.** Every path is resolved to its real path and must be inside a
  root; links that lead out are refused and not listed; secret-looking
  names are never read, written or listed; nothing is written inside
  `.git` except by git. The app's files are confined to `--app` the same way.
- **Commands.** `exec` runs the command with `/bin/sh -c`: only after
  pairing, only in a project under a root, with no input and without the
  secret-looking environment variables. A timeout kills the process group.
  **Stop all** (`/v1/stop-all`, the button, `stop-all` in the terminal)
  kills every running command and closes the browser.
- **Custom tools** never go through a shell, and an input is only ever one
  whole argument or an environment variable (above).
- **The browser** opens only this machine's pages unless `--allow-url`,
  checks the address after every action, uses a throwaway profile, and is
  closed by Stop all and when the companion stops.
- **Git** runs with argument arrays, checked names, `--` before a URL, and
  the machine's own credentials; the browser never sees a token.
- **Inference** runs the CLIs with argument arrays, in a temporary folder,
  with no tools.

### Confirmation (the interim, until the approval system exists)

**Push and clone** are allowed only with `"confirmed": true`, which the page
sends only after a person's click on **Push…** / **Clone…** and the confirm
dialog. `exec` refuses `git push`, `git clone`, `gh repo create|clone|delete|fork`,
`gh pr create|merge` and `gh release create` (428). That check reads the
command text: a guard against an engine wandering into a publish, not a
sandbox. There is no approval step for other commands yet, as in the
desktop app.

## Providers (`providers.json`, docs/companion-protocol.md §8)

Declared, never scanned: the companion probes only the entries of
`.dry/providers.json` (made from the repo's `providers.json` template on
the first run; paste entries from its `examples`), so the first run is
instant. Each entry is ok, failed or absent in the checks, in
`providers`, in the page and in `GET /v1/providers`; each is a `model.<name>`
capability over MCP, and its models are `<name>/<model>` in `/v1/models`
and the model picker.

| kind | entry | probe | chat |
|---|---|---|---|
| `cli` | `{"name": "claude", "kind": "cli", "cli": "claude", "path"?, "model"?}` | `<program> --version` | `claude -p --output-format json --no-session-persistence --tools "" --strict-mcp-config [--model m] [--system-prompt …]`, the prompt on stdin; `codex exec --color never --sandbox read-only --skip-git-repo-check --ephemeral -o FILE [--model m] -`; `copilot -p <prompt> -s --no-color --available-tools ""` |
| `port` | `{"name": "lmstudio", "kind": "port", "url": "http://127.0.0.1:1234"}` (LM Studio, Ollama, vLLM, a forwarded port) | `GET <url>/v1/models` (3 s) | passed through and streamed back |
| `native` | `{"name": "local", "kind": "native", "runner": "auto", "model": "…", "port"?}` | the runner (`mlx_lm.server` on Apple silicon, `llama-server` elsewhere, CUDA when there is an `nvidia-smi`) and the model on disk; then it starts the runner on a local port and waits for its `/v1/models` | as a port |

It never installs or downloads: a missing program is `absent` with its
install hint (`pip install mlx-lm`, `brew install llama.cpp`, `npm install
-g @openai/codex`…), a model not on disk is `failed` with the command the
person may run. A runner it started stops with it. A CLI takes one prompt,
so a chat is flattened (system messages as the system prompt, a single
user message as it is, a longer chat as a transcript); `stream: true` gets
the whole answer as one chunk.

## How it relates to the rest

- **The remote host (later).** The same protocol, served by dry-server on
  another machine, gives a phone or another computer the host's projects,
  tools and models. `dry_core::companion` holds the pure parts.
- **The in-browser container** (docs/container.md, `dry_core::container`):
  `BrowserVm` (Alpine in v86, in the page), `Local` (docker / podman on
  this computer) and `Remote` (the host's). The companion is the `Local`
  backend's transport: its exec is `exec` and `start`-with-callback, and
  `docker` / `podman` are in its capabilities.

## Tests

```sh
node conformance/run.mjs --cmd "$PWD/target/debug/dry companion"   # the same contract, the Rust companion
node conformance/run.mjs               # the wire contract (docs/companion-protocol.md §13), from outside: pairing,
                                       #   origins, the folder's primitives, the terminal, MCP, providers
node --test 'companion/*.test.mjs'     # dry-companion.test.mjs: origins, preflight, pairing, paths, exec, git, store, inference
                                       # dry-companion-more.test.mjs: the app served (types, confinement), pairings
                                       #   (list, revoke, expiry), the event log and resume, custom tools (definitions
                                       #   refused, injection attempts stay one argument, types, timeouts), MCP (legacy and
                                       #   modern, header checks, versions, 405, Origin), the browser URL allowlist, the
                                       #   browser with the machine's Chrome (open, text, click, type, errors, screenshot;
                                       #   skipped without Chrome), stop-all
                                       # dry-companion-capabilities.test.mjs: the parser (as dry_core's), argv safety,
                                       #   the handshake (hello, _meta grouping, resources), a declared call, the stream
                                       # dry-companion-workspace.test.mjs: folder paths, providers.json entries, the
                                       #   runner by hardware (never a download flag), the command line, switches saved
cargo test -p dry-core workspace companion browser   # the folder as a Host (file and terminal artifacts over the
                                       #   primitives), the snapshot and its restore; the protocol's readers
```

## Not yet

- In the Rust companion: projects mode, custom tools, `/v1/git/*`, and
  Esc and SIGTERM handling on Windows.
- The browser capability in the Python companion.
- The live view reads the folder over synchronous requests at each
  render; a change made outside the page shows at the next one.
- An approval system in place of the confirm dialog.
- Streaming a model's answer token by token from the CLIs.
- Images in snapshots (the screenshot is a path until vision is standard).
- The browser artifact's calls wait synchronously in the page (a page load
  takes a second or two); a terminal `run` and custom tools are async.
