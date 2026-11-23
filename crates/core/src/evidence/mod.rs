//! Evidence: what a project shows when it runs, captured the same way
//! every build and compared with the last (docs/evidence.md).
//!
//! ```text
//! evidence.md ──parse──▶ Spec (scenarios × viewports, build, run, masks, threshold)
//!                          │  the runner (dry evidence) builds, starts the app,
//!                          ▼  drives headless Chrome, captures each shot
//! capture.png + .txt ──compare──▶ Shot (same, changed, new) + diff.png
//!                          ▼
//!             evidence/manifest.json, summary.md
//! ```
//!
//! - `spec`: `evidence.md`, a template-style table and sections.
//! - `image`: PNG decode and encode, masks, the pixel diff and its boxes.
//! - `text`: the page-as-text line diff.
//! - `report`: the manifest, the summary, one shot compared.
//! - `artifact`: `kind: evidence`, the engine's tool and snapshot.
//! - `sha`: SHA-256 for the manifest.
//!
//! All pure, so every target (the browser's wasm too) can read and
//! compare evidence; only the runner touches processes and Chrome.

pub mod artifact;
pub mod image;
pub mod report;
pub mod sha;
pub mod spec;
pub mod text;
