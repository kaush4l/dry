//! Phases: the stages of an engine's loop, from its template
//! (`crate::template`): the phase table at the top holds each phase's
//! metadata, and a `## phase: <name>` section per row holds its goal.
//!
//! ```markdown
//! | phase | tools      | response |
//! |-------|------------|----------|
//! | plan  | read, list | standard |
//! | build |            |          |
//!
//! ## phase: plan
//! Read what is there, then write the plan as the `plan` artifact.
//!
//! ## phase: build
//! Do the plan, one step at a time.
//! ```
//!
//! Each phase has a goal (its text) and, optionally, tools that exist only
//! while it runs. There is no step limit: a phase runs until the engine
//! moves on or replies (stuck loops are handled elsewhere, not by a
//! count). No phase sections mean one implicit phase: right for most
//! engines.
//!
//! The run's position (which phase, how many steps taken in it) is not
//! spec: it is what is happening, kept in `convo/run.md`. A new message
//! from the user starts the run again at the first phase.
//! `goto_phase(name)` moves to any other phase (on to test, or back to
//! code when a test fails) and exists while there is more than one; the
//! work ends the way every turn ends: with `reply`.
//!
//! In the prompt the phase slot reads as a plan: every phase on one line
//! (the direction), the current one expanded with its goal (what to do
//! now). What was already done is in the history, which comes after.
//!
//! Phases are the loop around the loop. Inside a phase the engine runs
//! its usual step loop (observe, call, read the result: a react loop);
//! the phases are the outer loop of a workflow (triage → code → test), each
//! bringing its own prompt section and its own tools, and handing over to
//! another with `goto_phase`.
//!
//! A phase also names how it is answered (`response:`). `standard` is the
//! engine's response contract read from a model's text. `jev` is a newer,
//! much faster response standard planned for phases whose work is well
//! known with few paths; it needs a newer API and models that support it,
//! so today it is recognised and answered the standard way. Whether a
//! step could be answered by one is `crate::jev::check` (from metadata).
//!
//! Why phases: they make the loop predictable. The model always sees which
//! stage it is in, what that stage is for, and how many steps it has left;
//! going over is said out loud instead of running on.
//!
//! Pure: parse, render, move. The loop (`crate::turn`) calls these.

/// The tool that moves the run to another phase.
pub const GOTO: &str = "goto_phase";

/// How a phase is answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResponseMode {
    /// The engine's response contract, from the model's text.
    #[default]
    Standard,
    /// The planned faster response standard (not available yet).
    Jev,
}

impl ResponseMode {
    pub fn parse(word: &str) -> Option<ResponseMode> {
        match word.trim() {
            "standard" => Some(ResponseMode::Standard),
            "jev" => Some(ResponseMode::Jev),
            _ => None,
        }
    }

    /// Can this runtime answer this way? Only `standard`, today.
    pub fn available(self) -> bool {
        self == ResponseMode::Standard
    }
}

/// One stage of the loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Phase {
    pub name: String,
    /// Tools that exist only in this phase.
    pub tools: Vec<String>,
    /// How it is answered.
    pub response: ResponseMode,
    /// The skills it sees; none: all of the engine's.
    pub skills: Option<Vec<String>>,
    /// What it is for: its text.
    pub goal: String,
}

/// Where the run is: the phase (index) and the steps taken in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub phase: usize,
    pub step: u32,
}

impl Run {
    /// `convo/run.md`: the phase by name (so a reordered spec does not
    /// silently point elsewhere) and the step.
    pub fn to_md(self, phases: &[Phase]) -> String {
        match phases.get(self.phase) {
            Some(p) => format!("phase: {}\nstep: {}\n", p.name, self.step),
            None => String::new(),
        }
    }

    /// Read `run.md` against the phases it names. A phase that no longer
    /// exists starts over at the first.
    pub fn from_md(md: &str, phases: &[Phase]) -> Run {
        let field = |key: &str| md.lines().find_map(|l| l.strip_prefix(key)).map(str::trim);
        match field("phase:").and_then(|name| phases.iter().position(|p| p.name == name)) {
            Some(phase) => Run { phase, step: field("step:").and_then(|s| s.parse().ok()).unwrap_or(0) },
            None => Run::default(),
        }
    }

    /// One more step taken in the current phase.
    pub fn count(&mut self) {
        self.step += 1;
    }

    /// Can the run move (so `goto_phase` exists)? With two phases or more.
    pub fn can_move(phases: &[Phase]) -> bool {
        phases.len() > 1
    }

    /// `goto_phase(name)`: move to phase `name`, its steps from zero. What
    /// it says is the call's result, for the model to read.
    pub fn goto(&mut self, phases: &[Phase], name: &str) -> Result<String, String> {
        let names = || phases.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ");
        let to = phases.iter().position(|p| p.name == name).ok_or_else(|| format!("no phase `{name}`; the phases are {}", names()))?;
        if to == self.phase {
            return Err(format!("already in `{name}`"));
        }
        let from = &phases[self.phase].name;
        *self = Run { phase: to, step: 0 };
        let p = &phases[to];
        Ok(format!("left {from}; now {}", p.name))
    }

    /// How the current phase is answered: its mode, and why it is
    /// answered the standard way when that mode is not available.
    pub fn response(self, phases: &[Phase]) -> (ResponseMode, Option<String>) {
        let mode = phases.get(self.phase).map_or(ResponseMode::Standard, |p| p.response);
        let note = (!mode.available()).then(|| format!("phase asks for {mode:?} responses, not available yet: answered the standard way"));
        (mode, note)
    }

    /// The tools the current phase adds: its own, and `goto_phase` while
    /// there is another phase to go to.
    pub fn tools(self, phases: &[Phase]) -> Vec<String> {
        let mut tools = phases.get(self.phase).map(|p| p.tools.clone()).unwrap_or_default();
        if Run::can_move(phases) {
            tools.push(GOTO.to_string());
        }
        tools
    }

    /// The phase slot of the prompt: the plan (every phase on one line,
    /// the direction) with the current phase expanded (what to do now).
    /// Empty without phases.
    pub fn section(self, phases: &[Phase]) -> String {
        let Some(now) = phases.get(self.phase) else { return String::new() };
        let mut out = vec!["The plan:".to_string()];
        for (i, p) in phases.iter().enumerate() {
            let first = p.goal.lines().next().unwrap_or("");
            out.push(if i == self.phase {
                format!("{}. **{}** ← now, step {}", i + 1, p.name, self.step + 1)
            } else {
                format!("{}. {} — {first}", i + 1, p.name)
            });
        }
        out.push(String::new());
        out.push(format!("Now, {}:", now.name));
        out.push(now.goal.clone());
        out.push(String::new());
        out.push(if Run::can_move(phases) {
            format!("When it is done (or the work needs another phase), call {GOTO}(name=\"...\"). What was done so far is in the history. Reply when the whole work is done.")
        } else {
            "Reply when the work is done.".to_string()
        });
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "| phase | tools |\n|---|---|\n| plan | read, list |\n| build | |\n\n## phase: plan\nRead, then plan.\n\n## phase: build\nDo it.\n";

    fn parse(md: &str) -> Result<Vec<Phase>, String> {
        crate::template::parse(md)?.phases()
    }

    #[test]
    fn the_run_goes_to_any_other_phase_forward_or_back() {
        let p = parse(MD).unwrap();
        let mut run = Run::default();
        assert_eq!(run.tools(&p), ["read", "list", GOTO]);
        assert_eq!(run.goto(&p, "build").unwrap(), "left plan; now build");
        assert_eq!(run, Run { phase: 1, step: 0 });
        assert_eq!(run.tools(&p), [GOTO], "plan's own tools are gone");
        assert!(run.goto(&p, "build").unwrap_err().contains("already in"));
        assert!(run.goto(&p, "ship").unwrap_err().contains("the phases are plan, build"));
        assert_eq!(run.goto(&p, "plan").unwrap(), "left build; now plan", "back is fine");
        assert!(Run::default().tools(&parse("| phase |\n|-|\n| only |\n## phase: only\nx").unwrap()).is_empty(), "one phase: nowhere to go");
    }

    #[test]
    fn the_prompt_shows_the_plan_with_the_current_phase_expanded() {
        let p = parse(MD).unwrap();
        let text = Run { phase: 1, step: 1 }.section(&p);
        assert!(text.starts_with("The plan:\n1. plan — Read, then plan.\n2. **build** ← now, step 2\n\nNow, build:\nDo it."), "{text}");
        assert!(text.contains("goto_phase(name=\"...\")") && text.contains("in the history"));
    }

    #[test]
    fn a_phase_names_how_it_is_answered_and_jev_waits_for_support() {
        let p = parse("| phase | response | tools |\n|-|-|-|\n| triage | jev | read |\n| code | | |\n\n## phase: triage\nSort it.\n\n## phase: code\nWrite it.").unwrap();
        assert_eq!((p[0].response, p[0].tools.clone(), p[0].goal.as_str()), (ResponseMode::Jev, vec!["read".to_string()], "Sort it."));
        assert_eq!(p[1].response, ResponseMode::Standard, "standard unless said");
        let (mode, note) = Run::default().response(&p);
        assert_eq!(mode, ResponseMode::Jev);
        assert!(note.unwrap().contains("not available yet"));
        assert_eq!(Run { phase: 1, step: 0 }.response(&p).1, None);
    }

    #[test]
    fn the_position_is_kept_by_phase_name() {
        let p = parse(MD).unwrap();
        let run = Run { phase: 1, step: 2 };
        assert_eq!(run.to_md(&p), "phase: build\nstep: 2\n");
        assert_eq!(Run::from_md(&run.to_md(&p), &p), run);
        assert_eq!(Run::from_md("phase: gone\nstep: 4\n", &p), Run::default(), "a removed phase starts over");
        assert_eq!(Run::default().to_md(&[]), "");
    }
}
