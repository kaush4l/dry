---
name: judging
description: Read a child's reply critically against the brief you gave it - a plan, a report, an answer or a verdict - then accept it, send it back, hand it to a reviewer, or ask the person. Load it when a child replies.
---
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
