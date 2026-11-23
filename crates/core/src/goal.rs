//! `spec/goal.md`: what an engine is for, written by the person, read as
//! sections. Each section has one meaning and one place in the
//! instructions slot (the first slot of the prompt).
//!
//! ```markdown
//! ## Goal
//! Keep the dry-learn tests green.
//!
//! ## Scope
//! Run tests and read code alone. Ask before changing a public API.
//! Never commit.
//!
//! ## Done
//! Every test passes; reply with the command you ran and its last line.
//! ```
//!
//! | section | means | sent as |
//! |---|---|---|
//! | `Goal`  | what the engine is for, who it serves | `### Goal — …`, first |
//! | `Scope` | what it does alone, when it stops and asks, what it never does | `### Scope — …`, second |
//! | `Done`  | what a finished result looks like and how it is shown | `### Done — …`, last |
//!
//! The sections are sent in that order whatever the file's order. What
//! does not fit is reported the way the template reports problems (in the
//! prompt view's notes, never silently): a missing section, a section
//! that is none of these (not sent), one given twice (the second not
//! sent), text before the first section (not sent). A file with no
//! sections at all is an old `instruction.md`-style goal: all of it is
//! read as the Goal, and the note says so.
//!
//! The engine never edits its own `goal.md`; a parent writes its child's
//! when it makes it (`crate::engines`).
//!
//! Pure: text in, sections and notes out.

/// The sections, in the order they are sent: `(name, what it means)`.
pub const SECTIONS: [(&str, &str); 3] = [
    ("Goal", "what you are for, and who you serve"),
    ("Scope", "what you do alone, when you stop and ask, what you never do"),
    ("Done", "what a finished result looks like, and how you show it"),
];

/// The file name in `spec/`, and the old one read while it is missing.
pub const FILE: &str = "goal.md";
pub const OLD_FILE: &str = "instruction.md";

/// A read `goal.md`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Goal {
    /// Each section's text, in `SECTIONS` order; `None` when missing.
    pub sections: [Option<String>; 3],
    /// What did not fit, for the prompt view.
    pub notes: Vec<String>,
}

/// Read `goal.md`.
pub fn parse(md: &str) -> Goal {
    let mut g = Goal::default();
    if md.trim().is_empty() {
        return g;
    }
    let names = || SECTIONS.map(|(n, _)| n).join(", ");
    // (heading as written, index of a known section, text).
    let mut found: Vec<(String, Option<usize>, String)> = Vec::new();
    let mut before = String::new();
    for line in md.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            let head = head.trim().to_string();
            let known = SECTIONS.iter().position(|(n, _)| n.eq_ignore_ascii_case(&head));
            found.push((head, known, String::new()));
            continue;
        }
        match found.last_mut() {
            Some((_, _, text)) => {
                text.push_str(line);
                text.push('\n');
            }
            // A `# title` above the sections is fine.
            None if line.starts_with("# ") => {}
            None => {
                before.push_str(line);
                before.push('\n');
            }
        }
    }
    if found.is_empty() {
        g.sections[0] = Some(md.trim().to_string());
        g.notes.push(format!("goal.md has no sections ({}): all of it is read as the Goal", names()));
        return g;
    }
    if !before.trim().is_empty() {
        g.notes.push("goal.md: text before the first `## ` section is not sent; put it in a section".into());
    }
    for (head, known, text) in found {
        match known {
            None => g.notes.push(format!("goal.md: `## {head}` is not a goal section ({}): not sent", names())),
            Some(i) if g.sections[i].is_some() => g.notes.push(format!("goal.md: `## {}` twice: the second is not sent", SECTIONS[i].0)),
            Some(i) => g.sections[i] = Some(text.trim().to_string()),
        }
    }
    for (i, (name, means)) in SECTIONS.iter().enumerate() {
        if g.sections[i].as_deref().is_none_or(|t| t.is_empty()) {
            g.notes.push(format!("goal.md: no `## {name}` ({means})"));
        }
    }
    g
}

impl Goal {
    /// The instructions slot's body: every section with text, in order,
    /// each under `### Name — what it means`. Empty: nothing to send.
    pub fn render(&self) -> String {
        let mut out = Vec::new();
        for (i, (name, means)) in SECTIONS.iter().enumerate() {
            if let Some(text) = self.sections[i].as_deref().filter(|t| !t.is_empty()) {
                out.push(format!("### {name} — {means}\n{text}"));
            }
        }
        out.join("\n\n")
    }

    /// The Goal section's first line, for a card or a list.
    pub fn summary(&self) -> String {
        self.sections[0].as_deref().and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty())).unwrap_or("").to_string()
    }
}

/// The first line of `goal.md`'s Goal section (a card's one line).
pub fn summary(md: &str) -> String {
    parse(md).summary()
}

/// A `goal.md` with these sections (an empty one left out), the way a
/// parent writes its child's.
pub fn write(goal: &str, scope: &str, done: &str) -> String {
    let mut out = String::new();
    for ((name, _), text) in SECTIONS.iter().zip([goal, scope, done]) {
        if !text.trim().is_empty() {
            out.push_str(&format!("## {name}\n{}\n\n", text.trim()));
        }
    }
    out.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_are_read_by_name_and_sent_in_their_order() {
        let g = parse("# Tester\n\n## Done\nAll green.\n\n## goal\nKeep tests green.\nFor the owner.\n\n## Scope\nNever commit.\n");
        assert_eq!(g.sections[0].as_deref(), Some("Keep tests green.\nFor the owner."), "a heading matches in any case");
        assert!(g.notes.is_empty(), "{:?}", g.notes);
        let r = g.render();
        assert!(r.starts_with("### Goal — what you are for, and who you serve\nKeep tests green."), "{r}");
        let (goal, scope, done) = (r.find("### Goal").unwrap(), r.find("### Scope").unwrap(), r.find("### Done").unwrap());
        assert!(goal < scope && scope < done, "the fixed order, not the file's: {r}");
        assert_eq!(g.summary(), "Keep tests green.");
    }

    #[test]
    fn a_missing_unknown_twice_or_loose_part_is_reported_and_not_sent() {
        let g = parse("Loose words.\n## Goal\nA.\n## Notes\nsecret plan\n## Goal\nB.\n");
        assert_eq!(g.sections[0].as_deref(), Some("A."));
        let notes = g.notes.join("\n");
        assert!(notes.contains("text before the first `## ` section is not sent"), "{notes}");
        assert!(notes.contains("`## Notes` is not a goal section (Goal, Scope, Done): not sent"), "{notes}");
        assert!(notes.contains("`## Goal` twice"), "{notes}");
        assert!(notes.contains("no `## Scope` (what you do alone") && notes.contains("no `## Done`"), "{notes}");
        assert!(!g.render().contains("secret plan") && !g.render().contains("Loose") && !g.render().contains("B."));
    }

    #[test]
    fn an_old_instruction_without_sections_is_all_goal_and_said_so() {
        let g = parse("Be brief.\nTwo lines.");
        assert_eq!(g.sections[0].as_deref(), Some("Be brief.\nTwo lines."));
        assert_eq!(g.notes, ["goal.md has no sections (Goal, Scope, Done): all of it is read as the Goal"]);
        assert!(parse("  ").render().is_empty() && parse("").notes.is_empty(), "no goal: nothing, no notes");
    }

    #[test]
    fn a_written_goal_reads_back() {
        let md = write("Find the bug.", "", "Reply with the line.");
        assert_eq!(md, "## Goal\nFind the bug.\n\n## Done\nReply with the line.\n");
        let g = parse(&md);
        assert_eq!(
            (g.sections[0].as_deref(), g.sections[1].as_deref(), g.sections[2].as_deref()),
            (Some("Find the bug."), None, Some("Reply with the line."))
        );
    }
}
