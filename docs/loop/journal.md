# Hosted loop: journal

Every step of the `dev/loop` work, in order, as it happened (worktree
`../dry-learn-loop`, from `main` at `f129e64`). Times are local, 2026-11-23.
Shots are in `docs/loop/shots/`.

Goal (owner, 2026-11-23): the app is proactive and self driven. First it
runs in the browser: the hosted build connects to a local model in LM Studio
(`mlx-community/Inkling-2-Mini-oQ4e-mtp`), hosts the default (translator) and
the developer, and their loops build and test with feedback. First proof:
the local model writes a Python script that prints a centred tree of height
n, each level 2 stars wider. Prompts, skills and live artifacts are tuned
for that loop; skills may bring tools; the live view is the code view where
the work is followed; a theme learns from bolt.new and bolt.diy.

## 1. Where things stood (21:40 to 21:52)

| Check | Result |
|---|---|
| LM Studio server | on, `127.0.0.1:1234`, CORS on (`Access-Control-Allow-Origin: *` on the preflight), just-in-time loading on |
| Loaded models | none; `lms ls` lists one embedding model |
| Inkling download | running in LM Studio into `~/.omlx/models/mlx-community/Inkling-2-Mini-oQ4e-mtp`: 8.2 of 16.9 GB at 21:52 |
| Model facts (config on disk) | `model_type: inkling2_loop`, text + vision + an MTP head, 4-bit (8-bit embed and head). Its chat template adds a system line "Thinking effort level: 0.9" by default (`reasoning_effort` changes it) and declares tools natively |
| Wrong start | I first made a worktree in the old `dry` repo (the parity checker). Its static build draws a sample conversation and says "no model in this browser": no turn runs in that page. `dry-learn` is the app that runs turns in the page and is published on Pages, so the work moved here. The old worktree and branch were removed; nothing else was touched there |
| Worktree | `../dry-learn-loop`, branch `dev/loop` |
| Hosted build | `scripts/web-build.sh` passes on the baseline (37 MB with the VM image), served the way Pages serves it at `http://localhost:8771/dry/app/` (a port with an empty IndexedDB, so the owner's own store on 8765 is untouched) |
| Fresh page | the welcome box, the translator alone, "missing: no model chosen: Settings → Models → Detect" (shot `01-fresh-home.jpg`) |

## 2. The page's own Linux can run the task (21:54)

Settings → Environment on the fresh page: `Browser VM: Alpine Linux in this
page · ready · Alpine 3.24.4 x86 in v86, 256 MB · workspace /work`, and
`runtimes: none of python3, node, bun, java, rustc, cargo, tcc, gcc on PATH`.

So python is not there until someone installs it. By hand on `#/container`:

```text
$ time apk add --no-progress python3 2>&1 | tail -3; python3 --version; python3 /tmp/t.py 4
real	0m 6.62s
Python 3.14.10
   *
  ***
 *****
*******
```

The packages come from the copy served beside the app (`dist/app/vm/alpine`),
so nothing but the model call leaves the page. **Gap 1:** an engine is told
only "none of python3 … on PATH"; nothing says python can be installed, or
how (docs/elements.md, "Runtime packs", phase 3: not built).

Acceptance check for the tree task (outside the app, to judge what the model
writes): `check_tree.py` runs a script for n = 1, 2, 3, 5, 8 with n as an
argument, then on stdin, and compares the star lines with
`' ' * (n - i) + '*' * (2 * i - 1)`.

## 3. The gaps, found by reading the code (21:50 to 22:02)

Three readers mapped the browser path, the prompt and skills, and the live
view; one read bolt.new and bolt.diy ([bolt-study.md](bolt-study.md)).
What stops "write a python script, run it, fix it" in the page:

| # | Gap | Where |
|---|---|---|
| 1 | python3 is not in the VM and the engine is not told it can be installed | `environment::line` says only "none of … on PATH" |
| 2 | **No tool makes a file.** `open` needs the file to exist, `edit` needs an `old` text and an open file, `write` saves artifacts only. A new file had to be a shell heredoc inside a quoted `command` | `core/src/file.rs` |
| 3 | A developer child holds no `environment` element: it never reads where it is (`/work`) or what is installed | `engines::make` copies no `elements.md`; templates could not carry one |
| 4 | The environment line shows `/work`, and a root of `/work` was refused ("is absolute"); the prompting skill's examples use absolute roots | `environment::vm::dir` |
| 5 | A `reply` beside a `run` in one act ended the turn with a report written before the result | `turn.rs` (listed as not done in docs/default.md §6) |
| 6 | The page starts with no model until Settings → Models → Detect, and Detect could pick the embeddings model (LM Studio lists it first) | `web/src/app.rs` |
| 7 | Code inside a call: `\n` in a quoted value becomes a newline and `"` must be `\"`, so Python source is easy to mangle. No raw or triple-quoted value | `core/src/calls.rs` (to be measured on the model before changing) |
| 8 | The contract ("answer as thought/act") is appended to the newest *user* message only; after several steps it sits far back | `prompt.rs` `messages()` |
| 9 | Every wake restarts a developer at triage and drops its loaded skills; the template is written for a change in an existing project (git, tests to run first), not for making something new | `default/…/templates/developer` |
| 10 | The live view does not follow the work: the first tab is the one shown, a `run`'s output comes only when it ends, no link from the Desk, and the translator (who the person talks to) has no live view because its child holds the files | `html/src/render/live.rs`, `web/src/app.rs` |
| 11 | A theme picked in Settings never shows on the live routes: their `overrides.md` replaces every colour | `routes/desk/[id]/live/overrides.md` |
| 12 | A skill's `tools:` only names tools that exist already. A skill cannot bring a tool of its own | `core/src/skill.rs`, `live::toolbox` |
| 13 | Nothing starts work without a message: alerts are parsed and never fired; a turn ends at `reply` | docs/twelve-factor.md |

## 4. Fixed before the first model run (22:02 to 22:09)

Each with a test whose name says the rule. `cargo test -p dry-core`: 411
pass (was 408).

| Gap | Change | Test |
|---|---|---|
| 2 | `edit(path, new)` with no `old` makes a file: the whole text is kept as an unsaved edit with an empty `old`, `save` writes it (folders made, inside the root only). A file that holds text is never replaced whole. No new tool | `file::tests::an_edit_with_no_old_text_makes_a_new_file_and_never_replaces_one_that_holds_text`; `dry_tools::live::tests::files_are_read_listed_and_written_inside_the_root_only` |
| 1 | The environment line names what can be added and how: `install when needed: python3 with run(command="apk add python3"), …; nothing else can be installed here (no git, no pip)` (`environment::VM_PACKS`, the packages served beside the app). Settings → Environment shows a `can install` row | `environment::tests::the_runtime_check_reads_back_and_the_line_leaves_out_what_is_not_known` |
| 3 | A template may carry `elements.md`; the developer's holds `environment` | `engines::tests::a_child_made_from_a_template_gets_…` |
| 4 | `/work` and `/work/src` are the workspace and its folder; `engine_start(root="/work")` in the VM writes the child's roots as `.` so its files show in the workbench | `environment::vm::tests::roots_are_folders_of_work` |
| 5 | A reply beside a call that waits is held back: the run still runs, a line says why, the model replies in its next step. A reply beside `start` or `engine_start` is still sent | `turn::tests::a_reply_beside_a_call_that_waits_is_held_back_until_its_result_is_read` |
| 6 | With no model chosen the page asks the model server at load; the first choice is a loaded model that chats, else the first that chats, never an embeddings model (`served::first_choice`) | `served::tests::the_first_choice_is_a_loaded_model_that_chats_and_never_an_embeddings_model` |

The baseline binaries and `dist/` (before any of this) are kept in
`/Volumes/Backup/dry-build/loop-run/baseline/` for the first model run.

## 5. Hosting the model (22:09 to 22:16)

| Step | What happened |
|---|---|
| The download | All 13 files are on disk. The six large ones (four weight shards, `mtp.safetensors`, `tokenizer.json`) match Hugging Face's SHA-256; the seven small ones match their git hashes. 16,873,672,033 bytes = 16.87 GB = 15.71 GiB (the two numbers the owner saw are one size in two units) |
| LM Studio | Stuck at "Downloading 99%": its job marks eight small files "Checksum failed" although their bytes are right, never finishes, and so never lists the model. Also only its `omlx` runtime knows the architecture (`inkling2_loop`); `mlx-llm` was the selected one. I switched the runtime, then switched it back (`mlx-llm@1.14.0`) when the owner said to use oMLX instead. LM Studio is as it was |
| oMLX (owner, 22:14: "Run the model from omlx", port 8873) | It listed an older model that had failed to load. `POST /admin/api/reload` (no key needed on this machine): "Re-discovered 1 models", and `Inkling-2-Mini-oQ4e-mtp` was listed. First call: loaded in 3.4 s, answered "ready", 26 tokens/s, its thinking in `reasoning_content` |
| CORS on oMLX | `access-control-allow-origin: *` on the preflight: a page can call it |
| `models.json` | `local` is now `openai/c/Inkling-2-Mini-oQ4e-mtp` at `http://127.0.0.1:8873/v1` (was a model that is no longer on disk, at LM Studio's port). Changed again in §14: once LM Studio could host the model, `local` is LM Studio and `omlx` is this one |

Two more gaps from this:

| # | Gap | Fix |
|---|---|---|
| 14 | The window is asked of LM Studio and Ollama only; for oMLX it was "unknown", so 8,192 was assumed and the history was summarised early | a third probe reads `max_model_len` from `GET {base}/models` (oMLX, vLLM): `context::tests` |
| 15 | A first visit looked only at LM Studio's port | the page asks LM Studio (1234), oMLX (8873) and Ollama (11434) in order and takes the first with a model that chats (`settings::LOCAL_SERVERS`, `find_model_server`) |

## 6. Baseline: the shipped default, unchanged, on Inkling (22:16 to 22:26)

Run `base1` (`/Volumes/Backup/dry-build/loop-run/base1/`): the binaries and
the default as they were at `f129e64`, a scratch store, a scratch host on
port 8741, an empty project folder. The message, as the owner wrote it:

> Write a python script to print a tree of height n where each level has 2
> more '*'. Make it center aligned. Work in the folder <proj>.

What the model did (transcripts: `state/engines/*/convo/messages.md`):

1. **Translator**: loaded `prompting`, called it a small fix, started
   `dev-tree` from the developer template with a good brief (level k has
   2k-1 stars, centred on the bottom line, n from argv, show height 5),
   then replied with the brief.
2. **Developer, triage**: read notes and plan, `ls` and git (exit 128: not a
   repo), loaded `triage`, loaded `plan`, wrote the plan, `goto_phase(code)`.
3. **Developer, code**: loaded `code-style`; `open(path="tree.py")` →
   `error: no file tree.py under the root` (gap 2); wrote the file with a
   shell heredoc inside `run`, compiled it, ticked the plan, went to test.

What went wrong on the way, and why:

| Seen | Times | Cause | Fix |
|---|---|---|---|
| An empty answer, or the one word `reply` → "did not follow the response contract" | 3 in the first 12 answers | oMLX's log: `finish_reason=tool_calls`. The model wrote the call its own (native) way and the server moved it out of the text into `tool_calls`, which dry never read | **gap 16**: `tool_calls` are read as the answer's `act` (in the plain body and in the stream); the reasoning stands before them when there is no content. `inference::tests::calls_a_model_wrote_its_own_way_are_read_as_the_act`, `stream::tests::sse_reads_…` |
| `thought: … first.act: read(…)` on one line → "the answer has no act" | 1 | the key must open a line | **gap 17**: a required key glued to the end of another field's line opens its field there. `response::tests::toon_reads_prose_before_the_act_as_the_thought_and_a_key_glued_to_a_line` |
| A thought and no act | 2 | the model stopped after the thought | the error line already says what to do; the contract's text is made plainer (below) |
| The history summarised after 7 steps | 1 | window "unknown" → 8,192 assumed (gap 14) | fixed above |
| Two long skills loaded to learn that an empty folder is empty | every run | the template is written for a change in an existing project (gap 9) | the developer loop is rewritten (below) |

**Result: it passed.** `tree.py` is right for n = 1, 2, 3, 5, 8
(`check_tree.py`: PASS). It took 550 s and 25 answers (19 the developer's,
6 the translator's), 4 of them off the contract, and the history was
summarised 7 times. So the model can do the task; what was wrong was the
road to it.

## 7. The default loops, rewritten (22:26 to 22:50)

**The developer** (`default/…/templates/developer/`): the same three
phases, each now opening with one short line (the plan list in the prompt
shows only that line, where it used to show three whole paragraphs).

| Phase | Before | Now |
|---|---|---|
| triage | load `triage`, load `plan`, git, then the plan | `ls -la`; an empty folder or something new needs no skill; the plan is Ask / Steps / Proof (the exact command and what it must print) |
| code | load `code-style`; open, edit, save; a compile check; tests later | **write, save and run in one act**: `edit(path, new="…"); save(path); run(command="python3 tree.py 3")`, read the result, fix and run again |
| test | load `test`, load `critique`, git | run the Proof's commands with the smallest input and a larger one, compare line by line, report in a step of its own |

The long forms stay in the skills, loaded for a change in an existing
project. Skills learned to cope with a folder that is not a git
repository and with a runtime that has to be installed first.

**The translator**: it goes ahead on its own ("a detail with one natural
reading you decide, and you say which reading you took"); with no folder
named the work is in the workspace (`root="."`). Before this, one run
(`fix1`) stopped after 75 s with a form asking how many stars the first
level has.

**A skill that brings tools** (gap 12, `core/skilltool.rs`): a skill folder
may hold `tools.md` beside `SKILL.md`, in the format `capabilities/*.md`
already use. Loaded, each tool is the engine's as `<skill>.<tool>` and
runs as a command in the engine's terminal, each input one quoted
argument. The developer's new `python` skill brings `python.check`,
`python.run` and `python.test` (docs/engine-standard.md §4b).

**More slips read instead of refused**, each found in a transcript:

| Seen | Fix |
|---|---|
| A thought and no `act`, most often right after a long skill was loaded (19 times for one translator in `loop1`) | the contract is put after the newest message the model reads (your text or the last results), not left on the first message of the turn (gap 8). `turn::tests::the_contract_is_read_last_at_every_step` |
| A new child's first answer is empty | its first message was only the parent's context ("no project exists yet…"), which asks for nothing. It now opens with "Work on your goal…" and then the context. `engines::tests::a_child_made_from_a_template_gets_…` |
| `act:` with no thought → refused, a step lost | the calls run, and a line after the results says which field was left out. `turn::tests::calls_with_no_thought_still_run_and_the_missing_field_is_said` |

## 8. The live view is the code view (22:50 to 23:00)

- **It follows the work.** The file the engine touched last is the first
  tab (`liveview::last_file`); the newest run is the first session; the
  page draws it again every 2 s while a turn or a started command runs.
- **Its look** is `themes/bolt.md` written into the live routes'
  `overrides.md`: near-black depth scale (`#0A0A0A`, `#171717`, `#262626`),
  hairline borders, one purple accent, dense and square, no scene behind
  it. `bolt` is also a theme for the whole app (Settings → Theme).
- **Preview.** An `.html` file is drawn as a page above its code, in a
  frame that runs its own script and nothing else.
- **The Desk leads to it.** Under your post the thread now shows what it
  led to: the child's turns with a **Code ›** link to its live view, then
  the translator's report. Before, the thread stopped at the brief and the
  report was only on the engine's page.

## 9. In the browser, end to end (22:34 to 22:42)

The build served as Pages serves it, on a port never used before
(`http://localhost:8782/dry/app/`), so the page had no stored state.

| Time | What happened in the page |
|---|---|
| load | No model chosen, so it asked LM Studio, oMLX and Ollama. LM Studio lists Inkling unloaded (window 4,096); oMLX has it loaded. It took oMLX: `ok: 2 models listed at http://localhost:8873/v1; loaded now: Inkling-2-Mini-oQ4e-mtp` (an earlier build took LM Studio because it was asked first: now the loaded one wins) |
| 22:34 | On the Desk: "Write a python script to print a tree of height n where each level has 2 more '*'. Make it center aligned." |
| +1 min | Translator: loaded `prompting`, read its environment line ("python3 is not on PATH but is installable via apk add python3, which the child can do"), started `dev-tree-script` from the developer template with `root="."`, replied with the brief |
| +3 min | Developer: `ls -la` (empty), the plan, then `run(command="apk add python3")`: 17 packages from the copy served beside the app, exit 0 |
| +5 min | `edit(path="tree.py", new=…)`, `save`, and four runs (n = 3, 1, 5 and the default); the live view shows the file and the output (shot 03) |
| +7 min | Developer's report (Changed / Ran / Saw / Unverified); the translator loaded `judging`, accepted, and reported, naming the two readings it chose (n from the command line, level 1 = one star) |

16 answers in all (developer 9, translator 7), 3 off the contract, 10
runs, no skill loaded by the developer. The file the page's model wrote is
`runs/browser1/tree.py`; `check_tree.py` says PASS. Only the model calls
left the page.

Shots: `02` the live view at the first command, `03` the file and its
output, `04` the Desk thread with the child's work and the Code link, `05`
the whole workbench after a reload (and later `06`, `07` the counter
widget, `08` the alert that fired).

Hiccup: after a rebuild the page kept running the old build until the app's
files were fetched again without the cache (they keep their names). Noted
in docs/browser.md.

## 10. The runs, side by side

One task, one model (Inkling-2-Mini-oQ4e-mtp on oMLX, thinking effort at
its default), every run on a scratch store and an empty folder. Transcripts,
plans, the file each run wrote and its score are in `docs/loop/runs/<run>/`;
`check_tree.py` judged each file.

| Run | Code | Prompts | python skill | Wall | Answers (developer + translator) | Off contract | Summaries | Result |
|---|---|---|---|---|---|---|---|---|
| `base1` | `f129e64` | shipped | none | 550 s | 19 + 6 = 25 | 4 | 7 | PASS |
| `fix1` | first fixes | shipped | none | 75 s | 0 + 2 | 0 | 0 | stopped: the translator asked the person a form |
| `loop1` | + new loop, skill tools | new | loaded by choice | 380 s | 16 + 24 = 40 | 21 (19 the translator's, after a skill load) | 0 | PASS |
| `noskill1` | same | new, python skill removed | none | 355 s | 10 + 5 = 15 | 1 | 0 | PASS |
| `loop3` | final | new | loaded by choice | 380 s* | 11 + 4 = 15 | 1 | 0 | PASS |
| `noskill3` | final | new, python skill removed | none | 275 s* | 9 + 3 = 12 | 0 | 0 | PASS |
| `browser1` | web build | new | not loaded | about 8 min, with the python install | 9 + 7 = 16 | 3 | 0 | PASS, in the page |
| `alert1` | final + alerts, LM Studio | new, plus "look again by yourself in two minutes" | loaded by choice | 630 s*, then 260 s by itself | 13 + 6 = 19, then 3 + 3 by itself | 1 | 0 | PASS, and the re-check ran with nobody writing (section 14) |

\* The machine was also running the whole test suite and a browser run, so
these times are high.

What the numbers say:

- **The model could always do the task.** Every run that reached the
  developer wrote a correct script. The cost was in the loop around it.
- **Answers fell from 25 to 12–15**, slips from 4 to 0–1, and no history
  was summarised: the window is read from the server (1 M tokens instead
  of an assumed 8,192), the developer no longer loads two skills to look
  at an empty folder, and it writes, saves and runs in one act.
- **The contract's place mattered most.** `loop1` shows it: with the
  contract left on the first message, the translator answered in prose 19
  times after loading a skill. With the contract after the newest message
  (`loop3`): none.
- **Do skills make it easier?** For this task, no. The developer loaded
  `python` by itself and used its tools right the first time
  (`edit; save; python.check; python.run` in one act, no quoting slip),
  but it cost a step to load and it ran more commands (13 against 4). The
  run without it was the shortest. A skill's tools pay where a command is
  easy to get wrong or the owner wants one way of doing it; for one file
  and one command, `run` is enough. In the browser the developer did not
  load it and passed.

## 11. A small UI element, in the page (22:50 to 23:00)

A second fresh origin (`localhost:8783`). On the Desk: "Build a small
counter widget as one HTML file: a number, a + button, a - button and a
reset button. Dark, rounded, centered on the page."

The translator started `dev-counter` within a minute (the Desk showed its
row and the **Code ›** link while it ran). The developer wrote
`counter.html` in one `edit` (inline CSS and script, 70 lines), and the
live view drew it as a page above its code (shot `06`). I clicked the
previewed widget: + three times and − once, and it read 2 (shot `07`). So
the local model built a working small UI element in the page.

20 answers (developer 15, translator 5), 2 off the contract. The report
was honest: "What is NOT verified: the literal browser open … Open
counter.html in a real browser to confirm." Transcript and file:
`runs/browser2/`.

**Gap 18 (open):** in the page the developer cannot look at what it made.
There is no browser artifact there (it needs a companion with Chrome), so
it checked with `grep`, then spent steps trying to serve the file with
busybox. The person sees the Preview; the engine does not. A page-side
`browser` capability (the page's text and console of a sandboxed frame)
would close the loop for UI work. Until then the test phase says so: with
no browser artifact, check the file's parts with a command and report the
page as not opened (it loaded `browser-qa` and tried `browse` in this run,
before that line was written).

## 12. Left open

| # | What | Note |
|---|---|---|
| 13 | An engine wakes only by the clock | built in §14 (alerts fire). Not built: `watch` triggers (wake when a file or a page changes), and firing while no shell is open |
| 18 | The developer cannot see a page it made, in the browser build | above |
| – | Done is not refused without a passing run after the last save | a reply beside a run is held back; the stronger rule (bolt.diy's) is not built |
| – | A run's output shows when the run ends | the panel is drawn again every 2 s, but the VM returns a `run`'s output whole |
| – | GPUI | the live view follows the work there too (the first tab), but the Desk's follow rows, the Code link and the Preview are in the HTML shells only |
| – | LM Studio's runtime | hosting this model there needs its `omlx` runtime selected for MLX (it was `mlx-llm@1.14.0`, which cannot load it); other MLX models now load with `omlx` too. `lms runtime select mlx-llm-mac-arm64-apple-metal-advsimd@1.14.0` puts it back |
| – | Thinking effort | left at the model's default (0.9); a lower one may be faster, not measured |
| – | After a reload the status strip says "window assumed" (8.2k) | the window is asked again at the next turn; only the gauge is wrong until then |
| – | A first answer that is empty | still happens about once per engine; the retry line recovers it |

## 14. Proactive: an engine that starts by itself, and the model in LM Studio (23:00 to 23:20)

The first pass ended with two things not met. Both are now.

**The model is hosted in LM Studio.** Its stuck job had cleared and it
listed the model, but `lms load` failed: `Model type inkling2_loop not
supported` (its `mlx-llm` runtime). With LM Studio's own `omlx` runtime
selected, `lms load inkling-2-mini-oq4e-mtp -c 32768` loaded it in 27.5 s
and `/v1/chat/completions` on port 1234 answered. `models.json`: `local`
is the model in LM Studio (`http://127.0.0.1:1234/v1`), `omlx` the same
model on oMLX. A fresh page (`localhost:8784`) found it there by itself:
`ok: 2 models listed at http://localhost:1234/v1; loaded now:
inkling-2-mini-oq4e-mtp`.

**Gap 13, closed: alerts fire.** `core/alert.rs` already read
`spec/alerts/*.md` and decided when one is due; nothing looked.

| Piece | What |
|---|---|
| Firing | `alert::settle`, called from `turn::callbacks`: the path every shell (host, window, page) already ticks every second or two for a child's reply or a command that ended. An alert that is due is one more callback: a message, a new turn |
| The tool | the alerts artifact brings `alert(name, trigger, text, once?)`; the same name replaces, `text=""` removes; at most 8 |
| The rules | never at the moment it is made; a moment nobody looked at within 120 s is missed and said, never fired late; an engine in a turn is looked at when the turn ends; `once` and a dated `at` are removed when they fire |
| Memory | first seen, last fired, how often: `alert: …` lines in `convo/run.md`, so a restart does not fire a moment twice |
| The clock | `Host::clock`: the machine's or the browser's wall clock, so `at 09:35` is the person's 09:35 |
| The default | the translator holds `artifacts/alerts.md`; its goal says when to set one and to write its text as a brief to itself |

Tests: `alert::tests::an_alert_that_is_due_wakes_its_engine_once_and_a_spent_one_is_removed`,
`alert::tests::an_engine_makes_replaces_and_removes_its_own_alerts`,
`turn::tests::an_engine_sets_its_own_alert_and_is_woken_by_it_with_nobody_writing`.

**Proof in the page** (`runs/browser3/`, shot `08`), the model in LM
Studio. On the Desk, at 23:09: "In one minute, by yourself, tell me it is
time to stretch. I will not write again."

1. The translator's thought: "This is an alert, not work for a child
   engine: set an alert with trigger "every 1m" and once="true"".
   `alert(name="stretch-reminder", trigger="every 1m", …)` → `made alert
   stretch-reminder: every 1m, once.` It replied: "Got it. I'll tell you
   in one minute that it's time to stretch — no need to write again."
2. Nothing was sent. A minute later a second turn began by itself. Its
   thought: "The one-minute alert has fired, as expected." Its reply:
   **"Time to stretch. Stretching break — take a minute."**

The thread shows both turns under the one post.

**Proof on the host, with real work** (`runs/alert1/`), the model in LM
Studio. One message: the tree task, then "When that is done, look at it
again by yourself two minutes later: have it run for n=4 and tell me what
it printed."

1. The translator briefed a developer; it wrote `tree.py` (13 answers, 1
   slip, PASS). The translator judged the reply, then:
   `alert(name="tree-recheck", trigger="every 2m", once="true", text="Re-check
   the tree script … have dev-tree-script run `python3 tree.py 4` … Then
   tell the person exactly what it printed.")` → `made alert tree-recheck:
   every 2m, once.` It reported the script and said the re-check was set.
2. Hiccup, the harness's: it stopped the host 24 s after the alert was
   made (its wait counted from the first message, and the task took 606 s
   beside the test suite). The host was started again on the same store,
   71 s after the alert was made, with no message sent. The alert's mark
   was in `convo/run.md` (`alert: tree-recheck 1795493888 - 0 0`), so the
   two minutes still counted from when it was made.
3. At two minutes a turn began by itself: "Alert tree-recheck fired (every
   2m): Re-check the tree script…". The translator's thought: "I must not
   just repeat the first run's output as if it were fresh." It sent the
   developer the re-check; the developer ran `python3 tree.py 4` and
   `… | cat -A`, exit 0; the translator replied with the four lines,
   character for character. The alert, spent, was removed.

So an engine sets its own later work, a restart does not lose it, and the
work it starts is the same loop as any other (a brief, a child, a judged
reply).

## 13. Where everything is

| What | Where |
|---|---|
| This journal, the bolt study | `docs/loop/journal.md`, `docs/loop/bolt-study.md` |
| Every run: the message, each engine's whole conversation (thoughts, calls, results), its plan, the file it wrote, the score | `docs/loop/runs/<run>/` |
| The exact prompt an engine was sent (`dry prompt`), before and after | `docs/loop/runs/base1/prompt-*.txt`, `docs/loop/runs/loop3/prompt-*.txt` |
| Shots of the page | `docs/loop/shots/01` … `08` |
| The shipped loops | `default/engines/translator/spec/` (the translator's goal and skills; `templates/developer/` with its phases, its skills and `skills/python/tools.md`) |
| The theme | `themes/bolt.md`; the live routes' `overrides.md` |
| Scratch (stores, binaries per run, logs) | `/Volumes/Backup/dry-build/loop-run/`, `/Volumes/Backup/dry-build/loop-logs/` |

Checks at the end: `cargo test --workspace` passes (core 431 once on main, with the
server, tools, html, web and window crates), and the browser build compiles
(`cargo check -p dry-web --target wasm32-unknown-unknown`).

The changes are one commit on branch `dev/loop` in the `../dry-learn-loop`
worktree, not pushed and not merged, for the owner to read and run. The one file
outside it is a preview launch entry for the session's own browser pane
(`~/Downloads/Dev/dry/.claude/launch.json`, not tracked).

To see it: `scripts/web-build.sh`, serve the folder above `dist` as in
docs/browser.md, open `http://localhost:<port>/dry/app/` beside a running
oMLX (or LM Studio) with a model loaded, and write on the Desk.
