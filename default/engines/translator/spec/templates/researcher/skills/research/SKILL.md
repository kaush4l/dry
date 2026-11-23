---
name: research
description: Answer a question about code by reading it from its entry point and confirming what you read by running it; return findings as file:line, each with its proof. Changes nothing.
---
# Research: how it works, confirmed

You are asked so that someone else can act without reading it all again.
An answer they can trust names the lines and says how you know.

1. **The question in one line.** What is asked, and what the answer is for
   (a fix, a plan, a decision). If the question has two readings that the
   code does not settle, reply with both and stop.
2. **Lay of the land.** `git log --oneline -5`, `ls`, and how the project
   builds and tests itself (`package.json`, `Cargo.toml`, a `Makefile`, the
   README). Use only the commands it declares.
3. **From the entry point.** Find where the behaviour starts with
   `grep -rn` on a name, a route or a message the user sees. Open it and
   follow the calls to where the behaviour happens. Open several files in
   one step. Read whole functions, not a line out of each.
4. **Confirm it.** A claim about behaviour needs a run behind it: the test
   that exercises it, the program with the input, the page in the browser.
   Reading alone gives "read, not run", and the answer says so.
5. **Write as you go.** Each finding in the findings artifact:
   `path:line — what it does — confirmed by <command> (<what it showed>)`.
   Rewrite the artifact whole, keeping what was there.
6. **Stop at the answer.** Do not explore past what the question needs.
   When something cannot be found (no test, no running app), that is a
   finding too.
7. **Change nothing.** No edit, no save, no install, no `git checkout`,
   `stash`, `reset` or `commit`. A server you started with `background` is
   stopped before you reply. Last, run `git status --short` (when it is a
   git repo): its output goes in the answer as Tree, so "changed nothing"
   has a command behind it.

## The answer

Reply once, after the runs it quotes:

```text
Answer: the question's answer in a few sentences
Findings: each as path:line — what it does — how confirmed (in flow order)
Ran: each command, with its exit code and last line
Unverified: what was read but not run, or could not be found, and why
Next: what someone changing this should know first (or "nothing")
Tree: the last `git status --short`, unchanged by you (or "not a git repo", with the command that showed it)
```

Ideas from VS Code's search helper (return paths with line ranges, not
pasted code) and Plan agent (research without editing), and Open SWE's
planner (read the code before any plan), all MIT; written for dry.
