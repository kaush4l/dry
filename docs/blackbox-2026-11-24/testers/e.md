# Black-box report E: long-running goal, recovery, interruption, usage metadata

Tester E, 2026-11-24, 17:25 to 18:25 EST. Hosts: `e` (127.0.0.1:8115) and `e2` (127.0.0.1:8137),
both on model row `omlx` (Inkling-2-Mini). The model queue was slow for most of the hour
(one call took over 10 minutes), so the big goal did not finish inside the time box. Everything
below is what was seen; anything not seen is marked UNVERIFIED.

Evidence files (all under `bb/`): `e/timeline.log`, `e2/timeline.log` (status changes, 5 s
resolution), `e/events.log`, `e/events2.log` (the `/events` stream before and after the restart),
`e/state/engines/*/convo/messages.md`, `e/state/engines/*/spans.jsonl`, same under `e2/`,
`e/prompt_translator_0.txt`, `e/prompt_translator_wake.txt`, `e2/prompt_child.txt`,
`e/page_desk0_failed.txt`, `e/page_child_after_restart.txt`.

## 1. Scenarios

| # | What I asked (short) | Expected as a user | What happened | Verdict | Evidence |
|---|---|---|---|---|---|
| 1 | "In <proj>: (1) build ... wordfreq.py ... with unit tests that pass; (2) write notes.md comparing ... dict vs collections.Counter ... 1 MB ...; (3) make it at least 20% faster ... Plan it, do it, and give me a final summary with evidence. Do not ask me anything unless you are truly blocked." (host e, 17:25:20) | A plan within a few minutes, children working, a final summary, no help from me | Turn 1 died at 17:36 with `failed: timeout: global` on step 2, before any plan. Nothing restarted it. I had to write "It stopped with a timeout. Please continue with the goal above." (17:39:23). Plan artifact at 17:47:47, one child `dev-wordfreq` started 17:51:29, first file `wordfreq.py` at 18:19. At 18:25 (60 min) only deliverable 1 was half done (no tests), 0 of 3 plan lines ticked, no summary | FAIL (not finished; needed me once) | `e/timeline.log`; `e/state/engines/translator/spans.jsonl` turn span `"error":"timeout: global"`; `e/state/engines/translator/artifacts/plan.md` |
| 2 | Truth check of what it claimed | Every claim holds when I rerun it | It never claimed "done", so there was no false claim. The translator's replies said only what it had started ("I will report when it replies"). `wordfreq.py` on disk runs correctly on my own 10-word file. Tests, notes.md, timing, 20% speed-up: do not exist | PARTIAL (honest, but nothing to verify) | claim table below |
| 3a | "In <proj> there should be a file data/sales.csv ... Write total.py ... run it, and show me the output." with no such file (host e2, 17:41:11) | Notices the file is missing, says so plainly, does not invent data | Brief told the child to stop if the file is missing. Child ran `ls`, `cat` (exit 1), replied "data/sales.csv is missing ... I stopped rather than invent data". Translator asked me a two-option choice at 17:57:41 | PASS | `e2/state/engines/dev-sales-total/convo/messages.md` lines 1-30; `e2/state/engines/translator/convo/messages.md` (reply with `ask="choice"`) |
| 3b | Answered "Create a sample CSV with made-up rows" after making `data/` read-only (`chmod 555`) | Notices the write failed, finds out why, tells me | `save(path="data/sales.csv") → error: ... Permission denied (os error 13)`. Child ran `ls -ld data; id`, then `touch` to confirm. At 18:25 it changed its approach by itself: `chmod 755 data` (exit 0), without asking, then `save(path="data/sales.csv") → error: no unsaved edits in data/sales.csv`: the failed save had dropped the pending text, so the file was still missing and `python3 total.py` exited 1. It did not claim success. 16 model calls by 18:26, 7 thrown away as "no act"; 28 min after my answer it had still not replied, so I was never told | PARTIAL | same file, lines 100-143; `e2/timeline.log` |
| 4 | `host.sh restart e 8115 5` at 17:57:05 while `dev-wordfreq` was mid model call (step 2) | Work continues by itself, the page says what happened | After restart both engines showed `idle`; nothing said "interrupted". The `keep-going` alert fired at 18:00:12 (3 min idle, plan had open lines). The translator's first post to the child failed (625 > 600 characters), the second went through at 18:06:53 and the child went on. 9 min 48 s from restart to the child working again, no help from me. The child's in-flight call (about 4 min 50 s of model time) left no record at all | PARTIAL | `e/timeline.log` 17:57-18:06; `e/state/engines/translator/convo/messages.md` ("Alert keep-going fired"); spans: no llm span for dev-wordfreq between 17:52 and 18:06 |
| 5 | Usage metadata (spans, /analytics, prompts) | Real token counts per call, totals per turn/engine/goal, a cost, and the engine able to see them | Per call: model, provider, purpose, step, prompt_tokens, completion_tokens, ttft_ms, start/end, error. All token counts are estimates (`tokens_est: true`) and leave out the model's reasoning. Totals exist per engine page and for the day; none per goal across parent and children. Cost shows "–". Nothing about usage is in any engine's prompt | PARTIAL | section 4 below |
| 6 | Morning view from the pages alone | Planned / done / failed and why / cost | Planned: yes (plan page). Done: partly (per-engine timeline, terminal list). Failed and why: only the bare text `failed: timeout: global`; the restart and the 9 thrown-away answers are not visible as events. Cost: no. An open question to the person shows "Need you 0 · nothing" on Home | PARTIAL | section 3 |

### Timeline of the big goal (host e)

| Time | Event |
|---|---|
| 17:25:20 | Goal sent. Translator step 1: `skill(name="prompting")` (44 s) |
| 17:26:04 - 17:36:04 | Step 2: one model call, streaming reasoning the whole time (over 10,000 characters seen on `/events`), no answer text. Killed at exactly 600 s: `timeout: global`. Turn failed |
| 17:36 - 17:39 | Nothing. `keep-going` cannot fire: its condition is "plan has an open `- [ ]` step" and no plan exists yet |
| 17:39:23 | **I intervened** (the only time): "It stopped with a timeout. Please continue with the goal above." |
| 17:44:41 | Answer with no `act`; app asked again |
| 17:47:47 | Plan artifact written: 3 lines, all given to one child (serial, not parallel) |
| 17:51:29 | `engine_start dev-wordfreq` (template developer, root = my proj). Good brief |
| 17:51 - 17:56 | Translator needs 4 model calls to say "I started it": three answers with no `act` in a row |
| 17:57:05 | I restart the host (scenario 4) |
| 18:00:12 | `keep-going` alert wakes the translator (self-wake 1) |
| 18:05:10 | `post` to child fails: "an entry is at most 600 characters; this is 625" |
| 18:06:50 | Second post goes through; child resumes |
| 18:11 - 18:13 | Child: `python3 --version`, writes its own 8-step plan, `goto_phase code`, loads python skill |
| 18:13 - 18:19 | Child: three answers with no `act`, then writes and saves `wordfreq.py` (43 lines), `python.check` exit 0 |
| 18:20 - 18:25 | Child: another answer with no `act`; still on the test file when my time ended |

No judge round, no send-back, no reviewer and no final summary happened inside the hour.

### Claim check (scenario 2)

| Claim (who, when) | Verdict |
|---|---|
| Translator 17:56: "I started a developer child (dev-wordfreq) ... I will report when it replies" | TRUE (engine exists, brief matches) |
| Translator 18:10: "The child stopped before replying, so I sent it a nudge" | TRUE (thread post 18:06) |
| Child (in its thoughts, not a reply): "wordfreq.py v1 is saved and compiles" | TRUE. I ran `python3 wordfreq.py` on "the cat the dog The end a a a b": printed `a 3 / the 3 / b 1 / cat 1 / dog 1 / end 1`, exit 0 |
| Unit tests pass | UNSHOWN (no test file exists) |
| notes.md with a measured timing on 1 MB | UNSHOWN (no file) |
| 20% faster, before/after numbers | UNSHOWN (no file) |
| e2 child: "data/sales.csv is missing ... only data/, which is empty" | TRUE |
| e2: total.py written | TRUE (file exists); it fails with FileNotFoundError when I run it, as expected, since no CSV could be written |

## 2. Findings (most severe first)

1. **A model call is killed at 10 minutes even while it is streaming, and the failed turn is never retried.**
   Seen: translator step 2, 17:26:04 to 17:36:04, `"error":"timeout: global"`, `ttft_ms: 585483`; the
   `/events` stream showed the reasoning count growing to the end. The turn ended `failed`, and the
   goal stood still until I wrote to it.
   Reproduce: send the scenario-1 goal while the model is slow (shared by 6 testers).
   Why it matters: overnight this is the end of the job at minute 11. The only self-wake
   (`keep-going`) needs a plan with an open line, and the failure came before the plan.
   Severity: **blocker** for the overnight case. APP (loop): a timeout on total time instead of on
   silence, no retry, no alert for "a turn failed".

2. **Answers with a thought but no `act` are thrown away again and again, with the same sentence each time.**
   Seen 16 times in 41 model calls: host e translator 4, `dev-wordfreq` 4, e2 translator 1,
   `dev-sales-total` 7 (six in a row, each a word-for-word copy of the last thought). Each costs a
   full model call (20 s to 5 min). It happens exactly when the next act is big (a reply with the
   brief, a file's text, a save).
   Reproduce: any goal on this model; `grep -c 'did not follow the response contract' state/engines/*/convo/messages.md`.
   Why it matters: 39% of the model calls in my hour produced nothing. A person sees "calling the
   LLM again (step 10)" and no reason.
   Severity: **major**. Split: MODEL (it stops after the thought), but the APP's feedback does not
   help it: the retry text is always "Your answer did not follow the response contract (the answer
   has no act)", it does not grow firmer, does not show the model its own cut answer, and there is no
   different handling after the third identical miss. UNVERIFIED whether an output-length cap
   plays a part: in these calls the first visible token came late (40 to 190 s) and the visible
   answer was 45 to 100 tokens, which would also fit reasoning using up the allowance.

3. **Token counts are estimates and leave out the reasoning, so usage is far too low.**
   Every llm span has `"tokens_est": true`; /analytics says "Tokens marked est were counted from
   the text (the server sent no usage)". The timed-out call streamed over 10,000 characters of
   reasoning and has no `completion_tokens` at all. Another call streamed about 3,200 characters of
   reasoning and is recorded as 109 completion tokens. Whole hour on host e: 137,342 prompt
   tokens and only 3,446 completion tokens for 52 minutes of model time (about 1 token/s), which
   cannot be right. /analytics shows "Tokens/s 3.6".
   Why it matters: the owner wants usage as metadata the loop can plan with; these numbers would
   mislead it. Severity: **major**. APP (does not ask the server for usage, does not count the
   reasoning stream it already receives).

4. **The engine sees none of its usage.** `host.sh prompt e translator` and `host.sh prompt e2 dev-sales-total`
   contain no tokens, time, cost or budget, in long or short form (grep for token/cost/budget finds
   only prose). The children artifact shows a child as "idle · 0 replies" with no steps, time or
   tokens. The translator cannot know a child took 14 calls and 25 minutes. The engine page shows
   "prompt ≈ 7.3k of 1049k tokens (1%)" to the person only.
   Severity: **major** against the stated intent. APP.

5. **A restart loses the in-flight work silently.** After `restart`, the child that was mid-call is
   just `idle`; no page, no thread post and no span says it was cut off. The interrupted call
   (17:52:15 to 17:57:05) is in no spans file, so its time and tokens are gone from every total.
   Recovery took 3 min (alert) + 6.5 min (two translator steps) and worked only because a plan with
   open lines existed. Had the restart come before the plan, or with every line ticked but the
   summary unwritten, nothing would have woken. UNVERIFIED for those two cases.
   Severity: **major**. APP.

6. **The 600-character post limit cost a full step during recovery.** The translator's first
   "continue" post was 625 characters: `error: an entry is at most 600 characters; this is 625`.
   That retry took 1 min 40 s; the step before it took 5 min. The alert text tells it to "post to it
   what to go on with" while the judging skill says a brief must be complete, so long posts are the
   natural result. Severity: minor. APP (limit not shown as a remaining count; or truncate with a
   pointer to the brief).

7. **Home says "Need you 0 · nothing" while a question to the person is open.** On e2 at 17:58 the
   translator had an open `choice` ask; Home showed "Need you 0 nothing". A failed turn did show
   "Need you 1". Someone checking Home in the morning would not see they are the blocker.
   Severity: major for the overnight case. APP (UI).

8. **A self-wake is recorded as the person.** The alert turn's span has `"trigger":"user"`,
   `"by":"user"`, and /analytics counts it under "Turns you started, by hour · 18:00 · 1". Child turns
   show `post`, child replies `callback`, answers `answer`, so only alerts are mislabelled.
   Severity: minor. APP.

9. **The plan gave all three deliverables to one child, in order.** The translator's prompt says
   independent deliverables start together. The model judged them dependent (3 needs 1), which is
   fair, but notes.md (2) did not need the tool. No parallel work was seen. Severity: minor. MODEL;
   the plan line format has no place to say "needs: 1", which would make the choice explicit.

10. **No goal-level view.** Totals exist per engine page ("Model calls 2 · 11.7k tokens") and for
    the whole day on /analytics. There is no total for one goal across the translator and its
    children, no cost (always "–"), no budget. Severity: minor now, major for planning. APP.

11. **The environment line names the app's root, not the workspace.** Both prompts end with
    `environment: host · ready · .../bb/e/root · python3, ...` while the child's tools run in `proj`.
    The translator's rules say "when the person names no folder, it is the workspace on your
    environment line". May be an effect of the test script's setup; UNVERIFIED. Severity: minor.

12. **While a long call runs, the engine page shows only "calling the LLM again (step 2)".** For
    10 minutes `/desk/0` had no elapsed time and no reasoning text. Later `/desk` did show a child's
    live "Model · writing · thinking" text, so this may differ by page. Severity: polish. APP.

13. **A save that fails throws the unsaved text away.** e2 child: `edit(path="data/sales.csv", new=...)`
    → "new file ... unsaved"; `save` → "Permission denied (os error 13)"; after fixing the folder,
    `save` → "error: no unsaved edits in data/sales.csv". The child must write the whole file again.
    Reproduce: `chmod 555` a folder, ask for a new file in it, make it writable, save again.
    Severity: minor (one more wasted step in a recovery). APP (tools).

14. **The child changed folder permissions on its own.** `chmod 755 data` was outside "Change only
    what is needed ... total.py" and it did not ask. Here it was what I would want, but the scope
    text and the app have no rule for permission changes. Severity: minor. MODEL, with no APP guard.

15. **"Unsaved edit" trap.** The e2 child wrote two files and ran one in the same act without
    `save`; the run failed "No such file". The tool result did say "unsaved; save(path=...) writes
    it", and the child understood at once, but then lost 7 calls to finding 2. Severity: polish.
    MODEL (the template shows edit; save; run in one act).

What worked: the missing-file case was handled the way the owner wants (look, confirm with a
command, stop, say it plainly, offer two real options). The brief the translator wrote was precise
and carried a "do not invent data" stop rule. The child confirmed the read-only folder with two
commands before deciding. Nobody claimed anything that was not shown. The `keep-going` alert did
bring a restarted goal back without me.

On "a strict way to understand the problem": the shipped developer template has a `debugging`
skill (a loop that shows the failure, three hypotheses, one change at a time, stop after the same
failure twice) and the translator has `judging`. Neither was loaded in my runs: the failures were
plain enough, and no child reply came back on the big goal. So the strict path exists on paper;
UNVERIFIED in practice.

## 3. Usage feedback

Good: the plan page; the per-engine Timeline (each step, its model call, tokens, first-token
time, the error); the child's Live view with its terminal commands and exit codes; the thread post
showing exactly what the translator told the child; the honest replies.

Confusing or missing:
- `failed: timeout: global` does not say what timed out, after how long, or what to do.
- After the restart nothing said a restart happened or that a child was cut off.
- "calling the LLM again (step 10)" hides that seven of those steps were thrown away.
- The translator needed four calls just to tell me it had started the child.
- No single place says "goal: 0 of 3 done, running 58 min, 41 calls".
- Cost is "–" everywhere; tokens are marked "est" with no hint how far off.

Morning test (scenario 6), from pages alone: planned yes; done partly (must open each engine);
failed and why no (bare error text, no restart, no retries); cost no.

Five concrete improvements:
1. Time out on silence (no bytes for N seconds), not on total length; on a failed turn retry once,
   then raise an alert that does not depend on a plan existing, and show it under "Need you".
2. Count usage properly: ask the server for usage in the stream, or count the reasoning and answer
   characters the app already receives; store `in/out/reasoning/ttft/dur` on every call, including
   calls that failed or were cut off by a restart.
3. Put a one-line short-hand of usage in the prompt: for itself and per child in the children
   artifact, e.g. `dev-wordfreq · code · 8 calls · 38.8k in / 1.0k out · 14m · 4 retries`.
4. Make the no-act retry smarter: quote the cut answer back, say "write only the act line now",
   and after three identical misses change something (shorter prompt, the act asked for alone) and
   show the count on the page.
5. On start-up, mark every engine that was mid-turn as "interrupted at HH:MM (restart)", post it
   on the thread, and resume it at once instead of waiting for the idle alert. Add a goal page:
   plan lines, who is on each, time, calls, tokens, failures.

## 4. Numbers

Host e (big goal), 17:25:20 to 18:24, from `spans.jsonl` (all token counts are the app's estimates):

| Engine | Model calls | Errors | Prompt tokens | Completion tokens | Model wall time | Tool calls (failed) | No-act retries |
|---|---|---|---|---|---|---|---|
| translator | 12 | 1 (timeout) | 98,576 | 2,432 | 2,268 s | 5 (1: post too long) | 4 |
| dev-wordfreq | 8 | 0 | 38,766 | 1,014 | 859 s | 8 (0) | 4 |
| Total | 20 | 1 | 137,342 | 3,446 | 3,127 s (52 min) | 13 (1) | 8 |

Plus one child call lost at the restart (about 290 s, not recorded).
Translator turns: 3 (user, user, alert recorded as user): 2, 7 and 3 steps; 644 s, 1,016 s, 608 s.
First-token time p50: translator 61 s, child 50 s. Largest prompt 10.2k tokens; growth +615 tokens/step.
My interventions: 1. Self-wakes: 1. Children: 1 (serial). Judge rounds: 0. Plan lines ticked: 0 of 3.
Time to plan: 22 min 27 s. Time to first child: 26 min 9 s. Time to first file: about 54 min.

Host e2 (planted failures), 17:41:11 to 18:24:

| Engine | Model calls | Errors | Prompt tokens | Completion tokens | Model wall time | Tool calls (failed) | No-act retries |
|---|---|---|---|---|---|---|---|
| translator | 7 | 0 | 48,578 | 1,148 | 965 s | 3 (0) | 1 |
| dev-sales-total | 14 | 0 | 61,804 | 1,189 | 1,528 s | 9 (1: permission denied) | 7 |
| Total | 21 | 0 | 110,382 | 2,337 | 2,493 s (42 min) | 12 (1) | 8 |

Missing file: noticed in 2 child steps (about 5 min wall), question to me 16.5 min after the goal.
Read-only folder: noticed on the first save, confirmed in 2 more commands, worked around with chmod at 18:25; not reported to me in 28 min. (Table above is the 18:24 snapshot; by 18:26 the child had 16 calls and 2 failed tools. I stopped e2 at 18:26 and set data/ back to 755.)

Span fields seen. llm: model, provider, purpose (always "step"), step, prompt_tokens,
completion_tokens, tokens_est, ttft_ms, error. tool: name, mode, ok, error, result_chars, step.
step: step, calls, phase. turn: trigger (user / post / callback / answer), goal (cut to 80
characters), model, outcome, steps, error, ask, asked, ask_note. task: by, goal, outcome, turns,
parent_engine. No cost, no reasoning tokens, no cached tokens, no finish reason, no goal id that
links a child's spans to the parent's task other than `parent_engine`.
