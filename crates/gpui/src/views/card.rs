//! Draws `dry_core::card::Card`: the one element every backend object is
//! shown as (a program, an engine, ...). A glass card: the icon, the
//! title and a kind chip on top, then its lines, then the status with a
//! dot in the status's tone. The caller adds what clicking it does.

use dry_core::card::{Card, Status, Tone};
use dry_core::theme::Tokens;
use gpui::{Div, FontWeight, div, prelude::*, px};

use crate::ui::color;

/// A tint over the whole of its parent (which must be `relative`), under
/// the parent's other children.
pub fn wash(tint: u32, radius: f32) -> Div {
    div().absolute().top_0().left_0().size_full().rounded(px(radius)).bg(color(tint))
}

/// The colour of a tone.
pub fn tone_color(tone: Tone, t: &Tokens) -> u32 {
    match tone {
        Tone::Neutral => t.text_secondary,
        Tone::Good => t.text_accent,
        Tone::Busy => t.accent,
        Tone::Bad => t.error,
    }
}

/// A status as one line: a dot in its tone, then `word: detail`.
pub fn status_line(status: &Status, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(t.space_1))
        .text_size(px(t.text_sm))
        .child(div().text_color(color(tone_color(status.tone(), t))).child("●"))
        .child(div().min_w_0().text_color(color(t.text_secondary)).child(status.line()))
}

pub fn draw(card: &Card, t: &Tokens) -> Div {
    tinted(card, None, t)
}

/// `draw`, with `tint` (an activity's colour, `dry_core::activity`) laid
/// over the glass: a translucent wash under the card's content.
pub fn tinted(card: &Card, tint: Option<u32>, t: &Tokens) -> Div {
    crate::frame::glass(t)
        .relative()
        .when_some(tint, |d, tint| d.child(wash(tint, t.pane_radius)))
        .w(px(260.))
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .p(px(t.space_3))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().w(px(20.)).flex_none().text_size(px(16.)).text_color(color(t.text_accent)).child(card.icon.clone()))
                .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(card.title.clone()))
                .child(
                    div()
                        .px(px(t.space_2))
                        .rounded(px(t.radius))
                        .bg(color(t.background_muted))
                        .text_size(px(t.text_sm))
                        .text_color(color(t.text_secondary))
                        .child(card.kind.clone()),
                ),
        )
        .children(card.lines.iter().filter(|l| !l.is_empty()).map(|line| {
            div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(line.clone())
        }))
        .child(status_line(&card.status, t))
}

/// The one card of a page's sections: glass, its content in a column.
/// Every section, artifact card and terminal block starts from this.
pub fn panel(t: &Tokens) -> Div {
    crate::frame::glass(t).relative().flex().flex_col().gap(px(t.space_2)).p(px(t.space_3)).min_w_0()
}

/// A small label on the muted fill: a kind, a count, a mode.
pub fn chip(text: impl Into<gpui::SharedString>, t: &Tokens) -> Div {
    div()
        .flex_none()
        .px(px(t.space_2))
        .rounded(px(t.radius))
        .bg(color(t.background_muted))
        .text_size(px(t.text_sm))
        .text_color(color(t.text_secondary))
        .child(text.into())
}

/// A thin bar `ratio` full (0 to 1), in `fill`: a context window, a
/// learning section's cap.
pub fn gauge(ratio: f64, fill: u32, t: &Tokens) -> Div {
    let ratio = ratio.clamp(0.0, 1.0) as f32;
    div()
        .h(px(t.space_1))
        .w_full()
        .rounded(px(t.space_1))
        .bg(color(t.background_muted))
        .overflow_hidden()
        .child(div().h_full().w(gpui::relative(ratio)).rounded(px(t.space_1)).bg(color(fill)))
}
