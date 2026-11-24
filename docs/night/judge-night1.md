# Judgment: night1 run (black box)

> **Operator's corrections, checked after the report (2026-11-24 04:20).**
> The judge's text below is left as written. Two of its claims do not hold:
>
> 1. **There was a closing reply.** The last act (T:6866) is
>    `write(name="summary", …); write(name="plan", …); alert(name="keep-going", text=""); reply(text="Asked: overnight and unattended, …", show="table", data="character|base in-voice|best run|…")`.
>    A `reply` has no line under `### tools` in any turn (the same holds for
>    the reply at T:6667), so the tools section cannot show it. Every one of
>    the translator's 40 acts carries a `reply(` (`grep -c "^act:.*reply("`
>    gives 40); the judge counted only the 3 acts that *begin* with it. So
>    the rows "No final reply" (§1 row 7, §5, §8 item 7, the table) and
>    "3 in 40 turns" (§2) are wrong. What stays true: the summary's text is
>    written twice (artifact and reply), as fragile.md part 5 notes.
> 2. **The run took 4 h 18 min**, not 3 h 19 min (23:35:06 to 03:53:46).
>
> Accepted as found, and taken into the journal and fragile.md: four Opus
> stops rather than one, the second alert fire at 00:33, the `mask_prompt`
> lost in the Holmes brief, the 5 mismatched summary claims, and the cost
> of skill loading.

Paths are under `docs/night/runs/night1/`. Short forms: `T` = engines/translator/messages.md, `M(x)` = engines/x/messages.md, `S(x)` = engines/x/spans.jsonl (line = one span), `P(x)` = prompts-end/x.txt, `TL` = timeline.log, `C/x` = characters/x. Costs and tokens are the CLI's own `cost_usd` and `prompt_tokens` from the spans.

Totals: 23 engines, 22 children (children.md:2-23), 3 h 19 min (TL:1 23:35:06 to TL:594 03:53:46), **$406.83** and 49.1 M prompt tokens over all llm spans. The translator alone: 103 calls, $93.72.

## 1. Did the desk run its loop by itself?

Yes, to the last plan line. Every stall and failure, and what brought it back:

| # | Time | What | Brought back by |
|---|---|---|---|
| 1 | 23:36:03-16 | 5 research children on sonnet fail (S(research-yoda):6 and the four siblings); the translator is idle (TL:6) | **keep-going alert**, 185 s later (TL:7, T:236) |
| 2 | 23:52:49 | dev-sparrow on sonnet fails (S(dev-sparrow):1, TL:99) | translator's own reading of the children snapshot, during a turn woken by an unrelated research reply (T:1456) |
| 3 | 23:45:02-23:55:03 | one research-stark-opus call runs 600 s and is stopped (S(research-stark-opus):83, TL:109) | same: seen in the next unrelated wake (T:1791), told to go on (T:1878), done 23:58 (TL:122) |
| 4 | 00:30:30 | review-sparrow on sonnet fails (S(review-sparrow):12); translator idle since 00:30:18 | **keep-going alert**, 175 s later (TL:186, T:2471) |
| 5 | 00:58, 01:08, 02:41, 03:07 | Opus output stopped or withheld inside four developer steps (section 7) | each developer itself |
| 6 | three steps | Opus wrote raw `<invoke name="edit">` markup instead of an `act:` line; no call ran (M(dev-gandalf):2069, :2248; M(dev-stark):2397) | the developer's next step (M(dev-stark):2533 "My last message used the wrong format") |
| 7 | 03:53 | translator's last turn ends with no reply (T:6863-6872; S(translator):416 outcome `ready`) | **nothing**: the same act removed the alert (T:6872) |

Two of four child failures came back only by the alert; the other two were caught because an unrelated reply happened to wake the translator within 12 s. The journal's 00:30 row (journal.md:211) does not mention the second alert fire.

Operator intervention: none found. The translator's user messages are the goal (T:1), two alerts (T:236, T:2471) and child callbacks. Every message in a child's transcript matches an `engine_start` text from the translator; translator run.md:2 `heard: 0`.

## 2. Communication between engines

45 `engine_start` calls: 22 new children, 23 messages to existing children. Of those 23, **8 were mechanics** and 15 work:

- Mechanics: 4 research replies cut at 4,000 characters (T:545, :753, :846, :1461), the Stark timeout (T:1876), the dev-sparrow report cut (T:2169), the review-stark verdict cut (T:6468), and the review-stark path list missing (T:6568). Plus 7 children re-created after sonnet refusals.
- Work: 5 plan approvals, 2 research fixes (T:660 copy, T:1876 tags), 4 Sparrow review rounds, Yoda doc fixes (T:4747), Gandalf fix and recheck, Stark fix.

What was lost or misread:

- **Cut replies.** 10 of 36 replies were over 4,000 characters (largest 7,331, T:666). The wake line says "all of it is in the children artifact" (T:458) while the artifact holds only the first 4,000 (T:542 "The artifact cuts the reply at 'In Rebels he'"). The translator has no `read` tool (P(translator):26-51) although its own snapshot tells it to `read(name="summary")` (P(translator):95). From dev-holmes on, briefs said "under 4000 characters" (T:3627); later only review-stark went over (4,325).
- **A cap that cut the answer.** The translator capped the review-stark resend at 1,500 characters (T:6468); the 12 paths needed ~1,850, so they were dropped and a third round was spent (T:6563, T:6669).
- **A setting lost in a brief.** The dev-holmes brief lists Sparrow r1 as "rank 8, 16 layers, lr 1e-5, 400 iters" with no `mask_prompt` (T:3378). Sparrow trained with `mask_prompt: true` (M(dev-sparrow-opus):482); Holmes r1 did not (M(dev-holmes):1164), and Holmes r2 spent its one variable adding it back (C/sherlock-holmes/runs.md:11). The Yoda brief then passed on "r2 +mask_prompt" as a finding (T:4234).
- **A contradicted lesson passed on.** The dev-stark brief says "Val loss did not track voice in any build" (T:5953); C/gandalf/runs.md:43, accepted minutes before, says it did for Gandalf.
- **Sparrow review.** Two of three review rounds went on a misread `grep -c` count as a line number (T:2782) and on the reviewer reversing its own first verdict about the 'Hello.' recital (T:2880). The net change: runs.md:9 and :30.
- **No sender.** Children call the translator "the person" (M(review-stark):746; M(dev-stark):720, where the developer tried to ask "the person" how to go on).
- **Replies to the owner.** 3 in 40 turns (T:234, T:456, T:6667), although its instructions say to reply with every brief (P(translator):19) and to end with the summary as the reply (P(translator):16).

## 3. Parallel sub-engines

- The 5 research children ran together from 23:40:06 (TL:9) and were all judged by 23:59 (T:2068), 19 minutes. All five hit Wikipedia 429 and DuckDuckGo 403 or empty results (17-31 lines with 429 in each M(research-*-opus)); the open gaps in summary.md:36 come from those blocked reads. One shared rate limit is the likely cause; the run cannot prove it.
- Replies were taken one at a time (turns back to back 23:47:15-23:53:59); none was lost.
- Other overlaps (TL:408-411, T:6470) caused no conflict; one trainer at a time held.

## 4. Prompt rendering and growth

| engine | calls | first → last prompt tokens (max) | prompt tokens total | cost |
|---|---|---|---|---|
| translator | 103 | 7,265 → 167,116 (221,577, then compressed to 85,700 at 02:53, S(translator):303, :309; TL:475) | 11.97 M | $93.72 |
| dev-gandalf | 58 | 7,995 → 189,500 (254,517) | 6.30 M | $51.61 |
| dev-stark | 52 | 8,070 → 206,218 | 5.64 M | $47.69 |
| dev-yoda | 59 | 7,017 → 171,289 | 5.60 M | $47.21 |
| dev-sparrow-opus | 58 | 6,699 → 122,499 (171,703) | 4.72 M | $39.77 |
| dev-holmes | 48 | 7,596 → 173,380 (199,507) | 4.69 M | $38.58 |
| research-*-opus (5) | 18-34 | ~4,700 → 70,444-129,219 | 7.08 M | $61.51 |
| review-* (5 on opus) | 12-27 | 7,326-10,966 → 37,909-65,553 | 3.01 M | $26.50 |

What dominates the end prompts (characters):

- **P(translator)**, 381,211: 92% history even after compression. 25 skill bodies take **147,898 (39%)**: judging 17 times (63,699) and prompting 7 times (80,213). 17 whole-plan rewrites take 59,127 (15%). 29% of all long lines are exact repeats.
- Spans: **52 of 103 translator calls did nothing but load a skill** (the rule "skill(...) alone", P(translator):13, :16, :20). They cost **$48.39, 52% of the translator's spend**. That includes loading judging (~$1.6) to confirm a one-line fix it had already recorded (T:6584).
- **P(dev-gandalf)** and P(dev-stark), ~433,500: 96% history. The developer's own `edit` calls carrying whole data batches, and tool outputs reading blind batches in 15-29k blocks (P(dev-stark) largest block 29,364) make up most of it. Skill bodies are 3-4%; tool lists are 5,877. No developer was ever compressed.

## 5. The desk's summary

There is **no final reply**: the summary exists only as the artifact (engines/translator/summary.md). The last turn wrote it, ticked the plan, removed the alert and ended (T:6854-6872). Claims checked:

| # | Claim (summary.md line) | Source | Result |
|---|---|---|---|
| 1 | base 840M, 868,628,559 B (:5) | C/jack-sparrow/runs.md:3 | match |
| 2 | sonnet failed on "all 8 children" (:6) | children.md:2-6,12,14; spans: 5 research + dev-sparrow + review-sparrow = 7 | **mismatch** (the compression summary noticed it, T:3535) |
| 3 | research line counts 98/102/22, 89+83/152/21, 84/106/22, 102/146/20, 91/104/11 (:18) | `wc -l` of C/*/research | match |
| 4 | Sparrow 1.417 / 2.958 / 2.208 / 2.833 (:19) | C/jack-sparrow/runs.md:14-17; reviewer recompute T:2772 | match |
| 5 | Sparrow r1 lr 1e-5, 400 iters, 293.92 s, 215/24 (:19) | runs.md:15 | match |
| 6 | Sparrow accepted after two rounds (:19) | T:2683, :2880, :3076 | match |
| 7 | Holmes 1.00 / 1.62 (167.7 s) / 1.67 (177.8 s) / 2.00 (85.9 s, 1.23 GB, 2.804, 4 fails) (:20) | C/sherlock-holmes/runs.md:9-12 | match |
| 8 | Holmes 248, 223/25 (:20) | runs.md:3 | match |
| 9 | "Holmes r1 used the Sparrow r1 settings" (:10) | M(dev-sparrow-opus):482 vs M(dev-holmes):1164 | **mismatch** (no mask_prompt) |
| 10 | Yoda 73.6 s, r1-150 3.04 at 55.1 s, r2 74.9 s, r3 153.0 s, ckpt 300 3.08 at 115.0 s, 1.19 GB (:21) | C/yoda/runs.md:12-14, :28, :39 | match |
| 11 | Yoda 280/31, probe 2 of 12 (:21) | runs.md:3, :20 | match |
| 12 | Gandalf base 1.29 (6 fails), r1-150 2.46 at 63.30 s, r2-400 2.33 (86.22 s) (:22) | C/gandalf/runs.md:10, :12, :22 | match |
| 13 | Gandalf r3 88.87 s (:22) | runs.md:13 says 88.85, :31 says 88.77 | **mismatch** |
| 14 | Gandalf 292/32, probe 0 of 13 (:22) | runs.md:4, :36 | match |
| 15 | Stark 1.16 / 2.60 (73.26) / 2.72 (74.80) / 2.80 (76.33, 1262452736 B, 6 fails) (:23) | C/tony-stark/runs.md:10-13 | match |
| 16 | Stark 269/30, memo 0 in 13 sets, probe 0 of 13 (:23) | runs.md:4, :38, :39 | match |
| 17 | characters/runs.md 71 rows (:24) | 72 table lines including the header | match |
| 18 | "Val loss did not track voice in any build" (:27) | C/gandalf/runs.md:43 | **mismatch** |
| 19 | Holmes val minimum scored 1.58; Stark 2.28 (:27) | sherlock-holmes/runs.md:14; tony-stark/runs.md:47 | match |
| 20 | "The best scores came from short training" (:28) | Yoda best is r3-300 (yoda:16); Stark best is the last checkpoint, "may still be short" (tony-stark:51) | **mismatch** |
| 21 | Gandalf horse wrong in 11 of 12 checkpoints (:29) | gandalf/runs.md:41 | match |
| 22 | Stark f1, f2, f5, f6 fail in all 13 sets (:29) | tony-stark/runs.md:45 | match |
| 23 | Yoda r3 50-200 byte-identical to r2 (:13) | yoda/runs.md:43 | match |
| 24 | 12 stray dumps, 03:21:18-03:30:10 (:37) | review-stark's last reply, T:6669 | match |

**Tally: 24 checked, 19 match, 5 mismatch.**

Is it pure fact? Mostly. Not fact: "Delete them if you like" (:37); "instead of lesser-known characters" (:4), an alternative nobody raised; "because you asked for one small shared base" (:5), where the owner wrote that one "may be fetched once and shared" (message.txt:10).

Missing against message.txt:15:

- no reply;
- "what failed" leaves out the four Opus stops in Holmes, Gandalf and Stark (section 7), and calls the Gandalf cause "unknown" although M(dev-gandalf):1522 recorded "withheld";
- "every run with its numbers" leaves out Sparrow r2/r3 seconds, most facts-fail counts, val loss and all checkpoint rows;
- no cost, although the owner said the subscription was shared (message.txt:9).

## 6. Quality of the work

**Scoring.**

- Every score was given by the developer that wrote the card, the data, the prompts and the rubric. No second scorer or judge model was used in any build.
- The reviewers recomputed means from those scores only, "arithmetically only, not as judgements" (M(review-yoda):642).
- Blinding was by shuffled ids. But the base answers are recognisable ("a plain assistant with costume words", C/jack-sparrow/runs.md:14), developers saw r1 answers before their final batch (summary.md:34), and Sparrow's batch 2 was "only partly blind" (C/jack-sparrow/runs.md:31).
- Each build had n = 24-25 prompts. The Stark steps 2.60 → 2.72 → 2.80 are "small next to blind-scoring noise" in the developer's own words (C/tony-stark/runs.md:44).
- Across characters the eval differs (Sparrow temp 0 with a 167-word system line, the others temp 0.3 with rep 1.1; C/runs.md:4) and so do the prompts and rubrics. "Yoda 3.08, the best tonight" is not a like-for-like ranking.

**One variable per run.**

- Sparrow: one variable per run (iters).
- Holmes: r1 was not Sparrow r1 (see above), and r3 changed lr and iters together (C/sherlock-holmes/runs.md:12). review-holmes did not flag it.
- Yoda: r3 changed two variables against r1 (yoda:14). review-yoda flagged it, and the rule entered the Gandalf brief (T:4953).
- Gandalf: one variable each, but r3's lr change was a fallback after the data batch was stopped (gandalf:13).
- Stark: one variable each, but the r2 data change altered the split, so val loss is not comparable (tony-stark:12).

**Other checks.**

- The copy check compares with saved research sources only, so film lines cannot be caught by it. dev-sparrow-opus caught one near film line by its own reading (M(dev-sparrow-opus):364).
- Facts never improved in any build. Four runs.md files name the 1.5B base as the ceiling. The translator kept it, reading the owner's "may" as a rule (T:3925), although the owner put quality first (message.txt:8).

**Lessons carried:**

| Lesson (source) | Applied |
|---|---|
| choose by blind checkpoint scores (jack-sparrow:25) | Holmes on (sherlock-holmes:3, :16) |
| half everyday (jack-sparrow:26) | Holmes ~116/248 (M(dev-holmes):1164), Yoda 146/311, Gandalf 47%, Stark 51% |
| 6-10 per Never rule (jack-sparrow:27) | all; "are you an AI" still weak (gandalf:45) |
| temp 0.3, rep penalty, short system line (jack-sparrow:29) | Holmes on (sherlock-holmes:3) |
| memo vs data + paraphrase probe (jack-sparrow:30) | Holmes 0/10 (:12), Yoda 2/12, Gandalf 0/13, Stark 0/13 |
| one run with checkpoints, not three (jack-sparrow:36) | not applied; every build ran 3 runs |
| bigger base (sherlock-holmes:41) | not applied, by decision (summary.md:9) |
| 4-6 phrasings per fact (sherlock-holmes:42) | Yoda, Gandalf, Stark; facts did not take in any |
| vary greeting answers (sherlock-holmes:44) | not at Yoda ('Hi!' recited, yoda:57); applied at Stark (tony-stark:4, :48) |
| 5e-6, mask_prompt, 200 iters (sherlock-holmes:47) | Yoda r1 (yoda:12) and every later r1 |
| line-stamped logs (sherlock-holmes:48) | Yoda ts.py (yoda:22) on |
| dedupe / fewer checkpoints (sherlock-holmes:49, yoda:62) | Yoda still scored 408; Gandalf 312→309, Stark 325→325 |
| resume, not retrain (yoda:61) | Gandalf r2 (gandalf:12) |
| new seed per run (T:4953) | Gandalf 11/22/33, Stark |
| log peak memory (gandalf:11) | Stark every run (tony-stark:3) |
| more iters and lr 1e-5 did not help (gandalf:42) | Stark spent r2/r3 on data and rank (tony-stark:12-13) |

## 7. Refusals and withheld content

| Model | Engine | Evidence | Next |
|---|---|---|---|
| sonnet ("Sonnet 5.5's safeguards flagged this message") | research-sparrow, -holmes, -yoda, -gandalf, -stark at call 2 or 3 | S(research-yoda):6 and siblings | re-created on opus, same brief (T:439) |
| sonnet | dev-sparrow, call 1 | S(dev-sparrow):1 | dev-sparrow-opus (T:1666) |
| sonnet | review-sparrow, call 3 | S(review-sparrow):12 | review-sparrow-opus (T:2673); sonnet not used again (T:3375) |
| opus | dev-holmes 00:58 | "My last draft was stopped" (M(dev-holmes):935) | rewrote with "every line benign" |
| opus | dev-holmes 01:08 | "My last step was cut off" (M(dev-holmes):1122) | checked the disk, went on |
| opus | dev-gandalf 02:41 | "The fact-02 batch I was writing was withheld" (M(dev-gandalf):1521) | the r3 variable changed to lr; "withheld" was later removed from runs.md:13 (M(dev-gandalf):2496) |
| opus | dev-stark 03:07 | "stopped by a safety classifier" (M(dev-stark):720) | tried to ask the person; blocked only because the reply sat beside `run` (:740); retracted as wrong (:744) |

The "[reasoning_extraction]" tag (T:456, summary.md:6) is in no captured error; the span text is cut at "C…".

None of the four Opus stops is in a transcript or a span error. Each coincides with a call whose prompt tokens are about double its neighbours': S(dev-holmes):105 and :162, S(dev-gandalf):257, S(dev-stark):66. Two more such calls have no comment from the model (S(dev-sparrow-opus):163, S(dev-yoda):65). The journal reports one Opus withhold (journal.md:254); the transcripts show four.

## 8. Fragile parts, most costly first

1. **Skill bodies in history, plus "load it alone"**: 52 calls, $48.39, 39% of the last translator prompt. Covered by fragile.md part 2.1; its fix (one copy while loaded, the "alone" rule gone) would have removed most of it.
2. **Developer history without compression**: $225 over five developers, prompts to 254k tokens, 96% history (own data edits, blind-batch reads). Part 2.2 folds artifact writes only, so it would have saved little.
3. **Self-scored evals**: every number rests on the scorer that wrote the data. Not covered; a scorer child that never saw the data would fix it.
4. **Stopped or withheld output not recorded**: a lost variable (Gandalf), a near stop to ask an absent owner (Stark), and a cause nobody could state. Not covered; part 4 covers sonnet only, and its "refused N times" mark would have saved 2 sonnet attempts and one alert wait.
5. **Replies cut at 4,000, wake line says "all of it"**: 8 rounds. Part 1A (reply shown whole when it wakes) would have saved 6; the wording is not covered.
6. **A failed child calls nobody**: 360 s of alert waits, two near misses. Part 1.2, built after the run, would have helped all four.
7. **No closing reply**: not covered; part 5's `reply(artifact="summary")` or part 4's `leaves:` would catch it.
8. **Off-format answers**: 3 steps lost to raw `<invoke>`. Not covered.
9. **Root guard on edit only**: `cp combined-runs.md ../runs.md` (M(dev-stark):2558) and 12 dumps in other sessions' scratchpads; the brief itself asked for a file outside the root. Not covered.
10. **Lessons retold by the head**: mask_prompt dropped, a contradicted claim passed on. Not covered; briefs could cite runs.md lines instead.
11. **One rate limit for five research clients**; **no sender on messages** (part 1.3, small help).

| Finding | Evidence | Severity |
|---|---|---|
| No final reply; summary only as artifact | T:6863-6872, S(translator):416 | high |
| All scores self-given, no independent scorer | M(review-yoda):642, summary.md:34 | high |
| 52/103 translator calls only load a skill, $48.39 | S(translator), P(translator) | high |
| Opus stops not recorded; Gandalf data variable lost; Stark nearly asked absent owner | M(dev-gandalf):1521, M(dev-stark):720-744 | high |
| Child failures silent; 2 alert rescues, 2 lucky catches | TL:6-7, :185-186; T:1456, :1791 | medium |
| Replies cut at 4,000, wake line says "all of it"; 8 mechanics rounds | T:458, :542 | medium |
| mask_prompt lost in the Holmes brief; summary repeats it as fact | T:3378, M(dev-sparrow-opus):482, summary.md:10 | medium |
| Summary: 5 of 24 checked claims wrong | section 5 | medium |
| Developer prompts to 254k with no compression | S(dev-gandalf):257 | medium |
| Holmes r3 and Yoda r3 changed two variables | sherlock-holmes:12, yoda:14 | medium |
| Off-format `<invoke>` steps lost | M(dev-gandalf):2069, M(dev-stark):2397 | low |
| Shell writes outside root, stray scratchpad files | M(dev-stark):2558, summary.md:37 | low |
| No operator intervention | T user messages, run.md:2 | none |
