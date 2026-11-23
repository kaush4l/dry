//! An engine's page and its artifacts: `crates/gpui/src/views/engine.rs`
//! and `artifact.rs`, as HTML. Each reads the same core models: the
//! activity (`dry_core::activity`), the exact view (`dry_core::inspect`,
//! taken per request, `Ctx::snap`), the context budget (`dry_core::context`).

use std::fmt::Write;

use dry_core::composer::{self, Action, Capabilities};
use dry_core::inference::{Inference, Message, Role};
use dry_core::inspect::{self, ChildView, Detail, FileDetail, OpenFile, PostView, SectionFill, SessionView, Source};
use dry_core::response::ResponseFormat;
use dry_core::state::Engine;

use super::{Ctx, READ_ONLY, SIDE_WIDTH, chip, columns_html, esc, fold, gauge, missing, mono, note, section, tint, tone};

/// `engine-header`: one card on the activity's tint: the status dot, name,
/// kind, what it is doing, its model, its goal in a line, and its prompt
/// against the context window (an estimate: no call runs here).
pub fn header(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    let activity = cx.activity(i);
    let status = cx.status(i);
    let doing = match activity {
        dry_core::activity::Activity::Idle => status.word().to_string(),
        ref a => a.label(),
    };
    let model = match &e.session {
        Some(s) => format!("{} session · {}", s.tool, s.id.as_deref().unwrap_or("no id yet (starts on the first text)")),
        None => e.inference.describe(),
    };
    let _ = write!(
        out,
        "<div class=\"glass panel tinted\" id=\"engine-header\"{}><div class=\"line\"><span class=\"dot {}\">●</span><b class=\"grow-not trunc\">{}</b>{}<span class=\"grow\"></span><span class=\"note\">{}</span></div>{}",
        tint(&activity),
        tone(status.tone()),
        esc(&e.name),
        chip(e.kind()),
        esc(&doing),
        note(&model).replace("class=\"note\"", "class=\"note trunc\""),
    );
    if !e.description.is_empty() {
        let _ = write!(out, "<div class=\"small\">{}</div>", esc(&dry_core::goal::summary(&e.description)));
    }
    if e.session.is_none()
        && let Some(x) = cx.snap(i)
    {
        // The last call's measure when this page ran one, else the snapshot's.
        let (ratio, size, how) = match cx.world.reported.measured.get(&i) {
            Some(m) => (m.budget.ratio(), m.budget.line(), "at the last call"),
            None => (x.ratio, x.size.clone(), "estimate, no call yet"),
        };
        let over = ratio >= dry_core::context::COMPRESS_AT;
        let _ = write!(
            out,
            "<div class=\"line\" data-live=\"context\"><div style=\"width:120px;flex:none\">{}</div><span class=\"note trunc{}\">{} · {how}</span></div>",
            gauge(ratio, over),
            if over { " bad" } else { "" },
            esc(&size)
        );
    }
    out.push_str("</div>");
}

/// A message as shown: a model's answer read with the engine's contract,
/// one `key  value` row per field; if it does not read, the text as it
/// came and why. Anything else as it is.
fn text_of(out: &mut String, e: &Engine, m: &Message) {
    // Tool results are a program's words: monospace, its colours kept
    // (a terminal's output carries ANSI codes), long ones in a box that scrolls.
    if m.role == Role::Tool {
        let _ = write!(out, "<pre class=\"tool-out\">{}</pre>", crate::ansi::html(m.text.trim_end()));
        return;
    }
    if m.role != Role::Assistant || e.response.is_empty() {
        let _ = write!(out, "<div class=\"msg-text\">{}</div>", esc(&m.text));
        return;
    }
    match e.format.parse(&e.response, &m.text) {
        Ok(answer) => {
            if let Some(reply) = dry_core::turn::reply_in(e, &m.text) {
                let _ = write!(out, "<div class=\"msg-text reply\">{}</div>", esc(&reply));
            }
            for (k, v) in answer {
                let _ = write!(out, "<div class=\"field\"><span class=\"k\">{}</span><span class=\"v\">{}</span></div>", esc(&k), esc(&v));
            }
        }
        Err(why) => {
            let _ = write!(out, "<div class=\"msg-text\">{}</div>{}", esc(&m.text), note(&format!("(did not follow the contract: {why})")));
        }
    }
}

/// `chat`: the conversation, one message after another: who said it, then
/// what. Your texts on the muted fill; a reply that asks brings its ask
/// component under it, open until answered, then read-only.
pub fn chat(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    let _ = write!(
        out,
        "<div class=\"glass panel chat\" id=\"chat\"><div class=\"line\"><b>Conversation</b>{}</div>",
        chip(&inspect::count(e.messages.len(), "message", "messages"))
    );
    for (n, m) in e.messages.iter().enumerate() {
        let (who, class) = match m.role {
            Role::Assistant => ("Model", "assistant"),
            Role::Tool => ("Tools", "tool"),
            Role::Summary => ("Summary", "summary"),
            _ => ("You", "user"),
        };
        let _ = write!(out, "<div class=\"msg {class}\"><div class=\"who\">{who}</div><div class=\"body\">");
        text_of(out, e, m);
        if m.role == Role::Assistant
            && let Some(call) = dry_core::ask::reply_call(e, &m.text)
        {
            let parsed = dry_core::ask::Parsed::of(&call);
            if let Some(show) = &parsed.show {
                super::show::draw(out, show);
            }
            if let Some(ask) = &parsed.ask {
                super::ask::draw(out, ask, dry_core::ask::state(e, n), Some((i, n)), cx);
            }
        }
        out.push_str("</div></div>");
    }
    if let Some(v) = cx.world.reported.sessions.get(&i) {
        session_live(out, e, v);
    }
    if let Some(v) = cx.world.reported.streaming.get(&i) {
        streaming(out, v, cx);
    } else if e.messages.is_empty() && !cx.world.reported.sessions.contains_key(&i) {
        out.push_str(&note("No messages yet: type below."));
    }
    approvals(out, cx, Some(&e.id));
    out.push_str("</div>");
}

/// A session's run as far as it has gone (`dry_core::stream_json`): each
/// tool call a row (running, its first result line, or its error), an
/// agent's calls under it, the agents with how they are doing.
pub(super) fn session_live(out: &mut String, e: &Engine, v: &dry_core::stream_json::Live) {
    use dry_core::stream_json::RowState;
    let _ = write!(
        out,
        "<div class=\"msg tool streaming\" data-session-live=\"{}\"><div class=\"who\">Tools · {} · live</div><div class=\"body\">",
        v.rows.len(),
        esc(e.kind())
    );
    if v.rows.is_empty() {
        out.push_str(&note(if v.init.is_some() { "started; no tool calls yet" } else { "starting the program…" }));
    }
    for r in &v.rows {
        let (class, result) = match &r.state {
            RowState::Running => ("running", "running…".to_string()),
            RowState::Done(l) => ("done", l.clone()),
            RowState::Failed(l) => ("failed", format!("error: {l}")),
        };
        let sub = if r.parent.is_some() { " sub" } else { "" };
        let _ = write!(
            out,
            "<div class=\"tool-row {class}{sub}\" data-tool=\"{}\"><code>{}</code> <span class=\"note\">→ {}</span></div>",
            esc(&r.name),
            esc(&r.call),
            esc(&result)
        );
    }
    for a in &v.agents {
        let how = a.done.clone().unwrap_or_else(|| "running".into());
        let _ = write!(
            out,
            "<div class=\"tool-row agent\"><b>{}</b> <span class=\"note\">{} · {}: {}</span></div>",
            esc(&a.name),
            esc(&a.kind),
            esc(&a.description),
            esc(&how)
        );
    }
    let texts: Vec<&str> = v.texts.iter().map(String::as_str).collect();
    if let Some(t) = texts.last() {
        let _ = write!(out, "<div class=\"msg-text\">{}</div>", esc(t));
    }
    out.push_str("</div></div>");
}

/// The approvals waiting for a person (`dry_core::approval`): `engine`'s,
/// or every engine's. Allow and Deny answer them through the host.
pub fn approvals(out: &mut String, cx: &Ctx, engine: Option<&str>) {
    let list: Vec<&dry_core::approval::Approval> =
        cx.world.reported.approvals.iter().filter(|a| engine.is_none_or(|e| a.engine == e)).collect();
    if list.is_empty() {
        return;
    }
    let _ = write!(
        out,
        "<div class=\"glass panel approvals\" data-live=\"approvals\"><div class=\"line\"><b>Approvals</b>{}<span class=\"grow\"></span><span class=\"note\">waiting for you</span></div>",
        chip(&list.len().to_string())
    );
    for a in list {
        let wait = a.timeout.map(|t| format!(" · denied after {t}s")).unwrap_or_default();
        let _ = write!(
            out,
            "<div class=\"approval line\" data-approval=\"{}\"><span class=\"grow trunc\">{} · <b>{}</b> asks to use <code>{}</code><span class=\"note\">{wait}</span></span>{}{}</div>",
            esc(&a.id),
            esc(&a.id),
            esc(&a.engine),
            esc(&a.what()),
            cx.act("Allow", "primary", &format!("data-act=\"approve\" data-id=\"{}\" data-value=\"allow\"", esc(&a.id))),
            cx.act("Deny", "", &format!("data-act=\"approve\" data-id=\"{}\" data-value=\"deny\"", esc(&a.id))),
        );
    }
    out.push_str("</div>");
}

/// The answer as far as the model has written it (`dry_core::stream::View`):
/// its think fold, the thought, the reply's text growing, and its show
/// and ask as soon as their kind is written (not answerable until saved).
pub(super) fn streaming(out: &mut String, v: &dry_core::stream::View, cx: &Ctx) {
    out.push_str(
        "<div class=\"msg assistant streaming\" data-streaming=\"1\"><div class=\"who\">Model · writing</div><div class=\"body\">",
    );
    if !v.think.trim().is_empty() {
        let _ = write!(
            out,
            "<details class=\"think\" data-key=\"{}/think\"><summary>thinking</summary><div class=\"msg-text\">{}</div></details>",
            esc(&cx.engine().map(|(_, e)| e.id.clone()).unwrap_or_default()),
            esc(v.think.trim())
        );
    }
    let reply = v.reply.as_ref();
    match reply {
        Some(r) if !r.text.is_empty() => {
            let _ = write!(out, "<div class=\"msg-text reply\">{}<span class=\"caret\">▍</span></div>", esc(&r.text));
        }
        _ if !v.prose.trim().is_empty() => {
            let _ =
                write!(out, "<div class=\"field\"><span class=\"k\">thought</span><span class=\"v\">{}</span></div>", esc(v.prose.trim()));
        }
        _ => out.push_str("<div class=\"msg-text reply\"><span class=\"caret\">▍</span></div>"),
    }
    if let Some(r) = reply {
        if let Some(show) = &r.show {
            super::show::draw(out, show);
        }
        if let Some(ask) = &r.ask {
            super::ask::draw(out, ask, dry_core::ask::State::Open, None, cx);
        }
    }
    if !v.calls.is_empty() {
        out.push_str(&note(&format!("calls: {}", v.calls.join(", "))));
    }
    out.push_str("</div></div>");
}

/// `composer`: the glass capsule the app types into, drawn as in the
/// window (its buttons from `dry_core::composer::buttons`), pinned to the
/// bottom of the content column. Read-only: disabled, with the one line
/// that says why. Live: Enter or Send sends to the route's engine (on a
/// page without one, to a new engine).
pub fn composer(out: &mut String, actions: &[Action], cx: &Ctx) {
    let engine = cx.engine();
    let target = engine.map_or_else(|| "new".to_string(), |(i, _)| i.to_string());
    let busy = engine.is_some_and(|(i, _)| cx.activity(i).is_turn());
    // A session's `/` list: what its program said at its start (live, or
    // kept from its last turn).
    let slash: Vec<String> = engine
        .map(|(i, e)| match cx.world.reported.sessions.get(&i).map(|v| v.slash()).filter(|l| !l.is_empty()) {
            Some(l) => l,
            None => dry_core::stream_json::slash_of(&e.meta),
        })
        .unwrap_or_default();
    let mut attrs = format!("data-engine=\"{target}\"");
    if !slash.is_empty() {
        let _ = write!(attrs, " data-slash=\"{}\"", esc(&slash.join(" ")));
    }
    let to = match engine {
        Some((i, e)) => format!(
            "<span class=\"to-chip\" title=\"the turn runs on {}\"><span class=\"dot {}\">●</span>{}</span>",
            esc(&e.name),
            tone(cx.status(i).tone()),
            esc(&e.name)
        ),
        None => "<span class=\"to-chip\">new engine</span>".to_string(),
    };
    let line = match (busy, cx.world.hosted()) {
        (true, true) => "running · send when it ends",
        (true, false) => "running · your message waits for it",
        (false, _) if !slash.is_empty() => "Enter sends · Shift+Enter new line · / commands",
        (false, _) => "Enter sends · Shift+Enter new line",
    };
    let placeholder = match engine {
        Some((_, e)) => format!("Message {}", e.name),
        None => "Ask to start a new engine".to_string(),
    };
    shell(out, cx, actions, &Shell { attrs, keep: format!("composer:{target}"), placeholder, to, line: line.into(), busy, target });
}

/// What one composer says: its form attributes, the key its text is kept
/// under, its placeholder, the target chip (or picker), its status line.
pub(super) struct Shell {
    pub attrs: String,
    pub keep: String,
    pub placeholder: String,
    pub to: String,
    pub line: String,
    pub busy: bool,
    /// The `data-engine` its buttons carry.
    pub target: String,
}

/// The composer's markup: one auto-growing line to type in, then a bar:
/// who it goes to, the quiet buttons, the status inline, Send.
pub(super) fn shell(out: &mut String, cx: &Ctx, actions: &[Action], s: &Shell) {
    // Voice is the `transcribe` capability where this runtime has it (the
    // browser's Web Speech), else off with why.
    let mut caps = Capabilities::default();
    if cx.world.state.capabilities.0.present("transcribe") {
        caps.set(dry_core::composer::Capability::Voice, dry_core::card::Status::Ready);
    }
    let live = cx.world.live();
    let busy = if s.busy { " busy" } else { "" };
    if live {
        let _ = write!(
            out,
            "<form class=\"glass composer pinned{busy}\" data-composer {}><textarea name=\"text\" rows=\"1\" placeholder=\"{}\" data-keep=\"{}\"></textarea><div class=\"composer-bar\">{}",
            s.attrs,
            esc(&s.placeholder),
            esc(&s.keep),
            s.to
        );
    } else {
        let _ = write!(
            out,
            "<form class=\"glass composer pinned\" onsubmit=\"return false\"><textarea disabled rows=\"1\" placeholder=\"{}\" title=\"{}\"></textarea><div class=\"composer-bar\">{}",
            esc(&s.placeholder),
            esc(READ_ONLY),
            s.to
        );
    }
    let mut send = String::new();
    for b in composer::buttons(actions, &caps, s.target != "new") {
        let act = match b.action {
            Action::Send => "send",
            Action::Clear => "clear",
            Action::Voice => "voice",
        };
        let (label, class) = if b.action == Action::Send {
            (format!("{} {}", b.action.icon(), b.action.label()), "primary")
        } else {
            (b.action.icon().to_string(), "quiet icon")
        };
        let html = if !b.enabled {
            format!(
                "<button class=\"btn {class}\" disabled title=\"{}: {}\">{}</button>",
                esc(b.action.label()),
                esc(b.why.as_deref().unwrap_or("")),
                esc(&label)
            )
        } else {
            cx.act(&label, class, &format!("data-act=\"{act}\" data-engine=\"{}\" title=\"{}\"", esc(&s.target), esc(b.action.label())))
        };
        if b.action == Action::Send {
            send = html;
        } else {
            out.push_str(&html);
        }
    }
    if live {
        let _ = write!(out, "<span class=\"note grow trunc\" data-composer-note>{}</span>{send}</div></form>", esc(&s.line));
    } else {
        let _ = write!(out, "<span class=\"note grow trunc read-only\">Read-only</span>{send}</div></form>");
    }
}

/// `artifacts`: the route's engine's artifacts, only when it has any. One
/// row each: glyph, name and kind, its state in a few words, how many
/// tools it brings, and Open: its own page.
pub fn artifacts(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    if e.artifacts.is_empty() {
        return;
    }
    let Some(x) = cx.snap(i) else { return };
    let key = format!("{}/section:artifacts", e.id);
    fold(out, &key, true, "Artifacts", Some(&x.artifacts.len().to_string()), "artifacts", |out| {
        // The live view first, when the engine holds a file or terminal artifact (docs/developer-loop.md, "The Code page is the human view of the developer artifacts").
        if !dry_core::liveview::developer(e).is_empty() {
            let _ = write!(
                out,
                "<a class=\"artifact-row\" href=\"{}\"><span class=\"icon\">▦</span><div class=\"grow col\"><div class=\"line\"><b class=\"trunc\">Live view</b></div><span class=\"note trunc\">explorer, editor and terminal: what the engine sees, live</span></div><span class=\"open\">Open ›</span></a>",
                dry_core::liveview::path(i, None)
            );
        }
        for v in &x.artifacts {
            let path = format!("{}/artifacts/{}", dry_core::routes::engine_path(i), super::encode(&v.name));
            let _ = write!(
                out,
                "<a class=\"artifact-row\" href=\"{}\"><span class=\"icon\">{}</span><div class=\"grow col\"><div class=\"line\"><b class=\"trunc\">{}</b>{}</div>{}{}</div><span class=\"open\">Open ›</span></a>",
                esc(&path),
                inspect::icon(v.kind),
                esc(&v.name),
                chip(v.kind.map_or("text", |k| k.as_str())),
                note(&v.summary).replace("class=\"note\"", "class=\"note trunc\""),
                if v.tools.is_empty() {
                    String::new()
                } else {
                    format!("<div class=\"note dim\">{}</div>", inspect::count(v.tools.len(), "tool", "tools"))
                },
            );
        }
    });
}

/// One source's tools: its name and count, then each tool's call, what it
/// does, and its arguments.
fn tool_group(out: &mut String, g: &inspect::ToolGroup) {
    let _ = write!(
        out,
        "<div class=\"tool-group\"><div class=\"line small\"><span class=\"dim2\">{}</span>{}</div>",
        esc(&g.source.label().to_uppercase()),
        chip(&g.tools.len().to_string())
    );
    for tool in &g.tools {
        let args: Vec<String> = tool.args.iter().map(|(a, d)| if d.is_empty() { a.clone() } else { format!("{a}: {d}") }).collect();
        let _ = write!(
            out,
            "<div class=\"tool\"><code class=\"call\">{}</code><div class=\"small\">{}</div>",
            esc(&tool.call),
            esc(&tool.description)
        );
        if !args.is_empty() {
            out.push_str(&note(&args.join(" · ")));
        }
        out.push_str("</div>");
    }
    out.push_str("</div>");
}

/// `engine-tools`: what the engine may call now, grouped by where each
/// tool comes from. Closed by default; the count says how many.
pub fn tools_list(out: &mut String, cx: &Ctx) {
    let Some((_, e)) = cx.engine() else { return missing(out, cx.location) };
    let key = format!("{}/section:tools", e.id);
    if e.session.is_some() {
        let kind = e.kind().to_string();
        return fold(out, &key, false, "Tools", None, "", |out| out.push_str(&note(&format!("{kind} brings its own tools."))));
    }
    let groups = inspect::tool_groups(e);
    let total: usize = groups.iter().map(|g| g.tools.len()).sum();
    fold(out, &key, false, "Tools", Some(&total.to_string()), "", |out| {
        if e.response.is_empty() {
            out.push_str("<div class=\"note bad\">Not sent now: with no response contract the answer is free text.</div>");
        }
        for g in &groups {
            tool_group(out, g);
        }
    });
}

/// `prompt`: the prompt as rendered, behind a fold: when it was taken,
/// its size, slot by slot (each its own fold), notes, then the context lines.
pub fn prompt(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    if e.session.is_some() {
        return;
    }
    let Some(x) = cx.snap(i) else { return };
    let key = format!("{}/section:prompt", e.id);
    let size = format!("≈ {} tokens", dry_core::context::short(x.slots.iter().map(|s| s.tokens).sum()));
    fold(out, &key, false, "Prompt", Some(&size), "", |out| {
        out.push_str(&taken_line(x.taken, cx));
        let _ = write!(out, "<div class=\"small\">{}</div>", esc(&x.size));
        for s in &x.slots {
            match &s.empty {
                Some(why) => {
                    let _ = write!(
                        out,
                        "<div class=\"slot empty\"><span class=\"chev-dot\">·</span><span class=\"slot-title\">{}</span><span class=\"note\">empty: {}</span></div>",
                        esc(&s.title),
                        esc(why)
                    );
                }
                None => {
                    let _ = write!(
                        out,
                        "<details class=\"slot\" data-key=\"{}/slot:{}\"><summary><span class=\"chev\"></span><span class=\"slot-title\">{}</span><span class=\"note\">≈ {} tokens · {} characters</span></summary>{}</details>",
                        esc(&e.id),
                        esc(&s.title),
                        esc(&s.title),
                        dry_core::context::short(s.tokens),
                        s.text.chars().count(),
                        mono(&s.text)
                    );
                }
            }
        }
        for (what, n) in &x.notes {
            out.push_str(&note(&format!("{what}: {n}")));
        }
        out.push_str(&section("CONTEXT"));
        let window = cx
            .world
            .reported
            .measured
            .get(&i)
            .map_or_else(|| dry_core::context::Window::of(&e.inference, None), |m| m.budget.window.clone());
        let known = e.inference.context.is_some() || cx.world.reported.measured.contains_key(&i);
        out.push_str(&note(&format!("snapshot: {} (with the file and terminal snapshots, as sent)", x.size)));
        match cx.world.reported.measured.get(&i) {
            Some(m) => {
                out.push_str(&note(&format!("last call: {}", m.budget.line())));
                if let Some(why) = &m.over {
                    out.push_str(&note(why));
                }
                if let Some(c) = &m.compressed {
                    out.push_str(&note(c));
                }
            }
            None if cx.world.hosted() => out.push_str(&note("last call: none yet since the host started")),
            None if cx.world.live() => out.push_str(&note("last call: none yet in this page")),
            None => out.push_str(&note("last call: none seen here (the desktop app runs the calls)")),
        }
        out.push_str(&note(&format!(
            "window: {}{}; compresses at {:.0}%, keeping the newest messages and your message verbatim",
            window.label(),
            if known { "" } else { " until the server is asked at the first call" },
            dry_core::context::COMPRESS_AT * 100.0
        )));
        let done = dry_core::context::compressions(e);
        if let Some((covered, size)) = done.last() {
            out.push_str(&note(&format!(
                "compressed {} time(s); the last summary covers {covered} message(s) in ≈ {} tokens (### summary in convo/messages.md)",
                done.len(),
                dry_core::context::short(*size)
            )));
        }
    });
}

/// When the exact view was taken: on this request.
fn taken_line(taken: i64, cx: &Ctx) -> String {
    let at = dry_core::live::Clock::new(taken, cx.world.clock.offset);
    let how = if cx.world.live() && !cx.world.hosted() {
        "taken for this page, again after every change"
    } else {
        "read from disk for this page, again on every live update"
    };
    note(&format!("snapshot taken {at} · {how}"))
}

/// `response`: the contract (each key and its description) and the
/// format its answers are written in. Switching and reloading are the
/// desktop app's for now.
pub fn response(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    if e.session.is_some() {
        return;
    }
    let key = format!("{}/section:response", e.id);
    let count = format!("{} · {}", inspect::count(e.response.fields.len(), "field", "fields"), e.format.name());
    fold(out, &key, false, "Response", Some(&count), "", |out| {
        out.push_str("<div class=\"row\">");
        for name in ["toon", "toon=", "json"] {
            out.push_str(&cx.act(
                name,
                if e.format.name() == name { "primary" } else { "" },
                &format!("data-act=\"set-format\" data-engine=\"{i}\" data-value=\"{}\"", esc(name)),
            ));
        }
        let _ = write!(
            out,
            "</div><div class=\"row\">{}{}</div>",
            cx.act("Reload responses.md", "", &format!("data-act=\"reload-responses\" data-engine=\"{i}\"")),
            cx.act("Reload spec", "", &format!("data-act=\"reload-spec\" data-engine=\"{i}\""))
        );
        let mcp = if !e.mcp_tools.is_empty() {
            ""
        } else if e.mcp.trim().is_empty() {
            " · no mcp.json"
        } else {
            " · MCP not started"
        };
        out.push_str(&note(&format!("spec: engines/{}/spec{mcp}", e.id)));
        for f in &e.response.fields {
            let _ = write!(
                out,
                "<div class=\"field\"><span class=\"k accent\">{}</span><span class=\"v\">{}{}</span></div>",
                esc(&f.key),
                esc(&f.description),
                if f.optional { " (optional)" } else { "" }
            );
        }
        if e.response.is_empty() {
            out.push_str(&note("No contract: answers are free text. Reload to use responses.md."));
        }
    });
}

/// `metadata`: everything the engine knows about itself, quiet behind a fold.
pub fn metadata(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    let mut rows: Vec<(String, String)> = vec![("kind".into(), e.kind().to_string()), ("id".into(), e.id.clone())];
    match &e.session {
        Some(s) => rows.push(("session.id".into(), s.id.clone().unwrap_or_else(|| "(none yet)".into()))),
        None => rows.push(("inference".into(), e.inference.describe())),
    }
    rows.extend(e.meta.iter().map(|(k, v)| (k.clone(), v.clone())));
    let key = format!("{}/section:metadata", e.id);
    fold(out, &key, false, "Metadata", Some(&rows.len().to_string()), "quiet", |out| {
        for (k, v) in &rows {
            let _ =
                write!(out, "<div class=\"field\"><span class=\"k dim trunc\">{}</span><span class=\"v\">{}</span></div>", esc(k), esc(v));
        }
        if e.session.is_some() && e.meta.is_empty() {
            out.push_str(&note("Nothing yet: send a text and the program's report shows here."));
        }
        let _ = write!(
            out,
            "<div class=\"row\">{}</div>",
            cx.act("Delete engine", "danger", &format!("data-act=\"delete-engine\" data-engine=\"{i}\""))
        );
    });
}

// ---- an artifact's own page (`views/artifact.rs`) ----

/// `artifact-page`: head (glyph, name, kind, meaning, state), the body by
/// kind, its tools and state rows on the side, and its snapshot as sent.
pub fn artifact_page(out: &mut String, cx: &Ctx) {
    let Some((i, e)) = cx.engine() else { return missing(out, cx.location) };
    let name = cx.params.get("name").unwrap_or_default().to_string();
    let Some(x) = cx.snap(i) else { return missing(out, cx.location) };
    let Some(v) = x.artifacts.iter().find(|v| v.name == name) else {
        let _ = write!(
            out,
            "<div class=\"glass panel\"><b>No artifact \"{}\"</b>{}</div>",
            esc(&name),
            note(&format!("{} holds {}.", e.name, inspect::count(x.artifacts.len(), "artifact", "artifacts")))
        );
        return;
    };
    let _ = write!(
        out,
        "<div class=\"glass panel\"><div class=\"line\"><span class=\"icon accent\">{}</span><b class=\"trunc\">{}</b>{}<span class=\"grow\"></span><span class=\"note\">{}</span></div>{}",
        inspect::icon(v.kind),
        esc(&v.name),
        chip(v.kind.map_or("text", |k| k.as_str())),
        esc(&v.summary),
        note(&v.meaning)
    );
    if let Some(why) = &v.not_sent {
        let _ = write!(out, "<div class=\"note bad\">not sent now: {}</div>", esc(why));
    }
    out.push_str(&taken_line(x.taken, cx));
    out.push_str("</div>");

    let mut main = String::new();
    body(&mut main, &v.detail, cx, &e.id, &v.name);
    let key = format!("{}/{}/snapshot", e.id, v.name);
    let size = format!("≈ {} tokens", dry_core::context::short(dry_core::context::tokens(&v.snapshot)));
    fold(&mut main, &key, true, "Snapshot · as sent", Some(&size), "", |out| {
        // As sent, but a terminal's colour codes drawn rather than shown raw.
        if crate::ansi::has_codes(&v.snapshot) {
            let _ = write!(out, "<pre class=\"mono\">{}</pre>", crate::ansi::html(&v.snapshot));
        } else {
            out.push_str(&mono(&v.snapshot));
        }
    });

    let mut side = String::new();
    let groups: Vec<_> = inspect::tool_groups(e).into_iter().filter(|g| v.kind.is_some_and(|k| g.source == Source::Artifact(k))).collect();
    let _ = write!(
        side,
        "<div class=\"glass panel\"><div class=\"line\"><b>Tools</b>{}</div>",
        chip(&groups.iter().map(|g| g.tools.len()).sum::<usize>().to_string())
    );
    if groups.is_empty() {
        side.push_str(&note(match v.kind {
            None => "None of its own: a plain artifact is read and written with read(...) and write(...).",
            Some(_) => "None: this kind brings no tools.",
        }));
    }
    for g in &groups {
        tool_group(&mut side, g);
    }
    side.push_str("</div><div class=\"glass panel\"><b>State</b>");
    for (k, val) in &v.state {
        let _ =
            write!(side, "<div class=\"field\"><span class=\"k dim trunc\">{}</span><span class=\"v\">{}</span></div>", esc(k), esc(val));
    }
    side.push_str("</div>");
    columns_html(out, &main, &side, SIDE_WIDTH - 40.0);
}

/// The artifact's state, laid out by its kind.
fn body(out: &mut String, detail: &Detail, cx: &Ctx, engine: &str, artifact: &str) {
    match detail {
        Detail::Text(text) => out.push_str(&mono(text)),
        Detail::Broken(why) => {
            let _ = write!(out, "<div class=\"glass panel\"><div class=\"note bad\">Its state does not read: {}</div></div>", esc(why));
        }
        Detail::File(f) => file(out, f),
        Detail::Terminal { root, sessions } => terminal(out, root, sessions),
        Detail::Shared(posts) => shared(out, posts.as_deref()),
        Detail::Learning { sections, collecting } => learning(out, sections, collecting),
        Detail::Engines(children) => engines(out, children.as_deref(), cx),
        Detail::Evidence(ev) => evidence(out, ev, cx.world.evidence_files.then(|| evidence_url(engine, artifact)).as_deref()),
    }
}

/// Where the server serves an evidence artifact's files
/// (`GET /api/engines/{id}/evidence/{artifact}/{file}`).
pub fn evidence_url(engine: &str, artifact: &str) -> String {
    format!("/api/engines/{}/evidence/{}/", url_part(engine), url_part(artifact))
}

fn url_part(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// An evidence artifact: the verdict, then each shot; a changed one shows
/// before, after and the diff side by side, with its page-text lines.
/// `files`: where its images are served (none: their paths are shown).
fn evidence(out: &mut String, ev: &inspect::EvidenceDetail, files: Option<&str>) {
    use dry_core::evidence::report::Status;
    let tone = if ev.line.ends_with("pass") {
        "tone-good"
    } else if ev.line.ends_with("fail") {
        "tone-bad"
    } else {
        ""
    };
    let _ = write!(
        out,
        "<div class=\"glass panel\"><div class=\"line\"><b>Evidence</b><span class=\"small {tone}\">{}</span></div>",
        esc(&ev.line)
    );
    let when = ev.commit.as_ref().map_or(String::new(), |(c, _)| format!(" · commit {c}"));
    out.push_str(&note(&format!("Runs `{}` in {}{when}", ev.command, ev.root)));
    out.push_str("</div>");
    let img = |out: &mut String, label: &str, path: &str| {
        let file = path.rsplit('/').next().unwrap_or(path);
        let _ = match files {
            Some(base) => write!(
                out,
                "<figure style=\"margin:0;flex:1 1 220px;min-width:0\"><figcaption class=\"note\">{l}</figcaption><a href=\"{base}{f}\"><img src=\"{base}{f}\" alt=\"{l} {f}\" loading=\"lazy\" style=\"width:100%;height:auto;display:block;border-radius:6px\"></a></figure>",
                l = esc(label),
                f = esc(file)
            ),
            None => write!(
                out,
                "<div class=\"note\" style=\"flex:1 1 220px\">{}: <span class=\"mono-name\">{}</span></div>",
                esc(label),
                esc(path)
            ),
        };
    };
    for s in &ev.shots {
        let class = match s.status {
            Status::Changed | Status::Flaky => "tone-bad",
            Status::Same => "tone-good",
            _ => "tone-busy",
        };
        let pct = if s.status == Status::New { String::new() } else { format!("{:.2}%", s.changed) };
        let _ = write!(
            out,
            "<div class=\"glass panel\"><div class=\"line\"><b class=\"mono-name trunc grow\">{}</b><span class=\"small {class}\">● {}</span><span class=\"note\">{pct}</span></div><div style=\"display:flex;gap:10px;flex-wrap:wrap\">",
            esc(&s.name),
            s.status.word()
        );
        match (&s.after, &s.diff) {
            (Some(after), diff) => {
                img(out, "before", &s.png);
                img(out, "after", after);
                if let Some(d) = diff {
                    img(out, "diff", d);
                }
            }
            (None, Some(d)) => {
                img(out, "now", &s.png);
                img(out, "diff", d);
            }
            (None, None) => img(out, "now", &s.png),
        }
        out.push_str("</div>");
        if !s.text.is_empty() {
            let _ = write!(out, "<pre class=\"mono code-out\">{}</pre>", esc(&s.text.join("\n")));
        }
        out.push_str("</div>");
    }
}

/// A file artifact: each open file with line numbers, its focus lit and
/// its unsaved lines marked; the names under its root on the side.
fn file(out: &mut String, f: &FileDetail) {
    let mut open = String::new();
    if f.open.is_empty() {
        open.push_str(&format!(
            "<div class=\"glass panel\">{}</div>",
            note("No files open: the engine opens one with open(path=\"...\").")
        ));
    }
    for o in &f.open {
        open_file(&mut open, o);
    }
    let mut list = String::new();
    let count = f.files.as_ref().map_or(String::new(), |n| n.len().to_string());
    let _ = write!(
        list,
        "<div class=\"glass panel\"><div class=\"line\"><b>Files</b>{}<span class=\"note trunc\">root {}</span></div>",
        if count.is_empty() { String::new() } else { chip(&count) },
        esc(&f.root)
    );
    match &f.files {
        Err(why) => list.push_str(&note(why)),
        Ok(names) => {
            list.push_str("<div class=\"file-list\">");
            for (path, is_open) in names {
                let _ = write!(
                    list,
                    "<div class=\"{}\"><span class=\"mark\">{}</span>{}</div>",
                    if *is_open { "on" } else { "" },
                    if *is_open { "●" } else { "" },
                    esc(path)
                );
            }
            list.push_str("</div>");
        }
    }
    list.push_str("</div>");
    columns_html(out, &open, &list, 240.0);
}

/// Most lines of an open file drawn at once (around its focus).
const SHOWN_LINES: usize = 400;

fn open_file(out: &mut String, o: &OpenFile) {
    let total = o.lines.len();
    let start = o.focus.map_or(0, |(a, _)| a.saturating_sub(11)).min(total.saturating_sub(SHOWN_LINES.min(total)));
    let end = (start + SHOWN_LINES).min(total);
    let _ = write!(
        out,
        "<div class=\"glass panel\"><div class=\"line\"><b class=\"mono-name trunc\">{}</b><span class=\"grow\"></span>",
        esc(&o.path)
    );
    if let Some((a, b)) = o.focus {
        out.push_str(&chip(&format!("focus {a}-{b}")));
    }
    if o.unsaved > 0 {
        out.push_str(&chip(&inspect::count(o.unsaved, "unsaved edit", "unsaved edits")));
    }
    let _ = write!(out, "{}</div>", chip(&inspect::count(total, "line", "lines")));
    if let Some(n) = &o.note {
        out.push_str(&note(n));
    }
    if start > 0 || end < total {
        out.push_str(&note(&format!("lines {}-{end} of {total} shown", start + 1)));
    }
    out.push_str("<div class=\"code\">");
    for l in &o.lines[start..end] {
        let mut class = String::from("ln");
        if o.focus.is_some() {
            class.push_str(if l.focused { " focus" } else { " unfocused" });
        }
        if l.changed {
            class.push_str(" changed");
        }
        let _ = write!(
            out,
            "<div class=\"{class}\"><span class=\"n\">{}</span><span class=\"m\">{}</span><span class=\"t\">{}</span></div>",
            l.n,
            if l.changed { "*" } else { "" },
            esc(&l.text.replace('\t', "    "))
        );
    }
    out.push_str("</div></div>");
}

/// A terminal artifact: each session as a terminal (`term`): its state,
/// the command after a prompt, its output in colour, newest line in view.
fn terminal(out: &mut String, root: &str, sessions: &[SessionView]) {
    let running = sessions.iter().filter(|s| s.running).count();
    let _ = write!(
        out,
        "<div class=\"term-root note\"><span>Runs in <code>{}</code></span><span>{} · {}</span></div>",
        esc(root),
        inspect::count(sessions.len(), "session", "sessions"),
        if running == 0 { "none running".to_string() } else { format!("{running} running") }
    );
    if sessions.is_empty() {
        let _ = write!(
            out,
            "<div class=\"glass panel\">{}</div>",
            note("No sessions yet: run(...) runs one and waits; start(...) tells when it ends; background(...) does not.")
        );
    }
    for s in sessions {
        let (class, _, _) = super::term::state(s);
        let _ = write!(
            out,
            "<div class=\"glass panel term {class}\" data-session=\"{}\"><div class=\"term-head\">{}{}<span class=\"term-id\">#{}</span>{}{}<span class=\"note term-age\">{}</span>{}</div>",
            s.id,
            super::term::mark(s),
            super::term::prompt(&s.command),
            s.id,
            chip(&s.mode),
            super::term::badge(s),
            esc(&s.age),
            super::term::copy_button(),
        );
        super::term::output(out, &s.tail, if s.running { "no output yet" } else { "it printed nothing" });
        out.push_str("</div>");
    }
}

/// A shared context: its level's posts as a feed.
fn shared(out: &mut String, posts: Option<&[PostView]>) {
    let Some(posts) = posts else {
        let _ = write!(out, "<div class=\"glass panel\">{}</div>", note("Read with the engines when the view is taken."));
        return;
    };
    let _ = write!(out, "<div class=\"glass panel feed\"><div class=\"line\"><b>Posts</b>{}</div>", chip(&posts.len().to_string()));
    if posts.is_empty() {
        out.push_str(&note("No posts yet: post(text=\"...\") writes one for the level."));
    }
    for p in posts {
        let _ = write!(
            out,
            "<div class=\"post\"><div class=\"line small\"><b class=\"accent\">{}</b><span class=\"dim2\">{}</span></div><div>{}</div></div>",
            esc(&p.author),
            esc(&p.when),
            esc(&p.text)
        );
    }
    out.push_str("</div>");
}

/// A learning artifact: its sections, each with its fill against its cap.
fn learning(out: &mut String, sections: &[SectionFill], collecting: &str) {
    out.push_str(&note(&format!("Collecting now: {collecting}")));
    for s in sections {
        let ratio = s.used as f64 / s.cap.max(1) as f64;
        let _ = write!(
            out,
            "<div class=\"glass panel\"><div class=\"line\"><b>{}</b>{}<span class=\"grow\"></span><span class=\"note\">{} / {} characters</span></div>{}{}",
            esc(&s.name),
            chip(&inspect::count(s.entries.len(), "entry", "entries")),
            s.used,
            s.cap,
            gauge(ratio, ratio >= 0.9),
            note(&s.holds)
        );
        for e in &s.entries {
            let _ = write!(out, "<div class=\"small\">· {}</div>", esc(e));
        }
        out.push_str("</div>");
    }
}

/// An engines artifact: each child as a card, opening its engine page.
fn engines(out: &mut String, children: Option<&[ChildView]>, cx: &Ctx) {
    let Some(children) = children else {
        let _ = write!(out, "<div class=\"glass panel\">{}</div>", note("Read with the engines when the view is taken."));
        return;
    };
    if children.is_empty() {
        let _ =
            write!(out, "<div class=\"glass panel\">{}</div>", note("No children yet: engine_start(...) or engine_call(...) makes one."));
        return;
    }
    out.push_str("<div class=\"cards\">");
    for c in children {
        let j = cx.world.state.index_of(&c.id);
        let style = j.map(|j| tint(&cx.activity(j))).unwrap_or_default();
        let (open, close) = match j {
            Some(j) => (format!("<a class=\"glass panel card tinted link\" href=\"{}\"{style}>", dry_core::routes::engine_path(j)), "</a>"),
            None => (format!("<div class=\"glass panel card tinted\"{style}>"), "</div>"),
        };
        let _ = write!(
            out,
            "{open}<div class=\"line\"><span class=\"icon accent\">◇</span><b class=\"grow trunc\">{}</b>{}</div><div class=\"small\">{}</div>{}",
            esc(&c.name),
            chip(if c.call { "call" } else { "start" }),
            esc(if c.goal.is_empty() { "(no goal)" } else { &c.goal }),
            note(&format!(
                "{} · {}",
                if c.activity.is_empty() { "idle" } else { &c.activity },
                inspect::count(c.replies, "reply", "replies")
            ))
        );
        if let Some(why) = &c.failed {
            let _ = write!(out, "<div class=\"note bad\">{}</div>", esc(why));
        }
        if let Some(r) = &c.last_reply {
            let short: String = r.chars().take(240).collect();
            let more = if short.len() < r.len() { "…" } else { "" };
            out.push_str(&note(&format!("{short}{more}")));
        }
        out.push_str(close);
    }
    out.push_str("</div>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::evidence::artifact::ShotView;
    use dry_core::evidence::report::Status;

    fn shot(name: &str, status: Status, after: bool) -> ShotView {
        ShotView {
            name: name.into(),
            status,
            changed: if after { 1.25 } else { 0.0 },
            regions: usize::from(after),
            text: if after { vec!["- link \"Open\"".into(), "+ link \"Open now\"".into()] } else { vec![] },
            png: format!("evidence/{name}.png"),
            after: after.then(|| format!("evidence/{name}.new.png")),
            diff: after.then(|| format!("evidence/{name}.diff.png")),
        }
    }

    #[test]
    fn evidence_shows_before_after_and_diff_for_a_change() {
        let ev = inspect::EvidenceDetail {
            root: ".".into(),
            command: "dry evidence".into(),
            line: "2 shots · 1 changed (home@1280x800 1.25%) · fail".into(),
            commit: Some(("abc1234".into(), 0)),
            shots: vec![shot("home@1280x800", Status::Changed, true), shot("landing@1280x800", Status::Same, false)],
        };
        let mut out = String::new();
        let base = evidence_url("trip planner", "ev");
        assert_eq!(base, "/api/engines/trip%20planner/evidence/ev/");
        evidence(&mut out, &ev, Some(&base));
        assert!(out.contains("tone-bad\">2 shots · 1 changed"), "{out}");
        for want in ["before", "after", "diff"] {
            assert!(out.contains(&format!("<figcaption class=\"note\">{want}</figcaption>")), "{want}: {out}");
        }
        assert!(out.contains("src=\"/api/engines/trip%20planner/evidence/ev/home@1280x800.new.png\""));
        assert!(out.contains("+ link &quot;Open now&quot;") || out.contains("+ link \"Open now\""), "{out}");
        assert!(out.contains("<figcaption class=\"note\">now</figcaption>"), "an unchanged shot shows itself");
        // A shell that serves no files names them.
        let mut plain = String::new();
        evidence(&mut plain, &ev, None);
        assert!(plain.contains("evidence/home@1280x800.new.png") && !plain.contains("<img"));
    }
}
