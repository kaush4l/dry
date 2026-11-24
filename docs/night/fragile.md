# Fragile parts: what to consider, and a plan

From the night of 2026-11-23: two captured runs judged as a black box by an
agent with a fresh context ([judge-smoke.md](judge-smoke.md), 23 findings),
and what the night run itself showed ([journal.md](journal.md)). Each part
below says what was seen, why it will break on a longer run, the choices,
and the one proposed. **Nothing here is built** unless its row says so:
these are decisions for the owner, in the order proposed.

Three of the judge's findings are not the app's: 7 (a reply to a message
the translator never sent), 16 (a child failed after its report) and 23
(the error names Sonnet in an Opus run). All three come from one check made
by hand after `smoke3` ended: two children were switched to `claude/sonnet`
and written to directly, to see which prompts the safeguard refuses. What
stays true from them is in part 1 (a child's reply to *your* direct message
also wakes its parent).

## 1. Messages between engines

The part with the most findings (judge 1, 2, 3, 15, 19, 20), and the one
the owner named.

| Seen | Where |
|---|---|
| A reply over 2,000 characters reached the parent cut: 400 in the wake message, 2,000 in the `children` snapshot, and the translator holds no `read`. Opus noticed and spent 4 answers on a send-back | `smoke3` translator, line 401 |
| Every message to a child forces a reply (a turn ends only by `reply`), and every reply wakes the parent. Haiku: 7 follow-ups, 9 replies for two small jobs | `smoke2` translator |
| A parent's follow-up to a finished child starts the child's run again at its first phase, so the phase text ("Your one reply is your answer…", scope) contradicts the follow-up ("restate, short") | `smoke3` research prompt, lines 16 and 24 |
| `engine_call` beside a `reply` silently becomes a start; its result is the same 400-character excerpt | `smoke2` translator, line 78 |
| A message to a child carries no sender: children call the parent "the user" | `smoke3` research, line 640 |
| A failed child sends its parent nothing. In the night run five children failed in 10 s and the head sat idle; only the `keep-going` alert, 185 s later, brought it back | journal, 23:36 to 23:40 |

Why it is fragile: the night goal is ten children whose reports hold tables
of numbers. A cut report, or a follow-up that restarts a child's phases,
costs rounds each time, and the cost grows with the number of children.

Choices:

- **A. A reply is a file.** The child's whole reply is kept as it is now,
  and the parent reads any of it: `read(name="children", of="<child>")`, or
  the snapshot shows the newest unjudged reply whole and the others by
  their first lines. No new tool on the translator if the snapshot does it.
- **B. Raise the limits.** Done for tonight (2,000 → 4,000). It moves the
  edge, and ten children at 4,000 each is 40,000 characters in every
  translator prompt.
- **C. Reports go to disk.** The child writes `report.md` in its root and
  replies with the path and five lines; the parent would need a file tool,
  which the translator is designed not to hold.

Proposed: **A**, as a rule of the snapshot: *the reply that woke you is
shown whole (up to the context's room), every other child's last reply as
its first 400 characters*. One reply is whole at a time, the one being
judged. Then:

1. A follow-up is a **wake, not a new ask**: `engine_start` to a child that
   exists keeps its phase and skills (`State::add_wake`), as your thread
   posts already do. A parent that wants the run again from the start says
   so (`restart="yes"`).
2. A failed child **calls back**: `engines::settle` wakes the parent with
   `engine <x> failed: <why>` once, as it does for a reply. (Before, only a
   parent that *waits* heard a failure.) The alert stays as the net under it.
   **Built on 2026-11-24 00:10** (the child line gets a fourth word, `told`,
   cleared when the child works again; test
   `a_started_child_that_fails_calls_back_once_and_again_after_it_worked`).
   The night run's host is the older build, so it does not have it.
3. Every delivered message names its sender (`from translator:`).
4. `engine_call` beside a `reply` is refused with the reason, instead of
   changing its meaning.

Not proposed: a "no reply needed" message. A turn that ends without a reply
exists only for a wait on a job; a second way to end a turn needs its own
design (what the Desk shows, what the parent hears).

## 2. The prompt grows without limit that a CLI model meets

| Seen | Where |
|---|---|
| The research prompt went from 56,881 to 142,352 characters in about 15 answers; 88.7% is history | judge, section 1 |
| A skill's body is in the history once per load: `judging` 4 times at 3,681 characters; skill bodies are 38.8% of the translator's last prompt | judge 4, 9 |
| An artifact written whole (`write(name="log")`) is in the history once per write and once in its snapshot: 8 rewrites, 23,652 characters | judge 4 |
| `smoke3`: 958,288 prompt tokens for 37 calls, about 26,000 a call; the CLI reported $8.33 | journal, section 4 |

Why it is fragile: compression starts at 75% of the window, and for the
claude CLI the window is large, so it never starts in a short run; every
call pays for the whole history. A night of ten children is where it shows.

Choices and proposal:

1. **A loaded skill is an element, not a message** (proposed first): its
   body is drawn once, in a `skills loaded` section, while it is loaded;
   the history keeps one line (`skill(name="judging") → loaded`). Loading it
   again costs nothing. This also removes the reason for "load it alone, in
   a step of its own" (judge 9: 7 of Opus's 40 answers).
2. **A `write` result does not echo, and the call's text is folded** once
   the artifact is written again: the history keeps `write(name="log",
   text=<1,902 characters, replaced since>)`. The snapshot is the truth.
3. **A snapshot that must be cut keeps its end** for logs and its start for
   everything else, or better: says which, per artifact (`keep: end`).
   (judge 8)
4. Tools cut to the phase: the developer's browser and evidence tools are
   28.5% of its prompt for a one-file script (judge 14). The mechanism
   exists (a skill brings tools); the developer's artifacts are attached
   whether or not the brief needs them. Proposed: `browser` and `evidence`
   come with the `browser-qa` and `test` skills, not with the template.

## 3. What a tool gives back

| Seen | Fix | State |
|---|---|---|
| `run` returns the last 15 lines with no mark that lines above were dropped; 3 reruns, and a plan step ticked on output the result did not show (judge 5) | the first line of a cut result says `… earlier lines not shown (the terminal artifact shows the last 30)` | **built 2026-11-24** (the host is asked for 16 lines; a 16th means cut) |
| A skill tool's result names the wrapper (`session 6 'python3' '-c' '#web.read…`) and the stored terminal state holds the whole script per call (judge 21) | a session keeps the call as written (`web.read(url="…")`) as its name, the script only in the job | half built tonight: the snapshot and the short name hide the script (`terminal::shown`); the stored state still holds it |
| Haiku confused artifacts with files (`write`/`read` against `edit`/`open`): 14 tool errors (judge 10) | one word per thing in the tool texts: an *artifact* is written, a *file* is edited; the error says which the name is | small, not built |

## 4. A model that refuses, or skips

| Seen | Where |
|---|---|
| `claude/sonnet` refused the translator's and the developer's prompts at the first call, every time; and five of five research children in the night run. `[reasoning_extraction]` | journal, sections 4 and 5 |
| Haiku skipped the structure: no prompting skill, the plan written last, the developer never left triage, no `lessons.md`, a summary with wrong facts (judge 6, 11, 17) | `smoke2` |
| Opus left out the `thought:` key in 13 of 21 research answers; accepted silently (judge 18) | `smoke3` |

Consider:

- The refusal is the provider's decision about our prompt. It was not
  worked around. What the app owes its owner is **to say it plainly and
  offer the next row**: a child whose model refuses is a failed child with
  the reason (part 1, point 2), and the subscriptions section could mark a
  row "refused N times today" so the head does not pick it again. Whether
  to change the response contract's wording is the owner's call, with the
  provider's policy in hand; it should not be found by trial.
- "Is a step done" cannot rest on the model's word for small models. The
  deep-research `learn` phase says it is never skipped; Haiku skipped it.
  Proposed: a phase may declare what must exist before it is left
  (`leaves: summary.md, lessons.md`), checked by `goto_phase` and by
  `reply`. This is the "done is not refused without a passing run" item
  of the loop journal, generalised: a declared check, not a step count.
- A missing `thought:` could be shown on the Desk as a count per engine,
  so drift is seen without reading transcripts.

## 5. Names and places in the prompt

Small, and each one costs a model a wrong guess (judge 12, 13, 20, 22):

- one thing, one name: *thread* (not shared, shared context), *children*
  (not engines), *file artifact* (not code);
- the phase list is headed "The plan" beside a `plan` artifact: head it
  "Phases";
- the environment line shows the app's folder; a child with a root should
  read its root there;
- the summary is written to an artifact and then replied whole: the reply
  could be the artifact (`reply(artifact="summary")`), one copy.

## 6. The alert that keeps a plan going

Built tonight (journal, section 2). What to keep in mind:

- It is the net, not the path. In both smoke runs it never fired; in the
  night run it fired because failed children are silent. With part 1,
  point 2 it would fire less, and that is right.
- The quiet is judged from the looks the host takes every 2 s. A host that
  is down has no looks; on its return the quiet continues from the saved
  mark, so a run cut off by a restart is picked up after the alert's time.
  Verified by test for the mark, **not yet by a real restart**.
- An engine ends its own wake by editing its plan (`- [?]`, `- [~]`). A
  model that never does will be woken every 3 minutes for as long as a
  line is open. That is a cost, not a step limit; a Desk count of fires
  per alert would show it.
- The browser build gets the same alert through `turn::callbacks`; **not
  run there**.

## 7. What the night run added

The night run (4 h 18 min, 23 engines, 591 calls, $406.83 reported; judged
in [judge-night1.md](judge-night1.md), with the operator's corrections at
its top) puts numbers on parts 1, 2 and 4, and shows five parts not listed
above.

| Seen | Cost | Covered? |
|---|---|---|
| 52 of the translator's 103 calls only loaded a skill ("load it alone") | $48.39, 52% of its spend; skill bodies 39% of its last prompt | part 2.1; its fix would have removed most of it |
| Developer prompts grew to 254k tokens, 96% history (their own data edits, blind-batch reads); no developer was ever compressed | $225 over five developers | part 2.2 folds artifact writes only; a developer's `edit` of a data file needs the same fold |
| Every score was given by the developer that wrote the data, the prompts and the rubric; reviewers re-added its numbers | every result tonight rests on it | **not covered** |
| Opus stopped or withheld four developer answers (Holmes twice, Gandalf, Stark); nothing in a transcript or span says so; Gandalf lost its planned variable, Stark nearly stopped to ask the absent owner | one variable lost, a near stop | part 4 covers Sonnet's whole-call refusal only |
| 10 replies over 4,000 characters; the wake line said "all of it is in the children artifact" | 8 of 23 follow-ups were mechanics | part 1A; **the wording is fixed** (2026-11-24: past 4,000 it says "the first 4000 are in the … artifact") |
| Failed children: 2 alert rescues (185 s, 175 s), 2 caught by luck | 360 s idle, 2 near misses | part 1.2, built after the run |
| 3 answers written as raw `<invoke name="edit">` markup instead of `act:` | 3 steps | **not covered** |
| A shell `cp` wrote outside a developer's root; 12 eval dumps in the claude CLI's own scratchpad folders | files outside every root | **not covered**: the root guard is on `edit`/`save` only |
| The Holmes brief retold "Sparrow r1 settings" without `mask_prompt`; the Stark brief passed on "val loss did not track voice in any build" though Gandalf's `runs.md:43` says it did | Holmes r2 spent its one variable; the summary repeats both as fact | **not covered** |
| 131 host lines "the store changed on disk during a turn" | none seen; unexplained | **not looked into** |

Proposed, for the five not covered:

1. **A scorer that never saw the data.** The persona skill's eval step
   starts a child (a reviewer, or a template of its own) that gets only
   the card, the rubric and the shuffled answers, and returns the scores.
   The developer keeps training; it no longer grades itself.
2. **A stopped answer is an event.** When the CLI's answer is cut or
   withheld (a stop reason, or a known message), the step records it in the
   span (`stopped: <why>`) and the next prompt says it in one line. The
   Desk can count it like a refusal (part 4).
3. **Lessons are cited, not retold.** A brief names the line a lesson
   comes from (`jack-sparrow/runs.md:25`) and the child reads it; the
   prompting skill says so. A head that retells loses settings.
4. **Off-format answers**: read a model's own `<invoke name="x">` markup as
   the act, as native `tool_calls` already are (dev/loop, gap 16).
5. **Commands that write outside the root**: not by guessing at shell text.
   A run's working folder is the root already; the gap is the brief that
   asks for a path outside it. The prompting skill's brief rules can say
   that every path a child writes is under its root.

## Order proposed

1. Part 1, points 1 and 3 (reply shown whole when it wakes; a follow-up is a
   wake; point 2, a failed child calls back, is built): they remove rounds
   from every run.
2. Part 2, point 1 (a loaded skill is an element): the largest saving per
   prompt, and it deletes a rule from every template.
3. ~~Part 3, the `run` mark~~: built.
4. Part 4, `leaves:` on a phase.
5. Part 7.1 (a scorer that never saw the data): without it, no number
   from a persona build can be trusted.
6. Part 7.2 (a stopped answer is an event).
7. The rest, as the owner ranks them.
