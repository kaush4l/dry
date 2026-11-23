//! `live-view`: the UI the developer artifacts attach (`dry_core::liveview`),
//! drawn as a code editor a person already knows.
//!
//! ```text
//! explorer   OPEN EDITORS (the engine's open files, ● unsaved), then each
//!            file artifact's root as a tree: open files bright, folders
//!            holding them open, unsaved ones marked ●
//! editor     one tab per open file, the file as it is now with its unsaved
//!            edits; lines outside the engine's focus dimmed, changed ones
//!            marked in the gutter; switching to a tab reveals its focus
//! panel      the terminal sessions: the picked one's command, output tail
//!            and status, beside the list of all (running first, by role)
//! chat       the engine page's conversation and composer, newest at the bottom
//! status     engine · activity (on its tint) · model · counts · prompt gauge
//! ```
//!
//! The parts are placed by the page (`components/live-workbench.json`),
//! each one `live-view` component. They draw from one `LiveView` built in
//! core from the exact view (`AppView::inspect`), cached here until the
//! next snapshot, so no frame reads the disk. The snapshot is taken after
//! each step and each callback, and every 2 s while a live view with a
//! running session is on screen (`AppView::refresh_live`), so the output
//! tail moves.

use std::collections::HashMap;
use std::rc::Rc;

use dry_core::liveview::{self, LiveView, Part, Session, Tab};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use gpui::{
    Context, Div, FontWeight, ScrollHandle, ScrollStrategy, SharedString, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};

use super::card::{gauge, wash};
use crate::AppView;
use crate::ui::{self, color};

/// A row of a tree or list, a part's header, and how far a level indents:
/// a code editor's metrics, as on the Code page.
const ROW: f32 = 22.;
const HEADER: f32 = 35.;
const INDENT: f32 = 8.;
const TWISTIE: f32 = 16.;
/// The panel's list of sessions beside the output.
const SESSION_LIST: f32 = 220.;
/// How often a live view with a running session takes a new snapshot.
const REFRESH_SECS: i64 = 2;

/// What the live views keep between frames: the view built from the last
/// snapshot, which tab and session are picked, and the scroll positions.
#[derive(Default)]
pub(crate) struct LiveState {
    cache: Option<(String, Rc<LiveView>)>,
    /// `engine-id/scope/what` → the picked tab's or session's key.
    picked: HashMap<String, String>,
    /// The tab whose focus was last revealed, per view.
    revealed: HashMap<String, String>,
    editor: UniformListScrollHandle,
    chat: ScrollHandle,
    /// Messages seen per engine, to keep the chat at its newest.
    seen: HashMap<String, usize>,
}

impl LiveState {
    /// Drop the view built from the last snapshot (a new one was taken).
    pub(crate) fn forget(&mut self) {
        self.cache = None;
    }
}

/// The engine, its index and the view's key prefix for the route.
fn engine(app: &AppView, params: &Params) -> Option<(usize, String)> {
    let i = params.index("id")?;
    let e = app.state.engines.get(i)?;
    Some((i, format!("{}/{}", e.id, params.get("name").unwrap_or("*"))))
}

/// The live view of the route's engine (one artifact's with `:name`),
/// built once per snapshot.
fn view(app: &AppView, params: &Params) -> Option<(usize, String, Rc<LiveView>)> {
    let (i, prefix) = engine(app, params)?;
    let e = app.state.engines.get(i)?;
    let activity = app.activity(i);
    let gauge = match app.engine_context.get(&i) {
        Some(c) => Some((c.budget.ratio(), c.budget.line())),
        None => app.inspect(i).map(|x| (x.ratio, format!("{} · estimate", x.size))),
    };
    let key = format!("{prefix}|{}|{:?}", activity.label(), gauge.as_ref().map(|g| &g.1));
    if let Some((k, v)) = &app.inspects.borrow().live.cache
        && *k == key
    {
        return Some((i, prefix, v.clone()));
    }
    let built = {
        let x = app.inspect(i)?;
        Rc::new(liveview::build(e, &x.artifacts, params.get("name"), &activity, gauge))
    };
    app.inspects.borrow_mut().live.cache = Some((key, built.clone()));
    Some((i, prefix, built))
}

/// One part, filling its pane.
pub fn draw(part: Part, app: &AppView, params: &Params, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>) -> Div {
    let Some((i, prefix, lv)) = view(app, params) else { return super::missing(&app.view.location, t) };
    let body = match part {
        Part::Explorer => explorer(app, i, &prefix, &lv, t, cx),
        Part::Editor => editor(app, i, &prefix, &lv, t, cx),
        Part::Panel => panel(app, i, &prefix, &lv, t, cx),
        Part::Chat => chat(app, params, i, t, window, cx),
        Part::Status => status(app, i, &lv, t, cx),
    };
    body.size_full().min_h_0().min_w_0()
}

/// A part's header: its name small and upper-case, and what it holds.
fn title(text: &str, detail: Option<String>, t: &Tokens) -> Div {
    div()
        .flex_none()
        .h(px(HEADER))
        .pl(px(20.))
        .pr(px(t.space_3))
        .flex()
        .items_center()
        .gap(px(t.space_2))
        .text_size(px(t.text_sm))
        .child(div().flex_none().text_color(color(t.text_primary)).child(text.to_uppercase()))
        .children(detail.map(|d| div().flex_1().min_w_0().truncate().text_color(color(t.text_secondary)).child(d)))
}

/// Where a part has nothing to show in this view: why, and the way to
/// the view that does.
fn absent(why: String, app: &AppView, i: usize, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let whole = liveview::path(i, None);
    let here = app.view.location == whole;
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(t.space_2))
        .p(px(t.space_4))
        .text_color(color(t.text_disabled))
        .child(div().text_center().child(why))
        .when(!here, |d| {
            d.child(
                crate::button("live-whole", "Open the engine's live view", t)
                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(&whole, window, cx))),
            )
        })
}

/// Pick `value` for `what` of view `prefix` (a tab, a session).
fn pick(this: &mut AppView, prefix: &str, what: &str, value: String) {
    this.inspects.get_mut().live.picked.insert(format!("{prefix}/{what}"), value);
}

/// The picked tab's index (the first when none is picked or it closed).
fn active_tab(app: &AppView, prefix: &str, tabs: &[Tab]) -> Option<usize> {
    let picked = app.inspects.borrow().live.picked.get(&format!("{prefix}/tab")).cloned();
    if tabs.is_empty() {
        return None;
    }
    Some(picked.and_then(|k| tabs.iter().position(|t| t.key() == k)).unwrap_or(0))
}

fn active_session(app: &AppView, prefix: &str, sessions: &[Session]) -> Option<usize> {
    let picked = app.inspects.borrow().live.picked.get(&format!("{prefix}/session")).cloned();
    if sessions.is_empty() {
        return None;
    }
    Some(picked.and_then(|k| sessions.iter().position(|s| s.key() == k)).unwrap_or(0))
}

/// A dot for unsaved edits, where a code editor puts it.
fn dirty(t: &Tokens) -> Div {
    div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_accent)).child("●")
}

/// `explorer`: OPEN EDITORS (every file the engine has open), then each
/// file artifact's root as a tree.
fn explorer(app: &AppView, i: usize, prefix: &str, lv: &LiveView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    if !lv.parts.contains(&Part::Explorer) {
        return div().flex().flex_col().child(title("Explorer", None, t)).child(absent(
            lv.empty.clone().unwrap_or_else(|| "No file artifact in this view.".into()),
            app,
            i,
            t,
            cx,
        ));
    }
    let active = active_tab(app, prefix, &lv.tabs);
    let hover = color(t.overlay_hover);
    let open_editors = lv.tabs.iter().enumerate().map(|(n, tab)| {
        let key = tab.key();
        let prefix = prefix.to_string();
        div()
            .id(SharedString::from(format!("live-open-{n}")))
            .flex_none()
            .h(px(ROW))
            .pl(px(INDENT + TWISTIE))
            .pr(px(t.space_3))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .when(active == Some(n), |d| d.bg(color(t.overlay_pressed)))
            .when(active != Some(n), |d| d.hover(move |s| s.bg(hover)))
            .child(div().flex_none().text_size(px(t.text_sm - 1.)).text_color(color(t.text_secondary)).child(dry_core::workbench::glyph(&tab.name)))
            .child(div().flex_none().child(tab.name.clone()))
            .child(div().flex_1().min_w_0().truncate().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(tab.path.clone()))
            .when(tab.unsaved > 0, |d| d.child(dirty(t)))
            .on_click(cx.listener(move |this, _, _, cx| {
                pick(this, &prefix, "tab", key.clone());
                cx.notify();
            }))
    });
    let current = active.and_then(|n| lv.tabs.get(n)).map(|tab| (tab.artifact.clone(), tab.path.clone()));
    let roots = lv.roots.iter().map(|root| {
        let section = div()
            .flex_none()
            .h(px(ROW))
            .pl(px(2.))
            .flex()
            .items_center()
            .gap(px(2.))
            .text_size(px(t.text_sm))
            .font_weight(FontWeight::BOLD)
            .child(div().flex_none().w(px(TWISTIE)).flex().justify_center().child("⌄"))
            .child(div().min_w_0().truncate().child(format!("{} · {}", root.artifact.to_uppercase(), root.root)));
        let rows = match &root.rows {
            Err(why) => vec![div().pl(px(INDENT + TWISTIE)).text_color(color(t.text_disabled)).child(why.clone()).into_any_element()],
            Ok(rows) => {
                let open = app.inspects.borrow().open.clone();
                let flip = |p: &str| open.contains(&format!("live/{prefix}/{}/dir:{p}", root.artifact));
                liveview::visible(rows, &flip)
                    .into_iter()
                    .enumerate()
                    .map(|(n, (row, expanded))| {
                        let here = current.as_ref().is_some_and(|(a, p)| *a == root.artifact && *p == row.path);
                        let mark = if row.dir {
                            div().text_color(color(t.text_primary)).child(if expanded { "⌄" } else { "›" })
                        } else {
                            div().text_size(px(t.text_sm - 1.)).text_color(color(t.text_secondary)).child(dry_core::workbench::glyph(&row.name))
                        };
                        let tone = match (row.unsaved, row.open) {
                            (true, _) => t.text_accent,
                            (false, true) => t.text_primary,
                            (false, false) => t.text_secondary,
                        };
                        let (artifact, path, dir, is_open) = (root.artifact.clone(), row.path.clone(), row.dir, row.open);
                        let prefix = prefix.to_string();
                        let guides = (0..row.depth).map(|level| {
                            div().absolute().top_0().bottom_0().left(px(INDENT + INDENT * level as f32 + TWISTIE / 2.)).w(px(1.)).bg(color(t.border))
                        });
                        div()
                            .relative()
                            .flex_none()
                            .h(px(ROW))
                            .children(guides)
                            .child(
                                div()
                                    .id(SharedString::from(format!("live-tree-{}-{n}", root.artifact)))
                                    .size_full()
                                    .pl(px(INDENT + INDENT * row.depth as f32))
                                    .pr(px(t.space_3))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .when(here, |d| d.bg(color(t.overlay_pressed)))
                                    .when(!here, |d| d.hover(move |s| s.bg(hover)))
                                    .when(dir || is_open, |d| d.cursor_pointer())
                                    .child(div().flex_none().w(px(TWISTIE)).flex().justify_center().child(mark))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_color(color(tone))
                                            .when(row.open && !row.dir, |d| d.font_weight(FontWeight::SEMIBOLD))
                                            .child(row.name.clone()),
                                    )
                                    .when(row.open && !row.dir, |d| {
                                        d.child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_disabled)).child("open"))
                                    })
                                    .when(row.unsaved, |d| d.child(dirty(t)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if dir {
                                            let key = format!("live/{prefix}/{artifact}/dir:{path}");
                                            let set = &mut this.inspects.get_mut().open;
                                            if !set.remove(&key) {
                                                set.insert(key);
                                            }
                                        } else if is_open {
                                            pick(this, &prefix, "tab", format!("{artifact}:{path}"));
                                        }
                                        cx.notify();
                                    })),
                            )
                            .into_any_element()
                    })
                    .collect()
            }
        };
        div().flex_none().flex().flex_col().child(section).children(rows)
    });
    let unsaved: usize = lv.tabs.iter().map(|t| t.unsaved).sum();
    div()
        .flex()
        .flex_col()
        .child(title("Explorer", Some(format!("{} open{}", lv.tabs.len(), if unsaved > 0 { format!(", {unsaved} unsaved") } else { String::new() })), t))
        .child(
            div()
                .id(SharedString::from(format!("live-explorer-{prefix}")))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_none()
                        .h(px(ROW))
                        .pl(px(2.))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .text_size(px(t.text_sm))
                        .font_weight(FontWeight::BOLD)
                        .child(div().flex_none().w(px(TWISTIE)).flex().justify_center().child("⌄"))
                        .child("OPEN EDITORS"),
                )
                .children(open_editors)
                .when(lv.tabs.is_empty(), |d| d.child(div().pl(px(INDENT + TWISTIE)).h(px(ROW)).text_color(color(t.text_disabled)).child("none: the engine opens one with open(path)")))
                .child(div().flex_none().h(px(t.space_2)))
                .children(roots),
        )
}

/// `editor`: the open files as tabs and the picked one's lines.
fn editor(app: &AppView, i: usize, prefix: &str, lv: &Rc<LiveView>, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let base = div().flex().flex_col().bg(color(t.background_body));
    if !lv.parts.contains(&Part::Editor) {
        return base.child(absent(lv.empty.clone().unwrap_or_else(|| "No file artifact in this view: its terminal shows below.".into()), app, i, t, cx));
    }
    let Some(active) = active_tab(app, prefix, &lv.tabs) else {
        return base.child(absent("No files open: the engine opens one with open(path=\"...\").".into(), app, i, t, cx));
    };
    let line_border = color(t.border);
    let fg = color(t.text_primary);
    let tabs = lv.tabs.iter().enumerate().map(|(n, tab)| {
        let on = n == active;
        let key = tab.key();
        let prefix = prefix.to_string();
        div()
            .id(SharedString::from(format!("live-tab-{n}")))
            .flex_none()
            .h_full()
            .pl(px(10.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .border_r_1()
            .border_color(line_border)
            .cursor_pointer()
            .relative()
            .when(on, |d| d.bg(color(t.background_body)).text_color(fg).child(div().absolute().top_0().left_0().right_0().h(px(1.)).bg(color(t.accent))))
            .when(!on, |d| d.border_b_1().text_color(color(t.text_secondary)))
            .child(div().text_size(px(t.text_sm - 1.)).text_color(color(t.text_secondary)).child(dry_core::workbench::glyph(&tab.name)))
            .child(tab.name.clone())
            .when(tab.unsaved > 0, |d| d.child(dirty(t)))
            .on_click(cx.listener(move |this, _, _, cx| {
                pick(this, &prefix, "tab", key.clone());
                cx.notify();
            }))
    });
    let strip = div()
        .id(SharedString::from(format!("live-tabs-{prefix}")))
        .flex_none()
        .h(px(HEADER))
        .flex()
        .overflow_x_scroll()
        .bg(color(t.background_surface))
        .children(tabs)
        .child(div().flex_1().min_w(px(8.)).h_full().border_b_1().border_color(line_border));

    let tab = &lv.tabs[active];
    // Switching to a tab reveals the lines the engine looks at.
    {
        let mut st = app.inspects.borrow_mut();
        let key = format!("{prefix}|{}", tab.key());
        if st.live.revealed.get(prefix) != Some(&key) {
            st.live.revealed.insert(prefix.to_string(), key);
            let first = tab.focus.map_or(0, |(a, _)| a.saturating_sub(4));
            st.live.editor.scroll_to_item(first, ScrollStrategy::Top);
        }
    }
    let crumbs = std::iter::once(tab.artifact.clone()).chain(tab.path.split('/').map(str::to_string)).enumerate().map(|(n, part)| {
        div().flex().items_center().gap(px(4.)).when(n > 0, |d| d.child("›")).child(part)
    });
    let marks = div()
        .flex()
        .items_center()
        .gap(px(t.space_2))
        .children(tab.focus.map(|(a, b)| div().text_color(color(t.text_accent)).child(format!("engine sees lines {a}-{b}"))))
        .when(tab.focus.is_none(), |d| d.child("engine sees the whole file"))
        .when(tab.unsaved > 0, |d| {
            d.child(div().text_color(color(t.text_accent)).child(dry_core::inspect::count(tab.unsaved, "unsaved edit", "unsaved edits")))
        })
        .child(format!("{} · {}", dry_core::inspect::count(tab.lines.len(), "line", "lines"), tab.language));
    let breadcrumbs = div()
        .flex_none()
        .h(px(ROW))
        .pl(px(18.))
        .pr(px(t.space_3))
        .flex()
        .items_center()
        .gap(px(4.))
        .overflow_hidden()
        .text_size(px(t.text_base - 1.))
        .text_color(color(t.text_secondary))
        .children(crumbs)
        .child(div().flex_1())
        .child(marks);
    let text = if tab.lines.is_empty() {
        div().flex_1().p(px(t.space_4)).text_color(color(t.text_secondary)).child(tab.note.clone().unwrap_or_else(|| "(empty file)".into())).into_any_element()
    } else {
        let lh = (t.text_code * 1.5).round();
        let digits = tab.lines.last().map_or(3, |l| l.n.to_string().len()).max(3) as f32;
        let numbers = digits * t.text_code * 0.6;
        let lv = lv.clone();
        uniform_list(
            "live-editor-lines",
            tab.lines.len(),
            cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
                let t = ui::tokens(cx);
                let tab = &lv.tabs[active];
                let focus = tab.focus.is_some();
                range
                    .filter_map(|n| tab.lines.get(n))
                    .map(|l| {
                        let dim = focus && !l.focused;
                        div()
                            .h(px(lh))
                            .flex()
                            .whitespace_nowrap()
                            .when(l.changed, |d| d.bg(color(t.accent_muted)))
                            .when(focus && l.focused && !l.changed, |d| d.bg(color(t.line_highlight)))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(18. + numbers + 8.))
                                    .pl(px(18.))
                                    .pr(px(8.))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .text_color(color(if dim { t.text_disabled } else { t.text_secondary }))
                                    .child(l.n.to_string()),
                            )
                            // The gutter mark a code editor shows for a changed line.
                            .child(div().flex_none().w(px(3.)).h_full().when(l.changed, |d| d.bg(color(t.accent))))
                            .child(div().flex_none().w(px(t.space_3)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .overflow_hidden()
                                    .text_color(color(if dim { t.text_disabled } else { t.text_primary }))
                                    .child(if l.text.is_empty() { " ".to_string() } else { l.text.replace('\t', "    ") }),
                            )
                    })
                    .collect()
            }),
        )
        .track_scroll(&app.inspects.borrow().live.editor)
        .flex_1()
        .min_h_0()
        .font_family(t.font_mono.clone())
        .text_size(px(t.text_code))
        .into_any_element()
    };
    base.child(strip)
        .child(breadcrumbs)
        .children(tab.note.clone().filter(|_| !tab.lines.is_empty()).map(|n| div().pl(px(18.)).text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(n)))
        .child(text)
}

/// A session's status in its tone.
fn session_status(s: &Session, t: &Tokens) -> (String, u32) {
    match (s.view.running, s.view.exit) {
        (true, _) => ("running".into(), t.accent),
        (false, Some(0)) => ("exit 0".into(), t.text_accent),
        (false, _) => (s.view.status.clone(), t.error),
    }
}

/// `panel`: the terminal sessions, the picked one's output beside the list.
fn panel(app: &AppView, i: usize, prefix: &str, lv: &LiveView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let running = lv.sessions.iter().filter(|s| s.view.running).count();
    let tab = div()
        .flex_none()
        .h_full()
        .px(px(t.space_3))
        .flex()
        .items_center()
        .child(div().py(px(4.)).border_b_1().border_color(color(t.accent)).text_color(color(t.text_primary)).child("TERMINAL"));
    let header = div()
        .flex_none()
        .h(px(HEADER))
        .pl(px(t.space_2))
        .pr(px(t.space_4))
        .flex()
        .items_center()
        .text_size(px(t.text_sm))
        .child(tab)
        .child(div().flex_1())
        .when(lv.parts.contains(&Part::Panel), |d| {
            d.child(div().text_color(color(t.text_secondary)).child(format!("{running} running · {} done", lv.sessions.len() - running)))
        });
    let base = div().flex().flex_col().child(header);
    if !lv.parts.contains(&Part::Panel) {
        return base.child(absent("No terminal artifact in this view.".into(), app, i, t, cx));
    }
    let Some(active) = active_session(app, prefix, &lv.sessions) else {
        return base.child(
            div()
                .px(px(20.))
                .text_color(color(t.text_disabled))
                .child("No sessions yet: run(...) waits, start(...) calls back when it ends, background(...) does not."),
        );
    };
    let s = &lv.sessions[active];
    let (state, tone) = session_status(s, t);
    let lh = (t.text_code * 1.25).round();
    let output = div()
        .id(SharedString::from(format!("live-output-{prefix}")))
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(20.))
        .font_family(t.font_mono.clone())
        .text_size(px(t.text_code))
        .line_height(px(lh))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .gap(px(t.space_2))
                .child(div().text_color(color(t.text_secondary)).child("$"))
                .child(div().min_w_0().text_color(color(t.text_primary)).child(s.view.command.replace("\\n", "\n"))),
        )
        .children(s.view.tail.lines().map(|l| div().min_w_0().text_color(color(t.text_primary)).child(if l.is_empty() { " ".to_string() } else { l.to_string() })))
        .child(
            div()
                .pt(px(lh / 2.))
                .flex()
                .gap(px(t.space_2))
                .child(div().text_color(color(tone)).child(format!("● {state}")))
                .child(div().text_color(color(t.text_secondary)).child(format!("session {} · {} · {} · started {}", s.view.id, s.role.label(), s.view.mode, s.view.age))),
        );
    let hover = color(t.overlay_hover);
    let list = lv.sessions.iter().enumerate().map(|(n, x)| {
        let (state, tone) = session_status(x, t);
        let key = x.key();
        let prefix = prefix.to_string();
        div()
            .id(SharedString::from(format!("live-session-{n}")))
            .flex_none()
            .h(px(ROW))
            .px(px(t.space_2))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .when(n == active, |d| d.bg(color(t.overlay_pressed)))
            .when(n != active, |d| d.hover(move |s| s.bg(hover)))
            .child(div().flex_none().w(px(14.)).text_color(color(tone)).child(x.role.glyph()))
            .child(div().flex_1().min_w_0().truncate().child(x.title.clone()))
            .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(tone)).child(state))
            .on_click(cx.listener(move |this, _, _, cx| {
                pick(this, &prefix, "session", key.clone());
                cx.notify();
            }))
    });
    base.child(
        div().flex_1().min_h_0().flex().child(output).child(
            div()
                .id(SharedString::from(format!("live-sessions-{prefix}")))
                .flex_none()
                .w(px(SESSION_LIST))
                .h_full()
                .overflow_y_scroll()
                .border_l_1()
                .border_color(color(t.border))
                .text_size(px(t.text_base - 1.))
                .flex()
                .flex_col()
                .children(list),
        ),
    )
}

/// `chat`: the engine page's conversation and composer in a side bar, the
/// newest message in view.
fn chat(app: &AppView, params: &Params, i: usize, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>) -> Div {
    let e = &app.state.engines[i];
    let n = e.messages.len();
    {
        let mut st = app.inspects.borrow_mut();
        if st.live.seen.get(&e.id) != Some(&n) {
            st.live.seen.insert(e.id.clone(), n);
            st.live.chat.scroll_to_bottom();
        }
    }
    let activity = app.activity(i);
    let tint = activity.token().and_then(|name| t.color(name));
    let head = title("Chat", Some(format!("{} · {}", e.name, activity.label())), t).relative().when_some(tint, |d, tint| d.child(wash(tint, 0.)));
    let scroll = app.inspects.borrow().live.chat.clone();
    div()
        .flex()
        .flex_col()
        .child(head)
        .child(
            div()
                .id(SharedString::from(format!("live-chat-{}", e.id)))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .p(px(t.space_2))
                .child(super::engine::chat(app, params, t, window, cx)),
        )
        .child(div().flex_none().p(px(t.space_2)).child(super::composer::draw(app, params, &dry_core::composer::default_actions(), t, cx)))
}

/// `status`: engine, activity on its tint, model, counts; on the right
/// the prompt against its window, when the snapshot was taken, and the
/// engine and artifact pages.
fn status(app: &AppView, i: usize, lv: &LiveView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let item = |text: String| div().flex_none().h_full().px(px(5.)).flex().items_center().whitespace_nowrap().child(text);
    let s = &lv.status;
    let tint = s.tint.and_then(|name| t.color(name));
    let link = |id: String, text: &str, to: String| {
        let hover = color(t.overlay_hover);
        item(text.to_string()).id(SharedString::from(id)).cursor_pointer().hover(move |d| d.bg(hover)).on_click(cx.listener(move |this, _, window, cx| this.navigate(&to, window, cx)))
    };
    let taken = app.inspects.borrow().taken.get(&i).map(|x| (x.taken, x.when.clone()));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let over = s.gauge.as_ref().is_some_and(|(r, _)| *r >= dry_core::context::COMPRESS_AT);
    div()
        .flex()
        .items_center()
        .px(px(t.space_2))
        .text_size(px(t.text_base - 1.))
        .overflow_hidden()
        .child(item(format!("✦ {}", s.engine)).font_weight(FontWeight::SEMIBOLD))
        .child(item(s.activity.clone()).relative().when_some(tint, |d, tint| d.child(wash(tint, 0.))))
        .child(item(s.model.clone()))
        .when(!s.counts.is_empty(), |d| d.child(item(s.counts.clone())))
        .child(div().flex_1().min_w(px(8.)))
        .when_some(s.gauge.clone(), |d, (ratio, line)| {
            d.child(div().flex_none().w(px(80.)).child(gauge(ratio, if over { t.error } else { t.accent }, t)))
                .child(item(line).when(over, |d| d.text_color(color(t.error))))
        })
        .when_some(taken, |d, (at, when)| {
            d.child(
                item(format!("↻ {} ago · {when}", dry_core::inspect::ago(now - at)))
                    .id("live-refresh")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.take_inspect(i, "refreshed".into(), None);
                        cx.notify();
                    })),
            )
        })
        .when_some(lv.scope.clone(), |d, name| d.child(link("live-artifact".into(), "Details", format!("{}/artifacts/{name}", dry_core::routes::engine_path(i)))))
        .child(link("live-engine".into(), "Engine", dry_core::routes::engine_path(i)))
}

impl AppView {
    /// Keep a live view's output tail moving: while one is on screen and
    /// its engine has a running session, take a new snapshot every
    /// `REFRESH_SECS` (the 1 s tick calls this). Whether one was taken.
    pub(crate) fn refresh_live(&self) -> bool {
        let Some((route, params, _)) = self.router.resolve(&self.view.location) else { return false };
        if !route.pattern.ends_with("/live") {
            return false;
        }
        let Some(i) = params.index("id") else { return false };
        let Some(e) = self.state.engines.get(i) else { return false };
        let due = {
            let x = self.inspects.borrow();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            let running = x.live.cache.as_ref().is_some_and(|(_, v)| v.sessions.iter().any(|s| s.view.running));
            x.taken.get(&i).is_some_and(|x| x.engine == e.id && running && now - x.taken >= REFRESH_SECS)
        };
        if due {
            self.take_inspect(i, "live view".into(), None);
        }
        due
    }
}
