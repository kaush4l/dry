//! Capabilities over MCP (docs/companion-protocol.md §9–§10): the
//! companion's own (file, terminal, browser, evidence, git, container,
//! model.<name>) and the declared `capabilities/<id>.md` (read by
//! `dry_core::capability::declared`, run by `crate::capability::run_argv`:
//! argv only, never a shell), each with its switch; the MCP server at
//! `/mcp` (initialize with the hello, tools, resources, calls, ping) and
//! the peers' streams (list_changed, events, heartbeats).

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use dry_core::capability::declared::{self, Declared};
use dry_core::capability::{Event, State};
use serde_json::{Value, json};

use super::folder;
use super::http::{self, Request};
use super::rules::{Fail, fail};
use super::state::{Companion, McpStream, NAME, RUNTIME, VERSION};
use super::term;

/// How long a declared capability's probe answer is kept.
const PROBE_FOR: Duration = Duration::from_secs(30);
const MCP_MODERN: [&str; 1] = ["2026-07-28"];
const MCP_LEGACY: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
pub const BUILTIN: [&str; 6] = ["file", "terminal", "browser", "evidence", "git", "container"];

/// A capability's state block: where it comes from.
#[derive(Clone)]
enum Snapshot {
    None,
    Declared,
    Terminal,
    Container(PathBuf),
    Text(String),
}

/// One tool of a capability, as `tools/list` carries it.
#[derive(Clone)]
struct CapTool {
    name: String,
    description: String,
    schema: Value,
    approval: bool,
}

/// One capability, now.
#[derive(Clone)]
struct Cap {
    id: String,
    state: State,
    doc: String,
    tools: Vec<CapTool>,
    declared: Option<Declared>,
    snapshot: Snapshot,
    off: bool,
}

fn tool(name: &str, description: &str, props: Value, required: &[&str], approval: bool) -> CapTool {
    CapTool {
        name: name.into(),
        description: description.into(),
        schema: json!({ "type": "object", "properties": props, "required": required }),
        approval,
    }
}

/// The declared files in `--capabilities`, read; and the ones that did not read.
pub fn declared(c: &Companion) -> (Vec<Declared>, Vec<String>) {
    match &c.cap_dir {
        Some(d) => crate::capability::load(d),
        None => (Vec::new(), Vec::new()),
    }
}

fn cap_cwd(c: &Companion) -> PathBuf {
    c.cap_dir.clone().unwrap_or_else(|| c.folder.clone())
}

/// A declared capability's state: its probe's answer (kept 30 s).
pub fn probe(c: &Companion, d: &Declared, fresh: bool) -> State {
    if !fresh && let Some((s, at)) = c.probes.lock().unwrap().get(&d.id).filter(|(_, at)| at.elapsed() < PROBE_FOR) {
        let _ = at;
        return s.clone();
    }
    let s = match &d.probe {
        None => State::Present,
        Some(p) => {
            declared::probed(p, crate::capability::run_argv(&p[0], &p[1..], &[], declared::TIMEOUT, &cap_cwd(c)).map(|(code, _)| code))
        }
    };
    c.probes.lock().unwrap().insert(d.id.clone(), (s.clone(), Instant::now()));
    s
}

fn word(s: &State) -> &'static str {
    s.word()
}

/// The browser can run here: a Chrome to drive.
pub fn chrome(c: &Companion) -> Option<PathBuf> {
    crate::cdp::find_chrome(c.chrome.as_deref())
}

/// The companion's own capabilities, then one `model.<name>` per provider.
fn builtins(c: &Companion) -> Vec<Cap> {
    let mk = |id: &str, state: State, doc: String, tools: Vec<CapTool>, snapshot: Snapshot| Cap {
        id: id.into(),
        state,
        doc,
        tools,
        declared: None,
        snapshot,
        off: false,
    };
    let present_if = |ok: bool, why: &str| if ok { State::Present } else { State::Absent(why.into()) };
    let chrome = chrome(c);
    let containers: Vec<(&str, PathBuf)> = ["docker", "podman"].into_iter().filter_map(|n| c.program(n).map(|p| (n, p))).collect();
    let s = json!({ "type": "string" });
    let mut out = vec![
        mk(
            "file",
            State::Present,
            format!("The workspace folder's files: list, read, write, stat (over /v1/fs; list and read here). The folder is {}. Secrets, .git and .dry are never shown.", c.folder.display()),
            vec![
                tool("file.list", "The files under a folder of the workspace (at most 200), paths relative to it.", json!({ "path": { "type": "string", "description": "a folder in the workspace; empty: all of it" } }), &[], false),
                tool("file.read", "One file's text.", json!({ "path": { "type": "string", "description": "relative to the workspace folder" } }), &["path"], false),
            ],
            Snapshot::None,
        ),
        mk(
            "terminal",
            State::Present,
            format!("Commands in the workspace folder, in this machine's shell ({}): run, start, tail, stop (over /v1/term). A dry engine's terminal artifact uses it through the companion.", std::env::consts::OS),
            vec![tool("terminal.run", "Run a command in the workspace folder (a shell) and wait: its exit code and last lines.", json!({ "command": s, "cwd": { "type": "string", "description": "a folder in the workspace (default: the folder)" } }), &["command"], true)],
            Snapshot::Terminal,
        ),
        mk(
            "browser",
            present_if(chrome.is_some(), "no Chrome, Chromium, Edge or Brave found (give one with --chrome PATH)"),
            "A headless browser on this machine for its own pages (localhost): open, look, click, type, screenshot.".into(),
            super::browser::tools().into_iter().map(|(n, d, p, r)| tool(n, d, p, r, false)).collect(),
            Snapshot::None,
        ),
        mk(
            "evidence",
            present_if(chrome.is_some(), "evidence shoots pages in Chrome: no Chrome, Chromium, Edge or Brave found (--chrome PATH)"),
            "The workspace's evidence (its evidence.md, docs/evidence.md): run it, read the last one. Run in this companion (dry's own evidence code).".into(),
            vec![
                tool("evidence_run", "Run a project's evidence (its evidence.md): build, start the app, screenshot every scenario and compare with the committed evidence/ folder. One line back, then the summary.", json!({ "project": { "type": "string", "description": "a folder of the workspace (default: the folder)" }, "expect": { "type": "string", "description": "scenarios expected to change, comma separated" }, "update": { "type": "boolean" } }), &[], false),
                tool("evidence_read", "A project's last evidence: one line, then the summary (which shots changed, by how much, the page-text lines).", json!({ "project": { "type": "string" } }), &[], false),
            ],
            Snapshot::None,
        ),
        mk(
            "git",
            present_if(c.program("git").is_some(), "no git on PATH"),
            "git in the workspace folder, with this machine's credentials: status here; push and clone need a person's click.".into(),
            vec![tool("git.status", "The folder's (or a folder of it) branch and changed files.", json!({ "path": { "type": "string", "description": "a folder of the workspace (default: the folder)" } }), &[], false)],
            Snapshot::None,
        ),
        mk(
            "container",
            present_if(!containers.is_empty(), "no docker or podman on PATH"),
            format!(
                "Containers on this machine ({}): run them with the terminal.",
                if containers.is_empty() { "none".to_string() } else { containers.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ") }
            ),
            Vec::new(),
            containers.first().map_or(Snapshot::None, |(_, p)| Snapshot::Container(p.clone())),
        ),
    ];
    // Each declared provider is a `model.<name>` capability: its models.
    let pv = super::providers::now(c, false);
    for p in pv["providers"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or("");
        let models: Vec<&str> = p["models"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let ok = p["state"] == "ok";
        let hint = p["hint"].as_str().filter(|h| !h.is_empty()).map(|h| format!(" ({h})")).unwrap_or_default();
        let state = if ok {
            State::Present
        } else {
            State::Absent(format!("{}: {}{hint}", p["state"].as_str().unwrap_or("failed"), p["why"].as_str().unwrap_or("")))
        };
        let at = p["url"].as_str().map(|u| format!(" at {u}")).unwrap_or_default();
        out.push(mk(
            &format!("model.{name}"),
            state,
            format!(
                "Models from {name} ({}{at}), through this companion's /v1/chat/completions: {}.",
                p["kind"].as_str().unwrap_or(""),
                if models.is_empty() { "none".into() } else { models.join(", ") }
            ),
            Vec::new(),
            Snapshot::Text(if models.is_empty() { "no models".into() } else { models.join("\n") }),
        ));
    }
    out
}

/// Every capability: the companion's own, then the declared ones, probed;
/// one switched off is absent saying how to switch it on. And the files
/// that did not read.
fn all(c: &Companion, fresh: bool) -> (Vec<Cap>, Vec<String>) {
    let mut out = builtins(c);
    let (list, errors) = declared(c);
    for d in list {
        let state = probe(c, &d, fresh);
        let tools = d
            .tools
            .iter()
            .map(|t| CapTool {
                name: format!("{}.{}", d.id, t.tool.name),
                description: t.tool.doc.clone(),
                schema: t.tool.schema(),
                approval: t.tool.approval,
            })
            .collect();
        out.push(Cap { id: d.id.clone(), state, doc: d.doc.clone(), tools, snapshot: Snapshot::Declared, declared: Some(d), off: false });
    }
    for cap in &mut out {
        if !c.is_on(&cap.id) {
            cap.off = true;
            cap.state = State::Absent(format!("switched off in the companion's terminal (type: on {})", cap.id));
        }
    }
    (out, errors)
}

fn meta(cap: &Cap) -> Value {
    let mut m = json!({ "capability": cap.id, "doc": cap.doc, "state": word(&cap.state) });
    if !cap.state.why().is_empty() {
        m["why"] = json!(cap.state.why());
    }
    m
}

fn events_of(cap: &Cap) -> Vec<Event> {
    cap.declared.as_ref().map(|d| d.events.clone()).unwrap_or_default()
}

pub fn tools_list(c: &Companion) -> Value {
    let (caps, _) = all(c, false);
    let tools: Vec<Value> = caps
        .iter()
        .flat_map(|cap| {
            cap.tools.iter().map(move |t| {
                let mut m = meta(cap);
                m["approval"] = json!(t.approval);
                json!({ "name": t.name, "description": t.description, "inputSchema": t.schema, "_meta": m })
            })
        })
        .collect();
    json!({ "tools": tools })
}

pub fn resources_list(c: &Companion) -> Value {
    let (caps, _) = all(c, false);
    let resources: Vec<Value> = caps
        .iter()
        .map(|cap| {
            let mut m = meta(cap);
            m["snapshot"] = json!(!matches!(cap.snapshot, Snapshot::None) && !(matches!(cap.snapshot, Snapshot::Declared) && cap.declared.as_ref().is_some_and(|d| d.snapshot.is_none())));
            m["interval"] = cap.declared.as_ref().map_or(Value::Null, |d| json!(d.interval));
            m["events"] = json!(events_of(cap).iter().map(|e| json!({ "name": e.name, "when": e.when.text(), "text": e.text })).collect::<Vec<_>>());
            json!({ "uri": format!("dry://capability/{}", cap.id), "name": cap.id, "description": cap.doc.lines().next().unwrap_or(""), "mimeType": "text/plain", "_meta": m })
        })
        .collect();
    json!({ "resources": resources })
}

fn run_snapshot(c: &Companion, d: &Declared) -> String {
    let Some(s) = &d.snapshot else { return String::new() };
    match crate::capability::run_argv(&s[0], &s[1..], &[], declared::TIMEOUT, &cap_cwd(c)) {
        Ok((0, out)) => out.trim_end().to_string(),
        Ok((_, out)) => format!("snapshot failed: {}", out.trim()),
        Err(e) => format!("snapshot failed: {e}"),
    }
}

/// A capability's snapshot now (`resources/read`).
pub fn read(c: &Companion, uri: &str) -> Result<String, Fail> {
    let id = uri.strip_prefix("dry://capability/").unwrap_or(uri);
    let (caps, _) = all(c, false);
    let cap = caps.into_iter().find(|x| x.id == id).ok_or_else(|| fail(404, "not_found", format!("no capability {id}")))?;
    if cap.state != State::Present {
        return Ok(format!("({}: {})", word(&cap.state), cap.state.why()));
    }
    Ok(match &cap.snapshot {
        Snapshot::None => String::new(),
        Snapshot::Declared => cap.declared.as_ref().map(|d| run_snapshot(c, d)).unwrap_or_default(),
        Snapshot::Text(t) => t.clone(),
        Snapshot::Terminal => {
            let run: Vec<String> = c
                .sessions
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.running())
                .map(|s| format!("running {}: {}", s.key.clone().unwrap_or_else(|| s.id.clone()), s.command))
                .collect();
            if run.is_empty() { "nothing running".into() } else { run.join("\n") }
        }
        Snapshot::Container(bin) => {
            match crate::capability::run_argv(
                &bin.display().to_string(),
                &["ps".into(), "--format".into(), "{{.Names}} {{.Status}}".into()],
                &[],
                10,
                &c.folder,
            ) {
                Ok((0, out)) if out.trim().is_empty() => "no containers running".into(),
                Ok((0, out)) => out.trim().to_string(),
                Ok((_, out)) => format!("ps failed: {}", out.trim()),
                Err(e) => format!("ps failed: {e}"),
            }
        }
    })
}

fn text(t: impl Into<String>, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": t.into() }], "isError": is_error })
}

fn error_result(f: &Fail) -> Value {
    let mut v = text(&f.error, true);
    v["_meta"] = json!({ "code": f.code });
    v
}

/// A call's arguments as text values (a number or a boolean as written).
fn args_of(v: &Value) -> Result<Vec<(String, String)>, Fail> {
    let m = match v {
        Value::Null => return Ok(Vec::new()),
        Value::Object(m) => m,
        _ => return Err(fail(400, "bad_request", "arguments is an object")),
    };
    let mut out = Vec::new();
    for (k, v) in m {
        match v {
            Value::Null => {}
            Value::String(s) => out.push((k.clone(), s.clone())),
            Value::Number(_) | Value::Bool(_) => out.push((k.clone(), v.to_string())),
            _ => return Err(fail(400, "bad_request", format!("{k} must be text"))),
        }
    }
    Ok(out)
}

/// The capability a tool name belongs to.
fn cap_of(name: &str) -> Option<&str> {
    if let Some(i) = name.rfind('.') {
        Some(&name[..i])
    } else if name.starts_with("browser_") {
        Some("browser")
    } else if name.starts_with("evidence_") {
        Some("evidence")
    } else {
        None
    }
}

/// `tools/call`: the result (an error is `isError` with `_meta.code`);
/// `None`: no such tool (JSON-RPC -32602).
pub fn call(c: &Arc<Companion>, name: &str, args: &Value) -> Option<Value> {
    let decl = declared(c).0;
    let known = |id: &str| BUILTIN.contains(&id) || decl.iter().any(|d| d.id == id);
    let id = cap_of(name).filter(|id| known(id))?;
    let builtin_tool = matches!(name, "terminal.run" | "file.read" | "file.list" | "git.status" | "evidence_run" | "evidence_read")
        || super::browser::tools().iter().any(|(n, ..)| *n == name);
    if BUILTIN.contains(&id) && !builtin_tool {
        return None;
    }
    if let Err(f) = c.need(id) {
        return Some(error_result(&f));
    }
    let r = (|| -> Result<Value, Fail> {
        let a = &args;
        let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match name {
            "terminal.run" => {
                let command = s("command");
                if command.trim().is_empty() {
                    return Err(fail(400, "bad_request", "terminal.run needs command"));
                }
                term::check_publishing(&command)?;
                let cwd = folder::ws_path(c, &s("cwd"), false, true)?;
                let sess = term::launch(c, &cwd.real, &command, "run", None, Some(Duration::from_secs(30 * 60)));
                sess.wait();
                let v = sess.view(None);
                let tail = dry_core::terminal::last_lines(&sess.output(256 * 1024), 15);
                Ok(text(format!("exit {}\n{tail}", sess.exit_word()), v["exit"] != json!(0)))
            }
            "file.read" => {
                let r = folder::read(c, &s("path"))?;
                Ok(match r["kind"].as_str() {
                    Some("text") => text(r["text"].as_str().unwrap_or(""), false),
                    _ => text(
                        format!("{}: {} ({} bytes)", r["path"].as_str().unwrap_or(""), r["kind"].as_str().unwrap_or(""), r["bytes"]),
                        true,
                    ),
                })
            }
            "file.list" => {
                let r = folder::list(c, &s("path"), 200)?;
                let files: Vec<&str> = r["files"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                Ok(text(files.join("\n") + if r["truncated"] == json!(true) { "\n…" } else { "" }, false))
            }
            "git.status" => {
                let dir = folder::ws_path(c, &s("path"), false, true)?;
                let git = c.program("git").ok_or_else(|| fail(503, "unavailable", "no git on PATH"))?;
                let (code, out) = crate::capability::run_argv(
                    &git.display().to_string(),
                    &["status".into(), "--porcelain=v1".into(), "-b".into()],
                    &[("GIT_TERMINAL_PROMPT".into(), "0".into())],
                    120,
                    &dir.real,
                )
                .map_err(|e| fail(500, "internal", e))?;
                Ok(text(out, code != 0))
            }
            "evidence_run" | "evidence_read" => {
                let r = if name == "evidence_run" { super::evidence::run(c, args)? } else { super::evidence::read(c, &s("project"))? };
                let exit = r.get("code").and_then(Value::as_i64).map(|k| format!(" (exit {k})")).unwrap_or_default();
                let body = r["summary"].as_str().filter(|t| !t.is_empty()).or(r["output"].as_str()).unwrap_or("");
                Ok(text(
                    format!("evidence: {}{exit}\n\n{body}", r["line"].as_str().unwrap_or("")),
                    r["code"].as_i64().is_some_and(|k| k > 1),
                ))
            }
            n if n.starts_with("browser_") => super::browser::mcp(c, n, args),
            _ => {
                let d = decl.iter().find(|d| d.id == id).ok_or_else(|| fail(404, "not_found", format!("no capability {id}")))?;
                let st = probe(c, d, false);
                if st != State::Present {
                    return Ok(text(format!("{} is {}: {}", d.id, word(&st), st.why()), true));
                }
                let argv = d.argv(&name[id.len() + 1..], &args_of(args)?).map_err(|e| fail(400, "bad_request", e))?;
                match crate::capability::run_argv(&argv.program, &argv.args, &argv.env, argv.timeout, &cap_cwd(c)) {
                    Ok((code, out)) => {
                        let out = out.trim_end();
                        Ok(text(if out.is_empty() { "(done, no output)" } else { out }, code != 0))
                    }
                    Err(e) => Ok(text(e, true)),
                }
            }
        }
    })();
    Some(r.unwrap_or_else(|f| error_result(&f)))
}

/// For `/v1/capabilities`, `/v1/switches` and the terminal: each one's id,
/// state, why, tools, whether it is declared, on or off.
pub fn summary(c: &Companion, fresh: bool) -> Value {
    let (caps, errors) = all(c, fresh);
    let list: Vec<Value> = caps
        .iter()
        .map(|x| {
            json!({ "id": x.id, "state": if x.off { "present" } else { word(&x.state) }, "why": if x.off { "" } else { x.state.why() },
                "tools": x.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>(), "declared": x.declared.is_some(),
                "on": !x.off, "actuator": c.actuator(&x.id), "saved": c.saved(&x.id), "doc": x.doc.lines().next().unwrap_or("") })
        })
        .collect();
    json!({ "dir": c.cap_dir, "list": list, "errors": errors })
}

/// While a peer listens: the declared files read again (a change is
/// `tools/list_changed`: the peer re-registers), and the snapshots read
/// at their intervals (an event that became true rides on its update).
pub fn tick(c: &Companion) {
    let (list, _) = declared(c);
    let states: Vec<State> = list.iter().map(|d| probe(c, d, false)).collect();
    let sig = format!("{list:?}{states:?}");
    {
        let mut had = c.cap_sig.lock().unwrap();
        if had.as_ref().is_some_and(|h| *h != sig) {
            c.notify("notifications/tools/list_changed", json!({}));
        }
        *had = Some(sig);
    }
    for (d, st) in list.iter().zip(&states) {
        if d.snapshot.is_none() || *st != State::Present {
            continue;
        }
        let fresh = c.snaps.lock().unwrap().get(&d.id).is_some_and(|s| s.at.elapsed() < Duration::from_secs(d.interval));
        if fresh {
            continue;
        }
        let snap = run_snapshot(c, d);
        let holds: Vec<bool> = d.events.iter().map(|e| e.when.holds(&snap)).collect();
        let had = c
            .snaps
            .lock()
            .unwrap()
            .insert(d.id.clone(), super::state::Snap { text: snap.clone(), at: Instant::now(), holds: holds.clone() });
        // The first reading never fires.
        let Some(had) = had else { continue };
        let uri = format!("dry://capability/{}", d.id);
        let mut fired = false;
        for (i, e) in d.events.iter().enumerate() {
            if holds[i] && !had.holds.get(i).copied().unwrap_or(false) {
                fired = true;
                c.log(&format!("capability {}: event {}", d.id, e.name));
                c.notify(
                    "notifications/resources/updated",
                    json!({ "uri": uri, "_meta": { "event": { "name": e.name, "text": e.text } } }),
                );
            }
        }
        if !fired && snap != had.text {
            c.notify("notifications/resources/updated", json!({ "uri": uri }));
        }
    }
}

// ---- MCP: streamable HTTP at /mcp ----

fn decode_header(v: &str) -> String {
    match v.strip_prefix("=?base64?").and_then(|r| r.strip_suffix("?=")) {
        Some(b) => crate::cdp::base64(b).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default(),
        None => v.to_string(),
    }
}

/// A JSON-RPC answer: status and body.
type Reply = (u16, Option<Value>);

fn rpc_error(status: u16, id: &Value, code: i64, message: &str, data: Option<Value>) -> Reply {
    let mut e = json!({ "code": code, "message": message });
    if let Some(d) = data {
        e["data"] = d;
    }
    let mut v = json!({ "jsonrpc": "2.0", "error": e });
    if !id.is_null() {
        v["id"] = id.clone();
    }
    (status, Some(v))
}

/// `/mcp`: a peer's stream (GET with `Accept: text/event-stream`) or one
/// JSON-RPC request (POST). Writes the answer; its status.
pub fn handle(c: &Arc<Companion>, req: &Request, s: &TcpStream, cors: &[(String, String)], pairing: &str) -> u16 {
    if req.method == "GET" && req.header("accept").is_some_and(|a| a.contains("text/event-stream")) {
        stream(c, s, cors, pairing);
        return 200;
    }
    if req.method != "POST" {
        let mut h = cors.to_vec();
        h.push(("allow".into(), "POST".into()));
        http::write(s, 405, &h, b"");
        return 405;
    }
    let (status, body) = rpc(c, req, pairing);
    let mut h = cors.to_vec();
    h.push(("content-type".into(), "application/json".into()));
    http::write(s, status, &h, body.map(|b| b.to_string()).unwrap_or_default().as_bytes());
    status
}

fn rpc(c: &Arc<Companion>, req: &Request, pairing: &str) -> Reply {
    let null = Value::Null;
    let msg: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) if req.body.is_empty() => json!({}),
        Err(_) => return rpc_error(400, &null, -32700, "Parse error", None),
    };
    let method = msg.get("method").and_then(Value::as_str);
    if !msg.is_object() || msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_none() {
        return rpc_error(400, &null, -32600, "Invalid Request", None);
    }
    let method = method.unwrap_or_default();
    let Some(id) = msg.get("id").cloned() else { return (202, None) };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let modern = params.pointer("/_meta/io.modelcontextprotocol~1protocolVersion").and_then(Value::as_str);
    let header = req.header("mcp-protocol-version");
    let supported = || json!(MCP_MODERN.iter().chain(MCP_LEGACY.iter()).collect::<Vec<_>>());
    if let Some(v) = modern {
        // A modern request: stateless, its version and method mirrored in headers.
        if !MCP_MODERN.contains(&v) {
            return rpc_error(400, &id, -32022, "Unsupported protocol version", Some(json!({ "supported": supported(), "requested": v })));
        }
        if header != Some(v) {
            return rpc_error(
                400,
                &id,
                -32020,
                &format!("Header mismatch: MCP-Protocol-Version {} does not match {v}", header.unwrap_or("(missing)")),
                None,
            );
        }
        if req.header("mcp-method") != Some(method) {
            return rpc_error(
                400,
                &id,
                -32020,
                &format!("Header mismatch: Mcp-Method {} does not match {method}", req.header("mcp-method").unwrap_or("(missing)")),
                None,
            );
        }
        if method == "tools/call" && Some(decode_header(req.header("mcp-name").unwrap_or("")).as_str()) != params["name"].as_str() {
            return rpc_error(400, &id, -32020, &format!("Header mismatch: Mcp-Name does not match {}", params["name"]), None);
        }
    } else if let Some(h) = header
        && !MCP_LEGACY.contains(&h)
    {
        return rpc_error(400, &id, -32022, "Unsupported protocol version", Some(json!({ "supported": supported(), "requested": h })));
    }
    let info = json!({ "name": NAME, "version": VERSION });
    let result = match method {
        "initialize" => {
            if let Some(hello) = params.pointer("/_meta/hello").filter(|h| h.is_object()) {
                c.with_pairing(pairing, |p| p.hello = Some(hello.clone()));
                c.log(&format!(
                    "{pairing} is {} ({}{})",
                    hello["runtime"].as_str().unwrap_or("?"),
                    hello["platform"].as_str().unwrap_or("?"),
                    hello["version"].as_str().map(|v| format!(" {v}")).unwrap_or_default()
                ));
            }
            let asked = params["protocolVersion"].as_str().filter(|v| MCP_LEGACY.contains(v)).unwrap_or(MCP_LEGACY[0]);
            let hello = crate::capability::hello(RUNTIME);
            json!({
                "protocolVersion": asked,
                "capabilities": { "tools": { "listChanged": true }, "resources": { "listChanged": true, "subscribe": false } },
                "serverInfo": info,
                "instructions": "This machine's capabilities (terminal, file, browser, evidence, git, container, models, and capabilities/*.md), grouped by _meta.capability.",
                "_meta": { "hello": { "runtime": hello.runtime, "platform": hello.platform, "version": hello.version, "protocol": hello.protocol } },
            })
        }
        "server/discover" => json!({ "supportedVersions": supported(), "capabilities": { "tools": {} }, "serverInfo": info }),
        "ping" => json!({}),
        "tools/list" => tools_list(c),
        "resources/list" => resources_list(c),
        "resources/read" => match read(c, params["uri"].as_str().unwrap_or("")) {
            Ok(t) => json!({ "contents": [{ "uri": params["uri"], "mimeType": "text/plain", "text": t }] }),
            Err(f) => return rpc_error(if f.status == 404 { 200 } else { 400 }, &id, -32002, &f.error, None),
        },
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("").to_string();
            let t0 = Instant::now();
            c.log(&format!("call {name} from {pairing} …"));
            match call(c, &name, &params["arguments"]) {
                Some(r) => {
                    c.log(&format!(
                        "call {name} {} {}ms",
                        if r["isError"] == json!(true) { "failed" } else { "done" },
                        t0.elapsed().as_millis()
                    ));
                    r
                }
                None => return rpc_error(200, &id, -32602, &format!("no tool {name}"), None),
            }
        }
        _ if modern.is_some() => return rpc_error(404, &id, -32601, "Method not found", None),
        _ => return rpc_error(200, &id, -32601, "Method not found", None),
    };
    (200, Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })))
}

/// A peer's stream: notifications and heartbeats until it closes (or its pairing ends).
fn stream(c: &Arc<Companion>, s: &TcpStream, cors: &[(String, String)], pairing: &str) {
    let mut h = cors.to_vec();
    h.extend([("content-type".into(), "text/event-stream".into()), ("cache-control".into(), "no-store".into())]);
    let Ok(sock) = s.try_clone() else { return };
    let sink = Arc::new(std::sync::Mutex::new(sock));
    let mut head = http::head(200, &h, None);
    head.extend_from_slice(b": dry-companion capabilities stream\n\n");
    if !http::send(&sink, &String::from_utf8_lossy(&head)) {
        return;
    }
    let n = c.next_stream.fetch_add(1, Ordering::Relaxed);
    c.mcp_streams.lock().unwrap().push(McpStream { sink, pairing: pairing.into(), n });
    c.with_pairing(pairing, |p| p.streams += 1);
    c.log(&format!("peer {pairing} listening"));
    http::wait_closed(s);
    c.mcp_streams.lock().unwrap().retain(|m| m.n != n);
    c.with_pairing(pairing, |p| p.streams = p.streams.saturating_sub(1));
    c.log(&format!("peer {pairing} stopped listening"));
}
