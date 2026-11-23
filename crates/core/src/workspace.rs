//! A folder workspace: one folder on a person's computer, given to the
//! page by a companion (docs/companion-protocol.md, "The workspace folder" and "The workspace's state").
//!
//! The browser has no files and no terminal. A companion gives only thin
//! primitives over the folder (`Workspace`: read, write, list, stat; run,
//! start, tail, stop), and everything about context stays here, in core:
//! `Folder` is the `live::Host` over those primitives, so the `file` and
//! `terminal` artifacts (open, focus, edit, save, the unsaved edits laid
//! over the file, the open-file snapshot, sessions and their tails, the
//! callbacks) are the same code the desktop runs on its disk.
//!
//! ```text
//! page (wasm)                                   companion (Node / Rust / Python)
//! file::call, terminal::call, snapshots  ──▶  Folder<W>: Host  ──▶  W: Workspace  ──HTTP──▶  /v1/fs/*, /v1/term/*
//! ```
//!
//! **Roots.** An artifact's `root:` is a folder of the workspace: `.` is
//! the folder itself, `src` a folder in it. An absolute root names some
//! other computer's folder and is refused.
//!
//! **The snapshot** (`Snapshot`, `.dry/snapshot.json`): the workspace's
//! context at the last save, every engine's file and terminal artifacts
//! (open files with their focus, unsaved edits, sessions) and the tab a
//! person had in front. `restore` puts an artifact back where the state
//! lost it, and keeps one that has its own.
//!
//! Pure: the shell implements `Workspace` (the web app over the companion's
//! HTTP; a test in memory).

use serde::{Deserialize, Serialize};

use crate::live::{Content, Host, Job, Live, Proc};
use crate::state::State;

/// A session's output when the companion no longer has it.
pub const LOST: &str = "(the companion restarted since: this session's output is gone)";

/// The snapshot's version (`"version": 1`).
pub const VERSION: u32 = 1;

/// What a path in the folder is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stat {
    File { size: u64, modified: i64 },
    Dir,
    Missing,
}

/// Where a terminal session is, by its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    Running,
    /// It ended: its exit code (`None`: stopped by a signal).
    Exited(Option<i32>),
    /// The companion does not know the key: it restarted since.
    Unknown,
}

/// A session's state and the last lines it printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tail {
    pub run: Run,
    pub text: String,
}

/// The primitives a companion gives over its folder. Paths are relative
/// to the folder, `/`-separated; the companion refuses what leaves it
/// (`..`, absolute paths, links out), secret-looking names and `.dry/`,
/// and says why in the error.
pub trait Workspace {
    /// Unix seconds now.
    fn now(&self) -> i64;
    /// The files under `dir` (`""`: the folder), relative to it: a
    /// folder's own files first, then its folders', at most `max`.
    fn list(&self, dir: &str, max: usize) -> Result<Vec<String>, String>;
    fn read(&self, path: &str) -> Content;
    fn write(&self, path: &str, text: &str) -> Result<(), String>;
    fn stat(&self, path: &str) -> Result<Stat, String>;
    /// Run `command` in folder `cwd` and wait: its exit code.
    fn run(&self, key: &str, cwd: &str, command: &str) -> Result<Option<i32>, String>;
    /// Start it and return at once (`callback`: its end is an event).
    fn start(&self, key: &str, cwd: &str, command: &str, callback: bool) -> Result<(), String>;
    fn tail(&self, key: &str, lines: usize) -> Result<Tail, String>;
    fn stop(&self, key: &str) -> Result<(), String>;
}

/// A session's key: `<engine>/<artifact>/<n>`, as the companion keeps it.
pub fn key(job: &Job) -> String {
    format!("{}/{}/{}", job.engine, job.artifact, job.session)
}

/// An artifact's root as a folder of the workspace: `""` for the folder
/// itself, else its relative path. An absolute root is refused.
pub fn root_dir(root: &str) -> Result<String, String> {
    let r = root.trim();
    if r.starts_with('/') || r.starts_with('\\') || r.get(1..2) == Some(":") {
        return Err(format!("the root {r} is a folder on some computer: in a workspace a root is a folder in it (`.` is the folder)"));
    }
    let parts: Vec<&str> = r.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.contains(&"..") {
        return Err(format!("the root {r} leaves the workspace folder"));
    }
    Ok(parts.join("/"))
}

/// A path under an artifact's root, as a path in the folder.
pub fn in_root(root: &str, path: &str) -> Result<String, String> {
    let dir = root_dir(root)?;
    Ok(if dir.is_empty() { path.to_string() } else { format!("{dir}/{path}") })
}

/// The folder as a `Host`: what `file` and `terminal` artifacts reach.
pub struct Folder<W>(pub W);

impl<W: Workspace> Host for Folder<W> {
    fn now(&self) -> i64 {
        self.0.now()
    }
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        self.0.list(&root_dir(root)?, max)
    }
    fn read(&self, root: &str, path: &str) -> Content {
        match in_root(root, path) {
            Ok(p) => self.0.read(&p),
            Err(why) => Content::Missing(why),
        }
    }
    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        self.0.write(&in_root(root, path)?, text)
    }
    fn run(&self, job: &Job) -> Result<Option<i32>, String> {
        self.0.run(&key(job), &root_dir(job.root)?, job.command)
    }
    fn spawn(&self, job: &Job) -> Result<(), String> {
        self.0.start(&key(job), &root_dir(job.root)?, job.command, true)
    }
    fn status(&self, job: &Job) -> Proc {
        match self.0.tail(&key(job), 0) {
            Ok(Tail { run: Run::Running, .. }) => Proc::Running,
            Ok(Tail { run: Run::Exited(code), .. }) => Proc::Ended(code),
            // Lost when the companion restarted, or it cannot be reached: stopped.
            Ok(Tail { run: Run::Unknown, .. }) | Err(_) => Proc::Ended(None),
        }
    }
    fn tail(&self, job: &Job, lines: usize) -> String {
        match self.0.tail(&key(job), lines) {
            Ok(Tail { run: Run::Unknown, .. }) => LOST.to_string(),
            Ok(t) => t.text,
            Err(_) => String::new(),
        }
    }
}

// ---- the snapshot: .dry/snapshot.json ----

/// The workspace's context at a save.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    /// Unix seconds.
    pub saved: i64,
    /// The tab a person had in front (`<engine>/<artifact>:<path>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default)]
    pub engines: Vec<EngineSnap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EngineSnap {
    pub engine: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artifacts: Vec<ArtifactSnap>,
}

/// One file or terminal artifact: its state as fields (for any reader)
/// and as the text dry keeps (what `restore` puts back).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtifactSnap {
    pub name: String,
    pub kind: String,
    pub root: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open: Vec<OpenSnap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<EditSnap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<SessionSnap>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenSnap {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<[usize; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditSnap {
    pub path: String,
    pub old: String,
    pub new: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionSnap {
    pub id: u32,
    pub mode: String,
    /// `running`, `exit:<code>` or `stopped`.
    pub status: String,
    pub started: i64,
    pub command: String,
}

/// An artifact's text as a snapshot entry, when it is a file or terminal one.
fn artifact_snap(name: &str, text: &str) -> Option<ArtifactSnap> {
    let body = body_of(text);
    match Live::of(text)? {
        Live::File => {
            let s = crate::file::State::parse(&body).ok()?;
            Some(ArtifactSnap {
                name: name.into(),
                kind: "file".into(),
                root: s.root.clone(),
                open: s.open.iter().map(|o| OpenSnap { path: o.path.clone(), focus: o.focus.map(|(a, b)| [a, b]) }).collect(),
                edits: s.edits.iter().map(|e| EditSnap { path: e.path.clone(), old: e.old.clone(), new: e.new.clone() }).collect(),
                sessions: Vec::new(),
                text: text.to_string(),
            })
        }
        Live::Terminal => {
            let s = crate::terminal::State::parse(&body).ok()?;
            let line = s.to_text();
            // The session lines as the state writes them: `session: <n> <mode> <status> <started> <command>`.
            let sessions = line
                .lines()
                .filter_map(|l| l.strip_prefix("session: "))
                .zip(&s.sessions)
                .map(|(l, x)| {
                    let w: Vec<&str> = l.splitn(5, ' ').collect();
                    SessionSnap {
                        id: x.id,
                        mode: w.get(1).unwrap_or(&"").to_string(),
                        status: w.get(2).unwrap_or(&"").to_string(),
                        started: x.started,
                        command: x.command.clone(),
                    }
                })
                .collect();
            Some(ArtifactSnap {
                name: name.into(),
                kind: "terminal".into(),
                root: s.root,
                open: Vec::new(),
                edits: Vec::new(),
                sessions,
                text: text.to_string(),
            })
        }
        _ => None,
    }
}

/// An artifact's text with a `uses: [..]` header read as its kind line,
/// so the kind's own parser reads it.
fn body_of(text: &str) -> String {
    match Live::of(text) {
        Some(k) if !text.starts_with("kind:") => {
            let rest = text.split_once('\n').map_or("", |(_, r)| r);
            format!("kind: {}\n{rest}", k.as_str())
        }
        _ => text.to_string(),
    }
}

/// Whether a snapshot entry holds something to bring back.
fn has_context(a: &ArtifactSnap) -> bool {
    !a.open.is_empty() || !a.edits.is_empty() || !a.sessions.is_empty()
}

impl Snapshot {
    /// Every engine's file and terminal artifacts now.
    pub fn of(state: &State, saved: i64, focus: Option<&str>) -> Snapshot {
        let engines = state
            .engines
            .iter()
            .map(|e| EngineSnap {
                engine: e.id.clone(),
                name: e.name.clone(),
                artifacts: e.artifacts.iter().filter_map(|(n, t)| artifact_snap(n, t)).collect(),
            })
            .filter(|e| !e.artifacts.is_empty())
            .collect();
        Snapshot { version: VERSION, saved, focus: focus.map(str::to_string), engines }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Result<Snapshot, String> {
        let s: Snapshot = serde_json::from_str(text).map_err(|e| format!("the snapshot does not read: {e}"))?;
        if s.version != VERSION {
            return Err(format!("the snapshot is version {}, this reads {VERSION}", s.version));
        }
        Ok(s)
    }

    /// The open files, unsaved edits and sessions this snapshot has that
    /// `state` lost: an artifact that is gone, or that holds no context
    /// while the snapshot's does, gets the snapshot's text back. One that
    /// has its own context is kept (the state is the newer). Engines the
    /// state does not have are not made. What it brought back, one line each.
    pub fn restore(&self, state: &mut State) -> Vec<String> {
        let mut done = Vec::new();
        for es in &self.engines {
            let Some(e) = state.engines.iter_mut().find(|e| e.id == es.engine) else { continue };
            for a in es.artifacts.iter().filter(|a| has_context(a)) {
                let now = e.artifacts.get(&a.name).and_then(|t| artifact_snap(&a.name, t));
                let lost = match (&now, e.artifacts.contains_key(&a.name)) {
                    (_, false) => true,
                    (Some(n), true) => n.kind == a.kind && !has_context(n),
                    (None, true) => false,
                };
                if lost {
                    e.artifacts.insert(a.name.clone(), a.text.clone());
                    let what = match a.kind.as_str() {
                        "file" => format!("{} open, {} unsaved", a.open.len(), a.edits.len()),
                        _ => format!("{} sessions", a.sessions.len()),
                    };
                    done.push(format!("{}: {} ({what})", e.name, a.name));
                }
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calls::Call;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A companion's folder in memory: files by path, sessions by key.
    #[derive(Default)]
    struct Fake {
        files: RefCell<BTreeMap<String, String>>,
        sessions: RefCell<BTreeMap<String, (Run, String, String)>>,
        calls: RefCell<Vec<String>>,
    }

    impl Workspace for Fake {
        fn now(&self) -> i64 {
            1_795_400_000
        }
        fn list(&self, dir: &str, max: usize) -> Result<Vec<String>, String> {
            self.calls.borrow_mut().push(format!("list {dir}"));
            let pre = if dir.is_empty() { String::new() } else { format!("{dir}/") };
            Ok(self.files.borrow().keys().filter_map(|k| k.strip_prefix(&pre)).take(max).map(str::to_string).collect())
        }
        fn read(&self, path: &str) -> Content {
            self.calls.borrow_mut().push(format!("read {path}"));
            match self.files.borrow().get(path) {
                Some(t) => Content::Text(t.clone()),
                None => Content::Missing(format!("no such file: {path}")),
            }
        }
        fn write(&self, path: &str, text: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("write {path}"));
            self.files.borrow_mut().insert(path.into(), text.into());
            Ok(())
        }
        fn stat(&self, path: &str) -> Result<Stat, String> {
            Ok(match self.files.borrow().get(path) {
                Some(t) => Stat::File { size: t.len() as u64, modified: 0 },
                None => Stat::Missing,
            })
        }
        fn run(&self, key: &str, cwd: &str, command: &str) -> Result<Option<i32>, String> {
            self.calls.borrow_mut().push(format!("run {key} in '{cwd}': {command}"));
            self.sessions.borrow_mut().insert(key.into(), (Run::Exited(Some(0)), format!("ran {command}\n"), cwd.into()));
            Ok(Some(0))
        }
        fn start(&self, key: &str, cwd: &str, command: &str, callback: bool) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("start {key} in '{cwd}' callback={callback}: {command}"));
            self.sessions.borrow_mut().insert(key.into(), (Run::Running, String::new(), cwd.into()));
            Ok(())
        }
        fn tail(&self, key: &str, _: usize) -> Result<Tail, String> {
            Ok(self
                .sessions
                .borrow()
                .get(key)
                .map_or(Tail { run: Run::Unknown, text: String::new() }, |(r, t, _)| Tail { run: *r, text: t.clone() }))
        }
        fn stop(&self, key: &str) -> Result<(), String> {
            match self.sessions.borrow_mut().get_mut(key) {
                Some(s) => {
                    s.0 = Run::Exited(None);
                    Ok(())
                }
                None => Err(format!("no session {key}")),
            }
        }
    }

    fn c(s: &str) -> Call {
        crate::calls::parse(s).unwrap().remove(0)
    }

    fn folder() -> Folder<Fake> {
        let f = Fake::default();
        f.files.borrow_mut().insert("README.md".into(), "# hello\nline two\n".into());
        f.files.borrow_mut().insert("src/main.rs".into(), "fn main() {}\n".into());
        Folder(f)
    }

    #[test]
    fn roots_are_folders_of_the_workspace() {
        assert_eq!(root_dir("."), Ok(String::new()));
        assert_eq!(root_dir("./src/"), Ok("src".into()));
        assert_eq!(in_root(".", "a.txt"), Ok("a.txt".into()));
        assert_eq!(in_root("src", "main.rs"), Ok("src/main.rs".into()));
        assert!(root_dir("/Users/x/project").unwrap_err().contains("folder in it"));
        assert!(root_dir("C:\\x").is_err());
        assert!(root_dir("src/../..").unwrap_err().contains("leaves"));
        let f = folder();
        assert!(matches!(f.read("/abs", "x"), Content::Missing(why) if why.contains("folder in it")));
        assert!(f.0.calls.borrow().is_empty(), "a refused root never reaches the companion");
    }

    #[test]
    fn the_file_artifact_opens_edits_and_saves_through_the_primitives() {
        let f = folder();
        let mut text = "kind: file\nroot: .\n".to_string();
        crate::file::call(&mut text, &c(r#"open(path="README.md")"#), &f).unwrap();
        crate::file::call(&mut text, &c(r##"edit(path="README.md", old="# hello", new="# hi")"##), &f).unwrap();
        // The unsaved edit is laid over the file in the snapshot; the disk is unchanged.
        let snap = crate::file::snapshot(&text, &f);
        assert!(snap.contains("# hi"), "{snap}");
        assert_eq!(f.0.files.borrow()["README.md"], "# hello\nline two\n");
        assert!(crate::file::State::parse(&text).unwrap().edits.len() == 1);
        crate::file::call(&mut text, &c(r#"save(path="README.md")"#), &f).unwrap();
        assert_eq!(f.0.files.borrow()["README.md"], "# hi\nline two\n");
        assert!(crate::file::State::parse(&text).unwrap().edits.is_empty());
        assert!(f.0.calls.borrow().iter().any(|c| c == "write README.md"));
        // A root inside the folder: paths go to the folder joined.
        let mut sub = "kind: file\nroot: src\n".to_string();
        crate::file::call(&mut sub, &c(r#"open(path="main.rs")"#), &f).unwrap();
        assert!(crate::file::snapshot(&sub, &f).contains("fn main()"));
        assert!(f.0.calls.borrow().iter().any(|c| c == "read src/main.rs"));
        // Paths that leave the root are refused by core before the companion sees them.
        assert!(crate::file::call(&mut text, &c(r#"open(path="../x")"#), &f).is_err());
        assert!(crate::file::call(&mut text, &c(r#"open(path=".env")"#), &f).is_err());
    }

    #[test]
    fn the_terminal_artifact_runs_starts_and_settles_through_the_primitives() {
        let f = folder();
        let mut text = "kind: terminal\nroot: .\n".to_string();
        let ran = crate::terminal::call("e1", "term", &mut text, &c(r#"run(command="cargo test")"#), &f).unwrap();
        assert!(ran.contains("exit 0") && ran.contains("ran cargo test"), "{ran}");
        crate::terminal::call("e1", "term", &mut text, &c(r#"start(command="npm run dev")"#), &f).unwrap();
        assert!(f.0.calls.borrow().iter().any(|c| c == "start e1/term/2 in '' callback=true: npm run dev"), "{:?}", f.0.calls.borrow());
        assert!(crate::terminal::settle("e1", "term", &mut text, &f).is_empty(), "still running");
        f.0.stop("e1/term/2").unwrap();
        let back = crate::terminal::settle("e1", "term", &mut text, &f);
        assert_eq!(back.len(), 1, "the start's callback");
        assert!(text.contains("session: 2 start stopped"), "{text}");
        // A key the companion does not know (it restarted) reads as stopped.
        let mut lost = "kind: terminal\nroot: .\nsession: 7 start running 1 sleep 9\n".to_string();
        assert_eq!(crate::terminal::settle("e1", "lost", &mut lost, &f).len(), 1);
        assert!(lost.contains("session: 7 start stopped"));
        let job = Job { engine: "e1", artifact: "lost", session: 7, root: ".", command: "sleep 9" };
        assert_eq!(f.tail(&job, 5), LOST, "its output says why it is gone");
    }

    fn state_with(file: &str, term: &str) -> State {
        let mut s = State::default();
        let mut e = crate::state::Engine::new("dev", "", crate::inference::OpenAi::default());
        e.id = "e1".into();
        e.artifacts.insert("files".into(), file.into());
        e.artifacts.insert("term".into(), term.into());
        e.artifacts.insert("notes".into(), "plain text".into());
        s.engines.push(e);
        s
    }

    const FILES: &str =
        "kind: file\nroot: .\nopen: README.md 1-2\nopen: src/main.rs\nedit: README.md\n~~~ old\n# hello\n~~~ new\n# hi\n~~~\n";
    const TERM: &str = "kind: terminal\nroot: .\nsession: 1 run exit:0 1795399990 ls\nsession: 2 start running 1795399995 npm run dev\n";

    #[test]
    fn the_snapshot_says_what_is_open_unsaved_and_running() {
        let s = Snapshot::of(&state_with(FILES, TERM), 1_795_400_000, Some("e1/files:README.md"));
        assert_eq!(s.engines.len(), 1);
        let a = &s.engines[0].artifacts;
        assert_eq!(a.len(), 2, "plain artifacts are not in it");
        let files = a.iter().find(|x| x.kind == "file").unwrap();
        assert_eq!(
            files.open,
            vec![OpenSnap { path: "README.md".into(), focus: Some([1, 2]) }, OpenSnap { path: "src/main.rs".into(), focus: None }]
        );
        assert_eq!(files.edits, vec![EditSnap { path: "README.md".into(), old: "# hello".into(), new: "# hi".into() }]);
        let term = a.iter().find(|x| x.kind == "terminal").unwrap();
        assert_eq!(
            term.sessions.iter().map(|x| (x.id, x.mode.as_str(), x.status.as_str())).collect::<Vec<_>>(),
            [(1, "run", "exit:0"), (2, "start", "running")]
        );
        let back = Snapshot::from_json(&s.to_json()).unwrap();
        assert_eq!(back, s, "it reads back as written");
        assert!(s.to_json().contains("\"focus\":[1,2]"));
        assert!(Snapshot::from_json(r#"{"version":2,"saved":0}"#).unwrap_err().contains("version 2"));
    }

    #[test]
    fn restore_brings_back_what_the_state_lost_and_keeps_what_it_has() {
        let snap = Snapshot::of(&state_with(FILES, TERM), 1, None);
        // The page reopened with a state that lost the file artifact's context and the terminal artifact.
        let mut lost = state_with("kind: file\nroot: .\n", TERM);
        lost.engines[0].artifacts.remove("term");
        let done = snap.restore(&mut lost);
        assert_eq!(done.len(), 2, "{done:?}");
        assert_eq!(lost.engines[0].artifacts["files"], FILES);
        assert_eq!(lost.engines[0].artifacts["term"], TERM);
        // A state with its own context keeps it (it is the newer).
        let newer = "kind: file\nroot: .\nopen: other.md\n";
        let mut kept = state_with(newer, TERM);
        assert!(snap.restore(&mut kept).is_empty());
        assert_eq!(kept.engines[0].artifacts["files"], newer);
        // An engine the state does not have is not made; a plain artifact is never replaced.
        let mut other = State::default();
        assert!(snap.restore(&mut other).is_empty());
        assert!(other.engines.is_empty());
    }

    #[test]
    fn a_uses_header_is_read_as_its_kind() {
        let text = "uses: [file, clock]\nroot: .\nopen: README.md\n";
        let a = artifact_snap("code", text).unwrap();
        assert_eq!((a.kind.as_str(), a.open.len()), ("file", 1));
        assert_eq!(a.text, text, "the text is kept as dry keeps it");
    }
}
