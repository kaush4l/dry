//! The workspace folder's primitives (docs/companion-protocol.md §6–§7):
//! list, read, write, stat inside the one folder, every path confined
//! (`..`, absolute paths and links that lead out refused, secrets never
//! touched, `.dry/` only through the store and the snapshot), and the
//! page's own state in `.dry/`: the store and `snapshot.json`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::rules::{self, DRY_DIR, Fail, HIDDEN, MAX_READ, fail};
use super::state::{Companion, PROTOCOL, write_atomic};

/// A path of the folder, resolved.
pub struct WsPath {
    pub real: PathBuf,
    /// Its real place relative to the folder, `/`-separated.
    pub rel: String,
    pub exists: bool,
}

/// `real` is `base` or inside it.
pub fn within(real: &Path, base: &Path) -> bool {
    real.starts_with(base)
}

/// A path the page gives, as its real place in the folder; or why not
/// (the codes of §6). `for_write`: it may not exist yet; `dir`: it must
/// be a folder when it exists.
pub fn ws_path(c: &Companion, p: &str, for_write: bool, dir: bool) -> Result<WsPath, Fail> {
    let parts = rules::folder_parts(p)?;
    let abs = parts.iter().fold(c.folder.clone(), |a, s| a.join(s));
    let (real, exists) = match abs.canonicalize() {
        Ok(r) => (r, true),
        Err(_) => {
            if abs.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(fail(403, "path_outside", format!("{p} is a link that leads nowhere")));
            }
            // The nearest folder that is there, resolved, and the rest as given.
            let mut up = abs.clone();
            let mut rest = Vec::new();
            while !up.exists() {
                match (up.file_name(), up.parent()) {
                    (Some(n), Some(parent)) => {
                        rest.push(n.to_os_string());
                        up = parent.to_path_buf();
                    }
                    _ => break,
                }
            }
            let base = up.canonicalize().unwrap_or(up);
            (rest.iter().rev().fold(base, |a, s| a.join(s)), false)
        }
    };
    if !within(&real, &c.folder) {
        return Err(fail(403, "path_outside", format!("{p} leads outside the folder (a link?)")));
    }
    let rel: Vec<String> = real.strip_prefix(&c.folder).unwrap_or(&real).iter().map(|s| s.to_string_lossy().to_string()).collect();
    rules::check_parts(&rel)?;
    if for_write && rel.iter().any(|s| s == ".git") {
        return Err(fail(403, "dry_state", "files inside .git are written only by git"));
    }
    if !exists && !for_write {
        return Err(fail(404, "not_found", format!("no such file: {p}")));
    }
    if dir && exists && !real.is_dir() {
        return Err(fail(400, "not_a_folder", format!("{p} is not a folder")));
    }
    Ok(WsPath { real, rel: rel.join("/"), exists })
}

/// Every file under a folder of the workspace, relative to it: a folder's
/// own files first, by name, then its folders' (breadth first); at most
/// `max`. Never `.git`, `.dry`, build folders, secrets, links that lead out.
pub fn list(c: &Companion, dir: &str, max: usize) -> Result<Value, Fail> {
    let base = ws_path(c, dir, false, true)?;
    let mut out: Vec<String> = Vec::new();
    let mut queue = VecDeque::from([String::new()]);
    let mut truncated = false;
    'walk: while let Some(rel) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(base.real.join(&rel)) else { continue };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if HIDDEN.contains(&name.as_str()) || name == DRY_DIR || rules::secret_name(&name) {
                continue;
            }
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let Ok(ft) = e.file_type() else { continue };
            let mut is_dir = ft.is_dir();
            if ft.is_symlink() {
                match e.path().canonicalize() {
                    Ok(real) if within(&real, &c.folder) => is_dir = real.is_dir(),
                    _ => continue,
                }
            }
            if is_dir {
                queue.push_back(r);
            } else if out.len() >= max {
                truncated = true;
                break 'walk;
            } else {
                out.push(r);
            }
        }
    }
    Ok(json!({ "path": base.rel, "files": out, "truncated": truncated }))
}

const IMAGES: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "avif"];

/// One file: its text, or that it is binary or too large.
pub fn read(c: &Companion, p: &str) -> Result<Value, Fail> {
    let w = ws_path(c, p, false, false)?;
    let meta = std::fs::metadata(&w.real).map_err(|e| fail(500, "internal", e.to_string()))?;
    if meta.is_dir() {
        return Err(fail(400, "not_a_file", format!("{p} is a folder")));
    }
    if meta.len() > MAX_READ {
        return Ok(json!({ "path": w.rel, "kind": "too_large", "bytes": meta.len() }));
    }
    let bytes = std::fs::read(&w.real).map_err(|e| fail(500, "internal", e.to_string()))?;
    let n = bytes.len();
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Ok(json!({ "path": w.rel, "kind": "text", "text": text, "bytes": n })),
        _ => {
            let ext = w.real.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            Ok(json!({ "path": w.rel, "kind": "binary", "bytes": n, "image": IMAGES.contains(&ext.as_str()) }))
        }
    }
}

/// A whole file written (its folders made; atomic).
pub fn write(c: &Companion, p: &Value, text: &Value) -> Result<Value, Fail> {
    let text = text.as_str().ok_or_else(|| fail(400, "bad_request", "write takes {path, text}: the text is missing"))?;
    let p = p.as_str().ok_or_else(|| fail(400, "bad_request", "write takes {path, text}: the path is missing"))?;
    let w = ws_path(c, p, true, false)?;
    if w.real.is_dir() {
        return Err(fail(400, "not_a_file", format!("{p} is a folder")));
    }
    write_atomic(&w.real, text).map_err(|e| fail(500, "internal", format!("{p}: {e}")))?;
    Ok(json!({ "ok": true, "path": w.rel, "bytes": text.len() }))
}

/// Whether a path is a file, a folder or missing.
pub fn stat(c: &Companion, p: &str) -> Result<Value, Fail> {
    let w = ws_path(c, p, true, false)?;
    if !w.exists {
        return Ok(json!({ "path": w.rel, "kind": "missing" }));
    }
    let meta = std::fs::metadata(&w.real).map_err(|e| fail(500, "internal", e.to_string()))?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Ok(
        json!({ "path": w.rel, "kind": if meta.is_dir() { "dir" } else { "file" }, "size": if meta.is_dir() { 0 } else { meta.len() }, "modified": modified }),
    )
}

// ---- the workspace's own state: .dry/ ----

pub fn snapshot_read(c: &Companion) -> Value {
    std::fs::read_to_string(&c.snapshot_file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// The page's snapshot, kept as given: an object with `"version": 1`.
pub fn snapshot_write(c: &Companion, v: &Value) -> Result<Value, Fail> {
    if !v.is_object() || v.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(fail(400, "bad_snapshot", "a snapshot is {\"version\": 1, ...} (docs/companion-protocol.md §7)"));
    }
    let text = serde_json::to_string_pretty(v).unwrap_or_default() + "\n";
    write_atomic(&c.snapshot_file, &text).map_err(|e| fail(500, "internal", e.to_string()))?;
    Ok(json!({ "ok": true, "path": format!("{DRY_DIR}/snapshot.json") }))
}

pub fn workspace(c: &Companion) -> Value {
    let state = c
        .store_dir
        .strip_prefix(&c.folder)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| c.store_dir.display().to_string());
    json!({
        "protocol": PROTOCOL,
        "folder": c.folder,
        "name": c.folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        "state": state,
        "snapshot": snapshot_read(c),
        "switches": { "file": c.is_on("file"), "terminal": c.is_on("terminal") },
    })
}

/// Every text of the store (`.dry/state/`), by its path.
pub fn store_read(c: &Companion) -> Value {
    fn walk(dir: &Path, rel: &str, out: &mut Map<String, Value>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".tmp") {
                continue;
            }
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path(), &r, out),
                Ok(t) if t.is_file() => {
                    if let Ok(b) = std::fs::read(e.path()) {
                        out.insert(r, json!(String::from_utf8_lossy(&b)));
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Map::new();
    walk(&c.store_dir, "", &mut out);
    json!({ "texts": out })
}

/// Texts put and removed, each path checked first.
pub fn store_write(c: &Companion, put: &Value, remove: &Value) -> Result<(), Fail> {
    let empty = Map::new();
    let put = match put {
        Value::Null => &empty,
        Value::Object(m) => m,
        _ => return Err(fail(400, "bad_request", "put is {path: text}")),
    };
    let remove: Vec<&str> = match remove {
        Value::Null => Vec::new(),
        Value::Array(a) => a.iter().map(|v| v.as_str().unwrap_or("")).collect(),
        _ => return Err(fail(400, "bad_request", "remove is [path]")),
    };
    for p in put.keys().map(String::as_str).chain(remove.iter().copied()) {
        if !rules::store_path_ok(p) {
            return Err(fail(400, "bad_request", format!("not a store path: {p}")));
        }
    }
    let place = |p: &str| p.split('/').fold(c.store_dir.clone(), |a, s| a.join(s));
    for (p, text) in put {
        let text = match text {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        write_atomic(&place(p), &text).map_err(|e| fail(500, "internal", format!("{p}: {e}")))?;
    }
    for p in remove {
        let _ = std::fs::remove_file(place(p));
    }
    Ok(())
}
