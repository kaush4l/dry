### user

Write a python script to print a tree of height n where each level has 2 more '*'. Make it center aligned. Work in the folder /Volumes/Backup/dry-build/loop-run/alert1/proj. When that is done, look at it again by yourself two minutes later: have it run for n=4 and tell me what it printed.

### assistant

thought: The person wants a Python script in a specific folder that prints a centered tree of height n with 2 more '*' per level, then wants me to check it myself two minutes later and report what it printed for n=4. I have no direct file tools, so I need a developer child. First step is to load the prompting skill as required before starting a child.
act: skill(name="prompting")

### tools

skill(name="prompting") → Skill `prompting` loaded. Follow it:

# Prompting: from an ask to a brief

A child knows only what you write. It cannot see this conversation, and
whatever it brings back can only be checked against what you asked for. A
good brief is short and exact. It describes the work, not each step.

## 1. Think first

Write down, in your thought:

- **The ask in one line**, in your own words. If you cannot, ask the person.
- **What is really needed.** "The button is broken" asks for a fix and a test
  that proves it, not just an edit.
- **What a top engineer would do that was not asked.** For example: check
  where else the same thing is used, run the existing tests first, look at
  the running app. Add it only if it makes the result more right.
- **What is unknown.** If reading can settle it, the child finds out. If
  an experiment can settle it, the child tries it. If it has one natural
  reading, take it and write it into the brief ("level 1 has one star; n
  comes from the command line"). Ask the person first only when two
  readings would give two different results and nothing favours one.

## 2. Choose the template

Your engines artifact lists the templates with their phases. For code:

| the ask | template | why |
|---|---|---|
| a question: how does it work, where is it, why does it do that | `researcher` | it reads and runs, and has nothing to change |
| a change or a bug, and you know where it lives | `developer` | triage, plan, code, test, in one child |
| a change in a project or area nobody here has read, or one big enough to need a plan you can judge | `researcher` first, then `developer` with the findings in `text` | the plan starts from confirmed lines, not guesses |
| a change a developer finished that is more than a small fix | `reviewer`, with the brief and the developer's report | a second reader who did not write it, rerunning the proof |

A **small fix** is one or two files, nothing structural (no new type,
module, dependency or public API change), and a clear done. It gets one
developer, no plan first and no reviewer. Planning and review cost more
than they find on a small fix. A small new thing (a script, one page, one
function with its test) is the same: one developer, and its `done` names
the command to run and what it must print.

**Plan first.** Add "Plan first: reply with the plan before any edit." to
the developer's scope when the change is structural, touches more than
about five files, changes a public API, or the person asked to see the
plan. You approve it (see judging), and ask the person only for a choice
only they can make.

## 3. Write the brief

The engine_start fields are the parts of the brief:

| field | what goes in it |
|---|---|
| `name` | a short role and topic, lower case: `dev-greeting`, `dev-login-bug` |
| `goal` | one or two sentences: the outcome, and who it is for. Behaviour, not steps |
| `scope` | what it may do alone; when it stops and replies instead; what is out of scope; "Never commit, push or publish" |
| `done` | the checks a finished result passes, each one that can be checked on its own, plus the report it must return (below) |
| `text` | the context: how it works today as far as you know it, where to start reading, the commands the project uses, constraints the person gave |
| `template` | `researcher`, `developer` or `reviewer` (above) |
| `root` | the project folder, exactly as the person gave it; when they named none, `.` (the workspace on your environment line) |

Rules:

- **Point, do not paste.** Name the files, the functions and the commands to
  start from. The child opens them itself and sees them as they are now.
- **Name the contracts.** Say which function, command or page must behave
  differently, and how: "`greet("Ada")` returns `Hello, Ada!`", not "update
  the greeting code".
- **Current, then desired.** One line for what happens today, one line for
  what should happen.
- **The person's words count.** Copy their constraints exactly (a version,
  a style, "no new dependencies").
- **Out of scope, said out loud.** Name what it must not touch.
- **Positive rules.** Say what to do ("keep the public API") rather than
  listing what not to do.

## 4. The report it must return

Put this at the end of a developer's `done`, so every developer answers
the same way (a researcher and a reviewer have their own forms, in their
skills; name them in `done`: "Reply with the answer form" / "Reply with
the verdict form"):

```text
Reply with:
Changed: each file, one line on what changed
Ran: each command, with its exit code and last line
Saw: what the running app or output showed
Unverified: what you could not check, and why
Also changed: anything outside the plan (or "nothing")
```

If an item is missing, the work is not done (see the judging skill).

## 5. Example: a small change

The person says: "In ~/code/hello, make the greeting say the name with a
comma, like 'Hello, Ada!'".

```text
engine_start(name="dev-greeting", template="developer", root="/Users/me/code/hello",
  goal="The greeting reads 'Hello, <name>!' with a comma, for whoever runs the hello project.",
  scope="Change only the greeting and its test. Stop and reply if the greeting is used by anything else that would change. Never commit, push or publish.",
  done="greet('Ada') returns 'Hello, Ada!'; the project's tests pass; running the program prints it. Reply with Changed / Ran / Saw / Unverified / Also changed.",
  text="Today the greeting has no comma (the person says). Start from the project's entry file and its tests; find the commands in package.json or Cargo.toml. No new dependencies.")
```

## 6. Example: a question about how code works

The person says: "How does the login page decide where to send me?"

```text
engine_start(name="research-login-redirect", template="researcher", root="/Users/me/code/app",
  goal="Explain how the login page decides where to send the user after sign-in, so the person can change it.",
  scope="Read and run only: change no file. Stop at the redirect; the rest of auth is out of scope.",
  done="The flow from the submit to the redirect, each step as path:line, in order; each confirmed by a test or a run (or marked read, not run). Reply with the answer form.",
  text="Start from the login route or form; the project's commands are in package.json.")
```

Without "confirmed", you would get a guess.

## 7. Example: a review

A developer replied with its report for a change across four files. Before
you report, start a reviewer on the same root:

```text
engine_start(name="review-csv-export", template="reviewer", root="/Users/me/code/app",
  goal="Decide whether the CSV export change holds against its brief, for the person who will merge it.",
  scope="Review only: change no file, commit nothing. The brief below is the measure; the person's ask is in it.",
  done="Each done line of the brief checked with a rerun; scope and the code's standards checked apart; the tree unchanged. Reply with the verdict form.",
  text="The brief: <the developer's goal, scope and done, as you wrote them>. The developer's report: <its whole reply>.")
```

Paste the brief and the report: they are what is judged, and the reviewer
cannot see them anywhere else.

## 8. Sending it back

When a reply misses something, send the same child a message (engine_start
with its name and a `text`). The message names exactly what is missing ("Ran
has no test output; run the tests and reply again"). The child keeps
everything it has read. Make a new child only for a new approach, and give
it the old child's report in `text`.

## 9. Posts on the thread

A post is a brief too. Name `to` (an engine's id, or `you`) and write it so
the addressee can act without having seen anything else: what, why, what
done looks like, where the material is. A post with no `to` reaches nobody
and comes back to you.

Ideas from Matt Pocock's AGENT-BRIEF and writing-for-agents, Lauren Tan's
pstack `sharpen`, Garry Tan's gstack `captain` READY block, and Open SWE's
planner, programmer and reviewer roles (all MIT).
The text above is dry's own.

### assistant

thought: The ask: a Python script in /Volumes/Backup/dry-build/loop-run/alert1/proj that prints a center-aligned tree of height n, each level having 2 more '*' than the one above (level 1: 1 star). n comes from the command line. I have no direct terminal, so I delegate to a developer child with a precise brief. Then, two minutes later, I re-run it for n=4 myself (via a follow-up with the same child, since I cannot run commands directly) and report the output. I'll set the alert after the work is judged done.

Ambiguity taken: level 1 has 1 star (levels: 1,3,5,...), n from argv, lines centered on the widest line. This is the natural reading; I'll write it into the brief.
act: engine_start(name="dev-tree-script", goal="A Python script that prints a center-aligned tree of height n: level 1 has 1 '*', each level has 2 more than the level above (level 2: 3, level 3: 5, ...), n levels total, each line centered on the width of the widest line. Whoever runs it with a height gets that output.", scope="Create exactly one new Python script in /Volumes/Backup/dry-build/loop-run/alert1/proj (create the folder if it does not exist). No other files, no new dependencies, no commits, no pushes, no publish. Stop and reply if the folder cannot be created or the script cannot be run.", done="One script exists in the folder; running `python3 <script> 4` prints 4 lines of widths 1,3,5,7 (level 1: 1 star, level 2: 3, level 3: 5, level 4: 7), each line centered on the widest line (7), with no leading or trailing spaces on the longest line. Run it once and report the exact output. Reply with: Changed / Ran / Saw / Unverified / Also changed.", text="Folder: /Volumes/Backup/dry-build/loop-run/alert1/proj (create it if missing). Python 3, stdlib only. The height n comes from argv (e.g. `python3 tree.py 4`); if n is missing or not an integer, print a one-line usage message and exit non-zero. Center alignment: pad each line with leading spaces (and trailing spaces or not) so all lines are the same width, the width of the widest line (level n). Keep it simple and readable.", template="developer", root="/Volumes/Backup/dry-build/loop-run/alert1/proj")

### tools

engine_start(name="dev-tree-script", goal="A Python script that prints a center-aligned tree of height …", scope="Create exactly one new Python script in /Volumes/Backup/dry-…", done="One script exists in the folder; running `python3 <script> 4…", text="Folder: /Volumes/Backup/dry-build/loop-run/alert1/proj (crea…", template="developer", root="/Volumes/Backup/dry-build/loop-run/alert1/proj") → made dev-tree-script from template developer with your goal and started it; a message comes when it replies

### assistant

thought: The developer child is started. Per the brief rule, I reply now with exactly what I asked it, then wait for its message. The two-minute re-check will be set as an alert once the child replies and I judge the result done.
act: reply(text="Asked dev-tree-script to create the tree-printing script. Brief: goal — a Python script in /Volumes/Backup/dry-build/loop-run/alert1/proj that prints a center-aligned tree of height n (level 1: 1 '*', level 2: 3, level 3: 5, ...), lines centered on the widest line; n comes from argv. Scope — one new script only, stdlib only, no other files, no commits. Done — running `python3 <script> 4` prints 4 lines of widths 1,3,5,7, each centered on the widest line; reply with Changed / Ran / Saw / Unverified / Also changed. I'll set the two-minute re-check alert once its reply comes back.")

### user

engine dev-tree-script replied: "Done. One new stdlib-only script `tree.py` (33 lines) in /Volumes/Backup/dry-build/loop-run/alert1/proj.

Changed: tree.py — new script, prints a center-aligned tree of height n from argv (each level 2 stars wider than the one above), every line padded to the widest line's width; prints one-line usage and exits 2 on missing/non-integer/non-positive height.
Ran: python3 tree.py 4 → exit 0 (4 lines,…" (878 characters; all of it is in the children artifact) (engines children)

### assistant

thought: A child just replied. Per the rules, my first step is to load the judging skill, then read its full reply against the done criteria.
act: skill(name="judging")

### tools

skill(name="judging") → Skill `judging` loaded. Follow it:

# Judging a child's reply

The child's report is a claim. Your job is to decide whether the claim
holds, using only what it shows.

**Read the whole reply.** The message that wakes you shows only its
start. The whole reply is in the children artifact, under the child's
`last reply`. Judge that, never the cut message.

## Check, in order

1. **Every done check has evidence.** For each line of the `done` you
   wrote, find the evidence in the reply: a command with its exit code and
   last line, a file:line, what the page showed. "Tests pass" with no command
   and no output is not evidence, and neither is a fact with no command
   behind it ("the folder is not a git repo": which command showed that?).
2. **The evidence fits the claim.** A build that passed does not prove a
   behaviour. A test that passes but does not exercise the change proves
   nothing about it. A screenshot path with no text from the page is not
   something you can read.
3. **Scope held.** "Also changed" lists only what is needed. A file that
   was not in the plan needs a reason.
4. **The unverified part is said.** Something that could not be checked
   (no browser here, no evidence.md in the project) is fine if the reply
   says so. Silence about it is not.
5. **It answers the person's ask**, not only your brief. If your brief was
   wrong, say so to the person.

## By kind of reply

- **A plan** (a developer whose scope said "plan first"): check the steps
  cover every done line, each step names a file, the proof would fail on
  today's code, and out of scope matches yours. Approve it with a message
  to the same child: "Plan approved: code it." Send it back with what is
  missing. Ask the person (the plan in the reply text, one ask=) only when it holds
  a choice only they can make (two behaviours, a breaking change) or they
  asked to see it; their answer goes to the child word for word.
- **A developer's report**: the checks above. If it holds and the change
  is more than a small fix (prompting, §2), start a reviewer before you
  report (prompting, §7). A small fix that holds is reported.
- **A researcher's answer**: every finding has a path:line and a
  confirmation, or says "read, not run". Its Tree line (the last
  `git status --short`) shows nothing it changed: "no file was changed"
  needs that command, as a reviewer's does. If the answer feeds a change, the
  developer's `text` gets the findings as they are.
- **A reviewer's verdict**: `accept` with no blockers: report the change,
  with the reviewer's Ran lines beside the developer's and its "should"
  findings as notes. `send back`: send the blockers word for word to the
  same developer (engine_start with its name and `text`), then review its
  next report the same way, with the same reviewer. A blocker that comes
  back twice, or a developer and a reviewer that disagree on the facts:
  ask the person, with both sides.

## Decide

- **Accept**: every check holds. Report to the person: what was done, the
  evidence (copy the commands and their last lines exactly), and what is
  unverified.
- **Send back**: name exactly what is missing or wrong, in one message to the
  same child (engine_start with its name and `text`). Do not fix it
  yourself in the report.
- **Ask the person**: the result shows a choice only they can make (two
  behaviours, a breaking change), or the same failure came back twice. Ask
  one question with ask=.

Never upgrade a result: "probably works" stays "not verified".

Ideas from gstack's review lead-judgment rules, pstack's `interrogate`
(a finding counts only if a rerun proves it), and Open SWE's plan review
and reviewer loop, all MIT; written for dry.

### assistant

thought: The child's reply holds: one new file tree.py, run for n=4 with exit 0 and the exact centered output (4 lines, stars 1/3/5/7, each line 7 wide, widest line all stars), usage + exit 2 on bad input, __pycache__ cleaned up, no other changes, nothing unverified. This is a small new thing, one developer is enough — no reviewer. Next: set the two-minute re-check alert as the person asked, then reply with the report now; the alert will wake me to do the re-run and report the output.
act: alert(name="tree-recheck", trigger="every 2m", once="true", text="Re-check the tree script the person asked for: have dev-tree-script run `python3 tree.py 4` in /Volumes/Backup/dry-build/loop-run/alert1/proj (unchanged since the first run) and report the exact printed output. Then tell the person exactly what it printed.")

### tools

alert(name="tree-recheck", trigger="every 2m", once="true", text="Re-check the tree script the person asked for: have dev-tree…") → made alert tree-recheck: every 2m, once. It wakes you with its text; nothing fires at the moment it is made.

### assistant

thought: The child's reply holds — one new stdlib-only tree.py, run for n=4 with exit 0, centered lines of widths 1,3,5,7, usage/exit-2 on bad input, __pycache__ cleaned, nothing else changed. That is a small new thing, one developer is enough, no reviewer needed. The person also asked for a two-minute re-check; I set the alert. Now I reply with the report of the initial work and note the re-check is scheduled, then wait for the alert to fire and do the re-run.
act: reply(text="Done — a new stdlib-only script tree.py (33 lines) in /Volumes/Backup/dry-build/loop-run/alert1/proj. Running `python3 tree.py 4` (exit 0) prints, centered on the widest line (7):\n   *\n  ***\n *****\n*******\nLevels are 1,3,5,7 stars, each line 7 wide; missing/bad height prints a one-line usage and exits 2. I've also set a two-minute re-check alert: I'll have it re-run for n=4 in two minutes and tell you exactly what it prints.")

### user

Alert tree-recheck fired (every 2m): Re-check the tree script the person asked for: have dev-tree-script run `python3 tree.py 4` in /Volumes/Backup/dry-build/loop-run/alert1/proj (unchanged since the first run) and report the exact printed output. Then tell the person exactly what it printed.

### assistant

thought: The alert fired — two minutes have passed since the first run. The person asked for a re-check: have the script run for n=4 and tell me exactly what it printed. I have no terminal of my own, so the only honest way to re-run it is through the child that built it; I'll send it the re-check request, judge its reply, then report the exact output. I must not just repeat the first run's output as if it were fresh.
act: engine_start(name="dev-tree-script", text="Re-check: run `python3 tree.py 4` in /Volumes/Backup/dry-build/loop-run/alert1/proj (the file is unchanged since the first run). Do NOT change anything. Reply with the exit code and the exact printed output, character for character, including leading spaces, plus any notes.")

### tools

engine_start(name="dev-tree-script", text="Re-check: run `python3 tree.py 4` in /Volumes/Backup/dry-bui…") → sent dev-tree-script your message; a message comes when it replies

### assistant

thought: The re-check alert fired. I already sent dev-tree-script the re-run request (python3 tree.py 4, no changes, reply with exit code and exact output). Its reply has not come back yet, so I must not report an output yet. I'll tell the person the re-check is in flight and wait for the child's reply, then report exactly what it printed.
act: reply(text="Two-minute re-check is in progress: I've asked dev-tree-script to re-run `python3 tree.py 4` in the folder without changing anything. As soon as its reply comes back, I'll tell you exactly what it printed.")

### user

engine dev-tree-script replied: "Exit code: 0. Exact output (4 lines, each 7 characters wide):
```
   *   
  ***  
 ***** 
*******
```
Character-for-character, line 1 is 3 spaces + `*` + 3 spaces; line 2 is 2 spaces + `***` + 2 spaces; line 3 is 1 space + `*****` + 1 space; line 4 is `*******` (no padding). Verified with `cat -A` (shown above): each line ends with `$` except the last, which does not — all four lines are exactly 7…" (710 characters; all of it is in the children artifact) (engines children)

### assistant

thought: The child's re-check reply holds: exit 0, exact character-for-character output (4 lines, each 7 wide; line 1 = 3 spaces + star + 3 spaces, line 2 = 2 + 3 + 2, line 3 = 1 + 5 + 1, line 4 = 7 stars with no padding), verified with cat -A, nothing changed, only tree.py in the folder. This is a re-check of a small script, no reviewer needed. The alert already fired and was once-only, so no cleanup needed. I can now report the exact output to the person.
act: reply(text="Two-minute re-check done: `python3 tree.py 4` ran in /Volumes/Backup/dry-build/loop-run/alert1/proj, exit 0, and printed exactly (each line 7 characters wide, widest line all stars, lines 1-3 carry trailing spaces):\n   *   \n  ***  \n ***** \n*******\nLine 1: 3 spaces + '*' + 3 spaces; line 2: 2 spaces + '***' + 2 spaces; line 3: 1 space + '*****' + 1 space; line 4: '*******' (no padding). Verified character-for-character with cat -A; nothing was changed. The centering is real: every line is the width of the widest (7), and the tree widens by 2 stars per level, as asked.")

