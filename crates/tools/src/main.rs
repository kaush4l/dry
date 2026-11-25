//! `dry`: the command line for the files the app is built from. Every
//! read and write goes through the library (`src/lib.rs`), which the app
//! uses too; the app notices changed files within a second and redraws.
//!
//! ```text
//! cargo run -p dry-tools -- <command>
//!
//! routes                                 list every route (as the panel shows it)
//! page <path> <title> [--icon ✎] [--nav yes|no|per-engine] [--order N]
//!                                        create a route, or change its title and options
//! add <path> <component...>              add a component to a route's view
//! remove <path>                          delete a route (its page.json)
//! components                             list component files
//! component <name> [<component...>]      create a component file, or add to it
//! export [file]                          every route and component as one JSON
//! import <file>                          create the routes/ and components/ files back from it
//! themes                                 list the themes (themes/<name>/), * marks the active one
//! theme use <name>                       switch the app to it (app.md `theme:`)
//! scenes                                 list the scenes (scenes/<name>/), checked; * marks the one in force
//! scene use <name>|none                  the app's scene (app.md `scene:`; state/local.md's ## Scene row beats it)
//! theme|scene show <name>                its main file (theme.md, scene.md)
//! theme|scene new <name> [--from <name>] a new one, a copy of another's whole folder to edit
//! theme|scene set <name> <key> <value> [light|dark]
//!                                        one value: a theme's token; a scene's setting or palette key
//! theme|scene rm <name>                  delete its folder (not the one in use, not the default theme)
//! theme|scene export <name> [<file>]     its folder as one file to share (printed without <file>)
//! theme|scene import <file> [--replace]  a shared one back as its folder
//! motion <seconds>                       the app's motion (app.md `motion:`; 0 keeps the backdrop and scene still)
//! mcp                                    serve the app tools as an MCP server on stdio
//! mcp <group>                            serve one tool group: app (the default), settings, all, engines
//! snapshot --engine <id> [--since <cursor>] [--cursor <file>] [--start]
//!                                        a worker's live snapshot (what its hooks add to a turn;
//!                                        through the host when one runs, else the store)
//! models [<setting>|--no-ask]            the models.json settings, and what their servers report
//!                                        (state, context, family, quantization...; unknown where none says)
//! engine <engine>                        an engine's model, endpoint and sampling settings
//! engine keys                            the sampling settings: range, meaning, which servers honour them
//! engine use <engine> <setting> [<model>]  point it at a models.json setting (and a model id on it)
//! engine model <engine> <model id>       change only the model id, on its own server
//! engine set <engine> <key> <value...>   e.g. temperature 0.2; stop END STOP; param.min_p 0.05; default unsets
//! engine unset <engine> <key>            back to the server's default
//! evidence [--project <dir>] [--update] [--expect a,b] [--out <dir>] [--repeat N] [--only a]
//!                                        build and run the project, shoot its evidence.md
//!                                        scenarios in headless Chrome, compare with evidence/
//!                                        (docs/evidence.md); exit 1 on an unexpected change
//! companion [--folder DIR] [--port N] [--yes] …
//!                                        the companion: one folder, its terminal, models and
//!                                        capabilities for the web app (docs/companion-protocol.md)
//! capabilities                           every capability on this machine: provider, state, tools
//!                                        (compiled, and capabilities/*.md probed; docs/capabilities.md)
//! discover [--write <dir>]               propose capability files for this machine (printed, or
//!                                        written to <dir> to review; never enables an actuator)
//! tool [<name> [<json args>]]            list the app tools, or run one
//! prompt [<engine id>]                   the exact prompt the engine's model sees next
//!                                        (no id: a new engine with the defaults)
//! store [md:<folder>|sqlite:<file>]    the engines in a store (default: DRY_STORE or md:state)
//! store copy <from> <to>                 copy every engine from one store into another
//! default diff [<store>]                 which of default/'s files the store overrides, hides or holds unchanged
//! reset [<store>] [--yes]                Reset history: move everything the store holds (engines, messages,
//!                                        logs, spans) to <store>.reset-<UTC stamp> beside it, so the next
//!                                        load is default/ alone; keeps state/local.md. Without --yes it
//!                                        only prints what it would move (docs/default.md, "Reset")
//! skills <engine>                        an engine's skills: name, description, tools
//! skill import <engine> <path>           copy an existing skill (its folder or SKILL.md) into an engine
//! tokens [light|dark]                    every design token of the active theme, with colour swatches
//! token get <name>
//! token set <name> <value> [light|dark]  a colour without a mode sets both
//! token reset <name>|--all               back to the default
//!
//! <component...> is one of:
//!   heading <text> | text <text> | link <label> <to> | use <component>
//!   chat | tokens | appearance | models | routes | diagnostics | new-route
//!   engines | metadata | tools | capabilities | response | artifacts
//!   explorer | editor | terminal | theme-picker
//!   (panes are laid out by hand in page.json: {"type":"panes","panes":[...]})
//! ```

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use dry_core::pages::{self, Component, Page, Snapshot};
use dry_core::routes::Nav;
use dry_core::look::Kind;
use dry_core::theme::{self, COLOR_NAMES, FONT, Mode, NUMBER_NAMES, SCENE};
use dry_tools::{self as files, Result};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // Evidence has its own exit codes (1: a change, 2: could not run).
    if let ["evidence", rest @ ..] = args.as_slice() {
        return files::evidence::command(rest);
    }
    // The companion serves until it is stopped (docs/companion-protocol.md).
    if let ["companion", rest @ ..] = args.as_slice() {
        return files::companion::command(rest);
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dry: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result {
    match args {
        ["routes"] => list_routes(),
        ["capabilities"] => files::capability::list_command(),
        ["discover", rest @ ..] => files::capability::discover_command(rest),
        ["page", path, title, opts @ ..] => page(path, title, opts),
        ["add", path, component @ ..] => {
            let c = parse_component(component)?;
            files::add_component(path, c.clone())?;
            println!("{}: + {}", files::pattern(path), describe(&c));
            Ok(())
        }
        ["remove", path] => {
            println!("removed {}", files::delete_page(path)?);
            Ok(())
        }
        ["components"] => list_components(),
        ["component", name, component @ ..] => {
            let c = if component.is_empty() { None } else { Some(parse_component(component)?) };
            files::add_to_component(name, c)?;
            println!("{} (use it with `dry add <path> use {name}`)", files::component_file(name).display());
            Ok(())
        }
        ["export"] => {
            print!("{}", files::snapshot()?.to_json());
            Ok(())
        }
        ["export", file] => {
            fs::write(file, files::snapshot()?.to_json()).map_err(|e| e.to_string())?;
            println!("wrote {file}");
            Ok(())
        }
        ["import", file] => {
            let text = fs::read_to_string(Path::new(file)).map_err(|e| format!("{file}: {e}"))?;
            for line in files::import(&Snapshot::from_json(&text)?)? {
                println!("{line}");
            }
            Ok(())
        }
        ["themes"] => {
            let active = files::active_theme();
            for name in files::list_themes() {
                println!("{} {name}", if name == active { "*" } else { " " });
            }
            Ok(())
        }
        ["theme", "use", name] => {
            files::set_active_theme(name)?;
            println!("using theme {name} ({})", files::theme_file().display());
            Ok(())
        }
        ["scenes"] => list_scenes(),
        ["scene", "use", name] => scene_setting(Some(name), None),
        ["motion", secs] => scene_setting(None, Some(secs)),
        [kind @ ("theme" | "scene"), rest @ ..] => look_command(Kind::parse(kind).unwrap(), rest),
        ["mcp"] => files::app_tools::serve(),
        ["mcp", group] => files::app_tools::serve_tools(files::app_tools::group(group)?),
        ["snapshot", rest @ ..] => print(files::worker::snapshot_command(rest)),
        ["models"] => print(files::settings::models_report(&files::settings::read_models()?, None, true)),
        ["models", "--no-ask"] => print(files::settings::models_report(&files::settings::read_models()?, None, false)),
        ["models", setting] => print(files::settings::models_report(&files::settings::read_models()?, Some(setting), true)),
        ["engine", "keys"] => print(Ok(dry_core::sampling::describe_keys())),
        ["engine", "use", engine, setting] => engine_model(engine, Some(setting), None),
        ["engine", "use", engine, setting, model] => engine_model(engine, Some(setting), Some(model)),
        ["engine", "model", engine, model] => engine_model(engine, None, Some(model)),
        ["engine", "set", engine, key, value @ ..] if !value.is_empty() => {
            let store = files::store::open_default()?;
            print(files::settings::set_sampling(&*store, engine, &[(key.to_string(), files::settings::value_from_words(value))]))
        }
        ["engine", "unset", engine, key] => {
            let store = files::store::open_default()?;
            print(files::settings::set_sampling(&*store, engine, &[(key.to_string(), None)]))
        }
        ["engine", engine] => {
            let state = files::store::load_or_first(&*files::store::open_default()?)?;
            print(Ok(files::settings::engine_report(&state.engines[files::settings::find(&state, engine)?])))
        }
        ["tool"] => {
            for t in files::app_tools::all() {
                println!("{:<18} {}\n{:<18} {}", t.name, t.description, "", (t.schema)()["properties"]);
            }
            Ok(())
        }
        ["tool", name] => {
            println!("{}", files::app_tools::run(name, &serde_json::json!({}))?);
            Ok(())
        }
        ["tool", name, json] => {
            let args = serde_json::from_str(json).map_err(|e| format!("arguments: {e}"))?;
            println!("{}", files::app_tools::run(name, &args)?);
            Ok(())
        }
        ["prompt"] => show_prompt(None),
        ["prompt", id] => show_prompt(Some(id)),
        ["store"] => show_store(None),
        ["store", "copy", from, to] => {
            let state = files::store::open(from)?.load()?;
            files::store::open(to)?.save(&state)?;
            println!("copied {} engine(s) from {from} to {to}", state.engines.len());
            Ok(())
        }
        ["store", spec] => show_store(Some(spec)),
        ["default", "diff"] => print(files::store::default_diff(None)),
        ["default", "diff", spec] => print(files::store::default_diff(Some(spec))),
        ["reset"] => reset(None, false),
        ["reset", "--yes"] => reset(None, true),
        ["reset", spec] => reset(Some(spec), false),
        ["reset", spec, "--yes"] | ["reset", "--yes", spec] => reset(Some(spec), true),
        ["skills", engine] => skills(engine),
        ["skill", "import", engine, path] => skill_import(engine, path),
        ["tokens"] => tokens(None),
        ["tokens", mode] => tokens(Some(parse_mode(mode)?)),
        ["token", "get", name] => token_get(name),
        ["token", "set", name, value] => token_set(name, value, None),
        ["token", "set", name, value, mode] => token_set(name, value, Some(parse_mode(mode)?)),
        ["token", "reset", "--all"] => token_reset(None),
        ["token", "reset", name] => token_reset(Some(name)),
        _ => Err(format!("unknown command {args:?}; see the list at the top of crates/tools/src/main.rs")),
    }
}

/// `dry reset [<store>] [--yes]`: what Reset history would move, or (with
/// `--yes`) do it. While a host runs for the store it is the one writer:
/// the reset goes through it (`POST /api/reset`), so every page reloads.
fn reset(spec: Option<&str>, yes: bool) -> Result {
    let spec = spec.map_or_else(files::store::default_spec, str::to_string);
    let plan = files::store::reset_plan(&spec)?;
    let lock = files::host::current_at(&files::host::lock_path(&plan.store));
    if !yes || plan.is_empty() {
        println!("{}", plan.describe());
        if !plan.is_empty() {
            println!("(nothing changed: run `dry reset{} --yes` to do it)", if spec == files::store::default_spec() { String::new() } else { format!(" {spec}") });
        }
        return Ok(());
    }
    if let Some(lock) = lock {
        let backup = files::host::reset(&lock)?;
        println!("reset through the host at {}: the history is in {}", lock.url, backup.as_deref().unwrap_or("(nothing moved)"));
        return Ok(());
    }
    let done = files::store::reset(&spec)?;
    println!("reset {}: the history is in {}; the next load is default/ alone", done.store, done.backup.display());
    Ok(())
}

// ---- models and engine settings ---------------------------------------------

fn print(text: Result<String>) -> Result {
    println!("{}", text?);
    Ok(())
}

fn engine_model(engine: &str, setting: Option<&str>, model: Option<&str>) -> Result {
    let store = files::store::open_default()?;
    print(files::settings::set_model(&*store, &files::settings::read_models()?, engine, setting, model))
}

// ---- routes and components -------------------------------------------------

fn list_routes() -> Result {
    let loaded = files::load();
    for e in &loaded.errors {
        eprintln!("unreadable: {e}");
    }
    for route in pages::table(&loaded.pages).list() {
        let page = &loaded.pages.iter().find(|(p, _)| *p == route.pattern).expect("from these pages").1;
        let nav = match (route.nav, route.moved.as_deref()) {
            (_, Some(to)) => format!("  (moved to {to})"),
            (Nav::Yes, _) => String::new(),
            (Nav::No, _) => "  (not in panel)".into(),
            (Nav::PerEngine, _) => "  (one button per engine)".into(),
        };
        println!("{}  {:<16} {}{nav}", route.icon, route.pattern, route.title);
        for c in &page.view {
            println!("      {}", describe(c));
        }
    }
    Ok(())
}

fn describe(c: &Component) -> String {
    match c {
        Component::Heading { text } => format!("heading  {text}"),
        Component::Text { text } => format!("text     {text}"),
        Component::Link { label, to } => format!("link     {label} → {to}"),
        Component::Use { name } => format!("use      {name}"),
        Component::EngineHeader => "engine-header".into(),
        Component::Chat => "chat".into(),
        Component::Composer { .. } => "composer".into(),
        Component::Tokens => "tokens".into(),
        Component::Appearance => "appearance".into(),
        Component::ResetHistory => "reset-history".into(),
        Component::Models => "models".into(),
        Component::Routes => "routes".into(),
        Component::Diagnostics => "diagnostics".into(),
        Component::Environment => "environment".into(),
        Component::NewRoute => "new-route".into(),
        Component::Engines => "engines".into(),
        Component::Desk => "desk".into(),
        Component::Session => "session".into(),
        Component::Dashboard => "dashboard".into(),
        Component::Metadata => "metadata".into(),
        Component::Tools => "tools".into(),
        Component::Capabilities => "capabilities".into(),
        Component::Response => "response".into(),
        Component::Artifacts => "artifacts".into(),
        Component::EngineTools => "engine-tools".into(),
        Component::Prompt => "prompt".into(),
        Component::ArtifactPage => "artifact-page".into(),
        Component::Columns { main, side, .. } => format!("columns  {} main, {} side", main.len(), side.len()),
        Component::Panes { axis, panes, .. } => format!("panes    {axis:?}, {} panes", panes.len()),
        Component::Explorer => "explorer".into(),
        Component::Editor => "editor".into(),
        Component::Terminal => "terminal".into(),
        Component::ThemePicker => "theme-picker".into(),
        Component::LiveView { part } => format!("live-view {}", part.as_str()),
        Component::Ask { ask } => format!("ask      {}", ask.name()),
        Component::Show { show } => format!("show     {}", show.name()),
        Component::Analytics { part } => format!("analytics {}", part.as_str()),
    }
}

/// `heading <text>`, `link <label> <to>`, `chat`, ... as a component.
fn parse_component(args: &[&str]) -> Result<Component> {
    let text = |rest: &[&str]| rest.join(" ");
    let c = match args {
        ["heading", rest @ ..] if !rest.is_empty() => Component::Heading { text: text(rest) },
        ["text", rest @ ..] if !rest.is_empty() => Component::Text { text: text(rest) },
        ["link", label, to] => Component::Link { label: label.to_string(), to: files::pattern(to) },
        ["use", name] => Component::Use { name: name.to_string() },
        ["engine-header"] => Component::EngineHeader,
        ["chat"] => Component::Chat,
        ["composer"] => Component::Composer { actions: dry_core::composer::default_actions() },
        ["tokens"] => Component::Tokens,
        ["appearance"] => Component::Appearance,
        ["reset-history"] => Component::ResetHistory,
        ["models"] => Component::Models,
        ["routes"] => Component::Routes,
        ["diagnostics"] => Component::Diagnostics,
        ["environment"] => Component::Environment,
        ["new-route"] => Component::NewRoute,
        ["engines"] => Component::Engines,
        ["desk"] => Component::Desk,
        ["session"] => Component::Session,
        ["dashboard"] => Component::Dashboard,
        ["metadata"] => Component::Metadata,
        ["tools"] => Component::Tools,
        ["capabilities"] => Component::Capabilities,
        ["response"] => Component::Response,
        ["artifacts"] => Component::Artifacts,
        ["engine-tools"] => Component::EngineTools,
        ["prompt"] => Component::Prompt,
        ["artifact-page"] => Component::ArtifactPage,
        ["explorer"] => Component::Explorer,
        ["editor"] => Component::Editor,
        ["terminal"] => Component::Terminal,
        ["theme-picker"] => Component::ThemePicker,
        ["analytics", part] => match dry_core::analytics::Part::parse(part) {
            Some(part) => Component::Analytics { part },
            None => return Err(format!("not an analytics part: {part} (timeline | today | page)")),
        },
        ["live-view", part] => match dry_core::liveview::Part::parse(part) {
            Some(part) => Component::LiveView { part },
            None => return Err(format!("not a live-view part: {part} (explorer | editor | panel | chat | status)")),
        },
        // `ask <kind> [key=value ...]`, read like a reply's arguments.
        ["ask", kind, rest @ ..] => {
            let mut call = dry_core::calls::Call { name: "reply".into(), args: vec![("ask".into(), kind.to_string())] };
            call.args.extend(rest.iter().filter_map(|a| a.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())));
            let parsed = dry_core::ask::Parsed::of(&call);
            match parsed.ask {
                Some(ask) => Component::Ask { ask },
                None => return Err(parsed.notes.join("; ")),
            }
        }
        // `show <kind> data=... [caption=...]`, read like a reply's arguments.
        ["show", kind, rest @ ..] => {
            let mut call = dry_core::calls::Call { name: "reply".into(), args: vec![("show".into(), kind.to_string())] };
            call.args.extend(rest.iter().filter_map(|a| a.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())));
            let parsed = dry_core::ask::Parsed::of(&call);
            match parsed.show {
                Some(show) => Component::Show { show },
                None => return Err(parsed.notes.join("; ")),
            }
        }
        _ => {
            return Err(format!(
                "not a component: {args:?} (heading <text> | text <text> | link <label> <to> | use <name> | chat | composer | tokens | appearance | models | routes | diagnostics | new-route | dashboard | engines | metadata | tools | response | artifacts | engine-header | engine-tools | prompt | artifact-page | explorer | editor | terminal | theme-picker | live-view <part> | ask <kind> [key=value ...] | show <kind> data=...)"
            ));
        }
    };
    Ok(c)
}

fn page(path: &str, title: &str, opts: &[&str]) -> Result {
    let pattern = files::pattern(path);
    let existed = files::page_file(&pattern).exists();
    let mut page = if existed { files::read_page(&pattern)? } else { Page::new(title) };
    page.title = title.to_string();
    let mut opts = opts.iter();
    while let Some(opt) = opts.next() {
        let value = opts.next().ok_or_else(|| format!("{opt} needs a value"))?;
        match *opt {
            // The glyph itself: `--icon ✎`.
            "--icon" => page.icon = value.to_string(),
            "--nav" => {
                page.nav = match *value {
                    "yes" => Nav::Yes,
                    "no" => Nav::No,
                    "per-engine" => Nav::PerEngine,
                    _ => return Err("--nav is yes, no or per-engine".into()),
                }
            }
            "--order" => page.order = value.parse().map_err(|_| "--order is a whole number".to_string())?,
            _ => return Err(format!("unknown option {opt}")),
        }
    }
    files::write_page(&pattern, &page)?;
    println!("{} {pattern} ({})", if existed { "updated" } else { "created" }, files::page_file(&pattern).display());
    Ok(())
}

fn list_components() -> Result {
    let loaded = files::load();
    for (name, view) in &loaded.components {
        println!("{name}");
        for c in view {
            println!("      {}", describe(c));
        }
    }
    Ok(())
}

// ---- the prompt ----------------------------------------------------------------------

/// Print exactly what the engine's model gets next: the rendered prompt,
/// with the tools its MCP servers offer (they are started to ask them).
fn show_prompt(id: Option<&str>) -> Result {
    let mut e = match id {
        Some(id) => {
            let state = files::store::load_or_first(&*files::store::open_default()?)?;
            state.engines.into_iter().find(|e| e.id == id).ok_or_else(|| format!("no engine {id} in the store"))?
        }
        None => {
            let mut state = dry_core::state::State::default();
            let i = state.add_engine();
            let mut e = state.engines.remove(i);
            (e.response, e.format) = files::read_responses()?;
            e.mcp = files::default_mcp().unwrap_or_default();
            e.messages.push(dry_core::inference::Message { role: dry_core::inference::Role::User, text: "(your next message)".into() });
            e
        }
    };
    let (_clients, tools) = files::mcp::start_all(&e.mcp)?;
    e.mcp_tools = tools;
    // This machine's capabilities, as a shell here equips it, with the
    // subscriptions probed and their usage from the store's records.
    let mut reg = files::capability::local_cached().0.clone();
    let mut subs = files::subscriptions::probe();
    if let Ok(store) = files::store::open_default() {
        subs.usage(&files::subscriptions::recent(files::spans::load_all(&store.files_dir()).0.values()));
    }
    reg.subscriptions = Some(subs);
    e.machine = dry_core::capability::Machine::new(reg);
    println!("{}", dry_core::prompt::render(&e).to_text());
    Ok(())
}

// ---- stores ----------------------------------------------------------------------

/// The engine `id` in the default store, and the store with everything.
fn engine_in_store(id: &str) -> Result<(Box<dyn files::store::Store>, dry_core::state::State, usize)> {
    let store = files::store::open_default()?;
    let state = files::store::load_or_first(&*store)?;
    let i = state.engines.iter().position(|e| e.id == id).ok_or_else(|| {
        format!(
            "no engine `{id}` in {}; they are {}",
            store.describe(),
            state.engines.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(", ")
        )
    })?;
    Ok((store, state, i))
}

fn skills(id: &str) -> Result {
    let (_, state, i) = engine_in_store(id)?;
    let e = &state.engines[i];
    if e.skills.is_empty() {
        println!("{id}: no skills");
    }
    for (name, text) in &e.skills {
        match dry_core::skill::parse(text) {
            Ok(s) => println!(
                "{name:<20} {}{}",
                s.description,
                if s.tools.is_empty() { String::new() } else { format!("  [tools: {}]", s.tools.join(", ")) }
            ),
            Err(why) => println!("{name:<20} (does not read: {why})"),
        }
    }
    Ok(())
}

/// Copy an existing skill into engine `id`: `path` is its folder (holding
/// SKILL.md) or the SKILL.md itself. Only SKILL.md is kept: other files in
/// the folder are not copied (said, so nothing is lost silently).
fn skill_import(id: &str, path: &str) -> Result {
    let path = std::path::Path::new(path);
    let (file, dir) = if path.is_dir() { (path.join("SKILL.md"), Some(path)) } else { (path.to_path_buf(), path.parent()) };
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let skill = dry_core::skill::parse(&text)?;
    if !dry_core::skill::valid_name(&skill.name) {
        return Err(format!("skill name `{}`: lower case letters, digits, - and _", skill.name));
    }
    let (store, mut state, i) = engine_in_store(id)?;
    let replaced = state.engines[i].skills.insert(skill.name.clone(), text).is_some();
    store.save(&state)?;
    println!("{} skill {} in {id} ({})", if replaced { "updated" } else { "imported" }, skill.name, skill.description);
    let others: Vec<String> = dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n != "SKILL.md").collect())
        .unwrap_or_default();
    if !others.is_empty() {
        println!("not copied (only SKILL.md is kept): {}", others.join(", "));
    }
    Ok(())
}

fn show_store(spec: Option<&str>) -> Result {
    let store = match spec {
        Some(s) => files::store::open(s)?,
        None => files::store::open_default()?,
    };
    if !store.exists() {
        println!("{}: holds no state of its own yet (docs/default.md §1)", store.describe());
    }
    let state = if spec.is_some() { store.load()? } else { files::store::load_or_first(&*store)? };
    println!("{} · {} mode", store.describe(), state.mode.as_str());
    for e in &state.engines {
        println!(
            "  {:<16} {:<8} {} message(s) · {} artifact(s) · tools: {}{}",
            e.id,
            e.kind(),
            e.messages.len(),
            e.artifacts.len(),
            e.tools.join(","),
            if e.mcp.trim().is_empty() { "" } else { " · mcp.json" }
        );
    }
    Ok(())
}

// ---- themes and tokens -----------------------------------------------------------

/// The scene settings in force: the store's app.md (over the default)
/// with this machine's `state/local.md` over it.
fn scene_settings() -> Result<dry_core::scene::Settings> {
    let state = files::store::load_or_first(&*files::store::open_default()?)?;
    let local = files::read_local_theme()?;
    Ok(dry_core::scene::settings(&state, &files::read_theme()?, &local.iter().collect::<Vec<_>>()))
}

fn list_scenes() -> Result {
    let look = scene_settings()?;
    for name in files::list_scenes() {
        let mark = if look.scene.as_deref() == Some(name.as_str()) { "*" } else { " " };
        match files::read_scene(&name) {
            Ok(s) => println!("{mark} {name:<12} {:?}, {:?}, {} fps, {} px cells", s.kind, s.place, s.fps(), s.cell),
            Err(e) => println!("{mark} {name:<12} does not read: {e}"),
        }
    }
    println!("motion {}{}", look.motion, if look.still() { " (still)" } else { "" });
    Ok(())
}

/// Set the app's scene (`none` for none) or motion in the store's app.md.
fn scene_setting(scene: Option<&str>, motion: Option<&str>) -> Result {
    let store = files::store::open_default()?;
    let mut state = files::store::load_or_first(&*store)?;
    if let Some(name) = scene {
        if name != theme::NO_SCENE && !files::list_scenes().iter().any(|n| n == name) {
            return Err(format!("no scene \"{name}\" in scenes/ (there: {})", files::list_scenes().join(", ")));
        }
        state.scene = Some(name.to_string());
    }
    if let Some(secs) = motion {
        state.motion = Some(secs.parse::<f32>().ok().filter(|m| *m >= 0.0).ok_or_else(|| format!("\"{secs}\" is not a number of seconds"))?);
    }
    store.save(&state)?;
    let look = scene_settings()?;
    println!("scene {} · motion {} (in force; state/local.md's ## Scene rows win over app.md)", look.scene.as_deref().unwrap_or(theme::NO_SCENE), look.motion);
    Ok(())
}

/// `dry theme …` and `dry scene …` past `use`: the same verbs for both
/// (`dry_tools::looks`).
fn look_command(kind: Kind, rest: &[&str]) -> Result {
    use files::looks;
    let first = match kind {
        Kind::Theme => files::DEFAULT_THEME,
        Kind::Scene => "cat",
    };
    match rest {
        ["show", name] => print(looks::read_main(kind, name)),
        ["new", name] => look_new(kind, name, first),
        ["new", name, "--from", from] => look_new(kind, name, from),
        ["set", name, key, value] => look_set(kind, name, key, value, None),
        ["set", name, key, value, mode] => look_set(kind, name, key, value, Some(parse_mode(mode)?)),
        ["rm", name] => {
            looks::remove(kind, name, &in_use(kind)?)?;
            println!("removed {} {name} ({})", kind.word(), looks::folder(kind, name).display());
            Ok(())
        }
        ["export", name] => print(looks::export(kind, name)),
        ["import", file] => look_import(kind, file, false),
        ["import", file, "--replace"] | ["import", "--replace", file] => look_import(kind, file, true),
        ["export", name, file] => {
            std::fs::write(file, looks::export(kind, name)?).map_err(|e| format!("{file}: {e}"))?;
            println!("wrote {} {name} to {file}", kind.word());
            Ok(())
        }
        _ => Err(format!("unknown command: dry {} {}; see the list at the top of crates/tools/src/main.rs", kind.word(), rest.join(" "))),
    }
}

/// The looks of `kind` the app uses now, which `rm` refuses.
fn in_use(kind: Kind) -> Result<Vec<String>> {
    Ok(match kind {
        Kind::Theme => vec![files::active_theme()],
        Kind::Scene => scene_settings()?.scene.into_iter().collect(),
    })
}

fn look_new(kind: Kind, name: &str, from: &str) -> Result {
    let file = files::looks::add(kind, name, from)?;
    println!("created {} from {from} (use it with `dry {} use {name}`)", file.display(), kind.word());
    Ok(())
}

fn look_set(kind: Kind, name: &str, key: &str, value: &str, mode: Option<Mode>) -> Result {
    files::looks::edit(kind, name, key, value, mode)?;
    println!("{} {name}: {key} = {value}{}", kind.word(), mode.map_or(String::new(), |m| format!(" ({})", m.as_str())));
    Ok(())
}

fn look_import(kind: Kind, file: &str, replace: bool) -> Result {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let read = dry_core::look::import(&text)?;
    if read.kind != kind {
        return Err(format!("{file} is a {}: dry {} import {file}", read.kind.word(), read.kind.word()));
    }
    let pack = files::looks::import(&text, replace)?;
    println!("imported {} {} ({} files) into {}", pack.kind.word(), pack.name, pack.files.len(), files::looks::folder(pack.kind, &pack.name).display());
    Ok(())
}

fn parse_mode(name: &str) -> Result<Mode> {
    Mode::parse(name).ok_or_else(|| format!("\"{name}\" is not light or dark"))
}

/// A colour as a terminal swatch (24-bit colour escape codes).
fn swatch(rgba: u32) -> String {
    let (r, g, b) = (rgba >> 24, (rgba >> 16) & 0xFF, (rgba >> 8) & 0xFF);
    format!("\x1b[48;2;{r};{g};{b}m    \x1b[0m")
}

fn tokens(mode: Option<Mode>) -> Result {
    let theme = files::read_theme()?;
    println!("THEME {}", files::active_theme());
    let modes = mode.map_or(vec![Mode::Light, Mode::Dark], |m| vec![m]);
    let note = |name: &str| theme.defaults.notes.get(name).cloned().unwrap_or_default();
    println!("COLORS");
    for name in COLOR_NAMES {
        let mut line = format!("  {name:<20}");
        for &m in &modes {
            let v = colour(&theme::resolve(m, &theme), name);
            let mark = if theme.is_changed(m, name) { "*" } else { " " };
            line.push_str(&format!(" {} {}{mark}", swatch(v), theme::hex(v)));
        }
        println!("{line}  {}", note(name));
    }
    println!("SIZES");
    let t = theme::resolve(Mode::Light, &theme);
    for name in NUMBER_NAMES {
        let mark = if theme.is_changed(Mode::Light, name) { "*" } else { " " };
        println!("  {name:<20} {:>6}{mark}  {}", number(&t, name), note(name));
    }
    let mark = if theme.is_changed(Mode::Light, FONT) { "*" } else { " " };
    println!("FONT\n  {FONT:<20} {}{mark}", t.font_family);
    let look = scene_settings()?;
    println!("SCENE (app settings, not tokens: `dry scene use`, `dry motion`)");
    println!("  {SCENE:<20} {}\n  {:<20} {}", look.scene.as_deref().unwrap_or(theme::NO_SCENE), theme::MOTION, look.motion);
    println!("(* = changed from its default; columns: {})", modes.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", "));
    Ok(())
}

fn token_get(name: &str) -> Result {
    let theme = files::read_theme()?;
    for m in [Mode::Light, Mode::Dark] {
        let t = theme::resolve(m, &theme);
        let d = theme::defaults(&theme, m);
        let (now, default) = if COLOR_NAMES.contains(&name) {
            (format!("{} {}", swatch(colour(&t, name)), theme::hex(colour(&t, name))), theme::hex(colour(&d, name)))
        } else if NUMBER_NAMES.contains(&name) {
            (number(&t, name).to_string(), number(&d, name).to_string())
        } else if name == FONT {
            (t.font_family.clone(), d.font_family.clone())
        } else if name == SCENE || name == theme::MOTION {
            return Err(format!("{name} is an app setting, not a token: see `dry scenes`"));
        } else {
            return Err(format!("no token \"{name}\""));
        };
        println!("{name} {:<5} {now}  (default {default})", m.as_str());
    }
    Ok(())
}

fn token_set(name: &str, value: &str, mode: Option<Mode>) -> Result {
    let mut theme = files::read_theme()?;
    theme.set(name, value, mode)?;
    files::save_theme(&theme)?;
    token_get(name)
}

fn token_reset(name: Option<&str>) -> Result {
    let mut theme = files::read_theme()?;
    theme.reset(name);
    files::save_theme(&theme)?;
    println!("reset {}", name.unwrap_or("every token"));
    Ok(())
}

fn colour(t: &theme::Tokens, name: &str) -> u32 {
    t.color(name).unwrap_or(0)
}

/// A size token's value by name.
fn number(t: &theme::Tokens, name: &str) -> f32 {
    t.number(name).unwrap_or(0.0)
}
