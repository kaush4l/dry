# Black-box test of dry-learn, 2026-11-24

Six testers with a fresh context each used the app as a person would, on six
scratch hosts built from `main` (`992cc0f`), against the local model
(Inkling-2-Mini on oMLX 8873 and LM Studio 1234). They read no source and no
docs. Their full reports are in [testers/](testers/); the brief they got is
[BRIEF.md](BRIEF.md); the scratch-host script is [host.sh](host.sh).

Afterwards I checked the top findings against the store files and the code.
A finding marked **confirmed** was checked that way. One marked **seen** rests
on a tester's report only.

## Verdict

The parts work. The whole does not yet carry a goal by itself on a local model.

- The app's own suite is green: 543 passed, 0 failed, 2 ignored (core, tools,
  server, html).
- What works end to end: a turn, streaming, ask controls (choice, form, table),
  a child engine showing live in the UI, three children running at once,
  alerts firing by themselves, a restart resumed by the keep-going alert,
  themes and pages redrawn from files in about 2 s with no restart.
- What does not: the default engine cannot change the app (no tool for it);
  there is no answer engine; there is no refocus; a goal left alone for 60
  minutes produced one file of five and no summary; the engine sees no usage.
- No tester saw the translator claim something it had not done. Honest
  failure is the app's strongest property today.

The model queue was slow (six hosts on one model; single calls took 9 s to
over 600 s). That is not counted as a finding. What the app does while it
waits, and what it does when a call is slow, is.

## Your goals against what was seen

| What you asked to test | Result | What was seen |
|---|---|---|
| Change colour by asking | FAIL | "switch to light mode" started a researcher child on the app's files; the app stayed dark. `POST /api/mode` and `/api/theme` do it at once |
| Change layout by asking | FAIL | "add a page called Notes" took 601 s and ended in a hand-off brief, no page. `/api/routes/create` does it at once. No control anywhere reorders pages |
| Create an engine by asking | FAIL | "create an engine called poet" failed with `timeout: global`; the model spent 10 minutes choosing between the four templates, none of which fits |
| Answer in any style, through an answer engine | FAIL | No answer engine exists. A planets question asked "as a table" went to a deep-research child that returned 7 empty answers |
| Compaction refocuses on the right elements | PARTIAL | It compresses, and the essentials survived three rounds, because the model chose to keep them. Nothing in the app asks for plan, directions, done, to do, notes |
| Refocus at 90%, with notes | FAIL | The trigger is 75%. There is no refocus, no manual trigger, no way to see or edit the summary |
| Engines place alerts | PASS | Placed, shown in the prompt and the store, one-shot removed after firing |
| Alerts call the loop back | PASS | A turn started by itself 1.3 s after the due time; fired once after a busy turn (25 s late) and once after a restart (61 s late) |
| Several engine calls in parallel | PARTIAL | Three children busy together when asked for "a separate child each". Asked naturally, the same job went to one developer with no plan |
| A new sub-engine shows in the UI, with its loop | PARTIAL | The child appears everywhere within about 32 s and its live view works. The Desk says "No posts yet" while two engines work |
| Sessions | PASS, with one gap | Threads are isolated. A Claude session (haiku) ran on a seventh scratch host: it kept its memory across turns, read the app's state, posted on its thread, started a child and got its answer, all shown live. It cannot see or reach another thread ([Sessions](#sessions)) |
| Sub-engine picked by the query | PASS | Researcher for a folder question, developer for builds, answered a capital-city question itself |
| Shared context, as in t3 code | PARTIAL | A child sees thread posts addressed to it, but only after its turn ends. A direct follow-up to a finished child overwrote its report in the parent's view |
| A dynamic UI defined by data | PARTIAL | Pages, layouts, tokens and themes follow file edits live. The parts themselves (`dashboard`, `desk`, `session`) are sealed blocks |
| Usage metadata collected | PARTIAL | A span per call with model, provider, purpose, time to first token. Token counts are estimates and leave out reasoning |
| The loop gets usage in short-hand | FAIL | No usage, time, cost or budget is in any engine's prompt |
| Plans a bigger job overnight | FAIL | 60 minutes alone: turn died at minute 11, plan at 22, child at 26, first file at 54; 0 of 3 plan lines ticked |
| A strict way to handle failure | PARTIAL | A missing data file was diagnosed and put to the person as a two-way choice. A read-only folder was `chmod`-ed without asking |

## Findings, most severe first

### Blockers

**1. A model call is killed at 600 s, and the turn dies with it.** Confirmed:
`crates/tools/src/net.rs:18` (`TIMEOUT`, applied with `timeout_global`). It
cuts a call that is still streaming. The person sees only `failed`; the parent
of a child gets `engine <x> failed: timeout: global` with no partial result.
The turn is not retried, and the keep-going alert cannot rescue it when no plan
exists yet. Three testers hit it (A, B, E). This contradicts "no step limit:
the context window is the only limit", and it is the main reason the long goal
stalled.

**2. The default engine has no tool to change the app.** Confirmed from its
prompt: `reply`, `write`, `skill`, `alert`, `engine_call`, `engine_start`,
`post`, `edit_post`. Nothing for mode, theme, tokens, routes or engine
settings, and the prompt never says the app has them. The host already carries
out every one of these as an intent (`/api/mode`, `/api/theme`,
`/api/routes/*`), so the gap is only the tool in front of them.

**3. A repeating alert on a slow model locks the person out.** Seen (C): each
queued fire starts the moment the last turn ends, so every message from the
person got 409 "engine is in a turn" for over 9 minutes. There is no Stop
control, and a busy engine cannot be deleted.

### Major

**4. No refocus.** Confirmed in `crates/core/src/context.rs`: the trigger is
`COMPRESS_AT = 0.75`, and the instruction is one paragraph of free prose
("what the user asked for and still wants, decisions made, facts learned …
what is still open or next"). No slots for plan, directions, done, to do,
notes. Seen (D):

- Summaries grew, 1,763 → 2,166 → 2,632 characters. The third turned 93 tokens
  of history into a 656-token summary and left the prompt at 79%, above the
  trigger. Each compression took 209 to 270 s.
- When the fixed part plus the newest message is over the window, nothing is
  compressed and the prompt is sent as it is (seen at 101%, 192% and 276%).
  Only the engine's page says so.
- The fixed prompt is about 44% of an 8k window; a developer child starts at
  104% of a 4k window.
- Standing directions live only in the summary's prose. Notes survived because
  the model invented an artifact for them; the prompt does not tell it to.

**5. A child's reply can be lost.** Confirmed in tester F's store: `dev-hello`
ended with a `reply`, the translator's `children.md` still says `start 0`, and
its `run.md` says `heard: 0`. The translator was answering the person when the
child finished. It stayed idle on "waiting for its report" for good. Tester B
saw a related loss: a direct follow-up to a finished child replaced its report
in the parent's view while the parent was judging it (only the last reply is
kept).

**6. About a third of model calls are thrown away as "no act".** Confirmed: 52
times across the six stores, always answered with the same sentence ("Your
answer did not follow the response contract (the answer has no act) …"). By
host: 18 of 45 (A), 16 of 41 (E), 8 of 61 (B). The model writes its `thought:`
and stops; one child repeated the same thought six times. The model is at
fault for the text. The app is at fault for the retry: it says the same thing
each time and has no second strategy. Native `tool_calls` are handled
(`inference.rs:172`), so that is not the cause.

**7. Factual questions are sent to deep-research.** Seen (A), and it follows
from the goal text: "a question about the world that needs the web to
`deep-research`". A question the model could answer goes to a child, and the
format the person asked for is not passed on. There is no template for a plain
answer.

**8. Turns started by an alert look like the person wrote them.** Seen by C and
E: the fire is stored and drawn as a user message, the span says
`"trigger":"user"`, the timeline says "turn · user". Nothing on the page says
why a message appeared.

**9. An alert turn cannot end quietly.** Seen (C): a turn ends only with
`reply`, so "tell me only when the file appears" has no valid ending. The
model spun on that for 10 minutes until finding 1 killed it. The second try
fired 7 times in 18 minutes, made one real check through a child, and never
noticed the file. The translator has no file tool, so every check costs a
child.

**10. The engine has no clock.** Seen (C): asked for the time, it could not
say. The prompt carries no current time, and alert messages carry no due, fire
or late time.

**11. Usage is estimated and stops at the spans file.** Confirmed in a span:
`"tokens_est":true`. Seen (E): host `e` recorded 137k prompt tokens and 3.4k
completion tokens for 52 minutes of model time, because reasoning is not
counted. A timed-out call has no completion count; a call cut by a restart
(about 290 s) is in no span. Cost is "–" everywhere, there is no total per
goal, and the children artifact shows only "idle · 0 replies".

**12. A command's output reaches the child cut, with a wrong note.** Seen (B):
30 lines came back as the last 15 with a note that says "the last 30". The
child could not verify FizzBuzz, spent 7 steps trying, and hit finding 1.

**13. Route create accepts a path with `..`.** Confirmed:
`crates/tools/src/lib.rs:232` (`create_page`) checks nothing.
`POST /api/routes/create {"path":"../evil"}` wrote `root/evil/page.json`
outside `routes/`. On a host bound wider than loopback, a token holder can
write a `page.json` anywhere the server's user can.

**14. Delete shifts pages and can remove the default.** Seen (B): `/desk/<n>`
is an index, so after a delete the same address shows another engine. The
translator itself can be deleted; then its page is 404 and nothing recreates
it.

### Minor and polish

- The Desk says "No posts yet" while engines work; a translator waiting on its
  child shows `idle`; Home says "Need you 0 · nothing" while a question to the
  person is open (B, E, F).
- A wait of 60 to 120 s shows only "calling the LLM" with no clock (F).
- No auto-scroll; the sticky composer covers new replies and ask cards (F).
- Each stream event refetches the whole page: 77 GETs of about 39 KB in one
  30 s turn (F).
- After a 409 the red "engine is in a turn" stays when the turn has ended (F).
- A typo in a `page.json` turns the page into a silent 404; the real error is
  only under Settings → Routes (F).
- On a phone width the menu covers the content and the composer row overflows
  (F).
- A past date (`at 2020-01-01 09:00`) is accepted as an alert (C).
- A post is cut at 600 characters; a failed save drops the unsaved text (E).
- A restart leaves no trace for the person (E).
- A child ran `chmod` on a read-only folder without asking (E).
- "New thread" makes "Translator 2" with no name or goal (A).

## The UI as data

Tester F found about 35 part types. Working from files alone, with open pages
following in about 2 s: new pages, reordering and removing parts, title, icon
and order, layouts, local components, a token's value, a whole new theme.

What is hard-wired: `dashboard`, `desk` and `session` are each one sealed
block. There is no general stat, list, card or action part, so a page cannot
show an engine's value or call an intent unless a block was written for it in
Rust. That is the gap between "pages are data" (true) and "define components
and behaviour only" (not yet).

## Sessions

Run by me after the swarm, on a seventh scratch host (8151) with the `claude`
row of `sessions.json` on haiku and the app's own three MCP servers
(`dry mcp`, stdio, pointed at the scratch store). Two turns, 16 s and 20 s.

| Asked | Result | Seen |
|---|---|---|
| New session from the API | PASS | `POST /api/engines {"kind":"claude"}` made `claude-2`; its page shows the session id, live tool rows and a timeline |
| "Remember KESTREL", then ask in the next turn | PASS | Answered KESTREL: `--resume` keeps the session's own context |
| Read the app's state and post on the thread | PASS | `engines.state`, then `engines.post` ("posted entry 1") |
| Start a child and get its answer | PASS | `echo` appeared in `/api/status` while the session worked, ran on the local model, replied OK. The first `engine_call` was refused ("echo is still working") and the session retried by itself |
| See or message the translator | FAIL | `error: translator is not a child of claude-2`; its state snapshot lists only its own tree |

Three things to know:

- **No shared context across threads.** A session reaches only the engines it
  started. The t3-code behaviour you named (a thread invoking every other
  session, with a shared context) is not there: each root is its own island,
  for a session and for the translator alike.
- **Usage is real here, and half of it is missing.** The span has
  `cost_usd: 0.0164` and `completion_tokens: 1131`, but `prompt_tokens: 8`:
  the cached part of the prompt is not counted, so input looks free.
- **A session loads your own Claude setup.** Its page lists your slash
  commands and skills (`trade-desk`, `trader-broker` and the rest). A worker
  inside dry can reach whatever your global Claude configuration allows.

## What was not tested

- The GPUI window and the wasm build. Only the hosted HTML was driven: the
  window would open on your screen and follow your real store, and the wasm
  build needs its own served copy. Both run the same core loop.
- Codex, pi and hermes sessions; a translator handing work to a session row.
- The keep-going alert on an idle plan (C ran out of time; E saw it resume a
  restart after 9 min 48 s).
- More than 8 alerts; a child running through a compaction; a server that
  rejects an over-window prompt.
- Controls behind a native `confirm()` (Reset history and others): the test
  browser could not click them.

One caution about the setup: testers B and C report that children write into
the app's root folder, not the workspace. On my scratch hosts `DRY_ROOT` was a
copy beside an empty `proj/`, and the environment line named the root. I did
not check whether a normal install does the same.

## A strict plan: what to change, in order

Each step names the one thing to change and the check that says it worked.
The order is by what unblocks the next test, not by size. None of it is built.

| # | Change | Check |
|---|---|---|
| 1 | The model-call timeout: an idle timeout (no bytes for N s), not a total one; a failed turn tells the person why in the chat and wakes its parent with what the child had so far | A 15-minute streaming call finishes. A dead server fails in N s with a message in the thread |
| 2 | A Stop control for a turn, and a person's message goes ahead of queued alert fires | With an `every 1m` alert on a slow model, a message is taken within one turn |
| 3 | The "no act" retry: say what was received and what is missing, and on the second miss change strategy (ask for the `act` line alone, with the thought already kept) | Rejected calls under 5% on the same six scenarios |
| 4 | Settle a child's reply when the parent's turn ends, whoever started that turn; keep every unjudged reply, not the last | Tester F's sequence (child ends while the parent answers the person) wakes the parent |
| 5 | An `app` tool for the translator over the existing intents: mode, theme, token, route create/hide/order, engine create with a goal and no template | "switch to light mode" changes the page in one step and the reply says what changed |
| 6 | An `answer` template (no tools, one phase, a `style` field), and the goal text sends plain questions to it | "the planets as a table" comes back as a table from one child, in one round |
| 7 | Refocus: at 90%, a structured call that fills five named slots (plan, directions, done, to do, notes) into an artifact the prompt always shows; a hard size cap checked after the call; a second pass when the result is over the cap; a manual trigger and an editable result | The D script: after three rounds the slots hold the seeded facts, each result is smaller than its input, and the prompt is under 60% |
| 8 | A fixed-prompt budget: tools and skills cut to the phase, so a developer child fits a 4k window with room | `dry prompt` for a fresh developer child is under 50% of 8k |
| 9 | Usage: read the server's `usage` when it sends one, count reasoning, and add one short line per engine to the prompt (calls, tokens in and out, minutes, per child) | The translator can answer "what did dev-x cost" from its prompt alone |
| 10 | The trigger of a turn (`user`, `alert:<name>`, `child:<id>`, `restart`) in the span, the message and the page; the current time in the prompt | An alert turn is drawn as an alert, not as the person |
| 11 | Validate a route's path (no `..`, no absolute path); protect the default engine from delete; address engine pages by id | The traversal request is refused with a reason |
| 12 | Reach across roots: a `threads` read (every root's name, state and last reply) and a post to another root's thread, for the translator and for sessions | A session answers "what is the translator doing" and leaves it a message |
| 13 | A general `value` part and an `action` part bound to named values and intents, so `dashboard` can be rebuilt from them | Home is rebuilt from `page.json` alone and matches today's |

When a step's check fails, the order of questions is the same each time:

1. Is the prompt what you expect? `dry prompt <engine>` shows exactly what the
   model got.
2. Did the model answer within the contract? Count "no act" and empty answers
   in `convo/messages.md`.
3. Did the app do what the answer said? Compare the `### tools` block with the
   files and `/api/status`.
4. Did the person see it? Read the page, not the store.

A failure at 1, 3 or 4 is the app's. Only a failure at 2, with 1 right, is
the model's.

## Owner decisions on the plan (2026-11-24)

Given after reading the report; they overrule the table above where they
differ.

- **No timeout on a model call.** Local models are expected to be slow. Done
  in `../dry-learn-timeout` (`dev/timeout`, uncommitted): the 600 s cap is
  gone from the host's and the window's model calls.
- **Refocus stays at 75%, not 90%.** Focus drops sharply before the window is
  full, so that is where to refocus. The structured slots still stand.
- **No fixed-prompt budget.** No model in use is expected to have a window
  under about 121k tokens, so a child at 104% of a 4k window is not a case to
  design for. The step is dropped.
- **Answering with components is a must.** An answer engine, apart from the
  translator, with a components skill: which components exist, how each is
  used, the right call and arguments, so replies draw an A2UI-style dynamic
  UI.
- **Tools for the engine (app properties, usage) need their own design:**
  where they live and how they are written is a question of its own, before
  any is added.
- **A sandbox or environment per project comes first for file work.** Tests
  wrote at exact locations and it worked, but engines need their own
  environment so projects can run in parallel, sharing a workspace or getting
  a new one. The owner plans this next.
- **A session not knowing the translator is expected:** a session's tools run
  only when it calls them, while dry calls tools for its own loops. The simple
  fix is the dashboard. The other direction is the one wanted: an engine calls
  a session, and the session works through. A mandatory `response` tool call
  for sessions would make their replies standard.

## Numbers

| Tester | Theme | Host | Model calls | Thrown away | Verdicts (pass / partial / fail / blocked) |
|---|---|---|---|---|---|
| A | super engine, app properties | 8111 omlx | 45 | 18 | 3 / 1 / 4 / 3 |
| B | sub-engines, threads | 8112 local | 61 | 8 | 2 / 4 / 2 / 0 |
| C | alerts | 8113 omlx | not counted | 7 | 1 / 3 / 3 / 1 |
| D | compaction | 8114 local, 8k window | not counted | not counted | 2 / 6 / 3 / 0 |
| E | long-running goal | 8115 omlx | 41 | 16 | 1 / 5 / 1 / 0 |
| F | UI in a browser | 8116 local | not counted | not counted | 5 / 6 / 1 / 0 |

Every scratch host is stopped. Nothing in the repo was changed except this
folder, and nothing was committed.
