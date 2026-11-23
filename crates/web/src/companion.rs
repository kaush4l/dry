//! The companion: a small server on a computer (`companion/dry-companion.mjs`,
//! docs/companion.md) that gives this page what a browser cannot reach:
//! the computer's projects, its runtimes, local inference and git.
//!
//! - **Settings → Companion**: its URL, the one-time pairing code it
//!   printed, Connect / Disconnect, what it has. The session token it
//!   gives back is kept in this page's memory only (never in the store,
//!   never in a URL): a reload pairs again.
//! - **The host**: while connected, `shell()` is the companion, so `file`
//!   and `terminal` artifacts work (the plain browser refuses them).
//!   `Host`'s calls are synchronous, so the quick ones (list, read, write,
//!   start, status, tail) are synchronous requests to 127.0.0.1; a
//!   terminal `run`, which waits, goes through `io` as an ordinary async
//!   request, so the page keeps drawing while it runs.
//! - **Models**: the companion serves an OpenAI-compatible
//!   `/v1/chat/completions` (LM Studio, Ollama, `claude -p`, `copilot -p`),
//!   so "Use its models" points Settings → Models at it and `net::Fetch`
//!   sends the token as the key to that URL only.
//! - **Projects** (`#/projects`): list, open, the tree, a file, run /
//!   start / background a command (a start's exit comes back as an event),
//!   git status, diff, commit, push and pull (push and clone only from a
//!   click, after a confirm), and attaching a project to an engine as its
//!   `files` and `terminal` artifacts.

use std::cell::RefCell;
use std::collections::BTreeMap;

use dry_core::calls::Call;
use dry_core::companion::{self as proto, Capabilities, Entry, GitStatus, Kept, Pairing, Project};
use dry_core::store::{Changes, TextStore, Texts};
use dry_core::live::{Clock, Content, Host, Job, Proc, Time};
use dry_core::runner::{self, Came, Next, Transport};
use dry_core::workspace::{Folder, Run, Stat, Tail, Workspace as Workspace_};
use dry_core::turn::{self, Wait};
use js_sys::{Array, Function, Reflect};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Element, HtmlElement, HtmlInputElement, RequestInit, RequestMode, Response};

use crate::host::{self, Browser};

/// The route of the projects view.
pub const PATH: &str = "/projects";
const URL_KEY: &str = "dry.companion.url";
/// This tab's pairing (sessionStorage: it survives a reload, not the tab).
const PAIRING_KEY: &str = "dry.companion.pairing";
/// The last event id this tab had (sessionStorage), to resume from.
const EVENT_KEY: &str = "dry.companion.event";
/// Which companion keeps the state, if one does (localStorage).
const STORE_KEY: &str = "dry.companion.store";
/// The offline copy has changes the companion has not had (localStorage).
const PENDING_KEY: &str = "dry.companion.pending";

#[derive(Default)]
struct Conn {
    url: String,
    /// The session token: memory only.
    token: Option<String>,
    caps: Option<Capabilities>,
    /// One line: what the connection is doing, or why it failed.
    line: String,
    projects: Vec<Project>,
    /// The open project's path.
    open: Option<String>,
    /// The folder the tree shows.
    dir: Option<String>,
    entries: Vec<Entry>,
    /// The file shown: its path and text (or why not).
    file: Option<(String, String)>,
    /// What the last command or git operation said.
    output: String,
    sessions: Vec<Value>,
    git: Option<Result<GitStatus, String>>,
    /// What the person typed in this view's fields, kept across redraws.
    fields: BTreeMap<String, String>,
    listening: bool,
    /// Its custom tools, as engines get them (`companion.<name>`).
    tools: Vec<dry_core::mcp::Tool>,
    tool_errors: Vec<String>,
    pairings: Vec<Pairing>,
    /// When this tab's pairing ends (unix seconds).
    expires: i64,
    /// This page is served by a companion (same origin).
    served: bool,
    /// Its workspace folder (`/v1/workspace`, docs/companion-protocol.md, "The workspace folder").
    workspace: Option<Workspace>,
    /// Its declared providers (`/v1/providers`), as it answered.
    providers: Option<Value>,
}

/// A companion's workspace folder.
#[derive(Clone, Debug, Default)]
struct Workspace {
    folder: String,
    name: String,
    file: bool,
    terminal: bool,
}

thread_local! {
    static C: RefCell<Conn> = RefCell::new(Conn::default());
}

fn with<R>(f: impl FnOnce(&mut Conn) -> R) -> R {
    C.with(|c| f(&mut c.borrow_mut()))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn enc(s: &str) -> String {
    String::from(js_sys::encode_uri_component(s))
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

fn doc() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn here() -> String {
    let h = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
    h.trim_start_matches('#').split('?').next().unwrap_or("").to_string()
}

/// Whether a token is held.
pub fn connected() -> bool {
    with(|c| c.token.is_some())
}

/// The token, for a request to this companion's URL only (`net::Fetch`).
pub fn token_for(url: &str) -> Option<String> {
    with(|c| c.token.clone().filter(|_| !c.url.is_empty() && (url == c.url || url.starts_with(&format!("{}/", c.url)))))
}

// ---- requests ----

/// A synchronous request (for `Host`'s synchronous calls), through the
/// page's `XMLHttpRequest`; the body that came back, any status.
fn sync(method: &str, url: &str, token: &str, body: Option<&str>) -> Result<String, String> {
    let call = |o: &JsValue, name: &str, args: &[JsValue]| -> Result<JsValue, JsValue> {
        let f: Function = Reflect::get(o, &name.into())?.dyn_into()?;
        f.apply(o, &args.iter().collect::<Array>())
    };
    let run = || -> Result<String, JsValue> {
        let ctor: Function = Reflect::get(&js_sys::global(), &"XMLHttpRequest".into())?.dyn_into()?;
        let x = Reflect::construct(&ctor, &Array::new())?;
        call(&x, "open", &[method.into(), url.into(), false.into()])?;
        call(&x, "setRequestHeader", &[proto::TOKEN_HEADER.into(), token.into()])?;
        if body.is_some() {
            call(&x, "setRequestHeader", &["content-type".into(), "application/json".into()])?;
        }
        call(&x, "send", &[body.map_or(JsValue::NULL, JsValue::from_str)])?;
        let status = Reflect::get(&x, &"status".into())?.as_f64().unwrap_or(0.0);
        if status == 0.0 {
            return Err("could not reach the companion".into());
        }
        Ok(Reflect::get(&x, &"responseText".into())?.as_string().unwrap_or_default())
    };
    run().map_err(js_err)
}

/// An async request with the token (when held); the body, any status.
async fn fetch(method: &str, url: &str, token: Option<&str>, body: Option<&str>) -> Result<String, String> {
    let resp = send(method, url, token, body).await?;
    JsFuture::from(resp.text().map_err(js_err)?).await.map_err(js_err).map(|t| t.as_string().unwrap_or_default())
}

async fn send(method: &str, url: &str, token: Option<&str>, body: Option<&str>) -> Result<Response, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    let headers = web_sys::Headers::new().map_err(js_err)?;
    if let Some(t) = token {
        headers.set(proto::TOKEN_HEADER, t).map_err(js_err)?;
    }
    if let Some(b) = body {
        headers.set("content-type", "application/json").map_err(js_err)?;
        init.set_body(&JsValue::from_str(b));
    }
    init.set_headers(&headers);
    let req = web_sys::Request::new_with_str_and_init(url, &init).map_err(js_err)?;
    let w = web_sys::window().ok_or("no window")?;
    let resp = JsFuture::from(w.fetch_with_request(&req))
        .await
        .map_err(|e| format!("could not reach the companion at {url} ({}): is it running? (node companion/dry-companion.mjs)", js_err(e)))?;
    resp.dyn_into::<Response>().map_err(js_err)
}

/// A request with the token and an `Accept`; the body, any status (the
/// capabilities' MCP calls, `crate::caps`).
pub(crate) async fn request(method: &str, url: &str, token: &str, body: Option<&str>, accept: &str) -> Result<String, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    let headers = web_sys::Headers::new().map_err(js_err)?;
    headers.set("authorization", &format!("Bearer {token}")).map_err(js_err)?;
    headers.set("accept", accept).map_err(js_err)?;
    if let Some(b) = body {
        headers.set("content-type", "application/json").map_err(js_err)?;
        init.set_body(&JsValue::from_str(b));
    }
    init.set_headers(&headers);
    let req = web_sys::Request::new_with_str_and_init(url, &init).map_err(js_err)?;
    let w = web_sys::window().ok_or("no window")?;
    let resp: Response = JsFuture::from(w.fetch_with_request(&req)).await.map_err(js_err)?.dyn_into().map_err(js_err)?;
    JsFuture::from(resp.text().map_err(js_err)?).await.map_err(js_err).map(|t| t.as_string().unwrap_or_default())
}

/// A server-sent stream with the token: each `data:` line to `on` until
/// the stream ends (`Ok`) or `on` says stop (`Ok`); an error says why.
pub(crate) async fn stream_lines(url: &str, token: &str, on: &mut dyn FnMut(&str) -> bool) -> Result<(), String> {
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::Cors);
    let headers = web_sys::Headers::new().map_err(js_err)?;
    headers.set("authorization", &format!("Bearer {token}")).map_err(js_err)?;
    headers.set("accept", "text/event-stream").map_err(js_err)?;
    init.set_headers(&headers);
    let req = web_sys::Request::new_with_str_and_init(url, &init).map_err(js_err)?;
    let w = web_sys::window().ok_or("no window")?;
    let resp: Response = JsFuture::from(w.fetch_with_request(&req)).await.map_err(js_err)?.dyn_into().map_err(js_err)?;
    if !resp.ok() {
        return Err(format!("its stream answered {}", resp.status()));
    }
    let body = Reflect::get(&resp, &"body".into()).map_err(js_err)?;
    let get_reader: Function = Reflect::get(&body, &"getReader".into()).map_err(js_err)?.dyn_into().map_err(js_err)?;
    let reader = get_reader.call0(&body).map_err(js_err)?;
    let read: Function = Reflect::get(&reader, &"read".into()).map_err(js_err)?.dyn_into().map_err(js_err)?;
    let mut buf = String::new();
    loop {
        let p: js_sys::Promise = read.call0(&reader).map_err(js_err)?.dyn_into().map_err(js_err)?;
        let chunk = JsFuture::from(p).await.map_err(js_err)?;
        if Reflect::get(&chunk, &"done".into()).ok().and_then(|d| d.as_bool()) == Some(true) {
            return Err("its stream ended".into());
        }
        let bytes = js_sys::Uint8Array::new(&Reflect::get(&chunk, &"value".into()).map_err(js_err)?).to_vec();
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            for line in block.lines() {
                if let Some(d) = line.strip_prefix("data:")
                    && !on(d.trim())
                {
                    return Ok(());
                }
            }
        }
    }
}

/// The companion's URL and this tab's token, while paired.
pub fn conn() -> Option<(String, String)> {
    with(|c| c.token.clone().map(|t| (c.url.clone(), t)))
}

/// A call to the paired companion: `path` is `/v1/...`.
async fn api(method: &str, path: &str, body: Option<Value>) -> Result<String, String> {
    let (url, token) = with(|c| (c.url.clone(), c.token.clone()));
    let token = token.ok_or("not connected to a companion: Settings → Companion")?;
    let text = fetch(method, &format!("{url}{path}"), Some(&token), body.map(|b| b.to_string()).as_deref()).await?;
    match proto::error_of(&text) {
        Some(e) => Err(e),
        None => Ok(text),
    }
}

// ---- the host ----

/// The companion as a host: what `file` and `terminal` artifacts reach.
pub struct Remote {
    url: String,
    token: String,
    open: Option<String>,
    /// The companion has a workspace folder: a relative root is a folder in it.
    ws: bool,
}

/// The companion's workspace primitives (`/v1/fs/*`, `/v1/term/*`), for
/// core's `workspace::Folder` (the `Host` the file and terminal artifacts use).
pub struct Ws {
    url: String,
    token: String,
}

impl Ws {
    fn get(&self, path: &str) -> Result<Value, String> {
        let t = sync("GET", &format!("{}{path}", self.url), &self.token, None)?;
        if let Some(e) = proto::error_of(&t) {
            return Err(e);
        }
        serde_json::from_str(&t).map_err(|e| format!("the companion's answer does not read: {e}"))
    }
    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let t = sync("POST", &format!("{}{path}", self.url), &self.token, Some(&body.to_string()))?;
        if let Some(e) = proto::error_of(&t) {
            return Err(e);
        }
        serde_json::from_str(&t).map_err(|e| format!("the companion's answer does not read: {e}"))
    }
}

fn run_of(v: &Value) -> Run {
    match v.get("status").and_then(Value::as_str) {
        Some("running") => Run::Running,
        Some("exited") => Run::Exited(v.get("exit").and_then(Value::as_i64).map(|c| c as i32)),
        _ => Run::Unknown,
    }
}

impl Workspace_ for Ws {
    fn now(&self) -> i64 {
        host::now()
    }
    fn list(&self, dir: &str, max: usize) -> Result<Vec<String>, String> {
        let v = self.get(&format!("/v1/fs/list?path={}&max={max}", enc(dir)))?;
        Ok(v.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default())
    }
    fn read(&self, path: &str) -> Content {
        match sync("GET", &format!("{}/v1/fs/read?path={}", self.url, enc(path)), &self.token, None) {
            Ok(t) => proto::content_of(&t),
            Err(e) => Content::Missing(e),
        }
    }
    fn write(&self, path: &str, text: &str) -> Result<(), String> {
        self.post("/v1/fs/write", json!({ "path": path, "text": text })).map(|_| ())
    }
    fn stat(&self, path: &str) -> Result<Stat, String> {
        let v = self.get(&format!("/v1/fs/stat?path={}", enc(path)))?;
        Ok(match v.get("kind").and_then(Value::as_str) {
            Some("file") => Stat::File { size: v.get("size").and_then(Value::as_u64).unwrap_or(0), modified: v.get("modified").and_then(Value::as_i64).unwrap_or(0) },
            Some("dir") => Stat::Dir,
            _ => Stat::Missing,
        })
    }
    fn run(&self, key: &str, cwd: &str, command: &str) -> Result<Option<i32>, String> {
        let v = self.post("/v1/term/run", json!({ "key": key, "cwd": cwd, "command": command, "timeout_ms": 30 * 60 * 1000 }))?;
        Ok(v.get("exit").and_then(Value::as_i64).map(|c| c as i32))
    }
    fn start(&self, key: &str, cwd: &str, command: &str, callback: bool) -> Result<(), String> {
        self.post("/v1/term/start", json!({ "key": key, "cwd": cwd, "command": command, "callback": callback })).map(|_| ())
    }
    fn tail(&self, key: &str, lines: usize) -> Result<Tail, String> {
        let v = self.get(&format!("/v1/term/tail?key={}&lines={lines}", enc(key)))?;
        Ok(Tail { run: run_of(&v), text: v.get("tail").and_then(Value::as_str).unwrap_or("").to_string() })
    }
    fn stop(&self, key: &str) -> Result<(), String> {
        self.post("/v1/term/stop", json!({ "key": key })).map(|_| ())
    }
}

/// What this page reaches outside: the companion while connected, else
/// the page's environment, the browser VM (`crate::env`); the browser
/// alone refuses files and processes, saying why.
pub enum Shell {
    Browser(Browser),
    Companion(Remote),
    Vm(crate::env::VmHost),
}

/// The shell now: the environment the simple check picked
/// (`dry_core::environment::probe`).
pub fn shell() -> Shell {
    with(|c| match &c.token {
        Some(t) => Shell::Companion(Remote { url: c.url.clone(), token: t.clone(), open: c.open.clone(), ws: c.workspace.is_some() }),
        None => Shell::Vm(crate::env::VmHost),
    })
}

impl Remote {
    /// The workspace folder, for a root that is a folder in it (`.`, `src`).
    fn folder(&self, root: &str) -> Option<Folder<Ws>> {
        (self.ws && !root.trim().starts_with('/')).then(|| Folder(Ws { url: self.url.clone(), token: self.token.clone() }))
    }
    fn get(&self, path: &str) -> Result<String, String> {
        sync("GET", &format!("{}{path}", self.url), &self.token, None)
    }
    fn post(&self, path: &str, body: Value) -> Result<String, String> {
        sync("POST", &format!("{}{path}", self.url), &self.token, Some(&body.to_string()))
    }
    fn root(&self, root: &str) -> Result<String, String> {
        proto::resolve_root(self.open.as_deref(), root)
    }
    fn ok(text: Result<String, String>) -> Result<String, String> {
        let t = text?;
        proto::error_of(&t).map_or(Ok(t), Err)
    }
    fn session(&self, job: &Job, tail: usize) -> Result<String, String> {
        self.get(&format!("/v1/exec/session?key={}&tail={tail}", enc(&proto::session_key(job))))
    }

    /// A terminal `run`, waited for without blocking the page: its exit
    /// code and last lines, as `RunJob::wait` gives them.
    async fn run_async(&self, job: &dry_core::terminal::RunJob) -> Result<(Option<i32>, String), String> {
        let j = job.job();
        if self.folder(j.root).is_some() {
            let body = json!({ "key": dry_core::workspace::key(&j), "cwd": dry_core::workspace::root_dir(j.root)?, "command": j.command, "timeout_ms": 30 * 60 * 1000 });
            let text = fetch("POST", &format!("{}/v1/term/run", self.url), Some(&self.token), Some(&body.to_string())).await?;
            if let Some(e) = proto::error_of(&text) {
                return Err(e);
            }
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let out = v.get("output").and_then(Value::as_str).unwrap_or("");
            return Ok((v.get("exit").and_then(Value::as_i64).map(|c| c as i32), dry_core::terminal::last_lines(out, dry_core::terminal::RESULT_LINES)));
        }
        let cwd = self.root(j.root)?;
        let body = json!({ "cwd": cwd, "command": j.command, "key": proto::session_key(&j), "timeout_ms": 30 * 60 * 1000 });
        let text = fetch("POST", &format!("{}/v1/exec/run", self.url), Some(&self.token), Some(&body.to_string())).await?;
        proto::run_of(&text).map(|(code, out)| (code, dry_core::terminal::last_lines(&out, dry_core::terminal::RESULT_LINES)))
    }
}

impl Host for Remote {
    fn now(&self) -> i64 {
        host::now()
    }
    fn browser(&self, op: &str, args: &[(String, String)]) -> Result<String, String> {
        let body: serde_json::Map<String, Value> = args.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        let t = self.post(&format!("/v1/browser/{op}"), Value::Object(body))?;
        Remote::ok(Ok(t))
    }
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_list(max);
        }
        if let Some(f) = self.folder(root) {
            return f.list(root, max);
        }
        let root = self.root(root)?;
        let t = Remote::ok(self.get(&format!("/v1/files/list?root={}&max={max}", enc(&root))))?;
        let v: Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
        Ok(v.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default())
    }
    fn read(&self, root: &str, path: &str) -> Content {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_read(path);
        }
        if let Some(f) = self.folder(root) {
            return f.read(root, path);
        }
        let root = match self.root(root) {
            Ok(r) => r,
            Err(e) => return Content::Missing(e),
        };
        match self.get(&format!("/v1/files/read?path={}&within={}", enc(&proto::join(&root, path)), enc(&root))) {
            Ok(t) => proto::content_of(&t),
            Err(e) => Content::Missing(e),
        }
    }
    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_write(path, text);
        }
        if let Some(f) = self.folder(root) {
            return f.write(root, path, text);
        }
        let root = self.root(root)?;
        Remote::ok(self.post("/v1/files/write", json!({ "path": proto::join(&root, path), "within": root, "text": text }))).map(|_| ())
    }
    fn run(&self, job: &Job) -> Result<Option<i32>, String> {
        if let Some(f) = self.folder(job.root) {
            return f.run(job);
        }
        let cwd = self.root(job.root)?;
        let t = self.post("/v1/exec/run", json!({ "cwd": cwd, "command": job.command, "key": proto::session_key(job), "timeout_ms": 30 * 60 * 1000 }))?;
        proto::run_of(&t).map(|(code, _)| code)
    }
    fn spawn(&self, job: &Job) -> Result<(), String> {
        if let Some(f) = self.folder(job.root) {
            return f.spawn(job);
        }
        let cwd = self.root(job.root)?;
        Remote::ok(self.post("/v1/exec/start", json!({ "cwd": cwd, "command": job.command, "key": proto::session_key(job), "mode": "start" }))).map(|_| ())
    }
    fn status(&self, job: &Job) -> Proc {
        if let Some(f) = self.folder(job.root) {
            return f.status(job);
        }
        self.session(job, 1).map_or(Proc::Ended(None), |t| proto::proc_of(&t))
    }
    fn tail(&self, job: &Job, lines: usize) -> String {
        if let Some(f) = self.folder(job.root) {
            return f.tail(job, lines);
        }
        self.session(job, lines)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("tail").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default()
    }
}

impl Host for Shell {
    fn now(&self) -> i64 {
        host::now()
    }
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        match self {
            Shell::Browser(b) => b.list(root, max),
            Shell::Vm(v) => v.list(root, max),
            Shell::Companion(r) => r.list(root, max),
        }
    }
    fn read(&self, root: &str, path: &str) -> Content {
        match self {
            Shell::Browser(b) => b.read(root, path),
            Shell::Vm(v) => v.read(root, path),
            Shell::Companion(r) => r.read(root, path),
        }
    }
    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        match self {
            Shell::Browser(b) => b.write(root, path, text),
            Shell::Vm(v) => v.write(root, path, text),
            Shell::Companion(r) => r.write(root, path, text),
        }
    }
    fn run(&self, job: &Job) -> Result<Option<i32>, String> {
        match self {
            Shell::Browser(b) => b.run(job),
            Shell::Vm(v) => v.run(job),
            Shell::Companion(r) => r.run(job),
        }
    }
    fn spawn(&self, job: &Job) -> Result<(), String> {
        match self {
            Shell::Browser(b) => b.spawn(job),
            Shell::Vm(v) => v.spawn(job),
            Shell::Companion(r) => r.spawn(job),
        }
    }
    fn status(&self, job: &Job) -> Proc {
        match self {
            Shell::Browser(b) => b.status(job),
            Shell::Vm(v) => v.status(job),
            Shell::Companion(r) => r.status(job),
        }
    }
    fn tail(&self, job: &Job, lines: usize) -> String {
        match self {
            Shell::Browser(b) => b.tail(job, lines),
            Shell::Vm(v) => v.tail(job, lines),
            Shell::Companion(r) => r.tail(job, lines),
        }
    }
    fn browser(&self, op: &str, args: &[(String, String)]) -> Result<String, String> {
        match self {
            Shell::Browser(b) => b.browser(op, args),
            Shell::Vm(v) => v.browser(op, args),
            Shell::Companion(r) => r.browser(op, args),
        }
    }
}

/// Outside tools for a script's steps (synchronous): a companion tool, or
/// none (the built-ins' "no such tool").
pub fn external(call: &Call) -> Option<Result<String, String>> {
    if let Some(done) = crate::caps::route_sync(call) {
        return Some(done);
    }
    let (tool, url, token) = with(|c| (proto::tool_for(&c.tools, call).cloned(), c.url.clone(), c.token.clone()));
    let (Some(tool), Some(token)) = (tool, token) else { return host::external(call) };
    Some(sync("POST", &format!("{url}/v1/tools/call"), &token, Some(&proto::tool_call_body(&tool, call))).and_then(|t| proto::tool_result(&t)))
}

/// A companion tool's call, without blocking the page.
async fn call_tool(call: &Call) -> Option<Result<String, String>> {
    let tool = with(|c| proto::tool_for(&c.tools, call).cloned())?;
    let (url, token) = with(|c| (c.url.clone(), c.token.clone()));
    let token = token?;
    Some(fetch("POST", &format!("{url}/v1/tools/call"), Some(&token), Some(&proto::tool_call_body(&tool, call))).await.and_then(|t| proto::tool_result(&t)))
}

impl Time for Shell {
    fn clock(&self) -> Clock {
        Browser.clock()
    }
}

/// One move's I/O (`runner::io`), with a companion's terminal `run`s
/// waited for asynchronously, one after another.
pub async fn io<T: Transport>(next: &Next, t: &T, shell: &Shell, external: &dyn Fn(&Call) -> Option<Result<String, String>>) -> Came {
    if let Next::Run(waits) = next {
        let mut came = Vec::with_capacity(waits.len());
        for w in waits {
            came.push(match (w, shell) {
                (Wait::Run(job), Shell::Companion(r)) => turn::Came::Run(r.run_async(job).await),
                (Wait::Run(job), Shell::Vm(_)) => turn::Came::Run(crate::env::run(job).await),
                (Wait::Run(_), _) => turn::run_wait(w, &mut |c: &Call| external(c), shell),
                // A capability's tool first (the page's own, or the companion's over MCP),
                // then a companion custom tool, then whatever else knows it.
                (Wait::Outside(call), _) => turn::Came::Outside(match crate::caps::route(call).await {
                    Some(r) => Some(r),
                    None => match call_tool(call).await {
                        Some(r) => Some(r),
                        None => external(call),
                    },
                }),
            });
        }
        return Came::Ran(came);
    }
    runner::io(next, t, shell, external).await
}

// ---- the store on the companion ----

/// The companion's store (`/v1/store`): the state kept as files on its
/// computer (in a workspace, `<folder>/.dry/state/`), chosen in Settings →
/// Companion. One writer: while paired the page writes through the
/// companion; IndexedDB (`cache`) is the offline copy, written with every
/// save. Away (the companion stopped, the pairing ended), the page works
/// from the copy and marks it pending; paired again, the pending texts go
/// to the companion first (docs/companion-protocol.md, "The workspace's state").
pub struct Store {
    url: String,
    token: String,
    pub(crate) cache: Option<crate::idb::Idb>,
    online: std::cell::Cell<bool>,
}

/// The store this tab chose: the companion at that URL, with this tab's
/// pairing when it has one (`None` when it chose none).
pub fn chosen_store() -> Option<Store> {
    let chosen = storage()?.get_item(STORE_KEY).ok().flatten()?;
    let token = kept().filter(|k| k.url == chosen).map(|k| k.token).unwrap_or_default();
    Some(Store { url: chosen, token, cache: None, online: std::cell::Cell::new(true) })
}

/// The offline copy has texts the companion has not had yet (localStorage).
fn pending() -> bool {
    storage().and_then(|s| s.get_item(PENDING_KEY).ok().flatten()).is_some()
}

fn set_pending(on: bool) {
    if let Some(s) = storage() {
        let _ = if on { s.set_item(PENDING_KEY, "1") } else { s.remove_item(PENDING_KEY) };
    }
}

impl Store {
    /// It answers, and the pairing is still good.
    pub async fn check(&self) -> Result<(), String> {
        if self.token.is_empty() {
            return Err("this tab is not paired with it".into());
        }
        let t = fetch("GET", &format!("{}/v1/health", self.url), Some(&self.token), None).await?;
        match serde_json::from_str::<Value>(&t).ok().and_then(|v| v.get("paired")?.as_bool()) {
            Some(true) => Ok(()),
            _ => Err("the pairing ended".into()),
        }
    }

    /// Working from the offline copy from now on.
    pub fn offline(&self) {
        self.online.set(false);
    }

    pub fn is_online(&self) -> bool {
        self.online.get()
    }

    async fn remote_texts(&self) -> Result<Texts, String> {
        let t = fetch("GET", &format!("{}/v1/store", self.url), Some(&self.token), None).await?;
        if let Some(e) = proto::error_of(&t) {
            return Err(e);
        }
        let v: Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
        Ok(v.get("texts")
            .and_then(Value::as_object)
            .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
            .unwrap_or_default())
    }

    async fn remote_write(&self, changes: &Changes) -> Result<(), String> {
        let body = json!({ "put": changes.put, "remove": changes.remove }).to_string();
        let t = fetch("POST", &format!("{}/v1/store", self.url), Some(&self.token), Some(&body)).await?;
        proto::error_of(&t).map_or(Ok(()), Err)
    }
}

impl TextStore for Store {
    fn describe(&self) -> String {
        if self.online.get() { format!("companion:{}", self.url) } else { format!("companion:{} (away: this browser's copy)", self.url) }
    }
    async fn read_all(&self) -> Result<Texts, String> {
        let Some(cache) = &self.cache else { return self.remote_texts().await };
        if !self.online.get() {
            return cache.read_all().await;
        }
        // Back from away: what changed in the copy goes to the companion first.
        if pending() {
            let mine = cache.read_all().await?;
            let theirs = self.remote_texts().await?;
            self.remote_write(&Changes::between(&theirs, &mine)).await?;
            set_pending(false);
        }
        let texts = self.remote_texts().await?;
        let had = cache.read_all().await.unwrap_or_default();
        let _ = cache.write(&Changes::between(&had, &texts)).await;
        Ok(texts)
    }
    async fn write(&self, changes: &Changes) -> Result<(), String> {
        if let Some(cache) = &self.cache {
            cache.write(changes).await?;
        }
        if !self.online.get() {
            set_pending(true);
            return Ok(());
        }
        match self.remote_write(changes).await {
            Ok(()) => Ok(()),
            Err(e) if self.cache.is_some() => {
                // Kept in the copy; it goes back when the companion does.
                self.online.set(false);
                set_pending(true);
                crate::app::note(&format!("the companion did not take the save ({e}): kept in this browser until you pair again"));
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

/// The workspace's snapshot now, from the companion (`GET /v1/workspace`).
pub async fn workspace_snapshot(store: &Store) -> Option<dry_core::workspace::Snapshot> {
    if !store.is_online() {
        return None;
    }
    let t = fetch("GET", &format!("{}/v1/workspace", store.url), Some(&store.token), None).await.ok()?;
    let v: Value = serde_json::from_str(&t).ok()?;
    let snap = v.get("snapshot").filter(|s| !s.is_null())?;
    dry_core::workspace::Snapshot::from_json(&snap.to_string()).ok()
}

/// Write the workspace's snapshot (`.dry/snapshot.json`) after a save.
pub fn save_snapshot(store: &Store, state: &dry_core::state::State) {
    if !store.is_online() || with(|c| c.workspace.is_none()) {
        return;
    }
    let snap = dry_core::workspace::Snapshot::of(state, host::now(), None);
    let (url, token) = (store.url.clone(), store.token.clone());
    let body = json!({ "snapshot": serde_json::from_str::<Value>(&snap.to_json()).unwrap_or(Value::Null) }).to_string();
    spawn_local(async move {
        if let Err(e) = fetch("POST", &format!("{url}/v1/workspace/snapshot"), Some(&token), Some(&body)).await {
            web_sys::console::log_1(&JsValue::from_str(&format!("snapshot not saved: {e}")));
        }
    });
}

fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok().flatten()
}

/// This tab's pairing, if it has one that has not expired.
fn kept() -> Option<Kept> {
    let text = session_storage()?.get_item(PAIRING_KEY).ok().flatten()?;
    Kept::from_json(&text, host::now())
}

fn keep(k: Option<&Kept>) {
    if let Some(s) = session_storage() {
        let _ = match k {
            Some(k) => s.set_item(PAIRING_KEY, &k.to_json()),
            None => s.remove_item(PAIRING_KEY),
        };
        if k.is_none() {
            let _ = s.remove_item(EVENT_KEY);
        }
    }
}

fn last_event() -> u64 {
    session_storage().and_then(|s| s.get_item(EVENT_KEY).ok().flatten()).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn set_last_event(id: u64) {
    if let Some(s) = session_storage() {
        let _ = s.set_item(EVENT_KEY, &id.to_string());
    }
}

// ---- connecting ----

fn field(name: &str) -> String {
    with(|c| c.fields.get(name).cloned().unwrap_or_default())
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn set_line(line: impl Into<String>) {
    with(|c| c.line = line.into());
    crate::app::redraw();
}

fn connect() {
    let url = proto::clean_url(&field("url"));
    let code = field("code").trim().to_uppercase();
    if code.is_empty() {
        return crate::app::note("Type the pairing code the companion printed.");
    }
    with(|c| {
        c.url = url.clone();
        c.fields.insert("url".into(), url.clone());
        c.line = format!("pairing with {url}");
    });
    if let Some(s) = storage() {
        let _ = s.set_item(URL_KEY, &url);
    }
    crate::app::redraw();
    spawn_local(async move {
        let health = match fetch("GET", &format!("{url}/v1/health"), None, None).await {
            Ok(h) => h,
            Err(e) => return set_line(e),
        };
        let protocol = serde_json::from_str::<Value>(&health).ok().and_then(|v| v.get("protocol")?.as_u64());
        if protocol != Some(proto::PROTOCOL) {
            return set_line(format!("{url} is not a dry companion this page speaks to (protocol {protocol:?}, this page speaks {})", proto::PROTOCOL));
        }
        let paired = fetch("POST", &format!("{url}/v1/pair"), None, Some(&json!({ "code": code }).to_string())).await;
        let token = paired.and_then(|t| match proto::error_of(&t) {
            Some(e) => Err(e),
            None => serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| Some((v.get("token")?.as_str()?.to_string(), v.get("expires").and_then(Value::as_i64).unwrap_or(host::now() + 12 * 3600))))
                .ok_or("no token came back".into()),
        });
        let (token, expires) = match token {
            Ok(t) => t,
            Err(e) => return set_line(e),
        };
        // Kept in this tab's sessionStorage: a reload keeps the pairing, closing the tab ends it.
        keep(Some(&Kept { url: url.clone(), token: token.clone(), expires }));
        set_last_event(0);
        with(|c| {
            c.token = Some(token);
            c.expires = expires;
            c.fields.remove("code");
        });
        refresh().await;
        // A workspace folder that already keeps dry's state (or the one this
        // tab chose before, back after a restart): the page loads from it.
        if with(|c| c.workspace.is_some()) {
            let chosen = storage().and_then(|s| s.get_item(STORE_KEY).ok().flatten()).is_some_and(|u| u == url);
            let has_state = api("GET", "/v1/store", None)
                .await
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|v| v.get("texts")?.as_object().map(|m| m.keys().any(|k| k.starts_with("engines/"))))
                .unwrap_or(false);
            if chosen || has_state {
                if let Some(s) = storage() {
                    let _ = s.set_item(STORE_KEY, &url);
                }
                crate::app::note("Paired: loading the folder's state.");
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
                return;
            }
        }
        crate::app::companion_changed();
        listen_events();
        crate::app::note("Connected to the companion.");
    });
}

/// Capabilities and projects again.
async fn refresh() {
    let caps_text = api("GET", "/v1/capabilities", None).await;
    let caps = caps_text.clone().and_then(|t| Capabilities::from_json(&t));
    let tools = caps_text.as_deref().map(proto::tools_of).unwrap_or_default();
    let tool_errors: Vec<String> = caps_text
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("tool_errors")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|e| format!("{}: {}", e.get("file").and_then(Value::as_str).unwrap_or(""), e.get("error").and_then(Value::as_str).unwrap_or("")))
        .collect();
    let projects = api("GET", "/v1/projects", None).await.and_then(|t| proto::projects_of(&t));
    let pairings = api("GET", "/v1/pairings", None).await.and_then(|t| proto::pairings_of(&t));
    let workspace = api("GET", "/v1/workspace", None).await.ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| Workspace {
        folder: v.get("folder").and_then(Value::as_str).unwrap_or("").to_string(),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        file: v.pointer("/switches/file").and_then(Value::as_bool).unwrap_or(false),
        terminal: v.pointer("/switches/terminal").and_then(Value::as_bool).unwrap_or(false),
    });
    let providers = api("GET", "/v1/providers", None).await.ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    with(|c| {
        c.workspace = workspace;
        c.providers = providers;
        match caps {
            Ok(k) => {
                c.line = format!("connected to {} · {} · companion {}", c.url, k.platform, k.version);
                c.caps = Some(k);
            }
            Err(e) => c.line = e,
        }
        if let Ok(p) = projects {
            c.projects = p;
        }
        if let Ok(p) = pairings {
            c.pairings = p;
        }
        c.tools = tools;
        c.tool_errors = tool_errors;
    });
    crate::app::redraw();
}

fn disconnect() {
    let had = with(|c| c.token.is_some());
    if had {
        spawn_local(async {
            let _ = api("POST", "/v1/unpair", None).await;
            keep(None);
            with(|c| {
                c.token = None;
                c.tools.clear();
                c.caps = None;
                c.listening = false;
                c.line = "disconnected".into();
            });
            crate::app::companion_changed();
        });
    }
}

/// The companion's event stream (a fetch: `EventSource` cannot send the
/// token header). A session's exit is a callback: the engines' callbacks
/// run at once, and the projects view says so.
fn listen_events() {
    if with(|c| std::mem::replace(&mut c.listening, true)) {
        return;
    }
    spawn_local(async {
        let (url, token) = with(|c| (c.url.clone(), c.token.clone()));
        let Some(token) = token else { return };
        // From the last event this tab had: what ended while it was away comes first.
        let why = match read_events(&format!("{url}/v1/events?since={}", last_event()), &token).await {
            Ok(()) => "the companion's event stream ended (it stopped?)".to_string(),
            Err(e) => e,
        };
        let still = with(|c| {
            c.listening = false;
            c.token.as_deref() == Some(token.as_str())
        });
        if still {
            keep(None);
            with(|c| {
                c.token = None;
                c.tools.clear();
                c.line = format!("{why}: connect again (a restarted companion prints a new code)");
            });
            crate::app::companion_changed();
        }
    });
}

async fn read_events(url: &str, token: &str) -> Result<(), String> {
    let resp = send("GET", url, Some(token), None).await?;
    if !resp.ok() {
        return Err(format!("the event stream answered {}", resp.status()));
    }
    let body = Reflect::get(&resp, &"body".into()).map_err(js_err)?;
    let get_reader: Function = Reflect::get(&body, &"getReader".into()).map_err(js_err)?.dyn_into().map_err(js_err)?;
    let reader = get_reader.call0(&body).map_err(js_err)?;
    let read: Function = Reflect::get(&reader, &"read".into()).map_err(js_err)?.dyn_into().map_err(js_err)?;
    let mut buf = String::new();
    loop {
        if !connected() {
            return Ok(());
        }
        let p: js_sys::Promise = read.call0(&reader).map_err(js_err)?.dyn_into().map_err(js_err)?;
        let chunk = JsFuture::from(p).await.map_err(js_err)?;
        if Reflect::get(&chunk, &"done".into()).ok().and_then(|d| d.as_bool()) == Some(true) {
            return Ok(());
        }
        let bytes = js_sys::Uint8Array::new(&Reflect::get(&chunk, &"value".into()).map_err(js_err)?).to_vec();
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            let mut event = "message";
            let mut data = String::new();
            for line in block.lines() {
                if let Some(id) = line.strip_prefix("id: ").and_then(|i| i.trim().parse::<u64>().ok()) {
                    set_last_event(id);
                }
                if let Some(e) = line.strip_prefix("event: ") {
                    event = if e == "exit" { "exit" } else if e == "hello" { "hello" } else { "other" };
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data.push_str(d);
                }
            }
            if event == "exit" {
                on_exit(&data);
            }
        }
    }
}

fn on_exit(data: &str) {
    let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
    let id = v.get("id").and_then(Value::as_str).unwrap_or("?");
    let command = v.get("command").and_then(Value::as_str).unwrap_or("");
    let exit = v.get("exit").and_then(Value::as_i64).map_or_else(|| "stopped".to_string(), |c| format!("exit {c}"));
    let line = format!("session {id} `{command}` ended, {exit}");
    crate::app::note(&line);
    with(|c| c.output = format!("{line}\n{}", c.output));
    // An engine's terminal session that ended: its callback now, not at the next tick.
    crate::app::settle_now();
    spawn_local(async {
        load_sessions().await;
        crate::app::redraw();
    });
}

// ---- the projects view's actions ----

async fn load_sessions() {
    if let Ok(t) = api("GET", "/v1/exec/sessions", None).await {
        let list = serde_json::from_str::<Value>(&t).ok().and_then(|v| v.get("sessions")?.as_array().cloned()).unwrap_or_default();
        with(|c| c.sessions = list);
    }
}

async fn load_dir(dir: String) {
    let got = api("GET", &format!("/v1/files/tree?path={}", enc(&dir)), None).await.and_then(|t| proto::tree_of(&t));
    with(|c| match got {
        Ok(e) => {
            c.dir = Some(dir);
            c.entries = e;
        }
        Err(e) => c.output = e,
    });
}

async fn load_git() {
    let Some(open) = with(|c| c.open.clone()) else { return };
    let got = api("POST", "/v1/git/status", Some(json!({ "path": open }))).await.and_then(|t| GitStatus::from_json(&t));
    with(|c| c.git = Some(got));
}

fn cwd() -> Option<String> {
    with(|c| c.dir.clone().or(c.open.clone()))
}

fn out(text: impl Into<String>) {
    with(|c| c.output = text.into());
    crate::app::redraw();
}

fn confirm(text: &str) -> bool {
    web_sys::window().and_then(|w| w.confirm_with_message(text).ok()) == Some(true)
}

/// A control of this module was pressed (`data-act="companion-…"`).
pub fn act(what: &str, b: &Element) {
    let value = b.get_attribute("data-value").unwrap_or_default();
    match what {
        "companion-connect" => connect(),
        "companion-stop-all" => {
            if confirm("Stop every command this companion runs, and its browser?") {
                spawn_local(async {
                    let r = api("POST", "/v1/stop-all", None).await;
                    load_sessions().await;
                    out(r.map(|t| format!("stop all: {t}")).unwrap_or_else(|e| e));
                    crate::app::note("Stopped everything the companion was running.");
                });
            }
        }
        "companion-revoke" => spawn_local(async move {
            let r = api("POST", "/v1/pairings/revoke", Some(json!({ "id": value }))).await;
            if let Err(e) = r {
                crate::app::note(&e);
            }
            if connected() {
                refresh().await;
            }
        }),
        "companion-store-companion" => {
            let url = with(|c| c.url.clone());
            if !confirm(&format!("Keep this browser's engines on the companion ({url}) from now on? They are copied there, and the page reloads.")) {
                return;
            }
            spawn_local(async move {
                let texts = crate::app::all_texts();
                let there = api("GET", "/v1/store", None).await.ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
                let old: Vec<String> = there.and_then(|v| v.get("texts")?.as_object().map(|m| m.keys().cloned().collect())).unwrap_or_default();
                let remove: Vec<String> = old.into_iter().filter(|k| !texts.contains_key(k)).collect();
                match api("POST", "/v1/store", Some(json!({ "put": texts, "remove": remove }))).await {
                    Ok(_) => {
                        if let Some(s) = storage() {
                            let _ = s.set_item(STORE_KEY, &url);
                        }
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().reload();
                        }
                    }
                    Err(e) => crate::app::note(&format!("not copied: {e}")),
                }
            });
        }
        "companion-store-here" => {
            if confirm("Keep the state in this browser again? The page reloads with what this browser kept before (changes made on the companion stay there).") {
                if let Some(s) = storage() {
                    let _ = s.remove_item(STORE_KEY);
                }
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
            }
        }
        "companion-tool" => {
            let args = field(&format!("args-{value}"));
            let args: Value = if args.trim().is_empty() { json!({}) } else {
                match serde_json::from_str(&args) {
                    Ok(v) => v,
                    Err(e) => return crate::app::note(&format!("the arguments are JSON: {e}")),
                }
            };
            out(format!("{value}…"));
            spawn_local(async move {
                let r = api("POST", "/v1/tools/call", Some(json!({ "name": value, "arguments": args }))).await;
                out(match r.and_then(|t| proto::tool_result(&t)) {
                    Ok(o) => format!("tool {value}\n{o}"),
                    Err(e) => format!("tool {value} failed\n{e}"),
                });
            });
        }
        "companion-disconnect" => disconnect(),
        "companion-refresh" => spawn_local(refresh()),
        "companion-models" => {
            let base = with(|c| proto::inference_base(&c.url));
            crate::app::use_models_from(&base);
        }
        "companion-open" => {
            with(|c| {
                c.open = Some(value.clone());
                c.file = None;
                c.git = None;
            });
            crate::app::companion_changed();
            spawn_local(async move {
                load_dir(value).await;
                load_git().await;
                load_sessions().await;
                crate::app::redraw();
            });
            if here() != PATH {
                crate::app::go_to(&format!("#{PATH}"));
            }
        }
        "companion-dir" => spawn_local(async move {
            load_dir(value).await;
            crate::app::redraw();
        }),
        "companion-file" => spawn_local(async move {
            let got = api("GET", &format!("/v1/files/read?path={}", enc(&value)), None).await;
            let shown = match got.map(|t| proto::content_of(&t)) {
                Ok(Content::Text(t)) => t,
                Ok(Content::Binary { bytes, .. }) => format!("(not text: {bytes} bytes)"),
                Ok(Content::TooLarge(n)) => format!("(too large to show: {n} bytes)"),
                Ok(Content::Missing(why)) | Err(why) => why,
            };
            with(|c| c.file = Some((value, shown)));
            crate::app::redraw();
        }),
        "companion-run" | "companion-start" | "companion-background" => {
            let command = field("cmd");
            let Some(cwd) = cwd() else { return out("open a project first") };
            if command.trim().is_empty() {
                return crate::app::note("Type a command first.");
            }
            let mode = what.trim_start_matches("companion-").to_string();
            out(format!("$ {command}\n…"));
            spawn_local(async move {
                let body = json!({ "cwd": cwd, "command": command, "mode": mode, "timeout_ms": if mode == "run" { 10 * 60 * 1000 } else { 0 } });
                let path = if mode == "run" { "/v1/exec/run" } else { "/v1/exec/start" };
                let got = api("POST", path, Some(body)).await;
                let text = match got {
                    Ok(t) if mode == "run" => {
                        let (code, output) = proto::run_of(&t).unwrap_or((None, t));
                        format!("$ {command}\n{output}\n(exit {})", code.map_or("none: stopped".to_string(), |c| c.to_string()))
                    }
                    Ok(t) => {
                        let id = serde_json::from_str::<Value>(&t).ok().and_then(|v| v.get("id")?.as_str().map(str::to_string)).unwrap_or_default();
                        if mode == "start" {
                            format!("session {id} `{command}` started; a callback comes when it ends")
                        } else {
                            format!("session {id} `{command}` started in the background (no callback)")
                        }
                    }
                    Err(e) => format!("$ {command}\n{e}"),
                };
                load_sessions().await;
                out(text);
            });
        }
        "companion-stop" => spawn_local(async move {
            let r = api("POST", "/v1/exec/stop", Some(json!({ "id": value }))).await;
            load_sessions().await;
            out(r.map(|_| "stopping…".to_string()).unwrap_or_else(|e| e));
        }),
        "companion-tail" => spawn_local(async move {
            let r = api("GET", &format!("/v1/exec/session?id={}&tail=60", enc(&value)), None).await;
            let shown = r
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .map(|v| format!("session {value} `{}`\n{}", v.get("command").and_then(Value::as_str).unwrap_or(""), v.get("tail").and_then(Value::as_str).unwrap_or("")))
                .unwrap_or_default();
            out(shown);
        }),
        "companion-git-status" => spawn_local(async {
            load_git().await;
            crate::app::redraw();
        }),
        "companion-git-diff" => git_op("diff", json!({}), None),
        "companion-commit" => {
            let message = field("message");
            if message.trim().is_empty() {
                return crate::app::note("Type a commit message first.");
            }
            with(|c| {
                c.fields.remove("message");
            });
            git_op("commit", json!({ "message": message }), None);
        }
        "companion-pull" => git_op("pull", json!({}), None),
        "companion-push" => {
            let Some(open) = with(|c| c.open.clone()) else { return };
            let branch = with(|c| c.git.as_ref().and_then(|g| g.as_ref().ok()).map(|g| g.branch.clone())).unwrap_or_default();
            // The person's click is the confirmation (docs/companion.md).
            if confirm(&format!("Push {branch} of {open} to its remote (origin)? This publishes the commits.")) {
                git_op("push", json!({ "confirmed": true }), Some("pushed"));
            }
        }
        "companion-clone" => {
            let url = field("clone").trim().to_string();
            let root = with(|c| c.caps.as_ref().and_then(|k| k.roots.first().cloned())).unwrap_or_default();
            if url.is_empty() {
                return crate::app::note("Type a repository URL first.");
            }
            if confirm(&format!("Clone {url} into {root}?")) {
                spawn_local(async move {
                    let r = api("POST", "/v1/git/clone", Some(json!({ "url": url, "root": root, "confirmed": true }))).await;
                    refresh().await;
                    out(r.and_then(|t| proto::git_line(&t)).unwrap_or_else(|e| e));
                });
            }
        }
        "companion-attach-folder" => {
            let target = value.parse::<usize>().ok();
            let name = with(|c| c.workspace.as_ref().map(|w| w.name.clone())).unwrap_or_else(|| "folder".into());
            let artifacts = vec![("files".to_string(), "kind: file\nroot: .\n".to_string()), ("terminal".to_string(), "kind: terminal\nroot: .\n".to_string())];
            if let Some(i) = crate::app::attach(target, &name, artifacts) {
                crate::app::note(&format!("{name} is attached: the engine has the folder's files and a terminal in it."));
                crate::app::go_to(&format!("#{}", dry_core::liveview::path(i, None)));
            }
        }
        "companion-providers" => spawn_local(async {
            let p = api("GET", "/v1/providers?refresh=1", None).await.ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
            with(|c| c.providers = p);
            crate::app::redraw();
        }),
        "companion-add-provider" => {
            let mut entry = serde_json::Map::new();
            for k in ["name", "kind", "cli", "url", "model", "runner", "path"] {
                let v = field(&format!("p-{k}")).trim().to_string();
                if !v.is_empty() {
                    entry.insert(k.into(), json!(v));
                }
            }
            spawn_local(async move {
                let r = api("POST", "/v1/providers", Some(json!({ "provider": entry }))).await;
                match r.and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
                    Ok(v) => {
                        let p = &v["provider"];
                        crate::app::note(&format!("{} added: {}", p["name"].as_str().unwrap_or(""), p["state"].as_str().unwrap_or("")));
                        with(|c| {
                            for k in ["name", "kind", "cli", "url", "model", "runner", "path"] {
                                c.fields.remove(&format!("p-{k}"));
                            }
                        });
                    }
                    Err(e) => crate::app::note(&format!("not added: {e}")),
                }
                refresh().await;
            });
        }
        "companion-attach" => {
            let Some(open) = with(|c| c.open.clone()) else { return };
            let target = value.parse::<usize>().ok();
            let name = open.rsplit('/').next().unwrap_or("project").to_string();
            let artifacts = proto::project_artifacts(&open).to_vec();
            if let Some(i) = crate::app::attach(target, &name, artifacts) {
                crate::app::note(&format!("{name} is attached: the engine has its files and a terminal in it."));
                crate::app::go_to(&format!("#{}", dry_core::routes::engine_path(i)));
            }
        }
        _ => {}
    }
}

fn git_op(op: &'static str, mut body: Value, done: Option<&'static str>) {
    let Some(open) = with(|c| c.open.clone()) else { return out("open a project first") };
    body["path"] = json!(open);
    spawn_local(async move {
        let r = api("POST", &format!("/v1/git/{op}"), Some(body)).await.and_then(|t| proto::git_line(&t));
        load_git().await;
        let text = match r {
            Ok(t) if t.is_empty() => format!("git {op}: {}", done.unwrap_or("done")),
            Ok(t) => format!("git {op}\n{t}"),
            Err(e) => format!("git {op} failed\n{e}"),
        };
        out(text);
    });
}

// ---- drawing ----

/// Set up once: the URL kept from last time, and the listeners that keep
/// what is typed in this module's fields (`data-cfield`).
pub fn init() {
    let url = storage().and_then(|s| s.get_item(URL_KEY).ok().flatten()).unwrap_or_else(|| proto::DEFAULT_URL.to_string());
    let restored = kept();
    with(|c| {
        c.url = proto::clean_url(&url);
        c.line = "not connected".into();
        if let Some(k) = &restored {
            // The tab's pairing survives the reload: used at once (the store and
            // the engines' terminals need it), checked just after.
            c.url.clone_from(&k.url);
            c.token = Some(k.token.clone());
            c.expires = k.expires;
            c.line = format!("reconnecting to {}", k.url);
        }
        c.fields.insert("url".into(), c.url.clone());
    });
    spawn_local(async move {
        // A page served by a companion: that companion is the default.
        let origin = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
        if let Ok(t) = fetch("GET", &format!("{origin}/v1/health"), None, None).await
            && serde_json::from_str::<Value>(&t).ok().and_then(|v| v.get("name")?.as_str().map(str::to_string)).as_deref() == Some("dry-companion")
        {
            with(|c| {
                c.served = true;
                if c.token.is_none() {
                    c.url.clone_from(&origin);
                    c.fields.insert("url".into(), origin.clone());
                }
            });
            crate::app::redraw();
        }
        if restored.is_some() {
            let ok = Store { url: with(|c| c.url.clone()), token: with(|c| c.token.clone()).unwrap_or_default(), cache: None, online: std::cell::Cell::new(true) }.check().await;
            match ok {
                Ok(()) => {
                    refresh().await;
                    crate::app::companion_changed();
                    listen_events();
                }
                Err(why) => {
                    keep(None);
                    with(|c| {
                        c.token = None;
                        c.line = format!("the kept pairing did not work ({why}): connect again");
                    });
                    crate::app::companion_changed();
                }
            }
        }
    });
    let Some(d) = doc() else { return };
    let input = Closure::<dyn FnMut(web_sys::Event)>::new(|e: web_sys::Event| {
        if let Some(i) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
            && let Some(name) = i.get_attribute("data-cfield")
        {
            with(|c| c.fields.insert(name, i.value()));
        }
    });
    let _ = d.add_event_listener_with_callback("input", input.as_ref().unchecked_ref());
    input.forget();
    let key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        if e.key() != "Enter" || e.is_composing() {
            return;
        }
        let Some(name) = e.target().and_then(|t| t.dyn_into::<Element>().ok()).and_then(|t| t.get_attribute("data-cfield")) else { return };
        let act = match name.as_str() {
            "code" | "url" => "companion-connect",
            "cmd" => "companion-run",
            "message" => "companion-commit",
            _ => return,
        };
        e.prevent_default();
        if let Some(d) = doc()
            && let Ok(Some(b)) = d.query_selector(&format!("[data-act=\"{act}\"]"))
        {
            act_on(act, &b);
        }
    });
    let _ = d.add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref());
    key.forget();
}

fn act_on(what: &str, b: &Element) {
    act(what, b)
}

/// After every draw: the Projects link, the Companion panel on Settings,
/// and the projects view on its route.
pub fn decorate() {
    let Some(d) = doc() else { return };
    let route = here();
    // The companion's tools on every engine while connected (none otherwise).
    let tools = with(|c| if c.token.is_some() { c.tools.clone() } else { Vec::new() });
    crate::app::equip_tools(&tools);
    if let Some(nav) = d.get_element_by_id("nav")
        && d.get_element_by_id("nav-projects").is_none()
        && let Ok(a) = d.create_element("a")
    {
        a.set_id("nav-projects");
        a.set_class_name(if route == PATH { "nav-item on" } else { "nav-item" });
        let _ = a.set_attribute("href", &format!("#{PATH}"));
        a.set_inner_html("<span class=\"nav-icon\">⌂</span><span class=\"nav-text trunc\">Projects</span>");
        let first_label = nav.query_selector(".nav-label").ok().flatten();
        let _ = nav.insert_before(&a, first_label.as_ref().map(|e| e.unchecked_ref::<web_sys::Node>()));
    }
    if d.get_element_by_id("companion-css").is_none()
        && let (Ok(s), Ok(Some(head))) = (d.create_element("style"), d.query_selector("head"))
    {
        s.set_id("companion-css");
        s.set_text_content(Some(CSS));
        let _ = head.append_child(&s);
    }
    let focused = d.active_element().and_then(|e| e.get_attribute("data-cfield"));
    if route == "/settings" {
        let panel = settings_panel();
        if let Ok(Some(at)) = d.query_selector("#connection") {
            let _ = at.insert_adjacent_html("beforebegin", &panel);
        } else if let Ok(Some(c)) = d.query_selector("#main .content") {
            let _ = c.insert_adjacent_html("afterbegin", &panel);
        }
    } else if route == PATH {
        if let Ok(Some(h)) = d.query_selector("#header .here") {
            h.set_text_content(Some("Projects"));
        }
        if let Ok(Some(c)) = d.query_selector("#main .content") {
            c.set_inner_html(&projects_view());
        }
        d.set_title("Projects · dry");
    }
    if let Some(f) = focused
        && let Ok(Some(e)) = d.query_selector(&format!("[data-cfield=\"{f}\"]"))
        && let Ok(h) = e.dyn_into::<HtmlElement>()
    {
        let _ = h.focus();
    }
}

const CSS: &str = "#companion{display:flex;flex-direction:column;gap:10px}\
#companion .field-label,.cmp .field-label{display:flex;flex-direction:column;gap:4px}\
.cmp{display:flex;flex-direction:column;gap:12px}\
.cmp .panel{display:flex;flex-direction:column;gap:8px}\
.cmp pre,#companion pre{margin:0;white-space:pre-wrap;word-break:break-word;max-height:360px;overflow:auto;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--overlay_hover,rgba(127,127,127,.12));padding:8px;border-radius:8px}\
.cmp .tree{display:flex;flex-direction:column;gap:2px;max-height:420px;overflow:auto}\
.cmp .tree button,.cmp .proj{font:inherit;color:inherit;background:transparent;border:0;text-align:left;cursor:pointer;padding:3px 6px;border-radius:6px}\
.cmp .tree button:hover,.cmp .proj:hover{background:var(--overlay_hover,rgba(127,127,127,.12))}\
.cmp .cols{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1.4fr);gap:12px}\
@media (max-width:760px){.cmp .cols{grid-template-columns:minmax(0,1fr)}}\
.cmp .chips,#companion .chips{display:flex;flex-wrap:wrap;gap:4px}\
.cmp .off,#companion .off{opacity:.5}";

fn input(name: &str, placeholder: &str, kind: &str) -> String {
    format!(
        "<input class=\"field-in\" data-cfield=\"{name}\" type=\"{kind}\" value=\"{}\" placeholder=\"{}\" spellcheck=\"false\" autocomplete=\"off\">",
        esc(&field(name)),
        esc(placeholder)
    )
}

fn button(label: &str, act: &str, value: &str, primary: bool) -> String {
    format!(
        "<button class=\"btn{}\" type=\"button\" data-act=\"{act}\" data-value=\"{}\">{}</button>",
        if primary { " primary" } else { "" },
        esc(value),
        esc(label)
    )
}

fn settings_panel() -> String {
    let mut out = settings_main();
    // Drawn apart: its fields read `C` again.
    if let Some((w, p)) = with(|c| c.workspace.clone().filter(|_| c.token.is_some()).map(|w| (w, c.providers.clone()))) {
        out.push_str(&workspace_panel(&w, p));
    }
    out
}

fn settings_main() -> String {
    with(|c| {
        let mut out = String::from("<div class=\"glass panel\" id=\"companion\"><div class=\"line\"><b>Companion</b>");
        out.push_str(&format!("<span class=\"note trunc grow\">{}</span></div>", esc(&c.line)));
        out.push_str(
            "<p class=\"note\">A small server on a computer that gives this page one folder (its files and a terminal in it), the computer's models and its tools. \
             Run <code>node dry-companion.mjs --folder ~/my-project</code> in a terminal there, then type the code it prints. \
             The token it gives back is kept in this tab only (sessionStorage): a reload keeps the pairing, closing the tab ends it, and it expires (12 h by default).</p>",
        );
        out.push_str(&format!(
            "<label class=\"field-label\"><span class=\"note\">URL</span>{}</label>",
            format_args!(
                "<input class=\"field-in\" data-cfield=\"url\" value=\"{}\" placeholder=\"{}\" spellcheck=\"false\">",
                esc(c.fields.get("url").map_or(&c.url, |u| u)),
                proto::DEFAULT_URL
            )
        ));
        if c.token.is_none() {
            out.push_str(&format!(
                "<label class=\"field-label\"><span class=\"note\">Pairing code</span><input class=\"field-in\" data-cfield=\"code\" value=\"{}\" placeholder=\"6 letters, printed by the companion\" spellcheck=\"false\" autocomplete=\"off\"></label>",
                esc(c.fields.get("code").map_or("", |s| s))
            ));
            out.push_str(&format!("<div class=\"row\">{}</div>", button("Connect", "companion-connect", "", true)));
        } else {
            out.push_str(&format!(
                "<div class=\"row\">{}{}{}<a class=\"btn\" href=\"#{PATH}\">Projects</a></div>",
                button("Use its models", "companion-models", "", true),
                button("Refresh", "companion-refresh", "", false),
                button("Disconnect", "companion-disconnect", "", false)
            ));
        }
        if c.served {
            out.push_str("<p class=\"note\">This page is served by a companion: same origin, so no CORS and no local network prompt.</p>");
        }
        if c.token.is_some() {
            let on_companion = storage().and_then(|s| s.get_item(STORE_KEY).ok().flatten()).is_some_and(|u| u == c.url);
            let until = if c.expires > 0 { format!(" · pairing ends in {} h", ((c.expires - host::now()) / 3600).max(0)) } else { String::new() };
            out.push_str(&format!(
                "<p class=\"note\">State: {}{until}. {} tool{} · {}</p><div class=\"row\">{}{}</div>",
                match (on_companion, &c.workspace) {
                    (true, Some(w)) => format!("kept in the folder {} (.dry/state; this browser keeps a copy for when it is away)", w.name),
                    (true, None) => "kept on the companion".to_string(),
                    (false, _) => "kept in this browser".to_string(),
                },
                c.tools.len(),
                if c.tools.len() == 1 { "" } else { "s" },
                if c.tool_errors.is_empty() { "all tool files read".to_string() } else { format!("{} tool file(s) refused", c.tool_errors.len()) },
                if on_companion { button("Keep state in this browser", "companion-store-here", "", false) } else if c.workspace.is_some() { button("Keep state in the folder", "companion-store-companion", "", true) } else { button("Keep state on the companion", "companion-store-companion", "", false) },
                button("Stop all", "companion-stop-all", "", false)
            ));
            if !c.pairings.is_empty() {
                out.push_str("<div class=\"tree\">");
                for p in &c.pairings {
                    out.push_str(&format!(
                        "<div class=\"line\"><span class=\"note trunc grow\">{} · {}{}</span>{}</div>",
                        esc(&p.id),
                        esc(if p.origin.is_empty() { "(no origin)" } else { &p.origin }),
                        if p.current { " · this tab" } else { "" },
                        button("Revoke", "companion-revoke", &p.id, false)
                    ));
                }
                out.push_str("</div>");
            }
        }
        if let (Some(k), true) = (&c.caps, c.token.is_some()) {
            out.push_str(&format!("<p class=\"note\">{} · roots: {}</p><div class=\"chips\">", esc(&k.hostname), esc(&k.roots.join(", "))));
            for r in &k.runtimes {
                match &r.version {
                    Some(v) => out.push_str(&format!("<span class=\"chip\" title=\"{}\">{}</span>", esc(v), esc(&r.name))),
                    None => out.push_str(&format!("<span class=\"chip off\" title=\"not on this computer's PATH\">{}</span>", esc(&r.name))),
                }
            }
            out.push_str("</div>");
        }
        out.push_str("</div>");
        out
    })
}

/// The workspace folder, the engines it can be given to, and the
/// declared providers with the form that adds one (docs/companion-protocol.md, "The workspace folder" and "Providers").
fn workspace_panel(w: &Workspace, providers: Option<Value>) -> String {
    let onoff = |on: bool| if on { "on" } else { "off: switch it on in the companion's terminal (type: on terminal)" };
    let mut out = format!(
        "<div class=\"glass panel\" id=\"workspace\"><div class=\"line\"><b>Folder</b><span class=\"chip\">{}</span><span class=\"note trunc grow\">{}</span></div><p class=\"note\">files {} · terminal {}</p>",
        esc(&w.name),
        esc(&w.folder),
        onoff(w.file),
        onoff(w.terminal)
    );
    out.push_str("<p class=\"note\">Give the folder to an engine: it gets <code>files</code> (kind: file) and <code>terminal</code> (kind: terminal) rooted at the folder, and its live view shows them.</p><div class=\"row\">");
    out.push_str(&button("New thread", "companion-attach-folder", "new", true));
    for (i, n) in crate::app::engine_names().iter().enumerate() {
        out.push_str(&button(n, "companion-attach-folder", &i.to_string(), false));
    }
    out.push_str("</div></div>");
    // The providers.
    let p = providers.unwrap_or(Value::Null);
    let file = p.get("file").and_then(Value::as_str).unwrap_or("providers.json");
    let list = p.get("providers").and_then(Value::as_array).cloned().unwrap_or_default();
    out.push_str(&format!(
        "<div class=\"glass panel\" id=\"providers\"><div class=\"line\"><b>Providers</b><span class=\"chip\">{}</span><span class=\"note trunc grow\">{}</span>{}</div>",
        list.len(),
        esc(file),
        button("Probe again", "companion-providers", "", false)
    ));
    if list.is_empty() {
        out.push_str("<p class=\"note\">None declared. Paste entries from the template into the file's \"providers\", or add one here. Only what is declared is probed; nothing is installed or downloaded.</p>");
    }
    for x in &list {
        let g = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let models: Vec<String> = x.get("models").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let what = if g("state") == "ok" { models.join(", ") } else { format!("{}{}", g("why"), if g("hint").is_empty() { String::new() } else { format!(" → {}", g("hint")) }) };
        out.push_str(&format!(
            "<div class=\"line\"><span class=\"chip{}\">{}</span><b>{}</b><span class=\"note\">{}</span><span class=\"note trunc grow\" title=\"{}\">{}</span></div>",
            if g("state") == "ok" { "" } else { " off" },
            esc(&g("state")),
            esc(&g("name")),
            esc(&g("kind")),
            esc(&what),
            esc(&what)
        ));
    }
    for e in p.get("errors").and_then(Value::as_array).cloned().unwrap_or_default() {
        out.push_str(&format!("<p class=\"note off\">does not read: {}</p>", esc(e.get("error").and_then(Value::as_str).unwrap_or(""))));
    }
    out.push_str(&format!(
        "<b>Add a provider</b><div class=\"row\">{}{}{}</div><div class=\"row\">{}{}{}{}</div><div class=\"row\">{}</div>",
        input("p-name", "name (lmstudio)", "text"),
        input("p-kind", "kind: cli, port or native", "text"),
        input("p-cli", "cli: claude, codex or copilot", "text"),
        input("p-url", "url (port): http://127.0.0.1:1234", "text"),
        input("p-model", "model (optional for cli)", "text"),
        input("p-runner", "runner (native): auto, mlx, llama.cpp", "text"),
        input("p-path", "path: the program, if not on PATH", "text"),
        button("Add", "companion-add-provider", "", true)
    ));
    if let Some(t) = p.get("template") {
        out.push_str(&format!(
            "<details><summary class=\"note\">The template (examples to paste)</summary><pre>{}</pre></details>",
            esc(&serde_json::to_string_pretty(t).unwrap_or_default())
        ));
    }
    out.push_str("</div>");
    out
}

fn projects_view() -> String {
    let connected = connected();
    let mut out = String::from("<div class=\"cmp\">");
    if !connected {
        out.push_str(&format!(
            "<div class=\"glass panel\"><b>No companion connected</b><p class=\"note\">{}</p><p class=\"note\">Projects live on a computer: run the companion there and connect in <a href=\"#/settings\">Settings → Companion</a>.</p></div></div>",
            esc(&with(|c| c.line.clone()))
        ));
        return out;
    }
    let (projects, open, dir, entries, file, output, sessions, git) =
        with(|c| (c.projects.clone(), c.open.clone(), c.dir.clone(), c.entries.clone(), c.file.clone(), c.output.clone(), c.sessions.clone(), c.git.clone()));
    // The projects.
    out.push_str(&format!("<div class=\"glass panel\"><div class=\"line\"><b>Projects</b><span class=\"chip\">{}</span><span class=\"grow\"></span>{}</div>", projects.len(), button("Refresh", "companion-refresh", "", false)));
    if projects.is_empty() {
        out.push_str("<p class=\"note\">No folders with .git or a manifest under the companion's roots.</p>");
    }
    out.push_str("<div class=\"tree\">");
    for p in &projects {
        let on = open.as_deref() == Some(p.path.as_str());
        out.push_str(&format!(
            "<button class=\"proj\" type=\"button\" data-act=\"companion-open\" data-value=\"{}\">{}<b>{}</b> <span class=\"note\">{} · {}{}</span></button>",
            esc(&p.path),
            if on { "▸ " } else { "" },
            esc(&p.name),
            esc(&p.path),
            esc(&p.markers.join(" ")),
            p.branch.as_ref().map(|b| format!(" · {}", esc(b))).unwrap_or_default()
        ));
    }
    out.push_str("</div></div>");
    let Some(open) = open else {
        out.push_str("</div>");
        return out;
    };
    let name = open.rsplit('/').next().unwrap_or("").to_string();
    out.push_str("<div class=\"cols\">");
    // The tree.
    out.push_str(&format!("<div class=\"glass panel\"><div class=\"line\"><b>{}</b></div>", esc(&name)));
    if let Some(d) = &dir {
        out.push_str(&format!("<p class=\"note trunc\">{}</p><div class=\"tree\">", esc(d)));
        if d != &open
            && let Some((up, _)) = d.rsplit_once('/')
        {
            out.push_str(&format!("<button type=\"button\" data-act=\"companion-dir\" data-value=\"{}\">↑ ..</button>", esc(up)));
        }
        for e in &entries {
            let (act, icon) = if e.dir { ("companion-dir", "▸") } else { ("companion-file", "·") };
            out.push_str(&format!("<button type=\"button\" data-act=\"{act}\" data-value=\"{}\">{icon} {}</button>", esc(&e.path), esc(&e.name)));
        }
        out.push_str("</div>");
    }
    out.push_str("</div><div class=\"stack-2 cmp\">");
    // The file.
    if let Some((p, text)) = &file {
        out.push_str(&format!("<div class=\"glass panel\"><div class=\"line\"><b class=\"trunc\">{}</b></div><pre>{}</pre></div>", esc(p.rsplit('/').next().unwrap_or(p)), esc(text)));
    }
    // The terminal.
    out.push_str(&format!(
        "<div class=\"glass panel\"><div class=\"line\"><b>Terminal</b><span class=\"note trunc grow\">in {}</span></div><label class=\"field-label\">{}</label><div class=\"row\">{}{}{}</div>",
        esc(dir.as_deref().unwrap_or(&open)),
        input("cmd", "a command, run with /bin/sh in this folder", "text"),
        button("Run", "companion-run", "", true),
        button("Start (callback)", "companion-start", "", false),
        button("Background", "companion-background", "", false)
    ));
    if !output.is_empty() {
        out.push_str(&format!("<pre>{}</pre>", esc(&output)));
    }
    let shown: Vec<&Value> = sessions.iter().rev().take(8).collect();
    if !shown.is_empty() {
        out.push_str("<div class=\"tree\">");
        for s in shown {
            let g = |k: &str| s.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let status = match (g("status").as_str(), s.get("exit").and_then(Value::as_i64)) {
                ("running", _) => "running".to_string(),
                (_, Some(c)) => format!("exit {c}"),
                _ => "stopped".to_string(),
            };
            let stop = if g("status") == "running" { button("Stop", "companion-stop", &g("id"), false) } else { String::new() };
            out.push_str(&format!(
                "<div class=\"line\"><button type=\"button\" data-act=\"companion-tail\" data-value=\"{}\" class=\"trunc grow\">#{} {} · {} · <code>{}</code></button>{stop}</div>",
                esc(&g("id")),
                esc(&g("id")),
                esc(&g("mode")),
                esc(&status),
                esc(&g("command"))
            ));
        }
        out.push_str("</div>");
    }
    out.push_str(&format!("<div class=\"row\">{}</div>", button("Stop all", "companion-stop-all", "", false)));
    out.push_str("</div>");
    // Tools.
    let (tools, tool_errors) = with(|c| (c.tools.clone(), c.tool_errors.clone()));
    out.push_str(&format!(
        "<div class=\"glass panel\"><div class=\"line\"><b>Tools</b><span class=\"chip\">{}</span></div><p class=\"note\">From each project's <code>tools/</code> folder and the companion's <code>--tools</code>: every engine has them as <code>companion.&lt;name&gt;</code>, and MCP clients at <code>/mcp</code>.</p>",
        tools.len()
    ));
    for t in &tools {
        out.push_str(&format!(
            "<div class=\"line\"><code class=\"trunc grow\" title=\"{}\">{}</code></div><label class=\"field-label\">{}</label><div class=\"row\">{}</div>",
            esc(&t.description),
            esc(t.describe().trim_start_matches("- ")),
            input(&format!("args-{}", t.name), "arguments as JSON, e.g. {\"who\": \"Ada\"}", "text"),
            button("Call", "companion-tool", &t.name, false)
        ));
    }
    for e in &tool_errors {
        out.push_str(&format!("<p class=\"note off\">refused: {}</p>", esc(e)));
    }
    out.push_str("</div>");
    // Git.
    out.push_str("<div class=\"glass panel\"><div class=\"line\"><b>Git</b>");
    match &git {
        Some(Ok(g)) => out.push_str(&format!("<span class=\"note trunc grow\">{}</span></div>", esc(&g.line()))),
        Some(Err(e)) => out.push_str(&format!("<span class=\"note trunc grow\">{}</span></div>", esc(e))),
        None => out.push_str("</div>"),
    }
    if let Some(Ok(g)) = &git
        && !g.files.is_empty()
    {
        let list: String = g.files.iter().map(|(xy, p)| format!("{xy} {p}\n")).collect();
        out.push_str(&format!("<pre>{}</pre>", esc(&list)));
    }
    out.push_str(&format!(
        "<label class=\"field-label\">{}</label><div class=\"row\">{}{}{}{}{}</div>",
        input("message", "commit message", "text"),
        button("Status", "companion-git-status", "", false),
        button("Diff", "companion-git-diff", "", false),
        button("Commit all", "companion-commit", "", true),
        button("Pull", "companion-pull", "", false),
        button("Push…", "companion-push", "", false)
    ));
    out.push_str("<p class=\"note\">Push and clone run only from your click, after you confirm; an engine can commit with its terminal but cannot push (docs/companion.md).</p></div>");
    // Engines.
    let engines = crate::app::engine_names();
    out.push_str("<div class=\"glass panel\"><div class=\"line\"><b>Give it to an engine</b></div><p class=\"note\">Adds two artifacts rooted here: <code>files</code> (kind: file) and <code>terminal</code> (kind: terminal), so the engine sees the files and can run, start and background commands through this companion.</p><div class=\"row\">");
    out.push_str(&button("New thread", "companion-attach", "new", true));
    for (i, n) in engines.iter().enumerate() {
        out.push_str(&button(n, "companion-attach", &i.to_string(), false));
    }
    out.push_str("</div></div>");
    // Clone.
    out.push_str(&format!(
        "<div class=\"glass panel\"><div class=\"line\"><b>Clone</b></div><label class=\"field-label\">{}</label><div class=\"row\">{}</div></div>",
        input("clone", "https://… , git@host:path or file:///…", "text"),
        button("Clone…", "companion-clone", "", false)
    ));
    out.push_str("</div></div></div>");
    out
}
