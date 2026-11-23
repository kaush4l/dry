//! Routes: every page of the app, held in one table that can be read and
//! changed at run time.
//!
//! A route is a path pattern (`/`, `/desk`, `/desk/:id`) plus how it
//! shows in the left panel. The panel is drawn from the table, a click
//! navigates to the route's path, and the shell draws the view component
//! registered for the matched pattern in the centre.
//!
//! `RouteTable` is the table: `list` to see the routes, `add` and `remove`
//! to change them, `resolve` to find the route for a path. It holds data
//! only; each shell pairs routes with its own view components (for GPUI,
//! `crates/gpui/src/router.rs`).
//!
//! What the person sees is a list of route elements: a navigation button
//! (icon + label) paired with the page it opens. `elements` makes that list
//! from the table, the live `ViewState` (where we are, which views are
//! hidden) and the engines, so every shell (GPUI, a terminal) draws the
//! same navigation.

use std::collections::BTreeSet;

/// How a route shows in the navigation. In files: `"yes"`, `"no"`,
/// `"per-engine"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Nav {
    /// Not listed; reached from another page.
    No,
    /// One button.
    #[default]
    Yes,
    /// One button per engine (`/desk/:id` → `/desk/0`, `/desk/1`, ...).
    PerEngine,
}

/// One page of the app. Made from a `page.json` (see `pages`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// The path pattern. A `:name` segment matches any one segment and
    /// hands it to the view as a parameter.
    pub pattern: String,
    /// The page's title (header, panel entry).
    pub title: String,
    /// The navigation button's icon: the glyph itself, written in the
    /// route's `page.json` (`"icon": "⌂"`). Any character; every shell,
    /// a terminal included, draws it as text.
    pub icon: String,
    pub nav: Nav,
    /// Where this route moved to (`page.json`'s `moved`): the route and
    /// every path under it open there instead (see `RouteTable::moved_to`).
    pub moved: Option<String>,
}

/// The standard routes: the ones the app ships and stands on. They cannot
/// be deleted, but each can be hidden from the navigation or turned off
/// (removed) and put back. Every other route is the person's own: hide or
/// delete it.
pub const STANDARD: [&str; 3] = [HOME, "/settings", "/code"];

/// The route that lists the routes (and so turns them back on): it can be
/// hidden, never removed.
pub const SETTINGS: &str = "/settings";

pub fn is_standard(pattern: &str) -> bool {
    STANDARD.contains(&pattern)
}

/// The live state of the views: where we are and what is hidden. Not the
/// app's data (that is `State`); this is how it is being looked at.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    /// The current path (`/`, `/desk/2`, ...).
    pub location: String,
    /// Patterns whose navigation buttons are hidden. A hidden view still
    /// works when navigated to; it is only left out of the navigation.
    pub hidden: BTreeSet<String>,
    /// Standard routes turned off: not in the table at all (their files
    /// stay, so they can be put back).
    pub removed: BTreeSet<String>,
    /// What the person chose with the panel button; `None` follows the screen.
    pub panel: Option<bool>,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState { location: HOME.to_string(), hidden: BTreeSet::new(), removed: BTreeSet::new(), panel: None }
    }
}

/// The heading of the routes section in `state/local.md`.
pub const LOCAL_HEADING: &str = "## Routes";

impl ViewState {
    /// Turn standard route `pattern` off, or back on. Only standard
    /// routes are turned off (the others are deleted); Settings never is,
    /// since it is where they come back from.
    pub fn toggle_removed(&mut self, pattern: &str) -> Result<(), String> {
        if !is_standard(pattern) {
            return Err(format!("{pattern} is not a standard route; delete it instead"));
        }
        if pattern == SETTINGS && !self.removed.contains(pattern) {
            return Err("Settings turns routes back on, so it can be hidden but not removed".into());
        }
        if !self.removed.remove(pattern) {
            self.removed.insert(pattern.to_string());
        }
        Ok(())
    }

    /// The routes section of `state/local.md`: one row per route that is
    /// not shown the usual way. Empty when every route is.
    pub fn to_local_md(&self) -> String {
        let rows: Vec<String> = self
            .removed
            .iter()
            .map(|p| format!("| {p} | removed |"))
            .chain(self.hidden.iter().filter(|p| !self.removed.contains(*p)).map(|p| format!("| {p} | hidden |")))
            .collect();
        if rows.is_empty() {
            return String::new();
        }
        format!("{LOCAL_HEADING}\n\n| route | shown |\n|---|---|\n{}\n", rows.join("\n"))
    }

    /// Read the routes section of `state/local.md` (other sections are
    /// left alone). `hidden` keeps the button out of the navigation;
    /// `removed` turns a standard route off (anything else is ignored, so
    /// a typo never removes a page of your own).
    pub fn read_local_md(&mut self, md: &str) {
        self.hidden.clear();
        self.removed.clear();
        for line in section(md, LOCAL_HEADING).lines() {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
            match cells[..] {
                [p, "hidden"] => {
                    self.hidden.insert(p.to_string());
                }
                [p, "removed"] if is_standard(p) && p != SETTINGS => {
                    self.removed.insert(p.to_string());
                }
                _ => {}
            }
        }
    }

    /// Where the app opens: home, or the first route left when home is off.
    pub fn start(&self, table: &RouteTable) -> String {
        if !self.removed.contains(HOME) {
            return HOME.to_string();
        }
        table.list().iter().find(|r| r.nav == Nav::Yes).map_or(SETTINGS.to_string(), |r| r.pattern.clone())
    }

    pub fn is_hidden(&self, pattern: &str) -> bool {
        self.hidden.contains(pattern)
    }

    /// Hide a shown view, show a hidden one.
    pub fn toggle_hidden(&mut self, pattern: &str) {
        if !self.hidden.remove(pattern) {
            self.hidden.insert(pattern.to_string());
        }
    }
}

/// A route element: one navigation button and the page it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteElement {
    /// The page this button opens.
    pub path: String,
    pub label: String,
    pub icon: String,
    /// Is its page the one on screen?
    pub active: bool,
    /// The engine it opens, for the live part of the navigation: one
    /// button per engine (the shell adds New thread and New session after them). None for a
    /// fixed route.
    pub engine: Option<usize>,
}

/// The navigation: first the fixed routes (each listed, not hidden route,
/// in table order), then the live part: one element per engine in
/// `engines`, from the `PerEngine` route. Fixed things stay put; engines
/// come and go below them.
pub fn elements(table: &RouteTable, view: &ViewState, engines: &[&str]) -> Vec<RouteElement> {
    let mut fixed = Vec::new();
    let mut live = Vec::new();
    for route in table.list().iter().filter(|r| !view.is_hidden(&r.pattern) && r.moved.is_none()) {
        match route.nav {
            Nav::No => {}
            Nav::Yes => fixed.push(RouteElement {
                path: route.pattern.clone(),
                label: route.title.clone(),
                icon: route.icon.clone(),
                active: is_active(route, &view.location),
                engine: None,
            }),
            Nav::PerEngine => {
                for (i, name) in engines.iter().enumerate() {
                    let path = engine_path(i);
                    live.push(RouteElement {
                        active: view.location == path,
                        path,
                        label: name.to_string(),
                        icon: route.icon.clone(),
                        engine: Some(i),
                    });
                }
            }
        }
    }
    fixed.extend(live);
    fixed
}

/// The text of section `heading` (a `## ` line) of `md`, up to the next
/// `## ` heading.
fn section<'a>(md: &'a str, heading: &str) -> &'a str {
    let Some(start) = md.find(&format!("{heading}\n")).or_else(|| md.strip_suffix(heading).map(|m| m.len())) else { return "" };
    let body = &md[start + heading.len()..];
    let end = body.find("\n## ").map_or(body.len(), |i| i + 1);
    &body[..end]
}

/// `md` with section `heading` replaced by `new` (a whole section, its
/// heading included, or empty to drop it); appended when there was none.
pub fn replace_section(md: &str, heading: &str, new: &str) -> String {
    let Some(start) = md.find(&format!("{heading}\n")).or_else(|| md.strip_suffix(heading).map(|m| m.len())) else {
        if new.is_empty() {
            return md.to_string();
        }
        let sep = if md.is_empty() || md.ends_with("\n\n") { "" } else if md.ends_with('\n') { "\n" } else { "\n\n" };
        return format!("{md}{sep}{new}");
    };
    let rest = &md[start + heading.len()..];
    let end = rest.find("\n## ").map_or(md.len(), |i| start + heading.len() + i + 1);
    let mut out = md[..start].to_string();
    out.push_str(new);
    if end < md.len() {
        if !new.is_empty() && !new.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&md[end..]);
    } else if new.is_empty() && !out.trim().is_empty() {
        // The last section dropped: no blank lines left behind it.
        out = format!("{}\n", out.trim_end());
    }
    out
}

/// The trail to `path` for the header: each level of the path that is a
/// route, from the top level down, as (path, title); the last is `path`
/// itself. Home heads every trail (while it is a route), so the way back
/// always starts there: `/settings/ui` → Home, Settings, UI tokens; every
/// level but the last is a link back up. Levels that are no route are
/// skipped.
pub fn trail(table: &RouteTable, path: &str) -> Vec<(String, String)> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    if let Some((route, _)) = table.resolve(HOME) {
        out.push((HOME.to_string(), route.title.clone()));
    }
    for n in 1..=segments.len() {
        let level = format!("/{}", segments[..n].join("/"));
        if let Some((route, _)) = table.resolve(&level) {
            out.push((level, route.title.clone()));
        }
    }
    out
}

/// `trail`, with each level named by what it shows: a per-engine level
/// (`/desk/2`) by the engine's name in `engines`, and a level whose
/// pattern ends in a parameter other than `:id` (`/desk/:id/artifacts/:name`)
/// by that parameter's value. Home › Desk › Builder › code.
pub fn trail_titled(table: &RouteTable, path: &str, engines: &[&str]) -> Vec<(String, String)> {
    trail(table, path)
        .into_iter()
        .map(|(level, title)| {
            let Some((route, params)) = table.resolve(&level) else { return (level, title) };
            let last = route.pattern.rsplit('/').next().and_then(|s| s.strip_prefix(':'));
            let named = match (route.nav, last) {
                (Nav::PerEngine, _) => params.index("id").and_then(|i| engines.get(i)).map(|n| n.to_string()),
                (_, Some(param)) if param != "id" => params.get(param).map(str::to_string),
                _ => None,
            };
            (level, named.unwrap_or(title))
        })
        .collect()
}

/// The path the app opens on.
pub const HOME: &str = "/";

/// The parameters a match pulled out of the path, e.g. `id = "2"`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    /// A parameter read as a number (`/desk/2` → `Some(2)`).
    pub fn index(&self, name: &str) -> Option<usize> {
        self.get(name)?.parse().ok()
    }
}

/// The table of routes, in panel order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteTable {
    routes: Vec<Route>,
}

impl RouteTable {
    /// Every route, in panel order.
    pub fn list(&self) -> &[Route] {
        &self.routes
    }

    /// Add `route` at the end. A route with the same pattern is replaced in
    /// place instead (keeping its position); returns the one it replaced.
    pub fn add(&mut self, route: Route) -> Option<Route> {
        match self.routes.iter_mut().find(|r| r.pattern == route.pattern) {
            Some(existing) => Some(std::mem::replace(existing, route)),
            None => {
                self.routes.push(route);
                None
            }
        }
    }

    /// Remove the route with `pattern`; returns it, or `None` if there was none.
    pub fn remove(&mut self, pattern: &str) -> Option<Route> {
        let index = self.routes.iter().position(|r| r.pattern == pattern)?;
        Some(self.routes.remove(index))
    }

    /// Where `path` opens now, if a route it is under has moved: the
    /// moved route's prefix swapped for its `moved` target, the rest kept
    /// (`/desk/2/live` → `/desk/2/live` for `/desk` moved to `/desk`).
    /// `None`: the path is where it is. Each shell sends the person on
    /// (an HTTP redirect, a replaced hash, a navigation), so old links work.
    pub fn moved_to(&self, path: &str) -> Option<String> {
        let got: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        self.routes.iter().find_map(|route| {
            let to = route.moved.as_deref()?;
            let want: Vec<&str> = route.pattern.split('/').filter(|s| !s.is_empty()).collect();
            if got.len() < want.len() || matches(&route.pattern, &format!("/{}", got[..want.len()].join("/"))).is_none() {
                return None;
            }
            let rest = got[want.len()..].join("/");
            let to = to.trim_end_matches('/');
            Some(match (to.is_empty(), rest.is_empty()) {
                (true, true) => HOME.to_string(),
                (false, true) => to.to_string(),
                _ => format!("{to}/{rest}"),
            })
        })
    }

    /// The route `path` belongs to, with its parameters. The first route
    /// that matches wins. `None` means no route fits.
    pub fn resolve(&self, path: &str) -> Option<(&Route, Params)> {
        self.routes.iter().find_map(|route| matches(&route.pattern, path).map(|params| (route, params)))
    }
}

/// Does `path` fit `pattern`? Returns the `:name` parameters when it does.
pub fn matches(pattern: &str, path: &str) -> Option<Params> {
    let want: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let got: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if want.len() != got.len() {
        return None;
    }
    let mut params = Vec::new();
    for (w, g) in want.iter().zip(&got) {
        match w.strip_prefix(':') {
            Some(name) => params.push((name.to_string(), g.to_string())),
            None if w == g => {}
            None => return None,
        }
    }
    Some(Params(params))
}

/// Is the panel entry for `route` the current one? A section stays lit on
/// its detail pages: `/desk` is active on `/desk/2`.
pub fn is_active(route: &Route, path: &str) -> bool {
    if route.pattern == HOME {
        return path == HOME;
    }
    let pattern = &route.pattern;
    path == pattern || path.starts_with(&format!("{pattern}/"))
}

/// The path of one engine's page.
pub fn engine_path(index: usize) -> String {
    format!("/desk/{index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(pattern: &str, title: &str, icon: &str, nav: Nav) -> Route {
        Route { pattern: pattern.into(), title: title.into(), icon: icon.into(), nav, moved: None }
    }

    fn table() -> RouteTable {
        let mut t = RouteTable::default();
        t.add(route("/", "Home", "⌂", Nav::Yes));
        t.add(route("/desk", "Desk", "◆", Nav::No));
        t.add(route("/desk/:id", "Engine", "◆", Nav::PerEngine));
        t
    }

    #[test]
    fn each_path_finds_its_route_and_its_parameters() {
        let t = table();
        assert_eq!(t.resolve("/").unwrap().0.title, "Home");
        let (route, params) = t.resolve("/desk/2").unwrap();
        assert_eq!(route.pattern, "/desk/:id");
        assert_eq!(params.index("id"), Some(2));
        assert!(t.resolve("/nope").is_none());
    }

    #[test]
    fn routes_can_be_listed_added_replaced_and_removed() {
        let mut t = table();
        assert_eq!(t.list().len(), 3);
        assert!(t.add(route("/notes", "Notes", "◐", Nav::Yes)).is_none());
        assert_eq!(t.list().last().unwrap().pattern, "/notes");
        let old = t.add(route("/", "Start", "⌂", Nav::Yes));
        assert_eq!(old.unwrap().title, "Home", "same pattern replaces");
        assert_eq!(t.list()[0].title, "Start", "and keeps its place");
        assert_eq!(t.remove("/notes").unwrap().title, "Notes");
        assert!(t.resolve("/notes").is_none());
        assert!(t.remove("/notes").is_none());
    }

    #[test]
    fn the_navigation_lists_one_element_per_engine_and_leaves_out_hidden_views() {
        let t = table();
        let mut view = ViewState { location: "/desk/1".into(), ..ViewState::default() };
        let nav = elements(&t, &view, &["Alpha", "Beta"]);
        let labels: Vec<&str> = nav.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Home", "Alpha", "Beta"], "/desk itself is not listed");
        assert_eq!(nav[2].path, "/desk/1");
        assert!(nav[2].active && !nav[1].active && !nav[0].active);
        assert_eq!(nav[1].icon, "◆");
        assert_eq!((nav[1].engine, nav[0].engine), (Some(0), None));

        view.toggle_hidden("/desk/:id");
        assert_eq!(elements(&t, &view, &["Alpha"]).len(), 1, "engines hidden");
        assert!(t.resolve("/desk/0").is_some(), "a hidden view still opens");
        view.toggle_hidden("/desk/:id");
        assert_eq!(elements(&t, &view, &["Alpha"]).len(), 2, "shown again");
    }

    #[test]
    fn fixed_routes_come_first_and_the_live_engines_after_them() {
        let mut t = table();
        t.add(route("/settings", "Settings", "⚙", Nav::Yes));
        let nav = elements(&t, &ViewState::default(), &["Alpha"]);
        let labels: Vec<&str> = nav.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Home", "Settings", "Alpha"], "Settings is after the engines in the table, but fixed");
    }

    #[test]
    fn a_section_stays_active_on_its_detail_pages_and_home_only_on_itself() {
        let t = table();
        let desk = t.resolve("/desk").unwrap().0;
        let home = t.resolve("/").unwrap().0;
        assert!(is_active(desk, "/desk/3"));
        assert!(!is_active(home, "/desk"));
        assert!(is_active(home, "/"));
    }

    #[test]
    fn standard_routes_hide_or_turn_off_and_are_kept_in_local_md() {
        let mut v = ViewState::default();
        assert!(v.toggle_removed("/notes").unwrap_err().contains("delete it instead"));
        assert!(v.toggle_removed(SETTINGS).unwrap_err().contains("hidden but not removed"));
        v.toggle_removed("/code").unwrap();
        v.toggle_hidden(SETTINGS);
        let md = v.to_local_md();
        assert_eq!(md, "## Routes\n\n| route | shown |\n|---|---|\n| /code | removed |\n| /settings | hidden |\n");

        // Kept beside the other local choices, read back the same.
        let local = "# Local\n\n## Font\n\n| token | value |\n|---|---|\n| font | Amarante |\n";
        let both = replace_section(local, LOCAL_HEADING, &md);
        assert!(both.starts_with(local) && both.ends_with(&md), "{both}");
        let mut back = ViewState::default();
        back.read_local_md(&both);
        assert_eq!((back.hidden.clone(), back.removed.clone()), (v.hidden.clone(), v.removed.clone()));

        // Turned back on: the section goes, the font stays.
        v.toggle_removed("/code").unwrap();
        v.toggle_hidden(SETTINGS);
        assert_eq!(replace_section(&both, LOCAL_HEADING, &v.to_local_md()), local);
        let mid = replace_section(&format!("{md}\n## Font\nx\n"), LOCAL_HEADING, "");
        assert_eq!(mid, "## Font\nx\n", "a section in the middle is replaced in place");

        // A typo or a page of your own is never turned off from the file.
        back.read_local_md("## Routes\n| /notes | removed |\n| /settings | removed |\n| / | removed |\n");
        assert_eq!(back.removed.iter().map(String::as_str).collect::<Vec<_>>(), ["/"]);
    }


    #[test]
    fn the_header_trail_links_back_up_each_level() {
        let mut t = RouteTable::default();
        for (p, title) in [(HOME, "Home"), ("/settings", "Settings"), ("/settings/ui", "UI tokens"), ("/desk", "Desk"), ("/desk/:id", "Engine")] {
            t.add(Route { pattern: p.into(), title: title.into(), icon: String::new(), nav: Nav::Yes, moved: None });
        }
        let titles_in = |t: &RouteTable, p: &str| trail(t, p).into_iter().map(|(p, t)| format!("{p}={t}")).collect::<Vec<_>>();
        let titles = |p: &str| titles_in(&t, p);
        assert_eq!(titles("/settings/ui"), ["/=Home", "/settings=Settings", "/settings/ui=UI tokens"]);
        assert_eq!(titles("/desk/2"), ["/=Home", "/desk=Desk", "/desk/2=Engine"]);
        assert_eq!(titles("/"), ["/=Home"]);
        assert_eq!(titles("/nope"), ["/=Home"], "a page that is no route still has the way home");
        let mut t = t.clone();
        t.remove(HOME);
        assert_eq!(titles_in(&t, "/settings/ui"), ["/settings=Settings", "/settings/ui=UI tokens"], "home turned off: no head");
    }

    #[test]
    fn the_trail_names_an_engine_and_its_artifact() {
        let mut t = RouteTable::default();
        for (p, title, nav) in [
            (HOME, "Home", Nav::Yes),
            ("/desk", "Desk", Nav::Yes),
            ("/desk/:id", "Engine", Nav::PerEngine),
            ("/desk/:id/artifacts/:name", "Artifact", Nav::No),
        ] {
            t.add(Route { pattern: p.into(), title: title.into(), icon: String::new(), nav, moved: None });
        }
        let titles: Vec<String> = trail_titled(&t, "/desk/1/artifacts/code", &["A", "Builder"]).into_iter().map(|(p, t)| format!("{p}={t}")).collect();
        assert_eq!(titles, ["/=Home", "/desk=Desk", "/desk/1=Builder", "/desk/1/artifacts/code=code"]);
        let gone: Vec<String> = trail_titled(&t, "/desk/7", &["A"]).into_iter().map(|(_, t)| t).collect();
        assert_eq!(gone, ["Home", "Desk", "Engine"], "no such engine: the route's title");
    }

    #[test]
    fn a_moved_route_sends_it_and_every_path_under_it_to_the_new_place() {
        let mut t = table();
        t.add(Route { pattern: "/engines".into(), title: "Engines".into(), icon: String::new(), nav: Nav::No, moved: Some("/desk".into()) });
        assert_eq!(t.moved_to("/engines").as_deref(), Some("/desk"));
        assert_eq!(t.moved_to("/engines/2").as_deref(), Some("/desk/2"));
        assert_eq!(t.moved_to("/engines/2/artifacts/code/live").as_deref(), Some("/desk/2/artifacts/code/live"));
        assert_eq!(t.moved_to("/desk/2"), None, "where it moved to stays");
        assert_eq!(t.moved_to("/enginesx"), None, "a whole segment only");
        assert_eq!(t.moved_to("/"), None);
        let nav = elements(&t, &ViewState::default(), &[]);
        assert!(nav.iter().all(|e| e.path != "/engines"), "a moved route is not listed");
    }

}
