//! Everything a page is drawn from, read from disk once per request: the
//! routes and components (`dry_tools::load`), the active theme and this
//! machine's `state/local.md`, the store's `State`, `models.json` and
//! `sessions.json`. The same readers the GPUI window uses, from the same
//! root (`DRY_ROOT`, see `dry_tools::root`).
//!
//! Drawing a page only reads, host or not. The disk handed to the core for
//! snapshots (`ReadOnly`) refuses every write and every command, so
//! drawing can never change a file or start a process; the host's turns
//! write through their own disk (`host`).

use std::path::Path;

use dry_core::live::{Content, Host, Job, Proc};
use dry_core::models::Models;
use dry_core::pages::{self, ComponentFile};
use dry_core::routes::ViewState;
use dry_core::state::State;
use dry_html::world::{Access, Repo, World, lenient_view};

/// Everything a page is drawn from, as on disk now; with a host, its
/// state and its live turns instead of the store's copy, and its controls
/// live (`Access::Host`).
pub fn world(host: Option<&crate::host::Host>) -> World {
    let mut loaded = dry_tools::load();
    lenient(&mut loaded);
    let local_md = dry_tools::read_local_md();
    let mut view = ViewState::default();
    view.read_local_md(&local_md);
    let mut table = pages::table(&loaded.pages);
    for p in &view.removed {
        table.remove(p);
    }
    let (mut state, store, mut store_error) = read_state();
    let mut reported = dry_html::world::Reported::default();
    if let Some(h) = host {
        let snap = h.snapshot();
        state = snap.state;
        reported = snap.reported;
        store_error = snap.store_error;
    }
    let sessions = dry_tools::read_sessions();
    let programs = sessions
        .as_ref()
        .map(|s| s.tools.iter().map(|t| (t.name.clone(), t.start.first().and_then(|p| dry_tools::find_program(p)))).collect())
        .unwrap_or_default();
    let models = std::fs::read_to_string(dry_tools::root().join("models.json"))
        .map_err(|e| format!("cannot read models.json: {e}"))
        .and_then(|t| Models::from_json(&t).map_err(|e| format!("models.json does not parse: {e}")));
    let themes = dry_tools::list_themes();
    let active_theme = dry_tools::active_theme_in(&state);
    let disk = dry_tools::live::Disk::for_store(&store);
    if host.is_none() {
        // No host: the logs as the window (or the last host) left them.
        let (spans, daily) = dry_tools::spans::load_all(&disk.engines);
        reported.spans = spans;
        reported.daily = daily;
    }
    World {
        table,
        pages: loaded.pages.into_iter().collect(),
        components: loaded.components,
        local: loaded.local,
        errors: loaded.errors,
        view,
        state,
        store,
        store_error,
        theme: dry_tools::read_theme_named(&active_theme),
        local_theme: dry_tools::read_local_theme().ok().flatten(),
        named_themes: themes.iter().map(|n| (n.clone(), dry_tools::read_theme_named(n))).collect(),
        themes,
        active_theme,
        scenes: dry_tools::list_scenes().into_iter().map(|n| (n.clone(), dry_tools::read_scene(&n))).collect(),
        models,
        sessions,
        programs,
        repo_name: dry_tools::root().file_name().map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().to_string()),
        fonts: shipped_fonts(),
        served_by: format!("served by dry-server {} · root {}", env!("CARGO_PKG_VERSION"), dry_tools::root().display()),
        host: Box::new(ReadOnly(disk)),
        clock: dry_tools::live::clock(),
        repo: Box::new(Checkout),
        access: if host.is_some() { Access::Host } else { Access::ReadOnly },
        reported,
        connection: None,
        evidence_files: true,
    }
}

/// The font files in `assets/fonts/`, by name.
pub fn shipped_fonts() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dry_tools::root().join("assets/fonts")) else { return Vec::new() };
    dry_html::css::font_files(entries.flatten().filter_map(|e| e.file_name().to_str().map(String::from)))
}

/// The checkout, for the Code page: read from the repo root.
pub struct Checkout;

impl Repo for Checkout {
    fn list(&self, rel: &str) -> Result<Vec<(String, bool)>, String> {
        let entries = std::fs::read_dir(dry_tools::root().join(rel)).map_err(|e| e.to_string())?;
        Ok(entries.flatten().map(|e| (e.file_name().to_string_lossy().to_string(), e.path().is_dir())).collect())
    }
    fn read(&self, rel: &str, max: usize) -> Result<(Vec<u8>, u64), String> {
        use std::io::Read;
        let read = || -> std::io::Result<(Vec<u8>, u64)> {
            let file = std::fs::File::open(dry_tools::root().join(Path::new(rel)))?;
            let size = file.metadata()?.len();
            let mut bytes = Vec::new();
            file.take(max as u64).read_to_end(&mut bytes)?;
            Ok((bytes, size))
        };
        read().map_err(|e| e.to_string())
    }
}

/// The store's state, the way the window reads it (`crates/gpui/src/store.rs`):
/// the store over the default (`dry_tools::store::Layered`: its own files
/// win, the rest come from `default/`); a store with nothing in it reads
/// as the default alone.
pub fn read_state() -> (State, String, Option<String>) {
    let store = match dry_tools::store::open_default() {
        Ok(s) => s,
        Err(e) => return (State::default(), "none".into(), Some(e)),
    };
    let name = store.describe();
    match dry_tools::store::load_or_first(&*store) {
        Ok(state) => (state, name, None),
        Err(e) => (State::default(), name, Some(e)),
    }
}

/// A page or component file that did not read because it names a
/// component this build does not know still shows: read again leniently,
/// each unknown component becomes a placeholder (`UNKNOWN`). The error
/// stays listed.
fn lenient(loaded: &mut dry_tools::Loaded) {
    let root = dry_tools::root();
    for error in loaded.errors.clone() {
        let Some((file, _)) = error.split_once(": ") else { continue };
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if let Some(folder) = file.strip_prefix("routes/").and_then(|f| f.strip_suffix("page.json")) {
            let folder = folder.trim_end_matches('/');
            if folder.split('/').any(|s| pages::RESERVED.contains(&s)) {
                continue;
            }
            if let Some(page) = dry_html::world::lenient_page(value) {
                loaded.pages.push((pages::folder_to_pattern(folder), page));
            }
        } else if let Some(name) = file.strip_prefix("components/").and_then(|f| f.strip_suffix(".json"))
            && let Some(view) = value.get("view").cloned().map(lenient_view)
            && let Ok(file) = serde_json::from_value::<ComponentFile>(serde_json::json!({ "view": view }))
        {
            loaded.components.insert(name.to_string(), file.view);
        }
    }
}


/// The disk the core reads snapshots through (`dry_tools::live::Disk`),
/// with every write and command refused: drawing a page only reads.
pub struct ReadOnly(pub dry_tools::live::Disk);

const REFUSED: &str = "the web view only reads";

impl Host for ReadOnly {
    fn now(&self) -> i64 {
        self.0.now()
    }
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        self.0.list(root, max)
    }
    fn read(&self, root: &str, path: &str) -> Content {
        self.0.read(root, path)
    }
    fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err(REFUSED.into())
    }
    fn run(&self, _: &Job) -> Result<Option<i32>, String> {
        Err(REFUSED.into())
    }
    fn spawn(&self, _: &Job) -> Result<(), String> {
        Err(REFUSED.into())
    }
    fn status(&self, job: &Job) -> Proc {
        self.0.status(job)
    }
    fn tail(&self, job: &Job, lines: usize) -> String {
        self.0.tail(job, lines)
    }
}

