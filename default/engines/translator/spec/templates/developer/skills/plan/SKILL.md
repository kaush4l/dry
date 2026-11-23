---
name: plan
description: Write the change as a plan artifact - what is true now, what changes in which file, and the test that proves it - small enough to check, before any edit.
---
# Plan: the change, before it is made

Write it with `write(name="plan", text="...")`. The plan is what the code
phase follows and what the test phase judges against, so make it concrete.

```text
## Ask
<current behaviour> → <desired behaviour>

## Now
- file:line — what it does today (one line each, only what matters)

## Steps
- [ ] 1. path — what changes, and why
- [ ] 2. path — the test: what it checks

## Proof
- `command` — passes when <what it shows>
- run / look: what the running app or output should show

## Out of scope
- what you leave alone
```

Rules:

- **The smallest change that does the ask.** If it touches more than about
  five files, or adds a dependency, say why in the plan, or cut it down.
- **Two shapes for anything structural.** When the change adds a function,
  a type or a module, write two ways to do it in one line each and pick
  one with a reason. Skip this for a one-line fix.
- **A test that would fail today.** The proof has to tell before and after
  apart. A test that passes on the old code proves nothing.
- **Test behaviour, not insides.** Call what the user or caller calls (the
  public function, the command, the page), not private helpers.
- **Steps are the to-do list.** One `- [ ]` line per file or test, in the
  order the code phase does them. The code phase ticks each one (`- [x]`)
  when its file is saved and compiles; a step left open at the end is work
  not done.
- **Plan first, when asked.** If your instructions say "plan first", the
  plan is replied before any edit (the triage phase says how) and the code
  waits for the approval. A small fix (one or two files, nothing
  structural) is planned the same way but needs no approval unless asked.
- **One plan, kept current.** When the code phase learns the plan is wrong,
  the plan is rewritten in triage. It is never quietly worked around.

Ideas from gstack's `plan-eng-review` (scope check, design twice), Matt
Pocock's `tdd` and `to-spec` (public seams, vertical slices), Open SWE's
plan review and VS Code's to-do list and Plan agent, all MIT; written for
dry.
