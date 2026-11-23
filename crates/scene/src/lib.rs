//! The scene for the pages dry-server draws: `dry_core::scene` (the
//! same `start`, `step` and `frame` the GPUI window and the browser build
//! call) behind a small C ABI, so the page's script can run it from
//! WebAssembly with no glue (`crates/server/assets/scene.js`).
//!
//! Why a module and not frames from the server or a JavaScript port: the
//! picture is pure in time and in the inputs, and the inputs include the
//! pointer, so it has to be computed where the pointer is (a frame over
//! the network would lag behind the mouse). A port would be a second cat
//! to keep in step with the first; this is the first one.
//!
//! The script's side, in order:
//!
//! 1. `scene_alloc(n)`, copy the scene file's text in, `scene_load(ptr, n)`:
//!    1 when it reads; else 0 and `scene_text` says why.
//! 2. `scene_palette()`: the palette as text in `scene_text`, one
//!    `key\tvar` line per key: the CSS custom property its colour is in
//!    (`scene-f`, `dry_core::scene::css_var`). The server resolves the
//!    colours (the scene's own, or the theme's for a role) into the page's
//!    tokens for each mode, so this module and the script only draw.
//! 3. `scene_set(...)` whenever an input changes (the mood, the area, the
//!    floor, the pointer): 1 when the picture changed, so the loop wakes.
//! 4. `scene_frame(t)`: how many rectangles; `scene_draws()` points at
//!    them, `STRIDE` floats each (x, y, w, h in cells, the palette key as a
//!    code point, alpha, glow); `scene_next()` when the picture next
//!    changes (-1: not until an input does, so the loop parks).
//!
//! `scene_source()` points at `SOURCE`: the hash of the scene code it was
//! built from, so the server's test can tell a stale `scene.wasm`.

use std::cell::RefCell;

use dry_core::scene::{self, Inputs, Live, Mood, Place, Pointer, Scene};

/// Floats per rectangle in `scene_draws`.
pub const STRIDE: usize = 7;

/// `dry-scene-src:` and the FNV-1a hash of the scene code (build.rs).
pub const SOURCE: &str = concat!("dry-scene-src:", env!("DRY_SCENE_SOURCE"));

#[derive(Default)]
struct Held {
    scene: Option<Scene>,
    live: Option<Live>,
    inputs: Option<Inputs>,
    draws: Vec<f32>,
    next: f64,
    text: String,
    buffers: Vec<Vec<u8>>,
}

thread_local! {
    static HELD: RefCell<Held> = RefCell::new(Held::default());
}

fn with<R>(f: impl FnOnce(&mut Held) -> R) -> R {
    HELD.with(|h| f(&mut h.borrow_mut()))
}

/// Room for `len` bytes the script writes into (kept until the next load).
#[unsafe(no_mangle)]
pub extern "C" fn scene_alloc(len: usize) -> *mut u8 {
    with(|h| {
        let mut b = vec![0u8; len.max(1)];
        let p = b.as_mut_ptr();
        h.buffers.push(b);
        p
    })
}

/// Read the scene file at `ptr` (`len` bytes of UTF-8). 1: it reads.
///
/// # Safety
/// `ptr` must come from `scene_alloc(len)` (or be valid for `len` bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scene_load(ptr: *const u8, len: usize) -> u32 {
    // SAFETY: the caller hands back a buffer `scene_alloc` made, `len` long.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let text = String::from_utf8_lossy(bytes).to_string();
    with(|h| {
        h.buffers.clear();
        h.live = None;
        h.inputs = None;
        match Scene::from_md("scene", &text) {
            Ok(s) => {
                h.scene = Some(s);
                h.text.clear();
                1
            }
            Err(e) => {
                h.scene = None;
                h.text = e;
                0
            }
        }
    })
}

/// Logical pixels per cell.
#[unsafe(no_mangle)]
pub extern "C" fn scene_cell() -> f32 {
    with(|h| h.scene.as_ref().map_or(4.0, |s| s.cell))
}

/// The frame rate it draws at (`Scene::fps`).
#[unsafe(no_mangle)]
pub extern "C" fn scene_fps() -> f32 {
    with(|h| h.scene.as_ref().map_or(30.0, Scene::fps))
}

/// 1: the whole window (behind the panes, or over them); 0: the footer row.
#[unsafe(no_mangle)]
pub extern "C" fn scene_window() -> u32 {
    with(|h| u32::from(h.scene.as_ref().is_none_or(|s| s.place != Place::Footer)))
}

/// 1: drawn over the glass panes (`place: front`); 0: behind them.
#[unsafe(no_mangle)]
pub extern "C" fn scene_front() -> u32 {
    with(|h| u32::from(h.scene.as_ref().is_some_and(|s| s.place == Place::Front)))
}

/// Put the palette in `scene_text`: `key\tvar` lines (the CSS custom
/// property each key's resolved colour is in). Its length.
#[unsafe(no_mangle)]
pub extern "C" fn scene_palette() -> usize {
    with(|h| {
        h.text = h.scene.as_ref().map(|s| s.palette.keys().map(|k| format!("{k}\t{}\n", scene::css_var(*k))).collect()).unwrap_or_default();
        h.text.len()
    })
}

/// The last text: why the scene did not read, or the palette.
#[unsafe(no_mangle)]
pub extern "C" fn scene_text() -> *const u8 {
    with(|h| h.text.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn scene_text_len() -> usize {
    with(|h| h.text.len())
}

/// The mood as the page says it: 0 idle, 1 working, 2 needs you.
pub fn mood(n: u32) -> Mood {
    match n {
        1 => Mood::Working,
        2 => Mood::NeedsYou,
        _ => Mood::Idle,
    }
}

/// The inputs at time `t` (the area and floor in cells; the pointer in
/// cells too, `pointer` 0 when it has not been seen). 1: the picture
/// changed, so draw it now. The first call starts the scene.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub extern "C" fn scene_set(
    mood_n: u32,
    cols: f32,
    rows: f32,
    floor: f32,
    still: u32,
    pointer: u32,
    x: f32,
    y: f32,
    pressed: u32,
    clicks: u32,
    inside: u32,
    t: f64,
) -> u32 {
    let inputs = Inputs {
        mood: mood(mood_n),
        cols,
        rows,
        floor,
        still: still != 0,
        pointer: (pointer != 0).then_some(Pointer { x, y, pressed: pressed != 0, clicks, inside: inside != 0 }),
    };
    with(|h| {
        let Some(s) = &h.scene else { return 0 };
        h.inputs = Some(inputs);
        let Some(live) = &h.live else {
            h.live = Some(scene::start(inputs, t));
            return 1;
        };
        let next = scene::step(live, inputs, t);
        let fps = s.fps();
        let changed = scene::frame(s, live, t, fps) != scene::frame(s, &next, t, fps);
        h.live = Some(next);
        u32::from(changed)
    })
}

/// The picture at time `t`: how many rectangles (`scene_draws`).
#[unsafe(no_mangle)]
pub extern "C" fn scene_frame(t: f64) -> usize {
    with(|h| {
        h.draws.clear();
        let (Some(s), Some(live)) = (&h.scene, &h.live) else {
            h.next = -1.0;
            return 0;
        };
        let f = scene::frame(s, live, t, s.fps());
        for d in &f.draws {
            h.draws.extend_from_slice(&[d.x, d.y, d.w, d.h, d.key as u32 as f32, d.alpha, d.glow]);
        }
        h.next = f.next.unwrap_or(-1.0);
        f.draws.len()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn scene_draws() -> *const f32 {
    with(|h| h.draws.as_ptr())
}

/// When the picture next changes; -1 when not until an input does.
#[unsafe(no_mangle)]
pub extern "C" fn scene_next() -> f64 {
    with(|h| h.next)
}

/// `SOURCE`, for a check that the module is current.
#[unsafe(no_mangle)]
pub extern "C" fn scene_source() -> *const u8 {
    SOURCE.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn scene_source_len() -> usize {
    SOURCE.len()
}

/// The FNV-1a hash of the scene code, as the build script and the
/// server's freshness test compute it: every `.rs` file of
/// `crates/core/src/scene/`, then this crate's `src/lib.rs`, by name.
pub fn source_hash(core_scene: &std::path::Path, this: &std::path::Path) -> String {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(core_scene)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect())
        .unwrap_or_default();
    files.sort();
    files.push(this.to_path_buf());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in files {
        for b in std::fs::read(&f).unwrap_or_default() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(text: &str) -> u32 {
        let p = scene_alloc(text.len());
        // SAFETY: `p` is `text.len()` bytes from `scene_alloc`.
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len());
            scene_load(p, text.len())
        }
    }

    #[test]
    fn the_module_draws_what_the_core_draws() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenes/cat.md")).unwrap();
        assert_eq!(load(&text), 1);
        let n = scene_palette();
        // SAFETY: `scene_text` points at `n` bytes.
        let palette = std::str::from_utf8(unsafe { std::slice::from_raw_parts(scene_text(), n) }).unwrap().to_string();
        assert!(palette.contains("f\tscene-f\n"), "{palette}");
        assert_eq!(scene_set(1, 200.0, 120.0, 112.0, 0, 0, 0.0, 0.0, 0, 0, 0, 1.0), 1);
        let n = scene_frame(1.5);
        let scene = Scene::from_md("scene", &text).unwrap();
        let inputs = Inputs { mood: Mood::Working, cols: 200.0, rows: 120.0, floor: 112.0, still: false, pointer: None };
        let core = scene::frame(&scene, &scene::start(inputs, 1.0), 1.5, scene.fps());
        assert_eq!(n, core.draws.len());
        // SAFETY: `scene_draws` points at `n * STRIDE` floats.
        let got = unsafe { std::slice::from_raw_parts(scene_draws(), n * STRIDE) };
        assert_eq!(got[0..4], [core.draws[0].x, core.draws[0].y, core.draws[0].w, core.draws[0].h]);
        assert_eq!(load("not a scene"), 0);
        assert!(scene_text_len() > 0);
    }
}
