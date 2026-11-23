//! `render`: one function decides exactly what the model sees, and in
//! which order (the engine standard, `docs/engine-standard.md`).
//!
//! ```text
//! ┌ instructions   spec/goal.md: Goal, Scope, Done    ┐
//! ├ learning       memory/learning.md (turn start)     │
//! ├ phase          where the run is, what it is for    │ system prompt
//! ├ tools          direct + the phase's + artifacts'   │
//! └ artifacts      every artifact as it is now         ┘
//!   history        the newest summary (if compressed), then the messages after it
//!   response       the contract, after the newest user message
//! ```
//!
//! The slots never move, so the same engine in the same state sends the
//! same prompt. Each slot is a section of elements (`crate::element`):
//! `render` gathers the engine's elements, sorts them by place (section,
//! rank) then id, collects each one's part and joins them.
//!
//! Artifacts are shown as they are now: a plain one by its
//! text, a kind (`crate::live`) by its snapshot, the clock last; a
//! snapshot is never written into the messages or the artifact. The
//! response goes at the end of the newest user message (not a second
//! system message, which some local chat templates refuse), so it really
//! is the last thing read.
//!
//! A slot with nothing in it is not sent, but the prompt view lists it
//! with why (`Rendered::empty`), so nothing is missing silently.
//!
//! Pure: what only the shell knows (the time, the disk) comes in as
//! `Snapshots`.

use crate::element::{self, Contribution, Cx, Element, Place};
use crate::inference::{Message, Role};
use crate::live::Snapshots;
use crate::state::Engine;
use crate::template::Slot;

/// Most characters of one artifact shown in the prompt; past it the text
/// is cut with a note saying how to read the rest.
pub const ARTIFACT_CHARS: usize = 4_000;

/// Most characters of all artifacts together; artifacts past it are
/// listed by name and size only.
pub const ARTIFACTS_CHARS: usize = 16_000;

/// A rendered prompt, still in its parts (the UI can show each).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rendered {
    /// `(title, text)` sections of the system prompt, in slot order; only
    /// the ones with something in them.
    pub system: Vec<(String, String)>,
    /// `(slot, why)` for each slot left out because it is empty, and notes
    /// on how the template was read.
    pub empty: Vec<(String, String)>,
    pub history: Vec<Message>,
    /// The newest summary of the history before the cut
    /// (`crate::context`), sent first in the history slot.
    pub summary: Option<String>,
    /// The response contract in the engine's format; empty: free text.
    pub response: String,
    /// The heading the response is shown under.
    pub response_title: String,
    /// Can this step's answer be rendered down into choices
    /// (`crate::jev`)? Informational: the standard call still runs.
    pub jev: crate::jev::Jev,
    /// Every element walked, in order, and why it sent nothing (`None`:
    /// it sent its part). For a view that lists elements.
    pub elements: Vec<Listed>,
}

/// One element as rendered: its id, its place, and why it sent nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub id: String,
    pub place: Place,
    pub empty: Option<String>,
}

/// Render engine `e`'s prompt for showing: without what only the shell
/// knows, so a clock says it is taken when sent.
pub fn render(e: &Engine) -> Rendered {
    render_with(e, &Snapshots::default())
}

/// Render engine `e`'s prompt as it is sent now (`outside`: the time).
///
/// The element walk (`crate::element`): gather the engine's elements,
/// sorted by place (section, rank) then id; collect each; join each
/// section's parts under its title and lead.
pub fn render_with(e: &Engine, outside: &Snapshots) -> Rendered {
    let cx = Cx::new(e, outside);
    let mut r = Rendered::default();
    // A broken template is said and the default used, so the engine still runs.
    if let Some(why) = &cx.template_note {
        r.empty.push(("Template".into(), format!("template.md did not read, so the default is used: {why}")));
    }
    let walked: Vec<(Element, Contribution)> = element::gather(&cx)
        .into_iter()
        .map(|el| {
            let c = el.collect(&cx);
            (el, c)
        })
        .collect();
    let t = &cx.template;
    for s in Slot::ALL {
        let parts = walked.iter().filter(|(el, _)| el.place().section == s).map(|(_, c)| c);
        match s {
            // The history: the summary leads it (sent at the end of the
            // system prompt), then the messages after the cut.
            Slot::History => {
                for c in parts {
                    match (&c.text, c.messages.is_empty()) {
                        (Some(summary), _) => r.summary = Some(summary.clone()),
                        (None, false) => r.history.extend(c.messages.iter().cloned()),
                        (None, true) => {}
                    }
                }
            }
            Slot::Response => r.response = parts.filter_map(|c| c.text.clone()).collect::<Vec<_>>().join("\n\n"),
            // A system section: its parts joined, under its lead; with no
            // part, the first reason it is empty.
            _ => {
                let parts: Vec<&Contribution> = parts.collect();
                let texts: Vec<&str> = parts.iter().filter_map(|c| c.text.as_deref()).collect();
                if texts.is_empty() {
                    if let Some(why) = parts.iter().find_map(|c| c.empty.clone()) {
                        r.empty.push((t.title(s), why));
                    }
                    continue;
                }
                let body = texts.join(if s == Slot::Tools { "\n" } else { "\n\n" });
                if body.trim().is_empty() {
                    continue;
                }
                let lead = t.lead(s);
                r.system.push((t.title(s), if lead.is_empty() { body } else { format!("{lead}\n{body}") }));
            }
        }
    }
    // Notes on how things were read: a phase answered some other way than
    // this runtime can (answered the standard way), then goal.md's.
    if let (_, Some(note)) = e.run.response(&cx.phases) {
        r.empty.push(("Response mode".into(), note));
    }
    if e.legacy_goal {
        r.empty.push((
            crate::goal::FILE.to_string(),
            "read from spec/instruction.md: there is no goal.md yet; the next save writes goal.md".to_string(),
        ));
    }
    r.empty.extend(cx.goal.notes.iter().map(|n| (crate::goal::FILE.to_string(), n.clone())));
    r.empty.extend(cx.held_notes.iter().map(|n| (element::FILE.to_string(), n.clone())));
    r.response_title = t.title(Slot::Response);
    // The response and tools just rendered, reduced to choices: whether a
    // jev could answer this step (nothing is sent differently).
    r.jev = crate::jev::check(e);
    r.elements = walked
        .into_iter()
        .map(|(el, c)| {
            let sent = c.text.is_some() || !c.messages.is_empty();
            let empty = if sent { None } else { Some(c.empty.unwrap_or_else(|| "nothing in it".into())) };
            Listed { id: el.id(), place: el.place(), empty }
        })
        .collect();
    r
}

/// The artifacts slot in its sections, in order, each exactly as sent
/// (`### name ...` and its body): `(Some(artifact name), section)`, or
/// `(None, section)` for the skills element. The artifact view
/// (`crate::inspect`) shows each one from here, so it is what is sent.
pub fn artifact_sections(e: &Engine, outside: &Snapshots) -> Vec<(Option<String>, String)> {
    let cx = Cx::new(e, outside);
    element::artifacts(&cx)
        .into_iter()
        .filter_map(|el| {
            let text = el.collect(&cx).text?;
            Some((el.artifact().map(str::to_string), text))
        })
        .collect()
}

/// The heading the summary is sent under, at the end of the system prompt
/// (the history slot starts there, so chat templates that want one system
/// message and alternating turns still get them).
pub const SUMMARY_TITLE: &str = "Earlier conversation (summary)";

impl Rendered {
    /// The spec slots, tools and snapshots: the system prompt without the summary.
    fn fixed_text(&self) -> String {
        self.system.iter().map(|(title, text)| format!("## {title}\n\n{text}")).collect::<Vec<_>>().join("\n\n")
    }

    fn system_text(&self) -> String {
        let fixed = self.fixed_text();
        match &self.summary {
            Some(s) if fixed.is_empty() => format!("## {SUMMARY_TITLE}\n\n{s}"),
            Some(s) => format!("{fixed}\n\n## {SUMMARY_TITLE}\n\n{s}"),
            None => fixed,
        }
    }

    /// The estimated tokens never compressed (`crate::context`): the spec
    /// slots, tools and snapshots, the response contract, and the current
    /// user message.
    pub fn fixed_tokens(&self) -> usize {
        use crate::context::{MESSAGE_OVERHEAD, message_tokens, tokens};
        let system = self.fixed_text();
        let system = if system.is_empty() { 0 } else { tokens(&system) + MESSAGE_OVERHEAD };
        let response = if self.response.is_empty() { 0 } else { tokens(&format!("\n\n## {}\n\n{}", self.response_title, self.response)) };
        let user = self.history.iter().rev().find(|m| m.role == Role::User).map_or(0, message_tokens);
        system + response + user
    }

    /// The messages to send: the system prompt, then the history with the
    /// response contract after the newest user message.
    pub fn messages(&self) -> Vec<Message> {
        let mut out = Vec::new();
        let system = self.system_text();
        if !system.is_empty() {
            out.push(Message { role: Role::System, text: system });
        }
        out.extend(self.history.iter().cloned());
        if !self.response.is_empty()
            && let Some(last) = out.iter_mut().rev().find(|m| m.role == Role::User)
        {
            last.text = format!("{}\n\n## {}\n\n{}", last.text, self.response_title, self.response);
        }
        out
    }

    /// Everything as one text, for showing what the model sees, then the
    /// slots left out and why.
    pub fn to_text(&self) -> String {
        let mut text = self.messages().iter().map(|m| format!("[{:?}]\n{}", m.role, m.text)).collect::<Vec<_>>().join("\n\n");
        if !self.empty.is_empty() {
            text.push_str("\n\n[not sent]");
            for (slot, why) in &self.empty {
                text.push_str(&format!("\n- {slot}: {why}"));
            }
        }
        text
    }
}

#[cfg(test)]
mod legacy;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;
    use crate::response::{Format, Responses};

    #[test]
    fn instructions_first_history_then_the_response_last() {
        let mut e = Engine::new("E", "Be brief.", OpenAi::default());
        e.messages.push(Message { role: Role::User, text: "hi".into() });
        e.messages.push(Message { role: Role::Assistant, text: "thought: greet\nact: reply(text=\"hello\")".into() });
        e.messages.push(Message { role: Role::User, text: "again".into() });
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.format = Format::default();
        let m = render(&e).messages();
        assert_eq!(m[0].role, Role::System);
        assert!(m[0].text.starts_with("## Instructions\n\n### Goal — what you are for, and who you serve\nBe brief."), "{}", m[0].text);
        assert!(m[0].text.contains("## Tools\n\nCall these in `act`:\n- reply("), "{}", m[0].text);
        assert!(m[0].text.ends_with("## Artifacts\n\nNone yet."));
        assert_eq!(m[1].text, "hi", "older user messages are sent as they were");
        let last = &m.last().unwrap().text;
        assert!(last.starts_with("again\n\n## Response"), "{last}");
        assert!(last.contains("- act: "));
        // The engine's own messages are not changed by rendering.
        assert_eq!(e.messages[2].text, "again");
    }

    #[test]
    fn goal_sections_have_their_place_and_problems_are_noted() {
        let mut e = with_contract();
        e.description = "## Done\nA list.\n\n## Goal\nSort mail.\n\n## Mood\nhappy".into();
        e.legacy_goal = true;
        let r = render(&e);
        let instructions = &r.system[0].1;
        assert!(
            instructions
                .starts_with("### Goal — what you are for, and who you serve\nSort mail.\n\n### Done — what a finished result looks like"),
            "{instructions}"
        );
        assert!(!instructions.contains("happy"), "an unknown section is not sent");
        let notes: Vec<&str> = r.empty.iter().filter(|(n, _)| n == "goal.md").map(|(_, w)| w.as_str()).collect();
        assert_eq!(notes[0], "read from spec/instruction.md: there is no goal.md yet; the next save writes goal.md");
        assert!(
            notes.iter().any(|n| n.contains("`## Mood` is not a goal section")) && notes.iter().any(|n| n.contains("no `## Scope`")),
            "{notes:?}"
        );
        assert!(r.to_text().contains("\n- goal.md: read from spec/instruction.md"), "in the [not sent] notes");
    }

    #[test]
    fn the_learning_slot_is_the_turn_start_copy_until_the_next_message() {
        let mut e = with_contract();
        e.artifacts.insert("learn".into(), "kind: learning".into());
        e.learning = "## user\n- Short answers.\n".into();
        crate::learning::learn(&mut e, "user", "No lists.", None, 1).unwrap();
        let learning = |e: &Engine| render(e).system.into_iter().find(|(t, _)| t == "Learning").map(|(_, b)| b).unwrap();
        assert!(learning(&e).contains("- Short answers.") && !learning(&e).contains("No lists"), "the turn's first copy: {}", learning(&e));
        let mut s = crate::state::State { engines: vec![e], ..Default::default() };
        s.add_text(0, "next");
        assert!(learning(&s.engines[0]).contains("- No lists."), "the next message sends the file as it is");
    }

    #[test]
    fn the_render_says_whether_jev_could_answer_and_sends_the_same() {
        let mut e = with_contract();
        e.tools = Vec::new();
        e.template = "| phase | response |\n|-|-|\n| gate | jev |\n| work | |\n\n## phase: gate\nRun it?\n\n## phase: work\nDo it.".into();
        let r = render(&e);
        assert!(r.jev.possible, "{:?}", r.jev);
        let (_, notes) = crate::inspect::slots(&e, &r);
        assert!(notes.iter().any(|(k, v)| k == "jev" && v.starts_with("possible (1 choice: goto_phase work)")), "{notes:?}");
        let bare = render(&with_contract()).jev;
        assert!(!bare.possible && bare.missing.contains(&"tool `reply` has no jev metadata".to_string()), "{bare:?}");
        assert_eq!(r.messages(), render(&e).messages(), "nothing sent changes");
    }

    #[test]
    fn no_contract_means_free_text() {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.messages.push(Message { role: Role::User, text: "hi".into() });
        assert_eq!(render(&e).messages(), e.messages);
    }

    fn with_contract() -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.response = Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.messages.push(Message { role: Role::User, text: "what time is it?".into() });
        e
    }

    #[test]
    fn artifacts_are_shown_with_their_text_and_long_ones_are_cut() {
        let mut e = with_contract();
        e.artifacts.insert("plan".into(), "1. read\n2. answer".into());
        e.artifacts.insert("long".into(), "x".repeat(ARTIFACT_CHARS + 10));
        let system = &render(&e).messages()[0].text;
        assert!(system.contains("### plan (17 characters)\n\n1. read\n2. answer"), "{system}");
        let cut = format!("… cut: {ARTIFACT_CHARS} of {} characters shown; read(name=\"long\") for all of it.", ARTIFACT_CHARS + 10);
        assert!(system.contains(&cut), "{system}");
    }

    #[test]
    fn past_the_total_budget_artifacts_are_named_only() {
        let mut e = with_contract();
        for i in 0..5 {
            e.artifacts.insert(format!("part{i}"), "y".repeat(ARTIFACT_CHARS));
        }
        let system = &render(&e).messages()[0].text;
        assert!(system.contains("### part3 (4000 characters)\n\nyyy"), "the first four fit");
        assert!(system.contains("### part4 (4000 characters)\n\nNot shown"), "{system}");
    }

    #[test]
    fn artifacts_come_before_the_history_with_the_clock_last_and_never_kept() {
        let mut e = with_contract();
        e.artifacts.insert("a-now".into(), "kind: clock".into());
        e.artifacts.insert("plan".into(), "tea".into());
        e.artifacts.insert("z-scripts".into(), "kind: scripts".into());
        let at = Snapshots { clock: Some(crate::live::Clock::new(1_795_356_330, 0)), ..Default::default() };
        let m = render_with(&e, &at).messages();
        let system = &m[0].text;
        let (plan, scripts, clock) = (
            system.find("### plan").unwrap(),
            system.find("### z-scripts").unwrap(),
            system.find("### a-now — the date and time now\n\n2026-11-22 Sunday 14:05 UTC").expect(system),
        );
        assert!(plan < scripts && scripts < clock, "plain, then kinds, the clock last: {system}");
        assert!(system.ends_with("UTC"), "the clock is the last thing in the system prompt");
        assert_eq!(m[1].text.split("\n\n## Response").next(), Some("what time is it?"), "history after the artifacts, unchanged");
        assert_eq!(e.artifacts["a-now"], "kind: clock", "the snapshot is not written into the artifact");
        assert!(render(&e).messages()[0].text.contains("(taken when the prompt is sent)"));
    }

    #[test]
    fn every_slot_is_in_order_and_an_empty_one_says_why() {
        let mut e = with_contract();
        e.description = "## Goal\nBe brief.\n## Scope\nAlone.\n## Done\nSaid.".into();
        e.learning = "## lessons\n- Check first.\n".into();
        e.template = "| phase |\n|---|\n| only |\n\n## phase: only\nDo it.\n\n## tools\ntitle: Your tools\nCall these:".into();
        let r = render(&e);
        let names: Vec<&str> = r.system.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Instructions", "Learning", "Phase", "Your tools", "Artifacts"], "the template renames a slot");
        assert!(r.system[3].1.starts_with("Call these:\n- reply("), "and sets its lead");
        assert_eq!(r.system[1].1, "What you learned so far:\n### lessons — rules for next time, once an outcome is clear\n- Check first.");
        assert!(r.empty.is_empty(), "every goal section is there: no notes {:?}", r.empty);
        let bare = render(&with_contract());
        assert!(bare.empty.iter().any(|(n, why)| n == "Phase" && why.contains("no phases in the template")));
        assert!(bare.to_text().contains("[not sent]\n- Instructions: no instructions (goal.md is empty)\n- Learning: nothing learned yet"));
    }
}
