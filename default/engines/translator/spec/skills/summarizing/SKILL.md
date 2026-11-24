---
name: summarizing
description: Write the closing summary of a goal - what was asked, each direction taken and why, what each child did with its numbers, what was learned, what is not done - as plain facts. Load it when every line of the plan is ticked, or when the person asks where a goal stands.
---
# Summarizing: the goal, as facts

The summary is what the person reads instead of the whole thread. They
want to know what happened and why, and to trust each line. So it holds
facts only: what was asked, what was decided and on what evidence, what
was made, what the numbers were, what failed, what is open.

## Where the facts come from

- **The plan artifact**: the deliverables and their marks.
- **The children artifact**: each child's goal and its whole last reply
  (the message that woke you shows only the start of it).
- **This conversation**: the person's words, your briefs, what you sent
  back and why, the order things happened in.

You hold no files. A number you need that no reply shows (a score, a
time, a count) is asked of the child that has it: engine_start with its
name and a `text` that names the number and the file it is in. Wait for
it, then summarise. Never fill a gap from what is likely.

## The form

Reply once, with these parts, in this order:

```text
Asked: the goal in the person's words, then in one line what done meant
Direction: each decision, in the order it was taken, one line each:
  <what was decided> — because <the evidence: a reply, a number, an error> — instead of <what was not done>
Work: one block per child:
  <child> (<template>, <model>): its goal in one line
  Result: done / done in part / failed, in one line
  Numbers: the ones its reply shows (counts, seconds, scores, exit codes), each with its unit
  Files: the paths it reports
Learned: what worked and what failed, one line each, with the run or reply that showed it
Not done or not verified: each open plan line, each Unverified a child reported, each thing nobody checked
Files: every path a child reported, grouped by folder
```

Keep it too: write(name="summary", text="<the same text>") in a step of
its own, before the step that replies. A reply ends your turn.

## Rules

1. **Facts only.** No adjectives of praise or blame ("great", "robust",
   "unfortunately"), no "successfully", no feelings, no advice unless
   the person asked for it. "r2 scored 4.1 in voice against 2.3 for the
   base (eval/r2.md)" is a fact; "the model captures his charm" is not.
2. **Every number has an owner.** It comes from a child's reply or an
   artifact, and the line says which. A number nobody showed is asked
   for, or listed under Not verified as missing.
3. **Nothing claimed that was not shown.** A child that said "tests
   pass" with no command is reported as "says tests pass; no command
   shown".
4. **Direction is the part only you can write.** Each time the work
   turned (a child sent back, a plan line added or dropped, a second
   approach, a model changed), say what turned it. A goal that went
   straight has a short Direction, and that is fine.
5. **Failures are results.** A run that crashed, a site that refused, a
   child that had to be replaced: each is a line under Work or Learned
   with its error in its own words.
6. **The person's words win.** Asked quotes them. If what was built
   differs from what was asked, the first line of Not done says how.
7. **Short lines, no nesting below the form.** A reader should find any
   fact by its part. Under about 6000 characters; a table with show=
   when the numbers of several children line up in the same columns.

## Example lines

```text
Direction:
  research before any build — because a persona's facts must have sources — instead of writing the card from memory
  r2 with 400 examples instead of 200 — because r1 scored 3.1 in voice with valid loss still falling (runs.md row r1) — instead of more iterations
Learned:
  the DuckDuckGo search answered 14 of 14 queries; two sites refused with 403 (research-sparrow, lessons.md)
  a 1.5B base trained in 212 s; the 4B base took 1140 s for +0.2 in voice (dev-sparrow rows r1, r3)
Not done or not verified:
  nobody talked to the tuned model outside the eval prompts
```
