//! Pages: routes and views written as files, like Next.js folder routing.
//!
//! ```text
//! routes/
//!   page.json              → /
//!   desk/[id]/page.json    → /desk/:id
//!   settings/page.json     → /settings
//! components/
//!   welcome.json           → a component any page can `use`
//! ```
//!
//! Every folder under `routes/` is one path segment; `[name]` is a
//! parameter (`:name`). A `page.json` says how the route shows and what its
//! view is made of:
//!
//! ```json
//! {
//!   "title": "Notes",
//!   "icon": "✎",
//!   "nav": "yes",
//!   "order": 30,
//!   "view": [
//!     { "type": "heading", "text": "Notes" },
//!     { "type": "text", "text": "Anything you like." },
//!     { "type": "link", "label": "Back home", "to": "/" },
//!     { "type": "use", "name": "welcome" },
//!     { "type": "text", "text": "{engines} engines, {mode} mode" },
//!     { "type": "models" }
//!   ]
//! }
//! ```
//!
//! A view is a list of components (see `Component`): plain ones (`heading`,
//! `text`, `link`), ones bound to the app's state (`chat`, `tokens`,
//! `models`, ...), and `use`, which pulls in a component file from
//! `components/` (itself a list of components). Text may name live values
//! in braces (`{engines}`, `{mode}`; see `fill`).
//!
//! A route folder is shareable: besides `page.json` it may hold what only
//! that page uses, so copying the folder into another dry brings the whole
//! page (see `Local`):
//!
//! ```text
//! routes/code/
//!   page.json              → /code
//!   components/*.json      → components only this page sees (before components/)
//!   overrides.md           → token changes while this page is shown
//!   themes/*.md, active    → override sets to pick from on the page
//!   assets/                → files the page refers to
//! ```
//!
//! Those folder names (`RESERVED`) are never routes.
//!
//! A page that moved keeps its old folder with only `"moved": "/new"` in
//! its `page.json` (`routes/engines/page.json` → `/desk`): the old path and
//! every path under it open the same place under the new one
//! (`RouteTable::moved_to`), so links written before still work.
//!
//! This file only turns the JSON into values and back. Reading the folders
//! is the shell's job (`crates/gpui/src/store.rs`); writing them is the
//! `dry` tool's (`crates/tools`).

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::routes::{Nav, Route, RouteTable};
use crate::theme::Overrides;

/// One component in a view. These are the primitives every shell knows how
/// to draw, like HTML's elements: a page names them directly by `type`, and
/// there is no other registry. Plain ones show text; the rest are bound to
/// the app's state (the engine of the route, the theme, the models, the
/// routes). New pages are compositions of these, written as files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Component {
    /// A title line.
    Heading { text: String },
    /// A paragraph.
    Text { text: String },
    /// A button that navigates to `to`.
    Link { label: String, to: String },
    /// The top of an engine's page: its name, what it is doing (on the
    /// activity's tint), its model, and its prompt against the context
    /// window.
    EngineHeader,
    /// One engine's conversation (the engine is the route's `:id`).
    Chat,
    /// The text area the whole app types into: an input, Send, and its
    /// options (see `crate::composer`). On an engine's page the text goes
    /// to that engine; anywhere else it starts a new one. One per page.
    Composer {
        #[serde(default = "crate::composer::default_actions")]
        actions: Vec<crate::composer::Action>,
    },
    /// Every design token of the active theme, editable.
    Tokens,
    /// Light / dark.
    Appearance,
    /// Reset history (`crate::store::RESET_LABEL`): a button that opens a
    /// confirm step naming what goes and where the backup lands; only its
    /// second press moves the store's own files aside, so the app runs
    /// from the default alone (docs/default.md, "Reset").
    ResetHistory,
    /// The model settings from `models.json`.
    Models,
    /// Every route, with Hide/Show and Delete.
    Routes,
    /// What the window and screen are.
    Diagnostics,
    /// The machine the file and terminal tools run on
    /// (`crate::environment`): its kind, whether it is ready, and the
    /// browser VM's saved copy with a Forget that asks first.
    Environment,
    /// A form that creates a new route (writes its `page.json`).
    NewRoute,
    /// Home's summary: tiles for how many engines there are, which are
    /// working, which need you, and what this runtime can do.
    Dashboard,
    /// One card per engine (model or session), and a button to make each
    /// kind: a model engine, or a session of any program in `sessions.json`.
    Engines,
    /// The Desk (`crate::desk`): the engine tree with live status, the
    /// selected thread as a session stream (each guided post, and
    /// under it the work of the engine it called), and the composer pinned
    /// at the bottom, posting to that thread. One per page.
    Desk,
    /// The route's engine's conversation as one stream (`crate::desk::turns`):
    /// each message you sent, then its turn's thoughts, tool calls (one
    /// line each, opening to the whole call and result) and reply, live.
    Session,
    /// One card per program in `sessions.json`, with whether it is installed.
    Tools,
    /// Every capability this runtime has (`crate::capability`), by provider
    /// and state, with a Grant button for one that needs a person's
    /// gesture, and the peers that joined.
    Capabilities,
    /// The route's engine's artifacts: one compact card each (kind, name,
    /// its state in a few words, its tools), each opening its own page.
    Artifacts,
    /// The tools the route's engine may call now, grouped by where they
    /// come from (built-in, artifact, phase, skill, MCP).
    EngineTools,
    /// The route's engine's prompt as rendered, slot by slot with sizes
    /// and why a slot is empty, the context lines, and Copy as text.
    Prompt,
    /// One artifact's own page (`/desk/:id/artifacts/:name`): its
    /// tools, its state laid out by kind, and its snapshot as sent.
    ArtifactPage,
    /// One part of the route's engine's live view (`crate::liveview`):
    /// the UI its developer artifacts attach, a code editor's layout
    /// placed with `panes`. On `/desk/:id/artifacts/:name/live` it shows
    /// that artifact only; on `/desk/:id/live`, all of them.
    LiveView { part: crate::liveview::Part },
    /// Two columns on a wide screen: `main` on the left, `side` on the
    /// right (`side_width` wide); one column, `main` first, where there is
    /// less room.
    Columns {
        main: Vec<Component>,
        side: Vec<Component>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side_width: Option<f32>,
    },
    /// The route's engine's response contract and format, switchable while
    /// it runs, and the prompt as the model sees it.
    Response,
    /// Everything the route's engine (`:id`) knows about itself: its kind,
    /// its session id, and all the metadata its program reported.
    Metadata,
    /// Panes side by side (`row`) or stacked (`column`), each a view of
    /// its own with a fixed size or the room left over: how an app-like
    /// page (an editor, a dashboard) is laid out. With `resizable`, the
    /// edges between panes can be dragged.
    Panes {
        #[serde(default)]
        axis: Axis,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        resizable: bool,
        panes: Vec<Pane>,
    },
    /// A folder tree of the repo: folders open and close, a file opens in
    /// the `editor` (see `crate::workbench`).
    Explorer,
    /// The open files as tabs and the active one's text, read-only, with
    /// line numbers.
    Editor,
    /// A command line run in the repo folder when Enter is pressed, and
    /// what each command printed. It never runs anything by itself.
    Terminal,
    /// One button per theme the route ships (`themes/*.md` in its folder),
    /// switching the page's look live; on a route without any, the app's
    /// own themes.
    ThemePicker,
    /// The components of `components/<name>.json`.
    Use { name: String },
    /// An ask component (`crate::ask`): a control that asks the user one
    /// thing. A model's reply brings one into the chat (`reply(ask=...)`);
    /// on a page its answer goes to the route's engine.
    Ask { ask: crate::ask::Ask },
    /// A display component (`crate::show`): a table, list, progress bar
    /// or stat row. A model's reply brings one into the chat
    /// (`reply(show=...)`); on a page it shows what the file says.
    Show { show: crate::show::Show },
    /// Analytics from the engines' spans (`crate::analytics`): `timeline`
    /// (the route's engine: a waterfall of its last turns and its totals),
    /// `today` (Home: today's work vs waiting, cost, the slowest tools) or
    /// `page` (every query's table over a day range, `?days=`).
    Analytics {
        #[serde(default)]
        part: crate::analytics::Part,
    },
}

/// Which way `panes` are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
    /// Side by side, left to right.
    #[default]
    Row,
    /// Stacked, top to bottom.
    Column,
}

/// One pane of `panes`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    /// Logical pixels along the axis; none takes the room left over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// The smallest it can be dragged to (with `resizable`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    /// A colour token name to fill it with (`background_surface`, ...),
    /// so the theme, not the page, decides the colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// A colour token name for all the text in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A size token name (`space_2`, ...): room inside its edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<String>,
    /// A size token name: room between the components of its view (none
    /// by default, so parts meet edge to edge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
    /// A colour token name for a 1px line on one edge (`edge`), where
    /// this part meets the next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    /// Which edge `border` is drawn on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<Edge>,
    /// Draw the view as a bar of an app-like page instead of a column of
    /// components (see `Bar`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar: Option<Bar>,
    /// In a bar, which end its items sit at (the start by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default)]
    pub view: Vec<Component>,
}

/// An edge of a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

/// How a pane's view is drawn when it is a bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bar {
    /// A narrow column of icons: each `link` is a square cell with its
    /// label as the glyph, the one for the page on screen marked.
    Activity,
    /// A thin strip of items left to right: each `text` and `link` is one
    /// item; `theme-picker` is one item that moves to the next theme.
    Status,
}

/// Where a bar's items sit along it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    #[default]
    Start,
    End,
}

impl Pane {
    /// Does it take the room left over?
    pub fn fills(&self) -> bool {
        self.size.is_none()
    }
}

/// What a `page.json` holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub title: String,
    /// The navigation icon: the glyph itself (`"⌂"`, `"✎"`, an emoji ...).
    #[serde(default = "page_icon")]
    pub icon: String,
    #[serde(default)]
    pub nav: Nav,
    /// Position in the navigation: lower comes first; ties go by path.
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub view: Vec<Component>,
    /// The page has moved: this path and every path under it open the
    /// same place under `moved` instead (`/engines/2` → `/desk/2` for
    /// `"moved": "/desk"` on `routes/engines/page.json`), so old links
    /// keep working. Such a page draws nothing itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved: Option<String>,
}

fn page_icon() -> String {
    "▤".to_string()
}

/// What a `components/<name>.json` holds: a list of components.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComponentFile {
    #[serde(default)]
    pub view: Vec<Component>,
}

impl Page {
    pub fn new(title: &str) -> Page {
        Page { title: title.to_string(), icon: page_icon(), nav: Nav::Yes, order: 0, view: Vec::new(), moved: None }
    }

    pub fn from_json(text: &str) -> Result<Page, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("pages always serialise") + "\n"
    }

    /// The route this page makes at `pattern`.
    pub fn route(&self, pattern: &str) -> Route {
        Route { pattern: pattern.to_string(), title: self.title.clone(), icon: self.icon.clone(), nav: self.nav, moved: self.moved.clone() }
    }
}

impl ComponentFile {
    pub fn from_json(text: &str) -> Result<ComponentFile, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("components always serialise") + "\n"
    }
}

/// Every route and component in one value: what `dry export` writes and
/// `dry import` turns back into the `routes/` folders and `components/` files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Pattern (`/desk/:id`) → its page.
    #[serde(default)]
    pub routes: std::collections::BTreeMap<String, Page>,
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, Vec<Component>>,
}

impl Snapshot {
    pub fn from_json(text: &str) -> Result<Snapshot, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("snapshots always serialise") + "\n"
    }
}

/// Fill `{name}` placeholders in a text from `values` (`{engines}` →
/// `"2"`). An unknown name is left as written, so a typo shows.
pub fn fill(text: &str, values: &[(&str, String)]) -> String {
    let mut out = text.to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// A folder under `routes/` as a route pattern: `""` → `/`,
/// `desk/[id]` → `/desk/:id`.
pub fn folder_to_pattern(folder: &str) -> String {
    let segments: Vec<String> = folder
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Some(name) => format!(":{name}"),
            None => s.to_string(),
        })
        .collect();
    format!("/{}", segments.join("/"))
}

/// The other way: `/desk/:id` → `desk/[id]`, `/` → `""`.
pub fn pattern_to_folder(pattern: &str) -> String {
    pattern
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix(':') {
            Some(name) => format!("[{name}]"),
            None => s.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The route table for these pages, in navigation order.
pub fn table(pages: &[(String, Page)]) -> RouteTable {
    let mut sorted: Vec<&(String, Page)> = pages.iter().collect();
    sorted.sort_by(|(pa, a), (pb, b)| (a.order, pa).cmp(&(b.order, pb)));
    let mut table = RouteTable::default();
    for (pattern, page) in sorted {
        table.add(page.route(pattern));
    }
    table
}

/// The folders inside a route folder that hold the page's own files and
/// are never routes themselves.
pub const RESERVED: &[&str] = &["components", "assets", "themes"];

/// What a route folder holds besides its `page.json`: the page's own
/// components, its token overrides and the override sets ("themes") it
/// offers. Read by the loader; the core decides what they mean.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Local {
    /// `components/<name>.json` in the route folder, found before the
    /// app's `components/`.
    pub components: BTreeMap<String, Vec<Component>>,
    /// `overrides.md`: what the page always changes (spacing, font, ...).
    pub overrides: Overrides,
    /// `themes/<name>.md`: override sets laid over `overrides`, one at a time.
    pub themes: BTreeMap<String, Overrides>,
    /// `themes/active`: the chosen one, if any.
    pub active: Option<String>,
}

impl Local {
    /// The theme in use: the chosen one if it exists, else the first by name.
    pub fn theme(&self) -> Option<&str> {
        self.active.as_deref().filter(|a| self.themes.contains_key(*a)).or_else(|| self.themes.keys().next().map(String::as_str))
    }

    /// The override sets to draw the page with, in order: its overrides,
    /// then its theme. Empty for a route that changes nothing.
    pub fn layers(&self) -> Vec<&Overrides> {
        let mut out = Vec::new();
        if !self.overrides.is_empty() {
            out.push(&self.overrides);
        }
        out.extend(self.theme().and_then(|name| self.themes.get(name)));
        out
    }
}

/// Find a component by name the way a route does: its own folder first,
/// then the app's `components/`. A page's component may share a name with
/// an app one and wins on that page only.
pub fn find<'a>(local: Option<&'a Local>, global: &'a BTreeMap<String, Vec<Component>>) -> impl Fn(&str) -> Option<Vec<Component>> + 'a {
    move |name| local.and_then(|l| l.components.get(name)).or_else(|| global.get(name)).cloned()
}

/// A file name as a label: `dark-modern` → `Dark Modern`.
pub fn label(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Does this view fill its pane (it is laid out in `panes`) rather than
/// scroll in it? Then the frame gives it the whole box, edge to edge.
pub fn fills(view: &[Component]) -> bool {
    !view.is_empty() && view.iter().all(|c| matches!(c, Component::Panes { .. }))
}

/// `view` with every `use` replaced by that component file's components
/// (found with `lookup`), so the shell only draws plain and builtin ones.
/// A missing component, or `use` nested too deep (a component that uses
/// itself), shows as a text saying so instead of failing.
pub fn expand(view: &[Component], lookup: &dyn Fn(&str) -> Option<Vec<Component>>) -> Vec<Component> {
    fn go(view: &[Component], lookup: &dyn Fn(&str) -> Option<Vec<Component>>, depth: usize, out: &mut Vec<Component>) {
        for c in view {
            match c {
                Component::Use { name } if depth >= 8 => {
                    out.push(Component::Text { text: format!("Component \"{name}\" is nested too deep (does it use itself?).") })
                }
                Component::Use { name } => match lookup(name) {
                    Some(inner) => go(&inner, lookup, depth + 1, out),
                    None => out.push(Component::Text { text: format!("No component \"{name}\" in components/.") }),
                },
                // A pane's view is a view too: its `use`s are replaced the same way.
                Component::Panes { axis, resizable, panes } => out.push(Component::Panes {
                    axis: *axis,
                    resizable: *resizable,
                    panes: panes
                        .iter()
                        .map(|p| {
                            let mut view = Vec::new();
                            go(&p.view, lookup, depth + 1, &mut view);
                            Pane { view, ..p.clone() }
                        })
                        .collect(),
                }),
                // So are a column's.
                Component::Columns { main, side, side_width } => {
                    let (mut m, mut sd) = (Vec::new(), Vec::new());
                    go(main, lookup, depth + 1, &mut m);
                    go(side, lookup, depth + 1, &mut sd);
                    out.push(Component::Columns { main: m, side: sd, side_width: *side_width });
                }
                other => out.push(other.clone()),
            }
        }
    }
    let mut out = Vec::new();
    go(view, lookup, 0, &mut out);
    out
}

/// Is `view` laid out in two columns (`columns`, or an artifact's page,
/// which lays out its own), so its box may grow on a wide screen
/// (`layout::Frame::widened`)?
pub fn wide(view: &[Component]) -> bool {
    view.iter().any(|c| matches!(c, Component::Columns { .. } | Component::ArtifactPage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_writes_back_the_same() {
        let mut snap = Snapshot::default();
        snap.routes.insert("/notes".into(), Page::new("Notes"));
        snap.components.insert("w".into(), vec![Component::Text { text: "hi".into() }, Component::NewRoute]);
        assert_eq!(Snapshot::from_json(&snap.to_json()).unwrap(), snap);
        assert!(snap.to_json().contains(r#""type": "new-route""#));
        let page = Page::from_json(r#"{"title":"S","view":[{"type":"models"},{"type":"chat"}]}"#).unwrap();
        assert_eq!(page.view, vec![Component::Models, Component::Chat]);
    }

    #[test]
    fn placeholders_are_filled_and_unknown_ones_stay() {
        let values = [("engines", "2".to_string()), ("mode", "dark".to_string())];
        assert_eq!(fill("{engines} engines, {mode} mode, {nope}", &values), "2 engines, dark mode, {nope}");
    }

    #[test]
    fn folders_and_patterns_turn_into_each_other() {
        for (folder, pattern) in [("", "/"), ("settings", "/settings"), ("desk/[id]", "/desk/:id")] {
            assert_eq!(folder_to_pattern(folder), pattern);
            assert_eq!(pattern_to_folder(pattern), folder);
        }
    }

    #[test]
    fn an_ask_component_is_registered_like_the_others() {
        let page = Page::from_json(r#"{"title":"Plan","view":[{"type":"ask","ask":{"kind":"date"}},{"type":"ask","ask":{"kind":"choice","options":["a","b"]}}]}"#).unwrap();
        assert_eq!(page.view[0], Component::Ask { ask: crate::ask::Ask::Date });
        assert_eq!(page.view[1], Component::Ask { ask: crate::ask::Ask::Choice { options: vec!["a".into(), "b".into()] } });
        assert_eq!(Page::from_json(&page.to_json()).unwrap(), page);
        assert!(Page::from_json(r#"{"title":"x","view":[{"type":"ask","ask":{"kind":"calendar"}}]}"#).is_err());
        let page = Page::from_json(r#"{"title":"Stats","view":[{"type":"show","show":{"kind":"progress","value":3,"max":4}}]}"#).unwrap();
        assert_eq!(page.view[0], Component::Show { show: crate::show::Show::Progress { label: String::new(), value: 3.0, max: 4.0 } });
    }

    #[test]
    fn columns_read_with_their_views_and_their_uses_are_replaced() {
        let page = Page::from_json(
            r#"{"title":"Engine","view":[{"type":"engine-header"},{"type":"columns","main":[{"type":"chat"},{"type":"use","name":"w"}],"side":[{"type":"engine-tools"},{"type":"artifacts"},{"type":"prompt"}],"side_width":340}]}"#,
        )
        .unwrap();
        assert_eq!(page.view[0], Component::EngineHeader);
        assert!(wide(&page.view) && !wide(&page.view[..1]));
        assert_eq!(Page::from_json(&page.to_json()).unwrap(), page);
        let lookup = |n: &str| (n == "w").then(|| vec![Component::Text { text: "hi".into() }]);
        let Component::Columns { main, side, side_width } = &expand(&page.view, &lookup)[1] else { panic!() };
        assert_eq!(main, &[Component::Chat, Component::Text { text: "hi".into() }]);
        assert_eq!((side.len(), *side_width), (3, Some(340.0)));
        let art = Page::from_json(r#"{"title":"Artifact","nav":"no","view":[{"type":"artifact-page"}]}"#).unwrap();
        assert_eq!((art.nav, &art.view[..]), (Nav::No, &[Component::ArtifactPage][..]));
        assert!(wide(&art.view), "an artifact's page lays out its own columns");
    }

    #[test]
    fn a_page_file_reads_with_defaults_and_writes_back_the_same() {
        let page = Page::from_json(r#"{"title":"Notes","view":[{"type":"text","text":"hi"},{"type":"use","name":"w"}]}"#).unwrap();
        assert_eq!((page.icon.as_str(), page.nav, page.order), ("▤", Nav::Yes, 0));
        assert_eq!(page.route("/notes").icon, "▤");
        assert_eq!(Page::from_json(&page.to_json()).unwrap(), page);
        assert!(Page::from_json(r#"{"title":"x","view":[{"type":"nope"}]}"#).is_err());
    }

    #[test]
    fn pages_are_ordered_by_order_then_path() {
        let mut home = Page::new("Home");
        home.order = -1;
        let pages = vec![("/z".to_string(), Page::new("Z")), ("/a".to_string(), Page::new("A")), ("/".to_string(), home)];
        let titles: Vec<String> = table(&pages).list().iter().map(|r| r.title.clone()).collect();
        assert_eq!(titles, ["Home", "A", "Z"]);
    }

    #[test]
    fn panes_read_with_sizes_and_the_one_without_a_size_fills() {
        let page = Page::from_json(
            r#"{"title":"C","view":[{"type":"panes","axis":"column","panes":[
                {"view":[{"type":"editor"}]},
                {"size":22,"background":"accent","view":[{"type":"text","text":"ok"}]}]}]}"#,
        )
        .unwrap();
        let Component::Panes { axis, resizable, panes } = &page.view[0] else { panic!("{:?}", page.view) };
        assert_eq!((*axis, *resizable), (Axis::Column, false));
        assert!(panes[0].fills() && !panes[1].fills());
        assert_eq!(panes[1].background.as_deref(), Some("accent"));
        assert_eq!(Page::from_json(&page.to_json()).unwrap(), page);
        assert!(fills(&page.view));
        assert!(!fills(&[Component::Editor, page.view[0].clone()]), "mixed with plain components it scrolls");
        let bars = Page::from_json(
            r#"{"title":"B","view":[{"type":"panes","panes":[
                {"size":48,"bar":"activity","border":"border","edge":"right","view":[{"type":"link","label":"⌂","to":"/"}]},
                {"bar":"status","align":"end","view":[{"type":"theme-picker"}]}]}]}"#,
        )
        .unwrap();
        let Component::Panes { panes, .. } = &bars.view[0] else { panic!() };
        assert_eq!((panes[0].bar, panes[0].edge, panes[0].border.as_deref()), (Some(Bar::Activity), Some(Edge::Right), Some("border")));
        assert_eq!((panes[1].bar, panes[1].align), (Some(Bar::Status), Some(Align::End)));
        assert_eq!(Page::from_json(&bars.to_json()).unwrap(), bars);
        assert!(Page::from_json(r#"{"title":"x","view":[{"type":"panes","panes":[{"bar":"menu"}]}]}"#).is_err());
        let live = Page::from_json(r#"{"title":"x","view":[{"type":"live-view","part":"explorer"},{"type":"live-view","part":"chat"}]}"#).unwrap();
        assert_eq!(live.view[1], Component::LiveView { part: crate::liveview::Part::Chat });
        assert_eq!(Page::from_json(&live.to_json()).unwrap(), live);
        assert!(Page::from_json(r#"{"title":"x","view":[{"type":"live-view","part":"minimap"}]}"#).is_err());
        let kinds = Page::from_json(r#"{"title":"x","view":[{"type":"explorer"},{"type":"terminal"},{"type":"theme-picker"}]}"#).unwrap();
        assert_eq!(kinds.view, vec![Component::Explorer, Component::Terminal, Component::ThemePicker]);
    }

    #[test]
    fn a_route_finds_its_own_components_before_the_apps() {
        let global = BTreeMap::from([
            ("bar".to_string(), vec![Component::Text { text: "app bar".into() }]),
            ("other".to_string(), vec![Component::Text { text: "app other".into() }]),
        ]);
        let mut local = Local::default();
        local.components.insert("bar".into(), vec![Component::Text { text: "page bar".into() }, Component::Use { name: "other".into() }]);
        let view = [Component::Use { name: "bar".into() }];
        let out = expand(&view, &find(Some(&local), &global));
        assert_eq!(out, vec![Component::Text { text: "page bar".into() }, Component::Text { text: "app other".into() }]);
        assert_eq!(expand(&view, &find(None, &global)), vec![Component::Text { text: "app bar".into() }], "other routes keep the app's");
        let nested = [Component::Panes { axis: Axis::Row, resizable: false, panes: vec![Pane { view: view.to_vec(), ..Pane::default() }] }];
        let Component::Panes { panes, .. } = &expand(&nested, &find(Some(&local), &global))[0] else { panic!() };
        assert_eq!(panes[0].view[0], Component::Text { text: "page bar".into() }, "a use inside a pane is replaced too");
    }

    #[test]
    fn a_route_draws_with_its_overrides_then_its_chosen_theme() {
        use crate::theme::Overrides;
        let set = |r: f32| {
            let mut o = Overrides::default();
            o.numbers.insert("radius".into(), r);
            o
        };
        let mut local = Local { overrides: set(0.0), ..Local::default() };
        assert_eq!(local.layers().len(), 1);
        local.themes.insert("b-dark".into(), set(1.0));
        local.themes.insert("a-light".into(), set(2.0));
        assert_eq!(local.theme(), Some("a-light"), "none chosen: the first by name");
        local.active = Some("b-dark".into());
        assert_eq!(local.layers(), vec![&set(0.0), &set(1.0)]);
        local.active = Some("gone".into());
        assert_eq!(local.theme(), Some("a-light"), "a chosen theme that is not there falls back");
        assert!(Local::default().layers().is_empty());
        assert_eq!(label("high-contrast_dark"), "High Contrast Dark");
    }

    #[test]
    fn use_is_replaced_by_the_component_and_a_loop_or_a_missing_one_becomes_a_note() {
        let lookup = |name: &str| match name {
            "greet" => Some(vec![Component::Heading { text: "Hello".into() }, Component::Use { name: "sign".into() }]),
            "sign" => Some(vec![Component::Text { text: "- dry".into() }]),
            "loop" => Some(vec![Component::Use { name: "loop".into() }]),
            _ => None,
        };
        let out = expand(&[Component::Use { name: "greet".into() }], &lookup);
        assert_eq!(out, vec![Component::Heading { text: "Hello".into() }, Component::Text { text: "- dry".into() }]);
        let missing = expand(&[Component::Use { name: "gone".into() }], &lookup);
        assert!(matches!(&missing[0], Component::Text { text } if text.contains("gone")));
        let looped = expand(&[Component::Use { name: "loop".into() }], &lookup);
        assert!(matches!(&looped[0], Component::Text { text } if text.contains("too deep")));
    }
}
