//! The loop: an engine's turn is steps until it replies.
//!
//! ```text
//! your text ─▶ render ─▶ model ─▶ step ─┬─ reply(...)      ─▶ turn over
//!                ▲                     └─ other calls     ─▶ results ─┐
//!                └────────────────────────────────────────────────────┘
//! ```
//!
//! `step` takes one answer from the model: it saves it, reads it with the
//! engine's response contract, runs the calls in `act`, and saves what
//! they came to as one `Role::Tool` message the model reads next.
//!
//! **One step, many calls** (docs/engine-standard.md, "One step, many calls"): every call is invoked
//! at once and only the waiting is parallel. `begin_step` applies the
//! instant calls (pure state changes) in written order, then prepares the
//! waiting ones (`run`, outside tools); the shell runs those all together
//! (`run_wait` each, on its own thread) and `finish_step` merges every
//! result in written order. `step` is the same with the waits run one
//! after another, for tests and shells without threads. Long work
//! (`start`, `engine_start`) calls back instead (`callbacks`). A
//! mistake (an answer off the contract, a call that does not parse or
//! fails) is a result too, so the model can fix it on the next step. The
//! shell sends again on `Again`, until a reply: there is no step limit
//! (a stuck loop is handled another way, not by counting).
//!
//! **A reply is never sent beside a call that waits** (`run`, an outside
//! tool): it would be a report written before the result it reports. The
//! waiting calls run, the reply is held back with a line that says why,
//! and the model replies in its next step, having read the results. Work
//! that calls back (`start`, `engine_start`) may share an act with a
//! reply: nothing is claimed about it yet.
//!
//! The tools a step may call are the engine's own plus those its live
//! artifacts bring (`live::toolbox`). A `script(...)` call runs here: its
//! steps go through the same tools (never `reply` or `script`), in order,
//! and the whole run is one result.
//!
//! An `engine_call` (`crate::engines`) waits for a child's reply: the
//! step returns `Wait`, its result line says `waiting for helper's reply`
//! until the reply is in (`engines::settle` writes it there), then the
//! shell asks the model again. If the same answer also replied, the turn
//! is over and that child calls back with a message instead.

use crate::calls::{self, Call};
use crate::engines::Family;
use crate::inference::{Message, Role};
use crate::live;
use crate::response::{ResponseFormat, Responses};
use crate::script;
use crate::state::Engine;
use crate::tools::{self, Outcome};

/// The contract field whose value is the calls.
pub const ACT: &str = "act";

/// What happens after a step.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The engine answered the user: the turn is over.
    Reply(String),
    /// Results were saved: ask the model again.
    Again,
    /// Results were saved, and an `engine_call` waits for these children's
    /// replies: ask the model again once they are in.
    Wait(Vec<String>),
}

/// Runs a call that is not built in (an MCP tool): `None` if it does not
/// know the call, else what it came to.
pub type External<'a> = dyn FnMut(&calls::Call) -> Option<Result<String, String>> + 'a;

/// What a step can do outside without a shell: nothing (every file or
/// terminal call says so). Tests and the shells without a disk use it.
pub struct NoHost;

impl live::Host for NoHost {
    fn now(&self) -> i64 {
        0
    }
    fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
        Err("no file system here".into())
    }
    fn read(&self, _: &str, _: &str) -> live::Content {
        live::Content::Missing("no file system here".into())
    }
    fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err("no file system here".into())
    }
    fn run(&self, _: &live::Job) -> Result<Option<i32>, String> {
        Err("no terminal here".into())
    }
    fn spawn(&self, _: &live::Job) -> Result<(), String> {
        Err("no terminal here".into())
    }
    fn status(&self, _: &live::Job) -> live::Proc {
        live::Proc::Ended(None)
    }
    fn tail(&self, _: &live::Job, _: usize) -> String {
        String::new()
    }
}

/// Take one answer `text` from the model for engine `e`. Calls the
/// built-in tools do not have go to `external` first. No disk or
/// terminal: see `step_with`.
pub fn step(e: &mut Engine, text: &str, external: &mut External) -> Step {
    step_with(e, text, external, &NoHost)
}

/// `step`, with `host` for the tools file and terminal artifacts bring.
/// No other engines: see `step_in`.
pub fn step_with(e: &mut Engine, text: &str, external: &mut External, host: &dyn live::Host) -> Step {
    step_in(e, text, external, host, None)
}

/// `step_with`, with `family`: what the shell knows of the other engines,
/// for the engines and shared tools; what they ask of the shell is left
/// in its `effects` (`engines::apply` makes them). The act's waiting
/// calls run here one after another; the shell runs them at the same
/// time instead (`begin_step`, `run_wait`, `finish_step`), with the same
/// result.
pub fn step_in(e: &mut Engine, text: &str, external: &mut External, host: &dyn live::Host, family: Option<&mut Family>) -> Step {
    let pending = begin_step(e, text, external, host, family);
    let came: Vec<Came> = pending.waits.iter().map(|(_, w)| run_wait(w, external, host)).collect();
    finish_step(e, pending, came)
}

/// A call that waits. `begin_step` prepares it; the shell runs every one
/// of an act at the same time (`run_wait` each, on its own thread) and
/// hands what they came to to `finish_step`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wait {
    /// A terminal `run`, its session recorded as running.
    Run(crate::terminal::RunJob),
    /// A call none of the engine's own tools take (an MCP tool): the
    /// shell's `External`.
    Outside(Call),
}

/// What a waiting call came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Came {
    /// The command's exit code and last lines, or why it did not run.
    Run(Result<(Option<i32>, String), String>),
    /// What the outside tool said; `None`: no outside tool knows the call.
    Outside(Option<Result<String, String>>),
}

/// Run one waiting call and wait for it: what the shell does, once per
/// waiting call of an act, all at the same time.
pub fn run_wait(w: &Wait, external: &mut External, host: &dyn live::Host) -> Came {
    match w {
        Wait::Run(job) => Came::Run(job.wait(host)),
        Wait::Outside(call) => Came::Outside(external(call)),
    }
}

/// A step between its two halves: the instant calls are applied, the
/// waiting ones (`waits`, in written order) are for the shell to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    /// One per result line, in the order the calls were written: the line,
    /// or which wait's result goes there.
    slots: Vec<Slot>,
    /// The waiting calls, in written order, each with its call.
    pub waits: Vec<(Call, Wait)>,
    reply: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Line(String),
    Wait(usize),
}

impl Pending {
    /// The calls that wait, by name, for the activity.
    pub fn waiting_names(&self) -> Vec<String> {
        self.waits.iter().map(|(c, _)| c.name.clone()).collect()
    }
}

/// The first half of a step: save the answer, read it, apply every call
/// that is a pure state change (file open, focus, edit, save; write;
/// post; learn; engine_call and engine_start, which leave their effects
/// in `family`; start; reply) **in the order written**, then prepare the
/// calls that wait (`run`, outside tools), which the shell launches all
/// together. So `edit(a); save(a); run("cargo test")` tests the saved
/// file, and a waiting call never sees half of the act. `external` is
/// only for a script's steps: a script is one call whose steps run in
/// order, so its waits wait inside it.
pub fn begin_step(e: &mut Engine, text: &str, external: &mut External, host: &dyn live::Host, family: Option<&mut Family>) -> Pending {
    open_step(e, text);
    act_step(e, text, external, host, family, Vec::new())
}

/// The answer saved, and one more step counted in the current phase
/// (shown in the plan, never a limit): what `begin_step` does first.
pub(crate) fn open_step(e: &mut Engine, text: &str) {
    e.messages.push(Message { role: Role::Assistant, text: text.trim().to_string() });
    e.run.count();
}

/// What a call of the act came to before the act ended (`crate::early`):
/// it waits (and is a terminal `run`), or the instant call ran.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pre {
    Waits(bool),
    Done(Result<Outcome, String>),
}

/// `begin_step` after `open_step`: the act's calls, the first
/// `pre.len()` of them already decided while the answer streamed
/// (`crate::early`, on this same engine). The rest as `begin_step`.
pub(crate) fn act_step(e: &mut Engine, text: &str, external: &mut External, host: &dyn live::Host, mut family: Option<&mut Family>, pre: Vec<Pre>) -> Pending {
    let mut out = Pending { slots: Vec::new(), waits: Vec::new(), reply: None };
    // No contract: a free-text answer is the reply.
    if e.response.is_empty() {
        out.reply = Some(text.trim().to_string());
        return out;
    }
    let line = |s: String| Slot::Line(s);
    // An answer whose calls read but which left another field out (the
    // thought) still acts: refusing it would cost a whole step to hear
    // the same calls again. What was missing is said after the results.
    let mut left_out = None;
    let read = e.format.parse(&e.response, text).or_else(|why| {
        let calls_only = Responses { fields: e.response.fields.iter().filter(|f| f.key == ACT).cloned().collect() };
        match e.format.parse(&calls_only, text) {
            Ok(answer) if !calls_only.fields.is_empty() => {
                left_out = Some(why.clone());
                Ok(answer)
            }
            _ => Err(why),
        }
    });
    match read {
        Err(why) => out.slots.push(line(format!(
            "Your answer did not follow the response contract ({why}). Answer again, written as the Response section says."
        ))),
        Ok(answer) => match calls::parse(Responses::get(&answer, ACT).unwrap_or("")) {
            Err(why) => out.slots.push(line(format!("The calls in `{ACT}` did not read: {why}."))),
            Ok(list) if list.is_empty() => {
                out.slots.push(line(format!("No calls in `{ACT}`. To answer the user, call reply(text=\"...\").")))
            }
            Ok(list) => {
                let toolbox = live::toolbox(e);
                // Instant calls, in order; waiting ones keep their place.
                let mut later = Vec::new();
                let mut pre = pre.into_iter();
                for call in &list {
                    let done = match pre.next() {
                        Some(Pre::Waits(run)) => {
                            later.push((out.slots.len(), call, run));
                            out.slots.push(Slot::Wait(usize::MAX));
                            continue;
                        }
                        Some(Pre::Done(done)) => done,
                        None if waits(e, call, &toolbox) => {
                            later.push((out.slots.len(), call, live::waits(e, call)));
                            out.slots.push(Slot::Wait(usize::MAX));
                            continue;
                        }
                        None => run_call(e, call, &toolbox, external, host, family.as_deref_mut()),
                    };
                    match done {
                        Ok(Outcome::Reply(text)) => {
                            // A broken ask still replies; the model reads why.
                            if let Some(note) = crate::ask::Parsed::of(call).note() {
                                out.slots.push(line(format!("{call} → {note}")));
                            }
                            out.reply = Some(text)
                        }
                        Ok(Outcome::Result(r)) => out.slots.push(line(format!("{call} → {r}"))),
                        Err(why) => out.slots.push(line(format!("{call} → error: {why}"))),
                    }
                }
                // A reply beside a call that waits was written before that
                // call's result, so it cannot be a report of it: it is not
                // sent, the waiting calls still run, and the model reads
                // their results and replies in its next step.
                let mut held = None;
                if !later.is_empty() && out.reply.take().is_some() {
                    let mut names: Vec<&str> = later.iter().map(|(_, c, _)| c.name.as_str()).collect();
                    names.dedup();
                    held = Some(format!(
                        "reply(...) → not sent: this act also calls {}, and a reply written beside it cannot have read its result. The result is above: read it, then reply in a step of its own.",
                        names.join(", ")
                    ));
                }
                // Then the waiting ones are prepared, after every instant one.
                // A call to an outside capability's tool is checked first
                // (its inputs, an actuator's approval), then goes outside.
                for (slot, call, run) in later {
                    let wait = if run {
                        live::prepare(e, call, host).map(Wait::Run)
                    } else if let Some(as_run) = crate::skilltool::as_run(e, call) {
                        // A loaded skill's own tool: its command, run in the terminal.
                        as_run.and_then(|run| live::prepare(e, &run, host)).map(Wait::Run)
                    } else {
                        crate::capability::check(e, call).unwrap_or(Ok(())).map(|()| Wait::Outside(call.clone()))
                    };
                    out.slots[slot] = match wait {
                        Ok(w) => {
                            out.waits.push((call.clone(), w));
                            Slot::Wait(out.waits.len() - 1)
                        }
                        Err(why) => line(format!("{call} → error: {why}")),
                    };
                }
                // Last, so it is read after the results it points at.
                out.slots.extend(held.map(line));
                out.slots.extend(left_out.map(|why| line(format!("(Your calls ran, but {why}: write every field of the Response section next time.)"))));
            }
        },
    }
    out
}

/// Whether `call` waits: a terminal `run`, or a call none of the
/// engine's own tools take (it goes outside, to an MCP server).
pub(crate) fn waits(e: &Engine, call: &Call, toolbox: &[String]) -> bool {
    let own = [crate::skill::LOAD, crate::skill::SAVE, crate::phase::GOTO, "script"];
    if own.contains(&call.name.as_str()) && toolbox.contains(&call.name) {
        return false;
    }
    if toolbox.contains(&call.name) && live::Live::ALL.iter().any(|k| k.tools().contains(&call.name.as_str())) {
        return live::waits(e, call);
    }
    !(toolbox.contains(&call.name) && tools::BUILTIN.iter().any(|t| t.name == call.name))
}

/// The second half: the waiting calls' results (`came`, one per wait, in
/// the order of `pending.waits`, whatever order they finished in) merged
/// with the instant ones in the order the calls were written, saved as
/// one `Role::Tool` message the model reads next.
pub fn finish_step(e: &mut Engine, pending: Pending, came: Vec<Came>) -> Step {
    let Pending { slots, waits, reply } = pending;
    let mut came: Vec<Option<Came>> = came.into_iter().map(Some).collect();
    let mut results = Vec::new();
    for slot in slots {
        let text = match slot {
            Slot::Line(text) => text,
            Slot::Wait(k) => {
                let (call, wait) = &waits[k];
                let done = match (wait, came.get_mut(k).and_then(Option::take)) {
                    (Wait::Run(job), Some(Came::Run(r))) => match e.artifacts.get_mut(&job.artifact) {
                        Some(text) => {
                            let header = crate::capability::first_line(text);
                            let done = crate::terminal::finish_run(text, job, r);
                            crate::capability::keep_header(text, &header);
                            done
                        }
                        None => Err("the terminal artifact is gone".to_string()),
                    },
                    (Wait::Outside(_), Some(Came::Outside(Some(r)))) => r,
                    // No outside tool knows it: the built-in tools say why.
                    (Wait::Outside(_), Some(Came::Outside(None))) => {
                        tools::run(call, &live::toolbox(e), &mut e.artifacts).and_then(|o| match o {
                            Outcome::Result(r) => Ok(r),
                            Outcome::Reply(_) => Err("a reply is not a waiting call".to_string()),
                        })
                    }
                    _ => Err("no result came back for this call".to_string()),
                };
                match done {
                    Ok(r) => format!("{call} → {r}"),
                    Err(why) => format!("{call} → error: {why}"),
                }
            }
        };
        results.push(text);
    }
    if !results.is_empty() {
        e.messages.push(Message { role: Role::Tool, text: results.join("\n") });
    }
    // Replied while an engine_call waits: the turn is over, so those
    // children call back with a message instead.
    if reply.is_some() {
        crate::engines::stop_waiting(e);
    }
    match reply {
        Some(text) => Step::Reply(text),
        None if !e.waiting.is_empty() => Step::Wait(e.waiting.clone()),
        None => Step::Again,
    }
}

/// Every callback engine `i` has now, through one path: the children it
/// waits for (`engine_call`: their replies fill the step's lines, and
/// `resumed` says the turn goes on), else the work that calls back
/// (`engine_start` children that replied, terminal `start` sessions that
/// ended, the jobs a `start(..., wait="yes")` waits on, once the last
/// has ended, a guided post, an alert of its own that is due), as `wake`:
/// one message, a new turn that keeps the phase (`State::add_wake`).
/// `busy`: a turn is running,
/// so nothing is taken now; everything stays where it is and comes,
/// together, the first time the engine is free.
pub fn callbacks(state: &mut crate::state::State, i: usize, busy: bool, host: &dyn live::Host, seen: &dyn Fn(usize) -> crate::engines::Seen) -> crate::engines::Settled {
    if busy || i >= state.engines.len() {
        return crate::engines::Settled::default();
    }
    let waited = !state.engines[i].waiting.is_empty();
    let mut out = crate::engines::settle(state, i, seen);
    // Its turn waits on its own jobs (`start(..., wait="yes")`): once the
    // last has ended (and no child is waited for), their ends are the wake.
    // (A turn that goes on now, `resumed`, takes nothing else.)
    let waited = waited
        && (out.resumed || {
            out.wake.extend(crate::terminal::settle_waited(&mut state.engines[i], host));
            !state.engines[i].waiting.is_empty()
        });
    if !waited {
        out.wake.extend(crate::terminal::settle_engine(&mut state.engines[i], host));
        // A guided post on its thread calls it (`crate::desk`).
        out.wake.extend(crate::desk::guided(state, i));
        // An alert of its own that is due (`crate::alert`): the one way an
        // engine starts work with nobody writing to it.
        // An `idle` one asks whether anyone on its thread works now.
        if !state.engines[i].alerts.is_empty() || !state.engines[i].alert_marks.is_empty() {
            let root = crate::shared::root_of(state, &state.engines[i].id);
            let working = crate::shared::members(state, &root).into_iter().any(|j| j != i && seen(j).busy);
            let e = &mut state.engines[i];
            let look = crate::alert::Look { working, messages: e.messages.len(), artifacts: Some(&e.artifacts) };
            out.wake.extend(crate::alert::settle_with(&mut e.alerts, &mut e.alert_marks, host.clock(), &look));
        }
    }
    out
}

/// Run one call with the tools in `toolbox`: a script here, any other
/// call outside first, then the built-in tools.
pub(crate) fn run_call(
    e: &mut Engine,
    call: &Call,
    toolbox: &[String],
    external: &mut External,
    host: &dyn live::Host,
    mut family: Option<&mut Family>,
) -> Result<Outcome, String> {
    if call.name == crate::skill::LOAD && toolbox.iter().any(|t| t == crate::skill::LOAD) {
        let name = call.arg("name").ok_or("`skill` needs name=\"...\"")?;
        let text = e.visible_skills().into_iter().find(|(n, _)| *n == name).map(|(_, t)| t.to_string());
        let names = e.visible_skills().iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
        let text = text.ok_or_else(|| format!("no skill `{name}` here; yours are {names}"))?;
        let skill = crate::skill::parse(&text).map_err(|why| format!("skill `{name}` does not read: {why}"))?;
        if !e.loaded_skills.iter().any(|l| l == name) {
            e.loaded_skills.push(name.to_string());
        }
        return Ok(Outcome::Result(format!("{}{}", skill.loaded(), crate::skilltool::loaded(e, name))));
    }
    if call.name == crate::skill::SAVE && toolbox.iter().any(|t| t == crate::skill::SAVE) {
        let need = |key: &str| call.arg(key).map(str::to_string).ok_or_else(|| format!("`skill_save` needs {key}=\"...\""));
        let name = need("name")?;
        if !crate::skill::valid_name(&name) {
            return Err(format!("`{name}`: a skill name is lower case letters, digits, - and _"));
        }
        let tools = call.arg("tools").map_or(Vec::new(), |l| l.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect());
        let skill = crate::skill::Skill { name: name.clone(), description: need("description")?, tools, body: need("text")? };
        let replaced = e.skills.insert(name.clone(), skill.to_md()).is_some();
        return Ok(Outcome::Result(format!("{} skill {name}", if replaced { "updated" } else { "saved" })));
    }
    if call.name == crate::phase::GOTO && toolbox.iter().any(|t| t == crate::phase::GOTO) {
        let phases = e.phase_list()?;
        let name = call.arg("name").ok_or("`goto_phase` needs name=\"...\"")?;
        return e.run.goto(&phases, name).map(Outcome::Result);
    }
    if call.name == "script" && toolbox.iter().any(|t| t == "script") {
        let name = call.arg("name").ok_or("`script` needs name=\"...\"")?;
        let script = script::find(&e.scripts, name)?;
        let args: Vec<(String, String)> = call.args.iter().filter(|(k, _)| k != "name").cloned().collect();
        let inner: Vec<String> = toolbox.iter().filter(|t| !script::FORBIDDEN.contains(&t.as_str())).cloned().collect();
        let mut each = |step: &Call| match run_call(e, step, &inner, external, host, family.as_deref_mut())? {
            Outcome::Result(out) => Ok(out),
            Outcome::Reply(_) => Err("a script cannot reply".to_string()),
        };
        return script.run(&args, &mut each).map(Outcome::Result);
    }
    // A tool a file or terminal artifact brings, while it is there.
    if toolbox.contains(&call.name)
        && let Some(done) = live::call(e, call, host, family)
    {
        return done.map(Outcome::Result);
    }
    if let Some(Err(why)) = crate::capability::check(e, call) {
        return Err(why);
    }
    match external(call) {
        Some(done) => done.map(Outcome::Result),
        None => tools::run(call, toolbox, &mut e.artifacts),
    }
}

/// The names of the calls in an answer's `act`, in order (none when it
/// does not read): what the step is about to run, for the activity.
pub fn call_names(e: &Engine, text: &str) -> Vec<String> {
    if e.response.is_empty() {
        return Vec::new();
    }
    let Ok(answer) = e.format.parse(&e.response, text) else { return Vec::new() };
    calls::parse(Responses::get(&answer, ACT).unwrap_or("")).map_or(Vec::new(), |list| list.into_iter().map(|c| c.name).collect())
}

/// The reply in an answer, if its `act` calls `reply` (for showing it).
pub fn reply_in(e: &Engine, text: &str) -> Option<String> {
    let answer = e.format.parse(&e.response, text).ok()?;
    let calls = calls::parse(Responses::get(&answer, ACT)?).ok()?;
    calls.into_iter().find(|c| c.name == "reply").and_then(|c| c.arg("text").map(str::to_string))
}

/// The components of the reply in `text`: every argument but `text`,
/// written in the call grammar (`show="table", data="A|B;1|2"`), so a
/// parent can pass a child's answer on with what it shows and asks. None
/// for a reply of text alone.
pub fn reply_components(e: &Engine, text: &str) -> Option<String> {
    let answer = e.format.parse(&e.response, text).ok()?;
    let calls = calls::parse(Responses::get(&answer, ACT)?).ok()?;
    let mut reply = calls.into_iter().find(|c| c.name == "reply")?;
    reply.args.retain(|(k, _)| k != "text");
    if reply.args.is_empty() {
        return None;
    }
    let written = reply.written();
    Some(written["reply(".len()..written.len() - 1].to_string())
}

/// The options the reply in `text` asks the user to pick from (its
/// `choices`), if any. The chat draws them as buttons; the one picked is
/// sent as the user's next message.
pub fn choices_in(e: &Engine, text: &str) -> Vec<String> {
    let Some(answer) = e.format.parse(&e.response, text).ok() else { return Vec::new() };
    let Some(Ok(calls)) = Responses::get(&answer, ACT).map(calls::parse) else { return Vec::new() };
    calls.iter().find(|c| c.name == "reply").and_then(|c| c.arg("choices")).map(tools::split_choices).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;
    use crate::response::Format;

    fn engine() -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.format = Format::default();
        e.messages.push(Message { role: Role::User, text: "save a plan, then tell me".into() });
        e
    }

    #[test]
    fn a_turn_is_steps_until_reply() {
        let mut e = engine();
        // Step 1: a tool call, no reply: results, go again.
        let s = step(&mut e, "thought: save it first\nact: write(name=\"plan\", text=\"1. read\")", &mut |_| None);
        assert_eq!(s, Step::Again);
        assert_eq!(e.artifacts["plan"], "1. read");
        let last = e.messages.last().unwrap();
        assert_eq!((last.role, last.text.as_str()), (Role::Tool, "write(name=\"plan\", text=\"1. read\") → saved plan (7 characters)"));
        // Step 2: reply: the turn is over, nothing more to read.
        let s = step(&mut e, "thought: saved\nact: reply(text=\"Saved the plan.\")", &mut |_| None);
        assert_eq!(s, Step::Reply("Saved the plan.".into()));
        assert_eq!(e.messages.last().unwrap().role, Role::Assistant);
        assert_eq!(reply_in(&e, &e.messages.last().unwrap().text).as_deref(), Some("Saved the plan."));
    }

    #[test]
    fn the_contract_is_read_last_at_every_step() {
        let mut e = engine();
        let last = |e: &Engine| crate::prompt::render(e).messages().last().unwrap().text.clone();
        let count = |e: &Engine| crate::prompt::render(e).messages().iter().filter(|m| m.text.contains("## Response")).count();
        assert!(last(&e).starts_with("save a plan, then tell me\n\n## Response\n\n"), "after your text");
        // After a step it follows the results, and it is sent once.
        step(&mut e, "thought: save it first\nact: write(name=\"plan\", text=\"1. read\")", &mut |_| None);
        assert!(last(&e).starts_with("write(name=\"plan\", text=\"1. read\") → saved plan"), "{}", last(&e));
        assert!(last(&e).contains("\n\n## Response\n\n") && last(&e).trim_end().ends_with("act: ..."), "{}", last(&e));
        assert_eq!(count(&e), 1);
    }

    #[test]
    fn calls_with_no_thought_still_run_and_the_missing_field_is_said() {
        let mut e = engine();
        assert_eq!(step(&mut e, "act: write(name=\"plan\", text=\"1. read\")", &mut |_| None), Step::Again);
        assert_eq!(e.artifacts["plan"], "1. read", "the call ran");
        let out = e.messages.last().unwrap().text.clone();
        assert!(out.starts_with("write(name=\"plan\", text=\"1. read\") → saved plan"), "{out}");
        assert!(out.ends_with("(Your calls ran, but the answer has no thought: write every field of the Response section next time.)"), "{out}");
        // A reply written that way is sent: the turn ends.
        assert_eq!(step(&mut e, "act: reply(text=\"Saved.\")", &mut |_| None), Step::Reply("Saved.".into()));
        // A thought with no calls is still refused: there is nothing to run.
        step(&mut e, "thought: next I will read it", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("did not follow the response contract (the answer has no act)"));
    }

    #[test]
    fn mistakes_are_results_the_model_reads() {
        let mut e = engine();
        assert_eq!(step(&mut e, "hello!", &mut |_| None), Step::Again);
        assert!(e.messages.last().unwrap().text.starts_with("Your answer did not follow the response contract"));
        assert_eq!(step(&mut e, "thought: x\nact: none", &mut |_| None), Step::Again);
        assert!(e.messages.last().unwrap().text.starts_with("No calls in `act`"));
        assert_eq!(step(&mut e, "thought: x\nact: fly(to=\"moon\")", &mut |_| None), Step::Again);
        assert!(e.messages.last().unwrap().text.contains("error: no tool `fly`"));
    }

    #[test]
    fn a_call_the_engine_does_not_have_goes_out() {
        let mut e = engine();
        let mut asked = Vec::new();
        let mut mcp = |c: &calls::Call| {
            (c.name == "calc.add").then(|| {
                asked.push(c.to_string());
                Ok("5".to_string())
            })
        };
        assert_eq!(step(&mut e, "thought: add\nact: calc.add(a=2, b=3)", &mut mcp), Step::Again);
        assert_eq!(asked, ["calc.add(a=\"2\", b=\"3\")"]);
        assert_eq!(e.messages.last().unwrap().text, "calc.add(a=\"2\", b=\"3\") → 5");
    }

    #[test]
    fn a_reply_can_ask_the_user_to_pick_one() {
        let mut e = engine();
        let s = step(&mut e, "thought: ask\nact: reply(text=\"Which colour?\", choices=\"Red | Green|  |Blue\")", &mut |_| None);
        assert_eq!(s, Step::Reply("Which colour?".into()));
        let last = &e.messages.last().unwrap().text;
        assert_eq!(choices_in(&e, last), ["Red", "Green", "Blue"]);
        assert!(choices_in(&e, "thought: x\nact: reply(text=\"plain\")").is_empty());
        assert!(choices_in(&e, "not the contract").is_empty());
    }

    #[test]
    fn phases_steer_the_loop_from_plan_to_reply() {
        let mut e = engine();
        e.template = "| phase |\n|---|\n| plan |\n| build |\n\n## phase: plan\nWrite the plan.\n\n## phase: build\nDo it.\n".into();
        let prompt = |e: &Engine| crate::prompt::render(e).messages()[0].text.clone();
        assert!(prompt(&e).contains("1. **plan** ← now, step 1"));
        assert!(prompt(&e).contains("goto_phase(name=\"...\"): Move to another phase"), "goto_phase is offered");

        // Plan: as many steps as it takes; the plan counts them, never stops them.
        step(&mut e, "thought: plan\nact: write(name=\"plan\", text=\"1. do\")", &mut |_| None);
        step(&mut e, "thought: more\nact: list()", &mut |_| None);
        assert!(prompt(&e).contains("1. **plan** ← now, step 3"));
        assert!(!e.messages.last().unwrap().text.contains("over"), "no limit, no over-budget note");

        // goto_phase moves on; the plan shows build as now.
        step(&mut e, "thought: done\nact: goto_phase(name=\"build\")", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("left plan; now build"));
        assert!(prompt(&e).contains("1. plan — Write the plan.\n2. **build** ← now"));
        assert_eq!(step(&mut e, "thought: ok\nact: reply(text=\"Built.\")", &mut |_| None), Step::Reply("Built.".into()));

        // A new message from the user starts again at the first phase.
        let mut s = crate::state::State { engines: vec![e], ..Default::default() };
        s.add_text(0, "again");
        assert_eq!(s.engines[0].run, crate::phase::Run::default());
    }

    #[test]
    fn a_broken_template_is_shown_and_the_loop_runs_on_the_default() {
        let mut e = engine();
        e.template = "## phase: plan\n".into();
        assert!(crate::prompt::render(&e).to_text().contains("[not sent]\n- Template: template.md did not read, so the default is used"));
        assert_eq!(step(&mut e, "thought: x\nact: reply(text=\"ok\")", &mut |_| None), Step::Reply("ok".into()));
    }

    #[test]
    fn skills_are_listed_loaded_with_their_tools_saved_and_narrowed_by_phase() {
        let mut e = engine();
        e.tools = vec!["reply".into(), crate::skill::SAVE.into()];
        e.skills.insert("review".into(), "---\nname: review\ndescription: Review a change.\ntools: read\n---\nRead it closely.\n".into());
        let system = |e: &Engine| crate::prompt::render(e).messages()[0].text.clone();
        assert!(system(&e).contains("### skills — your skills") && system(&e).contains("- review: Review a change."));
        assert!(system(&e).contains("- skill(name=\"...\")") && !system(&e).contains("- read(name"), "read comes only with the skill");

        // Loading it gives its instructions and its tools.
        step(&mut e, "thought: load\nact: skill(name=\"review\")", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("Skill `review` loaded. Follow it:\n\nRead it closely."));
        assert!(system(&e).contains("- read(name"), "its tools are the engine's now");

        // Saving makes a new one, listed at once; a bad name is refused.
        step(&mut e, "thought: save\nact: skill_save(name=\"triage\", description=\"Sort a request.\", text=\"Sort it.\")", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("saved skill triage"));
        assert!(system(&e).contains("- triage: Sort a request."));
        step(&mut e, "thought: x\nact: skill_save(name=\"Bad Name\", description=\"d\", text=\"t\")", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("a skill name is lower case"));

        // A phase sees only its skills.
        e.template = "| phase | skills |\n|---|---|\n| sort | triage |\n\n## phase: sort\nSort it.".into();
        assert!(system(&e).contains("- triage:") && !system(&e).contains("- review:"));

        // Your next message drops what was loaded.
        let mut s = crate::state::State { engines: vec![e], ..Default::default() };
        s.add_text(0, "next");
        assert!(s.engines[0].loaded_skills.is_empty());
    }

    #[test]
    fn without_a_contract_the_answer_is_the_reply() {
        let mut e = Engine::new("E", "", OpenAi::default());
        assert_eq!(step(&mut e, " hi ", &mut |_| None), Step::Reply("hi".into()));
    }

    const SAVE_NOTE: &str = "---\nname: save_note\ndescription: Save a note and read it back.\nparams: note, text?\n---\n- write(name=\"{note}\", text=\"{text}\")\n- read(name=\"{note}\")\n";

    /// An engine with the save_note script and a `kind: scripts` artifact.
    fn with_script() -> Engine {
        let mut e = engine();
        e.scripts.insert("save_note".into(), SAVE_NOTE.into());
        e.artifacts.insert("scripts".into(), "kind: scripts".into());
        e
    }

    #[test]
    fn a_script_runs_its_calls_in_one_step_with_one_report() {
        let mut e = with_script();
        let s = step(&mut e, "thought: one trip\nact: script(name=\"save_note\", note=\"todo\", text=\"milk\")", &mut |_| None);
        assert_eq!(s, Step::Again);
        assert_eq!(e.artifacts["todo"], "milk");
        let report = &e.messages.last().unwrap().text;
        assert!(report.ends_with("→ script save_note: 2 steps done\n1. write(name=\"todo\", text=\"milk\") → saved todo (4 characters)\n2. read(name=\"todo\") → milk"), "{report}");
    }

    #[test]
    fn a_script_stops_at_the_first_failing_step() {
        let mut e = with_script();
        e.tools.retain(|t| t != "write");
        step(&mut e, "thought: x\nact: script(name=\"save_note\", note=\"todo\")", &mut |_| None);
        let report = &e.messages.last().unwrap().text;
        assert!(report.contains("→ error: script save_note: stopped at step 1 of 2\n1. write("), "{report}");
        assert!(report.contains("error: no tool `write`") && !report.contains("2. read"), "{report}");
        assert!(!e.artifacts.contains_key("todo"));
    }

    #[test]
    fn a_script_step_goes_out_like_any_call_and_cannot_reply() {
        let mut e = with_script();
        e.scripts.insert("ask".into(), "---\nname: ask\ndescription: d\n---\n- calc.add(a=1, b=2)\n- list()\n".into());
        let mut mcp = |c: &Call| (c.name == "calc.add").then(|| Ok("3".to_string()));
        step(&mut e, "thought: x\nact: script(name=\"ask\")", &mut mcp);
        assert!(e.messages.last().unwrap().text.contains("1. calc.add(a=\"1\", b=\"2\") → 3\n2. list() → scripts"));
        // A file that calls reply never reads as a script.
        e.scripts.insert("rude".into(), "---\nname: rude\ndescription: d\n---\n- list()\n- reply(text=\"hi\")\n".into());
        assert_eq!(step(&mut e, "thought: x\nact: script(name=\"rude\")", &mut |_| None), Step::Again);
        assert!(e.messages.last().unwrap().text.contains("cannot call reply"));
    }

    #[test]
    fn without_the_live_scripts_artifact_there_is_no_script_tool() {
        let mut e = with_script();
        e.artifacts.remove("scripts");
        step(&mut e, "thought: x\nact: script(name=\"save_note\", note=\"a\")", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("error: no tool `script`"));
        let tools = crate::prompt::render(&with_script()).messages()[0].text.clone();
        assert!(tools.contains("- script(name=\"...\"): Run one of your scripts"), "{tools}");
        assert!(!crate::prompt::render(&e).messages()[0].text.contains("- script("));
    }

    /// The system prompt as sent, with `host` for the live kinds.
    fn sent(e: &Engine, host: &dyn live::Host) -> String {
        crate::prompt::render_with(e, &live::Snapshots { clock: None, host: Some(host), family: None }).messages()[0].text.clone()
    }

    fn history(e: &Engine) -> String {
        e.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_file_turn_opens_focuses_edits_and_saves_with_the_text_only_in_the_snapshot() {
        let body = "fn main() {\n    println!(\"secret sauce\");\n}\n";
        let disk = crate::file::tests::FakeDisk::with(&[("src/main.rs", body), ("README.md", "# x\n")]);
        let mut e = engine();
        e.artifacts.insert("code".into(), "kind: file\nroot: .".into());
        assert!(sent(&e, &disk).contains("- open(path=\"...\"): Open a file"), "the tools come with the artifact");

        let s = step_with(&mut e, "thought: look\nact: open(path=\"src/main.rs\")", &mut |_| None, &disk);
        assert_eq!(s, Step::Again);
        assert_eq!(e.messages.last().unwrap().text, "open(path=\"src/main.rs\") → opened src/main.rs (3 lines)");
        let prompt = sent(&e, &disk);
        assert!(prompt.contains("### code — files on disk as they are now"), "{prompt}");
        assert!(prompt.contains("    2      println!(\"secret sauce\");"), "the content is in the snapshot: {prompt}");
        assert!(!history(&e).contains("secret sauce"), "and never in history");

        // The disk changes: the next render shows it, nothing in history changes.
        disk.files.borrow_mut().insert("src/main.rs".into(), body.replace("secret sauce", "new sauce"));
        assert!(sent(&e, &disk).contains("new sauce") && !sent(&e, &disk).contains("secret sauce"));

        step_with(&mut e, "thought: x\nact: focus(path=\"src/main.rs\", from=2, to=2); edit(path=\"src/main.rs\", old=\"new sauce\", new=\"hot sauce\")", &mut |_| None, &disk);
        let results = &e.messages.last().unwrap().text;
        assert!(results.contains("→ focus src/main.rs 2-2 (3 lines)") && results.contains("→ edited src/main.rs (unsaved: 1 edit"), "{results}");
        let prompt = sent(&e, &disk);
        assert!(prompt.contains("#### src/main.rs — 3 lines, lines 2-2 shown, 1 unsaved edit (lines marked *)\n    2*     println!(\"hot sauce\");"), "{prompt}");
        assert!(disk.files.borrow()["src/main.rs"].contains("new sauce"), "unsaved: not on disk yet");

        step_with(&mut e, "thought: x\nact: save(path=\"src/main.rs\")", &mut |_| None, &disk);
        assert!(e.messages.last().unwrap().text.ends_with("→ saved src/main.rs (1 edit, 3 lines)"));
        assert!(disk.files.borrow()["src/main.rs"].contains("hot sauce"), "saved");
        assert!(!history(&e).contains("println"), "history holds no file text, only the calls and short results");
        assert_eq!(step_with(&mut e, "thought: x\nact: reply(text=\"Done.\")", &mut |_| None, &disk), Step::Reply("Done.".into()));
    }

    #[test]
    fn a_terminal_turn_runs_starts_and_a_callback_runs_a_new_turn() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut state = crate::state::State { engines: vec![engine()], ..Default::default() };
        state.engines[0].id = "e".into();
        state.engines[0].artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        let e = &mut state.engines[0];

        step_with(e, "thought: check\nact: run(command=\"cargo check\")", &mut |_| None, &sh);
        assert!(e.messages.last().unwrap().text.ends_with("→ session 1 `cargo check`: exit 0\ncompiling\nran cargo check"));
        step_with(e, "thought: long\nact: start(command=\"cargo test\"); background(command=\"serve\")", &mut |_| None, &sh);
        assert!(e.messages.last().unwrap().text.contains("session 2 `cargo test` started; a message comes when it ends"));
        assert_eq!(step_with(e, "thought: wait\nact: reply(text=\"Tests are running.\")", &mut |_| None, &sh), Step::Reply("Tests are running.".into()));
        sh.logs.borrow_mut().insert(3, "listening on :8080\n".into());
        let prompt = sent(e, &sh);
        assert!(prompt.contains("#### session 3 — background, running 0s\n$ serve\nlistening on :8080"), "background shows in the snapshot: {prompt}");
        assert!(!history(e).contains("listening"), "and only there");

        // Nothing ended: no callback. Then the test run ends: one message, a new turn.
        assert!(crate::terminal::settle_engine(e, &sh).is_empty());
        sh.end(2, Some(0), "test result: ok. 9 passed\n");
        sh.end(3, Some(0), "");
        let wake = crate::terminal::settle_engine(&mut state.engines[0], &sh);
        assert_eq!(wake, ["session 2 `cargo test` finished, exit 0 (terminal term)"], "the background one never calls back");
        assert!(state.add_text(0, &wake.join("\n")));
        let e = &mut state.engines[0];
        assert!(sent(e, &sh).contains("#### last: session 3 — exit 0"), "{}", sent(e, &sh));
        assert!(sent(e, &sh).contains("Earlier: 2 `cargo test` exit 0"));
        assert_eq!(step_with(e, "thought: passed\nact: reply(text=\"All 9 pass.\")", &mut |_| None, &sh), Step::Reply("All 9 pass.".into()));
        assert!(crate::terminal::settle_engine(e, &sh).is_empty(), "a callback comes once");
    }

    /// A parent `p` with child `e` (asked with engine_start), `e` holding
    /// a terminal and two phases, now in `test`.
    fn waiting_family() -> crate::state::State {
        let mut parent = engine();
        parent.id = "p".into();
        parent.artifacts.insert("kids".into(), "kind: engines\nchild: e start 0\n".into());
        let mut e = engine();
        e.id = "e".into();
        e.parent = Some("p".into());
        e.template = "| phase |\n|---|\n| code |\n| test |\n\n## phase: code\nChange it.\n\n## phase: test\nProve it.\n".into();
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        let mut state = crate::state::State { engines: vec![parent, e], ..Default::default() };
        step(&mut state.engines[1], "thought: done\nact: goto_phase(name=\"test\")", &mut |_| None);
        state
    }

    /// Everything engine `i` has to be called back with, as the host
    /// settles it: a child that waits is busy.
    fn calls_back(state: &mut crate::state::State, i: usize, sh: &dyn live::Host) -> crate::engines::Settled {
        let seen: Vec<crate::engines::Seen> =
            state.engines.iter().map(|e| crate::engines::Seen { busy: !e.waiting.is_empty(), ..Default::default() }).collect();
        callbacks(state, i, false, sh, &|j| seen[j].clone())
    }

    fn phase_of(e: &Engine) -> String {
        e.phase_list().unwrap()[e.run.phase].name.clone()
    }

    #[test]
    fn start_with_wait_ends_the_turn_with_no_reply_and_tells_nobody() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut state = waiting_family();
        let s = step_with(&mut state.engines[1], "thought: long\nact: start(command=\"cargo test\", wait=\"yes\")", &mut |_| None, &sh);
        assert_eq!(s, Step::Wait(vec!["term#1".into()]), "the turn ends, waiting on its job");
        let e = &state.engines[1];
        assert!(e.messages.last().unwrap().text.contains("→ waiting for session 1 `cargo test` to end"), "{}", history(e));
        assert!(crate::engines::replies(e).is_empty(), "no reply");
        assert_eq!(crate::activity::Activity::after(&s, 1).label(), "waiting for session 1 (terminal term)");
        assert_eq!(crate::activity::waiting(e), ["session 1 `cargo test`"], "named once");
        // While it runs, nobody is called: not the child, not its parent.
        assert_eq!(calls_back(&mut state, 1, &sh), crate::engines::Settled::default());
        assert_eq!(calls_back(&mut state, 0, &sh), crate::engines::Settled::default());
        assert_eq!(state.engines[1].waiting, ["term#1"]);
        // Only start waits.
        let e = &mut state.engines[1];
        step_with(e, "thought: x\nact: run(command=\"ls\", wait=\"yes\"); start(command=\"x\", wait=\"maybe\")", &mut |_| None, &sh);
        let last = &e.messages.last().unwrap().text;
        assert!(last.contains("error: `run` takes no wait") && last.contains("wait is \"yes\" or \"no\""), "{last}");
    }

    #[test]
    fn the_last_waited_job_to_end_wakes_it_in_the_same_phase() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut state = waiting_family();
        let act = "thought: long\nact: start(command=\"cargo build\", wait=\"yes\"); start(command=\"cargo test\", wait=\"yes\")";
        assert_eq!(step_with(&mut state.engines[1], act, &mut |_| None, &sh), Step::Wait(vec!["term#1".into(), "term#2".into()]));
        sh.end(1, Some(0), "built\n");
        assert!(calls_back(&mut state, 1, &sh).wake.is_empty(), "one job still runs: the wait goes on");
        assert_eq!(state.engines[1].waiting, ["term#1", "term#2"]);
        sh.end(2, Some(1), "1 failed\n");
        let woke = calls_back(&mut state, 1, &sh);
        assert!(!woke.resumed);
        assert_eq!(
            woke.wake,
            ["session 1 `cargo build` finished, exit 0 (terminal term)", "session 2 `cargo test` finished, exit 1 (terminal term)"],
            "both ends, together, once the last is in"
        );
        assert!(state.add_wake(1, &woke.wake.join("\n")));
        let e = &state.engines[1];
        assert!(e.waiting.is_empty());
        assert_eq!(phase_of(e), "test", "the wake keeps the phase");
        assert!(!history(e).contains("waiting for session"), "the placeholders read as plain starts: {}", history(e));
        assert!(history(e).contains("session 2 `cargo test` started; a message comes when it ends"));
        assert!(calls_back(&mut state, 1, &sh).wake.is_empty(), "each end comes once");
        assert_eq!(step(&mut state.engines[1], "thought: x\nact: reply(text=\"Test 1 fails.\")", &mut |_| None), Step::Reply("Test 1 fails.".into()));
    }

    #[test]
    fn your_new_ask_cancels_the_wait_and_the_job_still_calls_back() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut state = waiting_family();
        step_with(&mut state.engines[1], "thought: long\nact: start(command=\"cargo test\", wait=\"yes\")", &mut |_| None, &sh);
        assert!(state.add_text(1, "Stop that; look at the README instead."));
        let e = &state.engines[1];
        assert!(e.waiting.is_empty(), "the ask ends the wait");
        assert_eq!(phase_of(e), "code", "a new ask starts the run again");
        assert!(history(e).contains("session 1 `cargo test` started; a message comes when it ends"), "{}", history(e));
        sh.end(1, Some(0), "ok\n");
        assert_eq!(calls_back(&mut state, 1, &sh).wake, ["session 1 `cargo test` finished, exit 0 (terminal term)"], "a plain callback");
        // A reply in the same act as the wait ends the turn the same way.
        let e = &mut state.engines[1];
        let s = step_with(e, "thought: x\nact: start(command=\"cargo doc\", wait=\"yes\"); reply(text=\"Docs are building.\")", &mut |_| None, &sh);
        assert_eq!(s, Step::Reply("Docs are building.".into()));
        assert!(e.waiting.is_empty());
    }

    #[test]
    fn a_restart_while_waiting_still_wakes_it_and_says_when_the_job_was_lost() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut state = waiting_family();
        step_with(&mut state.engines[1], "thought: long\nact: start(command=\"cargo test\", wait=\"yes\")", &mut |_| None, &sh);
        // The app stops: the engine goes to its files and comes back.
        let files = crate::files::to_files(&state.engines[1]);
        assert!(files.run.contains("waiting: term#1"), "{}", files.run);
        state.engines[1] = crate::files::from_files("e", &files).unwrap();
        assert_eq!(state.engines[1].waiting, ["term#1"]);
        assert_eq!(phase_of(&state.engines[1]), "test");
        // The job died with the app: no exit code.
        sh.end(1, None, "");
        let wake = calls_back(&mut state, 1, &sh).wake;
        assert_eq!(
            wake,
            ["session 1 `cargo test` finished, no exit code (stopped); its result is not known (it was killed, or the app stopped while it ran) (terminal term)"]
        );
        assert!(state.add_wake(1, &wake.join("\n")));
        assert_eq!(phase_of(&state.engines[1]), "test");
        // A job gone from its terminal is said to be gone.
        let mut state = waiting_family();
        state.engines[1].waiting = vec!["term#7".into()];
        assert_eq!(calls_back(&mut state, 1, &sh).wake, ["session 7 is gone from terminal term: its result is not known"]);
        assert!(state.engines[1].waiting.is_empty());
    }

    #[test]
    fn a_reply_beside_a_call_that_waits_is_held_back_until_its_result_is_read() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut e = engine();
        e.id = "e".into();
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        // The report was written before the run it reports: the run still
        // runs, the reply is not sent, and the turn goes on.
        let s = step_with(&mut e, "thought: done\nact: run(command=\"cargo check\"); reply(text=\"It compiles.\")", &mut |_| None, &sh);
        assert_eq!(s, Step::Again);
        let out = e.messages.last().unwrap().text.clone();
        assert!(out.starts_with("run(command=\"cargo check\") → session 1 `cargo check`: exit 0"), "{out}");
        assert!(out.ends_with("reply(...) → not sent: this act also calls run, and a reply written beside it cannot have read its result. The result is above: read it, then reply in a step of its own."), "{out}");
        // In a step of its own it is sent.
        assert_eq!(step_with(&mut e, "thought: read it\nact: reply(text=\"It compiles: exit 0.\")", &mut |_| None, &sh), Step::Reply("It compiles: exit 0.".into()));
        // Work that calls back claims nothing yet, so a reply may go with it.
        let s = step_with(&mut e, "thought: long\nact: start(command=\"cargo test\"); reply(text=\"Tests are running.\")", &mut |_| None, &sh);
        assert_eq!(s, Step::Reply("Tests are running.".into()));
    }

    #[test]
    fn a_loaded_skills_own_tool_runs_in_the_terminal_and_is_gone_with_the_skill() {
        let sh = crate::terminal::tests::FakeShell::default();
        let mut state = crate::state::State { engines: vec![engine()], ..Default::default() };
        let e = &mut state.engines[0];
        e.id = "e".into();
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        e.skills.insert("python".into(), "---\nname: python\ndescription: Python files.\n---\nCheck before you run.\n".into());
        e.skill_tools.insert("python".into(), "# python\nPython.\n\n## tool: check\nCompile one file.\ncommand: [\"python3\", \"-m\", \"py_compile\", \"{file}\"]\ninputs:\n- file (string, required): the file\n".into());
        // Not loaded: the call is no tool of the engine's.
        step_with(e, "thought: check\nact: python.check(file=\"tree.py\")", &mut |_| None, &sh);
        assert!(e.messages.last().unwrap().text.contains("error: no tool `python.check`"), "{}", e.messages.last().unwrap().text);
        // Loading the skill says what it brings, and lists it with the tools.
        step_with(e, "thought: load\nact: skill(name=\"python\")", &mut |_| None, &sh);
        let loaded = e.messages.last().unwrap().text.clone();
        assert!(loaded.contains("Check before you run.") && loaded.contains("- python.check(file=\"...\"): Compile one file."), "{loaded}");
        assert!(sent(e, &sh).contains("\n- python.check(file=\"...\"): Compile one file. (file: string, required, the file)"));
        // The call is a run of the terminal: a session, an exit code, the last lines.
        step_with(e, "thought: check\nact: python.check(file=\"my tree.py\")", &mut |_| None, &sh);
        let out = e.messages.last().unwrap().text.clone();
        assert!(out.starts_with("python.check(file=\"my tree.py\") → session 1 `'python3' '-m' 'py_compile' 'my tree.py'`: exit 0"), "{out}");
        assert!(e.artifacts["term"].contains("'python3' '-m' 'py_compile' 'my tree.py'"), "the session is in the terminal artifact");
        // A missing input is refused before anything runs.
        step_with(e, "thought: again\nact: python.check()", &mut |_| None, &sh);
        assert!(e.messages.last().unwrap().text.contains("python.check() → error:"), "{}", e.messages.last().unwrap().text);
        // The next message unloads the skill: the tool is gone.
        assert!(state.add_text(0, "thanks"));
        let e = &mut state.engines[0];
        assert!(!sent(e, &sh).contains("python.check("));
    }

    /// A host with nothing but a clock.
    struct At(std::cell::Cell<i64>);

    impl live::Host for At {
        fn now(&self) -> i64 {
            self.0.get()
        }
        fn list(&self, a: &str, b: usize) -> Result<Vec<String>, String> {
            NoHost.list(a, b)
        }
        fn read(&self, a: &str, b: &str) -> live::Content {
            NoHost.read(a, b)
        }
        fn write(&self, a: &str, b: &str, c: &str) -> Result<(), String> {
            NoHost.write(a, b, c)
        }
        fn run(&self, j: &live::Job) -> Result<Option<i32>, String> {
            NoHost.run(j)
        }
        fn spawn(&self, j: &live::Job) -> Result<(), String> {
            NoHost.spawn(j)
        }
        fn status(&self, j: &live::Job) -> live::Proc {
            NoHost.status(j)
        }
        fn tail(&self, j: &live::Job, n: usize) -> String {
            NoHost.tail(j, n)
        }
    }

    #[test]
    fn an_engine_sets_its_own_alert_and_is_woken_by_it_with_nobody_writing() {
        let host = At(std::cell::Cell::new(1_795_356_330));
        let mut state = crate::state::State { engines: vec![engine()], ..Default::default() };
        state.engines[0].id = "e".into();
        state.engines[0].artifacts.insert("alerts".into(), "kind: alerts".into());
        let idle = |_: usize| crate::engines::Seen::default();
        // The tool is the artifact's: it is in the prompt, and the engine sets its next look.
        assert!(sent(&state.engines[0], &host).contains("- alert(name=\"...\", trigger=\"...\", text=\"...\", once=\"...\", while=\"...\"): Make an alert"));
        let e = &mut state.engines[0];
        step_with(e, "thought: look again later\nact: alert(name=\"check\", trigger=\"every 1m\", text=\"Run the tests again.\")", &mut |_| None, &host);
        assert!(e.messages.last().unwrap().text.contains("→ made alert check: every 1m."), "{}", e.messages.last().unwrap().text);
        assert_eq!(step_with(e, "thought: done for now\nact: reply(text=\"I will look again in a minute.\")", &mut |_| None, &host), Step::Reply("I will look again in a minute.".into()));
        assert!(sent(e, &host).contains("- check (every 1m): Run the tests again. · not fired yet"));
        // Nothing yet; in a turn it is not looked at; a minute on, free, it wakes: one message, a new turn.
        assert!(callbacks(&mut state, 0, false, &host, &idle).wake.is_empty());
        host.0.set(host.0.get() + 61);
        assert!(callbacks(&mut state, 0, true, &host, &idle).wake.is_empty(), "busy: not now");
        let woke = callbacks(&mut state, 0, false, &host, &idle).wake;
        assert_eq!(woke, ["Alert check fired (every 1m): Run the tests again."]);
        assert!(state.add_text(0, &woke.join("\n")));
        assert_eq!(state.engines[0].messages.last().unwrap().text, "Alert check fired (every 1m): Run the tests again.");
        assert!(callbacks(&mut state, 0, false, &host, &idle).wake.is_empty(), "it fires once for a moment");
        // The mark survives a save and a load, so a restart does not fire the same moment again.
        let files = crate::files::to_files(&state.engines[0]);
        let back = crate::files::from_files("e", &files).unwrap();
        assert_eq!(back.alert_marks, state.engines[0].alert_marks);
        assert_eq!(back.alert_marks["check"].fired, 1);
    }

    #[test]
    fn several_artifacts_of_a_kind_are_named_in_the_call() {
        let disk = crate::file::tests::FakeDisk::with(&[("a", "x")]);
        let mut e = engine();
        e.artifacts.insert("one".into(), "kind: file".into());
        e.artifacts.insert("two".into(), "kind: file".into());
        step_with(&mut e, "thought: x\nact: open(path=\"a\")", &mut |_| None, &disk);
        assert!(e.messages.last().unwrap().text.contains("several file artifacts (one, two): say which with artifact=\"...\""));
        step_with(&mut e, "thought: x\nact: open(path=\"a\", artifact=\"two\")", &mut |_| None, &disk);
        assert!(e.artifacts["two"].contains("open: a") && !e.artifacts["one"].contains("open:"));
        // Without a host the call says so.
        step(&mut e, "thought: x\nact: open(path=\"a\", artifact=\"one\")", &mut |_| None);
        assert!(e.messages.last().unwrap().text.contains("error: no file system here"));
    }

    #[test]
    fn the_scripts_artifact_lists_them_as_they_are_now() {
        let mut e = with_script();
        let system = crate::prompt::render(&e).messages()[0].text.clone();
        assert!(system.contains("### scripts — your scripts"), "{system}");
        assert!(system.contains("- script(name=\"save_note\", note=\"...\", text=\"...\"?): Save a note and read it back. (2 steps)"), "{system}");
        e.scripts.clear();
        assert!(crate::prompt::render(&e).messages()[0].text.contains("No scripts yet."));
    }
}
