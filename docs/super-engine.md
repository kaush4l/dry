# The super engine (direction)

Owner, 2026-11-22. Direction, not built yet.

## Where we are

The app's state is a flat list of engines and sessions; the user picks
one and talks to it. Children exist (the `engines` artifact: `engine_call`,
`engine_start`), engines of a level share a context (`kind: shared`), and
phases narrow tools and skills.

## Where we go: one super engine

Every current app is one user, many sessions: the user holds the ideas
and opens a thread for each. dry is a personal bot with a super agent:

- **The app holds one super engine.** Every other engine, session and loop
  is inside it (its children, at its level or below).
- **The super engine is the app's maintainer, not an answering
  assistant.** Its one job is to translate the user's goals into app
  actions: make an engine with a goal, send a message to the right thread,
  start a loop, switch a model for cost, set an alert, add a page.
- **It has the prompting skills** to turn a request into a structured
  prompt: a child's `goal.md` (Goal, Scope, Done), a template, the skills
  and artifacts it needs.
- **Even answering is delegated.** Answering is an engine of its own, with
  a phase-based template and skills curated for answering only. Not every
  phase has every skill: the template's `skills` column already narrows
  them per phase.
- **Communication:** children report through their replies and the shared
  context of their level; the super engine reads them in its `engines`
  snapshot (the snapshot rule: always current, never in history).

## Multiple tool calls

- **Built (engine standard, [One step, many calls](engine-standard.md#3d-one-step-many-calls)):** one step's `act` holds every call it
  needs, and every call is invoked at once: instant state changes
  (`open`, `edit`, `save`, `post`, ...) apply first in written order, then
  every waiting call (`run`, MCP calls) is launched together, one thread
  each, and several `engine_call`s start their children together. The
  results come back as one message in written order. A call can still act
  on several targets where that is natural: `open(path="a.rs|b.rs")`.
- **Replaced:** the parked array-of-arrays idea (inner lists in sequence,
  the lists in parallel). Ordering inside an act is now by sort (instant
  before waiting); a sequence that needs order is a `script`, or the next
  step.

## Built: the translator (2026-11-23)

The shipped default is a first super engine: the translator, with no
action tools, a `prompting` and a `judging` skill, and a `developer`
template it makes children from ([default.md](default.md)).

## Open questions

1. Does the user still talk to a child directly (open its page and type),
   or only through the super engine?
2. The super engine's own tools: the `engines` artifact, the settings
   tools, the app's tools (routes, pages, themes), alerts. Which first?
3. Is the super engine a fixed, shipped spec (the app's default), editable
   like any engine, or locked?
4. Where it runs: on the host (the server goal), so it keeps working with
   the window closed.
