# Judge report: smoke3 (Opus) and smoke2 (Haiku)

> The run captures this cites (`docs/night/runs/`) are not in the tree; they are in commit `992cc0f`: `git show 992cc0f:docs/night/runs/<path>`.

Black-box review of two captured runs of the same task. Only files under
`docs/night/runs/smoke3` and `docs/night/runs/smoke2` were read; no source code.
Paths below are relative to `docs/night/runs/`. `S3` = smoke3, `S2` = smoke2.
`T` = `engines/translator/messages.md`. Character counts are Unicode characters
counted by script; they differ from `wc -c` bytes by under 1%.

Not checked, because the capture does not hold it:
- The live snapshots `children`, `thread`/`shared`, `code`, `terminal`, `browser`,
  `evidence`: every captured prompt shows only `(taken when the prompt is sent)` there
  (S3/prompts-end/translator.txt:121, :125; S3/prompts-end/dev-tree.txt:103-119), so the
  captures are not the exact prompt for those sections.
- The pages in `sources/`. Facts in notes.md/summary.md were checked against terminal
  logs only where a log shows the passage.
- Who sent the two extra messages to the smoke3 children (finding 7). Cost and wall time.

## Ranked findings

| # | Sev | Finding | Evidence | Kind |
|---|-----|---------|----------|------|
| 1 | high | A child's long reply does not reach the parent whole. The wake message shows 400 characters and says the rest is in the children artifact; the artifact was cut too, and the translator has no `read` tool. | S3/T:317 "(3678 characters; all of it is in the children artifact)"; S3/T:401 "The artifact cuts the reply partway through trait 1" | design |
| 2 | high | Every message to a child forces a reply, and every reply wakes the parent. There is no "no reply needed". Haiku turned this into a loop: 7 follow-up messages, 9 child replies for two small jobs. | S2/T:40, :74, :88, :174, :294, :378, :396; S2/engines/holmes-research/messages.md:595 "The format asks for one final turn" | design |
| 3 | high | A new message resets a child to its first phase, and the phase text then contradicts the message. | S3/prompts-end/research-holmes.txt:16 "1. **scope** ← now, step 2" and :24 "Your one reply is your answer and it ends your work", after the answer phase ended | design |
| 4 | high | History is never shortened and several things are stored many times. The research prompt went from 56,881 to 142,352 characters in about 15 answers; 88.7% of it is history. | section 1 table; 8 whole-log rewrites = 23,652 characters; judging skill body 4 x 3,681 | design |
| 5 | high | `run` returns the last 15 lines with no mark that lines above were dropped. It caused 3 reruns in smoke3 and one claim about output the result did not show. | S3/engines/dev-tree/messages.md:30-45, :49, :83; S3/engines/research-holmes/messages.md:455, :516 | small fix |
| 6 | high | Haiku's final summary holds wrong facts and leaves out the answer itself (the three traits are never named). | section 5, smoke2 table | model + prompt |
| 7 | high | A child's reply to a message the parent never sent is delivered to the parent as an ordinary reply; it arrived after the summary. The saved summary was not updated and is stale. | S3/engines/research-holmes/messages.md:636; S3/T:607; S3/engines/translator/artifacts/summary.md "2 replies" vs S3/T:693 "(3 replies)" | design |
| 8 | medium | A cut artifact snapshot keeps the first 4,000 characters and drops the end. For a log the end is the newest part. The cut notice names `read(name=...)`, which the translator does not have. | S3/prompts-end/research-holmes.txt:94-95; S3/prompts-end/translator.txt:105-106 vs tools :29-51 | small fix |
| 9 | medium | "Load the skill alone" costs one model answer per load. Opus spent 7 of 40 answers on it; Haiku ignored the rule and the calls worked. | S3/T:8, :229, :322, :417, :506, :612; S2/engines/holmes-research/messages.md:22-26 | design |
| 10 | medium | Haiku confused artifacts with files: 14 tool errors, the same 5 failed edits sent twice. | S2/engines/holmes-research/messages.md:377-381, :392-393, :406-410, :452 | small fix (tool naming) |
| 11 | medium | Haiku skipped the structure: no prompting skill, no plan before the children, developer never left triage, research left 6 questions open and wrote no lessons.md. | S2/T:9, :486; S2/engines/tree-developer/run.md "phase: triage"; S2/engines/holmes-research/artifacts/questions.md:11-16 | model + prompt |
| 12 | medium | One thing has several names in the prompt (thread / shared / shared context; children / engines; code / file artifact; "The plan" for the phase list beside a `plan` artifact). | section 1 | small fix |
| 13 | medium | The environment line names the app's folder, not the child's root, and the phase text says it is where the tools run. The root path is nowhere in the captured static prompt. | S3/prompts-end/dev-tree.txt:24, :121 | small fix |
| 14 | medium | Tools are not cut to the job. 28.5% of the developer prompt is tool text, including browser and evidence tools for a one-file script; each child carries 2,189 characters of ask/show controls. | S3/prompts-end/dev-tree.txt:31-68 | small fix |
| 15 | medium | `engine_call` does not do what its text says: its result is the same 400-character excerpt, and beside a `reply` it silently becomes a start. | S2/T:298; S2/T:78 "you replied, so a message comes when it replies" | design |
| 16 | medium | A child failed after its report was accepted; the transcript shows no answer and no retry. | S3/engines/dev-tree/messages.md:111 (last line); S3/T:693 "API Error: Sonnet 5.5's safeguards flagged this message" | design |
| 17 | medium | Haiku's translator put an answer from its own memory into the research brief, and the research notes credit "Scottish-born" to Wikipedia, whose saved page says "British author". | S2/T:9; S2/made/holmes/notes.md:4 vs S2/engines/holmes-research/artifacts/terminal/4.log:3 | model |
| 18 | medium | 13 of 21 Opus research answers have no `thought:` key although the format says "and nothing else". Accepted silently. | S3/engines/research-holmes/messages.md:180, :303, :329 ... :595 | small fix or accept |
| 19 | low | The excerpt limit cut 15 and 9 characters off replies of 415 and 409 characters; Haiku read that as missing proof. | S2/T:24, :54 | small fix |
| 20 | low | Messages to a child carry no sender; children call the parent "the user". | S3/engines/research-holmes/messages.md:640 "The user asks only for the title" | small fix |
| 21 | low | Tool results leak the wrapper: `session 6 'python3' '-c' '#web.read…`. The stored terminal state holds the whole script per web call (58,654 bytes for 30 sessions). | S3/engines/research-holmes/messages.md:148; S3/engines/research-holmes/artifacts/terminal.md:3 | small fix |
| 22 | low | The summary is written twice by rule (artifact, then reply): 3 copies in the next prompt. | S3/prompts-end/translator.txt:61-106, :713, :720 | small fix |
| 23 | low | The error text names "Sonnet 5.5" in a run labelled Opus. Not explained by the capture. | S3/T:693 | check |

## 1. The rendered prompt

Order (all engines): `[System]` → `## Instructions` (Goal, Scope, Done) → `## Phase`
(children only) → `## Tools` → `## Artifacts` (named ones, then skills, then live
snapshots) → an unlabelled environment line → messages → `## Response` → `[not sent]` notes.

Share of each section, end of run:

| Section | S3 research-holmes (142,352) | S3 translator (77,315) | S3 dev-tree (20,614) |
|---|---|---|---|
| Instructions | 993 (0.7%) | 5,521 (7.1%) | 928 (4.5%) |
| Phase | 1,378 (1.0%) | none | 1,636 (7.9%) |
| Tools | 4,828 (3.4%) | 5,707 (7.4%) | 5,878 (28.5%) |
| Artifacts, all | 6,684 (4.7%) | 6,914 (8.9%) | 2,401 (11.6%) |
| User messages | 1,136 (0.8%), 3 | 2,468 (3.2%), 5 | 472 (2.3%), 2 |
| Assistant messages | 61,839 (43.4%), 21 | 23,465 (30.3%), 13 | 6,373 (30.9%), 6 |
| Tool results | 64,468 (45.3%), 18 | 32,159 (41.6%), 11 | 1,900 (9.2%), 5 |
| Response | 944 (0.7%) | 944 (1.2%) | 944 (4.6%) |

Inside research-holmes history (approximate split): `write` calls 29,234 (of which 8
whole-log rewrites 23,652), `edit` calls with whole file texts 14,305, `run` results
23,684, `web.read` results 16,741, `web.search` results 12,856, web skill body 3,738.

Repeated or stale:
- Log: the phase text orders a whole rewrite each act, "kept whole and added to"
  (S3/prompts-mid/research-holmes.txt:27). Result: 8 copies in history plus the snapshot.
- Translator: judging skill body 4 times (S3/prompts-end/translator.txt:368, :456, :546, :731),
  3,681 characters each; prompting body 11,389 characters stays for the whole run (:152).
  Skill bodies are 30,025 of 77,315 characters (38.8%).
- Summary: snapshot (4,000 shown) + `write` call (4,266, :713) + `reply` (4,250, :720).
- The first translator answer holds each brief twice, in `engine_start` and in the `reply`
  (S3/T:206, 4,660 characters), as the instructions ask (translator.txt:19).
- Phase is stale after a reply: first phase again (finding 3; also S3/prompts-end/dev-tree.txt:16
  "1. **triage** ← now, step 1"; S2/prompts-end/holmes-research.txt:16, tree-developer.txt:16).

Contradictions a model can trip on:
- "all of it is in the children artifact" (S3/T:317) and "The whole reply is in the children
  artifact" (judging skill, translator.txt:374-376) against the cut the translator met (S3/T:401).
- "read(name="summary") for all of it" (translator.txt:106); the translator's tools are
  reply, write, skill, alert, engine_call, engine_start, post, edit_post (:29-51).
- "It is the one artifact you write" about the plan (translator.txt:14) against the
  summarizing skill "write(name="summary", ...)" (:670).
- "once it replied it is not working any more" (translator.txt:20) against the tool result
  "tree-developer is working" (S2/T:78); `engine_call` "Ask a child engine and wait ... its
  reply comes back as this call's result" (translator.txt:48) against finding 15.
- Response: "they all start at once; changes like open and edit apply first" (dev-tree.txt:226)
  reads as if edit then run works in one act. An edit is unsaved until `save`; Haiku's
  edit + run failed with "No such file or directory" (S2/engines/tree-developer/messages.md:19-21).
- Response says put every call in one act; skills, the summary write and the reply after a
  run each want "a step of its own". The reply-beside-run rule appears only as a tool error
  (S2/engines/tree-developer/messages.md:22).
- "Your environment line says where the tools run" (dev-tree.txt:24); the line gives
  `/Users/kaush/Downloads/Dev/dry-learn-night` (:121), the tools ran in `/Volumes/Backup/.../proj/tree`.

Naming:
- `### thread — your thread` (translator.txt:123) is `### shared — your thread` in children
  (dev-tree.txt:109), and `edit_post` calls it "the shared context" (translator.txt:51).
- `### children` (translator.txt:119); `engine_start` says "Its state is in the engines
  artifact" (:49); the wake message ends "(engines children)" (S3/T:216), unexplained.
- `### code` (dev-tree.txt:101) is "your file artifact" in `open` (:54). `## Phase` opens
  with "The plan:" (:15) while a `plan` artifact also exists (:77).
- The subscriptions table and the environment line have no heading and sit under
  `### thread` (translator.txt:123-141).
- Artifact content and skill bodies use `##` headings (`## Plan`, `## Log`, `## Decide`) at the
  same level as the prompt's own `## Tools`, `## Artifacts`, `## Response`.

Costly for little use: developer tools `browse`, `click`, `type`, `evidence` (1,046
characters), the `persona` skill line (dev-tree.txt:97) and empty `browser` and `evidence`
sections, for a Python script; the `reply` ask/show controls, 2,189 characters in every
child, whose reader is an engine; `engine_start` repeating the parameter text of
`engine_call` after "The same as engine_call" (translator.txt:48-49; 1,144 + 814 characters).

Response-format instruction: it is the last block before the model answers, after the
history (S3/prompts-mid/research-holmes.txt:410, last block). That is where a model reads
it. In captures taken after a reply it sits before the final `[Assistant]` block
(S3/prompts-end/translator.txt:803 then :812); that looks like a capture effect. It did not
hold Opus to the two keys (finding 18). Haiku read it as a duty to answer every message
(S2/engines/tree-developer/messages.md:97 "The response format asks for a closing acknowledgement").
The instruction to load `prompting` exists only in the skills list (translator.txt:112);
judging and summarizing are also in Scope. Haiku never loaded prompting.

## 2. Communication between engines

Briefs:
- Opus (S3/T:206): goal, scope, done and text for each child; exact expected output lines
  for tree.py; file names, url rule and reply form for the research; decided readings stated.
  Both children acted without a question. One gap: bad input with extra arguments was not
  specified; the developer decided it (reported, S3/engines/dev-tree/messages.md:107).
- Haiku (S2/T:9): no file names for the research, no reply form, no bad-input case, "takes n
  as an argument (or input)" beside "Read n from the command line argument". The research
  scope has no "never commit". The brief states the expected answer before any research:
  "i.e. A Study in Scarlet (1887), unless sources show otherwise".

A child's reply at the parent:
- Wake message: first 400 characters, then the length and a pointer (S3/T:216, :317, :410).
  Replies of 400 characters or less arrive whole (S3/T:607).
- The 3,680-character research reply was not readable whole. The translator saw it end
  "partway through trait 1"; trait 1 starts at character 1,857 and trait 2 at 2,296 of the
  reply, so the artifact limit is near 2,000 (inferred; snapshot not in the capture).
  The file list was at character 3,144. Cost: one send-back (S3/T:402), one more child
  answer, three more translator answers.
- Late: the third research reply came after the summary was sent (S3/T:603 then :607).
  Nothing was delivered twice.
- `engine_call` result is cut the same way: `"Both claims are true. ... sherlockadjacent-substack-co…" (712 characters; ...)` (S2/T:298).

Messages that wake an engine:
- First message to a child: "Work on your goal (see your instructions), then reply with the
  result." plus the brief's text (S3/engines/dev-tree/messages.md:3-5). A post arrives as
  "Guided posts for you: [thread 1] translator → tree-developer: ..." with its sender
  (S2/engines/tree-developer/messages.md:52-53). Both clear.
- A follow-up by `engine_start`/`engine_call` arrives as bare text with no sender
  (S3/engines/research-holmes/messages.md:627). The child cannot tell the parent from the person.
- Two messages reached smoke3 children that the translator's transcript does not contain:
  "Reply with the title of the first story, from your summary. No new search."
  (research-holmes/messages.md:636) and "Run python3 tree.py 2 once and reply with its exact
  output." (dev-tree/messages.md:111). Sender not checked. The first woke the translator as
  a normal child reply; the second has no answer and the child shows as failed.

Thread: smoke3 has no posts. Smoke2 has one (S2/engines/translator/thread.md), a request for
evidence the child had already sent. After it, all three smoke2 engines show `heard: 1`
(S2/engines/*/run.md), including holmes-research, which was not the addressee. Whether the
post text entered that child's prompt: not checked.

Could each receiver act without guessing? Opus children: yes. Opus translator: no for the
long reply. Haiku translator: it took the 400-character excerpt for the whole evidence
(S2/T:38 "the n=3 proof was cut off in the children artifact").

## 3. Sub-engine loops in parallel

- Started together: yes in both runs, two `engine_start` calls in one act (S3/T:206, S2/T:9).
- Ran at the same time: smoke3 dev-tree session 2 and research session 4 carry the same
  stamp 1795494056 (S3/engines/dev-tree/artifacts/terminal.md:4; research-holmes/artifacts/terminal.md:3).
  Smoke2: first sessions at 1795494014 and 1795494017.
- Separate: no mention of the other child's subject in any child transcript or captured
  child prompt (grep for tree.py/proj/tree in research files and holmes/sherlock in developer
  files: 0 matches in all 10 files). Each has its own root and terminal log folder.
  Shared thread across siblings: see section 2, not checked.
- One finished first: dev-tree replied after 4 sessions (stamps 045 to 083); research ran to
  stamp 356. The translator judged and ticked dev-tree alone and told the person
  (S3/T:308-309), then was woken once for the research. No polling answers by Opus.
- Haiku: while the research ran, the translator spent 7 answers on the already finished
  developer (S2/T:40-209) and sent the person three "Still in progress" replies (:50, :184, :209).
- The `keep-going` alert fires only "while `plan` has an open `- [ ]` step"
  (translator.txt:117). Haiku wrote no plan until its last act (S2/T:486), so the alert
  guarded nothing during that run.

## 4. Artifacts

| Artifact | Current truth? | Evidence |
|---|---|---|
| S3 translator plan | yes, both lines ticked | S3/engines/translator/artifacts/plan.md |
| S3 translator summary | stale: "2 replies", no word of the later failed developer turn; snapshot cut at 4,000 of 4,154, the "Files:" block is lost | S3/prompts-end/translator.txt:105-106; S3/T:693 |
| S3 research log | true to the search phase; snapshot drops the last 662 characters, the newest entries | S3/prompts-end/research-holmes.txt:94-95 |
| S3 research questions | yes, all ticked, matches summary.md | S3/engines/research-holmes/artifacts/questions.md |
| S3 dev-tree plan | yes; Proof corrected from `cat -A` to `cat -e` | S3/engines/dev-tree/messages.md:10, :50 |
| S3 run.md, both children | reset: dev-tree "phase: triage step: 0" after finishing in test | S3/engines/dev-tree/run.md |
| S2 translator plan | written once, already ticked, in the last act | S2/T:486 |
| S2 research log | not a log: rewritten shorter each time (780 → 448 → 483 → 547 → 758 characters); first entry was "Results pending"; failed-read lines dropped | S2/engines/holmes-research/messages.md:22, :155, :204 |
| S2 research questions | 6 lines open while the child says "The learn phase is closed" | S2/engines/holmes-research/artifacts/questions.md:11-16; messages.md:527 |
| S2 developer plan, notes | placeholders, though the file was written and run | S2/prompts-end/tree-developer.txt:81 |
| children, thread, code, terminal, browser, evidence | not checked (not in capture) | see top |

Shown twice (snapshot and history): the log, the summary, the questions. Files stay open
with no tool to close them (dev-tree.txt:54-57); research had three open, 13,173 bytes
(S3/engines/research-holmes/artifacts/code.md), whose texts are also in history as `edit`
calls (research-holmes/messages.md:477, :490). Snapshot rendering: not checked. Stored
terminal state keeps 30 sessions (4 to 33) with the full Python source of each web call.

## 5. Truthfulness

### smoke3, `engines/translator/artifacts/summary.md`

| Claim | Verdict |
|---|---|
| Asked: quoted in quotation marks | altered: the two folder paths are shortened to "…/proj/holmes" and "…/proj/tree" inside the quote |
| two children started together; tree readings; no reviewer | true (S3/T:206) |
| research sent back once, first reply cut | true as the translator saw it (S3/T:401-402); the artifact itself not checked |
| "model not set, so my own" | no `model=` in the call: true. Which model ran: not checked |
| creator, novel, 1887, Beeton's, Ward, Lock & Co. | matches the child's reply (research-holmes/messages.md:623) and S3/made/holmes/summary.md:6-15 |
| traits 1-3 with urls | match the second reply (research-holmes/messages.md:632); urls written without "https://www." |
| 10 pages; 6597 / 5080 / 1496 bytes; exit 0 | true (terminal/33.log; S3/made/holmes sizes) |
| "2 replies" | true when written; 3 by the end. Stale |
| tree.py 3 and no argument: exit 0 and the lines | true (dev-tree terminal/4.log) |
| tree.py 1: "exit 0" | no evidence for the exit code: that run's status was never printed (dev-tree/messages.md:85). Output `*$` is shown |
| 0, -2, abc, `3 1 2`: exit 1, 0 bytes, usage on stderr | true (terminal/3.log:23-38) |
| developer rejects extra arguments | true (S3/made/tree/tree.py:12-14) |
| day conflict 21 Nov / 1 Dec; Gillette "alleged" | true (research-holmes/messages.md:156, :164, :197) |
| height 7 run, output cut, not checked | true. The full correct output was on disk (dev-tree terminal/3.log:14-20); nothing told the engine |
| nothing committed, pushed or published | no git command in any terminal log: consistent. No command was run to show it |
| Files list | true for notes.md, summary.md, lessons.md, tree.py (S3/made); sources/ not in capture |

No invented fact found. The third reply and the failed developer turn are only in the later
reply (S3/T:693), not in the saved summary. Inside the developer: dev-tree/messages.md:49
says "`tree.py 3` gives `  *`, ` ***`, `*****`" and ticks the step, but its result (:30-45)
shows only the last 15 lines, from ` *******$`. Two answers later: "I haven't seen the
output of `tree.py 3` and `tree.py 1`" (:83), and a rerun. The report came after the rerun.

### smoke2, `engines/translator/artifacts/summary.md`

| Line | Claim | Verdict |
|---|---|---|
| 1 | "Done meant both files present and the outputs shown" | not what the person asked; sources and traits are missing from it |
| 5 | first story decided "because nobody could be asked" | it is a fact to research, not a reading; decided from memory before the research (S2/T:9) |
| 6 | "tree-developer's first proof pointed back to an earlier confirmation" | wrong. The first reply pasted both outputs (S2/engines/tree-developer/messages.md:48). The reply that pointed back was the fourth (:99), an answer to "No further work needed" |
| 11 | "tree.py 247 bytes (ls -l)" | 247 true; the command was `ls -la` (:59) |
| 15 | holmes-research "Numbers: none" | wrong. The reply gave 2054 bytes, 2158 bytes, 8 pages (S2/engines/holmes-research/messages.md:554) |
| 19 | output "matches the brief" | the brief said "centred with leading spaces"; the output has trailing spaces (`  *  `, tree-developer/messages.md:34). Not noticed |
| 20 | Doyle, A Study in Scarlet, 1887, Beeton's | matches S2/made/holmes/summary.md:4-7 |
| 23 | trait 3 unconfirmed, notes.md line 16, summary.md line 13 | true (S2/made/holmes) |
| 24 | lessons.md never written | true |
| 26 | nothing committed or published | children's statements only; no git command in logs |
| - | the three traits | never stated in the summary or in the final reply (S2/T:486: "Speech traits 1 and 2 are sourced. Trait 3 is unconfirmed") |
| - | 7 follow-up messages to children | only one of them is under Direction |

Also: the translator told the person "The tree script is confirmed" in the act in which it
asked the child to confirm (S2/T:74). S2/made/holmes/notes.md:4 says "Scottish-born author
... (source: https://en.wikipedia.org/wiki/Sherlock_Holmes)"; the saved page says "British
author" (terminal/4.log:3); "Scottish" is in a search snippet of a page that returned 403 (1.log:6).

## 6. Efficiency

| Engine | Answers | Off-format | Wasted or avoidable |
|---|---|---|---|
| S3 translator | 13 | 0 | 6 skill-only loads (4 of them the same judging text); 3 for the send-back round; 2 for the unasked third reply |
| S3 research-holmes | 21 | 13 without `thought:` | 1 skill-only load; 2 reruns after cut output (:455, :516); 5 failed reads and 1 failed search inside useful acts (403, PDF) |
| S3 dev-tree | 6 | 1 (:83) | 1 rerun after cut output; 1 answer that only ticked the plan and changed phase (:50) |
| S2 translator | 13 | 0 by shape; rules broken: skill never alone, plan last, summary write and reply in one act | 7: one post, five calls, one send-back to children that had already answered; "Still in progress" replies |
| S2 tree-developer | 9 | 0 | 1 failed act (edit without save, reply beside run, :11-22); 6 answers on confirmation rounds (:55-129) |
| S2 holmes-research | 23 | 0 | about 6 answers lost to artifact/file confusion (:373-476, 14 errors); 2 replies to "nothing more needed" (:587, :597); web skill loaded with searches in one act and it worked (:22) |

Totals: smoke3 40 answers, smoke2 45.

Where steps could be saved:
- Deliver the whole reply (or a file path to it): the smoke3 send-back, 4 answers.
- Keep judging loaded instead of one load per reply: 3 to 5 answers per run, 11,043 characters.
- Mark cut output and point to the log file: 3 answers in smoke3.
- Let a parent message need no reply: about 15 answers in smoke2.
- Let the reply be the saved summary: 1 answer, 4,250 characters.
- An append call for the log: about 20,000 characters of history in smoke3.

## 7. Haiku vs Opus on the same task

| Point | Opus (smoke3) | Haiku (smoke2) |
|---|---|---|
| prompting skill before children | loaded (T:8) | never loaded |
| plan before children | yes (T:206) | written at the end only (T:486) |
| brief | complete, exact outputs | thin; contains the expected answer |
| developer phases | triage → code → test | stayed in triage; plan artifact empty |
| tree.py | 32 lines, usage and exit 1 on bad input, no trailing spaces | 13 lines, `str.center` with trailing spaces, `int()` unguarded, bad input not tested |
| research files | notes.md 6,597, summary.md 5,080, lessons.md 1,496, 10 pages | notes.md 2,054, summary.md 2,158, no lessons.md, 8 pages |
| research sources for traits | study guide, Wikipedia, Doyle Encyclopedia | quote-collection sites, Wikipedia, one Substack post; trait 3 marked unconfirmed by the child itself |
| date conflict 21 Nov / 1 Dec | found and reported | "none recorded" (S2/made/holmes/notes.md:19); it did not read the EBSCO page |
| invented or misattributed | none found | "Scottish-born" credited to Wikipedia; "first proof pointed back"; "Numbers: none" |
| format slips | 14 answers without `thought:` | none in shape; 14 tool errors; rules on "alone" ignored |
| reading the children artifact | read it and found the cut | treated the 400-character excerpt as the reply |
| follow-ups to children | 1, needed | 7, none needed for new facts |
| final summary | full form, traits and urls, open points | no traits, no urls, two wrong lines |
| research sessions (stamps) | 056 to 356 | 017 to 134 |

Both hit the same blocked pages (Britannica, ResearchGate, PDFs). Opus wrote that into
lessons.md; nothing carries it on ("Learning: nothing learned yet", S3/prompts-end/research-holmes.txt:749).

## Fragile parts

Ranked. Each: what was seen, why it breaks in a longer or larger run, and the kind of fix.

1. **high: reply hand-off by excerpt and capped artifact.** Saw: finding 1; a 3,680-character
   reply could not be read whole and the parent has no tool to read more. Why: research
   answers and review verdicts are long; each costs a send-back round and the restated reply
   loses detail. Design decision (whole reply, a file the parent can read, or paging).
2. **high: history growth.** Saw: 142,352 characters after 21 answers and about 5 minutes;
   85,471 added between the mid and end captures. Why: a one-hour research child passes a
   million characters at this rate; the capture shows no cut or summary of old turns.
   Design decision.
3. **high: whole-artifact rewrites.** Saw: "kept whole and added to"
   (S3/prompts-mid/research-holmes.txt:27); 8 rewrites, 23,652 characters, for a
   4,662-character log. Why: cost grows with the square of the log; a weaker model shortens
   the log instead (Haiku, section 4). Small fix (an append call), plus item 2.
4. **high: reply-forces-reply.** Saw: finding 2. Why: with many children an unsure parent
   multiplies turns without limit, and each reply reloads a 3,681-character skill. Design
   decision (a message that needs no answer; a child may end a turn silent).
5. **high: phase reset on a new message.** Saw: finding 3; Opus ignored the stale phase.
   Why: a send-back that needs real work starts the child in triage or scope, told to plan
   or to "Search nothing yet". Design decision.
6. **high: unasked and late replies.** Saw: finding 7. Why: when the person or a watcher
   writes to a child, the parent re-judges after the goal is closed and the saved summary
   no longer matches the last word. Design decision (who gets a reply; what reopens a goal).
7. **medium: output tail without a marker.** Saw: finding 5; logs of 24, 38, 53, 66 and 77
   lines shown as 15 (S3/engines/*/artifacts/terminal/*.log), the full text on disk. Why:
   build and test output is long; models rerun, or assert what they did not see. A grep with
   `head -40` returns the last 15 matches and looks complete. Small fix (count of dropped
   lines, path of the log).
8. **medium: snapshot cut keeps the head.** Saw: finding 8. Why: logs, plans and summaries
   grow at the end; past 4,000 characters the engine stops seeing its newest lines. Small fix.
9. **medium: skills re-sent on every load.** Saw: judging 4 times; "its tools are yours until
   the next message" (translator.txt:46). Why: children x replies x 3,681 characters, all kept.
   Small fix or design (keep it loaded; do not repeat a body already in history).
10. **medium: one thread for the whole tree.** Saw: `heard: 1` in a child that was not
    addressed (S2/engines/holmes-research/run.md:3). Why: with many children every post may
    enter every prompt. Not checked beyond the counter. Design decision.
11. **medium: failure after an accepted report.** Saw: finding 16; the plan line stayed
    ticked and the translator wrote "I have not seen whether that failed turn did anything to
    the folder" (S3/T:693). Why: provider errors are certain in long runs; no retry or status
    rule is visible. Design decision.
12. **medium: files stay open; terminal state keeps full commands.** Saw: section 4. Why: a
    long developer run opens many files with no way to close them. Rendering not checked.
    Small fix.
13. **medium: rules kept only by an error or only in prose.** Saw: "reply ... not sent"
    (S2/engines/tree-developer/messages.md:22); "skill alone" and `thought:` not enforced;
    edit-then-run fails without save. Why: models follow different subsets, so cost and
    correctness vary by model. Small fixes, one per rule.
14. **low: big pages.** Saw: a 275,121-character page saved to a file and shown as 10 lines
    (S3/engines/research-holmes/messages.md:148-159). This part held; the remaining risk is item 7.
