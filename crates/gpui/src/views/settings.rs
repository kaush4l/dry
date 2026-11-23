use dry_core::card::AsCard;
use dry_core::routes::{self, Nav};
use dry_core::theme::{Mode, Tokens};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::section;
use crate::ui::color;
use crate::{AppView, button};

/// The settings sections. Each is a primitive a page can place
/// (`appearance`, `models`, `routes`, `diagnostics`, `new-route`);
/// `routes/settings/page.json` puts them together.
/// Light / Dark, and one button per theme in `themes/` (the active one
/// filled); a theme button switches to it at once.
pub fn appearance(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let choice = |mode: Mode, label: &'static str, cx: &mut Context<AppView>| {
        let active = app.state.mode == mode;
        button(SharedString::from(format!("mode-{}", mode.as_str())), label, t)
            .when(active, |b| b.bg(color(t.accent)).text_color(color(t.on_accent)))
            .on_click(cx.listener(move |this, _, window, cx| this.set_mode(mode, window, cx)))
    };
    let (names, active) = &app.themes;
    let themes: Vec<_> = names
        .iter()
        .map(|name| {
            let (on, pick) = (name == active, name.clone());
            button(SharedString::from(format!("theme-{name}")), name.clone(), t)
                .when(on, |b| b.bg(color(t.accent)).text_color(color(t.on_accent)))
                .on_click(cx.listener(move |this, _, window, cx| this.use_theme(&pick, window, cx)))
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(section("APPEARANCE", t))
        .child(div().flex().gap(px(t.space_2)).child(choice(Mode::Light, "☀ Light", cx)).child(choice(Mode::Dark, "☾ Dark", cx)))
        .child(section("THEME · themes/", t))
        .child(div().flex().flex_wrap().gap(px(t.space_2)).children(themes))
}

/// Reset history (`dry_core::store::RESET_LABEL`): the first press opens
/// the confirm step (what goes, where the backup lands, what stays); only
/// "Yes, reset history" there resets, through the host when one runs.
pub fn reset_history(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    use dry_core::store::{RESET_KEEPS, RESET_LABEL, RESET_REMOVES};
    let small = |s: String, c: gpui::Hsla| div().text_size(px(t.text_sm)).text_color(c).child(s);
    let open = app.reset_confirm;
    let mut out = div().flex().flex_col().gap(px(t.space_2)).child(
        div().flex().child(
            button("reset-history", if open { "Cancel" } else { RESET_LABEL }, t)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.reset_confirm = !this.reset_confirm;
                    this.reset_note = None;
                    cx.notify();
                })),
        ),
    );
    if open {
        let spec = dry_tools::store::default_spec();
        let backup = dry_tools::store::reset_plan(&spec)
            .map(|p| if p.is_empty() { "nothing to move: the store is the default already".to_string() } else { p.backup.display().to_string() })
            .unwrap_or_else(|e| e);
        let how = if app.hosted() { "The host does it (every open page reloads)." } else { "This window does it (no host runs)." };
        out = out.child(
            div()
                .flex()
                .flex_col()
                .gap(px(t.space_1))
                .p(px(t.space_3))
                .rounded(px(t.radius))
                .bg(color(t.background_muted))
                .child(small(format!("Goes: {RESET_REMOVES}."), color(t.text_primary)))
                .child(small(format!("They move, not deleted, to {backup}. {how}"), color(t.text_secondary)))
                .child(small("Then the app runs from default/ alone: the translator, on models.json's default model, with no messages.".into(), color(t.text_secondary)))
                .child(small(format!("Stays: {RESET_KEEPS}."), color(t.text_secondary)))
                .child(
                    div().flex().pt(px(t.space_1)).child(
                        button("reset-history-yes", "Yes, reset history", t)
                            .bg(color(t.accent))
                            .text_color(color(t.on_accent))
                            .on_click(cx.listener(|this, _, window, cx| this.reset_history(window, cx))),
                    ),
                ),
        );
    }
    out.when_some(app.reset_note.clone(), |d, n| d.child(small(n, color(t.text_secondary))))
}

/// One card per model setting, the same element as every other object
/// (`dry_core::card`): icon, name, default or not, id, where it points,
/// which model, and the last Detect/Test as its status. Under it: Detect
/// (ask the server what it has), Test (send one message), Make default,
/// and the ids Detect found (click one to use it).
pub fn models(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let cards = app.models.models.iter().map(|m| {
        let name = m.name.clone();
        let is_default = m.name == app.models.default;
        let mut card = (&app.models, m).card();
        if let Some(status) = app.model_status.get(&m.name) {
            card = card.with_status(status.clone());
        }
        let chosen = app.models.inference(m).map(|ai| ai.model).unwrap_or_default();
        let picks = app.detected.get(&m.name).cloned().unwrap_or_default().into_iter().map(|id| {
            let (name, on) = (name.clone(), id == chosen);
            button(SharedString::from(format!("pick-{name}-{id}")), id.clone(), t)
                .when(on, |b| b.bg(color(t.accent_muted)).text_color(color(t.text_accent)))
                .on_click(cx.listener(move |this, _, _, cx| this.pick_model(&name, &id, cx)))
        });
        let (n1, n2, n3) = (name.clone(), name.clone(), name.clone());
        super::card::draw(&card, t)
            .when(is_default, |d| d.border_color(color(t.accent)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(t.space_2))
                    .pt(px(t.space_1))
                    .child(
                        button(SharedString::from(format!("detect-{n1}")), "Detect", t)
                            .on_click(cx.listener(move |this, _, _, cx| this.detect_models(&n1, cx))),
                    )
                    .child(
                        button(SharedString::from(format!("test-{n2}")), "Test", t)
                            .bg(color(t.accent))
                            .text_color(color(t.on_accent))
                            .on_click(cx.listener(move |this, _, _, cx| this.test_model(&n2, cx))),
                    )
                    .when(!is_default, |d| {
                        d.child(
                            button(SharedString::from(format!("default-{n3}")), "Make default", t)
                                .on_click(cx.listener(move |this, _, _, cx| this.set_default_model(&n3, cx))),
                        )
                    }),
            )
            .child(div().flex().flex_wrap().gap(px(t.space_1)).children(picks))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(section("MODELS · models.json", t))
        .when_some(app.models_error.clone(), |d, e| d.child(div().text_color(color(t.error)).child(e)))
        .child(div().flex().flex_wrap().gap(px(t.space_3)).children(cards))
}

/// Every route read from `routes/`, with Hide/Show (its panel button) and,
/// for a standard route (Home, Settings, Code), Turn off / Turn on; any
/// other route can be deleted (removes its `page.json`). Hidden and
/// turned off are kept in `state/local.md`. Reload reads the folder again.
pub fn routes(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let off = app.router.removed.iter().map(|r| (r, true));
    let rows = app.router.routes().iter().map(|r| (r, false)).chain(off).map(|(r, removed)| {
        let pattern = r.pattern.clone();
        let (p_hide, p_del, p_off) = (pattern.clone(), pattern.clone(), pattern.clone());
        let standard = routes::is_standard(&pattern);
        div()
            .flex()
            .items_center()
            .gap(px(t.space_2))
            .py(px(t.space_1))
            .border_b_1()
            .border_color(color(t.border))
            .when(removed, |d| d.text_color(color(t.text_disabled)))
            .child(div().w(px(140.)).flex_none().truncate().child(pattern.clone()))
            .child(div().flex_1().min_w_0().truncate().text_color(color(t.text_secondary)).child(format!(
                "{} {}{}{}",
                r.icon,
                r.title,
                if standard { " · standard" } else { "" },
                match (removed, r.nav, r.moved.as_deref()) {
                    (true, _, _) => " (turned off)".to_string(),
                    (_, _, Some(to)) => format!(" (moved to {to})"),
                    (_, Nav::No, _) => " (not in panel)".into(),
                    (_, Nav::Yes, _) => String::new(),
                    (_, Nav::PerEngine, _) => " (one button per engine)".into(),
                }
            )))
            // Hide/Show only changes the panel: the route stays and still
            // opens; its button leaves the panel.
            .when(r.nav != Nav::No && !removed, |d| {
                let hidden = app.view.is_hidden(&pattern);
                d.child(
                    button(SharedString::from(format!("route-hide-{pattern}")), if hidden { "Show" } else { "Hide" }, t)
                        .when(hidden, |b| b.bg(color(t.accent_muted)).text_color(color(t.text_accent)))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_route_hidden(&p_hide, cx))),
                )
            })
            // A standard route is turned off, not deleted; Settings stays,
            // since it is where routes are turned back on.
            .when(standard && pattern != routes::SETTINGS, |d| {
                d.child(
                    button(SharedString::from(format!("route-off-{pattern}")), if removed { "Turn on" } else { "Turn off" }, t)
                        .when(removed, |b| b.bg(color(t.accent_muted)).text_color(color(t.text_accent)))
                        .on_click(cx.listener(move |this, _, window, cx| this.toggle_route_removed(&p_off, window, cx))),
                )
            })
            .when(!standard, |d| {
                d.child(
                    button(SharedString::from(format!("route-del-{pattern}")), "Delete", t)
                        .on_click(cx.listener(move |this, _, window, cx| this.delete_route(&p_del, window, cx))),
                )
            })
    });
    let errors = app.router.errors.iter().map(|e| div().text_color(color(t.error)).child(e.clone()));
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_1))
        .child(section("ROUTES · routes/", t))
        .children(errors)
        .children(rows)
        .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(format!(
            "Pages are folders under routes/; create one below, or with `cargo run -p dry-tools -- page /path Title`.{}",
            ""
        )))
        .child(
            div()
                .flex()
                .pt(px(t.space_2))
                .child(button("routes-restore", "Reload routes/", t).on_click(cx.listener(|this, _, _, cx| this.restore_routes(cx)))),
        )
}

/// What the window and screen are, asked of GPUI (which asks the OS).
pub fn diagnostics(t: &Tokens, window: &mut gpui::Window, cx: &mut Context<AppView>) -> Div {
    div().flex().flex_col().child(section("DIAGNOSTICS", t)).children(
        crate::screen_info(window, cx)
            .into_iter()
            .map(|line| div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(line)),
    )
}

/// The machine the file and terminal tools run on (`dry_core::environment`):
/// the window's is this computer, ready from the start.
pub fn environment(app: &AppView, t: &Tokens) -> Div {
    let small = |s: String, c: gpui::Hsla| div().text_size(px(t.text_sm)).text_color(c).child(s);
    let out = div().flex().flex_col().gap(px(t.space_1)).child(section("ENVIRONMENT", t));
    let Some(env) = &app.state.capabilities.0.environment else {
        return out.child(small("not reported by this shell".into(), color(t.text_secondary)));
    };
    let tone = if env.ready() { t.accent } else { t.error };
    out.child(div().flex().gap(px(t.space_2)).child(small("●".into(), color(tone))).child(div().child(env.kind.label())))
        .children(env.rows().into_iter().skip(1).map(|(k, v)| small(format!("{k}: {v}"), color(t.text_secondary))))
}

/// A form that creates a route at run time: path, title, icon glyph.
/// Create writes `routes/<path>/page.json` and opens the new page.
pub fn new_route(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let field = |label: &'static str, input: &gpui::Entity<gpui_component::input::InputState>, width: f32| {
        div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .w(px(width))
            .child(div().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(label))
            .child(gpui_component::input::Input::new(input))
    };
    let [path, title, icon] = &app.new_route;
    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(section("NEW PAGE", t))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_end()
                .gap(px(t.space_2))
                .child(field("Path", path, 160.))
                .child(field("Title", title, 160.))
                .child(field("Icon", icon, 64.))
                .child(
                    button("new-route-create", "Create", t)
                        .bg(color(t.accent))
                        .text_color(color(t.on_accent))
                        .on_click(cx.listener(|this, _, window, cx| this.create_route(window, cx))),
                ),
        )
        .when_some(app.new_route_error.clone(), |d, e| d.child(div().text_color(color(t.error)).child(e)))
}

/// Every capability this window has (`dry_core::capability::view`), by
/// provider and state: the same page as the web's. The window has no
/// browser permissions, so nothing here needs a grant; peers join the
/// host (dry-server), whose page lists them.
pub fn capabilities(app: &AppView, t: &Tokens) -> Div {
    use dry_core::capability::{self, State};
    let (groups, [present, grant, absent]) = capability::view(&app.state.capabilities.0);
    let small = |s: String, c: gpui::Hsla| div().text_size(px(t.text_sm)).text_color(c).child(s);
    let mut out = div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(section("CAPABILITIES", t))
        .child(small(format!("{present} present · {grant} need a grant · {absent} absent. An absent one is in no prompt and brings no tools."), color(t.text_secondary)));
    for g in groups {
        let mut panel = div()
            .flex()
            .flex_col()
            .gap(px(t.space_1))
            .p(px(t.space_3))
            .rounded(px(t.radius))
            .bg(color(t.background_muted))
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(capability::group_title(&g)));
        for r in g.rows {
            let (dot, word) = match &r.state {
                State::Present => (t.accent, "present"),
                State::NeedsGrant(_) => (t.error, "needs grant"),
                State::Absent(_) => (t.text_disabled, "absent"),
            };
            let mut row = div()
                .flex()
                .flex_col()
                .when(!r.used, |d| d.opacity(0.6))
                .child(
                    div()
                        .flex()
                        .gap(px(t.space_2))
                        .child(div().text_color(color(dot)).child("●"))
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(r.id.clone()))
                        .child(small(word.to_string(), color(t.text_secondary)))
                        .child(small(r.line.clone(), color(t.text_secondary))),
                );
            if !r.state.why().is_empty() {
                row = row.child(small(r.state.why().to_string(), color(t.text_disabled)));
            }
            if !r.tools.is_empty() {
                let tools: Vec<String> = r.tools.iter().map(|(n, a)| if *a { format!("{n} (needs approval)") } else { n.clone() }).collect();
                row = row.child(small(format!("tools: {}", tools.join(", ")), color(t.text_disabled)));
            }
            if let Some(s) = r.snapshot {
                row = row.child(div().font_family(t.font_mono.clone()).text_size(px(t.text_sm)).child(s));
            }
            panel = panel.child(row);
        }
        out = out.child(panel);
    }
    out
}
