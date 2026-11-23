//! A whole turn as a machine the shell feeds, so a shell with an async
//! network (the browser's `fetch`) runs the same loop as the window.
//!
//! ```text
//! Runner::start ─▶ next(Came::Start) ─▶ Next::Probe  ─▶ shell asks the server ─▶ Came::Window
//!                                   ─▶ Next::Post   ─▶ shell sends            ─▶ Came::Body
//!                                   ─▶ Next::Run    ─▶ shell runs the waits   ─▶ Came::Ran
//!                                   ─▶ Next::Waiting (an engine_call: resume when the children replied)
//!                                   ─▶ Next::Done   (replied, or why not)
//! ```
//!
//! Pure: it never sends anything. Each `next` borrows the state only
//! while it decides, so the shell can draw and take clicks while a call
//! is out. It makes the same moves as the GPUI window's `ask_step`,
//! `run_calls`, `wait_calls`, `finish_calls` and `compress`: measure the
//! prompt against the model's window (asking the server for it once),
//! compress the history at `context::COMPRESS_AT`, call the model, apply
//! the answer's instant calls in written order (`turn::begin_step`), hand
//! the shell the waiting ones, merge their results (`turn::finish_step`),
//! and again until a reply. No step limit.
//!
//! What the step asked of other engines is made here
//! (`engines::apply`); the children given a message are in `spawned`
//! for the shell to start, each with its own `Runner`.
//!
//! `Transport` is the one thing a shell implements to drive it (`io`
//! does one move's I/O with it): the browser with `fetch`, a test with a
//! fake model, a remote client over HTTP.

use crate::activity::Activity;
use crate::calls::Call;
use crate::card::Status;
use crate::context::{self, Budget, Compression, Plan, Probe, Window};
use crate::engines::{self, Seen};
use crate::inference::{Inference, OpenAi, Request};
use crate::live::{Clock, Host, Snapshots};
use crate::state::State;
use crate::turn::{self, Pending, Wait};

/// What the shell does next for the turn.
#[derive(Clone, Debug, PartialEq)]
pub enum Next {
    /// Learn `model`'s window: try these in order, give back the first
    /// that says it (`Came::Window`, `None` when none did).
    Probe { probes: Vec<Probe>, model: String },
    /// Send this to the model; give back the body (`Came::Body`).
    Post(Request),
    /// Run these waiting calls (all at once if it can); give back what
    /// each came to, in this order (`Came::Ran`).
    Run(Vec<Wait>),
    /// The step waits for these children's replies (`engine_call`). The
    /// shell's callbacks (`turn::callbacks`) say when to `Runner::resume`.
    Waiting(Vec<String>),
    /// The turn is over: `Ready` after a reply, else why it stopped.
    Done(Status),
}

/// What the shell brings back for the last `Next`.
#[derive(Clone, Debug, PartialEq)]
pub enum Came {
    /// Nothing yet: the first move.
    Start,
    Window(Option<(u32, String)>),
    Body(Result<String, String>),
    Ran(Vec<turn::Came>),
}

/// The prompt's size against the window at the last call.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub budget: Budget,
    /// Over the threshold and not compressed: why.
    pub over: Option<String>,
    /// The last compression, as a line (or why it did not happen).
    pub compressed: Option<String>,
}

/// What the runner reads from the shell at each move.
pub struct Cx<'a> {
    /// Files and terminals (the browser's refuses both, saying why).
    pub host: &'a dyn Host,
    pub clock: Clock,
    /// How the shell sees engine `j` of the state (its parent's tools read it).
    pub seen: &'a dyn Fn(&State, usize) -> Seen,
    /// A script's steps that go outside (MCP): `None` if nothing knows the call.
    pub external: &'a dyn Fn(&Call) -> Option<Result<String, String>>,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Measure,
    Llm,
    Compress(Box<Compression>),
    Calls(Box<Pending>),
    Over,
}

/// One engine's turn in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct Runner {
    /// The engine, by id: indices shift when one is deleted.
    pub id: String,
    pub step: usize,
    compressed: bool,
    ai: OpenAi,
    /// The model's window, once known (the shell may keep it per model).
    pub window: Option<Window>,
    phase: Phase,
    /// What it is doing now, for the tint and the labels.
    pub activity: Activity,
    pub measured: Option<Measured>,
    /// Engines this turn gave a message: the shell starts their turns.
    pub spawned: Vec<usize>,
    /// Things worth saying that did not stop the turn.
    pub notes: Vec<String>,
    /// The step's instant calls run while its answer streams (`early`).
    early: Option<Box<crate::early::Early>>,
    /// The turn's spans as they close (`crate::spans`), for the shell to
    /// take (`spans`) and append to the engine's log.
    pub trace: crate::spans::Trace,
    /// The time of the move being decided (`next_at`), unix ms.
    now: i64,
}

impl Runner {
    /// A new turn for engine `i`: its newest message is the user's. Why
    /// not, if it cannot run here: a session engine (its program runs on
    /// a computer), or no model chosen.
    pub fn start(state: &State, i: usize, window: Option<Window>) -> Result<Runner, Status> {
        Runner::at(state, i, 1, window)
    }

    /// Go on with engine `i`'s turn after the children its `engine_call`
    /// waited for replied (`turn::callbacks` said `resumed`).
    pub fn resume(state: &State, i: usize, window: Option<Window>) -> Result<Runner, Status> {
        let step = state.engines.get(i).map_or(1, |e| e.run.step as usize + 1);
        Runner::at(state, i, step, window)
    }

    fn at(state: &State, i: usize, step: usize, window: Option<Window>) -> Result<Runner, Status> {
        let e = state.engines.get(i).ok_or_else(|| Status::Failed("no such engine".into()))?;
        if let Some(s) = &e.session {
            return Err(Status::Missing(format!(
                "a {} session runs its program on a computer; open this engine in the desktop app",
                s.tool
            )));
        }
        if e.inference.model.is_empty() || e.inference.base_url.is_empty() {
            return Err(Status::Missing("no model: choose one in Settings → Models, then send again".into()));
        }
        let trigger = if step > 1 { crate::spans::Trigger::Callback } else { crate::spans::trigger_of(e) };
        let goal = e.messages.iter().rev().find(|m| m.role == crate::inference::Role::User).map_or("", |m| m.text.as_str());
        Ok(Runner {
            trace: crate::spans::Trace::new(&e.id, trigger, goal),
            now: 0,
            id: e.id.clone(),
            step,
            compressed: false,
            ai: e.inference.clone(),
            window,
            phase: Phase::Measure,
            activity: Activity::Llm { step },
            measured: None,
            spawned: Vec::new(),
            notes: Vec::new(),
            early: None,
        })
    }

    /// Is the turn over?
    pub fn is_over(&self) -> bool {
        self.phase == Phase::Over
    }

    // ── Streaming (`crate::stream`, `crate::early`): additive; a shell
    //    that never streams never calls these and nothing changes. ──

    /// Is the post out now the model's answer to a step (so its text may
    /// stream into a `stream::Reader`)? Not a compression's summary.
    pub fn streams(&self) -> bool {
        self.phase == Phase::Llm
    }

    /// A call of the answer closed while it streams (`stream::Event::CallDone`):
    /// run it now if it is an early instant call (`crate::early`). What
    /// ran is kept only if the whole answer reads with the same calls.
    pub fn early_call(&mut self, state: &State, cx: &Cx, call: &Call) -> bool {
        let Some(i) = state.index_of(&self.id).filter(|_| self.phase == Phase::Llm) else { return false };
        let early = self.early.get_or_insert_with(|| Box::new(crate::early::Early::new(&state.engines[i])));
        let mut external = |c: &Call| (cx.external)(c);
        early.offer(call, &mut external, cx.host)
    }

    /// The early run so far, for a shell to draw (`Early::engine`).
    pub fn early(&self) -> Option<&crate::early::Early> {
        self.early.as_deref()
    }

    // ── Spans (`crate::spans`): the shell passes the time of each move
    //    (`next_at`) and, if it can, when the answer's first text and
    //    component came and how long each waiting call took. ──

    /// The next move, at `cx.clock`'s second. A shell with a millisecond
    /// clock calls `next_at`.
    pub fn next(&mut self, state: &mut State, cx: &Cx, came: Came) -> Next {
        self.next_at(state, cx, came, cx.clock.unix * 1000)
    }

    /// The answer's first text came at `now_ms` (while it streams).
    pub fn first_token(&mut self, now_ms: i64) {
        self.trace.first_token(now_ms);
    }

    /// The reply's component was known at `now_ms` (while it streams).
    pub fn first_component(&mut self, now_ms: i64) {
        self.trace.first_component(now_ms);
    }

    /// When each of the step's waiting calls started and ended, in the
    /// order `Next::Run` gave them.
    pub fn wait_times(&mut self, times: Vec<(i64, i64)>) {
        self.trace.wait_times(times);
    }

    /// The spans closed since the last call, for the shell's log.
    pub fn spans(&mut self) -> Vec<crate::spans::Span> {
        self.trace.take()
    }

    /// The next move, given what the last one came to, at `now_ms` (unix
    /// milliseconds): the move's spans close at that time.
    pub fn next_at(&mut self, state: &mut State, cx: &Cx, came: Came, now_ms: i64) -> Next {
        self.now = now_ms;
        self.trace.moved(now_ms);
        if let Came::Body(body) = &came {
            let error = match body {
                Ok(b) => self.ai.reply(b).err().map(|e| format!("{e:?}")),
                Err(_) => None,
            };
            self.trace.body(now_ms, body, &self.ai, error);
        }
        let Some(i) = state.index_of(&self.id) else {
            return self.over(Status::Failed("the engine was deleted during its turn".into()));
        };
        let phase = std::mem::replace(&mut self.phase, Phase::Over);
        match (phase, came) {
            (Phase::Measure, Came::Window(found)) => {
                self.window = Some(Window::of(&self.ai, found));
                self.measure(state, i, cx)
            }
            (Phase::Measure, _) => self.measure(state, i, cx),
            (Phase::Compress(c), Came::Body(body)) => {
                let summary = body.and_then(|b| self.ai.reply(&b).map_err(|e| format!("{e:?}")));
                let note = match summary {
                    Ok(s) => context::apply(&mut state.engines[i], &c, &s),
                    Err(e) => Err(format!("the summary call failed: {e}")),
                };
                let note = note.unwrap_or_else(|why| format!("not compressed: {why}"));
                self.measured = Some(Measured { budget: c.budget.clone(), over: None, compressed: Some(note) });
                self.compressed = true;
                self.measure(state, i, cx)
            }
            (Phase::Llm, Came::Body(Err(why))) => self.over(Status::Failed(why)),
            (Phase::Llm, Came::Body(Ok(body))) => match self.ai.reply(&body) {
                Err(e) => self.over(Status::Failed(format!("the reply did not read: {e:?}"))),
                Ok(text) => self.answered(state, i, cx, text),
            },
            (Phase::Calls(p), Came::Ran(came)) => self.finish(state, i, cx, *p, came),
            (phase, came) => {
                self.phase = phase;
                self.over(Status::Failed(format!("the shell sent {came:?} out of turn")))
            }
        }
    }

    fn over(&mut self, status: Status) -> Next {
        self.over_with(status, serde_json::Map::new())
    }

    fn over_with(&mut self, status: Status, mut more: serde_json::Map<String, serde_json::Value>) -> Next {
        self.phase = Phase::Over;
        self.activity = Activity::of_status(&status);
        let outcome = match &status {
            Status::Ready | Status::Done(_) => "ready",
            Status::Missing(_) => "missing",
            Status::Failed(why) => {
                more.insert("error".into(), crate::spans::short(why, 160).into());
                "failed"
            }
            _ => "ended",
        };
        more.insert("model".into(), self.ai.model.clone().into());
        self.trace.end(self.now, outcome, more);
        Next::Done(status)
    }

    /// Render, measure against the window, then compress or send.
    fn measure(&mut self, state: &mut State, i: usize, cx: &Cx) -> Next {
        let Some(window) = self.window.clone() else {
            let probes = context::probes(&self.ai);
            if probes.is_empty() {
                self.window = Some(Window::of(&self.ai, None));
                return self.measure(state, i, cx);
            }
            self.phase = Phase::Measure;
            self.activity = Activity::Working("asking the server for the model's context size".into());
            return Next::Probe { probes, model: self.ai.model.clone() };
        };
        let family = self.family(state, i, cx);
        let e = &state.engines[i];
        let at = Snapshots { clock: Some(cx.clock), host: Some(cx.host), family: Some(&family) };
        let rendered = crate::prompt::render_with(e, &at);
        let (budget, over) = match context::plan(e, &rendered, &window) {
            Plan::Compress(c) if !self.compressed => {
                let req = self.ai.request(&c.request);
                self.trace.post(self.now, self.step, true, false, Some(c.budget.tokens));
                self.activity = Activity::Compressing { step: self.step };
                self.phase = Phase::Compress(Box::new(c));
                return Next::Post(req);
            }
            Plan::Compress(c) => (c.budget, Some("still over the threshold after compressing; sent as it is".to_string())),
            Plan::Over(b, why) => (b, Some(why)),
            Plan::Send(b) => (b, None),
        };
        let compressed = self.measured.take().and_then(|m| m.compressed);
        self.trace.post(self.now, self.step, false, self.compressed, Some(budget.tokens));
        self.measured = Some(Measured { budget, over, compressed });
        self.activity = Activity::Llm { step: self.step };
        self.phase = Phase::Llm;
        Next::Post(self.ai.request(&rendered.messages()))
    }

    fn family(&self, state: &State, i: usize, cx: &Cx) -> engines::Family {
        let mut f = engines::family(state, i, &|j| (cx.seen)(state, j));
        f.offset = cx.clock.offset;
        f
    }

    /// The model answered: the instant calls now, the children it asked
    /// made, then the waiting calls for the shell (or the end of the step).
    fn answered(&mut self, state: &mut State, i: usize, cx: &Cx, text: String) -> Next {
        self.activity = Activity::answered(&state.engines[i], &text, self.step);
        self.trace.answered(self.now);
        let mut family = self.family(state, i, cx);
        let external = |c: &Call| (cx.external)(c);
        let mut external = external;
        let pending = match self.early.take() {
            Some(early) => early.finish(&mut state.engines[i], &text, &mut external, cx.host, Some(&mut family)),
            None => turn::begin_step(&mut state.engines[i], &text, &mut external, cx.host, Some(&mut family)),
        };
        let applied = engines::apply(state, i, family.effects);
        self.notes.extend(applied.notes);
        self.spawned.extend(applied.run);
        if pending.waits.is_empty() {
            return self.finish(state, i, cx, pending, Vec::new());
        }
        let waits = pending.waits.iter().map(|(_, w)| w.clone()).collect();
        self.trace.run(self.now, pending.waiting_names());
        self.phase = Phase::Calls(Box::new(pending));
        Next::Run(waits)
    }

    /// The step's results are in: the model again, a wait, or the end.
    fn finish(&mut self, state: &mut State, i: usize, cx: &Cx, pending: Pending, came: Vec<turn::Came>) -> Next {
        let before = state.engines[i].messages.len();
        let step = turn::finish_step(&mut state.engines[i], pending, came);
        let e = &state.engines[i];
        let results = e.messages.get(before..).unwrap_or_default().iter().filter(|m| m.role == crate::inference::Role::Tool).map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n");
        let phase = e.phase_list().ok().and_then(|p| p.get(e.run.phase).map(|p| p.name.clone())).unwrap_or_default();
        self.trace.results(self.now, self.step, &results, &phase);
        match Activity::after(&step, self.step) {
            Activity::Llm { step } => {
                self.step = step;
                self.compressed = false;
                self.measure(state, i, cx)
            }
            waiting @ Activity::Waiting(_) => {
                self.activity = waiting;
                self.phase = Phase::Over;
                let turn::Step::Wait(on) = step else { unreachable!("after() said waiting") };
                let mut more = serde_json::Map::new();
                more.insert("waiting_on".into(), on.join(", ").into());
                self.trace.end(self.now, "waiting", more);
                Next::Waiting(on)
            }
            _ => {
                let more = crate::spans::reply_attrs(&state.engines[i]);
                self.over_with(Status::Ready, more)
            }
        }
    }
}

/// How a shell reaches a model server. The only thing it implements to
/// drive a `Runner`: the browser with `fetch`, a test with a fake model,
/// a remote client through its host.
#[allow(async_fn_in_trait)]
pub trait Transport {
    /// POST `req.body` (JSON) to `req.url`; the body that came back.
    async fn post(&self, req: &Request) -> Result<String, String>;
    /// GET `url`; the body.
    async fn get(&self, url: &str, key_env: &Option<String>) -> Result<String, String>;
    /// POST asking the server to stream (`stream::streaming`), each delta
    /// to `on` as it comes; the body a plain `post` would have given
    /// (`stream::Sse::finish`), so the runner reads it as always. A server
    /// that does not stream answers with a plain body, which comes back as
    /// it is. By default: no streaming, a plain `post`.
    async fn stream(&self, req: &Request, on: &mut dyn FnMut(crate::stream::Delta)) -> Result<String, String> {
        let _ = on;
        self.post(req).await
    }
}

/// One move's I/O: the probes in order, the post, or the waiting calls
/// (through `host` and `external`, one after another). `Waiting` and
/// `Done` need none: `Came::Start` comes back.
pub async fn io<T: Transport>(next: &Next, t: &T, host: &dyn Host, external: &dyn Fn(&Call) -> Option<Result<String, String>>) -> Came {
    match next {
        Next::Probe { probes, model } => {
            for p in probes {
                let body = match p.post() {
                    Some(req) => t.post(&req).await,
                    None => t.get(&p.url, &p.key_env).await,
                };
                if let Ok(body) = body
                    && let Some(found) = context::read_probe(p.server, &body, model)
                {
                    return Came::Window(Some(found));
                }
            }
            Came::Window(None)
        }
        Next::Post(req) => Came::Body(t.post(req).await),
        Next::Run(waits) => {
            let mut ext = |c: &Call| external(c);
            Came::Ran(waits.iter().map(|w| turn::run_wait(w, &mut ext, host)).collect())
        }
        Next::Waiting(_) | Next::Done(_) => Came::Start,
    }
}

/// `io`, but the model's answer to a step streams (`Transport::stream`):
/// each delta goes to `on` as it comes. Everything else as `io`.
pub async fn io_streaming<T: Transport>(
    next: &Next,
    streams: bool,
    t: &T,
    host: &dyn Host,
    external: &dyn Fn(&Call) -> Option<Result<String, String>>,
    on: &mut dyn FnMut(crate::stream::Delta),
) -> Came {
    match next {
        Next::Post(req) if streams => Came::Body(t.stream(&crate::stream::streaming(req), on).await),
        _ => io(next, t, host, external).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{Message, Role};
    use crate::response::Responses;
    use crate::store::ready;
    use std::cell::RefCell;

    /// A model that answers from a list, in order, and keeps what it was sent.
    struct Fake {
        answers: RefCell<Vec<String>>,
        sent: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(answers: &[&str]) -> Fake {
            Fake { answers: RefCell::new(answers.iter().rev().map(|s| s.to_string()).collect()), sent: RefCell::default() }
        }
        fn body(text: &str) -> String {
            serde_json::json!({ "choices": [{ "message": { "content": text } }] }).to_string()
        }
    }

    impl Transport for Fake {
        async fn post(&self, req: &Request) -> Result<String, String> {
            self.sent.borrow_mut().push(req.body.clone());
            self.answers.borrow_mut().pop().map(|a| Fake::body(&a)).ok_or_else(|| "no more answers".to_string())
        }
        async fn get(&self, _: &str, _: &Option<String>) -> Result<String, String> {
            Err("no server".into())
        }
        /// The answer as a server streams it, a few bytes at a time.
        async fn stream(&self, req: &Request, on: &mut dyn FnMut(crate::stream::Delta)) -> Result<String, String> {
            assert!(req.body.contains("\"stream\":true"));
            let body = serde_json::from_str::<serde_json::Value>(&self.post(req).await?).unwrap();
            let text = body.pointer("/choices/0/message/content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut sse = crate::stream::Sse::new(req);
            let chars: Vec<char> = text.chars().collect();
            let mut raw = String::new();
            for piece in chars.chunks(4) {
                let chunk = serde_json::json!({ "choices": [{ "delta": { "content": piece.iter().collect::<String>() } }] });
                raw.push_str(&format!("data: {chunk}\n\n"));
            }
            raw.push_str("data: [DONE]\n\n");
            for bytes in raw.as_bytes().chunks(7) {
                for d in sse.push(bytes) {
                    on(d);
                }
            }
            Ok(sse.finish())
        }
    }

    fn engine_state() -> State {
        let mut state = State::default();
        let i = state.add_engine();
        let e = &mut state.engines[i];
        e.inference = OpenAi { base_url: "http://x/v1".into(), model: "m".into(), context: Some(32_000), ..OpenAi::default() };
        e.response =
            Responses::from_md("format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n").unwrap().0;
        e.artifacts.insert("plan".into(), "first draft".into());
        e.tools = vec!["read".into(), "write".into(), "reply".into()];
        state.add_text(i, "Rewrite the plan, then tell me.");
        state
    }

    const ANSWERS: [&str; 3] = [
        "thought: look first\nact: read(name=\"plan\")",
        "thought: now write\nact: write(name=\"plan\", text=\"second draft\")",
        "thought: done\nact: reply(text=\"The plan says: second draft.\", ask=\"confirm\")",
    ];

    fn seen(_: &State, _: usize) -> Seen {
        Seen::default()
    }

    /// The turn through the runner and `io`, as the browser drives it.
    fn through_runner(state: &mut State, model: &Fake) -> (Status, usize) {
        let host = turn::NoHost;
        let none = |_: &Call| None;
        let cx = Cx { host: &host, clock: Clock::new(0, 0), seen: &seen, external: &none };
        let mut r = Runner::start(state, 0, None).unwrap();
        let mut came = Came::Start;
        let mut moves = 0;
        loop {
            let next = r.next(state, &cx, came);
            moves += 1;
            if let Next::Done(s) = next {
                return (s, moves);
            }
            came = ready(io(&next, model, &host, &none));
        }
    }

    /// The same turn by hand with the blocking step (`turn::step_in`), as
    /// the tests and the window always ran it.
    fn by_hand(state: &mut State, model: &Fake) {
        let host = turn::NoHost;
        loop {
            let e = &state.engines[0];
            let mut fam = engines::family(state, 0, &|_| Seen::default());
            let at = Snapshots { clock: Some(Clock::new(0, 0)), host: Some(&host), family: Some(&fam) };
            let req = e.inference.request(&crate::prompt::render_with(e, &at).messages());
            let text = e.inference.reply(&ready(model.post(&req)).unwrap()).unwrap();
            match turn::step_in(&mut state.engines[0], &text, &mut |_| None, &host, Some(&mut fam)) {
                turn::Step::Again => continue,
                _ => return,
            }
        }
    }

    #[test]
    fn the_async_path_makes_the_same_turn_as_the_blocking_one() {
        let (mut a, mut b) = (engine_state(), engine_state());
        let (ma, mb) = (Fake::new(&ANSWERS), Fake::new(&ANSWERS));
        let (status, moves) = through_runner(&mut a, &ma);
        by_hand(&mut b, &mb);
        assert_eq!(status, Status::Ready);
        assert_eq!(moves, 4, "three posts, then done");
        assert_eq!(a.engines[0].messages, b.engines[0].messages, "the same log");
        assert_eq!(a.engines[0].artifacts, b.engines[0].artifacts);
        assert_eq!(*ma.sent.borrow(), *mb.sent.borrow(), "the same requests, byte for byte");
        assert_eq!(a.engines[0].artifacts["plan"], "second draft");
        let last = a.engines[0].messages.last().unwrap();
        assert_eq!(last.role, Role::Assistant);
        assert!(crate::ask::ask_in(&a.engines[0], &last.text).is_some(), "the reply asks");
    }

    #[test]
    fn a_streamed_turn_with_early_calls_is_the_same_turn() {
        let (mut a, mut b) = (engine_state(), engine_state());
        let (ma, mb) = (Fake::new(&ANSWERS), Fake::new(&ANSWERS));
        through_runner(&mut a, &ma);
        let host = turn::NoHost;
        let none = |_: &Call| None;
        let cx = Cx { host: &host, clock: Clock::new(0, 0), seen: &seen, external: &none };
        let mut r = Runner::start(&b, 0, None).unwrap();
        let mut came = Came::Start;
        let mut early = 0;
        let mut reader_texts = Vec::new();
        loop {
            let next = r.next(&mut b, &cx, came);
            if let Next::Done(s) = next {
                assert_eq!(s, Status::Ready);
                break;
            }
            let mut reader = crate::stream::Reader::new(&b.engines[0]);
            let streams = r.streams();
            let state = &b;
            let runner = &mut r;
            let mut on = |d: crate::stream::Delta| {
                for ev in reader.feed(&d) {
                    if let crate::stream::Event::CallDone { call, .. } = ev {
                        early += usize::from(runner.early_call(state, &cx, &call));
                    }
                }
            };
            came = ready(io_streaming(&next, streams, &mb, &host, &none, &mut on));
            reader_texts.push(reader.text.clone());
        }
        assert_eq!(a.engines[0].messages, b.engines[0].messages, "the same log");
        assert_eq!(a.engines[0].artifacts, b.engines[0].artifacts);
        assert_eq!(*ma.sent.borrow(), mb.sent.borrow().iter().map(|s| s.replace(",\"stream\":true", "")).collect::<Vec<_>>());
        assert_eq!(early, 2, "read and write ran as they closed");
        assert_eq!(reader_texts, ANSWERS, "every answer streamed");
    }

    /// The turn through the runner with a clock that moves 100 ms per
    /// move: its spans, as the shell takes them.
    fn timed(state: &mut State, model: &Fake, external: &dyn Fn(&Call) -> Option<Result<String, String>>) -> Vec<crate::spans::Span> {
        let host = turn::NoHost;
        let cx = Cx { host: &host, clock: Clock::new(0, 0), seen: &seen, external };
        let mut r = Runner::start(state, 0, None).unwrap();
        let (mut came, mut now, mut spans) = (Came::Start, 1_000, Vec::new());
        loop {
            let next = r.next_at(state, &cx, came, now);
            spans.extend(r.spans());
            if let Next::Done(_) = next {
                return spans;
            }
            if matches!(next, Next::Post(_)) {
                r.first_token(now + 30);
            }
            if let Next::Run(w) = &next {
                r.wait_times(w.iter().map(|_| (now + 10, now + 60)).collect());
            }
            came = ready(io(&next, model, &host, external));
            now += 100;
        }
    }

    #[test]
    fn a_turn_emits_its_spans_at_the_times_the_shell_gives() {
        let mut state = engine_state();
        let model = Fake::new(&ANSWERS);
        let spans = timed(&mut state, &model, &|_| None);
        let of = |k: crate::spans::Kind| spans.iter().filter(|s| s.kind == k).collect::<Vec<_>>();
        use crate::spans::Kind;
        let turn = of(Kind::Turn);
        assert_eq!(turn.len(), 1);
        let t = turn[0];
        assert_eq!((t.start_ms, t.end_ms, t.str("trigger"), t.str("outcome")), (1_000, 1_300, Some("user"), Some("ready")));
        assert_eq!((t.num("steps"), t.str("ask"), t.flag("asked")), (Some(3.0), Some("confirm"), true));
        assert_eq!(t.str("goal"), Some("Rewrite the plan, then tell me."));
        let llm = of(Kind::Llm);
        assert_eq!(llm.iter().map(|s| (s.start_ms, s.end_ms)).collect::<Vec<_>>(), [(1_000, 1_100), (1_100, 1_200), (1_200, 1_300)]);
        assert_eq!((llm[0].start_ms, llm[0].end_ms, llm[0].num("ttft_ms")), (1_000, 1_100, Some(30.0)));
        assert_eq!((llm[0].str("model"), llm[0].str("provider"), llm[0].flag("tokens_est")), (Some("m"), Some("x"), true));
        let tools = of(Kind::Tool);
        assert_eq!(tools.iter().map(|s| (s.str("name").unwrap(), s.str("mode").unwrap(), s.flag("ok"))).collect::<Vec<_>>(), [("read", "instant", true), ("write", "instant", true)]);
        // Every span sits in the tree under the turn.
        let roots = crate::spans::tree(&spans);
        assert_eq!(roots.len(), 1, "{roots:?}");
        let flat: Vec<(usize, Kind)> = roots[0].flat().into_iter().map(|(d, s)| (d, s.kind)).collect();
        assert_eq!(flat[..4], [(0, Kind::Turn), (1, Kind::Step), (2, Kind::Llm), (2, Kind::Tool)]);
        assert_eq!(of(Kind::Step).len(), 3);
        assert!(state.engines[0].messages.iter().all(|m| !m.text.contains("spans")), "never in the history");
    }

    #[test]
    fn a_waiting_call_is_timed_by_the_shell_and_a_failure_is_marked() {
        let mut state = engine_state();
        state.engines[0].tools.push("mcp.fetch".into());
        let model = Fake::new(&["thought: fetch\nact: mcp.fetch(url=\"u\")", "thought: done\nact: reply(text=\"ok\")"]);
        let spans = timed(&mut state, &model, &|c: &Call| (c.name == "mcp.fetch").then(|| Err("refused".to_string())));
        let tool = spans.iter().find(|s| s.kind == crate::spans::Kind::Tool).unwrap();
        assert_eq!((tool.str("name"), tool.str("mode"), tool.flag("ok"), tool.str("error")), (Some("mcp.fetch"), Some("waiting"), false, Some("refused")));
        assert_eq!((tool.start_ms, tool.end_ms), (1_110, 1_160), "the times the shell gave");
        let mut tasks = crate::spans::Tasks::default();
        let mut all = spans.clone();
        tasks.adopt(&mut all);
        let task = all.iter().find(|s| s.kind == crate::spans::Kind::Task).unwrap();
        assert_eq!((task.str("outcome"), task.str("by")), (Some("ready"), Some("user")));
    }

    #[test]
    fn a_failed_call_ends_the_turn_and_says_why() {
        let mut state = engine_state();
        let model = Fake::new(&[]);
        let (status, _) = through_runner(&mut state, &model);
        assert_eq!(status, Status::Failed("no more answers".into()));
        assert_eq!(state.engines[0].messages.len(), 1, "nothing added");
    }

    #[test]
    fn it_asks_the_server_for_the_window_once_and_refuses_what_cannot_run() {
        let mut state = engine_state();
        state.engines[0].inference.context = None;
        let host = turn::NoHost;
        let none = |_: &Call| None;
        let cx = Cx { host: &host, clock: Clock::new(0, 0), seen: &seen, external: &none };
        let mut r = Runner::start(&state, 0, None).unwrap();
        let Next::Probe { probes: p, model } = r.next(&mut state, &cx, Came::Start) else { panic!() };
        assert_eq!(model, "m");
        assert!(p[0].url.ends_with("/api/v0/models"));
        let Next::Post(_) = r.next(&mut state, &cx, Came::Window(Some((4096, "LM Studio".into())))) else { panic!() };
        assert_eq!(r.window.as_ref().map(|w| w.tokens), Some(4096));

        state.engines[0].inference.model.clear();
        assert!(matches!(Runner::start(&state, 0, None), Err(Status::Missing(_))));
        state.engines[0].session = Some(crate::session::SessionRef { tool: "claude".into(), id: None });
        assert!(matches!(Runner::start(&state, 0, None), Err(Status::Missing(_))));
        let _ = Message { role: Role::User, text: String::new() };
    }
}
