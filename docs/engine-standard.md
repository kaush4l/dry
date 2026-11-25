# The engine standard (draft)

One shape for every engine, so the loop is deterministic and the guide can
teach it once. Sources: the old dry `STANDARDS.md` (spec vs
state, phases, fixed prompt elements) and dsh `docs/004-prompts.md` (a
template of parts, generated sections last, overrides remember their base).

## 1. Four folders

```
state/
  app.md       the engines in order; a child says its parent: `- helper (child of engine-1)`
  shared.md    the top level's shared context (`kind: shared`)
state/engines/<id>/
  spec/        shareable: the agent itself. Copy it and you have the engine.
    engine.md        header: name, description, kind (model | session), inference;
                     `allow:` rules and `approve_timeout:` for approvals
    goal.md          the goal, in sections: Goal, Scope, Done; an old
                     instruction.md is read while there is no goal.md
    template.md      optional: the prompt's layout, and its phases
    skills/<name>/SKILL.md  its skills
    responses.md     the answer contract (keys + descriptions, format)
    tools.md         the direct tools it holds
    artifacts.md     the artifacts it starts with: kind + name + settings
    mcp.json         outside tools
  convo/       local: what happened, and where the run is.
    messages.md      user / assistant / tools blocks, summary blocks at cuts
    meta.json        what a session program reported
    run.md           phase, step in phase
  memory/      local and personal: what it learned (`kind: learning`)
    learning.md      three sections, capped: user, environment, lessons
    learning.log     one line per change
  artifacts/   local: the engine's artifacts, each one's current state
    <name>.md        a text artifact's text; a kind's state (`kind: file`, ...)
    <name>/          a terminal artifact's session logs
  shared.md    the shared context of its children's level (`kind: shared`)
```

- **Children live at the top too.** A child engine ([artifacts](#4-artifacts-things-that-add-to-several-slots), `kind: engines`)
  has its own folder beside every other engine's; only `app.md` says
  whose it is. So the shell runs, saves and shows it as any engine.
- **Memory is not spec.** `memory/` is personal and changes while
  running; it is never shared with the spec, never shipped, and a template
  or a child starts with none.

- **Spec is instructions only.** Nothing personal and nothing that changes
  while running lives in `spec/`. Sharing an engine = sharing `spec/`.
- **History is what was said.** The model reads it; nothing in it is
  rewritten by the app. Compression ([the context window](#3b-the-context-window-the-only-limit)) adds a `### summary` block at a
  cut and never removes a message: `convo/messages.md` is the full log.
- **Artifacts are what is true now.** An artifact and a "live artifact" are
  the same thing: every artifact is shown fresh each step as a snapshot of
  its state, never put into history. A plain text artifact is the simplest
  kind (its snapshot is its text).

## 2. The template: fixed slots, filled by contributors

The prompt is one template with slots in a fixed order. Order never
depends on content, so the same engine in the same state sends the same
prompt (determinism), and the stable part stays first (cache).

Agreed order (owner, 2026-11-22): spec elements, tools, artifact
snapshots, history, response contract.

| # | Slot | Filled from | Changes |
|---|---|---|---|
| 1 | instructions | `spec/goal.md`, its sections in order | when the person edits it |
| 2 | learning | `memory/learning.md`, as it was when the turn began (`kind: learning`) | between turns |
| 3 | phase | the template's phase sections + `convo/run.md`: the phase's goal, step n of N | per phase |
| 4 | tools | direct tools + every artifact's tools + the phase's tools | when artifacts come or go |
| 5 | artifacts | every artifact's snapshot (text now; images once vision is standard), clock last | every step |
| 6 | history | `convo/messages.md` | every step |
| 7 | response | `spec/responses.md`, appended to the newest message | when the spec is edited |

Trade-off noted: snapshots before history means a state change re-sends
the history after it (no prompt-cache reuse past slot 4). Fine for local
models; revisit if a paid API's cache cost matters.

- **Learning keeps its turn-start copy.** A `learn` call writes the file
  at once, but the slot sends the file as it was when the turn began
  until your next message, so slots 1 to 3 are the same text all turn.
  The learning artifact's snapshot shows the change. Not compressed:
  capped (at most 4400 characters, about 1100 tokens). Empty: not sent,
  and the view says why.

- **One builder.** `prompt::render` is the only place a prompt is made;
  the Response view shows exactly what it sends.
- **Slots are element sections** (built 2026-11-23, docs/elements.md
  phase 0: `core/element.rs`). Each slot above is a section of
  elements (the goal, learning, the phase, the toolbox, the outside
  tools, each MCP tool, each artifact, the skills, the summary, the
  messages, the contract). `render` gathers them, sorts by
  `Place { section, rank }` then id, collects each one's `Contribution`
  and joins a section's parts under its title and lead; the prompt sent
  is the same as the slots made it (a test compares the two).
- **Every slot, always.** An empty slot is shown in the view with why
  (no phases, no artifacts), never silently dropped.
- **`spec/template.md` lays it out.** One `##` section per slot, in the
  fixed order; each starts with its metadata (`key: value` lines), then
  its text. Every section is optional: a slot left out keeps its default,
  and no file at all is the built-in default. The order cannot change
  (a section out of order is an error that names the order).
- **Phases are metadata driven.** The phase table at the top holds every
  phase's metadata, one row each; every row has its `## phase: <name>`
  section (the goal), name for name and in the same order. A row with no
  section, a section with no row, or a different order is an error that
  names it. An empty cell or a missing column is the default.

  ```markdown
  | phase  | tools      | response | skills |
  |--------|------------|----------|--------|
  | triage | read, list | standard |        |
  | code   |            |          |        |

  ## instructions

  ## learning

  ## phase: triage
  Sort the request.

  ## phase: code
  Make the change.

  ## tools
  title: Your tools
  Call these in `act`:

  ## artifacts
  ## history
  ## response
  ```

  | section | metadata | text |
  |---|---|---|
  | phase table (top, optional) | columns `phase`, then any of `tools`, `response` (standard \| jev), `skills` | none |
  | instructions, learning, tools, artifacts, response | `title:` (its heading) | the lead at its top (learning's default: `What you learned so far:`) |
  | `phase: <name>` (one per table row) | its table row | the phase's goal |
  | history | none | none |

- **Later:** an override records the default it was based on, so a changed
  default can be shown as "default updated" (dsh).

## 2a. The goal: `spec/goal.md`, in sections

Written by the person (or, for a child, by its parent when it makes it);
the engine never edits its own. Built: `core/goal.rs`. The sections
matter (owner, 2026-11-22): each has one meaning and one place in the
instructions slot, sent in this order whatever the file's order:

```markdown
## Goal
Keep the dry-learn tests green.

## Scope
Run tests and read code alone. Ask before changing a public API. Never commit.

## Done
Every test passes; reply with the command you ran and its last line.
```

| section | means | sent as |
|---|---|---|
| Goal | what it is for, who it serves | `### Goal — what you are for, and who you serve` |
| Scope | what it does alone, when it stops and asks, what it never does | `### Scope — …`, second |
| Done | what a finished result looks like, how it is shown | `### Done — …`, last |

- A heading matches in any case. A `# title` above the sections is fine.
- Reported in the prompt view's notes (`goal.md: …`), the way the
  template's problems are, never silently: a missing section; a section
  that is none of the three (not sent); one given twice (the second not
  sent); text before the first section (not sent).
- **No sections at all** (an old `instruction.md`): all of it is read as
  the Goal, and the note says so.
- **The rename:** the loader reads `instruction.md` while there is no
  `goal.md`; the prompt view says `read from spec/instruction.md`, and the
  next save writes `goal.md` (and drops `instruction.md`).
- A card's one line is the Goal section's first line.

## 3. Phases: the loop around the loop

An engine always runs one loop: steps, each an observation and calls
(a react loop). Phases add a second loop on top for real workflows
(triage → code → test, plan → build → verify): each phase brings its own
prompt element (slot 3) and its own tools, works in the inner loop, and
moves with `goto_phase(name)`: on when it is done, or back when the work
needs it (test fails → code). A phase can be mandatory work: the next one
only starts when the engine moves there.

Each phase also says how it is answered (`response:`):

- `standard` (default): the engine's response contract, read from the
  model's text.
- `jev`: a newer, much faster response standard, for phases whose work is
  well known with few paths. Planned: it needs a newer API and models that
  support it. Today a `jev` phase is accepted and answered the standard
  way, and the prompt view says so. The prompt view's `jev:` line says
  whether the step could be answered by a JEV endpoint, from metadata
  (see [JEV](jev.md)).

Phases are the phase table of `spec/template.md` (the metadata) and its
`## phase: <name>` sections (the goals), matched by name:

```markdown
| phase | tools      | response |
|-------|------------|----------|
| plan  | read, list | standard |
| build |            |          |

## phase: plan
Read what is there and write the plan as the `plan` artifact.

## phase: build
Do the plan, one step at a time.
```

- No phase sections = one implicit phase; right for most engines.
- Each phase: a goal (its text), optional `tools:`
  that exist only in that phase, optional `response:` (standard | jev).
- `goto_phase(name)` is a tool every phase gets while there are two or
  more; it moves `convo/run.md` to that phase, its steps from zero. The
  work ends, as every turn does, with `reply`.
- In the prompt the phase slot is a plan: every phase on one line (the
  direction), the current one expanded with its goal (what to do now).
  What was done so far is in the history, read after it.
- **No step limits.** Not per phase, not per turn: a turn runs until
  `reply`. The plan shows which step of the phase it is, as information
  only. Stuck loops will be handled another way (owner, 2026-11-22), not
  by a count.

## 3b. The context window: the only limit

A turn has no step limit; the model's context window is the only one
(owner, 2026-11-22). Built: `core/context.rs`.

- **The window.** `context` (tokens) on the model setting (`models.json`,
  copied to `engine.md` as `context: 32768`). Not set: the app asks the
  server once per model before the first call: LM Studio
  `GET {root}/api/v0/models` (`loaded_context_length`, the size it is
  loaded with, else `max_context_length`), then Ollama
  `POST {root}/api/show` (`num_ctx` in `parameters`, else
  `model_info.<arch>.context_length`). Neither: 8192, shown as *assumed*.
- **Measured before every LLM call.** The rendered prompt's size is an
  estimate (a quarter of the characters, plus 4 tokens per message),
  deterministic and cheap. The Response view shows it, e.g.
  `prompt ≈ 12.3k of 32k tokens (38%)`, with where the window came from.
- **At 75%, the history is compressed.** Core decides (`context::plan`),
  the shell makes the call. The same model gets a compression prompt and
  the older messages; its summary goes into the log as a `### summary`
  block at the cut (`context::apply`). Kept verbatim: the newest messages
  (up to 20% of the window, at least the newest one) and the current user
  message, even when it is older than the cut. A summary is at most 8% of
  the window; a second compression summarises the first summary with what
  came after it.
- **The full log stays.** Decided: *keep every message and mark the cut*
  (not a separate `archive.md`). `convo/messages.md` has every message,
  with each `### summary` block where its cut was made. The prompt's
  history slot is the newest summary (at the end of the system prompt,
  under `## Earlier conversation (summary)`, so chat templates that want
  one system message still get one) and every message after it.
- **Only the history.** The spec slots, tools and artifact snapshots are
  never compressed. When they and your message alone are at 75%, nothing
  is compressed: the Response view says so and the prompt is sent as it
  is. A prompt still over after one compression is also sent as it is, so
  nothing loops.
- **While it runs** the engine's activity is *compressing* ([activity](#3c-activity-what-an-engine-is-doing-now)).

## 3c. Activity: what an engine is doing now

Built: `core/activity.rs`. One value per engine, shown as a label and a
translucent background tint (theme tokens): a chip under Home's tiles, its card on
`/desk`, its LIVE nav entry and the top of its page:

| activity | label | token |
|---|---|---|
| idle | `idle` / `ready` | none |
| calling the LLM (step n) | `calling the LLM`, then `calling the LLM again (step 2)` | `activity_llm` |
| running tools | `running tools: read`, `running tools (5): open ×3, engine_call ×2` | `activity_tools` |
| compressing | `compressing the history` | `activity_compress` |
| waiting for a terminal callback | `waiting for session 2 \`cargo test\`` | `activity_wait` |
| waiting for a child's reply (`engine_call`) | `waiting for helper's reply` | `activity_wait` |
| a session's program working | what it reports, else `claude: working` | `activity_work` |
| needs you | `needs you: no model` | `activity_need` |
| failed | `failed: why` | `activity_fail` |

- The shell runs a step in phases and the window repaints between them:
  the context check (and compression), the HTTP call (`llm`), the calls
  of the answer (`tools`, by name), then `llm` again until a reply.
- It extends `card::Status`: `Activity::status()` is what the dots, tiles
  and the scene's mood read (`dashboard::live_status`), so they agree.
- A session program reports only when its run ends, so a session engine
  shows `working` until then (a program that writes an `activity` value
  is shown as it is, once one streams).

## 3d. One step, many calls

Built: `core/turn.rs` (`begin_step`, `run_wait`, `finish_step`,
`callbacks`), `tools/live.rs` (`run_waits`), `gpui/main.rs`
(`run_calls`, `wait_calls`, `finish_calls`). Owner, 2026-11-22: "all tool
calls are invoked immediately. So only waiting happens in parallel."

A step's `act` may hold many calls: several `open`s, several
`engine_call`s exploring at once, terminal `run`s, MCP calls. No limit on
how many: the context window is the only limit ([the context window](#3b-the-context-window-the-only-limit)), and nothing about
safety needs a count (each call is checked on its own: paths stay in the
root, a child cannot be its own parent, a child already asked in this
act is refused).

1. **Every call is invoked at once.** No call waits for another to
   finish before it starts.
2. **Only the waiting is parallel.** Calls are of two sorts:
   - **instant**: pure state changes, done in core with no waiting:
     `open`, `focus`, `edit`, `save`; `write`, `read`, `list`; `post`,
     `edit_post`; `collect`, `learn`; `skill`, `skill_save`,
     `goto_phase`; `engine_call` and `engine_start` (they record the ask;
     the child is made and started by the shell); `start` and
     `background` (they spawn and return); `reply`.
   - **waiting**: a terminal `run`, and every call that goes outside (an
     MCP tool). A waiting `engine_call` is instant to make and waits
     separately, for the child's reply (point 3).

   **The rule: the instant calls first, in the order written; then every
   waiting call launched together.** So `open(a); edit(a, …); save(a);
   run("cargo test")` tests the saved file, and no waiting call ever
   sees half of its act. The rule is by sort, not by place: in
   `run("cargo test"); edit(a, …); save(a)` the run also sees the saved
   edit. To run against the file as it was, run it in an earlier step.
   The step takes as long as its slowest waiting call, not their sum.
   - A **script** is one call whose steps run in order, so it is instant
     to the act and its own waits wait inside it, one after another (a
     script's steps may depend on each other).
   - **MCP**: one client is one stdin/stdout pair, so calls to one server
     take turns (a lock per server, `mcp::Servers`); calls to different
     servers run at the same time.
3. **Collect the evidence, then loop.** When every waiting call is back,
   all the results are one `Role::Tool` message, one line per call
   (`call → result`), **in the order the calls were written**, whatever
   order they finished in. The next LLM call reads it.
   - **Children** (`engine_call`): the instant half makes and asks every
     child of the act, and the shell starts them all at once, each its
     own turn (`ask`, `ask_step`), while the act's waiting calls wait.
     Each child's line is `waiting for x's reply` until its reply fills
     it (`x replied: "…"`); the parent's turn goes on when every child it
     waits for replied or failed, so their replies are in the one
     message, in written order. A child that fails gives
     `error: x failed: why`, never a stuck parent.
4. **Long work calls back; it never waits.** Terminal `start` and
   `engine_start` give an immediate `… started …` result. When the work
   is done, the owning engine is called with the result attached: a
   message naming the call and its result
   (`session 2 \`cargo test\` finished, exit 0 (terminal term)`,
   `engine docs replied: "…" (engines kids)`), and a new turn runs.
   - **Waiting on its own job** (`start(…, wait="yes")`): the turn ends
     after that step with no reply and nobody is told; when the last
     waited job ends, its result wakes the engine in the same phase
     (docs/desk.md, "Waiting on its own job").
   - **One callback path**: `turn::callbacks` gathers an engine's
     children and terminal sessions in one place; the window runs it on
     its 1 s tick and whenever a turn ends (so a waiting parent hears at
     once, not a second later).
   - **Queued while busy**: an engine in a turn takes nothing; what
     finished stays where it is, and the first time the engine is free
     every callback that came is delivered together, as one message and
     one turn. Each callback comes once.
5. **No step limits**, and no cap on the calls in one act.

How the shell runs a step (`gpui/main.rs`):

```text
answer ─▶ begin_step (background, on a copy)  instant calls in order;
          │                                   waiting ones prepared
          ├─▶ copy back; children asked start now (each its own turn)
          ├─▶ run_waits: one thread per waiting call, all joined
          └─▶ finish_step: results merged in written order, one message
                 ─▶ LLM again │ wait for children │ reply
```

While it waits the engine's activity names the calls, repeats counted:
`running tools (5): open ×3, engine_call ×2`.

**Early start while streaming** (`core/early.rs`, [streaming replies](#4e-streaming-replies-built-corestreamrs-coreearlyrs)): when the answer
streams, an instant call of the sort that touches only the engine and its
files (`open`, `focus`, `edit`, `save`, `write`, `read`, `list`,
`goto_phase`, `skill`) runs as soon as its `)` arrives, still in written
order; a waiting call keeps its place for the act's end; the first call of
any other sort (`reply`, `post`, `engine_call`, `start`, `script`, …)
stops the early run, and it and everything after it run when the act ends.
They run on a copy with file writes held: only when the whole answer reads
with the same first calls is the copy kept and the writes made; else the
step runs as it would have. A step's results are the same with and without
early start (`early::tests`).

Open: a script with a `run` step makes the act's other waits start after
it (a script is a sequence by definition); an HTTP tool would be one more
waiting sort when one exists.

## 4. Artifacts: things that add to several slots

An artifact is a typed thing an engine holds. It can contribute to more
than one slot:

- **tools** (slot 4): the tools that control its state. They exist exactly
  while the artifact does.
- **snapshots** (slot 5): what its state is now, collected at render. A
  snapshot is text today. Images are planned: once vision is a standard
  model feature, a snapshot may be an image; until then a text-only
  representation (or a one-line description) stands in.
- **its state** (`artifacts/<name>.md`): what the tools change.

Artifact kinds are a closed enum in core (one variant per kind = one
curated way to add a kind); the shell does the I/O and hands snapshots in,
so core stays pure. The kind is the first line of the file
(`kind: file`); no line means a plain text artifact.

**Everything is a capability** (docs/capabilities.md, built 2026-11-23).
An artifact's first line names the capabilities it uses: `uses: [file,
clock]`, `uses: [battery]`. `kind: x` is still read, as an alias
(`kind: learning` is `[learning, analytics]`), so existing specs and state
load unchanged. The kinds below are the compiled capabilities that run in
core. A declared capability (`capabilities/<id>.md`) or a remote one (a
peer that joined over MCP) adds its doc, its snapshot and its tools
(`<id>.<tool>`) to the artifact's section and the tools slot. **An absent
capability is not rendered**: it is not in the prompt and brings no tools,
and an artifact whose capabilities are all absent leaves the artifacts
slot. A call to an outside capability is checked in core (inputs, and an
`allow:` rule for an actuator), waits like an MCP call, and is routed by
the shell's registry to whoever provides it. The turn loop never names a
runtime.

**The rule for every kind: a tool call changes the state and its result
is short; the content goes in the snapshot.** History says "opened
turn.rs"; the file's text is in the artifacts slot, fresh at every step.
So the model never reads stale content from history, and history stays
small.

**Every artifact has a page of its own** (`/desk/:id/artifacts/:name`,
the `artifact-page` component; see [developer-loop.md, The Code page](developer-loop.md#2b-the-code-page-is-the-human-view-of-the-developer-artifacts)). The engine
page lists each artifact as one row (its kind, name, state in a few words,
tool count) and Open goes there. The page shows the artifact's tools with
descriptions, its state laid out by kind, and its snapshot exactly as
sent, all from the exact view (`core/inspect.rs`). A new kind adds a
`Detail` variant and a summary line in `inspect.rs` (with a test), and
one layout in `gpui/src/views/artifact.rs`; until it has one it shows as
text.

**A kind may attach a live view** (`core/liveview.rs`): `Live::view`
names the parts of a workbench that show it, renderer-neutral
(`liveview::Part`: explorer, editor, panel, plus chat and status in every
view). `file` attaches the explorer and editor, `terminal` the panel; the
other kinds attach none. Its page then has Live view ›
(`/desk/:id/artifacts/:name/live`), and an engine holding any has one
workbench of them all (`/desk/:id/live`); see [developer-loop.md, The Code page](developer-loop.md#2b-the-code-page-is-the-human-view-of-the-developer-artifacts).
A new kind that attaches one adds its parts to `Live::view` and what they
show to `liveview::build` (with a test).

### `kind: file` — the file system, live (built: `core/file.rs`)

What the files on disk are now, and which ones the engine has open.

- **State** (`artifacts/<name>.md`): the root, the open files, each with
  an optional focus (a line range, for a big file), and unsaved edits,
  each its `old` and `new` text in a `~~~` block (a text line starting
  with `~~~` gets a `\`):

  ```
  kind: file
  root: .
  open: crates/core/src/turn.rs 40-90
  open: README.md
  edit: README.md
  ~~~ old
  # dry
  ~~~ new
  # dry-learn
  ~~~
  ```
  `root:` is relative to the repo (`.`) or absolute. Only the owner sets
  it: `write` refuses a file or terminal artifact (making or replacing
  one), so its state changes only through its tools.
- **Tools** (exist only while the artifact does; with two artifacts of a
  kind, a call names one with `artifact="..."`). Results are one line:
  - `open(path)` → `opened crates/core/src/turn.rs (412 lines)`. At most
    32 files open; one more closes the oldest without unsaved edits
    (`…; closed a.rs (32 open at most)`).
  - `focus(path, from, to)` → `focus turn.rs 40-90 (412 lines)`: show only
    those lines (opens the file if needed).
  - `edit(path, old, new)` → `edited turn.rs (unsaved: 1 edit; save(path="turn.rs") writes it)`.
    The file must be open and `old` must be in it (with its unsaved
    edits) exactly once; otherwise the result says which.
  - `save(path)` → `saved turn.rs (1 edit, 414 lines)`. If the file changed
    on disk so an edit no longer applies, nothing is saved and that
    file's unsaved edits are dropped (said in the result), so a stale
    edit never blocks a file.
- **Snapshot**, every render, read from disk (the live state), at most
  24 000 characters:
  - `root: . — files (* open):` then up to 200 file names, the top
    folder first, the open ones marked `*`;
  - every open file with line numbers, `#### path — 412 lines`, all of
    it or only its focus; unsaved edits laid over it, their lines marked
    `*`; past the size bound it is cut with how to `focus` on the rest;
  - an image or binary file: one line until vision lands.
- **Safety:** a path is relative to the root with no `..` (an absolute one
  only inside an absolute root); the shell also follows links and
  refuses one leading out. Files whose names say they hold secrets
  (`.env*`, `*.pem`, `*.key`, `id_rsa*`, `credentials*`, `.netrc`, …) are
  never opened and not listed.
- **UI**: the Code page's explorer lists, under the tree, each file
  artifact an engine holds (`engine · name — 2 open`); a click opens its
  open files as tabs: the same files the engine sees.

### `kind: terminal` — the developer artifact (built: `core/terminal.rs`)

The engine's terminal sessions: what is running and what last ran.

- **State**: one line per session: its number, how it was started
  (`run | start | background`; `start` asked for a callback), its status
  (`running`, `exit:<code>`, `stopped` = ended with no code), when it
  started (unix seconds) and its command (`\n` for a line break). At most
  30; the oldest finished go first.

  ```
  kind: terminal
  root: .
  session: 1 run exit:0 1795356330 cargo test -q
  session: 2 start running 1795356400 cargo build --release
  ```
  Its output is in `artifacts/<name>/<session>.log` in the engine's
  folder, with `<session>.exit` (the exit code, when it ends) and
  `<session>.pid`, so a session's status is on disk and survives the
  app restarting; one that died with the app reads as stopped.
- **Tools** (results one line, `run` with a short tail):
  - `run(command)` — waits; `session 1 \`cargo test -q\`: exit 0` and the
    output's last 15 lines.
  - `start(command)` — `session 2 \`cargo build\` started; a message comes when it ends`.
    When it ends, a **callback**: the message
    `session 2 \`cargo build\` finished, exit 0 (terminal term)` is added
    as a message and the engine runs a turn, the way an alert does.
  - `background(command)` — `session 3 \`npm run dev\` started in the background (no message when it ends)`;
    its state is only in the snapshot.
- **Snapshot**: each running session as a terminal (`#### session 2 —
  start, running 1m 15s`, `$ command`, its last 30 lines), then the last
  finished one with its exit code and last lines, then the earlier
  finished ones by name only (the last 8, and a count of older ones).
- **The callback** ([one step, many calls](#3d-one-step-many-calls)): `turn::callbacks`, on the window's 1 s tick
  and whenever a turn ends, asks each idle engine's terminals which
  sessions ended (`terminal::settle_engine`, marking each once, so a
  callback comes once) and wakes the engine with the messages, together
  with any child replies, as one message. An engine mid-turn is left, and
  gets everything at once when it is free, so a callback never races a
  turn. No step limits: a callback is a new turn, not a budget.
- **`run` in two halves** ([one step, many calls](#3d-one-step-many-calls)): `terminal::prepare_run` records the
  session as running and gives the job; the shell waits on it with the
  act's other waiting calls; `terminal::finish_run` marks its exit and
  writes the result line.
- **Safety:** commands run with `/bin/sh -c` in the root, with no input,
  and without the environment variables whose names say they hold
  secrets (`*KEY*`, `*TOKEN*`, `*SECRET*`, `*PASSWORD*`, …), so no command
  can print one into a log. **There is no approval step yet:** whatever
  the model asks for runs. An engine that runs unattended (alerts,
  callbacks, the night) will need one before it may run commands.
- The shell does the I/O (`dry-tools` `live::Disk`: read, list, write,
  spawn, logs); core holds the state, applies the calls and renders the
  snapshot, all pure, through `live::Host`.

### `kind: engines` — engines as tools (built: `core/engines.rs`)

An engine can have tools, and an engine can be a tool (owner,
2026-11-22): a parent makes child engines, each with a goal, and talks to
them. Per the tool-count rule the tools come from this artifact, not from
every engine's toolbox.

- **State**: one line per child: its id, how the parent last asked it
  (`call` waits, `start` calls back) and how many of its replies the
  parent has had (so each reply is had once, across restarts too).

  ```
  kind: engines
  child: helper call 1
  child: docs-checker start 0
  ```
- **Tools** (results one line; the replies are in the snapshot):
  - `engine_call(name, goal?, scope?, done?, text?, template?, skills?, kinds?)`
    makes the child if it is new (`goal` needed then) and gives it `text`
    (else: "Work on your goal (see your instructions), then reply with the
    result."). Result `waiting for helper's reply` and the step returns
    `Step::Wait`: the parent's turn pauses (activity `waiting for helper's
    reply`). When the child replies, that line becomes
    `helper replied: "turn.rs:40, a missing trim."` (cut at 400 characters
    with a note; all of it is in the snapshot) and the turn goes on. If the
    child fails, `error: helper failed: why`.
  - `engine_start(...)`, the same arguments: `made helper with your goal and
    started it; a message comes when it replies` (an existing child: `sent
    helper your message; …`). When it replies, a **callback**: the message
    `engine helper replied: "…" (engines kids)` is added and the parent
    runs a turn, as a terminal `start` does.
  - A child still working is refused (`helper is still working; its reply
    comes first …`), so a message never races its turn. A name that is the
    engine itself or one of its parents is refused (`an engine cannot be
    its own child`), as is one that is another engine not its child.
  - Replying while an `engine_call` waits ends the turn; that child then
    calls back like an `engine_start` one. So does a new message from you.
- **Where children live**: in the same flat list as every engine, each in
  its own folder at the top of the store, the parent in `app.md`
  (`- helper (child of engine-1)`). Removing an engine removes its
  children (and theirs) and their shared context.
- **What a child gets**: its `goal.md`, written from `goal`, `scope` and
  `done`; the parent's model and settings (inference, sampling, context),
  response contract, format and own tools; a `kind: shared` artifact; and
  if asked, a `template` (its `template.md`, checked), some of the
  parent's `skills`, and more `kinds` (engines, learning, clock), so a
  child may make its own. **No step limit and no depth limit**; only a
  child that would be its own parent is refused.
- **Snapshot**: each child with its activity, reply count, whether the
  turn waits for it, its Goal line and last reply (up to 2000
  characters), then **their shared context** (the parent reads its
  children's level).
- **How the shell runs it**: the step gets a `Family` (what the shell
  knows of the others: children and their activity, the level's shared
  context, the ids taken, the ancestors) and leaves `effects`;
  `engines::apply` makes them (children made, messages added, posts
  kept) and the shell runs each child's turn through `ask`, the same
  `ask_step` path as any engine. The children of one act start together,
  right after the act's instant half, while its waiting calls wait
  ([one step, many calls](#3d-one-step-many-calls)). `turn::callbacks` (the 1 s tick, and whenever a turn ends) runs
  `engines::settle` for each parent not in a turn: an `engine_call` gets
  its result and the parent's `ask_step` goes on once every child it
  waits for is in; an `engine_start` one wakes it with a message. A turn
  waiting for a child counts as a turn for other callbacks, so nothing
  else races it; they come after, together.

### `kind: shared` — the thread of one root (built: `core/shared.rs`)

Every engine of one root's tree (the root, its children, theirs, sessions
and their child engines) shares one thread with the user: a list of
messages, which is how they talk to each other (docs/desk.md). The root is
the thread's **head**. Each child gets a `kind: shared` artifact (`thread`
is its other name, `dashboard` the old one); a root sees its thread when it
holds one.

- **State**: at the root, not in the engine (the artifact holds only its
  `kind:` line): `state/engines/<root>/shared.md`. One line per entry: time,
  author (`you` for the user), and who it is for (`→ tester`):

  ```
  - 1795356330 helper → translator: The failing test is turn.rs:40.
  - 1795356400 tester → helper: All 9 pass with that fix.
  ```
- **Routing**: a post is for the engine it names, whose turn starts with it
  (`core/snapshot.rs`, `inbox`; `core/desk.rs`, `guided`). A post with no
  `to` wakes nobody: it bounces back to its sender to post again with an
  addressee and the context it needs. Authors need not be engines: a
  session's sub-agents post as read-only participants (docs/sessions.md).
- **Tools**: `post(text, to)` → `posted entry 3 (420 characters on the
  thread)`; `edit_post(n, text)` → `edited entry 2 (…)`, with `text=""`
  `removed entry 2 (…)`, only an entry you wrote (`entry 1 is a's; you
  can edit only yours`).
- **Cap** (changed 2026-11-23, [docs/sessions.md, Threads](sessions.md#4-threads-lists-of-messages)): the thread keeps every
  entry; a post is never refused for room. `MAX_CHARS` (6000) is how much
  of the newest a snapshot shows, with `(N older entries not shown; the
  thread keeps them all)`. An entry is at most 600 characters. Before, a
  level was capped at 4000 and a post that did not fit was refused: that
  fought a list that grows with every report and approval.
- **Snapshot**: who is on the level, then the newest entries numbered with
  time, author and addressee, as they are now: in the artifacts slot, never
  in history, never compressed. Posts in one step are applied to the level
  as edits (`engines::apply`), so two siblings stepping at once both keep
  theirs.

### `kind: learning` — learning through curated tools (built: `core/learning.rs`)

`memory/learning.md`, three sections (`user`, `environment`, `lessons`;
caps 1200, 1600, 1600; one line of at most 300 characters each), sent in
the Learning slot ([the template](#2-the-template-fixed-slots-filled-by-contributors)). Learning comes from curated tools that collect
only the context a section learns from (owner, 2026-11-22), not a pass
over the whole history:

- `collect(what)`: the artifact shows that collection from now on,
  gathered live from the history (the newest 8):

  | what | collects | feeds |
  |---|---|---|
  | `corrections` | what the person said right after one of your replies, with that reply (callbacks are not the person) | `user` |
  | `fixes` | a call that failed, paired with the later call of the same tool that worked | `lessons` |
  | `facts` | results of tools that look at the world: terminal `run`, file `open`/`focus`/`save`, MCP, `engine_call`, scripts | `environment` |

  Result: `collected 1 fixes (failed calls paired with the call that
  later worked); they are in the learn artifact, for learn(section="lessons", ...)`.
- `learn(section, entry, old?)`: add (`added to lessons (45/1600)`),
  replace the entry `old` names (`replaced in user (…)`), or remove it
  with `entry=""` (`removed from environment (…)`). **A full section
  refuses** (`lessons is full: 1578/1600 used, this needs 103; merge two
  entries into one with old="...", or remove one …`); nothing is cut.
- **Every change is logged**: a line in `memory/learning.log`
  (`<unix> learn lessons add: …`, `… replace: new (was: old)`).
- **Snapshot**: each section's room, the change this turn (the slot shows
  the turn-start copy), the collection.
- A section the file has that is none of the three is kept as written,
  not sent, and said in the snapshot.

Other kinds: clock, scripts, alerts (built).

### `kind: alerts` — what wakes an engine by itself (built: `core/alert.rs`; firing 2026-11-23)

An alert is a file (`spec/alerts/<name>.md`: `trigger:` `at 09:35`,
`at 09:35 weekdays`, `at 2026-11-24 09:35` or `every 30m`; `once:`; the
body is the message). The artifact shows each on one line with how often
it has fired, and brings one tool:

```text
alert(name="check-build", trigger="every 30m", text="Look at the build; tell the person only if it broke.")
alert(name="check-build", text="")          removes it
```

- **Firing** is in the one path every shell already ticks for callbacks
  (`turn::callbacks` → `alert::settle`, with the host's wall clock,
  `Host::clock`): an alert that is due wakes its engine as a child's reply
  does, with `Alert check-build fired (every 30m): …` as the message of a
  new turn. Nobody writes to it. It works the same on the host, in the
  window and in the browser page (while the page is open).
- **The rules** are the old project's: an alert never fires at the moment
  it is made; a moment nobody looked at within 120 s is missed, said in
  the snapshot, never fired late; an engine in a turn is looked at when
  the turn ends. `once: true` and a dated `at` are removed when they fire.
  At most 8 alerts an engine.
- When each was first seen and last fired is kept in `convo/run.md`
  (`alert: <name> <created> <last> <fired> <missed>`), so a restart does
  not fire a moment twice.
- The shipped translator holds one alerts artifact; its goal says when to
  set one (docs/default.md §2). Tests: `alert::tests`,
  `turn::tests::an_engine_sets_its_own_alert_and_is_woken_by_it_with_nobody_writing`.


## 4b. Skills: instructions with metadata and tools

A skill is the standard `SKILL.md` (a `---` header with `name`,
`description`, optional `tools` or `allowed-tools`, then instructions),
kept in `spec/skills/<name>/SKILL.md`, so existing skills drop in as they
are (`dry skill import <engine> <path>`; only SKILL.md is copied, and the
command names any other files it leaves behind). It is an element:

- **artifacts slot:** every skill the engine may use now, by name and
  description (a broken one says why);
- **tools slot:** `skill(name)` loads one: its instructions come back as
  the result and its tools are the engine's until your next message
  (kept in `convo/run.md`); `skill_save(name, description, text, tools?)`
  creates or updates one, for an engine whose tools list it.
- **per phase:** `a, b` in a phase's `skills` column limits what that phase
  sees; without it, every skill.

**A skill can bring tools of its own** (built 2026-11-23,
`core/skilltool.rs`). Its `tools:` line only names tools that exist
already. Tools nobody else has go in `tools.md` beside `SKILL.md`, written
as a declared capability's tools are ([capabilities.md](capabilities.md)):

```markdown
spec/skills/python/tools.md
# python
Check, run and test Python files in the project's root.

## tool: check
Compile one Python file without running it.
command: ["python3", "-m", "py_compile", "{file}"]
inputs:
- file (string, required): the file, relative to the root
```

- The heading is the skill's name. While the skill is loaded each tool is
  the engine's, called `<skill>.<tool>` (`python.check(file="tree.py")`),
  listed in the tools slot with its inputs, and named in what `skill(name)`
  gives back. It goes with the skill at the next message.
- **A skill tool is a command in the engine's terminal.** The call is
  checked against the inputs, becomes the command with each input as one
  whole quoted argument (never shell text), and runs as a `run` of the
  terminal artifact: in the project's root, with a session, its exit code
  and last lines as the result. So it works wherever a terminal does (a
  host, a companion's folder, the browser's VM). An engine with no terminal
  artifact is told so.
- A template carries them too (`templates/<t>/skills/<s>/tools.md`), and a
  child made from it gets them. A file whose heading is not the skill's
  name, or that does not read, brings nothing and says why.
- The first shipped one is the developer's `python` skill (`check`, `run`,
  `test`). Tests: `skilltool::tests`,
  `turn::tests::a_loaded_skills_own_tool_runs_in_the_terminal_and_is_gone_with_the_skill`.

## 4c. Engine settings: model and sampling

An engine's inference (`spec/engine.md`) is its model, its endpoint
(`api`, `base_url`, `key_env`: the variable's name, never the key) and its
sampling settings. A sampling setting is a `key: value` line, the value as
JSON; a key that is not there is the server's default and is **not sent**,
so an engine with none set sends the same request as before.

```
temperature: 0.1
seed: 42
stop: ["END","###"]
param.min_p: 0.05
```

| key | range | honoured by |
|---|---|---|
| `temperature` | 0 to 2 | OpenAI chat + responses (not reasoning models), LM Studio, Ollama, vLLM, llama.cpp |
| `top_p` | 0 to 1 | the same |
| `top_k` | whole, ≥ 0 | LM Studio, vLLM, llama.cpp; OpenAI refuses it; Ollama's `/v1` ignores it |
| `max_tokens` | whole, ≥ 1 | all; sent as `max_output_tokens` on the Responses API |
| `seed` | whole | OpenAI chat (best effort), LM Studio, Ollama, vLLM, llama.cpp |
| `stop` | 1 to 4 texts | OpenAI chat, LM Studio, Ollama, vLLM, llama.cpp |
| `presence_penalty`, `frequency_penalty` | -2 to 2 | OpenAI chat, LM Studio, Ollama, vLLM, llama.cpp |
| `repeat_penalty` | 0 to 2 | LM Studio, llama.cpp (vLLM: `param.repetition_penalty`) |
| `param.<name>` | unchecked | whatever the server takes; sent as `<name>` |

The Responses API takes only `temperature`, `top_p` and `max_tokens`; the
tools say so when another is set on a responses engine. The deterministic
end is a low temperature **and** a fixed seed.

- **Core:** `core/sampling.rs` (keys, ranges, the request keys),
  `core/served.rs` (what a server says about its models).
  `OpenAi::request` adds the set keys to the body.
- **Tools:** the `settings` group of the app tools (`models`,
  `engine_settings`, `engine_model`, `engine_set`), served by
  `dry mcp settings` (`mcp/settings.json`) or `dry mcp all`; the same as
  `dry models`, `dry engine <id>`, `dry engine use|model|set|unset|keys`.
- **Engines:** an engine gets these tools only when its `mcp.json` names
  that server; the default engine's tools are unchanged.
- **Model list:** `models.json` settings, then each server's `/v1/models`,
  laid over with LM Studio's `/api/v0/models` or Ollama's `/api/tags`,
  `/api/ps` and `/api/show`. A field no endpoint gave is `unknown`.

### CLI models (built: `core/cli.rs`, `tools/cli.rs`)

A model can be a coding CLI in one-shot mode instead of an HTTP server,
for fast testing (local models are slow). It is a **stateless completion
provider**: dry renders the prompt and owns the context as for any model;
the CLI answers that one prompt and exits. (A session engine, `sessions.json`,
is the other thing: the CLI runs its own loop.)

```json
{ "name": "claude/haiku", "kind": "cli", "provider": "claude", "model": "haiku" }
{ "name": "codex/luna", "kind": "cli", "provider": "codex", "model": "gpt-5.6-luna" }
{ "name": "copilot/auto", "kind": "cli", "provider": "copilot", "model": "auto" }
```

Optional: `program` (its path, when not the one on `PATH`) and `context`
(else the CLI's: claude and codex 200k, copilot 128k). No `id`, no key.

- **Like a base-URL override.** The entry's inference is Chat Completions
  to `base_url: cli:<provider>` (`cli:claude?program=/path` with a
  program). The request is the usual body; the native transport
  (`tools/net.rs`) sees `cli:` and runs the program (`tools/cli.rs`)
  instead of POSTing, and gives back a Chat Completions body (the text,
  and `usage` with the tokens and cost the CLI reported). The runner, the
  stream reader and `OpenAi::reply` are unchanged; the context probes are
  skipped.
- **One caller, per-provider overrides.** `Cli` says the program, its
  args, the args that make it answer instead of act, the model flag, how
  the system text and the prompt go in, and how its output reads;
  `Cli::claude()`, `codex()`, `copilot()` fill it (flags checked with each
  CLI's `--help`):

| provider | answers, does not act | system text | output | streams |
|---|---|---|---|---|
| claude | `--tools ""`, `--strict-mcp-config`, `--setting-sources ""`, `--disable-slash-commands`, `--no-session-persistence` | `--system-prompt` (replaces Claude Code's) | `-p --output-format stream-json --verbose --include-partial-messages` | text deltas |
| codex | `exec --sandbox read-only --ignore-user-config --ignore-rules --ephemeral --skip-git-repo-check --strict-config` (no switch for its tools) | `-c developer_instructions="…"` | `--json` | the whole message at the end |
| copilot | `--available-tools= --disable-builtin-mcps --no-custom-instructions --no-ask-user --no-auto-update` | above the prompt in `<system>` (no flag) | `--output-format json` | text deltas |

- **Flattening.** The body's system messages, joined, are the system
  text. One user message is the prompt as it is; a longer conversation is
  a transcript, oldest first, each in a `<user>` or `<assistant>` tag
  (a tool result is a user message), then "Write only the next assistant
  message, nothing else." The prompt goes on stdin.
- **Running.** An argv array (no shell), in an empty folder of its own
  (`$TMPDIR/dry-cli`, so no `CLAUDE.md`/`AGENTS.md` is found), its own
  process group; 600 s timeout and a cancel flag kill the group; the
  error line is the CLI's error message, else its last stderr line.
- **Settings.** `models` shows each CLI model's program, whether it is on
  `PATH` and its `--version`, and lists every CLI there is; `engine_model`
  selects one (`dry engine use engine-1 claude/haiku`). Settings → Models
  lists them in the window and on the web; Test sends one message; Detect
  gives the CLI's model aliases (claude, copilot; codex lists none).
- **Not passed:** sampling settings (no CLI flag for them). **Not in a
  browser page:** a page cannot start a program; the host runs it.

Measured 2026-11-23, one dry turn ("Ask me which one database to use:
Postgres or SQLite"), reply + choice ask, through a test host:
claude/haiku 2.2–3.8 s, copilot/auto 4.4 s, codex/luna 6.3 s; LM Studio
(ripple-65b) 11.5 s.

## 4d. Reply components: asks and displays (built: `core/ask.rs`, `core/show.rs`, `gpui/views/{ask,show}.rs`, `html/render/{ask,show}.rs`)

How a reply becomes a UI: **the registry and the tool contract go in,
the framework renders.** The model is given the component library (one
example call per registered component, written from the registry under
`reply` in the Tools slot) and the `reply` contract; it writes one flat
call; core reads that call into typed components; each shell draws them
from tokens. The model never sends a UI tree or reads UI state: it writes
`reply(...)`, and an ask's answer comes back as text.

```text
registry ─▶ Tools slot ─▶ model ─▶ reply(text, ask=..., show=...)
          ─▶ core: Parsed { ask, show, notes } ─▶ GPUI / HTML / browser draw from tokens
          ─▶ user answers ─▶ "Answer (kind): ..." as the next user message
```

`reply` ends a turn. It carries at most one `ask=` (a control that asks)
and at most one `show=` (a display that only shows), together if needed:
"here is the plan, which day?" is one reply. Both sets are closed and
registered: `Ask` in `core/ask.rs`, `Show` in `core/show.rs`, and both are
page components too (`Component::Ask`, `Component::Show` in
`core/pages.rs`; `{"type": "ask", "ask": {"kind": "date"}}`,
`{"type": "show", "show": {"kind": "list", "items": ["a"]}}`, or
`dry add <path> ask date` / `dry add <path> show list data="a|b"`).

### Asks

| ask | for | extra arguments |
|---|---|---|
| `choice` | pick one | `options="A\|B\|C"` (2 or more) |
| `multi` | pick any | `options="A\|B\|C"` |
| `confirm` | yes or no to one short question, no details | none |
| `date` | a day | none |
| `datetime` | a day and a time | none |
| `number` | how many, how much | `min`, `max`, `unit` (all optional) |
| `text` | a short typed answer | `placeholder` (optional) |
| `review` | check, edit or reject the details of a plan | `title`, `rows="key: value\|key: value"` |
| `form` | several questions at once | `title` (optional), `fields="Label:kind\|..."` |

- **`review` was `card`.** `ask="card"` still reads, as a review, with a
  note that it is `review` now; a page file with `"kind": "card"` reads
  too, and an old `Answer (card): ...` still shows as answered.
- **Confirm with details is a review, structurally.** A small model told
  to "confirm" a summary picks `confirm` whatever the instructions say
  (measured: 0/4 when the task said "confirm"; rewording was unstable). So
  the parser decides by what the call carries: `ask="confirm"` with
  `rows=` (or with `options` that are all `key: value`) is read as a
  review, with the note `ask="confirm" with rows= is a review; drawn as
  one`. A confirm with only a question stays a confirm.
- **A form is several single asks in one box**, still one flat string:
  `reply(text="A few details", ask="form", title="Booking",
  fields="Day:date|Guests:number(1-12)|Seat:choice(inside,terrace)|Notes:text")`.
  A field is `Label:kind`, with what the kind needs in brackets:
  options for `choice`/`multi` (`,` `/` or `|` between them),
  a range and unit for `number` (`1-12 people`, `..120`, `-5..5 C`,
  `1 to 3`), a hint for `text`; `confirm`, `date` and `datetime` take
  nothing. Fields are split on `|` `;` or new lines outside brackets, so
  `Size:choice(S|M|L)` is one field. `fields=` alone (no `ask`) is a form;
  `fields=` with another `ask` is a form too, with a note. A field is
  never a review or a form. At most 12 fields.
- **Form near misses.** `Name` (no kind) is a text field; `Size:S/M/L`
  is a choice; an unknown kind is text; a choice with one option is text;
  `date(soon)` drops the bracket; a field with no label is left out;
  `number(9-2)` drops the range. Each says so in the note.

### Displays

| show | for | `data=` |
|---|---|---|
| `table` | rows and columns | `"Name\|Age;Ada\|36;Bob\|41"`: rows by `;`, cells by `\|`, first row the header |
| `list` | a few items | `"lint passed\|3 tests failed\|build skipped"` |
| `progress` | how far along | `"7/10 tests passed"`, `"7 of 10 ..."` or `"70% copied"` |
| `stat` | headline numbers | `"Revenue\|$12.4k\|+8%;Users\|1204\|-3%"`: label\|value\|change |

- **One `data=` string per display**, so a display never collides with an
  ask's `rows`, `min`, `max` or `title`; `caption=` is an optional title.
  `show="table"` with `cols=` and `rows=` (and no ask) reads too.
- **Core says what it means:** `Show::fraction` and `progress_text` for a
  bar, `Stat::trend` (up, down, flat from the change's sign) for the
  colour a shell gives it (`text_accent`, `error`, `text_secondary`).
- **Fallbacks.** An unknown `show` or a missing `data` shows nothing; a
  header with no rows shows nothing; ragged rows are padded or cut to the
  header; more than 8 columns or 30 rows are cut; a progress outside 0 to
  max is drawn at the nearest end; a stat without a value is left out.
  Each with a note.

### Rules for both

- **Flat strings only**, so a small model can write them.
- **The instructions come from the registries.** `ask::registry()` and
  `show::registry()` hold one example each; `ask::instructions()` writes
  each as `name (when): reply(...)` (the displays' after the asks'), and a
  test per registry reads every example back, so the instructions and the
  parsers cannot drift.
- **Nothing fails silently.** Every argument has a default; near misses
  are read (`yes/no` is confirm, `checkbox` is multi, `choices=` alone is
  a choice, `bullets` is a list, `kpi` is a stat). Unknown kinds, short
  lists, a review without rows, a form without fields fall back to a
  plain reply. Each fix is a result line, `reply(...) → note: ...`, the
  model reads on its next step.
- **The answer is the user's next message,** one fixed form the model
  reads without any UI state: `Answer (choice): Green`,
  `Answer (multi): unit, e2e` (or `none`), `Answer (confirm): yes`,
  `Answer (date): 2026-11-24 (Tue)`,
  `Answer (datetime): 2026-11-24 18:00 (Tue)`, `Answer (number): 3 copies`,
  `Answer (text): Ada`, a review's verdict with its rows under it
  (`confirmed`, `edited` with `(was ...)` on a changed value, or
  `rejected`), and a form's `submitted` with one `Label: value` line per
  field (`(blank)` for one left empty):

  ```text
  Answer (form): submitted
  Day: 2026-11-24 (Tue)
  Guests: 4
  Seat: terrace
  Notes: (blank)
  ```
- **Answered once.** An ask is open until the user's next message; then
  the same place shows the answer, read-only (`ask::state`). A form's
  fields only pick; its one Submit sends them all.
- **Renderer-neutral.** Core parses, keeps the calendar maths
  (`month_grid`, `quick_days`), reads what a control holds
  (`ask::Input` → `Ask::read_input`, and `Ask::submit` for a form, which
  names the field that cannot be sent: `Guests: at most 12`) and writes
  the answer. GPUI, the HTML renderer and the browser app only draw from
  tokens and gather `Input`s: in HTML each form field is a `.ask-field`
  whose controls `ask-pick` / `ask-toggle` / `ask-step` inside it.
- **Checked with the local model:** `the_model_asks_with_the_right_component`
  in `gpui/src/net.rs` (ignored) gives the real prompt one situation per
  component (asks, form, each display, and a table with a choice in one
  reply), each named and unnamed (`*`), and prints the pass rate per
  component.
  Measured 2026-11-23 on the owner's LM Studio model (4 runs each, 4096
  context): every named situation 4/4, including review (was `card`:
  3/4), form, table, list, progress, stat and table+choice; unnamed:
  review* 3/4 (the miss a full-context server error; `card*` was 2/4,
  both misses `confirm` with the details only in the text), form* 4/4,
  progress* 4/4, stat* 3/4, table* 2/4 (the misses answered in prose),
  date* 3/4 (one weekday choice).

## 4e. Streaming replies (built: `core/stream.rs`, `core/early.rs`)

The answer is read while the model writes it, so a reply's text grows and
its component appears before the answer ends. Meaning never changes: when
the answer is in, the full parsers read it as always and the saved message
is drawn.

```text
server ─▶ stream::Sse (bytes, split anywhere) ─▶ Delta::Text | Delta::Think
       ─▶ stream::Parser (resumable) ─▶ Event: Field, CallStart, ArgStart, ArgDelta,
          ArgDone, ListItem (each `|`), Row (each `;` of data), CallDone, ActDone, Text, Think
       ─▶ stream::View: reply text so far, the ask/show once its kind closed,
          options and rows one by one (provisional)
answer in ─▶ response + calls parsers ─▶ the message, as without streaming
```

- **`Sse`**: OpenAI-compatible `stream: true` (`choices[0].delta.content`,
  `reasoning_content` to think, `[DONE]`; the Responses API's
  `response.output_text.delta`). `finish` gives the body a plain call
  would have, so `Runner` reads it unchanged. A server that does not
  stream answers with a plain body: kept as it is; one that refuses
  `stream` with an error is asked again without it.
- **`Parser`**: the `toon` line rules (a key line, `- ` lists, fences
  skipped, the first line trimmed) or a `json` object, then the call
  grammar as a state machine (name, key, quoted value with escapes, bare
  value, `[...]`, backticks, `none`). Two acts: the full reader says which.
  Property test (`stream::tests`): every registry example, forms, shows,
  escapes, unicode, think blocks, many-call acts and 8 broken acts, split
  at every byte boundary, in random chunks and char by char, give the same
  normalised events and exactly `calls::parse`'s calls (or its words for
  why not).
- **`View`**: provisional. The reply's arguments so far go through
  `ask::Parsed::of` with only closed list items; a kind whose list is too
  short to read yet is drawn as an empty box of that kind (`choice` with no
  options, a table's header). When the reply call closes, `Parsed::of` of
  that call; when the answer is in, the saved message. `<think>` outside
  the calls goes to `think`, its own fold, never the reply.
- **`ask=` and `show=` come first** in the registry examples
  (`reply(ask="choice", options="A|B", text="…")`), so a model that copies
  them names the component early.
- **Shells**: `Transport::stream` (default: a plain `post`), `runner::io_streaming`,
  `Runner::streams` / `early_call`. Native: `dry_tools::net::stream` (ureq,
  the body read as it comes). Browser: `fetch` with the body's
  `ReadableStream`. GPUI: read on a background thread, the view sent to the
  window every 50 ms (`AppView::streaming`). HTML: `Reported::streaming`,
  drawn under the conversation (`render/engine.rs`, `.msg.streaming`). The
  host (`server/host.rs`) keeps the view for pages and sends a `reply`
  event (every 150 ms and at the end) that clients redraw on.
- **Measured** 2026-11-23, LM Studio, a 65B model, 4096 context, the real
  prompt (`dry_tools::net::tests`, ignored): the component known at
  9.2 s of 13.0 s (table), 10.0 of 15.2 (form), 16.5 of 19.3 (choice),
  27.0 of 36.4 (table + choice); in the browser 9.9 to 30.6 s of 16.3 to
  37.3 s. Most of the wait is the `thought` before `act`.

## 4f. Workers: sessions inside dry (built: `core/{snapshot,stream_json,approval}.rs`, `tools/worker.rs`; docs/sessions.md)

An engine run by an outside program (`kind: claude`, a Claude Code
session) is a worker: the dry super engine above it owns the context, and
the program gets a subset of tools.

- **Push**: hooks in a settings file written per session (never the
  owner's own) run `dry snapshot`: at SessionStart its goal, participants
  and tools; at each message its live snapshot (goal, phase, artifacts kept
  short, its thread's posts for it since its cursor), saying it supersedes
  the earlier ones, within 4000 characters (and saying what was cut).
- **Pull**: the `engines` MCP group: `state`, `artifact`, `thread` read;
  `post`, `engine_start`, `engine_call` write through the host only (no
  host: refused in one line). Children it starts are real dry engines, its
  children, on its thread.
- **Approvals**: its permission prompts go to `approve`: the spec's `allow:`
  rules first (a new session allows the read-only tools), else a record on
  the host, a notice to you on its thread, and Allow / Deny in the
  UI; no timeout unless `approve_timeout:` says one.
- **Live**: its program streams events (`stream-json`): each tool call is a
  live row in the chat (saved as one tools message at the end), each
  sub-agent a read-only participant on its thread, and its slash
  commands the composer's `/` list.

## 5. Order of work

1. ✅ Phases: `core/phase.rs` (parse, `goto_phase`), `convo/run.md`,
   the phase slot. Tests with a fake model. No step limits anywhere.
2. ✅ Template: `prompt::render` as the six slots, each with source + size +
   why-empty for the Response view.
3. ✅ Artifact kinds + snapshots (text; image later) + tools from artifacts.
4. ✅ The `file` artifact (open, focus, edit, save; live snapshot).
4b. ✅ The `terminal` artifact (run, start with callback, background).
   Next for both: an approval step before unattended commands run.
5. Then scripts and alerts as artifact kinds (from `port-draft.md`).
6. ✅ `spec/goal.md` in sections ([the goal](#2a-the-goal-specgoalmd-in-sections)); the Learning slot ([the template](#2-the-template-fixed-slots-filled-by-contributors)).
7. ✅ Engines as tools (`kind: engines`), the shared context of a level
   (`kind: shared`), learning through curated tools (`kind: learning`).
   Fake-model turn tests in `core/engines.rs`; a real one in
   `gpui/src/net.rs` (`a_real_parent_asks_a_child_and_reads_its_reply`).
8. ✅ One step, many calls ([one step, many calls](#3d-one-step-many-calls)): instant calls in order, waiting ones all
   at once, results in written order, one callback path. The logic check
   is `tools/tests/one_step_many_calls.rs` (fakes with delays and
   recorded times); a real one in `gpui/src/net.rs`
   (`one_act_explores_two_ways_at_once`).
9. ✅ Workers ([workers](#4f-workers-sessions-inside-dry-built-coresnapshotstream_jsonapprovalrs-toolsworkerrs-docssessionsmd)): sessions run on the host, pushed snapshots by hooks,
   the `engines` MCP group, approvals, live tool rows (docs/sessions.md).

## Decided (owner, 2026-11-22)

- An artifact and a live artifact are the same thing; one `artifacts/`
  folder, no `live/`.
- Vision is planned, not built: snapshots are text for now.
- Phases are the outer loop; each names its response mode; `jev` is
  planned (needs a newer API and model support).
- Step counts never limit a turn or a phase; stuck loops get their own
  handling later.
- The context window is the only limit: measured before every call,
  the history compressed at 75% ([the context window](#3b-the-context-window-the-only-limit)). The full log stays in
  `messages.md`, cuts marked by `### summary` blocks.
- Every engine shows its activity as a label and a tint ([activity](#3c-activity-what-an-engine-is-doing-now)).
- `instruction.md` is renamed `goal.md`, read in sections ([the goal](#2a-the-goal-specgoalmd-in-sections)).
- Learning comes from curated tools; engines are tools; engines of one
  level share one context ([artifacts](#4-artifacts-things-that-add-to-several-slots)).
- One step, many calls: every call invoked at once, only the waiting in
  parallel, the evidence collected in written order, long work calls
  back ([one step, many calls](#3d-one-step-many-calls)).
