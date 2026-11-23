//! The store: the only code that touches the disk. The app state (the
//! list of engines) goes through `dry_tools::store`: a folder of markdown
//! (`md:state`, the default) or a SQLite file (`DRY_STORE=sqlite:state.db`),
//! both giving the same `State`.

use std::fs;
use std::path::Path;

use dry_core::models::Models;
use dry_core::state::State;
use dry_core::theme::Theme;

/// What `load` found.
pub struct Loaded {
    pub state: State,
    /// Which store (`md:/…/state`), shown in the footer.
    pub store: String,
    /// Why it could not be read. While set, Save refuses, so a store that
    /// did not read is never overwritten with an empty state.
    pub error: Option<String>,
}

/// Read the state from the store, over the default
/// (`dry_tools::store::Layered`): the store's own files win, the rest come
/// from `default/`. An empty store reads as the default alone.
pub fn load() -> Loaded {
    let store = match dry_tools::store::open_default() {
        Ok(store) => store,
        Err(e) => return Loaded { state: State::default(), store: "none".into(), error: Some(e) },
    };
    let name = store.describe();
    match dry_tools::store::load_or_first(&*store) {
        Ok(state) => Loaded { state, store: name, error: None },
        Err(e) => Loaded { state: State::default(), store: name, error: Some(e) },
    }
}

/// Write the state to the store: only what differs from the default.
pub fn save(state: &State) -> Result<(), String> {
    dry_tools::store::open_default()?.save(state)
}

/// The model settings file, at the repo root.
pub const MODELS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../models.json");

/// Read `models.json`. The file is the only source of model settings:
/// there are no defaults in the code, so a missing file or one that does
/// not parse is an error (the caller shows it and runs with no models).
pub fn load_models() -> Result<Models, String> {
    let text = fs::read_to_string(MODELS_FILE).map_err(|e| format!("cannot read models.json: {e}"))?;
    Models::from_json(&text).map_err(|e| format!("models.json does not parse: {e}"))
}

/// Write `models.json` (temp file + rename, like `save`).
pub fn save_models(models: &Models) -> std::io::Result<()> {
    let tmp = Path::new(MODELS_FILE).with_extension("json.tmp");
    fs::write(&tmp, models.to_json())?;
    fs::rename(&tmp, MODELS_FILE)
}

/// The design tokens come from the active theme, `themes/<name>.md`
/// (`dry_tools` finds it: `themes/active`, else `default`). Missing or
/// broken is an error (there are no token values in the code to fall back on).
pub fn load_theme() -> Result<Theme, String> {
    dry_tools::read_theme()
}

/// This machine's own token choices, `state/local.md`; None without it.
pub fn load_local_theme() -> Result<Option<dry_core::theme::Overrides>, String> {
    dry_tools::read_local_theme()
}

/// Write the active theme's file (temp file + rename).
pub fn save_theme(theme: &Theme) -> Result<(), String> {
    dry_tools::save_theme(theme)
}

/// Every theme's name, and the active one's.
pub fn themes() -> (Vec<String>, String) {
    (dry_tools::list_themes(), dry_tools::active_theme())
}

/// Every scene in `scenes/`, read and checked: name → the scene, or why
/// it does not read. The app's scene setting names one.
pub fn load_scenes() -> std::collections::BTreeMap<String, Result<dry_core::scene::Scene, String>> {
    dry_tools::list_scenes().into_iter().map(|name| {
        let scene = dry_tools::read_scene(&name);
        (name, scene)
    }).collect()
}

/// Make `name` the active theme (writes `themes/active`).
pub fn use_theme(name: &str) -> Result<(), String> {
    dry_tools::set_active_theme(name)
}

/// The newest modification time among the files the app is built from
/// (everything under `themes/`, `scenes/`, `routes/` and `components/`, so also
/// `themes/active`, and `sessions.json`). When it changes, something was edited.
pub fn files_stamp() -> Option<std::time::SystemTime> {
    fn newest(path: &Path, best: &mut Option<std::time::SystemTime>) {
        let Ok(meta) = fs::metadata(path) else { return };
        if let Ok(t) = meta.modified() {
            *best = (*best).max(Some(t));
        }
        if meta.is_dir()
            && let Ok(entries) = fs::read_dir(path)
        {
            for e in entries.flatten() {
                newest(&e.path(), best);
            }
        }
    }
    let mut best = None;
    for p in [dry_tools::themes_dir(), dry_tools::scenes_dir(), dry_tools::routes_dir(), dry_tools::components_dir(), dry_tools::sessions_file()] {
        newest(&p, &mut best);
    }
    best
}

// ---- the workbench (`dry_core::workbench`): the repo's folders, files, commands

/// What is in folder `rel` of the repo: (name, is a folder) pairs. A link
/// to a folder counts as a folder.
pub fn read_dir(rel: &str) -> Result<Vec<(String, bool)>, String> {
    if !dry_core::workbench::safe(rel) {
        return Err(format!("{rel} is outside the repo"));
    }
    let entries = fs::read_dir(dry_tools::root().join(rel)).map_err(|e| e.to_string())?;
    Ok(entries.flatten().map(|e| (e.file_name().to_string_lossy().to_string(), e.path().is_dir())).collect())
}

/// Open file `rel` of the repo for the editor: at most `MAX_OPEN + 1`
/// bytes are read, so a huge file costs nothing.
pub fn read_file(rel: &str) -> dry_core::workbench::OpenFile {
    use dry_core::workbench::{MAX_OPEN, OpenFile};
    use std::io::Read;
    if !dry_core::workbench::safe(rel) || rel.is_empty() {
        return OpenFile::failed(rel, format!("{rel} is outside the repo"));
    }
    let read = || -> std::io::Result<(Vec<u8>, u64)> {
        let file = fs::File::open(dry_tools::root().join(rel))?;
        let size = file.metadata()?.len();
        let mut bytes = Vec::new();
        file.take(MAX_OPEN as u64 + 1).read_to_end(&mut bytes)?;
        Ok((bytes, size))
    };
    match read() {
        Ok((bytes, size)) => OpenFile::new(rel, &bytes, size),
        Err(e) => OpenFile::failed(rel, e.to_string()),
    }
}

/// Run `command` with `sh -c` in the repo folder, with no input, and wait
/// for it: what it printed (output, then errors) and its exit code.
/// Blocking: the app calls it on the background executor, and only when
/// the person pressed Enter in the terminal.
pub fn run(command: &str) -> Result<(String, Option<i32>), String> {
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(dry_tools::root())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("could not start /bin/sh: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((text, out.status.code()))
}
