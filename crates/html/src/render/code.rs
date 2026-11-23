//! The Code page's primitives: `panes` and their bars, the explorer, the
//! editor, the terminal and the theme picker (`crates/gpui/src/views/
//! code.rs`). Panes are flex boxes with the page file's sizes and token
//! colours; the explorer and editor read the repo through
//! `dry_core::workbench`, with the open folders and the file in the URL
//! (`?open=a,b&file=c`), so looking around needs no state on the server.

use std::fmt::Write;

use dry_core::pages::{self, Align, Axis, Bar, Component, Edge, Pane};
use dry_core::workbench::{self, OpenFile, Row, Tree};

use super::{Ctx, esc, note};

/// `panes`: side by side or stacked, each with its size or the room left,
/// its background, text colour, padding, gap and edge from tokens.
/// (Dragging the edges of a `resizable` group is the desktop's for now.)
pub fn panes(out: &mut String, axis: Axis, list: &[Pane], cx: &Ctx) {
    let dir = match axis {
        Axis::Row => "axis-row",
        Axis::Column => "axis-column",
    };
    let _ = write!(out, "<div class=\"panes {dir}\">");
    for pane in list {
        let mut style = String::new();
        match (pane.size, axis) {
            (Some(s), Axis::Row) => {
                let _ = write!(style, "flex:0 0 {s}px;width:{s}px;");
            }
            (Some(s), Axis::Column) => {
                let _ = write!(style, "flex:0 0 {s}px;height:{s}px;");
            }
            (None, _) => style.push_str("flex:1 1 0;"),
        }
        if let Some(bg) = pane.background.as_deref().filter(|n| is_token(n)) {
            let _ = write!(style, "background:var(--{bg});");
        }
        if let Some(fg) = pane.text.as_deref().filter(|n| is_token(n)) {
            let _ = write!(style, "--text_primary:var(--{fg});--text_secondary:var(--{fg});color:var(--{fg});");
        }
        if let Some(p) = pane.padding.as_deref().filter(|n| is_token(n)) {
            let _ = write!(style, "padding:var(--{p});");
        }
        if let (Some(b), Some(edge)) = (pane.border.as_deref().filter(|n| is_token(n)), pane.edge) {
            let side = match edge {
                Edge::Top => "top",
                Edge::Right => "right",
                Edge::Bottom => "bottom",
                Edge::Left => "left",
            };
            let _ = write!(style, "border-{side}:1px solid var(--{b});");
        }
        let _ = write!(out, "<div class=\"pane\" style=\"{style}\">");
        match pane.bar {
            Some(kind) => bar(out, kind, pane.align.unwrap_or_default(), &pane.view, cx),
            None => {
                let gap = pane.gap.as_deref().and_then(|n| cx.t.number(n));
                let centred = axis == Axis::Column && pane.size.is_some();
                if centred {
                    out.push_str("<div class=\"centred\">");
                }
                super::stack(out, &pane.view, gap, cx);
                if centred {
                    out.push_str("</div>");
                }
            }
        }
        out.push_str("</div>");
    }
    out.push_str("</div>");
}

/// A name a page file uses for a token: plain letters, digits and `_`,
/// so it is safe inside `var(--…)`.
fn is_token(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A bar: an activity column of icon cells, or a status strip of items.
fn bar(out: &mut String, kind: Bar, align: Align, view: &[Component], cx: &Ctx) {
    let values = cx.values();
    let class = match (kind, align) {
        (Bar::Activity, _) => "bar activity",
        (Bar::Status, Align::Start) => "bar status",
        (Bar::Status, Align::End) => "bar status end",
    };
    let _ = write!(out, "<div class=\"{class}\">");
    for c in view {
        match (kind, c) {
            (Bar::Activity, Component::Link { label, to }) => {
                let active = cx.location == to;
                let _ = write!(out, "<a class=\"cell{}\" href=\"{}\">{}</a>", if active { " on" } else { "" }, esc(to), esc(label));
            }
            (Bar::Status, Component::Link { label, to }) => {
                let _ = write!(out, "<a class=\"item\" href=\"{}\">{}</a>", esc(to), esc(label));
            }
            (Bar::Status, Component::Text { text }) => {
                // An item naming a value that is blank now (no file open) hides.
                let blank = values.iter().any(|(name, v)| v.is_empty() && text.contains(&format!("{{{name}}}")));
                if !blank {
                    let _ = write!(out, "<span class=\"item\">{}</span>", esc(&pages::fill(text, &values)));
                }
            }
            (Bar::Status, Component::ThemePicker) => theme_item(out, cx),
            _ => {}
        }
    }
    out.push_str("</div>");
}

/// The themes to pick from on this page: the route's own, else the app's;
/// and the one shown (a preview in the URL, else the saved choice).
fn themes(cx: &Ctx) -> (Vec<String>, String, bool) {
    match cx.world.local_of(cx.location).filter(|l| !l.themes.is_empty()) {
        Some(local) => {
            let names: Vec<String> = local.themes.keys().cloned().collect();
            let active = cx.query.theme.clone().filter(|q| names.contains(q)).or_else(|| local.theme().map(String::from)).unwrap_or_default();
            (names, active, true)
        }
        None => (cx.world.themes.clone(), cx.query.theme.clone().unwrap_or_else(|| cx.world.active_theme.clone()), false),
    }
}

/// A link to this page showing theme `name` (a preview, not saved).
fn theme_link(cx: &Ctx, name: &str) -> String {
    let q = super::Query { theme: Some(name.to_string()), ..cx.query.clone() };
    format!("{}{}", cx.location, q.with(&cx.query.open, cx.query.file.as_deref()))
}

/// The status bar's theme item: the theme shown; a click previews the next.
fn theme_item(out: &mut String, cx: &Ctx) {
    let (names, active, _) = themes(cx);
    let next = names.iter().position(|n| *n == active).map_or(0, |i| (i + 1) % names.len().max(1));
    match names.get(next) {
        Some(pick) => {
            let _ = write!(out, "<a class=\"item\" href=\"{}\" title=\"Preview the next theme (not saved)\">◐ {}</a>", esc(&theme_link(cx, pick)), esc(&pages::label(&active)));
        }
        None => {
            let _ = write!(out, "<span class=\"item\">◐ {}</span>", esc(&pages::label(&active)));
        }
    }
}

/// `theme-picker`: one button per theme the route ships (else the app's),
/// each a preview link.
pub fn theme_picker(out: &mut String, cx: &Ctx) {
    let (names, active, _) = themes(cx);
    out.push_str("<div class=\"row\">");
    for n in &names {
        let _ = write!(out, "<a class=\"btn{}\" href=\"{}\">{}</a>", if *n == active { " primary" } else { "" }, esc(&theme_link(cx, n)), esc(&pages::label(n)));
    }
    out.push_str("</div>");
}

/// Read folder `rel` of the repo: (name, is a folder) pairs.
fn read_dir(repo: &dyn crate::world::Repo, rel: &str) -> Result<Vec<(String, bool)>, String> {
    if !workbench::safe(rel) {
        return Err(format!("{rel} is outside the repo"));
    }
    repo.list(rel)
}

/// Open file `rel` for the editor, at most `MAX_OPEN + 1` bytes read.
pub fn open_file(repo: &dyn crate::world::Repo, rel: &str) -> OpenFile {
    if !workbench::safe(rel) || rel.is_empty() {
        return OpenFile::failed(rel, format!("{rel} is outside the repo"));
    }
    match repo.read(rel, workbench::MAX_OPEN + 1) {
        Ok((bytes, size)) => OpenFile::new(rel, &bytes, size),
        Err(e) => OpenFile::failed(rel, e),
    }
}

/// `explorer`: the repo's folder tree (`workbench::Tree`), folders open
/// per the URL; a folder link toggles it, a file link opens it.
pub fn explorer(out: &mut String, cx: &Ctx) {
    let mut tree = Tree::default();
    for dir in &cx.query.open {
        tree.toggle(dir);
    }
    for _ in 0..8 {
        let todo = tree.to_read();
        if todo.is_empty() {
            break;
        }
        for dir in todo {
            match read_dir(cx.world.repo.as_ref(), &dir) {
                Ok(found) => tree.set_children(&dir, found),
                Err(e) => tree.set_error(&dir, e),
            }
        }
    }
    let current = cx.query.file.as_deref();
    let _ = write!(out, "<div class=\"explorer\"><div class=\"title\">EXPLORER</div><div class=\"tree\"><div class=\"tree-repo\">{}</div>", esc(&cx.world.repo_name.to_uppercase()));
    for row in tree.rows() {
        match row {
            Row::Note { text, depth } => {
                let _ = write!(out, "<div class=\"tree-row note\" style=\"--depth:{depth}\">{}</div>", esc(&text));
            }
            Row::Entry { entry, depth, open } => {
                let href = if entry.dir {
                    let mut set = cx.query.open.clone();
                    if open {
                        set.retain(|d| d != &entry.path && !d.starts_with(&format!("{}/", entry.path)));
                    } else {
                        set.push(entry.path.clone());
                    }
                    format!("{}{}", cx.location, cx.query.with(&set, current))
                } else {
                    format!("{}{}", cx.location, cx.query.with(&cx.query.open, Some(&entry.path)))
                };
                let glyph = if entry.dir { if open { "⌄" } else { "›" } } else { workbench::glyph(&entry.name) };
                let _ = write!(
                    out,
                    "<a class=\"tree-row{}{}\" style=\"--depth:{depth}\" href=\"{}\"><span class=\"g\">{}</span><span class=\"trunc\">{}</span></a>",
                    if entry.dir { " dir" } else { "" },
                    if current == Some(entry.path.as_str()) { " on" } else { "" },
                    esc(&href),
                    esc(glyph),
                    esc(&entry.name)
                );
            }
        }
    }
    out.push_str("</div></div>");
}

/// Most lines the editor draws (the window scrolls through them all).
const SHOWN: usize = 4000;

/// `editor`: the file from the URL, read-only, with line numbers.
pub fn editor(out: &mut String, cx: &Ctx) {
    out.push_str("<div class=\"editor\">");
    let Some(path) = cx.query.file.as_deref() else {
        let _ = write!(out, "<div class=\"editor-empty\"><div class=\"big\">{}</div>{}</div></div>", esc(&cx.world.repo_name), note("Open a file from the explorer. Read-only."));
        return;
    };
    let f = open_file(cx.world.repo.as_ref(), path);
    let close = format!("{}{}", cx.location, cx.query.with(&cx.query.open, None));
    let _ = write!(
        out,
        "<div class=\"tabs\"><span class=\"tab on\"><span>{}</span><a href=\"{}\" title=\"Close\">✕</a></span></div><div class=\"crumbs\">{}</div>",
        esc(f.name()),
        esc(&close),
        esc(&f.path.replace('/', " › "))
    );
    match &f.lines {
        Err(why) => out.push_str(&note(why)),
        Ok(lines) => {
            out.push_str("<div class=\"code editor-lines\">");
            for (n, l) in lines.iter().take(SHOWN).enumerate() {
                let _ = write!(out, "<div class=\"ln\"><span class=\"n\">{}</span><span class=\"t\">{}</span></div>", n + 1, esc(l));
            }
            if lines.len() > SHOWN {
                out.push_str(&note(&format!("{} more lines: open it in the desktop app to see them all.", lines.len() - SHOWN)));
            }
            out.push_str("</div>");
        }
    }
    out.push_str("</div>");
}

/// `terminal`: the panel. On the host (`run-terminal`) it is the host's
/// terminal (`Reported::terminal`, `dry_core::workbench::Terminal`, the
/// window's model): every page sees the same commands and output, and a
/// line typed here runs on the host in the repo folder, its output
/// streaming in. Elsewhere its command line is disabled: a read-only web
/// view never runs one, and the browser has no processes at all.
pub fn terminal(out: &mut String, cx: &Ctx) {
    let host = cx.world.reported.terminal.as_ref().filter(|_| cx.world.can("run-terminal"));
    let Some(term) = host else {
        let why = if cx.world.hosted() {
            "No commands from this page yet: the host runs a turn's commands, not typed ones."
        } else if cx.world.live() { "No commands in the browser: it has no processes." } else { "No commands from the web: it only reads." };
        let _ = write!(
            out,
            "<div class=\"terminal\"><div class=\"title\">TERMINAL</div><div class=\"term-body\">{}<div class=\"prompt-line\"><span>$</span><input disabled placeholder=\"commands run from the desktop app\">{}</div></div></div>",
            note(why),
            cx.off("Run", "")
        );
        return;
    };
    let _ = write!(
        out,
        "<div class=\"terminal host-term\"><div class=\"title\"><span class=\"grow\">TERMINAL</span><span class=\"note\">sh · {} · on the host</span></div><div class=\"term-body\">",
        esc(&cx.world.repo_name)
    );
    if term.runs().is_empty() {
        out.push_str(&note("Type a command and press Enter: it runs on the host, in the repo folder, with no input. `clear` empties the list."));
    }
    for run in term.runs() {
        use dry_core::workbench::RunState;
        let (running, exit, status) = match &run.state {
            RunState::Running => (true, None, "running".to_string()),
            RunState::Exited(code) => (false, *code, code.map_or("stopped".into(), |c| format!("exit {c}"))),
            RunState::Failed(why) => (false, Some(-1), why.clone()),
        };
        let view = dry_core::inspect::SessionView {
            id: run.id as u32,
            mode: "run".into(),
            command: run.command.clone(),
            running,
            status: status.clone(),
            exit,
            age: String::new(),
            tail: run.output.join("\n"),
        };
        let _ = write!(
            out,
            "<div class=\"term\" data-key=\"host-run-{}\"><div class=\"term-head\">{}{}<span class=\"grow\"></span>{}</div>",
            run.id,
            super::term::mark(&view),
            super::term::prompt(&run.command),
            if let RunState::Failed(_) = run.state { format!("<span class=\"term-badge bad\">{}</span>", esc(&status)) } else { super::term::badge(&view) }
        );
        super::term::output(out, &view.tail, if running { "(nothing yet)" } else { "(no output)" });
        out.push_str("</div>");
    }
    let busy = term.busy();
    let _ = write!(
        out,
        "<form class=\"prompt-line\" data-terminal onsubmit=\"return false\"><span>{}</span><input name=\"line\" data-keep=\"host-terminal\" autocomplete=\"off\" spellcheck=\"false\" placeholder=\"{}\">{}</form></div></div>",
        if busy { "…" } else { "$" },
        if busy { "a command is running; another runs beside it" } else { "a command, run on the host in the repo folder" },
        cx.act("Run", "primary", "data-act=\"run-terminal\"")
    );
}
