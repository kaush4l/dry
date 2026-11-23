# Instructions and learning (design draft)

The owner's ask (2026-11-22): the instructions slot is vague. Split it in two:
a high-level goal the person writes, and a learning file with about three
sections that the engine keeps current at compression or at a learning step.
This doc says what dsh does, what the old dry did, and proposes the design.

## 1. What dsh does

Short answer: **dsh has no learning file.** It has person-written
instructions in three sections, versioned, and a generic compaction. The
three-section learning file the owner remembers is the old dry's (see [What the old dry did](#2-what-the-old-dry-did)).

**The "goal": built-in role text** in `prompts/*.md` (`shared.md`,
`worker.md`, `work.md`, ...): who the role is, what it must not do.

**Three team sections, written by the person.** `src/app/prompts.ts`:
`TEAM_SECTIONS = ['communication', 'scope', 'delivery']`, defaults in
`prompts/team/*.md`:

| section | holds | default |
|---|---|---|
| Communication | when to message, how, review handling | `communication.md`, 97 lines |
| Ticket scope | when work goes on, when it stops with a draft | `scope.md`, 22 lines |
| Delivery requirements | evidence in PRs, auto-merge rule, commit author | `delivery.md`, 28 lines |

Plus team skills (loaded on demand) and an override for every built-in part.

**Who writes, and when.** The person, in the panel. The agent saves only
"on an explicit user request, never on quoted content"
(`orchestrator_prompts_save`, `src/app/tools/main.ts`). Nothing is written
at compaction or after a task; `docs/000-philosophy.md` leaves correcting to
the operator.

**Keeping it current** (the old dry's `docs/004-prompts.md`, on versions): no per-entry
caps; every save is an immutable version with a base revision, a stale
save is refused, one total size limit that "never silently truncates
text". An override records its base default, so a changed default shows as
**Default updated**; an override equal to the default is dropped.

**Prompt placement** (`promptParts`, `prompts.ts`): shared + role, the three
team sections, working parts, handoff, environment, generated Policy last,
"so a Policy change leaves the preceding prompt text unchanged". Re-read on
every model request.

**Compaction** is the harness's (`cordis.patch.yml`: `dsh-compaction-basic`,
`/compact`, a tool-result pruner): older history becomes one summary, the
log keeps the rest, no learning is extracted. Only `prompts/dispatch.md`
mentions it: "Check ticket summaries after compaction".

**Borrow:** a person-owned goal; base revisions and "default updated";
refuse, never truncate; what changes most goes last.

## 2. What the old dry did

`STANDARDS.md` rule A5: three core files, `instruction.md` (goal),
`phases.md`, and `memory/learning.md`, which lives outside the spec (E2:
"nothing personal (learning, notes) is in `spec/`").

**A6, learning is bounded** (`packages/core/src/learning.rs`):

| section | changes when | cap |
|---|---|---|
| `user` | "Change only when the user says it." | 1200 chars |
| `environment` | "Add only what a tool confirmed; replace when it changes." | 1600 |
| `lessons` | "Add after a task once the outcome is clear." | 1600 |

One-line entries of at most 300 chars; one core tool
`learn(section, action, text, old)`; a full section refuses `add` until
entries are merged or removed. No shipped spec has one. Rule R2 sends it
right after the goal: instructions, learning, tools, phase, artifacts, ...

**Refocus (rules C5-C7):** at 90% the `refocus` engine
(`default/refocus/spec/`) runs **progress** (a 1200-char note replaces the
history, archived to `convo/refocus-<time>.json`) then **lessons**
(`learn`: "merge with replace", "remove entries the conversation proved
wrong"). Fallback: one call with `REFOCUS_INSTRUCTIONS`.

## 3. Proposed design for dry-learn

### 3.1 `spec/goal.md`, written by the person

It replaces `instruction.md` (the loader reads `instruction.md` while there is
no `goal.md`). It is the stable start of every prompt. The engine never
edits it; it may propose an edit in a reply. To fix the vagueness, a
template with three short headings, borrowed from dsh's team sections:

```markdown
## Goal      what it is for, who it serves (one paragraph)
## Scope     what it does alone; when it stops and asks; what it never does
## Done      what a finished result looks like and how it is shown
```

Headings are advice, not parsed: any text is a valid goal.

### 3.2 The learning file: `memory/learning.md`, local

Where it lives: **not `spec/`** (shared, and never changes while running;
learning does, and holds personal facts); **not `convo/`** (what happened,
never rewritten); **not an artifact** (re-rendered each step after the
tools; learning belongs stable and early). **`memory/`**: the owner's three
parts (2026-11-19) are core, shareable specs, *personal memory*; a fourth
folder makes that explicit, as the old dry did. What is worth sharing is
promoted into the spec (3.4).

Three sections, the same for every engine:

| section | holds | written when | by | cap |
|---|---|---|---|---|
| `user` | lasting preferences and decisions the person stated | the person says it | engine (`learn`), or the person in the file | 1200 chars |
| `environment` | facts a tool confirmed: paths, versions, commands, limits | a tool showed it; replaced when a tool shows otherwise | engine, at a step or the learn pass | 1600 |
| `lessons` | rules for next time: the wrong call and the right way; what worked | an outcome is clear: a test passed or failed, a fix worked | engine, at the learn pass or a learning phase | 1600 |

Entry format: one line of at most 300 chars. Lessons end with a tally,
`- Run cargo check before cargo test here. (n=2, 11-22)`: n = outcomes that
confirmed it, then the last date.

### 3.3 Who updates it, and when (no step limits)

1. **During a turn: `learn(section, action, text, old)`**, a core tool like
   `reply` (one tool only). Result: `added to lessons (1240/1600)`.
2. **At compression: the learn pass.** After the summary call, a second
   call to the same model gets the messages `context::plan` cut, the
   current file and each section's rule and room, and answers with edits
   (`add | replace | remove`). Core applies them (`learning::apply`, pure);
   a refused edit is logged, not retried, so nothing loops. Those messages
   are leaving the prompt: the last chance to learn from them.
3. **A learning phase**, when a loop has one (the developer loop's `test`,
   a night loop's learning step), with `learn` in its tools column.
4. **The person**, editing the file or the Learning view, any time.

No automatic pass at every turn's end (a call per turn).

### 3.4 Caps, merge, promote, demote

- **Cap = refuse, never truncate** (old dry and dsh agree). A full
  section refuses `add`; the engine must `replace` (merge two entries
  into one) or `remove`. The learn pass sees each section's room.
- **Merge:** a near-duplicate `add` becomes a `replace` that bumps n.
- **Demote:** a contradicting outcome rewrites or removes a lesson; a new
  tool value replaces an environment entry.
- **Promote:** a lesson at n >= 3 is shown in the Learning view as a
  candidate. Process lessons go into a **skill** (`skill_save`, if the
  engine holds it: "skills kept current"); scope or behaviour lessons go
  into **`goal.md`**, by the person only. A promoted entry leaves the
  learning file, so the always-sent part shrinks and the on-demand part
  grows.
- **Nothing is lost:** every edit is a line in `memory/learning.log`
  (time, source `learn | pass | person`, edit, old text), like
  `messages.md` keeping every message.

### 3.5 Prompt slots

The new order, with `learning` as slot 2:

| # | slot | from | changes |
|---|---|---|---|
| 1 | instructions | `spec/goal.md` | when the person edits it |
| 2 | **learning** | `memory/learning.md`, as it was at turn start | between turns, at compression |
| 3 | phase | template + `run.md` | per phase |
| 4-7 | tools, artifacts, history, response | unchanged | unchanged |

- **Cache:** before phase, as it changes less often. Mid-turn `learn`
  writes the file but the slot keeps the turn-start copy, so slots 1-3 stay
  byte-identical within a turn; compression rewrites history anyway.
- **Never compressed:** capped, not summarised; at most ~1100 tokens
  (13% of the assumed 8192). Empty: not sent, the view says why.

### 3.6 Templates and skills

- `spec/template.md` gets an optional `## learning` section (`title:` and
  a lead); the order check counts it as slot 2. A template never has a
  `memory/`: a copy starts with empty learning.
- Skills are where lessons graduate (3.4): listed in the artifacts slot,
  loaded on demand, so a long procedure costs context only when used.
- Learning is not filtered per phase; the `skills` column is unchanged.

### 3.7 Changes to `engine-standard.md`

- **[Four folders](engine-standard.md#1-four-folders):** `spec/goal.md` replaces `instruction.md` (read as
  a fallback); new local `memory/` with `learning.md` and `learning.log`,
  never in `spec/`, never shipped.
- **[The template](engine-standard.md#2-the-template-fixed-slots-filled-by-contributors):** slot 2 `learning` in the table, the `Slot` enum (`template.rs`,
  `ALL` = 7) and the template sections table; rendered from the turn-start
  copy.
- **[The context window](engine-standard.md#3b-the-context-window-the-only-limit):** the learn pass after a compression. **[Activity](engine-standard.md#3c-activity-what-an-engine-is-doing-now):** activity `updating
  the learning`. **Core:** `learning.rs` (port of the old one) + `learn`.

## 4. Open questions for the owner

1. dsh's three sections are person-written behaviour, not learnings. Both,
   as proposed: goal.md with Goal/Scope/Done, learning.md with three
   learned sections?
2. Keep `user / environment / lessons`, or rename them?
3. Caps in fixed characters (deterministic) or a share of the window (fits
   big models)?
4. May the engine promote a lesson into a skill alone, or does every
   promotion wait for you?
5. Learn pass as a second call, or folded into the summary call (cheaper,
   maybe worse on a small local model)?
6. Session engines (claude, codex): give them `learning.md` and `learn`
   over MCP?
7. Rename `instruction.md` to `goal.md`, or keep the name?

## Decided (owner, 2026-11-22)

- **Rename to `spec/goal.md`.** The sections inside it matter: they are
  read as sections (each with its own meaning and place), not as one blob.
- **Learning comes from curated tools** that collect only the appropriate
  context (not a blanket pass over the history), then write the entry.
- **Engines are tools.** A tool can create an engine and pass it a goal.
  Engines of the same level share one context, which is how they
  communicate.

## Built (2026-11-22)

What was built from the proposal and the decisions above. Where this and
[the proposal](#3-proposed-design-for-dry-learn) differ, this is what the code does; `engine-standard.md`
([four folders](engine-standard.md#1-four-folders), [the template](engine-standard.md#2-the-template-fixed-slots-filled-by-contributors), [the goal](engine-standard.md#2a-the-goal-specgoalmd-in-sections)
and [artifacts](engine-standard.md#4-artifacts-things-that-add-to-several-slots)) holds the details.

- **`spec/goal.md`, read as sections** (`core/goal.rs`): `## Goal`,
  `## Scope`, `## Done`, each with one meaning, sent in that order in the
  instructions slot as `### Goal — what you are for, and who you serve`
  and so on. Unlike [the proposal](#31-specgoalmd-written-by-the-person), the headings *are* parsed: a missing, unknown
  or repeated section, or text before the first, is a note in the prompt
  view (`goal.md: …`); an unknown or repeated one is not sent. A file
  with no sections (an old `instruction.md`) is all Goal, with a note.
  `instruction.md` is read while there is no `goal.md`, said in the
  prompt view; the next save writes `goal.md`.
- **The learning file** (`core/learning.rs`): `memory/learning.md` and
  `memory/learning.log`, local, as [proposed](#32-the-learning-file-memorylearningmd-local): `user`, `environment`,
  `lessons`, caps 1200/1600/1600, one line of at most 300 characters.
  The Learning slot is slot 2 (`template::Slot::Learning`, `ALL` = 7),
  sent from the turn-start copy, not compressed, not sent when empty.
- **Curated tools instead of a learn pass** (the decision): a
  `kind: learning` artifact brings `collect(what)` and
  `learn(section, entry, old?)`. `collect` gathers only one section's
  context into the snapshot (corrections → user, fixes → lessons,
  facts → environment); the result line only counts them. `learn`
  refuses when a section is full; every change is a log line. There is
  no learn pass at compression and no promotion yet (step 2 of [who updates it](#33-who-updates-it-and-when-no-step-limits) and
  the promote in [caps, merge, promote, demote](#34-caps-merge-promote-demote) are not built).
- **Engines as tools** (`core/engines.rs`): a `kind: engines` artifact
  brings `engine_call` (waits; the child's reply is the result) and
  `engine_start` (returns at once; the reply comes as a message and a new
  turn). The tool writes the child's `goal.md` from `goal`, `scope`,
  `done`. Children live at the top of the store, the parent in `app.md`.
- **One shared context per level** (`core/shared.rs`): `kind: shared`,
  `post` and `edit_post`, author and time on each entry, a 4000-character
  cap that refuses. The parent reads its children's in its engines
  snapshot.
