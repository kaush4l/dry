# Parity with the old dry

The owner's rule: the current (old) dry is the parity checker for this
rebuild. This page checks it, 2026-11-23, against old `main` at `31b2e5d`
(`/Users/kaush/Downloads/Dev/dry`) and this repo's working tree.

How it was made:

- The old app was inventoried from its **code**: the page files
  (`app/pages/*.md`), component files (`app/components/*.md`), the tool
  registry (`packages/tools/src/lib.rs`, `registry()` and `APP_TOOLS`), the
  ~120 server functions in `packages/api/src`, the boot steps
  (`packages/api/src/boot.rs`), the binaries in each `Cargo.toml`, and the
  core modules (`packages/core/src`).
- Each item was looked up in this repo's code, not its docs.
- The shared flows were **run** in both apps, side by side, on scratch
  copies (see [The run](#the-run)).

Marks: ✅ done · 🟡 partial (what is missing is said) · ❌ missing ·
⛔ out by decision (the decision is cited) · ➕ new in dry-learn.

Old paths are relative to the old repo, under `packages/` unless they start
with `app/`, `default/` or `templates/`. New paths are relative to this repo.

## Counts

| Mark | Rows |
|---|---|
| ✅ done | 37 |
| 🟡 partial | 34 |
| ❌ missing | 38 |
| ⛔ out by decision | 6 |
| ➕ new in dry-learn | 26 |
| **Old features checked** | **115** (✅ + 🟡 + ❌ + ⛔) |

## The run

Both apps were built from a copy of their source in the scratchpad (never
in either repo's `target/`), started on free ports with a scratch state, and
driven through the same flows in a scratch headless Chrome (port 9333) by a
small CDP script, every shot 1280×800. Nothing touched the owner's
instances (old dry's 8080, dry-learn's `dry-server` on 8090, `dry-gpui`) or
any real `state/`. Everything started was stopped afterwards.

**Old dry**: `dx bundle --package web --platform web --release`, then the
bundle's `server` with:

```text
DRY_STATE_DIR=<scratch>/old/state   # an empty folder: a test server, so the services gate is shut
DRY_NO_SERVICES=1                   # no services, no Telegram, no start scripts, no priorities rechecks
DRY_ENGINE_POLL_MS=0                # no engine-folder watch
DRY_MODELS=<scratch>/old/models.json  # one model: claude-cli/haiku (the shipped catalog names
                                    #   a "fusion" provider on 127.0.0.1:8090, dry-learn's port)
TELEGRAM_BOT_TOKEN, DRY_SERVICES, DRY_MCP unset; IP=127.0.0.1 PORT=8187
```

The log confirmed `[services] off`, `[telegram] not started`, `[init] no
start script run`, `[update] off` (a test server never updates itself). Two
things worth knowing:

- A debug `dx build` server never hydrates outside `dx serve`: the client
  waits for a devserver ("Your app is being rebuilt") and the views stay on
  "Loading…". The release bundle works. That is the path
  `scripts/build-all.sh` takes too.
- At boot, `trade_mcp::cleanup_stale` removed "1 stale turn folder" from
  `$TMPDIR`. By design it removes only folders of dry processes that are
  gone, but it is a test server touching a shared temp folder.

**dry-learn**: `cargo build -p dry-server -p dry-tools`, then
`dry-server --port 8197` with `DRY_ROOT=<scratch copy>` and
`DRY_STORE=md:<scratch>/new/state`. The copy's `models.json` default was
set to `claude/haiku`; its `state.txt` and `state.host.json` (the owner's)
were removed from the copy first. The host starts no MCP servers by design.

### Flows and results

Same model in both: the `claude` CLI on haiku, no API key stored.

| Flow | Old dry | dry-learn |
|---|---|---|
| Create an engine | ✅ `POST /api/sessions/create` `{kind: engine, name: parity, model: claude-cli/haiku}`. The new engine gets the default's instruction but **no response format**, so its prompt has no tools ("not sent: no tools, or the response has no `act` field"): a plain chat engine | ✅ `POST /api/engines` → `engine-1` with `reply, write, read, list`, toon format, `cli:claude` haiku |
| Run a turn | ✅ "hello from the old dry", 4.8 s, `claude-cli/haiku`, $0.0008 (from the message's meta) | ✅ "hello from dry-learn.", 3.6 s (`turn` span), one step |
| Answer an ask | ✅ as an **approval**, the old app's only ask: the `default` engine called `write(note.txt)`, it became job 1 "awaiting approval", **Approve** was clicked in the page, the job finished and the engine replied "done"; `note.txt` is on disk in its workspace | ✅ as an **A2UI ask**: `reply(ask="choice", options="Red|Green|Blue")` drew three buttons, **Green** was clicked in the page, the next message was `Answer (choice): Green`, the model replied "Green it is." |
| Engine page / live view | ✅ the engine page: card, step-by-step reasoning with per-step ms and tokens, and the next prompt with the live `workspace` artifact showing `note.txt` | 🟡 the engine page and `/desk/0/live` work, but `write` made a **text artifact**, not a file, so the live view says "holds no file or terminal artifact"; the artifact has its own page (`/desk/0/artifacts/note.txt`) |
| Settings / theme | ✅ Settings (approval rules, models, MCP, appearance, easter eggs); theme switched to `harbor` | ✅ Settings (mode, themes, models with Detect/Test, links to UI tokens and capabilities); mode switched to light |

Not compared by running: the GPUI windows (old `dry-gpui` is a thin client
of the old server; this repo's needs a bundled app for shots), the TUI, the
browser builds, and everything that needs a service (Telegram, MCP servers,
self-update, the bridge). Those rows are code comparison only.

A side effect seen in the old run: a restart loses a job in flight
("`[jobs] default: job 1 lost at restart (noted)`"), and the day file and
engine then say so.

### Screenshots

In `/private/tmp/claude-501/-Users-kaush-Downloads-Dev-dry/cc927124-d30f-4ccd-90dc-b5d03ae78109/scratchpad/parity/`
(scratch: copy them out to keep them).

| Page | Old dry | dry-learn |
|---|---|---|
| Home after the run | `old-01-home.png` (Dashboard: Day, Subscription, engine tile) | `new-01-home.png` (Home: counts, engines, today) |
| Engines | `old-02-engine.png` (Engine page) | `new-02-engines.png` (Engines list) |
| Ask pending | `old-03-ask-pending.png` (Approve / Deny / Review bar) | `new-03-ask-pending.png` (Red / Green / Blue) |
| Ask answered | `old-04-ask-answered.png` | `new-04-ask-answered.png` |
| Live view | `old-05-live.png` (next prompt: workspace with `note.txt`) | `new-05-live.png` (live view), `new-05b-artifact.png` (the `note.txt` artifact) |
| Settings | `old-06-settings.png`, `old-07-settings-appearance.png` | `new-06-settings.png`, `new-07-settings-ui.png` (UI tokens) |
| Theme switched | `old-11-theme-switched.png` (`harbor`) | `new-11-theme-switched.png` (light) |
| Pages only one has | `old-08-components.png`, `old-09-loops.png`, `old-10-changes.png` | `new-08-code.png`, `new-09-analytics.png` |

## The table

### Engines and specs

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Engine as a folder (spec, convo, memory) | `core/src/{engine,loader}.rs`, `state/<e>/` | `core/src/{state,files,store}.rs`, `state/engines/<id>/{spec,convo,artifacts}` | ✅ | |
| Create an engine | `api/src/lib.rs` `create_session`, `session_specs()` | `server/src/main.rs` `POST /api/engines`, `server/src/host.rs` | ✅ | Run in both |
| Create an engine by writing its folder | `api/src/folders.rs` (folder watch) | none | ❌ | dry-learn notices changed files (`live::watch`) but a new folder is not a new engine by itself |
| Spec layers: shipped `default/`, personal `engines/`, state overrides | `core/src/{layers,markdown}.rs` `engine_layers`, `default/`, `engines/` | `default/`, `core/src/shipped.rs` | 🟡 | Being built now by another agent (a fresh store gets `default/`). No personal layer over it, no per-file override like the old `[migrate] kept 2 overrides` |
| Special engines (creator, refocus, update) | `default/{creator,refocus}/`, `api/src/update.rs` | none | 🟡 | Refocus is built into the loop (`core/context.rs`); no creator engine; no updater |
| Templates (developer, translator, insight, night, skill-keeper…) | `templates/*` | `core/src/template.rs` (prompt slots only) | 🟡 | The prompt template exists; none of the engine templates were ported (trading ones are out) |
| Phases | `core/src/phase.rs`, `tools/src/phase.rs` (`phase_done`) | `core/src/phase.rs`, `goto_phase` | ✅ | |
| Goal | `tools/src/artifact.rs` `goal` | `core/src/goal.rs` | ✅ | |
| Response formats (toon, json) | `core/src/responses.rs` | `core/src/response.rs`, `responses.md` | ✅ | A new old-dry engine has no format, so no tools; a new dry-learn engine has toon and four tools |
| Scripts | `core/src/agent.rs`, `tools/src/agent.rs` (`script_create`, `script_delete`) | `core/src/script.rs`, `script` | 🟡 | Run and list; no create/delete tools (the engine writes the file) |
| Start scripts and services (`spec/scripts/init.md`, `spec/services/`) | `api/src/{init,services,service_guard}.rs`, `core/src/service.rs` | none | ⛔ | Dropped on purpose: [port-draft.md](port-draft.md) |
| Skills | `tools/src/agent.rs` `skill` | `core/src/skill.rs`, `skill`, `skill_save`, `dry skill import` | ✅ | |
| Skill keeper (provenance, keep current) | `core/src/skill_keeper.rs`, `api/src/skill_keeper.rs`, `templates/skill-keeper` | none | ❌ | |
| Learning file (three capped sections) | `core/src/learning.rs`, `tools/src/learn.rs` | `core/src/learning.rs` (`collect`, `learn`) | 🟡 | No promotion of lessons into skills, no learn pass |
| Refocus / compression | `core/src/prompt.rs` `refocus_threshold` (90%), `default/refocus` | `core/src/context.rs` (75%) | ✅ | |
| Tool groups and `expand` | `tools/src/lib.rs` `GROUPS`, `Expand` | artifacts bring their tools (`core/src/live.rs`) | 🟡 | A different answer to the same problem: tools appear with the artifact that needs them |
| Context meter and prompt parts | `api` `/api/engine/status`, `/api/engine/prompt` | `core/src/context.rs`, Prompt fold, `dry prompt` | ✅ | |
| Clear memory | engine page "Clear memory", `/api/messages/clear` | `POST /api/engines/:id/delete` | 🟡 | Delete the engine only; no clear, no per-message delete or toggle (`/api/messages/{delete,toggle}`) |

### Tools an engine can call

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| `reply` | `tools/src/reply.rs` | `core/src/tools.rs` | ✅ | |
| Files: `read`, `write`, `edit`, `list` | `tools/src/fs.rs` | `core/src/file.rs` (`open`, `focus`, `edit`, `save`), `core/src/tools.rs` (`read`, `write`, `list` on artifacts) | ✅ | Seen in the run: dry-learn's plain `write` makes an artifact, not a file; files need the `file` artifact |
| File `delete` | `tools/src/fs.rs` `Delete` | none | ❌ | |
| `terminal` | `tools/src/terminal.rs` | `core/src/terminal.rs` (`run`, `start`, `background`) | ✅ | dry-learn adds callbacks on `start` |
| Clock | `tools/src/clock.rs`, `clock` artifact | `Live::Clock` | ✅ | |
| Alerts: create and delete | `tools/src/agent.rs` `alert_create`, `alert_delete` | `Live::Alerts` (list only) | 🟡 | The engine writes the file; there is no tool |
| Child engines (`engine:<name>`, `ask`) | `api/src/host.rs` `ask`, `api/src/lib.rs` `ask_engine` | `core/src/engines.rs` (`engine_call`, `engine_start`), `POST /api/engines/:id/children` | ✅ | |
| Team notes (`note`) and the board | `tools/src/team.rs`, `api/src/board.rs` | `core/src/shared.rs` (`post`, `edit_post`), `POST /api/engines/:id/posts` | ✅ | |
| `critique` (phase tool) | `tools/src/team.rs` `Critique` | none | ❌ | |
| App state (`state` tool) | `tools/src/state.rs` | `GET /api/engines/:id/read`, `dry snapshot`, MCP `engines` group | 🟡 | For sessions over MCP; a model engine has no `state` tool |
| Pages and components (`ui`, `page_create`, `page_delete`, `catalog`, `component`) | `tools/src/{ui,page,catalog}.rs` | `tools/src/app_tools.rs` (`page_create`, `page_add`, `page_delete`, `component_create`) | 🟡 | Over MCP, for sessions and engines alike; no node-by-node edit, no catalog lifecycle |
| Theme (`css_get`, `css_set`, `css_reset`) | `tools/src/css.rs` | `app_tools.rs` (`token_set`, `token_reset`, `theme_use`), `dry token` | ✅ | Tokens, not CSS properties |
| `preview` (render a page) | `tools/src/preview.rs` | `core/src/browser.rs` (`browse`, `click`, `type`) | 🟡 | A general browser artifact; nothing renders the app's own pages for the engine |
| `developer_create`, `worktree_create` | `api/src/{lib,projects}.rs` | none | ❌ | |
| `telegram` | `api/src/telegram.rs` | none | ❌ | |
| `usage` | `api/src/usage.rs` | none | ❌ | |
| `bridge` | `api/src/bridge.rs` | none | ❌ | |
| `trade` | `api/src/{trade_mcp,execution}.rs` | none | ⛔ | Trading stays out of the general app (owner, memory "dry long-term goal"; [parity-and-features.md](parity-and-features.md)) |

### Live artifacts

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| `workspace` (files, open file content) | `tools/src/artifact.rs` | `core/src/file.rs` (`kind: file`) | ✅ | |
| `goal`, `clock`, `scripts`, `alerts` | `tools/src/artifact.rs` | `core/src/{goal,live,script,alert}.rs` | ✅ | |
| `notes` | `tools/src/artifact.rs` | `core/src/shared.rs`, text artifacts | 🟡 | |
| `dashboard` (team) | `tools/src/artifact.rs` | `core/src/{shared,dashboard}.rs` | ✅ | |
| `ui` (the app's pages as an artifact) | `tools/src/artifact.rs` | none | ❌ | |
| Open and close an artifact | `tools/src/artifact.rs` `open`, `close` | `open`, `focus` on `file` | 🟡 | Only for files |

### Alerts, jobs, scheduling

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Alert triggers parsed (`at`, `every`) | `core/src/agent.rs` `Trigger` | `core/src/alert.rs` | ✅ | |
| Other triggers (`watch`, `check`, `after`, `message`, `when`) | `core/src/agent.rs` | none | ❌ | |
| Alerts fire | `api/src/alerts.rs` (watcher, grace, catch-up, retry) | `core/src/alert.rs` ("Firing is not built yet") | ❌ | The host already ticks every 2 s (`host.rs` `tick`) |
| Jobs: slow calls become a job that wakes the engine | `api/src/jobs.rs` | `core/src/terminal.rs` callbacks, child engines settle (`host.rs` `tick`) | 🟡 | Callbacks for `start` and children only, not for any slow tool |
| Resume a turn after a restart | `api/src/resume.rs` (checkpoint on disk) | `host.rs` resumes a turn whose callbacks are done | 🟡 | No checkpoint: a restart mid-turn loses the turn |
| Night plan, calendar, pacing | `api/src/{plan,calendar}.rs`, `templates/night` | none | ❌ | |
| Spend budgets per job | `api/src/budget.rs` | none | ❌ | |

### Models

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| OpenAI chat and responses | `inference/src/openai.rs` | `core/src/inference.rs` | ✅ | |
| Anthropic Messages | `inference/src/anthropic.rs` | none | ❌ | |
| `claude` CLI as a model | `inference/src/cli.rs` | `core/src/cli.rs`, `tools/src/cli.rs` | ✅ | Run in both; dry-learn adds codex and copilot |
| Streaming (SSE) | `inference/src/sse.rs` | `core/src/stream.rs`, `early.rs` | ✅ | |
| Model discovery | `inference/src/discover.rs` | Settings Detect, `dry models` | ✅ | |
| Usage ledger | `inference/src/ledger.rs` | `core/src/spans.rs` (cost per turn) | 🟡 | Spans keep cost; no per-call ledger with origin and switches |
| Subscription and local balancing | `inference/src/balance.rs`, `api/src/{usage,subscription,calibrate}.rs` | none | ❌ | |
| Sampling settings per engine | none | `dry engine set`, `core/src/sampling.rs` | ➕ | |

### Sessions (program workers)

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Claude Code and Codex sessions | `inference/src/program.rs`, `session_specs()` | `core/src/session.rs`, `tools/src/worker.rs`, `sessions.json` (claude, codex, pi, hermes) | ✅ | |
| Live tool log of a session turn | `api/src/program_log.rs` | `core/src/stream_json.rs`, live tool rows on the host | ✅ | |
| Session instructions and model edit | `/api/sessions/{instruction,model}` | `dry engine use`, `dry engine model` | 🟡 | CLI only; host pages cannot start session engines (`HOST_LATER`) |

### Approvals

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| A tool call that needs approval waits for a person | `tools/src/lib.rs` `needs_approval`, `api/src/approvals.rs` | `core/src/approval.rs`, `server/src/host.rs` (sessions' permission requests) | 🟡 | Run in old. In dry-learn only for sessions; an engine's `write` and `terminal` never ask |
| Unattended rules (`approvals.json`, night shift) and their editor | `core/src/approval.rs`, Settings "Unattended approvals" | rules in `host.rs` `ask_approval` | 🟡 | No editor, no time windows; approvals live in host memory |
| Auto-approve switch | footer "Auto-approve", `/api/approvals/auto` | none | ❌ | |
| Trading rails | `core/src/rails.rs`, `app/components/trading_rails.md` | none | ⛔ | Trading is out |

### Server, clients, away

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| One server holds the state and runs turns | `packages/web` + `api` (Dioxus fullstack) | `crates/server` (actix) | ✅ | The old repo dropped actix on 2026-11-17 (memory "dry actix dropped"); dry-learn uses it again |
| Push updates (SSE, resume) | `api/src/push.rs`, `ui/src/push.rs` | `server/src/live.rs`, `/events` with `Last-Event-ID` | ✅ | |
| Origin guard and token for remote | `api/src/origin_guard.rs` | `server/src/main.rs` (Origin check, `--bind` needs `--token`) | ✅ | |
| gzip | `web/src/main.rs` `CompressionLayer` | none | ❌ | Small |
| Clients attached ("1 client") | `api/src/clients.rs`, footer | none | ❌ | |
| Paired devices (pair code) | `api/src/devices.rs`, `tui/src/bin/dry-pair.rs` | companion pairing (`core/src/companion.rs`) | 🟡 | Pairs a companion to the web app, not a device to the host |
| Server to server (the bridge) | `api/src/bridge.rs`, `app/pages/bridge.md` | none | ❌ | Owner's queued item 2 in [status.md, Planned and queued](status.md#planned-and-queued) |
| Telegram | `api/src/telegram.rs`, `telegram/` | none | ❌ | |
| Day file and transcript (away) | `core/src/{day,transcript}.rs`, `api/src/day.rs`, `dry-day` | none | ❌ | Pure modules, an easy port; the old Dashboard's first card |
| Boot as steps, services gate | `api/src/boot.rs`, `app/boot/*.md` | `server/src/main.rs` | 🟡 | dry-learn has no services to gate; no boot UI script |
| Modes Auto, Creator, Developer | `ui` `ModeSwitch`, `/api/mode` | none | ❌ | dry-learn's `/api/mode` is light or dark |

### Pages

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Dashboard / Home | `app/pages/dashboard.md` (day, usage glance, engine tree) | `routes/page.json` (Home) | 🟡 | Engine tiles and today's work; no day card, no subscription |
| Engine page | `app/pages/engine.md`, `ui/src/views/{engine,thread,prompt}.rs` | `routes/desk/[id]/page.json`, `html/src/render/engine.rs` | ✅ | Run in both. Old shows each step's ms, tokens and tok/s inline |
| Engine tree (main, subs, sessions row, special engines folded) | `ui/src/views/engine_tree.rs`, `app/components/engines.md` | `routes/desk/page.json`, sidebar "Live" list | 🟡 | A flat list; children are in the `engines` artifact |
| Changes (worktrees as PRs, diff, shots, merge) | `app/pages/changes.md`, `api/src/changes.rs`, `core/src/{diff,change}.rs` | none | ❌ | |
| Replay (a run step by step, sub runs in lanes) | `app/pages/replay.md`, `api/src/replay.rs`, `core/src/run.rs` | `routes/analytics` (spans) | 🟡 | Analytics has the spans; no step-by-step replay |
| Components gallery | `app/pages/components.md`, `ui/src/views/gallery.rs` | `dry components`, `routes/code` | 🟡 | |
| Bridge | `app/pages/bridge.md` | none | ❌ | |
| Plan (usage, budget) | `app/pages/plan.md` | none | ❌ | |
| Loops (blueprints as blocks) | `app/pages/loops.md`, `core/src/blueprint.rs`, `api/src/loops.rs` | none | ❌ | |
| Priorities (insights) | `app/pages/priorities.md`, `core/src/priority.rs`, `api/src/priorities.rs` | none | ❌ | |
| Settings | `app/pages/settings.md` (approvals, models, mcp, appearance) | `routes/settings/` | 🟡 | Run in both. Missing: approval rules, MCP servers, easter eggs |
| Pages as data, made at run time | `core/src/{page,component,bind,view/}.rs`, `app/pages/*.md` | `core/src/{routes,pages,card}.rs`, `routes/**/page.json`, `components/*.json` | 🟡 | Folder routes and a closed `Component` enum; no binding language, sources or actions |
| Old components with no counterpart | `app/components/{budget,changes,clients,day,loops,mcp,priorities,replay,self_update,services,updates,usage,usage_glance}.md` | none | ❌ | 13 components |

### MCP

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| MCP client, stdio | `mcp/src/rpc.rs` | `core/src/mcp.rs`, `tools/src/mcp.rs` | ✅ | |
| MCP over HTTP | `mcp/src/rpc.rs` | none | ❌ | |
| Servers kept warm, restart, list in Settings | `api` `/api/mcp/{servers,restart}`, `app/components/mcp.md` | GPUI starts them | 🟡 | The host starts none |
| The app's tools served over MCP | `api/src/trade_mcp.rs` (trading only) | `tools/src/app_tools.rs`, `dry mcp`, `mcp/*.json` | ➕ | |

### Developer tooling

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Developer engines with their own worktree | `core/src/project.rs`, `api/src/projects.rs`, `templates/developer*` | none | ❌ | |
| PR screenshots | `tools/src/pr_shots.rs`, `pr-shots` | `core/src/evidence`, `dry evidence` | ✅ | Evidence covers it, with a baseline and a report |
| Self-update | `api/src/update.rs`, `app/components/self_update.md` | none | ❌ | |
| Upstream installs | `api/src/upstream.rs`, `app/components/updates.md` | none | ❌ | |
| Trials and rounds | `trial/`, `dry-trial`, `trials/`, `rounds/` | none | ❌ | |
| Build metrics and transcript check | `metrics/`, `dry-metrics`, `transcript-check` | `core/src/{spans,analytics}.rs` | 🟡 | Run analytics, not build metrics |
| Test fakes (`service-fake`, `mcp-fake`) | `api/src/bin`, `mcp/src/bin` | in-test fakes | ✅ | |

### Look, scenes, pets

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Themes | `core/src/{theme,theme_doc}.rs`, `app/themes/{classroom,console,harbor}` | `core/src/theme.rs`, `themes/{default,cat,terminal,abyss}.md` | ✅ | Run in both |
| Presets, fonts, status tilt, done phases | `ui/src/views/theme.rs`, `/api/theme/{preset,prop}` | tokens | 🟡 | Every token is editable (GPUI); no presets; host pages cannot save |
| Backdrop wallpapers ("Dunes 1/3") | `ui` | `core/src/backdrop.rs`, scenes | 🟡 | |
| Pet (the cat on the floor) | `app/eggs/pet`, `core/src/pet.rs`, `gpui/src/pet.rs` | `core/src/scene/cat.rs` | 🟡 | The cat reacts in GPUI; the browser passes a fixed mood; hosted HTML draws no scene. Old shows it on the web page (seen in the run) |
| Other eggs (cursor glow, ripple, world) and the egg channel | `app/eggs/*`, `api/src/{eggs,egg_room}.rs` | none | ❌ | |
| Characters (husky, St Bernard) and the journey | `app/characters/`, `core/src/{character,journey}.rs` | none | ❌ | |
| Voice | `speech/` (macOS Speech, `--dry-hear`), `api/src/speech.rs` | `core/src/composer.rs` check, `web/src/caps.rs` (Web Speech) | 🟡 | Hosted HTML: "not checked"; GPUI: not wired |

### Shells and stores

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Web app with a server | `packages/web` | `crates/server` + `crates/html` | ✅ | |
| Static browser build | `scripts/browser-build.sh`, `browser` feature | `crates/web` (wasm, on Pages) | ✅ | dry-learn's runs turns in the page |
| GPUI | `packages/gpui` (thin client of the server) | `crates/gpui` (standalone, follows a host on the same machine) | 🟡 | Not a client of a remote host |
| Desktop webview | `packages/desktop` | none | ⛔ | GPUI is the desktop shell (the old repo's `docs/ideology.md`: GPUI primary) |
| Terminal UI | `packages/tui` | none | ❌ | Queued item 5 in [status.md, Planned and queued](status.md#planned-and-queued) |
| iOS | `packages/mobile` | none | ❌ | Queued item 7 |
| Stores: folder, sqlite, memory | `core/src/{store,sqlite,storage}.rs` | `tools/src/store.rs` (md, sqlite), `dry store copy` | ✅ | |
| IndexedDB | `core/src/indexeddb.rs` | `web/src/idb.rs` | ✅ | |
| Docs held to the code by tests | `core/tests/docs.rs`, `STANDARDS.md` | none | ❌ | |

### Trading

| Feature | Old dry | dry-learn | Mark | Note |
|---|---|---|---|---|
| Trader, desk, trader-skills templates | `templates/{trader,trader-skills,desk}` | none | ⛔ | Trading stays out of the general app |
| Execution, flatten, kill switch, replay of trades | `api/src/execution/`, `/api/trading/*`, `app/components/trading.md` | none | ⛔ | Same decision. The old app still seeds a `trading` folder into a fresh state (seen in the run) |

### New in dry-learn

| Feature | dry-learn | Mark | Note |
|---|---|---|---|
| A2UI asks (choice, multi, confirm, date, form, review…) | `core/src/ask.rs`, `html/src/render/ask.rs`, `gpui/src/views/ask.rs` | ➕ | Run: Red/Green/Blue answered by a click |
| Displays in a reply | `core/src/show.rs` | ➕ | |
| Capabilities: compiled, declared (`capabilities/*.md`), peers | `core/src/capability/`, `server/src/capabilities.rs`, `dry capabilities`, `dry discover` | ➕ | [capabilities.md](capabilities.md) |
| Companions (Node, Python, Rust), one protocol, conformance 40/40 | `companion/`, `tools/src/companion/`, `conformance/` | ➕ | |
| Folder workspace through a companion | `core/src/workspace.rs` | ➕ | |
| The whole app in the browser, published | `crates/web`, `.github/workflows/pages.yml` | ➕ | |
| Browser container (v86) | `core/src/container.rs`, `web/src/vm.rs` | ➕ | A spike |
| Browser artifact (CDP) | `core/src/browser.rs`, `tools/src/cdp.rs` | ➕ | |
| Evidence artifact and `dry evidence` | `core/src/evidence/`, `tools/src/evidence.rs` | ➕ | |
| Analytics spans and views | `core/src/{spans,analytics}.rs`, `routes/analytics` | ➕ | |
| Code page (explorer, editor, terminal, VS Code themes) | `core/src/workbench.rs`, `routes/code/`, `html/src/render/code.rs` | ➕ | |
| UI tokens page and `dry token` | `routes/settings/ui`, `core/src/theme.rs` | ➕ | |
| Scenes (cat, terminal, abyss) with mood | `core/src/scene/` | ➕ | |
| Model cards with Detect and Test | `routes/settings`, `POST /api/models/:name/{detect,test}` | ➕ | |
| `dry prompt` (the exact next prompt) | `tools/src/main.rs` | ➕ | |
| Live view (explorer, editor, panel) per engine | `routes/desk/[id]/live`, `core/src/liveview.rs` | ➕ | |
| Artifact pages (`/desk/:id/artifacts/:name`) | `routes/desk/[id]/artifacts/[name]` | ➕ | |
| Inbox and children `take` | `server/src/host.rs` `children`, `take` | ➕ | |
| Session hooks push the live snapshot into each turn | `tools/src/worker.rs`, `core/src/snapshot.rs` | ➕ | |
| Codex, copilot, pi, hermes programs | `models.json`, `sessions.json` | ➕ | |
| Store copy between md and sqlite | `dry store copy` | ➕ | |
| Export and import routes as one JSON | `dry export`, `dry import` | ➕ | |
| Liquid glass frame and layout | `core/src/{layout,screen}.rs`, `gpui/src/frame.rs` | ➕ | |
| Streaming parser checked at every byte | `core/src/stream.rs` | ➕ | The old app streams too, without the property test |

## Gaps, in the order to port them

Smallest first among the ones that matter. Each names where the old code
is.

1. **Alerts fire on the host.** The host already ticks every 2 s and
   `alert::due` exists; call it, append "Alert <name> fired", run a turn.
   Old: `api/src/alerts.rs` (grace, catch-up). Size S.
2. **File `delete`, and approval for an engine's `write`, `edit` and
   `terminal`.** The approval record and the Allow/Deny UI already exist
   for sessions; route engine tools through them. Old:
   `tools/src/lib.rs` `needs_approval_for`, `api/src/approvals.rs`. S.
3. **Day file and transcript**, then a Day card on Home. Pure modules.
   Old: `core/src/{day,transcript}.rs`. S.
4. **Clear memory and per-message delete** on the engine page. Old:
   `/api/messages/{clear,delete,toggle}`. S.
5. **Turn checkpoint and resume after a restart.** Old:
   `api/src/resume.rs`. S–M.
6. **gzip on the host** (one middleware) and a **clients count**. Old:
   `web/src/main.rs`, `api/src/clients.rs`. S.
7. **The host starts MCP servers**, and Settings lists them with Restart.
   Old: `/api/mcp/{servers,restart}`, `app/components/mcp.md`. M.
8. **Anthropic Messages provider and a usage ledger.** Old:
   `inference/src/{anthropic,ledger}.rs`. M.
9. **Approval rules editor and an Auto-approve switch.** Old:
   `core/src/approval.rs`, Settings "Unattended approvals". M.
10. **Jobs for any slow call** (not just `start` and children). Old:
    `api/src/jobs.rs`. M.
11. **Replay** from the spans analytics already keeps. Old:
    `api/src/replay.rs`, `core/src/run.rs`. M.
12. **More alert triggers** (`after`, `watch`, `message`, `check`). Old:
    `core/src/agent.rs`. M.
13. **Server to server** (the bridge), on the capability handshake. Old:
    `api/src/bridge.rs`. L.
14. **Telegram** reaching one engine, `/day` read-only. Needs 3. M.
15. **Developer worktrees and Changes.** Old: `core/src/{project,diff,change}.rs`,
    `api/src/{projects,changes}.rs`. L.
16. **Budgets, subscription feed, balancing, Plan page.** Old:
    `api/src/{budget,calendar,plan,usage,subscription}.rs`,
    `inference/src/balance.rs`. L.
17. **Loops, trials, priorities.** Old: `core/src/{blueprint,priority}.rs`,
    `trial/`. L.
18. **Pets and eggs on every shell**, then the TUI and iOS shells. L.

Kept out: trading in every form; start scripts and services; the desktop
webview.
