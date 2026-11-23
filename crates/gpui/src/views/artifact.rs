//! `artifact-page`: one artifact's own page, `/desk/:id/artifacts/:name`.
//!
//! The engine page shows each artifact as one row; Open comes here. The
//! page is drawn from the same exact view the engine page uses
//! (`dry_core::inspect`, taken with the disk the turn reads and kept until
//! Refresh), so what it shows is what is sent:
//!
//! ```text
//! head        its glyph, name, kind, meaning, state in a few words · taken at · Refresh
//! body        by kind: file list + open files · terminal blocks · posts · learning fill ·
//!             child cards · text for the clock, scripts, alerts and plain ones
//! side        its tools with descriptions · its state rows
//! snapshot    its section of the prompt, exactly as sent
//! ```
//!
//! The header trail reads Home › Engines › <engine> › <artifact>, so the
//! way back is one click. A `file` or `terminal` artifact's head has
//! Live view ›: the same state in a code editor's layout
//! (`/desk/:id/artifacts/:name/live`, `views::live_view`).

use dry_core::inspect::{ChildView, Detail, FileDetail, OpenFile, PostView, SectionFill, SessionView, Source};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*, px};

use super::card::{chip, gauge, panel};
use super::engine::{mono, note, taken_line, tool_group};
use super::{SIDE_WIDTH, columns, fold, is_open, missing};
use crate::AppView;
use crate::ui::color;

/// Most lines of an open file drawn at once (around its focus).
const SHOWN_LINES: usize = 400;

pub fn page(app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some((i, e)) = params.index("id").and_then(|i| app.state.engines.get(i).map(|e| (i, e))) else {
        return missing(&app.view.location, t);
    };
    let name = params.get("name").unwrap_or_default().to_string();
    let Some(x) = app.inspect(i) else { return missing(&app.view.location, t) };
    let Some(v) = x.artifacts.iter().find(|v| v.name == name) else {
        return panel(t)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("No artifact \"{name}\"")))
            .child(note(format!("{} holds {}.", e.name, dry_core::inspect::count(x.artifacts.len(), "artifact", "artifacts")), t));
    };

    let head = panel(t)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().flex_none().text_color(color(t.text_accent)).child(dry_core::inspect::icon(v.kind)))
                .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(v.name.clone()))
                .child(chip(v.kind.map_or("text", |k| k.as_str()), t))
                .child(div().flex_1())
                .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(v.summary.clone()))
                // A developer artifact attaches a live view (`dry_core::liveview`).
                .when(v.kind.is_some_and(|k| !k.view().is_empty()), |d| {
                    let to = dry_core::liveview::path(i, Some(&v.name));
                    d.child(
                        crate::button("artifact-live", "Live view ›", t)
                            .bg(color(t.accent))
                            .text_color(color(t.on_accent))
                            .on_click(cx.listener(move |this, _, window, cx| this.navigate(&to, window, cx))),
                    )
                }),
        )
        .child(note(v.meaning.clone(), t))
        .when_some(v.not_sent.clone(), |d, why| d.child(note(format!("not sent now: {why}"), t).text_color(color(t.error))))
        .child(taken_line(i, &x, t, cx));

    let body = body(app, &v.detail, t, cx);

    // Its tools, from the engine's toolbox: the group its kind brings.
    let groups: Vec<_> =
        dry_core::inspect::tool_groups(e).into_iter().filter(|g| v.kind.is_some_and(|k| g.source == Source::Artifact(k))).collect();
    let tools = panel(t)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Tools"))
                .child(chip(groups.iter().map(|g| g.tools.len()).sum::<usize>().to_string(), t)),
        )
        .when(groups.is_empty(), |d| {
            d.child(note(
                match v.kind {
                    None => "None of its own: a plain artifact is read and written with read(...) and write(...).",
                    Some(_) => "None: this kind brings no tools.",
                },
                t,
            ))
        })
        .children(groups.iter().map(|g| tool_group(g, t)));
    let state = panel(t).child(div().font_weight(FontWeight::SEMIBOLD).child("State")).children(v.state.iter().map(|(k, val)| {
        div()
            .flex()
            .gap(px(t.space_2))
            .text_size(px(t.text_sm))
            .child(div().w(px(88.)).flex_none().truncate().text_color(color(t.text_disabled)).child(k.clone()))
            .child(div().flex_1().min_w_0().text_color(color(t.text_secondary)).child(val.clone()))
    }));

    let key = format!("{}/{}/snapshot", e.id, v.name);
    let open = is_open(app, &key, true);
    let size = format!("≈ {} tokens", dry_core::context::short(dry_core::context::tokens(&v.snapshot)));
    let snapshot = fold(key, open, "Snapshot · as sent", Some(size), t, cx).when(open, |d| d.child(mono(v.snapshot.clone(), t)));

    let main = div().flex().flex_col().gap(px(t.space_3)).child(body).child(snapshot);
    let side = div().flex().flex_col().gap(px(t.space_3)).child(tools).child(state);
    div().flex().flex_col().gap(px(t.space_3)).child(head).child(columns(app, main, side, SIDE_WIDTH - 40., t))
}

/// The artifact's state, laid out by its kind.
fn body(app: &AppView, detail: &Detail, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    match detail {
        Detail::Text(text) => mono(text.clone(), t),
        Detail::Broken(why) => panel(t).child(note(format!("Its state does not read: {why}"), t).text_color(color(t.error))),
        Detail::File(f) => file(app, f, t),
        Detail::Terminal { root, sessions } => terminal(root, sessions, t),
        Detail::Shared(posts) => shared(posts.as_deref(), t),
        Detail::Learning { sections, collecting } => learning(sections, collecting, t),
        Detail::Engines(children) => engines(app, children.as_deref(), t, cx),
        Detail::Evidence(ev) => evidence(ev, t),
    }
}

/// An evidence artifact: its verdict, then each shot (changed ones first:
/// before, after, diff side by side) and the page-text lines that changed.
/// The images are read from disk under the artifact's root.
fn evidence(ev: &dry_core::inspect::EvidenceDetail, t: &Tokens) -> Div {
    use dry_core::evidence::report::Status;
    let base =
        if std::path::Path::new(&ev.root).is_absolute() { std::path::PathBuf::from(&ev.root) } else { dry_tools::root().join(&ev.root) };
    let shot = |label: &'static str, path: &str| {
        div()
            .flex_1()
            .min_w(px(180.))
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(label))
            .child(gpui::img(base.join(path).as_path()).w_full().rounded(px(t.radius)))
    };
    let verdict = if ev.line.ends_with("fail") { t.error } else { t.text_accent };
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_3))
        .child(
            panel(t)
                .child(
                    div()
                        .flex()
                        .gap(px(t.space_2))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Evidence"))
                        .child(div().text_color(color(verdict)).child(ev.line.clone())),
                )
                .child(note(
                    format!(
                        "Runs `{}` in {}{}",
                        ev.command,
                        ev.root,
                        ev.commit.as_ref().map_or(String::new(), |(c, _)| format!(" · commit {c}"))
                    ),
                    t,
                )),
        )
        .children(ev.shots.iter().map(|s| {
            let tone = match s.status {
                Status::Changed | Status::Flaky => t.error,
                Status::Same => t.text_accent,
                _ => t.accent,
            };
            let mut row = div().flex().flex_wrap().gap(px(t.space_2));
            row = match (&s.after, &s.diff) {
                (Some(after), diff) => {
                    let r = row.child(shot("before", &s.png)).child(shot("after", after));
                    match diff {
                        Some(d) => r.child(shot("diff", d)),
                        None => r,
                    }
                }
                (None, Some(d)) => row.child(shot("now", &s.png)).child(shot("diff", d)),
                (None, None) => row.child(shot("now", &s.png)),
            };
            panel(t)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(t.space_2))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(t.font_mono.clone())
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(s.name.clone()),
                        )
                        .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(tone)).child(format!("● {}", s.status.word())))
                        .when(s.status != Status::New, |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .text_size(px(t.text_sm))
                                    .text_color(color(t.text_secondary))
                                    .child(format!("{:.2}%", s.changed)),
                            )
                        }),
                )
                .child(row)
                .when(!s.text.is_empty(), |d| d.child(mono(s.text.join("\n"), t)))
        }))
}

/// A file artifact: the names under its root on the left (the open ones
/// marked), each open file on the right with line numbers, its focus
/// lit and its unsaved lines marked.
fn file(app: &AppView, f: &FileDetail, t: &Tokens) -> Div {
    let list = match &f.files {
        Err(why) => note(why.clone(), t).into_any_element(),
        Ok(names) => div()
            .id("artifact-files")
            .flex()
            .flex_col()
            .max_h(px(480.))
            .overflow_y_scroll()
            .children(names.iter().map(|(path, open)| {
                div()
                    .flex()
                    .gap(px(t.space_1))
                    .text_size(px(t.text_sm))
                    .font_family(t.font_mono.clone())
                    .child(div().w(px(t.space_3)).flex_none().text_color(color(t.text_accent)).child(if *open { "●" } else { "" }))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(color(if *open { t.text_primary } else { t.text_secondary }))
                            .when(*open, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .child(path.clone()),
                    )
            }))
            .into_any_element(),
    };
    let count = f.files.as_ref().map_or(String::new(), |n| n.len().to_string());
    let list = panel(t)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Files"))
                .when(!count.is_empty(), |d| d.child(chip(count, t)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(t.text_sm))
                        .text_color(color(t.text_secondary))
                        .child(format!("root {}", f.root)),
                ),
        )
        .child(list);
    let open = div()
        .flex()
        .flex_col()
        .gap(px(t.space_3))
        .when(f.open.is_empty(), |d| d.child(panel(t).child(note("No files open: the engine opens one with open(path=\"...\").", t))))
        .children(f.open.iter().map(|o| open_file(o, t)));
    columns(app, open, list, 240., t)
}

/// One open file: its path, focus and unsaved count, then its lines.
fn open_file(o: &OpenFile, t: &Tokens) -> Div {
    let total = o.lines.len();
    // Around the focus when the file is long.
    let start = o.focus.map_or(0, |(a, _)| a.saturating_sub(11)).min(total.saturating_sub(SHOWN_LINES.min(total)));
    let end = (start + SHOWN_LINES).min(total);
    let rows = o.lines[start..end].iter().map(|l| {
        let mut row = div()
            .flex()
            .gap(px(t.space_2))
            .px(px(t.space_1))
            .font_family(t.font_mono.clone())
            .text_size(px(t.text_code))
            .border_l_2()
            .border_color(if o.focus.is_some() && l.focused { color(t.accent) } else { gpui::transparent_black() })
            .when(o.focus.is_some() && l.focused, |d| d.bg(color(t.line_highlight)))
            .when(o.focus.is_some() && !l.focused, |d| d.text_color(color(t.text_secondary)))
            .child(div().w(px(40.)).flex_none().flex().justify_end().text_color(color(t.text_disabled)).child(l.n.to_string()))
            .child(div().w(px(t.space_2)).flex_none().text_color(color(t.text_accent)).child(if l.changed { "*" } else { "" }))
            .child(div().flex_1().min_w_0().truncate().child(if l.text.is_empty() {
                " ".to_string()
            } else {
                l.text.replace('\t', "    ")
            }));
        if l.changed {
            row = row.bg(color(t.accent_muted));
        }
        row
    });
    let shown = (start > 0 || end < total).then(|| format!("lines {}-{end} of {total} shown", start + 1));
    panel(t)
        .gap(px(t.space_1))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .pb(px(t.space_1))
                .child(div().min_w_0().truncate().font_family(t.font_mono.clone()).font_weight(FontWeight::SEMIBOLD).child(o.path.clone()))
                .child(div().flex_1())
                .children(o.focus.map(|(a, b)| chip(format!("focus {a}-{b}"), t)))
                .when(o.unsaved > 0, |d| {
                    d.child(chip(dry_core::inspect::count(o.unsaved, "unsaved edit", "unsaved edits"), t).text_color(color(t.text_accent)))
                })
                .child(chip(dry_core::inspect::count(total, "line", "lines"), t)),
        )
        .children(o.note.clone().map(|n| note(n, t)))
        .children(shown.map(|s| note(s, t)))
        .child(div().flex_none().flex().flex_col().rounded(px(t.space_2)).bg(color(t.background_surface)).py(px(t.space_1)).children(rows))
}

/// A terminal artifact: each session as a terminal block, running first.
fn terminal(root: &str, sessions: &[SessionView], t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_3))
        .child(note(format!("Runs in {root}"), t))
        .when(sessions.is_empty(), |d| {
            d.child(
                panel(t).child(note(
                    "No sessions yet: run(...) runs one and waits; start(...) tells when it ends; background(...) does not.",
                    t,
                )),
            )
        })
        .children(sessions.iter().map(|s| {
            let (dot, tone) = match (s.running, s.exit) {
                (true, _) => ("● running", t.accent),
                (false, Some(0)) => ("● exit 0", t.text_accent),
                (false, _) => ("● ", t.error),
            };
            let status = if s.running || s.exit == Some(0) { dot.to_string() } else { format!("● {}", s.status) };
            panel(t)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(t.space_2))
                        .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_disabled)).child(format!("#{}", s.id)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(t.font_mono.clone())
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("$ {}", s.command.lines().next().unwrap_or(""))),
                        )
                        .child(chip(s.mode.clone(), t))
                        .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(tone)).child(status))
                        .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(s.age.clone())),
                )
                .child(
                    div()
                        .p(px(t.space_2))
                        .rounded(px(t.space_2))
                        .bg(color(t.background_surface))
                        .font_family(t.font_mono.clone())
                        .text_size(px(t.text_code))
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .children(
                            s.tail.lines().map(|l| div().min_w_0().child(if l.is_empty() { " ".to_string() } else { l.to_string() })),
                        ),
                )
        }))
}

/// A shared context: its level's posts as a feed.
fn shared(posts: Option<&[PostView]>, t: &Tokens) -> Div {
    let Some(posts) = posts else { return panel(t).child(note("Read with the engines when the view is taken.", t)) };
    panel(t)
        .gap(px(t.space_3))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Posts"))
                .child(chip(posts.len().to_string(), t)),
        )
        .when(posts.is_empty(), |d| d.child(note("No posts yet: post(text=\"...\") writes one for the level.", t)))
        .children(posts.iter().enumerate().map(|(n, p)| {
            div()
                .flex()
                .flex_col()
                .gap(px(t.space_1))
                .when(n > 0, |d| d.pt(px(t.space_2)).border_t_1().border_color(color(t.border)))
                .child(
                    div()
                        .flex()
                        .gap(px(t.space_2))
                        .text_size(px(t.text_sm))
                        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(color(t.text_accent)).child(p.author.clone()))
                        .child(div().text_color(color(t.text_secondary)).child(p.when.clone())),
                )
                .child(div().child(p.text.clone()))
        }))
}

/// A learning artifact: its three sections, each with its fill against its
/// cap and its entries.
fn learning(sections: &[SectionFill], collecting: &str, t: &Tokens) -> Div {
    div().flex().flex_col().gap(px(t.space_3)).child(note(format!("Collecting now: {collecting}"), t)).children(sections.iter().map(|s| {
        let ratio = s.used as f64 / s.cap.max(1) as f64;
        panel(t)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t.space_2))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(s.name.clone()))
                    .child(chip(dry_core::inspect::count(s.entries.len(), "entry", "entries"), t))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(t.text_sm))
                            .text_color(color(t.text_secondary))
                            .child(format!("{} / {} characters", s.used, s.cap)),
                    ),
            )
            .child(gauge(ratio, if ratio >= 0.9 { t.error } else { t.accent }, t))
            .child(note(s.holds.clone(), t))
            .children(s.entries.iter().map(|e| div().text_size(px(t.text_sm)).child(format!("· {e}"))))
    }))
}

/// An engines artifact: each child as a card, opening its engine page.
fn engines(app: &AppView, children: Option<&[ChildView]>, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some(children) = children else { return panel(t).child(note("Read with the engines when the view is taken.", t)) };
    if children.is_empty() {
        return panel(t).child(note("No children yet: engine_start(...) or engine_call(...) makes one.", t));
    }
    div().flex().flex_wrap().gap(px(t.space_3)).children(children.iter().map(|c| {
        let j = app.state.index_of(&c.id);
        let tint = j.and_then(|j| app.activity(j).token()).and_then(|n| t.color(n));
        panel(t)
            .id(SharedString::from(format!("child-{}", c.id)))
            .w(px(260.))
            .when_some(tint, |d, tint| d.child(super::card::wash(tint, t.pane_radius)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t.space_2))
                    .child(div().flex_none().text_color(color(t.text_accent)).child("◇"))
                    .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(c.name.clone()))
                    .child(chip(if c.call { "call" } else { "start" }, t)),
            )
            .child(div().text_size(px(t.text_sm)).child(if c.goal.is_empty() { "(no goal)".to_string() } else { c.goal.clone() }))
            .child(note(
                format!(
                    "{} · {}",
                    if c.activity.is_empty() { "idle" } else { c.activity.as_str() },
                    dry_core::inspect::count(c.replies, "reply", "replies")
                ),
                t,
            ))
            .when_some(c.failed.clone(), |d, why| d.child(note(why, t).text_color(color(t.error))))
            .when_some(c.last_reply.clone(), |d, r| {
                let short: String = r.chars().take(240).collect();
                d.child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(if short.len() < r.len() {
                    format!("{short}…")
                } else {
                    short
                }))
            })
            .when_some(j, |d, j| {
                d.cursor_pointer()
                    .hover(|d| d.border_color(color(t.accent)))
                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(&dry_core::routes::engine_path(j), window, cx)))
            })
    }))
}
