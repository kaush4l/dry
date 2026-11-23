# Responses

The contract every new model engine starts with: what each answer must
contain, one row per field (`key`, `description`). An engine keeps its own
copy, so it can be changed while it runs; `format:` is how answers are
written (`json`, `toon`, or `toon` followed by its own separator, like `toon=`).

There is no "finished" field: answering the user is a call (`reply`), so
every step of the loop has the same shape.

format: toon

| key | description |
|---|---|
| thought | One or two sentences: what you understood and what you will do next. |
| act | The tool calls for this step, written `name(key="value")` and separated by `;`. Put every call this step needs in one `act` (open three files, edit, run the tests, ask two children): they all start at once; changes like open and edit apply first in the order written, then everything that waits (run, engine_call, MCP tools) waits together, and the results come back in the order written. To answer the user, call `reply(text="...")`. To ask them something, add one `ask=` to the reply (see reply in Tools) instead of asking in words; to show them a table, list, progress or numbers, add `show=`. |
