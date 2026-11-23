# Capabilities

The owner's standard: **everything is a capability**. Nothing in the turn
loop names a runtime. The loop (prompt → answer → calls → results) is the
same in the desktop window, on the host, and in the browser. Only the way a
runtime provides capability X differs: this build compiles it in, this
machine declares it in a file, or another runtime that joined provides it.

Built: `dry_core::capability` (the model, the registry, `uses:`, the
declared parser, the handshake's messages), `dry_tools::capability`
(declared commands, probes, peers over MCP, `dry discover`),
`dry-server` `capabilities.rs` (the host's registry, peers, routing),
`dry-web` `caps.rs` (the browser's own capabilities, the companion joining),
and the companion (`companion/dry-companion.mjs`, its capabilities over MCP).

## 1. The model

A capability documents itself and is modelled as tools:

| part | what |
|---|---|
| `id` | its name: `file`, `terminal`, `battery`, `gpio`, `model.local` |
| `doc` | what it is and how to use it. It goes into the prompt only while the capability is present |
| `tools` | typed inputs (`string`, `integer`, `number`, `boolean`, required or not); an actuator says `approval: yes` |
| `snapshot` | a state block, read again every `interval` |
| `events` | `when:` triggers. One that fires posts a message to every engine using the capability, and those engines run a turn |
| `state` | `present`, `needs-grant` (a person's gesture is needed first: a browser permission), `absent` (with why) |
| `provider` | `compiled` (this build), `declared` (`capabilities/<id>.md`), `remote:<peer>` |

**Self-injection.** An absent capability is not rendered, is not in the
prompt, and brings no tools. It behaves like an empty phase block. An
artifact whose capabilities are all absent does not appear in the artifacts
slot, and its tools are not in the tools slot. The engine page and Settings
→ Capabilities say why.

**The registry** (`Registry`) holds every capability from every provider.
For an id, the one an engine uses (`effective`) is the first present one in
this order: compiled, declared, remote. Otherwise it is the first one in any
state, so the page can say why. A shell holds one registry and gives it to
every engine (`State::equip`; `Engine::machine`, which is never saved or
compared).

## 2. Artifacts are groups of capabilities

An artifact's first line lists the capabilities it uses:

```text
uses: [file, clock]        a file artifact that also shows the time
uses: [battery]            this machine's battery (declared, or a peer's)
uses: [learning, analytics]
kind: file                 the old spelling: an alias, unchanged
```

- `kind: x` is an alias, so every existing spec and state loads unchanged.
  `kind: learning` stands for `[learning, analytics]`, because analytics is
  now its own capability and the learning kind used to bring its tool.
  `kind: thread` (and the old `kind: dashboard`) stands for `[shared]`. An unknown `kind:` is still a
  plain text artifact.
- An artifact holds the state of **at most one** capability that keeps
  state (file, terminal, engines, learning, browser, evidence).
  `capability::problem` says which ones clash. The others (clock, scripts,
  alerts, shared, analytics, container, any declared or remote one) add
  their tools, docs and snapshots beside it.
- A `uses:` header survives its kind's writes (`keep_header`). The state
  functions still write `kind: x`, and the header is put back.
- The compiled capabilities and the kinds that run them in core:

  | id | kind | tools |
  |---|---|---|
  | clock | Clock | |
  | scripts | Scripts | script |
  | alerts | Alerts | |
  | file | File | open, focus, edit, save |
  | terminal | Terminal | run, start, background |
  | engines | Engines | engine_call, engine_start |
  | shared (thread) | Shared | post, edit_post |
  | learning | Learning | collect, learn |
  | analytics | Learning | analytics |
  | browser | Browser | browse, click, type |
  | container | none (a presence) | none: commands go through the terminal |
  | evidence | Evidence | evidence |

- **Read mode: `uses: [file:read]`.** A `:read` after a compiled id is
  the same capability (present or absent as it is), held so that it
  changes nothing outside the artifact. It is data on the artifact, read
  by `capability::read_only`; the turn loop has no branch for it.
  - `file:read` brings `open` and `focus` only. `edit` and `save`
    (`Builtin::writes`, `file::WRITES`) are not in the toolbox, the
    tools slot, the artifact's section or the engine page. The section's
    heading says `(read-only: file:read …)`. A hand-made `edit` or
    `save` is refused with why (`capability::refusal`, checked in
    `live::call` and in `tools::run`'s "no tool" error), and nothing is
    held or written.
  - `terminal:read` keeps `run`, `start` and `background`, and refuses a
    command known to write (`terminal::writes`), before a session is
    recorded or anything runs. It refuses an unquoted `>`/`>>` to a file
    (not `2>&1` or `/dev/null`); `rm`, `mv`, `cp`, `mkdir`, `touch`,
    `chmod`, `ln`, `tee`, `dd`, `patch`, `rsync`, `eval` and the like;
    `sed -i`, `perl -i`; `find -delete`/`-exec`; git commands that change
    the tree, the index, the refs or the config (`add`, `commit`,
    `checkout`, `restore`, `reset`, `stash` but not `stash list`,
    `branch <name>`, `config` without `--get`, …); installs (`bun add`,
    `npm install`, `cargo add`, `pip install`, …); and formatters or
    fixers that rewrite (`cargo fmt` without `--check`, `prettier
    --write`, `eslint --fix`, `bun test -u`). It reads inside `sh -c
    "…"` and through `env`, `xargs`, `npx`, `bunx` and `bun x`.
  - **What the terminal guard does not stop.** It reads words, so it is
    not a sandbox. Any program can write by itself: a script
    (`bun run x.ts`), a test with side effects, `node -e`/`python -c`
    code, a package.json script (`bun run format`), a build's own output
    (`target/`, `dist/`), backticks, or a variable that expands to a
    write (`$(…)` is read as its own command). A reviewer or researcher still ends with
    `git status --short`, and the translator's judging skill checks that
    it shows only the author's change.
  - The `kind:` spelling takes it too (`kind: file:read`), and the mode
    survives the kind's own writes (`keep_header`).
- **Renderers draw by capability.** The artifacts slot, the engine page's
  rows and tool groups (`Source::Capability`), and Settings → Capabilities
  all come from `capability::equipped`. There is no per-kind branch for a
  declared or remote capability: its section is its doc and its snapshot
  (`capability::section`).

### Calls

- A **compiled** capability's tools keep their bare names (`open`, `run`)
  and run in core (`dry_core::live`) through the shell's `Host`. The shell
  binds the Host to whoever provides the capability: this machine's disk,
  or, in the browser, the paired companion's (`companion::Shell`). A peer's
  manifest for a compiled id (the companion's `terminal.run`) only says the
  capability is present there. The engine still gets the compiled tools.
- A **declared or remote** capability's tools are called `<id>.<tool>`
  (`battery.status`, `model.local.ask`). Core checks the call before it
  waits (`capability::check`): no unknown input, required inputs present,
  types right, no zero byte, at most 64 KB. An actuator (`approval: yes`)
  also needs an `allow:` rule in the engine's spec (`allow:
  gpio.set(*"pin":"17"*)`), or it is refused with how to allow it. The
  call then waits like an MCP call, and the shell's registry routes it
  (`Registry::route`) to its provider: a declared command, a peer's
  `tools/call`, or the browser's own API. The engine never knows which
  machine runs it.

## 3. Declared capabilities: `capabilities/<id>.md`

What a machine says it provides. The host and the window read the repo's
`capabilities/` (`DRY_CAPABILITIES=<dir>`, or `none`). The companion reads
`--capabilities DIR` (default `../capabilities` next to it). Each file is
probed when it is loaded.

```markdown
# battery
This Mac's battery: how charged it is, and whether it runs on power.

probe: pmset -g batt
snapshot: pmset -g batt
interval: 60s

## tool: status
The battery now, as pmset reports it.
command: pmset -g batt
timeout: 10s

## event: unplugged
when: contains discharging
The Mac went on battery power (unplugged).
```

| line | means |
|---|---|
| `# <id>` | first; lower case letters, digits, `-`, `_`; the file is `<id>.md` |
| text under it | the doc (required) |
| `probe:` | a command: exit 0 is present, anything else (or a missing program) is absent with why. No probe means present |
| `snapshot:`, `interval:` | the state block's command, and how often (default 60 s) |
| `## tool: <name>` | its doc; `command:` (an argument list: split on spaces, or a JSON array); `inputs:` then `- name (type, required): doc`; `timeout:` (default 30 s); `approval: yes` for an actuator |
| `## event: <name>` | `when:` (`contains <text>`, `missing <text>`, `<key> <op> <value>` where op is `< <= > >= = !=` and the key is a `key: value` or `key=value` in the snapshot), then what the engines are told |

- **Errors carry line numbers**, as the template and evidence parsers' do:
  `gpio.md:12: an input cannot follow -c: it would run as code`.
- **Inputs never go through a shell.** An argument that is exactly
  `{name}` becomes that input's value as one whole argument. It is left
  out when an optional input is not given. Every input is also in the
  environment as `DRY_INPUT_<NAME>`. These are refused when the file is
  read: a placeholder inside an argument (`pin={pin}`), the program as an
  input, a placeholder right after a code flag (`sh -c {x}`, `python -c`,
  `node -e`, …), and a probe or snapshot that takes inputs. Commands run
  with no input, in the repo root (the companion runs them in its
  capabilities folder), with the secret-looking environment variables
  removed, and they are killed at the timeout.
- **Events are edge-triggered.** The first reading never fires. A condition
  that stays true fires once, and fires again only after it was false.
- The examples: `capabilities/battery.md` works on this Mac (pmset).
  `capabilities/gpio.md` (Raspberry Pi pins via `pinctrl`; `set` needs
  approval) and `capabilities/sensors.md` (`vcgencmd`) probe as absent
  here, with why.

### `dry discover`, `dry capabilities`

`dry capabilities` lists every capability on this machine with its
provider, state, why, and tools (an actuator is marked). `dry discover`
looks at the machine (pmset battery and thermal state, df, docker or
podman, `/dev/gpiochip0`, vcgencmd) and prints a draft file for each thing
it finds. `dry discover --write <dir>` writes the drafts to a folder you
review. It never writes into `capabilities/` unless you name that folder,
it keeps files that exist, and every tool in a draft that acts says
`approval: yes`. Every draft is checked by the parser before it is
proposed.

## 4. The handshake: how another runtime joins

A companion, a host, or another device joins over **MCP**. The companion
already serves `/mcp`.

```text
pair            the companion's one-time code → a token (POST /v1/pair)
hello           initialize: params._meta.hello = {runtime, platform, version, protocol}
                            result._meta.hello = the provider's hello
                            (protocol "dry-capabilities/1")
manifest        tools/list: each tool's _meta = {capability, doc, state, why?, approval}
                resources/list: one resource per capability, dry://capability/<id>,
                  _meta = {capability, doc, state, why, snapshot, interval, events:[{name, when, text}]}
registered      merged with the compiled ones, tagged remote:<peer>
live            GET /mcp (Accept: text/event-stream): the provider's stream
                  notifications/resources/updated {uri, _meta.event?}
                    → resources/read: the snapshot; an event that rides along fires
                  notifications/tools/list_changed → list again: re-register (Changes say what)
                ping every 5 s: the heartbeat (presence)
                tools/call: a routed call
leave           the stream closes, or a ping fails, or POST …/leave:
                  its capabilities become absent and leave every prompt and page
```

- A tool without `_meta.capability` is an ordinary MCP tool (the
  companion's custom tools), not part of a capability.
- A peer's capability is used only where no compiled or declared one with
  that id is present. On the host, the companion's `terminal` shows as
  present but not used. In the browser, the page's own terminal is absent,
  so the companion's terminal is the one used.
- The **provider decides its own events**: it reads its snapshots at their
  intervals while a peer listens, and an event rides on its update. The
  registry does not evaluate a remote capability's events again
  (`Registry::snapshot`), so an event never fires twice.
- Pure parts: `capability::mcp` (`initialize`, `hello_of`, `manifest`,
  `describe`, `call`, `call_result`, `read_result`, `note`).

### Who implements it

| side | where | what |
|---|---|---|
| the provider | the companion | its declared files, plus what it already did, exposed as capabilities: `terminal` (`terminal.run`, approval), `file` (`file.list`, `file.read`), `browser` (the `browser_*` tools), `evidence` (`evidence_run`, `evidence_read`), `git` (`git.status`), `container` (docker or podman; snapshot `ps`), and `model.<name>` per provider declared in its `providers.json` (snapshot: that provider's models). Its terminal command `capabilities` lists them, and `pairings` shows each peer's hello. The person switches each on or off in its terminal (actuators start off); a switched-off one is absent with why. The wire contract is docs/companion-protocol.md |
| a client | dry-server | `POST /api/capabilities/peers {url, code, name?}` joins, `POST /api/capabilities/peers/<name>/leave` leaves, `GET /api/capabilities` lists. Threads read the stream and send the heartbeat. Snapshots and events: `Caps::tick` on the host's 2 s tick. Fired events wake the engines that use the capability (`capability::users`) |
| a client | the web app | pairing the companion in Settings → Companion also joins its capabilities (`caps::companion`). The stream is read with fetch (EventSource cannot send the token), and the heartbeat runs on the page's 2 s tick |

**The key use.** The wasm runner in the browser pairs with the companion,
and then its engines' `terminal`, `file` and `browser` are the companion's.
A build an engine runs (`run(command="cargo build")`) goes through the
same flow and runs on the host with the host's tools. Declared tools of the
host (`battery.status`) are called over MCP.

### Switches: the provider's side of an approval

A companion lists every capability with a switch when it starts
([docs/companion-protocol.md, Switches and approvals](companion-protocol.md#10-switches-and-approvals)). **Actuators start off**: `terminal` and
every capability with an `approval: yes` tool. The person flips them in
the companion's terminal (or `--on` / `--off`); the choice is saved in
`<folder>/.dry/companion.json`. A switched-off capability is `absent`
(`switched off in the companion's terminal (type: on <id>)`), so it leaves
every prompt and page like any absent one, and a flip sends
`tools/list_changed` so peers re-register. An actuator's call needs both:
the engine's `allow:` rule (core, before the call) and the switch (the
provider, at the call).

### Model providers are capabilities

Each entry of a companion's `providers.json` (`cli`, `port` or `native`,
[docs/companion-protocol.md, Providers](companion-protocol.md#8-providers-providersjson-declared-not-scanned)) is a capability `model.<name>`: present when
its probe is ok, absent with why and the fix otherwise; its snapshot is its
models. They come through the handshake like any other, the page's model
picker lists `<name>/<model>`, and an engine sees no difference between the
kinds.

## 5. The browser's own capabilities (`dry-web` `caps.rs`)

Each one is probed at boot. A permission that needs a person's gesture
shows as **needs-grant**, with a **Grant** button on Settings →
Capabilities. That click calls the browser's prompt directly, because a
gesture is only good for that one call.

| id | how | tools |
|---|---|---|
| `folder` | the File System Access API: the folder you pick (read when granted, up to 2000 files / 16 MB of text) is this tab's `file` capability. A file artifact with `root: folder` reads it, and writes go back to the disk | via `file` |
| `transcribe` | Web Speech: the composer's **Voice** button (it is enabled where `transcribe` is present) | none |
| `model.local` | Chrome's Prompt API (`LanguageModel`): present when `availability()` says available | `model.local.ask(text)` |
| `camera` | `getUserMedia`, after Grant | `camera.snap()`: the frame's size (the image waits for vision models) |
| `screen` | `getDisplayMedia`, after Grant | `screen.frame()` |
| `notify` | `Notification`: present when allowed, needs-grant before | `notify.send(title, body?)` |

`file` and `terminal` are the page's **environment**, the browser VM
(docs/container.md, "The environment"): absent with why while it boots
(`the environment (browser-vm) is still booting (…)`) or if it failed,
present once it is ready, with `/work` as the root `.` (a granted folder
is a `file` too). The compiled ones the page does not have are absent,
and each says what would provide it: `browser`, `evidence` (pair a
companion). Paired with a companion, `file`
and `terminal` are its workspace folder: a `file` or `terminal` artifact
with a relative root (`.`, `src`) reaches the folder through
`dry_core::workspace::Folder` (the `live::Host` over the companion's
`/v1/fs` and `/v1/term`), the same artifact code the desktop runs; the page's own `file` and
`terminal` are then absent (the companion's computer is the environment).
`container` is present (Alpine in v86, `#/container`).

## 6. Settings → Capabilities

`routes/settings/capabilities/page.json`, the `capabilities` component (the
html renderer and GPUI). It lists every capability grouped by provider
(compiled, declared, each peer with its hello and whether it is connected),
with its state, why, tools (an actuator is marked), events, and snapshot.
One that is present but not used (another provider's is used for its id)
is dimmed. A needs-grant one has its Grant button. On the host there is a
**Join a peer** form (address, pairing code, name) and a **Leave** button
per connected peer.

## 7. Tests and the end-to-end proof

```sh
cargo test -p dry-core capability       # registry merge, effective, absent, routing; self-injection;
                                        # kind: aliases (same toolbox); uses: header kept; the declared
                                        # parser (examples, line errors, argv safety); probes; events
                                        # (edge-triggered); approval and input checks before the wait;
                                        # the MCP manifest round trip, notes, hello
cargo test -p dry-tools capability      # commands without a shell, injection stays one argument,
                                        # timeouts, a failing probe, the shipped files load, drafts read
cargo test -p dry-server a_peer         # a fake peer: join (hello, manifest, first snapshot), a routed
                                        # call, its event fires once, list_changed re-registers, the
                                        # stream closing makes it absent and unroutable
node --test companion/dry-companion-capabilities.test.mjs
                                        # the same parser in JS, argv safety, the handshake (hello both
                                        # ways, _meta grouping, resources, a broken file listed with its
                                        # line), a declared call, the stream (an event, list_changed)
```

The end to end run (2026-11-23), on scratch copies and scratch ports only:

1. A companion on `:7791` with the repo's `capabilities/`. A dry-server on
   `:8791` with a scratch root and store and `DRY_CAPABILITIES=none`, so a
   battery can only come from the companion. A fake OpenAI-compatible
   model on `:1299` that calls `battery.status` when it is among its
   tools.
2. `POST /api/capabilities/peers` with the companion's code: `companion
   joined (dry-companion darwin arm64): +battery, browser, container, …`.
   `battery` is `remote:companion`, present, effective, with its snapshot.
3. An engine with the artifact `uses: [battery]`. Its prompt had `-
   battery.status(): …` in Tools and `### power — capabilities: battery` /
   `#### battery (remote:companion)` with the doc and the pmset line in
   Artifacts. The engine called it: `battery.status() → Now drawing from
   'AC Power' … 100%; charged`.
4. The companion stopped: `companion left (companion's stream closed)`,
   and `battery` became absent. The next prompt's artifacts slot was `None
   yet.`, there was no battery tool, and the engine answered that it had
   none.
5. The web app was served by the companion. Pairing in Settings → Companion
   joined its capabilities (`peer p1 (dry-web) listening`). Settings →
   Capabilities showed the page's own (folder, camera and screen
   needs-grant; transcribe present; terminal and file absent), and
   `remote:companion` with terminal, file, browser and battery used. An
   engine given the project wrote `uses: [battery]`, then called
   `battery.status(); run(command="node --version")` in one act: the
   battery over MCP and `v22.22.3` from the host's terminal. After the
   companion stopped, the next prompt had no files, terminal, browser or
   battery.

## Not yet

- A person's approval for an actuator, the way sessions have one
  (`dry_core::approval` records). Today it is an `allow:` rule or a refusal
  that says how to allow it.
- The window (GPUI) uses its own compiled and declared capabilities. Peers
  join the host, so open the host's page for them.
- A camera or screen frame as an image in the prompt (waits for vision).
- The folder is read once when granted. Edits outside the page are not
  seen until it is granted again.
