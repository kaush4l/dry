//! dry-html: the app as HTML, from the same page files and core view
//! models the GPUI window draws from. Pure, so every web shell draws the
//! same pages: the server (`crates/server`, from disk, read-only) and the
//! browser (`crates/web`, from its bundle and its store, live).
//!
//! - `world`: what a page is drawn from (`World`), and its seams: the
//!   host for snapshots, the repo for the Code page, the access.
//! - `page`: one page as a document, or as its parts for a shell that
//!   swaps regions (`page::parts`).
//! - `render`: `Component` → HTML.
//! - `css`: the tokens, the frame and the look as CSS.
//! - `ansi`: terminal output (ANSI colour codes) as HTML.

pub mod ansi;
pub mod css;
pub mod page;
pub mod render;
pub mod world;

pub use render::{Ctx, Query};
pub use world::{Access, World};
