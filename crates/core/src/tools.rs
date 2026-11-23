//! An engine's tools: what the calls in `act` can do.
//!
//! These are the built-in ones. Each is pure (it works on the engine's own
//! artifacts), so the loop runs in the core and is tested without a model.
//! `reply` is a tool like the others: answering the user is how a turn
//! ends, not a special field in the response.
//!
//! `BROUGHT` are the tools a live artifact brings (`crate::live`): never
//! in an engine's own list, there only while the artifact is. `script` is
//! one of them, and the loop runs it (`crate::turn`), since its steps are
//! calls to the other tools.

use std::collections::BTreeMap;

use crate::calls::Call;

/// What a tool is, as the model is told.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// `(argument, what it is)`.
    pub args: &'static [(&'static str, &'static str)],
}

pub const BUILTIN: &[ToolSpec] = &[
    ToolSpec {
        name: "reply",
        // `describe` adds the registered ask components (`crate::ask`).
        description: "Answer the user. Ends your turn.",
        args: &[("text", "what to say"), ("ask", "optional: an ask component, below"), ("show", "optional: a display component, below")],
    },
    ToolSpec {
        name: "write",
        description: "Save an artifact (replaces one of the same name).",
        args: &[("name", "a short name"), ("text", "its whole content")],
    },
    ToolSpec { name: "read", description: "Read an artifact.", args: &[("name", "its name")] },
    ToolSpec { name: "list", description: "The names of all artifacts.", args: &[] },
];

/// `engine_call` and `engine_start`'s arguments.
const CHILD_ARGS: &[(&str, &str)] = &[
    ("name", "the child's name"),
    ("goal", "what it is for (a new child only)"),
    ("scope", "optional, new child: what it does alone, when it stops, what it never does"),
    ("done", "optional, new child: what a finished result is, and what it reports"),
    ("text", "optional: what to tell it"),
    ("template", "optional, new child: one of your templates by name (with root), or a template.md's text"),
    ("root", "optional, with a template's name: the project folder its artifacts work in"),
    ("model", "optional, new child: who runs it, a present row of your subscriptions (a model, or a session program); left out: your model"),
];

/// Tools that come with something the engine holds: a live artifact
/// (`crate::live::Live::tools`) or a phase with a next one
/// (`crate::phase::Run::tools`). They exist only while it does.
pub const BROUGHT: &[ToolSpec] = &[
    ToolSpec {
        name: "script",
        description: "Run one of your scripts (listed in the scripts artifact): its calls in order, stopping at the first that fails, with one report back. Give its params too, as listed, e.g. script(name=\"save_note\", text=\"hi\").",
        args: &[("name", "the script's name")],
    },
    ToolSpec {
        name: crate::skill::LOAD,
        description: "Load one of your skills (listed in the skills artifact): its instructions come back, and its tools are yours until the next message.",
        args: &[("name", "the skill's name")],
    },
    ToolSpec {
        name: crate::skill::SAVE,
        description: "Create a skill, or replace one of the same name: instructions you or another engine can load later.",
        args: &[
            ("name", "lower case, - and _"),
            ("description", "one line: what it is for and when to load it"),
            ("text", "its instructions"),
            ("tools", "optional: tools it brings, comma separated"),
        ],
    },
    ToolSpec {
        name: "open",
        description: "Open a file of your file artifact: its text is in the artifact from now on, as it is on disk at every step. Open several at once with `|`: path=\"src/a.rs|src/b.rs\".",
        args: &[("path", "relative to its root")],
    },
    ToolSpec {
        name: "focus",
        description: "Show only these lines of an open (or not yet open) file, for a big one.",
        args: &[("path", "relative to its root"), ("from", "first line"), ("to", "last line")],
    },
    ToolSpec {
        name: "edit",
        description: "Change an open file: the old text (exactly once in it) becomes the new. Held as unsaved, shown marked *, until save.",
        args: &[("path", "an open file"), ("old", "the text to replace, exactly"), ("new", "what it becomes")],
    },
    ToolSpec { name: "save", description: "Write a file's unsaved edits to disk.", args: &[("path", "the file")] },
    ToolSpec {
        name: "run",
        description: "Run a shell command in the terminal's root and wait for it. Back: its exit code and last lines. For a long one use start.",
        args: &[("command", "the command line")],
    },
    ToolSpec {
        name: "start",
        description: "Start a long command and go on at once; when it ends a message wakes you. Its output is in the terminal artifact. Optional: wait=\"yes\" ends your turn here with no reply (nobody is told) and wakes you, in this phase, when it ends.",
        args: &[("command", "the command line")],
    },
    ToolSpec {
        name: "background",
        description: "Start a command that keeps running (a server, a watcher); no message when it ends. Its state is in the terminal artifact.",
        args: &[("command", "the command line")],
    },
    ToolSpec {
        name: "browse",
        description: "Open a page of an app on this machine (localhost) in your browser artifact: its text, console errors and a screenshot are in the artifact from now on.",
        args: &[("url", "http://127.0.0.1:…/ or http://localhost:…/")],
    },
    ToolSpec {
        name: "click",
        description: "Click an element of the open page: a CSS selector, or the text it shows (a button's, a link's).",
        args: &[("target", "a selector or the visible text")],
    },
    ToolSpec {
        name: "type",
        description: "Type text into a field of the open page; key=\"Enter\" presses a key after.",
        args: &[("target", "a selector, the field's label or placeholder"), ("text", "what to type")],
    },
    ToolSpec {
        name: "evidence",
        description: "The project's evidence (docs/evidence.md): op=\"run\" builds and starts the app, shoots every scenario and compares with the committed baseline; op=\"read\" reads the last run. Back: one line; the summary is in the evidence artifact. Optional: expect=\"home,settings\" (scenarios your change should change: those pass), update=\"yes\" (take every shot as the new baseline).",
        args: &[("op", "run or read"), ("expect", "optional: scenarios expected to change, comma separated")],
    },
    ToolSpec {
        name: "engine_call",
        description: "Ask a child engine and wait: made with goal (and scope, done) when it is new; its reply comes back as this call's result. Optional: text (else: work on your goal), and for a new one: template (one of your templates by name, with root=\"<project folder>\"; or a template.md's text), skills and kinds (comma separated: engines, learning, clock), and model (a present row of your subscriptions: a model runs it, a session row makes it a session of that program).",
        args: CHILD_ARGS,
    },
    ToolSpec {
        name: "engine_start",
        description: "The same as engine_call, but go on at once; when the child replies, a message wakes you. Its state is in the engines artifact.",
        args: CHILD_ARGS,
    },
    ToolSpec {
        name: "post",
        description: "Post a directed message on your thread (you, your head and every engine of its tree see it). Only the engine named in `to` is called; say what, why, what done looks like and where the material is, so it can act without having seen anything else.",
        args: &[
            ("text", "the message, at most 600 characters"),
            ("to", "the engine's id, or `you` for the person; a post with no `to` comes back to you undelivered"),
        ],
    },
    ToolSpec {
        name: "edit_post",
        description: "Change an entry of yours in the shared context; text=\"\" removes it.",
        args: &[("n", "the entry's number"), ("text", "its new text, or \"\"")],
    },
    ToolSpec {
        name: "collect",
        description: "Gather the context one learning section learns from, into the learning artifact: corrections (for user), fixes (for lessons), facts (for environment).",
        args: &[("what", "corrections, fixes or facts")],
    },
    ToolSpec {
        name: "learn",
        description: "Write one line into your learning file. old=\"...\" replaces the entry it names; with entry=\"\" removes it. A full section refuses: merge or remove first.",
        args: &[("section", "user, environment or lessons"), ("entry", "one line"), ("old", "optional: the entry to replace or remove")],
    },
    ToolSpec {
        name: "analytics",
        description: "What your own recorded runs say about the last 7 days, one line back and the facts into the learning artifact: tools (calls, failures, p95, never used), models (errors, tokens/s, first token, cost), time (work vs waiting), phases, context (compressions, prompt growth), asks (fallbacks).",
        args: &[("query", "tools, models, time, phases, context or asks")],
    },
    ToolSpec {
        name: crate::phase::GOTO,
        description: "Move to another phase of the plan (see Phase): on when this one is done, or back when the work needs it.",
        args: &[("name", "the phase to go to")],
    },
];

/// Every built-in tool's name: what a new engine holds.
pub fn builtin_names() -> Vec<String> {
    BUILTIN.iter().map(|t| t.name.to_string()).collect()
}

/// The tools named in `names`, described for the prompt.
pub fn describe(names: &[String]) -> String {
    BUILTIN
        .iter()
        .chain(BROUGHT)
        .filter(|t| names.iter().any(|n| n == t.name))
        .map(|t| {
            let args: Vec<String> = t.args.iter().map(|(a, _)| format!("{a}=\"...\"")).collect();
            let what: Vec<String> = t.args.iter().map(|(a, d)| format!("{a}: {d}")).collect();
            let what = if what.is_empty() { String::new() } else { format!(" ({})", what.join("; ")) };
            let more = if t.name == "reply" { crate::ask::instructions() } else { String::new() };
            format!("- {}({}): {}{what}{more}", t.name, args.join(", "), t.description)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The options in a `choices` argument: split on `|`, trimmed, blanks
/// dropped.
pub fn split_choices(choices: &str) -> Vec<String> {
    choices.split('|').map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).collect()
}

/// What a call came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// A result for the model to read on its next step.
    Result(String),
    /// The answer for the user: the turn is over.
    Reply(String),
}

/// Run one call with the engine's tools (`allowed`) on its artifacts.
/// Error: what was wrong, for the model to read and fix.
pub fn run(call: &Call, allowed: &[String], artifacts: &mut BTreeMap<String, String>) -> Result<Outcome, String> {
    if !allowed.contains(&call.name) {
        // A writing tool left out by a read-only artifact says why.
        if let Some(why) = crate::capability::refusal(artifacts.iter(), &call.name) {
            return Err(why);
        }
        return Err(format!("no tool `{}`; yours are {}", call.name, allowed.join(", ")));
    }
    let need = |key: &str| call.arg(key).map(str::to_string).ok_or_else(|| format!("`{}` needs {key}=\"...\"", call.name));
    match call.name.as_str() {
        "reply" => Ok(Outcome::Reply(need("text")?)),
        "write" => {
            let (name, text) = (need("name")?, need("text")?);
            // A file or terminal artifact's state changes only through its
            // tools (so its root, where files are read and commands run,
            // stays what was set by hand).
            let kind = [artifacts.get(&name).map_or("", String::as_str), text.as_str()].into_iter().find_map(crate::live::Live::of);
            if let Some(kind) = kind.filter(|k| k.has_state()) {
                let tools = kind.tools().join(", ");
                return Err(format!("{name} is a {} artifact: it changes only through its tools ({tools})", kind.as_str()));
            }
            let size = text.chars().count();
            artifacts.insert(name.clone(), text);
            Ok(Outcome::Result(format!("saved {name} ({size} characters)")))
        }
        "read" => {
            let name = need("name")?;
            artifacts.get(&name).cloned().map(Outcome::Result).ok_or_else(|| format!("no artifact {name}"))
        }
        "list" => Ok(Outcome::Result(if artifacts.is_empty() {
            "no artifacts yet".into()
        } else {
            artifacts.keys().cloned().collect::<Vec<_>>().join(", ")
        })),
        other => Err(format!("`{other}` is not built in")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calls::parse;

    #[test]
    fn write_read_list_and_reply() {
        let all = builtin_names();
        let mut artifacts = BTreeMap::new();
        let calls = parse(r#"write(name="a", text="hello"); read(name="a"); list(); reply(text="done")"#).unwrap();
        let out: Vec<_> = calls.iter().map(|c| run(c, &all, &mut artifacts)).collect();
        assert_eq!(out[0], Ok(Outcome::Result("saved a (5 characters)".into())));
        assert_eq!(out[1], Ok(Outcome::Result("hello".into())));
        assert_eq!(out[2], Ok(Outcome::Result("a".into())));
        assert_eq!(out[3], Ok(Outcome::Reply("done".into())));
        assert!(describe(&all).contains("- write(name=\"...\", text=\"...\"): Save an artifact"));
        assert!(describe(&all).contains("- reply(text=\"...\", ask=\"...\", show=\"...\")"));
    }

    #[test]
    fn mistakes_come_back_as_text_the_model_can_fix() {
        let only_reply = vec!["reply".to_string()];
        let mut artifacts = BTreeMap::new();
        let c = |s: &str| parse(s).unwrap().remove(0);
        assert!(run(&c("write(name=x, text=y)"), &only_reply, &mut artifacts).unwrap_err().starts_with("no tool `write`"));
        assert_eq!(run(&c("reply()"), &only_reply, &mut artifacts).unwrap_err(), "`reply` needs text=\"...\"");
        assert!(artifacts.is_empty());
    }

    #[test]
    fn write_never_makes_or_replaces_a_file_or_terminal_artifact() {
        let all = builtin_names();
        let mut artifacts = BTreeMap::from([(
            "code".to_string(),
            "kind: file
root: ."
                .to_string(),
        )]);
        let c = |s: &str| parse(s).unwrap().remove(0);
        assert!(
            run(&c("write(name=\"code\", text=\"x\")"), &all, &mut artifacts)
                .unwrap_err()
                .contains("changes only through its tools (open, focus, edit, save)")
        );
        assert!(run(&c("write(name=\"t\", text=\"kind: terminal\nroot: /\")"), &all, &mut artifacts).is_err());
        assert!(run(&c("write(name=\"now\", text=\"kind: clock\")"), &all, &mut artifacts).is_ok(), "a kind with no state of its own");
    }
}
