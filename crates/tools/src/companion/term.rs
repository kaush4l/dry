//! Terminal sessions in the folder (docs/companion-protocol.md §6): one
//! shell command line each (`/bin/sh -c`, `cmd.exe /d /s /c`), in its own
//! process group, no input, the secret-looking environment left out;
//! `run` waits, `start` returns at once and its end is an `exit` event
//! (a callback), `stop` and stop-all send SIGTERM then SIGKILL 2 s later.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::rules::{self, Fail, fail};
use super::state::Companion;
use super::sys;

/// What a session keeps of its output.
const MAX_OUTPUT: usize = 256 * 1024;
/// Sessions kept (the oldest finished go first).
const MAX_SESSIONS: usize = 200;
/// The longest a command may run.
pub const MAX_TIMEOUT: u64 = 60 * 60 * 1000;

/// One command's run.
pub struct Session {
    pub id: String,
    pub key: Option<String>,
    pub cwd: String,
    pub command: String,
    /// `run`, `start` or `background`.
    pub mode: &'static str,
    pub started: i64,
    pub pid: Mutex<Option<u32>>,
    run: Mutex<Run>,
    done: Condvar,
}

#[derive(Default)]
struct Run {
    running: bool,
    exit: Option<i32>,
    signal: Option<String>,
    timed_out: bool,
    ended: Option<i64>,
    output: String,
}

impl Session {
    pub fn running(&self) -> bool {
        self.run.lock().unwrap().running
    }

    /// Wait for its end.
    pub fn wait(&self) {
        let mut r = self.run.lock().unwrap();
        while r.running {
            r = self.done.wait(r).unwrap();
        }
    }

    fn fields(&self, r: &Run) -> Value {
        json!({ "key": self.key, "id": self.id, "mode": self.mode, "status": if r.running { "running" } else { "exited" },
            "exit": r.exit, "signal": r.signal, "timed_out": r.timed_out, "started": self.started, "ended": r.ended, "command": self.command })
    }

    /// As the terminal endpoints answer it; `tail`: with its last lines.
    pub fn view(&self, tail: Option<usize>) -> Value {
        let r = self.run.lock().unwrap();
        let mut v = self.fields(&r);
        if let Some(n) = tail {
            v["tail"] = json!(dry_core::terminal::last_lines(&r.output, n));
        }
        v
    }

    /// As events carry it (with its folder).
    pub fn public(&self) -> Value {
        let mut v = self.view(None);
        v["cwd"] = json!(self.cwd);
        v
    }

    /// Its output's last `max` bytes.
    pub fn output(&self, max: usize) -> String {
        let r = self.run.lock().unwrap();
        cut_front(&r.output, max).to_string()
    }

    pub fn exit_word(&self) -> String {
        let r = self.run.lock().unwrap();
        r.exit.map(|c| c.to_string()).or_else(|| r.signal.clone()).unwrap_or_else(|| "?".into())
    }
}

/// The last `max` bytes of `s`, on a character boundary.
fn cut_front(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut at = s.len() - max;
    while !s.is_char_boundary(at) {
        at += 1;
    }
    &s[at..]
}

/// Start a command. `timeout`: it is killed after that.
pub fn launch(
    c: &Arc<Companion>,
    cwd: &std::path::Path,
    command: &str,
    mode: &'static str,
    key: Option<String>,
    timeout: Option<Duration>,
) -> Arc<Session> {
    let id = c.next_session.fetch_add(1, Ordering::Relaxed).to_string();
    let s = Arc::new(Session {
        id: id.clone(),
        key: key.clone(),
        cwd: cwd.display().to_string(),
        command: command.into(),
        mode,
        started: sys::now(),
        pid: Mutex::new(None),
        run: Mutex::new(Run { running: true, ..Run::default() }),
        done: Condvar::new(),
    });
    {
        let mut all = c.sessions.lock().unwrap();
        if let Some(k) = &key {
            all.retain(|o| o.key.as_ref() != Some(k) || o.running());
        }
        all.push(s.clone());
        while all.len() > MAX_SESSIONS {
            match all.iter().position(|o| !o.running()) {
                Some(i) => {
                    all.remove(i);
                }
                None => break,
            }
        }
    }
    #[cfg(unix)]
    let mut cmd = {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(command);
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        cmd
    };
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()));
        std::os::windows::process::CommandExt::raw_arg(&mut cmd, format!("/d /s /c \"{command}\""));
        cmd
    };
    cmd.current_dir(cwd).stdin(Stdio::null());
    for (name, _) in std::env::vars_os() {
        if dry_core::terminal::secret_env(&name.to_string_lossy()) {
            cmd.env_remove(&name);
        }
    }
    // One pipe for stdout and stderr, so the output keeps its order.
    let started = std::io::pipe().and_then(|(reader, writer)| {
        cmd.stdout(writer.try_clone()?).stderr(writer);
        let child = cmd.spawn();
        drop(cmd);
        child.map(|ch| (reader, ch))
    });
    let (mut reader, mut child) = match started {
        Ok(x) => x,
        Err(e) => {
            finish(c, &s, None, None, Some(format!("could not start the shell: {e}")));
            return s;
        }
    };
    *s.pid.lock().unwrap() = Some(child.id());
    let out = {
        let (c, s) = (c.clone(), s.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending: Vec<u8> = Vec::new();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                pending.extend_from_slice(&buf[..n]);
                // Whole characters only; a split one waits for its rest.
                let upto = match std::str::from_utf8(&pending) {
                    Ok(_) => pending.len(),
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    Err(_) => pending.len(),
                };
                let text = String::from_utf8_lossy(&pending[..upto]).into_owned();
                pending.drain(..upto);
                {
                    let mut r = s.run.lock().unwrap();
                    r.output.push_str(&text);
                    if r.output.len() > MAX_OUTPUT {
                        r.output = cut_front(&r.output, MAX_OUTPUT).to_string();
                    }
                }
                c.output(&s.id, json!({ "id": s.id, "key": s.key, "text": text }));
            }
        })
    };
    if let Some(t) = timeout {
        let (c, s) = (c.clone(), s.clone());
        std::thread::spawn(move || {
            let r = s.run.lock().unwrap();
            let (r, waited) = s.done.wait_timeout_while(r, t, |r| r.running).unwrap();
            if waited.timed_out() && r.running {
                drop(r);
                s.run.lock().unwrap().timed_out = true;
                kill(&c, &s);
            }
        });
    }
    let (c2, s2) = (c.clone(), s.clone());
    std::thread::spawn(move || {
        let status = child.wait();
        let _ = out.join();
        let (code, signal) = match &status {
            Ok(st) => {
                #[cfg(unix)]
                let sig = std::os::unix::process::ExitStatusExt::signal(st).map(sys::signal_name);
                #[cfg(not(unix))]
                let sig = None;
                (st.code(), sig)
            }
            Err(_) => (None, None),
        };
        finish(&c2, &s2, code, signal, status.err().map(|e| e.to_string()));
    });
    s
}

fn finish(c: &Companion, s: &Session, exit: Option<i32>, signal: Option<String>, why: Option<String>) {
    {
        let mut r = s.run.lock().unwrap();
        if !r.running {
            return;
        }
        if let Some(w) = why {
            r.output.push_str(&format!("\n{w}\n"));
        }
        r.running = false;
        r.exit = exit;
        r.signal = signal;
        r.ended = Some(sys::now());
    }
    s.done.notify_all();
    let timed = if s.run.lock().unwrap().timed_out { " (timed out)" } else { "" };
    c.log(&format!("exec {} {} ended {}{timed}: {}", s.id, s.mode, s.exit_word(), s.command.chars().take(60).collect::<String>()));
    // A start's exit is a callback: every stream hears it; a run's (its
    // caller waits) and a background's only a stream for that session.
    c.emit("exit", s.public(), s.mode == "start");
}

/// SIGTERM to its process group, SIGKILL 2 s later if it still runs.
pub fn kill(_c: &Arc<Companion>, s: &Arc<Session>) {
    let Some(pid) = *s.pid.lock().unwrap() else { return };
    if !s.running() {
        return;
    }
    sys::kill_group(pid, sys::SIGTERM);
    let s = s.clone();
    std::thread::spawn(move || {
        let r = s.run.lock().unwrap();
        let (r, _) = s.done.wait_timeout_while(r, Duration::from_secs(2), |r| r.running).unwrap();
        if r.running {
            drop(r);
            sys::kill_group(pid, sys::SIGKILL);
        }
    });
}

/// The last session with `key`.
pub fn by_key(c: &Companion, key: &str) -> Option<Arc<Session>> {
    c.sessions.lock().unwrap().iter().rev().find(|s| s.key.as_deref() == Some(key)).cloned()
}

/// A key checked (`400 bad_key`).
pub fn key(v: &Value) -> Result<String, Fail> {
    match v.as_str() {
        Some(k) if rules::key_ok(k) => Ok(k.to_string()),
        _ => Err(fail(400, "bad_key", "key names a session: letters, digits and ._:/- (at most 200)")),
    }
}

/// A terminal call's checks, in order: the switch, the key, the command,
/// publishing, its folder.
pub fn prep(c: &Companion, b: &Value) -> Result<(String, String, std::path::PathBuf), Fail> {
    c.need("terminal")?;
    let key = key(&b["key"])?;
    let command = b["command"].as_str().unwrap_or("").to_string();
    if command.trim().is_empty() {
        return Err(fail(400, "bad_request", "a command is needed"));
    }
    if command.contains('\0') {
        return Err(fail(400, "bad_request", "the command holds a zero byte"));
    }
    check_publishing(&command)?;
    let cwd = match &b["cwd"] {
        Value::Null => "",
        Value::String(s) => s.as_str(),
        _ => return Err(fail(400, "bad_request", "cwd is a folder of the workspace")),
    };
    let w = super::folder::ws_path(c, cwd, false, true)?;
    Ok((key, command, w.real))
}

pub fn check_publishing(command: &str) -> Result<(), Fail> {
    if rules::needs_confirm(command) {
        return Err(fail(
            428,
            "needs_confirm",
            "push, clone and publishing commands go through the git endpoints with a person's click (docs/companion.md)",
        ));
    }
    Ok(())
}

/// Stop every running command: how many.
pub fn stop_all(c: &Arc<Companion>) -> usize {
    let running: Vec<Arc<Session>> = c.sessions.lock().unwrap().iter().filter(|s| s.running()).cloned().collect();
    for s in &running {
        kill(c, s);
    }
    running.len()
}
