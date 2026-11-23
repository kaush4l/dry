//! The app's files, bundled at build time (`build.rs`): routes,
//! components, themes, scenes, `responses.md`, `models.json`,
//! `sessions.json`. The browser reads them from here, never from a disk.

use std::collections::BTreeMap;

include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

/// Every bundled file by path.
pub fn files() -> BTreeMap<String, String> {
    FILES.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

/// The contract new engines start with, and its format.
pub fn responses() -> Result<(dry_core::response::Responses, dry_core::response::Format), String> {
    let text = FILES.iter().find(|(p, _)| *p == "responses.md").map(|(_, t)| *t).ok_or("responses.md is not bundled")?;
    let (r, f) = dry_core::response::Responses::from_md(text).map_err(|e| format!("responses.md: {e}"))?;
    Ok((r, f.unwrap_or_default()))
}
