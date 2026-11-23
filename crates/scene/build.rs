//! `DRY_SCENE_SOURCE`: the hash of the scene code this module is built
//! from (`dry_scene::source_hash`, the same sum written out here), so a
//! stale `crates/server/assets/scene.wasm` is caught by the server's test.

use std::path::{Path, PathBuf};

fn main() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core = here.join("../core/src/scene");
    let this = here.join("src/lib.rs");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&core)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect())
        .unwrap_or_default();
    files.sort();
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    println!("cargo:rerun-if-changed={}", core.display());
    println!("cargo:rerun-if-changed={}", this.display());
    files.push(this);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in files {
        for b in std::fs::read(&f).unwrap_or_default() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=DRY_SCENE_SOURCE={h:016x}");
}
