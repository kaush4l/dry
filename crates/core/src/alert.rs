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
//! - `every 30m` (or `2h`, `1d`; at least a minute).
//!
//! `due` decides, for one look at the clock, with three rules from the old
//! project: baseline first (a moment at or before the alert was made never
//! fires, so making an alert never fires it at once), missed is missed (a
//! moment nobody looked at within `GRACE` seconds is skipped with a reason,
//! never fired late), and the fire message above.
//!
//! Pure: the time comes in as a `Clock`, whose offset says which wall clock
//! `at` times are on (the shell's; the core knows no time zones). Firing
//! (a tick that looks, and `dry alerts` while the window is closed) is not
//! built yet.

use std::collections::BTreeMap;
use std::fmt;

use crate::live::{self, Clock};
use crate::script::{check_name, header};

/// A moment fires only if someone looks within this many seconds of it.
pub const GRACE: i64 = 120;

/// What makes an alert fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// At a wall-clock time: `minute` past midnight on `days`.
    At { minute: u32, days: Days },
    /// Every so many seconds, counted from the last fire (or from when the
    /// alert was made).
    Every(i64),
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
        let usage = "a trigger is `at 09:35`, `at 09:35 weekdays`, `at 2026-11-16 09:35` or `every 30m` (m, h or d)";
        match words.as_slice() {
            ["at", t] => Ok(Trigger::At { minute: clock(t)?, days: Days::Daily }),
            ["at", t, "weekdays"] => Ok(Trigger::At { minute: clock(t)?, days: Days::Weekdays }),
            ["at", d, t] => Ok(Trigger::At { minute: clock(t)?, days: date(d)? }),
            ["every", n] => {
                let bad = || format!("`every {n}`: a number then m, h or d, at least 1m, e.g. every 30m");
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
                Ok(Trigger::Every(count * unit))
            }
            _ => Err(usage.into()),
        }
    }

    /// The latest moment (unix seconds) at or before `now` this trigger
    /// names, for an `every` counted from `baseline`. `None`: none yet.
    fn latest(self, baseline: i64, now: Clock) -> Option<i64> {
        match self {
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
    Ok(Alert { name, description: meta.get("description").cloned().unwrap_or_default(), trigger, once, message: body })
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

    /// Whether firing uses it up: `once: true`, or a dated `at`.
    pub fn spent_when_fired(&self) -> bool {
        self.once || matches!(self.trigger, Trigger::At { days: Days::Date(..), .. })
    }
}

/// Every alert of an engine, by name, read.
pub fn all(files: &BTreeMap<String, String>) -> Vec<(String, Result<Alert, String>)> {
    files.iter().map(|(name, text)| (name.clone(), parse(name, text))).collect()
}

/// The `kind: alerts` snapshot: one line each, or why it does not read.
pub fn snapshot(files: &BTreeMap<String, String>) -> String {
    if files.is_empty() {
        return "No alerts yet.".into();
    }
    let mut lines: Vec<String> = all(files)
        .into_iter()
        .map(|(name, read)| match read {
            Ok(a) => {
                let once = if a.spent_when_fired() { ", once" } else { "" };
                format!("- {name} ({}{once}): {}", a.trigger, if a.description.is_empty() { &a.message } else { &a.description })
            }
            Err(why) => format!("- {name}: broken: {why}"),
        })
        .collect();
    lines.push("(Firing is not built yet: these do not wake you.)".into());
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

    #[test]
    fn triggers_read_and_write_back_the_same() {
        for t in ["at 09:35", "at 09:35 weekdays", "at 2026-11-16 09:35", "every 30m", "every 2h", "every 1d"] {
            assert_eq!(Trigger::parse(t).unwrap().to_string(), t);
        }
        assert_eq!(Trigger::parse("every 90m").unwrap().to_string(), "every 90m");
        for bad in ["at 24:00", "at 9:35", "at 2026-02-30 09:00", "every 0m", "every 30s", "watch x", ""] {
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
        let s = snapshot(&files);
        assert!(s.starts_with("- bad: broken: alert bad: a trigger is"), "{s}");
        assert!(s.contains("- wake (at 2026-11-23 07:00, once): Morning"), "{s}");
        assert!(s.ends_with("these do not wake you.)"));
        assert_eq!(snapshot(&BTreeMap::new()), "No alerts yet.");
    }
}
