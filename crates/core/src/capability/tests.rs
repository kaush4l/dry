use super::declared::{self, Arg};
use super::*;
use crate::inference::OpenAi;
use crate::live::{self, Snapshots};

const BATTERY: &str = include_str!("../../../../capabilities/battery.md");
const GPIO: &str = include_str!("../../../../capabilities/gpio.md");
const SENSORS: &str = include_str!("../../../../capabilities/sensors.md");

fn engine(artifacts: &[(&str, &str)], reg: Registry) -> Engine {
    let mut e = Engine::new("E", "Do it.", OpenAi::default());
    e.response = crate::response::Responses::from_md(include_str!("../../../../responses.md")).unwrap().0;
    e.format = crate::response::Format::default();
    for (n, t) in artifacts {
        e.artifacts.insert(n.to_string(), t.to_string());
    }
    e.machine = Machine::new(reg);
    e
}

fn battery_remote(peer: &str) -> Capability {
    let d = declared::parse("battery.md", BATTERY).unwrap();
    let mut c = d.manifest(State::Present);
    c.provider = Provider::Remote(peer.into());
    c
}

fn prompt(e: &Engine) -> String {
    crate::prompt::render_with(e, &Snapshots::default()).messages()[0].text.clone()
}

// ---- the registry: merge, absent, routing ----

#[test]
fn the_registry_holds_every_compiled_capability_present_and_merges_a_peer_by_provider() {
    let mut r = Registry::default();
    for id in compiled_ids() {
        assert!(r.present(id), "{id}");
        assert_eq!(r.effective(id).unwrap().provider, Provider::Compiled);
    }
    assert!(!r.present("battery"));
    let ch = r.register(Provider::Remote("mac".into()), vec![battery_remote("mac")]);
    assert_eq!(ch.added, ["battery"]);
    assert!(r.present("battery"));
    assert_eq!(r.effective("battery").unwrap().provider, Provider::Remote("mac".into()));
    // A declared one on this machine is used before a peer's.
    let local = declared::parse("battery.md", BATTERY).unwrap().manifest(State::Present);
    r.register(Provider::Declared, vec![local]);
    assert_eq!(r.effective("battery").unwrap().provider, Provider::Declared);
    // An absent compiled one gives way to a present peer's (the browser's terminal is the companion's).
    let mut term = Capability::new("terminal", "the companion's terminal", Provider::Remote("mac".into()));
    term.tools.push(Tool { name: "run".into(), doc: String::new(), inputs: Vec::new(), approval: true, remote: "terminal.run".into() });
    r.set_state("terminal", &Provider::Compiled, State::Absent("no terminal in the browser".into()));
    assert!(!r.present("terminal"));
    r.register(Provider::Remote("mac".into()), vec![battery_remote("mac"), term]);
    assert_eq!(r.effective("terminal").unwrap().provider, Provider::Remote("mac".into()));
}

#[test]
fn a_re_register_says_what_changed_and_a_disconnect_makes_the_peers_capabilities_absent() {
    let mut r = Registry::default();
    let peer = Provider::Remote("pi".into());
    r.hello(Peer { name: "pi".into(), runtime: "dry-companion".into(), ..Peer::default() });
    r.register(peer.clone(), vec![battery_remote("pi"), Capability::new("gpio", "pins", peer.clone())]);
    let mut changed = battery_remote("pi");
    changed.doc = "a new doc".into();
    let ch = r.register(peer.clone(), vec![changed]);
    assert_eq!((ch.added.len(), ch.changed, ch.removed), (0, vec!["battery".to_string()], vec!["gpio".to_string()]));
    let gone = r.disconnect("pi", "its stream closed");
    assert_eq!(gone, ["battery"]);
    assert!(!r.present("battery"));
    assert_eq!(r.state("battery"), State::Absent("pi left: its stream closed".into()));
    assert!(!r.peers[0].connected);
}

#[test]
fn a_call_routes_to_the_provider_by_capability_and_tool() {
    let mut r = Registry::default();
    r.register(Provider::Remote("mac".into()), vec![battery_remote("mac")]);
    let call = Call { name: "battery.status".into(), args: vec![("artifact".into(), "power".into())] };
    let routed = r.route(&call).unwrap();
    assert_eq!(routed.provider, Provider::Remote("mac".into()));
    assert_eq!(routed.tool, "battery.status", "a peer's tool by its MCP name");
    assert!(routed.args.is_empty(), "artifact= is the engine's, not the provider's");
    assert_eq!(r.route(&Call { name: "battery.nope".into(), args: vec![] }), None);
    assert_eq!(r.route(&Call { name: "open".into(), args: vec![] }), None, "compiled tools run in core");
    r.register(Provider::Declared, vec![declared::parse("battery.md", BATTERY).unwrap().manifest(State::Present)]);
    let routed = r.route(&call).unwrap();
    assert_eq!((routed.provider, routed.tool.as_str()), (Provider::Declared, "status"));
    r.disconnect("mac", "gone");
    r.set_state("battery", &Provider::Declared, State::Absent("probe failed".into()));
    assert_eq!(r.route(&call), None, "absent: nothing to route to");
}

// ---- self-injection: absent is not rendered ----

#[test]
fn an_absent_capability_is_not_rendered_has_no_tools_and_a_present_one_brings_its_doc_tools_and_snapshot() {
    let e = engine(&[("power", "uses: [battery]")], Registry::default());
    assert!(!live::toolbox(&e).iter().any(|t| t.starts_with("battery")));
    let p = prompt(&e);
    assert!(!p.contains("### power") && !p.contains("battery"), "absent: not in the prompt at all");

    let mut r = Registry::default();
    r.register(Provider::Remote("mac".into()), vec![battery_remote("mac")]);
    r.snapshot("battery", &Provider::Remote("mac".into()), "100%; charged; present: true", 10);
    let mut e = engine(&[("power", "uses: [battery]")], r.clone());
    assert!(live::toolbox(&e).contains(&"battery.status".to_string()));
    let p = prompt(&e);
    assert!(p.contains("### power — capabilities: battery"), "{p}");
    assert!(p.contains("This Mac's battery"), "the doc, while present");
    assert!(p.contains("100%; charged"), "the snapshot");
    assert!(p.contains("- battery.status(): The battery now"), "the tool line: {p}");

    r.disconnect("mac", "the companion stopped");
    e.machine = Machine::new(r);
    assert!(!prompt(&e).contains("battery"), "gone with its peer");
    assert!(!live::toolbox(&e).iter().any(|t| t.starts_with("battery.")));
}

#[test]
fn a_compiled_capability_a_peer_provides_keeps_its_compiled_tools() {
    let mut r = Registry::default();
    r.set_state("terminal", &Provider::Compiled, State::Absent("no terminal in the browser".into()));
    let mut term = Capability::new("terminal", "the companion's shell", Provider::Remote("mac".into()));
    term.tools.push(Tool { name: "run".into(), doc: String::new(), inputs: Vec::new(), approval: true, remote: "terminal.run".into() });
    term.tools.push(Tool { name: "exec".into(), doc: String::new(), inputs: Vec::new(), approval: true, remote: "terminal.exec".into() });
    r.register(Provider::Remote("mac".into()), vec![term]);
    let e = engine(&[("term", "kind: terminal\nroot: .")], r);
    let tools = live::toolbox(&e);
    for t in ["run", "start", "background"] {
        assert!(tools.contains(&t.to_string()), "{t}: {tools:?}");
    }
    assert!(!tools.iter().any(|t| t == "exec" || t.starts_with("terminal.")), "the peer's manifest names are not the engine's: {tools:?}");
    assert_eq!(live::present(&e), [("term".to_string(), Live::Terminal)]);
}

#[test]
fn a_compiled_capability_absent_here_takes_its_artifacts_out_of_the_prompt() {
    let mut r = Registry::default();
    r.set_state("terminal", &Provider::Compiled, State::Absent("no terminal in the browser".into()));
    let e = engine(&[("term", "kind: terminal\nroot: ."), ("now", "kind: clock")], r);
    assert!(!live::toolbox(&e).contains(&"run".to_string()));
    assert_eq!(live::present(&e).len(), 1, "only the clock");
    assert!(!prompt(&e).contains("### term"));
}

// ---- `kind:` stays an alias ----

#[test]
fn kind_lines_load_unchanged_and_mean_the_same_capabilities() {
    assert_eq!(uses("kind: file\nroot: ."), Some(vec!["file".to_string()]));
    assert_eq!(uses("kind: learning"), Some(vec!["learning".to_string(), "analytics".to_string()]));
    assert_eq!(uses("kind: dashboard"), Some(vec!["shared".to_string()]));
    assert_eq!(uses("kind: thread"), Some(vec!["shared".to_string()]));
    assert_eq!(uses("kind: weather"), None, "an unknown kind is plain text, as before");
    assert_eq!(uses("notes"), None);
    assert_eq!(uses("uses: [file, clock]"), Some(vec!["file".to_string(), "clock".to_string()]));
    assert_eq!(uses("uses: battery, gpio"), Some(vec!["battery".to_string(), "gpio".to_string()]));
    assert_eq!(Live::of("uses: [clock, file]"), Some(Live::Clock));
    assert_eq!(Live::of("kind: learning"), Some(Live::Learning));
    // The same toolbox as before for every old spelling.
    let old = engine(&[("learn", "kind: learning"), ("code", "kind: file\nroot: ."), ("now", "kind: clock")], Registry::default());
    let tools = live::toolbox(&old);
    for t in ["open", "focus", "edit", "save", "collect", "learn", "analytics"] {
        assert!(tools.contains(&t.to_string()), "{t}: {tools:?}");
    }
    let new = engine(
        &[("learn", "uses: [learning, analytics]"), ("code", "uses: [file]\nroot: ."), ("now", "uses: [clock]")],
        Registry::default(),
    );
    assert_eq!(live::toolbox(&new), tools);
    assert_eq!(
        problem("uses: [file, terminal]").unwrap(),
        "file and terminal each keep their state in the artifact: an artifact holds one; make one artifact each"
    );
    assert_eq!(problem("uses: [file, clock, battery]"), None);
}

#[test]
fn a_uses_artifact_keeps_its_header_through_its_kinds_writes() {
    struct Disk;
    impl crate::live::Host for Disk {
        fn now(&self) -> i64 {
            0
        }
        fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
            Ok(vec!["a.txt".into()])
        }
        fn read(&self, _: &str, _: &str) -> crate::live::Content {
            crate::live::Content::Text("hello\n".into())
        }
        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, _: &crate::live::Job) -> Result<Option<i32>, String> {
            Ok(Some(0))
        }
        fn spawn(&self, _: &crate::live::Job) -> Result<(), String> {
            Ok(())
        }
        fn status(&self, _: &crate::live::Job) -> crate::live::Proc {
            crate::live::Proc::Ended(Some(0))
        }
        fn tail(&self, _: &crate::live::Job, _: usize) -> String {
            String::new()
        }
    }
    let mut e = engine(&[("code", "uses: [file, clock]\nroot: .")], Registry::default());
    let call = Call { name: "open".into(), args: vec![("path".into(), "a.txt".into())] };
    let done = live::call(&mut e, &call, &Disk, None).unwrap();
    assert!(done.unwrap().starts_with("opened a.txt"));
    assert_eq!(e.artifacts["code"], "uses: [file, clock]\nroot: .\nopen: a.txt\n");
    assert!(live::toolbox(&e).contains(&"open".to_string()));
}

// ---- declared capabilities: the parser ----

#[test]
fn the_example_files_read() {
    let b = declared::parse("battery.md", BATTERY).unwrap();
    assert_eq!((b.id.as_str(), b.interval, b.tools.len(), b.events.len()), ("battery", 60, 1, 2));
    assert_eq!(b.probe, Some(vec!["pmset".to_string(), "-g".into(), "batt".into()]));
    assert!(b.actuators().is_empty());
    let g = declared::parse("gpio.md", GPIO).unwrap();
    assert_eq!(g.actuators(), ["set"], "setting a pin needs approval");
    assert_eq!(
        g.tools[1].command,
        [
            Arg::Text("pinctrl".into()),
            Arg::Text("set".into()),
            Arg::Input("pin".into()),
            Arg::Text("op".into()),
            Arg::Input("level".into())
        ]
    );
    let s = declared::parse("sensors.md", SENSORS).unwrap();
    assert_eq!(s.events[0].when, When::Compare { key: "temp".into(), op: Op::Gt, value: "70".into() });
    let m = g.manifest(State::Absent("probe `test -e /dev/gpiochip0` exited 1".into()));
    assert_eq!((m.provider, m.snapshot.unwrap().interval), (Provider::Declared, Some(10)));
}

#[test]
fn a_broken_file_says_its_line() {
    let err = |t: &str| declared::parse("x.md", t).unwrap_err();
    assert_eq!(err("battery\n"), "x.md:1: the file starts with `# <id>`, the capability's id");
    assert_eq!(err("# Bad Id\n"), "x.md:1: `Bad Id` is not a capability id (lower case letters, digits, - and _)");
    assert_eq!(err("# a\ndoc\n\n## widget: x\n"), "x.md:4: `## widget: x`: a section is `## tool: <name>` or `## event: <name>`");
    assert_eq!(err("# a\ndoc\n## tool: t\nno command\n"), "x.md:3: tool t has no `command:`");
    assert_eq!(err("# a\ndoc\nsnapshot: cat x\n## event: e\nwhen: temp ~ 3\n").split(": ").next().unwrap(), "x.md:5");
    assert_eq!(err("# a\ndoc\n## tool: t\ncommand: x\napproval: maybe\n"), "x.md:5: approval is yes or no, not `maybe`");
    assert_eq!(
        err("# a\ndoc\n## tool: t\ncommand: x {n}\ninputs:\n- n (colour): what\n"),
        "x.md:6: `colour`: an input's flags are its type (string, integer, number, boolean) and `required`"
    );
    assert_eq!(err("# a\n"), "x.md:1: say what it is and how to use it, under the `# id` line");
    assert_eq!(err("# a\ndoc\n## event: e\nwhen: contains x\n"), "x.md:1: events are read from the snapshot: add a `snapshot:` command");
}

#[test]
fn inputs_go_in_as_whole_arguments_never_through_a_shell() {
    let ok = "# t\ndoc\n## tool: greet\ncommand: echo hello {who}\ninputs:\n- who (string, required): who\n- n (integer): times\n";
    let d = declared::parse("t.md", ok).unwrap();
    let attack = "Ada; touch /tmp/x $(rm -rf ~) `id`";
    let a = d.argv("greet", &[("who".into(), attack.into())]).unwrap();
    assert_eq!(a.program, "echo");
    assert_eq!(a.args, ["hello", attack], "one whole argument, as given");
    assert!(a.env.contains(&("DRY_INPUT_WHO".to_string(), attack.to_string())));
    assert_eq!(d.argv("greet", &[]).unwrap_err(), "t.greet needs who=\"...\"");
    assert_eq!(d.argv("greet", &[("who".into(), "a".into()), ("n".into(), "2; ls".into())]).unwrap_err(), "n must be a whole number");
    assert_eq!(
        d.argv("greet", &[("who".into(), "a".into()), ("x".into(), "1".into())]).unwrap_err(),
        "t.greet has no input x; its inputs: who, n"
    );
    assert_eq!(d.argv("greet", &[("who".into(), "a\0b".into())]).unwrap_err(), "who holds a zero byte");
    let refuse =
        |cmd: &str| declared::parse("t.md", &format!("# t\ndoc\n## tool: x\ncommand: {cmd}\ninputs:\n- v (string): it\n")).unwrap_err();
    assert_eq!(refuse("echo hi={v}"), "t.md:4: \"hi={v}\": an input goes in as a whole argument ({name}), never inside one");
    assert_eq!(refuse("sh -c {v}"), "t.md:4: an input cannot follow -c: it would run as code");
    assert_eq!(refuse("python3 -c {v}"), "t.md:4: an input cannot follow -c: it would run as code");
    assert_eq!(refuse("{v} --help"), "t.md:4: the program itself cannot be an input");
    assert_eq!(refuse("echo {w}"), "t.md:4: {w} is not one of its inputs");
    assert!(declared::parse("t.md", "# t\ndoc\nsnapshot: cat {v}\n").is_err(), "a snapshot takes no inputs");
    let json = "# t\ndoc\n## tool: x\ncommand: [\"say\", \"two words\", \"{v}\"]\ninputs:\n- v: it\n";
    let a = declared::parse("t.md", json).unwrap().argv("x", &[("v".into(), "hi".into())]).unwrap();
    assert_eq!(a.args, ["two words", "hi"]);
}

#[test]
fn a_probe_answer_is_the_state() {
    let cmd = vec!["test".to_string(), "-e".into(), "/dev/gpiochip0".into()];
    assert_eq!(declared::probed(&cmd, Ok(0)), State::Present);
    assert_eq!(declared::probed(&cmd, Ok(1)), State::Absent("probe `test -e /dev/gpiochip0` exited 1".into()));
    assert_eq!(
        declared::probed(&cmd, Err("no such program".into())),
        State::Absent("probe `test -e /dev/gpiochip0` did not run: no such program".into())
    );
}

// ---- events, approvals ----

#[test]
fn an_event_fires_once_when_its_condition_becomes_true() {
    let mut r = Registry::empty();
    r.register(Provider::Declared, vec![declared::parse("battery.md", BATTERY).unwrap().manifest(State::Present)]);
    let p = Provider::Declared;
    assert_eq!(
        r.snapshot("battery", &p, "Now drawing from 'Battery Power' 80%; discharging", 0),
        Vec::new(),
        "the first reading never fires"
    );
    assert_eq!(r.snapshot("battery", &p, "Now drawing from 'Battery Power' 79%; discharging", 60), Vec::new(), "still true: no new event");
    let fired = r.snapshot("battery", &p, "Now drawing from 'AC Power' 79%; charging", 120);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].message(), "event battery.plugged: The Mac is on power again. (capability battery)");
    assert_eq!(r.snapshot("battery", &p, "'Battery Power' 78%; discharging", 180)[0].event, "unplugged");
    assert!(When::parse("temp > 70").unwrap().holds("temp=71.2'C"));
    assert!(!When::parse("temp > 70").unwrap().holds("temp=48.3'C"));
    assert!(When::parse("state = charged").unwrap().holds("x: 1\nstate: charged"));
    assert_eq!(r.due(180), Vec::new(), "read at 180, every 60s");
    assert_eq!(r.due(240), vec![("battery".to_string(), Provider::Declared)]);
}

#[test]
fn an_actuator_needs_an_allow_rule_and_inputs_are_checked_before_the_call_waits() {
    let mut r = Registry::default();
    r.register(Provider::Declared, vec![declared::parse("gpio.md", GPIO).unwrap().manifest(State::Present)]);
    let mut e = engine(&[("pins", "uses: [gpio]")], r);
    let set = Call { name: "gpio.set".into(), args: vec![("pin".into(), "17".into()), ("level".into(), "dh".into())] };
    let why = check(&e, &set).unwrap().unwrap_err();
    assert!(why.starts_with("gpio.set needs approval"), "{why}");
    e.allow = "gpio.set(*\"pin\":\"17\"*)".into();
    assert_eq!(check(&e, &set), Some(Ok(())));
    let read = Call { name: "gpio.read".into(), args: vec![("pin".into(), "seventeen".into())] };
    assert_eq!(check(&e, &read), Some(Err("pin must be a whole number".into())));
    assert_eq!(check(&e, &Call { name: "open".into(), args: vec![] }), None, "not an outside tool");
}

#[test]
fn an_outside_call_waits_and_its_check_fails_in_the_step() {
    let mut r = Registry::default();
    r.register(Provider::Declared, vec![declared::parse("gpio.md", GPIO).unwrap().manifest(State::Present)]);
    let mut e = engine(&[("pins", "uses: [gpio]")], r);
    e.messages.push(crate::inference::Message { role: crate::inference::Role::User, text: "read pin 17, then set it".into() });
    let answer = "thought: read then set\nact: gpio.read(pin=\"17\"); gpio.set(pin=\"17\", level=\"dh\")";
    let mut external = |_: &Call| None;
    let pending = crate::turn::begin_step(&mut e, answer, &mut external, &crate::turn::NoHost, None);
    assert_eq!(pending.waits.len(), 1, "the read waits; the set is refused before it goes out");
    assert_eq!(pending.waits[0].0.name, "gpio.read");
    let step = crate::turn::finish_step(&mut e, pending, vec![crate::turn::Came::Outside(Some(Ok("17: ip pu | hi".into())))]);
    assert_eq!(step, crate::turn::Step::Again);
    let last = &e.messages.last().unwrap().text;
    assert!(last.contains("gpio.read(pin=\"17\") → 17: ip pu | hi"), "{last}");
    assert!(last.contains("needs approval"), "{last}");
}

// ---- the handshake's messages ----

#[test]
fn a_manifest_round_trips_through_mcp_and_notes_read() {
    let mut c = declared::parse("gpio.md", GPIO).unwrap().manifest(State::Present);
    c.provider = Provider::Remote("pi".into());
    let (tools, resource) = mcp::describe(&c);
    let back = mcp::manifest("pi", &json!({ "tools": tools }), &json!({ "resources": [resource] }));
    assert_eq!(back.len(), 1);
    let b = &back[0];
    assert_eq!((b.id.as_str(), b.doc.as_str(), b.state.clone()), ("gpio", c.doc.as_str(), State::Present));
    assert_eq!(b.tools.iter().map(|t| (t.name.as_str(), t.approval)).collect::<Vec<_>>(), [("read", false), ("set", true)]);
    assert_eq!(b.tools[1].inputs.len(), 2);
    assert_eq!(b.events, c.events);
    assert_eq!(b.snapshot.as_ref().unwrap().uri, "dry://capability/gpio");
    // A tool without `_meta.capability` is an ordinary MCP tool, not a capability's.
    let plain = mcp::manifest("pi", &json!({ "tools": [{ "name": "greet", "inputSchema": {} }] }), &json!({}));
    assert!(plain.is_empty());
    // A tool grouped under a capability whose MCP name is not `<id>.<tool>`.
    let b = mcp::manifest("mac", &json!({ "tools": [{ "name": "browser_open", "_meta": { "capability": "browser" } }] }), &json!({}));
    assert_eq!((b[0].tools[0].name.as_str(), b[0].tools[0].remote.as_str()), ("browser_open", "browser_open"));
    let up = json!({ "jsonrpc": "2.0", "method": "notifications/resources/updated", "params": { "uri": "dry://capability/battery", "_meta": { "event": { "name": "unplugged", "text": "on battery" } } } });
    assert_eq!(mcp::note(&up), mcp::Note::Updated { capability: "battery".into(), event: Some(("unplugged".into(), "on battery".into())) });
    assert_eq!(mcp::note(&json!({ "method": "notifications/tools/list_changed" })), mcp::Note::ListChanged);
    let hello = mcp::Hello {
        runtime: "dry-server".into(),
        platform: "darwin arm64".into(),
        version: "0.1.0".into(),
        protocol: mcp::PROTOCOL.into(),
    };
    let init = mcp::initialize(1, &hello);
    assert_eq!(init.pointer("/params/_meta/hello"), Some(&hello.to_json()));
    assert_eq!(mcp::hello_of(&json!({ "_meta": { "hello": hello.to_json() } })), hello);
    assert_eq!(
        mcp::call_result(&json!({ "result": { "content": [{ "type": "text", "text": "100%" }], "isError": false } })),
        Ok("100%".into())
    );
    assert_eq!(
        mcp::call_result(&json!({ "result": { "content": [{ "type": "text", "text": "no" }], "isError": true } })),
        Err("no".into())
    );
}

// ---- read mode: `uses: [file:read]` ----

#[test]
fn a_read_mode_artifact_offers_its_reading_tools_only_and_says_so() {
    assert_eq!(uses("uses: [file:read]\nroot: ."), Some(vec!["file".to_string()]), "the same capability");
    assert_eq!(uses("kind: file:read\nroot: ."), Some(vec!["file".to_string()]));
    assert_eq!(read_only("uses: [file:read, clock]"), ["file"]);
    assert!(read_only("uses: [file]").is_empty() && read_only("kind: file").is_empty());
    assert_eq!(Live::of("uses: [file:read]"), Some(Live::File));
    assert_eq!(writes("file"), ["edit", "save"]);
    assert!(writes("terminal").is_empty(), "the terminal's guard is on its commands");

    let full = engine(&[("code", "uses: [file]\nroot: ."), ("term", "kind: terminal\nroot: .")], Registry::default());
    let read = engine(&[("code", "uses: [file:read]\nroot: ."), ("term", "uses: [terminal:read]\nroot: .")], Registry::default());
    let (full_tools, read_tools) = (live::toolbox(&full), live::toolbox(&read));
    for t in ["open", "focus", "edit", "save", "run", "start", "background"] {
        assert!(full_tools.contains(&t.to_string()), "{t}");
    }
    for t in ["open", "focus", "run", "start", "background"] {
        assert!(read_tools.contains(&t.to_string()), "{t}: {read_tools:?}");
    }
    for t in ["edit", "save"] {
        assert!(!read_tools.contains(&t.to_string()), "{t}: {read_tools:?}");
    }
    // The prompt and the engine page say the same: no edit or save.
    let p = prompt(&read);
    assert!(!p.contains("- edit(") && !p.contains("- save(") && p.contains("- open("), "{p}");
    assert!(p.contains("### code — files on disk as they are now, and the ones you have open (read-only: file:read"), "{p}");
    let page: Vec<String> = crate::inspect::tool_groups(&read).into_iter().flat_map(|g| g.tools.into_iter().map(|t| t.name)).collect();
    assert!(page.contains(&"open".to_string()) && !page.iter().any(|t| t == "edit" || t == "save"), "{page:?}");
    let shown = crate::inspect::artifacts(&read, &Snapshots::default());
    let code = shown.iter().find(|a| a.name == "code").unwrap();
    assert!(code.tools.iter().any(|t| t.contains("open(")) && !code.tools.iter().any(|t| t.contains("edit(")), "{:?}", code.tools);
}

#[test]
fn a_write_to_a_read_mode_artifact_is_refused_with_why() {
    struct Disk;
    impl crate::live::Host for Disk {
        fn now(&self) -> i64 {
            0
        }
        fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
            Ok(vec!["a.txt".into()])
        }
        fn read(&self, _: &str, _: &str) -> crate::live::Content {
            crate::live::Content::Text("hello\n".into())
        }
        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            panic!("a read-only artifact never writes")
        }
        fn run(&self, _: &crate::live::Job) -> Result<Option<i32>, String> {
            Ok(Some(0))
        }
        fn spawn(&self, _: &crate::live::Job) -> Result<(), String> {
            Ok(())
        }
        fn status(&self, _: &crate::live::Job) -> crate::live::Proc {
            crate::live::Proc::Ended(Some(0))
        }
        fn tail(&self, _: &crate::live::Job, _: usize) -> String {
            String::new()
        }
    }
    let mut e = engine(&[("code", "uses: [file:read]\nroot: .")], Registry::default());
    let open = Call { name: "open".into(), args: vec![("path".into(), "a.txt".into())] };
    assert!(live::call(&mut e, &open, &Disk, None).unwrap().unwrap().starts_with("opened a.txt"));
    assert_eq!(e.artifacts["code"], "uses: [file:read]\nroot: .\nopen: a.txt\n", "the mode survives the kind's writes");
    // A hand-made edit, straight to the artifact: refused before it is held.
    let edit = Call {
        name: "edit".into(),
        args: vec![("path".into(), "a.txt".into()), ("old".into(), "hello".into()), ("new".into(), "bye".into())],
    };
    let why = live::call(&mut e, &edit, &Disk, None).unwrap().unwrap_err();
    assert!(
        why.starts_with("`edit` is refused: your file artifact code is read-only (uses: [file:read]), so it brings open, focus only"),
        "{why}"
    );
    assert!(!e.artifacts["code"].contains("edit:"));
    // The way a model's call goes (not in its toolbox): the same why.
    let why = crate::tools::run(&edit, &live::toolbox(&e), &mut e.artifacts).unwrap_err();
    assert!(why.contains("is read-only (uses: [file:read])"), "{why}");
    let save = Call { name: "save".into(), args: vec![("path".into(), "a.txt".into())] };
    assert!(crate::tools::run(&save, &live::toolbox(&e), &mut e.artifacts).unwrap_err().starts_with("`save` is refused"));
    // A kind: header keeps its mode too.
    let mut k = engine(&[("code", "kind: file:read\nroot: .")], Registry::default());
    live::call(&mut k, &open, &Disk, None).unwrap().unwrap();
    assert!(k.artifacts["code"].starts_with("kind: file:read\n"), "{}", k.artifacts["code"]);
    // Another artifact that holds file with its writes: not refused there.
    assert_eq!(refusal([(&"a".to_string(), &"uses: [file]".to_string())], "edit"), None);
}
