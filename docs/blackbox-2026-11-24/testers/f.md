# Tester F: the UI in a real browser, and how dynamic it is

Host `f`, http://127.0.0.1:8116, model row `local`. Driven with the built-in browser tools (two tabs,
`seed` and `tab-1`), plus `curl` and hand edits under `bb/f/root/` for the "dynamic by definition" part.
No extra hosts were started. Wall clock about 45 minutes.

Tool limits that shaped the results:
- The test browser suppresses native `confirm()` dialogs (returns false). "Reset all" on UI tokens uses
  one, so I had to stub `confirm` to test it. Any other control behind a native confirm (Delete engine,
  route Delete, "Yes, reset history") is UNVERIFIED by click.
- Screenshots are a 1024x768 viewport; "mobile" is 375x812 emulation, "tablet" 768x1024.

State I left behind on host `f` (all inside `bb/f/`): pages `/notes` (with `overrides.md` and a local
component) and `/lab/deep`; theme `themes/tester.md`; Home has one extra `composer` part; scene set to
Off; `root/hello.py` written by the child engine. Backups of the shipped routes/themes are in `bb/f/bak/`.

## 1. Scenarios

| # | What I did (verbatim where typed) | Expected as a user | What happened | Verdict | Evidence |
|---|---|---|---|---|---|
| 1 | Opened `/` as a new user; walked Home, Desk, Engines, Analytics, Settings, Settings/UI tokens, Settings/Capabilities, Code, an artifact page | Know what this is and what to do first; no dead ends | Clean dashboard, no console errors, no failed requests. But the only explanation is "Home stays put; engines are the live part. Ask below to start a new one" and there is no composer on Home ("below" is a stat grid). Nav is hidden behind "Menu" on first paint at 1024 px. `/engines` silently redirects to `/desk`. "Voice: missing / not checked in this runtime" is a red top-level tile on Home | PARTIAL | `routes/page.json` (no composer part); screenshots of `/` |
| 2 | "Hi! What can you do for me here? Keep it short." typed in the composer, Enter | Reply streams, I can see what the engine is doing | Good. Within 2 s: header tint + "calling the LLM", nav item "Translator · LLM", then "Model · writing", a collapsed "thinking", `thought` text, and the reply streamed word by word with a caret. Timeline filled in at the end (29.2 s, first token 13.4 s) | PASS | screenshots at +2 s, +10 s, +20 s; `state/engines/translator/convo/messages.md` |
| 2b | "second message while busy", Enter, during the turn | Queue it, or a clear refusal | Text stayed in the box; hint changed from "running · send when it ends" to "engine is in a turn"; console logs a 409. After the turn ended the red "engine is in a turn" stayed on screen while the engine was idle, and nothing was sent | PARTIAL | console: `409 (Conflict)`; screenshot after turn end |
| 3 | "write a python script hello.py in the workspace that prints hello, run it and show me the output" | Child appears live, I can watch its loop, I get the output back | Child `dev-hello` appeared without reload in the nav LIVE list, the Home dashboard and the Desk tree, on both tabs, about 1 m 50 s after sending. `/desk/1` shows steps and tool rows live; `/desk/1/live` shows explorer, the opened `hello.py`, terminal sessions with exit codes and the conversation, all updating live. `hello.py` was written and run (`hello`). **But the translator was never woken with the child's reply**: both engines idle for 3+ minutes, the translator's last word on it is "Waiting for its report", and `artifacts/children.md` still says `child: dev-hello start 0`. I never got the output in the conversation I asked in | PARTIAL (live view PASS, round trip FAIL) | `/api/status` both `idle`, translator `messages:16`; `state/engines/dev-hello/convo/messages.md` last `reply(...)`; `state/engines/translator/artifacts/children.md` line 2 |
| 3b | Second tab on the same host | Stays in sync | Yes: nav, dashboard counters, engine labels, theme, mode and scene all followed within about a second with no reload | PASS | paired screenshots |
| 4 | "ask me which colour I prefer with a choice control, then tell me what I picked"; clicked Green | A clickable choice, then my pick echoed | Choice card with Red / Green / Blue; click collapsed it to "✓ Green", a bubble "Answer (choice): Green", reply "You picked Green." | PASS | `messages.md`; timeline row "wait · user ask · choice 17.2 s" |
| 4b | "Show me a small table of 3 fruits with their colours, and ask me with a form for my name and my favourite fruit"; filled Sam / banana, Submit | Table and form rendered, answers go back | Table (3 rows) and a form (text field + choice) in one reply; after Submit the card shows "✓ submitted / Name: Sam / Favourite fruit: banana"; reply "Sam — banana, it is." | PASS | `messages.md` lines 251-262 |
| 5 | Light / Dark, each theme (abyss, bolt, cat, default, terminal, my own `tester`), Reset all | Everything restyles at once, on all tabs | Yes, instantly on both tabs, including fonts, radii, pinned-nav layout (bolt, terminal) and the cat's colours. "Reset all" put my hand-edited token back in the file | PASS | screenshots per theme; `themes/default.md` accent row before/after |
| 5b | `resize_window` mobile and tablet | Usable | Tablet fine. Mobile: no horizontal scroll, but the menu was open over the content on every page load, the composer's bottom row overflows while busy, and 13 of 27 controls on the engine page are under 32 px | PARTIAL | mobile screenshots of `/`, `/desk`, `/desk/0` |
| 6a | Settings "New page" (`/notes`, "My Notes", ✎) and `POST /api/routes/create {"path":"/lab/deep",...}` | Page exists at once | Both appeared in the nav at once, no restart; Settings form navigates to the new page. Duplicate path → 400 `/notes already exists`; no title → 400 `a page needs a title` | PASS | `routes/notes/page.json`, `routes/lab/deep/page.json` |
| 6b | Hand-edited `page.json`: added/reordered/removed parts, changed title/icon/order | Open page follows | The open page changed within about 2 s with no reload: title, breadcrumb, nav icon, nav order, body. `/events` showed `event: change` | PASS | screenshot "Home-X"; `bak/events.txt` |
| 6c | Edited the `accent` dark value in `themes/default.md` | Restyle | Live on both tabs in about 2 s (buttons, bars, focus ring, even the cat turned red). UI tokens page marked the changed row | PASS | screenshot |
| 6d | Added `themes/tester.md` | Shows in the picker | Appeared as a "tester" button after a reload of Settings and worked | PASS | screenshot |
| 6e | Unknown part type; known types with a missing field; broken JSON | A clear error where I am looking | Unknown type: a dashed placeholder naming the type, rest of the page still renders. `link` without `to` and `text` without `text` give the same "this build does not know this component" (wrong reason). Broken JSON: the whole page becomes "Not found: Nothing is registered for /", disappears from the nav, and the parse error is only on Settings → Routes | PARTIAL | page HTML; Settings text `routes//page.json: expected , or } at line 1 column 49` |
| 6f | `POST /api/routes/create {"path":"../evil","title":"E","icon":"x"}` | 400 | 200 `{"pattern":"/../evil"}` and it wrote `root/evil/page.json`, outside `routes/` | FAIL | file existed at `bb/f/root/evil/page.json` (I removed it) |
| 7 | Keyboard, labels, text sizes, scene | Basics hold | Focus ring is a clear 2 px accent outline; Enter sends, Shift+Enter makes a line, the textarea grows and long words wrap; a draft survives live updates. Gaps listed in findings 12-14 | PARTIAL | JS probes quoted in findings |

## 2. Findings (most severe first)

1. **A child's finished reply never reached the translator; the person never got the answer.** APP (loop), major.
   Seen: `dev-hello` replied ("Changed: hello.py (new) … Saw: hello") and went idle. The translator stayed idle
   with "Waiting for its report" as its last statement; `children.md` still reads `child: dev-hello start 0`;
   `grep -c "Saw: hello" translator/convo/messages.md` = 0 three minutes later. The translator was inside
   another turn (my form answer) when the child replied, which is my best guess for why the wake was lost
   (UNVERIFIED as the cause). Repro: send the hello.py request, then while the child runs send two more
   requests to the translator so it is busy when the child ends. Why it matters: the whole promise of the
   super engine is "report back". The UI makes it worse: nothing on the translator page says the child is done.
2. **`/api/routes/create` accepts `..` and writes outside `routes/`.** APP, major (security).
   Repro: `POST /api/routes/create {"path":"../evil","title":"E","icon":"x"}` → 200, file `root/evil/page.json`.
   I did not probe further up. The Code page also runs any shell command on the host from a web form with no
   check; fine on localhost, worth a thought before the "hosted" version.
3. **The Desk, the page the nav calls the main place, shows none of the work.** APP (UI), major.
   With two engines busy and 3 turns done, `/desk` reads "No posts yet. Write below: it goes to translator, and
   its work shows here as it happens" over an empty page. The thread tree (you → Translator → dev-hello with
   live labels) is good, but the loop, the replies and the child's steps are only on `/desk/<n>` and
   `/desk/<n>/live`. A new user lands on an empty Desk and concludes nothing is happening.
4. **No auto-scroll, and the sticky composer covers new content.** APP (UI), major for chat.
   The streamed reply, the choice card and "You picked Green." all landed underneath the composer; I had to
   scroll to find each one. Repro: any turn once the session is taller than the viewport.
5. **Waiting 60-120 s shows only "◐ calling the LLM".** APP (feedback), major on a slow model.
   No elapsed time, no "waiting for first token", no token counter; the Timeline only fills in when a call
   ends (it showed the previous turn's bars for the whole of the next turn). First token p50 here was 1 m 06 s,
   so most of the time the page looks frozen. A running clock per step would fix the feel.
6. **Home tells you to "Ask below" and has nothing to ask with.** APP (shipped data), major for first run.
   Adding `{"type":"composer"}` to `routes/page.json` gives a working "Ask to start a new engine" box, so it is
   a one-line data fix. Also on Home: the "Voice: missing" tile in red is the loudest thing on a fresh install.
7. **Busy-send feedback is contradictory and goes stale.** APP (UI), minor.
   Hint says "running · send when it ends" (reads as "I will send it for you"), Enter gives "engine is in a
   turn", which then stays red after the turn ends. Either queue the message or say "wait, then press Send"
   and clear the error on the turn-end event. The Send button stays blue and enabled while it cannot send.
8. **Broken page file = silent 404.** APP, minor. A JSON typo in `page.json` removes the page from the nav and
   shows "Nothing is registered for /". The precise error (file, line, column) exists but only on Settings →
   Routes. Show it on the page itself and keep the last good version. Same wording appears when a `session` or
   `artifacts` part is put on a page with no engine ("Not found: Nothing is registered for /."), which is the
   wrong message for "this part needs an engine".
9. **Wrong reason for a part with a missing field.** APP, minor. `{"type":"link","label":"x"}` and
   `{"type":"text"}` render "this build does not know this component; update dry-core…". They are known
   components missing `to` / `text`. An unknown `part` (`analytics` + `part:"bogus"`) gets the same text.
10. **Live update = refetch the whole page.** APP (performance), minor now. One 30 s turn caused 77
    `GET /desk/0` requests of about 39 KB each (every stream event refetches the full HTML). It works and
    keeps drafts and focus, but it will hurt on a phone over a tunnel and with long sessions.
11. **Layout breaks.** APP (UI), minor.
    - The composer's bottom row re-flows when busy: Send jumps from the right edge to the middle (desktop) and
      is cut off the right edge on mobile (`/desk`).
    - The menu's open state is one `localStorage` flag shared by every width, so after opening it on desktop
      it covers the content on every mobile page load, and navigating does not close it.
    - Live view with the menu open at 1024 px: editor and terminal columns shrink to about 115 px and the
      status bar is clipped at both ends ("ello … En").
    - `panes` parts have no padding and text does not wrap inside a narrow pane (horizontal scrollbar).
    - `columns` needs the menu closed at 1024 px to become two columns; otherwise the side stack sits under
      the composer, a long way down.
    - Terminal output wraps on spaces, so `ls -la` columns fall apart in the narrow panel.
12. **The scene sits on top of controls.** APP (UI), minor. The cat walked over the composer hint, "Detect"
    and "Test" buttons in several screenshots. Settings → Scene → Off removes it on all tabs at once, and
    CSS has a `prefers-reduced-motion` rule, so it can be turned off; it should not overlap inputs.
13. **Accessibility basics.** APP, minor.
    - Selected Light/Dark, theme and scene buttons have no `aria-pressed`; the nav has no `aria-current`.
    - No headings on Settings at all (section titles are plain divs); no skip link.
    - Form control from the engine: the text field has no visible border in dark mode; a picked option in a
      form's choice shows no selected state before Submit.
    - 45 text nodes on Settings are 11 px; `text_sm` is 11 px in every theme.
    - Code page activity bar is four glyph-only links (❐ ⌂ ◐ ⚙).
    - Good: every button had a name, inputs in "New page" have labels, focus ring is visible.
14. **Small inconsistencies.** APP, polish.
    - Same engine, same card: "ready" top right and "idle" in the Session header; the nav dot stays accent
      blue when idle.
    - Timeline says "The last 3 turn(s)" when the session says "1 turn"; step label said "step 2" on the
      third model call.
    - Analytics: "Tasks 1 · 29.2 s in all · 1 turns" beside "Work 49.4 s" mid-turn.
    - Recent artifacts row: "Translator · child: dev-hello start 0" is store syntax, not a sentence.
    - Tool rows are cut with "…" and I found no way to expand them on `/desk/<n>`.
    - "Reset history" is the first and widest control on Settings; "Refresh" is the primary button on UI tokens.
    - UI tokens are read-only in the browser ("editing one token is the desktop app's for now").
    - Settings → Environment says the workspace is `…/bb/f/root`, so the child wrote `hello.py` into the
      app's own definition folder, next to `routes/` and `themes/`. `bb/f/proj/` stayed empty. May be how
      this test host was started (UNVERIFIED), but an engine with a terminal sharing a folder with the UI
      definition is risky.
15. **MODEL:** `dev-hello` left out `thought` on every step ("did not follow the contract: the answer has no
    thought", 7 times) and used 7 model calls and three phases for a one-line script. The app told it each
    time; it did not adapt. The app's nag is fine; the per-step cost of the phase template for a trivial ask
    is the app's design choice.

### How dynamic is it really (scenario 6)

More than I expected. With no restart and no code I could: create pages (form and API), reorder the nav,
change title/icon, compose a page from parts, nest layouts, define a reusable component (global in
`components/`, or local in `routes/<page>/components/`), restyle one page only (`overrides.md`), edit a
token and add a theme. The open page followed file edits in about 2 seconds.

Part types found in the shipped files, and what each needed:

| Part | Fields | Notes |
|---|---|---|
| `heading`, `text` | `text` | HTML is escaped; missing `text` → "unknown component" |
| `link` | `label`, `to` | drawn as a button; external URLs allowed |
| `use` | `name` | pulls `components/<name>.json` or the page's own `components/`; missing → "No component "x" in components/." |
| `columns` | `main[]`, `side[]` | two columns only when wide |
| `panes` | `axis`, `resizable`, `panes[]` each `size`, `min`, `background`, `text`, `border`, `edge`, `view[]` | token names as colours |
| `dashboard` | none | stat tiles + engines + recent artifacts, as one block |
| `analytics` | `part`: `today` / `page` / `timeline` | `timeline` needs an engine page |
| `desk` | none | thread tree + thread + composer, as one block |
| `composer` | none | "new engine" on a plain page, the engine's on `/desk/:id` |
| `session`, `engine-header`, `artifacts`, `engine-tools`, `prompt`, `response`, `metadata` | none | only on `/desk/:id`; elsewhere "Not found" |
| `artifact-page`, `live-view` (`part`: explorer / editor / panel / chat / status) | | engine or artifact routes |
| `explorer`, `editor`, `terminal`, `theme-picker` | none | the Code page's parts; `theme-picker` works anywhere |
| `appearance`, `models`, `routes`, `new-route`, `environment`, `diagnostics`, `tools`, `tokens`, `capabilities`, `reset-history` | none | Settings blocks, each works on any page |

What is hard-wired (on screen, in no file I could find): the frame (Menu button, breadcrumb, "host" chip,
footer "Saved by the host · live"), the nav's LIVE section and "New thread", everything inside the big
blocks (`dashboard` tiles including "Voice", `desk`, `session` step/tool rows, the ask controls, the
Timeline), all wording inside them, and the 404 page. So the page is a list of parts from data, but each part
is a sealed block: I cannot drop one tile from the dashboard, show only "Working", bind a tile to a value, or
change how a tool row looks. There is no generic "card", "stat", "list", "table", "markdown" or "button that
calls an action" part, and `text` is one plain paragraph. That is the gap between "pages are data" (true)
and "define components and behaviour as data" (not yet).

## 3. Usage feedback

Felt good: streaming and the activity words; the child showing up everywhere at once; the Live view (file,
terminal, conversation in one place is the best screen in the app); ask controls (choice, form, table all
worked first time and read well afterwards); themes switching instantly across tabs; file edits appearing
live; the Timeline bars once a turn ends; honest labels like "est" and "window assumed".

Confusing: where to type on first run; Desk being empty while work runs; "ready" vs "idle"; "send when it
ends"; whether the child was done and whether the translator knew; walls of 11 px engineer text (Prompt,
Response, Tools, Metadata) on the main chat page at the same weight as the chat.

Feedback I missed: elapsed time while waiting; "your child finished" on the parent; a scroll-to-latest; a
toast when a page file fails to parse; a selected state in form choices.

Improvements, ranked (structure first, then looks):

1. Wake the parent when a child replies even if the parent is mid-turn, and show the child's state inline in
   the parent's session (a live child card: name, phase, step, last tool, elapsed, "open live").
2. Make the Desk the live board: the selected thread shows the session and each child's loop as nested,
   collapsible rows that update in place. No empty state while anything is busy.
3. Split the big blocks into small data-bound parts: `stat` (label + named value), `list` (of engines,
   artifacts, posts), `card`, `markdown`, `table`, `action` (button → API call). Rebuild `dashboard` and
   `desk` from them in the shipped `page.json` so the default is the example.
4. Pin the chat to the bottom while streaming (unless the person scrolled up), and give the session its own
   scroll area so the composer never covers content.
5. A running clock on every busy label ("calling the LLM · 0:42 · waiting for first token"), and draw the
   current step in the Timeline as a growing bar.
6. Put the composer on Home in the shipped file, with two or three example asks as chips; move "Voice" out
   of the top tiles.
7. Queue a message sent while busy (show it as "queued", cancellable), or disable Send with one clear line.
8. Show page-file errors on the page (file, line, column, last good version kept); validate parts and say
   "link needs `to`".
9. Reject `..` in route paths; keep route writes inside `routes/`.
10. Send patches over `/events` (or swap only the changed part) instead of refetching the page.
11. On the engine page, default to chat + Timeline; fold Tools, Prompt, Response, Metadata into one
    "Inspect" drawer. Raise `text_sm` to 12 and body to 14 on the chat.
12. Keep the scene out of input areas (floor above the footer but behind the composer), and close the menu
    on navigation at phone width with its own per-width memory.
13. `aria-pressed` / `aria-current`, real headings, a visible border on form inputs, selected state on
    choices, 40 px touch targets on mobile.
14. One status vocabulary (idle / working / needs you / done) used in the nav dot, header and session.

## 4. Numbers

- Turns: translator 6 (4 messages I typed + 2 answers to ask controls), dev-hello 1. The Analytics page read
  "6 turns, 4 tasks" shortly before the last turn ended.
- Model calls: 14 total (translator 7, dev-hello 7). Tokens in → out about 79.9k → 1050 (estimated by the app).
- Steps per turn: translator 1, 3, 1, 1, 1, 1 (8 calls by that count against 7 in my tally of the
  store file, so one of these is off by one: UNVERIFIED); dev-hello 7.
- Tool calls: 11; errors 1 (`read(name="hello.py")` → "no artifact hello.py", the model's wrong tool).
- Time: work 17 m 55 s, waiting on me 17.2 s. First token p50 1 m 06 s; first plain reply 29.2 s; child
  appeared 1 m 50 s after the request; child turn 9 m 59 s for a one-line script.
- Live update cost: 77 full-page GETs (≈39 KB each) during one 30 s turn.
- File edit → open page updated: about 2 s. Theme/mode click → second tab updated: under 1 s.
- Console errors: one 409 (my busy send) and one 404 on `/settings/ui` whose URL I could not recover
  (UNVERIFIED which resource). No failed requests otherwise.
- Mobile: 13 of 27 interactive elements on `/desk/0` smaller than 32 px in one dimension; no horizontal
  page scroll.
