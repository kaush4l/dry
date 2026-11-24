### user

Write a python script to print a tree of height n where each level has 2 more '*'. Make it center aligned. Work in the folder /Volumes/Backup/dry-build/loop-run/loop3/proj.

### assistant

thought: The person wants a Python script that prints a centered tree of height n, with each level having 2 more '*'. This is a code-making task, so per my instructions I should load the prompting skill before making a child. Let me read it first.
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

thought: The ask: a Python script in /Volumes/Backup/dry-build/loop-run/loop3/proj that prints a centered tree of height n, each level with 2 more '*'. This is a small new thing (one script), so per the prompting skill: one developer child, no plan-first, no reviewer. Natural readings to fix in the brief: level 1 has 1 star (levels add 2: 1,3,5,7,...), n comes from argv with a default, and centering on the width of the widest line. I'll start the child and report the brief to the person in the same step.
act: engine_start(name="dev-tree-script", template="developer", root="/Volumes/Backup/dry-build/loop-run/loop3/proj", goal="A Python script that prints a centered tree of height n: each level has exactly 2 more '*' than the level above it (level 1: 1 star, level 2: 3 stars, ... level n: 2n-1 stars), every line centered.", scope="Create only the one script in the project folder. Stop and reply if the folder does not exist or the task conflicts with what is there. Never commit, push or publish.", done="Script exists in the project folder; running it (e.g. `python3 tree.py 5`) prints 5 centered levels with 1, 3, 5, 7, 9 stars. Reply with: Changed / Ran / Saw / Unverified / Also changed.", text="Folder /Volumes/Backup/dry-build/loop-run/loop3/proj. Spec: n comes from the command line (sys.argv), defaulting to 5 if not given. Level 1 has 1 star; each level adds 2 (level k has 2k-1 stars). Center alignment: each line centered on the width of the widest line (the last level). Filename: tree.py. No new dependencies."); reply(text="I'm starting a developer child to make the tree script. Brief: goal — a Python script printing a centered tree of height n, each level with exactly 2 more '*' than the level above (level 1: 1 star; level n: 2n-1 stars). Scope — only the one script in /Volumes/Backup/dry-build/loop-run/loop3/proj, never commit/push/publish. Done — `python3 tree.py 5` prints 5 centered levels with 1/3/5/7/9 stars, and the child returns Changed / Ran / Saw / Unverified / Also changed. Context — n comes from the command line (default 5), level 1 has 1 star, each level adds 2, lines centered on the widest line, no new dependencies. I'll report the result when the child replies.")

### tools

engine_start(name="dev-tree-script", template="developer", root="/Volumes/Backup/dry-build/loop-run/loop3/proj", goal="A Python script that prints a centered tree of height n: eac…", scope="Create only the one script in the project folder. Stop and r…", done="Script exists in the project folder; running it (e.g. `pytho…", text="Folder /Volumes/Backup/dry-build/loop-run/loop3/proj. Spec: …") → made dev-tree-script from template developer with your goal and started it; a message comes when it replies

### user

engine dev-tree-script replied: "Changed: tree.py — new file: prints a centered star tree of height n; n comes from sys.argv[1], default 5; level k has 2k-1 stars; each line centered on the widest line (width 2n-1, the last level). Standard library only.

Ran:
- python3 tree.py 5 — exit 0
- python3 tree.py — exit 0 (default, identical output)
- python3 tree.py 1 — exit 0, prints '*'
- python3 tree.py 7 — exit 0
- python3 tree.py…" (1025 characters; all of it is in the children artifact) (engines children)

### assistant

thought: A developer child (dev-tree-script) replied with its report for the tree script. Per my instructions, the first step when a child replies is to load the judging skill alone, then judge the reply against the brief.
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

thought: The developer's report holds. Every done check has evidence: `python3 tree.py 5` exit 0 with the tree shown (1/3/5/7/9 stars, each line exactly 9 chars, centered); default run identical; n=1 and n=7 run; error cases (bad arg, n=0) handled with exit 2 and messages; a repr/len/count check confirms exact widths and star counts. Scope held ("Also changed: nothing", only tree.py). Unverified: nothing. This is a small new thing (one script), so no reviewer is needed. I'll accept and report.
act: reply(text="Done — tree.py is in /Volumes/Backup/dry-build/loop-run/loop3/proj. It prints a centered tree of height n: level k has 2k-1 stars (level 1: 1, level 2: 3, ...), each line centered on the widest line (the last level). n comes from the command line (sys.argv[1]), default 5; standard library only.\n\nRan:\n- python3 tree.py 5 — exit 0\n- python3 tree.py — exit 0 (default, identical output)\n- python3 tree.py 1 — exit 0, prints '*'\n- python3 tree.py 7 — exit 0\n- python3 tree.py abc — exit 2, 'usage: tree.py [height]'\n- python3 tree.py 0 — exit 2, 'height must be at least 1'\n- repr/len/count check (n=5 and n=7) — exit 0, exact widths and star counts\n\nSaw (python3 tree.py 5):\n    *    \n   ***   \n  *****  \n ******* \n*********\nEvery line exactly 9 characters, centered, 1/3/5/7/9 stars. n=7 gives 13-character lines with 1/3/5/7/9/11/13 stars, centered.\n\nUnverified: nothing. Also changed: nothing.")

