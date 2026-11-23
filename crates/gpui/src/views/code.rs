//! The primitives an app-like page is built from: `panes` (the layout,
//! with bars: an activity column and a status strip), `explorer`, `editor`
//! and `terminal` (bound to the app's `dry_core::workbench::Workbench`),
//! and `theme-picker`. `routes/code/` composes them into a code editor;
//! none of them knows about that page.
//!
//! Every colour, text size and font is a token; the route's overrides make
//! them dense, square and flat while it is shown. The few fixed sizes
//! below are the workbench's metrics (a code editor's: 22px rows, 35px
//! headers, 48px activity cells), so the parts line up as in one.

use std::rc::Rc;

use dry_core::pages::{self, Align, Axis, Bar, Component, Edge, Pane};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use dry_core::workbench::{self, Row, RunState};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px, uniform_list};
use gpui_component::input::Input;
use gpui_component::{h_resizable, resizable_panel, v_resizable};

use crate::ui::{self, color};
use crate::{AppView, token_color, token_number};

/// One row of a tree or list, and the status strip's items.
const ROW: f32 = 22.;
/// A part's header: the side bar's title, the editor's tabs, the panel's tabs.
const HEADER: f32 = 35.;
/// One cell of the activity column (square).
const CELL: f32 = 48.;
/// How far each level of the tree is indented.
const INDENT: f32 = 8.;
/// The box a folder's chevron (or a file's mark) sits in.
const TWISTIE: f32 = 16.;
/// Room left of the line numbers, and between them and the code.
const GLYPH_MARGIN: f32 = 18.;
const DECORATIONS: f32 = 26.;

/// Height of one line of code.
fn code_line(t: &Tokens) -> f32 {
    (t.text_code * 1.5).round()
}

/// Height of one line of terminal output (set tighter than code).
fn terminal_line(t: &Tokens) -> f32 {
    (t.text_code * 1.25).round()
}

/// UI text one step under the base size: tabs' marks, the status strip,
/// breadcrumbs.
fn text_small(t: &Tokens) -> f32 {
    t.text_base - 1.
}

/// `c` with its alpha scaled by `f` (inactive icons, item hovers).
fn fade(c: u32, f: f32) -> u32 {
    let a = ((c & 0xFF) as f32 * f).round() as u32;
    (c & 0xFFFF_FF00) | a.min(0xFF)
}

/// A pane's fill, padding and edge line, from the token names it gives.
fn tinted(d: Div, pane: &Pane, t: &Tokens) -> Div {
    let d = d
        .when_some(pane.background.as_deref(), |d, name| d.bg(color(token_color(t, name))))
        .when_some(pane.padding.as_deref(), |d, name| d.p(px(token_number(t, name))));
    match (pane.border.as_deref(), pane.edge) {
        (Some(name), Some(edge)) => {
            let d = d.border_color(color(token_color(t, name)));
            match edge {
                Edge::Top => d.border_t_1(),
                Edge::Right => d.border_r_1(),
                Edge::Bottom => d.border_b_1(),
                Edge::Left => d.border_l_1(),
            }
        }
        _ => d,
    }
}

/// The tokens a pane's view is drawn with: a pane that names a `text`
/// colour has all its text in it, the quiet kind too (a status bar on a
/// dark strip in a light theme).
fn pane_tokens(pane: &Pane, t: &Tokens) -> Tokens {
    let mut inner = t.clone();
    if let Some(name) = pane.text.as_deref() {
        let c = token_color(t, name);
        inner.text_primary = c;
        inner.text_secondary = c;
    }
    inner
}

/// `panes`: each pane's view in a box of its size, the ones without a size
/// sharing what is left. Resizable groups use gpui-component's panels
/// (drag the edge); fixed ones are plain flex boxes. `key` keeps nested
/// groups' ids apart.
#[allow(clippy::too_many_arguments)]
pub fn panes(
    axis: Axis,
    resizable: bool,
    list: &[Pane],
    key: &str,
    app: &AppView,
    params: &Params,
    t: &Tokens,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let inner = |n: usize, pane: &Pane, window: &mut Window, cx: &mut Context<AppView>| {
        let pt = pane_tokens(pane, t);
        let key = format!("{key}.{n}");
        let view = match pane.bar {
            Some(kind) => bar(kind, pane.align.unwrap_or_default(), &pane.view, &key, app, &pt, cx),
            None => {
                let gap = pane.gap.as_deref().map_or(0., |name| token_number(t, name));
                let view = super::stack(&pane.view, app, params, &pt, gap, &key, window, cx);
                // A bar (a fixed height in a column) centres its line.
                let centred = axis == Axis::Column && pane.size.is_some();
                view.flex_1().min_h_0().when(centred, |v| v.justify_center())
            }
        };
        tinted(div().size_full().min_w_0().min_h_0().overflow_hidden().flex().flex_col(), pane, t)
            .text_color(color(pt.text_primary))
            .child(view)
    };
    if resizable {
        let mut group = match axis {
            Axis::Row => h_resizable(SharedString::from(format!("panes-{key}"))),
            Axis::Column => v_resizable(SharedString::from(format!("panes-{key}"))),
        }
        // The line between two panes: a hairline in `border`, lit in
        // `accent` while the pointer is on it or drags it.
        .with_handle_appearance({
            let (rest, lit) = (color(t.border), color(t.accent));
            Rc::new(move |handle, _, _| {
                let hot = handle.state() != Default::default();
                let line = div().flex_none().bg(if hot { lit } else { rest });
                Some(
                    match axis {
                        Axis::Row => line.w(px(1.)).h_full(),
                        Axis::Column => line.h(px(1.)).w_full(),
                    }
                    .into_any_element(),
                )
            })
        });
        for (n, pane) in list.iter().enumerate() {
            let mut panel = resizable_panel();
            if let Some(size) = pane.size {
                panel = panel.size(px(size)).fixed();
            }
            let min = pane.min.unwrap_or(60.);
            panel = panel.size_range(px(min)..px(f32::MAX));
            group = group.child(panel.child(inner(n, pane, window, cx)));
        }
        return div().size_full().min_h_0().min_w_0().child(group).into_any_element();
    }
    let mut row = div().size_full().min_w_0().min_h_0().flex().overflow_hidden();
    row = match axis {
        Axis::Row => row.flex_row(),
        Axis::Column => row.flex_col(),
    };
    for (n, pane) in list.iter().enumerate() {
        let cell = inner(n, pane, window, cx);
        let cell = match (pane.size, axis) {
            (Some(size), Axis::Row) => div().flex_none().w(px(size)).h_full().child(cell),
            (Some(size), Axis::Column) => div().flex_none().h(px(size)).w_full().child(cell),
            (None, _) => div().flex_1().min_w_0().min_h_0().child(cell),
        };
        row = row.child(cell);
    }
    row.into_any_element()
}

/// A pane drawn as a bar (`pages::Bar`): its `link`s, `text`s and
/// `theme-picker` as the bar's items; anything else is left out.
fn bar(kind: Bar, align: Align, view: &[Component], key: &str, app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let values = super::values(app);
    let items = view.iter().enumerate().filter_map(|(n, c)| {
        let id = SharedString::from(format!("bar-{key}-{n}"));
        match (kind, c) {
            (Bar::Activity, Component::Link { label, to }) => Some(activity_cell(id, label, to, app, t, cx).into_any_element()),
            (Bar::Status, Component::Link { label, to }) => {
                let to = to.clone();
                Some(
                    status_item(id, label.clone(), t)
                        .on_click(cx.listener(move |this, _, window, cx| this.navigate(&to, window, cx)))
                        .into_any_element(),
                )
            }
            (Bar::Status, Component::Text { text }) => {
                // An item naming a value that is blank now (no file open) hides.
                let blank = values.iter().any(|(name, v)| v.is_empty() && text.contains(&format!("{{{name}}}")));
                (!blank).then(|| status_item(id, pages::fill(text, &values), t).into_any_element())
            }
            (Bar::Status, Component::ThemePicker) => Some(theme_item(id, app, t, cx).into_any_element()),
            _ => None,
        }
    });
    let d = div().size_full().min_w_0().min_h_0().flex().overflow_hidden().children(items);
    match kind {
        Bar::Activity => d.flex_col().items_center(),
        Bar::Status => d
            .flex_row()
            .items_center()
            .px(px(t.space_2))
            .text_size(px(text_small(t)))
            .when(align == Align::End, |d| d.justify_end()),
    }
}

/// One cell of the activity column: the link's label as its icon, quiet
/// until hovered; the page on screen is lit and marked on its left edge.
fn activity_cell(id: SharedString, label: &str, to: &str, app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> impl IntoElement {
    let active = app.view.location == to;
    let lit = color(t.text_primary);
    let to = to.to_string();
    div()
        .id(id)
        .flex_none()
        .w(px(CELL))
        .h(px(CELL))
        .flex()
        .items_center()
        .justify_center()
        .border_l_2()
        .border_color(color(if active { t.accent } else { 0 }))
        .text_size(px(t.text_base * 1.6))
        .text_color(color(if active { t.text_primary } else { fade(t.text_primary, 0.6) }))
        .hover(move |s| s.text_color(lit))
        .cursor_pointer()
        .child(label.to_string())
        .on_click(cx.listener(move |this, _, window, cx| this.navigate(&to, window, cx)))
}

/// One item of the status strip: its text, washed lighter when hovered.
fn status_item(id: SharedString, text: String, t: &Tokens) -> gpui::Stateful<Div> {
    let hover = color(fade(t.text_primary, 0.12));
    div()
        .id(id)
        .flex_none()
        .h_full()
        .px(px(5.))
        .flex()
        .items_center()
        .whitespace_nowrap()
        .hover(move |s| s.bg(hover))
        .child(text)
}

/// The theme picker as one status item: the theme in use; a click moves
/// to the next one.
fn theme_item(id: SharedString, app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> gpui::Stateful<Div> {
    let (names, active, own) = match app.route_themes() {
        Some((names, active)) => (names, active, true),
        None => (app.themes.0.clone(), app.themes.1.clone(), false),
    };
    let next = names.iter().position(|n| *n == active).map_or(0, |i| (i + 1) % names.len().max(1));
    let pick = names.get(next).cloned();
    status_item(id, format!("◐ {}", pages::label(&active)), t).cursor_pointer().on_click(cx.listener(move |this, _, window, cx| {
        let Some(pick) = &pick else { return };
        if own {
            this.use_route_theme(pick, cx);
        } else {
            this.use_theme(pick, window, cx);
        }
    }))
}

/// A part's header: its name small and upper-case ("EXPLORER").
fn title(text: &str, t: &Tokens) -> Div {
    div()
        .flex_none()
        .h(px(HEADER))
        .pl(px(20.))
        .pr(px(t.space_3))
        .flex()
        .items_center()
        .text_size(px(t.text_sm))
        .text_color(color(t.text_primary))
        .child(text.to_uppercase())
}

/// A folder's chevron: pointing right when closed, down when open. The
/// down one sits low in its font, so it is lifted to line up.
fn chevron(open: bool, t: &Tokens) -> Div {
    let size = t.text_base + 1.;
    div()
        .text_size(px(size))
        .text_color(color(t.text_primary))
        .when(open, |d| d.relative().top(px(-size * 0.3)))
        .child(if open { "⌄" } else { "›" })
}

/// `explorer`: the repo's folder tree under its name. A folder opens or
/// closes on click; a file opens in the editor. Lines mark each level.
pub fn explorer(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let current = app.workbench.current().map(|f| f.path.clone());
    let guide = color(t.border);
    let rows = app.workbench.tree.rows().into_iter().enumerate().map(|(n, row)| {
        let (depth, body) = match row {
            Row::Note { text, depth } => (
                depth,
                div()
                    .pl(px(INDENT + INDENT * depth as f32 + TWISTIE))
                    .flex()
                    .items_center()
                    .text_color(color(t.text_disabled))
                    .truncate()
                    .child(text)
                    .into_any_element(),
            ),
            Row::Entry { entry, depth, open } => {
                let active = current.as_deref() == Some(entry.path.as_str());
                let hover = color(t.overlay_hover);
                // A folder shows its chevron; a file its mark, where a
                // folder's chevron would be.
                let mark = if entry.dir {
                    chevron(open, t)
                } else {
                    div().text_size(px(t.text_sm - 1.)).text_color(color(t.text_secondary)).child(workbench::glyph(&entry.name))
                };
                let path = entry.path.clone();
                let dir = entry.dir;
                (
                    depth,
                    div()
                        .id(SharedString::from(format!("tree-{n}-{}", entry.path)))
                        .size_full()
                        .pl(px(INDENT + INDENT * depth as f32))
                        .pr(px(t.space_3))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(color(t.overlay_pressed)))
                        .when(!active, |d| d.hover(move |s| s.bg(hover)))
                        .child(div().flex_none().w(px(TWISTIE)).flex().justify_center().child(mark))
                        .child(div().flex_1().min_w_0().truncate().child(entry.name))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if dir {
                                this.toggle_folder(&path, cx);
                            } else {
                                this.open_file(&path, cx);
                            }
                        }))
                        .into_any_element(),
                )
            }
        };
        // One line per level above this one, through its folder's chevron.
        let guides = (0..depth).map(move |level| {
            div().absolute().top_0().bottom_0().left(px(INDENT + INDENT * level as f32 + TWISTIE / 2.)).w(px(1.)).bg(guide)
        });
        div().relative().flex_none().h(px(ROW)).children(guides).child(body)
    });
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_h_0()
        .child(title("Explorer", t))
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
                .child(div().flex_none().w(px(TWISTIE)).flex().justify_center().child(chevron(true, t)))
                .child(app.repo_name.to_uppercase()),
        )
        .child(div().id("explorer-rows").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().children(rows))
        .children(engine_files(app, t, cx))
}

/// Under the tree: each `file` artifact an engine holds; a click opens
/// its open files as tabs, the files the engine sees.
fn engine_files(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Option<Div> {
    let held = app.file_artifacts();
    if held.is_empty() {
        return None;
    }
    let hover = color(t.overlay_hover);
    let rows = held.into_iter().map(|(i, name, label)| {
        div()
            .id(SharedString::from(format!("engine-files-{i}-{name}")))
            .flex_none()
            .h(px(ROW))
            .pl(px(INDENT + TWISTIE))
            .pr(px(t.space_3))
            .flex()
            .items_center()
            .truncate()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.show_file_artifact(i, &name, cx)))
    });
    Some(div().flex_none().flex().flex_col().border_t_1().border_color(color(t.border)).child(title("Engine files", t)).children(rows))
}

/// `editor`: the open files as tabs, the active one's path as breadcrumbs,
/// and its lines with their numbers. Read-only; a click puts the cursor
/// on a line. Only the lines on screen are drawn.
pub fn editor(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let fg = color(t.text_primary);
    let line_border = color(t.border);
    let tabs = app.workbench.tabs.iter().enumerate().map(|(i, file)| {
        let active = app.workbench.active == Some(i);
        let group = SharedString::from(format!("tab-{i}"));
        let close_hover = color(t.overlay_hover);
        div()
            .id(("tab", i))
            .group(group.clone())
            .flex_none()
            .h_full()
            .pl(px(10.))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(6.))
            .border_r_1()
            .border_color(line_border)
            .cursor_pointer()
            .relative()
            // The active tab: the editor's colour, a line of accent along its top.
            .when(active, |d| {
                d.bg(color(t.background_body)).text_color(fg).child(div().absolute().top_0().left_0().right_0().h(px(1.)).bg(color(t.accent)))
            })
            .when(!active, |d| d.border_b_1().text_color(color(t.text_secondary)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.workbench.focus(i);
                cx.notify();
            }))
            .child(div().text_size(px(t.text_sm - 1.)).text_color(color(t.text_secondary)).child(workbench::glyph(file.name())))
            .child(file.name().to_string())
            .child(
                div()
                    .id(("close", i))
                    .w(px(20.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(t.radius))
                    .text_color(color(t.text_secondary))
                    .when(!active, |d| d.invisible().group_hover(group, |s| s.visible()))
                    .hover(move |s| s.bg(close_hover).text_color(fg))
                    .child("×")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.workbench.close(i);
                        cx.notify();
                    })),
            )
    });
    // The strip's own bottom line runs on past the last tab; the active
    // tab has none, so it opens into the text below.
    let strip = div()
        .id("tabs")
        .flex_none()
        .h(px(HEADER))
        .flex()
        .overflow_x_scroll()
        .bg(color(t.background_surface))
        .children(tabs)
        .child(div().flex_1().min_w(px(8.)).h_full().border_b_1().border_color(line_border));

    let body = match app.workbench.current() {
        None => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(color(t.text_disabled))
            .child("Open a file from the explorer.")
            .into_any_element(),
        Some(file) => {
            let crumbs = file.path.split('/').enumerate().map(|(n, part)| {
                div().flex().items_center().gap(px(4.)).when(n > 0, |d| d.child("›")).child(part.to_string())
            });
            let breadcrumbs = div()
                .flex_none()
                .h(px(ROW))
                .pl(px(GLYPH_MARGIN))
                .flex()
                .items_center()
                .gap(px(4.))
                .overflow_hidden()
                .text_size(px(text_small(t)))
                .text_color(color(t.text_secondary))
                .children(crumbs);
            let text = match &file.lines {
                Err(why) => div().flex_1().p(px(t.space_4)).text_color(color(t.text_secondary)).child(why.clone()).into_any_element(),
                Ok(lines) => {
                    let lh = code_line(t);
                    // Room for the widest number, in the monospace digits' width.
                    let numbers = file.gutter().max(3) as f32 * t.text_code * 0.6;
                    uniform_list(
                        "editor-lines",
                        lines.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                            let t = ui::tokens(cx);
                            let Some(file) = this.workbench.current() else { return Vec::new() };
                            let Ok(lines) = &file.lines else { return Vec::new() };
                            let cursor = file.cursor;
                            range
                                .filter_map(|n| lines.get(n).map(|l| (n, l)))
                                .map(|(n, line)| {
                                    let here = n == cursor;
                                    div()
                                        .id(("line", n))
                                        .h(px(lh))
                                        .flex()
                                        .whitespace_nowrap()
                                        .cursor_text()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.workbench.set_cursor(n);
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex_none()
                                                .w(px(GLYPH_MARGIN + numbers + DECORATIONS))
                                                .pl(px(GLYPH_MARGIN))
                                                .pr(px(DECORATIONS))
                                                .flex()
                                                .items_center()
                                                .justify_end()
                                                .text_color(color(if here { t.text_primary } else { t.text_disabled }))
                                                .child((n + 1).to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .items_center()
                                                .overflow_hidden()
                                                .when(here, |d| d.border_2().border_color(color(t.line_highlight)))
                                                .child(line.clone()),
                                        )
                                })
                                .collect()
                        }),
                    )
                    .track_scroll(&app.editor_scroll)
                    .flex_1()
                    .min_h_0()
                    .font_family(t.font_mono.clone())
                    .text_size(px(t.text_code))
                    .into_any_element()
                }
            };
            div().flex_1().min_h_0().flex().flex_col().child(breadcrumbs).child(text).into_any_element()
        }
    };
    // No file open: no strip either, only the empty editor.
    let open = !app.workbench.tabs.is_empty();
    div().flex().flex_col().size_full().min_h_0().bg(color(t.background_body)).when(open, |d| d.child(strip)).child(body)
}

/// `terminal`: the panel's tab strip, what each command typed printed
/// (newest at the bottom), and the line to type the next one into. Enter
/// runs it in the repo folder; nothing runs any other way.
pub fn terminal(app: &AppView, t: &Tokens) -> Div {
    let lh = terminal_line(t);
    let runs = app.workbench.terminal.runs().iter().map(|run| {
        let state = match &run.state {
            RunState::Running => Some(("running…".to_string(), t.text_secondary)),
            RunState::Exited(Some(0)) => None,
            RunState::Exited(Some(code)) => Some((format!("exit {code}"), t.error)),
            RunState::Exited(None) => Some(("stopped".to_string(), t.error)),
            RunState::Failed(why) => Some((why.clone(), t.error)),
        };
        div()
            .flex()
            .flex_col()
            .pb(px(lh / 2.))
            .child(
                div()
                    .flex()
                    .gap(px(t.space_2))
                    .child(div().text_color(color(t.text_secondary)).child("$"))
                    .child(div().text_color(color(t.text_primary)).child(run.command.clone())),
            )
            .when(!run.output.is_empty(), |d| d.child(div().text_color(color(t.text_primary)).child(run.output.join("\n"))))
            .when_some(state, |d, (text, tone)| d.child(div().text_color(color(tone)).child(text)))
    });
    let busy = app.workbench.terminal.busy();
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
        .child(div().text_color(color(t.text_secondary)).child(format!("sh · {}", app.repo_name)));
    div()
        .flex()
        .flex_col()
        .size_full()
        .min_h_0()
        .child(header)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .font_family(t.font_mono.clone())
                .text_size(px(t.text_code))
                .line_height(px(lh))
                .child(
                    div()
                        .id("terminal-runs")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&app.terminal_scroll)
                        .px(px(20.))
                        .children(runs),
                )
                .child(
                    div()
                        .flex_none()
                        .pl(px(20.))
                        .pr(px(t.space_2))
                        .pb(px(t.space_1))
                        .flex()
                        .items_center()
                        .gap(px(t.space_2))
                        .child(div().flex_none().text_color(color(t.text_secondary)).child(if busy { "…" } else { "$" }))
                        .child(div().flex_1().min_w_0().child(Input::new(&app.terminal_input).appearance(false))),
                ),
        )
}

/// `theme-picker`: one button per theme of the route on screen (its
/// folder's `themes/`), the one in use lit; switching is live. A route
/// without themes lists the app's own, as Settings does. In a status bar
/// it is one item instead (see `bar`).
pub fn theme_picker(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let local = app.route_themes();
    let (names, active, own) = match &local {
        Some((names, active)) => (names.clone(), active.clone(), true),
        None => (app.themes.0.clone(), app.themes.1.clone(), false),
    };
    let buttons = names.into_iter().map(|name| {
        let lit = name == active;
        let label = pages::label(&name);
        let pick = name.clone();
        crate::button(SharedString::from(format!("theme-{name}")), label, t)
            .h(px(ROW))
            .px(px(t.space_2))
            .when(lit, |b| b.bg(color(t.accent)).text_color(color(t.on_accent)))
            .on_click(cx.listener(move |this, _, window, cx| {
                if own {
                    this.use_route_theme(&pick, cx);
                } else {
                    this.use_theme(&pick, window, cx);
                }
            }))
    });
    div().flex().flex_wrap().items_center().gap(px(t.space_1)).children(buttons)
}
