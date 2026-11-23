//! The abyss: the deep sea, lit from inside. Jellyfish rise through the
//! dark: each bell contracts and relaxes, and every contraction is a surge
//! upward, so they climb in pulses, trailing swaying tentacles. Plankton
//! motes drift up and twinkle; faint shafts of light lean in from above.
//!
//! No state: every jellyfish, mote and shaft is a function of `t` and its
//! own `noise(i, …)` numbers. The count of jellyfish is fixed (they are the
//! anchors); the detail tier adds bell frames, tentacle segments, motes and
//! finer steps in the shafts.

use std::f64::consts::TAU;

use super::{Draw, Frame, Inputs, Scene, frac, noise};

/// The bell: relaxed, contracting, contracted, relaxing. Below 60 fps,
/// only relaxed and contracted.
pub const SPRITES: &[(&str, usize)] = &[("bell", 4)];
/// Keys its code draws with: cyan and magenta glow, plankton, light.
pub const KEYS: &[char] = &['j', 'm', 'p', 's'];

const JELLIES: u32 = 6;
const STRANDS: [f32; 4] = [-3.0, -1.0, 1.0, 3.0];
/// The time a still sea (motion 0) is frozen at.
const STILL_AT: f64 = 7.0;

/// Jellyfish `n` at `t`: its bell's centre-bottom (cells), and how far
/// through its pulse it is (0 the start of a contraction).
fn jelly(n: u32, i: &Inputs, t: f64) -> (f32, f32, f64, f64) {
    let r = |k| noise(n + 1, k) as f64;
    let period = 2.8 + 1.0 * r(1);
    let speed = 1.4 + 1.2 * r(2);
    let beats = t / period + r(6);
    // Distance risen: steady, with a surge at each contraction (its rate
    // is speed × (1 + 0.45 cos), never below half the speed).
    let risen = speed * period * (beats + 0.45 * (TAU * beats).sin() / TAU);
    let span = i.rows as f64 + 34.0;
    let y = i.rows as f64 + 12.0 - frac((risen + r(7) * span) / span) * span;
    let sway = (3.0 + 3.0 * r(4)) * (TAU * t / (17.0 + 9.0 * r(5)) + r(8) * TAU).sin();
    let x = i.cols as f64 * (0.06 + 0.88 * r(3)) + sway;
    (x as f32, y as f32, frac(beats), period)
}

pub fn frame(scene: &Scene, i: &Inputs, t: f64, level: u8) -> Frame {
    let t = if i.still { STILL_AT } else { t };
    let mut draws = Vec::new();

    // Light shafts from above, leaning, breathing. Coarser steps at the
    // lower tiers.
    let step = [0.0, 10.0, 5.0, 2.0][level as usize];
    for k in 0..4u32 {
        let r = |j| noise(k + 100, j);
        let a = 0.05 + 0.03 * (TAU * t / 13.0 + k as f64 * 1.7).sin() as f32;
        let drift = 2.0 * (TAU * t / 29.0 + k as f64).sin() as f32;
        let top = i.cols * (0.1 + 0.24 * k as f32 + 0.06 * r(1)) + drift;
        let (w, lean, len) = (7.0 + 6.0 * r(2), 0.25 + 0.12 * r(3), i.rows * 0.85);
        let mut y = 0.0;
        while y < len {
            draws.push(Draw::cell(top + lean * y, y, w, step, 's', a * (1.0 - y / len)));
            y += step;
        }
    }

    // Plankton: more motes at higher tiers; mote k is the same mote at
    // every tier, so the shared ones are where they were.
    let motes = [0, 36, 70, 120][level as usize];
    let span = i.rows as f64 + 4.0;
    for k in 0..motes {
        let r = |j| noise(k + 1000, j) as f64;
        let rise = 0.5 + 1.0 * r(2);
        let y = i.rows as f64 + 2.0 - frac((t * rise + r(3) * span) / span) * span;
        let x = i.cols as f64 * r(1) + 1.5 * (t * (0.3 + 0.4 * r(4)) + k as f64).sin();
        let twinkle = 0.25 + 0.35 * (0.5 + 0.5 * (t * (1.0 + 2.0 * r(5)) + r(6) * TAU).sin());
        let size = if r(7) < 0.3 { 0.6 } else { 1.0 };
        let key = if k % 7 == 3 { 'm' } else { 'p' };
        draws.push(Draw::cell(x as f32, y as f32, size, size, key, twinkle as f32).glowing(if level >= 3 { 0.8 } else { 0.0 }));
    }

    // The jellyfish, cyan and magenta in turn.
    let mut anchors = Vec::new();
    for n in 0..JELLIES {
        let (x, y, beat, period) = jelly(n, i, t);
        anchors.push((x, y));
        let swap = |k: char| match (n % 2, k) {
            (1, 'j') => 'm',
            (1, 'm') => 'j',
            _ => k,
        };
        let rim = swap('j');
        // Squeezed, then slowly let go: the contraction is quick.
        let frame = if level >= 3 {
            match beat {
                b if b < 0.12 => 1,
                b if b < 0.30 => 2,
                b if b < 0.55 => 3,
                _ => 0,
            }
        } else if beat < 0.30 {
            2
        } else {
            0
        };
        let bell = scene.sprite("bell", frame);
        let (w, h) = (bell.w as f32, bell.h as f32);
        if level >= 2 {
            let glow = if level >= 3 { 7.0 } else { 5.0 };
            draws.push(Draw::cell(x - w * 0.3, y - h * 0.75, w * 0.6, h * 0.5, rim, 0.28).glowing(glow));
        }
        // Tentacles hang from the rim and sway; a surge stretches them.
        let segments = [0, 3, 5, 8][level as usize];
        let stretch = 1.0 + 0.3 * (TAU * beat).cos().max(0.0) as f32;
        let length = 10.0 * stretch;
        for (s, off) in STRANDS.iter().enumerate() {
            for j in 1..=segments {
                let f = j as f32 / segments as f32;
                let wave = (TAU * (t / period * 0.5) - f as f64 * 3.0 + s as f64 * 0.9).sin() as f32;
                let sx = x + off * (1.0 - 0.25 * f) + 1.6 * f * wave;
                let sy = y - 2.0 + f * length;
                draws.push(Draw::cell(sx - 0.5, sy, 1.0, (length / segments as f32 * 0.8).max(0.8), rim, 0.65 * (1.0 - f) + 0.1));
            }
        }
        bell.draw(x - w / 2.0, y - h, false, 0.92, swap, &mut draws);
    }

    Frame { draws, anchors, next: if i.still { None } else { Some(t) } }
}

#[cfg(test)]
mod tests {
    use super::super::Mood;
    use super::super::tests::inputs;
    use super::*;

    #[test]
    fn jellyfish_rise_in_pulses_and_never_sink() {
        let i = inputs(Mood::Idle);
        let (_, _, _, period) = jelly(0, &i, 0.0);
        let mut last = jelly(0, &i, 0.0).1;
        let mut steps = Vec::new();
        for n in 1..400 {
            let y = jelly(0, &i, n as f64 * 0.02).1;
            let dy = last - y;
            if dy > -10.0 {
                // (a big negative jump is the wrap back to the bottom)
                assert!(dy > 0.0, "it rises at every moment");
                steps.push(dy);
            }
            last = y;
        }
        let (lo, hi) = steps.iter().fold((f32::MAX, 0.0f32), |(a, b), d| (a.min(*d), b.max(*d)));
        assert!(hi > 2.0 * lo, "surges and glides ({lo}..{hi}) over a {period:.1}s pulse");
    }
}
