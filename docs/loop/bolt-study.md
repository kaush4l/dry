# What bolt.new and bolt.diy do, and what dry takes

Read 2026-11-23 from `stackblitz/bolt.new` and `stackblitz-labs/bolt.diy`
(both MIT), main branches. Paths are bolt.diy's unless marked. Nothing is
copied; the words here are dry's own.

## 1. The workbench

- **Split.** Chat on the left (min 533 px, messages capped at 33 rem), the
  workbench on the right, under a 54 px header (`chat/BaseChat.tsx`).
- **Three views** on one slider: Code, Diff, Preview
  (`workbench/Workbench.client.tsx`). The workbench slides in over 0.2 s.
- **Code view.** A file tree (20%) beside the editor (80%) with a
  breadcrumb, over a collapsible terminal (25%) (`EditorPanel.tsx`,
  `terminal/TerminalTabs.tsx`).
- **While the model writes.** The store selects the file being written and
  switches to Code unless the person chose a view
  (`lib/stores/workbench.ts`); the editor is read-only and refreshed every
  100 ms. Preview is picked the first time a port opens.
- **In the chat.** One card per piece of work with a list of actions, each
  with a state icon: pending, running, done, failed (`chat/Artifact.tsx`).
  File names are chips that open the file; a command shows as a code line.

## 2. The look

Tokens are `--bolt-elements-*` (`app/styles/variables.scss`), from
primitives in `uno.config.ts`.

| Token | Dark | Light |
|---|---|---|
| background depth 1 / 2 / 3 | `#0A0A0A` / `#171717` / `#262626` | `#FFFFFF` / `#FAFAFA` / `#E5E5E5` |
| depth 4, borders | white 5%, white 10% | `#171717` 5%, 10% |
| active border | `#9C7DFF` | `#8A5FFF` |
| text 1 / 2 / 3 | `#FFFFFF` / `#A3A3A3` / `#737373` | `#0A0A0A` / `#525252` / `#737373` |
| code background | `#262626` | `#F5F5F5` |
| success / error | `#4ADE80` / `#F87171` | `#22C55E` / `#EF4444` |
| terminal text, red, green, blue | `#eff0eb`, `#ff5c57`, `#5af78e`, `#57c7ff` | `#333333`, `#cd3131`, `#00bc00`, `#0451a5` |

Accent: bolt.diy purple (`#9C7DFF`, `#8A5FFF`, `#7645E8`), bolt.new blue
(`#2BA6FF`, `#1488FC`). A primary button is the accent at 10% (20% on
hover) with accent text. Inter for the UI; the editor 12 px at 1.5; the
terminal Menlo 12 px. Easing `cubic-bezier(0.4, 0, 0.2, 1)`, 150 to 200 ms.

## 3. The loop

- **The old protocol** (bolt.new, and bolt.diy's default prompts): the
  model writes one artifact holding actions (`file` with the whole content,
  `shell`, `start`). A stream parser emits each action as it opens; files
  stream into the editor; commands run in order in the page's shell.
- **Its feedback is a button.** A failed command raises an alert; the
  person presses "Ask Bolt" and the error goes back as a message. The model
  never sees a successful output, and a server that logs errors but stays
  up raises nothing.
- **The new agent mode** (bolt.diy, `lib/.server/llm/agent-loop.ts`):
  native tools that read, write, replace lines (line numbers plus a
  revision hash, so a stale patch is refused), run a command and read a
  process's output; results go back to the model. A plan is forced first.
  **Done is refused unless a run and a passing test happened after the
  last edit.** Output is cut to its last 32,000 characters.

## 4. Local models

- Chat is proxied through bolt.diy's server, so chat needs no CORS; only
  its settings checks call LM Studio from the page.
- Models are found from `/v1/models`. Every local model is assumed to take
  8,000 tokens; LM Studio gets no context handling.
- Small-model prompts are short variants of the big one
  (`lib/common/prompt-library.ts`).

## 5. The environment

WebContainer: Node and wasm in the page with its own shell. No native
programs, no compilers, no git; python is the standard library only. The
preview is an iframe on a URL the container gives when a port opens.

## What dry takes

| From bolt | In dry |
|---|---|
| Chat beside a code view that follows the file being written | the live view (`/desk/:id/live`): the editor shows the engine's latest file, the panel its latest run, and the person's chat links to it |
| Action list with state icons in the chat | the session stream's tool lines already do this; a file name in one opens the live view on that file |
| Dark depth scale, one accent, flat panels, mono 12 px | a theme, `themes/bolt.md`, on dry's own tokens |
| Done only after a run that passed since the last edit | a rule in core for an engine that holds a terminal and a file artifact (the reply is refused with what is missing) |
| Tail-cut output | already there (the terminal artifact's last lines) |
| Short prompt for small models | the developer template's phases and skills are cut down and measured on the local model |

## What dry does not take

- A feedback loop that needs a person's button: dry's `run` result is the
  next step's input, always.
- Whole-file rewrites as the only edit, and a fixed 8,000-token guess: dry
  asks LM Studio for the loaded window.
- Rewriting an error into a canned hint: the model gets the exact lines.
