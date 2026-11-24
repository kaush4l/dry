//! Streaming replies: the model's answer read while it is written, so a
//! reply's text grows and its component appears before the answer ends.
//!
//! ```text
//! server ─▶ Sse (bytes, split anywhere) ─▶ Delta::Text / Delta::Think
//!        ─▶ Parser (resumable, the same grammar as `response` + `calls`) ─▶ Event…
//!        ─▶ View (provisional: reply text so far, the ask/show once known)
//! answer ends ─▶ the full parsers read the whole text, as always: final truth
//! ```
//!
//! Three pure pieces, every one fed any split of its input:
//!
//! - **`Sse`** reads an OpenAI-compatible `stream: true` body (`data:
//!   {...choices[0].delta.content...}` lines, `[DONE]`; the Responses API's
//!   `response.output_text.delta` too) and gives back the body a
//!   non-streamed call would have (`Sse::finish`), so the runner reads it
//!   as it always did. A server that does not stream answers with a plain
//!   body: that body comes back as it is (the fallback).
//! - **`Parser`** is the call grammar as a state machine over the answer
//!   format (`toon` lines, a `json` object, or free text with no
//!   contract): outside a call, a call's name, an argument's key, a quoted
//!   value with its escapes, list separators. It says what it read as
//!   `Event`s. For a whole answer its calls are exactly
//!   `calls::parse(act)` (the property test in this file feeds every
//!   registry example, forms, displays, escapes, think blocks and acts of
//!   many calls split at every byte and in random chunks).
//! - **`View`** is the reply as far as it is written: its text, the ask
//!   and the show once `ask=`/`show=` closed, options and rows one by one.
//!   It is provisional: when the reply call closes it is read by
//!   `ask::Parsed::of` (aliases, near misses, notes), and when the answer
//!   ends the shell draws the saved message, read by the full parsers.
//!   Meaning never changes from a non-streamed answer.
//!
//! `<think>…</think>` outside the calls (and a server's separate reasoning
//! deltas) go to `View::think`, a fold of its own, never the reply.

use crate::ask::{self, Ask, Parsed};
use crate::calls::Call;
use crate::inference::Request;
use crate::response::Format;
use crate::show::{self, Show};
use crate::state::Engine;
use crate::turn::ACT;

// ───────────────────────────── SSE ─────────────────────────────

/// What one SSE event added to the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delta {
    /// The answer's text (`delta.content`).
    Text(String),
    /// Reasoning the server sends apart from the text (`delta.reasoning_content`).
    Think(String),
}

/// `req` asking the server to stream (`"stream": true` in its body).
pub fn streaming(req: &Request) -> Request {
    let mut out = req.clone();
    if let Ok(serde_json::Value::Object(mut body)) = serde_json::from_str::<serde_json::Value>(&req.body) {
        body.insert("stream".into(), serde_json::Value::Bool(true));
        out.body = serde_json::Value::Object(body).to_string();
    }
    out
}

/// `req` without `"stream"`: the plain call a transport falls back to.
pub fn plain(req: &Request) -> Request {
    let mut out = req.clone();
    if let Ok(serde_json::Value::Object(mut body)) = serde_json::from_str::<serde_json::Value>(&req.body) {
        body.remove("stream");
        out.body = serde_json::Value::Object(body).to_string();
    }
    out
}

/// Should a transport ask again without streaming? When nothing streamed
/// and the server answered with an error object (one that refuses
/// `stream`, say). A plain body that reads is kept as it is.
pub fn retry_plain(streamed: bool, body: &str) -> bool {
    !streamed && serde_json::from_str::<serde_json::Value>(body).ok().is_some_and(|v| v.get("error").is_some_and(|e| !e.is_null()))
}

/// Reads a streamed body, any split of its bytes.
#[derive(Clone, Debug, Default)]
pub struct Sse {
    responses: bool,
    line: Vec<u8>,
    raw: Vec<u8>,
    events: usize,
    text: String,
    /// The reasoning streamed apart from the text, kept for an answer
    /// whose calls came as `tool_calls` (`crate::inference`).
    think: String,
    /// `delta.tool_calls`, by their index: name and arguments so far.
    calls: Vec<(String, String)>,
    error: Option<String>,
    completed: Option<String>,
    done: bool,
}

impl Sse {
    /// For the answer to `req` (the Responses API streams other events).
    pub fn new(req: &Request) -> Sse {
        Sse { responses: req.url.trim_end_matches('/').ends_with("/responses"), ..Sse::default() }
    }

    /// The next bytes; what they added.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Delta> {
        self.raw.extend_from_slice(bytes);
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                let line = std::mem::take(&mut self.line);
                self.line_done(&line, &mut out);
            } else {
                self.line.push(b);
            }
        }
        out
    }

    /// Was anything streamed (else the body is a plain one)?
    pub fn streamed(&self) -> bool {
        self.events > 0
    }

    /// The answer's text so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    fn line_done(&mut self, line: &[u8], out: &mut Vec<Delta>) {
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end_matches('\r');
        let Some(data) = line.strip_prefix("data:") else { return };
        let data = data.strip_prefix(' ').unwrap_or(data).trim_end();
        self.events += 1;
        if data == "[DONE]" {
            self.done = true;
            return;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { return };
        if v.get("error").is_some_and(|e| !e.is_null()) {
            self.error = Some(data.to_string());
            return;
        }
        let s = |p: &str| v.pointer(p).and_then(serde_json::Value::as_str).filter(|t| !t.is_empty()).map(str::to_string);
        if let Some(t) = s("/choices/0/delta/content") {
            self.text.push_str(&t);
            out.push(Delta::Text(t));
        }
        if let Some(t) = s("/choices/0/delta/reasoning_content").or_else(|| s("/choices/0/delta/reasoning")) {
            self.think.push_str(&t);
            out.push(Delta::Think(t));
        }
        // Calls the model wrote its own way come in pieces: a name once,
        // then its arguments' JSON a few characters at a time.
        for c in v.pointer("/choices/0/delta/tool_calls").and_then(serde_json::Value::as_array).into_iter().flatten() {
            let i = c.get("index").and_then(serde_json::Value::as_u64).map_or(self.calls.len().saturating_sub(1), |i| i as usize);
            if i >= self.calls.len() && i < 64 {
                self.calls.resize(i + 1, Default::default());
            }
            let Some((name, args)) = self.calls.get_mut(i) else { continue };
            let f = |key: &str| c.pointer(&format!("/function/{key}")).and_then(serde_json::Value::as_str);
            name.push_str(f("name").unwrap_or(""));
            args.push_str(f("arguments").unwrap_or(""));
        }
        match v.get("type").and_then(serde_json::Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(t) = s("/delta") {
                    self.text.push_str(&t);
                    out.push(Delta::Text(t));
                }
            }
            Some("response.reasoning_text.delta" | "response.reasoning_summary_text.delta") => out.extend(s("/delta").map(Delta::Think)),
            Some("response.completed") => self.completed = v.get("response").map(|r| r.to_string()),
            Some("error") => self.error = Some(data.to_string()),
            _ => {}
        }
    }

    /// The body a non-streamed call would have answered: the text as one
    /// message, an error object as it came, or (nothing streamed) the
    /// body as it came.
    pub fn finish(mut self) -> String {
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.line_done(&line, &mut Vec::new());
        }
        if self.events == 0 {
            return String::from_utf8_lossy(&self.raw).into_owned();
        }
        if let Some(e) = self.error {
            return e;
        }
        if self.responses {
            return self.completed.unwrap_or_else(|| serde_json::json!({ "output_text": self.text }).to_string());
        }
        let mut message = serde_json::json!({ "role": "assistant", "content": self.text });
        // As a plain body carries them, so `Inference::reply` reads both the same way.
        if self.calls.iter().any(|(name, _)| !name.is_empty()) {
            message["reasoning_content"] = self.think.into();
            message["tool_calls"] = self.calls.iter().map(|(name, args)| serde_json::json!({ "type": "function", "function": { "name": name, "arguments": args } })).collect();
        }
        serde_json::json!({ "choices": [{ "message": message }] }).to_string()
    }
}

// ─────────────────────────── Events ────────────────────────────

/// What the parser read. `index` is the call's place in the act.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Prose outside the calls: a field that is not `act` (`thought`), or
    /// the whole answer when there is no contract.
    Text(String),
    /// Inside `<think>…</think>`, or the server's reasoning.
    Think(String),
    /// A contract field began (`thought`, `act`).
    Field(String),
    CallStart {
        index: usize,
        name: String,
    },
    ArgStart {
        index: usize,
        key: String,
    },
    /// More of a value, as written (escapes read).
    ArgDelta {
        index: usize,
        key: String,
        chunk: String,
    },
    /// A value is closed: the value `calls::parse` gives it.
    ArgDone {
        index: usize,
        key: String,
        value: String,
    },
    /// An item of a list value (`options`, `rows`, `fields`, `data`,
    /// `path`) closed, at each `|` (and `;` in `data`), and the last at
    /// the value's end. Trimmed.
    ListItem {
        index: usize,
        key: String,
        item: String,
    },
    /// A row of `data` closed, at each `;` and at the value's end.
    Row {
        index: usize,
        key: String,
        row: String,
    },
    CallDone {
        index: usize,
        call: Call,
    },
    /// The act ended: its calls, or where it stopped making sense (the
    /// words `calls::parse` says).
    ActDone(Result<usize, String>),
}

/// The values read as lists of items.
const LISTS: &[&str] = &["options", "choices", "rows", "fields", "data", "path"];

// ──────────────────────── The call machine ─────────────────────

#[derive(Clone, Debug, PartialEq)]
enum C {
    Start,
    Between,
    Name(String),
    AfterName(String),
    Args,
    Key(String),
    AfterEq,
    Quoted { q: char, esc: bool },
    Bare,
    Trailing,
    Over,
}

/// `calls::parse`, one character at a time.
#[derive(Clone, Debug)]
struct Calls {
    st: C,
    name: String,
    args: Vec<(String, String)>,
    key: String,
    value: String,
    delta: String,
    item: String,
    row: String,
    calls: Vec<Call>,
    bracketed: bool,
    /// Calls before a closing `]` (or a stray `` ` ``) was seen.
    trailed: Option<(usize, bool)>,
    result: Option<Result<usize, String>>,
}

impl Calls {
    fn new() -> Calls {
        Calls {
            st: C::Start,
            name: String::new(),
            args: Vec::new(),
            key: String::new(),
            value: String::new(),
            delta: String::new(),
            item: String::new(),
            row: String::new(),
            calls: Vec::new(),
            bracketed: false,
            trailed: None,
            result: None,
        }
    }

    fn fail(&mut self, why: String, out: &mut Vec<Event>) {
        self.flush(out);
        self.st = C::Over;
        self.result = Some(Err(why.clone()));
        out.push(Event::ActDone(Err(why)));
    }

    fn expected(&self) -> String {
        format!("expected a call like `name(key=\"value\")` after {} call(s)", self.calls.len())
    }

    fn index(&self) -> usize {
        self.calls.len()
    }

    fn flush(&mut self, out: &mut Vec<Event>) {
        if !self.delta.is_empty() {
            out.push(Event::ArgDelta { index: self.index(), key: self.key.clone(), chunk: std::mem::take(&mut self.delta) });
        }
    }

    /// One character of the value: kept, and its list items closed.
    fn value_char(&mut self, c: char, out: &mut Vec<Event>) {
        self.value.push(c);
        self.delta.push(c);
        if !LISTS.contains(&self.key.as_str()) {
            return;
        }
        let data = self.key == "data";
        if c == '|' || (data && c == ';') {
            self.flush(out);
            out.push(Event::ListItem { index: self.index(), key: self.key.clone(), item: self.item.trim().to_string() });
            self.item.clear();
        } else {
            self.item.push(c);
        }
        if data {
            if c == ';' {
                out.push(Event::Row { index: self.index(), key: self.key.clone(), row: self.row.trim().to_string() });
                self.row.clear();
            } else {
                self.row.push(c);
            }
        }
    }

    fn value_done(&mut self, out: &mut Vec<Event>, bare: bool) {
        self.flush(out);
        let i = self.index();
        let key = std::mem::take(&mut self.key);
        let mut value = std::mem::take(&mut self.value);
        if bare {
            value = value.trim().to_string();
        }
        if LISTS.contains(&key.as_str()) {
            out.push(Event::ListItem { index: i, key: key.clone(), item: self.item.trim().to_string() });
            if key == "data" {
                out.push(Event::Row { index: i, key: key.clone(), row: self.row.trim().to_string() });
            }
        }
        self.item.clear();
        self.row.clear();
        out.push(Event::ArgDone { index: i, key: key.clone(), value: value.clone() });
        self.args.push((key, value));
        self.st = C::Args;
    }

    fn push(&mut self, c: char, out: &mut Vec<Event>) {
        loop {
            let st = std::mem::replace(&mut self.st, C::Over);
            self.st = match st {
                C::Over => C::Over,
                C::Start if c.is_whitespace() || c == '`' => C::Start,
                C::Start if c == '[' => {
                    self.bracketed = true;
                    C::Between
                }
                C::Start => {
                    self.st = C::Between;
                    continue;
                }
                C::Between if c.is_whitespace() || c == ';' || c == ',' => C::Between,
                C::Between | C::Trailing if c == '`' || c == ']' => {
                    let closed = c == ']' || self.trailed.is_some_and(|(_, b)| b);
                    self.trailed = Some((self.trailed.map_or(self.calls.len(), |(n, _)| n), closed));
                    C::Trailing
                }
                C::Trailing if c.is_whitespace() => C::Trailing,
                C::Trailing => {
                    let why = format!("expected a call like `name(key=\"value\")` after {} call(s)", self.trailed.map_or(0, |(n, _)| n));
                    self.fail(why, out);
                    C::Over
                }
                C::Between if is_name(c) => C::Name(c.to_string()),
                C::Between => {
                    self.fail(self.expected(), out);
                    C::Over
                }
                C::Name(mut n) if is_name(c) => {
                    n.push(c);
                    C::Name(n)
                }
                C::Name(n) | C::AfterName(n) if c.is_whitespace() => C::AfterName(n),
                C::Name(n) | C::AfterName(n) if c == '(' => {
                    out.push(Event::CallStart { index: self.index(), name: n.clone() });
                    self.name = n;
                    self.args.clear();
                    C::Args
                }
                // `none]` or `none` in backticks: a lone none, its brackets checked at the end.
                C::Name(n) | C::AfterName(n) if (c == ']' || c == '`') && self.calls.is_empty() && lone(&n) => {
                    self.trailed = Some((0, c == ']'));
                    C::Trailing
                }
                C::Name(_) | C::AfterName(_) => {
                    self.fail(self.expected(), out);
                    C::Over
                }
                C::Args if c.is_whitespace() || c == ',' => C::Args,
                C::Args if c == ')' => {
                    let call = Call { name: std::mem::take(&mut self.name), args: std::mem::take(&mut self.args) };
                    out.push(Event::CallDone { index: self.index(), call: call.clone() });
                    self.calls.push(call);
                    C::Between
                }
                C::Args => {
                    self.st = C::Key(String::new());
                    continue;
                }
                C::Key(k) if c == ')' || c == ',' => {
                    let _ = k;
                    let why = format!("in `{}(...)`: an argument is written `key=\"value\"`", self.name);
                    self.fail(why, out);
                    C::Over
                }
                C::Key(k) if c == '=' => {
                    let k = k.trim().to_string();
                    if k.is_empty() {
                        let why = format!("in `{}(...)`: an argument is written `key=\"value\"`", self.name);
                        self.fail(why, out);
                        C::Over
                    } else {
                        out.push(Event::ArgStart { index: self.index(), key: k.clone() });
                        self.key = k;
                        C::AfterEq
                    }
                }
                C::Key(mut k) => {
                    k.push(c);
                    C::Key(k)
                }
                C::AfterEq if c.is_whitespace() => C::AfterEq,
                C::AfterEq if c == '"' || c == '\'' => C::Quoted { q: c, esc: false },
                C::AfterEq => {
                    self.st = C::Bare;
                    continue;
                }
                C::Quoted { q, esc: true } => {
                    self.value_char(
                        match c {
                            'n' => '\n',
                            't' => '\t',
                            c => c,
                        },
                        out,
                    );
                    C::Quoted { q, esc: false }
                }
                C::Quoted { q, esc: false } if c == '\\' => C::Quoted { q, esc: true },
                C::Quoted { q, esc: false } if c == q => {
                    self.value_done(out, false);
                    C::Args
                }
                C::Quoted { q, esc: false } => {
                    self.value_char(c, out);
                    C::Quoted { q, esc: false }
                }
                C::Bare if c == ',' || c == ')' => {
                    self.value_done(out, true);
                    continue;
                }
                C::Bare => {
                    self.value_char(c, out);
                    C::Bare
                }
            };
            return;
        }
    }

    /// The act's text is all in: close it as `calls::parse` would.
    fn end(&mut self, out: &mut Vec<Event>) {
        let st = std::mem::replace(&mut self.st, C::Over);
        let why = match st {
            C::Over => return,
            C::Start => None,
            C::Trailing if self.calls.is_empty() && self.trailed.is_some_and(|(_, b)| b) == self.bracketed => None,
            C::Between | C::Trailing => match self.trailed {
                Some((n, true)) if !self.bracketed => Some(format!("expected a call like `name(key=\"value\")` after {n} call(s)")),
                Some((_, true)) => None,
                _ if self.bracketed => Some("expected a call like `name(key=\"value\")` after 0 call(s)".into()),
                _ => None,
            },
            C::Name(n) | C::AfterName(n) => {
                let lone = self.calls.is_empty() && lone(&n);
                let closed = self.trailed.is_some_and(|(_, b)| b) == self.bracketed;
                if lone && closed { None } else { Some(self.expected()) }
            }
            C::Key(_) => Some(format!("in `{}(...)`: an argument is written `key=\"value\"`", self.name)),
            C::Quoted { q, .. } => Some(format!("in `{}(...)`: the value of {} is never closed with {q}", self.name, self.key)),
            C::Args | C::AfterEq | C::Bare => Some(format!("`{}(` is never closed", self.name)),
        };
        match why {
            Some(why) => self.fail(why, out),
            None => {
                self.result = Some(Ok(self.calls.len()));
                out.push(Event::ActDone(Ok(self.calls.len())));
            }
        }
    }
}

/// A whole act that means no calls.
fn lone(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "none" | "-" | "null")
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-')
}

// ─────────────────────────── Think ─────────────────────────────

/// Splits prose into text and `<think>…</think>`, tags split anywhere.
#[derive(Clone, Debug, Default)]
struct Think {
    hold: String,
    inside: bool,
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

impl Think {
    fn push(&mut self, s: &str, out: &mut Vec<Event>) {
        self.hold.push_str(s);
        loop {
            let tag = if self.inside { CLOSE } else { OPEN };
            let emit = |t: String, inside: bool, out: &mut Vec<Event>| {
                if !t.is_empty() {
                    out.push(if inside { Event::Think(t) } else { Event::Text(t) });
                }
            };
            if let Some(at) = self.hold.find(tag) {
                let before: String = self.hold.drain(..at).collect();
                self.hold.drain(..tag.len());
                emit(before, self.inside, out);
                self.inside = !self.inside;
                continue;
            }
            // Keep the longest end that may begin the tag.
            let keep = (1..tag.len())
                .rev()
                .find(|n| {
                    self.hold.len() >= *n
                        && self.hold.is_char_boundary(self.hold.len() - n)
                        && tag.starts_with(&self.hold[self.hold.len() - n..])
                })
                .unwrap_or(0);
            let cut = self.hold.len() - keep;
            let t: String = self.hold.drain(..cut).collect();
            emit(t, self.inside, out);
            return;
        }
    }

    fn end(&mut self, out: &mut Vec<Event>) {
        let t = std::mem::take(&mut self.hold);
        if !t.is_empty() {
            out.push(if self.inside { Event::Think(t) } else { Event::Text(t) });
        }
    }
}

// ─────────────────────────── Parser ────────────────────────────

#[derive(Clone, Debug)]
enum Mode {
    /// No contract: the whole answer is the reply.
    Free,
    Toon {
        sep: String,
        keys: Vec<String>,
    },
    Json,
}

#[derive(Clone, Debug, PartialEq)]
enum Line {
    /// The start of a line, not yet known to open a field.
    Start(String),
    /// A fence line: skipped.
    Fence,
    Body,
}

#[derive(Clone, Debug, PartialEq)]
enum J {
    Seek,
    Key {
        key: String,
        esc: Esc,
    },
    WantKey,
    Colon(String),
    WantValue(String),
    Str {
        key: String,
        esc: Esc,
    },
    /// A value that is not a string (or a nested one): skipped.
    Skip {
        depth: usize,
        string: bool,
        esc: bool,
    },
    AfterValue,
    Done,
}

#[derive(Clone, Debug, PartialEq, Default)]
enum Esc {
    #[default]
    No,
    Yes,
    /// `\u` and the hex digits so far.
    U(String),
    /// A high surrogate waiting for its low half (`\uD83D\u…`).
    High(u32, String),
}

/// The resumable reader of one answer (`push` any split of its text).
#[derive(Clone, Debug)]
pub struct Parser {
    mode: Mode,
    line: Line,
    /// The field being read (an index into the keys), and its name.
    field: Option<String>,
    first_line: bool,
    /// Something not blank on the field's first line (its start is trimmed).
    started: bool,
    held_ws: String,
    held_cr: bool,
    act_seen: bool,
    in_act: bool,
    calls: Calls,
    think: Think,
    j: J,
    ended: bool,
    /// Fields named `act` (more than one: the full parser says which counts).
    acts: usize,
    whole: String,
    format: Format,
    response: crate::response::Responses,
}

impl Parser {
    /// For answers of `e`'s contract and format.
    pub fn new(e: &Engine) -> Parser {
        let mode = if e.response.is_empty() {
            Mode::Free
        } else {
            match &e.format {
                Format::Json(_) => Mode::Json,
                Format::Toon(t) => {
                    Mode::Toon { sep: t.separator.trim().to_string(), keys: e.response.fields.iter().map(|f| f.key.clone()).collect() }
                }
            }
        };
        Parser {
            mode,
            line: Line::Start(String::new()),
            field: None,
            first_line: false,
            started: false,
            held_ws: String::new(),
            held_cr: false,
            act_seen: false,
            in_act: false,
            calls: Calls::new(),
            think: Think::default(),
            j: J::Seek,
            ended: false,
            acts: 0,
            whole: String::new(),
            format: e.format.clone(),
            response: e.response.clone(),
        }
    }

    /// The calls closed so far, in written order.
    pub fn calls(&self) -> &[Call] {
        &self.calls.calls
    }

    /// The act's calls once it ended (`None`: not yet, or no act).
    pub fn result(&self) -> Option<Result<Vec<Call>, String>> {
        // Several acts: which one counts is the contract reader's to say.
        if self.acts > 1
            && self.ended
            && let Ok(answer) = crate::response::ResponseFormat::parse(&self.format, &self.response, &self.whole)
        {
            return Some(crate::calls::parse(crate::response::Responses::get(&answer, ACT).unwrap_or("")));
        }
        self.calls.result.clone().map(|r| r.map(|_| self.calls.calls.clone()))
    }

    /// The next piece of the answer; what it said.
    pub fn push(&mut self, text: &str) -> Vec<Event> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        self.whole.push_str(text);
        match self.mode {
            Mode::Free => self.think.push(text, &mut out),
            Mode::Toon { .. } => {
                for c in text.chars() {
                    self.toon_cr(c, &mut out);
                }
            }
            Mode::Json => {
                for c in text.chars() {
                    self.json(c, &mut out);
                }
            }
        }
        self.calls.flush(&mut out);
        out
    }

    /// The answer is all in.
    pub fn finish(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        if let Mode::Toon { .. } = self.mode {
            if std::mem::take(&mut self.held_cr) {
                self.toon(Some('\r'), &mut out);
            }
            self.toon(None, &mut out);
        }
        if self.in_act {
            self.in_act = false;
            self.calls.end(&mut out);
        }
        self.think.end(&mut out);
        self.ended = true;
        out
    }

    // ── toon ──

    /// `lines()` drops the `\r` of a `\r\n`.
    fn toon_cr(&mut self, c: char, out: &mut Vec<Event>) {
        if std::mem::take(&mut self.held_cr) && c != '\n' {
            self.toon(Some('\r'), out);
        }
        if c == '\r' {
            self.held_cr = true;
        } else {
            self.toon(Some(c), out);
        }
    }

    /// One character (`None`: the end), through the line machine.
    fn toon(&mut self, c: Option<char>, out: &mut Vec<Event>) {
        match (&mut self.line, c) {
            (Line::Start(buf), Some(ch)) if ch != '\n' => {
                buf.push(ch);
                let buf = buf.clone();
                self.decide(&buf, false, out);
            }
            (Line::Start(buf), _) => {
                // The line ended before it was known: decide on all of it.
                let buf = buf.clone();
                if !buf.is_empty() || c.is_some() {
                    self.decide(&buf, true, out);
                }
                if c.is_some() {
                    self.line_end(out);
                }
            }
            (Line::Fence, Some('\n')) => self.line = Line::Start(String::new()),
            (Line::Fence, _) => {}
            (Line::Body, Some('\n')) => self.line_end(out),
            (Line::Body, Some(ch)) => self.body(ch, out),
            (Line::Body, None) => {}
        }
    }

    fn line_end(&mut self, out: &mut Vec<Event>) {
        if self.field.is_none() {
            self.think.push("\n", out);
        }
        self.first_line = false;
        self.held_ws.clear();
        self.line = Line::Start(String::new());
    }

    /// Whether the line so far opens a field, as `Toon::parse` decides.
    fn decide(&mut self, buf: &str, complete: bool, out: &mut Vec<Event>) {
        let Mode::Toon { sep, keys } = &self.mode else { return };
        let t = buf.trim_start();
        if t.starts_with("```") {
            self.line = Line::Fence;
            return;
        }
        if !complete && (t.is_empty() || (t.chars().all(|c| c == '`')) || t == "-") {
            return;
        }
        let rest = t.strip_prefix("- ").unwrap_or(t);
        let mut opened = None;
        for key in keys {
            match field_match(rest, key, sep) {
                Match::Yes(v) => {
                    opened = Some((key.clone(), v.to_string()));
                    break;
                }
                Match::Maybe if !complete => return,
                _ => {}
            }
        }
        match opened {
            Some((key, value)) => self.open(key, &value, out),
            None => {
                self.line = Line::Body;
                if self.field.is_some() {
                    self.body('\n', out);
                }
                for ch in buf.chars() {
                    self.body(ch, out);
                }
            }
        }
    }

    fn open(&mut self, key: String, value: &str, out: &mut Vec<Event>) {
        if self.in_act {
            self.in_act = false;
            self.calls.end(out);
        }
        out.push(Event::Field(key.clone()));
        if key == ACT {
            self.acts += 1;
        }
        if key == ACT && !self.act_seen {
            self.act_seen = true;
            self.in_act = true;
        }
        self.field = Some(key);
        self.first_line = true;
        self.started = false;
        self.held_ws.clear();
        self.line = Line::Body;
        for ch in value.trim_start().chars() {
            self.body(ch, out);
        }
    }

    /// A character of the current field's value.
    fn body(&mut self, c: char, out: &mut Vec<Event>) {
        if self.field.as_deref() == Some(ACT) && !self.in_act {
            return; // a second act: the first one counts
        }
        if self.first_line && c.is_whitespace() && c != '\n' {
            if self.started {
                self.held_ws.push(c);
            }
            return;
        }
        self.started = true;
        let held = std::mem::take(&mut self.held_ws);
        if self.in_act {
            for h in held.chars() {
                self.calls.push(h, out);
            }
            self.calls.push(c, out);
        } else {
            let mut s = held;
            s.push(c);
            self.think.push(&s, out);
        }
    }

    // ── json ──

    fn json(&mut self, c: char, out: &mut Vec<Event>) {
        let j = std::mem::replace(&mut self.j, J::Done);
        self.j = match j {
            J::Done => J::Done,
            J::Seek if c == '{' => J::WantKey,
            J::Seek => J::Seek,
            J::WantKey if c == '"' => J::Key { key: String::new(), esc: Esc::No },
            J::WantKey if c == '}' => self.json_end(out),
            J::WantKey => J::WantKey,
            J::Key { mut key, esc } => match unescape(c, esc, &mut key) {
                Some(esc) => J::Key { key, esc },
                None => J::Colon(key),
            },
            J::Colon(k) if c == ':' => J::WantValue(k),
            J::Colon(k) => J::Colon(k),
            J::WantValue(k) if c.is_whitespace() => J::WantValue(k),
            J::WantValue(key) if c == '"' => {
                out.push(Event::Field(key.clone()));
                if key == ACT {
                    self.acts += 1;
                }
                if key == ACT && !self.act_seen {
                    self.act_seen = true;
                    self.in_act = true;
                }
                self.field = Some(key.clone());
                J::Str { key, esc: Esc::No }
            }
            J::WantValue(_) if c == '{' || c == '[' => J::Skip { depth: 1, string: false, esc: false },
            J::WantValue(_) => J::Skip { depth: 0, string: false, esc: false },
            J::Str { key, esc } => {
                let mut got = String::new();
                match unescape(c, esc, &mut got) {
                    Some(esc) => {
                        self.json_text(&key, &got, out);
                        J::Str { key, esc }
                    }
                    None => {
                        if key == ACT && self.in_act {
                            self.in_act = false;
                            self.calls.end(out);
                        }
                        J::AfterValue
                    }
                }
            }
            J::Skip { depth, string: true, esc } => match (esc, c) {
                (true, _) => J::Skip { depth, string: true, esc: false },
                (false, '\\') => J::Skip { depth, string: true, esc: true },
                (false, '"') => J::Skip { depth, string: false, esc: false },
                _ => J::Skip { depth, string: true, esc: false },
            },
            J::Skip { depth, .. } if c == '"' => J::Skip { depth, string: true, esc: false },
            J::Skip { depth, .. } if c == '{' || c == '[' => J::Skip { depth: depth + 1, string: false, esc: false },
            J::Skip { depth: 0, .. } if c == '}' || c == ']' => self.json_end(out),
            J::Skip { depth: 1, .. } if c == '}' || c == ']' => J::AfterValue,
            J::Skip { depth, .. } if c == '}' || c == ']' => J::Skip { depth: depth - 1, string: false, esc: false },
            J::Skip { depth: 0, .. } if c == ',' => J::WantKey,
            J::Skip { depth, .. } => J::Skip { depth, string: false, esc: false },
            J::AfterValue if c == ',' => J::WantKey,
            J::AfterValue if c == '}' => self.json_end(out),
            J::AfterValue => J::AfterValue,
        };
    }

    fn json_text(&mut self, key: &str, got: &str, out: &mut Vec<Event>) {
        if got.is_empty() {
            return;
        }
        if key == ACT && self.in_act {
            for ch in got.chars() {
                self.calls.push(ch, out);
            }
        } else if key != ACT {
            self.think.push(got, out);
        }
    }

    fn json_end(&mut self, out: &mut Vec<Event>) -> J {
        if self.in_act {
            self.in_act = false;
            self.calls.end(out);
        }
        J::Done
    }
}

/// One character of a JSON string: `None` when it closes the string,
/// else the escape state after it (what it decoded is pushed to `to`).
fn unescape(c: char, esc: Esc, to: &mut String) -> Option<Esc> {
    match esc {
        Esc::No if c == '"' => None,
        Esc::No if c == '\\' => Some(Esc::Yes),
        Esc::No => {
            to.push(c);
            Some(Esc::No)
        }
        Esc::Yes => {
            match c {
                'n' => to.push('\n'),
                't' => to.push('\t'),
                'r' => to.push('\r'),
                'b' => to.push('\u{8}'),
                'f' => to.push('\u{c}'),
                'u' => return Some(Esc::U(String::new())),
                c => to.push(c),
            }
            Some(Esc::No)
        }
        Esc::U(mut hex) => {
            hex.push(c);
            if hex.len() < 4 {
                return Some(Esc::U(hex));
            }
            let n = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
            if (0xD800..0xDC00).contains(&n) {
                return Some(Esc::High(n, String::new()));
            }
            to.push(char::from_u32(n).unwrap_or('\u{FFFD}'));
            Some(Esc::No)
        }
        Esc::High(hi, mut rest) => {
            rest.push(c);
            // `\u` then four digits.
            if rest.len() < 6 {
                if (rest.len() == 1 && c != '\\') || (rest.len() == 2 && c != 'u') {
                    to.push('\u{FFFD}');
                    let mut s = String::new();
                    let more = unescape_str(&rest, &mut s);
                    to.push_str(&s);
                    return more;
                }
                return Some(Esc::High(hi, rest));
            }
            let lo = u32::from_str_radix(&rest[2..], 16).unwrap_or(0);
            let n = 0x10000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
            to.push(char::from_u32(n).unwrap_or('\u{FFFD}'));
            Some(Esc::No)
        }
    }
}

fn unescape_str(s: &str, to: &mut String) -> Option<Esc> {
    let mut esc = Esc::No;
    for c in s.chars() {
        esc = unescape(c, esc, to)?;
    }
    Some(esc)
}

enum Match<'a> {
    Yes(&'a str),
    Maybe,
    No,
}

/// Whether `rest` (a line, trimmed, its `- ` dropped) opens `key`.
fn field_match<'a>(rest: &'a str, key: &str, sep: &str) -> Match<'a> {
    if rest.len() < key.len() {
        return if key.starts_with(rest) { Match::Maybe } else { Match::No };
    }
    let Some(after) = rest.strip_prefix(key) else { return Match::No };
    let a = after.trim_start();
    if let Some(v) = a.strip_prefix(sep) {
        return Match::Yes(v);
    }
    if a.is_empty() || sep.starts_with(a) { Match::Maybe } else { Match::No }
}

// ──────────────────────────── View ─────────────────────────────

/// The reply as far as it is written (renderer-neutral).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    /// The ask, once `ask=` closed (options and rows as far as written).
    pub ask: Option<Ask>,
    pub show: Option<Show>,
    /// The reply call closed: `ask` and `show` are as `Parsed::of` reads them.
    pub done: bool,
    pub notes: Vec<String>,
}

/// What a shell draws while an answer streams.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// `<think>` and reasoning: its own fold, never the reply.
    pub think: String,
    /// Prose outside the calls (the `thought` field).
    pub prose: String,
    /// The calls of the act so far, by name, in order (the last may be open).
    pub calls: Vec<String>,
    pub closed: usize,
    /// The first reply call, as far as written.
    pub reply: Option<Reply>,
    /// The act stopped making sense (the full parser says it again).
    pub error: Option<String>,
    /// No contract: the prose is the reply.
    free: bool,
    reply_index: Option<usize>,
    args: Vec<(String, String)>,
    open: Option<(String, String)>,
    field: Option<String>,
}

impl View {
    pub fn new(e: &Engine) -> View {
        let free = e.response.is_empty();
        View { free, reply: free.then(Reply::default), ..View::default() }
    }

    /// Take one event.
    pub fn apply(&mut self, ev: &Event) {
        match ev {
            Event::Think(t) => self.think.push_str(t),
            Event::Text(t) if self.free => {
                let r = self.reply.get_or_insert_with(Reply::default);
                r.text.push_str(t);
                r.text = r.text.trim_start().to_string();
            }
            Event::Text(t) => {
                if self.field.as_deref() != Some(ACT) {
                    self.prose.push_str(t);
                }
            }
            Event::Field(f) => {
                if !self.prose.is_empty() && !self.prose.ends_with('\n') && f != ACT {
                    self.prose.push('\n');
                }
                self.field = Some(f.clone());
            }
            Event::CallStart { index, name } => {
                self.calls.push(name.clone());
                if name == "reply" && self.reply_index.is_none() {
                    self.reply_index = Some(*index);
                    self.args.clear();
                    self.reply = Some(Reply::default());
                }
            }
            Event::ArgStart { index, key } if Some(*index) == self.reply_index => {
                self.open = Some((key.clone(), String::new()));
                self.refresh();
            }
            Event::ArgDelta { index, chunk, .. } if Some(*index) == self.reply_index => {
                if let Some((_, v)) = &mut self.open {
                    v.push_str(chunk);
                }
                self.refresh();
            }
            Event::ArgDone { index, key, value } if Some(*index) == self.reply_index => {
                self.open = None;
                self.args.push((key.clone(), value.clone()));
                self.refresh();
            }
            Event::CallDone { index, call } => {
                self.closed = index + 1;
                if Some(*index) == self.reply_index {
                    let p = Parsed::of(call);
                    self.reply = Some(Reply {
                        text: call.arg("text").unwrap_or("").to_string(),
                        ask: p.ask,
                        show: p.show,
                        done: true,
                        notes: p.notes,
                    });
                }
            }
            Event::ActDone(Err(why)) => self.error = Some(why.clone()),
            _ => {}
        }
    }

    /// The reply's provisional components from the arguments so far.
    fn refresh(&mut self) {
        let Some(r) = &mut self.reply else { return };
        if r.done {
            return;
        }
        let mut args = self.args.clone();
        if let Some((k, v)) = &self.open {
            match k.as_str() {
                "text" | "title" | "caption" | "placeholder" | "unit" => args.push((k.clone(), v.clone())),
                "data" if self.args.iter().any(|(a, v)| a == "show" && show::kind_name(v) == Some("progress")) => {}
                "options" | "choices" | "rows" | "fields" | "data" => {
                    let list = self.args.iter().any(|(a, v)| a == "show" && show::kind_name(v) == Some("list"));
                    if let Some(cut) = complete_items(k, v, list) {
                        args.push((k.clone(), cut));
                    }
                }
                _ => {}
            }
        }
        let call = Call { name: "reply".into(), args };
        r.text = call.arg("text").unwrap_or("").to_string();
        let p = Parsed::of(&call);
        r.ask = p.ask.or_else(|| skeleton_ask(&call));
        r.show = p.show.or_else(|| skeleton_show(&call));
    }

    /// Is a component known yet?
    pub fn has_component(&self) -> bool {
        self.reply.as_ref().is_some_and(|r| r.ask.is_some() || r.show.is_some())
    }
}

/// The part of a list value whose items are closed (`None`: none yet).
fn complete_items(key: &str, v: &str, list: bool) -> Option<String> {
    let mut depth = 0i32;
    let mut cut = None;
    for (i, c) in v.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '|' if key != "data" && (key != "fields" || depth <= 0) => cut = Some(i),
            ';' | '\n' if key == "data" || (key == "fields" && depth <= 0) => cut = Some(i),
            '|' if key == "data" && list => cut = Some(i),
            _ => {}
        }
    }
    cut.map(|i| v[..i].to_string()).filter(|s| !s.trim().is_empty())
}

/// An ask whose kind is known but whose list is still too short to read.
fn skeleton_ask(call: &Call) -> Option<Ask> {
    let arg = |k: &str| call.arg(k).map(str::trim).filter(|v| !v.is_empty());
    let kind = match arg("ask") {
        Some(v) => ask::kind_name(v)?,
        None if arg("fields").is_some() => "form",
        None if arg("options").or(arg("choices")).is_some() => "choice",
        None => return None,
    };
    let items = arg("options").or(arg("choices")).map(ask::split).unwrap_or_default();
    let title = arg("title").unwrap_or("").to_string();
    Some(match kind {
        "choice" => Ask::Choice { options: items },
        "multi" => Ask::Multi { options: items },
        "review" | "card" => Ask::Review {
            title,
            rows: arg("rows")
                .map(ask::split)
                .unwrap_or_default()
                .into_iter()
                .map(|r| match r.split_once(':') {
                    Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                    None => (r, String::new()),
                })
                .collect(),
        },
        "form" => Ask::Form { title, fields: Vec::new() },
        _ => return None,
    })
}

/// A display whose kind is known but whose data is not yet readable.
fn skeleton_show(call: &Call) -> Option<Show> {
    let v = call.arg("show").map(str::trim).filter(|v| !v.is_empty())?;
    let caption = call.arg("caption").unwrap_or("").trim().to_string();
    let first = call
        .arg("data")
        .and_then(|d| d.split([';', '\n']).map(str::trim).find(|r| !r.is_empty()))
        .map(|r| r.split('|').map(|c| c.trim().to_string()).collect());
    Some(match show::kind_name(v)? {
        "table" => Show::Table { caption, head: first.unwrap_or_default(), rows: Vec::new() },
        "list" => Show::List { caption, items: Vec::new() },
        "progress" => Show::Progress { label: caption, value: 0.0, max: 1.0 },
        _ => Show::Stat { caption, stats: Vec::new() },
    })
}

// ─────────────────────────── Reader ────────────────────────────

/// Parser and view together: what a shell keeps per streaming answer.
#[derive(Clone, Debug)]
pub struct Reader {
    pub parser: Parser,
    pub view: View,
    /// The answer's text so far.
    pub text: String,
}

impl Reader {
    pub fn new(e: &Engine) -> Reader {
        Reader { parser: Parser::new(e), view: View::new(e), text: String::new() }
    }

    /// One delta from the server; the events it made.
    pub fn feed(&mut self, d: &Delta) -> Vec<Event> {
        let events = match d {
            Delta::Text(t) => {
                self.text.push_str(t);
                self.parser.push(t)
            }
            Delta::Think(t) => vec![Event::Think(t.clone())],
        };
        for ev in &events {
            self.view.apply(ev);
        }
        events
    }

    /// The answer is all in.
    pub fn finish(&mut self) -> Vec<Event> {
        let events = self.parser.finish();
        for ev in &events {
            self.view.apply(ev);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calls;
    use crate::inference::{Inference, OpenAi};
    use crate::response::{ResponseFormat, Responses};

    fn engine(format: &str) -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.format = Format::parse_name(format).unwrap();
        e
    }

    /// What one-shot parsing says the act's calls are (`None`: the format does not read).
    fn one_shot(e: &Engine, text: &str) -> Option<Result<Vec<Call>, String>> {
        let answer = e.format.parse(&e.response, text).ok()?;
        Some(calls::parse(Responses::get(&answer, ACT).unwrap_or("")))
    }

    /// Events with neighbouring text and deltas merged, so two splits compare.
    fn normal(events: Vec<Event>) -> Vec<Event> {
        let mut out: Vec<Event> = Vec::new();
        for ev in events {
            match (out.last_mut(), ev) {
                (Some(Event::Text(a)), Event::Text(b)) | (Some(Event::Think(a)), Event::Think(b)) => a.push_str(&b),
                (Some(Event::ArgDelta { index: i, key: k, chunk }), Event::ArgDelta { index, key, chunk: more })
                    if *i == index && *k == key =>
                {
                    chunk.push_str(&more)
                }
                (_, ev) => out.push(ev),
            }
        }
        out
    }

    fn feed(e: &Engine, pieces: &[&str]) -> (Vec<Event>, Parser, View) {
        let mut r = Reader::new(e);
        let mut events = Vec::new();
        for p in pieces {
            events.extend(r.feed(&Delta::Text(p.to_string())));
        }
        events.extend(r.finish());
        (events, r.parser, r.view)
    }

    /// The answers the property test reads: every registry example, forms,
    /// displays, escapes, think blocks, many calls in one act, near misses.
    fn corpus() -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        for entry in ask::registry() {
            out.push(("toon", format!("thought: ask {}\nact: {}", entry.ask.name(), entry.ask.call(entry.text))));
        }
        for entry in show::registry() {
            out.push(("toon", format!("thought: show it\nact: {}", entry.show.call(entry.text))));
        }
        let fixed = [
            "thought: plan\nact: open(path=\"src/a.rs|src/b.rs\"); edit(path=\"src/a.rs\", old=\"x\", new=\"y\"); save(path=\"src/a.rs\"); run(command=\"cargo test\")",
            "thought: write\nact: write(name=\"plan\", text=\"a, b; c)\\n\\\"d\\\"\"); read(name=plan) ; reply(text='hi')",
            "<think>Let me think.\nact: not yet\n</think>\nthought: ok\nact: reply(text=\"done\", ask=\"confirm\")",
            "thought: multi line\nsecond line\nact: reply(text=\"line one   \nline two\", ask=\"choice\", options=\"A|B\")",
            "Sure!\n```\nthought: fenced\n- act: reply(text=\"ok\")\n```",
            "thought: form\nact: reply(text=\"Details\", ask=\"form\", title=\"Booking\", fields=\"Day:date|Guests:number(1-12)|Seat:choice(inside|terrace)|Notes:text\")",
            "thought: both\nact: reply(text=\"Here\", show=\"table\", data=\"Name|Age;Ada|36;Bob|41\", ask=\"choice\", options=\"Ada|Bob\")",
            "thought: nothing\nact: none",
            "thought: list\nact: [read(name=\"a\"), read(name=\"b\")]",
            "thought: ticks\nact: `reply(text=\"x\")`",
            "thought: bare\nact: read(name = plan , ) ; list()",
            "thought: unicode\nact: reply(text=\"café ☕ — 日本\", ask=\"choice\", options=\"é|☕\")",
            "thought: crlf\r\nact: reply(text=\"a\r\nb\")\r\n",
            "thought: broken\nact: reply(text=\"open",
            "thought: broken\nact: just words",
            "thought: broken\nact: reply(\"no key\")",
            "thought: broken\nact: a(); ]",
            "thought: broken\nact: [a()",
            "thought: escapes\nact: reply(text=\"tab\\there \\\\ slash \\' q\", ask=\"text\", placeholder='it\\'s')",
            "thought: two acts\nact: reply(text=\"first\")\nact: reply(text=\"second\")",
            "act: reply(text=\"before thought\")\nthought: after",
            "thought: spaced key\n  act :  reply(text=\"x\")  ",
            "thought: act in a value\nact: write(name=\"n\", text=\"a\nthought: inside\nb\")",
            "thought: stray\nact: reply(text=\"x\") junk",
            "thought: review\nact: reply(text=\"Ok?\", ask=\"confirm\", rows=\"env: staging|time: 18:00\")",
        ];
        out.extend(fixed.iter().map(|t| ("toon", t.to_string())));
        let json = [
            r#"{"thought": "t", "act": "reply(text=\"hi, there\", ask=\"choice\", options=\"A|B\")"}"#,
            r#"Here: {"act": "write(name=\"a\", text=\"\\u00e9\\ud83d\\ude00\\n\"); reply(text=\"x\")", "thought": "t"} done"#,
            r#"```json
{"thought": {"nested": [1, "}"]}, "act": "reply(text=\"y\")"}
```"#,
        ];
        out.extend(json.iter().map(|t| ("json", t.to_string())));
        out
    }

    /// Byte offsets that are char boundaries.
    fn boundaries(t: &str) -> Vec<usize> {
        (0..=t.len()).filter(|i| t.is_char_boundary(*i)).collect()
    }

    #[test]
    fn every_split_gives_the_same_calls_as_one_shot_parsing() {
        let (mut checked, mut broken) = (0, 0);
        let all = corpus();
        for (format, text) in all.clone() {
            let e = engine(format);
            let (whole, parser, _) = feed(&e, &[&text]);
            let whole = normal(whole);
            let expected = one_shot(&e, &text);
            if let Some(expected) = &expected {
                assert_eq!(parser.result().as_ref(), Some(expected), "{format}: {text}");
                checked += 1;
                broken += usize::from(expected.is_err());
            }
            // Every split in two, at every boundary.
            for at in boundaries(&text) {
                let (events, p, _) = feed(&e, &[&text[..at], &text[at..]]);
                assert_eq!(normal(events), whole, "split at {at}: {text}");
                assert_eq!(p.result(), parser.result());
            }
            // Random chunkings (a fixed seed), and one char at a time.
            let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ text.len() as u64;
            let cuts = boundaries(&text);
            for _ in 0..40 {
                let mut pieces = Vec::new();
                let mut from = 0;
                for &b in &cuts {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    if b > from && seed.is_multiple_of(4) {
                        pieces.push(&text[from..b]);
                        from = b;
                    }
                }
                pieces.push(&text[from..]);
                let (events, _, _) = feed(&e, &pieces);
                assert_eq!(normal(events), whole, "random split: {text}");
            }
            let chars: Vec<String> = text.chars().map(String::from).collect();
            let refs: Vec<&str> = chars.iter().map(String::as_str).collect();
            assert_eq!(normal(feed(&e, &refs).0), whole, "char by char: {text}");
        }
        assert_eq!(checked, all.len(), "every answer of the corpus reads one-shot");
        assert_eq!(broken, 8, "the broken acts say the same words");
    }

    #[test]
    fn the_event_stream_is_consistent() {
        for (format, text) in corpus() {
            let e = engine(format);
            let (events, parser, _) = feed(&e, &[&text]);
            let mut open: Option<(usize, String)> = None;
            let (mut chunks, mut items, mut rows) = (String::new(), Vec::<String>::new(), Vec::<String>::new());
            let mut next = 0;
            let mut acts = 0;
            for ev in &events {
                match ev {
                    Event::CallStart { index, .. } => assert_eq!(*index, next, "{text}"),
                    Event::ArgStart { index, key } => {
                        assert!(open.is_none());
                        assert_eq!(*index, next);
                        open = Some((*index, key.clone()));
                        chunks.clear();
                        items.clear();
                        rows.clear();
                    }
                    Event::ArgDelta { index, key, chunk } => {
                        assert_eq!(open.as_ref(), Some(&(*index, key.clone())));
                        chunks.push_str(chunk);
                    }
                    Event::ListItem { item, .. } => items.push(item.clone()),
                    Event::Row { row, .. } => rows.push(row.clone()),
                    Event::ArgDone { index, key, value } => {
                        assert_eq!(open.take(), Some((*index, key.clone())));
                        assert!(chunks == *value || chunks.trim() == value, "{key}: {chunks:?} vs {value:?}");
                        if LISTS.contains(&key.as_str()) {
                            let seps: &[char] = if key == "data" { &['|', ';'] } else { &['|'] };
                            let want: Vec<String> = chunks.split(seps).map(|s| s.trim().to_string()).collect();
                            assert_eq!(items, want, "{key}");
                            if key == "data" {
                                assert_eq!(rows, chunks.split(';').map(|s| s.trim().to_string()).collect::<Vec<_>>());
                            }
                        }
                    }
                    Event::CallDone { index, call } => {
                        assert_eq!(*index, next);
                        assert_eq!(parser.calls()[*index], *call);
                        next += 1;
                    }
                    Event::ActDone(r) => {
                        acts += 1;
                        if let Ok(n) = r {
                            assert_eq!(*n, next);
                        }
                    }
                    _ => {}
                }
            }
            assert!(acts <= 1, "one act: {text}");
        }
    }

    #[test]
    fn the_view_grows_and_the_component_appears_once_its_kind_is_closed() {
        let e = engine("toon");
        let text = "thought: pick\nact: reply(ask=\"choice\", options=\"Red|Green|Blue\", text=\"Which colour?\")";
        let mut r = Reader::new(&e);
        let mut seen_kind_at = None;
        let mut options_seen = Vec::new();
        for (n, c) in text.chars().enumerate() {
            r.feed(&Delta::Text(c.to_string()));
            if let Some(Reply { ask: Some(Ask::Choice { options }), .. }) = &r.view.reply {
                seen_kind_at.get_or_insert(n);
                if options_seen.last() != Some(&options.len()) {
                    options_seen.push(options.len());
                }
            }
        }
        r.finish();
        let at = seen_kind_at.expect("the choice appeared");
        assert!(at < text.find("options").unwrap() + 10, "right after ask= closed: {at}");
        assert_eq!(options_seen, [0, 1, 2, 3], "options one by one");
        let reply = r.view.reply.unwrap();
        assert!(reply.done);
        assert_eq!(reply.text, "Which colour?");
        assert_eq!(r.view.prose, "pick");
        assert_eq!(reply, Reply { text: "Which colour?".into(), ask: ask::ask_in(&e, text), show: None, done: true, notes: vec![] });

        // A table's rows come one by one too, and the text grows.
        let text = "thought: t\nact: reply(show=\"table\", data=\"Name|Age;Ada|36;Bob|41\", text=\"Users.\")";
        let mut r = Reader::new(&e);
        let mut rows = Vec::new();
        for c in text.chars() {
            r.feed(&Delta::Text(c.to_string()));
            if let Some(Show::Table { rows: got, .. }) = r.view.reply.as_ref().and_then(|x| x.show.as_ref())
                && rows.last() != Some(&got.len())
            {
                rows.push(got.len());
            }
        }
        assert_eq!(rows, [0, 1, 2]);
    }

    #[test]
    fn think_goes_to_its_own_fold_and_free_text_is_the_reply() {
        let e = engine("toon");
        let (_, _, view) = feed(&e, &["<thi", "nk>hmm, red?</th", "ink>thought: go\nact: reply(text=\"Red.\")"]);
        assert_eq!(view.think, "hmm, red?");
        assert_eq!(view.reply.unwrap().text, "Red.");
        let mut free = engine("toon");
        free.response = Responses::default();
        let (_, _, view) = feed(&free, &["<think>a</think>", "Hello ", "there"]);
        assert_eq!((view.think.as_str(), view.reply.unwrap().text.as_str()), ("a", "Hello there"));
    }

    #[test]
    fn sse_reads_chunks_split_anywhere_and_falls_back_to_a_plain_body() {
        let req = Request { url: "http://x/v1/chat/completions".into(), body: "{\"model\":\"m\"}".into(), key_env: None };
        assert!(streaming(&req).body.contains("\"stream\":true"));
        let body = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"hé\"}}]}\r\n\r\n: keep-alive\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hm\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\ndata: [DONE]\n\n";
        let bytes = body.as_bytes();
        for at in 0..bytes.len() {
            let mut sse = Sse::new(&req);
            let mut got = sse.push(&bytes[..at]);
            got.extend(sse.push(&bytes[at..]));
            assert_eq!(got, [Delta::Text("hé".into()), Delta::Think("hm".into()), Delta::Text("llo".into())], "split at {at}");
            let out = sse.finish();
            assert_eq!(OpenAi::default().reply(&out), Ok("héllo".into()));
        }
        // Not streamed: the body as it came.
        let plain = r#"{"choices":[{"message":{"content":"whole"}}]}"#;
        let mut sse = Sse::new(&req);
        assert!(sse.push(plain.as_bytes()).is_empty());
        assert!(!sse.streamed());
        assert_eq!(OpenAi::default().reply(&sse.finish()), Ok("whole".into()));
        // An error mid-stream is the error body.
        let mut sse = Sse::new(&req);
        sse.push(b"data: {\"error\":{\"message\":\"context full\"}}\n\n");
        assert!(matches!(OpenAi::default().reply(&sse.finish()), Err(crate::inference::InferenceError::Api(m)) if m == "context full"));
        // Calls streamed the model's own way (`delta.tool_calls`, in pieces): read as the act, the reasoning before them.
        let mut sse = Sse::new(&req);
        sse.push(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Run it.\"}}]}\n\n");
        sse.push(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"run\",\"arguments\":\"\"}}]}}]}\n\n");
        sse.push(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\":\"}}]}}]}\n\n");
        sse.push(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ls -la\\\"}\"}},{\"index\":1,\"function\":{\"name\":\"list\",\"arguments\":\"{}\"}}]}}]}\n\ndata: [DONE]\n\n");
        assert_eq!(OpenAi::default().reply(&sse.finish()), Ok("Run it.\nact: run(command=\"ls -la\"); list()".into()));
        // The Responses API.
        let req = Request { url: "http://x/v1/responses".into(), ..req };
        let mut sse = Sse::new(&req);
        let d = sse.push(b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n");
        assert_eq!(d, [Delta::Text("ok".into())]);
        let ai = OpenAi { api: crate::inference::Api::Responses, ..OpenAi::default() };
        assert_eq!(ai.reply(&sse.finish()), Ok("ok".into()));
    }
}
