//! The companion's protocol: what a shell that reaches a dry companion
//! (`companion/dry-companion.mjs`, docs/companion.md) sends and reads. The
//! companion is a small server on the person's computer that gives a page
//! what a browser cannot reach: the computer's projects, its runtimes,
//! local inference and git. The same protocol can later be served by a
//! remote host, so this is renderer- and transport-neutral.
//!
//! Pure: the paths, the keys, and readers of the companion's JSON answers
//! into the core's own types (`live::Content`, `live::Proc`). The shell
//! does the I/O.

use serde_json::Value;

use crate::live::{Content, Job, Proc};

/// The protocol version this core speaks (`/v1/...`; `protocol` in the health answer).
pub const PROTOCOL: u64 = 1;
/// Where a companion listens unless told otherwise.
pub const DEFAULT_URL: &str = "http://127.0.0.1:7777";
/// The header the session token goes in (`Authorization: Bearer` works too).
pub const TOKEN_HEADER: &str = "x-dry-token";

/// A companion URL as typed: trimmed, `http://` added to a bare
/// `host:port`, no trailing `/`, and no `/v1` (the paths add it).
pub fn clean_url(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix("/v1").unwrap_or(u);
    if u.is_empty() {
        return DEFAULT_URL.to_string();
    }
    if u.contains("://") { u.to_string() } else { format!("http://{u}") }
}

/// The OpenAI-compatible base a model connection uses through it.
pub fn inference_base(url: &str) -> String {
    format!("{}/v1", clean_url(url))
}

/// The key a terminal session is known by at the companion: one per
/// engine, artifact and session number, so its status survives a reload
/// of the page (not a restart of the companion).
pub fn session_key(job: &Job) -> String {
    format!("{}/{}/{}", job.engine, job.artifact, job.session)
}

/// A file or terminal artifact's `root:` as a folder on the companion's
/// computer: an absolute root as it is; `.` (or nothing) is the open
/// project; a relative root is inside the open project.
pub fn resolve_root(open: Option<&str>, root: &str) -> Result<String, String> {
    let r = root.trim();
    if r.starts_with('/') {
        return Ok(r.trim_end_matches('/').to_string());
    }
    let open = open.ok_or("no project is open: open one in Projects, or give the artifact an absolute root")?;
    let open = open.trim_end_matches('/');
    Ok(match r.trim_start_matches("./").trim_end_matches('/') {
        "" | "." => open.to_string(),
        rel => format!("{open}/{rel}"),
    })
}

/// `path` under `root`.
pub fn join(root: &str, path: &str) -> String {
    format!("{}/{}", root.trim_end_matches('/'), path.trim_start_matches('/'))
}

fn v(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// The `error` an answer carries, if any.
pub fn error_of(text: &str) -> Option<String> {
    v(text).get("error").and_then(Value::as_str).map(str::to_string)
}

/// `GET /v1/files/read`'s answer as the core's `Content`.
pub fn content_of(text: &str) -> Content {
    let a = v(text);
    let n = |k: &str| a.get(k).and_then(Value::as_u64).unwrap_or(0);
    match a.get("kind").and_then(Value::as_str) {
        Some("text") => Content::Text(s(&a, "text")),
        Some("binary") => Content::Binary { bytes: n("bytes"), image: a.get("image").and_then(Value::as_bool).unwrap_or(false) },
        Some("too_large") => Content::TooLarge(n("bytes")),
        Some("missing") => Content::Missing(s(&a, "why")),
        _ => Content::Missing(error_of(text).unwrap_or_else(|| "the companion's answer does not read".into())),
    }
}

/// A session's answer (`/v1/exec/session`, `/v1/exec/run`) as a `Proc`.
/// No such session (the companion restarted): it ended, with no code.
pub fn proc_of(text: &str) -> Proc {
    let a = v(text);
    match a.get("status").and_then(Value::as_str) {
        Some("running") => Proc::Running,
        Some(_) => Proc::Ended(a.get("exit").and_then(Value::as_i64).and_then(|c| i32::try_from(c).ok())),
        None => Proc::Ended(None),
    }
}

/// `POST /v1/exec/run`'s answer: the exit code (`None`: killed, or timed
/// out) and its output, or why it did not run.
pub fn run_of(text: &str) -> Result<(Option<i32>, String), String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let a = v(text);
    if a.get("status").is_none() {
        return Err("the companion's answer does not read".into());
    }
    let code = a.get("exit").and_then(Value::as_i64).and_then(|c| i32::try_from(c).ok());
    let mut out = s(&a, "output");
    if a.get("timed_out").and_then(Value::as_bool) == Some(true) {
        out.push_str("\n(stopped: it ran past its timeout)");
    }
    Ok((code, out))
}

/// One runtime the companion found on its PATH.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    pub name: String,
    /// Its version line; `None`: not on this computer.
    pub version: Option<String>,
}

/// What the companion said it has (`GET /v1/capabilities`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub version: String,
    pub protocol: u64,
    pub platform: String,
    pub hostname: String,
    pub roots: Vec<String>,
    pub runtimes: Vec<Runtime>,
}

impl Capabilities {
    pub fn from_json(text: &str) -> Result<Capabilities, String> {
        if let Some(e) = error_of(text) {
            return Err(e);
        }
        let a = v(text);
        if !a.is_object() {
            return Err("the companion's capabilities do not read".into());
        }
        let mut runtimes: Vec<Runtime> = a
            .get("runtimes")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, r)| Runtime {
                        name: k.clone(),
                        version: r.as_object().map(|o| o.get("version").and_then(Value::as_str).unwrap_or("found").to_string()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        runtimes.sort_by(|a, b| (a.version.is_none(), &a.name).cmp(&(b.version.is_none(), &b.name)));
        Ok(Capabilities {
            version: s(&a, "version"),
            protocol: a.get("protocol").and_then(Value::as_u64).unwrap_or(0),
            platform: s(&a, "platform"),
            hostname: s(&a, "hostname"),
            roots: strings(a.get("roots")),
            runtimes,
        })
    }

    /// Whether it speaks this core's protocol.
    pub fn compatible(&self) -> bool {
        self.protocol == PROTOCOL
    }

    pub fn has(&self, runtime: &str) -> bool {
        self.runtimes.iter().any(|r| r.name == runtime && r.version.is_some())
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// A project on the companion's computer (`GET /v1/projects`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub path: String,
    /// What marks it: `.git`, `Cargo.toml`, `package.json` …
    pub markers: Vec<String>,
    pub git: bool,
    pub branch: Option<String>,
}

pub fn projects_of(text: &str) -> Result<Vec<Project>, String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let a = v(text);
    let list = a.get("projects").and_then(Value::as_array).ok_or("the companion's project list does not read")?;
    Ok(list
        .iter()
        .map(|p| Project {
            name: s(p, "name"),
            path: s(p, "path"),
            markers: strings(p.get("markers")),
            git: p.get("git").and_then(Value::as_bool).unwrap_or(false),
            branch: p.get("branch").and_then(Value::as_str).map(str::to_string),
        })
        .collect())
}

/// One entry of a folder (`GET /v1/files/tree`), folders first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub size: u64,
}

pub fn tree_of(text: &str) -> Result<Vec<Entry>, String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let a = v(text);
    let list = a.get("entries").and_then(Value::as_array).ok_or("the companion's folder listing does not read")?;
    Ok(list
        .iter()
        .map(|e| Entry {
            name: s(e, "name"),
            path: s(e, "path"),
            dir: e.get("dir").and_then(Value::as_bool).unwrap_or(false),
            size: e.get("size").and_then(Value::as_u64).unwrap_or(0),
        })
        .collect())
}

/// `POST /v1/git/status`'s answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    /// `(XY, path)` as `git status --porcelain` says them.
    pub files: Vec<(String, String)>,
}

impl GitStatus {
    pub fn from_json(text: &str) -> Result<GitStatus, String> {
        if let Some(e) = error_of(text) {
            return Err(e);
        }
        let a = v(text);
        if a.get("ok").and_then(Value::as_bool) == Some(false) {
            return Err(s(&a, "stderr"));
        }
        Ok(GitStatus {
            branch: s(&a, "branch"),
            upstream: a.get("upstream").and_then(Value::as_str).map(str::to_string),
            ahead: a.get("ahead").and_then(Value::as_u64).unwrap_or(0),
            behind: a.get("behind").and_then(Value::as_u64).unwrap_or(0),
            files: a
                .get("files")
                .and_then(Value::as_array)
                .map(|f| f.iter().map(|x| (s(x, "xy"), s(x, "path"))).collect())
                .unwrap_or_default(),
        })
    }

    /// One line: `main → origin/main, 1 ahead · 2 changed`.
    pub fn line(&self) -> String {
        let mut out = self.branch.clone();
        if let Some(u) = &self.upstream {
            out.push_str(&format!(" → {u}"));
        }
        match (self.ahead, self.behind) {
            (0, 0) => {}
            (a, 0) => out.push_str(&format!(", {a} ahead")),
            (0, b) => out.push_str(&format!(", {b} behind")),
            (a, b) => out.push_str(&format!(", {a} ahead, {b} behind")),
        }
        match self.files.len() {
            0 => out.push_str(" · clean"),
            n => out.push_str(&format!(" · {n} changed")),
        }
        out
    }
}

/// A git operation's answer as one line: what it printed, or why it failed.
pub fn git_line(text: &str) -> Result<String, String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let a = v(text);
    let out = format!("{}\n{}", s(&a, "stdout"), s(&a, "stderr")).trim().to_string();
    if a.get("ok").and_then(Value::as_bool) == Some(false) { Err(out) } else { Ok(out) }
}

/// The live artifacts that give an engine a project: its files, a
/// terminal in it, and a browser to look at what it serves, `(name, text)`.
pub fn project_artifacts(path: &str) -> [(String, String); 3] {
    let root = path.trim_end_matches('/');
    [
        ("files".into(), format!("kind: file\nroot: {root}\n")),
        ("terminal".into(), format!("kind: terminal\nroot: {root}\n")),
        ("browser".into(), "kind: browser\n".into()),
    ]
}

/// The name companion tools have on an engine: `companion.<tool>`.
pub const TOOLS_SERVER: &str = "companion";

/// The custom tools a companion offers (`tools` in `/v1/capabilities`,
/// or `/v1/tools`), as the outside tools engines get, the way MCP tools
/// reach them (`state::Engine::mcp_tools`): called `companion.<name>`.
pub fn tools_of(text: &str) -> Vec<crate::mcp::Tool> {
    let a = v(text);
    a.get("tools")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|t| {
                    Some(crate::mcp::Tool {
                        server: TOOLS_SERVER.to_string(),
                        name: t.get("name")?.as_str()?.to_string(),
                        description: s(t, "description"),
                        schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The companion tool a call names (`companion.<name>`), if any.
pub fn tool_for<'a>(tools: &'a [crate::mcp::Tool], call: &crate::calls::Call) -> Option<&'a crate::mcp::Tool> {
    tools.iter().find(|t| t.call_name() == call.name)
}

/// `POST /v1/tools/call`'s body for a call.
pub fn tool_call_body(tool: &crate::mcp::Tool, call: &crate::calls::Call) -> String {
    serde_json::json!({ "name": tool.name, "arguments": tool.arguments(call) }).to_string()
}

/// `POST /v1/tools/call`'s answer: the output, or why the tool failed.
pub fn tool_result(text: &str) -> Result<String, String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let a = v(text);
    let out = s(&a, "output");
    let code = a.get("exit").and_then(Value::as_i64);
    match a.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(out),
        _ => Err(format!("exit {}: {out}", code.map_or("none".to_string(), |c| c.to_string()))),
    }
}

/// A pairing the companion keeps (`GET /v1/pairings`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pairing {
    pub id: String,
    pub origin: String,
    pub created: i64,
    pub expires: i64,
    pub current: bool,
}

pub fn pairings_of(text: &str) -> Result<Vec<Pairing>, String> {
    if let Some(e) = error_of(text) {
        return Err(e);
    }
    let a = v(text);
    let list = a.get("pairings").and_then(Value::as_array).ok_or("the companion's pairings do not read")?;
    let n = |p: &Value, k: &str| p.get(k).and_then(Value::as_i64).unwrap_or(0);
    Ok(list
        .iter()
        .map(|p| Pairing {
            id: s(p, "id"),
            origin: s(p, "origin"),
            created: n(p, "created"),
            expires: n(p, "expires"),
            current: p.get("current").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect())
}

/// A pairing this page keeps (in the tab's sessionStorage): which
/// companion, its token, and when the token ends (unix seconds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub url: String,
    pub token: String,
    pub expires: i64,
}

impl Kept {
    pub fn to_json(&self) -> String {
        serde_json::json!({ "url": self.url, "token": self.token, "expires": self.expires }).to_string()
    }

    /// The pairing kept, if it reads and has not ended by `now`.
    pub fn from_json(text: &str, now: i64) -> Option<Kept> {
        let a = v(text);
        let k = Kept { url: s(&a, "url"), token: s(&a, "token"), expires: a.get("expires").and_then(Value::as_i64)? };
        (!k.url.is_empty() && !k.token.is_empty() && k.expires > now).then_some(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_roots_and_keys() {
        assert_eq!(clean_url(" 127.0.0.1:7790/ "), "http://127.0.0.1:7790");
        assert_eq!(clean_url("http://localhost:7777/v1"), "http://localhost:7777");
        assert_eq!(clean_url(""), DEFAULT_URL);
        assert_eq!(inference_base("127.0.0.1:7777"), "http://127.0.0.1:7777/v1");
        let job = Job { engine: "e1", artifact: "terminal", session: 3, root: "/p", command: "ls" };
        assert_eq!(session_key(&job), "e1/terminal/3");
        assert_eq!(resolve_root(Some("/x/app/"), "."), Ok("/x/app".into()));
        assert_eq!(resolve_root(Some("/x/app"), "./src/"), Ok("/x/app/src".into()));
        assert_eq!(resolve_root(None, "/abs/p/"), Ok("/abs/p".into()));
        assert!(resolve_root(None, ".").is_err());
        assert_eq!(join("/x/app/", "src/main.rs"), "/x/app/src/main.rs");
        let [files, term, browser] = project_artifacts("/x/app");
        assert_eq!(crate::live::Live::of(&browser.1), Some(crate::live::Live::Browser));
        assert_eq!(crate::live::Live::of(&files.1), Some(crate::live::Live::File));
        assert_eq!(crate::terminal::State::parse(&term.1).map(|s| s.root), Ok("/x/app".to_string()));
    }

    #[test]
    fn files_and_sessions_read_as_the_cores_types() {
        assert_eq!(content_of(r#"{"kind":"text","text":"hi\n","bytes":3}"#), Content::Text("hi\n".into()));
        assert_eq!(content_of(r#"{"kind":"binary","bytes":9,"image":true}"#), Content::Binary { bytes: 9, image: true });
        assert_eq!(content_of(r#"{"kind":"too_large","bytes":2000000}"#), Content::TooLarge(2_000_000));
        assert_eq!(content_of(r#"{"kind":"missing","why":"outside"}"#), Content::Missing("outside".into()));
        assert_eq!(content_of(r#"{"error":"not paired"}"#), Content::Missing("not paired".into()));
        assert_eq!(proc_of(r#"{"status":"running","exit":null}"#), Proc::Running);
        assert_eq!(proc_of(r#"{"status":"exited","exit":2}"#), Proc::Ended(Some(2)));
        assert_eq!(proc_of(r#"{"error":"no such session"}"#), Proc::Ended(None));
        assert_eq!(run_of(r#"{"status":"exited","exit":0,"output":"ok\n"}"#), Ok((Some(0), "ok\n".into())));
        assert_eq!(run_of(r#"{"status":"exited","exit":null,"timed_out":true,"output":""}"#).map(|r| r.0), Ok(None));
        assert_eq!(run_of(r#"{"error":"not inside a project"}"#), Err("not inside a project".into()));
    }

    #[test]
    fn capabilities_projects_trees_and_git_read() {
        let caps = Capabilities::from_json(
            r#"{"version":"0.1.0","protocol":1,"platform":"darwin arm64","roots":["/r"],"runtimes":{"git":{"path":"/usr/bin/git","version":"git version 2.50"},"mvn":null,"node":{"path":"/n","version":"v22"}}}"#,
        )
        .unwrap();
        assert!(caps.compatible() && caps.has("git") && !caps.has("mvn"));
        assert_eq!(caps.runtimes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["git", "node", "mvn"], "found ones first");
        assert!(Capabilities::from_json(r#"{"error":"not paired"}"#).is_err());

        let p = projects_of(r#"{"projects":[{"name":"app","path":"/r/app","markers":[".git","Cargo.toml"],"git":true,"branch":"main"}]}"#).unwrap();
        assert_eq!((p[0].name.as_str(), p[0].git, p[0].branch.as_deref()), ("app", true, Some("main")));
        let t = tree_of(r#"{"entries":[{"name":"src","path":"/r/app/src","dir":true,"size":0}]}"#).unwrap();
        assert!(t[0].dir);

        let g = GitStatus::from_json(r#"{"ok":true,"branch":"main","upstream":"origin/main","ahead":1,"behind":0,"files":[{"xy":" M","path":"a.txt"}]}"#).unwrap();
        assert_eq!(g.line(), "main → origin/main, 1 ahead · 1 changed");
        assert_eq!(git_line(r#"{"ok":false,"stdout":"","stderr":"fatal: no"}"#), Err("fatal: no".into()));
        assert_eq!(git_line(r#"{"error":"push needs a person's confirmation"}"#), Err("push needs a person's confirmation".into()));
    }

    #[test]
    fn custom_tools_reach_engines_as_outside_tools() {
        let caps = r#"{"tools":[{"name":"hello-app.greet","description":"Say hello.\nMore.","inputSchema":{"type":"object","properties":{"who":{"type":"string","description":"whom"},"times":{"type":"integer"}},"required":["who"]}}]}"#;
        let tools = tools_of(caps);
        assert_eq!(tools[0].call_name(), "companion.hello-app.greet");
        let line = tools[0].describe();
        assert!(line.starts_with("- companion.hello-app.greet(") && line.contains("who=\"...\"") && line.contains(": Say hello. ("), "{line}");
        assert!(line.contains("times: integer") && line.contains("who: string, whom"), "{line}");
        let call = crate::calls::parse("companion.hello-app.greet(who=\"Ada; touch /tmp/pwned\", times=\"2\")").unwrap().remove(0);
        let t = tool_for(&tools, &call).unwrap();
        let body: Value = serde_json::from_str(&tool_call_body(t, &call)).unwrap();
        assert_eq!(body["name"], "hello-app.greet");
        assert_eq!(body["arguments"]["who"], "Ada; touch /tmp/pwned", "a value stays one value");
        assert_eq!(body["arguments"]["times"], 2);
        assert_eq!(tool_result(r#"{"ok":true,"exit":0,"output":"hello"}"#), Ok("hello".into()));
        assert_eq!(tool_result(r#"{"ok":false,"exit":2,"output":"bad"}"#), Err("exit 2: bad".into()));
        assert!(tool_for(&tools, &crate::calls::parse("companion.nope()").unwrap()[0]).is_none());
    }

    #[test]
    fn a_kept_pairing_ends_when_it_expires() {
        let k = Kept { url: "http://127.0.0.1:7777".into(), token: "t".into(), expires: 100 };
        assert_eq!(Kept::from_json(&k.to_json(), 99), Some(k.clone()));
        assert_eq!(Kept::from_json(&k.to_json(), 100), None, "expired");
        assert_eq!(Kept::from_json("{}", 0), None);
        let p = pairings_of(r#"{"pairings":[{"id":"p1","origin":"http://localhost:7777","created":1,"expires":2,"current":true}]}"#).unwrap();
        assert!(p[0].current && p[0].id == "p1");
    }
}
