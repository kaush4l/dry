//! The spans logs on disk (`dry_core::spans`): `<engines>/<id>/spans.jsonl`,
//! append-only, beside the engine's files, and `spans-daily.jsonl` for the
//! days the retention rolled up. The store's saves leave both alone (they
//! are not texts of the state). When a host runs it is the one writer;
//! a window without a host writes its own turns.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use dry_core::spans::{self, Daily, Span};

/// Engine `engine`'s log.
pub fn file(engines: &Path, engine: &str) -> PathBuf {
    engines.join(engine).join(spans::FILE)
}

fn daily_file(engines: &Path, engine: &str) -> PathBuf {
    engines.join(engine).join(spans::DAILY)
}

/// Unix milliseconds now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The retention: `DRY_SPANS_KEEP_DAYS`, else 30 days.
pub fn keep_days() -> u32 {
    spans::keep_days(std::env::var("DRY_SPANS_KEEP_DAYS").ok().as_deref())
}

/// Append `spans` to their engines' logs (one write per engine). An
/// engine whose folder is not there yet gets it.
pub fn append(engines: &Path, list: &[Span]) -> Result<(), String> {
    let mut by: BTreeMap<&str, String> = BTreeMap::new();
    for s in list {
        if dry_core::workbench::safe(&s.engine) && !s.engine.is_empty() && !s.engine.contains('/') {
            let t = by.entry(s.engine.as_str()).or_default();
            t.push_str(&s.to_line());
            t.push('\n');
        }
    }
    for (engine, text) in by {
        let path = file(engines, engine);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        f.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Engine `engine`'s spans (none when it has no log).
pub fn load(engines: &Path, engine: &str) -> Vec<Span> {
    fs::read_to_string(file(engines, engine)).map(|t| spans::parse(&t)).unwrap_or_default()
}

/// Engine `engine`'s rolled-up days.
pub fn load_daily(engines: &Path, engine: &str) -> Vec<Daily> {
    fs::read_to_string(daily_file(engines, engine)).map(|t| spans::parse_daily(&t)).unwrap_or_default()
}

/// Every engine folder's spans, by engine id, and every rolled-up day.
pub fn load_all(engines: &Path) -> (BTreeMap<String, Vec<Span>>, Vec<Daily>) {
    let mut out = BTreeMap::new();
    let mut daily = Vec::new();
    let Ok(entries) = fs::read_dir(engines) else { return (out, daily) };
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().to_string();
        let s = load(engines, &id);
        if !s.is_empty() {
            out.insert(id.clone(), s);
        }
        daily.extend(load_daily(engines, &id));
    }
    (out, daily)
}

/// Roll every log's days older than `keep_days` into its daily totals:
/// each log is rewritten with the days it keeps (through a temporary
/// file, so a log is never half written). How many spans rolled.
pub fn roll(engines: &Path, now_ms: i64, offset: i32, keep_days: u32) -> usize {
    let mut rolled = 0;
    let Ok(entries) = fs::read_dir(engines) else { return 0 };
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().to_string();
        let list = load(engines, &id);
        if list.is_empty() {
            continue;
        }
        let r = spans::rollup(list, load_daily(engines, &id), now_ms, offset, keep_days);
        if r.rolled == 0 {
            continue;
        }
        let write = |path: PathBuf, text: String| -> Result<(), String> {
            let tmp = path.with_extension("jsonl.tmp");
            fs::write(&tmp, text).map_err(|e| e.to_string())?;
            fs::rename(&tmp, &path).map_err(|e| e.to_string())
        };
        let done = write(daily_file(engines, &id), spans::daily_text(&r.daily)).and_then(|()| write(file(engines, &id), spans::to_text(&r.kept)));
        match done {
            Ok(()) => rolled += r.rolled,
            Err(e) => eprintln!("spans: {id}: the rollup was not written: {e}"),
        }
    }
    rolled
}

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::spans::Kind;

    #[test]
    fn spans_append_per_engine_read_back_and_old_days_roll_up() {
        let dir = std::env::temp_dir().join(format!("dry-spans-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        const DAY: i64 = 86_400_000;
        let now = 1000 * DAY;
        let a = Span::new(Kind::Llm, "a1", "a", now - 40 * DAY, now - 40 * DAY + 5).with("cost_usd", 0.5);
        let b = Span::new(Kind::Tool, "b1", "b", now - 10, now).with("ok", true);
        append(&dir, &[a.clone(), b.clone()]).unwrap();
        append(&dir, &[Span::new(Kind::Turn, "a2", "a", now - 5, now)]).unwrap();
        append(&dir, &[Span::new(Kind::Turn, "x", "../up", 0, 1)]).unwrap();
        assert!(!dir.join("up").exists(), "an engine id never leaves the folder");
        assert_eq!(load(&dir, "a").len(), 2);
        let (all, daily) = load_all(&dir);
        assert_eq!((all.len(), daily.len()), (2, 0));
        assert_eq!(roll(&dir, now, 0, 30), 1);
        assert_eq!(load(&dir, "a").iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a2"]);
        let d = load_daily(&dir, "a");
        assert_eq!((d.len(), d[0].cost_usd, d[0].llm_calls), (1, 0.5, 1));
        assert_eq!(roll(&dir, now, 0, 30), 0, "nothing left to roll");
        let _ = fs::remove_dir_all(&dir);
    }
}
