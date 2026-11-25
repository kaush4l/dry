//! The looks on disk: `themes/<name>/` and `scenes/<name>/`, one folder
//! each (`dry_core::look` has the formats; this is only the disk).
//!
//! The same verbs for both kinds: list, read, add (a copy of one), edit
//! (one value), remove, export (the folder as one text) and import (that
//! text back as a folder). The command line (`dry theme …`, `dry scene …`),
//! the engines' app tools and the host's settings page all call these.

use std::fs;
use std::path::{Path, PathBuf};

use dry_core::look::{self, Kind, Pack};
use dry_core::theme::Mode;

use crate::{Result, read, root, write};

/// `themes/` or `scenes/` under the repo root.
pub fn dir(kind: Kind) -> PathBuf {
    root().join(kind.dir())
}

/// `themes/<name>/`.
pub fn folder(kind: Kind, name: &str) -> PathBuf {
    dir(kind).join(name)
}

/// `themes/<name>/theme.md`.
pub fn main_file(kind: Kind, name: &str) -> PathBuf {
    folder(kind, name).join(kind.main())
}

pub fn list(kind: Kind) -> Vec<String> {
    list_in(&root(), kind)
}

/// Err, naming what there is, unless `name` is a look of `kind`.
pub fn exists(kind: Kind, name: &str) -> Result {
    exists_in(&root(), kind, name)
}

pub fn read_main(kind: Kind, name: &str) -> Result<String> {
    read_main_in(&root(), kind, name)
}

/// Every text file of a look's folder: `(path in the folder, text)`.
pub fn files(kind: Kind, name: &str) -> Result<Vec<(String, String)>> {
    files_in(&root(), kind, name)
}

/// A new look `name`, a copy of `from`'s whole folder. Returns its main file.
pub fn add(kind: Kind, name: &str, from: &str) -> Result<PathBuf> {
    add_in(&root(), kind, name, from)
}

/// Set one value in a look's main file (`look::edit`).
pub fn edit(kind: Kind, name: &str, key: &str, value: &str, mode: Option<Mode>) -> Result {
    edit_in(&root(), kind, name, key, value, mode)
}

/// Delete a look's folder; refused for one named in `in_use`, and for the
/// `default` theme every other falls back to.
pub fn remove(kind: Kind, name: &str, in_use: &[String]) -> Result {
    remove_in(&root(), kind, name, in_use)
}

/// A look's folder as one text to share (`look::export`).
pub fn export(kind: Kind, name: &str) -> Result<String> {
    Ok(look::export(kind, name, &files(kind, name)?))
}

/// `export`'s text back as a folder. One of the same name is refused
/// unless `replace`. Returns what was imported.
pub fn import(text: &str, replace: bool) -> Result<Pack> {
    import_in(&root(), text, replace)
}

// ---- the same, under any root (the tests use a scratch one) -------------------

fn folder_in(base: &Path, kind: Kind, name: &str) -> PathBuf {
    base.join(kind.dir()).join(name)
}

fn list_in(base: &Path, kind: Kind) -> Vec<String> {
    let Ok(entries) = fs::read_dir(base.join(kind.dir())) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| look::valid_name(n) && folder_in(base, kind, n).join(kind.main()).is_file())
        .collect();
    names.sort();
    names
}

fn exists_in(base: &Path, kind: Kind, name: &str) -> Result {
    if look::valid_name(name) && folder_in(base, kind, name).join(kind.main()).is_file() {
        return Ok(());
    }
    let there = list_in(base, kind);
    Err(format!("no {} \"{name}\" ({}: {})", kind.word(), kind.dir(), if there.is_empty() { "none".to_string() } else { there.join(", ") }))
}

fn read_main_in(base: &Path, kind: Kind, name: &str) -> Result<String> {
    if !look::valid_name(name) {
        return Err(format!("\"{name}\" is not a {} name", kind.word()));
    }
    read(&folder_in(base, kind, name).join(kind.main()))
}

fn files_in(base: &Path, kind: Kind, name: &str) -> Result<Vec<(String, String)>> {
    exists_in(base, kind, name)?;
    let top = folder_in(base, kind, name);
    let mut out = Vec::new();
    let mut todo = vec![String::new()];
    while let Some(rel) = todo.pop() {
        let here = if rel.is_empty() { top.clone() } else { top.join(&rel) };
        let mut entries: Vec<_> = fs::read_dir(&here).map_err(|e| format!("{}: {e}", here.display()))?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Some(n) = e.file_name().to_str().map(String::from) else { continue };
            let path = if rel.is_empty() { n.clone() } else { format!("{rel}/{n}") };
            if !look::valid_file(&path) || n.ends_with(".tmp") {
                continue;
            }
            if e.path().is_dir() {
                todo.push(path);
            } else if let Ok(text) = fs::read_to_string(e.path()) {
                // Text only: a file that is not UTF-8 is not shared (yet).
                out.push((path, text));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn add_in(base: &Path, kind: Kind, name: &str, from: &str) -> Result<PathBuf> {
    if !look::valid_name(name) {
        return Err(format!("\"{name}\" is not a {} name (letters, digits, - and _)", kind.word()));
    }
    if folder_in(base, kind, name).exists() {
        return Err(format!("{} \"{name}\" already exists", kind.word()));
    }
    let files = files_in(base, kind, from)?;
    // Checked first, so a broken one is not copied.
    let main = files.iter().find(|(p, _)| p == kind.main()).map(|(_, t)| t.as_str()).unwrap_or_default();
    look::check(kind, name, main).map_err(|e| format!("{} {from} does not read: {e}", kind.word()))?;
    write_folder(base, kind, name, &files)?;
    Ok(folder_in(base, kind, name).join(kind.main()))
}

fn edit_in(base: &Path, kind: Kind, name: &str, key: &str, value: &str, mode: Option<Mode>) -> Result {
    exists_in(base, kind, name)?;
    let text = read_main_in(base, kind, name)?;
    let out = look::edit(kind, name, &text, key, value, mode)?;
    write(&folder_in(base, kind, name).join(kind.main()), &out)
}

fn remove_in(base: &Path, kind: Kind, name: &str, in_use: &[String]) -> Result {
    exists_in(base, kind, name)?;
    if kind == Kind::Theme && name == crate::DEFAULT_THEME {
        return Err("the default theme stays: every other falls back to it".into());
    }
    if in_use.iter().any(|n| n == name) {
        return Err(format!("{} {name} is in use: choose another first", kind.word()));
    }
    let dir = folder_in(base, kind, name);
    fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn import_in(base: &Path, text: &str, replace: bool) -> Result<Pack> {
    let pack = look::import(text)?;
    if folder_in(base, pack.kind, &pack.name).exists() && !replace {
        return Err(format!("{} \"{}\" already exists (import it with replace to overwrite it)", pack.kind.word(), pack.name));
    }
    write_folder(base, pack.kind, &pack.name, &pack.files)?;
    Ok(pack)
}

/// Write a whole folder: into a hidden folder beside it first, then
/// moved into place, so a reader never sees half of one.
fn write_folder(base: &Path, kind: Kind, name: &str, files: &[(String, String)]) -> Result {
    let dir = folder_in(base, kind, name);
    let tmp = base.join(kind.dir()).join(format!(".{name}.new"));
    let _ = fs::remove_dir_all(&tmp);
    for (path, text) in files {
        if !look::valid_file(path) {
            return Err(format!("\"{path}\" is not a file path inside a look's folder"));
        }
        write(&tmp.join(path), text)?;
    }
    if dir.exists() {
        let old = base.join(kind.dir()).join(format!(".{name}.old"));
        let _ = fs::remove_dir_all(&old);
        fs::rename(&dir, &old).map_err(|e| format!("{}: {e}", dir.display()))?;
        fs::rename(&tmp, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let _ = fs::remove_dir_all(&old);
        Ok(())
    } else {
        fs::rename(&tmp, &dir).map_err(|e| format!("{}: {e}", dir.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch root holding a copy of the shipped `default` theme and `cat` scene.
    fn scratch(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("dry-looks-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        for (kind, name) in [(Kind::Theme, "default"), (Kind::Scene, "cat")] {
            let text = read_main_in(&root(), kind, name).unwrap();
            write(&folder_in(&base, kind, name).join(kind.main()), &text).unwrap();
        }
        base
    }

    #[test]
    fn add_edit_export_import_and_remove_a_look() {
        let base = scratch("cycle");
        assert_eq!(list_in(&base, Kind::Theme), ["default"]);
        write(&folder_in(&base, Kind::Theme, "default").join("notes/why.md"), "Plain.\n").unwrap();

        add_in(&base, Kind::Theme, "mine", "default").unwrap();
        assert_eq!(list_in(&base, Kind::Theme), ["default", "mine"]);
        assert!(folder_in(&base, Kind::Theme, "mine").join("notes/why.md").is_file(), "the whole folder is copied");
        assert!(add_in(&base, Kind::Theme, "mine", "default").unwrap_err().contains("already exists"));
        assert!(add_in(&base, Kind::Theme, "../x", "default").is_err());
        assert!(add_in(&base, Kind::Theme, "x", "nope").unwrap_err().contains("no theme \"nope\" (themes: default, mine)"));

        edit_in(&base, Kind::Theme, "mine", "radius", "14", None).unwrap();
        assert!(read_main_in(&base, Kind::Theme, "mine").unwrap().contains("| radius | 14 |"));
        edit_in(&base, Kind::Scene, "cat", "fps", "30", None).unwrap();
        assert!(read_main_in(&base, Kind::Scene, "cat").unwrap().contains("| fps | 30 |"));

        let shared = look::export(Kind::Theme, "mine", &files_in(&base, Kind::Theme, "mine").unwrap());
        assert!(import_in(&base, &shared, false).unwrap_err().contains("already exists"));
        remove_in(&base, Kind::Theme, "mine", &[]).unwrap();
        assert_eq!(list_in(&base, Kind::Theme), ["default"]);
        let pack = import_in(&base, &shared, false).unwrap();
        assert_eq!((pack.kind, pack.name.as_str()), (Kind::Theme, "mine"));
        assert!(read_main_in(&base, Kind::Theme, "mine").unwrap().contains("| radius | 14 |"), "it comes back as it went");
        assert!(folder_in(&base, Kind::Theme, "mine").join("notes/why.md").is_file());
        import_in(&base, &shared.replace("| radius | 14 |", "| radius | 9 |"), true).unwrap();
        assert!(read_main_in(&base, Kind::Theme, "mine").unwrap().contains("| radius | 9 |"), "replace overwrites");
        assert!(!base.join("themes/.mine.new").exists() && !base.join("themes/.mine.old").exists());

        assert!(remove_in(&base, Kind::Theme, "default", &[]).unwrap_err().contains("default theme stays"));
        assert!(remove_in(&base, Kind::Theme, "mine", &["mine".into()]).unwrap_err().contains("in use"));
        let _ = fs::remove_dir_all(&base);
    }
}
