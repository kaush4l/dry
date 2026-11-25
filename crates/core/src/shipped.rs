//! The shipped default (docs/default.md): what a fresh state starts with
//! and what the published app opens on. The files are `default/` at the
//! repo root, laid out as a store is (`app.md`, `engines/<id>/spec/…`,
//! `engines/<id>/artifacts/…`), bundled at build time (`build.rs`), so a
//! binary without the repo and the browser build start the same way.
//!
//! The rule: these files are the base layer under every store
//! (`crate::store::{merge, split}`): a store's own file at a path wins, a
//! path it does not hold comes from here. A native shell reads `default/`
//! from disk instead when it is there (`dry_tools::store::base`); the
//! browser uses these. A shipped engine has no model: the shell gives it
//! the machine's default (`with_model`).

use crate::inference::OpenAi;
use crate::state::State;
use crate::store::{Texts, from_texts};

include!(concat!(env!("OUT_DIR"), "/shipped.rs"));

/// The shipped files, by path under `default/`.
pub fn texts() -> Texts {
    FILES.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

/// The shipped default as a state.
pub fn state() -> Result<State, String> {
    from_texts(&texts()).map_err(|e| format!("the shipped default (default/) does not read: {e}"))
}

/// The shipped default with `ai` for every engine that names no model.
pub fn with_model(ai: &OpenAi) -> Result<State, String> {
    let mut s = state()?;
    for e in s.engines.iter_mut().filter(|e| e.session.is_none() && e.inference.model.is_empty() && e.inference.base_url.is_empty()) {
        e.inference = ai.clone();
    }
    Ok(s)
}

/// The shipped translator's id: what a new thread is made from.
pub const HEAD: &str = "translator";

/// A new thread: a new root engine in `state` made from the default in
/// `base` (its translator, else its first top-level engine: spec, skills,
/// templates, artifacts, contract, tools), with a fresh unused id
/// (`translator-2`), `ai` when it names no model, and no conversation.
/// Its index. The shell opens its thread with the composer on it.
pub fn new_thread(state: &mut State, base: &Texts, ai: &OpenAi) -> Result<usize, String> {
    let shipped = from_texts(base).map_err(|e| format!("the default does not read: {e}"))?;
    let mut e = shipped
        .engines
        .iter()
        .find(|e| e.id == HEAD)
        .or_else(|| shipped.engines.iter().find(|e| e.parent.is_none()))
        .cloned()
        .ok_or("the default has no top-level engine to make a thread from")?;
    let taken: Vec<&str> = state.engines.iter().map(|e| e.id.as_str()).collect();
    let id = crate::files::new_id(&e.id, &taken);
    if let Some(n) = id.strip_prefix(&format!("{}-", e.id)) {
        e.name = format!("{} {n}", e.name);
    }
    e.id = id;
    e.parent = None;
    e.messages.clear();
    e.waiting.clear();
    e.heard = Some(crate::snapshot::Cursor::default());
    e.machine = state.capabilities.clone();
    if e.session.is_none() && e.inference.model.is_empty() && e.inference.base_url.is_empty() {
        e.inference = ai.clone();
    }
    state.engines.push(e);
    Ok(state.engines.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_thread_is_a_fresh_translator_at_the_top() {
        let ai = OpenAi { model: "m".into(), base_url: "http://x/v1".into(), ..OpenAi::default() };
        let mut s = with_model(&ai).unwrap();
        s.add_text(0, "hello");
        let i = new_thread(&mut s, &texts(), &ai).unwrap();
        let (t, e) = (&s.engines[0], &s.engines[i]);
        assert_eq!((e.id.as_str(), e.name.as_str(), e.parent.as_deref()), ("translator-2", "Translator 2", None));
        assert_eq!((&e.description, &e.skills, &e.artifacts, &e.response, &e.tools), (&t.description, &t.skills, &t.artifacts, &t.response, &t.tools));
        assert_eq!((e.messages.len(), e.inference.model.as_str()), (0, "m"));
        assert!(crate::shared::is_root(&s, &e.id));
        let j = new_thread(&mut s, &texts(), &ai).unwrap();
        assert_eq!(s.engines[j].id, "translator-3");
    }

    #[test]
    fn the_default_reads_with_a_translator_that_holds_no_action_tools() {
        let s = state().unwrap();
        let t = &s.engines[0];
        assert_eq!((t.id.as_str(), t.parent.as_deref()), ("translator", None));
        assert_eq!(t.tools, ["reply", "write"], "the translator acts only through children and replies; `write` keeps its plan and summary");
        let kinds: Vec<_> = crate::live::present(t).into_iter().map(|(_, k)| k).collect();
        assert!(kinds.contains(&crate::live::Live::Engines), "it holds an engines artifact");
        for k in [crate::live::Live::File, crate::live::Live::Terminal, crate::live::Live::Browser] {
            assert!(!kinds.contains(&k), "no {k:?} on the translator");
        }
        assert!(t.skills.contains_key("prompting"));
        for (n, text) in &t.skills {
            crate::skill::parse(text).unwrap_or_else(|e| panic!("skill {n}: {e}"));
        }
        assert!(!t.response.is_empty(), "it answers with the contract");
        // The goal is in its three sections.
        assert!(crate::goal::parse(&t.description).notes.is_empty(), "{:?}", crate::goal::parse(&t.description).notes);
        // It holds the subscriptions and the environment (spec/elements.md), and reads them without a note.
        assert_eq!(crate::element::held(&t.elements), (vec!["subscriptions".to_string(), "environment".to_string()], Vec::new()));
    }

    #[test]
    fn the_developer_template_is_whole() {
        let s = state().unwrap();
        let kits = crate::kit::kits(&s.engines[0]);
        let dev = kits.get("developer").expect("a developer template");
        assert_eq!(dev.problem(), None);
        assert_eq!(dev.phases(), ["triage", "code", "test"]);
        for skill in ["triage", "plan", "code-style", "test", "browser-qa", "critique", "debugging", "python"] {
            assert!(dev.skills.contains_key(skill), "developer skill {skill}");
        }
        // A skill may bring tools of its own: python's read, and are named after it.
        let python = crate::skilltool::parse("python", &dev.skill_tools["python"]).expect("python's tools.md reads");
        assert_eq!(python.tools.iter().map(|t| t.tool.name.as_str()).collect::<Vec<_>>(), ["check", "run", "test"]);
        // Every phase's skills are the template's own.
        for p in crate::template::parse(&dev.template).unwrap().phases().unwrap() {
            for name in p.skills.unwrap_or_default() {
                assert!(dev.skills.contains_key(&name), "phase {} names skill {name}, which the template lacks", p.name);
            }
        }
        let with = dev.artifacts_at(Some("/tmp/project"));
        assert!(with["code"].contains("root: /tmp/project") && with["terminal"].contains("root: /tmp/project"));
        assert!(dev.tools.as_ref().unwrap().contains(&"write".to_string()), "the developer writes its notes");
        assert!(dev.artifacts["plan"].contains("- [ ]"), "the plan keeps its steps as a to-do list");
    }

    #[test]
    fn the_answer_template_is_whole_and_holds_only_reply() {
        let s = state().unwrap();
        let kits = crate::kit::kits(&s.engines[0]);
        let k = kits.get("answer").expect("an answer template");
        assert_eq!(k.problem(), None);
        assert_eq!(k.phases(), ["answer"]);
        assert_eq!(k.tools.clone().unwrap_or_default(), ["reply"], "no files, no terminal");
        let skill = &k.skills["components"];
        // The skill names every registered component, so none is missing from it.
        let text = format!("{skill:?}");
        for kind in crate::show::registry().iter().map(|e| e.show.name().to_string()) {
            assert!(text.contains(&format!("show=\\\"{kind}\\\"")), "the skill shows show={kind}");
        }
        for kind in crate::ask::registry().iter().map(|e| e.ask.name().to_string()) {
            assert!(text.contains(&format!("ask=\\\"{kind}\\\"")), "the skill shows ask={kind}");
        }
        assert!(s.engines[0].description.contains("to `answer`"), "the translator sends plain questions to it");
    }

    #[test]
    fn the_reviewer_and_researcher_templates_are_whole() {
        let s = state().unwrap();
        let kits = crate::kit::kits(&s.engines[0]);
        for (name, phases, skill) in [("reviewer", ["check", "verdict"], "review"), ("researcher", ["explore", "answer"], "research")] {
            let k = kits.get(name).unwrap_or_else(|| panic!("a {name} template"));
            assert_eq!(k.problem(), None, "{name}");
            assert_eq!(k.phases(), phases, "{name}");
            assert!(k.skills.contains_key(skill), "{name} skill {skill}");
            assert!(!k.artifacts.contains_key("plan"), "{name} plans nothing");
            let with = k.artifacts_at(Some("/tmp/project"));
            assert!(with["code"].contains("root: /tmp/project") && with["terminal"].contains("root: /tmp/project"), "{name}");
            // Read-only by its artifacts, not only by its prompt: no tool
            // that writes a file is offered, and its terminal refuses the
            // commands known to write.
            assert!(with["code"].starts_with("uses: [file:read]") && with["terminal"].starts_with("uses: [terminal:read]"), "{name}");
            let mut e = crate::state::Engine::new(name, "Review.", OpenAi::default());
            e.tools = k.tools.clone().unwrap_or_default();
            e.artifacts = with;
            e.skills = k.skills.clone();
            let tools = crate::live::toolbox(&e);
            for t in ["open", "focus", "run", "read", "list", "reply"] {
                assert!(tools.contains(&t.to_string()), "{name} offers {t}: {tools:?}");
            }
            for t in crate::file::WRITES {
                assert!(!tools.contains(&t.to_string()), "{name} offers {t}: {tools:?}");
            }
            // `write` saves the engine's own artifacts (its findings), never a
            // file or the terminal's state.
            let mut arts = e.artifacts.clone();
            let w = crate::calls::Call {
                name: "write".into(),
                args: vec![("name".into(), "code".into()), ("text".into(), "kind: file\nroot: /".into())],
            };
            assert!(crate::tools::run(&w, &tools, &mut arts).unwrap_err().contains("changes only through its tools"), "{name}");
        }
    }

    #[test]
    fn every_engine_gets_the_model_and_the_texts_round_trip() {
        let ai = OpenAi { model: "m".into(), base_url: "cli:claude".into(), ..OpenAi::default() };
        let s = with_model(&ai).unwrap();
        assert!(s.engines.iter().all(|e| e.inference == ai));
        assert_eq!(from_texts(&crate::store::to_texts(&s)).unwrap(), s, "the default survives a store");
    }
}
