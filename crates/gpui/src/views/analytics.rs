//! Analytics (`dry_core::analytics`) in the window: an engine's Timeline
//! (its last turns as a waterfall, its totals), Home's today, and the
//! analytics page's sections over a day range. The rows come from the
//! core; tables, stats and bars are the display components (`show`), and
//! the waterfall's bars take the activity tokens' colours, as on the web
//! (`crates/html/src/render/analytics.rs`).

use dry_core::analytics::{self, Bar, Part, RANGES};
use dry_core::routes::Params;
use dry_core::spans::Kind;
use dry_core::theme::Tokens;
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*, px, relative};

use super::show;
use crate::ui::color;
use crate::{AppView, button};

const TURNS: usize = 3;

pub fn draw(part: Part, app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    match part {
        Part::Timeline => timeline(app, params, t),
        Part::Today => today(app, t),
        Part::Page => page(app, t, cx),
    }
}

fn dim(text: impl Into<SharedString>, t: &Tokens) -> Div {
    div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(text.into())
}

/// A bar's colour: its activity token, stronger than a tint.
fn bar_color(b: &Bar, t: &Tokens) -> u32 {
    let name = if b.bad {
        "activity_fail"
    } else {
        match b.kind {
            Kind::Llm if b.label.starts_with("compress") => "activity_compress",
            Kind::Llm => "activity_llm",
            Kind::Tool => "activity_tools",
            Kind::Wait => "activity_wait",
            Kind::Approval => "activity_need",
            Kind::Evidence => "activity_work",
            Kind::Task | Kind::Turn | Kind::Step => return t.text_disabled,
        }
    };
    t.color(name).map_or(t.text_disabled, |c| (c & 0xFFFF_FF00) | 0xAA)
}

fn waterfall(w: &analytics::Waterfall, t: &Tokens) -> Div {
    let rows = w.bars.iter().map(|b| {
        let thin = matches!(b.kind, Kind::Turn | Kind::Task | Kind::Step);
        let bar = div()
            .absolute()
            .top(px(if thin { 5. } else { 2. }))
            .bottom(px(if thin { 5. } else { 2. }))
            .left(relative(b.left))
            .w(relative(b.width.max(0.004)))
            .rounded(px(2.))
            .bg(color(bar_color(b, t)));
        let label = div()
            .w(relative(0.34))
            .flex_none()
            .pl(px(b.depth as f32 * 12.))
            .overflow_hidden()
            .text_color(color(if b.bad { t.error } else { t.text_primary }))
            .child(b.label.clone());
        let row = div()
            .flex()
            .items_center()
            .gap(px(t.space_2))
            .child(label)
            .child(div().relative().flex_1().h(px(14.)).rounded(px(3.)).bg(color(t.overlay_hover)).overflow_hidden().child(bar))
            .child(div().w(px(64.)).flex_none().text_right().text_color(color(t.text_secondary)).child(analytics::fmt_ms(b.ms)));
        let note = (!b.note.is_empty() && !thin).then(|| dim(b.note.clone(), t).pl(px(b.depth as f32 * 12. + 8.)));
        div().flex().flex_col().child(row).children(note)
    });
    div().flex().flex_col().gap(px(1.)).text_size(px(t.text_sm)).children(rows)
}

fn timeline(app: &AppView, params: &Params, t: &Tokens) -> Div {
    let Some(e) = params.index("id").and_then(|i| app.state.engines.get(i)) else { return dim("the timeline needs an engine's page", t) };
    let spans = app.span_log.spans.get(&e.id).cloned().unwrap_or_default();
    let tl = analytics::timeline(&spans, &e.id, TURNS);
    let body = match &tl.waterfall {
        None => dim("Nothing recorded yet: each turn's model calls, tools and waits show here once it runs.", t),
        Some(w) => div()
            .flex()
            .flex_col()
            .gap(px(t.space_2))
            .child(dim(format!("The last {TURNS} turn(s), {} from first to last.", analytics::fmt_ms(w.ms)), t))
            .child(waterfall(w, t)),
    };
    crate::frame::glass(t)
        .w_full()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .p(px(t.space_3))
        .child(dim("Timeline", t).font_weight(FontWeight::SEMIBOLD))
        .child(body)
        .child(show::draw(&tl.totals, t))
        .child(dim(format!("{} spans kept", tl.spans), t))
}

fn today(app: &AppView, t: &Tokens) -> Div {
    let clock = dry_tools::live::clock();
    let today = analytics::today(&app.span_log.all(), dry_tools::spans::now_ms(), clock.offset);
    div().flex().flex_col().gap(px(t.space_2)).child(show::draw(&today.stats, t)).children(today.share.as_ref().map(|s| show::draw(s, t))).children(today.slowest.as_ref().map(|s| show::draw(s, t)))
}

fn page(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let days = if RANGES.contains(&app.analytics_days) { app.analytics_days } else { 7 };
    let picker = div().flex().gap(px(t.space_2)).children(RANGES.map(|d| {
        let label = if d == 1 { "Today".to_string() } else { format!("{d} days") };
        let label = if d == days { format!("● {label}") } else { label };
        button(SharedString::from(format!("analytics-days-{d}")), label, t).on_click(cx.listener(move |this, _, _, cx| {
            this.analytics_days = d;
            this.refresh_spans(cx);
            cx.notify();
        }))
    }));
    let spans = app.span_log.all();
    let clock = dry_tools::live::clock();
    let input = analytics::PageInput {
        spans: &spans,
        daily: &app.span_log.daily,
        shared: &app.state.shared,
        engines: app.state.engines.iter().map(|e| (e.id.clone(), e.name.clone(), dry_core::live::toolbox(e))).collect(),
        now_ms: dry_tools::spans::now_ms(),
        offset: clock.offset,
        days,
    };
    let sections = analytics::page(&input).into_iter().map(|s| {
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_2))
            .child(div().text_size(px(t.text_base + 2.)).font_weight(FontWeight::SEMIBOLD).child(s.title.clone()))
            .children(s.shows.iter().map(|sh| show::draw(sh, t)))
            .children((!s.note.is_empty()).then(|| dim(s.note.clone(), t)))
    });
    let empty = (spans.is_empty() && app.span_log.daily.is_empty()).then(|| dim("Nothing recorded yet. Every turn writes its spans to its engine's spans.jsonl; run one and come back.", t));
    div().flex().flex_col().gap(px(t.space_4)).child(picker).children(empty).children(sections)
}
