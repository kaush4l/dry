//! An engine as files: what is sharable apart from what happened.
//!
//! ```text
//! <engine id>/
//!   spec/            what the engine is; copy it to share the engine
//!     engine.md        name, kind, model, format (`key: value` lines)
//!     goal.md          its goal, in sections (`crate::goal`); an old
//!                      `instruction.md` is read while there is none
//!     responses.md     its response contract (if it has one)
//!     tools.md         its tools, one `- name` line each
//!     skills/<name>/SKILL.md  its skills (`crate::skill`)
//!     skills/<name>/tools.md  the tools a skill brings of its own (`crate::skilltool`)
//!     template.md      how its prompt is laid out, and its phases (`crate::template`)
//!     templates/<name>/…  templates it starts children from (`crate::kit`)
//!     elements.md      extra elements it holds, one id per line (`crate::element::held`)
//!     mcp.json         MCP servers whose tools it gets (if any)
//!     scripts/<name>.md  its scripts (`crate::script`)
//!     alerts/<name>.md   its alerts (`crate::alert`)
//!   convo/           what happened
//!     messages.md      the history, one `### role` block per message
//!     meta.json        a session's program id and everything it reported
//!     run.md           where its run is: phase, step, skills loaded, child
//!                      engines it waits for (`waiting:`)
//!   memory/          local: what it learned (`crate::learning`)
//!     learning.md      three sections, capped
//!     learning.log     one line per change
//!   artifacts/       its working material, one `<name>.md` each
//! ```
//!
//! Which engine made which (`crate::engines`) is in `app.md`, the list of
//! engines: `- helper (child of engine-1)`. Every engine folder is at the
//! top of the store, a child's too; the shared context of a level is
//! `shared.md` in its parent's folder, and the top level's beside `app.md`
//! (`crate::shared`).
//!
//! Pure: these functions turn an engine into texts and back. A store (a
//! folder of files, or a database holding the same texts) does the reading
//! and writing, so every store yields the same `State`.

use std::collections::BTreeMap;

use crate::inference::{Api, Message, OpenAi, Role};
use crate::response::{Format, ResponseFormat, Responses};
use crate::session::SessionRef;
use crate::state::{Engine, State};
use crate::theme::Mode;

/// The three parts of an engine folder.
pub const SPEC: &str = "spec";
pub const CONVO: &str = "convo";
pub const ARTIFACTS: &str = "artifacts";
/// Local and personal: what the engine learned. Never in `spec/`.
pub const MEMORY: &str = "memory";
/// A root's thread file (`crate::shared`).
pub const SHARED: &str = "shared.md";

/// The folders inside `spec/`, one file per item. In `spec_files` their
/// files are named `<folder>/<name>.md`.
pub const SCRIPTS: &str = "scripts";
pub const ALERTS: &str = "alerts";
pub const SPEC_FOLDERS: [&str; 2] = [SCRIPTS, ALERTS];

/// The engine's spec files, `(name, text)`: everything needed to make the
/// same engine elsewhere, and nothing it did.
pub fn spec_files(e: &Engine) -> Vec<(String, String)> {
    let ai = &e.inference;
    let mut md = format!("# {}\n\nkind: {}\n", e.name, e.kind());
    let context = ai.context.map(|c| c.to_string()).unwrap_or_default();
    for (k, v) in [
        ("api", ai.api.as_str()),
        ("base_url", &ai.base_url),
        ("model", &ai.model),
        ("key_env", ai.key_env.as_deref().unwrap_or("")),
        ("context", &context),
    ] {
        if !v.is_empty() {
            md.push_str(&format!("{k}: {v}\n"));
        }
    }
    md.push_str(&format!("format: {}\n", e.format.name()));
    if !e.allow.trim().is_empty() {
        md.push_str(&format!("allow: {}\n", e.allow.trim()));
    }
    if let Some(t) = e.approve_timeout {
        md.push_str(&format!("approve_timeout: {t}\n"));
    }
    // Sampling settings (`crate::sampling`): only the set ones.
    md.push_str(&ai.sampling.to_lines());
    let mut files = vec![("engine.md".to_string(), md), (crate::goal::FILE.to_string(), format!("{}\n", e.description.trim_end()))];
    if !e.response.is_empty() {
        files.push(("responses.md".into(), e.response.to_md(&e.format)));
    }
    files.push(("tools.md".into(), e.tools.iter().map(|t| format!("- {t}\n")).collect()));
    if !e.mcp.trim().is_empty() {
        files.push(("mcp.json".into(), e.mcp.clone()));
    }
    if !e.template.trim().is_empty() {
        files.push(("template.md".into(), e.template.clone()));
    }
    if !e.elements.trim().is_empty() {
        files.push((crate::element::FILE.into(), e.elements.clone()));
    }
    for (folder, items) in [(SCRIPTS, &e.scripts), (ALERTS, &e.alerts)] {
        files.extend(items.iter().map(|(name, text)| (format!("{folder}/{name}.md"), text.clone())));
    }
    files.extend(e.skills.iter().map(|(name, text)| (skill_file(name), text.clone())));
    files.extend(e.skill_tools.iter().map(|(name, text)| (crate::skilltool::file(name), text.clone())));
    files.extend(e.templates.iter().map(|(path, text)| (format!("{}{path}", crate::kit::FOLDER), text.clone())));
    files
}

/// Where skill `name` is kept in `spec/`: its own folder, as skills are.
pub fn skill_file(name: &str) -> String {
    format!("skills/{name}/SKILL.md")
}

/// Read spec files back onto `e` (any missing one leaves its default).
pub fn apply_spec(e: &mut Engine, files: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(md) = files.get("engine.md") {
        let mut ai = OpenAi::default();
        for line in md.lines() {
            if let Some(name) = line.strip_prefix("# ") {
                e.name = name.trim().to_string();
            }
            let Some((k, v)) = line.split_once(':') else { continue };
            let v = v.trim();
            match k.trim() {
                "kind" if v != "model" => e.session = Some(SessionRef { tool: v.to_string(), id: e.session.take().and_then(|s| s.id) }),
                "api" => ai.api = Api::parse(v).ok_or_else(|| format!("engine.md: unknown api {v}"))?,
                "base_url" => ai.base_url = v.to_string(),
                "model" => ai.model = v.to_string(),
                "key_env" => ai.key_env = (!v.is_empty()).then(|| v.to_string()),
                "context" => ai.context = Some(v.parse().map_err(|_| format!("engine.md: context {v} is not a number of tokens"))?),
                "format" => e.format = Format::parse_name(v).ok_or_else(|| format!("engine.md: unknown format {v}"))?,
                "allow" => e.allow = v.to_string(),
                "approve_timeout" => {
                    e.approve_timeout = Some(v.parse().map_err(|_| format!("engine.md: approve_timeout {v} is not a number of seconds"))?)
                        .filter(|s| *s > 0)
                }
                k if crate::sampling::owns(k) => {
                    ai.sampling.set_text(k, v).map_err(|why| format!("engine.md: {why}"))?;
                }
                _ => {}
            }
        }
        e.inference = ai;
    }
    // The goal: goal.md, else the old instruction.md (said in the prompt
    // view; the next save writes goal.md).
    match (files.get(crate::goal::FILE), files.get(crate::goal::OLD_FILE)) {
        (Some(text), _) => e.description = text.trim().to_string(),
        (None, Some(text)) => {
            e.description = text.trim().to_string();
            e.legacy_goal = true;
        }
        (None, None) => {}
    }
    if let Some(md) = files.get("responses.md") {
        e.response = Responses::from_md(md)?.0;
    }
    if let Some(md) = files.get("tools.md") {
        e.tools = md.lines().filter_map(|l| l.trim().strip_prefix("- ")).map(|t| t.trim().to_string()).collect();
    }
    if let Some(json) = files.get("mcp.json") {
        e.mcp = json.clone();
    }
    if let Some(md) = files.get("template.md") {
        e.template = md.clone();
    }
    if let Some(md) = files.get(crate::element::FILE) {
        e.elements = md.clone();
    }
    for (file, text) in files {
        if let Some(path) = file.strip_prefix(crate::kit::FOLDER) {
            e.templates.insert(path.to_string(), text.clone());
            continue;
        }
        if let Some(name) = file.strip_prefix("skills/").and_then(|f| f.strip_suffix("/SKILL.md")) {
            e.skills.insert(name.to_string(), text.clone());
            continue;
        }
        if let Some(name) = file.strip_prefix("skills/").and_then(|f| f.strip_suffix(&format!("/{}", crate::skilltool::FILE))) {
            e.skill_tools.insert(name.to_string(), text.clone());
            continue;
        }
        let Some((folder, name)) = file.strip_suffix(".md").and_then(|f| f.split_once('/')) else { continue };
        match folder {
            SCRIPTS => e.scripts.insert(name.to_string(), text.clone()),
            ALERTS => e.alerts.insert(name.to_string(), text.clone()),
            _ => None,
        };
    }
    Ok(())
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tools",
        Role::Summary => "summary",
    }
}

/// The history as markdown: one `### role` heading per message, then its
/// text as it is. A text line that itself starts with `### ` gets a `\`.
pub fn messages_md(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        out.push_str(&format!("### {}\n\n", role_name(m.role)));
        for line in m.text.lines() {
            if line.starts_with("### ") || line.starts_with("\\### ") {
                out.push('\\');
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// Read `messages_md` back.
pub fn parse_messages(md: &str) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    for line in md.lines() {
        let role = match line {
            "### user" => Some(Role::User),
            "### assistant" => Some(Role::Assistant),
            "### tools" => Some(Role::Tool),
            "### system" => Some(Role::System),
            "### summary" => Some(Role::Summary),
            _ => None,
        };
        match (role, out.last_mut()) {
            (Some(role), _) => out.push(Message { role, text: String::new() }),
            (None, Some(m)) => {
                let line = line.strip_prefix('\\').filter(|l| l.starts_with("### ")).unwrap_or(line);
                m.text.push_str(line);
                m.text.push('\n');
            }
            (None, None) => {}
        }
    }
    for m in &mut out {
        m.text = m.text.trim().to_string();
    }
    out
}

/// `convo/meta.json`: a session's program id and everything it reported.
pub fn meta_json(e: &Engine) -> String {
    let id = e.session.as_ref().and_then(|s| s.id.clone());
    serde_json::json!({ "session_id": id, "meta": e.meta }).to_string()
}

pub fn apply_meta(e: &mut Engine, json: &str) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("meta.json: {e}"))?;
    if let (Some(s), Some(id)) = (e.session.as_mut(), v.get("session_id").and_then(|i| i.as_str())) {
        s.id = Some(id.to_string());
    }
    if let Some(meta) = v.get("meta").and_then(|m| m.as_object()) {
        e.meta = meta.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
    }
    Ok(())
}

/// `app.md`: the app's settings (the mode; the scene and the motion
/// when set) and the engines' ids, in order; a child engine says which
/// made it: `- helper (child of engine-1)`.
pub fn app_md(state: &State) -> String {
    let mut out = format!("mode: {}\n", state.mode.as_str());
    if let Some(scene) = &state.scene {
        out.push_str(&format!("scene: {scene}\n"));
    }
    if let Some(motion) = state.motion {
        out.push_str(&format!("motion: {motion}\n"));
    }
    out.push_str("\nengines:\n");
    for e in &state.engines {
        match &e.parent {
            Some(p) => out.push_str(&format!("- {} (child of {p})\n", e.id)),
            None => out.push_str(&format!("- {}\n", e.id)),
        }
    }
    out
}

/// Read `app_md`: the mode and the engines in order, each id with its
/// parent's.
pub fn parse_app(md: &str) -> (Mode, Vec<(String, Option<String>)>) {
    let mode = md.lines().find_map(|l| l.strip_prefix("mode:")).and_then(|m| Mode::parse(m.trim())).unwrap_or_default();
    let ids = md
        .lines()
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| match l.trim().strip_suffix(')').and_then(|l| l.split_once(" (child of ")) {
            Some((id, parent)) => (id.trim().to_string(), Some(parent.trim().to_string())),
            None => (l.trim().to_string(), None),
        })
        .collect();
    (mode, ids)
}

/// `app_md`'s scene and motion settings, each None when it does not say
/// (or says something that is not a scene name or a number of seconds).
pub fn parse_settings(md: &str) -> (Option<String>, Option<f32>) {
    let value = |key: &str| md.lines().take_while(|l| l.trim() != "engines:").find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(str::trim));
    let scene = value("scene").filter(|s| crate::theme::valid_scene_name(s)).map(str::to_string);
    let motion = value("motion").and_then(|m| m.parse::<f32>().ok()).filter(|m| *m >= 0.0);
    (scene, motion)
}

/// A folder name for `name`: lower case letters, digits and `-`, not one
/// of `taken`.
pub fn new_id(name: &str, taken: &[&str]) -> String {
    let mut base: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    base = base.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if base.is_empty() {
        base = "engine".into();
    }
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id.as_str()) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// Everything about one engine a store keeps, apart from its id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineFiles {
    pub spec: BTreeMap<String, String>,
    pub messages: String,
    pub meta: String,
    /// `convo/run.md`: where the run is. Empty: at the start.
    pub run: String,
    pub artifacts: BTreeMap<String, String>,
    /// `memory/learning.md` and `memory/learning.log`; empty: none.
    pub learning: String,
    pub learning_log: String,
}

/// An engine as its files.
pub fn to_files(e: &Engine) -> EngineFiles {
    EngineFiles {
        spec: spec_files(e).into_iter().collect(),
        messages: messages_md(&e.messages),
        meta: meta_json(e),
        run: run_md(e),
        artifacts: e.artifacts.clone(),
        learning: e.learning.clone(),
        learning_log: e.learning_log.clone(),
    }
}

/// `convo/run.md`: where the run is, and the skills it has loaded.
fn run_md(e: &Engine) -> String {
    let mut md = e.run.to_md(&e.phase_list().unwrap_or_default());
    if !e.loaded_skills.is_empty() {
        md.push_str(&format!("skills: {}\n", e.loaded_skills.join(", ")));
    }
    if !e.waiting.is_empty() {
        md.push_str(&format!("waiting: {}\n", e.waiting.join(", ")));
    }
    if let Some(h) = e.heard.filter(|h| *h != crate::snapshot::Cursor::default() || !e.messages.is_empty()) {
        md.push_str(&format!("heard: {}\n", h.text()));
    }
    // One line per alert: when it was first seen and last fired (`crate::alert::Mark`).
    for (name, mark) in &e.alert_marks {
        md.push_str(&format!("alert: {name} {}\n", mark.to_line()));
    }
    md
}

/// A `key: a, b` line of `run.md` as its list.
fn run_list(run: &str, key: &str) -> Vec<String> {
    run.lines()
        .find_map(|l| l.strip_prefix(key))
        .map_or(Vec::new(), |l| l.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// An engine back from its files (and its folder name).
pub fn from_files(id: &str, f: &EngineFiles) -> Result<Engine, String> {
    let mut e = Engine::new(id, "", OpenAi::default());
    e.id = id.to_string();
    apply_spec(&mut e, &f.spec).map_err(|why| format!("{id}/spec: {why}"))?;
    e.messages = parse_messages(&f.messages);
    if !f.meta.trim().is_empty() {
        apply_meta(&mut e, &f.meta).map_err(|why| format!("{id}/convo: {why}"))?;
    }
    e.run = crate::phase::Run::from_md(&f.run, &e.phase_list().unwrap_or_default());
    e.loaded_skills = run_list(&f.run, "skills:");
    e.waiting = run_list(&f.run, "waiting:");
    e.alert_marks = f
        .run
        .lines()
        .filter_map(|l| l.strip_prefix("alert: "))
        .filter_map(|l| l.split_once(' ').and_then(|(name, rest)| Some((name.to_string(), crate::alert::Mark::from_line(rest)?))))
        .collect();
    // No line: a new engine (nothing heard) if it has no messages yet,
    // else a store from before the guided rule (`crate::desk::guided`).
    // A cursor from before threads (`3.5`) counted other files: dropped,
    // so what its thread has counts as heard.
    e.heard = match f.run.lines().find_map(|l| l.strip_prefix("heard:")) {
        Some(c) if crate::snapshot::Cursor::is_old(c) => None,
        Some(c) => Some(crate::snapshot::Cursor::parse(c)),
        None if e.messages.is_empty() => Some(crate::snapshot::Cursor::default()),
        None => None,
    };
    e.artifacts = f.artifacts.clone();
    e.learning = f.learning.clone();
    e.learning_log = f.learning_log.clone();
    Ok(e)
}

/// A state with every part of an engine filled, for tests of any store.
pub fn sample() -> State {
    let mut state = State { mode: Mode::Light, ..State::default() };
    let a = state.add_engine();
    let e = &mut state.engines[a];
    e.description = "## Goal\nBe brief.\n\n## Scope\nTwo lines of instructions.".into();
    e.inference = OpenAi {
        api: Api::Responses,
        base_url: "https://x/v1".into(),
        model: "m".into(),
        key_env: Some("K".into()),
        context: Some(32_768),
        sampling: Default::default(),
    };
    e.inference.sampling.set_text("temperature", "0.2").unwrap();
    e.inference.sampling.set_text("stop", r#"["END", "a: b\n"]"#).unwrap();
    e.inference.sampling.set_text("param.min_p", "0.05").unwrap();
    e.response = Responses::from_md("format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n").unwrap().0;
    e.format = Format::parse_name("json").unwrap();
    e.mcp = "{\"mcpServers\":{}}".into();
    e.template = "| phase |\n|---|\n| plan |\n| do |\n\n## phase: plan\nPlan it.\n\n## phase: do\nDo it.\n".into();
    e.run = crate::phase::Run { phase: 1, step: 2 };
    e.elements = "environment\n".into();
    e.skills.insert("review".into(), "---\nname: review\ndescription: Review it.\n---\nLook closely.\n".into());
    e.loaded_skills = vec!["review".into()];
    e.templates.insert("dev/template.md".into(), "| phase |\n|---|\n| a |\n| b |\n\n## phase: a\nA.\n\n## phase: b\nB.\n".into());
    e.templates.insert("dev/skills/look/SKILL.md".into(), "---\nname: look\ndescription: Look.\n---\nLook.\n".into());
    e.messages = vec![
        Message { role: Role::User, text: "hi\n### not a heading".into() },
        Message { role: Role::Assistant, text: "thought: x\nact: reply(text=\"y\")".into() },
        Message { role: Role::Tool, text: "list() → none".into() },
        Message { role: Role::Summary, text: "Earlier: the user said hi.".into() },
        Message { role: Role::User, text: "next".into() },
    ];
    e.artifacts.insert("plan".into(), "1. a\n2. b".into());
    e.scripts.insert(
        "note".into(),
        "---\nname: note\ndescription: Save and list.\nparams: text\n---\n- write(name=\"n\", text=\"{text}\")\n- list()\n".into(),
    );
    e.alerts.insert("wake".into(), "---\nname: wake\ndescription: Morning\ntrigger: at 07:00\n---\nGood morning.\n".into());
    e.learning = "## user\n- Prefers short answers.\n".into();
    e.learning_log = "1795356330 learn user add: Prefers short answers.\n".into();
    e.artifacts.insert("kids".into(), "kind: engines\nchild: helper start 1\n".into());
    let b = state.add_session("claude", "Claude Code");
    state.engines[b].session.as_mut().unwrap().id = Some("abc".into());
    state.engines[b].meta.insert("usage.output_tokens".into(), "4".into());
    // A child of the first engine, waiting on nothing, on its thread.
    let c = state.add_engine();
    let child = &mut state.engines[c];
    child.id = "helper".into();
    child.name = "helper".into();
    child.description = "## Goal\nFind it.".into();
    child.parent = Some("engine-1".into());
    child.artifacts.insert("shared".into(), "kind: shared".into());
    state.engines[a].waiting = vec!["helper".into()];
    state.shared.insert("engine-1".into(), "- 1795356330 helper: Found it in turn.rs.\n".into());
    let other = state.engines[b].id.clone();
    state.shared.insert(other, "- 1795356300 you → engine-1: A note on the session's thread.\n".into());
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_engine_goes_to_files_and_back_the_same() {
        let state = sample();
        for e in &state.engines {
            // The parent is in app.md, not in the engine's own files.
            let mut back = from_files(&e.id, &to_files(e)).unwrap();
            back.parent = e.parent.clone();
            assert_eq!(&back, e, "{}", e.id);
        }
        let (mode, ids) = parse_app(&app_md(&state));
        assert_eq!(mode, Mode::Light);
        assert_eq!(parse_settings(&app_md(&state)), (None, None), "no scene line until one is set");
        let mut set = state.clone();
        (set.scene, set.motion) = (Some("none".into()), Some(0.0));
        assert_eq!(parse_settings(&app_md(&set)), (Some("none".into()), Some(0.0)));
        assert!(app_md(&set).starts_with("mode: light\nscene: none\nmotion: 0\n\nengines:\n"), "{}", app_md(&set));
        assert_eq!(parse_settings("scene: ../x\nmotion: fast\n"), (None, None));
        let ids: Vec<(&str, Option<&str>)> = ids.iter().map(|(i, p)| (i.as_str(), p.as_deref())).collect();
        assert_eq!(ids, [("engine-1", None), ("claude-2", None), ("helper", Some("engine-1"))]);
        assert!(app_md(&state).contains("- helper (child of engine-1)\n"));
    }

    #[test]
    fn the_spec_holds_nothing_that_happened() {
        let state = sample();
        let spec: String = spec_files(&state.engines[0]).into_iter().map(|(_, t)| t).collect();
        assert!(!spec.contains("hi") && !spec.contains("1. a"), "{spec}");
        assert!(spec.contains("kind: model") && spec.contains("| act |") && spec.contains("- write"));
        let names: Vec<String> = spec_files(&state.engines[0]).into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"scripts/note.md".to_string()) && names.contains(&"alerts/wake.md".to_string()), "{names:?}");
    }

    #[test]
    fn sampling_settings_are_engine_md_lines_and_a_bad_one_is_an_error() {
        let state = sample();
        let md = &spec_files(&state.engines[0])[0].1;
        assert!(
            md.contains("temperature: 0.2\n") && md.contains("param.min_p: 0.05\n") && md.contains(r#"stop: ["END","a: b\n"]"#),
            "{md}"
        );
        assert!(!spec_files(&state.engines[1])[0].1.contains("temperature"), "none set: no lines");
        let mut e = state.engines[0].clone();
        let bad: BTreeMap<String, String> = [("engine.md".to_string(), "# x\n\ntemperature: 9\n".to_string())].into();
        assert!(apply_spec(&mut e, &bad).unwrap_err().contains("temperature"));
    }

    #[test]
    fn goal_md_is_written_and_an_old_instruction_md_is_read_while_there_is_none() {
        let e = &sample().engines[0];
        let spec: BTreeMap<String, String> = spec_files(e).into_iter().collect();
        assert!(spec.contains_key("goal.md") && !spec.contains_key("instruction.md"), "{:?}", spec.keys());
        let old: BTreeMap<String, String> = [("instruction.md".to_string(), "Be brief.\n".to_string())].into();
        let mut fresh = Engine::new("x", "", OpenAi::default());
        apply_spec(&mut fresh, &old).unwrap();
        assert_eq!((fresh.description.as_str(), fresh.legacy_goal), ("Be brief.", true));
        let both: BTreeMap<String, String> =
            [("instruction.md".to_string(), "old".to_string()), ("goal.md".to_string(), "## Goal\nnew".to_string())].into();
        let mut fresh = Engine::new("x", "", OpenAi::default());
        apply_spec(&mut fresh, &both).unwrap();
        assert_eq!((fresh.description.as_str(), fresh.legacy_goal), ("## Goal\nnew", false), "goal.md wins");
    }

    #[test]
    fn ids_are_folder_names_and_never_repeat() {
        assert_eq!(new_id("Engine 1", &[]), "engine-1");
        assert_eq!(new_id("claude 2", &["claude-2"]), "claude-2-2");
        assert_eq!(new_id("!!", &[]), "engine");
    }
}
