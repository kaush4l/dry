//! The template: how an engine's prompt is laid out, from `spec/template.md`.
//!
//! ```markdown
//! | phase  | tools      | response | skills |
//! |--------|------------|----------|--------|
//! | triage | read, list | standard |        |
//! | code   |            |          |        |
//!
//! ## instructions
//!
//! ## learning
//!
//! ## phase: triage
//! Sort the request: what is it, what does it need.
//!
//! ## phase: code
//! Make the change.
//!
//! ## tools
//! title: Your tools
//! Call these in `act`:
//!
//! ## artifacts
//!
//! ## history
//!
//! ## response
//! ```
//!
//! One `##` section per slot, in the fixed order of `Slot` (the engine
//! standard): the order never moves, so the same state sends the same
//! prompt. A section starts with its metadata, `key: value` lines (which
//! keys a slot takes is in `Slot::keys`), then its text: for a slot, the
//! lead shown at its top; for a phase, its goal.
//!
//! Phases are metadata driven: their metadata is the table at the top
//! (one row per phase, a column per key of `PHASE_COLUMNS`, an empty cell
//! for the default), and every row has its `## phase: <name>` section with
//! the goal, in the same order. A row without a section, or a section
//! without a row, is an error that names it.
//!
//! Everything is optional. A slot left out of the file keeps its default;
//! phases are there only if the template has `## phase: <name>` sections
//! (`crate::phase` runs them). No `template.md` at all is `DEFAULT`.
//!
//! Pure: text in, `Template` out.

use crate::phase::{Phase, ResponseMode};

/// The prompt's slots, in the only order they are sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Instructions,
    /// What the engine learned (`memory/learning.md`, `crate::learning`).
    Learning,
    Phase,
    Tools,
    Artifacts,
    History,
    Response,
}

impl Slot {
    pub const ALL: [Slot; 7] = [Slot::Instructions, Slot::Learning, Slot::Phase, Slot::Tools, Slot::Artifacts, Slot::History, Slot::Response];

    /// Its word in a `##` heading.
    pub fn word(self) -> &'static str {
        match self {
            Slot::Instructions => "instructions",
            Slot::Learning => "learning",
            Slot::Phase => "phase",
            Slot::Tools => "tools",
            Slot::Artifacts => "artifacts",
            Slot::History => "history",
            Slot::Response => "response",
        }
    }

    /// Its heading in the prompt, unless the template's `title:` says
    /// otherwise.
    pub fn title(self) -> &'static str {
        match self {
            Slot::Instructions => "Instructions",
            Slot::Learning => "Learning",
            Slot::Phase => "Phase",
            Slot::Tools => "Tools",
            Slot::Artifacts => "Artifacts",
            Slot::History => "History",
            Slot::Response => "Response",
        }
    }

    /// The metadata keys its section takes.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            // A phase's metadata is its row of the phase table.
            Slot::Phase | Slot::History => &[],
            _ => &["title"],
        }
    }

    fn parse(word: &str) -> Option<Slot> {
        Slot::ALL.into_iter().find(|s| s.word() == word)
    }
}

/// The phase table's columns after `phase` (the name), in any order; a
/// column left out, or an empty cell, is the default.
pub const PHASE_COLUMNS: [&str; 3] = ["tools", "response", "skills"];

/// One `##` section: a slot (a phase also has its name), its metadata and
/// its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub slot: Slot,
    /// A phase's name; none for the other slots.
    pub name: Option<String>,
    pub meta: Vec<(String, String)>,
    pub text: String,
}

impl Section {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// The template an engine without `template.md` uses: every slot, no
/// phases, the tools introduced the usual way.
pub const DEFAULT: &str = "## instructions\n\n## learning\nWhat you learned so far:\n\n## tools\nCall these in `act`:\n\n## artifacts\n\n## history\n\n## response\n";

fn default_lead(slot: Slot) -> String {
    parse(DEFAULT).ok().and_then(|t| t.section(slot).map(|s| s.text.clone())).unwrap_or_default()
}

/// A read template.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub sections: Vec<Section>,
}

/// Read `template.md`. Blank text is `DEFAULT`.
pub fn parse(md: &str) -> Result<Template, String> {
    let md = if md.trim().is_empty() { DEFAULT } else { md };
    let mut sections: Vec<Section> = Vec::new();
    let mut table: Vec<&str> = Vec::new();
    let mut in_text = false;
    for line in md.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            let head = head.trim();
            let (word, name) = match head.split_once(':') {
                Some((w, n)) => (w.trim(), Some(n.trim().to_string())),
                None => (head, None),
            };
            let slot = Slot::parse(word).ok_or_else(|| {
                format!("template.md: `## {head}` is not a slot; the slots are {}", Slot::ALL.map(Slot::word).join(", "))
            })?;
            match (slot, &name) {
                (Slot::Phase, Some(n)) if !n.is_empty() => {
                    if sections.iter().any(|s| s.name.as_ref() == Some(n)) {
                        return Err(format!("template.md: two phases named `{n}`"));
                    }
                }
                (Slot::Phase, _) => return Err("template.md: a phase needs a name, like `## phase: plan`".into()),
                (_, Some(_)) => return Err(format!("template.md: only a phase has a name, not `## {head}`")),
                _ if sections.iter().any(|s| s.slot == slot) => return Err(format!("template.md: `## {word}` twice")),
                _ => {}
            }
            if let Some(last) = sections.last().filter(|s| s.slot > slot) {
                return Err(format!(
                    "template.md: `## {head}` comes after `## {}`; the order is fixed: {}",
                    last.slot.word(),
                    Slot::ALL.map(Slot::word).join(", ")
                ));
            }
            sections.push(Section { slot, name, meta: Vec::new(), text: String::new() });
            in_text = false;
            continue;
        }
        let Some(section) = sections.last_mut() else {
            if line.trim().is_empty() || line.starts_with("# ") {
                continue;
            }
            if line.trim_start().starts_with('|') {
                table.push(line);
                continue;
            }
            return Err("template.md: text before the first `## ` section (only a title and the phase table go there)".into());
        };
        // Metadata: `key: value` lines before the text, for keys the slot takes.
        if !in_text
            && let Some((key, value)) = line.split_once(':')
            && section.slot.keys().contains(&key.trim())
        {
            section.meta.push((key.trim().to_string(), value.trim().to_string()));
            continue;
        }
        if !line.trim().is_empty() {
            in_text = true;
        }
        section.text.push_str(line);
        section.text.push('\n');
    }
    for s in &mut sections {
        s.text = s.text.trim().to_string();
    }
    apply_table(&table, &mut sections)?;
    let t = Template { sections };
    t.phases()?;
    Ok(t)
}

/// A table row's cells, without the outer `|`.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|c| c.trim().to_string()).collect()
}

/// Read the phase table and give each phase section its row as metadata.
/// The rows and the `## phase:` sections must match, name for name, in
/// order.
fn apply_table(table: &[&str], sections: &mut [Section]) -> Result<(), String> {
    let headers: Vec<String> = sections.iter().filter(|s| s.slot == Slot::Phase).filter_map(|s| s.name.clone()).collect();
    let mut rows: Vec<Vec<String>> = table.iter().map(|l| cells(l)).collect();
    let Some(columns) = (!rows.is_empty()).then(|| rows.remove(0)) else {
        return match headers.first() {
            Some(n) => Err(format!("template.md: `## phase: {n}` needs its row in the phase table at the top (| phase | tools | ... |)")),
            None => Ok(()),
        };
    };
    if columns.first().map(String::as_str) != Some("phase") {
        return Err("template.md: the phase table's first column is `phase`".into());
    }
    for c in &columns[1..] {
        if !PHASE_COLUMNS.contains(&c.as_str()) {
            return Err(format!("template.md: the phase table has no column `{c}`; the columns are phase, {}", PHASE_COLUMNS.join(", ")));
        }
    }
    // The `|---|` line under the header.
    if rows.first().is_some_and(|r| r.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))) {
        rows.remove(0);
    }
    let mut names: Vec<String> = Vec::new();
    for row in &rows {
        if row.len() != columns.len() {
            return Err(format!("template.md: a phase table row has {} cells, the header {}: {}", row.len(), columns.len(), row.join(" | ")));
        }
        let name = &row[0];
        if name.is_empty() {
            return Err("template.md: a phase table row has no phase name".into());
        }
        if names.contains(name) {
            return Err(format!("template.md: two phases named `{name}`"));
        }
        names.push(name.clone());
        let section = sections
            .iter_mut()
            .find(|s| s.slot == Slot::Phase && s.name.as_ref() == Some(name))
            .ok_or_else(|| format!("template.md: phase `{name}` is in the table but has no `## phase: {name}` section"))?;
        section.meta = columns[1..].iter().zip(&row[1..]).filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
    }
    if let Some(n) = headers.iter().find(|h| !names.contains(h)) {
        return Err(format!("template.md: `## phase: {n}` is not in the phase table"));
    }
    if names != headers {
        return Err(format!("template.md: the phase table ({}) and the `## phase:` sections ({}) are in a different order", names.join(", "), headers.join(", ")));
    }
    Ok(())
}

impl Template {
    /// The section of slot `slot` (not a phase).
    pub fn section(&self, slot: Slot) -> Option<&Section> {
        self.sections.iter().find(|s| s.slot == slot && s.name.is_none())
    }

    /// The heading slot `slot` is shown under.
    pub fn title(&self, slot: Slot) -> String {
        self.section(slot).and_then(|s| s.get("title")).map_or(slot.title().to_string(), str::to_string)
    }

    /// The lead text at the top of slot `slot`; a slot left out of the
    /// template keeps the default's.
    pub fn lead(&self, slot: Slot) -> String {
        match self.section(slot) {
            Some(s) => s.text.clone(),
            None => default_lead(slot),
        }
    }

    /// The phases, in order. None: the run has one implicit phase.
    pub fn phases(&self) -> Result<Vec<Phase>, String> {
        self.sections
            .iter()
            .filter(|s| s.slot == Slot::Phase)
            .map(|s| {
                let name = s.name.clone().unwrap_or_default();
                let tools = s.get("tools").map_or(Vec::new(), |l| l.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect());
                let response = match s.get("response") {
                    None => ResponseMode::Standard,
                    Some(w) => ResponseMode::parse(w).ok_or_else(|| format!("template.md: phase `{name}` response is standard or jev, not {w}"))?,
                };
                let skills = s.get("skills").map(|l| l.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect());
                Ok(Phase { name, tools, response, skills, goal: s.text.clone() })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "# My engine's template\n\n| phase | tools | response |\n|---|---|---|\n| triage | read, list | jev |\n| code | | |\n\n## instructions\n\n## phase: triage\nSort it.\nNote: be quick.\n\n## phase: code\nWrite it.\n\n## tools\ntitle: Your tools\nCall these in `act`:\n\n## response\n";

    #[test]
    fn the_table_holds_the_phase_metadata_and_the_sections_their_goals() {
        let t = parse(MD).unwrap();
        let p = t.phases().unwrap();
        assert_eq!(p.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["triage", "code"]);
        assert_eq!((p[0].tools.clone(), p[0].response), (vec!["read".to_string(), "list".to_string()], ResponseMode::Jev));
        assert_eq!((p[1].tools.clone(), p[1].response, p[1].skills.clone()), (vec![], ResponseMode::Standard, None), "empty cell, missing column: default");
        assert_eq!(p[0].goal, "Sort it.\nNote: be quick.");
        assert_eq!((t.title(Slot::Tools), t.lead(Slot::Tools)), ("Your tools".to_string(), "Call these in `act`:".to_string()));
        let bare = parse("| phase |\n|-|\n| only |\n\n## phase: only\nGo.").unwrap();
        assert_eq!(bare.lead(Slot::Tools), "Call these in `act`:", "a slot left out keeps the default lead");
        assert_eq!(t.title(Slot::Artifacts), "Artifacts", "a slot left out keeps its default");
        let in_goal = parse("| phase | tools |\n|-|-|\n| a | read |\n\n## phase: a\ntools: list\n").unwrap().phases().unwrap();
        assert_eq!((in_goal[0].tools.clone(), in_goal[0].goal.as_str()), (vec!["read".to_string()], "tools: list"), "a section's `key:` line is its goal, the table is the metadata");
    }

    #[test]
    fn no_template_is_the_default_with_no_phases() {
        let t = parse("").unwrap();
        assert!(t.phases().unwrap().is_empty(), "phases are optional");
        assert_eq!(t.lead(Slot::Tools), "Call these in `act`:");
        assert_eq!(t.sections.len(), 6);
        assert_eq!(t.title(Slot::Learning), "Learning");
        assert!(t.lead(Slot::Learning).starts_with("What you learned so far"));
    }

    #[test]
    fn the_table_and_the_sections_match_name_for_name() {
        let err = |md: &str| parse(md).unwrap_err();
        let table = "| phase |\n|---|\n| a |\n| b |\n\n";
        assert!(err(&format!("{table}## phase: a\n")).contains("phase `b` is in the table but has no `## phase: b`"));
        assert!(err(&format!("{table}## phase: a\n## phase: b\n## phase: c\n")).contains("`## phase: c` is not in the phase table"));
        assert!(err(&format!("{table}## phase: b\n## phase: a\n")).contains("different order"));
        assert!(err("## phase: a\nDo it.").contains("needs its row in the phase table"));
        assert!(err("| name | tools |\n|-|-|\n| a | x |\n## phase: a\n").contains("first column is `phase`"));
        assert!(err("| phase | steps |\n|-|-|\n| a | 1 |\n## phase: a\n").contains("no column `steps`"));
        assert!(err("| phase | tools |\n|-|-|\n| a | x | y |\n## phase: a\n").contains("3 cells, the header 2"));
        assert!(err("| phase |\n|-|\n| a |\n| a |\n## phase: a\n").contains("two phases"));
    }

    #[test]
    fn the_order_is_fixed_and_mistakes_say_what_is_wrong() {
        let err = |md: &str| parse(md).unwrap_err();
        let a = "| phase |\n|-|\n| a |\n";
        assert!(err("## tools\n\n## instructions\n").contains("the order is fixed"));
        assert!(err(&format!("{a}## phase: a\n\n## instructions\n")).contains("comes after `## phase`"));
        assert!(err(&format!("{a}## phase: a\n\n## learning\n")).contains("comes after `## phase`"), "learning is slot 2, before the phases");
        assert!(parse(&format!("{a}## instructions\n\n## learning\ntitle: Notes\n\n## phase: a\n")).is_ok());
        assert!(err("## learning\n## instructions\n").contains("the order is fixed: instructions, learning, phase"));
        assert!(err("## prompt\n").contains("is not a slot"));
        assert!(err("## phase\n").contains("needs a name"));
        assert!(err("## tools: mine\n").contains("only a phase has a name"));
        assert!(err("## tools\n## tools\n").contains("twice"));
        assert!(err("## phase: a\n## phase: a\n").contains("two phases"));
        assert!(err("| phase | response |\n|-|-|\n| a | fast |\n## phase: a\n").contains("standard or jev"));
        assert!(err("hello\n## tools\n").contains("before the first"));
    }
}
