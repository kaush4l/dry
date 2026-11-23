//! MCP (Model Context Protocol): servers that give an engine more tools.
//!
//! An engine's spec may hold an `mcp.json`, in the shape other apps use:
//!
//! ```json
//! { "mcpServers": { "files": { "command": "npx", "args": ["-y", "@x/server-files", "."], "env": {} } } }
//! ```
//!
//! When the engine starts, the shell starts each server (JSON-RPC over its
//! stdin/stdout), asks it for its tools, and the engine gets them as
//! `server.tool` (so two servers' tools never clash). Pure: this reads the
//! config, builds the messages and reads the answers; the shell does the
//! process and the pipes (`dry_tools::mcp`).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::calls::Call;

/// The protocol version this client speaks.
pub const PROTOCOL: &str = "2025-06-18";

/// How to start one server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Server {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// Read `mcp.json`: each server by name. Empty text: no servers.
pub fn parse_config(json: &str) -> Result<BTreeMap<String, Server>, String> {
    if json.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let v: Value = serde_json::from_str(json).map_err(|e| format!("mcp.json: {e}"))?;
    let servers = v.get("mcpServers").and_then(Value::as_object).ok_or("mcp.json: no \"mcpServers\" object")?;
    servers
        .iter()
        .map(|(name, s)| {
            let command = s.get("command").and_then(Value::as_str).ok_or(format!("mcp.json: {name} has no \"command\""))?;
            let strings = |v: Option<&Value>| -> Vec<String> {
                v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
            };
            let env = s
                .get("env")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            Ok((name.clone(), Server { command: command.to_string(), args: strings(s.get("args")), env }))
        })
        .collect()
}

/// One tool a server offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub server: String,
    /// Its name on the server.
    pub name: String,
    pub description: String,
    /// Its input as JSON Schema (`properties`, `required`).
    pub schema: Value,
}

impl Tool {
    /// The name the engine calls it by: `server.tool`.
    pub fn call_name(&self) -> String {
        format!("{}.{}", self.server, self.name)
    }

    /// One line for the prompt, like the built-in tools'.
    pub fn describe(&self) -> String {
        let props = self.schema.get("properties").and_then(Value::as_object);
        let args: Vec<String> = props.map(|p| p.keys().map(|k| format!("{k}=\"...\"")).collect()).unwrap_or_default();
        let what: Vec<String> = props
            .map(|p| {
                p.iter()
                    .map(|(k, v)| {
                        let kind = v.get("type").and_then(Value::as_str).unwrap_or("text");
                        let d = v.get("description").and_then(Value::as_str).unwrap_or("");
                        if d.is_empty() { format!("{k}: {kind}") } else { format!("{k}: {kind}, {d}") }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let what = if what.is_empty() { String::new() } else { format!(" ({})", what.join("; ")) };
        let first_line = self.description.lines().next().unwrap_or("").trim();
        format!("- {}({}): {first_line}{what}", self.call_name(), args.join(", "))
    }

    /// A call's arguments as the JSON the tool wants: each value read as
    /// its schema type (`2` for a number, `true` for a boolean, JSON for an
    /// object or array), text otherwise.
    pub fn arguments(&self, call: &Call) -> Value {
        let props = self.schema.get("properties");
        let typed = |key: &str, value: &str| -> Value {
            let kind = props.and_then(|p| p.get(key)).and_then(|p| p.get("type")).and_then(Value::as_str).unwrap_or("string");
            match kind {
                "number" | "integer" | "boolean" | "object" | "array" => serde_json::from_str(value).unwrap_or_else(|_| json!(value)),
                _ => json!(value),
            }
        };
        Value::Object(call.args.iter().map(|(k, v)| (k.clone(), typed(k, v))).collect())
    }
}

/// A JSON-RPC request, one line.
pub fn request(id: u64, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

/// A JSON-RPC notification (no answer expected), one line.
pub fn notification(method: &str) -> String {
    json!({ "jsonrpc": "2.0", "method": method }).to_string()
}

/// The `initialize` request's params.
pub fn initialize_params() -> Value {
    json!({ "protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": { "name": "dry-learn", "version": "0.1.0" } })
}

/// The `result` of an answer, or its error's message.
pub fn result(answer: &Value) -> Result<&Value, String> {
    if let Some(e) = answer.get("error") {
        return Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string());
    }
    answer.get("result").ok_or_else(|| "an answer with no result".to_string())
}

/// The tools in a `tools/list` result.
pub fn parse_tools(server: &str, result: &Value) -> Vec<Tool> {
    let tools = result.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
    tools
        .iter()
        .filter_map(|t| {
            Some(Tool {
                server: server.to_string(),
                name: t.get("name")?.as_str()?.to_string(),
                description: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                schema: t.get("inputSchema").cloned().unwrap_or(json!({})),
            })
        })
        .collect()
}

/// A `tools/call` result as text: its text parts; `isError` makes it an error.
pub fn call_text(result: &Value) -> Result<String, String> {
    let parts = result.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    let text: Vec<String> = parts
        .iter()
        .map(|p| match p.get("text").and_then(Value::as_str) {
            Some(t) => t.to_string(),
            None => format!("({} content)", p.get("type").and_then(Value::as_str).unwrap_or("other")),
        })
        .collect();
    let text = text.join("\n");
    if result.get("isError").and_then(Value::as_bool).unwrap_or(false) { Err(text) } else { Ok(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calls;

    #[test]
    fn config_tools_and_typed_arguments() {
        let servers = parse_config(r#"{"mcpServers":{"calc":{"command":"python3","args":["calc.py"],"env":{"A":"1"}}}}"#).unwrap();
        assert_eq!(servers["calc"], Server { command: "python3".into(), args: vec!["calc.py".into()], env: [("A".into(), "1".into())].into() });
        assert!(parse_config("").unwrap().is_empty());
        assert!(parse_config("{}").is_err());

        let list = json!({"tools": [{"name": "add", "description": "Add two numbers.\nMore.",
            "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number", "description": "second"}}}}]});
        let tools = parse_tools("calc", &list);
        assert_eq!(tools[0].call_name(), "calc.add");
        assert_eq!(tools[0].describe(), "- calc.add(a=\"...\", b=\"...\"): Add two numbers. (a: number; b: number, second)");
        let call = calls::parse("calc.add(a=2, b=\"3.5\")").unwrap().remove(0);
        assert_eq!(tools[0].arguments(&call), json!({"a": 2, "b": 3.5}));
    }

    #[test]
    fn answers_and_results() {
        let ok = json!({"jsonrpc": "2.0", "id": 1, "result": {"content": [{"type": "text", "text": "5"}]}});
        assert_eq!(call_text(result(&ok).unwrap()), Ok("5".into()));
        let failed = json!({"result": {"content": [{"type": "text", "text": "bad"}], "isError": true}});
        assert_eq!(call_text(result(&failed).unwrap()), Err("bad".into()));
        assert_eq!(result(&json!({"error": {"message": "no such tool"}})), Err("no such tool".into()));
        assert!(request(7, "tools/list", json!({})).contains("\"id\":7"));
    }
}
