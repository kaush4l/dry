//! Live updates: a watcher, the event log and the `/events` stream.
//!
//! The watcher polls, like the window does every second (`files_stamp`):
//! twice a second it fingerprints two sets of files by path, size and
//! modification time, so a new, changed or removed file all count.
//!
//! - `store`: the store (`DRY_STORE`, else `md:state`: the whole folder,
//!   or the SQLite file) and `default/` under it. A terminal artifact's
//!   logs live in it too. The host's own saves are not counted: it takes
//!   the store's print under the same lock it saves under (`store_print`),
//!   so only a write by someone else is a change (the host then reads the
//!   store again, see `host::Host::outside_change`).
//! - `files`: what the app is built from: `routes/`, `components/`,
//!   `themes/`, `scenes/`, `assets/fonts/`, `state/local.md`,
//!   `models.json`, `sessions.json`.
//!
//! One generation number counts every change: the watcher's, and every
//! write the host makes (`bump`). It is the store's generation too: a
//! send's `base` is checked against it, and the host lock carries it.
//!
//! Every event goes into a log with an id, newest last (`push`), so a
//! client that drops resumes where it was (`Last-Event-ID`, or `?since=`):
//!
//! ```text
//! hello     {"gen":N,"last":ID}                     on connect (no id); "reset":true when the log no longer reaches back
//! change    {"gen":N,"what":["store","files"]}      the store or the files changed
//! activity  {"engine":"<id>","index":i,"kind":"llm","label":"…","short":"…","token":"activity_llm"}
//! message   {"engine":"<id>","index":i,"n":k,"role":"assistant","gen":N}
//! turn      {"engine":"<id>","index":i,"turn":"<id>/<n>","status":"ready","line":"…","gen":N}
//! ```
//!
//! A comment every 15 s keeps proxies from closing the stream.

use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How many events the log keeps for clients that resume.
const KEEP: usize = 1000;

/// One event as sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: u64,
    pub name: &'static str,
    pub data: String,
}

impl Event {
    pub fn text(&self) -> String {
        format!("id: {}\nevent: {}\ndata: {}\n\n", self.id, self.name, self.data)
    }
}

#[derive(Default)]
struct Log {
    events: VecDeque<Event>,
    next: u64,
}

/// The generation, the event log, and the store's print.
#[derive(Default)]
pub struct Live {
    generation: AtomicU64,
    log: Mutex<Log>,
    /// The store's fingerprint as last seen by the watcher or written by
    /// the host. Held while either looks at the store, so the host's own
    /// save never reads as someone else's.
    pub store_print: Mutex<Option<u64>>,
}

impl Live {
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// A new generation, and its `change` event.
    pub fn bump(&self, what: &[&str]) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let what: Vec<String> = what.iter().map(|w| format!("\"{w}\"")).collect();
        self.push("change", format!("{{\"gen\":{generation},\"what\":[{}]}}", what.join(",")));
        generation
    }

    /// Add an event to the log; its id.
    pub fn push(&self, name: &'static str, data: String) -> u64 {
        let mut log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        log.next += 1;
        let id = log.next;
        log.events.push_back(Event { id, name, data });
        while log.events.len() > KEEP {
            log.events.pop_front();
        }
        id
    }

    /// The id of the newest event (0: none yet).
    pub fn last_id(&self) -> u64 {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).next
    }

    /// Every event after `id`; `None` when the log no longer reaches back
    /// that far (the client should draw everything again).
    pub fn since(&self, id: u64) -> Option<Vec<Event>> {
        let log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        if id > log.next {
            return None;
        }
        let first = log.events.front().map_or(log.next + 1, |e| e.id);
        if id + 1 < first && id < log.next {
            return None;
        }
        Some(log.events.iter().filter(|e| e.id > id).cloned().collect())
    }
}

/// The files the store is in, and the default under it (`default/`, when
/// read from disk: an edit there reads in like an edit of the store).
pub fn store_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = dry_tools::store::base_dir().into_iter().collect();
    if let Ok(store) = dry_tools::store::open_default() {
        let spec = store.describe();
        match spec.split_once(':') {
            Some(("md", p)) => out.push(PathBuf::from(p)),
            Some(("sqlite", p)) => {
                out.push(PathBuf::from(p));
                out.push(PathBuf::from(format!("{p}-wal")));
                out.push(store.files_dir());
            }
            _ => {}
        }
    }
    out
}

/// What the store's print leaves out: this machine's `local.md`, which
/// counts as a file of the app.
pub fn store_skip() -> Vec<PathBuf> {
    vec![dry_tools::local_theme_file()]
}

/// The store's print now.
pub fn store_print() -> u64 {
    fingerprint(&store_paths(), &store_skip())
}

/// The files the app is built from.
fn file_paths() -> Vec<PathBuf> {
    let root = dry_tools::root();
    vec![
        dry_tools::routes_dir(),
        dry_tools::components_dir(),
        dry_tools::themes_dir(),
        dry_tools::scenes_dir(),
        root.join("assets/fonts"),
        dry_tools::local_theme_file(),
        root.join("models.json"),
        dry_tools::sessions_file(),
    ]
}

/// A fingerprint of every file under `paths` (path, size, modified),
/// `skip` left out; a missing path counts as nothing.
pub fn fingerprint(paths: &[PathBuf], skip: &[PathBuf]) -> u64 {
    fn walk(path: &Path, skip: &[PathBuf], h: &mut DefaultHasher, depth: usize) {
        if skip.iter().any(|s| s == path) || depth > 12 {
            return;
        }
        let Ok(meta) = std::fs::symlink_metadata(path) else { return };
        path.hash(h);
        if meta.is_dir() {
            let Ok(entries) = std::fs::read_dir(path) else { return };
            let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            children.sort();
            for c in children {
                walk(&c, skip, h, depth + 1);
            }
        } else {
            meta.len().hash(h);
            meta.modified().ok().hash(h);
        }
    }
    let mut h = DefaultHasher::new();
    for p in paths {
        walk(p, skip, &mut h, 0);
    }
    h.finish()
}

/// Start the watcher on its own thread; it runs as long as the server.
/// `outside`: called (on the watcher's thread) when the store changed and
/// the host did not write it.
pub fn watch(live: Arc<Live>, every: Duration, outside: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        {
            let mut print = live.store_print.lock().unwrap_or_else(|e| e.into_inner());
            *print = Some(store_print());
        }
        let mut files = fingerprint(&file_paths(), &[]);
        loop {
            std::thread::sleep(every);
            let store_changed = {
                let mut print = live.store_print.lock().unwrap_or_else(|e| e.into_inner());
                let now = store_print();
                let changed = *print != Some(now);
                *print = Some(now);
                changed
            };
            let now = fingerprint(&file_paths(), &[]);
            let files_changed = now != files;
            files = now;
            let mut what = Vec::new();
            if store_changed {
                what.push("store");
            }
            if files_changed {
                what.push("files");
            }
            if store_changed {
                outside();
            }
            if !what.is_empty() {
                live.bump(&what);
            }
        }
    });
}

/// The `/events` body: `hello`, then what the client missed after
/// `resume` (if the log still has it), then every new event as it comes,
/// and a keep-alive comment every 15 s.
pub fn stream(live: Arc<Live>, resume: Option<u64>) -> impl futures_util::Stream<Item = Result<actix_web::web::Bytes, std::convert::Infallible>> {
    let (seen, missed, reset) = match resume.map(|id| (id, live.since(id))) {
        Some((id, Some(missed))) => (id, missed, false),
        Some((_, None)) => (live.last_id(), Vec::new(), true),
        None => (live.last_id(), Vec::new(), false),
    };
    let mut first = format!(
        "retry: 2000\nevent: hello\ndata: {{\"gen\":{},\"last\":{seen}{}}}\n\n",
        live.generation(),
        if reset { ",\"reset\":true" } else { "" }
    );
    let mut seen = seen;
    for e in missed {
        first.push_str(&e.text());
        seen = e.id;
    }
    futures_util::stream::unfold((live, seen, Some(first), 0u32), |(live, seen, first, quiet)| async move {
        if let Some(text) = first {
            return Some((Ok(actix_web::web::Bytes::from(text)), (live, seen, None, 0)));
        }
        let mut quiet = quiet;
        loop {
            actix_web::rt::time::sleep(Duration::from_millis(100)).await;
            if live.last_id() > seen {
                let events = live.since(seen).unwrap_or_default();
                let last = events.last().map_or(live.last_id(), |e| e.id);
                let text: String = events.iter().map(Event::text).collect();
                return Some((Ok(actix_web::web::Bytes::from(text)), (live, last, None, 0)));
            }
            quiet += 1;
            if quiet >= 150 {
                return Some((Ok(actix_web::web::Bytes::from_static(b": keep-alive\n\n")), (live, seen, None, 0)));
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_changed_or_removed_file_changes_the_fingerprint() {
        let dir = std::env::temp_dir().join(format!("dry-server-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        let paths = [dir.clone()];
        let empty = fingerprint(&paths, &[]);
        std::fs::write(dir.join("a/x.md"), "one").unwrap();
        let one = fingerprint(&paths, &[]);
        assert_ne!(empty, one);
        assert_eq!(one, fingerprint(&paths, &[]), "nothing changed, same print");
        std::fs::write(dir.join("a/x.md"), "two!").unwrap();
        assert_ne!(one, fingerprint(&paths, &[]));
        assert_eq!(fingerprint(&paths, &[dir.join("a")]), fingerprint(&paths, &[dir.join("a")]));
        std::fs::remove_file(dir.join("a/x.md")).unwrap();
        assert_eq!(fingerprint(&paths, &[]), empty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_change_is_a_generation_and_an_event_a_client_can_resume_from() {
        let live = Live::default();
        assert_eq!(live.bump(&["store", "files"]), 1);
        assert_eq!(live.generation(), 1);
        let first = live.since(0).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].text(), "id: 1\nevent: change\ndata: {\"gen\":1,\"what\":[\"store\",\"files\"]}\n\n");
        live.push("activity", "{}".into());
        live.push("message", "{}".into());
        assert_eq!(live.since(1).unwrap().iter().map(|e| e.name).collect::<Vec<_>>(), ["activity", "message"]);
        assert!(live.since(3).unwrap().is_empty(), "nothing after the newest");
        assert!(live.since(9).is_none(), "an id from the future (another server run): start over");
        for _ in 0..KEEP + 5 {
            live.push("activity", "{}".into());
        }
        assert!(live.since(1).is_none(), "the log no longer reaches back: start over");
        let last = live.last_id();
        assert_eq!(live.since(last - 3).unwrap().len(), 3);
    }
}
