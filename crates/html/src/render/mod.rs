//! `Component` → HTML: the web's `crates/gpui/src/views`. Each arm of
//! `component` draws one primitive from the same core view models the
//! window draws it from (`dry_core::dashboard`, `card`, `activity`,
//! `inspect`, `ask`, `workbench`, ...), so the two read the same and this
//! file only says how it looks. A variant this file has no arm for, or a
//! component the page file names that this build does not know, draws a
//! placeholder naming it, never a crash.
//!
//! On the read-only server (docs/web.md) what would change the app (Send,
//! an ask's answer, Delete, Save) is drawn disabled, with the reason
//! (`READ_ONLY`). In the browser (`Access::Live`, docs/browser.md) those
//! are live: each is a button with `data-act` (and its `data-*` values)
//! the page's event delegation handles; what needs a computer stays off
//! (`NATIVE_ONLY`). On the host (`Access::Host`) the same buttons are
//! live for the intents the host takes (`HOST_ACTS`: sending, answers,
//! new and deleted engines, the format, the mode, Detect and Test); the
//! page's script sends them with `fetch`. Looking needs no host: links, folds, the Code page's
//! folders and files, and previews of a mode or a theme (`Query`) are
//! plain links.
//!
//! The submodules hold the bound primitives by page: `engine` (an
//! engine's page and its artifacts), `settings` (Home, Engines, Settings
//! and the tokens page), `code` (the Code page's panes and bars).

mod analytics;
mod ask;
mod code;
mod desk;
mod engine;
mod live;
mod settings;
mod show;
mod term;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::rc::Rc;

use dry_core::activity::Activity;
use dry_core::card::{Card, Status, Tone};
use dry_core::inspect::{ArtifactView, SlotView};
use dry_core::pages::{self, Component};
use dry_core::routes::Params;
use dry_core::theme::{Mode, Tokens};

pub use crate::world::{HOST_LATER, NATIVE_ONLY, READ_ONLY};
use crate::world::{UNKNOWN, World};

/// What a page's URL asks beyond its path: the Code page's open folders
/// and file, and a preview of a mode or a theme (shown, never saved).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Folders open in the explorer.
    pub open: Vec<String>,
    /// The file shown in the editor.
    pub file: Option<String>,
    pub mode: Option<Mode>,
    /// A theme to preview: the route's own (`routes/<folder>/themes/`) or
    /// the app's (`themes/`).
    pub theme: Option<String>,
    /// The analytics page's day range (`?days=7`).
    pub days: Option<u32>,
    /// The Desk's selected thread: a root's id (`?thread=translator`;
    /// `?board=` is its old name; none: the root of `to`, else the first).
    pub thread: Option<String>,
    /// The Desk's selected engine: the composer's target (`?to=helper`).
    pub to: Option<String>,
}

impl Query {
    pub fn parse(query: &str) -> Query {
        let mut q = Query::default();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = decode(v);
            match k {
                "open" => q.open.extend(v.split(',').filter(|s| !s.is_empty() && dry_core::workbench::safe(s)).map(String::from)),
                "file" if dry_core::workbench::safe(&v) && !v.is_empty() => q.file = Some(v),
                "mode" => q.mode = Mode::parse(&v),
                "theme" if dry_core::theme::valid_scene_name(&v) => q.theme = Some(v),
                "days" => q.days = v.parse().ok(),
                "thread" | "board" if id_like(&v) && !v.is_empty() => q.thread = Some(v),
                "to" if id_like(&v) && !v.is_empty() => q.to = Some(v),
                _ => {}
            }
        }
        q
    }

    /// This query with `open` and `file` as given, as `?...` (empty when nothing).
    pub fn with(&self, open: &[String], file: Option<&str>) -> String {
        let mut parts = Vec::new();
        if !open.is_empty() {
            parts.push(format!("open={}", encode(&open.join(","))));
        }
        if let Some(f) = file {
            parts.push(format!("file={}", encode(f)));
        }
        if let Some(m) = self.mode {
            parts.push(format!("mode={}", m.as_str()));
        }
        if let Some(t) = &self.theme {
            parts.push(format!("theme={}", encode(t)));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

/// An engine id as a query value: letters, digits, `-`, `_`, `.`.
fn id_like(v: &str) -> bool {
    v.len() <= 80 && v.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A query value: `%XX` and `+` decoded.
pub fn decode(s: &str) -> String {
    unescape(s, true)
}

/// A URL path: `%XX` decoded (a `+` is a plus there).
pub fn decode_path(s: &str) -> String {
    unescape(s, false)
}

fn unescape(s: &str, plus: bool) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' if plus => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Everything but unreserved characters and `/` as `%XX`.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Text made safe for HTML, attributes included.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The exact view of one engine (`dry_core::inspect`), taken once per
/// request with the read-only disk: what the window's snapshot holds.
pub struct Snap {
    pub taken: i64,
    pub artifacts: Vec<ArtifactView>,
    pub slots: Vec<SlotView>,
    pub notes: Vec<(String, String)>,
    /// `prompt ≈ … of … tokens`, against the window as known.
    pub size: String,
    pub ratio: f64,
}

/// What a component is drawn with: the world, where we are, and the
/// tokens of the mode on screen.
pub struct Ctx<'a> {
    pub world: &'a World,
    pub location: &'a str,
    pub params: &'a Params,
    pub query: &'a Query,
    pub t: &'a Tokens,
    snaps: RefCell<BTreeMap<usize, Rc<Snap>>>,
}

impl<'a> Ctx<'a> {
    pub fn new(world: &'a World, location: &'a str, params: &'a Params, query: &'a Query, t: &'a Tokens) -> Ctx<'a> {
        Ctx { world, location, params, query, t, snaps: RefCell::default() }
    }

    /// Engine `i`'s activity: its live turn if the shell runs one (the
    /// browser), else what the store says (`dry_core::activity::live`).
    pub fn activity(&self, i: usize) -> Activity {
        let r = &self.world.reported;
        self.world.state.engines.get(i).map_or(Activity::Idle, |e| dry_core::dashboard::live_activity(e, r.status.get(&i), r.activity.get(&i)))
    }

    pub fn status(&self, i: usize) -> Status {
        let r = &self.world.reported;
        self.world.state.engines.get(i).map_or(Status::Idle, |e| dry_core::dashboard::live_status(e, r.status.get(&i), r.activity.get(&i)))
    }

    /// A control that changes the app: live here (`data-act` and its
    /// `attrs`, for the page's event delegation), else drawn off.
    pub fn act(&self, label: &str, class: &str, attrs: &str) -> String {
        let act = attrs.split("data-act=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
        if self.world.can(act) {
            format!("<button class=\"btn {class}\" type=\"button\" {attrs}>{}</button>", esc(label))
        } else {
            self.off(label, class)
        }
    }

    /// A control that is off here, saying why.
    pub fn off(&self, label: &str, class: &str) -> String {
        format!("<button class=\"btn {class}\" disabled title=\"{}\">{}</button>", esc(self.world.off_reason()), esc(label))
    }

    /// The route's engine (`:id`), if it has one.
    pub fn engine(&self) -> Option<(usize, &'a dry_core::state::Engine)> {
        self.params.index("id").and_then(|i| self.world.state.engines.get(i).map(|e| (i, e)))
    }

    /// Engine `i`'s exact view, taken on first use in this request.
    pub fn snap(&self, i: usize) -> Option<Rc<Snap>> {
        if let Some(s) = self.snaps.borrow().get(&i) {
            return Some(s.clone());
        }
        let e = self.world.state.engines.get(i)?;
        let disk: &dyn dry_core::live::Host = self.world.host.as_ref();
        let seen = |j: usize| dry_core::engines::Seen { label: self.activity(j).label(), busy: self.activity(j).is_turn(), failed: None };
        let mut family = dry_core::engines::family(&self.world.state, i, &seen);
        family.offset = self.world.clock.offset;
        let at = dry_core::live::Snapshots { clock: Some(self.world.clock), host: Some(disk), family: Some(&family) };
        let rendered = dry_core::prompt::render_with(e, &at);
        let window = match self.world.reported.measured.get(&i) {
            Some(m) => m.budget.window.clone(),
            None => dry_core::context::Window::of(&e.inference, None),
        };
        let budget = dry_core::context::measure(&rendered, &window);
        let (slots, notes) = dry_core::inspect::slots(e, &rendered);
        let snap = Rc::new(Snap {
            taken: disk.now(),
            artifacts: dry_core::inspect::artifacts(e, &at),
            slots,
            notes,
            size: budget.line(),
            ratio: budget.ratio(),
        });
        self.snaps.borrow_mut().insert(i, snap.clone());
        Some(snap)
    }

    /// Live values a text can name in braces, like the window's `values`.
    pub fn values(&self) -> Vec<(&'static str, String)> {
        let file = self.query.file.as_deref().map(|f| code::open_file(self.world.repo.as_ref(), f));
        let theme = match self.world.local_of(self.location).and_then(|l| l.theme().map(String::from)) {
            Some(own) => pages::label(self.query.theme.as_deref().filter(|q| self.route_has_theme(q)).unwrap_or(&own)),
            None => self.query.theme.clone().unwrap_or_else(|| self.world.active_theme.clone()),
        };
        vec![
            ("engines", self.world.state.engines.len().to_string()),
            ("mode", self.t.mode.as_str().to_string()),
            ("file", file.as_ref().map_or_else(|| "no file open".to_string(), |f| f.path.clone())),
            ("lines", file.as_ref().map_or_else(String::new, |f| f.len().to_string())),
            ("line", file.as_ref().map_or_else(String::new, |_| "1".to_string())),
            ("language", file.as_ref().map_or("", |f| f.language()).to_string()),
            ("repo", self.world.repo_name.clone()),
            ("theme", theme),
        ]
    }

    fn route_has_theme(&self, name: &str) -> bool {
        self.world.local_of(self.location).is_some_and(|l| l.themes.contains_key(name))
    }
}

/// A view: its components top to bottom.
pub fn view(out: &mut String, view: &[Component], cx: &Ctx) {
    let class = if pages::fills(view) { "view fills" } else { "view" };
    let _ = write!(out, "<div class=\"{class}\">");
    for c in view {
        component(out, c, cx);
    }
    out.push_str("</div>");
}

/// The view a pane holds: no gap by default, so its parts meet.
fn stack(out: &mut String, view: &[Component], gap: Option<f32>, cx: &Ctx) {
    let style = gap.map_or("gap:0".to_string(), |g| format!("gap:{g}px"));
    let _ = write!(out, "<div class=\"view stack\" style=\"{style}\">");
    for c in view {
        component(out, c, cx);
    }
    out.push_str("</div>");
}

/// The component's `type`, as a page file names it.
pub fn type_name(c: &Component) -> String {
    serde_json::to_value(c).ok().and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from)).unwrap_or_else(|| "?".into())
}

/// A clear box where a component has no web view (yet).
pub fn placeholder(out: &mut String, kind: &str, why: &str) {
    let _ = write!(
        out,
        "<div class=\"placeholder\" data-component=\"{k}\"><b>{k}</b><span>{}</span></div>",
        esc(why),
        k = esc(kind)
    );
}

/// One component.
pub fn component(out: &mut String, c: &Component, cx: &Ctx) {
    match c {
        Component::Text { text } if text.starts_with(UNKNOWN) => {
            let kind = &text[UNKNOWN.len()..];
            placeholder(out, kind, "this build does not know this component; update dry-core, or check the page file's \"type\"");
        }
        Component::Heading { text } => {
            let _ = write!(out, "<h2 class=\"heading\">{}</h2>", esc(&pages::fill(text, &cx.values())));
        }
        Component::Text { text } => {
            let _ = write!(out, "<p class=\"text\">{}</p>", esc(&pages::fill(text, &cx.values())));
        }
        Component::Link { label, to } => {
            let _ = write!(out, "<div class=\"row\"><a class=\"btn\" href=\"{}\">{}</a></div>", esc(to), esc(label));
        }
        Component::EngineHeader => engine::header(out, cx),
        Component::Chat => engine::chat(out, cx),
        Component::Composer { actions } => engine::composer(out, actions, cx),
        Component::Artifacts => engine::artifacts(out, cx),
        Component::EngineTools => engine::tools_list(out, cx),
        Component::Prompt => engine::prompt(out, cx),
        Component::Response => engine::response(out, cx),
        Component::Metadata => engine::metadata(out, cx),
        Component::ArtifactPage => engine::artifact_page(out, cx),
        Component::Dashboard => settings::dashboard(out, cx),
        Component::Engines => settings::engines(out, cx),
        Component::Desk => desk::desk(out, cx),
        Component::Session => desk::session(out, cx),
        Component::Tools => settings::tools(out, cx),
        Component::Capabilities => settings::capabilities(out, cx),
        Component::Appearance => settings::appearance(out, cx),
        Component::ResetHistory => settings::reset_history(out, cx),
        Component::Models => settings::models(out, cx),
        Component::Routes => settings::routes(out, cx),
        Component::Diagnostics => settings::diagnostics(out, cx),
        Component::Environment => settings::environment(out, cx),
        Component::NewRoute => settings::new_route(out, cx),
        Component::Tokens => settings::tokens(out, cx),
        Component::Columns { main, side, side_width } => columns(out, main, side, side_width.unwrap_or(SIDE_WIDTH), cx),
        Component::Panes { axis, resizable: _, panes } => code::panes(out, *axis, panes, cx),
        Component::Explorer => code::explorer(out, cx),
        Component::Editor => code::editor(out, cx),
        Component::Terminal => code::terminal(out, cx),
        Component::ThemePicker => code::theme_picker(out, cx),
        // `expand` has already replaced these; one left means a bug.
        Component::Use { name } => placeholder(out, "use", &format!("component \"{name}\" was not expanded")),
        Component::Ask { ask } => ask::draw(out, ask, dry_core::ask::State::Open, None, cx),
        Component::Show { show } => show::draw(out, show),
        Component::Analytics { part } => analytics::draw(out, *part, cx),
        Component::LiveView { part } => live::part(out, *part, cx),
        // A variant added to `dry_core::pages::Component` after this file.
        #[allow(unreachable_patterns)]
        other => placeholder(out, &type_name(other), "no web view for this component yet; it shows in the desktop app"),
    }
}

/// The side column's width when a page does not say (the window's `SIDE_WIDTH`).
pub const SIDE_WIDTH: f32 = 340.0;
/// The least room the main column keeps beside the side one (`MAIN_MIN`).
pub const MAIN_MIN: f32 = 420.0;

/// Two columns where there is room, else one, main first: the main
/// column wants `MAIN_MIN`, the side its width, so they wrap exactly when
/// the window's rule would make one column.
pub fn columns(out: &mut String, main: &[Component], side: &[Component], width: f32, cx: &Ctx) {
    let (mut m, mut s) = (String::new(), String::new());
    for c in main {
        component(&mut m, c, cx);
    }
    for c in side {
        component(&mut s, c, cx);
    }
    columns_html(out, &m, &s, width);
}

/// `columns` of two parts already drawn.
pub fn columns_html(out: &mut String, main: &str, side: &str, width: f32) {
    let _ = write!(
        out,
        "<div class=\"columns\"><div class=\"col-main\" style=\"flex-basis:{MAIN_MIN}px\">{main}</div><div class=\"col-side\" style=\"flex-basis:{width}px\">{side}</div></div>"
    );
}

// ---- small pieces every view uses (`crates/gpui/src/views/card.rs`) ----

/// The CSS class of a tone.
pub fn tone(t: Tone) -> &'static str {
    match t {
        Tone::Neutral => "tone-neutral",
        Tone::Good => "tone-good",
        Tone::Busy => "tone-busy",
        Tone::Bad => "tone-bad",
    }
}

/// An activity's tint as an inline custom property (`--tint`), for a
/// `.tinted` element; empty when idle.
pub fn tint(a: &Activity) -> String {
    a.token().map_or(String::new(), |name| format!(" style=\"--tint:var(--{name})\""))
}

/// A small label on the muted fill.
pub fn chip(text: &str) -> String {
    format!("<span class=\"chip\">{}</span>", esc(text))
}

/// A quiet line of small grey text.
pub fn note(text: &str) -> String {
    format!("<div class=\"note\">{}</div>", esc(text))
}

/// A small grey section label.
pub fn section(label: &str) -> String {
    format!("<div class=\"section\">{}</div>", esc(label))
}

/// A status: a dot in its tone, then `word: detail`.
pub fn status_line(s: &Status) -> String {
    format!("<div class=\"status\"><span class=\"dot {}\">●</span><span>{}</span></div>", tone(s.tone()), esc(&s.line()))
}

/// A thin bar `ratio` full.
pub fn gauge(ratio: f64, over: bool) -> String {
    let pct = (ratio.clamp(0.0, 1.0) * 100.0).round();
    format!("<div class=\"gauge{}\"><i style=\"width:{pct}%\"></i></div>", if over { " over" } else { "" })
}

/// Text as it is sent: monospace, on the surface fill.
pub fn mono(text: &str) -> String {
    format!("<pre class=\"mono\">{}</pre>", esc(if text.is_empty() { "(nothing)" } else { text }))
}

/// A disabled button, saying why it is off on the read-only server
/// (`Ctx::off` says it for wherever the page is drawn).
pub fn off(label: &str, class: &str) -> String {
    format!("<button class=\"btn {class}\" disabled title=\"{}\">{}</button>", esc(READ_ONLY), esc(label))
}

/// A section card that opens and closes (`views::fold`): its header and,
/// inside, `body`. Its open state is kept across live refreshes by `key`.
pub fn fold(out: &mut String, key: &str, open: bool, title: &str, count: Option<&str>, class: &str, body: impl FnOnce(&mut String)) {
    let _ = write!(
        out,
        "<details class=\"glass panel fold {class}\" data-key=\"{}\"{}><summary><span class=\"chev\"></span><b>{}</b>{}</summary>",
        esc(key),
        if open { " open" } else { "" },
        esc(title),
        count.map(chip).unwrap_or_default()
    );
    body(out);
    out.push_str("</details>");
}

/// A card (`dry_core::card::Card`): icon, title, kind chip, lines, status.
/// `href` makes it a link; `tint` an activity wash under it.
pub fn card(out: &mut String, c: &Card, href: Option<&str>, tint_style: &str, extra: impl FnOnce(&mut String)) {
    let (open, close) = match href {
        Some(h) => (format!("<a class=\"glass card tinted link\" href=\"{}\"{tint_style}>", esc(h)), "</a>"),
        None => (format!("<div class=\"glass card tinted\"{tint_style}>"), "</div>"),
    };
    out.push_str(&open);
    let _ = write!(
        out,
        "<div class=\"card-head\"><span class=\"icon\">{}</span><b class=\"title\">{}</b>{}</div>",
        esc(&c.icon),
        esc(&c.title),
        chip(&c.kind)
    );
    for line in c.lines.iter().filter(|l| !l.is_empty()) {
        out.push_str(&note(line));
    }
    out.push_str(&status_line(&c.status));
    extra(out);
    out.push_str(close);
}

/// Shown for a path no route matches.
pub fn missing(out: &mut String, path: &str) {
    let _ = write!(out, "<div class=\"intro\"><h2>Not found</h2>{}</div>", note(&format!("Nothing is registered for {path}.")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_and_queries_read_back() {
        assert_eq!(esc("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
        let q = Query::parse("open=crates,crates%2Fcore&file=README.md&mode=light&theme=light-plus&x=1");
        assert_eq!(q.open, ["crates", "crates/core"]);
        assert_eq!((q.file.as_deref(), q.mode, q.theme.as_deref()), (Some("README.md"), Some(Mode::Light), Some("light-plus")));
        assert_eq!(Query::parse(q.with(&q.open, q.file.as_deref()).trim_start_matches('?')), q);
        let bad = Query::parse("file=../secret&open=..%2Fx&theme=../x");
        assert_eq!(bad, Query::default(), "nothing outside the repo, no path as a theme");
        assert_eq!(decode("a%20b+c%zz%"), "a b c%zz%");
    }
}
