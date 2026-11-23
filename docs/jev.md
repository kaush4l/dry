# JEV

JEV is a response standard of its own. It is not a way of reading a chat
model's text. A phase that says `response: jev` is meant to be answered by
a JEV endpoint: a fast decision maker for steps whose paths are known.
The first use case is a tool call's permission: approve, deny, or (rarely)
escalate.

The owner's words (2026-11-23): the response contract is tied to a model
today, and that is why it is long. For JEV, the value is different and the
check is different. Tools need more metadata, so that "jev possible" can be
marked. Calling a JEV-supported endpoint is still required. There are no
JEV-supported models locally.

Built: `dry_core::jev` (the metadata, the check, the step's JEV form),
`Rendered::jev` (`dry_core::prompt`), the `jev:` line in the prompt view
(`dry_core::inspect::slots`, shown by the html and gpui views), and the
`jev` flag in `models.json` (`dry_core::models`). Not built: calling a JEV
endpoint.

## What Jev is outside dry

The name exists. Jev is TypeSafe AI's "System One" decision model. It is
not a chat model. You send it a `state` and typed `questions`; it sends
back one typed answer per question, with probabilities. It writes no text,
calls no tools, and plans nothing: your code acts on its answer.

Where it is served:

- TypeSafe itself: `https://api.typesafe.ai`, route `/v1/systemone`. The
  model ids are `jev-latest`, `jev-preview`, and versions like
  `jev-1.13.0` and `jev-1.14.0`. No streaming. ([LiteLLM: TypeSafe](https://docs.litellm.ai/docs/pass_through/typesafe))
- OpenRouter: `POST https://openrouter.ai/api/alpha/decisions`, model
  `typesafe/jev-1.14`. ([OpenRouter: What is Jev](https://openrouter.ai/blog/insights/what-is-jev/))
- AI/ML API: `POST https://api.aimlapi.com/v1/decisions`, model
  `typesafe/jev`. ([AI/ML API: Jev](https://docs.aimlapi.com/api-references/decision-models/typesafe/jev))
- LLM Gateway: `/v1/systemone`, which it says will give way to a
  `/v1/decisions` route. ([LLM Gateway: systemone](https://docs.llmgateway.io/v1_systemone))

The route names differ between gateways, and gateways mark the format as
not yet stable. Treat TypeSafe's own docs as the authority once dry calls
it.

### The request

- `model`: the Jev id.
- `state`: what to decide about. Text, a JSON object, or a list of texts.
- `questions`: an object. Each key is yours and comes back in `answers`.
  Each question has a `type`, `instructions`, and for most types
  `criteria`.

The question types:

| type | criteria | answer |
|---|---|---|
| `choice` | required: option key → its description, 1 to 255 options | `choice` (the key), `probabilities` (sum to 1), `confidence` |
| `noul` | optional: what `true` and `false` mean | `noul`: the probability the proposition holds |
| `score` | required: 2 to 10 ordered levels, lowest first | `score` (expected level, may be fractional), `legend`, `probabilities`, `confidence` |
| `box` | none; needs an image (experimental) | edges of a target in the image |

The response also has `model` (the version that answered) and `usage`
(`input_tokens`, `output_tokens`).

### Speed and cost

- AI/ML API says Jev answers "well under a second"; its example call
  took 96 ms. ([AI/ML API: Jev](https://docs.aimlapi.com/api-references/decision-models/typesafe/jev))
- OpenRouter lists input at $0.042 per million tokens and output at $0.
  A three-question call used 447 input tokens, about $0.000019.
- Context: 64k tokens per request (32k for `state` plus one question),
  262k in long-context mode.

### Confidence

TypeSafe says Jev is calibrated in aggregate: a 0.8 is right about 80% of
the time across many answers. A `choice` or `score` confidence says how
concentrated the probabilities are, not whether the answer is right. A
`noul` near 0.5 means unsure. OpenRouter suggests three bands set from your
own labelled data: act, review, escalate.

For tool permission, keep the permission check in code. A Jev verdict
feeds the check; it does not replace it.

## The metadata

A step is jev-possible from metadata only. Nothing is guessed from tool
descriptions or the text contract.

### The response side

A phase that says `response: jev` has a JEV form (`jev::Form`): one
`choice` question.

- `instructions`: the phase's goal.
- `criteria`: one option per call the step offers, its description the
  tool's `when`.
- `state`: what the model would see. Not decided yet (see the open
  questions).

A standard phase has no JEV form. It is still checked, so the line says
what is missing.

### The tool side

A tool is offered to a JEV endpoint only when it carries `ToolJev`:

| field | what |
|---|---|
| `when` | when a call of it is the right answer: its option's criterion |
| `inputs` | each input with its fixed values; a JEV endpoint writes no arguments |

Each combination of input values is one option, up to 255 (Jev's
`choice` limit).

Where the metadata comes from today:

- `goto_phase`: built in. `when` is "this phase is done, or the work needs
  another phase"; `name` takes the other phases.
- An MCP tool: `x-jev` in its input schema, with `when`. Its inputs take
  their values from the schema's `enum`, `const` or `boolean`. An input
  without fixed values is an error that names it.

```json
{
  "properties": { "verdict": { "type": "string", "enum": ["approve", "deny", "escalate"] } },
  "required": ["verdict"],
  "x-jev": { "when": "the pending call is judged" }
}
```

No other tool has metadata yet. `reply`, `run`, `write` and the rest take
free text, so they are never options. Declared and remote capability tools
could get a `jev:` block next (see the open questions).

## The check

`jev::check(engine)` runs inside `prompt::render_with`, after the response
and tools are rendered. The result is `Rendered::jev`:

- `possible`: the phase says `response: jev`, every offered tool carries
  metadata, and there are at most 255 options.
- `form`: the question, when possible.
- `missing`: why not, one entry per missing piece.
- `asked`: the phase says `response: jev`.

Nothing in the turn changes. The standard call still runs, and the
prompt sent is the same.

### The prompt view line

The Prompt section lists it with the notes, in both the html and the gpui
view:

```text
jev: possible (4 choices: goto_phase work, guard.decide approve, guard.decide deny, guard.decide escalate); jev would be used here (calling a JEV endpoint is not built yet: answered the standard way)
jev: not possible — tool `run` has no jev metadata; this phase asks for jev, so it is answered the standard way
jev: not possible — phase `work` is answered standard: no jev form
jev: not possible — no phases: the response has no jev form (a phase with `response: jev` gives it one) (and 2 more)
```

## The endpoint

A model, or its provider, says it serves JEV with `"jev": true` in
`models.json`:

```json
{
  "providers": { "typesafe": { "base_url": "https://api.typesafe.ai/v1", "key_env": "TYPESAFE_API_KEY", "jev": true } },
  "models": [ { "name": "jev", "id": "typesafe/c/jev-latest" } ]
}
```

`Models::jev_endpoint(model)` gives its address (base URL, model id, key
variable). With none marked it says "no JEV endpoint configured". A CLI
model never answers JEV: a CLI only chats.

There is no JEV model on this machine. Nothing calls an endpoint yet, and
nothing fakes one.

### How it would be called

1. The step is jev-possible and a JEV endpoint is configured.
2. Send `state` and the form's `choice` question.
3. Read `choice` and `confidence`.
4. At or above `jev::MIN_CONFIDENCE` (0.8 to start, then set from
   labelled runs), the chosen option's call is the step's `act`.
5. Below it, or on any error, the step runs the standard way.

## Open questions for the owner

- ~~Is TypeSafe's Jev the JEV meant?~~ **Decided (owner, 2026-11-23):
  yes, TypeSafe's Jev.** The design follows its shape.
- What goes into `state`: the whole rendered prompt, or a short record
  (the pending call, the request, the rules)? A short record is cheaper
  and closer to what Jev is for.
- One `choice` per step, or several questions (a `noul` "is this call
  authorised?" plus a `choice` of what next)?
- Should a JEV step be able to end the turn? `reply` takes free text, so
  today a jev phase can only move on or call tools.
- Where should declared and remote capability tools carry their `jev:`
  block: in `capabilities/<id>.md`, or in the MCP `x-jev` key for both?
- Which route to call: TypeSafe's `/v1/systemone`, or a gateway's
  `/v1/decisions`?

## Related, not JEV

Chat models can also be held to a fixed set of answers. That is
constrained decoding or logprob scoring of the same model, not a JEV
endpoint, so it is not what `response: jev` means here. Short notes for
later:

- vLLM: `structured_outputs` with `choice` (the old `guided_choice` was
  removed in v0.12.0), and `logprobs`. ([vLLM: structured outputs](https://docs.vllm.ai/en/latest/features/structured_outputs.html))
- llama.cpp server: a GBNF `grammar` and `n_probs` on `/completion`.
  ([llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md))
- LM Studio and Ollama: a JSON schema in `response_format` or `format`.
  ([LM Studio: structured output](https://lmstudio.ai/docs/developer/openai-compat/structured-output),
  [Ollama: structured outputs](https://docs.ollama.com/capabilities/structured-outputs))
- OpenAI: strict JSON schema with `enum`, plus `logprobs` and
  `logit_bias`. ([OpenAI: structured outputs](https://developers.openai.com/api/docs/guides/structured-outputs))
- Anthropic: `tool_choice` and structured outputs, no logprobs.
  ([Anthropic: structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs))
- The CLIs: `claude -p --json-schema` and `codex exec --output-schema`
  check the shape, at a full session's cost; no logprobs.
  ([Claude Code: headless](https://code.claude.com/docs/en/headless),
  [Codex: non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode))
- Grammar masks cost little per token: XGrammar under 40 µs for JSON
  ([XGrammar paper](https://arxiv.org/html/2411.15100)), llguidance about
  50 µs ([llguidance](https://github.com/guidance-ai/llguidance)). For a
  one-word answer the prompt's prefill is most of the time.
