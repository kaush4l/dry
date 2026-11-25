//! Scenes: animated backgrounds, apart from the theme.
//!
//! Which scene is an app setting (`settings`: `app.md`'s `scene:`, which
//! a `## Scene` row of `state/local.md` or a route's overrides beats), not
//! a theme token; the scene is a file, `scenes/<name>.md`, read by
//! `dry-tools`. The file is data: which behaviour moves it (`kind`), where
//! it is drawn (`place`), how many frames a second it may draw (`fps`),
//! the size of one cell (and of one sprite pixel, which may be finer), a
//! palette giving each key its colour (`Ink`), and cell sprites. The
//! behaviours are the only code: `cat`, `terminal` and `abyss`.
//!
//! The standard every scene keeps:
//! - **Time, never frames.** Every position and phase is a pure function
//!   of `t`, seconds on the scene's clock. The same `t` gives the same
//!   pose at any frame rate.
//! - **More fps is more quality, never more speed.** `detail(fps)` is the
//!   tier a frame rate buys: in-between sprite frames, more particles,
//!   finer steps. A footer scene runs at a standard 24 fps; a scene behind
//!   the whole window declares its own.
//! - **Cells.** A frame is a list of rectangles in cell units, each with a
//!   palette key and an opacity. GPUI paints them as quads; a terminal
//!   could paint the same cells as blocks.
//! - **Its own colours.** A palette key has a light and a dark colour of
//!   its own, and may name a role, a standard theme token (`accent`, …):
//!   then the theme's value is drawn instead (`colours`, for the theme and
//!   mode on screen). So any theme shows any scene; a theme that has the
//!   role recolours it. Appearance (light/dark, `motion`) is an input.
//! - **Rest.** `Frame::next` says when the picture next changes. `None`:
//!   never, until the inputs change, so the shell parks its loop.
//!
//! Pure: no I/O and no drawing. The shell keeps a `Live` (the scene's
//! memory), calls `step` when the inputs change and `frame` to draw.

use std::collections::BTreeMap;

use crate::card::Status;
use crate::state::State;
use crate::theme::{self, COLOR_NAMES, LEGACY_COLOR_NAMES, Mode, NO_SCENE, Overrides, Theme, Tokens};

pub mod abyss;
pub mod cat;
pub mod terminal;

/// The frame rate a footer scene runs at, whatever its file says.
pub const FOOTER_FPS: f32 = 24.0;

/// The detail tier a frame rate buys: 1 below 30 fps, 2 below 60, 3 at 60
/// and up. Motion is the same at every tier; a higher one adds in-between
/// frames, particles and finer steps.
pub fn detail(fps: f32) -> u8 {
    if fps >= 60.0 {
        3
    } else if fps >= 30.0 {
        2
    } else {
        1
    }
}

/// Which behaviour moves a scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// dry's first cat (`app/eggs/pet`): on the floor, chasing and jumping
    /// for the pointer, purring, napping; a `!` when the engines need you.
    Cat,
    /// A phosphor terminal: a wandering cursor typing glowing traces, a
    /// slow scanline sweep.
    Terminal,
    /// The deep sea: jellyfish pulsing upward, rising plankton, light
    /// shafts from above.
    Abyss,
}

impl Kind {
    pub fn parse(word: &str) -> Option<Kind> {
        match word {
            "cat" => Some(Kind::Cat),
            "terminal" => Some(Kind::Terminal),
            "abyss" => Some(Kind::Abyss),
            _ => None,
        }
    }

    /// The sprite blocks it needs, with how many frames each.
    fn sprites(self) -> &'static [(&'static str, usize)] {
        match self {
            Kind::Cat => cat::SPRITES,
            Kind::Terminal => &[],
            Kind::Abyss => abyss::SPRITES,
        }
    }

    /// The palette keys its code draws with (sprites bring their own).
    fn keys(self) -> &'static [char] {
        match self {
            Kind::Cat => cat::KEYS,
            Kind::Terminal => terminal::KEYS,
            Kind::Abyss => abyss::KEYS,
        }
    }
}

/// Where a scene is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The whole window background, behind the glass panes.
    Window,
    /// The whole window, over the glass panes: a small walker the panes'
    /// frost would hide (the cat). It takes no pointer events, so every
    /// click still reaches the page under it.
    Front,
    /// The footer row only, behind its text. Always `FOOTER_FPS`.
    Footer,
}

/// One cell sprite frame: its size, and its lit cells as runs of one key
/// along a row, so one rectangle draws each run.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub w: u16,
    pub h: u16,
    /// (x, y, length, key), row by row.
    pub runs: Vec<(u16, u16, u16, char)>,
}

impl Sprite {
    /// Read one frame: one string per row, `.` or a space for clear. Rows
    /// may be ragged; the widest sets the width.
    fn parse(rows: &[&str]) -> Sprite {
        let mut runs = Vec::new();
        let mut w = 0;
        for (y, row) in rows.iter().enumerate() {
            let chars: Vec<char> = row.chars().collect();
            w = w.max(chars.len());
            let mut x = 0;
            while x < chars.len() {
                let key = chars[x];
                let start = x;
                while x < chars.len() && chars[x] == key {
                    x += 1;
                }
                if key != '.' && key != ' ' {
                    runs.push((start as u16, y as u16, (x - start) as u16, key));
                }
            }
        }
        Sprite { w: w as u16, h: rows.len() as u16, runs }
    }

    /// Its runs as draws, top-left at (x, y), mirrored left-right when
    /// `flip`, every key passed through `key` (a recolour).
    pub fn draw(&self, x: f32, y: f32, flip: bool, alpha: f32, key: impl Fn(char) -> char, out: &mut Vec<Draw>) {
        self.draw_scaled(x, y, 1.0, flip, alpha, key, out);
    }

    /// The same, one sprite pixel `scale` cells across (`Scene::art`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scaled(&self, x: f32, y: f32, scale: f32, flip: bool, alpha: f32, key: impl Fn(char) -> char, out: &mut Vec<Draw>) {
        for &(rx, ry, len, k) in &self.runs {
            let left = if flip { self.w - rx - len } else { rx };
            out.push(Draw::cell(x + left as f32 * scale, y + ry as f32 * scale, len as f32 * scale, scale, key(k), alpha));
        }
    }
}

/// A scene, read from its file.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    pub kind: Kind,
    pub place: Place,
    /// Frames a second it may draw when behind the window.
    pub fps: f32,
    /// Logical pixels per cell: the unit every position and speed is in.
    pub cell: f32,
    /// Logical pixels per sprite pixel: the cell unless the file says
    /// finer (the cat is drawn in 2 px pixels on 5 px cells).
    pub pixel: f32,
    /// Key → its colour.
    pub palette: BTreeMap<char, Ink>,
    /// Block name → its frames.
    pub sprites: BTreeMap<String, Vec<Sprite>>,
}

impl Scene {
    /// The frame rate it runs at: its own behind the window, the
    /// standard in the footer.
    pub fn fps(&self) -> f32 {
        match self.place {
            Place::Window | Place::Front => self.fps,
            Place::Footer => FOOTER_FPS,
        }
    }

    /// Cells per sprite pixel, each way: 1 unless sprites are finer.
    pub fn art(&self) -> f32 {
        self.pixel / self.cell
    }

    /// Frame `i` of sprite block `name` (wrapping). The file was checked
    /// to have every block its kind needs.
    pub fn sprite(&self, name: &str, i: usize) -> &Sprite {
        let frames = &self.sprites[name];
        &frames[i % frames.len()]
    }

    /// Read a scene file:
    ///
    /// - `## Scene`: `| key | value | use |` rows for `kind`, `place`,
    ///   `fps`, `cell` and `pixel` (a sprite pixel, default the cell);
    /// - `## Palette`: `| key | role | light | dark | use |`, one character
    ///   key to its colour (`Ink`): a role (a standard colour token, or `-`)
    ///   and its own light and dark `#RRGGBB[AA]`. The older
    ///   `| key | token | use |` row still reads (a role and no colour of
    ///   its own: clear where the theme lacks it);
    /// - `## Sprites`: `### <block>` headings, each followed by one fenced
    ///   block per frame, one string per row.
    ///
    /// It is checked whole: a known kind, a frame rate and cell size above
    /// 0, every palette role a real colour token, every key the kind and
    /// its sprites use in the palette, every block it needs with enough
    /// frames, and the frames of a block one size. Err names what is wrong.
    pub fn from_md(name: &str, text: &str) -> Result<Scene, String> {
        let mut section = "";
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        let mut palette = BTreeMap::new();
        let mut sprites: BTreeMap<String, Vec<Sprite>> = BTreeMap::new();
        let mut block: Option<String> = None;
        let mut fence: Option<Vec<String>> = None;
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let bad = |what: String| format!("line {}: {what}", n + 1);
            if let Some(rows) = fence.as_mut() {
                if line.starts_with("```") {
                    let rows = fence.take().unwrap_or_default();
                    let Some(b) = &block else { return Err(bad("a sprite frame outside a ### block".into())) };
                    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
                    sprites.entry(b.clone()).or_default().push(Sprite::parse(&refs));
                } else {
                    rows.push(line.to_string());
                }
                continue;
            }
            if let Some(h) = line.strip_prefix("## ") {
                section = match h.trim() {
                    "Scene" => "scene",
                    "Palette" => "palette",
                    "Sprites" => "sprites",
                    _ => "",
                };
                block = None;
                continue;
            }
            if section == "sprites" {
                if let Some(h) = line.strip_prefix("### ") {
                    block = Some(h.trim().to_string());
                } else if line.starts_with("```") {
                    fence = Some(Vec::new());
                }
                continue;
            }
            if !line.starts_with('|') || section.is_empty() {
                continue;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if cells[0] == "key" || cells[0].starts_with('-') {
                continue;
            }
            let [key, value, ..] = cells[..] else { return Err(bad("a row needs a key and a value".into())) };
            if section == "scene" {
                values.insert(key.to_string(), value.to_string());
            } else {
                let mut chars = key.chars();
                let (Some(k), None) = (chars.next(), chars.next()) else {
                    return Err(bad(format!("palette key \"{key}\" is not one character")));
                };
                if k == '.' || k == ' ' {
                    return Err(bad("`.` and space mean clear; they cannot be palette keys".into()));
                }
                palette.insert(k, Ink::from_cells(&cells[1..]).map_err(bad)?);
            }
        }
        if fence.is_some() {
            return Err("a sprite frame is not closed with ```".into());
        }
        let get = |k: &str| values.get(k).map(String::as_str);
        let kind = get("kind").and_then(Kind::parse).ok_or("`kind` must be cat, terminal or abyss")?;
        let place = match get("place").unwrap_or("window") {
            "window" => Place::Window,
            "front" => Place::Front,
            "footer" => Place::Footer,
            other => return Err(format!("`place` is window, front or footer, not \"{other}\"")),
        };
        let number = |k: &str, default: f32| -> Result<f32, String> {
            match get(k) {
                None => Ok(default),
                Some(v) => v.parse::<f32>().ok().filter(|v| *v > 0.0).ok_or(format!("`{k}` must be a number above 0, not \"{v}\"")),
            }
        };
        let cell = number("cell", 4.0)?;
        let pixel = number("pixel", cell)?;
        let scene = Scene { name: name.to_string(), kind, place, fps: number("fps", FOOTER_FPS)?, cell, pixel, palette, sprites };
        scene.check()?;
        Ok(scene)
    }

    fn check(&self) -> Result<(), String> {
        for &(block, frames) in self.kind.sprites() {
            let have = self.sprites.get(block).map_or(0, Vec::len);
            if have < frames {
                return Err(format!("sprite `{block}` needs {frames} frame(s), has {have}"));
            }
        }
        for (block, frames) in &self.sprites {
            let (w, h) = (frames[0].w, frames[0].h);
            if frames.iter().any(|f| (f.w, f.h) != (w, h)) {
                return Err(format!("the frames of sprite `{block}` are not one size"));
            }
            for f in frames {
                if let Some(&(_, _, _, k)) = f.runs.iter().find(|r| !self.palette.contains_key(&r.3)) {
                    return Err(format!("sprite `{block}` uses key `{k}`, which the palette lacks"));
                }
            }
        }
        if let Some(k) = self.kind.keys().iter().find(|k| !self.palette.contains_key(k)) {
            return Err(format!("the {:?} scene draws with key `{k}`, which the palette lacks", self.kind));
        }
        Ok(())
    }
}

/// One palette key's colour: the scene's own light and dark colour, and
/// an optional role, a standard theme token (`accent`, `on_accent`,
/// `text_accent`, `error`, `accent_muted`, …) whose value the theme on
/// screen gives instead. `-` in the file: no role, always its own colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ink {
    pub role: Option<String>,
    pub light: u32,
    pub dark: u32,
}

impl Ink {
    /// A palette row's cells after the key: `role, light, dark[, use]`,
    /// or the older `token[, use]`.
    fn from_cells(cells: &[&str]) -> Result<Ink, String> {
        let role = |r: &str| -> Result<Option<String>, String> {
            match r {
                "-" | "" => Ok(None),
                r if LEGACY_COLOR_NAMES.contains(&r) => {
                    Err(format!("\"{r}\" is gone from themes: give the key its own colours, `| key | - | #light | #dark | use |`"))
                }
                r if COLOR_NAMES.contains(&r) => Ok(Some(r.to_string())),
                r => Err(format!("\"{r}\" is not a colour token (a role is a theme token, or -)")),
            }
        };
        match cells {
            [r, light, dark, ..] if theme::parse_hex(light).is_some() || theme::parse_hex(dark).is_some() => {
                let hex = |s: &str| theme::parse_hex(s).ok_or_else(|| format!("\"{s}\" is not a colour (#RRGGBB or #RRGGBBAA)"));
                Ok(Ink { role: role(r)?, light: hex(light)?, dark: hex(dark)? })
            }
            // The older row: a token, no colour of its own.
            [token, ..] => Ok(Ink { role: Some(role(token)?.ok_or("an older `| key | token |` row needs a token")?), light: 0, dark: 0 }),
            [] => Err("a palette row needs a role and its light and dark colours".into()),
        }
    }

    /// Its colour: the role's value when `role` gives one (the theme on
    /// screen has that token), else its own for `mode`.
    pub fn colour(&self, mode: Mode, role: impl Fn(&str) -> Option<u32>) -> u32 {
        self.role.as_deref().and_then(role).unwrap_or(match mode {
            Mode::Light => self.light,
            Mode::Dark => self.dark,
        })
    }
}

/// Every palette key's colour for `mode`, with `role` the theme's value
/// of a token (None: the theme lacks it). Pure: what every renderer
/// paints with, resolved again whenever the mode or the theme changes.
pub fn colours(palette: &BTreeMap<char, Ink>, mode: Mode, role: impl Fn(&str) -> Option<u32>) -> BTreeMap<char, u32> {
    palette.iter().map(|(k, ink)| (*k, ink.colour(mode, &role))).collect()
}

impl Scene {
    /// Its colours with the tokens on screen (their theme and mode).
    pub fn colours(&self, t: &Tokens) -> BTreeMap<char, u32> {
        colours(&self.palette, t.mode, |name| t.color(name))
    }
}

/// The CSS custom property a palette key's colour is in on the server's
/// pages (`--scene-f`): the key itself when it is a letter or digit, else
/// `u` and its code point in hex.
pub fn css_var(key: char) -> String {
    if key.is_ascii_alphanumeric() { format!("scene-{key}") } else { format!("scene-u{:x}", key as u32) }
}

/// Seconds per loop of the slowest backdrop layer when nothing says.
pub const DEFAULT_MOTION: f32 = 40.0;

/// The scene settings in force: which scene (None: none) and the motion
/// (0: still). Part of the appearance, like light/dark, and not the
/// theme's: any theme shows any scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub scene: Option<String>,
    pub motion: f32,
}

impl Settings {
    /// Nothing moves (`motion` 0). A shell also stills it for the
    /// system's reduced-motion setting.
    pub fn still(&self) -> bool {
        self.motion <= 0.0
    }
}

/// The settings in force, nearest first: the override layers (later
/// wins: a route's overrides, then `state/local.md`), then the app's
/// own (`app.md`, `State::scene` / `State::motion`), then the shipped
/// default's `app.md`; for the motion, an older theme file's `motion`
/// row before that default. `none` (or blank) is no scene.
pub fn settings(state: &State, theme: &Theme, layers: &[&Overrides]) -> Settings {
    let shipped = || crate::files::parse_settings(crate::shipped::FILES.iter().find(|(p, _)| *p == "app.md").map_or("", |(_, t)| *t));
    let scene = layers.iter().rev().find_map(|l| l.scene.clone()).or_else(|| state.scene.clone()).or_else(|| shipped().scene);
    let motion = layers
        .iter()
        .rev()
        .find_map(|l| l.motion)
        .or(state.motion)
        .or_else(|| theme.legacy_motion())
        .or_else(|| shipped().motion)
        .unwrap_or(DEFAULT_MOTION);
    let scene = scene.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != NO_SCENE);
    Settings { scene, motion }
}

/// How the engines are doing, as a scene sees it. The same rule as the
/// dashboard's tiles (`crate::dashboard::tiles`): any failed or missing
/// engine needs you; else any working one means working; else idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mood {
    #[default]
    Idle,
    Working,
    NeedsYou,
}

/// The mood of a set of live statuses (`crate::dashboard::live_status`).
pub fn mood(statuses: &[Status]) -> Mood {
    if statuses.iter().any(|s| matches!(s, Status::Failed(_) | Status::Missing(_))) {
        Mood::NeedsYou
    } else if statuses.iter().any(|s| matches!(s, Status::Working(_))) {
        Mood::Working
    } else {
        Mood::Idle
    }
}

/// The pointer, as a scene sees it. The shell reports what the mouse did
/// over the window; the scene decides what it was over (the cat knows
/// where it is, so it knows when a click landed on it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pointer {
    /// Where it is, in cells from the scene's top-left corner.
    pub x: f32,
    pub y: f32,
    /// A button is held down.
    pub pressed: bool,
    /// Presses so far: one more is a new click.
    pub clicks: u32,
    /// Inside the window; false once it has left.
    pub inside: bool,
}

/// What a scene reacts to. Later inputs (weather, a map position) join
/// here; a scene that does not use one ignores it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    pub mood: Mood,
    /// The area, in cells.
    pub cols: f32,
    pub rows: f32,
    /// Where the floor is, in cells from the top: what a walking scene
    /// stands on. The shell sets it (the footer's top edge, so the cat is
    /// not hidden under the footer row).
    pub floor: f32,
    /// The `motion` token was 0: nothing moves; the pose still follows
    /// the mood.
    pub still: bool,
    /// The pointer, once it has been seen over the window. Each move or
    /// press is a change, so `step` runs at the moment it happened.
    pub pointer: Option<Pointer>,
}

/// One rectangle to paint, in cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// The palette key: its token gives the colour.
    pub key: char,
    /// Multiplies the colour's own opacity, 0..1.
    pub alpha: f32,
    /// A soft glow around it, in cells; 0 none. A renderer that cannot
    /// blur skips it.
    pub glow: f32,
}

impl Draw {
    pub fn cell(x: f32, y: f32, w: f32, h: f32, key: char, alpha: f32) -> Draw {
        Draw { x, y, w, h, key, alpha: alpha.clamp(0.0, 1.0), glow: 0.0 }
    }

    pub fn glowing(self, glow: f32) -> Draw {
        Draw { glow, ..self }
    }
}

/// One picture of a scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// Back to front.
    pub draws: Vec<Draw>,
    /// Where each moving thing is (the cat, each jellyfish, the cursor),
    /// in cells: the same at every detail tier.
    pub anchors: Vec<(f32, f32)>,
    /// When the picture next changes: `Some(t)` now or later (the shell
    /// waits until then, at least one frame at the scene's fps); `None`:
    /// not until the inputs change, so the loop parks.
    pub next: Option<f64>,
}

/// A scene's memory between frames: only what the inputs changed. Most
/// scenes need none; the cat remembers what it was doing.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub inputs: Inputs,
    cat: cat::Cat,
}

/// Start `scene` at time `t`.
pub fn start(inputs: Inputs, t: f64) -> Live {
    Live { inputs, cat: cat::Cat::start(&inputs, t) }
}

/// The inputs changed at time `t`: the scene's memory after. Unchanged
/// inputs change nothing, so calling it often is free.
pub fn step(live: &Live, inputs: Inputs, t: f64) -> Live {
    if inputs == live.inputs {
        return live.clone();
    }
    Live { inputs, cat: live.cat.step(&live.inputs, &inputs, t) }
}

/// The picture at time `t`, drawn at frame rate `fps` (which only sets
/// the detail).
pub fn frame(scene: &Scene, live: &Live, t: f64, fps: f32) -> Frame {
    let level = detail(fps);
    let i = &live.inputs;
    match scene.kind {
        Kind::Cat => live.cat.frame(scene, i, t, level),
        Kind::Terminal => terminal::frame(i, t, level),
        Kind::Abyss => abyss::frame(scene, i, t, level),
    }
}

/// A repeatable pseudo-random number in 0..1 for (`seed`, `n`): the same
/// arguments give the same number, so "random" paths are pure in time.
pub fn noise(seed: u32, n: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ n.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The fractional part, always 0..1 (also for negative numbers).
pub(crate) fn frac(x: f64) -> f64 {
    x - x.floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scene the app ships, `(name, text)`, read from `scenes/`.
    pub(crate) fn shipped_all() -> Vec<(String, String)> {
        crate::look::shipped_all(crate::look::Kind::Scene)
    }

    pub(crate) fn shipped(name: &str) -> Scene {
        let text = crate::look::shipped(crate::look::Kind::Scene, name);
        Scene::from_md(name, &text).unwrap_or_else(|e| panic!("scenes/{name}/scene.md: {e}"))
    }

    pub(crate) fn inputs(mood: Mood) -> Inputs {
        Inputs { mood, cols: 200.0, rows: 120.0, floor: 112.0, still: false, pointer: None }
    }

    #[test]
    fn every_shipped_scene_reads_and_owns_its_colours() {
        for (name, _) in shipped_all() {
            let s = shipped(&name);
            // The cat walks over the panes (their frost would hide a small
            // walker); the others are backgrounds behind them.
            let want = if name == "cat" { Place::Front } else { Place::Window };
            assert_eq!(s.place, want, "{name} moves across the whole window");
            for (k, ink) in &s.palette {
                assert!(ink.light & 0xFF != 0 && ink.dark & 0xFF != 0, "{name}'s `{k}` has visible colours of its own");
            }
        }
    }

    #[test]
    fn a_role_takes_the_themes_colour_and_its_own_is_the_fallback() {
        let theme = |text: &str| crate::theme::Theme::from_md(text).unwrap();
        let cat_theme = theme(&crate::look::shipped(crate::look::Kind::Theme, "cat"));
        let default_theme = theme(&crate::look::shipped(crate::look::Kind::Theme, "default"));
        let cat = shipped("cat");
        let fur = |th: &crate::theme::Theme, m: Mode| cat.colours(&crate::theme::resolve(m, th))[&'f'];
        assert_eq!(fur(&cat_theme, Mode::Light), 0xD9733FFF, "terracotta on the cat theme");
        assert_eq!(fur(&default_theme, Mode::Light), 0x0A84FFFF, "the default theme's blue accent");
        assert_eq!(fur(&default_theme, Mode::Dark), crate::theme::resolve(Mode::Dark, &default_theme).accent);
        // Without the role, the scene's own colour for the mode.
        let none = |_: &str| None;
        assert_eq!(colours(&cat.palette, Mode::Light, none)[&'f'], 0xD9733FFF);
        assert_eq!(colours(&cat.palette, Mode::Dark, none)[&'f'], 0xE8875AFF);
        // Abyss's art colours are its own whatever the theme.
        let abyss = shipped("abyss");
        let j = |th: &crate::theme::Theme| abyss.colours(&crate::theme::resolve(Mode::Dark, th))[&'j'];
        assert_eq!(j(&cat_theme), j(&default_theme));
        assert_eq!(css_var('f'), "scene-f");
        assert_eq!(css_var('#'), "scene-u23");
    }

    #[test]
    fn the_scene_is_a_setting_and_the_nearest_layer_wins() {
        let theme = crate::theme::Theme::from_md(&crate::look::shipped(crate::look::Kind::Theme, "default")).unwrap();
        let mut state = State::default();
        let shipped_default = settings(&state, &theme, &[]);
        assert_eq!(shipped_default, Settings { scene: Some("cat".into()), motion: 40.0 }, "default/app.md: the cat");
        state.scene = Some("abyss".into());
        state.motion = Some(25.0);
        assert_eq!(settings(&state, &theme, &[]), Settings { scene: Some("abyss".into()), motion: 25.0 });
        let route = Overrides { motion: Some(0.0), ..Overrides::default() };
        let local = Overrides { scene: Some("none".into()), ..Overrides::default() };
        let s = settings(&state, &theme, &[&route, &local]);
        assert_eq!((s.scene, s.motion), (None, 0.0), "none is no scene; a route stills it");
        assert!(settings(&state, &theme, &[&route]).still());
        // An older theme's motion row counts when the app says nothing.
        let mut old = theme.clone();
        old.defaults.numbers.insert("motion".into(), 30.0);
        assert_eq!(settings(&State::default(), &old, &[]).motion, 30.0);
    }

    #[test]
    fn a_bad_scene_file_says_what_is_wrong() {
        let head = "## Scene\n| kind | cat |\n";
        assert!(Scene::from_md("x", "## Scene\n| kind | dog |\n").unwrap_err().contains("kind"));
        assert!(Scene::from_md("x", &format!("{head}## Palette\n| o | not_a_token |\n")).unwrap_err().contains("not_a_token"));
        assert!(Scene::from_md("x", &format!("{head}## Palette\n| o | - | #12 | #000000 | x |\n")).unwrap_err().contains("#12"));
        assert!(Scene::from_md("x", &format!("{head}## Palette\n| o | scene_2 |\n")).unwrap_err().contains("own colours"), "the old inks are gone");
        assert!(Scene::from_md("x", head).unwrap_err().contains("sleep"), "a cat needs its sprites");
        let ragged = "## Scene\n| kind | terminal |\n## Palette\n| j | accent |\n## Sprites\n### bell\n```\njj\n```\n```\njjj\n```\n";
        assert!(Scene::from_md("x", ragged).unwrap_err().contains("one size"));
        let unknown = "## Scene\n| kind | terminal |\n## Palette\n| t | - | #00FF00 | #00FF00 | x |\n## Sprites\n### x\n```\nz\n```\n";
        assert!(Scene::from_md("x", unknown).unwrap_err().contains("`z`"));
        assert!(Scene::from_md("x", "## Scene\n| kind | terminal |\n| fps | 0 |\n").unwrap_err().contains("fps"));
    }

    #[test]
    fn a_sprite_pixel_can_be_finer_than_a_cell() {
        let keys = "## Palette\n| t | accent | #000000 | #FFFFFF |\n| c | - | #000000 | #FFFFFF |\n| l | text_primary |\n| a | error |\n";
        let base = format!("{keys}## Scene\n| kind | terminal |\n| cell | 5 |\n");
        assert_eq!(Scene::from_md("x", &base).unwrap().art(), 1.0, "a sprite pixel is a cell unless it says");
        let fine = Scene::from_md("x", &format!("{base}| pixel | 2 |\n")).unwrap();
        assert_eq!((fine.pixel, fine.art()), (2.0, 0.4));
        assert!(Scene::from_md("x", &format!("{base}| pixel | 0 |\n")).unwrap_err().contains("pixel"));
        let s = Sprite::parse(&["ab"]);
        let mut out = Vec::new();
        s.draw_scaled(10.0, 4.0, 0.4, true, 1.0, |k| k, &mut out);
        assert_eq!((out[0].x, out[0].w, out[0].h, out[1].x), (10.4, 0.4, 0.4, 10.0), "scaled, and mirrored inside");
    }

    #[test]
    fn runs_cover_every_lit_cell_once_and_mirror_inside_the_sprite() {
        let s = Sprite::parse(&[".ab", "bb.", "..."]);
        assert_eq!((s.w, s.h), (3, 3));
        assert_eq!(s.runs, vec![(1, 0, 1, 'a'), (2, 0, 1, 'b'), (0, 1, 2, 'b')]);
        let mut out = Vec::new();
        s.draw(10.0, 0.0, true, 1.0, |k| k, &mut out);
        assert_eq!((out[0].x, out[2].x, out[2].w), (11.0, 11.0, 2.0), "mirrored: x ↦ w - x - len");
    }

    #[test]
    fn mood_follows_the_tiles_rule() {
        let w = Status::Working("asking".into());
        assert_eq!(mood(&[]), Mood::Idle);
        assert_eq!(mood(&[Status::Ready, Status::Done("ok".into())]), Mood::Idle);
        assert_eq!(mood(&[Status::Idle, w.clone()]), Mood::Working);
        assert_eq!(mood(&[w, Status::Failed("timeout".into())]), Mood::NeedsYou, "needing you wins over working");
        assert_eq!(mood(&[Status::Missing("claude".into())]), Mood::NeedsYou);
    }

    #[test]
    fn a_footer_scene_runs_at_the_standard_rate_and_fps_buys_detail() {
        let mut s = shipped("abyss");
        assert_eq!(s.fps(), s.fps);
        s.place = Place::Footer;
        assert_eq!(s.fps(), FOOTER_FPS);
        assert_eq!((detail(24.0), detail(30.0), detail(60.0), detail(120.0)), (1, 2, 3, 3));
    }

    /// The fps rule: sampling at 24 and at 60 fps gives the same positions
    /// at the same `t`, and the higher tier draws more detail. The inputs
    /// change at their own moments, as they do in the app: the engines'
    /// mood, and a pointer that sweeps, holds still over the cat, clicks
    /// it, leaves the window and comes back.
    #[test]
    fn the_same_t_gives_the_same_pose_at_any_fps_and_more_fps_more_detail() {
        let at = |x: f32, y: f32| Some(Pointer { x, y, inside: true, ..Pointer::default() });
        type Change = Box<dyn Fn(Inputs) -> Inputs>;
        let mut events: Vec<(f64, Change)> = vec![
            (2.0, Box::new(|i| Inputs { mood: Mood::Working, ..i })),
            (6.0, Box::new(|i| Inputs { mood: Mood::Idle, ..i })),
            (9.0, Box::new(|i| Inputs { mood: Mood::NeedsYou, ..i })),
        ];
        // A sweep from 150 to 80, a move every 1/90 s.
        for n in 0..=135 {
            events.push((0.3 + n as f64 / 90.0, Box::new(move |i| Inputs { pointer: at(150.0 - n as f32 * 0.52, 30.0), ..i })));
        }
        // Held still just over its head, while it works; then a click on it
        // in its idle time, and away out of the window.
        events.push((2.5, Box::new(move |i| Inputs { pointer: at(80.0, 91.0), ..i })));
        let click = |pressed: bool| move |i: Inputs| {
            let p = i.pointer.unwrap_or_default();
            Inputs { pointer: Some(Pointer { pressed, clicks: p.clicks + pressed as u32, ..p }), ..i }
        };
        events.push((6.6, Box::new(click(true))));
        events.push((6.7, Box::new(click(false))));
        events.push((7.5, Box::new(|i| Inputs { pointer: i.pointer.map(|p| Pointer { inside: false, ..p }), ..i })));
        events.push((10.0, Box::new(move |i| Inputs { pointer: at(120.0, 100.0), ..i })));
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (name, _) in shipped_all() {
            let s = shipped(&name);
            let mut live24 = start(inputs(Mood::Idle), 0.0);
            let mut live60 = live24.clone();
            // Each frame, the shell steps with every input change since the
            // last frame, each stamped with the time it happened, not the
            // frame's. Every 1/12 s (a frame at both rates) it notes where
            // everything is.
            let run = |live: &mut Live, fps: u32, until: f64| {
                let (mut n, mut e, mut seen) = (0, 0, Vec::new());
                loop {
                    let t = n as f64 / fps as f64;
                    if t > until {
                        break seen;
                    }
                    while e < events.len() && events[e].0 <= t {
                        *live = step(live, events[e].1(live.inputs), events[e].0);
                        e += 1;
                    }
                    let f = frame(&s, live, t, fps as f32);
                    if (n * 12) % fps == 0 {
                        seen.push((t, f.anchors));
                    }
                    n += 1;
                }
            };
            let seen24 = run(&mut live24, 24, 12.0);
            let seen60 = run(&mut live60, 60, 12.0);
            assert_eq!(live24, live60, "{name}: the same memory after the same changes");
            assert_eq!(seen24.len(), 145);
            for ((t, a), (t60, b)) in seen24.iter().zip(&seen60) {
                assert_eq!(t, t60);
                assert_eq!(a, b, "{name} at t={t}: same positions");
                assert!(!a.is_empty(), "{name} has something that moves");
            }
            if name == "cat" {
                // The pointer reached it: it ran with the sweep to where
                // the pointer stopped, and done (working, then idle) purred
                // hearts (anchors past the cat's own).
                let at = |t: f64| &seen60[(t * 12.0).round() as usize].1;
                assert!((at(4.0)[0].0 - 80.0).abs() < 2.5, "it followed the sweep: {:?} to {:?}", at(0.5), at(4.0));
                assert!(at(7.0).len() > 1, "done purred: {:?}", at(7.0));
            }
            let busy = step(&live24, Inputs { mood: Mood::Working, ..live24.inputs }, 12.0);
            let t = 13.3;
            let (lo, hi) = (frame(&s, &busy, t, 24.0), frame(&s, &busy, t, 60.0));
            assert_eq!(lo.anchors, hi.anchors);
            assert!(hi.draws.len() > lo.draws.len(), "{name}: 60 fps draws more ({} vs {})", hi.draws.len(), lo.draws.len());
        }
    }

    #[test]
    fn motion_0_is_fully_still_and_parks() {
        for (name, _) in shipped_all() {
            let s = shipped(&name);
            let still = Inputs { still: true, ..inputs(Mood::Working) };
            let live = start(still, 0.0);
            let (a, b) = (frame(&s, &live, 1.0, 60.0), frame(&s, &live, 50.0, 60.0));
            assert_eq!(a, b, "{name} does not move");
            assert_eq!(a.next, None, "{name} parks its loop");
        }
    }
}
