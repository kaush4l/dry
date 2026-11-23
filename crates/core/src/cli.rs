//! CLI models: a coding CLI in one-shot mode as an engine's model, instead
//! of an OpenAI-compatible server. For fast testing (local models are
//! slow): `claude -p`, `codex exec`, `copilot` answer one prompt and exit.
//!
//! A CLI model is a **stateless completion provider**: dry owns the
//! context, renders the prompt as for any model, and the CLI answers it.
//! It is not a session engine (`crate::session`), where the CLI runs its
//! own loop and keeps its own conversation.
//!
//! ## How it fits the HTTP path
//!
//! Like a base-URL override. A `models.json` entry
//!
//! ```json
//! { "name": "claude/haiku", "kind": "cli", "provider": "claude", "model": "haiku" }
//! ```
//!
//! becomes an `OpenAi` whose `base_url` is `cli:claude` (`base_url`; a
//! `program` override is `cli:claude?program=/path/to/claude`). Its
//! `request` is then the usual Chat Completions body, sent to that URL;
//! the shell's transport sees the `cli:` scheme and runs the program
//! instead of POSTing (`dry_tools::cli`), and gives back a Chat
//! Completions body (`completion_body`), so `OpenAi::reply`, the runner
//! and the stream reader work unchanged.
//!
//! ## One caller, per-provider overrides
//!
//! `Cli` describes a program: its fixed args, the args that make it
//! answer instead of act (`quiet`: no tools, no agents, no project
//! instructions, no saved session), the model flag, how the system text
//! and the prompt go in, and how its output reads. `Cli::claude()`,
//! `Cli::codex()`, `Cli::copilot()` are the overrides; `invocation` builds
//! the argv (an array: no shell, nothing interpolated) and stdin.
//!
//! | provider | prompt | system | output | streams |
//! |---|---|---|---|---|
//! | claude  | stdin | `--system-prompt` (replaces Claude Code's own) | stream-json events | text deltas (`--include-partial-messages`) |
//! | codex   | stdin (`-`) | `-c developer_instructions=<toml string>` | JSONL events (`--json`) | whole message at the end |
//! | copilot | stdin | inline, above the prompt (no flag) | JSONL events | text deltas |
//!
//! ## Flattening
//!
//! The body's `system` messages (joined by a blank line) are the system
//! text. The other messages are the prompt: one user message is sent as
//! it is; a longer conversation is a transcript, oldest first, each
//! message in a `<user>`/`<assistant>` tag (a tool result was sent as a
//! user message, so it stays one), then a line asking for the next
//! assistant message only (`flatten`, tested below).
//!
//! Sampling settings (`temperature`, `max_tokens`, ...) have no flag on
//! these CLIs and are not passed. Pure: nothing here runs a program.

use serde_json::{Value, json};

use crate::stream::Delta;

/// The scheme of a CLI model's `base_url`.
pub const SCHEME: &str = "cli:";

/// Is this base URL a CLI model's?
pub fn is_cli(base_url: &str) -> bool {
    base_url.starts_with(SCHEME)
}

/// The base URL for `provider`, with its program when it overrides the
/// provider's own (`cli:claude`, `cli:claude?program=/opt/claude`).
pub fn base_url(provider: &str, program: Option<&str>) -> String {
    match program.filter(|p| !p.is_empty()) {
        Some(p) => format!("{SCHEME}{provider}?program={p}"),
        None => format!("{SCHEME}{provider}"),
    }
}

/// A CLI base URL (or a URL under it, like `cli:claude/models`) taken
/// apart: the provider and the program override.
pub fn parse_base(url: &str) -> Option<(String, Option<String>)> {
    let rest = url.strip_prefix(SCHEME)?;
    let (head, program) = match rest.split_once("?program=") {
        Some((h, p)) => (h, Some(p.to_string())),
        None => (rest, None),
    };
    let provider = head.split('/').next().unwrap_or_default();
    (!provider.is_empty()).then(|| (provider.to_string(), program))
}

/// The CLI a base URL names, with its program override applied.
pub fn from_base(url: &str) -> Result<Cli, String> {
    let (provider, program) = parse_base(url).ok_or_else(|| format!("{url} is not a cli: model"))?;
    let mut cli = Cli::named(&provider).ok_or_else(|| format!("no CLI provider \"{provider}\" (there are {})", PROVIDERS.join(", ")))?;
    if let Some(p) = program {
        cli.program = p;
    }
    Ok(cli)
}

/// The providers there are overrides for.
pub const PROVIDERS: [&str; 3] = ["claude", "codex", "copilot"];

/// How the prompt reaches the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptIn {
    /// Written to its stdin, which is then closed (no length limit).
    Stdin,
    /// The last argument.
    Arg,
}

/// How the system text reaches the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemIn {
    /// `<flag> <text>`.
    Flag(String),
    /// `-c <key>=<text as a TOML string>` (codex's config override).
    Config(String),
    /// Above the prompt, in a `<system>` tag: the program has no flag.
    Inline,
}

/// How the program's output reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// The whole output is the reply.
    Text,
    /// One JSON object; the reply at this pointer (`/result`).
    Json(&'static str),
    /// One JSON event a line, in this program's words.
    Events(Dialect),
}

/// Whose event lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// `claude -p --output-format stream-json --verbose --include-partial-messages`.
    Claude,
    /// `codex exec --json`.
    Codex,
    /// `copilot --output-format json`.
    Copilot,
}

/// A coding CLI as a completion provider.
#[derive(Clone, Debug, PartialEq)]
pub struct Cli {
    /// The provider's name (`claude`).
    pub name: String,
    /// The program: a name looked up on `PATH`, or a path.
    pub program: String,
    /// The args that run it once and set its output format.
    pub args: Vec<String>,
    /// The args that make it answer instead of act: no tools, no agents,
    /// no MCP servers, no project instructions, nothing saved.
    pub quiet: Vec<String>,
    /// `--model`; `None`: the model cannot be chosen.
    pub model_flag: Option<String>,
    pub system: SystemIn,
    pub prompt: PromptIn,
    pub output: Output,
    /// Its context window when the setting does not say (tokens).
    pub context: u32,
    /// Model names it takes (aliases), for Detect; empty: it does not say.
    pub models: Vec<String>,
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

impl Cli {
    /// The override for `name`, if there is one.
    pub fn named(name: &str) -> Option<Cli> {
        match name {
            "claude" => Some(Cli::claude()),
            "codex" => Some(Cli::codex()),
            "copilot" => Some(Cli::copilot()),
            _ => None,
        }
    }

    /// Claude Code (`claude --help`, 2.1.x). `--tools ""` leaves it no
    /// built-in tools, `--strict-mcp-config` with no `--mcp-config` no MCP
    /// servers, `--setting-sources ""` none of the user's or project's
    /// settings (hooks, permissions), `--disable-slash-commands` no skills;
    /// `--system-prompt` replaces its own system prompt with dry's. Not
    /// `--bare`: it reads only `ANTHROPIC_API_KEY`, never the login.
    pub fn claude() -> Cli {
        Cli {
            name: "claude".into(),
            program: "claude".into(),
            args: strings(&["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"]),
            quiet: strings(&[
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                "",
                "--disable-slash-commands",
                "--no-session-persistence",
            ]),
            model_flag: Some("--model".into()),
            system: SystemIn::Flag("--system-prompt".into()),
            prompt: PromptIn::Stdin,
            output: Output::Events(Dialect::Claude),
            context: 200_000,
            models: strings(&["haiku", "sonnet", "opus", "fable"]),
        }
    }

    /// Codex (`codex exec --help`, 0.17x). It has no switch for its tools:
    /// `--sandbox read-only` keeps what a command could do to reading;
    /// `--ignore-user-config` drops the user's provider, MCP servers and
    /// hooks (the login is still read), `--ignore-rules` their exec rules,
    /// `--ephemeral` saves nothing; `--strict-config` refuses a config key
    /// it does not know, so `developer_instructions` is checked.
    pub fn codex() -> Cli {
        Cli {
            name: "codex".into(),
            program: "codex".into(),
            args: strings(&["exec", "--json", "--color", "never"]),
            quiet: strings(&[
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--ignore-user-config",
                "--ignore-rules",
                "--strict-config",
            ]),
            model_flag: Some("--model".into()),
            system: SystemIn::Config("developer_instructions".into()),
            prompt: PromptIn::Stdin,
            output: Output::Events(Dialect::Codex),
            context: 200_000,
            models: Vec::new(),
        }
    }

    /// GitHub Copilot CLI (`copilot --help`, 1.0.x). Piped stdin is the
    /// prompt (non-interactive). `--available-tools=` (an empty list)
    /// leaves the model no tools, `--disable-builtin-mcps` no GitHub MCP,
    /// `--no-custom-instructions` no AGENTS.md, `--no-ask-user` no
    /// questions. No system flag: the system text goes above the prompt.
    pub fn copilot() -> Cli {
        Cli {
            name: "copilot".into(),
            program: "copilot".into(),
            args: strings(&["--output-format", "json", "--no-color"]),
            quiet: strings(&[
                "--available-tools=",
                "--disable-builtin-mcps",
                "--no-custom-instructions",
                "--no-ask-user",
                "--no-auto-update",
            ]),
            model_flag: Some("--model".into()),
            system: SystemIn::Inline,
            prompt: PromptIn::Stdin,
            output: Output::Events(Dialect::Copilot),
            context: 128_000,
            models: strings(&["auto"]),
        }
    }

    /// Does its output stream text as it comes?
    pub fn streams(&self) -> bool {
        matches!(self.output, Output::Events(Dialect::Claude | Dialect::Copilot))
    }

    /// The argv after the program, and the stdin, for `input`.
    pub fn invocation(&self, input: &Input) -> Invocation {
        let mut args = self.args.clone();
        args.extend(self.quiet.iter().cloned());
        if let Some(flag) = &self.model_flag
            && !input.model.is_empty()
        {
            args.extend([flag.clone(), input.model.clone()]);
        }
        let mut prompt = input.prompt.clone();
        match &self.system {
            // Always set: without it Claude Code answers as a coding agent.
            SystemIn::Flag(flag) => args.extend([flag.clone(), input.system.clone()]),
            SystemIn::Config(key) if !input.system.is_empty() => {
                args.extend(["-c".to_string(), format!("{key}={}", toml_string(&input.system))]);
            }
            SystemIn::Config(_) => {}
            SystemIn::Inline if !input.system.is_empty() => prompt = format!("<system>\n{}\n</system>\n\n{prompt}", input.system),
            SystemIn::Inline => {}
        }
        let stdin = match self.prompt {
            PromptIn::Stdin => {
                if self.output == Output::Events(Dialect::Codex) {
                    args.push("-".into());
                }
                Some(prompt)
            }
            PromptIn::Arg => {
                args.push(prompt);
                None
            }
        };
        Invocation { program: self.program.clone(), args, stdin }
    }

    /// A reader for this program's output.
    pub fn parser(&self) -> Parser {
        Parser { output: self.output, reply: Reply::default(), deltas: String::new(), raw: String::new(), error: None, finals: Vec::new() }
    }
}

/// A TOML basic string. JSON's string escapes (`\"`, `\\`, `\n`, `\t`,
/// `\uXXXX`) are all TOML's too.
fn toml_string(text: &str) -> String {
    Value::from(text).to_string()
}

/// What to run: the program, its args (an array, never through a shell)
/// and what to write to its stdin.
#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

/// A Chat Completions (or Responses) body flattened for a CLI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub model: String,
    pub system: String,
    pub prompt: String,
}

/// The line under a transcript.
pub const NEXT: &str = "Write only the next assistant message, nothing else.";

/// Read the request body `OpenAi::request` made (`messages` or the
/// Responses API's `input`) into the system text and one prompt.
pub fn flatten(body: &str) -> Result<Input, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("the request is not JSON: {e}"))?;
    let turns = v.get("messages").or_else(|| v.get("input")).and_then(Value::as_array).ok_or("the request has no messages")?;
    let mut system = Vec::new();
    let mut rest: Vec<(&str, String)> = Vec::new();
    for t in turns {
        let role = t.get("role").and_then(Value::as_str).unwrap_or("user");
        let text = content(t.get("content"));
        match role {
            "system" | "developer" => system.push(text),
            "assistant" => rest.push(("assistant", text)),
            _ => rest.push(("user", text)),
        }
    }
    let prompt = match rest.as_slice() {
        [] => return Err("the request has no user message".into()),
        [("user", only)] => only.clone(),
        many => {
            let mut out = String::from("The conversation so far, oldest first:\n\n");
            for (role, text) in many {
                out.push_str(&format!("<{role}>\n{text}\n</{role}>\n\n"));
            }
            out.push_str(NEXT);
            out
        }
    };
    let model = v.get("model").and_then(Value::as_str).unwrap_or_default().to_string();
    Ok(Input { model, system: system.join("\n\n"), prompt })
}

/// A message's text: a string, or the text parts of a list.
fn content(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""),
        _ => String::new(),
    }
}

/// What a run answered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    /// The model the program said it used.
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

/// Reads a run's stdout as it comes: `line` for each line (the deltas to
/// stream), then `finish`.
#[derive(Clone, Debug)]
pub struct Parser {
    output: Output,
    reply: Reply,
    /// The text the deltas gave so far.
    deltas: String,
    /// Everything, for `Text` and `Json`.
    raw: String,
    /// The program said it failed.
    error: Option<String>,
    /// Whole messages the program said (the last one is the answer).
    finals: Vec<String>,
}

fn num(v: &Value, pointer: &str) -> Option<u64> {
    v.pointer(pointer).and_then(Value::as_u64)
}

impl Parser {
    /// One line of stdout (without its newline); what it streams.
    pub fn line(&mut self, line: &str) -> Vec<Delta> {
        let dialect = match self.output {
            Output::Events(d) => d,
            _ => {
                self.raw.push_str(line);
                self.raw.push('\n');
                return Vec::new();
            }
        };
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Vec::new() };
        let out = match dialect {
            Dialect::Claude => self.claude(&v, line),
            Dialect::Codex => self.codex(&v),
            Dialect::Copilot => self.copilot(&v),
        };
        for d in &out {
            if let Delta::Text(t) = d {
                self.deltas.push_str(t);
            }
        }
        out
    }

    fn claude(&mut self, v: &Value, line: &str) -> Vec<Delta> {
        let t = v.get("type").and_then(Value::as_str).unwrap_or_default();
        if t == "stream_event" {
            let delta = v.pointer("/event/delta");
            return match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                Some("text_delta") => {
                    delta.and_then(|d| d.get("text")).and_then(Value::as_str).map(|s| vec![Delta::Text(s.into())]).unwrap_or_default()
                }
                Some("thinking_delta") => {
                    delta.and_then(|d| d.get("thinking")).and_then(Value::as_str).map(|s| vec![Delta::Think(s.into())]).unwrap_or_default()
                }
                _ => Vec::new(),
            };
        }
        if t == "system" && v.get("subtype").and_then(Value::as_str) == Some("init") {
            self.reply.model = v.get("model").and_then(Value::as_str).map(str::to_string);
        }
        // The message and result lines read as the session engines read
        // them (`crate::stream_json`); the usage is read here.
        for ev in crate::stream_json::parse_line(line) {
            match ev {
                crate::stream_json::Event::Text { text, parent: None } => self.finals.push(text),
                crate::stream_json::Event::Result { text, error, .. } => {
                    if let Some(text) = text {
                        if error {
                            self.error = Some(text);
                        } else {
                            self.finals.push(text);
                        }
                    }
                    self.reply.cost_usd = v.get("total_cost_usd").and_then(Value::as_f64);
                    let input = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                        .iter()
                        .filter_map(|k| num(v, &format!("/usage/{k}")))
                        .reduce(|a, b| a + b);
                    self.reply.input_tokens = input;
                    self.reply.output_tokens = num(v, "/usage/output_tokens");
                }
                _ => {}
            }
        }
        Vec::new()
    }

    fn codex(&mut self, v: &Value) -> Vec<Delta> {
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "item.completed" => {
                let item = &v["item"];
                let text = item.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                match item.get("type").and_then(Value::as_str) {
                    Some("agent_message") => {
                        self.finals.push(text.clone());
                        // Whole at the end: the one delta is the message.
                        let sep = if self.deltas.is_empty() { "" } else { "\n\n" };
                        vec![Delta::Text(format!("{sep}{text}"))]
                    }
                    Some("reasoning") if !text.is_empty() => vec![Delta::Think(text)],
                    _ => Vec::new(),
                }
            }
            "turn.completed" => {
                self.reply.input_tokens = num(v, "/usage/input_tokens");
                self.reply.output_tokens = num(v, "/usage/output_tokens");
                Vec::new()
            }
            "turn.failed" => {
                self.error = v.pointer("/error/message").and_then(Value::as_str).map(str::to_string).or(self.error.take());
                Vec::new()
            }
            "error" => {
                self.error = v.get("message").and_then(Value::as_str).map(str::to_string);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn copilot(&mut self, v: &Value) -> Vec<Delta> {
        let d = &v["data"];
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "assistant.message_delta" => {
                d.get("deltaContent").and_then(Value::as_str).map(|s| vec![Delta::Text(s.into())]).unwrap_or_default()
            }
            "assistant.message" => {
                self.reply.model = d.get("model").and_then(Value::as_str).map(str::to_string);
                if let Some(text) = d.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    self.finals.push(text.to_string());
                }
                self.reply.output_tokens = num(d, "/outputTokens").or(self.reply.output_tokens);
                Vec::new()
            }
            "session.error" | "error" => {
                self.error = d.get("message").or(v.get("message")).and_then(Value::as_str).map(str::to_string);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// The text the deltas gave so far.
    pub fn streamed(&self) -> &str {
        &self.deltas
    }

    /// The reply, once the program exited (`ok`: with status 0; `stderr`:
    /// what it said there, for the error line).
    pub fn finish(mut self, ok: bool, stderr: &str) -> Result<Reply, String> {
        let said = |stderr: &str| stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default().to_string();
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        let text = match self.output {
            Output::Text => Some(self.raw.trim().to_string()),
            Output::Json(pointer) => {
                let v: Value =
                    serde_json::from_str(self.raw.trim()).map_err(|e| format!("the output is not JSON ({e}): {}", said(stderr)))?;
                if v.get("is_error").and_then(Value::as_bool) == Some(true) {
                    return Err(v.pointer(pointer).and_then(Value::as_str).unwrap_or("the program said it failed").to_string());
                }
                self.reply.cost_usd = v.get("total_cost_usd").and_then(Value::as_f64);
                self.reply.output_tokens = num(&v, "/usage/output_tokens");
                v.pointer(pointer).and_then(Value::as_str).map(str::to_string)
            }
            // The last whole message; else what streamed.
            Output::Events(_) => self.finals.pop().or_else(|| (!self.deltas.is_empty()).then(|| self.deltas.clone())),
        };
        match text {
            Some(t) if ok && !t.trim().is_empty() => Ok(Reply { text: t, ..self.reply }),
            _ if !ok => Err(match said(stderr) {
                s if s.is_empty() => "the program exited with an error and said nothing".into(),
                s => s,
            }),
            _ => Err(format!(
                "the program answered no text{}",
                if stderr.trim().is_empty() { String::new() } else { format!(": {}", said(stderr)) }
            )),
        }
    }
}

/// The reply as a Chat Completions body, so `OpenAi::reply` reads it as a
/// server's: the text in `choices[0].message.content`, the usage the
/// program gave in `usage` (and its cost, which no server gives).
pub fn completion_body(reply: &Reply) -> String {
    let mut usage = serde_json::Map::new();
    if let Some(n) = reply.input_tokens {
        usage.insert("prompt_tokens".into(), n.into());
    }
    if let Some(n) = reply.output_tokens {
        usage.insert("completion_tokens".into(), n.into());
    }
    if let Some(c) = reply.cost_usd {
        usage.insert("cost_usd".into(), json!(c));
    }
    let mut body =
        json!({ "object": "chat.completion", "choices": [{ "index": 0, "message": { "role": "assistant", "content": reply.text } }] });
    if let Some(m) = &reply.model {
        body["model"] = Value::from(m.as_str());
    }
    if !usage.is_empty() {
        body["usage"] = Value::Object(usage);
    }
    body.to_string()
}

/// A `GET {cli:...}/models` answer: the aliases the CLI takes, as a
/// server's model list, or why it has none to give.
pub fn models_body(cli: &Cli) -> Result<String, String> {
    if cli.models.is_empty() {
        return Err(format!("{} does not list its models; set \"model\" in models.json", cli.name));
    }
    Ok(json!({ "object": "list", "data": cli.models.iter().map(|m| json!({ "id": m, "object": "model" })).collect::<Vec<_>>() })
        .to_string())
}

/// The first line of `--version`.
pub fn version_line(stdout: &str) -> String {
    stdout.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{Inference, Message, OpenAi, Role};

    fn run(cli: &Cli, out: &str, ok: bool) -> (Vec<Delta>, Result<Reply, String>) {
        let mut p = cli.parser();
        let deltas: Vec<Delta> = out.lines().flat_map(|l| p.line(l)).collect();
        (deltas, p.finish(ok, ""))
    }

    fn text(deltas: &[Delta]) -> String {
        deltas.iter().filter_map(|d| if let Delta::Text(t) = d { Some(t.as_str()) } else { None }).collect()
    }

    const COUNT: &str = "1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30";

    #[test]
    fn base_urls_name_the_provider_and_its_program() {
        assert_eq!(base_url("claude", None), "cli:claude");
        assert_eq!(base_url("claude", Some("/opt/c")), "cli:claude?program=/opt/c");
        assert_eq!(parse_base("cli:claude?program=/opt/c"), Some(("claude".into(), Some("/opt/c".into()))));
        assert_eq!(parse_base("cli:codex/models"), Some(("codex".into(), None)));
        assert_eq!(parse_base("http://x/v1"), None);
        assert_eq!(from_base("cli:claude?program=/opt/c").unwrap().program, "/opt/c");
        assert!(from_base("cli:nope").unwrap_err().contains("claude, codex, copilot"));
        let ai = OpenAi { base_url: "cli:claude".into(), model: "haiku".into(), ..OpenAi::default() };
        assert_eq!(ai.request(&[]).url, "cli:claude", "the URL is the base itself, no /chat/completions");
    }

    #[test]
    fn one_user_message_goes_as_it_is_and_the_system_apart() {
        let ai = OpenAi { base_url: "cli:claude".into(), model: "haiku".into(), ..OpenAi::default() };
        let body =
            ai.request(&[Message { role: Role::System, text: "Be brief.".into() }, Message { role: Role::User, text: "hi".into() }]).body;
        assert_eq!(flatten(&body).unwrap(), Input { model: "haiku".into(), system: "Be brief.".into(), prompt: "hi".into() });
    }

    #[test]
    fn a_conversation_is_a_transcript_oldest_first() {
        let ai = OpenAi { base_url: "cli:claude".into(), model: "m".into(), ..OpenAi::default() };
        let msgs = [
            Message { role: Role::System, text: "S1".into() },
            Message { role: Role::User, text: "first".into() },
            Message { role: Role::Assistant, text: "answer".into() },
            Message { role: Role::Tool, text: "tool said".into() },
        ];
        let input = flatten(&ai.request(&msgs).body).unwrap();
        assert_eq!(input.system, "S1");
        assert_eq!(
            input.prompt,
            format!(
                "The conversation so far, oldest first:\n\n<user>\nfirst\n</user>\n\n<assistant>\nanswer\n</assistant>\n\n<user>\ntool said\n</user>\n\n{NEXT}"
            )
        );
        let responses =
            r#"{"model":"m","input":[{"role":"developer","content":"D"},{"role":"user","content":[{"type":"input_text","text":"x"}]}]}"#;
        assert_eq!(flatten(responses).unwrap(), Input { model: "m".into(), system: "D".into(), prompt: "x".into() });
        assert!(flatten(r#"{"messages":[{"role":"system","content":"only"}]}"#).is_err());
        assert!(flatten("nope").is_err());
    }

    #[test]
    fn each_provider_gets_its_flags_and_no_tools() {
        let input = Input { model: "haiku".into(), system: "Say \"hi\"\nthen stop.".into(), prompt: "go".into() };
        let c = Cli::claude().invocation(&input);
        assert_eq!(c.program, "claude");
        let a = c.args.join(" ");
        assert!(a.starts_with("-p --output-format stream-json --verbose --include-partial-messages --tools  --strict-mcp-config"), "{a}");
        assert!(c.args.windows(2).any(|w| w == ["--model", "haiku"]));
        assert!(c.args.windows(2).any(|w| w[0] == "--system-prompt" && w[1] == input.system));
        assert_eq!(c.stdin.as_deref(), Some("go"));

        let x = Cli::codex().invocation(&input);
        assert_eq!(x.args[0], "exec");
        assert!(x.args.windows(2).any(|w| w == ["--sandbox", "read-only"]));
        assert!(x.args.windows(2).any(|w| w == ["-c", r#"developer_instructions="Say \"hi\"\nthen stop.""#]), "{:?}", x.args);
        assert_eq!(x.args.last().map(String::as_str), Some("-"), "codex reads the prompt from stdin");
        assert_eq!(x.stdin.as_deref(), Some("go"));

        let p = Cli::copilot().invocation(&Input { model: String::new(), ..input.clone() });
        assert!(!p.args.contains(&"--model".to_string()), "no model: the CLI's default");
        assert!(p.args.contains(&"--available-tools=".to_string()));
        assert_eq!(p.stdin.unwrap(), format!("<system>\n{}\n</system>\n\ngo", input.system));
    }

    #[test]
    fn claude_streams_its_text_deltas_and_gives_usage() {
        let (deltas, reply) = run(&Cli::claude(), include_str!("../tests/data/cli/claude-stream.jsonl"), true);
        assert!(deltas.len() > 4, "several deltas");
        assert_eq!(text(&deltas), COUNT);
        let reply = reply.unwrap();
        assert_eq!(reply.text, COUNT);
        assert_eq!(reply.model.as_deref(), Some("claude-haiku-5-5"));
        assert!(reply.output_tokens.unwrap() > 0 && reply.input_tokens.unwrap() > 0 && reply.cost_usd.unwrap() > 0.0);
    }

    #[test]
    fn claude_says_why_it_failed() {
        let (_, reply) = run(&Cli::claude(), include_str!("../tests/data/cli/claude-error.jsonl"), false);
        assert!(reply.unwrap_err().contains("issue with the selected model"));
    }

    #[test]
    fn codex_gives_the_whole_message_at_the_end() {
        let (deltas, reply) = run(&Cli::codex(), include_str!("../tests/data/cli/codex.jsonl"), true);
        assert_eq!(deltas, vec![Delta::Text(COUNT.into())]);
        let reply = reply.unwrap();
        assert_eq!((reply.text.as_str(), reply.output_tokens), (COUNT, Some(95)));
        let failed = "{\"type\":\"error\",\"message\":\"The 'x' model is not supported\"}\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"The 'x' model is not supported\"}}";
        assert_eq!(run(&Cli::codex(), failed, true).1.unwrap_err(), "The 'x' model is not supported");
    }

    #[test]
    fn copilot_streams_and_its_whole_message_wins() {
        let (deltas, reply) = run(&Cli::copilot(), include_str!("../tests/data/cli/copilot.jsonl"), true);
        assert_eq!(text(&deltas), COUNT);
        let reply = reply.unwrap();
        assert_eq!(reply.text, COUNT);
        assert_eq!(reply.model.as_deref(), Some("mai-code-2-flash"));
        let (_, err) = run(&Cli::copilot(), "", false);
        assert!(err.unwrap_err().contains("said nothing"));
    }

    #[test]
    fn json_and_text_outputs_read_whole() {
        let json = Cli { output: Output::Json("/result"), ..Cli::claude() };
        let (deltas, reply) = run(&json, include_str!("../tests/data/cli/claude.json"), true);
        assert!(deltas.is_empty());
        assert_eq!(reply.unwrap().text, "pong");
        let plain = Cli { output: Output::Text, ..Cli::claude() };
        assert_eq!(run(&plain, "  hello\nthere \n", true).1.unwrap().text, "hello\nthere");
        let mut p = plain.parser();
        p.line("");
        assert_eq!(p.finish(false, "boom\nError: no login\n").unwrap_err(), "Error: no login", "the last stderr line says why");
    }

    #[test]
    fn the_body_reads_as_a_chat_completion() {
        let reply =
            Reply { text: "pong".into(), model: Some("m".into()), input_tokens: Some(3), output_tokens: Some(1), cost_usd: Some(0.001) };
        let body = completion_body(&reply);
        let ai = OpenAi { base_url: "cli:claude".into(), ..OpenAi::default() };
        assert_eq!(ai.reply(&body), Ok("pong".into()));
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!((v["usage"]["prompt_tokens"].clone(), v["usage"]["cost_usd"].clone()), (json!(3), json!(0.001)));
        assert!(models_body(&Cli::claude()).unwrap().contains("\"haiku\""));
        assert!(models_body(&Cli::codex()).is_err());
        assert_eq!(version_line("\n2.1.330 (Claude Code)\n"), "2.1.330 (Claude Code)");
    }
}
