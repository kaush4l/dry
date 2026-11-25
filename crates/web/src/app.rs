//! The app in the page: one `App` (the world the renderer draws, this
//! browser's settings, what the store holds), drawn into three regions
//! (`#tokens`, `#nav`, `#pane`) after every change, with one listener per
//! event kind on the document (event delegation: every control says what
//! it does in `data-act`).
//!
//! A turn is `dry_core::runner::Runner` fed by `fetch`: each move borrows
//! the app only while the core decides, so the page draws and takes
//! clicks while the model thinks. Children an engine asks run their own
//! turns at the same time; their replies call back through
//! `turn::callbacks`, as in the window.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dry_core::activity::Activity;
use dry_core::ask::{Answer, Ask};
use dry_core::card::Status;
use dry_core::context::Window;
use dry_core::engines::Seen;
use dry_core::inference::{Inference, Message, Role};
use dry_core::live::Time;
use dry_core::runner::{Came, Cx, Next, Runner};
use dry_core::state::State;
use dry_core::store::{self, Saver, TextStore, Texts};
use dry_core::theme::Mode;
use dry_html::world::{Access, Connection, MemoryRepo, Reported, World};
use dry_html::{Query, page, render};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;
use web_sys::{Document, Element, HtmlElement, HtmlInputElement, HtmlTextAreaElement};

use crate::host::Browser;
use crate::idb::Backend;
use crate::net::Fetch;
use crate::settings::{self, Settings};
use crate::{bundle, demo};

/// The fonts shipped with the page (`assets/fonts/`, copied to `fonts/`).
const FONTS: &[&str] = &["Amarante-Regular.ttf"];
/// The page's own look on top of the shared stylesheet.
const WEB_CSS: &str = include_str!("../assets/web.css");

struct App {
    world: World,
    settings: Settings,
    saver: Saver,
    location: String,
    query: Query,
    /// Each model's window once a server said it, by `base model`.
    windows: BTreeMap<String, Window>,
    /// Nothing was stored yet: offer the demo.
    first_run: bool,
    saving: bool,
    dirty: bool,
    /// Tasks open across turns (`dry_core::spans`): each turn's spans go
    /// through it to the engine's `spans.jsonl` in the store.
    tasks: dry_core::spans::Tasks,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static STORE: RefCell<Option<Rc<Backend>>> = const { RefCell::new(None) };
}

/// Run `f` on the app (`None` before it is loaded).
fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|c| c.borrow_mut().as_mut().map(f))
}

fn doc() -> Document {
    web_sys::window().and_then(|w| w.document()).expect("a document")
}

fn by_id(id: &str) -> Option<Element> {
    doc().get_element_by_id(id)
}

fn log(text: &str) {
    web_sys::console::log_1(&JsValue::from_str(text));
}

/// A short line at the bottom of the page for a few seconds.
fn toast(text: &str) {
    if let Some(t) = by_id("toast") {
        t.set_text_content(Some(text));
        let _ = t.class_list().add_1("show");
        let cb = Closure::once_into_js(move || {
            if let Some(t) = by_id("toast") {
                let _ = t.class_list().remove_1("show");
            }
        });
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 5000);
        }
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&JsValue::from_str(&format!("dry panicked: {info}")))));
    // The landing page loads this module only for its scene (`scene::landing`).
    if by_id("pane").is_none() {
        return;
    }
    spawn_local(boot());
}

async fn boot() {
    let (backend, note) = Backend::open().await;
    let (mut state, saver, error) = match store::load(&backend).await {
        Ok((state, saver)) => (state, saver, note),
        Err(e) => (None, Saver::default(), Some(format!("the stored state does not read: {e}"))),
    };
    // A workspace folder's snapshot (`.dry/snapshot.json`): what the state lost comes back.
    if let Backend::Companion(s) = &backend
        && let Some(snap) = crate::companion::workspace_snapshot(s).await
        && let Some(st) = state.as_mut()
    {
        for line in snap.restore(st) {
            log(&format!("restored from the folder's snapshot: {line}"));
        }
    }
    let settings = saver.last.get(settings::PATH).map(|t| Settings::from_json(t)).unwrap_or_default();
    let first_run = state.is_none();
    // Nothing stored yet: the shipped default, the translator (docs/default.md).
    let state = state.unwrap_or_else(|| dry_core::shipped::with_model(&settings.inference()).unwrap_or_default());
    let files = bundle::files();
    let mut world = World::from_files(&files, state, "", settings.theme.as_deref());
    world.host = Box::new(crate::companion::shell());
    world.clock = dry_core::live::Time::clock(&Browser);
    world.repo = Box::new(MemoryRepo(files));
    world.repo_name = "dry (bundled files)".into();
    world.access = Access::Live;
    world.fonts = FONTS.iter().map(|f| f.to_string()).collect();
    world.served_by = format!("dry-web {} · WebAssembly in this page · files bundled at build time", env!("CARGO_PKG_VERSION"));
    world.store = backend.describe();
    world.store_error = error;
    world.sessions = Err("sessions run programs on a computer: not in the browser".into());
    STORE.with(|s| *s.borrow_mut() = Some(Rc::new(backend)));
    let (location, query) = route();
    let location = follow_moved(&world, location);
    APP.with(|c| {
        *c.borrow_mut() =
            Some(App { world, settings, saver, location, query, windows: BTreeMap::new(), first_run, saving: false, dirty: false, tasks: Default::default() })
    });
    load_spans();
    with(|a| a.sync_models());
    if let Some(css) = by_id("app-css") {
        let fonts: Vec<String> = FONTS.iter().map(|f| f.to_string()).collect();
        css.set_text_content(Some(&format!("{}\n{WEB_CSS}", dry_html::css::stylesheet(&fonts, "fonts/"))));
    }
    if let Some(b) = by_id("backdrop") {
        b.set_outer_html(&page::backdrop());
    }
    listen();
    crate::vm::init();
    crate::caps::init();
    crate::companion::init();
    // The environment: the VM boots (or comes back from IndexedDB) behind the page.
    crate::env::init();
    render();
    // No model chosen yet (a first visit): look for a model server.
    if with(|a| a.settings.model.is_empty()).unwrap_or(false) {
        spawn_local(find_model_server());
    }
    // The callbacks' tick, as the window's: children that replied.
    let tick = Closure::<dyn FnMut()>::new(settle);
    if let Some(w) = web_sys::window() {
        let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(tick.as_ref().unchecked_ref(), 2000);
    }
    tick.forget();
}

impl App {
    /// The settings into what the renderer shows (Settings → Models).
    fn sync_models(&mut self) {
        let old = self.world.connection.take().unwrap_or_default();
        self.world.models = dry_core::models::Models::from_json(&self.settings.models_json());
        self.world.connection = Some(Connection {
            endpoint: self.settings.endpoint.clone(),
            model: self.settings.model.clone(),
            has_key: self.settings.key.is_some(),
            detected: old.detected,
            check: old.check,
        });
        let mut reg = (*self.world.state.capabilities.0).clone();
        reg.subscriptions = Some(self.subscriptions());
        self.world.state.equip(reg);
    }

    /// The subscriptions as the page can tell (docs/elements.md): the
    /// page's model present once Settings → Models reached its server
    /// (else not known: a server across origins is not asked); a CLI needs
    /// a host or a companion; sessions run programs, so none here.
    fn subscriptions(&self) -> dry_core::subscriptions::Subscriptions {
        use dry_core::subscriptions::{Check, Presence};
        let reached = self.world.connection.as_ref().filter(|c| matches!(c.detected, Some(Ok(_)))).map(|c| c.endpoint.clone());
        let mut probe = |c: &Check| match c {
            Check::Program(_) => Presence::Absent("needs a host or companion".into()),
            Check::Local(base) if reached.as_deref() == Some(base.trim_end_matches('/')) => Presence::Present,
            Check::Local(_) => Presence::Unknown("not reached from this page yet (Settings → Models, Detect)".into()),
            Check::Key(_) => Presence::Unknown("a key variable is on a host; the page cannot look".into()),
            Check::Nothing => Presence::Present,
            Check::Broken(why) => Presence::Absent(why.clone()),
        };
        dry_core::subscriptions::build(self.world.models.clone(), self.world.sessions.clone(), &mut probe)
    }

    fn local_texts(&self) -> Texts {
        let mut t = Texts::new();
        t.insert(settings::PATH.into(), self.settings.to_json());
        t
    }

    fn busy(&self, i: usize) -> bool {
        self.world.reported.activity.get(&i).is_some_and(Activity::is_turn)
    }

    /// Engines and their reports are by index: after one is removed they
    /// go (the spans are by engine id: they stay).
    fn forget_reports(&mut self) {
        let spans = std::mem::take(&mut self.world.reported.spans);
        let daily = std::mem::take(&mut self.world.reported.daily);
        self.world.reported = Reported { spans, daily, ..Reported::default() };
    }
}

/// How engine `j` looks to its parent's tools.
fn seen_of(state: &State, reported: &Reported, j: usize) -> Seen {
    let a = state
        .engines
        .get(j)
        .map_or(Activity::Idle, |e| dry_core::dashboard::live_activity(e, reported.status.get(&j), reported.activity.get(&j)));
    let busy = reported.activity.get(&j).is_some_and(Activity::is_turn) || state.engines.get(j).is_some_and(|e| !e.waiting.is_empty());
    let failed = match reported.status.get(&j) {
        Some(Status::Failed(why) | Status::Missing(why)) => Some(why.clone()),
        _ => None,
    };
    Seen { label: a.label(), busy, failed }
}

fn window_key(ai: &dry_core::inference::OpenAi) -> String {
    format!("{} {}", ai.base_url, ai.model)
}

// ---- saving ----

/// Save what changed, in the background; saves never overlap.
fn save() {
    let go = with(|a| {
        if a.saving {
            a.dirty = true;
            false
        } else {
            a.saving = true;
            true
        }
    });
    if go != Some(true) {
        return;
    }
    spawn_local(async {
        let Some(store) = STORE.with(|s| s.borrow().clone()) else { return };
        loop {
            let Some(changes) = with(|a| a.saver.changes(&a.world.state, &a.local_texts())) else { return };
            if changes.is_empty() {
                with(|a| a.saving = false);
                return;
            }
            let done = store.write(&changes).await;
            let again = with(|a| {
                match done {
                    Ok(()) => {
                        a.saver.saved(&changes);
                        if let Backend::Companion(s) = &*store {
                            crate::companion::save_snapshot(s, &a.world.state);
                        }
                    }
                    Err(e) => a.world.store_error = Some(format!("not saved: {e}")),
                }
                let again = a.dirty;
                a.dirty = false;
                if !again {
                    a.saving = false;
                }
                again
            });
            if again != Some(true) {
                return;
            }
        }
    });
}

// ---- spans (`dry_core::spans`, docs/analytics.md) ----

/// Unix milliseconds now.
fn wall_ms() -> i64 {
    js_sys::Date::now() as i64
}

/// The logs the store holds (`engines/<id>/spans.jsonl`), into the
/// world; days past the retention rolled into daily totals and written back.
fn load_spans() {
    let changes = with(|a| {
        let offset = a.world.clock.offset;
        let mut put = Texts::new();
        let ids: Vec<String> = a
            .saver
            .last
            .keys()
            .filter_map(|p| p.strip_prefix("engines/").and_then(|r| r.strip_suffix(&format!("/{}", dry_core::spans::FILE))))
            .map(str::to_string)
            .collect();
        for id in ids {
            let text = a.saver.last.get(&dry_core::spans::path(&id)).cloned().unwrap_or_default();
            let daily = a.saver.last.get(&dry_core::spans::daily_path(&id)).map_or(Vec::new(), |t| dry_core::spans::parse_daily(t));
            let r = dry_core::spans::rollup(dry_core::spans::parse(&text), daily, wall_ms(), offset, dry_core::spans::KEEP_DAYS);
            if r.rolled > 0 {
                put.insert(dry_core::spans::path(&id), dry_core::spans::to_text(&r.kept));
                put.insert(dry_core::spans::daily_path(&id), dry_core::spans::daily_text(&r.daily));
            }
            a.world.reported.daily.extend(r.daily);
            a.world.reported.spans.insert(id, r.kept);
        }
        store::Changes { put, remove: Vec::new() }
    });
    if let Some(c) = changes.filter(|c| !c.is_empty()) {
        write_logs(c);
    }
}

/// A turn's spans: under their tasks, kept for the views, and each
/// engine's log written whole to the store (IndexedDB has no append).
fn record_spans(a: &mut App, mut spans: Vec<dry_core::spans::Span>) -> store::Changes {
    let mut put = Texts::new();
    if spans.is_empty() {
        return store::Changes::default();
    }
    for e in &a.world.state.engines {
        a.tasks.set_parent(&e.id, e.parent.as_deref());
    }
    a.tasks.adopt(&mut spans);
    let mut touched = std::collections::BTreeSet::new();
    for s in spans {
        touched.insert(s.engine.clone());
        a.world.reported.spans.entry(s.engine.clone()).or_default().push(s);
    }
    for id in touched {
        let text = dry_core::spans::to_text(&a.world.reported.spans[&id]);
        put.insert(dry_core::spans::path(&id), text);
    }
    store::Changes { put, remove: Vec::new() }
}

fn write_logs(changes: store::Changes) {
    if changes.is_empty() {
        return;
    }
    spawn_local(async move {
        let Some(store) = STORE.with(|s| s.borrow().clone()) else { return };
        if let Err(e) = store.write(&changes).await {
            log(&format!("spans not saved: {e}"));
        }
    });
}

// ---- turns ----

fn start_turn(i: usize, resume: bool) {
    if let Some(id) = with(|a| a.world.state.engines.get(i).map(|e| e.id.clone())).flatten() {
        spawn_local(run_turn(id, resume));
    }
}

/// One engine's turn, to its end: the core decides each move, this page
/// does its I/O (`runner::io`: `fetch` for the model, refusals for files,
/// commands and MCP).
async fn run_turn(id: String, resume: bool) {
    // The companion while one is connected (files and terminals work), else the browser.
    let host = crate::companion::shell();
    let made = with(|a| {
        let i = a.world.state.index_of(&id)?;
        if a.busy(i) {
            return None;
        }
        let w = a.windows.get(&window_key(&a.world.state.engines[i].inference)).cloned();
        let r = if resume { Runner::resume(&a.world.state, i, w) } else { Runner::start(&a.world.state, i, w) };
        a.world.reported.status.remove(&i);
        match r {
            Ok(r) => {
                a.world.reported.activity.insert(i, r.activity.clone());
                Some(Ok(r))
            }
            Err(status) => {
                a.world.reported.activity.remove(&i);
                a.world.reported.status.insert(i, status);
                Some(Err(()))
            }
        }
    })
    .flatten();
    let mut runner = match made {
        Some(Ok(r)) => r,
        Some(Err(())) => return render(),
        None => return,
    };
    render();
    let fetch = Fetch { key: with(|a| a.settings.key.clone()).flatten() };
    let mut came = Came::Start;
    loop {
        let step = with(|a| {
            let next = {
                let World { state, reported, .. } = &mut a.world;
                let seen = |s: &State, j: usize| seen_of(s, reported, j);
                let cx = Cx { host: &host, clock: host.clock(), seen: &seen, external: &crate::companion::external };
                runner.next_at(state, &cx, came.clone(), wall_ms())
            };
            let spans = runner.spans();
            let logs = record_spans(a, spans);
            let i = a.world.state.index_of(&runner.id);
            if let Some(i) = i {
                match &next {
                    Next::Done(status) => {
                        a.world.reported.activity.remove(&i);
                        a.world.reported.status.insert(i, status.clone());
                    }
                    _ => {
                        a.world.reported.activity.insert(i, runner.activity.clone());
                    }
                }
                if let Some(m) = &runner.measured {
                    a.world.reported.measured.insert(i, m.clone());
                }
                if let Some(w) = &runner.window {
                    a.windows.insert(window_key(&a.world.state.engines[i].inference), w.clone());
                }
            }
            for note in runner.notes.drain(..) {
                log(&format!("{}: {note}", runner.id));
            }
            let children: Vec<String> =
                runner.spawned.drain(..).filter_map(|c| a.world.state.engines.get(c).map(|e| e.id.clone())).collect();
            (next, children, logs)
        });
        let Some((next, children, logs)) = step else { return };
        write_logs(logs);
        for child in children {
            spawn_local(run_turn(child, false));
        }
        save();
        render();
        match next {
            Next::Done(_) | Next::Waiting(_) => {
                settle();
                return;
            }
            Next::Post(_) if runner.streams() => came = stream_answer(&mut runner, &next, &fetch, &host).await,
            _ => came = crate::companion::io(&next, &fetch, &host, &crate::companion::external).await,
        }
    }
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

/// The model's answer to a step, streamed (`Transport::stream`): the
/// reply drawn as it grows (`Reported::streaming`, at most every 40 ms),
/// its component as soon as `ask=`/`show=` closes, and each instant call
/// run as its `)` arrives (`Runner::early_call`, kept only if the whole
/// answer reads with the same calls). A server that does not stream
/// gives the whole body at once, as before.
async fn stream_answer(runner: &mut Runner, next: &Next, fetch: &Fetch, host: &crate::companion::Shell) -> Came {
    use dry_core::runner::Transport;
    use dry_core::stream::{Delta, Event, Reader};
    let Next::Post(req) = next else { return Came::Body(Err("not a post".into())) };
    let id = runner.id.clone();
    let Some(mut reader) = with(|a| a.world.state.index_of(&id).map(|i| Reader::new(&a.world.state.engines[i]))).flatten() else {
        return Came::Body(Err("the engine was deleted during its turn".into()));
    };
    let start = now_ms();
    let (mut last, mut first, mut component) = (0.0, None, None);
    let mut on = |d: Delta| {
        let events = reader.feed(&d);
        first.get_or_insert_with(|| now_ms() - start);
        if matches!(d, Delta::Text(_)) {
            runner.first_token(wall_ms());
        }
        if reader.view.has_component() {
            component.get_or_insert_with(|| now_ms() - start);
            runner.first_component(wall_ms());
        }
        with(|a| {
            let Some(i) = a.world.state.index_of(&id) else { return };
            for ev in &events {
                if let Event::CallDone { call, .. } = ev {
                    let World { state, reported, .. } = &mut a.world;
                    let seen = |s: &State, j: usize| seen_of(s, reported, j);
                    let cx = Cx { host, clock: host.clock(), seen: &seen, external: &crate::companion::external };
                    runner.early_call(state, &cx, call);
                }
            }
            a.world.reported.streaming.insert(i, reader.view.clone());
        });
        if now_ms() - last > 40.0 {
            last = now_ms();
            render();
        }
    };
    let body = fetch.stream(&dry_core::stream::streaming(req), &mut on).await;
    reader.finish();
    let ms = |t: Option<f64>| t.map_or("-".to_string(), |t| format!("{t:.0} ms"));
    log(&format!("{id}: streamed: first text {}, component {}, whole answer {:.0} ms ({} chars)", ms(first), ms(component), now_ms() - start, reader.text.chars().count()));
    with(|a| {
        if let Some(i) = a.world.state.index_of(&id) {
            a.world.reported.streaming.remove(&i);
        }
    });
    Came::Body(body)
}

/// Every engine's callbacks (`dry_core::turn::callbacks`): a parent whose
/// children all replied goes on; an `engine_start` child that replied
/// wakes its parent with a message. An engine in a turn is left for later.
fn settle() {
    crate::caps::tick();
    crate::env::tick();
    let host = crate::companion::shell();
    let mut run: Vec<(usize, bool)> = Vec::new();
    let changed = with(|a| {
        let mut changed = false;
        for i in 0..a.world.state.engines.len() {
            let busy = a.busy(i);
            let seen: Vec<Seen> = (0..a.world.state.engines.len()).map(|j| seen_of(&a.world.state, &a.world.reported, j)).collect();
            let settled = dry_core::turn::callbacks(&mut a.world.state, i, busy, &host, &|j| seen[j].clone());
            if settled.resumed {
                run.push((i, true));
                changed = true;
            } else if !settled.wake.is_empty() && a.world.state.add_wake(i, &settled.wake.join("\n")) {
                run.push((i, false));
                changed = true;
            }
        }
        changed
    });
    // A live view on screen while work goes on: drawn again on the tick,
    // so a started command's output moves and a file shows as it is saved
    // (the window does the same, `AppView::refresh_live`).
    let live = with(|a| a.location.ends_with("/live") && ((0..a.world.state.engines.len()).any(|i| a.busy(i)) || crate::env::running()));
    if changed == Some(true) {
        save();
        render();
    } else if live == Some(true) {
        render();
    }
    for (i, resume) in run {
        start_turn(i, resume);
    }
}

// ---- drawing ----

/// The route in the address: `#/desk/0?open=a` → (`/desk/0`, its query).
fn route() -> (String, Query) {
    let hash = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
    let h = hash.trim_start_matches('#');
    let (path, q) = h.split_once('?').unwrap_or((h, ""));
    let path = if path.is_empty() { "/".to_string() } else { render::decode_path(path) };
    (path, Query::parse(q))
}

/// A moved route's old address (`#/engines/2`) swapped in place for where
/// it opens now (`#/desk/2`), so old links keep working.
fn follow_moved(world: &World, location: String) -> String {
    let Some(to) = world.table.moved_to(&location) else { return location };
    if let Some(w) = web_sys::window() {
        let hash = w.location().hash().unwrap_or_default();
        let q = hash.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
        let _ = w.location().replace(&format!("#{to}{q}"));
    }
    to
}

fn go(path: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_hash(path);
    }
}

/// In-app links as hash routes: GitHub Pages has no server rewrites.
fn hash_links(html: &str) -> String {
    html.replace("href=\"/", "href=\"#/")
}

fn all(selector: &str) -> Vec<Element> {
    let Ok(list) = doc().query_selector_all(selector) else { return Vec::new() };
    (0..list.length()).filter_map(|i| list.get(i)?.dyn_into::<Element>().ok()).collect()
}

/// Draw the page for the route, keeping what the person had open, typed
/// and scrolled.
fn render() {
    let d = doc();
    let folds: Vec<(String, bool)> = all("details[data-key]")
        .into_iter()
        .filter_map(|e| Some((e.get_attribute("data-key")?, e.dyn_into::<web_sys::HtmlDetailsElement>().ok()?.open())))
        .collect();
    let kept: Vec<(String, String)> = all("[data-keep]")
        .into_iter()
        .filter_map(|e| Some((e.get_attribute("data-keep")?, e.dyn_into::<HtmlTextAreaElement>().ok()?.value())))
        .collect();
    // Each Settings → Models field: its name, what is typed in it, and the
    // value the app drew it with (`settings::kept_field`).
    let fields: Vec<(String, String, Option<String>)> = all("#connection input[name]")
        .into_iter()
        .filter_map(|e| {
            let i = e.dyn_into::<HtmlInputElement>().ok()?;
            Some((i.name(), i.value(), i.get_attribute("value")))
        })
        .collect();
    let focused =
        d.active_element().and_then(|e| e.get_attribute("data-keep").or_else(|| e.get_attribute("name").map(|n| format!("name:{n}"))));
    let scroll = by_id("main").map_or(0, |m| m.scroll_top());

    let drawn = with(|a| {
        a.world.clock = dry_core::live::Time::clock(&Browser);
        let p = page::parts(&a.world, &a.location, &a.query);
        // The scene setting's scene, in the colours it has with the theme and mode on screen.
        let scene = page::theme_of(&a.world, &a.location, &a.query).ok().map(|(th, m)| {
            let look = page::scene_of(&a.world, &a.location, &th);
            (page::scene_named(&a.world, &look).cloned(), dry_core::theme::resolve(m, &th), look.still())
        });
        // The scene's mood: Home's tiles' rule over every engine's live status.
        let r = &a.world.reported;
        let statuses: Vec<Status> = a
            .world
            .state
            .engines
            .iter()
            .enumerate()
            .map(|(j, e)| dry_core::dashboard::live_status(e, r.status.get(&j), r.activity.get(&j)))
            .collect();
        (p, a.first_run, scene, dry_core::scene::mood(&statuses))
    });
    let Some((parts, first_run, scene, mood)) = drawn else { return };
    crate::scene::set_mood(mood);
    if let Some((scene, t, still)) = scene {
        crate::scene::show("scene", scene, &t, still);
    }
    let parts = match parts {
        Ok(p) => p,
        Err(e) => {
            if let Some(b) = d.body() {
                b.set_inner_html(&page::broken(&e));
            }
            return;
        }
    };
    if let Some(html) = d.document_element() {
        let _ = html.set_attribute("data-mode", parts.mode);
        let open = html.class_list().contains("nav-open");
        html.set_class_name(parts.nav_class);
        if open {
            let _ = html.class_list().add_1("nav-open");
        }
    }
    d.set_title(&format!("{} · dry", parts.title));
    if let Some(t) = by_id("tokens") {
        t.set_text_content(Some(&parts.tokens));
    }
    if let Some(n) = by_id("nav") {
        n.set_outer_html(&hash_links(&parts.nav));
    }
    if let Some(p) = by_id("pane") {
        p.set_outer_html(&hash_links(&parts.pane));
    }
    // The container route draws itself into the pane (docs/container.md).
    crate::vm::nav_link();
    crate::vm::draw();
    // The companion's Projects link, Settings panel and #/projects view (docs/companion.md).
    crate::companion::decorate();
    for e in all("details[data-key]") {
        if let (Some(k), Ok(det)) = (e.get_attribute("data-key"), e.clone().dyn_into::<web_sys::HtmlDetailsElement>())
            && let Some((_, open)) = folds.iter().find(|(f, _)| *f == k)
        {
            det.set_open(*open);
        }
    }
    for e in all("[data-keep]") {
        if let (Some(k), Ok(t)) = (e.get_attribute("data-keep"), e.clone().dyn_into::<HtmlTextAreaElement>())
            && let Some((_, v)) = kept.iter().find(|(f, _)| *f == k)
        {
            t.set_value(v);
        }
    }
    for e in all("#connection input[name]") {
        if let Ok(i) = e.dyn_into::<HtmlInputElement>()
            && let Some((_, typed, drawn)) = fields.iter().find(|(n, _, _)| *n == i.name())
            && let Some(v) = settings::kept_field(typed, drawn.as_deref(), i.get_attribute("value").as_deref())
        {
            i.set_value(v);
        }
    }
    if let Some(f) = focused {
        let sel = match f.strip_prefix("name:") {
            Some(n) => format!("[name=\"{n}\"]"),
            None => format!("[data-keep=\"{f}\"]"),
        };
        if let Ok(Some(e)) = d.query_selector(&sel)
            && let Ok(h) = e.dyn_into::<HtmlElement>()
        {
            let _ = h.focus();
        }
    }
    if let Some(m) = by_id("main") {
        m.set_scroll_top(scroll);
    }
    if let (Some(v), Some(w)) = (d.query_selector("[data-viewport]").ok().flatten(), web_sys::window()) {
        let (iw, ih) =
            (w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0), w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0));
        v.set_text_content(Some(&format!("viewport: {iw}×{ih} px · device pixel ratio {}", w.device_pixel_ratio())));
    }
    welcome(first_run);
}

/// On a first visit (nothing stored): start empty, or load the demo.
fn welcome(show: bool) {
    let d = doc();
    let there = by_id("welcome");
    match (show, there) {
        (false, Some(w)) => w.remove(),
        (true, None) => {
            if let (Ok(w), Some(body)) = (d.create_element("div"), d.body()) {
                w.set_id("welcome");
                w.set_inner_html(&format!(
                    "<div class=\"glass panel\"><h2>dry, in this browser</h2>\
                     <p class=\"note\">The whole app runs in this page: engines, their turns, the asks. Nothing is sent anywhere but the model server you set; everything is kept in this browser ({}).</p>\
                     <div class=\"row\"><button class=\"btn primary\" type=\"button\" data-act=\"load-demo\">Load demo</button><button class=\"btn\" type=\"button\" data-act=\"start-empty\">Start with the translator</button></div>\
                     <p class=\"note\">The translator is dry's default engine: it turns what you ask into a brief for a developer engine and judges what comes back (docs/default.md). The demo shows engines with a history, artifacts, a child engine and asks, with no model server needed. When it opens, the page looks for a model server on this computer (LM Studio, oMLX, Ollama) and uses the one with a model loaded; Home's Model server card says which. None found: start one with CORS on, then Settings → Models → Detect.</p></div>",
                    with(|a| a.world.store.clone()).unwrap_or_default()
                ));
                let _ = body.append_child(&w);
            }
        }
        _ => {}
    }
}

// ---- events ----

fn listen() {
    let d = doc();
    let click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(|e: web_sys::MouseEvent| {
        let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
        if t.closest("[data-nav-toggle]").ok().flatten().is_some() {
            if let Some(html) = doc().document_element() {
                let open = html.class_list().toggle("nav-open").unwrap_or(false);
                if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
                    let _ = s.set_item("dry.nav", if open { "open" } else { "closed" });
                }
            }
            return;
        }
        if let Ok(Some(b)) = t.closest("[data-act]") {
            e.prevent_default();
            act(&b);
        }
    });
    let _ = d.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
    click.forget();

    let submit = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        if let Some(f) = e.target().and_then(|t| t.dyn_into::<Element>().ok()).filter(|f| f.has_attribute("data-composer")) {
            e.prevent_default();
            send(&f);
        }
    });
    let _ = d.add_event_listener_with_callback("submit", submit.as_ref().unchecked_ref());
    submit.forget();

    let key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        if e.key() != "Enter" || e.shift_key() || e.is_composing() {
            return;
        }
        let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
        if let Ok(Some(f)) = t.closest("form[data-composer]")
            && t.tag_name() == "TEXTAREA"
        {
            e.prevent_default();
            return send(&f);
        }
        if let Ok(Some(bx)) = t.closest(".ask")
            && t.tag_name() == "INPUT"
            && let Ok(Some(b)) = bx.query_selector("[data-act=\"ask\"]")
        {
            e.prevent_default();
            return answer(&b);
        }
        if t.closest("#connection").ok().flatten().is_some() && t.tag_name() == "INPUT" {
            e.prevent_default();
            connection_act("save-connection", None);
        }
    });
    let _ = d.add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref());
    key.forget();

    let hash = Closure::<dyn FnMut()>::new(|| {
        let (location, query) = route();
        with(|a| {
            a.location = follow_moved(&a.world, location);
            a.query = query;
        });
        render();
        if let Some(m) = by_id("main") {
            m.set_scroll_top(0);
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("hashchange", hash.as_ref().unchecked_ref());
    }
    hash.forget();
}

fn attr(e: &Element, name: &str) -> Option<String> {
    e.get_attribute(name)
}

fn engine_of(e: &Element) -> Option<usize> {
    attr(e, "data-engine").and_then(|v| v.parse().ok())
}

/// A control was pressed.
fn act(b: &Element) {
    let what = attr(b, "data-act").unwrap_or_default();
    let value = attr(b, "data-value").unwrap_or_default();
    match what.as_str() {
        "send" => {
            if let Some(f) = b.closest("form[data-composer]").ok().flatten() {
                send(&f);
            }
        }
        // A capability that needs a person's gesture: this click is it.
        "grant" => crate::caps::grant(&value),
        "voice" => {
            if let Some(t) = b.closest("form[data-composer]").ok().flatten().and_then(|f| f.query_selector("textarea").ok().flatten())
                && let Ok(t) = t.dyn_into::<HtmlTextAreaElement>()
            {
                crate::caps::voice(t);
            }
        }
        "clear" => {
            if let Some(t) = b.closest("form[data-composer]").ok().flatten().and_then(|f| f.query_selector("textarea").ok().flatten())
                && let Ok(t) = t.dyn_into::<HtmlTextAreaElement>()
            {
                t.set_value("");
            }
        }
        "set-format" => {
            let Some(i) = engine_of(b) else { return };
            with(|a| {
                if let Some(e) = a.world.state.engines.get_mut(i) {
                    use dry_core::response::Format;
                    e.format = Format::parse_name(&value).unwrap_or_else(|| e.format.clone());
                }
            });
            changed();
        }
        "reload-responses" => {
            let Some(i) = engine_of(b) else { return };
            match bundle::responses() {
                Ok((r, f)) => {
                    with(|a| {
                        if let Some(e) = a.world.state.engines.get_mut(i) {
                            (e.response, e.format) = (r, f);
                        }
                    });
                    changed();
                }
                Err(e) => toast(&e),
            }
        }
        "delete-engine" => {
            let Some(i) = engine_of(b) else { return };
            let name = with(|a| a.world.state.engines.get(i).map(|e| e.name.clone())).flatten().unwrap_or_default();
            let sure = web_sys::window()
                .and_then(|w| w.confirm_with_message(&format!("Delete {name} and its child engines? This cannot be undone.")).ok());
            if sure != Some(true) {
                return;
            }
            with(|a| {
                a.world.state.remove_engine(i);
                a.forget_reports();
            });
            save();
            go("#/desk");
        }
        // A new thread: a root from the shipped translator, its thread open
        // with the composer on it. `new-engine` is its old name.
        "new-thread" | "new-engine" => match with(new_thread) {
            Some(Ok(id)) => {
                save();
                go(&format!("#/desk?thread={id}&to={id}"));
                render();
                focus_thread();
            }
            Some(Err(why)) => toast(&why),
            None => {}
        },
        "ask" => answer(b),
        "ask-toggle" => {
            let on = b.class_list().toggle("on").unwrap_or(false);
            b.set_text_content(Some(&format!("{} {value}", if on { "☑" } else { "☐" })));
            // In a form the one Submit keeps its label.
            if let Some(bx) = b.closest(".ask").ok().flatten().filter(|_| b.closest(".ask-field").ok().flatten().is_none()) {
                let n = bx.query_selector_all("[data-act=\"ask-toggle\"].on").map_or(0, |l| l.length());
                if let Ok(Some(send)) = bx.query_selector("[data-act=\"ask\"]") {
                    send.set_text_content(Some(&format!("Send ({n})")));
                }
            }
        }
        "ask-pick" => {
            // One pick per kind (a day, a time, an option) in its field, or
            // in the box: marked, and kept on the field for Submit.
            if let Some(bx) = b.closest(".ask-field").ok().flatten().or_else(|| b.closest(".ask").ok().flatten()) {
                let (kind, keep) = [("data-day", "data-picked"), ("data-minute", "data-minute"), ("data-value", "data-chosen")]
                    .into_iter()
                    .find(|(k, _)| b.has_attribute(k))
                    .unwrap_or(("data-day", "data-picked"));
                for e in bx
                    .query_selector_all(&format!(".sel[{kind}]"))
                    .map(|l| (0..l.length()).filter_map(|i| l.get(i)?.dyn_into::<Element>().ok()).collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    let _ = e.class_list().remove_1("sel");
                }
                let _ = b.class_list().add_1("sel");
                let _ = bx.set_attribute(keep, &attr(b, kind).unwrap_or_default());
            }
        }
        "ask-step" => {
            let step: f64 = attr(b, "data-step").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            if let Some(i) = b
                .closest(".ask-field")
                .ok()
                .flatten()
                .or_else(|| b.closest(".ask").ok().flatten())
                .and_then(|bx| bx.query_selector("input[name=\"number\"]").ok().flatten())
                .and_then(|i| i.dyn_into::<HtmlInputElement>().ok())
            {
                let n: f64 = i.value().trim().parse().unwrap_or(0.0);
                i.set_value(&dry_core::ask::number_text(n + step));
            }
        }
        "ask-edit" => {
            if let Some(bx) = b.closest(".ask").ok().flatten() {
                for e in bx
                    .query_selector_all("input[name=\"row\"]")
                    .map(|l| (0..l.length()).filter_map(|i| l.get(i)?.dyn_into::<HtmlInputElement>().ok()).collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    e.set_read_only(false);
                }
            }
        }
        "set-mode" => {
            if let Some(m) = Mode::parse(&value) {
                with(|a| a.world.state.mode = m);
                changed();
            }
        }
        "set-scene" => {
            with(|a| a.world.state.scene = Some(value.clone()));
            changed();
        }
        "set-theme" => {
            with(|a| {
                if let Some(t) = a.world.named_themes.get(&value).cloned() {
                    a.world.theme = t;
                    a.world.active_theme = value.clone();
                    a.settings.theme = Some(value.clone());
                }
            });
            changed();
        }
        "refresh" => render(),
        // The second press of Reset history (its fold is the confirm step).
        "reset-history" => reset_history(),
        "load-demo" => {
            // On a first visit the translator was only seeded: no need to ask.
            let has = with(|a| !a.first_run && !a.world.state.engines.is_empty()).unwrap_or(false);
            if has
                && web_sys::window().and_then(|w| {
                    w.confirm_with_message("Load the demo in place of the engines kept in this browser? They are removed.").ok()
                }) != Some(true)
            {
                return;
            }
            with(|a| {
                a.world.state = demo::state(&a.settings.inference(), dry_core::live::Time::clock(&Browser).day());
                a.first_run = false;
                a.forget_reports();
            });
            changed();
            go("#/desk");
        }
        "start-empty" => {
            with(|a| a.first_run = false);
            changed();
        }
        "save-connection" | "detect" | "test" | "pick-model" | "forget-key" => connection_act(&what, Some(value)),
        w if w.starts_with("vm-") => crate::vm::act(w, b),
        // The second press of Forget cached VM (its fold is the confirm step).
        "env-forget" => crate::env::forget(),
        w if w.starts_with("companion-") => crate::companion::act(w, b),
        other => log(&format!("no action {other}")),
    }
}

/// Reset history in the browser (docs/default.md, "Reset"): every text
/// of this browser's store but its own settings (`local/…`: the model
/// connection and key) is removed, after a copy goes to the IndexedDB
/// database `dry-reset` (when it opens; localStorage has no room for one).
/// Then the app runs from the shipped default: the translator on this
/// browser's model. Refused while a turn runs or a save is on its way; a
/// companion's store is reset on its machine (`dry reset`, with a backup).
fn reset_history() {
    if with(|a| a.saving || (0..a.world.state.engines.len()).any(|i| a.busy(i))) == Some(true) {
        return toast("An engine is in a turn (or a save is on its way): let it end, then reset. Nothing was removed.");
    }
    let Some(store) = STORE.with(|s| s.borrow().clone()) else { return };
    if matches!(&*store, Backend::Companion(_)) {
        return toast("This tab keeps its state in the paired companion's folder: reset it there with `dry reset`, which keeps a backup. Nothing was removed.");
    }
    spawn_local(async move {
        let all = match store.read_all().await {
            Ok(t) => t,
            Err(e) => return toast(&format!("not reset: the store does not read ({e})")),
        };
        let remove = store::reset_removes(&all);
        let stamp = store::reset_backup_name("dry", wall_ms().div_euclid(1000));
        let copy = if remove.is_empty() {
            None
        } else {
            match crate::idb::Idb::open_named(crate::idb::RESET_DB).await {
                Ok(db) => {
                    let put = remove.iter().filter_map(|k| Some((format!("{stamp}/{k}"), all.get(k)?.clone()))).collect();
                    match db.write(&store::Changes { put, remove: Vec::new() }).await {
                        Ok(()) => Some(format!("a copy is in IndexedDB {} under {stamp}/", crate::idb::RESET_DB)),
                        Err(e) => return toast(&format!("not reset: the copy could not be kept ({e}); nothing was removed")),
                    }
                }
                Err(_) => None,
            }
        };
        if !remove.is_empty()
            && let Err(e) = store.write(&store::Changes { put: Texts::new(), remove: remove.clone() }).await
        {
            return toast(&format!("not reset: {e}"));
        }
        with(|a| {
            a.saver.last.retain(|k, _| k.starts_with(store::LOCAL));
            a.world.state = dry_core::shipped::with_model(&a.settings.inference()).unwrap_or_default();
            a.world.reported = Reported::default();
            a.tasks = Default::default();
            a.first_run = false;
        });
        changed();
        go("#/desk");
        toast(&match (remove.is_empty(), copy) {
            (true, _) => "Nothing to reset: this browser held only its settings.".to_string(),
            (false, Some(c)) => format!("History reset: {} texts cleared; {c}.", remove.len()),
            (false, None) => format!("History reset: {} texts cleared (no copy: IndexedDB is not there).", remove.len()),
        });
    });
}

/// Something changed: save and draw.
fn changed() {
    save();
    render();
}

/// A new thread on this browser's model (`dry_core::shipped::new_thread`
/// from the bundled default): its id.
fn new_thread(a: &mut App) -> Result<String, String> {
    let ai = a.settings.inference();
    let i = dry_core::shipped::new_thread(&mut a.world.state, &dry_core::shipped::texts(), &ai)?;
    Ok(a.world.state.engines[i].id.clone())
}

/// Give the Desk's composer the keyboard (after a new thread opened).
fn focus_thread() {
    if let Some(t) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("form[data-thread] textarea").ok().flatten())
        .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
    {
        let _ = t.focus();
    }
}

/// A new model engine on this browser's model, with the bundled contract
/// (a send with no engine yet, an attach).
fn new_engine(a: &mut App) -> usize {
    let i = a.world.state.add_engine();
    let e = &mut a.world.state.engines[i];
    e.inference = a.settings.inference();
    if let Ok((r, f)) = bundle::responses() {
        (e.response, e.format) = (r, f);
    }
    i
}

/// Send the composer's text to its engine (or a new one).
fn send(form: &Element) {
    let Some(t) = form.query_selector("textarea").ok().flatten().and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()) else { return };
    let text = t.value();
    if text.trim().is_empty() {
        return;
    }
    // The Desk's composer: a guided post on its thread to the picked
    // engine; the callbacks then start that engine's turn (`dry_core::desk`).
    if let Some(thread) = attr(form, "data-thread") {
        let to = form
            .query_selector("select[name=\"to\"]")
            .ok()
            .flatten()
            .and_then(|s| js_sys::Reflect::get(s.as_ref(), &"value".into()).ok())
            .and_then(|v| v.as_string())
            .filter(|v| !v.is_empty());
        let now = (js_sys::Date::now() / 1000.0) as i64;
        let posted = with(|a| {
            let (root, edit) = dry_core::desk::your_post(&a.world.state, &thread, to.as_deref(), &text, now)?;
            dry_core::shared::post(&mut a.world.state, &root, &edit)?;
            Ok::<(), String>(())
        });
        match posted {
            Some(Ok(())) => {
                t.set_value("");
                save();
                settle_now();
                render();
            }
            Some(Err(why)) => toast(&why),
            None => {}
        }
        return;
    }
    let target = attr(form, "data-engine").unwrap_or_default();
    let sent = with(|a| {
        let (i, made) = match target.parse::<usize>() {
            Ok(i) if i < a.world.state.engines.len() => (i, false),
            _ => (new_engine(a), true),
        };
        if a.busy(i) {
            return Err("A turn is running: wait for it to end, then send.".to_string());
        }
        // An engine made before a model was chosen follows the setting.
        let e = &mut a.world.state.engines[i];
        if e.session.is_none() && e.inference.model.is_empty() {
            e.inference = a.settings.inference();
        }
        a.world.state.add_text(i, &text);
        Ok((i, made))
    });
    match sent {
        Some(Ok((i, made))) => {
            t.set_value("");
            save();
            if made {
                go(&format!("#{}", dry_core::routes::engine_path(i)));
            }
            start_turn(i, false);
        }
        Some(Err(why)) => toast(&why),
        None => {}
    }
}

/// An ask's control: the answer the core writes for it, sent as the
/// person's next message.
fn answer(b: &Element) {
    let Some(bx) = b.closest(".ask").ok().flatten() else { return };
    let (Some(i), Some(msg)) = (engine_of(&bx), attr(&bx, "data-msg").and_then(|m| m.parse::<usize>().ok())) else { return };
    let Some(Some(ask)) = with(|a| a.world.state.engines.get(i).and_then(|e| dry_core::ask::ask_in(e, &e.messages.get(msg)?.text))) else {
        return;
    };
    let value = attr(b, "data-value").unwrap_or_default();
    let input = |name: &str| {
        bx.query_selector(&format!("input[name=\"{name}\"]"))
            .ok()
            .flatten()
            .and_then(|i| i.dyn_into::<HtmlInputElement>().ok())
            .map(|i| i.value())
    };
    let day = |e: &Element, a: &str| attr(e, a).and_then(|d| d.parse::<i64>().ok());
    let today = dry_core::live::Time::clock(&Browser).day();
    let answer = match &ask {
        Ask::Choice { .. } => Answer::Choice(value),
        Ask::Multi { .. } => Answer::Multi(
            bx.query_selector_all("[data-act=\"ask-toggle\"].on")
                .map(|l| (0..l.length()).filter_map(|k| l.get(k)?.dyn_into::<Element>().ok()?.get_attribute("data-value")).collect())
                .unwrap_or_default(),
        ),
        Ask::Confirm => Answer::Confirm(value == "yes"),
        Ask::Date => Answer::Date(day(b, "data-day").unwrap_or(today)),
        Ask::DateTime => {
            let d = day(&bx, "data-picked").or_else(|| day(b, "data-day")).unwrap_or(today);
            Answer::DateTime(d, attr(b, "data-minute").and_then(|m| m.parse().ok()).unwrap_or(9 * 60))
        }
        Ask::Number { .. } => match ask.check_number(&input("number").unwrap_or_default()) {
            Ok(n) => Answer::Number(n),
            Err(why) => return toast(&why),
        },
        Ask::Text { .. } => {
            let t = input("text").unwrap_or_default();
            if t.trim().is_empty() {
                return toast("Type an answer first.");
            }
            Answer::Text(t)
        }
        Ask::Form { .. } => match ask.submit(&form_inputs(&bx)) {
            Ok(text) => return send_answer(i, &text),
            Err(why) => return toast(&why),
        },
        Ask::Review { .. } if value == "reject" => Answer::Review(None),
        Ask::Review { .. } => Answer::Review(Some(
            bx.query_selector_all("input[name=\"row\"]")
                .map(|l| {
                    (0..l.length())
                        .filter_map(|k| {
                            let i = l.get(k)?.dyn_into::<HtmlInputElement>().ok()?;
                            Some((i.get_attribute("data-key")?, i.value()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )),
    };
    send_answer(i, &ask.answer(&answer));
}

/// Send an ask's answer to engine `i` as the person's next message.
fn send_answer(i: usize, text: &str) {
    let ok = with(|a| {
        if a.busy(i) {
            return false;
        }
        a.world.state.add_text(i, text)
    });
    match ok {
        Some(true) => {
            save();
            start_turn(i, false);
        }
        Some(false) => toast("A turn is running: wait for it to end, then answer."),
        None => {}
    }
}

/// A form's fields as the page holds them, one `Input` each, in order:
/// what was picked (`data-chosen`, or the ticked options), the day (a
/// quick pick, or the date field), the time, what was typed.
fn form_inputs(bx: &Element) -> Vec<dry_core::ask::Input> {
    let list = |e: &Element, sel: &str| {
        e.query_selector_all(sel).map(|l| (0..l.length()).filter_map(|k| l.get(k)?.dyn_into::<Element>().ok()).collect::<Vec<_>>()).unwrap_or_default()
    };
    list(bx, ".ask-field")
        .iter()
        .map(|f| {
            let typed = |name: &str| {
                f.query_selector(&format!("input[name=\"{name}\"]")).ok().flatten().and_then(|i| i.dyn_into::<HtmlInputElement>().ok()).map(|i| i.value())
            };
            let mut picked: Vec<String> = attr(f, "data-chosen").into_iter().collect();
            picked.extend(list(f, "[data-act=\"ask-toggle\"].on").iter().filter_map(|e| e.get_attribute("data-value")));
            // `<input type=date>` is `2026-11-24`.
            let typed_day = typed("day").and_then(|d| {
                let mut p = d.splitn(3, '-').map(|x| x.parse::<i64>().ok());
                Some(dry_core::live::days_from_civil(p.next()??, p.next()?? as u32, p.next()?? as u32))
            });
            dry_core::ask::Input {
                picked,
                day: typed_day.or_else(|| attr(f, "data-picked").and_then(|d| d.parse().ok())),
                minute: attr(f, "data-minute").and_then(|m| m.parse().ok()),
                typed: typed("number").or_else(|| typed("text")).unwrap_or_default(),
            }
        })
        .collect()
}

// ---- the model connection ----

fn field(name: &str) -> Option<String> {
    doc()
        .query_selector(&format!("#connection input[name=\"{name}\"]"))
        .ok()
        .flatten()?
        .dyn_into::<HtmlInputElement>()
        .ok()
        .map(|i| i.value())
}

/// Use `endpoint` and `model` from now on; engines on the old setting (or
/// on none) follow.
fn apply_connection(a: &mut App, endpoint: String, model: String) {
    let old = a.settings.inference();
    a.settings.endpoint = endpoint;
    a.settings.model = model;
    let new = a.settings.inference();
    for e in a.world.state.engines.iter_mut().filter(|e| e.session.is_none()) {
        if e.inference.model.is_empty() || (e.inference.base_url == old.base_url && e.inference.model == old.model) {
            e.inference.base_url.clone_from(&new.base_url);
            e.inference.model.clone_from(&new.model);
            e.inference.api = new.api;
        }
    }
    a.sync_models();
}

fn set_check(status: Status, detected: Option<Result<Vec<String>, String>>) {
    with(|a| {
        if let Some(c) = a.world.connection.as_mut() {
            c.check = Some(status);
            if detected.is_some() {
                c.detected = detected;
                // Reached or not: the subscriptions say so now.
                let mut reg = (*a.world.state.capabilities.0).clone();
                reg.subscriptions = Some(a.subscriptions());
                a.world.state.equip(reg);
            }
        }
    });
    render();
}

/// What the model server at `base` has: its list, and what LM Studio's
/// or oMLX's own endpoint says of each (its type, which are loaded).
async fn served_at(base: &str, key: Option<String>) -> Result<Vec<dry_core::served::ServedModel>, String> {
    use dry_core::runner::Transport;
    use dry_core::served;
    let fetch = Fetch { key };
    let models = fetch.get(&format!("{base}/models"), &None).await.and_then(|b| served::parse_openai(&b))?;
    let more = fetch.get(&format!("{}/api/v0/models", served::server_root(base)), &None).await;
    let models = served::merge(models, more.ok().and_then(|b| served::parse_lmstudio(&b).ok()).unwrap_or_default());
    // oMLX says which are loaded on its own endpoint.
    let status = fetch.get(&format!("{base}/models/status"), &None).await;
    Ok(served::merge(models, status.ok().and_then(|b| served::parse_omlx_status(&b).ok()).unwrap_or_default()))
}

/// The server at `base` answered with `models`: list them (the loaded
/// ones that chat first), and with no model chosen yet take the first
/// choice, never an embeddings model (`served::first_choice`).
fn use_served(base: &str, models: &[dry_core::served::ServedModel]) {
    use dry_core::served;
    let ordered: Vec<String> = served::ordered(models).into_iter().map(|m| m.id.clone()).collect();
    let loaded: Vec<&str> = models.iter().filter(|m| m.loaded == Some(true)).map(|m| m.id.as_str()).collect();
    let line = if loaded.is_empty() {
        format!("{} models listed at {base}", models.len())
    } else {
        format!("{} models listed at {base}; loaded now: {}", models.len(), loaded.join(", "))
    };
    with(|a| {
        if a.settings.model.is_empty()
            && let Some(first) = served::first_choice(models)
        {
            apply_connection(a, base.to_string(), first.id.clone());
        }
    });
    save();
    set_check(Status::Done(line), Some(Ok(ordered)));
}

/// A first visit, no model chosen: ask the local model servers a person
/// is likely to run (`settings::LOCAL_SERVERS`) and use the one that
/// has a model loaded (`served::first_server`), so a page opened beside
/// one is ready without a visit to Settings. None answers: the note says which
/// were asked and what to do.
async fn find_model_server() {
    set_check(Status::Working("looking for a model server on this computer".into()), None);
    // Every server is asked, then one is taken: the first with a model
    // already in memory, else the first with a model that chats.
    let mut found = Vec::new();
    for (_, base) in settings::LOCAL_SERVERS {
        if let Ok(models) = served_at(base, None).await {
            found.push((*base, models));
        }
    }
    if let Some((base, models)) = dry_core::served::first_server(&found) {
        return use_served(base, models);
    }
    let asked: Vec<String> = settings::LOCAL_SERVERS.iter().map(|(name, base)| format!("{name} ({base})")).collect();
    set_check(
        Status::Missing(format!("no model server answered: asked {}. Start one with CORS on, then Settings → Models → Detect", asked.join(", "))),
        Some(Err("none found".into())),
    );
}

fn connection_act(what: &str, value: Option<String>) {
    let endpoint = field("endpoint").map(|e| settings::clean_endpoint(&e)).filter(|e| !e.is_empty());
    let model = field("model");
    match what {
        "save-connection" => {
            with(|a| {
                let e = endpoint.clone().unwrap_or_else(|| a.settings.endpoint.clone());
                let m = model.clone().unwrap_or_else(|| a.settings.model.clone()).trim().to_string();
                if let Some(k) = field("key").filter(|k| !k.trim().is_empty()) {
                    a.settings.key = Some(k.trim().to_string());
                }
                apply_connection(a, e, m);
            });
            changed();
            toast("Saved in this browser.");
        }
        "pick-model" => {
            with(|a| {
                let e = endpoint.clone().unwrap_or_else(|| a.settings.endpoint.clone());
                apply_connection(a, e, value.unwrap_or_default());
            });
            changed();
        }
        "forget-key" => {
            with(|a| {
                a.settings.key = None;
                a.sync_models();
            });
            changed();
        }
        "detect" => {
            let Some(base) = endpoint.or_else(|| with(|a| a.settings.endpoint.clone())) else { return };
            set_check(Status::Working(format!("asking {base}/models")), None);
            let key = with(|a| a.settings.key.clone()).flatten();
            spawn_local(async move {
                match served_at(&base, key).await {
                    Ok(models) => use_served(&base, &models),
                    Err(why) => set_check(Status::Failed(why), Some(Err("not listed".into()))),
                }
            });
        }
        "test" => {
            let ai = with(|a| {
                let mut ai = a.settings.inference();
                if let Some(e) = &endpoint {
                    ai.base_url.clone_from(e);
                }
                if let Some(m) = model.as_ref().filter(|m| !m.trim().is_empty()) {
                    ai.model = m.trim().to_string();
                }
                ai
            });
            let Some(ai) = ai else { return };
            if ai.model.is_empty() {
                return set_check(Status::Missing("choose a model first (Detect lists them)".into()), None);
            }
            set_check(Status::Working(format!("asking {} for a word", ai.model)), None);
            let key = with(|a| a.settings.key.clone()).flatten();
            spawn_local(async move {
                use dry_core::runner::Transport;
                let req = ai.request(&[Message { role: Role::User, text: "Reply with the one word: ready".into() }]);
                let t0 = js_sys::Date::now();
                let got = Fetch { key }.post(&req).await.and_then(|b| ai.reply(&b).map_err(|e| format!("{e:?}")));
                let secs = (js_sys::Date::now() - t0) / 1000.0;
                match got {
                    Ok(text) => set_check(
                        Status::Done(format!("{} answered in {secs:.1} s: {}", ai.model, text.trim().chars().take(80).collect::<String>())),
                        None,
                    ),
                    Err(why) => set_check(Status::Failed(why), None),
                }
            });
        }
        _ => {}
    }
}

// ---- for the companion (crate::companion, docs/companion.md) ----

/// The companion connected or left: the host the pages draw with, and draw.
pub(crate) fn companion_changed() {
    with(|a| a.world.host = Box::new(crate::companion::shell()));
    // Its capabilities join (or leave) this tab's registry (`crate::caps`).
    crate::caps::companion(crate::companion::conn());
    render();
}

/// The capability registry changed: every engine uses it now, and draw.
pub(crate) fn equip(mut reg: dry_core::capability::Registry) {
    with(|a| {
        reg.subscriptions = Some(a.subscriptions());
        a.world.state.equip(reg)
    });
    render();
}

/// A capability's event: each engine using it gets the message and a turn
/// (one already in a turn reads it in its next step's history).
pub(crate) fn capability_event(capability: &str, message: &str) {
    let mut run = Vec::new();
    with(|a| {
        for i in dry_core::capability::users(&a.world.state, capability) {
            if a.world.state.add_wake(i, message) && !a.busy(i) {
                run.push(i);
            }
        }
    });
    save();
    render();
    for i in run {
        start_turn(i, false);
    }
}

pub(crate) fn redraw() {
    render();
}

pub(crate) fn note(text: &str) {
    toast(text);
}

pub(crate) fn go_to(path: &str) {
    go(path);
}

/// Run the engines' callbacks now (a companion session ended).
pub(crate) fn settle_now() {
    settle();
}

pub(crate) fn engine_names() -> Vec<String> {
    with(|a| a.world.state.engines.iter().map(|e| e.name.clone()).collect()).unwrap_or_default()
}

/// Point Settings → Models at `base` and list its models.
pub(crate) fn use_models_from(base: &str) {
    if let Ok(Some(e)) = doc().query_selector("#connection input[name=\"endpoint\"]")
        && let Ok(i) = e.dyn_into::<HtmlInputElement>()
    {
        i.set_value(base);
    }
    connection_act("detect", None);
}

/// Give engine `target` (or a new one, named `name`) these artifacts; its
/// index. The new one is a root, so its own thread ("New thread" on the
/// companion's pages); a plain model engine, not the translator, which
/// holds no file or terminal tools by design.
pub(crate) fn attach(target: Option<usize>, name: &str, artifacts: Vec<(String, String)>) -> Option<usize> {
    let i = with(|a| {
        let i = match target {
            Some(i) if i < a.world.state.engines.len() => i,
            _ => {
                let i = new_engine(a);
                a.world.state.engines[i].name = name.to_string();
                i
            }
        };
        let e = &mut a.world.state.engines[i];
        for (n, text) in artifacts {
            e.artifacts.insert(n, text);
        }
        a.first_run = false;
        i
    })?;
    changed();
    Some(i)
}

/// Give every engine the companion's custom tools (or none), the way MCP
/// tools reach engines; true when anything changed.
pub(crate) fn equip_tools(tools: &[dry_core::mcp::Tool]) -> bool {
    with(|a| {
        let mut changed = false;
        for e in a.world.state.engines.iter_mut() {
            if e.mcp_tools != tools {
                e.mcp_tools = tools.to_vec();
                changed = true;
            }
        }
        changed
    })
    .unwrap_or(false)
}

/// Everything the store would hold now: the state's texts and this browser's.
pub(crate) fn all_texts() -> Texts {
    with(|a| {
        let mut t = store::to_texts(&a.world.state);
        t.extend(a.local_texts());
        t
    })
    .unwrap_or_default()
}
