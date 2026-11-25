//! `kind: engines`: engines as tools. A parent makes child engines, each
//! with a goal, and talks to them; every engine of one root's tree is on
//! the root's thread with the user (`crate::shared`).
//!
//! ```text
//! artifacts/kids.md
//! kind: engines
//! child: helper call 1
//! child: tester start 0
//! ```
//!
//! One line per child: its id, how the parent last asked it (`call`: the
//! parent's turn waits for the reply; `start`: a message comes when it
//! replies) and how many of its replies the parent has had.
//!
//! **Tools** (only while the artifact is there; the tool-count rule):
//! - `engine_call(name, goal?, text?, ...)`: make the child (with `goal`)
//!   if it is new, give it `text` (or "work on your goal"), and wait: the
//!   result is the child's reply, `helper replied: "..."`. The parent's
//!   turn pauses (`Step::Wait`) and goes on when the reply is in.
//! - `engine_start(...)`: the same, but returns at once
//!   (`made helper ...; a message comes when it replies`). When the child
//!   replies, the message `engine helper replied: "..." (engines kids)` is
//!   added and the parent runs a turn, the way a terminal `start` calls
//!   back. A callback is a new turn, never a budget.
//!
//! **Where children live**: in the same flat list as every engine, each
//! in its own folder at the top of the store, with its parent in `app.md`
//! (`- helper (child of engine-1)`). So the shell runs a child's turns
//! exactly as any engine's, and its logs and files sit where every
//! engine's do.
//!
//! **What a child gets**: the goal the parent wrote, as its `goal.md`
//! (`goal`, `scope`, `done` become its sections); the parent's model and
//! settings (inference, sampling, context), response contract, format and
//! own tools; a `kind: shared` artifact for its thread; and,
//! when asked, a template (`template`), some of the parent's skills
//! (`skills`) and more kinds (`kinds`: engines, learning, clock), so a
//! child may make its own children. No depth limit and no step limit; a
//! child named as its parent (or the parent's parent ...) is refused.
//!
//! Pure. What the shell knows (the other engines, who is busy) comes in as
//! a `Family`; what must change outside this engine (a child made, a
//! message sent, a post) goes out as its `effects`, which `apply` makes.

use crate::calls::Call;
use crate::inference::Role;
use crate::state::{Engine, State};

/// The tools an engines artifact brings.
pub const TOOLS: [&str; 2] = ["engine_call", "engine_start"];

/// Kinds a child may hold besides `shared` (`kinds="..."`).
pub const CHILD_KINDS: [&str; 3] = ["engines", "learning", "clock"];

/// What a child is told when the parent gives no text.
pub const DEFAULT_TEXT: &str = "Work on your goal (see your instructions), then reply with the result.";

/// Most characters of a child's reply in a result or callback; all of it
/// is in the engines artifact's snapshot (up to `SNAPSHOT_REPLY`).
pub const REPLY_CHARS: usize = 400;
pub const SNAPSHOT_REPLY: usize = 4_000;

/// How the shell sees an engine now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// Its activity, as a label (`idle`, `calling the LLM`).
    pub label: String,
    /// A turn is running: no message may be added now.
    pub busy: bool,
    /// Its last turn failed: why.
    pub failed: Option<String>,
}

/// A child, as its parent sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Child {
    pub id: String,
    pub name: String,
    /// Its Goal section's first line.
    pub goal: String,
    pub seen: Seen,
    /// How many replies it has given.
    pub replies: usize,
    pub last_reply: Option<String>,
}

/// What the shell knows about an engine's family at a step: its thread,
/// its children. It goes into the render
/// (`live::Snapshots`) and the step, which fills `effects`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Family {
    /// The engine itself.
    pub id: String,
    /// Its thread: its root's id (`crate::shared::root_of`), the head.
    pub thread: String,
    /// The thread's other engines (the head and its whole tree).
    pub others: Vec<String>,
    /// Its thread, as at the step's start (posts in the step are applied
    /// to it too).
    pub shared: String,
    pub children: Vec<Child>,
    /// Every engine id (a new child's id must be new).
    pub taken: Vec<String>,
    /// Its parent, its parent's parent, ...
    pub ancestors: Vec<String>,
    /// The clock's offset east of UTC, for times in snapshots.
    pub offset: i32,
    /// Out: what the shell does after the step (`apply`).
    pub effects: Vec<Effect>,
}

/// A change outside the engine a step asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// To its thread.
    Post(crate::shared::Edit),
    /// Give a child (made first when `new`) a message, and run it.
    Ask(Ask),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub child: String,
    pub name: String,
    pub new: Option<NewChild>,
    pub text: String,
}

/// What a new child is made with.
#[derive(Clone, Debug, PartialEq)]
pub struct NewChild {
    /// Its `goal.md`.
    pub goal: String,
    pub template: String,
    pub skills: Vec<String>,
    pub kinds: Vec<String>,
    /// From a template (`crate::kit`): its skills, its artifacts (roots
    /// set), its direct tools (`None`: the parent's) and its name.
    pub kit_skills: std::collections::BTreeMap<String, String>,
    /// The tools those skills bring (`crate::skilltool`), by skill.
    pub kit_skill_tools: std::collections::BTreeMap<String, String>,
    pub artifacts: std::collections::BTreeMap<String, String>,
    pub tools: Option<Vec<String>>,
    /// The template's `elements.md`: the extra elements the child holds.
    pub elements: String,
    pub from: Option<String>,
    /// What `model=` chose (`crate::subscriptions::Subscriptions::choose`):
    /// a model row for its inference, or a session row (the child is a
    /// session engine of that program). `None`: the parent's model.
    pub model: Option<Box<crate::subscriptions::Choice>>,
}

/// Engine `i`'s family in `state`; `seen` says how the shell sees each
/// engine (by index).
pub fn family(state: &State, i: usize, seen: &dyn Fn(usize) -> Seen) -> Family {
    let e = &state.engines[i];
    let thread = crate::shared::root_of(state, &e.id);
    let mut ancestors = Vec::new();
    let mut at = e.parent.clone();
    while let Some(p) = at.filter(|p| !ancestors.contains(p) && *p != e.id) {
        ancestors.push(p.clone());
        at = state.engines.iter().find(|o| o.id == p).and_then(|o| o.parent.clone());
    }
    let children = state
        .engines
        .iter()
        .enumerate()
        .filter(|(_, c)| c.parent.as_deref() == Some(e.id.as_str()))
        .map(|(j, c)| {
            let replies = replies(c);
            Child {
                id: c.id.clone(),
                name: c.name.clone(),
                goal: crate::goal::summary(&c.description),
                seen: seen(j),
                replies: replies.len(),
                last_reply: replies.last().cloned(),
            }
        })
        .collect();
    Family {
        id: e.id.clone(),
        others: crate::shared::members(state, &thread).into_iter().map(|j| state.engines[j].id.clone()).filter(|id| *id != e.id).collect(),
        shared: state.shared.get(&thread).cloned().unwrap_or_default(),
        thread,
        children,
        taken: state.engines.iter().map(|o| o.id.clone()).collect(),
        ancestors,
        offset: 0,
        effects: Vec::new(),
    }
}

/// How a reply's components are named to its parent (`replies`).
pub const COMPONENTS: &str = "its components, to pass on as written";

/// The replies engine `e` gave, oldest first: each `reply(text)` of its
/// answers (with no contract, each answer). A reply that shows or asks
/// with a component carries it on a last line, in the call grammar, so
/// the parent can pass it on to the person (`turn::reply_components`).
pub fn replies(e: &Engine) -> Vec<String> {
    e.messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .filter_map(|m| {
            if e.response.is_empty() {
                return Some(m.text.clone());
            }
            let text = crate::turn::reply_in(e, &m.text)?;
            Some(match crate::turn::reply_components(e, &m.text) {
                Some(c) => format!("{text}\n({COMPONENTS}: {c})"),
                None => text,
            })
        })
        .collect()
}

/// One `child:` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    /// `true`: `engine_call` (waits); `false`: `engine_start`.
    pub call: bool,
    pub seen: usize,
    /// Its parent was told that its last turn failed (a fourth word,
    /// `told`): a failure is told once, until the child works again.
    pub told: bool,
}

/// An engines artifact's lines.
pub fn parse(text: &str) -> Vec<Line> {
    text.lines()
        .filter_map(|l| {
            let mut w = l.trim().strip_prefix("child:")?.split_whitespace();
            let id = w.next()?.to_string();
            let call = w.next()? == "call";
            let seen = w.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            Some(Line { id, call, seen, told: w.next() == Some("told") })
        })
        .collect()
}

pub fn to_text(lines: &[Line]) -> String {
    let mut out = "kind: engines\n".to_string();
    for l in lines {
        let told = if l.told { " told" } else { "" };
        out.push_str(&format!("child: {} {} {}{told}\n", l.id, if l.call { "call" } else { "start" }, l.seen));
    }
    out
}

/// Stop waiting for the children and jobs engine `e` waits for (its turn
/// ended another way: it replied, or a new message came): they call back
/// with a message instead. The ids (and `term#3` job entries) it waited
/// for.
pub fn stop_waiting(e: &mut Engine) -> Vec<String> {
    let waits = std::mem::take(&mut e.waiting);
    if waits.is_empty() {
        return waits;
    }
    for text in e.artifacts.values_mut().filter(|t| crate::live::Live::of(t) == Some(crate::live::Live::Engines)) {
        let mut lines = parse(text);
        for l in lines.iter_mut().filter(|l| waits.contains(&l.id)) {
            l.call = false;
        }
        *text = to_text(&lines);
    }
    // A job waited on (`start(..., wait="yes")`): it calls back as a plain start.
    crate::terminal::release(e, &waits);
    let note = |id: &str| format!("{id} is working; you replied, so a message comes when it replies");
    for m in e.messages.iter_mut().rev().filter(|m| m.role == Role::Tool).take(1) {
        for id in &waits {
            m.text = m.text.replace(&waiting_text(id), &note(id));
        }
    }
    waits
}

/// What a wait entry is waiting for, for people: `helper's reply`, or
/// `session 3 (terminal term)` for a job (`start(..., wait="yes")`).
pub fn wait_label(entry: &str) -> String {
    crate::terminal::wait_label(entry).unwrap_or_else(|| format!("{entry}'s reply"))
}

/// The placeholder result of an `engine_call` until the reply is in.
pub fn waiting_text(id: &str) -> String {
    format!("waiting for {id}'s reply")
}

/// `"…"`, cut at `max` characters with a note.
fn quote(text: &str, max: usize, artifact: &str) -> String {
    let n = text.chars().count();
    if n <= max {
        format!("\"{}\"", text.trim())
    } else {
        // Say only what the snapshot holds: past SNAPSHOT_REPLY it is cut too.
        let held = if n <= SNAPSHOT_REPLY { "all of it is in the".to_string() } else { format!("the first {SNAPSHOT_REPLY} are in the") };
        format!("\"{}…\" ({n} characters; {held} {artifact} artifact)", text.chars().take(max).collect::<String>().trim_end())
    }
}

/// `engine_call` / `engine_start` on engine `e`'s engines artifact
/// `artifact`. The result line; the child is made and asked by the
/// shell (`effects`).
pub fn call(e: &mut Engine, artifact: &str, call: &Call, fam: &mut Family) -> Result<String, String> {
    let wait = call.name == TOOLS[0];
    let name = call.arg("name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| format!("`{}` needs name=\"...\"", call.name))?;
    let id = crate::files::new_id(name, &[]);
    if id == e.id || fam.ancestors.contains(&id) {
        return Err(format!("`{id}` is you or one of your parents: an engine cannot be its own child"));
    }
    if fam.effects.iter().any(|x| matches!(x, Effect::Ask(a) if a.child == id)) {
        return Err(format!("{id} was already asked in this answer; wait for its reply"));
    }
    let child = fam.children.iter().find(|c| c.id == id).cloned();
    let (new, kept) = match &child {
        Some(c) if c.seen.busy => {
            return Err(format!("{id} is still working; its reply comes first (as a message, or as the result of your engine_call)"));
        }
        Some(_) => (None, call.arg("goal").is_some()),
        None if fam.taken.contains(&id) => return Err(format!("`{id}` is an engine that is not your child; pick another name")),
        None => {
            let goal = call
                .arg("goal")
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .ok_or_else(|| format!("{id} is new: give it a goal=\"...\" (and scope=\"...\", done=\"...\" if you like)"))?;
            let list = |key: &str| -> Vec<String> {
                call.arg(key).map_or(Vec::new(), |l| l.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
            };
            let given = call.arg("template").unwrap_or("").trim();
            let root = call.arg("root").map(str::trim).filter(|r| !r.is_empty());
            if root.is_some_and(|r| r.contains('\n')) {
                return Err("root is one folder: a path on one line".into());
            }
            // In the browser's VM a root is a folder of the workspace: the
            // workspace's full path (`/work`, as the environment line shows
            // it) is written as that folder (`.`), so the child's artifacts
            // read like any other and its files show in the workbench.
            let in_vm = e.machine.0.environment.as_ref().is_some_and(|env| env.kind == crate::environment::Kind::BrowserVm);
            let vm_root = match root {
                Some(r) if in_vm => Some(crate::environment::vm::folder(r)?),
                _ => None,
            };
            let root = vm_root.as_deref().or(root);
            // A template's name (`crate::kit`): the child is made from it.
            let kit = crate::kit::kits(e).remove(given);
            let kit_skill_tools = kit.as_ref().map(|k| k.skill_tools.clone()).unwrap_or_default();
            let (template, kit_skills, artifacts, tools, elements) = match &kit {
                Some(k) => {
                    if let Some(why) = k.problem() {
                        return Err(why);
                    }
                    (k.template.clone(), k.skills.clone(), k.artifacts_at(root), k.tools.clone(), k.elements.clone())
                }
                None if root.is_some() => {
                    return Err(format!(
                        "root= goes with a template's name; yours: {}",
                        crate::kit::kits(e).keys().cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
                None => {
                    crate::template::parse(given).map_err(|why| format!("the template does not read: {why}"))?;
                    (given.to_string(), Default::default(), Default::default(), None, String::new())
                }
            };
            let skills = list("skills");
            if let Some(s) = skills.iter().find(|s| !e.skills.contains_key(*s)) {
                return Err(format!("you have no skill `{s}` to give; yours: {}", e.skills.keys().cloned().collect::<Vec<_>>().join(", ")));
            }
            let kinds = list("kinds");
            if let Some(k) = kinds.iter().find(|k| !CHILD_KINDS.contains(&k.as_str())) {
                return Err(format!("a child may also hold {}, not `{k}`", CHILD_KINDS.join(", ")));
            }
            let model = choose(e, call.arg("model"))?.map(Box::new);
            let goal = crate::goal::write(goal, call.arg("scope").unwrap_or(""), call.arg("done").unwrap_or(""));
            let from = kit.map(|k| k.name);
            (Some(NewChild { goal, template, skills, kinds, kit_skills, kit_skill_tools, artifacts, tools, elements, from, model }), false)
        }
    };
    // A new child's first message asks for the work, then gives what its
    // parent wrote. The parent's text is context ("nothing exists yet; use
    // the standard library"), and context alone is not an ask: a model
    // that reads only that may answer nothing. A message to a child that
    // exists is sent as written.
    let mut text = match (call.arg("text").map(str::trim).filter(|t| !t.is_empty()), new.is_some()) {
        (None, _) => DEFAULT_TEXT.to_string(),
        (Some(given), true) => format!("{DEFAULT_TEXT}\n\n{given}"),
        (Some(given), false) => given.to_string(),
    };
    // A session's program reads no goal.md: its brief is its first message.
    if let Some(n) = new.as_ref().filter(|n| matches!(n.model.as_deref(), Some(crate::subscriptions::Choice::Session { .. }))) {
        text = format!("{}\n\n{text}", n.goal.trim());
    }
    let model_kept = child.is_some() && call.arg("model").is_some_and(|m| !m.trim().is_empty());
    // The artifact says how it was asked and how many replies were had.
    let state = e.artifacts.get_mut(artifact).ok_or("no engines artifact")?;
    let mut lines = parse(state);
    let seen = child.as_ref().map_or(0, |c| c.replies);
    match lines.iter_mut().find(|l| l.id == id) {
        Some(l) => *l = Line { id: id.clone(), call: wait, seen, told: false },
        None => lines.push(Line { id: id.clone(), call: wait, seen, told: false }),
    }
    *state = to_text(&lines);
    let made = new.is_some();
    let mut from = new.as_ref().and_then(|n| n.from.clone()).map_or(String::new(), |k| format!(" from template {k}"));
    match new.as_ref().and_then(|n| n.model.as_deref()) {
        Some(crate::subscriptions::Choice::Model { name, .. }) => from.push_str(&format!(" on {name}")),
        Some(crate::subscriptions::Choice::Session { tool }) => from.push_str(&format!(" as a {tool} session")),
        None => {}
    }
    fam.effects.push(Effect::Ask(Ask { child: id.clone(), name: name.to_string(), new, text }));
    let keeps = match (kept, model_kept) {
        (true, true) => " (it keeps its goal and its model; a person changes those)",
        (true, false) => " (it keeps its goal; its goal.md is changed by a person)",
        (false, true) => " (it keeps its model: model= is for a new child)",
        (false, false) => "",
    };
    if wait {
        e.waiting.push(id.clone());
        return Ok(waiting_text(&id));
    }
    Ok(if made {
        format!("made {id}{from} with your goal and started it; a message comes when it replies")
    } else {
        format!("sent {id} your message{keeps}; a message comes when it replies")
    })
}

/// What `model=` names (a row of the subscriptions the shell reported),
/// or why it cannot be used. Left out: `None` (the parent's model).
fn choose(e: &Engine, model: Option<&str>) -> Result<Option<crate::subscriptions::Choice>, String> {
    let Some(name) = model.map(str::trim).filter(|m| !m.is_empty()) else { return Ok(None) };
    let subs = e.machine.0.subscriptions.as_ref().ok_or_else(|| {
        format!("model=\"{name}\": this shell reported no subscriptions, so no row can be chosen; leave model out for your own model")
    })?;
    subs.choose(name).map(Some)
}

/// Make the child `ask` names for engine `parent` (`state.engines[parent]`):
/// the parent's model, settings, contract and tools, its goal, a shared
/// artifact. Its index.
fn make(state: &mut State, parent: usize, ask: &Ask, new: &NewChild) -> usize {
    let p = &state.engines[parent];
    let mut c = Engine::new(&ask.name, new.goal.trim(), p.inference.clone());
    c.id = ask.child.clone();
    c.parent = Some(p.id.clone());
    c.response = p.response.clone();
    c.format = p.format.clone();
    c.tools = new.tools.clone().unwrap_or_else(|| p.tools.clone());
    c.machine = p.machine.clone();
    c.template = new.template.clone();
    c.skills = new.kit_skills.clone();
    c.skill_tools = new.kit_skill_tools.clone();
    c.elements = new.elements.clone();
    for s in &new.skills {
        if let Some(text) = p.skills.get(s) {
            c.skills.insert(s.clone(), text.clone());
        }
        // A skill handed down keeps the tools it brings.
        if let Some(text) = p.skill_tools.get(s) {
            c.skill_tools.insert(s.clone(), text.clone());
        }
    }
    match new.model.as_deref() {
        Some(crate::subscriptions::Choice::Model { inference, .. }) => {
            // The chosen model as models.json sets it (its own sampling
            // defaults: the parent's settings are for the parent's model).
            c.inference = inference.clone();
        }
        Some(crate::subscriptions::Choice::Session { tool }) => {
            // A session engine, as New session makes one (`State::add_session`).
            c.session = Some(crate::session::SessionRef { tool: tool.clone(), id: None });
            c.allow = crate::approval::DEFAULT_ALLOW.to_string();
        }
        None => {}
    }
    c.artifacts = new.artifacts.clone();
    c.artifacts.insert("shared".into(), "kind: shared".into());
    for k in &new.kinds {
        c.artifacts.insert(k.clone(), format!("kind: {k}"));
    }
    state.engines.push(c);
    state.engines.len() - 1
}

/// What `apply` did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    /// The engines to run now (each was given a message).
    pub run: Vec<usize>,
    /// What could not be done (a post that no longer fits).
    pub notes: Vec<String>,
}

/// Make engine `i`'s step's `effects`: posts on its thread,
/// children made and given their messages. The shell runs `run`.
pub fn apply(state: &mut State, i: usize, effects: Vec<Effect>) -> Applied {
    let mut out = Applied::default();
    let id = state.engines[i].id.clone();
    for effect in effects {
        match effect {
            Effect::Post(edit) => {
                if let Err(why) = crate::shared::post(state, &id, &edit) {
                    out.notes.push(format!("a post was not kept: {why}"));
                }
            }
            Effect::Ask(ask) => {
                let child = match (state.index_of(&ask.child), &ask.new) {
                    (Some(c), _) => c,
                    (None, Some(new)) => make(state, i, &ask, new),
                    (None, None) => {
                        out.notes.push(format!("{} is gone", ask.child));
                        continue;
                    }
                };
                if state.add_text(child, &ask.text) {
                    out.run.push(child);
                }
            }
        }
    }
    out
}

/// What `settle` found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settled {
    /// Messages for a new turn: children asked with `engine_start` that
    /// replied.
    pub wake: Vec<String>,
    /// Every child its turn waited for has answered: go on with the turn.
    pub resumed: bool,
}

/// The parent `parent`'s children that replied since it last heard: an
/// `engine_call` it waits for gets its result, an `engine_start` one is a
/// wake message. Each reply is had once (the artifact counts them).
pub fn settle(state: &mut State, parent: usize, seen: &dyn Fn(usize) -> Seen) -> Settled {
    let mut out = Settled::default();
    let mut answered = false;
    // Waiting at the start: callbacks wait for the turn, even once the
    // last awaited reply is in during this pass.
    let waited = !state.engines[parent].waiting.is_empty();
    let names: Vec<String> = crate::live::present(&state.engines[parent])
        .into_iter()
        .filter(|(_, k)| *k == crate::live::Live::Engines)
        .map(|(n, _)| n)
        .collect();
    for artifact in names {
        let mut lines = parse(&state.engines[parent].artifacts[&artifact]);
        let mut changed = false;
        for line in &mut lines {
            let Some(c) = state.index_of(&line.id) else { continue };
            let now = seen(c);
            // It works again (or works now): a later failure is news.
            if line.told && (now.busy || now.failed.is_none()) {
                line.told = false;
                changed = true;
            }
            if now.busy {
                continue;
            }
            let waits = state.engines[parent].waiting.contains(&line.id);
            // A callback waits while the parent's turn waits for another.
            if !waits && waited {
                continue;
            }
            let replies = replies(&state.engines[c]);
            let result = if replies.len() > line.seen {
                line.seen = replies.len();
                changed = true;
                let reply = replies.last().expect("more than seen");
                Ok(format!("{} replied: {}", line.id, quote(reply, REPLY_CHARS, &artifact)))
            } else if let (false, Some(why)) = (line.told, now.failed) {
                // A failed child calls back once, as a reply does: a
                // parent that did not wait hears it too.
                line.told = true;
                changed = true;
                let why = if why.chars().count() > REPLY_CHARS { format!("{}…", why.chars().take(REPLY_CHARS).collect::<String>()) } else { why };
                Err(format!("error: {} failed: {why}", line.id))
            } else {
                continue;
            };
            let p = &mut state.engines[parent];
            if waits {
                let placeholder = format!("→ {}", waiting_text(&line.id));
                let with = format!("→ {}", result.unwrap_or_else(|e| e));
                if let Some(m) = p.messages.iter_mut().rev().find(|m| m.role == Role::Tool && m.text.contains(&placeholder)) {
                    m.text = m.text.replacen(&placeholder, &with, 1);
                }
                p.waiting.retain(|w| *w != line.id);
                answered = true;
            } else {
                let text = match result {
                    Ok(text) => text,
                    Err(e) => e.trim_start_matches("error: ").to_string(),
                };
                out.wake.push(format!("engine {text} (engines {artifact})"));
            }
        }
        if changed {
            state.engines[parent].artifacts.insert(artifact.clone(), to_text(&lines));
        }
    }
    out.resumed = answered && state.engines[parent].waiting.is_empty();
    out
}

/// The engines artifact's snapshot: each child with its goal, activity
/// and last reply, then their shared context.
pub fn snapshot(text: &str, e: &Engine, fam: &Family) -> String {
    let lines = parse(text);
    let mut out = String::from(
        "Your child engines. engine_call(name, goal) makes one and waits for its reply; engine_start(name, goal) makes one and returns at once, and a message comes when it replies. With a child that exists, the same calls send it text=\"...\".",
    );
    let kits = crate::kit::list(e);
    if !kits.is_empty() {
        out.push_str(&format!("\n\n{kits}"));
    }
    if fam.children.is_empty() {
        out.push_str("\n\nNo children yet.");
        return out;
    }
    for c in &fam.children {
        let how = match lines.iter().find(|l| l.id == c.id) {
            Some(_) if e.waiting.contains(&c.id) => " · your turn waits for its reply",
            Some(l) if !l.call => " · a message comes when it replies",
            _ => "",
        };
        let state = c.seen.failed.as_ref().map_or_else(|| c.seen.label.clone(), |why| format!("failed: {why}"));
        out.push_str(&format!(
            "\n\n#### {} — {state} · {} replies{how}\ngoal: {}",
            c.id,
            c.replies,
            if c.goal.is_empty() { "(none)" } else { &c.goal }
        ));
        if let Some(r) = &c.last_reply {
            let shown: String = r.chars().take(SNAPSHOT_REPLY).collect();
            let more = if r.chars().count() > SNAPSHOT_REPLY { " (cut)" } else { "" };
            out.push_str(&format!("\nlast reply{more}: {}", shown.trim()));
        }
    }
    let entries = crate::shared::parse(&fam.shared);
    out.push_str(&format!(
        "\n\n#### the thread — {} entries, {} characters\n{}",
        entries.len(),
        crate::shared::used(&entries),
        crate::shared::list(&fam.shared, fam.offset)
    ));
    out
}

/// `post` / `edit_post` on engine `e`'s thread, at unix `now`.
pub fn shared_call(e: &Engine, call: &Call, fam: &mut Family, now: i64) -> Result<String, String> {
    let edit = if call.name == crate::shared::TOOLS[0] {
        crate::shared::Edit::Post {
            author: e.id.clone(),
            at: now,
            to: call.arg("to").map(str::to_string),
            text: call.arg("text").unwrap_or("").to_string(),
        }
    } else {
        let n = call.arg("n").and_then(|n| n.trim().parse().ok()).ok_or("`edit_post` needs n=\"...\" (the entry's number)")?;
        crate::shared::Edit::Change {
            author: e.id.clone(),
            n,
            text: call.arg("text").ok_or("`edit_post` needs text=\"...\" (\"\" removes it)")?.to_string(),
        }
    };
    let (text, result) = crate::shared::apply(&fam.shared, &edit)?;
    fam.shared = text;
    fam.effects.push(Effect::Post(edit));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::Message;
    use crate::turn::{NoHost, Step, step_in};

    /// One top-level engine with a contract and a `kids` engines artifact.
    fn parent() -> State {
        let mut s = State::default();
        let p = s.add_engine();
        let e = &mut s.engines[p];
        e.response = crate::response::Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.artifacts.insert("kids".into(), "kind: engines".into());
        s
    }

    fn idle(_: usize) -> Seen {
        Seen { label: "idle".into(), ..Seen::default() }
    }

    /// One step of engine `i` with a fake model's answer `act`, the way the
    /// shell runs it: the family in, the effects made after.
    fn step(s: &mut State, i: usize, act: &str) -> (Step, Applied) {
        let mut fam = family(s, i, &idle);
        let done = step_in(&mut s.engines[i], &format!("thought: x\nact: {act}"), &mut |_| None, &NoHost, Some(&mut fam));
        let applied = apply(s, i, fam.effects);
        (done, applied)
    }

    fn last(s: &State, i: usize) -> String {
        s.engines[i].messages.last().map(|m| m.text.clone()).unwrap_or_default()
    }

    /// The system prompt of engine `i` as sent, with its family.
    fn prompt(s: &State, i: usize) -> String {
        let fam = family(s, i, &idle);
        crate::prompt::render_with(&s.engines[i], &crate::live::Snapshots { family: Some(&fam), ..Default::default() }).messages()[0]
            .text
            .clone()
    }

    /// `parent()` on a machine whose shell reports the sample subscriptions
    /// (`crate::subscriptions::tests::sample`).
    fn subscribed() -> State {
        let mut s = parent();
        let mut reg = crate::capability::Registry::default();
        reg.subscriptions = Some(crate::subscriptions::tests::sample());
        s.equip(reg);
        s
    }

    #[test]
    fn model_picks_an_api_row_for_the_childs_inference() {
        let mut s = subscribed();
        s.add_text(0, "go");
        let (_, applied) = step(&mut s, 0, "engine_start(name=\"h\", goal=\"Sum it.\", model=\"local\")");
        assert!(last(&s, 0).contains("made h on local with your goal"), "{}", last(&s, 0));
        let c = &s.engines[applied.run[0]];
        assert_eq!((c.inference.model.as_str(), c.inference.base_url.as_str()), ("ripple-65b", "http://127.0.0.1:1234/v1"));
        assert_eq!(c.session, None);
        assert_eq!(c.messages.last().unwrap().text, DEFAULT_TEXT, "a model child reads its goal.md");
        // Left out: the parent's model, as before.
        let (_, applied) = step(&mut s, 0, "engine_start(name=\"k\", goal=\"Sum it.\")");
        assert_eq!(s.engines[applied.run[0]].inference, s.engines[0].inference);
    }

    #[test]
    fn model_picks_a_session_row_and_the_child_is_a_session_with_its_brief_first() {
        let mut s = subscribed();
        s.add_text(0, "go");
        let (_, applied) =
            step(&mut s, 0, "engine_start(name=\"big\", goal=\"Port the parser.\", done=\"Tests pass.\", text=\"Start with lexer.rs.\", model=\"claude\")");
        assert!(last(&s, 0).contains("made big as a claude session"), "{}", last(&s, 0));
        let c = &s.engines[applied.run[0]];
        assert_eq!(c.session, Some(crate::session::SessionRef { tool: "claude".into(), id: None }));
        assert_eq!(c.allow, crate::approval::DEFAULT_ALLOW);
        assert!(c.description.contains("Port the parser."), "its goal.md: {}", c.description);
        let first = &c.messages.last().unwrap().text;
        assert!(first.contains("Port the parser.") && first.contains("Tests pass.") && first.ends_with("Start with lexer.rs."), "{first}");
        assert_eq!(c.parent.as_deref(), Some(s.engines[0].id.as_str()), "a child on the thread like any other");
    }

    #[test]
    fn model_refuses_an_unknown_or_absent_row_with_the_present_ones() {
        let mut s = subscribed();
        s.add_text(0, "go");
        let count = s.engines.len();
        step(&mut s, 0, "engine_start(name=\"h\", goal=\"Sum it.\", model=\"gpt-9\")");
        let unknown = last(&s, 0);
        assert!(unknown.contains("model=\"gpt-9\" is not a subscription") && unknown.contains("present: local, claude/haiku, claude"), "{unknown}");
        step(&mut s, 0, "engine_call(name=\"h\", goal=\"Sum it.\", model=\"hermes\")");
        let absent = last(&s, 0);
        assert!(absent.contains("is absent here (hermes is not on PATH)"), "{absent}");
        assert_eq!(s.engines.len(), count, "no child made");
        assert!(s.engines[0].waiting.is_empty(), "nothing waits on a refused call");
        // A shell that reported none: refused, and why.
        let mut bare = parent();
        bare.add_text(0, "go");
        step(&mut bare, 0, "engine_start(name=\"h\", goal=\"Sum it.\", model=\"local\")");
        assert!(last(&bare, 0).contains("reported no subscriptions"), "{}", last(&bare, 0));
    }

    #[test]
    fn a_child_made_from_a_template_gets_its_phases_tools_skills_and_artifacts_at_the_root() {
        let mut s = crate::shipped::state().unwrap();
        s.add_text(0, "make the greeting use a comma in /tmp/hello");
        assert!(prompt(&s, 0).contains("- developer: Makes or changes code"), "the translator sees what it can make");
        let (_, applied) = step(
            &mut s,
            0,
            "engine_start(name=\"dev-greeting\", goal=\"The greeting has a comma.\", done=\"Tests pass.\", text=\"Start from greet.ts.\", template=\"developer\", root=\"/tmp/hello\")",
        );
        assert!(last(&s, 0).contains("made dev-greeting from template developer"), "{}", last(&s, 0));
        let c = &s.engines[applied.run[0]];
        assert_eq!(c.parent.as_deref(), Some("translator"));
        assert_eq!(c.messages[0].text, format!("{DEFAULT_TEXT}\n\nStart from greet.ts."), "its first message asks for the work, then gives the parent's text");
        assert_eq!(c.tools, ["reply", "write", "read", "list"], "the template's tools, not the translator's");
        assert!(c.skills.contains_key("triage") && !c.skills.contains_key("prompting"), "its own skills only");
        assert_eq!(c.phase_list().unwrap().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["triage", "code", "test"]);
        assert!(c.artifacts["code"].contains("root: /tmp/hello") && c.artifacts["terminal"].contains("root: /tmp/hello"));
        assert!(c.artifacts.contains_key("notes") && c.artifacts.contains_key("shared"));
        assert_eq!(c.elements.trim(), "environment", "the template's elements.md: it reads where its tools run");
        assert!(c.skill_tools.contains_key("python") && !c.skill_tools.contains_key("triage"), "a skill's own tools come with it");
        // root= without a template's name is refused, so it never moves a root by accident.
        step(&mut s, 0, "engine_start(name=\"other\", goal=\"x\", root=\"/tmp\")");
        assert!(last(&s, 0).contains("root= goes with a template's name"), "{}", last(&s, 0));
    }

    #[test]
    fn a_reviewer_made_from_its_template_cannot_edit_save_or_commit() {
        let mut s = crate::shipped::state().unwrap();
        s.add_text(0, "review the change in /tmp/hello");
        let (_, applied) = step(&mut s, 0, "engine_start(name=\"rev\", goal=\"Review it.\", template=\"reviewer\", root=\"/tmp/hello\")");
        let i = applied.run[0];
        assert!(!prompt(&s, i).contains("- edit(") && !prompt(&s, i).contains("- save("), "not in its prompt");
        // Hand-made calls anyway: each refused with why, nothing held or run.
        step(&mut s, i, "edit(path=\"greet.ts\", old=\"a\", new=\"b\"); save(path=\"greet.ts\"); run(command=\"git commit -am x\")");
        let out = last(&s, i);
        assert!(out.contains("edit(") && out.contains("→ error: `edit` is refused: your file artifact code is read-only"), "{out}");
        assert!(out.contains("→ error: `save` is refused"), "{out}");
        assert!(out.contains("→ error: refused: this terminal is read-only (uses: [terminal:read])"), "{out}");
        assert!(!s.engines[i].artifacts["code"].contains("edit:") && !s.engines[i].artifacts["terminal"].contains("session:"));
    }

    #[test]
    fn a_childs_reply_with_components_reaches_the_parent_with_them() {
        let mut s = parent();
        s.add_text(0, "the inner planets as a table");
        step(&mut s, 0, "engine_call(name=\"ans\", goal=\"Answer as a table.\")");
        let said = "reply(text=\"Smallest first.\", show=\"table\", data=\"Planet|Moons;Mercury|0;Earth|1\")";
        assert_eq!(step(&mut s, 1, said).0, Step::Reply("Smallest first.".into()));
        settle(&mut s, 0, &idle);
        let p = prompt(&s, 0);
        let line = "last reply: Smallest first.\n(its components, to pass on as written: show=\"table\", data=\"Planet|Moons;Mercury|0;Earth|1\")";
        assert!(p.contains(line), "{p}");
        // Passed on as written, the parent's reply shows the same table.
        let passed = "reply(text=\"Smallest first.\", show=\"table\", data=\"Planet|Moons;Mercury|0;Earth|1\")";
        step(&mut s, 0, passed);
        let shown = |i: usize| {
            let e = &s.engines[i];
            let m = e.messages.iter().rev().find(|m| m.role == Role::Assistant).unwrap();
            crate::ask::reply_call(e, &m.text).map(|c| crate::ask::Parsed::of(&c).show)
        };
        assert!(shown(0).flatten().is_some() && shown(0) == shown(1), "the same display in both");
        // Text alone carries no such line.
        assert_eq!(crate::turn::reply_components(&s.engines[1], "thought: x\nact: reply(text=\"hi\")"), None);
    }

    #[test]
    fn a_child_call_waits_and_its_reply_is_the_result() {
        let mut s = parent();
        s.add_text(0, "find the bug");
        assert!(prompt(&s, 0).contains("- engine_call(name=\"...\""), "the tools come with the artifact");
        assert!(prompt(&s, 0).contains("### kids — your child engines") && prompt(&s, 0).contains("No children yet."));

        let (done, applied) = step(&mut s, 0, "engine_call(name=\"helper\", goal=\"Find the bug.\", done=\"Reply with the line.\")");
        assert_eq!(done, Step::Wait(vec!["helper".into()]));
        assert_eq!(crate::activity::Activity::after(&done, 1).label(), "waiting for helper's reply");
        assert!(last(&s, 0).ends_with("→ waiting for helper's reply"), "{}", last(&s, 0));
        assert_eq!(applied.run, [1], "the child runs now");
        let child = &s.engines[1];
        assert_eq!((child.id.as_str(), child.parent.as_deref()), ("helper", Some("engine-1")));
        assert_eq!(child.description, "## Goal\nFind the bug.\n\n## Done\nReply with the line.", "its goal.md, in sections");
        assert_eq!(
            (child.inference.clone(), child.response.clone()),
            (s.engines[0].inference.clone(), s.engines[0].response.clone()),
            "the parent's model and contract"
        );
        assert_eq!(child.artifacts["shared"], "kind: shared");
        assert_eq!(child.messages[0].text, DEFAULT_TEXT);
        assert!(prompt(&s, 1).starts_with("## Instructions\n\n### Goal — what you are for, and who you serve\nFind the bug."));

        // The child's turn: it posts on its thread, then replies.
        assert_eq!(step(&mut s, 1, "post(text=\"The bug is turn.rs:40.\", to=\"you\")").0, Step::Again);
        assert!(last(&s, 1).ends_with("→ posted entry 1 (28 characters on the thread)"), "{}", last(&s, 1));
        assert_eq!(step(&mut s, 1, "reply(text=\"turn.rs:40, a missing trim.\")").0, Step::Reply("turn.rs:40, a missing trim.".into()));

        // The tick: the call gets its result, and the parent's turn goes on.
        let busy = |_: usize| Seen { busy: true, ..Seen::default() };
        assert_eq!(settle(&mut s, 0, &busy), Settled::default(), "a busy child is left for a later tick");
        let settled = settle(&mut s, 0, &idle);
        assert_eq!(settled, Settled { wake: vec![], resumed: true });
        assert!(last(&s, 0).ends_with("→ helper replied: \"turn.rs:40, a missing trim.\""), "{}", last(&s, 0));
        assert!(s.engines[0].waiting.is_empty());
        let p = prompt(&s, 0);
        assert!(p.contains("#### helper — idle · 1 replies\ngoal: Find the bug.\nlast reply: turn.rs:40, a missing trim."), "{p}");
        assert!(
            p.contains("#### the thread — 1 entries") && p.contains("helper → you: The bug is turn.rs:40."),
            "the parent reads the thread: {p}"
        );
        assert_eq!(step(&mut s, 0, "reply(text=\"Fixed at turn.rs:40.\")").0, Step::Reply("Fixed at turn.rs:40.".into()));
        assert_eq!(settle(&mut s, 0, &idle), Settled::default(), "a reply is had once");
    }

    #[test]
    fn a_child_start_returns_at_once_and_calls_back_with_a_new_turn() {
        let mut s = parent();
        s.add_text(0, "check the docs");
        let (done, applied) = step(&mut s, 0, "engine_start(name=\"Docs Checker\", goal=\"Check the docs.\")");
        assert_eq!(done, Step::Again);
        assert!(
            last(&s, 0).ends_with("→ made docs-checker with your goal and started it; a message comes when it replies"),
            "{}",
            last(&s, 0)
        );
        assert_eq!(s.engines[1].name, "Docs Checker");
        assert_eq!(applied.run, [1]);
        assert_eq!(step(&mut s, 0, "reply(text=\"It is checking.\")").0, Step::Reply("It is checking.".into()));

        // Asked again while it works: refused, so a message never races its turn.
        let working = |j: usize| Seen { busy: j == 1, label: "calling the LLM".into(), ..Seen::default() };
        let mut fam = family(&s, 0, &working);
        step_in(
            &mut s.engines[0],
            "thought: x\nact: engine_start(name=\"docs-checker\", text=\"faster\")",
            &mut |_| None,
            &NoHost,
            Some(&mut fam),
        );
        assert!(last(&s, 0).contains("error: docs-checker is still working"), "{}", last(&s, 0));
        assert!(fam.effects.is_empty());

        step(&mut s, 1, "reply(text=\"Two broken links.\")");
        let settled = settle(&mut s, 0, &idle);
        assert_eq!(settled.wake, ["engine docs-checker replied: \"Two broken links.\" (engines kids)"]);
        assert!(!settled.resumed);
        assert!(crate::learning::is_callback(&settled.wake[0]), "not the person's words");
        // The shell adds it as a message and runs a turn.
        assert!(s.add_text(0, &settled.wake.join("\n")));
        assert_eq!(step(&mut s, 0, "reply(text=\"Two links are broken.\")").0, Step::Reply("Two links are broken.".into()));
        assert!(settle(&mut s, 0, &idle).wake.is_empty(), "once");

        // The same child again, with text: no new child, the same goal.
        let (_, applied) = step(&mut s, 0, "engine_start(name=\"docs-checker\", goal=\"other\", text=\"Now the README.\")");
        assert!(last(&s, 0).contains("→ sent docs-checker your message (it keeps its goal"), "{}", last(&s, 0));
        assert_eq!((applied.run.clone(), s.engines.len()), (vec![1], 2));
        assert_eq!(s.engines[1].messages.last().unwrap().text, "Now the README.");
    }

    #[test]
    fn a_tree_shares_one_thread_and_a_child_cannot_make_its_parent() {
        let mut s = parent();
        s.add_text(0, "split it");
        step(&mut s, 0, "engine_start(name=\"a\", goal=\"Part A.\", kinds=\"engines\"); engine_start(name=\"b\", goal=\"Part B.\")");
        assert_eq!(s.engines.len(), 3);
        assert_eq!(step(&mut s, 1, "post(text=\"A needs the schema from B.\", to=\"b\")").0, Step::Again);
        let b = prompt(&s, 2);
        assert!(b.contains("### shared — your thread: the user and every engine of your tree"), "{b}");
        assert!(
            b.contains("you (b), the user (posts as `you`) and the tree's other engines: engine-1, a.") && b.contains("1. 01-01 00:00 a → b: A needs the schema from B."),
            "b sees a's post as it is now: {b}"
        );
        assert!(!s.engines[2].messages.iter().any(|m| m.text.contains("schema")), "in the snapshot, never in b's history");
        step(&mut s, 2, "edit_post(n=1, text=\"mine\")");
        assert!(last(&s, 2).contains("error: entry 1 is a's; you can edit only yours"));
        step(&mut s, 2, "post(text=\"Schema: id, name.\", to=\"a\")");
        assert!(prompt(&s, 1).contains("2. 01-01 00:00 b → a: Schema: id, name."), "and a sees b's");
        assert!(prompt(&s, 0).contains("#### the thread — 2 entries"), "the parent reads it");
        assert_eq!(s.shared["engine-1"].lines().count(), 2, "kept on the root's thread, once");

        // a may make children of its own, but never its parent.
        step(&mut s, 1, "engine_start(name=\"engine-1\", goal=\"loop\")");
        assert!(last(&s, 1).contains("error: `engine-1` is you or one of your parents"), "{}", last(&s, 1));
        step(&mut s, 1, "engine_start(name=\"b\", goal=\"x\")");
        assert!(last(&s, 1).contains("`b` is an engine that is not your child"), "{}", last(&s, 1));
        let (_, applied) = step(&mut s, 1, "engine_start(name=\"a1\", goal=\"Half of A.\")");
        assert_eq!(s.engines[applied.run[0]].parent.as_deref(), Some("a"), "a grandchild: no depth limit");
        assert_eq!(family(&s, applied.run[0], &idle).ancestors, ["a", "engine-1"]);
        // Removing the parent removes its whole family and their contexts.
        s.remove_engine(0);
        assert!(s.engines.is_empty() && s.shared.is_empty());
    }

    #[test]
    fn replying_while_a_call_waits_turns_it_into_a_callback() {
        let mut s = parent();
        s.add_text(0, "go");
        let (done, _) = step(&mut s, 0, "engine_call(name=\"h\", goal=\"G.\"); reply(text=\"Asked h.\")");
        assert_eq!(done, Step::Reply("Asked h.".into()));
        assert!(s.engines[0].waiting.is_empty());
        assert!(last(&s, 0).contains("→ h is working; you replied, so a message comes when it replies"), "{}", last(&s, 0));
        assert!(!parse(&s.engines[0].artifacts["kids"])[0].call, "now a start: it calls back");
        step(&mut s, 1, "reply(text=\"done\")");
        assert_eq!(settle(&mut s, 0, &idle).wake, ["engine h replied: \"done\" (engines kids)"]);
    }

    #[test]
    fn a_message_from_you_mid_wait_ends_the_wait_and_the_child_calls_back() {
        let mut s = parent();
        s.add_text(0, "go");
        step(&mut s, 0, "engine_call(name=\"h\", goal=\"G.\")");
        assert!(s.add_text(0, "never mind, tell me later"));
        assert!(s.engines[0].waiting.is_empty(), "a new message is a new turn");
        assert!(s.engines[0].messages.iter().any(|m| m.text.contains("→ h is working; you replied, so a message comes when it replies")));
        step(&mut s, 1, "reply(text=\"G done.\")");
        let settled = settle(&mut s, 0, &idle);
        assert_eq!((settled.wake, settled.resumed), (vec!["engine h replied: \"G done.\" (engines kids)".to_string()], false));
    }

    #[test]
    fn a_failed_child_ends_the_wait_with_an_error() {
        let mut s = parent();
        s.add_text(0, "go");
        step(&mut s, 0, "engine_call(name=\"h\", goal=\"G.\")");
        let failed = |j: usize| Seen { failed: (j == 1).then(|| "no model".to_string()), ..Seen::default() };
        assert!(settle(&mut s, 0, &failed).resumed);
        assert!(last(&s, 0).ends_with("→ error: h failed: no model"), "{}", last(&s, 0));
        assert!(settle(&mut s, 0, &failed).wake.is_empty(), "told once, in the wait");
    }

    #[test]
    fn a_cut_reply_says_how_much_of_it_the_snapshot_holds() {
        let long = "x".repeat(SNAPSHOT_REPLY + 10);
        assert!(quote(&long, REPLY_CHARS, "kids").ends_with(&format!("({} characters; the first {SNAPSHOT_REPLY} are in the kids artifact)", SNAPSHOT_REPLY + 10)));
        assert!(quote(&"y".repeat(500), REPLY_CHARS, "kids").ends_with("(500 characters; all of it is in the kids artifact)"));
    }

    #[test]
    fn a_started_child_that_fails_calls_back_once_and_again_after_it_worked() {
        let mut s = parent();
        s.add_text(0, "go");
        step(&mut s, 0, "engine_start(name=\"h\", goal=\"G.\")");
        let failed = |j: usize| Seen { failed: (j == 1).then(|| "no model".to_string()), ..Seen::default() };
        assert_eq!(settle(&mut s, 0, &failed).wake, ["engine h failed: no model (engines kids)"]);
        assert!(settle(&mut s, 0, &failed).wake.is_empty(), "a failure is told once");
        assert!(s.engines[0].artifacts["kids"].contains("child: h start 0 told"), "{}", s.engines[0].artifacts["kids"]);
        let busy = |j: usize| Seen { busy: j == 1, ..Seen::default() };
        assert!(settle(&mut s, 0, &busy).wake.is_empty());
        assert_eq!(settle(&mut s, 0, &failed).wake, ["engine h failed: no model (engines kids)"], "it worked again, so a new failure is news");
    }

    #[test]
    fn learning_through_a_turn_collects_only_its_context_then_learns() {
        let mut s = parent();
        let e = &mut s.engines[0];
        e.artifacts.insert("learn".into(), "kind: learning".into());
        e.messages.push(Message { role: Role::User, text: "build it".into() });
        e.messages.push(Message {
            role: Role::Tool,
            text: "run(command=\"cargo biuld\") → error: exit 101\nrun(command=\"cargo build\") → session 2 `cargo build`: exit 0".into(),
        });
        assert_eq!(step(&mut s, 0, "collect(what=\"fixes\")").0, Step::Again);
        assert!(last(&s, 0).ends_with("→ collected 1 fixes (failed calls paired with the call that later worked); they are in the learn artifact, for learn(section=\"lessons\", ...)"), "{}", last(&s, 0));
        let p = prompt(&s, 0);
        assert!(
            p.contains("#### collected: fixes")
                && p.contains("failed: run(command=\"cargo biuld\") → exit 101\n   worked: run(command=\"cargo build\")"),
            "{p}"
        );
        step(&mut s, 0, "learn(section=\"lessons\", entry=\"It is cargo build, not biuld. (n=1, 11-22)\")");
        assert!(last(&s, 0).ends_with("→ added to lessons (45/1600)"), "{}", last(&s, 0));
        assert_eq!(s.engines[0].learning, "## lessons\n- It is cargo build, not biuld. (n=1, 11-22)\n");
        assert_eq!(s.engines[0].learning_log.lines().count(), 1, "every change is logged");
        assert!(!prompt(&s, 0).contains("## Learning\n"), "the slot keeps the turn's first copy (empty) until the next message");
        s.add_text(0, "next");
        assert!(prompt(&s, 0).contains("## Learning\n\nWhat you learned so far:\n### lessons"));
    }
}
