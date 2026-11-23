//! The dashboard: what Home shows. Home is fixed; engines (model engines
//! and sessions) are the live, changing part of the app. The dashboard
//! sums them up in a few tiles: how many, how many are working right now,
//! which need you, and what this runtime can do.
//!
//! Pure: the shell hands in the engines, the status each one reported
//! (its own `engine_status`), and the runtime's capabilities.

use crate::activity::Activity;
use crate::card::{Status, Tone};
use crate::composer::{Capabilities, Capability};
use crate::inference::Role;
use crate::state::Engine;

/// An engine's live status: what its turn is doing now (`now`, an
/// `Activity`), else what it last reported (failed, a job's result, ...),
/// else waiting on a terminal callback, else ready when the last message
/// is an answer, idle otherwise. The nav's dots, the cards, the tiles and
/// the scene's mood all read it from here, so they agree; `live_activity`
/// is the same answer as an activity, for the tint and its label.
pub fn live_status(e: &Engine, reported: Option<&Status>, now: Option<&Activity>) -> Status {
    let a = live_activity(e, reported, now);
    if a != Activity::Idle {
        return a.status();
    }
    match reported {
        Some(s) => s.clone(),
        None => match e.messages.last() {
            Some(m) if m.role == Role::Assistant => Status::Ready,
            _ => Status::Idle,
        },
    }
}

/// Engine `e`'s activity now (`crate::activity::live`).
pub fn live_activity(e: &Engine, reported: Option<&Status>, now: Option<&Activity>) -> Activity {
    crate::activity::live(e, reported, now)
}

/// One number on the dashboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub label: &'static str,
    pub value: String,
    pub detail: String,
    pub tone: Tone,
}

/// The tiles for `engines`, whose live statuses are `statuses` (same
/// order), in a runtime with `caps`.
pub fn tiles(engines: &[Engine], statuses: &[Status], caps: &Capabilities) -> Vec<Tile> {
    let sessions = engines.iter().filter(|e| e.session.is_some()).count();
    let named = |pick: fn(&Status) -> bool| -> Vec<&str> {
        engines.iter().zip(statuses).filter(|(_, s)| pick(s)).map(|(e, _)| e.name.as_str()).collect()
    };
    let working = named(|s| matches!(s, Status::Working(_)));
    let stuck = named(|s| matches!(s, Status::Failed(_) | Status::Missing(_)));
    let list = |names: &[&str], none: &str| match names {
        [] => none.to_string(),
        [a] => a.to_string(),
        [a, b] => format!("{a}, {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} +{}", rest.len()),
    };
    let messages: usize = engines.iter().map(|e| e.messages.len()).sum();
    let voice = caps.status(Capability::Voice);
    vec![
        Tile {
            label: "Engines",
            value: engines.len().to_string(),
            detail: format!("{} model · {sessions} session", engines.len() - sessions),
            tone: Tone::Neutral,
        },
        Tile {
            label: "Working",
            value: working.len().to_string(),
            detail: list(&working, "all quiet"),
            tone: if working.is_empty() { Tone::Neutral } else { Tone::Busy },
        },
        Tile {
            label: "Need you",
            value: stuck.len().to_string(),
            detail: list(&stuck, "nothing"),
            tone: if stuck.is_empty() { Tone::Good } else { Tone::Bad },
        },
        Tile { label: "Messages", value: messages.to_string(), detail: "in every engine".into(), tone: Tone::Neutral },
        Tile {
            label: "Voice",
            value: voice.word().to_string(),
            detail: voice.detail().unwrap_or("microphone and speech-to-text found").to_string(),
            tone: voice.tone(),
        },
    ]
}

/// One engine on the board: what it is doing (its tint), how many
/// replies it gave and its last one, and its prompt against its model's
/// window (`context`, a share of it, when the shell knows it).
#[derive(Clone, Debug, PartialEq)]
pub struct BoardEngine {
    pub index: usize,
    pub name: String,
    pub kind: String,
    /// The engine that made it, by name.
    pub parent: Option<String>,
    pub activity: Activity,
    pub status: Status,
    pub replies: usize,
    pub last_reply: Option<String>,
    pub context: Option<f64>,
}

/// An artifact on the board, most active engines' first.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardArtifact {
    pub engine: usize,
    pub engine_name: String,
    pub name: String,
    /// Its `kind:` (`file`, `engines`, ...), or `text`.
    pub kind: String,
    /// Its first line that says something.
    pub line: String,
}

/// What Home shows below its tiles, for any shell to draw: every engine
/// and the artifacts most recently worked on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub engines: Vec<BoardEngine>,
    pub artifacts: Vec<BoardArtifact>,
}

/// The most artifacts the board lists.
pub const BOARD_ARTIFACTS: usize = 6;

/// The board for `engines`, with each one's live activity, status and
/// context share (same order; a missing entry is idle / unknown).
pub fn board(engines: &[Engine], activities: &[Activity], statuses: &[Status], context: &[Option<f64>]) -> Board {
    let name_of = |id: &str| engines.iter().find(|e| e.id == id).map(|e| e.name.clone());
    let list = engines
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let replies = crate::engines::replies(e);
            BoardEngine {
                index: i,
                name: e.name.clone(),
                kind: e.kind().to_string(),
                parent: e.parent.as_deref().and_then(name_of),
                activity: activities.get(i).cloned().unwrap_or_default(),
                status: statuses.get(i).cloned().unwrap_or_default(),
                replies: replies.len(),
                last_reply: replies.last().cloned(),
                context: context.get(i).copied().flatten(),
            }
        })
        .collect();
    // Recent: the engines with the longest logs first (the log is the
    // only clock an engine keeps), then their artifacts by name.
    let mut order: Vec<usize> = (0..engines.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse((engines[i].messages.len(), i)));
    let mut artifacts = Vec::new();
    for i in order {
        let e = &engines[i];
        for (name, text) in &e.artifacts {
            let kind = crate::live::Live::of(text).map_or("text", |k| k.as_str()).to_string();
            let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("kind:")).unwrap_or("").trim_start_matches('#').trim().to_string();
            artifacts.push(BoardArtifact { engine: i, engine_name: e.name.clone(), name: name.clone(), kind, line });
        }
    }
    artifacts.truncate(BOARD_ARTIFACTS);
    Board { engines: list, artifacts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_lists_every_engine_and_the_busiest_ones_artifacts() {
        let s = crate::files::sample();
        let acts = vec![Activity::Llm { step: 1 }];
        let b = board(&s.engines, &acts, &[], &[Some(0.5)]);
        assert_eq!(b.engines.len(), s.engines.len());
        assert_eq!((b.engines[0].activity.clone(), b.engines[0].context), (Activity::Llm { step: 1 }, Some(0.5)));
        let child = s.engines.iter().position(|e| e.parent.is_some()).unwrap();
        let parent = s.engines.iter().find(|e| Some(&e.id) == s.engines[child].parent.as_ref()).unwrap();
        assert_eq!(b.engines[child].parent.as_deref(), Some(parent.name.as_str()));
        assert!(b.artifacts.iter().any(|a| a.name == "plan" && a.engine == 0));
        assert!(b.artifacts.len() <= BOARD_ARTIFACTS);
    }
    use crate::state::State;

    fn engines() -> Vec<Engine> {
        let mut s = State::default();
        s.add_engine();
        s.add_engine();
        s.add_session("claude", "");
        s.engines[0].name = "Alpha".into();
        s.engines[1].name = "Beta".into();
        s.engines[2].name = "Gamma".into();
        s.add_text(0, "hi");
        s.engines
    }

    #[test]
    fn a_reported_status_wins_else_the_last_message_decides() {
        let mut e = engines().remove(0);
        assert_eq!(live_status(&e, None, None), Status::Idle);
        e.messages.push(crate::inference::Message { role: Role::Assistant, text: "hello".into() });
        assert_eq!(live_status(&e, None, None), Status::Ready);
        assert_eq!(live_status(&e, Some(&Status::Working("thinking".into())), None), Status::Working("thinking".into()));
        assert_eq!(live_status(&e, Some(&Status::Done("3 MCP tool(s)".into())), None), Status::Done("3 MCP tool(s)".into()));
        // A turn's activity wins, and reads as working.
        let tools = Activity::Tools { step: 1, calls: vec!["read".into(), "edit".into()] };
        assert_eq!(live_status(&e, Some(&Status::Ready), Some(&tools)), Status::Working("running tools (2): read, edit".into()));
        assert_eq!(live_activity(&e, None, Some(&tools)).token(), Some("activity_tools"));
    }

    #[test]
    fn tiles_count_kinds_work_and_trouble() {
        let e = engines();
        let statuses = [Status::Working("asking".into()), Status::Failed("timeout".into()), Status::Idle];
        let t = tiles(&e, &statuses, &Capabilities::default());
        let by = |label: &str| t.iter().find(|t| t.label == label).unwrap().clone();
        assert_eq!((by("Engines").value.as_str(), by("Engines").detail.as_str()), ("3", "2 model · 1 session"));
        assert_eq!((by("Working").value.as_str(), by("Working").detail.as_str()), ("1", "Alpha"));
        assert_eq!((by("Need you").detail.as_str(), by("Need you").tone), ("Beta", Tone::Bad));
        assert_eq!(by("Messages").value, "1");
        assert_eq!(by("Voice").tone, Tone::Bad, "not checked counts as missing");
    }
}
