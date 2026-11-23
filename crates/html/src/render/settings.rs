//! Home, Engines and Settings: `crates/gpui/src/views/{engine,settings,
//! ui_page}.rs`'s tiles, cards and lists, as HTML. What would change the
//! app is drawn disabled; Light/Dark and the themes are previews (links
//! with `?mode=` and `?theme=`), shown here and never saved.

use std::fmt::Write;

use dry_core::card::{AsCard, Status};
use dry_core::composer::Capabilities;
use dry_core::routes::{self, Nav};
use dry_core::theme::{self, COLOR_NAMES, FONT, FONT_MONO, MOTION, Mode, NUMBER_NAMES, SCENE};

use super::{Ctx, card, chip, esc, note, section, tint, tone};

/// `dashboard`: Home's tiles (`dry_core::dashboard::tiles`), then every
/// engine doing something on its activity's tint.
pub fn dashboard(out: &mut String, cx: &Ctx) {
    let engines = &cx.world.state.engines;
    let statuses: Vec<Status> = (0..engines.len()).map(|i| cx.status(i)).collect();
    let tiles = dry_core::dashboard::tiles(engines, &statuses, &Capabilities::default());
    out.push_str("<div class=\"dashboard\" data-live=\"dashboard\"><div class=\"tiles\">");
    // The browser has no voice programs to check: no tile for it there.
    for tile in tiles.into_iter().filter(|t| !(cx.world.live() && !cx.world.hosted() && t.label == "Voice")) {
        let _ = write!(
            out,
            "<div class=\"glass tile\"><div class=\"note\">{}</div><div class=\"value\">{}</div><div class=\"small trunc {}\">{}</div></div>",
            esc(tile.label),
            esc(&tile.value),
            tone(tile.tone),
            esc(&tile.detail)
        );
    }
    out.push_str("</div><div class=\"row\">");
    for (i, e) in engines.iter().enumerate() {
        let a = cx.activity(i);
        if a.token().is_some() {
            let _ = write!(
                out,
                "<span class=\"busy tinted\"{}><b>{}</b><span class=\"dim2\">{}</span></span>",
                tint(&a),
                esc(&e.name),
                esc(&a.label())
            );
        }
    }
    out.push_str("</div>");
    board(out, cx, &statuses);
    out.push_str("</div>");
}

/// The board under the tiles (`dry_core::dashboard::board`): one-click
/// actions and the model connection (in the browser), every engine on its
/// activity's tint with its replies and context gauge, and the artifacts
/// most recently worked on.
fn board(out: &mut String, cx: &Ctx, statuses: &[Status]) {
    let engines = &cx.world.state.engines;
    let activities: Vec<_> = (0..engines.len()).map(|i| cx.activity(i)).collect();
    let context: Vec<Option<f64>> = (0..engines.len())
        .map(|i| match cx.world.reported.measured.get(&i) {
            Some(m) => Some(m.budget.ratio()),
            None if engines[i].session.is_none() => cx.snap(i).map(|x| x.ratio),
            None => None,
        })
        .collect();
    let b = dry_core::dashboard::board(engines, &activities, statuses, &context);
    out.push_str("<div class=\"board\">");
    if let Some(c) = &cx.world.connection {
        let status = match &c.check {
            Some(s) => s.clone(),
            None if c.model.is_empty() => Status::Missing("no model chosen: Settings → Models → Detect".into()),
            None => Status::Done(format!("{} at {}", c.model, c.endpoint)),
        };
        let _ = write!(
            out,
            "<div class=\"glass panel\"><div class=\"line\"><b>Model server</b><span class=\"note trunc grow\">{}</span>{}</div>{}<div class=\"row\">{}{}{}<a class=\"btn\" href=\"/settings\">Settings</a></div></div>",
            esc(&c.endpoint),
            chip(if c.model.is_empty() { "no model" } else { &c.model }),
            super::status_line(&status),
            cx.act("＋ New thread", "primary", "data-act=\"new-thread\""),
            cx.act("Load demo", "", "data-act=\"load-demo\""),
            cx.act("Check", "", "data-act=\"test\""),
        );
    }
    let mut main = String::new();
    let _ = write!(main, "<div class=\"glass panel\"><div class=\"line\"><b>Engines</b>{}</div>", chip(&b.engines.len().to_string()));
    if b.engines.is_empty() {
        main.push_str(&note("No engines yet: type below to start one, or load the demo."));
    }
    for e in &b.engines {
        let ratio = e.context.unwrap_or(0.0);
        let over = ratio >= dry_core::context::COMPRESS_AT;
        let doing = match &e.activity {
            dry_core::activity::Activity::Idle => e.status.word().to_string(),
            a => a.label(),
        };
        let _ = write!(
            main,
            "<a class=\"board-engine tinted\" href=\"{}\"{}><span class=\"dot {}\">●</span><span class=\"trunc\"><b>{}</b> <span class=\"dim2\">{}{}</span></span><span class=\"note\">{}</span>",
            esc(&routes::engine_path(e.index)),
            tint(&e.activity),
            tone(e.status.tone()),
            esc(&e.name),
            esc(&e.kind),
            e.parent.as_ref().map(|p| format!(" · child of {}", esc(p))).unwrap_or_default(),
            esc(&format!("{} · {}", doing, dry_core::inspect::count(e.replies, "reply", "replies"))),
        );
        if let Some(r) = &e.last_reply {
            let short: String = r.chars().take(160).collect();
            let _ = write!(main, "<span class=\"small trunc board-reply\">{}</span>", esc(&short));
        }
        if e.context.is_some() {
            main.push_str(&super::gauge(ratio, over));
        }
        main.push_str("</a>");
    }
    main.push_str("</div>");
    let mut side = String::new();
    let _ = write!(
        side,
        "<div class=\"glass panel\"><div class=\"line\"><b>Recent artifacts</b>{}</div>",
        chip(&b.artifacts.len().to_string())
    );
    if b.artifacts.is_empty() {
        side.push_str(&note("None yet: engines make them as they work."));
    }
    for a in &b.artifacts {
        let path = format!("{}/artifacts/{}", routes::engine_path(a.engine), super::encode(&a.name));
        let _ = write!(
            side,
            "<a class=\"artifact-row\" href=\"{}\"><div class=\"grow col\"><div class=\"line\"><b class=\"trunc\">{}</b>{}</div><div class=\"note trunc\">{}{}</div></div><span class=\"open\">›</span></a>",
            esc(&path),
            esc(&a.name),
            chip(&a.kind),
            esc(&a.engine_name),
            if a.line.is_empty() { String::new() } else { format!(" · {}", esc(&a.line)) }
        );
    }
    side.push_str("</div>");
    super::columns_html(out, &main, &side, super::SIDE_WIDTH);
    out.push_str("</div>");
}

/// `engines`: the desk's list, in two groups: the model engines, then the
/// sessions (an engine running a program from `sessions.json`, with that
/// program's icon). One card per engine, its status and its activity's
/// tint; click to open. Each group ends with its New buttons.
pub fn engines(out: &mut String, cx: &Ctx) {
    let tools = cx.world.sessions.as_ref().ok();
    let engines = &cx.world.state.engines;
    let group = |out: &mut String, sessions: bool| {
        let mut any = false;
        out.push_str("<div class=\"cards\">");
        for (i, e) in engines.iter().enumerate().filter(|(_, e)| e.session.is_some() == sessions) {
            any = true;
            let mut c = e.card();
            // The goal as a sentence, as the engine's header shows it (not its markdown).
            if !e.description.is_empty()
                && let Some(first) = c.lines.first_mut()
            {
                *first = dry_core::goal::summary(&e.description);
            }
            if let Some(tool) = e.session.as_ref().and_then(|s| tools.and_then(|t| t.get(&s.tool))) {
                c = c.with_icon(tool.icon.clone());
            }
            let c = c.with_status(cx.status(i));
            card(out, &c, Some(&routes::engine_path(i)), &tint(&cx.activity(i)), |_| {});
        }
        out.push_str("</div>");
        if !any {
            let _ = write!(out, "<div class=\"note\">{}</div>", if sessions { "No sessions yet." } else { "No model engines yet." });
        }
    };
    let _ = write!(out, "<div class=\"stack-2\" data-live=\"engines\">{}", section("ENGINES · models"));
    group(out, false);
    out.push_str("<div class=\"row\">");
    out.push_str(&cx.act("＋ New thread", "", "data-act=\"new-thread\""));
    out.push_str("</div>");
    out.push_str(&section("SESSIONS · programs"));
    group(out, true);
    out.push_str("<div class=\"row\">");
    match &cx.world.sessions {
        Ok(s) => {
            for tool in &s.tools {
                out.push_str(&cx.act(
                    &format!("{} New {} session", tool.icon, tool.name),
                    "",
                    &format!("data-act=\"new-session\" data-value=\"{}\"", esc(&tool.name)),
                ));
            }
        }
        Err(e) => {
            let _ = write!(out, "<span class=\"small bad\">{}</span>", esc(e));
        }
    }
    out.push_str("</div>");
    super::engine::approvals(out, cx, None);
    out.push_str("</div>");
}

/// `tools`: one card per program in `sessions.json`, ready when installed.
pub fn tools(out: &mut String, cx: &Ctx) {
    let _ = write!(out, "<div class=\"stack-2\">{}", section("PROGRAMS · sessions.json"));
    match &cx.world.sessions {
        Ok(sessions) => {
            out.push_str("<div class=\"cards\">");
            for tool in &sessions.tools {
                let mut c = tool.card();
                let status = match cx.world.programs.get(&tool.name).cloned().flatten() {
                    Some(path) => {
                        c.lines.push(path.display().to_string());
                        Status::Ready
                    }
                    None => Status::Missing(format!("`{}` is not on PATH", tool.start.first().map_or("", String::as_str))),
                };
                card(out, &c.with_status(status), None, "", |_| {});
            }
            out.push_str("</div>");
        }
        Err(e) => {
            let _ = write!(out, "<div class=\"bad\">{}</div>", esc(e));
        }
    }
    out.push_str("</div>");
}

/// `reset-history`: Reset history (`dry_core::store::RESET_LABEL`). The
/// first press opens the confirm step (a fold, kept open across live
/// refreshes): what goes, where the backup lands, what stays. Only its
/// second press, "Yes, reset history", sends `reset-history`.
pub fn reset_history(out: &mut String, cx: &Ctx) {
    use dry_core::store::{RESET_KEEPS, RESET_LABEL, RESET_REMOVES};
    if !cx.world.live() || !cx.world.can("reset-history") {
        let _ = write!(out, "<div class=\"row\">{}</div>", cx.off(RESET_LABEL, ""));
        return;
    }
    let (wherever, keeps) = if cx.world.hosted() {
        let path = cx.world.store.split_once(':').map_or(cx.world.store.as_str(), |(_, p)| p).trim_end_matches('/');
        let backup = match path.rsplit_once('.') {
            Some((stem, ext)) if !stem.ends_with('/') && !ext.contains('/') => format!("{stem}.reset-<UTC time>.{ext}"),
            _ => format!("{path}.reset-<UTC time>"),
        };
        (format!("They move, not deleted, to {backup} beside the store (put it back by hand to undo)."), RESET_KEEPS.to_string())
    } else {
        (
            format!("This browser's store ({}) is cleared; a copy goes to its IndexedDB database dry-reset when it has one, and nothing else is kept.", cx.world.store),
            "this browser's model connection and key (local/settings.json)".to_string(),
        )
    };
    super::fold(out, "reset-history", false, RESET_LABEL, None, "reset", |out| {
        let _ = write!(
            out,
            "{}{}{}<div class=\"row\">{}</div>",
            note(&format!("Goes: {RESET_REMOVES}. {wherever}")),
            note("Then the app runs from default/ alone: the translator, on models.json's default model, with no messages."),
            note(&format!("Stays: {keeps}. Close this to cancel.")),
            cx.act("Yes, reset history", "primary", "data-act=\"reset-history\""),
        );
    });
}

/// `appearance`: Light / Dark and the themes. Each is a preview here: a
/// link that shows the page that way; the desktop app saves a choice.
pub fn appearance(out: &mut String, cx: &Ctx) {
    let here = cx.location;
    let mode = cx.t.mode;
    let theme = cx.query.theme.clone().unwrap_or_else(|| cx.world.active_theme.clone());
    let link = |m: Mode, th: &str| {
        let mut q = vec![format!("mode={}", m.as_str())];
        if th != cx.world.active_theme {
            q.push(format!("theme={}", super::encode(th)));
        }
        format!("{here}?{}", q.join("&"))
    };
    let _ = write!(out, "<div class=\"stack-2\">{}<div class=\"row\">", section("APPEARANCE"));
    let live = cx.world.live();
    for (m, label) in [(Mode::Light, "☀ Light"), (Mode::Dark, "☾ Dark")] {
        let class = if m == mode { "primary" } else { "" };
        if live && cx.world.can("set-mode") {
            out.push_str(&cx.act(label, class, &format!("data-act=\"set-mode\" data-value=\"{}\"", m.as_str())));
        } else {
            let _ = write!(out, "<a class=\"btn {class}\" href=\"{}\">{label}</a>", esc(&link(m, &theme)));
        }
    }
    let _ = write!(out, "</div>{}<div class=\"row\">", section("THEME · themes/"));
    for name in &cx.world.themes {
        let class = if *name == theme { "primary" } else { "" };
        if live && cx.world.can("set-theme") {
            out.push_str(&cx.act(name, class, &format!("data-act=\"set-theme\" data-value=\"{}\"", esc(name))));
        } else {
            let _ = write!(out, "<a class=\"btn {class}\" href=\"{}\">{}</a>", esc(&link(mode, name)), esc(name));
        }
    }
    // The scene: its own thing, not part of a theme (any theme shows any
    // scene, or none). Saved as app.md's `scene:`.
    let _ = write!(out, "</div>{}<div class=\"row\">", section("SCENE · scenes/"));
    let now = cx.world.state.scene.clone().unwrap_or_else(|| theme::NO_SCENE.to_string());
    let names = std::iter::once(theme::NO_SCENE.to_string()).chain(cx.world.scenes.keys().cloned());
    for name in names {
        let class = if name == now { "primary" } else { "" };
        let label = if name == theme::NO_SCENE { "Off".to_string() } else { name.clone() };
        if live && cx.world.can("set-scene") {
            out.push_str(&cx.act(&label, class, &format!("data-act=\"set-scene\" data-value=\"{}\"", esc(&name))));
        } else {
            let _ = write!(out, "<span class=\"btn {class}\">{}</span>", esc(&label));
        }
    }
    let saved = format!("The app is in {} mode with theme {}.", cx.world.state.mode.as_str(), cx.world.active_theme);
    let how = if cx.world.hosted() {
        "The mode and the theme are saved by the host (the theme in themes/active)."
    } else if live {
        "Saved in this browser."
    } else {
        "Previews only, not saved."
    };
    let _ = write!(out, "</div>{}</div>", note(&format!("{how} {saved}")));
}

/// The browser's connection to its model server: the endpoint (LM Studio
/// at `localhost:1234` unless changed), the model, an optional key that
/// stays in this browser, Detect and Test. Saved in the browser's store.
fn connection(out: &mut String, c: &crate::world::Connection, cx: &Ctx) {
    let _ = write!(
        out,
        "<div class=\"glass panel\" id=\"connection\"><div class=\"line\"><b>This browser's model server</b>{}</div>\
         <label class=\"field-label\"><span class=\"note\">Endpoint (OpenAI-compatible, up to /v1)</span><input class=\"field-in\" name=\"endpoint\" value=\"{}\" placeholder=\"http://localhost:1234/v1\" spellcheck=\"false\"></label>\
         <label class=\"field-label\"><span class=\"note\">Model</span><input class=\"field-in\" name=\"model\" value=\"{}\" list=\"detected-models\" placeholder=\"press Detect, then pick one\" spellcheck=\"false\"></label><datalist id=\"detected-models\">",
        super::chip(if c.model.is_empty() { "no model" } else { "ready" }),
        esc(&c.endpoint),
        esc(&c.model)
    );
    if let Some(Ok(ids)) = &c.detected {
        for id in ids {
            let _ = write!(out, "<option value=\"{}\">", esc(id));
        }
    }
    out.push_str("</datalist>");
    if let Some(Ok(ids)) = &c.detected {
        out.push_str("<div class=\"row\">");
        for id in ids {
            out.push_str(&cx.act(
                id,
                if *id == c.model { "primary" } else { "outline" },
                &format!("data-act=\"pick-model\" data-value=\"{}\"", esc(id)),
            ));
        }
        out.push_str("</div>");
    }
    let _ = write!(
        out,
        "<label class=\"field-label\"><span class=\"note\">API key (only for a cloud server; not needed for LM Studio)</span><input class=\"field-in\" name=\"key\" type=\"password\" autocomplete=\"off\" placeholder=\"{}\"></label>\
         <div class=\"row\">{}{}{}{}</div>",
        if c.has_key { "a key is saved in this browser (type to replace)" } else { "none" },
        cx.act("Save", "primary", "data-act=\"save-connection\""),
        cx.act("Detect", "", "data-act=\"detect\""),
        cx.act("Test", "", "data-act=\"test\""),
        if c.has_key { cx.act("Forget key", "", "data-act=\"forget-key\"") } else { String::new() },
    );
    if let Some(s) = &c.check {
        out.push_str(&super::status_line(s));
    }
    out.push_str(&note(
        "A key typed here is kept only in this browser's storage (IndexedDB), readable by any script this page runs; never type one on a shared computer. New engines use this endpoint and model.",
    ));
    out.push_str(&note(
        "LM Studio: Developer → Settings → Enable CORS, and serve on port 1234. On the hosted (https) page, Chrome asks once to let it reach apps on this device: allow it. Safari: use the local copy. See docs/browser.md.",
    ));
    out.push_str("</div>");
}

/// `models`: one card per model setting in `models.json`, the default ringed.
pub fn models(out: &mut String, cx: &Ctx) {
    let label = if cx.world.connection.is_some() { "MODELS · this browser" } else { "MODELS · models.json" };
    let _ = write!(out, "<div class=\"stack-2\">{}", section(label));
    if let Some(c) = &cx.world.connection {
        connection(out, c, cx);
        out.push_str("</div>");
        return;
    }
    match &cx.world.models {
        Err(e) => {
            let _ = write!(out, "<div class=\"bad\">{}</div>", esc(e));
        }
        Ok(models) => {
            out.push_str("<div class=\"cards\">");
            for m in &models.models {
                let is_default = m.name == models.default;
                let c = (models, m).card();
                let ring = if is_default { " style=\"border-color:var(--accent)\"" } else { "" };
                card(out, &c, None, ring, |out| {
                    let name = esc(&m.name);
                    let _ = write!(
                        out,
                        "<div class=\"row\" data-model=\"{name}\">{}{}",
                        cx.act("Detect", "", &format!("data-act=\"detect-model\" data-value=\"{name}\"")),
                        cx.act("Test", "primary", &format!("data-act=\"test-model\" data-value=\"{name}\""))
                    );
                    if !is_default {
                        out.push_str(&cx.act("Make default", "", &format!("data-act=\"make-default\" data-value=\"{name}\"")));
                    }
                    out.push_str("</div>");
                });
            }
            out.push_str("</div>");
        }
    }
    out.push_str("</div>");
}

/// `routes`: every route, and the files that did not read.
pub fn routes(out: &mut String, cx: &Ctx) {
    let _ = write!(out, "<div class=\"stack-1\">{}", section("ROUTES · routes/"));
    for e in &cx.world.errors {
        let _ = write!(out, "<div class=\"bad small\">{}</div>", esc(e));
    }
    let removed: Vec<&String> = cx.world.view.removed.iter().collect();
    let rows = cx.world.table.list().iter().map(|r| (r.pattern.clone(), r.icon.clone(), r.title.clone(), r.nav, false));
    let off_rows = removed
        .iter()
        .filter_map(|p| cx.world.pages.get(*p).map(|page| ((*p).clone(), page.icon.clone(), page.title.clone(), page.nav, true)));
    for (pattern, icon, title, nav, is_off) in rows.chain(off_rows) {
        let standard = routes::is_standard(&pattern);
        let moved = cx.world.table.list().iter().find(|r| r.pattern == pattern).and_then(|r| r.moved.as_deref());
        let how = match (is_off, nav, moved) {
            (true, _, _) => " (turned off)".to_string(),
            (_, _, Some(to)) => format!(" (moved to {to})"),
            (_, Nav::No, _) => " (not in panel)".into(),
            (_, Nav::Yes, _) => String::new(),
            (_, Nav::PerEngine, _) => " (one button per engine)".into(),
        };
        let hidden = cx.world.view.is_hidden(&pattern);
        let _ = write!(
            out,
            "<div class=\"route-row{}\"><a class=\"pattern trunc\" href=\"{}\">{}</a><span class=\"grow dim2 trunc\">{} {}{}{how}</span>",
            if is_off { " off" } else { "" },
            esc(&pattern),
            esc(&pattern),
            esc(&icon),
            esc(&title),
            if standard { " · standard" } else { "" },
        );
        let p = esc(&pattern);
        if nav != Nav::No && !is_off {
            out.push_str(&cx.act(if hidden { "Show" } else { "Hide" }, "", &format!("data-act=\"route-hide\" data-value=\"{p}\"")));
        }
        if standard && pattern != routes::SETTINGS {
            out.push_str(&cx.act(if is_off { "Turn on" } else { "Turn off" }, "", &format!("data-act=\"route-off\" data-value=\"{p}\"")));
        }
        if !standard {
            out.push_str(&cx.act("Delete", "danger", &format!("data-act=\"route-delete\" data-value=\"{p}\"")));
        }
        out.push_str("</div>");
    }
    let _ = write!(
        out,
        "{}<div class=\"row\">{}</div></div>",
        note(if cx.world.can("route-create") {
            "Pages are folders under routes/: New page below writes one, or `cargo run -p dry-tools -- page /path Title`."
        } else {
            "Pages are folders under routes/; create one in the desktop app, or with `cargo run -p dry-tools -- page /path Title`."
        }),
        cx.act("Reload routes/", "", "data-act=\"refresh\"")
    );
}

/// `diagnostics`: what serves this page, and the window it is drawn in
/// (filled in by the page's script, which knows the browser's size).
pub fn diagnostics(out: &mut String, cx: &Ctx) {
    let w = cx.world;
    let _ = write!(
        out,
        "<div class=\"stack-1\">{}{}{}{}{}<div class=\"small dim2\" data-viewport>viewport: (measured by the browser)</div></div>",
        section("DIAGNOSTICS"),
        note(&w.served_by),
        note(&format!("store {}{}", w.store, w.store_error.as_ref().map(|e| format!(" · does not read: {e}")).unwrap_or_default())),
        note(&format!("{} routes · {} components · {} file(s) did not read", w.table.list().len(), w.components.len(), w.errors.len())),
        note(if w.hosted() {
            "turns run on this server (the host); live updates and turn events: /events (server-sent events)"
        } else if w.live() {
            "turns run in this page; the model is called with fetch"
        } else {
            "live updates: /events (server-sent events)"
        }),
    );
}

/// `environment`: the machine the file and terminal tools run on
/// (`dry_core::environment`): its kind, state and workspace, and for the
/// browser VM its saved copy, with a Forget whose fold is the confirm step.
pub fn environment(out: &mut String, cx: &Ctx) {
    let _ = write!(out, "<div class=\"stack-1\" data-live=\"environment\">{}", section("ENVIRONMENT"));
    let Some(env) = &cx.world.state.capabilities.0.environment else {
        let _ = write!(out, "{}</div>", note("not reported by this shell: its file and terminal tools reach no machine"));
        return;
    };
    let tone_class = match env.phase {
        dry_core::environment::Phase::Ready => "tone-good",
        dry_core::environment::Phase::Booting(_) => "tone-busy",
        dry_core::environment::Phase::Failed(_) => "tone-bad",
    };
    let _ = write!(
        out,
        "<div class=\"glass panel stack-1\" data-environment=\"{}\" data-state=\"{}\"><div class=\"line\"><span class=\"dot {tone_class}\">●</span><b>{}</b>{}</div>",
        env.kind.as_str(),
        env.phase.word(),
        esc(env.kind.label()),
        chip(env.phase.word())
    );
    for (k, v) in env.rows().into_iter().skip(1) {
        let _ = write!(out, "<div class=\"line small\"><span class=\"dim2\" style=\"min-width:88px\">{}</span><span class=\"trunc grow\">{}</span></div>", esc(k), esc(&v));
    }
    out.push_str("</div>");
    if env.can_forget() {
        super::fold(out, "env-forget", false, "Forget cached VM", None, "reset", |out| {
            let _ = write!(
                out,
                "{}{}<div class=\"row\">{}</div>",
                note("Goes: the saved machine in this browser (IndexedDB dry-vm): the files in /work and the packages installed since. It cannot be undone."),
                note("The VM running now stays until the page reloads and is not saved again; the next load boots a fresh one. Close this to cancel."),
                cx.act("Yes, forget the cached VM", "primary", "data-act=\"env-forget\""),
            );
        });
    }
    out.push_str("</div>");
}

/// `new-route`: the form. Creating a page writes a file, so it is live
/// only where the host takes it (`route-create`); elsewhere disabled.
pub fn new_route(out: &mut String, cx: &Ctx) {
    let on = cx.world.can("route-create");
    let field = |label: &str, name: &str, width: u32| {
        format!(
            "<label class=\"field-label\" style=\"width:{width}px\"><span class=\"note\">{label}</span><input class=\"field-in\" name=\"{name}\" data-keep=\"new-route-{name}\"{}></label>",
            if on { "" } else { " disabled" }
        )
    };
    let _ = write!(
        out,
        "<div class=\"stack-2\">{}<form class=\"row end\" data-new-route onsubmit=\"return false\">{}{}{}{}</form></div>",
        section("NEW PAGE"),
        field("Path", "path", 160),
        field("Title", "title", 160),
        field("Icon", "icon", 64),
        cx.act("Create", "primary", "data-act=\"route-create\"")
    );
}

/// `tokens`: every design token of the theme on screen: a swatch, its
/// name (changed ones lit), its value and its default.
pub fn tokens(out: &mut String, cx: &Ctx) {
    let Ok(theme) = &cx.world.theme else {
        let _ = write!(out, "<div class=\"bad\">{}</div>", esc(cx.world.theme.as_ref().err().map_or("", String::as_str)));
        return;
    };
    let mode = cx.t.mode;
    let defaults = theme::defaults(theme, mode);
    let t = cx.t;
    let _ = write!(
        out,
        "<div class=\"intro\"><h2>UI tokens</h2>{}</div><div class=\"row\">{}{}{}</div>",
        note(&format!(
            "Every design token, {} mode, as this page is drawn (the theme {} with this machine's state/local.md). Reset all writes the theme file (on the host or in the desktop app); editing one token is the desktop app's for now.",
            mode.as_str(),
            cx.world.active_theme
        )),
        cx.act("Refresh", "primary", "data-act=\"refresh\""),
        cx.act("Reset all", "", "data-act=\"reset-theme\""),
        cx.act("Reload theme", "", "data-act=\"refresh\"")
    );
    out.push_str("<div class=\"token-head\"><span></span><span>TOKEN</span><span>VALUE</span><span>DEFAULT</span></div>");
    let row = |out: &mut String, preview: String, name: &str, value: String, default: String, changed: bool| {
        let _ = write!(
            out,
            "<div class=\"token-row\">{preview}<span class=\"trunc{}\">{}</span><code class=\"trunc\">{}</code><span class=\"note trunc\">{}</span></div>",
            if changed { " accent" } else { "" },
            esc(name),
            esc(&value),
            esc(&default)
        );
    };
    for name in COLOR_NAMES {
        let v = t.color(name).unwrap_or(0);
        let swatch = format!("<span class=\"swatch\" style=\"background:{}\"></span>", theme::hex(v));
        row(out, swatch, name, theme::hex(v), theme::hex(defaults.color(name).unwrap_or(0)), theme.is_changed(mode, name));
    }
    for name in NUMBER_NAMES {
        let v = t.number(name).unwrap_or(0.0);
        let bar = format!("<span class=\"numbar\"><i style=\"width:{}px\"></i></span>", v.clamp(0.0, 24.0));
        row(out, bar, name, v.to_string(), defaults.number(name).unwrap_or(0.0).to_string(), theme.is_changed(mode, name));
    }
    let shipped = cx.world.fonts.join(", ");
    for (name, value, default) in [(FONT, &t.font_family, &theme.defaults.font), (FONT_MONO, &t.font_mono, &theme.defaults.font_mono)] {
        let preview = match name {
            FONT_MONO => "<span class=\"glyph mono-font\">Aa</span>".to_string(),
            _ => "<span class=\"glyph\">Aa</span>".to_string(),
        };
        row(out, preview, name, value.clone(), default.clone(), value != default);
    }
    out.push_str(&note(&format!(
        "The browser draws a font token with its family, then the platform's own; shipped in assets/fonts: {}.",
        if shipped.is_empty() { "none" } else { &shipped }
    )));
    // The scene settings: the appearance's, not the theme's (any theme
    // shows any scene). The value in force here; the default is app.md's.
    let look = crate::page::scene_of(cx.world, cx.location, theme);
    let state = &cx.world.state;
    let app = |v: Option<String>| v.map_or_else(|| "(default)".to_string(), |v| format!("app.md: {v}"));
    out.push_str(&section("SCENE"));
    let scene = look.scene.clone().unwrap_or_else(|| theme::NO_SCENE.to_string());
    let from_app = state.scene.clone().unwrap_or_default();
    row(out, "<span class=\"glyph\">≈</span>".to_string(), SCENE, scene.clone(), app(state.scene.clone()), scene != from_app);
    let motion = look.motion.to_string();
    let changed = state.motion.is_none_or(|m| m != look.motion);
    row(out, "<span class=\"glyph\">∿</span>".to_string(), MOTION, motion, app(state.motion.map(|m| m.to_string())), changed);
    let problem = match look.scene.as_deref().map(|n| (n, cx.world.scenes.get(n))) {
        Some((n, None)) => format!(" Scene \"{n}\" is not in scenes/, so nothing is drawn."),
        Some((_, Some(Err(e)))) => format!(" It does not read, so nothing is drawn: {e}"),
        _ => String::new(),
    };
    out.push_str(&note(&format!(
        "Not theme tokens: app.md's scene and motion, unless this machine's state/local.md (a ## Scene row) or the page's overrides say otherwise. The scene owns its colours; a key with a role takes the theme's.{problem}"
    )));
}

/// `capabilities`: every capability this runtime has
/// (`dry_core::capability::view`), by provider and state; one that needs a
/// person's gesture has a Grant button (the browser asks for the
/// permission on that click); the peers that joined, and a form to join
/// one (the host).
pub fn capabilities(out: &mut String, cx: &Ctx) {
    use dry_core::capability::{self, State};
    let reg = &cx.world.state.capabilities.0;
    let (groups, [present, grant, absent]) = capability::view(reg);
    let _ = write!(
        out,
        "<div class=\"stack-2\" data-live=\"capabilities\">{}<div class=\"note\">{present} present · {grant} need a grant · {absent} absent. An absent one is in no prompt and brings no tools; engines use a capability through an artifact that lists it (<code>uses: [battery]</code>).</div>",
        section("CAPABILITIES")
    );
    for g in &groups {
        let _ = write!(out, "<div class=\"glass panel stack-2\" data-provider=\"{}\"><div class=\"line\"><b>{}</b>", esc(&g.provider.text()), esc(&capability::group_title(g)));
        if let (capability::Provider::Remote(name), Some(p)) = (&g.provider, &g.peer)
            && p.connected
        {
            out.push_str(&cx.act("Leave", "", &format!("data-act=\"leave-peer\" data-value=\"{}\"", esc(name))));
        }
        out.push_str("</div>");
        for r in &g.rows {
            let (tone_class, word) = match &r.state {
                State::Present => ("tone-good", "present"),
                State::NeedsGrant(_) => ("tone-busy", "needs grant"),
                State::Absent(_) => ("tone-neutral", "absent"),
            };
            let _ = write!(
                out,
                "<div class=\"cap-row{}\" data-capability=\"{}\" data-state=\"{}\"><div class=\"line\"><span class=\"dot {tone_class}\">●</span><b>{}</b>{}<span class=\"note trunc grow\">{}</span>",
                if r.used { "" } else { " dim" },
                esc(&r.id),
                r.state.word(),
                esc(&r.id),
                chip(word),
                esc(&r.line)
            );
            if matches!(r.state, State::NeedsGrant(_)) {
                out.push_str(&cx.act("Grant", "primary", &format!("data-act=\"grant\" data-value=\"{}\"", esc(&r.id))));
            }
            out.push_str("</div>");
            if !r.state.why().is_empty() {
                out.push_str(&note(r.state.why()));
            }
            if !r.used && r.state == State::Present {
                out.push_str(&note("another provider's is used for this id"));
            }
            if !r.tools.is_empty() {
                let tools: Vec<String> = r.tools.iter().map(|(n, a)| if *a { format!("{n} (needs approval)") } else { n.clone() }).collect();
                out.push_str(&note(&format!("tools: {}", tools.join(", "))));
            }
            if !r.events.is_empty() {
                out.push_str(&note(&format!("events: {}", r.events.join(", "))));
            }
            if let Some(s) = &r.snapshot {
                let _ = write!(out, "<pre class=\"cap-snapshot\">{}</pre>", esc(s));
            }
            out.push_str("</div>");
        }
        out.push_str("</div>");
    }
    if cx.world.hosted() {
        let _ = write!(
            out,
            "<form class=\"glass panel stack-2\" data-peer onsubmit=\"return false\"><b>Join a peer</b><div class=\"note\">A companion (or another runtime) on this machine or one you reach: its address and the pairing code it printed. Its capabilities join this host's, tagged <code>remote:&lt;name&gt;</code>, and leave when it does.</div><div class=\"row\"><input name=\"peer-url\" placeholder=\"http://127.0.0.1:7777\"><input name=\"peer-code\" placeholder=\"pairing code\"><input name=\"peer-name\" placeholder=\"name (companion)\">{}</div></form>",
            cx.act("Join", "primary", "data-act=\"join-peer\"")
        );
    }
    out.push_str("</div>");
}
