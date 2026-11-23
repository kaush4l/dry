# Evidence

Every project can show what it looks like at test time: the app is
built, started, and every scenario is shot at fixed viewports in headless
Chrome, as a PNG and as the page in text (its accessibility tree). The
shots land in `evidence/`, are compared with the last committed ones, and
any visual change is visible: a percent, boxes around where it changed, a
diff image, the text lines that changed. Built 2026-11-23.

```text
evidence.md ─▶ build ({out}) ─▶ run ({port}) ─ready─▶ Chrome, per scenario × viewport:
  fresh context · fixed size, scale 1 · reduced motion · pinned clock, seeded random ·
  only the app's own origin loads · load + network idle + fonts · steps ·
  animations finished · masks painted · PNG + page text
─▶ compare with evidence/ (dry_core::evidence) ─▶ evidence/ + exit code
```

## 1. `evidence.md`

One per project, at its root, written like a template ([docs/engine-standard.md, The template](engine-standard.md#2-the-template-fixed-slots-filled-by-contributors)): a table at the top, one row per scenario, then `## ` sections of
`key: value` settings. Prose between the title and the table is for
people. Every error names its line (`evidence.md line 7: ready: is a URL`).

```markdown
| scenario | route   | viewports | steps                                       |
|----------|---------|-----------|---------------------------------------------|
| landing  | /       |           |                                             |
| home     | /app/#/ | 1280x800  | click "#welcome [data-act=load-demo]"; wait-for "Trip planner" |

## build
command: trunk build --release --dist {out}/site/app index.html

## run
command: exec python3 -m http.server {port} --bind 127.0.0.1 --directory {out}/site
ready: http://127.0.0.1:{port}/app/
url: http://127.0.0.1:{port}
timeout: 30

## viewports
1280x800
390x844
scale: 1

## capture
time: 2026-11-23T12:00:00Z
seed: 42
timezone: UTC
locale: en-US
scheme: dark
settle: 300

## masks
selector .clock
home@390x844: region 0,0,390,40

## threshold
pixel: 8
percent: 0
```

| Part | Meaning |
|---|---|
| `scenario` | a name (letters, digits, `-`, `_`); a shot is `<scenario>@<w>x<h>` |
| `route` | the path after `url` (`/app/#/settings`) |
| `viewports` | empty: every `## viewports` size; else some of them, comma separated |
| `steps` | done before the shot, `;` between: `click "<selector or visible text>"`, `type "<field>" "<text>"`, `press Enter`, `wait 300`, `wait-for "<text>"` (10 s at most), `scroll 400`, `goto "/route"` |
| `## build` | `command:` run once, in the project folder, before the app starts |
| `## run` | `command:` starts the app; `ready:` a URL that answers 200, or `log <text>` it prints; `url:` where the routes are (default: the ready URL's origin); `timeout:` seconds (60) |
| `## viewports` | `WxH` lines (default 1280x800); `scale:` the device pixel ratio (1) |
| `## capture` | `time:` the pinned clock (ISO or unix seconds), `seed:` for `Math.random` and `crypto.getRandomValues`, `timezone:` (UTC), `locale:` (en-US), `scheme:` light or dark (unset: the page's), `settle:` ms after the steps (300) |
| `## masks` | regions that change on purpose, painted flat grey: `selector <css>` (every box it matches, painted in the page) or `region x,y,w,h` (CSS pixels, painted in the PNG); for every shot, or after `scenario:` / `scenario@WxH:` |
| `## threshold` | `pixel:` the per-channel tolerance, 0-255 (8); `percent:` the share of pixels that may change before a shot counts as changed (0: any) |

`{port}` (a free port, never 8080, 8090 or 7777), `{out}` (a scratch
folder, default `target/evidence/`, or `--out`) and `{project}` are filled in.

## 2. Determinism: same input, same bytes

A shot must be the same every time for the same build, or a diff means
nothing. What makes it so (`crates/tools/src/evidence.rs`):

- **A fresh browser context per shot** (`Target.createBrowserContext`):
  no IndexedDB, storage or cache from the last one.
- **A fixed viewport** (`Emulation.setDeviceMetricsOverride`, scale 1,
  `mobile` under 768 wide), scrollbars hidden (`--hide-scrollbars`, CSS).
- **No motion**: `prefers-reduced-motion: reduce` (dry's scene keeps still
  then), animations and transitions at 0 s by an injected stylesheet,
  every `document.getAnimations()` finished (an infinite one held at 0)
  just before the shot, the caret transparent, focus dropped.
- **A pinned clock**: `Date` is replaced before the page's scripts run, so
  `new Date()` and `Date.now()` give `time:` (frozen); time zone and
  locale are set (`Emulation.setTimezoneOverride`, `setLocaleOverride`).
  Chosen over `Emulation.setVirtualTimePolicy`, which pauses the page's
  timers too, so a page that waits on a timeout would not settle.
- **Seeded random**: `Math.random`, `crypto.getRandomValues` and
  `crypto.randomUUID` from a mulberry32 seeded with `seed:` (for evidence
  only: never a real secret).
- **Ready means settled**: the load event, then the network quiet for
  300 ms (in-flight requests counted from `Network.*` events), then
  `document.fonts.ready`, then two animation frames; again after every
  step and after `settle:`.
- **Only the app's origin loads**: every request goes through
  `Fetch.requestPaused`; anything not under the app's origin (a model
  server, a companion, a CDN) is failed, so the shot never depends on what
  else runs on the machine, and the app never reaches the owner's live
  servers.
- **Chrome flags**: `--force-color-profile=srgb`, `--disable-lcd-text`,
  `--font-render-hinting=none`, `--run-all-compositor-stages-before-draw`,
  no GPU, no background networking.
- **dry encodes the PNG**: Chrome's PNG is decoded, region masks are
  painted, and it is written again with fixed settings (`png` crate,
  Balanced, adaptive filter), so the same pixels give the same file and
  sha256.

`--repeat 2` shoots every scenario twice in fresh contexts and marks a
shot `flaky` (the run fails) when the two differ: the built-in check
that a page is deterministic.

**Proof on dry's own pages** (2026-11-23, M4 Max): the 10 shots (5
scenarios × 2 viewports) from a fresh trunk build with `--repeat 2` (two
captures per shot in fresh contexts: no flaky), then three separate runs
(a new server and a new Chrome each): every PNG and page-text sha256
identical in all three, `10 shots · 0 changed · pass`, the build
fingerprint the same. A deliberate CSS change in a scratch copy of the
built landing page (`.kicker` letter-spacing .24em → .30em) gave
`2 changed (landing@1280x800 0.20%, landing@390x844 0.61%) · fail`, exit 1,
one box each around the changed line; a text change gave its `- `/`+ `
lines; putting the file back gave `0 changed` and removed the `.new` and
`.diff` files. And without trying, it caught a real one: work on the
analytics pages landing in the same tree changed Home (3.07%), Settings
(9.32%, 15.89%) and the engine page (text only, below the fold), each with
the new lines listed.

## 3. The folder

```text
evidence/manifest.json                build id, commit, time, viewports, threshold; per shot:
                                      sha256, baseline sha256, text sha256, changed %, regions,
                                      text lines changed, status, ms; removed shots; pass or fail
evidence/<scenario>@<w>x<h>.png       the baseline (what is committed)
evidence/<scenario>@<w>x<h>.txt       the page as text: roles, names, values, indented
evidence/<scenario>@<w>x<h>.new.png   only when changed and not accepted: the capture
evidence/<scenario>@<w>x<h>.new.txt   (the same, as text)
evidence/<scenario>@<w>x<h>.diff.png  only when changed: the baseline faded, changed pixels red,
                                      each changed region boxed
evidence/summary.md                   N shots, which changed and by how much, the text lines
```

The committed folder is the baseline. A run reads each shot's PNG and
text and the previous manifest's hashes (a capture with the same bytes as
the baseline is `same` without decoding a pixel), and never overwrites the
baseline unless a change is accepted. `summary.md` has no time in it, so a
rerun with nothing changed leaves it byte for byte.

| Status | When | Files | Run |
|---|---|---|---|
| `same` | within the threshold, and the same page text | baseline kept; stale `.new` / `.diff` removed | pass |
| `new` | no baseline yet | `.png`, `.txt` written | pass |
| `changed` | over the threshold, or the page text changed | `.new.png`, `.new.txt`, `.diff.png` | **fail** (exit 1) |
| `expected` | changed, and named by `--expect` | baseline replaced, `.diff.png` kept | pass |
| `updated` | changed, under `--update` | baseline replaced, `.diff.png` kept | pass |
| `flaky` | `--repeat` gave two different captures | `.new.*` | **fail** |

`build` is the fingerprint of the build output (`{out}`: every file's
path and sha256, hashed). `commit` is `git rev-parse --short HEAD`, with
`+dirty` when anything but `evidence/` changed. A scenario removed from the
table is listed as `removed` (and its files deleted under `--update`).

## 4. Diffs, in core

`dry_core::evidence` is pure and builds for every target, the browser's
wasm included:

| Module | Does |
|---|---|
| `spec` | `evidence.md`: the table, the sections, steps, times, `{port}` |
| `image` | PNG decode (any colour type, 8/16 bit) and encode (`png = "=0.18.1"`, pure Rust), flat masks, the pixel diff: a pixel changed when a channel moved more than the tolerance; regions are 8-px cells that touch, boxed (at most 20); the diff image |
| `text` | the line diff of the page text (longest common subsequence): `- old`, `+ new`, a hunk's removals first |
| `report` | one shot compared (`compare`), the manifest, `line()` (`10 shots · 1 changed (landing@390x844 0.61%) · fail`), `summary()` |
| `sha` | SHA-256 for the manifest |
| `artifact` | `kind: evidence` (below) |

Tested with synthetic images and texts (`cargo test -p dry-core evidence`).

## 5. The runner: `dry evidence`

```text
cargo run -q -p dry-tools -- evidence [--project <dir>] [--update] [--expect home,settings@390x844]
                                      [--out <dir>] [--repeat N] [--only <scenario>] [--no-build]
                                      [--chrome <path>] [--quiet]
```

It builds (`## build`, output in `{out}/build.log`), starts the app the
way a terminal `start` session does (`sh -c`, its own process group,
output in `{out}/run.log`), waits for `ready:`, drives Chrome, writes the
folder, prints one line, and stops the app (TERM to its group, then KILL)
and Chrome (`Browser.close`, then kill, its profile removed) whatever
happened. Exit 0: pass; 1: an unexpected change or a flaky shot; 2: it
could not run (the spec, the build, the app, Chrome, a step; the message
says which, a failed `wait-for` with what the page showed).

**Why Rust over CDP, not the companion's browser:** the evidence must run
where no companion runs (a terminal, CI, an engine's `run`), and the diff
is Rust already. Chrome's DevTools protocol needs only a WebSocket to
`127.0.0.1`, which is ~150 lines (`crates/tools/src/cdp.rs`: the upgrade,
masked frames out, fragmented/ping/close frames in), so there is no new
dependency and no download: it drives the installed Chrome (`--chrome`,
`CHROME_PATH`, the usual app folders, `PATH`), the same way and with the
same element finder as the companion. The companion runs this binary
rather than a second implementation (below).

**Timing** (dry's web build, 10 shots, debug `dry`, M4 Max): build
(trunk, release, cached) 11 s; ready 0.2 s; per shot 1.0-1.8 s (landing,
settings and container 1.0-1.2 s, Home 1.2-1.5 s, the engine page with
its steps 1.6-1.8 s; most of it is the settle waits: 300 ms of quiet
network after load and after each step, 300 ms `settle:`, frames); the
10 shots 12.7-13.2 s. PNGs are encoded Balanced (half Fast's bytes for
about 50 ms a shot; the encoder is optimised in debug builds too): the
folder is 2.8 MB.

## 6. In the developer loop

- **The test phase** runs `evidence(op="run")`. The convention
  ([docs/developer-loop.md, The loop as phases](developer-loop.md#1-the-loop-as-phases)): the plan names the scenarios the change
  should change; the test phase passes them as `expect="home,settings"`.
  Expected changes pass (and become the baseline); an unexpected change
  fails the run: the test phase writes notes (the shot, its %, the text
  lines from the snapshot) and goes to triage.
- **`kind: evidence`** (`dry_core::evidence::artifact`): an artifact
  `kind: evidence`, `root:` the project, `command:` what runs it (default
  `dry evidence`; for dry itself `cargo run -q -p dry-tools -- evidence`).
  Its one tool, `evidence(op="run"|"read", expect=..., update="yes")`,
  answers one line; the snapshot is `evidence/summary.md` as it is now
  (tools return short results; snapshots carry content).
- **The artifact page** (`/desk/:id/artifacts/:name`, HTML and GPUI):
  the verdict, then each shot, changed ones first: before, after and diff
  side by side, the text lines under them. The server serves the images
  (`GET /api/engines/{id}/evidence/{artifact}/{file}`, only shot PNGs
  inside the root's `evidence/`); the browser build names their paths.
- **The companion** (docs/companion.md): `POST /v1/evidence/run`
  `{project, expect?, update?}` runs the project's `dry evidence` (the
  binary from `--dry`, `DRY_BIN`, its checkout's `target/`, or `PATH`) and
  answers `{line, summary, manifest, code, output}`; `GET /v1/evidence`
  reads the last run; `GET /v1/evidence/file?project=&name=` serves one of
  its PNGs; MCP tools `evidence_run` and `evidence_read`. Stop-all stops a
  run.

## 7. Dry itself

`evidence.md` at the repo root shoots the web build: the landing page,
Home with the demo loaded, an engine page whose reply shows a table and
asks with a form (the demo's "Trip costs" engine), Settings, and the
container route not booted, at 1280x800 and 390x844. The build goes to
`target/evidence/site` (never `dist/`; not `scripts/web-build.sh`, which
wipes `dist/`), served by `python3 -m http.server` on a free port.

## 8. Not yet

- Native targets (the GPUI window): needs a bundled app to screenshot; its
  text view is `dry view` ([docs/developer-loop.md, Running the app and showing it to the tester](developer-loop.md#2-running-the-app-and-showing-it-to-the-tester)).
- Full-page shots (only the viewport now) and per-scenario thresholds.
- The browser build showing the images (it names their paths; the
  companion serves them, a blob URL away).
