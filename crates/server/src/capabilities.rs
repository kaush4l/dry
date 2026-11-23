//! The host's capabilities (docs/capabilities.md): its registry, the
//! declared files of this machine, and the peers that joined (a companion,
//! another device) over MCP. The host is the one place a call to an
//! outside capability is routed from: `route`, which a turn's waiting
//! calls go through like an MCP tool's.
//!
//! - **Local.** At start: the compiled capabilities (probed), and every
//!   `capabilities/<id>.md` probed (`dry_tools::capability::local`).
//! - **A peer joins** (`join`): pair with its code, hello and manifest
//!   (`Peer::handshake`), registered as `remote:<name>`. Then one thread
//!   reads its stream (`GET /mcp`): an update reads the snapshot again and
//!   fires its event; `list_changed` re-registers; the stream closing is
//!   the peer leaving. Another sends the heartbeat (`ping`, every 5 s);
//!   no answer is the peer leaving too. Leaving makes its capabilities
//!   absent: they go from every prompt and page.
//! - **Snapshots** of declared ones are read at their intervals (`tick`);
//!   an event that fires wakes the engines using it with one message.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use dry_core::calls::Call;
use dry_core::capability::declared::Declared;
use dry_core::capability::mcp::Note;
use dry_core::capability::{Fired, Peer as PeerInfo, Provider, Registry};
use dry_tools::capability::{self as local, Peer};
use serde_json::{Value, json};

/// What the host knows of capabilities.
pub struct Caps {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    registry: Registry,
    declared: Vec<Declared>,
    errors: Vec<String>,
    peers: HashMap<String, Arc<Peer>>,
    /// Bumped when a peer leaves, so its threads stop.
    epoch: HashMap<String, u64>,
    /// Events that fired and are not yet delivered.
    fired: Vec<Fired>,
}

impl Default for Caps {
    fn default() -> Caps {
        Caps { inner: Mutex::new(Inner::default()) }
    }
}

fn now() -> i64 {
    dry_tools::live::clock().unix
}

impl Caps {
    /// This machine's: compiled and declared, probed.
    pub fn local() -> Caps {
        let (registry, declared, errors) = local::local();
        for e in &errors {
            eprintln!("capabilities: {e}");
        }
        Caps { inner: Mutex::new(Inner { registry, declared, errors, ..Inner::default() }) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The registry now (what engines are equipped with).
    pub fn registry(&self) -> Registry {
        self.lock().registry.clone()
    }

    /// The subscriptions probed again (`dry_tools::subscriptions`), with the
    /// busy sessions it knew kept. Whether anything changed.
    pub fn set_subscriptions(&self, mut subs: dry_core::subscriptions::Subscriptions) -> bool {
        let mut g = self.lock();
        if let Some(old) = &g.registry.subscriptions {
            let running = old.rows.iter().filter(|r| r.running > 0).map(|r| (r.name.clone(), r.running)).collect();
            subs.set_running(&running);
        }
        let changed = g.registry.subscriptions.as_ref() != Some(&subs);
        g.registry.subscriptions = Some(subs);
        changed
    }

    /// How many session engines of each program are busy now. Whether
    /// that changed what the subscriptions say.
    pub fn set_running(&self, running: &std::collections::BTreeMap<String, usize>) -> bool {
        let mut g = self.lock();
        let Some(subs) = g.registry.subscriptions.as_mut() else { return false };
        let before = subs.clone();
        subs.set_running(running);
        *subs != before
    }

    /// The events that fired since the last call.
    pub fn take_fired(&self) -> Vec<Fired> {
        std::mem::take(&mut self.lock().fired)
    }

    /// Where an outside call goes, and what it came to. `None`: no outside
    /// capability has that tool (another outside tool may).
    pub fn route(&self, call: &Call) -> Option<Result<String, String>> {
        let (routed, declared, peers) = {
            let g = self.lock();
            let r = g.registry.route(call)?;
            (r, g.declared.clone(), g.peers.clone())
        };
        Some(match &routed.provider {
            Provider::Declared => local::call(&declared, &routed),
            Provider::Remote(name) => match peers.get(name) {
                Some(p) => p.call(&routed),
                None => Err(format!("{name} is not connected")),
            },
            Provider::Compiled => Err("a compiled capability runs in core".into()),
        })
    }

    /// The registry as JSON (`GET /api/capabilities`).
    pub fn to_json(&self) -> Value {
        let g = self.lock();
        let caps: Vec<Value> = g
            .registry
            .all()
            .iter()
            .map(|c| {
                json!({
                    "id": c.id, "provider": c.provider.text(), "state": c.state.word(), "why": c.state.why(), "doc": c.doc,
                    "tools": c.tools.iter().map(|t| json!({ "name": c.call_name(t), "approval": t.approval })).collect::<Vec<_>>(),
                    "snapshot": c.snapshot.as_ref().map(|s| s.text.clone()),
                    "effective": g.registry.effective(&c.id).is_some_and(|e| e.provider == c.provider),
                })
            })
            .collect();
        let peers: Vec<Value> = g
            .registry
            .peers
            .iter()
            .map(|p| json!({ "name": p.name, "runtime": p.runtime, "platform": p.platform, "version": p.version, "protocol": p.protocol, "url": p.url, "connected": p.connected, "seen": p.seen }))
            .collect();
        json!({ "capabilities": caps, "peers": peers, "errors": g.errors })
    }

    /// Read the declared snapshots that are due, and the remote ones with
    /// an interval. Whether the registry changed.
    pub fn tick(&self) -> bool {
        let (due, declared, peers) = {
            let g = self.lock();
            (g.registry.due(now()), g.declared.clone(), g.peers.clone())
        };
        if due.is_empty() {
            return false;
        }
        for (id, provider) in due {
            let text = match &provider {
                Provider::Declared => declared.iter().find(|d| d.id == id).map(local::snapshot),
                Provider::Remote(name) => peers.get(name).map(|p| p.read(&id)),
                Provider::Compiled => None,
            };
            let text = match text {
                Some(Ok(t)) => t,
                Some(Err(why)) => format!("(could not read it: {why})"),
                None => continue,
            };
            let mut g = self.lock();
            let fired = g.registry.snapshot(&id, &provider, &text, now());
            g.fired.extend(fired);
        }
        true
    }

    /// Pair with a peer and join it: the handshake, then its stream and
    /// heartbeat on threads. `changed` is called whenever its capabilities
    /// change (joined, updated, re-registered, left).
    pub fn join(self: &Arc<Self>, name: &str, url: &str, code: &str, changed: Arc<dyn Fn() + Send + Sync>) -> Result<Value, String> {
        let name = if name.is_empty() { "companion" } else { name };
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(format!("`{name}`: a peer's name is letters, digits, - and _"));
        }
        let peer = Arc::new(Peer::pair(name, url, code)?);
        self.connect(peer, changed)
    }

    /// Join a peer already paired (its token).
    pub fn connect(self: &Arc<Self>, peer: Arc<Peer>, changed: Arc<dyn Fn() + Send + Sync>) -> Result<Value, String> {
        let (hello, caps) = peer.handshake(&local::hello("dry-server"))?;
        let name = peer.name.clone();
        let epoch = {
            let mut g = self.lock();
            g.registry.hello(PeerInfo {
                name: name.clone(),
                runtime: hello.runtime.clone(),
                platform: hello.platform.clone(),
                version: hello.version.clone(),
                protocol: hello.protocol.clone(),
                url: peer.url.clone(),
                connected: true,
                seen: now(),
            });
            let ch = g.registry.register(Provider::Remote(name.clone()), caps.clone());
            g.peers.insert(name.clone(), peer.clone());
            let e = g.epoch.entry(name.clone()).or_default();
            *e += 1;
            eprintln!("capabilities: {name} joined ({} {}): +{}", hello.runtime, hello.platform, ch.added.join(", "));
            *e
        };
        // Its first snapshots.
        for c in caps.iter().filter(|c| c.present() && c.snapshot.is_some()) {
            if let Ok(t) = peer.read(&c.id) {
                self.lock().registry.snapshot(&c.id, &Provider::Remote(name.clone()), &t, now());
            }
        }
        changed();
        self.listen(peer.clone(), epoch, changed.clone());
        self.heartbeat(peer, epoch, changed);
        Ok(
            json!({ "peer": name, "runtime": hello.runtime, "platform": hello.platform, "capabilities": caps.iter().map(|c| json!({ "id": c.id, "state": c.state.word() })).collect::<Vec<_>>() }),
        )
    }

    fn current(&self, name: &str, epoch: u64) -> bool {
        self.lock().epoch.get(name) == Some(&epoch)
    }

    /// A peer left: its capabilities absent, its threads stopped.
    pub fn leave(&self, name: &str, why: &str) -> bool {
        let mut g = self.lock();
        if g.peers.remove(name).is_none() {
            return false;
        }
        *g.epoch.entry(name.to_string()).or_default() += 1;
        let gone = g.registry.disconnect(name, why);
        eprintln!("capabilities: {name} left ({why}): -{}", gone.join(", "));
        true
    }

    fn listen(self: &Arc<Self>, peer: Arc<Peer>, epoch: u64, changed: Arc<dyn Fn() + Send + Sync>) {
        let caps = self.clone();
        std::thread::spawn(move || {
            let name = peer.name.clone();
            let mut on = |note: Note| {
                if !caps.current(&name, epoch) {
                    return false;
                }
                let provider = Provider::Remote(name.clone());
                match note {
                    Note::Updated { capability, event } => {
                        let text = peer.read(&capability).unwrap_or_else(|why| format!("(could not read it: {why})"));
                        let mut g = caps.lock();
                        g.registry.snapshot(&capability, &provider, &text, now());
                        if let Some((ev, text)) = event
                            && let Some(f) = g.registry.remote_event(&name, &capability, &ev, &text)
                        {
                            g.fired.push(f);
                        }
                    }
                    Note::ListChanged => {
                        if let Ok(list) = peer.manifest() {
                            let ch = caps.lock().registry.register(provider, list);
                            if !ch.is_empty() {
                                eprintln!(
                                    "capabilities: {name} re-registered: +{} ~{} -{}",
                                    ch.added.join(","),
                                    ch.changed.join(","),
                                    ch.removed.join(",")
                                );
                            }
                        }
                    }
                    Note::Other => return true,
                }
                changed();
                true
            };
            let why = match peer.listen(&mut on) {
                Ok(()) => return,
                Err(why) => why,
            };
            if caps.current(&name, epoch) && caps.leave(&name, &why) {
                changed();
            }
        });
    }

    fn heartbeat(self: &Arc<Self>, peer: Arc<Peer>, epoch: u64, changed: Arc<dyn Fn() + Send + Sync>) {
        let caps = self.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(5));
                if !caps.current(&peer.name, epoch) {
                    return;
                }
                match peer.ping() {
                    Ok(()) => caps.lock().registry.beat(&peer.name, now()),
                    Err(why) => {
                        if caps.leave(&peer.name, &format!("no heartbeat: {why}")) {
                            changed();
                        }
                        return;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A peer that speaks the handshake over plain HTTP: pairs, answers
    /// MCP, and streams one event and one list_changed, then closes.
    fn fake_peer() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let version = Arc::new(AtomicUsize::new(0));
        let v = version.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let v = v.clone();
                std::thread::spawn(move || serve(conn, &v));
            }
        });
        (url, version)
    }

    fn serve(mut conn: TcpStream, version: &AtomicUsize) {
        let mut reader = BufReader::new(conn.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let (method, path) = {
            let mut p = line.split_whitespace();
            (p.next().unwrap_or("").to_string(), p.next().unwrap_or("").to_string())
        };
        let mut len = 0;
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).unwrap();
            if h.trim().is_empty() {
                break;
            }
            if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body).unwrap();
        let reply = |conn: &mut TcpStream, v: Value| {
            let b = v.to_string();
            let _ = write!(
                conn,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}",
                b.len()
            );
        };
        if path == "/v1/pair" {
            return reply(&mut conn, json!({ "token": "t0k" }));
        }
        if method == "GET" {
            let _ = write!(conn, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n: hi\n\n");
            std::thread::sleep(Duration::from_millis(300));
            let ev = json!({ "jsonrpc": "2.0", "method": "notifications/resources/updated", "params": { "uri": "dry://capability/battery", "_meta": { "event": { "name": "unplugged", "text": "on battery now" } } } });
            let _ = write!(conn, "data: {ev}\n\n");
            std::thread::sleep(Duration::from_millis(300));
            version.store(1, Ordering::SeqCst);
            let _ = write!(conn, "data: {}\n\n", json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }));
            std::thread::sleep(Duration::from_millis(600));
            return; // the stream closes: the peer left
        }
        let msg: Value = serde_json::from_slice(&body).unwrap();
        let id = msg["id"].clone();
        let tool = |n: &str| json!({ "name": n, "description": "the battery", "inputSchema": { "type": "object", "properties": {} }, "_meta": { "capability": "battery", "doc": "A battery." } });
        let result = match msg["method"].as_str().unwrap() {
            "initialize" => {
                json!({ "protocolVersion": "2025-11-25", "_meta": { "hello": { "runtime": "fake", "platform": "test", "version": "1", "protocol": "dry-capabilities/1" } } })
            }
            "tools/list" if version.load(Ordering::SeqCst) == 0 => json!({ "tools": [tool("battery.status")] }),
            "tools/list" => json!({ "tools": [tool("battery.status"), tool("battery.health")] }),
            "resources/list" => {
                json!({ "resources": [{ "uri": "dry://capability/battery", "_meta": { "capability": "battery", "doc": "A battery.", "state": "present", "snapshot": true,
                "events": [{ "name": "unplugged", "when": "contains discharging", "text": "on battery now" }] } }] })
            }
            "resources/read" => json!({ "contents": [{ "uri": "dry://capability/battery", "text": "80%; discharging" }] }),
            "tools/call" => {
                json!({ "content": [{ "type": "text", "text": format!("called {}", msg["params"]["name"]) }], "isError": false })
            }
            _ => json!({}),
        };
        reply(&mut conn, json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    fn until(what: &str, f: impl Fn() -> bool) {
        for _ in 0..100 {
            if f() {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("never: {what}");
    }

    #[test]
    fn a_peer_joins_its_event_fires_it_re_registers_and_leaving_makes_it_absent() {
        let (url, _) = fake_peer();
        let caps = Arc::new(Caps::default());
        let changes = Arc::new(AtomicUsize::new(0));
        let c2 = changes.clone();
        let joined = caps.join(
            "pi",
            &url,
            "CODE",
            Arc::new(move || {
                c2.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert_eq!(joined.unwrap()["runtime"], "fake");
        let r = caps.registry();
        let b = r.effective("battery").unwrap();
        assert_eq!((b.provider.clone(), b.present()), (Provider::Remote("pi".into()), true));
        assert_eq!(b.snapshot.as_ref().unwrap().text, "80%; discharging", "its first snapshot read at join");
        assert_eq!(r.peers[0].runtime, "fake");
        // A routed call goes to the peer.
        let call = Call { name: "battery.status".into(), args: vec![] };
        assert_eq!(caps.route(&call), Some(Ok("called \"battery.status\"".into())));
        // The event rides on its update, once.
        until("the event", || !caps.lock().fired.is_empty());
        let fired = caps.take_fired();
        assert_eq!(fired[0].message(), "event battery.unplugged: on battery now (capability battery)");
        // list_changed: listed again, re-registered.
        until("the re-register", || caps.registry().effective("battery").is_some_and(|c| c.tools.len() == 2));
        // The stream closes: the peer left, its capability absent, no route.
        until("the leave", || !caps.registry().present("battery"));
        assert!(caps.registry().state("battery").why().starts_with("pi left"));
        assert_eq!(caps.route(&call), None);
        assert!(changes.load(Ordering::SeqCst) >= 3, "joined, updated, re-registered, left");
    }
}
