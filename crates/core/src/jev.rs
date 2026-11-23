//! The jev check: is this step ready for a JEV endpoint?
//!
//! JEV is its own response standard (`docs/jev.md`), not a way of reading
//! a chat model's text. Jev (TypeSafe's "System One" decision model) takes
//! a `state` and typed `questions` (`choice`, `noul`, `score`) and answers
//! each with a typed value and its probabilities, never text. The long
//! text response contract exists because a chat model reads text; a JEV
//! endpoint needs a different value, so the check is different too.
//!
//! A step is jev-possible from metadata alone, nothing is guessed from
//! descriptions:
//!
//! - the response side: the phase says `response: jev`, which gives the
//!   step its JEV form (`Form`): one `choice` question, "which call now?",
//!   its instructions the phase's goal, one option per offered call;
//! - the tool side: every tool the step offers carries jev metadata
//!   (`ToolJev`): when it is the answer (the option's criterion), and its
//!   inputs with their fixed values (a JEV endpoint writes no arguments).
//!
//! Where tool metadata comes from: the built-in `goto_phase` (its `name`
//! is the other phases), and an MCP tool whose input schema has `x-jev`
//! (`{"when": "..."}`, its inputs from the schema's `enum`, `const` or
//! `boolean`). Every other tool has none yet, and the check says so.
//!
//! Nothing changes in the turn: the standard call still runs. Calling a
//! JEV endpoint is not built (`crate::models::Models::jev_endpoint` only
//! says whether one is configured); with one, a jev-possible phase is
//! answered by it, and an answer under `MIN_CONFIDENCE` falls back to the
//! standard call.
//!
//! Pure: from the engine as it is.

use serde_json::Value;

use crate::calls::Call;
use crate::phase::ResponseMode;
use crate::state::Engine;

/// Most options a JEV `choice` question takes (TypeSafe: 1 to 255).
pub const MAX_OPTIONS: usize = 255;

/// Below this confidence a JEV answer is not used: the step is answered
/// the standard way. A starting point, to be set from labelled runs.
pub const MIN_CONFIDENCE: f64 = 0.8;

/// The schema key an MCP tool's jev metadata is under.
pub const MCP_KEY: &str = "x-jev";

/// A tool's jev metadata: how it is offered to a JEV endpoint.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolJev {
    /// When a call of it is the right answer: its option's criterion.
    pub when: String,
    /// Its inputs, each with its fixed values (empty: no inputs).
    pub inputs: Vec<(String, Vec<String>)>,
}

/// One option of the step's question: a call with every input fixed.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// The option's key in the question (unique, short).
    pub key: String,
    pub call: Call,
    /// Its criterion.
    pub when: String,
}

/// The step's JEV form: the question a JEV endpoint is asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    /// What to decide: the phase's goal.
    pub instructions: String,
    /// The options: one `choice` question over them.
    pub choices: Vec<Choice>,
}

/// The check's result for one rendered step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Jev {
    /// The phase's response and every tool it offers carry jev metadata.
    pub possible: bool,
    /// The form, when possible.
    pub form: Option<Form>,
    /// Why not, each missing piece (`tool `run` has no jev metadata`).
    pub missing: Vec<String>,
    /// The current phase says `response: jev`.
    pub asked: bool,
}

impl Jev {
    /// One line for the prompt view.
    pub fn line(&self) -> String {
        match (&self.form, self.asked) {
            (Some(f), _) => {
                let keys: Vec<&str> = f.choices.iter().take(6).map(|c| c.key.as_str()).collect();
                let more = if f.choices.len() > 6 { ", …" } else { "" };
                let n = f.choices.len();
                format!(
                    "possible ({n} choice{}: {}{more}); jev would be used here (calling a JEV endpoint is not built yet: answered the standard way)",
                    if n == 1 { "" } else { "s" },
                    keys.join(", ")
                )
            }
            (None, asked) => {
                let first = self.missing.first().map_or("nothing offered", String::as_str);
                let more = match self.missing.len() {
                    0 | 1 => String::new(),
                    n => format!(" (and {} more)", n - 1),
                };
                let tail = if asked { "; this phase asks for jev, so it is answered the standard way" } else { "" };
                format!("not possible — {first}{more}{tail}")
            }
        }
    }
}

/// The built-in tools' jev metadata. Only `goto_phase` has it: where to go
/// is a known set (the other phases).
fn builtin(e: &Engine, name: &str, phases: &[crate::phase::Phase]) -> Option<ToolJev> {
    if name != crate::phase::GOTO {
        return None;
    }
    let now = phases.get(e.run.phase).map(|p| p.name.as_str());
    let to: Vec<String> = phases.iter().map(|p| p.name.clone()).filter(|p| Some(p.as_str()) != now).collect();
    Some(ToolJev { when: "this phase is done, or the work needs another phase".into(), inputs: vec![("name".into(), to)] })
}

/// An MCP tool's jev metadata, from `x-jev` in its input schema. Error:
/// it has `x-jev` but an input has no fixed values.
pub fn from_schema(schema: &Value) -> Option<Result<ToolJev, String>> {
    let meta = schema.get(MCP_KEY)?;
    let when = meta.get("when").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if when.is_empty() {
        return Some(Err(format!("{MCP_KEY} has no `when`")));
    }
    let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    let mut inputs = Vec::new();
    for (k, p) in schema.get("properties").and_then(Value::as_object).into_iter().flatten() {
        let values = if let Some(list) = p.get("enum").and_then(Value::as_array) {
            list.iter().map(text).collect()
        } else if let Some(one) = p.get("const") {
            vec![text(one)]
        } else if p.get("type").and_then(Value::as_str) == Some("boolean") {
            vec!["true".into(), "false".into()]
        } else {
            return Some(Err(format!("input `{k}` has no fixed values (enum, const or boolean)")));
        };
        inputs.push((k.clone(), values));
    }
    Some(Ok(ToolJev { when, inputs }))
}

/// Every call of one tool: the product of its inputs' values.
fn expand(name: &str, meta: &ToolJev) -> Vec<Choice> {
    let mut calls: Vec<Vec<(String, String)>> = vec![Vec::new()];
    for (input, values) in &meta.inputs {
        calls = calls
            .iter()
            .flat_map(|so_far| {
                values.iter().map(move |v| {
                    let mut c = so_far.clone();
                    c.push((input.clone(), v.clone()));
                    c
                })
            })
            .take(MAX_OPTIONS + 1)
            .collect();
    }
    calls
        .into_iter()
        .map(|args| {
            let values: Vec<&str> = args.iter().map(|(_, v)| v.as_str()).collect();
            let key = if values.is_empty() { name.to_string() } else { format!("{name} {}", values.join(" ")) };
            Choice { key, call: Call { name: name.to_string(), args }, when: meta.when.clone() }
        })
        .collect()
}

/// The check for engine `e`'s step as it renders now.
pub fn check(e: &Engine) -> Jev {
    let phases = e.phase_list().unwrap_or_default();
    let phase = phases.get(e.run.phase);
    let mut j = Jev { asked: e.run.response(&phases).0 == ResponseMode::Jev, ..Jev::default() };
    // The response side: only a phase that says `response: jev` has a JEV form.
    match phase {
        None => j.missing.push("no phases: the response has no jev form (a phase with `response: jev` gives it one)".into()),
        Some(p) if !j.asked => j.missing.push(format!("phase `{}` is answered standard: no jev form", p.name)),
        Some(_) => {}
    }
    // The tool side: every tool offered, in the order the tools slot lists them.
    let mut choices = Vec::new();
    let mut offered = 0;
    for name in crate::live::toolbox(e) {
        offered += 1;
        match builtin(e, &name, &phases) {
            Some(meta) => choices.extend(expand(&name, &meta)),
            None => j.missing.push(format!("tool `{name}` has no jev metadata")),
        }
    }
    for t in &e.mcp_tools {
        offered += 1;
        let name = t.call_name();
        match from_schema(&t.schema) {
            Some(Ok(meta)) => choices.extend(expand(&name, &meta)),
            Some(Err(why)) => j.missing.push(format!("tool `{name}`: {why}")),
            None => j.missing.push(format!("tool `{name}` has no jev metadata")),
        }
    }
    if offered == 0 {
        j.missing.push("no tools: nothing to choose".into());
    }
    if choices.len() > MAX_OPTIONS {
        j.missing.push(format!("more than {MAX_OPTIONS} choices"));
    }
    // A free response contract still matters for the standard fallback,
    // not for the JEV form: the form is the question, not the contract.
    j.possible = j.missing.is_empty();
    if j.possible {
        j.form = Some(Form { instructions: phase.map(|p| p.goal.clone()).unwrap_or_default(), choices });
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;
    use serde_json::json;

    const JEV_PHASE: &str = "| phase | response |\n|-|-|\n| gate | jev |\n| work | |\n\n## phase: gate\nShould the pending call run?\n\n## phase: work\nDo it.";

    fn engine(tools: &[&str], template: &str) -> Engine {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.tools = tools.iter().map(|t| t.to_string()).collect();
        e.template = template.into();
        e
    }

    fn guard(schema: Value) -> crate::mcp::Tool {
        crate::mcp::Tool { server: "guard".into(), name: "decide".into(), description: "Decide.".into(), schema }
    }

    fn verdict() -> Value {
        json!({"properties": {"verdict": {"type": "string", "enum": ["approve", "deny", "escalate"]}}, "required": ["verdict"], "x-jev": {"when": "the pending call is judged"}})
    }

    #[test]
    fn a_jev_phase_whose_tools_all_carry_metadata_is_possible() {
        let mut e = engine(&[], JEV_PHASE);
        e.mcp_tools.push(guard(verdict()));
        let j = check(&e);
        assert!(j.possible && j.asked, "{j:?}");
        let f = j.form.as_ref().unwrap();
        assert_eq!(f.instructions, "Should the pending call run?");
        let keys: Vec<&str> = f.choices.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["goto_phase work", "guard.decide approve", "guard.decide deny", "guard.decide escalate"]);
        assert_eq!(f.choices[2].call.to_string(), "guard.decide(verdict=\"deny\")");
        assert!(j.line().starts_with("possible (4 choices: goto_phase work, guard.decide approve,") && j.line().contains("jev would be used here"), "{}", j.line());
    }

    #[test]
    fn a_tool_without_metadata_says_which() {
        let mut e = engine(&["run"], JEV_PHASE);
        e.mcp_tools.push(guard(verdict()));
        let j = check(&e);
        assert!(!j.possible && j.form.is_none());
        assert_eq!(j.missing, ["tool `run` has no jev metadata"]);
        assert_eq!(j.line(), "not possible — tool `run` has no jev metadata; this phase asks for jev, so it is answered the standard way");
        let mut free = engine(&[], JEV_PHASE);
        free.mcp_tools.push(guard(json!({"properties": {"note": {"type": "string"}}, "x-jev": {"when": "always"}})));
        assert_eq!(check(&free).missing, ["tool `guard.decide`: input `note` has no fixed values (enum, const or boolean)"]);
    }

    #[test]
    fn a_standard_phase_is_still_checked_for_information() {
        let mut e = engine(&[], JEV_PHASE);
        e.mcp_tools.push(guard(verdict()));
        e.run.phase = 1;
        let j = check(&e);
        assert!(!j.asked && !j.possible);
        assert_eq!(j.line(), "not possible — phase `work` is answered standard: no jev form");
        let bare = check(&engine(&["reply", "list"], ""));
        assert_eq!(bare.missing.len(), 3, "{:?}", bare.missing);
        assert_eq!(bare.line(), "not possible — no phases: the response has no jev form (a phase with `response: jev` gives it one) (and 2 more)");
        assert!(check(&engine(&[], JEV_PHASE)).possible, "goto_phase alone carries its metadata");
    }

    #[test]
    fn metadata_needs_a_when() {
        assert_eq!(from_schema(&json!({"x-jev": {}})), Some(Err("x-jev has no `when`".into())));
        assert_eq!(from_schema(&json!({"properties": {}})), None);
        let ok = from_schema(&json!({"properties": {"ok": {"type": "boolean"}}, "x-jev": {"when": "w"}})).unwrap().unwrap();
        assert_eq!(ok.inputs, [("ok".to_string(), vec!["true".to_string(), "false".to_string()])]);
    }
}
