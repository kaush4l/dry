# The Desk

The Desk (`/desk`) shows every live engine as a tree and one **thread** per
root (top-level engine): the conversation you have with that engine and
every engine under it. The work on a thread shows as a session stream, like
a Claude Code session: each message, and under it the engine it called
working on it, step by step, live.

```text
┌ tree ─────────┬ thread ──────────────────────────────────┐
│ ● you         │ Translator's thread · you translator …    │
│  ● translator │ you → helper                   11-23 14:05│
│   ● helper    │   find why the greeting test fails        │
│    ● deep     │   ◐ helper  calling the LLM               │
│   ● tester    │   │ Read the test first.                  │
│  ● claude-1   │   │ ✓ read  name="greeting_test"  → …  ▸  │
│               │   │ ✕ run   command="cargo test"  → error │
│ ＋ New thread  ├───────────────────────────────────────────┤
│ ＋ New session │ to [helper ▾] type here…          ↑ Send  │ pinned
└───────────────┴───────────────────────────────────────────┘
```

## Threads

A thread is shared. Each root (an engine with no parent, or whose parent
is gone) is the **head** of one thread, and every engine in its whole tree
is on it with you: its children, theirs, sessions and their child engines,
any engine that is a tool of any engine there. There are no threads below
a root. A thread is a list of messages (`crate::shared`), kept in
`state/engines/<root>/shared.md`. Nothing else about a thread is stored: the
Desk model (`dry_core::desk::build`) is derived at runtime from the
engines' `parent` links, the posts and each engine's live status. Making a
child puts it on its root's thread; making a root makes a new thread.

- **Tree** (left): you, then the engines in their hierarchy, each with its
  status dot and what it is doing. Selecting any node shows its root's
  thread with the composer's `to` set to that node (`/desk?thread=<root>&to=<id>`;
  `?board=` is the old name and still reads).
- **Thread** (main): the participants with their status (you, the head,
  the rest of the tree, then read-only authors), then each post: from → to,
  when, the text, and under it the target engine's turn on it: thoughts,
  each tool call as one line (`✓ name args → first result line`, click to
  open the whole call and result), the reply, and a status (queued,
  running, waiting, done, failed). The turn is the engine's own messages
  (`dry_core::desk::turns`), the same data the engine page shows; the
  answer being written and a session program's live rows appear under a
  running turn. Live updates come over the same SSE events as every page.
- **Composer** (footer, pinned to the bottom of the thread's column at every
  width): a guided post on the selected thread, to the engine picked in
  `to` (default: the head). It always sends a `to`.
  `POST /api/threads/:root/posts {text, to}` (`top`: the first thread;
  `POST /api/boards/:level/posts` is the old route and does the same).

The engine page (`/desk/:id`) uses the same stream (`session` component):
your messages, the posts that called it, each turn's steps and reply, with
its composer pinned under the stream and the timeline beside it.

## New thread and New session

There is no "New engine" for you any more. The sidebar, the Desk's tree and
Settings → Engines offer:

- **New thread**: a new root engine made from the default's translator
  (`dry_core::shipped::new_thread`: its spec, goal, skills, templates,
  artifacts, contract and tools, as `default/engines/translator` holds
  them; a native shell reads `default/` from disk, the browser the bundled
  copy), with a fresh unused id (`translator-2`, named `Translator 2`), the
  machine's default model when it names none, and no conversation. Its
  thread opens on the Desk with the composer on it and focused.
- **New session**: a root run by a program in `sessions.json` (the existing
  session flow), which is also its own thread. With several programs the
  sidebar shows a picker, one per program.

`POST /api/engines {kind}` makes either (`kind: thread`, or a program's
name; the old `model` and no kind make a thread). The `new-engine` page
action is kept as an alias of `new-thread` for pages drawn before. Child
engines are still made by engines (`engine_start`), and tests use
`State::add_engine`. The companion's "give the folder to an engine" pages
say New thread too: that makes a root (so its own thread) that is a plain
model engine with the folder's `files` and `terminal`, not the translator,
which holds no file or terminal tools by design.

## The guided-message rule

**An engine is only called when its thread has a guided message for it.**
A guided message is a post addressed to it (by you or an engine). So
invoking any engine of a tree is a post addressed to it, by id, on its
root's thread; the rule wakes exactly that engine.

**A post with no `to` goes back to its sender.** It wakes nobody else:

- An engine's unaddressed post **bounces**: it is delivered back to the
  sender as a notice, which starts its turn so it can post again properly.
  The Desk marks it `bounced: no to`, so you can see which engine's prompt
  needs fixing. A bounce is not a post, so it never bounces again.
- Your unaddressed post (only the API can send one) is kept, wakes nobody,
  and shows as `not delivered: no addressee`.
- A read-only participant's post (a session's sub-agent) is a report: it
  calls nobody and never bounces. A post addressed to `you` calls nobody.

Each engine keeps how far it has heard its thread (`heard: 7` in
`convo/run.md`, a count of entries). On every settle (after each turn,
every two seconds on the host, at once after a post) `dry_core::turn::callbacks`
delivers what is new (`dry_core::desk::guided`) as one user message:

```text
Guided posts for you:
[thread 2] you → helper: make the greeting test pass
[thread 5] Not delivered — post #5 had no `to`. Address it to one of: you, translator, tester, with the context the recipient needs to act on it cold (what, why, what done looks like, where the material is). It said: found it
```

and the shell starts its turn, the same path an `engine_start` reply takes.
A busy engine hears its posts when its turn ends; each post (and each
bounce) is delivered once. **A wake keeps the run where it was**
(`State::add_wake`, owner 2026-11-23): guided posts, yours included, a
child's reply, a bounce and a capability event all leave the engine's
phase, step and loaded skills as they were. Only a direct new ask on the
engine's page, or a parent's `engine_start`/`engine_call` (`add_text`),
starts the run again at its first phase.

**Waiting on its own job.** An engine that starts a long check with
`start(command, wait="yes")` ends its turn after that step with no reply:
its turn on the Desk shows `waiting` (the call's line reads `waiting for
session 3 … to end`), its parent hears nothing (no reply, no callback),
and the wait is saved (`waiting: term#3` in `run.md`), so a restart keeps
it. When the job ends its end is the wake (`State::add_wake`): the run
goes on in the phase it was in, with the job's output in the terminal
artifact. Several jobs waited on in one step wake it once, when the last
has ended. A job that died with the app wakes it with "no exit code …
its result is not known". Your new ask cancels the wait
(`engines::stop_waiting`); the job's end then comes as a plain callback.
So a developer replies once, with its report, after the check has ended
(`crates/tools/tests/one_loop.rs`).

An engine from a store made before this rule (no
`heard:` line), or before threads (a `heard: 3.5` cursor, two per-level
counts), counts what its thread has as heard, so old posts call nobody.

**Prompts ask for it.** Wherever a post is taught (the `post` MCP tool, the
`shared` artifact's text, the session snapshot's tools line, the
translator's goal and its `prompting` skill), it says: every post names
`to`, and carries what the addressee needs to act on it cold (what, why,
what done looks like, where the material is).

Approval notices are posted on the engine's thread addressed to you, so
they call no engine.

## Stores from before threads

Before threads a store kept one file per level: `state/shared.md` for the
top level (all top-level engines and you) and
`state/engines/<parent>/shared.md` for each parent's children. On load
(`dry_core::store::from_texts`) `dry_core::shared::fold` moves them into
the roots' threads, nothing lost:

- a deeper engine's file goes into its root's thread;
- each post of the old top file goes into the thread of the engine it is
  for, else of its author, else the translator's (else the first root's);
- merged by time, a post already there kept once; the old files are left
  empty, so the next save removes them. Folding again moves nothing.

At the same load every engine on a thread that took posts has heard all of
it (a new engine with no messages too), and an old cursor (`3.5`, it
counted other files) or a missing one becomes what its thread holds; the
next save keeps that cursor. So no old post calls anyone, on this load or
a later one.

Before the guided rule an addressed post only waited: a model engine saw it
in its `shared` artifact on its next turn and a session in its snapshot's
inbox, but nothing started that turn.
