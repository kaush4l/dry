//! Elements: the units of an engine's prompt (docs/elements.md).
//!
//! `crate::prompt::render` walks an engine's elements in one fixed order
//! and each gives its part of the context: a part of its section's text,
//! the tools it offers, its evidence (snapshot rows), or why it has
//! nothing. Every kind of thing in the prompt is an element: the goal,
//! learning, the phase, the tools, each artifact, the history, the
//! response contract.
//!
//! The order is decided here and nowhere else: by `Place` (the section in
//! the agreed order, `Slot`, then a rank inside it), then by `id`. Ranks
//! are fixed numbers (`rank`), so no tie is left to a map's order or to
//! the order the elements were gathered in.
//!
//! Adding an element is adding a variant with its `id`, `place` and
//! `collect`, and gathering it in `gather`; the renderer does not change.
//!
//! Elements beyond the artifacts and tools are held by name in the
//! engine's `spec/elements.md` (`held`): one id per line. Built:
//! `subscriptions` (every model and program a child can be run by, with
//! whether each is present: `crate::subscriptions`) and `environment` (the
//! machine the file and terminal tools run on), each as the shell reported
//! it in its registry.
//!
//! Pure: what only the shell knows comes in through `Cx` (`Snapshots`,
//! and the shell's registry the engine carries, `Engine::machine`).

use crate::capability::{self, Equipped};
use crate::environment::Environment;
use crate::inference::Message;
use crate::live::{Live, Snapshots};
use crate::phase::Phase;
use crate::prompt::{ARTIFACT_CHARS, ARTIFACTS_CHARS};
use crate::response::ResponseFormat;
use crate::state::Engine;
use crate::subscriptions::Subscriptions;
use crate::template::{self, Slot, Template};

/// A section of the prompt: the slots, in the only order they are sent
/// (instructions, learning, phase, tools, artifacts, history, response).
pub type Section = Slot;

/// Where an element goes: its section, then its rank inside it (lower
/// first). Elements of the same place sort by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Place {
    pub section: Section,
    pub rank: u32,
}

/// The ranks inside each section. Gaps leave room for new elements (the
/// subscriptions and machines go between the kinds and the environment).
pub mod rank {
    /// The one element of a section that has one.
    pub const ONLY: u32 = 0;
    /// Tools: the direct tools and the phase's and artifacts' (the toolbox).
    pub const TOOLBOX: u32 = 100;
    /// Tools: those the outside capabilities bring.
    pub const OUTSIDE_TOOLS: u32 = 200;
    /// Tools: the MCP tools, from here on in the engine's order.
    pub const MCP: u32 = 300;
    /// Artifacts: the plain ones, by name.
    pub const PLAIN: u32 = 100;
    /// Artifacts: the skills element (what can be loaded).
    pub const SKILLS: u32 = 200;
    /// Artifacts: the kinds (`crate::live::Live`), from here on in the
    /// kinds' order, then by name.
    pub const KIND: u32 = 300;
    /// Artifacts: one with only outside or compiled capabilities, no kind.
    pub const CAPABILITIES: u32 = 600;
    /// Artifacts: the subscriptions (`spec/elements.md` holds them), after
    /// every artifact.
    pub const SUBSCRIPTIONS: u32 = 700;
    // 750 is kept for the machines (docs/elements.md, phase 4).
    /// Artifacts: the environment (`spec/elements.md` holds it), after
    /// the subscriptions and before the clock.
    pub const ENVIRONMENT: u32 = 800;
    /// Artifacts: the clock, last of all: it changes most.
    pub const CLOCK: u32 = 900;
    /// History: the newest summary.
    pub const SUMMARY: u32 = 100;
    /// History: the messages after it.
    pub const MESSAGES: u32 = 200;
}

/// One unit of the prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    /// goal.md's sections, in their order.
    Goal,
    /// memory/learning.md as at the turn's start.
    Learning,
    /// The plan, the current phase expanded.
    Phase,
    /// The toolbox: direct tools, the phase's, the artifacts'.
    Tools,
    /// The outside capabilities' tools (declared here, or a peer's).
    OutsideTools,
    /// One MCP tool: `index` in the engine's `mcp_tools`.
    Mcp { index: usize, name: String },
    /// A plain artifact by its text, with `room` characters it may show
    /// (what the artifacts' total budget leaves it).
    Artifact { name: String, room: usize },
    /// The skills that can be loaded.
    Skills,
    /// A capability artifact: a kind by its snapshot (`live`), or its
    /// capabilities' docs and snapshots.
    Kind { name: String, live: Option<Live> },
    /// Every model and program a child can be run by, each present or
    /// not and why, with its recorded usage (held in `spec/elements.md`).
    Subscriptions,
    /// The environment the file and terminal tools run on: kind, state,
    /// workspace, runtimes, saved machine (held in `spec/elements.md`).
    Environment,
    /// The artifacts section when there is no artifact.
    NoArtifacts,
    /// A section that means nothing without a response contract (tools,
    /// artifacts).
    NoContract(Section),
    /// The newest summary of the history before the cut.
    Summary,
    /// The messages after the cut.
    Messages,
    /// The response contract in the engine's format.
    Response,
}

/// An element's part of the context now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contribution {
    /// Its part of its section's text (the renderer joins the parts and
    /// puts the section's title and lead around them).
    pub text: Option<String>,
    /// The tools it offers, by name.
    pub tools: Vec<String>,
    /// Its snapshot (evidence) as read now.
    pub evidence: Vec<String>,
    /// The messages it sends (the history).
    pub messages: Vec<Message>,
    /// Why it has nothing (shown in the prompt view).
    pub empty: Option<String>,
}

impl Contribution {
    fn text(text: String) -> Contribution {
        Contribution { text: Some(text), ..Contribution::default() }
    }

    fn empty(why: impl Into<String>) -> Contribution {
        Contribution { empty: Some(why.into()), ..Contribution::default() }
    }

    fn of(body: Result<String, String>) -> Contribution {
        match body {
            Ok(b) => Contribution::text(b),
            Err(why) => Contribution::empty(why),
        }
    }
}

/// What render takes, read once: the engine, what only the shell knows,
/// its template, phases, goal and capability artifacts.
pub struct Cx<'a> {
    pub engine: &'a Engine,
    pub outside: &'a Snapshots<'a>,
    /// The engine's template, or the default when it does not read.
    pub template: Template,
    /// Why the engine's template did not read (the default is used).
    pub template_note: Option<String>,
    pub phases: Vec<Phase>,
    pub goal: crate::goal::Goal,
    /// Its capability artifacts (`crate::capability::equipped`).
    pub equipped: Vec<Equipped<'a>>,
    /// The extra elements it holds (`spec/elements.md`, `held`), and why
    /// any line of it was not read.
    pub held: Vec<String>,
    pub held_notes: Vec<String>,
    /// The environment the shell reported (`Registry::environment`, in
    /// the registry the engine carries). `None`: none reported.
    pub environment: Option<&'a Environment>,
    /// The subscriptions the shell reported (`Registry::subscriptions`).
    /// `None`: none reported.
    pub subscriptions: Option<&'a Subscriptions>,
}

/// The file in `spec/` that lists the extra elements an engine holds.
pub const FILE: &str = "elements.md";

/// The elements `spec/elements.md` may name that are built.
pub const BUILT: &[&str] = &["subscriptions", "environment"];
/// Named in the design (docs/elements.md) and not built yet.
pub const PLANNED: &[(&str, &str)] = &[("machines", "phase 4")];

/// `spec/elements.md` read: the ids it holds (built ones, each once, in
/// the file's order) and a note for every line that is not one, with why.
/// Blank lines and `#` lines are skipped; a `- ` before an id is allowed.
pub fn held(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut ids, mut notes) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let id = line.trim();
        let id = id.strip_prefix("- ").map_or(id, str::trim);
        if id.is_empty() || id.starts_with('#') {
            continue;
        }
        if BUILT.contains(&id) {
            if !ids.iter().any(|h| h == id) {
                ids.push(id.to_string());
            }
        } else if let Some((_, when)) = PLANNED.iter().find(|(p, _)| *p == id) {
            notes.push(format!("`{id}` is not built yet (docs/elements.md, {when}): left out"));
        } else {
            notes.push(format!("`{id}` is not an element (known: {}): left out", BUILT.join(", ")));
        }
    }
    (ids, notes)
}

impl<'a> Cx<'a> {
    pub fn new(engine: &'a Engine, outside: &'a Snapshots<'a>) -> Cx<'a> {
        let (template, template_note) = match engine.template() {
            Ok(t) => (t, None),
            Err(why) => (template::parse("").expect("the default template reads"), Some(why)),
        };
        let phases = template.phases().unwrap_or_default();
        let (held, held_notes) = held(&engine.elements);
        Cx {
            engine,
            outside,
            template,
            template_note,
            phases,
            goal: crate::goal::parse(&engine.description),
            equipped: capability::equipped(engine),
            held,
            held_notes,
            environment: engine.machine.0.environment.as_ref(),
            subscriptions: engine.machine.0.subscriptions.as_ref(),
        }
    }
}

impl Element {
    /// Its id: unique in one engine's prompt, and the last tie-break.
    pub fn id(&self) -> String {
        match self {
            Element::Goal => "goal".into(),
            Element::Learning => "learning".into(),
            Element::Phase => "phase".into(),
            Element::Tools => "tools".into(),
            Element::OutsideTools => "tools/outside".into(),
            Element::Mcp { name, .. } => format!("tools/mcp/{name}"),
            Element::Artifact { name, .. } | Element::Kind { name, .. } => format!("artifact/{name}"),
            Element::Skills => "skills".into(),
            Element::Subscriptions => "subscriptions".into(),
            Element::Environment => "environment".into(),
            Element::NoArtifacts => "artifacts/none".into(),
            Element::NoContract(s) => format!("{}/no-contract", s.word()),
            Element::Summary => "summary".into(),
            Element::Messages => "messages".into(),
            Element::Response => "response".into(),
        }
    }

    /// Where it goes.
    pub fn place(&self) -> Place {
        let at = |section, rank| Place { section, rank };
        match self {
            Element::Goal => at(Slot::Instructions, rank::ONLY),
            Element::Learning => at(Slot::Learning, rank::ONLY),
            Element::Phase => at(Slot::Phase, rank::ONLY),
            Element::Tools => at(Slot::Tools, rank::TOOLBOX),
            Element::OutsideTools => at(Slot::Tools, rank::OUTSIDE_TOOLS),
            Element::Mcp { index, .. } => at(Slot::Tools, rank::MCP + u32::try_from(*index).unwrap_or(u32::MAX - rank::MCP)),
            Element::Artifact { .. } => at(Slot::Artifacts, rank::PLAIN),
            Element::Skills => at(Slot::Artifacts, rank::SKILLS),
            Element::Kind { live: Some(Live::Clock), .. } => at(Slot::Artifacts, rank::CLOCK),
            Element::Kind { live: Some(l), .. } => at(Slot::Artifacts, rank::KIND + *l as u32),
            Element::Kind { live: None, .. } => at(Slot::Artifacts, rank::CAPABILITIES),
            Element::Subscriptions => at(Slot::Artifacts, rank::SUBSCRIPTIONS),
            Element::Environment => at(Slot::Artifacts, rank::ENVIRONMENT),
            Element::NoArtifacts | Element::NoContract(_) => at(self.section_of_gate(), rank::ONLY),
            Element::Summary => at(Slot::History, rank::SUMMARY),
            Element::Messages => at(Slot::History, rank::MESSAGES),
            Element::Response => at(Slot::Response, rank::ONLY),
        }
    }

    fn section_of_gate(&self) -> Section {
        match self {
            Element::NoContract(s) => *s,
            _ => Slot::Artifacts,
        }
    }

    /// The artifact it shows, if it is one (the skills element is not).
    pub fn artifact(&self) -> Option<&str> {
        match self {
            Element::Artifact { name, .. } | Element::Kind { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Its part of the context now.
    pub fn collect(&self, cx: &Cx) -> Contribution {
        let e = cx.engine;
        match self {
            Element::Goal => {
                let body = cx.goal.render();
                if body.trim().is_empty() { Contribution::empty("no instructions (goal.md is empty)") } else { Contribution::text(body) }
            }
            Element::Learning => Contribution::of(crate::learning::slot(e)),
            Element::Phase => {
                if cx.phases.is_empty() {
                    Contribution::empty("no phases in the template: one implicit phase")
                } else {
                    Contribution::text(e.run.section(&cx.phases))
                }
            }
            Element::Tools => {
                let names = crate::live::toolbox(e);
                Contribution { text: Some(crate::tools::describe(&names)), tools: names, ..Contribution::default() }
            }
            Element::OutsideTools => {
                let brought = capability::describe(e);
                if brought.is_empty() {
                    Contribution::empty("no outside capability brings a tool")
                } else {
                    let tools = capability::outside_tools(e).into_iter().map(|(n, _)| n).collect();
                    Contribution { text: Some(brought), tools, ..Contribution::default() }
                }
            }
            Element::Mcp { index, .. } => match e.mcp_tools.get(*index) {
                Some(tool) => Contribution { text: Some(tool.describe()), tools: vec![tool.name.clone()], ..Contribution::default() },
                None => Contribution::empty("no such MCP tool"),
            },
            Element::Artifact { name, room } => Contribution::text(plain(name, e.artifacts.get(name).map_or("", String::as_str), *room)),
            Element::Skills => {
                let skills = e.visible_skills();
                if skills.is_empty() {
                    Contribution::empty("no skills")
                } else {
                    Contribution::text(format!("### skills — your skills\n\n{}", crate::skill::snapshot(&skills)))
                }
            }
            Element::Kind { name, .. } => match cx.equipped.iter().find(|q| q.name == *name) {
                Some(q) => kind(name, q, cx),
                None => Contribution::empty("none of its capabilities is present"),
            },
            Element::Subscriptions => match cx.subscriptions {
                Some(subs) => {
                    let mut evidence = vec![subs.header()];
                    evidence.extend(subs.lines());
                    Contribution { text: Some(subs.text()), evidence, ..Contribution::default() }
                }
                None => Contribution::empty("no subscriptions known: this shell has not reported them (docs/elements.md, \"subscriptions\")"),
            },
            Element::Environment => match cx.environment {
                Some(env) => {
                    let line = env.line();
                    Contribution { text: Some(line.clone()), evidence: vec![line], ..Contribution::default() }
                }
                None => Contribution::empty("no environment known: this shell has not reported one (docs/container.md, \"The environment\")"),
            },
            Element::NoArtifacts => Contribution::text("None yet.".into()),
            Element::NoContract(Slot::Tools) => Contribution::empty("no response contract: the answer is free text"),
            Element::NoContract(_) => Contribution::empty("no response contract"),
            Element::Summary => match crate::context::summary(e) {
                Some(s) => Contribution::text(s),
                None => Contribution::empty("no summary: the history is not compressed"),
            },
            Element::Messages => {
                let messages = crate::context::history(e);
                if messages.is_empty() {
                    Contribution::empty("no messages yet")
                } else {
                    Contribution { messages, ..Contribution::default() }
                }
            }
            Element::Response => {
                if e.response.is_empty() {
                    Contribution::empty("no response contract: the answer is free text")
                } else {
                    Contribution::text(e.format.instructions(&e.response))
                }
            }
        }
    }
}

/// A plain artifact's section: its text, cut at `room` characters with a
/// note; named only when there is no room left.
fn plain(name: &str, text: &str, room: usize) -> String {
    let size = text.chars().count();
    if room == 0 {
        return format!("### {name} ({size} characters)\n\nNot shown (the artifacts are long); read(name=\"{name}\") for it.");
    }
    let shown: String = text.chars().take(room).collect();
    let note = if size > room {
        format!("\n… cut: {room} of {size} characters shown; read(name=\"{name}\") for all of it.")
    } else {
        String::new()
    };
    format!("### {name} ({size} characters)\n\n{}{note}", shown.trim_end())
}

/// A capability artifact's section: a kind by its snapshot, else its
/// compiled capabilities' docs; then each outside capability's part.
fn kind(name: &str, q: &Equipped, cx: &Cx) -> Contribution {
    let mut evidence = Vec::new();
    let mut section = match q.live {
        Some(kind) => {
            let snap = kind.snapshot(name, cx.engine, cx.outside);
            evidence.extend(snap.clone());
            let body = snap.unwrap_or_else(|| "(taken when the prompt is sent)".into());
            format!("### {name} — {}{}\n\n{body}", kind.meaning(), q.mode_note())
        }
        None => {
            let ids: Vec<&str> = q.compiled.iter().chain(&q.outside).map(|c| c.id.as_str()).collect();
            let lines: Vec<String> = q.compiled.iter().map(|c| format!("- {}: {}", c.id, c.doc)).collect();
            let head = format!("### {name} — capabilities: {}", ids.join(", "));
            if lines.is_empty() { head } else { format!("{head}\n\n{}", lines.join("\n")) }
        }
    };
    for c in &q.outside {
        section.push_str("\n\n");
        section.push_str(&capability::section(c));
        evidence.extend(c.snapshot.as_ref().map(|s| s.text.clone()).filter(|t| !t.trim().is_empty()));
    }
    Contribution { text: Some(section), evidence, ..Contribution::default() }
}

/// Sort elements into the prompt's order: place, then id.
pub fn sort(elements: &mut [Element]) {
    elements.sort_by_cached_key(|el| (el.place(), el.id()));
}

/// The artifacts section's elements, in order: each plain artifact (the
/// total budget shared out in name order), the skills, each capability
/// artifact (kinds, then capabilities only), the subscriptions and the
/// environment when the engine holds them, the clock last.
pub fn artifacts(cx: &Cx) -> Vec<Element> {
    let e = cx.engine;
    let mut out = Vec::new();
    let mut budget = ARTIFACTS_CHARS;
    for (name, text) in &e.artifacts {
        if capability::uses(text).is_some() {
            continue;
        }
        let room = ARTIFACT_CHARS.min(budget);
        budget -= text.chars().count().min(room);
        out.push(Element::Artifact { name: name.clone(), room });
    }
    if !e.visible_skills().is_empty() {
        out.push(Element::Skills);
    }
    out.extend(cx.equipped.iter().map(|q| Element::Kind { name: q.name.clone(), live: q.live }));
    if cx.held.iter().any(|h| h == "subscriptions") {
        out.push(Element::Subscriptions);
    }
    if cx.held.iter().any(|h| h == "environment") {
        out.push(Element::Environment);
    }
    sort(&mut out);
    out
}

/// Every element of engine `cx.engine`'s prompt, in order.
pub fn gather(cx: &Cx) -> Vec<Element> {
    let e = cx.engine;
    let mut out = vec![Element::Goal, Element::Learning, Element::Phase];
    // Tools and artifacts only mean something with a contract (its `act`).
    if e.response.is_empty() {
        out.extend([Element::NoContract(Slot::Tools), Element::NoContract(Slot::Artifacts)]);
    } else {
        out.extend([Element::Tools, Element::OutsideTools]);
        out.extend(e.mcp_tools.iter().enumerate().map(|(index, t)| Element::Mcp { index, name: t.name.clone() }));
        let artifacts = artifacts(cx);
        if artifacts.is_empty() {
            out.push(Element::NoArtifacts);
        } else {
            out.extend(artifacts);
        }
    }
    out.extend([Element::Summary, Element::Messages, Element::Response]);
    sort(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_is_place_then_id_whatever_order_they_are_gathered_in() {
        let mut a = vec![
            Element::Response,
            Element::Kind { name: "a-now".into(), live: Some(Live::Clock) },
            Element::Kind { name: "z".into(), live: Some(Live::Scripts) },
            Element::Artifact { name: "b".into(), room: 1 },
            Element::Skills,
            Element::Artifact { name: "a".into(), room: 1 },
            Element::Goal,
            Element::Kind { name: "caps".into(), live: None },
            Element::Environment,
            Element::Subscriptions,
        ];
        let mut b = a.clone();
        b.reverse();
        sort(&mut a);
        sort(&mut b);
        assert_eq!(a, b);
        let ids: Vec<String> = a.iter().map(Element::id).collect();
        assert_eq!(ids, ["goal", "artifact/a", "artifact/b", "skills", "artifact/z", "artifact/caps", "subscriptions", "environment", "artifact/a-now", "response"]);
    }

    #[test]
    fn elements_md_holds_built_ids_and_notes_the_rest() {
        let (ids, notes) = held("# what the head sees\nenvironment\n\n- environment\nsubscriptions\nmachines\nweather\n");
        assert_eq!(ids, ["environment", "subscriptions"]);
        assert_eq!(
            notes,
            [
                "`machines` is not built yet (docs/elements.md, phase 4): left out",
                "`weather` is not an element (known: subscriptions, environment): left out"
            ]
        );
        assert_eq!(held(""), (Vec::new(), Vec::new()));
    }
}
