//! A worker's live snapshot: what an engine run by an outside program (a
//! Claude Code session) is told about its place in dry, pushed into its
//! conversation by hooks (`dry snapshot`, docs/sessions.md). dry owns the
//! context: the program gets a compact, current view at the start of each
//! turn, and each one says it supersedes the earlier ones.
//!
//! - **`start`** (SessionStart): who it is, its goal, the participants of
//!   its thread, and the tools it has.
//! - **`turn`** (UserPromptSubmit): its goal line, its phase, its
//!   artifacts' snapshots as the prompt's artifacts slot renders them
//!   (each kept short), and the thread's posts for it since `since`.
//! - **Budget.** The whole text is at most `budget` characters; what does
//!   not fit is cut and the text says what was cut.
//!
//! **Routing** (`inbox`): an engine is on one thread, its root's (the
//! head's, `crate::shared`), with you and every engine of the tree. A post
//! is for the engine it names (`to`). An unaddressed post wakes nobody
//! else: it goes back to its sender (a bounce, `Post::bounce`), so the
//! sender re-posts it to someone with the context they need. So an
//! engine's inbox is the posts addressed to it by others, and its own
//! unaddressed ones, bounced.
//!
//! **Cursor**: the number of entries the thread had when it was last told
//! (`7`). A removed entry shifts the count, so one new post may then be
//! missed: removing is rare, and the whole list is always one `thread`
//! read away. A cursor from before threads (`3.5`, two per-level counts)
//! reads as `0` here; an engine's stored one is dropped (`crate::files`),
//! so what is there counts as heard.
//!
//! Pure: a state in, a text out.

use crate::engines::Seen;
use crate::live::{Clock, Snapshots};
use crate::shared::Entry;
use crate::state::{Engine, State};

/// Most characters of a snapshot unless told otherwise.
pub const BUDGET: usize = 4_000;
/// Most characters of one artifact's section in a snapshot.
pub const ARTIFACT_CHARS: usize = 600;

/// The first words of every snapshot (a later one replaces it).
pub const SUPERSEDES: &str = "supersedes every earlier dry snapshot in this conversation";

/// How far an engine has been told about its thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub n: usize,
}

impl Cursor {
    /// `7`; anything else (a cursor from before threads, `3.5`) is `0`.
    pub fn parse(text: &str) -> Cursor {
        Cursor { n: text.trim().parse().unwrap_or(0) }
    }

    /// A stored cursor from before threads (`3.5`).
    pub fn is_old(text: &str) -> bool {
        text.contains('.')
    }

    pub fn text(self) -> String {
        self.n.to_string()
    }
}

/// One post for an engine: its number on the thread, the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    pub n: usize,
    pub entry: Entry,
    /// Its own post with no `to`, back to it undelivered.
    pub bounce: bool,
}

/// The thread engine `e` is on: its root's id.
pub fn thread_of(state: &State, e: &Engine) -> String {
    crate::shared::root_of(state, &e.id)
}

/// The posts for engine `i` since `since`, and the cursor after them.
pub fn inbox(state: &State, i: usize, since: Cursor) -> (Vec<Post>, Cursor) {
    let e = &state.engines[i];
    let root = thread_of(state, e);
    let all = crate::shared::parse(state.shared.get(&root).map_or("", String::as_str));
    let out = all
        .iter()
        .enumerate()
        .skip(since.n)
        .filter_map(|(n, p)| {
            let mine = p.author == e.id;
            let bounce = mine && p.to.is_none();
            (bounce || !mine && p.to.as_deref() == Some(e.id.as_str())).then(|| Post { n: n + 1, entry: p.clone(), bounce })
        })
        .collect();
    (out, Cursor { n: all.len() })
}

/// A post for an engine as one line: the entry, or for a bounce what
/// came back and why.
pub fn post_line(p: &Post, offset: i32) -> String {
    let line = crate::shared::line(p.n, &p.entry, offset);
    if p.bounce { format!("{} (not delivered: it has no to)", line) } else { line }
}

/// The participants of engine `i`'s thread, as lines.
pub fn participants(state: &State, i: usize) -> Vec<String> {
    let e = &state.engines[i];
    let root = thread_of(state, e);
    let engines: Vec<String> = crate::shared::members(state, &root).into_iter().map(|j| state.engines[j].id.clone()).filter(|id| *id != e.id).collect();
    let mut others: Vec<String> = Vec::new();
    for p in crate::shared::parse(state.shared.get(&root).map_or("", String::as_str)) {
        if p.author != e.id && p.author != crate::desk::YOU && state.index_of(&p.author).is_none() && !others.contains(&p.author) {
            others.push(p.author);
        }
    }
    let none = |l: &[String]| if l.is_empty() { "none".to_string() } else { l.join(", ") };
    let head = if root == e.id { "you are its head".to_string() } else { format!("head {root}") };
    let mut line = format!("your thread ({head}); the user posts as `{}`; its other engines: {}", crate::desk::YOU, none(&engines));
    if !others.is_empty() {
        line.push_str(&format!("; read-only participants: {}", others.join(", ")));
    }
    vec![line]
}

/// What a worker can call in dry (the `engines` MCP group).
pub const TOOLS_LINE: &str = "state() this snapshot now; artifact(name) one artifact in full; thread(read, since) your thread's posts; post(text, to) a post on your thread: to=<engine> calls that engine, to=you is for the user; it must carry what the addressee needs to act cold (what, why, what done looks like, where the material is); a post with no to comes back to you undelivered; engine_start(goal, template?) a child engine that works on its own and posts or replies back; engine_call(id, text) give a child a message and wait for its reply";

/// Cut `text` to `max` characters, saying how much was cut.
fn cut(text: &str, max: usize, more: &str) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    format!("{}\n… cut: {max} of {n} characters shown; {more}", text.chars().take(max).collect::<String>().trim_end())
}

/// The SessionStart text for engine `i`.
pub fn start(state: &State, i: usize, now: Clock, budget: usize) -> String {
    let e = &state.engines[i];
    let mut out = vec![format!(
        "dry snapshot of {} ({}) at {} · {SUPERSEDES}. You are a worker engine in dry: the dry super engine above you owns the context (snapshots, compression); a new snapshot comes with each message.",
        e.id,
        e.name,
        stamp(now)
    )];
    out.push(format!(
        "## goal\n{}",
        if e.description.trim().is_empty() {
            "(none written)".to_string()
        } else {
            cut(e.description.trim(), 1_200, "artifact(name=\"goal\") is not one; state() has the goal line")
        }
    ));
    out.push(format!("## participants\n{}", participants(state, i).join("\n")));
    let allow = if e.allow.trim().is_empty() { "nothing".to_string() } else { e.allow.trim().to_string() };
    out.push(format!(
        "## tools\ndry (MCP server `engines`): {TOOLS_LINE}.\nAllowed without asking: {allow}. Anything else asks your owner first (an approval), and waits."
    ));
    fit(out, budget)
}

/// `11-23 14:05`.
fn stamp(now: Clock) -> String {
    let (_, m, d) = crate::live::civil_from_days(now.day());
    format!("{m:02}-{d:02} {:02}:{:02}", now.minute() / 60, now.minute() % 60)
}

/// The UserPromptSubmit text for engine `i`, with the posts since `since`;
/// and the cursor after them. `seen`: how the shell sees each engine.
pub fn turn(state: &State, i: usize, since: Cursor, now: Clock, seen: &dyn Fn(usize) -> Seen, budget: usize) -> (String, Cursor) {
    let e = &state.engines[i];
    let mut out = vec![format!("dry snapshot of {} at {} · {SUPERSEDES}.", e.id, stamp(now))];
    let goal = crate::goal::summary(&e.description);
    out.push(format!("goal: {}", if goal.is_empty() { "(none written)" } else { &goal }));
    let phases = e.phase_list().unwrap_or_default();
    match phases.get(e.run.phase) {
        Some(p) => out.push(format!("phase: {} (step {})", p.name, e.run.step)),
        None => out.push("phase: none (no phases in its template)".into()),
    }
    let mut fam = crate::engines::family(state, i, seen);
    fam.offset = now.offset;
    let snaps = Snapshots { clock: Some(now), host: None, family: Some(&fam) };
    let sections: Vec<String> = crate::prompt::artifact_sections(e, &snaps)
        .into_iter()
        .map(|(name, s)| match name {
            Some(n) => cut(&s, ARTIFACT_CHARS, &format!("artifact(name=\"{n}\") for all of it")),
            None => cut(&s, ARTIFACT_CHARS, "state() for the rest"),
        })
        .collect();
    out.push(if sections.is_empty() { "## artifacts\nnone".into() } else { format!("## artifacts\n{}", sections.join("\n\n")) });
    let (posts, after) = inbox(state, i, since);
    let lines: Vec<String> =
        posts.iter().map(|p| post_line(p, now.offset)).collect();
    out.push(if lines.is_empty() {
        format!("## new posts for you since {}\nnone", since.text())
    } else {
        format!("## new posts for you since {} ({})\n{}", since.text(), lines.len(), lines.join("\n"))
    });
    out.push(format!("cursor: {}", after.text()));
    (fit(out, budget), after)
}

/// Join the parts; over `budget`, the longest parts (never the first and
/// last lines) are cut until it fits, saying what was cut.
fn fit(mut parts: Vec<String>, budget: usize) -> String {
    let total = |p: &[String]| p.iter().map(|s| s.chars().count() + 2).sum::<usize>();
    let mut cuts = Vec::new();
    while total(&parts) > budget && parts.len() > 2 {
        let last = parts.len() - 1;
        let (j, n) = parts
            .iter()
            .enumerate()
            .take(last)
            .skip(1)
            .map(|(j, s)| (j, s.chars().count()))
            .max_by_key(|(_, n)| *n)
            .expect("a middle part");
        let over = total(&parts) - budget;
        let keep = n.saturating_sub(over + 40).max(60);
        if keep >= n {
            break;
        }
        let title = parts[j].lines().next().unwrap_or("").to_string();
        parts[j] = format!("{}…", parts[j].chars().take(keep).collect::<String>());
        cuts.push(format!("{} ({n} → {keep} characters)", title.trim_start_matches("## ")));
    }
    let mut text = parts.join("\n\n");
    if !cuts.is_empty() {
        text.push_str(&format!("\n\n(cut to fit {budget} characters: {}; state() and artifact(name) have all of it)", cuts.join(", ")));
    }
    text
}

/// What a worker reads (`dry snapshot`, the `engines` MCP tools).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    /// The SessionStart text.
    Start,
    /// The turn snapshot with the posts since a cursor.
    Turn(Cursor),
    /// One artifact in full: a kind's snapshot as the prompt renders it,
    /// else its text.
    Artifact(String),
    /// Its thread: `thread` (all of it) or `inbox` (only the posts for
    /// it), from a cursor.
    Thread { which: String, since: Cursor },
}

impl Read {
    /// From the words a request carries: `what` (`start`, `turn`,
    /// `artifact`, `thread`; `dashboard` is its old name), `name`, `which`, `since`.
    pub fn parse(what: &str, name: &str, which: &str, since: &str) -> Result<Read, String> {
        let since = Cursor::parse(since);
        Ok(match what {
            "start" => Read::Start,
            "turn" | "state" | "" => Read::Turn(since),
            "artifact" if !name.trim().is_empty() => Read::Artifact(name.trim().to_string()),
            "artifact" => return Err("artifact needs name=\"...\"".into()),
            "thread" | "dashboard" => Read::Thread { which: if which.trim().is_empty() { "thread".into() } else { which.trim().to_string() }, since },
            other => return Err(format!("nothing to read called {other}; start, turn, artifact or thread")),
        })
    }
}

/// Engine `i`'s thread (or only its inbox) from `since`.
pub fn thread(state: &State, i: usize, which: &str, since: Cursor, offset: i32) -> Result<String, String> {
    let e = &state.engines[i];
    let root = thread_of(state, e);
    let all = || {
        let entries = crate::shared::parse(state.shared.get(&root).map_or("", String::as_str));
        let lines: Vec<String> = entries.iter().enumerate().skip(since.n).map(|(n, p)| crate::shared::line(n + 1, p, offset)).collect();
        let shown = if lines.is_empty() { "no entries".to_string() } else { lines.join("\n") };
        let head = if root == e.id { "you are its head".to_string() } else { format!("head {root}") };
        format!("## your thread ({head}) — {} entries{}\n{shown}", entries.len(), if since.n > 0 { format!(", from {}", since.n + 1) } else { String::new() })
    };
    Ok(match which {
        // `both`, `level` and `children` were the two per-level lists.
        "thread" | "both" | "level" | "children" => all(),
        "inbox" => {
            let (posts, after) = inbox(state, i, since);
            let lines: Vec<String> =
                posts.iter().map(|p| post_line(p, offset)).collect();
            format!(
                "## posts for you since {} ({})\n{}\ncursor: {}",
                since.text(),
                lines.len(),
                if lines.is_empty() { "none".into() } else { lines.join("\n") },
                after.text()
            )
        }
        other => return Err(format!("read: thread or inbox, not {other}")),
    })
}

/// Engine `id`'s read `what`: the text, and the cursor after it for a turn.
pub fn read(state: &State, id: &str, what: &Read, now: Clock, seen: &dyn Fn(usize) -> Seen) -> Result<(String, Option<Cursor>), String> {
    let i = state.index_of(id).ok_or_else(|| format!("no engine {id}"))?;
    let e = &state.engines[i];
    Ok(match what {
        Read::Start => (start(state, i, now, BUDGET), None),
        Read::Turn(since) => {
            let (text, after) = turn(state, i, *since, now, seen, BUDGET);
            (text, Some(after))
        }
        Read::Artifact(name) => {
            let text = e.artifacts.get(name).ok_or_else(|| {
                format!("{id} has no artifact {name}; it has {}", e.artifacts.keys().cloned().collect::<Vec<_>>().join(", "))
            })?;
            match crate::live::Live::of(text) {
                Some(kind) => {
                    let mut fam = crate::engines::family(state, i, seen);
                    fam.offset = now.offset;
                    let snaps = Snapshots { clock: Some(now), host: None, family: Some(&fam) };
                    (
                        kind.snapshot(name, e, &snaps)
                            .unwrap_or_else(|| format!("{name}: a {} artifact; its snapshot is taken on the host", kind.as_str())),
                        None,
                    )
                }
                None => (text.clone(), None),
            }
        }
        Read::Thread { which, since } => (thread(state, i, which, *since, now.offset)?, None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{Edit, apply};

    const SUNDAY: i64 = 1_795_356_330;

    fn idle(_: usize) -> Seen {
        Seen { label: "idle".into(), ..Seen::default() }
    }

    /// A top-level session `claude-1` with a child `helper`, and a sibling `other`.
    fn state() -> State {
        let mut s = State::default();
        let w = s.add_session("claude", "Find the failing test.");
        s.engines[w].artifacts.insert("kids".into(), "kind: engines".into());
        s.engines[w].artifacts.insert("notes".into(), "x".repeat(900));
        let o = s.add_engine();
        s.engines[o].id = "other".into();
        let h = s.add_engine();
        s.engines[h].id = "helper".into();
        s.engines[h].parent = Some(s.engines[w].id.clone());
        s
    }

    fn post(s: &mut State, level: &str, author: &str, to: Option<&str>, text: &str) {
        let t = s.shared.get(level).cloned().unwrap_or_default();
        let (t, _) = apply(&t, &Edit::Post { author: author.into(), at: SUNDAY, to: to.map(str::to_string), text: text.into() }).unwrap();
        s.shared.insert(level.into(), t);
    }

    #[test]
    fn posts_go_to_the_one_they_name_and_unaddressed_back_to_the_sender() {
        let mut s = state();
        let me = s.engines[0].id.clone();
        post(&mut s, "other", "other", Some(&me), "another thread's");
        post(&mut s, &me, "helper", Some(&me), "found it");
        post(&mut s, &me, "helper", Some("someone"), "not yours");
        post(&mut s, &me, &me, None, "mine, no to");
        post(&mut s, &me, "helper", None, "helper's, no to");
        let (posts, after) = inbox(&s, 0, Cursor::default());
        let got: Vec<(&str, bool)> = posts.iter().map(|p| (p.entry.text.as_str(), p.bounce)).collect();
        assert_eq!(got, [("found it", false), ("mine, no to", true)]);
        assert_eq!(after, Cursor { n: 4 });
        assert_eq!(posts[0].n, 1);
        // Since the cursor: nothing new; one more post is.
        assert!(inbox(&s, 0, after).0.is_empty());
        post(&mut s, &me, "helper", Some(&me), "done");
        assert_eq!(inbox(&s, 0, after).0.len(), 1);
        // The child gets what is addressed to it, and its own unaddressed back.
        let h = s.index_of("helper").unwrap();
        post(&mut s, &me, crate::desk::YOU, Some("helper"), "try turn.rs");
        let got: Vec<(String, bool)> = inbox(&s, h, Cursor::default()).0.into_iter().map(|p| (p.entry.text, p.bounce)).collect();
        assert_eq!(got, [("helper's, no to".to_string(), true), ("try turn.rs".to_string(), false)]);
        assert_eq!(Cursor::parse("7"), Cursor { n: 7 });
        assert_eq!(Cursor::parse("3.5"), Cursor::default(), "a cursor from before threads");
        assert!(Cursor::is_old("3.5") && !Cursor::is_old("7"));
        assert_eq!(Cursor { n: 7 }.text(), "7");
    }

    #[test]
    fn a_turn_snapshot_says_it_supersedes_and_keeps_to_its_budget() {
        let mut s = state();
        let me = s.engines[0].id.clone();
        post(&mut s, &me, "helper", Some(&me), "found it");
        let now = Clock::new(SUNDAY, 0);
        let (text, after) = turn(&s, 0, Cursor::default(), now, &idle, BUDGET);
        assert!(text.starts_with(&format!("dry snapshot of {me} at 11-22 14:05 · {SUPERSEDES}.")), "{text}");
        assert!(text.contains("\ngoal: Find the failing test.\n"), "{text}");
        assert!(text.contains("phase: none"), "{text}");
        assert!(text.contains("### kids — "), "the engines artifact as the prompt renders it: {text}");
        assert!(text.contains("helper"), "{text}");
        assert!(text.contains("… cut: 600 of"), "a long artifact is kept short: {text}");
        assert!(text.contains("## new posts for you since 0 (1)\n1. 11-22 14:05 helper → "), "{text}");
        assert!(text.ends_with("cursor: 1"), "{text}");
        assert_eq!(after.n, 1);
        let (small, _) = turn(&s, 0, Cursor::default(), now, &idle, 700);
        assert!(small.chars().count() < 900, "{}", small.chars().count());
        assert!(small.contains("(cut to fit 700 characters: artifacts"), "{small}");
        assert!(small.ends_with("state() and artifact(name) have all of it)"), "{small}");
    }

    #[test]
    fn a_start_snapshot_names_goal_participants_and_tools() {
        let mut s = state();
        let me = s.engines[0].id.clone();
        post(&mut s, &me, "agent-say-ok", None, "started: say ok");
        let text = start(&s, 0, Clock::new(SUNDAY, 0), BUDGET);
        assert!(text.contains(SUPERSEDES), "{text}");
        assert!(text.contains("## goal\nFind the failing test."), "{text}");
        assert!(text.contains("your thread (you are its head); the user posts as `you`; its other engines: helper; read-only participants: agent-say-ok"), "{text}");
        assert!(text.contains("engine_start(goal, template?)") && text.contains("Allowed without asking: Read, Glob"), "{text}");
    }

    #[test]
    fn reads_by_name_and_the_thread_from_a_cursor() {
        let mut s = state();
        let me = s.engines[0].id.clone();
        post(&mut s, &me, "helper", Some(&me), "one");
        post(&mut s, &me, "helper", None, "two");
        post(&mut s, "other", "other", Some(&me), "elsewhere");
        let now = Clock::new(SUNDAY, 0);
        let r = |what: Read| read(&s, &me, &what, now, &idle).map(|(t, _)| t);
        assert_eq!(r(Read::Artifact("notes".into())).unwrap(), "x".repeat(900), "all of it");
        assert!(r(Read::Artifact("kids".into())).unwrap().contains("#### helper"), "a kind: its snapshot");
        assert!(r(Read::Artifact("nope".into())).unwrap_err().contains("it has kids, notes"));
        let all = r(Read::Thread { which: "thread".into(), since: Cursor { n: 1 } }).unwrap();
        assert_eq!(all, "## your thread (you are its head) — 2 entries, from 2\n2. 11-22 14:05 helper: two");
        assert_eq!(r(Read::Thread { which: "both".into(), since: Cursor { n: 1 } }).unwrap(), all, "the old words read the thread");
        let inbox = r(Read::Thread { which: "inbox".into(), since: Cursor::default() }).unwrap();
        assert!(inbox.contains("1. 11-22 14:05 helper → ") && inbox.ends_with("cursor: 2"), "{inbox}");
        assert!(r(Read::Thread { which: "x".into(), since: Cursor::default() }).is_err());
        assert_eq!(Read::parse("dashboard", "", "", "2").unwrap(), Read::Thread { which: "thread".into(), since: Cursor { n: 2 } });
        assert_eq!(Read::parse("thread", "", "inbox", "").unwrap(), Read::Thread { which: "inbox".into(), since: Cursor::default() });
        assert!(Read::parse("artifact", " ", "", "").is_err() && Read::parse("fly", "", "", "").is_err());
        assert!(read(&s, "ghost", &Read::Start, now, &idle).is_err());
    }
}
