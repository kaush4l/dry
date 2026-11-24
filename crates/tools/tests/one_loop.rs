//! "Run one loop completely" (docs/twelve-factor.md, gap 1) as one test:
//! the shipped default (`dry_core::shipped::state()`), a scripted model,
//! the real disk and shell (`dry_tools::live::Disk`) on a temp project.
//!
//! ```text
//! you ─post─▶ translator: skill(prompting) → engine_start(template=developer, root=<tmp>) + reply(brief)
//!                └─▶ dev: triage → code (edit, save, run check: FAILS, fix, PASSES) → test (start a long job, wait="yes")
//!                         the turn ends there with no reply: dev waits on its job, its parent hears nothing
//!                         the job ends ─wake─▶ dev, still in `test` → reply(report), its one reply
//!             translator ◀─wake─ "engine dev replied" → skill(judging) → reply(report to you)
//! ```
//!
//! The driver is the server host's loop (`crates/server/src/host.rs`,
//! `Host::run` and `Host::settle`) without threads: each turn runs to its
//! end through `dry_core::runner::Runner` (a model call is a scripted
//! answer, a waiting call runs on the disk), the children a turn made run
//! next, and when nothing runs every engine's callbacks
//! (`dry_core::turn::callbacks`) are settled and delivered with
//! `State::add_wake`, as the host does. The model is scripted per engine:
//! each answer is checked against the prompt it answers (what it must
//! hold, what it must not), so a step that got a stale or missing context
//! fails here, not in a person's run.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dry_core::calls::Call;
use dry_core::card::Status;
use dry_core::engines::Seen;
use dry_core::inference::{OpenAi, Request, Role};
use dry_core::runner::{Came, Cx, Next, Runner};
use dry_core::state::State;
use dry_tools::live::{Disk, run_waits};

/// One scripted model call: the answer (or the call's failure), and what
/// the prompt it answers must and must not hold.
struct Turn {
    answer: Result<String, String>,
    has: Vec<String>,
    lacks: Vec<String>,
}

fn says(act: &str) -> Turn {
    Turn { answer: Ok(format!("thought: next step\nact: {act}")), has: Vec::new(), lacks: Vec::new() }
}

impl Turn {
    fn has(mut self, text: &str) -> Turn {
        self.has.push(text.into());
        self
    }
    fn lacks(mut self, text: &str) -> Turn {
        self.lacks.push(text.into());
        self
    }
}

/// What one model call was sent, by engine.
struct Sent {
    engine: String,
    prompt: String,
}

/// The scripted model: answers per engine, in order; every prompt kept.
#[derive(Default)]
struct Script {
    turns: BTreeMap<String, VecDeque<Turn>>,
    sent: Vec<Sent>,
}

/// The prompt a request carries, as text: every message, in order.
fn prompt_of(req: &Request) -> String {
    let body: serde_json::Value = serde_json::from_str(&req.body).expect("a request body is JSON");
    let messages = body["messages"].as_array().expect("a chat request has messages");
    messages
        .iter()
        .map(|m| format!("[{}]\n{}", m["role"].as_str().unwrap_or("?"), m["content"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

impl Script {
    fn answer(&mut self, engine: &str, req: &Request) -> Result<String, String> {
        let prompt = prompt_of(req);
        let n = self.sent.iter().filter(|s| s.engine == engine).count() + 1;
        if let Ok(dir) = std::env::var("DRY_ONE_LOOP_PROMPTS") {
            let _ = std::fs::write(Path::new(&dir).join(format!("{engine}-{n}.txt")), &prompt);
        }
        self.sent.push(Sent { engine: engine.into(), prompt: prompt.clone() });
        let turn = self
            .turns
            .get_mut(engine)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| panic!("{engine} call {n}: no scripted answer left (an extra model call)\n{prompt}"));
        for want in &turn.has {
            assert!(prompt.contains(want.as_str()), "{engine} call {n}: the prompt lacks {want:?}\n---\n{prompt}");
        }
        for not in &turn.lacks {
            assert!(!prompt.contains(not.as_str()), "{engine} call {n}: the prompt still holds {not:?}\n---\n{prompt}");
        }
        turn.answer.map(|text| serde_json::json!({ "choices": [{ "message": { "content": text } }] }).to_string())
    }
}

/// The host's loop on one state, one turn at a time.
struct Loop {
    state: State,
    disk: Disk,
    model: Script,
    /// How each engine's last turn ended, by id (the host's `status`).
    status: BTreeMap<String, Status>,
}

impl Loop {
    fn seen(status: &BTreeMap<String, Status>, state: &State, j: usize) -> Seen {
        let Some(e) = state.engines.get(j) else { return Seen::default() };
        let failed = match status.get(&e.id) {
            Some(Status::Failed(why) | Status::Missing(why)) => Some(why.clone()),
            _ => None,
        };
        Seen { label: "idle".into(), busy: !e.waiting.is_empty(), failed }
    }

    /// Engine `i`'s turn to its end (`Host::run`): the children it gave a
    /// message, to run next.
    fn turn(&mut self, i: usize, resume: bool) -> Vec<usize> {
        let id = self.state.engines[i].id.clone();
        self.status.remove(&id);
        let mut r = if resume { Runner::resume(&self.state, i, None) } else { Runner::start(&self.state, i, None) }
            .unwrap_or_else(|s| panic!("{id} cannot run: {s:?}"));
        let none = |_: &Call| None;
        let mut came = Came::Start;
        let mut children = Vec::new();
        loop {
            let status = self.status.clone();
            let seen = move |s: &State, j: usize| Loop::seen(&status, s, j);
            let cx = Cx { host: &self.disk, clock: dry_tools::live::clock(), seen: &seen, external: &none };
            let next = r.next(&mut self.state, &cx, came);
            children.append(&mut r.spawned);
            came = match next {
                Next::Probe { .. } => Came::Window(None),
                Next::Post(req) => Came::Body(self.model.answer(&id, &req)),
                Next::Run(waits) => {
                    let waits: Vec<_> = waits.into_iter().map(|w| (Call { name: String::new(), args: Vec::new() }, w)).collect();
                    Came::Ran(run_waits(&waits, &none, &self.disk))
                }
                Next::Waiting(_) => return children,
                Next::Done(status) => {
                    self.status.insert(id, status);
                    return children;
                }
            };
        }
    }

    /// Every engine's callbacks (`Host::settle`): the turns they start.
    fn settle(&mut self) -> Vec<(usize, bool)> {
        let mut out = Vec::new();
        for i in 0..self.state.engines.len() {
            let seen: Vec<Seen> = (0..self.state.engines.len()).map(|j| Loop::seen(&self.status, &self.state, j)).collect();
            let settled = dry_core::turn::callbacks(&mut self.state, i, false, &self.disk, &|j| seen[j].clone());
            if settled.resumed {
                out.push((i, true));
            } else if !settled.wake.is_empty() && self.state.add_wake(i, &settled.wake.join("\n")) {
                out.push((i, false));
            }
        }
        out
    }

    /// Run until nothing runs and nothing more calls back (a running
    /// terminal session is waited for, up to `within`).
    fn run(&mut self, within: Duration) {
        let start = Instant::now();
        let mut queue: VecDeque<(usize, bool)> = self.settle().into();
        loop {
            while let Some((i, resume)) = queue.pop_front() {
                let children = self.turn(i, resume);
                queue.extend(children.into_iter().map(|c| (c, false)));
                if queue.is_empty() {
                    queue.extend(self.settle());
                }
            }
            queue.extend(self.settle());
            if !queue.is_empty() {
                continue;
            }
            let running =
                self.state.engines.iter().any(|e| e.artifacts.values().any(|a| a.starts_with("kind: terminal") && a.contains(" running ")));
            if !running {
                return;
            }
            assert!(start.elapsed() < within, "a terminal session still runs after {within:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A fresh folder for the test: the project and the session logs.
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dry-one-loop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::create_dir_all(dir.join("engines")).unwrap();
    dir.canonicalize().unwrap()
}

/// Line `n` of an open file as the file artifact's snapshot shows it.
fn shown(n: usize, text: &str) -> String {
    format!("    {n}  {text}")
}

const CHECK: &str = "#!/bin/sh\nwant='Hello, world!'\ngot=$(cat greeting.txt)\nif [ \"$got\" != \"$want\" ]; then echo \"FAIL: greeting.txt says '$got', want '$want'\"; exit 1; fi\necho \"PASS: greeting.txt says '$want'\"\n";

#[test]
fn one_loop_runs_completely_from_your_ask_to_the_heads_report() {
    let dir = scratch();
    let project = dir.join("project");
    std::fs::write(project.join("greeting.txt"), "Helo, world!\n").unwrap();
    std::fs::write(project.join("check.sh"), CHECK).unwrap();
    let root = project.display().to_string();

    let mut state = dry_core::shipped::state().unwrap();
    let ai =
        OpenAi { base_url: "http://scripted.invalid/v1".into(), model: "scripted".into(), context: Some(200_000), ..OpenAi::default() };
    for e in &mut state.engines {
        e.inference = ai.clone();
    }
    let head = state.index_of(dry_core::shipped::HEAD).expect("the shipped default has its translator");
    assert_eq!(state.engines.len(), 1, "the shipped default is one engine");
    assert_eq!(state.engines[head].heard.map(|c| c.n), Some(0), "a fresh head has heard nothing yet, so your first post calls it");

    // ---- the script ----
    let mut model = Script::default();
    let brief = format!(
        "engine_start(name=\"dev\", template=\"developer\", root=\"{root}\", goal=\"Fix the greeting in greeting.txt: it must read Hello, world!\", scope=\"Only greeting.txt. Commit nothing.\", done=\"sh check.sh prints PASS; the report quotes it.\", text=\"The greeting in greeting.txt is misspelt. Fix it and prove it with sh check.sh.\")"
    );
    model.turns.insert(
        "translator".into(),
        VecDeque::from([
            // 1. Your ask came as a guided post: load prompting first, alone.
            says("skill(name=\"prompting\")").has("fix the greeting").has("developer"),
            // 2. The brief, read with prompting loaded: the child, and the brief to you.
            says(&format!("{brief}; reply(text=\"Started dev (developer) on {root}: fix greeting.txt; done when sh check.sh prints PASS.\")"))
                .has("Prompting: from an ask to a brief"),
            // 4. The child's reply woke it: judging first, alone.
            says("skill(name=\"judging\")").has("engine dev replied:").has("PASS: greeting.txt says 'Hello, world!'"),
            // 5. Judged against the whole reply (the children artifact): the report.
            says("reply(text=\"Done: greeting.txt now reads Hello, world! Changed: greeting.txt. Ran: sh check.sh, exit 0, PASS: greeting.txt says 'Hello, world!'. Not verified: nothing else.\")")
                .has("Judging a child's reply")
                .has("last reply: Changed: greeting.txt"),
        ]),
    );
    model.turns.insert(
        "dev".into(),
        VecDeque::from([
            // triage: the phase, the goal and the project's files are there.
            says("skill(name=\"triage\"); open(path=\"greeting.txt\")").has("Now, triage:").has("Fix the greeting in greeting.txt").has("greeting.txt"),
            // The file as it is on disk now.
            says("write(name=\"plan\", text=\"- [ ] greeting.txt: Helo -> Hello. Proof: sh check.sh\"); goto_phase(name=\"code\")")
                .has("Now, triage:")
                .has(&shown(1, "Helo, world!")),
            // code: an edit with a typo, saved, and the quick check.
            says("edit(path=\"greeting.txt\", old=\"Helo, world!\", new=\"Hello, wrld!\"); save(path=\"greeting.txt\"); run(command=\"sh check.sh\")")
                .has("Now, code:")
                .has("left triage; now code"),
            // The failure is in its context, and the file shows what was saved.
            says("edit(path=\"greeting.txt\", old=\"Hello, wrld!\", new=\"Hello, world!\"); save(path=\"greeting.txt\"); run(command=\"sh check.sh\")")
                .has("Now, code:")
                .has("`sh check.sh`: exit 1\nFAIL: greeting.txt says 'Hello, wrld!', want 'Hello, world!'")
                .has(&shown(1, "Hello, wrld!"))
                .lacks(&shown(1, "Helo, world!")),
            says("write(name=\"plan\", text=\"- [x] greeting.txt: Helo -> Hello. Proof: sh check.sh\"); goto_phase(name=\"test\")")
                .has("Now, code:")
                .has("`sh check.sh`: exit 0\nPASS: greeting.txt says 'Hello, world!'")
                .has(&shown(1, "Hello, world!"))
                .lacks(&shown(1, "Hello, wrld!")),
            // test: the long job, started and waited on: the turn ends here, with no reply.
            says("skill(name=\"test\"); start(command=\"sleep 1; sh check.sh && echo full-check-done\", wait=\"yes\")")
                .has("Now, test:")
                .has("left code; now test"),
            // The job ended: the wake finds it still in `test`, with the job's output;
            // the wait's placeholder reads as a plain start now.
            says("reply(text=\"Changed: greeting.txt (Helo -> Hello). Ran: sh check.sh, exit 0, PASS: greeting.txt says 'Hello, world!'; the long check (session 3) exit 0, full-check-done. Saw: greeting.txt reads Hello, world!. Unverified: none.\")")
                .has("Now, test:")
                .has("finished, exit 0")
                .has("full-check-done")
                .has("started; a message comes when it ends")
                .lacks("waiting for session")
                .lacks("Now, triage:"),
        ]),
    );

    let disk = Disk { repo: project.clone(), engines: dir.join("engines") };
    let mut lp = Loop { state, disk, model, status: BTreeMap::new() };

    // ---- 1. you ask the head on its thread ----
    let (thread, post) = dry_core::desk::your_post(
        &lp.state,
        "",
        Some("translator"),
        "Please fix the greeting in my project, it has a typo.",
        dry_tools::live::clock().unix,
    )
    .unwrap();
    dry_core::shared::post(&mut lp.state, &thread, &post).unwrap();
    lp.run(Duration::from_secs(20));

    // ---- what is true at the end ----
    let state = &lp.state;
    let dev = state.index_of("dev").expect("the head made its child");
    // The change is on disk.
    assert_eq!(std::fs::read_to_string(project.join("greeting.txt")).unwrap(), "Hello, world!\n");
    // Every scripted step was taken, and no other model call was made.
    for (engine, left) in &lp.model.turns {
        assert!(left.is_empty(), "{engine}: {} scripted answers were never asked for", left.len());
    }
    let calls = |engine: &str| lp.model.sent.iter().filter(|s| s.engine == engine).collect::<Vec<_>>();
    assert_eq!((calls("translator").len(), calls("dev").len()), (4, 7));
    // The child's run went triage → code → test, in that order, and ended in test.
    let mut phases: Vec<&str> = Vec::new();
    for s in calls("dev") {
        let now = ["triage", "code", "test"]
            .into_iter()
            .find(|p| s.prompt.contains(&format!("Now, {p}:")))
            .expect("every child prompt names its phase");
        if phases.last() != Some(&now) {
            phases.push(now);
        }
    }
    assert_eq!(phases, ["triage", "code", "test"]);
    let d = &state.engines[dev];
    assert_eq!(d.phase_list().unwrap()[d.run.phase].name, "test", "the wake did not reset the child's run");
    // The child is done: it replied once, after its job ended, waits for
    // nothing, runs nothing.
    assert_eq!(dry_core::engines::replies(d).len(), 1);
    let wake_at = d.messages.iter().position(|m| m.role == Role::User && m.text.contains("finished, exit 0")).expect("the job's end woke it");
    let reply_at = d
        .messages
        .iter()
        .position(|m| m.role == Role::Assistant && dry_core::turn::reply_in(d, &m.text).is_some())
        .expect("it replied");
    assert!(reply_at > wake_at, "the child replied before its job ended");
    assert!(d.waiting.is_empty());
    assert_eq!(lp.status.get("dev"), Some(&Status::Ready));
    assert!(!d.artifacts.values().any(|a| a.starts_with("kind: terminal") && a.contains(" running ")), "no session still runs");
    // The head's last word is the report, with the evidence.
    let h = &state.engines[head];
    let last = h.messages.last().unwrap();
    assert_eq!(last.role, Role::Assistant);
    let report = dry_core::turn::reply_in(h, &last.text).expect("the head's last answer replies");
    assert!(report.contains("PASS: greeting.txt says 'Hello, world!'"), "{report}");
    assert_eq!(lp.status.get("translator"), Some(&Status::Ready));
    assert_eq!(dry_core::engines::replies(h).len(), 2, "the brief, then the report");
    // Each callback was delivered once: your post, the child's reply, the job's end.
    let woke =
        |e: &dry_core::state::Engine, what: &str| e.messages.iter().filter(|m| m.role == Role::User && m.text.contains(what)).count();
    assert_eq!(woke(h, "[thread 1]"), 1, "your post reached the head once");
    assert_eq!(woke(h, "engine dev replied"), 1, "the child's reply woke the head once");
    assert_eq!(woke(d, "finished, exit 0"), 1, "the job's end woke the child once");
    let posts = dry_core::shared::parse(state.shared.get(&thread).map_or("", String::as_str)).len();
    assert_eq!(posts, 1, "nothing else was posted on the thread");
    for e in &state.engines {
        assert_eq!(e.heard.map(|c| c.n), Some(posts), "{} heard the whole thread, once", e.id);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two loops side by side, and a head that goes on by itself while its
/// plan is open (docs/night/journal.md):
///
/// ```text
/// you ─post─▶ translator: write(plan: two `- [ ]` lines) + engine_start(dev-a) + engine_start(dev-b) + reply(the briefs), one act
///                ├─▶ dev-a (root a/, made for it): writes and runs a.txt → reply
///                └─▶ dev-b (root b/, made for it): writes and runs b.txt → reply
///             translator ◀─ one wake with both replies → ticks a, leaves b open, replies
///             ... nobody works, the plan holds an open step, three minutes pass ...
///             translator ◀─ "Alert keep-going fired (idle 3m)" → ticks b, replies with the summary
///             ... quiet again, no open step: the alert stays silent
/// ```
#[test]
fn two_loops_run_side_by_side_and_the_head_goes_on_while_its_plan_is_open() {
    let dir = std::env::temp_dir().join(format!("dry-two-loops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::create_dir_all(dir.join("engines")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let project = dir.join("project");

    let mut state = dry_core::shipped::state().unwrap();
    let ai =
        OpenAi { base_url: "http://scripted.invalid/v1".into(), model: "scripted".into(), context: Some(200_000), ..OpenAi::default() };
    for e in &mut state.engines {
        e.inference = ai.clone();
    }
    let head = state.index_of(dry_core::shipped::HEAD).unwrap();
    assert!(state.engines[head].alerts.contains_key("keep-going"), "the shipped translator holds the keep-going alert");

    let start = |name: &str| {
        format!(
            "engine_start(name=\"dev-{name}\", template=\"developer\", root=\"{name}\", goal=\"Make {name}.txt holding the word {name}.\", scope=\"Only {name}.txt.\", done=\"cat {name}.txt prints {name}.\", text=\"Write {name}.txt and show it with cat.\")"
        )
    };
    let child = |name: &str, other: &str| {
        VecDeque::from([
            // Its own brief and its own root; nothing of its sibling's.
            says(&format!("goto_phase(name=\"code\"); edit(path=\"{name}.txt\", new=\"{name}\"); save(path=\"{name}.txt\"); run(command=\"cat {name}.txt\")"))
                .has(&format!("Make {name}.txt holding the word {name}."))
                .has(&format!("root: {name}"))
                .lacks(&format!("Make {other}.txt"))
                .lacks(&format!("root: {other}")),
            says(&format!("reply(text=\"Changed: {name}.txt. Ran: cat {name}.txt, exit 0, {name}. Saw: the file holds {name}. Unverified: nothing.\")"))
                .has(&format!("`cat {name}.txt`: exit 0\n{name}"))
                // The thread is shared (your post names both files); its sibling's work is not here.
                .lacks(&format!("cat {other}.txt"))
                .lacks(&format!("Make {other}.txt")),
        ])
    };
    let mut model = Script::default();
    model.turns.insert(
        "translator".into(),
        VecDeque::from([
            // 1. The plan, both children and the briefs to you, in one act.
            says(&format!(
                "write(name=\"plan\", text=\"## Plan\\n- [ ] a.txt: dev-a\\n- [ ] b.txt: dev-b\"); {}; {}; reply(text=\"Plan written; started dev-a and dev-b together.\")",
                start("a"),
                start("b")
            ))
            .has("two files")
            .has("keep-going (idle 3m, while `plan` has an open `- [ ]` step)"),
            // 2. One wake carries both replies; the plan shows both lines open.
            says("write(name=\"plan\", text=\"## Plan\\n- [x] a.txt: dev-a\\n- [ ] b.txt: dev-b\"); reply(text=\"a is done; b is next.\")")
                .has("engine dev-a replied:")
                .has("engine dev-b replied:")
                .has("- [ ] a.txt: dev-a"),
            // 3. Nobody wrote: its own alert woke it, because a step is open.
            says("write(name=\"plan\", text=\"## Plan\\n- [x] a.txt: dev-a\\n- [x] b.txt: dev-b\"); write(name=\"summary\", text=\"Asked: two files. Work: dev-a made a.txt, dev-b made b.txt.\"); reply(text=\"Asked: two files. Work: dev-a made a.txt, dev-b made b.txt.\")")
                .has("Alert keep-going fired (idle 3m): Your plan has an open `- [ ]` step")
                .has("- [ ] b.txt: dev-b")
                .has("last reply: Changed: b.txt"),
        ]),
    );
    model.turns.insert("dev-a".into(), child("a", "b"));
    model.turns.insert("dev-b".into(), child("b", "a"));

    let disk = Disk { repo: project.clone(), engines: dir.join("engines") };
    let mut lp = Loop { state, disk, model, status: BTreeMap::new() };
    let (thread, post) =
        dry_core::desk::your_post(&lp.state, "", Some("translator"), "Make two files, a.txt and b.txt; they do not need each other.", dry_tools::live::clock().unix)
            .unwrap();
    dry_core::shared::post(&mut lp.state, &thread, &post).unwrap();
    lp.run(Duration::from_secs(20));

    // Both children ran, each in the root made for it, and the head heard both at once.
    assert_eq!(std::fs::read_to_string(project.join("a/a.txt")).unwrap(), "a");
    assert_eq!(std::fs::read_to_string(project.join("b/b.txt")).unwrap(), "b");
    let calls = |lp: &Loop, engine: &str| lp.model.sent.iter().filter(|s| s.engine == engine).count();
    assert_eq!((calls(&lp, "translator"), calls(&lp, "dev-a"), calls(&lp, "dev-b")), (2, 2, 2));
    // Quiet, with an open step: the alert has been looked at and not fired.
    let mark = lp.state.engines[head].alert_marks["keep-going"];
    assert!(mark.fired == 0 && mark.quiet.is_some(), "{mark:?}");

    // Three minutes of quiet later (the mark moved back, not the clock).
    let back = |lp: &mut Loop, secs: i64| {
        let m = lp.state.engines[head].alert_marks.get_mut("keep-going").unwrap();
        m.created -= secs;
        m.quiet = m.quiet.map(|q| q - secs);
        m.last = m.last.map(|l| l - secs);
    };
    back(&mut lp, 181);
    lp.run(Duration::from_secs(20));
    assert_eq!(calls(&lp, "translator"), 3, "the alert started the head's third turn");
    assert_eq!(lp.state.engines[head].alert_marks["keep-going"].fired, 1);
    assert!(lp.state.engines[head].artifacts["summary"].starts_with("Asked: two files."));

    // Quiet again, every step ticked: the alert stays silent however long.
    back(&mut lp, 3_600);
    lp.run(Duration::from_secs(20));
    assert_eq!(calls(&lp, "translator"), 3, "no open step, no wake (an extra model call would have panicked)");
    for (engine, left) in &lp.model.turns {
        assert!(left.is_empty(), "{engine}: {} scripted answers were never asked for", left.len());
    }
    // The run's marks are part of what is saved.
    let saved = dry_core::store::to_texts(&lp.state);
    assert!(saved["engines/translator/convo/run.md"].contains("alert: keep-going "), "{}", saved["engines/translator/convo/run.md"]);
    let _ = std::fs::remove_dir_all(&dir);
}
