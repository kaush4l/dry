//! `dry companion`: the Rust companion. The same wire contract as the
//! Node one (docs/companion-protocol.md), checked by the same black-box
//! suite (`node conformance/run.mjs --cmd "<path>/dry companion"`).
//!
//! ```text
//! dry companion [--folder DIR] [--port 7777] [--yes] [--on ID,..] [--off ID,..]
//!               [--providers FILE] [--config FILE] [--capabilities DIR | --no-capabilities]
//!               [--pair-code-file FILE] [--heartbeat S] [--app DIR | --no-app] [--origin URL]...
//!               [--ttl HOURS] [--store DIR] [--chrome PATH] [--lmstudio URL] [--ollama URL]
//!               [--quiet | --verbose]
//! ```
//!
//! One process, std's `TcpListener` on 127.0.0.1, a thread per connection.
//! The rules are dry's own code where dry has them: the capability parser
//! (`dry_core::capability::declared`), the declared runner and loader
//! (`crate::capability`), the secret rules (`dry_core::file::secret_file`,
//! `dry_core::terminal::secret_env`), the CLI runner (`crate::cli`), the
//! evidence runner and the DevTools client (`crate::evidence`, `crate::cdp`).
//!
//! The start in the terminal is the person's (docs/companion.md): the
//! folder confirmed, the checks (each found or missing, with the fix),
//! the switches (numbered; actuators start off; saved in
//! `.dry/companion.json`), then `listening http://127.0.0.1:<port>`, the
//! pairing code, and live lines. Esc stops everything it runs.

mod browser;
mod caps;
mod evidence;
mod folder;
mod http;
mod providers;
mod routes;
mod rules;
mod state;
mod sys;
mod term;

use std::io::{BufRead, IsTerminal, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{Value, json};

use state::{Companion, NAME, PROTOCOL, Setup, VERSION};

const DEFAULT_PORT: u16 = 7777;

const HELP: &str = "dry companion: one folder of this machine (its files and a terminal), its models and capabilities, for the dry web app.

  dry companion [options]

  --folder DIR     the folder the page gets (default: the current one); dry keeps its state in DIR/.dry/
  --port N         listen on 127.0.0.1:N (default 7777; 0: any free port)
  --yes            take the saved switches (and the folder) without asking
  --on ID[,ID]     switch capabilities on (saved); --off ID[,ID] switches them off
  --providers FILE the declared model providers (default: DIR/.dry/providers.json, made from the template)
  --config FILE    where the switches are saved (default: DIR/.dry/companion.json)
  --pair-code-file FILE   write each new pairing code there (for scripts and the conformance suite)
  --heartbeat S    a ping on every open stream each S seconds (default 15)
  --capabilities DIR   capability files (<id>.md) this machine declares (default: the repo's capabilities/)
  --no-capabilities    declare none
  --app DIR        serve the built app at / and /app/ (default: the repo's dist/, if built); --no-app: none
  --origin URL     one more page origin to answer (repeatable)
  --ttl HOURS      how long a pairing lasts (default 12)
  --store DIR      where the page's state goes (default: DIR/.dry/state)
  --chrome PATH    the Chrome / Chromium the browser and evidence drive (default: found on this machine)
  --lmstudio URL, --ollama URL   the same as declaring a port provider of that name
  --verbose        a line for every read too; --quiet: no lines

While it runs: Esc stops everything it runs. Type pairings · revoke <id> · on <id> · off <id> · switches · providers · code · capabilities · stop-all

Then in dry: Settings → Companion, type the code printed below, Connect.";

/// The command line, read (docs/companion-protocol.md §3).
struct Options {
    setup: Setup,
    yes: bool,
    no_capabilities: bool,
    no_app: bool,
    help: bool,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut o = Options {
        setup: Setup {
            folder: PathBuf::from("."),
            port: DEFAULT_PORT,
            providers: None,
            config: None,
            capabilities: None,
            pair_code_file: None,
            heartbeat: Duration::from_secs(15),
            app: None,
            origins: Vec::new(),
            ttl: 12 * 3600,
            store: None,
            quiet: false,
            verbose: false,
            lmstudio: None,
            ollama: None,
            chrome: None,
            on: Vec::new(),
            off: Vec::new(),
        },
        yes: false,
        no_capabilities: false,
        no_app: false,
        help: false,
    };
    let mut folder = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().map(|v| v.to_string()).ok_or_else(|| format!("{a} needs a value"));
        let s = &mut o.setup;
        match *a {
            "--folder" => folder = Some(PathBuf::from(value()?)),
            "--port" => s.port = value()?.parse().map_err(|_| "--port takes a port number")?,
            "--yes" | "-y" => o.yes = true,
            "--on" => s.on.extend(value()?.split(',').filter(|x| !x.is_empty()).map(String::from)),
            "--off" => s.off.extend(value()?.split(',').filter(|x| !x.is_empty()).map(String::from)),
            "--pair-code-file" => s.pair_code_file = Some(value()?.into()),
            "--providers" => s.providers = Some(value()?.into()),
            "--config" => s.config = Some(value()?.into()),
            "--heartbeat" => {
                let secs: f64 = value()?.parse().map_err(|_| "--heartbeat takes seconds")?;
                s.heartbeat = Duration::from_secs_f64(secs.max(0.1));
            }
            "--capabilities" => s.capabilities = Some(value()?.into()),
            "--no-capabilities" => o.no_capabilities = true,
            "--app" => s.app = Some(value()?.into()),
            "--no-app" => o.no_app = true,
            "--origin" => s.origins.push(value()?),
            "--ttl" => {
                let h: f64 = value()?.parse().map_err(|_| "--ttl takes hours")?;
                s.ttl = (h * 3600.0).max(60.0) as u64;
            }
            "--store" => s.store = Some(value()?.into()),
            "--chrome" => s.chrome = Some(value()?),
            "--lmstudio" => s.lmstudio = Some(value()?),
            "--ollama" => s.ollama = Some(value()?),
            "--quiet" => s.quiet = true,
            "--verbose" => s.verbose = true,
            "--help" | "-h" => o.help = true,
            other => return Err(format!("unknown option {other} (see --help)")),
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    o.setup.folder = folder
        .map(|f| PathBuf::from(rules::home(&f.to_string_lossy())))
        .map(|f| if f.is_absolute() { f } else { cwd.join(f) })
        .unwrap_or(cwd);
    Ok(o)
}

/// The capabilities folder: `--capabilities`, else the repo's (if there); `--no-capabilities`: none.
fn cap_dir(o: &Options) -> Option<PathBuf> {
    if o.no_capabilities {
        return None;
    }
    o.setup.capabilities.clone().or_else(|| crate::capability::dir().filter(|d| d.is_dir()))
}

/// The app folder: `--app` (it must hold an index.html), else the repo's dist/ if built.
fn app_dir(o: &Options) -> Result<Option<PathBuf>, String> {
    if o.no_app {
        return Ok(None);
    }
    let dir = o.setup.app.clone().unwrap_or_else(|| crate::root().join("dist"));
    let built = dir.join("app").join("index.html").is_file() || dir.join("index.html").is_file();
    match (built, o.setup.app.is_some()) {
        (true, _) => Ok(Some(dir.canonicalize().unwrap_or(dir))),
        (false, true) => Err(format!("--app {}: no index.html there (build the app first)", dir.display())),
        (false, false) => Ok(None),
    }
}

/// Ask one line (stdin is a terminal).
fn ask(question: &str) -> String {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

/// `dry companion …`.
pub fn command(args: &[&str]) -> ExitCode {
    let mut o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if o.help {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let interactive = std::io::stdin().is_terminal() && !o.yes;
    // 1. The folder, confirmed.
    println!("dry companion {VERSION}");
    let folder = o.setup.folder.clone();
    if interactive {
        let a = ask(&format!(
            "the page gets this folder: {}\n  its files, and a terminal in it (state in .dry/). Use it? [Y/n] ",
            folder.display()
        ));
        if a.to_lowercase().starts_with('n') {
            println!("give the folder with --folder DIR");
            return ExitCode::SUCCESS;
        }
    } else {
        println!("folder: {}", folder.display());
    }
    o.setup.capabilities = cap_dir(&o);
    o.setup.app = match app_dir(&o) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let (on, off) = (o.setup.on.clone(), o.setup.off.clone());
    let port = o.setup.port;
    let c = match Companion::new(o.setup) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if !on.is_empty() || !off.is_empty() {
        c.save_switches();
    }
    // A providers.json to paste into, made from the template on the first run.
    if !c.providers_file.exists() {
        let mut v: Value = serde_json::from_str(providers::TEMPLATE).unwrap_or_else(|_| json!({ "version": 1 }));
        if !v["providers"].is_array() {
            v["providers"] = json!([]);
        }
        match state::write_atomic(&c.providers_file, &(serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")) {
            Ok(()) => println!("providers: made {} from the template: paste entries into \"providers\"", c.providers_file.display()),
            Err(e) => println!("providers: could not make {}: {e}", c.providers_file.display()),
        }
    }
    // 2. The checks: each found or missing, with the fix.
    println!("\nchecks");
    for k in checks(&c) {
        println!("{k}");
    }
    // 3. The switches: what the page may use (actuators start off).
    loop {
        let list = caps::summary(&c, false)["list"].as_array().cloned().unwrap_or_default();
        println!("\nswitches (saved in {}; actuators start off)", c.config_file.display());
        for (n, x) in list.iter().enumerate() {
            let state = x["state"].as_str().unwrap_or("");
            println!(
                "  {:>2}  {}  {:<16} {}{}{}",
                n + 1,
                if x["on"] == json!(true) { "on " } else { "off" },
                x["id"].as_str().unwrap_or(""),
                if x["actuator"] == json!(true) { "(actuator) " } else { "" },
                if state != "present" { format!("[{state}] ") } else { String::new() },
                x["doc"].as_str().unwrap_or("").chars().take(70).collect::<String>()
            );
        }
        if !interactive {
            break;
        }
        let a = ask("flip some (their numbers, e.g. \"2 5\"), or Enter to go on: ");
        if a.is_empty() {
            c.save_switches();
            break;
        }
        for w in a.split([' ', ',']).filter(|w| !w.is_empty()) {
            if let Some(x) = w.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|i| list.get(i)) {
                c.set_switch(x["id"].as_str().unwrap_or(""), x["on"] != json!(true));
            }
        }
    }
    // 4. Listening, the pairing code, then live lines.
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not listen on 127.0.0.1:{port}: {e}");
            return ExitCode::from(1);
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    c.port.store(port, Ordering::Relaxed);
    sys::trap();
    {
        let c = c.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                let c = c.clone();
                std::thread::spawn(move || routes::connection(c, s));
            }
        });
    }
    {
        let c = c.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(c.heartbeat);
                c.beat();
            }
        });
    }
    {
        let c = c.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if !c.mcp_streams.lock().unwrap().is_empty() {
                    caps::tick(&c);
                }
            }
        });
    }
    println!();
    println!("{NAME} {VERSION} on http://127.0.0.1:{port}  (protocol {PROTOCOL}, rust)");
    println!("listening http://127.0.0.1:{port}");
    println!("folder: {}", c.folder.display());
    let extra = if c.origins.is_empty() { String::new() } else { format!(", {}", c.origins.join(", ")) };
    println!("pages allowed: https://kaush4l.github.io, http://localhost:*, http://127.0.0.1:*{extra}");
    if let Some(a) = &c.app_dir {
        println!("the app: http://localhost:{port}/app/  (from {})", a.display());
    }
    println!("MCP: http://localhost:{port}/mcp  (Bearer: a pairing token)");
    println!("type: pairings · revoke <id> · on|off <id> · switches · providers · code · capabilities · stop-all   (Esc: stop all)");
    c.announce_code();
    let _ = std::io::stdout().flush();
    if std::io::stdin().is_terminal() {
        keys(&c);
    } else {
        while !sys::STOP.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    println!("\nstopping");
    close(&c);
    ExitCode::SUCCESS
}

/// Everything it started, stopped: commands, runners, the browser, streams.
fn close(c: &Arc<Companion>) {
    term::stop_all(c);
    providers::stop_all(c);
    browser::close(c);
    let ids: Vec<String> = c.pairings.lock().unwrap().iter().map(|p| p.id.clone()).collect();
    for id in ids {
        c.close_streams(&id);
    }
    // The sessions' SIGKILL, 2 s after their SIGTERM, if any still runs.
    let deadline = std::time::Instant::now() + Duration::from_millis(2500);
    while std::time::Instant::now() < deadline && c.sessions.lock().unwrap().iter().any(|s| s.running()) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The terminal while it runs: a line is a command; Esc stops everything.
fn keys(c: &Arc<Companion>) {
    let mut raw = sys::Keys::start();
    let mut line = String::new();
    // Esc alone stops all; Esc [ … is an arrow key (a sequence), ignored.
    let (mut esc, mut seq) = (false, false);
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 64];
    while !sys::STOP.load(Ordering::SeqCst) {
        let n = match stdin.read(&mut buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            if !raw.raw() {
                // A closed terminal (cooked mode reads 0 only at the end).
                break;
            }
            if esc {
                esc = false;
                println!("{}", run_command(c, "stop-all"));
            }
            continue;
        }
        for &b in &buf[..n] {
            if seq {
                if (0x40..=0x7e).contains(&b) {
                    seq = false;
                }
                continue;
            }
            if esc {
                esc = false;
                if b == b'[' || b == b'O' {
                    seq = true;
                    continue;
                }
                println!("{}", run_command(c, "stop-all"));
            }
            match b {
                0x1b => esc = true,
                b'\r' | b'\n' => {
                    if raw.raw() {
                        println!();
                    }
                    let out = run_command(c, &line);
                    if !out.is_empty() {
                        println!("{out}");
                    }
                    line.clear();
                }
                0x7f | 0x08 => {
                    if line.pop().is_some() && raw.raw() {
                        print!("\x08 \x08");
                    }
                }
                0x03 => sys::STOP.store(true, Ordering::SeqCst),
                b if b >= 0x20 || b == b'\t' => {
                    line.push(b as char);
                    if raw.raw() {
                        print!("{}", b as char);
                    }
                }
                _ => {}
            }
            let _ = std::io::stdout().flush();
        }
    }
    raw.restore();
}

/// A line typed in the companion's terminal: what to print.
fn run_command(c: &Arc<Companion>, line: &str) -> String {
    let mut w = line.split_whitespace();
    let (cmd, arg) = (w.next().unwrap_or(""), w.next().unwrap_or(""));
    match cmd {
        "" => String::new(),
        "pairings" => c.pairings_text(),
        "revoke" if arg.is_empty() => "  revoke <id>: which pairing (type: pairings)".into(),
        "revoke" => {
            if c.end_pairing(arg, "revoked") {
                format!("  revoked {arg}")
            } else {
                format!("  no pairing {arg}")
            }
        }
        "stop-all" => {
            let r = routes::stop_all(c);
            format!("  stopped {} session(s){}", r["stopped"], if r["browser"] == json!(true) { ", closed the browser" } else { "" })
        }
        "code" => format!("  pairing code: {}", c.code()),
        "on" | "off" if arg.is_empty() => format!("  {cmd} <id>: which capability (type: switches)"),
        "on" | "off" => {
            let list = caps::summary(c, false);
            if !list["list"].as_array().into_iter().flatten().any(|x| x["id"] == arg) {
                return format!("  no capability {arg} (type: switches)");
            }
            c.set_switch(arg, cmd == "on");
            format!("  {arg} {cmd}")
        }
        "switches" => caps::summary(c, false)["list"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| {
                let state = x["state"].as_str().unwrap_or("");
                format!(
                    "  {}  {:<16}{}{}",
                    if x["on"] == json!(true) { "on " } else { "off" },
                    x["id"].as_str().unwrap_or(""),
                    if x["actuator"] == json!(true) { " actuator" } else { "" },
                    if state != "present" { format!(" ({state}: {})", x["why"].as_str().unwrap_or("")) } else { String::new() }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "providers" => {
            let v = providers::now(c, true);
            let mut out =
                vec![format!("  {}{}", c.providers_file.display(), if v["exists"] == json!(true) { "" } else { " (not there yet)" })];
            for x in v["providers"].as_array().into_iter().flatten() {
                let state = x["state"].as_str().unwrap_or("");
                let rest = if state == "ok" {
                    x["models"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
                } else {
                    let hint = x["hint"].as_str().filter(|h| !h.is_empty()).map(|h| format!(" → {h}")).unwrap_or_default();
                    format!("{}{hint}", x["why"].as_str().unwrap_or(""))
                };
                out.push(format!("  {state:<8} {:<12} {}: {rest}", x["name"].as_str().unwrap_or(""), x["kind"].as_str().unwrap_or("")));
            }
            for e in v["errors"].as_array().into_iter().flatten() {
                out.push(format!("  ! {}", e["error"].as_str().unwrap_or("")));
            }
            out.join("\n")
        }
        "capabilities" => {
            let sm = caps::summary(c, true);
            let mut out: Vec<String> = sm["list"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| {
                    let tools: Vec<&str> = x["tools"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    let why = x["why"].as_str().filter(|w| !w.is_empty()).map(|w| format!(" — {w}")).unwrap_or_default();
                    let tools = if tools.is_empty() { String::new() } else { format!("\n             tools: {}", tools.join(", ")) };
                    format!(
                        "  {:<10} {:<12}{}{why}{tools}",
                        x["id"].as_str().unwrap_or(""),
                        x["state"].as_str().unwrap_or(""),
                        if x["declared"] == json!(true) { " declared" } else { "" }
                    )
                })
                .collect();
            out.extend(sm["errors"].as_array().into_iter().flatten().map(|e| format!("  ! {}", e.as_str().unwrap_or(""))));
            out.join("\n")
        }
        _ => "  commands: pairings · revoke <id> · on <id> · off <id> · switches · providers · stop-all · code · capabilities".into(),
    }
}

/// One check line: `ok`, `missing`, `absent` or `failed`, what, and the fix.
fn check(mark: &str, what: &str, name: &str, line: &str, hint: &str) -> String {
    let named = if name.is_empty() || name == what { String::new() } else { format!("{name}: ") };
    let fix = if mark != "ok" && !hint.is_empty() { format!("\n             → {hint}") } else { String::new() };
    format!("  {mark:<8} {what:<10} {named}{line}{fix}")
}

/// What this machine has, each found or missing with how to fix it.
fn checks(c: &Arc<Companion>) -> Vec<String> {
    let mut out = vec![check("ok", "folder", "", &format!("{} (state in .dry/)", c.folder.display()), "")];
    let (declared, errors, exists) = providers::declared(c);
    if !exists && declared.is_empty() {
        out.push(check(
            "missing",
            "providers",
            &c.providers_file.display().to_string(),
            "none declared",
            "paste entries into it (the examples are in it), or add one in dry → Settings → Companion",
        ));
    }
    for e in &errors {
        out.push(check("missing", "providers", e["entry"].as_str().unwrap_or("providers.json"), e["error"].as_str().unwrap_or(""), ""));
    }
    providers::probe_all(c);
    for p in providers::now(c, false)["providers"].as_array().into_iter().flatten() {
        let state = p["state"].as_str().unwrap_or("failed");
        let models = p["models"].as_array().map_or(0, Vec::len);
        let line = if state == "ok" {
            let url = p["url"].as_str().map(|u| format!(" {u}")).unwrap_or_default();
            let runner = p["runner"].as_str().map(|r| format!(" ({r}, {})", p["accel"].as_str().unwrap_or(""))).unwrap_or_default();
            format!("{}{url}: {models} model{}{runner}", p["kind"].as_str().unwrap_or(""), if models == 1 { "" } else { "s" })
        } else {
            format!("{}: {}", p["kind"].as_str().unwrap_or(""), p["why"].as_str().unwrap_or(""))
        };
        out.push(check(state, "provider", p["name"].as_str().unwrap_or(""), &line, p["hint"].as_str().unwrap_or("")));
    }
    let (list, errors) = caps::declared(c);
    for d in &list {
        let st = caps::probe(c, d, true);
        let ok = st == dry_core::capability::State::Present;
        let line = if ok { d.doc.lines().next().unwrap_or("").to_string() } else { st.why().to_string() };
        out.push(check(if ok { "ok" } else { "absent" }, "capability", &d.id, &line, "its probe says this machine lacks it"));
    }
    for e in &errors {
        out.push(check("missing", "capability", "", e, "fix the line it names"));
    }
    match c.program("git") {
        Some(g) => {
            let v = crate::capability::run_argv(&g.display().to_string(), &["--version".into()], &[], 8, &c.folder)
                .map(|(_, o)| o.trim().to_string())
                .unwrap_or_else(|_| g.display().to_string());
            out.push(check("ok", "git", "", &v, ""));
        }
        None => out.push(check("missing", "git", "", "not on PATH", "install git (https://git-scm.com/downloads)")),
    }
    match caps::chrome(c) {
        Some(p) => out.push(check("ok", "chrome", "browser", &p.display().to_string(), "")),
        None => out.push(check(
            "missing",
            "chrome",
            "browser",
            "no Chrome, Chromium, Edge or Brave",
            "install Google Chrome or Chromium, or give --chrome PATH",
        )),
    }
    match ["docker", "podman"].into_iter().find_map(|n| c.program(n).map(|p| (n, p))) {
        Some((n, p)) => out.push(check("ok", "container", n, &p.display().to_string(), "")),
        None => {
            out.push(check("missing", "container", "", "no docker or podman", "install Docker Desktop or Podman (optional: containers)"))
        }
    }
    out
}
