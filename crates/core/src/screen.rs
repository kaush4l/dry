//! Screen classes: the window's width sorted into a few named sizes, so the
//! layout code asks "how much room is there?" instead of comparing raw
//! pixels everywhere. Pure, no GPUI, so a terminal shell can reuse it later
//! by turning its column count into the same classes.

/// How much horizontal room the window has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Phone-sized: each item stacks its name above its buttons.
    Compact,
    /// A normal window: one item per line.
    Regular,
    /// A big window: items flow into two columns.
    Wide,
}

/// Where one class ends and the next begins, in logical pixels.
pub const COMPACT_BELOW: f32 = 420.0;
pub const WIDE_FROM: f32 = 800.0;

impl Screen {
    pub fn from_width(width: f32) -> Screen {
        if width < COMPACT_BELOW {
            Screen::Compact
        } else if width < WIDE_FROM {
            Screen::Regular
        } else {
            Screen::Wide
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_width_falls_in_one_class_and_the_edges_belong_to_the_bigger_class() {
        assert_eq!(Screen::from_width(375.0), Screen::Compact);
        assert_eq!(Screen::from_width(COMPACT_BELOW), Screen::Regular);
        assert_eq!(Screen::from_width(600.0), Screen::Regular);
        assert_eq!(Screen::from_width(WIDE_FROM), Screen::Wide);
    }
}
