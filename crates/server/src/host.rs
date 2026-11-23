//! The host: the server as the one writer of the store. It holds the
//! state, runs every engine's turns (`dry_core::runner`, the same machine
//! the browser drives) on its own threads against the real disk
//! (`dry_tools::live::Disk`: file and terminal artifacts work) and the
//! native model client (`dry_tools::net::Native`), and takes intents from
//! clients over HTTP (docs/web.md, "The send path"). Clients never write
//! state; they send text, answers and small settings, and watch.
//!
//! - **One writer.** Every write goes through `commit`: the store is
//!   saved, the generation bumps (`Live::bump`), the engines whose texts
//!   changed are marked with it, and the lock file beside the store
//!   (`dry_tools::host`) carries it, so the window reloads.
//! - **Stale views.** A send says the generation its view is of (`base`).
//!   It is refused (409) when its engine changed after that, or when it
//!   names the engine by index and engines were added or removed after
//!   that (the index may be another engine now).
//! - **Turns.** One thread per turn. Each move borrows the state under
//!   the lock only while the runner decides; the model call and the
//!   waiting calls run outside it. Children a step gave a message start
//!   their own turns; callbacks (`dry_core::turn::callbacks`) settle
//!   after every turn and on a tick, as in the window. No step limit;
//!   the history compresses at `context::COMPRESS_AT` (75%), in the core.
//! - **Events.** Each move's activity, each message added and each turn's
//!   end go to `/events` (`live`), with ids a client resumes from.
//! - **MCP.** An engine's MCP servers (its spec's `mcp.json`) start on its
//!   first turn (`mcp_for`) and stay for the host's life, restarted when
//!   the file changes; their tools are the engine's `mcp_tools`, and a call
//!   no capability takes goes to them. The file and terminal tools run on
//!   this computer (the environment is `Host`: `dry_core::environment`).
//! - **Sessions** (docs/sessions.md). A session engine's turn runs its
//!   program here (`sessions.json`), one thread per turn, with its own
//!   settings file and environment (`dry_tools::worker::prepare`). A
//!   program that streams (`stream-json`) is read line by line
//!   (`dry_core::stream_json`): each tool call is a live row (a `tool`
//!   event), each sub-agent a read-only participant posting on the
//!   session's thread (started, done). At the end its rows are one tools
//!   message, its results the replies.
//! - **Workers' intents.** A session's MCP server (`dry mcp engines`)
//!   reads here (`read`) and writes only here: posts, child engines
//!   (`children`, the `engines` artifact's own calls), a child's reply
//!   taken once (`take`), and approvals (`ask_approval`, `decide`), which
//!   wait for a person unless the engine's rules allow at once.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use dry_core::activity::Activity;
use dry_core::approval::{Approval, Decided, Decision};
use dry_core::ask::{self, Answer, Ask, Input};
use dry_core::calls::Call;
use dry_core::card::Status;
use dry_core::context::Window;
use dry_core::engines::Seen;
use dry_core::inference::Role;
use dry_core::runner::{self, Came, Cx, Measured, Next, Runner};
use dry_core::state::{Engine, State};
use dry_tools::live::Disk;
use serde_json::{Value, json};

use crate::live::Live;

/// Why the host did not take an intent: the HTTP status and the words.
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    pub status: u16,
    pub error: String,
}

fn refuse(status: u16, error: impl Into<String>) -> Refused {
    Refused { status, error: error.into() }
}

/// The line every send to a busy engine gets (the window shows the same).
pub const BUSY: &str = "engine is in a turn";

/// A send or an answer that was taken: the turn it started and the
/// generation after its message was added.
#[derive(Clone, Debug, PartialEq)]
pub struct Taken {
    pub turn: String,
    pub generation: u64,
    pub index: usize,
    pub id: String,
}

/// An engine's turn while it runs.
#[derive(Clone, Debug)]
struct Turn {
    name: String,
    activity: Activity,
}

/// What the host holds.
#[derive(Default)]
pub struct Inner {
    pub state: State,
    /// What the store holds now (the state at the last save).
    saved: State,
    /// Why the store did not read at start: then nothing is written, so it
    /// is never overwritten with an empty state.
    pub load_error: Option<String>,
    /// The last save that failed, and why.
    pub save_error: Option<String>,
    /// Per engine id: the generation of the last write that changed it.
    changed: HashMap<String, u64>,
    /// The generation engines were last added, removed or reordered at.
    layout: u64,
    /// Per engine id: its running turn.
    turns: HashMap<String, Turn>,
    /// Per engine id: how its last turn ended.
    status: HashMap<String, Status>,
    /// Per engine id: the prompt against the window at the last call.
    measured: HashMap<String, Measured>,
    /// Per model (`base_url model`): its window, once asked.
    windows: HashMap<String, Window>,
    /// Per engine id: the activity last sent as an event.
    told: HashMap<String, String>,
    /// Per engine id: its answer as far as the model has written it
    /// (`dry_core::stream`), drawn under the conversation until saved.
    streaming: HashMap<String, dry_core::stream::View>,
    /// Per engine id: its session program's run as far as it has gone
    /// (`dry_core::stream_json`), drawn under the conversation until it ends.
    sessions: HashMap<String, dry_core::stream_json::Live>,
    /// Approvals asked of the host, oldest first (`dry_core::approval`);
    /// the newest `KEEP_APPROVALS` decided ones are kept to read back.
    approvals: Vec<Approval>,
    approvals_made: u64,
    /// Spans (`dry_core::spans`): the tasks open across turns, each
    /// engine's log as written (by id), the rolled-up days, and the folder
    /// the logs are appended in (`None`: in memory only, for tests).
    tasks: dry_core::spans::Tasks,
    spans: HashMap<String, Vec<dry_core::spans::Span>>,
    daily: Vec<dry_core::spans::Daily>,
    spans_dir: Option<PathBuf>,
    rolled_day: i64,
    /// The Code page's terminal (`dry_core::workbench::Terminal`): lines
    /// typed into it on any page, run here (`run_typed`), seen by all.
    terminal: dry_core::workbench::Terminal,
}

/// Decided approvals kept for reading back.
const KEEP_APPROVALS: usize = 50;

/// A turn to start: a model engine's runner, or a session's program.
enum Work {
    Model(Box<Runner>),
    Session(String),
}

/// What a page draws from the host, by index (`dry_html::world::Reported`).
pub struct Snapshot {
    pub state: State,
    pub reported: dry_html::world::Reported,
    pub store_error: Option<String>,
}

pub struct Host {
    inner: Mutex<Inner>,
    pub live: Arc<Live>,
    /// The store, as `Store::describe` names it (`md:/…/state`).
    pub spec: String,
    /// The lock file beside the store, and what it says.
    lock: Option<(PathBuf, dry_tools::host::Lock)>,
    disk: Disk,
    /// Its capabilities: this machine's, and the peers' that joined
    /// (`crate::capabilities`). Outside calls are routed through it.
    pub caps: Arc<crate::capabilities::Caps>,
    /// Each engine's running MCP servers, by engine id: the `mcp.json` they
    /// came from, the servers, their tools.
    mcp: Mutex<HashMap<String, Mcp>>,
}

/// One engine's MCP servers, as started from its `mcp.json`.
type Mcp = (String, Arc<dry_tools::mcp::Servers>, Vec<dry_core::mcp::Tool>);

fn window_key(ai: &dry_core::inference::OpenAi) -> String {
    format!("{} {}", ai.base_url, ai.model)
}

fn kind(a: &Activity) -> &'static str {
    match a {
        Activity::Idle => "idle",
        Activity::Llm { .. } => "llm",
        Activity::Tools { .. } => "tools",
        Activity::Compressing { .. } => "compress",
        Activity::Waiting(_) => "wait",
        Activity::Working(_) => "working",
        Activity::NeedsYou(_) => "needs-you",
        Activity::Failed(_) => "failed",
    }
}

fn status_word(s: &Status) -> &'static str {
    match s {
        Status::Idle => "idle",
        Status::Ready => "ready",
        Status::Working(_) => "working",
        Status::Done(_) => "done",
        Status::Missing(_) => "missing",
        Status::Failed(_) => "failed",
    }
}

fn status_line(s: &Status) -> String {
    match s {
        Status::Working(t) | Status::Done(t) | Status::Missing(t) | Status::Failed(t) => t.clone(),
        other => status_word(other).to_string(),
    }
}

impl Inner {
    fn busy(&self, id: &str) -> bool {
        self.turns.contains_key(id)
    }

    fn seen(&self, state: &State, j: usize) -> Seen {
        let Some(e) = state.engines.get(j) else { return Seen::default() };
        let a = dry_core::dashboard::live_activity(e, self.status.get(&e.id), self.turns.get(&e.id).map(|t| &t.activity));
        let failed = match self.status.get(&e.id) {
            Some(Status::Failed(why) | Status::Missing(why)) => Some(why.clone()),
            _ => None,
        };
        Seen { label: a.label(), busy: self.busy(&e.id) || !e.waiting.is_empty(), failed }
    }

    /// The engine `target` names: its index (`0`), else its id. `new` is
    /// the caller's to make. An index is checked against `base` (engines
    /// added or removed since: it may be another engine now).
    fn resolve(&self, target: &str, base: Option<u64>) -> Result<usize, Refused> {
        if let Ok(i) = target.parse::<usize>() {
            if base.is_some_and(|b| self.layout > b) {
                return Err(refuse(409, "stale: engines were added or removed since your view; reload and send again"));
            }
            return if i < self.state.engines.len() { Ok(i) } else { Err(refuse(404, format!("no engine {i}"))) };
        }
        self.state.index_of(target).ok_or_else(|| refuse(404, format!("no engine {target}")))
    }

    /// Refused when engine `i` changed after the view `base` is of.
    fn fresh(&self, i: usize, base: Option<u64>) -> Result<(), Refused> {
        let id = &self.state.engines[i].id;
        match (base, self.changed.get(id)) {
            (Some(b), Some(&at)) if at > b => Err(refuse(
                409,
                format!("stale: {} changed since your view (at {at}, you saw {b}); reload and send again", self.state.engines[i].name),
            )),
            _ => Ok(()),
        }
    }
}

/// The disk and the clock a turn reads through.
struct Io<'a> {
    disk: &'a Disk,
}

impl Host {
    /// The host for the app's store, or the reason it cannot be one
    /// (another host is alive for it). `url`: where it answers on this
    /// machine, for the lock.
    pub fn open(live: Arc<Live>, url: &str) -> Result<Arc<Host>, String> {
        let (state, spec, load_error) = crate::load::read_state();
        let lock_path = dry_tools::host::lock_path(&spec);
        if let Some(other) = dry_tools::host::read(&lock_path).filter(|l| l.pid != std::process::id() && l.alive()) {
            return Err(format!("{} already hosts {spec} (pid {})", other.url, other.pid));
        }
        let lock =
            dry_tools::host::Lock { pid: std::process::id(), url: url.to_string(), generation: live.generation(), store: spec.clone() };
        let disk = Disk::for_store(&spec);
        let caps = Arc::new(crate::capabilities::Caps::local());
        let mut state = state;
        state.equip(caps.registry());
        let mut inner = Inner { saved: state.clone(), state, load_error, ..Inner::default() };
        inner.spans_dir = Some(disk.engines.clone());
        Host::load_spans(&mut inner);
        let host = Host { inner: Mutex::new(inner), live, spec, lock: Some((lock_path, lock)), disk, caps, mcp: Mutex::default() };
        host.write_lock(host.live.generation());
        Ok(Arc::new(host))
    }

    /// A host on a state in memory and no store, lock or disk of its own
    /// (unit tests).
    #[cfg(test)]
    pub fn in_memory(state: State) -> Arc<Host> {
        let inner = Inner { saved: state.clone(), state, ..Inner::default() };
        Arc::new(Host {
            inner: Mutex::new(inner),
            live: Arc::new(Live::default()),
            spec: "memory".into(),
            lock: None,
            disk: Disk { repo: std::env::temp_dir(), engines: std::env::temp_dir() },
            caps: Arc::new(crate::capabilities::Caps::default()),
            mcp: Mutex::default(),
        })
    }

    /// The capabilities changed (a peer joined, left, re-registered, a
    /// snapshot came): every engine is equipped with the registry now, and
    /// pages redraw.
    pub fn capabilities_changed(&self) {
        let reg = self.caps.registry();
        {
            let mut g = self.lock();
            g.state.equip(reg.clone());
            g.saved.equip(reg);
        }
        self.live.bump(&["capabilities"]);
    }

    /// Deliver the events that fired: each engine using the capability
    /// gets one message and a turn (queued behind a running one, as a
    /// callback is: the message waits in its history).
    fn deliver_events(self: &Arc<Self>) {
        let fired = self.caps.take_fired();
        if fired.is_empty() {
            return;
        }
        let mut runners = Vec::new();
        {
            let mut guard = self.lock();
            let g = &mut *guard;
            for f in &fired {
                for i in dry_core::capability::users(&g.state, &f.capability) {
                    let busy = g.busy(&g.state.engines[i].id);
                    if g.state.add_wake(i, &f.message())
                        && !busy
                        && let Ok(r) = self.begin(g, i, false)
                    {
                        runners.push(r);
                    }
                }
            }
            self.commit(g);
        }
        self.spawn(runners);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write_lock(&self, generation: u64) {
        if let Some((path, lock)) = &self.lock {
            let lock = dry_tools::host::Lock { generation, ..lock.clone() };
            if let Err(e) = dry_tools::host::write(path, &lock) {
                eprintln!("host: could not write the lock: {e}");
            }
        }
    }

    /// The lock goes when the host does.
    pub fn release(&self) {
        if let Some((path, lock)) = &self.lock {
            dry_tools::host::release(path, lock.pid);
        }
        let _ = std::fs::remove_dir_all(terminal_dir());
    }

    // ---- spans (`dry_core::spans`, docs/analytics.md) ----

    /// Roll days past the retention into daily totals (once a day), then
    /// read every engine's log.
    fn load_spans(g: &mut Inner) {
        let Some(dir) = g.spans_dir.clone() else { return };
        let now = dry_tools::spans::now_ms();
        let offset = dry_tools::live::clock().offset;
        g.rolled_day = dry_core::spans::day_of(now, offset);
        let rolled = dry_tools::spans::roll(&dir, now, offset, dry_tools::spans::keep_days());
        if rolled > 0 {
            eprintln!("host: {rolled} spans past the retention rolled into daily totals");
        }
        let (all, daily) = dry_tools::spans::load_all(&dir);
        g.spans = all.into_iter().collect();
        g.daily = daily;
    }

    /// A turn's spans, just closed: placed under their tasks, appended to
    /// their engines' logs (the host is their one writer) and kept for pages.
    fn record(&self, g: &mut Inner, mut spans: Vec<dry_core::spans::Span>) {
        if spans.is_empty() {
            return;
        }
        for e in &g.state.engines {
            g.tasks.set_parent(&e.id, e.parent.as_deref());
        }
        g.tasks.adopt(&mut spans);
        if let Some(dir) = &g.spans_dir
            && let Err(e) = dry_tools::spans::append(dir, &spans)
        {
            eprintln!("host: spans not written: {e}");
        }
        for s in spans {
            g.spans.entry(s.engine.clone()).or_default().push(s);
        }
        let today = dry_core::spans::day_of(dry_tools::spans::now_ms(), dry_tools::live::clock().offset);
        if g.spans_dir.is_some() && today != g.rolled_day {
            Host::load_spans(g);
        }
    }

    /// What a page is drawn from now.
    pub fn snapshot(&self) -> Snapshot {
        let mut g = self.lock();
        g.state.equip_engines();
        let mut reported = dry_html::world::Reported::default();
        for (i, e) in g.state.engines.iter().enumerate() {
            if let Some(t) = g.turns.get(&e.id) {
                reported.activity.insert(i, t.activity.clone());
            }
            if let Some(s) = g.status.get(&e.id) {
                reported.status.insert(i, s.clone());
            }
            if let Some(m) = g.measured.get(&e.id) {
                reported.measured.insert(i, m.clone());
            }
            if let Some(v) = g.streaming.get(&e.id) {
                reported.streaming.insert(i, v.clone());
            }
            if let Some(v) = g.sessions.get(&e.id) {
                reported.sessions.insert(i, v.clone());
            }
        }
        reported.approvals = g.approvals.iter().filter(|a| a.decided.is_none()).cloned().collect();
        reported.spans = g.spans.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        reported.daily = g.daily.clone();
        reported.terminal = Some(g.terminal.clone());
        let store_error = g.load_error.clone().map(|e| format!("{e} (the host writes nothing)")).or_else(|| g.save_error.clone());
        Snapshot { state: g.state.clone(), reported, store_error }
    }

    /// The host's status for clients (`GET /api/status`).
    pub fn status_json(&self) -> Value {
        let g = self.lock();
        let engines: Vec<Value> = g
            .state
            .engines
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let a = dry_core::dashboard::live_activity(e, g.status.get(&e.id), g.turns.get(&e.id).map(|t| &t.activity));
                json!({ "index": i, "id": e.id, "name": e.name, "busy": g.busy(&e.id), "kind": kind(&a), "label": a.label(), "messages": e.messages.len() })
            })
            .collect();
        json!({ "host": true, "gen": self.live.generation(), "store": self.spec, "engines": engines })
    }

    /// Save what changed since the last save, bump the generation and say
    /// what changed. Nothing changed: the generation as it is.
    fn commit(&self, g: &mut Inner) -> u64 {
        if g.state == g.saved {
            return self.live.generation();
        }
        if let Some(e) = &g.load_error {
            g.save_error = Some(format!("not saved: the store did not read at start ({e})"));
            return self.live.generation();
        }
        if self.lock.is_some() {
            let mut print = self.live.store_print.lock().unwrap_or_else(|e| e.into_inner());
            match dry_tools::store::open(&self.spec).and_then(|s| s.save(&g.state)) {
                Ok(()) => g.save_error = None,
                Err(e) => g.save_error = Some(format!("save failed: {e}")),
            }
            *print = Some(crate::live::store_print());
        }
        let generation = self.live.bump(&["store"]);
        let old: BTreeMap<&str, &Engine> = g.saved.engines.iter().map(|e| (e.id.as_str(), e)).collect();
        let ids = |s: &State| s.engines.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        if ids(&g.state) != ids(&g.saved) {
            g.layout = generation;
        }
        let mut events = Vec::new();
        for (i, e) in g.state.engines.iter().enumerate() {
            let before = old.get(e.id.as_str());
            if before.is_some_and(|b| *b == e) {
                continue;
            }
            let from = before.map_or(0, |b| b.messages.len().min(e.messages.len()));
            for (n, m) in e.messages.iter().enumerate().skip(from) {
                let role = if m.role == Role::User { "user" } else { "assistant" };
                events.push(json!({ "engine": e.id, "index": i, "n": n, "role": role, "gen": generation }).to_string());
            }
            g.changed.insert(e.id.clone(), generation);
        }
        for e in events {
            self.live.push("message", e);
        }
        g.saved = g.state.clone();
        self.write_lock(generation);
        generation
    }

    /// Say engine `id`'s activity, if it is not what was said last.
    fn tell(&self, g: &mut Inner, id: &str) {
        let Some(i) = g.state.index_of(id) else { return };
        let e = &g.state.engines[i];
        let a = dry_core::dashboard::live_activity(e, g.status.get(id), g.turns.get(id).map(|t| &t.activity));
        let label = a.label();
        if g.told.get(id) == Some(&label) {
            return;
        }
        let data =
            json!({ "engine": id, "index": i, "kind": kind(&a), "label": label, "short": a.short(), "token": a.token() }).to_string();
        g.told.insert(id.to_string(), label);
        self.live.push("activity", data);
    }

    /// Engine `i`'s turn, marked as running: a runner for a model engine,
    /// its program for a session; why not if it cannot run (busy, no
    /// model, no such program).
    fn begin(&self, g: &mut Inner, i: usize, resume: bool) -> Result<Work, Status> {
        // The sessions busy now, for the subscriptions a prompt shows.
        self.sync_running(g);
        // Engines made since the registry last changed get it too.
        g.state.equip_engines();
        let e = &g.state.engines[i];
        if g.busy(&e.id) {
            return Err(Status::Working(BUSY.into()));
        }
        let w = g.windows.get(&window_key(&e.inference)).cloned();
        let id = e.id.clone();
        let name = format!("{id}/{}", e.messages.len().saturating_sub(1));
        let made = match &e.session {
            Some(s) => match dry_tools::read_sessions()
                .and_then(|all| all.get(&s.tool).cloned().ok_or_else(|| format!("no program \"{}\" in sessions.json", s.tool)))
            {
                Ok(_) => Ok((Work::Session(id.clone()), Activity::session(&s.tool, None))),
                Err(why) => Err(Status::Missing(why)),
            },
            None => {
                let r = if resume { Runner::resume(&g.state, i, w) } else { Runner::start(&g.state, i, w) };
                r.map(|r| {
                    let a = r.activity.clone();
                    (Work::Model(Box::new(r)), a)
                })
            }
        };
        g.status.remove(&id);
        match made {
            Ok((work, activity)) => {
                g.turns.insert(id.clone(), Turn { name, activity });
                self.tell(g, &id);
                Ok(work)
            }
            Err(status) => {
                g.status.insert(id.clone(), status.clone());
                self.live.push("turn", json!({ "engine": id, "index": i, "turn": name, "status": status_word(&status), "line": status_line(&status), "gen": self.live.generation() }).to_string());
                self.tell(g, &id);
                Err(status)
            }
        }
    }

    /// Start turns made by `begin`, one thread each.
    fn spawn(self: &Arc<Self>, works: Vec<Work>) {
        for w in works {
            let host = self.clone();
            match w {
                Work::Model(r) => std::thread::spawn(move || host.run(*r)),
                Work::Session(id) => std::thread::spawn(move || host.run_session(id)),
            };
        }
    }

    /// One turn, to its end.
    fn run(self: Arc<Self>, mut runner: Runner) {
        let io = Io { disk: &self.disk };
        // Outside calls: a declared or a peer's capability (`crate::capabilities`),
        // then the engine's MCP servers.
        let caps = self.caps.clone();
        let servers = self.mcp_for(&runner.id);
        let external = move |c: &Call| caps.route(c).or_else(|| servers.as_ref().and_then(|(s, tools)| s.call(tools, c)));
        let mut came = Came::Start;
        loop {
            let (next, children) = {
                let mut guard = self.lock();
                let g = &mut *guard;
                let next = {
                    let Inner { state, turns, status, .. } = g;
                    let seen = |s: &State, j: usize| g_seen(turns, status, s, j);
                    let cx = Cx { host: io.disk, clock: dry_tools::live::clock(), seen: &seen, external: &external };
                    runner.next_at(state, &cx, came, dry_tools::spans::now_ms())
                };
                let spans = runner.spans();
                self.record(g, spans);
                let id = runner.id.clone();
                match &next {
                    Next::Done(status) => {
                        let name = g.turns.remove(&id).map(|t| t.name).unwrap_or_default();
                        g.status.insert(id.clone(), status.clone());
                        let generation = self.commit(g);
                        let index = g.state.index_of(&id);
                        self.live.push(
                            "turn",
                            json!({ "engine": id, "index": index, "turn": name, "status": status_word(status), "line": status_line(status), "gen": generation }).to_string(),
                        );
                    }
                    Next::Waiting(_) => {
                        let name = g.turns.remove(&id).map(|t| t.name).unwrap_or_default();
                        let generation = self.commit(g);
                        let index = g.state.index_of(&id);
                        self.live.push("turn", json!({ "engine": id, "index": index, "turn": name, "status": "waiting", "line": runner.activity.label(), "gen": generation }).to_string());
                    }
                    _ => {
                        if let Some(t) = g.turns.get_mut(&id) {
                            t.activity = runner.activity.clone();
                        }
                        self.commit(g);
                    }
                }
                if let Some(m) = &runner.measured {
                    g.measured.insert(id.clone(), m.clone());
                }
                if let (Some(w), Some(i)) = (&runner.window, g.state.index_of(&id)) {
                    g.windows.insert(window_key(&g.state.engines[i].inference), w.clone());
                }
                for note in runner.notes.drain(..) {
                    eprintln!("host: {id}: {note}");
                }
                self.tell(g, &id);
                let mut children = Vec::new();
                for c in runner.spawned.drain(..) {
                    if c < g.state.engines.len()
                        && let Ok(r) = self.begin(g, c, false)
                    {
                        children.push(r);
                    }
                }
                (next, children)
            };
            self.spawn(children);
            match next {
                Next::Done(_) | Next::Waiting(_) => {
                    self.settle();
                    return;
                }
                Next::Run(_) => {
                    // The step's waiting calls: all at once, one thread each.
                    let waits = runner_waits(&next);
                    let (came_back, times) = dry_tools::live::run_waits_timed(&waits, &|c: &Call| external(c), io.disk).into_iter().unzip();
                    runner.wait_times(times);
                    came = Came::Ran(came_back);
                }
                Next::Post(_) if runner.streams() => came = self.stream_answer(&mut runner, &next, io.disk, &external),
                _ => came = dry_core::store::ready(runner::io(&next, &dry_tools::net::Native, io.disk, &external)),
            }
        }
    }

    /// Engine `id`'s MCP servers, started from its `mcp.json` the first
    /// time (on the turn's thread: starting waits for each server's tool
    /// list) and again when the file changed; their tools go on the engine
    /// (`mcp_tools`, in the state and its saved copy, so they are no
    /// change). `None`: it has none, or they did not start (logged).
    fn mcp_for(&self, id: &str) -> Option<(Arc<dry_tools::mcp::Servers>, Vec<dry_core::mcp::Tool>)> {
        let json = self.lock().state.engines.iter().find(|e| e.id == id).map(|e| e.mcp.clone())?;
        let mut running = self.mcp.lock().unwrap_or_else(|e| e.into_inner());
        if json.trim().is_empty() {
            running.remove(id);
            return None;
        }
        if let Some((was, servers, tools)) = running.get(id)
            && *was == json
        {
            return Some((servers.clone(), tools.clone()));
        }
        running.remove(id); // dropping the clients stops their servers
        let (clients, tools) = match dry_tools::mcp::start_all(&json) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("host: {id}: MCP servers did not start: {e}");
                return None;
            }
        };
        let servers = Arc::new(dry_tools::mcp::Servers::new(clients));
        running.insert(id.to_string(), (json, servers.clone(), tools.clone()));
        drop(running);
        let mut guard = self.lock();
        let g = &mut *guard;
        for e in g.state.engines.iter_mut().chain(g.saved.engines.iter_mut()).filter(|e| e.id == id) {
            e.mcp_tools = tools.clone();
        }
        Some((servers, tools))
    }

    /// The model's answer to a step, streamed (`dry_core::stream`): its
    /// view kept for pages (`Reported::streaming`) and sent as a `reply`
    /// event (at most every 150 ms, and once at the end), its instant
    /// calls run as each closes (`Runner::early_call`, kept only if the
    /// whole answer reads with the same calls). The lock is taken only
    /// for those moments, never across the network.
    fn stream_answer(
        &self,
        runner: &mut Runner,
        next: &Next,
        disk: &Disk,
        external: &dyn Fn(&Call) -> Option<Result<String, String>>,
    ) -> Came {
        let id = runner.id.clone();
        let Some(mut reader) = ({
            let g = self.lock();
            g.state.index_of(&id).map(|i| dry_core::stream::Reader::new(&g.state.engines[i]))
        }) else {
            return Came::Body(Err("the engine was deleted during its turn".into()));
        };
        let mut sent = std::time::Instant::now();
        let tell = |v: &dry_core::stream::View, done: bool| {
            let mut g = self.lock();
            let index = g.state.index_of(&id);
            g.streaming.insert(id.clone(), v.clone());
            let r = v.reply.clone().unwrap_or_default();
            let data = json!({ "engine": id, "index": index, "text": r.text, "ask": r.ask, "show": r.show, "closed": r.done, "calls": v.calls, "think": v.think.len(), "done": done });
            self.live.push("reply", data.to_string());
        };
        let mut on = |d: dry_core::stream::Delta| {
            let events = reader.feed(&d);
            if matches!(d, dry_core::stream::Delta::Text(_)) {
                runner.first_token(dry_tools::spans::now_ms());
            }
            if reader.view.has_component() {
                runner.first_component(dry_tools::spans::now_ms());
            }
            for ev in &events {
                if let dry_core::stream::Event::CallDone { call, .. } = ev {
                    let guard = self.lock();
                    let g = &*guard;
                    let seen = |s: &State, j: usize| g_seen(&g.turns, &g.status, s, j);
                    let cx = Cx { host: disk, clock: dry_tools::live::clock(), seen: &seen, external };
                    runner.early_call(&g.state, &cx, call);
                }
            }
            if sent.elapsed() >= Duration::from_millis(150) {
                sent = std::time::Instant::now();
                tell(&reader.view, false);
            }
        };
        let came = dry_core::store::ready(runner::io_streaming(next, true, &dry_tools::net::Native, disk, external, &mut on));
        reader.finish();
        tell(&reader.view, true);
        self.lock().streaming.remove(&id);
        came
    }

    /// Every engine's callbacks: a parent whose children all replied goes
    /// on; an `engine_start` child that replied, or a terminal session
    /// that ended, wakes its engine with a message.
    pub fn settle(self: &Arc<Self>) {
        let mut runners = Vec::new();
        {
            let mut guard = self.lock();
            let g = &mut *guard;
            self.expire(g);
            for i in 0..g.state.engines.len() {
                let id = g.state.engines[i].id.clone();
                let busy = g.busy(&id);
                let seen: Vec<Seen> = (0..g.state.engines.len()).map(|j| g.seen(&g.state, j)).collect();
                let settled = dry_core::turn::callbacks(&mut g.state, i, busy, &self.disk, &|j| seen[j].clone());
                if settled.resumed {
                    if let Ok(r) = self.begin(g, i, true) {
                        runners.push(r);
                    }
                } else if !settled.wake.is_empty()
                    && g.state.add_wake(i, &settled.wake.join("\n"))
                    && let Ok(r) = self.begin(g, i, false)
                {
                    runners.push(r);
                }
            }
            self.commit(g);
        }
        self.spawn(runners);
    }

    /// The subscriptions (docs/elements.md): probed on their own thread
    /// (a local server may take up to a second to answer) every minute and
    /// when models.json or sessions.json change, with their usage from the
    /// spans of the last days; every engine is equipped with them.
    pub fn watch_subscriptions(self: &Arc<Self>) {
        let host = self.clone();
        std::thread::spawn(move || {
            let mut watch = dry_tools::subscriptions::Watch::default();
            loop {
                if watch.due() {
                    let mut subs = dry_tools::subscriptions::probe();
                    {
                        let g = host.lock();
                        subs.usage(&dry_tools::subscriptions::recent(g.spans.values()));
                    }
                    // Only prompts show them: engines are equipped, no page
                    // is told to redraw.
                    if host.caps.set_subscriptions(subs) {
                        let reg = host.caps.registry();
                        let mut g = host.lock();
                        g.state.equip(reg.clone());
                        g.saved.equip(reg);
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }

    /// The busy session engines by program, into the subscriptions (their
    /// `running`), every engine equipped again when that changed.
    fn sync_running(&self, g: &mut Inner) {
        let mut running = BTreeMap::new();
        for e in g.state.engines.iter().filter(|e| g.turns.contains_key(&e.id)) {
            if let Some(s) = &e.session {
                *running.entry(s.tool.clone()).or_insert(0) += 1;
            }
        }
        if self.caps.set_running(&running) {
            let reg = self.caps.registry();
            g.state.equip(reg.clone());
            g.saved.equip(reg);
        }
    }

    /// The callbacks' tick, as the window's (every two seconds).
    pub fn tick(self: &Arc<Self>) {
        let host = self.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                host.settle();
                if host.caps.tick() {
                    host.capabilities_changed();
                }
                host.deliver_events();
            }
        });
    }

    /// The store changed and the host did not write it (someone edited the
    /// files, or an old window saved): read it again when no turn runs.
    pub fn outside_change(&self) {
        let mut g = self.lock();
        if g.load_error.is_some() || self.lock.is_none() {
            return;
        }
        let loaded = match dry_tools::store::open(&self.spec).and_then(|s| s.load()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("host: the store changed on disk and does not read: {e}");
                return;
            }
        };
        if loaded == g.saved {
            return;
        }
        if !g.turns.is_empty() {
            eprintln!("host: the store changed on disk during a turn; the host's copy is written over it at the next save");
            return;
        }
        let next = self.live.generation() + 1;
        let old: BTreeMap<String, Engine> = g.saved.engines.iter().map(|e| (e.id.clone(), e.clone())).collect();
        for e in &loaded.engines {
            if old.get(&e.id) != Some(e) {
                g.changed.insert(e.id.clone(), next);
            }
        }
        if loaded.engines.iter().map(|e| &e.id).ne(g.saved.engines.iter().map(|e| &e.id)) {
            g.layout = next;
        }
        // What this machine offers is the host's, not the store's: kept.
        let mut loaded = loaded;
        loaded.capabilities = g.state.capabilities.clone();
        loaded.equip_engines();
        g.state = loaded.clone();
        g.saved = loaded;
        drop(g);
        self.write_lock(next);
    }

    // ---- sessions ----

    /// Post `text` by `author` on the thread engine `on` is on (its
    /// root's), unaddressed unless `to`; the result line, or why not.
    fn post_on(g: &mut Inner, on: &str, author: &str, to: Option<String>, text: &str) -> Result<String, String> {
        let now = dry_tools::live::clock().unix;
        dry_core::shared::post(&mut g.state, on, &dry_core::shared::Edit::Post { author: author.to_string(), at: now, to, text: text.to_string() })
    }

    /// One turn of session engine `id`: its program run with the newest
    /// text, read live when it streams, then its reply, id and report saved.
    fn run_session(self: Arc<Self>, id: String) {
        let prepared = {
            let g = self.lock();
            g.state.index_of(&id).and_then(|i| {
                let e = &g.state.engines[i];
                let s = e.session.clone()?;
                let prompt = e.messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.text.clone()).unwrap_or_default();
                Some((s, prompt))
            })
        };
        let Some((session, prompt)) = prepared else { return self.end_session(&id, None, Status::Failed("the engine was deleted".into())) };
        let tool = match dry_tools::read_sessions()
            .and_then(|all| all.get(&session.tool).cloned().ok_or_else(|| format!("no program {} in sessions.json", session.tool)))
        {
            Ok(t) => t,
            Err(why) => return self.end_session(&id, None, Status::Missing(why)),
        };
        let meta_file =
            tool.wants_meta_file().then(|| std::env::temp_dir().join(format!("dry-host-session-{}-{id}.json", std::process::id())));
        let meta_arg = meta_file.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let prepared = match dry_tools::worker::prepare(&tool, &id, &prompt, session.id.as_deref(), &meta_arg) {
            Ok(p) => p,
            Err(why) => return self.end_session(&id, None, Status::Failed(why)),
        };
        let Some((bin, args)) = prepared.argv.split_first() else {
            return self.end_session(&id, None, Status::Failed("sessions.json: empty command".into()));
        };
        let started = std::time::Instant::now();
        let child = std::process::Command::new(bin)
            .args(args)
            .envs(prepared.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .current_dir(dry_tools::root())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return self.end_session(&id, None, Status::Failed(format!("could not start `{bin}`: {e}"))),
        };
        let stderr = child.stderr.take().map(|mut err| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = std::io::Read::read_to_string(&mut err, &mut text);
                text
            })
        });
        let mut out = String::new();
        let mut live = dry_core::stream_json::Live::default();
        let trigger = {
            let g = self.lock();
            g.state.index_of(&id).map_or_else(Default::default, |i| dry_core::spans::trigger_of(&g.state.engines[i]))
        };
        let mut trace = dry_core::spans::SessionTrace::new(&id, trigger, &prompt, &session.tool, dry_tools::spans::now_ms());
        if let Some(stdout) = child.stdout.take() {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                trace.line(dry_tools::spans::now_ms(), &line);
                out.push_str(&line);
                out.push('\n');
                if tool.streams() {
                    let changes = live.feed(&line);
                    if !changes.is_empty() {
                        self.session_moved(&id, &session.tool, &live, &changes);
                    }
                }
            }
        }
        let code = child.wait().ok().and_then(|s| s.code());
        let stderr = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
        let meta = meta_file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
        if let Some(f) = &meta_file {
            let _ = std::fs::remove_file(f);
        }
        let mut turn = tool.read(&out, meta.as_deref());
        let tail = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        for (k, v) in [
            ("run.program", bin.clone()),
            ("run.exit", code.map_or("signal".into(), |c| c.to_string())),
            ("run.ms", started.elapsed().as_millis().to_string()),
            ("run.cwd", dry_tools::root().display().to_string()),
        ] {
            turn.meta.insert(k.into(), v);
        }
        if !tail.is_empty() {
            turn.meta.insert("run.stderr".into(), tail.chars().take(dry_core::session::META_VALUE_CAP).collect());
        }
        let slash = live.slash();
        if !slash.is_empty() {
            turn.meta.insert(dry_core::stream_json::SLASH_META.into(), slash.join(" "));
        }
        let replies: Vec<String> = if live.results.iter().any(|(t, err)| !err && !t.trim().is_empty()) {
            live.results.iter().filter(|(t, err)| !err && !t.trim().is_empty()).map(|(t, _)| t.clone()).collect()
        } else {
            turn.reply.clone().into_iter().collect()
        };
        let status = match (&turn.error, replies.is_empty()) {
            (Some(err), _) => Status::Failed(err.clone()),
            (None, false) => Status::Ready,
            (None, true) => Status::Failed(format!("no reply (exit {}): {tail}", code.map_or("signal".into(), |c| c.to_string()))),
        };
        let saved = SessionEnd { id: turn.id.or(live.session_id.clone()), meta: turn.meta, tools: live.rows_text(), replies };
        let spans = trace.end(dry_tools::spans::now_ms(), if matches!(status, Status::Ready) { "ready" } else { "failed" });
        {
            let mut g = self.lock();
            self.record(&mut g, spans);
        }
        self.end_session(&id, Some(saved), status);
    }

    /// A streamed line changed the run: keep the view for pages, post the
    /// agents that started or ended, say it.
    fn session_moved(&self, id: &str, program: &str, live: &dry_core::stream_json::Live, changes: &[dry_core::stream_json::Change]) {
        use dry_core::stream_json::Change;
        let mut guard = self.lock();
        let g = &mut *guard;
        g.sessions.insert(id.to_string(), live.clone());
        if let Some(t) = g.turns.get_mut(id) {
            t.activity = Activity::session(program, Some(&live.doing()));
        }
        let mut posted = false;
        for c in changes {
            let (a, text) = match c {
                Change::AgentStarted(a) => {
                    (&live.agents[*a], format!("started ({}): {}", live.agents[*a].kind, live.agents[*a].description))
                }
                Change::AgentDone(a) => (&live.agents[*a], format!("done — {}", live.agents[*a].done.clone().unwrap_or_default())),
                _ => continue,
            };
            let text: String = text.chars().take(dry_core::shared::MAX_ENTRY).collect();
            match Host::post_on(g, id, &a.name, None, &text) {
                Ok(_) => posted = true,
                Err(why) => eprintln!("host: {id}: an agent's post was not kept: {why}"),
            }
        }
        if posted {
            self.commit(g);
        }
        let index = g.state.index_of(id);
        let rows = live.rows.len();
        self.live.push("tool", json!({ "engine": id, "index": index, "rows": rows, "doing": live.doing() }).to_string());
        self.tell(g, id);
    }

    /// A session turn ended: save what it left, say how it ended, settle.
    fn end_session(self: &Arc<Self>, id: &str, saved: Option<SessionEnd>, status: Status) {
        {
            let mut guard = self.lock();
            let g = &mut *guard;
            let name = g.turns.remove(id).map(|t| t.name).unwrap_or_default();
            g.sessions.remove(id);
            if let (Some(i), Some(end)) = (g.state.index_of(id), saved) {
                let e = &mut g.state.engines[i];
                if let (Some(sid), Some(s)) = (end.id, e.session.as_mut()) {
                    s.id = Some(sid);
                }
                if !end.meta.is_empty() {
                    e.meta = end.meta;
                }
                if !end.tools.trim().is_empty() {
                    g.state.add_tools(i, &end.tools);
                }
                for r in &end.replies {
                    g.state.add_reply(i, r);
                }
            }
            g.status.insert(id.to_string(), status.clone());
            let generation = self.commit(g);
            let index = g.state.index_of(id);
            self.live.push("turn", json!({ "engine": id, "index": index, "turn": name, "status": status_word(&status), "line": status_line(&status), "gen": generation }).to_string());
            self.tell(g, id);
        }
        self.settle();
    }

    // ---- what workers read and write (`dry_tools::worker`) ----

    /// Engine `id`'s read (`GET /api/engines/:id/read`).
    pub fn read(&self, id: &str, what: &dry_core::snapshot::Read) -> Result<(String, Option<dry_core::snapshot::Cursor>), Refused> {
        let g = self.lock();
        let seen = |j: usize| g.seen(&g.state, j);
        dry_core::snapshot::read(&g.state, id, what, dry_tools::live::clock(), &seen).map_err(|e| refuse(404, e))
    }

    /// A post by engine `id` (`POST /api/engines/:id/posts`) on its
    /// thread. One with no `to` is kept and comes back to it undelivered
    /// (`dry_core::desk::guided`); the line says so.
    pub fn post(&self, id: &str, text: &str, to: Option<&str>) -> Result<(String, u64), Refused> {
        let mut guard = self.lock();
        let g = &mut *guard;
        Host::writable(g)?;
        g.state.index_of(id).ok_or_else(|| refuse(404, format!("no engine {id}")))?;
        let to = to.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        let named = to.is_some();
        let line = Host::post_on(g, id, id, to, text).map_err(|e| refuse(400, e))?;
        let tail = if named { "" } else { "; it has no to, so it reaches nobody and comes back to you: post it again with to=<engine> or to=you" };
        Ok((format!("{line} on your thread{tail}"), self.commit(g)))
    }

    /// Your guided post from the Desk (`POST /api/threads/:id/posts`): on
    /// root `thread`'s thread (`""`: the first), to `to` (an engine of that
    /// thread; `None`: kept, delivered to nobody,
    /// `dry_core::desk::your_post`). The target's turn starts at the next
    /// settle, which runs at once: the result line, the target, whether it
    /// is busy (then it hears it when its turn ends).
    pub fn post_yours(self: &Arc<Self>, thread: &str, to: Option<&str>, text: &str) -> Result<(String, Option<String>, bool, u64), Refused> {
        let (line, target, busy, generation) = {
            let mut guard = self.lock();
            let g = &mut *guard;
            Host::writable(g)?;
            let now = dry_tools::live::clock().unix;
            let (root, edit) = dry_core::desk::your_post(&g.state, thread, to, text, now).map_err(|e| refuse(400, e))?;
            let dry_core::shared::Edit::Post { to: target, .. } = &edit else { return Err(refuse(400, "no post")) };
            let target = target.clone();
            let mut line = dry_core::shared::post(&mut g.state, &root, &edit).map_err(|e| refuse(400, e))?;
            if target.is_none() {
                line.push_str("; not delivered: no addressee");
            }
            let busy = target.as_deref().is_some_and(|t| g.busy(t));
            (line, target, busy, self.commit(g))
        };
        self.settle();
        Ok((line, target, busy, generation))
    }

    /// Engine `id` starts a child, or gives one a message
    /// (`POST /api/engines/:id/children`): `child` names an existing one,
    /// else `goal` (and `name`, `template`, `text`) makes one. The line, the
    /// child's id and how many replies it had before.
    pub fn children(self: &Arc<Self>, id: &str, body: &Value) -> Result<(String, String, usize), Refused> {
        let word = |k: &str| body.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let (out, works) = {
            let mut guard = self.lock();
            let g = &mut *guard;
            Host::writable(g)?;
            let i = g.state.index_of(id).ok_or_else(|| refuse(404, format!("no engine {id}")))?;
            let mut args: Vec<(String, String)> = Vec::new();
            let name = match (word("child"), word("name"), word("goal")) {
                (Some(c), _, _) => {
                    if !g.state.engines.iter().any(|o| o.id == c && o.parent.as_deref() == Some(id)) {
                        return Err(refuse(404, format!("{c} is not a child of {id}")));
                    }
                    c
                }
                (None, Some(n), Some(_)) => n,
                (None, None, Some(goal)) => goal.split_whitespace().take(4).collect::<Vec<_>>().join(" "),
                (None, _, None) => return Err(refuse(400, "a new child needs goal=\"...\"")),
            };
            args.push(("name".into(), name));
            for k in ["goal", "template", "text", "model"] {
                if let Some(v) = word(k) {
                    args.push((k.into(), v));
                }
            }
            let artifact = match dry_core::live::present(&g.state.engines[i]).into_iter().find(|(_, k)| *k == dry_core::live::Live::Engines)
            {
                Some((n, _)) => n,
                None => {
                    g.state.engines[i].artifacts.insert("engines".into(), "kind: engines".into());
                    "engines".to_string()
                }
            };
            let seen: Vec<Seen> = (0..g.state.engines.len()).map(|j| g.seen(&g.state, j)).collect();
            let mut fam = dry_core::engines::family(&g.state, i, &|j| seen[j].clone());
            let chose = args.iter().any(|(k, _)| k == "model");
            let call = Call { name: "engine_start".into(), args };
            let line = dry_core::engines::call(&mut g.state.engines[i], &artifact, &call, &mut fam).map_err(|e| refuse(409, e))?;
            let child = match fam.effects.iter().find_map(|x| match x {
                dry_core::engines::Effect::Ask(a) => Some(a.child.clone()),
                _ => None,
            }) {
                Some(c) => c,
                None => return Err(refuse(500, "the call asked no child")),
            };
            let before = g.state.index_of(&child).map_or(0, |c| dry_core::engines::replies(&g.state.engines[c]).len());
            let count = g.state.engines.len();
            let applied = dry_core::engines::apply(&mut g.state, i, fam.effects);
            // A session's child is a model engine: it gets the app's
            // default model, contract and MCP spec (a session has none to
            // give); its model stays the one model= chose, if any.
            if g.state.engines[i].session.is_some() {
                for c in count..g.state.engines.len() {
                    let template = Host::defaults();
                    let e = &mut g.state.engines[c];
                    if let Some(ai) = template.0.filter(|_| !chose) {
                        e.inference = ai;
                    }
                    if let Some((r, f)) = template.1 {
                        (e.response, e.format) = (r, f);
                    }
                    if let Some(mcp) = template.2 {
                        e.mcp = mcp;
                    }
                }
            }
            for note in applied.notes {
                eprintln!("host: {id}: {note}");
            }
            let works: Vec<Work> = applied.run.into_iter().filter_map(|c| self.begin(g, c, false).ok()).collect();
            self.commit(g);
            ((line, child, before), works)
        };
        self.spawn(works);
        Ok(out)
    }

    /// Engine `id`'s child `child`'s reply after its `after`th, once it is
    /// in (`POST /api/engines/:id/children/:child/take`): taken, so it does
    /// not also come back as a message.
    pub fn take(&self, id: &str, child: &str, after: usize) -> Result<Value, Refused> {
        let mut guard = self.lock();
        let g = &mut *guard;
        let i = g.state.index_of(id).ok_or_else(|| refuse(404, format!("no engine {id}")))?;
        let c = g
            .state
            .index_of(child)
            .filter(|c| g.state.engines[*c].parent.as_deref() == Some(id))
            .ok_or_else(|| refuse(404, format!("{child} is not a child of {id}")))?;
        let seen = g.seen(&g.state, c);
        if seen.busy {
            return Ok(json!({ "busy": true, "doing": seen.label }));
        }
        let replies = dry_core::engines::replies(&g.state.engines[c]);
        if replies.len() > after {
            for text in
                g.state.engines[i].artifacts.values_mut().filter(|t| dry_core::live::Live::of(t) == Some(dry_core::live::Live::Engines))
            {
                let mut lines = dry_core::engines::parse(text);
                for l in lines.iter_mut().filter(|l| l.id == child) {
                    l.seen = replies.len();
                }
                *text = dry_core::engines::to_text(&lines);
            }
            self.commit(g);
            return Ok(json!({ "reply": replies.last() }));
        }
        Ok(match seen.failed {
            Some(why) => json!({ "failed": why }),
            None => json!({ "busy": false }),
        })
    }

    /// The default model, contract and MCP spec a new engine gets.
    fn defaults()
    -> (Option<dry_core::inference::OpenAi>, Option<(dry_core::response::Responses, dry_core::response::Format)>, Option<String>) {
        let ai = dry_tools::settings::read_models().ok().and_then(|m| m.default_model().and_then(|d| m.inference(d).ok()));
        (ai, dry_tools::read_responses().ok(), dry_tools::default_mcp().ok())
    }

    // ---- approvals ----

    /// Engine `engine` asks to use `tool` (`POST /api/approvals`): allowed
    /// at once by a rule of its spec, else kept pending and its owner told.
    pub fn ask_approval(&self, engine: &str, tool: &str, input: Value) -> Result<Approval, Refused> {
        let mut guard = self.lock();
        let g = &mut *guard;
        Host::writable(g)?;
        let i = g.state.index_of(engine).ok_or_else(|| refuse(404, format!("no engine {engine}")))?;
        let e = &g.state.engines[i];
        g.approvals_made += 1;
        let mut a = Approval {
            id: format!("a{}", g.approvals_made),
            engine: engine.to_string(),
            tool: tool.to_string(),
            input,
            asked: dry_tools::live::clock().unix,
            timeout: e.approve_timeout,
            decided: None,
        };
        if let Some(rule) = dry_core::approval::allowed_by(&dry_core::approval::parse_rules(&e.allow), tool, &a.input) {
            a.decided = Some(Decided { decision: Decision::Allow, reason: String::new(), by: format!("rule {}", rule.text()) });
        } else {
            // To you on its thread: it calls no engine.
            if let Err(why) = Host::post_on(g, engine, engine, Some(dry_core::desk::YOU.into()), &a.notice()) {
                eprintln!("host: the approval notice was not posted: {why}");
            }
        }
        g.approvals.push(a.clone());
        Host::trim_approvals(g);
        if let Some(span) = dry_core::spans::approval(&a, dry_tools::spans::now_ms()) {
            self.record(g, vec![span]);
        }
        self.commit(g);
        self.live.push("approval", a.to_json().to_string());
        Ok(a)
    }

    fn trim_approvals(g: &mut Inner) {
        let decided = g.approvals.iter().filter(|a| a.decided.is_some()).count();
        let mut drop = decided.saturating_sub(KEEP_APPROVALS);
        g.approvals.retain(|a| {
            if drop > 0 && a.decided.is_some() {
                drop -= 1;
                false
            } else {
                true
            }
        });
    }

    /// One approval (`GET /api/approvals/:id`).
    pub fn approval(&self, id: &str) -> Option<Approval> {
        let mut guard = self.lock();
        self.expire(&mut guard);
        guard.approvals.iter().find(|a| a.id == id).cloned()
    }

    /// Every approval kept, pending first (`GET /api/approvals`).
    pub fn approvals(&self) -> Vec<Approval> {
        let g = self.lock();
        let mut out: Vec<Approval> = g.approvals.iter().filter(|a| a.decided.is_none()).cloned().collect();
        out.extend(g.approvals.iter().rev().filter(|a| a.decided.is_some()).cloned());
        out
    }

    /// A person's answer (`POST /api/approvals/:id`).
    pub fn decide(&self, id: &str, decision: Decision, reason: &str) -> Result<Approval, Refused> {
        let mut guard = self.lock();
        let g = &mut *guard;
        let a = g.approvals.iter_mut().find(|a| a.id == id).ok_or_else(|| refuse(404, format!("no approval {id}")))?;
        if a.decided.is_some() {
            return Err(refuse(409, format!("approval {id} is already decided")));
        }
        a.decided = Some(Decided { decision, reason: reason.trim().to_string(), by: "a person".into() });
        let a = a.clone();
        if let Some(span) = dry_core::spans::approval(&a, dry_tools::spans::now_ms()) {
            self.record(g, vec![span]);
        }
        self.live.push("approval", a.to_json().to_string());
        Ok(a)
    }

    /// Deny the approvals whose time ran out.
    fn expire(&self, g: &mut Inner) {
        let now = dry_tools::live::clock().unix;
        let mut spans = Vec::new();
        for a in g.approvals.iter_mut().filter(|a| a.timed_out(now)) {
            a.decided = Some(Decided {
                decision: Decision::Deny,
                reason: format!("nobody answered in {}s", a.timeout.unwrap_or(0)),
                by: "timeout".into(),
            });
            spans.extend(dry_core::spans::approval(a, dry_tools::spans::now_ms()));
            self.live.push("approval", a.to_json().to_string());
        }
        self.record(g, spans);
    }

    // ---- intents ----

    /// A new thread: a new root engine made from the default's translator
    /// (`dry_core::shipped::new_thread`) on the default model
    /// (`models.json`). Its index.
    fn new_thread(g: &mut Inner) -> Result<usize, Refused> {
        let ai = dry_tools::settings::read_models().ok().and_then(|m| m.default_model().and_then(|d| m.inference(d).ok())).unwrap_or_default();
        dry_core::shipped::new_thread(&mut g.state, &dry_tools::store::base().texts, &ai).map_err(|e| refuse(503, e))
    }

    fn writable(g: &Inner) -> Result<(), Refused> {
        match &g.load_error {
            Some(e) => Err(refuse(503, format!("the store did not read, so the host writes nothing: {e}"))),
            None => Ok(()),
        }
    }

    /// `text` to engine `target` (its index or id; `new`: a new engine),
    /// as the user's message, and its turn started.
    pub fn send(self: &Arc<Self>, target: &str, text: &str, base: Option<u64>) -> Result<Taken, Refused> {
        if text.trim().is_empty() {
            return Err(refuse(400, "nothing to send: the text is empty"));
        }
        let (taken, runner) = {
            let mut guard = self.lock();
            let g = &mut *guard;
            Host::writable(g)?;
            let i = if target == "new" { Host::new_thread(g)? } else { g.resolve(target, base)? };
            let id = g.state.engines[i].id.clone();
            if g.busy(&id) {
                return Err(refuse(409, BUSY));
            }
            g.fresh(i, base)?;
            g.state.add_text(i, text);
            let runner = self.begin(g, i, false).ok();
            let generation = self.commit(g);
            let turn = format!("{id}/{}", g.state.engines[i].messages.len() - 1);
            (Taken { turn, generation, index: i, id }, runner)
        };
        self.spawn(runner.into_iter().collect());
        Ok(taken)
    }

    /// Answer the ask in engine `target`'s message `message`: `answer` (the
    /// text `Ask::answer` makes), else what the page gathered (`pick`), as
    /// the user's next message, and the turn started.
    pub fn answer(
        self: &Arc<Self>,
        target: &str,
        message: usize,
        answer: Option<&str>,
        pick: &Value,
        base: Option<u64>,
    ) -> Result<Taken, Refused> {
        let (taken, runner) = {
            let mut guard = self.lock();
            let g = &mut *guard;
            Host::writable(g)?;
            let i = g.resolve(target, base)?;
            let e = &g.state.engines[i];
            let id = e.id.clone();
            if g.busy(&id) {
                return Err(refuse(409, BUSY));
            }
            let m = e
                .messages
                .get(message)
                .filter(|m| m.role == Role::Assistant)
                .ok_or_else(|| refuse(404, format!("no reply {message} in {}", e.name)))?;
            let a = ask::ask_in(e, &m.text).ok_or_else(|| refuse(404, format!("reply {message} asks nothing")))?;
            if ask::state(e, message) != ask::State::Open {
                return Err(refuse(409, "already answered"));
            }
            g.fresh(i, base)?;
            let text = match answer.map(str::trim).filter(|t| !t.is_empty()) {
                Some(t) => t.to_string(),
                None => answer_text(&a, pick).map_err(|why| refuse(400, why))?,
            };
            g.state.add_text(i, &text);
            let runner = self.begin(g, i, false).ok();
            let generation = self.commit(g);
            let turn = format!("{id}/{}", g.state.engines[i].messages.len() - 1);
            (Taken { turn, generation, index: i, id }, runner)
        };
        self.spawn(runner.into_iter().collect());
        Ok(taken)
    }

    /// A new thread or session (`POST /api/engines`): a new root engine,
    /// its index and id. `kind`: a program in `sessions.json` for a
    /// session, else (`thread`, or the old `model`) a new thread.
    pub fn create(&self, kind: Option<&str>) -> Result<Taken, Refused> {
        let mut g = self.lock();
        Host::writable(&g)?;
        let i = match kind.map(str::trim).filter(|k| !k.is_empty() && *k != "model" && *k != "thread") {
            Some(tool) => {
                let all = dry_tools::read_sessions().map_err(|e| refuse(503, e))?;
                let t = all.get(tool).ok_or_else(|| refuse(404, format!("no program {tool} in sessions.json")))?;
                g.state.add_session(tool, &t.description)
            }
            None => Host::new_thread(&mut g)?,
        };
        let id = g.state.engines[i].id.clone();
        let generation = self.commit(&mut g);
        Ok(Taken { turn: String::new(), generation, index: i, id })
    }

    /// Delete engine `target` and its children: refused while it is in a turn.
    pub fn delete(&self, target: &str, base: Option<u64>) -> Result<u64, Refused> {
        let mut g = self.lock();
        Host::writable(&g)?;
        let i = g.resolve(target, base)?;
        let id = g.state.engines[i].id.clone();
        if g.busy(&id) {
            return Err(refuse(409, BUSY));
        }
        g.state.remove_engine(i);
        g.status.retain(|k, _| *k != id);
        Ok(self.commit(&mut g))
    }

    /// Engine `target`'s answer format: `value` (`json`, `toon`, `toon=`),
    /// or the other one when `value` is empty.
    pub fn set_format(&self, target: &str, value: &str, base: Option<u64>) -> Result<u64, Refused> {
        use dry_core::response::Format;
        let mut g = self.lock();
        Host::writable(&g)?;
        let i = g.resolve(target, base)?;
        if g.busy(&g.state.engines[i].id) {
            return Err(refuse(409, BUSY));
        }
        let e = &mut g.state.engines[i];
        e.format = match value {
            "" => match e.format {
                Format::Json(_) => Format::Toon(Default::default()),
                Format::Toon(_) => Format::Json(dry_core::response::Json),
            },
            v => Format::parse_name(v).ok_or_else(|| refuse(400, format!("no format {v}")))?,
        };
        Ok(self.commit(&mut g))
    }

    /// Read engine `target`'s contract again: `responses` gives it the
    /// one in `responses.md` (its format too); `spec` reads its spec back
    /// from the store (name, instructions, contract, format, tools, model,
    /// `mcp.json`) after its files were edited by hand. What happened
    /// (messages, artifacts) stays. Refused while it is in a turn.
    pub fn reload(&self, target: &str, what: &str, base: Option<u64>) -> Result<u64, Refused> {
        let mut g = self.lock();
        Host::writable(&g)?;
        let i = g.resolve(target, base)?;
        let id = g.state.engines[i].id.clone();
        if g.busy(&id) {
            return Err(refuse(409, BUSY));
        }
        match what {
            "responses" => {
                let (r, f) = dry_tools::read_responses().map_err(|e| refuse(503, e))?;
                let e = &mut g.state.engines[i];
                (e.response, e.format) = (r, f);
            }
            "spec" => {
                let spec = dry_tools::store::open(&self.spec)
                    .and_then(|s| s.load())
                    .and_then(|s| s.engines.into_iter().find(|e| e.id == id).ok_or(format!("{id} is not in the store")))
                    .map_err(|e| refuse(404, e))?;
                let e = &mut g.state.engines[i];
                e.name = spec.name;
                e.description = spec.description;
                e.legacy_goal = spec.legacy_goal;
                e.inference = spec.inference;
                e.response = spec.response;
                e.format = spec.format;
                e.tools = spec.tools;
                e.mcp = spec.mcp;
            }
            other => return Err(refuse(400, format!("nothing to reload called {other}: responses or spec"))),
        }
        g.status.remove(&id);
        Ok(self.commit(&mut g))
    }

    /// A line typed into the Code page's terminal (`Terminal::submit`):
    /// `clear` empties it; a command runs on its own thread in the repo
    /// folder, through the terminal artifact's disk (`Disk::run`: `sh -c`,
    /// no input, secrets left out of its environment), its output shown
    /// as it comes. Every open page sees the same terminal.
    pub fn run_typed(self: &Arc<Self>, line: &str) -> Result<u64, Refused> {
        use dry_core::workbench::Submit;
        let submit = self.lock().terminal.submit(line);
        match submit {
            Submit::Nothing => return Err(refuse(400, "nothing to run: the line is empty")),
            Submit::Cleared => {}
            Submit::Run { id, command } => {
                let me = self.clone();
                std::thread::spawn(move || me.run_command(id, command));
            }
        }
        Ok(self.live.bump(&["terminal"]))
    }

    fn run_command(self: Arc<Self>, id: usize, command: String) {
        use dry_core::live::{Host as _, Job};
        use dry_core::workbench::MAX_OUTPUT_LINES;
        let disk = Disk { repo: self.disk.repo.clone(), engines: terminal_dir() };
        let session = id as u32;
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let (disk, command) = (disk.clone(), command.clone());
            std::thread::spawn(move || {
                let job = Job { engine: "host", artifact: "terminal", session, root: ".", command: &command };
                let _ = tx.send(disk.run(&job));
            });
        }
        let job = Job { engine: "host", artifact: "terminal", session, root: ".", command: &command };
        loop {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(ended) => {
                    let out = disk.tail(&job, MAX_OUTPUT_LINES);
                    self.lock().terminal.finish(id, ended.map(|code| (out, code)));
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let out = disk.tail(&job, MAX_OUTPUT_LINES);
                    let moved = self.lock().terminal.show(id, &out);
                    if moved {
                        self.live.bump(&["terminal"]);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.lock().terminal.finish(id, Err("the command's thread ended".into()));
                    break;
                }
            }
        }
        self.live.bump(&["terminal"]);
    }

    /// The app's mode (`light`, `dark`).
    /// The app's scene (app.md `scene:`): a name in scenes/, or `none` to
    /// turn it off. A machine's local.md `## Scene` row still beats it.
    pub fn set_scene(&self, value: &str) -> Result<u64, Refused> {
        let value = value.trim();
        if value != dry_core::theme::NO_SCENE && !dry_tools::scenes_dir().join(format!("{value}.md")).is_file() {
            return Err(refuse(400, format!("no scene {value} in scenes/")));
        }
        let mut g = self.lock();
        Host::writable(&g)?;
        g.state.scene = Some(value.to_string());
        Ok(self.commit(&mut g))
    }

    pub fn set_mode(&self, value: &str) -> Result<u64, Refused> {
        let mode = dry_core::theme::Mode::parse(value).ok_or_else(|| refuse(400, format!("no mode {value}")))?;
        let mut g = self.lock();
        Host::writable(&g)?;
        g.state.mode = mode;
        Ok(self.commit(&mut g))
    }

    /// Reset history (docs/default.md, "Reset"): under the host's lock,
    /// with no engine in a turn (refused otherwise: a turn is never cut
    /// off, stop it or let it end first), MOVE the store's own files to a
    /// backup beside it (`dry_tools::store::reset`), and start again from
    /// the default alone: the state, the spans, the statuses, the decided
    /// approvals and the terminal's finished commands all go. The
    /// generation bumps with every engine marked changed and the layout
    /// moved, so every page reloads and a view from before is stale.
    /// The plan carried out, and the generation.
    pub fn reset(&self) -> Result<(dry_tools::store::ResetPlan, u64), Refused> {
        let mut g = self.lock();
        if let Some(id) = g.turns.keys().next() {
            return Err(refuse(409, format!("{id} is in a turn: let it end (or stop it), then reset; nothing was moved")));
        }
        if g.terminal.busy() {
            return Err(refuse(409, "a command is running in the Code page's terminal: let it end, then reset; nothing was moved"));
        }
        if g.approvals.iter().any(|a| a.decided.is_none()) {
            return Err(refuse(409, "an approval is waiting for a decision: decide it first, then reset; nothing was moved"));
        }
        let (plan, state) = if self.lock.is_some() {
            // The watcher counts the move as the host's own write, not someone else's.
            let mut print = self.live.store_print.lock().unwrap_or_else(|e| e.into_inner());
            let plan = dry_tools::store::reset(&self.spec).map_err(|e| refuse(500, e))?;
            let state = match dry_tools::store::open(&self.spec).and_then(|s| s.load()) {
                Ok(s) => s,
                Err(e) => {
                    // Never write the old state into the emptied store.
                    let why = format!("reset (the history is in {}), but the default does not read: {e}", plan.backup.display());
                    g.load_error = Some(why.clone());
                    return Err(refuse(500, why));
                }
            };
            *print = Some(crate::live::store_print());
            (plan, state)
        } else {
            // In memory (tests): no store to move; the default as shipped.
            let state = dry_core::store::from_texts(&dry_core::shipped::texts()).map_err(|e| refuse(500, e))?;
            let plan = dry_tools::store::ResetPlan {
                store: self.spec.clone(),
                backup: PathBuf::new(),
                moves: Vec::new(),
                keeps: Vec::new(),
                engines: g.state.engines.len(),
                messages: 0,
            };
            (plan, state)
        };
        let mut state = state;
        state.capabilities = g.state.capabilities.clone();
        state.equip_engines();
        let mut terminal = std::mem::take(&mut g.terminal);
        let _ = terminal.submit("clear");
        let fresh = Inner {
            saved: state.clone(),
            state,
            spans_dir: g.spans_dir.clone(),
            approvals_made: g.approvals_made,
            terminal,
            ..Inner::default()
        };
        *g = fresh;
        Host::load_spans(&mut g);
        let generation = self.live.bump(&["store", "reset"]);
        g.layout = generation;
        let ids: Vec<String> = g.state.engines.iter().map(|e| e.id.clone()).collect();
        for id in ids {
            g.changed.insert(id, generation);
        }
        drop(g);
        self.write_lock(generation);
        Ok((plan, generation))
    }
}

/// Where the Code page's terminal keeps its commands' logs while this
/// host runs (removed when it stops).
fn terminal_dir() -> PathBuf {
    std::env::temp_dir().join(format!("dry-host-terminal-{}", std::process::id()))
}

/// What a session turn left to save.
struct SessionEnd {
    id: Option<String>,
    meta: BTreeMap<String, String>,
    /// Its tool calls as one tools message (empty: none).
    tools: String,
    replies: Vec<String>,
}

/// How engine `j` looks to its parent's tools, from the host's turns.
fn g_seen(turns: &HashMap<String, Turn>, status: &HashMap<String, Status>, state: &State, j: usize) -> Seen {
    let Some(e) = state.engines.get(j) else { return Seen::default() };
    let a = dry_core::dashboard::live_activity(e, status.get(&e.id), turns.get(&e.id).map(|t| &t.activity));
    let failed = match status.get(&e.id) {
        Some(Status::Failed(why) | Status::Missing(why)) => Some(why.clone()),
        _ => None,
    };
    Seen { label: a.label(), busy: turns.contains_key(&e.id) || !e.waiting.is_empty(), failed }
}

/// The waiting calls a `Next::Run` hands the shell, as `run_waits` takes them.
fn runner_waits(next: &Next) -> Vec<(Call, dry_core::turn::Wait)> {
    match next {
        Next::Run(waits) => waits.iter().map(|w| (Call { name: String::new(), args: Vec::new() }, w.clone())).collect(),
        _ => Vec::new(),
    }
}

/// An ask's answer from what a page gathered (`pick`), the way the
/// browser makes it: one `Input` for a single control (`input`), one per
/// field for a form (`inputs`), the rows (or `reject`) for a review.
pub fn answer_text(a: &Ask, pick: &Value) -> Result<String, String> {
    let input = |v: &Value| Input {
        picked: v
            .get("picked")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        day: v.get("day").and_then(Value::as_i64),
        minute: v.get("minute").and_then(Value::as_u64).map(|m| m as u32),
        typed: v.get("typed").and_then(Value::as_str).unwrap_or_default().to_string(),
    };
    match a {
        Ask::Form { .. } => {
            let inputs: Vec<Input> =
                pick.get("inputs").and_then(Value::as_array).map(|l| l.iter().map(input).collect()).unwrap_or_default();
            a.submit(&inputs)
        }
        Ask::Review { .. } if pick.get("reject").and_then(Value::as_bool) == Some(true) => Ok(a.answer(&Answer::Review(None))),
        Ask::Review { .. } => {
            let rows: Vec<(String, String)> = pick
                .get("rows")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(|r| Some((r.get(0)?.as_str()?.to_string(), r.get(1)?.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            Ok(a.answer(&Answer::Review(Some(rows))))
        }
        _ => match a.read_input(&input(pick.get("input").unwrap_or(&Value::Null)))? {
            Some(answer) => Ok(a.answer(&answer)),
            None => Err("nothing picked or typed: answer first".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::inference::OpenAi;

    fn state() -> State {
        let mut s = State::default();
        let i = s.add_engine();
        s.engines[i].inference = OpenAi::default();
        s.add_engine();
        s
    }

    #[test]
    fn every_write_bumps_the_generation_and_marks_what_it_changed() {
        let host = Host::in_memory(state());
        assert_eq!(host.live.generation(), 0);
        // No model: the message is added, the turn ends at once saying why.
        let t = host.send("0", "hello", Some(0)).unwrap();
        assert_eq!(t.generation, 1);
        assert_eq!(t.turn, format!("{}/0", t.id));
        let snap = host.snapshot();
        assert_eq!(snap.state.engines[0].messages.len(), 1);
        assert!(matches!(snap.reported.status.get(&0), Some(Status::Missing(_))), "{:?}", snap.reported.status);
        let names: Vec<&str> = host.live.since(0).unwrap().iter().map(|e| e.name).collect();
        assert!(names.contains(&"turn") && names.contains(&"message") && names.contains(&"change"), "{names:?}");

        // A view from before engine 0 changed is stale for it, not for engine 1.
        let stale = host.send("0", "again", Some(0)).unwrap_err();
        assert_eq!(stale.status, 409);
        assert!(stale.error.starts_with("stale"), "{}", stale.error);
        assert!(host.send("1", "fine", Some(0)).is_ok());
        assert!(host.send("0", "now fresh", Some(t.generation)).is_ok());

        // Adding an engine moves the layout: a send by index against an
        // older view is refused; by id it is not.
        let made = host.create(None).unwrap();
        let before = made.generation - 1;
        assert_eq!(host.send("1", "x", Some(before)).unwrap_err().status, 409);
        let id1 = host.snapshot().state.engines[1].id.clone();
        assert!(host.send(&id1, "by id", Some(host.live.generation())).is_ok());
        assert_eq!(host.send("9", "x", None).unwrap_err().status, 404);
        assert_eq!(host.send("0", "  ", None).unwrap_err().status, 400);
    }

    /// A fake CLI model: answers every prompt with `text`, its usage and cost.
    fn fake_model(dir: &std::path::Path, text: &str) -> OpenAi {
        let program = dir.join("fake-claude");
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake-haiku\"}}'\necho '{{\"type\":\"result\",\"is_error\":false,\"result\":\"{text}\",\"total_cost_usd\":0.25,\"usage\":{{\"input_tokens\":7,\"output_tokens\":3}}}}'\n"
        );
        std::fs::write(&program, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        OpenAi { base_url: dry_core::cli::base_url("claude", Some(&program.display().to_string())), model: "haiku".into(), context: Some(32_000), ..OpenAi::default() }
    }

    #[test]
    fn the_host_appends_each_turns_spans_to_the_engines_log() {
        let dir = std::env::temp_dir().join(format!("dry-host-spans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = State::default();
        let i = s.add_engine();
        s.engines[i].inference = fake_model(&dir, "pong");
        let id = s.engines[i].id.clone();
        let host = Host::in_memory(s);
        host.lock().spans_dir = Some(dir.join("engines"));
        host.send("0", "ping", None).unwrap();
        let start = std::time::Instant::now();
        while !host.snapshot().reported.status.contains_key(&0) && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(host.snapshot().reported.status.get(&0), Some(&Status::Ready));
        let logged = dry_tools::spans::load(&dir.join("engines"), &id);
        let kinds: Vec<&str> = logged.iter().map(|s| s.kind.as_str()).collect();
        assert!(kinds.contains(&"turn") && kinds.contains(&"llm") && kinds.contains(&"task"), "{kinds:?}");
        let llm = logged.iter().find(|s| s.kind == dry_core::spans::Kind::Llm).unwrap();
        assert_eq!((llm.num("cost_usd"), llm.num("prompt_tokens"), llm.str("provider")), (Some(0.25), Some(7.0), Some("claude")));
        let turn = logged.iter().find(|s| s.kind == dry_core::spans::Kind::Turn).unwrap();
        assert!(turn.parent.as_deref().is_some_and(|p| p.contains(":task:")), "under its task");
        assert_eq!(host.snapshot().reported.spans[&id].len(), logged.len(), "pages read what was written");

        // An approval a person decides is a span under the engine's log too.
        let a = host.ask_approval(&id, "Bash", json!({"command": "ls"})).unwrap();
        host.decide(&a.id, Decision::Allow, "").unwrap();
        let approvals: Vec<_> = dry_tools::spans::load(&dir.join("engines"), &id).into_iter().filter(|s| s.kind == dry_core::spans::Kind::Approval).collect();
        assert_eq!((approvals.len(), approvals[0].str("decided_by")), (1, Some("a person")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_guided_post_starts_its_engines_turn_and_nothing_else_does() {
        let dir = std::env::temp_dir().join(format!("dry-host-guided-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = State::default();
        for name in ["translator", "other"] {
            let i = s.add_engine();
            s.engines[i].id = name.into();
            s.engines[i].inference = fake_model(&dir, "on it");
        }
        let host = Host::in_memory(s);
        // A post for you calls nobody; so does yours with no addressee.
        host.post("other", "for the user", Some(dry_core::desk::YOU)).unwrap();
        let (line, to, _, _) = host.post_yours("translator", None, "anyone?").unwrap();
        assert!(line.ends_with("not delivered: no addressee") && to.is_none(), "{line}");
        host.settle();
        assert!(host.snapshot().state.engines.iter().all(|e| e.messages.is_empty()), "nobody was called");
        // Your post goes to the engine it names and starts its turn.
        let (line, to, busy, _) = host.post_yours("translator", Some("translator"), "make it green").unwrap();
        assert!(line.starts_with("posted entry 2"), "{line}");
        assert_eq!((to.as_deref(), busy), (Some("translator"), false));
        let start = std::time::Instant::now();
        while !host.snapshot().reported.status.contains_key(&0) && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let snap = host.snapshot();
        let t = &snap.state.engines[0];
        assert_eq!(t.messages[0].text, format!("{}\n[thread 2] you → translator: make it green", dry_core::desk::WAKE_HEAD));
        assert_eq!(t.messages.last().map(|m| m.text.as_str()), Some("on it"));
        assert!(snap.state.engines[1].messages.is_empty(), "other was not addressed");
        assert!(snap.state.shared["other"].contains("other → you: for the user"), "each root its own thread");
        // Delivered once.
        host.settle();
        assert_eq!(host.snapshot().state.engines[0].messages.len(), 2);
        assert_eq!(host.post_yours("", Some("ghost"), "x").unwrap_err().status, 400);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_engines_mcp_servers_start_once_and_take_its_calls() {
        let mut s = State::default();
        let i = s.add_engine();
        s.engines[i].mcp = dry_tools::mcp::test_server_config();
        let id = s.engines[i].id.clone();
        let host = Host::in_memory(s);
        let (servers, tools) = host.mcp_for(&id).expect("the test server starts");
        assert!(tools.iter().any(|t| t.call_name() == "calc.add"), "{tools:?}");
        assert_eq!(host.snapshot().state.engines[0].mcp_tools.len(), tools.len(), "the engine has its tools");
        let call = &dry_core::calls::parse("calc.add(a=2, b=3)").unwrap()[0];
        let got = servers.call(&tools, call).unwrap().unwrap();
        assert!(got.contains('5'), "{got}");
        let (again, _) = host.mcp_for(&id).unwrap();
        assert!(Arc::ptr_eq(&servers, &again), "started once");
        host.lock().state.engines[0].mcp.clear();
        assert!(host.mcp_for(&id).is_none(), "no mcp.json, no servers");
    }

    #[test]
    fn a_busy_engine_refuses_and_the_others_do_not() {
        let host = Host::in_memory(state());
        {
            let mut g = host.lock();
            let id = g.state.engines[0].id.clone();
            g.turns.insert(id, Turn { name: "x/0".into(), activity: Activity::Llm { step: 1 } });
        }
        let r = host.send("0", "hi", None).unwrap_err();
        assert_eq!(r, Refused { status: 409, error: BUSY.into() });
        assert_eq!(host.delete("0", None).unwrap_err().status, 409);
        assert!(host.send("1", "hi", None).is_ok());
        assert_eq!(host.snapshot().reported.activity.get(&0), Some(&Activity::Llm { step: 1 }));
    }

    #[test]
    fn an_answer_is_the_core_s_text_and_only_once() {
        let mut s = state();
        let e = &mut s.engines[0];
        e.response = dry_core::response::Responses::from_md(
            "format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n",
        )
        .unwrap()
        .0;
        e.tools = vec!["reply".into()];
        s.add_text(0, "Shall I?");
        s.add_reply(0, "thought: ask\nact: reply(text=\"Go ahead?\", ask=\"confirm\")");
        s.add_reply(1, "no ask here");
        let host = Host::in_memory(s);
        assert_eq!(host.answer("1", 0, Some("x"), &Value::Null, None).unwrap_err().status, 404);
        assert_eq!(host.answer("0", 0, Some("x"), &Value::Null, None).unwrap_err().status, 404, "message 0 is the user's");
        assert_eq!(host.answer("0", 1, None, &json!({}), None).unwrap_err().status, 400, "nothing picked");
        host.answer("0", 1, None, &json!({ "input": { "picked": ["yes"] } }), None).unwrap();
        let snap = host.snapshot();
        assert_eq!(snap.state.engines[0].messages[2].text, "Answer (confirm): yes");
        let again = host.answer("0", 1, Some("Answer (confirm): no"), &Value::Null, None).unwrap_err();
        assert_eq!(again, Refused { status: 409, error: "already answered".into() });
    }

    #[test]
    fn an_approval_is_allowed_by_a_rule_else_told_to_you_and_waits_for_a_person() {
        let mut s = state();
        let w = s.add_session("claude", "a worker");
        let id = s.engines[w].id.clone();
        s.engines[w].allow = "Read, Bash(git status*)".into();
        let host = Host::in_memory(s);
        let a = host.ask_approval(&id, "Read", json!({"file_path": "/x"})).unwrap();
        assert_eq!(a.decided.as_ref().map(|d| (d.decision, d.by.as_str())), Some((Decision::Allow, "rule Read")));
        assert!(!host.snapshot().state.shared.contains_key(&id), "a rule needs nobody: no notice");

        let b = host.ask_approval(&id, "Bash", json!({"command": "rm -rf build"})).unwrap();
        assert!(b.decided.is_none());
        let thread = host.snapshot().state.shared.get(&id).cloned().unwrap_or_default();
        assert!(
            thread.contains(&format!("{id} → you: approval a2: {id} asks to use Bash(rm -rf build)")),
            "you are told on its thread: {thread}"
        );
        assert_eq!(host.snapshot().reported.approvals.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["a2"]);
        assert_eq!(host.approvals()[0].id, "a2", "pending first");
        let names: Vec<&str> = host.live.since(0).unwrap().iter().map(|e| e.name).collect();
        assert!(names.contains(&"approval"), "{names:?}");

        let d = host.decide("a2", Decision::Deny, "not now").unwrap();
        assert_eq!(dry_core::approval::answer(d.decided.as_ref().unwrap(), &d.input), json!({"behavior": "deny", "message": "not now"}));
        assert_eq!(host.decide("a2", Decision::Allow, "").unwrap_err().status, 409);
        assert_eq!(host.decide("a9", Decision::Allow, "").unwrap_err().status, 404);
        assert!(host.snapshot().reported.approvals.is_empty());
        assert_eq!(host.ask_approval("ghost", "Bash", json!({})).unwrap_err().status, 404);
    }

    #[test]
    fn an_approval_with_a_timeout_is_denied_when_nobody_answers() {
        let mut s = state();
        let w = s.add_session("claude", "a worker");
        let id = s.engines[w].id.clone();
        s.engines[w].approve_timeout = Some(1);
        let host = Host::in_memory(s);
        let a = host.ask_approval(&id, "Edit", json!({"file_path": "a.rs"})).unwrap();
        assert!(host.approval(&a.id).unwrap().decided.is_none());
        host.lock().approvals[0].asked -= 5;
        let d = host.approval(&a.id).unwrap().decided.unwrap();
        assert_eq!((d.decision, d.by.as_str()), (Decision::Deny, "timeout"));
    }

    #[test]
    fn a_worker_posts_on_its_thread_and_its_child_reads_its_inbox() {
        let mut s = state();
        let w = s.add_session("claude", "a worker");
        let id = s.engines[w].id.clone();
        let c = s.add_engine();
        s.engines[c].parent = Some(id.clone());
        let child = s.engines[c].id.clone();
        let host = Host::in_memory(s);
        let (line, _) = host.post(&id, "try turn.rs", Some(&child)).unwrap();
        assert!(line.starts_with("posted entry 1") && line.ends_with("on your thread"), "{line}");
        let (line, _) = host.post(&id, "no to", None).unwrap();
        assert!(line.contains("on your thread; it has no to") && line.ends_with("to=you"), "{line}");
        let snap = host.snapshot().state;
        assert!(snap.shared[&id].contains(&format!("{id} → {child}: try turn.rs")));
        assert!(snap.shared[&id].contains(&format!("{id}: no to")));
        // The child's next turn sees what was addressed to it.
        let (text, cursor) = host.read(&child, &dry_core::snapshot::Read::Turn(Default::default())).unwrap();
        assert!(text.contains("try turn.rs"), "{text}");
        assert_eq!(cursor.unwrap().n, 2);
        assert_eq!(host.post(&id, " ", None).unwrap_err().status, 400);
        assert_eq!(host.post("ghost", "x", None).unwrap_err().status, 404);
        // A take before any reply: nothing yet; a stranger is refused.
        assert_eq!(host.take(&id, &child, 0).unwrap(), json!({ "busy": false }));
        assert_eq!(host.take(&child, &id, 0).unwrap_err().status, 404);
    }

    #[test]
    fn answers_from_what_a_page_gathered() {
        let choice = Ask::Choice { options: vec!["Red".into(), "Blue".into()] };
        assert_eq!(answer_text(&choice, &json!({ "input": { "picked": ["Blue"] } })).unwrap(), "Answer (choice): Blue");
        assert!(answer_text(&choice, &json!({ "input": { "picked": ["Pink"] } })).is_err());
        let text = Ask::Text { placeholder: String::new() };
        assert_eq!(answer_text(&text, &json!({ "input": { "typed": " hi " } })).unwrap(), "Answer (text): hi");
    }
}
