//! The layout standard: the frame every screen of the app sits in.
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ ●●●                                      │
//! │ ╭──╮ ╭─────────────────────────────────╮ │
//! │ │⌂ │ │ ☰ Title                   Save  │ │  ← header row, inside the pane
//! │ │◆ │ │   ┌─────────────────────────┐   │ │
//! │ │◐ │ │   │ content box (scrolls)   │   │ │
//! │ │⚙ │ │   └─────────────────────────┘   │ │
//! │ ╰──╯ │ status                          │ │  ← footer row, inside the pane
//! │ pill ╰─────────────────────────────────╯ │
//! └──────────────────────────────────────────┘
//! ```
//!
//! The rules every shell follows when drawing it:
//!
//! 1. The navigation is one of two styles (`NavStyle`, from the theme's
//!    `nav_pinned` token). Pinned: a sidebar of icons and labels, always
//!    open, left of the content (a terminal keeps its menu in view).
//!    Float: the content takes centre stage, full width, and the
//!    navigation is one clear button in the header (web, VR). Pressed, it
//!    opens where the screen has room (`Placement`): as a side panel that
//!    moves the content over on a regular or wide screen, or as an overlay
//!    dropped over the content on a compact one.
//! 2. The content is one pane, inset from the window on every side and
//!    from the pill on the left. Its header and footer rows are optional
//!    and keep their height; the box between them scrolls and grows up to
//!    `content_max`.
//! 3. Containment: every part keeps its content inside its own box. Too
//!    tall scrolls inside that part; too wide wraps, and anything that still
//!    does not fit is cut off at the part's edge.
//!
//! 4. A view laid out in panes (`pages::fills`) fills the content box edge
//!    to edge instead of scrolling in it (`Frame::filling`); the header and
//!    footer stay where they are.
//!
//! This file only decides the numbers. Each shell draws them its own way,
//! so the GPUI window, a web page or a terminal all get the same frame.

use crate::screen::Screen;

/// Height of the header row (title and actions) at the top of the pane.
pub const HEADER_HEIGHT: f32 = 52.0;
/// Height of the footer row (status) at the bottom of the pane.
pub const FOOTER_HEIGHT: f32 = 28.0;
/// The pill's width folded (icons only) and open (icons and labels).
pub const PILL: f32 = 52.0;
pub const PILL_OPEN: f32 = 200.0;
/// Space between the window's edge and the parts, and between the parts.
pub const INSET: f32 = 12.0;
/// The top of the pill and the pane: below the window's traffic-light buttons.
pub const PILL_TOP: f32 = 40.0;
/// The widest a page in two columns grows (`Frame::widened`).
pub const WIDE_PAGE: f32 = 1280.0;

/// How the navigation sits. See rule 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavStyle {
    Pinned,
    #[default]
    Float,
}

impl NavStyle {
    /// From the theme's `nav_pinned` token: 1 pinned, 0 float.
    pub fn from_token(value: f32) -> NavStyle {
        if value >= 0.5 { NavStyle::Pinned } else { NavStyle::Float }
    }
}

/// Where the navigation is drawn right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Not drawn: only the header button shows it can open.
    Hidden,
    /// A sidebar left of the content pane; the pane starts after it.
    Sidebar,
    /// A list dropped over the content, under the header button. The
    /// content does not move.
    Overlay,
}

/// The frame's numbers for one screen size and one navigation state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub screen: Screen,
    pub nav: NavStyle,
    pub placement: Placement,
    /// How wide the navigation is drawn when shown.
    pub panel_width: f32,
    /// The content box never grows past this; `None` means full width.
    pub content_max: Option<f32>,
    /// Space between the content box's edge and what is inside it.
    pub gutter: f32,
    /// The view fills the box instead of scrolling in it (rule 4).
    pub fill: bool,
}

/// The frame for a window classed `screen`, with the navigation in style
/// `nav`, `open` (its header button pressed) or not.
pub fn frame(screen: Screen, open: bool, nav: NavStyle) -> Frame {
    let placement = match (nav, open, screen) {
        (NavStyle::Pinned, _, _) => Placement::Sidebar,
        (NavStyle::Float, false, _) => Placement::Hidden,
        (NavStyle::Float, true, Screen::Compact) => Placement::Overlay,
        (NavStyle::Float, true, _) => Placement::Sidebar,
    };
    let (content_max, gutter) = match screen {
        Screen::Compact => (None, 12.0),
        Screen::Regular => (Some(680.0), 24.0),
        Screen::Wide => (Some(880.0), 32.0),
    };
    Frame { screen, nav, placement, panel_width: PILL_OPEN, content_max, gutter, fill: false }
}

impl Frame {
    /// The same frame for a view that fills its box (rule 4): no cap, no gutter.
    pub fn filling(self) -> Frame {
        Frame { content_max: None, gutter: 0.0, fill: true, ..self }
    }

    /// The same frame for a page in two columns (`pages::Component::Columns`):
    /// on a wide screen its box may grow to `WIDE_PAGE`, so both columns
    /// have room; on smaller screens nothing changes (it is one column).
    pub fn widened(self) -> Frame {
        match self.screen {
            Screen::Wide => Frame { content_max: Some(WIDE_PAGE), ..self },
            _ => self,
        }
    }

    /// Is the navigation drawn?
    pub fn panel_open(&self) -> bool {
        self.placement != Placement::Hidden
    }

    /// Does the header carry the navigation button? Only when it floats.
    pub fn header_button(&self) -> bool {
        self.nav == NavStyle::Float
    }

    /// Where the content pane starts: past the sidebar when there is one,
    /// at the inset otherwise (an overlay does not move it).
    pub fn content_left(&self) -> f32 {
        match self.placement {
            Placement::Sidebar => INSET + PILL_OPEN + INSET,
            Placement::Hidden | Placement::Overlay => INSET,
        }
    }

    /// Room left for the content box's insides in a window `viewport` wide:
    /// the window minus the sidebar's lane (if any) and the insets, capped
    /// at the box's max, minus the gutter on both sides. Inner elements
    /// size themselves from this.
    pub fn content_width(&self, viewport: f32) -> f32 {
        let pane = (viewport - self.content_left() - INSET).max(0.0);
        let boxed = match self.content_max {
            Some(max) => pane.min(max),
            None => pane,
        };
        (boxed - 2.0 * self.gutter).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_button_opens_a_panel_where_there_is_room_and_an_overlay_where_not() {
        let closed = frame(Screen::Regular, false, NavStyle::Float);
        assert_eq!(closed.placement, Placement::Hidden);
        assert!(closed.header_button());
        // Full width: 720 - 12 - 12 = 696, capped at 680, minus 2 × 24.
        assert_eq!(closed.content_width(720.0), 680.0 - 48.0);
        let panel = frame(Screen::Regular, true, NavStyle::Float);
        assert_eq!(panel.placement, Placement::Sidebar);
        assert_eq!(panel.content_left(), INSET + PILL_OPEN + INSET);
        let overlay = frame(Screen::Compact, true, NavStyle::Float);
        assert_eq!(overlay.placement, Placement::Overlay);
        assert_eq!(overlay.content_width(375.0), frame(Screen::Compact, false, NavStyle::Float).content_width(375.0));
    }

    #[test]
    fn a_pinned_sidebar_is_always_open_and_the_content_starts_after_it() {
        let pinned = frame(Screen::Regular, false, NavStyle::Pinned);
        assert!(pinned.panel_open());
        assert!(!pinned.header_button());
        assert_eq!(pinned.content_left(), INSET + PILL_OPEN + INSET);
        // 720 - 224 - 12 = 484, minus 2 × 24.
        assert_eq!(pinned.content_width(720.0), 484.0 - 48.0);
        assert_eq!(NavStyle::from_token(1.0), NavStyle::Pinned);
        assert_eq!(NavStyle::from_token(0.0), NavStyle::Float);
    }

    #[test]
    fn a_view_in_panes_fills_the_box_but_keeps_the_navigation() {
        let fill = frame(Screen::Regular, false, NavStyle::Pinned).filling();
        assert_eq!((fill.content_max, fill.gutter, fill.fill), (None, 0.0, true));
        assert!(!frame(Screen::Regular, false, NavStyle::Pinned).fill, "a plain page scrolls");
        assert!(fill.panel_open(), "the navigation stays");
        assert_eq!(fill.content_width(720.0), 720.0 - (INSET + PILL_OPEN + INSET) - INSET);
    }

    #[test]
    fn a_page_in_two_columns_grows_only_on_a_wide_screen() {
        assert_eq!(frame(Screen::Wide, false, NavStyle::Float).widened().content_width(1600.0), WIDE_PAGE - 64.0);
        assert_eq!(frame(Screen::Wide, false, NavStyle::Float).widened().content_width(1000.0), 1000.0 - 24.0 - 64.0);
        assert_eq!(frame(Screen::Regular, false, NavStyle::Float).widened(), frame(Screen::Regular, false, NavStyle::Float));
    }

    #[test]
    fn a_big_window_caps_the_content_box() {
        assert_eq!(frame(Screen::Wide, false, NavStyle::Float).content_width(1600.0), 880.0 - 64.0);
        assert_eq!(frame(Screen::Compact, false, NavStyle::Float).content_width(375.0), 375.0 - 24.0 - 24.0);
    }
}
