//! Skills: instructions with a name, a description and tools, that an
//! engine loads when it needs them.
//!
//! A skill is the standard `SKILL.md` file, so skills written for other
//! agents work as they are:
//!
//! ```markdown
//! ---
//! name: review
//! description: Review a change for bugs before it is merged.
//! tools: read, list
//! ---
//! Read the change, then ...
//! ```
//!
//! An engine keeps its skills in `spec/skills/<name>/SKILL.md` (shareable).
//! It is an element of the prompt in two places:
//!
//! - **artifacts**: every skill it may use now, by name and description,
//!   so it knows what it can load;
//! - **tools**: `skill(name)` loads one (its instructions come back as the
//!   result, and its `tools:` are the engine's until your next message),
//!   and `skill_save(...)` creates or updates one, for an engine whose
//!   tools list it.
//!
//! A phase may narrow which skills it sees (its `skills` column in the phase
//! table); without that, every skill is there.
//!
//! Pure: parse, list, write back.

/// The tool that loads a skill.
pub const LOAD: &str = "skill";
/// The tool that creates or updates a skill.
pub const SAVE: &str = "skill_save";

/// A read skill.
#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Tools it brings while loaded.
    pub tools: Vec<String>,
    /// Its instructions.
    pub body: String,
}

/// Read a `SKILL.md`. The header is the `---` block; `name` and
/// `description` are required. `tools` (or `allowed-tools`, as some skills
/// write it) is a comma or space separated list. A `>` or `|` value goes on
/// over the indented lines below it.
pub fn parse(text: &str) -> Result<Skill, String> {
    let rest = text.trim_start().strip_prefix("---").ok_or("SKILL.md starts with a `---` header")?;
    let (head, body) = rest.split_once("\n---").ok_or("SKILL.md: the `---` header is not closed")?;
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // A folded or block value's next line.
            if let Some((_, v)) = fields.last_mut() {
                if !v.is_empty() {
                    v.push(' ');
                }
                v.push_str(line.trim());
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        let v = if v == ">" || v == "|" { "" } else { v.trim_matches('"').trim_matches('\'') };
        fields.push((k.trim().to_string(), v.to_string()));
    }
    let get = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let name = get("name").filter(|n| !n.is_empty()).ok_or("SKILL.md needs `name:`")?;
    let description = get("description").filter(|d| !d.is_empty()).ok_or_else(|| format!("skill `{name}` needs `description:`"))?;
    let tools = get("tools")
        .or_else(|| get("allowed-tools"))
        .map(|l| l.split([',', ' ']).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    let body = body.strip_prefix('\n').unwrap_or(body).trim().to_string();
    Ok(Skill { name, description, tools, body })
}

impl Skill {
    /// Back to `SKILL.md`.
    pub fn to_md(&self) -> String {
        let mut md = format!("---\nname: {}\ndescription: {}\n", self.name, self.description);
        if !self.tools.is_empty() {
            md.push_str(&format!("tools: {}\n", self.tools.join(", ")));
        }
        format!("{md}---\n{}\n", self.body)
    }

    /// What `skill(name)` gives back: its instructions, and its tools.
    pub fn loaded(&self) -> String {
        let tools = if self.tools.is_empty() { String::new() } else { format!("\n\n(Its tools are yours until the next message: {}.)", self.tools.join(", ")) };
        format!("Skill `{}` loaded. Follow it:\n\n{}{tools}", self.name, self.body)
    }
}

/// A skill name: lower case letters, digits, `-` and `_`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The skills an engine may use now: all of them, or only those `only`
/// names (a phase's `skills:`). Name → text, as kept in the engine.
pub fn visible<'a>(skills: &'a std::collections::BTreeMap<String, String>, only: Option<&[String]>) -> Vec<(&'a str, &'a str)> {
    skills.iter().filter(|(n, _)| only.is_none_or(|o| o.iter().any(|x| x == *n))).map(|(n, t)| (n.as_str(), t.as_str())).collect()
}

/// The skills snapshot: one line each, name and description; a broken
/// file says why instead of hiding.
pub fn snapshot(visible: &[(&str, &str)]) -> String {
    let mut lines = vec![format!("Load one with {LOAD}(name=\"...\") when the work needs it.")];
    for (name, text) in visible {
        lines.push(match parse(text) {
            Ok(s) => format!("- {name}: {}", s.description),
            Err(why) => format!("- {name}: (does not read: {why})"),
        });
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const REVIEW: &str = "---\nname: review\ndescription: Review a change for bugs.\ntools: read, list\n---\nRead the change.\nThen list risks.\n";

    #[test]
    fn a_skill_reads_its_header_and_instructions_and_writes_back() {
        let s = parse(REVIEW).unwrap();
        assert_eq!((s.name.as_str(), s.description.as_str(), s.tools.clone()), ("review", "Review a change for bugs.", vec!["read".to_string(), "list".to_string()]));
        assert_eq!(s.body, "Read the change.\nThen list risks.");
        assert_eq!(parse(&s.to_md()).unwrap(), s);
        assert!(s.loaded().contains("Follow it:\n\nRead the change.") && s.loaded().contains("until the next message: read, list"));
    }

    #[test]
    fn existing_skills_read_as_they_are() {
        // The shape other agents' skills use: folded description, allowed-tools.
        let md = "---\nname: pdf\ndescription: >\n  Work with PDF files:\n  read, fill, merge.\nallowed-tools: Read Bash\nlicense: MIT\n---\n\n# PDF\nSteps...";
        let s = parse(md).unwrap();
        assert_eq!(s.description, "Work with PDF files: read, fill, merge.");
        assert_eq!(s.tools, ["Read", "Bash"]);
        assert!(s.body.starts_with("# PDF"));
    }

    #[test]
    fn mistakes_say_what_is_missing() {
        assert!(parse("no header").unwrap_err().contains("starts with"));
        assert!(parse("---\nname: x\n").unwrap_err().contains("not closed"));
        assert!(parse("---\ndescription: d\n---\n").unwrap_err().contains("needs `name:`"));
        assert!(parse("---\nname: x\n---\n").unwrap_err().contains("needs `description:`"));
        assert!(valid_name("code-review_2") && !valid_name("Code Review") && !valid_name(""));
    }

    #[test]
    fn a_phase_can_narrow_the_skills_and_the_list_shows_broken_ones() {
        let mut all = BTreeMap::new();
        all.insert("review".to_string(), REVIEW.to_string());
        all.insert("broken".to_string(), "oops".to_string());
        assert_eq!(visible(&all, None).len(), 2);
        let only = ["review".to_string()];
        assert_eq!(visible(&all, Some(&only)).iter().map(|(n, _)| *n).collect::<Vec<_>>(), ["review"]);
        let snap = snapshot(&visible(&all, None));
        assert!(snap.contains("- review: Review a change for bugs.") && snap.contains("- broken: (does not read"));
    }
}
