//! The host end to end: the real `dry-server` binary on a free port, on a
//! scratch root (`DRY_ROOT`) with one engine whose model is a fake model
//! server in this test (an OpenAI-compatible `/v1/chat/completions` that
//! answers from a list). A send is taken (202), the turn runs on the
//! server, its events come over `/events`, the reply is in the store;
//! sends to a busy engine or against a stale view are refused (409), an
//! ask is answered once. Nothing of the checkout's own state is touched.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dry_core::inference::OpenAi;
use dry_core::response::ResponseFormat as _;
use dry_core::state::State;
use dry_tools::store::{MdStore, StateStore as Store};
use serde_json::{Value, json};

// ---- a fake model ----

/// Answers each chat call with the next text, after `delay`.
#[derive(Clone)]
struct Model {
    url: String,
    answers: Arc<Mutex<Vec<String>>>,
    delay: Arc<Mutex<Duration>>,
    calls: Arc<Mutex<usize>>,
}

impl Model {
    fn start(answers: &[&str]) -> Model {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let m = Model {
            url,
            answers: Arc::new(Mutex::new(answers.iter().rev().map(|s| s.to_string()).collect())),
            delay: Arc::new(Mutex::new(Duration::ZERO)),
            calls: Arc::new(Mutex::new(0)),
        };
        let me = m.clone();
        std::thread::spawn(move || {
            for tcp in listener.incoming().flatten() {
                let me = me.clone();
                std::thread::spawn(move || me.answer(tcp));
            }
        });
        m
    }

    fn answer(&self, mut tcp: TcpStream) {
        let mut reader = BufReader::new(tcp.try_clone().unwrap());
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let l = line.trim_end().to_ascii_lowercase();
            if let Some(v) = l.strip_prefix("content-length:") {
                length = v.trim().parse().unwrap_or(0);
            }
            if l.is_empty() {
                break;
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        let delay = *self.delay.lock().unwrap();
        std::thread::sleep(delay);
        *self.calls.lock().unwrap() += 1;
        let text = self.answers.lock().unwrap().pop().unwrap_or_else(|| "thought: out of answers\nact: reply(text=\"(no more)\")".into());
        let out = json!({ "choices": [{ "message": { "content": text } }] }).to_string();
        let _ = write!(tcp, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
    }
}

// ---- the server ----

struct Server {
    child: Child,
    port: u16,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

const CONTRACT: &str = "format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n";

/// One engine on `model`, with the reply tool, and a second one.
fn state(model: &Model) -> State {
    let mut s = State::default();
    for _ in 0..2 {
        let i = s.add_engine();
        let e = &mut s.engines[i];
        e.inference = OpenAi { base_url: model.url.clone(), model: "fake".into(), context: Some(32_000), ..OpenAi::default() };
        e.response = dry_core::response::Responses::from_md(CONTRACT).unwrap().0;
        e.tools = vec!["read".into(), "write".into(), "reply".into()];
    }
    s
}

fn start(name: &str, model: &Model) -> Server {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("dry-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["routes", "components", "themes", "scenes", "assets/fonts"] {
        copy_dir(&repo.join(dir), &root.join(dir));
    }
    for file in ["models.json", "sessions.json", "responses.md"] {
        std::fs::copy(repo.join(file), root.join(file)).unwrap();
    }
    MdStore { root: root.join("state") }.save(&state(model)).unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let child = Command::new(env!("CARGO_BIN_EXE_dry-server"))
        .args(["--port", &port.to_string()])
        .env("DRY_ROOT", &root)
        .env_remove("DRY_STORE")
        .env_remove("DRY_SERVER_PORT")
        .env_remove("DRY_SERVER_BIND")
        .env_remove("DRY_SERVER_TOKEN")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server { child, port, root };
    let until = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < until, "the server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    server
}

/// One request; the status and the body.
fn call(s: &Server, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    let mut tcp = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: {}\r\nConnection: close\r\n", s.port, body.len());
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    write!(tcp, "{head}\r\n{body}").unwrap();
    let mut raw = Vec::new();
    tcp.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    (head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0), body.to_string())
}

fn post(s: &Server, path: &str, body: Value) -> (u16, Value) {
    let (status, text) = call(s, "POST", path, &[("Content-Type", "application/json")], &body.to_string());
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

fn status(s: &Server) -> Value {
    let (_, body) = call(s, "GET", "/api/status", &[], "");
    serde_json::from_str(&body).unwrap()
}

/// Wait until no engine is in a turn.
fn settled(s: &Server) {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let v = status(s);
        if v["engines"].as_array().unwrap().iter().all(|e| e["busy"] == false) {
            return;
        }
        assert!(Instant::now() < until, "the turn did not end: {v}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn load(s: &Server) -> State {
    MdStore { root: s.root.join("state") }.load().unwrap()
}

/// An `/events` stream, resuming after `last` if given.
fn events(s: &Server, last: Option<u64>) -> BufReader<TcpStream> {
    let mut tcp = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let resume = last.map(|l| format!("Last-Event-ID: {l}\r\n")).unwrap_or_default();
    write!(tcp, "GET /events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n{resume}\r\n").unwrap();
    BufReader::new(tcp)
}

/// Events (id, name, data) until one named `until` whose data has `with`.
fn read_until(lines: &mut impl BufRead, until: &str, with: &str, within: Duration) -> Vec<(Option<u64>, String, String)> {
    let end = Instant::now() + within;
    let (mut out, mut id, mut name) = (Vec::new(), None, String::new());
    while Instant::now() < end {
        let mut line = String::new();
        match lines.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => break,
        }
        let line = line.trim_end();
        if let Some(v) = line.strip_prefix("id: ") {
            id = v.parse().ok();
        } else if let Some(v) = line.strip_prefix("event: ") {
            name = v.to_string();
        } else if let Some(data) = line.strip_prefix("data: ") {
            out.push((id.take(), name.clone(), data.to_string()));
            if name == until && data.contains(with) {
                return out;
            }
        }
    }
    panic!("no {until} event with {with} in {out:?}");
}

const REPLY: &str = "thought: done\nact: reply(text=\"Hello from the host.\")";
const ASKS: &str = "thought: check\nact: reply(text=\"Shall I go on?\", ask=\"confirm\")";

#[test]
fn a_send_runs_its_turn_on_the_host_and_every_step_is_an_event() {
    let model = Model::start(&["thought: look\nact: write(name=\"plan\", text=\"from the host\")", REPLY]);
    let s = start("send", &model);
    let mut stream = events(&s, None);
    let hello = read_until(&mut stream, "hello", "gen", Duration::from_secs(10));
    assert!(hello[0].0.is_none(), "hello has no id: it is not replayed");

    let (code, v) = post(&s, "/api/engines/0/messages", json!({ "text": "Write the plan, then say hi.", "client": "test", "base": 0 }));
    assert_eq!(code, 202, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(v["turn"], format!("{id}/0"));
    let after_send = v["gen"].as_u64().unwrap();
    assert!(after_send >= 1);

    let seen = read_until(&mut stream, "turn", "\"ready\"", Duration::from_secs(20));
    let names: Vec<&str> = seen.iter().map(|(_, n, _)| n.as_str()).collect();
    assert!(seen.iter().all(|(id, _, _)| id.is_some()), "every turn event has an id: {seen:?}");
    assert!(seen.iter().any(|(_, n, d)| n == "activity" && d.contains("\"kind\":\"llm\"")), "{names:?}");
    // The write is an instant call: it runs inside the move, and the next
    // thing seen is the model again (a waiting call, `run`, shows `tools`).
    assert!(seen.iter().any(|(_, n, d)| n == "activity" && d.contains("step 2")), "the write ran, then the model again: {seen:?}");
    assert!(seen.iter().any(|(_, n, d)| n == "message" && d.contains("\"role\":\"user\"")), "{names:?}");
    assert!(seen.iter().any(|(_, n, d)| n == "message" && d.contains("\"role\":\"assistant\"")), "{names:?}");
    assert!(names.contains(&"change"));
    let ids: Vec<u64> = seen.iter().filter_map(|(id, _, _)| *id).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "ids rise: {ids:?}");

    // Written to the store by the host, and drawn on the page.
    let st = load(&s);
    let e = &st.engines[0];
    assert_eq!(e.artifacts.get("plan").map(String::as_str), Some("from the host"), "the file tool wrote through the host");
    assert!(e.messages.last().unwrap().text.contains("Hello from the host."));
    let (_, page) = call(&s, "GET", "/desk/0", &[], "");
    assert!(page.contains("Hello from the host.") && page.contains("data-composer") && !page.contains("<textarea disabled"), "the composer is live");
    assert!(page.contains(">host<"), "the header says this server is the host");

    // The lock beside the store carries the generation.
    let lock = dry_tools::host::read(&s.root.join("state.host.json")).expect("the host lock");
    assert_eq!(lock.url, format!("http://127.0.0.1:{}", s.port));
    assert_eq!(lock.generation, status(&s)["gen"].as_u64().unwrap());

    // A client that drops resumes after the last id it saw.
    let (from, _, _) = seen[1].clone();
    let mut again = events(&s, from);
    let replay = read_until(&mut again, "turn", "\"ready\"", Duration::from_secs(10));
    assert_eq!(replay[0].1, "hello");
    assert_eq!(replay[1].0, seen[2].0, "replayed from the next one");
}

#[test]
fn a_busy_engine_and_a_stale_view_are_refused() {
    let model = Model::start(&[REPLY, REPLY, REPLY]);
    *model.delay.lock().unwrap() = Duration::from_millis(1500);
    let s = start("busy", &model);
    let (code, first) = post(&s, "/api/engines/0/messages", json!({ "text": "one", "base": 0 }));
    assert_eq!(code, 202, "{first}");
    let (code, busy) = post(&s, "/api/engines/0/messages", json!({ "text": "two", "base": first["gen"] }));
    assert_eq!(code, 409);
    assert_eq!(busy["error"], "engine is in a turn");
    assert!(busy["gen"].is_u64());
    // The other engine is free.
    let (code, _) = post(&s, "/api/engines/1/messages", json!({ "text": "hi", "base": 0 }));
    assert_eq!(code, 202);
    settled(&s);
    *model.delay.lock().unwrap() = Duration::ZERO;

    // A view drawn before the reply: stale for engine 0.
    let (code, stale) = post(&s, "/api/engines/0/messages", json!({ "text": "three", "base": first["gen"] }));
    assert_eq!(code, 409, "{stale}");
    assert!(stale["error"].as_str().unwrap().starts_with("stale"), "{stale}");
    let now = status(&s)["gen"].clone();
    let (code, _) = post(&s, "/api/engines/0/messages", json!({ "text": "three", "base": now }));
    assert_eq!(code, 202);
    settled(&s);
    assert_eq!(load(&s).engines[0].messages.len(), 4, "one, its reply, three, its reply");

    // Writes are JSON from this server's pages.
    let (code, _) = call(&s, "POST", "/api/engines/0/messages", &[("Content-Type", "text/plain")], "{\"text\":\"x\"}");
    assert_eq!(code, 415);
    let (code, _) = call(&s, "POST", "/api/engines/0/messages", &[("Content-Type", "application/json"), ("Origin", "http://evil.example")], "{\"text\":\"x\"}");
    assert_eq!(code, 403);
    let (code, _) = post(&s, "/api/engines/7/messages", json!({ "text": "x" }));
    assert_eq!(code, 404);
}

#[test]
fn an_ask_is_answered_once_with_the_core_s_text() {
    let model = Model::start(&[ASKS, REPLY, ASKS, REPLY]);
    let s = start("answers", &model);
    let (code, _) = post(&s, "/api/engines/0/messages", json!({ "text": "Start." }));
    assert_eq!(code, 202);
    settled(&s);
    let (_, page) = call(&s, "GET", "/desk/0", &[], "");
    assert!(page.contains("data-ask=\"confirm\"") && page.contains("data-act=\"ask\""), "the ask is live on the page");

    // What the page gathered: the host writes the answer.
    let (code, v) = post(&s, "/api/engines/0/answers", json!({ "message": 1, "pick": { "input": { "picked": ["yes"] } }, "base": status(&s)["gen"] }));
    assert_eq!(code, 202, "{v}");
    settled(&s);
    let e = &load(&s).engines[0];
    assert_eq!(e.messages[2].text, "Answer (confirm): yes");
    assert!(e.messages[3].text.contains("Hello from the host."));
    let (code, again) = post(&s, "/api/engines/0/answers", json!({ "message": 1, "answer": "Answer (confirm): no" }));
    assert_eq!(code, 409);
    assert_eq!(again["error"], "already answered");

    // The window's way: through the client, the answer as text, the engine by id.
    let (code, _) = post(&s, "/api/engines/0/messages", json!({ "text": "Again." }));
    assert_eq!(code, 202);
    settled(&s);
    let lock = dry_tools::host::current_at(&s.root.join("state.host.json")).expect("a live host");
    let id = load(&s).engines[0].id.clone();
    let sent = dry_tools::host::answer(&lock, "window", &id, 5, "Answer (confirm): no", Some(lock.generation)).unwrap();
    assert!(matches!(sent, dry_tools::host::Sent::Accepted { .. }), "{sent:?}");
    settled(&s);
    assert_eq!(load(&s).engines[0].messages[6].text, "Answer (confirm): no");
    let busy_or_done = dry_tools::host::send(&lock, "window", &id, "  ", None).unwrap();
    assert!(matches!(busy_or_done, dry_tools::host::Sent::Refused { status: 400, .. }), "{busy_or_done:?}");
}

#[test]
fn new_and_deleted_engines_the_format_and_the_mode_go_through_the_host() {
    let model = Model::start(&[]);
    let s = start("writes", &model);
    let (code, made) = post(&s, "/api/engines", json!({}));
    assert_eq!(code, 201, "{made}");
    assert_eq!(made["index"], 2);
    assert_eq!(load(&s).engines.len(), 3);
    let (code, _) = post(&s, "/api/engines/2/format", json!({ "value": "json" }));
    assert_eq!(code, 200);
    assert_eq!(load(&s).engines[2].format.name(), "json");
    let (code, _) = post(&s, "/api/mode", json!({ "value": "light" }));
    assert_eq!(code, 200);
    assert_eq!(load(&s).mode.as_str(), "light");
    // By index against a view from before the new engine: refused.
    let (code, v) = post(&s, "/api/engines/2/delete", json!({ "base": 0 }));
    assert_eq!(code, 409, "{v}");
    let (code, _) = post(&s, "/api/engines/2/delete", json!({ "base": status(&s)["gen"] }));
    assert_eq!(code, 200);
    assert_eq!(load(&s).engines.len(), 2);
    // A composer on a page with no engine makes one.
    let (code, v) = post(&s, "/api/engines/new/messages", json!({ "text": "hello" }));
    assert_eq!(code, 202, "{v}");
    assert_eq!(v["index"], 2);
}

#[test]
fn a_wide_bind_needs_a_token() {
    let out = Command::new(env!("CARGO_BIN_EXE_dry-server"))
        .args(["--port", "1", "--bind", "0.0.0.0"])
        .env_remove("DRY_SERVER_TOKEN")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--token"));
}

#[test]
fn reset_history_needs_its_confirm_waits_for_turns_and_moves_the_store_aside() {
    let model = Model::start(&[REPLY]);
    *model.delay.lock().unwrap() = Duration::from_millis(1500);
    let s = start("reset", &model);
    std::fs::write(s.root.join("state/local.md"), "## Scene\n").unwrap();
    // Settings: Reset history is the first thing on the page, a confirm step first.
    let (_, page) = call(&s, "GET", "/settings", &[], "");
    let at = page.find("Reset history").expect("the control is on Settings");
    assert!(page.find("APPEARANCE").is_none_or(|a| at < a), "it comes first");
    assert!(page.contains("data-key=\"reset-history\"") && page.contains("data-act=\"reset-history\""));

    // No confirm: refused, nothing moved.
    for body in [json!({}), json!({ "confirm": false }), json!({ "confirm": "yes" })] {
        let (code, v) = post(&s, "/api/reset", body);
        assert_eq!(code, 400, "{v}");
    }
    // An engine in a turn: refused, nothing moved.
    let (code, _) = post(&s, "/api/engines/0/messages", json!({ "text": "one" }));
    assert_eq!(code, 202);
    let (code, v) = post(&s, "/api/reset", json!({ "confirm": true }));
    assert_eq!(code, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("in a turn"), "{v}");
    settled(&s);
    assert_eq!(load(&s).engines.len(), 2);

    let before = status(&s)["gen"].as_u64().unwrap();
    let (code, v) = post(&s, "/api/reset", json!({ "confirm": true }));
    assert_eq!(code, 200, "{v}");
    assert!(v["gen"].as_u64().unwrap() > before);
    let backup = PathBuf::from(v["backup"].as_str().unwrap());
    assert!(backup.starts_with(s.root.canonicalize().unwrap()) && backup.file_name().unwrap().to_string_lossy().starts_with("state.reset-"), "{v}");
    assert!(backup.join("app.md").is_file() && backup.join("engines").is_dir(), "moved, not deleted");
    let left: Vec<String> = std::fs::read_dir(s.root.join("state")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(left, vec!["local.md".to_string()], "only this machine's local.md stays");
    let now = status(&s);
    let engines = now["engines"].as_array().unwrap();
    assert_eq!(engines.len(), 1, "{now}");
    assert_eq!(engines[0]["id"], "translator");
    assert_eq!(engines[0]["messages"], 0);
    // Again: nothing of its own to move.
    let (code, v) = post(&s, "/api/reset", json!({ "confirm": true }));
    assert_eq!(code, 200, "{v}");
    assert!(v["backup"].is_null(), "{v}");
}
