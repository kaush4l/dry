//! The host: while a `dry-server` hosts a store, it is that store's one
//! writer. It says so with a lock file beside the store, and every other
//! shell (the GPUI window) reads the lock and becomes a client: it
//! watches the store's generation and sends through the host's API
//! instead of running turns and saving itself (docs/web.md, "The send
//! path").
//!
//! ```text
//! <store>.host.json   {"pid": 123, "url": "http://127.0.0.1:8090", "gen": 7, "store": "md:/…/state"}
//! ```
//!
//! - `md:/…/state` → `/…/state.host.json`; `sqlite:/…/state.db` →
//!   `/…/state.db.host.json`.
//! - `gen` is the store's generation: the host bumps it on every write it
//!   makes and rewrites the lock, so a client reloads exactly when it moves.
//! - A lock whose host does not answer on its port is stale: there is no
//!   host, and the window works as it always did.
//!
//! The client half (`send`, `answer`) is the HTTP the window uses to send
//! through the host; the server's tests use it too.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

/// What the lock file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lock {
    pub pid: u32,
    /// Where the host answers, on this machine (`http://127.0.0.1:<port>`).
    pub url: String,
    /// The store's generation at the host's last write.
    pub generation: u64,
    /// The store it hosts (`md:/…/state`).
    pub store: String,
}

impl Lock {
    pub fn to_json(&self) -> String {
        json!({ "pid": self.pid, "url": self.url, "gen": self.generation, "store": self.store }).to_string()
    }

    pub fn from_json(text: &str) -> Option<Lock> {
        let v: Value = serde_json::from_str(text).ok()?;
        Some(Lock {
            pid: v.get("pid")?.as_u64()? as u32,
            url: v.get("url")?.as_str()?.to_string(),
            generation: v.get("gen")?.as_u64()?,
            store: v.get("store").and_then(Value::as_str).unwrap_or_default().to_string(),
        })
    }

    /// The host's address, for the liveness check.
    fn addr(&self) -> Option<SocketAddr> {
        let rest = self.url.strip_prefix("http://")?;
        let hostport = rest.split('/').next()?;
        hostport.to_socket_addrs().ok()?.next()
    }

    /// Does the host answer on its port?
    pub fn alive(&self) -> bool {
        self.addr().is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
    }
}

/// The lock file for the store `spec` describes (`md:<folder>`,
/// `sqlite:<file>`, as `Store::describe` writes it: an absolute path).
pub fn lock_path(spec: &str) -> PathBuf {
    let p = spec.split_once(':').map_or(spec, |(_, p)| p).trim_end_matches('/');
    let p = if Path::new(p).is_absolute() { PathBuf::from(p) } else { crate::root().join(p) };
    let name = p.file_name().map_or_else(|| "state".to_string(), |n| n.to_string_lossy().to_string());
    p.with_file_name(format!("{name}.host.json"))
}

/// The lock file of the app's store (`DRY_STORE`, else `md:state`).
pub fn default_lock_path() -> Option<PathBuf> {
    crate::store::open_default().ok().map(|s| lock_path(&s.describe()))
}

/// Write the lock (through a temporary file, so a reader never sees half).
pub fn write(path: &Path, lock: &Lock) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, lock.to_json()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The lock at `path`, if there is one that reads.
pub fn read(path: &Path) -> Option<Lock> {
    std::fs::read_to_string(path).ok().and_then(|t| Lock::from_json(&t))
}

/// Remove the lock, if it is this process's.
pub fn release(path: &Path, pid: u32) {
    if read(path).is_some_and(|l| l.pid == pid) {
        let _ = std::fs::remove_file(path);
    }
}

/// The live host of the app's store, if one runs.
pub fn current() -> Option<Lock> {
    default_lock_path().and_then(|p| current_at(&p))
}

/// The live host whose lock is at `path`, if one runs.
pub fn current_at(path: &Path) -> Option<Lock> {
    read(path).filter(Lock::alive)
}

/// What a client does with what the lock says: `Reload` when the host
/// wrote since the client last read (`seen`), `Same` when not, and
/// `Alone` when there is no host (the client runs as it always did).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Watch {
    Alone,
    Same,
    Reload(u64),
}

/// The client's next move, given the lock it found (alive or not) and
/// the generation it last loaded.
pub fn watch(lock: Option<&Lock>, alive: bool, seen: Option<u64>) -> Watch {
    match lock {
        Some(l) if alive => {
            if seen == Some(l.generation) {
                Watch::Same
            } else {
                Watch::Reload(l.generation)
            }
        }
        _ => Watch::Alone,
    }
}

// ---- the client -------------------------------------------------------------------

/// A JSON request to the host: the status and the body (JSON, or
/// `{"error": text}`). `body` `None`: a GET.
pub fn call(lock: &Lock, path: &str, body: Option<&Value>, timeout: Duration) -> Result<(u16, Value), String> {
    let url = format!("{}{path}", lock.url.trim_end_matches('/'));
    let mut resp = match body {
        Some(b) => ureq::post(&url)
            .header("Content-Type", "application/json")
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .send(b.to_string().as_str()),
        None => ureq::get(&url).config().http_status_as_error(false).timeout_global(Some(timeout)).build().call(),
    }
    .map_err(|e| format!("the host did not answer: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "error": text }));
    Ok((status, v))
}

/// `call`, with a 2xx body as the answer and anything else as its error.
pub fn call_ok(lock: &Lock, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Value, String> {
    let (status, v) = call(lock, path, body, timeout)?;
    if (200..300).contains(&status) {
        Ok(v)
    } else {
        Err(v.get("error").and_then(Value::as_str).map_or_else(|| format!("the host said {status}"), str::to_string))
    }
}

/// Reset history through the host (`POST /api/reset {confirm: true}`):
/// where the old store went (`None`: it held nothing of its own), or why not.
pub fn reset(lock: &Lock) -> Result<Option<String>, String> {
    let v = call_ok(lock, "/api/reset", Some(&json!({ "confirm": true })), Duration::from_secs(30))?;
    Ok(v.get("backup").and_then(Value::as_str).map(str::to_string))
}

/// `s` as one path segment or query value.
pub fn escape(s: &str) -> String {
    encode(s)
}

/// What the host said to a send.
#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    /// 202: the turn runs on the host.
    Accepted { turn: String, generation: u64 },
    /// Refused (409: the engine is in a turn, the view was stale, already
    /// answered; 4xx: why), with the host's words.
    Refused { status: u16, error: String, generation: Option<u64> },
}

fn post_json(url: &str, body: &Value) -> Result<Sent, String> {
    let mut resp = ureq::post(url)
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .send(body.to_string().as_str())
        .map_err(|e| format!("the host did not answer: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let generation = v.get("gen").and_then(Value::as_u64);
    if (200..=202).contains(&status) {
        Ok(Sent::Accepted {
            turn: v.get("turn").and_then(Value::as_str).unwrap_or_default().to_string(),
            generation: generation.unwrap_or(0),
        })
    } else {
        let error = v.get("error").and_then(Value::as_str).map_or_else(|| format!("{status}: {text}"), str::to_string);
        Ok(Sent::Refused { status, error, generation })
    }
}

/// `/api/engines/<engine>/…`, the engine by id.
fn engine_url(lock: &Lock, engine: &str, what: &str) -> String {
    format!("{}/api/engines/{}/{what}", lock.url.trim_end_matches('/'), encode(engine))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Send `text` to engine `engine` (its id) through the host. `base`: the
/// generation the client's view is of.
pub fn send(lock: &Lock, client: &str, engine: &str, text: &str, base: Option<u64>) -> Result<Sent, String> {
    post_json(&engine_url(lock, engine, "messages"), &json!({ "text": text, "client": client, "base": base }))
}

/// Answer the ask in engine `engine`'s message `message` with `answer`
/// (the text `dry_core::ask::Ask::answer` makes) through the host.
pub fn answer(lock: &Lock, client: &str, engine: &str, message: usize, answer: &str, base: Option<u64>) -> Result<Sent, String> {
    post_json(&engine_url(lock, engine, "answers"), &json!({ "message": message, "answer": answer, "client": client, "base": base }))
}

/// Your guided post on root `thread`'s thread (`""`: the first) to `to`
/// (`None`: kept, for nobody) through the host (`POST /api/threads/:id/posts`).
pub fn post_thread(lock: &Lock, client: &str, thread: &str, to: Option<&str>, text: &str) -> Result<Sent, String> {
    let thread = if thread.is_empty() { "top" } else { thread };
    post_json(&format!("{}/api/threads/{}/posts", lock.url.trim_end_matches('/'), encode(thread)), &json!({ "text": text, "to": to, "client": client }))
}

/// One engine as the host's `/api/status` says it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub id: String,
    pub busy: bool,
    /// What it is doing, for the tint and the label.
    pub activity: dry_core::activity::Activity,
}

/// What the host's `kind` and `label` (`/api/status`, an `activity`
/// event) come to as an activity: enough for the tint and the words.
pub fn activity_of(kind: &str, label: &str) -> dry_core::activity::Activity {
    use dry_core::activity::Activity;
    match kind {
        "llm" => Activity::Llm { step: if label.contains("again") { 2 } else { 1 } },
        "tools" => Activity::Tools { step: 1, calls: Vec::new() },
        "compress" => Activity::Compressing { step: 1 },
        "wait" => Activity::Waiting(vec![label.trim_start_matches("waiting for ").to_string()]),
        "working" => Activity::Working(label.to_string()),
        "needs-you" => Activity::NeedsYou(label.trim_start_matches("needs you: ").to_string()),
        "failed" => Activity::Failed(label.trim_start_matches("failed: ").to_string()),
        _ => Activity::Idle,
    }
}

/// The engines in a `/api/status` body.
pub fn engines_of(status: &Value) -> Vec<Engine> {
    let Some(list) = status.get("engines").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|e| {
            Some(Engine {
                id: e.get("id")?.as_str()?.to_string(),
                busy: e.get("busy").and_then(Value::as_bool).unwrap_or(false),
                activity: activity_of(
                    e.get("kind").and_then(Value::as_str).unwrap_or("idle"),
                    e.get("label").and_then(Value::as_str).unwrap_or(""),
                ),
            })
        })
        .collect()
}

/// Ask the host what each engine is doing (`GET /api/status`).
pub fn engines(lock: &Lock) -> Result<Vec<Engine>, String> {
    let mut resp = ureq::get(&format!("{}/api/status", lock.url.trim_end_matches('/')))
        .config()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .call()
        .map_err(|e| format!("the host did not answer: {e}"))?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(engines_of(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_s_status_reads_as_activities() {
        use dry_core::activity::Activity;
        let v = json!({ "engines": [
            { "id": "a", "busy": true, "kind": "llm", "label": "calling the LLM again (step 2)" },
            { "id": "b", "busy": false, "kind": "wait", "label": "waiting for helper" },
            { "id": "c", "kind": "idle", "label": "idle" },
            { "busy": true }
        ]});
        let e = engines_of(&v);
        assert_eq!(e.len(), 3, "one without an id is left out");
        assert_eq!(e[0], Engine { id: "a".into(), busy: true, activity: Activity::Llm { step: 2 } });
        assert_eq!(e[1].activity, Activity::Waiting(vec!["helper".into()]));
        assert_eq!(e[2].activity, Activity::Idle);
        assert_eq!(activity_of("failed", "failed: no model"), Activity::Failed("no model".into()));
        assert!(activity_of("compress", "").is_turn());
    }

    #[test]
    fn the_lock_sits_beside_the_store_and_reads_back() {
        assert_eq!(lock_path("md:/a/b/state"), PathBuf::from("/a/b/state.host.json"));
        assert_eq!(lock_path("md:/a/b/state/"), PathBuf::from("/a/b/state.host.json"));
        assert_eq!(lock_path("sqlite:/a/state.db"), PathBuf::from("/a/state.db.host.json"));
        let l = Lock { pid: 7, url: "http://127.0.0.1:1".into(), generation: 3, store: "md:/a/state".into() };
        assert_eq!(Lock::from_json(&l.to_json()), Some(l.clone()));
        assert_eq!(Lock::from_json("{\"pid\":1}"), None, "a lock without its url and gen is no lock");

        let dir = std::env::temp_dir().join(format!("dry-host-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.host.json");
        write(&path, &l).unwrap();
        assert_eq!(read(&path), Some(l.clone()));
        release(&path, 8);
        assert!(path.exists(), "another process's lock is left alone");
        release(&path, 7);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_client_reloads_when_the_generation_moves_and_is_alone_without_a_live_host() {
        let l = Lock { pid: 1, url: "http://127.0.0.1:1".into(), generation: 5, store: String::new() };
        assert_eq!(watch(None, false, None), Watch::Alone);
        assert_eq!(watch(Some(&l), false, Some(5)), Watch::Alone, "a stale lock: no host");
        assert_eq!(watch(Some(&l), true, None), Watch::Reload(5), "first sight of a host: load what it has");
        assert_eq!(watch(Some(&l), true, Some(4)), Watch::Reload(5));
        assert_eq!(watch(Some(&l), true, Some(5)), Watch::Same);
        // Port 1 on loopback has nothing listening.
        assert!(!l.alive());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let up = Lock { url: format!("http://{}", listener.local_addr().unwrap()), ..l };
        assert!(up.alive());
    }

    #[test]
    fn engine_ids_are_escaped_in_the_path() {
        let l = Lock { pid: 1, url: "http://127.0.0.1:9/".into(), generation: 0, store: String::new() };
        assert_eq!(engine_url(&l, "engine 1/x", "messages"), "http://127.0.0.1:9/api/engines/engine%201%2Fx/messages");
    }
}
