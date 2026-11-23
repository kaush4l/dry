//! Approvals: a worker's request to use a tool it may not use on its own
//! (a Claude Code session's permission prompt, `--permission-prompt-tool`
//! in `sessions.json`), decided by its engine's rules first, else by a
//! person.
//!
//! - **Rules.** A small allow list in the engine's spec (`engine.md`,
//!   `allow: Read, Glob, Grep, Bash(git status*)`): a tool name, or a name
//!   and a pattern its main input must match (`*` is anything). A rule
//!   that matches allows at once; nothing here ever denies on its own.
//! - **Else a person.** The host keeps an approval record, posts a notice
//!   to you on the engine's thread (its root's; addressed to you, so it
//!   calls no engine), and the request waits until someone allows
//!   or denies it (`POST /api/approvals/:id`). No timeout unless the spec
//!   says `approve_timeout: <seconds>`; a night run waits for you.
//! - **The answer** is what the permission prompt tool returns:
//!   `{"behavior":"allow","updatedInput":{...}}` or
//!   `{"behavior":"deny","message":"..."}`.
//!
//! Pure: rules, records and texts. The host keeps the records.

use serde_json::{Value, json};

/// What a new session engine allows without asking: tools that only read.
pub const DEFAULT_ALLOW: &str = "Read, Glob, Grep, LS, TodoWrite, mcp__engines__state, mcp__engines__artifact, mcp__engines__thread";

/// One allow rule: `Read`, `Bash(git status*)`, `mcp__engines__*`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub tool: String,
    pub pattern: Option<String>,
}

impl Rule {
    pub fn text(&self) -> String {
        match &self.pattern {
            Some(p) => format!("{}({p})", self.tool),
            None => self.tool.clone(),
        }
    }
}

/// The rules in an `allow:` value, comma separated.
pub fn parse_rules(text: &str) -> Vec<Rule> {
    text.split(',')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(|r| match r.split_once('(') {
            Some((tool, rest)) => Rule { tool: tool.trim().to_string(), pattern: Some(rest.trim_end_matches(')').to_string()) },
            None => Rule { tool: r.to_string(), pattern: None },
        })
        .collect()
}

/// `*` is any run of characters; everything else is itself.
pub fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// The input a rule's pattern is matched against: the command, the file,
/// the address, ...; else the whole input as JSON.
pub fn subject(input: &Value) -> String {
    ["command", "file_path", "path", "url", "pattern", "query"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .map_or_else(|| input.to_string(), str::to_string)
}

/// The rule that allows `tool` with `input`, if one does.
pub fn allowed_by(rules: &[Rule], tool: &str, input: &Value) -> Option<Rule> {
    // The thread tool's old name: a rule written for it allows the new one.
    let old = (tool == "mcp__engines__thread").then_some("mcp__engines__dashboard");
    rules
        .iter()
        .find(|r| (glob(&r.tool, tool) || old.is_some_and(|o| glob(&r.tool, o))) && r.pattern.as_ref().is_none_or(|p| glob(p, &subject(input))))
        .cloned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

impl Decision {
    pub fn parse(word: &str) -> Option<Decision> {
        match word.trim() {
            "allow" => Some(Decision::Allow),
            "deny" => Some(Decision::Deny),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
        }
    }
}

/// How a request was decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decided {
    pub decision: Decision,
    pub reason: String,
    /// `rule Read`, `a person`, `timeout`.
    pub by: String,
}

/// One request, from asking to its answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Approval {
    /// `a1`, `a2`, ... (the host's counter).
    pub id: String,
    pub engine: String,
    pub tool: String,
    pub input: Value,
    /// Unix seconds.
    pub asked: i64,
    /// Seconds to wait before denying; `None`: wait for a person.
    pub timeout: Option<u64>,
    pub decided: Option<Decided>,
}

impl Approval {
    /// What it asks, in a line: `Bash(rm -rf build)`.
    pub fn what(&self) -> String {
        crate::stream_json::call_line(&self.tool, &self.input)
    }

    /// The post that tells the owner.
    pub fn notice(&self) -> String {
        let wait = match self.timeout {
            Some(s) => format!(" (denied in {s}s if nobody answers)"),
            None => String::new(),
        };
        format!(
            "approval {}: {} asks to use {} — Allow or Deny it in the app{wait}",
            self.id,
            self.engine,
            crate::stream_json::one_line(&self.what(), 300)
        )
    }

    /// Denied because nobody answered in time, at unix `now`?
    pub fn timed_out(&self, now: i64) -> bool {
        self.decided.is_none() && self.timeout.is_some_and(|s| now - self.asked >= s as i64)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "engine": self.engine, "tool": self.tool, "input": self.input, "what": self.what(),
            "asked": self.asked, "timeout": self.timeout,
            "decision": self.decided.as_ref().map(|d| d.decision.word()),
            "reason": self.decided.as_ref().map(|d| d.reason.clone()),
            "by": self.decided.as_ref().map(|d| d.by.clone()),
        })
    }

    pub fn from_json(v: &Value) -> Option<Approval> {
        let decided = v.get("decision").and_then(Value::as_str).and_then(Decision::parse).map(|decision| Decided {
            decision,
            reason: v.get("reason").and_then(Value::as_str).unwrap_or_default().to_string(),
            by: v.get("by").and_then(Value::as_str).unwrap_or_default().to_string(),
        });
        Some(Approval {
            id: v.get("id")?.as_str()?.to_string(),
            engine: v.get("engine")?.as_str()?.to_string(),
            tool: v.get("tool")?.as_str()?.to_string(),
            input: v.get("input").cloned().unwrap_or(Value::Null),
            asked: v.get("asked").and_then(Value::as_i64).unwrap_or(0),
            timeout: v.get("timeout").and_then(Value::as_u64),
            decided,
        })
    }
}

/// What the permission prompt tool answers.
pub fn answer(d: &Decided, input: &Value) -> Value {
    match d.decision {
        Decision::Allow => json!({ "behavior": "allow", "updatedInput": input }),
        Decision::Deny => {
            json!({ "behavior": "deny", "message": if d.reason.is_empty() { format!("denied by {}", d.by) } else { d.reason.clone() } })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_allow_by_name_and_by_pattern_and_never_deny() {
        let rules = parse_rules("Read, Bash(git status*), mcp__engines__*, ");
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[1].text(), "Bash(git status*)");
        assert_eq!(allowed_by(&rules, "Read", &json!({"file_path": "/x"})).map(|r| r.text()), Some("Read".into()));
        assert!(allowed_by(&rules, "Bash", &json!({"command": "git status --short"})).is_some());
        assert!(allowed_by(&rules, "Bash", &json!({"command": "rm -rf /"})).is_none());
        assert!(allowed_by(&rules, "mcp__engines__post", &json!({})).is_some());
        assert!(allowed_by(&rules, "Edit", &json!({})).is_none());
        assert!(glob("a*c*e", "abcde") && !glob("a*c*e", "abcd") && glob("*", "") && glob("x", "x") && !glob("x", "xy"));
        assert!(allowed_by(&parse_rules(DEFAULT_ALLOW), "Grep", &json!({"pattern": "fn"})).is_some());
        // The thread tool: allowed by default, and by a rule for its old name.
        assert!(allowed_by(&parse_rules(DEFAULT_ALLOW), "mcp__engines__thread", &json!({})).is_some());
        assert!(allowed_by(&parse_rules("mcp__engines__dashboard"), "mcp__engines__thread", &json!({})).is_some());
    }

    #[test]
    fn a_request_tells_its_owner_waits_and_answers_as_the_tool_must() {
        let mut a = Approval {
            id: "a1".into(),
            engine: "claude-1".into(),
            tool: "Bash".into(),
            input: json!({"command": "rm build"}),
            asked: 100,
            timeout: None,
            decided: None,
        };
        assert_eq!(a.notice(), "approval a1: claude-1 asks to use Bash(rm build) — Allow or Deny it in the app");
        assert!(!a.timed_out(1_000_000), "no timeout: it waits");
        a.timeout = Some(60);
        assert!(!a.timed_out(159) && a.timed_out(160));
        assert!(a.notice().ends_with("(denied in 60s if nobody answers)"));
        let back = Approval::from_json(&a.to_json()).unwrap();
        assert_eq!(back, a);
        let yes = Decided { decision: Decision::Allow, reason: String::new(), by: "a person".into() };
        assert_eq!(answer(&yes, &a.input), json!({"behavior": "allow", "updatedInput": {"command": "rm build"}}));
        let no = Decided { decision: Decision::Deny, reason: String::new(), by: "timeout".into() };
        assert_eq!(answer(&no, &a.input), json!({"behavior": "deny", "message": "denied by timeout"}));
        a.decided = Some(no);
        assert_eq!(Approval::from_json(&a.to_json()).unwrap().decided.unwrap().by, "timeout");
    }
}
