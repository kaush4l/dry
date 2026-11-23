//! The cat: dry's own cat (`app/eggs/pet/egg.js` in dry), ported exactly.
//! The same body (each part drawn twice, first fattened in the outline
//! colour and then filled, so the whole cat gets one sticker outline), the
//! same states, constants and physics, in CSS pixels (the scene file says
//! `cell 1`, so a cell is a pixel).
//!
//! States: `sit` (looks at the pointer; the tail sways a while after it
//! settles), `run` (speed by distance, eased), `stalk` (low, wiggling, tail
//! flicking), `crouch` (anticipation), `air` (ballistic arc; the spine
//! arches most at the top and the hind legs tuck, paws reach for the
//! pointer), `land` (squash, then a damped follow-through in the tail and
//! head), `happy` (wiggle and hearts) and `sleep` (lying down, eyes shut,
//! static).
//!
//! What it does, as in dry: it runs under the pointer; a pointer just
//! above its head (up to 340 px over the floor) makes it crouch and jump
//! for it; too high and it misses, more excited with every miss (higher
//! jumps, up to a limit; three tries a spot). A pointer held still in reach
//! 2.6 s is stalked, then pounced on and caught (a burst of hearts). A
//! click on it purrs hearts. Left alone 20 s it falls asleep; any move
//! wakes it. The pointer leaving through a side sends it to that edge.
//! `still` (motion 0, reduced motion): it sits and only turns its head.
//!
//! The engines' mood adds two things that do not change any of that:
//! `needs you` shows a `!` over its head, and `done` (working, then idle)
//! makes it happy, as a pat does. `working` changes nothing.
//!
//! Time: egg.js steps on every animation frame; here the same `update`
//! runs on a fixed 120 Hz tick, from the memory saved at the last input
//! change (`Cat`), so the pose at `t` is a pure function of `t` and the
//! inputs, and the same at any frame rate. As in egg.js the loop parks:
//! sitting still it stops ticking, a timer (2.5-6.5 s) wakes it for a blink
//! or a tail swish, and after 20 s alone it sleeps and nothing is drawn
//! again until an input changes. Chance is a seeded xorshift, so the timers
//! are repeatable.
//!
//! Drawing: the canvas paths of egg.js (ellipses, round-capped strokes on
//! lines and quadratic curves, the ear triangles, the eye arcs, the hearts)
//! are filled into a pixel grid in the same order, and each run of one
//! colour along a row is one rectangle, so every renderer only paints
//! rectangles. The grid is the scene's `pixel` at 60 fps and up, twice as
//! coarse below (more fps buys finer art, never different motion).

use super::{Draw, Frame, Inputs, Mood, Pointer, Scene};
use std::f64::consts::PI;

/// The sprite blocks a cat scene needs: the `z z` over a sleeping cat (the
/// text egg.js writes, as pixels) and the `!` when the engines need you.
/// The cat itself is drawn by code.
pub const SPRITES: &[(&str, usize)] = &[("sleep", 1), ("cue", 1)];
/// Keys its code draws with: fur, outline (and eyes), the far legs and
/// tail, the nose and hearts.
pub const KEYS: &[char] = &['f', 'o', 'd', 'h'];

/// Simulation steps a second (egg.js steps each animation frame; this is
/// the fixed step the frame rate never changes).
const TICK_MS: f64 = 1000.0 / 120.0;
/// Gravity, px/s².
const G: f64 = 2600.0;
/// Alone this long (ms), the next timer puts it to sleep.
const NAP_MS: f64 = 20_000.0;

/// JS `Math.sign`: 0 for 0.
fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}
/// egg.js's `cl`: never panics when the bounds cross.
fn cl(v: f64, a: f64, b: f64) -> f64 {
    if v < a {
        a
    } else if v > b {
        b
    } else {
        v
    }
}
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
/// JS `Math.round` (halves toward +∞).
fn jround(v: f64) -> f64 {
    (v + 0.5).floor()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    Sit,
    Run,
    Stalk,
    Crouch,
    Air,
    Land,
    Happy,
    Sleep,
}

/// A heart: born at `x`, `h` above the floor, rising at `vy` (slowing),
/// `t` seconds old; `k` its size, `p` its drift and wobble phase.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Heart {
    x: f64,
    h: f64,
    vy: f64,
    t: f64,
    k: f64,
    p: f64,
}

/// Every variable of egg.js, by its name there (egg.md, "Names in egg.js").
/// Times are ms on the scene clock.
#[derive(Clone, Debug, PartialEq)]
struct Sim {
    /// The window's width (`W`) and the floor (`F`, px from the top).
    w: f64,
    f: f64,
    x: f64,
    /// Height above the floor.
    y: f64,
    vx: f64,
    vy: f64,
    dir: f64,
    face: f64,
    ph: f64,
    st: St,
    t0: f64,
    cd: f64,
    bz: f64,
    bl: f64,
    sh: f64,
    lf: f64,
    nt: f64,
    /// The pointer; `None` until it is first seen (`cx=null`).
    cx: Option<f64>,
    cy: f64,
    ax: f64,
    ay: f64,
    mvd: f64,
    inp: f64,
    pk: bool,
    gh: bool,
    ex: f64,
    tr: i32,
    ca: bool,
    po: bool,
    got: bool,
    s: f64,
    c: f64,
    up: f64,
    ear: f64,
    sq: f64,
    /// The loop is running (`raf`), and its last step (`lt`).
    raf: bool,
    lt: f64,
    /// The last tick done (its time is `k * TICK_MS`).
    k: u64,
    /// The rest timer (`tm`), when it fires.
    tm: Option<f64>,
    mv: bool,
    ps: Vec<Heart>,
    rng: u32,
    reduce: bool,
}

impl Sim {
    fn new(i: &Inputs, n: f64) -> Sim {
        let mut s = Sim {
            w: 0.0,
            f: 0.0,
            x: 1e5,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            dir: 1.0,
            face: 1.0,
            ph: 0.0,
            st: St::Sit,
            t0: 0.0,
            cd: 0.0,
            bz: 0.0,
            bl: -1e4,
            sh: -1e4,
            lf: -1e4,
            nt: 0.0,
            cx: None,
            cy: 0.0,
            ax: -1e4,
            ay: 0.0,
            mvd: 0.0,
            inp: n,
            pk: false,
            gh: false,
            ex: 0.0,
            tr: 3,
            ca: false,
            po: false,
            got: false,
            s: 1.0,
            c: 0.0,
            up: 0.0,
            ear: 1.0,
            sq: 0.0,
            raf: false,
            lt: 0.0,
            k: 0,
            tm: None,
            mv: false,
            ps: Vec::new(),
            rng: 0x9e37_79b9,
            reduce: i.still,
        };
        s.size(i, n);
        s
    }

    /// `Math.random()`: a seeded xorshift32, values in [0, 1).
    fn rnd(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        f64::from(x >> 8) / f64::from(1u32 << 24)
    }

    /// `floor()` and `size()`: the window and the floor, a cat far outside
    /// put back in the middle, and the loop kicked.
    fn size(&mut self, i: &Inputs, n: f64) {
        self.w = f64::from(i.cols);
        // egg.js stands on the footer's top edge less one pixel.
        self.f = f64::from(i.floor) - 1.0;
        if self.x > self.w + 80.0 || self.x < -80.0 {
            self.x = self.w / 2.0;
        }
        self.kick(n);
    }

    fn go(&mut self, k: St, t: f64) {
        self.st = k;
        self.t0 = t;
    }
    fn spk(&mut self, h: f64, n: usize) {
        for _ in 0..n {
            let x = self.x + (self.rnd() - 0.5) * 40.0;
            let h = h + self.rnd() * 12.0;
            let vy = 50.0 + self.rnd() * 50.0;
            let k = 0.7 + self.rnd() * 0.6;
            let p = self.rnd() * 7.0;
            self.ps.push(Heart { x, h, vy, t: 0.0, k, p });
        }
    }
    fn leap(&mut self, p: bool, t: f64) {
        self.pk = false;
        self.tr -= 1;
        self.po = p;
        self.go(St::Crouch, t);
    }
    fn sit(&mut self, n: f64) {
        self.go(St::Sit, n);
        self.sh = n;
        self.bz = n + 2400.0;
    }

    /// Start the loop if it is parked: the next tick is the first after `n`.
    fn kick(&mut self, n: f64) {
        if self.raf {
            return;
        }
        self.tm = None;
        if self.x > self.w + 80.0 || self.x < -80.0 {
            self.x = self.w / 2.0;
        }
        self.lt = n;
        self.raf = true;
        self.k = (n.max(0.0) / TICK_MS).floor() as u64;
        while (self.k + 1) as f64 * TICK_MS <= n {
            self.k += 1;
        }
    }

    /// The loop stopped: a timer for a blink or a swish, or sleep.
    fn rest(&mut self) {
        self.tm = None;
        if self.reduce || self.st == St::Sleep {
            return;
        }
        let n = self.lt;
        self.tm = Some(n + 2500.0 + self.rnd() * 4000.0);
    }

    fn fire(&mut self, n: f64) {
        self.tm = None;
        if n - self.inp > NAP_MS {
            self.st = St::Sleep;
            self.sq = 0.3;
        } else if self.rnd() < 0.5 {
            self.bl = n;
            self.bz = n + 160.0;
        } else {
            self.sh = n;
            self.bz = n + 2400.0;
        }
        self.kick(n);
    }

    /// Run every tick and timer up to `until` (ms).
    fn advance(&mut self, until: f64) {
        loop {
            if self.raf {
                let n = (self.k + 1) as f64 * TICK_MS;
                if n > until {
                    break;
                }
                self.k += 1;
                self.tick(n);
            } else if let Some(due) = self.tm.filter(|d| *d <= until) {
                self.fire(due);
            } else {
                break;
            }
        }
    }

    fn tick(&mut self, n: f64) {
        let dt = cl((n - self.lt) / 1e3, 0.0, 0.05);
        self.lt = n;
        self.update(dt, n);
        let busy = dt == 0.0
            || !matches!(self.st, St::Sit | St::Sleep)
            || n < self.bz
            || !self.ps.is_empty()
            || self.mv
            || n < self.nt + 50.0;
        if self.reduce || !busy {
            self.raf = false;
            self.rest();
        }
    }

    /// egg.js's `update(dt, n)`, line by line.
    fn update(&mut self, dt: f64, n: f64) {
        let hv = self.cx.is_some();
        let cx = self.cx.unwrap_or(0.0);
        let q = if hv { cx - self.x } else { 0.0 };
        let dx = if hv { cl(cx, 24.0, self.w - 24.0) - self.x } else { 0.0 };
        let ad = dx.abs();
        let h = if hv { self.f - self.cy } else { 0.0 };
        let rc = 70.0 + self.ex * 40.0;
        let nd = h - 44.0;
        let tu = if self.st == St::Sleep {
            0.35
        } else if hv && self.st != St::Run {
            cl(-(self.f - self.cy - self.y - 34.0).atan2(q.abs() + 8.0) * 0.55, -0.7, 0.2)
        } else {
            0.0
        };
        if q.abs() > 8.0 && !matches!(self.st, St::Run | St::Air) {
            self.dir = sign(q);
        }
        if self.reduce {
            self.face = self.dir;
            self.up = tu;
            return;
        }
        if matches!(self.st, St::Sit | St::Stalk) {
            self.st = St::Sit;
            if hv && ad >= 16.0 {
                if n > self.nt {
                    self.go(St::Run, n);
                    if ad > 120.0 {
                        self.ex = 0.0;
                    }
                }
            } else if hv && h > 50.0 && h < 340.0 && (self.pk || !self.gh) {
                if nd > rc {
                    if (self.tr > 0 || self.pk) && n > self.cd {
                        self.leap(false, n);
                    }
                } else if self.pk && n > self.cd {
                    self.leap(false, n);
                } else if !self.ca && !self.gh {
                    self.st = St::Stalk;
                    if n - self.mvd > 2600.0 {
                        self.leap(true, n);
                    }
                }
            }
        }
        match self.st {
            St::Run => {
                let a = 1500.0 * dt;
                self.vx += cl(sign(dx) * (30.0 + ad * 2.6).min(480.0) - self.vx, -a, a);
                self.x += self.vx * dt;
                if self.vx.abs() > 5.0 {
                    self.dir = sign(self.vx);
                }
                self.ph += dt * self.vx.abs() * 1.57 / (3.0 + self.vx.abs() / 60.0).min(8.0);
                if ad < 2.5 && self.vx.abs() < 60.0 {
                    self.vx = 0.0;
                    self.sit(n);
                }
            }
            St::Crouch if n - self.t0 > if self.po { 120.0 } else { 230.0 } => {
                self.got = nd <= rc;
                self.vy = (2.0 * G * (cl(nd, 14.0, rc) + 4.0)).sqrt();
                self.vx = cl(dx * G / self.vy * if self.po { 1.0 } else { 0.85 }, -320.0, 320.0);
                self.go(St::Air, n);
            }
            St::Air => {
                let was = self.vy;
                self.vy -= G * dt;
                self.y += self.vy * dt;
                self.x = cl(self.x + self.vx * dt, -60.0, self.w + 60.0);
                if was > 0.0 && self.vy <= 0.0 && self.got && self.po {
                    self.spk(self.y + 46.0, 8);
                    self.ca = true;
                }
                if self.y <= 0.0 {
                    self.y = 0.0;
                    self.vx = 0.0;
                    if !self.got {
                        self.ex = (self.ex + 1.0).min(3.0);
                    }
                    self.go(St::Land, n);
                    self.cd = n + 350.0;
                    self.lf = n;
                    self.bz = self.bz.max(n + 900.0);
                }
            }
            St::Land if n - self.t0 > 170.0 => {
                if self.po && self.got {
                    self.go(St::Happy, n);
                } else {
                    self.sit(n);
                }
            }
            St::Happy => {
                if (n - self.t0) % 240.0 < dt * 1e3 && n - self.t0 < 1e3 {
                    self.spk(46.0, 1);
                }
                if n - self.t0 > 1500.0 {
                    self.sit(n);
                }
            }
            _ => {}
        }
        for p in &mut self.ps {
            p.t += dt;
            p.h += p.vy * dt;
            p.vy *= 1.0 - dt * 1.2;
        }
        self.ps.retain(|p| p.t <= 1.4);
        let k = 1.0 - (-dt * 14.0).exp();
        let o = [self.s, self.c, self.up, self.ear, self.sq, self.face];
        let cr = matches!(self.st, St::Crouch | St::Stalk | St::Sleep);
        let sitting = matches!(self.st, St::Sit | St::Happy);
        self.s += (f64::from(u8::from(sitting)) - self.s) * k;
        self.c += (f64::from(u8::from(cr)) - self.c) * k;
        self.up += (tu - self.up) * k;
        let ear = if self.st == St::Air {
            0.0
        } else if n - self.inp < 2500.0 || self.st != St::Sit {
            1.0
        } else {
            0.3
        };
        self.ear += (ear - self.ear) * k;
        let sq = if cr {
            0.28
        } else if self.st == St::Land {
            0.5
        } else if self.st == St::Air {
            if self.vy > 0.0 { -0.1 } else { -0.03 }
        } else {
            0.0
        };
        self.sq += (sq - self.sq) * (1.0 - (-dt * 30.0).exp());
        self.face = cl(self.face + sign(self.dir - self.face) * dt * 10.0, -1.0, 1.0);
        let now = [self.s, self.c, self.up, self.ear, self.sq, self.face];
        self.mv = now.iter().zip(o).any(|(v, w)| (v - w).abs() > 0.002);
    }

    /// The pointer moved to (px, py) (`aim`).
    fn aim(&mut self, px: f64, py: f64, n: f64) {
        // `px-cx` with cx null is px in JS.
        if (px - self.cx.unwrap_or(0.0)).abs() + (py - self.cy).abs() < 2.0 {
            return;
        }
        if n - self.inp > 1500.0 || self.st == St::Sleep {
            self.nt = n + 260.0;
        }
        if (px - self.ax).hypot(py - self.ay) > 24.0 {
            self.ax = px;
            self.ay = py;
            self.tr = 3;
            self.ca = false;
            self.pk = self.st != St::Sleep;
        }
        self.cx = Some(px);
        self.cy = py;
        self.mvd = n;
        self.inp = n;
        self.gh = false;
        if self.st == St::Sleep {
            self.st = St::Sit;
            self.sq = -0.2;
        }
        self.kick(n);
    }

    fn near(&self, px: f64, py: f64) -> bool {
        (px - self.x).abs() < 40.0 && py > self.f - self.y - 60.0 && py < self.f + 10.0
    }

    /// A pat (`pet`): happy, if it is sitting, stalking or asleep.
    fn pet(&mut self, n: f64) {
        self.inp = n;
        if !self.reduce && matches!(self.st, St::Sit | St::Stalk | St::Sleep) {
            self.go(St::Happy, n);
            self.kick(n);
        }
    }

    /// The pointer as the shell reports it changed from `before` to `p`.
    fn pointer(&mut self, before: Option<Pointer>, p: Pointer, n: f64) {
        let (px, py) = (f64::from(p.x), f64::from(p.y));
        let moved = before.is_none_or(|q| (q.x, q.y) != (p.x, p.y));
        let pressed = p.clicks != before.map_or(0, |q| q.clicks);
        let left = before.is_some_and(|q| q.inside) && !p.inside;
        // pointermove
        if p.inside && moved {
            self.aim(px, py, n);
        }
        // pointerdown: on the cat a pat, else an aim.
        if pressed {
            if self.near(px, py) {
                self.pet(n);
            } else {
                self.aim(px, py, n);
            }
        }
        // mouseout through a side: the cat runs to that edge.
        if left && !(px > 40.0 && px < self.w - 40.0) {
            let to = if px < self.w / 2.0 { -1.0 } else { self.w + 1.0 };
            self.aim(to, py, n);
        }
    }

    /// Motion turned off (or on). egg.js reads reduced motion once; here it
    /// can change, so the cat sits down where it is.
    fn set_reduce(&mut self, on: bool, n: f64) {
        self.reduce = on;
        if on {
            (self.y, self.vx, self.vy, self.st, self.s, self.c, self.sq) = (0.0, 0.0, 0.0, St::Sit, 1.0, 0.0, 0.0);
            self.ps.clear();
            self.raf = false;
            self.tm = None;
        }
        self.kick(n);
    }
}

// ---------- drawing ----------

/// A 2D affine transform, as a canvas holds it: screen = (a x + c y + e,
/// b x + d y + f).
#[derive(Clone, Copy, Debug)]
struct Af {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Af {
    fn at(x: f64, y: f64) -> Af {
        Af { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }
    fn translate(self, x: f64, y: f64) -> Af {
        Af { e: self.e + self.a * x + self.c * y, f: self.f + self.b * x + self.d * y, ..self }
    }
    fn scale(self, x: f64, y: f64) -> Af {
        Af { a: self.a * x, b: self.b * x, c: self.c * y, d: self.d * y, ..self }
    }
    fn rotate(self, th: f64) -> Af {
        let (s, c) = th.sin_cos();
        Af { a: self.a * c + self.c * s, b: self.b * c + self.d * s, c: -self.a * s + self.c * c, d: -self.b * s + self.d * c, ..self }
    }
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
    /// Screen to local; `None` when the transform is flat (a turn passing
    /// through face 0 draws nothing, as on a canvas).
    fn inverse(&self) -> Option<impl Fn(f64, f64) -> (f64, f64)> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-9 {
            return None;
        }
        let m = *self;
        Some(move |x: f64, y: f64| {
            let (x, y) = (x - m.e, y - m.f);
            ((m.d * x - m.c * y) / det, (-m.b * x + m.a * y) / det)
        })
    }
}

/// A filled canvas path, in local coordinates.
enum Shape {
    Ellipse(f64, f64, f64, f64),
    /// A round-capped, round-joined stroke along points, radius r.
    Stroke(Vec<(f64, f64)>, f64),
    Poly(Vec<(f64, f64)>),
}

impl Shape {
    fn bounds(&self) -> (f64, f64, f64, f64) {
        let pts = |p: &[(f64, f64)], r: f64| {
            p.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |(a, b, c, d), &(x, y)| (a.min(x - r), b.min(y - r), c.max(x + r), d.max(y + r)))
        };
        match self {
            Shape::Ellipse(u, v, rx, ry) => (u - rx, v - ry, u + rx, v + ry),
            Shape::Stroke(p, r) => pts(p, *r),
            Shape::Poly(p) => pts(p, 0.0),
        }
    }

    fn inside(&self, x: f64, y: f64) -> bool {
        match self {
            Shape::Ellipse(u, v, rx, ry) => {
                let (a, b) = ((x - u) / rx, (y - v) / ry);
                a * a + b * b <= 1.0
            }
            Shape::Stroke(p, r) => {
                let r2 = r * r;
                if p.len() == 1 {
                    return (x - p[0].0).powi(2) + (y - p[0].1).powi(2) <= r2;
                }
                p.windows(2).any(|s| {
                    let ((x1, y1), (x2, y2)) = (s[0], s[1]);
                    let (dx, dy) = (x2 - x1, y2 - y1);
                    let l2 = dx * dx + dy * dy;
                    let t = if l2 > 0.0 { cl(((x - x1) * dx + (y - y1) * dy) / l2, 0.0, 1.0) } else { 0.0 };
                    (x - x1 - t * dx).powi(2) + (y - y1 - t * dy).powi(2) <= r2
                })
            }
            Shape::Poly(p) => {
                let mut inside = false;
                let mut j = p.len() - 1;
                for i in 0..p.len() {
                    let ((xi, yi), (xj, yj)) = (p[i], p[j]);
                    if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
        }
    }
}

fn quad(x1: f64, y1: f64, qx: f64, qy: f64, x2: f64, y2: f64) -> Vec<(f64, f64)> {
    (0..=16)
        .map(|i| {
            let t = f64::from(i) / 16.0;
            let u = 1.0 - t;
            (u * u * x1 + 2.0 * u * t * qx + t * t * x2, u * u * y1 + 2.0 * u * t * qy + t * t * y2)
        })
        .collect()
}

/// A canvas `arc` (clockwise from `a0` to `a1`) as points.
fn arc(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Vec<(f64, f64)> {
    let a1 = if a1 < a0 { a1 + 2.0 * PI } else { a1 };
    (0..=12).map(|i| a0 + (a1 - a0) * f64::from(i) / 12.0).map(|a| (cx + r * a.cos(), cy + r * a.sin())).collect()
}

/// The pixel grid the paths fill, `p` px a pixel, top-left at (x0, y0).
struct Grid {
    p: f64,
    x0: f64,
    y0: f64,
    w: usize,
    h: usize,
    keys: Vec<u8>,
}

impl Grid {
    /// Covering the screen box (x0, y0)-(x1, y1), on the `p` grid.
    fn new(p: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Grid {
        let (gx, gy) = ((x0 / p).floor() * p, (y0 / p).floor() * p);
        let w = (((x1 - gx) / p).ceil().max(0.0) as usize).min(4096);
        let h = (((y1 - gy) / p).ceil().max(0.0) as usize).min(4096);
        Grid { p, x0: gx, y0: gy, w, h, keys: vec![0; w * h] }
    }

    /// Fill `shape` (local, under `m`) with `key`: each pixel at least a
    /// third covered (3x3 samples), so thin strokes (the eyes' 1.3 px
    /// lines, a blink) stay. A stroke is tested one segment at a time, each
    /// over its own box only.
    fn fill(&mut self, m: &Af, shape: &Shape, key: char) {
        let Some(inv) = m.inverse() else { return };
        let boxed = |x0: f64, y0: f64, x1: f64, y1: f64| {
            let c = [m.apply(x0, y0), m.apply(x1, y0), m.apply(x0, y1), m.apply(x1, y1)];
            c.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
        };
        let (bx0, by0, bx1, by1) = shape.bounds();
        let (sx0, sy0, sx1, sy1) = boxed(bx0, by0, bx1, by1);
        // Samples: 3 a pixel each way, over the shape's box on the grid.
        let to = |v: f64, o: f64, n: usize| cl(((v - o) / self.p * 3.0).floor(), 0.0, (n * 3) as f64) as usize;
        let (i0, i1) = (to(sx0, self.x0, self.w), (to(sx1, self.x0, self.w) + 1).min(self.w * 3));
        let (j0, j1) = (to(sy0, self.y0, self.h), (to(sy1, self.y0, self.h) + 1).min(self.h * 3));
        if i0 >= i1 || j0 >= j1 {
            return;
        }
        let (mw, mh) = (i1 - i0, j1 - j0);
        let mut mask = vec![false; mw * mh];
        let step = self.p / 3.0;
        let mut mark = |(x0, y0, x1, y1): (f64, f64, f64, f64), inside: &dyn Fn(f64, f64) -> bool| {
            let (a0, a1) = (to(x0, self.x0, self.w).max(i0), (to(x1, self.x0, self.w) + 1).min(i1));
            let (b0, b1) = (to(y0, self.y0, self.h).max(j0), (to(y1, self.y0, self.h) + 1).min(j1));
            for j in b0..b1 {
                for i in a0..a1 {
                    let at = (j - j0) * mw + (i - i0);
                    if !mask[at] {
                        let (lx, ly) = inv(self.x0 + (i as f64 + 0.5) * step, self.y0 + (j as f64 + 0.5) * step);
                        mask[at] = inside(lx, ly);
                    }
                }
            }
        };
        match shape {
            Shape::Stroke(pts, r) if pts.len() > 1 => {
                for s in pts.windows(2) {
                    let one = Shape::Stroke(s.to_vec(), *r);
                    let (x0, y0, x1, y1) = one.bounds();
                    mark(boxed(x0, y0, x1, y1), &|x, y| one.inside(x, y));
                }
            }
            _ => mark((sx0, sy0, sx1, sy1), &|x, y| shape.inside(x, y)),
        }
        // A pixel is filled when 3 of its 9 samples are.
        for pj in j0 / 3..j1.div_ceil(3) {
            for pi in i0 / 3..i1.div_ceil(3) {
                let mut n = 0;
                for j in (pj * 3).max(j0)..(pj * 3 + 3).min(j1) {
                    for i in (pi * 3).max(i0)..(pi * 3 + 3).min(i1) {
                        n += u8::from(mask[(j - j0) * mw + (i - i0)]);
                    }
                }
                if n >= 3 && pi < self.w && pj < self.h {
                    self.keys[pj * self.w + pi] = key as u8;
                }
            }
        }
    }

    /// Each run of one key along a row as one rectangle, in cells (`cell`
    /// px each).
    fn runs(&self, cell: f64, alpha: f32, out: &mut Vec<Draw>) {
        for j in 0..self.h {
            let row = &self.keys[j * self.w..(j + 1) * self.w];
            let mut i = 0;
            while i < self.w {
                let k = row[i];
                let start = i;
                while i < self.w && row[i] == k {
                    i += 1;
                }
                if k != 0 {
                    let (x, y) = (self.x0 + start as f64 * self.p, self.y0 + j as f64 * self.p);
                    let (w, h) = ((i - start) as f64 * self.p, self.p);
                    out.push(Draw::cell((x / cell) as f32, (y / cell) as f32, (w / cell) as f32, (h / cell) as f32, k as char, alpha));
                }
            }
        }
    }
}

/// egg.js's drawing pass: 0 the fattened outline, 1 the fill.
struct Pen<'g> {
    g: &'g mut Grid,
    fill: bool,
}

impl Pen<'_> {
    /// `ink`: a stroke `w` wide (4 wider, in the outline colour, on the
    /// outline pass).
    fn ink(&mut self, m: &Af, pts: Vec<(f64, f64)>, w: f64, col: char) {
        let (w, col) = if self.fill { (w, col) } else { (w + 4.0, 'o') };
        if w > 0.0 {
            self.g.fill(m, &Shape::Stroke(pts, w / 2.0), col);
        }
    }
    fn seg(&mut self, m: &Af, (x1, y1): (f64, f64), (x2, y2): (f64, f64), w: f64, col: char, q: Option<(f64, f64)>) {
        let pts = match q {
            None => vec![(x1, y1), (x2, y2)],
            Some((qx, qy)) => quad(x1, y1, qx, qy, x2, y2),
        };
        self.ink(m, pts, w, col);
    }
    fn ell(&mut self, m: &Af, u: f64, v: f64, rx: f64, ry: f64, col: char) {
        let (e, col) = if self.fill { (0.0, col) } else { (2.0, 'o') };
        self.g.fill(m, &Shape::Ellipse(u, v, rx + e, ry + e), col);
    }
    /// `earAt`: a triangle at angle `th` on the head, folded back as the
    /// ears drop.
    fn ear(&mut self, m: &Af, th: f64, ear: f64, col: char) {
        let t = th - (1.0 - ear) * 0.5;
        let e = |a: f64, r: f64| (a.sin() * r, -a.cos() * r);
        let pts = vec![e(t - 0.45, 8.0), e(t + 0.08 * ear, 15.0 + ear * 2.5), e(t + 0.4, 8.0)];
        self.g.fill(m, &Shape::Poly(pts.clone()), if self.fill { col } else { 'o' });
        if !self.fill {
            let mut ring = pts.clone();
            ring.push(pts[0]);
            self.ink(m, ring, 0.0, 'o');
        }
    }
}

impl Sim {
    /// egg.js's `eyes(n)`, in head coordinates.
    fn eyes(&self, g: &mut Grid, m: &Af, n: f64) {
        for e in [2.5, 8.0] {
            match self.st {
                St::Happy => g.fill(m, &Shape::Stroke(arc(e, 0.0, 2.0, 3.4, 6.0), 0.65), 'o'),
                St::Sleep => g.fill(m, &Shape::Stroke(arc(e, -1.0, 2.0, 0.3, 2.8), 0.65), 'o'),
                _ => {
                    g.fill(m, &Shape::Ellipse(e, -1.0, 1.9, if n - self.bl < 150.0 { 0.4 } else { 2.6 }), 'o');
                    g.fill(m, &Shape::Ellipse(e + 0.6, if self.up < -0.2 { -2.2 } else { -1.4 }, 1.0, 1.0), 'f');
                }
            }
        }
        g.fill(m, &Shape::Ellipse(10.8, 2.0, 1.3, 1.3), 'h');
    }

    /// egg.js's `cat(n)`: both passes, under `m` (the cat's own transform).
    fn body(&self, g: &mut Grid, m: &Af, n: f64) {
        let (s, c, vx, vy) = (self.s, self.c, self.vx, self.vy);
        let run = self.st == St::Run;
        let air = self.st == St::Air;
        let stk = self.st == St::Stalk;
        let sl = self.st == St::Sleep;
        let sp = vx.abs();
        let a = (3.0 + sp / 60.0).min(8.0);
        // In the air: fl falling (0-1), tk tucked (1 at the top of the arc).
        let fl = if air && vy < 0.0 { cl(-vy / 900.0, 0.0, 1.0) } else { 0.0 };
        let tk = if air { cl(1.0 - vy.abs() / 650.0, 0.0, 1.0) } else { 0.0 };
        let hx = lerp(-12.0, -7.0, s) - c * 2.0;
        let mut hy = lerp(-17.0, -9.0, s) + c * 6.0;
        let sx = lerp(11.0, 5.0, s);
        let mut sy = lerp(-18.0, -27.0, s) + c * 9.0;
        let p = if air { cl(-vy / 1e3, -0.6, 0.4) } else { 0.0 };
        if stk {
            hy -= 3.0 + (n / 55.0).sin() * 1.5;
        }
        if run {
            let b = self.ph.sin().abs() * sp / 150.0;
            hy -= b;
            sy -= b;
        }
        hy -= p * 11.0;
        sy += p * 11.0;
        // Landing follow-through: a damped bounce through the tail and head.
        let fa = n - self.lf;
        let ft = if fa < 900.0 { (-fa / 260.0).exp() * (fa / 70.0).sin() } else { 0.0 };
        let ph = self.ph;
        let foot = |bx: f64, i: f64| {
            if run { (bx - a * (ph + i * PI).cos(), -(3.0 + sp / 90.0) * (ph + i * PI).sin().max(0.0)) } else { (bx, 0.0) }
        };
        let (mut ff, mut fnn, mut bf, mut bn) = (foot(sx + 1.0, 0.0), foot(sx + 4.0, 1.0), foot(hx - 2.0, 1.0), foot(hx + 1.0, 0.0));
        if air {
            let look = -(self.f - self.cy - self.y - 34.0);
            let look = if look == 0.0 || look.is_nan() { -1.0 } else { look };
            let r = lerp(cl(look.atan2((self.cx.unwrap_or(0.0) - self.x).abs()), -1.5, -0.6), 0.9, fl);
            ff = (sx + 1.0 + (r + 0.3).cos() * 22.0, sy + (r + 0.3).sin() * 22.0);
            fnn = (sx + 3.0 + r.cos() * 24.0, sy + r.sin() * 24.0);
            bf = (lerp(lerp(hx - 10.0, hx - 2.0, fl), hx + 6.0, tk * 0.8), lerp(lerp(hy + 11.0, 0.0, fl), hy + 6.0, tk * 0.8));
            bn = (bf.0 + 3.0, bf.1);
        }
        let (airf, stkf, slf) = (f64::from(u8::from(air)), f64::from(u8::from(stk)), f64::from(u8::from(sl)));
        let hdx = sx + lerp(7.0, 4.0, s) + c * 3.0 + airf * 6.0;
        let hdy = sy + lerp(-9.0, -10.0, s) + c * 4.0 + airf * 3.0 + slf * 3.0;
        let swish = n - self.sh;
        let sw = if swish < 2400.0 { (swish / 2400.0 * PI).sin() } else { 0.0 } + stkf * 0.5;
        let tw = (n / if stk { 90.0 } else { 260.0 }).sin() * sw;
        // Spine: arched over the jump (most at the top), a little in a crouch.
        let ar = if air { 5.0 + 9.0 * tk } else { c * 3.0 };
        let (mx, my) = ((hx + sx) / 2.0, (hy + sy) / 2.0 - ar);
        let head = m.translate(hdx, hdy - ft * 1.5).rotate(self.up);
        for pass in [false, true] {
            let mut pen = Pen { g: &mut *g, fill: pass };
            let leg = |pen: &mut Pen, tx: f64, ty: f64, f: (f64, f64), hind: bool, col: char| {
                let q = hind.then(|| (tx - 7.0 - c * 4.0 - tk * 3.0, (ty + f.1) / 2.0 + 2.0 - tk * 3.0));
                pen.seg(m, (tx, ty), f, 5.0, col, q);
                if air && !hind {
                    pen.ell(m, f.0, f.1, 3.5, 3.5, col);
                }
            };
            if !air {
                leg(&mut pen, sx, sy + 4.0, ff, false, 'd');
            }
            if s < 0.5 {
                leg(&mut pen, hx, hy + 3.0, bf, true, 'd');
            }
            let te = lerp(hx - 18.0, hx - 1.0, s) + tw * 7.0 - slf * 4.0 + if air { -vx / 60.0 } else { 0.0 };
            let tv = if sl { -2.0 } else { lerp(if run || air { hy - 9.0 - p * 6.0 } else { hy - 20.0 }, -2.0 - sw * 4.0, s) + tw * 3.0 - ft * 6.0 };
            pen.seg(m, (hx - 7.0, hy - 1.0), (te, tv), 4.5, 'd', Some((lerp(hx - 18.0, hx - 20.0, s) - tw * 3.0, lerp(hy - 4.0, 2.0, s) + ft * 3.0)));
            pen.ear(&head, -0.55, self.ear, 'd');
            pen.seg(m, (hx, hy), (sx, sy), 17.0 - c * 1.5, 'f', Some((mx, my)));
            if s >= 0.5 || c > 0.5 {
                pen.ell(m, hx, hy - 1.0, 10.0, 8.5, 'f');
                pen.seg(m, (hx + 2.0, -2.5), (hx + 10.0, -2.5), 5.0, 'f', None);
            } else {
                leg(&mut pen, hx, hy + 3.0, bn, true, 'f');
            }
            if air {
                leg(&mut pen, sx + 1.0, sy, ff, false, 'd');
                leg(&mut pen, sx + 3.0, sy, fnn, false, 'f');
            } else {
                leg(&mut pen, sx + 2.0, sy + 4.0, fnn, false, 'f');
            }
            pen.ell(&head, 0.0, 0.0, 10.5, 9.5, 'f');
            pen.ell(&head, 5.5, 3.0, 5.5, 4.2, 'f');
            pen.ear(&head, 0.3, self.ear, 'f');
            if pass {
                self.eyes(pen.g, &head, n);
            }
        }
    }
}

/// The cat's memory: egg.js's variables as they were at the last input
/// change, and the inputs then. The pose at a later `t` is found by
/// running the ticks from here.
#[derive(Clone, Debug, PartialEq)]
pub struct Cat {
    sim: Sim,
    mood: Mood,
}

impl Cat {
    pub fn start(i: &Inputs, t: f64) -> Cat {
        Cat { sim: Sim::new(i, t * 1e3), mood: i.mood }
    }

    /// The inputs went from `old` to `new` at `t`: the ticks up to then,
    /// then what egg.js does with each change (a resize, motion, the
    /// mood, the pointer).
    pub fn step(&self, old: &Inputs, new: &Inputs, t: f64) -> Cat {
        let n = t * 1e3;
        let mut c = self.clone();
        c.sim.advance(n);
        if (new.cols, new.floor) != (old.cols, old.floor) {
            c.sim.size(new, n);
        }
        if new.still != old.still {
            c.sim.set_reduce(new.still, n);
        }
        if new.mood != old.mood {
            // Done (working, then idle): as happy as a pat makes it.
            if old.mood == Mood::Working && new.mood == Mood::Idle {
                c.sim.pet(n);
            }
            c.mood = new.mood;
        }
        if let Some(p) = new.pointer
            && new.pointer != old.pointer
        {
            c.sim.pointer(old.pointer, p, n);
        }
        c
    }

    pub fn frame(&self, scene: &Scene, _inputs: &Inputs, t: f64, level: u8) -> Frame {
        let n = t * 1e3;
        let mut sim = self.sim.clone();
        sim.advance(n);
        let cell = f64::from(scene.cell);
        // The grid: the scene's pixel at 60 fps, twice as coarse below.
        let p = f64::from(scene.pixel) * if level >= 3 { 1.0 } else { 2.0 };
        let (ox, oy) = (jround(sim.x), jround(sim.f - sim.y));
        let mut draws = Vec::new();
        let mut anchors = vec![(sim.x as f32, (sim.f - sim.y) as f32)];
        // Hearts: pop in, drift and wobble as they rise, fade.
        for hp in &sim.ps {
            let a = hp.t;
            let z = hp.k * if a < 0.12 { a / 0.12 * 1.3 } else if a < 0.24 { lerp(1.3, 1.0, (a - 0.12) / 0.12) } else { 1.0 };
            let (hx, hy) = (ox + hp.x - sim.x + (a * 3.0 + hp.p).sin() * 7.0 * a, oy + sim.y - hp.h);
            let m = Af::at(hx, hy).rotate((a * 6.0 + hp.p).sin() * 0.3).scale(z, z);
            let mut heart = arc(-2.5, 0.0, 2.5, PI, 0.0);
            heart.extend(arc(2.5, 0.0, 2.5, PI, 0.0));
            heart.push((0.0, 6.0));
            let mut g = Grid::new(p, hx - 12.0, hy - 12.0, hx + 12.0, hy + 12.0);
            g.fill(&m, &Shape::Poly(heart), 'h');
            g.runs(cell, cl(1.6 - a * 1.2, 0.0, 1.0) as f32, &mut draws);
            anchors.push((hp.x as f32, (sim.f - hp.h) as f32));
        }
        let art = scene.art();
        if sim.st == St::Sleep {
            // `z z`, on the text's baseline.
            let z = scene.sprite("sleep", 0);
            let (zx, zy) = (ox + 18.0 * sim.face - 8.0, oy - 30.0 - f64::from(z.h) * f64::from(scene.pixel));
            z.draw_scaled((zx / cell) as f32, (zy / cell) as f32, art, false, 1.0, |k| k, &mut draws);
        }
        let mut m = Af::at(ox, oy).scale(sim.face * (1.0 + sim.sq * 0.15), 1.0 - sim.sq * 0.2);
        if sim.st == St::Happy {
            m = m.rotate((n / 70.0).sin() * 0.07);
        }
        let mut g = Grid::new(p, ox - 80.0, oy - 100.0, ox + 80.0, oy + 30.0);
        sim.body(&mut g, &m, n);
        g.runs(cell, 1.0, &mut draws);
        if self.mood == Mood::NeedsYou {
            let cue = scene.sprite("cue", 0);
            let (cw, ch) = (f64::from(cue.w) * f64::from(scene.pixel), f64::from(cue.h) * f64::from(scene.pixel));
            let (hx, hy) = m.apply(lerp(11.0, 5.0, sim.s) + 5.0, lerp(-18.0, -27.0, sim.s) - 28.0);
            cue.draw_scaled(((hx - cw / 2.0) / cell) as f32, ((hy - ch) / cell) as f32, art, false, 1.0, |k| k, &mut draws);
        }
        let next = if sim.raf { Some(t) } else { sim.tm.map(|d| d / 1e3) };
        Frame { draws, anchors, next }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::shipped;
    use super::super::{Inputs, Live, Mood, Pointer, Scene};
    use super::super::{frame, start, step};

    /// A wide window: 1000 px, the floor at 601 (so the cat stands at 600).
    fn wide(mood: Mood) -> Inputs {
        Inputs { mood, cols: 1000.0, rows: 700.0, floor: 601.0, still: false, pointer: None }
    }
    const F: f32 = 600.0;

    fn point(live: &Live, x: f32, y: f32, t: f64) -> Live {
        let p = Pointer { x, y, inside: true, ..live.inputs.pointer.unwrap_or_default() };
        step(live, Inputs { pointer: Some(p), ..live.inputs }, t)
    }
    fn click(live: &Live, t: f64) -> Live {
        let p = live.inputs.pointer.expect("a pointer first");
        step(live, Inputs { pointer: Some(Pointer { pressed: true, clicks: p.clicks + 1, ..p }), ..live.inputs }, t)
    }
    fn x(s: &Scene, live: &Live, t: f64) -> f32 {
        frame(s, live, t, 60.0).anchors[0].0
    }
    fn lift(s: &Scene, live: &Live, t: f64) -> f32 {
        F - frame(s, live, t, 60.0).anchors[0].1
    }
    fn hearts(s: &Scene, live: &Live, t: f64) -> usize {
        frame(s, live, t, 60.0).anchors.len() - 1
    }
    /// Every tick from `a` to `b`, sampled between ticks.
    fn over(a: f64, b: f64) -> impl Iterator<Item = f64> {
        (0..).map(move |n| a + (n as f64 + 0.5) / 120.0).take_while(move |t| *t <= b)
    }
    fn peaks(s: &Scene, live: &Live, a: f64, b: f64) -> Vec<f32> {
        let mut l: Vec<f32> = over(a, b).map(|t| lift(s, live, t)).collect();
        l.dedup();
        l.windows(3).filter(|w| w[1] > 1.0 && w[1] >= w[0] && w[1] > w[2]).map(|w| w[1]).collect()
    }

    #[test]
    fn it_starts_in_the_middle_sitting_and_the_loop_parks() {
        let s = shipped("cat");
        let live = start(wide(Mood::Idle), 0.0);
        let f = frame(&s, &live, 1.0, 60.0);
        assert_eq!(f.anchors[0], (500.0, F), "egg.js puts it in the middle, on the floor");
        let wake = f.next.expect("a timer for a blink or a swish");
        assert!((2.5..=6.6).contains(&wake), "parked until the timer: {wake}");
        assert!(f.draws.iter().any(|d| d.key == 'o') && f.draws.iter().any(|d| d.key == 'f') && f.draws.iter().any(|d| d.key == 'h'));
    }

    #[test]
    fn it_runs_to_the_pointer_eased_up_to_480_px_a_second_then_sits() {
        let s = shipped("cat");
        let live = start(wide(Mood::Idle), 0.0);
        // Quiet 1.5 s and more: a 260 ms beat to notice.
        let moved = point(&live, 900.0, 100.0, 3.0);
        assert_eq!(frame(&s, &moved, 3.0, 60.0).next, Some(3.0), "the loop wakes the moment it moves");
        assert_eq!(x(&s, &moved, 3.25), 500.0, "a beat to notice first");
        let xs: Vec<f32> = over(3.0, 5.0).map(|t| x(&s, &moved, t)).collect();
        let speeds: Vec<f32> = xs.windows(2).map(|w| (w[1] - w[0]) * 120.0).collect();
        let top = speeds.iter().copied().fold(0.0, f32::max);
        assert!(top > 400.0 && top <= 481.0, "up to 480 px/s: {top}");
        assert!(speeds.windows(2).any(|w| w[1] > w[0] + 1.0), "it speeds up (1500 px/s²)");
        assert!((x(&s, &moved, 5.5) - 900.0).abs() < 2.5, "and stops under it: {}", x(&s, &moved, 5.5));
        assert_ne!(frame(&s, &moved, 8.0, 60.0).next, Some(8.0), "then sits: parked");
        // The pointer is kept 24 px from the sides.
        let edge = point(&live, 3.0, 100.0, 0.5);
        assert!((x(&s, &edge, 3.0) - 24.0).abs() < 2.5);
    }

    #[test]
    fn a_pointer_over_its_head_is_jumped_for_out_of_reach_it_misses_higher_each_time() {
        let s = shipped("cat");
        let live = start(wide(Mood::Idle), 0.0);
        // In reach: 100 px over the floor, nd = 100 - 44 = 56; the arc tops
        // out at nd + 4.
        let near = point(&live, 500.0, F - 100.0, 0.5);
        assert_eq!(lift(&s, &near, 0.7), 0.0, "a 230 ms crouch first");
        let p = peaks(&s, &near, 0.5, 2.0);
        assert!(!p.is_empty() && (p[0] - 60.0).abs() < 3.5, "up to it (less a step of the arc): {p:?}");
        // Out of reach (300 px): three tries, 70, 110, 150 px of reach (+4).
        let high = point(&live, 500.0, F - 300.0, 0.5);
        let p = peaks(&s, &high, 0.5, 8.0);
        assert_eq!(p.len(), 3, "three tries: {p:?}");
        for (got, want) in p.iter().zip([74.0, 114.0, 154.0]) {
            assert!(*got <= want && *got > want - 5.0, "each higher (less a step of the arc): {p:?}");
        }
    }

    #[test]
    fn held_still_in_reach_it_stalks_then_pounces_and_catches_it() {
        let s = shipped("cat");
        let live = point(&start(wide(Mood::Idle), 0.0), 500.0, F - 100.0, 0.5);
        // The first jump (a fresh aim), then on the ground it stalks: low, wiggling.
        assert!(over(1.6, 3.0).all(|t| lift(&s, &live, t) == 0.0));
        assert_ne!(frame(&s, &live, 2.0, 60.0).draws, frame(&s, &live, 2.05, 60.0).draws, "wiggling");
        // 2.6 s after the pointer stopped: the pounce, and hearts at its top.
        let p = peaks(&s, &live, 3.0, 4.5);
        assert_eq!(p.len(), 1, "one pounce: {p:?}");
        assert!(over(3.0, 4.5).any(|t| hearts(&s, &live, t) >= 8), "caught: a burst of hearts");
        assert!(peaks(&s, &live, 4.5, 15.0).is_empty(), "caught: it does not pounce again");
    }

    #[test]
    fn a_click_on_it_purrs_hearts_elsewhere_it_is_an_aim() {
        let s = shipped("cat");
        let on = point(&start(wide(Mood::Idle), 0.0), 500.0, F - 20.0, 0.2);
        let pat = click(&on, 1.0);
        assert!(over(1.0, 1.3).any(|t| hearts(&s, &pat, t) > 0), "hearts at once");
        assert!(hearts(&s, &pat, 1.8) >= 3, "more as it purrs: {}", hearts(&s, &pat, 1.8));
        assert_eq!(hearts(&s, &pat, 4.0), 0, "then they fade");
        let far = click(&point(&on, 800.0, 100.0, 0.5), 0.6);
        assert_eq!(hearts(&s, &far, 1.0), 0);
    }

    #[test]
    fn left_alone_it_sleeps_and_any_move_wakes_it() {
        let s = shipped("cat");
        let live = point(&start(wide(Mood::Idle), 0.0), 520.0, 100.0, 0.5);
        let asleep = frame(&s, &live, 40.0, 60.0);
        assert_eq!(asleep.next, None, "asleep: parked");
        assert!(asleep.draws.iter().any(|d| d.key == 'f' && d.y < F - 32.0), "the z z");
        assert_eq!(asleep, frame(&s, &live, 4000.0, 60.0), "and stays asleep");
        let woke = point(&live, 900.0, 100.0, 50.0);
        assert_eq!(frame(&s, &woke, 50.0, 60.0).next, Some(50.0), "a move anywhere wakes it");
        assert_eq!(x(&s, &woke, 50.25), x(&s, &live, 40.0), "after a beat");
        assert!((x(&s, &woke, 53.0) - 900.0).abs() < 2.5, "then it runs to it");
    }

    #[test]
    fn leaving_through_a_side_sends_it_to_that_edge() {
        let s = shipped("cat");
        let live = point(&start(wide(Mood::Idle), 0.0), 300.0, 300.0, 0.5);
        // It left fast: the last position heard is 10 px from the left side.
        let p = live.inputs.pointer.unwrap();
        let out = step(&live, Inputs { pointer: Some(Pointer { x: 10.0, inside: false, ..p }), ..live.inputs }, 0.6);
        assert!((x(&s, &out, 4.0) - 24.0).abs() < 2.5, "to the edge, 24 px in: {}", x(&s, &out, 4.0));
        assert!((x(&s, &live, 4.0) - 300.0).abs() < 2.5, "staying in, it stays under the pointer");
    }

    #[test]
    fn done_makes_it_happy_needing_you_shows_the_cue() {
        let s = shipped("cat");
        let busy = step(&start(wide(Mood::Idle), 0.0), wide(Mood::Working), 1.0);
        let done = step(&busy, wide(Mood::Idle), 2.0);
        assert!(hearts(&s, &done, 2.3) > 0, "done: hearts");
        assert_eq!(hearts(&s, &busy, 2.3), 0);
        let needs = step(&busy, wide(Mood::NeedsYou), 2.0);
        assert!(frame(&s, &needs, 3.0, 60.0).draws.iter().any(|d| d.key == 'c'), "the cue");
        assert!(!frame(&s, &busy, 3.0, 60.0).draws.iter().any(|d| d.key == 'c'));
    }

    #[test]
    fn motion_0_sits_and_only_turns_its_head() {
        let s = shipped("cat");
        let still = start(Inputs { still: true, ..wide(Mood::Idle) }, 0.0);
        let looked = point(&still, 100.0, 300.0, 1.0);
        assert_eq!(frame(&s, &looked, 2.0, 60.0).anchors, frame(&s, &still, 2.0, 60.0).anchors, "it does not move");
        assert_ne!(frame(&s, &looked, 2.0, 60.0).draws, frame(&s, &still, 2.0, 60.0).draws, "it turns to look");
        assert_eq!(frame(&s, &looked, 2.0, 60.0).next, None, "and the loop parks");
        assert_eq!(hearts(&s, &click(&looked, 3.0), 3.5), 0, "no hearts");
    }

    #[test]
    fn a_resize_keeps_a_sleeping_cat_asleep_and_brings_a_lost_one_back() {
        let s = shipped("cat");
        let live = start(wide(Mood::Idle), 0.0);
        let wider = step(&live, Inputs { cols: 1200.0, ..live.inputs }, 100.0);
        assert_eq!(frame(&s, &wider, 101.0, 60.0).next, None);
        let narrow = step(&live, Inputs { cols: 300.0, ..live.inputs }, 100.0);
        assert_eq!(x(&s, &narrow, 101.0), 150.0, "outside the window: back in the middle");
    }
}
