# Status

A snapshot of dry-learn on 2026-11-23, compiled from a fresh read of
`docs/`, the crates, `git log` and `git status`, and the test runs below.
Other agents were editing `crates/html`, `crates/server`,
`crates/core/src/scene`, `scenes/` and `themes/` at the time, so those
areas may have moved since.

Marks:

- ✅ built and verified: a test passes, or a doc records a verification run.
- 🟡 built, not verified: the code is there; what is missing is said.
- ❌ planned only.

## What dry is

dry is a personal app built around **engines**: agent loops that each
have a spec (goal, template, phases, skills, tools), a conversation,
artifacts that show what is true now, and a model. An engine runs a turn
step by step until it writes `reply`, with no step limit; the model's
context window is the only limit. The same core (`dry-core`, pure: no UI,
disk or network) runs in a native GPUI window, on a host server, and in
the browser as WebAssembly. Everything an engine can touch (files, a
terminal, a browser, a battery, a model) is a **capability**: compiled
into a build, declared by a machine in a file, or provided by a peer that
joined over MCP ([capabilities.md](capabilities.md)).

The topology (owner, 2026-11-23):

| Piece | What it is | Today |
|---|---|---|
| Live server | runs on any hardware, holds the state, runs the turns, and has its own default UI where it runs | ✅ `dry-server` is the host and serves its own HTML UI ([web.md, The send path (built)](web.md#the-send-path-built)) |
| Shell UIs | connect to a live server and stream its state and updates (web, GPUI, terminal, phone) | 🟡 the server's own pages stream over SSE; the GPUI window follows a host only on the same machine (it reads the lock file beside the store); no terminal or phone shell |
| Server to server | two live servers exchange messages, start sub-agents on each other, share a thread | ❌ not built; `dry-server` serves no `/mcp`, so it cannot be another server's peer |
| Code-server mode (the owner calls this the most important) | the app runs as a service behind a server, one server per hosting; the UI is streamed to the browser | ✅ proved on 2026-11-23 from a Linux container and a fresh browser profile ([server.md](server.md)): token required (401), pages, `/events` live, turns on the host, asks answered remotely, a second tab updated without a reload. Every former `HOST_LATER` control is a host intent (theme, routes, Make default, reloads, the Code page's terminal). launchd and systemd files in `scripts/service/`. On macOS the application firewall blocks a new unsigned binary until allowed (the proof went through a relay, with `--strict`) |
| Companions | small local servers that join by the capability handshake (pair → hello → manifest → stream) | ✅ three companions, one contract, 40 of 40 conformance each |
| The wasm build | the whole app in the browser, state in IndexedDB, models over `fetch`; a companion gives it a folder and a terminal | ✅ published on GitHub Pages ([browser.md](browser.md)) |

## Current state by area

| Area | Mark | Evidence, and what is missing |
|---|---|---|
| Engines and the turn loop | ✅ | `core/turn.rs`, `core/runner.rs`. The async runner equals the blocking turn byte for byte (`runner` tests). One step, many calls: [`tools/tests/one_step_many_calls.rs`](../crates/tools/tests/one_step_many_calls.rs) (9 pass). Context window and compression at 75%: `core/context.rs` tests; real runs in ignored `gpui/src/net.rs` tests ([engine-standard.md, The context window](engine-standard.md#3b-the-context-window-the-only-limit)) |
| Phases and templates | ✅ | `core/phase.rs`, `core/template.rs`, fake-model tests; marked done in [engine-standard.md, Order of work](engine-standard.md#5-order-of-work). `jev` phases are accepted but answered the standard way (planned) |
| A2UI asks and displays | ✅ | `core/ask.rs` (11 tests), `core/show.rs`; drawn in GPUI and HTML. Measured on the owner's LM Studio model, 2026-11-23: every named situation 4/4 ([engine-standard.md, Rules for both](engine-standard.md#rules-for-both)) |
| Streaming parser | ✅ | `core/stream.rs`: a property test splits every example at every byte and checks the events equal `calls::parse` ([engine-standard.md, Streaming replies](engine-standard.md#4e-streaming-replies-built-corestreamrs-coreearlyrs)). Measured on LM Studio, native and browser. `early.rs` notes its engine copy is "not yet true" |
| Host split and SSE | ✅ | [`crates/server/tests/host.rs`](../crates/server/tests/host.rs) (5 pass): a send is taken, events come with rising ids, `Last-Event-ID` resumes, 409 for busy and stale, 415, 403, `--bind` without a token refuses. Gaps: the host starts no MCP servers; the window's other edits do not go through the host |
| Claude sessions as workers | ✅ | [`crates/server/tests/workers.rs`](../crates/server/tests/workers.rs) against the real host binary; a browser check on a scratch host with `claude` on haiku ([sessions.md, Checked](sessions.md#checked)). Approvals live in host memory only; the `/` list is not in GPUI |
| CLI models (claude, codex, copilot) | ✅ | `core/cli.rs` (10 tests), `tools/cli.rs`; measured 2026-11-23: claude/haiku 2.2–3.8 s per turn ([engine-standard.md, CLI models](engine-standard.md#cli-models-built-coreclirs-toolsclirs)). Not in a browser page (a page cannot start a program) |
| Evidence | ✅ | `core/evidence` (24 tests); [`evidence/summary.md`](../evidence/summary.md): 10 shots, 0 changed, pass. Not yet: native (GPUI) shots, full-page shots, images in the browser build ([evidence.md, Not yet](evidence.md#8-not-yet)) |
| The shipped default (translator + developer) | ✅ | [default.md](default.md): `default/` is what a fresh state starts with (host, window, `dry`, the web build). `core/shipped.rs`, `core/kit.rs` tests, `engines` test for a child from a template; proved on a scratch project with claude/sonnet (triage → code → test, judged report). Skills from [skills-survey.md](skills-survey.md). With claude/haiku the loop runs but skips phases and invents facts |
| The developer loop on a local model | ✅ | [loop/journal.md](loop/journal.md): Inkling-2-Mini on oMLX, natively and in the browser page; 25 answers and 4 slips before, 12–15 and 0–1 after (`docs/loop/runs/`). A skill can bring tools (`core/skilltool.rs`; the developer's `python`). Open: nothing starts work without a message; in the page the developer cannot look at a page it made |
| Analytics | 🟡 | `core/spans.rs` (7 tests), `core/analytics.rs` (8 tests); views in HTML and GPUI. No end-to-end verification note in [analytics.md](analytics.md). Missing: the browser's `analytics` tool, GPUI session traces, evidence spans, schedule triggers |
| Capabilities: compiled | ✅ | `core/capability` (16 tests): registry, `effective`, self-injection, `kind:` aliases, `uses:` |
| Capabilities: declared | ✅ | the parser, argv safety, edge-triggered events; `dry-tools capability` tests; `capabilities/battery.md` works on this Mac; `gpio.md` and `sensors.md` probe absent with why |
| Capabilities: remote (peers) | ✅ | `cargo test -p dry-server a_peer`, plus the end-to-end run of 2026-11-23: a companion's `battery` used by a host engine, then absent when it left ([capabilities.md, Tests and the end-to-end proof](capabilities.md#7-tests-and-the-end-to-end-proof)). The GPUI window does not join peers |
| Capabilities: browser runtime | 🟡 | `crates/web/src/caps.rs` (folder, transcribe, `model.local`, camera, screen, notify). Seen in Settings → Capabilities in the end-to-end run, but no automated test of the probes. Camera and screen frames are not images in the prompt yet |
| Companion protocol and conformance | ✅ | Node, Python and Rust each pass `node conformance/run.mjs`, 40 of 40 (run for this page). The Python and Rust companions are uncommitted (see Known gaps) |
| Folder workspace | ✅ | `core/workspace.rs` (6 tests): `Folder` is the `live::Host` over the companion's primitives; proved end to end on 2026-11-23 (restart, fresh browser, snapshot restore) ([web.md, The browser app and a folder on the person's computer](web.md#the-browser-app-and-a-folder-on-the-persons-computer)). The live view reads the folder by synchronous requests at each render |
| `providers.json` | ✅ | the template at the repo root; `conformance/providers.test.mjs` covers port, cli, native, add and 409, for all three companions |
| Browser container (v86) | ✅ | the page's environment: Alpine in v86 behind the `file` and `terminal` artifacts (`dry_web::env`, [container.md](container.md)). Proved 2026-11-23 with a local model: python installed by the engine, a script written, run and reported, all in the page ([loop/journal.md](loop/journal.md) §9). 32-bit only; no git; a `run`'s output comes when it ends |
| Web app (wasm) | ✅ | `crates/web`; `idb.rs` tests run in a real browser by hand ([browser.md, Tests](browser.md#tests)); evidence shoots landing, Home, an engine, Settings and the container. The remote-host client (`TextStore` and `Transport` over HTTP) is not built |
| GPUI | 🟡 | `crates/gpui` (1 test runs, 11 ignored need a local model). Its real-model checks are recorded in docs, but there are no screenshots of it (evidence needs a bundled app). Voice recording is not wired |
| Scenes and the cat | 🟡 | `core/scene` (24 tests, fake clock); the cat, terminal and abyss scenes draw in GPUI with mood and pointer. In the browser the canvas draws the scene, but the code passes a fixed `Mood::Idle` and no pointer, so the cat does not react there. The server's HTML draws no scene |
| Themes | 🟡 | `core/theme.rs` (12 tests); `default`, `cat`, `terminal`, `abyss`, `bolt` (flat; the live view wears it always). Editing themes from a host page or the browser is off. Motion tokens beyond `motion` are not there |
| Pages publishing | ✅ | [`.github/workflows/pages.yml`](../.github/workflows/pages.yml); the run of 2026-11-23 04:14 UTC passed and Pages reports `built` at https://kaush4l.github.io/dry/. It was built from `origin/main` (`b2baf24`), not this working tree |

## Parity tables

### Across the shells

GPUI = the native window. Web/wasm = the app in the browser (Pages or
served by a companion). Hosted HTML = the pages `dry-server` draws.

| Feature | GPUI | Web/wasm | Hosted HTML |
|---|---|---|---|
| Runs turns | ✅ when no host runs | ✅ in the page | ✅ the host |
| Store | md or SQLite on disk | IndexedDB; or the folder through a companion | md or SQLite on disk (one writer) |
| Follows a host | 🟡 same machine only (lock file) | ❌ the remote-host client is not built | it is the host |
| Live updates | 1 s file watch | its own runner | ✅ `/events` SSE with resume |
| Streaming replies | ✅ | ✅ (`fetch` body stream) | ✅ `reply` events |
| Asks and displays | ✅ | ✅ | ✅ |
| `file` and `terminal` artifacts | ✅ the disk | ✅ only paired with a companion | ✅ the host's disk |
| Live view (explorer, editor, panel) | ✅ | ✅ | ✅ |
| `browser` artifact | the companion's | the companion's | the companion's |
| MCP servers | ✅ starts them | ❌ none | ❌ the host starts none |
| Session engines (claude) | 🟡 runs them, no live rows | ❌ refused | ✅ live tool rows, `/` list |
| Approvals (Allow / Deny) | ❌ no UI | ❌ | ✅ |
| Capability peers | ❌ own compiled and declared only | ✅ the paired companion | ✅ Join a peer / Leave |
| Browser capabilities (camera, Prompt API, folder) | n/a | ✅ | n/a |
| Analytics views | ✅ | ✅ (no `analytics` tool) | ✅ |
| Scene | ✅ cat with mood and pointer | ✅ mood and pointer, standing on the footer | ✅ the core's scene code as `scene.wasm`: mood, pointer, on the footer |
| Theme and route editing | ✅ | ❌ refused | ✅ theme, Reset all, routes (hide, off, delete, create); one token's value is still the desktop's |
| Code page | ✅ (terminal panel not bound to an artifact) | read-only bundled files | ✅ typed terminal on the host, shared by every page |
| Pane edge dragging | ✅ | ❌ | ❌ |
| Voice | ❌ checked, recording not wired | 🟡 `transcribe` via Web Speech | ❌ "not checked" |
| Container (v86) | ❌ | 🟡 `#/container` | ❌ |
| Evidence shots of itself | ❌ needs a bundled app | ✅ | ❌ |

### Across the three companions

From [companion.md](companion.md) and [companion-protocol.md](companion-protocol.md),
checked by running the suite against each.

| Area | Node | Python | Rust (`dry companion`) |
|---|---|---|---|
| Conformance (pairing, origins, workspace, terminal, MCP, providers) | ✅ 40/40 | ✅ 40/40 | ✅ 40/40 |
| Own unit tests | ✅ 33 (`node --test`) | ✅ 10 (`unittest`) | ✅ 7 in `tools/src/companion` |
| Declared capabilities, switches, stop-all | ✅ | ✅ | ✅ (dry's own parser and runner) |
| Providers: cli, port, native | ✅ | ✅ | ✅ |
| `browser` capability | ✅ (Node 22+) | ❌ no WebSocket in the standard library | ✅ `dry_tools::cdp` |
| Evidence | ✅ runs the `dry` binary | ✅ with the `dry` binary | ✅ in the process |
| Serves the built app (`--app`) | ✅ | ✅ | ✅ |
| Projects mode (`--root`, `/v1/files`, `/v1/exec`) | ✅ | ❌ | ❌ |
| Custom tools (`tools/*.md`) | ✅ | ❌ | ❌ |
| `/v1/git/*` | ✅ | ❌ | ❌ |
| Windows | 🟡 not tested | 🟡 Esc via msvcrt, not tested | ❌ Windows paths written, not built or run |
| Start time, idle memory (M4 Max) | ~340 ms, ~64–72 MB | ~90 ms, ~39 MB | ~80 ms, ~5 MB |

Projects mode, custom tools and git are outside the contract
([companion-protocol.md, What is not in the contract](companion-protocol.md#14-what-is-not-in-the-contract)).

### dry-learn against the old dry

Checked 2026-11-23 against old `main` (`31b2e5d`) from its code, with the
shared flows run in both apps on scratch copies: full table, run results,
screenshots and the port order in [parity.md](parity.md).

- Of 115 old features: 37 ✅ done, 34 🟡 partial, 38 ❌ missing, 6 ⛔ out
  (trading, start scripts and services, the desktop webview). 26 things are
  new in dry-learn.
- Run in both with the `claude` CLI on haiku: create an engine, a turn, an
  ask answered in the page (an approval in the old app, an A2UI choice
  here), the engine page and live view, Settings and a theme switch. All
  passed; here the live view had nothing to show, because `write` makes a
  text artifact, not a file.
- Port first, smallest first: alerts firing on the host tick; approval for
  an engine's `write`, `edit` and `terminal` (plus file `delete`); the day
  file and transcript ([parity.md, Gaps, in the order to port them](parity.md#gaps-in-the-order-to-port-them)).

## Planned and queued

In order. Each line: what, then where it comes from.

1. **Apple Swift helper capabilities**: Speech, on-device Foundation
   Models, Vision as capabilities, through a small Swift helper for a Mac
   companion. Source: owner decision 2026-11-23 (mobile and platform
   APIs); not in `docs/` yet.
2. **Server-to-server collaboration**: live servers exchange messages,
   start sub-agents on each other, share a thread. Source: owner
   topology 2026-11-23; the threads it reuses are in [sessions.md, Threads](sessions.md#4-threads-lists-of-messages).
3. **Sources and override layers**: a source (which live server the UI
   shows) between the app and the desk, several at once; overrides
   cascade shipped default → server → device, nearest wins. Source: owner
   decision 2026-11-23; related open question 5 in [port-draft.md, Open questions](port-draft.md#open-questions).
4. **Provider choice per capability at run time**: today the registry
   picks compiled, then declared, then remote, in a fixed order
   ([capabilities.md, The model](capabilities.md#1-the-model)). Source: owner topology 2026-11-23.
5. **Shell UI attach (a terminal shell)**: a terminal renderer that
   attaches to a live server. Source: owner topology 2026-11-23; feature C5
   in [parity-and-features.md, Service and clients](parity-and-features.md#c-service-and-clients); [scenes.md, Later](scenes.md#later) (cells as half blocks).
6. **Download page, release workflow and connect stepper**: per-platform
   builds of all three companions from CI, picked on the landing page,
   then a stepper in the web UI. Source: owner decision 2026-11-23; not in
   `docs/` yet. Publishing a release is the owner's step.
7. **Mobile builds with platform capabilities**. Source: owner decision
   2026-11-23; [browser.md, Not yet](browser.md#not-yet) ("a mobile shell").
8. **A person-approval step for actuators**, like the sessions' records.
   Source: [capabilities.md, Not yet](capabilities.md#not-yet); [companion.md, Not yet](companion.md#not-yet).
9. **Live-view parts from the capability registry**: today `Live::view`
   attaches parts per kind ([engine-standard.md, Artifacts](engine-standard.md#4-artifacts-things-that-add-to-several-slots)). Source: owner
   standard 2026-11-23 ("a capability can expose tools, a snapshot and a
   view").
10. **Cleanup of § section numbers and numbered headings**: `§` appears
    10 times in [companion.md](companion.md), 19 times in
    [companion-protocol.md](companion-protocol.md), and once in the
    `about` text of [`providers.json`](../providers.json). Numbered headings
    remain in capabilities, companion-protocol, developer-loop,
    engine-standard, evidence, instructions-and-learning,
    parity-and-features and sessions. Source: the owner's rule for
    heading-name links.
11. **Open decisions**:
    - `fs/stat` and `fs/read` inside `.git`: all three companions refuse
      writes into `.git` but allow reads and stats; the protocol states
      only the write rule ([companion-protocol.md, The workspace folder](companion-protocol.md#6-the-workspace-folder)).
    - CLI command lines: the protocol's argv (`claude -p --output-format
      json --tools "" …`, [companion-protocol.md, Providers](companion-protocol.md#8-providers-providersjson-declared-not-scanned)) differs from
      `dry_core::cli` (`stream-json`, `--setting-sources ""`,
      `--disable-slash-commands`, [engine-standard.md, CLI models](engine-standard.md#cli-models-built-coreclirs-toolsclirs)). The Rust
      companion uses the protocol's. Which one is the standard is open.

## Known gaps and risks

**The repository**

- Local `main` (`5f3349b`) and `origin/main` (`b2baf24`) have diverged:
  one commit each, same message and time. Pages was deployed from
  `b2baf24`.
- 45 changed paths are uncommitted, including whole features:
  `companion/dry_companion.py`, `crates/tools/src/companion/` (the Rust
  companion), `crates/core/src/workspace.rs` (the folder workspace) and
  `crates/html/src/render/live.rs`. The ✅ marks for them rest on the
  working tree.

**Docs that disagree with the code or each other**

- [browser.md](browser.md) says streaming and voice are not in the browser and
  that `file` artifacts are refused. [engine-standard.md](engine-standard.md)
  (browser streaming measured) and [capabilities.md](capabilities.md)
  (Web Speech; files through a companion) say otherwise.
- [browser.md](browser.md) lists a `card` ask; it is now `review`.
- [parity-and-features.md](parity-and-features.md) names main `f5cbc5f`, not in
  this history, and 142 tests.

**Not built, from the docs' "Not yet" lists and the code**

- Alerts fire (built 2026-11-23: `alert::settle` on the callbacks tick, the
  `alert` tool; docs/engine-standard.md, `kind: alerts`). Still not built:
  `watch` triggers, firing while no shell runs (`dry alerts`), and a
  schedule trigger in the analytics spans.
- No person-approval for actuators; push and clone use an interim
  confirm dialog. Session approvals are lost when the host restarts. A
  parent cannot decide its child's approvals.
- The host starts no MCP servers. Host pages cannot edit one token's
  value, and cannot use Voice.
- `--strict` asks the token of local clients too, but the GPUI window and
  session workers do not send one yet.
- The GPUI window as a client: its new-thread, format, delete, settings
  and mode edits stay local; no prompt or compression line for host turns.
- The browser cannot attach to a remote host (no HTTP `TextStore` or
  `Transport`). No OPFS store. A granted folder is read once.
- Rust companion: no projects mode, custom tools or git; Windows not
  built or run; `stop-all` does not interrupt an evidence run. Python
  companion: no browser.
- CLIs answer whole, not token by token. Snapshots are text only (no
  vision). `jev` is a placeholder.
- Container: not wired to `terminal`; networking out needs a relay;
  mounts are empty; whole-machine snapshots only (53 MB each).
- Learning: no promotion of lessons into skills; no learn pass.
- Evidence: no native shots, no full-page shots; the evidence span is
  defined but nothing emits it.

**Verification that is manual only**

- 11 GPUI and 2 dry-tools tests are ignored: they need LM Studio. Their
  numbers are in the docs, not in CI.
- The IndexedDB tests (`crates/web/tests/idb.rs`) run only with a
  browser by hand; `cargo test` runs 0 of them.
- Firefox and Safari were not tested; Safari needs the companion-served
  app.

## Numbers

Counted 2026-11-23 on the working tree.

| What | Count |
|---|---|
| Rust crates | 6: core, tools, gpui, server, html, web |
| Rust lines (`*.rs`) | 65,521: core 32,365 · tools 11,546 (Rust companion 4,658) · gpui 7,902 · web 5,508 · server 4,104 · html 4,096 |
| Node companion | 2,913 lines, plus 770 of tests |
| Python companion | 3,231 lines, plus 176 of tests |
| Conformance suite | 859 lines (`conformance/*.mjs`), 40 tests |
| `cargo test -q` | 446 passed, 0 failed, 13 ignored: core 356 · tools 46 + 9 · server 13 + 5 + 3 + 1 · html 7 + 3 · web 2 · gpui 1 |
| `node --test 'companion/*.test.mjs'` | 33 passed, 0 failed |
| `python3 -m unittest companion/test_dry_companion.py` | 10 passed |
| `node conformance/run.mjs` | 40/40 Node · 40/40 Python · 40/40 Rust |
| Evidence | 10 shots, 0 changed, pass |
| Docs | 17 in `docs/` with this one (about 5,100 lines before it), plus `README.md`, `evidence.md`, `responses.md`, and 3 files in `capabilities/` |
| Commits | 1 on local `main` |
