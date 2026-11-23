//! The workbench: what the `explorer`, `editor` and `terminal` components
//! show, so a page can be laid out like a code editor (see `routes/code/`).
//!
//! ```text
//! ┌──┬──────────┬──────────────────────────────┐
//! │⌂ │ EXPLORER │ main.rs ×  lib.rs ×          │  ← tabs
//! │  │ ▾ crates │  1  mod frame;               │
//! │  │   ▸ core │  2  mod net;                 │  ← the open file, read-only
//! │  │   main.rs├──────────────────────────────┤
//! │  │          │ $ cargo test -q              │  ← terminal: Enter runs it
//! │  │          │ test result: ok.             │
//! └──┴──────────┴──────────────────────────────┘
//! ```
//!
//! Three parts, one value (`Workbench`):
//!
//! - `Tree`: the repo's folders, read lazily. A folder is read the first
//!   time it is opened (`toggle` says when); `rows` is what the explorer
//!   draws, top to bottom.
//! - Tabs: the files opened from the tree (`OpenFile`), one active.
//! - `Terminal`: the commands typed and what they printed. A command runs
//!   only when the person presses Enter (`submit`); nothing here starts one.
//!
//! Pure: reading folders and files and running commands is the shell's job.
//! Paths are relative to the repo root, `/`-separated; `""` is the root.

use std::collections::{BTreeMap, BTreeSet};

/// Names the tree leaves out: build output and version-control insides,
/// big and never what you want to read.
pub const HIDDEN: &[&str] = &[".git", "target", "node_modules", ".DS_Store"];
/// The largest file the editor opens, in bytes.
pub const MAX_OPEN: usize = 1_000_000;
/// The most lines one command's output keeps (the last ones).
pub const MAX_OUTPUT_LINES: usize = 2_000;
/// The most commands the terminal keeps (the last ones).
pub const MAX_RUNS: usize = 50;
/// How many spaces a tab is drawn as.
const TAB: &str = "    ";

/// One file or folder in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// From the repo root: `crates/core`.
    pub path: String,
    pub dir: bool,
}

/// `dir` + `name` as a path: `("", "a")` → `a`, `("a", "b")` → `a/b`.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }
}

/// Is `path` inside the repo? Relative, and no `..` or empty segment, so
/// the shell can join it to the root without leaving it.
pub fn safe(path: &str) -> bool {
    path.is_empty() || (!path.starts_with('/') && path.split('/').all(|s| !s.is_empty() && s != "." && s != ".."))
}

/// The folder tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    /// Folder path → what is in it, folders first, then by name.
    children: BTreeMap<String, Vec<Entry>>,
    /// Folder path → why it could not be read.
    errors: BTreeMap<String, String>,
    /// The folders shown open. The root always is.
    open: BTreeSet<String>,
}

/// One line of the explorer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Entry { entry: Entry, depth: usize, open: bool },
    /// A folder that could not be read, or is being read.
    Note { text: String, depth: usize },
}

impl Tree {
    /// Has `dir` been read?
    pub fn is_loaded(&self, dir: &str) -> bool {
        self.children.contains_key(dir) || self.errors.contains_key(dir)
    }

    pub fn is_open(&self, dir: &str) -> bool {
        dir.is_empty() || self.open.contains(dir)
    }

    /// What was read in `dir`: (name, is a folder) pairs, in any order.
    /// Hidden names are left out; folders come first, then by name
    /// ignoring case, like every file browser.
    pub fn set_children(&mut self, dir: &str, found: Vec<(String, bool)>) {
        let mut entries: Vec<Entry> = found
            .into_iter()
            .filter(|(name, _)| !HIDDEN.contains(&name.as_str()))
            .map(|(name, d)| Entry { path: join(dir, &name), name, dir: d })
            .collect();
        entries.sort_by_key(|e| (!e.dir, e.name.to_lowercase()));
        self.errors.remove(dir);
        self.children.insert(dir.to_string(), entries);
    }

    /// `dir` could not be read: shown under it instead of its entries.
    pub fn set_error(&mut self, dir: &str, why: String) {
        self.children.remove(dir);
        self.errors.insert(dir.to_string(), why);
    }

    /// Open a closed folder, close an open one. True when it was just
    /// opened and has not been read: the shell reads it then.
    pub fn toggle(&mut self, dir: &str) -> bool {
        if dir.is_empty() {
            return !self.is_loaded(dir);
        }
        if !self.open.remove(dir) {
            self.open.insert(dir.to_string());
            return !self.is_loaded(dir);
        }
        false
    }

    /// Forget what was read, keeping which folders are open, so the next
    /// look reads the disk again (after a command changed it).
    pub fn forget(&mut self) {
        self.children.clear();
        self.errors.clear();
    }

    /// The open folders that are not read yet: what the shell reads before drawing.
    pub fn to_read(&self) -> Vec<String> {
        std::iter::once(String::new()).chain(self.open.iter().cloned()).filter(|d| !self.is_loaded(d)).collect()
    }

    /// The explorer's lines: the root's entries, and under each open
    /// folder its own, indented one more.
    pub fn rows(&self) -> Vec<Row> {
        fn go(tree: &Tree, dir: &str, depth: usize, out: &mut Vec<Row>) {
            if let Some(why) = tree.errors.get(dir) {
                out.push(Row::Note { text: why.clone(), depth });
                return;
            }
            let Some(entries) = tree.children.get(dir) else {
                out.push(Row::Note { text: "Reading…".into(), depth });
                return;
            };
            for e in entries {
                let open = e.dir && tree.is_open(&e.path);
                out.push(Row::Entry { entry: e.clone(), depth, open });
                if open {
                    go(tree, &e.path, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        go(self, "", 0, &mut out);
        out
    }
}

/// A file opened in the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFile {
    pub path: String,
    /// Its lines, tabs drawn as spaces; or why it cannot be shown.
    pub lines: Result<Vec<String>, String>,
    /// The line the cursor is on (from 0): the one clicked last.
    pub cursor: usize,
}

impl OpenFile {
    /// `bytes` read from `path`; `size` is the whole file's size (the shell
    /// reads at most `MAX_OPEN + 1` bytes). Too big, or not text (a zero
    /// byte, or not UTF-8), shows a line saying so instead.
    pub fn new(path: &str, bytes: &[u8], size: u64) -> OpenFile {
        let lines = if size > MAX_OPEN as u64 {
            Err(format!("Too large to show ({} KB; the editor opens up to {} KB).", size / 1000, MAX_OPEN / 1000))
        } else if bytes.contains(&0) {
            Err("A binary file: nothing to show as text.".to_string())
        } else {
            match std::str::from_utf8(bytes) {
                Ok(text) => Ok(text.lines().map(|l| l.replace('\t', TAB)).collect()),
                Err(_) => Err("Not UTF-8 text: nothing to show.".to_string()),
            }
        };
        OpenFile { path: path.to_string(), lines, cursor: 0 }
    }

    /// The file could not be read at all.
    pub fn failed(path: &str, why: String) -> OpenFile {
        OpenFile { path: path.to_string(), lines: Err(why), cursor: 0 }
    }

    /// The tab's label: the file's name.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// How many lines it has (0 when it cannot be shown).
    pub fn len(&self) -> usize {
        self.lines.as_ref().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Its language, by extension, as an editor's status bar names it.
    pub fn language(&self) -> &'static str {
        language(self.name())
    }

    /// How many digits the line numbers need, so they line up.
    pub fn gutter(&self) -> usize {
        self.len().max(1).to_string().len()
    }
}

/// The language a file name is written in, by its extension (or its
/// whole name), as an editor's status bar names it.
pub fn language(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map_or(name, |(_, e)| e).to_lowercase();
    match ext.as_str() {
        "rs" => "Rust",
        "md" => "Markdown",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "sh" | "zsh" | "bash" => "Shell Script",
        "py" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" => "TypeScript",
        "html" => "HTML",
        "css" => "CSS",
        "swift" => "Swift",
        "lock" => "Lockfile",
        _ => "Plain Text",
    }
}

/// A short mark (one or two characters) for a file in the explorer and on its tab, by
/// its kind. Plain text glyphs, so any font draws them.
pub fn glyph(name: &str) -> &'static str {
    match language(name) {
        "Rust" => "rs",
        "Markdown" => "md",
        "JSON" => "{}",
        "TOML" | "YAML" | "Lockfile" => "⚙",
        "Shell Script" => "$",
        "Python" | "JavaScript" | "TypeScript" | "Swift" => "λ",
        "HTML" | "CSS" => "<>",
        _ => "≡",
    }
}

/// Where one command is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    Running,
    /// It ended with this exit code (`None`: stopped by a signal).
    Exited(Option<i32>),
    /// It could not be started.
    Failed(String),
}

/// One command typed into the terminal and what it printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub id: usize,
    pub command: String,
    pub output: Vec<String>,
    pub state: RunState,
}

/// What Enter did with the line typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Submit {
    /// A blank line: nothing.
    Nothing,
    /// `clear`: the history is emptied; nothing runs.
    Cleared,
    /// The shell runs `command` and calls `finish` with this id.
    Run { id: usize, command: String },
}

/// The terminal's history.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terminal {
    runs: Vec<Run>,
    next: usize,
}

impl Terminal {
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// Is a command still running?
    pub fn busy(&self) -> bool {
        self.runs.iter().any(|r| r.state == RunState::Running)
    }

    /// The person pressed Enter on `line`. The only way a command starts.
    pub fn submit(&mut self, line: &str) -> Submit {
        let command = line.trim();
        if command.is_empty() {
            return Submit::Nothing;
        }
        if command == "clear" {
            self.runs.retain(|r| r.state == RunState::Running);
            return Submit::Cleared;
        }
        let id = self.next;
        self.next += 1;
        self.runs.push(Run { id, command: command.to_string(), output: Vec::new(), state: RunState::Running });
        if self.runs.len() > MAX_RUNS {
            self.runs.remove(0);
        }
        Submit::Run { id, command: command.to_string() }
    }

    /// Command `id` ended: what it printed and its exit code, or why it
    /// could not start. Only the last `MAX_OUTPUT_LINES` lines are kept.
    pub fn finish(&mut self, id: usize, result: Result<(String, Option<i32>), String>) {
        let Some(run) = self.runs.iter_mut().find(|r| r.id == id) else { return };
        match result {
            Ok((output, code)) => {
                run.output = kept(&output);
                run.state = RunState::Exited(code);
            }
            Err(why) => run.state = RunState::Failed(why),
        }
    }

    /// Command `id` still runs and has printed `output` so far (a shell
    /// that streams shows it as it comes; `finish` has the last word).
    /// False when nothing changed, or it is not running.
    pub fn show(&mut self, id: usize, output: &str) -> bool {
        let Some(run) = self.runs.iter_mut().find(|r| r.id == id && r.state == RunState::Running) else { return false };
        let lines = kept(output);
        if run.output == lines {
            return false;
        }
        run.output = lines;
        true
    }
}

/// What a command printed, as the terminal keeps it: its last
/// `MAX_OUTPUT_LINES` lines (saying how many were cut), tabs as spaces.
fn kept(output: &str) -> Vec<String> {
    let lines: Vec<&str> = output.lines().collect();
    let cut = lines.len().saturating_sub(MAX_OUTPUT_LINES);
    let mut out = Vec::new();
    if cut > 0 {
        out.push(format!("… {cut} earlier lines not kept"));
    }
    out.extend(lines[cut..].iter().map(|l| l.replace('\t', TAB)));
    out
}

/// Everything the three components share.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Workbench {
    pub tree: Tree,
    pub tabs: Vec<OpenFile>,
    pub active: Option<usize>,
    pub terminal: Terminal,
}

impl Workbench {
    /// The file shown in the editor.
    pub fn current(&self) -> Option<&OpenFile> {
        self.tabs.get(self.active?)
    }

    /// The tab showing `path`, if one does.
    pub fn tab_of(&self, path: &str) -> Option<usize> {
        self.tabs.iter().position(|f| f.path == path)
    }

    /// Show `file`: in its tab if it has one (with what was just read), else in a new tab at the end.
    pub fn open(&mut self, file: OpenFile) {
        let i = match self.tab_of(&file.path) {
            Some(i) => {
                self.tabs[i] = file;
                i
            }
            None => {
                self.tabs.push(file);
                self.tabs.len() - 1
            }
        };
        self.active = Some(i);
    }

    /// Put the cursor of the file shown on line `n` (from 0), within it.
    pub fn set_cursor(&mut self, n: usize) {
        if let Some(file) = self.active.and_then(|i| self.tabs.get_mut(i)) {
            file.cursor = n.min(file.len().saturating_sub(1));
        }
    }

    pub fn focus(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = Some(i);
        }
    }

    /// Close tab `i`. Closing the active one shows its neighbour (the one
    /// after it, else before); closing another keeps the same file shown.
    pub fn close(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > i => Some(a - 1),
            Some(a) if a == i => Some(i.min(self.tabs.len() - 1)),
            other => other,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(names: &[(&str, bool)]) -> Vec<(String, bool)> {
        names.iter().map(|(n, d)| (n.to_string(), *d)).collect()
    }

    #[test]
    fn folders_come_first_hidden_names_are_left_out_and_open_folders_nest() {
        let mut tree = Tree::default();
        assert_eq!(tree.to_read(), vec![String::new()], "the root is read first");
        tree.set_children("", found(&[("README.md", false), ("target", true), ("crates", true), (".git", true), ("Cargo.toml", false)]));
        let names = |tree: &Tree| -> Vec<String> {
            tree.rows()
                .iter()
                .map(|r| match r {
                    Row::Entry { entry, depth, .. } => format!("{}{}", "  ".repeat(*depth), entry.name),
                    Row::Note { text, depth } => format!("{}({text})", "  ".repeat(*depth)),
                })
                .collect()
        };
        assert_eq!(names(&tree), ["crates", "Cargo.toml", "README.md"]);
        assert!(tree.toggle("crates"), "opened and not read: the shell reads it");
        assert_eq!(names(&tree), ["crates", "  (Reading…)", "Cargo.toml", "README.md"]);
        tree.set_children("crates", found(&[("core", true)]));
        assert_eq!(names(&tree)[1], "  core");
        assert!(!tree.toggle("crates"), "closing reads nothing");
        assert_eq!(names(&tree).len(), 3);
        assert!(!tree.toggle("crates"), "opening again: already read");
        tree.set_error("crates", "denied".into());
        assert_eq!(names(&tree)[1], "  (denied)");
        tree.forget();
        assert_eq!(tree.to_read(), vec![String::new(), "crates".to_string()], "the open folders are read again");
    }

    #[test]
    fn a_file_has_a_language_and_a_mark_and_the_cursor_stays_inside_it() {
        assert_eq!((language("main.rs"), language("README.MD"), language("Makefile")), ("Rust", "Markdown", "Plain Text"));
        assert_eq!((glyph("page.json"), glyph("notes.txt")), ("{}", "≡"));
        let mut wb = Workbench::default();
        wb.set_cursor(3);
        wb.open(OpenFile::new("a.rs", b"1\n2\n3\n", 6));
        assert_eq!(wb.current().unwrap().cursor, 0);
        wb.set_cursor(1);
        assert_eq!(wb.current().unwrap().cursor, 1);
        wb.set_cursor(99);
        assert_eq!(wb.current().unwrap().cursor, 2, "past the end: the last line");
    }

    #[test]
    fn only_paths_inside_the_repo_are_safe() {
        assert!(safe("") && safe("crates/core") && safe(".github/x"));
        assert!(!safe("/etc") && !safe("../x") && !safe("a/../b") && !safe("a//b") && !safe("./a"));
        assert_eq!((join("", "a"), join("a", "b")), ("a".to_string(), "a/b".to_string()));
    }

    #[test]
    fn a_text_file_opens_as_lines_and_a_binary_or_huge_one_says_why() {
        let f = OpenFile::new("src/main.rs", b"fn main() {\n\tdry();\n}\n", 22);
        assert_eq!(f.lines.as_ref().unwrap(), &["fn main() {", "    dry();", "}"]);
        assert_eq!((f.name(), f.len(), f.gutter()), ("main.rs", 3, 1));
        assert!(OpenFile::new("a.png", b"\x89PNG\0\0", 6).lines.unwrap_err().contains("binary"));
        assert!(OpenFile::new("big.log", b"x", 5_000_000).lines.unwrap_err().contains("Too large"));
        assert!(OpenFile::new("bad", &[0xff, 0xfe], 2).lines.is_err());
        assert_eq!(OpenFile::new("e", b"", 0).gutter(), 1);
    }

    #[test]
    fn opening_a_file_twice_reuses_its_tab_and_closing_shows_a_neighbour() {
        let mut wb = Workbench::default();
        for p in ["a", "b", "c"] {
            wb.open(OpenFile::new(p, b"x", 1));
        }
        assert_eq!(wb.current().unwrap().path, "c");
        wb.open(OpenFile::new("a", b"new", 3));
        assert_eq!((wb.tabs.len(), wb.active), (3, Some(0)));
        assert_eq!(wb.current().unwrap().lines.as_ref().unwrap(), &["new"], "read again");
        wb.close(0);
        assert_eq!(wb.current().unwrap().path, "b", "the one after it");
        wb.focus(1);
        wb.close(0);
        assert_eq!(wb.current().unwrap().path, "c", "closing another keeps the shown file");
        wb.close(0);
        assert_eq!(wb.current(), None);
    }

    #[test]
    fn a_command_runs_only_on_enter_and_keeps_its_output() {
        let mut term = Terminal::default();
        assert_eq!(term.submit("   "), Submit::Nothing);
        assert!(term.runs().is_empty(), "nothing runs by itself");
        let Submit::Run { id, command } = term.submit(" ls -a ") else { panic!() };
        assert_eq!(command, "ls -a");
        assert!(term.busy());
        term.finish(id, Ok(("a\n\tb\n".into(), Some(0))));
        assert_eq!(term.runs()[0].output, ["a", "    b"]);
        assert_eq!(term.runs()[0].state, RunState::Exited(Some(0)));
        let Submit::Run { id, .. } = term.submit("nope") else { panic!() };
        term.finish(id, Err("not found".into()));
        assert_eq!(term.runs()[1].state, RunState::Failed("not found".into()));
        assert_eq!(term.submit("clear"), Submit::Cleared);
        assert!(term.runs().is_empty());
    }

    #[test]
    fn long_output_and_long_histories_keep_the_newest() {
        let mut term = Terminal::default();
        let Submit::Run { id, .. } = term.submit("yes") else { panic!() };
        let output: String = (0..MAX_OUTPUT_LINES + 5).map(|n| format!("{n}\n")).collect();
        term.finish(id, Ok((output, Some(0))));
        let out = &term.runs()[0].output;
        assert_eq!(out.len(), MAX_OUTPUT_LINES + 1);
        assert!(out[0].contains("5 earlier lines"));
        assert_eq!(out.last().unwrap(), &(MAX_OUTPUT_LINES + 4).to_string());
        for n in 0..MAX_RUNS + 3 {
            term.submit(&format!("echo {n}"));
        }
        assert_eq!(term.runs().len(), MAX_RUNS);
        assert_eq!(term.runs().last().unwrap().command, format!("echo {}", MAX_RUNS + 2));
    }
}
