//! Where the window opens: the size and place it was left at, kept per
//! machine in `window.host.json` beside the repo (git ignores `*.host.json`),
//! so a restart (watchexec, `cargo run`) opens it as big as it was.

use std::path::PathBuf;

use gpui::{Bounds, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Place {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    #[serde(default)]
    maximized: bool,
}

fn file() -> PathBuf {
    dry_tools::root().join("window.host.json")
}

fn place(b: &WindowBounds) -> Place {
    let r = b.get_bounds();
    Place {
        x: f32::from(r.origin.x),
        y: f32::from(r.origin.y),
        w: f32::from(r.size.width),
        h: f32::from(r.size.height),
        maximized: matches!(b, WindowBounds::Maximized(_)),
    }
}

/// The saved place, when there is one that makes sense (a window too small
/// to use is not restored).
pub fn saved() -> Option<WindowBounds> {
    let p: Place = serde_json::from_str(&std::fs::read_to_string(file()).ok()?).ok()?;
    if p.w < 480. || p.h < 320. {
        return None;
    }
    let r = Bounds { origin: point(px(p.x), px(p.y)), size: size(px(p.w), px(p.h)) };
    Some(if p.maximized { WindowBounds::Maximized(r) } else { WindowBounds::Windowed(r) })
}

/// Keep the window's place now (only when it changed; fullscreen is not kept).
pub fn save(b: &WindowBounds) {
    if matches!(b, WindowBounds::Fullscreen(_)) {
        return;
    }
    let now = place(b);
    let path = file();
    let same = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Place>(&t).ok()).is_some_and(|old| old == now);
    if !same && let Ok(text) = serde_json::to_string(&now) {
        let _ = std::fs::write(path, text);
    }
}
