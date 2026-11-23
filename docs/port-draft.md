# Port draft: scripts, alerts, live artifacts

Idea draft, not a plan yet. Sources: the old `dry` project (scripts, alerts,
live artifacts) and `Sadhaka/dsh-superagent-prototype` (ideas). Target: the
structure dry-learn already has. Goal: few new primitives, each in one known
place.

## What dry-learn has

| Primitive | Where | Gives |
|---|---|---|
| Engine | `core/state.rs` | messages, tools, artifacts, mcp, session, meta, response, format |
| Engine as files | `core/files.rs`, `state/<id>/{spec,convo,artifacts}` | `spec/` sharable, `convo/` history, `artifacts/<name>.md` |
| Calls | `core/calls.rs` | `name(key="v"); ...` |
| Built-in tools | `core/tools.rs` | reply (with choices), write, read, list |
| Loop | `core/turn.rs` | step until `reply`, no step limit |
| Prompt | `core/prompt.rs` | instructions → tools → artifacts (names only today) → history → contract |
| Card | `core/card.rs` | one display + Status for every object |
| Pages | `core/pages.rs`, `routes/`, `components/` | closed `Component` enum, pages are JSON |
| Dashboard | `core/dashboard.rs` | live status, tiles: count, working, need you |
| MCP / app tools | `core/mcp.rs`, `tools/src/{mcp,app_tools}.rs` | outside tools; the app's own tools as `dry mcp` |
| Stores | `tools/src/store.rs` | md and sqlite → same State |
| Tick | `gpui/src/main.rs` | 1 s file watch loop |

## What the old project has worth porting

**Scripts** — `spec/scripts/<name>.md`: `---` header (name, description,
`params: a, b?`), body of `- call(...)` lines with `{param}` slots.
Parser `packages/core/src/agent.rs` (2..12 steps, 8 per engine). Run in order,
stop at first failure, one report back, never `reply` or another script.
Keep: format, limits, slot escaping, one report. Drop: one tool per script,
start/boot scripts, inline shell one-liners.

**Alerts** — `spec/alerts/<name>.md`: header (name, description, `trigger:`,
`once:`), body is the message the engine wakes with. 8 trigger kinds
(`agent.rs:104`); a 5 s watcher in `packages/api/src/lib.rs`, pure steps in
`packages/api/src/alerts.rs`. Rules: baseline first (never fire at once),
wait while busy, missed is missed, fire message
`Alert <name> fired (<trigger>): <what>`. Keep those and `at` / `every` /
`after <engine>`, later `when`. Drop `watch`, `check <sh>`, `message`, env
knobs, and the duplicate Python alert engine.

**Live artifacts** — `LiveArtifact` trait (`packages/tools/src/artifact.rs`):
fixed name and one-line meaning, `snapshot()` at every render and never in
history, brings its own tools, clock rendered last. 9 kinds. Keep the
behaviour; make the kinds a closed enum in core instead of types in the I/O
crate; drop `blocking()` and the nested Goal.

## What dsh teaches

1. The engine never schedules itself: waiting is an event source feeding the
   same entry as the composer.
2. Attention first: "does anything need me now?" leads the UI (our Need you tile).
3. Structured reports (progress, waiting, ready, blocked, failed) — a
   contract field, not free text.
4. Store the work first, then do it; retry with backoff; raise after 3 fails.
5. One read tool over resource paths beats many tools.
6. Overrides remember their base default ("Default updated" + diff).
7. Generated sections go last in the prompt (cache-friendly; clock last).

## Where each lives (curated)

New: `core/script.rs`, `core/alert.rs`, a `Live` enum, one `script` tool.
Everything else reuses calls, turn, card, stores and the tick.

| Thing | Files | Pure logic | I/O | Engine reaches it | UI |
|---|---|---|---|---|---|
| Script | `spec/scripts/<name>.md` | `core/script.rs`: parse, expand, limits | store reads `spec/scripts/` | one `script(name=…)` tool | card per script, last-run status |
| Alert | `spec/alerts/<name>.md`, fires logged in `convo/alerts.log` | `core/alert.rs`: `Trigger {At, Every, After}`, `due(last, now)` | tick (window open) or `dry alerts` (closed) appends a user message and runs a turn | write the file, or an `alert` tool | card per alert, dashboard tile |
| Live artifact | `artifacts/<name>.md` with `live: clock\|alerts\|scripts\|engines` | `Live` enum: meaning, brought tools; `prompt::render` takes snapshots, clock last | shell builds snapshot text | tools exist only while the artifact does | Artifacts component marks them live |

## Order (smallest first, each testable alone)

1. Prompt renders artifact content; `live: clock` last, never in history.
2. `core/script.rs` + `script` tool; fake-model turn test.
3. `scripts` / `alerts` live artifacts bringing their tools.
4. `core/alert.rs` with `at` and `every`; fake-clock tests.
5. Firing: tick + `dry alerts`, `alerts.log`, wait while busy, tile.
6. `after <engine>`, then `when` (needs an approval idea first).

## Open questions

1. Who fires alerts while the window is closed: a `dry alerts` daemon, or the future server?
2. Engines create scripts/alerts by writing files (fewer tools), or via tools?
3. No approvals yet: hold `when` and unattended scripts until there is one?
4. Do sessions (claude via `sessions.json`) get alerts, listed in their prompt?
5. Should spec files record their base default, solving shipped → live sync once?
