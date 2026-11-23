//! The companion's state, shared by every connection's thread: the folder
//! and its files' places, the pairing code and the pairings (memory
//! only), the switches (saved), the terminal sessions and the event log,
//! the open streams, the providers' and capabilities' probes.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use dry_core::capability::State;
use serde_json::{Value, json};

use super::http::{self, Sink};
use super::rules::{DRY_DIR, Fail, fail};
use super::sys;
use super::term::Session;

pub const NAME: &str = "dry-companion";
/// This implementation, in the hello and `/v1/health` (`impl`).
pub const RUNTIME: &str = "dry-companion-rust";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL: u32 = 1;
/// The event log a page resumes from.
pub const MAX_EVENTS: usize = 1000;
/// Wrong codes before the code changes.
pub const FAILED_PAIRS: u32 = 5;
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// What the command line gave, resolved.
pub struct Setup {
    pub folder: PathBuf,
    pub port: u16,
    pub providers: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub capabilities: Option<PathBuf>,
    pub pair_code_file: Option<PathBuf>,
    pub heartbeat: Duration,
    pub app: Option<PathBuf>,
    pub origins: Vec<String>,
    pub ttl: u64,
    pub store: Option<PathBuf>,
    pub quiet: bool,
    pub verbose: bool,
    pub lmstudio: Option<String>,
    pub ollama: Option<String>,
    pub chrome: Option<String>,
    pub on: Vec<String>,
    pub off: Vec<String>,
}

/// One pairing: a token a page holds.
pub struct Pairing {
    pub token: String,
    pub id: String,
    pub origin: String,
    pub created: i64,
    pub expires: i64,
    pub seen: i64,
    pub hello: Option<Value>,
    pub streams: usize,
}

/// One event of the log: an id a page resumes from.
pub struct Ev {
    pub id: u64,
    pub event: String,
    pub data: Value,
    /// Every stream hears it (a `start`'s exit: a callback).
    pub all: bool,
}

/// An open `/v1/events` stream.
pub struct Listener {
    pub sink: Sink,
    pub pairing: String,
    /// Only this session's events (`?id=`), besides the ones for all.
    pub id: Option<String>,
    pub all: bool,
    pub n: u64,
}

/// An open `GET /mcp` stream: a peer listening.
pub struct McpStream {
    pub sink: Sink,
    pub pairing: String,
    pub n: u64,
}

/// A declared capability's last snapshot.
pub struct Snap {
    pub text: String,
    pub at: Instant,
    pub holds: Vec<bool>,
}

/// A provider's state (docs/companion-protocol.md §8).
pub struct ProviderState {
    pub entry: super::rules::Entry,
    pub state: &'static str,
    pub why: String,
    pub hint: String,
    pub models: Vec<String>,
    pub url: Option<String>,
    pub bin: Option<String>,
    pub version: Option<String>,
    pub runner: Option<(&'static str, &'static str)>,
    /// A native runner this companion started.
    pub child: Option<std::process::Child>,
}

pub struct Companion {
    pub folder: PathBuf,
    pub store_dir: PathBuf,
    pub snapshot_file: PathBuf,
    pub config_file: PathBuf,
    pub providers_file: PathBuf,
    pub cap_dir: Option<PathBuf>,
    pub app_dir: Option<PathBuf>,
    pub pair_code_file: Option<PathBuf>,
    pub heartbeat: Duration,
    pub origins: Vec<String>,
    pub ttl: u64,
    pub quiet: bool,
    pub verbose: bool,
    pub lmstudio: Option<String>,
    pub ollama: Option<String>,
    pub chrome: Option<String>,
    pub port: AtomicU16,
    pub code: Mutex<(String, u32)>,
    pub pairings: Mutex<Vec<Pairing>>,
    pub next_pairing: AtomicU64,
    pub switches: Mutex<BTreeMap<String, Value>>,
    pub sessions: Mutex<Vec<Arc<Session>>>,
    pub next_session: AtomicU64,
    pub events: Mutex<(VecDeque<Ev>, u64)>,
    pub listeners: Mutex<Vec<Listener>>,
    pub mcp_streams: Mutex<Vec<McpStream>>,
    pub next_stream: AtomicU64,
    pub providers: Mutex<BTreeMap<String, ProviderState>>,
    pub probes: Mutex<HashMap<String, (State, Instant)>>,
    pub snaps: Mutex<HashMap<String, Snap>>,
    pub cap_sig: Mutex<Option<String>>,
    pub evidence_runs: AtomicU64,
    pub browser: Mutex<Option<super::browser::Browser>>,
    pub tools_on_path: OnceLock<BTreeMap<&'static str, Option<PathBuf>>>,
}

fn new_code() -> String {
    sys::random(6).iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

fn abs(p: &Path) -> PathBuf {
    let p = PathBuf::from(super::rules::home(&p.to_string_lossy()));
    if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) }
}

impl Companion {
    pub fn new(s: Setup) -> Result<Companion, String> {
        let given = abs(&s.folder);
        if !given.is_dir() {
            return Err(format!("the folder {} is not a folder", given.display()));
        }
        let folder = given.canonicalize().map_err(|e| format!("{}: {e}", given.display()))?;
        if folder.parent().is_none() {
            return Err("the whole disk cannot be the folder: give one folder with --folder".into());
        }
        let dry_dir = folder.join(DRY_DIR);
        let cap_dir = match s.capabilities {
            Some(d) => {
                let d = abs(&d);
                if !d.is_dir() {
                    return Err(format!("--capabilities {}: not a folder", d.display()));
                }
                Some(d.canonicalize().unwrap_or(d))
            }
            None => None,
        };
        let c = Companion {
            store_dir: s.store.map(|p| abs(&p)).unwrap_or_else(|| dry_dir.join("state")),
            snapshot_file: dry_dir.join("snapshot.json"),
            config_file: s.config.map(|p| abs(&p)).unwrap_or_else(|| dry_dir.join("companion.json")),
            providers_file: s.providers.map(|p| abs(&p)).unwrap_or_else(|| dry_dir.join("providers.json")),
            folder,
            cap_dir,
            app_dir: s.app,
            pair_code_file: s.pair_code_file.map(|p| abs(&p)),
            heartbeat: s.heartbeat,
            origins: s.origins.iter().map(|o| o.trim_end_matches('/').to_string()).collect(),
            ttl: s.ttl,
            quiet: s.quiet,
            verbose: s.verbose,
            lmstudio: s.lmstudio.map(|u| u.trim_end_matches('/').to_string()),
            ollama: s.ollama.map(|u| u.trim_end_matches('/').to_string()),
            chrome: s.chrome,
            port: AtomicU16::new(s.port),
            code: Mutex::new((new_code(), 0)),
            pairings: Mutex::default(),
            next_pairing: AtomicU64::new(1),
            switches: Mutex::default(),
            sessions: Mutex::default(),
            next_session: AtomicU64::new(1),
            events: Mutex::new((VecDeque::new(), 1)),
            listeners: Mutex::default(),
            mcp_streams: Mutex::default(),
            next_stream: AtomicU64::new(1),
            providers: Mutex::default(),
            probes: Mutex::default(),
            snaps: Mutex::default(),
            cap_sig: Mutex::new(None),
            evidence_runs: AtomicU64::new(0),
            browser: Mutex::new(None),
            tools_on_path: OnceLock::new(),
        };
        // The switches saved, then the command line's.
        let saved = std::fs::read_to_string(&c.config_file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(Value::Object(m)) = saved.as_ref().and_then(|v| v.get("switches")) {
            c.switches.lock().unwrap().extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        for id in &s.on {
            c.switches.lock().unwrap().insert(id.clone(), json!(true));
        }
        for id in &s.off {
            c.switches.lock().unwrap().insert(id.clone(), json!(false));
        }
        Ok(c)
    }

    pub fn port(&self) -> u16 {
        self.port.load(Ordering::Relaxed)
    }

    /// A line in the companion's terminal: the time, then what happened.
    pub fn log(&self, line: &str) {
        if !self.quiet {
            println!("{}  {line}", sys::clock(sys::now()));
        }
    }

    // ---- pairing ----

    pub fn code(&self) -> String {
        self.code.lock().unwrap().0.clone()
    }

    /// Write the code to `--pair-code-file` and print it.
    pub fn announce_code(&self) {
        let code = self.code();
        if let Some(f) = &self.pair_code_file
            && let Err(e) = std::fs::write(f, format!("{code}\n"))
        {
            eprintln!("could not write {}: {e}", f.display());
        }
        if !self.quiet {
            println!("\n  pairing code: {code}   (type it in dry → Settings → Companion; it works once)\n");
        }
    }

    /// A pairing code sent to `/v1/pair`: a new pairing, or `wrong_code`
    /// (five in a row make a new code).
    pub fn pair(&self, given: &str, origin: &str) -> Result<Value, Fail> {
        let given = given.trim().to_ascii_uppercase();
        {
            let mut code = self.code.lock().unwrap();
            let same = given.len() == code.0.len() && given.bytes().zip(code.0.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
            if !same {
                code.1 += 1;
                let changed = code.1 >= FAILED_PAIRS;
                if changed {
                    *code = (new_code(), 0);
                }
                drop(code);
                if changed {
                    self.log(&format!("{FAILED_PAIRS} wrong codes: the code changed"));
                    self.announce_code();
                }
                return Err(fail(401, "wrong_code", "wrong pairing code: use the one this companion printed"));
            }
            *code = (new_code(), 0);
        }
        let token: String = sys::random(32).iter().map(|b| format!("{b:02x}")).collect();
        let now = sys::now();
        let id = format!("p{}", self.next_pairing.fetch_add(1, Ordering::Relaxed));
        let expires = now + self.ttl as i64;
        self.pairings.lock().unwrap().push(Pairing {
            token: token.clone(),
            id: id.clone(),
            origin: origin.into(),
            created: now,
            expires,
            seen: now,
            hello: None,
            streams: 0,
        });
        self.log(&format!(
            "connected: {id} from {} until {}",
            if origin.is_empty() { "(no origin: a program)" } else { origin },
            &sys::clock(expires)[..5]
        ));
        self.announce_code();
        Ok(json!({ "token": token, "id": id, "expires": expires, "ttl": self.ttl, "name": NAME, "version": VERSION, "protocol": PROTOCOL }))
    }

    /// The pairing a token is, while it has not expired: its id.
    pub fn pairing_of(&self, token: Option<&str>) -> Option<String> {
        let token = token?;
        let now = sys::now();
        let mut list = self.pairings.lock().unwrap();
        let i = list.iter().position(|p| p.token == token)?;
        if now >= list[i].expires {
            let p = list.remove(i);
            drop(list);
            self.log(&format!("pairing {} expired", p.id));
            self.close_streams(&p.id);
            return None;
        }
        list[i].seen = now;
        Some(list[i].id.clone())
    }

    pub fn list_pairings(&self, current: Option<&str>) -> Vec<Value> {
        let now = sys::now();
        self.pairings
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.expires > now)
            .map(|p| {
                let mut v = json!({ "id": p.id, "origin": p.origin, "created": p.created, "expires": p.expires, "seen": p.seen,
                    "current": Some(p.id.as_str()) == current, "streams": p.streams });
                if let Some(h) = &p.hello {
                    v["hello"] = h.clone();
                }
                v
            })
            .collect()
    }

    /// End a pairing (revoke or unpair): its streams close.
    pub fn end_pairing(&self, id: &str, why: &str) -> bool {
        let gone = {
            let mut list = self.pairings.lock().unwrap();
            let before = list.len();
            list.retain(|p| p.id != id);
            before != list.len()
        };
        if gone {
            self.close_streams(id);
            self.log(&format!("pairing {id} {why}"));
        }
        gone
    }

    pub fn with_pairing(&self, id: &str, f: impl FnOnce(&mut Pairing)) {
        if let Some(p) = self.pairings.lock().unwrap().iter_mut().find(|p| p.id == id) {
            f(p);
        }
    }

    pub fn pairings_text(&self) -> String {
        let list = self.pairings.lock().unwrap();
        if list.is_empty() {
            return "  no pairings".into();
        }
        let mut out: Vec<String> = list
            .iter()
            .map(|p| {
                let hello = p.hello.as_ref().map_or(String::new(), |h| {
                    format!(
                        " [{} {}{}]",
                        h["runtime"].as_str().unwrap_or("?"),
                        h["platform"].as_str().unwrap_or(""),
                        if p.streams > 0 { ", listening" } else { "" }
                    )
                });
                let origin = if p.origin.is_empty() { "(no origin)" } else { &p.origin };
                format!("  {}{hello}  {origin}  since {}  until {}", p.id, sys::clock(p.created), sys::clock(p.expires))
            })
            .collect();
        out.push("  (type: revoke <id>)".into());
        out.join("\n")
    }

    /// A pairing that ended: its open streams end too (the peer sees it leave).
    pub fn close_streams(&self, pairing: &str) {
        let shut = |sink: &Sink| {
            if let Ok(s) = sink.lock() {
                let _ = s.shutdown(Shutdown::Both);
            }
        };
        self.listeners.lock().unwrap().retain(|l| {
            let keep = l.pairing != pairing;
            if !keep {
                shut(&l.sink);
            }
            keep
        });
        self.mcp_streams.lock().unwrap().retain(|m| {
            let keep = m.pairing != pairing;
            if !keep {
                shut(&m.sink);
            }
            keep
        });
    }

    // ---- switches ----

    /// An actuator acts on the world: the terminal, and a declared
    /// capability with an `approval: yes` tool. Off until switched on.
    pub fn actuator(&self, id: &str) -> bool {
        id == "terminal" || super::caps::declared(self).0.iter().any(|d| d.id == id && !d.actuators().is_empty())
    }

    pub fn is_on(&self, id: &str) -> bool {
        match self.switches.lock().unwrap().get(id).and_then(Value::as_bool) {
            Some(b) => b,
            None => !self.actuator(id),
        }
    }

    pub fn saved(&self, id: &str) -> bool {
        self.switches.lock().unwrap().get(id).is_some_and(Value::is_boolean)
    }

    /// A capability's calls go only while it is on: else `403 capability_off`.
    pub fn need(&self, id: &str) -> Result<(), Fail> {
        if self.is_on(id) {
            Ok(())
        } else {
            Err(fail(403, "capability_off", format!("{id} is switched off in the companion's terminal (type: on {id})")))
        }
    }

    pub fn save_switches(&self) {
        let v = json!({ "version": 1, "switches": Value::Object(self.switches.lock().unwrap().clone().into_iter().collect()) });
        if let Err(e) = write_atomic(&self.config_file, &(serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")) {
            eprintln!("could not save the switches in {}: {e}", self.config_file.display());
        }
    }

    pub fn set_switch(&self, id: &str, on: bool) {
        self.switches.lock().unwrap().insert(id.into(), json!(on));
        self.save_switches();
        self.log(&format!("{id} switched {} (saved in {})", if on { "on" } else { "off" }, self.config_file.display()));
        self.notify("notifications/tools/list_changed", json!({}));
    }

    // ---- the event log and the streams ----

    /// Record an event and send it to the streams that want it.
    pub fn emit(&self, event: &str, data: Value, all: bool) {
        let mut ev = self.events.lock().unwrap();
        let id = ev.1;
        ev.1 += 1;
        let e = Ev { id, event: event.into(), data, all };
        let text = sse(&e);
        self.listeners.lock().unwrap().retain(|l| !wants(l, &e) || http::send(&l.sink, &text));
        ev.0.push_back(e);
        if ev.0.len() > MAX_EVENTS {
            ev.0.pop_front();
        }
    }

    /// An event only for a session's own streams (`?id=`), not logged.
    pub fn output(&self, id: &str, data: Value) {
        let text = format!("event: output\ndata: {data}\n\n");
        self.listeners.lock().unwrap().retain(|l| l.id.as_deref() != Some(id) || http::send(&l.sink, &text));
    }

    /// A notification to every peer's `/mcp` stream.
    pub fn notify(&self, method: &str, params: Value) {
        let text = format!("data: {}\n\n", json!({ "jsonrpc": "2.0", "method": method, "params": params }));
        self.mcp_streams.lock().unwrap().retain(|m| http::send(&m.sink, &text));
    }

    /// A `: ping` on every open stream (each heartbeat).
    pub fn beat(&self) {
        self.listeners.lock().unwrap().retain(|l| http::send(&l.sink, ": ping\n\n"));
        self.mcp_streams.lock().unwrap().retain(|m| http::send(&m.sink, ": ping\n\n"));
    }

    // ---- this machine's programs ----

    /// git, docker, podman and nvidia-smi on PATH (looked for once).
    pub fn program(&self, name: &str) -> Option<PathBuf> {
        self.tools_on_path
            .get_or_init(|| ["git", "docker", "podman", "nvidia-smi"].into_iter().map(|n| (n, crate::cli::which(n))).collect())
            .get(name)
            .cloned()
            .flatten()
    }
}

/// Whether a `/v1/events` stream hears `e`.
pub fn wants(l: &Listener, e: &Ev) -> bool {
    e.all || l.all || (l.id.is_some() && l.id.as_deref() == e.data.get("id").and_then(Value::as_str))
}

/// An event as server-sent text.
pub fn sse(e: &Ev) -> String {
    format!("id: {}\nevent: {}\ndata: {}\n\n", e.id, e.event, e.data)
}

/// Write a file whole: a temporary file beside it, then a rename.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.dry-{}.tmp", sys::random(4).iter().map(|b| format!("{b:02x}")).collect::<String>()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
