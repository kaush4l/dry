//! Stores: where the app's `State` (the list of engines) lives.
//!
//! `Store` is the abstract reader and writer. Two implement it, and both
//! read and write the same texts (`dry_core::files`), so they hold the same
//! `State` and one can be copied into the other:
//!
//! - `MdStore`: a folder of markdown files, one folder per engine with its
//!   sharable `spec/`, its `convo/` and its `artifacts/`.
//! - `SqliteStore`: one database file with the same texts in tables.
//!
//! `open("md:state")` or `open("sqlite:state.db")` picks one; the app uses
//! `DRY_STORE`, else `md:state` (paths relative to the repo root).
//!
//! Layers (docs/default.md §1): what `open` gives is the store over the
//! default (`Layered`). The default's files are the base: `default/` read
//! live from disk when it is there (`base`), else the files bundled into
//! the build. The store is the override layer: a file it holds at a path
//! wins, a path it does not hold comes from the default, and a save keeps
//! in it only what differs (`dry_core::store::split`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use dry_core::files::{self, ARTIFACTS, CONVO, MEMORY, SHARED, SPEC, SPEC_FOLDERS};
use dry_core::state::State;
use dry_core::store::{HIDDEN, Shadow, Texts, from_texts, merge, split, to_texts};

use crate::Result;

/// The shared interface (`describe`, `exists`, `load`, `save`) is
/// `dry_core::store::Store`, which the browser's stores implement too;
/// it is re-exported here as `StateStore`, so `use dry_tools::store::
/// StateStore as _` brings its methods in.
pub use dry_core::store::Store as StateStore;

/// A store on this machine's disk: the core's interface, its texts by
/// path, and a folder for the files that are not texts of the state.
pub trait Store: StateStore {
    /// The folder holding one folder per engine for files that are not
    /// texts of the state (a terminal artifact's logs:
    /// `<engine>/artifacts/<name>/<session>.log`). Saving leaves them be.
    fn files_dir(&self) -> PathBuf;
    /// Every text it holds, by path (`dry_core::store`'s paths, and `HIDDEN`).
    fn read_texts(&self) -> Result<Texts>;
    /// Make it hold exactly `texts`. `engines`: the ids of the state's
    /// engines; another engine's folder is removed with its logs.
    fn write_texts(&self, texts: &Texts, engines: &[String]) -> Result;
}

fn engine_ids(state: &State) -> Vec<String> {
    state.engines.iter().map(|e| e.id.clone()).collect()
}

/// A store as it is, with no default under it (a copy's target, tests).
pub fn open_plain(spec: &str) -> Result<Box<dyn Store>> {
    let path = |p: &str| if Path::new(p).is_absolute() { PathBuf::from(p) } else { crate::root().join(p) };
    match spec.split_once(':') {
        Some(("md", p)) => Ok(Box::new(MdStore { root: path(p) })),
        Some(("sqlite", p)) => Ok(Box::new(SqliteStore { path: path(p) })),
        _ => Err(format!("a store is md:<folder> or sqlite:<file>, not \"{spec}\"")),
    }
}

/// `md:<folder>` or `sqlite:<file>` (relative to the repo root), over the
/// default (`Layered`).
pub fn open(spec: &str) -> Result<Box<dyn Store>> {
    Ok(Box::new(Layered { store: open_plain(spec)?, base: None }))
}

/// The app's store spec: `DRY_STORE`, else `md:state`.
pub fn default_spec() -> String {
    std::env::var("DRY_STORE").unwrap_or_else(|_| "md:state".into())
}

/// The app's store: `DRY_STORE`, else `md:state`.
pub fn open_default() -> Result<Box<dyn Store>> {
    open(&default_spec())
}

// ---- the default under the store -----------------------------------------------

/// The default's files (the base layer), and where they came from.
#[derive(Clone, Debug, Default)]
pub struct Base {
    pub texts: Texts,
    /// The `default/` folder read; None: the files bundled into the build.
    pub dir: Option<PathBuf>,
}

impl Base {
    pub fn describe(&self) -> String {
        match &self.dir {
            Some(d) => format!("{} ({} files, live)", d.display(), self.texts.len()),
            None => format!("bundled into this build ({} files)", self.texts.len()),
        }
    }
}

/// The `default/` folder the base is read from, if it is there:
/// `DRY_DEFAULT` (a folder, relative to the repo root; `bundled` for the
/// build's own files), else `<repo root>/default` (`crate::root`: `DRY_ROOT`,
/// else the checkout this was built from). A folder is used only when it
/// holds an `app.md`.
pub fn base_dir() -> Option<PathBuf> {
    let dir = match std::env::var("DRY_DEFAULT") {
        Ok(v) if v == "bundled" => return None,
        Ok(v) if Path::new(&v).is_absolute() => PathBuf::from(v),
        Ok(v) => crate::root().join(v),
        Err(_) => crate::root().join("default"),
    };
    if dir.join("app.md").is_file() {
        Some(dir)
    } else {
        if std::env::var("DRY_DEFAULT").is_ok() {
            eprintln!("DRY_DEFAULT: {} holds no app.md; using the bundled default", dir.display());
        }
        None
    }
}

/// The base layer now: `default/` from disk (`base_dir`), else bundled.
pub fn base() -> Base {
    match base_dir() {
        Some(dir) => {
            let mut texts = Texts::new();
            default_files(&dir, "", &mut texts);
            Base { texts, dir: Some(dir) }
        }
        None => Base { texts: dry_core::shipped::texts(), dir: None },
    }
}

/// Every `.md`/`.json` file under `dir`, as `crates/core/build.rs` bundles them.
fn default_files(dir: &Path, rel: &str, out: &mut Texts) {
    let Ok(entries) = fs::read_dir(dir.join(rel)) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.ends_with(".tmp") {
            continue;
        }
        let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        if e.path().is_dir() {
            default_files(dir, &path, out);
        } else if (path.ends_with(".md") || path.ends_with(".json"))
            && let Ok(text) = fs::read_to_string(e.path())
        {
            out.insert(path, text);
        }
    }
}

/// `models.json`'s default model, for a default engine that names none.
fn default_inference() -> dry_core::inference::OpenAi {
    crate::settings::read_models().ok().and_then(|m| m.default_model().and_then(|d| m.inference(d).ok())).unwrap_or_default()
}

/// A store over the default: reads `merge(base, store)`, saves
/// `split(base, state)`. `describe`, `exists` and `files_dir` are the
/// store's own: `exists` is whether the store holds a state (its `app.md`).
pub struct Layered {
    pub store: Box<dyn Store>,
    /// The base; None reads it at each load and save (`base`), so an edit
    /// of `default/` shows at the next read.
    pub base: Option<Texts>,
}

impl Layered {
    fn base_texts(&self) -> Texts {
        self.base.clone().unwrap_or_else(|| base().texts)
    }
}

impl StateStore for Layered {
    fn describe(&self) -> String {
        self.store.describe()
    }

    fn exists(&self) -> bool {
        self.store.exists()
    }

    /// A default engine whose `engine.md` the store does not hold names no
    /// model: it gets `models.json`'s default, as a fresh state always did.
    fn load(&self) -> Result<State> {
        let base = self.base_texts();
        let own = self.store.read_texts()?;
        let mut state = from_texts(&merge(&base, &own))?;
        let mut ai = None;
        for e in state.engines.iter_mut() {
            let engine_md = format!("engines/{}/{SPEC}/engine.md", e.id);
            if e.session.is_none()
                && e.inference.model.is_empty()
                && e.inference.base_url.is_empty()
                && base.contains_key(&engine_md)
                && !own.contains_key(&engine_md)
            {
                e.inference = ai.get_or_insert_with(default_inference).clone();
            }
        }
        Ok(state)
    }

    fn save(&self, state: &State) -> Result {
        self.write_texts(&to_texts(state), &engine_ids(state))
    }
}

impl Store for Layered {
    fn files_dir(&self) -> PathBuf {
        self.store.files_dir()
    }
    fn read_texts(&self) -> Result<Texts> {
        Ok(merge(&self.base_texts(), &self.store.read_texts()?))
    }
    fn write_texts(&self, texts: &Texts, engines: &[String]) -> Result {
        self.store.write_texts(&split(&self.base_texts(), texts), engines)
    }
}

/// The store's state. A store that holds nothing yet (no `app.md`) reads
/// as the default alone. The old one-file `state.txt` is no longer read:
/// a fresh store starts from `default/`, nothing else.
pub fn load_or_first(store: &dyn Store) -> Result<State> {
    store.load()
}

/// `dry default diff`: which of the default's files the store (`spec`,
/// else the app's) shadows, and how.
pub fn default_diff(spec: Option<&str>) -> Result<String> {
    let spec = spec.map_or_else(default_spec, str::to_string);
    let store = open_plain(&spec)?;
    let base = base();
    let own = store.read_texts()?;
    let shadows = dry_core::store::shadows(&base.texts, &own);
    let mut out = format!("default: {}\nstore:   {}\n", base.describe(), store.describe());
    let count = |k: Shadow| shadows.iter().filter(|(_, s)| *s == k).count();
    for (path, s) in &shadows {
        let what = match s {
            Shadow::Differs => "overrides",
            Shadow::Same => "same",
            Shadow::Hidden => "hides",
        };
        out.push_str(&format!("  {what:<9}  {path}\n"));
    }
    out.push_str(&format!(
        "{} override the default ({} differ, {} the same: the next save drops those), {} hidden; {} file(s) of the store are its own",
        count(Shadow::Differs) + count(Shadow::Same),
        count(Shadow::Differs),
        count(Shadow::Same),
        count(Shadow::Hidden),
        own.keys().filter(|k| !base.texts.contains_key(*k) && !matches!(k.as_str(), HIDDEN | "app.md")).count(),
    ));
    Ok(out)
}

// ---- Reset history -------------------------------------------------------------

/// The machine's own file a reset keeps in a markdown store's folder
/// (`state/local.md`: appearance, hidden routes; not a text of the state).
pub const KEEP_LOCAL: &str = "local.md";

/// What a reset of a store moves, and where to.
#[derive(Clone, Debug, PartialEq)]
pub struct ResetPlan {
    /// The store, as `describe` names it.
    pub store: String,
    /// Where its contents go: the folder (md) or database file (sqlite)
    /// beside it, named `<name>.reset-<UTC stamp>` (`dry_core::store::reset_backup_name`).
    pub backup: PathBuf,
    /// What is there now and moves: (from, to).
    pub moves: Vec<(PathBuf, PathBuf)>,
    /// What stays (copied back into the empty store): `local.md`.
    pub keeps: Vec<PathBuf>,
    /// The engines and messages the app shows now, for the confirm line.
    pub engines: usize,
    pub messages: usize,
}

impl ResetPlan {
    /// Does the store hold anything to move?
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// For people: what goes, where, and what stays.
    pub fn describe(&self) -> String {
        if self.is_empty() {
            return format!("{} holds nothing of its own: it is the default already; nothing to reset", self.store);
        }
        let mut out = format!(
            "reset {}: {} engine(s) and {} message(s) now; all of it and its history move to {}\n",
            self.store,
            self.engines,
            self.messages,
            self.backup.display()
        );
        for (from, to) in &self.moves {
            out.push_str(&format!("  move  {} -> {}\n", from.display(), to.display()));
        }
        for k in &self.keeps {
            out.push_str(&format!("  keep  {}\n", k.display()));
        }
        out.push_str(&format!("moves {}; keeps {}. The next load is default/ alone.", dry_core::store::RESET_REMOVES, dry_core::store::RESET_KEEPS));
        out
    }
}

/// A store spec's kind and path (absolute; relative to the repo root).
fn store_path(spec: &str) -> Result<(&str, PathBuf)> {
    let path = |p: &str| if Path::new(p).is_absolute() { PathBuf::from(p) } else { crate::root().join(p) };
    match spec.split_once(':') {
        Some((kind @ ("md" | "sqlite"), p)) => Ok((kind, path(p.trim_end_matches('/')))),
        _ => Err(format!("a store is md:<folder> or sqlite:<file>, not \"{spec}\"")),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `path` with its file name replaced by `name`.
fn beside(path: &Path, name: &str) -> PathBuf {
    path.with_file_name(name)
}

/// What resetting store `spec` would move, at time `unix` (nothing is changed).
pub fn reset_plan_at(spec: &str, unix: i64) -> Result<ResetPlan> {
    let (kind, path) = store_path(spec)?;
    let plain = open_plain(spec)?;
    // What the app shows now (the store over the default), for the confirm line.
    let now = Layered { store: open_plain(spec)?, base: None }.load().unwrap_or_default();
    let (engines, messages) = (now.engines.len(), now.engines.iter().map(|e| e.messages.len()).sum());
    let name = path.file_name().map_or_else(|| "state".into(), |n| n.to_string_lossy().to_string());
    // A name not taken yet (two resets in one second get `-2`, `-3`, …).
    let mut n = 1;
    let backup = loop {
        let mut b = dry_core::store::reset_backup_name(&name, unix);
        if n > 1 {
            b = match kind {
                "sqlite" => match b.rsplit_once('.') {
                    Some((stem, ext)) => format!("{stem}-{n}.{ext}"),
                    None => format!("{b}-{n}"),
                },
                _ => format!("{b}-{n}"),
            };
        }
        let b = beside(&path, &b);
        if !b.exists() && !b.with_extension("files").exists() {
            break b;
        }
        n += 1;
    };
    let mut moves = Vec::new();
    let mut keeps = Vec::new();
    match kind {
        "md" => {
            // Anything but this machine's local.md (and a Finder's .DS_Store) is the store's.
            let holds = fs::read_dir(&path)
                .map(|it| it.flatten().any(|e| !matches!(e.file_name().to_str(), Some(KEEP_LOCAL | ".DS_Store"))))
                .unwrap_or(false);
            if holds {
                moves.push((path.clone(), backup.clone()));
                if path.join(KEEP_LOCAL).is_file() {
                    keeps.push(path.join(KEEP_LOCAL));
                }
            }
        }
        _ => {
            let file_name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let (db, bk) = (file_name(&path), file_name(&backup));
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let from = beside(&path, &format!("{db}{suffix}"));
                if from.exists() {
                    moves.push((from, beside(&backup, &format!("{bk}{suffix}"))));
                }
            }
            let files = path.with_extension("files");
            if files.exists() {
                moves.push((files, backup.with_extension("files")));
            }
        }
    }
    Ok(ResetPlan { store: plain.describe(), backup, moves, keeps, engines, messages })
}

/// What resetting store `spec` would move now.
pub fn reset_plan(spec: &str) -> Result<ResetPlan> {
    reset_plan_at(spec, unix_now())
}

/// Reset history (docs/default.md, "Reset"): MOVE everything store `spec`
/// holds to a backup beside it and leave it empty, so the next load is the
/// default alone. Nothing is deleted: a markdown store's folder is renamed
/// (`state` → `state.reset-<stamp>/`) and a new one made holding only this
/// machine's `local.md` (copied back); a SQLite store's file (and its
/// `-wal`/`-shm` and `.files/` folder of logs) is renamed the same way.
/// The plan carried out; `is_empty` when there was nothing to move.
///
/// The caller makes sure no one writes the store meanwhile (the host
/// does it under its lock, with no turn running).
pub fn reset(spec: &str) -> Result<ResetPlan> {
    let plan = reset_plan(spec)?;
    let (kind, path) = store_path(spec)?;
    for (from, to) in &plan.moves {
        fs::rename(from, to).map_err(|e| format!("could not move {} to {}: {e} (nothing was removed)", from.display(), to.display()))?;
    }
    if kind == "md" && !plan.is_empty() {
        fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for keep in &plan.keeps {
            let from = plan.backup.join(keep.file_name().unwrap_or_default());
            fs::copy(&from, keep).map_err(|e| format!("could not keep {}: {e} (it is in {})", keep.display(), from.display()))?;
        }
    }
    Ok(plan)
}

// ---- markdown folders ------------------------------------------------------------

/// ```text
/// <root>/app.md                 mode and the engines, in order (a child: `- helper (child of engine-1)`)
/// <root>/engines/<id>/spec/     sharable: engine.md, goal.md, ...
/// <root>/engines/<id>/convo/    messages.md, meta.json, run.md
/// <root>/engines/<id>/memory/   learning.md, learning.log (local)
/// <root>/engines/<id>/artifacts/<name>.md
/// <root>/engines/<id>/shared.md the thread of a root (top-level engine) and its tree
/// ```
pub struct MdStore {
    pub root: PathBuf,
}

/// Every `*.md` (and, unless `only_md`, `*.json`) file in `dir`, by file name.
fn read_dir_texts(dir: &Path, only_md: bool) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else { return Ok(out) };
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !path.is_file() || (only_md && ext != "md") || !(ext == "md" || ext == "json") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        out.insert(name, crate::read(&path)?);
    }
    Ok(out)
}

impl MdStore {
    fn engine_dir(&self, id: &str) -> PathBuf {
        self.root.join("engines").join(id)
    }
}

impl Store for MdStore {
    /// The engines' own folders: the logs sit beside their artifacts.
    fn files_dir(&self) -> PathBuf {
        self.root.join("engines")
    }

    /// `app.md`, `shared.md`, `hidden.md`, and each engine folder's texts
    /// (`engines/<id>/…`, every folder there, listed in `app.md` or not).
    fn read_texts(&self) -> Result<Texts> {
        let mut out = Texts::new();
        for name in ["app.md", SHARED, HIDDEN] {
            let p = self.root.join(name);
            if p.is_file() {
                out.insert(name.to_string(), crate::read(&p)?);
            }
        }
        let Ok(entries) = fs::read_dir(self.root.join("engines")) else { return Ok(out) };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            let dir = entry.path();
            let at = |rel: &str| format!("engines/{id}/{rel}");
            for (name, text) in read_spec(&dir.join(SPEC))? {
                out.insert(at(&format!("{SPEC}/{name}")), text);
            }
            for (folder, file) in
                [(CONVO, "messages.md"), (CONVO, "meta.json"), (CONVO, "run.md"), (MEMORY, "learning.md"), (MEMORY, "learning.log")]
            {
                let p = dir.join(folder).join(file);
                if p.is_file() {
                    out.insert(at(&format!("{folder}/{file}")), crate::read(&p)?);
                }
            }
            for (name, text) in read_dir_texts(&dir.join(ARTIFACTS), true)? {
                out.insert(at(&format!("{ARTIFACTS}/{name}")), text);
            }
            if dir.join(SHARED).is_file() {
                out.insert(at(SHARED), crate::read(&dir.join(SHARED))?);
            }
        }
        Ok(out)
    }

    /// Write what changed, remove the texts no longer there (and folders
    /// that leaves empty). An engine folder that held texts and is not in
    /// `engines` is removed whole.
    fn write_texts(&self, texts: &Texts, engines: &[String]) -> Result {
        let now = self.read_texts()?;
        for (path, text) in texts {
            if now.get(path) != Some(text) {
                crate::write(&self.root.join(path), text)?;
            }
        }
        for path in now.keys().filter(|p| !texts.contains_key(*p)) {
            let file = self.root.join(path);
            fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            // Folders it leaves empty go too (a skill's, a template's), up to the store's.
            let mut dir = file.parent();
            while let Some(d) = dir.filter(|d| d.starts_with(&self.root) && *d != self.root && *d != self.root.join("engines")) {
                if fs::remove_dir(d).is_err() {
                    break;
                }
                dir = d.parent();
            }
        }
        for id in now.keys().filter_map(|p| p.strip_prefix("engines/")?.split_once('/').map(|(id, _)| id)) {
            let dir = self.engine_dir(id);
            if !engines.iter().any(|e| e == id) && dir.is_dir() {
                fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
        }
        Ok(())
    }
}

impl StateStore for MdStore {
    fn describe(&self) -> String {
        format!("md:{}", self.root.display())
    }

    fn exists(&self) -> bool {
        self.root.join("app.md").exists()
    }

    fn load(&self) -> Result<State> {
        if !self.exists() {
            return Err(format!("{}: no app.md", self.root.display()));
        }
        from_texts(&self.read_texts()?)
    }

    /// Make the folder hold exactly the state's texts: a spec or artifact
    /// file no longer part of an engine is removed; an engine no longer in
    /// the state has its folder removed.
    fn save(&self, state: &State) -> Result {
        self.write_texts(&to_texts(state), &engine_ids(state))
    }
}

/// `spec/`: its own files, and each `SPEC_FOLDERS` file as `<folder>/<name>.md`.
fn read_spec(dir: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = read_dir_texts(dir, false)?;
    for folder in SPEC_FOLDERS {
        for (name, text) in read_dir_texts(&dir.join(folder), true)? {
            out.insert(format!("{folder}/{name}"), text);
        }
    }
    // Skills: one folder each, holding its SKILL.md (and maybe more files,
    // which are left as they are).
    if let Ok(entries) = fs::read_dir(dir.join("skills")) {
        for entry in entries.flatten() {
            let file = entry.path().join("SKILL.md");
            if file.is_file() {
                out.insert(files::skill_file(&entry.file_name().to_string_lossy()), crate::read(&file)?);
            }
            // The tools it brings of its own (`dry_core::skilltool`).
            let tools = entry.path().join(dry_core::skilltool::FILE);
            if tools.is_file() {
                out.insert(dry_core::skilltool::file(&entry.file_name().to_string_lossy()), crate::read(&tools)?);
            }
        }
    }
    // Templates (`dry_core::kit`): every `.md` file under `templates/`, at any depth.
    let mut found = Vec::new();
    template_files(&dir.join("templates"), "", &mut found);
    for rel in found {
        out.insert(format!("{}{rel}", dry_core::kit::FOLDER), crate::read(&dir.join("templates").join(&rel))?);
    }
    Ok(out)
}

/// Every `.md` file under `dir`, as `/`-separated paths below it.
fn template_files(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if entry.path().is_dir() {
            template_files(&entry.path(), &path, out);
        } else if name.ends_with(".md") {
            out.push(path);
        }
    }
}

// ---- sqlite --------------------------------------------------------------------------

/// The same texts as the folders, in tables:
///
/// ```text
/// app(key, value)                      mode, and the engine order
/// spec(engine, file, text)             one row per spec file
/// convo(engine, messages, meta)
/// run(engine, text)
/// artifacts(engine, name, text)
/// memory(engine, file, text)            learning.md, learning.log
/// shared(level, text)                   a root's thread ("" a store from before threads)
/// ```
pub struct SqliteStore {
    pub path: PathBuf,
}

impl SqliteStore {
    fn connect(&self) -> Result<rusqlite::Connection> {
        let db = rusqlite::Connection::open(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS app(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS spec(engine TEXT, file TEXT, text TEXT NOT NULL, PRIMARY KEY(engine, file));
             CREATE TABLE IF NOT EXISTS convo(engine TEXT PRIMARY KEY, messages TEXT NOT NULL, meta TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS artifacts(engine TEXT, name TEXT, text TEXT NOT NULL, PRIMARY KEY(engine, name));
             CREATE TABLE IF NOT EXISTS run(engine TEXT PRIMARY KEY, text TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS memory(engine TEXT, file TEXT, text TEXT NOT NULL, PRIMARY KEY(engine, file));
             CREATE TABLE IF NOT EXISTS shared(level TEXT PRIMARY KEY, text TEXT NOT NULL);",
        )
        .map_err(sql)?;
        Ok(db)
    }
}

fn sql(e: rusqlite::Error) -> String {
    format!("sqlite: {e}")
}

impl Store for SqliteStore {
    /// `state.db` → `state.files/engines`, beside the database.
    fn files_dir(&self) -> PathBuf {
        self.path.with_extension("files").join("engines")
    }

    /// The tables as the folder's paths (`HIDDEN` is the `app` row `hidden`).
    fn read_texts(&self) -> Result<Texts> {
        let db = self.connect()?;
        let mut out = Texts::new();
        let mut rows = |q: &str, path: &dyn Fn(&str, &str) -> String| -> Result {
            let mut stmt = db.prepare(q).map_err(sql)?;
            let found = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).map_err(sql)?;
            for row in found {
                let (a, b, text) = row.map_err(sql)?;
                out.insert(path(&a, &b), text);
            }
            Ok(())
        };
        rows("SELECT key, '', value FROM app WHERE key IN ('app', 'hidden')", &|k, _| {
            if k == "app" { "app.md".into() } else { HIDDEN.into() }
        })?;
        rows("SELECT level, '', text FROM shared", &|level, _| if level.is_empty() { SHARED.into() } else { format!("engines/{level}/{SHARED}") })?;
        rows("SELECT engine, file, text FROM spec", &|id, file| format!("engines/{id}/{SPEC}/{file}"))?;
        rows("SELECT engine, 'messages.md', messages FROM convo", &|id, f| format!("engines/{id}/{CONVO}/{f}"))?;
        rows("SELECT engine, 'meta.json', meta FROM convo", &|id, f| format!("engines/{id}/{CONVO}/{f}"))?;
        rows("SELECT engine, 'run.md', text FROM run", &|id, f| format!("engines/{id}/{CONVO}/{f}"))?;
        rows("SELECT engine, name, text FROM artifacts", &|id, name| format!("engines/{id}/{ARTIFACTS}/{name}.md"))?;
        rows("SELECT engine, file, text FROM memory", &|id, file| format!("engines/{id}/{MEMORY}/{file}"))?;
        Ok(out)
    }

    /// Replace everything in one transaction.
    fn write_texts(&self, texts: &Texts, _engines: &[String]) -> Result {
        let mut db = self.connect()?;
        let tx = db.transaction().map_err(sql)?;
        tx.execute_batch("DELETE FROM app; DELETE FROM spec; DELETE FROM convo; DELETE FROM artifacts; DELETE FROM run; DELETE FROM memory; DELETE FROM shared;")
            .map_err(sql)?;
        let mut convo: BTreeMap<&str, (String, String)> = BTreeMap::new();
        for (path, text) in texts {
            let run = |q: &str, args: [&str; 3]| -> Result {
                let n = q.matches('?').count();
                tx.execute(q, rusqlite::params_from_iter(&args[..n])).map(|_| ()).map_err(sql)
            };
            if path == "app.md" {
                run("INSERT INTO app(key, value) VALUES ('app', ?1)", [text, "", ""])?;
            } else if path == HIDDEN {
                run("INSERT INTO app(key, value) VALUES ('hidden', ?1)", [text, "", ""])?;
            } else if path == SHARED {
                run("INSERT INTO shared VALUES ('', ?1)", [text, "", ""])?;
            } else if let Some((id, rest)) = path.strip_prefix("engines/").and_then(|p| p.split_once('/')) {
                if rest == SHARED {
                    run("INSERT INTO shared VALUES (?1, ?2)", [id, text, ""])?;
                } else if let Some(file) = rest.strip_prefix(&format!("{SPEC}/")) {
                    run("INSERT INTO spec VALUES (?1, ?2, ?3)", [id, file, text])?;
                } else if rest == format!("{CONVO}/messages.md") {
                    convo.entry(id).or_default().0 = text.clone();
                } else if rest == format!("{CONVO}/meta.json") {
                    convo.entry(id).or_default().1 = text.clone();
                } else if rest == format!("{CONVO}/run.md") {
                    run("INSERT INTO run VALUES (?1, ?2)", [id, text, ""])?;
                } else if let Some(name) = rest.strip_prefix(&format!("{ARTIFACTS}/")).and_then(|n| n.strip_suffix(".md")) {
                    run("INSERT INTO artifacts VALUES (?1, ?2, ?3)", [id, name, text])?;
                } else if let Some(file) = rest.strip_prefix(&format!("{MEMORY}/")) {
                    run("INSERT INTO memory VALUES (?1, ?2, ?3)", [id, file, text])?;
                }
            }
        }
        for (id, (messages, meta)) in &convo {
            tx.execute("INSERT INTO convo VALUES (?1, ?2, ?3)", [id, messages.as_str(), meta.as_str()]).map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }
}

impl StateStore for SqliteStore {
    fn describe(&self) -> String {
        format!("sqlite:{}", self.path.display())
    }

    fn exists(&self) -> bool {
        self.path.exists()
            && self.connect().ok().and_then(|db| db.query_row("SELECT 1 FROM app WHERE key='app'", [], |_| Ok(())).ok()).is_some()
    }

    fn load(&self) -> Result<State> {
        if !self.exists() {
            return Err(format!("{}: no app", self.path.display()));
        }
        from_texts(&self.read_texts()?)
    }

    fn save(&self, state: &State) -> Result {
        self.write_texts(&to_texts(state), &engine_ids(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dry-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn both_stores_hold_the_same_state() {
        let state = files::sample();
        let dir = scratch("both");
        let md = MdStore { root: dir.join("state") };
        let db = SqliteStore { path: dir.join("state.db") };
        assert!(!md.exists() && !db.exists());
        md.save(&state).unwrap();
        db.save(&state).unwrap();
        assert_eq!(md.load().unwrap(), state);
        assert_eq!(db.load().unwrap(), state);
        // Copy one into the other: still the same.
        let copy = MdStore { root: dir.join("copy") };
        copy.save(&db.load().unwrap()).unwrap();
        assert_eq!(copy.load().unwrap(), state);
        // The spec is its own folder, apart from what happened.
        let e = &state.engines[0];
        assert!(dir.join("state/engines").join(&e.id).join("spec/mcp.json").exists());
        assert!(dir.join("state/engines").join(&e.id).join("convo/messages.md").exists());
        assert!(dir.join("state/engines").join(&e.id).join("artifacts/plan.md").exists());
        assert!(dir.join("state/engines").join(&e.id).join("spec/scripts/note.md").exists());
        assert!(dir.join("state/engines").join(&e.id).join("spec/alerts/wake.md").exists());
        assert!(dir.join("state/engines").join(&e.id).join("spec/goal.md").exists());
        assert!(dir.join("state/engines").join(&e.id).join("memory/learning.md").exists());
        assert!(dir.join("state/engines").join(&e.id).join("memory/learning.log").exists());
        assert!(dir.join("state/engines/engine-1/shared.md").exists() && !dir.join("state/shared.md").exists(), "each root's thread");
        assert!(crate::read(&dir.join("state/app.md")).unwrap().contains("- helper (child of engine-1)"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Every file under `dir`, by path relative to it.
    fn files_under(dir: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(d) = todo.pop() {
            for entry in fs::read_dir(&d).unwrap().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    todo.push(p);
                } else {
                    out.insert(p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"), crate::read(&p).unwrap());
                }
            }
        }
        out
    }

    #[test]
    fn the_md_folder_holds_exactly_the_cores_texts() {
        // The browser's stores keep `dry_core::store::to_texts`; a folder
        // made from them is the same folder this store writes.
        let state = files::sample();
        let dir = scratch("texts");
        MdStore { root: dir.clone() }.save(&state).unwrap();
        assert_eq!(files_under(&dir), dry_core::store::to_texts(&state));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_deleted_engine_or_artifact_leaves_no_files() {
        let mut state = files::sample();
        let dir = scratch("delete");
        let md = MdStore { root: dir.clone() };
        md.save(&state).unwrap();
        let gone = state.engines.remove(1).id;
        state.shared.remove(&gone);
        state.engines[0].artifacts.clear();
        state.engines[0].scripts.clear();
        md.save(&state).unwrap();
        assert!(!dir.join("engines").join(&state.engines[0].id).join("spec/scripts/note.md").exists());
        assert!(!dir.join("engines").join(gone).exists());
        assert!(!dir.join("engines").join(&state.engines[0].id).join("artifacts/plan.md").exists());
        assert_eq!(md.load().unwrap(), state);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_over_the_default_holds_only_what_differs_and_reads_the_default_live() {
        let dir = scratch("layered");
        let base = dry_core::shipped::texts();
        let fresh = from_texts(&base).unwrap();
        for plain in [Box::new(MdStore { root: dir.join("md") }) as Box<dyn Store>, Box::new(SqliteStore { path: dir.join("s.db") })] {
            let layered = Layered { store: plain, base: Some(base.clone()) };
            assert!(!layered.exists());
            let first = layered.load().unwrap();
            assert_eq!(first.engines[0].id, "translator", "nothing stored reads as the default");
            assert_eq!(first.engines[0].inference, default_inference(), "on models.json's default model");
            // An edited skill: the store holds it and the convo, not the default's other files.
            let mut s = fresh.clone();
            s.engines[0].inference.model = "m".into(); // its engine.md is the store's (no models.json default goes in)
            s.engines[0].skills.insert("judging".into(), base["engines/translator/spec/skills/judging/SKILL.md"].replace("# Judging", "# Judging, mine"));
            layered.save(&s).unwrap();
            assert!(layered.exists());
            let own = layered.store.read_texts().unwrap();
            assert!(own.contains_key("engines/translator/spec/skills/judging/SKILL.md"), "{:?}", own.keys());
            assert!(!own.contains_key("engines/translator/spec/goal.md") && !own.contains_key("engines/translator/spec/skills/prompting/SKILL.md"));
            assert_eq!(layered.load().unwrap(), s);
            // An edit of the default shows through; the store's own file still wins.
            let mut newer = base.clone();
            for f in ["engines/translator/spec/goal.md", "engines/translator/spec/skills/judging/SKILL.md"] {
                newer.insert(f.into(), base[f].replacen('\n', "\n\nnewer default line.\n", 1));
            }
            let later = Layered { store: layered.store, base: Some(newer) };
            let read = later.load().unwrap();
            assert!(read.engines[0].description.contains("newer default line"));
            assert!(read.engines[0].skills["judging"].contains("# Judging, mine") && !read.engines[0].skills["judging"].contains("newer default"));
            // Removing a default file hides it, and it stays hidden.
            let mut less = read.clone();
            less.engines[0].skills.remove("prompting");
            later.save(&less).unwrap();
            assert!(!later.load().unwrap().engines[0].skills.contains_key("prompting"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_copy_store_keeps_winning_and_its_next_save_drops_the_copies() {
        let dir = scratch("fullcopy");
        let base = dry_core::shipped::texts();
        let mut s = from_texts(&base).unwrap();
        s.engines[0].inference.model = "mine".into();
        let md = MdStore { root: dir.clone() };
        md.save(&s).unwrap(); // the old way: every file
        assert!(dir.join("engines/translator/spec/goal.md").exists());
        let layered = Layered { store: Box::new(MdStore { root: dir.clone() }), base: Some(base.clone()) };
        assert_eq!(layered.load().unwrap(), s, "it reads as it did");
        let shadows = dry_core::store::shadows(&base, &md.read_texts().unwrap());
        assert!(shadows.contains(&("engines/translator/spec/engine.md".into(), Shadow::Differs)), "the model is its own");
        assert!(shadows.contains(&("engines/translator/spec/goal.md".into(), Shadow::Same)));
        layered.save(&s).unwrap();
        assert!(!dir.join("engines/translator/spec/goal.md").exists() && !dir.join("engines/translator/spec/templates").exists());
        assert!(dir.join("engines/translator/spec/engine.md").exists());
        assert_eq!(layered.load().unwrap(), s);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reset_moves_the_store_aside_keeps_local_md_and_loads_the_default_alone() {
        let dir = scratch("reset-md");
        let root = dir.join("state");
        let base = dry_core::shipped::texts();
        let spec = format!("md:{}", root.display());
        let layered = Layered { store: open_plain(&spec).unwrap(), base: Some(base.clone()) };
        let mut s = files::sample();
        s.engines[0].messages.truncate(2);
        layered.save(&s).unwrap();
        crate::spans::append(&root.join("engines"), &[dry_core::spans::Span::new(dry_core::spans::Kind::Turn, "t1", &s.engines[0].id, 1, 2)]).unwrap();
        fs::write(root.join(KEEP_LOCAL), "## Scene\n").unwrap();
        let plan = reset_plan(&spec).unwrap();
        assert!(!plan.is_empty() && plan.engines == s.engines.len() && plan.messages > 0, "{plan:?}");
        assert!(plan.describe().contains(&plan.backup.display().to_string()));
        let done = reset(&spec).unwrap();
        // Moved, not removed: the backup is the old folder, logs and all.
        let backup = &done.backup;
        assert!(backup.file_name().unwrap().to_string_lossy().starts_with("state.reset-"), "{}", backup.display());
        assert!(backup.join("app.md").is_file() && backup.join("engines").join(&s.engines[0].id).join(dry_core::spans::FILE).is_file());
        // The store holds only this machine's local.md, and reads as the default.
        let left: Vec<String> = fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left, vec![KEEP_LOCAL.to_string()]);
        assert_eq!(fs::read_to_string(root.join(KEEP_LOCAL)).unwrap(), "## Scene\n");
        let fresh = Layered { store: open_plain(&spec).unwrap(), base: Some(base) }.load().unwrap();
        assert_eq!(fresh.engines.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["translator"]);
        assert!(fresh.engines[0].messages.is_empty());
        assert_eq!(fresh.engines[0].inference, default_inference(), "models.json's default model");
        // Again: nothing of its own to move.
        assert!(reset(&spec).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sqlite_reset_renames_the_file_and_its_logs() {
        let dir = scratch("reset-db");
        let spec = format!("sqlite:{}", dir.join("state.db").display());
        let layered = Layered { store: open_plain(&spec).unwrap(), base: Some(dry_core::shipped::texts()) };
        layered.save(&files::sample()).unwrap();
        let logs = layered.files_dir();
        fs::create_dir_all(logs.join("x")).unwrap();
        fs::write(logs.join("x/spans.jsonl"), "{}\n").unwrap();
        let done = reset(&spec).unwrap();
        assert!(done.backup.is_file() && done.backup.to_string_lossy().ends_with(".db"), "{}", done.backup.display());
        assert!(done.backup.with_extension("files").join("engines/x/spans.jsonl").is_file());
        assert!(!dir.join("state.db").exists() && !dir.join("state.files").exists());
        let fresh = open(&spec).unwrap().load().unwrap();
        assert_eq!(fresh.engines.len(), 1);
        assert_eq!(fresh.engines[0].id, "translator");
        let _ = fs::remove_dir_all(&dir);
    }
}

