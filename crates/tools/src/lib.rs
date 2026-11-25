//! The files the app is built from, read and written in one place: the
//! `routes/` folders, the `components/` files and the `themes/`. Both the app
//! (crates/gpui, which reads them and creates or deletes routes at run
//! time) and the `dry` command (src/main.rs) use these functions, so there
//! is one way to find, read and write each file.
//!
//! The formats are in `dry_core` (`pages`, `theme`); this is only the disk.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use dry_core::pages::{self, Component, ComponentFile, Local, Page, Snapshot};
use dry_core::response::{Format, Responses};
use dry_core::session::Sessions;
use dry_core::look::Kind;
use dry_core::theme::{Overrides, Theme};

pub mod app_tools;
pub mod capability;
pub mod cdp;
pub mod cli;
pub mod companion;
pub mod evidence;
pub mod host;
pub mod live;
pub mod looks;
pub mod mcp;
pub mod net;
pub mod settings;
pub mod spans;
pub mod store;
pub mod subscriptions;
pub mod worker;

pub type Result<T = ()> = std::result::Result<T, String>;

/// The repo root: `DRY_ROOT` if set, else two folders up from this crate.
pub fn root() -> PathBuf {
    let root = std::env::var("DRY_ROOT").map(PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    root.canonicalize().unwrap_or(root)
}

pub fn routes_dir() -> PathBuf {
    root().join("routes")
}

pub fn components_dir() -> PathBuf {
    root().join("components")
}

/// The themes: one folder each, `themes/<name>/theme.md` (`dry_core::look`).
pub fn themes_dir() -> PathBuf {
    root().join("themes")
}

/// The scenes: one folder each, `scenes/<name>/scene.md`; app.md's
/// `scene:` names the one in use.
pub fn scenes_dir() -> PathBuf {
    root().join("scenes")
}

/// The theme used when none is chosen, or the chosen one is gone.
pub const DEFAULT_THEME: &str = "default";

/// `themes/<name>/theme.md`.
pub fn theme_file_named(name: &str) -> PathBuf {
    looks::main_file(Kind::Theme, name)
}

/// The active theme's file: what the token tools read and write.
pub fn theme_file() -> PathBuf {
    theme_file_named(&active_theme())
}

/// `routes/<folder>/page.json` for a path like `/notes` or `/desk/:id`.
pub fn page_file(pattern: &str) -> PathBuf {
    routes_dir().join(pages::pattern_to_folder(pattern)).join("page.json")
}

/// A path as a pattern: always starts with `/`, no trailing `/`.
pub fn pattern(path: &str) -> String {
    pages::folder_to_pattern(&pages::pattern_to_folder(path))
}

pub(crate) fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write a file, making its folders. Temp file + rename, so a reader (the
/// app checks every second) never sees half a file.
pub(crate) fn write(path: &Path, text: &str) -> Result {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- reading everything ------------------------------------------------------

/// Every route and component on disk, and the files that could not be read
/// (one bad file does not hide the others).
#[derive(Default)]
pub struct Loaded {
    /// Every `page.json` under `routes/`, with its pattern.
    pub pages: Vec<(String, Page)>,
    /// Every `components/*.json`, by name.
    pub components: BTreeMap<String, Vec<Component>>,
    /// Pattern → what its route folder holds besides `page.json` (its own
    /// components, overrides and themes). Only routes that have any.
    pub local: BTreeMap<String, Local>,
    pub errors: Vec<String>,
}

pub fn load() -> Loaded {
    let mut loaded = Loaded::default();
    walk(&routes_dir(), "", &mut loaded);
    loaded.components = read_components(&components_dir(), "components", &mut loaded.errors);
    loaded
}

/// Every `*.json` in `dir` as a component file, by name. `label` names
/// the folder in errors.
fn read_components(dir: &Path, label: &str, errors: &mut Vec<String>) -> BTreeMap<String, Vec<Component>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_stem().and_then(|n| n.to_str()).filter(|_| path.extension().is_some_and(|e| e == "json")) else {
            continue;
        };
        match read(&path).and_then(|t| ComponentFile::from_json(&t)) {
            Ok(file) => {
                out.insert(name.to_string(), file.view);
            }
            Err(e) => errors.push(format!("{label}/{name}.json: {e}")),
        }
    }
    out
}

/// What a route folder holds besides its `page.json` (see `pages::Local`):
/// `components/*.json`, `overrides.md`, `themes/*.md` and `themes/active`.
/// A file that does not read is reported and left out; the rest still count.
fn read_local(dir: &Path, folder: &str, errors: &mut Vec<String>) -> Local {
    let label = if folder.is_empty() { "routes".to_string() } else { format!("routes/{folder}") };
    let mut local = Local { components: read_components(&dir.join("components"), &format!("{label}/components"), errors), ..Local::default() };
    let overrides = dir.join("overrides.md");
    if overrides.exists() {
        match read(&overrides).and_then(|t| Overrides::from_md(&t)) {
            Ok(o) => local.overrides = o,
            Err(e) => errors.push(format!("{label}/overrides.md: {e}")),
        }
    }
    let themes = dir.join("themes");
    if let Ok(entries) = fs::read_dir(&themes) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_stem().and_then(|n| n.to_str()).filter(|_| path.extension().is_some_and(|e| e == "md")) else {
                continue;
            };
            match read(&path).and_then(|t| Overrides::from_md(&t)) {
                Ok(o) => {
                    local.themes.insert(name.to_string(), o);
                }
                Err(e) => errors.push(format!("{label}/themes/{name}.md: {e}")),
            }
        }
        local.active = fs::read_to_string(themes.join("active")).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    }
    local
}

/// Read `dir/page.json` (route `folder`) and the page's own files, then
/// every sub-folder that is a route (not one of `pages::RESERVED`).
fn walk(dir: &Path, folder: &str, loaded: &mut Loaded) {
    let file = dir.join("page.json");
    if file.exists() {
        match read(&file).and_then(|t| Page::from_json(&t)) {
            Ok(page) => {
                let pattern = pages::folder_to_pattern(folder);
                let local = read_local(dir, folder, &mut loaded.errors);
                if local != Local::default() {
                    loaded.local.insert(pattern.clone(), local);
                }
                loaded.pages.push((pattern, page));
            }
            Err(e) => loaded.errors.push(format!("routes/{folder}/page.json: {e}")),
        }
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut subs: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().is_dir() && !pages::RESERVED.contains(&e.file_name().to_string_lossy().as_ref()))
        .collect();
    subs.sort_by_key(|e| e.file_name());
    for sub in subs {
        let name = sub.file_name().to_string_lossy().to_string();
        let child = if folder.is_empty() { name } else { format!("{folder}/{name}") };
        walk(&sub.path(), &child, loaded);
    }
}

// ---- routes ------------------------------------------------------------------

/// Choose theme `name` of the route at `pattern`: writes
/// `routes/<folder>/themes/active`, so the choice travels with the folder.
/// Err if the route has no `themes/<name>.md`.
pub fn set_route_theme(pattern: &str, name: &str) -> Result {
    let themes = routes_dir().join(pages::pattern_to_folder(pattern)).join("themes");
    if !dry_core::look::valid_name(name) || !themes.join(format!("{name}.md")).exists() {
        return Err(format!("route {pattern} has no theme \"{name}\""));
    }
    write(&themes.join("active"), &format!("{name}\n"))
}

pub fn read_page(pattern: &str) -> Result<Page> {
    Page::from_json(&read(&page_file(pattern))?)
}

pub fn write_page(pattern: &str, page: &Page) -> Result {
    write(&page_file(pattern), &page.to_json())
}

/// Create the route at `path` with `title` and `icon` and a starting view
/// (its title as a heading). Err if the route already exists. Returns its
/// pattern.
pub fn create_page(path: &str, title: &str, icon: &str) -> Result<String> {
    let pattern = pattern(path);
    if pattern == "/" || page_file(&pattern).exists() {
        return Err(format!("{pattern} already exists"));
    }
    if title.trim().is_empty() {
        return Err("a page needs a title".into());
    }
    let mut page = Page::new(title.trim());
    if !icon.trim().is_empty() {
        page.icon = icon.trim().to_string();
    }
    // Between the engines (10) and UI (80): new pages sort by path there.
    page.order = 50;
    page.view.push(Component::Heading { text: title.trim().to_string() });
    write_page(&pattern, &page)?;
    Ok(pattern)
}

/// Add a component to the end of a route's view.
pub fn add_component(path: &str, component: Component) -> Result {
    let pattern = pattern(path);
    if !page_file(&pattern).exists() {
        return Err(format!("no route {pattern}; create it first"));
    }
    let mut page = read_page(&pattern)?;
    page.view.push(component);
    write_page(&pattern, &page)
}

/// Delete a route's `page.json`, and the folders that leaves empty.
/// `/` cannot be deleted.
pub fn delete_page(path: &str) -> Result<String> {
    let pattern = pattern(path);
    if dry_core::routes::is_standard(&pattern) {
        return Err(format!("{pattern} is a standard route: hide it or turn it off in Settings, it cannot be deleted"));
    }
    let file = page_file(&pattern);
    fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut dir = file.parent().map(Path::to_path_buf);
    while let Some(d) = dir.filter(|d| *d != routes_dir()) {
        if fs::remove_dir(&d).is_err() {
            break; // not empty: it holds other routes
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    Ok(pattern)
}

// ---- components ----------------------------------------------------------------

pub fn component_file(name: &str) -> PathBuf {
    components_dir().join(format!("{name}.json"))
}

/// Create `components/<name>.json`, or add `component` to it.
pub fn add_to_component(name: &str, component: Option<Component>) -> Result {
    let file = component_file(name);
    let mut cf = if file.exists() { ComponentFile::from_json(&read(&file)?)? } else { ComponentFile::default() };
    cf.view.extend(component);
    write(&file, &cf.to_json())
}

// ---- export / import -------------------------------------------------------------

pub fn snapshot() -> Result<Snapshot> {
    let loaded = load();
    if let Some(e) = loaded.errors.first() {
        return Err(e.clone());
    }
    Ok(Snapshot { routes: loaded.pages.into_iter().collect(), components: loaded.components })
}

/// Write every route and component of `snap` back as files. Files of routes
/// or components not in `snap` are left alone. Returns what was written.
pub fn import(snap: &Snapshot) -> Result<Vec<String>> {
    let mut done = Vec::new();
    for (pattern, page) in &snap.routes {
        write_page(pattern, page)?;
        done.push(format!("route     {pattern}"));
    }
    for (name, view) in &snap.components {
        write(&component_file(name), &ComponentFile { view: view.clone() }.to_json())?;
        done.push(format!("component {name}"));
    }
    Ok(done)
}

// ---- sessions ------------------------------------------------------------------

/// `sessions.json`: the agent programs an engine can be run by.
pub fn sessions_file() -> PathBuf {
    root().join("sessions.json")
}

/// Where `program` is: itself if it is a path, else the first match in
/// the `PATH` folders. None: it is not installed (or not on `PATH`).
pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Path::new(program).is_file().then(|| PathBuf::from(program));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|p| p.is_file())
}

/// Read `sessions.json`. Missing or broken is an error (no program is
/// named in the code), shown where the session buttons would be.
pub fn read_sessions() -> Result<Sessions> {
    Sessions::from_json(&read(&sessions_file())?).map_err(|e| format!("sessions.json: {e}"))
}

// ---- capabilities ----------------------------------------------------------------

/// `voice.json`: how this machine records speech and turns it into text.
pub fn voice_file() -> PathBuf {
    root().join("voice.json")
}

pub fn read_voice() -> Result<dry_core::composer::Voice> {
    dry_core::composer::Voice::from_json(&read(&voice_file())?).map_err(|e| format!("voice.json: {e}"))
}

/// Check what this runtime can do, once, at start. Voice: `voice.json`
/// reads, its programs are on `PATH`, and its `check` lists a microphone.
/// Nothing is recorded. Blocking (the check runs a program for a moment).
pub fn capabilities() -> dry_core::composer::Capabilities {
    use dry_core::card::Status;
    use dry_core::composer::{Capabilities, Capability};
    let mut caps = Capabilities::default();
    let voice = match read_voice() {
        Err(e) => Status::Missing(e),
        Ok(v) => {
            let found: Vec<(&str, bool)> = v.programs().into_iter().map(|p| (p, find_program(p).is_some())).collect();
            let output = v.check.split_first().filter(|_| found.iter().all(|(_, ok)| *ok)).and_then(|(bin, args)| {
                let out = std::process::Command::new(bin).args(args).stdin(std::process::Stdio::null()).output().ok()?;
                Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
            });
            v.status(&found, output.as_deref())
        }
    };
    caps.set(Capability::Voice, voice);
    caps
}

// ---- responses -------------------------------------------------------------------

/// `mcp/dry.json`: the MCP servers a new engine starts with (the app's own
/// tools, served by `dry mcp`). Claude Code sessions get it too.
pub fn default_mcp_file() -> PathBuf {
    root().join("mcp").join("dry.json")
}

pub fn default_mcp() -> Result<String> {
    read(&default_mcp_file())
}

/// `responses.md`: the response contract a new model engine starts with.
pub fn responses_file() -> PathBuf {
    root().join("responses.md")
}

/// Read `responses.md`: the contract and its format (`toon` if it names none).
pub fn read_responses() -> Result<(Responses, Format)> {
    let (r, f) = Responses::from_md(&read(&responses_file())?).map_err(|e| format!("responses.md: {e}"))?;
    Ok((r, f.unwrap_or_default()))
}

// ---- theme ---------------------------------------------------------------------

/// Every theme's name (the folders in `themes/` with a `theme.md`), sorted.
pub fn list_themes() -> Vec<String> {
    looks::list(Kind::Theme)
}

/// The theme `state` says (app.md's `theme:`), if there is such a folder;
/// else `default`.
pub fn active_theme_in(state: &dry_core::state::State) -> String {
    state.theme.clone().filter(|name| dry_core::look::valid_name(name) && theme_file_named(name).exists()).unwrap_or_else(|| DEFAULT_THEME.to_string())
}

/// The active theme's name, from the store's app.md (`theme:`). No such
/// line, or a name with no folder: `default`.
pub fn active_theme() -> String {
    match store::open_default().and_then(|s| store::load_or_first(&*s)) {
        Ok(state) => active_theme_in(&state),
        Err(_) => DEFAULT_THEME.to_string(),
    }
}

/// Make `name` the active theme: app.md's `theme:` in the store. Err if
/// there is no `themes/<name>/`. A running host sets it in its own state
/// instead (`Host::set_theme`), so the two never race.
pub fn set_active_theme(name: &str) -> Result {
    looks::exists(Kind::Theme, name)?;
    let store = store::open_default()?;
    let mut state = store::load_or_first(&*store)?;
    state.theme = Some(name.to_string());
    store.save(&state)
}

/// Read `themes/<name>/theme.md`. An error names the file.
pub fn read_theme_named(name: &str) -> Result<Theme> {
    Theme::from_md(&looks::read_main(Kind::Theme, name)?).map_err(|e| format!("themes/{name}/theme.md {e}"))
}

/// Create theme `name` as a copy of theme `from` (its whole folder), a
/// template to edit. Returns the new main file.
pub fn new_theme(name: &str, from: &str) -> Result<PathBuf> {
    looks::add(Kind::Theme, name, from)
}

/// Read the active theme.
pub fn read_theme() -> Result<Theme> {
    read_theme_named(&active_theme())
}

/// This machine's own token choices, `state/local.md` (not shared, not in
/// git): the same tables as an override set, laid over the active theme.
pub fn local_theme_file() -> PathBuf {
    root().join("state").join("local.md")
}

/// The text of `state/local.md`; empty when there is none.
pub fn read_local_md() -> String {
    fs::read_to_string(local_theme_file()).unwrap_or_default()
}

/// Write the routes section of `state/local.md` (which routes are hidden
/// or turned off), keeping the rest of the file.
pub fn save_local_routes(view: &dry_core::routes::ViewState) -> Result {
    use dry_core::routes::{LOCAL_HEADING, replace_section};
    let file = local_theme_file();
    let md = replace_section(&read_local_md(), LOCAL_HEADING, &view.to_local_md());
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write(&file, &md)
}

/// Read `state/local.md`. None when there is no such file.
pub fn read_local_theme() -> Result<Option<Overrides>> {
    let file = local_theme_file();
    if !file.exists() {
        return Ok(None);
    }
    Overrides::from_md(&read(&file)?).map(Some).map_err(|e| format!("state/local.md {e}"))
}

/// Write the active theme.
pub fn save_theme(theme: &Theme) -> Result {
    write(&theme_file(), &theme.to_md())
}

/// Write theme `name`'s main file.
pub fn write_theme_named(name: &str, theme: &Theme) -> Result {
    looks::exists(Kind::Theme, name)?;
    write(&theme_file_named(name), &theme.to_md())
}

// ---- scenes --------------------------------------------------------------------

/// Every scene's name (the folders in `scenes/` with a `scene.md`), sorted.
pub fn list_scenes() -> Vec<String> {
    looks::list(Kind::Scene)
}

/// Read `scenes/<name>/scene.md`. An error names the file.
pub fn read_scene(name: &str) -> Result<dry_core::scene::Scene> {
    let text = looks::read_main(Kind::Scene, name)?;
    dry_core::scene::Scene::from_md(name, &text).map_err(|e| format!("scenes/{name}/scene.md: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_on_disk_reads_with_every_token() {
        let names = list_themes();
        assert!(names.iter().any(|n| n == DEFAULT_THEME) && names.iter().any(|n| n == "terminal"), "{names:?}");
        for name in names {
            let theme = read_theme_named(&name).unwrap();
            assert_eq!(theme.missing(), Vec::<&str>::new(), "themes/{name}/theme.md");
        }
        let default = dry_core::shipped::state().unwrap();
        assert_eq!(active_theme_in(&default), DEFAULT_THEME, "default/app.md names the default theme");
    }

    #[test]
    fn a_route_folder_brings_its_own_components_overrides_and_themes() {
        let dir = std::env::temp_dir().join(format!("dry-local-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let page = dir.join("routes/code");
        write(&page.join("page.json"), r#"{"title":"Code","view":[{"type":"use","name":"bar"}]}"#).unwrap();
        write(&page.join("components/bar.json"), r#"{"view":[{"type":"explorer"}]}"#).unwrap();
        write(&page.join("overrides.md"), "## Sizes\n| radius | 0 |\n").unwrap();
        write(&page.join("themes/dark.md"), "## Colors\n| accent | #FF0000 | #FF0000 |\n").unwrap();
        write(&page.join("themes/broken.md"), "## Colors\n| nope | #FF0000 | #FF0000 |\n").unwrap();
        write(&page.join("themes/active"), "dark\n").unwrap();
        write(&page.join("assets/logo.txt"), "not a route").unwrap();
        let mut loaded = Loaded::default();
        walk(&dir.join("routes"), "", &mut loaded);
        let patterns: Vec<&str> = loaded.pages.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(patterns, ["/code"], "components/, themes/ and assets/ are not routes");
        let local = &loaded.local["/code"];
        assert_eq!(local.components["bar"], vec![Component::Explorer]);
        assert_eq!(local.overrides.numbers["radius"], 0.0);
        assert_eq!((local.theme(), local.layers().len()), (Some("dark"), 2));
        assert!(!local.themes.contains_key("broken"));
        assert!(loaded.errors.iter().any(|e| e.contains("routes/code/themes/broken.md")), "{:?}", loaded.errors);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_code_route_ships_as_one_folder_that_reads_whole() {
        let loaded = load();
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let (_, page) = loaded.pages.iter().find(|(p, _)| p == "/code").expect("routes/code/page.json");
        let local = &loaded.local["/code"];
        let view = pages::expand(&page.view, &pages::find(Some(local), &loaded.components));
        assert!(pages::fills(&view), "the page is laid out in panes");
        let json = serde_json::to_string(&view).unwrap();
        for part in ["explorer", "editor", "terminal", "theme-picker"] {
            assert!(json.contains(&format!(r#""type":"{part}""#)), "{part} in {json}");
        }
        let names: Vec<&str> = local.themes.keys().map(String::as_str).collect();
        assert_eq!(names, ["dark-modern", "dark-plus", "high-contrast-dark", "light-modern", "light-plus"]);
        assert_eq!(local.overrides.font.as_deref(), Some(".SystemUIFont"), "the UI in the system font");
        assert_eq!(local.overrides.font_mono.as_deref(), Some("Menlo"), "code in a monospace one");
        assert_eq!(local.layers().len(), 2, "its overrides, then the chosen theme");
        assert!(local.themes.values().all(|t| t.colors.len() == 2 * 29), "every theme sets the same 29 colours, light and dark");
    }

    #[test]
    fn every_scene_on_disk_reads_and_the_default_names_one_that_exists() {
        let scenes = list_scenes();
        assert_eq!(scenes, ["abyss", "cat", "terminal"]);
        for name in &scenes {
            read_scene(name).unwrap();
        }
        let default = dry_core::shipped::state().unwrap();
        let look = dry_core::scene::settings(&default, &read_theme_named(DEFAULT_THEME).unwrap(), &[]);
        let scene = look.scene.expect("default/app.md names a scene");
        assert!(scenes.contains(&scene), "default/app.md names scene {scene}, which is not in scenes/");
        assert!(read_scene("../themes/default").is_err());
    }

    #[test]
    fn theme_names_are_plain_file_names() {
        assert!(dry_core::look::valid_name("my-theme_2"));
        assert!(!dry_core::look::valid_name("") && !dry_core::look::valid_name("../x") && !dry_core::look::valid_name("a.b"));
        assert!(set_active_theme("no-such-theme").is_err());
        assert!(new_theme(DEFAULT_THEME, "terminal").is_err(), "exists already");
        assert!(new_theme("x", "no-such-theme").is_err());
        assert!(!theme_file_named("x").exists());
    }
}
