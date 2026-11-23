//! An MCP client over stdio: starts a server from an engine's `mcp.json`,
//! does the handshake, lists its tools and calls them. The messages and
//! their meaning are `dry_core::mcp`; this is the process and the pipes.
//! Blocking: run it off the thread that draws.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use dry_core::mcp::{self, Server, Tool};
use serde_json::{Value, json};

use crate::Result;

/// A running server.
pub struct Client {
    pub name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    /// Start `server` (in the repo root) and do the MCP handshake.
    pub fn start(name: &str, server: &Server) -> Result<Client> {
        let mut child = Command::new(&server.command)
            .args(&server.args)
            .envs(&server.env)
            .current_dir(crate::root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("mcp {name}: could not start `{}`: {e}", server.command))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut client = Client { name: name.to_string(), child, stdin, stdout, next_id: 1 };
        client.ask("initialize", mcp::initialize_params())?;
        client.send(&mcp::notification("notifications/initialized"))?;
        Ok(client)
    }

    fn send(&mut self, line: &str) -> Result {
        writeln!(self.stdin, "{line}").and_then(|_| self.stdin.flush()).map_err(|e| format!("mcp {}: {e}", self.name))
    }

    /// Send a request and wait for its answer (other lines, like the
    /// server's own notifications, are skipped).
    fn ask(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&mcp::request(id, method, params))?;
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).map_err(|e| format!("mcp {}: {e}", self.name))?;
            if n == 0 {
                return Err(format!("mcp {}: the server stopped", self.name));
            }
            let Ok(answer) = serde_json::from_str::<Value>(line.trim()) else { continue };
            if answer.get("id").and_then(Value::as_u64) == Some(id) {
                return mcp::result(&answer).cloned().map_err(|e| format!("mcp {} {method}: {e}", self.name));
            }
        }
    }

    pub fn list_tools(&mut self) -> Result<Vec<Tool>> {
        let result = self.ask("tools/list", json!({}))?;
        Ok(mcp::parse_tools(&self.name, &result))
    }

    /// Call `tool` with `arguments`; the text it returned, or why it failed.
    pub fn call(&mut self, tool: &str, arguments: Value) -> Result<String> {
        let result = self.ask("tools/call", json!({ "name": tool, "arguments": arguments }))?;
        mcp::call_text(&result)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start every server in `mcp.json` text: the running clients and all
/// their tools, or the first thing that went wrong.
pub fn start_all(mcp_json: &str) -> Result<(Vec<Client>, Vec<Tool>)> {
    let mut clients = Vec::new();
    let mut tools = Vec::new();
    for (name, server) in mcp::parse_config(mcp_json)? {
        let mut client = Client::start(&name, &server)?;
        tools.extend(client.list_tools()?);
        clients.push(client);
    }
    Ok((clients, tools))
}

/// An engine's running servers, shareable across threads. A client is one
/// stdin/stdout pair, so calls to one server go one at a time (its lock);
/// calls to different servers run at the same time.
pub struct Servers(Vec<(String, std::sync::Mutex<Client>)>);

impl Servers {
    pub fn new(clients: Vec<Client>) -> Servers {
        Servers(clients.into_iter().map(|c| (c.name.clone(), std::sync::Mutex::new(c))).collect())
    }

    /// Run `call` if one of `tools` is it (`server.tool`): what the server
    /// said. `None`: none of these servers has it.
    pub fn call(&self, tools: &[Tool], call: &dry_core::calls::Call) -> Option<std::result::Result<String, String>> {
        let tool = tools.iter().find(|t| t.call_name() == call.name)?;
        let (_, client) = self.0.iter().find(|(name, _)| *name == tool.server)?;
        let mut client = match client.lock() {
            Ok(c) => c,
            Err(_) => return Some(Err(format!("mcp {}: an earlier call failed badly", tool.server))),
        };
        Some(client.call(&tool.name, tool.arguments(call)))
    }
}

/// A tiny MCP server (python3, stdlib only) with `add` and `echo`, for
/// tests here and in the app.
pub const TEST_SERVER: &str = r#"
import json, sys
TOOLS = [
  {"name": "add", "description": "Add two numbers.", "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}, "required": ["a", "b"]}},
  {"name": "echo", "description": "Say text back.", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}},
]
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m: continue
    if m["method"] == "initialize": r = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "test", "version": "1"}}
    elif m["method"] == "tools/list": r = {"tools": TOOLS}
    elif m["method"] == "tools/call":
        a = m["params"]["arguments"]
        if m["params"]["name"] == "add": r = {"content": [{"type": "text", "text": str(a["a"] + a["b"])}]}
        else: r = {"content": [{"type": "text", "text": a.get("text", "")}]}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32601, "message": "no method"}}), flush=True); continue
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
"#;

/// Write `TEST_SERVER` to a temp file; the `mcp.json` that starts it as `calc`.
pub fn test_server_config() -> String {
    let script = std::env::temp_dir().join(format!("dry-mcp-test-{}.py", std::process::id()));
    let _ = std::fs::write(&script, TEST_SERVER);
    json!({"mcpServers": {"calc": {"command": "python3", "args": [script.display().to_string()]}}}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_server_starts_lists_and_calls() {
        let (mut clients, tools) = start_all(&test_server_config()).expect("server starts");
        assert_eq!(tools.iter().map(Tool::call_name).collect::<Vec<_>>(), ["calc.add", "calc.echo"]);
        let call = dry_core::calls::parse("calc.add(a=2, b=3)").unwrap().remove(0);
        assert_eq!(clients[0].call("add", tools[0].arguments(&call)), Ok("5".into()));
        assert_eq!(clients[0].call("echo", json!({"text": "hi"})), Ok("hi".into()));
    }

    #[test]
    fn calls_from_several_threads_take_turns_on_one_server() {
        let (clients, tools) = start_all(&test_server_config()).expect("server starts");
        let servers = Servers::new(clients);
        let answers: Vec<_> = std::thread::scope(|scope| {
            let calls: Vec<_> = (0..4)
                .map(|i| {
                    let (servers, tools) = (&servers, &tools);
                    scope.spawn(move || {
                        let call = dry_core::calls::parse(&format!("calc.add(a={i}, b=10)")).unwrap().remove(0);
                        servers.call(tools, &call)
                    })
                })
                .collect();
            calls.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert_eq!(answers, [Some(Ok("10".into())), Some(Ok("11".into())), Some(Ok("12".into())), Some(Ok("13".into()))]);
        let unknown = dry_core::calls::parse("other.tool()").unwrap().remove(0);
        assert_eq!(servers.call(&tools, &unknown), None);
    }
}
