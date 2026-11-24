//! `kind: file`: the file system, live. (Not `crate::files`, which is how
//! an engine is kept as files.)
//!
//! ```text
//! artifacts/code.md
//! kind: file
//! root: .
//! open: crates/core/src/turn.rs 40-90
//! open: README.md
//! edit: README.md
//! ~~~ old
//! # dry
//! ~~~ new
//! # dry-learn
//! ~~~
//! ```
//!
//! The state is the root, the open files (each with an optional focus, a
//! line range for a big file) and the unsaved edits, each the `old` text
//! and its `new` one in a `~~~` block. A text line that itself starts with
//! `~~~` gets a `\` in front.
//!
//! A file is made the same way: `edit` with only `new` (no `old`) on a
//! path that is not there yet, or holds nothing, keeps the whole text as
//! an unsaved edit with an empty `old`; `save` writes it. A file that
//! holds text is never replaced whole.
//!
//! The tools change only the state, and their results are short
//! (`opened turn.rs (412 lines)`); the content is in the snapshot, read
//! from disk at every render, with the unsaved edits laid over it and
//! marked. So history never holds a file's text, and what the model reads
//! is what is on disk now.
//!
//! Paths stay inside the root: relative, no `..` (`check_path`), and the
//! shell checks where links lead. Files whose names say they hold secrets
//! (`.env`, keys) are never opened.
//!
//! Read mode (`uses: [file:read]`, `crate::capability::READ`): only open
//! and focus are brought; edit and save (`WRITES`) are refused.
//!
//! Pure. The shell reads, lists and writes through `live::Host`.

use crate::calls::Call;
use crate::live::{Content, Host};

/// The tools a file artifact brings.
pub const TOOLS: [&str; 4] = ["open", "focus", "edit", "save"];
/// The ones that change a file on disk: not brought where the artifact
/// is read-only (`uses: [file:read]`, `crate::capability::READ`).
pub const WRITES: [&str; 2] = ["edit", "save"];
/// Most file names listed in the snapshot.
pub const MAX_NAMES: usize = 200;
/// Most files open at once; opening one more closes the oldest without
/// unsaved edits.
pub const MAX_OPEN: usize = 8;
/// Most characters of the whole snapshot; past it an open file is cut, and
/// the result says how to see the rest (`focus`).
pub const SNAPSHOT_CHARS: usize = 24_000;

const FENCE: &str = "~~~";

/// An open file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub path: String,
    /// Lines `from..=to` (from 1) only; `None`: the whole file.
    pub focus: Option<(usize, usize)>,
}

/// An unsaved edit: `old` becomes `new` in `path` when it is saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub path: String,
    pub old: String,
    pub new: String,
}

/// A file artifact's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Relative to the repo (`.`), or absolute (set by hand: the tools
    /// never change it).
    pub root: String,
    pub open: Vec<Open>,
    pub edits: Vec<Edit>,
}

impl Default for State {
    fn default() -> State {
        State { root: ".".into(), open: Vec::new(), edits: Vec::new() }
    }
}

impl State {
    /// Read an artifact's text (its `kind: file` line first).
    pub fn parse(text: &str) -> Result<State, String> {
        let mut s = State::default();
        let mut lines = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).skip(1).peekable();
        while let Some(line) = lines.next() {
            let line = line.trim_end();
            if line.trim().is_empty() {
                continue;
            }
            if let Some(root) = line.strip_prefix("root:") {
                s.root = root.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("open:") {
                let rest = rest.trim();
                let focus = rest.rsplit_once(' ').and_then(|(p, r)| Some((p, range(r)?)));
                s.open.push(match focus {
                    Some((path, r)) => Open { path: path.trim().to_string(), focus: Some(r) },
                    None => Open { path: rest.to_string(), focus: None },
                });
            } else if let Some(path) = line.strip_prefix("edit:") {
                let path = path.trim().to_string();
                if lines.next().map(str::trim_end) != Some("~~~ old") {
                    return Err(format!("edit: {path}: a `~~~ old` line must follow"));
                }
                let mut old = Vec::new();
                let mut new = Vec::new();
                let mut in_new = false;
                loop {
                    match lines.next() {
                        None => return Err(format!("edit: {path}: no closing `~~~`")),
                        Some(l) if l.trim_end() == "~~~ new" && !in_new => in_new = true,
                        Some(l) if l.trim_end() == FENCE && in_new => break,
                        Some(l) => (if in_new { &mut new } else { &mut old }).push(unescape(l)),
                    }
                }
                s.edits.push(Edit { path, old: old.join("\n"), new: new.join("\n") });
            } else {
                return Err(format!("a file artifact line is root:, open: or edit:, not `{line}`"));
            }
        }
        Ok(s)
    }

    pub fn to_text(&self) -> String {
        let mut out = format!("kind: file\nroot: {}\n", self.root);
        for o in &self.open {
            match o.focus {
                Some((a, b)) => out.push_str(&format!("open: {} {a}-{b}\n", o.path)),
                None => out.push_str(&format!("open: {}\n", o.path)),
            }
        }
        for e in &self.edits {
            out.push_str(&format!("edit: {}\n~~~ old\n", e.path));
            for l in e.old.split('\n') {
                out.push_str(&escape(l));
                out.push('\n');
            }
            out.push_str("~~~ new\n");
            for l in e.new.split('\n') {
                out.push_str(&escape(l));
                out.push('\n');
            }
            out.push_str("~~~\n");
        }
        out
    }

    fn is_open(&self, path: &str) -> bool {
        self.open.iter().any(|o| o.path == path)
    }

    fn edits_of(&self, path: &str) -> Vec<&Edit> {
        self.edits.iter().filter(|e| e.path == path).collect()
    }

    /// Room for one more open file: at `MAX_OPEN` the oldest one without
    /// unsaved edits is closed (its path comes back); with none to close,
    /// why not.
    fn make_room(&mut self) -> Result<Option<String>, String> {
        let mut closed = None;
        if self.open.len() >= MAX_OPEN
            && let Some(i) = self.open.iter().position(|o| !self.edits.iter().any(|e| e.path == o.path))
        {
            closed = Some(self.open.remove(i).path);
        }
        if self.open.len() >= MAX_OPEN {
            return Err(format!("{MAX_OPEN} files are open, each with unsaved edits: save one first"));
        }
        Ok(closed)
    }

    /// The open files as paths from the repo, in the order opened: the
    /// tabs a workbench shows for this artifact. Empty for a root outside
    /// the repo (an absolute one), which the workbench cannot show.
    pub fn tabs(&self) -> Vec<String> {
        let root = self.root.trim_start_matches("./").trim_end_matches('/');
        if root.starts_with('/') {
            return Vec::new();
        }
        let root = if root == "." { "" } else { root };
        let paths = self.open.iter().map(|o| crate::workbench::join(root, &o.path));
        paths.filter(|p| crate::workbench::safe(p)).collect()
    }
}

/// `40-90` → `(40, 90)`.
fn range(text: &str) -> Option<(usize, usize)> {
    let (a, b) = text.split_once('-')?;
    let (a, b) = (a.parse().ok()?, b.parse().ok()?);
    (a >= 1 && b >= a).then_some((a, b))
}

fn escape(line: &str) -> String {
    if line.starts_with(FENCE) || line.starts_with("\\~~~") { format!("\\{line}") } else { line.to_string() }
}

fn unescape(line: &str) -> String {
    match line.strip_prefix('\\') {
        Some(rest) if rest.starts_with(FENCE) || rest.starts_with("\\~~~") => rest.to_string(),
        _ => line.to_string(),
    }
}

/// Whether a file name says it holds secrets: never opened, never shown.
pub fn secret_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || [".netrc", ".npmrc", ".pypirc", ".pgpass"].contains(&name.as_str())
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || name.starts_with("credentials")
        || name.starts_with("secrets.")
        || [".pem", ".key", ".p12", ".pfx", ".keystore"].iter().any(|ext| name.ends_with(ext))
}

/// `path` as a path under `root`, `/`-separated with no `.` parts; or why
/// it is refused. An absolute path is accepted only inside an absolute
/// root.
pub fn check_path(root: &str, path: &str) -> Result<String, String> {
    let given = path.trim();
    let mut rel = given;
    if given.starts_with('/') {
        let root = root.trim_end_matches('/');
        match given.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
            Some(r) if root.starts_with('/') => rel = r,
            _ => return Err(format!("`{given}` is outside the root ({root}): give a path inside it, relative to the root")),
        }
    }
    let mut parts = Vec::new();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(format!("`{given}`: `..` is not allowed; paths stay inside the root ({root})")),
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err("give a file's path, relative to the root".into());
    }
    let clean = parts.join("/");
    if secret_file(&clean) {
        return Err(format!("`{clean}` may hold secrets, so it is never opened or shown"));
    }
    Ok(clean)
}

/// `text` with `edits` laid over it in order, and per line of the result
/// whether an edit made it. Error: the edit (from 1) whose old text is no
/// longer there.
pub fn apply(text: &str, edits: &[&Edit]) -> Result<(String, Vec<bool>), usize> {
    let mut text = text.to_string();
    let mut changed = vec![false; text.split('\n').count()];
    for (n, e) in edits.iter().enumerate() {
        let at = text.find(&e.old).ok_or(n + 1)?;
        let first = text[..at].matches('\n').count();
        let gone = e.old.matches('\n').count() + 1;
        let come = e.new.matches('\n').count() + 1;
        text.replace_range(at..at + e.old.len(), &e.new);
        changed.splice(first..(first + gone).min(changed.len()), std::iter::repeat_n(true, come));
    }
    Ok((text, changed))
}

/// `1 line`, `412 lines`.
fn lines_of(text: &str) -> String {
    match text.lines().count() {
        1 => "1 line".into(),
        n => format!("{n} lines"),
    }
}

/// The file's text from disk, or why there is none.
fn read_text(host: &dyn Host, root: &str, path: &str) -> Result<String, String> {
    match host.read(root, path) {
        Content::Text(t) => Ok(t),
        Content::Binary { image: true, .. } => Err(format!("{path} is an image: it cannot be edited")),
        Content::Binary { .. } => Err(format!("{path} is not text: it cannot be edited")),
        Content::TooLarge(b) => Err(format!("{path} is too large ({} KB)", b / 1000)),
        Content::Missing(why) => Err(why),
    }
}

/// Whether `edits` start a file from nothing: the first has no old text
/// (`edit` with only `new`).
pub fn makes_new(edits: &[&Edit]) -> bool {
    edits.first().is_some_and(|e| e.old.is_empty())
}

/// `read_text`, where a file that is not on disk reads as empty when it
/// is being made (`new`): a new file's text is its edits alone.
fn text_or_new(host: &dyn Host, root: &str, path: &str, new: bool) -> Result<String, String> {
    match host.read(root, path) {
        Content::Missing(_) if new => Ok(String::new()),
        _ => read_text(host, root, path),
    }
}

/// Run one of the file tools on the artifact's `text` (its state), which
/// changes only when the call works. The result is one short line.
pub fn call(text: &mut String, call: &Call, host: &dyn Host) -> Result<String, String> {
    // One call, several files: `open(path="a.rs|b.rs")` opens each in turn;
    // the result is one line per file. It works if any file opened.
    if call.name == "open"
        && let Some(paths) = call.arg("path").filter(|p| p.contains('|'))
    {
        let mut ok = false;
        let lines: Vec<String> = paths
            .split('|')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| {
                let one = Call { name: call.name.clone(), args: vec![("path".into(), p.to_string())] };
                match self::call(text, &one, host) {
                    Ok(out) => {
                        ok = true;
                        out
                    }
                    Err(why) => format!("{p}: error: {why}"),
                }
            })
            .collect();
        return if ok { Ok(lines.join("; ")) } else { Err(lines.join("; ")) };
    }
    let mut s = State::parse(text).map_err(|why| format!("the file artifact does not read: {why}"))?;
    let need = |key: &str| call.arg(key).map(str::to_string).ok_or_else(|| format!("`{}` needs {key}=\"...\"", call.name));
    let path = check_path(&s.root, &need("path")?)?;
    let out = match call.name.as_str() {
        "open" | "focus" => {
            let focus = if call.name == "focus" {
                let num = |key: &str| -> Result<usize, String> {
                    need(key)?.trim().parse().map_err(|_| format!("`focus` needs {key} as a line number"))
                };
                let (from, to) = (num("from")?, num("to")?);
                if from == 0 || to < from {
                    return Err("`focus` needs 1 ≤ from ≤ to".into());
                }
                Some((from, to))
            } else {
                None
            };
            let size = match host.read(&s.root, &path) {
                Content::Text(t) => lines_of(&t),
                Content::Binary { bytes, image } => format!("{}, {} KB; shown as one line", if image { "an image" } else { "not text" }, bytes / 1000),
                Content::TooLarge(b) => format!("too large to show, {} KB", b / 1000),
                Content::Missing(why) => return Err(why),
            };
            let was_open = s.is_open(&path);
            let mut closed = None;
            match s.open.iter_mut().find(|o| o.path == path) {
                Some(o) => o.focus = focus.or(if call.name == "open" { None } else { o.focus }),
                None => {
                    closed = s.make_room()?;
                    s.open.push(Open { path: path.clone(), focus });
                }
            }
            let what = match (call.name.as_str(), focus) {
                ("focus", Some((a, b))) => format!("focus {path} {a}-{b} ({size})"),
                _ if was_open => format!("{path} is open ({size}); whole file shown"),
                _ => format!("opened {path} ({size})"),
            };
            match closed {
                Some(c) => format!("{what}; closed {c} ({MAX_OPEN} open at most)"),
                None => what,
            }
        }
        // No old text: the new text is the whole of a file that is not
        // there yet (or holds nothing). This is how a file is made: no
        // tool of its own, and never over text that is already there.
        "edit" if call.arg("old").is_none_or(str::is_empty) => {
            let new = call.arg("new").unwrap_or("").to_string();
            if new.is_empty() {
                return Err("`edit` needs old=\"...\" (the text to replace) and new=\"...\"; for a new file, only new=\"...\" with its whole text".into());
            }
            let edits = s.edits_of(&path);
            let disk = text_or_new(host, &s.root, &path, edits.is_empty() || makes_new(&edits))?;
            let now = apply(&disk, &edits).map_err(|n| stale(&path, n))?.0;
            if !now.trim().is_empty() {
                return Err(format!(
                    "{path} already holds text, so `edit` needs old=\"...\": the text to replace, exactly as it is in the file (only a new or empty file takes a whole text)"
                ));
            }
            if !s.is_open(&path) {
                s.make_room()?;
                s.open.push(Open { path: path.clone(), focus: None });
            }
            let size = lines_of(&new);
            s.edits.push(Edit { path: path.clone(), old: String::new(), new });
            format!("new file {path} ({size}, unsaved; save(path=\"{path}\") writes it)")
        }
        "edit" => {
            if !s.is_open(&path) {
                return Err(format!("{path} is not open: open(path=\"{path}\") first"));
            }
            let (old, new) = (need("old")?, call.arg("new").unwrap_or("").to_string());
            let disk = text_or_new(host, &s.root, &path, makes_new(&s.edits_of(&path)))?;
            let (now, _) = apply(&disk, &s.edits_of(&path)).map_err(|n| stale(&path, n))?;
            match now.matches(&old).count() {
                0 => return Err(format!("the old text is not in {path} (with its unsaved edits): copy it exactly from the snapshot")),
                1 => {}
                n => return Err(format!("the old text is in {path} {n} times: give more of the text around it")),
            }
            s.edits.push(Edit { path: path.clone(), old, new });
            let n = s.edits_of(&path).len();
            format!("edited {path} (unsaved: {n} edit{}; save(path=\"{path}\") writes it)", if n == 1 { "" } else { "s" })
        }
        "save" => {
            let edits = s.edits_of(&path);
            if edits.is_empty() {
                return Err(format!("no unsaved edits in {path}"));
            }
            let n = edits.len();
            let disk = text_or_new(host, &s.root, &path, makes_new(&edits))?;
            match apply(&disk, &edits) {
                Ok((new, _)) => {
                    host.write(&s.root, &path, &new)?;
                    s.edits.retain(|e| e.path != path);
                    format!("saved {path} ({n} edit{}, {})", if n == 1 { "" } else { "s" }, lines_of(&new))
                }
                Err(k) => {
                    // The file changed under the edits: they are dropped, so
                    // a stale edit can never block the file for good.
                    s.edits.retain(|e| e.path != path);
                    *text = s.to_text();
                    return Err(format!("{}; nothing saved and its {n} unsaved edit(s) dropped: edit it again", stale(&path, k)));
                }
            }
        }
        other => return Err(format!("`{other}` is not a file tool")),
    };
    *text = s.to_text();
    Ok(out)
}

fn stale(path: &str, n: usize) -> String {
    format!("unsaved edit {n} of {path} no longer applies (the file changed on disk)")
}

/// What the model reads: the file names under the root (the open ones
/// marked), then each open file with line numbers (all of it, or its
/// focus), its unsaved edits laid over it and marked `*`.
pub fn snapshot(text: &str, host: &dyn Host) -> String {
    let s = match State::parse(text) {
        Ok(s) => s,
        Err(why) => return format!("This file artifact does not read: {why}"),
    };
    let mut out = String::new();
    match host.list(&s.root, MAX_NAMES + 1) {
        Ok(names) => {
            let names: Vec<&String> = names.iter().filter(|n| !secret_file(n)).collect();
            out.push_str(&format!("root: {} — files (* open):\n", s.root));
            for n in names.iter().take(MAX_NAMES) {
                let mark = if s.is_open(n) { "* " } else { "  " };
                out.push_str(&format!("{mark}{n}\n"));
            }
            if names.len() > MAX_NAMES {
                out.push_str(&format!("  … more: only the first {MAX_NAMES} are listed\n"));
            }
        }
        Err(why) => out.push_str(&format!("root: {} — cannot be listed: {why}\n", s.root)),
    }
    if s.open.is_empty() {
        out.push_str("\nNo files open: open(path=\"...\") shows one here; edit(path=\"...\", new=\"...\") makes a new one.");
        return out;
    }
    for o in &s.open {
        out.push('\n');
        let edits = s.edits_of(&o.path);
        let unsaved = match edits.len() {
            0 => String::new(),
            n => format!(", {n} unsaved edit{} (lines marked *)", if n == 1 { "" } else { "s" }),
        };
        let disk = match host.read(&s.root, &o.path) {
            Content::Text(t) => t,
            Content::Binary { bytes, image } => {
                let what = if image { "an image" } else { "not text" };
                out.push_str(&format!("#### {} — {what}, {} KB: not shown (until vision lands)\n", o.path, bytes / 1000));
                continue;
            }
            Content::TooLarge(b) => {
                out.push_str(&format!("#### {} — too large to show ({} KB)\n", o.path, b / 1000));
                continue;
            }
            // A file being made: not on disk until it is saved.
            Content::Missing(_) if makes_new(&edits) => String::new(),
            Content::Missing(why) => {
                out.push_str(&format!("#### {} — not readable now: {why}{unsaved}\n", o.path));
                continue;
            }
        };
        let (shown, changed) = match apply(&disk, &edits) {
            Ok(x) => x,
            Err(n) => {
                let marks = vec![false; disk.split('\n').count()];
                out.push_str(&format!("({}; save drops them)\n", stale(&o.path, n)));
                (disk, marks)
            }
        };
        let lines: Vec<&str> = shown.lines().collect();
        let total = lines.len();
        let (from, to) = match o.focus {
            Some((a, b)) => (a.min(total.max(1)), b.min(total)),
            None => (1, total),
        };
        let part = if o.focus.is_some() { format!(", lines {from}-{to} shown") } else { String::new() };
        let count = if total == 1 { "1 line".to_string() } else { format!("{total} lines") };
        out.push_str(&format!("#### {} — {count}{part}{unsaved}\n", o.path));
        for (i, line) in lines.iter().enumerate().take(to).skip(from - 1) {
            let mark = if changed.get(i).copied().unwrap_or(false) { '*' } else { ' ' };
            let row = format!("{:>5}{mark} {line}\n", i + 1);
            if out.chars().count() + row.chars().count() > SNAPSHOT_CHARS {
                out.push_str(&format!(
                    "… cut at line {i} of {total} (the open files are long): focus(path=\"{}\", from=\"{}\", to=\"...\") shows more\n",
                    o.path,
                    i + 1
                ));
                break;
            }
            out.push_str(&row);
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::live::{Job, Proc};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A disk in memory: path → text (`\0` in it: a binary file).
    #[derive(Default)]
    pub struct FakeDisk {
        pub files: RefCell<BTreeMap<String, String>>,
    }

    impl FakeDisk {
        pub fn with(files: &[(&str, &str)]) -> FakeDisk {
            FakeDisk { files: RefCell::new(files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()) }
        }
    }

    impl Host for FakeDisk {
        fn now(&self) -> i64 {
            0
        }
        fn list(&self, _: &str, max: usize) -> Result<Vec<String>, String> {
            Ok(self.files.borrow().keys().take(max).cloned().collect())
        }
        fn read(&self, _: &str, path: &str) -> Content {
            match self.files.borrow().get(path) {
                Some(t) if t.contains('\0') => Content::Binary { bytes: t.len() as u64, image: path.ends_with(".png") },
                Some(t) => Content::Text(t.clone()),
                None => Content::Missing(format!("no file {path} under the root")),
            }
        }
        fn write(&self, _: &str, path: &str, text: &str) -> Result<(), String> {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            Ok(())
        }
        fn run(&self, _: &Job) -> Result<Option<i32>, String> {
            Err("no processes here".into())
        }
        fn spawn(&self, _: &Job) -> Result<(), String> {
            Err("no processes here".into())
        }
        fn status(&self, _: &Job) -> Proc {
            Proc::Ended(None)
        }
        fn tail(&self, _: &Job, _: usize) -> String {
            String::new()
        }
    }

    fn c(s: &str) -> Call {
        crate::calls::parse(s).unwrap().remove(0)
    }

    #[test]
    fn the_state_reads_back_as_written() {
        let s = State {
            root: ".".into(),
            open: vec![Open { path: "a b/turn.rs".into(), focus: Some((40, 90)) }, Open { path: "README.md".into(), focus: None }],
            edits: vec![Edit { path: "README.md".into(), old: "~~~ fence\nline\n".into(), new: "\\~~~ x".into() }],
        };
        assert_eq!(State::parse(&s.to_text()), Ok(s.clone()));
        assert!(s.to_text().starts_with("kind: file\nroot: .\nopen: a b/turn.rs 40-90\nopen: README.md\nedit: README.md\n~~~ old\n\\~~~ fence"));
        assert!(State::parse("kind: file\nwhat: x").is_err());
    }

    #[test]
    fn the_tabs_are_the_open_files_from_the_repo() {
        let mut s = State::parse("kind: file\nroot: crates/core\nopen: src/lib.rs\nopen: Cargo.toml").unwrap();
        assert_eq!(s.tabs(), ["crates/core/src/lib.rs", "crates/core/Cargo.toml"]);
        s.root = ".".into();
        assert_eq!(s.tabs(), ["src/lib.rs", "Cargo.toml"]);
        s.root = "/elsewhere".into();
        assert!(s.tabs().is_empty());
    }

    #[test]
    fn paths_stay_inside_the_root_and_secrets_stay_shut() {
        assert_eq!(check_path(".", "./crates//core/lib.rs"), Ok("crates/core/lib.rs".into()));
        assert!(check_path(".", "../x").unwrap_err().contains("`..` is not allowed"));
        assert!(check_path(".", "/etc/passwd").unwrap_err().contains("outside the root"));
        assert_eq!(check_path("/repo", "/repo/a.rs"), Ok("a.rs".into()));
        assert!(check_path(".", "config/.env").unwrap_err().contains("secrets"));
        assert!(secret_file("deploy/server.pem") && secret_file(".env.local") && !secret_file("environment.rs"));
    }

    #[test]
    fn edits_are_laid_over_and_marked() {
        let e1 = Edit { path: "f".into(), old: "b".into(), new: "B1\nB2".into() };
        let e2 = Edit { path: "f".into(), old: "d".into(), new: "D".into() };
        let (text, marks) = apply("a\nb\nc\nd", &[&e1, &e2]).unwrap();
        assert_eq!(text, "a\nB1\nB2\nc\nD");
        assert_eq!(marks, [false, true, true, false, true]);
        assert_eq!(apply("a", &[&e1]), Err(1));
    }

    #[test]
    fn tools_change_the_state_with_short_results() {
        let disk = FakeDisk::with(&[("src/main.rs", "fn main() {\n    hi();\n}\n"), ("logo.png", "\0png"), ("notes.md", "one\ntwo\nthree\n")]);
        let mut text = "kind: file\nroot: .".to_string();
        assert_eq!(call(&mut text, &c("open(path=\"src/main.rs\")"), &disk), Ok("opened src/main.rs (3 lines)".into()));
        assert!(call(&mut text, &c("open(path=\"logo.png\")"), &disk).unwrap().contains("an image"));
        assert!(call(&mut text, &c("open(path=\"gone.rs\")"), &disk).unwrap_err().contains("no file gone.rs"));
        assert_eq!(call(&mut text, &c("focus(path=\"notes.md\", from=2, to=3)"), &disk), Ok("focus notes.md 2-3 (3 lines)".into()));
        assert!(text.contains("open: notes.md 2-3"));
        // Edits: open first, the old text exactly once.
        assert!(call(&mut text, &c("edit(path=\"other.rs\", old=\"x\", new=\"y\")"), &disk).unwrap_err().contains("is not open"));
        assert!(call(&mut text, &c("edit(path=\"src/main.rs\", old=\"bye\", new=\"y\")"), &disk).unwrap_err().contains("not in src/main.rs"));
        assert_eq!(
            call(&mut text, &c("edit(path=\"src/main.rs\", old=\"hi();\", new=\"hello();\")"), &disk),
            Ok("edited src/main.rs (unsaved: 1 edit; save(path=\"src/main.rs\") writes it)".into())
        );
        assert_eq!(disk.files.borrow()["src/main.rs"], "fn main() {\n    hi();\n}\n", "unsaved: the disk is unchanged");
        assert_eq!(call(&mut text, &c("save(path=\"src/main.rs\")"), &disk), Ok("saved src/main.rs (1 edit, 3 lines)".into()));
        assert_eq!(disk.files.borrow()["src/main.rs"], "fn main() {\n    hello();\n}\n");
        assert!(call(&mut text, &c("save(path=\"src/main.rs\")"), &disk).unwrap_err().contains("no unsaved edits"));
        assert!(call(&mut text, &c("open(path=\"../up\")"), &disk).unwrap_err().contains("`..`"));
    }

    #[test]
    fn an_edit_with_no_old_text_makes_a_new_file_and_never_replaces_one_that_holds_text() {
        let disk = FakeDisk::with(&[("README.md", "# hi\n"), ("empty.txt", "")]);
        let mut text = "kind: file\nroot: .".to_string();
        // A new file: not opened first, its whole text in `new`, unsaved until save.
        assert_eq!(
            call(&mut text, &c("edit(path=\"src/tree.py\", new=\"n = 3\nprint(n)\n\")"), &disk),
            Ok("new file src/tree.py (2 lines, unsaved; save(path=\"src/tree.py\") writes it)".into())
        );
        assert!(!disk.files.borrow().contains_key("src/tree.py"), "unsaved: nothing on disk yet");
        let snap = snapshot(&text, &disk);
        assert!(snap.contains("#### src/tree.py — 2 lines, 1 unsaved edit (lines marked *)\n    1* n = 3\n    2* print(n)"), "{snap}");
        // It reads back from its saved state, and a later edit lays over it.
        assert_eq!(State::parse(&text).unwrap().edits[0], Edit { path: "src/tree.py".into(), old: String::new(), new: "n = 3\nprint(n)\n".into() });
        call(&mut text, &c("edit(path=\"src/tree.py\", old=\"n = 3\", new=\"n = 4\")"), &disk).unwrap();
        assert_eq!(call(&mut text, &c("save(path=\"src/tree.py\")"), &disk), Ok("saved src/tree.py (2 edits, 2 lines)".into()));
        assert_eq!(disk.files.borrow()["src/tree.py"], "n = 4\nprint(n)\n");
        // An empty file takes a whole text too; one that holds text never does.
        assert!(call(&mut text, &c("edit(path=\"empty.txt\", new=\"x\")"), &disk).unwrap().starts_with("new file empty.txt (1 line"));
        let why = call(&mut text, &c("edit(path=\"README.md\", new=\"gone\")"), &disk).unwrap_err();
        assert!(why.contains("README.md already holds text") && why.contains("old=\"...\""), "{why}");
        let why = call(&mut text, &c("edit(path=\"src/tree.py\", new=\"again\")"), &disk).unwrap_err();
        assert!(why.contains("already holds text"), "{why}");
        assert!(call(&mut text, &c("edit(path=\"x.py\")"), &disk).unwrap_err().contains("for a new file, only new="));
        // A file that is gone and was never being made still says so.
        assert!(call(&mut text, &c("save(path=\"x.py\")"), &disk).unwrap_err().contains("no unsaved edits"));
    }

    #[test]
    fn a_stale_edit_is_dropped_at_save_and_never_blocks() {
        let disk = FakeDisk::with(&[("a.txt", "one\n")]);
        let mut text = "kind: file\nroot: .".to_string();
        call(&mut text, &c("open(path=\"a.txt\")"), &disk).unwrap();
        call(&mut text, &c("edit(path=\"a.txt\", old=\"one\", new=\"1\")"), &disk).unwrap();
        disk.files.borrow_mut().insert("a.txt".into(), "changed\n".into());
        assert!(snapshot(&text, &disk).contains("unsaved edit 1 of a.txt no longer applies"));
        let why = call(&mut text, &c("save(path=\"a.txt\")"), &disk).unwrap_err();
        assert!(why.contains("nothing saved") && why.contains("dropped"), "{why}");
        assert!(!text.contains("edit:"));
    }

    #[test]
    fn opening_past_the_most_closes_the_oldest() {
        let names: Vec<String> = (0..=MAX_OPEN).map(|i| format!("f{i}")).collect();
        let disk = FakeDisk::with(&names.iter().map(|n| (n.as_str(), "x")).collect::<Vec<_>>());
        let mut text = "kind: file".to_string();
        for n in &names[..MAX_OPEN] {
            call(&mut text, &c(&format!("open(path=\"{n}\")")), &disk).unwrap();
        }
        let last = call(&mut text, &c(&format!("open(path=\"{}\")", names[MAX_OPEN])), &disk).unwrap();
        assert!(last.ends_with("; closed f0 (8 open at most)"), "{last}");
    }

    #[test]
    fn the_snapshot_lists_marks_numbers_and_focuses() {
        let disk = FakeDisk::with(&[("a.rs", "l1\nl2\nl3\nl4\n"), ("b.rs", "x\n"), (".env", "KEY=1"), ("pic.png", "\0")]);
        let mut text = "kind: file\nroot: .".to_string();
        assert!(snapshot(&text, &disk).ends_with("No files open: open(path=\"...\") shows one here; edit(path=\"...\", new=\"...\") makes a new one."));
        call(&mut text, &c("focus(path=\"a.rs\", from=2, to=3)"), &disk).unwrap();
        call(&mut text, &c("open(path=\"pic.png\")"), &disk).unwrap();
        call(&mut text, &c("edit(path=\"a.rs\", old=\"l2\", new=\"L2\")"), &disk).unwrap();
        let snap = snapshot(&text, &disk);
        assert!(snap.starts_with("root: . — files (* open):\n* a.rs\n  b.rs\n* pic.png"), "{snap}");
        assert!(!snap.contains(".env"), "a secret file is not even listed");
        assert!(snap.contains("#### a.rs — 4 lines, lines 2-3 shown, 1 unsaved edit (lines marked *)\n    2* L2\n    3  l3\n"), "{snap}");
        assert!(!snap.contains("l1") && !snap.contains("l4"), "only the focus");
        assert!(snap.contains("#### pic.png — an image, 0 KB: not shown"), "{snap}");
    }

    #[test]
    fn one_open_call_opens_several_files() {
        let disk = FakeDisk::with(&[("src/main.rs", "fn main() {\n}\n// end\n"), ("a.txt", "hi\n")]);
        let mut text = "kind: file\nroot: .\n".to_string();
        let out = call(&mut text, &c("open(path=\"src/main.rs|a.txt|gone.rs\")"), &disk).unwrap();
        assert!(out.starts_with("opened src/main.rs (3 lines); opened a.txt") && out.contains("gone.rs: error: no file gone.rs"), "{out}");
        let s = State::parse(&text).unwrap();
        assert_eq!(s.open.iter().map(|o| o.path.as_str()).collect::<Vec<_>>(), ["src/main.rs", "a.txt"]);
        assert!(call(&mut text, &c("open(path=\"x|y\")"), &disk).is_err(), "none opened: an error");
    }

}
