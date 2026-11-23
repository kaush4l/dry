//! This machine's Chrome, headless, over the DevTools protocol: the same
//! approach as the companion's browser (companion/dry-companion.mjs), in
//! Rust so `dry evidence` needs neither Node nor a download.
//!
//! ```text
//! Chrome --headless=new --remote-debugging-port=0 ──stderr──▶ "DevTools listening on ws://…"
//!   ▲                                                              │
//!   └──────── JSON commands {id, method, params, sessionId} ◀── Ws (a small WebSocket client)
//! ```
//!
//! The WebSocket client is the few lines RFC 6455 needs for a local
//! socket: the upgrade, masked text frames out, frames (fragmented, ping,
//! close) in. Only `ws://127.0.0.1`, which is all Chrome gives.
//!
//! While a command waits, events are handled as they come: the request
//! gate (`Fetch.requestPaused`: only the app's own origin may load), the
//! network's in-flight count (for "network idle"), load events.

use std::collections::{BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;

/// A WebSocket to Chrome.
pub struct Ws {
    stream: TcpStream,
    buf: Vec<u8>,
    partial: Vec<u8>,
    mask: u32,
}

impl Ws {
    pub fn connect(url: &str) -> Result<Ws> {
        let rest = url.strip_prefix("ws://").ok_or_else(|| format!("not a ws:// URL: {url}"))?;
        let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
        let mut stream = TcpStream::connect(host).map_err(|e| format!("could not reach Chrome at {host}: {e}"))?;
        stream.set_nodelay(true).ok();
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ZHJ5LWV2aWRlbmNlLWtleQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).map_err(|e| format!("Chrome closed the upgrade: {e}"))?;
            head.push(byte[0]);
            if head.len() > 16 * 1024 {
                return Err("Chrome's upgrade answer is too long".into());
            }
        }
        let head = String::from_utf8_lossy(&head);
        if !head.starts_with("HTTP/1.1 101") {
            return Err(format!("Chrome refused the WebSocket: {}", head.lines().next().unwrap_or("")));
        }
        Ok(Ws { stream, buf: Vec::new(), partial: Vec::new(), mask: 0x5eed_1234 })
    }

    fn frame(&mut self, opcode: u8, payload: &[u8]) -> Result<()> {
        let mut out = Vec::with_capacity(payload.len() + 14);
        out.push(0x80 | opcode);
        match payload.len() {
            n if n < 126 => out.push(0x80 | n as u8),
            n if n <= 0xffff => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                out.push(0x80 | 127);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        self.mask = self.mask.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let key = self.mask.to_be_bytes();
        out.extend_from_slice(&key);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        self.stream.write_all(&out).map_err(|e| format!("Chrome's socket: {e}"))
    }

    pub fn send(&mut self, text: &str) -> Result<()> {
        self.frame(0x1, text.as_bytes())
    }

    /// One whole frame from the buffer, if it holds one: (fin, opcode, payload).
    fn take_frame(&mut self) -> Option<(bool, u8, Vec<u8>)> {
        let b = &self.buf;
        if b.len() < 2 {
            return None;
        }
        let (fin, opcode, masked) = (b[0] & 0x80 != 0, b[0] & 0x0f, b[1] & 0x80 != 0);
        let (len, mut at) = match b[1] & 0x7f {
            126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as usize, 4),
            127 if b.len() >= 10 => (u64::from_be_bytes(b[2..10].try_into().ok()?) as usize, 10),
            126 | 127 => return None,
            n => (n as usize, 2),
        };
        let key = if masked {
            if b.len() < at + 4 {
                return None;
            }
            at += 4;
            Some([b[at - 4], b[at - 3], b[at - 2], b[at - 1]])
        } else {
            None
        };
        if b.len() < at + len {
            return None;
        }
        let mut payload = b[at..at + len].to_vec();
        if let Some(k) = key {
            for (i, x) in payload.iter_mut().enumerate() {
                *x ^= k[i % 4];
            }
        }
        self.buf.drain(..at + len);
        Some((fin, opcode, payload))
    }

    /// The next text message, waiting at most `timeout`. `None`: none came.
    pub fn recv(&mut self, timeout: Duration) -> Result<Option<String>> {
        let until = Instant::now() + timeout;
        loop {
            while let Some((fin, opcode, payload)) = self.take_frame() {
                match opcode {
                    0x0..=0x2 => {
                        self.partial.extend_from_slice(&payload);
                        if fin {
                            let msg = String::from_utf8_lossy(&std::mem::take(&mut self.partial)).into_owned();
                            return Ok(Some(msg));
                        }
                    }
                    0x8 => return Err("Chrome closed the DevTools socket".into()),
                    0x9 => self.frame(0xA, &payload)?,
                    _ => {}
                }
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.stream.set_read_timeout(Some(left.max(Duration::from_millis(1)))).ok();
            let mut chunk = [0u8; 64 * 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err("Chrome closed the DevTools socket".into()),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(None),
                Err(e) => return Err(format!("Chrome's socket: {e}")),
            }
        }
    }
}

/// Where Chrome is on this machine (`--chrome`, `CHROME_PATH`, the usual
/// places, then `PATH`), as the companion looks.
pub fn find_chrome(given: Option<&str>) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = given.map(PathBuf::from).into_iter().collect();
    if let Ok(p) = std::env::var("CHROME_PATH") {
        c.push(p.into());
    }
    for p in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ] {
        c.push(p.into());
    }
    if let Some(found) = c.into_iter().find(|p| p.exists()) {
        return Some(found);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge"]
        .iter()
        .flat_map(|b| std::env::split_paths(&path).map(move |d| d.join(b)))
        .find(|p| p.exists())
}

/// A running headless Chrome. Dropping it closes it and removes its profile.
pub struct Chrome {
    child: Child,
    profile: PathBuf,
    ws: Ws,
    next: u64,
    events: VecDeque<Value>,
    /// Requests to these URL prefixes may load; every other is failed.
    pub allow: Vec<String>,
    /// Requests still loading, by session: (session, request id).
    inflight: BTreeSet<(String, String)>,
    /// When the network last did something.
    pub last_net: Instant,
    /// Requests the gate failed (shown once per run).
    pub blocked: BTreeSet<String>,
}

/// The flags that make a headless shot the same every time.
pub const FLAGS: [&str; 17] = [
    "--headless=new",
    "--remote-debugging-port=0",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-gpu",
    "--disable-extensions",
    "--disable-background-networking",
    "--disable-sync",
    "--disable-component-update",
    "--hide-scrollbars",
    "--mute-audio",
    "--force-color-profile=srgb",
    "--disable-lcd-text",
    "--font-render-hinting=none",
    "--run-all-compositor-stages-before-draw",
    "--disable-threaded-animation",
    "--disable-renderer-backgrounding",
];

impl Chrome {
    pub fn launch(bin: &Path, profile_parent: &Path) -> Result<Chrome> {
        std::fs::create_dir_all(profile_parent).map_err(|e| e.to_string())?;
        let profile = profile_parent.join(format!("chrome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile);
        let mut child = Command::new(bin)
            .args(FLAGS)
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", bin.display()))?;
        let stderr = child.stderr.take().ok_or("no stderr from Chrome")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stderr).lines().map_while(std::result::Result::ok) {
                if !sent && let Some(i) = line.find("ws://") {
                    let _ = tx.send(line[i..].trim().to_string());
                    sent = true;
                }
            }
        });
        let url = match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(u) => u,
            Err(_) => {
                let _ = child.kill();
                return Err("Chrome did not say where its DevTools socket is within 20 s".into());
            }
        };
        let ws = Ws::connect(&url)?;
        Ok(Chrome {
            child,
            profile,
            ws,
            next: 1,
            events: VecDeque::new(),
            allow: Vec::new(),
            inflight: BTreeSet::new(),
            last_net: Instant::now(),
            blocked: BTreeSet::new(),
        })
    }

    /// Handle one message: an event the runner acts on at once, or keep it.
    fn on_event(&mut self, v: Value) -> Result<()> {
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        let session = v.get("sessionId").and_then(Value::as_str).unwrap_or("").to_string();
        let p = v.get("params").cloned().unwrap_or(Value::Null);
        let rid = || p.get("requestId").and_then(Value::as_str).unwrap_or("").to_string();
        match method {
            "Fetch.requestPaused" => {
                let url = p.pointer("/request/url").and_then(Value::as_str).unwrap_or("");
                let ok = url.starts_with("data:") || url.starts_with("blob:") || self.allow.iter().any(|a| url.starts_with(a.as_str()));
                let (m, params) = if ok {
                    ("Fetch.continueRequest", json!({"requestId": rid()}))
                } else {
                    self.blocked.insert(url.chars().take(120).collect());
                    ("Fetch.failRequest", json!({"requestId": rid(), "errorReason": "BlockedByClient"}))
                };
                let id = self.next;
                self.next += 1;
                let msg = json!({"id": id, "method": m, "params": params, "sessionId": session});
                self.ws.send(&msg.to_string())?;
            }
            "Network.requestWillBeSent" => {
                self.inflight.insert((session, rid()));
                self.last_net = Instant::now();
            }
            "Network.loadingFinished" | "Network.loadingFailed" => {
                self.inflight.remove(&(session, rid()));
                self.last_net = Instant::now();
            }
            _ => self.events.push_back(v),
        }
        Ok(())
    }

    /// Read and handle messages for at most `wait`.
    pub fn pump(&mut self, wait: Duration) -> Result<()> {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.ws.recv(left)? {
                Some(text) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&text)
                        && v.get("method").is_some()
                    {
                        self.on_event(v)?;
                    }
                }
                None => return Ok(()),
            }
            if Instant::now() >= until {
                return Ok(());
            }
        }
    }

    /// Send a command and wait for its answer (events meanwhile handled).
    pub fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.call_for(method, params, session, Duration::from_secs(30))
    }

    pub fn call_for(&mut self, method: &str, params: Value, session: Option<&str>, timeout: Duration) -> Result<Value> {
        let id = self.next;
        self.next += 1;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws.send(&msg.to_string())?;
        let until = Instant::now() + timeout;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("{method} took longer than {} s", timeout.as_secs()));
            }
            let Some(text) = self.ws.recv(left)? else { continue };
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("Chrome sent what does not read: {e}"))?;
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(e) = v.get("error") {
                    return Err(format!("{method}: {}", e.get("message").and_then(Value::as_str).unwrap_or("failed")));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            if v.get("method").is_some() {
                self.on_event(v)?;
            }
        }
    }

    /// Wait for an event of `method` on `session` (kept ones first).
    pub fn wait_event(&mut self, method: &str, session: &str, timeout: Duration) -> Result<Value> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(i) = self.events.iter().position(|e| {
                e.get("method").and_then(Value::as_str) == Some(method) && e.get("sessionId").and_then(Value::as_str) == Some(session)
            }) {
                return Ok(self.events.remove(i).unwrap_or(Value::Null));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("no {method} within {} s", timeout.as_secs()));
            }
            self.pump(left.min(Duration::from_millis(200)))?;
        }
    }

    /// Take the kept events of `session`: the ones of `methods` back, the
    /// rest dropped (a page's console, for the companion's browser).
    pub fn take(&mut self, session: &str, methods: &[&str]) -> Vec<Value> {
        let mut out = Vec::new();
        self.events.retain(|e| {
            if e.get("sessionId").and_then(Value::as_str) != Some(session) {
                return true;
            }
            if e.get("method").and_then(Value::as_str).is_some_and(|m| methods.contains(&m)) {
                out.push(e.clone());
            }
            false
        });
        out
    }

    /// Drop the kept events of a session (it is closed).
    pub fn forget(&mut self, session: &str) {
        self.events.retain(|e| e.get("sessionId").and_then(Value::as_str) != Some(session));
        self.inflight.retain(|(s, _)| s != session);
    }

    /// Requests of `session` still loading.
    pub fn loading(&self, session: &str) -> usize {
        self.inflight.iter().filter(|(s, _)| s == session).count()
    }

    /// Wait until `session`'s network has been quiet for `quiet` (at most `max`).
    pub fn network_idle(&mut self, session: &str, quiet: Duration, max: Duration) -> Result<bool> {
        let until = Instant::now() + max;
        loop {
            if self.loading(session) == 0 && self.last_net.elapsed() >= quiet {
                return Ok(true);
            }
            if Instant::now() >= until {
                return Ok(false);
            }
            self.pump(Duration::from_millis(50))?;
        }
    }

    /// Evaluate an expression in the page, awaiting a promise; its value.
    pub fn eval(&mut self, session: &str, expression: &str) -> Result<Value> {
        let r =
            self.call("Runtime.evaluate", json!({"expression": expression, "returnByValue": true, "awaitPromise": true}), Some(session))?;
        if let Some(d) = r.get("exceptionDetails") {
            let why = d.pointer("/exception/description").or_else(|| d.get("text")).and_then(Value::as_str).unwrap_or("an exception");
            return Err(why.lines().next().unwrap_or(why).to_string());
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        let id = self.next;
        let _ = self.ws.send(&json!({"id": id, "method": "Browser.close"}).to_string());
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// Base64 (standard alphabet, padded) to bytes: Chrome's screenshots.
pub fn base64(text: &str) -> Result<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c).ok_or("not base64")? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_padded_and_unpadded() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVsbG8h").unwrap(), b"hello!");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("").unwrap(), b"");
        assert!(base64("a*b=").is_err());
    }

    /// A server frame and a fragmented one read back; a ping is skipped.
    #[test]
    fn frames_read_whole_fragmented_and_long() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut b = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                s.read_exact(&mut b).unwrap();
                head.push(b[0]);
            }
            s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
            s.write_all(&[0x81, 5]).unwrap();
            s.write_all(b"hello").unwrap();
            s.write_all(&[0x89, 0]).unwrap(); // ping
            s.write_all(&[0x01, 3]).unwrap();
            s.write_all(b"abc").unwrap();
            s.write_all(&[0x80, 3]).unwrap();
            s.write_all(b"def").unwrap();
            let long = vec![b'x'; 70_000];
            s.write_all(&[0x81, 127]).unwrap();
            s.write_all(&(long.len() as u64).to_be_bytes()).unwrap();
            s.write_all(&long).unwrap();
            // The client's masked message comes back unmasked to us.
            let mut f = [0u8; 2 + 4 + 2];
            s.read_exact(&mut f[..2]).unwrap(); // the pong
            s.read_exact(&mut f[2..6]).unwrap(); // its mask
            s.read_exact(&mut f[..2]).unwrap();
            assert_eq!(f[0], 0x81);
            let mut key = [0u8; 4];
            s.read_exact(&mut key).unwrap();
            let mut p = vec![0u8; (f[1] & 0x7f) as usize];
            s.read_exact(&mut p).unwrap();
            let text: Vec<u8> = p.iter().enumerate().map(|(i, x)| x ^ key[i % 4]).collect();
            assert_eq!(text, b"ok");
        });
        let mut ws = Ws::connect(&format!("ws://{addr}/devtools/browser/x")).unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(ws.recv(t).unwrap().as_deref(), Some("hello"));
        assert_eq!(ws.recv(t).unwrap().as_deref(), Some("abcdef"));
        assert_eq!(ws.recv(t).unwrap().map(|s| s.len()), Some(70_000));
        ws.send("ok").unwrap();
        server.join().unwrap();
    }
}
