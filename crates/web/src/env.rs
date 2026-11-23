//! The page's environment (`dry_core::environment`, docs/container.md
//! "The environment"): Alpine Linux in v86 (`crate::vm`) as the machine
//! the `file` and `terminal` tools run on, with the same tool names and
//! results as on a host. Only the model call leaves the page.
//!
//! - **Start.** `init` boots the VM in the background (the page does not
//!   wait): restored from IndexedDB when a machine is saved (~0.5 s), else
//!   booted fresh (~3 s after a 14.5 MB download) and saved at once, so the
//!   next load restores. The workspace is `/work`; a root `.` is `/work`.
//! - **Files.** `live::Host` is synchronous and the console is not, so the
//!   page keeps a mirror of `/work` (`Env::files`, at most 2000 files /
//!   16 MB, as a granted folder is kept). Reads and lists answer from it.
//!   A write lands in the mirror at once and goes to the VM in its turn at
//!   the console (base64 chunks, `environment::vm::write`), before any
//!   command asked for after it. After every `run`, and when a `start`ed
//!   job ends, the mirror is read again (a manifest, then the files whose
//!   size or time changed), so what a command wrote is what the next read
//!   sees. A path with a write still on its way keeps the mirror's copy.
//! - **Commands.** `run` waits without blocking the page (`run`, called by
//!   `companion::io`); `start` and `background` are VM jobs whose log tail
//!   is read on the 2 s tick, and whose end is the terminal's callback.
//! - **Runtimes.** Which of `environment::RUNTIMES` are on PATH is checked
//!   once after the boot and again after a command that installs (`apk
//!   add` / `apk del`), and kept in the environment: the head's prompt
//!   shows it (the `environment` element).
//! - **Persistence.** The root is in RAM: what the tools changed is kept by
//!   saving the whole machine again (`environment::Save`: 5 s after the
//!   last change, at most every 30 s, in its turn at the console). A
//!   reload within that window loses the last changes.

use std::cell::RefCell;
use std::collections::BTreeMap;

use dry_core::container::Container;
use dry_core::environment::{Cache, Environment, Kind, Phase, Save, vm as frame};
use dry_core::live::{Content, Host, Job, Proc};
use dry_core::terminal::{RESULT_LINES, RunJob, last_lines};
use wasm_bindgen_futures::spawn_local;

use crate::vm::BrowserVm;

/// A session's output after a reload (the page kept it, not the VM).
const LOST: &str = "(this session ran before the page reloaded: its output is gone)";
/// Lines of a job's log kept for its tail.
const KEEP_LINES: usize = 200;
/// The longest a `run` waits for the VM to come up.
const BOOT_WAIT_MS: f64 = 180_000.0;

struct File {
    size: u64,
    modified: i64,
    content: Content,
}

/// A terminal session in the VM, by its key (`workspace::key`).
#[derive(Default)]
struct Session {
    /// Its VM job, for `start` and `background`.
    job: Option<u64>,
    /// Its end: `None` while it runs.
    end: Option<Option<i32>>,
    tail: String,
    /// Its log read after it ended.
    last_read: bool,
    reading: bool,
}

#[derive(Default)]
struct Env {
    env: Option<Environment>,
    files: BTreeMap<String, File>,
    /// Writes on their way to the VM, by path.
    pending: BTreeMap<String, usize>,
    sessions: BTreeMap<String, Session>,
    save: Save,
    saving: bool,
    /// The saved machine was forgotten: not saved again until a reload,
    /// so the next load boots fresh.
    forgot: bool,
}

thread_local! {
    static ENV: RefCell<Env> = RefCell::new(Env::default());
}

fn with<R>(f: impl FnOnce(&mut Env) -> R) -> R {
    ENV.with(|e| f(&mut e.borrow_mut()))
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

/// The VM's environment, as this page has it now.
fn vm_env() -> Option<Environment> {
    with(|e| e.env.clone())
}

/// The environment the page reports: the paired companion's computer
/// while one is joined (`peer`), else the VM (`environment::probe`).
pub fn current(peer: bool) -> Option<Environment> {
    use dry_core::environment::{Probe, probe};
    match probe(Probe { page: true, companion: peer, ..Probe::default() }) {
        Some(Kind::Host) => {
            let url = crate::companion::conn().map(|(u, _)| u).unwrap_or_default();
            Some(Environment::host(&format!("the paired companion's computer ({url}); the browser VM waits"), "the companion's folder"))
        }
        _ => vm_env(),
    }
}

fn set(f: impl FnOnce(&mut Environment)) {
    with(|e| {
        if let Some(env) = e.env.as_mut() {
            f(env);
        }
    });
    crate::caps::publish();
}

fn phase(p: Phase) {
    set(|e| e.phase = p);
}

/// Boot (or restore) the VM in the background, and report each step.
pub fn init() {
    with(|e| {
        e.env = Some(Environment {
            kind: Kind::BrowserVm,
            phase: Phase::Booting("starting".into()),
            detail: "Alpine Linux x86 in v86, 256 MB, in this page".into(),
            workspace: frame::WORK.into(),
            cache: None,
            runtimes: None,
        })
    });
    crate::caps::publish();
    spawn_local(async {
        if !crate::vm::js_loaded().await {
            return phase(Phase::Failed(
                "vm/vm.js did not load: the VM's files are not beside the app (scripts/web-build.sh copies them)".into(),
            ));
        }
        read_cache().await;
        match crate::vm::ensure(&|what| phase(Phase::Booting(what.to_string()))).await {
            Ok((fresh, release)) => {
                set(|e| e.detail = format!("Alpine {release} x86 in v86, 256 MB, in this page"));
                phase(Phase::Booting("reading /work".into()));
                refresh().await;
                check_runtimes().await;
                phase(Phase::Ready);
                if fresh {
                    // Set up once: kept so the next load restores it.
                    save_now().await;
                }
            }
            Err(e) => phase(Phase::Failed(e)),
        }
    });
}

/// Which runtimes are in the VM (`environment::runtimes_check`): once
/// after the boot, and again after a command that installs (`installs`).
/// A check that did not run leaves what was known.
async fn check_runtimes() {
    if let Ok(o) = crate::vm::exec(&dry_core::environment::runtimes_check()).await
        && o.code == 0
    {
        let found = dry_core::environment::parse_runtimes(&o.text);
        set(|e| e.runtimes = Some(found));
    }
}

async fn read_cache() {
    let meta = crate::vm::saved_meta().await;
    set(|e| e.cache = meta.map(|(bytes, saved)| Cache { bytes, saved }));
}

/// The saved machine was forgotten (Settings, or `#/container`).
pub fn forgotten() {
    set(|e| e.cache = None);
    with(|e| e.forgot = true);
}

/// Settings → Environment → Forget cached VM (its fold is the confirm).
pub fn forget() {
    spawn_local(async {
        match crate::vm::forget().await {
            Ok(()) => {
                forgotten();
                crate::app::note("the cached VM is forgotten: the next load boots a fresh one");
            }
            Err(e) => crate::app::note(&format!("could not forget the cached VM: {e}")),
        }
    });
}

async fn save_now() {
    if with(|e| e.forgot || std::mem::replace(&mut e.saving, true)) {
        return;
    }
    with(|e| e.save.saving(now_ms()));
    let r = BrowserVm.snapshot().await;
    with(|e| e.saving = false);
    match r {
        Ok(_) => read_cache().await,
        Err(e) => crate::app::note(&format!("the VM was not saved: {e}")),
    }
}

fn changed() {
    with(|e| e.save.changed(now_ms()));
}

/// Why the tools cannot use the VM now.
fn not_ready() -> Option<String> {
    match vm_env() {
        None => Some("no environment in this page".into()),
        Some(e) if e.ready() && !crate::vm::running() => {
            Some("the browser VM was stopped (#/container): reload the page to boot it again".into())
        }
        Some(e) => e.not_ready(),
    }
}

/// The 2 s tick (the app's `settle`): the tails of running jobs, the
/// last tail of ended ones, and the save rule.
pub fn tick() {
    if not_ready().is_some() {
        return;
    }
    let read: Vec<(String, u64)> = with(|e| {
        e.sessions
            .iter_mut()
            .filter(|(_, s)| !s.reading && !s.last_read)
            .filter_map(|(k, s)| {
                let job = s.job?;
                s.reading = true;
                Some((k.clone(), job))
            })
            .collect()
    });
    for (key, job) in read {
        spawn_local(async move {
            let ended = with(|e| e.sessions.get(&key).is_some_and(|s| s.end.is_some()));
            let out = crate::vm::exec(&format!("tail -n {KEEP_LINES} /tmp/dry-job-{job}.log 2>/dev/null")).await;
            with(|e| {
                if let Some(s) = e.sessions.get_mut(&key) {
                    s.reading = false;
                    if let Ok(o) = out {
                        s.tail = o.text;
                    }
                    s.last_read |= ended;
                }
            });
        });
    }
    if with(|e| e.save.due(now_ms()) && !e.saving) {
        spawn_local(save_now());
    }
}

/// Read `/work` into the mirror again: the manifest, then the files whose
/// size or time changed (paths with a write on its way are left alone).
async fn refresh() {
    let Ok(out) = crate::vm::exec(&frame::manifest()).await else { return };
    if out.code != 0 {
        return;
    }
    let entries = frame::parse_manifest(&out.text);
    let fetch: Vec<String> = with(|e| {
        let mut budget = frame::MIRROR_BYTES;
        entries
            .iter()
            .take(frame::MIRROR_FILES)
            .filter(|x| x.size <= dry_core::workbench::MAX_OPEN as u64 && !dry_core::file::secret_file(&x.path))
            .filter(|x| e.files.get(&x.path).is_none_or(|f| f.size != x.size || f.modified != x.modified))
            .filter(|x| {
                let ok = x.size <= budget;
                budget = budget.saturating_sub(x.size);
                ok
            })
            .map(|x| x.path.clone())
            .collect()
    });
    let mut got = BTreeMap::new();
    for group in frame::fetch_groups(&fetch) {
        if let Ok(o) = crate::vm::exec(&frame::fetch(&group)).await {
            got.extend(frame::parse_fetch(&o.text));
        }
    }
    with(|e| {
        let Env { files, pending, .. } = e;
        let seen: std::collections::BTreeSet<&str> = entries.iter().map(|x| x.path.as_str()).collect();
        files.retain(|p, _| seen.contains(p.as_str()) || pending.contains_key(p));
        for x in entries.iter().take(frame::MIRROR_FILES) {
            if pending.contains_key(&x.path) || dry_core::file::secret_file(&x.path) {
                continue;
            }
            let content = match got.remove(&x.path) {
                Some(bytes) => frame::content(&x.path, &bytes),
                None if x.size > dry_core::workbench::MAX_OPEN as u64 => Content::TooLarge(x.size),
                None if files.contains_key(&x.path) => continue,
                None => Content::TooLarge(x.size),
            };
            files.insert(x.path.clone(), File { size: x.size, modified: x.modified, content });
        }
    });
}

/// Wait for the VM (it may still be booting); why not, if it will not come.
async fn wait_ready() -> Result<(), String> {
    let t = now_ms();
    loop {
        match vm_env().map(|e| e.phase) {
            Some(Phase::Booting(_)) if now_ms() - t < BOOT_WAIT_MS => {
                let p = js_sys::Promise::new(&mut |ok, _| {
                    if let Some(w) = web_sys::window() {
                        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&ok, 250);
                    }
                });
                let _ = wasm_bindgen_futures::JsFuture::from(p).await;
            }
            _ => return not_ready().map_or(Ok(()), Err),
        }
    }
}

/// A terminal `run` in the VM, waited for without blocking the page: its
/// exit code and last lines, as `RunJob::wait` gives them on a host.
pub async fn run(job: &RunJob) -> Result<(Option<i32>, String), String> {
    wait_ready().await?;
    let dir = frame::dir(&job.root)?;
    let key = dry_core::workspace::key(&job.job());
    with(|e| e.sessions.insert(key.clone(), Session::default()));
    let out = crate::vm::exec(&frame::run(&dir, &job.command)).await;
    changed();
    let came = out.map(|o| {
        with(|e| {
            if let Some(s) = e.sessions.get_mut(&key) {
                s.end = Some(Some(o.code));
                s.tail = last_lines(&o.text, KEEP_LINES);
                s.last_read = true;
            }
        });
        (Some(o.code), last_lines(&o.text, RESULT_LINES))
    });
    if came.is_err() {
        with(|e| e.sessions.entry(key.clone()).or_default().end = Some(None));
    }
    refresh().await;
    if dry_core::environment::installs(&job.command) {
        check_runtimes().await;
    }
    came
}

/// The VM as a `live::Host`: the mirror for files, VM jobs for commands.
/// A root `folder` is still the granted folder (`crate::caps`).
pub struct VmHost;

impl Host for VmHost {
    fn now(&self) -> i64 {
        crate::host::now()
    }

    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_list(max);
        }
        if let Some(why) = not_ready() {
            return Err(why);
        }
        let dir = frame::dir(root)?;
        let dir = dir.strip_prefix(frame::WORK).unwrap_or("").trim_start_matches('/').to_string();
        Ok(with(|e| frame::list(e.files.keys(), &dir, max)))
    }

    fn read(&self, root: &str, path: &str) -> Content {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_read(path);
        }
        if let Some(why) = not_ready() {
            return Content::Missing(why);
        }
        match frame::rel(root, path) {
            Ok(rel) => with(|e| match e.files.get(&rel) {
                Some(f) => f.content.clone(),
                None => Content::Missing(format!("{path}: no such file in {}", frame::WORK)),
            }),
            Err(why) => Content::Missing(why),
        }
    }

    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_write(path, text);
        }
        if let Some(why) = not_ready() {
            return Err(why);
        }
        let rel = frame::rel(root, path)?;
        let steps = frame::write(&rel, text);
        // Its place at the console now, so a command asked for after it sees it.
        let t = crate::vm::ticket();
        with(|e| {
            let modified = crate::host::now();
            e.files.insert(rel.clone(), File { size: text.len() as u64, modified, content: Content::Text(text.to_string()) });
            *e.pending.entry(rel.clone()).or_default() += 1;
        });
        changed();
        spawn_local(async move {
            let r = crate::vm::exec_steps(t, steps).await;
            with(|e| {
                if let Some(n) = e.pending.get_mut(&rel) {
                    *n -= 1;
                    if *n == 0 {
                        e.pending.remove(&rel);
                        // The VM's time for it, read with the next refresh.
                        if let Some(f) = e.files.get_mut(&rel) {
                            f.modified = 0;
                        }
                    }
                }
            });
            match r {
                Ok(o) if o.code == 0 => {}
                Ok(o) => crate::app::note(&format!("{rel} was not written in the VM: exit {} {}", o.code, o.text.trim())),
                Err(e) => crate::app::note(&format!("{rel} was not written in the VM: {e}")),
            }
        });
        Ok(())
    }

    /// Scripts' steps are synchronous; a VM command is not.
    fn run(&self, _: &Job) -> Result<Option<i32>, String> {
        Err("`run` waits for the browser VM: call it on its own, not inside a script".into())
    }

    fn spawn(&self, job: &Job) -> Result<(), String> {
        if let Some(why) = not_ready() {
            return Err(why);
        }
        let dir = frame::dir(job.root)?;
        let key = dry_core::workspace::key(job);
        let k = key.clone();
        let installs = dry_core::environment::installs(job.command);
        let id = BrowserVm.start(
            &frame::run(&dir, job.command),
            Box::new(move |out| {
                with(|e| {
                    if let Some(s) = e.sessions.get_mut(&k) {
                        s.end = Some(Some(out.code));
                    }
                });
                changed();
                spawn_local(async move {
                    refresh().await;
                    if installs {
                        check_runtimes().await;
                    }
                });
            }),
        )?;
        with(|e| e.sessions.insert(key, Session { job: Some(id), ..Session::default() }));
        changed();
        Ok(())
    }

    fn status(&self, job: &Job) -> Proc {
        with(|e| match e.sessions.get(&dry_core::workspace::key(job)) {
            Some(Session { end: None, .. }) => Proc::Running,
            Some(Session { end: Some(code), .. }) => Proc::Ended(*code),
            // Not this page's: it ran before a reload.
            None => Proc::Ended(None),
        })
    }

    fn tail(&self, job: &Job, lines: usize) -> String {
        with(|e| match e.sessions.get(&dry_core::workspace::key(job)) {
            Some(s) => last_lines(&s.tail, lines),
            None => LOST.to_string(),
        })
    }
}
