//! Ask components: how a reply asks the user something with a real
//! control instead of words.
//!
//! ```text
//! reply(ask="date", text="When should I run it?")
//!   ─▶ Ask::Date ─▶ a shell draws a calendar ─▶ the user picks
//!   ─▶ "Answer (date): 2026-11-24 (Tue)" as the user's next message
//! ```
//!
//! The set is closed: each `ask` has one fixed meaning, and every shell
//! (GPUI now, a terminal or phone later) draws the same `Ask` and sends
//! back the same text, made here by `Ask::answer`. The model never reads
//! UI state, only that line.
//!
//! Arguments are flat strings on `reply`, so a small model can write
//! them. Each has a default, and a broken one never fails the reply: it
//! falls back (to fewer arguments, or to a plain reply) and `Parsed::notes`
//! says what was wrong, which the loop gives the model as a result so it
//! can fix it next time.

use serde::{Deserialize, Serialize};

use crate::calls::{self, Call};
use crate::inference::Role;
use crate::live::{civil_from_days, days_from_civil};
use crate::response::{ResponseFormat, Responses};
use crate::state::Engine;

/// The most options a choice or multi shows, review rows, and form fields.
pub const MAX_ITEMS: usize = 12;

/// One registered ask component, as the model is told it: an example of
/// it, when to use it, and the question the example asks.
pub struct Entry {
    pub ask: Ask,
    pub when: &'static str,
    pub text: &'static str,
}

/// The registry: every ask component, in the order the model reads them.
/// The `reply` tool's instructions are made from it (`instructions`), and
/// a test reads every example back, so a new component is one entry here,
/// one variant of `Ask`, and its arm in `Parsed::of`.
pub fn registry() -> Vec<Entry> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let field = |label: &str, ask: Ask| Field { label: label.into(), ask };
    vec![
        Entry { ask: Ask::Choice { options: s(&["Red", "Green", "Blue"]) }, when: "pick one", text: "Which colour?" },
        Entry { ask: Ask::Multi { options: s(&["unit", "ui", "e2e"]) }, when: "pick any", text: "Which tests should I run?" },
        Entry { ask: Ask::Confirm, when: "yes or no to one short question, no details", text: "Delete the old logs?" },
        Entry { ask: Ask::Date, when: "a day", text: "Which day?" },
        Entry { ask: Ask::DateTime, when: "a day and a time", text: "When should I run it?" },
        Entry {
            ask: Ask::Number { min: Some(1.0), max: Some(10.0), unit: "copies".into() },
            when: "how many, how much",
            text: "How many copies?",
        },
        Entry { ask: Ask::Text { placeholder: "project name".into() }, when: "a short typed answer", text: "What is the project called?" },
        Entry {
            ask: Ask::Review { title: "Deploy".into(), rows: vec![("env".into(), "staging".into()), ("time".into(), "18:00".into())] },
            when: "show the details of a plan to check, edit or reject before you act",
            text: "Is this right?",
        },
        Entry {
            ask: Ask::Form {
                title: "Booking".into(),
                fields: vec![
                    field("Day", Ask::Date),
                    field("Guests", Ask::Number { min: Some(1.0), max: Some(12.0), unit: String::new() }),
                    field("Seat", Ask::Choice { options: s(&["inside", "terrace"]) }),
                    field("Notes", Ask::Text { placeholder: String::new() }),
                ],
            },
            when: "several questions at once",
            text: "A few details for the table:",
        },
    ]
}

/// Every `ask` value, in the registry's order.
pub fn kinds() -> Vec<&'static str> {
    registry().iter().map(|e| e.ask.name()).collect()
}

/// What `reply`'s description adds: one line per registered component,
/// `name (when): reply(...)`, what the answer looks like, then the
/// display components (`crate::show`).
pub fn instructions() -> String {
    let lines: Vec<String> = registry().iter().map(|e| format!("  {} ({}): {}", e.ask.name(), e.when, e.ask.call(e.text))).collect();
    format!(
        " To ask the user something, add ask= and they get a control instead of typing; their answer comes back as your next message, like `Answer (date): 2026-11-24 (Tue)`. One ask per reply. Use the one that fits:\n{}\nOptions and rows are separated by |. A form's fields are Label:kind, with options or a range in brackets.{}",
        lines.join("\n"),
        crate::show::instructions()
    )
}

/// The arguments `reply` reads (with `crate::show::ARGS`); any other is
/// named in a note.
const ARGS: &[&str] = &["text", "ask", "options", "choices", "title", "rows", "min", "max", "unit", "placeholder", "fields"];

/// What a reply asks for: the registered ask components. In a page file
/// it is `{"type": "ask", "ask": {"kind": "date"}}` (`crate::pages`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Ask {
    /// Pick one of `options` (2 or more).
    Choice { options: Vec<String> },
    /// Pick any of `options`, none included.
    Multi { options: Vec<String> },
    /// Yes or no.
    Confirm,
    /// A day.
    Date,
    /// A day and a time of day.
    DateTime,
    /// A number, within `min` and `max` when given; `unit` is shown after it.
    Number {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        unit: String,
    },
    /// A short typed answer; `placeholder` is the hint in the empty field.
    Text {
        #[serde(default)]
        placeholder: String,
    },
    /// A titled summary of `key: value` rows to confirm, edit or reject.
    /// Was `card`, which still reads.
    #[serde(alias = "card")]
    Review {
        #[serde(default)]
        title: String,
        rows: Vec<(String, String)>,
    },
    /// Several single asks in one box, each under its label, sent
    /// together with one Submit. A field is never a review or a form.
    Form {
        #[serde(default)]
        title: String,
        fields: Vec<Field>,
    },
}

/// One field of a form: its label and what it asks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub label: String,
    pub ask: Ask,
}

/// A reply's components as read: the ask (none for a plain reply), what
/// it shows (`crate::show`), and what was wrong with its arguments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub ask: Option<Ask>,
    pub show: Option<crate::show::Show>,
    pub notes: Vec<String>,
}

/// The user's answer to an ask, before it is written as text.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Choice(String),
    Multi(Vec<String>),
    Confirm(bool),
    /// Days since 1970-01-01.
    Date(i64),
    /// Days since 1970-01-01, and minutes after midnight.
    DateTime(i64, u32),
    Number(f64),
    Text(String),
    /// The rows as the user left them; `None` when rejected.
    Review(Option<Vec<(String, String)>>),
    /// One per field, in order; `None` for one left blank.
    Form(Vec<Option<Answer>>),
}

/// What a shell gathered from one control before it is an answer: the
/// options picked (by their text; `yes`/`no` for a confirm), the day and
/// time picked, and what was typed. `Ask::read_input` decides what it
/// means, so every shell reads a form the same way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub picked: Vec<String>,
    pub day: Option<i64>,
    pub minute: Option<u32>,
    pub typed: String,
}

/// The `|`-separated items of a list argument: trimmed, blanks and repeats
/// dropped.
pub fn split(list: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in list.split('|').map(str::trim).filter(|i| !i.is_empty()) {
        if !out.iter().any(|o| o == item) {
            out.push(item.to_string());
        }
    }
    out
}

/// The kind an `ask` value names (near misses read), for a reply
/// still being written (`crate::stream`).
pub fn kind_name(value: &str) -> Option<&'static str> {
    kind(value)
}

/// The kind an `ask` value names, forgiving the near misses a model writes.
/// `card` is its own answer, so the reader can say it is `review` now.
fn kind(value: &str) -> Option<&'static str> {
    let v = value.trim().to_ascii_lowercase().replace(['-', '_', ' ', '/'], "");
    Some(match v.as_str() {
        "choice" | "choices" | "one" | "single" | "select" | "pick" | "radio" | "option" | "options" | "dropdown" => "choice",
        "multi" | "multiple" | "many" | "checkbox" | "checkboxes" | "multichoice" | "multiselect" => "multi",
        "confirm" | "yesno" | "yn" | "bool" | "boolean" => "confirm",
        "date" | "day" => "date",
        "datetime" | "time" | "when" | "dateandtime" => "datetime",
        "number" | "num" | "int" | "integer" | "count" | "amount" | "slider" => "number",
        "text" | "input" | "string" | "name" | "short" => "text",
        "review" | "summary" | "check" | "details" => "review",
        "card" => "card",
        "form" | "fields" | "survey" | "questions" | "questionnaire" => "form",
        _ => return None,
    })
}

/// `key: value` rows from `items`; a row without `:` is kept with an
/// empty value and a note.
fn review_rows(items: Vec<String>, notes: &mut Vec<String>) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for item in cap(items, notes) {
        match item.split_once(':') {
            Some((k, v)) if !k.trim().is_empty() => rows.push((k.trim().to_string(), v.trim().to_string())),
            _ => {
                notes.push(format!("review row \"{item}\" is not `key: value`"));
                rows.push((item, String::new()));
            }
        }
    }
    rows
}

impl Parsed {
    /// Read the ask and the display on a `reply` call.
    pub fn of(call: &Call) -> Parsed {
        let mut notes = Vec::new();
        for (key, _) in &call.args {
            if !ARGS.contains(&key.as_str()) && !crate::show::ARGS.contains(&key.as_str()) {
                notes.push(format!("unknown argument {key}= was ignored"));
            }
        }
        let ask = Self::ask(call, &mut notes);
        let show = crate::show::read(call, &mut notes);
        Parsed { ask, show, notes }
    }

    fn ask(call: &Call, notes: &mut Vec<String>) -> Option<Ask> {
        let arg = |k: &str| call.arg(k).map(str::trim).filter(|v| !v.is_empty());
        let list = || arg("options").or(arg("choices")).map(split).unwrap_or_default();
        let mut kind = match arg("ask") {
            // Today's reply with choices: a choice; with fields: a form.
            None if arg("fields").is_some() => "form",
            None if arg("options").is_some() || arg("choices").is_some() => "choice",
            None => return None,
            Some(v) => match kind(v) {
                Some(k) => k,
                None => {
                    notes.push(format!("ask=\"{v}\" is not one of {}; sent as a plain reply", kinds().join(", ")));
                    return None;
                }
            },
        };
        if kind == "card" {
            notes.push("ask=\"card\" is now ask=\"review\"; read as a review".into());
            kind = "review";
        }
        // A confirm that carries details is a review: the user checks them.
        if kind == "confirm" {
            let detail = list();
            if arg("rows").is_some() {
                notes.push("ask=\"confirm\" with rows= is a review; drawn as one (confirm, edit or reject)".into());
                kind = "review";
            } else if !detail.is_empty() && detail.iter().all(|d| d.contains(':')) {
                notes.push("ask=\"confirm\" with `key: value` options is a review; drawn as one (confirm, edit or reject)".into());
                kind = "review";
            }
        }
        if arg("fields").is_some() && kind != "form" {
            notes.push(format!("fields= makes a form; ask=\"{kind}\" read as ask=\"form\""));
            kind = "form";
        }
        match kind {
            "choice" | "multi" => {
                let items = cap(list(), notes);
                if items.len() < 2 {
                    notes.push(format!("ask=\"{kind}\" needs 2 or more options, like options=\"A|B|C\"; sent as a plain reply"));
                    None
                } else if kind == "choice" {
                    Some(Ask::Choice { options: items })
                } else {
                    Some(Ask::Multi { options: items })
                }
            }
            "confirm" => Some(Ask::Confirm),
            "date" => Some(Ask::Date),
            "datetime" => Some(Ask::DateTime),
            "number" => {
                let mut bound = |k: &str| {
                    let v = arg(k)?;
                    let n = v.parse::<f64>().ok().filter(|n| n.is_finite());
                    if n.is_none() {
                        notes.push(format!("{k}=\"{v}\" is not a number; left out"));
                    }
                    n
                };
                let (min, max) = (bound("min"), bound("max"));
                let (min, max) = ordered(min, max, notes);
                Some(Ask::Number { min, max, unit: arg("unit").unwrap_or("").to_string() })
            }
            "text" => Some(Ask::Text { placeholder: arg("placeholder").unwrap_or("").to_string() }),
            "form" => {
                let fields: Vec<Field> = top_level(arg("fields").unwrap_or("")).into_iter().filter_map(|f| Field::read(f, notes)).collect();
                let fields = if fields.len() > MAX_ITEMS {
                    notes.push(format!("a form shows {MAX_ITEMS} fields at most; the rest were cut"));
                    fields.into_iter().take(MAX_ITEMS).collect()
                } else {
                    fields
                };
                if fields.is_empty() {
                    notes.push("ask=\"form\" needs fields, like fields=\"Day:date|Guests:number(1-12)|Notes:text\"; sent as a plain reply".into());
                    None
                } else {
                    Some(Ask::Form { title: arg("title").unwrap_or("").to_string(), fields })
                }
            }
            _ => {
                // review: its rows, written `key: value`.
                let items = arg("rows").map(split).unwrap_or_else(list);
                let rows = review_rows(items, notes);
                if rows.is_empty() {
                    notes.push("ask=\"review\" needs rows, like rows=\"env: staging|time: 18:00\"; sent as a plain reply".into());
                    None
                } else {
                    Some(Ask::Review { title: arg("title").unwrap_or("Summary").to_string(), rows })
                }
            }
        }
    }

    /// The notes as one result line for the model, if any.
    pub fn note(&self) -> Option<String> {
        (!self.notes.is_empty()).then(|| format!("note: {}", self.notes.join("; ")))
    }
}

/// `min` and `max` as given, both left out (with a note) when `min` is
/// above `max`.
fn ordered(min: Option<f64>, max: Option<f64>, notes: &mut Vec<String>) -> (Option<f64>, Option<f64>) {
    match (min, max) {
        (Some(lo), Some(hi)) if lo > hi => {
            notes.push(format!("min={} is above max={}; both left out", number_text(lo), number_text(hi)));
            (None, None)
        }
        _ => (min, max),
    }
}

/// `fields` split on `|`, `;` or new lines outside brackets, so
/// `Size:choice(S|M|L)` stays one field.
fn top_level(text: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            '|' | ';' | '\n' if depth == 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out.into_iter().map(str::trim).filter(|f| !f.is_empty()).collect()
}

/// A number field's bracket: `1-12`, `1..12`, `..5`, `-3..3`, then an
/// optional unit (`1-12 people`); anything else is the unit.
fn range(args: &str) -> (Option<f64>, Option<f64>, String) {
    let args = args.trim().replacen(" to ", "..", 1);
    let args = args.as_str();
    let (first, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let split = first.split_once("..").or_else(|| first.char_indices().skip(1).find(|(_, c)| *c == '-').map(|(i, _)| (&first[..i], &first[i + 1..])));
    if let Some((a, b)) = split {
        let side = |s: &str| if s.trim().is_empty() { Some(None) } else { s.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(Some) };
        if let (Some(lo), Some(hi)) = (side(a), side(b)) {
            return (lo, hi, rest.trim().to_string());
        }
    }
    (None, None, args.to_string())
}

impl Field {
    /// One field of `fields=`: `Label:kind`, `Label:kind(options or range
    /// or hint)`. A bad one is read as the nearest field, with a note.
    fn read(spec: &str, notes: &mut Vec<String>) -> Option<Field> {
        let Some(at) = spec.find([':', '=']) else {
            notes.push(format!("form field \"{spec}\" has no kind (Label:kind); read as text"));
            return Some(Field { label: spec.to_string(), ask: Ask::Text { placeholder: String::new() } });
        };
        let (label, rest) = (spec[..at].trim(), spec[at + 1..].trim());
        if label.is_empty() {
            notes.push(format!("form field \"{spec}\" has no label; left out"));
            return None;
        }
        let (name, args) = match rest.find(['(', '[']) {
            Some(open) => {
                let inner = &rest[open + 1..];
                let close = inner.rfind([')', ']']).unwrap_or(inner.len());
                (rest[..open].trim(), inner[..close].trim())
            }
            None => (rest, ""),
        };
        let text = |placeholder: &str| Ask::Text { placeholder: placeholder.to_string() };
        let options = |list: &str| {
            let mut out: Vec<String> = Vec::new();
            for o in list.split([',', '/', '|', ';']).map(str::trim).filter(|o| !o.is_empty()) {
                if !out.iter().any(|x| x == o) {
                    out.push(o.to_string());
                }
            }
            out
        };
        let ask = match kind(name) {
            None if args.is_empty() && name.contains([',', '/']) => {
                notes.push(format!("form field \"{label}\": \"{name}\" read as choice({name})"));
                Ask::Choice { options: options(name) }
            }
            None => {
                notes.push(format!("form field \"{label}\": \"{name}\" is not a field kind; read as text"));
                text("")
            }
            Some(k @ ("form" | "review" | "card")) => {
                notes.push(format!("form field \"{label}\": a {k} cannot be a field; read as text"));
                text("")
            }
            Some(k @ ("choice" | "multi")) => {
                let o = options(args);
                if o.len() < 2 {
                    notes.push(format!("form field \"{label}\": {k} needs 2 or more options, like {label}:{k}(A,B); read as text"));
                    text("")
                } else if k == "choice" {
                    Ask::Choice { options: cap(o, notes) }
                } else {
                    Ask::Multi { options: cap(o, notes) }
                }
            }
            Some("number") => {
                let (min, max, unit) = range(args);
                let (min, max) = ordered(min, max, notes);
                Ask::Number { min, max, unit }
            }
            Some("text") => text(args),
            Some(k) => {
                if !args.is_empty() {
                    notes.push(format!("form field \"{label}\": {k} takes nothing in brackets; ({args}) ignored"));
                }
                match k {
                    "confirm" => Ask::Confirm,
                    "date" => Ask::Date,
                    _ => Ask::DateTime,
                }
            }
        };
        Some(Field { label: label.to_string(), ask })
    }

    /// As `fields=` writes it: `Guests:number(1-12 people)`.
    pub fn spec(&self) -> String {
        let more = match &self.ask {
            Ask::Choice { options } | Ask::Multi { options } => options.join(","),
            Ask::Number { min, max, unit } => {
                let n = |v: &Option<f64>| v.map(number_text).unwrap_or_default();
                let r = match (min, max) {
                    (None, None) => String::new(),
                    (Some(lo), Some(_)) if *lo >= 0.0 => format!("{}-{}", n(min), n(max)),
                    _ => format!("{}..{}", n(min), n(max)),
                };
                [r, unit.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
            }
            Ask::Text { placeholder } => placeholder.clone(),
            _ => String::new(),
        };
        if more.is_empty() { format!("{}:{}", self.label, self.ask.name()) } else { format!("{}:{}({more})", self.label, self.ask.name()) }
    }
}

/// `reply(key="value", ...)`, each value quoted with `\` and `"` escaped.
pub fn write_call(args: &[(&str, String)]) -> String {
    let q = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    let args: Vec<String> = args.iter().map(|(k, v)| format!("{k}=\"{}\"", q(v))).collect();
    format!("reply({})", args.join(", "))
}

/// At most `MAX_ITEMS` of `items`, with a note when some are cut.
fn cap(mut items: Vec<String>, notes: &mut Vec<String>) -> Vec<String> {
    if items.len() > MAX_ITEMS {
        notes.push(format!("only the first {MAX_ITEMS} are shown"));
        items.truncate(MAX_ITEMS);
    }
    items
}

/// Weekday names, Monday first.
pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// 0 for Monday … 6 for Sunday, of a day since 1970-01-01 (a Thursday).
pub fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

/// `2026-11-24 (Tue)`.
pub fn date_text(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02} ({})", WEEKDAYS[weekday(day)])
}

/// `18:05`.
pub fn time_text(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60 % 24, minute % 60)
}

/// A number as typed: no `.0` on a whole one.
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

/// A month as a calendar: `(year, month)`'s days as weeks of seven,
/// Monday first, `None` before the 1st and after the last day. Each day is
/// days since 1970-01-01.
pub fn month_grid(year: i64, month: u32) -> Vec<[Option<i64>; 7]> {
    let first = days_from_civil(year, month, 1);
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let end = days_from_civil(ny, nm, 1);
    let mut weeks = Vec::new();
    let mut day = first - weekday(first) as i64;
    while day < end {
        let mut week = [None; 7];
        for (i, cell) in week.iter_mut().enumerate() {
            let d = day + i as i64;
            *cell = (first..end).contains(&d).then_some(d);
        }
        weeks.push(week);
        day += 7;
    }
    weeks
}

/// `November 2026`.
pub fn month_title(year: i64, month: u32) -> String {
    const NAMES: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!("{} {year}", NAMES[(month as usize + 11) % 12])
}

/// The quick picks for a date, from `today`: `(label, day)`.
pub fn quick_days(today: i64) -> Vec<(&'static str, i64)> {
    let monday = today + 7 - weekday(today) as i64;
    vec![("Today", today), ("Tomorrow", today + 1), ("Next Monday", monday), ("In a week", today + 7)]
}

/// The quick picks for a time of day, in minutes.
pub const QUICK_TIMES: [u32; 5] = [9 * 60, 12 * 60, 15 * 60, 18 * 60, 21 * 60];

impl Ask {
    /// The kind's name, as `ask` writes it.
    pub fn name(&self) -> &'static str {
        match self {
            Ask::Choice { .. } => "choice",
            Ask::Multi { .. } => "multi",
            Ask::Confirm => "confirm",
            Ask::Date => "date",
            Ask::DateTime => "datetime",
            Ask::Number { .. } => "number",
            Ask::Text { .. } => "text",
            Ask::Review { .. } => "review",
            Ask::Form { .. } => "form",
        }
    }

    /// The arguments that ask this, after `text`.
    pub fn args(&self) -> Vec<(&'static str, String)> {
        let mut args = vec![("ask", self.name().to_string())];
        match self {
            Ask::Choice { options } | Ask::Multi { options } => args.push(("options", options.join("|"))),
            Ask::Confirm | Ask::Date | Ask::DateTime => {}
            Ask::Number { min, max, unit } => {
                args.extend(min.map(|n| ("min", number_text(n))));
                args.extend(max.map(|n| ("max", number_text(n))));
                if !unit.is_empty() {
                    args.push(("unit", unit.clone()));
                }
            }
            Ask::Text { placeholder } if placeholder.is_empty() => {}
            Ask::Text { placeholder } => args.push(("placeholder", placeholder.clone())),
            Ask::Review { title, rows } => {
                args.push(("title", title.clone()));
                args.push(("rows", rows.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("|")));
            }
            Ask::Form { title, fields } => {
                if !title.is_empty() {
                    args.push(("title", title.clone()));
                }
                args.push(("fields", fields.iter().map(Field::spec).collect::<Vec<_>>().join("|")));
            }
        }
        args
    }

    /// The flat `reply` call that asks this, with `text` as the question.
    /// `ask=` comes first, so a model that copies it names the component
    /// early and a streaming shell draws it while the text is written.
    pub fn call(&self, text: &str) -> String {
        let mut args = self.args();
        args.push(("text", text.to_string()));
        write_call(&args)
    }

    /// A typed number, checked against the bounds. Error: why not, for the
    /// user.
    pub fn check_number(&self, typed: &str) -> Result<f64, String> {
        let Ask::Number { min, max, .. } = self else { return Err("not a number ask".into()) };
        let n = typed.trim().parse::<f64>().ok().filter(|n| n.is_finite()).ok_or("type a number")?;
        if let Some(lo) = min.filter(|lo| n < *lo) {
            return Err(format!("at least {}", number_text(lo)));
        }
        if let Some(hi) = max.filter(|hi| n > *hi) {
            return Err(format!("at most {}", number_text(hi)));
        }
        Ok(n)
    }

    /// The number field's first value: 0 moved inside the bounds.
    pub fn start_number(&self) -> f64 {
        match self {
            Ask::Number { min, max, .. } => {
                let n = min.map_or(0.0, |lo| lo.max(0.0));
                max.map_or(n, |hi| n.min(hi))
            }
            _ => 0.0,
        }
    }

    /// What a single control's `input` comes to: `None` when it was left
    /// blank (nothing picked, nothing typed). Error: why it cannot be
    /// sent, for the user.
    pub fn read_input(&self, input: &Input) -> Result<Option<Answer>, String> {
        let first = input.picked.first().map(|p| p.trim().to_string());
        Ok(match self {
            Ask::Choice { options } => match first {
                Some(p) if options.contains(&p) => Some(Answer::Choice(p)),
                Some(p) => return Err(format!("\"{p}\" is not one of the options")),
                None => None,
            },
            Ask::Multi { options } => Some(Answer::Multi(options.iter().filter(|o| input.picked.iter().any(|p| p.trim() == o.as_str())).cloned().collect())),
            Ask::Confirm => first.map(|p| Answer::Confirm(matches!(p.to_ascii_lowercase().as_str(), "yes" | "y" | "true"))),
            Ask::Date => input.day.map(Answer::Date),
            Ask::DateTime => input.day.map(|d| Answer::DateTime(d, input.minute.unwrap_or(9 * 60))),
            Ask::Number { .. } if input.typed.trim().is_empty() => None,
            Ask::Number { .. } => Some(Answer::Number(self.check_number(&input.typed)?)),
            Ask::Text { .. } => Some(input.typed.trim()).filter(|t| !t.is_empty()).map(|t| Answer::Text(t.to_string())),
            Ask::Review { .. } | Ask::Form { .. } => return Err(format!("a {} is not one control", self.name())),
        })
    }

    /// A form's answer from one `Input` per field, in order. Error: the
    /// first field that cannot be sent, `Label: why`.
    pub fn submit(&self, inputs: &[Input]) -> Result<String, String> {
        let Ask::Form { fields, .. } = self else { return Err("not a form".into()) };
        let blank = Input::default();
        let mut values = Vec::new();
        for (n, f) in fields.iter().enumerate() {
            values.push(f.ask.read_input(inputs.get(n).unwrap_or(&blank)).map_err(|why| format!("{}: {why}", f.label))?);
        }
        Ok(self.answer(&Answer::Form(values)))
    }

    /// The answer's value, without the `Answer (kind): ` before it.
    pub fn value(&self, a: &Answer) -> String {
        match (self, a) {
            (_, Answer::Choice(c)) => c.clone(),
            (_, Answer::Multi(v)) if v.is_empty() => "none".into(),
            (_, Answer::Multi(v)) => v.join(", "),
            (_, Answer::Confirm(yes)) => if *yes { "yes" } else { "no" }.into(),
            (_, Answer::Date(day)) => date_text(*day),
            (_, Answer::DateTime(day, minute)) => {
                let d = date_text(*day);
                let (date, wd) = d.split_once(' ').unwrap_or((&d, ""));
                format!("{date} {} {wd}", time_text(*minute))
            }
            (Ask::Number { unit, .. }, Answer::Number(n)) if !unit.is_empty() => format!("{} {unit}", number_text(*n)),
            (_, Answer::Number(n)) => number_text(*n),
            (_, Answer::Text(t)) => t.trim().replace('\n', " "),
            (_, Answer::Review(None)) => "rejected".into(),
            (Ask::Review { rows: asked, .. }, Answer::Review(Some(rows))) => {
                let edited = rows.iter().zip(asked).any(|((_, v), (_, was))| v.trim() != was);
                let lines: Vec<String> = rows
                    .iter()
                    .zip(asked)
                    .map(|((k, v), (_, was))| {
                        let v = v.trim();
                        if v == was { format!("{k}: {v}") } else { format!("{k}: {v} (was {was})") }
                    })
                    .collect();
                format!("{}\n{}", if edited { "edited" } else { "confirmed" }, lines.join("\n"))
            }
            (_, Answer::Review(Some(rows))) => rows.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n"),
            (Ask::Form { fields, .. }, Answer::Form(values)) => {
                let lines: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(n, f)| {
                        let v = values.get(n).and_then(Option::as_ref).map_or("(blank)".to_string(), |a| f.ask.value(a));
                        format!("{}: {}", f.label, v.replace('\n', " "))
                    })
                    .collect();
                format!("submitted\n{}", lines.join("\n"))
            }
            (_, Answer::Form(_)) => "submitted".into(),
        }
    }

    /// The answer as the user's message: `Answer (<kind>): <value>`, a
    /// review's rows and a form's fields one per line under it.
    pub fn answer(&self, a: &Answer) -> String {
        format!("Answer ({}): {}", self.name(), self.value(a))
    }
}

/// The value of a user message that answers an ask (`Answer (date): …`):
/// `(kind, value)`. `card` is still read, for histories from before
/// `review`.
pub fn read_answer(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("Answer (")?;
    let (kind, value) = rest.split_once("): ")?;
    (kinds().contains(&kind) || kind == "card").then_some((kind, value))
}

/// The `reply` call in a model's answer, read with the engine's contract.
pub fn reply_call(e: &Engine, text: &str) -> Option<Call> {
    if e.response.is_empty() {
        return None;
    }
    let answer = e.format.parse(&e.response, text).ok()?;
    calls::parse(Responses::get(&answer, crate::turn::ACT)?).ok()?.into_iter().find(|c| c.name == "reply")
}

/// The ask in a model's answer, if its reply has one.
pub fn ask_in(e: &Engine, text: &str) -> Option<Ask> {
    reply_call(e, text).and_then(|c| Parsed::of(&c).ask)
}

/// What the reply in a model's answer shows (`crate::show`), if anything.
pub fn show_in(e: &Engine, text: &str) -> Option<crate::show::Show> {
    reply_call(e, text).and_then(|c| Parsed::of(&c).show)
}

/// Where message `n`'s ask stands: open (nothing from the user since, so
/// it can be answered), or answered with the user's next message (`None`
/// if they typed something else instead).
#[derive(Clone, Debug, PartialEq)]
pub enum State<'a> {
    Open,
    Answered(Option<&'a str>),
}

/// The state of the ask in message `n` of `e`.
pub fn state(e: &Engine, n: usize) -> State<'_> {
    match e.messages.iter().skip(n + 1).find(|m| m.role == Role::User) {
        None => State::Open,
        Some(m) => State::Answered(read_answer(&m.text).map(|(_, v)| v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calls::parse;

    fn read(s: &str) -> Parsed {
        Parsed::of(&parse(s).unwrap().remove(0))
    }

    #[test]
    fn each_component_reads_from_flat_arguments() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(read(r#"reply(text="Pick", ask="choice", options="Red|Green|Blue")"#).ask, Some(Ask::Choice { options: s(&["Red", "Green", "Blue"]) }));
        assert_eq!(read(r#"reply(text="Pick", ask="multi", options="unit| ui |e2e|ui")"#).ask, Some(Ask::Multi { options: s(&["unit", "ui", "e2e"]) }));
        assert_eq!(read(r#"reply(text="Go?", ask="confirm")"#).ask, Some(Ask::Confirm));
        assert_eq!(read(r#"reply(text="When?", ask="date")"#).ask, Some(Ask::Date));
        assert_eq!(read(r#"reply(text="When?", ask="datetime")"#).ask, Some(Ask::DateTime));
        assert_eq!(
            read(r#"reply(text="How many?", ask="number", min="1", max="10", unit="copies")"#).ask,
            Some(Ask::Number { min: Some(1.0), max: Some(10.0), unit: "copies".into() })
        );
        assert_eq!(read(r#"reply(text="Name?", ask="text", placeholder="a name")"#).ask, Some(Ask::Text { placeholder: "a name".into() }));
        assert_eq!(
            read(r#"reply(text="Right?", ask="review", title="Deploy", rows="env: staging|time: 18:00")"#).ask,
            Some(Ask::Review { title: "Deploy".into(), rows: vec![("env".into(), "staging".into()), ("time".into(), "18:00".into())] })
        );
        for r in ["Red|Green", "env: x"] {
            assert!(read(&format!("reply(text=\"x\", ask=\"review\", rows=\"{r}\")")).ask.is_some());
        }
    }

    #[test]
    fn every_registered_example_reads_back_as_itself() {
        let all = instructions();
        for e in registry() {
            let call = e.ask.call(e.text);
            assert_eq!(read(&call), Parsed { ask: Some(e.ask.clone()), show: None, notes: vec![] }, "{call}");
            assert!(all.contains(&format!("  {} ({}): {call}", e.ask.name(), e.when)), "{all}");
        }
        assert_eq!(kinds(), ["choice", "multi", "confirm", "date", "datetime", "number", "text", "review", "form"]);
        let json = serde_json::to_string(&Ask::Date).unwrap();
        assert_eq!(json, r#"{"kind":"date"}"#);
        // A page file written with `card` still reads, as a review.
        let card: Ask = serde_json::from_str(r#"{"kind":"card","rows":[["env","staging"]]}"#).unwrap();
        assert_eq!(card, Ask::Review { title: String::new(), rows: vec![("env".into(), "staging".into())] });
        let form: Ask = serde_json::from_str(r#"{"kind":"form","fields":[{"label":"Day","ask":{"kind":"date"}}]}"#).unwrap();
        assert_eq!(form, Ask::Form { title: String::new(), fields: vec![Field { label: "Day".into(), ask: Ask::Date }] });
        // The displays are in the same instructions.
        assert!(all.contains("show=\"table\""), "{all}");
    }

    #[test]
    fn defaults_and_near_misses() {
        // No ask: a plain reply; choices alone is today's choice.
        assert_eq!(read(r#"reply(text="hi")"#), Parsed::default());
        assert_eq!(read(r#"reply(text="Pick", choices="A|B")"#).ask, Some(Ask::Choice { options: vec!["A".into(), "B".into()] }));
        assert_eq!(read(r#"reply(text="Ok?", ask="Yes/No")"#).ask, Some(Ask::Confirm));
        assert_eq!(read(r#"reply(text="x", ask="checkbox", choices="a|b")"#).ask.map(|a| a.name()), Some("multi"));
        assert_eq!(read(r#"reply(text="x", ask="number")"#).ask, Some(Ask::Number { min: None, max: None, unit: String::new() }));
        assert_eq!(read(r#"reply(text="x", ask="text")"#).ask, Some(Ask::Text { placeholder: String::new() }));
        let review = read(r#"reply(text="x", ask="summary", options="a: 1")"#);
        assert_eq!(review.ask, Some(Ask::Review { title: "Summary".into(), rows: vec![("a".into(), "1".into())] }));
        assert!(review.notes.is_empty());
    }

    #[test]
    fn broken_input_degrades_with_a_note() {
        let p = read(r#"reply(text="x", ask="calendar")"#);
        assert_eq!(p.ask, None);
        assert!(p.note().unwrap().contains("ask=\"calendar\" is not one of choice, multi, confirm"), "{p:?}");
        let p = read(r#"reply(text="x", ask="choice", options="only")"#);
        assert_eq!(p.ask, None);
        assert!(p.notes[0].contains("needs 2 or more options"));
        let p = read(r#"reply(text="x", ask="review")"#);
        assert_eq!(p.ask, None);
        assert!(p.notes[0].contains("needs rows"));
        let p = read(r#"reply(text="x", ask="number", min="one", max="5")"#);
        assert_eq!(p.ask, Some(Ask::Number { min: None, max: Some(5.0), unit: String::new() }));
        assert_eq!(p.notes, ["min=\"one\" is not a number; left out"]);
        let p = read(r#"reply(text="x", ask="number", min="9", max="2")"#);
        assert_eq!(p.ask, Some(Ask::Number { min: None, max: None, unit: String::new() }));
        let p = read(r#"reply(text="x", ask="review", rows="env: a|just words", question="?")"#);
        assert!(p.ask.is_some());
        assert_eq!(p.notes, ["unknown argument question= was ignored", "review row \"just words\" is not `key: value`"]);
        let many = (0..20).map(|i| i.to_string()).collect::<Vec<_>>().join("|");
        let p = read(&format!("reply(text=\"x\", ask=\"multi\", options=\"{many}\")"));
        assert!(matches!(&p.ask, Some(Ask::Multi { options }) if options.len() == MAX_ITEMS));
    }

    #[test]
    fn answers_are_one_readable_line() {
        let day = days_from_civil(2026, 11, 24);
        let choice = Ask::Choice { options: vec!["A".into(), "B".into()] };
        assert_eq!(choice.answer(&Answer::Choice("B".into())), "Answer (choice): B");
        assert_eq!(Ask::Multi { options: vec![] }.answer(&Answer::Multi(vec!["unit".into(), "e2e".into()])), "Answer (multi): unit, e2e");
        assert_eq!(Ask::Multi { options: vec![] }.answer(&Answer::Multi(vec![])), "Answer (multi): none");
        assert_eq!(Ask::Confirm.answer(&Answer::Confirm(false)), "Answer (confirm): no");
        assert_eq!(Ask::Date.answer(&Answer::Date(day)), "Answer (date): 2026-11-24 (Tue)");
        assert_eq!(Ask::DateTime.answer(&Answer::DateTime(day, 18 * 60 + 5)), "Answer (datetime): 2026-11-24 18:05 (Tue)");
        let n = Ask::Number { min: None, max: None, unit: "copies".into() };
        assert_eq!(n.answer(&Answer::Number(3.0)), "Answer (number): 3 copies");
        assert_eq!(Ask::Text { placeholder: String::new() }.answer(&Answer::Text(" Ada\n".into())), "Answer (text): Ada");
        let card = Ask::Review { title: "Deploy".into(), rows: vec![("env".into(), "staging".into()), ("time".into(), "18:00".into())] };
        let same = vec![("env".into(), "staging".into()), ("time".into(), "18:00".into())];
        assert_eq!(card.answer(&Answer::Review(Some(same))), "Answer (review): confirmed\nenv: staging\ntime: 18:00");
        let edited = vec![("env".into(), "prod".into()), ("time".into(), "18:00".into())];
        assert_eq!(card.answer(&Answer::Review(Some(edited))), "Answer (review): edited\nenv: prod (was staging)\ntime: 18:00");
        assert_eq!(card.answer(&Answer::Review(None)), "Answer (review): rejected");
        assert_eq!(read_answer("Answer (card): rejected"), Some(("card", "rejected")));
        assert_eq!(read_answer("Answer (date): 2026-11-24 (Tue)"), Some(("date", "2026-11-24 (Tue)")));
        assert_eq!(read_answer("Answer (nope): x"), None);
    }

    #[test]
    fn card_and_confirm_with_details_read_as_a_review() {
        let rows = vec![("env".to_string(), "staging".to_string()), ("time".to_string(), "18:00".to_string())];
        let p = read(r#"reply(text="Right?", ask="card", title="Deploy", rows="env: staging|time: 18:00")"#);
        assert_eq!(p.ask, Some(Ask::Review { title: "Deploy".into(), rows: rows.clone() }));
        assert_eq!(p.notes, ["ask=\"card\" is now ask=\"review\"; read as a review"]);
        let p = read(r#"reply(text="Deploy this?", ask="confirm", title="Deploy", rows="env: staging|time: 18:00")"#);
        assert_eq!(p.ask, Some(Ask::Review { title: "Deploy".into(), rows: rows.clone() }));
        assert!(p.note().unwrap().contains("ask=\"confirm\" with rows= is a review"), "{p:?}");
        // Rows without a title: the default one.
        let p = read(r#"reply(text="Ok?", ask="yes/no", rows="env: staging|time: 18:00")"#);
        assert_eq!(p.ask, Some(Ask::Review { title: "Summary".into(), rows: rows.clone() }));
        let p = read(r#"reply(text="Ok?", ask="confirm", options="env: staging|time: 18:00")"#);
        assert_eq!(p.ask, Some(Ask::Review { title: "Summary".into(), rows }));
        assert!(p.notes[0].contains("`key: value` options is a review"));
        // A plain confirm stays one; a title alone changes nothing.
        assert_eq!(read(r#"reply(text="Go?", ask="confirm", title="Deploy")"#), Parsed { ask: Some(Ask::Confirm), ..Parsed::default() });
    }

    #[test]
    fn a_form_reads_from_one_flat_string() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let f = |label: &str, ask: Ask| Field { label: label.into(), ask };
        let p = read(r#"reply(text="Book it", ask="form", fields="When:datetime | Guests:number(1-12 people)|Size:choice(S,M,L)|Extras:multi(bread/wine)|Notes:text(any allergies?)|Ok:confirm")"#);
        assert_eq!(
            p.ask,
            Some(Ask::Form {
                title: String::new(),
                fields: vec![
                    f("When", Ask::DateTime),
                    f("Guests", Ask::Number { min: Some(1.0), max: Some(12.0), unit: "people".into() }),
                    f("Size", Ask::Choice { options: s(&["S", "M", "L"]) }),
                    f("Extras", Ask::Multi { options: s(&["bread", "wine"]) }),
                    f("Notes", Ask::Text { placeholder: "any allergies?".into() }),
                    f("Ok", Ask::Confirm),
                ],
            })
        );
        assert!(p.notes.is_empty(), "{p:?}");
        // `|` inside the brackets stays in the field; ranges in other spellings.
        let p = read(r#"reply(text="x", fields="Size: choice(S|M|L); Age=number(..120); Temp:number(-5..5 C); N:number(1 to 3)")"#);
        let Some(Ask::Form { ref fields, .. }) = p.ask else { panic!("{p:?}") };
        assert_eq!(fields[0].ask, Ask::Choice { options: s(&["S", "M", "L"]) });
        assert_eq!(fields[1].ask, Ask::Number { min: None, max: Some(120.0), unit: String::new() });
        assert_eq!(fields[2].ask, Ask::Number { min: Some(-5.0), max: Some(5.0), unit: "C".into() });
        assert_eq!(fields[3].ask, Ask::Number { min: Some(1.0), max: Some(3.0), unit: String::new() });
        assert!(p.notes.is_empty(), "{p:?}");
        // Each field writes back as it reads.
        for field in fields {
            let again = read(&format!("reply(text=\"x\", ask=\"form\", fields=\"{}\")", field.spec()));
            assert_eq!(again.ask, Some(Ask::Form { title: String::new(), fields: vec![field.clone()] }), "{}", field.spec());
        }
    }

    #[test]
    fn a_broken_form_field_is_the_nearest_one_with_a_note() {
        let p = read(r#"reply(text="x", ask="form", fields="Name|Size:S/M/L|Pet:animal|Day:date(soon)|Pick:choice(one)|:text|Sub:form|Age:number(9-2)")"#);
        let Some(Ask::Form { fields, .. }) = &p.ask else { panic!("{p:?}") };
        let kinds: Vec<&str> = fields.iter().map(|f| f.ask.name()).collect();
        assert_eq!(kinds, ["text", "choice", "text", "date", "text", "text", "number"]);
        let all = p.notes.join("\n");
        for want in [
            "form field \"Name\" has no kind",
            "\"S/M/L\" read as choice(S/M/L)",
            "\"animal\" is not a field kind",
            "date takes nothing in brackets; (soon) ignored",
            "choice needs 2 or more options",
            "form field \":text\" has no label; left out",
            "a form cannot be a field",
            "min=9 is above max=2",
        ] {
            assert!(all.contains(want), "{want} not in {all}");
        }
        let p = read(r#"reply(text="x", ask="form")"#);
        assert_eq!(p.ask, None);
        assert!(p.notes[0].contains("needs fields"));
        // fields= with another ask: a form.
        let p = read(r#"reply(text="x", ask="text", fields="A:text|B:date")"#);
        assert!(matches!(p.ask, Some(Ask::Form { .. })));
        assert!(p.notes[0].contains("fields= makes a form"));
        let many = (0..20).map(|i| format!("F{i}:text")).collect::<Vec<_>>().join("|");
        let p = read(&format!("reply(text=\"x\", ask=\"form\", fields=\"{many}\")"));
        assert!(matches!(&p.ask, Some(Ask::Form { fields, .. }) if fields.len() == MAX_ITEMS));
    }

    #[test]
    fn a_form_answers_once_with_a_line_per_field() {
        let day = days_from_civil(2026, 11, 24);
        let Some(form) = read(r#"reply(text="x", ask="form", fields="Day:datetime|Guests:number(1-12 people)|Seat:choice(in,out)|Extras:multi(a,b)|Notes:text|Ok:confirm")"#).ask else {
            panic!()
        };
        let i = |picked: &[&str], day: Option<i64>, typed: &str| Input { picked: picked.iter().map(|p| p.to_string()).collect(), day, minute: None, typed: typed.into() };
        let inputs = [i(&[], Some(day), ""), i(&[], None, " 4 "), i(&["out"], None, ""), i(&["b"], None, ""), i(&[], None, "  "), i(&["yes"], None, "")];
        assert_eq!(
            form.submit(&inputs),
            Ok("Answer (form): submitted\nDay: 2026-11-24 09:00 (Tue)\nGuests: 4 people\nSeat: out\nExtras: b\nNotes: (blank)\nOk: yes".into())
        );
        // Missing inputs are blank; a bad number names its field.
        assert!(form.submit(&[]).unwrap().contains("Day: (blank)\nGuests: (blank)\nSeat: (blank)\nExtras: none"));
        assert_eq!(form.submit(&[i(&[], None, ""), i(&[], None, "40")]), Err("Guests: at most 12".into()));
        assert_eq!(form.submit(&[i(&[], None, ""), i(&[], None, ""), i(&["sideways"], None, "")]), Err("Seat: \"sideways\" is not one of the options".into()));
        assert_eq!(read_answer(&form.submit(&[]).unwrap()).map(|(k, _)| k), Some("form"));
        assert!(Ask::Confirm.submit(&[]).is_err());
        assert!(form.read_input(&Input::default()).is_err());
    }

    #[test]
    fn a_reply_can_ask_and_show_at_once() {
        let p = read(r#"reply(text="Here is the plan. Which day?", ask="date", show="table", data="Step|Owner;build|me;ship|you")"#);
        assert_eq!(p.ask, Some(Ask::Date));
        assert!(matches!(p.show, Some(crate::show::Show::Table { .. })));
        assert!(p.notes.is_empty());
        let p = read(r#"reply(text="x", show="chart")"#);
        assert_eq!((&p.ask, &p.show), (&None, &None));
        assert!(p.note().unwrap().contains("show=\"chart\""));
    }

    #[test]
    fn numbers_and_dates() {
        let n = Ask::Number { min: Some(1.0), max: Some(10.0), unit: String::new() };
        assert_eq!(n.check_number(" 4 "), Ok(4.0));
        assert_eq!(n.check_number("0"), Err("at least 1".into()));
        assert_eq!(n.check_number("11.5"), Err("at most 10".into()));
        assert_eq!(n.check_number("x"), Err("type a number".into()));
        assert_eq!(n.start_number(), 1.0);
        assert_eq!(Ask::Number { min: Some(-5.0), max: Some(-2.0), unit: String::new() }.start_number(), -2.0);
        assert_eq!(Ask::Number { min: None, max: None, unit: String::new() }.start_number(), 0.0);
        assert_eq!(number_text(2.5), "2.5");
        // 2026-11-22 is a Sunday; November 2026 starts on a Sunday.
        let sunday = days_from_civil(2026, 11, 22);
        assert_eq!(WEEKDAYS[weekday(sunday)], "Sun");
        let grid = month_grid(2026, 11);
        assert_eq!(grid[0][..6], [None; 6]);
        assert_eq!(grid[0][6], Some(days_from_civil(2026, 11, 1)));
        assert_eq!(grid.last().unwrap()[0], Some(days_from_civil(2026, 11, 30)));
        assert_eq!(grid.len(), 6);
        assert_eq!(month_title(2026, 12), "December 2026");
        let q = quick_days(sunday);
        assert_eq!(q[2], ("Next Monday", sunday + 1));
        assert_eq!(time_text(9 * 60), "09:00");
    }
}
