//! The browser's capabilities (docs/capabilities.md, "The browser's own capabilities"): what this page
//! can use, each probed, and the companion's, registered over MCP.
//!
//! | id | how | state |
//! |---|---|---|
//! | `folder` | the File System Access API: a folder you pick is this tab's `file` capability (artifacts rooted at `folder`) | needs-grant (a click picks it) |
//! | `transcribe` | Web Speech (`SpeechRecognition`): the composer's Voice button | present where the browser has it |
//! | `model.local` | Chrome's Prompt API (`LanguageModel`): `model.local.ask` | present when available, else absent |
//! | `camera` | `getUserMedia`: `camera.snap` | needs-grant |
//! | `screen` | `getDisplayMedia`: `screen.frame` | needs-grant (every share is a click) |
//! | `notify` | `Notification`: `notify.send` | present when allowed, needs-grant before |
//!
//! A permission that needs a person's gesture is a Grant ask on Settings
//! → Capabilities: the click calls the browser's prompt directly (a
//! gesture is only good for that one call).
//!
//! **The companion joins** (`companion`): `initialize` with this page's
//! hello, `tools/list` and `resources/list` read into its manifest,
//! registered as `remote:companion`; then its stream (`GET /mcp`: updates,
//! events, list_changed) and a heartbeat on the tick. While it is here
//! its `terminal`, `file` and `browser` are this tab's (the page's own are
//! absent), so a build an engine runs goes to the host's tools through
//! the same flow. When it leaves, they leave every prompt.

use std::cell::RefCell;
use std::collections::BTreeMap;

use dry_core::calls::Call;
use dry_core::capability::mcp::{self, Hello, Note};
use dry_core::capability::{Capability, Input, Provider, Registry, Routed, State, Tool, Ty};
use js_sys::{Array, Function, Object, Promise, Reflect};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

/// The root word a file artifact names to read the granted folder.
pub const FOLDER_ROOT: &str = "folder";
/// Most files and bytes read from a granted folder.
const FOLDER_FILES: usize = 2_000;
const FOLDER_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct Caps {
    registry: Registry,
    /// The granted folder: its handle and its files' texts (read when
    /// granted and before each turn; writes go back at once).
    folder: Option<Folder>,
    camera: Option<JsValue>,
    screen: Option<JsValue>,
    /// The companion's session, while it is joined, and when it last answered.
    peer: Option<(String, String)>,
    beat: f64,
    listening: bool,
}

struct Folder {
    name: String,
    handle: JsValue,
    files: BTreeMap<String, String>,
    /// Files bigger than kept, or not text: their sizes.
    other: BTreeMap<String, u64>,
}

thread_local! {
    static C: RefCell<Caps> = RefCell::new(Caps::default());
}

fn with<R>(f: impl FnOnce(&mut Caps) -> R) -> R {
    C.with(|c| f(&mut c.borrow_mut()))
}

fn global(name: &str) -> Option<JsValue> {
    Reflect::get(&js_sys::global(), &name.into()).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

fn get(o: &JsValue, k: &str) -> Option<JsValue> {
    Reflect::get(o, &k.into()).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

fn call(o: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let f: Function = get(o, name).ok_or_else(|| format!("no {name}"))?.dyn_into().map_err(|_| format!("{name} is not a function"))?;
    f.apply(o, &args.iter().collect::<Array>()).map_err(js_err)
}

async fn wait(v: JsValue) -> Result<JsValue, String> {
    match v.dyn_into::<Promise>() {
        Ok(p) => JsFuture::from(p).await.map_err(js_err),
        Err(v) => Ok(v),
    }
}

fn js_err(e: JsValue) -> String {
    e.as_string().or_else(|| get(&e, "message").and_then(|m| m.as_string())).unwrap_or_else(|| format!("{e:?}"))
}

fn navigator() -> Option<JsValue> {
    global("navigator")
}

fn tool(name: &str, doc: &str, inputs: &[(&str, &str, bool)]) -> Tool {
    Tool {
        name: name.into(),
        doc: doc.into(),
        inputs: inputs.iter().map(|(n, d, r)| Input { name: n.to_string(), ty: Ty::String, required: *r, doc: d.to_string() }).collect(),
        approval: false,
        remote: String::new(),
    }
}

fn cap(id: &str, doc: &str, state: State, tools: Vec<Tool>) -> Capability {
    Capability { tools, state, ..Capability::new(id, doc, Provider::Compiled) }
}

/// What the page's own probes find now.
fn probe() -> Vec<Capability> {
    let has = |name: &str| global(name).is_some();
    let media = navigator().and_then(|n| get(&n, "mediaDevices"));
    let folder = match with(|c| c.folder.as_ref().map(|f| f.name.clone())) {
        Some(_) => State::Present,
        None if has("showDirectoryPicker") => {
            State::NeedsGrant("pick a folder: the browser asks which one, and whether this page may change it".into())
        }
        None => State::Absent("this browser has no File System Access API (Chrome and Edge do)".into()),
    };
    let speech = has("SpeechRecognition") || has("webkitSpeechRecognition");
    let notify = match global("Notification").and_then(|n| get(&n, "permission")).and_then(|p| p.as_string()).as_deref() {
        Some("granted") => State::Present,
        Some("denied") => State::Absent("notifications are blocked for this page (the browser's site settings)".into()),
        Some(_) => State::NeedsGrant("allow notifications: the browser asks".into()),
        None => State::Absent("this browser has no notifications".into()),
    };
    let camera = match (with(|c| c.camera.is_some()), media.as_ref().is_some_and(|m| get(m, "getUserMedia").is_some())) {
        (true, _) => State::Present,
        (false, true) => State::NeedsGrant("allow the camera: the browser asks".into()),
        _ => State::Absent("no camera API here (it needs https or localhost)".into()),
    };
    let screen = match (with(|c| c.screen.is_some()), media.as_ref().is_some_and(|m| get(m, "getDisplayMedia").is_some())) {
        (true, _) => State::Present,
        (false, true) => State::NeedsGrant("share a screen or a window: the browser asks which".into()),
        _ => State::Absent("no screen sharing here".into()),
    };
    vec![
        cap(
            "folder",
            "A folder on this computer you picked (the File System Access API): this tab's file capability. A file artifact with `root: folder` reads and writes it.",
            folder,
            Vec::new(),
        ),
        cap(
            "transcribe",
            "Speech to text in this browser (Web Speech): the composer's Voice button puts what you say in the box.",
            if speech { State::Present } else { State::Absent("this browser has no speech recognition".into()) },
            Vec::new(),
        ),
        cap(
            "model.local",
            "A small model inside this browser (Chrome's Prompt API): ask it short things without a server.",
            if has("LanguageModel") {
                State::Absent("checking whether the model is ready".into())
            } else {
                State::Absent("this browser has no Prompt API (Chrome with the built-in model)".into())
            },
            vec![tool("ask", "Ask the browser's own model one thing; its answer back.", &[("text", "what to ask", true)])],
        ),
        cap(
            "camera",
            "This computer's camera, after you allow it: one frame at a time.",
            camera,
            vec![tool("snap", "Take one frame: its size back (the image waits for vision).", &[])],
        ),
        cap(
            "screen",
            "A screen or window you chose to share: one frame at a time.",
            screen,
            vec![tool("frame", "Take one frame of the shared screen: its size back.", &[])],
        ),
        cap(
            "notify",
            "This computer's notifications, after you allow them: tell the person something when they are away from the tab.",
            notify,
            vec![tool("send", "Show a notification.", &[("title", "its title", true), ("body", "its text", false)])],
        ),
    ]
}

/// The registry, given to the state every page draws and every engine
/// uses; then draw. The environment (`crate::env`) decides `file` and
/// `terminal` first.
pub(crate) fn publish() {
    let reg = with(|c| {
        environment(c);
        c.registry.clone()
    });
    crate::app::equip(reg);
}

/// The page's own `file` and `terminal`: the browser VM's when it is
/// ready (a granted folder is a `file` too), absent with why while it
/// boots or after it failed, and absent while a companion is joined (its
/// computer is the environment then, and its file and terminal are used).
fn environment(c: &mut Caps) {
    let peer = c.peer.is_some();
    let env = crate::env::current(peer);
    let why = if peer {
        Some("the paired companion's computer is the environment: its file and terminal are used".to_string())
    } else {
        match &env {
            Some(e) => e.not_ready(),
            None => Some("no environment in this page: pair a companion (Settings → Companion)".into()),
        }
    };
    let state = |why: &Option<String>| why.as_ref().map_or(State::Present, |w| State::Absent(w.clone()));
    let file = if c.folder.is_some() { State::Present } else { state(&why) };
    c.registry.set_state("file", &Provider::Compiled, file);
    c.registry.set_state("terminal", &Provider::Compiled, state(&why));
    c.registry.environment = env;
}

/// At boot: the compiled ones the browser lacks, the page's own probed,
/// the Prompt API asked whether its model is ready.
pub fn init() {
    with(|c| {
        let r = &mut c.registry;
        for (id, why) in [
            ("file", "no files in the browser: grant a folder (Settings → Capabilities), or pair a companion"),
            ("terminal", "no terminal in the browser: pair a companion (Settings → Companion) to run commands on its computer"),
            ("browser", "no headless browser in the page: pair a companion"),
            ("evidence", "evidence runs on a computer: pair a companion"),
        ] {
            r.set_state(id, &Provider::Compiled, State::Absent(why.into()));
        }
    });
    refresh();
    spawn_local(async {
        let Some(lm) = global("LanguageModel") else { return };
        let state = match call(&lm, "availability", &[]) {
            Ok(p) => match wait(p).await.ok().and_then(|v| v.as_string()).as_deref() {
                Some("available") => State::Present,
                Some("downloadable" | "downloading") => {
                    State::Absent("the browser's model is not downloaded (Chrome downloads it on first use)".into())
                }
                Some(other) => State::Absent(format!("the Prompt API says {other}")),
                None => State::Absent("the Prompt API did not answer".into()),
            },
            Err(e) => State::Absent(e),
        };
        with(|c| c.registry.set_state("model.local", &Provider::Compiled, state));
        publish();
    });
}

/// Probe the page's own again and publish.
fn refresh() {
    let model = with(|c| c.registry.effective("model.local").map(|m| m.state.clone()));
    for mut c in probe() {
        if c.id == "model.local"
            && let Some(s) = &model
            && !s.why().starts_with("this browser")
            && *s != State::Absent("checking whether the model is ready".into())
        {
            c.state = s.clone();
        }
        with(|x| x.registry.upsert(c));
    }
    publish();
}

/// Whether the composer's Voice button works here.
pub fn can_transcribe() -> bool {
    with(|c| c.registry.present("transcribe"))
}

/// A Grant click: the browser's prompt, called inside the gesture.
pub fn grant(id: &str) {
    let fail = |e: String| crate::app::note(&format!("{id}: {e}"));
    match id {
        "notify" => match global("Notification").map(|n| call(&n, "requestPermission", &[])) {
            Some(Ok(p)) => spawn_local(async move {
                let _ = wait(p).await;
                refresh();
            }),
            Some(Err(e)) => fail(e),
            None => fail("no notifications here".into()),
        },
        "folder" => match global("window").map(|w| call(&w, "showDirectoryPicker", &[json_obj(&json!({ "mode": "readwrite" }))])) {
            Some(Ok(p)) => spawn_local(async move {
                match wait(p).await {
                    Ok(handle) => match read_folder(&handle).await {
                        Ok(f) => {
                            let line = format!("folder {}: {} text files read", f.name, f.files.len());
                            with(|c| c.folder = Some(f));
                            refresh();
                            crate::app::note(&line);
                        }
                        Err(e) => crate::app::note(&format!("folder: {e}")),
                    },
                    Err(e) => crate::app::note(&format!("folder: {e}")),
                }
            }),
            Some(Err(e)) => fail(e),
            None => fail("no File System Access API here".into()),
        },
        "camera" | "screen" => {
            let Some(media) = navigator().and_then(|n| get(&n, "mediaDevices")) else { return fail("no media devices here".into()) };
            let (method, opts) =
                if id == "camera" { ("getUserMedia", json!({ "video": true })) } else { ("getDisplayMedia", json!({ "video": true })) };
            match call(&media, method, &[json_obj(&opts)]) {
                Ok(p) => {
                    let id = id.to_string();
                    spawn_local(async move {
                        match wait(p).await {
                            Ok(stream) => {
                                with(|c| if id == "camera" { c.camera = Some(stream) } else { c.screen = Some(stream) });
                                refresh();
                            }
                            Err(e) => crate::app::note(&format!("{id}: {e}")),
                        }
                    })
                }
                Err(e) => fail(e),
            }
        }
        other => fail(format!("{other} has nothing to grant")),
    }
}

fn json_obj(v: &Value) -> JsValue {
    js_sys::JSON::parse(&v.to_string()).unwrap_or_else(|_| Object::new().into())
}

// ---- the folder: the File System Access API as the file capability ----

async fn read_folder(handle: &JsValue) -> Result<Folder, String> {
    let name = get(handle, "name").and_then(|n| n.as_string()).unwrap_or_else(|| "folder".into());
    let mut f = Folder { name, handle: handle.clone(), files: BTreeMap::new(), other: BTreeMap::new() };
    let mut bytes = 0usize;
    let mut queue: Vec<(String, JsValue)> = vec![(String::new(), handle.clone())];
    while let Some((rel, dir)) = queue.pop() {
        let iter = call(&dir, "values", &[])?;
        loop {
            let next = wait(call(&iter, "next", &[])?).await?;
            if get(&next, "done").and_then(|d| d.as_bool()) == Some(true) {
                break;
            }
            let Some(entry) = get(&next, "value") else { break };
            let ename = get(&entry, "name").and_then(|n| n.as_string()).unwrap_or_default();
            let path = if rel.is_empty() { ename.clone() } else { format!("{rel}/{ename}") };
            if dry_core::workbench::HIDDEN.contains(&ename.as_str()) || dry_core::file::secret_file(&path) {
                continue;
            }
            match get(&entry, "kind").and_then(|k| k.as_string()).as_deref() {
                Some("directory") => queue.push((path, entry)),
                Some("file") if f.files.len() < FOLDER_FILES => {
                    let file = wait(call(&entry, "getFile", &[])?).await?;
                    let size = get(&file, "size").and_then(|s| s.as_f64()).unwrap_or(0.0) as u64;
                    if size as usize > dry_core::workbench::MAX_OPEN || bytes + size as usize > FOLDER_BYTES {
                        f.other.insert(path, size);
                        continue;
                    }
                    let text = wait(call(&file, "text", &[])?).await?.as_string().unwrap_or_default();
                    if text.contains('\0') {
                        f.other.insert(path, size);
                        continue;
                    }
                    bytes += text.len();
                    f.files.insert(path, text);
                }
                _ => {}
            }
        }
    }
    Ok(f)
}

/// The granted folder's files (`Host::list` for `root: folder`).
pub fn folder_list(max: usize) -> Result<Vec<String>, String> {
    with(|c| c.folder.as_ref().map(|f| f.files.keys().chain(f.other.keys()).take(max).cloned().collect()))
        .ok_or_else(|| "no folder granted: Settings → Capabilities → folder → Grant".into())
}

/// One file of the granted folder (`Host::read`).
pub fn folder_read(path: &str) -> dry_core::live::Content {
    use dry_core::live::Content;
    with(|c| match &c.folder {
        None => Content::Missing("no folder granted: Settings → Capabilities → folder → Grant".into()),
        Some(f) => match (f.files.get(path), f.other.get(path)) {
            (Some(t), _) => Content::Text(t.clone()),
            (None, Some(n)) => Content::TooLarge(*n),
            _ => Content::Missing(format!("{path} is not in {}", f.name)),
        },
    })
}

/// Write one file of the granted folder: kept here at once, written to
/// the disk right after (the API is asynchronous; a failure is a toast).
pub fn folder_write(path: &str, text: &str) -> Result<(), String> {
    let handle = with(|c| {
        let f = c.folder.as_mut()?;
        f.files.insert(path.to_string(), text.to_string());
        Some(f.handle.clone())
    })
    .ok_or("no folder granted: Settings → Capabilities → folder → Grant")?;
    let (path, text) = (path.to_string(), text.to_string());
    spawn_local(async move {
        let write = async {
            let mut dir = handle;
            let parts: Vec<&str> = path.split('/').collect();
            for p in &parts[..parts.len() - 1] {
                dir = wait(call(&dir, "getDirectoryHandle", &[JsValue::from_str(p), json_obj(&json!({ "create": true }))])?).await?;
            }
            let fh = wait(call(&dir, "getFileHandle", &[JsValue::from_str(parts[parts.len() - 1]), json_obj(&json!({ "create": true }))])?)
                .await?;
            let w = wait(call(&fh, "createWritable", &[])?).await?;
            wait(call(&w, "write", &[JsValue::from_str(&text)])?).await?;
            wait(call(&w, "close", &[])?).await?;
            Ok::<(), String>(())
        };
        if let Err(e) = write.await {
            crate::app::note(&format!("could not write {path} to the folder: {e}"));
        }
    });
    Ok(())
}

// ---- calls to the page's own capabilities, and to the companion's ----

/// An outside call: the page's own capability (notify, model.local,
/// camera, screen), or the companion's over MCP. `None`: no capability
/// has that tool.
pub async fn route(c: &Call) -> Option<Result<String, String>> {
    let r = with(|x| x.registry.route(c))?;
    Some(match &r.provider {
        Provider::Compiled => own(&r).await,
        Provider::Remote(_) => match with(|x| x.peer.clone()) {
            Some((url, token)) => {
                rpc(&url, &token, "tools/call", mcp::call(0, &r.tool, &r.args)["params"].clone()).await.and_then(|a| mcp::call_result(&a))
            }
            None => Err("the companion is not joined".into()),
        },
        Provider::Declared => Err("the browser declares nothing".into()),
    })
}

/// The same for a script's steps (synchronous): only what needs no wait.
pub fn route_sync(c: &Call) -> Option<Result<String, String>> {
    let r = with(|x| x.registry.route(c))?;
    Some(match (r.capability.as_str(), r.tool.as_str()) {
        ("notify", "send") => notify(&r),
        _ => Err(format!("{} waits for the browser: call it on its own, not in a script", c.name)),
    })
}

fn arg<'a>(r: &'a Routed, k: &str) -> &'a str {
    r.args.iter().find(|(n, _)| n == k).map_or("", |(_, v)| v.as_str())
}

fn notify(r: &Routed) -> Result<String, String> {
    let ctor: Function = global("Notification").ok_or("no notifications here")?.dyn_into().map_err(|_| "no notifications here")?;
    let opts = json_obj(&json!({ "body": arg(r, "body") }));
    Reflect::construct(&ctor, &Array::of2(&JsValue::from_str(arg(r, "title")), &opts)).map_err(js_err)?;
    Ok(format!("notified: {}", arg(r, "title")))
}

async fn frame(stream: Option<JsValue>, what: &str) -> Result<String, String> {
    let stream = stream.ok_or_else(|| format!("no {what}: Grant it first"))?;
    let track = call(&stream, "getVideoTracks", &[])?;
    let track = Reflect::get(&track, &0.into()).map_err(js_err)?;
    let s = call(&track, "getSettings", &[])?;
    let n = |k: &str| get(&s, k).and_then(|v| v.as_f64()).unwrap_or(0.0) as u32;
    Ok(format!("{what} frame {}x{} taken (the image waits for vision models)", n("width"), n("height")))
}

async fn own(r: &Routed) -> Result<String, String> {
    match (r.capability.as_str(), r.tool.as_str()) {
        ("notify", "send") => notify(r),
        ("camera", "snap") => frame(with(|c| c.camera.clone()), "camera").await,
        ("screen", "frame") => frame(with(|c| c.screen.clone()), "screen").await,
        ("model.local", "ask") => {
            let lm = global("LanguageModel").ok_or("no Prompt API here")?;
            let session = wait(call(&lm, "create", &[])?).await?;
            let answer = wait(call(&session, "prompt", &[JsValue::from_str(arg(r, "text"))])?).await?;
            let _ = call(&session, "destroy", &[]);
            answer.as_string().ok_or_else(|| "the model gave no text".into())
        }
        (c, t) => Err(format!("{c} has no tool {t} here")),
    }
}

// ---- the companion joins: the handshake over MCP ----

async fn rpc(url: &str, token: &str, method: &str, params: Value) -> Result<Value, String> {
    post(url, token, &mcp::request(1, method, params)).await
}

async fn post(url: &str, token: &str, msg: &Value) -> Result<Value, String> {
    let text =
        crate::companion::request("POST", &format!("{url}/mcp"), token, Some(&msg.to_string()), "application/json, text/event-stream")
            .await?;
    serde_json::from_str(&text).map_err(|_| format!("the companion answered: {text}"))
}

/// This page's hello.
fn hello() -> Hello {
    let platform = navigator().and_then(|n| get(&n, "userAgent")).and_then(|u| u.as_string()).unwrap_or_default();
    Hello { runtime: "dry-web".into(), platform, version: env!("CARGO_PKG_VERSION").into(), protocol: mcp::PROTOCOL.into() }
}

/// The companion connected (`Some(url, token)`) or left (`None`).
pub fn companion(conn: Option<(String, String)>) {
    match conn {
        None => {
            let had = with(|c| c.peer.take().is_some());
            if had {
                with(|c| c.registry.disconnect("companion", "the pairing ended"));
                publish();
            }
        }
        Some((url, token)) => {
            if with(|c| c.peer.as_ref().is_some_and(|(u, t)| *u == url && *t == token)) {
                return;
            }
            spawn_local(async move {
                match join(&url, &token).await {
                    Ok(n) => {
                        with(|c| {
                            c.peer = Some((url.clone(), token.clone()));
                            c.beat = js_sys::Date::now();
                        });
                        publish();
                        crate::app::note(&format!("the companion joined: {n} capabilities"));
                        listen(url, token);
                    }
                    Err(e) => crate::app::note(&format!("the companion's capabilities did not register: {e}")),
                }
            });
        }
    }
}

async fn join(url: &str, token: &str) -> Result<usize, String> {
    let init = post(url, token, &mcp::initialize(1, &hello())).await?;
    let them = mcp::hello_of(mcp::result(&init)?);
    let list = manifest(url, token).await?;
    let n = list.iter().filter(|c| c.present()).count();
    let mut first = Vec::new();
    for c in list.iter().filter(|c| c.present() && c.snapshot.is_some()) {
        if let Ok(t) = read(url, token, &c.id).await {
            first.push((c.id.clone(), t));
        }
    }
    with(|c| {
        c.registry.hello(dry_core::capability::Peer {
            name: "companion".into(),
            runtime: them.runtime,
            platform: them.platform,
            version: them.version,
            protocol: them.protocol,
            url: url.to_string(),
            connected: true,
            seen: crate::host::now(),
        });
        c.registry.register(Provider::Remote("companion".into()), list);
        for (id, t) in first {
            c.registry.snapshot(&id, &Provider::Remote("companion".into()), &t, crate::host::now());
        }
    });
    Ok(n)
}

async fn manifest(url: &str, token: &str) -> Result<Vec<Capability>, String> {
    let tools = rpc(url, token, "tools/list", json!({})).await?;
    let resources = rpc(url, token, "resources/list", json!({})).await?;
    Ok(mcp::manifest("companion", mcp::result(&tools)?, mcp::result(&resources).unwrap_or(&json!({}))))
}

async fn read(url: &str, token: &str, id: &str) -> Result<String, String> {
    mcp::read_result(&rpc(url, token, "resources/read", json!({ "uri": format!("{}{id}", mcp::URI) })).await?)
}

/// The companion's stream of notifications, while it is joined.
fn listen(url: String, token: String) {
    if with(|c| std::mem::replace(&mut c.listening, true)) {
        return;
    }
    spawn_local(async move {
        let on = |note: Note| {
            let (u, k) = (url.clone(), token.clone());
            spawn_local(async move {
                let p = Provider::Remote("companion".into());
                match note {
                    Note::Updated { capability, event } => {
                        let text = read(&u, &k, &capability).await.unwrap_or_else(|e| format!("(could not read it: {e})"));
                        let fired = with(|c| {
                            c.registry.snapshot(&capability, &p, &text, crate::host::now());
                            event.and_then(|(ev, t)| c.registry.remote_event("companion", &capability, &ev, &t))
                        });
                        publish();
                        if let Some(f) = fired {
                            crate::app::capability_event(&f.capability, &f.message());
                        }
                    }
                    Note::ListChanged => {
                        if let Ok(list) = manifest(&u, &k).await {
                            with(|c| c.registry.register(p, list));
                            publish();
                        }
                    }
                    Note::Other => {}
                }
            });
            with(|c| c.peer.as_ref().is_some_and(|(_, t)| *t == token))
        };
        let why = crate::companion::stream_lines(&format!("{url}/mcp"), &token, &mut |data: &str| {
            serde_json::from_str::<Value>(data).map_or(true, |m| on(mcp::note(&m)))
        })
        .await
        .err()
        .unwrap_or_else(|| "its stream ended".into());
        let still = with(|c| {
            c.listening = false;
            c.peer.as_ref().is_some_and(|(u, t)| *u == url && *t == token)
        });
        if still {
            with(|c| {
                c.peer = None;
                c.registry.disconnect("companion", &why);
            });
            publish();
        }
    });
}

/// The tick (every 2 s): a heartbeat every 5 s while the companion is
/// joined; no answer and it leaves.
pub fn tick() {
    let Some((url, token)) = with(|c| c.peer.clone()) else { return };
    let now = js_sys::Date::now();
    if now - with(|c| c.beat) < 5_000.0 {
        return;
    }
    with(|c| c.beat = now);
    spawn_local(async move {
        match rpc(&url, &token, "ping", json!({})).await.and_then(|a| mcp::result(&a).map(|_| ())) {
            Ok(()) => with(|c| c.registry.beat("companion", crate::host::now())),
            Err(why) => {
                with(|c| {
                    c.peer = None;
                    c.registry.disconnect("companion", &format!("no heartbeat: {why}"));
                });
                publish();
            }
        }
    });
}

/// The registry now (tests in the page, the Settings page).
pub fn registry() -> Registry {
    with(|c| c.registry.clone())
}

/// The composer's Voice button: listen once, put the words in the box.
pub fn voice(textarea: web_sys::HtmlTextAreaElement) {
    let Some(ctor) = global("SpeechRecognition").or_else(|| global("webkitSpeechRecognition")) else {
        return crate::app::note("no speech recognition in this browser");
    };
    let Ok(ctor) = ctor.dyn_into::<Function>() else { return };
    let Ok(rec) = Reflect::construct(&ctor, &Array::new()) else { return };
    let _ = Reflect::set(&rec, &"interimResults".into(), &false.into());
    let lang = navigator().and_then(|n| get(&n, "language")).unwrap_or_else(|| "en-US".into());
    let _ = Reflect::set(&rec, &"lang".into(), &lang);
    let ta = textarea.clone();
    let on = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
        let text = get(&ev, "results")
            .and_then(|r| Reflect::get(&r, &0.into()).ok())
            .and_then(|r| Reflect::get(&r, &0.into()).ok())
            .and_then(|a| get(&a, "transcript"))
            .and_then(|t| t.as_string());
        if let Some(t) = text {
            let now = ta.value();
            ta.set_value(&if now.trim().is_empty() { t } else { format!("{} {t}", now.trim_end()) });
        }
    });
    let _ = Reflect::set(&rec, &"onresult".into(), on.as_ref());
    on.forget();
    let err = Closure::<dyn FnMut(JsValue)>::new(|ev: JsValue| {
        let why = get(&ev, "error").and_then(|e| e.as_string()).unwrap_or_else(|| "it stopped".into());
        crate::app::note(&format!("voice: {why}"));
    });
    let _ = Reflect::set(&rec, &"onerror".into(), err.as_ref());
    err.forget();
    match call(&rec, "start", &[]) {
        Ok(_) => crate::app::note("listening… speak, then pause"),
        Err(e) => crate::app::note(&format!("voice: {e}")),
    }
}
