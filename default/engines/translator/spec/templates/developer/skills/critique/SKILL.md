---
name: critique
description: Judge the result against the plan and the ask before replying - only findings you can show count - then pass with the report, or fail back to triage with notes.
---
# Critique: is it right, shown?

Review your own work as a stranger would. You have the plan artifact, the
terminal output, the page or evidence, and the saved files.

1. **Against the ask.** Does the behaviour now match the desired line of the
   plan, exactly? Quote the output or page text that shows it.
2. **Against the plan.** Every step is ticked (`- [x]`) and was made and
   saved. Nothing
   outside it changed: run `git status --short` and `git diff --stat` and
   compare with the plan's list.
3. **Against the code around it.** Same style, no debugging prints left,
   no new warnings in the build output.
4. **Findings need proof.** A finding counts only if you can point at a
   line or rerun a command that shows it. Style preferences and "might
   break someday" are not findings. No findings is a complete result.
5. **Severity.** A blocker fails the test (a wrong result, a failing check,
   lost data, a leaked secret). Anything smaller goes in the report and
   does not block.

**Pass**: reply, in a step of its own after the runs it quotes (never in
the same act as a run: the report quotes results you have read), with

```text
Changed: each file, one line on what changed
Ran: each command, with its exit code and last line
Saw: what the running app or output showed, in its own words
Unverified: what you could not check, and why (or "nothing")
Also changed: anything outside the plan (or "nothing")
```

**Fail**: write the notes, keeping what was there:

```text
## Notes
### test <n>: failed
- what failed: the command or step
- output: the exact lines
- why, as far as you can tell
- next: what triage should look at
```

Then `goto_phase(name="triage")`.

Ideas from gstack's `review` (blocker / should / nit, "no findings" is a
result), pstack's `interrogate` (proof by rerun) and Matt Pocock's
`code-review` (judge against the spec and the standards separately), all
MIT; written for dry.
