mod client;
mod frame;
mod live;
mod net;
mod program;
mod router;
mod scene;
mod spans;
mod store;
mod ui;
mod window_place;
mod views;

use gpui::{
    App, Bounds, Context, Div, ElementId, Entity, FontWeight, Pixels, ScrollHandle, ScrollStrategy, SharedString, Stateful, Subscription,
    TitlebarOptions, UniformListScrollHandle, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, prelude::*, px, size,
};
use gpui_component::Root;
use gpui_component::input::{InputEvent, InputState, TextareaState};

use std::collections::HashMap;
use std::sync::Arc;

use dry_core::activity::Activity;
use dry_core::dashboard;
use dry_core::card::Status;
use dry_core::inference::{Inference, Message, Role, parse_model_ids};
use dry_core::layout;
use dry_core::models::Models;
use dry_core::routes;
use dry_core::screen::Screen;
use dry_core::session::Sessions;
use dry_core::state::State;
use dry_core::theme::{self, Mode, Tokens};
use frame::Parts;

/// The UI tokens page: a page of Settings.
pub(crate) const UI_PATH: &str = "/settings/ui";
use ui::color;

/// The window. `state` is what you see and edit; `saved` is what is on
/// disk. They differ after an edit until you press Save.
pub(crate) struct AppView {
    state: State,
    saved: State,
    /// Which store the state is in, and why it could not be read (then
    /// Save refuses, so it is never overwritten with an empty state).
    store: (String, Option<String>),
    /// The model settings from `models.json` (saved as soon as they change).
    models: Models,
    /// The design tokens, read from the active theme file and written back to it.
    pub(crate) theme: theme::Theme,
    /// This machine's token choices, `state/local.md`, laid over the theme.
    pub(crate) local_theme: Option<theme::Overrides>,
    /// Every theme in `themes/`, and the active one's name (Settings shows
    /// one button each). Read again with the theme.
    pub(crate) themes: (Vec<String>, String),
    /// Every scene in `scenes/` (or why it does not read); the app's
    /// scene setting picks one (`scene_settings`). Read again with the theme.
    pub(crate) scenes: std::collections::BTreeMap<String, Result<dry_core::scene::Scene, String>>,
    /// The scene behind the glass: its own entity, so it keeps its own
    /// clock and loop.
    scene_view: Entity<scene::SceneView>,
    /// Why `models.json` could not be read, if it could not (shown in Settings).
    models_error: Option<String>,
    /// The agent programs from `sessions.json`, or why it could not be read.
    /// Read again when the file changes.
    pub(crate) sessions: Result<Sessions, String>,
    /// Per program in `sessions.json`: where it is installed, if it is.
    /// Found when the file is read, so a card never searches `PATH`.
    pub(crate) programs: HashMap<String, Option<std::path::PathBuf>>,
    /// Per engine (by id): its running MCP servers, started from its
    /// spec's mcp.json. Shared with the background calls (one at a time
    /// per server, servers at the same time: `mcp::Servers`).
    mcp: HashMap<String, Arc<dry_tools::mcp::Servers>>,
    /// Per model setting: the model ids its server offered at the last Detect.
    detected: HashMap<String, Vec<String>>,
    /// Per model setting: the last Detect/Test result, shown on its card.
    model_status: HashMap<String, Status>,
    /// Per engine (by index): what its last question is doing ("Waiting for
    /// ...", or why it failed). No entry: nothing to say.
    pub(crate) engine_status: HashMap<usize, Status>,
    /// Per engine (by index): what its turn is doing right now
    /// (`dry_core::activity`), while one runs. Shown as a tint and a label
    /// on its card, nav entry and page.
    pub(crate) engine_activity: HashMap<usize, Activity>,
    /// Per engine (by index): its answer as far as the model has written
    /// it (`dry_core::stream`), drawn under the conversation until saved.
    pub(crate) streaming: HashMap<usize, dry_core::stream::View>,
    /// Per engine (by index): its prompt's size at the last call, with a
    /// note when it was over the threshold, and the last compression.
    pub(crate) engine_context: HashMap<usize, EngineContext>,
    /// Per model (`base_url model`): its context window, once known (the
    /// setting, the server's answer, or assumed).
    pub(crate) windows: HashMap<String, dry_core::context::Window>,
    /// Per engine: its exact view (artifacts, prompt slots) as last taken
    /// (`live.rs`). In a cell so a view can take one the first time.
    pub(crate) inspects: std::cell::RefCell<live::Inspects>,
    /// Every page and the view component that draws it.
    router: router::Router,
    /// The live view state: the current path (the centre draws the view
    /// registered for the route it matches), the hidden views, the panel.
    pub(crate) view: routes::ViewState,
    /// The text area on an engine's page (a gpui-component). It owns the
    /// text being typed, the caret, selection and undo.
    pub(crate) input: Entity<TextareaState>,
    /// What this runtime can do (voice, ...), checked once at start.
    pub(crate) caps: dry_core::composer::Capabilities,
    /// A line under the composer: why an option is off, or what it did.
    pub(crate) composer_note: Option<String>,
    /// The Desk's selection: the thread (a root's id, `""`: the first) and
    /// the engine the composer posts to (`None`: the thread's head).
    pub(crate) desk_pick: (String, Option<String>),
    /// Settings → Reset history: the confirm step is open (the first
    /// press), and what the last reset said.
    pub(crate) reset_confirm: bool,
    pub(crate) reset_note: Option<String>,
    /// The host running the turns, while a `dry-server` hosts the store
    /// (`client.rs`): then this window sends through it and only reads.
    pub(crate) host: Option<dry_tools::host::Lock>,
    /// The store's generation this window last read from the host.
    pub(crate) host_gen: Option<u64>,
    /// The edit boxes on the UI page, one per token. Made when the page
    /// opens, filled with the current values; read back on Refresh.
    token_inputs: Vec<TokenInput>,
    /// Values on the UI page that did not parse at the last Refresh.
    token_errors: Vec<String>,
    /// The New page form's boxes: path, title, icon glyph.
    pub(crate) new_route: [Entity<InputState>; 3],
    /// Why the last Create or Delete did not work.
    pub(crate) new_route_error: Option<String>,
    /// What the explorer, editor and terminal show (`dry_core::workbench`).
    pub(crate) workbench: dry_core::workbench::Workbench,
    /// The terminal's command line; Enter runs it.
    pub(crate) terminal_input: Entity<InputState>,
    /// The terminal's history scrolls to the newest after a command.
    pub(crate) terminal_scroll: ScrollHandle,
    /// The editor's lines; back to the top when another file opens.
    pub(crate) editor_scroll: UniformListScrollHandle,
    /// The repo folder's name, shown over the explorer and terminal.
    pub(crate) repo_name: String,
    /// The width the page on screen has inside its box (`layout::Frame::
    /// content_width`), set each render: `columns` splits on it.
    pub(crate) content_width: f32,
    /// Spans of the turns this window runs (`spans.rs`): each running
    /// turn's trace by engine id, the tasks open across turns, every log
    /// as read for the views, and the analytics page's day range.
    traces: HashMap<String, dry_core::spans::Trace>,
    tasks: dry_core::spans::Tasks,
    pub(crate) span_log: spans::Log,
    pub(crate) analytics_days: u32,
    /// Held so these callbacks stay registered: window moved or resized,
    /// and events from the text area.
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    fn new(
        state: State,
        theme: theme::Theme,
        models: Models,
        models_error: Option<String>,
        store: (String, Option<String>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // The text area: grows from 1 to 6 lines; Enter submits (we get
        // `PressEnter`), Shift+Enter starts a new line.
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 6).submit_on_enter(true).placeholder("Type, Enter to send"));
        let field =
            |hint: &'static str, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).placeholder(hint));
        let new_route = [field("/notes", window, cx), field("Notes", window, cx), field("✎", window, cx)];
        let terminal_input = field("Type a command; Enter runs it in the repo folder", window, cx);
        // Which routes are hidden or turned off: this machine's choice,
        // `state/local.md`. The app opens on home, or the first route left.
        let mut view = routes::ViewState::default();
        view.read_local_md(&dry_tools::read_local_md());
        let router = router::Router::load(&view.removed);
        view.location = view.start(router.table());
        let subscriptions = vec![
            cx.subscribe_in(&terminal_input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.run_terminal(window, cx);
                }
            }),
            // Resizing already re-renders, but moving the window (for example
            // to another monitor) does not; this asks for a render whenever the
            // window's bounds change, so the screen info stays current.
            cx.observe_window_bounds(window, |_, window, cx| {
                window_place::save(&window.window_bounds());
                cx.notify()
            }),
            cx.subscribe_in(&input, window, Self::on_input),
        ];
        // Live files: once a second, see whether themes/, builtins.json,
        // routes/ or components/ changed (by the `dry` tool, an editor, or
        // the UI page) and read them again, without restarting the window.
        cx.spawn_in(window, async move |this, cx| {
            let mut stamp = store::files_stamp();
            let mut ticks: u64 = 0;
            let mut subs_watch = dry_tools::subscriptions::Watch::default();
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                ticks += 1;
                // The subscriptions (docs/elements.md): probed off this
                // thread every minute and when models.json or sessions.json
                // change.
                if subs_watch.due() {
                    let subs = cx.background_executor().spawn(async { dry_tools::subscriptions::probe() }).await;
                    if this.update(cx, |app, _| app.set_subscriptions(subs)).is_err() {
                        break;
                    }
                }
                // A host (`client.rs`): read its lock and what its engines
                // do, off this thread.
                let (found, alive, engines) = cx
                    .background_executor()
                    .spawn(async {
                        let found = dry_tools::host::default_lock_path().and_then(|p| dry_tools::host::read(&p));
                        let alive = found.as_ref().is_some_and(|l| l.alive());
                        let engines = found.as_ref().filter(|_| alive).and_then(|l| dry_tools::host::engines(l).ok());
                        (found, alive, engines)
                    })
                    .await;
                // Terminal sessions that ended: marked, and a `start` one
                // wakes its engine (`live.rs`). With a host, it does that.
                let alive = this.update(cx, |app, cx| {
                    // The spans logs, for the analytics views (`spans.rs`).
                    if ticks % 5 == 1 {
                        app.refresh_spans(cx);
                    }
                    app.watch_host(found, alive, cx);
                    if let Some(engines) = engines {
                        app.host_activity(engines, cx);
                    }
                    if !app.hosted() {
                        app.settle_callbacks(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
                let now = store::files_stamp();
                if now == stamp {
                    continue;
                }
                stamp = now;
                eprintln!("files changed; reading routes/, components/ and the theme again");
                let alive = this.update_in(cx, |app, window, cx| {
                    app.router = router::Router::load(&app.view.removed);
                    app.sessions = dry_tools::read_sessions();
                    app.programs = find_programs(&app.sessions);
                    app.reload_theme(window, cx);
                    // The page on screen was deleted: go home.
                    if app.router.resolve(&app.view.location).is_none() {
                        app.navigate(routes::HOME, window, cx);
                    }
                });
                if alive.is_err() {
                    break; // the window closed
                }
            }
        })
        .detach();
        // This machine's capabilities (compiled and capabilities/*.md, probed).
        let mut state = state;
        state.equip(dry_tools::capability::local_cached().0.clone());
        let themes = store::themes(&state);
        AppView {
            saved: state.clone(),
            store,
            state,
            theme,
            local_theme: store::load_local_theme().ok().flatten(),
            themes,
            scenes: store::load_scenes(),
            scene_view: cx.new(|cx| scene::SceneView::new(window, cx)),
            models,
            models_error,
            programs: find_programs(&dry_tools::read_sessions()),
            sessions: dry_tools::read_sessions(),
            mcp: HashMap::new(),
            detected: HashMap::new(),
            model_status: HashMap::new(),
            engine_status: HashMap::new(),
            engine_activity: HashMap::new(),
            streaming: HashMap::new(),
            engine_context: HashMap::new(),
            windows: HashMap::new(),
            inspects: Default::default(),
            router,
            view,
            input,
            caps: dry_tools::capabilities(),
            composer_note: None,
            desk_pick: (String::new(), None),
            reset_confirm: false,
            reset_note: None,
            host: None,
            host_gen: None,
            token_inputs: Vec::new(),
            token_errors: Vec::new(),
            new_route,
            new_route_error: None,
            workbench: dry_core::workbench::Workbench::default(),
            terminal_input,
            terminal_scroll: ScrollHandle::new(),
            editor_scroll: UniformListScrollHandle::new(),
            repo_name: dry_tools::root().file_name().map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().to_string()),
            content_width: 0.,
            traces: HashMap::new(),
            tasks: dry_core::spans::Tasks::default(),
            span_log: spans::Log::default(),
            analytics_days: 7,
            _subscriptions: subscriptions,
        }
    }

    /// Draw with the active theme, and over it the overrides of the route
    /// on screen (its `overrides.md`, then its chosen theme). Called on
    /// every change of page, mode or theme, so leaving a route that changes
    /// the look puts the app's own look back.
    pub(crate) fn apply_theme(&self, cx: &mut App) {
        ui::apply(self.state.mode, &theme::layered(&self.theme, &self.layers()), self.local_theme.as_ref(), cx);
    }

    /// The override layers on screen: the route's (its overrides and
    /// chosen theme), then this machine's own (`state/local.md`), which win.
    fn layers(&self) -> Vec<&theme::Overrides> {
        let mut layers = self.router.page(&self.view.location).and_then(|(_, local)| local).map(|l| l.layers()).unwrap_or_default();
        layers.extend(self.local_theme.as_ref());
        layers
    }

    /// The scene settings in force here (`dry_core::scene::settings`): the
    /// layers over the state's `app.md` settings.
    pub(crate) fn scene_settings(&self) -> dry_core::scene::Settings {
        dry_core::scene::settings(&self.state, &self.theme, &self.layers())
    }

    /// The themes the route on screen ships (names, the one in use), if any.
    pub(crate) fn route_themes(&self) -> Option<(Vec<String>, String)> {
        let (_, local) = self.router.page(&self.view.location)?;
        let local = local?;
        let active = local.theme()?.to_string();
        Some((local.themes.keys().cloned().collect(), active))
    }

    /// Use theme `name` of the route on screen: written to its folder's
    /// `themes/active`, and drawn now.
    pub(crate) fn use_route_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(pattern) = self.router.pattern(&self.view.location) else { return };
        if let Err(e) = dry_tools::set_route_theme(&pattern, name) {
            eprintln!("{e}");
            return;
        }
        self.router.set_theme(&pattern, name);
        self.apply_theme(cx);
        cx.notify();
    }

    /// Read the open folders the tree has not read yet.
    fn read_tree(&mut self) {
        for dir in self.workbench.tree.to_read() {
            match store::read_dir(&dir) {
                Ok(found) => self.workbench.tree.set_children(&dir, found),
                Err(e) => self.workbench.tree.set_error(&dir, e),
            }
        }
    }

    /// A folder in the explorer was clicked: open or close it.
    pub(crate) fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.workbench.tree.toggle(path) {
            self.read_tree();
        }
        cx.notify();
    }

    /// A file in the explorer was clicked: read it and show it.
    pub(crate) fn open_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.workbench.open(store::read_file(path));
        self.editor_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// Enter in the terminal: run the line in the repo folder in the
    /// background, then show what it printed and read the tree again (the
    /// command may have changed files). The only place a command starts.
    fn run_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.terminal_input.read(cx).value().to_string();
        let submit = self.workbench.terminal.submit(&line);
        if submit != dry_core::workbench::Submit::Nothing {
            self.terminal_input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        if let dry_core::workbench::Submit::Run { id, command } = submit {
            cx.spawn(async move |this, cx| {
                let result = cx.background_executor().spawn(async move { store::run(&command) }).await;
                this.update(cx, |app, cx| {
                    app.workbench.terminal.finish(id, result);
                    app.workbench.tree.forget();
                    app.read_tree();
                    app.terminal_scroll.scroll_to_bottom();
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.terminal_scroll.scroll_to_bottom();
        cx.notify();
    }

    fn unsaved(&self) -> bool {
        self.state != self.saved
    }

    /// Go to `path`. The next render draws its view. On an engine page the
    /// text area is emptied and given the keyboard.
    pub(crate) fn navigate(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        // A moved route (`page.json`'s `moved`) opens where it moved to.
        let moved = self.router.table().moved_to(path);
        let path = moved.as_deref().unwrap_or(path);
        self.view.location = path.to_string();
        // An engine page opened: its exact view is taken again.
        self.inspects.get_mut().taken.clear();
        // The new page may bring its own look, or the old one took one away.
        self.apply_theme(cx);
        if self.workbench.tree.to_read().contains(&String::new()) {
            self.read_tree();
        }
        if path == UI_PATH {
            self.fill_token_inputs(window, cx);
        }
        if self.open_engine().is_some() {
            self.input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            });
        }
        cx.notify();
    }

    /// The engine the current path points at (`/desk/:id`), if any.
    fn open_engine(&self) -> Option<usize> {
        let (route, params, _) = self.router.resolve(&self.view.location)?;
        let index = params.index("id").filter(|_| route.pattern == "/desk/:id")?;
        (index < self.state.engines.len()).then_some(index)
    }

    /// Open root `id`'s thread on the Desk with the composer to it, and
    /// give the composer the keyboard.
    pub(crate) fn open_thread(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.desk_pick = (id.to_string(), Some(id.to_string()));
        self.navigate("/desk", window, cx);
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
    }

    /// New thread: a new root engine made from the default's translator
    /// (`dry_core::shipped::new_thread`) on the default model, its thread
    /// opened. Through the host when one hosts the store.
    pub(crate) fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.hosted() {
            return self.create_on_host(serde_json::json!({ "kind": "thread" }), window, cx);
        }
        let ai = default_inference(&self.models).unwrap_or_default();
        match dry_core::shipped::new_thread(&mut self.state, &dry_tools::store::base().texts, &ai) {
            Ok(i) => {
                self.start_mcp(i, cx);
                let id = self.state.engines[i].id.clone();
                self.open_thread(&id, window, cx);
            }
            Err(why) => {
                self.composer_note = Some(why);
                cx.notify();
            }
        }
    }

    /// A new root on the host (`POST /api/engines`), its thread opened
    /// once the host says its id.
    fn create_on_host(&mut self, body: serde_json::Value, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lock) = self.host.clone() else { return };
        cx.spawn_in(window, async move |this, cx| {
            let made = cx
                .background_executor()
                .spawn(async move { dry_tools::host::call_ok(&lock, "/api/engines", Some(&body), std::time::Duration::from_secs(10)) })
                .await;
            this.update_in(cx, |app, window, cx| match made {
                Ok(v) => {
                    let id = v.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
                    app.open_thread(&id, window, cx);
                }
                Err(e) => {
                    app.composer_note = Some(format!("The host refused: {e}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// A new model engine on the default model with the contract in
    /// responses.md (a send on a page with no engine), its page opened.
    pub(crate) fn new_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let i = self.state.add_engine();
        // A new engine talks to the default model setting, and answers to
        // the contract in responses.md.
        if let Some(ai) = default_inference(&self.models) {
            self.state.engines[i].inference = ai;
        }
        self.load_response(i);
        // ...and with the app's own tools (mcp/dry.json), served by `dry mcp`.
        if let Ok(mcp) = dry_tools::default_mcp() {
            self.state.engines[i].mcp = mcp;
        }
        self.start_mcp(i, cx);
        self.navigate(&routes::engine_path(i), window, cx);
    }

    /// Start engine `i`'s MCP servers (its spec's mcp.json), in the
    /// background, and give it their tools. Any it had running stop first.
    pub(crate) fn start_mcp(&mut self, i: usize, cx: &mut Context<Self>) {
        let e = &mut self.state.engines[i];
        self.mcp.remove(&e.id); // dropping the clients stops their servers
        e.mcp_tools.clear();
        if e.mcp.trim().is_empty() {
            return;
        }
        let (id, json) = (e.id.clone(), e.mcp.clone());
        self.engine_status.insert(i, Status::Working("starting MCP servers".into()));
        cx.spawn(async move |this, cx| {
            let started = cx.background_executor().spawn(async move { dry_tools::mcp::start_all(&json) }).await;
            this.update(cx, |app, cx| {
                let Some(i) = app.state.engines.iter().position(|e| e.id == id) else { return };
                match started {
                    Ok((clients, tools)) => {
                        app.engine_status.insert(i, Status::Done(format!("{} MCP tool(s)", tools.len())));
                        // Found at start, not edited: the saved copy gets them too,
                        // so they do not count as an unsaved change.
                        if let Some(saved) = app.saved.engines.iter_mut().find(|e| e.id == id) {
                            saved.mcp_tools = tools.clone();
                        }
                        app.state.engines[i].mcp_tools = tools;
                        app.mcp.insert(id, Arc::new(dry_tools::mcp::Servers::new(clients)));
                    }
                    Err(e) => {
                        app.engine_status.insert(i, Status::Failed(e));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Every engine's MCP servers (at start).
    pub(crate) fn start_all_mcp(&mut self, cx: &mut Context<Self>) {
        for i in 0..self.state.engines.len() {
            self.start_mcp(i, cx);
        }
    }

    /// Read engine `i`'s spec again from the store (after its files were
    /// edited, or a spec folder was copied in): instructions, contract,
    /// format, tools, model and mcp.json; then restart its MCP servers.
    /// What happened (messages, artifacts) stays as it is.
    pub(crate) fn reload_spec(&mut self, i: usize, cx: &mut Context<Self>) {
        let id = self.state.engines[i].id.clone();
        let found = dry_tools::store::open_default()
            .and_then(|s| s.load())
            .and_then(|s| s.engines.into_iter().find(|e| e.id == id).ok_or(format!("{id} is not in the store yet (Save first)")));
        match found {
            Ok(spec) => {
                let e = &mut self.state.engines[i];
                e.name = spec.name;
                e.description = spec.description;
                e.legacy_goal = spec.legacy_goal;
                e.inference = spec.inference;
                e.response = spec.response;
                e.format = spec.format;
                e.tools = spec.tools;
                e.mcp = spec.mcp;
                self.engine_status.remove(&i);
                self.start_mcp(i, cx);
            }
            Err(e) => {
                self.engine_status.insert(i, Status::Failed(e));
            }
        }
    }

    /// Give engine `i` the contract in `responses.md` (its format too).
    /// Also how an engine's contract is changed while it runs: edit the
    /// file, then Reload on its page.
    pub(crate) fn load_response(&mut self, i: usize) {
        match dry_tools::read_responses() {
            Ok((response, format)) => {
                let e = &mut self.state.engines[i];
                (e.response, e.format) = (response, format);
                self.engine_status.remove(&i);
            }
            Err(e) => {
                self.engine_status.insert(i, Status::Missing(e));
            }
        }
    }

    /// Switch engine `i`'s answer format (`json`, `toon`, ...); its
    /// contract stays as it is.
    pub(crate) fn set_format(&mut self, i: usize, format: dry_core::response::Format) {
        if let Some(e) = self.state.engines.get_mut(i) {
            e.format = format;
        }
    }

    /// The subscriptions probed again: their usage from the spans logs and
    /// the busy sessions counted, into the registry every engine uses.
    pub(crate) fn set_subscriptions(&mut self, mut subs: dry_core::subscriptions::Subscriptions) {
        subs.usage(&dry_tools::subscriptions::recent(self.span_log.spans.values()));
        let mut running = std::collections::BTreeMap::new();
        for (j, e) in self.state.engines.iter().enumerate() {
            if let Some(s) = e.session.as_ref().filter(|_| self.seen(j).busy) {
                *running.entry(s.tool.clone()).or_insert(0) += 1;
            }
        }
        subs.set_running(&running);
        let mut reg = (*self.state.capabilities.0).clone();
        if reg.subscriptions.as_ref() == Some(&subs) {
            return;
        }
        reg.subscriptions = Some(subs);
        self.state.equip(reg);
    }

    /// New session: a new root engine run by program `tool` (from
    /// `sessions.json`), its own thread opened.
    pub(crate) fn new_session(&mut self, tool: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.hosted() {
            return self.create_on_host(serde_json::json!({ "kind": tool }), window, cx);
        }
        let description = self.sessions.as_ref().ok().and_then(|s| s.get(tool)).map(|t| t.description.clone()).unwrap_or_default();
        let i = self.state.add_session(tool, &description);
        let id = self.state.engines[i].id.clone();
        self.open_thread(&id, window, cx);
    }

    /// Create a route from the New page form: write its `page.json`, read
    /// the routes again, and open it. The panel shows it at once.
    pub(crate) fn create_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let [path, title, icon] = self.new_route.clone().map(|input| input.read(cx).value().to_string());
        if path.trim().is_empty() {
            self.new_route_error = Some("a page needs a path, like /notes".into());
            return cx.notify();
        }
        match dry_tools::create_page(&path, &title, &icon) {
            Ok(pattern) => {
                self.new_route_error = None;
                for input in self.new_route.clone() {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
                self.router = router::Router::load(&self.view.removed);
                self.navigate(&pattern, window, cx);
            }
            Err(e) => {
                self.new_route_error = Some(e);
                cx.notify();
            }
        }
    }

    /// Hide or show route `pattern`'s navigation button; kept in
    /// `state/local.md`.
    pub(crate) fn toggle_route_hidden(&mut self, pattern: &str, cx: &mut Context<Self>) {
        self.view.toggle_hidden(pattern);
        if let Err(e) = dry_tools::save_local_routes(&self.view) {
            self.new_route_error = Some(e);
        }
        cx.notify();
    }

    /// Turn standard route `pattern` off or back on; kept in
    /// `state/local.md`. Off, it leaves the table; if it was on screen,
    /// go to where the app now starts.
    pub(crate) fn toggle_route_removed(&mut self, pattern: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.view.toggle_removed(pattern) {
            self.new_route_error = Some(e);
            cx.notify();
            return;
        }
        if let Err(e) = dry_tools::save_local_routes(&self.view) {
            self.new_route_error = Some(e);
        }
        self.router = router::Router::load(&self.view.removed);
        if self.router.resolve(&self.view.location).is_none() {
            let start = self.view.start(self.router.table());
            self.navigate(&start, window, cx);
        }
        cx.notify();
    }

    /// Delete a route: remove its `page.json` (Settings page). If it was the
    /// page on screen, go home.
    pub(crate) fn delete_route(&mut self, pattern: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = dry_tools::delete_page(pattern) {
            self.new_route_error = Some(e);
        }
        self.router = router::Router::load(&self.view.removed);
        if self.router.resolve(&self.view.location).is_none() {
            let start = self.view.start(self.router.table());
            self.navigate(&start, window, cx);
        }
        cx.notify();
    }

    /// Read `routes/` and `components/` again: brings back removed routes
    /// and picks up files changed on disk.
    pub(crate) fn restore_routes(&mut self, cx: &mut Context<Self>) {
        self.router = router::Router::load(&self.view.removed);
        cx.notify();
    }

    /// Write `models.json`; a failure shows on the model's card.
    fn save_models(&mut self, name: &str) {
        if let Err(e) = store::save_models(&self.models) {
            self.model_status.insert(name.to_string(), Status::Failed(format!("could not save models.json: {e}")));
        }
    }

    pub(crate) fn set_default_model(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.models.set_default(name) {
            self.save_models(name);
        }
        cx.notify();
    }

    /// Use model `model` for setting `name` (same provider and API).
    pub(crate) fn pick_model(&mut self, name: &str, model: &str, cx: &mut Context<Self>) {
        match self.models.get_mut(name).map(|m| m.set_model(model)) {
            Some(Ok(())) => self.save_models(name),
            Some(Err(e)) => {
                self.model_status.insert(name.to_string(), Status::Missing(e));
            }
            None => {}
        }
        cx.notify();
    }

    /// Setting `name`'s inference object; on failure the reason goes on its card.
    fn inference_for(&mut self, name: &str) -> Option<dry_core::inference::OpenAi> {
        let m = self.models.get(name)?;
        match self.models.inference(m) {
            Ok(ai) => Some(ai),
            Err(e) => {
                self.model_status.insert(name.to_string(), Status::Missing(e));
                None
            }
        }
    }

    /// Ask setting `name`'s server which models it has (`GET {base}/models`).
    /// Runs in the background; the ids show as buttons on the card. Nothing
    /// is picked for you: on a local server, the first call to a model loads
    /// it into memory, and some are tens of GB.
    pub(crate) fn detect_models(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(ai) = self.inference_for(name) else { return cx.notify() };
        let (url, key_env, name) = (format!("{}/models", ai.base_url.trim_end_matches('/')), ai.key_env, name.to_string());
        self.model_status.insert(name.clone(), Status::Working(format!("asking {url}")));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let body = cx.background_executor().spawn(async move { net::get(&url, &key_env) }).await;
            let ids = body.and_then(|b| parse_model_ids(&b).map_err(|e| format!("{e:?}")));
            this.update(cx, |app, cx| {
                match ids {
                    Ok(ids) => {
                        app.model_status.insert(name.clone(), Status::Done(format!("{} models found; click one to use it", ids.len())));
                        app.detected.insert(name, ids);
                    }
                    Err(e) => {
                        app.model_status.insert(name, Status::Failed(format!("detect: {e}")));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Send one short message through setting `name`'s inference object and
    /// show the reply. Runs in the background; a local server may take a
    /// while on the first call while it loads the model.
    pub(crate) fn test_model(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(ai) = self.inference_for(name) else { return cx.notify() };
        let name = name.to_string();
        if ai.model.is_empty() {
            self.model_status.insert(name, Status::Missing("choose a model first (Detect)".into()));
            cx.notify();
            return;
        }
        let req = ai.request(&[Message { role: Role::User, text: "Reply with one short sentence to say hello.".into() }]);
        self.model_status.insert(name.clone(), Status::Working(format!("sending to {} (the first call can take a while)", ai.model)));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let body = cx.background_executor().spawn(async move { net::post(&req) }).await;
            let status = match body.map(|b| ai.reply(&b)) {
                Ok(Ok(reply)) => Status::Done(format!("reply: {reply}")),
                Ok(Err(e)) => Status::Failed(format!("bad reply: {e:?}")),
                Err(e) => Status::Failed(format!("request: {e}")),
            };
            this.update(cx, |app, cx| {
                app.model_status.insert(name, status);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Change light/dark: change the state, then inject the new tokens app-wide.
    pub(crate) fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.state.mode = mode;
        self.apply_theme(cx);
        // Colours are per mode, so the UI page's boxes now show other values.
        if self.view.location == UI_PATH {
            self.fill_token_inputs(window, cx);
        }
    }

    /// Use scene `name` (`none` for none): app.md's `scene:`, saved with
    /// the state. A machine's `state/local.md` `## Scene` row still wins.
    pub(crate) fn use_scene(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if name != theme::NO_SCENE && !self.scenes.contains_key(name) {
            return;
        }
        self.state.scene = Some(name.to_string());
        if self.view.location == UI_PATH {
            self.fill_token_inputs(window, cx);
        }
        cx.notify();
    }

    /// (Re)make the UI page's edit boxes, each holding its token's current value.
    fn fill_token_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t = theme::resolve(self.state.mode, &self.theme);
        let mut rows = Vec::new();
        for name in theme::COLOR_NAMES {
            rows.push((TokenKind::Color, *name, theme::hex(token_color(&t, name))));
        }
        for name in theme::NUMBER_NAMES {
            rows.push((TokenKind::Number, *name, token_number(&t, name).to_string()));
        }
        rows.push((TokenKind::Font, "font", t.font_family.clone()));
        rows.push((TokenKind::Font, "font_mono", t.font_mono.clone()));
        // The scene settings: app.md's (saved with the state), not tokens.
        let look = self.scene_settings();
        let scene = self.state.scene.clone().or(look.scene).unwrap_or_else(|| theme::NO_SCENE.to_string());
        rows.push((TokenKind::Setting, theme::SCENE, scene));
        rows.push((TokenKind::Setting, theme::MOTION, self.state.motion.unwrap_or(look.motion).to_string()));
        self.token_inputs = rows
            .into_iter()
            .map(|(kind, name, value)| {
                let input = cx.new(|cx| {
                    let mut state = InputState::new(window, cx);
                    state.set_value(value, window, cx);
                    state
                });
                TokenInput { kind, name, input }
            })
            .collect();
        self.token_errors.clear();
    }

    /// Refresh: read every box on the UI page into the theme, write
    /// its file, then inject the result app-wide. A value equal to its
    /// default clears the change; a value that does not parse is reported.
    pub(crate) fn refresh_tokens(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.state.mode;
        let mut errors = Vec::new();
        for row in &self.token_inputs {
            let text = row.input.read(cx).value().trim().to_string();
            if row.kind == TokenKind::Setting {
                // App settings, saved with the state (app.md).
                match row.name {
                    theme::SCENE if text == theme::NO_SCENE || self.scenes.contains_key(&text) => self.state.scene = Some(text),
                    theme::SCENE => errors.push(format!("scene: no \"{text}\" in scenes/ (or none)")),
                    _ => match text.parse::<f32>() {
                        Ok(m) if m >= 0.0 => self.state.motion = Some(m),
                        _ => errors.push(format!("motion: \"{text}\" is not a number of seconds")),
                    },
                }
                continue;
            }
            // Colours change the current mode only; sizes and the font both.
            let side = (row.kind == TokenKind::Color).then_some(mode);
            if let Err(e) = self.theme.set(row.name, &text, side) {
                errors.push(format!("{}: {e}", row.name));
            }
        }
        self.save_theme(&mut errors);
        self.apply_theme(cx);
        self.fill_token_inputs(window, cx);
        self.token_errors = errors;
        cx.notify();
    }

    /// Put one token back to its default, in both modes (`None`: every
    /// token), and write the theme file.
    pub(crate) fn reset_tokens(&mut self, name: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        // A scene setting goes back to the default's (app.md under the store).
        match name {
            Some(theme::SCENE) => self.state.scene = dry_core::scene::settings(&State::default(), &self.theme, &[]).scene.or(Some(theme::NO_SCENE.into())),
            Some(theme::MOTION) => self.state.motion = Some(dry_core::scene::settings(&State::default(), &self.theme, &[]).motion),
            _ => self.theme.reset(name),
        }
        let mut errors = Vec::new();
        self.save_theme(&mut errors);
        self.apply_theme(cx);
        self.fill_token_inputs(window, cx);
        self.token_errors = errors;
        cx.notify();
    }

    /// Switch to theme `name` (the state's `theme:`, saved with it) and
    /// draw with it now.
    pub(crate) fn use_theme(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        match dry_tools::looks::exists(dry_core::look::Kind::Theme, name) {
            Ok(()) => {
                self.state.theme = Some(name.to_string());
                self.reload_theme(window, cx)
            }
            Err(e) => {
                self.token_errors = vec![e];
                cx.notify();
            }
        }
    }

    /// Read the active theme again (after switching, or editing it by hand
    /// or with the tool).
    pub(crate) fn reload_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.themes = store::themes(&self.state);
        self.scenes = store::load_scenes();
        match store::load_theme(&self.state) {
            Ok(theme) => {
                self.theme = theme;
                self.token_errors.clear();
            }
            Err(e) => self.token_errors = vec![e],
        }
        match store::load_local_theme() {
            Ok(local) => self.local_theme = local,
            Err(e) => self.token_errors.push(e),
        }
        self.apply_theme(cx);
        let errors = std::mem::take(&mut self.token_errors);
        self.fill_token_inputs(window, cx);
        self.token_errors = errors;
        cx.notify();
    }

    fn save_theme(&mut self, errors: &mut Vec<String>) {
        if let Err(e) = store::save_theme(&self.state, &self.theme) {
            errors.push(format!("could not write the theme: {e}"));
        }
    }

    /// Events from the text area: Enter sends.
    fn on_input(&mut self, _: &Entity<TextareaState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let InputEvent::PressEnter { .. } = event {
            self.send(window, cx);
        }
    }

    /// Send the composer's text: to the open engine, or, on a page without
    /// one, to a new engine (whose page then opens). Empty: nothing.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        // On the Desk the text is a guided post on the selected thread.
        if self.view.location == "/desk" || self.view.location.starts_with("/desk?") {
            return self.desk_send(window, cx);
        }
        if self.hosted() {
            let engine = self.open_engine();
            return self.send_to_host(engine, text, window, cx);
        }
        let engine = match self.open_engine() {
            Some(i) => i,
            None => {
                self.new_engine(window, cx);
                self.state.engines.len() - 1
            }
        };
        if self.state.add_text(engine, &text) {
            self.input.update(cx, |input, cx| input.set_value("", window, cx));
            self.composer_note = None;
            self.ask(engine, cx);
            cx.notify();
        }
    }

    /// The Desk's composer: your guided post on the selected thread to the
    /// picked engine (`dry_core::desk::your_post`), through the host when
    /// one hosts the store; here, the post is kept and the callbacks run
    /// at once, so the engine it is for starts its turn (`desk::guided`).
    pub(crate) fn desk_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        // The thread and engine the Desk shows (its head unless one is picked).
        let (picked, to) = self.desk_pick.clone();
        let model = dry_core::desk::build(&self.state, &|_| dry_core::desk::Live::default());
        let Some((level, to)) = model.pick(Some(picked.as_str()).filter(|s| !s.is_empty()), to.as_deref()).map(|(t, to)| (t.id.clone(), to)) else {
            self.composer_note = Some("there is no thread yet: start one with New thread".into());
            return cx.notify();
        };
        if let Some(lock) = self.host.clone() {
            self.composer_note = Some("posting to the host…".into());
            let input = self.input.clone();
            cx.spawn_in(window, async move |this, cx| {
                let sent = cx.background_executor().spawn(async move { dry_tools::host::post_thread(&lock, "gpui", &level, to.as_deref(), &text) }).await;
                this.update_in(cx, |app, window, cx| {
                    app.composer_note = match sent {
                        Ok(dry_tools::host::Sent::Accepted { .. }) => {
                            input.update(cx, |input, cx| input.set_value("", window, cx));
                            None
                        }
                        Ok(dry_tools::host::Sent::Refused { error, .. }) => Some(format!("The host refused: {error}")),
                        Err(e) => Some(e),
                    };
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        match dry_core::desk::your_post(&self.state, &level, to.as_deref(), &text, dry_tools::live::clock().unix) {
            Ok((root, edit)) => {
                match dry_core::shared::post(&mut self.state, &root, &edit) {
                    Ok(_) => {
                        self.input.update(cx, |input, cx| input.set_value("", window, cx));
                        self.composer_note = None;
                        self.settle_callbacks(cx);
                    }
                    Err(why) => self.composer_note = Some(why),
                }
            }
            Err(why) => self.composer_note = Some(why),
        }
        cx.notify();
    }

    /// Answer engine `engine` with `text` (a choice its reply offered), as
    /// if typed and sent.
    pub(crate) fn answer(&mut self, engine: usize, text: &str, cx: &mut Context<Self>) {
        if self.hosted() {
            return self.answer_to_host(engine, text, cx);
        }
        if self.state.add_text(engine, text) {
            self.ask(engine, cx);
            cx.notify();
        }
    }

    /// A composer button was pressed.
    pub(crate) fn composer_action(&mut self, action: dry_core::composer::Action, window: &mut Window, cx: &mut Context<Self>) {
        use dry_core::composer::Action;
        match action {
            Action::Send => return self.send(window, cx),
            Action::Clear => {
                self.input.update(cx, |input, cx| input.set_value("", window, cx));
                self.composer_note = None;
            }
            // The check passed (it is on); recording itself is the next step.
            Action::Voice => {
                self.composer_note = Some("Voice: microphone and voice.json programs found; recording is not wired yet".into())
            }
        }
        cx.notify();
    }

    /// Send engine `engine`'s texts, as one conversation, through its
    /// inference object; the reply is saved as the next text. The request
    /// runs in the background so the window keeps drawing while the model
    /// thinks.
    fn ask(&mut self, engine: usize, cx: &mut Context<Self>) {
        self.state.equip_engines();
        // A host runs every turn (`client.rs`): never two writers.
        if self.hosted() {
            return;
        }
        let e = &self.state.engines[engine];
        if let Some(session) = e.session.clone() {
            return self.ask_session(engine, session, cx);
        }
        if e.inference.model.is_empty() {
            self.engine_status.insert(
                engine,
                Status::Missing("no model: set the default in models.json (Settings → Models), then start a new thread".into()),
            );
            return;
        }
        self.engine_status.remove(&engine);
        self.ask_step(engine, 1, false, cx);
    }

    /// Step `n` of engine `engine`'s turn, as phases the window repaints
    /// between (`dry_core::activity`): measure the prompt against the
    /// model's context and compress the history if the core says so
    /// (`compress`), then call the LLM, then run the answer's calls
    /// (`run_calls`), then the LLM again, until a reply. No step limit:
    /// the context window is the only limit. `compressed`: this step
    /// already compressed once, so a prompt still over is sent as it is.
    fn ask_step(&mut self, engine: usize, n: usize, compressed: bool, cx: &mut Context<Self>) {
        use dry_core::context::{self, Plan};
        let e = &self.state.engines[engine];
        let ai = e.inference.clone();
        let name = e.name.clone();
        // The window: known, else ask the server once, then come back.
        let key = format!("{} {}", ai.base_url, ai.model);
        let Some(window) = self.windows.get(&key).cloned() else {
            let probes = context::probes(&ai);
            self.engine_activity.insert(engine, Activity::Working("asking the server for the model's context size".into()));
            cx.spawn(async move |this, cx| {
                let model = ai.model.clone();
                let found = cx
                    .background_executor()
                    .spawn(async move {
                        probes.iter().find_map(|p| {
                            let body = match p.post() {
                                Some(req) => net::post(&req),
                                None => net::get(&p.url, &p.key_env),
                            };
                            context::read_probe(p.server, &body.ok()?, &model)
                        })
                    })
                    .await;
                this.update(cx, |app, cx| {
                    app.windows.insert(key, context::Window::of(&ai, found));
                    if app.state.engines.get(engine).is_some_and(|e| e.name == name) {
                        app.ask_step(engine, n, compressed, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        };
        // The core renders and decides; the shell only moves bytes.
        // File and terminal artifacts read the disk at render (`live.rs`);
        // a clock artifact reads the real clock; engines and shared ones
        // read the family.
        let disk = self.disk();
        let family = self.family(engine);
        let at = dry_core::live::Snapshots { clock: Some(dry_tools::live::clock()), host: Some(&disk), family: Some(&family) };
        let rendered = dry_core::prompt::render_with(e, &at);
        let (budget, over) = match context::plan(e, &rendered, &window) {
            Plan::Compress(c) if !compressed => return self.compress(engine, n, c, cx),
            Plan::Compress(c) => (c.budget, Some("still over the threshold after compressing; sent as it is".to_string())),
            Plan::Over(b, why) => (b, Some(why)),
            Plan::Send(b) => (b, None),
        };
        let last = self.engine_context.remove(&engine).and_then(|c| c.compressed);
        let tokens = budget.tokens;
        self.engine_context.insert(engine, EngineContext { budget, over, compressed: last });
        if let Some(tr) = self.trace(engine, n) {
            tr.post(spans::now(), n, false, compressed, Some(tokens));
        }
        let e = &self.state.engines[engine];
        self.take_inspect(engine, format!("step {n}, as sent"), Some(&rendered));
        let req = ai.request(&rendered.messages());
        self.engine_activity.insert(engine, Activity::Llm { step: n });
        cx.notify();
        // The answer streams (`dry_core::stream`): read on a background
        // thread as it comes, its view sent here to draw (every 50 ms), its
        // instant calls run as each closes (`dry_core::early`, kept only if
        // the whole answer reads with the same calls). A server that does
        // not stream gives the whole body at once.
        let streamed = e.clone();
        let servers = self.mcp.get(&e.id).cloned();
        let disk = self.disk();
        let (tx, rx) = std::sync::mpsc::channel::<dry_core::stream::View>();
        let traced_ai = ai.clone();
        cx.spawn(async move |this, cx| {
            let task = cx.background_executor().spawn(async move {
                let mut reader = dry_core::stream::Reader::new(&streamed);
                let mut early = dry_core::early::Early::new(&streamed);
                let tools = streamed.mcp_tools.clone();
                let mut mcp = |call: &dry_core::calls::Call| dry_tools::capability::route_local(call).or_else(|| servers.as_ref()?.call(&tools, call));
                let (mut first, mut component) = (None, None);
                let body = dry_tools::net::stream(&dry_core::stream::streaming(&req), &mut |d| {
                    if matches!(d, dry_core::stream::Delta::Text(_)) {
                        first.get_or_insert_with(spans::now);
                    }
                    for ev in reader.feed(&d) {
                        if let dry_core::stream::Event::CallDone { call, .. } = ev {
                            early.offer(&call, &mut mcp, &disk);
                        }
                    }
                    if reader.view.has_component() {
                        component.get_or_insert_with(spans::now);
                    }
                    let _ = tx.send(reader.view.clone());
                });
                let reply = body.clone().and_then(|b| ai.reply(&b).map_err(|e| format!("{e:?}")));
                (reply, early, body, first, component)
            });
            // Draw the answer as it grows; the sender goes when it is in.
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
                let mut latest = None;
                let over = loop {
                    match rx.try_recv() {
                        Ok(v) => latest = Some(v),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break false,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break true,
                    }
                };
                if let Some(v) = latest {
                    this.update(cx, |app, cx| {
                        app.streaming.insert(engine, v);
                        cx.notify();
                    })
                    .ok();
                }
                if over {
                    break;
                }
            }
            let (done, early, body, first, component) = task.await;
            this.update(cx, |app, cx| {
                app.streaming.remove(&engine);
                // The engine may have been deleted while we waited; only
                // answer it if the same engine is still at this index.
                if app.state.engines.get(engine).is_none_or(|e| e.name != name) {
                    return;
                }
                if let Some(tr) = app.trace(engine, n) {
                    first.into_iter().for_each(|f| tr.first_token(f));
                    component.into_iter().for_each(|c| tr.first_component(c));
                    let unread = done.as_ref().err().filter(|_| body.is_ok()).cloned();
                    tr.body(spans::now(), &body, &traced_ai, unread);
                }
                match done {
                    Ok(text) => app.run_calls(engine, n, text, Some(early), cx),
                    Err(e) => app.fail(engine, e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The model answered step `n` with `text`: show its calls as the
    /// activity (the window repaints), then run the step in two halves
    /// (`dry_core::turn`, docs/engine-standard.md, "One step, many calls"). First, in the
    /// background on a copy of the engine, every instant call in written
    /// order (`begin_step`); the copy goes back, and the children it asked
    /// start now. Then every waiting call (a `run`, an MCP call) at once,
    /// one thread each (`dry_tools::live::run_waits`), and when all are
    /// back their results merge in written order (`finish_step`) and the
    /// turn goes on: the model again, a wait for children, or the reply.
    fn run_calls(&mut self, engine: usize, n: usize, text: String, early: Option<dry_core::early::Early>, cx: &mut Context<Self>) {
        use dry_core::turn;
        if let Some(tr) = self.trace(engine, n) {
            tr.answered(spans::now());
        }
        let e = &self.state.engines[engine];
        self.engine_activity.insert(engine, Activity::answered(e, &text, n));
        let name = e.name.clone();
        let mut copy = e.clone();
        let servers = self.mcp.get(&e.id).cloned();
        let disk = self.disk();
        // The other engines, for the engines and shared tools; what they
        // ask (a child made, a post) comes back in its effects.
        let mut family = self.family(engine);
        cx.spawn(async move |this, cx| {
            let (copy, pending, family) = cx
                .background_executor()
                .spawn(async move {
                    // Only a script's steps go out here; the act's own
                    // waiting calls go out together below.
                    let tools = copy.mcp_tools.clone();
                    let mut mcp = |call: &dry_core::calls::Call| dry_tools::capability::route_local(call).or_else(|| servers.as_ref()?.call(&tools, call));
                    // What ran while the answer streamed is kept if the whole answer has the same calls.
                    let pending = match early {
                        Some(early) => early.finish(&mut copy, &text, &mut mcp, &disk, Some(&mut family)),
                        None => turn::begin_step(&mut copy, &text, &mut mcp, &disk, Some(&mut family)),
                    };
                    (copy, pending, family)
                })
                .await;
            this.update(cx, |app, cx| {
                if app.state.engines.get(engine).is_none_or(|e| e.name != name) {
                    return;
                }
                app.state.engines[engine] = copy;
                // Posts into its level's context; children made or given a
                // message run their own turns now, the same way as any
                // engine, while this step's waiting calls wait.
                let applied = dry_core::engines::apply(&mut app.state, engine, family.effects);
                for note in &applied.notes {
                    eprintln!("{name}: {note}");
                }
                for child in applied.run {
                    app.ask(child, cx);
                }
                app.wait_calls(engine, n, pending, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The second half of step `n`: every waiting call at once, then the
    /// results in written order, then the turn goes on.
    fn wait_calls(&mut self, engine: usize, n: usize, pending: dry_core::turn::Pending, cx: &mut Context<Self>) {
        let e = &self.state.engines[engine];
        let name = e.name.clone();
        if pending.waits.is_empty() {
            return self.finish_calls(engine, n, pending, Vec::new(), cx);
        }
        let servers = self.mcp.get(&e.id).cloned();
        let tools = e.mcp_tools.clone();
        let disk = self.disk();
        let waits = pending.waits.clone();
        if let Some(tr) = self.trace(engine, n) {
            tr.run(spans::now(), pending.waiting_names());
        }
        cx.spawn(async move |this, cx| {
            let (came, times): (Vec<_>, Vec<_>) = cx
                .background_executor()
                .spawn(async move {
                    let mcp = |call: &dry_core::calls::Call| dry_tools::capability::route_local(call).or_else(|| servers.as_ref()?.call(&tools, call));
                    dry_tools::live::run_waits_timed(&waits, &mcp, &disk).into_iter().unzip()
                })
                .await;
            this.update(cx, |app, cx| {
                if app.state.engines.get(engine).is_none_or(|e| e.name != name) {
                    return;
                }
                if let Some(tr) = app.trace(engine, n) {
                    tr.wait_times(times);
                }
                app.finish_calls(engine, n, pending, came, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Step `n`'s results are in: saved as one message, then the model
    /// again, a wait for children, or the end of the turn.
    fn finish_calls(&mut self, engine: usize, n: usize, pending: dry_core::turn::Pending, came: Vec<dry_core::turn::Came>, cx: &mut Context<Self>) {
        let before = self.state.engines[engine].messages.len();
        let step = dry_core::turn::finish_step(&mut self.state.engines[engine], pending, came);
        self.take_inspect(engine, format!("after step {n}"), None);
        {
            let e = &self.state.engines[engine];
            let results: Vec<&str> = e.messages[before.min(e.messages.len())..].iter().filter(|m| m.role == dry_core::inference::Role::Tool).map(|m| m.text.as_str()).collect();
            let results = results.join("\n");
            let phase = e.phase_list().ok().and_then(|p| p.get(e.run.phase).map(|p| p.name.clone())).unwrap_or_default();
            if let Some(tr) = self.trace(engine, n) {
                tr.results(spans::now(), n, &results, &phase);
            }
        }
        match Activity::after(&step, n) {
            Activity::Llm { step } => self.ask_step(engine, step, false, cx),
            // An engine_call waits: its children's replies fill its lines
            // and the turn goes on (`live.rs`, `settle_callbacks`).
            waiting @ Activity::Waiting(_) => {
                self.engine_activity.insert(engine, waiting);
                let on = self.state.engines[engine].waiting.join(", ");
                self.end_trace(engine, "waiting", dry_core::spans::attrs(&[("waiting_on", &on)]));
                self.settle_callbacks(cx);
            }
            _ => {
                let more = dry_core::spans::reply_attrs(&self.state.engines[engine]);
                self.end_trace(engine, "ready", more);
                self.engine_activity.remove(&engine);
                self.engine_status.insert(engine, Status::Ready);
                // A parent waiting on this one hears now, not at the tick.
                self.settle_callbacks(cx);
            }
        }
    }

    /// The prompt for step `n` is over the threshold: the same model
    /// summarises the older history (`dry_core::context`), the summary goes
    /// into the log at the cut (every message is kept), and the step goes
    /// on with the smaller prompt.
    fn compress(&mut self, engine: usize, n: usize, c: dry_core::context::Compression, cx: &mut Context<Self>) {
        let e = &self.state.engines[engine];
        let ai = e.inference.clone();
        let name = e.name.clone();
        let req = ai.request(&c.request);
        self.engine_activity.insert(engine, Activity::Compressing { step: n });
        if let Some(tr) = self.trace(engine, n) {
            tr.post(spans::now(), n, true, false, Some(c.budget.tokens));
        }
        let traced_ai = ai.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (done, body) = cx
                .background_executor()
                .spawn(async move {
                    let body = net::post(&req);
                    (body.clone().and_then(|b| ai.reply(&b).map_err(|e| format!("{e:?}"))), body)
                })
                .await;
            this.update(cx, |app, cx| {
                if app.state.engines.get(engine).is_none_or(|e| e.name != name) {
                    return;
                }
                if let Some(tr) = app.trace(engine, n) {
                    tr.body(spans::now(), &body, &traced_ai, done.as_ref().err().filter(|_| body.is_ok()).cloned());
                }
                let note = match done {
                    Ok(summary) => dry_core::context::apply(&mut app.state.engines[engine], &c, &summary),
                    Err(e) => Err(format!("the summary call failed: {e}")),
                };
                let note = note.unwrap_or_else(|why| format!("not compressed: {why}"));
                app.engine_context.insert(engine, EngineContext { budget: c.budget.clone(), over: None, compressed: Some(note) });
                app.ask_step(engine, n, true, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A turn of engine `engine` failed: why, and nothing running.
    fn fail(&mut self, engine: usize, why: String) {
        let short = dry_core::spans::short(&why, 160);
        self.end_trace(engine, "failed", dry_core::spans::attrs(&[("error", &short)]));
        self.engine_activity.remove(&engine);
        self.engine_status.insert(engine, Status::Failed(why));
    }

    /// Engine `engine`'s activity now (its turn, else what was reported,
    /// else a terminal callback it waits on).
    pub(crate) fn activity(&self, engine: usize) -> Activity {
        match self.state.engines.get(engine) {
            Some(e) => dashboard::live_activity(e, self.engine_status.get(&engine), self.engine_activity.get(&engine)),
            None => Activity::Idle,
        }
    }

    /// Engine `engine`'s live status (`dashboard::live_status`).
    pub(crate) fn live_status(&self, engine: usize) -> Status {
        match self.state.engines.get(engine) {
            Some(e) => dashboard::live_status(e, self.engine_status.get(&engine), self.engine_activity.get(&engine)),
            None => Status::Idle,
        }
    }

    /// Statuses and activities are kept by index: after an engine is
    /// removed they no longer match, so they go.
    pub(crate) fn forget_engine_states(&mut self) {
        self.engine_status.clear();
        self.engine_activity.clear();
        self.engine_context.clear();
        self.inspects.get_mut().taken.clear();
    }
}

/// An engine's prompt size against its model's context at the last call.
#[derive(Clone, Debug)]
pub(crate) struct EngineContext {
    pub budget: dry_core::context::Budget,
    /// Over the threshold and not compressed: why.
    pub over: Option<String>,
    /// The last compression, as a line (or why it did not happen).
    pub compressed: Option<String>,
}

impl AppView {
    /// A session engine's turn: send only the newest text to its program
    /// (the program keeps the conversation under its own session id), then
    /// save the reply, the id for next time, and everything it reported.
    fn ask_session(&mut self, engine: usize, session: dry_core::session::SessionRef, cx: &mut Context<Self>) {
        let tool = match self.sessions.as_ref().map(|s| s.get(&session.tool).cloned()) {
            Ok(Some(tool)) => tool,
            Ok(None) => {
                self.engine_status.insert(engine, Status::Missing(format!("no program \"{}\" in sessions.json", session.tool)));
                return;
            }
            Err(e) => {
                self.engine_status.insert(engine, Status::Missing(e.clone()));
                return;
            }
        };
        let e = &self.state.engines[engine];
        let prompt = e.messages.iter().rev().find(|m| m.role == Role::User).map(|m| m.text.clone()).unwrap_or_default();
        let name = e.name.clone();
        let meta_file = tool.wants_meta_file().then(|| program::meta_file(engine));
        // Its settings file (the hooks that push dry's snapshot) and its
        // environment, as the host gives a session (`dry_tools::worker`).
        let prepared = dry_tools::worker::prepare(&tool, &e.id, &prompt, session.id.as_deref(), &meta_file.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        let (argv, env) = match prepared {
            Ok(p) => (p.argv, p.env),
            Err(why) => {
                self.engine_status.insert(engine, Status::Failed(why));
                return;
            }
        };
        self.engine_status.remove(&engine);
        // Its program reports only when it ends: until then, working.
        self.engine_activity.insert(engine, Activity::session(&session.tool, None));
        cx.spawn(async move |this, cx| {
            let run = cx.background_executor().spawn(async move { program::run(&argv, &env, meta_file.as_deref()) }).await;
            this.update(cx, |app, cx| {
                if app.state.engines.get(engine).is_none_or(|e| e.name != name) {
                    return;
                }
                app.engine_activity.remove(&engine);
                let (turn, failed) = match run {
                    Ok(run) => {
                        let mut turn = tool.read(&run.stdout, run.meta.as_deref());
                        turn.meta.extend(run.facts());
                        let failed = match (&turn.reply, &turn.error) {
                            (_, Some(err)) => Some(err.clone()),
                            (Some(_), None) => None,
                            (None, None) => Some(format!("no reply (exit {}): {}", run.code_text(), run.stderr_tail())),
                        };
                        (turn, failed)
                    }
                    Err(e) => (dry_core::session::Turn::default(), Some(e)),
                };
                let e = &mut app.state.engines[engine];
                if let (Some(id), Some(s)) = (&turn.id, e.session.as_mut()) {
                    s.id = Some(id.clone());
                }
                if !turn.meta.is_empty() {
                    e.meta = turn.meta;
                }
                if let Some(reply) = &turn.reply {
                    app.state.add_reply(engine, reply);
                }
                match failed {
                    Some(why) => app.engine_status.insert(engine, Status::Failed(why)),
                    None => app.engine_status.insert(engine, Status::Ready),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Where each program in `sessions.json` is installed (its command's first word).
fn find_programs(sessions: &Result<Sessions, String>) -> HashMap<String, Option<std::path::PathBuf>> {
    let Ok(sessions) = sessions else { return HashMap::new() };
    sessions.tools.iter().map(|t| (t.name.clone(), t.start.first().and_then(|bin| dry_tools::find_program(bin)))).collect()
}

/// The default model setting's inference object, if `models.json` has a
/// usable one.
fn default_inference(models: &Models) -> Option<dry_core::inference::OpenAi> {
    models.default_model().and_then(|m| models.inference(m).ok())
}

/// What kind of value a UI-page row edits.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TokenKind {
    Color,
    Number,
    Font,
    /// An app setting, not a token: the scene and the motion (app.md,
    /// saved with the state).
    Setting,
}

/// One row of the UI page: a token and the box that edits it.
pub(crate) struct TokenInput {
    pub kind: TokenKind,
    pub name: &'static str,
    pub input: Entity<InputState>,
}

/// A colour token's value by name, read from resolved tokens.
pub(crate) fn token_color(t: &Tokens, name: &str) -> u32 {
    t.color(name).unwrap_or(0)
}

/// A size token's value by name, read from resolved tokens.
pub(crate) fn token_number(t: &Tokens, name: &str) -> f32 {
    t.number(name).unwrap_or(0.0)
}

/// A small clickable box, drawn from the tokens. `id` must be unique in the
/// window so GPUI can tell which one was clicked.
pub(crate) fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, t: &Tokens) -> Stateful<Div> {
    // A glass capsule: 30 high, `radius` (15) makes it round-ended.
    let hover = color(t.overlay_pressed);
    div()
        .id(id.into())
        .flex_none()
        .h(px(30.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(t.radius))
        .bg(color(t.glass_sheen))
        .border_1()
        .border_color(color(t.glass_edge))
        .shadow(vec![gpui::BoxShadow {
            color: color(t.shadow),
            offset: gpui::point(px(0.), px(1.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        }])
        .hover(move |s| s.bg(hover))
        .cursor_pointer()
        .font_weight(FontWeight::MEDIUM)
        .child(label.into())
}

/// A pill button: a 36×36 icon cell, plus the label when the pill is
/// open. The current page is a tinted glass capsule with an accent rim.
fn nav(id: impl Into<ElementId>, icon: &str, label: &str, open: bool, active: bool, t: &Tokens) -> Stateful<Div> {
    let hover = color(t.overlay_hover);
    div()
        .id(id.into())
        .h(px(36.))
        .flex()
        .items_center()
        .rounded(px(18.))
        .cursor_pointer()
        .border_1()
        .border_color(gpui::transparent_black())
        .when(active, |d| d.bg(color(t.accent_muted)).border_color(color(t.accent)).text_color(color(t.text_primary)))
        .when(!active, |d| d.text_color(color(t.text_secondary)).hover(move |s| s.bg(hover)))
        .child(div().size(px(34.)).flex_none().flex().items_center().justify_center().text_size(px(17.)).child(icon.to_string()))
        .when(open, |d| d.child(div().flex_1().min_w_0().pr(px(12.)).truncate().font_weight(FontWeight::MEDIUM).child(label.to_string())))
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The tokens injected at the top (see `ui`). Everything below draws
        // with these and nothing else.
        let t = ui::tokens(cx);

        // GPUI calls `render` again whenever the window is resized, so
        // reading the size here is all it takes to follow the screen.
        let width = f32::from(window.viewport_size().width);
        // The navigation's style comes from the theme (pinned sidebar, or a
        // header button); the view state says whether that button is
        // pressed. The frame decides where it then opens: a side panel, or
        // an overlay on a compact screen.
        let style = layout::NavStyle::from_token(t.nav_pinned);
        let pressed = self.view.panel == Some(true);
        let mut frame = layout::frame(Screen::from_width(width), pressed, style);
        if let Some((_, _, view)) = self.router.resolve(&self.view.location) {
            if dry_core::pages::fills(&view) {
                frame = frame.filling();
            } else if dry_core::pages::wide(&view) {
                frame = frame.widened();
            }
        }
        self.content_width = frame.content_width(width);
        let overlay = frame.placement == layout::Placement::Overlay;
        let viewport = (width, f32::from(window.viewport_size().height));

        // The centre: resolve the path to a route, then draw that route's
        // view. No route: the "not found" view.
        let location = self.view.location.clone();
        let resolved = self.router.resolve(&location);
        let body = match &resolved {
            Some((_, params, view)) => views::draw(view, self, params, &t, window, cx),
            None => views::missing(&location, &t),
        };
        // The trail names each level by what it shows: an engine by its
        // name, an artifact by its own (Home › Engines › Builder › code).
        let names: Vec<&str> = self.state.engines.iter().map(|e| e.name.as_str()).collect();
        let mut trail = routes::trail_titled(self.router.table(), &location, &names);
        let here = resolved.is_some() && trail.last().is_some_and(|(p, _)| *p == location);
        let title = match resolved {
            Some(_) if here => trail.pop().map(|(_, title)| title).unwrap_or_default(),
            Some((route, _, _)) => route.title.clone(),
            None => "Not found".to_string(),
        };

        // Header row, on the pane: the navigation button (when the
        // navigation floats), the page title, and Save.
        let header = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(t.space_2))
            .when(frame.header_button(), |d| {
                d.child(
                    button("header-nav", if pressed { "✕  Close" } else { "☰  Menu" }, &t)
                        .when(pressed, |b| b.bg(color(t.accent_muted)).border_color(color(t.accent)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.view.panel = (!pressed).then_some(true);
                            cx.notify();
                        })),
                )
            })
            .child({
                // The trail: every level above the page is a link back up
                // (Home › Settings › UI tokens); the page itself, bold,
                // last. A page that is no route: every level is a link.
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(t.space_1))
                    .text_size(px(t.text_lg))
                    .children(trail.into_iter().enumerate().map(|(i, (path, name))| {
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(t.space_1))
                            .child(
                                div()
                                    .id(SharedString::from(format!("trail-{i}")))
                                    .cursor_pointer()
                                    .text_color(color(t.text_secondary))
                                    .hover(|d| d.text_color(color(t.text_accent)))
                                    .child(name)
                                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(&path, window, cx))),
                            )
                            .child(div().text_color(color(t.text_disabled)).child("›"))
                    }))
                    .child(div().min_w_0().truncate().font_weight(FontWeight::BOLD).child(title))
            })
            .child(button("save", "Save", &t).bg(color(t.accent)).text_color(color(t.on_accent)).on_click(cx.listener(
                |this, _, _, cx| {
                    if this.hosted() {
                        this.composer_note = Some("A server hosts the store and saves it; this window does not write over it.".into());
                        cx.notify();
                        return;
                    }
                    if let Some(e) = &this.store.1 {
                        eprintln!("not saving: the store did not read ({e})");
                        return;
                    }
                    match store::save(&this.state) {
                        Ok(()) => this.saved = this.state.clone(),
                        Err(e) => this.store.1 = Some(format!("save failed: {e}")),
                    }
                    cx.notify();
                },
            )));

        // The navigation: the route elements, made by the core from the
        // route table, the view state and the engines. The fixed routes
        // first; then, under LIVE, one per engine with a dot in its live
        // status's tone, and New thread / New session (actions, not pages).
        let names: Vec<&str> = self.state.engines.iter().map(|e| e.name.as_str()).collect();
        let elements = routes::elements(self.router.table(), &self.view, &names);
        let mut entries: Vec<gpui::AnyElement> = Vec::new();
        let label = |text: &str| {
            div().px(px(12.)).pt(px(t.space_3)).pb(px(2.)).text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(text.to_string())
        };
        let mut live_started = false;
        for el in &elements {
            if el.engine.is_some() && !live_started {
                live_started = true;
                entries.push(label("LIVE").into_any_element());
            }
            let path = el.path.clone();
            // An engine's entry: its activity's tint behind it and a few
            // words after its name, then the dot in its status's tone.
            let activity = el.engine.map(|i| self.activity(i)).unwrap_or_default();
            let label = match activity {
                Activity::Idle => el.label.clone(),
                ref a => format!("{} · {}", el.label, a.short()),
            };
            let mut item = nav(SharedString::from(format!("nav-{}", el.path)), &el.icon, &label, true, el.active, &t);
            if let Some(i) = el.engine {
                if let Some(tint) = activity.token().and_then(|name| t.color(name)) {
                    item = item.bg(color(tint));
                }
                let status = self.live_status(i);
                item = item.child(
                    div().flex_none().pr(px(12.)).text_size(px(9.)).text_color(color(views::card::tone_color(status.tone(), &t))).child("●"),
                );
            }
            entries.push(
                item.on_click(cx.listener(move |this, _, window, cx| {
                    if overlay {
                        this.view.panel = None;
                    }
                    this.navigate(&path, window, cx)
                }))
                .into_any_element(),
            );
        }
        if !live_started {
            entries.push(label("LIVE").into_any_element());
        }
        entries.push(
            nav("nav-new-thread", "＋", "New thread", true, false, &t)
                .text_color(color(t.text_secondary))
                .on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx)))
                .into_any_element(),
        );
        // New session: one per program in sessions.json (the picker when
        // there are several).
        let programs: Vec<(String, String)> = self.sessions.as_ref().map_or(Vec::new(), |s| s.tools.iter().map(|t| (t.icon.clone(), t.name.clone())).collect());
        let one = programs.len() == 1;
        for (icon, name) in programs {
            let label = if one { "New session".to_string() } else { format!("New {name} session") };
            let tool = name.clone();
            entries.push(
                nav(SharedString::from(format!("nav-new-session-{name}")), &icon, &label, true, false, &t)
                    .text_color(color(t.text_secondary))
                    .on_click(cx.listener(move |this, _, window, cx| this.new_session(&tool, window, cx)))
                    .into_any_element(),
            );
        }
        let panel = div().flex().flex_col().gap(px(4.)).children(entries);

        // The scene setting's scene, fed how the engines are doing (the
        // tiles' rule) and whether motion is off. A footer scene covers the
        // footer row; any other, the whole window.
        let statuses: Vec<Status> = (0..self.state.engines.len()).map(|i| self.live_status(i)).collect();
        let mood = dry_core::scene::mood(&statuses);
        let look = self.scene_settings();
        let wanted = look.scene.as_deref().and_then(|name| self.scenes.get(name)).and_then(|s| s.as_ref().ok());
        let place = wanted.map(|s| s.place);
        // Its floor: the footer row's top edge, so a walking scene stands on
        // the footer instead of hiding under it.
        let (area, floor) = match place {
            Some(dry_core::scene::Place::Footer) => ((viewport.0 - frame.content_left() - layout::INSET, layout::FOOTER_HEIGHT), layout::FOOTER_HEIGHT - 1.),
            _ => (viewport, viewport.1 - layout::INSET - layout::FOOTER_HEIGHT),
        };
        self.scene_view.update(cx, |view, cx| view.sync(wanted, mood, look.still(), area, floor, cx));
        let scene = place.map(|p| (p, self.scene_view.clone().into_any_element()));
        // Footer row: whether everything is saved.
        let footer = div().truncate().child(match &self.store {
            (_, Some(e)) => format!("Store problem: {e}"),
            (name, None) if self.hosted() => format!("Hosted by {} · sending through it · {name}", self.host.as_ref().map_or("", |h| h.url.as_str())),
            (name, None) => format!("{} · {name}", if self.unsaved() { "Unsaved changes" } else { "Saved" }),
        });

        frame::draw(
            &frame,
            &t,
            viewport,
            Parts {
                panel: panel.into_any_element(),
                header: Some(header.into_any_element()),
                body: body.into_any_element(),
                footer: Some(footer.into_any_element()),
                scene,
                motion: look.motion,
            },
        )
    }
}

/// `w×h` in logical pixels (the units layout uses; on a Retina screen one
/// logical pixel is two physical ones).
fn wh(size: gpui::Size<Pixels>) -> String {
    format!("{:.0}×{:.0}", f32::from(size.width), f32::from(size.height))
}

/// Facts about the screen showing this window, one line each. Everything
/// here is asked of GPUI, which asks the operating system.
pub(crate) fn screen_info(window: &Window, cx: &App) -> Vec<String> {
    let mut lines = Vec::new();

    // Where the window sits on the whole desktop, and its full size
    // (title bar included; the viewport is the inside only).
    let win = window.bounds();
    lines.push(format!("Window: {} at ({:.0}, {:.0})", wh(win.size), f32::from(win.origin.x), f32::from(win.origin.y)));

    // Every monitor attached, and which one holds this window.
    let displays = cx.displays();
    let primary = cx.primary_display().map(|d| d.id());
    match window.display(cx) {
        Some(display) => {
            let index = displays.iter().position(|d| d.id() == display.id()).map_or(0, |i| i + 1);
            let main = if Some(display.id()) == primary { ", primary" } else { "" };
            lines.push(format!("Display {index} of {}{main}", displays.len()));
            lines.push(format!("Display size: {}", wh(display.bounds().size)));
            // The part not covered by the menu bar and Dock.
            lines.push(format!("Usable area: {}", wh(display.visible_bounds().size)));
        }
        None => lines.push(format!("Display: unknown ({} attached)", displays.len())),
    }

    // Physical pixels per logical pixel: 2 on Retina, 1 on most external screens.
    lines.push(format!("Scale factor: {}×", window.scale_factor()));
    if let Some(frame) = window.refresh_interval(cx) {
        lines.push(format!("Refresh rate: {:.0} Hz", 1.0 / frame.as_secs_f64()));
    }
    lines.push(format!("Appearance: {:?}", window.appearance()));
    lines
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        // gpui-component must be set up once before any of its components.
        gpui_component::init(cx);

        // Load the state first: its saved mode decides the tokens, which are
        // injected app-wide before the first frame is drawn.
        // Fonts shipped in assets/fonts (Amarante) must be loaded before the
        // tokens check whether their font family exists.
        ui::load_fonts(cx);
        let store::Loaded { state, store: store_name, error: store_error } = store::load();
        let mut state = state;
        if let Some(e) = &store_error {
            eprintln!("the store did not read: {e}");
        }
        // Every colour, size and the font come from the active theme file.
        let theme = store::load_theme(&state).unwrap_or_else(|e| {
            eprintln!("{e}");
            theme::Theme::default()
        });
        // Model settings come only from models.json; without it the app
        // runs with none and Settings says why.
        let (models, models_error) = match store::load_models() {
            Ok(models) => (models, None),
            Err(e) => {
                eprintln!("{e}");
                (Models::default(), Some(e))
            }
        };
        // Nothing saved yet: start with one engine on the default model, so
        // there is always somewhere to type.
        if let Some(i) = state.ensure_engine()
            && let Some(ai) = default_inference(&models)
        {
            state.engines[i].inference = ai;
        }
        let local_theme = store::load_local_theme().unwrap_or_else(|e| {
            eprintln!("{e}");
            None
        });
        ui::apply(state.mode, &theme::layered(&theme, &local_theme.iter().collect::<Vec<_>>()), local_theme.as_ref(), cx);

        // The size it was left at (`window.host.json`, this machine's
        // own), else most of the screen: never the old small window.
        let bounds = window_place::saved().unwrap_or_else(|| {
            let screen = cx.primary_display().map(|d| d.bounds().size).unwrap_or(size(px(1440.), px(900.)));
            let (w, h) = (f32::from(screen.width) * 0.85, f32::from(screen.height) * 0.85);
            WindowBounds::Windowed(Bounds::centered(None, size(px(w.max(1100.)), px(h.max(720.))), cx))
        });
        let options = WindowOptions {
            window_bounds: Some(bounds),
            // Glass: the OS blurs whatever is behind the window (macOS
            // vibrancy; Mica on Windows 11), and our translucent colours
            // let it show through.
            window_background: WindowBackgroundAppearance::Blurred,
            // No title bar strip: the panes reach the top edge and the
            // traffic lights float over the left panel.
            titlebar: Some(TitlebarOptions {
                title: Some("dry".into()),
                appears_transparent: true,
                // Inside the pill's lane, clear of the content pane.
                traffic_light_position: Some(point(px(12.), px(16.))),
            }),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let view = cx.new(|cx| AppView::new(state, theme, models, models_error, (store_name, store_error), window, cx));
            // Each engine starts its MCP servers (its spec's mcp.json).
            view.update(cx, |app, cx| app.start_all_mcp(cx));
            // gpui-component's components (the text area's menus, popovers)
            // need its `Root` as the window's top view, wrapping ours.
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("open window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
