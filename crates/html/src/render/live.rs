//! The live view's parts on the web (`dry_core::liveview`, docs/developer-loop.md,
//! "The Code page is the human view of the developer artifacts"): the explorer, the editor tabs, the terminal panel, the chat and
//! the status strip, drawn from the same exact view the GPUI window draws
//! (`liveview::build` over `inspect::artifacts`). Core decides what each
//! part shows; this only draws it.
//!
//! In the browser app the `file` and `terminal` artifacts reach a folder
//! on the person's computer through the companion (`dry_core::workspace`),
//! so these parts show that folder live.

use std::fmt::Write as _;

use dry_core::liveview::{self, LiveView, Part};

use super::{Ctx, chip, engine, esc, note};

/// One part of engine `:id`'s live view (one artifact's: `:name`).
pub fn part(out: &mut String, part: Part, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else {
        return note_box(out, "No engine here: a live view belongs to an engine's page.");
    };
    let Some(snap) = cx.snap(i) else { return note_box(out, "No view of this engine yet.") };
    let scope = cx.params.get("name");
    let gauge = Some((snap.ratio, snap.size.clone()));
    let v = liveview::build(e, &snap.artifacts, scope, &cx.activity(i), gauge);
    if let Some(why) = &v.empty
        && part != Part::Chat
        && part != Part::Status
    {
        return note_box(out, why);
    }
    if !v.parts.contains(&part) {
        let whole = liveview::path(i, None);
        let _ = write!(
            out,
            "<div class=\"lv-empty\">{}<a class=\"btn\" href=\"{}\">The engine's whole view ›</a></div>",
            note(&format!("{} attaches no {}.", scope.unwrap_or("this view"), part.as_str())),
            esc(&whole)
        );
        return;
    }
    let _ = write!(out, "<div class=\"lv lv-{}\" data-part=\"{}\">", part.as_str(), part.as_str());
    match part {
        Part::Explorer => explorer(out, &v),
        Part::Editor => editor(out, &v, e.id.as_str()),
        Part::Panel => panel(out, &v, e.id.as_str()),
        Part::Chat => {
            // The conversation scrolls above the composer and opens at its
            // newest message (a reversed column anchors at the bottom).
            out.push_str("<div class=\"lv-chat-scroll\" data-stick><div>");
            engine::chat(out, cx);
            out.push_str("</div></div>");
            engine::composer(out, &[], cx);
        }
        Part::Status => status(out, &v, i, cx),
    }
    out.push_str("</div>");
}

fn note_box(out: &mut String, why: &str) {
    let _ = write!(out, "<div class=\"lv-empty\">{}</div>", note(why));
}

fn explorer(out: &mut String, v: &LiveView) {
    out.push_str("<div class=\"lv-head\">EXPLORER</div>");
    if !v.tabs.is_empty() {
        out.push_str("<div class=\"lv-sub\">OPEN EDITORS</div><div class=\"lv-list\">");
        for t in &v.tabs {
            let _ = write!(
                out,
                "<div class=\"lv-row open\" title=\"{}\"><span class=\"lv-dot\">{}</span><span class=\"trunc\">{}</span><span class=\"note trunc\">{}</span></div>",
                esc(&t.path),
                if t.unsaved > 0 { "●" } else { "" },
                esc(&t.name),
                esc(t.path.rsplit_once('/').map_or("", |(d, _)| d))
            );
        }
        out.push_str("</div>");
    }
    for r in &v.roots {
        let _ =
            write!(out, "<div class=\"lv-sub\">{} · <span class=\"trunc\">{}</span></div>", esc(&r.artifact).to_uppercase(), esc(&r.root));
        match &r.rows {
            Err(why) => out.push_str(&note(why)),
            Ok(rows) if rows.is_empty() => out.push_str(&note("No files.")),
            Ok(rows) => {
                out.push_str("<div class=\"lv-list lv-tree\">");
                for (row, expanded) in liveview::visible(rows, &|_| false) {
                    let glyph = if row.dir { if expanded { "▾" } else { "▸" } } else { "" };
                    let class = match (row.open, row.unsaved) {
                        (_, true) => "lv-row unsaved",
                        (true, _) => "lv-row open",
                        _ => "lv-row",
                    };
                    let _ = write!(
                        out,
                        "<div class=\"{class}\" style=\"padding-left:{}px\" data-path=\"{}\"><span class=\"lv-glyph\">{glyph}</span><span class=\"trunc\">{}</span>{}</div>",
                        8 + row.depth * 12,
                        esc(&row.path),
                        esc(&row.name),
                        if row.unsaved && !row.dir { "<span class=\"lv-dot\">●</span>" } else { "" }
                    );
                }
                out.push_str("</div>");
            }
        }
    }
}

fn editor(out: &mut String, v: &LiveView, engine: &str) {
    if v.tabs.is_empty() {
        out.push_str(&note("No files open: the engine opens them with open(path=\"…\")."));
        return;
    }
    out.push_str("<div class=\"lv-tabs\">");
    for t in &v.tabs {
        let _ = write!(
            out,
            "<span class=\"lv-tab\" title=\"{}\">{}{}</span>",
            esc(&t.path),
            esc(&t.name),
            if t.unsaved > 0 { " ●" } else { "" }
        );
    }
    out.push_str("</div>");
    for (n, t) in v.tabs.iter().enumerate() {
        let key = format!("{engine}/live/{}", t.key());
        let focus = t.focus.map(|(a, b)| format!(" · lines {a}–{b}")).unwrap_or_default();
        let _ = write!(
            out,
            "<details class=\"lv-file\" data-key=\"{}\"{}><summary><b class=\"trunc\">{}</b><span class=\"note\">{} · {}{focus}{}</span></summary>",
            esc(&key),
            if n == 0 { " open" } else { "" },
            esc(&t.path),
            esc(&t.artifact),
            esc(t.language),
            if t.unsaved > 0 { format!(" · {} unsaved", t.unsaved) } else { String::new() }
        );
        if let Some(why) = &t.note {
            out.push_str(&note(why));
        }
        out.push_str("<pre class=\"lv-code\">");
        for l in &t.lines {
            let class = match (l.changed, l.focused) {
                (true, _) => "ch",
                (false, false) => "dim",
                _ => "",
            };
            let _ = writeln!(out, "<span class=\"{class}\"><i>{:>4}{}</i>{}</span>", l.n, if l.changed { "*" } else { " " }, esc(&l.text));
        }
        out.push_str("</pre></details>");
    }
}

fn panel(out: &mut String, v: &LiveView, engine: &str) {
    let running = v.sessions.iter().filter(|s| s.view.running).count();
    let _ = write!(
        out,
        "<div class=\"lv-head\">TERMINAL {}</div>",
        chip(&if running == 0 { format!("{} done", v.sessions.len()) } else { format!("{running} running") })
    );
    if v.sessions.is_empty() {
        out.push_str(&note("No sessions yet: the engine runs commands with run, start and background."));
        return;
    }
    // Each session a terminal (`term`): the newest output in view, and a
    // fold so a person can close the ones they are not watching.
    for (n, s) in v.sessions.iter().enumerate() {
        let key = format!("{engine}/live/{}", s.key());
        let (class, _, _) = super::term::state(&s.view);
        let _ = write!(
            out,
            "<details class=\"lv-session term {class}\" data-key=\"{}\"{}><summary class=\"term-head\">{}{}<span class=\"note trunc\">{} · {}</span>{}<span class=\"note term-age\">{}</span>{}</summary>",
            esc(&key),
            if n == 0 { " open" } else { "" },
            super::term::mark(&s.view),
            super::term::prompt(&s.view.command),
            s.role.label(),
            esc(&s.view.mode),
            super::term::badge(&s.view),
            esc(&s.view.age),
            super::term::copy_button(),
        );
        super::term::output(out, &s.view.tail, if s.view.running { "no output yet" } else { "it printed nothing" });
        out.push_str("</details>");
    }
}

fn status(out: &mut String, v: &LiveView, i: usize, cx: &Ctx) {
    let s = &v.status;
    let tint = s.tint.map(|t| format!(" style=\"background:var(--{t})\"")).unwrap_or_default();
    let gauge = s.gauge.as_ref().map(|(_, line)| format!("<span class=\"note\">{}</span>", esc(line))).unwrap_or_default();
    let taken = cx.snap(i).map(|x| x.taken).unwrap_or(0);
    let _ = write!(
        out,
        "<div class=\"lv-status\"><b>{}</b><span class=\"lv-activity\"{tint}>{}</span><span class=\"note\">{}</span><span class=\"note\">{}</span>{gauge}<span class=\"grow\"></span><span class=\"note\">view at {}</span><a class=\"lv-link\" href=\"{}\">Engine ›</a></div>",
        esc(&s.engine),
        esc(&s.activity),
        esc(&s.model),
        esc(&s.counts),
        esc(&clock(taken, cx.world.clock.offset)),
        esc(&dry_core::routes::engine_path(i))
    );
}

fn clock(unix: i64, offset: i32) -> String {
    let t = (unix + offset as i64).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60)
}
