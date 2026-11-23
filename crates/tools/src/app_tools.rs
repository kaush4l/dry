//! The app's own tools: what an engine (or a Claude Code session) may do
//! to the app itself. List any state object, change a design token (a
//! colour, a size, the font), switch the theme, make a page, add a
//! component to one.
//!
//! One list, served two ways by `dry mcp` (an MCP server on stdio): an
//! engine gets them through its spec's `mcp.json` (the default one is
//! `mcp/dry.json`), a Claude Code session through `--mcp-config`. Every
//! tool writes the same files the UI and the `dry` command do, and the app
//! sees a changed file within a second, so a change shows at once.
//!
//! A second group, `settings` (`settings_tools`), reads the models and
//! changes an engine's model and sampling settings in the store. It is
//! served only on request (`dry mcp settings`, or `dry mcp all`), so an
//! engine gets those tools only when its `mcp.json` names that server. The
//! app does not watch the store: a running app reads a changed engine with
//! its Reload spec.

use dry_core::pages::Component;
use dry_core::theme::Mode;
use serde_json::{Value, json};

use crate::Result;

/// One tool: its name, what it does, its input (JSON Schema) and the code.
pub struct AppTool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub run: fn(&Value) -> Result<String>,
}

fn text(args: &Value, key: &str) -> Result<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("needs {key} (text)"))
}

fn props(p: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": p, "required": required })
}

/// What `list` can list, each as text a model reads well.
const LISTS: &[&str] = &["engines", "routes", "components", "tokens", "themes", "models", "programs", "capabilities"];

fn list(args: &Value) -> Result<String> {
    let what = text(args, "what")?;
    Ok(match what.as_str() {
        "engines" => {
            let store = crate::store::open_default()?;
            if !store.exists() {
                return Ok(format!("{}: nothing saved yet", store.describe()));
            }
            store
                .load()?
                .engines
                .iter()
                .map(|e| {
                    format!("- {} ({}): {} · {} message(s), {} artifact(s)", e.id, e.kind(), e.name, e.messages.len(), e.artifacts.len())
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        "routes" => {
            let loaded = crate::load();
            loaded
                .pages
                .iter()
                .map(|(pattern, page)| {
                    let view: Vec<String> = page.view.iter().map(|c| serde_json::to_string(c).unwrap_or_default()).collect();
                    format!("- {pattern} {} {} (nav {:?}, order {}): {}", page.icon, page.title, page.nav, page.order, view.join(", "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        "components" => crate::load()
            .components
            .iter()
            .map(|(name, view)| format!("- {name}: {}", serde_json::to_string(view).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n"),
        // The active theme file itself: every token, its value, default and use.
        "tokens" => format!("active theme: {}\n\n{}", crate::active_theme(), crate::read_theme()?.to_md()),
        "themes" => crate::list_themes()
            .iter()
            .map(|t| if *t == crate::active_theme() { format!("- {t} (active)") } else { format!("- {t}") })
            .collect::<Vec<_>>()
            .join("\n"),
        "models" => crate::read(&crate::root().join("models.json"))?,
        "programs" => crate::read_sessions()?
            .tools
            .iter()
            .map(|t| format!("- {} {}: {}", t.icon, t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n"),
        "capabilities" => crate::capabilities()
            .iter()
            .map(|(c, s)| format!("- {}: {}{}", c.name(), s.word(), s.detail().map(|d| format!(" ({d})")).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n"),
        other => return Err(format!("cannot list {other}; one of {}", LISTS.join(", "))),
    })
}

fn token_set(args: &Value) -> Result<String> {
    let (name, value) = (text(args, "name")?, text(args, "value")?);
    let mode = match args.get("mode").and_then(Value::as_str) {
        None | Some("") | Some("both") => None,
        Some(m) => Some(Mode::parse(m).ok_or_else(|| format!("mode is light, dark or both, not {m}"))?),
    };
    let mut theme = crate::read_theme()?;
    theme.set(&name, &value, mode)?;
    crate::save_theme(&theme)?;
    Ok(format!("{name} = {value} in theme {}", crate::active_theme()))
}

fn token_reset(args: &Value) -> Result<String> {
    let name = text(args, "name")?;
    let mut theme = crate::read_theme()?;
    theme.reset(if name == "all" { None } else { Some(&name) });
    crate::save_theme(&theme)?;
    Ok(format!("reset {name}"))
}

fn theme_use(args: &Value) -> Result<String> {
    let name = text(args, "name")?;
    crate::set_active_theme(&name)?;
    Ok(format!("theme {name} is active"))
}

fn page_create(args: &Value) -> Result<String> {
    let icon = args.get("icon").and_then(Value::as_str).unwrap_or("");
    let pattern = crate::create_page(&text(args, "path")?, &text(args, "title")?, icon)?;
    Ok(format!("created {pattern}"))
}

fn component(args: &Value) -> Result<Component> {
    let c = args.get("component").ok_or("needs component (an object like {\"type\": \"text\", \"text\": \"...\"})")?;
    // Accept it as an object or as JSON text.
    let c = match c.as_str() {
        Some(s) => serde_json::from_str(s).map_err(|e| format!("component: {e}"))?,
        None => c.clone(),
    };
    serde_json::from_value(c).map_err(|e| format!("component: {e}"))
}

fn page_add(args: &Value) -> Result<String> {
    let path = text(args, "path")?;
    crate::add_component(&path, component(args)?)?;
    Ok(format!("added to {}", crate::pattern(&path)))
}

fn page_delete(args: &Value) -> Result<String> {
    Ok(format!("deleted {}", crate::delete_page(&text(args, "path")?)?))
}

fn component_create(args: &Value) -> Result<String> {
    let name = text(args, "name")?;
    let c = if args.get("component").is_some() { Some(component(args)?) } else { None };
    crate::add_to_component(&name, c)?;
    Ok(format!("components/{name}.json (place it with page_add and {{\"type\": \"use\", \"name\": \"{name}\"}})"))
}

const COMPONENT_HELP: &str = "A component object: {\"type\": \"heading\"|\"text\", \"text\": \"...\"}, {\"type\": \"link\", \"label\": \"...\", \"to\": \"/path\"}, {\"type\": \"use\", \"name\": \"<component file>\"}, or one of the app's own views: {\"type\": \"chat\"|\"tokens\"|\"appearance\"|\"models\"|\"routes\"|\"diagnostics\"|\"new-route\"|\"engines\"|\"metadata\"|\"tools\"|\"response\"|\"artifacts\"}.";

// ---- models and engine settings (group `settings`) -------------------------------

fn models(args: &Value) -> Result<String> {
    let only = args.get("setting").and_then(Value::as_str).filter(|s| !s.is_empty());
    let ask = args.get("ask_servers").and_then(Value::as_bool).unwrap_or(true);
    crate::settings::models_report(&crate::settings::read_models()?, only, ask)
}

fn engine_settings(args: &Value) -> Result<String> {
    let store = crate::store::open_default()?;
    let state = store.load()?;
    let i = crate::settings::find(&state, &text(args, "engine")?)?;
    Ok(crate::settings::engine_report(&state.engines[i]))
}

fn engine_model(args: &Value) -> Result<String> {
    let word = |k: &str| args.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let store = crate::store::open_default()?;
    crate::settings::set_model(&*store, &crate::settings::read_models()?, &text(args, "engine")?, word("setting"), word("model"))
}

/// The changes an `engine_set` call asks for: `key` + `value`, and/or a
/// `settings` object of them. No value, null or "default": unset.
fn changes(args: &Value) -> Result<Vec<(String, Option<Value>)>> {
    let mut out = Vec::new();
    if let Some(key) = args.get("key").and_then(Value::as_str) {
        out.push((key.to_string(), args.get("value").cloned()));
    }
    if let Some(map) = args.get("settings").and_then(Value::as_object) {
        out.extend(map.iter().map(|(k, v)| (k.clone(), Some(v.clone()))));
    }
    if out.is_empty() {
        return Err("needs key (with value) or settings (an object of key: value)".into());
    }
    Ok(out)
}

fn engine_set(args: &Value) -> Result<String> {
    let store = crate::store::open_default()?;
    crate::settings::set_sampling(&*store, &text(args, "engine")?, &changes(args)?)
}

const ENGINE_SET_HELP: &str = "Change an engine's sampling settings (saved in its spec/engine.md; only the set ones are sent, so an unset one is the server's default). \
Give key and value, or settings as an object to change several at once. A value of null or \"default\" unsets the key. \
Values are checked: temperature 0 to 2, top_p 0 to 1, top_k and max_tokens whole numbers, seed a whole number, stop 1 to 4 texts, presence_penalty and frequency_penalty -2 to 2, repeat_penalty 0 to 2. \
For another key the server takes, use param.<name> (e.g. param.min_p); it is sent as <name> unchecked. \
Deterministic end: temperature 0 (or near it), and also a fixed seed, so the same prompt gives the same answer where the server honours seeds. Creative end: temperature around 0.8 to 1.2, top_p near 1. \
Who honours what: OpenAI chat takes temperature, top_p, seed, stop and the penalties but refuses top_k and repeat_penalty (and reasoning models such as gpt-5 refuse temperature and top_p); the Responses API takes only temperature, top_p and max_tokens (sent as max_output_tokens); LM Studio and llama.cpp take every key; Ollama's /v1 ignores top_k and repeat_penalty; vLLM calls repeat_penalty repetition_penalty (param.repetition_penalty).";

/// The models and engine settings tools. Not in `all()`: an engine sees
/// them only when its spec's `mcp.json` asks for this group
/// (`dry mcp settings`, as `mcp/settings.json` does), so the default
/// engine's tool list stays as it was.
pub fn settings_tools() -> Vec<AppTool> {
    vec![
        AppTool {
            name: "models",
            description: "The models there are: every setting in models.json (model, API, server, which key variable and whether it is set, context), and what each server it names reports: its models with state (loaded or not), type, family, publisher, parameters, quantization, format, size, max and loaded context and capabilities, from /v1/models and LM Studio's /api/v0/models or Ollama's /api/tags and /api/show. Anything no server said is \"unknown\". CLI models (kind cli: claude, codex or copilot run one-shot as the model) show their program, whether it is on PATH and its version. Read only: nothing is loaded.",
            schema: || {
                props(
                    json!({ "setting": { "type": "string", "description": "only this models.json setting" },
                            "ask_servers": { "type": "boolean", "description": "ask the servers too (default true)" } }),
                    &[],
                )
            },
            run: models,
        },
        AppTool {
            name: "engine_settings",
            description: "One engine's settings: its model, its endpoint (API, server, key variable), its context, its answer format, and every sampling setting (temperature, top_p, top_k, max_tokens, seed, stop, presence_penalty, frequency_penalty, repeat_penalty, param.*), \"default\" where the server chooses.",
            schema: || {
                props(json!({ "engine": { "type": "string", "description": "its id (see list what=engines) or name" } }), &["engine"])
            },
            run: engine_settings,
        },
        AppTool {
            name: "engine_model",
            description: "Change an engine's model. setting: a models.json setting (its server, API, key variable and model; or a CLI model such as claude/haiku); model: the id the server knows the model by (see the models tool), on the setting's server or the engine's own. Give either or both. Its sampling settings stay. Warns when the server does not list the model.",
            schema: || {
                props(
                    json!({ "engine": { "type": "string" }, "setting": { "type": "string", "description": "a name in models.json" }, "model": { "type": "string", "description": "a model id on the server" } }),
                    &["engine"],
                )
            },
            run: engine_model,
        },
        AppTool {
            name: "engine_set",
            description: ENGINE_SET_HELP,
            schema: || {
                props(
                    json!({ "engine": { "type": "string" },
                            "key": { "type": "string", "description": "temperature, top_p, top_k, max_tokens, seed, stop, presence_penalty, frequency_penalty, repeat_penalty, or param.<name>" },
                            "value": { "description": "a number, a text, a list of texts (stop), or null / \"default\" to unset" },
                            "settings": { "type": "object", "description": "several at once, e.g. {\"temperature\": 0, \"seed\": 7}" } }),
                    &["engine"],
                )
            },
            run: engine_set,
        },
    ]
}

/// The tools one `dry mcp` group serves: `app` (the default, `all()`),
/// `settings` (`settings_tools()`), `all` (both), or `engines` (a
/// worker's: `crate::worker::tools`, for a session engine).
pub fn group(name: &str) -> Result<Vec<AppTool>> {
    match name {
        "app" => Ok(all()),
        "settings" => Ok(settings_tools()),
        "all" => Ok(all().into_iter().chain(settings_tools()).collect()),
        "engines" => Ok(crate::worker::tools()),
        other => Err(format!("no tool group {other}; app, settings, all or engines")),
    }
}

/// Every app tool. A new one is one entry here.
pub fn all() -> Vec<AppTool> {
    vec![
        AppTool {
            name: "list",
            description: "List app state as text: engines, routes (pages and their components), components, tokens (the active theme: every colour, size and the font), themes, models, programs or capabilities (what this runtime can do, like voice).",
            schema: || props(json!({ "what": { "type": "string", "enum": LISTS } }), &["what"]),
            run: list,
        },
        AppTool {
            name: "token_set",
            description: "Change one design token in the active theme; the app redraws at once. Colours are #RRGGBB or #RRGGBBAA, sizes are numbers, `font` is a font family name.",
            schema: || {
                props(
                    json!({ "name": { "type": "string" }, "value": { "type": "string" }, "mode": { "type": "string", "enum": ["light", "dark", "both"], "description": "for a colour; both if left out" } }),
                    &["name", "value"],
                )
            },
            run: token_set,
        },
        AppTool {
            name: "token_reset",
            description: "Put a design token (or `all`) back to its default in the active theme.",
            schema: || props(json!({ "name": { "type": "string" } }), &["name"]),
            run: token_reset,
        },
        AppTool {
            name: "theme_use",
            description: "Switch the active theme (see list what=themes).",
            schema: || props(json!({ "name": { "type": "string" } }), &["name"]),
            run: theme_use,
        },
        AppTool {
            name: "page_create",
            description: "Create a page (a route) at a path like /notes, with a title and an optional icon glyph; it appears in the navigation.",
            schema: || {
                props(
                    json!({ "path": { "type": "string" }, "title": { "type": "string" }, "icon": { "type": "string" } }),
                    &["path", "title"],
                )
            },
            run: page_create,
        },
        AppTool {
            name: "page_add",
            description: "Add a component to the end of a page.",
            schema: || {
                props(
                    json!({ "path": { "type": "string" }, "component": { "type": "object", "description": COMPONENT_HELP } }),
                    &["path", "component"],
                )
            },
            run: page_add,
        },
        AppTool {
            name: "page_delete",
            description: "Delete a page (not /).",
            schema: || props(json!({ "path": { "type": "string" } }), &["path"]),
            run: page_delete,
        },
        AppTool {
            name: "component_create",
            description: "Create a reusable component file, or add a component to one.",
            schema: || {
                props(json!({ "name": { "type": "string" }, "component": { "type": "object", "description": COMPONENT_HELP } }), &["name"])
            },
            run: component_create,
        },
    ]
}

/// Run tool `name` with `args`.
pub fn run(name: &str, args: &Value) -> Result<String> {
    let tool = all().into_iter().chain(settings_tools()).find(|t| t.name == name).ok_or_else(|| format!("no app tool {name}"))?;
    (tool.run)(args)
}

/// Serve every app tool as an MCP server on stdin/stdout (`dry mcp`).
pub fn serve() -> Result {
    serve_tools(all())
}

/// Serve `tools` (one `group`) as an MCP server (`dry mcp <group>`).
pub fn serve_tools(tools: Vec<AppTool>) -> Result {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue }; // a notification
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let mut reply = match msg.get("method").and_then(Value::as_str).unwrap_or("") {
            "initialize" => json!({ "result": {
                "protocolVersion": dry_core::mcp::PROTOCOL,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "dry", "version": "0.1.0" } } }),
            "tools/list" => json!({ "result": { "tools": tools.iter().map(|t| json!({
                "name": t.name, "description": t.description, "inputSchema": (t.schema)() })).collect::<Vec<_>>() } }),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                // `dashboard` is the thread tool's old name: a session set up before still calls it.
                let name = if name == "dashboard" { "thread" } else { name };
                let found = tools.iter().find(|t| t.name == name).ok_or_else(|| format!("no tool {name} here"));
                match found.and_then(|t| (t.run)(&args)) {
                    Ok(t) => json!({ "result": { "content": [{ "type": "text", "text": t }] } }),
                    Err(e) => json!({ "result": { "content": [{ "type": "text", "text": e }], "isError": true } }),
                }
            }
            "ping" => json!({ "result": {} }),
            other => json!({ "error": { "code": -32601, "message": format!("no method {other}") } }),
        };
        reply["jsonrpc"] = json!("2.0");
        reply["id"] = id;
        writeln!(out, "{reply}").and_then(|_| out.flush()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_an_object_schema_and_lists_read() {
        for t in all() {
            assert_eq!((t.schema)()["type"], "object", "{}", t.name);
        }
        assert!(run("list", &json!({"what": "themes"})).unwrap().contains("(active)"));
        assert!(run("list", &json!({"what": "tokens"})).unwrap().contains("| font |"));
        assert!(run("list", &json!({"what": "nope"})).is_err());
        assert!(run("fly", &json!({})).is_err());
        assert!(component(&json!({"component": "{\"type\": \"text\", \"text\": \"hi\"}"})).is_ok());
    }

    #[test]
    fn the_settings_group_is_its_own_and_its_calls_are_checked() {
        let names = |g: &str| group(g).unwrap().iter().map(|t| t.name).collect::<Vec<_>>();
        assert!(!names("app").contains(&"engine_set"), "the default engine's tools stay as they were");
        assert_eq!(names("settings"), ["models", "engine_settings", "engine_model", "engine_set"]);
        assert_eq!(names("all").len(), names("app").len() + 4);
        assert!(group("nope").is_err());
        for t in settings_tools() {
            assert_eq!((t.schema)()["type"], "object", "{}", t.name);
        }
        assert!(ENGINE_SET_HELP.contains("seed") && ENGINE_SET_HELP.contains("Deterministic"));
        let c = changes(&json!({"engine": "e", "key": "seed", "value": 7, "settings": {"temperature": 0}})).unwrap();
        assert_eq!(c, [("seed".to_string(), Some(json!(7))), ("temperature".to_string(), Some(json!(0)))]);
        assert_eq!(changes(&json!({"engine": "e", "key": "seed"})).unwrap(), [("seed".to_string(), None)]);
        assert!(changes(&json!({"engine": "e"})).is_err());
        assert!(run("engine_set", &json!({})).is_err());
    }
}
