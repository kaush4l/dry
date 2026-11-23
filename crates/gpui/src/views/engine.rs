use dry_core::card::{AsCard, Status};
use dry_core::inference::{Inference, Message, Role};
use dry_core::response::{Format, ResponseFormat};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::card::{chip, gauge, panel};
use super::{fold, is_open, missing, section};
use crate::ui::color;
use crate::{AppView, button};

/// `dashboard`: Home's tiles (`dry_core::dashboard::tiles`), each a small
/// glass card: the label, a big number, a line of detail in the tile's tone.
pub fn dashboard(app: &AppView, t: &Tokens) -> Div {
    let statuses: Vec<Status> = (0..app.state.engines.len()).map(|i| app.live_status(i)).collect();
    let tiles = dry_core::dashboard::tiles(&app.state.engines, &statuses, &app.caps);
    // Under the tiles, every engine doing something: its name and activity
    // on the activity's tint.
    let busy = app.state.engines.iter().enumerate().filter_map(|(i, e)| {
        let a = app.activity(i);
        let tint = a.token().and_then(|n| t.color(n))?;
        Some(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(t.space_1))
                .px(px(t.space_3))
                .py(px(t.space_1))
                .rounded(px(t.radius))
                .border_1()
                .border_color(color(t.border))
                .text_size(px(t.text_sm))
                .child(super::card::wash(tint, t.radius))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(e.name.clone()))
                .child(div().text_color(color(t.text_secondary)).child(a.label())),
        )
    });
    let tiles = div().flex().flex_wrap().gap(px(t.space_3)).children(tiles.into_iter().map(|tile| {
        crate::frame::glass(t)
            .w(px(150.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(t.space_3))
            .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(tile.label))
            .child(div().text_size(px(t.text_lg)).font_weight(gpui::FontWeight::SEMIBOLD).child(tile.value))
            .child(div().text_size(px(t.text_sm)).truncate().text_color(color(super::card::tone_color(tile.tone, t))).child(tile.detail))
    }));
    div().flex().flex_col().gap(px(t.space_2)).child(tiles).child(div().flex().flex_wrap().gap(px(t.space_2)).children(busy))
}

/// `engines`: one card per engine. A model engine and a session are the
/// same object, so they are the same element (`dry_core::card`): its icon
/// (a session's program's icon), kind, facts and status. Click to open.
/// In two groups, as the desk lists them: the model engines with a New
/// button, then the sessions with one for every program in `sessions.json`.
pub fn cards(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let tools = app.sessions.as_ref().ok();
    // One group: the model engines, or the sessions; a line when it is empty.
    let group = |sessions: bool, cx: &mut Context<AppView>| {
        let cards: Vec<_> = app
            .state
            .engines
            .iter()
            .enumerate()
            .filter(|(_, e)| e.session.is_some() == sessions)
            .map(|(i, e)| {
                let mut card = e.card();
                if let Some(tool) = e.session.as_ref().and_then(|s| tools.and_then(|t| t.get(&s.tool))) {
                    card = card.with_icon(tool.icon.clone());
                }
                // Its status, and its activity as a tint over the card.
                let tint = app.activity(i).token().and_then(|n| t.color(n));
                super::card::tinted(&card.with_status(app.live_status(i)), tint, t)
                    .id(SharedString::from(format!("engine-card-{i}")))
                    .cursor_pointer()
                    .hover(|d| d.border_color(color(t.accent)))
                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(&dry_core::routes::engine_path(i), window, cx)))
            })
            .collect();
        if cards.is_empty() {
            let empty = if sessions { "No sessions yet." } else { "No model engines yet." };
            return div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(empty);
        }
        div().flex().flex_wrap().gap(px(t.space_3)).children(cards)
    };
    let engines = group(false, cx);
    let sessions = group(true, cx);
    let new_engine = div().flex().flex_wrap().gap(px(t.space_2)).child(
        button("new-thread", "＋ New thread", t).on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx))),
    );
    let mut new = div().flex().flex_wrap().gap(px(t.space_2));
    match &app.sessions {
        Ok(sessions) => {
            for tool in &sessions.tools {
                let name = tool.name.clone();
                new = new.child(
                    button(SharedString::from(format!("new-session-{name}")), format!("{} New {name} session", tool.icon), t)
                        .on_click(cx.listener(move |this, _, window, cx| this.new_session(&name, window, cx))),
                );
            }
        }
        Err(e) => new = new.child(div().text_size(px(t.text_sm)).text_color(color(t.error)).child(e.clone())),
    }
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(section("ENGINES · models", t))
        .child(engines)
        .child(new_engine)
        .child(section("SESSIONS · programs", t))
        .child(sessions)
        .child(new)
}

/// `tools`: one card per program in `sessions.json`, ready when it is
/// installed (with where), missing when it is not on `PATH`.
pub fn tools(app: &AppView, t: &Tokens) -> Div {
    let body = match &app.sessions {
        Ok(sessions) => div().flex().flex_wrap().gap(px(t.space_3)).children(sessions.tools.iter().map(|tool| {
            let mut card = tool.card();
            let status = match app.programs.get(&tool.name).cloned().flatten() {
                Some(path) => {
                    card.lines.push(path.display().to_string());
                    Status::Ready
                }
                None => Status::Missing(format!("`{}` is not on PATH", tool.start.first().map_or("", String::as_str))),
            };
            super::card::draw(&card.with_status(status), t)
        })),
        Err(e) => div().text_color(color(t.error)).child(e.clone()),
    };
    div().flex().flex_col().gap(px(t.space_2)).child(section("PROGRAMS · sessions.json", t)).child(body)
}

/// A message as shown: a model's answer read with the engine's contract,
/// one `key  value` row per field; if it does not follow the contract, the
/// text as it came and why it did not read. Anything else as it is.
fn text_of(e: &dry_core::state::Engine, m: &Message, t: &Tokens) -> Div {
    if m.role != Role::Assistant || e.response.is_empty() {
        return div().child(m.text.clone());
    }
    match e.format.parse(&e.response, &m.text) {
        // The reply, if it has one, first and plain; then each field, dim.
        Ok(answer) => div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .when_some(dry_core::turn::reply_in(e, &m.text), |d, reply| d.child(div().pb(px(t.space_1)).child(reply)))
            .children(answer.into_iter().map(|(k, v)| {
                div()
                    .flex()
                    .gap(px(t.space_2))
                    .text_size(px(t.text_sm))
                    .text_color(color(t.text_secondary))
                    .child(div().w(px(64.)).flex_none().text_color(color(t.text_disabled)).child(k))
                    .child(div().flex_1().min_w_0().child(v))
            })),
        Err(why) => div()
            .flex()
            .flex_col()
            .child(m.text.clone())
            .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(format!("(did not follow the contract: {why})"))),
    }
}

/// When the exact view was taken, and Refresh (take it again now).
pub(crate) fn taken_line(i: usize, x: &crate::live::Inspect, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(x.taken, |d| d.as_secs() as i64);
    let at = dry_core::live::Clock::new(x.taken, 0);
    div()
        .flex()
        .items_center()
        .gap(px(t.space_2))
        .text_size(px(t.text_sm))
        .text_color(color(t.text_secondary))
        .child(div().flex_1().min_w_0().truncate().child(format!(
            "snapshot taken {at} ({} ago) · {} · read from disk then, not every frame",
            dry_core::inspect::ago(now - x.taken),
            x.when
        )))
        .child(button(SharedString::from(format!("inspect-refresh-{i}")), "Refresh", t).on_click(cx.listener(move |this, _, _, cx| {
            this.take_inspect(i, "refreshed".into(), None);
            cx.notify();
        })))
}

/// Text as it is sent: monospace, on the muted background.
pub(crate) fn mono(text: String, t: &Tokens) -> Div {
    div()
        .p(px(t.space_2))
        .rounded(px(t.space_2))
        .bg(color(t.background_surface))
        .font_family(t.font_mono.clone())
        .text_size(px(t.text_sm))
        .min_w_0()
        .child(if text.is_empty() { "(nothing)".to_string() } else { text })
}

/// The rendered prompt slot by slot (`dry_core::inspect::slots`), from
/// the snapshot last taken: each slot's title and size, open to its text
/// as sent; an empty one with why. Then notes, and Copy (the whole prompt
/// as one text).
fn prompt_slots(app: &AppView, i: usize, e: &dry_core::state::Engine, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some(x) = app.inspect(i) else { return div() };
    let open = app.inspects.borrow().open.clone();
    let text = x.text.clone();
    let slots = x.slots.iter().map(|s| {
        let key = format!("{}/slot:{}", e.id, s.title);
        let is_open = open.contains(&key) && s.empty.is_none();
        let size = match &s.empty {
            Some(why) => format!("empty: {why}"),
            None => format!("≈ {} tokens · {} characters", dry_core::context::short(s.tokens), s.text.chars().count()),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .child(
                div()
                    .id(SharedString::from(format!("slot-{}", s.title)))
                    .flex()
                    .gap(px(t.space_2))
                    .text_size(px(t.text_sm))
                    .when(s.empty.is_none(), |d| d.cursor_pointer())
                    .child(div().flex_none().text_color(color(t.text_secondary)).child(match (&s.empty, is_open) {
                        (Some(_), _) => "·",
                        (None, true) => "▾",
                        (None, false) => "▸",
                    }))
                    .child(div().w(px(96.)).flex_none().text_color(color(if s.empty.is_some() { t.text_secondary } else { t.text_accent })).child(s.title.clone()))
                    .child(div().flex_1().min_w_0().text_color(color(t.text_secondary)).child(size))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let open = &mut this.inspects.get_mut().open;
                        if !open.remove(&key) {
                            open.insert(key.clone());
                        }
                        cx.notify();
                    })),
            )
            .when(is_open, |d| d.child(mono(s.text.clone(), t)))
    }).collect::<Vec<_>>();
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .child(taken_line(i, &x, t, cx))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().flex_1().min_w_0().text_size(px(t.text_sm)).child(x.size.clone()))
                .child(button(SharedString::from(format!("prompt-copy-{i}")), "Copy as text", t).on_click(cx.listener(move |_, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                }))),
        )
        .children(slots)
        .children(x.notes.iter().map(|(what, note)| div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(format!("{what}: {note}"))))
}

/// The Response view's context lines, `(text, is_a_warning)`: the prompt's
/// size against the model's window (as last sent, else estimated now with
/// what is known of the window), where the window came from, when the
/// history was compressed, and why a prompt over the threshold was sent
/// as it is.
fn context_lines(app: &AppView, i: usize, e: &dry_core::state::Engine, rendered: &dry_core::prompt::Rendered) -> Vec<(String, bool)> {
    use dry_core::context;
    let key = format!("{} {}", e.inference.base_url, e.inference.model);
    let window = app.windows.get(&key).cloned().unwrap_or_else(|| context::Window::of(&e.inference, None));
    let known = e.inference.context.is_some() || app.windows.contains_key(&key);
    let now = context::measure(rendered, &window);
    let mut lines = vec![match app.inspect(i) {
        Some(x) => (format!("snapshot: {} (with the file and terminal snapshots, as sent)", x.size), false),
        None => (format!("now: {} (shown without the live snapshots)", now.line()), false),
    }];
    match app.engine_context.get(&i) {
        Some(c) => lines.push((format!("last call: {}", c.budget.line()), false)),
        None => lines.push(("last call: none yet".into(), false)),
    }
    lines.push((
        format!(
            "window: {}{}; compresses at {:.0}%, keeping the newest messages and your message verbatim",
            window.label(),
            if known { "" } else { " until the server is asked at the first call" },
            context::COMPRESS_AT * 100.0
        ),
        false,
    ));
    let done = context::compressions(e);
    if !done.is_empty() {
        let (covered, size) = done[done.len() - 1];
        lines.push((
            format!("compressed {} time(s); the last summary covers {covered} message(s) in ≈ {} tokens (### summary in convo/messages.md)", done.len(), context::short(size)),
            false,
        ));
    }
    if let Some(c) = app.engine_context.get(&i) {
        if let Some(note) = &c.compressed {
            lines.push((format!("last compression: {note}"), note.starts_with("not compressed")));
        }
        if let Some(why) = &c.over {
            lines.push((format!("over {:.0}%: {why}", context::COMPRESS_AT * 100.0), true));
        }
    }
    lines
}

/// The engine of the route, if there is one: `(index, engine)`.
fn engine_of<'a>(app: &'a AppView, params: &Params) -> Option<(usize, &'a dry_core::state::Engine)> {
    params.index("id").and_then(|i| app.state.engines.get(i).map(|e| (i, e)))
}

/// A quiet line of small grey text.
pub(crate) fn note(text: impl Into<SharedString>, t: &Tokens) -> Div {
    div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(text.into())
}

/// `engine-header`: the top of an engine's page, one card on its
/// activity's tint (`dry_core::activity`): its name and kind, what it is
/// doing, its model, its goal in a line, and its prompt against the
/// context window (at the last call; before one, the snapshot's estimate).
pub fn header(app: &AppView, params: &Params, t: &Tokens) -> Div {
    let Some((i, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    let activity = app.activity(i);
    let status = app.live_status(i);
    let (ratio, size) = match (app.engine_context.get(&i), e.session.is_none()) {
        (Some(c), true) => (Some(c.budget.ratio()), Some(c.budget.line())),
        (None, true) => app.inspect(i).map_or((None, None), |x| (Some(x.ratio), Some(format!("{} · estimate, no call yet", x.size)))),
        (_, false) => (None, None),
    };
    let model = match &e.session {
        Some(s) => format!("{} session · {}", s.tool, s.id.as_deref().unwrap_or("no id yet (starts on the first text)")),
        None => e.inference.describe(),
    };
    let doing = match activity {
        dry_core::activity::Activity::Idle => status.word().to_string(),
        ref a => a.label(),
    };
    let over = ratio.is_some_and(|r| r >= dry_core::context::COMPRESS_AT);
    panel(t)
        .when_some(activity.token().and_then(|n| t.color(n)), |d, tint| d.child(super::card::wash(tint, t.pane_radius)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().flex_none().text_color(color(super::card::tone_color(status.tone(), t))).child("●"))
                .child(div().min_w_0().truncate().font_weight(gpui::FontWeight::SEMIBOLD).child(e.name.clone()))
                .child(chip(e.kind().to_string(), t))
                .child(div().flex_1())
                .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(doing)),
        )
        .child(note(model, t).truncate())
        .when(!e.description.is_empty(), |d| d.child(div().text_size(px(t.text_sm)).child(dry_core::goal::summary(&e.description))))
        .when_some(ratio.zip(size), |d, (ratio, size)| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t.space_3))
                    .child(div().w(px(120.)).flex_none().child(gauge(ratio, if over { t.error } else { t.accent }, t)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(t.text_sm))
                            .text_color(color(if over { t.error } else { t.text_secondary }))
                            .child(size),
                    ),
            )
        })
}

/// `/desk/:id`: the conversation, one message after another: who said
/// it, then what. Your texts sit on the muted fill; the model's replies
/// are read with the engine's contract; a reply that asks
/// (`reply(ask=...)`) brings its component under it, open until you
/// answer, then read-only. The text area is its own component.
pub fn chat(app: &AppView, params: &Params, t: &Tokens, window: &mut gpui::Window, cx: &mut Context<AppView>) -> Div {
    let Some((i, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    let mut asks = std::collections::HashMap::new();
    for (n, m) in e.messages.iter().enumerate().filter(|(_, m)| m.role == Role::Assistant) {
        let Some(call) = dry_core::ask::reply_call(e, &m.text) else { continue };
        let parsed = dry_core::ask::Parsed::of(&call);
        // What the reply shows, then what it asks.
        let mut parts = Vec::new();
        if let Some(show) = &parsed.show {
            parts.push(super::show::draw(show, t));
        }
        if let Some(ask) = &parsed.ask {
            let target = super::ask::Target { engine: Some(i), key: format!("{}/{n}", e.id) };
            parts.push(super::ask::draw(ask, target, dry_core::ask::state(e, n), t, window, cx));
        }
        if !parts.is_empty() {
            asks.insert(n, div().flex().flex_col().gap(px(t.space_2)).children(parts));
        }
    }
    // The answer streaming in now, as far as it is written (`dry_core::stream`).
    let streaming = app.streaming.get(&i).map(|v| streaming(e, v, t, window, cx));
    let texts = e.messages.iter().enumerate().map(|(n, m)| {
        let (who, who_color) = match m.role {
            Role::Assistant => ("Model", t.text_accent),
            Role::Tool => ("Tools", t.text_secondary),
            Role::Summary => ("Summary", t.text_accent),
            _ => ("You", t.text_secondary),
        };
        let mine = !matches!(m.role, Role::Assistant | Role::Tool | Role::Summary);
        let body = div().min_w_0().flex().flex_col().gap(px(t.space_2)).child(text_of(e, m, t)).children(asks.remove(&n));
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t.space_2))
                    .text_size(px(t.text_sm))
                    .child(div().flex_1().text_color(color(who_color)).child(who))
                    .child(
                        div()
                            .id(SharedString::from(format!("text-del-{n}")))
                            .flex_none()
                            .px(px(t.space_1))
                            .cursor_pointer()
                            .text_color(color(t.text_disabled))
                            .hover(|d| d.text_color(color(t.error)))
                            .child("✕")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.remove_text(i, n);
                                cx.notify();
                            })),
                    ),
            )
            .child(if mine {
                body.px(px(t.space_3)).py(px(t.space_2)).rounded(px(t.space_3)).bg(color(t.background_muted))
            } else if m.role == Role::Assistant {
                body
            } else {
                body.text_size(px(t.text_sm)).text_color(color(t.text_secondary))
            })
    });
    panel(t)
        .gap(px(t.space_3))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Conversation"))
                .child(chip(dry_core::inspect::count(e.messages.len(), "message", "messages"), t)),
        )
        .children(texts)
        .children(streaming)
        .when(e.messages.is_empty() && !app.streaming.contains_key(&i), |d| d.child(note("No messages yet: type below.", t)))
        // Waiting for a reply, or why the last one failed.
        .when_some(app.engine_status.get(&i).filter(|_| !app.activity(i).is_turn()), |d, status| d.child(super::card::status_line(status, t)))
}

/// The answer as the model writes it: the think fold's size, the thought,
/// the reply's text growing, and its show and ask as soon as their kind is
/// written (drawn, not yet answerable: no engine to send to).
pub(crate) fn streaming(e: &dry_core::state::Engine, v: &dry_core::stream::View, t: &Tokens, window: &mut gpui::Window, cx: &mut Context<AppView>) -> Div {
    let mut body = div().min_w_0().flex().flex_col().gap(px(t.space_2));
    if !v.think.trim().is_empty() {
        body = body.child(note(format!("thinking… ({} chars)", v.think.trim().chars().count()), t));
    }
    let reply = v.reply.as_ref();
    match reply {
        Some(r) if !r.text.is_empty() => body = body.child(div().child(format!("{}▍", r.text))),
        _ if !v.prose.trim().is_empty() => body = body.child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(v.prose.trim().to_string())),
        _ => body = body.child(div().text_color(color(t.accent)).child("▍")),
    }
    if let Some(r) = reply {
        if let Some(show) = &r.show {
            body = body.child(super::show::draw(show, t));
        }
        if let Some(ask) = &r.ask {
            let target = super::ask::Target { engine: None, key: format!("{}/streaming", e.id) };
            body = body.child(super::ask::draw(ask, target, dry_core::ask::State::Open, t, window, cx));
        }
    }
    if !v.calls.is_empty() {
        body = body.child(note(format!("calls: {}", v.calls.join(", ")), t));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .child(div().text_size(px(t.text_sm)).text_color(color(t.text_accent)).child("Model · writing"))
        .child(body)
}

/// `engine-tools`: what the route's engine may call now, grouped by where
/// each tool comes from (`dry_core::inspect::tool_groups`): built-in, an
/// artifact, the phase, a skill, an MCP server. Each with its arguments
/// and its description. Closed by default; the count says how many.
pub fn tools_list(app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some((_, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    let key = format!("{}/section:tools", e.id);
    if e.session.is_some() {
        let open = is_open(app, &key, false);
        return fold(key, open, "Tools", None, t, cx).when(open, |d| d.child(note(format!("{} brings its own tools.", e.kind()), t)));
    }
    let groups = dry_core::inspect::tool_groups(e);
    let total: usize = groups.iter().map(|g| g.tools.len()).sum();
    let open = is_open(app, &key, false);
    let body = open.then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_3))
            .when(e.response.is_empty(), |d| {
                d.child(note("Not sent now: with no response contract the answer is free text.", t).text_color(color(t.error)))
            })
            .children(groups.iter().map(|g| tool_group(g, t)))
    });
    fold(key, open, "Tools", Some(total.to_string()), t, cx).children(body)
}

/// One source's tools: its name and count, then each tool's call, what it
/// does, and its arguments.
pub(crate) fn tool_group(g: &dry_core::inspect::ToolGroup, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .text_size(px(t.text_sm))
                .child(div().text_color(color(t.text_secondary)).child(g.source.label().to_uppercase()))
                .child(chip(g.tools.len().to_string(), t)),
        )
        .children(g.tools.iter().map(|tool| {
            let args: Vec<String> = tool.args.iter().map(|(a, d)| if d.is_empty() { a.clone() } else { format!("{a}: {d}") }).collect();
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .font_family(t.font_mono.clone())
                        .text_size(px(t.text_sm))
                        .text_color(color(t.text_accent))
                        .truncate()
                        .child(tool.call.clone()),
                )
                .child(div().text_size(px(t.text_sm)).child(tool.description.clone()))
                .when(!args.is_empty(), |d| d.child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(args.join(" · "))))
        }))
}

/// `artifacts`: the route's engine's artifacts, only when it has any. One
/// row each: its kind's glyph, its name and kind, its state in a few words
/// (`dry_core::inspect`), how many tools it brings, and Open: its own page.
pub fn artifacts(app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some((i, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    if e.artifacts.is_empty() {
        return div();
    }
    let Some(x) = app.inspect(i) else { return div() };
    let key = format!("{}/section:artifacts", e.id);
    let open = is_open(app, &key, true);
    let rows = open.then(|| {
        x.artifacts
            .iter()
            .enumerate()
            .map(|(n, v)| {
                let path = format!("{}/artifacts/{}", dry_core::routes::engine_path(i), v.name);
                div()
                    .id(SharedString::from(format!("artifact-{}", v.name)))
                    .flex()
                    .items_center()
                    .gap(px(t.space_2))
                    .py(px(t.space_2))
                    .when(n > 0, |d| d.border_t_1().border_color(color(t.border)))
                    .cursor_pointer()
                    .child(div().w(px(t.space_4 + t.space_2)).flex_none().text_color(color(t.text_accent)).child(dry_core::inspect::icon(v.kind)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(t.space_2))
                                    .child(div().min_w_0().truncate().font_weight(gpui::FontWeight::MEDIUM).child(v.name.clone()))
                                    .child(chip(v.kind.map_or("text", |k| k.as_str()), t)),
                            )
                            .child(note(v.summary.clone(), t).truncate())
                            .when(!v.tools.is_empty(), |d| {
                                d.child(note(dry_core::inspect::count(v.tools.len(), "tool", "tools"), t).text_color(color(t.text_disabled)))
                            }),
                    )
                    .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_accent)).child("Open ›"))
                    .hover(|d| d.bg(color(t.overlay_hover)))
                    .on_click(cx.listener(move |this, _, window, cx| this.navigate(&path, window, cx)))
            })
            .collect::<Vec<_>>()
    });
    // Its developer artifacts in one code editor's layout, first.
    let developer = dry_core::liveview::developer(e);
    let live = (open && !developer.is_empty()).then(|| {
        let to = dry_core::liveview::path(i, None);
        let names: Vec<String> = developer.iter().map(|(n, k)| format!("{n} · {}", k.as_str())).collect();
        div()
            .id("artifacts-live")
            .flex()
            .items_center()
            .gap(px(t.space_2))
            .py(px(t.space_2))
            .cursor_pointer()
            .child(div().w(px(t.space_4 + t.space_2)).flex_none().text_color(color(t.text_accent)).child("⌨"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(gpui::FontWeight::MEDIUM).child("Live view"))
                    .child(note(format!("files, terminal and this chat in one editor: {}", names.join(", ")), t).truncate()),
            )
            .child(div().flex_none().text_size(px(t.text_sm)).text_color(color(t.text_accent)).child("Open ›"))
            .hover(|d| d.bg(color(t.overlay_hover)))
            .on_click(cx.listener(move |this, _, window, cx| this.navigate(&to, window, cx)))
    });
    fold(key, open, "Artifacts", Some(x.artifacts.len().to_string()), t, cx).children(live).children(rows.into_iter().flatten())
}

/// `prompt`: the prompt as rendered, behind a fold: when the view was
/// taken (Refresh), its size and Copy as text, slot by slot with sizes or
/// why a slot is empty, then the context lines.
pub fn prompt(app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some((i, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    if e.session.is_some() {
        return div();
    }
    let key = format!("{}/section:prompt", e.id);
    let open = is_open(app, &key, false);
    let size = app.inspect(i).map(|x| format!("≈ {} tokens", dry_core::context::short(x.slots.iter().map(|s| s.tokens).sum())));
    let body = open.then(|| {
        let rendered = dry_core::prompt::render(e);
        let context = context_lines(app, i, e, &rendered);
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_2))
            .child(prompt_slots(app, i, e, t, cx))
            .child(section("CONTEXT", t))
            .children(context.into_iter().map(|(line, warn)| note(line, t).when(warn, |d| d.text_color(color(t.error)))))
    });
    fold(key, open, "Prompt", size, t, cx).children(body)
}

/// `response`: the route's engine's response contract (each key and its
/// description), the format its answers are written in (switch with a
/// click, the contract stays), Reload `responses.md`, Reload spec. Closed
/// by default.
pub fn response(app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some((i, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    if e.session.is_some() {
        return div();
    }
    let key = format!("{}/section:response", e.id);
    let open = is_open(app, &key, false);
    let count = format!("{} · {}", dry_core::inspect::count(e.response.fields.len(), "field", "fields"), e.format.name());
    let body = open.then(|| {
        let formats = ["toon", "toon=", "json"].map(|name| {
            let on = e.format.name() == name;
            button(SharedString::from(format!("format-{name}")), name, t)
                .when(on, |b| b.bg(color(t.accent)).text_color(color(t.on_accent)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(f) = Format::parse_name(name) {
                        this.set_format(i, f);
                    }
                    cx.notify();
                }))
        });
        let fields = e.response.fields.iter().map(|f| {
            div()
                .flex()
                .gap(px(t.space_3))
                .text_size(px(t.text_sm))
                .child(div().w(px(72.)).flex_none().text_color(color(t.text_accent)).child(f.key.clone()))
                .child(div().flex_1().min_w_0().text_color(color(t.text_secondary)).child(format!(
                    "{}{}",
                    f.description,
                    if f.optional { " (optional)" } else { "" }
                )))
        });
        let mcp = if !e.mcp_tools.is_empty() {
            ""
        } else if e.mcp.trim().is_empty() {
            " · no mcp.json"
        } else {
            " · MCP not started"
        };
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_2))
            .child(div().flex().flex_wrap().gap(px(t.space_2)).children(formats))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(t.space_2))
                    .child(button("response-reload", "Reload responses.md", t).on_click(cx.listener(move |this, _, _, cx| {
                        this.load_response(i);
                        cx.notify();
                    })))
                    .child(button("spec-reload", "Reload spec", t).on_click(cx.listener(move |this, _, _, cx| {
                        this.reload_spec(i, cx);
                        cx.notify();
                    }))),
            )
            .child(note(format!("spec: engines/{}/spec{mcp}", e.id), t))
            .children(fields)
            .when(e.response.is_empty(), |d| d.child(note("No contract: answers are free text. Reload to use responses.md.", t)))
    });
    fold(key, open, "Response", Some(count), t, cx).children(body)
}

/// `metadata`: everything the route's engine knows about itself, kept
/// quiet behind a fold: its kind, session or model, and for a session
/// every value its program reported on the last turn. Delete engine is
/// here, out of the way.
pub fn metadata(app: &AppView, params: &Params, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let Some((i, e)) = engine_of(app, params) else { return missing(&app.view.location, t) };
    let mut rows: Vec<(String, String)> = vec![("kind".into(), e.kind().to_string()), ("id".into(), e.id.clone())];
    match &e.session {
        Some(s) => rows.push(("session.id".into(), s.id.clone().unwrap_or_else(|| "(none yet)".into()))),
        None => rows.push(("inference".into(), e.inference.describe())),
    }
    rows.extend(e.meta.iter().map(|(k, v)| (k.clone(), v.clone())));
    let key = format!("{}/section:metadata", e.id);
    let open = is_open(app, &key, false);
    let n = rows.len();
    let body = open.then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .text_size(px(t.text_sm))
            .children(rows.into_iter().map(|(k, v)| {
                div()
                    .flex()
                    .gap(px(t.space_2))
                    .child(div().w(px(96.)).flex_none().truncate().text_color(color(t.text_disabled)).child(k))
                    .child(div().flex_1().min_w_0().text_color(color(t.text_secondary)).child(v))
            }))
            .when(e.session.is_some() && e.meta.is_empty(), |d| d.child(note("Nothing yet: send a text and the program's report shows here.", t)))
            .child(div().pt(px(t.space_2)).flex().child(button("engine-del", "Delete engine", t).text_color(color(t.error)).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.state.remove_engine(i);
                    // Statuses are kept by index, which just shifted.
                    this.forget_engine_states();
                    this.navigate(dry_core::routes::HOME, window, cx);
                },
            ))))
    });
    fold(key, open, "Metadata", Some(n.to_string()), t, cx).opacity(0.85).children(body)
}
