//! The environment: the machine the `file` and `terminal` tools run
//! against (docs/container.md, "The environment"). Every shell has one,
//! picked at start by a simple check (`probe`):
//!
//! | kind | where | picked when |
//! |---|---|---|
//! | `Host` | this computer's disk and processes (server, window, CLI), or the paired companion's in the browser | the shell can start processes, or a companion is paired |
//! | `BrowserVm` | Alpine Linux in v86, in the page (`dry_web::env`); the workspace is `/work` in it | a browser build with no companion |
//! | `Local`, `Remote` | docker on this computer, the dry server's host | later: named so the kinds have room, not built |
//!
//! The kind is reported, not chosen per call: the tools keep their names
//! and results everywhere (`live::Host` is the seam). Only the model call
//! leaves the page in a browser with a VM.
//!
//! Pure. `vm` frames file operations as commands for a console shell
//! (the browser VM's serial console), and `Save` is when the VM's
//! in-RAM root is saved again.

use crate::live::Content;

/// What kind of machine the tools reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Host,
    BrowserVm,
    /// Later: a docker / podman container on this computer.
    Local,
    /// Later: the dry server's host, over its API.
    Remote,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Host => "host",
            Kind::BrowserVm => "browser-vm",
            Kind::Local => "local",
            Kind::Remote => "remote",
        }
    }

    /// What it is, in a few words.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Host => "Host: this computer's disk and processes",
            Kind::BrowserVm => "Browser VM: Alpine Linux in this page",
            Kind::Local => "Local: a container on this computer",
            Kind::Remote => "Remote: the dry server's host",
        }
    }
}

/// Where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Being made ready: what it is doing now.
    Booting(String),
    Ready,
    /// It will not come: why.
    Failed(String),
}

impl Phase {
    pub fn word(&self) -> &'static str {
        match self {
            Phase::Booting(_) => "booting",
            Phase::Ready => "ready",
            Phase::Failed(_) => "failed",
        }
    }

    /// The word and what goes with it.
    pub fn line(&self) -> String {
        match self {
            Phase::Booting(what) if !what.is_empty() => format!("booting: {what}"),
            Phase::Failed(why) => format!("failed: {why}"),
            p => p.word().to_string(),
        }
    }
}

/// A saved machine (the browser VM's snapshot in IndexedDB).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cache {
    /// Its size as stored (gzip).
    pub bytes: u64,
    /// When it was saved, unix seconds (0: not known).
    pub saved: i64,
}

/// The environment a shell reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    pub kind: Kind,
    pub phase: Phase,
    /// What runs it (`macos aarch64`, `Alpine 3.24.4 in v86, 256 MB`).
    pub detail: String,
    /// Where an artifact's root `.` is (the repo on a host, `/work` in the VM).
    pub workspace: String,
    /// The saved machine, for a kind that keeps one.
    pub cache: Option<Cache>,
    /// The runtimes found on it (`RUNTIMES`, by `runtimes_check` or a
    /// PATH look on a host), in `RUNTIMES` order. `None`: not checked yet.
    pub runtimes: Option<Vec<String>>,
}

/// The runtimes looked for, in the order they are listed: what the owner
/// asks for most (python, node or bun, java, rust) and the VM's C compiler.
pub const RUNTIMES: &[&str] = &["python3", "node", "bun", "java", "rustc", "cargo", "tcc", "gcc"];

/// The one shell line that says which of `RUNTIMES` are on PATH, one name
/// per line (cheap: `command -v` only, nothing is started). Run once after
/// a boot and after an install (`installs`).
pub fn runtimes_check() -> String {
    format!("for p in {}; do command -v \"$p\" >/dev/null 2>&1 && echo \"$p\"; done; true", RUNTIMES.join(" "))
}

/// What `runtimes_check` printed: the names found, in `RUNTIMES` order
/// (anything else on the console is left out).
pub fn parse_runtimes(out: &str) -> Vec<String> {
    let found: Vec<&str> = out.lines().map(|l| l.trim()).collect();
    RUNTIMES.iter().filter(|r| found.contains(r)).map(|r| r.to_string()).collect()
}

/// Whether a command may have installed or removed a runtime, so the
/// check is run again after it (`apk add`, `apk del`).
pub fn installs(command: &str) -> bool {
    let c = command.split_whitespace().collect::<Vec<_>>().join(" ");
    ["apk add", "apk del"].iter().any(|w| c.contains(w))
}

impl Environment {
    /// This computer: ready from the start.
    pub fn host(detail: &str, workspace: &str) -> Environment {
        Environment { kind: Kind::Host, phase: Phase::Ready, detail: detail.into(), workspace: workspace.into(), cache: None, runtimes: None }
    }

    /// Its line in the prompt (the `environment` element,
    /// `crate::element`): kind, state (and why, booting or failed),
    /// workspace, the runtimes found, the saved machine. A piece not known
    /// is left out, never guessed.
    pub fn line(&self) -> String {
        let mut parts = vec![self.kind.as_str().to_string(), self.phase.line()];
        if !self.workspace.is_empty() {
            parts.push(self.workspace.clone());
        }
        match &self.runtimes {
            Some(r) if r.is_empty() => parts.push(format!("none of {} on PATH", RUNTIMES.join(", "))),
            Some(r) => parts.push(r.join(", ")),
            None => {}
        }
        match &self.cache {
            Some(c) if c.bytes > 0 => parts.push(format!("saved {:.0} MB", c.bytes as f64 / 1e6)),
            Some(_) => parts.push("saved".into()),
            None => {}
        }
        format!("environment: {}", parts.join(" · "))
    }

    pub fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    /// Why the file and terminal tools cannot be used now (`None`: they can).
    pub fn not_ready(&self) -> Option<String> {
        match &self.phase {
            Phase::Ready => None,
            Phase::Booting(what) => Some(format!(
                "the environment ({}) is still booting{}: try again in a few seconds",
                self.kind.as_str(),
                if what.is_empty() { String::new() } else { format!(" ({what})") }
            )),
            Phase::Failed(why) => Some(format!("the environment ({}) failed: {why}", self.kind.as_str())),
        }
    }

    /// Settings → Environment, renderer-neutral: label and value per row.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![("kind", self.kind.label().to_string()), ("state", self.phase.line())];
        if !self.detail.is_empty() {
            rows.push(("runs", self.detail.clone()));
        }
        if !self.workspace.is_empty() {
            rows.push(("workspace", self.workspace.clone()));
        }
        if let Some(r) = &self.runtimes {
            rows.push(("runtimes", if r.is_empty() { format!("none of {} on PATH", RUNTIMES.join(", ")) } else { r.join(", ") }));
        }
        if self.kind == Kind::BrowserVm {
            rows.push((
                "cache",
                match &self.cache {
                    Some(c) if c.bytes == 0 => "saved in this browser (IndexedDB dry-vm; its size was not recorded)".into(),
                    Some(c) => format!("{:.1} MB saved in this browser (IndexedDB dry-vm)", c.bytes as f64 / 1e6),
                    None => "none saved yet".into(),
                },
            ));
        }
        rows
    }

    /// A saved machine to forget (Settings' "Forget cached VM").
    pub fn can_forget(&self) -> bool {
        self.kind == Kind::BrowserVm && self.cache.is_some()
    }
}

/// What a shell can do, for `probe`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// It can start processes on its own computer (a native build).
    pub processes: bool,
    /// It is a page (a WebAssembly build in a browser).
    pub page: bool,
    /// A page paired with a companion that has a workspace folder.
    pub companion: bool,
}

/// The simple check: a shell that can start processes, or a page paired
/// with a companion, uses that computer (`Host`); a page with none boots
/// its own Linux (`BrowserVm`). `None`: nothing to run tools on.
pub fn probe(p: Probe) -> Option<Kind> {
    match p {
        Probe { processes: true, .. } => Some(Kind::Host),
        Probe { page: true, companion: true, .. } => Some(Kind::Host),
        Probe { page: true, .. } => Some(Kind::BrowserVm),
        _ => None,
    }
}

// ---- when the browser VM is saved again ----

/// The browser VM's root is in RAM, so what the tools changed is kept
/// only by saving the whole machine again (53 MB gzip, about 1.6 s).
/// Saved when the machine changed (a file written, a command run or
/// ended), nothing changed for `QUIET_MS`, and the last save is
/// `GAP_MS` old. A save waits its turn at the console, so it never cuts
/// a command in half.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Save {
    /// When the first change not saved yet came (ms on the page's clock).
    pub dirty_since: Option<f64>,
    /// The latest change.
    pub last_change: f64,
    pub last_save: f64,
}

impl Save {
    pub const QUIET_MS: f64 = 5_000.0;
    pub const GAP_MS: f64 = 30_000.0;

    pub fn changed(&mut self, now: f64) {
        self.dirty_since.get_or_insert(now);
        self.last_change = now;
    }

    pub fn due(&self, now: f64) -> bool {
        self.dirty_since.is_some() && now - self.last_change >= Self::QUIET_MS && now - self.last_save >= Self::GAP_MS
    }

    /// A save began at `now`: changes from here on are the next one's.
    pub fn saving(&mut self, now: f64) {
        self.dirty_since = None;
        self.last_save = now;
    }
}

// ---- file operations as console commands ----

/// File operations framed for a shell reached over a console (the
/// browser VM's serial port), run with `container::exec_line`.
///
/// The console's tty reads a line of at most 4095 bytes in canonical
/// mode and drops the rest, so no command typed is longer than
/// `LINE_MAX`: a longer one is written to a script first, in base64
/// chunks appended to a file, and that script is run. File contents
/// always cross as base64 (both ways), so a `\r\n`, a zero byte or an
/// exit marker inside a file cannot be mistaken for the console's own.
pub mod vm {
    use super::Content;
    use crate::container::sh_quote;

    /// The workspace in the VM: an artifact's root `.`.
    pub const WORK: &str = "/work";
    /// The longest line typed into the console (the tty's limit is 4095).
    pub const LINE_MAX: usize = 3_000;
    /// Base64 characters per appended chunk.
    pub const CHUNK: usize = 2_400;
    /// Folders never mirrored or listed (as on the host).
    pub const HIDDEN: &[&str] = crate::workbench::HIDDEN;
    /// The most files and bytes mirrored into the page.
    pub const MIRROR_FILES: usize = 2_000;
    pub const MIRROR_BYTES: u64 = 16 * 1024 * 1024;

    /// An artifact's root as a folder in the VM. A root is a folder of
    /// the workspace (`.`, `src`); an absolute one is refused.
    pub fn dir(root: &str) -> Result<String, String> {
        let r = root.trim();
        if r.starts_with('/') {
            return Err(format!("the root {r} is absolute: in the browser's VM a root is a folder of {WORK} (`.` is {WORK})"));
        }
        let d = crate::workspace::root_dir(r)?;
        Ok(if d.is_empty() { WORK.to_string() } else { format!("{WORK}/{d}") })
    }

    /// A path under an artifact's root as a path relative to `WORK`.
    pub fn rel(root: &str, path: &str) -> Result<String, String> {
        let r = root.trim();
        if r.starts_with('/') {
            return Err(format!("the root {r} is absolute: in the browser's VM a root is a folder of {WORK} (`.` is {WORK})"));
        }
        let p = crate::workspace::in_root(r, path)?;
        let parts: Vec<&str> = p.split('/').filter(|x| !x.is_empty() && *x != ".").collect();
        if parts.contains(&"..") || parts.is_empty() {
            return Err(format!("{path} is not a file of the workspace"));
        }
        Ok(parts.join("/"))
    }

    /// A terminal command, run in folder `dir` (made if missing).
    pub fn run(dir: &str, command: &str) -> String {
        format!("mkdir -p {d} && cd {d} || exit 1; {command}", d = sh_quote(dir))
    }

    /// The lines to exec for `cmd`: itself when its exec line fits, else
    /// the steps that write it to `/tmp/dry-cmd-<id>.sh` and the line that
    /// runs that script. The last line's output is the command's.
    pub fn steps(id: u64, cmd: &str) -> Vec<String> {
        if crate::container::exec_line(id, cmd).len() <= LINE_MAX {
            return vec![cmd.to_string()];
        }
        let script = format!("/tmp/dry-cmd-{id}.sh");
        let mut out = chunks(&script, cmd.as_bytes());
        out.push(format!("sh {s}; c=$?; rm -f {s}; exit $c", s = sh_quote(&script)));
        out
    }

    /// The lines that write `bytes` to `to` (base64 appended to a temp
    /// file, then decoded into place). The temp file is `<to>.b64` in /tmp.
    fn chunks(to: &str, bytes: &[u8]) -> Vec<String> {
        let tmp = format!("/tmp/dry-w-{}.b64", hash(to));
        let b64 = encode(bytes);
        let mut out = vec![format!(": > {}", sh_quote(&tmp))];
        for c in b64.as_bytes().chunks(CHUNK) {
            // Base64 is ASCII: a chunk is whole characters.
            out.push(format!("printf %s {} >> {}", sh_quote(std::str::from_utf8(c).unwrap_or("")), sh_quote(&tmp)));
        }
        let parent = to.rsplit_once('/').map_or("", |(d, _)| d);
        let mkdir = if parent.is_empty() { String::new() } else { format!("mkdir -p {} && ", sh_quote(parent)) };
        out.push(format!("{mkdir}base64 -d {t} > {to}; c=$?; rm -f {t}; exit $c", t = sh_quote(&tmp), to = sh_quote(to)));
        out
    }

    /// The lines that write `text` to workspace file `rel`.
    pub fn write(rel: &str, text: &str) -> Vec<String> {
        chunks(&format!("{WORK}/{rel}"), text.as_bytes())
    }

    /// A small, stable name for a temp file.
    fn hash(s: &str) -> u64 {
        s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    }

    /// Every file of the workspace with its size and modified time, the
    /// hidden folders left out: `<size> <mtime> ./<path>` per line.
    pub fn manifest() -> String {
        let prune = HIDDEN.iter().map(|h| format!("-name {}", sh_quote(h))).collect::<Vec<_>>().join(" -o ");
        format!("mkdir -p {WORK} && cd {WORK} && find . \\( {prune} \\) -prune -o -type f -exec stat -c '%s %Y %n' {{}} +")
    }

    /// One file in the manifest.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Entry {
        pub path: String,
        pub size: u64,
        pub modified: i64,
    }

    pub fn parse_manifest(text: &str) -> Vec<Entry> {
        text.lines()
            .filter_map(|l| {
                let mut w = l.trim_end_matches('\r').splitn(3, ' ');
                let size = w.next()?.parse().ok()?;
                let modified = w.next()?.parse().ok()?;
                let path = w.next()?.strip_prefix("./")?.to_string();
                (!path.is_empty()).then_some(Entry { path, size, modified })
            })
            .collect()
    }

    /// The line that prints workspace files `paths` as base64, each after
    /// an `@@dry <path>` line.
    pub fn fetch(paths: &[String]) -> String {
        let names = paths.iter().map(|p| sh_quote(p)).collect::<Vec<_>>().join(" ");
        format!("cd {WORK} && for f in {names}; do printf '@@dry %s\\n' \"$f\"; base64 \"$f\"; done")
    }

    /// Groups of `paths` whose `fetch` line fits the console.
    pub fn fetch_groups(paths: &[String]) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        let mut len = 0;
        for p in paths {
            let n = p.len() + 4;
            if out.is_empty() || len + n > LINE_MAX - 200 {
                out.push(Vec::new());
                len = 0;
            }
            len += n;
            if let Some(g) = out.last_mut() {
                g.push(p.clone());
            }
        }
        out
    }

    /// What `fetch` printed: each file's bytes.
    pub fn parse_fetch(text: &str) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut cur: Option<(String, String)> = None;
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(p) = line.strip_prefix("@@dry ") {
                if let Some((p, b)) = cur.take() {
                    out.push((p, decode(&b)));
                }
                cur = Some((p.to_string(), String::new()));
            } else if let Some((_, b)) = cur.as_mut() {
                b.push_str(line.trim());
            }
        }
        if let Some((p, b)) = cur {
            out.push((p, decode(&b)));
        }
        out
    }

    /// A file's bytes as the tools read them (as the host's disk does).
    pub fn content(path: &str, bytes: &[u8]) -> Content {
        if bytes.len() > crate::workbench::MAX_OPEN {
            return Content::TooLarge(bytes.len() as u64);
        }
        let image = image(path);
        if bytes.contains(&0) {
            return Content::Binary { bytes: bytes.len() as u64, image };
        }
        match std::str::from_utf8(bytes) {
            Ok(t) => Content::Text(t.to_string()),
            Err(_) => Content::Binary { bytes: bytes.len() as u64, image },
        }
    }

    fn image(path: &str) -> bool {
        let p = path.to_lowercase();
        [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico", ".tiff", ".heic"].iter().any(|e| p.ends_with(e))
    }

    /// The files under folder `dir` (relative to `WORK`, `""` for all) of
    /// `paths`, relative to it, in the host's order: folder by folder from
    /// the top, names sorted, a folder's own files before its folders'.
    pub fn list<'a>(paths: impl IntoIterator<Item = &'a String>, dir: &str, max: usize) -> Vec<String> {
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let mut under: Vec<Vec<&str>> =
            paths.into_iter().filter_map(|p| p.strip_prefix(prefix.as_str())).map(|p| p.split('/').collect()).collect();
        under.retain(|parts| !parts.iter().any(|x| HIDDEN.contains(x)));
        // Breadth first: by depth, then by the folder, then by name.
        under.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        under.into_iter().take(max).map(|p| p.join("/")).collect()
    }

    // ---- base64 (the standard alphabet, padded) ----

    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// Decode, skipping whitespace and anything not in the alphabet.
    pub fn decode(text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let (mut n, mut bits) = (0u32, 0);
        for b in text.bytes() {
            let Some(v) = ABC.iter().position(|&a| a == b) else { continue };
            n = (n << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((n >> bits) as u8);
                n &= (1 << bits) - 1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::vm::*;
    use super::*;
    use crate::container::{exec_line, sh_quote};

    #[test]
    fn the_check_picks_the_kind() {
        assert_eq!(probe(Probe { processes: true, ..Probe::default() }), Some(Kind::Host));
        assert_eq!(probe(Probe { page: true, ..Probe::default() }), Some(Kind::BrowserVm));
        assert_eq!(probe(Probe { page: true, companion: true, ..Probe::default() }), Some(Kind::Host));
        assert_eq!(probe(Probe::default()), None);
    }

    #[test]
    fn rows_and_reasons() {
        let mut e = Environment {
            kind: Kind::BrowserVm,
            phase: Phase::Booting("restoring".into()),
            detail: String::new(),
            workspace: WORK.into(),
            cache: None,
            runtimes: None,
        };
        assert!(e.not_ready().unwrap().contains("still booting (restoring)"));
        assert!(!e.can_forget());
        e.phase = Phase::Failed("no image".into());
        assert_eq!(e.not_ready().unwrap(), "the environment (browser-vm) failed: no image");
        e.phase = Phase::Ready;
        e.cache = Some(Cache { bytes: 52_900_000, saved: 0 });
        assert!(e.not_ready().is_none() && e.can_forget());
        let rows = e.rows();
        assert_eq!(rows[1], ("state", "ready".to_string()));
        assert!(rows.iter().any(|(k, v)| *k == "cache" && v.starts_with("52.9 MB")));
        assert!(!Environment::host("macos", "/repo").rows().iter().any(|(k, _)| *k == "cache"));
    }

    #[test]
    fn the_runtime_check_reads_back_and_the_line_leaves_out_what_is_not_known() {
        let check = runtimes_check();
        assert!(check.starts_with("for p in python3 node bun java rustc cargo tcc gcc; do command -v"), "{check}");
        assert!(check.len() < LINE_MAX);
        assert_eq!(parse_runtimes("tcc\r\npython3\r\nnoise\n$ \n"), ["python3", "tcc"]);
        assert!(installs("apk  add python3") && installs("cd x && apk del nodejs") && !installs("python3 -c 'print(1)'"));
        let mut e = Environment {
            kind: Kind::BrowserVm,
            phase: Phase::Booting("restoring".into()),
            detail: String::new(),
            workspace: WORK.into(),
            cache: None,
            runtimes: None,
        };
        assert_eq!(e.line(), "environment: browser-vm · booting: restoring · /work");
        e.phase = Phase::Ready;
        e.runtimes = Some(vec!["python3".into(), "tcc".into()]);
        e.cache = Some(Cache { bytes: 36_200_000, saved: 0 });
        assert_eq!(e.line(), "environment: browser-vm · ready · /work · python3, tcc · saved 36 MB");
        e.runtimes = Some(Vec::new());
        assert!(e.line().contains(" · none of python3, node, bun, java, rustc, cargo, tcc, gcc on PATH · "), "{}", e.line());
        let h = Environment::host("macos", "/repo");
        assert_eq!(h.line(), "environment: host · ready · /repo");
    }

    #[test]
    fn roots_are_folders_of_work() {
        assert_eq!(dir(".").unwrap(), "/work");
        assert_eq!(dir("src/").unwrap(), "/work/src");
        assert!(dir("/tmp").is_err());
        assert!(dir("../x").is_err());
        assert_eq!(rel(".", "a/b.rs").unwrap(), "a/b.rs");
        assert_eq!(rel("src", "./main.rs").unwrap(), "src/main.rs");
        assert!(rel(".", "../etc/passwd").is_err());
        assert_eq!(run("/work/my dir", "make"), "mkdir -p '/work/my dir' && cd '/work/my dir' || exit 1; make");
    }

    #[test]
    fn base64_round_trips() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            assert_eq!(decode(&encode(s.as_bytes())), s.as_bytes());
        }
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(b"fo"), "Zm8=");
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)), all);
        // busybox wraps at 76 columns, with \r\n on the console
        assert_eq!(decode("Zm9v\r\nYmFy\r\n"), b"foobar");
    }

    #[test]
    fn a_write_is_chunked_under_the_line_limit() {
        let text = "x'y\r\n$(rm -rf /)\0".repeat(2_000); // 36 KB with quotes, CRLF, a zero byte, a command
        let lines = write("src/a b.txt", &text);
        assert!(lines.len() > 3);
        for (i, l) in lines.iter().enumerate() {
            assert!(exec_line(i as u64, l).len() <= LINE_MAX + 200, "line {i} is {} bytes", l.len());
        }
        assert!(lines[0].starts_with(": > '/tmp/dry-w-"));
        let last = lines.last().unwrap();
        assert!(last.starts_with("mkdir -p '/work/src' && base64 -d '/tmp/dry-w-"), "{last}");
        assert!(last.contains("> '/work/src/a b.txt'"));
        // The chunks put back together are the file.
        let b64: String = lines[1..lines.len() - 1]
            .iter()
            .map(|l| l.strip_prefix("printf %s '").and_then(|r| r.split('\'').next()).unwrap().to_string())
            .collect();
        assert_eq!(decode(&b64), text.as_bytes());
    }

    #[test]
    fn a_long_command_goes_through_a_script() {
        assert_eq!(steps(1, "echo hi"), vec!["echo hi".to_string()]);
        let long = format!("cat > big.txt <<'EOF'\n{}\nEOF", "line\n".repeat(1_000));
        let s = steps(9, &long);
        assert!(s.len() > 2);
        assert_eq!(s.last().unwrap(), "sh '/tmp/dry-cmd-9.sh'; c=$?; rm -f '/tmp/dry-cmd-9.sh'; exit $c");
        assert!(s.iter().all(|l| exec_line(9, l).len() <= LINE_MAX + 200));
    }

    #[test]
    fn manifest_and_fetch_read_back() {
        let m = manifest();
        assert!(m.contains("-name '.git' -o -name 'target'"), "{m}");
        let got = parse_manifest("12 1795356330 ./src/main.rs\r\n0 1795356331 ./a b.txt\nnoise\n");
        assert_eq!(
            got,
            vec![
                Entry { path: "src/main.rs".into(), size: 12, modified: 1_795_356_330 },
                Entry { path: "a b.txt".into(), size: 0, modified: 1_795_356_331 }
            ]
        );
        let f = fetch(&["a b.txt".into(), "it's.c".into()]);
        assert_eq!(
            f,
            format!("cd /work && for f in 'a b.txt' {}; do printf '@@dry %s\\n' \"$f\"; base64 \"$f\"; done", sh_quote("it's.c"))
        );
        let printed = format!("@@dry a b.txt\r\n\r\n@@dry src/x.rs\r\n{}\r\n", encode(b"fn main() {}\r\n"));
        assert_eq!(parse_fetch(&printed), vec![("a b.txt".to_string(), vec![]), ("src/x.rs".to_string(), b"fn main() {}\r\n".to_vec())]);
        let many: Vec<String> = (0..400).map(|i| format!("some/long/path/file-{i}.txt")).collect();
        let groups = fetch_groups(&many);
        assert!(groups.len() > 1 && groups.iter().all(|g| fetch(g).len() < LINE_MAX));
        assert_eq!(groups.concat(), many);
    }

    #[test]
    fn content_and_list_as_on_the_host() {
        assert_eq!(content("a.txt", b"hi"), Content::Text("hi".into()));
        assert_eq!(content("a.png", b"\x89PNG\0"), Content::Binary { bytes: 5, image: true });
        assert_eq!(content("a.bin", &[0xff, 0xfe]), Content::Binary { bytes: 2, image: false });
        let paths: Vec<String> =
            ["src/b.rs", "z.md", "a.md", "src/a/x.rs", "target/t", "src/.git/c", "docs/d.md"].map(String::from).to_vec();
        assert_eq!(list(&paths, "", 10), ["a.md", "z.md", "docs/d.md", "src/b.rs", "src/a/x.rs"]);
        assert_eq!(list(&paths, "src", 10), ["b.rs", "a/x.rs"]);
        assert_eq!(list(&paths, "", 2), ["a.md", "z.md"]);
    }

    #[test]
    fn saved_again_when_quiet() {
        let mut s = Save::default();
        assert!(!s.due(100_000.0));
        s.changed(100_000.0);
        assert!(!s.due(103_000.0), "not quiet yet");
        assert!(s.due(105_000.0));
        s.saving(105_000.0);
        assert!(!s.due(200_000.0), "nothing new");
        s.changed(110_000.0);
        assert!(!s.due(120_000.0), "too soon after the last save");
        assert!(s.due(135_000.0));
    }
}
