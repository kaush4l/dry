//! Capabilities on this machine (docs/capabilities.md): the declared
//! files read and probed, their commands run (argv only, never a shell),
//! the compiled ones probed, peers joined over MCP, and `dry discover`.
//! The model and the registry are `dry_core::capability`.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dry_core::capability::declared::{self, Declared};
use dry_core::capability::mcp::{self, Hello, Note};
use dry_core::capability::{Capability, Provider, Registry, Routed, State};
use serde_json::{Value, json};

/// The folder of declared capabilities: `DRY_CAPABILITIES` (`none`: no
/// folder), else `capabilities/` at the repo root.
pub fn dir() -> Option<PathBuf> {
    match std::env::var("DRY_CAPABILITIES") {
        Ok(v) if v == "none" || v.is_empty() => None,
        Ok(v) => Some(PathBuf::from(v)),
        Err(_) => Some(crate::root().join("capabilities")),
    }
}

/// Every `<id>.md` in `dir`, read, by id; and the files that did not read
/// (each error names its file and line).
pub fn load(dir: &Path) -> (Vec<Declared>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (out, errors) };
    let mut files: Vec<PathBuf> =
        rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
    files.sort();
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match std::fs::read_to_string(&f).map_err(|e| format!("{name}: {e}")).and_then(|t| declared::parse(&name, &t)) {
            Ok(d) if out.iter().any(|o: &Declared| o.id == d.id) => errors.push(format!("{name}: a second capability `{}`", d.id)),
            Ok(d) if format!("{}.md", d.id) != name => errors.push(format!("{name}: its id is `{}`: name the file {}.md", d.id, d.id)),
            Ok(d) => out.push(d),
            Err(e) => errors.push(e),
        }
    }
    (out, errors)
}

/// Run a program with its arguments, no shell, no input, the
/// secret-looking environment left out plus `env`; killed after `timeout`
/// seconds. Its exit code and output (stdout then stderr, at most 64 KB).
pub fn run_argv(program: &str, args: &[String], env: &[(String, String)], timeout: u64, cwd: &Path) -> Result<(i32, String), String> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (name, _) in std::env::vars_os() {
        if dry_core::terminal::secret_env(&name.to_string_lossy()) {
            cmd.env_remove(&name);
        }
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut a = Vec::new();
        let _ = out.read_to_end(&mut a);
        a
    });
    let reader_err = std::thread::spawn(move || {
        let mut a = Vec::new();
        let _ = err.read_to_end(&mut a);
        a
    });
    let start = Instant::now();
    let code = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s.code().unwrap_or(-1),
            None if start.elapsed() > Duration::from_secs(timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program}: timed out after {timeout}s"));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let mut text = String::from_utf8_lossy(&reader.join().unwrap_or_default()).to_string();
    text.push_str(&String::from_utf8_lossy(&reader_err.join().unwrap_or_default()));
    if text.len() > 65_536 {
        let cut = text.len() - 65_536;
        let at = (cut..text.len()).find(|i| text.is_char_boundary(*i)).unwrap_or(cut);
        text = text[at..].to_string();
    }
    Ok((code, text))
}

/// The folder declared commands run in: the repo root.
fn cwd() -> PathBuf {
    crate::root()
}

/// A declared capability's state on this machine: its probe's answer.
pub fn probe(d: &Declared) -> State {
    match &d.probe {
        None => State::Present,
        Some(p) => declared::probed(p, run_argv(&p[0], &p[1..], &[], declared::TIMEOUT, &cwd()).map(|(c, _)| c)),
    }
}

/// Its snapshot now.
pub fn snapshot(d: &Declared) -> Result<String, String> {
    let s = d.snapshot.as_ref().ok_or("it has no snapshot")?;
    let (code, out) = run_argv(&s[0], &s[1..], &[], declared::TIMEOUT, &cwd())?;
    if code == 0 { Ok(out.trim_end().to_string()) } else { Err(format!("`{}` exited {code}: {}", s.join(" "), out.trim())) }
}

/// A routed call to a declared tool: its command, its output (an exit
/// other than 0 is an error with the output).
pub fn call(all: &[Declared], r: &Routed) -> Result<String, String> {
    let d = all.iter().find(|d| d.id == r.capability).ok_or_else(|| format!("no declared capability {}", r.capability))?;
    let a = d.argv(&r.tool, &r.args)?;
    let (code, out) = run_argv(&a.program, &a.args, &a.env, a.timeout, &cwd())?;
    let out = out.trim_end().to_string();
    if code == 0 { Ok(if out.is_empty() { "(done, no output)".into() } else { out }) } else { Err(format!("exit {code}: {out}")) }
}

/// Whether a program is on this machine's PATH.
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// The compiled capabilities' states in a shell on this machine: all
/// there, but `container` only with docker or podman.
pub fn probe_compiled(r: &mut Registry) {
    let found: Vec<&str> = ["docker", "podman"].into_iter().filter(|p| on_path(p)).collect();
    let state = if found.is_empty() {
        State::Absent("no docker or podman on this machine (the browser has its own Linux: docs/container.md)".into())
    } else {
        State::Present
    };
    r.set_state("container", &Provider::Compiled, state);
    // The simple check (`dry_core::environment::probe`): a native build
    // starts processes, so its environment is this computer.
    use dry_core::environment::{Environment, Kind, Probe, probe};
    if probe(Probe { processes: true, ..Probe::default() }) == Some(Kind::Host) {
        let detail = format!("{} {}: the disk and processes of this computer", std::env::consts::OS, std::env::consts::ARCH);
        let mut env = Environment::host(&detail, &crate::root().display().to_string());
        // The runtimes on PATH, looked up once at start (what `command -v`
        // finds; nothing is started).
        env.runtimes = Some(dry_core::environment::RUNTIMES.iter().filter(|p| on_path(p)).map(|p| p.to_string()).collect());
        r.environment = Some(env);
    }
}

/// This machine's registry: the compiled capabilities (probed), and every
/// declared file in `dir()` probed. With the declared files (to run them)
/// and the files that did not read.
pub fn local() -> (Registry, Vec<Declared>, Vec<String>) {
    let mut r = Registry::default();
    probe_compiled(&mut r);
    let (all, errors) = dir().map(|d| load(&d)).unwrap_or_default();
    let caps: Vec<Capability> = all.iter().map(|d| d.manifest(probe(d))).collect();
    r.register(Provider::Declared, caps);
    (r, all, errors)
}

/// This machine's registry and declared files, read once for the process
/// (a shell with no host of its own: the window, the CLI).
pub fn local_cached() -> &'static (Registry, Vec<Declared>) {
    static LOCAL: std::sync::OnceLock<(Registry, Vec<Declared>)> = std::sync::OnceLock::new();
    LOCAL.get_or_init(|| {
        let (r, d, errors) = local();
        for e in errors {
            eprintln!("capabilities: {e}");
        }
        (r, d)
    })
}

/// A call to a declared capability of this machine (`local_cached`), run
/// here. `None`: no declared capability has that tool.
pub fn route_local(call: &dry_core::calls::Call) -> Option<Result<String, String>> {
    let (r, declared) = local_cached();
    let routed = r.route(call).filter(|x| x.provider == Provider::Declared)?;
    Some(self::call(declared, &routed))
}

/// This runtime's hello.
pub fn hello(runtime: &str) -> Hello {
    Hello {
        runtime: runtime.into(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol: mcp::PROTOCOL.into(),
    }
}

// ---- peers: a companion or another runtime, over MCP ----

/// A peer this shell joined: where it answers and its token.
pub struct Peer {
    pub name: String,
    /// Its base URL (`http://127.0.0.1:7777`); MCP is at `/mcp`.
    pub url: String,
    pub token: String,
    next: AtomicU64,
}

impl Peer {
    pub fn new(name: &str, url: &str, token: &str) -> Peer {
        Peer { name: name.into(), url: url.trim_end_matches('/').into(), token: token.into(), next: AtomicU64::new(1) }
    }

    /// Pair with a companion: its one-time code for a token.
    pub fn pair(name: &str, url: &str, code: &str) -> Result<Peer, String> {
        let url = url.trim_end_matches('/');
        let mut resp = ureq::post(&format!("{url}/v1/pair"))
            .header("Content-Type", "application/json")
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .send(json!({ "code": code.trim() }).to_string().as_str())
            .map_err(|e| format!("could not reach {url}: {e}"))?;
        let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&body).map_err(|_| format!("{url} did not answer as a companion: {body}"))?;
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            return Err(e.to_string());
        }
        let token = v.get("token").and_then(Value::as_str).ok_or("no token in the answer")?;
        Ok(Peer::new(name, url, token))
    }

    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    /// One JSON-RPC request to its `/mcp`; the answer.
    pub fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        self.post(&mcp::request(self.id(), method, params))
    }

    /// A JSON-RPC message as it is; the answer.
    fn post(&self, msg: &Value) -> Result<Value, String> {
        let mut resp = ureq::post(&format!("{}/mcp", self.url))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("MCP-Protocol-Version", mcp::VERSION)
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .send(msg.to_string().as_str())
            .map_err(|e| format!("{} is not answering: {e}", self.name))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        if status == 401 {
            return Err(format!("{} refused the token (the pairing ended): pair again", self.name));
        }
        serde_json::from_str(&body).map_err(|_| format!("{} answered {status}: {body}", self.name))
    }

    /// The handshake: hello (initialize), then the manifest (tools/list and
    /// resources/list). Its hello and its capabilities.
    pub fn handshake(&self, me: &Hello) -> Result<(Hello, Vec<Capability>), String> {
        let init = self.post(&mcp::initialize(self.id(), me))?;
        let them = mcp::hello_of(mcp::result(&init)?);
        Ok((them, self.manifest()?))
    }

    /// Its manifest now (a re-register).
    pub fn manifest(&self) -> Result<Vec<Capability>, String> {
        let tools = self.rpc("tools/list", json!({}))?;
        let resources = self.rpc("resources/list", json!({}))?;
        Ok(mcp::manifest(&self.name, mcp::result(&tools)?, mcp::result(&resources).unwrap_or(&json!({}))))
    }

    /// The heartbeat.
    pub fn ping(&self) -> Result<(), String> {
        mcp::result(&self.rpc("ping", json!({}))?).map(|_| ())
    }

    /// A routed call.
    pub fn call(&self, r: &Routed) -> Result<String, String> {
        let msg = mcp::call(0, &r.tool, &r.args);
        mcp::call_result(&self.rpc("tools/call", msg["params"].clone())?)
    }

    /// A capability's snapshot (`resources/read`).
    pub fn read(&self, id: &str) -> Result<String, String> {
        mcp::read_result(&self.rpc("resources/read", json!({ "uri": format!("{}{id}", mcp::URI) }))?)
    }

    /// Its stream of notifications (`GET /mcp`, server-sent events): each
    /// read note to `on`, until the stream closes (the peer left) or `on`
    /// says stop.
    pub fn listen(&self, on: &mut dyn FnMut(Note) -> bool) -> Result<(), String> {
        let mut resp = ureq::get(&format!("{}/mcp", self.url))
            .header("Accept", "text/event-stream")
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("MCP-Protocol-Version", mcp::VERSION)
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("{} stream: {e}", self.name))?;
        if resp.status().as_u16() != 200 {
            return Err(format!("{} has no stream (status {})", self.name, resp.status().as_u16()));
        }
        let reader = std::io::BufReader::new(resp.body_mut().as_reader());
        for line in reader.lines() {
            let line = line.map_err(|e| format!("{} stream: {e}", self.name))?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let Ok(msg) = serde_json::from_str::<Value>(data.trim()) else { continue };
            if !on(mcp::note(&msg)) {
                return Ok(());
            }
        }
        Err(format!("{}'s stream closed", self.name))
    }
}

// ---- dry discover ----

/// A draft capability file for something this machine seems to have.
pub struct Draft {
    pub id: String,
    pub text: String,
    /// Why it is proposed (what was found).
    pub found: String,
}

/// Look at this machine for what it could declare. Each draft is probed
/// before it is proposed; none enables an actuator (a tool that acts says
/// `approval: yes`, and the drafts go to a folder you review first).
pub fn discover() -> Vec<Draft> {
    let mut out = Vec::new();
    let ok = |prog: &str, args: &[&str]| -> Option<String> {
        on_path(prog)
            .then(|| run_argv(prog, &args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), &[], 10, &cwd()).ok())
            .flatten()
            .filter(|(c, _)| *c == 0)
            .map(|(_, o)| o)
    };
    if let Some(o) = ok("pmset", &["-g", "batt"]).filter(|o| o.contains("InternalBattery")) {
        out.push(Draft {
            id: "battery".into(),
            found: format!("pmset reports a battery: {}", o.lines().nth(1).unwrap_or("").trim()),
            text: "# battery\nThis Mac's battery: how charged it is, and whether it runs on power.\n\nprobe: pmset -g batt\nsnapshot: pmset -g batt\ninterval: 60s\n\n## tool: status\nThe battery now.\ncommand: pmset -g batt\n\n## event: unplugged\nwhen: contains discharging\nThe Mac went on battery power.\n".into(),
        });
    }
    if ok("pmset", &["-g", "therm"]).is_some() {
        out.push(Draft {
            id: "thermal".into(),
            found: "pmset reports the thermal state".into(),
            text: "# thermal\nWhether this Mac is slowing its CPU for heat (pmset's thermal warning and CPU speed limit).\n\nprobe: pmset -g therm\nsnapshot: pmset -g therm\ninterval: 60s\n\n## event: throttled\nwhen: CPU_Speed_Limit < 100\nThe Mac is slowing its CPU for heat.\n".into(),
        });
    }
    if ok("df", &["-k", "/"]).is_some() {
        out.push(Draft {
            id: "disk".into(),
            found: "df reads the disks".into(),
            text: "# disk\nThe free space on this machine's main disk.\n\nsnapshot: df -h /\ninterval: 5m\n\n## tool: usage\nThe free space on every mounted disk now.\ncommand: df -h\n".into(),
        });
    }
    for rt in ["docker", "podman"] {
        if ok(rt, &["version"]).is_some() {
            out.push(Draft {
                id: rt.into(),
                found: format!("{rt} answers"),
                text: format!("# {rt}\nThe containers {rt} runs on this machine: list them; starting one goes through the terminal.\n\nprobe: {rt} version\nsnapshot: {rt} ps\ninterval: 30s\n\n## tool: ps\nThe running containers.\ncommand: {rt} ps\n\n## tool: stop\nStop one container.\napproval: yes\ncommand: {rt} stop {{name}}\ninputs:\n- name (string, required): the container's name or id\n"),
            });
        }
    }
    if Path::new("/dev/gpiochip0").exists() {
        out.push(Draft {
            id: "gpio".into(),
            found: "/dev/gpiochip0 is here".into(),
            text: "# gpio\nThis board's pins: read one; setting one drives hardware and needs approval.\n\nprobe: test -e /dev/gpiochip0\nsnapshot: pinctrl get 2-27\ninterval: 10s\n\n## tool: read\nOne pin.\ncommand: pinctrl get {pin}\ninputs:\n- pin (integer, required): the BCM number\n\n## tool: set\nDrive a pin: dh high, dl low.\napproval: yes\ncommand: pinctrl set {pin} op {level}\ninputs:\n- pin (integer, required): the BCM number\n- level (string, required): dh or dl\n".into(),
        });
    }
    if ok("vcgencmd", &["version"]).is_some() {
        out.push(Draft {
            id: "sensors".into(),
            found: "vcgencmd answers".into(),
            text: "# sensors\nThis Pi's SoC temperature.\n\nprobe: vcgencmd version\nsnapshot: vcgencmd measure_temp\ninterval: 30s\n\n## event: hot\nwhen: temp > 70\nThe SoC is above 70°C.\n".into(),
        });
    }
    // Every draft must read as a capability file: one that does not is a bug here.
    out.retain(|d| declared::parse(&format!("{}.md", d.id), &d.text).is_ok());
    out
}

/// `dry discover [--write <dir>]`: print the drafts, or write them for
/// review (never into the live `capabilities/` folder unless named).
pub fn discover_command(args: &[&str]) -> Result<(), String> {
    let drafts = discover();
    let out = match args {
        [] => None,
        ["--write", dir] => Some(PathBuf::from(dir)),
        _ => return Err("usage: dry discover [--write <dir>]".into()),
    };
    if drafts.is_empty() {
        println!("found nothing to declare on this machine");
        return Ok(());
    }
    let existing = dir().map(|d| load(&d).0).unwrap_or_default();
    for d in &drafts {
        let have = if existing.iter().any(|e| e.id == d.id) { " (already declared)" } else { "" };
        match &out {
            None => println!("## {}.md — {}{have}\n{}\n", d.id, d.found, d.text),
            Some(dir) => {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let f = dir.join(format!("{}.md", d.id));
                if f.exists() {
                    println!("kept {} (it exists)", f.display());
                    continue;
                }
                std::fs::write(&f, &d.text).map_err(|e| format!("{}: {e}", f.display()))?;
                println!("wrote {} — {}{have}", f.display(), d.found);
            }
        }
    }
    if out.is_some() {
        println!("review them, then move the ones you want into capabilities/ (a tool that acts says `approval: yes`)");
    }
    Ok(())
}

/// `dry capabilities`: every capability on this machine, by provider and state.
pub fn list_command() -> Result<(), String> {
    let (r, _, errors) = local();
    for c in r.all() {
        let tools: Vec<String> =
            c.tools.iter().map(|t| format!("{}{}", c.call_name(t), if t.approval { " (approval)" } else { "" })).collect();
        let why = if c.state.why().is_empty() { String::new() } else { format!(" — {}", c.state.why()) };
        println!("{:<10} {:<9} {:<12}{why}", c.id, c.provider.text(), c.state.word());
        if !tools.is_empty() {
            println!("           tools: {}", tools.join(", "));
        }
    }
    for e in errors {
        println!("! {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_runs_without_a_shell_and_an_input_stays_one_argument() {
        let d = declared::parse(
            "t.md",
            "# t\ndoc\n## tool: say\ncommand: printf %s|%s {a} {b}\ninputs:\n- a (string, required): one\n- b (string): two\n",
        )
        .unwrap();
        let attack = "x; touch /tmp/dry-capability-attack $(id)";
        let r =
            Routed { provider: Provider::Declared, capability: "t".into(), tool: "say".into(), args: vec![("a".into(), attack.into())] };
        assert_eq!(call(std::slice::from_ref(&d), &r).unwrap(), format!("{attack}|"));
        assert!(!Path::new("/tmp/dry-capability-attack").exists(), "nothing ran in a shell");
    }

    #[test]
    fn a_failing_probe_is_absent_with_why_and_a_missing_program_too() {
        let d = declared::parse("gpio.md", include_str!("../../../capabilities/gpio.md")).unwrap();
        if !Path::new("/dev/gpiochip0").exists() {
            assert_eq!(probe(&d), State::Absent("probe `test -e /dev/gpiochip0` exited 1".into()));
        }
        let none = declared::parse("n.md", "# n\ndoc\nprobe: dry-no-such-program-here\n").unwrap();
        assert!(matches!(probe(&none), State::Absent(w) if w.contains("did not run")));
        let slow = declared::parse("s.md", "# s\ndoc\n## tool: wait\ncommand: sleep 5\ntimeout: 1s\n").unwrap();
        let r = Routed { provider: Provider::Declared, capability: "s".into(), tool: "wait".into(), args: vec![] };
        assert_eq!(call(&[slow], &r).unwrap_err(), "sleep: timed out after 1s");
    }

    #[test]
    fn every_draft_reads_and_none_acts_without_approval() {
        for d in discover() {
            let parsed = declared::parse(&format!("{}.md", d.id), &d.text).unwrap();
            for t in &parsed.tools {
                let acts = ["set", "stop"].contains(&t.tool.name.as_str());
                assert!(!acts || t.tool.approval, "{}.{} acts: it needs approval", d.id, t.tool.name);
            }
        }
    }

    #[test]
    fn the_shipped_files_load() {
        let (all, errors) = load(&crate::root().join("capabilities"));
        assert!(errors.is_empty(), "{errors:?}");
        let ids: Vec<&str> = all.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["battery", "gpio", "sensors"]);
    }
}
