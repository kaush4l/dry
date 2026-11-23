//! Analytics: queries over the spans (`crate::spans`) and the view
//! models every renderer draws them from (docs/analytics.md).
//!
//! | query | answers |
//! |---|---|
//! | `time` | work (model + tools) vs wait (between turns, approvals; the part on a person) vs task time |
//! | `models` | per model: calls, errors, tokens, tokens/s, time to first token, cost, cost per task |
//! | `tools` | per tool: calls, failures, p50/p95; the tools never used |
//! | `phases` | per phase: steps and time; test → triage loops |
//! | `context` | compressions, prompt growth per step, the largest prompt |
//! | `asks` | per ask kind: how often it fell back with a note |
//! | `you` | time to answer asks and approvals, active hours |
//! | `messages` | posts per thread, who → whom, time to handle |
//!
//! The views: an engine's `Timeline` (a waterfall of its last turns and
//! its totals), Home's `today`, and the `/analytics` page's sections, as
//! `crate::show` values (stat rows, tables, a progress bar) so every
//! renderer draws them with the display components it already has.
//!
//! `tool` is the curated `analytics(query)` call the learning artifact
//! brings (`crate::learning`): a closed set of queries, one line back,
//! the facts in the artifact's snapshot.
//!
//! Pure: spans in, rows out.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::show::{Show, Stat};
use crate::spans::{Daily, Kind, Span, Trigger, day_of};

/// Which analytics view a page shows (`pages::Component::Analytics`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Part {
    /// The route's engine: its last turns as a waterfall, and its totals.
    Timeline,
    /// Today across engines: work vs waiting, cost, the slowest tools.
    Today,
    /// Every query's table over a day range.
    #[default]
    Page,
}

impl Part {
    pub fn as_str(self) -> &'static str {
        match self {
            Part::Timeline => "timeline",
            Part::Today => "today",
            Part::Page => "page",
        }
    }

    pub fn parse(word: &str) -> Option<Part> {
        [Part::Timeline, Part::Today, Part::Page].into_iter().find(|p| p.as_str() == word)
    }
}

// ---- ranges ---------------------------------------------------------------------

/// A time range, unix ms, `from` inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub from_ms: i64,
    pub to_ms: i64,
}

impl Range {
    /// The last `days` local days, today included, on a clock `offset`
    /// seconds east of UTC.
    pub fn days(now_ms: i64, offset: i32, days: u32) -> Range {
        let today = day_of(now_ms, offset);
        let first = today - i64::from(days.max(1)) + 1;
        let start = |d: i64| (d * 86_400 - i64::from(offset)) * 1000;
        Range { from_ms: start(first), to_ms: start(today + 1) }
    }

    pub fn all() -> Range {
        Range { from_ms: i64::MIN, to_ms: i64::MAX }
    }

    pub fn has(&self, s: &Span) -> bool {
        s.start_ms >= self.from_ms && s.start_ms < self.to_ms
    }
}

/// The spans in `range`.
pub fn within(spans: &[Span], range: Range) -> Vec<Span> {
    spans.iter().filter(|s| range.has(s)).cloned().collect()
}

/// `p` (0..=100) of `values`, nearest rank.
pub fn percentile(values: &[i64], p: u32) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let rank = (f64::from(p.min(100)) / 100.0 * v.len() as f64).ceil() as usize;
    Some(v[rank.clamp(1, v.len()) - 1])
}

/// A duration for people: `420 ms`, `3.2 s`, `4m 05s`, `1h 20m`.
pub fn fmt_ms(ms: i64) -> String {
    let ms = ms.max(0);
    match ms {
        0..=999 => format!("{ms} ms"),
        1000..=59_999 => format!("{:.1} s", ms as f64 / 1000.0),
        60_000..=3_599_999 => format!("{}m {:02}s", ms / 60_000, (ms / 1000) % 60),
        _ => format!("{}h {:02}m", ms / 3_600_000, (ms / 60_000) % 60),
    }
}

fn fmt_opt(ms: Option<i64>) -> String {
    ms.map_or("–".into(), fmt_ms)
}

fn fmt_cost(c: Option<f64>) -> String {
    match c {
        Some(c) if c >= 1.0 => format!("${c:.2}"),
        Some(c) => format!("${c:.4}"),
        None => "–".into(),
    }
}

fn fmt_tokens(n: u64) -> String {
    if n >= 10_000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}

// ---- time ---------------------------------------------------------------------

/// Where the time went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeSplit {
    /// Model calls plus waiting tool calls.
    pub work_ms: i64,
    pub llm_ms: i64,
    pub tool_ms: i64,
    /// Between turns (children, callbacks, the person's answer) and approvals.
    pub wait_ms: i64,
    pub approval_ms: i64,
    /// Of the waits, the part on a person (asks and approvals).
    pub person_ms: i64,
    /// From each task's start to its end.
    pub task_ms: i64,
    pub tasks: usize,
    pub turns: usize,
}

pub fn time(spans: &[Span]) -> TimeSplit {
    let mut t = TimeSplit::default();
    for s in spans {
        match s.kind {
            Kind::Llm => t.llm_ms += s.ms(),
            Kind::Tool => t.tool_ms += s.ms(),
            Kind::Wait => {
                t.wait_ms += s.ms();
                if s.str("on") == Some("user ask") {
                    t.person_ms += s.ms();
                }
            }
            Kind::Approval => {
                t.wait_ms += s.ms();
                t.approval_ms += s.ms();
                t.person_ms += s.num("person_ms").unwrap_or(0.0) as i64;
            }
            Kind::Task if !s.flag("agent") => {
                t.task_ms += s.ms();
                t.tasks += 1;
            }
            Kind::Turn => t.turns += 1,
            _ => {}
        }
    }
    t.work_ms = t.llm_ms + t.tool_ms;
    t
}

// ---- ancestry ---------------------------------------------------------------------

/// Each span's task (walking up its parents), by span id.
fn tasks_of(spans: &[Span]) -> HashMap<&str, &str> {
    let parent: HashMap<&str, &str> = spans.iter().filter_map(|s| s.parent.as_deref().map(|p| (s.id.as_str(), p))).collect();
    let tasks: BTreeSet<&str> = spans.iter().filter(|s| s.kind == Kind::Task && !s.flag("agent")).map(|s| s.id.as_str()).collect();
    let mut out = HashMap::new();
    for s in spans {
        let mut at = s.id.as_str();
        for _ in 0..16 {
            if tasks.contains(at) {
                out.insert(s.id.as_str(), at);
                break;
            }
            match parent.get(at) {
                Some(p) => at = p,
                None => {
                    // Parented to a task id not in this window (it closes later).
                    if at.contains(":task:") {
                        out.insert(s.id.as_str(), at);
                    }
                    break;
                }
            }
        }
    }
    out
}

// ---- models -------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelRow {
    pub model: String,
    pub provider: String,
    pub calls: usize,
    pub errors: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Some token counts were estimated (the body said none).
    pub estimated: bool,
    pub tokens_per_s: Option<f64>,
    pub ttft_p50_ms: Option<i64>,
    pub cost_usd: Option<f64>,
    pub tasks: usize,
    pub cost_per_task: Option<f64>,
}

impl ModelRow {
    pub fn error_rate(&self) -> f64 {
        self.errors as f64 / self.calls.max(1) as f64
    }
}

pub fn models(spans: &[Span]) -> Vec<ModelRow> {
    let task_of = tasks_of(spans);
    let mut rows: BTreeMap<String, (ModelRow, i64, Vec<i64>, BTreeSet<String>)> = BTreeMap::new();
    for s in spans.iter().filter(|s| s.kind == Kind::Llm) {
        let model = s.str("model").unwrap_or("?").to_string();
        let (r, gen_ms, ttfts, tasks) = rows.entry(model.clone()).or_insert_with(|| {
            (ModelRow { model, provider: s.str("provider").unwrap_or("").to_string(), ..ModelRow::default() }, 0, Vec::new(), BTreeSet::new())
        });
        r.calls += 1;
        r.errors += usize::from(s.str("error").is_some());
        r.prompt_tokens += s.num("prompt_tokens").unwrap_or(0.0) as u64;
        let completion = s.num("completion_tokens").unwrap_or(0.0) as u64;
        r.completion_tokens += completion;
        r.estimated |= s.flag("tokens_est");
        if let Some(c) = s.num("cost_usd") {
            *r.cost_usd.get_or_insert(0.0) += c;
        }
        let ttft = s.num("ttft_ms").map(|t| t as i64);
        if let Some(t) = ttft {
            ttfts.push(t);
        }
        if completion > 0 && s.str("error").is_none() {
            *gen_ms += (s.ms() - ttft.unwrap_or(0)).max(1);
        }
        if let Some(t) = task_of.get(s.id.as_str()) {
            tasks.insert(t.to_string());
        }
    }
    rows.into_values()
        .map(|(mut r, gen_ms, ttfts, tasks)| {
            r.tokens_per_s = (gen_ms > 0 && r.completion_tokens > 0).then(|| r.completion_tokens as f64 / (gen_ms as f64 / 1000.0));
            r.ttft_p50_ms = percentile(&ttfts, 50);
            r.tasks = tasks.len();
            r.cost_per_task = r.cost_usd.filter(|_| r.tasks > 0).map(|c| c / r.tasks as f64);
            r
        })
        .collect()
}

// ---- tools --------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolRow {
    pub name: String,
    pub count: usize,
    pub failures: usize,
    /// Of the calls that waited (instant calls take no time).
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    pub waiting: usize,
}

impl ToolRow {
    pub fn failure_rate(&self) -> f64 {
        self.failures as f64 / self.count.max(1) as f64
    }
}

/// Per tool, the most used first.
pub fn tools(spans: &[Span]) -> Vec<ToolRow> {
    let mut rows: BTreeMap<String, (ToolRow, Vec<i64>)> = BTreeMap::new();
    for s in spans.iter().filter(|s| s.kind == Kind::Tool) {
        let name = s.str("name").unwrap_or("?").to_string();
        let (r, times) = rows.entry(name.clone()).or_insert_with(|| (ToolRow { name, ..ToolRow::default() }, Vec::new()));
        r.count += 1;
        r.failures += usize::from(!s.flag("ok"));
        if s.str("mode") == Some("waiting") {
            r.waiting += 1;
            times.push(s.ms());
        }
    }
    let mut out: Vec<ToolRow> = rows
        .into_values()
        .map(|(mut r, times)| {
            r.p50_ms = percentile(&times, 50);
            r.p95_ms = percentile(&times, 95);
            r
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    out
}

/// The tools in `toolbox` no span used.
pub fn never_used(spans: &[Span], toolbox: &[String]) -> Vec<String> {
    let used: BTreeSet<&str> = spans.iter().filter(|s| s.kind == Kind::Tool).filter_map(|s| s.str("name")).collect();
    let replied = spans.iter().any(|s| s.kind == Kind::Turn && s.str("outcome") == Some("ready"));
    toolbox.iter().filter(|t| !used.contains(t.as_str()) && !(t.as_str() == "reply" && replied)).cloned().collect()
}

/// The slowest tools by p95, at most `n`.
pub fn slowest(spans: &[Span], n: usize) -> Vec<ToolRow> {
    let mut rows: Vec<ToolRow> = tools(spans).into_iter().filter(|r| r.p95_ms.is_some()).collect();
    rows.sort_by(|a, b| b.p95_ms.cmp(&a.p95_ms).then(a.name.cmp(&b.name)));
    rows.truncate(n);
    rows
}

// ---- phases -------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhaseRow {
    pub phase: String,
    pub steps: usize,
    pub ms: i64,
}

pub fn phases(spans: &[Span]) -> Vec<PhaseRow> {
    let mut rows: BTreeMap<String, PhaseRow> = BTreeMap::new();
    for s in spans.iter().filter(|s| s.kind == Kind::Step) {
        let phase = s.str("phase").unwrap_or("(none)").to_string();
        let r = rows.entry(phase.clone()).or_insert_with(|| PhaseRow { phase, ..PhaseRow::default() });
        r.steps += 1;
        r.ms += s.ms();
    }
    rows.into_values().collect()
}

/// How many times a step in a phase named like `from` was followed (in
/// the same engine) by one in a phase named like `to`: test → triage loops.
pub fn loops(spans: &[Span], from: &str, to: &str) -> usize {
    let mut steps: Vec<&Span> = spans.iter().filter(|s| s.kind == Kind::Step && s.str("phase").is_some()).collect();
    steps.sort_by_key(|s| (s.engine.clone(), s.start_ms));
    steps.windows(2).filter(|w| w[0].engine == w[1].engine && w[0].str("phase").is_some_and(|p| p.contains(from)) && w[1].str("phase").is_some_and(|p| p.contains(to))).count()
}

// ---- context ------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextStats {
    pub compressions: usize,
    /// Mean change of the prompt from one step to the next in a turn.
    pub growth_per_step: Option<f64>,
    pub max_prompt: Option<u64>,
    pub steps: usize,
}

pub fn context(spans: &[Span]) -> ContextStats {
    let mut c = ContextStats::default();
    let mut by_turn: BTreeMap<String, Vec<(i64, u64)>> = BTreeMap::new();
    let turn_of: HashMap<&str, &str> = spans.iter().filter(|s| s.kind == Kind::Step).filter_map(|s| s.parent.as_deref().map(|p| (s.id.as_str(), p))).collect();
    for s in spans.iter().filter(|s| s.kind == Kind::Llm) {
        if s.str("purpose") == Some("compress") {
            c.compressions += 1;
            continue;
        }
        let Some(p) = s.num("prompt_tokens").map(|p| p as u64) else { continue };
        c.max_prompt = Some(c.max_prompt.map_or(p, |m| m.max(p)));
        c.steps += 1;
        let turn = s.parent.as_deref().and_then(|st| turn_of.get(st).copied()).unwrap_or("");
        by_turn.entry(turn.to_string()).or_default().push((s.start_ms, p));
    }
    let mut deltas = Vec::new();
    for v in by_turn.values_mut() {
        v.sort();
        deltas.extend(v.windows(2).map(|w| w[1].1 as f64 - w[0].1 as f64));
    }
    c.growth_per_step = (!deltas.is_empty()).then(|| deltas.iter().sum::<f64>() / deltas.len() as f64);
    c
}

// ---- asks ---------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AskRow {
    pub kind: String,
    pub count: usize,
    /// Fell back (to a plain reply, or a cut ask) with a note.
    pub noted: usize,
}

pub fn asks(spans: &[Span]) -> Vec<AskRow> {
    let mut rows: BTreeMap<String, AskRow> = BTreeMap::new();
    for s in spans.iter().filter(|s| s.kind == Kind::Turn) {
        let Some(kind) = s.str("ask") else { continue };
        let r = rows.entry(kind.to_string()).or_insert_with(|| AskRow { kind: kind.to_string(), ..AskRow::default() });
        r.count += 1;
        r.noted += usize::from(s.flag("ask_note"));
    }
    rows.into_values().collect()
}

// ---- you ----------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct You {
    pub asks_answered: usize,
    pub ask_p50_ms: Option<i64>,
    pub approvals: usize,
    pub approval_p50_ms: Option<i64>,
    /// Turns you started (messages and answers) by local hour.
    pub hours: [usize; 24],
}

impl You {
    pub fn active_hours(&self) -> usize {
        self.hours.iter().filter(|h| **h > 0).count()
    }
}

pub fn you(spans: &[Span], offset: i32) -> You {
    let mut y = You::default();
    let asks: Vec<i64> = spans.iter().filter(|s| s.kind == Kind::Wait && s.str("on") == Some("user ask")).map(Span::ms).collect();
    let approvals: Vec<i64> =
        spans.iter().filter(|s| s.kind == Kind::Approval && s.str("decided_by") == Some("a person")).map(|s| s.num("person_ms").unwrap_or(0.0) as i64).collect();
    y.asks_answered = asks.len();
    y.ask_p50_ms = percentile(&asks, 50);
    y.approvals = approvals.len();
    y.approval_p50_ms = percentile(&approvals, 50);
    for s in spans.iter().filter(|s| s.kind == Kind::Turn) {
        if matches!(s.str("trigger").and_then(Trigger::parse), Some(Trigger::User | Trigger::Answer)) {
            let hour = ((s.start_ms.div_euclid(1000) + i64::from(offset)).rem_euclid(86_400) / 3600) as usize;
            y.hours[hour] += 1;
        }
    }
    y
}

// ---- messages -----------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Messages {
    /// Posts per thread (its root's name; `earlier`: a store's file from
    /// before threads).
    pub threads: Vec<(String, usize)>,
    /// Who posted to whom, how many times.
    pub pairs: Vec<(String, String, usize)>,
    /// From a post to its reader's next turn.
    pub handle_p50_ms: Option<i64>,
    pub posts: usize,
}

/// The posts on every thread in `range` (`state.shared`),
/// with the readers' turns from `spans`. `name`: an engine id's name.
pub fn messages(shared: &BTreeMap<String, String>, spans: &[Span], range: Range, name: &dyn Fn(&str) -> String) -> Messages {
    let mut m = Messages::default();
    let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut handle = Vec::new();
    let mut turns: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for s in spans.iter().filter(|s| s.kind == Kind::Turn) {
        turns.entry(s.engine.as_str()).or_default().push(s.start_ms);
    }
    for v in turns.values_mut() {
        v.sort_unstable();
    }
    for (level, text) in shared {
        let thread = if level.is_empty() { "earlier".to_string() } else { name(level) };
        let mut n = 0;
        for e in crate::shared::parse(text) {
            let at = e.at * 1000;
            if at < range.from_ms || at >= range.to_ms {
                continue;
            }
            n += 1;
            // A post with no `to` went back to its sender: no reader.
            let to = e.to.as_deref().map_or_else(|| "no addressee".to_string(), name);
            *pairs.entry((name(&e.author), to)).or_default() += 1;
            if let Some(t) = e.to.as_deref().and_then(|r| turns.get(r)).and_then(|v| v.iter().find(|t| **t >= at)) {
                handle.push(t - at);
            }
        }
        if n > 0 {
            m.threads.push((thread, n));
            m.posts += n;
        }
    }
    m.pairs = pairs.into_iter().map(|((a, b), n)| (a, b, n)).collect();
    m.pairs.sort_by_key(|p| std::cmp::Reverse(p.2));
    m.handle_p50_ms = percentile(&handle, 50);
    m
}

// ---- views ----------------------------------------------------------------------

/// One bar of a waterfall.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub label: String,
    pub kind: Kind,
    pub depth: usize,
    /// Where it starts and how wide it is, as parts of the whole (0..=1).
    pub left: f32,
    pub width: f32,
    pub ms: i64,
    /// What it was, in a few words (tokens, the result, the error).
    pub note: String,
    pub bad: bool,
}

/// An engine's last turns as bars on one time axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Waterfall {
    pub start_ms: i64,
    pub ms: i64,
    pub bars: Vec<Bar>,
}

fn bar_text(s: &Span) -> (String, String) {
    let num = |k: &str| s.num(k).map(|n| n as u64);
    match s.kind {
        Kind::Task => (format!("task · {}", s.str("goal").unwrap_or("")), format!("by {} → {}", s.str("by").unwrap_or("?"), s.str("outcome").unwrap_or("?"))),
        Kind::Turn => (
            format!("turn · {}", s.str("trigger").unwrap_or("?")),
            format!("{} · {} steps{}", s.str("outcome").unwrap_or("?"), num("steps").unwrap_or(0), s.str("ask").map_or(String::new(), |a| format!(" · asks {a}"))),
        ),
        Kind::Step => (format!("step {}{}", num("step").unwrap_or(0), s.str("phase").map_or(String::new(), |p| format!(" · {p}"))), format!("{} calls", num("calls").unwrap_or(0))),
        Kind::Llm => {
            let what = if s.str("purpose") == Some("compress") { "compress" } else { "llm" };
            let mut note = format!("{}→{} tok", fmt_tokens(num("prompt_tokens").unwrap_or(0)), fmt_tokens(num("completion_tokens").unwrap_or(0)));
            if s.flag("tokens_est") {
                note.push_str(" (est)");
            }
            if let Some(t) = s.num("ttft_ms") {
                note.push_str(&format!(" · first {}", fmt_ms(t as i64)));
            }
            if let Some(c) = s.num("cost_usd") {
                note.push_str(&format!(" · {}", fmt_cost(Some(c))));
            }
            if let Some(e) = s.str("error") {
                note = format!("error: {e}");
            }
            (format!("{what} · {}", s.str("model").unwrap_or("?")), note)
        }
        Kind::Tool => (
            format!("{} {}", if s.str("mode") == Some("waiting") { "⧗" } else { "·" }, s.str("name").unwrap_or("?")),
            s.str("error").map_or_else(|| format!("ok · {} chars", num("result_chars").unwrap_or(0)), |e| format!("error: {e}")),
        ),
        Kind::Wait => (format!("wait · {}", s.str("on").unwrap_or("?")), s.str("detail").unwrap_or("").to_string()),
        Kind::Approval => (
            format!("approval · {}", s.str("tool").unwrap_or("?")),
            format!("{} by {}", s.str("decision").unwrap_or("?"), s.str("decided_by").unwrap_or("?")),
        ),
        Kind::Evidence => (
            "evidence".into(),
            format!("{} shots · {:.1}% changed · {}", num("shots").unwrap_or(0), s.num("changed_pct").unwrap_or(0.0), if s.flag("pass") { "pass" } else { "fail" }),
        ),
    }
}

fn bad(s: &Span) -> bool {
    match s.kind {
        Kind::Tool => !s.flag("ok"),
        Kind::Llm => s.str("error").is_some(),
        Kind::Turn | Kind::Task => matches!(s.str("outcome"), Some("failed" | "missing")),
        Kind::Evidence => !s.flag("pass"),
        Kind::Approval => s.str("decision") == Some("deny"),
        _ => false,
    }
}

/// Engine `engine`'s last `turns` turns as a waterfall: each turn with
/// its steps, model calls and tools under it, and the waits and
/// approvals between them. `None`: no turn recorded yet.
pub fn waterfall(spans: &[Span], engine: &str, turns: usize) -> Option<Waterfall> {
    let mine: Vec<&Span> = spans.iter().filter(|s| s.engine == engine).collect();
    let mut last: Vec<&Span> = mine.iter().copied().filter(|s| s.kind == Kind::Turn).collect();
    last.sort_by_key(|s| s.start_ms);
    let last: Vec<&Span> = last.into_iter().rev().take(turns.max(1)).collect();
    let from = last.iter().map(|s| s.start_ms).min()?;
    let to = last.iter().map(|s| s.end_ms).max()?;
    let picked: Vec<Span> = mine
        .iter()
        .filter(|s| s.kind != Kind::Task && s.start_ms >= from && s.start_ms <= to.max(from))
        .filter(|s| s.kind != Kind::Turn || last.iter().any(|t| t.id == s.id))
        .map(|s| (*s).clone())
        .collect();
    let to = picked.iter().map(|s| s.end_ms).max().unwrap_or(to).max(from + 1);
    let whole = (to - from) as f32;
    let roots = crate::spans::tree(&picked);
    let mut bars = Vec::new();
    for root in &roots {
        for (depth, s) in root.flat() {
            let (label, note) = bar_text(s);
            bars.push(Bar {
                label,
                kind: s.kind,
                depth,
                left: (s.start_ms - from) as f32 / whole,
                width: (s.ms() as f32 / whole).max(0.0),
                ms: s.ms(),
                note,
                bad: bad(s),
            });
        }
    }
    Some(Waterfall { start_ms: from, ms: to - from, bars })
}

/// A stat row's stat.
fn stat(label: &str, value: String, delta: String) -> Stat {
    Stat { label: label.into(), value, delta }
}

/// An engine's totals over `spans` (its own): one stat row.
pub fn totals(spans: &[Span]) -> Show {
    let t = time(spans);
    let llm: Vec<&Span> = spans.iter().filter(|s| s.kind == Kind::Llm).collect();
    let tools: Vec<&Span> = spans.iter().filter(|s| s.kind == Kind::Tool).collect();
    let failed = tools.iter().filter(|s| !s.flag("ok")).count();
    let tokens: u64 = llm.iter().map(|s| s.num("prompt_tokens").unwrap_or(0.0) as u64 + s.num("completion_tokens").unwrap_or(0.0) as u64).sum();
    let cost: Option<f64> = llm.iter().filter_map(|s| s.num("cost_usd")).fold(None, |a, c| Some(a.unwrap_or(0.0) + c));
    Show::Stat {
        caption: String::new(),
        stats: vec![
            stat("Work", fmt_ms(t.work_ms), format!("llm {} · tools {}", fmt_ms(t.llm_ms), fmt_ms(t.tool_ms))),
            stat("Waiting", fmt_ms(t.wait_ms), if t.person_ms > 0 { format!("on you {}", fmt_ms(t.person_ms)) } else { String::new() }),
            stat("Model calls", llm.len().to_string(), format!("{} tokens", fmt_tokens(tokens))),
            stat("Tool calls", tools.len().to_string(), if failed > 0 { format!("{failed} failed") } else { String::new() }),
            stat("Tasks", t.tasks.to_string(), format!("{} turns", t.turns)),
            stat("Cost", fmt_cost(cost), String::new()),
        ],
    }
}

/// Work against work + waiting, in seconds, as a bar.
fn share(t: &TimeSplit, label: &str) -> Show {
    let s = |ms: i64| (ms as f64 / 100.0).round() / 10.0;
    Show::Progress { label: label.into(), value: s(t.work_ms), max: s(t.work_ms + t.wait_ms).max(0.1) }
}

/// An engine's Timeline section: its last turns as a waterfall, and its
/// totals (all its spans kept).
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    pub waterfall: Option<Waterfall>,
    pub totals: Show,
    pub spans: usize,
}

pub fn timeline(spans: &[Span], engine: &str, turns: usize) -> Timeline {
    let mine: Vec<Span> = spans.iter().filter(|s| s.engine == engine).cloned().collect();
    Timeline { waterfall: waterfall(&mine, engine, turns), totals: totals(&mine), spans: mine.len() }
}

/// Home's line for today: work vs wait, cost, the slowest tools.
#[derive(Clone, Debug, PartialEq)]
pub struct Today {
    pub stats: Show,
    /// Work against waiting; none before anything ran today.
    pub share: Option<Show>,
    pub slowest: Option<Show>,
}

pub fn today(spans: &[Span], now_ms: i64, offset: i32) -> Today {
    let s = within(spans, Range::days(now_ms, offset, 1));
    let t = time(&s);
    let cost: Option<f64> = s.iter().filter(|x| x.kind == Kind::Llm).filter_map(|x| x.num("cost_usd")).fold(None, |a, c| Some(a.unwrap_or(0.0) + c));
    let slow = slowest(&s, 3);
    Today {
        stats: Show::Stat {
            caption: "Today".into(),
            stats: vec![
                stat("Work", fmt_ms(t.work_ms), format!("{} tasks · {} turns", t.tasks, t.turns)),
                stat("Waiting", fmt_ms(t.wait_ms), if t.person_ms > 0 { format!("on you {}", fmt_ms(t.person_ms)) } else { String::new() }),
                stat("Cost", fmt_cost(cost), String::new()),
            ],
        },
        share: (t.work_ms + t.wait_ms > 0).then(|| share(&t, "seconds today were work, of work + waiting")),
        slowest: (!slow.is_empty()).then(|| Show::Table {
            caption: "Slowest tools today".into(),
            head: vec!["Tool".into(), "p95".into(), "Calls".into()],
            rows: slow.iter().map(|r| vec![r.name.clone(), fmt_opt(r.p95_ms), r.count.to_string()]).collect(),
        }),
    }
}

/// One section of the analytics page.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub shows: Vec<Show>,
    /// A quiet line under it (what it counts, or why it is empty).
    pub note: String,
}

/// The `/analytics` page's day ranges.
pub const RANGES: [u32; 3] = [1, 7, 30];

/// What the page is drawn from.
pub struct PageInput<'a> {
    pub spans: &'a [Span],
    pub daily: &'a [Daily],
    pub shared: &'a BTreeMap<String, String>,
    /// Engine id → name, and its toolbox now.
    pub engines: Vec<(String, String, Vec<String>)>,
    pub now_ms: i64,
    pub offset: i32,
    pub days: u32,
}

fn pct(n: usize, of: usize) -> String {
    if of == 0 { "–".into() } else { format!("{:.0}%", 100.0 * n as f64 / of as f64) }
}

fn table(caption: &str, head: &[&str], rows: Vec<Vec<String>>) -> Show {
    Show::Table { caption: caption.into(), head: head.iter().map(|h| h.to_string()).collect(), rows: rows.into_iter().take(crate::show::MAX_ROWS).collect() }
}

/// The `/analytics` page over the last `days` days.
pub fn page(input: &PageInput) -> Vec<Section> {
    let range = Range::days(input.now_ms, input.offset, input.days);
    let s = within(input.spans, range);
    let name = |id: &str| input.engines.iter().find(|(i, _, _)| i == id).map_or(id.to_string(), |(_, n, _)| n.clone());
    let mut out = Vec::new();

    let t = time(&s);
    out.push(Section {
        title: "Time".into(),
        shows: vec![
            Show::Stat {
                caption: String::new(),
                stats: vec![
                    stat("Work", fmt_ms(t.work_ms), format!("llm {} · tools {}", fmt_ms(t.llm_ms), fmt_ms(t.tool_ms))),
                    stat("Waiting", fmt_ms(t.wait_ms), format!("approvals {}", fmt_ms(t.approval_ms))),
                    stat("On you", fmt_ms(t.person_ms), "asks and approvals".into()),
                    stat("Tasks", t.tasks.to_string(), format!("{} in all · {} turns", fmt_ms(t.task_ms), t.turns)),
                ],
            },
            share(&t, "seconds were work, of work + waiting"),
        ],
        note: "Work is model calls plus tool calls that waited (parallel calls each count); waiting is between turns and on approvals.".into(),
    });

    let m = models(&s);
    out.push(Section {
        title: "Models".into(),
        shows: vec![table(
            "",
            &["Model", "Calls", "Errors", "Tokens in → out", "Tokens/s", "First token p50", "Cost", "Per task"],
            m.iter()
                .map(|r| {
                    vec![
                        format!("{} ({})", r.model, r.provider),
                        r.calls.to_string(),
                        format!("{} ({})", r.errors, pct(r.errors, r.calls)),
                        format!("{} → {}{}", fmt_tokens(r.prompt_tokens), fmt_tokens(r.completion_tokens), if r.estimated { " est" } else { "" }),
                        r.tokens_per_s.map_or("–".into(), |v| format!("{v:.1}")),
                        fmt_opt(r.ttft_p50_ms),
                        fmt_cost(r.cost_usd),
                        fmt_cost(r.cost_per_task),
                    ]
                })
                .collect(),
        )],
        note: if m.is_empty() { "No model calls in this range.".into() } else { "Tokens marked est were counted from the text (the server sent no usage); first token needs streaming.".into() },
    });

    let tl = tools(&s);
    let toolbox: Vec<String> = input.engines.iter().flat_map(|(_, _, t)| t.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let never = never_used(&s, &toolbox);
    let mut shows = vec![table(
        "",
        &["Tool", "Calls", "Failed", "p50", "p95"],
        tl.iter().map(|r| vec![r.name.clone(), r.count.to_string(), format!("{} ({})", r.failures, pct(r.failures, r.count)), fmt_opt(r.p50_ms), fmt_opt(r.p95_ms)]).collect(),
    )];
    if !never.is_empty() {
        shows.push(Show::List { caption: "Never used".into(), items: never.into_iter().take(crate::show::MAX_ROWS).collect() });
    }
    out.push(Section { title: "Tools".into(), shows, note: "p50 and p95 are of the calls that waited; instant calls take no time.".into() });

    let ph = phases(&s);
    out.push(Section {
        title: "Phases".into(),
        shows: vec![
            table("", &["Phase", "Steps", "Time"], ph.iter().map(|r| vec![r.phase.clone(), r.steps.to_string(), fmt_ms(r.ms)]).collect()),
            Show::Stat { caption: String::new(), stats: vec![stat("test → triage loops", loops(&s, "test", "triage").to_string(), String::new())] },
        ],
        note: "Steps by the phase their engine was in; (none) for engines without phases.".into(),
    });

    let c = context(&s);
    out.push(Section {
        title: "Context".into(),
        shows: vec![Show::Stat {
            caption: String::new(),
            stats: vec![
                stat("Compressions", c.compressions.to_string(), String::new()),
                stat("Prompt growth", c.growth_per_step.map_or("–".into(), |g| format!("{g:+.0} tok/step")), format!("over {} steps", c.steps)),
                stat("Largest prompt", c.max_prompt.map_or("–".into(), fmt_tokens), String::new()),
            ],
        }],
        note: String::new(),
    });

    let a = asks(&s);
    out.push(Section {
        title: "Asks".into(),
        shows: vec![table("", &["Ask", "Replies", "Fell back with a note", "Rate"], a.iter().map(|r| vec![r.kind.clone(), r.count.to_string(), r.noted.to_string(), pct(r.noted, r.count)]).collect())],
        note: if a.is_empty() { "No reply asked anything in this range.".into() } else { String::new() },
    });

    let y = you(&s, input.offset);
    let hours: Vec<String> = y.hours.iter().enumerate().filter(|(_, n)| **n > 0).map(|(h, n)| format!("{h:02}:00 · {n}")).collect();
    let mut shows = vec![Show::Stat {
        caption: String::new(),
        stats: vec![
            stat("Answering asks", fmt_opt(y.ask_p50_ms), format!("p50 of {}", y.asks_answered)),
            stat("Approvals", fmt_opt(y.approval_p50_ms), format!("p50 of {}", y.approvals)),
            stat("Active hours", y.active_hours().to_string(), "hours you started turns in".into()),
        ],
    }];
    if !hours.is_empty() {
        shows.push(Show::List { caption: "Turns you started, by hour".into(), items: hours });
    }
    out.push(Section { title: "You".into(), shows, note: String::new() });

    let msg = messages(input.shared, &s, range, &name);
    let mut shows = vec![Show::Stat {
        caption: String::new(),
        stats: vec![stat("Posts", msg.posts.to_string(), String::new()), stat("Time to handle", fmt_opt(msg.handle_p50_ms), "p50, post → reader's turn".into())],
    }];
    if !msg.threads.is_empty() {
        shows.push(table("Per thread", &["Thread", "Posts"], msg.threads.iter().map(|(b, n)| vec![b.clone(), n.to_string()]).collect()));
        shows.push(table("Who → whom", &["From", "To", "Posts"], msg.pairs.iter().map(|(a, b, n)| vec![a.clone(), b.clone(), n.to_string()]).collect()));
    }
    out.push(Section { title: "Messages".into(), shows, note: String::new() });

    let first = day_of(range.from_ms, input.offset);
    let days: Vec<&Daily> = input.daily.iter().filter(|d| d.day >= first).collect();
    if !days.is_empty() {
        let mut by_day: BTreeMap<i64, Daily> = BTreeMap::new();
        for d in days {
            let e = by_day.entry(d.day).or_insert_with(|| Daily { day: d.day, ..Daily::default() });
            e.tasks += d.tasks;
            e.llm_calls += d.llm_calls;
            e.tool_calls += d.tool_calls;
            e.tool_errors += d.tool_errors;
            e.llm_ms += d.llm_ms;
            e.tool_ms += d.tool_ms;
            e.wait_ms += d.wait_ms;
            e.cost_usd += d.cost_usd;
        }
        out.push(Section {
            title: "Older days".into(),
            shows: vec![table(
                "",
                &["Day", "Tasks", "Model calls", "Tool calls", "Work", "Waiting", "Cost"],
                by_day
                    .values()
                    .map(|d| {
                        let (y, m, dd) = crate::live::civil_from_days(d.day);
                        vec![
                            format!("{y}-{m:02}-{dd:02}"),
                            d.tasks.to_string(),
                            d.llm_calls.to_string(),
                            format!("{} ({} failed)", d.tool_calls, d.tool_errors),
                            fmt_ms(d.llm_ms + d.tool_ms),
                            fmt_ms(d.wait_ms),
                            fmt_cost(Some(d.cost_usd)),
                        ]
                    })
                    .collect(),
            )],
            note: "Days past the retention are kept as daily totals only.".into(),
        });
    }
    out
}

// ---- the curated tool ---------------------------------------------------------------

/// The queries `analytics(query)` takes: one line back, the facts in the
/// learning artifact's snapshot. Over the last seven days.
pub const QUERIES: [(&str, &str); 6] = [
    ("tools", "per tool: calls, failures, p95; the tools you never used"),
    ("models", "per model: calls, errors, tokens/s, first token, cost"),
    ("time", "your work vs waiting, and the part waiting on the person"),
    ("phases", "steps and time per phase, test → triage loops"),
    ("context", "compressions and how fast your prompt grows per step"),
    ("asks", "per ask kind: how often it fell back with a note"),
];

/// The days `analytics` looks back.
pub const TOOL_DAYS: u32 = 7;

/// `analytics(query)` over `spans` (the engine's own) in the last
/// `TOOL_DAYS` days: the one line it answers, and the facts for the
/// snapshot (one per line).
pub fn tool(query: &str, spans: &[Span], toolbox: &[String], now_ms: i64, offset: i32) -> Result<(String, Vec<String>), String> {
    let q = query.trim();
    if !QUERIES.iter().any(|(n, _)| *n == q) {
        return Err(format!("analytics query=\"...\": one of {}", QUERIES.map(|(n, _)| n).join(", ")));
    }
    let s = within(spans, Range::days(now_ms, offset, TOOL_DAYS));
    let week = "this week";
    let (line, facts) = match q {
        "tools" => {
            let rows = tools(&s);
            let never = never_used(&s, toolbox);
            let facts: Vec<String> = rows
                .iter()
                .map(|r| format!("{}: {} calls, failed {}/{}, p95 {}", r.name, r.count, r.failures, r.count, fmt_opt(r.p95_ms)))
                .chain((!never.is_empty()).then(|| format!("never used: {}", never.join(", "))))
                .collect();
            let worst = rows.iter().filter(|r| r.failures > 0).max_by(|a, b| a.failure_rate().total_cmp(&b.failure_rate()));
            let line = match worst {
                Some(r) => format!("{week}: tool {} failed {}/{}; {} tools used", r.name, r.failures, r.count, rows.len()),
                None if rows.is_empty() => format!("{week}: no tool calls"),
                None => format!("{week}: {} tools used, none failed", rows.len()),
            };
            (line, facts)
        }
        "models" => {
            let rows = models(&s);
            let facts: Vec<String> = rows
                .iter()
                .map(|r| {
                    format!(
                        "{}: {} calls, {} errors, {} tok/s, first token {}, cost {}",
                        r.model,
                        r.calls,
                        r.errors,
                        r.tokens_per_s.map_or("–".into(), |v| format!("{v:.1}")),
                        fmt_opt(r.ttft_p50_ms),
                        fmt_cost(r.cost_usd)
                    )
                })
                .collect();
            let line = match rows.iter().max_by_key(|r| r.calls) {
                Some(r) => format!("{week}: {} model calls; {} most ({} calls, {} errors)", rows.iter().map(|r| r.calls).sum::<usize>(), r.model, r.calls, r.errors),
                None => format!("{week}: no model calls"),
            };
            (line, facts)
        }
        "time" => {
            let t = time(&s);
            let line = format!("{week}: work {}, waiting {} ({} on the person), {} tasks", fmt_ms(t.work_ms), fmt_ms(t.wait_ms), fmt_ms(t.person_ms), t.tasks);
            (line.clone(), vec![line, format!("model {} · tools {} · approvals {}", fmt_ms(t.llm_ms), fmt_ms(t.tool_ms), fmt_ms(t.approval_ms))])
        }
        "phases" => {
            let rows = phases(&s);
            let loops = loops(&s, "test", "triage");
            let facts: Vec<String> = rows.iter().map(|r| format!("{}: {} steps, {}", r.phase, r.steps, fmt_ms(r.ms))).collect();
            let line = match rows.iter().max_by_key(|r| r.ms) {
                Some(r) => format!("{week}: most time in phase {} ({} steps, {}); {loops} test → triage loops", r.phase, r.steps, fmt_ms(r.ms)),
                None => format!("{week}: no steps"),
            };
            (line, facts)
        }
        "context" => {
            let c = context(&s);
            let line = format!(
                "{week}: {} compressions; prompt grows {} per step; largest {}",
                c.compressions,
                c.growth_per_step.map_or("–".into(), |g| format!("{g:+.0} tokens")),
                c.max_prompt.map_or("–".into(), fmt_tokens)
            );
            (line.clone(), vec![line])
        }
        _ => {
            let rows = asks(&s);
            let facts: Vec<String> = rows.iter().map(|r| format!("{}: {} replies, {} fell back with a note", r.kind, r.count, r.noted)).collect();
            let noted: usize = rows.iter().map(|r| r.noted).sum();
            let total: usize = rows.iter().map(|r| r.count).sum();
            (format!("{week}: {noted}/{total} asks fell back with a note"), facts)
        }
    };
    Ok((line, facts))
}

/// The learning artifact's analytics block: the query asked and its facts.
pub fn block(query: &str, facts: &[String]) -> String {
    let mut out = format!("analytics: {query}\n");
    for f in facts.iter().take(crate::show::MAX_ROWS) {
        out.push_str(&format!("- {f}\n"));
    }
    out
}

/// The analytics block of a learning artifact's text, if any.
pub fn block_in(text: &str) -> Option<(&str, Vec<&str>)> {
    let at = text.find("\nanalytics: ").map(|n| n + 1).or_else(|| text.starts_with("analytics: ").then_some(0))?;
    let mut lines = text[at..].lines();
    let query = lines.next()?.strip_prefix("analytics: ")?.trim();
    Some((query, lines.filter_map(|l| l.strip_prefix("- ")).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spans::Span;

    const H: i64 = 3_600_000;
    const DAY: i64 = 24 * H;
    /// Noon on day 20000.
    const NOW: i64 = 20_000 * DAY + 12 * H;

    fn sp(kind: Kind, id: &str, parent: Option<&str>, start: i64, end: i64) -> Span {
        Span::new(kind, id, "e", NOW - H + start, NOW - H + end).under(parent.map(str::to_string))
    }

    /// A task: two turns (the first waits on a child), steps in two
    /// phases, model calls with usage and one estimated, tools (one
    /// failed), a compression, an approval and an ask that fell back.
    fn synthetic() -> Vec<Span> {
        vec![
            sp(Kind::Task, "task", None, 0, 10_000).with("goal", "fix").with("by", "user").with("outcome", "ready").with("turns", 2),
            sp(Kind::Turn, "t1", Some("task"), 0, 3000).with("trigger", "user").with("outcome", "waiting"),
            sp(Kind::Step, "s1", Some("t1"), 0, 1500).with("step", 1).with("phase", "test").with("calls", 2),
            sp(Kind::Llm, "l0", Some("s1"), 0, 200).with("model", "m").with("purpose", "compress").with("prompt_tokens", 900),
            sp(Kind::Llm, "l1", Some("s1"), 200, 1200).with("model", "m").with("provider", "p").with("prompt_tokens", 1000).with("completion_tokens", 100).with("ttft_ms", 200).with("cost_usd", 0.01),
            sp(Kind::Tool, "x1", Some("s1"), 1200, 1500).with("name", "run").with("mode", "waiting").with("ok", false).with("error", "exit 1"),
            sp(Kind::Tool, "x2", Some("s1"), 1200, 1200).with("name", "read").with("mode", "instant").with("ok", true),
            sp(Kind::Step, "s2", Some("t1"), 1500, 3000).with("step", 2).with("phase", "triage").with("calls", 1),
            sp(Kind::Llm, "l2", Some("s2"), 1500, 2500).with("model", "m").with("prompt_tokens", 1300).with("completion_tokens", 50).with("tokens_est", true).with("cost_usd", 0.01),
            sp(Kind::Tool, "x3", Some("s2"), 2500, 3000).with("name", "run").with("mode", "waiting").with("ok", true),
            sp(Kind::Wait, "w", Some("task"), 3000, 6000).with("on", "child"),
            sp(Kind::Approval, "ap", None, 3500, 5500).with("tool", "Bash").with("decided_by", "a person").with("person_ms", 2000).with("decision", "allow"),
            sp(Kind::Turn, "t2", Some("task"), 6000, 7000).with("trigger", "callback").with("outcome", "ready").with("ask", "choice").with("ask_note", true),
            sp(Kind::Step, "s3", Some("t2"), 6000, 7000).with("step", 3).with("phase", "test").with("calls", 0),
            sp(Kind::Llm, "l3", Some("s3"), 6000, 7000).with("model", "n").with("error", "timeout"),
            sp(Kind::Wait, "w2", Some("task"), 7000, 9000).with("on", "user ask"),
            sp(Kind::Turn, "t3", Some("task"), 9000, 10_000).with("trigger", "answer").with("outcome", "ready").with("ask", "confirm"),
        ]
    }

    #[test]
    fn work_wait_and_task_time() {
        let t = time(&synthetic());
        assert_eq!((t.llm_ms, t.tool_ms, t.work_ms), (3200, 800, 4000));
        assert_eq!((t.wait_ms, t.approval_ms, t.person_ms), (7000, 2000, 4000), "waits 3 s + 2 s, approval 2 s; person: ask 2 s + approval 2 s");
        assert_eq!((t.task_ms, t.tasks, t.turns), (10_000, 1, 3));
    }

    #[test]
    fn per_model_rates_cost_and_errors() {
        let rows = models(&synthetic());
        let m = rows.iter().find(|r| r.model == "m").unwrap();
        assert_eq!((m.calls, m.errors, m.prompt_tokens, m.completion_tokens, m.estimated), (3, 0, 3200, 150, true));
        // 100 tokens in 800 ms after the first token + 50 in 1000 ms.
        assert!((m.tokens_per_s.unwrap() - 150.0 / 1.8).abs() < 1e-6, "{:?}", m.tokens_per_s);
        assert_eq!(m.ttft_p50_ms, Some(200));
        assert_eq!((m.cost_usd, m.tasks, m.cost_per_task), (Some(0.02), 1, Some(0.02)));
        let n = rows.iter().find(|r| r.model == "n").unwrap();
        assert_eq!((n.calls, n.errors, n.error_rate()), (1, 1, 1.0));
    }

    #[test]
    fn per_tool_counts_percentiles_failures_and_never_used() {
        let rows = tools(&synthetic());
        assert_eq!(rows[0].name, "run");
        assert_eq!((rows[0].count, rows[0].failures, rows[0].p50_ms, rows[0].p95_ms), (2, 1, Some(300), Some(500)));
        assert_eq!((rows[1].name.as_str(), rows[1].p95_ms), ("read", None), "instant: no time");
        let toolbox: Vec<String> = ["read", "run", "write", "reply"].map(String::from).to_vec();
        assert_eq!(never_used(&synthetic(), &toolbox), ["write"], "reply: the turns replied");
        assert_eq!(slowest(&synthetic(), 3).iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["run"]);
        assert_eq!(percentile(&[5, 1, 3, 2, 4], 50), Some(3));
        assert_eq!(percentile(&[], 95), None);
    }

    #[test]
    fn phases_loops_context_asks_and_you() {
        let s = synthetic();
        let ph = phases(&s);
        assert_eq!(ph.iter().map(|r| (r.phase.as_str(), r.steps, r.ms)).collect::<Vec<_>>(), [("test", 2, 2500), ("triage", 1, 1500)]);
        assert_eq!(loops(&s, "test", "triage"), 1);
        let c = context(&s);
        assert_eq!((c.compressions, c.max_prompt, c.growth_per_step, c.steps), (1, Some(1300), Some(300.0), 2));
        let a = asks(&s);
        assert_eq!(a.iter().map(|r| (r.kind.as_str(), r.count, r.noted)).collect::<Vec<_>>(), [("choice", 1, 1), ("confirm", 1, 0)]);
        let y = you(&s, 0);
        assert_eq!((y.asks_answered, y.ask_p50_ms, y.approvals, y.approval_p50_ms), (1, Some(2000), 1, Some(2000)));
        assert_eq!((y.active_hours(), y.hours[11]), (1, 2), "the user and answer turns at 11:00");
    }

    #[test]
    fn posts_per_thread_who_to_whom_and_time_to_handle() {
        let at = (NOW - H) / 1000;
        let mut shared = BTreeMap::new();
        shared.insert(String::new(), format!("- {} boss → e: look at this\n- {} e: done\n", at + 1, at + 5));
        shared.insert("boss".to_string(), format!("- {} e → boss: for you\n", at + 1));
        let text = &shared[""];
        assert_eq!(crate::shared::parse(text).len(), 2, "the format: {text}");
        let name = |id: &str| if id == "boss" { "Boss".to_string() } else { id.to_string() };
        let m = messages(&shared, &synthetic(), Range::days(NOW, 0, 1), &name);
        assert_eq!(m.posts, 3);
        assert_eq!(m.threads, [("earlier".to_string(), 2), ("Boss".to_string(), 1)]);
        assert!(m.pairs.contains(&("e".into(), "no addressee".into(), 1)), "{:?}", m.pairs);
        assert!(m.pairs.contains(&("Boss".into(), "e".into(), 1)) && m.pairs.contains(&("e".into(), "Boss".into(), 1)), "{:?}", m.pairs);
        assert_eq!(m.handle_p50_ms, Some(5000), "the post at 1 s waits for e's turn at 6 s");
    }

    #[test]
    fn the_waterfall_lays_the_last_turns_on_one_axis() {
        let w = waterfall(&synthetic(), "e", 2).unwrap();
        assert_eq!((w.start_ms, w.ms), (NOW - H + 6000, 4000), "turns t2 and t3, the wait between");
        let labels: Vec<(usize, &str)> = w.bars.iter().map(|b| (b.depth, b.label.as_str())).collect();
        assert_eq!(labels[0], (0, "turn · callback"));
        assert!(labels.contains(&(2, "llm · n")) && labels.contains(&(0, "wait · user ask")), "{labels:?}");
        let llm = w.bars.iter().find(|b| b.label == "llm · n").unwrap();
        assert!(llm.bad && llm.note.starts_with("error"));
        assert!(w.bars.iter().all(|b| b.left >= 0.0 && b.left + b.width <= 1.0001));
        assert!(waterfall(&synthetic(), "other", 2).is_none());
        let tl = timeline(&synthetic(), "e", 1);
        let Show::Stat { stats, .. } = &tl.totals else { panic!() };
        assert_eq!((stats[0].value.as_str(), stats[5].value.as_str()), ("4.0 s", "$0.0200"));
    }

    #[test]
    fn today_and_the_page_read_the_range() {
        let mut s = synthetic();
        s.push(Span::new(Kind::Tool, "old", "e", NOW - 3 * DAY, NOW - 3 * DAY + 9000).with("name", "slow").with("mode", "waiting").with("ok", true));
        let t = today(&s, NOW, 0);
        let Some(Show::Table { rows, .. }) = &t.slowest else { panic!() };
        assert_eq!(rows[0][0], "run", "yesterday's slow tool is not today's");
        let daily = vec![Daily { day: 20_000 - 20, engine: "e".into(), tasks: 4, cost_usd: 1.5, ..Daily::default() }];
        let shared = BTreeMap::new();
        let input = |days| PageInput { spans: &s, daily: &daily, shared: &shared, engines: vec![("e".into(), "E".into(), vec!["write".into()])], now_ms: NOW, offset: 0, days };
        let week = page(&input(7));
        let titles: Vec<&str> = week.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, ["Time", "Models", "Tools", "Phases", "Context", "Asks", "You", "Messages"]);
        let Show::Table { rows, .. } = &week[2].shows[0] else { panic!() };
        assert!(rows.iter().any(|r| r[0] == "slow"), "three days ago is in the week");
        assert!(matches!(&week[2].shows[1], Show::List { items, .. } if items == &["write"]));
        let month = page(&input(30));
        assert_eq!(month.last().unwrap().title, "Older days");
    }

    #[test]
    fn the_tool_answers_one_line_from_a_closed_set() {
        let toolbox: Vec<String> = ["run", "read", "write"].map(String::from).to_vec();
        let (line, facts) = tool("tools", &synthetic(), &toolbox, NOW, 0).unwrap();
        assert_eq!(line, "this week: tool run failed 1/2; 2 tools used");
        assert!(facts.contains(&"never used: write".to_string()), "{facts:?}");
        for (q, _) in QUERIES {
            let (line, _) = tool(q, &synthetic(), &toolbox, NOW, 0).unwrap();
            assert!(!line.contains('\n') && line.starts_with("this week"), "{q}: {line}");
        }
        assert!(tool("everything", &[], &toolbox, NOW, 0).unwrap_err().contains("one of tools, models"));
        let b = block("tools", &facts);
        let text = format!("kind: learning\ncollect: fixes\n{b}");
        let (q, f) = block_in(&text).unwrap();
        assert_eq!((q, f.len()), ("tools", facts.len()));
        assert_eq!(fmt_ms(65_000), "1m 05s");
    }
}
