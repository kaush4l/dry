//! Everything a page is drawn from, as plain data and three seams. A shell
//! fills it: the server from disk (`crates/server/src/load.rs`), the
//! browser from its bundled files and its store (`crates/web`).
//!
//! - `host`: files and terminals for the artifacts' snapshots
//!   (`dry_core::live::Host`). The server's refuses writes and commands;
//!   the browser's refuses files and commands, saying why.
//! - `repo`: the folder tree the Code page shows (`Repo`). The server's is
//!   the checkout; the browser's is the app's own bundled files.
//! - `access`: whether controls that change the app are live here, or drawn
//!   disabled with the reason.

use std::collections::BTreeMap;
use std::path::PathBuf;

use dry_core::activity::Activity;
use dry_core::card::Status;
use dry_core::live::{Clock, Host};
use dry_core::models::Models;
use dry_core::pages::{self, Component, ComponentFile, Local, Page};
use dry_core::routes::{Params, Route, RouteTable, ViewState};
use dry_core::runner::Measured;
use dry_core::session::Sessions;
use dry_core::state::State;
use dry_core::theme::{Overrides, Theme};

/// Marks a component the page file names but this build does not know
/// (`unknown`): it is read as a text starting with this, and the renderer
/// draws it as a placeholder naming the type. Never typed by a person.
pub const UNKNOWN: &str = "\u{FFFC}unknown-component:";

/// The one line every disabled control says on the read-only server.
pub const READ_ONLY: &str = "sending from the web arrives with the host split; send from the desktop app for now";

/// The line a control says in the browser when it needs a computer.
pub const NATIVE_ONLY: &str = "not in the browser: this needs the desktop app (a disk, processes or a program)";

/// The line a control says on the hosted server when the web cannot do
/// it yet (the host runs it, but nothing sends it there yet).
pub const HOST_LATER: &str = "not from the web yet: do this in the desktop app (it writes files the host does not take from a page yet)";

/// The controls (`data-act`) a page served by the host can use: each one
/// is an intent the host takes over HTTP (docs/web.md, "The send path").
/// The rest stay drawn off, saying `HOST_LATER` (none are left but the
/// ones that need a person at the machine: voice).
pub const HOST_ACTS: &[&str] = &[
    "send",
    "join-peer",
    "leave-peer",
    "approve",
    "new-session",
    "clear",
    "ask",
    "ask-toggle",
    "ask-pick",
    "ask-step",
    "ask-edit",
    "new-thread",
    // The old name of `new-thread`, for pages drawn before it.
    "new-engine",
    "delete-engine",
    "set-format",
    "set-mode",
    "set-scene",
    "detect-model",
    "test-model",
    "refresh",
    "reload-responses",
    "set-theme",
    "make-default",
    "route-hide",
    "route-off",
    "route-delete",
    "route-create",
    "reset-theme",
    "look-new",
    "look-rm",
    "look-import",
    "reload-spec",
    "run-terminal",
    "reset-history",
];

/// Controls only a host takes: they write the app's files on its disk
/// (`routes/`, `themes/`, `scenes/`, `state/local.md`, `models.json`), read its
/// store, or run a typed command on its machine. The browser draws them
/// off (`NATIVE_ONLY`): it has none of those.
pub const HOST_ONLY: &[&str] =
    &["make-default", "route-hide", "route-off", "route-delete", "route-create", "reset-theme", "look-new", "look-rm", "look-import", "reload-spec", "run-terminal"];

/// Whether the controls that change the app work here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    /// Drawn disabled, saying why (`READ_ONLY`): a server that is not the
    /// host (another one hosts the store, or `--read-only`).
    #[default]
    ReadOnly,
    /// This page runs the app (the browser): sending, answering, new and
    /// deleted engines, settings. What needs a computer stays off
    /// (`NATIVE_ONLY`).
    Live,
    /// The server is the host: it runs the turns and is the one writer of
    /// the store; this page sends it intents (`HOST_ACTS`) with `fetch`.
    /// What it does not take yet stays off (`HOST_LATER`).
    Host,
}

/// A folder tree to look through: the Code page's explorer and editor.
pub trait Repo {
    /// Folder `rel`'s entries: (name, is a folder).
    fn list(&self, rel: &str) -> Result<Vec<(String, bool)>, String>;
    /// File `rel`'s bytes, at most `max` of them, and its whole size.
    fn read(&self, rel: &str, max: usize) -> Result<(Vec<u8>, u64), String>;
}

/// No folder at all.
pub struct NoRepo;

impl Repo for NoRepo {
    fn list(&self, _: &str) -> Result<Vec<(String, bool)>, String> {
        Err("no folder here".into())
    }
    fn read(&self, _: &str, _: usize) -> Result<(Vec<u8>, u64), String> {
        Err("no folder here".into())
    }
}

/// Files held in memory by path (the browser's bundled app files).
#[derive(Clone, Debug, Default)]
pub struct MemoryRepo(pub BTreeMap<String, String>);

impl Repo for MemoryRepo {
    fn list(&self, rel: &str) -> Result<Vec<(String, bool)>, String> {
        let prefix = if rel.is_empty() { String::new() } else { format!("{}/", rel.trim_end_matches('/')) };
        let mut out: Vec<(String, bool)> = Vec::new();
        for path in self.0.keys().filter(|p| p.starts_with(&prefix)) {
            let rest = &path[prefix.len()..];
            let (name, dir) = match rest.split_once('/') {
                Some((d, _)) => (d, true),
                None => (rest, false),
            };
            if !out.iter().any(|(n, _)| n == name) {
                out.push((name.to_string(), dir));
            }
        }
        if out.is_empty() && !rel.is_empty() {
            return Err(format!("no folder {rel}"));
        }
        Ok(out)
    }
    fn read(&self, rel: &str, max: usize) -> Result<(Vec<u8>, u64), String> {
        let text = self.0.get(rel).ok_or_else(|| format!("no file {rel}"))?;
        Ok((text.as_bytes().iter().take(max).copied().collect(), text.len() as u64))
    }
}

/// What the shell running the turns reports per engine (by index): the
/// browser's live turns. The server has none (its world reads the store).
#[derive(Clone, Debug, Default)]
pub struct Reported {
    pub status: BTreeMap<usize, Status>,
    pub activity: BTreeMap<usize, Activity>,
    /// The prompt against the model's window at the last call.
    pub measured: BTreeMap<usize, Measured>,
    /// The answer streaming in now (`dry_core::stream`): drawn under the
    /// conversation until the answer is saved as a message.
    pub streaming: BTreeMap<usize, dry_core::stream::View>,
    /// A session program's run as far as it has gone
    /// (`dry_core::stream_json`): its tool rows and agents, drawn under the
    /// conversation until the turn ends.
    pub sessions: BTreeMap<usize, dry_core::stream_json::Live>,
    /// Approvals waiting for a person (`dry_core::approval`), oldest first.
    pub approvals: Vec<dry_core::approval::Approval>,
    /// Each engine's spans as its log holds them (`dry_core::spans`), by
    /// engine id: the Timeline, Home's today and `/analytics` read them.
    pub spans: BTreeMap<String, Vec<dry_core::spans::Span>>,
    /// The days the retention rolled up (`spans-daily.jsonl`).
    pub daily: Vec<dry_core::spans::Daily>,
    /// The host's terminal on the Code page (`dry_core::workbench::Terminal`):
    /// the commands typed into it from any page and what they printed.
    /// `None`: this shell runs no typed commands.
    pub terminal: Option<dry_core::workbench::Terminal>,
}

impl Reported {
    /// Every engine's spans in one list.
    pub fn all_spans(&self) -> Vec<dry_core::spans::Span> {
        self.spans.values().flatten().cloned().collect()
    }
}

/// The browser's way to its model server (Settings → Models): where, which
/// model, and what the last check said. The key, if one was ever entered,
/// is never drawn: only whether there is one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Connection {
    /// Up to and including `/v1`.
    pub endpoint: String,
    pub model: String,
    pub has_key: bool,
    /// The models the server listed at the last Detect, or why not.
    pub detected: Option<Result<Vec<String>, String>>,
    /// The last Detect or Test, as a status.
    pub check: Option<Status>,
}

/// What the app is made of, as the shell has it now.
pub struct World {
    pub table: RouteTable,
    pub pages: BTreeMap<String, Page>,
    pub components: BTreeMap<String, Vec<Component>>,
    pub local: BTreeMap<String, Local>,
    /// Files that did not read (shown in Settings, like the window does).
    pub errors: Vec<String>,
    /// Hidden and turned-off routes, from `state/local.md`.
    pub view: ViewState,
    pub state: State,
    /// Which store (`md:/…/state`, `indexeddb:dry`), for the footer.
    pub store: String,
    /// Why the store did not read.
    pub store_error: Option<String>,
    pub theme: Result<Theme, String>,
    /// `state/local.md` as overrides.
    pub local_theme: Option<Overrides>,
    pub themes: Vec<String>,
    /// Each theme by name, for previews (`?theme=`).
    pub named_themes: BTreeMap<String, Result<Theme, String>>,
    pub active_theme: String,
    /// Every scene by name (`scenes/<name>.md`), or why it does not read.
    pub scenes: BTreeMap<String, Result<dry_core::scene::Scene, String>>,
    pub models: Result<Models, String>,
    pub sessions: Result<Sessions, String>,
    /// Program → where it was found on `PATH`.
    pub programs: BTreeMap<String, Option<PathBuf>>,
    pub repo_name: String,
    /// The font files shipped (`assets/fonts/`), by name.
    pub fonts: Vec<String>,
    /// What serves this page and from where, for Diagnostics.
    pub served_by: String,
    pub host: Box<dyn Host>,
    pub clock: Clock,
    pub repo: Box<dyn Repo>,
    pub access: Access,
    pub reported: Reported,
    /// The browser's model connection (none on the server).
    pub connection: Option<Connection>,
    /// Whether this shell serves evidence images
    /// (`render::engine::evidence_url`): the server does; the browser shows
    /// their paths.
    pub evidence_files: bool,
}

impl World {
    /// A world from the app's files as texts by path (`routes/…`,
    /// `components/…`, `themes/…`, `models.json`, `sessions.json`; what
    /// the browser bundles), a state, and this machine's `local.md` (its
    /// token rows and its hidden routes). `active`: the theme chosen here,
    /// else the state's (app.md `theme:`). The host, repo and the rest are the shell's
    /// to set after; until then nothing is reached (`NoHost`, `NoRepo`).
    pub fn from_files(files: &BTreeMap<String, String>, state: State, local_md: &str, active: Option<&str>) -> World {
        let loaded = load_files(files);
        let mut view = ViewState::default();
        view.read_local_md(local_md);
        let mut table = pages::table(&loaded.pages);
        for p in &view.removed {
            table.remove(p);
        }
        use dry_core::look::{self, Kind};
        let mut named_themes = BTreeMap::new();
        for (path, text) in files.range("themes/".to_string()..).take_while(|(p, _)| p.starts_with("themes/")) {
            if let Some(name) = look::name_of(Kind::Theme, path) {
                named_themes.insert(name.clone(), Theme::from_md(text).map_err(|e| format!("{path} {e}")));
            }
        }
        let themes: Vec<String> = named_themes.keys().cloned().collect();
        let wanted = active.map(str::to_string).or_else(|| state.theme.clone());
        let active_theme = wanted.filter(|n| named_themes.contains_key(n)).unwrap_or_else(|| "default".to_string());
        let theme = named_themes.get(&active_theme).cloned().unwrap_or_else(|| Err(format!("no theme {active_theme}")));
        let local_theme = if local_md.trim().is_empty() { None } else { Overrides::from_md(local_md).ok() };
        let scenes = files
            .range("scenes/".to_string()..)
            .take_while(|(p, _)| p.starts_with("scenes/"))
            .filter_map(|(path, text)| {
                let name = look::name_of(Kind::Scene, path)?;
                let scene = dry_core::scene::Scene::from_md(&name, text).map_err(|e| format!("{path}: {e}"));
                Some((name, scene))
            })
            .collect();
        let json = |name: &str| files.get(name).ok_or_else(|| format!("no {name}"));
        World {
            table,
            pages: loaded.pages.into_iter().collect(),
            components: loaded.components,
            local: loaded.local,
            errors: loaded.errors,
            view,
            state,
            store: "memory".into(),
            store_error: None,
            theme,
            local_theme,
            themes,
            named_themes,
            active_theme,
            scenes,
            models: json("models.json").and_then(|t| Models::from_json(t).map_err(|e| format!("models.json does not parse: {e}"))),
            sessions: json("sessions.json").and_then(|t| Sessions::from_json(t).map_err(|e| format!("sessions.json: {e}"))),
            programs: BTreeMap::new(),
            repo_name: "dry".into(),
            fonts: Vec::new(),
            served_by: String::new(),
            host: Box::new(dry_core::turn::NoHost),
            clock: Clock::new(0, 0),
            repo: Box::new(NoRepo),
            access: Access::ReadOnly,
            reported: Reported::default(),
            connection: None,
            evidence_files: false,
        }
    }

    /// The route for `path`, its parameters and its view with every `use`
    /// replaced (`dry_core::pages::expand`), like the window's router.
    pub fn resolve(&self, path: &str) -> Option<(Route, Params, Vec<Component>)> {
        let (route, params) = self.table.resolve(path)?;
        let page = self.pages.get(&route.pattern)?;
        let view = pages::expand(&page.view, &pages::find(self.local.get(&route.pattern), &self.components));
        Some((route.clone(), params, view))
    }

    /// The route folder's own files for `path`, if it has any.
    pub fn local_of(&self, path: &str) -> Option<&Local> {
        let (route, _) = self.table.resolve(path)?;
        self.local.get(&route.pattern)
    }

    /// Engine names, in order (the navigation and the trail name engines by these).
    pub fn engine_names(&self) -> Vec<&str> {
        self.state.engines.iter().map(|e| e.name.as_str()).collect()
    }

    /// Why a control that is off here is off.
    pub fn off_reason(&self) -> &'static str {
        match self.access {
            Access::ReadOnly => READ_ONLY,
            Access::Live => NATIVE_ONLY,
            Access::Host => HOST_LATER,
        }
    }

    /// Does this page send (the browser running the app, or a page of the host)?
    pub fn live(&self) -> bool {
        matches!(self.access, Access::Live | Access::Host)
    }

    /// Is this a page of the host (the server runs the turns)?
    pub fn hosted(&self) -> bool {
        self.access == Access::Host
    }

    /// Does control `act` (its `data-act`) work here?
    pub fn can(&self, act: &str) -> bool {
        match self.access {
            Access::ReadOnly => false,
            Access::Live => !HOST_ONLY.contains(&act),
            Access::Host => HOST_ACTS.contains(&act),
        }
    }
}

/// The app's route and component files, read from texts by path
/// (`routes/…/page.json`, `components/*.json`, a route's
/// `components/`, `overrides.md`, `themes/`): what `dry_tools::load`
/// reads from disk, for a shell with no disk (the browser's bundle).
/// A page or component that names a component this build does not know
/// still reads (`lenient`), with the error listed.
#[derive(Default)]
pub struct Loaded {
    pub pages: Vec<(String, Page)>,
    pub components: BTreeMap<String, Vec<Component>>,
    pub local: BTreeMap<String, Local>,
    pub errors: Vec<String>,
}

/// Read every route and component in `files`.
pub fn load_files(files: &BTreeMap<String, String>) -> Loaded {
    let mut out = Loaded::default();
    let components_in = |prefix: &str, label: &str, errors: &mut Vec<String>| -> BTreeMap<String, Vec<Component>> {
        let mut map = BTreeMap::new();
        for (path, text) in files.range(prefix.to_string()..).take_while(|(p, _)| p.starts_with(prefix)) {
            let Some(name) = path[prefix.len()..].strip_suffix(".json").filter(|n| !n.contains('/')) else { continue };
            match ComponentFile::from_json(text) {
                Ok(f) => {
                    map.insert(name.to_string(), f.view);
                }
                Err(e) => {
                    errors.push(format!("{label}/{name}.json: {e}"));
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                        && let Some(view) = v.get("view").cloned().map(lenient_view)
                        && let Ok(f) = serde_json::from_value::<ComponentFile>(serde_json::json!({ "view": view }))
                    {
                        map.insert(name.to_string(), f.view);
                    }
                }
            }
        }
        map
    };
    out.components = components_in("components/", "components", &mut out.errors);
    for (path, text) in files.iter().filter(|(p, _)| p.starts_with("routes/") && p.ends_with("page.json")) {
        let folder = path["routes/".len()..path.len() - "page.json".len()].trim_end_matches('/');
        if folder.split('/').any(|s| pages::RESERVED.contains(&s)) {
            continue;
        }
        let page = match Page::from_json(text) {
            Ok(p) => p,
            Err(e) => {
                out.errors.push(format!("{path}: {e}"));
                match serde_json::from_str(text).ok().and_then(lenient_page) {
                    Some(p) => p,
                    None => continue,
                }
            }
        };
        let pattern = pages::folder_to_pattern(folder);
        let dir = if folder.is_empty() { "routes".to_string() } else { format!("routes/{folder}") };
        let mut local = Local {
            components: components_in(&format!("{dir}/components/"), &format!("{dir}/components"), &mut out.errors),
            ..Local::default()
        };
        if let Some(t) = files.get(&format!("{dir}/overrides.md")) {
            match Overrides::from_md(t) {
                Ok(o) => local.overrides = o,
                Err(e) => out.errors.push(format!("{dir}/overrides.md: {e}")),
            }
        }
        let themes = format!("{dir}/themes/");
        for (p, t) in files.range(themes.clone()..).take_while(|(p, _)| p.starts_with(&themes)) {
            let Some(name) = p[themes.len()..].strip_suffix(".md").filter(|n| !n.contains('/')) else { continue };
            match Overrides::from_md(t) {
                Ok(o) => {
                    local.themes.insert(name.to_string(), o);
                }
                Err(e) => out.errors.push(format!("{p}: {e}")),
            }
        }
        local.active = files.get(&format!("{themes}active")).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        if local != Local::default() {
            out.local.insert(pattern.clone(), local);
        }
        out.pages.push((pattern, page));
    }
    out
}

/// A page file whose view names components this build does not know, read
/// with each of those as a placeholder.
pub fn lenient_page(mut value: serde_json::Value) -> Option<Page> {
    let view = value.get("view").cloned().map(lenient_view)?;
    value["view"] = view;
    serde_json::from_value(value).ok()
}

/// A view (a JSON list of components) with every component that does not
/// read replaced by a placeholder; containers keep their children.
pub fn lenient_view(view: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Array(items) = view else { return serde_json::Value::Array(Vec::new()) };
    serde_json::Value::Array(items.into_iter().map(lenient_component).collect())
}

fn lenient_component(mut c: serde_json::Value) -> serde_json::Value {
    for key in ["main", "side"] {
        if let Some(v) = c.get(key).cloned() {
            c[key] = lenient_view(v);
        }
    }
    if let Some(serde_json::Value::Array(panes)) = c.get_mut("panes") {
        for p in panes {
            if let Some(v) = p.get("view").cloned() {
                p["view"] = lenient_view(v);
            }
        }
    }
    if serde_json::from_value::<Component>(c.clone()).is_ok() {
        return c;
    }
    let kind = c.get("type").and_then(|t| t.as_str()).unwrap_or("(no type)").to_string();
    serde_json::json!({ "type": "text", "text": format!("{UNKNOWN}{kind}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_component_becomes_a_placeholder_and_its_neighbours_stay() {
        let view = serde_json::json!([
            { "type": "heading", "text": "Hi" },
            { "type": "hologram", "size": 3 },
            { "type": "columns", "main": [{ "type": "chat" }, { "type": "sparkles" }], "side": [] }
        ]);
        let page = lenient_page(serde_json::json!({ "title": "T", "view": view })).unwrap();
        assert_eq!(page.view[0], Component::Heading { text: "Hi".into() });
        assert_eq!(page.view[1], Component::Text { text: format!("{UNKNOWN}hologram") });
        let Component::Columns { main, .. } = &page.view[2] else { panic!("{:?}", page.view[2]) };
        assert_eq!(main[1], Component::Text { text: format!("{UNKNOWN}sparkles") });
    }

    #[test]
    fn the_repo_in_memory_lists_folders_then_reads_files() {
        let mut files = BTreeMap::new();
        files.insert("routes/page.json".to_string(), "{}".to_string());
        files.insert("routes/code/page.json".to_string(), "{}".to_string());
        files.insert("models.json".to_string(), "{\"a\":1}".to_string());
        let r = MemoryRepo(files);
        assert_eq!(r.list("").unwrap(), [("models.json".to_string(), false), ("routes".to_string(), true)]);
        assert_eq!(r.list("routes").unwrap(), [("code".to_string(), true), ("page.json".to_string(), false)]);
        assert_eq!(r.read("models.json", 3).unwrap(), (b"{\"a".to_vec(), 7));
        assert!(r.list("nope").is_err());
    }
}
