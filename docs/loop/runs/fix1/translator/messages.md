### user

Write a python script to print a tree of height n where each level has 2 more '*'. Make it center aligned. Work in the folder /Volumes/Backup/dry-build/loop-run/fix1/proj.

### assistant

thought: The user wants a Python script in /Volumes/Backup/dry-build/loop-run/fix1/proj that prints a tree of height n, each level having 2 more '*', center aligned. I have no file tools myself, so this goes to a developer child engine. First I must read the prompting skill to know how to write the brief.
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
  only the person can settle it, ask before you start. If an experiment can
  settle it, the child tries it.

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
than they find on a small fix.

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
| `root` | the project folder, exactly as the person gave it |

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

thought: The ask: a Python script in /Volumes/Backup/dry-build/loop-run/fix1/proj that prints a tree of height n, each level having 2 more '*', center aligned. The folder is given. Two contract details are not settlable by reading: what the first level contains (e.g. 2 or 1 '*') and how n is supplied (command-line argument vs hardcoded). I'll ask one form before starting a developer.
act: reply(ask="form", title="Tree script", fields="First level stars:number(1-5)|n comes from:choice(command-line argument,hardcoded)", text="A few details for the tree script:")

