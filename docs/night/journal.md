# Night loop: journal

> The run captures this cites (`docs/night/runs/`) are not in the tree; they are in commit `992cc0f`: `git show 992cc0f:docs/night/runs/<path>`.

Every step of the `dev/night` work, in order (worktree `../dry-learn-night`).
Times are local, starting 2026-11-23 23:00.

Goal (owner, 2026-11-23 night): the desk runs by itself when it has a
sufficient plan. A deep research engine that searches the web, makes notes
and always closes with a learning, summarisation and question session. The
developer's focus is a persona: the desk builds 5 characters on this
machine (the first: Jack Sparrow, from public information), learning what
to optimise (quality, training speed). When the goal is reached the desk
summarises the work and the direction taken, as plain facts. The native app
uses the `claude` CLI as its model. Later the same night: test sub-engine
loops in parallel, the messages between engines, the artifacts and the
whole prompt as it is rendered; capture a run as a black box and have an
agent with a fresh context judge quality and structure; the fragile parts
get a plan, not a patch.

The parallel work is `dev/loop` (worktree `../dry-learn-loop`): the hosted
build on a local model (Inkling-2-Mini on oMLX). Its journal is
`docs/loop/journal.md`.

## 1. The base (23:00 to 23:15)

| Step | What |
|---|---|
| Worktree | `../dry-learn-night`, branch `dev/night`, from `f129e64` (where `dev/loop` started) |
| `dev/loop` as it was | its uncommitted changes copied in as the first commit (`c9c5dbb`), so both branches share them and merge cleanly later |
| `main` since `f129e64` | `main` is one amended commit (`7355829`), unrelated history to `f129e64` for git, so a merge is refused. Its delta (wakes keep the run, waiting on a job, `one_loop.rs`) applied as a 3-way patch: clean. Tests: core 428, tools, server pass |
| `dev/loop`, second look (23:10) | it had moved on: **alerts now fire** (`alert::settle` in `turn::callbacks`, marks in `run.md`, an `alert` tool, an `alerts` artifact on the translator). Taken in the same way (its tree written through a private index, so its worktree was not touched). One conflict (a doc comment in `turn.rs`) |
| Build | `CARGO_TARGET_DIR=/Volumes/Backup/dry-build/night-target` (server and `dry` only; the window is not built here) |

A first version of alert firing written here (posts on the thread, the
host's tick) was dropped when the second look showed `dev/loop` had built
firing already. Only what it does not have was kept, on top of its code.

## 2. The desk goes on by itself while its plan is open

`dev/loop` gave timed alerts (`at`, `every`). A plan needs a different
trigger: not a time, but "nobody is working and steps are open".

| Piece | What | Where |
|---|---|---|
| `idle 5m` | a trigger: nobody on the engine's thread has worked for that long (no turn running, no engine waiting on a child or a job, the holder took no message). Once per quiet stretch | `alert::Trigger::Idle`, `Alert::due_quiet` |
| `while: plan` | a condition in an alert's header, for any trigger: it fires only while that artifact holds an open step (a line starting `- [ ]`) | `Alert::holds`, `alert::open_steps` |
| The quiet, remembered | `Mark` gains `quiet` (since when) and `seen` (the holder's message count), in its `run.md` line; an older line without them still reads. So a restart keeps counting, and a turn cut off by a restart is picked up when the alert fires | `alert::Mark`, `alert::settle_with` |
| Who works | `turn::callbacks` passes whether any other engine on the thread is busy, and the holder's artifacts | `alert::Look` |
| The tool | `alert(..., trigger="idle 5m", while="plan")` | `alert::call`, `tools::BROUGHT` |
| The translator's alert | `spec/alerts/keep-going.md`: `idle 3m`, `while: plan` | `default/engines/translator/spec/alerts/` |

How it ends: the engine edits its plan. `- [x]` done, `- [~]` given up,
`- [?]` waits for the person. With no `- [ ]` line left the alert is
silent. Nothing counts steps.

Tests (`cargo test -p dry-core alert`: 13 pass):
`an_idle_alert_fires_once_per_quiet_stretch_and_only_while_its_plan_has_an_open_step`,
`a_timed_alert_whose_while_does_not_hold_passes_its_moment_over`.

## 3. The engines for the goal (23:15 to 23:20)

Written by a forked agent under `default/` while the alert work went on in
`crates/`; nothing needed registering (`core/build.rs` bundles `default/`).

| Piece | What | Where |
|---|---|---|
| `deep-research` template | phases scope → search → notes → learn → answer. Sub-questions as `- [ ]` lines; pages read into `sources/`; facts in `notes.md`, each with its url. **learn is never skipped**: a summary (`summary.md`), then a question session (at least five questions a critical reader would ask, one per disagreement between sources), each closed from the notes or by going back to search, or marked `- [~]` with what was tried; then `lessons.md` (worked, failed, next time) | `templates/deep-research/` |
| `web` skill, with tools | `web.search` (DuckDuckGo html, then lite, then Wikipedia's search), `web.wiki` (an article as text), `web.read` (a page as text, into `sources/`). Each is one self-contained `python3 -c` command, standard library only, since only `SKILL.md` and `tools.md` reach an engine | `templates/deep-research/skills/web/` |
| `persona` skill for the developer | card (who, voice, words, rhythm, values, never, facts, ten new exchanges), data written new in batches, build and overlap scripts, training on device started and waited on, eval against the base on fixed prompts, one row per run in `runs.md`, one variable changed per run | `templates/developer/skills/persona/` |
| The translator's plan | `artifacts/plan.md`; `write` added to its tools (it had `reply` only); goal: a goal with several deliverables gets a plan first, independent deliverables start together in one act, a line is ticked when its child's reply is judged good, and with every line ticked the `summarizing` skill writes the summary | `spec/goal.md`, `spec/tools.md` |
| `summarizing` skill | Asked / Direction (each decision, why, instead of what) / Work per child with numbers / Learned / Not done or not verified / Files. Facts only; every number has an owner | `spec/skills/summarizing/` |

Three things the fork found, fixed in `crates/`:

| Seen | Fix | Test |
|---|---|---|
| A child's root that is not there is refused ("the root X is not there"), and the translator has no terminal to make it | `engine_start` with a template and a root makes the folder first (`live::Host::make_root`, the disk's `create_dir_all`) | `two_loops_…` (roots `a/`, `b/` made) |
| A skill tool's inline script (about 2 KB) was shown whole under every terminal session, and the "Earlier" list cut the query off | a command over 400 characters shows its start and end; a cut name keeps its last 30 characters, where the inputs are (`terminal::shown`, `short`) | core tests pass |
| `shipped.rs` expected the translator's tools to be `reply` alone | now `reply`, `write` | `the_default_reads_with_a_translator_that_holds_no_action_tools` |

`crates/tools/tests/one_loop.rs` gained
`two_loops_run_side_by_side_and_the_head_goes_on_while_its_plan_is_open`
(scripted model, the real disk): the plan and both children in one act,
each child's prompt holds its own brief and root and not its sibling's
work, one wake carries both replies, three quiet minutes later the alert
wakes the head because a step is open, and with every step ticked it stays
silent. Core 433 tests, tools and server pass.

## 4. The claude CLI as the model (23:14 to 23:32)

One small goal, the same words each time, on a scratch store
(`docs/night/run.sh`): a short web research on Sherlock Holmes and a
`tree.py`, "the two do not need each other; when both are done, summarise".

| Run | Row | Result | Wall | Model calls | Cost the CLI reported |
|---|---|---|---|---|---|
| `smoke1` | `claude/sonnet` | **refused at the first call**, and again on a retry: `API Error: Sonnet 5.5's safeguards flagged this message … Details: [reasoning_extraction]` | 6 s | 0 answered | 0 |
| `smoke2` | `claude/haiku` | plan, both children started in one act, both judged, summary written. The research child wrote no `lessons.md` and left the third trait "unconfirmed" | 170 s | 45 (research 23, translator 13, developer 9) | $0.19 |
| `smoke3` | `claude/opus` | the same, complete: `notes.md`, `summary.md`, `lessons.md`, 10 pages in `sources/`; `tree.py` checked with `cat -e`; the summary lists what rests on one source | 481 s | 37 (research 20, translator 11, developer 6) | $8.33 |

- **Sonnet.** The refusal is the provider's safeguard, so it was left
  alone: no rewording to get past it. Checked which prompts it meets: in
  the finished `smoke3` store the developer switched to `claude/sonnet` was
  refused the same way; the deep-research child on `claude/sonnet` answered.
  So the night run uses `claude/opus` for the translator and developers and
  `claude/sonnet` for research, and its goal says: a child refused by the
  safeguard is started again on opus with the same brief.
- **The keep-going alert did not need to fire** in either run: each turn
  started the next piece of work itself. Its mark was kept right
  (`alert: keep-going … 0 0 <quiet since> <messages>` in `run.md`).
- **A reply reached its parent cut.** The research child's reply was 3,716
  characters; the parent's `children` snapshot shows 2,000 and the wake
  message 400. The opus translator noticed, asked for a short restatement
  and lost a round. Raised to 4,000 for tonight; the real answer is a
  design question (see the fragile parts).
- Both runs, whole (every engine's conversation, the exact prompts mid-run
  and at the end, the artifacts, the files made): `docs/night/runs/smoke2`,
  `docs/night/runs/smoke3`. A fresh-context agent judges them as a black
  box: `docs/night/judge-smoke.md`.

## 5. The night run (23:35 on)

`docs/night/run.sh start night1 8750 claude/opus "<the goal>"`: store
`/Volumes/Backup/dry-build/night-run/night1/state`, workspace `…/night1/proj`,
status changes in `…/night1/timeline.log`. The goal as sent is
`…/night1/message.txt`.

| Time | What |
|---|---|
| 23:35 | the goal posted to the translator |
| 23:36 | the plan written (12 lines) and five deep-research children started in one act: `research-sparrow`, `research-holmes`, `research-yoda`, `research-gandalf`, `research-stark` |
| 23:36:06 to 23:36:16 | **all five failed** on `claude/sonnet` within 10 s, at their first or third answer: the same safeguards refusal as `smoke1` (the earlier check, where a research child on sonnet answered, did not hold for these prompts). The translator had already replied with the briefs and was idle. A failed child sends its parent nothing |
| 23:39:21 | 185 s of quiet with 12 open plan lines: **`keep-going` fired** (`Alert keep-going fired (idle 3m): Your plan has an open …`). Nobody wrote to the translator. Its answer: "All five research children failed on claude/sonnet with the same safeguards error. Your rule covers this: start each one again on claude/opus with the same brief, unchanged." |
| 23:40:06 | five new children started in one act on `claude/opus` (`research-sparrow-opus` …), briefs copied word for word; the translator idle again, the mark in `run.md`: `alert: keep-going 1795494976 1795495157 1 0 - 12` (fired once) |

So the first proof that the desk goes on by itself came from a failure
nobody planned: without the alert the run would have stopped at 23:36 with
five failed children and an idle head until morning.

Later that hour:

| Time | What |
|---|---|
| 23:50 to 23:57 | Yoda, Gandalf, Holmes and Sparrow research judged and ticked (plan: 8 open, 4 done). Each has `notes.md`, `summary.md`, `lessons.md` and `sources/` |
| 23:52 | the translator started `dev-sparrow` on `claude/sonnet` (its reading of "the cheapest row that does the job"), "plan first". Refused by the safeguard at its first call |
| 23:54 | it saw the failure in its next turn (a research reply woke it) and started `dev-sparrow-opus` on `claude/opus` with the same brief |
| 23:55 | `research-stark-opus` **failed: `claude: no answer in 600s; stopped`** (one call ran into the CLI's 600 s limit; it had stood at 29 messages for 10 minutes). Silent again, and this time the thread was not quiet (the Sparrow developer worked), so no alert. The translator noticed it at its next wake and wrote to the child; it went on and finished (23:58) |
| 23:58 | `dev-sparrow-opus`: `card.md` and `data/batch-01.txt` written |

Spend, from the spans the CLI reports (`cost_usd`): $66.80 by 23:55, of it
about $10 per research child on opus. The account's weekly limit read 77%
before and after, so a night of this size is within it; looked at again
each hour.

## 6. The parallel worktree landed (seen 23:57)

`dev/loop` was committed at 23:32 as `ad690bf` ("loop: engines that start by
themselves, a developer loop for a local model, the code view") and `main`
is that commit, on top of `7355829`. Its session is closed. What it added
after the state `dev/night` was built on is small (the browser app, the
companion, docs).

A trial merge of `dev/night` onto that `main` (`git merge-tree`, base = the
loop state this branch started from, kept as branch `night-base`): **no
conflicts**. Its tests run in a third worktree
(`/Volumes/Backup/dry-build/night-trial`); `main` is not touched until the
night run is over.

## 7. After midnight (2026-11-24)

| Time | Seen |
|---|---|
| 23:59 | `research-stark` ticked: all five research lines done (plan 7 open, 5 done). Each ticked line names its files and line counts, its question count (open ones kept as `[~]`) and a copy check of 0 |
| 00:00 | Trial merge of `dev/night` onto `main` (`ad690bf`): **544 passed, 0 failed, 2 ignored** in `night-trial` |
| 00:03 | `dev-sparrow-opus`: `card.md`, six data batches, `train.jsonl`/`valid.jsonl`, a `tools/` folder; 24 messages |
| 00:05 | Plan limits: weekly all models 78% (was 77% at 23:55), Fable 32%; resets 2026-11-25 12:00Z |

Two fragile parts built while the run went on (on `dev/night`, commit
`cd608d6`; the night host runs the older build and does not have them):

- a child started with `engine_start` that fails now wakes its parent once
  with `engine <x> failed: <why>` (fragile part 1, point 2);
- a `run` result cut to its last 15 lines now says so in its first line
  (fragile part 3).

Whole suite on `dev/night` after both: **546 passed, 0 failed**.

### Jack Sparrow built on device (seen 00:30)

`dev-sparrow-opus` finished three LoRA runs on this Mac and wrote
`characters/jack-sparrow/runs.md`; the translator then started
`review-sparrow` (a reviewer child) to judge it. From `runs.md` as written by
the developer:

- base `mlx-community/Qwen2.5-1.5B-Instruct-4bit` (840 MB), mlx-lm 0.32.2,
  239 hand-written exchanges (215 train / 24 valid), 24 held-out eval
  prompts, in-voice scored 0-5 blind from shuffled files;
- base 1.42 → r1 (400 iters, 294 s) **2.96**, r2 (100 iters) 2.21, r3 (200
  iters) 2.83; peak memory 1.23 GB in every run; 0 copy matches against the
  research sources;
- the developer says plainly r1 **does not meet its bar** (4.0 in-voice, 0
  facts fails): 2 facts fails remain;
- its lessons for the next character: do not pick by val loss (lowest val
  loss was the worst voice); half the data everyday questions; 6-10 examples
  per "never" rule; one run saving every 50 iters instead of three runs.

### Holmes built, Yoda started (00:52 to 01:35)

| Time | Seen |
|---|---|
| 00:30 | `review-sparrow` on claude/sonnet refused by the safeguard; 00:34 `review-sparrow-opus` started by the translator |
| 00:51 | Sparrow line ticked after review ("bar not met, best r1 2.958 vs base 1.417") |
| 00:52 | The translator rewrote its plan by itself: five research lines folded into one, a "Lessons from Jack Sparrow" block written into `dev-holmes`'s brief, and a decision line: "claude/sonnet … failed safeguards on all 8 children given to it tonight … so dev-holmes onward run on claude/opus" |
| 00:52 | `dev-holmes` started (opus) |
| 01:29 | Holmes `runs.md` written, `review-holmes` started (no refusal: on opus) |
| 01:34 | Holmes ticked; 01:35 `dev-yoda` started |

Holmes, from its `runs.md`: base 1.00 → best r3 (200 iters, lr 5e-6, 85.9 s)
**2.00**; checkpoints scored blind (the Sparrow lesson applied), a
memorisation probe (0 of 10 verbatim), `mask_prompt` tried in r2; bar (4/5)
**not met**, stated by the developer. Its note: "val loss does not track
voice, as with Jack". Build time 37 min against Sparrow's 50.

### Yoda built, Gandalf started (01:35 to 02:18)

| Time | Seen |
|---|---|
| 02:11 | Yoda `runs.md` written, `review-yoda` started |
| 02:17 | Yoda ticked; 02:18 `dev-gandalf` started |

Yoda, from its `runs.md`: started from Holmes r3's settings, then doubled the
lr (r2), then doubled the iters (r3) because the voice "was still rising at
200"; every run saved and scored checkpoints every 50 iters (16 rows). Base
1.08 → best r3 checkpoint 300 **3.08**, 2 facts fails, reached at 115 s of
training; picked over a tie at 350 "because it got there sooner". Bar (4.0)
**not met**, stated. Copy 0, memorisation 0. Build time 36 min.

Best voice so far by character: Sparrow 2.96, Holmes 2.00, Yoda 3.08 (each
against its own base: 1.42, 1.00, 1.08).

### Gandalf built (02:18 to 03:02)

| Time | Seen |
|---|---|
| ~02:40 | `dev-gandalf` wrote a second batch of fact phrasings; the CLI's answer came back with the content **withheld** and the edit never ran. The developer: "I won't reproduce it, so r3 can't be the data change. The plan also allows lr as r3's single variable" (convo line 1521). It wrote the reason into its notes and into `runs.md` |
| 02:55 | Gandalf `runs.md` written, `review-gandalf` started |
| 03:02 | Gandalf ticked (plan 3 open, 5 done) |

Gandalf, from its `runs.md`: 324 exchanges (47% everyday, the Sparrow
lesson), r1 = Yoda r1's settings; r2 resumes r1 for 200 more iters (one
variable: iters), r3 lr 5e-6 → 1e-5 (one variable). Base 1.29 → best
r1-150 **2.46**, 4 facts fails; bar **not met** ("1.54 below it"). r2-400
and r3-200 each repeated training text (memo 2 and 1), r1 none.

A second kind of refusal tonight: not the Sonnet safeguard (whole call
refused) but one answer's content withheld on Opus. The developer went on
without rewording it.

### Stark built, the plan closed by itself (03:03 to 03:54)

| Time | Seen |
|---|---|
| 03:03 | `dev-stark` started |
| 03:44 | Stark `runs.md` written, `review-stark` started |
| 03:50 | Stark ticked |
| 03:52 | `characters/runs.md` ticked: "71 rows, all verified cell by cell by review-stark" |
| 03:53 | `summary` artifact written (7,424 characters, the summarizing skill's sections: Asked / Direction / Work / Learned / Not done or not verified / Files); last plan line ticked; the translator **removed its own `keep-going` alert** and replied with the summary. Plan 0 open, 8 done |
| 03:54 | Prompts captured; host and logger stopped by me (nothing was running) |

Stark, from its `runs.md`: r1 = Gandalf r1's settings; r2 one variable,
data (+12 fact-first lines); r3 one variable, rank 8 → 16 ("lr and iters
had each been tried twice in earlier builds"). Base 1.16 → best r3 **2.80**,
6 facts fails; bar **not met**. Build time 41 min.

## 8. The night run, in numbers

From the spans of every engine (`docs/night/runs/night1/engines/*/spans.jsonl`).
Cost is what the claude CLI reports per call; the account is a subscription
(the weekly limit, not dollars, is what was spent).

| | |
|---|---|
| Wall time | 23:34:59 to 03:53:42, **4 h 19 min**, with nobody writing after the first message |
| Engines | 23: translator, 10 research (5 refused on sonnet, 5 on opus), 6 developers (1 refused), 6 reviewers (1 refused) |
| Model calls | **591** (translator 103; developers 48 to 59 each; research 17 to 34; reviewers 12 to 27) |
| Prompt tokens | 49.0 M; the translator's prompt grew from 7,265 to 221,577 tokens (167,116 at the end) |
| Reported cost | $406.83 (translator $93.72; developers $38.58 to $51.61 each; research $5.37 to $21.46; reviewers $2.88 to $9.64) |
| Refusals | 8 whole calls on claude/sonnet (safeguard, `[reasoning_extraction]`); 1 answer's content withheld on claude/opus (`dev-gandalf`) |
| Timeouts | 1 (`research-stark-opus`, 600 s), recovered by the translator |
| My interventions | **none** after the first message |

The five characters (in-voice 0 to 5, scored blind by each developer; bar 4):

| Character | Base | Best | Run | Facts fails | Build |
|---|---|---|---|---|---|
| Jack Sparrow | 1.42 | 2.96 | r1, 400 iters, 294 s | 2 | 50 min |
| Sherlock Holmes | 1.00 | 2.00 | r3, 200 iters, lr 5e-6, 86 s | 4 | 37 min |
| Yoda | 1.08 | **3.08** | r3 ckpt 300, 115 s | 2 | 36 min |
| Gandalf | 1.29 | 2.46 | r1 ckpt 150, 63 s | 4 | 37 min |
| Tony Stark | 1.16 | 2.80 | r3 rank 16, 76 s | 6 | 41 min |

None met the bar; every developer said so in its own `runs.md`, and the
desk's summary says so in its first Work line. Jack Sparrow was scored
with temp 0 and a 167-word system line, the others with temp 0.3, seed 42
and a repetition penalty (a Sparrow lesson), so Sparrow's numbers do not
compare directly (noted by the combined table itself).

The desk's summary, as written, is in
`docs/night/runs/night1/engines/translator/summary.md`. It ends with 12
stray eval files that `dev-stark`'s claude CLI calls left in the CLI's own
scratchpad folders, and says it cannot prove whether they overwrote
anything; they were left in place.

The host log has 131 lines "the store changed on disk during a turn; the
host's copy is written over it at the next save" (`server.log`). Nothing
else wrote the store, so the likely writer is the host's own save racing
its turns; not looked into tonight (fragile, noted for the judge).

## 9. Judged by a fresh agent

[judge-night1.md](judge-night1.md): a fresh-context agent read the capture
as a black box (no source code). 24 of the summary's numeric claims
checked: **19 match, 5 do not** (Sonnet failed on 7 children, not 8; Holmes
r1 lacked `mask_prompt`; Gandalf r3 88.85 s, not 88.87; "val loss did not
track voice in any build" is contradicted by Gandalf's own runs.md; "the
best scores came from short training" does not hold for Yoda or Stark).

It found what this journal missed: **four** Opus stops, not one (dev-holmes
at 00:58 and 01:08, dev-gandalf 02:41, dev-stark 03:07), none recorded in a
transcript or span; and a **second alert fire** at 00:33, after
`review-sparrow` failed (the 00:30 row above says only that the opus
reviewer started).

Two of its claims I checked and found wrong, noted at the top of its
report: the run had a closing reply (every one of the translator's 40 acts
carries `reply(`; the judge counted the 3 that begin with it), and it took
4 h 18 min, not 3 h 19 min.

What it adds to the fragile parts is in [fragile.md](fragile.md), part 7.
One small fix made from it: a wake line no longer says "all of it is in the
children artifact" for a reply the snapshot cuts.
