//! dry-server: the web side, and the host. Every route of the app as HTML,
//! drawn on the server from the same page files and core view models as
//! the GPUI window (`dry_html`: the renderer the browser build draws with
//! too), in the same look (`dry_html::css`), kept live over server-sent
//! events (`live`, `/events`). It is the host of the store (`host`): the
//! one writer, running the engines' turns; pages and the window send it
//! intents (docs/web.md).
//!
//! ```text
//! GET  /api/engines/{id}/evidence/{artifact}/{file}  an evidence artifact's shot (PNG)
//! GET  /…                           any route: the page (404 when none matches)
//! GET  /events                      server-sent events: hello, change, activity, message, turn (ids; Last-Event-ID resumes)
//! GET  /assets/app.css, app.js      the look and the live script
//! GET  /assets/fonts/<file>         a font shipped in assets/fonts/
//! GET  /api/status                  the host: generation, engines, who is in a turn
//! POST /api/engines                 {kind?} a new thread (a root from the default's translator), or kind: a sessions.json program, a new session → 201 {index, id, gen}
//! POST /api/engines/:id/messages    {text, client, base}               → 202 {turn, gen, index} | 409 {error, gen}
//! POST /api/engines/:id/answers     {message, answer | pick, base}     → 202 {turn, gen, index} | 409 {error, gen}
//! POST /api/engines/:id/delete      {base}                             → 200 {gen} | 409
//! POST /api/engines/:id/format      {value, base}                      → 200 {gen}
//! POST /api/mode                    {value}                            → 200 {gen}
//! POST /api/scene                   {value} (a scene's name, or none)  → 200 {gen}
//! POST /api/reset                   {confirm: true}                    → 200 {gen, backup, moved} | 400 no confirm | 409 a turn runs
//!                                   Reset history: the store's own files move to a backup beside it; the default alone after
//! GET  /api/engines/:id/read        ?what=start|turn|artifact|thread&name=&which=&since= → 200 {text, cursor?} (`dashboard`: thread's old name)
//! POST /api/engines/:id/posts       {text, to?}                        → 200 {line, gen}
//! POST /api/threads/:id/posts       {text, to?} your guided post on root `id`'s thread (`top`: the first; no `to`: kept, for nobody) → 200 {line, to, busy, gen}
//! POST /api/boards/:level/posts     the same, its old name (pages opened before threads)
//! POST /api/engines/:id/children    {goal, name?, template?, text?} | {child, text} → 200 {line, child, replies}
//! POST /api/engines/:id/children/:child/take {after}                   → 200 {reply} | {busy} | {failed}
//! GET  /api/approvals               pending first, then the newest decided → 200 {approvals}
//! POST /api/approvals               {engine, tool, input}              → 201 the approval (decided at once by a rule)
//! GET  /api/approvals/:id                                              → 200 the approval | 404
//! POST /api/approvals/:id           {decision: allow|deny, reason?}    → 200 the approval | 409 decided
//! POST /api/models/:name/detect     (models.json's setting)            → 200 {ok, line, models}
//! POST /api/models/:name/test                                          → 200 {ok, line}
//! ```
//!
//! `cargo run -p dry-server` listens on 127.0.0.1:8090 (8080 is kept for
//! `dx serve`); `--port 8091` or `DRY_SERVER_PORT=8091` picks another.
//! `--bind 0.0.0.0` (or `DRY_SERVER_BIND`) listens wider, and then needs
//! `--token <t>` (or `DRY_SERVER_TOKEN`): every request from another
//! machine must carry it; `--strict` (`DRY_SERVER_STRICT=1`) asks it of this
//! machine too (behind a local proxy). `--read-only` serves without hosting. The repo
//! root is `DRY_ROOT`, else this checkout (`dry_tools::root`).

mod capabilities;
mod files;
mod host;
mod live;
mod load;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use actix_web::http::StatusCode;
use actix_web::{App, HttpRequest, HttpResponse, HttpServer, web};
use serde_json::{Value, json};

use crate::host::{Host, Refused};
use crate::live::Live;

/// Where the server listens unless told otherwise.
const HOST: &str = "127.0.0.1";
const PORT: u16 = 8090;

/// What the command line and the environment chose.
#[derive(Clone, Debug, Default, PartialEq)]
struct Config {
    port: u16,
    bind: String,
    token: Option<String>,
    read_only: bool,
    /// `--strict`: the token from every client, this machine's too (behind
    /// a proxy on this machine, every request looks local).
    strict: bool,
}

/// `--port N`, `--bind ADDR`, `--token T`, `--read-only`; else
/// `DRY_SERVER_PORT`, `DRY_SERVER_BIND`, `DRY_SERVER_TOKEN`; else 8090 on
/// 127.0.0.1 with no token.
fn config(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Config {
    let mut c = Config {
        port: env("DRY_SERVER_PORT").and_then(|p| p.parse().ok()).unwrap_or(PORT),
        bind: env("DRY_SERVER_BIND").unwrap_or_else(|| HOST.into()),
        token: env("DRY_SERVER_TOKEN").filter(|t| !t.is_empty()),
        read_only: false,
        strict: env("DRY_SERVER_STRICT").is_some_and(|v| v == "1"),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next().cloned());
        match flag {
            "--port" => match value().map(|v| (v.parse::<u16>(), v)) {
                Some((Ok(p), _)) => c.port = p,
                Some((Err(_), v)) => eprintln!("--port {v} is not a port; using {}", c.port),
                None => {}
            },
            "--bind" => {
                if let Some(v) = value() {
                    c.bind = v;
                }
            }
            "--token" => c.token = value().filter(|t| !t.is_empty()),
            "--read-only" => c.read_only = true,
            "--strict" => c.strict = true,
            _ => {}
        }
    }
    c
}

/// Is `bind` this machine only?
fn loopback(bind: &str) -> bool {
    bind == "localhost" || bind.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

struct Shared {
    live: Arc<Live>,
    host: Option<Arc<Host>>,
    /// Why this server is not the host, if it is not.
    not_host: Option<String>,
    token: Option<String>,
    strict: bool,
}

type Data = web::Data<Shared>;

// ---- who may ----

/// The token a request carries: `Authorization: Bearer`, the `dry_token`
/// cookie (set when a page was opened with `?token=`), or `?token=`.
fn carried(req: &HttpRequest) -> Option<String> {
    if let Some(t) = req.headers().get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")) {
        return Some(t.trim().to_string());
    }
    if let Some(c) = req.cookie("dry_token") {
        return Some(c.value().to_string());
    }
    query_token(req.query_string())
}

fn query_token(q: &str) -> Option<String> {
    q.split('&').find_map(|kv| kv.strip_prefix("token=")).map(dry_html::render::decode_path)
}

/// Compare without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Requests from this machine are let in; from another, only with the
/// token (a server bound wider always has one, `main` makes sure).
fn let_in(req: &HttpRequest, s: &Shared) -> Result<(), No> {
    let local = !s.strict && req.peer_addr().is_some_and(|a| a.ip().to_canonical().is_loopback());
    match &s.token {
        _ if local => Ok(()),
        Some(t) if carried(req).is_some_and(|c| same(&c, t)) => Ok(()),
        _ => Err(No(401, "this server needs its token from another machine: open the page once with ?token=<token>, or send Authorization: Bearer <token>".into())),
    }
}

/// A write also must be JSON, and from a page of this server when it says
/// where it is from: so another site open in the same browser cannot post
/// to it (a cross-site JSON post needs a preflight, which this server never answers).
fn let_write(req: &HttpRequest, s: &Shared) -> Result<(), No> {
    let_in(req, s)?;
    let json = req.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    if !json {
        return Err(No(415, "send JSON (Content-Type: application/json)".into()));
    }
    if let Some(origin) = req.headers().get("origin").and_then(|v| v.to_str().ok()) {
        let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or_default();
        let ok = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")).is_some_and(|o| o == host);
        if !ok {
            return Err(No(403, format!("a write from {origin} is not from this server's pages")));
        }
    }
    Ok(())
}

/// A request refused before it reached the host: the status and why.
struct No(u16, String);

impl No {
    fn response(self) -> HttpResponse {
        error(self.0, self.1)
    }
}

fn error(status: u16, why: impl Into<String>) -> HttpResponse {
    HttpResponse::build(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST))
        .content_type("application/json")
        .insert_header(("Cache-Control", "no-store"))
        .body(json!({ "error": why.into() }).to_string())
}

fn reply(status: u16, body: Value) -> HttpResponse {
    HttpResponse::build(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
        .content_type("application/json")
        .insert_header(("Cache-Control", "no-store"))
        .body(body.to_string())
}

fn refused(r: Refused, live: &Live) -> HttpResponse {
    reply(r.status, json!({ "error": r.error, "gen": live.generation() }))
}

// ---- pages ----

/// Any route: the page for the path, drawn from the files as they are now.
async fn page(req: HttpRequest, s: Data) -> HttpResponse {
    if let Err(no) = let_in(&req, &s) {
        return no.response();
    }
    // Opened with the token: keep it in a cookie and show the plain address.
    if let (Some(t), Some(want)) = (query_token(req.query_string()), &s.token)
        && same(&t, want)
    {
        let rest: Vec<&str> = req.query_string().split('&').filter(|kv| !kv.starts_with("token=") && !kv.is_empty()).collect();
        let to = if rest.is_empty() { req.path().to_string() } else { format!("{}?{}", req.path(), rest.join("&")) };
        return HttpResponse::SeeOther()
            .insert_header(("Location", to))
            .insert_header(("Set-Cookie", format!("dry_token={t}; Path=/; HttpOnly; SameSite=Strict")))
            .finish();
    }
    let location = dry_html::render::decode_path(req.path());
    let query = dry_html::Query::parse(req.query_string());
    let generation = s.live.generation();
    let host = s.host.clone();
    let raw = req.path().to_string();
    let drawn = web::block(move || {
        let world = load::world(host.as_deref());
        // A moved route: send the address on to where it opens now (the
        // rest of the path as it came, still encoded).
        match world.table.moved_to(&raw) {
            Some(to) => Err(to),
            None => Ok(dry_html::page::document(&world, &location, &query, generation)),
        }
    })
    .await;
    match drawn {
        Ok(Err(to)) => {
            let q = req.query_string();
            let to = if q.is_empty() { to } else { format!("{to}?{q}") };
            HttpResponse::Found().insert_header(("Location", to)).finish()
        }
        Ok(Ok((status, html))) => HttpResponse::build(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
            .content_type("text/html; charset=utf-8")
            .insert_header(("Cache-Control", "no-store"))
            .body(html),
        Err(e) => HttpResponse::InternalServerError().body(format!("drawing the page failed: {e}")),
    }
}

async fn stylesheet() -> HttpResponse {
    let css = web::block(|| dry_html::css::stylesheet(&load::shipped_fonts(), "/assets/fonts/")).await.unwrap_or_default();
    HttpResponse::Ok().content_type("text/css; charset=utf-8").insert_header(("Cache-Control", "no-cache")).body(css)
}

async fn script() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/javascript; charset=utf-8")
        .insert_header(("Cache-Control", "no-cache"))
        .body(include_str!("../assets/app.js"))
}

/// A font from `assets/fonts/`: only a file that is there by that name.
async fn font(name: web::Path<String>) -> HttpResponse {
    let name = name.into_inner();
    if !load::shipped_fonts().contains(&name) {
        return HttpResponse::NotFound().finish();
    }
    match std::fs::read(dry_tools::root().join("assets/fonts").join(&name)) {
        Ok(bytes) => HttpResponse::Ok()
            .content_type(if name.ends_with(".otf") { "font/otf" } else { "font/ttf" })
            .insert_header(("Cache-Control", "max-age=3600"))
            .body(bytes),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

/// An evidence artifact's image (a shot, its after or its diff), for the
/// artifact page (`dry_html::render::engine::evidence_url`).
async fn evidence_image(req: HttpRequest, s: Data, p: web::Path<(String, String, String)>) -> HttpResponse {
    if let Err(no) = let_in(&req, &s) {
        return no.response();
    }
    let (id, artifact, file) = p.into_inner();
    let found = web::block(move || {
        let (state, _, _) = load::read_state();
        let e = state.engines.iter().find(|e| e.id == id).ok_or("no such engine")?;
        let text = e.artifacts.get(&artifact).ok_or("no such artifact")?;
        if dry_core::live::Live::of(text) != Some(dry_core::live::Live::Evidence) {
            return Err("not an evidence artifact".to_string());
        }
        let st = dry_core::evidence::artifact::State::parse(text)?;
        dry_tools::evidence::image(&dry_tools::root(), &st.root, &file)
    })
    .await;
    match found {
        Ok(Ok(bytes)) => HttpResponse::Ok().content_type("image/png").insert_header(("Cache-Control", "no-cache")).body(bytes),
        Ok(Err(why)) => error(404, why),
        Err(e) => error(500, e.to_string()),
    }
}

/// `/events`: server-sent events (see `live`). A client resumes after the
/// id it last saw: `Last-Event-ID` (the browser sends it on reconnect) or `?since=`.
async fn events(req: HttpRequest, s: Data) -> HttpResponse {
    if let Err(no) = let_in(&req, &s) {
        return no.response();
    }
    let resume = req
        .headers()
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .or_else(|| req.query_string().split('&').find_map(|kv| kv.strip_prefix("since=")).and_then(|v| v.parse().ok()));
    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming(live::stream(s.live.clone(), resume))
}

// ---- the API ----

async fn status(req: HttpRequest, s: Data) -> HttpResponse {
    if let Err(no) = let_in(&req, &s) {
        return no.response();
    }
    match &s.host {
        Some(h) => reply(200, h.status_json()),
        None => reply(200, json!({ "host": false, "gen": s.live.generation(), "why": s.not_host })),
    }
}

/// The host and the JSON body, or the response that says why not.
fn intent(req: &HttpRequest, s: &Shared, body: &[u8]) -> Result<(Arc<Host>, Value), No> {
    let_write(req, s)?;
    let host = s.host.clone().ok_or_else(|| {
        No(503, format!("this server is not the host ({}); {}", s.not_host.as_deref().unwrap_or("read-only"), dry_html::render::READ_ONLY))
    })?;
    let v: Value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(body).map_err(|e| No(400, format!("the body is not JSON: {e}")))?
    };
    Ok((host, v))
}

fn base(v: &Value) -> Option<u64> {
    v.get("base").and_then(Value::as_u64)
}

fn taken(status: u16, t: host::Taken) -> HttpResponse {
    reply(status, json!({ "turn": t.turn, "gen": t.generation, "index": t.index, "id": t.id }))
}

async fn send(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let text = v.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
    let target = id.into_inner();
    let b = base(&v);
    match web::block(move || host.send(&target, &text, b)).await {
        Ok(Ok(t)) => taken(202, t),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn answer(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let Some(message) = v.get("message").and_then(Value::as_u64) else { return error(400, "which reply: {\"message\": <its index>}") };
    let target = id.into_inner();
    let b = base(&v);
    let done = web::block(move || {
        let text = v.get("answer").and_then(Value::as_str).map(str::to_string);
        host.answer(&target, message as usize, text.as_deref(), v.get("pick").unwrap_or(&Value::Null), b)
    })
    .await;
    match done {
        Ok(Ok(t)) => taken(202, t),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn create(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let kind = v.get("kind").and_then(Value::as_str).map(str::to_string);
    match web::block(move || host.create(kind.as_deref())).await {
        Ok(Ok(t)) => taken(201, t),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn delete(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let target = id.into_inner();
    match web::block(move || host.delete(&target, base(&v))).await {
        Ok(Ok(g)) => reply(200, json!({ "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn format(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let target = id.into_inner();
    let value = v.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
    match web::block(move || host.set_format(&target, &value, base(&v))).await {
        Ok(Ok(g)) => reply(200, json!({ "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn scene(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let value = v.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
    match web::block(move || host.set_scene(&value)).await {
        Ok(Ok(g)) => reply(200, json!({ "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn theme(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let value = v.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
    match web::block(move || host.set_theme(&value)).await {
        Ok(Ok(g)) => reply(200, json!({ "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn mode(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let value = v.get("value").and_then(Value::as_str).unwrap_or_default().to_string();
    match web::block(move || host.set_mode(&value)).await {
        Ok(Ok(g)) => reply(200, json!({ "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

/// Reset history (`Host::reset`): only with `{"confirm": true}` (the
/// page's second press), refused while an engine is in a turn. Every
/// page reloads on the generation it bumps.
async fn reset(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    if v.get("confirm").and_then(Value::as_bool) != Some(true) {
        return error(400, "Reset history moves every engine, message and log out of the store: send {\"confirm\": true} to do it");
    }
    match web::block(move || host.reset()).await {
        Ok(Ok((plan, g))) => {
            let backup = (!plan.is_empty()).then(|| plan.backup.display().to_string());
            let moved: Vec<String> = plan.moves.iter().map(|(from, _)| from.display().to_string()).collect();
            reply(200, json!({ "gen": g, "backup": backup, "moved": moved }))
        }
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

/// Detect or Test on a model setting from `models.json`. Nothing is
/// written: the result is the answer (the window shows it on the card).
async fn model(req: HttpRequest, s: Data, path: web::Path<(String, String)>, body: web::Bytes) -> HttpResponse {
    if let Err(no) = intent(&req, &s, &body) {
        return no.response();
    }
    let (name, what) = path.into_inner();
    let done = web::block(move || -> Result<Value, String> {
        let models = dry_tools::settings::read_models()?;
        let m = models.get(&name).ok_or_else(|| format!("no model setting {name} in models.json"))?;
        let ai = models.inference(m)?;
        match what.as_str() {
            "detect" => {
                let url = format!("{}/models", ai.base_url.trim_end_matches('/'));
                let ids = dry_tools::net::get(&url, &ai.key_env)
                    .and_then(|b| dry_core::inference::parse_model_ids(&b).map_err(|e| format!("{e:?}")))?;
                Ok(json!({ "ok": true, "line": format!("{} models found at {url}", ids.len()), "models": ids }))
            }
            "test" => {
                if ai.model.is_empty() {
                    return Err("choose a model first (Detect)".into());
                }
                use dry_core::inference::Inference as _;
                let req = ai.request(&[dry_core::inference::Message {
                    role: dry_core::inference::Role::User,
                    text: "Reply with one short sentence to say hello.".into(),
                }]);
                let reply = dry_tools::net::post(&req).and_then(|b| ai.reply(&b).map_err(|e| format!("bad reply: {e:?}")))?;
                Ok(json!({ "ok": true, "line": format!("{}: {}", ai.model, reply.trim().chars().take(160).collect::<String>()) }))
            }
            other => Err(format!("no model action {other}")),
        }
    })
    .await;
    match done {
        Ok(Ok(v)) => reply(200, v),
        Ok(Err(why)) => reply(200, json!({ "ok": false, "line": why })),
        Err(e) => error(500, e.to_string()),
    }
}

// ---- workers (`dry_tools::worker`) ----

/// The host for a read, or why not.
fn reader(req: &HttpRequest, s: &Shared) -> Result<Arc<Host>, No> {
    let_in(req, s)?;
    s.host.clone().ok_or_else(|| No(503, format!("this server is not the host ({})", s.not_host.as_deref().unwrap_or("read-only"))))
}

fn query(q: &str, key: &str) -> String {
    q.split('&').find_map(|kv| kv.strip_prefix(&format!("{key}="))).map(dry_html::render::decode_path).unwrap_or_default()
}

async fn read(req: HttpRequest, s: Data, id: web::Path<String>) -> HttpResponse {
    let host = match reader(&req, &s) {
        Ok(h) => h,
        Err(no) => return no.response(),
    };
    let q = req.query_string().to_string();
    let what = match dry_core::snapshot::Read::parse(&query(&q, "what"), &query(&q, "name"), &query(&q, "which"), &query(&q, "since")) {
        Ok(w) => w,
        Err(why) => return error(400, why),
    };
    let target = id.into_inner();
    match web::block(move || host.read(&target, &what)).await {
        Ok(Ok((text, cursor))) => reply(200, json!({ "text": text, "cursor": cursor.map(|c| c.text()) })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn posts(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let target = id.into_inner();
    let done = web::block(move || {
        let text = v.get("text").and_then(Value::as_str).unwrap_or_default();
        let done = host.post(&target, text, v.get("to").and_then(Value::as_str));
        // An addressed post calls its engine (`dry_core::desk::guided`).
        host.settle();
        done
    })
    .await;
    match done {
        Ok(Ok((line, g))) => reply(200, json!({ "line": line, "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn thread_post(req: HttpRequest, s: Data, level: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let level = match level.into_inner() {
        l if l == "top" => String::new(),
        l => l,
    };
    let done = web::block(move || {
        let text = v.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
        let to = v.get("to").and_then(Value::as_str).map(str::to_string);
        host.post_yours(&level, to.as_deref(), &text)
    })
    .await;
    match done {
        Ok(Ok((line, to, busy, g))) => reply(200, json!({ "line": line, "to": to, "busy": busy, "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn children(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let target = id.into_inner();
    match web::block(move || host.children(&target, &v)).await {
        Ok(Ok((line, child, replies))) => reply(200, json!({ "line": line, "child": child, "replies": replies })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn take(req: HttpRequest, s: Data, path: web::Path<(String, String)>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let (target, child) = path.into_inner();
    let after = v.get("after").and_then(Value::as_u64).unwrap_or(0) as usize;
    match web::block(move || host.take(&target, &child, after)).await {
        Ok(Ok(v)) => reply(200, v),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn approvals(req: HttpRequest, s: Data) -> HttpResponse {
    match reader(&req, &s) {
        Ok(h) => reply(200, json!({ "approvals": h.approvals().iter().map(|a| a.to_json()).collect::<Vec<_>>() })),
        Err(no) => no.response(),
    }
}

async fn ask_approval(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let word = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let (engine, tool) = (word("engine"), word("tool"));
    if engine.is_empty() || tool.is_empty() {
        return error(400, "an approval needs {\"engine\", \"tool\", \"input\"}");
    }
    let input = v.get("input").cloned().unwrap_or(json!({}));
    match web::block(move || host.ask_approval(&engine, &tool, input)).await {
        Ok(Ok(a)) => reply(201, a.to_json()),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn approval(req: HttpRequest, s: Data, id: web::Path<String>) -> HttpResponse {
    match reader(&req, &s) {
        Ok(h) => match h.approval(&id) {
            Some(a) => reply(200, a.to_json()),
            None => error(404, format!("no approval {id} (the host keeps them only while it runs)")),
        },
        Err(no) => no.response(),
    }
}

async fn decide(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let Some(d) = v.get("decision").and_then(Value::as_str).and_then(dry_core::approval::Decision::parse) else {
        return error(400, "decision: allow or deny");
    };
    let reason = v.get("reason").and_then(Value::as_str).unwrap_or_default().to_string();
    match host.decide(&id, d, &reason) {
        Ok(a) => reply(200, a.to_json()),
        Err(r) => refused(r, &s.live),
    }
}

// ---- capabilities (docs/capabilities.md) ----

/// Every capability by provider and state, and the peers.
async fn capabilities_list(req: HttpRequest, s: Data) -> HttpResponse {
    if let Err(no) = let_in(&req, &s) {
        return no.response();
    }
    match &s.host {
        Some(h) => reply(200, h.caps.to_json()),
        None => error(503, "this server is not the host: it has no capabilities of its own to list"),
    }
}

/// Join a peer: `{url, code, name?}`, its pairing code (a companion prints one).
async fn join_peer(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let field = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let (url, code, name) = (field("url"), field("code"), field("name"));
    if url.is_empty() || code.is_empty() {
        return error(400, "joining a peer takes {url, code}: its address and the pairing code it printed");
    }
    let caps = host.caps.clone();
    let h = host.clone();
    let changed: Arc<dyn Fn() + Send + Sync> = Arc::new(move || h.capabilities_changed());
    match web::block(move || caps.join(&name, &url, &code, changed)).await {
        Ok(Ok(v)) => reply(200, v),
        Ok(Err(why)) => error(502, why),
        Err(e) => error(500, e.to_string()),
    }
}

/// A peer leaves now (its capabilities become absent).
async fn leave_peer(req: HttpRequest, s: Data, name: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, _) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    if host.caps.leave(&name, "asked to leave") {
        host.capabilities_changed();
        reply(200, json!({ "ok": true }))
    } else {
        error(404, format!("no peer {name}"))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let c = config(&args, &|k| std::env::var(k).ok());
    if (c.strict || !loopback(&c.bind)) && c.token.is_none() {
        eprintln!("dry-server: --bind {} (or --strict) needs a --token (or DRY_SERVER_TOKEN) first: it lets other clients in", c.bind);
        std::process::exit(2);
    }
    let live = Arc::new(Live::default());
    let url = format!("http://127.0.0.1:{}", c.port);
    let (host, not_host) = if c.read_only {
        (None, Some("--read-only".to_string()))
    } else {
        match Host::open(live.clone(), &url) {
            Ok(h) => (Some(h), None),
            Err(why) => {
                eprintln!("dry-server: not hosting: {why}");
                (None, Some(why))
            }
        }
    };
    let outside = host.clone();
    live::watch(live.clone(), Duration::from_millis(500), move || {
        if let Some(h) = &outside {
            h.outside_change();
        }
    });
    if let Some(h) = &host {
        h.tick();
        h.watch_subscriptions();
    }
    println!(
        "dry-server on http://{}:{} (root {}){}",
        c.bind,
        c.port,
        dry_tools::root().display(),
        match &host {
            Some(h) => format!(" · host of {}", h.spec),
            None => " · read-only".to_string(),
        }
    );
    let data = web::Data::new(Shared { live, host: host.clone(), not_host, token: c.token.clone(), strict: c.strict });
    let served = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            // Before `/api/models/{name}/{what}`: the app's files and the scene (files.rs).
            .configure(files::routes)
            .route("/events", web::get().to(events))
            .route("/assets/app.css", web::get().to(stylesheet))
            .route("/assets/app.js", web::get().to(script))
            .route("/assets/fonts/{name}", web::get().to(font))
            .route("/api/status", web::get().to(status))
            .route("/api/engines", web::post().to(create))
            .route("/api/engines/{id}/messages", web::post().to(send))
            .route("/api/engines/{id}/answers", web::post().to(answer))
            .route("/api/engines/{id}/delete", web::post().to(delete))
            .route("/api/engines/{id}/format", web::post().to(format))
            .route("/api/mode", web::post().to(mode))
            .route("/api/scene", web::post().to(scene))
            .route("/api/theme", web::post().to(theme))
            .route("/api/reset", web::post().to(reset))
            .route("/api/engines/{id}/read", web::get().to(read))
            .route("/api/engines/{id}/evidence/{artifact}/{file}", web::get().to(evidence_image))
            .route("/api/engines/{id}/posts", web::post().to(posts))
            .route("/api/threads/{level}/posts", web::post().to(thread_post))
            // The old name, for pages opened before threads.
            .route("/api/boards/{level}/posts", web::post().to(thread_post))
            .route("/api/engines/{id}/children", web::post().to(children))
            .route("/api/engines/{id}/children/{child}/take", web::post().to(take))
            .route("/api/approvals", web::get().to(approvals))
            .route("/api/approvals", web::post().to(ask_approval))
            .route("/api/approvals/{id}", web::get().to(approval))
            .route("/api/approvals/{id}", web::post().to(decide))
            .route("/api/models/{name}/{what}", web::post().to(model))
            .route("/api/capabilities", web::get().to(capabilities_list))
            .route("/api/capabilities/peers", web::post().to(join_peer))
            .route("/api/capabilities/peers/{name}/leave", web::post().to(leave_peer))
            .default_service(web::get().to(page))
    })
    // `/events` streams never end, so a graceful stop would always wait
    // out the timeout: a service manager's stop takes 3 s, not 30.
    .shutdown_timeout(3)
    .bind((c.bind.as_str(), c.port))?
    .run()
    .await;
    if let Some(h) = &host {
        h.release();
    }
    served
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_then_the_environment_then_the_defaults() {
        let none = |_: &str| None;
        assert_eq!(config(&[], &none), Config { port: 8090, bind: "127.0.0.1".into(), token: None, read_only: false, strict: false });
        let env = |k: &str| match k {
            "DRY_SERVER_PORT" => Some("8095".to_string()),
            "DRY_SERVER_TOKEN" => Some("t1".to_string()),
            _ => None,
        };
        assert_eq!(config(&[], &env).port, 8095);
        assert_eq!(config(&[], &env).token.as_deref(), Some("t1"));
        let c = config(&args(&["--port=9001", "--bind", "0.0.0.0", "--token", "s3", "--read-only"]), &env);
        assert_eq!(c, Config { port: 9001, bind: "0.0.0.0".into(), token: Some("s3".into()), read_only: true, strict: false });
        assert_eq!(config(&args(&["--port", "x"]), &none).port, 8090);
        assert!(config(&args(&["--strict"]), &none).strict && !config(&[], &none).strict);
    }

    #[test]
    fn only_loopback_binds_go_without_a_token() {
        assert!(loopback("127.0.0.1") && loopback("::1") && loopback("localhost"));
        assert!(!loopback("0.0.0.0") && !loopback("192.168.1.4"));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
        assert_eq!(query_token("a=1&token=x%20y"), Some("x y".into()));
    }
}
