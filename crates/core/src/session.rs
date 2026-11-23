//! Sessions: an engine whose replies come from an agent program that is
//! already built (claude, codex, pi, hermes, ...) instead of from a model
//! we call ourselves. The engine is the same object either way (a name,
//! a description, its messages); a session engine only also says which
//! program runs it and that program's own session id, so the next turn
//! continues the same conversation.
//!
//! The programs are described in `sessions.json` (no program is named in
//! the code): how to start a turn, how to continue one, and where its
//! output keeps the reply, the session id and any error. Everything else
//! the program reports (model, tokens, cost, timings, ...) is kept as the
//! engine's metadata, flattened to `dotted.key = value` lines, so the UI
//! can show all of it without knowing any of it.
//!
//! A program may also take a settings file of its own (`settings`, a
//! JSON template written per session: Claude Code's hooks that push dry's
//! snapshot into each turn), an environment (`env`: the engine's id for
//! the MCP server it starts), and report live (`live: "stream-json"`: one
//! event a line, read as they come by `crate::stream_json`).
//!
//! Pure: this builds the command line and reads the output. The shell
//! runs the program in between.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `sessions.json`: every program an engine can be run by.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sessions {
    pub tools: Vec<Tool>,
}

impl Sessions {
    pub fn from_json(text: &str) -> Result<Sessions, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }
}

/// How the program writes its output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    /// One JSON object (`claude -p --output-format json`).
    #[default]
    Json,
    /// One JSON object per line, as events (`codex exec --json`, `pi --mode json`).
    Jsonl,
    /// Plain text: all of it is the reply (`hermes -z`). Metadata can come
    /// from a JSON file the program writes (`{meta_file}` in the command).
    Text,
}

/// A value to take from the output: the one at `path` (dotted, numbers
/// index arrays: `message.content.0.text`), in an object whose `when`
/// paths all have those values. With JSONL the last match wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    pub path: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub when: BTreeMap<String, String>,
}

/// One program. In `start` and `resume`, `{prompt}` is the new message,
/// `{id}` the program's session id, `{meta_file}` a file it may write
/// its usage report to, `{settings}` the settings file written from
/// `settings`, `{engine}` the engine's id. In `settings` and `env`,
/// `{engine}`, `{dry}` (the `dry` command) and `{cursor}` (the session's
/// cursor file, `crate::snapshot`) are filled in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    /// A glyph for its card and its sessions' cards, used as it is.
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default)]
    pub description: String,
    /// The command for the first turn.
    pub start: Vec<String>,
    /// The command for later turns, once there is an id. Empty: every turn
    /// uses `start` (the program then forgets earlier turns).
    #[serde(default)]
    pub resume: Vec<String>,
    #[serde(default)]
    pub output: Output,
    /// Where the reply is. Empty with `Text` output: the whole output.
    #[serde(default)]
    pub reply: Vec<Pick>,
    /// Where the program's own session id is.
    #[serde(default)]
    pub id: Vec<Pick>,
    /// Where an error message is (the turn failed).
    #[serde(default)]
    pub error: Vec<Pick>,
    /// Metadata keys starting with one of these are not kept (events that
    /// only repeat the conversation, say).
    #[serde(default)]
    pub meta_skip: Vec<String>,
    /// How it reports while it runs: `stream-json` (`crate::stream_json`),
    /// or nothing (it reports when it ends).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<String>,
    /// A settings file written for each session (`{settings}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Value>,
    /// Environment variables the program runs with.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// `{name}` in `template` from `vars`, in one pass: a value is never read
/// again, so a prompt that holds `{id}` stays as typed. An unknown name
/// stays as written.
pub fn fill(template: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').map(|close| (&after[..close], close)) {
            Some((name, close)) if vars.contains_key(name) => {
                out.push_str(&vars[name]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Every string in `v` filled from `vars`.
fn fill_value(v: &Value, vars: &BTreeMap<&str, String>) -> Value {
    match v {
        Value::String(s) => Value::String(fill(s, vars)),
        Value::Array(items) => Value::Array(items.iter().map(|x| fill_value(x, vars)).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, x)| (k.clone(), fill_value(x, vars))).collect()),
        other => other.clone(),
    }
}

fn default_icon() -> String {
    "▣".into()
}

/// What an engine needs to be a session: the program, and the program's
/// session id once the first turn has given one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionRef {
    pub tool: String,
    pub id: Option<String>,
}

impl Tool {
    /// The command line for a turn: `resume` when there is an id and a
    /// resume command, else `start`. The prompt is one argument, never
    /// split, so nothing in it is read as a flag or by a shell.
    pub fn command(&self, prompt: &str, id: Option<&str>, meta_file: &str) -> Vec<String> {
        self.command_with(prompt, id, &BTreeMap::from([("meta_file", meta_file.to_string())]))
    }

    /// `command` with more values (`settings`, `engine`, ...).
    pub fn command_with(&self, prompt: &str, id: Option<&str>, vars: &BTreeMap<&str, String>) -> Vec<String> {
        let template = match id {
            Some(_) if !self.resume.is_empty() => &self.resume,
            _ => &self.start,
        };
        let mut vars = vars.clone();
        vars.insert("prompt", prompt.to_string());
        vars.insert("id", id.unwrap_or("").to_string());
        template.iter().map(|a| fill(a, &vars)).collect()
    }

    /// Does a turn hand the program a usage file to write?
    pub fn wants_meta_file(&self) -> bool {
        self.uses("meta_file")
    }

    /// Does a command name `{name}`?
    pub fn uses(&self, name: &str) -> bool {
        let token = format!("{{{name}}}");
        self.start.iter().chain(&self.resume).any(|a| a.contains(&token))
    }

    /// The settings file for one session, filled from `vars` (pretty JSON).
    pub fn settings_text(&self, vars: &BTreeMap<&str, String>) -> Option<String> {
        self.settings.as_ref().map(|v| serde_json::to_string_pretty(&fill_value(v, vars)).unwrap_or_default())
    }

    /// The environment for one session, filled from `vars`.
    pub fn env_with(&self, vars: &BTreeMap<&str, String>) -> Vec<(String, String)> {
        self.env.iter().map(|(k, v)| (k.clone(), fill(v, vars))).collect()
    }

    /// Does it report live as `crate::stream_json` reads?
    pub fn streams(&self) -> bool {
        self.live.as_deref() == Some("stream-json")
    }

    /// Read what the program printed (and its usage file, if any).
    pub fn read(&self, stdout: &str, meta_file: Option<&str>) -> Turn {
        let objects: Vec<Value> = match self.output {
            Output::Json => serde_json::from_str(stdout.trim()).into_iter().collect(),
            Output::Jsonl => stdout.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect(),
            Output::Text => Vec::new(),
        };
        let mut turn = Turn::default();
        for obj in &objects {
            // JSONL events are kept under their `type`, so one event's
            // `usage` does not overwrite another's.
            let prefix = match (self.output, obj.get("type").and_then(Value::as_str)) {
                (Output::Jsonl, Some(kind)) => kind.to_string(),
                _ => String::new(),
            };
            flatten(obj, &prefix, &mut turn.meta);
            if let Some(v) = pick(&self.reply, obj) {
                turn.reply = Some(v);
            }
            if let Some(v) = pick(&self.id, obj) {
                turn.id = Some(v);
            }
            if let Some(v) = pick(&self.error, obj) {
                turn.error = Some(v);
            }
        }
        if self.output == Output::Text && self.reply.is_empty() {
            let text = stdout.trim();
            turn.reply = (!text.is_empty()).then(|| text.to_string());
        }
        if let Some(Ok(v)) = meta_file.map(serde_json::from_str::<Value>) {
            flatten(&v, "usage", &mut turn.meta);
        }
        turn.meta.retain(|k, _| !self.meta_skip.iter().any(|s| k.starts_with(s.as_str())));
        turn
    }
}

/// One turn's result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    pub reply: Option<String>,
    pub id: Option<String>,
    pub error: Option<String>,
    /// Everything the program reported, `dotted.key -> value`.
    pub meta: BTreeMap<String, String>,
}

/// Longest metadata value kept (the reply has its own place).
pub const META_VALUE_CAP: usize = 300;

/// The first `pick` that matches `obj`, as text.
fn pick(picks: &[Pick], obj: &Value) -> Option<String> {
    picks.iter().find_map(|p| {
        let ok = p.when.iter().all(|(path, want)| get(obj, path).is_some_and(|v| scalar(v).as_deref() == Some(want.as_str())));
        if !ok {
            return None;
        }
        get(obj, &p.path).and_then(scalar).filter(|s| !s.trim().is_empty())
    })
}

/// The value at a dotted path.
fn get<'a>(obj: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(obj, |v, key| match v {
        Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
        _ => v.get(key),
    })
}

/// A string, number or bool as text; `null`, objects and arrays: none.
fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Every scalar under `v` as `prefix.key.0.key = value`.
fn flatten(v: &Value, prefix: &str, out: &mut BTreeMap<String, String>) {
    let join = |k: &str| if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
    match v {
        Value::Object(map) => map.iter().for_each(|(k, v)| flatten(v, &join(k), out)),
        Value::Array(items) => items.iter().enumerate().for_each(|(i, v)| flatten(v, &join(&i.to_string()), out)),
        Value::Null => {}
        _ => {
            if let Some(mut s) = scalar(v) {
                if s.chars().count() > META_VALUE_CAP {
                    s = s.chars().take(META_VALUE_CAP).collect::<String>() + "…";
                }
                out.insert(prefix.to_string(), s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Sessions {
        Sessions::from_json(include_str!("../../../sessions.json")).expect("sessions.json parses")
    }

    #[test]
    fn claude_streams_its_events_and_gets_hooks_mcp_and_approvals() {
        let claude = tools().get("claude").cloned().expect("claude in sessions.json");
        assert!(claude.streams());
        // The recorded run: the last result is the reply, the id and the
        // result's numbers are kept, the conversation's events are not.
        let turn = claude.read(include_str!("../tests/data/claude-stream.jsonl"), None);
        assert_eq!(turn.reply.as_deref(), Some("Subagent returned: ok"));
        assert_eq!(turn.id.as_deref(), Some("sess-1"));
        assert_eq!(turn.error, None);
        assert_eq!(turn.meta["result.total_cost_usd"], "0.016689930000000002");
        assert!(
            !turn.meta.keys().any(|k| k.starts_with("assistant") || k.starts_with("user.") || k.starts_with("system")),
            "{:?}",
            turn.meta.keys().collect::<Vec<_>>()
        );

        let vars = BTreeMap::from([
            ("settings", "/s/claude-1/settings.json".to_string()),
            ("engine", "claude-1".to_string()),
            ("dry", "/r/target/debug/dry".to_string()),
            ("cursor", "/s/claude-1/cursor".to_string()),
        ]);
        let resume = claude.command_with("hi {id}", Some("ad43"), &vars);
        assert_eq!(
            resume[..],
            [
                "claude",
                "-p",
                "hi {id}",
                "--settings",
                "/s/claude-1/settings.json",
                "--mcp-config",
                "mcp/dry.json",
                "mcp/settings.json",
                "mcp/engines.json",
                "--allowedTools",
                "mcp__dry",
                "mcp__settings",
                "mcp__engines",
                "--permission-prompt-tool",
                "mcp__engines__approve",
                "--output-format",
                "stream-json",
                "--verbose",
                "--resume",
                "ad43"
            ]
        );
        assert!(!claude.command_with("hi", None, &vars).contains(&"--resume".to_string()));
        assert!(claude.uses("settings") && !claude.wants_meta_file());
        let settings: Value = serde_json::from_str(&claude.settings_text(&vars).unwrap()).unwrap();
        let hook = |event: &str| settings["hooks"][event][0]["hooks"][0]["command"].as_str().unwrap_or_default().to_string();
        assert_eq!(hook("SessionStart"), "'/r/target/debug/dry' snapshot --engine 'claude-1' --start");
        assert_eq!(hook("UserPromptSubmit"), "'/r/target/debug/dry' snapshot --engine 'claude-1' --cursor '/s/claude-1/cursor'");
        assert_eq!(settings["hooks"]["SessionStart"][0]["matcher"], "startup|clear|compact", "a resumed turn keeps its start");
        assert_eq!(claude.env_with(&vars), [("DRY_ENGINE".to_string(), "claude-1".to_string())]);
    }

    #[test]
    fn filling_is_one_pass() {
        let vars = BTreeMap::from([("a", "{b}".to_string()), ("b", "x".to_string())]);
        assert_eq!(fill("{a}-{b}-{c}-{", &vars), "{b}-x-{c}-{");
        let old = Tool { start: vec!["p".into(), "{prompt}".into(), "{meta_file}".into()], ..Tool::default() };
        assert_eq!(old.command("say {meta_file}", None, "/m.json"), ["p", "say {meta_file}", "/m.json"]);
    }

    #[test]
    fn codex_events_give_the_agent_message_and_the_thread() {
        let codex = tools().get("codex").cloned().expect("codex in sessions.json");
        let out = concat!(
            r#"{"type":"thread.started","thread_id":"01a2"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"thinking"}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"ok"}}"#,
            "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":2}}"#,
        );
        let turn = codex.read(out, None);
        assert_eq!(turn.reply.as_deref(), Some("ok"));
        assert_eq!(turn.id.as_deref(), Some("01a2"));
        assert_eq!(turn.meta["turn.completed.usage.input_tokens"], "10");
        // A failed turn says why.
        let failed = codex.read(r#"{"type":"turn.failed","error":{"message":"Missing environment variable"}}"#, None);
        assert_eq!(failed.error.as_deref(), Some("Missing environment variable"));
        assert_eq!(failed.reply, None);
    }

    #[test]
    fn text_output_is_the_reply_and_the_usage_file_is_metadata() {
        let hermes = tools().get("hermes").cloned().expect("hermes in sessions.json");
        assert!(hermes.wants_meta_file());
        let turn = hermes.read("ok\n", Some(r#"{"model":"m","total_tokens":12}"#));
        assert_eq!(turn.reply.as_deref(), Some("ok"));
        assert_eq!(turn.meta["usage.total_tokens"], "12");
    }

    #[test]
    fn long_values_are_cut_and_skipped_keys_dropped() {
        let tool = Tool { output: Output::Json, meta_skip: vec!["big".into()], ..Tool::default() };
        let long = "x".repeat(META_VALUE_CAP + 50);
        let turn = tool.read(&format!(r#"{{"a":"{long}","big":{{"b":1}}}}"#), None);
        assert_eq!(turn.meta["a"].chars().count(), META_VALUE_CAP + 1);
        assert!(!turn.meta.contains_key("big.b"));
    }
}
