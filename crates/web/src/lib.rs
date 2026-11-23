//! dry in the browser. The app compiled to WebAssembly and run in the
//! page: the core's state and loop, the shared renderer (`dry_html`), the
//! state kept in IndexedDB, and each step sent to the model with `fetch`.
//! See docs/browser.md.
//!
//! Pure parts (tested natively too): `bundle` (the app's files, built
//! in), `settings` (this browser's model server), `demo` (the seeded
//! state). The browser parts are only built for wasm32: `app` (state,
//! turns, events, drawing), `idb` (the stores), `net` (fetch), `host`
//! (what the browser refuses), `vm` (the container route: Alpine in v86,
//! docs/container.md), `env` (that VM as the environment the file and
//! terminal tools run on).

pub mod bundle;
pub mod demo;
pub mod settings;

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
pub mod caps;
#[cfg(target_arch = "wasm32")]
pub mod companion;
#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(target_arch = "wasm32")]
pub mod idb;
#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
pub mod scene;
#[cfg(target_arch = "wasm32")]
mod env;
#[cfg(target_arch = "wasm32")]
mod vm;
