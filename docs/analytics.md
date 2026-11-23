# Analytics

Built-in analytics: every runner writes **spans** as engines run, and the
views and the `analytics(query)` tool are queries over them. Nothing here
is a metric counter; a number on a page is always computed from spans.

```text
engine runs ─▶ Runner / Trace closes spans at the shell's times
            ─▶ Tasks places each turn under its task, adds task + wait spans
            ─▶ appended to engines/<id>/spans.jsonl          (host: the one writer)
            ─▶ Timeline · Home today · /analytics · analytics(query)
```

## Spans

`dry_core::spans::Span { kind, id, parent, engine, start_ms, end_ms, attrs }`,
one JSON line each. Times are unix milliseconds from the shell's clock;
the core never reads a clock. Spans are never in the history and never in
the prompt.

| kind | what | attrs |
|---|---|---|
| `task` | one piece of work, over as many turns as it took | `goal`, `by` (the first turn's trigger), `outcome`, `turns`, `parent_engine`; a session's sub-agent: `agent: true` |
| `turn` | one turn | `trigger` (user / callback / post / schedule / answer), `outcome` (ready / failed / missing / waiting), `steps`, `goal`, `asked`, `ask`, `ask_note`, `show`, `waiting_on`, `error` |
| `step` | the model's answer and its calls | `step`, `phase`, `calls` |
| `llm` | one model call | `model`, `provider`, `purpose` (step / compress / session), `prompt_tokens`, `completion_tokens`, `tokens_est`, `cost_usd`, `ttft_ms`, `first_component_ms`, `compressed`, `error` |
| `tool` | one call | `name`, `mode` (instant / waiting), `ok`, `error`, `result_chars`, `step` |
| `wait` | between turns | `on` (child / user ask), `detail` |
| `approval` | a tool use asked of a person | `tool`, `decision`, `decided_by`, `person_ms` |
| `evidence` | a check with screenshots | `shots`, `changed_pct`, `pass` (the hook: `spans::evidence`; nothing calls it yet) |

The tree: task → turn → step → llm / tool; waits sit under the task, a
session's tool calls under the turn (or under their sub-agent's task).

**Triggers** are read from the newest message (`spans::trigger_of`): a
callback line the app wrote, an answer to the last reply's ask, a message
from the engine's parent (post), else the person. A resumed runner
(`Runner::resume`) is a callback.

**Tasks** (`spans::Tasks`, kept by each shell) cross turns: a turn that
ends `waiting` (engine_call) leaves its task open with a wait on the
child; a reply that asks leaves it open with a wait on the user ask; a
callback or answer turn goes on with it (and closes the wait); a new user
message while one is open closes it as `left`.

**Tokens and cost.** From the body's `usage` (Chat Completions, Responses,
and a CLI model's `cost_usd`, `cli::completion_body`). A server that sends
no usage (a stream without it) gets an estimate: the measured prompt and
the answer's characters / 4, marked `tokens_est`.

## Where they are recorded

| shell | how |
|---|---|
| host (`crates/server/src/host.rs`) | `Runner::next_at` with the wall clock; waits timed per call (`dry_tools::live::run_waits_timed`); first token and first component from the stream; session programs through `SessionTrace` (each tool_use → result a tool span, `Task`/`Agent` a sub-task, the result line's usage and cost an llm span); approvals when decided (person, rule or timeout) |
| window (`crates/gpui/src/spans.rs`) | its own loop calls the same `Trace` at the same points; only when no host runs (the host writes otherwise) |
| browser (`crates/web/src/app.rs`) | `next_at` with `Date.now()`; the log is a text in the TextStore (IndexedDB) at the same path, written whole after each move |
| read-only server | reads the logs from disk for its pages |

The runner's API for a shell: `next_at(state, cx, came, now_ms)` (plain
`next` uses `cx.clock` seconds), `first_token(now)`, `first_component(now)`,
`wait_times(Vec<(start, end)>)`, `spans()` to drain.

## Storage and retention

- `engines/<id>/spans.jsonl` beside the engine's files, append-only. The
  md store never removes it; a TextStore save never removes a `*.jsonl`
  (`store::is_log`).
- Days older than the retention roll into `engines/<id>/spans-daily.jsonl`
  (one line per engine and day: tasks, turns, calls, errors, ms, tokens,
  cost). The host rolls at start and when the day changes; the browser at
  load. Retention: `DRY_SPANS_KEEP_DAYS` (default 30).

## Queries (`dry_core::analytics`)

| query | answers |
|---|---|
| `time` | work (llm + tool) vs wait (wait + approval) and the part on a person; task time |
| `models` | per model: calls, errors, tokens in/out, tokens/s (after the first token), TTFT p50, cost, cost per task |
| `tools` | per tool: count, failures, p50/p95 of the waiting calls; `never_used` against the toolboxes; `slowest` |
| `phases` | steps and time per phase; `loops(spans, "test", "triage")` |
| `context` | compressions, mean prompt growth per step within a turn, largest prompt |
| `asks` | per ask kind: replies and how many fell back with a note |
| `you` | time to answer asks and approvals (p50), turns you started by hour, active hours |
| `messages` | posts per thread, who → whom (`no addressee` for a post with no `to`), post → addressee's next turn (p50), from `state.shared` |

## Views

- **Engine page → Timeline** (`{"type": "analytics", "part": "timeline"}`):
  the last three turns as a waterfall (bars tinted with the activity
  tokens: blue model, purple tools, amber waits, yellow approvals, red
  failures) and the engine's totals.
- **Home → today** (`"part": "today"`): work, waiting, cost, work share,
  slowest tools today.
- **/analytics** (`routes/analytics/page.json`, in the nav before
  Settings; Settings links to it): every query as tables, stats and bars,
  `?days=1|7|30`; rolled-up days as their own table.

The view models are renderer-neutral (`analytics::timeline`, `today`,
`page` return `show::Show` values and `Waterfall` bars); HTML draws them
in `crates/html/src/render/analytics.rs`, GPUI in
`crates/gpui/src/views/analytics.rs`.

## The learning tool

The learning artifact (`kind: learning`) brings `analytics(query)`, with
`query` one of tools, models, time, phases, context, asks, over the
engine's own spans of the last 7 days. It answers one line
(`this week: tool run failed 4/10; 5 tools used`) and writes the facts
into the artifact, which its snapshot shows under `#### analytics`; a
`collect` keeps them. Spans reach the core through `Host::spans`
(the disk host reads the log; the browser's host says it keeps none yet).

## Not yet

- The browser's `analytics` tool: its host does not hand the core the
  spans it keeps (the companion shell owns that seam).
- An approval asked while its turn runs has no parent (its task opens
  when the turn ends).
- GPUI session turns (`ask_session`) are not traced; the host's are.
- `schedule` triggers: alerts do not start turns yet.
- The evidence kind is defined; the evidence runner does not emit it yet.
