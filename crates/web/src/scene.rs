//! The scene setting's scene (`dry_core::scene`) on a `<canvas>`: the same frames
//! the GPUI window paints as quads, painted as rectangles (a glow is the
//! canvas's shadow blur). Behind the app, and behind the landing page.
//!
//! The core says what to draw and when the picture next changes; this
//! only paints, on `requestAnimationFrame`, never faster than the scene's
//! fps, and parks when the frame says nothing moves. `prefers-reduced-
//! motion` and the motion setting 0 keep it still. Its colours are
//! resolved for the theme and mode on screen (`Scene::colours`) whenever
//! the app redraws with other tokens.
//!
//! What it reacts to, as in the GPUI window: the engines' mood (`set_mood`,
//! from the app on every draw: `dry_core::scene::mood`, Home's tiles' rule)
//! and the pointer. Mouse move, press, release and leaving the page are
//! heard on the document in the capture phase and never stopped (the
//! canvas itself takes no pointer events), so the page under it gets every
//! click as before. Each change steps the scene's memory at the moment it
//! happened and wakes the loop.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dry_core::scene::{self, Inputs, Live, Mood, Pointer, Scene};
use dry_core::theme::{self, Mode, Theme, Tokens};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::bundle;

struct Painter {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    scene: Scene,
    /// Each palette key's colour for the theme and mode on screen.
    colours: BTreeMap<char, u32>,
    live: Option<Live>,
    still: bool,
    last: f64,
}

/// What the scene hears from the page: the engines' mood and the pointer
/// (in CSS pixels; turned into cells when the scene steps). Kept apart
/// from the painter, so a new scene or theme keeps them.
#[derive(Clone, Copy, Default)]
struct Heard {
    mood: Mood,
    pointer: Option<(f64, f64, bool, u32, bool)>,
}

/// The frame callback, held while the loop runs.
type Frame = Rc<Closure<dyn FnMut(f64)>>;

thread_local! {
    static PAINTER: RefCell<Option<Painter>> = const { RefCell::new(None) };
    static LOOP: RefCell<Option<Frame>> = const { RefCell::new(None) };
    static HEARD: RefCell<Heard> = RefCell::new(Heard::default());
    static LISTENING: RefCell<bool> = const { RefCell::new(false) };
}

fn now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now() / 1000.0)
}

fn reduced_motion() -> bool {
    web_sys::window().and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten()).is_some_and(|m| m.matches())
}

/// A scene by name from the bundle.
pub fn named(name: &str) -> Option<Scene> {
    let text = bundle::FILES.iter().find(|(p, _)| *p == dry_core::look::main_path(dry_core::look::Kind::Scene, name)).map(|(_, t)| *t)?;
    Scene::from_md(name, text).ok()
}

/// A theme by name from the bundle.
pub fn theme_named(name: &str) -> Option<Theme> {
    let text = bundle::FILES.iter().find(|(p, _)| *p == dry_core::look::main_path(dry_core::look::Kind::Theme, name)).map(|(_, t)| *t)?;
    Theme::from_md(text).ok()
}

/// Paint `scene` in the colours it has with `tokens` on the canvas `#id`
/// from now on, still when `still` (the motion setting 0); a new call
/// replaces the old scene (`None` clears it).
pub fn show(id: &str, scene: Option<Scene>, tokens: &Tokens, still: bool) {
    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<HtmlCanvasElement>().ok())
    else {
        return;
    };
    let front = scene.as_ref().is_some_and(|s| s.place == dry_core::scene::Place::Front);
    let _ = canvas.set_attribute("data-place", if front { "front" } else { "back" });
    let Some(scene) = scene else {
        canvas.set_width(0);
        PAINTER.with(|p| *p.borrow_mut() = None);
        return;
    };
    let colours = scene.colours(tokens);
    let still = still || reduced_motion();
    let same = PAINTER.with(|p| p.borrow().as_ref().is_some_and(|p| p.scene == scene && p.colours == colours && p.still == still));
    if same {
        return;
    }
    let Some(ctx) = canvas.get_context("2d").ok().flatten().and_then(|c| c.dyn_into::<CanvasRenderingContext2d>().ok()) else { return };
    PAINTER.with(|p| *p.borrow_mut() = Some(Painter { canvas, ctx, scene, colours, live: None, still, last: 0.0 }));
    listen();
    wake();
}

/// How the engines are doing (`dry_core::scene::mood`). Called on every
/// app draw; only a change steps the scene.
pub fn set_mood(mood: Mood) {
    let changed = HEARD.with(|h| {
        let mut h = h.borrow_mut();
        let changed = h.mood != mood;
        h.mood = mood;
        changed
    });
    if changed {
        heard();
    }
}

/// Listen to the pointer on the whole page, once: capture phase, never
/// stopped or prevented.
fn listen() {
    if LISTENING.with(|l| std::mem::replace(&mut *l.borrow_mut(), true)) {
        return;
    }
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let on = |target: &web_sys::EventTarget, kind: &str, f: fn(&mut (f64, f64, bool, u32, bool), &web_sys::MouseEvent)| {
        let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
            HEARD.with(|h| {
                let mut h = h.borrow_mut();
                let mut p = h.pointer.unwrap_or_default();
                f(&mut p, &e);
                h.pointer = Some(p);
            });
            heard();
        });
        let _ = target.add_event_listener_with_callback_and_bool(kind, cb.as_ref().unchecked_ref(), true);
        cb.forget();
    };
    let at = |p: &mut (f64, f64, bool, u32, bool), e: &web_sys::MouseEvent| {
        p.0 = e.client_x() as f64;
        p.1 = e.client_y() as f64;
        p.4 = true;
    };
    on(&doc, "mousemove", at);
    on(&doc, "mousedown", |p, e| {
        p.0 = e.client_x() as f64;
        p.1 = e.client_y() as f64;
        p.4 = true;
        p.2 = true;
        p.3 = p.3.wrapping_add(1);
    });
    on(&doc, "mouseup", |p, e| {
        p.0 = e.client_x() as f64;
        p.1 = e.client_y() as f64;
        p.2 = false;
    });
    if let Some(root) = doc.document_element() {
        on(&root, "mouseleave", |p, _| p.4 = false);
    }
}

/// The floor in CSS pixels from the top: the footer row's top edge, so a
/// walking scene stands on the footer, as in the window (`layout::INSET`
/// and `FOOTER_HEIGHT` above the bottom while there is no footer yet).
fn floor_px(ch: f64) -> f64 {
    let top = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("footer"))
        .map_or(0.0, |f| f.get_bounding_client_rect().top());
    if top > 0.0 && top <= ch { top } else { ch - (dry_core::layout::INSET + dry_core::layout::FOOTER_HEIGHT) as f64 }
}

/// The scene's inputs now, for an area of `cw` x `ch` CSS pixels.
fn inputs(scene: &Scene, still: bool, cw: f64, ch: f64) -> Inputs {
    let cell = scene.cell as f64;
    let h = HEARD.with(|h| *h.borrow());
    let pointer = h.pointer.map(|(x, y, pressed, clicks, inside)| Pointer { x: (x / cell) as f32, y: (y / cell) as f32, pressed, clicks, inside });
    Inputs {
        mood: h.mood,
        cols: (cw / cell) as f32,
        rows: (ch / cell) as f32,
        floor: (floor_px(ch) / cell).floor() as f32,
        still,
        pointer,
    }
}

/// The mood or the pointer changed: step the scene at this moment (so a
/// click is timed when it happened, not at the next frame) and wake the
/// loop. A move far from a sleeping cat changes nothing, and the loop
/// stays parked.
fn heard() {
    let Some((cw, ch)) = web_sys::window().and_then(|w| Some((w.inner_width().ok()?.as_f64()?, w.inner_height().ok()?.as_f64()?))) else {
        return;
    };
    let t = now();
    let changed = PAINTER.with(|p| {
        let mut p = p.borrow_mut();
        let p = p.as_mut()?;
        let live = p.live.as_ref()?;
        let next = scene::step(live, inputs(&p.scene, p.still, cw, ch), t);
        let fps = p.scene.fps();
        let changed = scene::frame(&p.scene, live, t, fps) != scene::frame(&p.scene, &next, t, fps);
        p.live = Some(next);
        if changed {
            // Draw it on the next animation frame, whatever the fps gate.
            p.last = 0.0;
        }
        Some(changed)
    });
    if changed == Some(true) {
        wake();
    }
}

/// Start the frame loop if it is not running.
fn wake() {
    let running = LOOP.with(|l| l.borrow().is_some());
    if running {
        return;
    }
    let f: Frame = Rc::new(Closure::new(|_: f64| {
        let again = PAINTER.with(|p| p.borrow_mut().as_mut().map(paint)).flatten();
        match again {
            Some(()) => request(),
            None => LOOP.with(|l| *l.borrow_mut() = None),
        }
    }));
    LOOP.with(|l| *l.borrow_mut() = Some(f));
    request();
}

fn request() {
    LOOP.with(|l| {
        if let (Some(f), Some(w)) = (l.borrow().as_ref(), web_sys::window()) {
            let _ = w.request_animation_frame(f.as_ref().as_ref().unchecked_ref());
        }
    });
}

/// One frame, if it is time; `Some(())` to keep the loop going.
fn paint(p: &mut Painter) -> Option<()> {
    let t = now();
    let fps = p.scene.fps();
    if t - p.last < 1.0 / fps as f64 - 0.004 {
        return Some(());
    }
    let w = web_sys::window()?;
    let dpr = w.device_pixel_ratio();
    let (cw, ch) = (w.inner_width().ok()?.as_f64()?, w.inner_height().ok()?.as_f64()?);
    let (pw, ph) = ((cw * dpr) as u32, (ch * dpr) as u32);
    if p.canvas.width() != pw || p.canvas.height() != ph {
        p.canvas.set_width(pw);
        p.canvas.set_height(ph);
    }
    let cell = p.scene.cell as f64;
    let inputs = inputs(&p.scene, p.still, cw, ch);
    let live = match &p.live {
        Some(l) => scene::step(l, inputs, t),
        None => scene::start(inputs, t),
    };
    let frame = scene::frame(&p.scene, &live, t, fps);
    p.live = Some(live);
    p.last = t;
    let ctx = &p.ctx;
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0).ok()?;
    ctx.clear_rect(0.0, 0.0, cw, ch);
    for d in &frame.draws {
        let Some(&c) = p.colours.get(&d.key) else { continue };
        let a = ((c & 0xFF) as f64 / 255.0) * d.alpha as f64;
        if a <= 0.0 {
            continue;
        }
        let rgb = format!("rgba({},{},{},{a:.3})", c >> 24, (c >> 16) & 0xFF, (c >> 8) & 0xFF);
        ctx.set_fill_style_str(&rgb);
        if d.glow > 0.0 {
            ctx.set_shadow_color(&rgb);
            ctx.set_shadow_blur(d.glow as f64 * cell);
        } else {
            ctx.set_shadow_blur(0.0);
        }
        ctx.fill_rect(d.x as f64 * cell, d.y as f64 * cell, d.w as f64 * cell, d.h as f64 * cell);
    }
    // Parks when nothing will move (a still scene); the loop wakes on `show`.
    frame.next.map(|_| ())
}

/// For the landing page: the scene `name` (named here, not a setting) in
/// theme `theme`'s dark mode on canvas `#id`, and the theme's tokens and
/// the scene's colours as CSS for the page (`#tokens`, `--scene-<key>`).
#[wasm_bindgen]
pub fn landing(id: &str, name: &str, theme_name: &str) {
    let Some(th) = theme_named(theme_name).or_else(|| theme_named("default")) else { return };
    let (light, dark) = (theme::resolve(Mode::Light, &th), theme::resolve(Mode::Dark, &th));
    let scene = named(name);
    let motion = th.legacy_motion().unwrap_or(scene::DEFAULT_MOTION);
    if let Some(s) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("tokens")) {
        let colours = scene.as_ref().map(|s| dry_html::css::scene(s, &light, &dark)).unwrap_or_default();
        s.set_text_content(Some(&(dry_html::css::tokens(&light, &dark, motion) + &colours)));
    }
    show(id, scene, &dark, false);
}
