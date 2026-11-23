//! Design tokens, Astryx-style: one set of named, *semantic* values that the
//! whole app draws with. A component never says "grey #6b7280"; it says
//! `text_secondary`, and the theme decides what that is.
//!
//! Like Astryx's `light-dark(light, dark)` tokens, every colour is a pair.
//! The `Mode` picks one side.
//!
//! The values live in theme files, `themes/<name>.md` at the repo root (the
//! active one is named in `themes/active`; `default` when there is none):
//! each row holds a token's current value and its default. The code
//! only knows the token *names* (`COLOR_NAMES`, `NUMBER_NAMES`). `Theme`
//! is that file read in: `defaults` plus `changes` (the rows whose value
//! differs from the default). `resolve` layers the two into the `Tokens`
//! every component reads; reset removes a change and the default is back.
//!
//! Colours are `0xRRGGBBAA`. `themes/default.md` follows Astryx's default
//! theme; `themes/terminal.md` is black and phosphor green, in a monospace font.

use std::collections::BTreeMap;

/// Light or dark appearance. Saved with the state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    Light,
    /// The default: a pitch-black window with dark glass on it.
    #[default]
    Dark,
}

impl Mode {
    pub fn toggled(self) -> Mode {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
        }
    }

    /// The name used in `state.txt`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }

    pub fn parse(name: &str) -> Option<Mode> {
        match name {
            "light" => Some(Mode::Light),
            "dark" => Some(Mode::Dark),
            _ => None,
        }
    }
}

/// A colour with a light and a dark value, like CSS `light-dark()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightDark {
    pub light: u32,
    pub dark: u32,
}

impl LightDark {
    pub const fn new(light: u32, dark: u32) -> Self {
        LightDark { light, dark }
    }

    pub fn pick(self, mode: Mode) -> u32 {
        match mode {
            Mode::Light => self.light,
            Mode::Dark => self.dark,
        }
    }
}

/// Every colour token's name, in display order. The values live in
/// the theme file; code only knows which tokens exist.
pub const COLOR_NAMES: &[&str] = &[
    "background_body",
    "background_surface",
    "background_muted",
    "overlay_hover",
    "overlay_pressed",
    "text_primary",
    "text_secondary",
    "text_disabled",
    "text_accent",
    "accent",
    "accent_muted",
    "on_accent",
    "border",
    "border_emphasized",
    "error",
    "shadow",
    "window_tint",
    "glass_fill",
    "glass_sheen",
    "glass_edge",
    "glass_highlight",
    "backdrop_1",
    "backdrop_2",
    "backdrop_3",
    "status_bar",
    "on_status_bar",
    "line_highlight",
    "activity_llm",
    "activity_tools",
    "activity_compress",
    "activity_wait",
    "activity_work",
    "activity_need",
    "activity_fail",
];

/// Colour tokens a theme no longer has: the scene inks, now each scene's
/// own (`crate::scene::Ink`). An older theme or override file that still
/// has their rows reads, and the rows are dropped.
pub const LEGACY_COLOR_NAMES: &[&str] = &["scene_1", "scene_2", "scene_3", "scene_4", "scene_5", "scene_6"];

/// Every size token's name (logical pixels; the same in both modes).
pub const NUMBER_NAMES: &[&str] =
    &["space_1", "space_2", "space_3", "space_4", "radius", "text_sm", "text_base", "text_code", "text_lg", "pane_gap", "pane_radius", "nav_pinned"];

/// The token that holds the font family.
pub const FONT: &str = "font";
/// The token that holds the monospace font family: code, line numbers and
/// terminal output.
pub const FONT_MONO: &str = "font_mono";
/// The setting that names the scene (`scenes/<name>.md`, see
/// `crate::scene`), or `none`. Not a theme token: an app setting
/// (`app.md`'s `scene:`), which a `## Scene` row of an override set
/// (`state/local.md`, a route's `overrides.md`) can override.
pub const SCENE: &str = "scene";
/// The scene value that means no scene.
pub const NO_SCENE: &str = "none";
/// The setting for how fast things move: seconds per loop of the slowest
/// backdrop layer; 0 keeps the backdrop and the scene still. Also an app
/// setting (`app.md`'s `motion:`, a `## Scene` row). A theme file's old
/// `motion` size row is still read (`Theme::legacy_motion`).
pub const MOTION: &str = "motion";

/// The defaults, as written in the theme file, plus each token's `use` note.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Defaults {
    pub colors: BTreeMap<String, LightDark>,
    pub numbers: BTreeMap<String, f32>,
    pub font: String,
    pub font_mono: String,
    /// Token name → what it is for (the last column of the tables).
    pub notes: BTreeMap<String, String>,
}

/// The whole theme: the defaults and, on top, what was changed.
/// The theme file holds both: a row's value and its default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Theme {
    pub defaults: Defaults,
    pub changes: Overrides,
}

/// What was changed. Anything not in here uses its default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    /// Colour changes, per mode: (mode, token name) → value.
    pub colors: BTreeMap<(Mode, String), u32>,
    /// Size changes: token name → value.
    pub numbers: BTreeMap<String, f32>,
    /// Font family change.
    pub font: Option<String>,
    /// Monospace font family change.
    pub font_mono: Option<String>,
    /// The scene setting (`none`: no scene), from a `## Scene` row. Not a
    /// theme token: `crate::scene::settings` reads it from the layers.
    pub scene: Option<String>,
    /// The motion setting, from a `## Scene` row (or an old `## Sizes`
    /// `motion` row). Read the same way.
    pub motion: Option<f32>,
}

impl Theme {
    /// The default of colour `name` in `mode`.
    pub fn default_color(&self, name: &str, mode: Mode) -> Option<u32> {
        self.defaults.colors.get(name).map(|pair| pair.pick(mode))
    }

    pub fn default_number(&self, name: &str) -> Option<f32> {
        self.defaults.numbers.get(name).copied()
    }

    /// Set colour `name` in `mode` to `value`. Setting it to its default
    /// removes the change instead, so only real changes are kept.
    /// Returns false for a name that is not a colour token.
    pub fn set_color(&mut self, mode: Mode, name: &str, value: u32) -> bool {
        let Some(default) = self.default_color(name, mode) else { return false };
        if value == default {
            self.changes.colors.remove(&(mode, name.to_string()));
        } else {
            self.changes.colors.insert((mode, name.to_string()), value);
        }
        true
    }

    /// Same as `set_color`, for a size token.
    pub fn set_number(&mut self, name: &str, value: f32) -> bool {
        let Some(default) = self.default_number(name) else { return false };
        if value == default {
            self.changes.numbers.remove(name);
        } else {
            self.changes.numbers.insert(name.to_string(), value);
        }
        true
    }

    /// Set the font; the default font (or blank) clears the change.
    pub fn set_font(&mut self, family: &str) {
        let family = family.trim();
        self.changes.font = (!family.is_empty() && family != self.defaults.font).then(|| family.to_string());
    }

    /// Same as `set_font`, for the monospace font.
    pub fn set_font_mono(&mut self, family: &str) {
        let family = family.trim();
        self.changes.font_mono = (!family.is_empty() && family != self.defaults.font_mono).then(|| family.to_string());
    }

    /// A theme file's old `motion` size row (before motion was an app
    /// setting): its value, if the file has one.
    pub fn legacy_motion(&self) -> Option<f32> {
        self.changes.numbers.get(MOTION).or_else(|| self.defaults.numbers.get(MOTION)).copied()
    }

    /// Set any token from text (`#RRGGBBAA`, a number, a font name). For a
    /// colour, `mode` says which side; `None` sets both. Err says why not.
    pub fn set(&mut self, name: &str, value: &str, mode: Option<Mode>) -> Result<(), String> {
        if name == FONT {
            self.set_font(value);
            return Ok(());
        }
        if name == FONT_MONO {
            self.set_font_mono(value);
            return Ok(());
        }
        if name == SCENE || name == MOTION {
            return Err(format!("{name} is an app setting, not a theme token (app.md, or a ## Scene row in state/local.md)"));
        }
        if COLOR_NAMES.contains(&name) {
            let v = parse_hex(value).ok_or_else(|| format!("\"{value}\" is not #RRGGBB or #RRGGBBAA"))?;
            for m in mode.map_or(vec![Mode::Light, Mode::Dark], |m| vec![m]) {
                self.set_color(m, name, v);
            }
            return Ok(());
        }
        if NUMBER_NAMES.contains(&name) {
            let v: f32 = value.trim().parse().map_err(|_| format!("\"{value}\" is not a number"))?;
            if v < 0.0 {
                return Err("sizes cannot be negative".into());
            }
            self.set_number(name, v);
            return Ok(());
        }
        Err(format!("no token \"{name}\""))
    }

    /// Put one token back to its default, in both modes. `None`: every token.
    pub fn reset(&mut self, name: Option<&str>) {
        match name {
            None => self.changes = Overrides::default(),
            Some(FONT) => self.changes.font = None,
            Some(FONT_MONO) => self.changes.font_mono = None,
            Some(name) => {
                self.changes.colors.retain(|(_, n), _| n != name);
                self.changes.numbers.remove(name);
            }
        }
    }

    /// Is this token changed from its default (in `mode`, for colours)?
    pub fn is_changed(&self, mode: Mode, name: &str) -> bool {
        let c = &self.changes;
        c.colors.contains_key(&(mode, name.to_string())) || c.numbers.contains_key(name) || (name == FONT && c.font.is_some()) || (name == FONT_MONO && c.font_mono.is_some())
    }

    /// Token names in the code that the theme file does not have.
    pub fn missing(&self) -> Vec<&'static str> {
        let d = &self.defaults;
        let mut out: Vec<&'static str> = COLOR_NAMES.iter().copied().filter(|n| !d.colors.contains_key(*n)).collect();
        out.extend(NUMBER_NAMES.iter().copied().filter(|n| !d.numbers.contains_key(*n)));
        if d.font.is_empty() {
            out.push(FONT);
        }
        if d.font_mono.is_empty() {
            out.push(FONT_MONO);
        }
        out
    }

    /// Read the theme file: the Colors, Sizes and Font tables. Rows are
    /// `| token | value(s) | default(s) | use |`; header and `---` rows,
    /// and anything outside the tables, are skipped. A row that does not
    /// parse is an error naming its line. An older file's `## Scene` table
    /// and `scene_1` ... `scene_6` rows are skipped (the scene is an app
    /// setting and owns its colours now); its `motion` row is kept as
    /// `legacy_motion`.
    pub fn from_md(text: &str) -> Result<Theme, String> {
        let mut theme = Theme::default();
        let mut section = "";
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if let Some(heading) = line.strip_prefix("## ") {
                section = match heading.trim() {
                    "Colors" => "colors",
                    "Sizes" => "sizes",
                    "Font" => "font",
                    "Scene" => "scene",
                    _ => "",
                };
                continue;
            }
            if !line.starts_with('|') || section.is_empty() {
                continue;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if cells[0] == "token" || cells[0].starts_with('-') {
                continue;
            }
            let bad = |what: &str| format!("line {}: {what}", n + 1);
            let name = cells[0].to_string();
            let note = cells.last().filter(|_| cells.len() > 3).map(|s| s.to_string());
            if section == "scene" || (section == "colors" && LEGACY_COLOR_NAMES.contains(&name.as_str())) {
                continue;
            }
            match section {
                "colors" => {
                    let [_, light, dark, def_light, def_dark, ..] = cells[..] else { return Err(bad("a colour row needs 5 cells")) };
                    let hex = |s: &str| parse_hex(s).ok_or_else(|| bad(&format!("\"{s}\" is not a colour")));
                    theme.defaults.colors.insert(name.clone(), LightDark::new(hex(def_light)?, hex(def_dark)?));
                    theme.set_color(Mode::Light, &name, hex(light)?);
                    theme.set_color(Mode::Dark, &name, hex(dark)?);
                }
                "sizes" => {
                    let [_, value, default, ..] = cells[..] else { return Err(bad("a size row needs 3 cells")) };
                    let num = |s: &str| s.parse::<f32>().map_err(|_| bad(&format!("\"{s}\" is not a number")));
                    theme.defaults.numbers.insert(name.clone(), num(default)?);
                    theme.set_number(&name, num(value)?);
                    if name == MOTION {
                        continue;
                    }
                }
                _ => {
                    let [_, value, default, ..] = cells[..] else { return Err(bad("a font row needs 3 cells")) };
                    match name.as_str() {
                        FONT => {
                            theme.defaults.font = default.to_string();
                            theme.set_font(value);
                        }
                        FONT_MONO => {
                            theme.defaults.font_mono = default.to_string();
                            theme.set_font_mono(value);
                        }
                        _ => return Err(bad(&format!("no font token \"{name}\""))),
                    }
                }
            }
            if let Some(note) = note {
                theme.defaults.notes.insert(name, note);
            }
        }
        Ok(theme)
    }

    /// Write the theme file back: the same tables, current value and default
    /// side by side, in the code's token order.
    pub fn to_md(&self) -> String {
        let note = |name: &str| self.defaults.notes.get(name).cloned().unwrap_or_default();
        let mut out = String::from(MD_INTRO);
        out.push_str("\n## Colors\n\n| token | light | dark | default light | default dark | use |\n|---|---|---|---|---|---|\n");
        for name in COLOR_NAMES {
            let Some(pair) = self.defaults.colors.get(*name) else { continue };
            let (light, dark) = (resolve_color(self, Mode::Light, name), resolve_color(self, Mode::Dark, name));
            out.push_str(&format!(
                "| {name} | {} | {} | {} | {} | {} |\n",
                hex(light),
                hex(dark),
                hex(pair.light),
                hex(pair.dark),
                note(name)
            ));
        }
        out.push_str("\n## Sizes\n\n| token | value | default | use |\n|---|---|---|---|\n");
        for name in NUMBER_NAMES {
            let Some(default) = self.default_number(name) else { continue };
            let value = self.changes.numbers.get(*name).copied().unwrap_or(default);
            out.push_str(&format!("| {name} | {value} | {default} | {} |\n", note(name)));
        }
        let font = self.changes.font.clone().unwrap_or_else(|| self.defaults.font.clone());
        let mono = self.changes.font_mono.clone().unwrap_or_else(|| self.defaults.font_mono.clone());
        out.push_str(&format!(
            "\n## Font\n\n| token | value | default | use |\n|---|---|---|---|\n| font | {font} | {} | {} |\n| font_mono | {mono} | {} | {} |\n",
            self.defaults.font,
            note(FONT),
            self.defaults.font_mono,
            note(FONT_MONO)
        ));
        out
    }
}

impl Overrides {
    /// Read an override set: the same `## Colors` / `## Sizes` / `## Font`
    /// tables as a theme file, but only the rows it wants to change, and
    /// only the value columns are read (`| token | light | dark | ... |`,
    /// `| token | value | ... |`). Default and note columns may be there
    /// or not, so a whole theme file is also a valid override set. A name
    /// that is not a token, or a value that does not parse, is an error
    /// naming its line: a typo in an override would otherwise do nothing.
    ///
    /// A `## Scene` table holds the app settings it overrides:
    /// `| scene | abyss |` (or `none`) and `| motion | 0 |`. An old
    /// `## Sizes` `motion` row is the same as the latter; old `scene_1` ...
    /// `scene_6` colour rows are skipped.
    pub fn from_md(text: &str) -> Result<Overrides, String> {
        let mut out = Overrides::default();
        let mut section = "";
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if let Some(heading) = line.strip_prefix("## ") {
                section = match heading.trim() {
                    "Colors" => "colors",
                    "Sizes" => "sizes",
                    "Font" => "font",
                    "Scene" => "scene",
                    _ => "",
                };
                continue;
            }
            if !line.starts_with('|') || section.is_empty() {
                continue;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if matches!(cells[0], "token" | "setting") || cells[0].starts_with('-') {
                continue;
            }
            let bad = |what: &str| format!("line {}: {what}", n + 1);
            let name = cells[0];
            let motion = |value: &str| value.parse::<f32>().ok().filter(|v| *v >= 0.0).ok_or_else(|| bad(&format!("motion \"{value}\" is not a number of seconds")));
            match section {
                "colors" if LEGACY_COLOR_NAMES.contains(&name) => {}
                "sizes" if name == MOTION => {
                    let [_, value, ..] = cells[..] else { return Err(bad("a size row needs a value")) };
                    out.motion = Some(motion(value)?);
                }
                "colors" => {
                    if !COLOR_NAMES.contains(&name) {
                        return Err(bad(&format!("no colour token \"{name}\"")));
                    }
                    let [_, light, dark, ..] = cells[..] else { return Err(bad("a colour row needs a light and a dark value")) };
                    let hex = |s: &str| parse_hex(s).ok_or_else(|| bad(&format!("\"{s}\" is not a colour")));
                    out.colors.insert((Mode::Light, name.to_string()), hex(light)?);
                    out.colors.insert((Mode::Dark, name.to_string()), hex(dark)?);
                }
                "sizes" => {
                    if !NUMBER_NAMES.contains(&name) {
                        return Err(bad(&format!("no size token \"{name}\"")));
                    }
                    let [_, value, ..] = cells[..] else { return Err(bad("a size row needs a value")) };
                    let v = value.parse::<f32>().map_err(|_| bad(&format!("\"{value}\" is not a number")))?;
                    out.numbers.insert(name.to_string(), v);
                }
                "scene" => {
                    let [_, value, ..] = cells[..] else { return Err(bad("a scene row needs a value")) };
                    match name {
                        SCENE if valid_scene_name(value) => out.scene = Some(value.to_string()),
                        MOTION => out.motion = Some(motion(value)?),
                        _ => return Err(bad(&format!("\"{name} {value}\" is not a scene row (scene <name>|none, motion <seconds>)"))),
                    }
                }
                _ => {
                    let [_, value, ..] = cells[..] else { return Err(bad("a font row needs a value")) };
                    match name {
                        FONT => out.font = Some(value.to_string()),
                        FONT_MONO => out.font_mono = Some(value.to_string()),
                        _ => return Err(bad(&format!("no font token \"{name}\""))),
                    }
                }
            }
        }
        Ok(out)
    }

    /// Nothing changed.
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty() && self.numbers.is_empty() && self.font.is_none() && self.font_mono.is_none() && self.scene.is_none() && self.motion.is_none()
    }
}

/// `theme` with each override set laid over it, later ones winning: what
/// a route with its own overrides is drawn with. The theme itself is not
/// touched, so leaving the route (drawing with `theme` again) restores it,
/// and the token editor never writes a route's look into the theme file.
/// A layer's scene settings are not theme tokens: `crate::scene::settings`
/// reads them from the same layers.
pub fn layered(theme: &Theme, layers: &[&Overrides]) -> Theme {
    let mut out = theme.clone();
    for layer in layers {
        out.changes.colors.extend(layer.colors.iter().map(|(k, v)| (k.clone(), *v)));
        out.changes.numbers.extend(layer.numbers.iter().map(|(k, v)| (k.clone(), *v)));
        if layer.font.is_some() {
            out.changes.font.clone_from(&layer.font);
        }
        if layer.font_mono.is_some() {
            out.changes.font_mono.clone_from(&layer.font_mono);
        }
    }
    out
}

/// A scene name is one plain file in `scenes/` (letters, digits, `-`, `_`).
pub fn valid_scene_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Where a font option comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSource {
    /// This machine's own choice (`state/local.md`, not shared).
    Local,
    /// The theme's value (or a route's override of it).
    Theme,
    /// The theme's default.
    Default,
    /// A font file shipped in `assets/fonts`.
    Shipped,
    /// Always there: the last resort.
    Fallback,
}

impl FontSource {
    pub fn word(self) -> &'static str {
        match self {
            FontSource::Local => "local",
            FontSource::Theme => "theme",
            FontSource::Default => "default",
            FontSource::Shipped => "shipped",
            FontSource::Fallback => "fallback",
        }
    }
}

/// One family a font token could be drawn in, and whether this machine
/// has it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontOption {
    pub family: String,
    pub source: FontSource,
    pub available: bool,
}

/// The options for font token `token` (`FONT` or `FONT_MONO`), best first:
/// the value `theme` resolves to (labelled local when `local` set it), its
/// default, the shipped fonts, then `fallbacks`. Each family once, each
/// checked with `has`. What is drawn is the first available
/// (`pick_font`), so a saved choice that is not on this machine falls back
/// instead of drawing nothing.
pub fn font_options(theme: &Theme, token: &str, local: Option<&Overrides>, shipped: &[String], fallbacks: &[&str], has: impl Fn(&str) -> bool) -> Vec<FontOption> {
    let mono = token == FONT_MONO;
    let (changed, default) = if mono { (&theme.changes.font_mono, &theme.defaults.font_mono) } else { (&theme.changes.font, &theme.defaults.font) };
    let local = local.and_then(|l| if mono { l.font_mono.as_ref() } else { l.font.as_ref() });
    let mut wanted: Vec<(String, FontSource)> = Vec::new();
    if let Some(v) = changed {
        wanted.push((v.clone(), if local == Some(v) { FontSource::Local } else { FontSource::Theme }));
    }
    wanted.push((default.clone(), FontSource::Default));
    wanted.extend(shipped.iter().map(|f| (f.clone(), FontSource::Shipped)));
    wanted.extend(fallbacks.iter().map(|f| (f.to_string(), FontSource::Fallback)));
    let mut out: Vec<FontOption> = Vec::new();
    for (family, source) in wanted {
        if !family.is_empty() && !out.iter().any(|o| o.family == family) {
            let available = has(&family);
            out.push(FontOption { family, source, available });
        }
    }
    out
}

/// The option to draw with: the first one this machine has.
pub fn pick_font(options: &[FontOption]) -> Option<&FontOption> {
    options.iter().find(|o| o.available)
}

/// The text above the tables in the theme file.
const MD_INTRO: &str = "# Theme

One theme: the design tokens for the whole app. Every colour, size and the font are read from the active theme (`themes/active` names it); the UI tokens page (`/settings/ui`) and `cargo run -p dry-tools -- token ...` write back to it. A value that differs from its default is a change; reset puts the default back.

Colours are `#RRGGBBAA`. Text outside the tables is rewritten when the tools save.
";

fn resolve_color(theme: &Theme, mode: Mode, name: &str) -> u32 {
    theme.changes.colors.get(&(mode, name.to_string())).copied().or_else(|| theme.default_color(name, mode)).unwrap_or(0xFF00FFFF)
}

/// The resolved tokens for one mode: what every component reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub mode: Mode,
    // Colours (0xRRGGBBAA)
    pub background_body: u32,
    pub background_surface: u32,
    pub background_muted: u32,
    pub overlay_hover: u32,
    pub overlay_pressed: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub text_disabled: u32,
    pub text_accent: u32,
    pub accent: u32,
    pub accent_muted: u32,
    pub on_accent: u32,
    pub border: u32,
    pub border_emphasized: u32,
    pub error: u32,
    pub shadow: u32,
    pub window_tint: u32,
    pub glass_fill: u32,
    pub glass_sheen: u32,
    pub glass_edge: u32,
    pub glass_highlight: u32,
    /// The light behind the glass (`crate::backdrop`), one colour per blob.
    pub backdrop: [u32; 3],
    /// The status strip at the bottom of an app-like page, and its text.
    pub status_bar: u32,
    pub on_status_bar: u32,
    /// The line the cursor is on, in an editor.
    pub line_highlight: u32,
    /// What an engine is doing (`crate::activity`): a translucent tint
    /// behind its card, nav entry and page header, one per activity, in
    /// `crate::activity::TOKENS` order (llm, tools, compress, wait, work,
    /// need, fail).
    pub activity: [u32; 7],
    // Spacing, in logical pixels: a 4px scale.
    pub space_1: f32,
    pub space_2: f32,
    pub space_3: f32,
    pub space_4: f32,
    // Corner radius.
    pub radius: f32,
    // Type sizes.
    pub text_sm: f32,
    pub text_base: f32,
    /// Code: an editor's lines and a terminal's output.
    pub text_code: f32,
    pub text_lg: f32,
    // Glass panes.
    pub pane_gap: f32,
    pub pane_radius: f32,
    /// How the navigation sits: 1 pinned as a sidebar, 0 a floating
    /// button that opens on hover (`layout::NavStyle`).
    pub nav_pinned: f32,
    /// The font family to draw text with.
    pub font_family: String,
    /// The monospace font family code is drawn with.
    pub font_mono: String,
}

impl Tokens {
    /// A colour token's value by its name; None for a name that is not one.
    /// The one lookup by name, so a scene's palette, the token editor and
    /// the tools all read colours the same way.
    pub fn color(&self, name: &str) -> Option<u32> {
        Some(match name {
            "background_body" => self.background_body,
            "background_surface" => self.background_surface,
            "background_muted" => self.background_muted,
            "overlay_hover" => self.overlay_hover,
            "overlay_pressed" => self.overlay_pressed,
            "text_primary" => self.text_primary,
            "text_secondary" => self.text_secondary,
            "text_disabled" => self.text_disabled,
            "text_accent" => self.text_accent,
            "accent" => self.accent,
            "accent_muted" => self.accent_muted,
            "on_accent" => self.on_accent,
            "border" => self.border,
            "border_emphasized" => self.border_emphasized,
            "error" => self.error,
            "shadow" => self.shadow,
            "window_tint" => self.window_tint,
            "glass_fill" => self.glass_fill,
            "glass_sheen" => self.glass_sheen,
            "glass_edge" => self.glass_edge,
            "glass_highlight" => self.glass_highlight,
            "backdrop_1" => self.backdrop[0],
            "backdrop_2" => self.backdrop[1],
            "backdrop_3" => self.backdrop[2],
            "status_bar" => self.status_bar,
            "on_status_bar" => self.on_status_bar,
            "line_highlight" => self.line_highlight,
            "activity_llm" => self.activity[0],
            "activity_tools" => self.activity[1],
            "activity_compress" => self.activity[2],
            "activity_wait" => self.activity[3],
            "activity_work" => self.activity[4],
            "activity_need" => self.activity[5],
            "activity_fail" => self.activity[6],
            _ => return None,
        })
    }

    /// A size token's value by its name; None for a name that is not one.
    pub fn number(&self, name: &str) -> Option<f32> {
        Some(match name {
            "space_1" => self.space_1,
            "space_2" => self.space_2,
            "space_3" => self.space_3,
            "space_4" => self.space_4,
            "radius" => self.radius,
            "text_sm" => self.text_sm,
            "text_base" => self.text_base,
            "text_code" => self.text_code,
            "text_lg" => self.text_lg,
            "pane_gap" => self.pane_gap,
            "pane_radius" => self.pane_radius,
            "nav_pinned" => self.nav_pinned,
            _ => return None,
        })
    }
}

/// The defaults alone for `mode` (no changes).
pub fn defaults(theme: &Theme, mode: Mode) -> Tokens {
    resolve(mode, &Theme { defaults: theme.defaults.clone(), changes: Overrides::default() })
}

/// The theme's tokens for `mode`: each token's change, else its default.
/// A token the theme file lacks is magenta (colour) or 0 (size), so it shows.
pub fn resolve(mode: Mode, theme: &Theme) -> Tokens {
    let c = |name: &str| resolve_color(theme, mode, name);
    let n = |name: &str| theme.changes.numbers.get(name).copied().or_else(|| theme.default_number(name)).unwrap_or(0.0);
    Tokens {
        mode,
        background_body: c("background_body"),
        background_surface: c("background_surface"),
        background_muted: c("background_muted"),
        overlay_hover: c("overlay_hover"),
        overlay_pressed: c("overlay_pressed"),
        text_primary: c("text_primary"),
        text_secondary: c("text_secondary"),
        text_disabled: c("text_disabled"),
        text_accent: c("text_accent"),
        accent: c("accent"),
        accent_muted: c("accent_muted"),
        on_accent: c("on_accent"),
        border: c("border"),
        border_emphasized: c("border_emphasized"),
        error: c("error"),
        shadow: c("shadow"),
        window_tint: c("window_tint"),
        glass_fill: c("glass_fill"),
        glass_sheen: c("glass_sheen"),
        glass_edge: c("glass_edge"),
        glass_highlight: c("glass_highlight"),
        backdrop: [c("backdrop_1"), c("backdrop_2"), c("backdrop_3")],
        status_bar: c("status_bar"),
        on_status_bar: c("on_status_bar"),
        line_highlight: c("line_highlight"),
        activity: crate::activity::TOKENS.map(c),
        space_1: n("space_1"),
        space_2: n("space_2"),
        space_3: n("space_3"),
        space_4: n("space_4"),
        radius: n("radius"),
        text_sm: n("text_sm"),
        text_base: n("text_base"),
        text_code: n("text_code"),
        text_lg: n("text_lg"),
        pane_gap: n("pane_gap"),
        pane_radius: n("pane_radius"),
        nav_pinned: n("nav_pinned"),
        font_family: theme.changes.font.clone().unwrap_or_else(|| theme.defaults.font.clone()),
        font_mono: theme.changes.font_mono.clone().unwrap_or_else(|| theme.defaults.font_mono.clone()),
    }
}

/// `0xRRGGBBAA` as `#RRGGBBAA`.
pub fn hex(value: u32) -> String {
    format!("#{value:08X}")
}

/// Read `#RRGGBB` (opaque) or `#RRGGBBAA`; the `#` is optional.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    let value = u32::from_str_radix(digits, 16).ok()?;
    match digits.len() {
        6 => Some((value << 8) | 0xFF),
        8 => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default theme file the app ships with.
    fn shipped() -> Theme {
        Theme::from_md(include_str!("../../../themes/default.md")).expect("themes/default.md parses")
    }

    /// Every theme file the app ships with, by name.
    const SHIPPED: &[(&str, &str)] = &[
        ("default", include_str!("../../../themes/default.md")),
        ("terminal", include_str!("../../../themes/terminal.md")),
        ("cat", include_str!("../../../themes/cat.md")),
        ("abyss", include_str!("../../../themes/abyss.md")),
    ];

    #[test]
    fn an_older_theme_with_scene_rows_still_reads_and_drops_them() {
        // The rows themes had before the scene was a setting with its own colours.
        let old = format!(
            "{}\n| scene_1 | #D9733FFF | #E8875AFF | #D9733FFF | #E8875AFF | fur |\n\n## Sizes\n| token | value | default |\n| motion | 30 | 40 |\n\n## Scene\n\n| token | value | default | use |\n|---|---|---|---|\n| scene | cat | cat | x |\n",
            SHIPPED[0].1.split("\n## Sizes").next().unwrap()
        );
        let t = Theme::from_md(&old).unwrap();
        assert!(!t.defaults.colors.contains_key("scene_1"), "the scene ink is dropped");
        assert_eq!(t.legacy_motion(), Some(30.0), "the old motion row is still honoured");
        assert!(!t.to_md().contains("## Scene") && !t.to_md().contains("| motion |"), "and neither is written back");
        assert!(Theme::default().set("scene", "cat", None).is_err(), "the scene is no theme token");
        let o = Overrides::from_md("## Colors\n| scene_2 | #000000 | #000000 |\n## Sizes\n| motion | 0 |\n").unwrap();
        assert_eq!((o.colors.len(), o.motion), (0, Some(0.0)), "an old override's rows: ink skipped, motion kept");
    }

    #[test]
    fn a_scene_table_in_an_override_set_holds_the_scene_settings() {
        let o = Overrides::from_md("## Scene\n| setting | value |\n|---|---|\n| scene | none |\n| motion | 12 |\n").unwrap();
        assert_eq!((o.scene.as_deref(), o.motion), (Some("none"), Some(12.0)));
        assert!(!o.is_empty());
        assert!(Overrides::from_md("## Scene\n| scene | ../x |\n").is_err());
        assert!(Overrides::from_md("## Scene\n| motion | fast |\n").is_err());
        assert!(Overrides::from_md("## Scene\n| speed | 1 |\n").is_err());
        // Laying it over a theme changes no token.
        assert_eq!(layered(&shipped(), &[&o]), shipped());
    }

    #[test]
    fn every_shipped_theme_parses_has_every_token_and_no_changes() {
        for (name, text) in SHIPPED {
            let theme = Theme::from_md(text).unwrap_or_else(|e| panic!("themes/{name}.md: {e}"));
            assert_eq!(theme.missing(), Vec::<&str>::new(), "themes/{name}.md");
            assert_eq!(theme.changes, Overrides::default(), "themes/{name}.md ships its values as its defaults");
            assert_eq!(Theme::from_md(&theme.to_md()).unwrap(), theme, "themes/{name}.md writes back the same");
        }
    }

    #[test]
    fn the_terminal_theme_is_flat_black_and_monospace() {
        let theme = Theme::from_md(SHIPPED[1].1).unwrap();
        let dark = resolve(Mode::Dark, &theme);
        assert_eq!(dark.background_body, 0x000000FF);
        assert_eq!(dark.font_family, "Menlo");
        assert_eq!(dark.glass_sheen, dark.glass_fill, "no sheen");
        assert_eq!(dark.radius, 0.0);
        assert_ne!(resolve(Mode::Light, &theme).background_body, dark.background_body);
    }

    #[test]
    fn the_shipped_theme_md_has_every_token_and_writes_back_the_same() {
        let theme = shipped();
        assert_eq!(theme.missing(), Vec::<&str>::new());
        assert_eq!(Theme::from_md(&theme.to_md()).unwrap(), theme);
        assert_eq!(theme.defaults.font, ".SystemUIFont");
    }

    #[test]
    fn one_mode_change_swaps_every_colour_and_keeps_sizes() {
        let theme = shipped();
        let (light, dark) = (resolve(Mode::Light, &theme), resolve(Mode::Dark, &theme));
        assert_ne!(light.background_body, dark.background_body);
        assert_eq!(light.space_3, dark.space_3);
        assert_eq!(Mode::Light.toggled(), Mode::Dark);
        assert_eq!(Mode::parse(Mode::Dark.as_str()), Some(Mode::Dark));
    }

    #[test]
    fn a_change_is_a_value_that_differs_from_its_default_and_reset_removes_it() {
        let mut theme = shipped();
        theme.set("accent", "#FF0000", Some(Mode::Dark)).unwrap();
        assert_eq!(resolve(Mode::Dark, &theme).accent, 0xFF0000FF);
        assert_eq!(resolve(Mode::Light, &theme).accent, defaults(&theme, Mode::Light).accent, "light keeps its default");
        assert!(theme.to_md().contains("| accent | #0A84FFFF | #FF0000FF | #0A84FFFF |"), "{}", theme.to_md());
        let read = Theme::from_md(&theme.to_md()).unwrap();
        assert!(read.is_changed(Mode::Dark, "accent") && !read.is_changed(Mode::Light, "accent"));
        theme.reset(Some("accent"));
        assert!(theme.changes.colors.is_empty());
        theme.set("radius", "10", None).unwrap();
        theme.set("font", "Inter", None).unwrap();
        assert_eq!(resolve(Mode::Light, &theme).radius, 10.0);
        theme.reset(None);
        assert_eq!(theme, shipped());
        assert!(theme.set("nope", "1", None).is_err());
        assert!(theme.set("accent", "blue", None).is_err());
    }

    #[test]
    fn an_override_set_changes_only_its_rows_and_leaving_restores_the_theme() {
        let theme = shipped();
        let set = Overrides::from_md(
            "## Colors\n| token | light | dark | use |\n|---|---|---|---|\n| accent | #FF0000 | #00FF00 | x |\n\n## Sizes\n| token | value |\n| radius | 0 |\n\n## Font\n| token | value |\n| font | Menlo |\n",
        )
        .unwrap();
        let drawn = layered(&theme, &[&set]);
        let (light, dark) = (resolve(Mode::Light, &drawn), resolve(Mode::Dark, &drawn));
        assert_eq!((light.accent, dark.accent), (0xFF0000FF, 0x00FF00FF));
        assert_eq!((dark.radius, dark.font_family.as_str()), (0.0, "Menlo"));
        assert_eq!(dark.text_primary, resolve(Mode::Dark, &theme).text_primary, "rows it does not name keep the theme's value");
        assert_eq!(theme, shipped(), "the theme itself is untouched");
    }

    #[test]
    fn later_override_sets_win_and_a_whole_theme_file_is_also_one() {
        let base = Overrides::from_md("## Sizes\n| radius | 0 |\n| space_1 | 2 |\n").unwrap();
        let top = Overrides::from_md("## Sizes\n| radius | 3 |\n").unwrap();
        let t = resolve(Mode::Dark, &layered(&shipped(), &[&base, &top]));
        assert_eq!((t.radius, t.space_1), (3.0, 2.0));
        let whole = Overrides::from_md(SHIPPED[1].1).unwrap();
        assert_eq!(resolve(Mode::Dark, &layered(&shipped(), &[&whole])), resolve(Mode::Dark, &Theme::from_md(SHIPPED[1].1).unwrap()));
        assert!(Overrides::default().is_empty() && !base.is_empty());
    }

    #[test]
    fn an_override_with_a_typo_says_which_line() {
        let err = Overrides::from_md("## Colors\n| acent | #000000 | #000000 |\n").unwrap_err();
        assert!(err.contains("line 2") && err.contains("acent"), "{err}");
        assert!(Overrides::from_md("## Sizes\n| radius | round |\n").is_err());
    }

    #[test]
    fn a_bad_row_names_its_line() {
        let err = Theme::from_md("## Sizes\n| token | value | default |\n| radius | big | 6 |\n").unwrap_err();
        assert!(err.contains("line 3"), "{err}");
    }

    #[test]
    fn hex_reads_six_or_eight_digits_and_writes_eight() {
        assert_eq!(parse_hex("#0064E0"), Some(0x0064E0FF));
        assert_eq!(parse_hex("0064e033"), Some(0x0064E033));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(hex(0x0064E0FF), "#0064E0FF");
    }

    #[test]
    fn a_font_token_lists_its_options_and_draws_the_first_it_has() {
        let mut theme = Theme::default();
        theme.defaults.font = ".SystemUIFont".into();
        theme.defaults.font_mono = "Menlo".into();
        let local = Overrides { font: Some("Amarante".into()), ..Overrides::default() };
        let on_top = layered(&theme, &[&local]);
        let shipped = ["Amarante".to_string()];
        let opts = font_options(&on_top, FONT, Some(&local), &shipped, &[".SystemUIFont"], |f| f != "Nope");
        let seen: Vec<(&str, FontSource)> = opts.iter().map(|o| (o.family.as_str(), o.source)).collect();
        assert_eq!(seen, [("Amarante", FontSource::Local), (".SystemUIFont", FontSource::Default)], "each family once, best first");
        assert_eq!(pick_font(&opts).unwrap().family, "Amarante");

        let gone = Overrides { font: Some("Nope".into()), ..Overrides::default() };
        let opts = font_options(&layered(&theme, &[&gone]), FONT, Some(&gone), &shipped, &[], |f| f != "Nope");
        assert!(!opts[0].available && opts[0].source == FontSource::Local);
        assert_eq!(pick_font(&opts).unwrap().family, ".SystemUIFont", "a saved font this machine lacks falls back to the default");

        let mono = font_options(&theme, FONT_MONO, None, &shipped, &["Menlo", ".SystemUIFont"], |_| true);
        assert_eq!(mono.iter().map(|o| o.family.as_str()).collect::<Vec<_>>(), ["Menlo", "Amarante", ".SystemUIFont"]);
    }

}
