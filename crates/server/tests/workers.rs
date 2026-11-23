//! A worker's MCP tools against the real host (docs/sessions.md): the
//! `engines` group (`dry_tools::worker`) run in this process as a session
//! engine (`DRY_ENGINE`), the `dry-server` binary as the host of a scratch
//! store. Writes go only through the host: refused while none runs, taken
//! once it does. No session program runs here, and no model: the scratch
//! default model points at a closed port.

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dry_tools::store::{MdStore, StateStore as Store};
use serde_json::{Value, json};

struct Server {
    child: Child,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn tool(name: &str, args: Value) -> Result<String, String> {
    let t = dry_tools::worker::tools().into_iter().find(|t| t.name == name).unwrap();
    (t.run)(&args)
}

#[test]
fn a_worker_reads_without_a_host_and_writes_only_through_one() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("dry-workers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["routes", "components", "themes", "scenes", "assets/fonts"] {
        copy_dir(&repo.join(dir), &root.join(dir));
    }
    for file in ["sessions.json", "responses.md"] {
        std::fs::copy(repo.join(file), root.join(file)).unwrap();
    }
    std::fs::write(
        root.join("models.json"),
        r#"{"default": "dead", "models": [{"name": "dead", "id": "openai/c/none", "base_url": "http://127.0.0.1:9/v1"}]}"#,
    )
    .unwrap();
    let mut state = dry_core::files::sample();
    let w = state.add_session("claude", "Find the failing test.");
    let me = state.engines[w].id.clone();
    MdStore { root: root.join("state") }.save(&state).unwrap();
    // SAFETY: this test binary has this one test; nothing else reads them.
    unsafe {
        std::env::set_var("DRY_ROOT", &root);
        std::env::set_var("DRY_ENGINE", &me);
        std::env::remove_var("DRY_STORE");
    }

    // No host: reads come from the store, writes are refused in one line.
    let snap = tool("state", json!({})).unwrap();
    assert!(snap.contains(&format!("dry snapshot of {me}")) && snap.contains("goal: Find the failing test."), "{snap}");
    let refused = tool("post", json!({ "text": "hello" })).unwrap_err();
    assert!(refused.starts_with("not done: no dry host is running"), "{refused}");
    assert!(!refused.contains('\n'));
    assert!(tool("engine_start", json!({ "goal": "x" })).unwrap_err().contains("only the host writes"));
    let no_host: Value =
        serde_json::from_str(&tool("approve", json!({ "tool_name": "Bash", "input": { "command": "ls" } })).unwrap()).unwrap();
    assert_eq!(no_host["behavior"], "deny");
    let read_ok: Value =
        serde_json::from_str(&tool("approve", json!({ "tool_name": "Grep", "input": { "pattern": "x" } })).unwrap()).unwrap();
    assert_eq!(read_ok["behavior"], "allow", "the rules still allow with no host");

    // The host.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let child = Command::new(env!("CARGO_BIN_EXE_dry-server"))
        .args(["--port", &port.to_string()])
        .env("DRY_ROOT", &root)
        .env_remove("DRY_STORE")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server { child, root: root.clone() };
    let until = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() || dry_tools::host::current().is_none() {
        assert!(Instant::now() < until, "the host did not start");
        std::thread::sleep(Duration::from_millis(50));
    }

    let line = tool("post", json!({ "text": "on it", "to": "you" })).unwrap();
    assert!(line.starts_with("posted entry ") && line.ends_with("on your thread"), "{line}");
    let line = tool("engine_start", json!({ "goal": "Count the tests in turn.rs.", "name": "counter" })).unwrap();
    assert_eq!(line, "made counter with your goal and started it; a message comes when it replies");
    // The child is a real engine of the store, a child of the session, on
    // the default model; with no model to answer, its turn fails.
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let saved = MdStore { root: root.join("state") }.load().unwrap();
        if let Some(c) = saved.engines.iter().find(|e| e.id == "counter") {
            assert_eq!(c.parent.as_deref(), Some(me.as_str()));
            assert_eq!(c.inference.base_url, "http://127.0.0.1:9/v1");
            assert!(c.description.contains("Count the tests"), "{}", c.description);
            break;
        }
        assert!(Instant::now() < until, "the child was not saved");
        std::thread::sleep(Duration::from_millis(100));
    }
    let err = tool("engine_call", json!({ "id": "counter", "text": "again" })).unwrap_err();
    assert!(err.starts_with("counter failed:") || err.contains("still working"), "{err}");
    let thread = tool("thread", json!({})).unwrap();
    assert!(thread.contains(&format!("{me} → you: on it")), "{thread}");
    assert_eq!(tool("thread", json!({ "read": "both" })).unwrap(), thread, "the old read words read the thread");
    assert!(tool("artifact", json!({ "name": "engines" })).unwrap().contains("#### counter"));

    // A rule allows at once; anything else waits for a person.
    let allowed: Value =
        serde_json::from_str(&tool("approve", json!({ "tool_name": "Read", "input": { "file_path": "/x" } })).unwrap()).unwrap();
    assert_eq!(allowed, json!({ "behavior": "allow", "updatedInput": { "file_path": "/x" } }));
    let waiting = std::thread::spawn(|| tool("approve", json!({ "tool_name": "Bash", "input": { "command": "rm -rf build" } })).unwrap());
    let lock = dry_tools::host::current().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let id = loop {
        let v = dry_tools::host::call_ok(&lock, "/api/approvals", None, Duration::from_secs(5)).unwrap();
        if let Some(a) = v["approvals"].as_array().unwrap().iter().find(|a| a["decision"].is_null()) {
            assert_eq!(a["what"], "Bash(rm -rf build)");
            break a["id"].as_str().unwrap().to_string();
        }
        assert!(Instant::now() < until, "no approval was asked");
        std::thread::sleep(Duration::from_millis(100));
    };
    let thread = MdStore { root: root.join("state") }.load().unwrap().shared.get(&me).cloned().unwrap_or_default();
    assert!(thread.contains(&format!("{me} → you: approval {id}: {me} asks to use Bash(rm -rf build)")), "you are told on its thread: {thread}");
    let page = dry_tools::host::call(&lock, &format!("/desk/{w}"), None, Duration::from_secs(10)).unwrap().1;
    let page = page["error"].as_str().unwrap_or_default().to_string();
    assert!(page.contains("data-act=\"approve\"") && page.contains("Bash(rm -rf build)"), "the engine's page shows it with Allow and Deny");
    let decided =
        dry_tools::host::call_ok(&lock, &format!("/api/approvals/{id}"), Some(&json!({ "decision": "allow" })), Duration::from_secs(5))
            .unwrap();
    assert_eq!(decided["by"], "a person");
    let answer: Value = serde_json::from_str(&waiting.join().unwrap()).unwrap();
    assert_eq!(answer, json!({ "behavior": "allow", "updatedInput": { "command": "rm -rf build" } }));
    let again = dry_tools::host::call(&lock, &format!("/api/approvals/{id}"), Some(&json!({ "decision": "deny" })), Duration::from_secs(5))
        .unwrap();
    assert_eq!(again.0, 409);

    // The hook's snapshot reads through the host and keeps its cursor.
    let cursor = root.join("cursor");
    let first = dry_tools::worker::snapshot_command(&["--engine", &me, "--cursor", cursor.to_str().unwrap()]).unwrap();
    assert!(first.contains("supersedes every earlier dry snapshot"), "{first}");
    let kept = std::fs::read_to_string(&cursor).unwrap();
    assert!(kept.parse::<usize>().is_ok_and(|n| n >= 2), "one count on the thread: {kept}");
    let start = dry_tools::worker::snapshot_command(&["--engine", &me, "--start"]).unwrap();
    assert!(start.contains("## participants") && start.contains("its other engines: counter"), "{start}");
}
