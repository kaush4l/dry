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
            "<span class=\"lv-tab{}\" title=\"{}\">{}{}</span>",
            if std::ptr::eq(t, &v.tabs[0]) { " now" } else { "" },
            esc(&t.path),
            esc(&t.name),
            if t.unsaved > 0 { " ●" } else { "" }
        );
    }
    out.push_str("</div>");
    for (n, t) in v.tabs.iter().enumerate() {
        // The first tab is the file the engine touched last, open by
        // default. Its key says so: when the engine moves to another file
        // both keys change, so the page's kept folds (`data-key`) do not
        // hold the view on the old file, and a person's own choice stays
        // only while the engine stays.
        let key = format!("{engine}/live/{}{}", if n == 0 { "now:" } else { "" }, t.key());
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
        // A page is looked at first, read second: its preview stands above its code.
        preview(out, t);
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

/// The page an HTML file makes, above its code: what a person following
/// the work wants to see of a small UI element, without a server. It is
/// drawn in a frame that can run the page's own script and nothing else:
/// no access to this page (`sandbox` without same-origin) and no network
/// (the policy line put before the file's text), since the text was
/// written by a model. Only for a whole file (no focus) that holds text.
fn preview(out: &mut String, t: &dry_core::liveview::Tab) {
    let html = t.path.ends_with(".html") || t.path.ends_with(".htm");
    if !html || t.focus.is_some() || t.lines.is_empty() {
        return;
    }
    let text: String = t.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    let page = format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{PREVIEW_POLICY}\">{text}");
    let _ = write!(
        out,
        "<div class=\"lv-preview\"><div class=\"lv-sub\">PREVIEW<span class=\"note\">{} as a page · its own script runs, nothing is fetched</span></div><iframe title=\"preview of {}\" sandbox=\"allow-scripts\" srcdoc=\"{}\"></iframe></div>",
        esc(&t.name),
        esc(&t.path),
        esc(&page)
    );
}

/// What a previewed page may load: only what is written in the file
/// itself (inline style and script, data: images). Nothing from a network.
pub const PREVIEW_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:";

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
        // As the editor's first tab: the first session is the one running
        // or the newest, and its key follows it.
        let key = format!("{engine}/live/{}{}", if n == 0 { "now:" } else { "" }, s.key());
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

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::inspect::FileLine;
    use dry_core::liveview::Tab;

    fn tab(path: &str, text: &str, focus: Option<(usize, usize)>) -> Tab {
        let lines = text.lines().enumerate().map(|(i, l)| FileLine { n: i + 1, text: l.to_string(), changed: false, focused: true }).collect();
        Tab { artifact: "code".into(), path: path.into(), name: path.into(), language: "HTML", focus, unsaved: 0, lines, note: None }
    }

    #[test]
    fn an_html_file_is_previewed_in_a_frame_with_no_network_and_no_way_into_the_page() {
        let mut out = String::new();
        preview(&mut out, &tab("ui/counter.html", "<button onclick=\"n++\">Count \"it\"</button>\n<script>let n = 0</script>", None));
        assert!(out.contains("<iframe title=\"preview of ui/counter.html\" sandbox=\"allow-scripts\" srcdoc=\""), "{out}");
        assert!(!out.contains("allow-same-origin") && !out.contains("allow-top-navigation"), "its script cannot reach this page");
        // The policy stands before the file's text, and the text is an attribute's value, never markup of this page.
        assert!(out.contains("srcdoc=\"&lt;meta http-equiv=&quot;Content-Security-Policy&quot; content=&quot;default-src &#39;none&#39;;"), "{out}");
        assert!(out.contains("&lt;button onclick=&quot;n++&quot;&gt;Count &quot;it&quot;&lt;/button&gt;") && !out.contains("<button"), "{out}");
        // Nothing for a file that is not a page, one shown in part, or an empty one.
        for t in [tab("tree.py", "print(1)", None), tab("a.html", "<p>x</p>", Some((1, 1))), tab("b.html", "", None)] {
            let mut none = String::new();
            preview(&mut none, &t);
            assert_eq!(none, "", "{}", t.path);
        }
    }
}
