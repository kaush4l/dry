# Parity and features (proposal)

> Superseded: its parity table is replaced by [parity.md](parity.md) (2026-11-23, checked from the code and by running both apps); the outside apps and proposed features below still stand.

Research only, 2026-11-22. Old = `/Users/kaush/Downloads/Dev/dry` (Dioxus 0.7,
main at 31b2e5d). New = this repo (main at f5cbc5f, 142 tests against the old
1176). Status was read from the code, not the docs. Paths are relative to each
repo. Nothing here is decided; the owner picks.

## 1. Parity table

Status: **done** (the new one covers it), **partial**, **missing**,
**new-only**, **out** (left out on purpose).

| Area | Old (where) | New (where) | Status | Notes |
|---|---|---|---|---|
| Engines as files | `core/src/{engine,agent,layers,loader}.rs`, `default/`, `engines/`, `state/<e>/{spec,convo,memory}` | `core/src/{state,files}.rs`, `state/<id>/{spec,convo,artifacts}` | partial | The new one has no layered spec (personal over default) and no `default/` shipped as the base. "Create engine = write files" (`api/src/folders.rs`) is not in the new one. |
| Sessions (agent programs) | `inference/src/program.rs`, `api/src/program_log.rs` | `core/src/session.rs`, `sessions.json`, `gpui/src/program.rs` | partial | The new one runs and resumes the program with data-driven reply/id picks. It has no live per-tool log while the turn runs. |
| Loop, phases, template | `core/src/{phase,prompt,run}.rs`, `tools/src/phase.rs` (`phase_done`) | `core/src/{turn,phase,template,prompt}.rs` | done | The new one is cleaner: a phase table, `goto_phase`, six fixed slots, no step limits. Refocus (compaction) is in the old `prompt.rs` but missing here. |
| Response contract | `core/src/responses.rs` | `core/src/response.rs`, `responses.md` | done | `jev` is a placeholder in both. |
| Models and inference | `inference/src/{openai,anthropic,cli,fetch,sse,discover,ledger,balance}.rs` | `core/src/{inference,models}.rs`, `models.json`, `gpui/src/net.rs` | partial | The new one speaks only the OpenAI wire format, blocking. Missing: Anthropic Messages, `claude-cli` as a backend, streaming (SSE), model discovery, a usage ledger, subscription/local balancing. |
| Built-in tools | `tools/src/{fs,reply,learn,clock,terminal,state,catalog,ui,css,preview,team}.rs` | `core/src/tools.rs` (reply, write, read, list, script, skill, skill_save, goto_phase) | partial | Missing: the file tools (the `file` artifact is the planned path), terminal, `state` (one read tool), `learn`, catalog CRUD, preview. |
| App tools over MCP | `api/src/trade_mcp.rs` (one endpoint) | `tools/src/app_tools.rs`, `dry mcp`, `mcp/dry.json` | new-only | list, token_set/reset, theme_use, page_create/add/delete, component_create. One list serves engines and Claude sessions alike. |
| MCP client | `mcp/src/{config,rpc,tool}.rs` (stdio and HTTP) | `core/src/mcp.rs`, `tools/src/mcp.rs` (stdio) | partial | No HTTP transport. Neither has OAuth. Servers aren't kept warm from boot. |
| Skills | `core/src/skill_keeper.rs`, `api/src/skill_keeper.rs` | `core/src/skill.rs`, `dry skill import` | partial | Loading, saving and SKILL.md import are done. Missing: the skill keeper (provenance, keeping skills current). |
| Memory and learning | `core/src/learning.rs` (`memory/learning.md`, three sections), `tools/src/team.rs` (notes) | none | missing | Personal memory is one of the owner's three parts. Nothing holds it yet. |
| Artifacts / live | `tools/src/artifact.rs` (`LiveArtifact`, 9 kinds) | `core/src/live.rs` (`Live`: clock, scripts, alerts) | partial | The kinds are now a closed enum in core, which is better. The `file` artifact (engine-standard step 4) and the `engines` artifact aren't built. |
| Scripts | `core/src/agent.rs`, `api/src/init.rs` | `core/src/script.rs`, run in `turn.rs` | done | Start/init scripts are dropped on purpose (`port-draft.md`). |
| Alerts | `api/src/alerts.rs` (8 triggers, 5 s watcher) | `core/src/alert.rs` (`at`, `every`, `due()`, snapshot) | partial | Nothing fires: no shell or tool calls `due()`. `after <engine>` is drafted but not parsed. |
| Scheduling, jobs, resume | `api/src/{jobs,resume,plan,calendar,budget}.rs` | none | missing | Old: non-blocking tool groups that wake the engine when done, turn checkpoints that survive a restart, a night plan, spend budgets. |
| Child engines, teams, inbox | `core/src/team.rs`, `engine(name, task)`, dashboards | `core/src/dashboard.rs` (tiles only) | missing | The owner's original goal (a team of focused loops). Nothing lets one engine call or message another. |
| Server and clients | `api/` (44k lines): `push.rs` SSE, `clients.rs`, `endpoints.rs`, `origin_guard.rs`, `devices.rs` | `crates/server` (20 lines, `GET /`) | missing | The vision is "service first, every UI a client", but here the GPUI window owns the state and runs the turns. |
| Away mode | `core/src/{transcript,day}.rs`, `docs/away.md`, `metrics/src/day.rs` | none | missing | log → transcript → day file is pure core and an easy port. |
| Telegram | `api/src/telegram.rs` (+status, secrets, ideas) | none | missing | It only reaches the translator, and `/day` is read-only. |
| Pairing, bridge | `api/src/{devices,bridge}.rs`, `tui/src/bin/dry-pair.rs` | none | missing | Waits for the server. |
| Approvals and rails | `core/src/{approval,rails}.rs`, `api/src/approvals.rs` | none | missing | Needed before unattended scripts or `when` alerts (`port-draft` Q3). |
| Developer tooling | `core/src/{project,diff,change}.rs`, `api/src/{projects,changes,update,upstream,pr_shots}.rs`, `templates/developer*` | `core/src/workbench.rs`, `routes/code/` (explorer, editor, terminal) | partial | The new one has an editor-like page. Missing: worktrees per developer, the Changes/diff view, self-update, upstream installs, PR screenshots. |
| Loops, trials, metrics | `core/src/blueprint.rs`, `api/src/loops.rs`, `trial/`, `metrics/`, `trials/` | none | missing | Goal runs, rounds, morning reports and build metrics are the evidence behind "a developer that grows". |
| Priorities, insights, services | `core/src/{priority,service}.rs`, `api/src/{priorities,services,service_guard}.rs` | none | missing | Services are programs an engine needs running (e.g. a data MCP). |
| Trading | `api/src/{execution,trade_mcp}.rs`, `core/src/rails.rs`, `templates/{trader,trader-skills,desk}`, `app/components/trading*.md` | none | out | Correct per the owner: it stays out of the general app. Port nothing. |
| Pets, eggs, scenes | `core/src/{pet,character,journey,egg}.rs`, `app/eggs/{pet,world,cursor-glow,ripple}`, `app/characters/{husky,bernard}`, `gpui/src/{pet,fps}.rs` | `core/src/backdrop.rs`, `docs/scenes.md` (scenes in progress) | partial | Another agent owns scenes, themes and frame right now. The cat, journey and `FrameScene` aren't ported. |
| Voice | `speech/` (macOS Speech), `api/src/speech.rs`, `ui/components/voice.rs` | `voice.json`, `core/src/composer.rs` capability check | partial | The capability check is done. Recording isn't wired (`gpui/src/main.rs:780` says so). |
| UI shell | `ui/` (Dioxus, 44 views, 15 containers), `gpui/` (thin client), `tui/` | `gpui/` (frame, router, card, composer, engine, settings, code) | partial | The new one has its own GPUI views. There's no TUI or web client yet, so the renderer-neutral goal isn't proven by a second renderer. |
| Liquid glass, frame | none | `core/src/{layout,backdrop,screen}.rs`, `gpui/src/frame.rs` | new-only | |
| Themes, tokens | `core/src/{theme,theme_doc}.rs`, `app/themes/{classroom,console,harbor}` | `core/src/theme.rs` (941 lines), `themes/{default,terminal}.md`, `state/local.md`, `dry token` | partial | The token model is richer. Only 2 themes exist. Motion tokens (from the ideology) aren't there yet. |
| Routes, pages, components | `core/src/{page,component,bind,view/}`, `app/pages/*.md`, `app/components/*.md` | `core/src/{routes,pages,card}.rs`, `routes/**/page.json`, `components/*.json` | partial | New: folder routing and a closed `Component` enum. Old: md declarations, a binding language, sources and actions. Most old components (approvals, budget, changes, day, loops, usage, mcp, services…) have no counterpart. |
| CLI | `trial`, `metrics`, `dry-day`, `transcript-check`, `pr-shots`, `dry-pair` binaries | `tools/src/main.rs` (`dry` routes/page/component/theme/token/mcp/tool/prompt/store/skills) | partial | `dry prompt` is new-only and good. Missing: `dry alerts`, `dry trial`, `dry day`. |
| Stores | `core/src/{store,sqlite,indexeddb,storage}.rs` (folder, sqlite, memory, IndexedDB) | `tools/src/store.rs` (md, sqlite) | partial | Store lives in tools here. No IndexedDB or memory store (the browser target needs one). |
| Tests, docs | 1176 tests, `STANDARDS.md`, `docs/parity.md` (+ `core/tests/docs.rs`), `docs/ideology.md` | 142 tests, `docs/{engine-standard,port-draft,eggs-draft}.md` | partial | Not ported: the parity-doc test, the ideology doc and STANDARDS. |
| Book | `archive/book` branch only (commit 8526b27, "WIP parked") | none | missing | The note says it was "built on dev/book", but main has no `docs/book` and no `packages/book`. |

### Where docs and code disagree

- **This repo's README is stale.** It lists core/gpui/server only, says nothing of `crates/tools`, and describes core as "State, Screen, layout".
- **`crates/server` uses actix-web.** The old repo rolled actix back on 2026-11-17 (note `dry-actix-dropped`). Did that decision carry over here?
- **Alert triggers.** `port-draft.md` keeps `at`/`every`/`after`; the code parses only `at` and `every`.
- **The book.** See the last table row: it's on an archive branch, not on main.
- **Old `docs/parity.md`** describes the dev/restructure state of a GPUI client over the old server. The GPUI app here is a separate standalone shell, so that table doesn't carry over.

### Gaps worth porting, ranked

1. **Alert firing** (tick + `dry alerts`) plus the `file` artifact. This finishes engine-standard steps 3-5. Small, and already designed.
2. **Transcript and day file** (`core/src/{transcript,day}.rs`). Pure, easy to port, and the base for away mode and the morning read.
3. **Child engines and inbox** (`engine(name, task)`, one tool). The owner's original goal.
4. **Service host plus one push stream.** Move turns and alerts out of the window, so a closed client doesn't stop the work.
5. **Jobs and resume.** Non-blocking tool groups, plus a turn checkpoint that survives restarts.
6. **Inference breadth.** Anthropic, `claude-cli`, streaming, a usage ledger. Budgets are measured in spend, never steps.
7. **Memory/learning file plus refocus slot.** Covers the "personal memory" part of the three parts.
8. **Approvals and rails.** A gate for anything unattended.
9. **Trials, goal runs and metrics.** Evidence for the night loops.
10. **Telegram and pairing.** Once (4) is in.

## 2. Outside apps

Sources are the official repos and docs (links at the end). Two names were
ambiguous, and both are flagged below.

### T3 Code (pingdotgg/t3code): confirmed

- **A control plane, not a model reseller.** It drives the agents you already pay for (Claude Code, Codex, Cursor, OpenCode, ACP) through normalized provider adapters.
- **One server, many clients.** Desktop, web, iOS and Android all show the same threads. Code and credentials stay on the server machine.
- **A shared RPC contract package** (`packages/contracts`) plus a shared client runtime. Each client adds only its platform pieces.
- **Event sourcing.** A command becomes events. The events, the views built from them, a receipt and the queued side effects are saved in one transaction, and the effects run afterwards. That makes retries safe, and clients replay on reconnect.
- **Checkpoints as hidden git refs.** Each turn that changed a repo gets one. A rewind restores every repo or none, and a provider that can't rewind is rejected before any file changes.
- **Worktree per thread,** plus built-in diff review, commit, push and PR.
- **Model switch and handoff mid-thread** (Claude to Codex). You can approve from your phone.
- **Remote machines** over LAN, Tailscale or T3 Connect.
- **Run settlement on the server.** The server decides when a run is finished, apart from the provider's own turn state, and syncs PR state.
- **dry can learn:** the command → event → effect split for the service, and hidden-ref checkpoints for cheap undo. Keep the owner's critique in mind: rebuild each feature from dry's own primitives (engine, alert, artifact, tool, page, script), not as one coupled orchestrator.

### herdr: confirmed (agent multiplexer)

There are two related projects: `ogulcancelik/herdr` (the original Rust TUI)
and the newer `herdr.dev` / `herdrdev` family. How they relate is unclear.

- It runs inside your own terminal, with tmux-style prefix keys plus mouse split and drag.
- **A status badge on every pane** (working, blocked, idle/done) and a flag when an agent needs input.
- **OSC 7501.** An open status escape that any program or script can send with one `printf`. The older TUI guessed status from process and output.
- **A background server** (herdr-core) keeps terminals alive through detach, SSH drop or app update, and the layout comes back after a reboot.
- **Agents drive herdr too.** Over a CLI and socket they start helpers, send them text, and "wait until another agent is blocked".
- **Many machines** in one list over SSH. Phone apps answer inside the real terminal.
- **Config is plain files,** with an `AGENTS.md` that explains the config to your agent. There's a mods gallery.
- **dry can learn:** one status signal that the dashboard, the Need-you tile and the pets all read; "wait until blocked" as a wake trigger; an `AGENTS.md` per content folder, in the folder-as-content spirit.

### rex: ambiguous, four candidates

I picked **Superlogical's Rex** (Mitchell Hashimoto's terminal and
multiplexer on libghostty, in beta). It's the only notable one, and it shares
the OSC 7501 status protocol with herdr. Its first-party docs aren't public;
the details below come from secondary write-ups. Second choice:
`TristanBietsch/rex` (a Go multi-agent process control tool), whose features
fit dry closely. Owner: tell me if you meant another one.

- **(Superlogical)** No built-in AI. Programs report idle, working, done, blocked or errored through OSC 7501 ("Program Status Protocol").
- **(Superlogical)** `rex skill setup` installs a skill plus a start hook into Claude Code, Codex, OpenCode and Pi.
- **(TristanBietsch)** A kanban of agents: READY (needs you) / WAITING / WORKING / DONE.
- **(TristanBietsch)** A daemon runs each agent in its own pty, with attach/detach and hot upgrade without killing agents.
- **(TristanBietsch)** "Loop until a check passes," per-turn snapshots with undo, and transcript export.
- **(TristanBietsch)** Opt-in folder trust: an agent can't widen its own permissions.
- **(rex-cli / AIOS)** Local project memory searched on demand, and run evidence (commands, exit codes, test output) written per run.
- **dry can learn:** the four-column board as a dashboard view; "loop until check" as a phase exit (not a step cap); per-run evidence files for night loops; a one-shot installer for the skill and hook that makes sessions report status.

### ElizaOS (elizaOS/eliza): confirmed, but now v4 "on Vibestack"

- **A character file** (`character.ts`, formerly json): bio, system, style, examples, topics, plugins, knowledge, templates, env-backed secrets.
- **A plugin is four parts.** Actions (do), Providers (know: ordered by `position`, dynamic or private), Evaluators (learn, after each response), Services (long-lived, `start`/`stop`).
- **The loop:** `composeState()` merges providers into one state; actions are validated and run; evaluators fold facts back into memory.
- **One `Memory` record** (entity, agent, room and world ids, content, embedding). Short-term, long-term and knowledge memory are tags, not separate tables.
- **Search** mixes semantic and keyword, with a match threshold. Context is trimmed to a token budget, and storage goes through an `IDatabaseAdapter`.
- **Tasks:** `TaskWorker` with repeat (`updateInterval`), self-deleting one-offs, and a Jobs API.
- **Clients are plugins:** Discord, Telegram, X, over one messaging and sessions API.
- **A counter-example:** 90+ plugins, the tool sprawl dry avoids.
- **dry can learn:**
  - Evaluators. A post-turn pass that writes capped facts gives personal memory with zero new engine tools.
  - "Service" as a named, lifecycle-managed thing an engine depends on.
  - Clients as adapters over one messaging API.
  - Leave out the plugin sprawl.

## 3. Proposed features for dry-learn

Each line gives: **what** · *why (which note)* · where · size · deps. The
**engine tools** column says how many tools the engine sees (the tool-count
restraint).

### A. Finish the engine standard (first)

| # | Feature | Where | Size | Deps | Engine tools |
|---|---|---|---|---|---|
| A1 | **Alert firing.** The window's 1 s tick and a `dry alerts` command call `alert::due`, append `Alert <name> fired`, and run a turn. Baseline first, wait while busy, missed is missed. *Why: port-draft, super-engine "proactive".* | core `alert.rs` (decide), tools `alerts.rs` (log), gpui tick | S | none | 0 (write the file) |
| A2 | **`file` artifact.** `open`/`read`/`write`/`edit` exist only while it does. *Why: engine-standard step 4.* | core `live.rs` + `tools.rs` | M | none | +4 while held |
| A3 | **`after <engine>` trigger,** then **"when blocked"** (herdr). *Why: port-draft step 6.* | core `alert.rs` | S | A1, B2 | 0 |
| A4 | **Status field in the response contract:** progress / waiting / ready / blocked / failed. Session programs map onto it, and later OSC 7501 from terminals. *Why: dsh "structured reports"; it feeds Need-you, the board and pets.* | core `response.rs`, `dashboard.rs`, `session.rs` | S | none | 0 |
| A5 | **Refocus slot.** When history passes a size, a refocus phase writes a capped summary artifact. Triggered by size, never by a step count. *Why: roadmap "refocus prompt"; the owner designs the prompt.* | core `template.rs`, `prompt.rs` | M | A2 | 0 |

### B. Engines that work together

| # | Feature | Where | Size | Deps | Engine tools |
|---|---|---|---|---|---|
| B1 | **`engines` artifact with one `engine(name, task)` tool.** Posts into the child's inbox and returns its reply. *Why: the owner's original team goal; tool-count note.* | core `live.rs` (`Live::Engines`), `turn.rs` | M | A2 | +1 |
| B2 | **Jobs.** A group of calls (child engines, long tools) becomes one job. A callback wakes the parent when the whole group is done, and new messages are taken meanwhile. *Why: service brief "non-blocking".* | core `job.rs` (pure states), gpui/server runner | M | B1 | 0 |
| B3 | **Sessions board view:** READY / WAITING / WORKING / DONE columns of every engine and session. *Why: dsh "attention first", rex kanban.* | core `dashboard.rs` (a `Board` value), gpui `views/board.rs`, `Component::Board` | S | A4 | 0 |
| B4 | **Evaluator pass ("learn") after each turn.** Writes `memory/learning.md` in three capped sections. Daily "dreaming" later. *Why: three parts = personal memory; ElizaOS evaluators.* | core `memory.rs` (format, cap), a post-turn phase | M | A5 | 0 (not an engine tool) |

### C. Service and clients

| # | Feature | Where | Size | Deps | Engine tools |
|---|---|---|---|---|---|
| C1 | **Event log.** Every command (send, fire, step) becomes events in `convo/events.jsonl`, and `State` is rebuilt from them. Side effects are queued after the write. *Why: service brief; T3 receipts.* | core `event.rs` (pure apply), tools `store.rs` | M | none | 0 |
| C2 | **Core host** (`crates/server`, or the `dry` binary as a daemon). Owns turns, alerts and MCP. GPUI becomes a client over one push stream. *Why: "start as a service, a closed client does not stop the work".* | tools/server | L | C1 | 0 |
| C3 | **Transcript and day file** ported from the old pure modules; `dry day`. *Why: away redesign, 9.5/10 acceptance.* | core `transcript.rs`, `day.rs`; tools CLI | S | none (C1 helps) | 0 |
| C4 | **Telegram client** that reaches the translator only; `/day` read-only. *Why: away mode, ideas arrive while out.* | tools `telegram.rs` | M | C2, C3 | 0 |
| C5 | **Second renderer: a TUI** from the same `Component` list. *Why: rebuild direction "one description, two renderers"; stakeholders liked the TUI.* | new `crates/tui` | L | C2 | 0 |

### D. Developer that grows

| # | Feature | Where | Size | Deps | Engine tools |
|---|---|---|---|---|---|
| D1 | **Checkpoints as hidden git refs** per turn that changed files, with all-or-none rewind. *Why: T3; cheap trust for night runs.* | tools `git.rs`, core `checkpoint.rs` (refs as values) | M | A2 | 0 (UI action) |
| D2 | **Worktree per developer engine** (`workspace: worktree` header) plus a diff/Changes component. *Why: developer that grows; old `projects.rs`.* | core `diff.rs` (port), tools `git.rs`, gpui `views/diff.rs` | M | D1 | 0 |
| D3 | **Trials:** `dry trial <name>`. A goal plus checks, a throwaway state, a fake or real model, and a `report.md` that gives the verdict first. *Why: goal runs, setups, "work is rated".* | new `tools/src/trial.rs` + core `trial.rs` (spec, check) | M | A1 | 0 |
| D4 | **Loop blueprint:** stages, trigger, spend budget, morning check, as one md file shown as a block diagram. *Why: the super-engine "loop builder".* | core `blueprint.rs` (port), gpui view | L | A1, B2, D3 | 0 |

### E. Inference

| # | Feature | Where | Size | Deps | Engine tools |
|---|---|---|---|---|---|
| E1 | **More `Inference` implementations:** Anthropic Messages and `claude-cli`, keyed by `models.json`. | core `inference.rs` | M | none | 0 |
| E2 | **Streaming** (SSE parse in core, bytes in gpui `net.rs`). | core `sse.rs` | S | E1 | 0 |
| E3 | **Usage ledger plus spend budget per engine.** Shown and paced, never a step cap. *Why: super-engine budgets, capacity balancing.* | core `ledger.rs`, tools append | M | E1 | 0 |

### F. UI and content

| # | Feature | Where | Size | Deps |
|---|---|---|---|---|
| F1 | **Cmd-K finder** over routes, engines, artifacts and skills (from T3's wish list). | core `search.rs`, gpui view | S | none |
| F2 | **Voice recording wired** (`voice.json` record → transcribe → composer). | gpui `program.rs` reuse | S | none |
| F3 | **The cat in the footer** (eggs-draft), after scenes land. Pure state machine plus `FrameScene` in core, quads in gpui. | core `scene`/`pet` (coordinate with the scenes agent) | M | scenes |
| F4 | **Motion tokens** in `theme.rs` (from the ideology). | core `theme.rs` (after the themes agent) | S | themes |
| F5 | **`AGENTS.md` per content folder** (`routes/`, `themes/`, `components/`), so any coding tool can add one correctly (herdr). | docs only | S | none |
| F6 | **Book seed** (`docs/book/`), started from the archived branch. Also port the parity-doc test so the docs can't drift. | docs + a core test | M | none |

Kept out on purpose: trading anything; a plugin marketplace (Eliza); a
step-limit anything; WebMCP, JEV and decision paths (the owner hasn't
designed them).

## 4. Suggested order

1. **A1** alert firing, plus **A4** status field. Small, and they finish what's already parsed.
2. **A2** `file` artifact. Closes engine-standard steps 3-5.
3. **C3** transcript and day file. A pure port, and an early win for away.
4. **B1** `engines` artifact plus **B3** board.
5. **C1** event log. Lay it under the state before the host exists.
6. **E1/E2** Anthropic, `claude-cli`, streaming.
7. **C2** core host, with GPUI as a client. Then B2 jobs and C4 Telegram.
8. **D1/D3** checkpoints and trials. They give night loops their evidence.

## 5. Open questions for the owner

1. Is `crates/server` (actix) the future host, or should the host be the `dry` binary as a daemon? The old repo dropped actix; is that still the call?
2. Alerts while the window is closed: `dry alerts` now, the host later? (`port-draft` Q1.)
3. Should status be a response field (A4), a terminal escape (OSC 7501), or both?
4. Does personal memory live under `state/<id>/memory/`, and who caps it: a fixed line count, or the refocus prompt you're designing?
5. Event log (C1): is jsonl in `convo/` acceptable beside `messages.md`, or should messages become a view of the events?
6. Rex: did you mean Superlogical's Rex, or another one?
7. Approvals: build a minimal approve/deny gate before unattended scripts, or wait for JEV?
8. Should the book come back from `archive/book`, or start fresh here?

Sources:
- T3 Code: https://github.com/pingdotgg/t3code, internals at https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md
- herdr: https://github.com/ogulcancelik/herdr, https://herdr.dev, https://github.com/herdrdev
- Rex: https://www.superlogical.com/ (secondary: https://www.mindstudio.ai/blog/rex-terminal-mitchell-hashimoto/), https://github.com/TristanBietsch/rex, https://github.com/rexleimo/rex-cli
- ElizaOS: https://github.com/elizaOS/eliza, https://docs.elizaos.ai/ (character-interface, plugins/components, agents/memory-and-state, guides/background-tasks, plugin-registry/platform/telegram)

## 6. Decided (owner, 2026-11-22)

1. **Host.** The goal is the app running on a server, with the native
   mobile app connecting to it over a direct tunnel or connection. Today
   the host is also the client (the GPUI window runs the turns); that
   splits into a host and clients. The host is the `dry` binary as a
   daemon or `crates/server` (actix), chosen per target.
2. **Live updates and alerts** reach clients over a live connection: SSE
   or a WebSocket, whichever the target fits. Alerts fire on the host.
3. **State is visible.** Every engine or session shows what it is doing
   as it works. For a local model engine: calling the LLM, calling
   tools, calling the LLM again; each state has its own background
   colour (tokens) in the UI. A Claude session reports what it can.
   Built: `core/activity.rs`, tokens `activity_*` (engine standard, [Activity](engine-standard.md#3c-activity-what-an-engine-is-doing-now)).
4. **The only limit is the model's context.** Before every LLM call the
   rendered prompt is measured; at about 75% of the model's context
   window, the history is compressed. No step limits.
   Built: `core/context.rs` (engine standard, [The context window](engine-standard.md#3b-the-context-window-the-only-limit)).
5. **Rex** is a multiplexer like herdr (the TristanBietsch / Superlogical
   question is closed: same family as herdr).
6. **Approvals and jev** come later; plans exist.
7. **The developer loop** (triage → look → plan → code → test, notes back
   to triage), how the tester sees the running app, and where tools come
   from (`mcp.json` general, `tools.md`/`tools/` custom): see
   `docs/developer-loop.md`. Claude sessions get `mcp/settings.json`;
   a model switch keeps the context setting (switching is for cost).
