//! `kind: shared`: the thread, one conversation shared by you and every
//! engine under one head (a top-level engine), which is how they talk to
//! each other.
//!
//! A thread belongs to a root (a top-level engine, the head). Every engine
//! in the head's whole tree (its children, theirs, sessions and their
//! child engines: any engine that is a tool of any engine there) is on the
//! head's one thread, and so are you. Each engine that holds a
//! `kind: shared` artifact sees the thread in its artifacts slot, as it is
//! now (a snapshot, never in history, never compressed), and may add to it.
//!
//! ```text
//! state/engines/<root>/shared.md     the thread of <root> and its tree
//!
//! - 1795356330 helper: The failing test is turn.rs:40; the fix is a missing trim.
//! - 1795356400 you → tester: Run the suite again.
//! ```
//!
//! One line per entry: when (unix seconds), who wrote it (`you`, an engine
//! id, or a read-only participant such as a session's sub-agent), who it is
//! for when it is addressed (`helper → tester: ...`), and the text (a line
//! break written as `\n`).
//!
//! **A thread is a list of messages.** An unaddressed post is for the head;
//! an addressed one for the engine it names, and a post for an engine calls
//! it: its turn starts with the post (`crate::desk::guided`, docs/desk.md).
//! So invoking any engine of a tree is a post addressed to it on its head's
//! thread. Posts are never refused for room: the thread keeps every entry,
//! and `MAX_CHARS` is how much of the newest a snapshot shows (it says how
//! many older ones it left out). An entry is at most `MAX_ENTRY` characters.
//!
//! Stores from before threads kept one file per level (`state/shared.md`
//! for the top, `state/engines/<parent>/shared.md` for a parent's
//! children). `fold` moves them into their roots' threads on load.
//!
//! Tools (results are one line; the entries are in the snapshot):
//! - `post(text, to?)` → `posted entry 3 (420 characters on the thread)`
//! - `edit_post(n, text)` → `edited entry 2 (...)`, or with `text=""`
//!   `removed entry 2 (...)`; only an entry you wrote.
//!
//! Pure: texts in, texts out. The shell keeps the files.

use crate::state::State;

/// The tools a shared artifact brings.
pub const TOOLS: [&str; 2] = ["post", "edit_post"];
/// Most characters of a thread's entries (each entry's author and text) a
/// snapshot shows: the newest that fit. The thread keeps all of them.
pub const MAX_CHARS: usize = 6_000;
/// Most characters of one entry's text.
pub const MAX_ENTRY: usize = 600;

/// One entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub at: i64,
    pub author: String,
    /// Who it is for; `None`: the thread's head.
    pub to: Option<String>,
    pub text: String,
}

/// A thread's entries, from its `shared.md`. A line that is not an entry
/// is skipped.
pub fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("- ")?;
            let (at, rest) = rest.split_once(' ')?;
            let (who, text) = rest.split_once(": ")?;
            let (author, to) = match who.split_once(" → ") {
                Some((a, t)) => (a, Some(t.trim().to_string()).filter(|t| !t.is_empty())),
                None => (who, None),
            };
            Some(Entry { at: at.parse().ok()?, author: author.to_string(), to, text: unescape(text) })
        })
        .collect()
}

pub fn to_text(entries: &[Entry]) -> String {
    entries.iter().map(|e| format!("- {} {}: {}\n", e.at, who(e), escape(&e.text))).collect()
}

/// `author`, or `author → to` for an addressed entry.
pub fn who(e: &Entry) -> String {
    match &e.to {
        Some(to) => format!("{} → {to}", e.author),
        None => e.author.clone(),
    }
}

/// Characters the entries use (what the cap counts).
pub fn used(entries: &[Entry]) -> usize {
    entries.iter().map(|e| e.author.chars().count() + e.text.chars().count()).sum()
}

/// A change to a thread, as one engine's call (or you) asked it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Post {
        author: String,
        at: i64,
        /// Who it is for; `None`: the thread's head.
        to: Option<String>,
        text: String,
    },
    /// Change entry `n` (1-based) to `text`; empty text removes it.
    Change { author: String, n: usize, text: String },
}

/// Apply `edit` to a thread's `text`: the new text and the
/// result line, or why not.
pub fn apply(text: &str, edit: &Edit) -> Result<(String, String), String> {
    let mut entries = parse(text);
    let room = |e: &[Entry]| format!("{} characters on the thread", used(e));
    let check = |t: &str| {
        if t.chars().count() > MAX_ENTRY {
            Err(format!("an entry is at most {MAX_ENTRY} characters; this is {}", t.chars().count()))
        } else {
            Ok(())
        }
    };
    let done = match edit {
        Edit::Post { author, at, to, text } => {
            let text = text.trim();
            if text.is_empty() {
                return Err("post needs text=\"...\"".into());
            }
            check(text)?;
            let to = to.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
            entries.push(Entry { at: *at, author: author.clone(), to, text: text.to_string() });
            format!("posted entry {}", entries.len())
        }
        Edit::Change { author, n, text } => {
            let i = n.checked_sub(1).filter(|i| *i < entries.len()).ok_or_else(|| format!("no entry {n}; there are {}", entries.len()))?;
            if entries[i].author != *author {
                return Err(format!("entry {n} is {}'s; you can edit only yours", entries[i].author));
            }
            let text = text.trim();
            check(text)?;
            if text.is_empty() {
                entries.remove(i);
                format!("removed entry {n}")
            } else {
                entries[i].text = text.to_string();
                format!("edited entry {n}")
            }
        }
    };
    Ok((to_text(&entries), format!("{done} ({})", room(&entries))))
}

/// `11-22 14:05` on a clock `offset` seconds east of UTC.
pub fn when(at: i64, offset: i32) -> String {
    let c = crate::live::Clock::new(at, offset);
    let (_, m, d) = crate::live::civil_from_days(c.day());
    format!("{m:02}-{d:02} {:02}:{:02}", c.minute() / 60, c.minute() % 60)
}

/// One entry, numbered, as a snapshot shows it.
pub fn line(n: usize, e: &Entry, offset: i32) -> String {
    format!("{n}. {} {}: {}", when(e.at, offset), who(e), e.text)
}

/// The entries, numbered: the newest that fit `MAX_CHARS`, and a line
/// saying how many older ones are not shown.
pub fn list(text: &str, offset: i32) -> String {
    list_within(text, offset, MAX_CHARS)
}

/// `list` with another window: the newest entries whose characters fit `max`.
pub fn list_within(text: &str, offset: i32, max: usize) -> String {
    let entries = parse(text);
    if entries.is_empty() {
        return "No entries yet.".into();
    }
    let mut from = entries.len();
    let mut room = max;
    while from > 0 {
        let e = &entries[from - 1];
        let size = e.author.chars().count() + e.text.chars().count();
        if size > room && from < entries.len() {
            break;
        }
        room = room.saturating_sub(size);
        from -= 1;
    }
    let mut lines = Vec::new();
    if from > 0 {
        lines.push(format!("({from} older entries not shown; the thread keeps them all)"));
    }
    lines.extend(entries.iter().enumerate().skip(from).map(|(n, e)| line(n + 1, e, offset)));
    lines.join("\n")
}

/// The shared artifact's snapshot for engine `me`: its thread `text`,
/// whose head is `head` and whose other engines are `others`.
pub fn snapshot(text: &str, me: &str, head: &str, others: &[String], offset: i32) -> String {
    let entries = parse(text);
    let with = if others.is_empty() { "no other engine yet".to_string() } else { others.join(", ") };
    let head = if head == me { "you are its head: unaddressed posts are for you".to_string() } else { format!("its head is {head}: unaddressed posts are for it") };
    format!(
        "Your thread ({head}): you ({me}), the user (posts as `{}`) and the tree's other engines: {with}. Everyone sees it as it is now. post(text, to?) adds an entry (to=\"<engine>\" calls that engine; to=\"you\" is for the user); edit_post(n, text) changes one of yours, text=\"\" removes it. {} entries, {} characters:\n{}",
        crate::desk::YOU,
        entries.len(),
        used(&entries),
        list(text, offset)
    )
}

/// The root of engine `id`'s tree: the top-level engine above it (itself
/// at the top). An engine whose parent is gone is a root.
pub fn root_of(state: &State, id: &str) -> String {
    let mut at = id.to_string();
    let mut seen = vec![at.clone()];
    while let Some(p) = state.engines.iter().find(|e| e.id == at).and_then(|e| e.parent.clone()) {
        if state.index_of(&p).is_none() || seen.contains(&p) {
            break;
        }
        seen.push(p.clone());
        at = p;
    }
    at
}

/// Engine `id` is a root (a head): no parent, or its parent is gone.
pub fn is_root(state: &State, id: &str) -> bool {
    root_of(state, id) == id
}

/// The engines on root `root`'s thread: the root, then its tree, depth
/// first in the state's order. Indexes.
pub fn members(state: &State, root: &str) -> Vec<usize> {
    fn walk(state: &State, id: &str, out: &mut Vec<usize>) {
        for (i, e) in state.engines.iter().enumerate() {
            if e.parent.as_deref() == Some(id) && !out.contains(&i) {
                out.push(i);
                walk(state, &e.id, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(r) = state.index_of(root) {
        out.push(r);
        walk(state, root, &mut out);
    }
    out
}

/// Apply `edit` to the thread engine `id` is on (its root's): the result
/// line, or why not.
pub fn post(state: &mut State, id: &str, edit: &Edit) -> Result<String, String> {
    let root = root_of(state, id);
    let old = state.shared.get(&root).cloned().unwrap_or_default();
    let (new, line) = apply(&old, edit)?;
    state.shared.insert(root, new);
    Ok(line)
}

/// Move the posts of every file that is not a root's thread into one:
/// a deeper engine's (`engines/<non-root>/shared.md`, from before threads)
/// into its root's; the old top file's (`""`) each into the thread of the
/// engine it is for, else of its author, else the translator's (else the
/// first root's). Merged by time, an entry already there kept once; the
/// old files are left empty, so the next save removes them. Every engine
/// on a thread that took posts has heard all of it, so no old post calls
/// anyone. Nothing moves when every file is a root's. With no engine, the
/// top file stays.
pub fn fold(state: &mut State) {
    let roots: Vec<String> = state.engines.iter().filter(|e| is_root(state, &e.id)).map(|e| e.id.clone()).collect();
    let Some(fallback) = roots.iter().find(|r| *r == "translator").or_else(|| roots.first()).cloned() else { return };
    let mut moved: std::collections::BTreeMap<String, Vec<Entry>> = std::collections::BTreeMap::new();
    let keys: Vec<String> = state.shared.keys().filter(|k| !roots.contains(k)).cloned().collect();
    for key in keys {
        let text = state.shared.remove(&key).unwrap_or_default();
        for e in parse(&text) {
            let root = if key.is_empty() {
                let engine = |id: &str| state.index_of(id).map(|_| root_of(state, id));
                e.to.as_deref().and_then(engine).or_else(|| engine(&e.author)).unwrap_or_else(|| fallback.clone())
            } else if state.index_of(&key).is_some() {
                root_of(state, &key)
            } else {
                // The file of an engine that is gone: its posts are dropped
                // with it, as removing an engine does.
                continue;
            };
            moved.entry(root).or_default().push(e);
        }
    }
    for (root, extra) in moved {
        let mut all = parse(state.shared.get(&root).map_or("", String::as_str));
        for e in extra {
            if !all.contains(&e) {
                all.push(e);
            }
        }
        all.sort_by_key(|e| e.at);
        let heard = crate::snapshot::Cursor { n: all.len() };
        state.shared.insert(root.clone(), to_text(&all));
        // Old posts call nobody: every engine on the thread has heard all
        // of it (`crate::desk::guided`), a new one included; it is saved.
        for i in members(state, &root) {
            state.engines[i].heard = Some(heard);
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n")
}

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUNDAY: i64 = 1_795_356_330;

    fn post(text: &str, who: &str, what: &str) -> Result<(String, String), String> {
        apply(text, &Edit::Post { author: who.into(), at: SUNDAY, to: None, text: what.into() })
    }

    #[test]
    fn posts_carry_author_and_time_and_read_back() {
        let (text, r) = post("", "helper", "Found it.\nIn turn.rs.").unwrap();
        assert_eq!(r, "posted entry 1 (27 characters on the thread)");
        assert_eq!(text, "- 1795356330 helper: Found it.\\nIn turn.rs.\n");
        assert_eq!(parse(&text)[0], Entry { at: SUNDAY, author: "helper".into(), to: None, text: "Found it.\nIn turn.rs.".into() });
        let snap = snapshot(&text, "tester", "helper", &["helper".into()], 0);
        assert!(snap.starts_with("Your thread (its head is helper: unaddressed posts are for it): you (tester), the user (posts as `you`) and the tree's other engines: helper."), "{snap}");
        assert!(snap.ends_with("1 entries, 27 characters:\n1. 11-22 14:05 helper: Found it.\nIn turn.rs."), "{snap}");
    }

    #[test]
    fn an_addressed_post_keeps_who_it_is_for() {
        let (text, _) =
            apply("", &Edit::Post { author: "helper".into(), at: SUNDAY, to: Some("tester".into()), text: "your turn".into() }).unwrap();
        assert_eq!(text, "- 1795356330 helper → tester: your turn\n");
        let e = &parse(&text)[0];
        assert_eq!((e.author.as_str(), e.to.as_deref()), ("helper", Some("tester")));
        assert_eq!(list(&text, 0), "1. 11-22 14:05 helper → tester: your turn");
        // An empty `to` is the owner's.
        let (text, _) = apply("", &Edit::Post { author: "a".into(), at: SUNDAY, to: Some(" ".into()), text: "x".into() }).unwrap();
        assert_eq!(parse(&text)[0].to, None);
    }

    #[test]
    fn only_your_own_entry_changes_and_the_thread_keeps_every_post() {
        let (text, _) = post("", "helper", "one").unwrap();
        let change = |t: &str, who: &str, n, what: &str| apply(t, &Edit::Change { author: who.into(), n, text: what.into() });
        assert!(change(&text, "tester", 1, "mine now").unwrap_err().contains("entry 1 is helper's"));
        assert!(change(&text, "helper", 2, "x").unwrap_err().contains("no entry 2; there are 1"));
        let (text, r) = change(&text, "helper", 1, "one, edited").unwrap();
        assert_eq!(r, "edited entry 1 (17 characters on the thread)");
        let mut full = text.clone();
        for _ in 0..12 {
            full = post(&full, "tester", &"x".repeat(MAX_ENTRY)).unwrap().0;
        }
        // Past the window: kept, and the snapshot shows the newest.
        let (full, r) = post(&full, "tester", &"y".repeat(400)).unwrap();
        assert!(r.starts_with("posted entry 14"), "{r}");
        assert_eq!(parse(&full).len(), 14);
        let shown = list(&full, 0);
        assert!(shown.starts_with("(4 older entries not shown; the thread keeps them all)\n5. "), "{}", &shown[..80]);
        assert!(shown.ends_with(&"y".repeat(400)));
        assert!(post(&full, "tester", &"z".repeat(MAX_ENTRY + 1)).unwrap_err().contains("at most 600"));
        let (_, r) = change(&full, "helper", 1, "").unwrap();
        assert!(r.starts_with("removed entry 1"), "{r}");
    }

    #[test]
    fn fold_moves_every_old_level_into_its_roots_thread_once() {
        let mut s = State::default();
        for (id, parent) in [("translator", None), ("helper", Some("translator")), ("deep", Some("helper")), ("other", None)] {
            let i = s.add_engine();
            s.engines[i].id = id.into();
            s.engines[i].parent = parent.map(String::from);
        }
        s.shared.insert("".into(), "- 3 you → other: hi other\n- 1 you → translator: hi\n- 5 you: to nobody\n".into());
        s.shared.insert("translator".into(), "- 2 helper: found it\n".into());
        s.shared.insert("helper".into(), "- 4 deep: done\n- 2 helper: found it\n".into());
        s.shared.insert("ghost".into(), "- 9 ghost: gone\n".into());
        fold(&mut s);
        assert_eq!(s.shared.keys().collect::<Vec<_>>(), ["other", "translator"]);
        let texts = |k: &str| parse(&s.shared[k]).into_iter().map(|e| e.text).collect::<Vec<_>>();
        assert_eq!(texts("translator"), ["hi", "found it", "done", "to nobody"], "by time, a copy kept once");
        assert_eq!(texts("other"), ["hi other"]);
        assert_eq!(s.engines[0].heard, Some(crate::snapshot::Cursor { n: 4 }), "all of it heard");
        for i in 0..s.engines.len() {
            assert!(crate::desk::guided(&mut s, i).is_empty(), "{} was called by an old post", s.engines[i].id);
        }
        let again = s.shared.clone();
        fold(&mut s);
        assert_eq!(s.shared, again, "nothing moves twice");
        assert_eq!((root_of(&s, "deep"), is_root(&s, "other"), members(&s, "translator")), ("translator".to_string(), true, vec![0, 1, 2]));
    }
}
