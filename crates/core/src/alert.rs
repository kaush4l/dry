//! Alerts: what wakes an engine at the right moment.
//!
//! ```text
//! spec/alerts/night-start.md
//! ---
//! name: night-start
//! description: Start the night shift at 22:00
//! trigger: at 22:00
//! once: false
//! ---
//! Night shift. Gather, pick, hand off.
//! ```
//!
//! The body is the message the engine wakes with:
//! `Alert night-start fired (at 22:00): Night shift. ...`.
//!
//! Triggers, for now:
//!
//! - `at 09:35` every day, `at 09:35 weekdays` Monday to Friday, or
//!   `at 2026-11-16 09:35` once on that date;
//! - `every 30m` (or `2h`, `1d`; at least a minute);
//! - `idle 5m`: nobody on the engine's thread has worked for that long (no
//!   turn running, no engine waiting on a child or a job, and the engine
//!   took no message meanwhile). It fires once per quiet stretch: the turn
//!   it starts ends the stretch, and the next begins when that work is over.
//!
//! `while: plan` in the header is a condition on any trigger: the alert
//! fires only while the artifact it names holds an open step, a line that
//! starts `- [ ]`. With `idle` it is how an engine with a plan keeps
//! itself going: it is woken while steps are open and nobody works, and is
//! silent once each step is ticked (`- [x]`), given up (`- [~]`) or waits
//! for the person (`- [?]`). The engine ends its own wake by editing its
//! plan; nothing counts steps.
//!
//! `due` decides, for one look at the clock, with three rules from the old
//! project: baseline first (a moment at or before the alert was made never
//! fires, so making an alert never fires it at once), missed is missed (a
//! moment nobody looked at within `GRACE` seconds is skipped with a reason,
//! never fired late), and the fire message above.
//!
//! **Firing** (`settle`): every shell already looks at each engine for its
//! callbacks every second or two (`crate::turn::callbacks`: a child that
//! replied, a command that ended). The alerts are looked at in the same
//! place, so one that is due wakes the engine exactly as those do: a
//! message, a new turn, with nobody writing to it. An engine in a turn is
//! not looked at; a moment that passes meanwhile is fired if the turn
//! ends within `GRACE`, else missed (and the snapshot says so).
//!
//! **Making one** (`call`): an alerts artifact (`kind: alerts`) brings the
//! tool `alert(name, trigger, text, once?)`, so an engine can set its own
//! next look: `alert(name="check-build", trigger="every 30m", text="Look at
//! the build; tell the person only if it broke.")`. The same name replaces
//! it; `text=""` removes it. A person makes one by writing the file.
//!
//! When each was first seen and last fired (`Mark`) is kept with the run
//! (`convo/run.md`), so a restart neither fires an old moment again nor
//! forgets one that is still within its grace.
//!
//! Pure: the time comes in as a `Clock`, whose offset says which wall clock
//! `at` times are on (the shell's; the core knows no time zones).

use std::collections::BTreeMap;
use std::fmt;

use crate::live::{self, Clock};
use crate::script::{check_name, header};

/// A moment fires only if someone looks within this many seconds of it.
pub const GRACE: i64 = 120;

/// The tool an alerts artifact brings.
pub const TOOLS: [&str; 1] = ["alert"];

/// Most alerts one engine keeps: each is a reason it wakes by itself, so
/// they stay few enough to read at a glance.
pub const MAX_PER_ENGINE: usize = 8;

/// What is remembered of one alert between looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    /// When it was first seen (the baseline: no moment at or before it fires).
    pub created: i64,
    /// The moment it last fired or was missed for.
    pub last: Option<i64>,
    /// How many times it fired.
    pub fired: u32,
    /// The last moment was missed (nothing looked within `GRACE`).
    pub missed: bool,
    /// For an `idle` trigger: since when its thread has been quiet, as far
    /// as the looks saw (`None`: someone worked at the last look).
    pub quiet: Option<i64>,
    /// The holder's message count at the last look: a change since means
    /// it worked in between, so the quiet starts again.
    pub seen: usize,
}

impl Mark {
    /// `<created> <last or -> <fired> <missed 0|1> <quiet or -> <seen>`:
    /// its line in `run.md` (the last two may be missing: an older store).
    pub fn to_line(&self) -> String {
        let or_dash = |t: Option<i64>| t.map_or("-".to_string(), |l| l.to_string());
        format!("{} {} {} {} {} {}", self.created, or_dash(self.last), self.fired, u8::from(self.missed), or_dash(self.quiet), self.seen)
    }

    pub fn from_line(line: &str) -> Option<Mark> {
        let mut w = line.split_whitespace();
        let created = w.next()?.parse().ok()?;
        let last = match w.next()? {
            "-" => None,
            l => Some(l.parse().ok()?),
        };
        let (fired, missed) = (w.next()?.parse().ok()?, w.next()? == "1");
        let quiet = w.next().and_then(|q| q.parse().ok());
        Some(Mark { created, last, fired, missed, quiet, seen: w.next().and_then(|n| n.parse().ok()).unwrap_or(0) })
    }
}

/// One look at the clock for an engine's alerts: the messages of those
/// that fire now, in name order. An alert seen for the first time gets
/// its baseline and does not fire; one that fires is marked, and removed
/// if it is spent (`once`, or a dated `at`); a missed moment is marked so
/// it is said once and never fired late. Marks of alerts that are gone
/// are dropped. A file that does not read is left alone (the snapshot
/// says why).
pub fn settle(files: &mut BTreeMap<String, String>, marks: &mut BTreeMap<String, Mark>, now: Clock) -> Vec<String> {
    settle_with(files, marks, now, &Look::default())
}

/// What an `idle` trigger and a `while:` condition look at, from the shell
/// and the engine (`crate::turn::callbacks`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Look<'a> {
    /// Someone on the engine's thread works now (a turn runs, or an
    /// engine waits on a child or a job).
    pub working: bool,
    /// The engine's message count: it grows when it takes a message or
    /// answers one.
    pub messages: usize,
    /// The engine's artifacts, for `while:`. `None`: no condition holds
    /// back (a caller with no engine at hand).
    pub artifacts: Option<&'a BTreeMap<String, String>>,
}

/// `settle`, with what `idle` and `while:` need. A due alert whose
/// `while:` does not hold stays silent: a timed one's moment is passed
/// over, an `idle` one stays armed and fires as soon as it holds.
pub fn settle_with(files: &mut BTreeMap<String, String>, marks: &mut BTreeMap<String, Mark>, now: Clock, look: &Look) -> Vec<String> {
    marks.retain(|name, _| files.contains_key(name));
    let mut out = Vec::new();
    let mut spent = Vec::new();
    for (name, read) in all(files) {
        let Ok(alert) = read else { continue };
        let mark = marks.entry(name.clone()).or_insert(Mark { created: now.unix, ..Mark::default() });
        // The quiet stretch as this look sees it: over while someone works,
        // begun again when the holder itself worked since the last look.
        if matches!(alert.trigger, Trigger::Idle(_)) {
            if look.working {
                mark.quiet = None;
            } else if mark.quiet.is_none() || mark.seen != look.messages {
                mark.quiet = Some(now.unix);
            }
            mark.seen = look.messages;
        }
        match alert.due_quiet(mark.last, mark.created, now, mark.quiet) {
            Due::Fire { at, .. } if look.artifacts.is_some_and(|a| !alert.holds(a)) => {
                if !matches!(alert.trigger, Trigger::Idle(_)) {
                    mark.last = Some(at);
                }
            }
            Due::Fire { at, message } => {
                *mark = Mark { last: Some(at), fired: mark.fired + 1, missed: false, ..*mark };
                out.push(message);
                if alert.spent_when_fired() {
                    spent.push(name);
                }
            }
            Due::Skip { at, .. } => *mark = Mark { last: Some(at), missed: true, ..*mark },
            Due::No => {}
        }
    }
    for name in spent {
        files.remove(&name);
        marks.remove(&name);
    }
    out
}

/// The open steps of a plan-like text: its lines that start `- [ ]`.
pub fn open_steps(text: &str) -> usize {
    text.lines().filter(|l| l.trim_start().starts_with("- [ ]")).count()
}

/// `alert(name, trigger, text, once?)`: make an alert, replace the one of
/// that name, or with `text=""` remove it. A new or replaced one starts
/// from now (its old mark is dropped), so it never fires at once.
pub fn call(files: &mut BTreeMap<String, String>, marks: &mut BTreeMap<String, Mark>, call: &crate::calls::Call, now: i64) -> Result<String, String> {
    let name = call.arg("name").map(str::trim).filter(|n| !n.is_empty()).ok_or("`alert` needs name=\"...\"")?.to_string();
    let text = call.arg("text").map(str::trim).unwrap_or("");
    if text.is_empty() {
        marks.remove(&name);
        return match files.remove(&name) {
            Some(_) => Ok(format!("removed alert {name}")),
            None => Err(format!("no alert `{name}` to remove; to make one give trigger=\"...\" and text=\"...\"")),
        };
    }
    let trigger = call.arg("trigger").map(str::trim).unwrap_or("");
    let once = match call.arg("once").map(str::trim) {
        None | Some("" | "false" | "no") => false,
        Some("true" | "yes") => true,
        Some(v) => return Err(format!("`once={v}`: true or false")),
    };
    // One line of description (the snapshot shows it), the whole text as the message.
    let first = text.lines().next().unwrap_or("").trim();
    let whilst = call.arg("while").map(str::trim).filter(|w| !w.is_empty()).map_or(String::new(), |w| format!("while: {w}\n"));
    let file = format!("---\nname: {name}\ndescription: {first}\ntrigger: {trigger}\nonce: {once}\n{whilst}---\n{text}\n");
    let alert = parse(&name, &file)?;
    let replaced = files.contains_key(&name);
    if !replaced && files.len() >= MAX_PER_ENGINE {
        return Err(format!("you have {MAX_PER_ENGINE} alerts, the most one engine keeps: remove one first (alert(name=\"...\", text=\"\"))"));
    }
    files.insert(name.clone(), file);
    marks.insert(name.clone(), Mark { created: now, ..Mark::default() });
    Ok(format!(
        "{} alert {name}: {}{}. It wakes you with its text; nothing fires at the moment it is made.",
        if replaced { "replaced" } else { "made" },
        alert.trigger,
        if alert.spent_when_fired() { ", once" } else { "" }
    ))
}

/// What makes an alert fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// At a wall-clock time: `minute` past midnight on `days`.
    At { minute: u32, days: Days },
    /// Every so many seconds, counted from the last fire (or from when the
    /// alert was made).
    Every(i64),
    /// When nobody on the engine's thread has worked for so many seconds.
    Idle(i64),
}

/// Which days an `at` fires on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Days {
    Daily,
    /// Monday to Friday.
    Weekdays,
    /// One date `(year, month, day)`: fired once, then spent.
    Date(i64, u32, u32),
}

impl Trigger {
    /// `at 09:35`, `at 09:35 weekdays`, `at 2026-11-16 09:35`, `every 30m`.
    pub fn parse(text: &str) -> Result<Trigger, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let usage = "a trigger is `at 09:35`, `at 09:35 weekdays`, `at 2026-11-16 09:35`, `every 30m` or `idle 5m` (m, h or d)";
        match words.as_slice() {
            ["at", t] => Ok(Trigger::At { minute: clock(t)?, days: Days::Daily }),
            ["at", t, "weekdays"] => Ok(Trigger::At { minute: clock(t)?, days: Days::Weekdays }),
            ["at", d, t] => Ok(Trigger::At { minute: clock(t)?, days: date(d)? }),
            [word @ ("every" | "idle"), n] => {
                let bad = || format!("`{word} {n}`: a number then m, h or d, at least 1m, e.g. {word} 30m");
                let unit = match n.chars().last() {
                    Some('m') => 60,
                    Some('h') => 3_600,
                    Some('d') => 86_400,
                    _ => return Err(bad()),
                };
                let count: i64 = n[..n.len() - 1].parse().map_err(|_| bad())?;
                if count < 1 {
                    return Err(bad());
                }
                Ok(if *word == "idle" { Trigger::Idle(count * unit) } else { Trigger::Every(count * unit) })
            }
            _ => Err(usage.into()),
        }
    }

    /// The latest moment (unix seconds) at or before `now` this trigger
    /// names, for an `every` counted from `baseline`. `None`: none yet.
    fn latest(self, baseline: i64, now: Clock) -> Option<i64> {
        match self {
            // A condition, not a moment on the clock: `Alert::due_quiet`.
            Trigger::Idle(_) => None,
            Trigger::Every(secs) => {
                let n = (now.unix - baseline).div_euclid(secs);
                (n > 0).then(|| baseline + n * secs)
            }
            Trigger::At { minute, days: Days::Date(y, m, d) } => {
                let at = now.at(live::days_from_civil(y, m, d), minute);
                (at <= now.unix).then_some(at)
            }
            Trigger::At { minute, days } => (0..8)
                .map(|back| now.day() - back)
                .filter(|day| days != Days::Weekdays || !live::is_weekend(*day))
                .map(|day| now.at(day, minute))
                .find(|at| *at <= now.unix),
        }
    }
}

/// `09:35` → minutes since midnight.
fn clock(t: &str) -> Result<u32, String> {
    let bad = || format!("`{t}` is not a time like 09:35 (00:00 to 23:59)");
    let (h, m) = t.split_once(':').filter(|(h, m)| h.len() == 2 && m.len() == 2).ok_or_else(bad)?;
    match (h.parse::<u32>(), m.parse::<u32>()) {
        (Ok(h), Ok(m)) if h < 24 && m < 60 => Ok(h * 60 + m),
        _ => Err(bad()),
    }
}

/// `2026-11-16` → that date, checked (no 2026-02-30).
fn date(d: &str) -> Result<Days, String> {
    let bad = || format!("`{d}` is not a date like 2026-11-16");
    let parts: Vec<&str> = d.split('-').collect();
    let [y, m, day] = parts[..] else { return Err(bad()) };
    let (y, m, day): (i64, u32, u32) = (y.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?, day.parse().map_err(|_| bad())?);
    if !(1970..=9999).contains(&y) || !(1..=12).contains(&m) || live::civil_from_days(live::days_from_civil(y, m, day)) != (y, m, day) {
        return Err(bad());
    }
    Ok(Days::Date(y, m, day))
}

impl fmt::Display for Trigger {
    /// Written back as it is parsed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Trigger::At { minute, days } => {
                let t = format!("{:02}:{:02}", minute / 60, minute % 60);
                match days {
                    Days::Daily => write!(f, "at {t}"),
                    Days::Weekdays => write!(f, "at {t} weekdays"),
                    Days::Date(y, m, d) => write!(f, "at {y:04}-{m:02}-{d:02} {t}"),
                }
            }
            Trigger::Every(secs) if secs % 86_400 == 0 => write!(f, "every {}d", secs / 86_400),
            Trigger::Every(secs) if secs % 3_600 == 0 => write!(f, "every {}h", secs / 3_600),
            Trigger::Every(secs) => write!(f, "every {}m", secs / 60),
            Trigger::Idle(secs) if secs % 86_400 == 0 => write!(f, "idle {}d", secs / 86_400),
            Trigger::Idle(secs) if secs % 3_600 == 0 => write!(f, "idle {}h", secs / 3_600),
            Trigger::Idle(secs) => write!(f, "idle {}m", secs / 60),
        }
    }
}

/// An alert, read from its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub name: String,
    pub description: String,
    pub trigger: Trigger,
    /// `once: true`: removed after it fires.
    pub once: bool,
    /// What the engine wakes with.
    pub message: String,
    /// `while: <artifact>`: it fires only while that artifact holds an
    /// open step (a line that starts `- [ ]`). `None`: always.
    pub whilst: Option<String>,
}

/// What one look at the clock says about an alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Due {
    /// Wake the engine with `message`; `at` is the moment it was for.
    Fire { at: i64, message: String },
    /// The moment `at` came while nobody looked: say so once, never fire.
    Skip { at: i64, why: String },
    /// Nothing to do.
    No,
}

/// Read `spec/alerts/<file_name>.md`.
pub fn parse(file_name: &str, text: &str) -> Result<Alert, String> {
    let (meta, body) = header(text).map_err(|why| format!("alert {file_name}: {why}"))?;
    let name = check_name("alert", file_name, &meta)?;
    let trigger = Trigger::parse(meta.get("trigger").map(String::as_str).unwrap_or("")).map_err(|why| format!("alert {name}: {why}"))?;
    let once = match meta.get("once").map(String::as_str) {
        None | Some("false" | "no") => false,
        Some("true" | "yes") => true,
        Some(v) => return Err(format!("alert {name}: `once: {v}`: true or false")),
    };
    if body.is_empty() {
        return Err(format!("alert {name}: needs a message (the body): what to do when it fires"));
    }
    let whilst = meta.get("while").map(|w| w.trim().to_string()).filter(|w| !w.is_empty());
    Ok(Alert { name, description: meta.get("description").cloned().unwrap_or_default(), trigger, once, message: body, whilst })
}

impl Alert {
    /// Should it fire `now`? `last_fired`: when it last fired; `created`:
    /// when it was made (or first seen), the baseline.
    pub fn due(&self, last_fired: Option<i64>, created: i64, now: Clock) -> Due {
        let baseline = last_fired.unwrap_or(created).max(created);
        let Some(at) = self.trigger.latest(baseline, now) else { return Due::No };
        if at <= baseline {
            return Due::No;
        }
        if now.unix - at > GRACE {
            let when = Clock::new(at, now.offset);
            return Due::Skip { at, why: format!("missed {when}: nothing was looking then") };
        }
        Due::Fire { at, message: format!("Alert {} fired ({}): {}", self.name, self.trigger, self.message) }
    }

    /// `due`, with what an `idle` trigger looks at: `quiet`, since when
    /// nobody on the engine's thread has worked (`None`: someone works
    /// now). An `idle 5m` is due once the thread has been quiet for five
    /// minutes, counted from the later of the quiet's start and the day
    /// the alert was made, and once per quiet stretch: a fire at or after
    /// the stretch began is that stretch's.
    pub fn due_quiet(&self, last_fired: Option<i64>, created: i64, now: Clock, quiet: Option<i64>) -> Due {
        let Trigger::Idle(secs) = self.trigger else { return self.due(last_fired, created, now) };
        let Some(since) = quiet else { return Due::No };
        let start = since.max(created);
        if last_fired.is_some_and(|l| l >= since) || now.unix < start + secs {
            return Due::No;
        }
        Due::Fire { at: now.unix, message: format!("Alert {} fired ({}): {}", self.name, self.trigger, self.message) }
    }

    /// Its `while:` condition against the engine's artifacts: the named
    /// artifact holds an open step. No condition: it holds.
    pub fn holds(&self, artifacts: &BTreeMap<String, String>) -> bool {
        match &self.whilst {
            None => true,
            Some(name) => artifacts.get(name).is_some_and(|t| open_steps(t) > 0),
        }
    }

    /// Whether firing uses it up: `once: true`, or a dated `at`.
    pub fn spent_when_fired(&self) -> bool {
        self.once || matches!(self.trigger, Trigger::At { days: Days::Date(..), .. })
    }
}

/// Every alert of an engine, by name, read.
pub fn all(files: &BTreeMap<String, String>) -> Vec<(String, Result<Alert, String>)> {
    files.iter().map(|(name, text)| (name.clone(), parse(name, text))).collect()
}

/// The `kind: alerts` snapshot: one line each (its trigger, what it is
/// for, how often it has fired, a missed moment), or why it does not read.
pub fn snapshot(files: &BTreeMap<String, String>, marks: &BTreeMap<String, Mark>) -> String {
    if files.is_empty() {
        return "No alerts yet: alert(name=\"...\", trigger=\"every 30m\", text=\"...\") makes one that wakes you by itself.".into();
    }
    let lines: Vec<String> = all(files)
        .into_iter()
        .map(|(name, read)| match read {
            Ok(a) => {
                let once = if a.spent_when_fired() { ", once" } else { "" };
                let state = match marks.get(&name) {
                    Some(m) if m.missed => format!(" · fired {} time(s); the last moment was missed (you were busy or the app was closed)", m.fired),
                    Some(m) if m.fired > 0 => format!(" · fired {} time(s)", m.fired),
                    _ => " · not fired yet".to_string(),
                };
                let whilst = a.whilst.as_ref().map_or(String::new(), |w| format!(", while `{w}` has an open `- [ ]` step"));
                format!("- {name} ({}{once}{whilst}): {}{state}", a.trigger, if a.description.is_empty() { &a.message } else { &a.description })
            }
            Err(why) => format!("- {name}: broken: {why}"),
        })
        .collect();
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-22 14:05:30 UTC, a Sunday.
    const SUNDAY: i64 = 1_795_356_330;
    const MIN: i64 = 60;
    const DAY: i64 = 86_400;

    fn alert(trigger: &str) -> Alert {
        parse("wake", &format!("---\nname: wake\ndescription: d\ntrigger: {trigger}\n---\nCheck the inbox.")).unwrap()
    }

    /// The UTC clock at `secs` after SUNDAY.
    fn at(secs: i64) -> Clock {
        Clock::new(SUNDAY + secs, 0)
    }

    fn c(text: &str) -> crate::calls::Call {
        crate::calls::parse(text).unwrap().remove(0)
    }

    #[test]
    fn an_alert_that_is_due_wakes_its_engine_once_and_a_spent_one_is_removed() {
        let mut files = BTreeMap::from([("wake".to_string(), "---\nname: wake\ndescription: d\ntrigger: every 30m\n---\nCheck the inbox.".to_string())]);
        let mut marks = BTreeMap::new();
        // First seen: its baseline, nothing fires.
        assert!(settle(&mut files, &mut marks, at(0)).is_empty());
        assert_eq!(marks["wake"], Mark { created: SUNDAY, ..Mark::default() });
        assert!(settle(&mut files, &mut marks, at(29 * MIN)).is_empty());
        // Due: the message, once; the next look at the same moment says nothing.
        assert_eq!(settle(&mut files, &mut marks, at(30 * MIN)), ["Alert wake fired (every 30m): Check the inbox."]);
        assert!(settle(&mut files, &mut marks, at(30 * MIN + 5)).is_empty());
        assert_eq!((marks["wake"].fired, marks["wake"].last), (1, Some(SUNDAY + 30 * MIN)));
        // Nobody looked for hours: the moment is missed, said in the snapshot, never fired late.
        assert!(settle(&mut files, &mut marks, at(5 * 60 * MIN + 10 * MIN)).is_empty());
        assert!(marks["wake"].missed && snapshot(&files, &marks).contains("the last moment was missed"), "{}", snapshot(&files, &marks));
        // And it goes on from there.
        assert_eq!(settle(&mut files, &mut marks, at(5 * 60 * MIN + 30 * MIN)).len(), 1);
        assert!(snapshot(&files, &marks).contains("- wake (every 30m): d · fired 2 time(s)"), "{}", snapshot(&files, &marks));
        // The marks read back from their line in run.md.
        assert_eq!(Mark::from_line(&marks["wake"].to_line()), Some(marks["wake"]));
        // A once alert fires and is gone, with its mark; a file that is gone takes its mark with it.
        files.insert("soon".into(), "---\nname: soon\ntrigger: every 1m\nonce: true\n---\nLook once.".into());
        let t = 6 * 60 * MIN;
        assert!(settle(&mut files, &mut marks, at(t)).iter().all(|m| !m.contains("soon")), "first seen: not it");
        assert!(settle(&mut files, &mut marks, at(t + MIN)).contains(&"Alert soon fired (every 1m): Look once.".to_string()));
        assert!(!files.contains_key("soon") && !marks.contains_key("soon"));
        files.clear();
        settle(&mut files, &mut marks, at(t + 2 * MIN));
        assert!(marks.is_empty());
    }

    #[test]
    fn an_engine_makes_replaces_and_removes_its_own_alerts() {
        let (mut files, mut marks) = (BTreeMap::new(), BTreeMap::new());
        let made = call(&mut files, &mut marks, &c("alert(name=\"check-build\", trigger=\"every 30m\", text=\"Look at the build.\nTell the person only if it broke.\")"), SUNDAY).unwrap();
        assert_eq!(made, "made alert check-build: every 30m. It wakes you with its text; nothing fires at the moment it is made.");
        let a = parse("check-build", &files["check-build"]).unwrap();
        assert_eq!((a.description.as_str(), a.message.as_str()), ("Look at the build.", "Look at the build.\nTell the person only if it broke."));
        // Made now, so nothing fires now; 30 minutes on it does.
        assert!(settle(&mut files, &mut marks, at(0)).is_empty());
        assert_eq!(settle(&mut files, &mut marks, at(30 * MIN)).len(), 1);
        // Replaced: counted from the new moment; once is read.
        let again = call(&mut files, &mut marks, &c("alert(name=\"check-build\", trigger=\"at 2026-11-23 09:00\", text=\"One look.\", once=\"true\")"), SUNDAY + 31 * MIN).unwrap();
        assert!(again.starts_with("replaced alert check-build: at 2026-11-23 09:00, once."), "{again}");
        assert_eq!(marks["check-build"].fired, 0);
        // Mistakes say what to write; removing takes the mark too.
        assert!(call(&mut files, &mut marks, &c("alert(name=\"x\", trigger=\"soon\", text=\"t\")"), SUNDAY).unwrap_err().contains("a trigger is `at 09:35`"));
        assert!(call(&mut files, &mut marks, &c("alert(trigger=\"every 1m\", text=\"t\")"), SUNDAY).unwrap_err().contains("needs name"));
        assert!(call(&mut files, &mut marks, &c("alert(name=\"nope\", text=\"\")"), SUNDAY).unwrap_err().contains("no alert `nope`"));
        assert_eq!(call(&mut files, &mut marks, &c("alert(name=\"check-build\", text=\"\")"), SUNDAY).unwrap(), "removed alert check-build");
        assert!(files.is_empty() && marks.is_empty());
        // No more than a few: each is a reason to wake.
        for n in 0..MAX_PER_ENGINE {
            call(&mut files, &mut marks, &c(&format!("alert(name=\"a{n}\", trigger=\"every 1h\", text=\"t\")")), SUNDAY).unwrap();
        }
        assert!(call(&mut files, &mut marks, &c("alert(name=\"one-more\", trigger=\"every 1h\", text=\"t\")"), SUNDAY).unwrap_err().contains("the most one engine keeps"));
    }

    #[test]
    fn triggers_read_and_write_back_the_same() {
        for t in ["at 09:35", "at 09:35 weekdays", "at 2026-11-16 09:35", "every 30m", "every 2h", "every 1d", "idle 5m", "idle 1h"] {
            assert_eq!(Trigger::parse(t).unwrap().to_string(), t);
        }
        assert_eq!(Trigger::parse("every 90m").unwrap().to_string(), "every 90m");
        for bad in ["at 24:00", "at 9:35", "at 2026-02-30 09:00", "every 0m", "every 30s", "idle 0m", "idle soon", "watch x", ""] {
            assert!(Trigger::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_alert_file_has_a_trigger_a_message_and_maybe_once() {
        let a = parse("night-start", "---\nname: night-start\ndescription: Start\ntrigger: at 22:00\nonce: true\n---\nNight shift.").unwrap();
        assert_eq!((a.trigger.to_string(), a.once, a.message.as_str()), ("at 22:00".into(), true, "Night shift."));
        assert!(parse("x", "---\nname: x\ntrigger: at 22:00\n---\n").unwrap_err().contains("needs a message"));
        assert!(parse("x", "---\nname: x\ntrigger: at 22:00\nonce: maybe\n---\nGo").unwrap_err().contains("true or false"));
        assert!(alert("at 2026-11-16 09:35").spent_when_fired() && !alert("at 09:35").spent_when_fired());
    }

    #[test]
    fn making_an_alert_never_fires_it_at_once() {
        // 14:05 now; made at 14:05:30, the 14:05 moment is before it.
        assert_eq!(alert("at 14:05").due(None, SUNDAY, at(10)), Due::No);
        // An `every` counts from when it was made.
        let every = alert("every 30m");
        assert_eq!(every.due(None, SUNDAY, at(29 * MIN)), Due::No);
        assert!(matches!(every.due(None, SUNDAY, at(30 * MIN)), Due::Fire { at, .. } if at == SUNDAY + 30 * MIN));
    }

    #[test]
    fn a_daily_time_fires_once_and_says_why() {
        let a = alert("at 14:10");
        let made = SUNDAY - 60 * MIN;
        let ten_past = SUNDAY + 4 * MIN + 30;
        assert_eq!(a.due(None, made, at(4 * MIN)), Due::No, "14:09:30: not yet");
        let fire = a.due(None, made, at(5 * MIN));
        assert_eq!(fire, Due::Fire { at: ten_past, message: "Alert wake fired (at 14:10): Check the inbox.".into() });
        assert_eq!(a.due(Some(ten_past + 30), made, at(6 * MIN)), Due::No, "it fired");
    }

    #[test]
    fn missed_is_missed_skipped_with_a_reason() {
        let a = alert("at 14:00");
        // Nobody looked from 13:00 until 14:05:30: the 14:00 moment is old.
        let made = SUNDAY - 2 * DAY;
        let due = a.due(Some(SUNDAY - DAY), made, at(0));
        assert_eq!(due, Due::Skip { at: SUNDAY - 5 * MIN - 30, why: "missed 2026-11-22 Sunday 14:00 UTC: nothing was looking then".into() });
        // Asleep for three periods of an `every`: one skip, never three fires.
        let every = alert("every 10m");
        assert!(matches!(every.due(None, SUNDAY, at(35 * MIN)), Due::Skip { at, .. } if at == SUNDAY + 30 * MIN));
    }

    #[test]
    fn weekdays_skip_the_weekend_and_a_date_fires_on_its_day_only() {
        let a = alert("at 14:05 weekdays");
        // Sunday 14:05:30: the latest weekday moment is Friday's, long missed.
        assert!(matches!(a.due(Some(SUNDAY - 3 * DAY), SUNDAY - 9 * DAY, at(0)), Due::Skip { at, .. } if at == SUNDAY - 2 * DAY - 30));
        // Monday 14:05:30: fires.
        assert!(matches!(a.due(Some(SUNDAY - 2 * DAY), SUNDAY - 9 * DAY, at(DAY)), Due::Fire { .. }));
        let dated = alert("at 2026-11-23 09:00");
        assert_eq!(dated.due(None, SUNDAY, at(0)), Due::No, "not yet");
        let nine = SUNDAY + DAY - 5 * 3600 - 5 * MIN - 30;
        assert!(matches!(dated.due(None, SUNDAY, Clock::new(nine + 10, 0)), Due::Fire { at, .. } if at == nine));
        assert_eq!(dated.due(Some(nine + 10), SUNDAY, at(30 * DAY)), Due::No, "spent");
    }

    #[test]
    fn at_times_are_on_the_clock_the_shell_passes() {
        // 09:05 at UTC-05:00 is 14:05 UTC.
        let a = alert("at 09:05");
        let local = Clock::new(SUNDAY + 10, -5 * 3600);
        assert!(matches!(a.due(None, SUNDAY - DAY, local), Due::Fire { at, .. } if at == SUNDAY - 30));
    }

    #[test]
    fn the_snapshot_lists_each_alert_on_one_line() {
        let mut files = BTreeMap::new();
        files.insert("wake".to_string(), "---\nname: wake\ndescription: Morning\ntrigger: at 2026-11-23 07:00\n---\nUp.".to_string());
        files.insert("bad".to_string(), "---\nname: bad\ntrigger: sometimes\n---\nx".to_string());
        let s = snapshot(&files, &BTreeMap::new());
        assert!(s.starts_with("- bad: broken: alert bad: a trigger is"), "{s}");
        assert!(s.ends_with("- wake (at 2026-11-23 07:00, once): Morning · not fired yet"), "{s}");
        assert!(snapshot(&BTreeMap::new(), &BTreeMap::new()).starts_with("No alerts yet: alert(name="), "an empty one says how to make one");
    }

    #[test]
    fn an_idle_alert_fires_once_per_quiet_stretch_and_only_while_its_plan_has_an_open_step() {
        let mut files = BTreeMap::from([(
            "keep-going".to_string(),
            "---\nname: keep-going\ndescription: go on\ntrigger: idle 5m\nwhile: plan\n---\nYour plan has open steps.".to_string(),
        )]);
        let mut marks = BTreeMap::new();
        let mut arts = BTreeMap::from([("plan".to_string(), "## Plan\n- [x] research\n- [ ] build the first character".to_string())]);
        let look = |working: bool, messages: usize, arts: &BTreeMap<String, String>, files: &mut BTreeMap<String, String>, marks: &mut BTreeMap<String, Mark>, t: i64| {
            settle_with(files, marks, at(t), &Look { working, messages, artifacts: Some(arts) })
        };
        // First seen: baselined, the quiet counted from this look.
        assert!(look(false, 4, &arts, &mut files, &mut marks, 0).is_empty());
        // Quiet for less than five minutes: nothing. Someone works: nothing, and the quiet is over.
        assert!(look(false, 4, &arts, &mut files, &mut marks, 4 * MIN).is_empty());
        assert!(look(true, 4, &arts, &mut files, &mut marks, 9 * MIN).is_empty());
        assert_eq!(marks["keep-going"].quiet, None);
        // Quiet again from 10 min: it fires at 15, once.
        assert!(look(false, 4, &arts, &mut files, &mut marks, 10 * MIN).is_empty());
        assert!(look(false, 4, &arts, &mut files, &mut marks, 14 * MIN).is_empty());
        let fired = look(false, 4, &arts, &mut files, &mut marks, 15 * MIN);
        assert_eq!(fired, ["Alert keep-going fired (idle 5m): Your plan has open steps."]);
        // The same quiet stretch never fires twice (the engine could not start, say).
        assert!(look(false, 4, &arts, &mut files, &mut marks, 40 * MIN).is_empty());
        // The holder took a turn (its messages grew): the quiet starts at this look, and fires five minutes on.
        assert!(look(false, 7, &arts, &mut files, &mut marks, 50 * MIN).is_empty());
        assert!(look(false, 7, &arts, &mut files, &mut marks, 54 * MIN).is_empty());
        assert_eq!(look(false, 7, &arts, &mut files, &mut marks, 55 * MIN).len(), 1);
        // Every step ticked, given up or waiting for the person: silent, and armed.
        arts.insert("plan".into(), "- [x] research\n- [~] the fifth\n- [?] which voice".into());
        assert!(look(false, 9, &arts, &mut files, &mut marks, 60 * MIN).is_empty());
        assert!(look(false, 9, &arts, &mut files, &mut marks, 90 * MIN).is_empty());
        // A step opened again in the same quiet stretch: it fires at the next look.
        arts.insert("plan".into(), "- [ ] one more".into());
        assert_eq!(look(false, 9, &arts, &mut files, &mut marks, 91 * MIN).len(), 1);
        assert_eq!(marks["keep-going"].fired, 3);
        // The marks, the quiet with them, read back from their line (a restart keeps counting).
        let m = marks["keep-going"];
        assert_eq!(Mark::from_line(&m.to_line()), Some(m));
        assert_eq!(Mark::from_line("100 - 0 0"), Some(Mark { created: 100, ..Mark::default() }));
    }

    #[test]
    fn a_timed_alert_whose_while_does_not_hold_passes_its_moment_over() {
        let mut files = BTreeMap::from([("wake".to_string(), "---\nname: wake\ntrigger: every 30m\nwhile: plan\n---\nLook at the runs.".to_string())]);
        let mut marks = BTreeMap::new();
        let done = BTreeMap::from([("plan".to_string(), "- [x] all".to_string())]);
        let open = BTreeMap::from([("plan".to_string(), "- [ ] one".to_string())]);
        let look = |arts| Look { working: true, messages: 1, artifacts: Some(arts) };
        assert!(settle_with(&mut files, &mut marks, at(0), &look(&done)).is_empty());
        assert!(settle_with(&mut files, &mut marks, at(30 * MIN), &look(&done)).is_empty());
        assert_eq!(marks["wake"].fired, 0);
        // The moment at 30 is gone; the next one, with an open step, fires (working or not).
        assert!(settle_with(&mut files, &mut marks, at(31 * MIN), &look(&open)).is_empty());
        assert_eq!(settle_with(&mut files, &mut marks, at(60 * MIN), &look(&open)).len(), 1);
        // An engine makes one with a condition through the tool.
        let c = crate::calls::parse("alert(name=\"go\", trigger=\"idle 5m\", while=\"plan\", text=\"Go on with the plan.\")").unwrap().remove(0);
        call(&mut files, &mut marks, &c, SUNDAY).unwrap();
        let made = parse("go", &files["go"]).unwrap();
        assert_eq!((made.trigger, made.whilst.as_deref()), (Trigger::Idle(300), Some("plan")));
    }
}
