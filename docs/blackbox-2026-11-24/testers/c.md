# Report C: alerts (an engine places an alert, the alert calls the loop back)

Tester C, 2026-11-24, 17:24 to 18:14 local (about 50 min; the model queue was slow).
Hosts: `c` (8113, main, left running), `c2` (8129, stopped), `c3` (8130, stopped).
Evidence paths are relative to `bb/`. `c/ev/events.log` is my timestamped recording of `GET /events` on host c.
Epoch seconds in `run.md` and `spans.jsonl`: 1795559113 = 17:25:13 local.

## 1. Scenarios

| # | Asked (verbatim, short) | Expected as a user | What happened | Verdict | Evidence |
|---|---|---|---|---|---|
| 1 | "In one minute, tell me the time and say PING." (c, 17:24:44) | An alert is placed, the reply says when, it fires on time by itself, the reply has PING and the time, the one-shot goes away | Alert `one-minute-ping` (`every 1m`, once) made 17:25:13. Reply: "Set — in one minute I'll tell you the time and say PING." (no clock time). A turn started by itself at 17:26:14.5 (61.3 s after it was made, so 1.3 s after due). Reply at 17:27:00: "PING ... I don't have a clock tool in this session, so I can't read the exact wall time". The alert file was removed after firing. | PARTIAL (alert loop PASS; "tell me the time" not possible) | `c/state/engines/translator/convo/messages.md` lines 1-25; `c/state/engines/translator/spans.jsonl` turn rows start_ms 1795559084094 and 1795559174509; `c/ev/events.log` 17:26:14 `activity llm` + `message role user` |
| 2a | "Every minute, check whether the file watch.txt exists in the workspace and tell me only when it appears." (c, 17:29:10) | A repeating alert, quiet until the file is there | The model thought for 10 minutes in a loop about how to stay silent (a turn must end with a reply), then the call was cut: `failed: timeout: global` at 17:39:11. No alert, no reply, about 6000 characters of streamed thinking lost. The page shows only "failed". | FAIL | spans turn 1795559350970 `"error":"timeout: global"`, 600 s; events.log 17:39:11; `/desk/0` text "✕ failed" |
| 2b | Same ask, plus "If it is not there yet, a one-word reply like 'no' is fine." (c, 17:41:03) | Alert every minute, a "no" each time, then a notice when the file appears | Alert `watch-check` (`every 1m`) made 17:50:05 (the turn took 10 min 27 s, 3 steps). It then fired 7 times (17:51:05 to 18:08:05 by `run.md`), every fire starting at the instant the previous turn ended. The translator had no file tool, started a `researcher` child `check-watch` (honest, no false claim). The child ran `ls`/`find` once at 17:56-17:57 and replied "No" at 18:07. I created `watch.txt` at 18:00:47 (in `c/root`, the real workspace, and `c/proj`). It was never detected: the child did not run a check after that time. Replies between were "Still waiting on the check-watch researcher", not "no". | FAIL | `c/state/engines/translator/convo/run.md` (`alert: watch-check 1795560605 1795561685 7`); messages.md lines 53-331; `c/state/engines/check-watch/artifacts/terminal.md` (last run 1795561352 = 18:02:32, a `git status`) |
| 3a | "What alerts do I have? And please cancel the repeating one." (c, from 18:02:36, retried every 2 s) | An answer | HTTP 409 "engine is in a turn" on every try for more than 9 minutes: alert turns ran back to back, with no gap for a person. | FAIL | `c/ev/cancel.log` is empty (no try got through); first 409 at 18:00:47 |
| 3b | Thread post: "Please cancel the repeating watch-check alert now and tell me which alerts are left." (c, 18:07:56) | Cancelled soon | The post was accepted while busy (200, "posted entry 1"). It reached the model at 18:08:37 inside one message together with a child reply and one more alert fire. The model removed the alert at about 18:13 (`alert(name="watch-check", text="")`); the store folder `spec/alerts/` is gone. About 5 min from ask to removal. The final reply listing what is left was not seen before I stopped. | PARTIAL | messages.md lines 318-326 and 411; `ls c/state/engines/translator/spec/alerts` → missing at 18:13:40 |
| 3c | "Remind me at 25:99 to stretch." (c3) | Told it is not a time | Model asked with a text control: "25:99 isn't a valid time. What time did you mean — and should it be a one-time reminder or recurring?" | PASS | `c3/state/engines/translator/convo/messages.md` (tail) |
| 3d | Probe: set `at 25:99`, `at 2020-01-01 09:00`, `every 10s` exactly | Clear errors for all three | `at 25:99` → "error: alert bad-time: `25:99` is not a time like 09:35 (00:00 to 23:59)". `every 10s` → "error: ... a number then m, h or d, at least 1m, e.g. every 30m". `at 2020-01-01 09:00` → **accepted**: "made alert past-date: at 2020-01-01 09:00, once." The model then removed it by itself, so whether a past alert ever fires is UNVERIFIED. | PARTIAL | c3 messages.md, the tools block after "I am testing the app" |
| 3e | More than 8 alerts | A limit with a clear message | Not run (time). | BLOCKED | - |
| 4 | Alert due while the engine is busy | Fired once after the turn | Seen three ways on c: (i) `watch-check` made 17:50:05 inside a turn that ended 17:51:30; due 17:51:05; `run.md` records the fire at 17:51:05, the turn started 17:51:30 (25 s late), once. (ii) Fires that came due during later turns were delivered one per turn end, never two at once, never dropped: 7 fires in 18 minutes for an "every 1m" alert, so about 11 due minutes were skipped silently (coalesced). (iii) Nothing says "this fired late". | PASS for "after the turn, once"; see finding 1 for the side effect | spans turn rows 1795560690 to 1795561717 (each start = previous end); run.md count 7 |
| 5 | "In two minutes, say PONG to me." then host down across the due time (c3) | Fired late and said so, or marked missed | Alert made 17:46:14, due 17:48:14. Host killed 17:46:44, back at about 17:49:14. The alert turn started 17:49:15.6 (61 s late, 1-2 s after start). Reply "PONG". The one-shot was removed. Nothing in the message, the reply or the page says it was late or that the host was down. | PARTIAL (not lost; lateness not told) | `c3/.../spans.jsonl` turn start_ms 1795560555644; c3 messages.md line 24 "Alert pong-timer fired (every 2m): ..." |
| 6 | Built-in `keep-going` with an open plan (c2) | An idle engine with open `- [ ]` lines is woken, and the page says why | First ask (two files) gave no plan: the model sent one developer child (it ran 35+ min for two one-line files and was still going). Second ask, explicit, wrote a plan with two open lines at about 18:10. I ran out of time before 3 idle minutes could pass. `keep-going` never fired on any host. | BLOCKED | `c2/state/engines/translator/artifacts/plan.md`; c2 run.md `alert: keep-going ... 0` |
| 7 | Can a person tell an alert turn from their own? | A badge or a different bubble; a trigger field in spans | No. The fired alert is stored as `### user`, drawn as `<div class="msg user">` like my own messages, the timeline says "turn · user", spans say `"trigger":"user"` and `"by":"user"`, `/events` sends `message role user`. The only clue is the text "Alert <name> fired (every 1m): ...". | FAIL | spans turn row 1795559174509 (`"trigger":"user"`); `/desk/0` HTML; events.log 17:26:14 |

## 2. Findings (most severe first)

1. **A repeating alert on a slow model locks the person out of their own engine.** (APP, blocker)
   Seen: `every 1m` alert, turns of 80-300 s. Each queued fire starts at the same millisecond the previous turn ends (spans: end 1795560986878, next start 1795560986878 region), so `POST /api/engines/translator/messages` returned 409 on every try for 9+ minutes (retry every 2 s). The person cannot say "stop". 7 model turns were spent saying "Still waiting".
   Reproduce: ask "Every minute, check whether the file watch.txt exists in the workspace and tell me when it appears. If it is not there yet, a one-word reply like 'no' is fine." on a model whose turn takes more than a minute; then try to send any message.
   Why it matters: an alert the user asked for becomes a loop they cannot interrupt, and it burns model time without end.
   What would fix it: a person's message goes before a queued alert fire (or is queued, not refused); an alert does not fire again while its last fire's work (its turn, or a child it started) is still running; a cancel/pause control for each alert on the alerts page that needs no model turn.

2. **An alert turn cannot end without a reply, so "tell me only when..." has no valid answer; the model spun for 10 minutes and the turn was killed.** (APP mainly; MODEL for looping)
   Seen: the live thinking on `/desk/0` said "If the file doesn't appear, I should... maybe not reply at all? But the alert wakes me and I must do something." It repeated this until `timeout: global` at exactly 600 s. Nothing was saved to `messages.md`; the page shows "✕ failed" with no reason and no retry hint (the reason "failed: timeout: global" is only in `/api/status` and `/events`).
   Reproduce: the scenario 2a message.
   What would let the model do it right: one sentence in the alert tool text such as "when an alert fires and there is nothing to tell the person, end with reply(text="") / a `quiet` call: nothing is shown"; and the tool to do it. Also show the failure reason and keep the thinking of a failed turn.

3. **The watch never saw the file: an alert whose check goes through a child is not a check per fire.** (APP design + MODEL)
   Seen: 7 fires, 1 real check (17:56-17:57). The file made at 18:00:47 was not found by 18:13. The translator's replies between were filler, and one message to the child ("The alert fired again. Please do the check now") was not followed by any new `run` in the child's terminal record (UNVERIFIED why: the child was still in its first turn).
   Why it matters: the user is told "every minute I'll check", but one check took 11 minutes.
   What would help: a cheap read-only probe for the translator's alerts (file exists, command exit code) or an alert that calls a child directly; the alert's reply should say the real interval it can keep ("a check takes me several minutes on this model").

4. **Nothing marks a turn as started by an alert.** (APP, major)
   Seen: scenario 7. Role `user`, class `msg user`, "turn · user", `"trigger":"user"`, `"by":"user"`. A reader of the page sees a message they never wrote in their own bubble style.
   Why it matters: a person returning to the page cannot tell why the engine spoke, and analytics count alert turns as user turns (Home says "2 tasks" after one ask).
   Fix: `trigger: "alert:<name>"` in spans and events, a distinct bubble ("⚑ alert one-minute-ping fired, set 17:25, due 17:26") and a link to the alert.

5. **The engine has no clock, and alert texts carry no times.** (APP, major)
   Seen: "tell me the time" → "I don't have a clock tool in this session" (honest; the prompt from `host.sh prompt c translator` has no date or time anywhere, though the page prints "snapshot taken 2026-11-24 Tuesday 17:27"). The made-alert result says "made alert one-minute-ping: every 1m, once" with no due time, so the reply cannot say when it fires. The fire message says "Alert pong-timer fired (every 2m)" with no fire time, no due time and no lateness.
   Fix: put "now" on the environment line; return "next: 17:26:13" from the alert tool; write "fired 17:49:15, due 17:48:14 (61 s late: the host was down)" in the fire message.

6. **A late fire after a restart is silent.** (APP, minor)
   Seen: scenario 5. Good: the alert was not lost and fired 1-2 s after the host came back. Missing: any word that it was late. For "remind me at 09:00" firing at 11:00 this would mislead.

7. **A date in the past is accepted as a one-shot alert.** (APP, minor)
   Seen: `alert(name="past-date", trigger="at 2020-01-01 09:00")` → "made alert past-date: at 2020-01-01 09:00, once." The alerts list showed it as "not fired yet". Whether it ever fires is UNVERIFIED (the model removed it). It should be refused like `25:99`, or fire at once with a note.

8. **The alerts list shows no next time and no last time.** (APP, minor)
   Seen on `/desk/0/artifacts/alerts`: "- pong-timer (every 2m, once): ... · not fired yet" and "- watch-check (every 1m): ... · fired 6 time(s)". No "next at", no "last fired at", no remove button. The same text is what the model sees. Alerts are not shown on `/desk` or Home at all; a person must open Translator → Artifacts → alerts.

9. **After a one-shot fires, the model is not told it was removed.** (APP, polish)
   Seen on c3: the model called `alert(name="pong-timer", text="")` and got "error: no alert `pong-timer` to remove". One extra tool error per one-shot. Add "(one-shot: already removed)" to the fire message.

10. **Three wake-ups arrive as one `user` message.** (APP, polish)
    Seen (c messages.md lines 318-326): a child reply, "Guided posts for you: [thread 1] you → translator: ..." and "Alert watch-check fired ..." in one block under `### user`. The model handled it (it took the person's post first), but on the page it reads as one message from the person.

11. **The model often answers without an `act` line.** (MODEL; the app recovers)
    Seen 7 times (c 1, c2 1, c3 5): "Your answer did not follow the response contract (the answer has no act)". On c3 the PONG turn needed 4 steps for one word. The retry works; each retry cost 20-90 s here.

12. **Workspace is the app's root folder.** (setup or APP, UNVERIFIED which)
    The environment line and the children use `c/root` (routes/, themes/, models.json) as the workspace, not `c/proj` where the server was started. On c2 the developer child wrote `hello.txt` and `colors.md` into `c2/root`. A person asking for "a file in this workspace" gets it between the app's own files.

## 3. Usage feedback

Good:
- The one-shot worked end to end with no setup: placed in one step, fired 1.3 s after due, started a turn by itself, removed itself.
- Bad triggers get clear, specific errors (`25:99`, `every 10s`).
- An alert due during a turn or during a host outage is not lost and not doubled.
- Live thinking on the page while waiting is useful; it is how I saw why turn 2a was stuck.
- The translator was honest: no invented time, no invented file check.
- A thread post is accepted while the engine is busy, and the model gave it priority.

Confusing:
- A message I did not write appears in my bubble style.
- "Set — in one minute" but no time; "fired 6 time(s)" but no when.
- "✕ failed" with no reason after 10 minutes of visible thinking.
- The direct message box refuses (409) while a post on the Desk goes through. Two ways to talk, two behaviours.
- "Every minute" quietly became "when the last turn ends".

Missed feedback:
- A countdown or "next at" for each alert, on the engine page and the Desk.
- A sign on a running turn: "started by alert watch-check".
- A notice of skipped or late fires.

Five improvements:
1. Let the person in: queue or prioritise a person's message over alert fires, and add Pause/Remove buttons per alert that work without a model turn.
2. Mark alert turns: `trigger: alert:<name>` in spans/events, an alert-styled message, the due and actual times in its text.
3. Give alert turns a quiet ending (nothing to report) and say so in the tool text.
4. Do not fire an alert again while the work of its last fire is running; record skipped fires ("skipped 3: busy").
5. Give the engine the current time, and return the next due time from the alert tool so replies can say "at 17:26".

## 4. Numbers

- Hosts: 3. Person messages sent: c 4 (+1 thread post, +1 refused for 9 min), c2 2, c3 3.
- Translator turns on c: 11 or more (10 finished in spans at 18:12, 1 running). Steps per turn: 1, 1, 1 (failed), 3, 3, 1, 1, 1, 1, 1. LLM calls on c translator: 15.
- Turn lengths on c (s): 29, 45, 600 (timeout), 627, 296, 189, 171, 173, 79, 116.
- Alert timing: one-shot due +60 s, turn start +61.3 s (1.3 s late). Busy case: 25 s late (delivered at turn end). Host-down case: due 17:48:14, fired 17:49:15.6 (61 s late; 1-2 s after the host was back).
- `watch-check`: 7 fires in 18 min for `every 1m`; gap between turn end and next alert turn: 0 s every time.
- Tool errors: 3 (`at 25:99`, `every 10s`: both expected; removing an already removed one-shot).
- Response-contract retries: 7 (c 1, c2 1, c3 5).
- Failed turns: 1 (`timeout: global`, 600 s).
- 409s for the person on c: every try from 18:00:47 to 18:12 (about 300 tries at 2 s).
- `keep-going` fires: 0 on all hosts (never had 3 idle minutes with an open plan).
- Not run: more than 8 alerts; "what alerts do I have?" as a direct question; a past-date alert left to fire.
