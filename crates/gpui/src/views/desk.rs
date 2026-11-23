//! The Desk (`dry_core::desk`) and an engine's session stream in the
//! window: `crates/html/src/render/desk.rs` as GPUI. The tree on the
//! left (click opens the node's root's thread with the composer to it),
//! the thread's posts with the work each started, a call per line (click
//! opens it), and the composer at the bottom of the thread's column.

use dry_core::desk::{self, Item, Live, Phase, Turn};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use gpui::{Context, Div, SharedString, div, prelude::*, px};
use gpui_component::input::Textarea;

use super::card::{chip, panel, tone_color};
use super::is_open;
use crate::frame::glass;
use crate::ui::color;
use crate::{AppView, button};

/// Engine `i`'s live status for the desk model.
fn live(app: &AppView, i: usize) -> Live {
    let a = app.activity(i);
    Live { status: app.live_status(i), label: a.label(), busy: a.is_turn() }
}

fn mark(p: &Phase, t: &Tokens) -> (&'static str, u32) {
    match p {
        Phase::Queued => ("·", t.text_secondary),
        Phase::Running => ("◐", t.accent),
        Phase::Waiting => ("⧗", t.accent),
        Phase::Done => ("✓", t.text_accent),
        Phase::Failed(_) => ("✕", t.error),
    }
}

/// The New thread / New session buttons under the tree.
fn new_buttons(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let mut row = div().flex().flex_wrap().gap(px(2.)).mt(px(t.space_2)).child(
        button("desk-new-thread", "＋ New thread", t).on_click(cx.listener(|this, _, window, cx| this.new_thread(window, cx))),
    );
    if let Ok(s) = &app.sessions {
        for tool in &s.tools {
            let name = tool.name.clone();
            row = row.child(
                button(SharedString::from(format!("desk-new-session-{name}")), format!("{} New {name} session", tool.icon), t)
                    .on_click(cx.listener(move |this, _, window, cx| this.new_session(&name, window, cx))),
            );
        }
    }
    row
}

/// `desk`: the tree, the selected thread, its composer.
pub fn desk(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let model = desk::build(&app.state, &|i| live(app, i));
    let (picked_thread, picked_to) = app.desk_pick.clone();
    let Some((thread, to)) = model.pick(Some(picked_thread.as_str()).filter(|s| !s.is_empty()), picked_to.as_deref()) else {
        let tree = panel(t).w(px(220.)).flex_none().gap(px(2.)).child(super::section("THREADS", t)).child(new_buttons(app, t, cx));
        return div().flex().items_start().gap(px(t.space_3)).child(tree).child(super::engine::note("No thread yet: start one with New thread.".to_string(), t));
    };
    let thread = thread.clone();
    let level = thread.id.clone();
    let targets: Vec<(String, String)> = desk::targets(&thread).iter().map(|p| (p.id.clone(), p.name.clone())).collect();

    let tree = model.tree.iter().enumerate().map(|(k, n)| {
        let selected = n.index.is_some() && to.as_deref() == Some(n.id.as_str()) && n.thread == level;
        let pick = (n.thread.clone(), n.index.map(|_| n.id.clone()));
        div()
            .id(SharedString::from(format!("desk-node-{k}")))
            .flex()
            .items_center()
            .gap(px(t.space_2))
            .pl(px(t.space_2 + n.depth as f32 * 14.))
            .pr(px(t.space_2))
            .py(px(4.))
            .rounded(px(t.space_2))
            .text_size(px(t.text_sm))
            .cursor_pointer()
            .when(selected, |d| d.bg(color(t.accent_muted)))
            .hover(|d| d.bg(color(t.overlay_hover)))
            .child(div().flex_none().text_size(px(9.)).text_color(color(tone_color(n.status.tone(), t))).child("●"))
            .child(div().flex_none().truncate().child(n.name.clone()))
            .child(div().flex_1().truncate().text_right().text_color(color(t.text_secondary)).child(if n.index.is_some() { n.label.clone() } else { String::new() }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.desk_pick = pick.clone();
                cx.notify();
            }))
    });
    let tree = panel(t).w(px(220.)).flex_none().gap(px(2.)).child(super::section("THREADS", t)).children(tree).child(new_buttons(app, t, cx));

    let who = thread.participants.iter().map(|p| {
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(t.space_2))
            .rounded(px(t.radius))
            .bg(color(t.background_muted))
            .text_size(px(t.text_sm))
            .text_color(color(if p.head { t.text_primary } else { t.text_secondary }))
            .child(div().text_size(px(9.)).text_color(color(tone_color(p.status.tone(), t))).child("●"))
            .child(p.name.clone())
    });
    let head = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(t.space_2))
        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(thread.title.clone()))
        .child(chip(thread.messages.len().to_string(), t))
        .child(div().flex_1())
        .child(div().flex().flex_wrap().gap(px(t.space_1)).children(who));

    let offset = dry_tools::live::clock().offset;
    let posts = thread.messages.iter().map(|m| {
        let key = format!("{}:{}", thread.id, m.n);
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(t.space_1))
                    .text_size(px(t.text_sm))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(m.from.clone()))
                    .child(div().text_color(color(t.text_disabled)).child("→"))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(m.to.clone().unwrap_or_else(|| "nobody".into())))
                    .when(m.bounce, |d| d.child(chip("bounced: no to", t)))
                    .when(m.lost, |d| d.child(chip("not delivered: no addressee", t)))
                    .child(div().flex_1())
                    .child(div().text_color(color(t.text_secondary)).child(dry_core::shared::when(m.at, offset))),
            )
            .child(div().px(px(t.space_3)).py(px(t.space_2)).rounded(px(t.space_3)).bg(color(t.background_muted)).child(m.text.clone()))
            .children(m.work.as_ref().map(|w| work(app, w, &key, t, cx)))
    });
    let stream = div().flex().flex_col().gap(px(t.space_3)).children(posts).when(thread.messages.is_empty(), |d| {
        d.child(super::engine::note(
            match &thread.default_to {
                Some(to) => format!("No posts yet. Write below: it goes to {to}, and its work shows here as it happens."),
                None => "No engines on this thread yet.".into(),
            },
            t,
        ))
    });

    // The composer: who it goes to (one chip per engine on the thread), the
    // text, Send.
    let picks = targets.into_iter().map(|(id, name)| {
        let on = to.as_deref() == Some(id.as_str());
        let pick = (level.clone(), Some(id.clone()));
        div()
            .id(SharedString::from(format!("desk-to-{id}")))
            .px(px(t.space_2))
            .h(px(24.))
            .flex()
            .items_center()
            .rounded(px(t.radius))
            .text_size(px(t.text_sm))
            .cursor_pointer()
            .bg(color(if on { t.accent_muted } else { t.background_muted }))
            .child(name)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.desk_pick = pick.clone();
                cx.notify();
            }))
    });
    let busy = to.as_deref().and_then(|t| app.state.index_of(t)).is_some_and(|i| app.activity(i).is_turn());
    let line = match (&to, busy) {
        (None, _) => "no engine on this thread yet".to_string(),
        (Some(t), true) => format!("{t} is running · it reads this when its turn ends"),
        (Some(t), false) => format!("Enter posts · calls {t}"),
    };
    let composer = glass(t)
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .p(px(t.space_2))
        .child(Textarea::new(&app.input).appearance(false))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_1))
                .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child("to"))
                .children(picks)
                .child(div().flex_1().truncate().text_size(px(t.text_sm)).text_color(color(if busy { t.accent } else { t.text_secondary })).child(app.composer_note.clone().unwrap_or(line)))
                .child(
                    button("desk-send", "↑ Send", t)
                        .bg(color(t.accent))
                        .text_color(color(t.on_accent))
                        .on_click(cx.listener(|this, _, window, cx| this.desk_send(window, cx))),
                ),
        );
    let main = div().flex_1().min_w_0().flex().flex_col().gap(px(t.space_3)).child(head).child(stream).child(div().flex_1()).child(composer);
    div().flex().items_start().gap(px(t.space_3)).min_h_full().child(tree).child(main)
}

/// The work under a post: its engine's turn, or why there is none yet.
fn work(app: &AppView, w: &Turn, key: &str, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let (m, c) = mark(&w.phase, t);
    let label = match (&w.phase, app.state.index_of(&w.engine)) {
        (Phase::Queued, _) => "queued · it hears this when it is free".to_string(),
        (Phase::Done, _) if w.start.is_none() => "delivered".to_string(),
        (Phase::Running, Some(i)) => app.activity(i).label(),
        (Phase::Failed(why), _) => format!("failed: {why}"),
        (p, _) => p.word().to_string(),
    };
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
                .child(div().text_color(color(c)).child(m))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(w.engine.clone()))
                .child(div().truncate().text_color(color(t.text_secondary)).child(label)),
        )
        .child(steps(app, w, &format!("work:{key}"), t, cx))
}

/// A turn's steps: thoughts, a call per line (click opens the whole call
/// and result), then the reply.
fn steps(app: &AppView, w: &Turn, key: &str, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let items = w.items.iter().enumerate().map(|(j, item)| match item {
        Item::Thought(text) | Item::Text(text) => div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(text.clone()),
        Item::Summary(text) => div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(format!("≡ earlier steps, summarised: {text}")),
        Item::Call(c) => {
            let fold = format!("{key}:call:{j}");
            let open = is_open(app, &fold, false);
            let (m, mc) = mark(&c.phase, t);
            let failed = matches!(c.phase, Phase::Failed(_));
            let line = div()
                .id(SharedString::from(fold.clone()))
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .px(px(t.space_1))
                .rounded(px(t.space_1))
                .text_size(px(t.text_sm))
                .cursor_pointer()
                .hover(|d| d.bg(color(t.overlay_hover)))
                .child(div().flex_none().text_color(color(mc)).child(m))
                .child(div().flex_none().font_family(t.font_mono.clone()).child(c.name.clone()))
                .child(div().flex_shrink_0().truncate().max_w(px(260.)).font_family(t.font_mono.clone()).text_color(color(t.text_secondary)).child(c.args.clone()))
                .child(div().flex_1().truncate().text_color(color(if failed { t.error } else { t.text_secondary })).child(c.result.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let set = &mut this.inspects.get_mut().open;
                    if !set.remove(&fold) {
                        set.insert(fold.clone());
                    }
                    cx.notify();
                }));
            div().flex().flex_col().child(line).when(open, |d| d.child(super::engine::mono(c.full.clone(), t)))
        }
    });
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .when(!w.items.is_empty(), |d| d.pl(px(t.space_3)).border_l_2().border_color(color(if w.phase == Phase::Running { t.accent } else { t.border })))
        .children(items)
        .children(w.reply.clone().map(|r| div().pt(px(t.space_1)).child(r)))
}

/// `session`: the route's engine's conversation as one stream of turns.
pub fn session(app: &AppView, params: &Params, t: &Tokens, window: &mut gpui::Window, cx: &mut Context<AppView>) -> Div {
    let Some(i) = params.index("id").filter(|&i| i < app.state.engines.len()) else { return super::missing(&app.view.location, t) };
    let e = &app.state.engines[i];
    let l = live(app, i);
    let turns = desk::turns(e, &l);
    let count = turns.iter().filter(|w| w.start.is_some()).count();
    let rows = turns.iter().enumerate().map(|(k, w)| {
        let mut d = div().flex().flex_col().gap(px(t.space_1));
        if w.start.is_some() {
            let (text, posts) = match w.text.strip_prefix(desk::WAKE_HEAD) {
                Some(p) => (format!("from the thread:\n{}", p.trim()), true),
                None => (w.text.clone(), false),
            };
            let bubble = div().px(px(t.space_3)).py(px(t.space_2)).rounded(px(t.space_3)).child(text);
            d = d.child(if posts { bubble.border_1().border_color(color(t.border_emphasized)) } else { bubble.bg(color(t.background_muted)) });
        }
        if w.phase != Phase::Done {
            let (m, c) = mark(&w.phase, t);
            d = d.child(div().flex().gap(px(t.space_2)).text_size(px(t.text_sm)).child(div().text_color(color(c)).child(m)).child(div().text_color(color(t.text_secondary)).child(if w.phase == Phase::Running { l.label.clone() } else { w.phase.word().to_string() })));
        }
        d.child(steps(app, w, &format!("{}:{k}", e.id), t, cx))
    });
    let rows: Vec<Div> = rows.collect();
    let streaming = app.streaming.get(&i).map(|v| super::engine::streaming(e, v, t, window, cx));
    panel(t)
        .gap(px(t.space_3))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(t.space_2))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Session"))
                .child(chip(dry_core::inspect::count(count, "turn", "turns"), t))
                .child(div().flex_1())
                .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(l.label.clone())),
        )
        .children(rows)
        .children(streaming)
        .when(turns.is_empty() && !app.streaming.contains_key(&i), |d| d.child(super::engine::note("No messages yet: type below.", t)))
}
