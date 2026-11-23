//! App-wide design tokens for GPUI: the "head level" injection.
//!
//! The resolved `dry_core::theme::Tokens` live in one GPUI *global*, `Ui`.
//! A global is a value the whole app shares; any component reads it with
//! `ui::tokens(cx)`, so nothing below the top passes colours around or
//! writes raw `rgb(...)` values. Any change (mode, a token, the font) calls
//! `apply` once: it resolves defaults + overrides from the state, swaps the
//! global, mirrors the tokens into gpui-component's theme (so its Textarea,
//! inputs, ... match ours) and repaints every window.

use std::borrow::Cow;

use dry_core::theme::{self, Tokens};
use gpui::{App, Global, Hsla, rgba};
use gpui_component::{Theme, ThemeMode};

/// GPUI's name for the platform's own UI font.
const SYSTEM_FONT: &str = ".SystemUIFont";
/// Font files shipped with the app (see `assets/fonts/README.md`).
const FONT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");

/// The global that holds the current tokens.
pub struct Ui {
    pub tokens: Tokens,
    /// The font the tokens ask for, when it could not be found and the
    /// one drawn is another option.
    pub missing_font: Option<String>,
    /// Each font token's options, best first, each marked available or not.
    pub font_options: Vec<theme::FontOption>,
    pub mono_options: Vec<theme::FontOption>,
}

impl Global for Ui {}

/// The families loaded from `assets/fonts` (set by `load_fonts`).
struct Shipped(Vec<String>);

impl Global for Shipped {}

/// The current tokens. Every component that draws calls this.
pub fn tokens(cx: &App) -> Tokens {
    cx.global::<Ui>().tokens.clone()
}

/// A token colour (`0xRRGGBBAA`) as a GPUI colour.
pub fn color(value: u32) -> Hsla {
    rgba(value).into()
}

/// Load every font file in `assets/fonts` into GPUI's text system, so a
/// family that is not installed on the machine can still be used.
pub fn load_fonts(cx: &mut App) {
    cx.set_global(Shipped(Vec::new()));
    let Ok(entries) = std::fs::read_dir(FONT_DIR) else { return };
    let fonts: Vec<Cow<'static, [u8]>> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ttf" || x == "otf"))
        .filter_map(|p| std::fs::read(p).ok())
        .map(Cow::Owned)
        .collect();
    if fonts.is_empty() {
        return;
    }
    let before = cx.text_system().all_font_names();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("loading fonts failed: {e}");
    }
    // The families those files added: the shipped options.
    let shipped = cx.text_system().all_font_names().into_iter().filter(|n| !before.contains(n)).collect();
    cx.set_global(Shipped(shipped));
}

/// Inject the theme's tokens for `mode` (from the active theme file: defaults + the
/// changes, with `local`, this machine's choices, already laid over it)
/// into the whole app and repaint it.
pub fn apply(mode: theme::Mode, theme: &theme::Theme, local: Option<&theme::Overrides>, cx: &mut App) {
    let mut t = theme::resolve(mode, theme);

    // Each font token draws its first option GPUI knows (installed, or
    // loaded from assets): the chosen one, its default, a shipped font,
    // then Menlo (always on macOS) and the system font, which is always
    // there whatever it is called today.
    let names = cx.text_system().all_font_names();
    let has = |family: &str| family == SYSTEM_FONT || names.iter().any(|n| n == family);
    let shipped = cx.try_global::<Shipped>().map(|s| s.0.clone()).unwrap_or_default();
    let font_options = theme::font_options(theme, theme::FONT, local, &shipped, &[SYSTEM_FONT], has);
    let mono_options = theme::font_options(theme, theme::FONT_MONO, local, &[], &["Menlo", SYSTEM_FONT], has);
    let pick = |options: &[theme::FontOption]| theme::pick_font(options).map_or(SYSTEM_FONT.to_string(), |o| o.family.clone());
    let mut missing_font = None;
    for (family, options) in [(&mut t.font_family, &font_options), (&mut t.font_mono, &mono_options)] {
        let drawn = pick(options);
        if *family != drawn {
            eprintln!("font \"{family}\" not found; using {drawn}");
            missing_font.get_or_insert_with(|| family.clone());
            *family = drawn;
        }
    }

    // gpui-component keeps its own theme global; point it at the same mode,
    // then overwrite the colours and font its components use with ours.
    let component_mode = match mode {
        theme::Mode::Light => ThemeMode::Light,
        theme::Mode::Dark => ThemeMode::Dark,
    };
    Theme::change(component_mode, None, cx);
    let component = Theme::global_mut(cx);
    component.font_family = t.font_family.clone().into();
    let c = &mut component.colors;
    // Components (the text area) sit on glass panes, so their fill is the
    // pane's. Root paints the window with `tokens.background`: the
    // translucent tint, or it would cover the frosted window.
    c.background = color(t.glass_fill);
    component.tokens.background = color(t.window_tint).into();
    c.foreground = color(t.text_primary);
    c.border = color(t.border_emphasized);
    c.input = color(t.border_emphasized);
    c.caret = color(t.text_primary);
    c.ring = color(t.accent);
    c.selection = color(t.accent_muted);
    c.muted = color(t.background_muted);
    c.muted_foreground = color(t.text_secondary);
    c.accent = color(t.overlay_hover);
    c.accent_foreground = color(t.text_primary);
    c.primary = color(t.accent);
    c.primary_foreground = color(t.on_accent);
    c.popover = color(t.background_surface);
    c.popover_foreground = color(t.text_primary);

    cx.set_global(Ui { tokens: t, missing_font, font_options, mono_options });
    cx.refresh_windows();
}
