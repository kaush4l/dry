//! The Desk: every live engine as a tree, one thread per root, and each
//! engine's work as a session stream (docs/desk.md).
//!
//! ```text
//! you
//! ├─ translator ─ thread "translator" (state/engines/translator/shared.md)
//! │  ├─ helper     (on translator's thread)
//! │  │  └─ deep    (on translator's thread)
//! │  └─ tester
//! └─ claude-1   ─ thread "claude-1"
//! ```
//!
//! Nothing here is stored. The tree is the engines' `parent` links, a
//! thread is a root's posts (`crate::shared`) and its participants (you,
//! the head, every engine of its tree, read-only authors), and the work
//! under a post is the target engine's own turn in its messages: the user
//! message that delivered the post, then each step's thought, its calls
//! with their results, and its reply. So a new child joins its root's
//! thread, a new root is a new thread, and what a thread shows is the
//! same data the engine page shows.
//!
//! **The guided-message rule** (`guided`): an engine is only called when
//! its thread has a guided message for it: a post addressed to it (by
//! you or an engine), or its own post with no `to`, which bounces back to
//! it as a notice so it re-posts it to someone with the context they
//! need. Each engine keeps how far it has heard (`Engine::heard`,
//! `convo/run.md`); `turn::callbacks` delivers what is new as one user
//! message (`WAKE_HEAD` and a line per post) and the shell starts its
//! turn, the way an `engine_start` reply wakes a parent. Posts by
//! read-only participants (a session's sub-agents) are reports: they call
//! nobody and never bounce. Your post with no `to` wakes nobody; the
//! thread shows it as not delivered.
//!
//! Pure: a state and each engine's live status in, a model out.

use crate::card::Status;
use crate::inference::Role;
use crate::response::ResponseFormat;
use crate::shared::Entry;
use crate::snapshot::Cursor;
use crate::state::{Engine, State};

/// Who you are on the threads.
pub const YOU: &str = "you";
/// The first line of the message that delivers guided posts.
pub const WAKE_HEAD: &str = "Guided posts for you:";
/// Most characters of a call's argument summary.
pub const ARGS_CHARS: usize = 80;

/// What the shell knows about an engine now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub status: Status,
    /// What it is doing, in a few words (`idle`, `calling the LLM`).
    pub label: String,
    /// A turn is running.
    pub busy: bool,
}

/// The whole Desk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Desk {
    /// You first, then every engine in tree order (`depth` indents).
    pub tree: Vec<Node>,
    /// One per root, in tree order.
    pub threads: Vec<Thread>,
}

impl Desk {
    /// The thread `id` names, else the first (the one the Desk opens on).
    pub fn thread(&self, id: Option<&str>) -> Option<&Thread> {
        id.and_then(|id| self.threads.iter().find(|t| t.id == id)).or_else(|| self.threads.first())
    }

    /// What a selection shows: the thread `thread` names, else the one
    /// engine `to` is on, else the first; and who the composer posts to:
    /// `to` when it is on that thread, else its head.
    pub fn pick(&self, thread: Option<&str>, to: Option<&str>) -> Option<(&Thread, Option<String>)> {
        let on = |id: &str| self.tree.iter().find(|n| n.index.is_some() && n.id == id).map(|n| n.thread.as_str());
        let t = self.thread(thread.filter(|id| self.threads.iter().any(|t| t.id == *id)).or_else(|| to.and_then(on)))?;
        let to = to.filter(|id| targets(t).iter().any(|p| p.id == *id)).map(str::to_string).or_else(|| t.default_to.clone());
        Some((t, to))
    }
}

/// One row of the tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    /// The engine's id; `""` for you.
    pub id: String,
    pub name: String,
    /// The engine's index in the state; `None` for you.
    pub index: Option<usize>,
    pub depth: usize,
    pub status: Status,
    pub label: String,
    /// The thread selecting it shows: its root's (`""` for you: the first).
    pub thread: String,
}

/// One root's thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thread {
    /// Its root's id (the head).
    pub id: String,
    pub title: String,
    /// You, the head, the rest of its tree, then read-only authors.
    pub participants: Vec<Participant>,
    pub messages: Vec<Msg>,
    /// Who a new post goes to unless picked: the head.
    pub default_to: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub status: Status,
    pub label: String,
    /// The thread's head.
    pub head: bool,
    /// An engine (it can be called); else you or a read-only participant.
    pub engine: bool,
}

/// One post on a thread, and the work it started.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Msg {
    /// Its number on the thread (1-based).
    pub n: usize,
    pub at: i64,
    pub from: String,
    /// Who it is for; `None`: it named nobody.
    pub to: Option<String>,
    pub text: String,
    /// An engine's post with no `to`: it went back to its sender (whose
    /// prompt needs fixing).
    pub bounce: bool,
    /// Your post with no `to`: kept, delivered to nobody.
    pub lost: bool,
    /// The turn it started: the addressee's, or for a bounce the sender's;
    /// `None` when it calls nobody.
    pub work: Option<Turn>,
}

/// Where a turn is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Not delivered yet: the engine is busy, or the next check is soon.
    #[default]
    Queued,
    Running,
    /// It waits for a child (`engine_call`).
    Waiting,
    Done,
    Failed(String),
}

impl Phase {
    pub fn word(&self) -> &'static str {
        match self {
            Phase::Queued => "queued",
            Phase::Running => "running",
            Phase::Waiting => "waiting",
            Phase::Done => "done",
            Phase::Failed(_) => "failed",
        }
    }
}

/// One turn of an engine: what started it and each step as it happened.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    pub engine: String,
    /// Its user message's index in the engine's messages; `None` before it
    /// was delivered.
    pub start: Option<usize>,
    /// The user message (your text, or the delivered posts).
    pub text: String,
    pub phase: Phase,
    pub items: Vec<Item>,
    pub reply: Option<String>,
}

/// One thing a step did.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Thought(String),
    Call(CallRow),
    /// A summary the history was compressed to.
    Summary(String),
    /// An answer that does not read with the contract, as it came.
    Text(String),
}

/// A tool call as one line: `name  args → result`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallRow {
    pub name: String,
    /// The arguments, cut to `ARGS_CHARS`.
    pub args: String,
    /// The first line of the result (`running…` while it runs).
    pub result: String,
    /// The whole call and result, for the fold.
    pub full: String,
    pub phase: Phase,
}

/// The start of post `n`'s line in a wake message.
fn wake_tag(n: usize) -> String {
    format!("[thread {n}] ")
}

/// One delivered post, as its line in the wake message.
pub fn wake_line(n: usize, e: &Entry) -> String {
    format!("{}{}: {}", wake_tag(n), crate::shared::who(e), e.text)
}

/// Engine `i`'s own post `n` with no `to`, back to it: why, and whom it
/// may address on its thread.
pub fn bounce_line(state: &State, i: usize, n: usize, e: &Entry) -> String {
    let me = &state.engines[i].id;
    let root = crate::shared::root_of(state, me);
    let mut who = vec![YOU.to_string()];
    who.extend(crate::shared::members(state, &root).into_iter().map(|j| state.engines[j].id.clone()).filter(|id| id != me));
    format!(
        "{}Not delivered — post #{n} had no `to`. Address it to one of: {}, with the context the recipient needs to act on it cold (what, why, what done looks like, where the material is). It said: {}",
        wake_tag(n),
        who.join(", "),
        e.text
    )
}

/// The guided posts engine `i` has not heard yet, as the message that
/// delivers them (empty: none), and its cursor moved past them. An engine
/// that never counted (`heard: None`, a store from before this rule or
/// from before threads) counts what is there as heard, so old posts call
/// nobody.
pub fn guided(state: &mut State, i: usize) -> Vec<String> {
    let Some(e) = state.engines.get(i) else { return Vec::new() };
    let (posts, after) = crate::snapshot::inbox(state, i, e.heard.unwrap_or_default());
    let Some(since) = e.heard else {
        state.engines[i].heard = Some(after);
        return Vec::new();
    };
    let calls = |author: &str| author == YOU || state.engines.iter().any(|o| o.id == author);
    let lines: Vec<String> = posts
        .iter()
        .filter(|p| calls(&p.entry.author))
        .map(|p| if p.bounce { bounce_line(state, i, p.n, &p.entry) } else { wake_line(p.n, &p.entry) })
        .collect();
    if after != since {
        state.engines[i].heard = Some(after);
    }
    if lines.is_empty() {
        return Vec::new();
    }
    vec![format!("{WAKE_HEAD}\n{}", lines.join("\n"))]
}

/// The Desk now, with each engine's live status from `live`.
pub fn build(state: &State, live: &dyn Fn(usize) -> Live) -> Desk {
    let mut desk = Desk::default();
    let lives: Vec<Live> = (0..state.engines.len()).map(live).collect();
    desk.tree.push(Node { id: String::new(), name: YOU.into(), index: None, depth: 0, status: Status::Idle, label: "you".into(), thread: String::new() });
    fn walk(state: &State, lives: &[Live], parent: Option<&str>, depth: usize, out: &mut Vec<Node>) {
        for (i, e) in state.engines.iter().enumerate() {
            let here = match parent {
                None => crate::shared::is_root(state, &e.id),
                Some(want) => e.parent.as_deref() == Some(want) && !crate::shared::is_root(state, &e.id),
            };
            if !here || out.iter().any(|n| n.index == Some(i)) {
                continue;
            }
            out.push(Node {
                id: e.id.clone(),
                name: e.name.clone(),
                index: Some(i),
                depth,
                status: lives[i].status.clone(),
                label: lives[i].label.clone(),
                thread: crate::shared::root_of(state, &e.id),
            });
            walk(state, lives, Some(&e.id), depth + 1, out);
        }
    }
    walk(state, &lives, None, 1, &mut desk.tree);
    let roots: Vec<String> = desk.tree.iter().filter(|n| n.depth == 1).map(|n| n.id.clone()).collect();
    for root in &roots {
        desk.threads.push(thread(state, &lives, root));
    }
    if let Some(first) = roots.first() {
        desk.tree[0].thread = first.clone();
    }
    desk
}

/// Root `root`'s thread.
fn thread(state: &State, lives: &[Live], root: &str) -> Thread {
    let part = |i: usize, head: bool| Participant {
        id: state.engines[i].id.clone(),
        name: state.engines[i].name.clone(),
        status: lives[i].status.clone(),
        label: lives[i].label.clone(),
        head,
        engine: true,
    };
    let mut participants = vec![Participant { id: YOU.into(), name: YOU.into(), label: "you".into(), ..Participant::default() }];
    let members = crate::shared::members(state, root);
    participants.extend(members.iter().map(|&i| part(i, state.engines[i].id == root)));
    let entries = crate::shared::parse(state.shared.get(root).map_or("", String::as_str));
    for e in &entries {
        if !participants.iter().any(|p| p.id == e.author) {
            participants.push(Participant { id: e.author.clone(), name: e.author.clone(), label: "posts here".into(), ..Participant::default() });
        }
    }
    let name = state.index_of(root).map_or(root, |i| state.engines[i].name.as_str());
    let messages = entries
        .iter()
        .enumerate()
        .map(|(k, e)| {
            let n = k + 1;
            let engine = state.index_of(&e.author).is_some();
            let bounce = e.to.is_none() && engine;
            let lost = e.to.is_none() && e.author == YOU;
            // Whose turn it starts: the addressee's (posts by you or an
            // engine), or the sender's for a bounce.
            let target = match &e.to {
                Some(to) if (engine || e.author == YOU) && *to != e.author => state.index_of(to),
                None if bounce => state.index_of(&e.author),
                _ => None,
            };
            let work = target.and_then(|t| work_on(state, &lives[t], t, n));
            Msg { n, at: e.at, from: e.author.clone(), to: e.to.clone(), text: e.text.clone(), bounce, lost, work }
        })
        .collect();
    Thread { id: root.to_string(), title: format!("{name}'s thread"), participants, messages, default_to: state.index_of(root).map(|_| root.to_string()) }
}

/// Engine `t`'s turn on post `n` of its thread: the turn whose message
/// delivered it; queued while not delivered.
fn work_on(state: &State, live: &Live, t: usize, n: usize) -> Option<Turn> {
    let e = &state.engines[t];
    let tag = wake_tag(n);
    let start = e.messages.iter().position(|m| m.role == Role::User && m.text.starts_with(WAKE_HEAD) && m.text.lines().any(|l| l.starts_with(&tag)));
    match start {
        Some(k) => turns(e, live).into_iter().find(|t| t.start == Some(k)),
        None => {
            let past = e.heard.unwrap_or_default().n >= n;
            let phase = if past { Phase::Done } else { Phase::Queued };
            Some(Turn { engine: e.id.clone(), phase, ..Turn::default() })
        }
    }
}

/// Engine `e`'s conversation as turns, oldest first: each from a user
/// message to the next, its steps read with the engine's contract. The
/// last one is `Running` while `live.busy`.
pub fn turns(e: &Engine, live: &Live) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    let ms = &e.messages;
    let mut k = 0;
    while k < ms.len() {
        let m = &ms[k];
        let mut t = Turn { engine: e.id.clone(), ..Turn::default() };
        match m.role {
            Role::User => {
                t.start = Some(k);
                t.text = m.text.clone();
                k += 1;
            }
            Role::Summary => {
                t.items.push(Item::Summary(m.text.clone()));
                k += 1;
            }
            _ => {}
        }
        while k < ms.len() && ms[k].role != Role::User {
            let m = &ms[k];
            match m.role {
                Role::Assistant => {
                    let next_tool = ms.get(k + 1).filter(|n| n.role == Role::Tool);
                    step(e, &m.text, next_tool.map(|n| n.text.as_str()), &mut t);
                    k += if next_tool.is_some() { 2 } else { 1 };
                }
                Role::Tool => {
                    rows(&m.text, &mut t);
                    k += 1;
                }
                Role::Summary => {
                    t.items.push(Item::Summary(m.text.clone()));
                    k += 1;
                }
                _ => k += 1,
            }
        }
        t.phase = Phase::Done;
        out.push(t);
    }
    if let Some(last) = out.last_mut() {
        last.phase = if live.busy {
            Phase::Running
        } else if !e.waiting.is_empty() {
            Phase::Waiting
        } else if let Status::Failed(why) = &live.status {
            Phase::Failed(why.clone())
        } else {
            Phase::Done
        };
        if last.phase == Phase::Running {
            // Calls of the last answer with no results yet are running.
            for item in &mut last.items {
                if let Item::Call(c) = item
                    && c.phase == Phase::Queued
                {
                    c.phase = Phase::Running;
                }
            }
        } else {
            for item in &mut last.items {
                if let Item::Call(c) = item
                    && c.phase == Phase::Queued
                {
                    c.phase = Phase::Failed("no result".into());
                    c.result = "no result came back".into();
                }
            }
        }
    }
    out
}

/// One answer (and its results, when they came) into `t`.
fn step(e: &Engine, text: &str, results: Option<&str>, t: &mut Turn) {
    if e.response.is_empty() {
        t.reply = Some(text.trim().to_string());
        if let Some(r) = results {
            rows(r, t);
        }
        return;
    }
    let Ok(answer) = e.format.parse(&e.response, text) else {
        t.items.push(Item::Text(text.trim().to_string()));
        if let Some(r) = results {
            rows(r, t);
        }
        return;
    };
    if let Some(thought) = crate::response::Responses::get(&answer, "thought").map(str::trim).filter(|s| !s.is_empty()) {
        t.items.push(Item::Thought(thought.to_string()));
    }
    if let Some(reply) = crate::turn::reply_in(e, text) {
        t.reply = Some(reply);
    }
    match results {
        Some(r) => rows(r, t),
        None => {
            // Not run yet (or the turn stopped): its calls, but `reply`.
            let calls = crate::response::Responses::get(&answer, crate::turn::ACT).and_then(|a| crate::calls::parse(a).ok()).unwrap_or_default();
            for c in calls.into_iter().filter(|c| c.name != "reply") {
                let full = c.to_string();
                t.items.push(Item::Call(CallRow { name: c.name.clone(), args: args_of(&full), result: "running…".into(), full, phase: Phase::Queued }));
            }
        }
    }
}

/// A tools message's `call → result` lines into `t`.
fn rows(text: &str, t: &mut Turn) {
    for (call, result) in crate::learning::results_of(text) {
        let name = call.split('(').next().unwrap_or("").trim().to_string();
        let first = result.lines().next().unwrap_or("").to_string();
        let phase = match first.strip_prefix("error: ") {
            Some(why) => Phase::Failed(why.to_string()),
            None if first.starts_with("waiting for ") => Phase::Waiting,
            None => Phase::Done,
        };
        t.items.push(Item::Call(CallRow { name, args: args_of(&call), result: first, full: format!("{call} → {result}"), phase }));
    }
}

/// `name(a="x", b="y")` → `a="x", b="y"`, cut to `ARGS_CHARS`.
fn args_of(call: &str) -> String {
    let inner = call.split_once('(').map_or("", |(_, r)| r.strip_suffix(')').unwrap_or(r));
    let n = inner.chars().count();
    if n <= ARGS_CHARS { inner.to_string() } else { format!("{}…", inner.chars().take(ARGS_CHARS).collect::<String>()) }
}


/// Every engine that may be a post's target on thread `t`: the head and
/// its tree.
pub fn targets(t: &Thread) -> Vec<&Participant> {
    t.participants.iter().filter(|p| p.engine).collect()
}

/// The guided post you make from the Desk's composer: on root `thread`'s
/// thread (`""`: the first root's, for the old `top` route), to `to` (an
/// engine of that thread). The composer always names one (its default is
/// the head, `Thread::default_to`); with `None` the post is kept and
/// delivered to nobody. The thread's id and the edit, or why not.
pub fn your_post(state: &State, thread_id: &str, to: Option<&str>, text: &str, at: i64) -> Result<(String, crate::shared::Edit), String> {
    let lives = vec![Live::default(); state.engines.len()];
    let desk = build(state, &|i| lives[i].clone());
    let t = if thread_id.is_empty() {
        desk.thread(None).ok_or("there is no thread yet: start one with New thread")?
    } else {
        desk.threads.iter().find(|t| t.id == thread_id).ok_or_else(|| format!("no thread {thread_id}: no such top-level engine"))?
    };
    let to = match to.map(str::trim).filter(|t| !t.is_empty()) {
        Some(name) => {
            if !targets(t).iter().any(|p| p.id == name) {
                let on: Vec<&str> = targets(t).iter().map(|p| p.id.as_str()).collect();
                return Err(format!("{name} is not on this thread; its engines: {}", if on.is_empty() { "none".into() } else { on.join(", ") }));
            }
            Some(name.to_string())
        }
        None => None,
    };
    if text.trim().is_empty() {
        return Err("nothing to send: the text is empty".into());
    }
    Ok((t.id.clone(), crate::shared::Edit::Post { author: YOU.into(), at, to, text: text.trim().to_string() }))
}

/// Cursor helper for tests and shells: where engine `i` has heard to.
pub fn heard(state: &State, i: usize) -> Option<Cursor> {
    state.engines.get(i).and_then(|e| e.heard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::Message;
    use crate::shared::{Edit, apply};

    fn post(state: &mut State, root: &str, author: &str, to: Option<&str>, text: &str) {
        let old = state.shared.get(root).cloned().unwrap_or_default();
        let (new, _) = apply(&old, &Edit::Post { author: author.into(), at: 1_795_356_330, to: to.map(String::from), text: text.into() }).unwrap();
        state.shared.insert(root.into(), new);
    }

    /// translator at the top, helper and tester its children.
    fn family() -> State {
        let mut s = State::default();
        for (name, parent) in [("translator", None), ("helper", Some("translator")), ("tester", Some("translator"))] {
            let i = s.add_engine();
            s.engines[i].id = name.into();
            s.engines[i].name = name.into();
            s.engines[i].parent = parent.map(String::from);
            s.engines[i].response = crate::response::Responses::from_md("format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n").unwrap().0;
            s.engines[i].format = crate::response::Format::parse_name("toon").unwrap();
        }
        s
    }

    fn idle(_: usize) -> Live {
        Live { status: Status::Idle, label: "idle".into(), busy: false }
    }

    #[test]
    fn the_tree_and_one_thread_per_root_come_from_the_engines() {
        let mut s = family();
        let d = build(&s, &idle);
        let tree: Vec<(&str, usize, &str)> = d.tree.iter().map(|n| (n.id.as_str(), n.depth, n.thread.as_str())).collect();
        assert_eq!(tree, [("", 0, "translator"), ("translator", 1, "translator"), ("helper", 2, "translator"), ("tester", 2, "translator")]);
        assert_eq!(d.threads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["translator"]);
        let t = &d.threads[0];
        let who: Vec<(&str, bool)> = t.participants.iter().map(|p| (p.id.as_str(), p.head)).collect();
        assert_eq!(who, [(YOU, false), ("translator", true), ("helper", false), ("tester", false)]);
        assert_eq!((t.title.as_str(), t.default_to.as_deref()), ("translator's thread", Some("translator")));
        // A grandchild joins the root's thread: no thread of its own.
        let i = s.add_engine();
        s.engines[i].id = "deep".into();
        s.engines[i].parent = Some("helper".into());
        let d = build(&s, &idle);
        assert_eq!(d.threads.len(), 1);
        assert_eq!(d.tree.iter().find(|n| n.id == "deep").map(|n| (n.depth, n.thread.as_str())), Some((3, "translator")));
        assert!(d.threads[0].participants.iter().any(|p| p.id == "deep"));
        // A new top-level engine is a new thread.
        let j = s.add_engine();
        s.engines[j].id = "other".into();
        let d = build(&s, &idle);
        assert_eq!(d.threads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["translator", "other"]);
        assert!(!d.threads[0].participants.iter().any(|p| p.id == "other"));
        assert_eq!(d.thread(Some("other")).map(|t| t.id.as_str()), Some("other"));
        assert_eq!(d.thread(Some("ghost")).map(|t| t.id.as_str()), Some("translator"), "else the first");
        let pick = |th: Option<&str>, to: Option<&str>| d.pick(th, to).map(|(t, to)| (t.id.clone(), to));
        assert_eq!(pick(None, Some("deep")), Some(("translator".into(), Some("deep".into()))), "a node opens its root's thread");
        assert_eq!(pick(Some("other"), Some("deep")), Some(("other".into(), Some("other".into()))), "not on it: the head");
        assert_eq!(pick(None, None), Some(("translator".into(), Some("translator".into()))));
    }

    #[test]
    fn only_a_guided_post_calls_an_engine_and_each_once() {
        let mut s = family();
        // Nothing for anyone: nobody is called.
        for i in 0..3 {
            assert!(guided(&mut s, i).is_empty());
        }
        post(&mut s, "translator", "helper", Some("translator"), "for the head");
        post(&mut s, "translator", "helper", Some("tester"), "run the suite");
        post(&mut s, "translator", YOU, Some("translator"), "make it green");
        post(&mut s, "translator", "agent-1", None, "a sub-agent started");
        let wake = guided(&mut s, 0);
        assert_eq!(wake, [format!("{WAKE_HEAD}\n[thread 1] helper → translator: for the head\n[thread 3] you → translator: make it green")]);
        assert!(guided(&mut s, 0).is_empty(), "each post is delivered once");
        assert_eq!(guided(&mut s, 2), [format!("{WAKE_HEAD}\n[thread 2] helper → tester: run the suite")]);
        assert!(guided(&mut s, 1).is_empty(), "helper wrote them; nothing is for it");
        // A store from before the rule: what is there counts as heard.
        s.engines[1].heard = None;
        post(&mut s, "translator", "tester", Some("helper"), "old");
        assert!(guided(&mut s, 1).is_empty());
        post(&mut s, "translator", "tester", Some("helper"), "new");
        assert_eq!(guided(&mut s, 1), [format!("{WAKE_HEAD}\n[thread 6] tester → helper: new")]);
    }

    #[test]
    fn an_unaddressed_post_bounces_to_its_sender_once_but_never_yours_or_a_reports() {
        let mut s = family();
        post(&mut s, "translator", "helper", None, "found it");
        post(&mut s, "translator", YOU, None, "anyone?");
        post(&mut s, "translator", "agent-1", None, "a sub-agent started");
        let wake = guided(&mut s, 1);
        assert_eq!(wake.len(), 1);
        assert!(
            wake[0].starts_with(&format!("{WAKE_HEAD}\n[thread 1] Not delivered — post #1 had no `to`. Address it to one of: you, translator, tester, with the context")),
            "{wake:?}"
        );
        assert!(wake[0].ends_with("It said: found it"), "{wake:?}");
        assert!(guided(&mut s, 1).is_empty(), "the bounce is delivered once");
        assert!(guided(&mut s, 0).is_empty() && guided(&mut s, 2).is_empty(), "nobody else is woken by a post with no to");
        let d = build(&s, &idle);
        let m = &d.threads[0].messages;
        assert_eq!((m[0].bounce, m[0].lost, m[0].work.as_ref().map(|w| w.engine.as_str())), (true, false, Some("helper")));
        assert_eq!((m[1].bounce, m[1].lost, m[1].work.is_none()), (false, true, true), "yours is kept, not delivered");
        assert_eq!((m[2].bounce, m[2].lost, m[2].work.is_none()), (false, false, true), "a report calls nobody");
    }

    #[test]
    fn a_thread_shows_each_post_with_the_turn_it_started() {
        let mut s = family();
        post(&mut s, "translator", "translator", Some("helper"), "find the bug");
        let wake = guided(&mut s, 1);
        s.add_text(1, &wake[0]);
        s.engines[1].messages.push(Message { role: Role::Assistant, text: "thought: Read it first.\nact: read(name=\"test\"); run(command=\"cargo test\")".into() });
        let busy = |i: usize| Live { busy: i == 1, ..idle(i) };
        let d = build(&s, &busy);
        let m = &d.threads[0].messages[0];
        assert_eq!((m.from.as_str(), m.to.as_deref()), ("translator", Some("helper")));
        let w = m.work.as_ref().unwrap();
        assert_eq!(w.phase, Phase::Running);
        assert_eq!(w.items[0], Item::Thought("Read it first.".into()));
        let Item::Call(c) = &w.items[1] else { panic!() };
        assert_eq!((c.name.as_str(), c.args.as_str(), c.phase.clone()), ("read", "name=\"test\"", Phase::Running));
        // The results come in and it replies.
        s.engines[1].messages.push(Message { role: Role::Tool, text: "read(name=\"test\") → assert!(x)\nrun(command=\"cargo test\") → error: 1 failed\nmore".into() });
        s.engines[1].messages.push(Message { role: Role::Assistant, text: "thought: Found it.\nact: reply(text=\"A missing trim.\")".into() });
        let d = build(&s, &idle);
        let w = d.threads[0].messages[0].work.clone().unwrap();
        assert_eq!((w.phase, w.reply.as_deref()), (Phase::Done, Some("A missing trim.")));
        let Item::Call(c) = &w.items[2] else { panic!() };
        assert_eq!((c.result.as_str(), c.phase.clone()), ("error: 1 failed", Phase::Failed("1 failed".into())));
        assert!(c.full.ends_with("1 failed\nmore"));
        // A post not delivered yet is queued; one for you calls nobody.
        post(&mut s, "translator", "tester", Some("helper"), "again");
        post(&mut s, "translator", "translator", Some(YOU), "for you");
        let d = build(&s, &idle);
        assert_eq!(d.threads[0].messages[1].work.as_ref().map(|w| w.phase.clone()), Some(Phase::Queued));
        assert_eq!((d.threads[0].messages[2].to.as_deref(), d.threads[0].messages[2].work.is_none()), (Some(YOU), true));
    }

    #[test]
    fn your_post_goes_to_an_engine_of_the_thread() {
        let s = family();
        let (_, Edit::Post { author, to, .. }) = your_post(&s, "translator", Some("translator"), "hi", 1).unwrap() else { panic!() };
        assert_eq!((author.as_str(), to.as_deref()), (YOU, Some("translator")));
        let (_, Edit::Post { to, .. }) = your_post(&s, "translator", None, "hi", 1).unwrap() else { panic!() };
        assert_eq!(to, None, "kept, for nobody");
        assert_eq!(your_post(&s, "", Some("helper"), "hi", 1).unwrap().0, "translator", "the old top route: the first thread");
        let (_, Edit::Post { to, .. }) = your_post(&s, "translator", Some("tester"), "hi", 1).unwrap() else { panic!() };
        assert_eq!(to.as_deref(), Some("tester"));
        assert!(your_post(&s, "translator", Some("ghost"), "hi", 1).unwrap_err().contains("not on this thread"));
        assert!(your_post(&s, "helper", None, "hi", 1).is_err(), "a thread is a root's");
        assert!(your_post(&s, "nobody", None, "hi", 1).is_err());
        assert!(your_post(&s, "translator", Some("helper"), " ", 1).is_err());
    }
}
