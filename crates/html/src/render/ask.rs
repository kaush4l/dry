//! The ask components (`dry_core::ask::Ask`), drawn like the window's
//! (`crates/gpui/src/views/ask.rs`): choices as buttons, a calendar, a
//! number with its range, a text field, a card of rows. An answered one
//! shows its answer.
//!
//! An open one is disabled on the read-only server: answering is sending.
//! Live (the browser), it answers a reply in the chat (`target`: the
//! engine and the message): each control is a button with `data-act`
//! (`ask` answers; `ask-toggle`, `ask-pick`, `ask-step` and `ask-edit`
//! only change what the box shows) and the value it carries (`data-value`,
//! `data-day`, `data-minute`), plus the box's own inputs. The page reads
//! them and the core writes the answer (`Ask::answer`), so every shell
//! sends the same text.
//!
//! A form is one box of fields, each a `.ask-field` (`data-field`, its
//! number) holding the same controls, which only pick (`ask-pick`,
//! `ask-toggle`, `ask-step`) until the one Submit (`ask`). The page reads
//! each field into a `dry_core::ask::Input` and the core writes the answer
//! (`Ask::submit`).

use std::fmt::Write;

use dry_core::ask::{self, Ask, State};
use dry_core::live::civil_from_days;

use super::{Ctx, esc};

/// One control: `act` with `attrs` when this box answers, else off.
fn control(cx: &Ctx, live: bool, label: &str, class: &str, act: &str, attrs: &str) -> String {
    if live { cx.act(label, class, &format!("data-act=\"{act}\" {attrs}")) } else { cx.off(label, class) }
}

/// Draw `a`. `target`: the engine and the reply it answers (none for an
/// ask a page file shows on its own, which never answers).
pub fn draw(out: &mut String, a: &Ask, state: State<'_>, target: Option<(usize, usize)>, cx: &Ctx) {
    let answered = matches!(state, State::Answered(_));
    let live = cx.world.live() && target.is_some();
    let data = target.map_or(String::new(), |(e, m)| format!(" data-engine=\"{e}\" data-msg=\"{m}\""));
    let _ = write!(
        out,
        "<div class=\"glass panel ask{}\" data-ask=\"{}\"{data}><div class=\"section strong\">{}</div>",
        if answered { " answered" } else { "" },
        a.name(),
        esc(&a.name().to_uppercase())
    );
    if let State::Answered(answer) = state {
        answer_of(out, a, answer);
        out.push_str("</div>");
        return;
    }
    let c = |label: &str, class: &str, act: &str, attrs: &str| control(cx, live, label, class, act, attrs);
    let field = |attrs: &str| if live { attrs.to_string() } else { format!("disabled title=\"{}\"", esc(cx.world.off_reason())) };
    match a {
        Ask::Choice { options } => {
            out.push_str("<div class=\"row\">");
            for o in options {
                out.push_str(&c(o, "outline", "ask", &format!("data-value=\"{}\"", esc(o))));
            }
            out.push_str("</div>");
        }
        Ask::Multi { options } => {
            out.push_str("<div class=\"row\">");
            for o in options {
                out.push_str(&c(&format!("☐ {o}"), "", "ask-toggle", &format!("data-value=\"{}\"", esc(o))));
            }
            let _ = write!(out, "</div><div class=\"row\">{}</div>", c("Send", "primary", "ask", ""));
        }
        Ask::Confirm => {
            let _ = write!(out, "<div class=\"row\">{}{}</div>", c("Yes", "primary", "ask", "data-value=\"yes\""), c("No", "", "ask", "data-value=\"no\""));
        }
        Ask::Date | Ask::DateTime => date(out, matches!(a, Ask::DateTime), live, cx),
        Ask::Number { min, max, unit } => {
            let range = match (min, max) {
                (Some(lo), Some(hi)) => format!("{} to {}", ask::number_text(*lo), ask::number_text(*hi)),
                (Some(lo), None) => format!("at least {}", ask::number_text(*lo)),
                (None, Some(hi)) => format!("at most {}", ask::number_text(*hi)),
                (None, None) => String::new(),
            };
            let _ = write!(
                out,
                "<div class=\"row\">{}<input class=\"field-in\" name=\"number\" inputmode=\"decimal\" {} value=\"{}\" size=\"8\">{}<span class=\"note\">{} {}</span></div><div class=\"row\">{}</div>",
                c("−", "", "ask-step", "data-step=\"-1\""),
                field(""),
                esc(&ask::number_text(a.start_number())),
                c("+", "", "ask-step", "data-step=\"1\""),
                esc(unit),
                esc(&range),
                c("Send", "primary", "ask", "")
            );
        }
        Ask::Text { placeholder } => {
            let p = if placeholder.is_empty() { "Type your answer" } else { placeholder };
            let _ = write!(
                out,
                "<div class=\"row\"><input class=\"field-in grow\" name=\"text\" {} placeholder=\"{}\">{}</div>",
                field(""),
                esc(p),
                c("Send", "primary", "ask", "")
            );
        }
        Ask::Review { title, rows } => {
            if !title.is_empty() {
                let _ = write!(out, "<b>{}</b>", esc(title));
            }
            for (k, v) in rows {
                if live {
                    let _ = write!(
                        out,
                        "<div class=\"field\"><span class=\"k dim\">{}</span><input class=\"field-in v\" name=\"row\" data-key=\"{}\" value=\"{}\" readonly></div>",
                        esc(k),
                        esc(k),
                        esc(v)
                    );
                } else {
                    let _ = write!(out, "<div class=\"field\"><span class=\"k dim\">{}</span><span class=\"v\">{}</span></div>", esc(k), esc(v));
                }
            }
            let _ = write!(
                out,
                "<div class=\"row\">{}{}{}</div>",
                c("Confirm", "primary", "ask", "data-value=\"confirm\""),
                c("Edit", "", "ask-edit", ""),
                c("Reject", "", "ask", "data-value=\"reject\"")
            );
        }
        Ask::Form { title, fields } => {
            if !title.is_empty() {
                let _ = write!(out, "<b>{}</b>", esc(title));
            }
            for (n, f) in fields.iter().enumerate() {
                let _ = write!(out, "<div class=\"ask-field\" data-field=\"{n}\" data-kind=\"{}\"><div class=\"ask-label\">{}</div>", f.ask.name(), esc(&f.label));
                form_field(out, &f.ask, live, cx);
                out.push_str("</div>");
            }
            let _ = write!(out, "<div class=\"row\">{}</div>", c("Submit", "primary", "ask", ""));
        }
    }
    if !live {
        let _ = write!(out, "<div class=\"note read-only\">{}</div>", esc(cx.world.off_reason()));
    }
    out.push_str("</div>");
}

/// The answer, read-only: a tick and its first line, then the rest dim.
fn answer_of(out: &mut String, a: &Ask, answer: Option<&str>) {
    let Some(answer) = answer else {
        out.push_str("<div class=\"note\">answered in words</div>");
        return;
    };
    if let Ask::Review { title, .. } | Ask::Form { title, .. } = a
        && !title.is_empty()
    {
        let _ = write!(out, "<b>{}</b>", esc(title));
    }
    let mut lines = answer.lines();
    let _ = write!(out, "<div class=\"line\"><span class=\"accent\">✓</span>{}</div>", esc(lines.next().unwrap_or("")));
    for l in lines {
        let _ = write!(out, "<div class=\"note\">{}</div>", esc(l));
    }
}

/// One field of a form: its control, which only picks; the form's
/// Submit sends them all.
fn form_field(out: &mut String, a: &Ask, live: bool, cx: &Ctx) {
    let c = |label: &str, class: &str, act: &str, attrs: &str| control(cx, live, label, class, act, attrs);
    let input = |name: &str, extra: &str| {
        let off = if live { String::new() } else { format!(" disabled title=\"{}\"", esc(cx.world.off_reason())) };
        format!("<input class=\"field-in grow\" name=\"{name}\"{off} {extra}>")
    };
    match a {
        Ask::Choice { options } => {
            out.push_str("<div class=\"row\">");
            for o in options {
                out.push_str(&c(o, "outline", "ask-pick", &format!("data-value=\"{}\"", esc(o))));
            }
            out.push_str("</div>");
        }
        Ask::Multi { options } => {
            out.push_str("<div class=\"row\">");
            for o in options {
                out.push_str(&c(&format!("☐ {o}"), "", "ask-toggle", &format!("data-value=\"{}\"", esc(o))));
            }
            out.push_str("</div>");
        }
        Ask::Confirm => {
            let _ = write!(out, "<div class=\"row\">{}{}</div>", c("Yes", "", "ask-pick", "data-value=\"yes\""), c("No", "", "ask-pick", "data-value=\"no\""));
        }
        Ask::Date | Ask::DateTime => {
            let today = cx.world.clock.day();
            out.push_str("<div class=\"row\">");
            for (label, day) in ask::quick_days(today) {
                out.push_str(&c(label, "", "ask-pick", &format!("data-day=\"{day}\"")));
            }
            out.push_str("</div>");
            if live {
                let _ = write!(out, "<input class=\"field-in\" type=\"date\" name=\"day\" value=\"\">");
            }
            if matches!(a, Ask::DateTime) {
                out.push_str("<div class=\"row\">");
                for q in ask::QUICK_TIMES {
                    out.push_str(&c(&ask::time_text(q), "", "ask-pick", &format!("data-minute=\"{q}\"")));
                }
                out.push_str("</div>");
            }
        }
        Ask::Number { min, max, unit } => {
            let range = match (min, max) {
                (Some(lo), Some(hi)) => format!("{} to {}", ask::number_text(*lo), ask::number_text(*hi)),
                (Some(lo), None) => format!("at least {}", ask::number_text(*lo)),
                (None, Some(hi)) => format!("at most {}", ask::number_text(*hi)),
                (None, None) => String::new(),
            };
            let _ = write!(
                out,
                "<div class=\"row\">{}{}{}<span class=\"note\">{} {}</span></div>",
                c("−", "", "ask-step", "data-step=\"-1\""),
                input("number", "inputmode=\"decimal\" size=\"8\" placeholder=\"a number\""),
                c("+", "", "ask-step", "data-step=\"1\""),
                esc(unit),
                esc(&range)
            );
        }
        Ask::Text { placeholder } => {
            let p = if placeholder.is_empty() { "Type your answer" } else { placeholder };
            let _ = write!(out, "<div class=\"row\">{}</div>", input("text", &format!("placeholder=\"{}\"", esc(p))));
        }
        // Never a field (the core reads them as text).
        Ask::Review { .. } | Ask::Form { .. } => {}
    }
}

/// The month of today as a grid, today ringed, the past dim; quick days
/// above it and, for a date and time, the quick times below. Live, a day
/// answers a date; for a date and time a day is picked first, then a time
/// answers (today when no day was picked).
fn date(out: &mut String, with_time: bool, live: bool, cx: &Ctx) {
    let today = cx.world.clock.day();
    let (y, m, _) = civil_from_days(today);
    let day_act = if with_time { "ask-pick" } else { "ask" };
    let c = |label: &str, class: &str, act: &str, attrs: &str| control(cx, live, label, class, act, attrs);
    out.push_str("<div class=\"row\">");
    for (label, day) in ask::quick_days(today) {
        out.push_str(&c(label, "", day_act, &format!("data-day=\"{day}\"")));
    }
    let _ = write!(out, "</div><div class=\"calendar\"><div class=\"cal-title\">{}</div><div class=\"cal-grid\">", esc(&ask::month_title(y, m)));
    for w in ask::WEEKDAYS {
        let _ = write!(out, "<span class=\"wd\">{}</span>", &w[..2]);
    }
    for week in ask::month_grid(y, m) {
        for d in week {
            match d {
                None => out.push_str("<span></span>"),
                Some(d) => {
                    let (_, _, dom) = civil_from_days(d);
                    let class = if d == today {
                        "day today"
                    } else if d < today {
                        "day past"
                    } else {
                        "day"
                    };
                    if live {
                        let _ = write!(out, "<button type=\"button\" class=\"{class}\" data-act=\"{day_act}\" data-day=\"{d}\">{dom}</button>");
                    } else {
                        let _ = write!(out, "<span class=\"{class}\">{dom}</span>");
                    }
                }
            }
        }
    }
    out.push_str("</div></div>");
    if with_time {
        out.push_str("<div class=\"row\">");
        for q in ask::QUICK_TIMES {
            out.push_str(&c(&ask::time_text(q), "", "ask", &format!("data-minute=\"{q}\"")));
        }
        out.push_str("</div>");
    }
    if live {
        let how = if with_time { "Pick a day, then a time." } else { "Pick a day." };
        let _ = write!(out, "<div class=\"note\">{how}</div>");
    } else {
        let _ = write!(out, "<div class=\"row\">{}</div>", cx.off("Pick a day", "primary"));
    }
}
