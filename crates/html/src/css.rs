//! The look as CSS, made from the same numbers the window draws with.
//!
//! - `tokens`: every design token as a custom property (`--text_primary`,
//!   `--space_2`, `--font`), colours once per mode under
//!   `:root[data-mode=light|dark]`, so the page follows the app's mode by
//!   one attribute. Written per page: a route's own overrides and this
//!   machine's `state/local.md` are already laid over the theme. The
//!   motion setting (`dry_core::scene::Settings`, not a theme token) is
//!   `--motion` and the backdrop's timing.
//! - `scene`: the scene's colours, resolved for each mode (its own, or the
//!   theme's for a role), as `--scene-<key>`: what `assets/scene.js` paints
//!   with, swapped with the tokens on every live update.
//! - `frame`: the layout standard (`dry_core::layout`) as rules: the
//!   insets, the navigation's width and where it opens on each screen
//!   class, the header and footer heights, the content box's cap and gutter.
//! - `backdrop`: the moving light behind the glass (`dry_core::backdrop`),
//!   each layer's closed loop sampled into a CSS animation.
//! - `fonts`: an `@font-face` for each font shipped in `assets/fonts/`
//!   (the shell says which files and where it serves them).
//!
//! The rest of the look (glass, cards, buttons) is plain CSS in
//! `assets/app.css`, written only in these properties.

use std::fmt::Write;

use dry_core::backdrop::LAYERS;
use dry_core::layout::{self, FOOTER_HEIGHT, HEADER_HEIGHT, INSET, NavStyle, PILL_OPEN, WIDE_PAGE};
use dry_core::scene::Scene;
use dry_core::screen::{COMPACT_BELOW, Screen, WIDE_FROM};
use dry_core::theme::{self, COLOR_NAMES, NUMBER_NAMES, Tokens};

/// The platform's own UI font, as the theme files name it.
const SYSTEM_FONT: &str = ".SystemUIFont";

/// A token colour (`0xRRGGBBAA`) as CSS.
pub fn color(value: u32) -> String {
    theme::hex(value)
}

/// A font token as a CSS font stack: the family, then the platform's.
pub fn font_stack(family: &str, mono: bool) -> String {
    let fallback = if mono { "ui-monospace, Menlo, Consolas, monospace" } else { "system-ui, -apple-system, \"Segoe UI\", sans-serif" };
    let clean: String = family.chars().filter(|c| !matches!(c, '"' | '\\' | '<' | '>' | ';' | '{' | '}')).collect();
    if clean.is_empty() || clean == SYSTEM_FONT {
        return fallback.to_string();
    }
    format!("\"{clean}\", {fallback}")
}

/// Every token as custom properties: the sizes and fonts once, the
/// colours of each mode under its own selector; `motion` (seconds, 0
/// still) as `--motion` and the backdrop's timing.
pub fn tokens(light: &Tokens, dark: &Tokens, motion: f32) -> String {
    let mut out = String::from(":root{");
    for name in NUMBER_NAMES {
        let v = dark.number(name).unwrap_or(0.0);
        // `nav_pinned` is not a length.
        let unit = if *name == "nav_pinned" { "" } else { "px" };
        let _ = write!(out, "--{name}:{v}{unit};");
    }
    let _ = write!(out, "--motion:{motion};");
    let _ = write!(out, "--font:{};--font_mono:{};", font_stack(&dark.font_family, false), font_stack(&dark.font_mono, true));
    out.push('}');
    for t in [light, dark] {
        let _ = write!(out, "\n:root[data-mode={}]{{color-scheme:{};", t.mode.as_str(), t.mode.as_str());
        for name in COLOR_NAMES {
            let _ = write!(out, "--{name}:{};", color(t.color(name).unwrap_or(0)));
        }
        out.push('}');
    }
    out.push('\n');
    out.push_str(&backdrop_timing(motion));
    out
}

/// The scene's colours for each mode (`Scene::colours`: its own, or the
/// theme's for a role) as `--scene-<key>` under the mode's selector.
pub fn scene(scene: &Scene, light: &Tokens, dark: &Tokens) -> String {
    let mut out = String::new();
    for t in [light, dark] {
        let _ = write!(out, ":root[data-mode={}]{{", t.mode.as_str());
        for (k, c) in scene.colours(t) {
            let _ = write!(out, "--{}:{};", dry_core::scene::css_var(k), color(c));
        }
        out.push_str("}\n");
    }
    out
}

/// The frame of `dry_core::layout`, as rules on `#nav`, `#pane`,
/// `#header`, `#main` and `#footer`. The window has no traffic lights,
/// so the parts start at the inset (the window's `PILL_TOP` is for those).
pub fn frame() -> String {
    let side = INSET + PILL_OPEN + INSET;
    let mut out = String::new();
    let _ = write!(
        out,
        "#nav{{position:fixed;top:{INSET}px;bottom:{INSET}px;left:{INSET}px;width:{PILL_OPEN}px;display:none;z-index:5}}\n\
         #pane{{position:fixed;top:{INSET}px;bottom:{INSET}px;left:{INSET}px;right:{INSET}px}}\n\
         #header{{height:{HEADER_HEIGHT}px}}\n#footer{{height:{FOOTER_HEIGHT}px}}\n\
         html.nav-open #nav,html.nav-pinned #nav{{display:flex}}\n\
         html.nav-pinned #pane{{left:{side}px}}\n\
         @media (min-width:{COMPACT_BELOW}px){{html.nav-open #pane{{left:{side}px}}}}\n\
         @media (max-width:{}px){{html.nav-open:not(.nav-pinned) #nav{{top:{}px;left:{}px;bottom:auto;max-height:480px;box-shadow:0 12px 36px var(--shadow),0 1px 3px var(--shadow)}}}}\n",
        COMPACT_BELOW - 0.02,
        INSET + HEADER_HEIGHT,
        INSET + 8.0,
    );
    // The content box: capped and guttered by screen class, as `layout::frame` says.
    let open = |s: Screen| layout::frame(s, false, NavStyle::Float);
    let rule = |f: layout::Frame| {
        let max = f.content_max.map_or("none".to_string(), |m| format!("{m}px"));
        format!("--content-max:{max};--gutter:{}px;", f.gutter)
    };
    let _ = writeln!(out, "#pane{{{}}}", rule(open(Screen::Compact)));
    let _ = writeln!(out, "@media (min-width:{COMPACT_BELOW}px){{#pane{{{}}}}}", rule(open(Screen::Regular)));
    let _ = writeln!(out, "@media (min-width:{WIDE_FROM}px){{#pane{{{}}}#pane[data-frame=wide]{{--content-max:{WIDE_PAGE}px}}}}", rule(open(Screen::Wide)));
    out.push_str(&backdrop_paths());
    out
}

/// Each backdrop layer's loop (`Layer::at`, a closed curve) sampled at
/// even steps into keyframes; its size and softness from its depth.
fn backdrop_paths() -> String {
    const STEPS: usize = 24;
    let mut out = String::new();
    for (n, l) in LAYERS.iter().enumerate() {
        let size = l.size() * 100.0;
        let _ = write!(
            out,
            "#backdrop .l{n}{{width:{size:.1}vmin;height:{size:.1}vmin;background:radial-gradient(circle,var(--backdrop_{}) 0%,transparent {:.0}%);animation-name:dry-backdrop-{n}}}\n@keyframes dry-backdrop-{n}{{",
            l.color + 1,
            40.0 + 30.0 * l.softness(),
        );
        for i in 0..=STEPS {
            let (x, y) = l.at(i as f32 / STEPS as f32);
            let _ = write!(out, "{:.2}%{{transform:translate(calc({:.2}vw - 50%),calc({:.2}vh - 50%))}}", i as f32 * 100.0 / STEPS as f32, x * 100.0, y * 100.0);
        }
        out.push_str("}\n");
    }
    out
}

/// How long each layer's loop lasts for `motion` (`Layer::period`); still
/// when it is 0.
fn backdrop_timing(motion: f32) -> String {
    let mut out = String::new();
    for (n, l) in LAYERS.iter().enumerate() {
        match l.period(motion) {
            Some(secs) => {
                let _ = writeln!(out, "#backdrop .l{n}{{animation-duration:{secs:.2}s}}");
            }
            None => {
                let (x, y) = l.at(0.0);
                let _ = writeln!(
                    out,
                    "#backdrop .l{n}{{animation:none;transform:translate(calc({:.2}vw - 50%),calc({:.2}vh - 50%))}}",
                    x * 100.0,
                    y * 100.0
                );
            }
        }
    }
    out
}

/// An `@font-face` for each `.ttf`/`.otf` in `files`, named by its file
/// (`Amarante-Regular.ttf` is family `Amarante`), served at `<base><file>`
/// (the server's `/assets/fonts/`, the browser's `fonts/`).
pub fn fonts(files: &[String], base: &str) -> String {
    let mut out = String::new();
    for file in files {
        let family = file.split(['-', '.']).next().unwrap_or(file).to_string();
        let format = if file.ends_with(".otf") { "opentype" } else { "truetype" };
        let _ = writeln!(out, "@font-face{{font-family:\"{family}\";src:url(\"{base}{file}\") format(\"{format}\");font-display:swap}}");
    }
    out
}

/// Which of `names` are font files a page may load: `.ttf`/`.otf`, no
/// folders, no hidden files; sorted.
pub fn font_files(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = names.into_iter().filter(|n| (n.ends_with(".ttf") || n.ends_with(".otf")) && !n.contains('/') && !n.starts_with('.')).collect();
    names.sort();
    names
}

/// The whole stylesheet: fonts (`fonts`, at `font_base`), the frame, then the look.
pub fn stylesheet(fonts_shipped: &[String], font_base: &str) -> String {
    format!("{}{}{}", fonts(fonts_shipped, font_base), frame(), include_str!("../assets/app.css"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::theme::Mode;

    /// The tokens of `theme` for both modes.
    fn for_theme(theme: &theme::Theme) -> String {
        tokens(&theme::resolve(Mode::Light, theme), &theme::resolve(Mode::Dark, theme), 40.0)
    }

    fn theme() -> theme::Theme {
        theme::Theme::from_md(include_str!("../../../themes/default.md")).unwrap()
    }

    #[test]
    fn every_token_becomes_a_custom_property_with_colours_per_mode() {
        let css = for_theme(&theme());
        for name in COLOR_NAMES {
            assert_eq!(css.matches(&format!("--{name}:#")).count(), 2, "{name} once per mode");
        }
        for name in NUMBER_NAMES {
            assert!(css.contains(&format!("--{name}:")), "{name}");
        }
        let light = css.split(":root[data-mode=light]").nth(1).unwrap().split('}').next().unwrap();
        let dark = css.split(":root[data-mode=dark]").nth(1).unwrap().split('}').next().unwrap();
        assert!(light.contains("--background_body:#F1F4F7FF;") && dark.contains("--background_body:#000000FF;"));
        assert!(css.contains("--space_2:8px;") && css.contains("--pane_radius:16px;") && css.contains("--motion:40;"));
        assert!(css.contains("--font:system-ui"), "the system font token is the platform's stack");
    }

    #[test]
    fn overrides_reach_the_css_and_a_still_backdrop_does_not_move() {
        let mut o = theme::Overrides { font: Some("Amarante".into()), ..Default::default() };
        o.colors.insert((Mode::Dark, "accent".into()), 0xFF0000FF);
        let css = for_theme(&theme::layered(&theme(), &[&o]));
        assert!(css.contains("--font:\"Amarante\", system-ui"));
        assert!(css.contains("--accent:#FF0000FF;"));
        let t = theme();
        let still = tokens(&theme::resolve(Mode::Light, &t), &theme::resolve(Mode::Dark, &t), 0.0);
        assert!(still.contains("animation:none") && still.contains("--motion:0;"), "motion 0 keeps the backdrop still");
        assert!(!for_theme(&theme()).contains("animation:none"));
        assert_eq!(font_stack("Evil\"};<x>", true), "\"Evilx\", ui-monospace, Menlo, Consolas, monospace");
    }

    #[test]
    fn the_scenes_colours_are_resolved_per_mode() {
        let cat = Scene::from_md("cat", include_str!("../../../scenes/cat.md")).unwrap();
        let t = theme();
        let css = scene(&cat, &theme::resolve(Mode::Light, &t), &theme::resolve(Mode::Dark, &t));
        let light = css.split(":root[data-mode=light]").nth(1).unwrap().split('}').next().unwrap();
        assert!(light.contains("--scene-f:#0A84FFFF;"), "the default theme's accent: a blue cat: {css}");
        assert!(css.contains(":root[data-mode=dark]{--scene-c:"));
    }

    #[test]
    fn the_frame_uses_the_layout_numbers() {
        let css = frame();
        assert!(css.contains(&format!("width:{PILL_OPEN}px")));
        assert!(css.contains(&format!("@media (min-width:{COMPACT_BELOW}px){{html.nav-open #pane{{left:{}px}}}}", INSET + PILL_OPEN + INSET)));
        assert!(css.contains("--content-max:680px;--gutter:24px;") && css.contains("--content-max:880px;--gutter:32px;"));
        assert!(css.contains("--content-max:none;--gutter:12px;"));
        assert_eq!(css.matches("@keyframes dry-backdrop-").count(), LAYERS.len());
    }
}
