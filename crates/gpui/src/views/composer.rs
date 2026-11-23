//! `composer`: the one text area the whole app types into (see
//! `dry_core::composer`). A glass capsule: the input on top, its options
//! below it on the left and Send on the right. The input itself draws no
//! fill or border, so the glass shows through it. Which buttons it has and
//! whether each is on comes from the core (`composer::buttons`), from what
//! this runtime can do (`AppView::caps`).

use dry_core::composer::{self, Action};
use dry_core::routes::Params;
use dry_core::theme::Tokens;
use gpui::{Context, Div, SharedString, div, prelude::*, px};
use gpui_component::input::Textarea;

use crate::frame::glass;
use crate::ui::color;
use crate::{AppView, button};

pub fn draw(app: &AppView, params: &Params, actions: &[Action], t: &Tokens, cx: &mut Context<AppView>) -> Div {
    let engine = params.index("id").filter(|&i| i < app.state.engines.len());

    let mut options = div().flex().flex_1().min_w_0().items_center().gap(px(t.space_1));
    let mut send = None;
    for b in composer::buttons(actions, &app.caps, engine.is_some()) {
        let action = b.action;
        let label = format!("{} {}", action.icon(), action.label());
        let mut el = button(SharedString::from(format!("composer-{}", action.label())), label, t);
        el = if b.enabled {
            el.on_click(cx.listener(move |this, _, window, cx| this.composer_action(action, window, cx)))
        } else {
            // Off: dimmed, and pressing it says why.
            let why = b.why.clone().unwrap_or_default();
            el.opacity(0.45).on_click(cx.listener(move |this, _, _, cx| {
                this.composer_note = Some(format!("{}: {why}", action.label()));
                cx.notify();
            }))
        };
        if action == Action::Send {
            send = Some(el.bg(color(t.accent)).text_color(color(t.on_accent)));
        } else {
            options = options.child(el);
        }
    }

    glass(t)
        .flex()
        .flex_col()
        .gap(px(t.space_2))
        .p(px(t.space_2))
        .child(Textarea::new(&app.input).appearance(false))
        .child(div().flex().items_center().gap(px(t.space_2)).child(options).children(send))
        .when_some(app.composer_note.clone(), |d, note| {
            d.child(div().px(px(t.space_1)).text_size(px(t.text_sm)).text_color(color(t.text_secondary)).child(note))
        })
}
