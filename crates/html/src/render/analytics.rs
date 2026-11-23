//! Analytics (`dry_core::analytics`) as HTML: an engine's Timeline (its
//! last turns as a waterfall, its totals), Home's today, and the
//! `/analytics` page's sections over a day range. The numbers and rows
//! are the core's; tables, stats and bars are the display components
//! (`show::draw`), and the waterfall's bars are tinted with the
//! activity tokens, so a model call is the blue of an engine calling its
//! model.

use std::fmt::Write;

use dry_core::analytics::{self, Bar, Part, RANGES};
use dry_core::spans::Kind;

use super::{Ctx, esc, note, placeholder, section, show};

/// The turns a Timeline shows.
const TURNS: usize = 3;

pub fn draw(out: &mut String, part: Part, cx: &Ctx) {
    match part {
        Part::Timeline => timeline(out, cx),
        Part::Today => today(out, cx),
        Part::Page => page(out, cx),
    }
}

fn now_ms(cx: &Ctx) -> i64 {
    cx.world.clock.unix * 1000
}

/// The activity token a bar is tinted with.
fn token(b: &Bar) -> Option<&'static str> {
    if b.bad {
        return Some("activity_fail");
    }
    Some(match b.kind {
        Kind::Llm if b.label.starts_with("compress") => "activity_compress",
        Kind::Llm => "activity_llm",
        Kind::Tool => "activity_tools",
        Kind::Wait => "activity_wait",
        Kind::Approval => "activity_need",
        Kind::Evidence => "activity_work",
        Kind::Task | Kind::Turn | Kind::Step => return None,
    })
}

fn timeline(out: &mut String, cx: &Ctx) {
    let Some((_, e)) = cx.engine() else { return placeholder(out, "analytics", "the timeline needs an engine's page") };
    let spans = cx.world.reported.spans.get(&e.id).cloned().unwrap_or_default();
    let tl = analytics::timeline(&spans, &e.id, TURNS);
    out.push_str("<div class=\"glass panel timeline\" data-live=\"timeline\">");
    out.push_str(&section("Timeline"));
    match &tl.waterfall {
        None => out.push_str(&note("Nothing recorded yet: each turn's model calls, tools and waits show here once it runs.")),
        Some(w) => {
            let _ = write!(out, "<div class=\"note\">The last {} turn(s), {} from first to last.</div>", TURNS, esc(&analytics::fmt_ms(w.ms)));
            waterfall(out, w);
        }
    }
    show::draw(out, &tl.totals);
    let _ = write!(out, "<div class=\"note\">{} spans kept · <a href=\"/analytics\">all analytics</a></div>", tl.spans);
    out.push_str("</div>");
}

fn waterfall(out: &mut String, w: &analytics::Waterfall) {
    out.push_str("<div class=\"waterfall\">");
    for b in &w.bars {
        let tint = token(b).map_or(String::new(), |t| format!("--bar:var(--{t});"));
        let class = match b.kind {
            Kind::Turn | Kind::Task => "wf-bar wf-group",
            Kind::Step => "wf-bar wf-step",
            _ => "wf-bar",
        };
        let _ = write!(
            out,
            "<div class=\"wf-row{}\" title=\"{} · {} · {}\"><div class=\"wf-label\" style=\"padding-left:{}px\">{}</div><div class=\"wf-track\"><div class=\"{class}\" style=\"{tint}left:{:.2}%;width:max({:.2}%,2px)\"></div></div><div class=\"wf-ms\">{}</div></div>",
            if b.bad { " bad" } else { "" },
            esc(&b.label),
            esc(&analytics::fmt_ms(b.ms)),
            esc(&b.note),
            b.depth * 12,
            esc(&b.label),
            b.left * 100.0,
            b.width * 100.0,
            esc(&analytics::fmt_ms(b.ms)),
        );
        if !b.note.is_empty() && matches!(b.kind, Kind::Llm | Kind::Tool | Kind::Wait | Kind::Approval | Kind::Evidence) {
            let _ = write!(out, "<div class=\"wf-note\" style=\"padding-left:{}px\">{}</div>", b.depth * 12 + 8, esc(&b.note));
        }
    }
    out.push_str("</div>");
}

fn today(out: &mut String, cx: &Ctx) {
    let spans = cx.world.reported.all_spans();
    let t = analytics::today(&spans, now_ms(cx), cx.world.clock.offset);
    out.push_str("<div class=\"today\" data-live=\"today\">");
    show::draw(out, &t.stats);
    if let Some(s) = &t.share {
        show::draw(out, s);
    }
    if let Some(s) = &t.slowest {
        show::draw(out, s);
    }
    out.push_str("<div class=\"row\"><a class=\"btn\" href=\"/analytics\">Analytics</a></div></div>");
}

fn page(out: &mut String, cx: &Ctx) {
    let days = cx.query.days.filter(|d| RANGES.contains(d)).unwrap_or(7);
    out.push_str("<div class=\"analytics\" data-live=\"analytics\"><div class=\"row\">");
    for d in RANGES {
        let label = if d == 1 { "Today".to_string() } else { format!("{d} days") };
        let on = if d == days { " on" } else { "" };
        let _ = write!(out, "<a class=\"btn{on}\" href=\"?days={d}\">{label}</a>");
    }
    out.push_str("</div>");
    let spans = cx.world.reported.all_spans();
    let engines = cx.world.state.engines.iter().map(|e| (e.id.clone(), e.name.clone(), dry_core::live::toolbox(e))).collect();
    let input = analytics::PageInput {
        spans: &spans,
        daily: &cx.world.reported.daily,
        shared: &cx.world.state.shared,
        engines,
        now_ms: now_ms(cx),
        offset: cx.world.clock.offset,
        days,
    };
    if spans.is_empty() && cx.world.reported.daily.is_empty() {
        out.push_str(&note("Nothing recorded yet. Every turn writes its spans to its engine's spans.jsonl; run one and come back."));
    }
    for s in analytics::page(&input) {
        let _ = write!(out, "<section class=\"an-section\"><h2 class=\"heading\">{}</h2>", esc(&s.title));
        for sh in &s.shows {
            show::draw(out, sh);
        }
        if !s.note.is_empty() {
            out.push_str(&note(&s.note));
        }
        out.push_str("</section>");
    }
    out.push_str("</div>");
}
