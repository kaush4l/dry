//! The backdrop: soft light behind the glass, moving in layers.
//!
//! Liquid glass reads as glass only when something moves behind it. The
//! backdrop is a few large, blurred blobs of colour between the window's
//! tint and the panes. Each sits at its own depth: far layers are bigger,
//! softer and slower; near ones smaller and quicker. The panes slide over
//! them like glass over a slow light show, which is the motion feel.
//!
//! Every layer moves on a closed loop (a Lissajous curve with whole-number
//! frequencies), so at phase 1 it is back where it began and the motion
//! repeats with no jump. The shell turns phase into time: one loop of the
//! slowest layer lasts `motion` seconds (a theme token); 0 keeps it still.
//!
//! Pure: positions are fractions of the window (0..1), sizes fractions of
//! its shorter side. The shell turns them into pixels and paints.

use std::f32::consts::TAU;

/// One blob of light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    /// Which backdrop colour token (`backdrop_1` ... `backdrop_3`), 0-based.
    pub color: usize,
    /// How far back it is: 1 the farthest, 0 right behind the glass.
    pub depth: f32,
    /// Its middle at rest, as a fraction of the window (x, y).
    pub center: (f32, f32),
    /// How far it wanders from there (x, y), as a fraction of the window.
    pub reach: (f32, f32),
    /// Whole loops per cycle on each axis (x, y). Different on the two
    /// axes gives a figure-eight-like path instead of an ellipse.
    pub loops: (u32, u32),
    /// Where on its loop it starts, 0..1, so layers do not move in step.
    pub offset: f32,
}

impl Layer {
    /// Its diameter, as a fraction of the window's shorter side: far
    /// layers are big.
    pub fn size(&self) -> f32 {
        0.45 + 0.55 * self.depth
    }

    /// How soft its edge is, as a fraction of its diameter: far layers
    /// are blurrier.
    pub fn softness(&self) -> f32 {
        0.5 + 0.5 * self.depth
    }

    /// How much quicker than the slowest layer it loops: far layers 1×,
    /// near ones up to 2.5×.
    pub fn speed(&self) -> f32 {
        1.0 + (1.0 - self.depth) * 1.5
    }

    /// Its centre at `phase` (0..1 of its own loop), as window fractions.
    pub fn at(&self, phase: f32) -> (f32, f32) {
        let a = TAU * (phase + self.offset);
        let (fx, fy) = (self.loops.0 as f32, self.loops.1 as f32);
        (self.center.0 + self.reach.0 * (a * fx).sin(), self.center.1 + self.reach.1 * (a * fy).cos())
    }

    /// How long one loop lasts, in seconds, when the slowest takes
    /// `motion`. None: still.
    pub fn period(&self, motion: f32) -> Option<f32> {
        (motion > 0.0).then(|| motion / self.speed())
    }
}

/// The layers, back to front.
pub const LAYERS: [Layer; 4] = [
    Layer { color: 0, depth: 1.0, center: (0.25, 0.30), reach: (0.18, 0.14), loops: (1, 1), offset: 0.0 },
    Layer { color: 1, depth: 0.8, center: (0.78, 0.70), reach: (0.16, 0.18), loops: (1, 2), offset: 0.35 },
    Layer { color: 2, depth: 0.5, center: (0.60, 0.20), reach: (0.22, 0.10), loops: (2, 1), offset: 0.6 },
    Layer { color: 0, depth: 0.2, center: (0.30, 0.85), reach: (0.20, 0.08), loops: (1, 1), offset: 0.8 },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loop_ends_where_it_starts() {
        for l in LAYERS {
            let (a, b) = (l.at(0.0), l.at(1.0));
            assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4, "{l:?} jumps");
        }
    }

    #[test]
    fn far_layers_are_bigger_softer_and_slower() {
        let (far, near) = (LAYERS[0], LAYERS[3]);
        assert!(far.size() > near.size());
        assert!(far.softness() > near.softness());
        assert!(far.period(30.0).unwrap() > near.period(30.0).unwrap());
        assert_eq!(far.period(0.0), None, "motion 0 keeps it still");
    }

    #[test]
    fn layers_stay_mostly_on_the_window() {
        for l in LAYERS {
            for i in 0..=20 {
                let (x, y) = l.at(i as f32 / 20.0);
                assert!((-0.1..=1.1).contains(&x) && (-0.1..=1.1).contains(&y), "{l:?} leaves at {x},{y}");
            }
        }
    }
}
