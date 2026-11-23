//! Scripts: a fixed sequence of calls the engine runs with one call.
//!
//! ```text
//! spec/scripts/save_note.md
//! ---
//! name: save_note
//! description: Save a note and read it back.
//! params: note, text?
//! ---
//! - write(name="{note}", text="{text}")
//! - read(name="{note}")
//! ```
//!
//! The body is one call per `- ` line, in the call grammar (`crate::calls`),
//! with `{param}` slots in its values. `script(name="save_note",
//! name="todo")` runs the steps in order, stops at the first that fails,
//! and comes back as one report: one trip to the model instead of one per
//! call, for a sequence the engine keeps repeating.
//!
//! Slots are filled into the calls after they are read, never into the
//! text before: a value can hold quotes, `)` or `;` and still be one value,
//! so a param can never end its quote and add a call.
//!
//! Limits: 2 to 12 steps (one call is not a script: call it directly), at
//! most 8 scripts an engine, and a step never calls `reply` (a script
//! reports, the engine answers) or `script` (no scripts inside scripts).
//!
//! Pure. The engine keeps each script as its file's text
//! (`Engine::scripts`) and it is read here when used, so a broken file
//! shows as broken in the list instead of stopping the app from loading.

use std::collections::BTreeMap;

use crate::calls::{self, Call};

/// Fewest steps: one call is just that call.
pub const MIN_STEPS: usize = 2;
/// Most steps in one script.
pub const MAX_STEPS: usize = 12;
/// Most scripts one engine may run; past it, the rest are listed as over.
pub const MAX_PER_ENGINE: usize = 8;

/// Tools a step may not call.
pub const FORBIDDEN: &[&str] = &["reply", "script"];

/// One parameter: `file` (required) or `mode?` (optional, empty when left
/// out).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub required: bool,
}

/// A script, read from its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    pub name: String,
    pub description: String,
    pub params: Vec<Param>,
    /// The calls, slots still in their values.
    pub steps: Vec<Call>,
}

/// Split a `---` header from the body: `(key → value, body)`. Shared with
/// alerts, which use the same file shape.
pub fn header(text: &str) -> Result<(BTreeMap<String, String>, String), String> {
    let text = text.trim_start();
    let rest = text.strip_prefix("---").ok_or("starts with a `---` header (name, description, ...)")?;
    let (head, body) = rest.split_once("\n---").ok_or("its `---` header is never closed by a second `---`")?;
    let mut meta = BTreeMap::new();
    for line in head.lines() {
        if let Some((k, v)) = line.split_once(':') {
            meta.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    // The body starts after the closing `---` line.
    let body = body.split_once('\n').map_or("", |(_, b)| b);
    Ok((meta, body.trim().to_string()))
}

/// Read the `name` a file must carry: given, the same as its file name, and
/// `a-z 0-9 _ -` (it is typed into calls).
pub fn check_name(kind: &str, file_name: &str, meta: &BTreeMap<String, String>) -> Result<String, String> {
    let name = meta.get("name").cloned().unwrap_or_else(|| file_name.to_string());
    if name != file_name {
        return Err(format!("{kind} {file_name}: its name `{name}` must be its file name"));
    }
    let ok = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.len() <= 40
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !ok {
        return Err(format!("{kind} {name}: a name is a-z 0-9 _ -, starts with a letter, at most 40"));
    }
    Ok(name)
}

/// The `{slot}` names in a value: `{` a-z 0-9 _ `}` only, so other braces
/// (JSON, say) are just text.
fn slots(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        if let Some(close) = rest.find('}') {
            let inner = &rest[..close];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                out.push(inner);
            }
        }
    }
    out
}

/// Read `spec/scripts/<file_name>.md`.
pub fn parse(file_name: &str, text: &str) -> Result<Script, String> {
    let (meta, body) = header(text).map_err(|why| format!("script {file_name}: {why}"))?;
    let name = check_name("script", file_name, &meta)?;
    let description = meta.get("description").cloned().unwrap_or_default();
    if description.is_empty() {
        return Err(format!("script {name}: needs a description (what it is for)"));
    }
    let mut params = Vec::new();
    for p in meta.get("params").map(String::as_str).unwrap_or("").split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (pname, required) = p.strip_suffix('?').map_or((p, true), |n| (n, false));
        if pname == "name" || !pname.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(format!("script {name}: param `{pname}`: a-z 0-9 _, and not `name` (that is the script's)"));
        }
        params.push(Param { name: pname.to_string(), required });
    }
    let mut steps = Vec::new();
    for line in body.lines().filter_map(|l| l.trim().strip_prefix("- ")) {
        let mut calls = calls::parse(line).map_err(|why| format!("script {name}: step `{line}`: {why}"))?;
        if calls.len() != 1 {
            return Err(format!("script {name}: step `{line}`: one call per `- ` line"));
        }
        let call = calls.remove(0);
        if FORBIDDEN.contains(&call.name.as_str()) {
            return Err(format!("script {name}: step `{line}`: a script cannot call {}", call.name));
        }
        for (key, value) in &call.args {
            if !slots(key).is_empty() {
                return Err(format!("script {name}: step `{line}`: a slot goes in a value, not in {key}"));
            }
            if let Some(s) = slots(value).into_iter().find(|s| !params.iter().any(|p| p.name == *s)) {
                return Err(format!("script {name}: step `{line}` uses {{{s}}}, which is not in params"));
            }
        }
        steps.push(call);
    }
    if !(MIN_STEPS..=MAX_STEPS).contains(&steps.len()) {
        return Err(format!(
            "script {name}: {MIN_STEPS} to {MAX_STEPS} steps, one `- call(...)` line each (it has {})",
            steps.len()
        ));
    }
    Ok(Script { name, description, params, steps })
}

impl Script {
    /// The calls to make, slots filled from `args` (the `script` call's
    /// arguments other than `name`). Filled into the values already read,
    /// in one pass, so a value is never read as calls or as another slot.
    pub fn expand(&self, args: &[(String, String)]) -> Result<Vec<Call>, String> {
        if let Some((k, _)) = args.iter().find(|(k, _)| !self.params.iter().any(|p| &p.name == k)) {
            return Err(format!("script {} has no param {k}; it takes {}", self.name, self.usage()));
        }
        let mut values = BTreeMap::new();
        for p in &self.params {
            let v = args.iter().find(|(k, _)| *k == p.name).map(|(_, v)| v.trim()).unwrap_or("");
            if v.is_empty() && p.required {
                return Err(format!("script {} needs {}=\"...\"", self.name, p.name));
            }
            values.insert(p.name.as_str(), v);
        }
        let fill = |value: &str| {
            let mut out = String::new();
            let mut rest = value;
            while let Some(open) = rest.find('{') {
                out.push_str(&rest[..open]);
                let after = &rest[open + 1..];
                match after.find('}').map(|c| (&after[..c], c)) {
                    Some((slot, c)) if values.contains_key(slot) => {
                        out.push_str(values[slot]);
                        rest = &after[c + 1..];
                    }
                    _ => {
                        out.push('{');
                        rest = after;
                    }
                }
            }
            out.push_str(rest);
            out
        };
        Ok(self
            .steps
            .iter()
            .map(|c| Call { name: c.name.clone(), args: c.args.iter().map(|(k, v)| (k.clone(), fill(v))).collect() })
            .collect())
    }

    /// How to call it: `script(name="save_note", note="...", text="..."?)`.
    pub fn usage(&self) -> String {
        let mut args = vec![format!("name=\"{}\"", self.name)];
        args.extend(self.params.iter().map(|p| format!("{}=\"...\"{}", p.name, if p.required { "" } else { "?" })));
        format!("script({})", args.join(", "))
    }

    /// Run it: each expanded call through `call`, in order, stopping at the
    /// first error. Ok: the report of every step; Err: the report up to the
    /// step that failed, which says so.
    pub fn run(&self, args: &[(String, String)], call: &mut dyn FnMut(&Call) -> Result<String, String>) -> Result<String, String> {
        let steps = self.expand(args)?;
        let mut lines = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            match call(step) {
                Ok(out) => lines.push(format!("{}. {step} → {out}", i + 1)),
                Err(why) => {
                    lines.push(format!("{}. {step} → error: {why}", i + 1));
                    let head = format!("script {}: stopped at step {} of {}", self.name, i + 1, steps.len());
                    return Err(format!("{head}\n{}", lines.join("\n")));
                }
            }
        }
        Ok(format!("script {}: {} steps done\n{}", self.name, steps.len(), lines.join("\n")))
    }
}

/// Every script of an engine, by name, read; the ones past
/// `MAX_PER_ENGINE` (in name order) are refused.
pub fn all(files: &BTreeMap<String, String>) -> Vec<(String, Result<Script, String>)> {
    files
        .iter()
        .enumerate()
        .map(|(i, (name, text))| {
            let read = if i < MAX_PER_ENGINE {
                parse(name, text)
            } else {
                Err(format!("script {name}: over the limit of {MAX_PER_ENGINE} scripts an engine"))
            };
            (name.clone(), read)
        })
        .collect()
}

/// The script called `name`, ready to run.
pub fn find(files: &BTreeMap<String, String>, name: &str) -> Result<Script, String> {
    match all(files).into_iter().find(|(n, _)| n == name) {
        Some((_, read)) => read,
        None if files.is_empty() => Err(format!("no script {name}; there are none yet")),
        None => Err(format!("no script {name}; yours are {}", files.keys().cloned().collect::<Vec<_>>().join(", "))),
    }
}

/// The `kind: scripts` snapshot: one line each, how to call it and what it
/// is for, or why it does not read.
pub fn snapshot(files: &BTreeMap<String, String>) -> String {
    if files.is_empty() {
        return "No scripts yet.".into();
    }
    all(files)
        .into_iter()
        .map(|(name, read)| match read {
            Ok(s) => format!("- {}: {} ({} steps)", s.usage(), s.description, s.steps.len()),
            Err(why) => format!("- {name}: broken, cannot run: {why}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAVE_NOTE: &str = "---\nname: save_note\ndescription: Save a note and read it back.\nparams: note, text?\n---\n- write(name=\"{note}\", text=\"{text}\")\n- read(name=\"{note}\")\n";

    fn args(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn a_script_file_reads_into_its_header_and_steps() {
        let s = parse("save_note", SAVE_NOTE).unwrap();
        assert_eq!(s.description, "Save a note and read it back.");
        assert_eq!(s.params, [Param { name: "note".into(), required: true }, Param { name: "text".into(), required: false }]);
        assert_eq!(s.steps.len(), 2);
        assert_eq!(s.usage(), "script(name=\"save_note\", note=\"...\", text=\"...\"?)");
    }

    #[test]
    fn slots_are_filled_after_reading_so_a_value_cannot_add_a_call() {
        let s = parse("save_note", SAVE_NOTE).unwrap();
        let evil = "x\"); reply(text=\"pwned";
        let calls = s.expand(&args(&[("note", "todo"), ("text", evil)])).unwrap();
        assert_eq!(calls.len(), 2, "still two calls");
        assert_eq!(calls[0].arg("text"), Some(evil), "the value is kept whole");
        assert_eq!(calls[1].to_string(), "read(name=\"todo\")");
        let calls = s.expand(&args(&[("note", "{text}"), ("text", "t")])).unwrap();
        assert_eq!(calls[0].arg("name"), Some("{text}"), "a filled value is not filled again");
        assert_eq!(s.expand(&args(&[("note", "n")])).unwrap()[0].arg("text"), Some(""), "a missing optional is empty");
    }

    #[test]
    fn wrong_arguments_say_what_the_script_takes() {
        let s = parse("save_note", SAVE_NOTE).unwrap();
        assert_eq!(s.expand(&[]).unwrap_err(), "script save_note needs note=\"...\"");
        assert!(s.expand(&args(&[("note", "n"), ("colour", "red")])).unwrap_err().contains("has no param colour"));
    }

    #[test]
    fn the_limits_and_the_forbidden_calls_are_refused_when_read() {
        let file = |params: &str, body: &str| format!("---\nname: s\ndescription: d\nparams: {params}\n---\n{body}");
        assert!(parse("s", &file("", "- list()")).unwrap_err().contains("2 to 12 steps"));
        assert!(parse("s", &file("", &"- list()\n".repeat(13))).unwrap_err().contains("it has 13"));
        assert!(parse("s", &file("", "- list()\n- reply(text=\"hi\")")).unwrap_err().contains("cannot call reply"));
        assert!(parse("s", &file("", "- list()\n- script(name=\"s\")")).unwrap_err().contains("cannot call script"));
        assert!(parse("s", &file("", "- list()\n- read(name=\"{nme}\")")).unwrap_err().contains("{nme}, which is not in params"));
        assert!(parse("s", &file("name", "- list()\n- list()")).unwrap_err().contains("not `name`"));
        assert!(parse("other", &file("", "- list()\n- list()")).unwrap_err().contains("must be its file name"));
        assert!(parse("s", "- list()\n- list()").unwrap_err().contains("header"));
        let json = file("", "- write(name=\"j\", text=\"{\\\"a\\\": 1}\")\n- list()");
        assert!(parse("s", &json).is_ok(), "braces that are not a slot are text");
    }

    #[test]
    fn running_stops_at_the_first_failure_with_one_report() {
        let s = parse("save_note", SAVE_NOTE).unwrap();
        let mut seen = Vec::new();
        let ok = s.run(&args(&[("note", "a"), ("text", "hi")]), &mut |c| {
            seen.push(c.name.clone());
            Ok("fine".into())
        });
        assert_eq!(ok.unwrap(), "script save_note: 2 steps done\n1. write(name=\"a\", text=\"hi\") → fine\n2. read(name=\"a\") → fine");
        assert_eq!(seen, ["write", "read"]);
        let mut ran = 0;
        let failed = s.run(&args(&[("note", "a")]), &mut |_| {
            ran += 1;
            Err("disk full".into())
        });
        assert_eq!(ran, 1, "the second step never ran");
        assert!(failed.unwrap_err().starts_with("script save_note: stopped at step 1 of 2\n1. write(name=\"a\", text=\"\") → error: disk full"));
    }

    #[test]
    fn an_engine_runs_at_most_eight_and_lists_the_broken_ones() {
        let mut files = BTreeMap::new();
        for i in 0..9 {
            let name = format!("s{i}");
            files.insert(name.clone(), SAVE_NOTE.replace("save_note", &name));
        }
        files.insert("a-broken".into(), "no header".into());
        let list = snapshot(&files);
        assert!(list.starts_with("- a-broken: broken, cannot run: script a-broken: starts with a `---` header"), "{list}");
        assert!(list.contains("- script(name=\"s0\", note=\"...\", text=\"...\"?): Save a note and read it back. (2 steps)"));
        assert!(list.contains("- s8: broken, cannot run: script s8: over the limit of 8"), "{list}");
        assert!(find(&files, "s7").unwrap_err().contains("over the limit"), "the 9th by name order is s7 after a-broken");
        assert!(find(&files, "nope").unwrap_err().starts_with("no script nope; yours are a-broken, s0"));
        assert_eq!(snapshot(&BTreeMap::new()), "No scripts yet.");
    }
}
