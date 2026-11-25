---
name: prompting
description: Turn an ask into a brief for a sub-engine - which template, its goal, scope, done criteria, the context it must read first and the evidence it must return. Load it before every engine_start or engine_call that makes a child.
---
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

Your engines artifact lists the templates with their phases:

| the ask | template | why |
|---|---|---|
| a question a model can answer from what it knows, or any answer asked for in a style or format (a table, three bullets, for a five-year-old, JSON only) | `answer` | it has the components skill and nothing else; its brief is the question as asked, the style asked for, and any facts from this conversation it needs. No plan, no reviewer; you pass its reply on as it is |
| a question about a project's code: how does it work, where is it, why does it do that | `researcher` | it reads and runs, and has nothing to change |
| a question about the world that needs sources, or facts newer than a model knows: a person, a product, a method, what is known about something | `deep-research` | it searches the web, reads pages into files, and leaves notes.md and summary.md with a source for every fact |
| a change or a bug, and you know where it lives | `developer` | triage, plan, code, test, in one child |
| a change in a project or area nobody here has read, or one big enough to need a plan you can judge | `researcher` first, then `developer` with the findings in `text` | the plan starts from confirmed lines, not guesses |
| a change a developer finished that is more than a small fix | `reviewer`, with the brief and the developer's report | a second reader who did not write it, rerunning the proof |
| something to build that rests on facts nobody here has (a character from a film, a tool for a format, a comparison of libraries) | `deep-research` first, then `developer` with the research folder's files named in `text` | the build starts from sourced facts, not from what a model remembers |

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
| `template` | `researcher`, `deep-research`, `developer` or `reviewer` (above) |
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
the same way (a researcher, a deep-research child and a reviewer have
their own forms, in their templates; name them in `done`: "Reply with the
answer form" / "Reply with the verdict form"):

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

## 8. Example: research on the web, then a build on it

The person says: "Build Jack Sparrow as a character that runs on this
machine." Two deliverables, the second needs the first. The plan first,
then the research alone:

```text
write(name="plan", text="## Plan\n- [ ] research-sparrow (deep-research): notes.md, summary.md and lessons.md in characters/jack-sparrow/research, every fact with a source\n- [ ] dev-sparrow (developer, persona skill): card, data, a tuned model and its eval in characters/jack-sparrow, after the research")
engine_start(name="research-sparrow", template="deep-research", root="characters/jack-sparrow/research",
  goal="What is known about how the film character Jack Sparrow speaks, behaves and decides, so a developer can write his character card and new lines in his manner.",
  scope="Read and note only. Summarise in your own words: copy no script line, lyric or long quote. Stop when the sub-questions are answered or marked open.",
  done="notes.md, summary.md and lessons.md in the root; every fact with its url; the question session done; open questions marked. Reply with the answer form.",
  text="Sub-questions to start from: who created and played him and what they said they built him from; how his speech is described (pace, vocabulary, humour); his values and habits; what he never does. Use web.wiki for the encyclopedia page, then interviews and named critics.")
```

When its answer is judged good, tick its line and start the build, with
the files named:

```text
engine_start(name="dev-sparrow", template="developer", root="characters/jack-sparrow",
  goal="A Jack Sparrow persona that answers in his voice on this machine: a character card, original training lines, a small model tuned here and an eval that shows it beats the untuned model.",
  scope="Load the persona skill first and follow it. Facts only from research/notes.md and research/summary.md. Every line written new: never copy a film line. One training run at a time. Never commit, push or publish.",
  done="card.md; data/train.jsonl and data/valid.jsonl; an adapter under adapters/; eval/base.md and eval/<run>.md scored; runs.md with a row per run; in voice at least 4 of 5 and above the base; no copied line. Reply with Changed / Ran / Saw / Unverified / Also changed, with the best row and the base row.",
  text="The research is in research/ under your root: open research/summary.md and research/notes.md first. Optimise for quality first; note the training seconds of every run.")
```

Deliverables that do not need each other (five characters, each with its
own research) start together: several engine_start calls in one act,
each with its own root. A child that waits for another's files is
started when those files are there.

## 9. Sending it back

When a reply misses something, send the same child a message (engine_start
with its name and a `text`). The message names exactly what is missing ("Ran
has no test output; run the tests and reply again"). The child keeps
everything it has read. Make a new child only for a new approach, and give
it the old child's report in `text`.

## 10. Posts on the thread

A post is a brief too. Name `to` (an engine's id, or `you`) and write it so
the addressee can act without having seen anything else: what, why, what
done looks like, where the material is. A post with no `to` reaches nobody
and comes back to you.

Ideas from Matt Pocock's AGENT-BRIEF and writing-for-agents, Lauren Tan's
pstack `sharpen`, Garry Tan's gstack `captain` READY block, and Open SWE's
planner, programmer and reviewer roles (all MIT).
The text above is dry's own.
