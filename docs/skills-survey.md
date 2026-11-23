# Engineering skill sets: a survey (2026-11-23)

The owner asked for "pstack and Matt Pocock skills ... working skills for an
engineer" as the source for dry's default (docs/default.md). Three sets were
read, from their repositories, on 2026-11-23. All three are MIT. dry's skills
are written fresh from their ideas, in dry's plain style. Each skill credits
the ideas it took. No text is copied.

| set | where | licence | shape |
|---|---|---|---|
| gstack (Garry Tan) | github.com/garrytan/gstack | MIT; its NOTICE.md also credits Apache-2.0 and MIT sources, pstack among them | about 70 skills compiled from templates, with a large shared preamble, Bun helper binaries, telemetry and question menus |
| pstack (Lauren Tan, "poteto") | github.com/cursor/plugins, folder `pstack` | MIT (`pstack/LICENSE`) | about 60 skills: a hub (`poteto-mode`), about 25 short `principle-*` skills, and engineering, prompting and review skills |
| Matt Pocock's skills | github.com/mattpocock/skills | MIT | small, composable SKILL.md files (7–140 lines) that call each other |

"pstack" is real: it is the Cursor plugin, and gstack credits several of its
ideas to it. gstack is the one people usually mean by "Garry Tan's stack".

## What each set does well

### gstack

- **plan-eng-review**: a scope gate first. It asks what already solves this,
  what the smallest change is, and stops for cuts past about 8 files or 2
  new services. Then architecture, code quality, tests and performance,
  never skipped. "Design twice": a second, structurally different shape
  before choosing. Findings carry a severity and a confidence. If you
  cannot quote the line behind a finding, its confidence is capped.
- **review**: a narrow blocking floor. A finding blocks only if it would
  fail a test, break production, or lose data or secrets. Blocker / should
  / nit. Mechanical fixes are made at once, judgment calls are asked
  about. "No findings" is a valid result.
- **qa**: baseline, triage, then a fix loop per issue: reproduce, regression
  test first, the smallest fix, re-test. Stop rules: review every 5 fixes,
  stop after 2 reverts.
- **build-check**: run only the checks the repo declares, never invented
  ones. Typed verdicts. An environment failure is reported, never worked
  around. Never weaken a test. Stop when the same failure comes back twice.
- **spec / office-hours**: verify the current behaviour before planning,
  then ask "who, now, desired, why now, how we'll know".
- **captain**: a fixed report a sub-agent must return, including what is
  unverified, failed, or changed beyond the ask.
- **ETHOS**: attack the premise, search before building, explain the number.
- **Caution**: its own November 2026 test on 100 everyday bugs found no gain
  in fix rate from the skills. They help workflow, review and safety, not
  raw ability. Its machinery (preamble, telemetry, binaries) is the
  opposite of dry's few-tools rule.

### pstack

- **architect**: two shapes before any code.
- **interrogate**: a finding survives only if a rerun command proves it.
  Style and "might break someday" are out.
- **sharpen**: make a task deeper, not bigger. Ask four questions: what would
  a top professional add? What is the rawest source? Which methods would
  they use? What outside you shows it is right? Settle by experiment what an
  experiment can settle. Only preference goes back to the person.
- **Delegation rules (poteto-mode)**: pass pointers, not pasted context. A
  fresh agent for each retry, carrying the old report. The parent reviews
  everything.
- **principle-\***: one idea each, 10–20 lines. Prove it works, fix root
  causes, test behaviour not implementation, subtract before you add.

### Matt Pocock

- **AGENT-BRIEF** (in `triage`): the clearest brief standard of the three.
  It names contracts (types, functions, behaviour), not file paths or line
  numbers. Current → desired. Acceptance criteria that can each be checked
  on their own. Out of scope said out loud.
- **diagnosing-bugs**: phase 1 is the skill. Build a loop that goes red on
  the bug, deterministic and fast, before any hypothesis. Then minimise,
  rank 3–5 falsifiable hypotheses, change one variable at a time, and write
  the regression test first (only at a real seam).
- **tdd**: test at public seams agreed first. Vertical slices, not all tests
  first. No tautological tests.
- **code-review**: two separate reviewers, standards and spec, never merged.
  So one axis cannot hide the other.
- **triage**: a label state machine. Checks that the work is already done or
  was rejected before. Reproduce the claim first.
- **grilling**: ask the whole frontier of settled-prerequisite questions,
  each with a recommended answer. Facts are the agent's job, decisions the
  person's.
- **writing-for-agents**: wording decides whether an agent reaches material.
  Every step ends on a clear completion criterion. Positive rules. Prune
  no-op lines.

## What fits dry's phases

dry's developer loop is triage → code → test & critique, run by a child
the translator makes (docs/default.md). The fit, and the dry skill each one
became:

| dry skill (phase) | ideas taken | from |
|---|---|---|
| `prompting` (translator) | contracts not paths, current → desired, checkable done criteria, out of scope; think deeper first; a fixed report with Unverified / Also changed; the same child for a send-back, a fresh one with the old report for a new approach | Pocock AGENT-BRIEF, writing-for-agents; pstack sharpen, poteto-mode; gstack captain |
| `judging` (translator) | evidence for every claim; "no findings" vs "not verified"; send back what is missing; ask the person after the same failure twice | gstack review lead judgment; pstack interrogate |
| `triage` | verify today's behaviour before planning; check it is not already done; reproduce the claim | Pocock triage; gstack spec |
| `plan` | smallest change, scope trip-wire, design twice for structural work, a test that fails today, public seams | gstack plan-eng-review; Pocock tdd, to-spec |
| `code-style` | the repository's style wins; no drive-by edits; never weaken a check | gstack build-check, review |
| `test` | declared checks only; run it the way its user would; report the environment, never dodge it | gstack build-check, qa |
| `browser-qa` | use the page like a person, read after each step, record the exact text | gstack qa, browse |
| `critique` | findings need proof; blocker vs the rest; against the ask and the plan separately; fixed pass report and fail notes | gstack review; pstack interrogate; Pocock code-review |
| `debugging` | a red loop first, minimise, falsifiable hypotheses, one change at a time, stop after the same failure twice | Pocock diagnosing-bugs; gstack qa stop rules |

Left out on purpose:
- the shipping and deploy skills (dry never commits or publishes unless the
  person asks);
- the multi-model and arena skills (one model per engine for now);
- telemetry and the question menus;
- the label state machines, which need an issue tracker.

The interview style (`grilling`, `office-hours`) is folded into the
translator's rule: ask one question only when only the person can answer it.

## Measured in dry (2026-11-23)

The first runs of the default on a scratch project (docs/default.md,
"Proof"). With claude/haiku as the model, the loop worked end to end, but:
- the developer did edits in triage and skipped the code phase;
- it once replied in the same act as its runs, so the report was written
  before the results came back;
- it twice wrote "not a git repo" without running git, and the translator
  accepted it.

The skills were tightened for each of these: reply alone, after the runs;
no edit in triage; a fact needs a command. The judging skill says to read
the whole reply in the children artifact, not the cut message. With
claude/sonnet, see docs/default.md.

## Two coding loops: Open SWE and VS Code agent mode (2026-11-23)

Read on 2026-11-23 for the developer setup: LangChain's Open SWE
(github.com/langchain-ai/open-swe, its launch post and its `AGENTS.md`)
and VS Code's agent (github.com/microsoft/vscode-copilot-chat, the prompts
under `src/extension/prompts/node/agent/`, and the VS Code planning docs).
Both MIT. Summarised here in dry's words; nothing is copied.

### Open SWE

- **Four roles.** A manager takes the task and hands it on. A planner reads
  the code (views files, searches) and writes a step-by-step plan. A
  programmer carries the plan out in a sandbox: edits, runs tests, looks up
  docs. A reviewer, under the programmer, checks the result for
  correctness and completeness, runs the tests and formatters, and sends it
  back with feedback until it approves. Then a summary and a pull request.
- **Plan approval.** By default the person sees the plan first and can
  accept it, edit it, cut parts, or send the planner back to dig more.
  Skipping the review is allowed.
- **Sandbox.** Each task has its own isolated machine, so commands need no
  approval one by one.
- **Steering while it runs.** A new message mid-run changes the task
  without a restart.
- **Its own lesson.** The authors say planning and review are too much for
  a small fix; their lighter version chooses whether to plan or review.
- **Today** (renamed Robin, on Deep Agents): the reviewer is its own run
  kind, told never to commit or push, and chat on a pull request gets no
  tools that change things.

### VS Code agent mode

- **Context first.** Gather what is needed before acting; read large
  pieces rather than many small ones; many reads at once; never re-read
  what is already in context.
- **Keep going.** Call tools until the task is done; do not stop to ask
  what to do next, and do not end with an open to-do item.
- **Error-driven edits.** After an edit, the editor's errors come back with
  the result. Fix the ones the edit caused and confirm they are gone. After
  three failed tries on the same file, stop and ask the person.
- **A to-do list** for anything multi-step, kept current as each step
  finishes.
- **Two narrow helpers.** A search helper that only reads and returns
  file paths with line ranges, and an execution helper that runs commands
  and returns one line per command with its result (and the error lines on
  a failure). Both keep the main context small.
- **Plan agent.** Researches without editing, asks clarifying questions,
  and returns steps, the files involved and how to verify. The person
  comments on it, then implements, approves only, or rejects.
- **A turn cap** with a "continue?" prompt. dry leaves this out: steps are
  never capped (the owner's rule).

### What dry took (2026-11-23)

| idea | from | where in dry |
|---|---|---|
| a read-only explorer that returns file:line findings, each confirmed | VS Code search helper and Plan agent; Open SWE planner | `researcher` template |
| a separate reviewer that reruns the proof and sends back or accepts; never commits | Open SWE reviewer; Pocock `code-review` (two axes); gstack `review` | `reviewer` template |
| plan approval before code, skipped for small fixes | Open SWE plan review and its own lesson; VS Code Plan agent | developer triage `plan first`; translator `judging` |
| a step list kept current, none left open at the end | VS Code to-do list | the developer's `plan` artifact, `## Steps` |
| a quick compile check after each saved file, three tries then back | VS Code errors after edits | developer `code` phase, `code-style` |
| compact reports: one line per command with its result | VS Code execution helper | every template's report |
| the translator joins the roles, not a manager engine | Open SWE manager | `translator` (no new engine) |

Left out: the turn cap and "continue?" prompt, the sandbox per task (dry
runs in the project root; the host approval step is still planned), the
pull request and issue tracker flow, and a separate manager.
