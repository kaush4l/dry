//! Templates an engine starts children from: a sub-engine made ready, so
//! a parent with no action tools of its own (the translator, docs/default.md)
//! can make a developer by name instead of writing its template out.
//!
//! ```text
//! spec/templates/<name>/
//!   about.md                its first line: what a child made from it does
//!   template.md             the child's template.md: its phases (`crate::template`)
//!   tools.md                optional: its direct tools, one `- name` line each
//!                           (without it the child gets its parent's)
//!   skills/<s>/SKILL.md     the child's skills (`crate::skill`)
//!   skills/<s>/tools.md     optional: the tools that skill brings (`crate::skilltool`)
//!   artifacts/<a>.md        the artifacts it starts with: `kind: file`,
//!                           `uses: [browser]`, a plain `notes` text, ...
//!   elements.md             optional: the extra elements the child holds
//!                           (`crate::element::held`), e.g. `environment`,
//!                           so it reads where its tools run and what is
//!                           installed there (without it: none)
//! ```
//!
//! `engine_start(name, goal, template="developer", root="/path/to/repo")`
//! makes the child from it (`crate::engines`): its template, tools, skills
//! and artifacts; a `root:` line in an artifact is set to `root`, so its
//! file, terminal and evidence artifacts point at the project. An
//! artifact whose capabilities are absent on the machine drops out of the
//! child's prompt on its own (docs/capabilities.md), so a browser artifact
//! costs nothing where no browser is.
//!
//! The parent's engines artifact lists its templates (`list`), so it knows
//! what it can make. Pure: texts in, texts out.

use std::collections::BTreeMap;

use crate::state::Engine;

/// Where templates live in `spec/` (and the prefix of their spec files).
pub const FOLDER: &str = "templates/";

/// One template, read from its files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kit {
    pub name: String,
    /// `about.md`'s first line.
    pub about: String,
    pub template: String,
    /// `tools.md`'s names; `None`: the parent's.
    pub tools: Option<Vec<String>>,
    pub skills: BTreeMap<String, String>,
    /// `skills/<s>/tools.md`: the tools a skill brings (`crate::skilltool`).
    pub skill_tools: BTreeMap<String, String>,
    pub artifacts: BTreeMap<String, String>,
    /// `elements.md` as written; empty: the child holds no extra element.
    pub elements: String,
}

impl Kit {
    /// Its phases' names, in order; empty when it has none or does not read.
    pub fn phases(&self) -> Vec<String> {
        crate::template::parse(&self.template).and_then(|t| t.phases()).map(|p| p.into_iter().map(|p| p.name).collect()).unwrap_or_default()
    }

    /// Why it cannot make a child, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if self.template.trim().is_empty() {
            return Some(format!("template {} has no template.md", self.name));
        }
        if let Err(why) = crate::template::parse(&self.template).and_then(|t| t.phases().map(|_| ())) {
            return Some(format!("template {}: {why}", self.name));
        }
        let skill = self.skills.iter().find_map(|(n, t)| crate::skill::parse(t).err().map(|why| format!("template {}: skill {n}: {why}", self.name)));
        skill.or_else(|| self.skill_tools.iter().find_map(|(n, t)| crate::skilltool::parse(n, t).err().map(|why| format!("template {}: {why}", self.name))))
    }

    /// Its artifacts with every `root:` line set to `root` (none: as written).
    pub fn artifacts_at(&self, root: Option<&str>) -> BTreeMap<String, String> {
        let Some(root) = root else { return self.artifacts.clone() };
        self.artifacts
            .iter()
            .map(|(n, t)| {
                let text: Vec<String> =
                    t.lines().map(|l| if l.trim_start().starts_with("root:") { format!("root: {root}") } else { l.to_string() }).collect();
                (n.clone(), format!("{}\n", text.join("\n")))
            })
            .collect()
    }
}

/// The templates engine `e` holds, by name.
pub fn kits(e: &Engine) -> BTreeMap<String, Kit> {
    let mut out: BTreeMap<String, Kit> = BTreeMap::new();
    for (path, text) in &e.templates {
        let Some((name, rest)) = path.split_once('/') else { continue };
        let kit = out.entry(name.to_string()).or_insert_with(|| Kit { name: name.to_string(), ..Kit::default() });
        match rest {
            "about.md" => kit.about = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("").to_string(),
            "template.md" => kit.template = text.clone(),
            "elements.md" => kit.elements = text.clone(),
            "tools.md" => kit.tools = Some(text.lines().filter_map(|l| l.trim().strip_prefix("- ")).map(|t| t.trim().to_string()).collect()),
            _ => {
                if let Some(skill) = rest.strip_prefix("skills/").and_then(|r| r.strip_suffix("/SKILL.md")) {
                    kit.skills.insert(skill.to_string(), text.clone());
                } else if let Some(skill) = rest.strip_prefix("skills/").and_then(|r| r.strip_suffix(&format!("/{}", crate::skilltool::FILE))) {
                    kit.skill_tools.insert(skill.to_string(), text.clone());
                } else if let Some(a) = rest.strip_prefix("artifacts/").and_then(|r| r.strip_suffix(".md")).filter(|a| !a.contains('/')) {
                    kit.artifacts.insert(a.to_string(), text.clone());
                }
            }
        }
    }
    out
}

/// What the parent's engines artifact says about its templates.
pub fn list(e: &Engine) -> String {
    let kits = kits(e);
    if kits.is_empty() {
        return String::new();
    }
    let mut out = String::from("Templates you can make a child from (engine_start(name, goal, template=\"<name>\", root=\"<project folder>\")):");
    for k in kits.values() {
        if let Some(why) = k.problem() {
            out.push_str(&format!("\n- {}: broken: {why}", k.name));
            continue;
        }
        let artifacts: Vec<String> = k
            .artifacts
            .iter()
            .map(|(n, t)| match t.lines().next().map(str::trim) {
                Some(first) if first.starts_with("kind:") || first.starts_with("uses:") => format!("{n} ({first})"),
                _ => n.clone(),
            })
            .collect();
        out.push_str(&format!(
            "\n- {}: {}\n  phases: {} · skills: {} · artifacts: {}",
            k.name,
            if k.about.is_empty() { "(no about.md)" } else { &k.about },
            k.phases().join(" → "),
            k.skills.keys().cloned().collect::<Vec<_>>().join(", "),
            artifacts.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;

    fn parent() -> Engine {
        let mut e = Engine::new("p", "", OpenAi::default());
        let t = |p: &str, s: &str| (p.to_string(), s.to_string());
        e.templates = BTreeMap::from([
            t("dev/about.md", "# dev\n\nChanges code: look, change, test.\n"),
            t("dev/template.md", "| phase |\n|---|\n| look |\n| test |\n\n## phase: look\nLook.\n\n## phase: test\nTest.\n"),
            t("dev/tools.md", "- reply\n- write\n"),
            t("dev/skills/look/SKILL.md", "---\nname: look\ndescription: How to look.\n---\nRead first.\n"),
            t("dev/artifacts/code.md", "kind: file\nroot: .\n"),
            t("dev/artifacts/notes.md", "## Notes\n"),
        ]);
        e
    }

    #[test]
    fn a_template_reads_from_its_files_and_is_listed() {
        let e = parent();
        let k = &kits(&e)["dev"];
        assert_eq!(k.about, "Changes code: look, change, test.");
        assert_eq!(k.phases(), ["look", "test"]);
        assert_eq!(k.tools.as_deref(), Some(&["reply".to_string(), "write".to_string()][..]));
        assert_eq!(k.problem(), None);
        let listed = list(&e);
        assert!(listed.contains("- dev: Changes code") && listed.contains("phases: look → test") && listed.contains("code (kind: file), notes"), "{listed}");
    }

    #[test]
    fn root_is_set_on_every_root_line_and_a_broken_template_says_why() {
        let e = parent();
        let k = &kits(&e)["dev"];
        assert_eq!(k.artifacts_at(Some("/tmp/x"))["code"], "kind: file\nroot: /tmp/x\n");
        assert_eq!(k.artifacts_at(None)["code"], "kind: file\nroot: .\n");
        let mut bad = e.clone();
        bad.templates.insert("dev/template.md".into(), "## phase\nx".into());
        assert!(list(&bad).contains("dev: broken"), "{}", list(&bad));
    }
}
