//! Context: the only limit on a turn is the model's context window.
//!
//! ```text
//! render ─▶ measure ─┬─ under 75%            ─▶ send
//!                    ├─ history can shrink    ─▶ compress ─▶ render again ─▶ send
//!                    └─ the fixed part is big ─▶ say so    ─▶ send as it is
//! ```
//!
//! Before every LLM call the shell renders the prompt and asks `plan`.
//! Under `COMPRESS_AT` of the window it is sent. Over it, the older
//! history is summarised by the same model (`Compression::request`), and
//! `apply` puts the summary into the log as a `Role::Summary` message at
//! the cut. Nothing is deleted: `convo/messages.md` keeps every message,
//! with `### summary` blocks marking each cut. The prompt's history slot
//! is then the newest summary, then what came after it; the current user
//! message is always sent verbatim, even when it is older than the cut.
//!
//! Only the history is ever compressed: the spec slots, the tools and the
//! artifact snapshots are what the engine is and what is true now. When
//! those alone are over the threshold, `plan` says so (shown in the
//! Response view) and the prompt goes as it is; nothing loops.
//!
//! The window comes from the model setting (`context` in `models.json`,
//! or `engine.md`), else from the server (`probes`, read by
//! `read_probe`), else `ASSUMED`, which the UI marks as assumed.
//!
//! Sizes are an estimate, deterministic and cheap: a quarter of the
//! characters, plus a few tokens per message for its framing.

use serde_json::{Value, json};

use crate::inference::{Message, OpenAi, Request, Role};
use crate::prompt::Rendered;
use crate::state::Engine;

/// Compress when the prompt reaches this share of the window.
pub const COMPRESS_AT: f64 = 0.75;
/// The newest messages kept verbatim at a cut: up to this share of the
/// window (always at least the newest one).
pub const KEEP_SHARE: f64 = 0.20;
/// The longest a summary may be, as a share of the window.
pub const SUMMARY_SHARE: f64 = 0.08;
/// The window assumed when neither the setting nor the server says.
pub const ASSUMED: u32 = 8_192;
/// Tokens of framing per message (its role, separators).
pub const MESSAGE_OVERHEAD: usize = 4;
/// The share of the window the text to summarise may take in the
/// compression request (the rest is its instructions and the answer).
const SUMMARISE_SHARE: f64 = 0.6;

/// The estimated tokens of `text`: a quarter of its characters, rounded up.
pub fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// The estimated tokens of one message as sent.
pub fn message_tokens(m: &Message) -> usize {
    tokens(&m.text) + MESSAGE_OVERHEAD
}

/// `950`, `12.3k`, `32k`.
pub fn short(n: usize) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let k = n as f64 / 1_000.0;
    if k >= 100.0 || (k - k.round()).abs() < 0.05 { format!("{:.0}k", k.round()) } else { format!("{k:.1}k") }
}

/// Where a window size came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The model setting's `context`.
    Setting,
    /// The server said (which field).
    Server(String),
    /// Nobody said: `ASSUMED`.
    Assumed,
}

/// A model's context window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub tokens: u32,
    pub source: Source,
}

impl Window {
    /// The window for `ai`: its setting, else what the server said
    /// (`found`, from `read_probe`), else assumed.
    pub fn of(ai: &OpenAi, found: Option<(u32, String)>) -> Window {
        match (ai.context, found) {
            (Some(tokens), _) if tokens > 0 => Window { tokens, source: Source::Setting },
            (_, Some((tokens, field))) if tokens > 0 => Window { tokens, source: Source::Server(field) },
            _ => Window { tokens: ASSUMED, source: Source::Assumed },
        }
    }

    /// `32k (models.json)`, `4.1k (server: loaded_context_length)`, `8.2k (assumed)`.
    pub fn label(&self) -> String {
        let from = match &self.source {
            Source::Setting => "set in the model setting".to_string(),
            Source::Server(field) => format!("server: {field}"),
            Source::Assumed => "assumed: neither the setting nor the server says".to_string(),
        };
        format!("{} ({from})", short(self.tokens as usize))
    }
}

/// A rendered prompt's size against a window.
#[derive(Clone, Debug, PartialEq)]
pub struct Budget {
    /// The whole prompt as sent.
    pub tokens: usize,
    /// What is never compressed: the system prompt's spec slots, tools and
    /// snapshots, the response contract and the current user message.
    pub fixed: usize,
    pub window: Window,
}

impl Budget {
    pub fn ratio(&self) -> f64 {
        self.tokens as f64 / self.window.tokens.max(1) as f64
    }

    /// At or past `COMPRESS_AT`.
    pub fn over(&self) -> bool {
        self.ratio() >= COMPRESS_AT
    }

    /// `prompt ≈ 12.3k of 32k tokens (38%)`, with `window assumed` when it is.
    pub fn line(&self) -> String {
        let assumed = if self.window.source == Source::Assumed { " · window assumed" } else { "" };
        format!(
            "prompt ≈ {} of {} tokens ({:.0}%){assumed}",
            short(self.tokens),
            short(self.window.tokens as usize),
            self.ratio() * 100.0
        )
    }
}

/// Measure `r` against `w`.
pub fn measure(r: &Rendered, w: &Window) -> Budget {
    Budget { tokens: r.messages().iter().map(message_tokens).sum(), fixed: r.fixed_tokens(), window: w.clone() }
}

/// The newest summary's index in `e`'s log.
pub fn last_summary(e: &Engine) -> Option<usize> {
    e.messages.iter().rposition(|m| m.role == Role::Summary)
}

/// The current user message's index: the newest one from you.
pub fn last_user(e: &Engine) -> Option<usize> {
    e.messages.iter().rposition(|m| m.role == Role::User)
}

/// The newest summary's text, sent in the history slot before the rest.
pub fn summary(e: &Engine) -> Option<String> {
    last_summary(e).map(|s| e.messages[s].text.clone())
}

/// The messages the model reads: after the newest summary (all of them
/// with none), with the current user message first when it is older than
/// the cut, so it is always read verbatim.
pub fn history(e: &Engine) -> Vec<Message> {
    let Some(s) = last_summary(e) else { return e.messages.clone() };
    let mut out = Vec::new();
    if let Some(u) = last_user(e).filter(|u| *u < s) {
        out.push(e.messages[u].clone());
    }
    out.extend(e.messages[s + 1..].iter().filter(|m| m.role != Role::Summary).cloned());
    out
}

/// What to do before a call.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Under the threshold: send it.
    Send(Budget),
    /// Summarise the older history first.
    Compress(Compression),
    /// Over, and compressing cannot help: send as it is, and say why.
    Over(Budget, String),
}

/// One compression: what is summarised, and the call that does it.
#[derive(Clone, Debug, PartialEq)]
pub struct Compression {
    pub budget: Budget,
    /// The messages summarised: `from..to` in the log, without the current
    /// user message (`kept`, when it is in that range).
    pub from: usize,
    pub to: usize,
    pub kept: Option<usize>,
    /// The log's length when planned: `apply` refuses a log that changed.
    pub len: usize,
    /// How many messages and tokens are summarised.
    pub messages: usize,
    pub tokens: usize,
    /// The longest summary kept, in characters.
    pub max_chars: usize,
    /// The messages to send to the model (same inference as the engine).
    pub request: Vec<Message>,
}

/// The compression prompt.
const PROMPT: &str = "You compress a conversation between a user and an assistant engine so it can go on in less space. \
Write one summary of the conversation you are given, to replace it. Keep: what the user asked for and still wants, \
decisions made, facts learned, what the tools did and the results that still matter (names, paths, numbers), \
and what is still open or next. Leave out greetings, repetition and anything already done with. \
Do not add anything that is not in it. Plain text, no preamble.";

/// Decide, for engine `e` whose prompt rendered as `r`, against window `w`.
pub fn plan(e: &Engine, r: &Rendered, w: &Window) -> Plan {
    let budget = measure(r, w);
    if !budget.over() {
        return Plan::Send(budget);
    }
    let limit = (w.tokens as f64 * COMPRESS_AT) as usize;
    if budget.fixed >= limit {
        let why = format!(
            "the spec, tools, artifact snapshots and your message alone are ≈ {} of {} tokens; only the history is ever compressed, so the prompt is sent as it is",
            short(budget.fixed),
            short(w.tokens as usize)
        );
        return Plan::Over(budget, why);
    }
    let start = last_summary(e).map_or(0, |s| s + 1);
    let user = last_user(e);
    let n = e.messages.len();
    // Keep the newest messages verbatim, up to KEEP_SHARE (at least one).
    let keep = (w.tokens as f64 * KEEP_SHARE) as usize;
    let (mut k, mut kept) = (n, 0);
    while k > start {
        let t = message_tokens(&e.messages[k - 1]);
        if k < n && kept + t > keep {
            break;
        }
        kept += t;
        k -= 1;
    }
    let covered: Vec<usize> = (start..k).filter(|i| Some(*i) != user).collect();
    if covered.is_empty() {
        let why = format!("nothing older in the history to compress: the newest messages alone are ≈ {} tokens; sent as it is", short(kept));
        return Plan::Over(budget, why);
    }
    let max_chars = ((w.tokens as f64 * SUMMARY_SHARE) as usize * 4).max(400);
    let room = ((w.tokens as f64 * SUMMARISE_SHARE) as usize * 4).max(2_000);
    let each = (room / covered.len()).max(80);
    let mut transcript = String::new();
    for &i in &covered {
        let m = &e.messages[i];
        transcript.push_str(&format!("### {}\n{}\n\n", role_word(m.role), cut_middle(&m.text, each)));
    }
    let previous = start.checked_sub(1).map(|s| format!("The summary so far (of everything before):\n{}\n\n", e.messages[s].text));
    let words = max_chars / 6;
    let request = vec![
        Message { role: Role::System, text: format!("{PROMPT} At most {words} words.") },
        Message {
            role: Role::User,
            text: format!("{}The conversation to summarise:\n\n{}", previous.unwrap_or_default(), transcript.trim_end()),
        },
    ];
    Plan::Compress(Compression {
        budget,
        from: start,
        to: k,
        kept: user.filter(|u| (start..k).contains(u)),
        len: n,
        messages: covered.len(),
        tokens: covered.iter().map(|&i| message_tokens(&e.messages[i])).sum(),
        max_chars,
        request,
    })
}

/// Put `summary` (the model's answer to `c.request`) into `e`'s log at
/// the cut. Every message stays; the next render reads the summary and
/// what came after it. Returns a line for the UI.
pub fn apply(e: &mut Engine, c: &Compression, summary: &str) -> Result<String, String> {
    if e.messages.len() != c.len {
        return Err("the history changed while compressing; nothing was replaced".into());
    }
    let text = summary.trim();
    if text.is_empty() {
        return Err("the model gave an empty summary; nothing was replaced".into());
    }
    let text = if text.chars().count() > c.max_chars {
        format!("{}…", text.chars().take(c.max_chars).collect::<String>().trim_end())
    } else {
        text.to_string()
    };
    let size = tokens(&text);
    e.messages.insert(c.to, Message { role: Role::Summary, text });
    Ok(format!("compressed {} messages (≈ {} tokens) into a summary of ≈ {} tokens", c.messages, short(c.tokens), short(size)))
}

/// Every compression in `e`'s log, oldest first: how many messages each
/// summary covers (since the one before) and its size.
pub fn compressions(e: &Engine) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut since = 0;
    for m in &e.messages {
        if m.role == Role::Summary {
            out.push((since, tokens(&m.text)));
            since = 0;
        } else {
            since += 1;
        }
    }
    out
}

fn role_word(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tools",
        Role::System => "system",
        Role::Summary => "summary",
    }
}

/// `text`, at most `max` characters: its start and end, the middle cut.
fn cut_middle(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let half = max / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(n - half).collect();
    format!("{head}\n… ({} characters cut) …\n{tail}", n - 2 * half)
}

/// Which kind of server a probe asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Server {
    /// `GET {root}/api/v0/models`: `loaded_context_length`, else `max_context_length`.
    LmStudio,
    /// `POST {root}/api/show {"model": ...}`: `num_ctx` in `parameters`,
    /// else `model_info.<arch>.context_length`.
    Ollama,
}

/// One request that may say the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub server: Server,
    /// A GET when `body` is `None`.
    pub url: String,
    pub body: Option<String>,
    /// The key's variable name; the key is never in here.
    pub key_env: Option<String>,
}

impl Probe {
    /// As a POST request, for a sender that only posts.
    pub fn post(&self) -> Option<Request> {
        self.body.as_ref().map(|body| Request { url: self.url.clone(), body: body.clone(), key_env: self.key_env.clone() })
    }
}

/// The requests to try, in order, to learn `ai`'s window from its server
/// (none when the setting says it, or no model is chosen).
pub fn probes(ai: &OpenAi) -> Vec<Probe> {
    // A CLI model has no server to ask (`crate::cli`; its setting says).
    if ai.context.is_some() || ai.model.is_empty() || ai.base_url.is_empty() || crate::cli::is_cli(&ai.base_url) {
        return Vec::new();
    }
    let base = ai.base_url.trim_end_matches('/');
    let root = base.strip_suffix("/v1").unwrap_or(base);
    vec![
        Probe { server: Server::LmStudio, url: format!("{root}/api/v0/models"), body: None, key_env: ai.key_env.clone() },
        Probe {
            server: Server::Ollama,
            url: format!("{root}/api/show"),
            body: Some(json!({ "model": ai.model }).to_string()),
            key_env: ai.key_env.clone(),
        },
    ]
}

/// The window `model` has, from a probe's answer, and the field it was in.
pub fn read_probe(server: Server, body: &str, model: &str) -> Option<(u32, String)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let num = |v: &Value| v.as_u64().filter(|n| *n > 0).and_then(|n| u32::try_from(n).ok());
    match server {
        Server::LmStudio => {
            let m = v.get("data")?.as_array()?.iter().find(|m| m.get("id").and_then(Value::as_str) == Some(model))?;
            // What it is loaded with is the real limit; the most it could be loaded with is next.
            for field in ["loaded_context_length", "max_context_length"] {
                if let Some(n) = m.get(field).and_then(num) {
                    return Some((n, format!("LM Studio {field}")));
                }
            }
            None
        }
        Server::Ollama => {
            let set = v.get("parameters").and_then(Value::as_str).and_then(|p| {
                p.lines().find_map(|l| l.trim().strip_prefix("num_ctx").and_then(|n| n.trim().parse::<u32>().ok()))
            });
            if let Some(n) = set {
                return Some((n, "Ollama num_ctx".into()));
            }
            let info = v.get("model_info")?.as_object()?;
            info.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, n)| num(n)).map(|n| (n, "Ollama context_length".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{Format, Responses};

    fn engine(window: u32) -> Engine {
        let mut e = Engine::new("E", "Be brief.", OpenAi { model: "m".into(), context: Some(window), ..OpenAi::default() });
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.format = Format::default();
        e
    }

    fn talk(e: &mut Engine, turns: usize, size: usize) {
        for i in 0..turns {
            e.messages.push(Message { role: Role::User, text: format!("question {i}: {}", "q".repeat(size)) });
            e.messages.push(Message { role: Role::Assistant, text: format!("thought: t\nact: reply(text=\"answer {i}: {}\")", "a".repeat(size)) });
        }
    }

    fn window(e: &Engine) -> Window {
        Window::of(&e.inference, None)
    }

    #[test]
    fn sizes_are_a_cheap_deterministic_estimate() {
        assert_eq!(tokens(""), 0);
        assert_eq!(tokens("abcd"), 1);
        assert_eq!(tokens("abcde"), 2);
        assert_eq!(message_tokens(&Message { role: Role::User, text: "abcd".into() }), 1 + MESSAGE_OVERHEAD);
        assert_eq!((short(950), short(12_345), short(32_768), short(32_000), short(131_072)), ("950".into(), "12.3k".into(), "32.8k".into(), "32k".into(), "131k".into()));
        let mut e = engine(32_000);
        talk(&mut e, 1, 10);
        let b = measure(&crate::prompt::render(&e), &window(&e));
        assert_eq!(b, measure(&crate::prompt::render(&e), &window(&e)), "the same state measures the same");
        assert!(b.line().starts_with("prompt ≈ ") && b.line().contains(" of 32k tokens ("), "{}", b.line());
        assert!(b.fixed > 0 && b.fixed < b.tokens);
    }

    #[test]
    fn the_window_comes_from_the_setting_else_the_server_else_assumed() {
        let mut ai = OpenAi { model: "qwen3-8b".into(), base_url: "http://127.0.0.1:1234/v1".into(), ..OpenAi::default() };
        let p = probes(&ai);
        assert_eq!(p[0].url, "http://127.0.0.1:1234/api/v0/models");
        assert_eq!((p[1].url.as_str(), p[1].body.as_deref()), ("http://127.0.0.1:1234/api/show", Some("{\"model\":\"qwen3-8b\"}")));
        // LM Studio, as the local server answers: the loaded length wins.
        let lm = r#"{"data":[{"id":"other","max_context_length":9},{"id":"qwen3-8b","state":"loaded","max_context_length":2097152,"loaded_context_length":4096}]}"#;
        assert_eq!(read_probe(Server::LmStudio, lm, "qwen3-8b"), Some((4096, "LM Studio loaded_context_length".into())));
        let unloaded = r#"{"data":[{"id":"qwen3-8b","state":"not-loaded","max_context_length":262144}]}"#;
        assert_eq!(read_probe(Server::LmStudio, unloaded, "qwen3-8b").unwrap().0, 262_144);
        assert_eq!(read_probe(Server::LmStudio, lm, "missing"), None);
        // Ollama: num_ctx set, else the model's own length.
        let ol = r#"{"parameters":"stop \"<|im_end|>\"\nnum_ctx                        16384","model_info":{"qwen3.context_length":40960}}"#;
        assert_eq!(read_probe(Server::Ollama, ol, "x"), Some((16_384, "Ollama num_ctx".into())));
        assert_eq!(read_probe(Server::Ollama, r#"{"model_info":{"qwen3.context_length":40960}}"#, "x").unwrap().0, 40_960);
        assert_eq!(read_probe(Server::Ollama, "<html>404</html>", "x"), None);

        let from_server = Window::of(&ai, Some((4096, "LM Studio loaded_context_length".into())));
        assert_eq!(from_server.label(), "4.1k (server: LM Studio loaded_context_length)");
        let assumed = Window::of(&ai, None);
        assert_eq!((assumed.tokens, &assumed.source), (ASSUMED, &Source::Assumed));
        ai.context = Some(32_768);
        assert!(probes(&ai).is_empty(), "set: nothing to ask");
        assert_eq!(Window::of(&ai, Some((4096, "x".into()))).source, Source::Setting, "the setting wins");
    }

    #[test]
    fn below_the_threshold_nothing_happens() {
        let mut e = engine(32_000);
        talk(&mut e, 3, 100);
        let r = crate::prompt::render(&e);
        assert!(matches!(plan(&e, &r, &window(&e)), Plan::Send(b) if !b.over()));
        assert!(last_summary(&e).is_none());
    }

    /// Above 75%: the summary call is made (by a fake model), the history
    /// is cut there with every message kept, the next render is under the
    /// threshold, and the turn goes on as before: no step limit anywhere.
    #[test]
    fn above_the_threshold_the_history_is_summarised_and_every_message_kept() {
        let mut e = engine(4_000);
        talk(&mut e, 16, 400);
        e.messages.push(Message { role: Role::User, text: "now save a plan".into() });
        let before = e.messages.clone();
        let r = crate::prompt::render(&e);
        let p = plan(&e, &r, &window(&e));
        let Plan::Compress(c) = p else { panic!("over 75%: compress, got {p:?}") };
        assert!(c.budget.over());
        // The request is the same model with the compression prompt and the old turns.
        assert_eq!(c.request[0].role, Role::System);
        assert!(c.request[0].text.starts_with("You compress a conversation"));
        assert!(c.request[1].text.contains("### user\nquestion 0:") && !c.request[1].text.contains("now save a plan"));

        let note = apply(&mut e, &c, "The user asked sixteen questions about q and got answers about a.").unwrap();
        assert!(note.starts_with(&format!("compressed {} messages", c.messages)), "{note}");
        // The full log is kept: every message, plus one summary at the cut.
        let without: Vec<Message> = e.messages.iter().filter(|m| m.role != Role::Summary).cloned().collect();
        assert_eq!(without, before);
        assert_eq!(last_summary(&e), Some(c.to));
        let md = crate::files::messages_md(&e.messages);
        assert!(md.contains("### summary\n\nThe user asked sixteen"));
        assert_eq!(crate::files::parse_messages(&md), e.messages, "messages.md reads back with its summary");

        // The next render is under the threshold, the summary in the history slot.
        let r = crate::prompt::render(&e);
        let b = measure(&r, &window(&e));
        assert!(!b.over(), "{}", b.line());
        assert!(matches!(plan(&e, &r, &window(&e)), Plan::Send(_)));
        let sent = r.messages();
        assert!(sent[0].text.ends_with("## Earlier conversation (summary)\n\nThe user asked sixteen questions about q and got answers about a."));
        assert!(!sent.iter().any(|m| m.text.starts_with("question 0:")), "the old turns are not sent");
        assert!(sent.last().unwrap().text.starts_with("now save a plan\n\n## Response"), "the current message is sent verbatim, with the contract");
        assert_eq!(compressions(&e), [(c.to, tokens("The user asked sixteen questions about q and got answers about a."))]);

        // The turn carries on normally after it.
        let s = crate::turn::step(&mut e, "thought: save\nact: write(name=\"plan\", text=\"1. tea\")", &mut |_| None);
        assert_eq!(s, crate::turn::Step::Again);
        let s = crate::turn::step(&mut e, "thought: done\nact: reply(text=\"Saved.\")", &mut |_| None);
        assert_eq!(s, crate::turn::Step::Reply("Saved.".into()));
        let r = crate::prompt::render(&e);
        assert!(r.messages().iter().any(|m| m.text.contains("saved plan")), "results after the cut are read");
    }

    #[test]
    fn the_current_message_older_than_the_cut_is_still_sent_verbatim() {
        let mut e = engine(4_000);
        e.messages.push(Message { role: Role::User, text: "build it".into() });
        for i in 0..24 {
            e.messages.push(Message { role: Role::Assistant, text: format!("thought: {}\nact: list()", "s".repeat(300 + i)) });
            e.messages.push(Message { role: Role::Tool, text: format!("list() → {}", "r".repeat(300)) });
        }
        let Plan::Compress(c) = plan(&e, &crate::prompt::render(&e), &window(&e)) else { panic!("compress") };
        assert_eq!(c.kept, Some(0), "the user message is in the cut range but kept");
        assert!(!c.request[1].text.contains("build it"));
        apply(&mut e, &c, "Listed many times.").unwrap();
        let h = history(&e);
        assert_eq!(h[0].text, "build it");
        assert!(!measure(&crate::prompt::render(&e), &window(&e)).over());
        // A second compression summarises the first summary with what came after.
        for _ in 0..24 {
            e.messages.push(Message { role: Role::Assistant, text: format!("thought: {}\nact: list()", "s".repeat(300)) });
            e.messages.push(Message { role: Role::Tool, text: "list() → x".repeat(30) });
        }
        let Plan::Compress(c2) = plan(&e, &crate::prompt::render(&e), &window(&e)) else { panic!("compress again") };
        assert!(c2.request[1].text.starts_with("The summary so far (of everything before):\nListed many times."));
        apply(&mut e, &c2, "Listed even more.").unwrap();
        assert_eq!(compressions(&e).len(), 2);
        assert_eq!(summary(&e).as_deref(), Some("Listed even more."));
        assert_eq!(history(&e)[0].text, "build it");
    }

    #[test]
    fn snapshots_alone_over_budget_are_said_not_looped() {
        let mut e = engine(2_000);
        e.artifacts.insert("big".into(), "x".repeat(crate::prompt::ARTIFACT_CHARS));
        e.artifacts.insert("big2".into(), "y".repeat(crate::prompt::ARTIFACT_CHARS));
        talk(&mut e, 3, 50);
        let Plan::Over(b, why) = plan(&e, &crate::prompt::render(&e), &window(&e)) else { panic!("over, nothing to do") };
        assert!(b.over() && b.fixed >= 1_500);
        assert!(why.contains("only the history is ever compressed"), "{why}");
        assert!(last_summary(&e).is_none());
    }

    #[test]
    fn a_changed_log_or_an_empty_summary_replaces_nothing() {
        let mut e = engine(4_000);
        talk(&mut e, 16, 400);
        let Plan::Compress(c) = plan(&e, &crate::prompt::render(&e), &window(&e)) else { panic!() };
        assert!(apply(&mut e, &c, "  ").is_err());
        e.messages.push(Message { role: Role::User, text: "more".into() });
        assert!(apply(&mut e, &c, "sum").unwrap_err().contains("changed"));
        assert!(last_summary(&e).is_none());
    }
}
