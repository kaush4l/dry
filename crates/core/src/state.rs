//! The application state: everything the app knows, in one value. Saving
//! the app means saving this; starting the app means loading it.
//!
//! For now it holds a growing list of engines. An engine is a name, a
//! description, the inference it talks to, and its messages: what you typed
//! and what the model replied, in order.

use std::collections::BTreeMap;

use crate::inference::{Api, Message, OpenAi, Role};
use crate::response::{Field, Format, ResponseFormat, Responses};
use crate::session::SessionRef;
use crate::theme::Mode;

/// The engine prototype: what every engine is made of. More properties
/// come later; each one also gets a line in the file (`to_text`).
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    /// Its folder name in a store (`engine-1`): set once when it is made,
    /// so renaming the engine keeps its files.
    pub id: String,
    pub name: String,
    /// Its goal: `spec/goal.md` as written (`crate::goal` reads its
    /// sections). The Goal section's first line is its one line on a card.
    pub description: String,
    /// Its goal was read from the old `spec/instruction.md` (there was no
    /// `goal.md`); the next save writes `goal.md`. Said in the prompt view.
    pub legacy_goal: bool,
    /// The engine that made it (`crate::engines`), by id; `None` for a
    /// top-level engine. Engines with the same parent are one level and
    /// share one context (`crate::shared`).
    pub parent: Option<String>,
    /// The child engines whose reply this turn waits for (`engine_call`),
    /// by id. Kept in `convo/run.md`.
    pub waiting: Vec<String>,
    /// `memory/learning.md`: what it learned (`crate::learning`). Local,
    /// never in `spec/`.
    pub learning: String,
    /// `memory/learning.log`: one line per change to the learning file.
    pub learning_log: String,
    /// The learning file as it was when this turn's first `learn` changed
    /// it: the Learning slot sends this copy until your next message, so
    /// the first slots stay the same text within a turn. Not saved.
    pub learning_turn: Option<String>,
    /// The object this engine sends its requests through. It implements
    /// `crate::inference::Inference`; the app gives a new engine the
    /// default model from `models.json`.
    pub inference: OpenAi,
    /// The conversation, oldest first: `Role::User` for what you typed,
    /// `Role::Assistant` for the model's reply. All of it is sent to the model.
    pub messages: Vec<Message>,
    /// Set when this engine is a session: an agent program from
    /// `sessions.json` (claude, codex, ...) answers instead of `inference`,
    /// and keeps the conversation itself under its own session id.
    pub session: Option<SessionRef>,
    /// Everything the session's program reported on its last turn (model,
    /// tokens, cost, timings, ...), `dotted.key -> value`. Empty for a
    /// model engine.
    pub meta: BTreeMap<String, String>,
    /// The response contract: what each of its model's answers must
    /// contain. Set when the engine is made (from `responses.md`) and
    /// changeable while it runs. Empty: free-text answers.
    pub response: Responses,
    /// How its answers are written (`json`, `toon`, ...).
    pub format: Format,
    /// The tools its calls may use, by name (`crate::tools`). A new
    /// engine holds every built-in one.
    pub tools: Vec<String>,
    /// Its working material, by name: what `write` saves and `read` reads.
    pub artifacts: BTreeMap<String, String>,
    /// Its scripts (`spec/scripts/<name>.md`), name → the file's text as
    /// written. Read when used (`crate::script`), so a broken file shows
    /// as broken instead of stopping the app from loading.
    pub scripts: BTreeMap<String, String>,
    /// Its alerts (`spec/alerts/<name>.md`), name → the file's text, read
    /// when used (`crate::alert`) like the scripts.
    pub alerts: BTreeMap<String, String>,
    /// When each alert was first seen and last fired (`crate::alert::Mark`).
    /// Kept in `convo/run.md`.
    pub alert_marks: BTreeMap<String, crate::alert::Mark>,
    /// Its skills (`spec/skills/<name>/SKILL.md`, `crate::skill`), name →
    /// the file's text, read when used.
    pub skills: BTreeMap<String, String>,
    /// The tools its skills bring of their own (`spec/skills/<name>/tools.md`,
    /// `crate::skilltool`), skill name → the file's text, read when used.
    pub skill_tools: BTreeMap<String, String>,
    /// The skills loaded in this run (their tools are its tools until your
    /// next message). Kept in `convo/run.md`.
    pub loaded_skills: Vec<String>,
    /// `template.md` as written in its spec (`crate::template`): how its
    /// prompt is laid out, and its phases. Empty: the default template.
    pub template: String,
    /// The templates it can start children from (`spec/templates/<name>/…`,
    /// `crate::kit`): path under `templates/` → the file's text.
    pub templates: BTreeMap<String, String>,
    /// `elements.md` as written in its spec (`crate::element::held`): the
    /// extra elements it holds beyond its artifacts and tools, one id per
    /// line (`environment`). Empty: none.
    pub elements: String,
    /// Where its run is (phase, steps in it), kept in `convo/run.md`.
    pub run: crate::phase::Run,
    /// `mcp.json` as written in its spec: MCP servers whose tools it gets
    /// when it starts. Empty: none.
    pub mcp: String,
    /// The tools its MCP servers offered when they started (`server.tool`).
    /// Not saved: found again each time the engine starts.
    pub mcp_tools: Vec<crate::mcp::Tool>,
    /// What it may use without asking a person (`crate::approval`): the
    /// `allow:` rules in its `engine.md`, as written.
    pub allow: String,
    /// Seconds an approval waits before it is denied (`approve_timeout:`);
    /// `None`: it waits for a person.
    pub approve_timeout: Option<u64>,
    /// What this machine offers (`crate::capability`): the shell's registry,
    /// shared. Not saved, never compared: set by the shell (`State::equip`).
    pub machine: crate::capability::Machine,
    /// How far it has heard its thread (`crate::desk::guided`): the
    /// posts before this cursor were delivered. Kept in `convo/run.md`;
    /// `None` (a store from before): what is there counts as heard.
    pub heard: Option<crate::snapshot::Cursor>,
}

impl Engine {
    /// A new engine with no messages.
    pub fn new(name: impl Into<String>, description: impl Into<String>, inference: OpenAi) -> Engine {
        Engine {
            id: String::new(),
            name: name.into(),
            description: description.into(),
            legacy_goal: false,
            parent: None,
            waiting: Vec::new(),
            learning: String::new(),
            learning_log: String::new(),
            learning_turn: None,
            inference,
            messages: Vec::new(),
            session: None,
            meta: BTreeMap::new(),
            response: Responses::default(),
            format: Format::default(),
            tools: crate::tools::builtin_names(),
            artifacts: BTreeMap::new(),
            scripts: BTreeMap::new(),
            alerts: BTreeMap::new(),
            alert_marks: BTreeMap::new(),
            skills: BTreeMap::new(),
            skill_tools: BTreeMap::new(),
            loaded_skills: Vec::new(),
            template: String::new(),
            templates: BTreeMap::new(),
            elements: String::new(),
            run: crate::phase::Run::default(),
            mcp: String::new(),
            mcp_tools: Vec::new(),
            allow: String::new(),
            approve_timeout: None,
            machine: crate::capability::Machine::default(),
            heard: Some(crate::snapshot::Cursor::default()),
        }
    }

    /// Its template, read from `template`.
    pub fn template(&self) -> Result<crate::template::Template, String> {
        crate::template::parse(&self.template)
    }

    /// Its phases, from its template. Broken: the error, and the loop runs
    /// as if there were none.
    pub fn phase_list(&self) -> Result<Vec<crate::phase::Phase>, String> {
        self.template()?.phases()
    }

    /// The skills it may use now: all, or the current phase's `skills:`.
    pub fn visible_skills(&self) -> Vec<(&str, &str)> {
        let phases = self.phase_list().unwrap_or_default();
        let only = phases.get(self.run.phase).and_then(|p| p.skills.as_deref());
        crate::skill::visible(&self.skills, only)
    }

    /// What answers this engine, in a word: the program's name for a
    /// session, `model` otherwise.
    pub fn kind(&self) -> &str {
        self.session.as_ref().map_or("model", |s| s.tool.as_str())
    }
}

/// The whole app state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Light or dark: the theme mode the whole UI is drawn in.
    pub mode: Mode,
    /// The scene setting (`app.md`'s `scene:`; `none` for none); None when
    /// app.md does not say (`crate::scene::settings` then goes on down).
    pub scene: Option<String>,
    /// The motion setting (`app.md`'s `motion:`, seconds; 0 still).
    pub motion: Option<f32>,
    pub engines: Vec<Engine>,
    /// Each root's thread (`crate::shared`), by the root's id (`""`: a
    /// store's top file from before threads, folded in on load). The text
    /// of its `shared.md`.
    pub shared: BTreeMap<String, String>,
    /// The shell's capability registry (`crate::capability`): not saved,
    /// never compared. `equip` gives it to every engine.
    pub capabilities: crate::capability::Machine,
}

impl State {
    /// Set the shell's capability registry and give it to every engine.
    pub fn equip(&mut self, registry: crate::capability::Registry) {
        self.capabilities = crate::capability::Machine::new(registry);
        self.equip_engines();
    }

    /// Give the registry the state holds to every engine (after engines
    /// were added or loaded).
    pub fn equip_engines(&mut self) {
        for e in &mut self.engines {
            e.machine = self.capabilities.clone();
        }
    }

    /// Create a new, empty engine at the end and return its index.
    pub fn add_engine(&mut self) -> usize {
        let name = format!("Engine {}", self.engines.len() + 1);
        let mut e = Engine::new(name, "Talks to the default model.", OpenAi::default());
        e.machine = self.capabilities.clone();
        e.id = self.new_id(&e.name);
        self.engines.push(e);
        self.engines.len() - 1
    }

    /// Create a new session engine run by program `tool` and return its index.
    pub fn add_session(&mut self, tool: &str, description: &str) -> usize {
        let i = self.add_engine();
        let e = &mut self.engines[i];
        e.name = format!("{tool} {}", i + 1);
        let taken: Vec<String> = self.engines.iter().take(i).map(|e| e.id.clone()).collect();
        let id = crate::files::new_id(&self.engines[i].name, &taken.iter().map(String::as_str).collect::<Vec<_>>());
        let e = &mut self.engines[i];
        e.id = id;
        e.description = description.to_string();
        e.session = Some(SessionRef { tool: tool.to_string(), id: None });
        e.allow = crate::approval::DEFAULT_ALLOW.to_string();
        i
    }

    /// Add a tools message (what a session's program ran) to engine `engine`.
    pub fn add_tools(&mut self, engine: usize, text: &str) -> bool {
        self.push(engine, Role::Tool, text, false)
    }

    /// A folder name for a new engine called `name`, unused by the others.
    fn new_id(&self, name: &str) -> String {
        crate::files::new_id(name, &self.engines.iter().map(|e| e.id.as_str()).collect::<Vec<_>>())
    }

    /// Give every engine without an id one (engines read from the old
    /// one-file format had none).
    pub fn ensure_ids(&mut self) {
        for i in 0..self.engines.len() {
            if self.engines[i].id.is_empty() {
                let taken: Vec<String> = self.engines.iter().map(|e| e.id.clone()).collect();
                let id = crate::files::new_id(&self.engines[i].name, &taken.iter().map(String::as_str).collect::<Vec<_>>());
                self.engines[i].id = id;
            }
        }
    }

    /// Make sure there is at least one engine: with none, create one and
    /// return its index (so the caller can give it an inference). With
    /// engines already, `None`.
    pub fn ensure_engine(&mut self) -> Option<usize> {
        self.engines.is_empty().then(|| self.add_engine())
    }

    /// Remove engine `index`, its child engines with it (theirs too), and
    /// the shared context of their level. An index that does not exist is
    /// ignored.
    pub fn remove_engine(&mut self, index: usize) {
        let Some(e) = self.engines.get(index) else { return };
        let mut gone = vec![e.id.clone()];
        let mut i = 0;
        while i < gone.len() {
            let parent = gone[i].clone();
            gone.extend(self.engines.iter().filter(|c| c.parent.as_deref() == Some(parent.as_str())).map(|c| c.id.clone()));
            i += 1;
        }
        self.engines.retain(|e| !gone.contains(&e.id));
        for id in &gone {
            self.shared.remove(id);
        }
    }

    /// The index of the engine with id `id`.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.engines.iter().position(|e| e.id == id)
    }

    /// Save what you typed to engine `engine`. Blank text is refused
    /// (returns false) so Enter on an empty line adds nothing.
    pub fn add_text(&mut self, engine: usize, text: &str) -> bool {
        self.push(engine, Role::User, text, true)
    }

    /// Deliver a wake to engine `engine`: a child's reply, guided posts,
    /// a bounce, a capability event. It is a user message, but the run
    /// goes on where it was: its phase, step, loaded skills and any wait
    /// stay (a developer in `test` that started a long job wakes in
    /// `test`). Only a new ask (`add_text`) starts the run again.
    pub fn add_wake(&mut self, engine: usize, text: &str) -> bool {
        self.push(engine, Role::User, text, false)
    }

    /// Save the model's reply to engine `engine`.
    pub fn add_reply(&mut self, engine: usize, text: &str) -> bool {
        self.push(engine, Role::Assistant, text, false)
    }

    fn push(&mut self, engine: usize, role: Role, text: &str, fresh_run: bool) -> bool {
        let text = text.trim();
        match self.engines.get_mut(engine) {
            Some(e) if !text.is_empty() => {
                // A new ask starts the run at its first phase.
                if fresh_run {
                    e.run = crate::phase::Run::default();
                    e.loaded_skills.clear();
                    e.learning_turn = None;
                    // A turn that waited for a child is over: it calls back.
                    crate::engines::stop_waiting(e);
                }
                e.messages.push(Message { role, text: text.to_string() });
                true
            }
            _ => false,
        }
    }

    /// Remove message `index` from engine `engine`.
    pub fn remove_text(&mut self, engine: usize, index: usize) {
        if let Some(e) = self.engines.get_mut(engine)
            && index < e.messages.len()
        {
            e.messages.remove(index);
        }
    }

    /// Write the state as text: the theme mode, then one section per engine:
    ///
    /// ```text
    /// [theme] dark
    /// [engine] Engine 1
    /// # what this engine is for
    /// = chat-completions<TAB>http://127.0.0.1:1234/v1<TAB>local-model<TAB>
    /// - what you typed
    /// > the model's reply
    /// ```
    ///
    /// A tools' result is `* ` (sent to the model, shown as Tools). The
    /// engine's tools are one `^ reply,write,...` line; each artifact is
    /// `& name<TAB>text`.
    ///
    /// A model engine with a response contract has `~ format` and one
    /// `! key<TAB>description` line per field (`(optional)` at the end of
    /// an optional one's description).
    ///
    /// A session engine also has `@ tool<TAB>session id` and one
    /// `% key<TAB>value` line per metadata entry.
    ///
    /// The `= ` line is the engine's inference (copied from `models.json`
    /// when the engine was made): API, base URL, model, and the
    /// name of the environment variable with the key (empty for none). The
    /// key itself is never written. Each text gets `- ` (yours) or `> ` (a
    /// reply) in front, so a text can never be mistaken for a section or
    /// inference line. One text is one line: a line break inside it is
    /// written as `\n` (and a backslash as `\\`).
    ///
    /// Scripts and alerts are not in this one-file format (it is only read
    /// to move old saves into a store); the stores keep them (`crate::files`).
    pub fn to_text(&self) -> String {
        let mut out = format!("[theme] {}\n", self.mode.as_str());
        for engine in &self.engines {
            out.push_str(&format!("[engine] {}\n", engine.name));
            if !engine.description.is_empty() {
                out.push_str(&format!("# {}\n", escape(&engine.description)));
            }
            let ai = &engine.inference;
            let key = ai.key_env.as_deref().unwrap_or("");
            out.push_str(&format!("= {}\t{}\t{}\t{key}\n", ai.api.as_str(), ai.base_url, ai.model));
            if let Some(s) = &engine.session {
                out.push_str(&format!("@ {}\t{}\n", s.tool, s.id.as_deref().unwrap_or("")));
            }
            out.push_str(&format!("^ {}\n", engine.tools.join(",")));
            for (name, text) in &engine.artifacts {
                out.push_str(&format!("& {}\t{}\n", escape(name), escape(text)));
            }
            if !engine.response.is_empty() {
                out.push_str(&format!("~ {}\n", engine.format.name()));
                for f in &engine.response.fields {
                    let opt = if f.optional { " (optional)" } else { "" };
                    out.push_str(&format!("! {}\t{}{opt}\n", escape(&f.key), escape(&f.description)));
                }
            }
            for (k, v) in &engine.meta {
                out.push_str(&format!("% {}\t{}\n", escape(k), escape(v)));
            }
            for m in &engine.messages {
                let mark = match m.role {
                    Role::Assistant => '>',
                    Role::Tool => '*',
                    // The old one-file format has no summary line: it is
                    // only read to move old saves into a store.
                    Role::Summary => continue,
                    _ => '-',
                };
                out.push_str(&format!("{mark} {}\n", escape(&m.text)));
            }
        }
        out
    }

    /// Read `to_text`'s format back. A `- ` or `> ` line belongs to the engine above
    /// it; any other line (an unknown section, or a text before the first
    /// engine) is skipped.
    pub fn from_text(text: &str) -> State {
        let mut state = State::default();
        // True while the lines belong to an engine section.
        let mut in_engine = false;
        for line in text.lines() {
            if let Some(mode) = line.strip_prefix("[theme] ").and_then(Mode::parse) {
                state.mode = mode;
                in_engine = false;
            } else if let Some(name) = line.strip_prefix("[engine] ") {
                state.engines.push(Engine::new(name, "", OpenAi::default()));
                in_engine = true;
            } else if line.starts_with('[') {
                in_engine = false;
            } else if in_engine && let Some(engine) = state.engines.last_mut() {
                if let Some(text) = line.strip_prefix("- ") {
                    engine.messages.push(Message { role: Role::User, text: unescape(text) });
                } else if let Some(text) = line.strip_prefix("> ") {
                    engine.messages.push(Message { role: Role::Assistant, text: unescape(text) });
                } else if let Some(text) = line.strip_prefix("# ") {
                    engine.description = unescape(text);
                } else if let Some(ai) = line.strip_prefix("= ").and_then(parse_inference) {
                    engine.inference = ai;
                } else if let Some(rest) = line.strip_prefix("@ ") {
                    let (tool, id) = rest.split_once('\t').unwrap_or((rest, ""));
                    let id = (!id.is_empty()).then(|| id.to_string());
                    engine.session = Some(SessionRef { tool: tool.to_string(), id });
                } else if let Some(text) = line.strip_prefix("* ") {
                    engine.messages.push(Message { role: Role::Tool, text: unescape(text) });
                } else if let Some(names) = line.strip_prefix("^ ").or(line.strip_prefix("^")) {
                    engine.tools = names.split(',').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect();
                } else if let Some((name, text)) = line.strip_prefix("& ").and_then(|r| r.split_once('\t')) {
                    engine.artifacts.insert(unescape(name), unescape(text));
                } else if let Some(format) = line.strip_prefix("~ ").and_then(Format::parse_name) {
                    engine.format = format;
                } else if let Some((k, d)) = line.strip_prefix("! ").and_then(|r| r.split_once('\t')) {
                    let d = unescape(d);
                    let (description, optional) = match d.strip_suffix(" (optional)") {
                        Some(d) => (d.to_string(), true),
                        None => (d, false),
                    };
                    engine.response.fields.push(Field { key: unescape(k), description, optional });
                } else if let Some((k, v)) = line.strip_prefix("% ").and_then(|r| r.split_once('\t')) {
                    engine.meta.insert(unescape(k), unescape(v));
                }
            }
        }
        state.ensure_ids();
        state
    }
}

/// A text as one line: `\` becomes `\\`, a line break becomes `\n`.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n")
}

/// Undo `escape`.
fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Read an `= api<TAB>base<TAB>model<TAB>key_env` line. A line that does
/// not parse is ignored, so the engine keeps the default inference.
fn parse_inference(line: &str) -> Option<OpenAi> {
    let mut fields = line.split('\t');
    let api = Api::parse(fields.next()?)?;
    let base_url = fields.next()?.to_string();
    let model = fields.next()?.to_string();
    let key_env = fields.next().filter(|k| !k.is_empty()).map(str::to_string);
    Some(OpenAi { api, base_url, model, key_env, context: None, sampling: Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wake_keeps_the_run_where_it_was_and_a_new_ask_starts_it_again() {
        let mut state = State::default();
        let e = state.add_engine();
        state.engines[e].run = crate::phase::Run { phase: 2, step: 4 };
        state.engines[e].loaded_skills = vec!["test".into()];
        // A child's reply, guided posts, a bounce, a capability event.
        assert!(state.add_wake(e, "Guided posts for you:\n[1] helper → dev: the job ended"));
        assert_eq!(state.engines[e].run, crate::phase::Run { phase: 2, step: 4 }, "a wake does not reset the phase");
        assert_eq!(state.engines[e].loaded_skills, ["test"]);
        assert_eq!(state.engines[e].messages.last().unwrap().role, Role::User);
        // Your new ask starts the run at its first phase.
        assert!(state.add_text(e, "now do the next thing"));
        assert_eq!(state.engines[e].run, crate::phase::Run::default());
        assert!(state.engines[e].loaded_skills.is_empty());
    }

    #[test]
    fn a_state_with_engines_written_as_text_reads_back_the_same() {
        let mut state = State { mode: Mode::Dark, ..State::default() };
        let first = state.add_engine();
        let second = state.add_engine();
        assert!(state.add_text(first, "hello"));
        assert!(state.add_text(first, "[engine] looks like a section but is a text"));
        assert!(state.add_text(second, "second engine's note"));
        state.engines[second].description = "talks to OpenAI".into();
        assert!(state.add_reply(second, "a reply\nover two lines, with a \\ backslash"));
        state.engines[second].inference = OpenAi {
            api: Api::Responses,
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-5".into(),
            key_env: Some("OPENAI_API_KEY".into()),
            context: None,
            sampling: Default::default(),
        };
        let third = state.add_session("claude", "Claude Code, headless");
        state.engines[third].session.as_mut().unwrap().id = Some("ad43-uuid".into());
        state.engines[third].meta.insert("usage.output_tokens".into(), "4".into());
        state.engines[third].meta.insert("note".into(), "two\nlines\tand a tab".into());
        assert_eq!(state.engines[third].kind(), "claude");
        // The old one-file format keeps no approval rules (stores do).
        assert!(state.engines[third].allow.starts_with("Read, Glob"));
        state.engines[third].allow.clear();
        let (contract, _) = Responses::from_md("| key | description |\n|---|---|\n| act | calls |\n| note | extra (optional) |").unwrap();
        state.engines[first].response = contract;
        state.engines[first].format = Format::parse_name("toon=").unwrap();
        state.engines[first].artifacts.insert("plan".into(), "1. read\n2. answer".into());
        state.engines[first].tools = vec!["reply".into()];
        state.engines[first].messages.push(Message { role: Role::Tool, text: "write(...) → saved".into() });
        assert_eq!(State::from_text(&state.to_text()), state);
    }

    #[test]
    fn a_section_this_version_does_not_know_is_skipped_with_its_lines() {
        let old = "[tokens]\ncolor dark accent #FF0000FF\n[notes]\nsomething\n[engine] Engine 1\n- kept\n";
        let state = State::from_text(old);
        assert_eq!(state.engines.len(), 1);
        assert_eq!(state.engines[0].messages, vec![Message { role: Role::User, text: "kept".into() }]);
        assert_eq!(state.engines[0].inference, OpenAi::default(), "an engine saved before inference existed gets an empty one");
    }

    #[test]
    fn blank_text_is_refused_and_a_line_break_stays_on_one_line_in_the_file() {
        let mut state = State::default();
        let e = state.add_engine();
        assert!(!state.add_text(e, "   "));
        assert!(state.add_text(e, "two\nlines"));
        assert!(state.to_text().contains("- two\\nlines\n"), "{}", state.to_text());
        assert_eq!(State::from_text(&state.to_text()).engines[e].messages[0].text, "two\nlines");
    }

    #[test]
    fn an_empty_state_gets_one_engine_and_a_full_one_gets_none() {
        let mut state = State::default();
        assert_eq!(state.ensure_engine(), Some(0));
        assert_eq!(state.ensure_engine(), None);
        assert_eq!(state.engines.len(), 1);
    }
}
