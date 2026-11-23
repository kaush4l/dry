//! Early instant calls: an act's instant calls run as soon as each one's
//! `)` arrives in the streaming answer (`crate::stream`), still in written
//! order, while the model writes the rest.
//!
//! ```text
//! open(a) ─)─▶ runs now      edit(a,…) ─)─▶ runs now      run("cargo test") ─)─▶ waits, as always
//! …answer ends ─▶ the full parser reads it: the same calls? keep what ran, go on (turn::act_step)
//!                                            not the same?  drop it all, run the step as today
//! ```
//!
//! The meaning of a step never changes (docs/engine-standard.md, "One step, many calls"): instant
//! calls in written order, then every waiting call launched together when
//! the act ends, results merged in written order. So:
//!
//! - **Only calls that touch nothing but the engine and the files** run
//!   early (`EARLY`: open, focus, edit, save, write, read, list,
//!   goto_phase, skill). Every waiting call (run, an MCP call) keeps its
//!   place for the act's end, as today. The first call of another sort
//!   (reply, post, engine_call, start, script, learn, …) stops the early
//!   run: it and every call after it run when the act ends, in order.
//! - **On a copy.** The calls run on a copy of the engine, the answer's
//!   message already saved and the step counted, as `begin_step` does;
//!   file writes are held (`Held`) and read back by later calls. Only when
//!   the whole answer reads with the same first calls is the copy kept
//!   and the writes made; else everything is dropped and the step runs
//!   as it would have without streaming. A step's results are the same
//!   with and without early start (the test below).

use std::cell::RefCell;

use crate::calls::{self, Call};
use crate::engines::Family;
use crate::live::{self, Content, Host, Job, Proc};
use crate::response::{ResponseFormat, Responses};
use crate::state::Engine;
use crate::turn::{self, ACT, External, Pending, Pre};

/// The calls that may run before the act ends.
pub const EARLY: &[&str] = &["open", "focus", "edit", "save", "write", "read", "list", "goto_phase", "skill"];

/// One step's early run, while its answer streams.
#[derive(Clone, Debug, PartialEq)]
pub struct Early {
    /// The engine when the step began: it must be the same at the end.
    base: Engine,
    copy: Engine,
    toolbox: Vec<String>,
    /// The calls decided so far, in written order, and what each came to.
    offered: Vec<Call>,
    pre: Vec<Pre>,
    /// A call that must wait for the act's end came: nothing more runs early.
    stopped: bool,
    /// Writes held until the answer is known: (call, root, path, text).
    writes: RefCell<Vec<(usize, String, String, String)>>,
}

impl Early {
    pub fn new(e: &Engine) -> Early {
        let mut copy = e.clone();
        turn::open_step(&mut copy, "");
        let toolbox = live::toolbox(&copy);
        Early { base: e.clone(), copy, toolbox, offered: Vec::new(), pre: Vec::new(), stopped: false, writes: RefCell::default() }
    }

    /// A call of the act closed. Run it now if it may (true), else keep
    /// its place: a waiting call for the end, anything else stops the run.
    pub fn offer(&mut self, call: &Call, external: &mut External, host: &dyn Host) -> bool {
        if self.stopped {
            return false;
        }
        if turn::waits(&self.copy, call, &self.toolbox) {
            self.pre.push(Pre::Waits(live::waits(&self.copy, call)));
            self.offered.push(call.clone());
            return false;
        }
        if !EARLY.contains(&call.name.as_str()) {
            self.stopped = true;
            return false;
        }
        let held = Held { inner: host, writes: &self.writes, call: self.offered.len() };
        let done = turn::run_call(&mut self.copy, call, &self.toolbox, external, &held, None);
        self.pre.push(Pre::Done(done));
        self.offered.push(call.clone());
        true
    }

    /// How many calls ran early.
    pub fn ran(&self) -> usize {
        self.pre.iter().filter(|p| matches!(p, Pre::Done(_))).count()
    }

    /// The engine as the early calls left it, for a shell to draw (files
    /// opened, artifacts written) before the act ends. Not yet true.
    pub fn engine(&self) -> &Engine {
        &self.copy
    }

    /// The answer is all in: `begin_step`'s half of the step, with what ran
    /// early kept when the whole answer has the same first calls.
    pub fn finish(self, e: &mut Engine, text: &str, external: &mut External, host: &dyn Host, family: Option<&mut Family>) -> Pending {
        let same = !self.offered.is_empty()
            && *e == self.base
            && !e.response.is_empty()
            && e.format
                .parse(&e.response, text)
                .ok()
                .and_then(|a| calls::parse(Responses::get(&a, ACT).unwrap_or("")).ok())
                .is_some_and(|list| list.starts_with(&self.offered));
        if !same {
            return turn::begin_step(e, text, external, host, family);
        }
        let Early { mut copy, mut pre, writes, .. } = self;
        for (n, root, path, body) in writes.into_inner() {
            if let Err(why) = host.write(&root, &path, &body)
                && let Some(Pre::Done(done)) = pre.get_mut(n)
            {
                *done = Err(why);
            }
        }
        if let Some(m) = copy.messages.last_mut() {
            m.text = text.trim().to_string();
        }
        *e = copy;
        turn::act_step(e, text, external, host, family, pre)
    }
}

/// The shell's host with its file writes held: read back by the calls
/// after them, made only if the early run is kept.
struct Held<'a> {
    inner: &'a dyn Host,
    writes: &'a RefCell<Vec<(usize, String, String, String)>>,
    call: usize,
}

impl Host for Held<'_> {
    fn now(&self) -> i64 {
        self.inner.now()
    }
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        let mut files = self.inner.list(root, max)?;
        for (_, r, p, _) in self.writes.borrow().iter() {
            if r == root && !files.contains(p) && files.len() < max {
                files.push(p.clone());
            }
        }
        Ok(files)
    }
    fn read(&self, root: &str, path: &str) -> Content {
        match self.writes.borrow().iter().rev().find(|(_, r, p, _)| r == root && p == path) {
            Some((_, _, _, text)) => Content::Text(text.clone()),
            None => self.inner.read(root, path),
        }
    }
    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        self.writes.borrow_mut().push((self.call, root.to_string(), path.to_string(), text.to_string()));
        Ok(())
    }
    fn run(&self, _: &Job) -> Result<Option<i32>, String> {
        Err("not before the act ends".into())
    }
    fn spawn(&self, _: &Job) -> Result<(), String> {
        Err("not before the act ends".into())
    }
    fn status(&self, job: &Job) -> Proc {
        self.inner.status(job)
    }
    fn tail(&self, job: &Job, lines: usize) -> String {
        self.inner.tail(job, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::tests::FakeDisk;
    use crate::inference::OpenAi;
    use crate::stream::{Delta, Event, Reader};
    use crate::turn::{Came, finish_step};

    fn engine() -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.tools = vec!["read".into(), "write".into(), "list".into(), "reply".into()];
        e.artifacts.insert("code".into(), "kind: file\nroot: .".into());
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        e
    }

    fn files() -> FakeDisk {
        FakeDisk::with(&[("src/a.rs", "fn a() { x }\n"), ("README.md", "hi\n")])
    }

    /// A step as today: the whole answer, then its waits, then the merge.
    fn today(text: &str) -> (Engine, Vec<(String, String)>) {
        let (mut e, disk) = (engine(), files());
        let pending = turn::begin_step(&mut e, text, &mut |_| None, &disk, None);
        let came: Vec<Came> = pending.waits.iter().map(|(_, w)| turn::run_wait(w, &mut |_| None, &disk)).collect();
        finish_step(&mut e, pending, came);
        (e, disk.files.borrow().clone().into_iter().collect())
    }

    /// The same step streamed a few characters at a time, each closed
    /// call offered at once. How many ran early, too.
    fn streamed(text: &str) -> (Engine, Vec<(String, String)>, usize) {
        let (mut e, disk) = (engine(), files());
        let mut reader = Reader::new(&e);
        let mut early = Early::new(&e);
        let chars: Vec<char> = text.chars().collect();
        let mut writes_before_end = 0;
        for piece in chars.chunks(3) {
            for ev in reader.feed(&Delta::Text(piece.iter().collect())) {
                if let Event::CallDone { call, .. } = ev {
                    early.offer(&call, &mut |_| None, &disk);
                }
            }
            writes_before_end =
                writes_before_end.max(usize::from(disk.files.borrow().get("src/a.rs").map(String::as_str) != Some("fn a() { x }\n")));
        }
        for ev in reader.finish() {
            if let Event::CallDone { call, .. } = ev {
                early.offer(&call, &mut |_| None, &disk);
            }
        }
        assert_eq!(writes_before_end, 0, "nothing reaches the disk before the answer is known");
        let ran = early.ran();
        let pending = early.finish(&mut e, text, &mut |_| None, &disk, None);
        let came: Vec<Came> = pending.waits.iter().map(|(_, w)| turn::run_wait(w, &mut |_| None, &disk)).collect();
        finish_step(&mut e, pending, came);
        (e, disk.files.borrow().clone().into_iter().collect(), ran)
    }

    #[test]
    fn a_step_comes_to_the_same_with_and_without_early_start() {
        let cases = [
            // Instants, a wait between them, a stop (reply), all kept.
            (
                "thought: fix\nact: open(path=\"src/a.rs\"); edit(path=\"src/a.rs\", old=\"x\", new=\"y\"); run(command=\"cargo test\"); save(path=\"src/a.rs\"); write(name=\"plan\", text=\"done\"); read(name=\"plan\"); reply(text=\"Fixed.\", ask=\"confirm\")",
                5,
            ),
            // A save then a read of what it saved (held, read back).
            (
                "thought: w\nact: open(path=\"README.md\"); edit(path=\"README.md\", old=\"hi\", new=\"hello\"); save(path=\"README.md\"); list(); reply(text=\"ok\")",
                4,
            ),
            // The act breaks after the early calls: nothing of them is kept.
            (
                "thought: broken\nact: open(path=\"src/a.rs\"); edit(path=\"src/a.rs\", old=\"x\", new=\"z\"); save(path=\"src/a.rs\"); reply(text=\"open",
                3,
            ),
            // No thought: the answer is off the contract, so nothing runs at all.
            ("act: open(path=\"src/a.rs\"); edit(path=\"src/a.rs\", old=\"x\", new=\"q\"); save(path=\"src/a.rs\")", 3),
            // Several acts: the full parser picks one; the early run is dropped if it is not the first.
            ("thought: t\nact: write(name=\"a\", text=\"1\")\nact: write(name=\"a\", text=\"2\")", 1),
            // An error early is an error line, as today.
            ("thought: t\nact: edit(path=\"nope.rs\", old=\"a\", new=\"b\"); reply(text=\"x\")", 1),
        ];
        for (text, early) in cases {
            let (a, disk_a) = today(text);
            let (b, disk_b, ran) = streamed(text);
            assert_eq!(ran, early, "{text}");
            assert_eq!(a.messages, b.messages, "the same log: {text}");
            assert_eq!(a, b, "the same engine: {text}");
            assert_eq!(disk_a, disk_b, "the same files: {text}");
        }
        let (e, disk, _) = streamed(cases[0].0);
        assert!(disk.iter().any(|(p, t)| p == "src/a.rs" && t.contains('y')), "kept: {disk:?}");
        assert!(e.messages.last().unwrap().text.contains("run(command=\"cargo test\") → error"), "the run waited and ran at the end");
    }
}
