//! Subscriptions: every model and program an engine can be run by
//! (docs/elements.md, "subscriptions"). One row per `models.json` model
//! (an API model, a local server, a CLI subscription) and per
//! `sessions.json` program (a session), each with whether it is present
//! here and why not, and its usage where dry's own records have it.
//!
//! The shell builds the value (`build`, with its probes: a program on
//! PATH, a local server that answers, a key variable that is set) and
//! keeps it in its registry (`crate::capability::Registry::subscriptions`),
//! the same route as the environment; the `subscriptions` element shows
//! it (`text`), and `model=` on `engine_start` / `engine_call` names a row
//! (`choose`).
//!
//! Never a key: a row knows only the *name* of a key variable, and the
//! shell's probe says only whether it is set.
//!
//! Pure: the files' values, the probes' answers and the spans come in.

use std::collections::BTreeMap;

use crate::inference::OpenAi;
use crate::models::{ModelConfig, Models};
use crate::session::Sessions;
use crate::spans::{Kind as SpanKind, Span};

/// What kind of row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An HTTP model (a provider's API, or a local server).
    Api,
    /// A CLI subscription answering as a model (`crate::cli`).
    Cli,
    /// A program a session engine is run by (`crate::session`).
    Session,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Api => "api",
            Kind::Cli => "cli",
            Kind::Session => "session",
        }
    }
}

/// Whether a row can run here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    Present,
    /// It cannot: why.
    Absent(String),
    /// Not checked here: why (a browser build cannot look).
    Unknown(String),
}

impl Presence {
    pub fn word(&self) -> &'static str {
        match self {
            Presence::Present => "present",
            Presence::Absent(_) => "absent",
            Presence::Unknown(_) => "unknown",
        }
    }

    fn why(&self) -> Option<&str> {
        match self {
            Presence::Present => None,
            Presence::Absent(w) | Presence::Unknown(w) => Some(w),
        }
    }
}

/// What a shell checks to say whether a row is present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// A program on PATH (or a path): a CLI or a session program.
    Program(String),
    /// A local server (`127.0.0.1`, `localhost`): `GET <base>/models`.
    Local(String),
    /// A remote API whose key is in this variable: only whether it is set.
    Key(String),
    /// Nothing to check (a remote API with no key variable).
    Nothing,
    /// The row does not read: why (it is absent).
    Broken(String),
}

/// One row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The name `model=` takes.
    pub name: String,
    pub kind: Kind,
    /// What runs it: the model (and a server of its own), the CLI and its
    /// model, the program.
    pub what: String,
    pub presence: Presence,
    /// Output tokens per second over the usage window (`usage`).
    pub tokens_per_s: Option<f64>,
    /// Cost in US dollars over the usage window, where recorded.
    pub cost_usd: Option<f64>,
    /// A session row: how many of its session engines are busy now.
    pub running: usize,
}

/// What `model=` chose.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    /// The child's inference comes from this model row.
    Model { name: String, inference: OpenAi },
    /// The child is a session engine of this program.
    Session { tool: String },
}

/// Every row, the default's name, and why a file did not read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Subscriptions {
    /// `models.json`'s default (the model a child gets with no `model=`).
    pub default: String,
    pub rows: Vec<Row>,
    /// Why `models.json` or `sessions.json` did not read (left out of the rows).
    pub notes: Vec<String>,
    /// The files as read, for `choose` (names of key variables only).
    pub models: Models,
    pub sessions: Sessions,
}

/// How many days of usage a row shows.
pub const USAGE_DAYS: u32 = 7;

/// Is `base_url` on this machine (`127.0.0.1`, `localhost`, `[::1]`)?
pub fn is_local(base_url: &str) -> bool {
    host_of(base_url).is_some_and(|h| {
        let name = h.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(h, |(n, _)| n);
        matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "0.0.0.0")
    })
}

/// `host[:port]` of a URL.
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    rest.split('/').next().filter(|h| !h.is_empty())
}

/// What a model row checks.
pub fn check(models: &Models, m: &ModelConfig) -> Check {
    if m.is_cli() {
        let Some(provider) = m.provider.as_deref().filter(|p| !p.is_empty()) else {
            return Check::Broken("a cli model needs a provider".into());
        };
        let Some(cli) = crate::cli::Cli::named(provider) else {
            return Check::Broken(format!("no CLI provider `{provider}`"));
        };
        return Check::Program(m.program.clone().unwrap_or(cli.program));
    }
    match models.inference(m) {
        Err(why) => Check::Broken(why),
        Ok(ai) if is_local(&ai.base_url) => Check::Local(ai.base_url),
        Ok(ai) => ai.key_env.map_or(Check::Nothing, Check::Key),
    }
}

/// A session program's check: its command on PATH.
pub fn program_check(tool: &crate::session::Tool) -> Check {
    match tool.start.first().filter(|p| !p.is_empty()) {
        Some(p) => Check::Program(p.clone()),
        None => Check::Broken("sessions.json: its start command is empty".into()),
    }
}

/// What a model row shows it runs.
fn model_what(m: &ModelConfig) -> String {
    if m.is_cli() {
        let provider = m.provider.clone().unwrap_or_default();
        return match m.model.as_deref().filter(|x| !x.is_empty()) {
            Some(model) => format!("{provider} {model}"),
            None => provider,
        };
    }
    let model = m.parsed().map(|id| id.model.to_string()).unwrap_or_else(|_| m.id.clone());
    match m.base_url.as_deref().and_then(host_of) {
        Some(host) => format!("{model} @{host}"),
        None => model,
    }
}

/// What a session row shows it runs: its description up to the first
/// comma or bracket (`Claude Code`), else its name.
fn session_what(tool: &crate::session::Tool) -> String {
    let short = tool.description.split([',', '(', ':', '.']).next().unwrap_or("").trim();
    let short: String = short.chars().take(40).collect();
    if short.is_empty() { tool.name.clone() } else { short }
}

/// The rows of `models` and `sessions` (either may not have read: its
/// error is a note), each present or not by `probe`.
pub fn build(
    models: Result<Models, String>,
    sessions: Result<Sessions, String>,
    probe: &mut dyn FnMut(&Check) -> Presence,
) -> Subscriptions {
    let mut out = Subscriptions::default();
    match models {
        Ok(m) => out.models = m,
        Err(why) => out.notes.push(format!("models.json: {why}")),
    }
    match sessions {
        Ok(s) => out.sessions = s,
        Err(why) => out.notes.push(format!("sessions.json: {why}")),
    }
    out.default = out.models.default_model().map(|m| m.name.clone()).unwrap_or_default();
    for m in &out.models.models {
        let presence = match check(&out.models, m) {
            Check::Broken(why) => Presence::Absent(why),
            c => probe(&c),
        };
        let kind = if m.is_cli() { Kind::Cli } else { Kind::Api };
        out.rows.push(Row { name: m.name.clone(), kind, what: model_what(m), presence, tokens_per_s: None, cost_usd: None, running: 0 });
    }
    for t in &out.sessions.tools {
        let presence = match program_check(t) {
            Check::Broken(why) => Presence::Absent(why),
            c => probe(&c),
        };
        out.rows.push(Row { name: t.name.clone(), kind: Kind::Session, what: session_what(t), presence, tokens_per_s: None, cost_usd: None, running: 0 });
    }
    out
}

impl Subscriptions {
    pub fn get(&self, name: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.name == name)
    }

    /// The rows that can be chosen (present, or not checked here).
    pub fn present(&self) -> Vec<&str> {
        self.rows.iter().filter(|r| !matches!(r.presence, Presence::Absent(_))).map(|r| r.name.as_str()).collect()
    }

    /// Usage from dry's own records: `spans` (the caller keeps the last
    /// `USAGE_DAYS` days). A model row's are its model calls (the model
    /// and the server's provider name it was called with), a session
    /// row's its program's session turns. A row with none shows none.
    pub fn usage(&mut self, spans: &[Span]) {
        let llm: Vec<&Span> = spans.iter().filter(|s| s.kind == SpanKind::Llm).collect();
        for row in &mut self.rows {
            let mine: Vec<Span> = match row.kind {
                Kind::Session => {
                    llm.iter().filter(|s| s.str("purpose") == Some("session") && s.str("provider") == Some(&row.name)).map(|s| (*s).clone()).collect()
                }
                Kind::Api | Kind::Cli => {
                    let Some(ai) = self.models.get(&row.name).and_then(|m| self.models.inference(m).ok()) else { continue };
                    let provider = crate::spans::provider(&ai.base_url);
                    llm.iter()
                        .filter(|s| {
                            s.str("purpose") != Some("session") && s.str("model") == Some(&ai.model) && s.str("provider") == Some(&provider)
                        })
                        .map(|s| (*s).clone())
                        .collect()
                }
            };
            let stats = crate::analytics::models(&mine);
            let completion: u64 = stats.iter().map(|r| r.completion_tokens).sum();
            // Tokens/s across the rows: their generation time added up.
            let gen_s: f64 = stats.iter().filter_map(|r| r.tokens_per_s.map(|t| r.completion_tokens as f64 / t)).sum();
            row.tokens_per_s = (gen_s > 0.0 && completion > 0).then(|| completion as f64 / gen_s);
            let costs: Vec<f64> = stats.iter().filter_map(|r| r.cost_usd).collect();
            row.cost_usd = (!costs.is_empty()).then(|| costs.iter().sum());
        }
    }

    /// How many session engines of each program are busy now.
    pub fn set_running(&mut self, running: &BTreeMap<String, usize>) {
        for row in self.rows.iter_mut().filter(|r| r.kind == Kind::Session) {
            row.running = running.get(&row.name).copied().unwrap_or(0);
        }
    }

    /// Each row as a line, the columns lined up.
    pub fn lines(&self) -> Vec<String> {
        let name_w = self.rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(0);
        let what_w = self.rows.iter().map(|r| r.what.chars().count()).max().unwrap_or(0);
        self.rows
            .iter()
            .map(|r| {
                let mut tail: Vec<String> = Vec::new();
                if let Some(why) = r.presence.why() {
                    tail.push(why.to_string());
                }
                if let Some(t) = r.tokens_per_s {
                    tail.push(format!("{t:.0} tok/s"));
                }
                if let Some(c) = r.cost_usd {
                    tail.push(format!("{} in {USAGE_DAYS}d", dollars(c)));
                }
                if r.running > 0 {
                    tail.push(format!("{} running", r.running));
                }
                let line = format!(
                    "{:name_w$}  {:7}  {:what_w$}  {:7}  {}",
                    r.name,
                    r.kind.word(),
                    r.what,
                    r.presence.word(),
                    tail.join("  ")
                );
                line.trim_end().to_string()
            })
            .collect()
    }

    /// The header line.
    pub fn header(&self) -> String {
        if self.default.is_empty() { "subscriptions (no default model)".into() } else { format!("subscriptions (default: {})", self.default) }
    }

    /// The element's text: the header, a line per row, the notes.
    pub fn text(&self) -> String {
        let mut out = vec![self.header()];
        if self.rows.is_empty() {
            out.push("(none: models.json and sessions.json list nothing)".into());
        }
        out.extend(self.lines());
        out.extend(self.notes.iter().map(|n| format!("note: {n}")));
        out.join("\n")
    }

    /// What `model=name` runs a child with, or why it cannot (with the
    /// rows that can).
    pub fn choose(&self, name: &str) -> Result<Choice, String> {
        let name = name.trim();
        let can = || {
            let p = self.present();
            if p.is_empty() { "none is present".to_string() } else { format!("present: {}", p.join(", ")) }
        };
        let row = self.get(name).ok_or_else(|| format!("model=\"{name}\" is not a subscription ({}); pick a name from the subscriptions section", can()))?;
        if let Presence::Absent(why) = &row.presence {
            return Err(format!("model=\"{name}\" is absent here ({why}); {}", can()));
        }
        match row.kind {
            Kind::Session => Ok(Choice::Session { tool: row.name.clone() }),
            Kind::Api | Kind::Cli => {
                let m = self.models.get(name).ok_or_else(|| format!("model=\"{name}\" is not in models.json"))?;
                let inference = self.models.inference(m).map_err(|why| format!("model=\"{name}\" does not read: {why}"))?;
                Ok(Choice::Model { name: name.to_string(), inference })
            }
        }
    }
}

/// `$0`, `$0.41`, `$12.30`.
fn dollars(c: f64) -> String {
    if c < 0.005 { "$0".into() } else { format!("${c:.2}") }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const MODELS: &str = r#"{
        "default": "local",
        "providers": { "openai": { "base_url": "https://api.openai.com/v1", "key_env": "OPENAI_API_KEY" } },
        "models": [
            { "name": "local", "id": "openai/c/ripple-65b", "base_url": "http://127.0.0.1:1234/v1" },
            { "name": "gpt", "id": "openai/r/gpt-5-mini" },
            { "name": "claude/haiku", "kind": "cli", "provider": "claude", "model": "haiku" }
        ]
    }"#;
    pub const SESSIONS: &str = r#"{ "tools": [
        { "name": "claude", "description": "Claude Code, headless (claude -p)", "start": ["claude", "-p", "{prompt}"] },
        { "name": "hermes", "description": "Hermes, one-shot", "start": ["hermes", "-z", "{prompt}"] }
    ] }"#;

    /// The rows a shell would see: the local server up, no OpenAI key,
    /// claude on PATH, hermes not.
    pub fn sample() -> Subscriptions {
        let mut probe = |c: &Check| match c {
            Check::Local(_) => Presence::Present,
            Check::Key(v) => Presence::Absent(format!("{v} not set")),
            Check::Program(p) if p == "claude" => Presence::Present,
            Check::Program(p) => Presence::Absent(format!("{p} is not on PATH")),
            _ => Presence::Present,
        };
        build(Models::from_json(MODELS), Sessions::from_json(SESSIONS), &mut probe)
    }

    #[test]
    fn rows_are_every_model_then_every_program_with_presence_and_why() {
        let mut s = sample();
        s.set_running(&BTreeMap::from([("claude".to_string(), 2)]));
        assert_eq!(
            s.text(),
            "subscriptions (default: local)\n\
             local         api      ripple-65b @127.0.0.1:1234  present\n\
             gpt           api      gpt-5-mini                  absent   OPENAI_API_KEY not set\n\
             claude/haiku  cli      claude haiku                present\n\
             claude        session  Claude Code                 present  2 running\n\
             hermes        session  Hermes                      absent   hermes is not on PATH"
        );
    }

    #[test]
    fn checks_are_a_program_a_local_server_or_a_key_variable() {
        let m = Models::from_json(MODELS).unwrap();
        let checks: Vec<Check> = m.models.iter().map(|x| check(&m, x)).collect();
        assert_eq!(
            checks,
            [Check::Local("http://127.0.0.1:1234/v1".into()), Check::Key("OPENAI_API_KEY".into()), Check::Program("claude".into())]
        );
        assert!(is_local("http://localhost:11434/v1") && is_local("http://[::1]:8080") && !is_local("https://api.openai.com/v1"));
        let broken = build(Models::from_json(r#"{"models":[{"name":"x","id":"nowhere/c/m"}]}"#), Err("missing".into()), &mut |_| Presence::Present);
        assert!(matches!(&broken.rows[0].presence, Presence::Absent(w) if w.contains("nowhere")), "{:?}", broken.rows);
        assert!(broken.text().ends_with("note: sessions.json: missing"), "{}", broken.text());
    }

    #[test]
    fn usage_comes_only_from_recorded_calls_of_that_row() {
        let mut s = sample();
        let call = |model: &str, provider: &str, purpose: &str, completion: f64, ms: i64, cost: Option<f64>| {
            let mut sp = Span::new(SpanKind::Llm, format!("{model}{ms}"), "e", 0, ms);
            sp.set("model", model);
            sp.set("provider", provider);
            sp.set("purpose", purpose);
            sp.set("completion_tokens", completion);
            if let Some(c) = cost {
                sp.set("cost_usd", c);
            }
            sp
        };
        let spans = vec![
            call("ripple-65b", "127.0.0.1:1234", "step", 120.0, 10_000, None),
            call("ripple-65b", "elsewhere:1", "step", 999.0, 1_000, None),
            call("claude-opus", "claude", "session", 50.0, 5_000, Some(0.4123)),
        ];
        s.usage(&spans);
        let lines = s.lines();
        assert!(lines[0].ends_with("present  12 tok/s"), "{}", lines[0]);
        assert!(lines[1].ends_with("not set"), "no usage recorded, none shown: {}", lines[1]);
        assert!(lines[3].ends_with("present  10 tok/s  $0.41 in 7d"), "{}", lines[3]);
    }

    #[test]
    fn choose_names_a_present_row_and_refuses_the_rest_with_the_present_ones() {
        let s = sample();
        match s.choose("local").unwrap() {
            Choice::Model { name, inference } => {
                assert_eq!((name.as_str(), inference.model.as_str(), inference.base_url.as_str()), ("local", "ripple-65b", "http://127.0.0.1:1234/v1"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(s.choose("claude").unwrap(), Choice::Session { tool: "claude".into() });
        let unknown = s.choose("gpt-9").unwrap_err();
        assert!(unknown.contains("not a subscription") && unknown.contains("present: local, claude/haiku, claude"), "{unknown}");
        let absent = s.choose("gpt").unwrap_err();
        assert!(absent.contains("absent here (OPENAI_API_KEY not set)") && absent.contains("present: local"), "{absent}");
    }
}
