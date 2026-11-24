//! Skill tools: the tools a skill brings of its own.
//!
//! A skill's `tools:` line can only name tools that exist already. A
//! skill may also carry tools nobody else has, in `tools.md` beside its
//! `SKILL.md`, written as a declared capability's tools are
//! (`crate::capability::declared`, docs/capabilities.md):
//!
//! ```markdown
//! spec/skills/python/tools.md
//! # python
//! Check, run and test Python files in the project.
//!
//! ## tool: check
//! Compile one file without running it. Prints nothing when it is fine.
//! command: ["python3", "-m", "py_compile", "{file}"]
//! inputs:
//! - file (string, required): the file, relative to the root
//! ```
//!
//! The heading is the skill's name. While the skill is loaded (and the
//! phase lets it be seen) each tool is the engine's, called
//! `<skill>.<tool>`: `python.check(file="tree.py")`. It is in the tools
//! slot with its inputs, and it goes away with the skill at the next
//! message.
//!
//! **A skill tool is a command in the engine's terminal.** The call is
//! checked against the tool's inputs, becomes its command with each
//! input as one whole quoted argument (never spliced into a shell line:
//! `declared`'s rules), and runs as a `run` of the engine's terminal
//! artifact: in the project's root, with a session, its exit code and
//! last lines as the result. So it works wherever a terminal does (a
//! host, a companion's folder, the browser's VM), and an engine with no
//! terminal artifact is told so. A read-only terminal's guard applies to
//! it as to any `run`.
//!
//! Not used from the declared format: `probe:`, `snapshot:` and events
//! (a skill is loaded by choice, it is not probed), and `timeout:` (a
//! terminal `run` has none). A file that names them still reads.
//!
//! Pure: the file's text in, the tool names, their lines and the `run`
//! call out.

use crate::calls::Call;
use crate::capability::declared::{self, Declared};
use crate::state::Engine;

/// The file beside `SKILL.md` that holds a skill's own tools.
pub const FILE: &str = "tools.md";

/// Where skill `name`'s tools are kept in `spec/`.
pub fn file(name: &str) -> String {
    format!("skills/{name}/{FILE}")
}

/// Read skill `skill`'s `tools.md`. Its heading must be the skill's name,
/// so a call's prefix always says which skill brought the tool.
pub fn parse(skill: &str, text: &str) -> Result<Declared, String> {
    let d = declared::parse(&file(skill), text)?;
    if d.id != skill {
        return Err(format!("{}: its heading is `# {}`, and must be the skill's name, `# {skill}`", file(skill), d.id));
    }
    Ok(d)
}

/// The tool files of the skills engine `e` has loaded and may see now,
/// read; one that does not read is left out (the skills artifact says
/// why, `problems`).
pub fn held(e: &Engine) -> Vec<Declared> {
    let visible = e.visible_skills();
    e.loaded_skills
        .iter()
        .filter(|l| visible.iter().any(|(n, _)| n == l))
        .filter_map(|l| parse(l, e.skill_tools.get(l)?).ok())
        .collect()
}

/// The names engine `e` may call now: `<skill>.<tool>`.
pub fn names(e: &Engine) -> Vec<String> {
    held(e).iter().flat_map(|d| d.tools.iter().map(|t| format!("{}.{}", d.id, t.tool.name))).collect()
}

/// The tools slot's lines for them, like the built-in tools'.
pub fn describe(e: &Engine) -> String {
    held(e).iter().flat_map(|d| d.tools.iter().map(|t| t.tool.describe(&format!("{}.{}", d.id, t.tool.name)))).collect::<Vec<_>>().join("\n")
}

/// What loading skill `skill` adds to its instructions: its own tools,
/// one line each. Empty: it brings none (or its file does not read: why).
pub fn loaded(e: &Engine, skill: &str) -> String {
    let Some(text) = e.skill_tools.get(skill) else { return String::new() };
    match parse(skill, text) {
        Ok(d) if d.tools.is_empty() => String::new(),
        Ok(d) => {
            let lines: Vec<String> = d.tools.iter().map(|t| t.tool.describe(&format!("{}.{}", d.id, t.tool.name))).collect();
            format!("\n\nIt brings these tools, yours until the next message (each runs in your terminal):\n{}", lines.join("\n"))
        }
        Err(why) => format!("\n\n(Its tools.md does not read, so it brings no tools: {why})"),
    }
}

/// Every skill of `e` whose `tools.md` does not read, and why: for the
/// skills artifact, so a broken file is seen instead of missing silently.
pub fn problems(e: &Engine) -> Vec<String> {
    e.skill_tools.iter().filter_map(|(skill, text)| parse(skill, text).err()).collect()
}

/// A call to a skill tool as the terminal `run` it is: its command, each
/// input one quoted argument. `None`: `call` is not a skill tool `e`
/// holds now. `artifact=` (which terminal, when the engine has several)
/// is passed on.
pub fn as_run(e: &Engine, call: &Call) -> Option<Result<Call, String>> {
    let (skill, tool) = call.name.split_once('.')?;
    let d = held(e).into_iter().find(|d| d.id == skill && d.tools.iter().any(|t| t.tool.name == tool))?;
    let inputs: Vec<(String, String)> = call.args.iter().filter(|(k, _)| k != "artifact").cloned().collect();
    Some(d.argv(tool, &inputs).map(|argv| {
        let line: Vec<String> = std::iter::once(&argv.program).chain(&argv.args).map(|a| crate::container::sh_quote(a)).collect();
        let mut args = vec![("command".to_string(), line.join(" "))];
        args.extend(call.args.iter().filter(|(k, _)| k == "artifact").cloned());
        Call { name: crate::terminal::TOOLS[0].to_string(), args }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;

    pub(crate) const PYTHON_SKILL: &str = "---\nname: python\ndescription: Python files.\n---\nCheck before you run.\n";
    pub(crate) const PYTHON_TOOLS: &str = "# python\nCheck and run Python files.\n\n## tool: check\nCompile one file without running it.\ncommand: [\"python3\", \"-m\", \"py_compile\", \"{file}\"]\ninputs:\n- file (string, required): the file, relative to the root\n\n## tool: run\nRun one file.\ncommand: [\"python3\", \"{file}\", \"{arg}\"]\ninputs:\n- file (string, required): the file\n- arg (string): one argument\n";

    pub(crate) fn engine() -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.skills.insert("python".into(), PYTHON_SKILL.into());
        e.skill_tools.insert("python".into(), PYTHON_TOOLS.into());
        e.artifacts.insert("term".into(), "kind: terminal\nroot: .".into());
        e
    }

    #[test]
    fn a_skills_own_tools_are_the_engines_only_while_the_skill_is_loaded() {
        let mut e = engine();
        assert!(names(&e).is_empty() && describe(&e).is_empty(), "not loaded: not there");
        assert!(!crate::live::toolbox(&e).iter().any(|t| t.starts_with("python.")));
        e.loaded_skills.push("python".into());
        assert_eq!(names(&e), ["python.check", "python.run"]);
        assert!(crate::live::toolbox(&e).contains(&"python.check".to_string()));
        let lines = describe(&e);
        assert!(lines.starts_with("- python.check(file=\"...\"): Compile one file without running it. (file: string, required, the file, relative to the root)"), "{lines}");
        assert!(loaded(&e, "python").contains("It brings these tools") && loaded(&e, "python").contains("- python.run(file=\"...\", arg=\"...\")"));
        // The next message unloads the skill, and its tools go with it.
        e.loaded_skills.clear();
        assert!(names(&e).is_empty());
    }

    #[test]
    fn a_skill_tool_call_is_a_terminal_run_with_each_input_one_quoted_argument() {
        let mut e = engine();
        e.loaded_skills.push("python".into());
        let call = crate::calls::parse("python.run(file=\"my tree.py\", arg=\"5; rm -rf x\")").unwrap().remove(0);
        let run = as_run(&e, &call).unwrap().unwrap();
        assert_eq!(run.name, "run");
        assert_eq!(run.arg("command"), Some("'python3' 'my tree.py' '5; rm -rf x'"), "an input is one argument, never shell text");
        // An optional input left out leaves its argument out; a missing required one is refused.
        let run = as_run(&e, &crate::calls::parse("python.run(file=\"a.py\")").unwrap().remove(0)).unwrap().unwrap();
        assert_eq!(run.arg("command"), Some("'python3' 'a.py'"));
        assert!(as_run(&e, &crate::calls::parse("python.check()").unwrap().remove(0)).unwrap().is_err());
        // Not a skill tool of this engine: nothing to say.
        assert!(as_run(&e, &crate::calls::parse("git.status()").unwrap().remove(0)).is_none());
        assert!(as_run(&e, &crate::calls::parse("python.nope()").unwrap().remove(0)).is_none());
    }

    #[test]
    fn a_tools_file_must_carry_the_skills_name_and_a_broken_one_is_said() {
        let mut e = engine();
        e.loaded_skills.push("python".into());
        e.skill_tools.insert("python".into(), PYTHON_TOOLS.replacen("# python", "# other", 1));
        assert!(names(&e).is_empty());
        assert!(problems(&e)[0].contains("must be the skill's name, `# python`"), "{:?}", problems(&e));
        assert!(loaded(&e, "python").contains("does not read, so it brings no tools"));
    }
}
