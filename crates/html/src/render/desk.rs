//! The Desk (`dry_core::desk`) and an engine's session stream, as HTML:
//! `crates/gpui/src/views/desk.rs` in the window. Both read the same core
//! model: the tree, one thread per root, and each engine's turns from its
//! messages, so the Desk and the engine page show the same steps.
//!
//! ```text
//! ┌ tree ─────────┬ thread ──────────────────────────────┐
//! │ ● you         │ Translator's thread · you, translator │
//! │  ● translator │ you → translator 11-23 14:05          │
//! │   ● helper    │   text                                │
//! │   ● tester    │   ● translator running                │
//! │  ● claude-1   │     thought …                         │
//! │               │     ✓ read  name="x"  → 12 lines  ▸   │
//! │ ＋ New thread  ├───────────────────────────────────────┤
//! │ ＋ claude      │ [to ▾] type here…          ↑ Send     │ pinned
//! └───────────────┴───────────────────────────────────────┘
//! ```

use std::fmt::Write;

use dry_core::composer::Action;
use dry_core::desk::{self, Item, Live, Phase, Thread, Turn};
use dry_core::inference::Role;

use super::engine::{Shell, shell};
use super::{Ctx, chip, esc, missing, note, section, tone};

/// Engine `i`'s live status for the desk model.
fn live(cx: &Ctx, i: usize) -> Live {
    let a = cx.activity(i);
    Live { status: cx.status(i), label: a.label(), busy: a.is_turn() }
}

/// A phase's mark and its tone class.
fn mark(p: &Phase) -> (&'static str, &'static str) {
    match p {
        Phase::Queued => ("·", "tone-neutral"),
        Phase::Running => ("◐", "tone-busy"),
        Phase::Waiting => ("⧗", "tone-busy"),
        Phase::Done => ("✓", "tone-good"),
        Phase::Failed(_) => ("✕", "tone-bad"),
    }
}

/// The New thread / New session buttons (the sidebar, the Desk, Settings).
pub fn new_buttons(out: &mut String, cx: &Ctx, class: &str) {
    out.push_str(&cx.act("＋ New thread", class, "data-act=\"new-thread\""));
    if let Ok(s) = &cx.world.sessions {
        for tool in &s.tools {
            out.push_str(&cx.act(&format!("{} New {} session", tool.icon, tool.name), "quiet", &format!("data-act=\"new-session\" data-value=\"{}\"", esc(&tool.name))));
        }
    }
}

/// `desk`: the tree on the left, the selected thread's stream, and the
/// composer pinned at the bottom of the thread's column.
pub fn desk(out: &mut String, cx: &Ctx) {
    let model = desk::build(&cx.world.state, &|i| live(cx, i));
    out.push_str("<div class=\"desk\" data-live=\"desk\"><nav class=\"glass panel desk-tree\" aria-label=\"Engines\">");
    out.push_str(&section("THREADS"));
    let Some((thread, to)) = model.pick(cx.query.thread.as_deref(), cx.query.to.as_deref()) else {
        out.push_str("<div class=\"tree-acts\">");
        new_buttons(out, cx, "primary");
        out.push_str("</div></nav><section class=\"desk-thread\">");
        out.push_str(&note("No thread yet: start one with New thread."));
        out.push_str("</section></div>");
        return;
    };
    for n in &model.tree {
        let selected = match n.index {
            None => false,
            Some(_) => to.as_deref() == Some(n.id.as_str()) && n.thread == thread.id,
        };
        let href = match n.index {
            None => "/desk".to_string(),
            Some(_) => format!("/desk?thread={}&to={}", super::encode(&n.thread), super::encode(&n.id)),
        };
        let _ = write!(
            out,
            "<a class=\"node{}\" href=\"{}\" style=\"--depth:{}\" data-key=\"node:{}\"><span class=\"dot {}\">●</span><span class=\"name trunc\">{}</span><span class=\"note trunc\">{}</span></a>",
            if selected { " here" } else { "" },
            esc(&href),
            n.depth,
            esc(&n.id),
            tone(n.status.tone()),
            esc(&n.name),
            esc(if n.index.is_none() { "" } else { &n.label })
        );
    }
    out.push_str("<div class=\"tree-acts\">");
    new_buttons(out, cx, "quiet");
    out.push_str("</div></nav><section class=\"desk-thread\">");
    head(out, thread);
    out.push_str("<div class=\"stream\" data-stick>");
    if thread.messages.is_empty() {
        out.push_str(&note(match &thread.default_to {
            Some(t) => format!("No posts yet. Write below: it goes to {t}, and its work shows here as it happens."),
            None => "No engines on this thread yet.".to_string(),
        }.as_str()));
    }
    for m in &thread.messages {
        let marker = if m.bounce {
            chip("bounced: no to")
        } else if m.lost {
            chip("not delivered: no addressee")
        } else {
            String::new()
        };
        let _ = write!(
            out,
            "<article class=\"post{}{}\" data-key=\"post:{}:{}\"><div class=\"post-head\"><b>{}</b><span class=\"arrow\">→</span><b>{}</b>{}<span class=\"grow\"></span><span class=\"note\">{}</span></div><div class=\"msg-text\">{}</div>",
            if m.from == desk::YOU { " you" } else { "" },
            if m.bounce || m.lost { " undelivered" } else { "" },
            esc(&thread.id),
            m.n,
            esc(&m.from),
            esc(m.to.as_deref().unwrap_or("nobody")),
            marker,
            esc(&dry_core::shared::when(m.at, cx.world.clock.offset)),
            esc(&m.text)
        );
        if let Some(w) = &m.work {
            work(out, cx, w, &format!("{}:{}", thread.id, m.n));
        }
        out.push_str("</article>");
    }
    out.push_str("</div>");
    super::engine::approvals(out, cx, None);
    composer(out, cx, thread, to.as_deref());
    out.push_str("</section></div>");
}

/// The thread's title and who is on it, each with their status.
fn head(out: &mut String, t: &Thread) {
    let _ = write!(out, "<div class=\"thread-head\"><b>{}</b>{}<span class=\"grow\"></span><span class=\"who-on\">", esc(&t.title), chip(&t.messages.len().to_string()));
    for p in &t.participants {
        let _ = write!(
            out,
            "<span class=\"who{}\" title=\"{}\"><span class=\"dot {}\">●</span>{}</span>",
            if p.head { " head" } else { "" },
            esc(&format!("{}{}", p.label, if p.head { " · the thread's head" } else { "" })),
            tone(p.status.tone()),
            esc(&p.name)
        );
    }
    out.push_str("</span></div>");
}

/// The Desk's composer: a guided post on thread `t`, to the picked engine.
fn composer(out: &mut String, cx: &Ctx, t: &Thread, to: Option<&str>) {
    let targets = desk::targets(t);
    let mut pick = String::from("<label class=\"to-chip pick\"><span class=\"note\">to</span><select name=\"to\" data-keep-select>");
    for p in &targets {
        let _ = write!(pick, "<option value=\"{}\"{}>{}</option>", esc(&p.id), if Some(p.id.as_str()) == to { " selected" } else { "" }, esc(&p.name));
    }
    pick.push_str("</select></label>");
    let target = to.and_then(|t| cx.world.state.index_of(t));
    let busy = target.is_some_and(|i| cx.activity(i).is_turn());
    let line = match (to, busy) {
        (None, _) => "no engine on this thread yet".to_string(),
        (Some(t), true) => format!("{t} is running · it reads this when its turn ends"),
        (Some(t), false) => format!("Enter posts · calls {t}"),
    };
    let id = &t.id;
    shell(
        out,
        cx,
        &[Action::Send],
        &Shell {
            attrs: format!("data-engine=\"thread:{}\" data-thread=\"{}\"", esc(id), esc(id)),
            keep: format!("desk:{id}"),
            placeholder: format!("Message {} on {}", to.unwrap_or("…"), t.title.to_lowercase()),
            to: pick,
            line,
            busy,
            target: format!("thread:{id}"),
        },
    );
}

/// The work under a post: its engine's turn, or why there is none yet.
fn work(out: &mut String, cx: &Ctx, t: &Turn, key: &str) {
    let (m, class) = mark(&t.phase);
    let i = cx.world.state.index_of(&t.engine);
    let label = match (&t.phase, i) {
        (Phase::Queued, _) => "queued · it hears this when it is free".to_string(),
        (Phase::Done, _) if t.start.is_none() => "delivered".to_string(),
        (Phase::Running, Some(i)) => cx.activity(i).label(),
        (Phase::Failed(why), _) => format!("failed: {why}"),
        (p, _) => p.word().to_string(),
    };
    let href = i.map(dry_core::routes::engine_path).unwrap_or_default();
    let _ = write!(
        out,
        "<div class=\"turn phase-{}\" data-key=\"work:{}\"><div class=\"turn-head\"><span class=\"mark {class}\">{m}</span><a href=\"{}\"><b>{}</b></a><span class=\"note trunc\">{}</span></div>",
        t.phase.word(),
        esc(key),
        esc(&href),
        esc(&t.engine),
        esc(&label)
    );
    steps(out, t, &format!("{}:{}", t.engine, t.start.unwrap_or(0)));
    if t.phase == Phase::Running
        && let Some(i) = i
    {
        live_tail(out, cx, i);
    }
    out.push_str("</div>");
}

/// What a running engine is writing now, or its program's live rows.
fn live_tail(out: &mut String, cx: &Ctx, i: usize) {
    if let (Some(v), Some(e)) = (cx.world.reported.sessions.get(&i), cx.world.state.engines.get(i)) {
        super::engine::session_live(out, e, v);
    }
    if let Some(v) = cx.world.reported.streaming.get(&i) {
        super::engine::streaming(out, v, cx);
    }
}

/// A turn's steps: thoughts as text, each call one line that opens to
/// the whole call and result, then the reply.
fn steps(out: &mut String, t: &Turn, key: &str) {
    if !t.items.is_empty() {
        out.push_str("<div class=\"steps\">");
    }
    for (j, item) in t.items.iter().enumerate() {
        match item {
            Item::Thought(text) => {
                let _ = write!(out, "<div class=\"step thought\">{}</div>", esc(text));
            }
            Item::Summary(text) => {
                let _ = write!(
                    out,
                    "<details class=\"step summary\" data-key=\"sum:{}:{j}\"><summary><span class=\"mark tone-neutral\">≡</span><span class=\"note\">earlier steps, summarised</span></summary><div class=\"msg-text small\">{}</div></details>",
                    esc(key),
                    esc(text)
                );
            }
            Item::Text(text) => {
                let _ = write!(out, "<div class=\"step text msg-text\">{}</div>", esc(text));
            }
            Item::Call(c) => {
                let (m, class) = mark(&c.phase);
                let _ = write!(
                    out,
                    "<details class=\"step call phase-{}\" data-key=\"call:{}:{j}\"><summary><span class=\"mark {class}\">{m}</span><code class=\"name\">{}</code><span class=\"args trunc\">{}</span><span class=\"res trunc\">{}</span></summary><pre class=\"tool-out\">{}</pre></details>",
                    c.phase.word(),
                    esc(key),
                    esc(&c.name),
                    esc(&c.args),
                    esc(&c.result),
                    crate::ansi::html(c.full.trim_end())
                );
            }
        }
    }
    if !t.items.is_empty() {
        out.push_str("</div>");
    }
    if let Some(r) = &t.reply {
        let _ = write!(out, "<div class=\"msg-text reply\">{}</div>", esc(r));
    }
}

/// `session`: the route's engine's conversation as one stream: each
/// message you sent (or the posts that called it), then its turn's steps
/// and reply as they happen, the answer being written, asks and approvals.
pub fn session(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    let l = live(cx, i);
    let turns = desk::turns(e, &l);
    let _ = write!(
        out,
        "<div class=\"glass panel chat session\" id=\"chat\"><div class=\"line\"><b>Session</b>{}<span class=\"grow\"></span><span class=\"note\">{}</span></div><div class=\"stream\">",
        chip(&dry_core::inspect::count(turns.iter().filter(|t| t.start.is_some()).count(), "turn", "turns")),
        esc(&l.label)
    );
    let starts: Vec<usize> = turns.iter().map(|t| t.start.unwrap_or(0)).collect();
    for (k, t) in turns.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(e.messages.len());
        let _ = write!(out, "<div class=\"turn phase-{}\" data-key=\"turn:{}\">", t.phase.word(), starts[k]);
        if t.start.is_some() {
            match t.text.strip_prefix(desk::WAKE_HEAD) {
                Some(posts) => {
                    let _ = write!(out, "<div class=\"msg user posts\"><div class=\"who\">from the thread</div><div class=\"body\"><div class=\"msg-text\">{}</div></div></div>", esc(posts.trim()));
                }
                None => {
                    let _ = write!(out, "<div class=\"msg user\"><div class=\"body\"><div class=\"msg-text\">{}</div></div></div>", esc(&t.text));
                }
            }
        }
        let (m, class) = mark(&t.phase);
        if t.phase != Phase::Done {
            let _ = write!(out, "<div class=\"turn-head\"><span class=\"mark {class}\">{m}</span><span class=\"note\">{}</span></div>", esc(&if t.phase == Phase::Running { l.label.clone() } else { t.phase.word().to_string() }));
        }
        steps(out, t, &format!("{}:{}", e.id, starts[k]));
        // A reply that shows or asks brings its component under it.
        for n in starts[k]..end {
            let msg = &e.messages[n];
            if msg.role == Role::Assistant
                && let Some(call) = dry_core::ask::reply_call(e, &msg.text)
            {
                let parsed = dry_core::ask::Parsed::of(&call);
                if let Some(show) = &parsed.show {
                    super::show::draw(out, show);
                }
                if let Some(ask) = &parsed.ask {
                    super::ask::draw(out, ask, dry_core::ask::state(e, n), Some((i, n)), cx);
                }
            }
        }
        if t.phase == Phase::Running && k + 1 == turns.len() {
            live_tail(out, cx, i);
        }
        out.push_str("</div>");
    }
    if turns.is_empty() {
        if cx.world.reported.sessions.contains_key(&i) || cx.world.reported.streaming.contains_key(&i) {
            live_tail(out, cx, i);
        } else {
            out.push_str(&note("No messages yet: type below."));
        }
    }
    out.push_str("</div>");
    super::engine::approvals(out, cx, Some(&e.id));
    out.push_str("</div>");
}
