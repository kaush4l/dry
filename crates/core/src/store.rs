//! Stores: where the app's `State` lives, as an interface every shell can
//! implement. Pure: nothing here reads a disk or a database.
//!
//! The state is a set of texts by path (`Texts`), the same files a
//! markdown folder holds (`to_texts`, `from_texts`, made from
//! `crate::files`), so every store keeps the same `State` and one can be
//! copied into another:
//!
//! ```text
//! app.md                               mode and the engines, in order
//! engines/<id>/spec/…                  engine.md, goal.md, scripts/…, skills/<n>/SKILL.md
//! engines/<id>/convo/messages.md       meta.json, run.md
//! engines/<id>/memory/learning.md      learning.log
//! engines/<id>/artifacts/<name>.md
//! engines/<root>/shared.md             the thread of a root and its tree
//! ```
//!
//! Two traits, by how the place is reached:
//!
//! - `Store`: whole states, at once (blocking). The native stores
//!   (`dry_tools::store`: a markdown folder, SQLite) and `Memory`.
//! - `TextStore`: texts by path, async, saved as changes (`Changes`). The
//!   browser's stores (IndexedDB, localStorage) and `Memory`. A remote
//!   host (texts over HTTP) or OPFS is one more implementation of it.
//!
//! Paths outside the state's own (`local/…`: a browser's settings, the
//! machine's overrides) travel in the same store and are left alone by
//! `to_texts`/`from_texts`.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::files::{self, ARTIFACTS, CONVO, EngineFiles, MEMORY, SHARED, SPEC};
use crate::state::State;

/// Texts by path (`/`-separated, relative).
pub type Texts = BTreeMap<String, String>;

/// Where texts of this machine go that are not part of the state.
pub const LOCAL: &str = "local/";

/// An append-only log kept beside an engine's texts (`crate::spans`):
/// not a text of the state, so a state save never removes it.
pub fn is_log(path: &str) -> bool {
    path.ends_with(".jsonl")
}

/// Reads and writes the whole app state, at once.
pub trait Store {
    /// Where it is, for people (`md:state`, `sqlite:state.db`, `memory`).
    fn describe(&self) -> String;
    /// Is there anything stored yet?
    fn exists(&self) -> bool;
    fn load(&self) -> Result<State, String>;
    fn save(&self, state: &State) -> Result<(), String>;
}

/// What changed between two sets of texts: write these, remove those.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub put: Texts,
    pub remove: Vec<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.remove.is_empty()
    }

    /// From `old` to `new`. Only `new`'s own kind of paths are compared:
    /// `local/…` is never removed by a state save.
    pub fn between(old: &Texts, new: &Texts) -> Changes {
        let put = new.iter().filter(|(k, v)| old.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let remove = old.keys().filter(|k| !k.starts_with(LOCAL) && !is_log(k) && !new.contains_key(*k)).cloned().collect();
        Changes { put, remove }
    }

    /// `texts` with these changes made.
    pub fn apply_to(&self, texts: &mut Texts) {
        for k in &self.remove {
            texts.remove(k);
        }
        texts.extend(self.put.clone());
    }
}

/// Texts by path, reached asynchronously (a browser database, a host over
/// the network). Single-threaded: futures need not be `Send`.
#[allow(async_fn_in_trait)]
pub trait TextStore {
    fn describe(&self) -> String;
    /// Every text it holds (empty: nothing stored yet).
    async fn read_all(&self) -> Result<Texts, String>;
    async fn write(&self, changes: &Changes) -> Result<(), String>;
}

/// A `TextStore`'s state with what was last saved, so each save writes
/// only what changed.
#[derive(Clone, Debug, Default)]
pub struct Saver {
    pub last: Texts,
}

impl Saver {
    /// What to write so the store holds `state` (and `local`'s texts).
    pub fn changes(&self, state: &State, local: &Texts) -> Changes {
        let mut new = to_texts(state);
        new.extend(local.iter().filter(|(k, _)| k.starts_with(LOCAL)).map(|(k, v)| (k.clone(), v.clone())));
        let mut c = Changes::between(&self.last, &new);
        // A local text no longer given is removed too.
        c.remove.extend(self.last.keys().filter(|k| k.starts_with(LOCAL) && !new.contains_key(*k)).cloned());
        c
    }

    /// After `changes` were written.
    pub fn saved(&mut self, changes: &Changes) {
        changes.apply_to(&mut self.last);
    }
}

/// Load from a `TextStore`: the state (`None`: nothing stored yet), and
/// a `Saver` that knows what is there.
pub async fn load<S: TextStore>(store: &S) -> Result<(Option<State>, Saver), String> {
    let texts = store.read_all().await?;
    let state = if texts.contains_key("app.md") { Some(from_texts(&texts)?) } else { None };
    Ok((state, Saver { last: texts }))
}

/// Save `state` (and `local`'s `local/…` texts) to a `TextStore`, only what changed.
pub async fn save<S: TextStore>(store: &S, saver: &mut Saver, state: &State, local: &Texts) -> Result<usize, String> {
    let c = saver.changes(state, local);
    if c.is_empty() {
        return Ok(0);
    }
    store.write(&c).await?;
    saver.saved(&c);
    Ok(c.put.len() + c.remove.len())
}

fn engine_dir(id: &str) -> String {
    format!("engines/{id}")
}

fn shared_path(level: &str) -> String {
    if level.is_empty() { SHARED.to_string() } else { format!("{}/{SHARED}", engine_dir(level)) }
}

/// The state as the texts a markdown folder holds (the same paths and
/// the same rule for empty files as `dry_tools::store::MdStore`).
pub fn to_texts(state: &State) -> Texts {
    let mut out = Texts::new();
    for e in &state.engines {
        let dir = engine_dir(&e.id);
        let f = files::to_files(e);
        for (name, text) in &f.spec {
            out.insert(format!("{dir}/{SPEC}/{name}"), text.clone());
        }
        out.insert(format!("{dir}/{CONVO}/messages.md"), f.messages);
        out.insert(format!("{dir}/{CONVO}/meta.json"), f.meta);
        if !f.run.is_empty() {
            out.insert(format!("{dir}/{CONVO}/run.md"), f.run);
        }
        for (name, text) in &f.artifacts {
            out.insert(format!("{dir}/{ARTIFACTS}/{name}.md"), text.clone());
        }
        for (file, text) in [("learning.md", f.learning), ("learning.log", f.learning_log)] {
            if !text.trim().is_empty() {
                out.insert(format!("{dir}/{MEMORY}/{file}"), text);
            }
        }
    }
    for level in std::iter::once("").chain(state.engines.iter().map(|e| e.id.as_str())) {
        if let Some(text) = state.shared.get(level).filter(|t| !t.trim().is_empty()) {
            out.insert(shared_path(level), text.clone());
        }
    }
    out.insert("app.md".into(), files::app_md(state));
    out
}

/// The state back from `to_texts`' texts. Paths it does not know are left.
pub fn from_texts(texts: &Texts) -> Result<State, String> {
    let app = texts.get("app.md").map_or("", String::as_str);
    let (mode, ids) = files::parse_app(app);
    let (scene, motion) = files::parse_settings(app);
    let mut state = State { mode, scene, motion, ..State::default() };
    for level in std::iter::once(String::new()).chain(ids.iter().map(|(id, _)| id.clone())) {
        if let Some(text) = texts.get(&shared_path(&level)) {
            state.shared.insert(level, text.clone());
        }
    }
    for (id, parent) in ids {
        let dir = engine_dir(&id);
        let under = |folder: &str| -> BTreeMap<String, String> {
            let prefix = format!("{dir}/{folder}/");
            texts
                .range(prefix.clone()..)
                .take_while(|(k, _)| k.starts_with(&prefix))
                .map(|(k, v)| (k[prefix.len()..].to_string(), v.clone()))
                .collect()
        };
        let text = |path: String| texts.get(&path).cloned().unwrap_or_default();
        let f = EngineFiles {
            spec: under(SPEC),
            messages: text(format!("{dir}/{CONVO}/messages.md")),
            meta: text(format!("{dir}/{CONVO}/meta.json")),
            run: text(format!("{dir}/{CONVO}/run.md")),
            artifacts: under(ARTIFACTS)
                .into_iter()
                .filter(|(n, _)| !n.contains('/') && n.ends_with(".md"))
                .map(|(n, t)| (n.trim_end_matches(".md").to_string(), t))
                .collect(),
            learning: text(format!("{dir}/{MEMORY}/learning.md")),
            learning_log: text(format!("{dir}/{MEMORY}/learning.log")),
        };
        let mut e = files::from_files(&id, &f)?;
        e.parent = parent;
        state.engines.push(e);
    }
    // Files from before threads (the top's, a deeper engine's) move into
    // their roots' threads; the next save removes them.
    crate::shared::fold(&mut state);
    // An engine with no cursor (a store from before the guided rule or
    // before threads) has heard what its thread holds, kept from now on.
    for i in 0..state.engines.len() {
        if state.engines[i].heard.is_none() {
            state.engines[i].heard = Some(crate::snapshot::inbox(&state, i, crate::snapshot::Cursor::default()).1);
        }
    }
    Ok(state)
}

// ---- layers: the default under the store (docs/default.md §1) --------------------
//
// A store is an override layer over a base (the shipped default's files,
// laid out as a store is). Nearest wins: a text the store holds at a path
// is the one read; a path it does not hold comes from the base. A store
// hides a base file it no longer has by naming it in `HIDDEN`. A save
// keeps in the store only what differs from the base (`split`), so the
// default's own files are never copied in again.

/// The store's list of base files it hides (removed from the state): one
/// `- <path>` line each. Not a text of the state (`from_texts` ignores it).
pub const HIDDEN: &str = "hidden.md";

/// The paths `HIDDEN`'s text names.
pub fn hidden_paths(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.trim().strip_prefix("- ")).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// The `app.md` settings layered one by one, not as the whole file: a
/// store's `app.md` that does not say one reads the base's.
pub const LAYERED_SETTINGS: &[&str] = &["scene", "motion"];

/// `app.md`'s `key: value` line for `key` (before the engines), if any.
fn setting_line<'a>(app: &'a str, key: &str) -> Option<&'a str> {
    app.lines().take_while(|l| l.trim() != "engines:").find(|l| l.strip_prefix(key).is_some_and(|r| r.starts_with(':')))
}

/// `over`'s `app.md` with each layered setting it does not say taken from
/// `base`'s (after its first line, the mode).
fn fill_app(base: &str, over: &str) -> String {
    let missing: Vec<&str> = LAYERED_SETTINGS.iter().filter(|k| setting_line(over, k).is_none()).filter_map(|k| setting_line(base, k)).collect();
    if missing.is_empty() {
        return over.to_string();
    }
    let (first, rest) = over.split_once('\n').unwrap_or((over, ""));
    format!("{first}\n{}{rest}", missing.iter().map(|l| format!("{l}\n")).collect::<String>())
}

/// `app.md` without the layered settings that say what `base`'s says.
fn strip_app(base: &str, app: &str) -> String {
    let same = |l: &str| LAYERED_SETTINGS.iter().any(|k| setting_line(base, k) == Some(l) && setting_line(app, k) == Some(l));
    app.split_inclusive('\n').filter(|l| !same(l.trim_end_matches('\n'))).collect()
}

/// What a store reads as: `base`, less what `over` hides, with `over`'s
/// own texts on top (nearest wins). `app.md` wins as a whole except for
/// the `LAYERED_SETTINGS` it does not say, which come from the base's.
pub fn merge(base: &Texts, over: &Texts) -> Texts {
    let hidden = over.get(HIDDEN).map(|t| hidden_paths(t)).unwrap_or_default();
    let mut out: Texts = base.iter().filter(|(k, _)| !hidden.contains(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    out.extend(over.iter().filter(|(k, _)| k.as_str() != HIDDEN).map(|(k, v)| (k.clone(), v.clone())));
    if let (Some(b), Some(o)) = (base.get("app.md"), over.get("app.md")) {
        out.insert("app.md".into(), fill_app(b, o));
    }
    out
}

/// The base as the state writes it: its texts read and written again
/// (`from_texts` → `to_texts`), so a state text is compared with what the
/// base would have given it. A base that does not read is taken as it is.
fn written(base: &Texts) -> Texts {
    from_texts(base).map(|s| to_texts(&s)).unwrap_or_else(|_| base.clone())
}

/// Is `text` at `path` what the base gives (as its file, or as written)?
fn from_base(base: &Texts, written: &Texts, path: &str, text: &str) -> bool {
    base.get(path).is_some_and(|t| t == text) || written.get(path).is_some_and(|t| t == text)
}

/// What a store over `base` keeps so it reads as `texts` (`merge(base,
/// split(base, texts))` reads as the same state): every text that differs
/// from the base, and `HIDDEN` naming each base file of the state that
/// `texts` no longer has. `app.md` is always kept: it is what says the
/// store holds a state of its own (`Store::exists`), less the layered
/// settings that say what the base's says (so a later default shows).
pub fn split(base: &Texts, texts: &Texts) -> Texts {
    let written = written(base);
    let mut out: Texts =
        texts.iter().filter(|(k, v)| k.as_str() == "app.md" || !from_base(base, &written, k, v)).map(|(k, v)| (k.clone(), v.clone())).collect();
    if let (Some(b), Some(app)) = (base.get("app.md"), out.get_mut("app.md")) {
        *app = strip_app(b, app);
    }
    let hidden: Vec<&String> = base.keys().filter(|k| written.contains_key(*k) && !texts.contains_key(*k)).collect();
    if !hidden.is_empty() {
        out.insert(HIDDEN.into(), hidden.iter().map(|p| format!("- {p}\n")).collect());
    }
    out
}

/// How a store's text stands to the base.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shadow {
    /// The store holds the path and its text is not the base's: it wins.
    Differs,
    /// The store holds the same text as the base (a full copy): the next
    /// save drops it.
    Same,
    /// The store hides the base's file (`HIDDEN`).
    Hidden,
}

/// Every base file the store `over` shadows, by path. `app.md` is listed
/// only when it differs: the store always keeps its own (`split`).
pub fn shadows(base: &Texts, over: &Texts) -> Vec<(String, Shadow)> {
    let written = written(base);
    let hidden = over.get(HIDDEN).map(|t| hidden_paths(t)).unwrap_or_default();
    let mut out: Vec<(String, Shadow)> = over
        .iter()
        .filter(|(k, _)| base.contains_key(*k))
        .map(|(k, v)| {
            let v = if k == "app.md" { fill_app(&base[k], v) } else { v.clone() };
            (k.clone(), if from_base(base, &written, k, &v) { Shadow::Same } else { Shadow::Differs })
        })
        .filter(|(k, s)| k != "app.md" || *s == Shadow::Differs)
        .collect();
    out.extend(hidden.into_iter().filter(|p| base.contains_key(p) && !over.contains_key(p)).map(|p| (p, Shadow::Hidden)));
    out.sort();
    out
}

// ---- Reset history (docs/default.md, "Reset") ----

/// The control's label, the same in every shell.
pub const RESET_LABEL: &str = "Reset history";

/// What a reset takes out of the store: everything it holds of its own.
/// The next load is the default alone (`default/`: the translator, on
/// `models.json`'s default model).
pub const RESET_REMOVES: &str = "every engine you added, every change to the default's engines, every message and transcript, the turn and terminal logs, the analytics spans and the artifacts' state";

/// What a reset leaves alone.
pub const RESET_KEEPS: &str = "this machine's state/local.md (appearance, hidden routes), the default/ folder, models.json, providers.json and sessions.json";

/// The name a reset moves a store to, beside it: `state` →
/// `state.reset-20261123T174500Z`, `state.db` → `state.reset-….db`.
/// `unix`: seconds since 1970, UTC.
pub fn reset_backup_name(name: &str, unix: i64) -> String {
    let (day, secs) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    let (y, m, d) = crate::live::civil_from_days(day);
    let stamp = format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}.reset-{stamp}.{ext}"),
        _ => format!("{name}.reset-{stamp}"),
    }
}

/// The texts a browser's store drops at a reset: all but its own
/// settings (`local/…`), which are this browser's, like `state/local.md`.
pub fn reset_removes(texts: &Texts) -> Vec<String> {
    texts.keys().filter(|k| !k.starts_with(LOCAL)).cloned().collect()
}

/// Texts in memory: a `Store` and a `TextStore`. For tests, and for a
/// shell that keeps nothing (a private window, a demo).
#[derive(Debug, Default)]
pub struct Memory {
    pub texts: RefCell<Texts>,
}

impl Store for Memory {
    fn describe(&self) -> String {
        "memory".into()
    }
    fn exists(&self) -> bool {
        self.texts.borrow().contains_key("app.md")
    }
    fn load(&self) -> Result<State, String> {
        from_texts(&self.texts.borrow())
    }
    fn save(&self, state: &State) -> Result<(), String> {
        let c = Changes::between(&self.texts.borrow(), &to_texts(state));
        c.apply_to(&mut self.texts.borrow_mut());
        Ok(())
    }
}

impl TextStore for Memory {
    fn describe(&self) -> String {
        "memory".into()
    }
    async fn read_all(&self) -> Result<Texts, String> {
        Ok(self.texts.borrow().clone())
    }
    async fn write(&self, changes: &Changes) -> Result<(), String> {
        changes.apply_to(&mut self.texts.borrow_mut());
        Ok(())
    }
}

/// Run a future that never waits on anything outside (an in-memory store,
/// a fake model) to its end. For tests and for pure code driving the
/// async traits with ready values; a future that would wait panics.
pub fn ready<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("store::ready: the future waits on something outside"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_from_before_threads_folds_and_its_old_posts_call_nobody() {
        let mut texts = Texts::new();
        texts.insert("app.md".into(), "engines:\n- translator\n- helper (child of translator)\n- quiet\n".into());
        texts.insert("engines/translator/convo/messages.md".into(), "### user\n\nhi\n".into());
        texts.insert("engines/translator/convo/run.md".into(), "heard: 1.1\n".into());
        texts.insert("shared.md".into(), "- 5 you → translator: old top\n- 6 you → quiet: for the empty one\n".into());
        texts.insert("engines/translator/shared.md".into(), "- 7 helper: old unaddressed\n".into());
        let mut s = from_texts(&texts).unwrap();
        let thread: Vec<String> = crate::shared::parse(&s.shared["translator"]).into_iter().map(|e| e.text).collect();
        assert_eq!(thread, ["old top", "old unaddressed"]);
        assert_eq!(crate::shared::parse(&s.shared["quiet"]).len(), 1);
        for i in 0..s.engines.len() {
            assert!(crate::desk::guided(&mut s, i).is_empty(), "{} was called by an old post", s.engines[i].id);
        }
        // Saved and read again: the old top file is gone, still nobody called.
        let again = to_texts(&s);
        assert!(!again.contains_key("shared.md"));
        let mut back = from_texts(&again).unwrap();
        for i in 0..back.engines.len() {
            assert!(crate::desk::guided(&mut back, i).is_empty());
        }
    }

    #[test]
    fn texts_hold_the_whole_state_and_come_back_the_same() {
        let state = files::sample();
        let texts = to_texts(&state);
        assert_eq!(from_texts(&texts).unwrap(), state);
        let e = &state.engines[0];
        for p in [
            "spec/engine.md",
            "spec/goal.md",
            "spec/mcp.json",
            "spec/scripts/note.md",
            "convo/messages.md",
            "artifacts/plan.md",
            "memory/learning.md",
        ] {
            assert!(texts.contains_key(&format!("engines/{}/{p}", e.id)), "{p}");
        }
        assert!(texts.contains_key(&format!("engines/{}/shared.md", state.engines[1].id)) && !texts.contains_key("shared.md") && texts["app.md"].contains("(child of engine-1)"));
    }

    #[test]
    fn the_memory_store_is_both_kinds_of_store() {
        let state = files::sample();
        let m = Memory::default();
        assert!(!Store::exists(&m));
        Store::save(&m, &state).unwrap();
        assert!(Store::exists(&m));
        assert_eq!(Store::load(&m).unwrap(), state);

        // Async: first load finds it; a save writes only what changed and
        // keeps local texts; removing an engine removes its texts.
        let (loaded, mut saver) = ready(load(&m)).unwrap();
        assert_eq!(loaded.as_ref(), Some(&state));
        let mut local = Texts::new();
        local.insert("local/settings.json".into(), "{}".into());
        let mut next = state.clone();
        next.engines[0].messages.push(crate::inference::Message { role: crate::inference::Role::User, text: "hi".into() });
        let written = ready(save(&m, &mut saver, &next, &local)).unwrap();
        assert_eq!(written, 2, "the messages and the local file");
        assert_eq!(ready(save(&m, &mut saver, &next, &local)).unwrap(), 0, "nothing changed");
        let gone = next.engines.remove(1).id;
        next.shared.remove(&gone);
        ready(save(&m, &mut saver, &next, &local)).unwrap();
        let texts = m.texts.borrow().clone();
        assert!(!texts.keys().any(|k| k.starts_with(&format!("engines/{gone}/"))));
        assert_eq!(texts["local/settings.json"], "{}");
        assert_eq!(from_texts(&texts).unwrap(), next);
        // Local texts go when no longer given.
        ready(save(&m, &mut saver, &next, &Texts::new())).unwrap();
        assert!(!m.texts.borrow().contains_key("local/settings.json"));
    }

    #[test]
    fn a_save_never_removes_an_engines_spans_log() {
        let state = files::sample();
        let mut old = to_texts(&state);
        let log = crate::spans::path(&state.engines[0].id);
        old.insert(log.clone(), "{}\n".into());
        let c = Changes::between(&old, &to_texts(&state));
        assert!(c.is_empty(), "{c:?}");
        assert!(is_log(&log) && !is_log("engines/x/memory/learning.log"));
    }

    /// The shipped default, and a store over it that holds what `state` needs.
    fn layered(state: &State) -> (Texts, Texts) {
        let base = crate::shipped::texts();
        let over = split(&base, &to_texts(state));
        (base, over)
    }

    #[test]
    fn the_scene_and_motion_settings_layer_one_by_one_over_the_default() {
        let base = crate::shipped::texts();
        assert!(base["app.md"].contains("scene: cat\n"), "the default names the cat");
        // An older store's app.md says only the mode: it reads the default's scene.
        let mut over = Texts::new();
        over.insert("app.md".into(), "mode: dark\n\nengines:\n- translator\n".into());
        let s = from_texts(&merge(&base, &over)).unwrap();
        assert_eq!((s.mode, s.scene.as_deref(), s.motion), (crate::theme::Mode::Dark, Some("cat"), Some(40.0)));
        assert!(shadows(&base, &over).iter().all(|(k, _)| k != "app.md"), "the same settings: not a difference");
        // Saved, the store keeps only what is its own: not the default's scene.
        let kept = split(&base, &to_texts(&s));
        assert!(!kept["app.md"].contains("scene:") && kept["app.md"].contains("mode: dark"), "{}", kept["app.md"]);
        // Its own choice (none is one) is kept and wins.
        let mut off = s.clone();
        off.scene = Some("none".into());
        let kept = split(&base, &to_texts(&off));
        assert!(kept["app.md"].contains("scene: none\n"));
        assert_eq!(from_texts(&merge(&base, &kept)).unwrap().scene.as_deref(), Some("none"));
        // A later default shows through where the store says nothing.
        let mut base2 = base.clone();
        base2.insert("app.md".into(), base["app.md"].replace("scene: cat", "scene: abyss"));
        assert_eq!(from_texts(&merge(&base2, &over)).unwrap().scene.as_deref(), Some("abyss"));
    }

    #[test]
    fn a_store_over_the_default_keeps_only_what_differs() {
        let base = crate::shipped::texts();
        let fresh = from_texts(&base).unwrap();
        let (_, over) = layered(&fresh);
        // Not a copy of the default: only what a state adds (its convo),
        // and app.md, which says the store holds a state.
        for k in over.keys() {
            assert!(k.contains("/convo/") || k == "app.md", "{k} is the default's own file, kept again");
        }
        assert_eq!(from_texts(&merge(&base, &over)).unwrap(), fresh);

        // An edit is kept; a removal is a hidden path; both read back.
        let mut s = fresh.clone();
        let t = &mut s.engines[0];
        let skill = t.skills.keys().next().unwrap().clone();
        t.skills.insert(skill.clone(), t.skills[&skill].replace("description:", "description: edited,"));
        let gone = t.skills.keys().nth(1).unwrap().clone();
        t.skills.remove(&gone);
        let artifact = t.artifacts.keys().next().unwrap().clone();
        t.artifacts.remove(&artifact);
        let (_, over) = layered(&s);
        assert!(over.contains_key(&format!("engines/translator/spec/skills/{skill}/SKILL.md")));
        let hidden = hidden_paths(&over[HIDDEN]);
        assert!(hidden.contains(&format!("engines/translator/spec/skills/{gone}/SKILL.md")), "{hidden:?}");
        assert_eq!(from_texts(&merge(&base, &over)).unwrap(), s);
        let kinds: Vec<Shadow> = shadows(&base, &over).into_iter().map(|(_, k)| k).collect();
        assert!(kinds.contains(&Shadow::Differs) && kinds.contains(&Shadow::Hidden));
        assert_eq!(shadows(&base, &over).into_iter().filter(|(_, k)| *k == Shadow::Same).count(), 0);

        // A later edit of the default shows through where the store holds nothing.
        let goal = "engines/translator/spec/goal.md";
        let mut base2 = base.clone();
        base2.insert(goal.into(), base[goal].replace("## Goal", "## Goal\n\nA newer goal line."));
        let read = from_texts(&merge(&base2, &over)).unwrap();
        assert!(read.engines[0].description.contains("A newer goal line"));
    }

    #[test]
    fn a_full_copy_reads_as_its_own_files_and_a_save_drops_the_copies() {
        let base = crate::shipped::texts();
        // A store the old way: the whole default copied in, one file edited.
        let mut full = to_texts(&from_texts(&base).unwrap());
        let goal = "engines/translator/spec/goal.md";
        full.insert(goal.into(), full[goal].replace("## Goal", "## Goal\n\nMine."));
        let state = from_texts(&merge(&base, &full)).unwrap();
        assert!(state.engines[0].description.contains("Mine."), "the store's file wins");
        let shadows = shadows(&base, &full);
        assert!(shadows.contains(&(goal.to_string(), Shadow::Differs)));
        assert!(shadows.iter().filter(|(_, k)| *k == Shadow::Same).count() > 10, "the rest are copies");
        // The next save keeps the edited file, not the copies.
        let over = split(&base, &to_texts(&state));
        assert!(over.contains_key(goal) && !over.contains_key("engines/translator/spec/engine.md"));
        assert_eq!(from_texts(&merge(&base, &over)).unwrap(), state);
        // A store with its own app.md keeps its engines: the default adds none.
        // (Its app.md names no scene or motion: those read the default's.)
        let own = to_texts(&files::sample());
        let with_default = State { scene: Some("cat".into()), motion: Some(40.0), ..files::sample() };
        let read = from_texts(&merge(&base, &own)).unwrap();
        assert_eq!(read, with_default);
        let over = split(&base, &own);
        assert!(hidden_paths(&over[HIDDEN]).iter().any(|p| p.starts_with("engines/translator/")));
        assert_eq!(from_texts(&merge(&base, &over)).unwrap(), with_default);
    }

    #[test]
    fn nothing_stored_loads_as_none() {
        let (state, saver) = ready(load(&Memory::default())).unwrap();
        assert!(state.is_none() && saver.last.is_empty());
    }

    #[test]
    fn a_reset_backup_is_named_beside_the_store_in_utc() {
        // 2026-11-23 17:45:00 UTC
        let t = days_from(2026, 11, 23) * 86_400 + 17 * 3600 + 45 * 60;
        assert_eq!(reset_backup_name("state", t), "state.reset-20261123T174500Z");
        assert_eq!(reset_backup_name("state.db", t), "state.reset-20261123T174500Z.db");
        let mut texts = Texts::new();
        texts.insert("app.md".into(), "x".into());
        texts.insert("engines/a/spans.jsonl".into(), "x".into());
        texts.insert("local/settings.json".into(), "{}".into());
        assert_eq!(reset_removes(&texts), vec!["app.md".to_string(), "engines/a/spans.jsonl".to_string()]);
    }

    fn days_from(y: i64, m: u32, d: u32) -> i64 {
        crate::live::days_from_civil(y, m, d)
    }
}
