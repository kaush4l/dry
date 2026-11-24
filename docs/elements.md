# Elements: subscriptions, machines and environments in the prompt

Status: design, 2026-11-23. Phase 0 (the element walk), phase 1 (the
`environment` element, held through `spec/elements.md`) and phase 2 (the
`subscriptions` element and `model=`) are built
(`crates/core/src/element.rs`, `crates/core/src/subscriptions.rs`);
machines are not. It
follows the owner's words of the same day:

> The application is a long running loops manager. So the top level desk
> engine is only tasked with managing the loops. […] At the application
> level is where we have the subscriptions, other server control and
> basically server machines. So the orchestrator will use the subscription
> and hardware to run projects.

and the correction that came after:

> Actually, subscriptions and other things are also just prompt elements.
> And like every element, elements provide tools, state, snapshots.

and the owner's definition, after a first draft mixed the two words:

> Capability and element are both different. From an engine, the render
> prompt always renders all elements together. And the order can be
> deterministic. So that is where we can add element in an order. An
> element, a tool, an artifact as well. So, when rendering prompt, we will
> go through them and collects all the evidence. or context.

## Element and capability

- An **element** is one unit of an engine's prompt. The renderer
  (`prompt::render`, the one place a prompt is made) walks the engine's
  elements **in a fixed order** and each one contributes its part of the
  context: a section of text, tools, a snapshot (evidence), or why it has
  nothing. Every kind of thing in the prompt is an element: the goal's
  sections, learning, the phase, each tool, each artifact, the thread, the
  history, the response contract, and the new ones here (subscriptions,
  machines, environment).
- A **capability** (docs/capabilities.md) is what a runtime *can do*:
  file, terminal, battery, a peer's model. It is where an element's tools
  and data come from, and whether they are present here. An artifact
  element uses capabilities (`uses: [file, clock]`); a capability by itself
  is not in the prompt.

So: elements are *what the model sees, in order*; capabilities are *what
backs them on this machine*.

### The element contract

```rust
/// One unit of the prompt.
pub trait Element {
    fn id(&self) -> &str;
    /// Where it goes: its section, then its rank inside the section.
    fn place(&self) -> Place;
    /// Its part of the context now. Pure: what only the shell knows
    /// comes in through `cx` (snapshots, the registry, the clock).
    fn collect(&self, cx: &Cx) -> Contribution;
}

pub struct Contribution {
    pub section: Option<(String, String)>, // (title, text) in its section
    pub tools: Vec<ToolSpec>,              // offered while it is present
    pub evidence: Vec<Evidence>,           // snapshot rows, images later
    pub empty: Option<String>,             // why it has nothing (shown in the prompt view)
}
```

`render` becomes: gather the engine's elements, sort by `place` (section in
the agreed order, then rank, then id: no tie is left to chance), call
`collect` on each, and join the contributions. The prompt view lists every
element with its contribution or its `empty` reason, as it lists slots
today.

### The order

The sections keep the agreed order (docs/engine-standard.md §2):
instructions, learning, phase, tools, artifacts, history, response. The
stable part stays first, for the prompt cache. Inside a section, elements
sort by rank, then id. Elements that change every step go late:

| section | elements, in rank order |
|---|---|
| instructions | goal: Goal, Scope, Done |
| learning | learning (turn-start copy) |
| phase | phase |
| tools | each tool, in the order its element offers it (direct, then phase, then artifacts') |
| artifacts | each artifact (by name), then **subscriptions**, **machines**, **environment**, then the thread, the clock last |
| history | summary, messages |
| response | the contract |

Adding an element is adding a type with a `place`; nothing else in the
renderer changes. The same engine in the same state still sends the same
prompt.

### As built (phase 0)

`crates/core/src/element.rs`, used by `prompt::render_with` and
`prompt::artifact_sections`:

- `Element` is an **enum**, not a trait (pure, cloneable, comparable):
  `Goal`, `Learning`, `Phase`, `Tools` (the toolbox), `OutsideTools`,
  `Mcp { index, name }`, `Artifact { name, room }` (a plain one, with the
  characters the `ARTIFACTS_CHARS` budget leaves it), `Skills`,
  `Kind { name, live }` (a capability artifact), `NoArtifacts`
  ("None yet."), `NoContract(section)`, `Summary`, `Messages`,
  `Response`. Each has `id()`, `place()`, `collect(&Cx)`.
- `Place { section, rank }`; `Section` is `template::Slot`, whose order is
  the agreed one. Ranks are constants in `element::rank` with gaps
  (tools: toolbox 100, outside 200, MCP 300 + its index; artifacts:
  plain 100, skills 200, kinds 300 + the kind's order, capabilities only
  600, clock 900; history: summary 100, messages 200). Ties sort by id
  (`artifact/<name>`), so plain artifacts go by name.
- `Contribution { text, tools, evidence, messages, empty }`: `text` is the
  element's part of its section (the renderer adds the section's title
  and lead, and joins parts with a blank line, tools with a newline);
  `messages` carries the history. A section with no part says its first
  element's `empty` reason (`Rendered::empty`, as before).
- `Cx { engine, outside, template, template_note, phases, goal, equipped }`
  is read once per render.
- `Rendered::elements` lists every element walked (`Listed { id, place,
  empty }`); no view shows it yet.
- The old renderer is kept as `prompt::legacy` (tests only) and
  `the_element_walk_sends_the_same_prompt_as_the_slots` compares the two
  (the whole `Rendered`, the messages, the prompt-view text, the artifact
  sections and `inspect::slots`) for the shipped translator and a child
  of each template, and hand-made engines (no contract, renamed slots,
  phases and jev, a broken template, legacy goal, cut and over-budget
  artifacts, every kind, outside capabilities, skills, MCP tools, a
  summary), each with no snapshots, a clock, a disk and a family.

A new element: a variant, its `id`, `place` and `collect`, and one line
in `element::gather`.

### As built (phase 1)

- **`spec/elements.md`**: the extra elements an engine holds, one id per
  line (blank and `#` lines skipped, a `- ` before an id allowed). Kept
  as `Engine::elements` (the text as written), saved and read like
  `template.md` (`files::spec_files` / `apply_spec`; every store, the
  `default/` layer and the browser's bundled default carry it as one more
  spec file). There is no spec-file editor in the views; it is edited as
  a file. `element::held` reads it: built ids (`element::BUILT`:
  `subscriptions`, `environment`) are held, each once; `machines` says
  it is not built yet; anything else says it is not an element. Each
  such line is a note in the prompt view (`[not sent]`, under
  `elements.md`), never a panic.
- **`Element::Environment`**, id `environment`, in the artifacts section
  at `rank::ENVIRONMENT` = **800**: after the plain artifacts (100),
  skills (200), kinds (300 + order) and capability-only artifacts (600),
  before the clock (900). 700 is the subscriptions' (phase 2), 750 is
  left for the machines. `collect` reads `Cx::environment`, which is the shell's
  `Registry::environment` from the registry the engine carries
  (`Engine::machine`, set by `State::equip`), so collect stays pure and no
  `Snapshots` field was needed. Its part is one line, also its evidence
  (`Environment::line`):

  ```text
  environment: browser-vm · ready · /work · python3, tcc · saved 36 MB
  environment: browser-vm · booting: restoring the saved machine · /work · saved 36 MB
  environment: browser-vm · failed: vm/vm.js did not load · /work
  environment: host · ready · /Users/me/repo · python3, node, cargo
  ```

  A piece not known is left out (runtimes not checked yet, no saved
  machine); a check that found none says `none of … on PATH`. No
  environment reported: nothing is sent, and the walk lists it as empty
  with why. Tools: none.
- **Runtimes** (`environment::RUNTIMES`: python3, node, bun, java, rustc,
  cargo, tcc, gcc), kept in `Environment::runtimes` and shown in Settings
  → Environment too. In the browser VM: `environment::runtimes_check`
  (one `command -v` loop, nothing started) run at the console once after
  the boot, and again after a `run` or a `start`ed job whose command
  installs (`environment::installs`: `apk add` / `apk del`). On a host
  (server, window, CLI): each name looked up on PATH once at start
  (`dry_tools::capability::probe_compiled`, the same lookup as
  `command -v`, no process started). A paired companion's runtimes are not
  known to the page yet: left out.
- The head holds it: `default/engines/translator/spec/elements.md` is
  `environment`. An engine without `spec/elements.md` sends the same
  prompt byte for byte, whatever environment the shell reports (the
  phase-0 equality test runs every case again with a ready, booting and
  failed VM and a host); `the_head_holds_the_environment_and_sends_its_line`
  checks the translator's line for each.

### As built (phase 2)

- **`dry_core::subscriptions`**: `Subscriptions { default, rows, notes,
  models, sessions }`, one `Row { name, kind, what, presence,
  tokens_per_s, cost_usd, running }` per `models.json` model (`api`, or
  `cli` for `kind: cli`; a model with a `base_url` of its own shows
  `@host:port`) and per `sessions.json` program (`session`; `what` is its
  description up to the first comma). `build(models, sessions, probe)`
  makes the rows: a file that does not read is a `note:` line, a row that
  does not read is absent with why. `Presence` is `present`, `absent`
  (why) or `unknown` (why: the browser cannot look). The value holds the
  two files as read (key *variable names* only) so `choose` can make a
  child's inference.
- **`Element::Subscriptions`**, id `subscriptions`, artifacts section,
  `rank::SUBSCRIPTIONS` = **700** (after every artifact, before the
  environment). `collect` reads `Cx::subscriptions`, the shell's
  `Registry::subscriptions` (the same route as the environment). Its text
  (also its evidence, a line each):

  ```text
  subscriptions (default: local)
  local         api      ripple-65b @127.0.0.1:1234  present  12 tok/s
  gpt           api      gpt-5-mini                  absent   OPENAI_API_KEY not set
  claude/haiku  cli      claude haiku                present
  claude        session  Claude Code                 present  10 tok/s  $0.41 in 7d  2 running
  ```

  None reported: nothing is sent, and the walk lists it as empty with why.
  Tools: none.
- **Usage** (`Subscriptions::usage`): only from dry's own spans of the
  last `USAGE_DAYS` = 7 days: a model row's model calls (span `model` and
  `provider` = the row's inference), a session row's session turns (span
  `purpose: session`, `provider` = the program). Tokens/s and cost, each
  left out when no record has it. No CLI usage feed (open question).
- **Probes** (`dry_tools::subscriptions`, in the shells): a CLI or a
  session program on PATH (`find_program`, nothing started); a model whose
  base URL is local (`127.0.0.1`, `localhost`, `[::1]`): `GET <base>/models`
  with an 800 ms timeout and no key; a remote API model: whether its
  `key_env` variable is set (`absent: X not set`), the value never read
  into anything. `Watch` says when to probe again: every 60 s and at once
  when `models.json` or `sessions.json` changes.
  - **server**: its own thread (`Host::watch_subscriptions`), usage from
    the host's spans; `running` (busy session engines per program) is
    counted again at every turn's `begin`. Engines are equipped; no page
    redraw is asked (no page shows them yet).
  - **window**: the 1 s tick probes on the background executor, usage
    from its span log, `running` from `seen(j).busy`, counted at each
    probe.
  - **CLI** (`dry-tools prompt`): probed once, usage from the store's logs.
  - **browser**: the page's own model is present once Settings → Models
    Detect reached its server, else `unknown`; a CLI row is absent "needs
    a host or companion"; there are no session rows (`sessions.json` is
    not in the browser: a note says so).
- **`model=`** on `engine_start` / `engine_call` (`tools::CHILD_ARGS`, and
  the MCP `engines` tool's `engine_start` schema and the host's
  `POST /api/engines/:id/children`): a new child only.
  `Subscriptions::choose` names a row: an `api` / `cli` row gives the
  child that model's inference (`Models::inference`, its own default
  sampling); a `session` row makes the child a session engine of that
  program (`session`, `DEFAULT_ALLOW`, as `State::add_session` does), on
  the parent's thread like any child, and its first message is its
  goal.md then the text (a program reads no goal.md). Unknown or absent:
  refused with why and the rows that can be chosen; no subscriptions
  reported: refused, and model left out works as before. `unknown`
  rows may be chosen. On an existing child it is ignored with a note
  ("it keeps its model"). A session parent's new child keeps the model
  it chose (the host's default-model fill is skipped then).
- The head holds it: `default/engines/translator/spec/elements.md` is
  `subscriptions` then `environment`; its goal's Scope has one line on
  picking `model=` (a long or code-heavy job can go to a session row).
- Not done: Settings → Models and Sessions do not show the rows'
  presence yet (three renderers and their evidence shots); events
  (`limit-near`, `went-down`, `came-back`); a CLI's own usage feed.

## Tools, artifacts and alerts (owner, 2026-11-23)

The owner's definitions, which every element follows:

> A tool is basically an object with name, description, and a way to call
> it. And mostly some tools do require state to perform actions.

- **A tool** is an object: a name, a description, and a way to call it.
  It describes itself, so nothing outside it names it or explains it.
  Some tools need state to act: Chrome DevTools' tools need a running
  Chrome. That state belongs to whatever brings the tool.
- **An artifact** brings a set of tools and a snapshot. The snapshot is
  a context technique: instead of the model asking for a file, changing
  it elsewhere and asking for the whole file again, the artifact's live
  section shows the current state; the model changes it with the
  artifact's tools, sees the new state in the next step, and decides the
  next change. One latest snapshot, never a history of reads.
- **An alert** is a tool object too, of two kinds: the hook, "if this,
  then that".
  - **Time-based**: at a time of day (cron), at an exact moment ("wake me
    at 18:00"), or after a delay ("remind me in 5 minutes").
  - **Event-based**: when something happens (a capability's event, a
    file changes, a job ends, a post arrives).
  - Made by a tool call with its trigger and its **input**. When it
    fires, it wakes exactly the engine that made it (or the one it
    names), with that input, through the thread's guided-message rule
    (a post addressed to that engine, delivered with `add_wake`, so its
    phase stays). Its schedule and last fire are saved in the engine's
    folder, so it survives a restart and fires once per due time.
  - Today `alert.rs` holds alerts but never fires them
    (docs/twelve-factor.md, gap 2): firing is the next build.

### What the tools are for

The owner: every tool gives an engine one of a few abilities, and the
engine's one job is to be proactive: from what it sees, judge and create
the next run.

| ability | how it reaches the engine |
|---|---|
| schedule a task | an alert (time- or event-based), with its input |
| observe a loop | the thread and the engines artifact (children's state, replies) |
| observe an environment | live artifacts' snapshots, the environment element |
| be woken | a guided post, a child's reply, an alert firing, a capability event, the owner's message |
| act | the tools an artifact or capability brings |

A tool that does not serve one of these is a candidate to fold or drop
(docs/twelve-factor.md, the tool inventory).

## Three new elements

Subscriptions, machines and environments are **not** a new settings
system. They are three more elements, each backed by capabilities and
files:

| element | what it is | its state lives in | backed by |
|---|---|---|---|
| `subscriptions` | every model and program an engine can be run by: API models, local servers, CLI subscriptions (claude, codex, copilot), session programs | `models.json`, `sessions.json` (and a companion's `providers.json`) | a compiled capability that reads the files and probes each |
| `machines` | this machine and every machine that joined: what it is, what it can do, how busy it is | the registry's peers (capabilities.md §4 handshake), plus this host | this machine's capabilities, and each peer's (remote) |
| `environment` | where the file and terminal tools run on a machine: host, browser VM, later docker | `Registry::environment` (`dry_core::environment`) | the `file` and `terminal` capabilities' runtime, per machine |

## What each one gives

### `subscriptions`

**Snapshot** (re-read every 60 s, and at once when a file changes). One
row per entry, never a key's value:

```text
subscriptions (default: local)
local         api      ripple-65b @127.0.0.1:1234   present  12 tok/s  $0
gpt           api      gpt-5-mini                   absent   OPENAI_API_KEY not set
claude/haiku  cli      claude haiku                 present  5h window 41% used
codex/luna    cli      codex gpt-5.6-luna           present
claude        session  Claude Code (-p, resumable)  present  2 running
```

- `present` comes from a probe: a CLI on PATH (`claude --version`), a local
  server that answers `GET /v1/models`, a key variable that is set (only
  *whether* it is set). Absent says why.
- Usage and limits where a source exists (the analytics records already
  hold tokens, cost and tokens/s per model; a CLI's own usage feed later).
  A value with no source is left out, not guessed.
- **Events**: `limit-near` (a window over 80%), `went-down` (a local server
  stopped answering), `came-back`. An event posts to the head's thread
  addressed to the head, so the orchestrator can move work.

**Tools**: none of its own. Choosing a subscription is an input of the
tools the head already has:

```text
engine_start(name, goal, scope, done, text, template, root, model="claude/haiku")
```

`model=` names a row. A **session row** is how work is "passed over to a
session": `engine_start(..., model="claude")` makes a session engine, the
same as New session in the UI. So model engine vs session is one choice in
one tool, not two flows.

**State files stay.** `models.json` and `sessions.json` keep their shape;
they are the element's state, edited in Settings as today. The Settings
Models and Sessions sections become this element's view: its doc, its rows
with state, its events.

### `machines`

**Snapshot** (every 30 s; a peer's comes with its heartbeat):

```text
machines
this    macOS · M4 Max 16c · 128 GB · NPU 16c   host env   load 22%  power 38 W  engines 3 busy
hearth  Linux arm64 · 16 GB                     host env   load 4%   joined 2h ago
phone   iOS · app                               viewer     —
```

- This machine's row comes from what is already probed (`dry discover`:
  pmset, thermal state, df) plus a few compiled readings (cores, memory,
  power where the platform gives it). A peer's row is its hello and its
  manifest's snapshot.
- A viewer (the mobile app, a hosted page looking at the server's state)
  is listed so the orchestrator knows who is watching; it runs nothing.
- **Events**: `joined`, `left`, `hot` (thermal state), `on-battery`.

**Tools**: none of its own; again an input of `engine_start`:
`machine="hearth"`. Left out: this machine. A machine whose environment is
absent refuses with why. Running a child on a peer needs the peer to run
engines (a dry server there): phase 4 below.

### `environment`

The kind that is built (`dry_core::environment`), made an element.

**Snapshot**: kind, state, workspace, the runtimes present, cache size:

```text
environment: browser-vm · ready · /work · python3 3.12, tcc, node 22 · saved 36 MB
```

**Tools**: none; `file` and `terminal` already go through it.
**Events**: `ready`, `failed`. Settings → Environment (built) becomes its
view.

## Who holds them

The head (the desk orchestrator) lists the three in its spec, so they are
in its element list after its artifacts:

```text
<!-- default/engines/translator/spec/elements.md -->
subscriptions
machines
environment
```

so its artifacts section shows what it can spend and where, every step,
and it picks `model=` and `machine=` from what it sees. A child holds
them only if its template says so: a developer needs its environment row
(in its terminal's section already), not the subscription list.

Per the tool-count rule: **no new tools**. Two new inputs on `engine_start`
and `engine_call` (`model`, `machine`), three new snapshots.

## The environment's runtimes

The owner (2026-11-23):

> For the VM, I have other requirements as well. Basically, I just need
> java, python, rust or bun runtimes for 90% of what I would ask to do. As
> required other things I can host as well.

The browser VM is v86: **32-bit x86 only**, 256 MB given, about 30× slower
than native (docs/container.md). Per runtime:

| runtime | in the browser VM | notes |
|---|---|---|
| python | **works** (measured: `apk add python3` 7.5–9 s; `sum(range(10**6))` 1.7–2 s) | |
| java | *unverified*: Alpine's x86 openjdk packages; a JVM in 256 MB | likely needs more guest memory (512 MB+); slow start |
| rust | *unverified*: Alpine's x86 `rust` / `cargo` | rustc in 256 MB is tight; small crates only; crates.io needs a relay |
| bun | **not possible**: bun ships x64 and arm64 builds only, no 32-bit | node (Alpine x86 `nodejs`) can stand in for scripts; bun itself runs on a host |

So the environment lists runtimes as **packs**:

- A pack is a named set of packages (`python`, `node`, `java`, `rust`),
  served beside the app as the apk subset is today
  (`scripts/vm-fetch.py` `PACKAGES`), installed into the VM on first use
  and kept in the saved machine.
- The snapshot says which packs are installed, which can be, and which
  cannot here and why (`bun: no 32-bit build; runs on a host`).
- What the browser VM cannot run goes to a machine that can ("as required
  other things I can host"): the orchestrator sees it in the snapshots and
  picks `machine=`.

Ways past the 32-bit limit, for later: container2wasm (amd64 via Bochs;
needs `SharedArrayBuffer`, large images), or a relay for crates.io and
Maven (docs/container.md, "Networking").

## Phases

0. **The element walk** (built 2026-11-23): `Element`, `Place`,
   `Contribution`; `render` gathers, sorts and collects; today's slots
   become elements with no change to the prompt sent (a test compares old
   and new output). See "As built" below.
1. **`environment` as an element** with its snapshot (built
   2026-11-23, see "As built (phase 1)"). The head lists it in
   `spec/elements.md`.
2. **`subscriptions`** (built 2026-11-23, see "As built (phase 2)"): an
   element over models.json and sessions.json, with probes and the
   analytics usage; `model=` on `engine_start` (a session row makes a
   session engine). Settings Models and Sessions drawn from it: not yet.
3. **Runtime packs** in the VM (first part built 2026-11-23:
   `environment::VM_PACKS`; the environment line names what can be added
   and with which command, and a developer made from the template holds
   that line): python (works), node, then java and rust
   measured for real (guest memory raised if needed), each pack's numbers
   in docs/container.md. bun stays host-only.
4. **`machines`**: this machine's row, then peers from the handshake;
   `machine=` refuses until a peer runs engines; then a dry server on a
   peer runs the child and posts on the same thread.

## Open questions for the owner

- ~~Rename capability to element?~~ **Decided (owner, 2026-11-23): they
  are different**; see "Element and capability".
- ~~Where an engine names the elements it holds?~~ Built as a
  `spec/elements.md` list (phase 1).
- ~~Should the orchestrator be able to change `models.json` itself?~~
  Built as: it only chooses (`model=`); changing is the person's.
- Usage for CLI subscriptions (the claude 5-hour window): read from the
  CLI's own output, or only from dry's analytics of its own runs?
- Java and rust in the VM: raise the guest to 512 MB–1 GB (the tab then
  holds about 1–1.5 GB), or send those to a host?
