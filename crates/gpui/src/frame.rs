//! Draws core's layout standard (`dry_core::layout::Frame`) with GPUI. It
//! knows nothing about engines: it takes finished pieces (the pill's
//! buttons, optional header, body, optional footer) and puts each in its
//! place. Any screen uses the same frame by handing it different pieces.
//!
//! The look is liquid glass. The window itself is frosted by the OS (see
//! `main`) and only lightly tinted (`window_tint`). On the tint drift soft
//! blobs of light, each at its own depth and pace (`dry_core::backdrop`);
//! over them float the glass layers: the navigation and one content pane.
//! The navigation is a pinned sidebar, or opens from the header's button
//! as a side panel or an overlay (`layout::Placement`).
//!
//! Containment (rule 3 in `dry_core::layout`) is enforced here, once: every
//! part clips what does not fit, and the content box scrolls inside itself.
//!
//! Every colour and size comes from the design tokens (`ui::tokens`).

use dry_core::backdrop::LAYERS;
use dry_core::layout::{FOOTER_HEIGHT, Frame, HEADER_HEIGHT, INSET, PILL_TOP, Placement};
use dry_core::scene::Place;
use dry_core::theme::Tokens;
use std::time::Duration;

use gpui::{Animation, AnimationExt, AnyElement, BoxShadow, Div, Hsla, div, linear_color_stop, linear_gradient, point, prelude::*, px};

use crate::ui::color;

/// The pieces a screen hands to the frame.
pub struct Parts {
    /// The navigation's buttons, top to bottom (shown when open).
    pub panel: AnyElement,
    pub header: Option<AnyElement>,
    pub body: AnyElement,
    pub footer: Option<AnyElement>,
    /// The scene setting's scene (`crate::scene`), and where it goes: behind the
    /// glass across the whole window, or behind the footer row's text.
    pub scene: Option<(Place, AnyElement)>,
    /// The motion setting (`dry_core::scene::Settings`): seconds per loop
    /// of the slowest backdrop layer; 0 still.
    pub motion: f32,
}

/// The light behind the glass: each layer of `dry_core::backdrop` as a blob
/// with no fill and a wide blurred shadow (GPUI has no blur filter; a big
/// shadow is a soft glow). Each moves on its own loop and pace, capped at
/// 30 frames a second: slow drift needs no more. `viewport` is the window
/// size; `motion` (the setting) 0 or a clear colour leaves the layer
/// still or out.
fn backdrop(t: &Tokens, viewport: (f32, f32), motion: f32) -> Vec<AnyElement> {
    let (w, h) = viewport;
    let side = w.min(h);
    LAYERS
        .iter()
        .enumerate()
        .filter(|(_, l)| t.backdrop[l.color] & 0xFF != 0)
        .map(|(n, l)| {
            let l = *l;
            let d = l.size() * side;
            let glow = color(t.backdrop[l.color]);
            // The core of the blob is a third of it; the blur makes the rest.
            let core = d / 3.;
            let blob = div().absolute().size(px(core)).rounded(px(core / 2.)).shadow(vec![BoxShadow {
                color: glow,
                offset: point(px(0.), px(0.)),
                blur_radius: px(d * l.softness()),
                spread_radius: px(core / 2.),
                inset: false,
            }]);
            let place = move |el: Div, phase: f32| {
                let (x, y) = l.at(phase);
                el.left(px(x * w - core / 2.)).top(px(y * h - core / 2.))
            };
            match l.period(motion) {
                None => place(blob, 0.).into_any_element(),
                Some(secs) => blob
                    .with_animation(
                        ("backdrop", n),
                        Animation::new(Duration::from_secs_f32(secs)).repeat_synced().with_max_fps(30.),
                        place,
                    )
                    .into_any_element(),
            }
        })
        .collect()
}

/// A soft drop shadow.
fn drop(c: Hsla, y: f32, blur: f32) -> BoxShadow {
    BoxShadow { color: c, offset: point(px(0.), px(y)), blur_radius: px(blur), spread_radius: px(0.), inset: false }
}

/// The glass material: translucent fill, lighter at the top (the sheen,
/// light falling on it), a bright 1px rim, a highlight just inside the top
/// edge, and a two-layer shadow that lifts it off what is behind. Any
/// component can be glass by starting from this; anything set after it
/// (`.bg`, `.rounded`, `.shadow`, ...) overrides it locally.
pub fn glass(t: &Tokens) -> Div {
    div()
        .rounded(px(t.pane_radius))
        .bg(linear_gradient(180., linear_color_stop(color(t.glass_sheen), 0.), linear_color_stop(color(t.glass_fill), 1.)))
        .border_1()
        .border_color(color(t.glass_edge))
        .shadow(vec![
            drop(color(t.shadow), 8., 28.),
            drop(color(t.shadow), 1., 3.),
            // The top edge catching light.
            BoxShadow { inset: true, ..drop(color(t.glass_highlight), 1., 0.) },
        ])
}

pub fn draw(frame: &Frame, t: &Tokens, viewport: (f32, f32), parts: Parts) -> Div {
    let inset = px(INSET);
    let (behind, in_footer, front) = match parts.scene {
        Some((Place::Window, el)) => (Some(el), None, None),
        Some((Place::Front, el)) => (None, None, Some(el)),
        Some((Place::Footer, el)) => (None, Some(el), None),
        None => (None, None, None),
    };

    // The content pane: one glass layer, inset from the window on every
    // side and past the pill's lane on the left. Header row, the box that
    // scrolls, footer row.
    let content = |body: AnyElement| {
        div()
            .w_full()
            .min_w_0()
            .overflow_x_hidden()
            .when_some(frame.content_max, |d, max| d.max_w(px(max)))
            .px(px(frame.gutter))
            .py(px(frame.gutter))
            .child(body)
    };
    // `flex_1` + `min_h_0` gives the box the leftover height and lets it be
    // shorter than its content, so the content scrolls here.
    // A view in panes (rule 5) gets the whole box and lays itself out;
    // anything else scrolls in it.
    let main = if frame.fill {
        div().id("main").flex_1().min_h_0().min_w_0().flex().flex_col().overflow_hidden().child(div().size_full().min_h_0().flex().flex_col().child(parts.body))
    } else {
        div().id("main").flex_1().min_h_0().overflow_y_scroll().overflow_x_hidden().flex().flex_col().items_center().child(content(parts.body))
    };
    let row = |height: f32| div().flex_none().h(px(height)).px(px(frame.gutter)).flex().items_center().gap(px(t.space_2)).overflow_hidden();
    let pane = glass(t)
        .absolute()
        .top(px(PILL_TOP))
        .bottom(inset)
        .left(px(frame.content_left()))
        .right(inset)
        .flex()
        .flex_col()
        .overflow_hidden()
        .when_some(parts.header, |d, header| d.child(row(HEADER_HEIGHT).border_b_1().border_color(color(t.border)).child(header)))
        .child(main)
        .when_some(parts.footer, |d, footer| {
            d.child(
                row(FOOTER_HEIGHT)
                    .relative()
                    .border_t_1()
                    .border_color(color(t.border))
                    .text_size(px(t.text_sm))
                    .text_color(color(t.text_secondary))
                    // A footer scene first, so the row's text is drawn over it.
                    .children(in_footer)
                    .child(footer),
            )
        });

    // The navigation, where the frame places it: a sidebar from the top to
    // the bottom inset (the pane starts after it), or an overlay dropped
    // from the header button over the pane, with a deeper shadow. Hidden:
    // nothing; the header's button opens it.
    let nav = glass(t).id("panel").absolute().w(px(frame.panel_width)).overflow_y_scroll().overflow_x_hidden().p(px(8.));
    let pill = match frame.placement {
        Placement::Hidden => None,
        Placement::Sidebar => Some(nav.top(px(PILL_TOP)).bottom(inset).left(inset)),
        Placement::Overlay => Some(
            nav.top(px(PILL_TOP + HEADER_HEIGHT))
                .left(px(frame.content_left() + 8.))
                .max_h(px(480.))
                .shadow(vec![drop(color(t.shadow), 12., 36.), drop(color(t.shadow), 1., 3.)]),
        ),
    }
    .map(|nav| nav.child(parts.panel));

    // The window: the tint, the light moving on it, the scene, the pane,
    // and the navigation, on top. A `window` scene is behind the glass, so
    // the panes frost it like the backdrop; a `front` one is last, over
    // everything (its view takes no clicks).
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .bg(color(t.window_tint))
        .text_color(color(t.text_primary))
        .text_size(px(t.text_base))
        // The font token, set once here; every text below inherits it.
        .font_family(t.font_family.clone())
        .children(backdrop(t, viewport, parts.motion))
        .children(behind)
        .child(pane)
        .children(pill)
        .children(front)
}
