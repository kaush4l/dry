//! Bundles the app's files into the wasm: every route, component, theme
//! and scene, the default `responses.md`, `models.json` and
//! `sessions.json`, as `(path, text)` pairs (`OUT_DIR/bundle.rs`, read
//! by `src/bundle.rs`). The browser has no repo to read them from.

use std::path::{Path, PathBuf};

const FOLDERS: [&str; 4] = ["routes", "components", "themes", "scenes"];
const FILES: [&str; 3] = ["responses.md", "models.json", "sessions.json"];

fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(rel)) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.ends_with(".tmp") {
            continue;
        }
        let path = format!("{rel}/{name}");
        if e.path().is_dir() {
            walk(root, &path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let root: PathBuf = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..").canonicalize().unwrap();
    let mut files = Vec::new();
    for f in FOLDERS {
        println!("cargo:rerun-if-changed={}", root.join(f).display());
        walk(&root, f, &mut files);
    }
    for f in FILES {
        println!("cargo:rerun-if-changed={}", root.join(f).display());
        if root.join(f).is_file() {
            files.push(f.to_string());
        }
    }
    let mut code = String::from("/// The app's files: (path, text).\npub const FILES: &[(&str, &str)] = &[\n");
    for f in &files {
        code.push_str(&format!("    ({f:?}, include_str!({:?})),\n", root.join(f).display().to_string()));
    }
    code.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("bundle.rs");
    std::fs::write(out, code).unwrap();
}
