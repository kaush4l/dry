# The companion protocol (`protocol: 1`)

The one wire contract every companion keeps. A companion is a program a
person starts in a terminal on their computer; the dry page (wasm, in any
browser) pairs with it and gets what a browser does not have: one folder's
files, a terminal in it, the computer's models, and its capabilities.

There are three companions, one contract:

| companion | where | state |
|---|---|---|
| Node | `companion/dry-companion.mjs` (Node 20+, one file, no dependencies) | passes `conformance/` |
| Rust | `dry companion` (dry-tools, `crates/tools/src/companion/`; one binary, std only) | passes `conformance/` |
| Python | `companion/dry_companion.py` (Python 3.9+, one file, the standard library only) | passes `conformance/` |

An implementation is thin: the capabilities (`capabilities/*.md`) and the
providers template (`providers.json`) are **data files** every companion
reads the same way, and everything about context (open files, unsaved
edits, terminal sessions, snapshots) is decided by dry's core in the page
(`dry_core::workspace`). A companion only gives primitives.

`conformance/` drives a companion from outside, over HTTP and MCP only,
and is the definition of "keeps the contract": `node conformance/run.mjs`
(§13). What this document says and the suite checks are the same thing; if
they differ, the suite is wrong or this is, and both are fixed together.

Contents: 1 versions · 2 transport and errors · 3 the command line ·
4 origins · 5 pairing · 6 the workspace folder · 7 the workspace's state ·
8 providers · 9 capabilities over MCP · 10 switches and approvals ·
11 stop all · 12 the data files · 13 conformance · 14 what is not in the
contract.

## 1. Versions

- `protocol` is an integer, `1` here, in `GET /v1/health`, `POST /v1/pair`
  and `GET /v1/workspace`. A page refuses a companion whose `protocol` it
  does not speak, saying both numbers.
- Within a protocol version, a companion may **add** fields, endpoints,
  error codes and capabilities; it never removes or changes one. A client
  ignores what it does not know.
- The capabilities handshake carries its own version string,
  `dry-capabilities/1` (§9).

## 2. Transport and errors

- HTTP/1.1 on `127.0.0.1:<port>` only. JSON bodies (`content-type:
  application/json`), UTF-8. A body is at most 8 MB (`413 too_large`).
- Every call but `GET /v1/health`, `POST /v1/pair`, `OPTIONS` and the
  served app needs the session token: `x-dry-token: <token>` or
  `Authorization: Bearer <token>`.
- **An error** is a 4xx/5xx status with

  ```json
  { "error": "a sentence a person can read", "code": "path_outside" }
  ```

  `error` is for people; `code` is for programs and is one of:

  | status | code | when |
  |---|---|---|
  | 400 | `bad_request` | a body or query that does not read; a missing input |
  | 400 | `bad_key` | a terminal key that is not `[A-Za-z0-9._:/-]{1,200}` |
  | 400 | `not_a_folder`, `not_a_file` | a terminal `cwd` that is a file; a read of a folder |
  | 400 | `bad_provider`, `bad_snapshot` | a providers entry or a snapshot that does not check |
  | 401 | `not_paired` | no token, a wrong one, an expired or revoked one |
  | 401 | `wrong_code` | a pairing code that is not the current one |
  | 403 | `host_refused` | the `Host` header is not a loopback name with this port |
  | 403 | `origin_refused` | an `Origin` not on the allowlist (§4) |
  | 403 | `path_outside` | an absolute path, `..` past the folder, a link that leads out |
  | 403 | `secret_path` | a secret-looking name (§6) anywhere in the path |
  | 403 | `dry_state` | `.dry/` through the file primitives; a write into `.git` |
  | 403 | `capability_off` | a capability the person switched off (§10) |
  | 404 | `not_found` | no such file, session, pairing or endpoint |
  | 404 | `no_provider` | a model whose provider is not declared |
  | 409 | `exists` | a provider name that is taken |
  | 428 | `needs_confirm` | a publishing command (`git push`, …) outside the git endpoints |
  | 502 | `upstream` | a provider that did not answer well |
  | 503 | `unavailable`, `provider_down` | something this machine lacks; a provider not ok |
  | 504 | `timeout` | |
  | 500 | `internal` | |

  A client acts on `code` and shows `error`. A new code may be added in
  `protocol: 1`; a client treats an unknown code by its status.

## 3. The command line

Every companion takes these options, so a person (and the conformance
suite) starts any of them the same way:

| option | means |
|---|---|
| `--folder DIR` | the workspace folder (default: the current folder). The whole disk (`/`) is refused |
| `--port N` | listen on `127.0.0.1:N` (default 7777; `0`: any free port) |
| `--yes` | take the saved switches and the folder without asking (no prompt reads stdin) |
| `--on ID[,ID]`, `--off ID[,ID]` | switch capabilities on or off, and save that (§10) |
| `--providers FILE` | the providers file (default `DIR/.dry/providers.json`) |
| `--config FILE` | where the switches are saved (default `DIR/.dry/companion.json`) |
| `--capabilities DIR`, `--no-capabilities` | the declared capability files (default: the `capabilities/` that ships beside it) |
| `--pair-code-file FILE` | write every new pairing code to FILE (one line), replacing it |
| `--heartbeat S` | the streams' ping interval in seconds (default 15) |
| `--no-app` | serve no app |

What it prints on stdout, in this order, is part of the contract:

1. the checks (§10), one line each: `ok`, `missing`, `absent` or `failed`,
   what, and on the next line `→ <how to fix it>` when it is not ok;
2. the switches, one line each;
3. `listening http://127.0.0.1:<port>` (exactly this, on its own line);
4. `pairing code: <CODE>` (and again whenever the code changes);
5. live lines: who connected (`connected: p1 from <origin>`), who a peer
   is (`p1 is dry-web (…)`), each command and call as it runs and ends.

It stops on SIGINT / SIGTERM (and when its terminal closes), stopping
every command it started. While it runs, **one key stops everything it
runs: Esc** (the same as typing `stop-all`). It works the same in the
terminals of macOS, Linux and Windows: plain stdout and stdin, no screen
drawing.

## 4. Origins, Host, preflight

- The `Host` header must be `127.0.0.1:<port>`, `localhost:<port>` or
  `[::1]:<port>`, else `403 host_refused` (DNS rebinding).
- A request with an `Origin` is answered only for `https://kaush4l.github.io`,
  `http://localhost[:port]`, `http://127.0.0.1[:port]`, `http://[::1][:port]`
  and each `--origin`, else `403 origin_refused`. `Origin: null` is refused.
- An allowed origin gets `access-control-allow-origin: <that origin>`
  (never `*`) and `vary: Origin` on every answer.
- `OPTIONS` (a preflight) from an allowed origin: `204` with
  `access-control-allow-methods: GET, POST, OPTIONS`,
  `access-control-allow-headers` including `content-type, authorization,
  x-dry-token, last-event-id, mcp-protocol-version, mcp-method, mcp-name,
  accept`, and `access-control-allow-private-network: true` (Chrome's
  Private Network Access: an https page reaching a loopback server).
- A page the companion serves itself is same origin.

## 5. Pairing

```text
GET  /v1/health   → {ok: true, name, version, protocol: 1, paired: bool, app: bool}
POST /v1/pair     {code} → {token, id, expires, ttl, name, version, protocol}
POST /v1/unpair   → {ok: true}                      this token ends; its streams close
GET  /v1/pairings → {pairings: [{id, origin, created, expires, seen, current}]}
POST /v1/pairings/revoke {id} → {ok: true} | 404     that pairing ends; its streams close
```

- The **code** is 6 characters from `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`,
  printed at start; it is matched without regard to case and surrounding
  spaces. It works **once**: a pairing makes a new one (printed, and
  written to `--pair-code-file`). **Five wrong codes** in a row make a new
  one too.
- The **token** is 64 hex characters (256 random bits), kept in the
  companion's memory only: a restarted companion knows no tokens. `id` is
  `p<n>`. `expires` is unix seconds; `ttl` the pairing's length in seconds
  (default 12 h).
- A token that expired, was revoked or unpaired gets `401 not_paired`
  everywhere; `GET /v1/health` then says `paired: false`.
- The page keeps its token in the tab's sessionStorage (a reload keeps the
  pairing; closing the tab ends it), never in IndexedDB, localStorage or a
  URL.

## 6. The workspace folder

The person picks **one folder** when they start the companion
(`--folder`, confirmed in its terminal unless `--yes`). The page has no
files and no terminal of its own; these primitives are all it gets, and
dry's core builds everything else on them (`dry_core::workspace::Folder`
is the `live::Host` over these calls, the same `file` and `terminal`
artifact code the desktop runs).

**Paths** are relative to the folder, `/`-separated; `""` and `.` are the
folder. Refused, before anything is read: an absolute path (`/x`, `C:\x`)
and `..` that leaves the folder (`403 path_outside`); a path whose real
place (after links) is outside the folder (`403 path_outside`); any part
that looks secret (`403 secret_path`); anything under `.dry/` (`403
dry_state`). Writes into `.git` are refused too (`403 dry_state`).

Secret-looking names (dry's `file::secret_file`, which the live view uses,
plus `.git-credentials` and the key folders below, which `secret_file`
does not name: a companion checks every part of a path against all of
them): `.env`, `.env.*`, `.netrc`, `.npmrc`, `.pypirc`, `.pgpass`,
`.git-credentials`, `id_rsa*`, `id_ed25519*`, `id_ecdsa*`, `credentials*`,
`secrets.*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.keystore`, and the
folders `.ssh`, `.gnupg`, `.aws`, `.kube`, `.docker`. Never listed, read or
written.

### Files (`file` capability)

```text
GET  /v1/fs/list?path=DIR&max=N  → {path, files: ["a.txt", "src/main.rs", …], truncated}
GET  /v1/fs/read?path=FILE       → {path, kind: "text", text, bytes}
                                 | {path, kind: "binary", bytes, image}
                                 | {path, kind: "too_large", bytes}        (over 1 000 000 bytes)
GET  /v1/fs/stat?path=P          → {path, kind: "file" | "dir", size, modified} | {path, kind: "missing"}
POST /v1/fs/write {path, text}   → {ok: true, path, bytes}                  (folders made; atomic)
```

- `list`: every file under `DIR` (default the folder), paths relative to
  `DIR`; a folder's own files first, by name (byte order), then its
  folders' (breadth first); at most `max` (default 1000, at most 10 000),
  `truncated` says it stopped. Never listed: `.git`, `.dry`, `target`,
  `node_modules`, `.DS_Store`, secret-looking names, links that lead out.
- `read`: text is UTF-8 with no zero byte; anything else is `binary`. A
  missing file is `404 not_found`; a folder `400 not_a_file`.
- `path` in an answer is the real path relative to the folder.

### Terminal (`terminal` capability; an actuator, off until switched on)

```text
POST /v1/term/run   {key, command, cwd?, timeout_ms?}  waits → {key, id, mode: "run", status: "exited", exit, signal, timed_out, output, ms}
POST /v1/term/start {key, command, cwd?, callback?}    at once → {key, id, mode: "start" | "background", status: "running", started}
GET  /v1/term/tail?key=K&lines=N                       → {key, status: "running" | "exited", exit, signal, tail} | {key, status: "unknown", exit: null, tail: ""}
POST /v1/term/stop  {key}                              → the session (SIGTERM to its process group, SIGKILL 2 s later) | 404
GET  /v1/events?since=ID                               server-sent events: `exit` when a session ends
```

- `command` is one shell command line (`/bin/sh -c` on macOS and Linux,
  `cmd.exe /d /s /c` on Windows), run in `cwd` (a folder in the
  workspace, default the folder), with no input and without the
  environment variables whose names say they hold secrets (`KEY`, `TOKEN`,
  `SECRET`, `PASSWORD`, `PASSWD`, `CREDENTIAL`, `AUTH`, `COOKIE`,
  `SESSION`). Publishing commands (`git push`, `git clone`, `gh pr
  create`, …) are `428 needs_confirm`.
- `key` names the session the way the page does
  (`<engine>/<artifact>/<n>`); a new session with a key replaces a
  finished one with the same key. `tail` of a key the companion does not
  know (it restarted) is `status: "unknown"`: the page reads it as stopped.
- `run` waits for the end (`timeout_ms`, default 30 min, kills it after):
  `output` is its last 64 KB. `start` returns at once; with `callback`
  (the default) its end goes to every event stream as `exit` (a callback),
  without it (`"callback": false`, a background session) only to a stream
  for it.
- `GET /v1/events`: `event: hello` first (`{sessions, last}`), then
  `id: <n>` / `event: exit` / `data: <the session>`; `?since=<id>` (or
  `Last-Event-ID`) replays what came after that id (the last 1000 are
  kept). A `: ping` comment every heartbeat.

### The workspace itself

```text
GET  /v1/workspace           → {protocol: 1, folder, name, state: ".dry/state", snapshot: {…} | null, switches: {file, terminal}}
POST /v1/workspace/snapshot {snapshot} → {ok: true, path: ".dry/snapshot.json"}
GET  /v1/store               → {texts: {path: text}}        the page's state, `.dry/state/` (§7)
POST /v1/store {put: {path: text}, remove: [path]} → {ok: true}
```

## 7. The workspace's state

In the folder, written by the companion only when the page asks:

```text
<folder>/.dry/
  state/              the page's state, the same layout as dry's state/ (engines/<id>/…,
                      artifacts, history, spans): one text per path, through /v1/store
  snapshot.json       what the workspace looked like at the last save (below)
  providers.json      the declared providers (§8)
  companion.json      the switches (§10)
```

- A store path is relative, `/`-separated, with no `.`, `..` or empty
  part (`400` otherwise). Writes are atomic per file.
- **One writer.** While a page is paired, it writes through the companion;
  its IndexedDB is the offline cache. Offline (the companion stopped, the
  pairing ended), the page keeps working from the cache and marks it
  pending; when it pairs again, the pending texts go to the companion first,
  then it loads from it. So reopening the page and pairing again brings the
  engines, open files and unsaved edits back, also after the companion
  restarted.
- **The snapshot** is written by the page on every save. It is the
  workspace's context, readable without dry: the open files with their
  focus lines, the unsaved edits, the terminal sessions, and the tab the
  person had in front. `dry_core::workspace::Snapshot` makes it and
  restores from it (an artifact that lost its state gets it back; one that
  has its own is kept):

  ```json
  {
    "version": 1,
    "saved": 1795400000,
    "focus": "e1/files:README.md",
    "engines": [
      { "engine": "e1", "name": "dev",
        "artifacts": [
          { "name": "files", "kind": "file", "root": ".",
            "open": [{ "path": "README.md", "focus": [1, 40] }],
            "edits": [{ "path": "README.md", "old": "# x", "new": "# y" }],
            "text": "kind: file\nroot: .\nopen: README.md 1-40\n…" },
          { "name": "terminal", "kind": "terminal", "root": ".",
            "sessions": [{ "id": 1, "mode": "run", "status": "exit:0", "started": 1795399990, "command": "ls" }],
            "text": "kind: terminal\nroot: .\nsession: 1 run exit:0 1795399990 ls\n" } ] } ]
  }
  ```

  `text` is the artifact as dry keeps it (what restore uses); the other
  fields say the same for any reader. A companion stores the snapshot as
  given and checks only that it is an object with `"version": 1`.

## 8. Providers: `providers.json`, declared, not scanned

The companion probes **only what is declared**, so a first run is instant
and nothing on the machine is guessed at. The template is
`providers.json` beside `models.json` (the examples); the companion makes
`.dry/providers.json` from it on its first run, and the person pastes
entries into `"providers"` (or adds one from the page, below).

```json
{ "version": 1, "providers": [
  { "name": "claude",   "kind": "cli",    "cli": "claude", "model": "sonnet" },
  { "name": "copilot",  "kind": "cli",    "cli": "copilot", "path": "/opt/tools/copilot" },
  { "name": "lmstudio", "kind": "port",   "url": "http://127.0.0.1:1234" },
  { "name": "local",    "kind": "native", "runner": "auto", "model": "mlx-community/Qwen3-4B-4bit" }
] }
```

| field | |
|---|---|
| `name` | `[a-z0-9][a-z0-9_-]{0,39}`, unique: the models are `<name>/<model>` |
| `kind` | `cli`, `port` or `native` |
| `cli` | for `cli`: `claude`, `codex` or `copilot` (default: the name) |
| `path` | for `cli` and `native`: the program's absolute path, when it is not on `PATH` |
| `model` | one model (`cli`: else its usual ones; `native`: required: a local file or folder, or a model id already downloaded) |
| `url` | for `port`: an OpenAI-compatible server (LM Studio, Ollama, vLLM, a forwarded port); a trailing `/v1` is dropped |
| `runner` | for `native`: `auto` (the default), `mlx` or `llama.cpp` |
| `port` | for `native`: the local port to start it on (default: a free one) |

**Probing** (at start, on `GET /v1/providers?refresh=1`, after an add):

| kind | ok when | else |
|---|---|---|
| `cli` | the program (`path`, else `PATH`) answers `--version` | `absent` (not there: the install hint) or `failed` |
| `port` | `GET <url>/v1/models` answers within 3 s: its `data[].id` are the models | `failed`, with what the request said |
| `native` | the runner and the model are on this machine; the companion starts the runner on a local port, waits for its `/v1/models`, and from then it is a port | `absent` (no runner: the install hint) or `failed` (no model: the companion never downloads) |

- `runner: auto` chooses by hardware: Apple silicon → `mlx`
  (`mlx_lm.server --model M --host 127.0.0.1 --port P`), NVIDIA (an
  `nvidia-smi`) → `llama.cpp` with CUDA, anything else → `llama.cpp` on the
  CPU or Vulkan (`llama-server -m M --host 127.0.0.1 --port P`). Never a
  download flag; a model not on disk is reported, with the command the
  person may run themselves.
- **A companion never installs or downloads anything.** A missing program
  is `absent` with its install hint; a missing model is `failed` with
  where to get it.
- The runner a companion started is stopped when the companion stops.

```text
GET  /v1/providers?refresh=1 → {file, exists, template, errors: [{entry, error}],
                                providers: [{name, kind, entry, state: "ok" | "failed" | "absent" | "starting", why, hint, models: ["<name>/<model>"], url?, runner?, accel?, version?}]}
POST /v1/providers {provider: {…an entry…}} → {provider: {…as above, probed}} | 400 bad_provider | 409 exists
GET  /v1/models              → {object: "list", data: [{id: "<name>/<model>", object: "model", owned_by, provider, kind}]}   ok providers only
POST /v1/chat/completions    OpenAI's body; `model` is "<name>/<model>" → OpenAI's answer
```

- A `port` or `native` provider's chat is passed through to its server
  (streamed as it streams). A `cli` provider runs its CLI with argument
  arrays, in a temporary folder, with no tools (`claude -p --output-format
  json --tools "" …`, `codex exec --sandbox read-only --ephemeral -o FILE -`,
  `copilot -p … --available-tools ""`); `stream: true` gets the whole
  answer as one chunk.
- `404 no_provider` for an undeclared name, `503 provider_down` for one not ok.
- Each provider is also a capability, `model.<name>` (§9): present when
  ok, absent with why (and the hint) otherwise; its snapshot is its models.
  So its models come through the handshake, the page's model picker lists
  them, and an engine sees no difference between the kinds.

## 9. Capabilities over MCP

`/mcp` is an MCP server (streamable HTTP; the token is the bearer). This is
docs/capabilities.md §4's handshake.

```text
POST /mcp initialize   params._meta.hello = {runtime, platform, version, protocol: "dry-capabilities/1"}
                       → result._meta.hello (the companion's), capabilities.tools.listChanged = true
POST /mcp tools/list   → tools; a capability's tool has _meta = {capability, doc, state, why?, approval}
POST /mcp resources/list → one resource per capability: uri dry://capability/<id>,
                       _meta = {capability, doc, state, why?, snapshot, interval, events: [{name, when, text}]}
POST /mcp resources/read {uri} → contents[0].text: its snapshot now ("(absent: why)" when absent)
POST /mcp tools/call {name, arguments} → {content: [{type: "text", text}], isError, _meta?: {code}}
POST /mcp ping         → {}
GET  /mcp  (Accept: text/event-stream) → the peer's stream:
                       data: {"jsonrpc":"2.0","method":"notifications/tools/list_changed","params":{}}
                       data: {"jsonrpc":"2.0","method":"notifications/resources/updated","params":{"uri":…,"_meta":{"event":{name,text}}?}}
                       : ping            every heartbeat
```

- **Register.** `initialize` keeps the peer's hello on its pairing (the
  terminal's `pairings` shows it); a second `initialize` replaces it
  (**re-register**). Then `tools/list` and `resources/list` are the
  manifest.
- **Re-register on change.** When the capabilities change (a file added
  to or changed in the capabilities folder, a probe that changed, a switch
  flipped, a provider added), every stream gets
  `notifications/tools/list_changed`; the peer lists again.
- **Events** are the provider's: while a stream listens, the declared
  snapshots are read at their intervals; an event whose condition became
  true rides on `notifications/resources/updated` (`_meta.event`), once
  per edge (docs/capabilities.md §3).
- **Heartbeat and leaving.** The stream carries `: ping` at least every
  heartbeat (15 s; `--heartbeat`); a peer `ping`s too. A peer has left
  when its stream closes. Unpairing or revoking a pairing **closes its
  streams** (`/mcp` and `/v1/events`), so the page sees it leave at once.
- A tool without `_meta.capability` is an ordinary MCP tool.
- A `tools/call` that fails is a result with `isError: true` (and
  `_meta.code`, §2, when it has one: `capability_off`, `path_outside`…);
  an unknown tool is JSON-RPC error `-32602`. Malformed JSON-RPC is `-32700`
  / `-32600`; an unknown method `-32601`.
- MCP's own versions (legacy `initialize`, the stateless 2026-07-28 era
  with its header checks) are as docs/companion.md "MCP" says.

The companion's own capabilities:

| id | tools | actuator |
|---|---|---|
| `file` | `file.list(path?)`, `file.read(path)` (and the `/v1/fs` primitives) | no |
| `terminal` | `terminal.run(command, cwd?)` (and `/v1/term`) | **yes** |
| `browser` | `browser_open`, `browser_look`, `browser_click`, `browser_type`, `browser_screenshot` | no |
| `evidence`, `git`, `container` | as docs/companion.md | no |
| `model.<name>` | none: the models, through the handshake (§8) | no |
| `<id>` | each `capabilities/<id>.md`: its tools `<id>.<tool>` | when a tool says `approval: yes` |

**Inputs never go through a shell.** A declared tool's command is an
argument list; an input is one whole argument (`{name}`) and the
environment variable `DRY_INPUT_<NAME>`, never spliced into a string. A
value like `x; touch /tmp/p $(id)` arrives as one argument, as typed. The
parser refuses a file that would do otherwise (docs/capabilities.md §3).

## 10. Switches and approvals

- **Switches.** At start the companion prints the checks (the declared
  providers, the declared capabilities, git, Chrome, a container runtime:
  each found or missing, with the fix) and then every capability with its
  switch. The person flips them there (their numbers), or later by typing
  `on <id>` / `off <id>`, or starts it with `--on` / `--off`. The choice is
  saved (`.dry/companion.json`: `{"version": 1, "switches": {"terminal":
  true}}`); `--yes` takes the saved choices. **Actuators start off**: the
  terminal and every capability with an `approval: yes` tool. A switched-off
  capability is `absent` with why (`switched off in the companion's
  terminal (type: on <id>)`) in the manifest, its calls are `403
  capability_off` (HTTP) or `isError` with `_meta.code: "capability_off"`
  (MCP), and a flip sends `notifications/tools/list_changed`.
- **Approvals.** An actuator's tools say `_meta.approval: true`. The
  client decides before calling: dry's core refuses the call unless the
  engine's spec has an `allow:` rule for it (`allow: gpio.set(*"pin":"17"*)`),
  and says how to allow it (docs/capabilities.md §2). The companion's
  switch is the person's approval on the machine's side; both must say yes.
  A page never flips a switch: only the person at the companion's terminal.

## 11. Stop all

```text
POST /v1/stop-all → {stopped: <sessions stopped>, browser: <was it open>}
```

Every running command (SIGTERM to its process group, SIGKILL 2 s later),
every evidence run, and the browser. The same as `stop-all` or **Esc** in
the companion's terminal, and the page's **Stop all**. Each stopped
session ends with `status: "exited"` and no exit code (`exit: null`,
`signal` set); a `start`'s end is still an `exit` event.

## 12. The data files

What every companion reads the same way, so implementations stay thin:

| file | what | read by |
|---|---|---|
| `capabilities/<id>.md` | a declared capability (docs/capabilities.md §3) | the parser (`dry_core::capability::declared`, and each companion's) |
| `providers.json` | the providers template (§8) | each companion, on its first run |
| `<folder>/.dry/providers.json` | this folder's providers | the companion |
| `<folder>/.dry/companion.json` | the switches | the companion |

## 13. Conformance

```sh
node --test 'conformance/*.test.mjs'                          # the Node companion (the default command)
DRY_COMPANION_CMD="dry companion" node --test 'conformance/*.test.mjs'   # any other: the command that starts it
node conformance/run.mjs --cmd "python3 $PWD/companion/dry_companion.py"   # the same, the command as an option
```

The suite needs only Node 22 (for the test runner, `fetch` and its fake
servers). It starts the command from its scratch folder, so a path in the
command is absolute (`$PWD/...`), or the program is on `PATH`. For each
test file it makes a scratch folder, a scratch capabilities folder and a
scratch providers file, starts the companion as

```text
<cmd> --folder <tmp>/ws --port <free> --yes --pair-code-file <tmp>/code --config <tmp>/companion.json
      --capabilities <tmp>/caps --providers <tmp>/providers.json --heartbeat 1 --no-app [--on terminal,…]
```

waits for the `listening` line, reads the code from the file, and drives it
over HTTP and MCP only. It stops it after (SIGTERM). What it covers:

| file | covers |
|---|---|
| `pairing.test.mjs` | health; a wrong code (401 `wrong_code`), the right one once; the code file changes; five wrong codes change it; no token / a bad token (401 `not_paired`); unpair; revoke; expiry is in the answer |
| `origins.test.mjs` | a foreign Origin (403 `origin_refused`), `Origin: null`, a foreign Host (403 `host_refused`), the preflight with `Access-Control-Allow-Private-Network`, the allowed origin echoed, never `*` |
| `workspace.test.mjs` | list (order, hidden folders, secrets never listed, `.dry` never listed), read (text, binary, too large, missing), write (folders made), stat; `..`, absolute paths and a link out refused (403 `path_outside`); secrets refused (403 `secret_path`); `.dry` refused (`dry_state`); the snapshot round trip; the store |
| `terminal.test.mjs` | switched off: 403 `capability_off`; on: run (exit code, output, cwd), start then the `exit` event, tail, stop, an unknown key is `unknown`, a bad key, a publishing command (428), secrets kept out of the environment, stop-all |
| `mcp.test.mjs` | initialize with hello both ways, re-register, tools/list `_meta.capability` and `approval`, resources, a declared tool with an injection attempt (one argument), a switched-off capability, `ping`, the stream: list_changed on a new capability file, `: ping` heartbeats, revoke closes it |
| `providers.test.mjs` | a declared port (a fake server: ok, its models in `/v1/models` and as `model.<name>`, a chat passed through), a dead port (failed, why), a cli not there (absent, the hint), a cli by `path`, a native runner (a fake runner by `path`: started on a port, its models, stopped with the companion), a native runner absent (the hint; nothing downloaded), bad entries, add through `POST /v1/providers` (409 for a taken name), an undeclared model (404 `no_provider`) |

A new companion is done when `node conformance/run.mjs --cmd "<its command>"` passes with its
command. A change to this document comes with the change to the suite.

## 14. What is not in the contract

The Node companion also has, outside this contract (docs/companion.md):
projects mode (`--root`, `/v1/projects`, `/v1/files/*`, `/v1/exec/*`), git
(`/v1/git/*`), the browser over HTTP (`/v1/browser/*`), evidence runs,
custom tools (`tools/*.md`, `/v1/tools`), and serving the built app. A
later companion may add them the same way; a page checks for them by
their capability, never by the companion's name.
