//! Workers: engines run by an outside program (a Claude Code session),
//! wired into dry (docs/sessions.md). dry owns the context; the program
//! is told and acts through three seams:
//!
//! - **Push (hooks).** Each session gets a settings file of its own,
//!   written from `sessions.json`'s `settings` beside the store
//!   (`<store>.sessions/<engine>/settings.json`; the owner's own
//!   `~/.claude` is never touched). Its hooks run `dry snapshot`: at
//!   SessionStart who it is, its goal, participants and tools; at
//!   UserPromptSubmit the live snapshot and the posts for it since its
//!   cursor (`<store>.sessions/<engine>/cursor`).
//! - **Pull (MCP).** `dry mcp engines` serves the `engines` group
//!   (`tools`): `state`, `artifact`, `thread` read (`dashboard`, its old
//!   name, still answers); `post`,
//!   `engine_start`, `engine_call` write, and only through the host's API
//!   (the host is the one writer): with no host running a write is
//!   refused with one line, and reads go to the store, read-only.
//! - **Approvals.** `approve` is the program's permission prompt tool: the
//!   engine's rules first, else an approval record on the host that a
//!   person allows or denies.
//!
//! The engine is the one `DRY_ENGINE` names (`sessions.json`'s `env`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use dry_core::snapshot::{Cursor, Read};
use serde_json::{Value, json};

use crate::Result;
use crate::app_tools::AppTool;
use crate::host::Lock;

/// The folder beside the store for its sessions' own files
/// (`/…/state` → `/…/state.sessions`).
pub fn sessions_dir() -> Result<PathBuf> {
    let spec = crate::store::open_default()?.describe();
    let lock = crate::host::lock_path(&spec);
    let name = lock.file_name().map(|n| n.to_string_lossy().trim_end_matches(".host.json").to_string()).unwrap_or_else(|| "state".into());
    Ok(lock.with_file_name(format!("{name}.sessions")))
}

/// One session's folder.
pub fn session_dir(engine: &str) -> Result<PathBuf> {
    Ok(sessions_dir()?.join(engine))
}

/// The `dry` command the hooks run: `DRY_BIN`, else this checkout's build.
pub fn dry_bin() -> PathBuf {
    std::env::var("DRY_BIN").map(PathBuf::from).unwrap_or_else(|_| crate::root().join("target/debug/dry"))
}

/// A session turn, ready to run: the command line and its environment.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The command for engine `engine`'s next turn with program `tool`: its
/// settings file written first (when the program takes one).
pub fn prepare(tool: &dry_core::session::Tool, engine: &str, prompt: &str, id: Option<&str>, meta_file: &str) -> Result<Prepared> {
    prepare_in(&session_dir(engine)?, tool, engine, prompt, id, meta_file)
}

/// `prepare` with the session's folder given.
pub fn prepare_in(
    dir: &std::path::Path,
    tool: &dry_core::session::Tool,
    engine: &str,
    prompt: &str,
    id: Option<&str>,
    meta_file: &str,
) -> Result<Prepared> {
    let mut vars: BTreeMap<&str, String> = BTreeMap::from([
        ("engine", engine.to_string()),
        ("dry", dry_bin().display().to_string()),
        ("cursor", dir.join("cursor").display().to_string()),
        ("meta_file", meta_file.to_string()),
    ]);
    if let Some(text) = tool.settings_text(&vars) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let file = dir.join("settings.json");
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        vars.insert("settings", file.display().to_string());
    }
    Ok(Prepared { argv: tool.command_with(prompt, id, &vars), env: tool.env_with(&vars) })
}

/// The engine this program works for (`DRY_ENGINE`).
pub fn engine_env() -> Result<String> {
    std::env::var("DRY_ENGINE")
        .ok()
        .filter(|e| !e.trim().is_empty())
        .ok_or_else(|| "DRY_ENGINE is not set: these tools serve a session engine dry started (its sessions.json env)".into())
}

/// The live host, or why a write cannot be made.
fn writer(what: &str) -> Result<Lock> {
    crate::host::current().ok_or_else(|| {
        format!("not done: no dry host is running, and only the host writes the store ({what} needs dry-server); reads still work")
    })
}

fn engine_path(engine: &str, rest: &str) -> String {
    format!("/api/engines/{}/{rest}", crate::host::escape(engine))
}

/// Read `what` for `engine`: through the host when one runs (live
/// activity), else from the store as it is on disk.
pub fn read(engine: &str, what: &Read) -> Result<(String, Option<Cursor>)> {
    if let Some(lock) = crate::host::current() {
        let (w, name, which, since) = match what {
            Read::Start => ("start", String::new(), String::new(), String::new()),
            Read::Turn(c) => ("turn", String::new(), String::new(), c.text()),
            Read::Artifact(n) => ("artifact", n.clone(), String::new(), String::new()),
            Read::Thread { which, since } => ("thread", String::new(), which.clone(), since.text()),
        };
        let e = crate::host::escape;
        let path = engine_path(engine, &format!("read?what={w}&name={}&which={}&since={}", e(&name), e(&which), e(&since)));
        let v = crate::host::call_ok(&lock, &path, None, Duration::from_secs(10))?;
        let text = v.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
        return Ok((text, v.get("cursor").and_then(Value::as_str).map(Cursor::parse)));
    }
    let state = crate::store::open_default()?.load()?;
    let idle = |_: usize| dry_core::engines::Seen { label: "idle (no host is running)".into(), ..Default::default() };
    dry_core::snapshot::read(&state, engine, what, crate::live::clock(), &idle)
}

/// `dry snapshot --engine <id> [--since <cursor>] [--cursor <file>] [--start]`:
/// the text a hook adds to the turn. With `--cursor` the cursor is read
/// from that file and the new one written back (a hook's memory).
pub fn snapshot_command(args: &[&str]) -> Result<String> {
    let mut engine = None;
    let mut since = None;
    let mut cursor_file = None;
    let mut start = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--engine" => engine = it.next().map(|s| s.to_string()),
            "--since" => since = it.next().map(|s| Cursor::parse(s)),
            "--cursor" => cursor_file = it.next().map(PathBuf::from),
            "--start" => start = true,
            other => {
                return Err(format!("dry snapshot: unknown argument {other}; --engine <id> [--since <cursor>] [--cursor <file>] [--start]"));
            }
        }
    }
    let engine = match engine {
        Some(e) => e,
        None => engine_env()?,
    };
    if start {
        return read(&engine, &Read::Start).map(|(t, _)| t);
    }
    let since = since.unwrap_or_else(|| {
        cursor_file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).map_or_else(Cursor::default, |t| Cursor::parse(&t))
    });
    let (text, after) = read(&engine, &Read::Turn(since))?;
    if let (Some(f), Some(after)) = (&cursor_file, after) {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(f, after.text()).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    Ok(text)
}

fn arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn need(args: &Value, key: &str) -> Result<String> {
    arg(args, key).ok_or_else(|| format!("needs {key} (text)"))
}

// ---- the `engines` MCP group ---------------------------------------------------------

fn t_state(args: &Value) -> Result<String> {
    let since = Cursor::parse(&arg(args, "since").unwrap_or_default());
    read(&engine_env()?, &Read::Turn(since)).map(|(t, _)| t)
}

fn t_artifact(args: &Value) -> Result<String> {
    read(&engine_env()?, &Read::Artifact(need(args, "name")?)).map(|(t, _)| t)
}

fn t_thread(args: &Value) -> Result<String> {
    let which = arg(args, "read").unwrap_or_else(|| "thread".into());
    let since = Cursor::parse(&arg(args, "since").unwrap_or_default());
    read(&engine_env()?, &Read::Thread { which, since }).map(|(t, _)| t)
}

fn t_post(args: &Value) -> Result<String> {
    let engine = engine_env()?;
    let lock = writer("post")?;
    let v = crate::host::call_ok(
        &lock,
        &engine_path(&engine, "posts"),
        Some(&json!({ "text": need(args, "text")?, "to": arg(args, "to") })),
        Duration::from_secs(10),
    )?;
    Ok(v.get("line").and_then(Value::as_str).unwrap_or("posted").to_string())
}

fn t_engine_start(args: &Value) -> Result<String> {
    let engine = engine_env()?;
    let lock = writer("engine_start")?;
    let body =
        json!({ "goal": need(args, "goal")?, "template": arg(args, "template"), "name": arg(args, "name"), "text": arg(args, "text"), "model": arg(args, "model") });
    let v = crate::host::call_ok(&lock, &engine_path(&engine, "children"), Some(&body), Duration::from_secs(10))?;
    Ok(v.get("line").and_then(Value::as_str).unwrap_or("started").to_string())
}

fn t_engine_call(args: &Value) -> Result<String> {
    let engine = engine_env()?;
    let lock = writer("engine_call")?;
    let child = need(args, "id")?;
    let v = crate::host::call_ok(
        &lock,
        &engine_path(&engine, "children"),
        Some(&json!({ "child": child, "text": need(args, "text")? })),
        Duration::from_secs(10),
    )?;
    let after = v.get("replies").and_then(Value::as_u64).unwrap_or(0);
    // Its reply, however long it takes (no step or time limit).
    let path = engine_path(&engine, &format!("children/{}/take", crate::host::escape(&child)));
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let lock = crate::host::current().ok_or("the host stopped while the child worked; its reply is in the store")?;
        let v = crate::host::call_ok(&lock, &path, Some(&json!({ "after": after })), Duration::from_secs(10))?;
        if let Some(r) = v.get("reply").and_then(Value::as_str) {
            return Ok(format!("{child} replied: {r}"));
        }
        if let Some(why) = v.get("failed").and_then(Value::as_str) {
            return Err(format!("{child} failed: {why}"));
        }
    }
}

/// The permission prompt tool: `{tool_name, input}` in, the answer as
/// JSON text out (`dry_core::approval::answer`).
pub fn approve(args: &Value) -> Result<String> {
    use dry_core::approval::{Decided, Decision};
    let engine = engine_env()?;
    let tool = arg(args, "tool_name").unwrap_or_default();
    let input = args.get("input").cloned().unwrap_or(json!({}));
    let Some(lock) = crate::host::current() else {
        // No host: the rules still allow; nothing else can.
        let state = crate::store::open_default()?.load()?;
        let rules = state.engines.iter().find(|e| e.id == engine).map(|e| dry_core::approval::parse_rules(&e.allow)).unwrap_or_default();
        let d = match dry_core::approval::allowed_by(&rules, &tool, &input) {
            Some(r) => Decided { decision: Decision::Allow, reason: String::new(), by: format!("rule {}", r.text()) },
            None => Decided {
                decision: Decision::Deny,
                reason: format!(
                    "no dry host is running, so nobody can approve {tool}; start dry-server, or add it to {engine}'s allow rules"
                ),
                by: "no host".into(),
            },
        };
        return Ok(dry_core::approval::answer(&d, &input).to_string());
    };
    let mut v = crate::host::call_ok(
        &lock,
        "/api/approvals",
        Some(&json!({ "engine": engine, "tool": tool, "input": input })),
        Duration::from_secs(10),
    )?;
    let id = v.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    loop {
        if let Some(a) = dry_core::approval::Approval::from_json(&v)
            && let Some(d) = &a.decided
        {
            return Ok(dry_core::approval::answer(d, &input).to_string());
        }
        std::thread::sleep(Duration::from_secs(1));
        let Some(lock) = crate::host::current() else {
            let d =
                Decided { decision: Decision::Deny, reason: "the dry host stopped before anyone answered".into(), by: "no host".into() };
            return Ok(dry_core::approval::answer(&d, &input).to_string());
        };
        v = match crate::host::call(&lock, &format!("/api/approvals/{}", crate::host::escape(&id)), None, Duration::from_secs(10))? {
            (200, v) => v,
            (_, v) => {
                let why = v.get("error").and_then(Value::as_str).unwrap_or("the host has no such approval (it restarted?)").to_string();
                return Ok(
                    dry_core::approval::answer(&Decided { decision: Decision::Deny, reason: why, by: "host".into() }, &input).to_string()
                );
            }
        };
    }
}

fn props(p: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": p, "required": required })
}

/// The `engines` group (`dry mcp engines`): few tools, for a worker.
pub fn tools() -> Vec<AppTool> {
    vec![
        AppTool {
            name: "state",
            description: "Your dry snapshot now: goal, phase, your artifacts (short), and your thread's posts for you since `since` (a cursor like 7; none: all). It supersedes earlier snapshots.",
            schema: || props(json!({ "since": { "type": "string", "description": "a cursor from an earlier snapshot" } }), &[]),
            run: t_state,
        },
        AppTool {
            name: "artifact",
            description: "One of your artifacts in full (a kind such as engines or shared: its live snapshot).",
            schema: || props(json!({ "name": { "type": "string" } }), &["name"]),
            run: t_artifact,
        },
        AppTool {
            name: "thread",
            description: "Your thread's posts: one conversation shared by the user and every engine of your tree. read=thread (default, all of it) or inbox (only the posts for you); since: a cursor like 7.",
            schema: || {
                props(json!({ "read": { "type": "string", "enum": ["thread", "inbox"] }, "since": { "type": "string" } }), &[])
            },
            run: t_thread,
        },
        AppTool {
            name: "post",
            description: "Post on your thread. to: the engine it is for (any engine of your tree: its turn starts with your post) or `you` for the user. Write it so the addressee can act on it cold: what, why, what done looks like, where the material is. A post with no to reaches nobody and comes back to you.",
            schema: || {
                props(
                    json!({ "text": { "type": "string", "description": "at most 600 characters" },
                            "to": { "type": "string", "description": "an engine id on your thread, or you" } }),
                    &["text", "to"],
                )
            },
            run: t_post,
        },
        AppTool {
            name: "engine_start",
            description: "Start a child engine: a real dry engine (on the default model, or the subscription model= names) with this goal, shown in the app, on your thread. It works on its own; when it replies, a message comes to you. Returns at once.",
            schema: || {
                props(
                    json!({ "goal": { "type": "string" }, "template": { "type": "string", "description": "optional: its template.md (layout and phases)" },
                            "name": { "type": "string", "description": "optional: a short name (else from the goal)" },
                            "text": { "type": "string", "description": "optional: its first message (else: work on your goal)" },
                            "model": { "type": "string", "description": "optional: who runs it, a present row of the subscriptions (a model, or a session program); else the default model" } }),
                    &["goal"],
                )
            },
            run: t_engine_start,
        },
        AppTool {
            name: "engine_call",
            description: "Give one of your child engines a message and wait for its reply (the result).",
            schema: || {
                props(json!({ "id": { "type": "string", "description": "the child's id" }, "text": { "type": "string" } }), &["id", "text"])
            },
            run: t_engine_call,
        },
        AppTool {
            name: "approve",
            description: "The permission prompt: dry decides whether a tool may run (your rules, else a person). Called by Claude Code itself, not by you.",
            schema: || {
                props(
                    json!({ "tool_name": { "type": "string" }, "input": { "type": "object" }, "tool_use_id": { "type": "string" } }),
                    &["tool_name", "input"],
                )
            },
            run: approve,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_group_is_few_tools_with_object_schemas() {
        let names: Vec<&str> = tools().iter().map(|t| t.name).collect();
        assert_eq!(names, ["state", "artifact", "thread", "post", "engine_start", "engine_call", "approve"]);
        for t in tools() {
            assert_eq!((t.schema)()["type"], "object", "{}", t.name);
        }
    }

    #[test]
    fn a_session_turn_gets_its_own_settings_file_and_engine() {
        let tool = dry_core::session::Sessions::from_json(include_str!("../../../sessions.json")).unwrap().get("claude").cloned().unwrap();
        let dir = std::env::temp_dir().join(format!("dry-worker-{}", std::process::id()));
        let p = prepare_in(&dir.join("claude-9"), &tool, "claude-9", "hi", None, "").unwrap();
        let settings = dir.join("claude-9/settings.json");
        let i = p.argv.iter().position(|a| a == "--settings").unwrap();
        assert_eq!(p.argv[i + 1], settings.display().to_string());
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.contains("snapshot --engine 'claude-9' --cursor") && text.contains("claude-9/cursor"), "{text}");
        assert_eq!(p.env, [("DRY_ENGINE".to_string(), "claude-9".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
