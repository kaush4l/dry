//! The primitives a page's view is drawn with.
//!
//! Pages are files (`routes/**/page.json`); their views are lists of
//! `dry_core::pages::Component`. `draw` draws such a list: each component's
//! `type` is matched right here, so there is no registry and no mapping to
//! keep in step. The bound primitives live in the files of this folder
//! (`engine::chat`, `ui_page::tokens`, `settings::*`). A new kind of
//! primitive is one variant in `Component` and one arm in `draw`; a new
//! page is only a file.

mod analytics;
mod artifact;
mod ask;
pub mod card;
mod code;
mod composer;
mod desk;
mod engine;
pub(crate) mod live_view;
mod settings;
mod show;
mod ui_page;

use dry_core::pages::{self, Component};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use gpui::{Context, Div, SharedString, Window, div, prelude::*, px};

use crate::ui::color;
use crate::{AppView, button};

/// Draw a view: its components top to bottom.
pub fn draw(view: &[Component], app: &AppView, params: &Params, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>) -> Div {
    let column = stack(view, app, params, t, t.space_3, "view", window, cx);
    // A view in panes fills the box the frame gives it.
    if pages::fills(view) { column.size_full().min_h_0() } else { column }
}

/// `draw` with `gap` between the components; a pane's view is drawn with
/// none, so its parts meet edge to edge. `key` names where in the page
/// this view is, so nested panes get ids of their own.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stack(
    view: &[Component],
    app: &AppView,
    params: &Params,
    t: &Tokens,
    gap: f32,
    key: &str,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> Div {
    let values = values(app);
    let mut column = div().flex().flex_col().gap(px(gap));
    for (n, component) in view.iter().enumerate() {
        column = match component {
            Component::Heading { text } => {
                let text = &pages::fill(text, &values);
                column.child(div().text_size(px(t.text_base + 2.)).font_weight(gpui::FontWeight::SEMIBOLD).child(text.clone()))
            }
            Component::Text { text } => column.child(div().text_color(color(t.text_secondary)).child(pages::fill(text, &values))),
            Component::Link { label, to } => {
                let to = to.clone();
                column.child(
                    div().flex().child(
                        button(SharedString::from(format!("link-{n}")), label.clone(), t)
                            .on_click(cx.listener(move |this, _, window, cx| this.navigate(&to, window, cx))),
                    ),
                )
            }
            Component::EngineHeader => column.child(engine::header(app, params, t)),
            Component::Chat => column.child(engine::chat(app, params, t, window, cx)),
            Component::Composer { actions } => column.child(composer::draw(app, params, actions, t, cx)),
            Component::Tokens => column.child(ui_page::tokens(app, t, cx)),
            Component::Appearance => column.child(settings::appearance(app, t, cx)),
            Component::ResetHistory => column.child(settings::reset_history(app, t, cx)),
            Component::Models => column.child(settings::models(app, t, cx)),
            Component::Routes => column.child(settings::routes(app, t, cx)),
            Component::Diagnostics => column.child(settings::diagnostics(t, window, cx)),
            Component::Environment => column.child(settings::environment(app, t)),
            Component::NewRoute => column.child(settings::new_route(app, t, cx)),
            Component::Engines => column.child(engine::cards(app, t, cx)),
            Component::Desk => column.flex_1().child(desk::desk(app, t, cx)),
            Component::Session => column.child(desk::session(app, params, t, window, cx)),
            Component::Dashboard => column.child(engine::dashboard(app, t)),
            Component::Metadata => column.child(engine::metadata(app, params, t, cx)),
            Component::Tools => column.child(engine::tools(app, t)),
            Component::Capabilities => column.child(settings::capabilities(app, t)),
            Component::Response => column.child(engine::response(app, params, t, cx)),
            Component::Artifacts => column.child(engine::artifacts(app, params, t, cx)),
            Component::EngineTools => column.child(engine::tools_list(app, params, t, cx)),
            Component::Prompt => column.child(engine::prompt(app, params, t, cx)),
            Component::ArtifactPage => column.child(artifact::page(app, params, t, cx)),
            Component::Columns { main, side, side_width } => {
                let key = format!("{key}.{n}");
                let main = stack(main, app, params, t, t.space_3, &format!("{key}.main"), window, cx);
                let side = stack(side, app, params, t, t.space_3, &format!("{key}.side"), window, cx);
                column.child(columns(app, main, side, side_width.unwrap_or(SIDE_WIDTH), t))
            }
            Component::Panes { axis, resizable, panes } => {
                let key = format!("{key}.{n}");
                column = column.size_full().min_h_0();
                column.child(div().flex_1().min_h_0().child(code::panes(*axis, *resizable, panes, &key, app, params, t, window, cx)))
            }
            Component::Explorer => column.child(code::explorer(app, t, cx).flex_1()),
            Component::Editor => column.child(code::editor(app, t, cx).flex_1()),
            Component::Terminal => column.child(code::terminal(app, t).flex_1()),
            Component::ThemePicker => column.child(code::theme_picker(app, t, cx)),
            Component::LiveView { part } => column.child(live_view::draw(*part, app, params, t, window, cx).flex_1()),
            // `expand` has already replaced these; one left means a bug.
            Component::Use { name } => column.child(format!("(use {name})")),
            // On a page, the answer goes to the route's engine, if it has one.
            Component::Ask { ask } => {
                let target = ask::Target { engine: params.index("id"), key: format!("{key}-ask-{n}") };
                column.child(ask::draw(ask, target, dry_core::ask::State::Open, t, window, cx))
            }
            Component::Show { show } => column.child(show::draw(show, t)),
            Component::Analytics { part } => column.child(analytics::draw(*part, app, params, t, cx)),
        };
    }
    column
}

/// The side column's width when a page does not say.
pub const SIDE_WIDTH: f32 = 340.0;
/// The least room the main column keeps beside the side one; less than
/// this and the page is one column.
pub const MAIN_MIN: f32 = 420.0;

/// Two columns where the page's box has room (`AppView::content_width`):
/// `main` taking what is left, `side` at `width`; else one column, `main`
/// first. The side column's sections are cards of their own.
pub(crate) fn columns(app: &AppView, main: Div, side: Div, width: f32, t: &Tokens) -> Div {
    if app.content_width >= width + MAIN_MIN + t.space_4 {
        div()
            .flex()
            .items_start()
            .gap(px(t.space_4))
            .child(main.flex_1().min_w_0())
            .child(side.w(px(width)).flex_none().min_w_0())
    } else {
        div().flex().flex_col().gap(px(t.space_4)).child(main.min_w_0()).child(side.min_w_0())
    }
}

/// Is the fold `key` open? A fold opens and closes with a click; `key` is
/// in `Inspects::open` while it is the other way from `default`.
pub(crate) fn is_open(app: &AppView, key: &str, default: bool) -> bool {
    app.inspects.borrow().open.contains(key) != default
}

/// A section card that opens and closes: its header (a chevron, the title
/// and a count chip), click to toggle. The caller
/// adds the body when `open` (`is_open`), so a closed one builds nothing.
pub(crate) fn fold(key: String, open: bool, title: &str, count: Option<String>, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let head = div()
        .id(SharedString::from(format!("fold-{key}")))
        .flex()
        .items_center()
        .gap(px(t.space_2))
        .cursor_pointer()
        .child(div().flex_none().w(px(t.space_3)).text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(if open { "▾" } else { "▸" }))
        .child(div().flex_none().font_weight(gpui::FontWeight::SEMIBOLD).child(title.to_string()))
        .children(count.map(|c| card::chip(c, t)))
        .child(div().flex_1())
        .on_click(cx.listener(move |this, _, _, cx| {
            let set = &mut this.inspects.get_mut().open;
            if !set.remove(&key) {
                set.insert(key.clone());
            }
            cx.notify();
        }));
    card::panel(t).child(head)
}

/// Live values a text can name in braces (`{file}`, `{lines}`, ...).
pub(crate) fn values(app: &AppView) -> [(&'static str, String); 8] {
    let file = app.workbench.current();
    [
        ("engines", app.state.engines.len().to_string()),
        ("mode", app.state.mode.as_str().to_string()),
        ("file", file.map_or_else(|| "no file open".to_string(), |f| f.path.clone())),
        // Blank with no file open, so a status item naming them hides.
        ("lines", file.map_or_else(String::new, |f| f.len().to_string())),
        ("line", file.map_or_else(String::new, |f| (f.cursor + 1).to_string())),
        ("language", file.map_or("", |f| f.language()).to_string()),
        ("repo", app.repo_name.clone()),
        ("theme", app.route_themes().map_or_else(|| app.themes.1.clone(), |(_, active)| pages::label(&active))),
    ]
}

/// Shown for a path no route matches.
pub fn missing(path: &str, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(div().text_size(px(t.text_lg)).child("Not found"))
        .child(div().text_color(color(t.text_secondary)).child(format!("Nothing is registered for {path}.")))
}

/// A heading and a line of explanation, used at the top of every page.
pub fn intro(title: &str, line: &str, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .child(div().text_size(px(t.text_lg)).child(title.to_string()))
        .child(div().text_color(color(t.text_secondary)).child(line.to_string()))
}

/// A small grey section label.
pub fn section(label: &str, t: &Tokens) -> Div {
    div().pt(px(t.space_2)).text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(label.to_string())
}
