---
name: triage
description: Find out how the thing works today and confirm it before planning a change - read the code from its entry point, run read-only commands, open every piece of evidence.
---
# Triage: how it works today

You cannot change what you have not seen work. Triage ends with a flow you
have confirmed, not a guess.

1. **Notes and the message first.** If the notes artifact holds a
   failure, that is your starting point: what failed, the output, what the
   test phase thought should change. If the message holds review findings
   (a blocker with its file:line and proof), add them to the notes as they
   are and start from them. If the plan artifact holds your plan and the
   message approves it, there is nothing to triage: go to code.
2. **Lay of the land.** `run(command="ls -la")`, and when there is a
   `.git` folder `run(command="git status --short && git log --oneline -5")`
   (where git is not installed, say so in the report and go on). Find how
   the project builds and tests itself:
   `package.json` scripts, `Cargo.toml`, a `Makefile`, the README. Use only
   commands the project declares. Never invent one.
3. **Find the entry point.** For the behaviour asked about, find where it
   starts (a route, a command, a function the test calls) with
   `run(command="grep -rn 'name' src")`. Open it, and follow the calls to
   where the behaviour happens. Open several files in one step
   (`open(path="a|b")`). Use `focus` on the lines that matter in a big file.
4. **Confirm it runs the way you think.** Run the existing tests as they
   are now, and run the program or open the page. A claim about today's
   behaviour needs a command or a file:line behind it.
5. **Open all the evidence.** `evidence(op="read")` when the project has an
   `evidence.md`. The browser artifact's page when the app is running. The
   output of the last run in the terminal artifact. Read them all, not
   just the first.
6. **Check the ask is still needed.** Maybe it is done already, or the code
   says why it is the way it is (a comment, a test, a commit). If so,
   reply and say so instead of changing it.
7. **Restate the ask** in one line: current behaviour → desired behaviour.

If the ask has two readings and the code does not settle it, reply with
the question and the two readings. Do not pick one silently.

Ideas from Matt Pocock's `triage` and gstack's `spec` (verify the current
behaviour before you plan), both MIT; written for dry.
