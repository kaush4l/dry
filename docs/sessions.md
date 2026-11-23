# Sessions as workers

How a Claude Code session (or any program in `sessions.json`) works inside
dry. Owner's decisions, 2026-11-23: the dry super engine stays the top and
owns the context (snapshots, compression); a session is a **worker** with a
subset of tools; a thread is a shared list of messages, one per root
(top-level engine), shared by you and every engine of its tree
(docs/desk.md); few tools per engine.

```text
                    the user (on every thread)
                             │
        ┌────────────────────┴───────────────────┐
   claude-3 (session, a root: its own thread)   translator (a root: its own thread)
     │  push: hooks → `dry snapshot`  (SessionStart, UserPromptSubmit)
     │  pull: MCP `dry mcp engines`   (state, artifact, thread, post,
     │                                 engine_start, engine_call)
     │  ask:  --permission-prompt-tool mcp__engines__approve
     │  live: --output-format stream-json → tool rows, agents, `/` list
     │
     ├── adder (a real dry engine, child of claude-3)   on state/engines/claude-3/shared.md
     └── agent-say-ok (a sub-agent: read-only participant on that thread)
```

## 1. The host runs the session

A session engine's turn runs on the host (`crates/server/src/host.rs`,
`run_session`), like a model engine's: a message to it (from a page, the
window, or a callback such as a child's reply) starts its program, one
thread per turn, in the repo root. The command, its settings file and its
environment come from `sessions.json` (`dry_core::session::Tool`) and are
filled by `dry_tools::worker::prepare`:

| placeholder | becomes |
|---|---|
| `{prompt}` | the newest text (one argument, never read by a shell) |
| `{id}` | the program's own session id (the next turn `--resume`s it) |
| `{settings}` | `<store>.sessions/<engine>/settings.json`, written from `settings` |
| `{engine}` | the engine's id (also `DRY_ENGINE` in `env`) |
| `{dry}`, `{cursor}` | the `dry` command; `<store>.sessions/<engine>/cursor` |

Filling is one pass, so a prompt that contains `{id}` stays as typed. The
session folder sits beside the store (`state.sessions/`, git-ignored), never
in it, and the owner's own `~/.claude` settings are never touched: each
session gets `--settings <its file>`.

Each turn is `claude -p … --resume <id>`: a new process per message. A
long-running process with `--input-format stream-json` was not built: it
adds a process to keep alive, restart and route stdin to, for little gain
while every turn already resumes the same conversation.

## 2. Push: hooks with the live snapshot

The settings file holds two hooks (`sessions.json`, `settings.hooks`):

- **SessionStart** (`startup|clear|compact`; a resumed turn keeps its
  context): `dry snapshot --engine <id> --start` — who it is, its goal,
  the participants of its thread (its head, every other engine of the
  tree, read-only participants; you post as `you`), its tools and what it
  may use without asking.
- **UserPromptSubmit**: `dry snapshot --engine <id> --cursor <file>` — its
  goal line, its phase, its artifacts' snapshots as the prompt's artifacts
  slot renders them (each cut to 600 characters, saying so), and the
  thread's posts for it since its cursor; the new cursor is written back.

Every snapshot's first line says it **supersedes every earlier dry
snapshot**. The whole text is at most 4000 characters; over that, the
longest parts are cut and the text says what was cut (`state()` and
`artifact(name)` have all of it). A hook's stdout is added to the turn as
context. `dry snapshot` reads through the host when one runs (its lock file
and `GET /api/engines/:id/read`), else the store, read-only. The text is
`dry_core::snapshot` (pure, tested).

## 3. Pull: the `engines` MCP group

`dry mcp engines` (`mcp/engines.json`, in the session's `--mcp-config` and
`--allowedTools mcp__engines`), for the engine `DRY_ENGINE` names:

| tool | does |
|---|---|
| `state(since?)` | the turn snapshot now |
| `artifact(name)` | one artifact in full (a kind: its live snapshot) |
| `thread(read?, since?)` | `thread` (all of it) or `inbox` (only the posts for it), from a cursor; `dashboard` is its old name and still answers (and an allow rule for `mcp__engines__dashboard` allows it) |
| `post(text, to)` | a post on its thread; see routing |
| `engine_start(goal, template?, name?, text?)` | a child engine; returns at once |
| `engine_call(id, text)` | a message to a child; waits for its reply (no time limit) |
| `approve` | the permission prompt tool (Claude Code calls it, not the model) |

**One writer.** Every write goes through the host's API
(`POST /api/engines/:id/posts`, `…/children`, `…/children/:child/take`,
`/api/approvals`). With no host running a write is refused in one line
(`not done: no dry host is running, and only the host writes the store …`);
reads go to the store.

**Children are real dry engines.** `engine_start` is the `engines`
artifact's own call (`dry_core::engines::call`, an `engines` artifact is
added to the session the first time): the child is in the store, in the UI,
a child of the session, on the session's thread, with a `shared`
artifact. A session has no model or contract to hand down, so a session's
child gets the app's default model (`models.json`), contract
(`responses.md`) and MCP spec. When it replies, the session is woken with
`engine adder replied: "…"` (the callback every parent gets) and runs a
turn. `engine_call` takes the reply as its result instead (the host marks it
had, so it does not also come back as a message).

## 4. Threads: lists of messages

A thread is a root's shared context (`kind: shared`, `dry_core::shared`),
with an addressee per post. Every engine of the root's tree (a session, its
child engines, theirs) is on the one thread, with you:

```
- 1795418421 adder → claude-3: 4
- 1795418521 agent-say-ok: started (general-purpose): say ok
- 1795418600 claude-3 → adder: try turn.rs
```

- **Routing** (`dry_core::snapshot::inbox`): a post is for the engine it
  names; its next turn starts with it (docs/desk.md, the guided-message
  rule). A post with no `to` wakes nobody: an engine's comes back to it as
  a bounce (`(not delivered: it has no to)` in its snapshot), so it posts
  again to someone. `to: you` is for the user and calls nobody.
- **Every post names `to`** and carries what the addressee needs to act on
  it cold: what, why, what done looks like, where the material is. The
  `post` tool's description and the snapshot's tools line say so.
- **Cursor**: one count of the thread's entries (`7`). Removing an entry
  shifts the count, so a cursor may then miss one post; removing is rare
  and `thread` always has the list. A cursor from before threads (`3.5`)
  reads as `0`.
- **The cap, redesigned.** It was 4000 characters per level, refusing a post
  that did not fit: right for a small shared notepad, wrong for a list of
  messages that grows with every report and approval. Now the thread keeps
  every entry, and `MAX_CHARS` (6000) is only how much of the newest a
  snapshot shows, with `(N older entries not shown; the thread keeps them
  all)`. An entry is still at most 600 characters.

## 5. Approvals

`--permission-prompt-tool mcp__engines__approve`: when Claude Code would
ask permission, it calls `approve({tool_name, input})`.

1. **Rules first.** The engine's spec allows some tools without asking:
   `allow:` in `engine.md`, comma separated, a tool name or a name and a
   pattern for its main input (`Read, Grep, Bash(git status*),
   mcp__engines__*`). A new session gets the read-only ones
   (`dry_core::approval::DEFAULT_ALLOW`). A rule only allows; nothing denies
   on its own.
2. **Else a person.** The host keeps an approval record
   (`POST /api/approvals`), posts a notice to you on the engine's thread,
   addressed to you so it calls no engine (`claude-3 → you: approval a2:
   claude-3 asks to use Write(…) — Allow or Deny it in the app`), and sends an `approval` event. The web UI shows the
   pending ones on the engine's page and on the Desk, with **Allow** and
   **Deny** (`POST /api/approvals/:id {decision, reason}`; a deny asks why,
   and the session reads the reason). The tool waits (polling the host) and
   answers `{"behavior":"allow","updatedInput":…}` or
   `{"behavior":"deny","message":…}`.
3. **No timeout by default**: a night run waits for you.
   `approve_timeout: <seconds>` in `engine.md` denies after that.
4. **No host**: rules still allow; anything else is denied with the reason.

Records live in the host's memory (the newest 50 decided are kept to read
back); a host restart forgets pending ones and their waiting tool denies.
A parent engine deciding its child's approvals is not built: the notice
is on the thread it shares, the decision is a person's.

## 6. The live UI from stream-json

The session runs with `--output-format stream-json --verbose`; the host
reads it line by line (`dry_core::stream_json`, a pure parser tested on a
recorded run in `crates/core/tests/data/claude-stream.jsonl`):

- **Tool rows.** Each `tool_use` is a row (`Bash(echo hi)`, running), closed
  by its `tool_result` (its first line, or `error: …`); a `tool` event per
  change, so the page redraws as each call starts and ends. The activity
  says what runs (`claude: running Bash(echo hi)`). At the end of the turn
  the rows are saved as one tools message, like a dry engine's calls
  (`Bash(echo hi) → hi`), before the replies.
- **Agents.** A `Task`/`Agent` call is mirrored as a read-only participant
  on the session's thread (`agent-say-ok: started (general-purpose): say
  ok`, then `done — completed: ok`), from `task_started` and
  `task_notification` (or the call's own result in the foreground).
- **Results.** One run can end several turns (a background agent finishing
  wakes it): every result is a reply, in order.
- **The `/` list.** The `init` event's `slash_commands` and `skills` are kept
  in the engine's metadata (`init.slash_commands`); the web composer pointed
  at that session lists them as you type `/` (Tab or Enter picks). Not in
  the GPUI window yet (its composer has no popup to put them in).

The GPUI window, when no host runs, still runs a session itself
(`ask_session`): it uses the same `prepare` (settings file, hooks, `DRY_ENGINE`)
and reads the stream at the end, so it gets the reply, id and metadata but
no live rows.

## Checked

`cargo test`: the parser on the recorded run, snapshot rendering and
routing, approval rules and records, the session command and settings file,
the host's approvals and posts (`host.rs`), and the worker's tools against
the real host binary (`crates/server/tests/workers.rs`: reads with no host,
writes refused without one, a child made through the host, an approval
asked, shown on the page, allowed, and answered). In the browser, on a
scratch host with `claude` on haiku: a session's tool rows appear live, it
starts a dry child that posts `4` on its thread and replies, which wakes
the session; a Bash and a Write show as approvals that were allowed; `/`
lists its commands; an Agent call shows as a participant on its thread.
(That browser run was before threads; the thread tests are
`crates/core/src/desk.rs` and `crates/server/tests/workers.rs`.)
