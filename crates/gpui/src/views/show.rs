//! The display components (`dry_core::show::Show`), drawn: what a reply
//! shows with `reply(show=...)`, and the `show` component of a page. Core
//! reads them and says what they mean (`fraction`, `trend`); this file
//! only draws, from tokens. Nothing in them takes input.

use dry_core::show::{Show, Trend};
use dry_core::theme::Tokens;
use gpui::{Div, FontWeight, SharedString, div, prelude::*, px, relative};

use crate::ui::color;

fn dim(text: impl Into<SharedString>, t: &Tokens) -> Div {
    div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(text.into())
}

/// Draw `s`.
pub fn draw(s: &Show, t: &Tokens) -> Div {
    let panel = crate::frame::glass(t).w_full().max_w(px(560.)).flex().flex_col().gap(px(t.space_2)).p(px(t.space_3));
    let panel = if s.caption().is_empty() { panel } else { panel.child(dim(s.caption().to_string(), t).font_weight(FontWeight::SEMIBOLD)) };
    match s {
        Show::Table { head, rows, .. } => {
            let line = |cells: &[String], header: bool| {
                div()
                    .flex()
                    .gap(px(t.space_3))
                    .py(px(t.space_1))
                    .border_b_1()
                    .border_color(color(if header { t.border_emphasized } else { t.border }))
                    .children(cells.iter().map(|c| {
                        let cell = div().flex_1().min_w_0().text_size(px(t.text_sm)).child(c.clone());
                        if header { cell.text_color(color(t.text_secondary)).font_weight(FontWeight::SEMIBOLD) } else { cell }
                    }))
            };
            panel.child(line(head, true)).children(rows.iter().map(|r| line(r, false)))
        }
        Show::List { items, .. } => panel.children(items.iter().map(|i| {
            div().flex().gap(px(t.space_2)).child(div().text_color(color(t.text_accent)).child("•")).child(div().flex_1().min_w_0().child(i.clone()))
        })),
        Show::Progress { .. } => panel
            .child(
                div()
                    .w_full()
                    .h(px(t.space_1 * 1.5))
                    .rounded(px(t.space_1))
                    .bg(color(t.background_muted))
                    .child(div().h_full().w(relative(s.fraction() as f32)).rounded(px(t.space_1)).bg(color(t.accent))),
            )
            .child(dim(s.progress_text(), t)),
        Show::Stat { stats, .. } => panel.child(div().flex().flex_wrap().gap(px(t.space_4)).children(stats.iter().map(|st| {
            let tone = match st.trend() {
                Trend::Up => t.text_accent,
                Trend::Down => t.error,
                Trend::Flat => t.text_secondary,
            };
            div()
                .flex()
                .flex_col()
                .min_w(px(96.))
                .child(dim(st.label.clone(), t))
                .child(div().text_size(px(t.text_lg)).font_weight(FontWeight::SEMIBOLD).child(st.value.clone()))
                .when(!st.delta.is_empty(), |d| d.child(div().text_size(px(t.text_sm)).text_color(color(tone)).child(st.delta.clone())))
        }))),
    }
}
