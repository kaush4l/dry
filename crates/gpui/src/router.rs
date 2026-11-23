//! The router: the routes read from the `routes/` folder, each with its
//! view (a list of components), and the `components/` files they `use`.
//! A route folder may bring its own components and token overrides
//! (`dry_core::pages::Local`); its components are found before the app's.
//!
//! Routes are files, not code (see `dry_core::pages`): adding a page is
//! adding a folder with a `page.json`, which the app (Settings, New page),
//! the `dry` command, or an editor can do. `dry_tools` reads them; the app
//! calls `Router::load` again whenever they change on disk.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use dry_core::pages::{self, Component, Local, Page};
use dry_core::routes::{Params, Route, RouteTable};

/// Routes, their views and the component files.
pub struct Router {
    table: RouteTable,
    /// Pattern → the page that made it (its view).
    pages: HashMap<String, Page>,
    /// `components/<name>.json`, by name.
    components: BTreeMap<String, Vec<Component>>,
    /// Pattern → the route folder's own components, overrides and themes.
    local: BTreeMap<String, Local>,
    /// Files that could not be read, shown in Settings.
    pub errors: Vec<String>,
    /// Standard routes turned off: out of the table, kept to turn back on.
    pub removed: Vec<Route>,
}

impl Router {
    /// Read `routes/` and `components/` from disk, leaving out the
    /// standard routes in `removed` (turned off in Settings).
    pub fn load(removed: &BTreeSet<String>) -> Router {
        let loaded = dry_tools::load();
        let mut table = pages::table(&loaded.pages);
        let removed = removed.iter().filter_map(|p| table.remove(p)).collect();
        Router {
            table,
            pages: loaded.pages.into_iter().collect(),
            components: loaded.components,
            local: loaded.local,
            errors: loaded.errors,
            removed,
        }
    }

    /// Every route, in panel order.
    pub fn routes(&self) -> &[Route] {
        self.table.list()
    }

    /// The route table itself (the core builds the navigation from it).
    pub fn table(&self) -> &RouteTable {
        &self.table
    }

    /// The route for `path`, its parameters, and its view with every `use`
    /// already replaced by the component it names.
    pub fn resolve(&self, path: &str) -> Option<(Route, Params, Vec<Component>)> {
        let (route, params) = self.table.resolve(path)?;
        let page = self.pages.get(&route.pattern)?;
        let view = pages::expand(&page.view, &pages::find(self.local.get(&route.pattern), &self.components));
        Some((route.clone(), params, view))
    }

    /// The page `path` opens, and its folder's own files (if it has any).
    pub fn page(&self, path: &str) -> Option<(&Page, Option<&Local>)> {
        let (route, _) = self.table.resolve(path)?;
        Some((self.pages.get(&route.pattern)?, self.local.get(&route.pattern)))
    }

    /// The pattern `path` belongs to (`/desk/:id` for `/desk/2`).
    pub fn pattern(&self, path: &str) -> Option<String> {
        self.table.resolve(path).map(|(route, _)| route.pattern.clone())
    }

    /// Choose the route's theme in memory too, so it shows before the
    /// file watcher reads `themes/active` back.
    pub fn set_theme(&mut self, pattern: &str, name: &str) {
        if let Some(local) = self.local.get_mut(pattern) {
            local.active = Some(name.to_string());
        }
    }
}
