//! The renderer as it was before the element walk (docs/elements.md,
//! phase 0), kept only for the test that proves the walk sends the same
//! prompt, byte for byte. Copied unchanged.

use super::{ARTIFACT_CHARS, ARTIFACTS_CHARS, Rendered};
use crate::live::{self, Snapshots};
use crate::response::ResponseFormat;
use crate::state::Engine;
use crate::template::{self, Slot};

pub fn render_with(e: &Engine, outside: &Snapshots) -> Rendered {
    // History: after the newest summary, if the history was compressed
    // (`crate::context`); the summary itself leads the history slot.
    let mut r = Rendered { history: crate::context::history(e), summary: crate::context::summary(e), ..Rendered::default() };
    // The engine's template (`spec/template.md`); a broken one is said and
    // the default used, so the engine still runs.
    let t = e.template().unwrap_or_else(|why| {
        r.empty.push(("Template".into(), format!("template.md did not read, so the default is used: {why}")));
        template::parse("").expect("the default template reads")
    });
    let mut slot = |s: Slot, body: Result<String, String>| {
        let lead = t.lead(s);
        match body {
            Ok(b) if !b.trim().is_empty() => r.system.push((t.title(s), if lead.is_empty() { b } else { format!("{lead}\n{b}") })),
            Ok(_) => {}
            Err(why) => r.empty.push((t.title(s), why)),
        }
    };
    // 1. instructions: goal.md's sections, in their order; what does not
    // fit is said, as the template's problems are.
    let goal = crate::goal::parse(&e.description);
    let body = goal.render();
    slot(Slot::Instructions, if body.trim().is_empty() { Err("no instructions (goal.md is empty)".into()) } else { Ok(body) });
    let mut goal_notes = Vec::new();
    if e.legacy_goal {
        goal_notes.push((
            crate::goal::FILE.to_string(),
            "read from spec/instruction.md: there is no goal.md yet; the next save writes goal.md".to_string(),
        ));
    }
    goal_notes.extend(goal.notes.into_iter().map(|n| (crate::goal::FILE.to_string(), n)));
    // 2. learning: memory/learning.md as at the turn's start.
    slot(Slot::Learning, crate::learning::slot(e));
    // 3. phase: the plan, the current phase expanded.
    let phases = t.phases().unwrap_or_default();
    slot(
        Slot::Phase,
        if phases.is_empty() { Err("no phases in the template: one implicit phase".into()) } else { Ok(e.run.section(&phases)) },
    );
    // 4, 5. tools and artifacts only mean something with a contract (its `act`).
    if e.response.is_empty() {
        slot(Slot::Tools, Err("no response contract: the answer is free text".into()));
        slot(Slot::Artifacts, Err("no response contract".into()));
    } else {
        let mut tools = crate::tools::describe(&live::toolbox(e));
        // The outside capabilities' tools (declared on this machine, or a
        // peer's), only while present.
        let brought = crate::capability::describe(e);
        if !brought.is_empty() {
            tools.push('\n');
            tools.push_str(&brought);
        }
        for tool in &e.mcp_tools {
            tools.push('\n');
            tools.push_str(&tool.describe());
        }
        slot(Slot::Tools, Ok(tools));
        slot(Slot::Artifacts, Ok(artifacts(e, outside)));
    }
    // A phase answered some other way than this runtime can: said in the
    // prompt view, answered the standard way.
    if let (_, Some(note)) = e.run.response(&phases) {
        r.empty.push(("Response mode".into(), note));
    }
    r.empty.extend(goal_notes);
    r.response = if e.response.is_empty() { String::new() } else { e.format.instructions(&e.response) };
    r.response_title = t.title(Slot::Response);
    // The response and tools just rendered, reduced to choices: whether a
    // jev could answer this step (nothing is sent differently).
    r.jev = crate::jev::check(e);
    r
}

/// The artifacts slot: every artifact as it is now. A plain one by its
/// text (bounded), a kind by its snapshot; the clock last of all, since it
/// changes most.
fn artifacts(e: &Engine, outside: &Snapshots) -> String {
    let sections = artifact_sections(e, outside);
    if sections.is_empty() {
        return "None yet.".into();
    }
    sections.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n\n")
}

/// The artifacts slot in its sections, in order, each exactly as sent
/// (`### name ...` and its body): `(Some(artifact name), section)`, or
/// `(None, section)` for the skills element. The artifact view
/// (`crate::inspect`) shows each one from here, so it is what is sent.
pub fn artifact_sections(e: &Engine, outside: &Snapshots) -> Vec<(Option<String>, String)> {
    let skills = e.visible_skills();
    let mut out = Vec::new();
    let mut kinds = Vec::new();
    let mut budget = ARTIFACTS_CHARS;
    for (name, text) in &e.artifacts {
        if crate::capability::uses(text).is_some() {
            continue;
        }
        let size = text.chars().count();
        let room = ARTIFACT_CHARS.min(budget);
        if room == 0 {
            out.push((
                Some(name.clone()),
                format!("### {name} ({size} characters)\n\nNot shown (the artifacts are long); read(name=\"{name}\") for it."),
            ));
            continue;
        }
        let shown: String = text.chars().take(room).collect();
        let note = if size > room {
            format!("\n… cut: {room} of {size} characters shown; read(name=\"{name}\") for all of it.")
        } else {
            String::new()
        };
        budget -= shown.chars().count();
        out.push((Some(name.clone()), format!("### {name} ({size} characters)\n\n{}{note}", shown.trim_end())));
    }
    // Capability artifacts (`crate::capability::equipped`): only what is
    // present; one whose capabilities are all absent is not rendered.
    for q in crate::capability::equipped(e) {
        let name = q.name.clone();
        let mut section = match q.live {
            Some(kind) => {
                let body = kind.snapshot(&name, e, outside).unwrap_or_else(|| "(taken when the prompt is sent)".into());
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
            section.push_str(&crate::capability::section(c));
        }
        kinds.push((Some(name), section));
    }
    // Skills are an element too: what can be loaded, before the kinds (the
    // clock stays last).
    if !skills.is_empty() {
        out.push((None, format!("### skills — your skills\n\n{}", crate::skill::snapshot(&skills))));
    }
    out.extend(kinds);
    out
}

/// The element walk sends what the slots sent, byte for byte, for a
/// spread of engines and of what the shell knows.
mod same {
    use super::super::{Rendered, artifact_sections, render_with};
    use crate::capability::{Machine, Provider, Registry, State as Cap, declared};
    use crate::engines::{Seen, apply, family};
    use crate::environment::{Cache, Environment, Kind, Phase};
    use crate::file::tests::FakeDisk;
    use crate::inference::{Message, OpenAi, Role};
    use crate::live::{Clock, Snapshots};
    use crate::response::{Format, Responses};
    use crate::state::{Engine, State};
    use crate::turn::{NoHost, step_in};

    fn contract(e: &mut Engine) {
        e.response = Responses::from_md(include_str!("../../../../responses.md")).unwrap().0;
        e.format = Format::default();
    }

    fn engine(description: &str) -> Engine {
        let mut e = Engine::new("E", description, OpenAi::default());
        contract(&mut e);
        e.messages.push(Message { role: Role::User, text: "what now?".into() });
        e
    }

    fn idle(_: usize) -> Seen {
        Seen { label: "idle".into(), ..Seen::default() }
    }

    /// The shipped default, and a child of each of its templates.
    fn shipped() -> State {
        let mut s = crate::shipped::state().unwrap();
        s.add_text(0, "change /tmp/hello");
        for template in ["developer", "reviewer", "researcher"] {
            let mut fam = family(&s, 0, &idle);
            let act = format!(
                "thought: x\nact: engine_start(name=\"{template}-1\", goal=\"Do it.\", done=\"Done.\", template=\"{template}\", root=\"/tmp/hello\")"
            );
            step_in(&mut s.engines[0], &act, &mut |_| None, &NoHost, Some(&mut fam));
            let applied = apply(&mut s, 0, fam.effects);
            assert_eq!(applied.run.len(), 1, "made {template}: {:?}", s.engines[0].messages.last());
        }
        s
    }

    /// Hand-made engines, one for each thing a slot did.
    fn made() -> Vec<(&'static str, Engine)> {
        let mut out = Vec::new();
        let mut free = Engine::new("E", "Be brief.", OpenAi::default());
        free.messages.push(Message { role: Role::User, text: "hi".into() });
        out.push(("no contract", free));
        out.push(("empty everything", Engine::new("E", "", OpenAi::default())));
        out.push(("contract, nothing else", engine("")));
        let mut e = engine("## Done\nA list.\n\n## Goal\nSort mail.\n\n## Mood\nhappy");
        e.legacy_goal = true;
        out.push(("legacy goal with notes", e));
        let mut e = engine("## Goal\nBe brief.\n## Scope\nAlone.\n## Done\nSaid.");
        e.learning = "## lessons\n- Check first.\n".into();
        e.template = "| phase |\n|---|\n| only |\n\n## phase: only\nDo it.\n\n## tools\ntitle: Your tools\nCall these:\n\n## artifacts\ntitle: Things\nNow:".into();
        out.push(("phases, learning, renamed slots with leads", e));
        let mut e = engine("Go.");
        e.template = "| phase | response |\n|-|-|\n| gate | jev |\n| work | |\n\n## phase: gate\nRun it?\n\n## phase: work\nDo it.".into();
        e.tools = Vec::new();
        out.push(("a jev phase", e));
        let mut e = engine("Go.");
        e.template = "| phase | response |\n|-|-|\n| a | nonsense |".into();
        out.push(("a broken template", e));
        let mut e = engine("Go.");
        e.artifacts.insert("plan".into(), "1. read\n2. answer  \n".into());
        e.artifacts.insert("long".into(), "x".repeat(super::ARTIFACT_CHARS + 10));
        out.push(("a plain and a cut artifact", e));
        let mut e = engine("Go.");
        for i in 0..6 {
            e.artifacts.insert(format!("part{i}"), "y".repeat(super::ARTIFACT_CHARS - 300 * i));
        }
        out.push(("past the total budget", e));
        let mut e = engine("Go.");
        for (n, t) in [
            ("a-now", "kind: clock"),
            ("plan", "tea"),
            ("z-scripts", "kind: scripts"),
            ("alerts", "kind: alerts"),
            ("code", "kind: file\nroot: .\nopen: src/a.rs 1-2\nopen: README.md"),
            ("look", "uses: [file:read]\nroot: .\nopen: README.md"),
            ("term", "uses: [terminal]\nroot: ."),
            ("kids", "kind: engines"),
            ("thread", "kind: shared"),
            ("learn", "kind: learning"),
            ("both", "uses: [file, clock]\nroot: ."),
        ] {
            e.artifacts.insert(n.into(), t.into());
        }
        e.learning = "## user\n- Short answers.\n".into();
        out.push(("every kind, the clock last", e));
        let mut e = engine("Go.");
        e.skills.insert("triage".into(), "---\nname: triage\ndescription: sort issues\n---\nSort them.".into());
        e.artifacts.insert("notes".into(), "n".into());
        out.push(("skills", e));
        let mut e = engine("Go.");
        e.mcp_tools = vec![
            crate::mcp::Tool {
                server: "fs".into(),
                name: "zeta".into(),
                description: "Last by name, first in order.".into(),
                schema: serde_json::json!({}),
            },
            crate::mcp::Tool {
                server: "fs".into(),
                name: "alpha".into(),
                description: "Reads.".into(),
                schema: serde_json::json!({"properties": {"path": {"type": "string"}}, "required": ["path"]}),
            },
        ];
        out.push(("mcp tools keep their order", e));
        let mut r = Registry::default();
        let declare = |file: &str, text: &str| declared::parse(file, text).unwrap().manifest(Cap::Present);
        r.register(
            Provider::Declared,
            vec![
                declare("battery.md", include_str!("../../../../capabilities/battery.md")),
                declare("gpio.md", include_str!("../../../../capabilities/gpio.md")),
            ],
        );
        r.snapshot("battery", &Provider::Declared, "Now drawing from 'AC Power' 79%; charging", 0);
        let mut e = engine("Go.");
        for (n, t) in [
            ("power", "uses: [battery]"),
            ("pins", "uses: [gpio]"),
            ("power-and-time", "uses: [battery, clock]"),
            ("gone", "uses: [sensors]"),
        ] {
            e.artifacts.insert(n.into(), t.into());
        }
        e.machine = Machine::new(r);
        out.push(("outside capabilities", e));
        let mut e = engine("Go.");
        e.messages = vec![
            Message { role: Role::User, text: "first".into() },
            Message { role: Role::Assistant, text: "thought: a\nact: reply(text=\"b\")".into() },
            Message { role: Role::Summary, text: "They asked twice.".into() },
            Message { role: Role::Assistant, text: "thought: c\nact: reply(text=\"d\")".into() },
            Message { role: Role::User, text: "again".into() },
        ];
        out.push(("a summary", e));
        let mut e = engine("");
        e.messages =
            vec![Message { role: Role::User, text: "older".into() }, Message { role: Role::Summary, text: "Only a summary.".into() }];
        out.push(("a summary and no goal", e));
        out
    }

    /// The environments a shell reports: a ready VM, a booting one, a
    /// failed one, a host.
    fn environments() -> Vec<Environment> {
        let vm = |phase| Environment {
            kind: Kind::BrowserVm,
            phase,
            detail: "Alpine 3.24.4 x86 in v86, 256 MB, in this page".into(),
            workspace: "/work".into(),
            cache: Some(Cache { bytes: 36_200_000, saved: 0 }),
            runtimes: Some(vec!["python3".into(), "tcc".into()]),
        };
        let mut booting = vm(Phase::Booting("restoring the saved machine".into()));
        booting.runtimes = None;
        let mut failed = vm(Phase::Failed("vm/vm.js did not load".into()));
        failed.runtimes = None;
        let mut host = Environment::host("macos aarch64", "/Users/me/repo");
        host.runtimes = Some(vec!["python3".into(), "node".into(), "cargo".into()]);
        vec![vm(Phase::Ready), booting, failed, host]
    }

    /// `e` on a machine whose shell reports `env` (its registry otherwise kept).
    fn with_env(e: &Engine, env: Environment) -> Engine {
        let mut r: Registry = (*e.machine.0).clone();
        r.environment = Some(env);
        let mut e = e.clone();
        e.machine = Machine::new(r);
        e
    }

    /// `e` on a machine whose shell reports the sample subscriptions
    /// (`crate::subscriptions::tests::sample`).
    fn with_subs(e: &Engine) -> Engine {
        let mut r: Registry = (*e.machine.0).clone();
        r.subscriptions = Some(crate::subscriptions::tests::sample());
        let mut e = e.clone();
        e.machine = Machine::new(r);
        e
    }

    #[test]
    fn the_head_holds_the_subscriptions_before_the_environment() {
        let s = shipped();
        let t = with_env(&with_subs(&s.engines[0]), environments().remove(0));
        let at = Snapshots { clock: Some(Clock::new(1_795_356_330, 0)), ..Default::default() };
        let r = render_with(&t, &at);
        let system = r.messages()[0].text.clone();
        let subs = crate::subscriptions::tests::sample().text();
        let env = "environment: browser-vm · ready · /work · python3, tcc · saved 36 MB";
        assert!(system.ends_with(&format!("\n\n{subs}\n\n{env}")), "subscriptions, then the environment, last: {system}");
        assert!(system.find("### thread").unwrap() < system.find("subscriptions (default: local)").unwrap());
        let ids: Vec<&str> = r.elements.iter().map(|l| l.id.as_str()).filter(|i| *i == "subscriptions" || *i == "environment").collect();
        assert_eq!(ids, ["subscriptions", "environment"]);
        // None reported: nothing sent for it, and the walk says why.
        let r = render_with(&s.engines[0], &at);
        assert!(!r.messages()[0].text.contains("subscriptions (default:"));
        let listed = r.elements.iter().find(|l| l.id == "subscriptions").expect("listed");
        assert!(listed.empty.as_deref().unwrap().starts_with("no subscriptions known"), "{listed:?}");
        // Only the artifacts section changes.
        let old = super::render_with(&s.engines[0], &at);
        assert_eq!(render_with(&t, &at).system.iter().filter(|(n, _)| n != "Artifacts").collect::<Vec<_>>(), old.system.iter().filter(|(n, _)| n != "Artifacts").collect::<Vec<_>>());
    }

    #[test]
    fn the_head_holds_the_environment_and_sends_its_line() {
        let s = shipped();
        let t = &s.engines[0];
        assert_eq!(t.elements.trim(), "subscriptions\nenvironment", "the shipped translator holds it");
        let at = Snapshots { clock: Some(Clock::new(1_795_356_330, 0)), ..Default::default() };
        let text = |env: Option<Environment>| {
            let e = match env {
                Some(env) => with_env(t, env),
                None => t.clone(),
            };
            let r = render_with(&e, &at);
            (r.messages()[0].text.clone(), r)
        };
        let lines = [
            "environment: browser-vm · ready · /work · python3, tcc · saved 36 MB",
            "environment: browser-vm · booting: restoring the saved machine · /work · saved 36 MB",
            "environment: browser-vm · failed: vm/vm.js did not load · /work · saved 36 MB",
            "environment: host · ready · /Users/me/repo · python3, node, cargo",
        ];
        for (env, line) in environments().into_iter().zip(lines) {
            let (system, r) = text(Some(env));
            // After every artifact (the translator has no clock, so it is last).
            assert!(system.ends_with(&format!("\n\n{line}")), "{line} last in {system}");
            assert!(system.find("### thread").unwrap() < system.find(line).unwrap());
            let listed = r.elements.iter().find(|l| l.id == "environment").expect("listed");
            assert_eq!(listed.empty, None);
            // Only the artifacts section changes: the rest is what the slots sent.
            let old = super::render_with(t, &at);
            assert_eq!(r.system.iter().filter(|(n, _)| n != "Artifacts").collect::<Vec<_>>(), old.system.iter().filter(|(n, _)| n != "Artifacts").collect::<Vec<_>>());
            assert_eq!(r.history, old.history);
            assert_eq!(r.response, old.response);
        }
        // None reported: nothing sent for it, and the walk says why.
        let (system, r) = text(None);
        assert!(!system.contains("environment: "), "{system}");
        let listed = r.elements.iter().find(|l| l.id == "environment").expect("listed");
        assert!(listed.empty.as_deref().unwrap().starts_with("no environment known"), "{listed:?}");
        // A line it cannot read is a note, never a panic, and the rest still reads.
        let mut odd = with_env(t, environments().remove(0));
        odd.elements = "environment\nmachines\nweather\n".into();
        let r = render_with(&odd, &at);
        let notes: Vec<&str> = r.empty.iter().filter(|(n, _)| n == "elements.md").map(|(_, w)| w.as_str()).collect();
        assert_eq!(notes, ["`machines` is not built yet (docs/elements.md, phase 4): left out", "`weather` is not an element (known: subscriptions, environment): left out"]);
        assert!(r.to_text().contains("\n- elements.md: `weather` is not an element"), "in the [not sent] notes");
        assert!(r.messages()[0].text.contains("environment: browser-vm · ready"));
    }

    fn check(what: &str, e: &Engine, at: &Snapshots) {
        let new = render_with(e, at);
        let old = super::render_with(e, at);
        assert!(!new.elements.is_empty(), "{what}: the walk lists its elements");
        assert_eq!(Rendered { elements: Vec::new(), ..new.clone() }, old, "{what}");
        assert_eq!(new.messages(), old.messages(), "{what}: the messages sent");
        assert_eq!(new.to_text(), old.to_text(), "{what}: the prompt view's text");
        assert_eq!(new.fixed_tokens(), old.fixed_tokens(), "{what}");
        assert_eq!(artifact_sections(e, at), super::artifact_sections(e, at), "{what}: the artifact views");
        assert_eq!(crate::inspect::slots(e, &new), crate::inspect::slots(e, &old), "{what}: the slots view");
    }

    #[test]
    fn the_element_walk_sends_the_same_prompt_as_the_slots() {
        let disk = FakeDisk::with(&[
            ("src/a.rs", "fn a() {}\nfn b() {}\n"),
            ("README.md", "# hello\n"),
            ("/tmp/hello/greet.ts", "export const g = 'hi'\n"),
        ]);
        let s = shipped();
        let mut cases: Vec<(String, Engine, Option<usize>)> =
            s.engines.iter().enumerate().map(|(i, e)| (format!("shipped {}", e.id), e.clone(), Some(i))).collect();
        assert!(cases.len() >= 4, "the translator and its three children: {}", cases.len());
        cases.extend(made().into_iter().map(|(n, e)| (n.to_string(), e, None)));
        let mut family_state = State::default();
        for (what, e, i) in &cases {
            let fam = match i {
                Some(i) => family(&s, *i, &idle),
                None => {
                    family_state.engines = vec![e.clone()];
                    family(&family_state, 0, &idle)
                }
            };
            let clock = Some(Clock::new(1_795_356_330, 0));
            for (how, at) in [
                ("for showing", Snapshots::default()),
                ("with the clock", Snapshots { clock, ..Default::default() }),
                ("with the disk", Snapshots { clock, host: Some(&disk), family: None }),
                ("with everything", Snapshots { clock, host: Some(&disk), family: Some(&fam) }),
            ] {
                check(&format!("{what} ({how})"), e, &at);
                // An engine that holds no extra elements (no spec/elements.md)
                // sends the same whatever environment the shell reports.
                if e.elements.trim().is_empty() {
                    for env in environments() {
                        check(&format!("{what} ({how}, {})", env.line()), &with_env(e, env), &at);
                    }
                    check(&format!("{what} ({how}, with subscriptions)"), &with_subs(e), &at);
                }
            }
        }
        // The cases reach what each slot did, so the equality means something.
        let at = Snapshots { clock: Some(Clock::new(1_795_356_330, 0)), host: Some(&disk), family: None };
        let sent = |what: &str| {
            let (_, e) = made().into_iter().find(|(n, _)| *n == what).unwrap();
            let r = render_with(&e, &at);
            (r.to_text(), r)
        };
        let (kinds, _) = sent("every kind, the clock last");
        assert!(kinds.contains("### code — ") && kinds.contains("fn a() {}") && kinds.trim_end().contains("UTC"), "{kinds}");
        let (outside, _) = sent("outside capabilities");
        assert!(outside.contains("#### battery (") && outside.contains("79%; charging") && outside.contains("- battery."), "{outside}");
        let (mcp, _) = sent("mcp tools keep their order");
        assert!(mcp.find("fs.zeta").unwrap() < mcp.find("fs.alpha").unwrap(), "{mcp}");
        let (_, summary) = sent("a summary");
        assert_eq!(summary.summary.as_deref(), Some("They asked twice."));
        let (budget, _) = sent("past the total budget");
        assert!(budget.contains("Not shown (the artifacts are long)"), "{budget}");
        let (_, broken) = sent("a broken template");
        assert_eq!(broken.empty[0].0, "Template", "{:?}", broken.empty);
        let child = s.engines.iter().find(|e| e.id.starts_with("developer")).expect("a developer child");
        assert!(!child.phase_list().unwrap().is_empty(), "the developer child has phases");
    }
}
