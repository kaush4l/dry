//! The exact view: what the engine sees, laid out for a person.
//!
//! The `/code` page is the human view of the developer artifacts (two or
//! three files at a time, not a copy). The engine page carries the exact
//! one, built here from the same render the turn sends:
//!
//! ```text
//! artifact   kind and meaning · its tools as described · its state parsed · its snapshot as sent
//! prompt     instructions, learning, phase, tools, artifacts, history, response: each with its size,
//!            or why it is empty
//! ```
//!
//! Pure. The shell passes the same `Snapshots` (its disk) the turn uses
//! and keeps the result until the next step or a Refresh, so the disk is
//! not read every frame.

use crate::context::{message_tokens, tokens};
use crate::live::{Live, Snapshots};
use crate::prompt::{self, Rendered};
use crate::state::Engine;
use crate::template::{self, Slot};

/// One artifact as the engine sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactView {
    pub name: String,
    /// `None`: a plain text artifact.
    pub kind: Option<Live>,
    /// The line the model is told about it (a plain one: what it is).
    pub meaning: String,
    /// The tools it brings, each as the Tools slot describes it.
    pub tools: Vec<String>,
    /// Its state, `(what, value)`: the artifact's file parsed.
    pub state: Vec<(String, String)>,
    /// Its section of the artifacts slot, exactly as sent.
    pub snapshot: String,
    /// Why the snapshot is not in the prompt now, when it is not.
    pub not_sent: Option<String>,
    /// Its state in a few words, for its card: `3 open files`,
    /// `1 running, 2 done`, `4 posts`.
    pub summary: String,
    /// Its state laid out by kind, for its own page.
    pub detail: Detail,
}

/// An artifact's state, laid out for its page by kind. Read with the
/// same disk and family as the snapshot, so the two agree.
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    /// A plain artifact, the clock, scripts, alerts: shown as text.
    Text(String),
    File(FileDetail),
    /// Its sessions, the running ones first, then the newest done.
    Terminal { root: String, sessions: Vec<SessionView> },
    /// The level's posts, oldest first. `None`: taken without the family.
    Shared(Option<Vec<PostView>>),
    /// Its three sections with their fill, and what it collects now.
    Learning { sections: Vec<SectionFill>, collecting: String },
    /// Its children. `None`: taken without the family.
    Engines(Option<Vec<ChildView>>),
    /// The project's evidence: its shots, changed ones first.
    Evidence(EvidenceDetail),
    /// Its state did not read: why.
    Broken(String),
}

/// An evidence artifact: the last run's verdict and its shots.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceDetail {
    pub root: String,
    pub command: String,
    /// The one line (`12 shots · 0 changed · pass`), or why there is none.
    pub line: String,
    /// The commit and when it ran (unix seconds); none: no run yet.
    pub commit: Option<(String, i64)>,
    pub shots: Vec<crate::evidence::artifact::ShotView>,
}

fn evidence_detail(s: &crate::evidence::artifact::State, host: Option<&dyn crate::live::Host>) -> EvidenceDetail {
    let (line, commit, shots) = match host.map(|h| crate::evidence::artifact::manifest(&s.root, h)) {
        None => ("read with the disk, when the prompt is sent".to_string(), None, Vec::new()),
        Some(Err(why)) => (why, None, Vec::new()),
        Some(Ok(m)) => (m.line(), Some((m.commit.clone(), m.time)), crate::evidence::artifact::shots(&m)),
    };
    EvidenceDetail { root: s.root.clone(), command: s.command.clone(), line, commit, shots }
}

/// A file artifact: the names under its root, and its open files.
#[derive(Clone, Debug, PartialEq)]
pub struct FileDetail {
    pub root: String,
    /// `(path, open)`; `Err`: why the root could not be listed (or that
    /// it is read with the disk).
    pub files: Result<Vec<(String, bool)>, String>,
    pub open: Vec<OpenFile>,
}

/// One open file as the engine sees it: disk text with its unsaved edits
/// laid over, each line numbered.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenFile {
    pub path: String,
    /// The lines the engine looks at; `None`: the whole file.
    pub focus: Option<(usize, usize)>,
    /// How many unsaved edits it has.
    pub unsaved: usize,
    pub lines: Vec<FileLine>,
    /// Why there are no lines, or that some were left out.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileLine {
    /// From 1.
    pub n: usize,
    pub text: String,
    /// Changed by an unsaved edit.
    pub changed: bool,
    /// Inside the focus range (every line, without a focus).
    pub focused: bool,
}

/// One terminal session.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionView {
    pub id: u32,
    /// `run`, `start`, `background`.
    pub mode: String,
    pub command: String,
    pub running: bool,
    /// `running`, `exit 0`, `no exit code (stopped)`.
    pub status: String,
    pub exit: Option<i32>,
    /// `3m ago`, or `age unknown` without the disk.
    pub age: String,
    /// The output's last lines (`crate::terminal::TAIL_LINES`).
    pub tail: String,
}

/// One post of a shared context.
#[derive(Clone, Debug, PartialEq)]
pub struct PostView {
    pub author: String,
    pub when: String,
    pub text: String,
}

/// One learning section and how full it is.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionFill {
    pub name: String,
    /// What it holds and when it changes.
    pub holds: String,
    pub used: usize,
    pub cap: usize,
    pub entries: Vec<String>,
}

/// One child engine, as its parent sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChildView {
    pub id: String,
    pub name: String,
    pub goal: String,
    /// Its activity, as a label.
    pub activity: String,
    pub busy: bool,
    pub failed: Option<String>,
    pub replies: usize,
    pub last_reply: Option<String>,
    /// Asked with `engine_call` (the parent waits), else `engine_start`.
    pub call: bool,
}

/// Most lines an open file shows on its page.
pub const PAGE_LINES: usize = 2_000;

/// Every artifact engine `e` holds, in the artifacts slot's order (the
/// skills element is left out: it is no artifact).
pub fn artifacts(e: &Engine, outside: &Snapshots) -> Vec<ArtifactView> {
    let now = outside.host.map(|h| h.now());
    let not_sent = e.response.is_empty().then(|| "no response contract: tools and artifacts are not sent".to_string());
    prompt::artifact_sections(e, outside)
        .into_iter()
        .filter_map(|(name, snapshot)| {
            let name = name?;
            let text = e.artifacts.get(&name).map_or("", String::as_str);
            let kind = Live::of(text);
            // What its capabilities bring here (`crate::capability`).
            let caps = crate::capability::equipped(e).into_iter().find(|q| q.name == name);
            let kind = kind.filter(|_| caps.as_ref().is_some_and(|q| q.live.is_some()));
            let mut tools: Vec<String> = match &caps {
                Some(q) => q.compiled_tools().iter().map(|t| crate::tools::describe(std::slice::from_ref(t))).collect(),
                None => Vec::new(),
            };
            for c in caps.iter().flat_map(|q| q.outside.iter()) {
                tools.extend(c.tools.iter().map(|t| t.describe(&c.call_name(t))));
            }
            let outside_only = caps.as_ref().filter(|q| q.live.is_none());
            let detail = match outside_only {
                Some(_) => Detail::Text(snapshot.split_once("\n\n").map_or("", |(_, b)| b).to_string()),
                None => detail(kind, &name, text, e, outside),
            };
            let meaning = match (kind, outside_only) {
                (Some(k), _) => k.meaning().to_string(),
                (None, Some(q)) => {
                    let ids: Vec<String> = q.compiled.iter().chain(&q.outside).map(|c| format!("{} ({})", c.id, c.provider.text())).collect();
                    format!("capabilities: {}", ids.join(", "))
                }
                (None, None) => "plain text: shown by its text".to_string(),
            };
            let summary = match outside_only {
                Some(q) => count(q.outside.len() + q.compiled.len(), "capability", "capabilities"),
                None => summary(kind, text, e, &detail),
            };
            Some(ArtifactView {
                meaning,
                state: state(kind, text, e, now),
                summary,
                detail,
                tools,
                snapshot,
                not_sent: not_sent.clone(),
                kind,
                name,
            })
        })
        .collect()
}

/// The glyph a kind is shown with (a plain artifact: `¶`).
pub fn icon(kind: Option<Live>) -> &'static str {
    match kind {
        None => "¶",
        Some(Live::Clock) => "◷",
        Some(Live::Scripts) => "≡",
        Some(Live::Alerts) => "⚑",
        Some(Live::File) => "▤",
        Some(Live::Terminal) => "❯",
        Some(Live::Engines) => "◇",
        Some(Live::Shared) => "☷",
        Some(Live::Learning) => "✎",
        Some(Live::Browser) => "◱",
        Some(Live::Evidence) => "◩",
    }
}

/// `1 open file`, `3 open files`: `n` and the word for one or for many.
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// An artifact's state in a few words.
fn summary(kind: Option<Live>, text: &str, e: &Engine, detail: &Detail) -> String {
    if let Detail::Broken(_) = detail {
        return "does not read".into();
    }
    match (kind, detail) {
        (None, _) => count(text.lines().count(), "line", "lines"),
        (Some(Live::Clock), _) => "the time, read at each render".into(),
        (Some(Live::Scripts), _) => count(e.scripts.len(), "script", "scripts"),
        (Some(Live::Alerts), _) => count(e.alerts.len(), "alert", "alerts"),
        (Some(Live::Browser), _) => crate::browser::State::parse(text).ok().and_then(|s| s.url).unwrap_or_else(|| "no page open".into()),
        (_, Detail::Evidence(ev)) => ev.line.clone(),
        (_, Detail::File(f)) => {
            let unsaved: usize = f.open.iter().map(|o| o.unsaved).sum();
            let open = if f.open.is_empty() { "no files open".to_string() } else { count(f.open.len(), "open file", "open files") };
            if unsaved == 0 { open } else { format!("{open}, {unsaved} unsaved") }
        }
        (_, Detail::Terminal { sessions, .. }) => {
            let running = sessions.iter().filter(|s| s.running).count();
            match (running, sessions.len() - running) {
                (0, 0) => "no sessions yet".into(),
                (r, d) => format!("{r} running, {d} done"),
            }
        }
        (_, Detail::Shared(Some(posts))) => if posts.is_empty() { "no posts yet".into() } else { count(posts.len(), "post", "posts") },
        (_, Detail::Shared(None)) => "the level's posts".into(),
        (_, Detail::Learning { sections, .. }) => {
            let entries: usize = sections.iter().map(|s| s.entries.len()).sum();
            let fullest = sections.iter().max_by_key(|s| s.used * 1000 / s.cap.max(1));
            match fullest {
                Some(f) if entries > 0 => format!("{}, fullest {} {}%", count(entries, "entry", "entries"), f.name, f.used * 100 / f.cap.max(1)),
                _ => "nothing learned yet".into(),
            }
        }
        (_, Detail::Engines(Some(children))) => {
            let busy = children.iter().filter(|c| c.busy).count();
            match children.len() {
                0 => "no children yet".into(),
                n if busy > 0 => format!("{}, {busy} busy", count(n, "child", "children")),
                n => count(n, "child", "children"),
            }
        }
        (_, Detail::Engines(None)) => "its children".into(),
        (_, _) => String::new(),
    }
}

/// An artifact's page, by kind.
fn detail(kind: Option<Live>, name: &str, text: &str, e: &Engine, outside: &Snapshots) -> Detail {
    let offset = outside.family.map_or(0, |f| f.offset);
    match kind {
        None => Detail::Text(text.to_string()),
        Some(Live::Clock) => Detail::Text(outside.clock.map_or_else(|| "(read when the prompt is sent)".to_string(), |c| c.to_string())),
        Some(Live::Scripts) => Detail::Text(crate::script::snapshot(&e.scripts)),
        Some(Live::Alerts) => Detail::Text(crate::alert::snapshot(&e.alerts, &e.alert_marks)),
        Some(Live::Evidence) => match crate::evidence::artifact::State::parse(text) {
            Err(why) => Detail::Broken(why),
            Ok(s) => Detail::Evidence(evidence_detail(&s, outside.host)),
        },
        Some(Live::Browser) => match crate::browser::State::parse(text) {
            Err(why) => Detail::Broken(why),
            Ok(_) => Detail::Text(
                outside.host.map_or_else(|| "(the page is read when the prompt is sent)".to_string(), |h| crate::browser::snapshot(&e.id, name, text, h)),
            ),
        },
        Some(Live::File) => match crate::file::State::parse(text) {
            Err(why) => Detail::Broken(why),
            Ok(s) => Detail::File(file_detail(&s, outside.host)),
        },
        Some(Live::Terminal) => match crate::terminal::State::parse(text) {
            Err(why) => Detail::Broken(why),
            Ok(s) => Detail::Terminal { sessions: sessions(&e.id, name, &s, outside.host), root: s.root },
        },
        Some(Live::Shared) => Detail::Shared(outside.family.map(|f| {
            crate::shared::parse(&f.shared)
                .into_iter()
                .map(|p| PostView { when: crate::shared::when(p.at, offset), author: p.author, text: p.text })
                .collect()
        })),
        Some(Live::Learning) => {
            let l = crate::learning::Learning::parse(&e.learning);
            let sections = crate::learning::SECTIONS
                .iter()
                .enumerate()
                .map(|(i, (n, holds, cap))| SectionFill {
                    name: n.to_string(),
                    holds: holds.to_string(),
                    used: l.used(i),
                    cap: *cap,
                    entries: l.entries[i].clone(),
                })
                .collect();
            Detail::Learning { sections, collecting: crate::learning::collecting(text).unwrap_or("nothing").to_string() }
        }
        Some(Live::Engines) => {
            let lines = crate::engines::parse(text);
            Detail::Engines(outside.family.map(|f| {
                f.children
                    .iter()
                    .map(|c| ChildView {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        goal: c.goal.clone(),
                        activity: c.seen.label.clone(),
                        busy: c.seen.busy,
                        failed: c.seen.failed.clone(),
                        replies: c.replies,
                        last_reply: c.last_reply.clone(),
                        call: lines.iter().any(|l| l.id == c.id && l.call),
                    })
                    .collect()
            }))
        }
    }
}

/// A file artifact's names and open files, read through `host`.
fn file_detail(s: &crate::file::State, host: Option<&dyn crate::live::Host>) -> FileDetail {
    use crate::live::Content;
    let Some(host) = host else {
        let open = s
            .open
            .iter()
            .map(|o| OpenFile {
                path: o.path.clone(),
                focus: o.focus,
                unsaved: s.edits.iter().filter(|x| x.path == o.path).count(),
                lines: Vec::new(),
                note: Some("read with the disk when the view is taken".into()),
            })
            .collect();
        return FileDetail { root: s.root.clone(), files: Err("read with the disk when the view is taken".into()), open };
    };
    let files = host.list(&s.root, crate::file::MAX_NAMES).map(|names| {
        names.into_iter().filter(|n| !crate::file::secret_file(n)).map(|n| {
            let open = s.open.iter().any(|o| o.path == n);
            (n, open)
        }).collect()
    });
    let open = s
        .open
        .iter()
        .map(|o| {
            let edits: Vec<&crate::file::Edit> = s.edits.iter().filter(|x| x.path == o.path).collect();
            let mut file = OpenFile { path: o.path.clone(), focus: o.focus, unsaved: edits.len(), lines: Vec::new(), note: None };
            let disk = match host.read(&s.root, &o.path) {
                Content::Text(t) => t,
                Content::Binary { bytes, image } => {
                    file.note = Some(format!("{}, {} KB: not shown", if image { "an image" } else { "not text" }, bytes / 1000));
                    return file;
                }
                Content::TooLarge(b) => {
                    file.note = Some(format!("too large to show ({} KB)", b / 1000));
                    return file;
                }
                // A file being made: its text is its unsaved edits alone.
                Content::Missing(_) if crate::file::makes_new(&edits) => String::new(),
                Content::Missing(why) => {
                    file.note = Some(format!("not readable now: {why}"));
                    return file;
                }
            };
            let (shown, changed) = match crate::file::apply(&disk, &edits) {
                Ok(x) => x,
                Err(n) => {
                    file.note = Some(format!("unsaved edit {n} no longer applies (the file changed on disk); shown without the edits"));
                    let marks = vec![false; disk.split('\n').count()];
                    (disk, marks)
                }
            };
            let total = shown.lines().count();
            file.lines = shown
                .lines()
                .enumerate()
                .take(PAGE_LINES)
                .map(|(i, line)| FileLine {
                    n: i + 1,
                    text: line.to_string(),
                    changed: changed.get(i).copied().unwrap_or(false),
                    focused: o.focus.is_none_or(|(a, b)| (a..=b).contains(&(i + 1))),
                })
                .collect();
            if total > PAGE_LINES {
                file.note = Some(format!("the first {PAGE_LINES} of {total} lines shown"));
            }
            file
        })
        .collect();
    FileDetail { root: s.root.clone(), files, open }
}

/// A terminal artifact's sessions: running first, then the newest done.
fn sessions(engine: &str, artifact: &str, s: &crate::terminal::State, host: Option<&dyn crate::live::Host>) -> Vec<SessionView> {
    use crate::terminal::Status;
    let now = host.map(|h| h.now());
    let mut out: Vec<SessionView> = s
        .sessions
        .iter()
        .map(|x| {
            let job = crate::live::Job { engine, artifact, session: x.id, root: &s.root, command: &x.command };
            let tail = host.map_or_else(
                || "(read with the disk when the view is taken)".to_string(),
                |h| {
                    let t = h.tail(&job, crate::terminal::TAIL_LINES);
                    if t.trim().is_empty() { "(no output yet)".to_string() } else { crate::terminal::last_lines(&t, crate::terminal::TAIL_LINES) }
                },
            );
            SessionView {
                id: x.id,
                mode: x.mode.as_str().to_string(),
                command: x.command.clone(),
                running: x.status == Status::Running,
                status: x.status.text(),
                exit: match x.status {
                    Status::Exit(c) => Some(c),
                    _ => None,
                },
                age: now.map_or_else(|| "age unknown".to_string(), |n| format!("{} ago", ago(n - x.started))),
                tail,
            }
        })
        .collect();
    // Running first; within each, the newest first.
    out.sort_by_key(|x| (!x.running, std::cmp::Reverse(x.id)));
    out
}

/// Where a tool an engine may call comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Its own list (`spec/tools.md`).
    BuiltIn,
    /// A live artifact of this kind brings it.
    Artifact(Live),
    /// The current phase of its plan.
    Phase,
    /// A skill: `skill` itself, or a loaded skill's tools.
    Skill,
    /// An MCP server, by name.
    Mcp(String),
    /// A declared or remote capability (`crate::capability`), by id and provider.
    Capability(String),
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::BuiltIn => "Built-in".into(),
            Source::Artifact(k) => format!("Artifact · {}", k.as_str()),
            Source::Phase => "Phase".into(),
            Source::Skill => "Skill".into(),
            Source::Mcp(server) => format!("MCP · {server}"),
            Source::Capability(c) => format!("Capability · {c}"),
        }
    }
}

/// One tool, as the model is told it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolLine {
    pub name: String,
    /// `open(path, from, to)`.
    pub call: String,
    /// Its description's first line.
    pub description: String,
    /// `(argument, what it is)`.
    pub args: Vec<(String, String)>,
}

/// The tools of one source.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolGroup {
    pub source: Source,
    pub tools: Vec<ToolLine>,
}

/// Every tool engine `e` may call now, grouped by where it comes from, in
/// the toolbox's order (`crate::live::toolbox`), then its MCP tools. A
/// tool two sources bring is in the first one's group only.
pub fn tool_groups(e: &Engine) -> Vec<ToolGroup> {
    let mut groups: Vec<ToolGroup> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut add = |source: Source, line: ToolLine, groups: &mut Vec<ToolGroup>| {
        if seen.contains(&line.name) {
            return;
        }
        seen.push(line.name.clone());
        match groups.iter_mut().find(|g| g.source == source) {
            Some(g) => g.tools.push(line),
            None => groups.push(ToolGroup { source, tools: vec![line] }),
        }
    };
    let skills = e.visible_skills();
    let mut sourced: Vec<(Source, String)> = e.tools.iter().map(|t| (Source::BuiltIn, t.clone())).collect();
    if !skills.is_empty() {
        sourced.push((Source::Skill, crate::skill::LOAD.to_string()));
    }
    for (_, text) in skills.iter().filter(|(n, _)| e.loaded_skills.iter().any(|l| l == n)) {
        sourced.extend(crate::skill::parse(text).map(|s| s.tools).unwrap_or_default().into_iter().map(|t| (Source::Skill, t)));
    }
    sourced.extend(e.run.tools(&e.phase_list().unwrap_or_default()).into_iter().map(|t| (Source::Phase, t)));
    // What each artifact offers: not the writing tools of one in read mode.
    let mut kinds: Vec<(Live, Vec<String>)> =
        crate::capability::equipped(e).into_iter().filter_map(|q| q.live.map(|l| (l, q.compiled_tools()))).collect();
    kinds.sort_by_key(|(l, _)| (*l == Live::Clock, *l));
    for (kind, tools) in kinds {
        sourced.extend(tools.into_iter().filter(|t| kind.tools().contains(&t.as_str())).map(|t| (Source::Artifact(kind), t)));
    }
    for (source, name) in sourced {
        add(source, tool_line(&name), &mut groups);
    }
    for q in crate::capability::equipped(e) {
        for c in q.outside {
            for t in &c.tools {
                let name = c.call_name(t);
                let line = ToolLine {
                    call: format!("{name}({})", t.inputs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(", ")),
                    description: t.doc.clone(),
                    args: t.inputs.iter().map(|i| (i.name.clone(), i.doc.clone())).collect(),
                    name,
                };
                add(Source::Capability(format!("{} ({})", c.id, c.provider.text())), line, &mut groups);
            }
        }
    }
    for t in &e.mcp_tools {
        let props = t.schema.get("properties").and_then(serde_json::Value::as_object);
        let args: Vec<(String, String)> = props
            .map(|p| p.iter().map(|(k, v)| (k.clone(), v.get("description").and_then(serde_json::Value::as_str).unwrap_or("").to_string())).collect())
            .unwrap_or_default();
        let line = ToolLine {
            name: t.call_name(),
            call: format!("{}({})", t.call_name(), args.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>().join(", ")),
            description: t.description.lines().next().unwrap_or("").trim().to_string(),
            args,
        };
        add(Source::Mcp(t.server.clone()), line, &mut groups);
    }
    groups
}

/// A tool by name, as its spec describes it (`crate::tools`).
fn tool_line(name: &str) -> ToolLine {
    match crate::tools::BUILTIN.iter().chain(crate::tools::BROUGHT).find(|t| t.name == name) {
        Some(t) => ToolLine {
            name: name.to_string(),
            call: format!("{name}({})", t.args.iter().map(|(a, _)| *a).collect::<Vec<_>>().join(", ")),
            description: t.description.lines().next().unwrap_or("").to_string(),
            args: t.args.iter().map(|(a, d)| (a.to_string(), d.to_string())).collect(),
        },
        None => ToolLine { name: name.to_string(), call: format!("{name}()"), description: "(no description: not a known tool)".into(), args: Vec::new() },
    }
}

/// An artifact's state rows.
fn state(kind: Option<Live>, text: &str, e: &Engine, now: Option<i64>) -> Vec<(String, String)> {
    let row = |k: &str, v: String| (k.to_string(), v);
    match kind {
        None => vec![row("size", format!("{} characters, {} lines", text.chars().count(), text.lines().count()))],
        Some(Live::Clock) => vec![row("state", "none: the time is read at the render".into())],
        Some(Live::Scripts) => vec![row("scripts", e.scripts.len().to_string())],
        Some(Live::Alerts) => vec![row("alerts", e.alerts.len().to_string())],
        Some(Live::Evidence) => match crate::evidence::artifact::State::parse(text) {
            Err(why) => vec![row("state", format!("did not read: {why}"))],
            Ok(s) => vec![row("root", s.root), row("command", s.command), row("folder", format!("{}/", crate::evidence::artifact::FOLDER))],
        },
        Some(Live::Browser) => match crate::browser::State::parse(text) {
            Err(why) => vec![row("state", format!("did not read: {why}"))],
            Ok(s) => vec![row("url", s.url.unwrap_or_else(|| "none".into())), row("screenshot", s.shot.unwrap_or_else(|| "none".into()))],
        },
        Some(Live::Engines) => {
            let lines = crate::engines::parse(text);
            let mut rows: Vec<(String, String)> = lines
                .iter()
                .map(|l| (format!("child {}", l.id), format!("{} · {} replies had{}", if l.call { "call" } else { "start" }, l.seen, if e.waiting.contains(&l.id) { " · waited for" } else { "" })))
                .collect();
            if rows.is_empty() {
                rows.push(row("children", "none".into()));
            }
            rows
        }
        Some(Live::Shared) => vec![row("state", "its thread: its root's engines/<root>/shared.md".to_string())],
        Some(Live::Learning) => {
            let l = crate::learning::Learning::parse(&e.learning);
            let mut rows: Vec<(String, String)> = l.room().into_iter().map(|r| ("room".to_string(), r)).collect();
            rows.push(row("collecting", crate::learning::collecting(text).unwrap_or("nothing").to_string()));
            rows.push(row("log", format!("{} changes", e.learning_log.lines().count())));
            rows
        }
        Some(Live::File) => match crate::file::State::parse(text) {
            Err(why) => vec![row("state", format!("did not read: {why}"))],
            Ok(s) => {
                let mut rows = vec![row("root", s.root.clone())];
                if s.open.is_empty() {
                    rows.push(row("open", "none".into()));
                }
                for o in &s.open {
                    let focus = o.focus.map_or_else(|| "whole file".to_string(), |(a, b)| format!("lines {a}-{b}"));
                    rows.push(row("open", format!("{} · {focus}", o.path)));
                }
                for x in &s.edits {
                    let n = |t: &str| t.split('\n').count();
                    rows.push(row("unsaved", format!("{} · {} line(s) → {} line(s)", x.path, n(&x.old), n(&x.new))));
                }
                rows
            }
        },
        Some(Live::Terminal) => match crate::terminal::State::parse(text) {
            Err(why) => vec![row("state", format!("did not read: {why}"))],
            Ok(s) => {
                let mut rows = vec![row("root", s.root.clone())];
                if s.sessions.is_empty() {
                    rows.push(row("sessions", "none".into()));
                }
                for x in &s.sessions {
                    let age = now.map_or_else(|| "age unknown".to_string(), |n| format!("{} ago", ago(n - x.started)));
                    let one = x.command.split('\n').next().unwrap_or("");
                    rows.push((format!("session {}", x.id), format!("{} · {} · {age} · {one}", x.mode.as_str(), x.status.text())));
                }
                rows
            }
        },
    }
}

/// `45s`, `3m`, `2h 5m`, `3d`.
pub fn ago(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d", s / 86_400),
    }
}

/// One slot of the rendered prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotView {
    /// Its title as sent (the template may rename it).
    pub title: String,
    /// What is sent in it; empty when nothing is.
    pub text: String,
    /// Its estimated tokens (`crate::context::tokens`).
    pub tokens: usize,
    /// Why nothing is sent in it.
    pub empty: Option<String>,
}

/// Rendered prompt `r` of engine `e`, slot by slot in the standard's
/// order, then notes on how it was read (`(what, note)`: a broken
/// template, a phase answered another way).
pub fn slots(e: &Engine, r: &Rendered) -> (Vec<SlotView>, Vec<(String, String)>) {
    let t = e.template().unwrap_or_else(|_| template::parse("").expect("the default template reads"));
    let mut out = Vec::new();
    let mut used = Vec::new();
    for s in [Slot::Instructions, Slot::Learning, Slot::Phase, Slot::Tools, Slot::Artifacts] {
        let title = t.title(s);
        let sent = r.system.iter().find(|(n, _)| *n == title).map(|(_, text)| text.clone());
        let empty = r.empty.iter().find(|(n, _)| *n == title).map(|(_, why)| why.clone());
        used.push(title.clone());
        let empty = match (&sent, empty) {
            (Some(_), _) => None,
            (None, Some(why)) => Some(why),
            (None, None) => Some("nothing in it".into()),
        };
        let text = sent.unwrap_or_default();
        out.push(SlotView { tokens: if text.is_empty() { 0 } else { tokens(&text) }, title, text, empty });
    }
    // History: the newest summary (sent at the end of the system prompt),
    // then the messages after it.
    let mut parts = Vec::new();
    let mut size = 0;
    if let Some(s) = &r.summary {
        parts.push(format!("[{}]\n{s}", prompt::SUMMARY_TITLE));
        size += tokens(s);
    }
    for m in &r.history {
        parts.push(format!("[{:?}]\n{}", m.role, m.text));
        size += message_tokens(m);
    }
    out.push(SlotView {
        title: t.title(Slot::History),
        text: parts.join("\n\n"),
        tokens: size,
        empty: parts.is_empty().then(|| "no messages yet".to_string()),
    });
    out.push(SlotView {
        title: r.response_title.clone(),
        tokens: if r.response.is_empty() { 0 } else { tokens(&r.response) },
        empty: r.response.is_empty().then(|| "no response contract: the answer is free text".to_string()),
        text: r.response.clone(),
    });
    let mut notes: Vec<(String, String)> = r.empty.iter().filter(|(n, _)| !used.contains(n)).cloned().collect();
    // Whether this step renders down into choices (`crate::jev`): one
    // line, for every view that lists the notes.
    notes.push(("jev".into(), r.jev.line()));
    (out, notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::tests::FakeDisk;
    use crate::inference::{Message, OpenAi, Role};
    use crate::response::Responses;

    fn engine() -> Engine {
        let mut e = Engine::new("E", "Be brief.", OpenAi::default());
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.messages.push(Message { role: Role::User, text: "fix a".into() });
        e
    }

    #[test]
    fn a_file_artifact_shows_its_tools_state_and_the_snapshot_as_sent() {
        let mut e = engine();
        e.artifacts.insert(
            "code".into(),
            "kind: file\nroot: .\nopen: src/a.rs 1-2\nopen: README.md\nedit: README.md\n~~~ old\n# a\n~~~ new\n# b\n~~~".into(),
        );
        e.artifacts.insert("plan".into(), "1. read".into());
        let disk = FakeDisk::with(&[("src/a.rs", "fn a() {}\n"), ("README.md", "# a\n")]);
        let at = Snapshots { clock: None, host: Some(&disk), family: None };
        let views = artifacts(&e, &at);
        let names: Vec<&str> = views.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["plan", "code"], "the artifacts slot's order: plain first, then kinds");
        let code = &views[1];
        assert_eq!(code.kind, Some(Live::File));
        assert_eq!(code.tools.len(), 4);
        assert!(code.tools[0].starts_with("- open(path=\"...\"): Open a file"), "{:?}", code.tools);
        assert_eq!(
            code.state,
            [
                ("root".to_string(), ".".to_string()),
                ("open".into(), "src/a.rs · lines 1-2".into()),
                ("open".into(), "README.md · whole file".into()),
                ("unsaved".into(), "README.md · 1 line(s) → 1 line(s)".into()),
            ]
        );
        // Exactly what is sent: every section is in the system prompt as is.
        let system = &prompt::render_with(&e, &at).messages()[0].text;
        for v in &views {
            assert!(system.contains(&v.snapshot), "{} is not as sent: {}", v.name, v.snapshot);
        }
        assert!(code.snapshot.contains("fn a() {}"), "read through the host: {}", code.snapshot);
        assert_eq!(views[0].state, [("size".to_string(), "7 characters, 1 lines".to_string())]);
        assert!(views[0].tools.is_empty() && views[0].not_sent.is_none());
    }

    #[test]
    fn a_terminal_artifact_shows_its_sessions_with_status_and_age() {
        let mut e = engine();
        e.artifacts.insert(
            "term".into(),
            "kind: terminal\nroot: .\nsession: 1 run exit:0 100 cargo test -q\nsession: 2 start running 160 cargo build".into(),
        );
        let shell = crate::terminal::tests::FakeShell::default();
        *shell.now.borrow_mut() = 220;
        let v = &artifacts(&e, &Snapshots { clock: None, host: Some(&shell), family: None })[0];
        assert_eq!(v.tools.len(), 3);
        assert_eq!(v.state[1], ("session 1".to_string(), "run · exit 0 · 2m ago · cargo test -q".to_string()));
        assert_eq!(v.state[2], ("session 2".to_string(), "start · running · 1m ago · cargo build".to_string()));
        let shown = &artifacts(&e, &Snapshots::default())[0];
        assert!(shown.snapshot.ends_with("(taken when the prompt is sent)"), "without a host it says so");
        assert!(shown.state[1].1.contains("age unknown"));
    }

    #[test]
    fn the_prompt_is_slot_by_slot_with_sizes_and_why_empty() {
        let mut e = engine();
        let (slots, notes) = slots(&e, &prompt::render(&e));
        let titles: Vec<&str> = slots.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Instructions", "Learning", "Phase", "Tools", "Artifacts", "History", "Response"]);
        assert_eq!(slots[0].text, "### Goal — what you are for, and who you serve\nBe brief.");
        assert!(slots[0].tokens > 0 && slots[0].empty.is_none());
        assert!(slots[1].empty.as_deref().unwrap().contains("nothing learned yet"), "{:?}", slots[1]);
        assert!(slots[2].empty.as_deref().unwrap().contains("no phases"), "{:?}", slots[2]);
        assert_eq!(slots[5].text, "[User]\nfix a");
        assert!(slots[6].tokens > 0);
        assert_eq!(notes[0], ("goal.md".to_string(), "goal.md has no sections (Goal, Scope, Done): all of it is read as the Goal".to_string()));
        assert_eq!(notes.len(), 2, "then the jev line: {notes:?}");
        assert!(notes[1].0 == "jev" && notes[1].1.starts_with("not possible — no phases"), "{notes:?}");
        e.response = Default::default();
        e.messages.clear();
        let (slots, _) = super::slots(&e, &prompt::render(&e));
        assert!(slots[3].empty.as_deref().unwrap().contains("no response contract"));
        assert_eq!(slots[5].empty.as_deref(), Some("no messages yet"));
        assert!(slots[6].empty.is_some() && slots[6].tokens == 0);
    }

    #[test]
    fn a_file_artifact_page_numbers_its_open_files_marks_focus_and_unsaved_lines() {
        let mut e = engine();
        e.artifacts.insert(
            "code".into(),
            "kind: file\nroot: .\nopen: src/a.rs 2-3\nopen: src/b.rs\nopen: README.md\nedit: README.md\n~~~ old\n# a\n~~~ new\n# b\n~~~".into(),
        );
        let disk = FakeDisk::with(&[("src/a.rs", "1\n2\n3\n4\n"), ("src/b.rs", "b\n"), ("README.md", "# a\nmore\n"), ("x.txt", "x")]);
        let v = &artifacts(&e, &Snapshots { clock: None, host: Some(&disk), family: None })[0];
        assert_eq!(v.summary, "3 open files, 1 unsaved");
        let Detail::File(f) = &v.detail else { panic!("{:?}", v.detail) };
        assert_eq!(f.files.as_ref().unwrap().iter().filter(|(_, open)| *open).count(), 3);
        assert!(f.files.as_ref().unwrap().contains(&("x.txt".to_string(), false)));
        let a = &f.open[0];
        assert_eq!((a.path.as_str(), a.focus, a.lines.len()), ("src/a.rs", Some((2, 3)), 4));
        assert_eq!(a.lines.iter().map(|l| l.focused).collect::<Vec<_>>(), [false, true, true, false]);
        let readme = &f.open[2];
        assert_eq!((readme.unsaved, readme.lines[0].text.as_str(), readme.lines[0].changed, readme.lines[1].changed), (1, "# b", true, false));
        // Without the disk it says when it is read.
        let bare = &artifacts(&e, &Snapshots::default())[0];
        let Detail::File(f) = &bare.detail else { panic!() };
        assert!(f.files.is_err() && f.open[0].lines.is_empty() && f.open[0].note.is_some());
        assert_eq!(bare.summary, "3 open files, 1 unsaved");
    }

    #[test]
    fn a_terminal_artifact_page_puts_running_sessions_first_with_their_output() {
        let mut e = engine();
        e.artifacts.insert(
            "term".into(),
            "kind: terminal\nroot: .\nsession: 1 run exit:0 100 cargo test -q\nsession: 2 start running 160 cargo build\nsession: 3 run exit:101 170 cargo clippy".into(),
        );
        let shell = crate::terminal::tests::FakeShell::default();
        *shell.now.borrow_mut() = 220;
        shell.logs.borrow_mut().insert(1, "test result: ok\n".into());
        let v = &artifacts(&e, &Snapshots { clock: None, host: Some(&shell), family: None })[0];
        assert_eq!(v.summary, "1 running, 2 done");
        let Detail::Terminal { root, sessions } = &v.detail else { panic!() };
        assert_eq!(root, ".");
        assert_eq!(sessions.iter().map(|s| s.id).collect::<Vec<_>>(), [2, 3, 1], "running first, then the newest done");
        assert_eq!((sessions[1].exit, sessions[1].status.as_str(), sessions[1].age.as_str()), (Some(101), "exit 101", "50s ago"));
        assert_eq!(sessions[2].tail, "test result: ok");
        assert_eq!(sessions[0].tail, "(no output yet)");
    }

    #[test]
    fn every_kind_has_a_summary_for_its_card() {
        let mut e = engine();
        e.artifacts.insert("plan".into(), "1. read\n2. fix".into());
        e.artifacts.insert("kids".into(), "kind: engines".into());
        e.artifacts.insert("notes".into(), "kind: learning".into());
        e.artifacts.insert("level".into(), "kind: shared".into());
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        e.learning = "## lessons\n- run the tests first\n".into();
        let family = crate::engines::Family {
            shared: "- 100 E: hello\n- 160 F: hi back\n".into(),
            children: vec![crate::engines::Child { id: "c".into(), name: "C".into(), goal: "check".into(), ..Default::default() }],
            ..Default::default()
        };
        let views = artifacts(&e, &Snapshots { clock: None, host: None, family: Some(&family) });
        let of = |n: &str| views.iter().find(|v| v.name == n).unwrap().summary.clone();
        assert_eq!(of("plan"), "2 lines");
        assert_eq!(of("kids"), "1 child");
        assert_eq!(of("notes"), "1 entry, fullest lessons 1%");
        assert_eq!(of("level"), "2 posts");
        assert_eq!(of("term"), "no sessions yet");
        let level = views.iter().find(|v| v.name == "level").unwrap();
        let Detail::Shared(Some(posts)) = &level.detail else { panic!() };
        assert_eq!((posts[1].author.as_str(), posts[1].text.as_str()), ("F", "hi back"));
        let notes = views.iter().find(|v| v.name == "notes").unwrap();
        let Detail::Learning { sections, .. } = &notes.detail else { panic!() };
        assert_eq!(sections.iter().map(|s| (s.name.as_str(), s.cap)).collect::<Vec<_>>(), [("user", 1200), ("environment", 1600), ("lessons", 1600)]);
        assert_eq!(sections[2].entries, ["run the tests first"]);
    }

    #[test]
    fn tools_are_grouped_by_where_they_come_from() {
        let mut e = engine();
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        e.mcp_tools.push(crate::mcp::Tool {
            server: "web".into(),
            name: "fetch".into(),
            description: "Fetch a page.\nMore.".into(),
            schema: serde_json::json!({"properties": {"url": {"type": "string", "description": "the page"}}}),
        });
        let groups = tool_groups(&e);
        let labels: Vec<String> = groups.iter().map(|g| g.source.label()).collect();
        assert_eq!(labels, ["Built-in", "Artifact · terminal", "MCP · web"]);
        let names: Vec<&str> = groups.iter().flat_map(|g| g.tools.iter().map(|t| t.name.as_str())).collect();
        let mut want = crate::live::toolbox(&e);
        want.push("web.fetch".into());
        assert_eq!(names, want, "every tool once, in the toolbox's order");
        let run = &groups[1].tools[0];
        assert_eq!(run.call, "run(command)");
        assert!(!run.description.is_empty());
        let fetch = &groups[2].tools[0];
        assert_eq!((fetch.call.as_str(), fetch.description.as_str()), ("web.fetch(url)", "Fetch a page."));
        assert_eq!(fetch.args, [("url".to_string(), "the page".to_string())]);
    }

    #[test]
    fn ages_read_short() {
        assert_eq!([ago(5), ago(125), ago(3_725), ago(200_000), ago(-3)], ["5s", "2m", "1h 2m", "2d", "0s"]);
    }
}
