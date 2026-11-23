//! Declared capabilities: `capabilities/<id>.md`, what a machine says it
//! provides about itself (docs/capabilities.md, "Declared capabilities"). Pure: the parser, the
//! argv a call becomes, and the manifest a registry holds; the shell runs
//! the commands.
//!
//! ```markdown
//! # battery
//! The Mac's battery: how charged it is, and whether it is on power.
//!
//! probe: pmset -g batt
//! snapshot: pmset -g batt
//! interval: 60s
//!
//! ## tool: status
//! The battery now, as pmset reports it.
//! command: pmset -g batt
//!
//! ## event: unplugged
//! when: contains discharging
//! The Mac went on battery power.
//! ```
//!
//! - `probe:` a command; exit 0 means present, anything else (or a
//!   program that is not there) absent, with why. No probe: present.
//! - `snapshot:` a command whose output is the state block, read every
//!   `interval:` (default 60 s).
//! - `## tool: <name>`: its doc, `command:` (an argument list, the program
//!   first: split on spaces, or a JSON array), `inputs:` (`- name (type,
//!   required): doc`), `timeout:`, `approval: yes` for an actuator.
//! - `## event: <name>`: `when:` (`contains <text>`, `missing <text>`,
//!   `<key> <op> <value>`), then the text the engines using it are told.
//!
//! **Inputs never go through a shell.** An argument that is exactly
//! `{name}` becomes that input's value as one whole argument (left out
//! when an optional input is not given); every given input is also in the
//! environment as `DRY_INPUT_<NAME>`. A placeholder inside an argument
//! (`"pin={pin}"`), the program as an input, or a placeholder right after
//! a code flag (`sh -c {x}`, `python -c`, `node -e`) is refused when the
//! file is read. Probe and snapshot commands take no inputs.

use super::{Capability, Event, Input, Provider, Snapshot, State, Tool, Ty, When};

/// Flags after which an argument is run as code.
pub const CODE_FLAGS: [&str; 11] = ["-c", "-e", "--eval", "-p", "--print", "-Command", "-command", "/c", "/C", "-r", "--exec"];

/// The default snapshot interval, seconds.
pub const INTERVAL: u64 = 60;
/// The default timeout of a command, seconds.
pub const TIMEOUT: u64 = 30;

/// One argument of a command: as written, or an input's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Text(String),
    Input(String),
}

/// A declared tool: its manifest part and its command.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTool {
    pub tool: Tool,
    pub command: Vec<Arg>,
    pub timeout: u64,
}

/// One `capabilities/<id>.md`, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Declared {
    pub id: String,
    pub doc: String,
    pub probe: Option<Vec<String>>,
    pub snapshot: Option<Vec<String>>,
    pub interval: u64,
    pub tools: Vec<DeclaredTool>,
    pub events: Vec<Event>,
}

/// A command ready to run: the program, its arguments, the inputs' environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Argv {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub timeout: u64,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn valid_input(n: &str) -> bool {
    let mut b = n.bytes();
    b.next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_') && b.all(|c| c.is_ascii_alphanumeric() || c == b'_') && n.len() <= 64
}

/// `60s`, `2m`, `500ms` (rounded up to a second), `90` (seconds).
pub fn duration(v: &str) -> Option<u64> {
    let v = v.trim();
    let (n, unit) = v.find(|c: char| !c.is_ascii_digit()).map_or((v, ""), |i| (&v[..i], v[i..].trim()));
    let n: u64 = n.parse().ok()?;
    Some(match unit {
        "" | "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "ms" => n.div_ceil(1000),
        _ => return None,
    })
}

/// A command line as an argument list: a JSON array, or split on spaces.
fn argv(v: &str) -> Result<Vec<String>, String> {
    let v = v.trim();
    let list: Vec<String> = if v.starts_with('[') {
        serde_json::from_str::<Vec<String>>(v).map_err(|e| format!("not a JSON list of strings: {e}"))?
    } else {
        v.split_whitespace().map(str::to_string).collect()
    };
    if list.is_empty() || list.iter().any(String::is_empty) {
        return Err("a command is an argument list, the program first".into());
    }
    Ok(list)
}

/// The placeholders inside `arg` (`{name}`).
fn placeholders(arg: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = arg;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if valid_input(&after[..close]) => {
                out.push(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// A tool's command checked: inputs only as whole arguments, never the
/// program, never after a code flag.
fn command(list: Vec<String>, inputs: &[Input]) -> Result<Vec<Arg>, String> {
    let mut out = Vec::new();
    for (n, arg) in list.iter().enumerate() {
        let whole = arg.strip_prefix('{').and_then(|a| a.strip_suffix('}')).filter(|a| valid_input(a));
        if let Some(name) = whole {
            if !inputs.iter().any(|i| i.name == name) {
                return Err(format!("{{{name}}} is not one of its inputs"));
            }
            if n == 0 {
                return Err("the program itself cannot be an input".into());
            }
            if CODE_FLAGS.contains(&list[n - 1].as_str()) {
                return Err(format!("an input cannot follow {}: it would run as code", list[n - 1]));
            }
            out.push(Arg::Input(name.to_string()));
        } else {
            if placeholders(arg).iter().any(|p| inputs.iter().any(|i| i.name == *p)) {
                return Err(format!("\"{arg}\": an input goes in as a whole argument ({{name}}), never inside one"));
            }
            out.push(Arg::Text(arg.clone()));
        }
    }
    Ok(out)
}

/// `- pin (integer, required): the pin number`
fn input_line(line: &str) -> Option<Result<Input, String>> {
    let rest = line.trim_start().strip_prefix("- ")?.trim();
    let name_end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let name = &rest[..name_end];
    let mut rest = rest[name_end..].trim_start();
    let mut ty = Ty::String;
    let mut required = false;
    if let Some(r) = rest.strip_prefix('(') {
        let Some(close) = r.find(')') else { return Some(Err(format!("`{line}`: a `(` with no `)`"))) };
        for flag in r[..close].split(',').map(str::trim).filter(|f| !f.is_empty()) {
            match (flag, Ty::parse(flag)) {
                ("required", _) => required = true,
                (_, Some(t)) => ty = t,
                _ => {
                    return Some(Err(format!("`{flag}`: an input's flags are its type (string, integer, number, boolean) and `required`")));
                }
            }
        }
        rest = r[close + 1..].trim_start();
    }
    if !valid_input(name) {
        return Some(Err(format!("`{name}` is not an input name (letters, digits, _)")));
    }
    let doc = rest.strip_prefix(':').unwrap_or(rest).trim().to_string();
    Some(Ok(Input { name: name.to_string(), ty, required, doc }))
}

/// The tool or event section being read.
#[derive(Default)]
struct Section {
    start: usize,
    name: String,
    doc: Vec<String>,
    cmd: Option<(usize, Vec<String>)>,
    inputs: Vec<Input>,
    timeout: Option<u64>,
    approval: bool,
    when: Option<When>,
}

impl Section {
    fn finish(self, d: &mut Declared, part: &Part, file: &str) -> Result<(), String> {
        let err = |n: usize, why: String| format!("{file}:{n}: {why}");
        let name = self.name;
        match part {
            Part::Head => Ok(()),
            Part::Tool => {
                let (at, list) = self.cmd.ok_or_else(|| err(self.start, format!("tool {name} has no `command:`")))?;
                let command = command(list, &self.inputs).map_err(|w| err(at, w))?;
                if d.tools.iter().any(|t| t.tool.name == name) {
                    return Err(err(self.start, format!("a second tool named {name}")));
                }
                let tool = Tool { name, doc: self.doc.join(" "), inputs: self.inputs, approval: self.approval, remote: String::new() };
                d.tools.push(DeclaredTool { tool, command, timeout: self.timeout.unwrap_or(TIMEOUT) });
                Ok(())
            }
            Part::Event => {
                let when = self.when.ok_or_else(|| err(self.start, format!("event {name} has no `when:`")))?;
                if d.events.iter().any(|e| e.name == name) {
                    return Err(err(self.start, format!("a second event named {name}")));
                }
                let text = if self.doc.is_empty() { format!("{name}: {}", when.text()) } else { self.doc.join(" ") };
                d.events.push(Event { name, when, text });
                Ok(())
            }
        }
    }
}

#[derive(PartialEq)]
enum Part {
    Head,
    Tool,
    Event,
}

/// Read one capability file. `file` names it in errors (`battery.md`); an
/// error says its line: `battery.md:12: …`.
pub fn parse(file: &str, text: &str) -> Result<Declared, String> {
    let err = |n: usize, why: String| format!("{file}:{n}: {why}");
    let mut d = Declared {
        id: String::new(),
        doc: String::new(),
        probe: None,
        snapshot: None,
        interval: INTERVAL,
        tools: Vec::new(),
        events: Vec::new(),
    };
    let mut doc = Vec::new();
    let mut part = Part::Head;
    let mut sec = Section::default();
    let mut in_inputs = false;
    let mut interval_set = false;

    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim_end();
        let t = line.trim();
        if d.id.is_empty() {
            if t.is_empty() {
                continue;
            }
            let id =
                t.strip_prefix("# ").map(str::trim).ok_or_else(|| err(n, "the file starts with `# <id>`, the capability's id".into()))?;
            if !valid_id(id) {
                return Err(err(n, format!("`{id}` is not a capability id (lower case letters, digits, - and _)")));
            }
            d.id = id.to_string();
            continue;
        }
        if let Some(head) = t.strip_prefix("## ") {
            std::mem::take(&mut sec).finish(&mut d, &part, file)?;
            in_inputs = false;
            sec.start = n;
            let (what, name) = head.split_once(':').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((head.trim(), ""));
            part = match what {
                "tool" => Part::Tool,
                "event" => Part::Event,
                _ => return Err(err(n, format!("`## {head}`: a section is `## tool: <name>` or `## event: <name>`"))),
            };
            if !valid_input(name) {
                return Err(err(n, format!("`{name}` is not a {what} name (letters, digits, _)")));
            }
            sec.name = name.to_string();
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if in_inputs && let Some(input) = input_line(line) {
            let input = input.map_err(|w| err(n, w))?;
            if sec.inputs.iter().any(|x| x.name == input.name) {
                return Err(err(n, format!("a second input named {}", input.name)));
            }
            sec.inputs.push(input);
            continue;
        }
        in_inputs = false;
        let kv = t.split_once(':').map(|(k, v)| (k.trim(), v.trim()));
        match (&part, kv) {
            (Part::Head, Some(("probe", v))) => d.probe = Some(argv(v).map_err(|w| err(n, w))?),
            (Part::Head, Some(("snapshot", v))) => {
                let list = argv(v).map_err(|w| err(n, w))?;
                if list.iter().any(|a| !placeholders(a).is_empty()) {
                    return Err(err(n, "a snapshot command takes no inputs".into()));
                }
                d.snapshot = Some(list);
            }
            (Part::Head, Some(("interval", v))) => {
                d.interval = duration(v).filter(|s| *s > 0).ok_or_else(|| err(n, format!("`{v}` is not a duration (60s, 5m)")))?;
                interval_set = true;
            }
            (Part::Tool, Some(("command", v))) => sec.cmd = Some((n, argv(v).map_err(|w| err(n, w))?)),
            (Part::Tool, Some(("timeout", v))) => {
                sec.timeout = Some(duration(v).filter(|s| *s > 0).ok_or_else(|| err(n, format!("`{v}` is not a duration")))?)
            }
            (Part::Tool, Some(("approval", v))) => {
                sec.approval = match v {
                    "yes" | "true" => true,
                    "no" | "false" => false,
                    _ => return Err(err(n, format!("approval is yes or no, not `{v}`"))),
                }
            }
            (Part::Tool, Some(("inputs", ""))) => in_inputs = true,
            (Part::Event, Some(("when", v))) => sec.when = Some(When::parse(v).map_err(|w| err(n, w))?),
            (Part::Head, _) => doc.push(t.to_string()),
            (_, _) => sec.doc.push(t.to_string()),
        }
    }
    if d.id.is_empty() {
        return Err(err(1, "empty: the file starts with `# <id>`".into()));
    }
    sec.finish(&mut d, &part, file)?;
    if let Some(p) = &d.probe
        && p.iter().any(|a| !placeholders(a).is_empty())
    {
        return Err(err(1, "a probe takes no inputs".into()));
    }
    if interval_set && d.snapshot.is_none() {
        return Err(err(1, "`interval:` without a `snapshot:` command".into()));
    }
    if !d.events.is_empty() && d.snapshot.is_none() {
        return Err(err(1, "events are read from the snapshot: add a `snapshot:` command".into()));
    }
    d.doc = doc.join("\n");
    if d.doc.is_empty() {
        return Err(err(1, "say what it is and how to use it, under the `# id` line".into()));
    }
    Ok(d)
}

impl Declared {
    /// What a registry holds: the manifest, in `state` (the probe's answer).
    pub fn manifest(&self, state: State) -> Capability {
        Capability {
            id: self.id.clone(),
            doc: self.doc.clone(),
            tools: self.tools.iter().map(|t| t.tool.clone()).collect(),
            snapshot: self.snapshot.as_ref().map(|_| Snapshot { interval: Some(self.interval), ..Snapshot::default() }),
            events: self.events.clone(),
            state,
            provider: Provider::Declared,
        }
    }

    /// Whether it has an actuator (a tool needing approval).
    pub fn actuators(&self) -> Vec<&str> {
        self.tools.iter().filter(|t| t.tool.approval).map(|t| t.tool.name.as_str()).collect()
    }

    /// The command a call of tool `name` with `args` becomes: inputs
    /// checked (`Tool::check`), placed as whole arguments and in the
    /// environment, never interpolated.
    pub fn argv(&self, name: &str, args: &[(String, String)]) -> Result<Argv, String> {
        let t = self.tools.iter().find(|t| t.tool.name == name).ok_or_else(|| format!("{} has no tool {name}", self.id))?;
        let call = crate::calls::Call { name: format!("{}.{name}", self.id), args: args.to_vec() };
        t.tool.check(&call)?;
        let given = |n: &str| args.iter().find(|(k, v)| k == n && !v.is_empty()).map(|(_, v)| v.clone());
        let mut list = Vec::new();
        for a in &t.command {
            match a {
                Arg::Text(s) => list.push(s.clone()),
                Arg::Input(n) => list.extend(given(n)),
            }
        }
        let mut env: Vec<(String, String)> =
            t.tool.inputs.iter().filter_map(|i| given(&i.name).map(|v| (format!("DRY_INPUT_{}", i.name.to_uppercase()), v))).collect();
        env.push(("DRY_TOOL".into(), format!("{}.{name}", self.id)));
        let program = list.remove(0);
        Ok(Argv { program, args: list, env, timeout: t.timeout })
    }
}

/// A probe's or command's run, as the shell reports it, as the state:
/// exit 0 present, else absent with why.
pub fn probed(command: &[String], result: Result<i32, String>) -> State {
    match result {
        Ok(0) => State::Present,
        Ok(code) => State::Absent(format!("probe `{}` exited {code}", command.join(" "))),
        Err(why) => State::Absent(format!("probe `{}` did not run: {why}", command.join(" "))),
    }
}
