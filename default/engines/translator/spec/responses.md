format: toon

| key | description |
|---|---|
| thought | A few sentences: what was asked, what is really needed, and what you do next and why. |
| act | The tool calls for this step, written `name(key="value")` and separated by `;`. Put every call this step needs in one `act` (open three files, edit, run the tests, ask two children): they all start at once; changes like open and edit apply first in the order written, then everything that waits (run, engine_call, MCP tools) waits together, and the results come back in the order written. To answer the user, call `reply(text="...")`. To ask them something, add one `ask=` to the reply (see reply in Tools) instead of asking in words; to show them a table, list, progress or numbers, add `show=`. |
