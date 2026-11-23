use dry_core::theme::{self, Tokens};
use gpui::{Context, Div, SharedString, div, prelude::*, px};
use gpui_component::input::Input;

use super::intro;
use crate::ui::{self, color};
use crate::{AppView, TokenKind, button, token_color, token_number};

/// `/settings/ui` (a page of Settings): every design token, its current and default value, a box to
/// change it and a Reset. Refresh applies all boxes app-wide at once.
pub fn tokens(app: &AppView, t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let mode = app.state.mode;
    let defaults = theme::defaults(&app.theme, mode);
    let missing_font = cx.global::<ui::Ui>().missing_font.clone();
    let (font_options, mono_options) = {
        let g = cx.global::<ui::Ui>();
        (g.font_options.clone(), g.mono_options.clone())
    };

    let rows = app.token_inputs.iter().map(|row| {
        let name = row.name;
        let (default, preview) = match row.kind {
            TokenKind::Color => {
                let current = token_color(t, name);
                // A swatch of the live value, bordered so translucent colours still show.
                let swatch = div()
                    .flex_none()
                    .size(px(24.))
                    .rounded(px(t.radius))
                    .border_1()
                    .border_color(color(t.border_emphasized))
                    .bg(color(current));
                (theme::hex(token_color(&defaults, name)), swatch)
            }
            TokenKind::Number => {
                let value = token_number(t, name);
                // A bar as wide as the value, so sizes can be compared.
                let bar =
                    div().flex_none().w(px(24.)).flex().items_center().child(div().h(px(6.)).w(px(value.min(24.))).bg(color(t.accent)));
                (token_number(&defaults, name).to_string(), bar)
            }
            TokenKind::Font => {
                let default = if name == dry_core::theme::FONT_MONO { &app.theme.defaults.font_mono } else { &app.theme.defaults.font };
                (default.clone(), div().flex_none().w(px(24.)).child("Aa"))
            }
            TokenKind::Setting => {
                let d = dry_core::scene::settings(&dry_core::state::State::default(), &app.theme, &[]);
                let default = if name == theme::SCENE { d.scene.unwrap_or_else(|| theme::NO_SCENE.into()) } else { d.motion.to_string() };
                (default, div().flex_none().w(px(24.)).child(if name == theme::SCENE { "≈" } else { "∿" }))
            }
        };
        let changed = match row.kind {
            TokenKind::Setting => row.input.read(cx).value().trim() != default,
            _ => app.theme.is_changed(mode, name),
        };
        // A font token also lists its options (local, theme, default,
        // shipped, fallback), each marked if this machine has it; the
        // first one it has is drawn. Clicking one puts it in the box.
        let options = match row.kind {
            TokenKind::Font if name == theme::FONT_MONO => Some(&mono_options),
            TokenKind::Font => Some(&font_options),
            _ => None,
        };
        let drawn = options.and_then(|o| theme::pick_font(o)).map(|o| o.family.clone());
        let chips = options.map(|options| {
            div().flex().flex_wrap().gap(px(t.space_1)).pl(px(24. + t.space_2)).pb(px(t.space_1)).text_size(px(t.text_sm)).children(
                options.iter().enumerate().map(|(i, o)| {
                    let input = row.input.clone();
                    let family = o.family.clone();
                    let mark = if !o.available { "✗" } else if drawn.as_deref() == Some(o.family.as_str()) { "●" } else { "✓" };
                    button(SharedString::from(format!("font-{name}-{i}")), format!("{mark} {} · {}", o.family, o.source.word()), t)
                        .when(!o.available, |b| b.text_color(color(t.text_disabled)))
                        .when(drawn.as_deref() == Some(o.family.as_str()), |b| b.text_color(color(t.text_accent)))
                        .on_click(cx.listener(move |_, _, window, cx| input.update(cx, |s, cx| s.set_value(family.clone(), window, cx))))
                }),
            )
        });
        let line = div()
            .flex()
            .items_center()
            .gap(px(t.space_2))
            .py(px(t.space_1))
            .child(preview)
            .child(div().w(px(150.)).flex_none().truncate().when(changed, |d| d.text_color(color(t.text_accent))).child(name))
            .child(div().flex_1().min_w_0().child(Input::new(&row.input)))
            .child(div().w(px(90.)).flex_none().truncate().text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(default))
            .child(
                button(SharedString::from(format!("reset-{name}")), "Reset", t)
                    .when(!changed, |b| b.text_color(color(t.text_disabled)))
                    .on_click(cx.listener(move |this, _, window, cx| this.reset_tokens(Some(name), window, cx))),
            );
        div().flex().flex_col().border_b_1().border_color(color(t.border)).child(line).children(chips)
    });

    div()
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .child(intro(
            "UI tokens",
            &format!(
                "Every design token. Colours edit the {} mode. Change values, then Refresh: it writes the active theme file (themes/<name>.md). Scene and motion are app settings, not tokens: they are saved with the state (app.md); a ## Scene row in state/local.md wins over them.",
                mode.as_str()
            ),
            t,
        ))
        .child(
            div()
                .flex()
                .gap(px(t.space_2))
                .child(
                    button("tokens-refresh", "Refresh", t)
                        .bg(color(t.accent))
                        .text_color(color(t.on_accent))
                        .on_click(cx.listener(|this, _, window, cx| this.refresh_tokens(window, cx))),
                )
                .child(
                    button("tokens-reset-all", "Reset all", t)
                        .on_click(cx.listener(|this, _, window, cx| this.reset_tokens(None, window, cx))),
                )
                .child(
                    button("tokens-reload", "Reload theme", t)
                        .on_click(cx.listener(|this, _, window, cx| this.reload_theme(window, cx))),
                ),
        )
        .when_some(missing_font, |d, font| {
            d.child(
                div().text_color(color(t.error)).child(format!(
                    "Font \"{font}\" is not installed; drawing the system font. Put its .ttf in assets/fonts/ and restart."
                )),
            )
        })
        .when_some(scene_problem(app, t), |d, why| d.child(div().text_color(color(t.error)).child(why)))
        .when(!app.token_errors.is_empty(), |d| {
            d.child(div().text_color(color(t.error)).child(format!("Not applied (bad value): {}", app.token_errors.join(", "))))
        })
        .child(
            div()
                .flex()
                .gap(px(t.space_2))
                .text_size(px(t.text_sm))
                .text_color(color(t.text_secondary))
                .child(div().w(px(24.)).flex_none())
                .child(div().w(px(150.)).flex_none().child("TOKEN"))
                .child(div().flex_1().child("VALUE"))
                .child(div().w(px(90.)).flex_none().child("DEFAULT")),
        )
        .children(rows)
}

/// Why the scene setting's scene is not drawn, if it names one that is
/// not there or does not read.
fn scene_problem(app: &AppView, _t: &Tokens) -> Option<String> {
    let look = app.scene_settings();
    let name = look.scene.as_deref()?;
    match app.scenes.get(name) {
        None => Some(format!("Scene \"{name}\" is not in scenes/ (there: {}); nothing is drawn.", app.scenes.keys().cloned().collect::<Vec<_>>().join(", "))),
        Some(Err(e)) => Some(format!("Scene \"{name}\" does not read, so nothing is drawn: {e}")),
        Some(Ok(_)) => None,
    }
}
