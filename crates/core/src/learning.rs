//! `memory/learning.md`: what an engine has learned, in three fixed
//! sections, kept current through curated tools (`kind: learning`).
//!
//! ```markdown
//! ## user
//! - Prefers short answers.
//!
//! ## environment
//! - `cargo test -q` passes here in about 40 s.
//!
//! ## lessons
//! - Run cargo check before cargo test here. (n=2, 11-22)
//! ```
//!
//! | section | holds | changes when | cap |
//! |---|---|---|---|
//! | `user` | lasting preferences and decisions the person stated | the person says it | 1200 |
//! | `environment` | facts a tool confirmed: paths, versions, commands, limits | a tool showed it; replaced when a tool shows otherwise | 1600 |
//! | `lessons` | rules for next time: the wrong call and the right way | an outcome is clear | 1600 |
//!
//! It lives in `memory/` (local and personal, never in the shareable
//! `spec/`), is sent in the Learning slot (the second, after the goal),
//! and is never compressed: it is capped instead. A full section refuses
//! an entry rather than cutting one; the engine merges two (`old=`) or
//! removes one first. Every change is a line in `memory/learning.log`.
//!
//! **Curated tools**, not a pass over the whole history (owner,
//! 2026-11-22): `collect(what)` gathers only the context one section
//! learns from, from the engine's own history, into the learning
//! artifact's snapshot:
//!
//! | `what` | collects | feeds |
//! |---|---|---|
//! | `corrections` | what the person said right after one of your replies, with the reply it answered | `user` |
//! | `fixes` | a call that failed, paired with the later call of the same tool that worked | `lessons` |
//! | `facts` | results of tools that look at the world (terminal, files, MCP, child engines) | `environment` |
//!
//! Then `learn(section, entry, old?)` writes one entry. Results are one
//! line; what was collected is only in the snapshot (the snapshot rule).
//!
//! Pure: texts in, texts out.

use crate::inference::{Message, Role};
use crate::state::Engine;

/// The file, in the engine's folder.
pub const FILE: &str = "memory/learning.md";
pub const LOG: &str = "memory/learning.log";

/// `(name, what it holds and when it changes, cap in characters)`.
pub const SECTIONS: [(&str, &str, usize); 3] = [
    ("user", "what the person stated they want; change only when they say it", 1200),
    ("environment", "facts a tool confirmed; replace one when a tool shows otherwise", 1600),
    ("lessons", "rules for next time, once an outcome is clear", 1600),
];

/// Most characters of one entry (one line).
pub const MAX_ENTRY: usize = 300;

/// The tools the learning artifact brings.
/// `analytics(query)` (`crate::analytics::tool`): what the engine's own
/// spans say about the last week, facts into the snapshot.
pub const TOOLS: [&str; 3] = ["collect", "learn", "analytics"];

/// What `collect` can gather, and the section each feeds.
pub const COLLECT: [(&str, &str, &str); 3] = [
    ("corrections", "what the person said right after one of your replies", "user"),
    ("fixes", "failed calls paired with the call that later worked", "lessons"),
    ("facts", "results of tools that looked at the world", "environment"),
];

/// Most items one collection shows, and characters per part of one.
pub const COLLECT_ITEMS: usize = 8;
pub const COLLECT_CHARS: usize = 240;

/// The learning file, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Learning {
    /// Entries per section, in `SECTIONS` order, without the `- `.
    pub entries: [Vec<String>; 3],
    /// Sections the file has that are none of the three, kept as written
    /// (and said in the snapshot), so nothing a person wrote is lost.
    pub other: String,
}

fn index(section: &str) -> Result<usize, String> {
    SECTIONS
        .iter()
        .position(|(n, _, _)| n.eq_ignore_ascii_case(section.trim()))
        .ok_or_else(|| format!("no section `{section}`; the sections are {}", SECTIONS.map(|(n, _, _)| n).join(", ")))
}

impl Learning {
    pub fn parse(md: &str) -> Learning {
        let mut l = Learning::default();
        let mut at: Option<usize> = None;
        let mut in_other = false;
        for line in md.lines() {
            if let Some(head) = line.strip_prefix("## ") {
                at = index(head).ok();
                in_other = at.is_none();
                if in_other {
                    l.other.push_str(line);
                    l.other.push('\n');
                }
                continue;
            }
            if in_other {
                l.other.push_str(line);
                l.other.push('\n');
            } else if let (Some(i), Some(entry)) = (at, line.trim().strip_prefix("- ")) {
                l.entries[i].push(entry.trim().to_string());
            }
        }
        l
    }

    /// Back to the file: every section with entries, then the others.
    pub fn to_md(&self) -> String {
        let mut out = String::new();
        for (i, (name, _, _)) in SECTIONS.iter().enumerate() {
            if !self.entries[i].is_empty() {
                out.push_str(&format!("## {name}\n"));
                for e in &self.entries[i] {
                    out.push_str(&format!("- {e}\n"));
                }
                out.push('\n');
            }
        }
        out.push_str(&self.other);
        out.trim_end().to_string() + if out.trim().is_empty() { "" } else { "\n" }
    }

    /// Characters section `i` uses (what its cap counts): each entry and
    /// its `- ` and line break.
    pub fn used(&self, i: usize) -> usize {
        self.entries[i].iter().map(|e| e.chars().count() + 3).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.iter().all(Vec::is_empty)
    }

    /// `user 120/1200` for each section.
    pub fn room(&self) -> Vec<String> {
        SECTIONS.iter().enumerate().map(|(i, (n, _, cap))| format!("{n} {}/{cap}", self.used(i))).collect()
    }

    /// The Learning slot's body: each section with entries, under
    /// `### name — what it holds`.
    pub fn slot(&self) -> String {
        let mut out = Vec::new();
        for (i, (name, holds, _)) in SECTIONS.iter().enumerate() {
            if !self.entries[i].is_empty() {
                let lines: Vec<String> = self.entries[i].iter().map(|e| format!("- {e}")).collect();
                out.push(format!("### {name} — {holds}\n{}", lines.join("\n")));
            }
        }
        out.join("\n\n")
    }

    /// One change: add `entry` (no `old`), replace the entry `old` names
    /// with `entry`, or remove it (`entry` empty). The result line and
    /// the log line. A full section refuses; nothing is ever cut.
    pub fn learn(&mut self, section: &str, entry: &str, old: Option<&str>) -> Result<(String, String), String> {
        let i = index(section)?;
        let (name, _, cap) = SECTIONS[i];
        let entry = entry.trim().trim_start_matches("- ").trim().to_string();
        if entry.contains('\n') || entry.chars().count() > MAX_ENTRY {
            return Err(format!("an entry is one line of at most {MAX_ENTRY} characters"));
        }
        let at = match old.map(str::trim).filter(|o| !o.is_empty()) {
            None => None,
            Some(o) => {
                let exact = self.entries[i].iter().position(|e| e == o);
                let within: Vec<usize> = self.entries[i].iter().enumerate().filter(|(_, e)| e.contains(o)).map(|(n, _)| n).collect();
                match (exact, within.as_slice()) {
                    (Some(n), _) => Some(n),
                    (None, [n]) => Some(*n),
                    (None, []) => return Err(format!("no entry in {name} has `{o}`")),
                    (None, _) => return Err(format!("{} entries in {name} have `{o}`: give more of the one you mean", within.len())),
                }
            }
        };
        let size = |s: &Learning| s.used(i);
        let mut next = self.clone();
        let (done, log) = match (at, entry.is_empty()) {
            (None, true) => return Err("an entry is needed (or old=\"...\" with entry=\"\" to remove one)".into()),
            (None, false) => {
                if self.entries[i].contains(&entry) {
                    return Err(format!("{name} already has that entry"));
                }
                next.entries[i].push(entry.clone());
                ("added to", format!("add: {entry}"))
            }
            (Some(n), true) => {
                let gone = next.entries[i].remove(n);
                ("removed from", format!("remove: {gone}"))
            }
            (Some(n), false) => {
                let was = std::mem::replace(&mut next.entries[i][n], entry.clone());
                ("replaced in", format!("replace: {entry} (was: {was})"))
            }
        };
        if size(&next) > cap && size(&next) > size(self) {
            return Err(format!(
                "{name} is full: {}/{cap} used, this needs {}; merge two entries into one with old=\"...\", or remove one with entry=\"\" and old=\"...\"",
                size(self),
                size(&next) - size(self)
            ));
        }
        *self = next;
        Ok((format!("{done} {name} ({}/{cap})", size(self)), format!("{name} {log}")))
    }
}

/// `learn(section, entry, old?)` on engine `e`, at unix second `now`: the
/// file changes and the change is logged; the Learning slot keeps the
/// turn's first copy until your next message.
pub fn learn(e: &mut Engine, section: &str, entry: &str, old: Option<&str>, now: i64) -> Result<String, String> {
    let mut l = Learning::parse(&e.learning);
    let (result, log) = l.learn(section, entry, old)?;
    if e.learning_turn.is_none() {
        e.learning_turn = Some(e.learning.clone());
    }
    e.learning = l.to_md();
    e.learning_log.push_str(&format!("{now} learn {}\n", log.replace('\n', " ")));
    Ok(result)
}

/// The Learning slot for `e`: the file as at the turn's start.
pub fn slot(e: &Engine) -> Result<String, String> {
    let text = e.learning_turn.as_deref().unwrap_or(&e.learning);
    let l = Learning::parse(text);
    if l.is_empty() {
        return Err(format!("nothing learned yet ({FILE} is empty)"));
    }
    Ok(l.slot())
}

// ---- the learning artifact ---------------------------------------------------------

/// The collection a learning artifact shows (`collect: fixes`), if any.
pub fn collecting(text: &str) -> Option<&'static str> {
    let word = text.lines().find_map(|l| l.trim().strip_prefix("collect:"))?.trim();
    COLLECT.iter().find(|(w, _, _)| *w == word).map(|(w, _, _)| *w)
}

/// `collect(what)`: the artifact shows that collection from now on. The
/// result says how many items, never the items.
pub fn collect(e: &mut Engine, artifact: &str, what: &str) -> Result<String, String> {
    let (word, means, feeds) = COLLECT
        .iter()
        .find(|(w, _, _)| *w == what.trim())
        .ok_or_else(|| format!("collect what=\"...\": one of {}", COLLECT.map(|(w, _, _)| w).join(", ")))?;
    let n = gather(e, word).len();
    let text = e.artifacts.get_mut(artifact).ok_or("no learning artifact")?;
    let kept = crate::analytics::block_in(text).map(|(q, f)| crate::analytics::block(q, &f.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
    *text = format!("kind: learning\ncollect: {word}\n{}", kept.unwrap_or_default());
    Ok(if n == 0 {
        format!("no {word} in your history ({means})")
    } else {
        format!("collected {n} {word} ({means}); they are in the {artifact} artifact, for learn(section=\"{feeds}\", ...)")
    })
}

fn cut(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max).collect::<String>()) }
}

/// Is a user message a callback the app added (a terminal session that
/// ended, a child engine that replied), not the person's words?
pub fn is_callback(text: &str) -> bool {
    text.lines().filter(|l| !l.trim().is_empty()).all(|l| {
        let l = l.trim();
        (l.starts_with("session ") && l.ends_with(')') && l.contains("(terminal ")) || (l.starts_with("engine ") && l.contains(" replied"))
    })
}

/// The tools whose results say something about the world (`facts`).
fn looks_at_world(call: &str) -> bool {
    let name = call.split('(').next().unwrap_or("");
    name.contains('.') || ["run", "open", "focus", "save", "engine_call", "script"].contains(&name)
}

/// Every `call → result` line of the tool messages, oldest first.
fn results(messages: &[Message]) -> Vec<(String, String)> {
    messages.iter().filter(|m| m.role == Role::Tool).flat_map(|m| results_of(&m.text)).collect()
}

/// The `call → result` pairs of one tools message, in order (a result's
/// later lines, like a run's output tail, stay with it). The spans read
/// a step's calls from it (`crate::spans::Trace::results`).
pub fn results_of(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    {
        let mut last: Option<(String, String)> = None;
        for line in text.lines() {
            match line.split_once(" → ") {
                Some((call, result)) if call.contains('(') => {
                    out.extend(last.take());
                    last = Some((call.to_string(), result.to_string()));
                }
                // A result's later lines (a run's output tail).
                _ => {
                    if let Some((_, r)) = last.as_mut() {
                        r.push('\n');
                        r.push_str(line);
                    }
                }
            }
        }
        out.extend(last);
    }
    out
}

/// The items of collection `what`, newest last, each already one or two
/// short lines.
pub fn gather(e: &Engine, what: &str) -> Vec<String> {
    let mut items = Vec::new();
    match what {
        "corrections" => {
            let mut last_reply: Option<String> = None;
            for m in &e.messages {
                match m.role {
                    Role::Assistant => {
                        last_reply = Some(crate::turn::reply_in(e, &m.text).unwrap_or_else(|| m.text.clone()));
                    }
                    Role::User if !is_callback(&m.text) => {
                        if let Some(reply) = last_reply.take() {
                            items.push(format!(
                                "you replied: {}\n   they said: {}",
                                cut(&reply, COLLECT_CHARS),
                                cut(&m.text, COLLECT_CHARS)
                            ));
                        }
                    }
                    Role::User => last_reply = None,
                    _ => {}
                }
            }
        }
        "fixes" => {
            let all = results(&e.messages);
            let tool = |call: &str| call.split('(').next().unwrap_or("").to_string();
            for (n, (call, result)) in all.iter().enumerate() {
                let Some(why) = result.strip_prefix("error: ") else { continue };
                let fixed = all[n + 1..].iter().find(|(c, r)| tool(c) == tool(call) && !r.starts_with("error: "));
                if let Some((good, worked)) = fixed {
                    items.push(format!(
                        "failed: {} → {}\n   worked: {} → {}",
                        cut(call, COLLECT_CHARS),
                        cut(why, COLLECT_CHARS),
                        cut(good, COLLECT_CHARS),
                        cut(worked.lines().next().unwrap_or(""), COLLECT_CHARS)
                    ));
                }
            }
        }
        "facts" => {
            for (call, result) in results(&e.messages) {
                if looks_at_world(&call) && !result.starts_with("error: ") {
                    items.push(format!("{} → {}", cut(&call, COLLECT_CHARS), cut(result.lines().next().unwrap_or(""), COLLECT_CHARS)));
                }
            }
        }
        _ => {}
    }
    let skip = items.len().saturating_sub(COLLECT_ITEMS);
    items.into_iter().skip(skip).collect()
}

/// The learning artifact's snapshot: each section's room, and the
/// collection it shows.
pub fn snapshot(text: &str, e: &Engine) -> String {
    let l = Learning::parse(&e.learning);
    let mut out = format!(
        "Your learning ({FILE}), sent in the Learning slot: {}. learn(section, entry) adds one line (at most {MAX_ENTRY} characters); old=\"...\" replaces that entry, with entry=\"\" removes it. A full section refuses.",
        l.room().join(", ")
    );
    if e.learning_turn.as_ref().is_some_and(|t| *t != e.learning) {
        out.push_str("\nChanged this turn: the Learning slot shows the file as it was when the turn began, until the next message.");
        let now = l.slot();
        if !now.is_empty() {
            out.push_str(&format!("\nNow:\n{now}"));
        }
    }
    if !l.other.trim().is_empty() {
        out.push_str("\nThe file also has sections that are none of the three; they are kept but not sent.");
    }
    match collecting(text) {
        None => out.push_str(&format!(
            "\ncollect(what=\"...\") gathers the context a section learns from: {}.",
            COLLECT.map(|(w, m, f)| format!("{w} ({m}; for {f})")).join(", ")
        )),
        Some(what) => {
            let (_, means, feeds) = COLLECT.iter().find(|(w, _, _)| *w == what).expect("a known collection");
            let items = gather(e, what);
            out.push_str(&format!("\n\n#### collected: {what} — {means}; for learn(section=\"{feeds}\", ...)\n"));
            if items.is_empty() {
                out.push_str("None in your history.");
            }
            for (n, item) in items.iter().enumerate() {
                out.push_str(&format!("{}. {item}\n", n + 1));
            }
        }
    }
    if let Some((query, facts)) = crate::analytics::block_in(text) {
        out.push_str(&format!("\n\n#### analytics: {query} (your spans, the last {} days)\n", crate::analytics::TOOL_DAYS));
        if facts.is_empty() {
            out.push_str("Nothing recorded.");
        }
        for f in facts {
            out.push_str(&format!("- {f}\n"));
        }
    }
    out.trim_end().to_string()
}

/// `analytics(query)` on learning artifact `artifact`: the line back; the
/// facts replace the artifact's last analytics block (its collection stays).
pub fn analytics(e: &mut Engine, artifact: &str, query: &str, host: &dyn crate::live::Host) -> Result<String, String> {
    let spans = host.spans(&e.id)?;
    let toolbox = crate::live::toolbox(e);
    let now = host.now() * 1000;
    let (line, facts) = crate::analytics::tool(query, &spans, &toolbox, now, host.offset())?;
    let text = e.artifacts.get_mut(artifact).ok_or("no learning artifact")?;
    let head: String = match text.find("\nanalytics: ") {
        Some(n) => text[..=n].to_string(),
        None => format!("{}\n", text.trim_end()),
    };
    *text = format!("{head}{}", crate::analytics::block(query.trim(), &facts));
    Ok(format!("{line}; the facts are in the {artifact} artifact"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_reads_writes_and_keeps_a_section_it_does_not_know() {
        let md = "## user\n- Prefers short answers.\n\n## lessons\n- Run check first. (n=2, 11-22)\n\n## notes\nmine\n";
        let l = Learning::parse(md);
        assert_eq!(l.entries[0], ["Prefers short answers."]);
        assert_eq!(l.entries[2], ["Run check first. (n=2, 11-22)"]);
        assert_eq!(Learning::parse(&l.to_md()), l);
        assert!(l.to_md().contains("## notes\nmine"), "kept: {}", l.to_md());
        assert_eq!(l.room(), ["user 25/1200", "environment 0/1600", "lessons 32/1600"]);
        assert!(l.slot().starts_with("### user — what the person stated they want"));
        assert!(!l.slot().contains("mine"));
    }

    #[test]
    fn learn_adds_replaces_removes_and_a_full_section_refuses() {
        let mut l = Learning::default();
        assert_eq!(l.learn("user", "Short answers.", None).unwrap().0, "added to user (17/1200)");
        assert_eq!(
            l.learn("user", "Short answers, no lists.", Some("Short")).unwrap(),
            ("replaced in user (27/1200)".to_string(), "user replace: Short answers, no lists. (was: Short answers.)".to_string())
        );
        assert!(l.learn("user", "Short answers, no lists.", None).unwrap_err().contains("already has"));
        assert!(l.learn("mood", "x", None).unwrap_err().contains("the sections are user, environment, lessons"));
        assert!(l.learn("user", &"x".repeat(MAX_ENTRY + 1), None).is_err() && l.learn("user", "a\nb", None).is_err());
        assert_eq!(l.learn("user", "", Some("no lists")).unwrap().0, "removed from user (0/1200)");
        // Fill lessons to its cap: the next add is refused, nothing is cut.
        for n in 0..5 {
            l.learn("lessons", &format!("{n} {}", "y".repeat(290)), None).unwrap();
        }
        l.learn("lessons", &"z".repeat(100), None).unwrap();
        let before = l.clone();
        let full = l.learn("lessons", &"w".repeat(100), None).unwrap_err();
        assert!(full.starts_with("lessons is full: 1578/1600 used, this needs 103; merge two entries"), "{full}");
        assert_eq!(l, before, "a refused entry changes nothing");
        // A replace that makes it smaller is fine even when full.
        assert!(l.learn("lessons", "short", Some(&"z".repeat(100))).is_ok());
    }

    fn engine() -> Engine {
        let mut e = Engine::new("E", "", crate::inference::OpenAi::default());
        e.response = crate::response::Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e
    }

    fn says(e: &mut Engine, role: Role, text: &str) {
        e.messages.push(Message { role, text: text.into() });
    }

    #[test]
    fn collectors_gather_only_their_context() {
        let mut e = engine();
        says(&mut e, Role::User, "fix the build");
        says(&mut e, Role::Assistant, "thought: x\nact: run(command=\"cargo tset\")");
        says(
            &mut e,
            Role::Tool,
            "run(command=\"cargo tset\") → error: no such command\nwrite(name=\"a\", text=\"b\") → saved a (1 characters)",
        );
        says(&mut e, Role::Assistant, "thought: x\nact: run(command=\"cargo test\")");
        says(
            &mut e,
            Role::Tool,
            "run(command=\"cargo test\") → session 2 `cargo test`: exit 0\ntest result: ok\nopen(path=\"a.rs\") → opened a.rs (3 lines)",
        );
        says(&mut e, Role::Assistant, "thought: done\nact: reply(text=\"Fixed: it was a typo.\")");
        says(&mut e, Role::User, "session 3 `cargo build` finished, exit 0 (terminal term)");
        says(&mut e, Role::Assistant, "thought: ok\nact: reply(text=\"Built.\")");
        says(&mut e, Role::User, "No, always use cargo check first.");

        let fixes = gather(&e, "fixes");
        assert_eq!(fixes.len(), 1, "{fixes:?}");
        assert!(fixes[0].starts_with("failed: run(command=\"cargo tset\") → no such command\n   worked: run(command=\"cargo test\") → session 2 `cargo test`: exit 0"), "{}", fixes[0]);
        let facts = gather(&e, "facts");
        assert_eq!(
            facts,
            ["run(command=\"cargo test\") → session 2 `cargo test`: exit 0", "open(path=\"a.rs\") → opened a.rs (3 lines)"],
            "no write, no error"
        );
        let corrections = gather(&e, "corrections");
        assert_eq!(corrections, ["you replied: Built.\n   they said: No, always use cargo check first."], "the callback is not the person");
        assert!(is_callback("engine helper replied: \"ok\" (engines kids)"));
        assert!(!is_callback("No, again"));
    }

    #[test]
    fn collect_then_learn_through_the_artifact_and_every_change_is_logged() {
        let mut e = engine();
        e.artifacts.insert("learn".into(), "kind: learning".into());
        says(&mut e, Role::Assistant, "thought: x\nact: reply(text=\"Done in a list.\")");
        says(&mut e, Role::User, "Please no lists.");
        assert!(snapshot("kind: learning", &e).contains("collect(what=\"...\") gathers"));
        let r = collect(&mut e, "learn", "corrections").unwrap();
        assert_eq!(
            r,
            "collected 1 corrections (what the person said right after one of your replies); they are in the learn artifact, for learn(section=\"user\", ...)"
        );
        assert!(!r.contains("no lists"), "the result is short; the content is in the snapshot");
        let snap = snapshot(&e.artifacts["learn"], &e);
        assert!(snap.contains("#### collected: corrections") && snap.contains("they said: Please no lists."), "{snap}");
        assert_eq!(learn(&mut e, "user", "No lists in answers.", None, 100).unwrap(), "added to user (23/1200)");
        assert_eq!(e.learning, "## user\n- No lists in answers.\n");
        assert_eq!(e.learning_log, "100 learn user add: No lists in answers.\n");
        assert_eq!(e.learning_turn.as_deref(), Some(""), "the slot keeps the turn's first copy");
        assert!(slot(&e).unwrap_err().contains("nothing learned yet"));
        assert!(snapshot(&e.artifacts["learn"], &e).contains("Changed this turn"));
        e.learning_turn = None;
        assert!(slot(&e).unwrap().contains("- No lists in answers."));
        assert!(collect(&mut e, "learn", "gossip").unwrap_err().contains("corrections, fixes, facts"));
    }
}
