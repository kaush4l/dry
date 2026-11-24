//! The seeded demo: what the hosted page shows on "Load demo", so the app
//! can be looked at with no model server. A few engines with a history,
//! artifacts, a child engine and asks (one open, one answered). Every
//! reply is written the way a model answers (the contract's `thought` and
//! `act`), so the page reads them like any other.

use dry_core::ask::{Answer, Ask};
use dry_core::engines::{Line, to_text};
use dry_core::inference::{Message, OpenAi, Role};
use dry_core::state::State;

use crate::bundle;

fn msg(role: Role, text: &str) -> Message {
    Message { role, text: text.to_string() }
}

/// The model's answer that replies `text`, asking `ask` when given.
fn reply(thought: &str, text: &str, ask: Option<&Ask>) -> String {
    let call = match ask {
        Some(a) => a.call(text),
        None => format!("reply(text=\"{}\")", text.replace('"', "\\\"")),
    };
    format!("thought: {thought}\nact: {call}")
}

/// The demo state. `ai`: the model the engines talk to (this browser's
/// setting, so sending works once a server is set); `today`: days since
/// 1970-01-01, for the dates in it.
pub fn state(ai: &OpenAi, today: i64) -> State {
    let (response, format) = bundle::responses().unwrap_or_default();
    let mut state = State::default();
    let add = |state: &mut State, name: &str, goal: &str| -> usize {
        let i = state.add_engine();
        let id = dry_core::files::new_id(name, &state.engines.iter().take(i).map(|e| e.id.as_str()).collect::<Vec<_>>());
        let e = &mut state.engines[i];
        e.name = name.to_string();
        e.id = id;
        e.description = dry_core::goal::write(goal, "", "");
        e.inference = ai.clone();
        e.response = response.clone();
        e.format = format.clone();
        i
    };

    // 1. A plan with an open choice.
    let trip = add(&mut state, "Trip planner", "Plan a long weekend: where, how and what it costs.");
    let colour = Ask::Choice { options: vec!["Lisbon".into(), "Porto".into(), "Seville".into()] };
    let e = &mut state.engines[trip];
    e.artifacts.insert(
        "plan".into(),
        "## Weekend\n\n- Fly Friday evening, back Monday night\n- Budget: 600 per person\n- Wants: food, walking, the sea\n".into(),
    );
    e.messages = vec![
        msg(Role::User, "Plan a long weekend somewhere warm in November. Food, walking, the sea."),
        msg(
            Role::Assistant,
            "thought: Start a plan artifact with what I was told, then ask where.\nact: write(name=\"plan\", text=\"## Weekend\")",
        ),
        msg(Role::Tool, "write(name=\"plan\", text=\"## Weekend\") → wrote plan (10 characters)"),
        msg(
            Role::Assistant,
            &reply(
                "Three fit food, walking and the sea in November; let them pick.",
                "Three places fit: Lisbon, Porto or Seville. Which one?",
                Some(&colour),
            ),
        ),
    ];

    // 2. A release with an answered date, and a child engine that helped.
    let release = add(&mut state, "Release notes", "Write the release notes for the next version and pick the day it ships.");
    let child = add(&mut state, "Changelog reader", "Read the changelog and list what changed, one line each.");
    state.engines[child].parent = Some(state.engines[release].id.clone());
    let child_id = state.engines[child].id.clone();
    let day = Ask::Date;
    let picked = day.answer(&Answer::Date(today + 3));
    let summary = "Three changes: dry runs in the browser; a store trait with IndexedDB; the renderer is shared.";
    state.engines[child].messages = vec![
        msg(Role::User, "List what changed since the last version."),
        msg(Role::Assistant, &reply("I read the changelog.", summary, None)),
    ];
    let e = &mut state.engines[release];
    e.artifacts.insert("helpers".into(), to_text(&[Line { id: child_id.clone(), call: true, seen: 1, told: false }]));
    e.artifacts.insert("notes".into(), format!("## Next version\n\n{summary}\n"));
    e.messages = vec![
        msg(Role::User, "Draft the release notes and ask me when it ships."),
        msg(
            Role::Assistant,
            &format!(
                "thought: A helper reads the changelog while I wait.\nact: engine_call(name=\"{child_id}\", goal=\"Read the changelog and list what changed.\")"
            ),
        ),
        msg(Role::Tool, &format!("engine_call(name=\"{child_id}\") → {child_id} replied: \"{summary}\"")),
        msg(
            Role::Assistant,
            &reply(
                "The notes are drafted; the day is theirs to pick.",
                "The notes are in the notes artifact. Which day does it ship?",
                Some(&day),
            ),
        ),
        msg(Role::User, &picked),
        msg(
            Role::Assistant,
            &reply("They picked the day.", &format!("Done: it ships {}. The notes are ready.", &picked["Answer (date): ".len()..]), None),
        ),
    ];

    // 3. An empty one, ready for a first message.
    add(&mut state, "Scratch", "Anything: try a message.");

    // 4. A reply that shows a table and asks with a form (the displays and
    // the form ask, docs/engine-standard.md 4d).
    let costs = add(&mut state, "Trip costs", "Split the weekend's costs and book what is left.");
    state.engines[costs].messages = vec![
        msg(Role::User, "What does the Lisbon weekend cost each of us, and what is left to book?"),
        msg(
            Role::Assistant,
            "thought: Lay the costs out as a table, then ask the rest in one form.\nact: reply(text=\"Here is what it costs so far. Fill in the rest and I will book it.\", show=\"table\", data=\"Item|Cost|Each;Flights|380|190;Flat, 3 nights|420|210;Food and trams|240|120\", ask=\"form\", fields=\"Day back:date|Guests:number(1-6 people)|Room:choice(One room/Two rooms)|Notes:text(anything else?)\")",
        ),
    ];
    // As the store gives it back (the goal files end with a newline).
    dry_core::store::from_texts(&dry_core::store::to_texts(&state)).unwrap_or(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::ask::{State as AskState, ask_in, state as ask_state};

    #[test]
    fn the_demo_reads_back_with_an_open_ask_and_an_answered_one() {
        let s = state(&OpenAi::default(), 20_000);
        assert_eq!(s.engines.len(), 5);
        let trip = &s.engines[0];
        let last = trip.messages.len() - 1;
        assert!(matches!(ask_in(trip, &trip.messages[last].text), Some(Ask::Choice { .. })));
        assert_eq!(ask_state(trip, last), AskState::Open);
        let rel = &s.engines[1];
        assert!(matches!(ask_state(rel, 3), AskState::Answered(Some(_))));
        assert_eq!(s.engines[2].parent.as_deref(), Some(rel.id.as_str()));
        // A reply with a table and a form ask.
        let costs = &s.engines[4];
        assert!(matches!(ask_in(costs, &costs.messages[1].text), Some(Ask::Form { .. })));
        assert!(matches!(dry_core::ask::show_in(costs, &costs.messages[1].text), Some(dry_core::show::Show::Table { .. })));
        // It survives the store's texts.
        assert_eq!(dry_core::store::from_texts(&dry_core::store::to_texts(&s)).unwrap(), s);
    }
}
