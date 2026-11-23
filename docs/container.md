# Containers: a Linux the engine can build in and look at

Goal: a developer environment the engine loop (triage, look, plan, code,
test) can use for evidence. Tools create a container, run build and run
commands in it, and read the UI it serves. This page is the spike's
result: one backend (`BrowserVm`, Alpine in the page) built and measured,
the rest sketched.

## The environment

Every shell reports one **environment**: the machine its `file` and
`terminal` tools run on (`dry_core::environment`, carried on the
capability registry as `Registry::environment`, shown in **Settings →
Environment**). A simple check picks it at start (`environment::probe`):

| kind | picked when | where the tools run |
|---|---|---|
| `Host` | the shell can start processes (dry-server, dry-gpui, the CLI: `dry_tools::capability::probe_compiled`), or a page paired with a companion | the real disk and processes (the companion's, in a page) |
| `BrowserVm` | a browser build with no companion | Alpine in v86 in the page (`dry_web::env`); the workspace is `/work`, a root `.` is `/work` |
| `Local`, `Remote` | (named, not built) | docker on this computer; the dry server's host |

The tool names and results are the same everywhere: `live::Host` is the
seam. In a page with the VM only the model call leaves the page.

Settings → Environment shows the kind, the state (`booting: <what>`,
`ready`, `failed: <why>`), what runs it, the workspace and, for the VM,
the saved machine's size, with **Forget cached VM** (a fold that says what
goes first; after it the running VM is not saved again, so the next load
boots fresh). The server and the window show `Host` with this computer's
OS and the repo root. The server also starts each engine's MCP servers
(its spec's `mcp.json`) on its first turn (`Host::mcp_for`), like the
window; a call no capability takes goes to them.

### The browser VM as the environment (`dry_web::env`)

- **Start.** The app boots the VM behind the page (nothing waits on it):
  restored from IndexedDB when a machine is saved, else booted fresh and
  saved at once. Measured here: first load 14.5 MB download and a fresh
  boot, saved as 26.0 MB gzip; a reload is `ready` about **0.8 s** after
  navigation (wasm load and restore together). `file` and `terminal` are
  absent with the reason while it boots or if it failed, present once it
  is ready, and absent while a companion is joined (its are used).
- **One command at the console at a time.** Every exec, start, write and
  save takes a ticket in call order and waits its turn (`vm::ticket`,
  `vm::held`): the guest's tty takes at most 4095 bytes a line and v86's
  serial queue 4096, so nothing is typed ahead, and a file written before
  a `start` is there when it starts. Ids differ per page load, so a
  restored machine's old job markers are never taken for new ones.
- **Files: a mirror of `/work`.** `live::Host` is synchronous and the
  console is not, so the page keeps `/work` in memory (at most 2000 files,
  16 MB; hidden folders as on a host). `list`/`read` answer from it. A
  `write` lands in the mirror at once and goes to the VM in its turn.
  After each `run`, and when a started job ends, the mirror is read again.
  How each operation is framed (`environment::vm`, pure, tested):

  | operation | typed into the console |
  |---|---|
  | write `rel` | `: > /tmp/dry-w-<h>.b64`, then `printf %s '<base64 chunk>' >> …` per 2400 characters, then `mkdir -p <dir> && base64 -d … > /work/<rel>` |
  | read back | `cd /work && find . \( -name .git -o … \) -prune -o -type f -exec stat -c '%s %Y %n' {} +` (the manifest), then `for f in <changed>; do printf '@@dry %s\n' "$f"; base64 "$f"; done` in groups that fit a line |
  | `run` / `start` | `mkdir -p '<dir>' && cd '<dir>' \|\| exit 1; <command>`, through `exec_line` / `start_line`; a command whose line would pass 3000 bytes is written to `/tmp/dry-cmd-<id>.sh` the same way and run with `sh` |

  Contents cross as base64 both ways, so `\r\n`, a zero byte or an exit
  marker inside a file cannot be mistaken for the console's own. A path
  with a write still on its way keeps the mirror's copy through a refresh.
- **Commands.** `run` waits without blocking the page (`companion::io`
  calls `env::run`); its exit code and last lines are the result.
  `start` / `background` are VM jobs; their log tail is read on the 2 s
  tick, and a `start`'s end is the terminal's callback message. A run
  inside a script step is refused (scripts are synchronous). Session
  output lives in the page: after a reload an old session says its output
  is gone (the files it made are kept).
- **Persistence rule.** The root is in RAM, so changes are kept by saving
  the whole machine again (`environment::Save`): after a write or a
  command run or ended, once nothing changed for **5 s**, at most every
  **30 s**, in its turn at the console. A reload inside that window loses
  the last changes. Measured: after the check below, 85 MB state, 36.4 MB
  gzip.

### The end-to-end check (2026-11-23, scratch dist on `http://localhost:8794/dry/app/`)

A fake OpenAI-compatible model on `:1299` (CORS open, scripted answers)
drove the shipped translator: `engine_start(template="developer",
root=".")` made `dev`, whose code (file) and terminal artifacts ran in the
VM:

1. `run(printf 'int main(void){return 0;}\n' > main.c && ls -la /work)`: exit 0, `main.c` listed.
2. `open(main.c)`: `opened main.c (1 line)` (from the mirror the run refreshed).
3. `edit` + `save`: `saved main.c (1 edit, 2 lines)` (the write went to the VM).
4. `run(cat main.c; apk add -q tcc …; tcc -o hello main.c && ./hello)`: the
   edited source came back, and `hello from the file tool, built by tcc in
   the browser VM`, `exit=0`.
5. `start(sleep 2; echo started-job-done > job.txt)`: the callback came
   (`session 3 … finished, exit 0`), the parent got dev's reply.
6. Reload: `ready` in 0.84 s; dev's file artifact listed `hello`, `job.txt`,
   `main.c` with the edited text. Forget cached VM: IndexedDB empty, the
   row says `none saved yet`.

## What runs now: `#/container` in the browser app

| Piece | What |
|---|---|
| Emulator | **v86 0.5.473** (2026-10-30, BSD-2-Clause), an x86 PC in WebAssembly, loaded from `cdn.jsdelivr.net/npm/v86@0.5.473/build/` when a VM boots; its BIOS from the same commit (`gh/copy/v86@315b2b0/bios/`) |
| Image | Alpine **3.24.4 x86** minirootfs + the **linux-virt 6.18.64** kernel, both from `dl-cdn.alpinelinux.org`, built into a kernel + initramfs by `scripts/vm-image.py` (no 9p root, no disk image) |
| Console | the serial port: `/init` starts `sh` on `ttyS0` with no echo and no prompt |
| `exec(cmd)` | types `sh -c '<cmd>' </dev/null 2>&1; printf '\n__dry_exit_<id>_%d__\n' $?` and reads up to that marker (`dry_core::container::{exec_line, take}`) |
| `start(cmd, callback)` | `( sh -c '<cmd>' >/tmp/dry-job-<id>.log 2>&1; printf <marker> ) &`; the callback runs when the marker comes |
| Ports | v86's own TCP stack (`network_adapter.connect(port)`): an HTTP/1.0 GET from the page to the guest, no relay, no service worker. The page shows the answer in a sandboxed `<iframe srcdoc>` and its text (`container::html_text`) as an engine would read it |
| Network out | v86's **fetch** backend: guest HTTP on port 80 becomes the page's `fetch()`. So the guest reaches anything that answers CORS, including **the page's own origin** |
| `apk add` | from a copy of the official packages served beside the app (`dist/app/vm/alpine/v3.24/{main,community}/x86/`: Alpine's signed `APKINDEX.tar.gz` as published plus the packages the checks need). apk checks the index signature and each package's hash itself |
| Persistence | **Save machine**: v86 `save_state()` (the whole PC: RAM, so the in-RAM root and running processes too), gzip (`CompressionStream`), IndexedDB `dry-vm`. **Restore saved** boots from it |
| Mounts | v86's 9p is mounted at `/mnt/host` (empty); putting the page's files there (`create_file`) is not wired |

Files: `crates/core/src/container.rs` (the trait sketch, the exec markers,
HTTP to a port; pure, tested), `crates/web/src/vm.rs` (`BrowserVm`, the
route), `crates/web/vm/vm.js` (moves bytes to and from v86),
`scripts/vm-fetch.py` and `scripts/vm-image.py` (the image),
`scripts/web-build.sh` (copies the image into `dist/app/vm/`). The image
(`vm/image/`, 12 MB) and the apk subset (`vm/apk/`, 22 MB) are kept in git
for now (owner, 2026-11-22: to move somewhere else later), so the Pages
build needs no download; the scripts below remake them.

```sh
python3 -I scripts/vm-fetch.py     # → vendor-dl/ (git-ignored): minirootfs (sha256 checked), linux-virt, the apk subset
python3 -I scripts/vm-image.py     # → vendor-dl/out/vmlinuz-virt, alpine-dry.cpio.gz
scripts/web-build.sh               # copies vm/ (or vendor-dl/out) into dist/app/vm/
# serve dist as in docs/browser.md, open http://localhost:<port>/dry/app/#/container
```

Open it at **`localhost`**, not `127.0.0.1`: the guest reaches the page's
origin as `<port>.external`, which v86 turns into `http://localhost:<port>`.
On Pages the guest uses the page's own host (v86 upgrades it to https).

### Why v86, not container2wasm

- v86 boots a stock Alpine kernel in about 2.6 s, runs single-threaded
  (no `SharedArrayBuffer`, so no COOP/COEP), ships as one npm package
  (2 MB wasm, 0.36 MB JS), and has a network stack in the page with a TCP
  `connect` to the guest: ports work with no relay.
- container2wasm runs real OCI images (amd64 via Bochs, riscv64 via
  TinyEMU) but its browser build needs `SharedArrayBuffer` (COI headers or
  a service worker), its images are large (tens to hundreds of MB per
  image), and its networking needs either its WebSocket proxy or a
  fetch-based stack of its own. Worth it later only for "run this exact
  Docker image" — not for a dev shell.
- v86's limit: 32-bit x86 (i686) only, so the Alpine `x86` packages; no
  KVM-speed CPU (it JIT-compiles x86 to wasm).

## Measured (Chrome, M4 Max, local copy at `http://localhost:8772/dry/`, 2026-11-23)

| What | Number |
|---|---|
| Download for a VM, cold | **14.5 MB** in 0.16 s locally: kernel 8.1 MB, initramfs 3.8 MB (gzip), v86.wasm 2.0 MB, libv86.mjs 0.36 MB, BIOS 0.17 MB. Over a real link this is the cost; Pages and jsDelivr cache it after |
| apk mirror on Pages | 19.5 MB (22 packages: python3 and its libs, tcc, musl-dev) + 2 indexes (0.8 MB) |
| Boot to shell (fresh) | **2.6 s** (2565–2628 ms over three boots) |
| `exec` round-trip (`true`, 10 runs) | median **11 ms** (min 10, max 24) |
| busybox loop, 10 000 iterations | 0.49–0.53 s |
| `apk add python3 tcc musl-dev …` (from the page's origin) | **7.5–9.1 s**, 23 packages, 42.9 MiB installed |
| `tcc hello.c` (compile + run) | **85–97 ms** |
| `python3 -c 'print(sum(range(10**6)))'` | 1.7–2.0 s (about 30× slower than native) |
| `python3 -m http.server 8000` start + first GET from the page | 2.0–2.8 s; then a GET is **12–16 ms** |
| Guest memory | 256 MB given (`free -m`: 219 total, 26–28 used after the checks) |
| Page JS heap (`performance.memory`) | 25–150 MB; it does not count v86's wasm memory (~256 MB + JIT), so the tab is roughly 0.4–0.5 GB with a VM |
| Save machine | 131 MB state → **52.9 MB gzip** in IndexedDB, 1.6 s |
| Restore after a reload | **0.46–0.51 s** to a working shell; python3, `/srv`, `/tmp/hello` and the **running** http.server all came back (GET 8000 answered 100 ms after restore) |

What survives a reload: nothing unless **Save machine** was pressed; then
everything in the machine (it is a snapshot, not a disk overlay). There is
no per-file overlay: the root is in RAM. A per-file one would be a 9p
filesystem backed by IndexedDB/OPFS (v86's 9p can be given a storage) with
the root on it, or a `virtio-blk` disk image with an OPFS-backed overlay;
both are possible later, the snapshot was enough for the spike.

### Bugs found and fixed on the way

- A restored machine lost what was typed before it ran: `vm.js` now waits
  for `emulator-started`.
- A restored machine's ports stopped answering (the network's MAC no
  longer matched the guest's): `preserve_mac_from_state_image: true`.
- `udhcpc` needs `AF_PACKET` (a module): the guest's address is static
  (v86's defaults, 192.168.86.100 via 192.168.86.1, its DNS there too).
- Alpine's tcc looks for `crt1.o` in `/usr/lib/i386-linux-gnu`: `/init`
  links that to `/usr/lib`.
- The request bytes for a port must be copied into a JS `Uint8Array`: a
  view into wasm memory was overwritten before v86 sent it.

## Cross-origin isolation (COOP/COEP)

**Not needed, and not added.** v86 uses no `SharedArrayBuffer` and no
threads: it ran with `crossOriginIsolated === false` for everything above.
So no coi-serviceworker on Pages (it would cost a forced reload on the
first visit and a service worker on every page for nothing).

If a later backend needs it (container2wasm, a threaded build), it was
tested with the headers served for real (what coi-serviceworker emulates),
both modes, page at `http://localhost`:

| Under `COOP: same-origin` + | `require-corp` | `credentialless` |
|---|---|---|
| `crossOriginIsolated`, `SharedArrayBuffer` | yes | yes |
| LM Studio `GET /v1/models`, `POST /v1/chat/completions` (CORS, with preflight) | works | works |
| a CORS server on 127.0.0.1 (stand-in for the companion's `http://127.0.0.1:7777`, answering `Access-Control-Allow-Origin: *`): GET and JSON POST | works | works |
| v86 from jsDelivr (sends CORP `cross-origin`) | works | works |
| the VM, its port page in `<iframe srcdoc>` | works | — (not re-run) |
| landing page and its scene | — | works |

CORS fetches (`mode: "cors"`, what `net::Fetch` sends) are allowed under
both modes; COEP only blocks no-cors cross-origin loads without CORP. The
companion server itself was not running here, so it was checked with a
stand-in that answers CORS the same way. Safari has no `credentialless`;
use `require-corp` if it is ever needed.

## Networking: what works, what it takes for the rest

- **Works with nothing else**: HTTP from the guest to the page's origin
  (the apk subset), and to any server that sends CORS headers.
- **Does not**: `dl-cdn.alpinelinux.org` sends no
  `Access-Control-Allow-Origin`, so the guest cannot fetch from it through
  the page (`HTTP/1.1 502 Fetch Error`, the console says CORS). HTTPS
  from the guest, raw TCP, and any non-80 port out do not work with the
  fetch backend either.
- **What full networking takes**: a relay. v86 speaks **Wisp** (a
  WebSocket multiplexing TCP/UDP: `relay_url: "wisp://…"`) or a raw
  Ethernet-over-WebSocket switch (`wss://…`, e.g. websockproxy). Either
  is a small server: on this machine (the dry server or a companion), or
  public relays (not used here: they see all traffic; ask first). With a
  CORS proxy (`cors_proxy`), plain HTTP to any host also works.
- **Packages without a relay**: add names to `PACKAGES` in
  `scripts/vm-fetch.py`; the build copies them beside the app. gcc is
  70 MB with its dependencies: left out (tcc compiles C).

## The design: `Container` and its backends

In `dry_core::container` (a sketch; only `BrowserVm` implements it):

```rust
pub trait Container {
    async fn create(&mut self, image: &str, mounts: &[Mount]) -> Result<(), String>;
    async fn exec(&mut self, cmd: &str) -> Result<Output, String>;            // Output { code, text }
    fn start(&mut self, cmd: &str, done: Box<dyn FnOnce(Output)>) -> Result<u64, String>;
    async fn stop(&mut self) -> Result<(), String>;
    async fn get(&mut self, port: u16, path: &str) -> Result<Http, String>;  // a port's page, as evidence
    async fn snapshot(&mut self) -> Result<String, String>;
}
```

`ports` in the first sketch became `get(port, path)`: what the engine
needs from a port is its page, and every backend can answer that (the
browser over v86's TCP, docker over a published port, the remote host
over its API).

| Backend | Where | Image | exec | Ports | Snapshot | Status |
|---|---|---|---|---|---|---|
| `BrowserVm` | the page (`dry_web::vm`) | Alpine x86 in v86 | serial console + markers | v86 TCP `connect` | `save_state` → IndexedDB | **built (spike)** |
| `Local` | this computer (`dry_tools`) | any OCI image | `docker exec` / `podman exec` | published port, `GET` | `docker commit` | sketch |
| `Remote` | the dry server host | the host's `Local` | the host's API | the host proxies | the host's | later |

How the `terminal` artifact would use it: its `run` becomes
`Container::exec` (the result is the `Output`), `start` with a callback
becomes `Container::start` (the callback is the turn's wake, as the
terminal's `settle_engine` does today), `background` is `start` with no
wake. The artifact keeps one container per engine, made on first use.

**One tool, not wired**: `container_create(image)` — the only new tool
the engine would need; everything else stays on the terminal artifact's
existing tools (`run`, `start`, `background`) plus one `look(port, path)`
for a port's page. Not added to any toolbox in this spike.

## Blocked, and why

| What | Why |
|---|---|
| `apk add` from Alpine's mirrors directly | no CORS on dl-cdn; needs a relay or CORS proxy (above) |
| gcc | 70 MB with dependencies; over the spike's 50 MB download limit. tcc works |
| 64-bit images, Docker images | v86 is 32-bit x86; container2wasm would be the route |
| per-file persistence | the root is in RAM; only whole-machine snapshots (53 MB each) |
| mounts | 9p is mounted, but nothing puts files in it yet |
| precise tab memory | `performance.memory` misses wasm memory; `measureUserAgentSpecificMemory` needs cross-origin isolation |
| Safari | not tried here |

## Next step

Done: `BrowserVm` is the page's environment; `file` and `terminal` run in
it (above). Next: the one `look(port, path)` call for a port's page (a
server an engine starts in the VM, read as evidence); per-file
persistence (a 9p root backed by IndexedDB/OPFS) so a save is not the
whole 36+ MB machine; session logs kept in the VM so output survives a
reload; a `run` timeout; then `Local` (docker) in `dry_tools` behind the
same trait, and `Remote`. Not checked yet: Safari, and two tabs on one
origin (each boots its own VM and both save to the same IndexedDB key; the
last save wins).
