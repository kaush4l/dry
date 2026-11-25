# Tester A: the default engine as a "super engine"

Theme: change the app's own properties by asking, create engines by asking, answer in a style.
Hosts: `a` (8111, main), extra `a2` (8121), `a3` (8122), `a4` (8123), all on model row omlx.
Paths below are relative to the `bb/` folder. Wall clock used: about 55 minutes.
The model queue was very slow (single calls of 1 to 10 minutes), so the scenarios ran in
parallel on four hosts and several never reached a final answer inside the time box. Those are
marked BLOCKED or UNVERIFIED; nothing below is guessed.

## 1. Scenarios

| # | Asked (verbatim) | Expected as a user | What happened | Verdict | Evidence |
|---|---|---|---|---|---|
| 1a | `switch to light mode` (host a, translator) | The app goes light within seconds; reply says so | 8 steps and 231 s later the only reply was "Brief sent to the researcher: find where light/dark mode is set or toggled in this workspace ... Waiting for its reply." It treated the app as an unknown code project and started child `research-light-mode`, which was still grepping the root folder (12 steps, about 18 min of model time) when I stopped. The app never changed: `data-mode="dark"`, `state/app.md` `mode: dark`. No false claim of success. | FAIL | `a/state/engines/translator/convo/messages.md` lines 1-260; `a/state/engines/research-light-mode/convo/messages.md`; `a/ev/base-home.html` vs live `GET /` |
| 1b | UI control: `POST /api/mode {"value":"light"}` (a2) | Light mode | `{"gen":1}` 200, page `data-mode="light"`, `state/app.md` `mode: light`. Instant. Bad value: 400 `no mode purple`. | PASS | `a2/ev/light-home.html` |
| 1c | UI control: `POST /api/theme {"value":"terminal"}` (a2) | Terminal theme | 200; `themes/active` = `terminal`, accent `#0A84FF` -> `#0A6B1F`/`#33FF66`, font Menlo, `class="nav-pinned"`. Bad value: 400 `no theme "nonexistent" (themes: abyss, bolt, cat, default, terminal)` (a good error). | PASS | `a2/ev/term-home.html`, `a2/ev/term-settings.html` |
| 1d | `use the terminal theme`, `make the accent colour orange` (by asking) | - | Not sent: host a's thread was still occupied by 1a's researcher. No tool exists for either (see finding 1). The UI has no one-call accent control that I found either (only `/settings/ui` and `/api/theme/reset`). | BLOCKED | `a/ev/prompt0.txt` |
| 2a | `add a page called Notes to the navigation` (a4) | A Notes link in the nav | 5 steps, 601 s to the first reply: "Starting research-nav to map how the navigation and pages work in this project before adding a Notes page..." Child `research-nav` still exploring at close. No `routes/notes`, nav unchanged. No false claim. | FAIL | `a4/state/engines/translator/convo/messages.md`; `ls a4/root/routes` |
| 2b | UI control: `POST /api/routes/create {"path":"/notes","title":"Notes","icon":"✎"}` (a2) | Notes page in nav | 200 `{"pattern":"/notes"}`; nav becomes Home, Desk, Notes, Analytics, Settings; `GET /notes` 200; `routes/notes/page.json` written. Instant. | PASS | a2 `GET /` nav, `a2/root/routes/notes/page.json` |
| 2c | UI control: `POST /api/routes/hide {"value":"/analytics"}` (a2) | Analytics leaves the nav | 200; link gone from the nav, `GET /analytics` still 200. The same call again puts it back (it is a toggle). | PASS | a2 `GET /` nav before/after |
| 2d | "put the Desk first" | Reordered nav | No ordering control exists: `POST /api/routes/order` -> 400 `no route action order: hide, off, delete or create`. By asking: not sent (time). | BLOCKED (no app feature found) | response quoted |
| 3a | `create an engine called poet whose goal is to write short poems` (a3) | A `poet` engine in status and on the Desk | Step 1: empty answer. Step 2: loaded the prompting skill. Step 3: the model reasoned for 600 s about which of four templates fits a poet ("Honestly none fits...") and the call died: status `failed: timeout: global`. No engine was made. Sent again; still in its first call (more than 5 min) at close. | FAIL | `a3/state/engines/translator/spans.jsonl` (turn `outcome: failed`, `error: timeout: global`); `a3/ev/busy-desk0.html` |
| 3b | UI control: `POST /api/engines {}` (a2) | A new engine | 201, `translator-2` ("Translator 2") in `/api/status` and on the Desk at once, and it answers messages. It is a second translator: no name or goal can be given. | PARTIAL | a2 `/api/status` |
| 3c | Second engine with another goal; talk to poet directly | - | Not reached. | BLOCKED | - |
| 4a | `What are the four largest planets in our solar system and their diameters? Show it as a table.` (a2) | A table within a turn | 5 steps, 601 s, then: "Brief sent to a deep-research child ... I'll judge and show you the table." The child `research-planet-diameters` then produced 7 empty answers in a row (7 contract errors, 0 tool calls, about 9 min) and was still looping at close. No table. | FAIL | `a2/state/engines/translator/convo/messages.md`; `a2/state/engines/research-planet-diameters/convo/messages.md` |
| 4b | `Before anything else, ask me which colour I prefer: red, green or blue. Just ask, I will pick.` (a2, translator-2) | A choice control | One step, 17 s: `reply(ask="choice", options="Red|Green|Blue", text="Which colour do you prefer?")`. Page HTML has a real control: `<div class="glass panel ask" data-ask="choice" data-engine="1" data-msg="1">` with three `<button ... data-act="ask" data-value="Red">`. | PASS | `a2/ev/ask-desk1.html` |
| 4c | Answer: `POST /api/engines/translator-2/answers {"message":1,"answer":"Green"}` | The engine gets "Green" | 202; convo shows user message `Green`; reply "Green it is. What would you like me to do?" (one step, 80 s). | PASS | `a2/state/engines/translator-2/convo/messages.md` |
| 4d | `Why is the sky blue? Answer as a haiku.` (a2, translator-2) | A haiku | Still in its first model call after more than 8 minutes at close. | BLOCKED (UNVERIFIED) | a2 `/api/status` |
| 4e | bullets / JSON only / 5-year-old; show= table/list/stat rendering | - | Not reached. Whether show= renders as a real table is UNVERIFIED (I only saw `data-show="stat"` on the Home page's own panel). | BLOCKED | - |
| 5 | `host.sh prompt a translator` | See the tools the model has | Tools given: reply, write, skill, alert, engine_call, engine_start, post, edit_post. Nothing for mode, theme, tokens, scene, routes or models. | - | `a/ev/prompt0.txt` |

Requests with **no tool at all** behind them: switch to light mode, use the terminal theme, accent
colour, add a page, hide a page, reorder pages. "Create an engine" has a tool (`engine_start` /
`engine_call`), but the goal text steers it to four work templates.

## 2. Findings

1. **The translator has no tool for any app property, and nothing tells it the app exists.**
   Seen: "switch to light mode" and "add a page called Notes" both became research projects on
   the workspace folder; neither changed the app; first feedback after 231 s and 601 s; the work
   was still running 20+ minutes later. Reproduce: `POST /api/engines/translator/messages
   {"text":"switch to light mode"}` on a fresh host. Why it matters: the owner's intent ("do
   things in the app with proper feedback") fails for the simplest request, while the same
   change is one instant call from the Settings page (`/api/mode`, `/api/theme`,
   `/api/routes/create|hide`). Severity: **blocker** for the theme. Cause: **APP**. The prompt
   says "You have no action tools" and "Work goes to a child made from a template"; it lists no
   app tool and never says that dry itself has modes, themes and pages. The model followed its
   instructions faithfully. A small `app(...)` tool (mode, theme, token, route create/hide,
   scene) or one "this app's settings" skill would let it do this in one step.

2. **Every factual question is pushed to a deep-research child, which then cannot work.**
   Seen: the planets/table question took 601 s to produce only a brief; the child then returned
   7 empty answers in a row. Reproduce: the 4a message on a fresh host. Why it matters: a
   one-line answer plus a `show="table"` was available in a single step. Severity: **major**.
   Cause: **APP** for the routing (goal text: "a question about the world that needs the web to
   deep-research"; "a question you can answer from this conversation alone you answer
   yourself" excludes general knowledge). The empty answers of the child are **MODEL**, but the
   loop that lets them repeat unseen is APP (finding 4). There is no "answer engine" and no
   style handling: the translator neither answered nor passed the table format on as a control.

3. **"Create an engine called poet" cannot be expressed cleanly, and the turn died.**
   Seen: 600 s of visible reasoning about which template fits ("researcher, deep-research,
   developer, reviewer ... Honestly none fits"), then `failed: timeout: global`. Reproduce: the
   3a message on a fresh host. Why it matters: making a named engine with a goal is the core
   act of a super engine. Severity: **major**. Split: the 10-minute rumination is **MODEL**;
   the dilemma is **APP**: the goal text says work goes to a child "made from a template" and
   the prompting skill's table lists only four, while the tool text quietly allows no template.
   One line ("a plain engine needs no template: engine_start(name, goal)") would settle it.

4. **Contract errors repeat with the same correction, and nobody is told.**
   Seen: "the answer has no act" 5 times in the 1a turn, 3 times in 4a, 2 times in 2a; "no
   `key: value` lines" 7 times in a row in the planets child; "has no thought" 4 times each in
   two researcher children. Each failed step re-sends the whole prompt (7k+ tokens after the
   skill) and gets the same one-line correction; the model then repeats the same thought almost
   word for word. Completion sizes were 67 to 138 tokens: the model stopped after the thought
   line. Why it matters: 18 of the 45 model calls I ran (40%) were wasted this way, and the user
   only sees "calling the LLM again (step N)". Severity: **major**. Cause: **MODEL** writes the
   bad answer; **APP** for the unchanging correction, for dropping the skill's 11k characters
   into the turn just before the step that fails most, and for no visible "N answers rejected"
   signal. Why the model stops after the thought is UNVERIFIED.

5. **A failed turn leaves no message in the conversation, and there is no stop control.**
   Seen on a3: after the timeout the chat stream ends with the skill text; the 10 minutes of
   reasoning shown live are gone; the failure is only a status label (`failed: timeout:
   global`) and "Need you 1" on Home. While busy, the page offers Send, Clear, Delete engine,
   but nothing to stop a turn, and the client script has no stop call. A message sent meanwhile
   gets 409 `engine is in a turn`. Severity: **major**. Cause: **APP**.

6. **The first reply is a brief, not an answer, and it arrives late.** The "reply at once
   with the brief" rule means the person waits minutes to learn only that work was handed off,
   in internal words (goal, scope, done, path:line). For "switch to light mode" that reads as
   a non-answer. Severity: minor. Cause: APP (prompt rule).

7. **The Desk shows rejected steps and whole skill texts in the chat.** The 1a stream is 57k
   characters: four near-identical thoughts, then the entire prompting skill. Severity: polish.
   Cause: APP.

8. **`POST /api/routes/hide` is a toggle with no state in the response** (`{"gen","pattern"}`
   both times). A caller cannot tell hidden from shown without reading the page. Severity:
   polish. Cause: APP.

9. **The environment line names the app's own root as the workspace** (`.../a/root`, though the
   server was started in `proj/`). The children therefore explore `themes/`, `routes/`. This is
   why a developer child might eventually edit `themes/active` by hand; whether that would take
   effect is UNVERIFIED. Severity: minor. Cause: APP (or the test setup).

What worked honestly: in no run did the translator claim a change it had not made.

## 3. Usage feedback

Felt good: the ask control (asked, real buttons, answer round trip in two short turns); live
status labels with step numbers; the live reasoning stream on the Desk; clear 400 errors from
the settings calls that list the valid values; children appear in the nav and status at once;
the Settings page states the current mode and theme in a sentence.

Confusing: asking the app to change itself starts a code investigation of the app's files;
"Brief sent to the researcher" as the answer to "switch to light mode"; a plain question
becoming notes.md, summary.md and lessons.md; "Translator 2" when I wanted a named engine.

Missed: any sign that answers were being rejected and retried; a stop button; an elapsed time
on the busy label; a message in the chat when a turn fails; a hint that Settings can already do
what I asked.

Five improvements:
1. Give the translator one app tool (mode, theme, one token, scene, route create/hide/order)
   that returns the before and after value, and name the app's properties in its prompt.
2. Let it answer general questions itself, with show= for the format; keep deep-research for
   "look this up" requests, and never start it when no web tool is present.
3. Say that a plain engine needs only a name and a goal; let "New thread" take a name and goal.
4. On a rejected answer: vary the correction (show the expected two lines with the tool the
   thought named), show "answer rejected, retry 3" in the status, and stop with a plain message
   after a few identical failures instead of looping.
5. Add a stop control, and write a visible chat line when a turn fails or times out.

## 4. Numbers

| host / engine | model calls | model seconds | tool calls (failed) | turns: steps, seconds, outcome | contract errors |
|---|---|---|---|---|---|
| a translator | 8 | 231 | 2 (0) | 8, 231, ready | 5 no act |
| a research-light-mode | 11+ | 1073+ | 11 (0) | unfinished at 12 steps | 4 no thought |
| a2 translator | 5 | 601 | 2 (0) | 5, 601, ready | 3 no act |
| a2 translator-2 | 2 (+1 running) | 98 | 0 | 1, 17, ready; 1, 80, ready | 0 |
| a2 research-planet-diameters | 7+ | 534+ | 0 | unfinished at 8 steps | 7 empty answers |
| a3 translator | 3 (+1 running) | 688 | 1 (0) | 3, 688, failed (timeout: global) | 1 empty answer |
| a4 translator | 5 | 601 | 2 (0) | 5, 601, ready | 2 no act |
| a4 research-nav | 4+ | 399+ | 10 (0) | unfinished at 5 steps | 4 no thought |

- User turns sent: 8. Finished: 5 (3 of them only a hand-off brief). Failed: 1. Still running at close: 2.
- Tool errors: 0 of 28 tool calls. Rejected answers (no act, or empty): 18 of 45 finished model calls (40%); 8 more ran but were warned for a missing thought.
- Single model call: 9 s to 600 s (the timeout). First token: 6 s to 116 s.
- App changes made by asking: 0 of 3. By the UI's own calls: 5 of 5, each under a second.
- Extra hosts a2, a3, a4 stopped at the end; host a left running.
- No text in any page, file or reply asked me to act outside the brief.
