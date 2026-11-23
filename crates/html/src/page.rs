//! One page as a whole HTML document, in the layout standard
//! (`dry_core::layout`, drawn by `crates/gpui/src/frame.rs` in the window):
//! the light behind the glass, the navigation (fixed routes, then LIVE:
//! one entry per engine with its activity's tint and status dot), and the
//! content pane with its header (the menu button and the Home › … trail),
//! the view, and the footer.
//!
//! The server's live script (`crates/server/assets/app.js`) swaps
//! `#tokens`, `#nav` and `#pane` from a fresh copy of this same page when
//! `/events` says something changed, so a live update is this function run
//! again. The browser (`crates/web`) sets the same three regions from
//! `parts` after every change.

use std::fmt::Write;

use dry_core::activity::Activity;
use dry_core::backdrop::LAYERS;
use dry_core::layout::NavStyle;
use dry_core::pages;
use dry_core::routes::{self, ViewState};
use dry_core::theme::{self, Mode, Theme};

use crate::world::World;
use crate::render::{self, Ctx, Query, esc, tint, tone};

/// The theme this page is drawn with, and the mode: the app's theme (or a
/// previewed one), the route's own overrides and chosen theme (or a
/// previewed one of its own), then this machine's `state/local.md`.
pub fn theme_of(world: &World, location: &str, query: &Query) -> Result<(Theme, Mode), String> {
    let mode = query.mode.unwrap_or(world.state.mode);
    let mut local = world.local_of(location).cloned();
    let own = query.theme.as_ref().is_some_and(|q| local.as_ref().is_some_and(|l| l.themes.contains_key(q)));
    if own && let Some(l) = local.as_mut() {
        l.active = query.theme.clone();
    }
    let base = match &query.theme {
        Some(name) if !own && world.themes.contains(name) => match world.named_themes.get(name) {
            Some(t) => t.clone()?,
            None => world.theme.clone()?,
        },
        _ => world.theme.clone()?,
    };
    let mut layers = local.as_ref().map(|l| l.layers()).unwrap_or_default();
    layers.extend(world.local_theme.as_ref());
    Ok((theme::layered(&base, &layers), mode))
}

/// The scene settings in force on `location` drawn with `theme`
/// (`dry_core::scene::settings`): its route's overrides, then this
/// machine's `state/local.md`, over the app's `app.md`.
pub fn scene_of(world: &World, location: &str, theme: &Theme) -> dry_core::scene::Settings {
    let mut layers = world.local_of(location).map(|l| l.layers()).unwrap_or_default();
    layers.extend(world.local_theme.as_ref());
    dry_core::scene::settings(&world.state, theme, &layers)
}

/// The scene the settings name, if it is there and reads.
pub fn scene_named<'a>(world: &'a World, settings: &dry_core::scene::Settings) -> Option<&'a dry_core::scene::Scene> {
    world.scenes.get(settings.scene.as_deref()?)?.as_ref().ok()
}

/// One page in the regions a shell swaps: what goes in `<head>` and the
/// three live regions (`#tokens`' CSS, `#nav`, `#pane`).
pub struct Parts {
    /// 404 when no route matches; 500 when the theme does not read.
    pub status: u16,
    pub title: String,
    /// `light` or `dark`: the `<html data-mode>`.
    pub mode: &'static str,
    /// The `<html>` class: `nav-pinned` or `nav-float`.
    pub nav_class: &'static str,
    /// Before the first paint, open the navigation where it was left
    /// (`nav-float` only): the script's body.
    pub nav_script: String,
    pub tokens: String,
    pub nav: String,
    pub pane: String,
    /// The scene setting's scene (`scenes/<name>.md`), empty for none (or
    /// one that does not read): the `<canvas id="scene" data-scene>` the
    /// server's page paints it on, with the colours `tokens` holds.
    pub scene: String,
    /// How the engines are doing (`dry_core::scene::mood`, Home's tiles'
    /// rule): `idle`, `working` or `needs-you`, the `<html data-mood>`.
    pub mood: &'static str,
}

/// The whole document for `location` (a path) with `query`, as the
/// server sends it. The status is 404 when no route matches.
pub fn document(world: &World, location: &str, query: &Query, generation: u64) -> (u16, String) {
    let p = match parts(world, location, query) {
        Ok(p) => p,
        Err(e) => return (500, broken(&e)),
    };
    let mut out = String::with_capacity(32 * 1024);
    let _ = write!(
        out,
        "<!doctype html><html lang=\"en\" data-mode=\"{mode}\" data-gen=\"{generation}\" data-mood=\"{mood}\" class=\"{nav_class}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title} · dry</title>\
         <link rel=\"stylesheet\" href=\"/assets/app.css\"><style id=\"tokens\">{tokens}</style>{script}\
         <script src=\"/assets/app.js\" defer></script><script src=\"/assets/scene.js\" defer></script></head><body>\
         <canvas id=\"scene\" data-scene=\"{scene}\" aria-hidden=\"true\"></canvas>",
        mode = p.mode,
        mood = p.mood,
        scene = esc(&p.scene),
        nav_class = p.nav_class,
        title = esc(&p.title),
        tokens = p.tokens,
        script = if p.nav_script.is_empty() { String::new() } else { format!("<script>{}</script>", p.nav_script) },
    );
    out.push_str(&backdrop());
    out.push_str(&p.nav);
    out.push_str(&p.pane);
    out.push_str("</body></html>");
    (p.status, out)
}

/// The page's parts, or why the theme does not read.
pub fn parts(world: &World, location: &str, query: &Query) -> Result<Parts, String> {
    // A moved route (`page.json`'s `moved`) draws the page it moved to.
    let moved = world.table.moved_to(location);
    let location = moved.as_deref().unwrap_or(location);
    let (theme, mode) = theme_of(world, location, query)?;
    let (light, dark) = (theme::resolve(Mode::Light, &theme), theme::resolve(Mode::Dark, &theme));
    let t = if mode == Mode::Light { &light } else { &dark };
    let nav = NavStyle::from_token(t.nav_pinned);
    let look = scene_of(world, location, &theme);
    let scene = scene_named(world, &look);

    let resolved = world.resolve(location);
    let names = world.engine_names();
    let mut trail = routes::trail_titled(&world.table, location, &names);
    let here = resolved.is_some() && trail.last().is_some_and(|(p, _)| p == location);
    let title = match &resolved {
        Some(_) if here => trail.pop().map(|(_, title)| title).unwrap_or_default(),
        Some((route, _, _)) => route.title.clone(),
        None => "Not found".to_string(),
    };
    let frame = match &resolved {
        Some((_, _, view)) if pages::fills(view) => "fill",
        Some((_, _, view)) if pages::wide(view) => "wide",
        _ => "plain",
    };

    let empty = routes::Params::default();
    let params = resolved.as_ref().map_or(&empty, |(_, p, _)| p);
    let cx = Ctx::new(world, location, params, query, t);
    let mut body = String::new();
    match &resolved {
        Some((_, _, view)) => render::view(&mut body, view, &cx),
        None => render::missing(&mut body, location),
    }

    let mut nav_out = String::new();
    nav_html(&mut nav_out, world, location, &cx);
    let mut out = String::with_capacity(32 * 1024);
    let _ = write!(out, "<div id=\"pane\" class=\"glass\" data-frame=\"{frame}\"><header id=\"header\">");
    if nav == NavStyle::Float {
        out.push_str("<button class=\"btn nav-button\" data-nav-toggle type=\"button\"><span class=\"when-closed\"><span aria-hidden=\"true\">☰&nbsp; </span><span class=\"lbl\">Menu</span></span><span class=\"when-open\"><span aria-hidden=\"true\">✕&nbsp; </span><span class=\"lbl\">Close</span></span></button>");
    }
    out.push_str("<div class=\"trail\">");
    for (path, name) in trail {
        let _ = write!(out, "<a href=\"{}\">{}</a><span class=\"sep\">›</span>", esc(&path), esc(&name));
    }
    let chip = if world.hosted() {
        format!("<span class=\"chip\" title=\"this server runs the turns and is the one writer of {}\">host</span>", esc(&world.store))
    } else if world.live() {
        format!("<span class=\"chip\" title=\"turns run in this page; state is kept in {}\">in this browser</span>", esc(&world.store))
    } else {
        format!("<span class=\"chip ro\" title=\"{}\">read-only</span>", esc(render::READ_ONLY))
    };
    let _ = write!(out, "<b class=\"here\">{}</b></div>{chip}</header><main id=\"main\"><div class=\"content\">{body}</div></main>", esc(&title));
    let store = match (&world.store_error, world.live()) {
        (Some(e), _) => format!("Store problem: {e}"),
        (None, true) if world.hosted() => format!("Saved by the host · {}", world.store),
        (None, true) => format!("Kept in this browser · {}", world.store),
        (None, false) => format!("Read-only view · {}", world.store),
    };
    let (live_title, live_word) = if world.live() && !world.hosted() { ("the model server this page calls", "local") } else { ("live updates from /events", "connecting") };
    // The store's place in full on hover; its last two parts in the line.
    let _ = write!(
        out,
        "<footer id=\"footer\"><span class=\"trunc grow\" title=\"{}\">{}</span><span id=\"live\" class=\"live\" title=\"{live_title}\"><span class=\"dot\">●</span><span class=\"word\">{live_word}</span></span></footer></div>",
        esc(&store),
        esc(&short_store(&store))
    );
    Ok(Parts {
        status: if resolved.is_some() { 200 } else { 404 },
        title,
        mode: mode.as_str(),
        nav_class: if nav == NavStyle::Pinned { "nav-pinned" } else { "nav-float" },
        // Before the first paint: open the navigation where the person left
        // it, else where the screen has room (compact screens start closed).
        nav_script: if nav == NavStyle::Float {
            format!(
                "(function(){{var d=document.documentElement,s=null;try{{s=localStorage.getItem('dry.nav')}}catch(e){{}}if(s?s==='open':innerWidth>={})d.classList.add('nav-open')}})()",
                dry_core::screen::COMPACT_BELOW
            )
        } else {
            String::new()
        },
        tokens: crate::css::tokens(&light, &dark, look.motion) + &scene.map(|s| crate::css::scene(s, &light, &dark)).unwrap_or_default(),
        nav: nav_out,
        pane: out,
        scene: scene.map(|s| s.name.clone()).unwrap_or_default(),
        mood: mood_word(dry_core::scene::mood(&(0..world.state.engines.len()).map(|i| cx.status(i)).collect::<Vec<_>>())),
    })
}

/// The mood as the page's attribute says it.
pub fn mood_word(m: dry_core::scene::Mood) -> &'static str {
    match m {
        dry_core::scene::Mood::Idle => "idle",
        dry_core::scene::Mood::Working => "working",
        dry_core::scene::Mood::NeedsYou => "needs-you",
    }
}

/// A footer line with the store's path cut to its last two parts
/// (`Saved by the host · md:…/root/state`); anything else as it is.
fn short_store(line: &str) -> String {
    let Some(at) = ["md:", "sqlite:"].iter().filter_map(|k| line.find(k).map(|i| i + k.len())).min() else { return line.to_string() };
    let (head, path) = line.split_at(at);
    let parts: Vec<&str> = path.trim_end_matches('/').rsplit('/').take(3).collect();
    if parts.len() < 3 {
        return line.to_string();
    }
    format!("{head}…/{}/{}", parts[1], parts[0])
}

/// The light behind the glass: one blob per layer (`dry_core::backdrop`).
pub fn backdrop() -> String {
    let mut out = String::from("<div id=\"backdrop\" aria-hidden=\"true\">");
    for n in 0..LAYERS.len() {
        let _ = write!(out, "<div class=\"l{n}\"></div>");
    }
    out.push_str("</div>");
    out
}

/// The navigation (`dry_core::routes::elements`): the fixed routes, then
/// under LIVE one entry per engine on its activity's tint, a few words of
/// what it does after its name and a dot in its status's tone. New thread
/// and New session are actions, so they are drawn off on a read-only page.
fn nav_html(out: &mut String, world: &World, location: &str, cx: &Ctx) {
    let view = ViewState { location: location.to_string(), ..world.view.clone() };
    let names = world.engine_names();
    out.push_str("<nav id=\"nav\" class=\"glass\">");
    let mut live = false;
    for el in routes::elements(&world.table, &view, &names) {
        if el.engine.is_some() && !live {
            live = true;
            out.push_str("<div class=\"nav-label\">LIVE</div>");
        }
        let activity = el.engine.map(|i| cx.activity(i)).unwrap_or_default();
        let label = match &activity {
            Activity::Idle => el.label.clone(),
            a => format!("{} · {}", el.label, a.short()),
        };
        let _ = write!(
            out,
            "<a class=\"nav-item{}{}\" href=\"{}\"{}><span class=\"nav-icon\">{}</span><span class=\"nav-text trunc\">{}</span>",
            if el.active { " on" } else { "" },
            if el.engine.is_some() { " tinted" } else { "" },
            esc(&el.path),
            tint(&activity),
            esc(&el.icon),
            esc(&label)
        );
        if let Some(i) = el.engine {
            let _ = write!(out, "<span class=\"nav-dot {}\">●</span>", tone(cx.status(i).tone()));
        }
        out.push_str("</a>");
    }
    if !live {
        out.push_str("<div class=\"nav-label\">LIVE</div>");
    }
    let programs: Vec<(String, String)> = world.sessions.as_ref().map_or(Vec::new(), |s| s.tools.iter().map(|t| (t.icon.clone(), t.name.clone())).collect());
    if world.live() {
        out.push_str("<button class=\"nav-item\" type=\"button\" data-act=\"new-thread\"><span class=\"nav-icon\">＋</span><span class=\"nav-text\">New thread</span></button>");
        match programs.as_slice() {
            [] => {}
            [(icon, name)] => {
                let _ = write!(
                    out,
                    "<button class=\"nav-item\" type=\"button\" data-act=\"new-session\" data-value=\"{}\" title=\"a {} session\"><span class=\"nav-icon\">{}</span><span class=\"nav-text\">New session</span></button>",
                    esc(name),
                    esc(name),
                    esc(icon)
                );
            }
            several => {
                // Several programs: the picker, one per program.
                out.push_str("<details class=\"nav-pick\"><summary class=\"nav-item\"><span class=\"nav-icon\">＋</span><span class=\"nav-text\">New session</span></summary>");
                for (icon, name) in several {
                    let _ = write!(
                        out,
                        "<button class=\"nav-item sub\" type=\"button\" data-act=\"new-session\" data-value=\"{}\"><span class=\"nav-icon\">{}</span><span class=\"nav-text trunc\">{}</span></button>",
                        esc(name),
                        esc(icon),
                        esc(name)
                    );
                }
                out.push_str("</details>");
            }
        }
        out.push_str("</nav>");
    } else {
        for label in std::iter::once("New thread").chain((!programs.is_empty()).then_some("New session")) {
            let _ = write!(out, "<span class=\"nav-item off\" title=\"{}\"><span class=\"nav-icon\">＋</span><span class=\"nav-text\">{label}</span></span>", esc(render::READ_ONLY));
        }
        out.push_str("</nav>");
    }
}

/// A page when the theme itself does not read: no tokens to draw with,
/// so plain text saying which file and why.
pub fn broken(why: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>dry</title></head><body style=\"font:14px system-ui;padding:24px\"><h1>The theme does not read</h1><p>{}</p><p>Fix the file; this page reloads when it changes.</p><script src=\"/assets/app.js\" defer></script></body></html>",
        esc(why)
    )
}
