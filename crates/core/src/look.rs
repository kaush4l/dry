//! Looks: the themes and the scenes, each one folder you can share.
//!
//! A look is a folder, `themes/<name>/` or `scenes/<name>/`, holding its
//! main file (`theme.md`, `scene.md`) and whatever else it brings beside
//! it (text files only, for now). Which one is in use is a setting, not a
//! file in the folder: `app.md`'s `theme:` and `scene:`
//! (`crate::files::Settings`), which a machine's `state/local.md` or a
//! route's overrides may beat.
//!
//! The same verbs work on both kinds (the shells do the disk; this is the
//! part every shell shares): list the names (`names`), check a main file
//! (`check`), edit a value (`edit`), and share a folder as one text
//! (`export`) that comes back as a folder (`import`).
//!
//! The share text is markdown: a `# dry <kind>: <name>` title, then each
//! file as a `## <path>` heading over a tilde fence holding its text.
//!
//! Pure: no I/O.

use crate::theme::{Mode, Theme};

/// What a look is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The design tokens: colours, sizes, fonts (`crate::theme`).
    Theme,
    /// An animated background or a pet (`crate::scene`).
    Scene,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Theme, Kind::Scene];

    /// `theme`, `scene`: the word on the command line and in the share title.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Theme => "theme",
            Kind::Scene => "scene",
        }
    }

    pub fn parse(word: &str) -> Option<Kind> {
        match word.trim() {
            "theme" | "themes" => Some(Kind::Theme),
            "scene" | "scenes" => Some(Kind::Scene),
            _ => None,
        }
    }

    /// The folder every look of this kind is in, from the repo root.
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Theme => "themes",
            Kind::Scene => "scenes",
        }
    }

    /// The main file in a look's folder.
    pub fn main(self) -> &'static str {
        match self {
            Kind::Theme => "theme.md",
            Kind::Scene => "scene.md",
        }
    }
}

/// A look's name: letters, digits, `-` and `_` (one folder, no path).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A file's path inside a look's folder: `/`-separated parts of letters,
/// digits, `-`, `_` and `.`, none starting with a dot (no `..`, no hidden
/// files, nothing outside the folder).
pub fn valid_file(path: &str) -> bool {
    !path.is_empty()
        && path
            .split('/')
            .all(|p| !p.is_empty() && !p.starts_with('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
}

/// `themes/<name>/theme.md`: a look's main file from the repo root.
pub fn main_path(kind: Kind, name: &str) -> String {
    format!("{}/{name}/{}", kind.dir(), kind.main())
}

/// The name a path from the repo root is the main file of, if it is one.
pub fn name_of(kind: Kind, path: &str) -> Option<String> {
    let rest = path.strip_prefix(kind.dir())?.strip_prefix('/')?;
    let name = rest.strip_suffix(kind.main())?.strip_suffix('/')?;
    valid_name(name).then(|| name.to_string())
}

/// Every look of `kind` among `paths` (from the repo root), sorted.
pub fn names<'a>(kind: Kind, paths: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = paths.into_iter().filter_map(|p| name_of(kind, p)).collect();
    out.sort();
    out.dedup();
    out
}

/// Whether `text` reads as the main file of a `kind` look named `name`.
pub fn check(kind: Kind, name: &str, text: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!("\"{name}\" is not a {} name (letters, digits, - and _)", kind.word()));
    }
    match kind {
        Kind::Theme => {
            let t = Theme::from_md(text)?;
            let missing = t.missing();
            if missing.is_empty() { Ok(()) } else { Err(format!("tokens missing: {}", missing.join(", "))) }
        }
        Kind::Scene => crate::scene::Scene::from_md(name, text).map(|_| ()),
    }
}

/// `text`, a `kind` look's main file, with `key` set to `value` (in
/// `mode` only, where a value has one per mode; both when `None`).
///
/// A theme: `key` is a token (`Theme::set`). A scene: a row of its
/// `## Scene` table (`kind`, `place`, `fps`, `cell`, `pixel`) or a key of
/// its `## Palette` (a colour, `#RRGGBB` or `#RRGGBBAA`). The result is
/// checked before it is returned.
pub fn edit(kind: Kind, name: &str, text: &str, key: &str, value: &str, mode: Option<Mode>) -> Result<String, String> {
    let out = match kind {
        Kind::Theme => {
            let mut t = Theme::from_md(text)?;
            t.set(key, value, mode)?;
            t.to_md()
        }
        Kind::Scene => {
            let columns: &[&str] = match mode {
                Some(Mode::Light) => &["light"],
                Some(Mode::Dark) => &["dark"],
                None => &["light", "dark"],
            };
            set_cells(text, "Scene", key, &["value"], value)
                .or_else(|_| set_cells(text, "Palette", key, columns, value))
                .map_err(|_| format!("scene {name} has no setting or palette key \"{key}\""))?
        }
    };
    check(kind, name, &out).map(|_| out)
}

/// `md` with the row whose first cell is `key`, in the table under
/// `## <section>`, given `value` in each of `columns` (by header name).
fn set_cells(md: &str, section: &str, key: &str, columns: &[&str], value: &str) -> Result<String, String> {
    if value.contains('|') || value.contains('\n') {
        return Err("a value has no | or line break".into());
    }
    let heading = format!("## {section}");
    let mut out = Vec::new();
    let (mut inside, mut header, mut done) = (false, None::<Vec<String>>, false);
    for line in md.lines() {
        if line.starts_with("## ") {
            inside = line.trim() == heading;
            header = None;
        }
        if inside
            && !done
            && let Some(cells) = row(line)
        {
            match &header {
                None => header = Some(cells.iter().map(|c| c.to_lowercase()).collect()),
                Some(h) if cells.first().map(String::as_str) == Some(key) => {
                    let mut cells = cells.clone();
                    for col in columns {
                        let i = h.iter().position(|c| c == col).ok_or_else(|| format!("no {col} column"))?;
                        *cells.get_mut(i).ok_or_else(|| format!("row {key} has no {col} cell"))? = value.to_string();
                    }
                    out.push(format!("| {} |", cells.join(" | ")));
                    done = true;
                    continue;
                }
                Some(_) => {}
            }
        }
        out.push(line.to_string());
    }
    if !done {
        return Err(format!("no row {key} under {heading}"));
    }
    let mut text = out.join("\n");
    if md.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

/// A table row's cells, trimmed; `None` for any other line and for the
/// `|---|` rule under a header.
fn row(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    let cells: Vec<String> = inner.split('|').map(|c| c.trim().to_string()).collect();
    if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
        return None;
    }
    Some(cells)
}

/// A look as one text to share: every file of its folder (paths inside
/// it, the main file first, then by path).
pub fn export(kind: Kind, name: &str, files: &[(String, String)]) -> String {
    let mut files: Vec<&(String, String)> = files.iter().collect();
    files.sort_by_key(|(p, _)| (p != kind.main(), p.clone()));
    let mut out = format!(
        "# dry {}: {name}\n\nOne {} folder as one file. Import it with `dry {} import <file>` or from the settings page.\n",
        kind.word(),
        kind.word(),
        kind.word()
    );
    for (path, text) in files {
        let fence = "~".repeat(longest_run(text, '~').max(2) + 1);
        out.push_str(&format!("\n## {path}\n\n{fence}\n{text}"));
        if !text.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("{fence}\n"));
    }
    out
}

fn longest_run(text: &str, ch: char) -> usize {
    let (mut best, mut now) = (0, 0);
    for c in text.chars() {
        now = if c == ch { now + 1 } else { 0 };
        best = best.max(now);
    }
    best
}

/// A look read back from `export`'s text.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub kind: Kind,
    pub name: String,
    /// `(path in the folder, text)`, the main file among them.
    pub files: Vec<(String, String)>,
}

/// Read `export`'s text. Every path must be a `valid_file`, the main file
/// must be there and `check` must pass.
pub fn import(text: &str) -> Result<Pack, String> {
    let mut lines = text.lines();
    let title = lines.find(|l| !l.trim().is_empty()).unwrap_or_default();
    let (kind, name) = title
        .strip_prefix("# dry ")
        .and_then(|t| t.split_once(':'))
        .and_then(|(k, n)| Some((Kind::parse(k)?, n.trim().to_string())))
        .ok_or("not a dry look: the first line is `# dry theme: <name>` or `# dry scene: <name>`")?;
    if !valid_name(&name) {
        return Err(format!("\"{name}\" is not a {} name", kind.word()));
    }
    let mut files: Vec<(String, String)> = Vec::new();
    while let Some(line) = lines.next() {
        let Some(path) = line.strip_prefix("## ").map(str::trim) else { continue };
        if !valid_file(path) {
            return Err(format!("\"{path}\" is not a file path inside a look's folder"));
        }
        let fence = lines.by_ref().find(|l| !l.trim().is_empty()).map(str::trim).unwrap_or_default();
        if fence.len() < 3 || !fence.chars().all(|c| c == '~') {
            return Err(format!("{path}: its text is not in a ~~~ fence"));
        }
        let mut body = Vec::new();
        let mut closed = false;
        for l in lines.by_ref() {
            if l.trim_end() == fence {
                closed = true;
                break;
            }
            body.push(l);
        }
        if !closed {
            return Err(format!("{path}: its fence is never closed"));
        }
        if files.iter().any(|(p, _)| p == path) {
            return Err(format!("{path} is in it twice"));
        }
        files.push((path.to_string(), format!("{}\n", body.join("\n"))));
    }
    let main = files.iter().find(|(p, _)| p == kind.main()).ok_or_else(|| format!("no {} in it", kind.main()))?;
    check(kind, &name, &main.1)?;
    Ok(Pack { kind, name, files })
}

/// Every look of `kind` the repo ships, `(name, main file's text)`, read
/// from the folders (tests only: no list of names to keep).
#[cfg(test)]
pub(crate) fn shipped_all(kind: Kind) -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(kind.dir());
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let text = std::fs::read_to_string(e.path().join(kind.main())).ok()?;
            valid_name(&name).then_some((name, text))
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no {} in {}", kind.dir(), dir.display());
    out
}

/// One shipped look's main file (tests only).
#[cfg(test)]
pub(crate) fn shipped(kind: Kind, name: &str) -> String {
    shipped_all(kind).into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no shipped {} {name}", kind.word())).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_names() {
        assert_eq!(main_path(Kind::Theme, "bolt"), "themes/bolt/theme.md");
        assert_eq!(name_of(Kind::Scene, "scenes/cat/scene.md").as_deref(), Some("cat"));
        assert_eq!(name_of(Kind::Scene, "scenes/cat/sprites.md"), None);
        assert_eq!(name_of(Kind::Theme, "themes/a/b/theme.md"), None, "one folder deep");
        let paths = ["themes/b/theme.md", "themes/a/theme.md", "themes/a/notes.md", "scenes/cat/scene.md"];
        assert_eq!(names(Kind::Theme, paths), ["a", "b"]);
        assert!(valid_file("sprites/walk.md") && !valid_file("../x.md") && !valid_file(".env") && !valid_file("a//b") && !valid_file("/a"));
        assert!(!valid_name("a/b") && !valid_name("") && !valid_name(".."));
    }

    #[test]
    fn a_look_goes_out_as_one_text_and_comes_back_whole() {
        let theme = shipped(Kind::Theme, "default");
        let files =
            vec![("notes.md".to_string(), "Made from default.\n~~~\nfenced\n~~~\n".to_string()), ("theme.md".to_string(), theme.clone())];
        let text = export(Kind::Theme, "mine", &files);
        assert!(text.starts_with("# dry theme: mine\n"), "{text}");
        assert!(text.find("## theme.md").unwrap() < text.find("## notes.md").unwrap(), "the main file first");
        let pack = import(&text).unwrap();
        assert_eq!((pack.kind, pack.name.as_str()), (Kind::Theme, "mine"));
        assert_eq!(pack.files.iter().find(|(p, _)| p == "theme.md").unwrap().1, theme);
        assert_eq!(pack.files.iter().find(|(p, _)| p == "notes.md").unwrap().1, files[0].1, "a ~~~ inside survives");
        let scene = export(Kind::Scene, "cat", &[("scene.md".into(), shipped(Kind::Scene, "cat"))]);
        assert_eq!(import(&scene).unwrap().kind, Kind::Scene);
    }

    #[test]
    fn a_bad_share_is_refused_with_why() {
        assert!(import("hello").unwrap_err().contains("not a dry look"));
        assert!(import("# dry theme: ../x\n").unwrap_err().contains("not a theme name"));
        assert!(import("# dry theme: x\n\n## ../evil.md\n\n~~~\nx\n~~~\n").unwrap_err().contains("not a file path"));
        assert!(import("# dry theme: x\n\n## notes.md\n\n~~~\nx\n~~~\n").unwrap_err().contains("no theme.md"));
        assert!(import("# dry theme: x\n\n## theme.md\n\n~~~\nnot a theme\n~~~\n").is_err());
        assert!(import("# dry theme: x\n\n## theme.md\n\n~~~\nopen\n").unwrap_err().contains("never closed"));
    }

    #[test]
    fn edits_land_in_the_right_cell_and_are_checked() {
        let theme = shipped(Kind::Theme, "default");
        let t = edit(Kind::Theme, "default", &theme, "radius", "12", None).unwrap();
        assert!(t.contains("| radius | 12 |"), "{t}");
        assert!(edit(Kind::Theme, "default", &theme, "no_such_token", "1", None).is_err());

        let cat = shipped(Kind::Scene, "cat");
        let fps = edit(Kind::Scene, "cat", &cat, "fps", "30", None).unwrap();
        assert!(fps.contains("| fps | 30 |"), "{fps}");
        assert_eq!(crate::scene::Scene::from_md("cat", &fps).unwrap().fps(), 30.0);
        let fur = edit(Kind::Scene, "cat", &cat, "f", "#112233", Some(Mode::Dark)).unwrap();
        assert!(fur.contains("| f | accent | #D9733F | #112233 |"), "only the dark cell: {fur}");
        assert!(edit(Kind::Scene, "cat", &cat, "place", "nowhere", None).is_err(), "checked: no such place");
        assert!(edit(Kind::Scene, "cat", &cat, "zz", "1", None).unwrap_err().contains("no setting or palette key"));
        assert!(edit(Kind::Scene, "cat", &cat, "fps", "1 | 2", None).is_err());
    }
}
