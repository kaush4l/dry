# Morning brief, 2026-11-24

> The run captures this cites (`docs/night/runs/`) are not in the tree; they are in commit `992cc0f`: `git show 992cc0f:docs/night/runs/<path>`.

Two worktrees ran in parallel on the evening of 2026-11-23: `dev/loop`
(the hosted page on the on-device model) and `dev/night` (the desk driving
itself overnight on the claude CLI). Both are merged and squashed into one
commit on `main`, `992cc0f`, pushed. The whole suite on it: **547 passed,
0 failed, 13 ignored**.

Details: [docs/loop/journal.md](../loop/journal.md),
[journal.md](journal.md), [judge-night1.md](judge-night1.md),
[fragile.md](fragile.md).

## 1. dev/loop: the hosted page on the local model

**Tested**: one task (a Python script printing a centred tree of height
n), one model (Inkling-2-Mini-oQ4e-mtp on oMLX, then LM Studio), the
shipped default, every run on a scratch store. Eight runs, plus a small
HTML widget built in the page.

**Learned from testing**

- The model could always do the task; the cost was in the loop around it.
  The baseline (`f129e64`, unchanged) passed in 550 s and 25 answers, with
  4 off the response contract and the history summarised 7 times.
- 13 gaps were found by reading the code before the first run, and five
  more came from the runs. The largest: no tool could make a file; native
  `tool_calls` from the server were never read; the window was assumed to
  be 8,192 tokens; every wake restarted the developer at triage.
- **Where the contract sits mattered most.** With it on the first message,
  the translator answered in prose 19 times after loading a skill; with it
  after the newest message, 0 times.
- A skill that brings tools did not make this task easier: one more step to
  load, more commands run. The shortest run had no skill.
- In the browser build the developer cannot look at a page it made (no
  browser there); it checked with `grep`. Still open.

**Changed**: `edit(path, new)` makes a file; skills can bring tools
(`skills/<name>/tools.md`, called `<skill>.<tool>`); native tool calls and
reasoning are read; the contract follows the newest message; the window is
read from the server; the developer template writes, saves and runs in one
act; alerts fire (`alert::settle`, marks in `convo/run.md`); the live view
follows the last file touched and previews HTML; a bolt theme.

**Result**: answers fell from 25 to 12–15, slips from 4 to 0–1, no
history summarised. 7 of 7 runs that reached the developer passed.
`alert1`: the engine re-checked its work with nobody writing, by its own
alert.

## 2. dev/night: the desk drives itself overnight

**Built**

| What | Where |
|---|---|
| `idle 5m` alert trigger: fires once per quiet stretch of the whole thread; `while: <artifact>` holds an alert to an open `- [ ]` line | `core/src/alert.rs`, `turn.rs` |
| `keep-going` alert shipped with the translator (idle 3m, while plan) | `default/engines/translator/spec/alerts/` |
| deep-research template: scope → search → notes → learn → answer; `web.search`, `web.wiki`, `web.read` as skill tools | `…/templates/deep-research/` |
| persona skill for the developer (card, original data, mlx-lm LoRA, blind eval against the base, `runs.md`, one variable per run) | `…/templates/developer/skills/persona/` |
| summarizing skill: Asked / Direction / Work / Learned / Not done or not verified / Files; facts only | `…/translator/spec/skills/summarizing/` |
| a failed child calls its parent back once; a cut `run` result says so; a cut reply says how much the snapshot holds | `core/src/engines.rs`, `terminal.rs` |
| child roots made for them; long commands shown short; claude/sonnet and claude/opus rows | `live.rs`, `terminal.rs`, `models.json` |
| test: two loops side by side, the head going on while its plan is open | `crates/tools/tests/one_loop.rs` |

**The night run**: one message at 23:35; the desk closed its plan at 03:53
and removed its own alert. **4 h 18 min, 23 engines, 591 model calls, 49 M
prompt tokens, $406.83 as the CLI reports it** (a subscription: the weekly
limit went from 77% to about 78% in the first hour; not rechecked since).
**No operator step after the message.**

| Character | Untuned | Best tuned | Facts wrong | Build |
|---|---|---|---|---|
| Jack Sparrow | 1.42 | 2.96 (r1, 400 iters, 294 s) | 2 | 50 min |
| Sherlock Holmes | 1.00 | 2.00 (r3, 86 s) | 4 | 37 min |
| Yoda | 1.08 | 3.08 (r3 checkpoint 300, 115 s) | 2 | 36 min |
| Gandalf | 1.29 | 2.46 (r1 checkpoint 150, 63 s) | 4 | 37 min |
| Tony Stark | 1.16 | 2.80 (r3, rank 16, 76 s) | 6 | 41 min |

In-voice is 0–5, scored blind by each developer; the bar was 4. **No
character met it**, and every report says so. Base for all:
Qwen2.5-1.5B-Instruct-4bit, about 1.2 GB peak memory per training run.
Sparrow was scored with different eval settings from the other four, so
its row is not directly comparable.

**How the desk kept going**: 5 research children refused on Sonnet → the
alert woke the desk after 185 s → it restarted them on Opus. A reviewer
refused on Sonnet → second alert fire, 175 s. A 600 s CLI timeout and a
refused developer were caught at the next wake. After the seventh refusal
the desk wrote in its plan that Sonnet had failed every child (it counted
8) and moved all later work to Opus. Lessons went from each character's `runs.md` into
the next brief: choose by blind checkpoint score, not val loss; half the
data everyday questions; a memorisation probe; a new seed per run; resume
instead of retraining.

**The desk's summary** (`runs/night1/engines/translator/summary.md`, also
sent as its last reply): a fresh-context judge checked 24 numeric claims,
**19 match and 5 do not** (Sonnet failed 7 children, not 8; Holmes r1 lacked
`mask_prompt`; a time off by 0.02 s; two general "learned" claims that some
builds contradict). It leaves out the cost and the Opus stops.

**Learned from testing**

- **The desk runs a long plan by itself.** The idle alert is the net, and
  it was needed: a failed child told nobody (now fixed).
- **Sonnet's safeguard refuses the default's prompts** (`[reasoning_extraction]`)
  for the translator, developer, research and reviewer: 7 of 7 children
  given to it tonight. This was not worked around. Opus also stopped or
  withheld 4 developer answers, and nothing records it.
- **The prompt grows without limit on the CLI.** The translator went from
  7k to 222k tokens. 52 of its 103 calls only loaded a skill: $48 of its
  $94. Developers reached 254k tokens and were never compressed: $225 of
  the night.
- **Every score is self-scored.** Each developer graded the answers to
  prompts it wrote, from data it wrote; reviewers re-added the numbers. No
  result tonight is independent.
- **The head loses detail when it retells a lesson**: `mask_prompt`
  dropped from the Holmes brief, a contradicted claim passed on to Stark.
- **The 1.5B base invents facts in every build.** Four developers named
  the base as the ceiling; the desk kept it, reading "may be fetched once
  and shared" as a rule.

## 3. The fragile parts (decisions for the owner)

[fragile.md](fragile.md) has the evidence and choices for each. Built
tonight: failed-child callback, the `run` cut mark, the cut-reply wording.
Proposed order for the rest:

1. A reply shown whole when it wakes its parent; a follow-up is a wake, not
   a new ask; every message names its sender.
2. A loaded skill is an element, not a message (the largest saving; it
   deletes the "load it alone" rule).
3. `leaves:` on a phase (what must exist before it is left).
4. A scorer that never saw the data, for persona builds.
5. A stopped or withheld answer is an event, recorded and shown.
6. Then: lessons cited by line rather than retold; a model's own `<invoke>`
   markup read as the act; a fold for developers' data edits.

## 4. Not done or not verified

- No character reached the bar; no independent scorer exists.
- The browser build was not run with the idle alert; a real host restart
  mid-plan was tested by unit test only.
- 131 host log lines "the store changed on disk during a turn": not looked
  into.
- `dev-stark`'s CLI calls left 12 eval files in the claude CLI's own
  scratchpad folders (paths in the desk's summary); left in place.
- Left for you: branches `dev/loop` and `dev/night` and their worktrees
  (`../dry-learn-loop`, `../dry-learn-night`); the night run's adapters and
  data (about 2.7 GB) in `/Volumes/Backup/dry-build/night-run/night1/proj`;
  the default's spec changes do not reach a live `state/default/spec` until
  merged there.
- I committed to `main` and pushed because you asked for the merge and the
  push; the standing rule for this repo is that you test and commit.
