//! The terminal: a phosphor screen at rest. A block cursor wanders the
//! glass, types a short line of glowing traces (word-shaped bars, not
//! letters), pauses blinking, and jumps somewhere else. Old lines fade like
//! phosphor. A faint scanline sweeps slowly down the whole window.
//!
//! No state: everything is a function of `t`. Line `n` starts at
//! `n × PERIOD`; where it sits and how its words run come from `noise(n, …)`,
//! so the lines on screen at `t` are found directly, with nothing kept.

use super::{Draw, Frame, Inputs, frac, noise};

/// Keys its code draws with: traces, the cursor, the scanline, amber
/// prompt traces.
pub const KEYS: &[char] = &['t', 'c', 'l', 'a'];

/// One character: 2 cells wide, 3 high, 3 cells apart; lines 6 apart.
const CHAR_W: f32 = 2.0;
const CHAR_H: f32 = 3.0;
const PITCH: f32 = 3.0;
const LINE: f32 = 6.0;
/// A new line every `PERIOD` seconds, typed at `RATE` characters a second.
const PERIOD: f64 = 4.2;
const RATE: f64 = 16.0;
/// Seconds an old line takes to fade out.
const FADE: f64 = 10.0;
/// Seconds for the scanline to sweep the window once.
const SWEEP: f64 = 9.0;
/// The time a still screen (motion 0) is frozen at: a line half typed.
const STILL_AT: f64 = 5.0;

/// Line `n`: where it starts (cells), its words (lengths in characters,
/// each followed by one space), and whether it is an amber prompt line.
fn line(n: i64, i: &Inputs) -> (f32, f32, Vec<u32>, bool) {
    let n = n as u32;
    let total = 10 + (noise(n, 1) * 22.0) as u32;
    let mut words = Vec::new();
    let mut used = 0;
    let mut k = 2;
    while used < total {
        let len = (2 + (noise(n, k) * 6.0) as u32).min(total - used);
        words.push(len);
        used += len + 1;
        k += 1;
    }
    let width = used as f32 * PITCH;
    let lines = ((i.rows - 8.0) / LINE).floor().max(1.0);
    let y = 4.0 + (noise(n, 50) * lines).floor() * LINE;
    let room = (i.cols - 8.0 - width).max(1.0);
    let x = 4.0 + (noise(n, 51) * room / PITCH).floor() * PITCH;
    (x, y, words, noise(n, 52) < 0.2)
}

pub fn frame(i: &Inputs, t: f64, level: u8) -> Frame {
    let t = if i.still { STILL_AT } else { t };
    let mut draws = Vec::new();
    let glow = [0.0, 0.0, 0.8, 1.6][level as usize];

    // The lines on screen: the one being typed and those still fading.
    let now = (t / PERIOD).floor() as i64;
    let oldest = ((t - FADE - PERIOD) / PERIOD).floor() as i64;
    let mut cursor = (0.0, 0.0);
    for n in oldest.max(0)..=now {
        let (x, y, words, amber) = line(n, i);
        let age = t - n as f64 * PERIOD;
        if age < 0.0 {
            continue;
        }
        let total: u32 = words.iter().map(|w| w + 1).sum();
        let typed = ((age * RATE).floor() as u32).min(total);
        let typing_done = total as f64 / RATE;
        let alpha = 0.85 * (-(age - typing_done).max(0.0) / 3.5).exp() as f32;
        if alpha < 0.02 && n != now {
            continue;
        }
        let key = if amber { 'a' } else { 't' };
        let mut at = 0;
        for len in &words {
            let shown = typed.saturating_sub(at).min(*len);
            if shown > 0 {
                let w = shown as f32 * PITCH - (PITCH - CHAR_W);
                draws.push(Draw::cell(x + at as f32 * PITCH, y, w, CHAR_H, key, alpha).glowing(glow));
            }
            at += len + 1;
        }
        if n == now {
            cursor = (x + typed as f32 * PITCH, y);
            // Solid while typing; then it blinks, half a second on, half off.
            let blinking = typed >= total;
            let on = !blinking || frac((age - typing_done) * 1.0) < 0.5;
            if on {
                draws.push(Draw::cell(cursor.0, y, CHAR_W, CHAR_H, 'c', 1.0).glowing(glow * 1.5));
                if level >= 3 {
                    // Phosphor bloom around the hot cursor.
                    draws.push(Draw::cell(cursor.0 - 1.0, y - 1.0, CHAR_W + 2.0, CHAR_H + 2.0, 'c', 0.12).glowing(3.0));
                }
            }
        }
    }

    // The scanline, with a soft tail above it at the higher tiers.
    let scan = (frac(t / SWEEP) * (i.rows as f64 + 8.0) - 4.0) as f32;
    let tail: &[f32] = match level {
        1 => &[0.10],
        2 => &[0.10, 0.055, 0.025],
        _ => &[0.10, 0.07, 0.045, 0.028, 0.016, 0.008],
    };
    for (k, a) in tail.iter().enumerate() {
        draws.push(Draw::cell(0.0, scan - k as f32, i.cols, 1.0, 'l', *a));
    }

    Frame { draws, anchors: vec![cursor, (0.0, scan)], next: if i.still { None } else { Some(t) } }
}

#[cfg(test)]
mod tests {
    use super::super::tests::inputs;
    use super::super::{Mood, frame as draw, start};
    use super::*;

    #[test]
    fn the_cursor_types_a_line_then_jumps_and_old_lines_fade() {
        let s = super::super::tests::shipped("terminal");
        let live = start(inputs(Mood::Idle), 0.0);
        let at = |t: f64| draw(&s, &live, t, 24.0);
        let (a, b) = (at(0.1).anchors[0], at(0.6).anchors[0]);
        assert!(b.0 > a.0 && a.1 == b.1, "it types along its line: {a:?} → {b:?}");
        assert_ne!(at(4.3).anchors[0].1, at(4.1).anchors[0].1, "a new line somewhere else");
        let traces = |t| at(t).draws.iter().filter(|d| d.key == 't' || d.key == 'a').count();
        assert!(traces(9.0) > traces(1.0), "old lines stay a while");
        let faded = at(20.0).draws.iter().filter(|d| d.key == 't' || d.key == 'a').map(|d| d.alpha).fold(1.0, f32::min);
        assert!(faded < 0.5, "and fade");
    }

    #[test]
    fn lines_stay_on_the_screen() {
        let i = inputs(Mood::Idle);
        for n in 0..200 {
            let (x, y, words, _) = line(n, &i);
            let w: u32 = words.iter().map(|w| w + 1).sum();
            assert!(x >= 4.0 && x + w as f32 * PITCH <= i.cols - 3.0, "line {n} at {x}");
            assert!(y >= 4.0 && y + CHAR_H <= i.rows - 4.0, "line {n} at {y}");
        }
    }
}
