//! Bundles the shipped default (`default/` at the repo root,
//! docs/default.md) into the core as `(path, text)` pairs
//! (`OUT_DIR/shipped.rs`, read by `src/shipped.rs`), so every shell, the
//! browser's too, starts a fresh state from the same files.

use std::path::{Path, PathBuf};

fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(rel)) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.ends_with(".tmp") {
            continue;
        }
        let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        if e.path().is_dir() {
            walk(root, &path, out);
        } else if path.ends_with(".md") || path.ends_with(".json") {
            out.push(path);
        }
    }
}

fn main() {
    let root: PathBuf = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../default");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, "", &mut files);
    let mut code = String::from("/// The shipped default's files: (path under default/, text).\npub const FILES: &[(&str, &str)] = &[\n");
    for f in &files {
        code.push_str(&format!("    ({f:?}, include_str!({:?})),\n", root.join(f).canonicalize().unwrap().display().to_string()));
    }
    code.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("shipped.rs"), code).unwrap();
}
