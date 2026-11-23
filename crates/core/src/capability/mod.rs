//! Capabilities: everything an engine can use, whatever runs it
//! (docs/capabilities.md). The owner's standard: *everything is a
//! capability*. Nothing in the turn loop names a runtime; the loop is the
//! same everywhere, and only the way a runtime provides capability X
//! differs.
//!
//! A capability is self-documented and modelled as tools:
//!
//! | part | what |
//! |---|---|
//! | `id` | its name: `file`, `terminal`, `battery`, `gpio` |
//! | `doc` | what it is and how to use it; in the prompt only while present |
//! | `tools` | typed inputs; `approval: yes` for an actuator |
//! | `snapshot` | a state block, refreshed at an interval |
//! | `events` | `when:` triggers that post to the engines using it |
//! | `state` | present, needs-grant (a person's gesture), absent |
//! | `provider` | compiled (this build), declared (`capabilities/<id>.md`), remote (`remote:<peer>`) |
//!
//! **Self-injection.** An absent capability is not rendered, not in the
//! prompt and brings no tools, the way an empty phase block is left out.
//!
//! **Artifacts are groups of capabilities.** An artifact's first line is
//! `uses: [file, clock]` (`kind: file` is the old spelling and stays an
//! alias, so every existing spec and state loads unchanged). The artifact
//! holds the state of at most one stateful capability (file, terminal,
//! engines, learning, browser, evidence); the rest add their tools, docs
//! and snapshots beside it.
//!
//! **Routing.** A compiled capability runs in core (`crate::live`) through
//! the shell's `Host`, which the shell binds to whoever provides it (this
//! machine's disk, or a paired companion). A declared or remote one is
//! called by name (`battery.status`): core checks the inputs and the
//! approval rule (`check`), the call waits like an MCP call, and the
//! shell's registry routes it to the provider (`Registry::route`). The
//! engine never knows which machine runs it.
//!
//! Pure: the shells probe, run commands and talk to peers.

pub mod declared;
pub mod mcp;

use std::sync::Arc;

use serde_json::{Value, json};

use crate::calls::Call;
use crate::live::Live;
use crate::state::Engine;

/// An input's type, as the companion's custom tools have them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    String,
    Integer,
    Number,
    Boolean,
}

impl Ty {
    pub const ALL: [Ty; 4] = [Ty::String, Ty::Integer, Ty::Number, Ty::Boolean];

    pub fn as_str(self) -> &'static str {
        match self {
            Ty::String => "string",
            Ty::Integer => "integer",
            Ty::Number => "number",
            Ty::Boolean => "boolean",
        }
    }

    pub fn parse(word: &str) -> Option<Ty> {
        Ty::ALL.into_iter().find(|t| t.as_str() == word.trim())
    }

    /// Whether `v` is a value of this type (as text: a call's arguments are).
    pub fn accepts(self, v: &str) -> bool {
        match self {
            Ty::String => true,
            Ty::Integer => {
                let d = v.strip_prefix('-').unwrap_or(v);
                !d.is_empty() && d.len() <= 15 && d.bytes().all(|b| b.is_ascii_digit())
            }
            Ty::Number => v.parse::<f64>().is_ok_and(f64::is_finite) && !v.contains(['x', 'X', 'n', 'N', 'i', 'I']),
            Ty::Boolean => v == "true" || v == "false",
        }
    }
}

/// One input of a tool.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub name: String,
    pub ty: Ty,
    pub required: bool,
    pub doc: String,
}

/// Most bytes one input may hold.
pub const MAX_INPUT: usize = 64 * 1024;

/// One tool of a capability.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    /// Its name within the capability (`status`); the engine calls a
    /// declared or remote one as `<capability>.<name>`.
    pub name: String,
    pub doc: String,
    pub inputs: Vec<Input>,
    /// An actuator: it acts on the world, so a call needs approval (an
    /// `allow:` rule of the engine, else it is refused with how to allow).
    pub approval: bool,
    /// The name the provider knows it by (a remote MCP tool's name, which
    /// need not be `<capability>.<name>`). Empty: `<capability>.<name>`.
    pub remote: String,
}

impl Tool {
    /// A call's arguments checked against the inputs: no unknown input, every
    /// required one given, each of its type, no zero byte, at most 64 KB.
    pub fn check(&self, call: &Call) -> Result<(), String> {
        let name = &call.name;
        for (k, v) in &call.args {
            if k == "artifact" {
                continue;
            }
            let Some(i) = self.inputs.iter().find(|i| &i.name == k) else {
                let known: Vec<&str> = self.inputs.iter().map(|i| i.name.as_str()).collect();
                return Err(if known.is_empty() {
                    format!("{name} takes no inputs")
                } else {
                    format!("{name} has no input {k}; its inputs: {}", known.join(", "))
                });
            };
            if v.contains('\0') {
                return Err(format!("{k} holds a zero byte"));
            }
            if v.len() > MAX_INPUT {
                return Err(format!("{k} is longer than 64 KB"));
            }
            if !i.ty.accepts(v) {
                return Err(format!(
                    "{k} must be {}",
                    match i.ty {
                        Ty::Integer => "a whole number",
                        Ty::Number => "a number",
                        Ty::Boolean => "true or false",
                        Ty::String => "text",
                    }
                ));
            }
        }
        for i in self.inputs.iter().filter(|i| i.required) {
            if call.arg(&i.name).is_none_or(str::is_empty) {
                return Err(format!("{name} needs {}=\"...\"", i.name));
            }
        }
        Ok(())
    }

    /// Its inputs as JSON Schema (what MCP's `tools/list` carries).
    pub fn schema(&self) -> Value {
        let props: serde_json::Map<String, Value> = self
            .inputs
            .iter()
            .map(|i| {
                let mut p = json!({ "type": i.ty.as_str() });
                if !i.doc.is_empty() {
                    p["description"] = json!(i.doc);
                }
                (i.name.clone(), p)
            })
            .collect();
        let required: Vec<&str> = self.inputs.iter().filter(|i| i.required).map(|i| i.name.as_str()).collect();
        json!({ "type": "object", "properties": props, "required": required })
    }

    /// One line for the tools slot, like the built-in tools'.
    pub fn describe(&self, call_name: &str) -> String {
        let args: Vec<String> = self.inputs.iter().map(|i| format!("{}=\"...\"", i.name)).collect();
        let what: Vec<String> = self
            .inputs
            .iter()
            .map(|i| {
                let need = if i.required { ", required" } else { "" };
                if i.doc.is_empty() {
                    format!("{}: {}{need}", i.name, i.ty.as_str())
                } else {
                    format!("{}: {}{need}, {}", i.name, i.ty.as_str(), i.doc)
                }
            })
            .collect();
        let what = if what.is_empty() { String::new() } else { format!(" ({})", what.join("; ")) };
        let approval = if self.approval { " Needs approval: it acts on the world." } else { "" };
        format!("- {call_name}({}): {}{what}{approval}", args.join(", "), self.doc)
    }
}

/// A capability's state block and how often it is read again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// The block as last read. Empty: not read yet.
    pub text: String,
    /// Seconds between reads (`None`: read once, or pushed by the provider).
    pub interval: Option<u64>,
    /// When it was last read (unix seconds).
    pub at: Option<i64>,
    /// Where a remote provider serves it (an MCP resource's URI).
    pub uri: String,
}

/// When an event fires: its condition on the snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum When {
    /// The snapshot holds this text.
    Contains(String),
    /// The snapshot does not hold this text.
    Missing(String),
    /// A `key: value` (or `key=value`) in the snapshot compared with a
    /// number (`<`, `<=`, `>`, `>=`) or a text (`=`, `!=`).
    Compare { key: String, op: Op, value: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Eq => "=",
            Op::Ne => "!=",
        }
    }
}

impl When {
    /// `contains <text>`, `missing <text>`, `<key> <op> <value>`.
    pub fn parse(text: &str) -> Result<When, String> {
        let t = text.trim();
        let unquote = |s: &str| s.trim().trim_matches('"').to_string();
        if let Some(rest) = t.strip_prefix("contains ") {
            return Ok(When::Contains(unquote(rest)));
        }
        if let Some(rest) = t.strip_prefix("missing ") {
            return Ok(When::Missing(unquote(rest)));
        }
        let mut parts = t.splitn(3, ' ');
        let (key, op, value) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let op = match op {
            "<" => Op::Lt,
            "<=" => Op::Le,
            ">" => Op::Gt,
            ">=" => Op::Ge,
            "=" | "==" => Op::Eq,
            "!=" => Op::Ne,
            _ => {
                return Err(format!(
                    "`{t}`: a condition is `contains <text>`, `missing <text>` or `<key> <op> <value>` (op: < <= > >= = !=)"
                ));
            }
        };
        if key.is_empty() || value.trim().is_empty() {
            return Err(format!("`{t}`: a comparison needs a key and a value"));
        }
        if matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge) && value.trim().parse::<f64>().is_err() {
            return Err(format!("`{t}`: {} compares numbers", op.as_str()));
        }
        Ok(When::Compare { key: key.to_string(), op, value: unquote(value) })
    }

    pub fn text(&self) -> String {
        match self {
            When::Contains(s) => format!("contains {s}"),
            When::Missing(s) => format!("missing {s}"),
            When::Compare { key, op, value } => format!("{key} {} {value}", op.as_str()),
        }
    }

    /// Whether it holds for `snapshot`.
    pub fn holds(&self, snapshot: &str) -> bool {
        match self {
            When::Contains(s) => snapshot.contains(s.as_str()),
            When::Missing(s) => !snapshot.contains(s.as_str()),
            When::Compare { key, op, value } => {
                let Some(found) = value_of(snapshot, key) else { return false };
                match op {
                    Op::Eq => found == *value,
                    Op::Ne => found != *value,
                    _ => {
                        let (Some(a), Ok(b)) = (leading_number(&found), value.parse::<f64>()) else { return false };
                        match op {
                            Op::Lt => a < b,
                            Op::Le => a <= b,
                            Op::Gt => a > b,
                            _ => a >= b,
                        }
                    }
                }
            }
        }
    }
}

/// The value of `key` in a snapshot: the first `key: value` or `key=value`,
/// at a line's start or after a space, a tab or a `;`.
pub fn value_of(snapshot: &str, key: &str) -> Option<String> {
    for line in snapshot.lines() {
        let mut from = 0;
        while let Some(at) = line[from..].find(key) {
            let start = from + at;
            let before_ok = start == 0 || line[..start].ends_with([' ', '\t', ';', ',']);
            let rest = &line[start + key.len()..];
            if before_ok && let Some(v) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) {
                let v = v.trim_start();
                let end = v.find([';', ',', '\t']).unwrap_or(v.len());
                return Some(v[..end].trim().to_string());
            }
            from = start + key.len();
        }
    }
    None
}

/// `48.3'C` → 48.3, `85%` → 85.
fn leading_number(v: &str) -> Option<f64> {
    let end = v.char_indices().find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || (*i == 0 && *c == '-'))).map_or(v.len(), |(i, _)| i);
    v[..end].parse().ok()
}

/// One event a capability raises.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub when: When,
    /// What the engines using it are told.
    pub text: String,
}

/// Whether a capability can be used here now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Present,
    /// It is here, but a person must allow it first (a browser permission
    /// that needs a click): why, and the Grant ask says so.
    NeedsGrant(String),
    /// Not here: why (the probe failed, the peer left, this runtime has none).
    Absent(String),
}

impl State {
    pub fn word(&self) -> &'static str {
        match self {
            State::Present => "present",
            State::NeedsGrant(_) => "needs-grant",
            State::Absent(_) => "absent",
        }
    }

    pub fn why(&self) -> &str {
        match self {
            State::Present => "",
            State::NeedsGrant(w) | State::Absent(w) => w,
        }
    }

    pub fn parse(word: &str, why: &str) -> State {
        match word {
            "present" => State::Present,
            "needs-grant" => State::NeedsGrant(why.to_string()),
            _ => State::Absent(if why.is_empty() { "the provider says it is absent".into() } else { why.to_string() }),
        }
    }
}

/// Who provides a capability.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provider {
    /// Built into this build (`crate::live`), through the shell's `Host`.
    Compiled,
    /// A `capabilities/<id>.md` file of this machine (`declared`).
    Declared,
    /// Another runtime that joined (a companion, a host, a device): its name.
    Remote(String),
}

impl Provider {
    pub fn text(&self) -> String {
        match self {
            Provider::Compiled => "compiled".into(),
            Provider::Declared => "declared".into(),
            Provider::Remote(p) => format!("remote:{p}"),
        }
    }
}

/// One capability, as a registry holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Capability {
    pub id: String,
    pub doc: String,
    pub tools: Vec<Tool>,
    pub snapshot: Option<Snapshot>,
    pub events: Vec<Event>,
    pub state: State,
    pub provider: Provider,
}

impl Capability {
    pub fn new(id: &str, doc: &str, provider: Provider) -> Capability {
        Capability {
            id: id.into(),
            doc: doc.into(),
            tools: Vec::new(),
            snapshot: None,
            events: Vec::new(),
            state: State::Present,
            provider,
        }
    }

    pub fn present(&self) -> bool {
        self.state == State::Present
    }

    /// Whether it is built in and runs in core (its tools keep their bare
    /// names: `open`, `run`).
    pub fn compiled_id(&self) -> bool {
        compiled_live(&self.id).is_some() || COMPILED.iter().any(|b| b.id == self.id)
    }

    /// The name an engine calls one of its tools by: a compiled
    /// capability's bare (`run`), any other's `<id>.<tool>`.
    pub fn call_name(&self, tool: &Tool) -> String {
        if self.compiled_id() { tool.name.clone() } else { format!("{}.{}", self.id, tool.name) }
    }

    /// The doc's first line (a heading for its section).
    pub fn line(&self) -> &str {
        self.doc.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(&self.id)
    }
}

/// A built-in capability: its id, the live kind that runs it in core (none
/// for one that is only a presence, like `container`), its tools and doc.
struct Builtin {
    id: &'static str,
    live: Option<Live>,
    tools: &'static [&'static str],
    /// The tools that change something outside the artifact (a file on
    /// disk): left out where the artifact uses it in read mode (`READ`).
    writes: &'static [&'static str],
    doc: &'static str,
}

const COMPILED: [Builtin; 12] = [
    Builtin { id: "clock", live: Some(Live::Clock), tools: &[], writes: &[], doc: "the date and time now" },
    Builtin {
        id: "scripts",
        live: Some(Live::Scripts),
        tools: &["script"],
        writes: &[],
        doc: "your scripts: call sequences you run with one script(...) call",
    },
    Builtin { id: "alerts", live: Some(Live::Alerts), tools: &[], writes: &[], doc: "your alerts: what wakes you, and when" },
    Builtin {
        id: "file",
        live: Some(Live::File),
        tools: &crate::file::TOOLS,
        writes: &crate::file::WRITES,
        doc: "files on disk as they are now, and the ones you have open",
    },
    Builtin {
        id: "terminal",
        live: Some(Live::Terminal),
        tools: &crate::terminal::TOOLS,
        writes: &[],
        doc: "your terminal: what is running and what last ran",
    },
    Builtin {
        id: "engines",
        live: Some(Live::Engines),
        tools: &crate::engines::TOOLS,
        writes: &[],
        doc: "your child engines: their goals, what they are doing, their replies",
    },
    Builtin {
        id: "shared",
        live: Some(Live::Shared),
        tools: &crate::shared::TOOLS,
        writes: &[],
        doc: "your thread: the user and every engine of your tree; how you talk to each other",
    },
    Builtin {
        id: "learning",
        live: Some(Live::Learning),
        tools: &["collect", "learn"],
        writes: &[],
        doc: "what you learned, and the context you collected to learn from",
    },
    Builtin {
        id: "analytics",
        live: Some(Live::Learning),
        tools: &["analytics"],
        writes: &[],
        doc: "your recorded work: turns, waits, models and tools, by query",
    },
    Builtin {
        id: "browser",
        live: Some(Live::Browser),
        tools: &crate::browser::TOOLS,
        writes: &[],
        doc: "a page in a browser as it is now: its text, its console errors, its last screenshot",
    },
    Builtin {
        id: "container",
        live: None,
        tools: &[],
        writes: &[],
        doc: "a Linux to build and run things in (docs/container.md); its commands go through the terminal",
    },
    Builtin {
        id: "evidence",
        live: Some(Live::Evidence),
        tools: &crate::evidence::artifact::TOOLS,
        writes: &[],
        doc: "the project's evidence: every scenario's screenshot and page text against the committed baseline, and what changed",
    },
];

/// Every compiled capability's id, in order.
pub fn compiled_ids() -> Vec<&'static str> {
    COMPILED.iter().map(|b| b.id).collect()
}

/// The live kind that runs compiled capability `id` in core.
pub fn compiled_live(id: &str) -> Option<Live> {
    COMPILED.iter().find(|b| b.id == id).and_then(|b| b.live)
}

/// The compiled capability that brings tool `name` (`open` → file).
pub fn compiled_for_tool(name: &str) -> Option<(&'static str, Live)> {
    COMPILED.iter().find(|b| b.tools.contains(&name)).and_then(|b| b.live.map(|l| (b.id, l)))
}

// ---- read mode: `uses: [file:read]` ----

/// The mode suffix on a capability in an artifact's `uses:` line:
/// `file:read` is the file capability without the tools that change
/// anything outside the artifact (`writes`), and a `terminal:read` refuses
/// the commands known to write (`crate::terminal::writes`). A mode is data
/// on the artifact: the capability is the same one, present or absent as
/// it is, and nothing else changes.
pub const READ: &str = ":read";

/// An id from a `uses:` line and whether it is in read mode.
fn split_mode(id: &str) -> (&str, bool) {
    match id.trim().strip_suffix(READ) {
        Some(base) => (base.trim(), true),
        None => (id.trim(), false),
    }
}

/// The tools of compiled capability `id` that write (none for an id that
/// is not compiled).
pub fn writes(id: &str) -> &'static [&'static str] {
    COMPILED.iter().find(|b| b.id == id).map_or(&[], |b| b.writes)
}

/// The capabilities an artifact's text uses in read mode (`file` for
/// `uses: [file:read]`).
pub fn read_only(text: &str) -> Vec<String> {
    let Some(first) = text.lines().next().map(str::trim) else { return Vec::new() };
    let list = first.strip_prefix("uses:").or_else(|| first.strip_prefix("kind:")).unwrap_or("").trim();
    let list = list.strip_prefix('[').and_then(|l| l.strip_suffix(']')).unwrap_or(list);
    list.split(',').map(split_mode).filter(|(_, read)| *read).map(|(id, _)| id.to_string()).collect()
}

/// Why a call to a writing tool is refused: the artifacts that bring it
/// hold its capability in read mode, so it is not this engine's tool
/// (`None`: it is not refused here). For the error a hand-made call gets.
pub fn refusal<'a>(artifacts: impl IntoIterator<Item = (&'a String, &'a String)>, tool: &str) -> Option<String> {
    let (id, _) = compiled_for_tool(tool)?;
    if !writes(id).contains(&tool) {
        return None;
    }
    let held: Vec<(&String, bool)> = artifacts
        .into_iter()
        .filter(|(_, t)| uses(t).is_some_and(|ids| ids.iter().any(|x| x == id)))
        .map(|(n, t)| (n, read_only(t).iter().any(|x| x == id)))
        .collect();
    let names: Vec<&str> = held.iter().filter(|(_, read)| *read).map(|(n, _)| n.as_str()).collect();
    (!held.is_empty() && held.iter().all(|(_, read)| *read)).then(|| {
        let left: Vec<&str> = COMPILED.iter().find(|b| b.id == id).map_or(Vec::new(), |b| b.tools.iter().filter(|t| !b.writes.contains(t)).copied().collect());
        format!(
            "`{tool}` is refused: your {id} artifact {} is read-only (uses: [{id}{READ}]), so it brings {} only. You change nothing here: say what should change in your reply",
            names.join(", "),
            left.join(", ")
        )
    })
}

/// The compiled capabilities, all present (a shell marks what it lacks).
pub fn compiled() -> Vec<Capability> {
    COMPILED
        .iter()
        .map(|b| {
            let tools = b
                .tools
                .iter()
                .map(|t| {
                    let spec = crate::tools::BROUGHT.iter().find(|s| s.name == *t);
                    Tool {
                        name: t.to_string(),
                        doc: spec.map_or("", |s| s.description).to_string(),
                        inputs: spec
                            .map(|s| {
                                s.args
                                    .iter()
                                    .map(|(a, d)| Input { name: a.to_string(), ty: Ty::String, required: false, doc: d.to_string() })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        approval: false,
                        remote: String::new(),
                    }
                })
                .collect();
            Capability { tools, ..Capability::new(b.id, b.doc, Provider::Compiled) }
        })
        .collect()
}

// ---- artifacts: `uses: [..]`, and `kind:` as its alias ----

/// The capabilities an old `kind:` word stands for. `learning` brought the
/// `analytics` tool before analytics was its own capability, so it stands
/// for both; `thread` (and its old name `dashboard`) is the shared
/// capability's other name.
pub fn alias(kind: &str) -> Option<Vec<String>> {
    let k = kind.trim();
    match k {
        "learning" => Some(vec!["learning".into(), "analytics".into()]),
        "thread" | "dashboard" => Some(vec!["shared".into()]),
        _ => Live::ALL.iter().any(|l| l.as_str() == k).then(|| vec![k.to_string()]),
    }
}

/// The capabilities an artifact's text uses: its first line, `uses: [a, b]`
/// (or `uses: a, b`), or `kind: x` (`alias`). `None`: a plain text artifact
/// (an unknown `kind:` too, as before).
pub fn uses(text: &str) -> Option<Vec<String>> {
    let first = text.lines().next()?.trim();
    if let Some(kind) = first.strip_prefix("kind:") {
        return alias(split_mode(kind).0);
    }
    let list = first.strip_prefix("uses:")?.trim();
    let list = list.strip_prefix('[').and_then(|l| l.strip_suffix(']')).unwrap_or(list);
    let mut out: Vec<String> = Vec::new();
    // A mode (`file:read`) names the same capability (`read_only`).
    for id in list.split(',').map(|id| split_mode(id).0).filter(|s| !s.is_empty()) {
        for id in alias(id).unwrap_or_else(|| vec![id.to_string()]) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    Some(out)
}

/// The `uses:` header for a list (`uses: [file, clock]`).
pub fn header(ids: &[&str]) -> String {
    format!("uses: [{}]", ids.join(", "))
}

/// What is wrong with an artifact's `uses:` line, if anything: more than
/// one capability that keeps state in the artifact.
pub fn problem(text: &str) -> Option<String> {
    let ids = uses(text)?;
    let stateful: Vec<&String> = ids.iter().filter(|id| compiled_live(id).is_some_and(Live::has_state) && *id != "analytics").collect();
    (stateful.len() > 1).then(|| {
        format!(
            "{} each keep their state in the artifact: an artifact holds one; make one artifact each",
            stateful.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" and ")
        )
    })
}

/// An artifact's state functions write `kind: x` as the first line; one
/// whose header was `uses: [..]` keeps it (call after a write).
pub fn keep_header(text: &mut String, header: &str) {
    // A `kind: file:read` keeps its mode too.
    let keep = header.starts_with("uses:") || (header.starts_with("kind:") && header.trim_end().ends_with(READ));
    if !keep || text.lines().next() == Some(header) {
        return;
    }
    let rest = text.split_once('\n').map_or("", |(_, r)| r).to_string();
    *text = if rest.is_empty() { header.to_string() } else { format!("{header}\n{rest}") };
}

/// The first line of an artifact (its header).
pub fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").to_string()
}

/// One artifact as the capabilities it uses make it: the live kind that
/// runs its state in core (if that capability is present), and the
/// present capabilities that are not compiled (their docs, tools and
/// snapshots go beside it).
#[derive(Clone, Debug, PartialEq)]
pub struct Equipped<'a> {
    pub name: String,
    pub live: Option<Live>,
    /// The present compiled capabilities it uses (for its tools).
    pub compiled: Vec<&'a Capability>,
    /// The present declared or remote capabilities it uses.
    pub outside: Vec<&'a Capability>,
    /// The ones it uses that are not present: (id, why).
    pub missing: Vec<(String, String)>,
    /// The ones it uses in read mode (`uses: [file:read]`): their writing
    /// tools are not offered (`offers`).
    pub read: Vec<String>,
}

impl Equipped<'_> {
    /// Whether this artifact offers tool `tool` of capability `c`: not a
    /// writing one where it holds `c` in read mode.
    pub fn offers(&self, c: &Capability, tool: &str) -> bool {
        !(self.read.contains(&c.id) && writes(&c.id).contains(&tool))
    }

    /// The compiled tools it offers, by name, in order.
    pub fn compiled_tools(&self) -> Vec<String> {
        self.compiled.iter().flat_map(|c| c.tools.iter().filter(|t| self.offers(c, &t.name)).map(|t| t.name.clone())).collect()
    }

    /// What the model is told about its modes (empty: none).
    pub fn mode_note(&self) -> String {
        if self.read.is_empty() {
            return String::new();
        }
        let ids: Vec<String> = self.read.iter().map(|id| format!("{id}{READ}")).collect();
        format!(" (read-only: {}: you read and run checks here, you change nothing)", ids.join(", "))
    }
}

/// Every capability artifact engine `e` holds, with what is present: in
/// render order (by kind, the clock last, then by name; one with only
/// outside capabilities after the kinds, before the clock). An artifact
/// whose capabilities are all absent is left out (self-injection).
pub fn equipped(e: &Engine) -> Vec<Equipped<'_>> {
    let reg = &e.machine.0;
    let mut out = Vec::new();
    for (name, text) in &e.artifacts {
        let Some(ids) = uses(text) else { continue };
        let mut eq = Equipped {
            name: name.clone(),
            live: None,
            compiled: Vec::new(),
            outside: Vec::new(),
            missing: Vec::new(),
            read: read_only(text),
        };
        for id in &ids {
            match reg.effective(id) {
                Some(c) if c.present() => {
                    // A compiled id runs in core whoever provides it (the
                    // provider is the `Host` the shell binds): its tools are
                    // the compiled ones, not a peer's manifest of them.
                    let built = || reg.all().iter().find(|x| x.id == *id && x.provider == Provider::Compiled).unwrap_or(c);
                    if let Some(l) = compiled_live(id) {
                        eq.live = eq.live.or(Some(l));
                        eq.compiled.push(built());
                    } else if c.compiled_id() {
                        eq.compiled.push(built());
                    } else {
                        eq.outside.push(c);
                    }
                }
                Some(c) => eq.missing.push((id.clone(), format!("{}: {}", c.state.word(), c.state.why()))),
                None => eq.missing.push((id.clone(), "absent: nothing here provides it".into())),
            }
        }
        if eq.live.is_some() || !eq.outside.is_empty() || !eq.compiled.is_empty() {
            out.push(eq);
        }
    }
    let rank = |q: &Equipped| match q.live {
        Some(Live::Clock) => (2u8, Live::Clock),
        Some(l) => (0, l),
        None => (1, Live::Clock),
    };
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The tools the outside capabilities of `e`'s artifacts bring, by the
/// name the engine calls them (`battery.status`), each once.
pub fn outside_tools(e: &Engine) -> Vec<(String, &Tool)> {
    let mut out: Vec<(String, &Tool)> = Vec::new();
    for eq in equipped(e) {
        for c in eq.outside {
            for t in &c.tools {
                let name = c.call_name(t);
                if !out.iter().any(|(n, _)| *n == name) {
                    out.push((name, t));
                }
            }
        }
    }
    out
}

/// The tools slot's lines for the outside tools.
pub fn describe(e: &Engine) -> String {
    outside_tools(e).into_iter().map(|(n, t)| t.describe(&n)).collect::<Vec<_>>().join("\n")
}

/// An outside capability's part of its artifact's section: its doc (only
/// present ones get here), then its snapshot.
pub fn section(c: &Capability) -> String {
    let mut s = format!("#### {} ({})\n{}", c.id, c.provider.text(), c.doc.trim());
    if let Some(snap) = &c.snapshot {
        let body = if snap.text.trim().is_empty() { "(not read yet)" } else { snap.text.trim_end() };
        s.push_str(&format!("\n\nnow:\n{body}"));
    }
    s
}

/// Check a call to an outside capability's tool before it waits: its
/// inputs, and for an actuator an `allow:` rule of the engine. `None`: not
/// an outside tool of this engine.
pub fn check(e: &Engine, call: &Call) -> Option<Result<(), String>> {
    let (_, tool) = outside_tools(e).into_iter().find(|(n, _)| *n == call.name)?;
    if let Err(why) = tool.check(call) {
        return Some(Err(why));
    }
    if tool.approval {
        let input = Value::Object(call.args.iter().map(|(k, v)| (k.clone(), json!(v))).collect());
        let rules = crate::approval::parse_rules(&e.allow);
        if crate::approval::allowed_by(&rules, &call.name, &input).is_none() {
            return Some(Err(format!(
                "{} needs approval: it acts on the world. Ask the person (reply with an ask), or its owner allows it in engine.md (`allow: {}`)",
                call.name, call.name
            )));
        }
    }
    Some(Ok(()))
}

/// A call on its way to the provider that answers it.
#[derive(Clone, Debug, PartialEq)]
pub struct Routed {
    pub provider: Provider,
    pub capability: String,
    /// The tool's name on the provider (a declared tool's name, a remote
    /// MCP tool's name).
    pub tool: String,
    pub args: Vec<(String, String)>,
}

/// The registry of one shell: every capability, tagged by provider.
#[derive(Clone, Debug, PartialEq)]
pub struct Registry {
    caps: Vec<Capability>,
    /// The remote peers that joined, with what they said in their hello.
    pub peers: Vec<Peer>,
    /// The machine the `file` and `terminal` capabilities run on
    /// (`crate::environment`): reported by the shell, shown in Settings.
    pub environment: Option<crate::environment::Environment>,
    /// Every model and program an engine can be run by, with whether each
    /// is present here (`crate::subscriptions`): built and refreshed by
    /// the shell, shown by the `subscriptions` element. `None`: none reported.
    pub subscriptions: Option<crate::subscriptions::Subscriptions>,
}

/// A runtime that joined (the handshake's hello).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peer {
    pub name: String,
    pub runtime: String,
    pub platform: String,
    pub version: String,
    pub protocol: String,
    /// Where it answers (its MCP URL).
    pub url: String,
    pub connected: bool,
    /// Unix seconds of its last heartbeat.
    pub seen: i64,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry { caps: compiled(), peers: Vec::new(), environment: None, subscriptions: None }
    }
}

/// What a register changed, by capability id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// An event that fired.
#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub capability: String,
    pub event: String,
    pub text: String,
}

impl Fired {
    /// The message the engines using it get.
    pub fn message(&self) -> String {
        format!("event {}.{}: {} (capability {})", self.capability, self.event, self.text, self.capability)
    }
}

fn rank(p: &Provider) -> u8 {
    match p {
        Provider::Compiled => 0,
        Provider::Declared => 1,
        Provider::Remote(_) => 2,
    }
}

impl Registry {
    /// An empty registry (no compiled ones either).
    pub fn empty() -> Registry {
        Registry { caps: Vec::new(), peers: Vec::new(), environment: None, subscriptions: None }
    }

    /// Every capability, by id, then by provider.
    pub fn all(&self) -> &[Capability] {
        &self.caps
    }

    /// The one an engine uses for `id`: the first present one (compiled,
    /// then declared, then remote); else the first of any state (to say why).
    pub fn effective(&self, id: &str) -> Option<&Capability> {
        let mut of: Vec<&Capability> = self.caps.iter().filter(|c| c.id == id).collect();
        of.sort_by_key(|c| rank(&c.provider));
        of.iter().find(|c| c.present()).or(of.first()).copied()
    }

    pub fn present(&self, id: &str) -> bool {
        self.effective(id).is_some_and(Capability::present)
    }

    /// The state of `id` as an engine sees it.
    pub fn state(&self, id: &str) -> State {
        self.effective(id).map_or_else(|| State::Absent("nothing here provides it".into()), |c| c.state.clone())
    }

    fn sort(&mut self) {
        self.caps.sort_by(|a, b| {
            a.id.cmp(&b.id).then_with(|| rank(&a.provider).cmp(&rank(&b.provider))).then_with(|| a.provider.cmp(&b.provider))
        });
    }

    /// Set the state of `id` from `provider` (a shell's probe of a compiled one).
    pub fn set_state(&mut self, id: &str, provider: &Provider, state: State) {
        if let Some(c) = self.caps.iter_mut().find(|c| c.id == id && c.provider == *provider) {
            c.state = state;
        }
    }

    /// Add one capability, or replace the one with its id and provider (a
    /// shell's own runtime capability, probed or granted).
    pub fn upsert(&mut self, c: Capability) {
        match self.caps.iter_mut().find(|o| o.id == c.id && o.provider == c.provider) {
            Some(o) => {
                let snapshot = o.snapshot.take();
                *o = c;
                if o.snapshot.is_none() {
                    o.snapshot = snapshot;
                }
            }
            None => {
                self.caps.push(c);
                self.sort();
            }
        }
    }

    /// Register `provider`'s capabilities: they replace what it had (a
    /// re-register sends its whole manifest). What changed, by id.
    pub fn register(&mut self, provider: Provider, caps: Vec<Capability>) -> Changes {
        let mut ch = Changes::default();
        let old: Vec<Capability> = self.caps.iter().filter(|c| c.provider == provider).cloned().collect();
        for c in &caps {
            match old.iter().find(|o| o.id == c.id) {
                None => ch.added.push(c.id.clone()),
                Some(o) if !same_manifest(o, c) => ch.changed.push(c.id.clone()),
                Some(_) => {}
            }
        }
        for o in &old {
            if !caps.iter().any(|c| c.id == o.id) {
                ch.removed.push(o.id.clone());
            }
        }
        self.caps.retain(|c| c.provider != provider);
        for mut c in caps {
            c.provider = provider.clone();
            // A re-register keeps the last snapshot read (it is not in a manifest).
            if let (Some(o), Some(s)) = (old.iter().find(|o| o.id == c.id), c.snapshot.as_mut())
                && let Some(os) = &o.snapshot
                && s.text.is_empty()
            {
                s.text = os.text.clone();
                s.at = os.at;
            }
            self.caps.push(c);
        }
        self.sort();
        ch
    }

    /// A peer joined (its hello): kept, marked connected.
    pub fn hello(&mut self, peer: Peer) {
        self.peers.retain(|p| p.name != peer.name);
        self.peers.push(Peer { connected: true, ..peer });
        self.peers.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// A heartbeat from a peer.
    pub fn beat(&mut self, peer: &str, now: i64) {
        if let Some(p) = self.peers.iter_mut().find(|p| p.name == peer) {
            p.seen = now;
            p.connected = true;
        }
    }

    /// A peer left (its stream closed, its heartbeat stopped): every
    /// capability it provides becomes absent, so it leaves prompts and UI.
    /// The ids that went.
    pub fn disconnect(&mut self, peer: &str, why: &str) -> Vec<String> {
        if let Some(p) = self.peers.iter_mut().find(|p| p.name == peer) {
            p.connected = false;
        }
        let provider = Provider::Remote(peer.to_string());
        let mut gone = Vec::new();
        for c in self.caps.iter_mut().filter(|c| c.provider == provider) {
            if c.present() {
                gone.push(c.id.clone());
            }
            c.state = State::Absent(format!("{peer} left: {why}"));
        }
        gone
    }

    /// A new snapshot of `id` from `provider`, read at `now`: kept, and
    /// the events whose condition became true fire (edge-triggered: the
    /// first reading never fires, a condition that stays true fires once).
    /// A remote provider decides its events itself and sends them with
    /// its update (`remote_event`), so here they do not fire twice.
    pub fn snapshot(&mut self, id: &str, provider: &Provider, text: &str, now: i64) -> Vec<Fired> {
        let Some(c) = self.caps.iter_mut().find(|c| c.id == id && c.provider == *provider) else { return Vec::new() };
        let snap = c.snapshot.get_or_insert_with(Snapshot::default);
        let before = (snap.at.is_some()).then(|| snap.text.clone());
        snap.text = text.to_string();
        snap.at = Some(now);
        let Some(before) = before.filter(|_| !matches!(provider, Provider::Remote(_))) else { return Vec::new() };
        c.events
            .iter()
            .filter(|ev| ev.when.holds(text) && !ev.when.holds(&before))
            .map(|ev| Fired { capability: c.id.clone(), event: ev.name.clone(), text: ev.text.clone() })
            .collect()
    }

    /// An event a peer sent with its update (`mcp::Note::Updated`), if it
    /// provides that capability and it is present.
    pub fn remote_event(&self, peer: &str, id: &str, event: &str, text: &str) -> Option<Fired> {
        let provider = Provider::Remote(peer.to_string());
        let c = self.caps.iter().find(|c| c.id == id && c.provider == provider && c.present())?;
        let text = if text.is_empty() { c.events.iter().find(|e| e.name == event).map_or(event, |e| e.text.as_str()) } else { text };
        Some(Fired { capability: id.to_string(), event: event.to_string(), text: text.to_string() })
    }

    /// The snapshots due now: (id, provider) of every present one with an
    /// interval whose last read is that long ago (or never read).
    pub fn due(&self, now: i64) -> Vec<(String, Provider)> {
        self.caps
            .iter()
            .filter(|c| c.present() && c.provider != Provider::Compiled)
            .filter_map(|c| {
                let s = c.snapshot.as_ref()?;
                let due = match (s.at, s.interval) {
                    (None, _) => true,
                    (Some(at), Some(every)) => now - at >= every as i64,
                    (Some(_), None) => false,
                };
                due.then(|| (c.id.clone(), c.provider.clone()))
            })
            .collect()
    }

    /// Where a call to an outside tool goes: the effective present
    /// capability whose tool it names (`battery.status`). `None`: no
    /// outside capability has it (an MCP tool, or a built-in).
    pub fn route(&self, call: &Call) -> Option<Routed> {
        // `model.local.ask`: the id may hold a dot; the tool is after the last.
        let (id, tool) = call.name.rsplit_once('.')?;
        let c = self.effective(id).filter(|c| c.present() && !c.compiled_id())?;
        let t = c.tools.iter().find(|t| t.name == tool)?;
        let remote = if t.remote.is_empty() { call.name.clone() } else { t.remote.clone() };
        let name = match c.provider {
            Provider::Declared => t.name.clone(),
            _ => remote,
        };
        Some(Routed {
            provider: c.provider.clone(),
            capability: c.id.clone(),
            tool: name,
            args: call.args.iter().filter(|(k, _)| k != "artifact").cloned().collect(),
        })
    }
}

/// Whether two capabilities say the same (state and snapshot text aside).
fn same_manifest(a: &Capability, b: &Capability) -> bool {
    a.id == b.id && a.doc == b.doc && a.tools == b.tools && a.events == b.events && a.state == b.state
}

/// The registry an engine sees: shared, never saved and never compared (an
/// engine is the same engine whatever this machine offers today).
#[derive(Clone, Debug, Default)]
pub struct Machine(pub Arc<Registry>);

impl PartialEq for Machine {
    fn eq(&self, _: &Machine) -> bool {
        true
    }
}

impl Machine {
    pub fn new(r: Registry) -> Machine {
        Machine(Arc::new(r))
    }
}

/// The engines that use capability `id` (an artifact of theirs lists it)
/// and should hear its events: their indexes.
pub fn users(state: &crate::state::State, id: &str) -> Vec<usize> {
    state
        .engines
        .iter()
        .enumerate()
        .filter(|(_, e)| e.artifacts.values().any(|t| uses(t).is_some_and(|u| u.iter().any(|x| x == id))))
        .map(|(i, _)| i)
        .collect()
}

// ---- the Settings → Capabilities page, renderer-neutral ----

/// One capability as the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub state: State,
    pub line: String,
    /// Its tools as an engine calls them, an actuator marked.
    pub tools: Vec<(String, bool)>,
    /// Its snapshot's first lines, if it has one.
    pub snapshot: Option<String>,
    /// Engines use this one for its id (else another provider's is used,
    /// or none is present).
    pub used: bool,
    pub events: Vec<String>,
}

/// The capabilities of one provider.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub provider: Provider,
    /// A peer's hello (runtime, platform) and whether it is connected.
    pub peer: Option<Peer>,
    pub rows: Vec<Row>,
}

/// The page: every capability grouped by provider (compiled, declared,
/// each peer), and the counts by state.
pub fn view(r: &Registry) -> (Vec<Group>, [usize; 3]) {
    let mut groups: Vec<Group> = Vec::new();
    let mut counts = [0usize; 3];
    let mut caps: Vec<&Capability> = r.all().iter().collect();
    caps.sort_by(|a, b| rank(&a.provider).cmp(&rank(&b.provider)).then_with(|| a.provider.cmp(&b.provider)).then_with(|| a.id.cmp(&b.id)));
    for c in caps {
        counts[match c.state {
            State::Present => 0,
            State::NeedsGrant(_) => 1,
            State::Absent(_) => 2,
        }] += 1;
        let row = Row {
            id: c.id.clone(),
            state: c.state.clone(),
            line: c.line().to_string(),
            tools: c.tools.iter().map(|t| (c.call_name(t), t.approval)).collect(),
            snapshot: c
                .snapshot
                .as_ref()
                .filter(|s| !s.text.trim().is_empty())
                .map(|s| s.text.lines().take(6).collect::<Vec<_>>().join("\n")),
            used: r.effective(&c.id).is_some_and(|e| e.provider == c.provider && e.present()),
            events: c.events.iter().map(|e| format!("{} (when {})", e.name, e.when.text())).collect(),
        };
        match groups.iter_mut().find(|g| g.provider == c.provider) {
            Some(g) => g.rows.push(row),
            None => {
                let peer = match &c.provider {
                    Provider::Remote(name) => r.peers.iter().find(|p| &p.name == name).cloned(),
                    _ => None,
                };
                groups.push(Group { provider: c.provider.clone(), peer, rows: vec![row] });
            }
        }
    }
    (groups, counts)
}

/// A group's title on the page.
pub fn group_title(g: &Group) -> String {
    match (&g.provider, &g.peer) {
        (Provider::Compiled, _) => "COMPILED · built into this build".into(),
        (Provider::Declared, _) => "DECLARED · capabilities/*.md on this machine".into(),
        (Provider::Remote(n), Some(p)) => {
            format!("REMOTE · {n} · {} {}{}", p.runtime, p.platform, if p.connected { "" } else { " · left" })
        }
        (Provider::Remote(n), None) => format!("REMOTE · {n}"),
    }
}

#[cfg(test)]
mod tests;
