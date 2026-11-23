//! The logic check for the engine standard's "One step, many calls" (docs/engine-standard.md):
//! every call of an act is invoked at once, only the waiting happens in
//! parallel, the results come back in written order as one message, and
//! long work calls back instead of waiting.
//!
//! The fakes have controllable delays and record when each call started
//! and finished: a host whose `run(command="sleep <ms>")` sleeps that long,
//! and an outside tool `slow.wait(ms=..., say=...)` (an MCP call's place).
//! The steps run the way the window runs them: `turn::begin_step`, then
//! `dry_tools::live::run_waits` (one thread per waiting call), then
//! `turn::finish_step`. Timings print with `--nocapture`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use dry_core::calls::Call;
use dry_core::engines::{self, Seen};
use dry_core::inference::{Message, OpenAi, Role};
use dry_core::live::{Content, Host, Job, Proc};
use dry_core::response::{Format, Responses};
use dry_core::state::{Engine, State};
use dry_core::turn::{self, Came, Step};
use dry_tools::live::run_waits;

/// What a fake call did: its name, when it started and finished.
#[derive(Clone, Debug)]
struct Span {
    what: String,
    start: Instant,
    end: Instant,
}

/// Files in memory and commands that sleep, each recorded.
#[derive(Default)]
struct Fake {
    files: Mutex<BTreeMap<String, String>>,
    /// When each file was last written.
    written: Mutex<BTreeMap<String, Instant>>,
    spans: Mutex<Vec<Span>>,
    /// What `a.rs` said when each run started.
    seen_by_run: Mutex<Vec<String>>,
    /// Sessions (by number) that ended, for `status`.
    ended: Mutex<BTreeSet<u32>>,
    logs: Mutex<BTreeMap<u32, String>>,
    t0: Option<Instant>,
}

impl Fake {
    fn new() -> Fake {
        Fake { t0: Some(Instant::now()), ..Fake::default() }
    }

    fn with(files: &[(&str, &str)]) -> Fake {
        let f = Fake::new();
        f.files.lock().unwrap().extend(files.iter().map(|(p, t)| (p.to_string(), t.to_string())));
        f
    }

    fn spans(&self) -> Vec<Span> {
        self.spans.lock().unwrap().clone()
    }

    fn ms(&self, at: Instant) -> u128 {
        at.duration_since(self.t0.unwrap()).as_millis()
    }

    /// Sleep `ms`, recorded as `what`.
    fn sleep(&self, what: &str, ms: u64) {
        let start = Instant::now();
        std::thread::sleep(Duration::from_millis(ms));
        self.spans.lock().unwrap().push(Span { what: what.into(), start, end: Instant::now() });
    }

    /// The outside tool: `slow.wait(ms, say)`; anything else is unknown.
    fn outside(&self, call: &Call) -> Option<Result<String, String>> {
        (call.name == "slow.wait").then(|| {
            let ms = call.arg("ms").and_then(|m| m.parse().ok()).unwrap_or(0);
            let say = call.arg("say").unwrap_or("done").to_string();
            self.sleep(&format!("slow.wait {say}"), ms);
            Ok(say)
        })
    }
}

impl Host for Fake {
    fn now(&self) -> i64 {
        0
    }
    fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
        Ok(self.files.lock().unwrap().keys().cloned().collect())
    }
    fn read(&self, _: &str, path: &str) -> Content {
        self.files.lock().unwrap().get(path).map_or_else(|| Content::Missing(format!("no file {path}")), |t| Content::Text(t.clone()))
    }
    fn write(&self, _: &str, path: &str, text: &str) -> Result<(), String> {
        self.files.lock().unwrap().insert(path.into(), text.into());
        self.written.lock().unwrap().insert(path.into(), Instant::now());
        Ok(())
    }
    fn run(&self, job: &Job) -> Result<Option<i32>, String> {
        let a = self.files.lock().unwrap().get("a.rs").cloned().unwrap_or_default();
        self.seen_by_run.lock().unwrap().push(a);
        let ms = job.command.strip_prefix("sleep ").and_then(|m| m.trim().parse().ok()).unwrap_or(0);
        self.sleep(&format!("run {}", job.command), ms);
        self.logs.lock().unwrap().insert(job.session, format!("slept {ms}\n"));
        Ok(Some(0))
    }
    fn spawn(&self, _: &Job) -> Result<(), String> {
        Ok(())
    }
    fn status(&self, job: &Job) -> Proc {
        if self.ended.lock().unwrap().contains(&job.session) { Proc::Ended(Some(0)) } else { Proc::Running }
    }
    fn tail(&self, job: &Job, _: usize) -> String {
        self.logs.lock().unwrap().get(&job.session).cloned().unwrap_or_default()
    }
}

fn engine() -> Engine {
    let mut e = Engine::new("E", "", OpenAi::default());
    e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
    e.format = Format::default();
    e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
    e.artifacts.insert("code".into(), "kind: file\nroot: .".into());
    e.messages.push(Message { role: Role::User, text: "go".into() });
    e
}

fn answer(act: &str) -> String {
    format!("thought: x\nact: {act}")
}

/// One step the way the window runs it: instant calls, then every
/// waiting call at once, then the merge. Its time, from the waits' launch.
fn shell_step(e: &mut Engine, act: &str, fake: &Fake, family: Option<&mut engines::Family>) -> (Step, Duration) {
    let pending = turn::begin_step(e, &answer(act), &mut |c| fake.outside(c), fake, family);
    let at = Instant::now();
    let came = run_waits(&pending.waits, &|c: &Call| fake.outside(c), fake);
    let took = at.elapsed();
    (turn::finish_step(e, pending, came), took)
}

fn last(e: &Engine) -> String {
    e.messages.last().map(|m| m.text.clone()).unwrap_or_default()
}

#[test]
fn every_waiting_call_starts_before_any_finishes() {
    let fake = Fake::new();
    let mut e = engine();
    let (step, took) =
        shell_step(&mut e, "run(command=\"sleep 100\"); slow.wait(ms=100, say=\"b\"); run(command=\"sleep 100\")", &fake, None);
    assert_eq!(step, Step::Again);
    let spans = fake.spans();
    assert_eq!(spans.len(), 3);
    let last_start = spans.iter().map(|s| s.start).max().unwrap();
    let first_end = spans.iter().map(|s| s.end).min().unwrap();
    assert!(last_start < first_end, "all three started before any finished: {spans:?}");
    println!(
        "three 100 ms waits: step took {} ms (sequential would be 300); starts at {:?} ms, ends at {:?} ms",
        took.as_millis(),
        spans.iter().map(|s| fake.ms(s.start)).collect::<Vec<_>>(),
        spans.iter().map(|s| fake.ms(s.end)).collect::<Vec<_>>()
    );
    assert!(took < Duration::from_millis(200), "about the slowest call, not the sum: {took:?}");
    assert!(took >= Duration::from_millis(100));
}

#[test]
fn results_come_back_in_written_order_whatever_order_they_finish_in() {
    let fake = Fake::new();
    let mut e = engine();
    let act =
        "slow.wait(ms=150, say=\"first\"); write(name=\"note\", text=\"hi\"); run(command=\"sleep 10\"); slow.wait(ms=80, say=\"third\")";
    let (_, took) = shell_step(&mut e, act, &fake, None);
    let finished: Vec<String> = {
        let mut s = fake.spans();
        s.sort_by_key(|s| s.end);
        s.into_iter().map(|s| s.what).collect()
    };
    assert_eq!(finished, ["run sleep 10", "slow.wait third", "slow.wait first"], "they finished in another order");
    let lines: Vec<String> = last(&e).lines().filter(|l| l.contains(" → ")).map(str::to_string).collect();
    assert_eq!(lines.len(), 4, "{}", last(&e));
    assert!(lines[0].starts_with("slow.wait(ms=\"150\", say=\"first\") → first"), "{lines:?}");
    assert!(lines[1].starts_with("write(name=\"note\", text=\"hi\") → saved note"), "{lines:?}");
    assert!(lines[2].starts_with("run(command=\"sleep 10\") → session 1 `sleep 10`: exit 0"), "{lines:?}");
    assert!(lines[3].starts_with("slow.wait(ms=\"80\", say=\"third\") → third"), "{lines:?}");
    assert_eq!(e.messages.iter().filter(|m| m.role == Role::Tool).count(), 1, "one tool message");
    println!("written order kept; finish order was {finished:?}; step took {} ms", took.as_millis());
}

#[test]
fn instant_calls_apply_in_order_before_the_waiting_ones_launch() {
    let fake = Fake::with(&[("a.rs", "let x = 1;\n")]);
    let mut e = engine();
    // edit then save then run: the run tests the saved file.
    shell_step(
        &mut e,
        "open(path=\"a.rs\"); edit(path=\"a.rs\", old=\"1\", new=\"2\"); save(path=\"a.rs\"); run(command=\"sleep 20\")",
        &fake,
        None,
    );
    assert_eq!(fake.seen_by_run.lock().unwrap().as_slice(), ["let x = 2;\n"], "the run saw the saved edit");
    let saved = fake.written.lock().unwrap()["a.rs"];
    assert!(saved < fake.spans()[0].start, "saved before the run started");
    let text = last(&e);
    assert!(
        text.contains("→ opened a.rs") && text.contains("→ edited a.rs") && text.contains("→ saved a.rs") && text.contains("→ session 1"),
        "{text}"
    );

    // The rule, not the written place: a run written first still waits
    // for the act's instant calls; to test the file as it was, run it in
    // an earlier step.
    shell_step(&mut e, "run(command=\"sleep 5\"); edit(path=\"a.rs\", old=\"2\", new=\"3\"); save(path=\"a.rs\")", &fake, None);
    assert_eq!(fake.seen_by_run.lock().unwrap()[1], "let x = 3;\n");
    let text = last(&e);
    let lines: Vec<&str> = text.lines().filter(|l| l.contains(" → ")).collect();
    assert!(lines[0].starts_with("run(") && lines[2].starts_with("save("), "still written order: {lines:?}");
}

/// A parent with an engines artifact and a terminal, at index 0.
fn family_state() -> State {
    let mut s = State::default();
    let p = s.add_engine();
    let e = &mut s.engines[p];
    e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
    e.format = Format::default();
    e.artifacts.insert("kids".into(), "kind: engines".into());
    e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
    s
}

fn idle(_: usize) -> Seen {
    Seen { label: "idle".into(), ..Seen::default() }
}

#[test]
fn two_engine_calls_start_both_children_at_once_and_the_parent_waits_for_both() {
    let fake = Fake::new();
    let mut s = family_state();
    s.add_text(0, "explore two ways");
    let act = "engine_call(name=\"a\", goal=\"Way A.\"); engine_call(name=\"b\", goal=\"Way B.\"); run(command=\"sleep 50\")";
    let mut fam = engines::family(&s, 0, &idle);
    let pending = turn::begin_step(&mut s.engines[0], &answer(act), &mut |c| fake.outside(c), &fake, Some(&mut fam));
    // Both children are asked by the instant half, before any wait.
    let applied = engines::apply(&mut s, 0, fam.effects);
    assert_eq!(applied.run, [1, 2], "both start now");
    assert!(fake.spans().is_empty(), "nothing has waited yet");

    // The children's turns run at the same time (a fake model of 100 ms
    // each), while the parent's run waits.
    let at = Instant::now();
    let kids: Vec<Engine> = applied.run.iter().map(|&c| s.engines[c].clone()).collect();
    let (came, kids) = std::thread::scope(|scope| {
        let turns: Vec<_> = kids
            .into_iter()
            .map(|mut k| {
                let fake = &fake;
                scope.spawn(move || {
                    fake.sleep(&format!("model {}", k.id), 100);
                    let reply = format!("reply(text=\"{} found it.\")", k.id);
                    turn::step(&mut k, &answer(&reply), &mut |_| None);
                    k
                })
            })
            .collect();
        let came = run_waits(&pending.waits, &|c: &Call| fake.outside(c), &fake);
        (came, turns.into_iter().map(|t| t.join().unwrap()).collect::<Vec<_>>())
    });
    let took = at.elapsed();
    let models: Vec<Span> = fake.spans().into_iter().filter(|s| s.what.starts_with("model")).collect();
    assert!(models.iter().map(|s| s.start).max() < models.iter().map(|s| s.end).min(), "the children ran at once: {models:?}");
    assert!(took < Duration::from_millis(190), "two 100 ms children and a 50 ms run took {took:?}");
    println!("two children (100 ms each) and a 50 ms run: {} ms", took.as_millis());

    let step = turn::finish_step(&mut s.engines[0], pending, came);
    assert_eq!(step, Step::Wait(vec!["a".into(), "b".into()]));
    assert!(last(&s.engines[0]).contains("→ waiting for a's reply\n") && last(&s.engines[0]).contains("→ session 1 `sleep 50`: exit 0"));

    // a replied, b is still working: the parent still waits.
    s.engines[1] = kids[0].clone();
    let busy_b = |j: usize| Seen { busy: j == 2, ..idle(j) };
    let settled = turn::callbacks(&mut s, 0, false, &fake, &busy_b);
    assert!(!settled.resumed && settled.wake.is_empty());
    assert!(last(&s.engines[0]).contains("a replied: \"a found it.\"") && last(&s.engines[0]).contains("waiting for b's reply"));
    // b replied too: the parent's turn goes on, both replies in its one message, in written order.
    s.engines[2] = kids[1].clone();
    let settled = turn::callbacks(&mut s, 0, false, &fake, &idle);
    assert!(settled.resumed);
    let text = last(&s.engines[0]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].ends_with("→ a replied: \"a found it.\"") && lines[1].ends_with("→ b replied: \"b found it.\""), "{text}");
    assert!(lines[2].starts_with("run(command=\"sleep 50\") → session 1"), "{text}");
}

#[test]
fn a_failed_child_is_an_error_line_not_a_stuck_parent() {
    let fake = Fake::new();
    let mut s = family_state();
    s.add_text(0, "explore");
    let mut fam = engines::family(&s, 0, &idle);
    let act = "engine_call(name=\"a\", goal=\"A.\"); engine_call(name=\"b\", goal=\"B.\")";
    let pending = turn::begin_step(&mut s.engines[0], &answer(act), &mut |_| None, &fake, Some(&mut fam));
    engines::apply(&mut s, 0, fam.effects);
    assert_eq!(turn::finish_step(&mut s.engines[0], pending, Vec::new()), Step::Wait(vec!["a".into(), "b".into()]));
    turn::step(&mut s.engines[1], &answer("reply(text=\"A done.\")"), &mut |_| None);
    let b_failed = |j: usize| Seen { failed: (j == 2).then(|| "no model".to_string()), ..idle(j) };
    assert!(turn::callbacks(&mut s, 0, false, &fake, &b_failed).resumed);
    let text = last(&s.engines[0]);
    assert!(text.contains("→ a replied: \"A done.\"\n") && text.ends_with("→ error: b failed: no model"), "{text}");
}

#[test]
fn long_work_returns_at_once_and_calls_back_queued_while_busy() {
    let fake = Fake::new();
    let mut s = family_state();
    s.add_text(0, "build and check docs");
    let mut fam = engines::family(&s, 0, &idle);
    let at = Instant::now();
    let pending = turn::begin_step(
        &mut s.engines[0],
        &answer("start(command=\"sleep 5000\"); engine_start(name=\"docs\", goal=\"Check the docs.\")"),
        &mut |_| None,
        &fake,
        Some(&mut fam),
    );
    assert!(pending.waits.is_empty(), "nothing to wait for");
    let applied = engines::apply(&mut s, 0, fam.effects);
    assert_eq!(turn::finish_step(&mut s.engines[0], pending, Vec::new()), Step::Again);
    assert!(at.elapsed() < Duration::from_millis(50), "returned at once: {:?}", at.elapsed());
    let text = last(&s.engines[0]);
    assert!(
        text.contains("session 1 `sleep 5000` started; a message comes when it ends")
            && text.contains("made docs with your goal and started it"),
        "{text}"
    );
    turn::step(&mut s.engines[0], &answer("reply(text=\"Both are running.\")"), &mut |_| None);

    // Both complete while the parent is in a turn: nothing is taken.
    fake.ended.lock().unwrap().insert(1);
    turn::step(&mut s.engines[applied.run[0]], &answer("reply(text=\"Two broken links.\")"), &mut |_| None);
    assert_eq!(turn::callbacks(&mut s, 0, true, &fake, &idle), engines::Settled::default(), "busy: queued");
    // Free: both come together, as one message for one new turn.
    let settled = turn::callbacks(&mut s, 0, false, &fake, &idle);
    assert_eq!(
        settled.wake,
        ["engine docs replied: \"Two broken links.\" (engines kids)", "session 1 `sleep 5000` finished, exit 0 (terminal term)"]
    );
    assert!(s.add_text(0, &settled.wake.join("\n")));
    let msg = &s.engines[0].messages.last().unwrap();
    assert_eq!(msg.role, Role::User);
    assert!(msg.text.contains("docs replied") && msg.text.contains("session 1"), "the owning engine is called with the results attached");
    assert_eq!(
        turn::step(&mut s.engines[0], &answer("reply(text=\"Docs: two broken links; build done.\")"), &mut |_| None),
        Step::Reply("Docs: two broken links; build done.".into())
    );
    assert_eq!(turn::callbacks(&mut s, 0, false, &fake, &idle), engines::Settled::default(), "each callback comes once");
}

#[test]
fn a_single_call_behaves_as_before() {
    for act in [
        "run(command=\"sleep 5\")",
        "write(name=\"plan\", text=\"1. tea\")",
        "slow.wait(ms=5, say=\"ok\")",
        "fly(to=\"moon\")",
        "reply(text=\"Hi.\")",
    ] {
        let (fa, fb) = (Fake::new(), Fake::new());
        let (mut a, mut b) = (engine(), engine());
        let old = turn::step_with(&mut a, &answer(act), &mut |c| fa.outside(c), &fa);
        let (new, _) = shell_step(&mut b, act, &fb, None);
        assert_eq!(old, new, "{act}");
        assert_eq!(a.messages, b.messages, "{act}");
        assert_eq!(a.artifacts, b.artifacts, "{act}");
    }
}

#[test]
fn a_waiting_call_that_does_not_prepare_is_an_error_at_once() {
    let fake = Fake::new();
    let mut e = engine();
    let pending = turn::begin_step(&mut e, &answer("run(command=\"\"); slow.wait(ms=1)"), &mut |_| None, &fake, None);
    assert_eq!(pending.waiting_names(), ["slow.wait"]);
    let step = turn::finish_step(&mut e, pending, vec![Came::Outside(Some(Ok("done".into())))]);
    assert_eq!(step, Step::Again);
    assert!(last(&e).starts_with("run(command=\"\") → error: `run` needs command"), "{}", last(&e));
}

#[test]
fn the_activity_counts_the_calls_of_one_step() {
    let e = {
        let mut e = engine();
        e.artifacts.insert("kids".into(), "kind: engines".into());
        e
    };
    let text = answer(
        "open(path=\"a\"); open(path=\"b\"); engine_call(name=\"x\", goal=\"g\"); open(path=\"c\"); engine_call(name=\"y\", goal=\"g\")",
    );
    let a = dry_core::activity::Activity::answered(&e, &text, 1);
    assert_eq!(a.label(), "running tools (5): open ×3, engine_call ×2");
    assert_eq!(a.short(), "tools: open ×3, engine_call ×2");
}
