//! Display components: what a reply shows with its text, instead of
//! drawing a table or a bar in characters.
//!
//! ```text
//! reply(show="table", data="Name|Age;Ada|36;Bob|41", text="Here are the users.")
//!   ─▶ Show::Table ─▶ a shell draws a table, from tokens
//! ```
//!
//! They show, they never ask: nothing comes back from the user. A reply
//! carries at most one `show=`, next to at most one `ask=` (`crate::ask`),
//! so "here is the plan, which day?" is one reply.
//!
//! Like the asks, the set is closed and registered (`registry`), the
//! model's instructions are written from the registry, and every argument
//! is one flat string: `data=`, with `|` between the cells of a row and
//! `;` between rows. A broken one never fails the reply: it is cut or
//! left out, and the note says why (`crate::ask::Parsed::notes`).

use serde::{Deserialize, Serialize};

use crate::ask::number_text;
use crate::calls::Call;

/// The most rows a table or list shows, and stats a stat row.
pub const MAX_ROWS: usize = 30;
/// The most columns a table shows.
pub const MAX_COLS: usize = 8;

/// One registered display component, as the model is told it.
pub struct Entry {
    pub show: Show,
    pub when: &'static str,
    pub text: &'static str,
}

/// The registry: every display component, in the order the model reads
/// them. A test reads every example back, like the asks'.
pub fn registry() -> Vec<Entry> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    vec![
        Entry {
            show: Show::Table { caption: String::new(), head: s(&["Name", "Age"]), rows: vec![s(&["Ada", "36"]), s(&["Bob", "41"])] },
            when: "rows and columns",
            text: "Here are the users.",
        },
        Entry { show: Show::List { caption: String::new(), items: s(&["lint passed", "3 tests failed", "build skipped"]) }, when: "a few items", text: "The CI run:" },
        Entry { show: Show::Progress { label: "tests passed".into(), value: 7.0, max: 10.0 }, when: "how far along", text: "Still running." },
        Entry {
            show: Show::Stat {
                caption: String::new(),
                stats: vec![
                    Stat { label: "Revenue".into(), value: "$12.4k".into(), delta: "+8%".into() },
                    Stat { label: "Users".into(), value: "1204".into(), delta: "-3%".into() },
                ],
            },
            when: "headline numbers",
            text: "This week:",
        },
    ]
}

/// Every `show` value, in the registry's order.
pub fn kinds() -> Vec<&'static str> {
    registry().iter().map(|e| e.show.name()).collect()
}

/// What `reply`'s description adds for the displays.
pub fn instructions() -> String {
    let lines: Vec<String> = registry().iter().map(|e| format!("  {} ({}): {}", e.show.name(), e.when, e.show.call(e.text))).collect();
    format!(
        " To show data with a reply, add show= and data= (it can go with an ask=); the user sees a real table, list, bar or numbers. One show per reply:\n{}\nIn data, | separates the cells of a row and ; the rows; a table's first row is its header, a stat is label|value|change.",
        lines.join("\n")
    )
}

/// The arguments the displays read.
pub const ARGS: &[&str] = &["show", "data", "caption", "cols", "columns"];

/// What a reply shows: the registered display components. In a page file
/// it is `{"type": "show", "show": {"kind": "list", "items": ["a"]}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Show {
    /// Rows under a header row; every row as wide as the header.
    Table {
        #[serde(default)]
        caption: String,
        head: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// Items, one per line.
    List {
        #[serde(default)]
        caption: String,
        items: Vec<String>,
    },
    /// `value` of `max`, as a bar, with what it counts.
    Progress {
        #[serde(default)]
        label: String,
        value: f64,
        max: f64,
    },
    /// Headline numbers side by side.
    Stat {
        #[serde(default)]
        caption: String,
        stats: Vec<Stat>,
    },
}

/// One headline number: what it is, its value, and how it changed
/// (`+8%`, `-3`, or empty).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stat {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub delta: String,
}

/// Which way a stat moved, for the colour a shell gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    Up,
    Down,
    Flat,
}

impl Stat {
    pub fn trend(&self) -> Trend {
        match self.delta.trim().chars().next() {
            Some('+' | '▲' | '↑') => Trend::Up,
            Some('-' | '−' | '▼' | '↓') => Trend::Down,
            _ => Trend::Flat,
        }
    }
}

/// The kind a `show` value names (near misses read), for a reply
/// still being written (`crate::stream`).
pub fn kind_name(value: &str) -> Option<&'static str> {
    kind(value)
}

/// The kind a `show` value names, forgiving near misses.
fn kind(value: &str) -> Option<&'static str> {
    let v = value.trim().to_ascii_lowercase().replace(['-', '_', ' '], "");
    Some(match v.as_str() {
        "table" | "grid" | "rows" | "datatable" => "table",
        "list" | "items" | "bullets" | "bullet" => "list",
        "progress" | "bar" | "progressbar" | "meter" | "gauge" => "progress",
        "stat" | "stats" | "kpi" | "metric" | "metrics" | "number" | "numbers" => "stat",
        _ => return None,
    })
}

/// Rows of `data`: split on `;` or new lines, trimmed, blanks dropped.
fn rows_of(data: &str) -> Vec<&str> {
    data.split([';', '\n']).map(str::trim).filter(|r| !r.is_empty()).collect()
}

/// Cells of a row: split on `|`, trimmed.
fn cells(row: &str) -> Vec<String> {
    row.split('|').map(|c| c.trim().to_string()).collect()
}

/// Read the display on a `reply` call, adding what was wrong to `notes`.
pub fn read(call: &Call, notes: &mut Vec<String>) -> Option<Show> {
    let arg = |k: &str| call.arg(k).map(str::trim).filter(|v| !v.is_empty());
    let caption = arg("caption").unwrap_or("").to_string();
    let v = arg("show")?;
    let Some(kind) = kind(v) else {
        notes.push(format!("show=\"{v}\" is not one of {}; nothing shown", kinds().join(", ")));
        return None;
    };
    // A table written with cols= (and rows=, when no ask uses it).
    let cols = arg("cols").or(arg("columns"));
    let data = match (arg("data"), cols) {
        (Some(d), _) => d.to_string(),
        (None, Some(c)) if kind == "table" && call.arg("ask").is_none() => {
            format!("{c};{}", arg("rows").unwrap_or(""))
        }
        _ => {
            notes.push(format!("show=\"{kind}\" needs data=, like {}; nothing shown", example_data(kind)));
            return None;
        }
    };
    match kind {
        "table" => {
            let mut all = rows_of(&data).into_iter().map(cells);
            let Some(mut head) = all.next() else {
                notes.push("show=\"table\" data= is empty; nothing shown".into());
                return None;
            };
            if head.len() > MAX_COLS {
                notes.push(format!("a table shows {MAX_COLS} columns at most; the rest were cut"));
                head.truncate(MAX_COLS);
            }
            let mut rows: Vec<Vec<String>> = all.collect();
            if rows.is_empty() {
                notes.push("show=\"table\" needs rows after its header row, like data=\"Name|Age;Ada|36\"; nothing shown".into());
                return None;
            }
            if rows.len() > MAX_ROWS {
                notes.push(format!("a table shows {MAX_ROWS} rows at most; the rest were cut"));
                rows.truncate(MAX_ROWS);
            }
            let mut ragged = false;
            for r in &mut rows {
                if r.len() != head.len() {
                    ragged = true;
                    r.resize(head.len(), String::new());
                }
            }
            if ragged {
                notes.push(format!("every table row needs {} cells, like its header; short rows were padded, long ones cut", head.len()));
            }
            Some(Show::Table { caption, head, rows })
        }
        "list" => {
            let mut items: Vec<String> = data.split(['|', ';', '\n']).map(str::trim).filter(|i| !i.is_empty()).map(str::to_string).collect();
            if items.len() > MAX_ROWS {
                notes.push(format!("a list shows {MAX_ROWS} items at most; the rest were cut"));
                items.truncate(MAX_ROWS);
            }
            if items.is_empty() {
                notes.push("show=\"list\" data= has no items; nothing shown".into());
                return None;
            }
            Some(Show::List { caption, items })
        }
        "progress" => match progress(&data) {
            Some((value, max, label)) => {
                let label = if label.is_empty() { caption } else { label };
                if value > max || value < 0.0 {
                    notes.push(format!("progress {} is outside 0 to {}; shown at the nearest end", number_text(value), number_text(max)));
                }
                Some(Show::Progress { label, value: value.clamp(0.0, max), max })
            }
            None => {
                notes.push(format!("progress data=\"{data}\" is not like \"7/10 tests passed\" or \"70% copied\"; nothing shown"));
                None
            }
        },
        _ => {
            let mut stats = Vec::new();
            for r in rows_of(&data) {
                let mut c = cells(r);
                if c.len() == 1
                    && let Some((k, v)) = r.split_once(':')
                {
                    c = vec![k.trim().to_string(), v.trim().to_string()];
                }
                if c.len() < 2 || c[0].is_empty() {
                    notes.push(format!("stat \"{r}\" is not label|value|change; left out"));
                    continue;
                }
                if c.len() > 3 {
                    notes.push(format!("stat \"{r}\" has more than label|value|change; the rest was cut"));
                }
                stats.push(Stat { label: c[0].clone(), value: c[1].clone(), delta: c.get(2).cloned().unwrap_or_default() });
            }
            if stats.len() > MAX_ROWS {
                stats.truncate(MAX_ROWS);
            }
            if stats.is_empty() {
                notes.push("show=\"stat\" has no label|value; nothing shown".into());
                return None;
            }
            Some(Show::Stat { caption, stats })
        }
    }
}

/// `7/10 label`, `7 of 10 label` or `70% label`: `(value, max, label)`.
fn progress(data: &str) -> Option<(f64, f64, String)> {
    let data = data.trim();
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    let (first, rest) = data.split_once(char::is_whitespace).unwrap_or((data, ""));
    if let Some(p) = first.strip_suffix('%') {
        return Some((num(p)?, 100.0, rest.trim().to_string()));
    }
    let (value, max, rest) = if let Some((a, b)) = first.split_once('/') {
        (num(a)?, num(b)?, rest.trim())
    } else {
        // `7 of 10 label`
        let mut words = rest.splitn(3, char::is_whitespace);
        if words.next() != Some("of") {
            return None;
        }
        (num(first)?, num(words.next()?)?, words.next().unwrap_or("").trim())
    };
    (max > 0.0).then(|| (value, max, rest.to_string()))
}

fn example_data(kind: &str) -> String {
    registry().into_iter().find(|e| e.show.name() == kind).map_or(String::new(), |e| format!("data=\"{}\"", e.show.data()))
}

impl Show {
    /// The kind's name, as `show` writes it.
    pub fn name(&self) -> &'static str {
        match self {
            Show::Table { .. } => "table",
            Show::List { .. } => "list",
            Show::Progress { .. } => "progress",
            Show::Stat { .. } => "stat",
        }
    }

    /// Its `data=` string.
    pub fn data(&self) -> String {
        match self {
            Show::Table { head, rows, .. } => std::iter::once(head).chain(rows).map(|r| r.join("|")).collect::<Vec<_>>().join(";"),
            Show::List { items, .. } => items.join("|"),
            Show::Progress { label, value, max } => format!("{}/{} {label}", number_text(*value), number_text(*max)).trim().to_string(),
            Show::Stat { stats, .. } => stats
                .iter()
                .map(|s| if s.delta.is_empty() { format!("{}|{}", s.label, s.value) } else { format!("{}|{}|{}", s.label, s.value, s.delta) })
                .collect::<Vec<_>>()
                .join(";"),
        }
    }

    /// The caption it carries (`caption=`), if any.
    pub fn caption(&self) -> &str {
        match self {
            Show::Table { caption, .. } | Show::List { caption, .. } | Show::Stat { caption, .. } => caption,
            Show::Progress { .. } => "",
        }
    }

    /// The arguments that write it, after `text` and any `ask`.
    pub fn args(&self) -> Vec<(&'static str, String)> {
        let mut args = vec![("show", self.name().to_string()), ("data", self.data())];
        if !self.caption().is_empty() {
            args.push(("caption", self.caption().to_string()));
        }
        args
    }

    /// The flat `reply` call that shows this, with `text`, `show=` first
    /// (as an ask's: drawn early while streaming).
    pub fn call(&self, text: &str) -> String {
        let mut args = self.args();
        args.push(("text", text.to_string()));
        crate::ask::write_call(&args)
    }

    /// How full a progress bar is, 0 to 1.
    pub fn fraction(&self) -> f64 {
        match self {
            Show::Progress { value, max, .. } if *max > 0.0 => (value / max).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    /// A progress bar's words: `7 of 10 tests passed (70%)`.
    pub fn progress_text(&self) -> String {
        match self {
            Show::Progress { label, value, max } => {
                let pct = (self.fraction() * 100.0).round();
                let of = if *max == 100.0 { format!("{}%", number_text(*value)) } else { format!("{} of {} ({pct}%)", number_text(*value), number_text(*max)) };
                if label.is_empty() { of } else { format!("{of} {label}") }
            }
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calls::parse;

    fn read_one(s: &str) -> (Option<Show>, Vec<String>) {
        let mut notes = Vec::new();
        let show = read(&parse(s).unwrap().remove(0), &mut notes);
        (show, notes)
    }

    #[test]
    fn every_registered_example_reads_back_as_itself() {
        let all = instructions();
        for e in registry() {
            let call = e.show.call(e.text);
            assert_eq!(read_one(&call), (Some(e.show.clone()), vec![]), "{call}");
            assert!(all.contains(&format!("  {} ({}): {call}", e.show.name(), e.when)), "{all}");
        }
        assert_eq!(kinds(), ["table", "list", "progress", "stat"]);
        let list: Show = serde_json::from_str(r#"{"kind":"list","items":["a"]}"#).unwrap();
        assert_eq!(list, Show::List { caption: String::new(), items: vec!["a".into()] });
    }

    #[test]
    fn near_misses_read() {
        let (t, n) = read_one(r#"reply(text="x", show="Table", cols="Name|Age", rows="Ada|36;Bob|41")"#);
        assert!(matches!(t, Some(Show::Table { ref rows, .. }) if rows.len() == 2), "{t:?}");
        assert!(n.is_empty());
        let (l, _) = read_one("reply(text=\"x\", show=\"bullets\", data=\"a;b\nc\")");
        assert_eq!(l, Some(Show::List { caption: String::new(), items: vec!["a".into(), "b".into(), "c".into()] }));
        let (p, _) = read_one(r#"reply(text="x", show="bar", data="70% copied")"#);
        assert_eq!(p, Some(Show::Progress { label: "copied".into(), value: 70.0, max: 100.0 }));
        let (p, _) = read_one(r#"reply(text="x", show="progress", data="3 of 4 steps", caption="ignored")"#);
        assert_eq!(p.as_ref().map(Show::progress_text).as_deref(), Some("3 of 4 (75%) steps"));
        let (s, n) = read_one(r#"reply(text="x", show="kpi", data="Revenue: $3k;Users|9|+1")"#);
        let Some(Show::Stat { stats, .. }) = s else { panic!() };
        assert_eq!((stats[0].value.as_str(), stats[1].trend(), stats[0].trend()), ("$3k", Trend::Up, Trend::Flat));
        assert!(n.is_empty());
        assert_eq!(Stat { label: "a".into(), value: "1".into(), delta: "−2".into() }.trend(), Trend::Down);
    }

    #[test]
    fn broken_input_degrades_with_a_note() {
        let (s, n) = read_one(r#"reply(text="x", show="chart", data="1|2")"#);
        assert_eq!(s, None);
        assert!(n[0].contains("show=\"chart\" is not one of table, list, progress, stat"));
        let (s, n) = read_one(r#"reply(text="x", show="table")"#);
        assert_eq!(s, None);
        assert!(n[0].contains("needs data=, like data=\"Name|Age;Ada|36;Bob|41\""), "{n:?}");
        let (s, n) = read_one(r#"reply(text="x", show="table", data="Name|Age")"#);
        assert_eq!(s, None);
        assert!(n[0].contains("needs rows after its header"));
        let (s, n) = read_one(r#"reply(text="x", show="table", data="A|B;1;2|3|4")"#);
        assert_eq!(s, Some(Show::Table { caption: String::new(), head: vec!["A".into(), "B".into()], rows: vec![vec!["1".into(), "".into()], vec!["2".into(), "3".into()]] }));
        assert!(n[0].contains("needs 2 cells"));
        let (s, n) = read_one(r#"reply(text="x", show="progress", data="most of it")"#);
        assert_eq!(s, None);
        assert!(n[0].contains("is not like"));
        let (s, n) = read_one(r#"reply(text="x", show="progress", data="12/10")"#);
        assert_eq!(s, Some(Show::Progress { label: String::new(), value: 10.0, max: 10.0 }));
        assert!(n[0].contains("outside 0 to 10"));
        let (s, n) = read_one(r#"reply(text="x", show="stat", data="alone;A|1")"#);
        assert!(matches!(s, Some(Show::Stat { ref stats, .. }) if stats.len() == 1));
        assert!(n[0].contains("stat \"alone\""));
        let many = (0..40).map(|i| i.to_string()).collect::<Vec<_>>().join("|");
        let (s, n) = read_one(&format!("reply(text=\"x\", show=\"list\", data=\"{many}\")"));
        assert!(matches!(s, Some(Show::List { ref items, .. }) if items.len() == MAX_ROWS));
        assert_eq!(n.len(), 1);
    }
}
