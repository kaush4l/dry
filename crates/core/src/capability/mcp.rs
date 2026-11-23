//! The handshake over MCP (docs/capabilities.md, "The handshake: how another runtime joins"): how a companion, a
//! host or another device joins and registers what it provides. Pure:
//! the messages a client sends and how their answers read.
//!
//! ```text
//! pair (the companion's pairing code → a token)
//!   → initialize        params._meta.hello = {runtime, platform, version, protocol}
//!                       result._meta.hello = the provider's hello
//!   → tools/list        each tool's _meta: {capability, doc, state, approval}
//!   → resources/list    one resource per capability, `dry://capability/<id>`,
//!                       _meta: {capability, doc, state, why, interval, events}
//!   → registered        merged with the compiled ones, tagged remote:<peer>
//! live:
//!   GET /mcp (SSE)      notifications/resources/updated {uri, _meta.event?}
//!                         → resources/read: the snapshot (and the event fires)
//!                       notifications/tools/list_changed → list again: re-register
//!   ping                every few seconds: the heartbeat; no answer = gone
//!   tools/call          a routed call (`Registry::route`)
//! stream closed or heartbeat lost → its capabilities absent
//! ```

use serde_json::{Value, json};

use super::{Capability, Event, Input, Provider, Snapshot, State, Tool, Ty, When};

/// The protocol of this handshake (`_meta.hello.protocol`).
pub const PROTOCOL: &str = "dry-capabilities/1";
/// The MCP protocol version a client asks for.
pub const VERSION: &str = "2025-11-25";
/// A capability's resource URI.
pub const URI: &str = "dry://capability/";

/// A runtime's hello.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hello {
    pub runtime: String,
    pub platform: String,
    pub version: String,
    pub protocol: String,
}

impl Hello {
    pub fn to_json(&self) -> Value {
        json!({ "runtime": self.runtime, "platform": self.platform, "version": self.version, "protocol": self.protocol })
    }

    pub fn of(v: &Value) -> Option<Hello> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        v.is_object().then(|| Hello { runtime: s("runtime"), platform: s("platform"), version: s("version"), protocol: s("protocol") })
    }
}

/// A JSON-RPC request.
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

/// The `initialize` a client sends, with its hello.
pub fn initialize(id: u64, me: &Hello) -> Value {
    request(
        id,
        "initialize",
        json!({
            "protocolVersion": VERSION,
            "capabilities": {},
            "clientInfo": { "name": me.runtime, "version": me.version },
            "_meta": { "hello": me.to_json() },
        }),
    )
}

/// The result of an answer, or its error's message.
pub fn result(answer: &Value) -> Result<&Value, String> {
    if let Some(e) = answer.get("error") {
        return Err(e.get("message").and_then(Value::as_str).unwrap_or("the peer answered with an error").to_string());
    }
    answer.get("result").ok_or_else(|| "the answer has no result".to_string())
}

/// The provider's hello in its `initialize` result (else from serverInfo).
pub fn hello_of(init: &Value) -> Hello {
    if let Some(h) = init.pointer("/_meta/hello").and_then(Hello::of) {
        return h;
    }
    let info = init.get("serverInfo");
    let s = |k: &str| info.and_then(|i| i.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
    Hello { runtime: s("name"), version: s("version"), platform: String::new(), protocol: String::new() }
}

fn inputs_of(schema: &Value) -> Vec<Input> {
    let required: Vec<&str> =
        schema.get("required").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| {
            p.iter()
                .map(|(k, v)| Input {
                    name: k.clone(),
                    ty: v.get("type").and_then(Value::as_str).and_then(Ty::parse).unwrap_or(Ty::String),
                    required: required.contains(&k.as_str()),
                    doc: v.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn events_of(v: Option<&Value>) -> Vec<Event> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let name = e.get("name")?.as_str()?.to_string();
                    let when = When::parse(e.get("when")?.as_str()?).ok()?;
                    let text = e.get("text").and_then(Value::as_str).unwrap_or(&name).to_string();
                    Some(Event { name, when, text })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn state_of(meta: &Value) -> State {
    let word = meta.get("state").and_then(Value::as_str).unwrap_or("present");
    State::parse(word, meta.get("why").and_then(Value::as_str).unwrap_or(""))
}

/// The capabilities in a provider's `tools/list` and `resources/list`
/// results: grouped by `_meta.capability`; a tool without one is not a
/// capability's (an ordinary MCP tool) and is left out.
pub fn manifest(peer: &str, tools: &Value, resources: &Value) -> Vec<Capability> {
    let mut out: Vec<Capability> = Vec::new();
    let provider = Provider::Remote(peer.to_string());
    for r in resources.get("resources").and_then(Value::as_array).into_iter().flatten() {
        let Some(meta) = r.get("_meta") else { continue };
        let Some(id) = meta.get("capability").and_then(Value::as_str) else { continue };
        let interval = meta.get("interval").and_then(Value::as_u64);
        let uri = r.get("uri").and_then(Value::as_str).unwrap_or("").to_string();
        let snapshot =
            meta.get("snapshot").and_then(Value::as_bool).unwrap_or(true).then(|| Snapshot { interval, uri, ..Snapshot::default() });
        let mut c = Capability::new(
            id,
            meta.get("doc").and_then(Value::as_str).or(r.get("description").and_then(Value::as_str)).unwrap_or(""),
            provider.clone(),
        );
        c.state = state_of(meta);
        c.snapshot = snapshot;
        c.events = events_of(meta.get("events"));
        if !out.iter().any(|o| o.id == c.id) {
            out.push(c);
        }
    }
    for t in tools.get("tools").and_then(Value::as_array).into_iter().flatten() {
        let Some(meta) = t.get("_meta") else { continue };
        let Some(id) = meta.get("capability").and_then(Value::as_str) else { continue };
        let Some(remote) = t.get("name").and_then(Value::as_str) else { continue };
        let name = remote.strip_prefix(&format!("{id}.")).unwrap_or(remote).to_string();
        let tool = Tool {
            name,
            doc: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
            inputs: t.get("inputSchema").map(inputs_of).unwrap_or_default(),
            approval: meta.get("approval").and_then(Value::as_bool).unwrap_or(false),
            remote: remote.to_string(),
        };
        let at = match out.iter().position(|c| c.id == id) {
            Some(at) => at,
            None => {
                let mut c = Capability::new(id, meta.get("doc").and_then(Value::as_str).unwrap_or(""), provider.clone());
                c.state = state_of(meta);
                out.push(c);
                out.len() - 1
            }
        };
        if !out[at].tools.iter().any(|x| x.name == tool.name) {
            out[at].tools.push(tool);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// A capability as MCP describes it (a provider written in Rust; and the
/// tests' round trip): its tools for `tools/list`, its resource for
/// `resources/list`.
pub fn describe(c: &Capability) -> (Vec<Value>, Value) {
    let tools = c
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": if t.remote.is_empty() { format!("{}.{}", c.id, t.name) } else { t.remote.clone() },
                "description": t.doc,
                "inputSchema": t.schema(),
                "_meta": { "capability": c.id, "doc": c.doc, "state": c.state.word(), "approval": t.approval },
            })
        })
        .collect();
    let events: Vec<Value> = c.events.iter().map(|e| json!({ "name": e.name, "when": e.when.text(), "text": e.text })).collect();
    let resource = json!({
        "uri": format!("{URI}{}", c.id),
        "name": c.id,
        "description": c.line(),
        "mimeType": "text/plain",
        "_meta": {
            "capability": c.id, "doc": c.doc, "state": c.state.word(), "why": c.state.why(),
            "snapshot": c.snapshot.is_some(), "interval": c.snapshot.as_ref().and_then(|s| s.interval), "events": events,
        },
    });
    (tools, resource)
}

/// A `tools/call` request for a routed call.
pub fn call(id: u64, tool: &str, args: &[(String, String)]) -> Value {
    let arguments: serde_json::Map<String, Value> = args.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    request(id, "tools/call", json!({ "name": tool, "arguments": arguments }))
}

/// A `tools/call` result as one result line: its text content; `isError`
/// as an error.
pub fn call_result(answer: &Value) -> Result<String, String> {
    let r = result(answer)?;
    let text: Vec<&str> =
        r.get("content").and_then(Value::as_array).into_iter().flatten().filter_map(|c| c.get("text").and_then(Value::as_str)).collect();
    let text = text.join("\n");
    if r.get("isError").and_then(Value::as_bool).unwrap_or(false) { Err(text) } else { Ok(text) }
}

/// A `resources/read` result's text.
pub fn read_result(answer: &Value) -> Result<String, String> {
    let r = result(answer)?;
    Ok(r.get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// A notification from the provider's stream, read.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    /// A capability's snapshot changed (read it again); an event may ride along.
    Updated { capability: String, event: Option<(String, String)> },
    /// Its tools changed: list them again (re-register).
    ListChanged,
    /// Anything else.
    Other,
}

/// One JSON-RPC notification (an SSE `data:` line).
pub fn note(msg: &Value) -> Note {
    match msg.get("method").and_then(Value::as_str) {
        Some("notifications/tools/list_changed" | "notifications/resources/list_changed") => Note::ListChanged,
        Some("notifications/resources/updated") => {
            let p = msg.get("params");
            let uri = p.and_then(|p| p.get("uri")).and_then(Value::as_str).unwrap_or("");
            let Some(id) = uri.strip_prefix(URI) else { return Note::Other };
            let event = p.and_then(|p| p.pointer("/_meta/event")).and_then(|e| {
                Some((e.get("name")?.as_str()?.to_string(), e.get("text").and_then(Value::as_str).unwrap_or("").to_string()))
            });
            Note::Updated { capability: id.to_string(), event }
        }
        _ => Note::Other,
    }
}
