# The shipped default: the translator and its developer

Owner, 2026-11-23: "Curate the default application towards a developer. The
default engine is a loop with skills ... The default is translator ... It
should be high level super agent that is more of a system 2 thinker."

The default is what ships: the published web app (GitHub Pages), the
binaries, and every fresh state. It lives in the repo at `default/`. The
owner's own `state/` is never overwritten by it: its own files win (§1).

## 1. What a fresh state gets

```text
default/                                  laid out as a store is (docs/engine-standard.md §1)
  app.md                                  mode, scene: cat, motion: 40; - translator
  engines/translator/
    spec/engine.md                        name, kind: model, format: toon (no model: the machine's default)
    spec/goal.md                          Goal / Scope / Done
    spec/responses.md                     the contract (thought asks for the reasoning)
    spec/tools.md                         - reply   (its only direct tool)
    spec/elements.md                      subscriptions, environment   (extra elements it holds: docs/elements.md)
    spec/skills/prompting/SKILL.md        an ask -> a sub-agent brief
    spec/skills/judging/SKILL.md          a child's reply -> accept, send back, or ask
    spec/templates/developer/             the first sub-engine (§3)
      about.md  template.md  tools.md
      artifacts/{code,terminal,evidence,browser,plan,notes}.md
      skills/{triage,plan,code-style,test,browser-qa,critique,debugging}/SKILL.md
    artifacts/alerts.md                   kind: alerts   (alert: what wakes it by itself)
    artifacts/children.md                 kind: engines  (engine_start, engine_call)
    artifacts/thread.md                   kind: shared   (post, edit_post)
```

**The rule: the default is a layer under the store.** Nearest wins, the
same cascade as shipped default → server → device:

- **Base:** the default's files. A shell on this machine reads `default/`
  live from disk, so an edit there shows at the next read. The folder is
  `DRY_DEFAULT` (a folder, relative to the repo root, or `bundled`), else
  `<repo root>/default` (`DRY_ROOT`, else the checkout the binary was built
  from). It is used only when it holds an `app.md`; otherwise, and in the
  browser, it is the files bundled into the build (`crates/core/build.rs` →
  `core/src/shipped.rs`).
- **Override:** the store (`state/`, `DRY_STORE`). A file the store holds
  at a path wins. A path it does not hold comes from the base. A default
  file the state removed (a skill, an artifact) is named in the store's
  `hidden.md` (`- <path>` lines), so it stays gone.
- **Saves** keep in the store only what differs from the base: `app.md`
  (always: it says the store holds a state), the overrides, `hidden.md`,
  and what runs make (convo, memory, logs). The default is never copied in
  again. A store file that equals the default's is dropped at the next
  save.
- **`app.md`'s settings** layer one by one: `mode:` is the store's;
  `scene:` and `motion:` (docs/scenes.md) that the store's `app.md` does
  not say come from the default's (`default/app.md`: `scene: cat`,
  `motion: 40`), and a save drops them from the store's `app.md` while
  they equal the default's, so a later default shows through.
- **A store that holds a state** (an `app.md`) keeps its engines, even
  none: its `app.md` wins, so the default adds no engine to it. A full
  copy made the old way reads exactly as before for every file it holds;
  a default file it never had (a newer skill or template of an engine it
  has, such as `translator`) now shows through. Its next save drops the
  copies that equal the default and keeps the rest.
- **The model:** a shipped engine names none. A default engine whose
  `engine.md` the store does not hold gets `models.json`'s default (or the
  browser's setting) when it is read; once it is saved with that model,
  its `engine.md` is the store's own.
- **No migration:** the old one-file `state.txt` is not read. A store
  holding no `app.md` is the default alone: the translator at the top
  level, on `models.json`'s default model (owner, 2026-11-23).

The merge is pure (`dry_core::store::{merge, split, shadows}`); the disk is
`dry_tools::store` (`Layered` over `MdStore` or `SqliteStore`; `base`,
`base_dir`). Every shell opens the store this way:

| shell | where | what |
|---|---|---|
| `dry-server` (host) | `server/src/load.rs` `read_state`; saves in `host.rs` `commit` | `dry_tools::store::load_or_first(open_default())`. `server/src/live.rs` `store_paths` watches `default/` with the store, so an edit there reads in like an outside edit of the store (and reload `spec` reads it) |
| GPUI | `gpui/src/store.rs` `load` / `save` | the same. `scripts/dev.sh` also watches `default/`, so an edit reopens the window; "reload spec" reads it in place |
| `dry` | every command that opens the store (`engine …`, `prompt <id>`, `skills`, `store`, `mcp`) | the same |
| browser (wasm) | `web/src/app.rs` `boot` | the bundled files only, on a store with nothing in it: `dry_core::shipped::with_model(<this browser's model>)`. The welcome box offers "Start with the translator" or "Load demo" |

**What shadows the default:** `dry default diff [<store>]` lists each
default file the store `overrides` (differs), holds the `same` (dropped at
the next save), or `hides`, and how many files are the store's own. `dry
store copy <from> <to>` copies through the layers: the target keeps only
what differs.

Tests: `shipped::tests` (it reads, the translator holds no action tools,
the developer template is whole, it survives a store), `dry_core::store::
tests` (a store over the default keeps only what differs; a full copy
reads as its files and its save drops the copies) and `dry_tools::store::
tests` (both stores, on disk: the default read live, removals stay
hidden).

### Reset history

Settings → **Reset history** (the first item, `reset-history` in
`routes/settings/page.json`) puts the app back to the shipped default with
a clean history: the store keeps none of its own files, so the next load is
`default/` alone (the translator, on `models.json`'s default model, no
messages). The first press opens a confirm step that says what goes, where
it goes and what stays; only its second press ("Yes, reset history") resets.

- **Moves, never deletes** (`dry_tools::store::reset`). An md store's
  folder is renamed beside it, `state` → `state.reset-<UTC stamp>/`
  (e.g. `state.reset-20261123T174500Z`), with everything in it: engine
  overrides and extra engines, `convo/` (messages, meta, run), `memory/`,
  artifacts, the terminal artifacts' logs and the analytics spans
  (`spans.jsonl`, `spans-daily.jsonl`). A new empty folder takes its place.
  A SQLite store's file is renamed the same way (`state.reset-<stamp>.db`,
  with its `-wal`/`-shm` and its `.files/` folder of logs). To undo, put the
  backup back by hand.
- **Kept:** this machine's `state/local.md` (appearance, hidden routes),
  copied back into the new folder; `default/`, `models.json`,
  `providers.json`, `sessions.json`; the host lock `state.host.json`.
- **The host** (`POST /api/reset {"confirm": true}`) does it under its
  lock and refuses (409) while an engine is in a turn, a terminal command
  runs or an approval waits: a turn is never cut off. It drops its own
  history too (spans, statuses, decided approvals, finished terminal
  commands) and bumps the generation with every engine changed, so every
  open page and the window reload. Without `confirm: true` it is 400.
- **The window** sends it to the host when one runs, else resets the store
  itself the same way.
- **`dry reset [<store>] [--yes]`** prints what would move; with `--yes`
  it resets (through the host when one runs).
- **The browser build** has no folder to move: its store's texts
  (IndexedDB, else localStorage) are removed, all but its own settings
  (`local/settings.json`: the model connection and key). A copy goes first
  to the IndexedDB database `dry-reset` under `dry.reset-<stamp>/` when
  IndexedDB opens; with localStorage only, nothing is kept. A tab on a
  paired companion's store refuses: reset that folder with `dry reset`.

Tests: `dry_tools::store::tests::a_reset_*` and `a_sqlite_reset_*`
(backup made, store empty, the default alone after, `local.md` kept),
`dry_core::store::tests::a_reset_backup_*`, and `crates/server/tests/
host.rs` `reset_history_*` (Settings shows it first; refused without
confirm and during a turn; the backup and the translator alone after).

## 2. The translator (the super agent)

It is a loop with skills: one phase, no step limit.

- **No action tools.** No file, terminal or browser. Its toolbox:
  - `reply`, with `ask=` and `show=`;
  - `skill`;
  - `engine_start` / `engine_call` (children);
  - `post` / `edit_post` (the thread; every post names `to`);
  - `alert` (its alerts artifact, since 2026-11-23): it sets its own next
    look ("in an hour", "each morning", "keep an eye on it") and is woken
    with the text it wrote, with nobody writing to it
    (docs/engine-standard.md, `kind: alerts`).

  **New thread** makes another root like it (`dry_core::shipped::new_thread`,
  `translator-2`), with its own thread (docs/desk.md).

  Learning is left out to keep the toolbox small. It can be added as one
  `kind: learning` artifact.
- **System 2.** The contract's `thought` asks what was asked, what is really
  needed, and what it does next and why.

  For code work, the first step is `skill(name="prompting")` alone. It
  picks the template (§3), then writes the brief in the next step, with
  goal, scope, done, text, template and root in their own fields. It then
  replies at once with the brief, so the person sees exactly what the
  child was asked.

  When a child replies, the first step is `skill(name="judging")` alone. It
  reads the whole reply in the children artifact against its done criteria.
  Then it accepts and reports, sends the child back with what is missing,
  starts a reviewer, or asks the person.
- **It is the manager.** Open SWE has a manager, a planner, a programmer and
  a reviewer. In dry the translator joins them: it picks the child, approves
  a plan, and moves a reviewer's blockers back to the developer. No manager
  engine is added.
- **It goes ahead on its own.** A detail with one natural reading (how a
  number is passed in, where a count starts) it decides, and says which
  reading it took in the brief and the report. It asks, with one `ask=`,
  only for a choice between two different results. With no folder named
  the work happens in the workspace of its environment line (`root="."`).

The `prompting` skill
(`default/engines/translator/spec/skills/prompting/SKILL.md`) covers:
- think first: the ask in one line, what is really needed, what a top
  engineer would add, what is unknown;
- the brief, field by field;
- the rules: point, do not paste; name the contracts; current → desired;
  the person's words; out of scope;
- the fixed report (Changed / Ran / Saw / Unverified / Also changed);
- which template: `researcher` for a question, `developer` for a change,
  `researcher` then `developer` for an area nobody has read, `reviewer`
  after a developer for anything more than a small fix;
- a **small fix** (one or two files, nothing structural, a clear done) gets
  one developer, no plan first and no reviewer: Open SWE's own lesson is
  that planning and review cost more than they find there;
- **plan first** for a structural change, more than about five files, a
  public API change, or when the person asks to see the plan;
- three examples (a change, a question, a review), and how to send work
  back.

The `judging` skill covers each kind of reply:
- a plan: approve it ("Plan approved: code it."), send it back, or ask the
  person when it holds a choice only they can make;
- a developer's report: the evidence checks, then a reviewer unless it was
  a small fix;
- a researcher's answer: every finding has a path:line and a confirmation,
  or says "read, not run";
- a reviewer's verdict: accept and report, or send the blockers word for
  word to the same developer, then review again; a blocker that comes back
  twice, or a disagreement on the facts, goes to the person.

## 3. Templates: a sub-engine made ready (`core/kit.rs`)

Three ship: `developer` (§4), `reviewer` and `researcher` (§4b). A template
is found by its folder; nothing registers it in code. The engines artifact
lists every template with its phases, skills and artifacts, and that list
is what the translator chooses from.

A parent can make a child from a template by name, so the translator never
writes a template out:

```text
engine_start(name="dev-greeting", template="developer", root="/path/to/project",
             goal="...", scope="...", done="...", text="...")
```

`spec/templates/<name>/` holds the following:
- `about.md`: one line, listed in the parent's engines artifact;
- `template.md`: the child's phases;
- `tools.md`: the child's direct tools, in place of the parent's;
- `skills/`: the child's own skills, not the parent's;
- `artifacts/`: what the child starts with. Every `root:` line is set to
  `root=`.

`root=` is accepted only with a template's name. A child with no template
cannot have its roots moved. A broken template is refused with why. The
MdStore reads and writes `templates/` at any depth. SQLite and the browser
stores keep spec files by path already. Test:
`engines::tests::a_child_made_from_a_template_gets_its_phases_tools_skills_and_artifacts_at_the_root`.

## 4. The developer template

| phase | skills | what it does |
|---|---|---|
| triage | triage, plan, debugging | looks (`ls -la`, the environment line), then writes the plan: `## Ask`, `## Steps` as `- [ ]` lines, `## Proof` with the exact command and what it must print. An empty folder or something new needs no skill; something to change loads `triage` first. An approved plan goes straight to code. With "plan first" it replies with the plan and waits. No edit here |
| code | code-style, python, debugging | **write, save and run in one act**, again until the output is right: a new file is `edit(path, new)` then `save`; the result of the run comes back at once. A missing runtime is installed as the environment line says. The same error three times loads `debugging`. Each step ticked `- [x]` when its file runs |
| test | test, python, browser-qa, critique, debugging | the Proof's commands and the project's own checks, the smallest input and a larger one; the output compared with the ask line by line; wrong → notes → back to code; right → one reply with the report, in a step of its own |

The loop was rewritten on 2026-11-23 for a small local model
(docs/loop/journal.md): each phase's first line is one short sentence (the
plan list in the prompt shows only that line), the long forms moved into
the skills, and what a phase needs every time is in the phase.

- **`python`** is the first skill that brings tools of its own
  (`python.check`, `python.run`, `python.test`; docs/engine-standard.md
  §4b).
- **`elements.md`** in the template gives the child the `environment`
  line: where its tools run, what is installed, and what can be installed
  with which command (`environment::VM_PACKS`).
- **Rules now in code, not only in the text:** a reply beside a `run` is
  held back until the result was read (`turn.rs`); a file is made with
  `edit(path, new)` (`file.rs`).

What changed on 2026-11-23, and from where:
- **Plan approval** (Open SWE's plan review, VS Code's Plan agent): with
  "plan first" in its scope, triage replies with the plan in short and
  stops. A new message starts the run at triage again (`phase.rs`), so
  triage reads the plan artifact and the approval and goes to code.
- **Steps as a to-do list** (VS Code's to-do list): the plan's steps are
  `- [ ]` lines, ticked by the code phase, checked by critique.
- **Error-driven edits** (VS Code's errors after each edit, three tries):
  a quick compile check after each saved file, in the code phase. The tests
  stay in the test phase.

The template's artifacts:
- `code` (`kind: file`), `terminal` (`kind: terminal`) and `evidence`
  (`kind: evidence`), each at the root;
- `browser` (`uses: [browser]`);
- `plan` and `notes` (plain text).

Capabilities decide what is in the prompt. The browser artifact drops out
where no companion or Chrome provides one, and its skill says so. Tools:
`reply, write, read, list`, plus what the artifacts bring. A rule in every
phase: the one reply is the report, and it never goes in the same act as a
`run`.

The skills are dry's own text, from the ideas of gstack, pstack and Matt
Pocock's skills (MIT; [skills-survey.md](skills-survey.md)).

## 4b. The reviewer and the researcher templates

Both change nothing, and both have the tools `reply, write, read, list`
plus what their artifacts bring. Neither is a new tool: each is a folder of
files beside the developer.

| template | phases | skills | artifacts | it returns |
|---|---|---|---|---|
| `reviewer` | check → verdict | review | code (`file:read`), terminal (`terminal:read`), evidence, findings | `Verdict: accept \| send back`, Blockers (file:line, what, proof), Should, Ran, Unverified, Tree |
| `researcher` | explore → answer | research | code (`file:read`), terminal (`terminal:read`), browser, findings | Answer, Findings (path:line, what, how confirmed), Ran, Unverified, Next, Tree |

- **Read-only by their artifacts** (2026-11-23,
  [capabilities.md](capabilities.md) §2 "Read mode"). `code` is
  `uses: [file:read]`: it brings `open` and `focus`, never `edit` or
  `save`; those are not in the prompt or on the engine page, and a
  hand-made call is refused with why. `terminal` is
  `uses: [terminal:read]`: it refuses the commands known to write
  (redirects, `rm`/`mv`/`cp`/`tee`, `sed -i`, git commands that change
  the tree, installs, rewriting formatters) before anything runs. That
  guard reads words, so a program that writes by itself (a script, a
  test, `node -e`) still can. So both still end with `git status
  --short`, and judging checks it. The builtin `write` stays: it saves
  the engine's own artifacts (its findings), and it refuses a file or
  terminal artifact. Tests: `capability::tests::a_read_mode_artifact_*`,
  `capability::tests::a_write_to_a_read_mode_artifact_is_refused_with_why`,
  `terminal::tests::a_read_only_terminal_refuses_the_commands_known_to_write`,
  `engines::tests::a_reviewer_made_from_its_template_cannot_edit_save_or_commit`,
  `shipped::tests::the_reviewer_and_researcher_templates_are_whole`.
- **reviewer** (Open SWE's reviewer, Matt Pocock's two reviewers, gstack's
  review). It reads `git diff` and the changed files, reruns every command
  the report quotes and the project's checks, and judges against the brief
  and against the code's standards one after the other. A finding needs a
  line or a rerun. It ends with `git status --short` to show the tree is
  as the author left it. The translator gives it the developer's brief and
  report in `text`, because it cannot see them anywhere else.
- **researcher** (VS Code's search helper and Plan agent, Open SWE's
  planner). For a question, the developer's code phase and edit tools are
  dead weight. The researcher follows the flow from its entry point and
  confirms each finding by a run; a finding it only read is marked "read,
  not run". Its findings go into a developer's `text` when a change
  follows.

Why only these two: the evidence names a planner, a programmer and a
reviewer (Open SWE), a search helper and an execution helper (VS Code). The
developer already plans and executes inside its phases, and dry's report
form already gives the execution helper's compact one-line-per-command
result. A separate reviewer and a read-only explorer are the two roles the
developer cannot play for itself.

## 5. Proof (2026-11-23, scratch only)

The run used a scratch store (`DRY_STORE=md:<scratchpad>/default/state`)
and a scratch host on port 8731, with `DRY_CAPABILITIES=none` and no MCP or
companion. The project was a scratch git repo `hello`: Bun, with
`greet.ts`, `greet.test.ts` and `index.ts`. The model was `claude/haiku`
from `models.json` with the model id `sonnet`, through `cli:claude`. The
ask: "the greeting should read 'Hello, Ada!' ... make sure it is tested."

1. The seed: `dry engine use translator claude/haiku sonnet` on the empty
   store wrote the default, and the host read it.
2. The translator loaded `prompting`, then started `dev-greeting` from the
   developer template. The brief:
   - goal: "The greeting reads 'Hello, Ada!' ... for whoever runs the hello
     project, and a test proves it";
   - scope: only the greeting and its test; check other uses; run the
     existing tests first; keep the public API; no new dependencies; never
     commit;
   - done: an exact string, a test that fails on the old output, tests
     pass, the program prints it, and the five-line report.

   In the same act it replied with that brief.
3. The developer ran each phase:
   - triage: open, grep, `git status`/`git log`, baseline `bun test` and
     `bun run index.ts Ada`, then the plan written;
   - code: two edits, saved;
   - test: `bun test` exit 0, `bun run index.ts Ada` → `Hello, Ada!`,
     `git status --short`, then `critique` and the report.
4. The translator loaded `judging` and accepted. Its report gave each file
   and each command with its exit code and last line. It also listed as
   not verified that the new test was never run against the old code, and
   that no lint is declared.

The diff in `hello` (verified by hand, `bun test` 1 pass) was
`return \`Hello, ${name}!\`;` and `toBe("Hello, Ada!")`. Transcripts,
state, the diff and screenshots of the engine pages:
`<scratchpad>/default/run4/`. Earlier runs are kept in `run1`–`run3`
(haiku).

Found with claude/haiku and fixed in the skills:
- it replied "Pending ..." in the same act as its runs, so the report was
  written before the results;
- it edited in triage and skipped the code phase;
- it twice wrote "not a git repo" without running git, and the translator
  accepted it;
- it judged the 400-character callback, not the whole reply.

Haiku still skips phases at times. Sonnet followed the loop exactly.

### Proof of the three templates (run5, 2026-11-23)

The same setup (scratch store, scratch host on port 8732,
`DRY_CAPABILITIES=none`, `claude/haiku` with the model id `sonnet`), on a
copy of `hello` committed clean. The ask: add a `--shout` flag
(`bun run index.ts Ada --shout` prints `HELLO, ADA!`, no flag stays
`Hello, Ada!`), tested, "and have the change reviewed before you tell me
it is done".

1. The translator loaded `prompting`, called it a small fix (no plan
   first), and started `dev-shout-flag` from `developer`. It replied with
   the brief, and said a reviewer would follow because the person asked.
   Its done added a case the person did not name: `--shout Ada` must not
   be taken as the name.
2. The developer:
   - triage: opened four files in one step; ran `git status`, `bun test`
     and the program three ways, which showed `--shout Ada` printing
     `Hello, --shout!`; weighed two shapes (a `shout` parameter or
     uppercasing in `index.ts`) and wrote the plan with three `- [ ]`
     steps;
   - code: three edits, saved; `bun build <file> --target bun` on each;
     the plan rewritten with every step `- [x]`;
   - test: `bun test` (2 pass), the program three ways, then `critique`
     with `git status`/`git diff --stat`, then the report alone.
3. The translator loaded `judging`, found the report held, and started
   `review-shout-flag` from `reviewer`, with the brief and the whole report
   in `text`.
4. The reviewer ran `git diff`, opened the files, reran all four commands,
   wrote its findings artifact, ran `git status --short` again, and replied
   `Verdict: accept`, no blockers. It changed nothing (the tree held only
   the developer's three files).
5. The translator reported both sets of runs side by side and listed as
   not verified that the new test was never run against the old code.

Not caught: the flag parsing in `index.ts` has no test (only `greet()` is
tested; the parse was only run by hand). The review skill now names a
changed file with behaviour and no test as a "should". It was rerun in
run6d (below), and the reviewer now flags it. The diff, state and host log:
`<scratchpad>/default/run5/`.

### Proof of read mode, plan first, the researcher and the review gap (run6, 2026-11-23)

The same setup as run5: a scratch store per run
(`DRY_STORE=md:<scratchpad>/default/run6<x>/state`), a scratch host per run
on ports 8741–8745 (`DRY_CAPABILITIES=none`, no companion, no MCP), a clean
clone of `hello` (base `5a32a97`), and `claude/haiku` with the model id
`sonnet` through `cli:claude`. The script is
`<scratchpad>/default/start-run.sh <run> <port>`. Every host was stopped
afterwards. The reviewers and researchers ran with the new read-only
artifacts (§4b). `dry prompt review-shout` on the run6c store shows the
toolbox with `open` and `focus` and no `edit` or `save`, and the headings
`### code — … (read-only: file:read …)` and `### terminal — … (read-only:
terminal:read …)`. No run tried a write, so the guard refused nothing.
Every reviewer's and researcher's last `git status --short` showed only
the author's files (or nothing).

**a. Plan first (run6a).** The ask: add `--lang` (`--lang es` prints
`¡Hola, Ada!`, none or `en` stays English, an unknown language prints an
error and exits 1), tested, "Plan first: show me the plan before any code
is written".
1. The translator loaded `prompting` and started `dev-lang-flag` from
   `developer`. Its scope began "Plan first: reply with the plan before any
   edit". It replied with the brief.
2. The developer's triage opened four files in one step, ran `bun test` and
   the program, wrote the plan (three `- [ ]` steps with file:line, the
   proof), and replied with the plan only. Nothing was edited.
3. The translator loaded `judging`. The plan covered every done line.
   Because the person had asked to see it, the translator showed the plan
   and asked (`ask="confirm"`) instead of approving it alone, as judging
   says.
4. The person (here, the run's driver) answered "Yes, the plan is
   approved". The translator sent
   "Plan approved by the person: code it as planned (greet.ts, index.ts,
   greet.test.ts) …" to the same child.
5. The developer started again at triage, read its kept plan and went
   straight to `goto_phase(name="code")`. It made two edits each in
   `greet.ts`, `index.ts` and `greet.test.ts`, saved them, ran `bun build`
   after each, and ticked the steps. Then it ran the test phase: `bun test`
   (5 pass) and the program four ways, with `--lang xx` exiting 1.
6. Judging called a new CLI flag across three files more than a small fix,
   so the translator started `review-lang-flag`. Its verdict was `accept`,
   with two "should" findings: `--lang` with no value gives a blank
   language in the error (index.ts:5, run), and a flag before the name is
   taken as the name (index.ts:3, read, not run). The translator reported
   both runs and both notes.

**b. The researcher (run6b, then run6e).** The ask: "how is the greeting
chosen, and where is it tested?"
- run6b: the translator started `research-greeting` from `researcher`. It
  opened four files, ran `bun test`, the program with and without a name,
  and `git log`. It answered with path:line findings in flow order
  (index.ts:3 → index.ts:4 → greet.ts:2-3; tested at greet.test.ts:4-6; the
  `"world"` default untested). The translator accepted it.
  - **Defect:** the translator wrote "No file was changed" with no command
    behind it, because the researcher never ran `git status`.
  - **Fixed in default/:** the research skill and the answer phase now end
    with `git status --short` as a `Tree:` line, and judging checks a
    researcher's Tree line the way it checks a reviewer's.
- run6e (the same ask, after the fix): the same findings, then
  `git status --short` (exit 0, empty), and `Tree: … the tree is
  unchanged`. The translator's judgment cited it.

**c. The review gap (run6c, then run6d).**
- run6c reran run5's ask (`--shout`, tested, reviewed). This time the
  developer tested the parse itself: a new `index.test.ts` runs the
  program for `Ada`, `Ada --shout` and `--shout Ada` (4 pass). The reviewer
  said accept with "Should: none". It gave the reason: index.ts is covered
  by a test that runs the program, and the old code would fail it. There
  was no gap to flag, so this run does not prove the rule.
- run6d replayed run5's exact diff (`greet(name, shout)`, the parse in
  `index.ts`, and only `greet()` tested). The ask was "review it before I
  merge it, and change nothing". The translator started
  `review-shout-flag`. It reran eight commands and replied `accept`, with
  this "should": `index.ts:4-6 — the flag parsing has no test.
  greet.test.ts imports only ./greet, so a break in parsing … would pass
  bun test`. It also gave a second one: an unknown flag is taken as the
  name (read, not run). Its Tree showed the author's three files only. The
  translator reported both as notes, not blockers.

Transcripts, states, diffs and host logs: `<scratchpad>/default/run6a/` to
`run6e/`.

## 6. Not done

- **The read-only terminal is a word guard, not a sandbox** (§4b). A
  run in a copy of the tree (a worktree the shell throws away) would make
  it whole; not done.
- **Plan approval by the translator alone was not run.** In run6a the
  person asked to see the plan, so the translator asked them, as judging
  says. The path where the translator approves a plan itself ("Plan
  approved: code it.") is the same message to the child, but no run has
  taken it yet.
- **A child's reply is cut at 2,000 characters** in the engines artifact
  (`engines::SNAPSHOT_REPLY`). A long plan or report is cut for the
  translator; the developer's plan reply is asked to stay under 1,500.
- **No hard phase guard.** A phase's `tools` column only adds tools, so
  `edit` exists in triage. Haiku used it. A per-phase deny list (or the
  file artifact read-only in triage) would make "no edit in triage"
  structural.
- **Done is not gated on a passing run.** A reply beside a `run` is held
  back now, but nothing refuses a report with no run after the last save
  (bolt.diy's rule, docs/loop/bolt-study.md).
- **The host approval step for unattended `run`** is still planned
  (engine-standard §4b). The developer runs what the model asks, in the
  root.
- **The browser and evidence paths were not exercised.** The scratch
  project had no `evidence.md` and the host had no companion. The browser
  QA path is written, not run.
- **Not run in the window or the browser build.** The GPUI window and the
  web build were not opened to see the translator; both compile and seed
  from the same function.
- **The server's engine page by id.** `/desk/translator` shows "Not
  found" panes; `/desk/0` works. This is a server/HTML matter, left to
  the agent working there.
