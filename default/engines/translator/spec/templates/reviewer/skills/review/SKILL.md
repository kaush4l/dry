---
name: review
description: Review a change you did not make, against its brief and against the code's standards, by rerunning its proof; accept it or send it back with blockers. Changes nothing.
---
# Review: is the change right, shown?

The author's report is a claim. You decide whether it holds, with what you
see and rerun yourself, and you leave the tree exactly as you found it.

## Read

1. **The brief.** The goal, scope and done the author was given are in
   your instructions. Each done line is one check.
2. **What changed.** `git status --short`, `git diff --stat`, then
   `git diff -- <path>` per file. Open each changed file at those lines:
   a diff shows the change, the file shows what it lands in. No git: the
   report's Changed list, and the files it names.
3. **The neighbours.** Open one file like each changed one (the same
   folder, the same kind). The change should read like it.

## Rerun

4. **The proof, again.** Every command the report quotes, run here. The
   project's declared checks (tests, build, lint) as the project declares
   them. The program run the way its user runs it. If an output differs
   from the report, that is a finding, with both outputs.
5. **The test is real.** It calls what the user or caller calls. Read its
   assertion against the done line: would it fail on the old code? Say how
   you know (the old line from `git diff`, or "not verified"). Then list
   each changed file that holds behaviour: a file with no test that
   reaches it (only a manual run) is a "should", named with its lines.
6. **Read-only.** Your code and terminal artifacts are read-only (no
   edit or save; a command known to write is refused), but the guard is
   word by word, so it is still on you: no install, no formatter that writes,
   no `git checkout`, `stash`, `reset` or `commit`. A server you started
   with `background` is stopped before you reply.

## Judge, two axes apart

- **Against the brief**: each done line holds or fails, with its proof.
- **Against the standards**: scope, style of the code around it, a real
  test, nothing left behind.

Keeping them apart stops a clean diff from hiding a missed ask, and a met
ask from hiding a mess.

A finding counts only when a line or a rerun shows it. "No findings" is a
complete result. A blocker fails the change: a done line that does not
hold, a failing check, lost data, a leaked secret, an unexplained change
outside the scope. Everything else is a "should" and does not block.

## The verdict

Reply once, after the runs it quotes:

```text
Verdict: accept | send back
Blockers: each as file:line — what is wrong — the command or line that shows it (or "none")
Should: smaller findings, same form (or "none")
Ran: each command, with its exit code and last line
Unverified: what you could not check, and why (or "nothing")
Tree: the last `git status --short`, unchanged by you
```

Send back when there is one blocker or more. The blockers are written so
the author can start from them without asking you anything.

Ideas from Open SWE's reviewer (rerun the tests, send back until it
holds, never commit), Matt Pocock's `code-review` (spec and standards as
two reviewers), gstack's `review` (a narrow blocking floor, "no findings"
is a result) and pstack's `interrogate` (a finding needs a rerun), all MIT;
written for dry.
