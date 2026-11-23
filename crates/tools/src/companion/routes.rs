//! Every request, in the contract's order (docs/companion-protocol.md
//! §2–§11): the Host, the Origin, a preflight, the served app, `/mcp`,
//! health and pairing (no token), then the token, then the calls.

use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::http::{self, Request, Sink};
use super::rules::{self, Fail, fail};
use super::state::{Companion, Listener, NAME, PROTOCOL, RUNTIME, VERSION, sse, wants};
use super::{browser, caps, evidence, folder, providers, term};

/// Reads the page makes at every render: not lines (`--verbose` shows them).
const QUIET_GETS: [&str; 11] = [
    "/v1/fs/read",
    "/v1/fs/list",
    "/v1/fs/stat",
    "/v1/term/tail",
    "/v1/health",
    "/v1/workspace",
    "/v1/pairings",
    "/v1/providers",
    "/v1/models",
    "/v1/store",
    "/v1/switches",
];
const QUIET_POSTS: [&str; 2] = ["/v1/store", "/v1/workspace/snapshot"];

const MIME: [(&str, &str); 24] = [
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("json", "application/json"),
    ("wasm", "application/wasm"),
    ("svg", "image/svg+xml"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("ico", "image/x-icon"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("txt", "text/plain; charset=utf-8"),
    ("md", "text/markdown; charset=utf-8"),
    ("map", "application/json"),
    ("ts", "text/plain; charset=utf-8"),
    ("gz", "application/gzip"),
    ("bin", "application/octet-stream"),
    ("img", "application/octet-stream"),
];

/// What a route answers.
enum Out {
    Json(Value),
    /// Written already (a stream, a passthrough); its status.
    Done(u16),
}

/// One connection: one request, its answer, a line in the terminal.
pub fn connection(c: Arc<Companion>, s: TcpStream) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(30)));
    let Some(req) = http::read(&s) else { return };
    let t0 = Instant::now();
    let origin = req.header("origin").unwrap_or("").to_string();
    let cors = cors(&c, &origin);
    let (status, note) = match answer(&c, &req, &s, &origin, &cors) {
        Ok((Out::Json(v), note)) => {
            send(&s, 200, &v, &cors);
            (200, note)
        }
        Ok((Out::Done(st), note)) => (st, note),
        Err(f) => {
            send(&s, f.status, &f.json(), &cors);
            (f.status, f.error.chars().take(80).collect())
        }
    };
    let p = req.path.as_str();
    let quiet = (req.method == "GET" && QUIET_GETS.contains(&p))
        || (req.method == "POST" && QUIET_POSTS.contains(&p))
        || note == "app"
        || (p == "/mcp" && note.is_empty());
    if !c.verbose && p == "/favicon.ico" {
        return;
    }
    if c.verbose || status >= 400 || !quiet {
        let from = if origin.is_empty() {
            String::new()
        } else {
            format!(" {}", origin.trim_start_matches("https://").trim_start_matches("http://"))
        };
        let note = if note.is_empty() { String::new() } else { format!(" {note}") };
        c.log(&format!("{:<4} {p} {status} {}ms{from}{note}", req.method, t0.elapsed().as_millis()));
    }
}

fn cors(c: &Companion, origin: &str) -> Vec<(String, String)> {
    if rules::origin_allowed(origin, &c.origins) {
        vec![("access-control-allow-origin".into(), origin.into()), ("vary".into(), "Origin".into())]
    } else {
        Vec::new()
    }
}

fn send(s: &TcpStream, status: u16, v: &Value, cors: &[(String, String)]) {
    let mut h = cors.to_vec();
    h.push(("content-type".into(), "application/json; charset=utf-8".into()));
    h.push(("cache-control".into(), "no-store".into()));
    http::write(s, status, &h, v.to_string().as_bytes());
}

fn body(req: &Request) -> Result<Value, Fail> {
    if req.too_large {
        return Err(fail(413, "too_large", "the body is too large (8 MB at most)"));
    }
    if req.body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&req.body).map_err(|_| fail(400, "bad_request", "the body is not JSON"))
}

fn token(req: &Request) -> Option<&str> {
    if let Some(t) = req.header("x-dry-token").filter(|t| !t.is_empty()) {
        return Some(t);
    }
    req.header("authorization").and_then(|a| a.strip_prefix("Bearer ")).map(str::trim)
}

/// The built app's files (`--app`): `/` and `/app/…`, confined to its folder.
fn serve_app(c: &Companion, req: &Request, s: &TcpStream) -> Option<u16> {
    let dir = c.app_dir.as_ref()?;
    if req.method != "GET" && req.method != "HEAD" {
        return None;
    }
    if req.path == "/app" {
        http::write(s, 301, &[("location".into(), "/app/".into())], b"");
        return Some(301);
    }
    let mut full = req.path.trim_start_matches('/').split('/').filter(|p| !p.is_empty()).fold(dir.clone(), |a, p| a.join(p));
    if full.is_dir() {
        full = full.join("index.html");
    }
    let real = full.canonicalize().ok()?;
    if !real.starts_with(dir) {
        return None;
    }
    let bytes = std::fs::read(&real).ok()?;
    let ext = real.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ctype = MIME.iter().find(|(e, _)| *e == ext).map_or("application/octet-stream", |(_, t)| t);
    let h: Vec<(String, String)> = [
        ("content-type", ctype),
        ("cache-control", "no-cache"),
        ("x-content-type-options", "nosniff"),
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-embedder-policy", "require-corp"),
        ("cross-origin-resource-policy", "same-origin"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if req.method == "HEAD" {
        let _ = std::io::Write::write_all(&mut &*s, &http::head(200, &h, Some(bytes.len())));
    } else {
        http::write(s, 200, &h, &bytes);
    }
    Some(200)
}

fn answer(c: &Arc<Companion>, req: &Request, s: &TcpStream, origin: &str, cors: &[(String, String)]) -> Result<(Out, String), Fail> {
    let p = req.path.as_str();
    let (get, post) = (req.method == "GET", req.method == "POST");
    let none = String::new;
    if !rules::host_allowed(req.header("host").unwrap_or(""), c.port()) {
        return Err(fail(403, "host_refused", "this server answers only to 127.0.0.1 / localhost"));
    }
    if !origin.is_empty() && !rules::origin_allowed(origin, &c.origins) {
        return Err(fail(403, "origin_refused", format!("origin {origin} is not allowed (add it with --origin)")));
    }
    if req.method == "OPTIONS" {
        let mut h = cors.to_vec();
        for (k, v) in [
            ("access-control-allow-methods", "GET, POST, OPTIONS"),
            (
                "access-control-allow-headers",
                "content-type, authorization, x-dry-token, last-event-id, mcp-protocol-version, mcp-method, mcp-name, accept",
            ),
            ("access-control-allow-private-network", "true"),
            ("access-control-max-age", "600"),
        ] {
            h.push((k.into(), v.into()));
        }
        http::write(s, 204, &h, b"");
        return Ok((Out::Done(204), none()));
    }
    if !p.starts_with("/v1/") && !p.starts_with("/api/") && p != "/mcp" {
        if let Some(st) = serve_app(c, req, s) {
            return Ok((Out::Done(st), "app".into()));
        }
        if c.app_dir.is_some() && (get || req.method == "HEAD") {
            return Err(fail(404, "not_found", format!("no file {p} in the app")));
        }
    }
    if p == "/mcp" {
        let Some(pairing) = c.pairing_of(token(req)) else {
            let mut h = cors.to_vec();
            h.push(("content-type".into(), "application/json".into()));
            h.push(("www-authenticate".into(), "Bearer realm=\"dry-companion\"".into()));
            let v = json!({ "error": "not paired: use the pairing token as a Bearer token", "code": "not_paired" });
            http::write(s, 401, &h, v.to_string().as_bytes());
            return Ok((Out::Done(401), none()));
        };
        return Ok((Out::Done(caps::handle(c, req, s, cors, &pairing)), none()));
    }
    if p == "/v1/health" && get {
        let paired = c.pairing_of(token(req)).is_some();
        return Ok((
            Out::Json(
                json!({ "ok": true, "name": NAME, "version": VERSION, "protocol": PROTOCOL, "paired": paired, "app": c.app_dir.is_some(), "impl": RUNTIME }),
            ),
            none(),
        ));
    }
    if p == "/v1/pair" && post {
        let b = body(req)?;
        let r = c.pair(b["code"].as_str().unwrap_or(""), origin)?;
        let note = format!("paired {}", r["id"].as_str().unwrap_or(""));
        return Ok((Out::Json(r), note));
    }
    let tok = token(req).map(String::from);
    let Some(me) = c.pairing_of(tok.as_deref()) else {
        return Err(fail(401, "not_paired", "not paired (or the pairing expired or was revoked): send a pairing code to /v1/pair"));
    };
    let q = |k: &str| req.q(k).unwrap_or("").to_string();
    let json = |v: Value| Ok((Out::Json(v), String::new()));
    match (req.method.as_str(), p) {
        ("POST", "/v1/unpair") => {
            c.end_pairing(&me, "left");
            json(json!({ "ok": true }))
        }
        ("GET", "/v1/pairings") => json(json!({ "pairings": c.list_pairings(Some(&me)) })),
        ("POST", "/v1/pairings/revoke") => {
            let id = body(req)?["id"].as_str().unwrap_or("").to_string();
            if !c.end_pairing(&id, "revoked") {
                return Err(fail(404, "not_found", format!("no pairing {id}")));
            }
            Ok((Out::Json(json!({ "ok": true })), format!("revoked {id}")))
        }
        ("POST", "/v1/stop-all") => {
            let r = stop_all(c);
            let note = format!("stopped {}", r["stopped"]);
            Ok((Out::Json(r), note))
        }
        ("GET", "/v1/workspace") => json(folder::workspace(c)),
        ("POST", "/v1/workspace/snapshot") => Ok((Out::Json(folder::snapshot_write(c, &body(req)?["snapshot"])?), "snapshot saved".into())),
        ("GET", "/v1/fs/list") => {
            c.need("file")?;
            let max = q("max").parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(1000).min(10_000);
            json(folder::list(c, &q("path"), max)?)
        }
        ("GET", "/v1/fs/read") => {
            c.need("file")?;
            json(folder::read(c, &q("path"))?)
        }
        ("GET", "/v1/fs/stat") => {
            c.need("file")?;
            json(folder::stat(c, &q("path"))?)
        }
        ("POST", "/v1/fs/write") => {
            c.need("file")?;
            let b = body(req)?;
            let r = folder::write(c, &b["path"], &b["text"])?;
            let note = format!("wrote {} ({} bytes)", r["path"].as_str().unwrap_or(""), r["bytes"]);
            Ok((Out::Json(r), note))
        }
        ("POST", "/v1/term/run") => {
            c.need("terminal")?;
            let b = body(req)?;
            let (key, command, cwd) = term::prep(c, &b)?;
            let ms = b["timeout_ms"].as_u64().filter(|n| *n > 0).unwrap_or(30 * 60 * 1000).clamp(100, term::MAX_TIMEOUT);
            c.log(&format!("run {key}: {}", command.chars().take(80).collect::<String>()));
            let t0 = Instant::now();
            let sess = term::launch(c, &cwd, &command, "run", Some(key.clone()), Some(Duration::from_millis(ms)));
            sess.wait();
            let mut v = sess.view(None);
            v["output"] = json!(sess.output(64 * 1024));
            v["ms"] = json!(t0.elapsed().as_millis() as u64);
            Ok((Out::Json(v), format!("{key} exit {}", sess.exit_word())))
        }
        ("POST", "/v1/term/start") => {
            c.need("terminal")?;
            let b = body(req)?;
            let (key, command, cwd) = term::prep(c, &b)?;
            let mode = if b["callback"] == json!(false) { "background" } else { "start" };
            let timeout = b["timeout_ms"].as_u64().filter(|n| *n > 0).map(|n| Duration::from_millis(n.clamp(100, term::MAX_TIMEOUT)));
            c.log(&format!("{mode} {key}: {}", command.chars().take(80).collect::<String>()));
            let sess = term::launch(c, &cwd, &command, mode, Some(key), timeout);
            json(sess.view(None))
        }
        ("GET", "/v1/term/tail") => {
            c.need("terminal")?;
            let key = term::key(&json!(q("key")))?;
            let lines = q("lines").parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(30).min(2000);
            json(match term::by_key(c, &key) {
                Some(sess) => sess.view(Some(lines)),
                None => json!({ "key": key, "status": "unknown", "exit": null, "tail": "" }),
            })
        }
        ("POST", "/v1/term/stop") => {
            c.need("terminal")?;
            let key = term::key(&body(req)?["key"])?;
            let sess = term::by_key(c, &key).ok_or_else(|| fail(404, "not_found", format!("no session {key}")))?;
            term::kill(c, &sess);
            Ok((Out::Json(sess.view(None)), format!("stop {key}")))
        }
        ("GET", "/v1/events") => Ok((Out::Done(events(c, req, s, cors, &me)), "stream open".into())),
        ("GET", "/v1/events/log") => {
            let since = q("since").parse::<u64>().unwrap_or(0);
            let ev = c.events.lock().unwrap();
            let list: Vec<Value> =
                ev.0.iter().filter(|e| e.id > since).map(|e| json!({ "id": e.id, "event": e.event, "data": e.data })).collect();
            json(json!({ "events": list, "last": ev.1 - 1 }))
        }
        ("GET", "/v1/store") => json(folder::store_read(c)),
        ("POST", "/v1/store") => {
            let b = body(req)?;
            folder::store_write(c, &b["put"], &b["remove"])?;
            json(json!({ "ok": true }))
        }
        ("GET", "/v1/providers") => json(providers::now(c, !q("refresh").is_empty() && q("refresh") != "0")),
        ("POST", "/v1/providers") => {
            let b = body(req)?;
            let entry = if b.get("provider").is_some() { b["provider"].clone() } else { b };
            let r = providers::add(c, &entry)?;
            let note = format!("provider {} {}", r["name"].as_str().unwrap_or(""), r["state"].as_str().unwrap_or(""));
            Ok((Out::Json(json!({ "provider": r })), note))
        }
        ("GET", "/v1/models" | "/api/v0/models") => json(providers::models(c)),
        ("POST", "/v1/chat/completions") => {
            let b = body(req)?;
            let note = b["model"].as_str().unwrap_or("").to_string();
            Ok((Out::Done(providers::chat(c, &b, s, cors)?), note))
        }
        ("GET", "/v1/switches") => {
            let sm = caps::summary(c, false);
            let list: Vec<Value> = sm["list"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| json!({ "id": x["id"], "on": x["on"], "actuator": x["actuator"] }))
                .collect();
            json(json!({ "file": c.config_file, "switches": list }))
        }
        ("GET", "/v1/capabilities") => {
            let refresh = !q("refresh").is_empty();
            json(json!({
                "name": NAME, "version": VERSION, "protocol": PROTOCOL, "impl": RUNTIME,
                "platform": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
                "folder": c.folder, "store": c.store_dir,
                "providers": providers::now(c, refresh)["providers"],
                "browser": { "chrome": caps::chrome(c), "allow": ["localhost", "127.0.0.1"] },
                "capabilities": caps::summary(c, refresh),
                "app": c.app_dir, "mcp": "/mcp", "events": c.events.lock().unwrap().1 - 1,
            }))
        }
        ("POST", "/v1/evidence/run") => {
            let r = evidence::run(c, &body(req)?)?;
            let note = format!("evidence {}", r["line"].as_str().unwrap_or(""));
            Ok((Out::Json(r), note))
        }
        ("GET", "/v1/evidence") => json(evidence::read(c, &q("project"))?),
        ("GET", "/v1/evidence/file") => {
            let bytes = evidence::file(c, &q("project"), &q("name"))?;
            let mut h = cors.to_vec();
            h.push(("content-type".into(), "image/png".into()));
            h.push(("cache-control".into(), "no-store".into()));
            http::write(s, 200, &h, &bytes);
            Ok((Out::Done(200), String::new()))
        }
        ("POST", _) if p.starts_with("/v1/browser/") => {
            let op = &p["/v1/browser/".len()..];
            let b = body(req)?;
            let r = browser::op(c, op, &b)?;
            Ok((Out::Json(r), format!("browser {op}")))
        }
        _ => Err(fail(404, "not_found", format!("no {} {p}", req.method))),
    }
}

/// `GET /v1/events`: `hello`, then what came after `?since=` (or
/// `Last-Event-ID`), then each event as it happens, and a `: ping` every heartbeat.
fn events(c: &Arc<Companion>, req: &Request, s: &TcpStream, cors: &[(String, String)], pairing: &str) -> u16 {
    let Ok(sock) = s.try_clone() else { return 500 };
    let sink: Sink = Arc::new(std::sync::Mutex::new(sock));
    let mut h = cors.to_vec();
    h.extend([("content-type".into(), "text/event-stream".into()), ("cache-control".into(), "no-store".into())]);
    let since = req.q("since").or(req.header("last-event-id")).and_then(|v| v.trim().parse::<u64>().ok());
    let n = c.next_stream.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let l = Listener { sink: sink.clone(), pairing: pairing.into(), id: req.q("id").map(String::from), all: req.q("all") == Some("1"), n };
    {
        // The log and the listeners together: nothing is missed between the replay and the stream.
        let ev = c.events.lock().unwrap();
        let running = c.sessions.lock().unwrap().iter().filter(|x| x.running()).count();
        let mut text = String::from_utf8_lossy(&http::head(200, &h, None)).into_owned();
        text.push_str(&format!("event: hello\ndata: {}\n\n", json!({ "sessions": running, "last": ev.1 - 1 })));
        if let Some(since) = since {
            for e in ev.0.iter().filter(|e| e.id > since && wants(&l, e)) {
                text.push_str(&sse(e));
            }
        }
        if !http::send(&sink, &text) {
            return 200;
        }
        c.listeners.lock().unwrap().push(l);
    }
    http::wait_closed(s);
    c.listeners.lock().unwrap().retain(|x| x.n != n);
    200
}

/// The kill switch: every running command, every evidence run (counted:
/// it ends with its own timeouts), the browser.
pub fn stop_all(c: &Arc<Companion>) -> Value {
    let n = term::stop_all(c);
    let b = browser::close(c);
    c.log(&format!("stop all: {n} session{} stopped{}", if n == 1 { "" } else { "s" }, if b { ", browser closed" } else { "" }));
    json!({ "stopped": n, "browser": b })
}
