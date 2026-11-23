//! Spans: what an engine did and how long it took, as a tree of timed
//! records written while it runs (docs/analytics.md).
//!
//! ```text
//! task   "Rewrite the plan"  by user ─────────────────────────────── ready
//! └ turn trigger=user ──────────────────────────────────────────────
//!   ├ step 1 ───────────────┐ ├ step 2 ────────────┐
//!   │ ├ llm  model=m  ▇▇▇▇▇  │ │ ├ llm ▇▇▇▇▇▇        │
//!   │ └ tool read  ·         │ │ └ tool run ▇▇▇▇▇▇▇ │
//! └ wait on=child (between turns)
//! ```
//!
//! Spans, not metrics: each one says what it was (`kind`), where it sits
//! (`parent`), whose it was (`engine`), when (`start_ms`..`end_ms`, unix
//! milliseconds) and the facts of its kind (`attrs`). Every number the
//! views show is a query over them (`crate::analytics`).
//!
//! Pure. The runner (`crate::runner`) keeps a `Trace` and closes spans at
//! the times the shell passes in (`Runner::next_at`); the shell drains
//! them (`Runner::spans`), lets `Tasks` place them under their task (a
//! task outlives a turn: waits, callbacks and answers continue it) and
//! appends them to the engine's `spans.jsonl` beside its files. Spans are
//! never in the history and never in the prompt.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The append-only log, in each engine's folder.
pub const FILE: &str = "spans.jsonl";
/// Days older than the retention, rolled into daily totals.
pub const DAILY: &str = "spans-daily.jsonl";
/// Days of spans kept by default before they roll into daily totals.
pub const KEEP_DAYS: u32 = 30;

/// The log's path in a store of texts (`crate::store`): the browser's
/// IndexedDB keeps it under the same path a markdown folder does.
pub fn path(engine: &str) -> String {
    format!("engines/{engine}/{FILE}")
}

/// The daily totals' path.
pub fn daily_path(engine: &str) -> String {
    format!("engines/{engine}/{DAILY}")
}

/// What a span was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// One piece of work from the message that asked for it to its end,
    /// over as many turns as it took. `goal`, `by`, `outcome`, `turns`.
    Task,
    /// One turn of the engine. `trigger`, `outcome`, `steps`, `goal`,
    /// `asked` (its reply asks), `ask` (the kind written), `ask_note`.
    Turn,
    /// One step: the model's answer and its calls. `step`, `phase`, `calls`.
    Step,
    /// One model call. `model`, `provider`, `purpose` (step | compress),
    /// `prompt_tokens`, `completion_tokens`, `tokens_est`, `cost_usd`,
    /// `ttft_ms`, `first_component_ms`, `compressed`, `error`, `step`.
    Llm,
    /// One tool call. `name`, `mode` (instant | waiting), `ok`, `error`,
    /// `result_chars`, `step`.
    Tool,
    /// Waiting between turns. `on` (callback | approval | child | user ask),
    /// `detail`.
    Wait,
    /// A tool use asked of a person. `tool`, `decision`, `decided_by`,
    /// `person_ms` (how long it waited on a person).
    Approval,
    /// A check with screenshots (`crate::evidence` when it exists).
    /// `shots`, `changed_pct`, `pass`.
    Evidence,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Task => "task",
            Kind::Turn => "turn",
            Kind::Step => "step",
            Kind::Llm => "llm",
            Kind::Tool => "tool",
            Kind::Wait => "wait",
            Kind::Approval => "approval",
            Kind::Evidence => "evidence",
        }
    }
}

/// A span's facts.
pub type Attrs = Map<String, Value>;

/// Facts from text pairs (a shell without `serde_json` of its own).
pub fn attrs(pairs: &[(&str, &str)]) -> Attrs {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::from(*v))).collect()
}

/// One timed record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub kind: Kind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub engine: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attrs: Map<String, Value>,
}

impl Span {
    pub fn new(kind: Kind, id: impl Into<String>, engine: &str, start_ms: i64, end_ms: i64) -> Span {
        Span { kind, id: id.into(), parent: None, engine: engine.to_string(), start_ms, end_ms: end_ms.max(start_ms), attrs: Map::new() }
    }

    /// With `key` set (a `null` value is left out).
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Span {
        self.set(key, value);
        self
    }

    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        let v = value.into();
        if !v.is_null() {
            self.attrs.insert(key.to_string(), v);
        }
    }

    pub fn under(mut self, parent: Option<String>) -> Span {
        self.parent = parent;
        self
    }

    pub fn ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).and_then(Value::as_str)
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        self.attrs.get(key).and_then(Value::as_f64)
    }

    pub fn flag(&self, key: &str) -> bool {
        self.attrs.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    /// One line of the log.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// The spans of a log (a line that does not read is skipped: a log cut
/// mid-write loses that line only).
pub fn parse(text: &str) -> Vec<Span> {
    text.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect()
}

/// Spans as log lines, each ending in a newline.
pub fn to_text(spans: &[Span]) -> String {
    spans.iter().map(|s| format!("{}\n", s.to_line())).collect()
}

// ---- the tree -------------------------------------------------------------------

/// A span and the spans under it, by start.
#[derive(Clone, Debug, PartialEq)]
pub struct Node<'a> {
    pub span: &'a Span,
    pub children: Vec<Node<'a>>,
}

impl Node<'_> {
    /// The nodes in depth-first order with their depth.
    pub fn flat(&self) -> Vec<(usize, &Span)> {
        let mut out = Vec::new();
        self.walk(0, &mut out);
        out
    }

    fn walk<'b>(&'b self, depth: usize, out: &mut Vec<(usize, &'b Span)>) {
        out.push((depth, self.span));
        for c in &self.children {
            c.walk(depth + 1, out);
        }
    }
}

/// The spans as trees: a span whose parent is not among them is a root.
/// Roots and children are ordered by start (then kind, so a task comes
/// before the turn that started with it).
pub fn tree(spans: &[Span]) -> Vec<Node<'_>> {
    let ids: BTreeMap<&str, usize> = spans.iter().enumerate().map(|(n, s)| (s.id.as_str(), n)).collect();
    let mut kids: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut roots = Vec::new();
    for (n, s) in spans.iter().enumerate() {
        match s.parent.as_deref().and_then(|p| ids.get(p)).filter(|p| **p != n) {
            Some(&p) => kids.entry(p).or_default().push(n),
            None => roots.push(n),
        }
    }
    let order = |v: &mut Vec<usize>| v.sort_by_key(|&n| (spans[n].start_ms, spans[n].kind, n));
    fn build<'a>(n: usize, spans: &'a [Span], kids: &BTreeMap<usize, Vec<usize>>, depth: usize) -> Node<'a> {
        let children = if depth > 64 { Vec::new() } else { kids.get(&n).map_or(Vec::new(), |k| k.iter().map(|&c| build(c, spans, kids, depth + 1)).collect()) };
        Node { span: &spans[n], children }
    }
    for v in kids.values_mut() {
        order(v);
    }
    order(&mut roots);
    roots.into_iter().map(|n| build(n, spans, &kids, 0)).collect()
}

// ---- what started a turn --------------------------------------------------------

/// What started a turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// The person's message.
    #[default]
    User,
    /// Work it waited on came back: children replied, a terminal session
    /// ended.
    Callback,
    /// Another engine gave it a message (its parent).
    Post,
    /// An alert's time came.
    Schedule,
    /// The person answered its ask.
    Answer,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::User => "user",
            Trigger::Callback => "callback",
            Trigger::Post => "post",
            Trigger::Schedule => "schedule",
            Trigger::Answer => "answer",
        }
    }

    pub fn parse(word: &str) -> Option<Trigger> {
        [Trigger::User, Trigger::Callback, Trigger::Post, Trigger::Schedule, Trigger::Answer].into_iter().find(|t| t.as_str() == word)
    }

    /// Does a turn so started go on with the open task (rather than start one)?
    pub fn continues(self) -> bool {
        matches!(self, Trigger::Callback | Trigger::Answer)
    }
}

/// What started engine `e`'s new turn, read from its newest message: a
/// callback the app wrote, an answer to its last reply's ask, a message
/// from its parent (a child's messages come from the engine that made
/// it), else the person.
pub fn trigger_of(e: &crate::state::Engine) -> Trigger {
    use crate::inference::Role;
    let Some(last) = e.messages.iter().rev().find(|m| m.role == Role::User) else { return Trigger::User };
    if crate::learning::is_callback(&last.text) {
        return Trigger::Callback;
    }
    let before = e.messages.iter().rev().skip_while(|m| m.role == Role::User).find(|m| m.role == Role::Assistant);
    if before.is_some_and(|m| crate::ask::ask_in(e, &m.text).is_some()) && e.messages.last().is_some_and(|m| m.role == Role::User) {
        return Trigger::Answer;
    }
    if e.parent.is_some() {
        return Trigger::Post;
    }
    Trigger::User
}

/// A message's first line, short: a task's goal.
pub fn goal_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() > 80 { format!("{}…", line.chars().take(80).collect::<String>()) } else { line.to_string() }
}

// ---- usage from a model's body ---------------------------------------------------

/// What a model's body said it used.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub prompt: Option<u64>,
    pub completion: Option<u64>,
    pub cost_usd: Option<f64>,
}

/// The usage in a Chat Completions or Responses body (and a CLI model's
/// cost, `crate::cli::completion_body`).
pub fn usage(body: &str) -> Usage {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return Usage::default() };
    let Some(u) = v.get("usage").filter(|u| u.is_object()) else { return Usage::default() };
    let n = |keys: &[&str]| keys.iter().find_map(|k| u.get(*k).and_then(Value::as_u64));
    Usage {
        prompt: n(&["prompt_tokens", "input_tokens"]),
        completion: n(&["completion_tokens", "output_tokens"]),
        cost_usd: ["cost_usd", "cost"].iter().find_map(|k| u.get(*k).and_then(Value::as_f64)),
    }
}

/// Who serves a model: a CLI model's provider (`claude`), else the base
/// URL's host (`localhost:1234`).
pub fn provider(base_url: &str) -> String {
    if let Some((p, _)) = crate::cli::parse_base(base_url) {
        return p;
    }
    let rest = base_url.split("://").nth(1).unwrap_or(base_url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

// ---- the runner's recorder -------------------------------------------------------

/// A model call out now.
#[derive(Clone, Debug, PartialEq)]
struct Out {
    start: i64,
    step: usize,
    compress: bool,
    compressed: bool,
    prompt_est: Option<usize>,
    first_token: Option<i64>,
    first_component: Option<i64>,
}

/// One turn's spans as they close. The runner calls it with the time of
/// each move; a shell with its own loop (the window) can call it the same
/// way. Ids are `<engine>:<kind>:<start>:<n>`, so they never repeat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace {
    pub engine: String,
    pub trigger: Trigger,
    /// The message that started it, one line.
    pub goal: String,
    seq: u32,
    turn: Option<(String, i64)>,
    step: Option<(String, i64, usize)>,
    out: Option<Out>,
    /// The step's waiting calls out now: when they went, their names.
    run: Option<(i64, Vec<String>)>,
    /// When each waiting call started and ended, if the shell timed them.
    wait_times: Vec<(i64, i64)>,
    answered: i64,
    steps: usize,
    closed: Vec<Span>,
}

impl Trace {
    pub fn new(engine: &str, trigger: Trigger, goal: &str) -> Trace {
        Trace { engine: engine.to_string(), trigger, goal: goal_line(goal), ..Trace::default() }
    }

    fn id(&mut self, kind: Kind, start: i64) -> String {
        self.seq += 1;
        format!("{}:{}:{start}:{}", self.engine, kind.as_str(), self.seq)
    }

    /// The turn's id (it opens at the first move).
    pub fn turn_id(&self) -> Option<&str> {
        self.turn.as_ref().map(|(id, _)| id.as_str())
    }

    /// A move at `now`: the turn opens at the first one.
    pub fn moved(&mut self, now: i64) {
        if self.turn.is_none() {
            let id = self.id(Kind::Turn, now);
            self.turn = Some((id, now));
        }
    }

    fn open_step(&mut self, now: i64, step: usize) {
        if self.step.as_ref().is_some_and(|(_, _, n)| *n == step) {
            return;
        }
        let id = self.id(Kind::Step, now);
        self.step = Some((id, now, step));
    }

    fn step_parent(&self) -> Option<String> {
        self.step.as_ref().map(|(id, _, _)| id.clone()).or_else(|| self.turn_id().map(str::to_string))
    }

    /// A model call went out at `now` for step `step`: its answer
    /// (`compress` false) or a summary of the history. `compressed`: the
    /// history was compressed for this step. `prompt_est`: the prompt's
    /// size as measured, for a body that says no usage.
    pub fn post(&mut self, now: i64, step: usize, compress: bool, compressed: bool, prompt_est: Option<usize>) {
        self.moved(now);
        self.open_step(now, step);
        self.out = Some(Out { start: now, step, compress, compressed, prompt_est, first_token: None, first_component: None });
    }

    /// The first text of the answer streaming in.
    pub fn first_token(&mut self, now: i64) {
        if let Some(o) = self.out.as_mut() {
            o.first_token.get_or_insert(now);
        }
    }

    /// The reply's component known while it streams (`stream::View::has_component`).
    pub fn first_component(&mut self, now: i64) {
        if let Some(o) = self.out.as_mut() {
            o.first_component.get_or_insert(now);
        }
    }

    /// The model call came back at `now` with `body` (or why not).
    /// `error`: why the body did not read as an answer, if it did not.
    pub fn body(&mut self, now: i64, body: &Result<String, String>, ai: &crate::inference::OpenAi, error: Option<String>) {
        let Some(o) = self.out.take() else { return };
        let id = self.id(Kind::Llm, o.start);
        let mut s = Span::new(Kind::Llm, id, &self.engine, o.start, now).under(self.step_parent());
        s.set("model", ai.model.clone());
        s.set("provider", provider(&ai.base_url));
        s.set("purpose", if o.compress { "compress" } else { "step" });
        s.set("step", o.step);
        if o.compressed {
            s.set("compressed", true);
        }
        let u = body.as_ref().map(|b| usage(b)).unwrap_or_default();
        let text_chars = body.as_ref().ok().and_then(|b| ai_text_chars(b));
        match (u.prompt, o.prompt_est) {
            (Some(p), _) => s.set("prompt_tokens", p),
            (None, Some(p)) => {
                s.set("prompt_tokens", p);
                s.set("tokens_est", true);
            }
            _ => {}
        }
        match (u.completion, text_chars) {
            (Some(c), _) => s.set("completion_tokens", c),
            (None, Some(chars)) => {
                s.set("completion_tokens", chars.div_ceil(4));
                s.set("tokens_est", true);
            }
            _ => {}
        }
        if let Some(c) = u.cost_usd {
            s.set("cost_usd", c);
        }
        if let Some(t) = o.first_token {
            s.set("ttft_ms", t - o.start);
        }
        if let Some(t) = o.first_component {
            s.set("first_component_ms", t - o.start);
        }
        if let Some(why) = body.as_ref().err().cloned().or(error) {
            s.set("error", short(&why, 160));
        }
        self.closed.push(s);
    }

    /// The model answered at `now`: its instant calls ran then.
    pub fn answered(&mut self, now: i64) {
        self.answered = now;
    }

    /// The step's waiting calls went out at `now`.
    pub fn run(&mut self, now: i64, names: Vec<String>) {
        self.run = Some((now, names));
    }

    /// When each waiting call started and ended, in the order they were
    /// handed out (a shell that times them; else they span the whole run).
    pub fn wait_times(&mut self, times: Vec<(i64, i64)>) {
        self.wait_times = times;
    }

    /// The step's results are in at `now`: `results` is the tools message
    /// it saved (`call → result` lines), one tool span per call; the
    /// waiting ones (by name, in order) over the time they ran, the
    /// instant ones at the moment the answer was read. Then the step closes.
    pub fn results(&mut self, now: i64, step: usize, results: &str, phase: &str) {
        let parent = self.step_parent();
        let (run_start, mut waiting) = match self.run.take() {
            Some((t, names)) => (t, names),
            None => (now, Vec::new()),
        };
        let mut times = std::mem::take(&mut self.wait_times).into_iter();
        let lines = crate::learning::results_of(results);
        let calls = lines.len();
        for (call, result) in lines {
            let name = call.split('(').next().unwrap_or("").trim().to_string();
            let waited = waiting.iter().position(|w| *w == name).map(|k| waiting.remove(k)).is_some();
            let at = if self.answered > 0 { self.answered.min(now) } else { now };
            let (start, end) = if waited { times.next().unwrap_or((run_start, now)) } else { (at, at) };
            let id = self.id(Kind::Tool, start);
            let mut s = Span::new(Kind::Tool, id, &self.engine, start, end).under(parent.clone());
            s.set("name", name);
            s.set("mode", if waited { "waiting" } else { "instant" });
            s.set("step", step);
            let error = result.strip_prefix("error: ");
            s.set("ok", error.is_none());
            if let Some(why) = error {
                s.set("error", short(why, 120));
            }
            s.set("result_chars", result.chars().count());
            self.closed.push(s);
        }
        self.step_end(now, step, phase, calls);
    }

    /// Step `step` is over at `now`.
    pub fn step_end(&mut self, now: i64, step: usize, phase: &str, calls: usize) {
        let Some((id, start, _)) = self.step.take() else { return };
        let mut s = Span::new(Kind::Step, id, &self.engine, start, now).under(self.turn_id().map(str::to_string));
        s.set("step", step);
        if !phase.is_empty() {
            s.set("phase", phase);
        }
        s.set("calls", calls);
        self.steps += 1;
        self.closed.push(s);
    }

    /// The turn is over at `now`: `outcome` (ready, failed, missing,
    /// waiting, asked), and what else is known of how it ended.
    pub fn end(&mut self, now: i64, outcome: &str, more: Map<String, Value>) {
        self.moved(now);
        if self.out.is_some() {
            // A call out when the turn ended (it failed): close it as failed.
            let ai = crate::inference::OpenAi::default();
            let model = more.get("model").and_then(Value::as_str).unwrap_or("").to_string();
            self.body(now, &Err("the turn ended".into()), &crate::inference::OpenAi { model, ..ai }, None);
        }
        if let Some((_, _, n)) = self.step.clone() {
            self.step_end(now, n, "", 0);
        }
        let Some((id, start)) = self.turn.take() else { return };
        let mut s = Span::new(Kind::Turn, id, &self.engine, start, now);
        s.set("trigger", self.trigger.as_str());
        s.set("outcome", outcome);
        s.set("steps", self.steps);
        if !self.goal.is_empty() {
            s.set("goal", self.goal.clone());
        }
        for (k, v) in more {
            s.set(&k, v);
        }
        self.closed.push(s);
    }

    /// The spans closed since the last take.
    pub fn take(&mut self) -> Vec<Span> {
        std::mem::take(&mut self.closed)
    }
}

fn ai_text_chars(body: &str) -> Option<usize> {
    let v: Value = serde_json::from_str(body).ok()?;
    let text = v.pointer("/choices/0/message/content").and_then(Value::as_str).or_else(|| v.get("output_text").and_then(Value::as_str))?;
    Some(text.chars().count())
}

/// At most `max` characters, on one line.
pub fn short(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max).collect::<String>()) }
}

/// How a turn's reply ended, for its span: whether it asks, the ask kind
/// written, whether it fell back with a note (`ask::Parsed::note`).
pub fn reply_attrs(e: &crate::state::Engine) -> Map<String, Value> {
    let mut m = Map::new();
    let Some(last) = e.messages.last().filter(|m| m.role == crate::inference::Role::Assistant) else { return m };
    let Some(call) = crate::ask::reply_call(e, &last.text) else { return m };
    let parsed = crate::ask::Parsed::of(&call);
    if let Some(kind) = call.arg("ask") {
        m.insert("ask".into(), json!(kind.split_whitespace().next().unwrap_or(kind)));
        m.insert("ask_note".into(), json!(parsed.note().is_some()));
        m.insert("asked".into(), json!(parsed.ask.is_some()));
    }
    if let Some(kind) = call.arg("show") {
        m.insert("show".into(), json!(kind));
    }
    m
}

// ---- tasks: across turns ---------------------------------------------------------

/// A task still open, per engine.
#[derive(Clone, Debug, PartialEq)]
struct OpenTask {
    id: String,
    start: i64,
    goal: String,
    by: Trigger,
    turns: usize,
    parent_engine: Option<String>,
    /// Waiting between turns since then, on what.
    wait: Option<(i64, String, String)>,
}

/// The tasks open in a shell: a turn's spans go through `adopt`, which
/// places each turn under its task (opening one, or going on with the
/// one a wait left open) and adds the task and wait spans as they close.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tasks {
    open: BTreeMap<String, OpenTask>,
    /// Each engine's parent, for its tasks (`set_parent`).
    parents: BTreeMap<String, String>,
}

impl Tasks {
    /// Engine `engine` was made by `parent`: its tasks say so.
    pub fn set_parent(&mut self, engine: &str, parent: Option<&str>) {
        match parent {
            Some(p) => self.parents.insert(engine.to_string(), p.to_string()),
            None => self.parents.remove(engine),
        };
    }

    /// The open task of `engine`, if any: (id, start, goal).
    pub fn open(&self, engine: &str) -> Option<(&str, i64, &str)> {
        self.open.get(engine).map(|t| (t.id.as_str(), t.start, t.goal.as_str()))
    }

    /// Place `spans` (some engine's, just closed) under their tasks; the
    /// task and wait spans that closed with them are added.
    pub fn adopt(&mut self, spans: &mut Vec<Span>) {
        let mut added = Vec::new();
        for s in spans.iter_mut().filter(|s| s.kind == Kind::Turn && s.parent.is_none()) {
            let trigger = s.str("trigger").and_then(Trigger::parse).unwrap_or_default();
            let engine = s.engine.clone();
            // A new piece of work ends the one still open (it was left).
            if !trigger.continues()
                && let Some(t) = self.open.remove(&engine)
            {
                added.extend(close(t, &engine, s.start_ms, "left"));
            }
            let parent_engine = self.parents.get(&engine).cloned();
            let t = self.open.entry(engine.clone()).or_insert_with(|| OpenTask {
                id: format!("{engine}:task:{}", s.start_ms),
                start: s.start_ms,
                goal: s.str("goal").unwrap_or("").to_string(),
                by: trigger,
                turns: 0,
                parent_engine,
                wait: None,
            });
            if let Some((start, on, detail)) = t.wait.take() {
                let mut w = Span::new(Kind::Wait, format!("{engine}:wait:{start}"), &engine, start, s.start_ms).under(Some(t.id.clone()));
                w.set("on", on);
                if !detail.is_empty() {
                    w.set("detail", detail);
                }
                added.push(w);
            }
            t.turns += 1;
            s.parent = Some(t.id.clone());
            let outcome = s.str("outcome").unwrap_or("").to_string();
            let wait = match outcome.as_str() {
                "waiting" => Some(("child".to_string(), s.str("waiting_on").unwrap_or("").to_string())),
                _ if s.flag("asked") => Some(("user ask".to_string(), s.str("ask").unwrap_or("").to_string())),
                _ => None,
            };
            match wait {
                Some((on, detail)) => t.wait = Some((s.end_ms, on, detail)),
                None => {
                    let t = self.open.remove(&engine).expect("just opened");
                    added.extend(close(t, &engine, s.end_ms, &outcome));
                }
            }
        }
        // An approval asked during a task sits under it.
        for s in spans.iter_mut().filter(|s| s.kind == Kind::Approval && s.parent.is_none()) {
            s.parent = self.open.get(&s.engine).map(|t| t.id.clone());
        }
        spans.extend(added);
    }
}

fn close(t: OpenTask, engine: &str, end: i64, outcome: &str) -> Option<Span> {
    let mut s = Span::new(Kind::Task, t.id, engine, t.start, end);
    s.set("goal", t.goal);
    s.set("by", t.by.as_str());
    s.set("outcome", outcome);
    s.set("turns", t.turns);
    if let Some(p) = t.parent_engine {
        s.set("parent_engine", p);
    }
    Some(s)
}

// ---- other spans the shells make --------------------------------------------------

/// An approval, once decided: asked at `asked_ms`, decided at `now`.
pub fn approval(a: &crate::approval::Approval, now: i64) -> Option<Span> {
    let d = a.decided.as_ref()?;
    let start = a.asked * 1000;
    let mut s = Span::new(Kind::Approval, format!("{}:approval:{}", a.engine, a.id), &a.engine, start, now.max(start));
    s.set("tool", a.tool.clone());
    s.set("decision", d.decision.word());
    s.set("decided_by", d.by.clone());
    s.set("person_ms", if d.by == "a person" { now.max(start) - start } else { 0 });
    Some(s)
}

/// An evidence check (the hook for `crate::evidence`): its screenshots,
/// how much changed, whether it passed.
pub fn evidence(engine: &str, start: i64, end: i64, shots: usize, changed_pct: f64, pass: bool) -> Span {
    Span::new(Kind::Evidence, format!("{engine}:evidence:{start}"), engine, start, end)
        .with("shots", shots)
        .with("changed_pct", changed_pct)
        .with("pass", pass)
}

/// A session program's run (`crate::stream_json`) as spans: each tool
/// call from its use to its result, each sub-agent (`Task`/`Agent`) as a
/// task under the turn with its own calls under it, and the program's
/// usage and cost from its result line as one model span.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionTrace {
    trace: Trace,
    /// Open rows by tool-use id: (start, name, parent use id).
    rows: BTreeMap<String, (i64, String, Option<String>)>,
    agents: BTreeMap<String, (String, i64)>,
    start: i64,
    model: String,
    program: String,
}

impl SessionTrace {
    pub fn new(engine: &str, trigger: Trigger, goal: &str, program: &str, now: i64) -> SessionTrace {
        let mut trace = Trace::new(engine, trigger, goal);
        trace.moved(now);
        SessionTrace { trace, start: now, program: program.to_string(), ..SessionTrace::default() }
    }

    fn turn(&self) -> Option<String> {
        self.trace.turn_id().map(str::to_string)
    }

    /// One line of the program's output at `now`.
    pub fn line(&mut self, now: i64, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return };
        if v.get("type").and_then(Value::as_str) == Some("result") {
            let u = usage(&json!({ "usage": v.get("usage").cloned().unwrap_or(Value::Null) }).to_string());
            let id = self.trace.id(Kind::Llm, self.start);
            let mut s = Span::new(Kind::Llm, id, &self.trace.engine, self.start, now).under(self.turn());
            s.set("model", if self.model.is_empty() { self.program.clone() } else { self.model.clone() });
            s.set("provider", self.program.clone());
            s.set("purpose", "session");
            s.set("prompt_tokens", u.prompt);
            s.set("completion_tokens", u.completion);
            s.set("cost_usd", v.get("total_cost_usd").and_then(Value::as_f64).or(u.cost_usd));
            if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
                s.set("error", short(v.get("result").and_then(Value::as_str).unwrap_or("error"), 120));
            }
            self.trace.closed.push(s);
        }
        for ev in crate::stream_json::parse_line(line) {
            use crate::stream_json::Event;
            match ev {
                Event::Init(init) => self.model = init.model,
                Event::ToolUse { id, name, parent, .. } => {
                    if crate::stream_json::AGENT_TOOLS.contains(&name.as_str()) {
                        let tid = format!("{}:task:{now}:{id}", self.trace.engine);
                        self.agents.insert(id.clone(), (tid, now));
                    }
                    self.rows.insert(id, (now, name, parent));
                }
                Event::ToolResult { id, error, text } => self.close_row(now, &id, error, &text),
                Event::AgentDone { id, status, .. } => self.close_agent(now, &id, &status),
                _ => {}
            }
        }
    }

    fn close_row(&mut self, now: i64, id: &str, error: bool, text: &str) {
        let Some((start, name, parent)) = self.rows.remove(id) else { return };
        let under = parent.and_then(|p| self.agents.get(&p).map(|(t, _)| t.clone())).or_else(|| self.turn());
        let sid = self.trace.id(Kind::Tool, start);
        let mut s = Span::new(Kind::Tool, sid, &self.trace.engine, start, now).under(under);
        s.set("name", name);
        s.set("mode", "waiting");
        s.set("ok", !error);
        if error {
            s.set("error", short(text, 120));
        }
        s.set("result_chars", text.chars().count());
        self.trace.closed.push(s);
        self.close_agent(now, id, if error { "failed" } else { "completed" });
    }

    fn close_agent(&mut self, now: i64, id: &str, status: &str) {
        let Some((tid, start)) = self.agents.remove(id) else { return };
        let mut s = Span::new(Kind::Task, tid, &self.trace.engine, start, now).under(self.turn());
        s.set("by", "post");
        s.set("goal", "sub-agent");
        s.set("outcome", status.split(':').next().unwrap_or(status));
        s.set("agent", true);
        self.trace.closed.push(s);
    }

    /// The run ended at `now` with `outcome`.
    pub fn end(&mut self, now: i64, outcome: &str) -> Vec<Span> {
        for id in self.rows.keys().cloned().collect::<Vec<_>>() {
            self.close_row(now, &id, true, "the run ended first");
        }
        self.trace.end(now, outcome, Map::new());
        self.trace.take()
    }
}

// ---- retention ---------------------------------------------------------------------

/// One engine's day, summed: what a rolled-up day keeps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Daily {
    /// Local day since 1970-01-01.
    pub day: i64,
    pub engine: String,
    #[serde(default)]
    pub tasks: u64,
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub llm_calls: u64,
    #[serde(default)]
    pub llm_errors: u64,
    #[serde(default)]
    pub llm_ms: i64,
    #[serde(default)]
    pub tool_calls: u64,
    #[serde(default)]
    pub tool_errors: u64,
    #[serde(default)]
    pub tool_ms: i64,
    #[serde(default)]
    pub wait_ms: i64,
    #[serde(default)]
    pub person_ms: i64,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub cost_usd: f64,
}

impl Daily {
    fn add(&mut self, s: &Span) {
        match s.kind {
            Kind::Task if !s.flag("agent") => self.tasks += 1,
            Kind::Turn => self.turns += 1,
            Kind::Llm => {
                self.llm_calls += 1;
                self.llm_errors += u64::from(s.str("error").is_some());
                self.llm_ms += s.ms();
                self.prompt_tokens += s.num("prompt_tokens").unwrap_or(0.0) as u64;
                self.completion_tokens += s.num("completion_tokens").unwrap_or(0.0) as u64;
                self.cost_usd += s.num("cost_usd").unwrap_or(0.0);
            }
            Kind::Tool => {
                self.tool_calls += 1;
                self.tool_errors += u64::from(!s.flag("ok"));
                self.tool_ms += s.ms();
            }
            Kind::Wait => self.wait_ms += s.ms(),
            Kind::Approval => {
                self.wait_ms += s.ms();
                self.person_ms += s.num("person_ms").unwrap_or(0.0) as i64;
            }
            _ => {}
        }
    }

    fn merge(&mut self, o: &Daily) {
        self.tasks += o.tasks;
        self.turns += o.turns;
        self.llm_calls += o.llm_calls;
        self.llm_errors += o.llm_errors;
        self.llm_ms += o.llm_ms;
        self.tool_calls += o.tool_calls;
        self.tool_errors += o.tool_errors;
        self.tool_ms += o.tool_ms;
        self.wait_ms += o.wait_ms;
        self.person_ms += o.person_ms;
        self.prompt_tokens += o.prompt_tokens;
        self.completion_tokens += o.completion_tokens;
        self.cost_usd += o.cost_usd;
    }
}

/// The local day of a time in ms on a clock `offset` seconds east of UTC.
pub fn day_of(ms: i64, offset: i32) -> i64 {
    (ms.div_euclid(1000) + i64::from(offset)).div_euclid(86_400)
}

/// Daily totals as log lines.
pub fn daily_text(days: &[Daily]) -> String {
    days.iter().map(|d| format!("{}\n", serde_json::to_string(d).unwrap_or_default())).collect()
}

pub fn parse_daily(text: &str) -> Vec<Daily> {
    text.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect()
}

/// What the retention does to a log: the spans of the last `keep_days`
/// days (today included) stay; older days are summed into `daily`
/// (merged with the totals already there, one line per engine and day).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rolled {
    pub kept: Vec<Span>,
    pub daily: Vec<Daily>,
    /// How many spans were rolled up.
    pub rolled: usize,
}

pub fn rollup(spans: Vec<Span>, daily: Vec<Daily>, now_ms: i64, offset: i32, keep_days: u32) -> Rolled {
    let first_kept = day_of(now_ms, offset) - i64::from(keep_days.max(1)) + 1;
    let mut days: BTreeMap<(i64, String), Daily> = BTreeMap::new();
    for d in daily {
        days.entry((d.day, d.engine.clone())).or_insert_with(|| Daily { day: d.day, engine: d.engine.clone(), ..Daily::default() }).merge(&d);
    }
    let mut kept = Vec::new();
    let mut rolled = 0;
    for s in spans {
        let day = day_of(s.start_ms, offset);
        if day >= first_kept {
            kept.push(s);
        } else {
            rolled += 1;
            days.entry((day, s.engine.clone())).or_insert_with(|| Daily { day, engine: s.engine.clone(), ..Daily::default() }).add(&s);
        }
    }
    Rolled { kept, daily: days.into_values().collect(), rolled }
}

/// The retention in days: `DRY_SPANS_KEEP_DAYS` as the shell read it, else `KEEP_DAYS`.
pub fn keep_days(setting: Option<&str>) -> u32 {
    setting.and_then(|s| s.trim().parse().ok()).filter(|d| *d > 0).unwrap_or(KEEP_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(kind: Kind, id: &str, parent: Option<&str>, start: i64, end: i64) -> Span {
        Span::new(kind, id, "e", start, end).under(parent.map(str::to_string))
    }

    #[test]
    fn a_span_reads_back_from_its_line_and_a_broken_line_is_skipped() {
        let s = span(Kind::Llm, "a", Some("p"), 10, 30).with("model", "m").with("ok", true).with("none", Value::Null);
        let text = format!("{}\n{{broken\n{}\n", s.to_line(), span(Kind::Tool, "b", None, 1, 2).to_line());
        let back = parse(&text);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], s);
        assert!(!back[0].attrs.contains_key("none"));
        assert_eq!(back[0].ms(), 20);
        assert_eq!(to_text(&back), format!("{}\n{}\n", back[0].to_line(), back[1].to_line()));
        assert!(s.to_line().contains("\"kind\":\"llm\""));
    }

    #[test]
    fn spans_form_a_tree_by_parent_and_start() {
        let spans = vec![
            span(Kind::Tool, "t2", Some("s1"), 50, 60),
            span(Kind::Turn, "turn", Some("task"), 0, 100),
            span(Kind::Step, "s1", Some("turn"), 0, 70),
            span(Kind::Llm, "l1", Some("s1"), 0, 40),
            span(Kind::Task, "task", None, 0, 200),
            span(Kind::Tool, "orphan", Some("gone"), 5, 6),
        ];
        let roots = tree(&spans);
        assert_eq!(roots.iter().map(|n| n.span.id.as_str()).collect::<Vec<_>>(), ["task", "orphan"]);
        let flat: Vec<(usize, &str)> = roots[0].flat().into_iter().map(|(d, s)| (d, s.id.as_str())).collect();
        assert_eq!(flat, [(0, "task"), (1, "turn"), (2, "s1"), (3, "l1"), (3, "t2")]);
    }

    #[test]
    fn usage_reads_chat_responses_and_cli_bodies() {
        let chat = r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;
        assert_eq!(usage(chat), Usage { prompt: Some(12), completion: Some(3), cost_usd: None });
        let resp = r#"{"usage":{"input_tokens":5,"output_tokens":2}}"#;
        assert_eq!(usage(resp).completion, Some(2));
        let cli = r#"{"usage":{"prompt_tokens":3,"completion_tokens":1,"cost_usd":0.001}}"#;
        assert_eq!(usage(cli).cost_usd, Some(0.001));
        assert_eq!(usage("nope"), Usage::default());
        assert_eq!(provider("cli:claude"), "claude");
        assert_eq!(provider("http://localhost:1234/v1"), "localhost:1234");
    }

    fn turn(start: i64, end: i64, trigger: &str, outcome: &str) -> Span {
        Span::new(Kind::Turn, format!("e:turn:{start}"), "e", start, end).with("trigger", trigger).with("outcome", outcome).with("goal", "do it")
    }

    #[test]
    fn a_task_spans_its_turns_and_the_waits_between_them() {
        let mut tasks = Tasks::default();
        tasks.set_parent("e", Some("boss"));
        let mut a = vec![turn(0, 100, "user", "waiting").with("waiting_on", "helper")];
        tasks.adopt(&mut a);
        assert_eq!(a.len(), 1, "the task is still open");
        assert_eq!(a[0].parent.as_deref(), Some("e:task:0"));
        assert_eq!(tasks.open("e").map(|t| t.0), Some("e:task:0"));
        let mut b = vec![turn(400, 500, "callback", "ready").with("asked", true).with("ask", "confirm")];
        tasks.adopt(&mut b);
        let wait = b.iter().find(|s| s.kind == Kind::Wait).unwrap();
        assert_eq!((wait.start_ms, wait.end_ms, wait.str("on"), wait.str("detail")), (100, 400, Some("child"), Some("helper")));
        assert_eq!(b.len(), 2, "asked: the task waits on the person");
        let mut c = vec![turn(900, 950, "answer", "ready")];
        tasks.adopt(&mut c);
        let task = c.iter().find(|s| s.kind == Kind::Task).unwrap();
        assert_eq!((task.start_ms, task.end_ms, task.str("outcome"), task.num("turns")), (0, 950, Some("ready"), Some(3.0)));
        assert_eq!(task.str("parent_engine"), Some("boss"));
        assert_eq!(c.iter().find(|s| s.kind == Kind::Wait).and_then(|w| w.str("on")), Some("user ask"));
        assert!(tasks.open("e").is_none());
        // A new message while one waits: the old task was left.
        let mut d = vec![turn(1000, 1100, "user", "ready").with("asked", true)];
        tasks.adopt(&mut d);
        let mut e = vec![turn(2000, 2100, "user", "ready")];
        tasks.adopt(&mut e);
        let left = e.iter().find(|s| s.kind == Kind::Task && s.str("outcome") == Some("left")).unwrap();
        assert_eq!((left.start_ms, left.end_ms), (1000, 2000));
        assert!(e.iter().any(|s| s.kind == Kind::Task && s.start_ms == 2000));
    }

    #[test]
    fn days_older_than_the_retention_roll_into_daily_totals() {
        const DAY: i64 = 86_400_000;
        let now = 100 * DAY + 3_600_000;
        let old = |d: i64, kind: Kind| span(kind, &format!("{d}{kind:?}"), None, d * DAY + 10, d * DAY + 1010);
        let spans = vec![
            old(50, Kind::Llm).with("prompt_tokens", 10).with("completion_tokens", 4).with("cost_usd", 0.5),
            old(50, Kind::Tool).with("ok", false),
            old(50, Kind::Task),
            old(99, Kind::Tool).with("ok", true),
            old(100, Kind::Turn),
        ];
        let prior = vec![Daily { day: 50, engine: "e".into(), tasks: 2, cost_usd: 0.25, ..Daily::default() }];
        let r = rollup(spans, prior, now, 0, 2);
        assert_eq!(r.kept.len(), 2, "days 99 and 100 kept");
        assert_eq!(r.rolled, 3);
        assert_eq!(r.daily.len(), 1);
        let d = &r.daily[0];
        assert_eq!((d.day, d.tasks, d.llm_calls, d.tool_calls, d.tool_errors, d.llm_ms), (50, 3, 1, 1, 1, 1000));
        assert_eq!((d.prompt_tokens, d.completion_tokens, d.cost_usd), (10, 4, 0.75));
        assert_eq!(parse_daily(&daily_text(&r.daily)), r.daily);
        assert_eq!(keep_days(Some("7")), 7);
        assert_eq!(keep_days(Some("x")), KEEP_DAYS);
        assert_eq!(day_of(-1, 0), -1);
    }

    #[test]
    fn a_session_run_becomes_tool_agent_and_usage_spans() {
        let mut t = SessionTrace::new("s", Trigger::User, "fix it", "claude", 1000);
        t.line(1000, r#"{"type":"system","subtype":"init","model":"opus","session_id":"x","tools":[]}"#);
        t.line(1100, r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a1","name":"Task","input":{"description":"look"}}]}}"#);
        t.line(1200, r#"{"type":"assistant","parent_tool_use_id":"a1","message":{"content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"ls"}}]}}"#);
        t.line(1300, r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b1","content":"ok"}]}}"#);
        t.line(1400, r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a1","content":"done"}]}}"#);
        t.line(1500, r#"{"type":"result","is_error":false,"result":"hi","total_cost_usd":0.02,"usage":{"input_tokens":30,"output_tokens":5}}"#);
        let spans = t.end(1600, "ready");
        let by = |k: Kind| spans.iter().filter(|s| s.kind == k).collect::<Vec<_>>();
        let agent = by(Kind::Task)[0];
        let bash = by(Kind::Tool).into_iter().find(|s| s.str("name") == Some("Bash")).unwrap();
        assert_eq!(bash.parent.as_deref(), Some(agent.id.as_str()), "the agent's call is under it");
        assert_eq!((bash.start_ms, bash.end_ms), (1200, 1300));
        let llm = by(Kind::Llm)[0];
        assert_eq!((llm.num("cost_usd"), llm.num("prompt_tokens"), llm.str("model")), (Some(0.02), Some(30.0), Some("opus")));
        let turn = by(Kind::Turn)[0];
        assert_eq!((turn.start_ms, turn.end_ms, turn.str("outcome")), (1000, 1600, Some("ready")));
        assert_eq!(agent.parent.as_deref(), Some(turn.id.as_str()));
    }

    #[test]
    fn an_approval_says_how_long_it_waited_on_a_person() {
        use crate::approval::{Approval, Decided, Decision};
        let mut a = Approval { id: "a1".into(), engine: "e".into(), tool: "Bash".into(), input: json!({}), asked: 10, timeout: None, decided: None };
        assert!(approval(&a, 20_000).is_none());
        a.decided = Some(Decided { decision: Decision::Allow, reason: String::new(), by: "a person".into() });
        let s = approval(&a, 25_000).unwrap();
        assert_eq!((s.ms(), s.num("person_ms"), s.str("decided_by")), (15_000, Some(15_000.0), Some("a person")));
        let e = evidence("e", 0, 10, 3, 1.5, true);
        assert_eq!((e.kind, e.num("shots"), e.flag("pass")), (Kind::Evidence, Some(3.0), true));
    }
}
