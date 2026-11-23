//! Draws the scene setting's scene (`dry_core::scene`) with GPUI, and only
//! draws.
//!
//! `SceneView` is its own entity. The app hands it, on every render, the
//! scene the settings name, the engines' mood, whether motion is 0, and
//! the area it covers (`sync`); it keeps the scene's memory (`Live`) and a
//! clock. Its render asks core for the frame at the clock's `t` and paints
//! each cell rectangle as one quad on a canvas (a glow is a blurred shadow
//! in the same colour), each palette key in the colour it has with the
//! tokens on screen (`Scene::colours`: its own for the mode, or the
//! theme's for a role), so a new mode or theme recolours it at once.
//!
//! The pointer reaches the scene through window-level mouse listeners,
//! registered when the scene paints: move, press, release and leaving the
//! window, all in the capture phase, so they see every event wherever it
//! lands (over a pane, a button, the bare window) and never stop it: the
//! UI above gets every click as before. Each change steps the scene's
//! memory at that moment and redraws at once if the picture changed.
//!
//! The loop is a timer task, not `with_animation(...repeat)`, which never
//! rests: it waits until the frame's `next` (at least one frame at the
//! scene's fps), asks for a redraw, and ends (parks) when the frame says
//! nothing will change (`next` is None: `motion` 0, a sleeping cat), when
//! there is no scene, or while the window is not visible (GPUI's
//! `observe_window_visibility`: covered, minimised, display asleep). A new
//! input, the theme, or the window showing again starts it once more.

use std::time::{Duration, Instant};

use dry_core::scene::{self, Inputs, Live, Mood, Pointer, Scene};
use gpui::{
    Bounds, BoxShadow, Context, DispatchPhase, Hsla, IntoElement, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Render, Subscription, Task, Window, canvas, div, fill, point, prelude::*, px, size,
};

use crate::ui::{self, color};

pub struct SceneView {
    scene: Option<Scene>,
    live: Option<Live>,
    /// The area it covers, in logical pixels.
    area: (f32, f32),
    clock: Instant,
    visible: bool,
    /// When the last frame drawn said the picture next changes.
    next: Option<f64>,
    /// The pointer as the scene last heard it, in cells.
    pointer: Option<Pointer>,
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl SceneView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe_window_visibility(window, |this, visibility, _, cx| {
            this.visible = visibility.is_visible();
            this.wake(cx);
        })];
        SceneView {
            scene: None,
            live: None,
            area: (0., 0.),
            clock: Instant::now(),
            visible: window.is_visible(),
            next: None,
            pointer: None,
            task: None,
            _subscriptions: subscriptions,
        }
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    /// What the app sees now. Called on every render; only a change does
    /// anything: a new scene starts fresh, a new mood or size steps the
    /// scene's memory at this moment, and either wakes the loop.
    pub fn sync(&mut self, wanted: Option<&Scene>, mood: Mood, still: bool, area: (f32, f32), floor: f32, cx: &mut Context<Self>) {
        if wanted != self.scene.as_ref() {
            self.scene = wanted.cloned();
            self.live = None;
        }
        let Some(cell) = self.scene.as_ref().map(|s| s.cell) else {
            self.task = None;
            return;
        };
        self.area = area;
        let inputs = Inputs { mood, cols: area.0 / cell, rows: area.1 / cell, floor: (floor / cell).floor(), still, pointer: self.pointer };
        let t = self.now();
        let live = match &self.live {
            Some(live) if live.inputs == inputs => return,
            Some(live) => scene::step(live, inputs, t),
            None => scene::start(inputs, t),
        };
        self.live = Some(live);
        // Draw the new pose now, then follow the frame's own clock.
        self.next = Some(t);
        self.wake(cx);
    }

    /// The pointer did something (`change`) at a window position. Steps
    /// the scene now, and redraws now if the picture changed (its eyes
    /// turn the frame it moves); a move far from a sleeping cat changes
    /// nothing, so nothing is drawn.
    fn pointer(&mut self, at: Option<Point<Pixels>>, origin: Point<Pixels>, change: impl FnOnce(&mut Pointer), cx: &mut Context<Self>) {
        let (Some(scene), Some(live)) = (&self.scene, &self.live) else { return };
        let mut p = self.pointer.unwrap_or_default();
        if let Some(at) = at {
            p.x = f32::from(at.x - origin.x) / scene.cell;
            p.y = f32::from(at.y - origin.y) / scene.cell;
        }
        change(&mut p);
        if Some(p) == self.pointer {
            return;
        }
        self.pointer = Some(p);
        let t = self.now();
        let stepped = scene::step(live, Inputs { pointer: Some(p), ..live.inputs }, t);
        let fps = scene.fps();
        let same = scene::frame(scene, live, t, fps) == scene::frame(scene, &stepped, t, fps);
        self.live = Some(stepped);
        if !same {
            self.next = Some(t);
            cx.notify();
            self.wake(cx);
        }
    }

    /// (Re)start the loop, or park it.
    fn wake(&mut self, cx: &mut Context<Self>) {
        if !self.visible || self.scene.is_none() {
            self.task = None;
            return;
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(wait)) = this.update(cx, |view, _| view.wait()) else { break };
                cx.background_executor().timer(wait).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }

    /// How long until the next frame: until the picture changes, and at
    /// least one frame at the scene's fps. None: park.
    fn wait(&self) -> Option<Duration> {
        let fps = self.scene.as_ref()?.fps() as f64;
        let next = self.next?;
        if !self.visible {
            return None;
        }
        Some(Duration::from_secs_f64((next - self.now()).max(1.0 / fps)))
    }
}

impl Render for SceneView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        let root = div().absolute().top_0().left_0().size_full().overflow_hidden();
        let (Some(scene), Some(live)) = (&self.scene, &self.live) else { return root };
        let t = ui::tokens(cx);
        let now = self.now();
        let frame = scene::frame(scene, live, now, scene.fps());
        self.next = frame.next;

        // Cells to pixels, keys to their colours (times the draw's opacity).
        let cell = scene.cell;
        let colours = scene.colours(&t);
        let quads: Vec<(f32, f32, f32, f32, Hsla, f32)> = frame
            .draws
            .iter()
            .filter_map(|d| {
                let c = *colours.get(&d.key)?;
                let a = ((c & 0xFF) as f32 * d.alpha).round() as u32;
                (a > 0).then(|| (d.x * cell, d.y * cell, d.w * cell, d.h * cell, color((c & !0xFF) | a.min(0xFF)), d.glow * cell))
            })
            .collect();
        root.child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    // The pointer, wherever it is over the window: capture
                    // phase, never stopped, so the UI above is untouched.
                    let origin = bounds.origin;
                    let v = view.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = v.update(cx, |view, cx| view.pointer(Some(e.position), origin, |p| p.inside = true, cx));
                        }
                    });
                    let v = view.clone();
                    window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = v.update(cx, |view, cx| {
                                view.pointer(
                                    Some(e.position),
                                    origin,
                                    |p| {
                                        p.inside = true;
                                        p.pressed = true;
                                        p.clicks = p.clicks.wrapping_add(1);
                                    },
                                    cx,
                                )
                            });
                        }
                    });
                    let v = view.clone();
                    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = v.update(cx, |view, cx| view.pointer(Some(e.position), origin, |p| p.pressed = false, cx));
                        }
                    });
                    let v = view.clone();
                    window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = v.update(cx, |view, cx| view.pointer(None, origin, |p| p.inside = false, cx));
                        }
                    });
                    for (x, y, w, h, c, glow) in quads {
                        let r = Bounds { origin: bounds.origin + point(px(x), px(y)), size: size(px(w), px(h)) };
                        if glow > 0.0 {
                            let round = px(w.min(h) / 2.);
                            let halo = BoxShadow {
                                color: c,
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(glow),
                                spread_radius: px(0.),
                                inset: false,
                            };
                            window.paint_drop_shadows(r, round.into(), &[halo]);
                        }
                        window.paint_quad(fill(r, c));
                    }
                },
            )
            .size_full(),
        )
    }
}
