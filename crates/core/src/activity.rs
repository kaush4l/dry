//! Activity: what an engine is doing right now, as one value the UI
//! shows by a label and a background tint.
//!
//! ```text
//! idle ─▶ llm (step 1) ─▶ tools: read, edit ─▶ llm again (step 2) ─▶ … ─▶ idle
//!            │                                    ▲
//!            └─ compressing (at 75% of context) ──┘
//! ```
//!
//! A model engine's turn moves through these as the shell runs it: before
//! the HTTP call it is `Llm`, while its calls run `Tools` (their names),
//! then `Llm` again, until the reply. `Compressing` is the summary call
//! (`crate::context`). Between turns an engine can be `Waiting` on a
//! terminal session that calls back (`crate::terminal`), and a session
//! engine (claude, codex) shows what its program reports, else `Working`.
//!
//! Each activity has a label and a colour token (`activity_llm`, ...): a
//! translucent tint the theme sets, so every theme decides how it looks.
//! It extends `card::Status`, never replaces it: `status()` gives the
//! status every tone, tile and scene mood already reads.

use crate::card::Status;
use crate::state::Engine;
use crate::turn::{self, Step};

/// What an engine is doing now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Activity {
    /// Nothing running.
    #[default]
    Idle,
    /// Waiting for the model's answer to step `step` (1 = the first call
    /// of the turn; later steps are "again").
    Llm { step: usize },
    /// Running the calls of step `step`'s answer, by name.
    Tools { step: usize, calls: Vec<String> },
    /// Summarising the older history before step `step`'s call.
    Compressing { step: usize },
    /// Between turns, a terminal session will call back when it ends
    /// (`session 2 \`cargo test\``).
    Waiting(Vec<String>),
    /// A session's program, or another job (starting MCP servers): what it
    /// reports, else `working`.
    Working(String),
    /// It cannot go on without you: what is missing.
    NeedsYou(String),
    /// The last thing it did went wrong: why.
    Failed(String),
}

/// Every activity colour token, in `crate::theme::COLOR_NAMES` order.
pub const TOKENS: [&str; 7] =
    ["activity_llm", "activity_tools", "activity_compress", "activity_wait", "activity_work", "activity_need", "activity_fail"];

impl Activity {
    /// The colour token of its tint; `None` when idle (no tint).
    pub fn token(&self) -> Option<&'static str> {
        Some(match self {
            Activity::Idle => return None,
            Activity::Llm { .. } => TOKENS[0],
            Activity::Tools { .. } => TOKENS[1],
            Activity::Compressing { .. } => TOKENS[2],
            Activity::Waiting(_) => TOKENS[3],
            Activity::Working(_) => TOKENS[4],
            Activity::NeedsYou(_) => TOKENS[5],
            Activity::Failed(_) => TOKENS[6],
        })
    }

    /// One short line for people.
    pub fn label(&self) -> String {
        match self {
            Activity::Idle => "idle".into(),
            Activity::Llm { step: 0 | 1 } => "calling the LLM".into(),
            Activity::Llm { step } => format!("calling the LLM again (step {step})"),
            Activity::Tools { calls, .. } if calls.is_empty() => "reading the answer".into(),
            Activity::Tools { calls, .. } if calls.len() == 1 => format!("running tools: {}", calls[0]),
            Activity::Tools { calls, .. } => format!("running tools ({}): {}", calls.len(), grouped(calls)),
            Activity::Compressing { .. } => "compressing the history".into(),
            Activity::Waiting(on) => format!("waiting for {}", on.join(", ")),
            Activity::Working(what) => what.clone(),
            Activity::NeedsYou(what) => format!("needs you: {what}"),
            Activity::Failed(why) => format!("failed: {why}"),
        }
    }

    /// A few words for a narrow place (a nav entry): `LLM`, `tools: read`.
    pub fn short(&self) -> String {
        match self {
            Activity::Idle => "idle".into(),
            Activity::Llm { step: 0 | 1 } => "LLM".into(),
            Activity::Llm { .. } => "LLM again".into(),
            Activity::Tools { calls, .. } if calls.is_empty() => "reading".into(),
            Activity::Tools { calls, .. } => format!("tools: {}", grouped(calls)),
            Activity::Compressing { .. } => "compressing".into(),
            Activity::Waiting(_) => "waiting".into(),
            Activity::Working(_) => "working".into(),
            Activity::NeedsYou(_) => "needs you".into(),
            Activity::Failed(_) => "failed".into(),
        }
    }

    /// Is a turn running (the shell must not start another, nor wake it)?
    pub fn is_turn(&self) -> bool {
        matches!(self, Activity::Llm { .. } | Activity::Tools { .. } | Activity::Compressing { .. } | Activity::Working(_))
    }

    /// As a `card::Status`, for tones, tiles and the scene's mood.
    pub fn status(&self) -> Status {
        match self {
            Activity::Idle => Status::Idle,
            Activity::NeedsYou(what) => Status::Missing(what.clone()),
            Activity::Failed(why) => Status::Failed(why.clone()),
            busy => Status::Working(busy.label()),
        }
    }

    /// From a status someone reported: a failure, something missing or a
    /// job running; anything else is idle.
    pub fn of_status(status: &Status) -> Activity {
        match status {
            Status::Working(what) => Activity::Working(what.clone()),
            Status::Failed(why) => Activity::Failed(why.clone()),
            Status::Missing(what) => Activity::NeedsYou(what.clone()),
            Status::Idle | Status::Ready | Status::Done(_) => Activity::Idle,
        }
    }

    /// What step `step` does once the model answered `text`: run its calls
    /// (by name; `reply` is no tool work, so it is left out).
    pub fn answered(e: &Engine, text: &str, step: usize) -> Activity {
        Activity::Tools { step, calls: turn::call_names(e, text).into_iter().filter(|c| c != "reply").collect() }
    }

    /// After step `step` ran: the model again, or idle on a reply.
    pub fn after(done: &Step, step: usize) -> Activity {
        match done {
            Step::Again => Activity::Llm { step: step + 1 },
            Step::Wait(on) => Activity::Waiting(on.iter().map(|c| crate::engines::wait_label(c)).collect()),
            Step::Reply(_) => Activity::Idle,
        }
    }

    /// A session engine's turn: its program's report of what it is doing
    /// (`meta` key `activity`, when a program writes one), else `working`.
    pub fn session(program: &str, reported: Option<&str>) -> Activity {
        match reported.map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => Activity::Working(format!("{program}: {r}")),
            None => Activity::Working(format!("{program}: working")),
        }
    }
}

/// Call names with repeats counted, in first-seen order:
/// `open ×3, engine_call ×2, run`. One step's calls all run at once.
pub fn grouped(calls: &[String]) -> String {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for c in calls {
        match seen.iter_mut().find(|(n, _)| n == c) {
            Some((_, k)) => *k += 1,
            None => seen.push((c, 1)),
        }
    }
    seen.iter().map(|(n, k)| if *k > 1 { format!("{n} ×{k}") } else { n.to_string() }).collect::<Vec<_>>().join(", ")
}

/// What engine `e` waits on: the child engines its turn waits for
/// (`engine_call`), then every terminal `start` session still running,
/// which calls back when it ends (or ends the wait of a
/// `start(..., wait="yes")`).
pub fn waiting(e: &Engine) -> Vec<String> {
    // A job waited on is named once, below, with its command.
    let mut out: Vec<String> =
        e.waiting.iter().filter(|w| crate::terminal::parse_wait(w).is_none()).map(|c| crate::engines::wait_label(c)).collect();
    for text in e.artifacts.values() {
        if crate::live::Live::of(text) != Some(crate::live::Live::Terminal) {
            continue;
        }
        let Ok(state) = crate::terminal::State::parse(text) else { continue };
        for s in &state.sessions {
            if s.mode == crate::terminal::Mode::Start && s.status == crate::terminal::Status::Running {
                let command: String = s.command.chars().take(40).collect();
                out.push(format!("session {} `{command}`", s.id));
            }
        }
    }
    out
}

/// Engine `e`'s activity now: what its turn is doing (`now`), else what
/// was reported about it (a failure, a job), else a callback it waits on,
/// else idle.
pub fn live(e: &Engine, reported: Option<&Status>, now: Option<&Activity>) -> Activity {
    if let Some(a) = now.filter(|a| **a != Activity::Idle) {
        return a.clone();
    }
    match reported.map(Activity::of_status) {
        Some(a) if a != Activity::Idle => a,
        _ => {
            let on = waiting(e);
            if on.is_empty() { Activity::Idle } else { Activity::Waiting(on) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{Message, OpenAi, Role};
    use crate::response::{Format, Responses};
    use crate::theme::{COLOR_NAMES, Mode, Theme, resolve};

    fn engine() -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.format = Format::default();
        e.messages.push(Message { role: Role::User, text: "save a plan, then tell me".into() });
        e
    }

    /// A turn with a fake model, driven the way the shell drives it: the
    /// activity before each call, after each answer, after each step.
    #[test]
    fn a_turn_moves_llm_tools_llm_then_idle() {
        let mut e = engine();
        let answers = ["thought: save\nact: write(name=\"plan\", text=\"1. tea\"); list()", "thought: done\nact: reply(text=\"Saved.\")"];
        let mut seen = vec![Activity::Llm { step: 1 }];
        for (step, text) in (1..).zip(answers) {
            seen.push(Activity::answered(&e, text, step));
            let done = turn::step(&mut e, text, &mut |_| None);
            let next = Activity::after(&done, step);
            seen.push(next.clone());
            if next == Activity::Idle {
                break;
            }
        }
        assert_eq!(
            seen,
            [
                Activity::Llm { step: 1 },
                Activity::Tools { step: 1, calls: vec!["write".into(), "list".into()] },
                Activity::Llm { step: 2 },
                Activity::Tools { step: 2, calls: vec![] },
                Activity::Idle,
            ]
        );
        let labels: Vec<String> = seen.iter().map(Activity::label).collect();
        assert_eq!(labels[..3], ["calling the LLM", "running tools (2): write, list", "calling the LLM again (step 2)"]);
        assert_eq!(seen[..3].iter().map(Activity::short).collect::<Vec<_>>(), ["LLM", "tools: write, list", "LLM again"]);
        // Busy states read as working everywhere a status is read.
        assert_eq!(seen[1].status(), Status::Working("running tools (2): write, list".into()));
        assert_eq!(crate::scene::mood(&[seen[1].status()]), crate::scene::Mood::Working);
        assert_eq!(crate::scene::mood(&[Activity::Idle.status()]), crate::scene::Mood::Idle);
        assert_eq!(crate::scene::mood(&[Activity::Failed("x".into()).status()]), crate::scene::Mood::NeedsYou);
    }

    #[test]
    fn every_activity_tint_is_a_token_every_shipped_theme_sets() {
        for t in TOKENS {
            assert!(COLOR_NAMES.contains(&t), "{t} is a colour token");
        }
        let all = [
            Activity::Llm { step: 1 },
            Activity::Tools { step: 1, calls: vec![] },
            Activity::Compressing { step: 1 },
            Activity::Waiting(vec![]),
            Activity::Working("x".into()),
            Activity::NeedsYou("x".into()),
            Activity::Failed("x".into()),
        ];
        assert_eq!(all.iter().filter_map(Activity::token).collect::<Vec<_>>(), TOKENS, "one token each, in order");
        assert_eq!(Activity::Idle.token(), None, "idle has no tint");
        for (name, text) in [
            ("default", include_str!("../../../themes/default.md")),
            ("terminal", include_str!("../../../themes/terminal.md")),
            ("cat", include_str!("../../../themes/cat.md")),
            ("abyss", include_str!("../../../themes/abyss.md")),
            ("bolt", include_str!("../../../themes/bolt.md")),
        ] {
            let theme = Theme::from_md(text).unwrap();
            for mode in [Mode::Light, Mode::Dark] {
                let t = resolve(mode, &theme);
                for a in &all {
                    let c = t.color(a.token().unwrap()).unwrap();
                    let alpha = c & 0xFF;
                    assert!(alpha > 0 && alpha < 0xFF, "themes/{name}.md {}: a translucent tint, not {c:08X}", a.token().unwrap());
                }
            }
        }
    }

    #[test]
    fn a_session_shows_what_its_program_reports_else_working() {
        let mut s = crate::state::State::default();
        let i = s.add_session("claude", "");
        let e = &s.engines[i];
        let running = Activity::session("claude", None);
        assert_eq!(running.label(), "claude: working");
        assert_eq!(Activity::session("claude", Some("editing turn.rs")).label(), "claude: editing turn.rs");
        assert!(running.is_turn());
        assert_eq!(live(e, None, Some(&running)), running);
        assert_eq!(live(e, Some(&Status::Failed("exit 1".into())), None), Activity::Failed("exit 1".into()));
        assert_eq!(live(e, Some(&Status::Missing("no claude".into())), None).label(), "needs you: no claude");
        assert_eq!(live(e, Some(&Status::Ready), None), Activity::Idle);
    }

    #[test]
    fn a_start_session_running_is_waiting_between_turns() {
        let mut e = engine();
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .\nsession: 1 run exit:0 1 cargo check\nsession: 2 start running 2 cargo test".into());
        let a = live(&e, None, None);
        assert_eq!(a, Activity::Waiting(vec!["session 2 `cargo test`".into()]));
        assert_eq!(a.token(), Some("activity_wait"));
        assert!(!a.is_turn(), "a callback may wake it");
        // A turn running wins over waiting.
        assert_eq!(live(&e, None, Some(&Activity::Llm { step: 1 })), Activity::Llm { step: 1 });
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .\nsession: 2 start exit:0 2 cargo test".into());
        assert_eq!(live(&e, None, None), Activity::Idle);
    }
}
