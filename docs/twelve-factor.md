# dry against the 12-factor agents

A review of dry-learn's architecture against HumanLayer's
[12-Factor Agents](https://github.com/humanlayer/12-factor-agents)
(README, factors 1–12 and appendix 13), judged first by the owner's own
criteria. Read-only on code. Written 2026-11-23, while another agent was
editing `crates/core/src/{element.rs, prompt.rs, state.rs, files.rs,
shipped.rs, environment.rs}`: **line numbers below are from that day and
may have moved.** A claim with no file reference was checked in the docs
only. Anything marked *unverified* was not checked in code.

## The twelve factors, in short

| # | Factor | What it asks |
|---|---|---|
| 1 | Natural language to tool calls | The LLM's job is to turn a request into structured output (a call). Deterministic code then runs it. |
| 2 | Own your prompts | Write and version the prompt yourself, as you would code. Don't hide it behind a framework's black box. |
| 3 | Own your context window | Build the context yourself in whatever dense format works (history, results, errors, docs). Filter out what doesn't help. Everything is context engineering. |
| 4 | Tools are structured outputs | A "tool" is just JSON the code switches on. What the code does with it (run it, pause, ask someone) is the code's choice. |
| 5 | Unify execution and business state | Derive "where are we" (step, waiting, retries) from the event history, not a second store. Keep outside state to a minimum, so the thread can be serialised, resumed and forked. |
| 6 | Launch / pause / resume with simple APIs | Anything can start an agent. It can pause for long work and resume from an outside trigger (a webhook), including between a call being chosen and being run. |
| 7 | Contact humans with tool calls | Asking a person is a structured call (question, options). The thread is saved, the loop ends, and the answer is appended when it comes. |
| 8 | Own your control flow | Your code decides what each call does: loop on cheap sync calls, break out for risky or long ones, and add your own summarising, approval and waiting. |
| 9 | Compact errors into the context | Put a failed call's error back in the context so the model can recover. Cap consecutive errors (about 3), then escalate. |
| 10 | Small, focused agents | 3–20 steps, one job. Agents are parts of a mostly deterministic system. |
| 11 | Trigger from anywhere | Start agents from chat, email, cron, events, outages, and reply on the same channel. Outer-loop agents run long and call a human at key points. |
| 12 | Stateless reducer | The agent is a fold: `(state, event) → next state`. *(The page is mostly two diagrams of a `foldl`.)* |
| 13 | Pre-fetch context (appendix) | If the model will almost surely need some data, fetch it before the call and put it in the context, rather than spending a round trip on it. |

## 1. Verdict table

| Factor | dry today (refs) | Verdict | Owner's criterion |
|---|---|---|---|
| 1 NL → tool calls | The head (translator) only turns asks into `engine_start` / `engine_call` / `post` / `reply` (`default/engines/translator/spec/goal.md` Scope: "no action tools"). An answer is `thought` + `act` in the call grammar, parsed by `response.rs` and `calls::parse`, and run by `turn::act_step` (`turn.rs:208`). | **meets** | "The desk loop only translates natural language into loops" |
| 2 Own your prompts | One builder, `prompt::render_with` (`prompt.rs:90`). Every word comes from a spec file (`goal.md`, `template.md`, `responses.md`, skills) or a `ToolSpec`. The default ships only the head; children get their spec at runtime from `engine_start(goal, scope, done, template)` (`engines.rs:294`). The prompt view shows exactly what is sent. But some prompt text is still written in Rust: tool descriptions (`tools.rs:25-160`), `snapshot::TOOLS_LINE` (`snapshot.rs:127`), the guided-post wrapper (`desk.rs:239`) and the bounce text. | **meets, with Rust-held text** (gap 3) | "Owner owns every bit of the prompt" |
| 3 Own the context window | The element walk: a fixed order of sections, then rank, then id (`element.rs:442` `gather`; docs/elements.md). Artifacts are snapshots, never history (engine-standard §1). Compression at 75 % keeps the full log, with a summary at each cut (`context.rs:37`, engine-standard §3b). Phases send only the current phase's goal and tools (`phase.rs:157`). Learning keeps a turn-start copy. The rendered size is measured before every call. | **meets, goes further** | "Context window owned perfectly"; phases; live snapshots |
| 4 Tools = structured outputs | A call is parsed, then sorted as *instant* (run in core, in written order) or *waiting* (terminal `run`, MCP). Results come back as one tool message in written order (`turn.rs:208-330`). `reply` is a tool, not a special field (`tools.rs:5`). | **meets** | Component/context-based |
| 5 Unify execution state | Most of "where are we" lives in the engine's own files: `convo/messages.md`, `convo/run.md` (phase, step, skills loaded, `waiting:`, `heard:`; `files.rs:21`, `files.rs:353-362`), artifact state, terminal `.pid`/`.exit` files. But turns in flight, statuses, activity, approvals and the SSE ring live in host memory only (`host.rs` `Inner`; docs/status.md "Approvals live in host memory only"). | **partly** (gap 4) | "Unify execution state": next focus |
| 6 Launch / pause / resume | Launch: `POST /api/engines`, `/messages`, `/threads/:root/posts` (`server/src/main.rs:9-37`). Durable pause: an `engine_call` wait is saved (`waiting:`) and resumed by `settle` → `Runner::resume` (`host.rs:746-772`, `runner.rs:135`); a wait on its own job (`start(..., wait="yes")`, `waiting: term#3`) is saved the same way and ends with a wake when the job ends, after a restart too. Missing: a turn cut off mid-LLM-call by a restart does not resume, and there is no stop or pause API for a running turn (no route, *grep found none*). There is no pause between choosing and running a call, except a session's approval (`host.rs:1192`). | **partly** (gap 5) | "Launch/pause/resume: the restart saves the latest state" |
| 7 Contact humans with tools | `reply(ask=…)` gives a typed control (`ask.rs`). The answer comes back as the next message via `POST /api/engines/:id/answers`. A post `to=you` calls no engine. Approval notices are posted to you; a session's approvals wait for `POST /api/approvals/:id`. | **meets** | Owner as the one who directs |
| 8 Own control flow | The guided-message rule: only an addressed post wakes an engine; an unaddressed one bounces (docs/desk.md). The instant/waiting rule, callbacks queued while busy, phases with `goto_phase`, per-template tool narrowing, `allow:` rules. Gaps: an actuator is refused rather than paused (`capability/mod.rs:842-857`). A terminal `run` has no approval at all (engine-standard, terminal "Safety"). | **meets, with gaps** | "Constraints the owner sets take full effect" |
| 9 Compact errors | Every failed call is `call → error: why` in the tool message (`turn.rs:254, 271, 328`). An answer that does not parse, or has no calls, gets a corrective line (`turn.rs:221-224`). `collect(fixes)` pairs a failure with the later fix. But a failed model call ends the turn with `Status::Failed` and nothing goes in the history (`runner.rs:263-265`). Repeats are not counted: on purpose (no step limits); stuck loops are "handled another way", not built. | **partly** (gap 6) | "Errors appended back" |
| 10 Small, focused agents | Children made from templates: researcher, developer (triage → code → test), reviewer. Each has narrowed tools and phase skills (`engines.rs:603-640`). One head manages them. No step caps (owner's rule). | **deliberately different** | "Army of loops under one high-level loop" |
| 11 Trigger from anywhere | Built: you (UI, HTTP, posts); callbacks (terminal `start`, `engine_start`); capability **events** (edge-triggered `when:` in declared capabilities, fired on the host's 2 s tick: `host.rs:315-337`, `host.rs:774-788`). Not built: **alerts do not fire** (`alert.rs:29-31`, docs/parity.md:177). No cron, no watcher element, no inbound channel (Telegram, email, webhook). Events skip the thread (gap 2). | **gap** (gap 2) | "Alerts: cron jobs or watchers that wake on any trigger" |
| 12 Stateless reducer | Core is pure: `Runner::next(Came) → Next` (`runner.rs:1-60`); `turn::step` works on `&mut Engine`; the element walk is deterministic, with no ties left to chance. The same engine in the same state sends the same prompt, and a test compares the old and new renderers byte for byte. One exception: `State::push` resets the phase on **every** user-role message, callbacks included (`state.rs:303-313`), so the next state depends on *how* an event arrived (gap 1b). | **meets, one bug** | "Given all elements and direction, the next step is predictable" |
| 13 Pre-fetch | Artifact snapshots *are* pre-fetch: file, terminal, children, thread, environment, clock and capability snapshots are collected at render, with no tool call (`live.rs:153-165`). The head's `environment` line comes the same way (docs/elements.md phase 1). | **meets** | Live artifacts give snapshots |

## 2. The gaps, by value

### Gap 1: "Run one loop completely" is proved once, by hand, and has two breaks

The owner's basic expectation is that the app can run one loop completely.
Here is one loop traced through today's code:

| # | Step | Where (file:function) | State |
|---|---|---|---|
| 1 | You write a goal in natural language on a thread | `server/src/main.rs` `posts` → `Host::post_yours` (`host.rs:1046`); the composer always sends `to` | ✅ built, tested (`a_guided_post_starts_its_engines_turn_and_nothing_else_does`) |
| 2 | The head wakes on the guided post | `Host::settle` → `turn::callbacks` → `desk::guided` (`desk.rs:239`) → `add_text` → `Host::begin` → `Runner::start` | ✅ |
| 3 | The prompt is rendered | `prompt::render_with` → `element::gather` / `collect` | ✅ tested (old renderer equals new) |
| 4 | The head loads `prompting`, then makes a child with goal/scope/done and a template (its phases) | `turn::run_call` (skill) → `live::call` → `engines.rs:294` (`engine_start` with `template=`, `root=`) → `engines::apply` (`engines.rs:396`) | ✅ unit-tested with literal calls (`engines.rs:603`); 🟡 the model's choice of template is tested only in the run below |
| 5 | The head replies with the brief | `reply` → `Outcome::Reply` | ✅ |
| 6 | The child works through its phases in the environment | `Host::spawn` → `Runner` → `turn::act_step`; file tools (`file.rs`), terminal (`terminal.rs`), `goto_phase` (`phase.rs:135`) | ✅ fake-model tests per piece (`turn.rs:527` phases, `turn.rs:662` file, `turn.rs:696` terminal) |
| 7 | Errors feed back | `turn.rs:254, 271, 328` | ✅ for calls. ❌ a model-call failure ends the child's turn (`runner.rs:263`): the parent gets `error: x failed: why` for an `engine_call`, but an `engine_start` child that fails is reported only as a status (*unverified* whether its parent is woken) |
| 8 | A long job calls back | `start` → `terminal::settle_engine` → wake → `add_wake` | ✅ a wake keeps the phase (`State::add_wake`). ~~A turn ended only with a reply or a failure, so a developer that started a long job replied first and its parent heard twice~~: **closed (2026-11-23)**: `start(command, wait="yes")` ends the turn with no reply, the engine waits on the job (`waiting: term#3`, saved), and the job's end wakes it in the same phase (`terminal::settle_waited`, `turn::callbacks`). Tested end to end (`tools/tests/one_loop.rs`) and per rule (`turn.rs` tests) |
| 9 | The child reports | `reply` ends its turn; `engines::settle` (`engines.rs:437`) wakes the head with `engine <x> replied: "…"` (cut at 400 characters; the whole reply is in the `children` snapshot) | ✅ tested (`engines.rs:688`) |
| 10 | The head judges | `skill(name="judging")`; then accept, send back (`engine_call` to the same child) or a reviewer child | 🟡 no automated test; prompt-only |
| 11 | Done: the report to you | `reply` → SSE `turn` event | ✅ |
| — | The whole chain | docs/default.md §5: one scratch run with claude (model id `sonnet`) was correct end to end; with haiku it ran but skipped phases and invented facts | 🟡 **proved by hand, not a test**. No fake-model test drives head → child → phases → reply → judge → report |

**Smallest fix (component/context-based, no new tool):**
1. In `State::push`, reset the run only for *your* messages, not
   callbacks, events or guided posts from engines. The push needs to know
   the message's origin: an origin field on `Message`, or a separate
   `add_callback`. A callback is not a new direction; the phase is part of
   the state the next step reduces over.
2. One scripted-model test (`tools/tests/`), modelled on
   `one_step_many_calls.rs`. It drives a translator and a developer child
   through every step above against a temp project, and asserts the
   thread, `run.md` and the head's final reply. This is the "one loop
   completely" acceptance test the owner asks for.
3. A model-call failure in a child posts `error: … failed: why` to its
   parent the same way for `engine_start` as for `engine_call`.

### Gap 2: Triggers. Alerts are designed but never fire, and events skip the thread

- **Closed in part, 2026-11-23:** alerts fire. `alert::settle` is looked
  at in `turn::callbacks` with the other callbacks, and the alerts
  artifact brings `alert(name, trigger, text, once?)`, so an engine sets
  its own next look (docs/engine-standard.md, `kind: alerts`;
  docs/loop/journal.md §14). Open: `watch` triggers, and a fired alert
  wakes by `add_text`, not by a post on the thread.
- Capability events wake engines by `add_text` straight into their
  history (`host.rs:315-337`), not by a post on the thread. That breaks
  "an engine is only called when its thread has a guided message"
  (docs/desk.md). The event doesn't show on the Desk thread, and it resets
  the phase (gap 1).
- No inbound channel (Telegram, email, webhook) and no long-running
  watcher.

Why it matters: "there should also be alerts: cron jobs or long-running
watchers that wake on any trigger". The design is in §4.

### Gap 3: Tool descriptions are not self-describing elements; there are five sources

The owner wants every tool to bring its own description and its runner
through one standard spec, so nothing is hard-coded or coupled. Today a
tool is described in one of five places:

| Source | Where | Tools |
|---|---|---|
| `ToolSpec` tables, central | `tools.rs:25` `BUILTIN`, `tools.rs:55` `BROUGHT` | reply, write, read, list; script, analytics, goto_phase; **and** the file, terminal, browser, evidence, engines, shared and learning tools, even though those modules hold only a name list (`file.rs:42` `TOOLS`, `terminal.rs:37`, …) |
| Artifact text | `skill.rs:116` | skill, skill_save |
| Declared capability file | `capabilities/*.md` `## tool:` | battery.status, gpio.set, … (the standard the owner describes: doc, inputs, command, timeout, approval) |
| MCP schema | `mcp.json` servers; `tools/src/worker.rs:301`; `tools/src/app_tools.rs:217, 281` | outside tools; the session `engines` group; app/settings tools |
| A hand-written line | `snapshot.rs:127` `TOOLS_LINE` | the same session tools, described a second time |

Dispatch is also partly by name outside the tool's module:
`turn::run_call` matches `skill`, `skill_save`, `goto_phase` and
`script` by name (`turn.rs:380-417`). `early.rs:39` lists which tools may
start early. `learning.rs:284` lists which tools are "facts".
`turn.rs:283` lists which ones are instant. These are properties of the
tool held elsewhere.

**Smallest fix:** one `ToolSpec` shape for every source (name,
description, inputs, *sort* = instant | waiting, *early* yes/no,
*fact* yes/no, *actuator* yes/no). Each module owns its own spec next to
its code (`file::SPECS`, …). `tools::describe` only gathers. The
declared-capability `## tool:` format already is this standard, so make it
the text form, and generate `TOOLS_LINE` from the worker group. Nothing
changes in the prompt sent (the element-walk equality test guards it).

### Gap 4: Execution state is split between files and host memory

See §5. In short, approvals, statuses, an in-flight turn and the event
ring are memory only. A phone or CLI that reconnects after a restart sees
the files' truth, but not a pending approval or why an engine failed.

**Smallest fix:** persist the host's `status` per engine as a line in
`convo/run.md` (`status: failed: why`). Persist approvals in
`state/approvals.md`, one line each. Both are ordinary engine state, read
back by `Host::open`.

### Gap 5: A turn cut off by a restart is not resumed; there is no stop

`Host::open` (`host.rs:263`) loads the state and starts the tick. It
starts no turn. An engine whose last message is a user or tool message (a
turn cut off mid-call) sits idle until something else wakes it. There is
no route to stop or pause a running model turn.

**Smallest fix (no tool):** on open, for each engine whose history ends
on a user or tool message and that has no `waiting:`, record
`status: interrupted` in `run.md`. Then either resume it by the same path
as `settle → begin(resume)`, or (owner's choice) post on its thread to
`you`: "translator was cut off at step 3; reply `resume` to go on". Stop:
`POST /api/engines/:id/stop` sets a flag the runner reads between moves
(`Runner::next` already returns between every move), ending with
`Status::Stopped`. This is an intent from a client, not a model tool.

### Gap 6: A model-call failure is not context; stuck loops have no handling

`runner.rs:263-265`: a failed HTTP call, or a body that doesn't read, ends
the turn, and the next turn's model never learns it. Repeated identical
failing calls are not detected (no step cap is the owner's rule, so this
should not be a count).

**Smallest fix:** an `errors` element (artifacts section, empty when
clean). It shows the last failure of the model call and any call that
failed with the same error N times in a row ("`run cargo test` failed the
same way 3 times"). It is information in the context, not a limit, which
matches factor 9's "control how errors are represented" without a step
cap.

### Gap 7: No approval for a terminal `run`, and actuators are refused, not paused

An unattended engine (alerts, night loops) can run any command
(engine-standard, terminal "Safety"). An actuator call returns an error
telling the model to ask (`capability/mod.rs:851-854`), instead of
pausing between selection and execution as sessions do (`host.rs:1192`).

**Smallest fix:** the same `Approval` record sessions use, for model
engines. A call that needs approval becomes a *waiting* call (the turn's
existing wait sort), resolved by `POST /api/approvals/:id`. `allow:` in
`engine.md` covers `run(...)` patterns too.

## 3. Tool inventory, and the owner's small core

Every tool an engine can be offered today (`[x]` = what offers it):

| Owner's verb | Tools today | Offered by | Notes |
|---|---|---|---|
| **create engine** | `engine_start`, `engine_call` | `engines` artifact (`engines.rs:51`); worker MCP group | Two tools for one verb: make/message a child, then wait or call back. Could be one `engine(name, …, wait=yes/no)` |
| **write goal** | goal/scope/done are inputs of `engine_start` | same | An engine cannot edit its own goal (by design). The head writes a child's goal only at creation; there is no "rewrite a child's goal" (*unverified* whether a second `engine_start` with `goal=` replaces it; the doc says an existing child just gets the message) |
| **create phase** | `template=` input of `engine_start` (a kit's name or raw `template.md` text, `engines.rs:294-316`); `goto_phase` to move | `engines`; a phase brings `goto_phase` (`phase.rs:55`) | **Exists only for a child at creation.** An engine cannot add or change its own phases, and no tool changes a child's template later |
| **create tool** | `skill_save` (a skill: instructions plus a tool list); scripts are **files only** (`script.rs`, no save tool); declared capabilities are **files only** (`capabilities/*.md`); MCP servers are **files only** (`mcp.json`) | skills element | **Does not exist as an engine ability.** The nearest is `skill_save`, which bundles existing tools and cannot define a new one |
| **change UI** | `page_create`, `page_add`, `page_delete`, `component_create`, `token_set`, `token_reset`, `theme_use`, `list` | `dry mcp app` (`app_tools.rs:281`), for sessions only (*unverified* whether any model engine is offered them) | A second, separate tool set. Not offered to the head |
| **manage context** | `write`, `read`, `list` (plain artifacts); `collect`, `learn`, `analytics` (learning); `skill` (load); `edit_post`; settings `engine_settings`, `engine_model`, `engine_set`, `models` (`app_tools.rs:217`) | builtin; learning artifact; skills; `dry mcp settings` | `read` duplicates what the snapshot already shows for live artifacts; `analytics` is listed both in `BROUGHT` and `learning::TOOLS` |
| **act in environment** | `open`, `focus`, `edit`, `save` (file); `run`, `start`, `background` (terminal); `browse`, `click`, `type` (browser); `evidence`; `script`; every declared, compiled or remote capability tool; MCP tools | each artifact / capability | `run`/`start`/`background` are one verb with three wait modes. `open`/`focus` could be one |
| **talk on thread** | `post`, `edit_post`, `reply` (with `ask=`, `show=`); worker: `thread`, `post`, `state`, `artifact`, `approve` | `shared` artifact; builtin; worker MCP | The worker's `state`/`artifact`/`thread` reads repeat what snapshots carry for model engines (they're needed because a session has no element walk) |

Count: 26 distinct core tools for model engines (4 builtin, 3 brought,
4 file, 3 terminal, 3 browser, 1 evidence, 2 engines, 2 shared,
3 learning, 2 skills, minus `analytics` counted twice), plus 7 worker
MCP tools and 12 app/settings MCP tools for sessions. The head holds only
`reply` and its artifacts' tools (`engine_*`, `post`, `edit_post`, plus
`skill`), which is the owner's restraint working.

**The smallest set that covers the owner's core verbs** (proposal; every
other capability stays an element that brings its own self-described
tools):

| Verb | One tool | Folds in |
|---|---|---|
| create engine / write goal / create phase | `engine(name, goal?, scope?, done?, template?, text?, wait?)` | `engine_start` + `engine_call`; a second call with `goal=`/`template=` rewrites that child's spec (the parent owns its children's prompts) |
| create tool | `define(kind, name, text)`, where kind is `skill`, `script`, `capability`, `alert` or `watch` | `skill_save`; makes scripts, declared capabilities and alerts engine-writable *as spec files*, checked by their existing parsers. Actuators and the head's own spec stay the person's |
| talk on thread | `post(text, to)` and `reply(text, ask?, show?)` | `edit_post` becomes `post(n=…)` |
| manage context | `skill(name)`, `learn(…)`, `goto_phase(name)` | `collect` becomes an input of `learn`; `analytics` a learning collection; `read`/`list` only for plain artifacts |
| change UI | none for model engines: an `ui` element (pages, tokens) whose tools appear only when the head holds it in `spec/elements.md` | the app MCP group, the same specs |
| act in environment | from the element held (file, terminal, browser, …), with one `ToolSpec` standard | `run`/`start`/`background` → `run(command, wait=yes|callback|no)` |

This is a proposal only; it changes prompts, so each fold needs the
owner's yes, and a measured run with the local model, before it lands.

## 4. Where dry goes further than 12-factor

All of these were checked in code or tests:

- **The element walk** (`element.rs:442`; docs/elements.md). Every part
  of the prompt is an element with a `Place`, sorted by section, rank and
  then id. 12-factor says "own your context"; dry makes the order a
  tested invariant (`the_element_walk_sends_the_same_prompt_as_the_slots`).
  Each element says why it is empty instead of being silently dropped.
- **Phases** (`phase.rs`, `template.rs`). A second loop above the react
  loop, sending only the current phase's goal, tools and skills. This is
  factor 10's "small agent" achieved *inside* one engine, without capping
  steps.
- **Live artifacts as snapshots** (`live.rs:153-165`). State is rendered
  fresh each step and never enters the history, so the prompt holds only
  the latest snapshot (factor 13 by construction, and factor 3's density).
- **Instant then waiting calls** (engine-standard §3d). A deterministic
  rule for many calls in one step: results come in written order, and a
  waiting call always sees its act's edits. Early start while streaming
  gives the same results as without it (`early::tests`).
- **Threads with directed posts and the guided-message rule** (docs/desk.md,
  `desk.rs:239`). Agent-to-agent contact is a structured, addressed event
  on one shared list per root. A post with no `to` bounces to its sender
  (factor 7 generalised to agents).
- **Durable waits** (`files.rs:358`, `runner.rs:135`). `waiting:` and
  `heard:` in `run.md`, and the children's reply counts in the `engines`
  artifact, mean an `engine_call` survives a restart and each reply or
  post is delivered once.
- **Environment as an element**, and capabilities that are compiled,
  declared or remote, each with probes and edge-triggered events
  (docs/capabilities.md). The model sees where it runs and what it can
  use, without a tool call.
- **Compression that keeps the full log** (`context.rs`): a summary block
  at each cut, measured before every call.

## 5. Triggers and alerts

### What exists

| Trigger | Wakes by | Persists across restart |
|---|---|---|
| Your post / message | guided post → `desk::guided` → `add_text` → turn | yes (`shared.md`, `heard:`) |
| Child reply (`engine_start`) | `engines::settle` wake | yes (reply counts in the `engines` artifact) |
| Child reply (`engine_call`) | `engines::settle` resume | yes (`waiting:`) |
| Terminal `start` ends | `terminal::settle_engine` (`.exit` file) | yes; died with the app → `stopped` |
| Capability event (`## event:` `when:`) | `Host::deliver_events` → `add_text` into every user's history (`host.rs:315-337`) | edge state in memory only (*unverified* whether the first reading after a restart re-baselines; the rule "first reading never fires" suggests it does, so an edge that happened while the host was down is lost) |
| Alert (`at`, `every`) | fired on the callbacks tick (`alert::settle`, 2026-11-23); made by the person's file or the engine's `alert` tool | the marks (first seen, last fired) in `convo/run.md`; a moment missed while no shell ran is not fired late |
| `spans::Trigger::Schedule` | defined for analytics, never produced (docs/analytics.md:121) | — |

### Design: alerts and watchers as elements that post to a thread

The owner's rule set gives the shape: few tools, everything an element,
wake only by a guided post.

1. **`alerts` element** (artifacts section, rank 650, held by listing
   `alerts` in `spec/elements.md`). It is backed by the engine's
   `spec/alerts/*.md` (built parser) plus one new trigger form: `watch
   <capability> <when>`, which reuses `capability::When`. Its snapshot has
   one line per alert: next fire, last fire, missed with why. Tools: none.
   The head makes one with `define(kind="alert", …)` (§3), or the person
   writes the file.
2. **Firing on the host tick** (`Host::tick`, every 2 s, already running):
   for each held alert, `alert::due(now)` with the existing rules
   (baseline first, missed is missed with `GRACE`). A fire is **a post on
   the engine's root thread, author `alert:<name>`, `to` = the holding
   engine**. It then wakes through `desk::guided` like any post. So the
   Desk shows it, it obeys the guided rule, and it is delivered once.
   Capability events move to the same path: author `event:<cap>/<name>`,
   addressed to each user engine, instead of `add_text`.
3. **Persistence:** add `last: <unix>` per alert to `convo/run.md`, or to
   an `alerts.state` line beside the spec, so the baseline and "missed is
   missed" hold across a restart. The thread entry is the durable record
   that it fired. A watcher's edge state (true/false at its last reading)
   goes in the same line, so a condition that was already true before a
   restart does not fire again.
4. **Long-running watchers** are declared capabilities with an
   `interval:` and `## event:` (built), held by an engine. A watcher on a
   peer fires on the peer, and its event rides the stream (built). The
   only new part is "post, don't `add_text`".
5. **Inbound channels** (Telegram, email, webhook) are capabilities
   whose event text is the message. A reply goes out through the same
   capability's actuator tool, behind approval. No new core concept.
6. **Unattended safety first:** gap 7 (approval for `run`) lands before
   any alert can wake an engine that holds a terminal.

## 6. Unified execution state, and launch / pause / resume

### Stored where

| State | Where | Survives restart |
|---|---|---|
| Engines, order, parents | `state/app.md` | yes |
| Spec (goal, template/phases, skills, tools, responses, elements, alerts, scripts, mcp) | `state/engines/<id>/spec/` | yes |
| History, summaries | `convo/messages.md` | yes |
| Phase, step, loaded skills, `waiting:`, `heard:` | `convo/run.md` | yes |
| Session program report | `convo/meta.json` | yes |
| Artifacts (file tabs, terminal sessions, children, plan, notes) | `artifacts/<name>.md`, terminal logs with `.pid`/`.exit` | yes |
| Learning | `memory/learning.md`, `.log` | yes |
| Thread | `state/engines/<root>/shared.md` | yes |
| Spans (analytics) | per-engine log, appended by `Host::record` | yes |
| Turn in flight (runner, model call, waiting calls) | host memory | **no**: lost, and nothing resumes it |
| Status (`failed: why`, `needs you`) and activity | host memory (`Inner.status`, `turns`) | **no** |
| Approvals | host memory | **no** (docs/status.md) |
| SSE event ring (`Last-Event-ID`) | `Live` in memory | **no**: a client after a restart reloads |
| Capability edges, peers | memory | **no**; peers rejoin |

So execution and business state are mostly unified in the engine folder,
which is what factor 5 asks. The memory-only rows are the gap.

### What an outside client can do today

Through `dry-server` over HTTP with a token (`--bind`, `--token`): list
status (`GET /api/status`), follow everything live (`GET /events`),
read an engine's start, turn, artifact or thread (`GET
/api/engines/:id/read`), start a thread or session, send messages and
posts, answer asks, decide approvals, delete, set format, mode and scene,
and reset. This was proved from a Linux container and a fresh browser
(docs/status.md, "Code-server mode"). The server's HTML pages are the
hosted client.

Not yet: a phone client, and the wasm app as a remote client (its
`TextStore`/`Transport` over HTTP is not built; docs/status.md). GPUI
follows a host only on the same machine. No terminal shell. There is no
stop, pause or resume intent, and no way to list or edit alerts or
`spec/elements.md` from a client (they are edited as files).

### The gap, as the owner states it

"Access and control the application correctly from anywhere it is
reached" needs:

1. the memory-only rows made engine state (gap 4);
2. intents for **stop** and **resume** (gap 5). Pause is then "stop at
   the next move boundary, keep `run.md`". Resume is `begin(resume)`;
3. every client reading the same thing. The `read` route already returns
   the rendered start and the artifacts, so a phone needs only the SSE
   stream plus `read`; no new state API.

## 7. Open questions for the owner

1. ~~Callbacks and events reset the phase today (gap 1, step 8). Is that
   ever wanted, or should only *your* message restart the run?~~
   **Decided and built (owner, 2026-11-23): a wake keeps the phase**, your
   thread posts included (`State::add_wake`); only a direct new ask or a
   parent's engine_start/engine_call restarts it. Proven by
   `crates/tools/tests/one_loop.rs`.
2. After a restart, should a cut-off turn resume by itself, or post to
   you and wait?
3. Should engines be able to `define` scripts, alerts and declared
   capabilities as spec files (§3), or stay read-only on all tool
   definitions, as they are now (except skills)?
4. Should the head be able to rewrite a child's goal or template after
   creation, or only message it?
5. Are capability events allowed to wake an engine, or must they always
   go through the head (posted `to` the head, which decides)?
6. The fold proposals in §3 change prompts. Measure each one on the local
   model first, or take them together in one rebuild?
7. Should the "one loop completely" scripted test use the shipped
   translator and developer spec verbatim (it breaks when a skill
   changes), or a frozen copy?
