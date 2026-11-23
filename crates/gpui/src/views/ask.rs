//! The ask components (`dry_core::ask::Ask`), drawn: what a model's reply
//! asks with `reply(ask=...)`, and the `ask` component of a page. Core
//! holds what each one means and the text its answer is sent as; this
//! file only draws it and keeps the picks made so far.
//!
//! An open ask takes input and sends one answer (a click for choice and
//! confirm, Send for the rest). Once the user has answered, the same
//! place shows the answer, read-only, in the history.
//!
//! A form draws each field's control under its label, keyed
//! `<key>/f<n>`; they only pick, and its one Submit reads every field
//! into a `dry_core::ask::Input` for the core to answer (`Ask::submit`).

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use dry_core::ask::{self, Answer, Ask, Field, Input as Picked, State};
use dry_core::live::civil_from_days;
use dry_core::theme::Tokens;
use gpui::{App, Context, Div, Entity, FontWeight, Global, SharedString, Stateful, Window, div, prelude::*, px};
use gpui_component::input::{Input, InputState};

use crate::ui::color;
use crate::{AppView, button};

/// Where an ask's answer goes, and the name its picks are kept under.
#[derive(Clone)]
pub struct Target {
    /// The engine the answer is sent to as the user's message; none on a
    /// page without one (the answer is then only shown).
    pub engine: Option<usize>,
    /// Unique in the window: `<engine id>/<message>` in a chat.
    pub key: String,
}

/// The picks made so far in each open ask, by `Target::key`.
#[derive(Default)]
struct Drafts(HashMap<String, Draft>);

impl Global for Drafts {}

#[derive(Default)]
struct Draft {
    picked: BTreeSet<usize>,
    day: Option<i64>,
    /// The month the calendar shows: `(year, month)`.
    month: Option<(i64, u32)>,
    minute: Option<u32>,
    /// The typed field (text, number).
    field: Option<Entity<InputState>>,
    /// A card's values, while it is being edited.
    rows: Vec<Entity<InputState>>,
    editing: bool,
    error: Option<String>,
    /// The answer, on a page with no engine to send it to.
    sent: Option<String>,
}

fn draft<'a>(cx: &'a mut App, key: &str) -> &'a mut Draft {
    cx.default_global::<Drafts>().0.entry(key.to_string()).or_default()
}

/// This machine's offset from UTC in seconds, asked once (`date +%z`).
fn offset() -> i32 {
    static OFFSET: OnceLock<i32> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        let out = std::process::Command::new("date").arg("+%z").output().ok();
        let z = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let (sign, digits) = match z.split_at_checked(1) {
            Some(("-", d)) => (-1, d),
            Some((_, d)) => (1, d),
            None => return 0,
        };
        let h: i32 = digits.get(..2).and_then(|h| h.parse().ok()).unwrap_or(0);
        let m: i32 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
        sign * (h * 3600 + m * 60)
    })
}

/// Today, as days since 1970-01-01 on this machine's clock.
fn today() -> i64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    dry_core::live::Clock::new(now, offset()).day()
}

/// Send `text` as the answer: to the engine, or kept to show.
fn send(this: &mut AppView, target: &Target, text: String, cx: &mut Context<AppView>) {
    match target.engine {
        Some(i) => {
            cx.default_global::<Drafts>().0.remove(&target.key);
            this.answer(i, &text, cx);
        }
        None => draft(cx, &target.key).sent = Some(text),
    }
    cx.notify();
}

/// A chip: a small button, filled with the accent when `on`.
fn chip(id: String, label: impl Into<SharedString>, on: bool, t: &Tokens) -> Stateful<Div> {
    button(SharedString::from(id), label, t).when(on, |b| b.bg(color(t.accent)).text_color(color(t.on_accent)).border_color(color(t.accent)))
}

/// The primary action: always accent.
fn primary(id: String, label: impl Into<SharedString>, t: &Tokens) -> Stateful<Div> {
    chip(id, label, true, t)
}

fn row(t: &Tokens) -> Div {
    div().flex().flex_wrap().items_center().gap(px(t.space_2))
}

fn dim(text: impl Into<SharedString>, t: &Tokens) -> Div {
    div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(text.into())
}

/// Draw `ask` for `target`; `state` says whether it is still open.
pub fn draw(ask: &Ask, target: Target, state: State<'_>, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>) -> Div {
    let sent = cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).and_then(|d| d.sent.clone());
    let panel = crate::frame::glass(t).w_full().max_w(px(460.)).flex().flex_col().gap(px(t.space_2)).p(px(t.space_3));
    let head = dim(ask.name().to_uppercase(), t).font_weight(FontWeight::SEMIBOLD);
    if let State::Answered(answer) = state {
        return panel.opacity(0.85).child(head).child(answered(ask, answer, t));
    }
    if let Some(sent) = sent {
        return panel.child(head).child(answered(ask, ask::read_answer(&sent).map(|(_, v)| v), t));
    }
    let body = match ask {
        Ask::Choice { options } => choice(options, &target, ask, t, cx),
        Ask::Multi { options } => multi(options, &target, ask, t, cx, true),
        Ask::Confirm => row(t).children([(true, "Yes"), (false, "No")].map(|(yes, label)| {
            let (target, ask) = (target.clone(), ask.clone());
            chip(format!("{}-confirm-{label}", target.key), label, yes, t)
                .on_click(cx.listener(move |this, _, _, cx| send(this, &target, ask.answer(&Answer::Confirm(yes)), cx)))
        })),
        Ask::Date | Ask::DateTime => date(&target, ask, t, cx, true),
        Ask::Number { .. } => number(&target, ask, t, window, cx, true),
        Ask::Text { placeholder } => text(placeholder, &target, ask, t, window, cx, true),
        Ask::Review { title, rows } => card(title, rows, &target, ask, t, window, cx),
        Ask::Form { title, fields } => form(title, fields, &target, ask, t, window, cx),
    };
    let error = cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).and_then(|d| d.error.clone());
    panel.child(head).child(body).when_some(error, |d, e| d.child(div().text_size(px(t.text_sm)).text_color(color(t.error)).child(e)))
}

/// An answered ask: its answer as sent, read-only.
fn answered(ask: &Ask, answer: Option<&str>, t: &Tokens) -> Div {
    let Some(answer) = answer else { return dim("answered in words", t) };
    let mut lines = answer.lines();
    let first = lines.next().unwrap_or("");
    let rest: Vec<String> = lines.map(str::to_string).collect();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .when_some(if let Ask::Review { title, .. } | Ask::Form { title, .. } = ask { Some(title.clone()).filter(|t| !t.is_empty()) } else { None }, |d, title| {
            d.child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        })
        .child(div().flex().gap(px(t.space_1)).child(div().text_color(color(t.text_accent)).child("✓")).child(first.to_string()))
        .children(rest.into_iter().map(|l| dim(l, t)))
}

fn choice(options: &[String], target: &Target, ask: &Ask, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    row(t).children(options.iter().enumerate().map(|(n, o)| {
        let (target, ask, o) = (target.clone(), ask.clone(), o.clone());
        button(SharedString::from(format!("{}-choice-{n}", target.key)), o.clone(), t)
            .border_color(color(t.accent))
            .on_click(cx.listener(move |this, _, _, cx| send(this, &target, ask.answer(&Answer::Choice(o.clone())), cx)))
    }))
}

fn multi(options: &[String], target: &Target, ask: &Ask, t: &Tokens, cx: &mut Context<AppView>, solo: bool) -> Div {
    let picked = cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).map(|d| d.picked.clone()).unwrap_or_default();
    let boxes = options.iter().enumerate().map(|(n, o)| {
        let on = picked.contains(&n);
        let key = target.key.clone();
        chip(format!("{key}-multi-{n}"), format!("{} {o}", if on { "☑" } else { "☐" }), false, t)
            .when(on, |b| b.bg(color(t.accent_muted)).border_color(color(t.accent)))
            .on_click(cx.listener(move |_, _, _, cx| {
                let d = draft(cx, &key);
                if !d.picked.remove(&n) {
                    d.picked.insert(n);
                }
                cx.notify();
            }))
    });
    let (target2, ask2, options) = (target.clone(), ask.clone(), options.to_vec());
    let send_label = format!("Send ({})", picked.len());
    if !solo {
        return row(t).children(boxes);
    }
    div().flex().flex_col().gap(px(t.space_2)).child(row(t).children(boxes)).child(row(t).child(
        primary(format!("{}-multi-send", target.key), send_label, t).on_click(cx.listener(move |this, _, _, cx| {
            let d = draft(cx, &target2.key);
            let chosen = d.picked.iter().filter_map(|n| options.get(*n).cloned()).collect();
            send(this, &target2, ask2.answer(&Answer::Multi(chosen)), cx);
        })),
    ))
}

/// A date: quick picks, then a month as a calendar grid; with a time row
/// for a datetime.
fn date(target: &Target, ask: &Ask, t: &Tokens, cx: &mut Context<AppView>, solo: bool) -> Div {
    let today = today();
    let (day, month, minute) =
        cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).map_or((None, None, None), |d| (d.day, d.month, d.minute));
    let (y, m) = month.unwrap_or_else(|| {
        let (y, m, _) = civil_from_days(day.unwrap_or(today));
        (y, m)
    });
    let key = target.key.clone();
    let pick = move |cx: &mut App, d: i64| {
        let draft = draft(cx, &key);
        draft.day = Some(d);
        let (y, m, _) = civil_from_days(d);
        draft.month = Some((y, m));
    };
    let quick = row(t).children(ask::quick_days(today).into_iter().map(|(label, d)| {
        let pick = pick.clone();
        chip(format!("{}-quick-{label}", target.key), label, day == Some(d), t).on_click(cx.listener(move |_, _, _, cx| {
            pick(cx, d);
            cx.notify();
        }))
    }));
    // ‹ November 2026 ›
    let step = |delta: i32| {
        let key = target.key.clone();
        move |_: &mut AppView, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<AppView>| {
            let total = y * 12 + i64::from(m) - 1 + i64::from(delta);
            draft(cx, &key).month = Some((total.div_euclid(12), (total.rem_euclid(12) + 1) as u32));
            cx.notify();
        }
    };
    let cell = px(34.);
    let nav = div()
        .flex()
        .items_center()
        .gap(px(t.space_2))
        .child(button(SharedString::from(format!("{}-prev", target.key)), "‹", t).on_click(cx.listener(step(-1))))
        .child(div().flex_1().text_center().font_weight(FontWeight::SEMIBOLD).child(ask::month_title(y, m)))
        .child(button(SharedString::from(format!("{}-next", target.key)), "›", t).on_click(cx.listener(step(1))));
    let head = div().flex().children(ask::WEEKDAYS.map(|w| dim(&w[..2], t).w(cell).text_center()));
    let weeks = ask::month_grid(y, m).into_iter().map(|week| {
        div().flex().children(week.into_iter().enumerate().map(|(n, d)| {
            let Some(d) = d else { return div().w(cell).h(px(30.)).into_any_element() };
            let (_, _, dom) = civil_from_days(d);
            let on = day == Some(d);
            let pick = pick.clone();
            div()
                .id(SharedString::from(format!("{}-day-{d}-{n}", target.key)))
                .w(cell)
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(t.radius))
                .cursor_pointer()
                .hover(|s| s.bg(color(t.overlay_hover)))
                .when(d < today, |s| s.text_color(color(t.text_disabled)))
                .when(d == today, |s| s.border_1().border_color(color(t.accent)))
                .when(on, |s| s.bg(color(t.accent)).text_color(color(t.on_accent)))
                .child(dom.to_string())
                .on_click(cx.listener(move |_, _, _, cx| {
                    pick(cx, d);
                    cx.notify();
                }))
                .into_any_element()
        }))
    });
    let calendar = div().flex().flex_col().gap(px(2.)).child(nav).child(head).children(weeks);
    let with_time = matches!(ask, Ask::DateTime);
    let minute = minute.unwrap_or(9 * 60);
    let time = with_time.then(|| {
        let set = |to: Box<dyn Fn(u32) -> u32>| {
            let key = target.key.clone();
            move |_: &mut AppView, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<AppView>| {
                let d = draft(cx, &key);
                d.minute = Some(to(d.minute.unwrap_or(9 * 60)));
                cx.notify();
            }
        };
        row(t)
            .child(button(SharedString::from(format!("{}-m-", target.key)), "−15m", t).on_click(cx.listener(set(Box::new(|m| (m + 1440 - 15) % 1440)))))
            .child(div().w(px(56.)).text_center().font_weight(FontWeight::SEMIBOLD).child(ask::time_text(minute)))
            .child(button(SharedString::from(format!("{}-m+", target.key)), "+15m", t).on_click(cx.listener(set(Box::new(|m| (m + 15) % 1440)))))
            .children(ask::QUICK_TIMES.map(|q| {
                chip(format!("{}-t-{q}", target.key), ask::time_text(q), minute == q, t).on_click(cx.listener(set(Box::new(move |_| q))))
            }))
    });
    let (target2, ask2) = (target.clone(), ask.clone());
    let label = match day {
        Some(d) if with_time => format!("Send {} {}", ask::date_text(d), ask::time_text(minute)),
        Some(d) => format!("Send {}", ask::date_text(d)),
        None => "Pick a day".into(),
    };
    let send_button = primary(format!("{}-date-send", target.key), label, t).when(day.is_none(), |b| b.opacity(0.5)).on_click(cx.listener(
        move |this, _, _, cx| {
            let Some(d) = day else { return };
            let a = if with_time { Answer::DateTime(d, minute) } else { Answer::Date(d) };
            send(this, &target2, ask2.answer(&a), cx);
        },
    ));
    div().flex().flex_col().gap(px(t.space_2)).child(quick).child(calendar).children(time).when(solo, |d| d.child(row(t).child(send_button)))
}

/// The typed field of `key`, made the first time it is drawn.
fn field(key: &str, placeholder: &str, value: &str, window: &mut Window, cx: &mut Context<AppView>) -> Entity<InputState> {
    if let Some(f) = cx.try_global::<Drafts>().and_then(|d| d.0.get(key)).and_then(|d| d.field.clone()) {
        return f;
    }
    let (placeholder, value) = (placeholder.to_string(), value.to_string());
    let f = cx.new(|cx| {
        let mut s = InputState::new(window, cx).placeholder(placeholder);
        s.set_value(value, window, cx);
        s
    });
    draft(cx, key).field = Some(f.clone());
    f
}

fn number(target: &Target, ask: &Ask, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>, solo: bool) -> Div {
    let Ask::Number { min, max, unit } = ask else { return div() };
    // In a form it starts blank: a field can be left out.
    let start = if solo { ask::number_text(ask.start_number()) } else { String::new() };
    let input = field(&target.key, "a number", &start, window, cx);
    let stepper = |by: f64| {
        let (input, ask, key) = (input.clone(), ask.clone(), target.key.clone());
        move |_: &mut AppView, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<AppView>| {
            let now = input.read(cx).value().trim().parse::<f64>().unwrap_or_else(|_| ask.start_number());
            let Ask::Number { min, max, .. } = &ask else { return };
            let mut next = now + by;
            next = min.map_or(next, |lo| next.max(lo));
            next = max.map_or(next, |hi| next.min(hi));
            input.update(cx, |s, cx| s.set_value(ask::number_text(next), window, cx));
            draft(cx, &key).error = None;
            cx.notify();
        }
    };
    let range = match (min, max) {
        (Some(lo), Some(hi)) => format!("{} to {}", ask::number_text(*lo), ask::number_text(*hi)),
        (Some(lo), None) => format!("at least {}", ask::number_text(*lo)),
        (None, Some(hi)) => format!("at most {}", ask::number_text(*hi)),
        (None, None) => String::new(),
    };
    let (target2, ask2, input2) = (target.clone(), ask.clone(), input.clone());
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(button(SharedString::from(format!("{}-minus", target.key)), "−", t).on_click(cx.listener(stepper(-1.))))
                .child(div().w(px(96.)).flex_none().child(Input::new(&input)))
                .child(button(SharedString::from(format!("{}-plus", target.key)), "+", t).on_click(cx.listener(stepper(1.))))
                .when(!unit.is_empty(), |d| d.child(dim(unit.clone(), t)))
                .when(!range.is_empty(), |d| d.child(dim(range, t))),
        )
        .when(solo, |d| d.child(row(t).child(primary(format!("{}-number-send", target.key), "Send", t).on_click(cx.listener(move |this, _, _, cx| {
            let typed = input2.read(cx).value().to_string();
            match ask2.check_number(&typed) {
                Ok(n) => send(this, &target2, ask2.answer(&Answer::Number(n)), cx),
                Err(why) => {
                    draft(cx, &target2.key).error = Some(why);
                    cx.notify();
                }
            }
        })))))
}

#[allow(clippy::too_many_arguments)]
fn text(placeholder: &str, target: &Target, ask: &Ask, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>, solo: bool) -> Div {
    let input = field(&target.key, if placeholder.is_empty() { "Type your answer" } else { placeholder }, "", window, cx);
    if !solo {
        return row(t).child(div().flex_1().min_w(px(160.)).child(Input::new(&input)));
    }
    let (target2, ask2, input2) = (target.clone(), ask.clone(), input.clone());
    row(t).child(div().flex_1().min_w(px(160.)).child(Input::new(&input))).child(
        primary(format!("{}-text-send", target.key), "Send", t).on_click(cx.listener(move |this, _, _, cx| {
            let typed = input2.read(cx).value().trim().to_string();
            if typed.is_empty() {
                draft(cx, &target2.key).error = Some("type an answer first".into());
                return cx.notify();
            }
            send(this, &target2, ask2.answer(&Answer::Text(typed)), cx);
        })),
    )
}

#[allow(clippy::too_many_arguments)]
fn card(title: &str, rows: &[(String, String)], target: &Target, ask: &Ask, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>) -> Div {
    let editing = cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).is_some_and(|d| d.editing);
    // The value fields, made when editing starts.
    if editing && cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).is_none_or(|d| d.rows.is_empty()) {
        let fields: Vec<_> = rows
            .iter()
            .map(|(_, v)| {
                let v = v.clone();
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx);
                    s.set_value(v, window, cx);
                    s
                })
            })
            .collect();
        draft(cx, &target.key).rows = fields;
    }
    let fields = cx.try_global::<Drafts>().and_then(|d| d.0.get(&target.key)).map(|d| d.rows.clone()).unwrap_or_default();
    let lines = rows.iter().enumerate().map(|(n, (k, v))| {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(t.space_2))
            .py(px(2.))
            .border_b_1()
            .border_color(color(t.border))
            .child(dim(k.clone(), t).w(px(110.)).flex_none())
            .child(match fields.get(n).filter(|_| editing) {
                Some(f) => div().flex_1().min_w(px(140.)).child(Input::new(f)),
                None => div().flex_1().min_w(px(140.)).child(v.clone()),
            })
    });
    let confirm = {
        let (target, ask, rows, fields) = (target.clone(), ask.clone(), rows.to_vec(), fields.clone());
        let label = if editing { "Send edits" } else { "Confirm" };
        primary(format!("{}-card-ok", target.key), label, t).on_click(cx.listener(move |this, _, _, cx| {
            let now: Vec<(String, String)> = rows
                .iter()
                .enumerate()
                .map(|(n, (k, v))| (k.clone(), if editing { fields.get(n).map_or(v.clone(), |f| f.read(cx).value().to_string()) } else { v.clone() }))
                .collect();
            send(this, &target, ask.answer(&Answer::Review(Some(now))), cx);
        }))
    };
    let edit = {
        let key = target.key.clone();
        button(SharedString::from(format!("{}-card-edit", target.key)), if editing { "Stop editing" } else { "Edit" }, t).on_click(cx.listener(
            move |_, _, _, cx| {
                let d = draft(cx, &key);
                d.editing = !d.editing;
                cx.notify();
            },
        ))
    };
    let reject = {
        let (target, ask) = (target.clone(), ask.clone());
        button(SharedString::from(format!("{}-card-no", target.key)), "Reject", t)
            .on_click(cx.listener(move |this, _, _, cx| send(this, &target, ask.answer(&Answer::Review(None)), cx)))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(t.text_base + 1.)).child(title.to_string()))
        .child(div().flex().flex_col().children(lines))
        .child(row(t).child(confirm).child(edit).child(reject))
}

/// One pick of `options` in the field `key`: a chip each, the one picked
/// filled.
fn pick(key: &str, options: &[String], t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let picked = cx.try_global::<Drafts>().and_then(|d| d.0.get(key)).map(|d| d.picked.clone()).unwrap_or_default();
    row(t).children(options.iter().enumerate().map(|(n, o)| {
        let key = key.to_string();
        chip(format!("{key}-pick-{n}"), o.clone(), picked.contains(&n), t).on_click(cx.listener(move |_, _, _, cx| {
            let d = draft(cx, &key);
            d.picked = BTreeSet::from([n]);
            cx.notify();
        }))
    }))
}

/// What the field `key` holds now, for the core to read.
fn input_of(key: &str, f: &Field, cx: &App) -> Picked {
    let Some(d) = cx.try_global::<Drafts>().and_then(|d| d.0.get(key)) else { return Picked::default() };
    let names: Vec<String> = match &f.ask {
        Ask::Choice { options } | Ask::Multi { options } => options.clone(),
        Ask::Confirm => vec!["yes".into(), "no".into()],
        _ => Vec::new(),
    };
    Picked {
        picked: d.picked.iter().filter_map(|n| names.get(*n).cloned()).collect(),
        day: d.day,
        minute: d.minute,
        typed: d.field.as_ref().map(|i| i.read(cx).value().to_string()).unwrap_or_default(),
    }
}

/// A form: each field under its label, then one Submit.
#[allow(clippy::too_many_arguments)]
fn form(title: &str, fields: &[Field], target: &Target, ask: &Ask, t: &Tokens, window: &mut Window, cx: &mut Context<AppView>) -> Div {
    let mut items = Vec::new();
    for (n, f) in fields.iter().enumerate() {
        let sub = Target { engine: target.engine, key: format!("{}/f{n}", target.key) };
        let body = match &f.ask {
            Ask::Choice { options } => pick(&sub.key, options, t, cx),
            Ask::Multi { options } => multi(options, &sub, &f.ask, t, cx, false),
            Ask::Confirm => pick(&sub.key, &["Yes".to_string(), "No".to_string()], t, cx),
            Ask::Date | Ask::DateTime => date(&sub, &f.ask, t, cx, false),
            Ask::Number { .. } => number(&sub, &f.ask, t, window, cx, false),
            Ask::Text { placeholder } => text(placeholder, &sub, &f.ask, t, window, cx, false),
            // Never a field (the core reads them as text).
            Ask::Review { .. } | Ask::Form { .. } => div(),
        };
        items.push(
            div()
                .flex()
                .flex_col()
                .gap(px(t.space_1))
                .py(px(t.space_2))
                .border_b_1()
                .border_color(color(t.border))
                .child(dim(f.label.clone(), t).font_weight(FontWeight::SEMIBOLD))
                .child(body),
        );
    }
    let (target2, ask2, fields2) = (target.clone(), ask.clone(), fields.to_vec());
    let submit = primary(format!("{}-form-send", target.key), "Submit", t).on_click(cx.listener(move |this, _, _, cx| {
        let inputs: Vec<Picked> = fields2.iter().enumerate().map(|(n, f)| input_of(&format!("{}/f{n}", target2.key), f, cx)).collect();
        match ask2.submit(&inputs) {
            Ok(text) => {
                for n in 0..fields2.len() {
                    cx.default_global::<Drafts>().0.remove(&format!("{}/f{n}", target2.key));
                }
                send(this, &target2, text, cx)
            }
            Err(why) => {
                draft(cx, &target2.key).error = Some(why);
                cx.notify();
            }
        }
    }));
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .when(!title.is_empty(), |d| d.child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(t.text_base + 1.)).child(title.to_string())))
        .children(items)
        .child(row(t).pt(px(t.space_2)).child(submit))
}
