//! Live views: the UI an artifact kind attaches to show itself live.
//!
//! An artifact kind (`crate::live::Live`) declares the parts of a
//! workbench that show it (`Live::view`). Core names the parts and builds
//! what each shows (`LiveView`); a shell only draws them. The developer
//! artifacts attach a code editor's layout, the one a person knows:
//!
//! ```text
//! ┌ explorer ──┬ editor (tabs) ──────────────┬ chat ──────┐
//! │ root       │ every file the engine has   │ the engine's│
//! │ ● open     │ open, focus lit, unsaved *  │ history and │
//! │ ● unsaved  ├ panel ──────────────────────┤ composer    │
//! │            │ terminal sessions: running  │             │
//! │            │ servers and tests first     │             │
//! ├────────────┴─────────────────────────────┴─────────────┤
//! │ status: engine · activity · model · prompt ≈ x of y    │
//! └────────────────────────────────────────────────────────┘
//! ```
//!
//! | kind | parts |
//! |---|---|
//! | `file` | explorer, editor |
//! | `terminal` | panel |
//! | every view | chat, status |
//!
//! A page places the parts as data (`pages::Component::LiveView`, one per
//! part, in `panes`), so the same parts can make other layouts. It is
//! built from the exact view (`crate::inspect`, taken by the shell after
//! each step and each callback), never from the disk at a frame: what it
//! shows is what the engine was sent.
//!
//! Pure.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::activity::Activity;
use crate::inspect::{ArtifactView, Detail, FileLine, SessionView};
use crate::live::Live;
use crate::state::Engine;

/// One part of a live view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Part {
    /// The file artifact's root as a tree: open files and unsaved edits marked.
    Explorer,
    /// One tab per file the engine has open: focus lit, unsaved lines marked.
    Editor,
    /// The terminal artifact's sessions: running ones first.
    Panel,
    /// The engine's conversation and the composer.
    Chat,
    /// One strip: engine, activity, model, the prompt against its window.
    Status,
}

impl Part {
    pub const ALL: [Part; 5] = [Part::Explorer, Part::Editor, Part::Panel, Part::Chat, Part::Status];

    /// Its word in a page file (`"part": "explorer"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Part::Explorer => "explorer",
            Part::Editor => "editor",
            Part::Panel => "panel",
            Part::Chat => "chat",
            Part::Status => "status",
        }
    }

    pub fn parse(word: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.as_str() == word)
    }
}

/// The parts every live view has, whatever artifacts it shows.
pub const ALWAYS: [Part; 2] = [Part::Chat, Part::Status];

impl Live {
    /// The parts of a live view that show this kind; empty: it attaches
    /// none (its own page shows it).
    pub fn view(self) -> &'static [Part] {
        match self {
            Live::File => &[Part::Explorer, Part::Editor],
            Live::Terminal => &[Part::Panel],
            _ => &[],
        }
    }
}

/// The artifacts of `e` that attach a live view, `(name, kind)`.
pub fn developer(e: &Engine) -> Vec<(String, Live)> {
    crate::live::present(e).into_iter().filter(|(_, k)| !k.view().is_empty()).collect()
}

/// Where engine `index`'s live view is: all its developer artifacts in
/// one workbench (`scope` none), or one artifact's.
pub fn path(index: usize, scope: Option<&str>) -> String {
    match scope {
        None => format!("{}/live", crate::routes::engine_path(index)),
        Some(name) => format!("{}/artifacts/{name}/live", crate::routes::engine_path(index)),
    }
}

/// A live view, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveView {
    /// One artifact's view (its name), or the whole engine's.
    pub scope: Option<String>,
    /// The parts that have something to show, in `Part` order.
    pub parts: Vec<Part>,
    /// One per file artifact.
    pub roots: Vec<Root>,
    /// Every open file of every file artifact, in their order.
    pub tabs: Vec<Tab>,
    /// Every session of every terminal artifact: running first.
    pub sessions: Vec<Session>,
    pub status: StatusLine,
    /// Why it shows no developer artifact, when it shows none.
    pub empty: Option<String>,
}

/// A file artifact's root as a tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub artifact: String,
    pub root: String,
    /// Folders first, then files, each by name; `Err`: why it did not list.
    pub rows: Result<Vec<TreeRow>, String>,
}

/// One row of the explorer.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    /// 0 at the root.
    pub depth: usize,
    pub name: String,
    /// From the root; a folder's without a trailing `/`.
    pub path: String,
    pub dir: bool,
    /// A file: the engine has it open. A folder: it holds an open file.
    pub open: bool,
    /// A file: it has unsaved edits. A folder: it holds such a file.
    pub unsaved: bool,
}

/// One editor tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub artifact: String,
    pub path: String,
    /// The file name.
    pub name: String,
    pub language: &'static str,
    pub focus: Option<(usize, usize)>,
    pub unsaved: usize,
    /// The file as it is now with its unsaved edits laid over.
    pub lines: Vec<FileLine>,
    /// Why there are no lines, or that some were left out.
    pub note: Option<String>,
}

impl Tab {
    /// Its key among the tabs (two artifacts may open the same path).
    pub fn key(&self) -> String {
        format!("{}:{}", self.artifact, self.path)
    }
}

/// What a session is, read from its command: a person looks for the
/// servers and the test runs first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Server,
    Test,
    Build,
    Command,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Server => "server",
            Role::Test => "test",
            Role::Build => "build",
            Role::Command => "command",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Role::Server => "◉",
            Role::Test => "⚗",
            Role::Build => "⚒",
            Role::Command => "❯",
        }
    }

    /// From a command and how it was started: a test word wins, then a
    /// build word; a server word, or anything left running in the
    /// background, is a server.
    pub fn of(command: &str, mode: &str, running: bool) -> Role {
        let c = command.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| c.split(|ch: char| !ch.is_alphanumeric() && ch != '-').any(|part| part == *w));
        if has(&["test", "tests", "pytest", "jest", "vitest", "nextest", "rspec", "playwright"]) {
            Role::Test
        } else if has(&["build", "check", "clippy", "compile", "tsc", "fmt", "lint"]) {
            Role::Build
        } else if has(&["serve", "server", "dev", "watch", "watchexec", "http", "preview", "start", "up"]) || (running && mode != "run") {
            Role::Server
        } else {
            Role::Command
        }
    }
}

/// One terminal session in the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub artifact: String,
    pub role: Role,
    /// The command's first line.
    pub title: String,
    pub view: SessionView,
}

impl Session {
    pub fn key(&self) -> String {
        format!("{}:{}", self.artifact, self.view.id)
    }
}

/// The status strip.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusLine {
    pub engine: String,
    /// The activity's label (`idle`, `running tools: open`).
    pub activity: String,
    /// Its tint token; `None` when idle.
    pub tint: Option<&'static str>,
    /// The model, or the session's program.
    pub model: String,
    /// `(share of the window, prompt ≈ x of y tokens)`.
    pub gauge: Option<(f64, String)>,
    /// `3 open · 1 unsaved · 1 running`.
    pub counts: String,
}

/// Build engine `e`'s live view from its exact view (`artifacts`, from
/// `crate::inspect::artifacts`): one artifact's (`scope`) or all of its
/// developer artifacts'. `gauge`: the prompt against the context window,
/// as the engine page shows it.
pub fn build(e: &Engine, artifacts: &[ArtifactView], scope: Option<&str>, activity: &Activity, gauge: Option<(f64, String)>) -> LiveView {
    let shown: Vec<&ArtifactView> = artifacts
        .iter()
        .filter(|v| v.kind.is_some_and(|k| !k.view().is_empty()))
        .filter(|v| scope.is_none_or(|s| v.name == s))
        .collect();
    let mut roots = Vec::new();
    let mut tabs = Vec::new();
    let mut sessions = Vec::new();
    let mut broken = Vec::new();
    for v in &shown {
        match &v.detail {
            Detail::File(f) => {
                let unsaved: Vec<&str> = f.open.iter().filter(|o| o.unsaved > 0).map(|o| o.path.as_str()).collect();
                let open: Vec<&str> = f.open.iter().map(|o| o.path.as_str()).collect();
                roots.push(Root {
                    artifact: v.name.clone(),
                    root: f.root.clone(),
                    rows: f.files.as_ref().map(|names| tree(names.iter().map(|(n, _)| n.as_str()), &open, &unsaved)).map_err(Clone::clone),
                });
                tabs.extend(f.open.iter().map(|o| Tab {
                    artifact: v.name.clone(),
                    name: o.path.rsplit('/').next().unwrap_or(&o.path).to_string(),
                    language: crate::workbench::language(&o.path),
                    path: o.path.clone(),
                    focus: o.focus,
                    unsaved: o.unsaved,
                    lines: o.lines.clone(),
                    note: o.note.clone(),
                }));
            }
            Detail::Terminal { sessions: list, .. } => sessions.extend(list.iter().map(|s| Session {
                artifact: v.name.clone(),
                role: Role::of(&s.command, &s.mode, s.running),
                title: s.command.lines().next().unwrap_or("").to_string(),
                view: s.clone(),
            })),
            Detail::Broken(why) => broken.push(format!("{} does not read: {why}", v.name)),
            _ => {}
        }
    }
    // Running first; each artifact's own order (newest first) kept.
    sessions.sort_by_key(|s| !s.view.running);

    let mut parts: Vec<Part> = shown.iter().filter_map(|v| v.kind).flat_map(|k| k.view().iter().copied()).chain(ALWAYS).collect();
    parts.sort();
    parts.dedup();

    let empty = match scope {
        _ if !broken.is_empty() => Some(broken.join("; ")),
        _ if !shown.is_empty() => None,
        Some(name) => Some(match artifacts.iter().find(|v| v.name == name) {
            None => format!("{} holds no artifact \"{name}\"", e.name),
            Some(v) => format!("a {} artifact attaches no live view: its page shows it", v.kind.map_or("text", |k| k.as_str())),
        }),
        None => Some(format!("{} holds no file or terminal artifact", e.name)),
    };

    let unsaved: usize = tabs.iter().map(|t| t.unsaved).sum();
    let running = sessions.iter().filter(|s| s.view.running).count();
    let mut counts = Vec::new();
    if parts.contains(&Part::Editor) {
        counts.push(crate::inspect::count(tabs.len(), "open", "open"));
        if unsaved > 0 {
            counts.push(format!("{unsaved} unsaved"));
        }
    }
    if parts.contains(&Part::Panel) {
        counts.push(format!("{running} running"));
    }
    let model = match &e.session {
        Some(s) => format!("{} session", s.tool),
        None => e.inference.model.clone(),
    };
    LiveView {
        scope: scope.map(str::to_string),
        parts,
        roots,
        tabs,
        sessions,
        status: StatusLine {
            engine: e.name.clone(),
            activity: activity.label(),
            tint: activity.token(),
            model,
            gauge: gauge.filter(|_| e.session.is_none()),
            counts: counts.join(" · "),
        },
        empty,
    }
}

/// Paths under a root as a tree: folders first, then files, each by
/// name; open and unsaved marks on files and on the folders that hold
/// them.
pub fn tree<'a>(paths: impl Iterator<Item = &'a str>, open: &[&str], unsaved: &[&str]) -> Vec<TreeRow> {
    #[derive(Default)]
    struct Node {
        dirs: BTreeMap<String, Node>,
        files: Vec<String>,
    }
    let mut top = Node::default();
    for p in paths {
        let mut parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
        let Some(file) = parts.pop() else { continue };
        let mut node = &mut top;
        for d in parts {
            node = node.dirs.entry(d.to_string()).or_default();
        }
        node.files.push(file.to_string());
    }
    fn walk(node: &Node, prefix: &str, depth: usize, open: &[&str], unsaved: &[&str], out: &mut Vec<TreeRow>) {
        let under = |dir: &str, list: &[&str]| list.iter().any(|p| p.starts_with(&format!("{dir}/")));
        for (name, child) in &node.dirs {
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            out.push(TreeRow { depth, name: name.clone(), open: under(&path, open), unsaved: under(&path, unsaved), dir: true, path: path.clone() });
            walk(child, &path, depth + 1, open, unsaved, out);
        }
        let mut files = node.files.clone();
        files.sort();
        for name in files {
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            out.push(TreeRow { depth, open: open.contains(&path.as_str()), unsaved: unsaved.contains(&path.as_str()), dir: false, name, path });
        }
    }
    let mut out = Vec::new();
    walk(&top, "", 0, open, unsaved, &mut out);
    out
}

/// The rows a person sees: a folder is open when it holds an open file,
/// unless `flipped` says the person closed it (or opened a closed one);
/// the insides of a closed folder are left out.
pub fn visible<'a>(rows: &'a [TreeRow], flipped: &dyn Fn(&str) -> bool) -> Vec<(&'a TreeRow, bool)> {
    let mut out = Vec::new();
    let mut closed_at: Option<usize> = None;
    for r in rows {
        if let Some(d) = closed_at {
            if r.depth > d {
                continue;
            }
            closed_at = None;
        }
        let expanded = r.dir && (r.open != flipped(&r.path));
        if r.dir && !expanded {
            closed_at = Some(r.depth);
        }
        out.push((r, expanded));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::tests::FakeDisk;
    use crate::inference::OpenAi;
    use crate::live::Snapshots;

    fn session(id: u32, mode: &str, command: &str, running: bool) -> SessionView {
        SessionView {
            id,
            mode: mode.into(),
            command: command.into(),
            running,
            status: if running { "running".into() } else { "exit 0".into() },
            exit: (!running).then_some(0),
            age: "1m ago".into(),
            tail: "ok".into(),
        }
    }

    fn view(name: &str, kind: Live, detail: Detail) -> ArtifactView {
        ArtifactView {
            name: name.into(),
            kind: Some(kind),
            meaning: String::new(),
            tools: Vec::new(),
            state: Vec::new(),
            snapshot: String::new(),
            not_sent: None,
            summary: String::new(),
            detail,
        }
    }

    #[test]
    fn the_developer_kinds_attach_their_parts_and_the_others_none() {
        assert_eq!(Live::File.view(), [Part::Explorer, Part::Editor]);
        assert_eq!(Live::Terminal.view(), [Part::Panel]);
        for k in [Live::Clock, Live::Scripts, Live::Alerts, Live::Engines, Live::Shared, Live::Learning] {
            assert!(k.view().is_empty(), "{k:?}");
        }
        for p in Part::ALL {
            assert_eq!(Part::parse(p.as_str()), Some(p));
        }
        assert_eq!(path(2, None), "/desk/2/live");
        assert_eq!(path(2, Some("code")), "/desk/2/artifacts/code/live");
    }

    #[test]
    fn paths_become_a_tree_with_folders_first_and_marks_carried_up() {
        let names = ["README.md", "src/main.rs", "src/ui/view.rs", "Cargo.toml", "docs/a.md"];
        let rows = tree(names.into_iter(), &["src/ui/view.rs", "README.md"], &["src/ui/view.rs"]);
        let shown: Vec<(usize, &str, bool, bool)> = rows.iter().map(|r| (r.depth, r.path.as_str(), r.open, r.unsaved)).collect();
        assert_eq!(
            shown,
            [
                (0, "docs", false, false),
                (1, "docs/a.md", false, false),
                (0, "src", true, true),
                (1, "src/ui", true, true),
                (2, "src/ui/view.rs", true, true),
                (1, "src/main.rs", false, false),
                (0, "Cargo.toml", false, false),
                (0, "README.md", true, false),
            ]
        );
        // Folders holding an open file are open; others closed; a flip turns one.
        let seen: Vec<&str> = visible(&rows, &|_| false).iter().map(|(r, _)| r.path.as_str()).collect();
        assert_eq!(seen, ["docs", "src", "src/ui", "src/ui/view.rs", "src/main.rs", "Cargo.toml", "README.md"]);
        let seen: Vec<&str> = visible(&rows, &|p| p == "src" || p == "docs").iter().map(|(r, _)| r.path.as_str()).collect();
        assert_eq!(seen, ["docs", "docs/a.md", "src", "Cargo.toml", "README.md"]);
    }

    #[test]
    fn sessions_read_as_servers_tests_and_builds() {
        assert_eq!(Role::of("cargo test -q", "run", false), Role::Test);
        assert_eq!(Role::of("npm run dev", "background", true), Role::Server);
        assert_eq!(Role::of("python3 -m http.server 8000", "background", true), Role::Server);
        assert_eq!(Role::of("cargo clippy --all-targets", "run", false), Role::Build);
        assert_eq!(Role::of("./bin/thing", "background", true), Role::Server, "left running in the background");
        assert_eq!(Role::of("ls -la", "run", false), Role::Command);
        assert_eq!(Role::of("latest-version", "run", false), Role::Command, "a word inside another is not one");
    }

    #[test]
    fn an_engine_with_both_shows_one_workbench_from_its_exact_view() {
        let mut e = Engine::new("Builder", "", OpenAi::default());
        e.inference.model = "qwen3".into();
        e.artifacts.insert(
            "code".into(),
            "kind: file\nroot: .\nopen: src/a.rs 2-3\nopen: README.md\nedit: README.md\n~~~ old\n# a\n~~~ new\n# b\n~~~".into(),
        );
        e.artifacts.insert("plan".into(), "1. read".into());
        let disk = FakeDisk::with(&[("src/a.rs", "fn a() {}\nfn b() {}\nfn c() {}\n"), ("README.md", "# a\n"), (".env", "KEY=1")]);
        let at = Snapshots { clock: None, host: Some(&disk), family: None };
        let mut views = crate::inspect::artifacts(&e, &at);
        views.push(view(
            "term",
            Live::Terminal,
            Detail::Terminal {
                root: ".".into(),
                sessions: vec![session(2, "background", "npm run dev", true), session(1, "run", "cargo test -q", false)],
            },
        ));
        let busy = Activity::Tools { step: 1, calls: vec!["open".into()] };
        let lv = build(&e, &views, None, &busy, Some((0.25, "prompt ≈ 2k of 8k tokens".into())));
        assert_eq!(lv.parts, Part::ALL, "file + terminal + the chat and status every view has");
        assert_eq!(lv.empty, None);
        let tabs = lv.tabs.iter().map(|t| (t.path.as_str(), t.focus, t.unsaved));
        assert!(tabs.eq([("src/a.rs", Some((2, 3)), 0), ("README.md", None, 1)]));
        assert_eq!(lv.tabs[1].lines[0].text, "# b", "the file as it is now, with its unsaved edit");
        assert!(lv.tabs[1].lines[0].changed);
        assert_eq!(lv.tabs[0].language, crate::workbench::language("a.rs"));
        let rows = lv.roots[0].rows.as_ref().unwrap();
        assert!(rows.iter().all(|r| r.name != ".env"), "a secret-looking file is never listed");
        assert!(rows.iter().any(|r| r.path == "README.md" && r.open && r.unsaved));
        assert_eq!((lv.sessions[0].role, lv.sessions[0].view.running), (Role::Server, true), "running first");
        assert_eq!(lv.sessions[1].role, Role::Test);
        assert_eq!(lv.status.engine, "Builder");
        assert_eq!(lv.status.model, "qwen3");
        assert_eq!(lv.status.tint, Some("activity_tools"));
        assert_eq!(lv.status.activity, "running tools: open");
        assert_eq!(lv.status.counts, "2 open · 1 unsaved · 1 running");
        assert_eq!(lv.status.gauge.as_ref().map(|g| g.0), Some(0.25));
    }

    #[test]
    fn one_artifacts_view_shows_only_its_parts_and_says_why_when_it_has_none() {
        let e = Engine::new("E", "", OpenAi::default());
        let views = vec![
            view("term", Live::Terminal, Detail::Terminal { root: ".".into(), sessions: vec![session(1, "run", "ls", false)] }),
            view("kids", Live::Engines, Detail::Engines(None)),
        ];
        let lv = build(&e, &views, Some("term"), &Activity::Idle, None);
        assert_eq!(lv.parts, [Part::Panel, Part::Chat, Part::Status]);
        assert_eq!((lv.tabs.len(), lv.sessions.len(), lv.status.tint), (0, 1, None));
        assert_eq!(lv.status.counts, "0 running");
        let none = build(&e, &views, Some("kids"), &Activity::Idle, None);
        assert_eq!(none.parts, ALWAYS);
        assert!(none.empty.as_deref().is_some_and(|w| w.contains("engines artifact attaches no live view")), "{:?}", none.empty);
        assert!(build(&e, &views, Some("gone"), &Activity::Idle, None).empty.unwrap().contains("no artifact \"gone\""));
        assert!(build(&e, &views[1..], None, &Activity::Idle, None).empty.unwrap().contains("no file or terminal artifact"));
    }
}
