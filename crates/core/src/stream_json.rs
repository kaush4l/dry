//! The live events of a session's program (`--output-format stream-json`
//! in `sessions.json`): one JSON object a line, read as they come, so the
//! UI shows the program working instead of a spinner until it ends.
//!
//! What is read (everything else is skipped):
//!
//! ```text
//! {"type":"system","subtype":"init", "session_id", "model", "tools", "slash_commands", "skills", "agents"}
//! {"type":"assistant","message":{"content":[{"type":"text"} | {"type":"tool_use","id","name","input"}]}, "parent_tool_use_id"}
//! {"type":"user","message":{"content":[{"type":"tool_result","tool_use_id","content","is_error"}]}}
//! {"type":"system","subtype":"task_started", "tool_use_id", "description", "subagent_type", "is_backgrounded"}
//! {"type":"system","subtype":"task_notification", "tool_use_id", "status", "summary"}
//! {"type":"result", "result", "is_error", "session_id"}
//! ```
//!
//! - **Tool rows.** Each `tool_use` is a row (`Bash(echo hi)`, running),
//!   closed by its `tool_result` (its first line, or the error). A row a
//!   sub-agent ran keeps its agent (`parent`).
//! - **Agents.** A `Task`/`Agent` call is also an agent: started, then
//!   done with its summary (`task_notification`), or with its result when
//!   it ran in the foreground. The shell mirrors them as read-only
//!   participants on the session's thread.
//! - **Init.** The program's slash commands, skills, tools and agents, for
//!   the composer's `/` list.
//! - **Results.** A program can end more than one turn in one run (a
//!   background agent finishing wakes it): every result is kept, in order.
//!
//! Pure: lines in, a view out. The shell runs the program and feeds it.

use serde_json::Value;

/// Tool names whose call starts a sub-agent.
pub const AGENT_TOOLS: [&str; 2] = ["Task", "Agent"];

/// Most characters of a row's call or result line.
pub const LINE_CHARS: usize = 160;

/// One event a line held (an assistant message may hold several).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Init(Init),
    Text { text: String, parent: Option<String> },
    ToolUse { id: String, name: String, input: Value, parent: Option<String> },
    ToolResult { id: String, error: bool, text: String },
    AgentStarted { id: String, description: String, kind: String, background: bool },
    AgentDone { id: String, status: String, summary: String },
    Result { text: Option<String>, error: bool, session_id: Option<String> },
}

/// What the program said it is, at its start.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Init {
    pub session_id: Option<String>,
    pub model: String,
    pub tools: Vec<String>,
    pub slash_commands: Vec<String>,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn list(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(Value::as_array).map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The text of a tool result's `content` (a string, or a list of blocks).
fn content_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(blocks)) => blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The events in one line of output; none for a line that is not one.
pub fn parse_line(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Vec::new() };
    let parent = v.get("parent_tool_use_id").and_then(Value::as_str).map(str::to_string);
    let blocks = || v.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
    match (v.get("type").and_then(Value::as_str), v.get("subtype").and_then(Value::as_str)) {
        (Some("system"), Some("init")) => vec![Event::Init(Init {
            session_id: v.get("session_id").and_then(Value::as_str).map(str::to_string),
            model: s(&v, "model"),
            tools: list(&v, "tools"),
            slash_commands: list(&v, "slash_commands"),
            skills: list(&v, "skills"),
            agents: list(&v, "agents"),
        })],
        (Some("system"), Some("task_started")) => vec![Event::AgentStarted {
            id: s(&v, "tool_use_id"),
            description: s(&v, "description"),
            kind: s(&v, "subagent_type"),
            background: v.get("is_backgrounded").and_then(Value::as_bool).unwrap_or(false),
        }],
        (Some("system"), Some("task_notification")) => {
            vec![Event::AgentDone { id: s(&v, "tool_use_id"), status: s(&v, "status"), summary: s(&v, "summary") }]
        }
        (Some("assistant"), _) => blocks()
            .iter()
            .filter_map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => Some(Event::Text { text: s(b, "text"), parent: parent.clone() }),
                Some("tool_use") => Some(Event::ToolUse {
                    id: s(b, "id"),
                    name: s(b, "name"),
                    input: b.get("input").cloned().unwrap_or(Value::Null),
                    parent: parent.clone(),
                }),
                _ => None,
            })
            .collect(),
        (Some("user"), _) => blocks()
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            .map(|b| Event::ToolResult {
                id: s(b, "tool_use_id"),
                error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                text: content_text(b.get("content")),
            })
            .collect(),
        (Some("result"), _) => vec![Event::Result {
            text: v.get("result").and_then(Value::as_str).map(str::to_string),
            error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            session_id: v.get("session_id").and_then(Value::as_str).map(str::to_string),
        }],
        _ => Vec::new(),
    }
}

/// `text` on one line, cut at `max` characters.
pub fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max { flat } else { format!("{}…", flat.chars().take(max).collect::<String>()) }
}

/// A call as a row shows it: `Bash(echo hi)`, `Read(src/a.rs)`,
/// `engines.post(...)` for an MCP tool.
pub fn call_line(name: &str, input: &Value) -> String {
    let key = ["command", "file_path", "path", "pattern", "url", "description", "query", "prompt"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str).filter(|v| !v.trim().is_empty()));
    let arg = match key {
        Some(v) => v.to_string(),
        None => match input {
            Value::Object(m) if m.is_empty() => String::new(),
            Value::Null => String::new(),
            other => other.to_string(),
        },
    };
    let name = match name.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
        Some((server, tool)) => format!("{server}.{tool}"),
        None => name.to_string(),
    };
    format!("{name}({})", one_line(&arg, LINE_CHARS))
}

/// How a row is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    Running,
    /// Its result's first line.
    Done(String),
    Failed(String),
}

/// One tool call.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: String,
    /// `Bash(echo hi)`.
    pub call: String,
    /// The agent that ran it (its `Task`/`Agent` call's id), if not the session.
    pub parent: Option<String>,
    pub state: RowState,
}

/// One sub-agent, as a read-only participant.
#[derive(Clone, Debug, PartialEq)]
pub struct Agent {
    /// Its `Task`/`Agent` call's id.
    pub id: String,
    /// The name it posts under (`agent-say-ok`).
    pub name: String,
    pub description: String,
    pub kind: String,
    pub background: bool,
    /// `completed: ok`, once it ended.
    pub done: Option<String>,
}

/// What changed with a line, for the shell (posts, events).
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Init,
    Row(usize),
    AgentStarted(usize),
    AgentDone(usize),
    Text,
    Result,
}

/// A run as far as it has gone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub init: Option<Init>,
    pub rows: Vec<Row>,
    pub agents: Vec<Agent>,
    /// The session's own text blocks (not its agents'), in order.
    pub texts: Vec<String>,
    /// Every result: its text and whether it is an error.
    pub results: Vec<(String, bool)>,
    pub session_id: Option<String>,
}

impl Live {
    /// Read one line; what changed.
    pub fn feed(&mut self, line: &str) -> Vec<Change> {
        parse_line(line).into_iter().filter_map(|e| self.apply(e)).collect()
    }

    fn apply(&mut self, e: Event) -> Option<Change> {
        match e {
            Event::Init(init) => {
                self.session_id = init.session_id.clone().or(self.session_id.take());
                self.init = Some(init);
                Some(Change::Init)
            }
            Event::Text { text, parent: None } if !text.trim().is_empty() => {
                self.texts.push(text);
                Some(Change::Text)
            }
            Event::Text { .. } => None,
            Event::ToolUse { id, name, input, parent } => {
                if AGENT_TOOLS.contains(&name.as_str()) && !self.agents.iter().any(|a| a.id == id) {
                    let description = input.get("description").and_then(Value::as_str).unwrap_or("agent").to_string();
                    let kind = input.get("subagent_type").and_then(Value::as_str).unwrap_or("general-purpose").to_string();
                    let taken: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();
                    let name = crate::files::new_id(&format!("agent {description}"), &taken);
                    self.agents.push(Agent { id: id.clone(), name, description, kind, background: false, done: None });
                }
                self.rows.push(Row { call: call_line(&name, &input), id, name, parent, state: RowState::Running });
                Some(Change::Row(self.rows.len() - 1))
            }
            Event::ToolResult { id, error, text } => {
                let r = self.rows.iter().position(|r| r.id == id)?;
                let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or(if error { "error" } else { "done" });
                let line = one_line(first, LINE_CHARS);
                self.rows[r].state = if error { RowState::Failed(line.clone()) } else { RowState::Done(line.clone()) };
                // A foreground agent ends with its call.
                if let Some(a) = self.agents.iter().position(|a| a.id == id && !a.background && a.done.is_none()) {
                    self.agents[a].done = Some(if error { format!("failed: {line}") } else { format!("completed: {line}") });
                    return Some(Change::AgentDone(a));
                }
                Some(Change::Row(r))
            }
            Event::AgentStarted { id, description, kind, background } => {
                let a = match self.agents.iter().position(|a| a.id == id) {
                    Some(a) => a,
                    None => {
                        let taken: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();
                        let name = crate::files::new_id(&format!("agent {description}"), &taken);
                        self.agents.push(Agent { id, name, description: description.clone(), kind: kind.clone(), background, done: None });
                        self.agents.len() - 1
                    }
                };
                self.agents[a].background = background;
                if !kind.is_empty() {
                    self.agents[a].kind = kind;
                }
                Some(Change::AgentStarted(a))
            }
            Event::AgentDone { id, status, summary } => {
                let a = self.agents.iter().position(|a| a.id == id)?;
                if self.agents[a].done.is_some() {
                    return None;
                }
                let status = if status.is_empty() { "completed".to_string() } else { status };
                self.agents[a].done = Some(format!("{status}: {}", one_line(&summary, LINE_CHARS)));
                Some(Change::AgentDone(a))
            }
            Event::Result { text, error, session_id } => {
                self.session_id = session_id.or(self.session_id.take());
                self.results.push((text.unwrap_or_default(), error));
                Some(Change::Result)
            }
        }
    }

    /// What it is doing now: the newest running row, else `working`.
    pub fn doing(&self) -> String {
        match self.rows.iter().rev().find(|r| r.state == RowState::Running) {
            Some(r) => format!("running {}", r.call),
            None => "working".into(),
        }
    }

    /// The rows as one tools message, like a dry engine's calls:
    /// `Bash(echo hi) → hi`, an agent's rows under it. Empty: no calls.
    pub fn rows_text(&self) -> String {
        self.rows
            .iter()
            .map(|r| {
                let indent = if r.parent.is_some() { "  " } else { "" };
                let result = match &r.state {
                    RowState::Running => "(still running when the turn ended)".to_string(),
                    RowState::Done(l) => l.clone(),
                    RowState::Failed(l) => format!("error: {l}"),
                };
                format!("{indent}{} → {result}", r.call)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The `/` list for the composer: slash commands, then skills not
    /// already there.
    pub fn slash(&self) -> Vec<String> {
        let Some(init) = &self.init else { return Vec::new() };
        let mut out = init.slash_commands.clone();
        for s in &init.skills {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
        out
    }
}

/// The metadata key a session's `/` list is kept under (space separated).
pub const SLASH_META: &str = "init.slash_commands";

/// The `/` list kept in an engine's metadata.
pub fn slash_of(meta: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    meta.get(SLASH_META).map(|s| s.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/data/claude-stream.jsonl");

    fn run() -> (Live, Vec<Change>) {
        let mut live = Live::default();
        let changes = SAMPLE.lines().flat_map(|l| live.feed(l)).collect();
        (live, changes)
    }

    #[test]
    fn a_recorded_run_gives_rows_agents_init_and_both_results() {
        let (live, changes) = run();
        let init = live.init.clone().expect("an init event");
        assert_eq!(init.model, "claude-haiku-5-5");
        assert_eq!(init.slash_commands, ["compact", "context", "review", "caveman"]);
        assert_eq!(live.slash(), ["compact", "context", "review", "caveman"], "a skill that is a command is not listed twice");
        assert_eq!(live.session_id.as_deref(), Some("sess-1"));

        let calls: Vec<(&str, &RowState)> = live.rows.iter().map(|r| (r.call.as_str(), &r.state)).collect();
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(calls[0].0, "Bash(echo hi)");
        assert!(matches!(calls[0].1, RowState::Failed(l) if l.starts_with("PreToolUse:Bash hook error")), "{:?}", calls[0].1);
        assert_eq!(calls[1], ("Bash(echo hi)", &RowState::Done("hi".into())));
        assert_eq!(calls[2].0, "Agent(say ok)");

        // The agent ran in the background: its launch line closes the row,
        // the notification ends the agent.
        assert_eq!(live.agents.len(), 1);
        let a = &live.agents[0];
        assert_eq!((a.name.as_str(), a.kind.as_str(), a.background), ("agent-say-ok", "general-purpose", true));
        assert_eq!(a.done.as_deref(), Some("completed: ok"));
        assert!(changes.contains(&Change::AgentStarted(0)) && changes.contains(&Change::AgentDone(0)), "{changes:?}");

        // The agent's own text is not the session's.
        assert!(!live.texts.iter().any(|t| t == "ok"), "{:?}", live.texts);
        assert_eq!(live.results.len(), 2);
        assert_eq!(live.results[1], ("Subagent returned: ok".to_string(), false));
        assert!(live.rows_text().starts_with("Bash(echo hi) → error: PreToolUse:Bash hook error"), "{}", live.rows_text());
        assert!(
            live.rows_text().contains("\nBash(echo hi) → hi\nAgent(say ok) → Async agent launched successfully."),
            "{}",
            live.rows_text()
        );
        assert_eq!(live.doing(), "working");
    }

    #[test]
    fn a_running_row_is_what_it_is_doing_and_a_foreground_agent_ends_with_its_result() {
        let mut live = Live::default();
        live.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Task","input":{"description":"find it","subagent_type":"Explore","prompt":"x"}}]}}"#);
        live.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Grep","input":{"pattern":"fn main"}}]},"parent_tool_use_id":"t1"}"#);
        assert_eq!(live.doing(), "running Grep(fn main)");
        assert_eq!(live.rows[1].parent.as_deref(), Some("t1"));
        live.feed(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":"src/main.rs:1"}]}}"#);
        let c = live.feed(r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"In src/main.rs.\nmore"}]}]}}"#);
        assert_eq!(c, [Change::AgentDone(0)]);
        assert_eq!(live.agents[0].done.as_deref(), Some("completed: In src/main.rs."));
        assert_eq!(live.rows_text(), "Task(find it) → In src/main.rs.\n  Grep(fn main) → src/main.rs:1");
        // Lines that are not events are skipped.
        assert!(live.feed("not json").is_empty());
        assert!(live.feed(r#"{"type":"rate_limit_event"}"#).is_empty());
    }

    #[test]
    fn calls_read_short() {
        assert_eq!(call_line("mcp__engines__post", &serde_json::json!({"text": "hi"})), "engines.post({\"text\":\"hi\"})");
        assert_eq!(call_line("Read", &serde_json::json!({"file_path": "/a/b.rs"})), "Read(/a/b.rs)");
        assert_eq!(call_line("TodoWrite", &serde_json::json!({})), "TodoWrite()");
        assert_eq!(one_line("a\n  b", 10), "a b");
        assert_eq!(one_line(&"x".repeat(12), 10), format!("{}…", "x".repeat(10)));
    }
}
