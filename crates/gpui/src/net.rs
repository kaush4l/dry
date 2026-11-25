//! HTTP for inference: the only code that talks to the network. Core's
//! `Inference` builds a `Request` and reads the reply; this sends it.
//! Blocking calls, so they run on GPUI's background executor, never on
//! the thread that draws. A `cli:` URL is a CLI model
//! (`dry_core::cli`): `dry_tools::net` runs its program.

use std::time::Duration;

use dry_core::inference::Request;

/// `Authorization: Bearer <key>` from the named environment variable.
fn bearer(key_env: &Option<String>) -> Result<Option<String>, String> {
    match key_env {
        None => Ok(None),
        Some(var) => {
            std::env::var(var).map(|key| Some(format!("Bearer {key}"))).map_err(|_| format!("environment variable {var} is not set"))
        }
    }
}

/// No timeout on a model call: a local model may load, queue and think
/// for as long as it needs (owner, 2026-11-24). A server that is not
/// there refuses the connection at once.
///
/// POST the request's JSON body; returns the response body, whatever the
/// status (an API error body is still read, so its message can be shown).
pub fn post(req: &Request) -> Result<String, String> {
    if dry_core::cli::is_cli(&req.url) {
        return dry_tools::net::post(req);
    }
    let mut call = ureq::post(&req.url).header("Content-Type", "application/json");
    if let Some(auth) = bearer(&req.key_env)? {
        call = call.header("Authorization", &auth);
    }
    let mut resp = call
        .config()
        .http_status_as_error(false)
        .build()
        .send(req.body.as_str())
        .map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// GET `url`; returns the response body.
pub fn get(url: &str, key_env: &Option<String>) -> Result<String, String> {
    if dry_core::cli::is_cli(url) {
        return dry_tools::net::get(url, key_env);
    }
    let mut call = ureq::get(url);
    if let Some(auth) = bearer(key_env)? {
        call = call.header("Authorization", &auth);
    }
    let mut resp = call
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .call()
        .map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    /// A test against a real model stops somewhere, so it cannot hang;
    /// the app itself has no step limit.
    const TEST_STEPS: usize = 8;

    use dry_core::inference::OpenAi;

    /// The model setting these tests use, read from `models.json`: the one
    /// named in `DRY_TEST_MODEL`, else the default.
    fn model() -> OpenAi {
        let models = crate::store::load_models().expect("models.json");
        let name = std::env::var("DRY_TEST_MODEL").unwrap_or_else(|_| models.default.clone());
        let m = models.get(&name).unwrap_or_else(|| panic!("no model \"{name}\" in models.json"));
        models.inference(m).expect("a usable model setting")
    }

    /// Asks the model's server which models it has. Only lists, so nothing
    /// is loaded. Run with `cargo test -p dry-gpui -- --ignored`.
    #[test]
    #[ignore]
    fn the_server_lists_its_models() {
        let ai = model();
        let body = super::get(&format!("{}/models", ai.base_url), &ai.key_env).expect("server answers");
        let ids = dry_core::inference::parse_model_ids(&body).expect("a model list");
        assert!(!ids.is_empty(), "{body}");
        println!("models: {ids:?}");
    }

    /// Sends one message to the model, through the same path the Test
    /// button uses: core's `OpenAi` builds the request, `post` sends it,
    /// `reply` reads it. On a local server the first call loads the model,
    /// which can take minutes.
    /// `DRY_TEST_MODEL=<name> cargo test -p dry-gpui -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn the_model_answers_one_message() {
        use dry_core::inference::{Inference, Message, Role};
        let ai = model();
        let req = ai.request(&[Message { role: Role::User, text: "Reply with one short sentence to say hello.".into() }]);
        let body = super::post(&req).expect("server answers");
        let reply = ai.reply(&body).unwrap_or_else(|e| panic!("{e:?}: {body}"));
        assert!(!reply.trim().is_empty());
        println!("reply: {reply}");
    }

    /// One message through `prompt::render` with the contract in
    /// `responses.md`, in the format named by `DRY_TEST_FORMAT` (default
    /// its own): does the model's answer read back into the fields?
    /// `cargo test -p dry-gpui -- --ignored --nocapture follows_the_contract`
    #[test]
    #[ignore]
    fn the_model_follows_the_contract() {
        use dry_core::inference::{Inference, Message, Role};
        use dry_core::response::{Format, ResponseFormat};
        let mut e = dry_core::state::Engine::new("test", "You are a helpful assistant.", model());
        let (response, format) = dry_tools::read_responses().expect("responses.md");
        e.response = response;
        e.format = std::env::var("DRY_TEST_FORMAT").ok().and_then(|f| Format::parse_name(&f)).unwrap_or(format);
        e.messages.push(Message { role: Role::User, text: "Say hello in five words.".into() });
        let req = e.inference.request(&dry_core::prompt::render(&e).messages());
        let body = super::post(&req).expect("server answers");
        let text = e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
        println!("raw ({}):\n{text}", e.format.name());
        let answer = e.format.parse(&e.response, &text).unwrap_or_else(|why| panic!("did not follow the contract: {why}"));
        println!("read: {answer:?}");
    }

    /// A whole turn of the loop with the real model, the way the app runs
    /// it: render, send, `turn::step`, again until `reply` (or the limit).
    /// `cargo test -p dry-gpui -- --ignored --nocapture a_real_turn`
    #[test]
    #[ignore]
    fn a_real_turn_loops_until_reply() {
        use dry_core::inference::{Inference, Message, Role};
        use dry_core::turn::{self, Step};
        let mut e = dry_core::state::Engine::new("test", "You are a careful assistant.", model());
        (e.response, e.format) = dry_tools::read_responses().expect("responses.md");
        e.messages.push(Message {
            role: Role::User,
            text: std::env::var("DRY_TEST_TASK").unwrap_or_else(|_| {
                "Save a two-line plan for making tea as an artifact called plan, then tell me you saved it.".into()
            }),
        });
        for n in 1..=TEST_STEPS {
            let req = e.inference.request(&dry_core::prompt::render(&e).messages());
            let body = super::post(&req).expect("server answers");
            let text = e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
            println!("--- step {n}\n{text}");
            match turn::step(&mut e, &text, &mut |_| None) {
                Step::Reply(reply) => {
                    let choices = e.messages.last().map(|m| turn::choices_in(&e, &m.text)).unwrap_or_default();
                    println!("=== reply after {n} step(s): {reply}\nchoices: {choices:?}\nartifacts: {:?}", e.artifacts);
                    return;
                }
                Step::Again | Step::Wait(_) => println!("results: {}", e.messages.last().map_or("", |m| m.text.as_str())),
            }
        }
        panic!("no reply after {} steps", TEST_STEPS);
    }

    /// A real turn that needs an MCP tool: the test server (`calc.add`)
    /// started from an mcp.json, its tools in the prompt, the call routed to
    /// it from inside the loop, the result read by the model.
    /// `cargo test -p dry-gpui -- --ignored --nocapture uses_an_mcp_tool`
    #[test]
    #[ignore]
    fn a_real_turn_uses_an_mcp_tool() {
        use dry_core::inference::{Inference, Message, Role};
        use dry_core::turn::{self, Step};
        let mut e = dry_core::state::Engine::new("test", "You are a careful assistant. Use tools for arithmetic.", model());
        (e.response, e.format) = dry_tools::read_responses().expect("responses.md");
        e.mcp = dry_tools::mcp::test_server_config();
        let (mut clients, tools) = dry_tools::mcp::start_all(&e.mcp).expect("MCP server starts");
        e.mcp_tools = tools;
        e.messages.push(Message { role: Role::User, text: "What is 1234 + 4321? Use calc.add, then tell me.".into() });
        for n in 1..=TEST_STEPS {
            let req = e.inference.request(&dry_core::prompt::render(&e).messages());
            let body = super::post(&req).expect("server answers");
            let text = e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
            println!("--- step {n}\n{text}");
            let tools = e.mcp_tools.clone();
            let mut mcp = |call: &dry_core::calls::Call| {
                let tool = tools.iter().find(|t| t.call_name() == call.name)?;
                Some(clients[0].call(&tool.name, tool.arguments(call)))
            };
            match turn::step(&mut e, &text, &mut mcp) {
                Step::Reply(reply) => {
                    println!("=== reply after {n} step(s): {reply}");
                    assert!(e.messages.iter().any(|m| m.role == Role::Tool && m.text.contains("→ 5555")), "the MCP tool answered");
                    return;
                }
                Step::Again | Step::Wait(_) => println!("results: {}", e.messages.last().map_or("", |m| m.text.as_str())),
            }
        }
        panic!("no reply after {} steps", TEST_STEPS);
    }

    /// The ask and display components with the real model: one situation
    /// per component, each through the real rendered prompt and the loop
    /// (up to 3 steps), checked that the reply's `ask` and `show` read as
    /// meant (a want `show:table` is a display). Prints the pass rate per
    /// component. `DRY_ASK_RUNS` runs each situation that many times
    /// (default 2); `DRY_ASK_ONLY` names the situations to run (a `*` one
    /// never names its control).
    /// `cargo test -p dry-gpui -- --ignored --nocapture the_model_asks`
    #[test]
    #[ignore]
    fn the_model_asks_with_the_right_component() {
        use dry_core::ask::{Ask, Parsed};
        use dry_core::inference::{Inference, Message, Role};
        use dry_core::show::Show;
        use dry_core::turn::{self, Step};
        let situations: &[(&str, &[&str], &str)] = &[
            ("date", &["date", "datetime"], "I want you to run the database backup once, later. Ask me when to schedule it."),
            ("multi", &["multi"], "Write release notes for v2. First ask me which of these sections to include: features, fixes, breaking changes, credits."),
            ("review", &["review"], "Deploy the api service to staging at 18:00 with 2 replicas. Before you do, show me that summary so I can confirm it."),
            ("number", &["number"], "Create some test users for me. Ask me how many first."),
            ("confirm", &["confirm"], "Ask me yes or no: should you delete the build cache?"),
            ("text", &["text"], "I am starting a new project. Ask me for its name."),
            ("choice", &["choice"], "Ask me which one database to use: Postgres, SQLite or MySQL."),
            ("form", &["form"], "Book a table at the restaurant for me. Ask me the day, how many guests, and inside or terrace, all in one go."),
            ("table", &["show:table"], "Show me these users as a table: Ada, 36, London; Bob, 41, Paris; Cy, 29, Rome."),
            ("list", &["show:list"], "Show me the deploy steps as a list: build, test, push, restart."),
            ("progress", &["show:progress"], "The migration has done 45 of 60 tables. Tell me, with a progress bar."),
            ("stat", &["show:stat"], "Give me this week's numbers as stats: revenue $12.4k up 8%, users 1204 down 3%."),
            ("table+choice", &["show:table", "choice"], "Open tickets: #12 login bug (high), #15 typo (low), #19 crash (high). Show them in a table and ask me which one to fix first."),
            // The same kinds, where the task never names the control.
            ("date*", &["date", "datetime"], "Book a dentist appointment for me next week. Find out from me when suits."),
            ("multi*", &["multi"], "Set up CI for my repo. The jobs available are lint, test, build and deploy; I may want several of them. Check with me first."),
            ("review*", &["review"], "Plan a trip: Paris on 3 December, 2 travellers, hotel budget 200 EUR a night. Let me check the details before you book."),
            ("number*", &["number"], "Order pizzas for the team party. Check the amount with me first."),
            ("confirm*", &["confirm"], "I am about to let you wipe the staging database. Check with me before you do it."),
            ("text*", &["text"], "Rename report.txt for me. I will give you the new file name."),
            ("choice*", &["choice"], "Set the colour theme of my site. The themes are light, dark and solarized; I want exactly one."),
            ("form*", &["form"], "Set up my account on the new laptop. I need to give you my full name, my team (design, eng or sales) and my start date. Get it all from me at once."),
            ("table*", &["show:table"], "Compare Postgres, SQLite and MySQL for me: licence, and whether each needs a server."),
            ("progress*", &["show:progress"], "Backup status: 3 of 8 disks copied so far. Report it to me."),
            ("stat*", &["show:stat"], "Summarise this week's dashboard for me: revenue $12.4k (+8% on last week), signups 310 (-5%), churn 2.1% (flat)."),
        ];
        let runs: usize = std::env::var("DRY_ASK_RUNS").ok().and_then(|n| n.parse().ok()).unwrap_or(2);
        let mut table = Vec::new();
        // `DRY_ASK_ONLY=review*,confirm*` runs only those situations.
        let only = std::env::var("DRY_ASK_ONLY").ok();
        let big = |a: &Ask| match a {
            Ask::Multi { options } | Ask::Choice { options } => options.len() >= 3,
            Ask::Review { rows, .. } => rows.len() >= 2,
            Ask::Form { fields, .. } => fields.len() >= 3,
            _ => true,
        };
        let full = |s: &Show| match s {
            Show::Table { rows, .. } => rows.len() >= 3,
            Show::List { items, .. } => items.len() >= 4,
            Show::Stat { stats, .. } => stats.len() >= 2,
            Show::Progress { .. } => true,
        };
        for (name, want, task) in situations {
            if only.as_deref().is_some_and(|o| !o.split(',').any(|n| n == *name)) {
                continue;
            }
            let (shows, asks): (Vec<&str>, Vec<&str>) = want.iter().partition(|w| w.starts_with("show:"));
            let shows: Vec<&str> = shows.iter().map(|w| &w[5..]).collect();
            let mut pass = 0;
            for run in 0..runs {
                let mut e = dry_core::state::Engine::new("test", "You are a helpful assistant.", model());
                (e.response, e.format) = dry_tools::read_responses().expect("responses.md");
                e.messages.push(Message { role: Role::User, text: task.to_string() });
                if *name == "date" && run == 0 && std::env::var("DRY_ASK_PRINT").is_ok() {
                    println!("--- Tools slot as sent:\n{}", dry_core::prompt::render(&e).messages()[0].text);
                }
                let mut got = None;
                for _ in 0..3 {
                    let req = e.inference.request(&dry_core::prompt::render(&e).messages());
                    // A server error (a full context, say) fails this run, not the table.
                    let text = match super::post(&req).map_err(|e| e.to_string()).and_then(|b| e.inference.reply(&b).map_err(|err| format!("{err:?}: {b}"))) {
                        Ok(t) => t,
                        Err(why) => {
                            got = Some((format!("server error: {}", why.chars().take(160).collect::<String>()), None));
                            break;
                        }
                    };
                    if let Step::Reply(_) = turn::step(&mut e, &text, &mut |_| None) {
                        let call = dry_core::ask::reply_call(&e, &text);
                        got = Some((call.as_ref().map(|c| c.to_string()).unwrap_or(text.clone()), call.map(|c| Parsed::of(&c))));
                        break;
                    }
                }
                let ok = match &got {
                    Some((_, Some(p))) => {
                        let ask_ok = if asks.is_empty() { true } else { p.ask.as_ref().is_some_and(|a| asks.contains(&a.name()) && big(a)) };
                        let show_ok = if shows.is_empty() { true } else { p.show.as_ref().is_some_and(|s| shows.contains(&s.name()) && full(s)) };
                        ask_ok && show_ok
                    }
                    _ => false,
                };
                pass += usize::from(ok);
                println!("[{}] {name} #{run}: {}", if ok { "pass" } else { "FAIL" }, got.as_ref().map_or("no reply".to_string(), |(c, p)| {
                    format!("{c}{}", p.as_ref().and_then(|p| p.note()).map(|n| format!("  ({n})")).unwrap_or_default())
                }));
            }
            table.push(format!("{name}: {pass}/{runs}"));
        }
        println!("=== pass rate per component: {}", table.join(", "));
    }

    /// Asks the model's server for its context window, the way the app
    /// does before the first call (`dry_core::context::probes`).
    /// `cargo test -p dry-gpui -- --ignored --nocapture the_server_says_its_context`
    #[test]
    #[ignore]
    fn the_server_says_its_context() {
        use dry_core::context;
        let mut ai = model();
        ai.context = None;
        let found = context::probes(&ai).iter().find_map(|p| {
            let body = match p.post() {
                Some(req) => super::post(&req),
                None => super::get(&p.url, &p.key_env),
            };
            println!("{:?} {} -> {}", p.server, p.url, body.as_deref().map_or_else(|e| e.to_string(), |b| b.chars().take(120).collect()));
            context::read_probe(p.server, &body.ok()?, &ai.model)
        });
        let w = context::Window::of(&ai, found.clone());
        println!("window: {}", w.label());
        assert!(found.is_some(), "the server says the window");
    }

    /// A real compression: a long made-up history against a small window,
    /// summarised by the real model, then one more real step on the
    /// smaller prompt.
    /// `cargo test -p dry-gpui -- --ignored --nocapture a_real_compression`
    #[test]
    #[ignore]
    fn a_real_compression_then_the_turn_goes_on() {
        use dry_core::context::{self, Plan};
        use dry_core::inference::{Inference, Message, Role};
        let mut e = dry_core::state::Engine::new("test", "You are a careful assistant.", model());
        (e.response, e.format) = dry_tools::read_responses().expect("responses.md");
        e.inference.context = Some(3_000);
        for i in 0..18 {
            e.messages.push(Message { role: Role::User, text: format!("Note {i}: the tea for guest {i} is {}.", ["green", "black", "mint"][i % 3]) });
            e.messages.push(Message { role: Role::Assistant, text: format!("thought: noted\nact: reply(text=\"Noted guest {i}. {}\")", "Details kept. ".repeat(30)) });
        }
        e.messages.push(Message { role: Role::User, text: "Which tea does guest 4 get? Reply with one word.".into() });
        let w = context::Window::of(&e.inference, None);
        let plan = context::plan(&e, &dry_core::prompt::render(&e), &w);
        let Plan::Compress(c) = plan else { panic!("over 75% of 3k: compress, got {plan:?}") };
        println!("before: {}", c.budget.line());
        let body = super::post(&e.inference.request(&c.request)).expect("server answers");
        let summary = e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
        println!("summary:\n{summary}");
        println!("{}", context::apply(&mut e, &c, &summary).expect("applied"));
        let r = dry_core::prompt::render(&e);
        let after = context::measure(&r, &w);
        println!("after: {}", after.line());
        assert!(!after.over());
        let body = super::post(&e.inference.request(&r.messages())).expect("server answers");
        let text = e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
        println!("next step:\n{text}");
        let step = dry_core::turn::step(&mut e, &text, &mut |_| None);
        println!("step: {step:?}");
    }
    /// Engines as tools with the real model, driven the way the window
    /// drives them: a parent with an engines artifact makes a child with a
    /// goal through `engine_call`, the child's turn runs, its reply becomes
    /// the parent's result (`engines::settle`), and the parent answers.
    /// `cargo test -p dry-gpui -- --ignored --nocapture a_real_parent_asks_a_child`
    #[test]
    #[ignore]
    fn a_real_parent_asks_a_child_and_reads_its_reply() {
        use dry_core::engines::{self, Seen};
        use dry_core::inference::{Inference, Role};
        use dry_core::state::State;
        use dry_core::turn::{self, Step};
        let mut s = State::default();
        let p = s.add_engine();
        let e = &mut s.engines[p];
        e.inference = model();
        (e.response, e.format) = dry_tools::read_responses().expect("responses.md");
        e.description = "## Goal\nYou coordinate: you hand arithmetic to a child engine and pass on its answer.\n\n## Scope\nNever work a sum out yourself: ask a child with engine_call, then reply with what it said.".into();
        e.artifacts.insert("kids".into(), "kind: engines".into());
        let task = std::env::var("DRY_TEST_TASK").unwrap_or_else(|_| {
            "Make a child engine named calc whose goal is to answer arithmetic questions; ask it with engine_call what 17 times 3 is, then tell me its answer.".into()
        });
        s.add_text(p, &task);
        let idle = |_: usize| Seen { label: "idle".into(), ..Seen::default() };
        // Engines with a turn to run, the newest first (a child before the
        // parent that waits for it).
        let mut queue = vec![p];
        for _ in 0..TEST_STEPS * 3 {
            let Some(&i) = queue.last() else { break };
            let mut fam = engines::family(&s, i, &idle);
            let at = dry_core::live::Snapshots { clock: Some(dry_tools::live::clock()), host: None, family: Some(&fam) };
            let req = s.engines[i].inference.request(&dry_core::prompt::render_with(&s.engines[i], &at).messages());
            let body = super::post(&req).expect("server answers");
            let text = s.engines[i].inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
            println!("--- {}\n{text}", s.engines[i].id);
            let step = turn::step_in(&mut s.engines[i], &text, &mut |_| None, &turn::NoHost, Some(&mut fam));
            println!("results: {}", s.engines[i].messages.last().map_or("", |m| m.text.as_str()));
            let applied = engines::apply(&mut s, i, fam.effects);
            match step {
                Step::Again => {}
                Step::Wait(_) => queue.retain(|q| *q != i),
                Step::Reply(reply) if i == p => {
                    println!("=== parent replied: {reply}");
                    let child = s.engines.iter().find(|c| c.parent.as_deref() == Some(s.engines[p].id.as_str())).expect("a child was made");
                    println!("child {} goal.md:\n{}", child.id, child.description);
                    assert!(!engines::replies(child).is_empty(), "the child replied");
                    assert!(s.engines[p].messages.iter().any(|m| m.role == Role::Tool && m.text.contains(" replied: ")), "the parent read its reply");
                    return;
                }
                Step::Reply(_) => {
                    queue.retain(|q| *q != i);
                    let parent = s.engines[i].parent.clone().and_then(|id| s.index_of(&id)).expect("a child has a parent");
                    let settled = engines::settle(&mut s, parent, &idle);
                    if !settled.wake.is_empty() {
                        s.add_wake(parent, &settled.wake.join("\n"));
                    }
                    if (settled.resumed || !settled.wake.is_empty()) && !queue.contains(&parent) {
                        queue.push(parent);
                    }
                }
            }
            for c in applied.run {
                if !queue.contains(&c) {
                    queue.push(c);
                }
            }
        }
        panic!("no reply from the parent after {} steps in all", TEST_STEPS * 3);
    }

    /// One step, many calls, with the real model (docs/engine-standard.md,
    /// "One step, many calls"): the parent's one act opens several files and starts two
    /// `engine_call` explorations. The children run at the same time (one
    /// thread each, the way the window runs every engine's turn) while
    /// the act's waiting calls wait, and both replies arrive in the
    /// parent's one tool message, in written order.
    /// `cargo test -p dry-gpui -- --ignored --nocapture one_act_explores_two_ways`
    #[test]
    #[ignore]
    fn one_act_explores_two_ways_at_once() {
        use dry_core::engines::{self, Seen};
        use dry_core::inference::{Inference, Role};
        use dry_core::state::{Engine, State};
        use dry_core::turn::{self, Step};
        use std::time::Instant;
        let dir = std::env::var("DRY_TEST_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("dry-one-act"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("repo/notes")).unwrap();
        for (name, text) in [("a.txt", "apples: 3\n"), ("b.txt", "bananas: 5\n"), ("c.txt", "cherries: 7\n")] {
            std::fs::write(dir.join("repo/notes").join(name), text).unwrap();
        }
        let disk = dry_tools::live::Disk { repo: dir.join("repo").canonicalize().unwrap(), engines: dir.join("engines") };
        let mut s = State::default();
        let p = s.add_engine();
        let e = &mut s.engines[p];
        e.inference = model();
        (e.response, e.format) = dry_tools::read_responses().expect("responses.md");
        e.description = "## Goal\nYou coordinate. You do several things in one step: every call in one act starts at once.\n\n## Scope\nNever answer a question a child was asked yourself: ask children with engine_call, then reply with what they said.".into();
        e.artifacts.insert("kids".into(), "kind: engines".into());
        e.artifacts.insert("code".into(), "kind: file\nroot: .".into());
        let task = std::env::var("DRY_TEST_TASK").unwrap_or_else(|_| {
            "In ONE act (one step, all calls together): open notes/a.txt, notes/b.txt and notes/c.txt with three open calls, and ask two child engines at the same time with two engine_call calls: one named poet whose goal is to write a four-word poem about fruit, and one named mathcalc whose goal is to answer what 12 times 12 is. When both replied, reply to me with both answers.".into()
        });
        s.add_text(p, &task);
        let idle = |_: usize| Seen { label: "idle".into(), ..Seen::default() };
        let ask = |e: &Engine, fam: &engines::Family| -> String {
            let at = dry_core::live::Snapshots { clock: Some(dry_tools::live::clock()), host: Some(&disk), family: Some(fam) };
            let req = e.inference.request(&dry_core::prompt::render_with(e, &at).messages());
            let body = super::post(&req).expect("server answers");
            e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"))
        };
        let mut both_in_one = false;
        for n in 1..=TEST_STEPS {
            let fam = engines::family(&s, p, &idle);
            let text = ask(&s.engines[p], &fam);
            println!("--- parent step {n}\n{text}");
            let mut fam = fam;
            let pending = turn::begin_step(&mut s.engines[p], &text, &mut |_| None, &disk, Some(&mut fam));
            let applied = engines::apply(&mut s, p, fam.effects);
            let kids: Vec<(usize, Engine, engines::Family)> = applied.run.iter().map(|&c| (c, s.engines[c].clone(), engines::family(&s, c, &idle))).collect();
            let started = Instant::now();
            let (came, done) = std::thread::scope(|scope| {
                let turns: Vec<_> = kids
                    .into_iter()
                    .map(|(c, mut k, mut f)| {
                        let ask = &ask;
                        let disk = &disk;
                        scope.spawn(move || {
                            let at = Instant::now();
                            let mut replied = false;
                            for _ in 0..TEST_STEPS {
                                let text = ask(&k, &f);
                                if matches!(turn::step_in(&mut k, &text, &mut |_| None, disk, Some(&mut f)), Step::Reply(_)) {
                                    replied = true;
                                    break;
                                }
                            }
                            println!("child {} {} after {} ms", k.id, if replied { "replied" } else { "gave up" }, at.elapsed().as_millis());
                            (c, k, f.effects, replied)
                        })
                    })
                    .collect();
                let came = dry_tools::live::run_waits(&pending.waits, &|_: &dry_core::calls::Call| None, &disk);
                (came, turns.into_iter().map(|t| t.join().expect("a child's turn")).collect::<Vec<_>>())
            });
            if !done.is_empty() {
                println!("{} children together: {} ms", done.len(), started.elapsed().as_millis());
            }
            let failed: Vec<usize> = done.iter().filter(|d| !d.3).map(|d| d.0).collect();
            for (c, k, effects, _) in done {
                s.engines[c] = k;
                engines::apply(&mut s, c, effects);
            }
            let step = turn::finish_step(&mut s.engines[p], pending, came);
            if let Step::Wait(_) = step {
                let seen = |j: usize| Seen { failed: failed.contains(&j).then(|| "no reply in the test's steps".to_string()), ..idle(j) };
                let settled = turn::callbacks(&mut s, p, false, &disk, &seen);
                assert!(settled.resumed, "every awaited child answered");
            }
            let tool = s.engines[p].messages.iter().rev().find(|m| m.role == Role::Tool).map(|m| m.text.clone()).unwrap_or_default();
            println!("results:\n{tool}");
            let replies = tool.lines().filter(|l| l.contains(" replied: ")).count();
            let opens = tool.lines().filter(|l| l.starts_with("open(")).count();
            if replies >= 2 {
                println!("=== one tool message: {replies} replies, {opens} opens");
                both_in_one = true;
            }
            if let Step::Reply(reply) = step {
                println!("=== parent replied after {n} step(s): {reply}");
                assert!(both_in_one, "both children's replies came in one tool message");
                return;
            }
        }
        panic!("no reply from the parent after {TEST_STEPS} steps");
    }
}
