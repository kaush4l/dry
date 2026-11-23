//! `BrowserVm`: a container in the page (docs/container.md). v86 boots
//! Alpine (`vm/vm.js` moves the bytes); this side types into its serial
//! console and reads what comes back with `dry_core::container`'s exec
//! markers, asks a port in the VM for a page over v86's fake TCP, and
//! draws the `#/container` route: a terminal, the page a port serves, and
//! the numbers measured.
//!
//! One VM per page. Its state lives here (`VM`), not in the app's state:
//! a VM is this tab's, like a terminal session. It is also the page's
//! environment (`crate::env`): booted, or restored from IndexedDB, in the
//! background when the app starts (`ensure`).
//!
//! **One command at the console at a time.** Every exec, start and save
//! takes a ticket when it is asked for (`ticket`, in call order) and waits
//! its turn (`held`): the tty reads at most 4095 bytes a line and v86's
//! serial queue holds 4096, so nothing is typed ahead of a running command,
//! and a file written before a `start` is there when it starts.

use std::cell::RefCell;
use std::fmt::Write as _;

use dry_core::container::{self, Container, Http, Mount, Output};
use dry_core::environment;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Element, HtmlInputElement, HtmlTextAreaElement};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = dryVm, js_name = boot, catch)]
    fn js_boot(opts: &JsValue, on_bytes: &js_sys::Function) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = send, catch)]
    fn js_send(text: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = tcp, catch)]
    fn js_tcp(port: u16, bytes: &js_sys::Uint8Array, ms: u32) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = save, catch)]
    fn js_save() -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = forget, catch)]
    fn js_forget() -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = hasSaved, catch)]
    fn js_has_saved() -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = stats, catch)]
    fn js_stats() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = stop, catch)]
    fn js_stop() -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = dryVm, js_name = meta, catch)]
    fn js_meta() -> Result<js_sys::Promise, JsValue>;
}

/// The route.
pub const PATH: &str = "/container";
/// The transcript keeps this much text.
const TERM_MAX: usize = 120_000;
const MEMORY_MB: u32 = 256;

/// A background job's callback.
type Done = Box<dyn FnOnce(Output)>;

#[derive(Default)]
struct Vm {
    /// Booting, running, stopped.
    phase: Phase,
    /// What the console printed since the oldest exec still waiting.
    buf: String,
    /// Bytes that are not a whole UTF-8 character yet.
    partial: Vec<u8>,
    /// What the terminal view shows.
    term: String,
    next: u64,
    /// Background jobs: id, command, and their end once it came.
    jobs: Vec<(u64, String, Option<Output>)>,
    callbacks: Vec<(u64, Done)>,
    /// The last page a port served.
    page: Option<(u16, String, Result<Http, String>)>,
    /// Numbers for docs/container.md: (what, value).
    measured: Vec<(String, String)>,
    /// A saved machine is in IndexedDB.
    saved: Option<bool>,
    /// Execs waiting on `buf`.
    waiting: usize,
    dirty: bool,
    note: String,
    /// The console's queue: the last ticket given, the tickets served.
    tickets: u64,
    serving: u64,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Phase {
    #[default]
    Off,
    Booting,
    Running,
}

thread_local! {
    static VM: RefCell<Vm> = RefCell::new(Vm::default());
}

fn with<R>(f: impl FnOnce(&mut Vm) -> R) -> R {
    VM.with(|v| f(&mut v.borrow_mut()))
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |ok, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&ok, ms);
        }
    });
    let _ = JsFuture::from(p).await;
}

fn measure(what: &str, value: String) {
    with(|v| {
        v.measured.retain(|(w, _)| w != what);
        v.measured.push((what.to_string(), value));
    });
    draw();
}

fn note(text: &str) {
    with(|v| v.note = text.to_string());
    draw();
}

/// Bytes from the serial console.
fn on_bytes(bytes: js_sys::Uint8Array) {
    with(|v| {
        v.partial.extend(bytes.to_vec());
        let text = match std::str::from_utf8(&v.partial) {
            Ok(s) => s.to_string(),
            Err(e) if e.error_len().is_none() && v.partial.len() < 4 => return,
            Err(_) => String::from_utf8_lossy(&v.partial).to_string(),
        };
        v.partial.clear();
        v.buf.push_str(&text);
        v.term.push_str(&text.replace("\r\n", "\n"));
        if v.term.len() > TERM_MAX {
            let cut = v.term.len() - TERM_MAX;
            let cut = (cut..v.term.len()).find(|i| v.term.is_char_boundary(*i)).unwrap_or(0);
            v.term.drain(..cut);
        }
        v.dirty = true;
    });
}

/// Every 100 ms: the terminal view, and the background jobs that ended.
fn tick() {
    let (dirty, ended) = with(|v| {
        let buf = std::mem::take(&mut v.buf);
        ended_jobs(v, &buf);
        v.buf = buf;
        // Every job whose end is known and whose callback has not run.
        let mut calls = Vec::new();
        for (id, _, end) in &v.jobs {
            if let (Some(out), Some(i)) = (end, v.callbacks.iter().position(|(c, _)| c == id)) {
                calls.push((v.callbacks.remove(i).1, out.clone()));
            }
        }
        (std::mem::take(&mut v.dirty), calls)
    });
    for (f, out) in ended {
        f(out);
    }
    if dirty {
        draw_term();
    }
}

/// Mark the background jobs whose marker is in `text`.
fn ended_jobs(v: &mut Vm, text: &str) {
    for (id, _, end) in v.jobs.iter_mut().filter(|j| j.2.is_none()) {
        if let Some((out, _)) = container::take(text, *id) {
            *end = Some(out);
        }
    }
}

/// A place in the console's queue, taken when the work is asked for.
pub fn ticket() -> u64 {
    with(|v| {
        v.tickets += 1;
        v.tickets
    })
}

/// The console is this ticket's until dropped.
pub struct Held;

impl Drop for Held {
    fn drop(&mut self) {
        with(|v| v.serving += 1);
    }
}

/// Wait for ticket `t`'s turn at the console.
pub async fn held(t: u64) -> Held {
    while with(|v| v.serving + 1 != t) {
        sleep(2).await;
    }
    Held
}

/// Run `cmd` to its end, in its turn: long ones through a script
/// (`dry_core::environment::vm::steps`). What its last step printed.
pub async fn exec(cmd: &str) -> Result<Output, String> {
    let t = ticket();
    let id = with(|v| v.next + 1_000_000);
    exec_steps(t, environment::vm::steps(id, cmd)).await
}

/// Exec `steps` one after another in ticket `t`'s turn; the first that
/// fails ends them (its output is the error).
pub async fn exec_steps(t: u64, steps: Vec<String>) -> Result<Output, String> {
    let _h = held(t).await;
    let mut last = Output { code: 0, text: String::new() };
    let n = steps.len();
    for (i, step) in steps.iter().enumerate() {
        last = exec_now(step).await?;
        if last.code != 0 && i + 1 < n {
            return Err(format!("exit {}: {}", last.code, last.text.trim()));
        }
    }
    Ok(last)
}

/// The VM is on.
pub fn running() -> bool {
    with(|v| v.phase == Phase::Running)
}

/// A saved machine's size and when it was saved, from IndexedDB.
pub async fn saved_meta() -> Option<(u64, i64)> {
    let m = JsFuture::from(js_meta().ok()?).await.ok().filter(|m| !m.is_null() && !m.is_undefined())?;
    let get = |k: &str| js_sys::Reflect::get(&m, &k.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    Some((get("stored_bytes") as u64, get("saved") as i64))
}

/// Forget the saved machine (IndexedDB `dry-vm`).
pub async fn forget() -> Result<(), String> {
    JsFuture::from(js_forget().map_err(js_err)?).await.map_err(js_err)?;
    with(|v| v.saved = Some(false));
    Ok(())
}

/// Wait until vm.js has loaded (a module script may come after the wasm).
pub async fn js_loaded() -> bool {
    for _ in 0..200 {
        if js_sys::Reflect::get(&js_sys::global(), &"dryVm".into()).is_ok_and(|v| !v.is_undefined()) {
            return true;
        }
        sleep(50).await;
    }
    false
}

/// The VM ready for the tools: on already, or restored from the saved
/// machine, or booted fresh (a restore that fails boots fresh). Whether
/// it was booted fresh, and the Alpine version.
pub async fn ensure(say: &dyn Fn(&str)) -> Result<(bool, String), String> {
    if running() {
        return Ok((false, String::new()));
    }
    if !js_loaded().await {
        return Err("vm/vm.js did not load".into());
    }
    let saved = JsFuture::from(js_has_saved().map_err(js_err)?).await.ok().and_then(|v| v.as_bool()).unwrap_or(false);
    with(|v| v.saved = Some(saved));
    if saved {
        say("restoring the saved machine from IndexedDB");
        match BrowserVm::boot(true).await {
            Ok(r) => return Ok((false, r)),
            Err(e) => {
                say(&format!("the saved machine did not restore ({e}): booting fresh"));
                let _ = BrowserVm.stop().await;
            }
        }
    }
    say("downloading the kernel and booting Alpine (first load)");
    BrowserVm::boot(false).await.map(|r| (true, r))
}

/// The host the guest uses to reach this page's origin through v86's
/// fetch network: the guest's HTTP goes to port 80, and the adapter turns
/// `<port>.external` into `localhost:<port>`.
fn guest_origin() -> String {
    let loc = web_sys::window().map(|w| w.location());
    let host = loc.as_ref().and_then(|l| l.hostname().ok()).unwrap_or_default();
    let port = loc.as_ref().and_then(|l| l.port().ok()).unwrap_or_default();
    let path = loc.as_ref().and_then(|l| l.pathname().ok()).unwrap_or_default();
    let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
    let host = if !port.is_empty() && (host == "localhost" || host == "127.0.0.1") { format!("{port}.external") } else { host };
    format!("http://{host}{dir}")
}

/// The VM as a `Container` (dry_core::container).
pub struct BrowserVm;

impl BrowserVm {
    async fn boot(restore: bool) -> Result<String, String> {
        if with(|v| v.phase != Phase::Off) {
            return Err("the VM is already on".into());
        }
        with(|v| {
            v.phase = Phase::Booting;
            v.term.clear();
            v.buf.clear();
        });
        note(if restore { "restoring the saved machine…" } else { "booting Alpine…" });
        let opts = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&opts, &"base".into(), &"vm/".into());
        let _ = js_sys::Reflect::set(&opts, &"memoryMb".into(), &MEMORY_MB.into());
        let _ = js_sys::Reflect::set(&opts, &"restore".into(), &restore.into());
        let cb = Closure::<dyn FnMut(js_sys::Uint8Array)>::new(on_bytes);
        let t0 = now_ms();
        let p = js_boot(&opts, cb.as_ref().unchecked_ref()).map_err(js_err)?;
        cb.forget();
        let r = JsFuture::from(p).await.map_err(|e| {
            with(|v| v.phase = Phase::Off);
            js_err(e)
        })?;
        let get = |k: &str| js_sys::Reflect::get(&r, &k.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        measure("download (kernel, initramfs, BIOS, v86)", format!("{:.0} ms", get("download_ms")));
        let restored = js_sys::Reflect::get(&r, &"restored".into()).ok().and_then(|v| v.as_bool()).unwrap_or(false);
        if restore && !restored {
            let _ = BrowserVm.stop().await;
            return Err("no saved machine in IndexedDB".into());
        }
        if restored {
            measure("restore saved machine", format!("{:.0} ms ({:.1} MB gzip from IndexedDB)", get("restore_ms"), get("saved_bytes") / 1e6));
        } else {
            let mut waited = 0;
            while !with(|v| v.term.contains("dry: ready")) {
                sleep(50).await;
                waited += 50;
                if waited > 120_000 {
                    return Err("no `dry: ready` from the VM in 120 s".into());
                }
            }
            measure("boot to shell (fresh)", format!("{:.0} ms", now_ms() - t0));
            with(|v| v.buf.clear());
        }
        with(|v| v.phase = Phase::Running);
        // apk from this page's own origin (vm/alpine/, a copy of the official packages).
        let repos = format!("{0}vm/alpine/v3.24/main\n{0}vm/alpine/v3.24/community\n", guest_origin());
        let out = BrowserVm
            .exec(&format!(
                "printf %s {} > /etc/apk/repositories; mkdir -p {}; cat /etc/alpine-release",
                container::sh_quote(&repos),
                environment::vm::WORK
            ))
            .await?;
        note(&format!("running: Alpine {} · apk from {}", out.text.trim(), guest_origin()));
        Ok(out.text.trim().to_string())
    }
}

impl Container for BrowserVm {
    async fn create(&mut self, image: &str, mounts: &[Mount]) -> Result<(), String> {
        if image != "alpine" {
            return Err(format!("the browser VM has one image, alpine (asked: {image})"));
        }
        if !mounts.is_empty() {
            return Err("mounts are not wired yet: v86's 9p is mounted at /mnt/host but nothing is put in it".into());
        }
        BrowserVm::boot(false).await.map(|_| ())
    }

    async fn exec(&mut self, cmd: &str) -> Result<Output, String> {
        exec(cmd).await
    }

    fn start(&mut self, cmd: &str, done: Box<dyn FnOnce(Output)>) -> Result<u64, String> {
        if with(|v| v.phase == Phase::Off) {
            return Err("the VM is off".into());
        }
        let t = ticket();
        let id = with(|v| {
            v.next += 1;
            v.jobs.push((v.next, cmd.to_string(), None));
            v.callbacks.push((v.next, done));
            v.next
        });
        let cmd = cmd.to_string();
        spawn_local(async move {
            let _h = held(t).await;
            // A long command is written to a script first; the job runs it.
            let mut steps = environment::vm::steps(id, &cmd);
            let run = steps.pop().unwrap_or_default();
            for s in &steps {
                if let Err(e) = exec_now(s).await {
                    with(|v| {
                        if let Some(j) = v.jobs.iter_mut().find(|j| j.0 == id) {
                            j.2 = Some(Output { code: -1, text: e.clone() });
                        }
                    });
                    return;
                }
            }
            with(|v| v.term.push_str(&format!("\n$ {cmd} &   (job {id}, log /tmp/dry-job-{id}.log)\n")));
            if let Err(e) = js_send(&container::start_line(id, &run)) {
                note(&js_err(e));
            }
        });
        Ok(id)
    }

    async fn stop(&mut self) -> Result<(), String> {
        JsFuture::from(js_stop().map_err(js_err)?).await.map_err(js_err)?;
        with(|v| {
            v.phase = Phase::Off;
            v.jobs.clear();
            v.callbacks.clear();
        });
        Ok(())
    }

    async fn get(&mut self, port: u16, path: &str) -> Result<Http, String> {
        // A copy: v86 writes it after the connection opens, when a view
        // into wasm memory could already hold something else.
        let req = js_sys::Uint8Array::from(container::http_get(path).as_bytes());
        let p = js_tcp(port, &req, 10_000).map_err(js_err)?;
        let raw = JsFuture::from(p).await.map_err(js_err)?;
        container::http_parse(&js_sys::Uint8Array::new(&raw).to_vec())
    }

    /// The whole machine to IndexedDB, in its turn at the console.
    async fn snapshot(&mut self) -> Result<String, String> {
        let _h = held(ticket()).await;
        let r = JsFuture::from(js_save().map_err(js_err)?).await.map_err(js_err)?;
        let get = |k: &str| js_sys::Reflect::get(&r, &k.into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
        with(|v| v.saved = Some(true));
        Ok(format!("{:.0} MB state, {:.1} MB gzip in IndexedDB, {:.0} ms", get("bytes") / 1e6, get("stored_bytes") / 1e6, get("ms")))
    }
}

/// Type one exec line and wait for its marker; the caller holds the console.
async fn exec_now(cmd: &str) -> Result<Output, String> {
    if with(|v| v.phase == Phase::Off) {
        return Err("the VM is off".into());
    }
    let id = with(|v| {
        v.next += 1;
        v.next
    });
    let start = with(|v| {
        v.term.push_str(&format!("\n$ {cmd}\n"));
        v.dirty = true;
        v.waiting += 1;
        v.buf.len()
    });
    js_send(&container::exec_line(id, cmd)).map_err(js_err)?;
    loop {
        if let Some(out) = with(|v| {
            let (out, after) = container::take(v.buf.get(start..).unwrap_or(""), id)?;
            v.waiting -= 1;
            // This exec's output leaves the buffer (a job's marker that came
            // inside it is marked first), so the buffer stays small.
            let mine = v.buf.get(start..).unwrap_or("").to_string();
            ended_jobs(v, &mine);
            v.buf.truncate(start);
            v.buf.push_str(&after);
            // Nothing waits on the buffer once this exec is done, unless a
            // background job's marker is still to come.
            if v.waiting == 0 && v.jobs.iter().all(|j| j.2.is_some()) {
                v.buf.clear();
            }
            Some(out)
        }) {
            return Ok(out);
        }
        sleep(5).await;
    }
}

/// Exec and time it, into the terminal; failures become notes.
async fn timed(what: &str, cmd: &str) -> Option<Output> {
    let t = now_ms();
    match BrowserVm.exec(cmd).await {
        Ok(out) => {
            measure(what, format!("{:.0} ms, exit {}", now_ms() - t, out.code));
            Some(out)
        }
        Err(e) => {
            note(&e);
            None
        }
    }
}

/// The checks behind docs/container.md's numbers, in order.
async fn checks() {
    let mut rt = Vec::new();
    for _ in 0..10 {
        let t = now_ms();
        if BrowserVm.exec("true").await.is_err() {
            return note("the VM is off: boot it first");
        }
        rt.push(now_ms() - t);
    }
    rt.sort_by(|a, b| a.total_cmp(b));
    measure("exec round-trip (`true`, 10 runs)", format!("median {:.0} ms, min {:.0}, max {:.0}", rt[5], rt[0], rt[9]));
    timed("busybox loop, 10 000 iterations", "i=0; while [ $i -lt 10000 ]; do i=$((i+1)); done; echo $i").await;
    timed("apk add python3 tcc musl-dev (from this origin)", "apk add --no-progress python3 tcc tcc-libs-static tcc-dev musl-dev 2>&1 | tail -3").await;
    timed(
        "tcc hello.c (compile + run)",
        "cd /tmp && printf '#include <stdio.h>\\nint main(void){puts(\"hello from tcc\");return 0;}\\n' > hello.c && tcc -o hello hello.c && ./hello",
    )
    .await;
    timed("python3 sum(range(10**6))", "python3 -c 'print(sum(range(10**6)))'").await;
    if let Some(out) = timed("memory in the VM (free -m)", "free -m | sed -n 2p").await {
        measure("guest memory total / used (MB)", out.text.split_whitespace().skip(1).take(2).collect::<Vec<_>>().join(" / "));
    }
    // A server in the VM, then its page as evidence.
    let site = "mkdir -p /srv && cd /srv && printf '<!doctype html><title>dry in alpine</title><h1>Served from Alpine in this page</h1><p>python3 http.server on port 8000</p>\\n' > index.html";
    let _ = BrowserVm.exec(site).await;
    let _ = BrowserVm.start("cd /srv && exec python3 -m http.server 8000", Box::new(|out| note(&format!("http.server ended, exit {}", out.code))));
    let t = now_ms();
    let mut got = Err("not tried".to_string());
    for _ in 0..60 {
        got = BrowserVm.get(8000, "/").await;
        if got.is_ok() {
            break;
        }
        sleep(250).await;
    }
    measure("port 8000 first page (server start + GET)", format!("{:.0} ms", now_ms() - t));
    let t = now_ms();
    if BrowserVm.get(8000, "/").await.is_ok() {
        measure("port 8000 GET (warm)", format!("{:.0} ms", now_ms() - t));
    }
    with(|v| v.page = Some((8000, "/".into(), got)));
    stats();
    note("checks done");
}

fn stats() {
    if let Ok(s) = js_stats() {
        let get = |k: &str| js_sys::Reflect::get(&s, &k.into()).ok();
        if let Some(h) = get("js_heap_mb").and_then(|v| v.as_f64()) {
            measure("page JS heap (performance.memory)", format!("{h:.0} MB"));
        }
        let coi = get("cross_origin_isolated").and_then(|v| v.as_bool()).unwrap_or(false);
        measure("cross-origin isolated", if coi { "yes".into() } else { "no (v86 does not need it)".into() });
        if let Some(l) = get("loaded") {
            let mut parts = Vec::new();
            let mut total = 0.0;
            for k in js_sys::Object::keys(l.unchecked_ref::<js_sys::Object>()).iter() {
                let name = k.as_string().unwrap_or_default();
                let b = js_sys::Reflect::get(&l, &k)
                    .ok()
                    .and_then(|o| js_sys::Reflect::get(&o, &"bytes".into()).ok())
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                total += b;
                parts.push(format!("{name} {:.1} MB", b / 1e6));
            }
            measure("downloaded images", format!("{:.1} MB: {}", total / 1e6, parts.join(", ")));
        }
    }
}

// ---- the route ----

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn pane() -> String {
    with(|v| {
        let mut o = String::new();
        let on = v.phase != Phase::Off;
        let dis = |b: bool| if b { "" } else { " disabled" };
        let _ = write!(
            o,
            "<section class=\"card glass vm\"><h2>Container</h2>\
             <p class=\"note\">Alpine Linux in this page: v86 (x86 in WebAssembly), {MEMORY_MB} MB. Commands go to its serial console; a port's page comes back over v86's network. docs/container.md.</p>\
             <div class=\"row\">\
             <button class=\"btn primary\" type=\"button\" data-act=\"vm-boot\"{}>Boot</button>\
             <button class=\"btn\" type=\"button\" data-act=\"vm-restore\"{}>Restore saved</button>\
             <button class=\"btn\" type=\"button\" data-act=\"vm-check\"{}>Run checks</button>\
             <button class=\"btn\" type=\"button\" data-act=\"vm-save\"{}>Save machine</button>\
             <button class=\"btn\" type=\"button\" data-act=\"vm-forget\"{}>Forget saved</button>\
             <button class=\"btn\" type=\"button\" data-act=\"vm-stop\"{}>Stop</button></div>\
             <p class=\"note\" id=\"vm-note\">{}</p>",
            dis(!on),
            dis(!on && v.saved == Some(true)),
            dis(v.phase == Phase::Running),
            dis(v.phase == Phase::Running),
            dis(v.saved == Some(true)),
            dis(on),
            esc(if v.note.is_empty() { "off" } else { &v.note }),
        );
        let _ = write!(
            o,
            "<pre id=\"vm-term\" class=\"vm-term\"></pre>\
             <form class=\"row\" data-vm-form><textarea name=\"vm-cmd\" data-keep=\"vm-cmd\" rows=\"1\" placeholder=\"a command, e.g. uname -a\"></textarea>\
             <button class=\"btn\" type=\"button\" data-act=\"vm-exec\"{}>Exec</button></form></section>",
            dis(v.phase == Phase::Running)
        );
        let _ = write!(
            o,
            "<section class=\"card glass vm\"><h2>A port's page</h2><div class=\"row\">\
             <input name=\"vm-port\" value=\"8000\" size=\"6\" aria-label=\"port\"><input name=\"vm-path\" value=\"/\" aria-label=\"path\">\
             <button class=\"btn\" type=\"button\" data-act=\"vm-get\"{}>Open</button></div>",
            dis(v.phase == Phase::Running)
        );
        match &v.page {
            Some((port, path, Ok(h))) => {
                let _ = write!(
                    o,
                    "<p class=\"note\">GET {port}{} → {} · {}</p><iframe class=\"vm-page\" sandbox=\"\" title=\"the page port {port} serves\" srcdoc=\"{}\"></iframe>\
                     <p class=\"note\">Its text, as an engine reads it:</p><pre class=\"vm-text\" id=\"vm-text\">{}</pre>",
                    esc(path),
                    h.status,
                    esc(h.header("content-type").unwrap_or("")),
                    esc(&h.body),
                    esc(&container::html_text(&h.body))
                );
            }
            Some((port, path, Err(e))) => {
                let _ = write!(o, "<p class=\"note\">GET {port}{}: {}</p>", esc(path), esc(e));
            }
            None => o.push_str("<p class=\"note\">Nothing opened yet. Run checks starts python3 -m http.server 8000.</p>"),
        }
        o.push_str("</section><section class=\"card glass vm\"><h2>Measured</h2><table class=\"vm-table\" id=\"vm-measured\">");
        for (w, val) in &v.measured {
            let _ = write!(o, "<tr><td>{}</td><td>{}</td></tr>", esc(w), esc(val));
        }
        o.push_str("</table></section>");
        o
    })
}

fn doc() -> Option<web_sys::Document> {
    web_sys::window().and_then(|w| w.document())
}

fn draw_term() {
    let Some(d) = doc() else { return };
    if let Some(t) = d.get_element_by_id("vm-term") {
        // Without the exec markers and carriage returns.
        let shown = with(|v| v.term.replace('\r', "").split('\n').filter(|l| !l.contains("__dry_exit_")).collect::<Vec<_>>().join("\n"));
        t.set_text_content(Some(&shown));
        t.set_scroll_top(t.scroll_height());
    }
}

/// Draw the route into the pane the renderer drew ("Not found" for a path
/// it does not know): its title and its content.
pub fn draw() {
    let Some(d) = doc() else { return };
    let here = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
    if !here.starts_with(&format!("#{PATH}")) {
        return;
    }
    let kept = d
        .query_selector("[data-keep=\"vm-cmd\"]")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlTextAreaElement>().ok())
        .map(|t| t.value());
    if let Ok(Some(h)) = d.query_selector("#header .here") {
        h.set_text_content(Some("Container"));
    }
    if let Ok(Some(c)) = d.query_selector("#main .content") {
        c.set_inner_html(&pane());
    }
    if let (Some(k), Ok(Some(t))) = (kept, d.query_selector("[data-keep=\"vm-cmd\"]"))
        && let Ok(t) = t.dyn_into::<HtmlTextAreaElement>()
    {
        t.set_value(&k);
    }
    d.set_title("Container · dry");
    draw_term();
}

/// The route's link in the navigation, after the renderer's own.
pub fn nav_link() {
    let Some(d) = doc() else { return };
    let Some(nav) = d.get_element_by_id("nav") else { return };
    if d.get_element_by_id("nav-container").is_some() {
        return;
    }
    let on = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default().starts_with(&format!("#{PATH}"));
    if let Ok(a) = d.create_element("a") {
        a.set_id("nav-container");
        a.set_class_name(if on { "nav-item on" } else { "nav-item" });
        let _ = a.set_attribute("href", &format!("#{PATH}"));
        a.set_inner_html("<span class=\"nav-icon\">▣</span><span class=\"nav-text trunc\">Container</span>");
        let first_label = nav.query_selector(".nav-label").ok().flatten();
        let _ = nav.insert_before(&a, first_label.as_ref().map(|e| e.unchecked_ref::<web_sys::Node>()));
    }
}

/// Set up once: the terminal tick, and whether a machine is saved.
pub fn init() {
    // Ids differ per page load: a restored machine's old jobs may still
    // print their markers, and must not be taken for this page's.
    with(|v| v.next = (js_sys::Date::now() as u64 % 1_000_000) * 1_000_000);
    let tick = Closure::<dyn FnMut()>::new(tick);
    if let Some(w) = web_sys::window() {
        let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), 100);
    }
    tick.forget();
    spawn_local(async {
        // vm.js is a module script: it may come after the wasm starts.
        for _ in 0..100 {
            if js_sys::Reflect::get(&js_sys::global(), &"dryVm".into()).is_ok_and(|v| !v.is_undefined()) {
                break;
            }
            sleep(50).await;
        }
        if let Ok(p) = js_has_saved() {
            let saved = JsFuture::from(p).await.ok().and_then(|v| v.as_bool());
            with(|v| v.saved = saved);
            draw();
        }
    });
}

fn field(name: &str) -> String {
    doc()
        .and_then(|d| d.query_selector(&format!("[name=\"{name}\"]")).ok().flatten())
        .and_then(|e| {
            e.clone().dyn_into::<HtmlInputElement>().map(|i| i.value()).ok().or_else(|| e.dyn_into::<HtmlTextAreaElement>().ok().map(|t| t.value()))
        })
        .unwrap_or_default()
}

/// A `vm-…` control was pressed.
pub fn act(what: &str, _b: &Element) {
    let what = what.to_string();
    spawn_local(async move {
        let r: Result<(), String> = async {
            match what.as_str() {
                "vm-boot" => BrowserVm.create("alpine", &[]).await,
                "vm-restore" => BrowserVm::boot(true).await.map(|_| ()),
                "vm-check" => {
                    checks().await;
                    Ok(())
                }
                "vm-exec" => {
                    let cmd = field("vm-cmd");
                    if cmd.trim().is_empty() {
                        return Ok(());
                    }
                    let t = now_ms();
                    let out = BrowserVm.exec(cmd.trim()).await?;
                    note(&format!("exit {} in {:.0} ms", out.code, now_ms() - t));
                    Ok(())
                }
                "vm-get" => {
                    let port: u16 = field("vm-port").trim().parse().map_err(|_| "a port is a number".to_string())?;
                    let path = field("vm-path");
                    let got = BrowserVm.get(port, &path).await;
                    with(|v| v.page = Some((port, path, got)));
                    Ok(())
                }
                "vm-save" => {
                    note("saving the machine…");
                    let s = BrowserVm.snapshot().await?;
                    measure("save machine (save_state + gzip + IndexedDB)", s.clone());
                    note(&format!("saved: {s}"));
                    Ok(())
                }
                "vm-forget" => {
                    forget().await?;
                    crate::env::forgotten();
                    note("the saved machine is gone");
                    Ok(())
                }
                "vm-stop" => {
                    BrowserVm.stop().await?;
                    note("stopped");
                    Ok(())
                }
                other => Err(format!("no action {other}")),
            }
        }
        .await;
        if let Err(e) = r {
            note(&e);
        }
        draw();
    });
}
