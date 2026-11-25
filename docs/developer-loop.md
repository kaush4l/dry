# The developer loop (notes)

Owner's goal (2026-11-22): the most accurate developer loop. Given a goal:
look at the repository, the current code and its style, plan the change,
make it, then test it: build, run, check the UI. A failed test adds notes
and goes back to triage. Notes, not built yet.

## 1. The loop as phases

All of it fits what exists: a template (`spec/template.md`) with phases,
`goto_phase`, the `file` and `terminal` artifacts, skills per phase. No
step limits; the context window is the only limit (compression at 75%).

```markdown
| phase  | tools                     | skills  |
|--------|---------------------------|---------|
| triage |                           | triage  |
| look   | open, focus, run          | explore |
| plan   | open, focus               | plan    |
| code   | open, focus, edit, save   | style   |
| test   | run, start, background, evidence | test    |

## phase: triage
Read the goal and any notes from the last test. Decide what is asked,
what is done already, and where to go next.

## phase: look
Read the repository: layout, the code the goal touches, how it is
written (naming, comments, tests). Write what you found as notes.

## phase: plan
Write the plan as the `plan` artifact: files, changes, the test that
proves it.

## phase: code
Make the changes, file by file, in the repository's style. Save each.

## phase: test
Build, run the tests, then evidence(op="run", expect="<the scenarios the
plan said would change>"): it runs the app and compares what it shows.
Pass: reply. Fail (a test, or an unexpected change): write notes (what
failed, the output, the changed shots and text lines, what to change) and
goto_phase(name="triage").
```

- **Notes** are one plain artifact (`artifacts/notes.md`) that every phase
  reads in its snapshot and the test phase writes. Triage reads them first,
  so a failure is never lost to compression (snapshots are never
  compressed; history is).
- **Evidence** (docs/evidence.md, built 2026-11-23): the test phase's view
  of the running app. The plan names the scenarios a change should change;
  the test phase passes them as `expect=`. An expected change passes and
  becomes the baseline; an unexpected one fails, and its shot, percent and
  changed text lines (in the evidence artifact's snapshot) go into the
  notes, so triage sees exactly what moved.
- Accuracy comes from the snapshot rule: the code the engine edits is
  always the file as it is on disk now, never a stale copy in history.

## 2. Running the app and showing it to the tester

The question: what do we need so the loop can run the application and the
developer artifact carries a snapshot of the page, for the tester to
verify?

- **A long-running app:** `start(command)` already runs it (a dev server,
  `cargo run`), and the session's output tail is in the snapshot.
- **A view of what it shows.** Vision is planned, not standard yet, so the
  snapshot is text first, an image later:
  - **Web targets** (dx serve, a dev server): a headless browser opens the
    URL and gives back the page as text: its accessibility tree (roles,
    names, values) plus console errors; a screenshot file beside it for
    when vision lands. Candidates: Playwright's CLI or a small Chromium
    driver (CDP) run by `dry-tools`; Bun's built-in webview is the same
    idea for Bun apps. One new artifact kind, `kind: browser`:
    `open(url)`, `click(target)`, `type(target, text)`; snapshot = the
    page's tree as text, the errors, and the screenshot's path.
  - **Our own GPUI app:** it does not need a browser. Pages are data (the
    closed `Component` enum, routes as files), so the app can render any
    route as text, the same view tree a terminal shell would draw. A
    `dry view <path>` command (and a tool) prints the page as the tester
    would read it: headings, cards, states, values. That is the snapshot,
    renderer-neutral by construction. A screenshot needs a bundled app
    (the unbundled binary is invisible to screen capture); later.
- **Where it lands:** the `terminal` artifact (the developer artifact)
  holds the running session; the `browser` artifact (or `dry view`
  output) holds what it shows. Both are snapshots, so the tester always
  reads the app as it is now.
- **Safety:** the browser opens only URLs on the machine (localhost)
  unless the engine's spec allows more. Unattended runs wait for the
  approval system (planned).

What is needed, smallest first:

1. A `notes` convention (a plain artifact read by every phase).
2. The developer template above, shipped as an example engine spec.
3. `dry view <path>`: a route rendered as text, from the page data.
4. `kind: browser` with a headless browser (built: the companion's,
   docs/companion.md), accessibility tree as text, screenshot path.
5. **Evidence at test time (built 2026-11-23, docs/evidence.md):** every
   project's `evidence.md` names scenarios (route, steps) and fixed
   viewports; `dry evidence` builds, starts the app (a `start` session,
   stopped after), shoots each scenario in headless Chrome (the installed
   one, over CDP, deterministic: same input, same bytes), writes the PNG
   and the page as text into `evidence/`, and diffs against the committed
   baseline (pixels with a tolerance and a threshold, changed regions
   boxed, a diff image, the text lines). `kind: evidence` gives an engine
   `evidence(op="run"|"read")`: one line back, the summary in the
   snapshot; its artifact page shows before, after and diff for each
   changed shot. Dry's own web build is the first project
   (`evidence.md` at the root).
6. Images in snapshots when vision is standard (the evidence PNGs are
   ready for it).

## 2b. The Code page is the human view of the developer artifacts

The owner's rule: the `/code` page shows a person what the engine sees,
live, from the same state. Nothing on it is a second copy.

| Code page | Shows | From |
|---|---|---|
| explorer (left) | the files present, open ones marked | the `file` artifact's root |
| main view (tabs) | the open files, focus lines, unsaved edits marked | the `file` artifact's `open:` / `edit:` |
| terminal panel | running sessions (command, time, output tail), the last command run | the `terminal` artifact's sessions and logs |

It is not an exact copy. A person sees two or three files at a time; a
model, within its context, holds many more open at once. And every edit
replaces the file in the next render (the snapshot is the file as saved
now), so edits cost no history. An interactive mode (later) can navigate
and highlight code for the person. The engine page carries the exact view:
each artifact's tools, state and snapshot, and the rendered prompt.

### The live view: UI the developer artifacts attach (built 2026-11-22)

The owner: "for components, we can attach UI elements. So a developer
artifact will have the live view ability." An artifact kind declares the
parts of a workbench that show it (`Live::view`, in `core/liveview.rs`):
`file` attaches the explorer and the editor, `terminal` the panel; every
live view also has the chat and the status strip. Core builds what each
part shows (`liveview::build`, from the exact view `inspect::artifacts`,
with tests); GPUI only draws (`gpui/src/views/live_view.rs`). A page
places the parts as data, one `live-view` component per part
(`{"type": "live-view", "part": "explorer"}`) inside `panes`
(`components/live-workbench.json`), so the same parts can make other
layouts.

| Part | Shows | From |
|---|---|---|
| explorer (left) | OPEN EDITORS (every file the engine has open, ● unsaved), then the root as a tree: folders first, open files bright and marked, folders holding them open, unsaved marked ● and carried up to their folders; secret-looking files never listed | the `file` artifact's root and `open:` / `edit:` |
| editor (centre) | one tab per open file (a model may hold eight); the file as it is now with its unsaved edits laid over; lines outside the engine's focus dimmed, changed lines marked in the gutter; a tab reveals its focus when picked; breadcrumbs say what the engine sees | the same |
| panel (bottom) | the terminal sessions: the picked one's command, output tail and status beside the list of all, running first, each read as server, test, build or command from its command (`liveview::Role`) | the `terminal` artifact's sessions and logs |
| chat (right) | the engine page's `chat` and `composer` (asks, tints and all), newest message in view | the engine |
| status (bottom) | engine, activity on its tint, model, `n open · n unsaved · n running`, the prompt gauge, when the snapshot was taken (click: take it again), links to the engine and artifact pages | activity, context, inspect |

Routes (both `nav: no`, both `use` the workbench):

- `/desk/:id/live`: all the engine's file and terminal artifacts in
  one workbench. The engine page's Artifacts section links it first
  (Live view · Open ›) when the engine holds one.
- `/desk/:id/artifacts/:name/live`: one artifact's (its page's Live
  view › button). A part the artifact does not attach says so and links
  the engine's whole view.

The look is a flat workbench (changed 2026-11-23): each live route folder
has an `overrides.md` with the Code page's dense, square sizes and the
colours of `themes/bolt/theme.md` (a depth scale of near-blacks, hairline
borders, one purple accent, learned from bolt.new and bolt.diy:
docs/loop/bolt-study.md), in light and dark, so it follows the app's mode.
The colours are written out there, so the workbench looks the same
whatever theme the rest of the app wears; `bolt` is also a theme to pick
in Settings for the whole app. No scene is drawn behind it. (Before: the
code editor's Light Modern and Dark Modern.)

**It follows the work** (2026-11-23):

- The file the engine touched last (`liveview::last_file`: the newest
  open, focus, edit or save in its answers) is the first tab, and the
  first tab is the one a shell shows. A file being made shows before it is
  saved, its lines marked as unsaved.
- The newest or running session is the first in the panel. In the page it
  is drawn again on the 2 s tick while a turn or a started command runs.
- An `.html` file has a **Preview** above its code: the page it makes, in
  a frame that runs the page's own script and nothing else (no network, no
  way into the app: `html/render/live.rs` `preview`). HTML shells only.
- The Desk leads there: under the post you wrote, the thread shows the
  work it led to (`desk::follow`: the child's turns, then the report), and
  an engine that holds files or a terminal has a **Code ›** link to its
  live view on its row.

On the web (2026-11-23): the same parts are drawn by the shared HTML
renderer (`dry_html::render::live`), so the browser app shows a folder on
the person's computer through a companion (docs/companion-protocol.md;
`dry_core::workspace::Folder` is the `Host` there), and the web host shows
its own.

Live, without reading the disk per frame: the view is cached until the
next snapshot (`Inspects::live`), taken after each step, after each
callback (a session that ended), on Refresh, and every 2 s while a live
view with a running session is on screen (`AppView::refresh_live`, from
the 1 s tick), so a server's output tail moves. The `/code` route stays
as it was: the owner's own browsing workbench.

### The exact view on the engine page (built 2026-11-22)

`dry_core::inspect` builds it from the same render the turn sends
(`prompt::render_with` with the real `Disk` host), and the engine page
first showed it in two components (since redesigned, below):

| Component | Shows |
|---|---|
| `artifacts` | one card per artifact, closed to a line (name, kind, meaning, tool count, size); open: its tools as the Tools slot describes them, its state (a `file` artifact: root, open files with focus ranges, unsaved edits; a `terminal` artifact: sessions with mode, status or exit code, age, command), and its section of the artifacts slot exactly as sent. A plain artifact shows its text as sent. |
| `response` | under PROMPT, the rendered prompt slot by slot (instructions, phase, tools, artifacts, history, response), each with its size in tokens and characters and open to its text, an empty one with why; notes (a broken template, a phase answered another way); the `prompt ≈ … of … tokens` line; Copy as text (the whole prompt, `Rendered::to_text`). |

### The engine page and the artifact pages (redesigned 2026-11-22)

The owner found the page messy to follow. It is now laid out as data
(`routes/desk/[id]/page.json`), from new components in the closed
`pages::Component` enum:

| Part | Component | Shows |
|---|---|---|
| top strip | `engine-header` | name, kind, activity on its `activity_*` tint, model, the goal's first line, and a context gauge (`prompt ≈ x of y`, from the last call or the snapshot's estimate) |
| two columns | `columns` | `main` left, `side` right (340 wide) where the box has room (`AppView::content_width`); one column, main first, on a narrower or compact screen. A page with columns may grow to 1280 on a wide screen (`layout::Frame::widened`). |
| main | `session`, `composer` | one stream (2026-11-23, docs/desk.md): each message you sent (or the thread posts that called it), then its turn's thoughts, tool calls one line each (click opens the call and result), the reply, asks; the composer pinned under it at every width (`chat` still draws the old message list) |
| side | `analytics` (`timeline`) | the waterfall beside the stream, never under the box |
| side | `artifacts` | only when there are any: one row each, the kind's glyph, name, kind, its state in a few words from core (`inspect::ArtifactView::summary`: `2 open files, 1 unsaved`, `1 running, 2 done`, `3 posts`, `1 child`), its tool count, and Open › to its page. Open by default. |
| side | `engine-tools` | the tools the engine may call now, grouped by source (`inspect::tool_groups`: built-in, artifact · kind, phase, skill, MCP · server), each with its call, description and arguments; closed by default, the count in the header |
| side | `prompt` | the rendered prompt slot by slot (sizes, empty slots with why), Refresh and its time, Copy as text, the context lines; closed by default |
| side | `response`, `metadata` | the contract and format; the engine's metadata and Delete engine, quiet; both closed by default |

Every section is one card (`card::panel`), its header a fold with a count
(`views::fold`; open state in `Inspects::open`, a key there flips the
default).

**An artifact's own page**, `/desk/:id/artifacts/:name`
(`routes/desk/[id]/artifacts/[name]/page.json`, `nav: no`,
component `artifact-page`): the header trail reads Home › Desk ›
<engine> › <artifact> (`routes::trail_titled`), so the way back is one
click. It shows the artifact's meaning and summary, when the view was
taken and Refresh; its body by kind (`inspect::Detail`, built in core with
the same disk and family as the snapshot); its tools with descriptions and
its state rows on the side; and its snapshot exactly as sent.

| Kind | Body |
|---|---|
| `file` | open files on the left with line numbers, the focus range lit with an accent bar, unsaved lines marked `*`; the names under the root on the right, open ones marked |
| `terminal` | each session as a terminal block: command, mode, status or exit code, age, output tail; running first, then the newest done |
| `shared` | the level's posts as a feed: author, time, text |
| `learning` | its three sections, each with a gauge of its fill against its cap and its entries |
| `engines` | each child as a card: goal, activity (on its tint), replies, last reply; click opens its engine page |
| clock, scripts, alerts, plain | the text, monospace |

The snapshot is taken when an engine page opens, when each step's prompt
is sent (that very render is kept), after each step's calls ran, and on
Refresh; it is cached (`gpui/src/live.rs`, `Inspect`), so no frame reads
the disk. Each view says when it was taken and how long ago.

So a person and the engine read the same context to make a change. Today
the page has its own workbench state plus an "Engine files" section
(open an artifact's files as tabs); the terminal panel is not yet bound to
a terminal artifact's sessions. To do: pick the engine whose artifacts
the page shows, bind the explorer root, the tabs and the terminal panel
to its `file` and `terminal` artifacts, and keep a person's own browsing
(no engine) as it is now.

## 3. Decided (owner, 2026-11-22)

- **Claude sessions get the MCP tools too**: `sessions.json` passes
  `mcp/dry.json` and `mcp/settings.json` (done).
- **Switching models keeps the context setting.** Models are switched
  mostly for cost (a cheaper model for easy phases), not for a different
  window.
- **Where tools come from:**
  - `mcp.json`: the general tools (outside servers, the app's own).
    Opting an engine into a server is listing it there; that stays the
    way an engine gets the settings tools.
  - `tools.md` (or a `tools/` folder): custom tools, written as the tool
    object and its metadata (name, description, inputs), stored as
    input, not code. Custom tools land there.
