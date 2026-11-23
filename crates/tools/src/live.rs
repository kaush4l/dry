//! The disk and the processes behind `file` and `terminal` artifacts: the
//! `dry_core::live::Host` the app hands the core.
//!
//! - Roots: a relative root is under the repo (`.` is the repo itself), an
//!   absolute one is used as it is. A path the core passes is already
//!   relative with no `..`; here it is also checked after links are
//!   followed, so a link cannot lead out of the root.
//! - Sessions: each runs `/bin/sh -c <command>` in the root, with no
//!   input and without the environment variables whose names say they
//!   hold secrets (`terminal::secret_env`). What it prints goes to
//!   `<engines>/<engine>/artifacts/<artifact>/<session>.log`; its exit code
//!   to `<session>.exit` when it ends, and its process id to `<session>.pid`.
//!   So a session's status is on disk: it survives the app restarting,
//!   and one that died with the app reads as stopped.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use dry_core::live::{Content, Host, Job, Proc};
use dry_core::workbench::{HIDDEN, MAX_OPEN};

/// The exit files of the sessions this process started and still waits on.
static RUNNING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// The disk under a repo, and an engines folder for the logs.
#[derive(Clone, Debug)]
pub struct Disk {
    /// What a relative root is relative to.
    pub repo: PathBuf,
    /// `<engines>/<engine id>/artifacts/...`: the store's engine folders.
    pub engines: PathBuf,
}

impl Disk {
    /// For the store `spec` describes (`md:<folder>`, `sqlite:<file>`);
    /// one that does not open keeps its logs under `state/engines`.
    pub fn for_store(spec: &str) -> Disk {
        let engines = crate::store::open(spec).map(|s| s.files_dir()).unwrap_or_else(|_| crate::root().join("state/engines"));
        Disk { repo: crate::root(), engines }
    }

    fn root(&self, root: &str) -> Result<PathBuf, String> {
        let dir = if Path::new(root).is_absolute() { PathBuf::from(root) } else { self.repo.join(root) };
        dir.canonicalize().map_err(|e| format!("the root {root} is not there: {e}"))
    }

    /// `path` under `root`, links followed, still inside it.
    fn inside(&self, root: &str, path: &str) -> Result<PathBuf, String> {
        let root = self.root(root)?;
        let full = root.join(path);
        let real = full.canonicalize().map_err(|_| format!("no file {path} under the root"))?;
        if !real.starts_with(&root) {
            return Err(format!("{path} leads outside the root (a link)"));
        }
        Ok(real)
    }

    /// `<session>.<ext>` in the job's log folder.
    fn file(&self, job: &Job, ext: &str) -> PathBuf {
        self.engines.join(job.engine).join(dry_core::files::ARTIFACTS).join(job.artifact).join(format!("{}.{ext}", job.session))
    }

    /// The command, in the root, output to the log; secrets left out.
    fn command(&self, job: &Job, script: &str, args: &[&str]) -> Result<Command, String> {
        let dir = self.root(job.root)?;
        let log = self.file(job, "log");
        fs::create_dir_all(log.parent().expect("a log has a folder")).map_err(|e| e.to_string())?;
        let out = fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let _ = fs::remove_file(self.file(job, "exit"));
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(script).args(args).current_dir(dir).stdin(Stdio::null()).stdout(out).stderr(err);
        for (name, _) in std::env::vars_os() {
            if dry_core::terminal::secret_env(&name.to_string_lossy()) {
                cmd.env_remove(&name);
            }
        }
        Ok(cmd)
    }
}

/// Whether a process is still there (after a restart, when this process
/// does not wait on it).
fn alive(pid: &str) -> bool {
    Command::new("kill").args(["-0", pid.trim()]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The wall clock now, with this machine's offset from UTC (asked of
/// `date +%z` once; UTC when that cannot be read). What a prompt's clock
/// artifact shows when it is sent.
pub fn clock() -> dry_core::live::Clock {
    static OFFSET: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    let offset = *OFFSET.get_or_init(|| {
        let out = Command::new("date").arg("+%z").output().ok().filter(|o| o.status.success());
        out.and_then(|o| parse_offset(String::from_utf8_lossy(&o.stdout).trim())).unwrap_or(0)
    });
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    dry_core::live::Clock::new(now, offset)
}

/// `+0530` / `-0500` as seconds east of UTC.
fn parse_offset(z: &str) -> Option<i32> {
    let (sign, digits) = match z.as_bytes().first()? {
        b'+' => (1, &z[1..]),
        b'-' => (-1, &z[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m): (i32, i32) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
    Some(sign * (h * 3600 + m * 60))
}

impl Host for Disk {
    fn now(&self) -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
    }
    fn spans(&self, engine: &str) -> Result<Vec<dry_core::spans::Span>, String> {
        if !dry_core::workbench::safe(engine) || engine.contains('/') || engine.is_empty() {
            return Err(format!("not an engine id: {engine}"));
        }
        Ok(crate::spans::load(&self.engines, engine))
    }
    fn offset(&self) -> i32 {
        clock().offset
    }

    /// Folder by folder from the root (so a cut list still shows the top),
    /// names sorted, hidden ones (`.git`, `target`, ...) left out.
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        let base = self.root(root)?;
        let mut out = Vec::new();
        let mut dirs = std::collections::VecDeque::from([String::new()]);
        while let Some(rel) = dirs.pop_front() {
            let Ok(entries) = fs::read_dir(base.join(&rel)) else { continue };
            let mut names: Vec<(String, bool)> = entries
                .flatten()
                .map(|e| (e.file_name().to_string_lossy().to_string(), e.file_type().is_ok_and(|t| t.is_dir())))
                .filter(|(n, _)| !HIDDEN.contains(&n.as_str()))
                .collect();
            names.sort();
            for (name, dir) in names {
                let path = dry_core::workbench::join(&rel, &name);
                if dir {
                    dirs.push_back(path);
                } else {
                    out.push(path);
                    if out.len() >= max {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }

    fn read(&self, root: &str, path: &str) -> Content {
        let real = match self.inside(root, path) {
            Ok(p) => p,
            Err(why) => return Content::Missing(why),
        };
        let Ok(meta) = fs::metadata(&real) else { return Content::Missing(format!("{path} cannot be read")) };
        if meta.is_dir() {
            return Content::Missing(format!("{path} is a folder"));
        }
        if meta.len() > MAX_OPEN as u64 {
            return Content::TooLarge(meta.len());
        }
        match fs::read(&real) {
            Ok(bytes) if bytes.contains(&0) => Content::Binary { bytes: meta.len(), image: image(path) },
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Content::Text(text),
                Err(_) => Content::Binary { bytes: meta.len(), image: image(path) },
            },
            Err(e) => Content::Missing(format!("{path}: {e}")),
        }
    }

    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        // In place, so the file keeps its permissions (a script stays
        // executable) and no temp file lands in the person's folder.
        let real = self.inside(root, path)?;
        fs::write(&real, text).map_err(|e| format!("{path}: {e}"))
    }

    fn run(&self, job: &Job) -> Result<Option<i32>, String> {
        let status = self.command(job, job.command, &[])?.status().map_err(|e| format!("could not start /bin/sh: {e}"))?;
        let code = status.code();
        if let Some(c) = code {
            let _ = fs::write(self.file(job, "exit"), c.to_string());
        }
        Ok(code)
    }

    fn spawn(&self, job: &Job) -> Result<(), String> {
        let exit = self.file(job, "exit");
        // The outer shell writes the exit code when the command ends; if it
        // is killed with the app, there is none, and the session is stopped.
        let script = "/bin/sh -c \"$1\"; echo $? > \"$2\"";
        let mut child = self.command(job, script, &["sh", job.command, &exit.to_string_lossy()])?.spawn().map_err(|e| format!("could not start /bin/sh: {e}"))?;
        let _ = fs::write(self.file(job, "pid"), child.id().to_string());
        RUNNING.lock().map_err(|e| e.to_string())?.insert(exit.clone());
        std::thread::spawn(move || {
            let _ = child.wait();
            if let Ok(mut running) = RUNNING.lock() {
                running.remove(&exit);
            }
        });
        Ok(())
    }

    fn status(&self, job: &Job) -> Proc {
        let exit = self.file(job, "exit");
        if let Ok(code) = fs::read_to_string(&exit) {
            return Proc::Ended(code.trim().parse().ok());
        }
        if RUNNING.lock().is_ok_and(|r| r.contains(&exit)) {
            return Proc::Running;
        }
        match fs::read_to_string(self.file(job, "pid")) {
            Ok(pid) if alive(&pid) => Proc::Running,
            _ => Proc::Ended(None),
        }
    }

    /// The end of the log (its last 64 KB), as text.
    fn tail(&self, job: &Job, lines: usize) -> String {
        let Ok(mut f) = fs::File::open(self.file(job, "log")) else { return String::new() };
        let len = f.metadata().map_or(0, |m| m.len());
        let _ = f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024)));
        let mut bytes = Vec::new();
        let _ = f.read_to_end(&mut bytes);
        dry_core::terminal::last_lines(&String::from_utf8_lossy(&bytes), lines)
    }
}

/// Run an act's waiting calls (`dry_core::turn::Pending::waits`) all at
/// once, one thread each, and wait for every one: what each came to, in
/// the order written, whatever order they finished in. So the step takes
/// as long as its slowest call, not their sum. `outside` runs a call no
/// built-in tool takes (an MCP tool: `mcp::Servers::call`); it is called
/// from several threads at once.
pub fn run_waits<H, F>(waits: &[(dry_core::calls::Call, dry_core::turn::Wait)], outside: &F, host: &H) -> Vec<dry_core::turn::Came>
where
    H: Host + Sync,
    F: Fn(&dry_core::calls::Call) -> Option<Result<String, String>> + Sync,
{
    run_waits_timed(waits, outside, host).into_iter().map(|(c, _)| c).collect()
}

/// `run_waits`, with when each call started and ended (unix ms), for its
/// span (`dry_core::runner::Runner::wait_times`).
pub fn run_waits_timed<H, F>(waits: &[(dry_core::calls::Call, dry_core::turn::Wait)], outside: &F, host: &H) -> Vec<(dry_core::turn::Came, (i64, i64))>
where
    H: Host + Sync,
    F: Fn(&dry_core::calls::Call) -> Option<Result<String, String>> + Sync,
{
    std::thread::scope(|scope| {
        // Every call is launched before any is waited for.
        let running: Vec<_> = waits
            .iter()
            .map(|(_, w)| {
                scope.spawn(move || {
                    let mut out = |c: &dry_core::calls::Call| outside(c);
                    let start = crate::spans::now_ms();
                    let came = dry_core::turn::run_wait(w, &mut out, host);
                    (came, (start, crate::spans::now_ms()))
                })
            })
            .collect();
        running
            .into_iter()
            .map(|t| {
                t.join().unwrap_or_else(|_| {
                    let now = crate::spans::now_ms();
                    (dry_core::turn::Came::Outside(Some(Err("the call panicked".into()))), (now, now))
                })
            })
            .collect()
    })
}

fn image(path: &str) -> bool {
    let p = path.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico", ".tiff", ".heic"].iter().any(|e| p.ends_with(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_has_this_machines_offset() {
        assert_eq!([parse_offset("+0530"), parse_offset("-0500"), parse_offset("+0000")], [Some(19_800), Some(-18_000), Some(0)]);
        assert_eq!([parse_offset("PST"), parse_offset("+5")], [None, None]);
        let c = clock();
        assert!(c.unix > 1_700_000_000 && c.offset.abs() <= 14 * 3600, "{c}");
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dry-live-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("repo/src")).unwrap();
        dir.canonicalize().unwrap()
    }

    fn job<'a>(session: u32, command: &'a str) -> Job<'a> {
        Job { engine: "e", artifact: "term", session, root: ".", command }
    }

    #[test]
    fn files_are_read_listed_and_written_inside_the_root_only() {
        let dir = scratch("files");
        let disk = Disk { repo: dir.join("repo"), engines: dir.join("engines") };
        fs::write(dir.join("repo/src/a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.join("repo/top.md"), "# top\n").unwrap();
        fs::write(dir.join("repo/pic.png"), [0x89, 0, 1]).unwrap();
        fs::write(dir.join("outside.txt"), "no").unwrap();
        std::os::unix::fs::symlink(dir.join("outside.txt"), dir.join("repo/link.txt")).unwrap();
        assert_eq!(disk.list(".", 10).unwrap(), ["link.txt", "pic.png", "top.md", "src/a.rs"], "the top folder first");
        assert_eq!(disk.list(".", 2).unwrap().len(), 2);
        assert_eq!(disk.read(".", "src/a.rs"), Content::Text("fn a() {}\n".into()));
        assert_eq!(disk.read(".", "pic.png"), Content::Binary { bytes: 3, image: true });
        assert!(matches!(disk.read(".", "link.txt"), Content::Missing(why) if why.contains("outside the root")));
        assert!(disk.write(".", "link.txt", "x").is_err());
        disk.write(".", "top.md", "# changed\n").unwrap();
        assert_eq!(fs::read_to_string(dir.join("repo/top.md")).unwrap(), "# changed\n");
        assert_eq!(fs::read_to_string(dir.join("outside.txt")).unwrap(), "no");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sessions_run_in_the_root_log_their_output_and_end_on_disk() {
        let dir = scratch("terminal");
        let disk = Disk { repo: dir.join("repo"), engines: dir.join("engines") };
        // SAFETY: tests in this crate do not read this variable elsewhere.
        unsafe { std::env::set_var("DRY_TEST_SECRET_TOKEN", "hunter2") };
        assert_eq!(disk.run(&job(1, "pwd; echo \"[$DRY_TEST_SECRET_TOKEN]\"; exit 3")).unwrap(), Some(3));
        let tail = disk.tail(&job(1, ""), 5);
        assert!(tail.ends_with("repo\n[]"), "in the root, no secret: {tail}");
        assert!(dir.join("engines/e/artifacts/term/1.log").exists());
        assert_eq!(disk.status(&job(1, "")), Proc::Ended(Some(3)));

        disk.spawn(&job(2, "sleep 0.3; echo done")).unwrap();
        assert_eq!(disk.status(&job(2, "")), Proc::Running);
        let ended = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            disk.status(&job(2, "")) == Proc::Ended(Some(0))
        });
        assert!(ended, "it ended with exit 0");
        assert_eq!(disk.tail(&job(2, ""), 5), "done");
        // A session with no exit file and no live process: stopped.
        assert_eq!(disk.status(&job(9, "")), Proc::Ended(None));
        let _ = fs::remove_dir_all(&dir);
    }

    /// The whole loop on a real disk: a fake model's answers, the real
    /// files and processes.
    #[test]
    fn a_turn_on_the_real_disk_edits_a_file_and_a_started_command_calls_back() {
        use dry_core::turn::{Step, step_with};
        let dir = scratch("turn");
        let disk = Disk { repo: dir.join("repo"), engines: dir.join("engines") };
        fs::write(dir.join("repo/src/a.rs"), "fn a() -> u8 { 1 }\n").unwrap();
        let mut state = dry_core::state::State::default();
        let i = state.add_engine();
        let e = &mut state.engines[i];
        e.response = dry_core::response::Responses::from_md(include_str!("../../../responses.md")).unwrap().0;
        e.artifacts.insert("code".into(), "kind: file\nroot: .".into());
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        e.messages.push(dry_core::inference::Message { role: dry_core::inference::Role::User, text: "make a return 2".into() });
        let act = |e: &mut dry_core::state::Engine, calls: &str| step_with(e, &format!("thought: x\nact: {calls}"), &mut |_| None, &disk);
        act(e, "open(path=\"src/a.rs\"); edit(path=\"src/a.rs\", old=\"{ 1 }\", new=\"{ 2 }\"); save(path=\"src/a.rs\")");
        assert!(e.messages.last().unwrap().text.ends_with("→ saved src/a.rs (1 edit, 1 line)"), "{}", e.messages.last().unwrap().text);
        assert_eq!(fs::read_to_string(dir.join("repo/src/a.rs")).unwrap(), "fn a() -> u8 { 2 }\n");
        act(e, "start(command=\"sleep 0.2; grep -c 2 src/a.rs\")");
        assert_eq!(act(e, "reply(text=\"started\")"), Step::Reply("started".into()));
        let mut wake = Vec::new();
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            wake = dry_core::terminal::settle_engine(&mut state.engines[i], &disk);
            if !wake.is_empty() {
                break;
            }
        }
        assert_eq!(wake, ["session 1 `sleep 0.2; grep -c 2 src/a.rs` finished, exit 0 (terminal term)"]);
        assert!(state.add_text(i, &wake.join("\n")));
        let sent = dry_core::prompt::render_with(&state.engines[i], &dry_core::live::Snapshots { clock: None, host: Some(&disk), family: None }).messages();
        let system = &sent[0].text;
        // Its age is 0s or 1s, by where the second boundary falls.
        let last = system.split("#### last: session 1 — exit 0, ").nth(1).expect(system);
        assert!(last.split_once("s ago\n").is_some_and(|(age, rest)| age.parse::<u32>().is_ok_and(|a| a <= 1) && rest.starts_with("$ sleep 0.2; grep -c 2 src/a.rs\n1")), "{system}");
        assert!(system.contains("#### src/a.rs — 1 line\n    1  fn a() -> u8 { 2 }"), "saved: read from disk, not marked: {system}");
        assert!(dir.join("engines").join(&state.engines[i].id).join("artifacts/term/1.log").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
