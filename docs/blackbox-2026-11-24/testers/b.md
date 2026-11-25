# Tester B report: sub-engines, parallel calls, shared context, sessions/threads

Hosts used: `b` (8112, main), `b2` (8125, threads / routing / failure), `b3` (8126, parallel asked
naturally), `b4` (8127, parallel asked explicitly). All on model row `local`. Paths below are
relative to the `bb/` folder. Wall clock: 17:24 to about 18:08.

The model queue was very slow (average 165 s per model call over 57 calls, see Numbers). Because of
that, only two children finished a turn inside the time box. Everything that needed a finished
child and a judged final report is marked BLOCKED or PARTIAL, and says how far it got.

One slip on my side: one `grep -c` used the glob `*/state/...` and so printed match counts from
other testers' folders (a, c, e). I read only the counts, used none of them, and narrowed the glob
to `b*` afterwards.

## 1. Scenarios

| # | Asked (verbatim, short) | Expected as a user | What happened | Verdict | Evidence |
|---|---|---|---|---|---|
| 1 | "write a python script fizz.py in the workspace that prints FizzBuzz to 30, run it and show me the last lines" | A child appears, I can watch its loop, I get the last lines in a few minutes | Child `dev-fizzbuzz` appeared in `/api/status` 32 s after the ask (translator step 3). Home, Desk, nav and the child's page all showed it with a live step label and streamed thinking. First reply showed the brief. The file was written correctly at step 5 (17:31). The child then spent 7 more steps trying to verify it and was killed at 36 min: translator got `engine dev-fizzbuzz failed: timeout: global (engines children)`. No result reached me in 44 min. | FAIL (visibility part PASS) | `b/state/engines/translator/convo/messages.md` tail; `b/state/engines/dev-fizzbuzz/convo/messages.md:96-121`; `b/root/fizz.py` (runs, last lines 28 / 29 / FizzBuzz) |
| 2a | "In the folder themes in the workspace, how many theme files are there and what are they called? Just read, don't change anything." | A read-only child | `researcher` child `list-themes`, scope "Read and run only". Found 5 theme files + `active` pointer by step 5; not replied after 27 min | PARTIAL (routing PASS, no answer in time) | `b2/state/engines/translator-2/convo/messages.md` (`act: engine_start(name="list-themes", template="researcher"`) |
| 2b | fizz.py ask (scenario 1), three-scripts asks (scenario 3) | A builder child | `developer` each time | PASS (routing) | translator convo files on b, b3, b4 |
| 2c | "What is the capital of Australia, and roughly how many people live there?" | A direct answer | Answered itself, no child: "Canberra ... ~450k; Australia ~26-27 million", flagged as not verified. Took 2 model calls because the first had no `act` | PASS | `b2/state/engines/translator/convo/messages.md` (engine since deleted in scenario 6; text quoted in my log) |
| 3a | "Please make three tiny python scripts, each in its own folder: one/hello.py ... two/count.py ... three/date.py ... Run each and tell me what each printed." | Three children at once, a plan with three lines | ONE developer child `dev-tiny-scripts` for all three, no plan artifact. Files existed and ran after ~20 min; child still verifying at 29 min when I stopped the host | FAIL for parallelism (work itself was done) | `b3/state/engines/translator/convo/messages.md` thought: "Small new thing, one developer, no plan needed"; `b3/root/one/hello.py` etc. |
| 3b | "I have three independent small jobs; please give each one to its own separate child so they run at the same time: (1) in folder alpha, a script hello.py ... (2) beta count.py ... (3) gamma upper.py ..." | Three children busy together, plan with 3 lines, ticked, 3 results judged | Plan artifact written with three `- [ ]` lines and three `engine_start` in one act; status showed `dev-alpha`, `dev-beta`, `dev-gamma` all busy together from 17:44. None lost or duplicated. None finished in 24 min, so no ticks, no judging seen. Files landed in the wrong place: `alpha/alpha/hello.py`, `beta/beta/count.py`, `gamma/gamma/upper.py` | PARTIAL | `b4/state/engines/translator/artifacts/plan.md`; `find b4/root/alpha b4/root/beta b4/root/gamma -type f` |
| 4 | POST `/api/threads/translator/posts` `{"text":"One more thing for the fizz script: please add a final line that prints the word done after 30.","to":"dev-fizzbuzz"}` while the child was busy; and a post with no `to` | Child sees it and knows it is from me; unaddressed post goes to the head | With `to`: `{"busy":true,"line":"posted entry 1 ...","to":"dev-fizzbuzz"}`; Desk showed "queued · it hears this when it is free"; the child's `shared` artifact showed `1. 11-24 17:32 you → dev-fizzbuzz: ...` (author and addressee both named); delivered 29 min later, only after the turn died, as `Guided posts for you: [thread 1] you → dev-fizzbuzz: ...`. Without `to`: `"not delivered: no addressee"`, nobody called. "Ask the translator to tell a child something": not run (translator was never free with a live child) | PARTIAL | `/desk/1/artifacts/shared` page on b; `b/state/engines/dev-fizzbuzz/convo/messages.md` tail |
| 5 | POST message to busy child; later "Quick follow-up from me directly: what exit code did frobnicate give, in one line? Don't rerun anything." to finished child `check-file-cmd` | Busy: queued or refused clearly. Finished: answers from memory, parent stays sane | Busy child: `409 {"error":"engine is in a turn"}` (not queued, unlike a thread post). Finished child: accepted `202`, turn `check-file-cmd/12` started at 18:03:28. Outcome: answered from memory, see Addendum; side effect on the parent, finding 3 | PARTIAL | status lines in my log |
| 6 | POST `/api/engines {}`; messages to `new`; "Remember this for me: my favourite colour is teal and my dog is called Biscuit." in one thread, "What did I tell you about my dog and my favourite colour? If you don't know, say so." in another | Separate threads, no leak, delete works | New threads `translator-2`, `translator-3`, `translator-4` (201 / 202). No leak: second thread said "I don't know — you haven't told me anything about your dog or your favourite colour in this conversation". Delete of idle thread: 200, gone from status, nav and Desk. UI offers "＋ New thread" (nav and Desk); the words "New session" appear nowhere on this host. Problems found on delete, see findings 5 and 6 | PASS (isolation), PARTIAL (delete) | `b2/state/engines/translator-2/convo/messages.md`; status after delete |
| 7 | "Please show me the first 5 lines of the file /etc/dry-does-not-exist/config.yaml, and also run the command 'frobnicate --version' and tell me its output." | The failure comes back in words | `researcher` child `check-file-cmd` ran both, and replied after 26 min: "The file cannot be read — /etc/dry-does-not-exist/config.yaml does not exist ... frobnicate is not installed ... exited 1". The reply reached translator-3 as a message, which started judging. Final words to the person: see Addendum | PARTIAL (failure surfaced to the translator; not hung) | `b2/state/engines/translator-3/convo/messages.md` tail |

## 2. Findings (most severe first)

1. **A child's command output is cut to the last 15 lines, and the note about it is wrong. This
   made FizzBuzz unverifiable and cost the whole turn.** (major, APP)
   Seen: `python.run(file="fizz.py")` printed 30 lines; the tool result the child got was
   `… earlier lines not shown (the terminal artifact shows the last 30)` followed by lines 16 to 30
   only (`b/state/engines/dev-fizzbuzz/convo/messages.md:102-118`). The terminal artifact snapshot
   holds only the last session's output, so lines 1 to 15 were never shown to the model anywhere.
   The on-disk log `b/state/engines/dev-fizzbuzz/artifacts/terminal/3.log` has all 30. The child
   then ran `python3 fizz.py | wc -l; python3 fizz.py` (count pushed out of view again), then
   `python3 fizz.py | wc -l`, then kept reasoning about how to read its own output until the timeout.
   Reproduce: scenario 1. Why it matters: the brief's `done` said "prints exactly 30 lines"; the
   app gave the child no way to see 30 lines. The model's behaviour (not claiming what it could not
   see) was what its instructions asked for. Fix on the app side: show at least what the note
   promises, say the true number shown, and give a tool to read a session's log by line range.

2. **A child that is killed by the timeout loses 36 minutes of work, and the person is told
   nothing while it happens.** (major, APP)
   Seen: at 18:01 the translator received `engine dev-fizzbuzz failed: timeout: global (engines
   children)`. The child had the correct file and a ticked plan since 17:31, but no report was
   salvaged: the message carries no partial result, no last step, no path. The limit itself is
   shown nowhere (child page, children artifact, status). UNVERIFIED: the exact limit; the child's
   model time summed to 2167 s when it fired. Good part: it did not hang silently, the parent was
   woken. Final words to the person: see Addendum. Fix: put the last thought, the plan and the
   changed files in the failure message, and show the remaining budget on the child's page.

3. **A direct follow-up to a finished child replaces its report in the parent's view, while the
   parent is judging it.** (major, APP)
   Seen on b2: `check-file-cmd` replied with its 1144 character report; translator-3 was woken with
   "engine check-file-cmd replied: ... (1144 characters; all of it is in the children artifact)" and
   began judging. I then sent the child a direct question. The child answered me well and from
   memory (see scenario 5), but translator-3's children artifact now read
   `check-file-cmd — idle · 2 replies ... last reply: exit 127 (frobnicate: command not found ...)`:
   only the last reply is kept there, so the report the parent was told to read "all of" was gone
   from that place (a 160 character preview stays in its conversation). The parent got no second
   wake-up for the reply to me (one "replied" message in its conversation). Reproduce: let a child
   reply, then POST a message to the child before the parent has finished judging.
   UNVERIFIED: what translator-3 finally told the person (not done when I stopped the host).
   Fix: keep each reply with who it answered; show the parent the reply to its own brief.

4. **While waiting for a child the translator shows `idle`, and the thread says "No posts yet".**
   (major, APP, feedback)
   Seen: `/api/status` gave `translator: kind idle, label idle, busy false` for 34 minutes while
   its child worked. Home counted it out of "Working". Desk's "Translator's thread" showed `0` and
   "No posts yet. Write below: it goes to translator, and its work shows here as it happens" although
   I had asked, the translator had replied with a brief and a child was running; the work never
   showed there, only my own post did later. A person looking at the Desk cannot tell "waiting for
   dev-fizzbuzz, step 9, 21 min" from "nothing going on". The step label ("calling the LLM again
   (step 12)") also stayed unchanged for 10 minutes with no elapsed time. Reproduce: scenario 1,
   `GET /desk`. Fix: a `waiting on <child>` kind, elapsed time on the label, and the ask, the brief
   and child start/finish as thread entries.

5. **`/desk/<n>` addresses change owner when an engine is deleted.** (major, APP)
   Seen on b2: indices were translator 0, translator-2 1, translator-3 2, list-themes 3,
   check-file-cmd 4. After `POST /api/engines/translator/delete {}` they became translator-2 0,
   translator-3 1, list-themes 2, check-file-cmd 3; `/desk/0` now opened "Translator 2", and `/desk/3`
   returned 200 for a different engine than before. An open tab, a bookmark or the composer
   (`data-engine="1"`) can point at another engine after a delete. Also `translator-4` was created
   with `"index":5` while status listed it fifth (index 4 expected). Fix: address pages by id.

6. **The default translator can be deleted with one call, and nothing brings it back.**
   (major, APP) Seen: `POST /api/engines/translator/delete {}` → `200`; afterwards
   `POST /api/engines/translator/messages` → `404 no engine translator`. Its two child-less
   siblings stayed. UNVERIFIED: whether a restart restores it, and whether the UI button asks first
   (the page has a `Delete engine` button; I did not drive a browser).

7. **No way to stop a running engine.** (major, APP) Seen: delete of a busy child →
   `409 engine is in a turn`; the child's page offers Send, Clear, Delete engine, Reload spec,
   Reload responses.md, format buttons, and no Stop. With steps of several minutes and a 36 minute
   timeout, a child going the wrong way cannot be halted. UNVERIFIED: a stop route may exist that
   no page links to.

8. **Children write into the app's own root folder, not `proj/`.** (major, APP or host setup)
   Seen: the translator's environment line names `.../bb/b/root` as the workspace, so `fizz.py` and
   `__pycache__` landed in `b/root` next to `models.json`, `routes/`, `default/`; `b/proj` stayed
   empty though the server was started there. A "workspace" that is also the folder holding the
   app's routes, themes and shipped spec invites a child to edit them. If this is only how the
   test host was started, it is a setup note; the app still gives no warning.

9. **Two ways to write to a busy child behave differently.** (minor, APP) A message
   (`/api/engines/dev-fizzbuzz/messages`) is refused with 409; a thread post with `to` is accepted
   and queued. The post was only read after the turn ended (29 min later, and only because the turn
   died), although the child's prompt says of the thread "Everyone sees it as it is now". So a
   correction sent mid-work cannot reach the child while it matters.

10. **Three deliverables in one message went to one child, with no plan.** (minor, MODEL, helped
   by the prompt) Seen on b3. The translator's instructions say "A goal with several deliverables
   gets a plan before any child" and "start together", but the prompting skill it must load first
   says "A small new thing (a script, one page ...) is the same: one developer", and the model
   followed the skill. When asked explicitly (b4) it did plan and start three. Fix in the prompt:
   say which rule wins when the deliverables are small.

11. **Parallel children wrote to `alpha/alpha/hello.py`.** (minor, MODEL, helped by the app)
    Seen on b4. The brief set `root="alpha"` and also said "Create only alpha/hello.py (create the
    folder if needed)"; the child, whose root already was `alpha`, obeyed literally. The person asked
    for `alpha/hello.py`. What would have helped: the prompting skill's root row could say "paths in
    goal, scope and done are relative to root", and the child's prompt could state its absolute root.

12. **Replies with no `act` cost a whole model call each.** (minor, MODEL; cost shaped by APP)
    Seen 8 times in 61 model calls on my hosts ("Your answer did not follow the response contract
    (the answer has no act)"): dev-fizzbuzz 3, b3 translator 2, and one each on b2 translator,
    check-file-cmd, dev-beta. Each time the thought was complete and only the call line was
    missing; at 2 to 4 minutes a call this was roughly 20 minutes of my waiting.

13. **A person's post with no `to` goes to nobody, while the Desk says it "goes to translator".**
    (polish, APP) API result: `"not delivered: no addressee"`. The Desk form has a `to` select
    that defaults to the head, so the page and the API disagree on the default. The child's own
    prompt says "unaddressed posts are for [the head]", a third version.

14. **Small page texts that do not match what is there.** (polish, APP) Home says "Ask below to
    start a new one" but Home has no input box (no `textarea` in the page). The translator's
    Timeline says "The last 3 turn(s)" for one turn of three steps. Analytics shows "Waiting 0 ms"
    after the translator had waited half an hour for a child. `/engines` answers 302 to another page.

15. **The children artifact shows only a child's goal.** (polish, APP) Scope, done and text of
    the brief are not listed there, though the translator is told to judge the reply "against the
    done criteria you wrote" (they are still in its conversation).

## 3. Usage feedback

What felt good
- A child shows up everywhere at once: nav "LIVE" list, Home tiles, Desk thread members, its own
  page. Its page is rich: the goal, each thought, each tool call with result, and the model's
  thinking streaming live. This is the best part of the app for the owner's intent.
- The first reply always showed the brief (goal, scope, done), in every thread, as promised.
- Template choice was sensible every time: researcher for reading and for running a doubtful
  command, developer for building, itself for general knowledge.
- Threads are really separate; no leak between them. The second thread said plainly it did not know.
- The post on the thread showed "queued · it hears this when it is free", and the child's view of
  it named author and addressee.
- A failing command came back as a clear, honest answer from the child, and a dead child woke its
  parent instead of hanging.
- Analytics (work, model calls, tokens, first-token time) is clear and useful.

What was confusing
- "idle" for a translator that is waiting on its child; "No posts yet" on a thread with work on it.
- Which folder is "the workspace".
- Why a child at "step 12" has not moved for ten minutes, and that a hidden limit was about to end it.
- Post versus message: two composers (thread, engine) with different rules when the engine is busy.

Feedback and progress I missed
- Elapsed time per step and per turn, and the time left before the timeout.
- One line per child on the parent's page: phase (triage / code / test), step, last tool.
- A notice to the person when a child fails, and what it had finished.
- The plan artifact on the Desk thread, with its ticks, as the place to follow a multi-child goal.

Five concrete improvements
1. Show the full tail the note promises for command output, with the true line count, and give
   children a way to read an older part of a session log.
2. Add a `waiting on <child>` state with elapsed time, and write the ask, the brief, child start,
   child reply and failure as entries on the thread.
3. On timeout, hand the parent the child's last thought, plan and changed files, and show the
   budget beforehand; add a Stop button.
4. Address engine pages by id, and protect the default translator from delete (or recreate it).
5. Let a post to a busy child be read at its next step, not only when the turn ends; make one
   rule for unaddressed posts and say it the same way in the page, the API and the prompts.

## 4. Numbers

- Person turns sent: 9 (b: 1; b2: 6 across four threads, one of them a direct message to a child;
  b3: 1; b4: 1) plus 2 thread posts.
- Engines made: 4 extra threads (1 deleted, default translator also deleted on b2), 7 children
  (dev-fizzbuzz, list-themes, check-file-cmd, dev-tiny-scripts, dev-alpha, dev-beta, dev-gamma).
- Children that finished a turn inside the box: 1 replied (check-file-cmd, 6 steps, 26 min),
  1 timed out (dev-fizzbuzz, 12 steps, 36 min). 5 still running or stopped with the host.
- Model calls: 57 at 17:58 totalling 9457 s (average 165 s); 61 by the end; longest single call 402 s.
  First-token p50 on host b: 1m 15s (Analytics page).
- Steps per turn: translator 2 to 3 steps to start a child (skill, engine_start, reply), 5 on b3
  (two no-act retries); children 5 to 12 steps without finishing.
- Tool errors: 0 failed tool spans in any `spans.jsonl`. Contract errors (no act): 8.
- HTTP: 409 on message to a busy engine and on delete of a busy engine; 404 on unknown engine;
  everything else 200 / 201 / 202.
- Time to child visible in status after the ask: 32 s (b), about 4 min (b2), about 7 min (b4, one
  402 s call that wrote the plan and three briefs).
- Parallel: 3 children busy at the same time on b4 from 17:44 to the end.
- Time waiting on the model: about 40 of 44 minutes.

## Addendum: state when I stopped (18:06)

- Scenario 1 (host b, left running): at 18:01:15 the translator was woken by
  `engine dev-fizzbuzz failed: timeout: global (engines children)`; five minutes later it was still
  in its first model call for that turn, so I saw no words to the person about the failure
  (UNVERIFIED what it will say). At the same moment the child started a new turn on my queued post
  ("add a final line that prints the word done"), with the translator's children line now
  `dev-fizzbuzz start 0 told`. So the dead child is working again, on my post, while its parent is
  deciding what to do about its death; where that turn's reply goes is UNVERIFIED.
- Scenario 5: the finished child answered my direct follow-up in one step, from memory, without
  rerunning: "exit 127 (frobnicate: command not found; ... the session itself showed exit 0 only
  because the trailing echo ran)". Context kept: PASS. Effect on the parent: finding 3.
- Scenario 7: translator-3's judging turn spent its first model call on a reply with no act and was
  in its second when I stopped b2. The failure reached the translator; the final report to the
  person was not seen.
- Scenario 2a and 3b: `list-themes` (step 7) and `dev-alpha/beta/gamma` (steps 7 to 8) were still
  working when I stopped b2 and b4; no tick on the b4 plan.
- Hosts: b2, b3, b4 stopped. Main host b left running.
- Totals at the end: 61 model calls on my hosts, 8 of them answers with no act.
