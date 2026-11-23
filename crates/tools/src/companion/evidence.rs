//! The workspace's evidence (docs/evidence.md) from the page: dry's own
//! evidence code (`crate::evidence::run`, in this process: no `dry`
//! binary to find, no Node), and its folder read back.

use std::sync::atomic::Ordering;
use std::time::Instant;

use dry_core::evidence::report::Manifest;
use serde_json::{Value, json};

use super::folder;
use super::rules::{Fail, fail};
use super::state::Companion;

/// A project: a folder of the workspace (default the folder itself),
/// confined like every path.
fn project(c: &Companion, p: &str) -> Result<std::path::PathBuf, Fail> {
    let w = folder::ws_path(c, p, false, true)?;
    Ok(w.real)
}

/// Its evidence now: the line, the summary, the manifest.
pub fn read(c: &Companion, p: &str) -> Result<Value, Fail> {
    let dir = project(c, p)?;
    let ev = dir.join("evidence");
    let manifest = std::fs::read_to_string(ev.join("manifest.json")).ok();
    let summary = std::fs::read_to_string(ev.join("summary.md")).unwrap_or_default();
    let parsed = manifest.as_deref().and_then(|t| Manifest::from_json(t).ok());
    let line = parsed.as_ref().map(Manifest::line).unwrap_or_else(|| "no evidence yet".into());
    let manifest: Value = manifest.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    Ok(json!({ "project": dir, "line": line, "summary": summary, "manifest": manifest }))
}

/// Run it and wait: `{line, summary, manifest, code, output, ms}` (`code`
/// 0: the same, 1: an unexpected change, 2: it could not run).
pub fn run(c: &Companion, b: &Value) -> Result<Value, Fail> {
    c.need("evidence")?;
    let dir = project(c, b["project"].as_str().unwrap_or(""))?;
    if !dir.join("evidence.md").is_file() {
        return Err(fail(404, "not_found", format!("{} has no evidence.md (docs/evidence.md)", dir.display())));
    }
    let expect: Vec<String> =
        b["expect"].as_str().unwrap_or("").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if expect.iter().any(|e| !e.chars().all(|ch| ch.is_ascii_alphanumeric() || "_@-".contains(ch))) {
        return Err(fail(400, "bad_request", "expect is scenario names (or name@WxH), comma separated"));
    }
    let o = crate::evidence::Options {
        project: dir.clone(),
        update: b["update"].as_bool().unwrap_or(false),
        expect,
        repeat: 1,
        chrome: c.chrome.clone(),
        quiet: true,
        ..crate::evidence::Options::default()
    };
    let t0 = Instant::now();
    c.log(&format!("evidence {}{}", dir.display(), if o.update { " --update" } else { "" }));
    c.evidence_runs.fetch_add(1, Ordering::Relaxed);
    let r = crate::evidence::run(&o);
    c.evidence_runs.fetch_sub(1, Ordering::Relaxed);
    let (code, output) = match &r {
        Ok(m) => (if m.status == "pass" { 0 } else { 1 }, format!("evidence: {}", m.line())),
        Err(e) => (2, format!("dry evidence: {e}")),
    };
    let mut v = read(c, b["project"].as_str().unwrap_or(""))?;
    if let Ok(m) = &r {
        v["line"] = json!(m.line());
    }
    v["code"] = json!(code);
    v["output"] = json!(output);
    v["ms"] = json!(t0.elapsed().as_millis() as u64);
    c.log(&format!("evidence: {} (exit {code})", v["line"].as_str().unwrap_or("")));
    Ok(v)
}

/// One evidence image (a shot, its `.new` or `.diff`), only from its folder.
pub fn file(c: &Companion, p: &str, name: &str) -> Result<Vec<u8>, Fail> {
    let dir = project(c, p)?;
    crate::evidence::image(&dir, ".", name).map_err(|e| {
        fail(if e.starts_with("not an") { 400 } else { 404 }, if e.starts_with("not an") { "bad_request" } else { "not_found" }, e)
    })
}
