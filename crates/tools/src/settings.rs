//! Models and engine settings: read which models there are (in
//! `models.json` and on the servers it names), read an engine's settings,
//! change its model, change its sampling settings.
//!
//! The rules are in the core (`dry_core::sampling` checks a value and adds
//! the set ones to a request; `dry_core::served` reads what a server says
//! about its models). This is the I/O: HTTP to the model servers, and the
//! store the engines are in. The app tools (`app_tools`, group `settings`)
//! and the `dry models` / `dry engine` commands both call these.
//!
//! Keys are never read out: only the name of the variable that holds one,
//! and whether it is set.

use std::time::Duration;

use dry_core::inference::{Api, OpenAi};
use dry_core::models::Models;
use dry_core::response::ResponseFormat;
use dry_core::sampling;
use dry_core::served::{self, ServedModel, ServerKind};
use dry_core::state::{Engine, State};
use serde_json::Value;

use crate::Result;
use crate::store::Store;

/// A model server is asked briefly; one that does not answer is reported
/// as unreachable, not waited for.
const TIMEOUT: Duration = Duration::from_secs(5);

/// `models.json` at the repo root.
pub fn read_models() -> Result<Models> {
    let file = crate::root().join("models.json");
    Models::from_json(&crate::read(&file)?).map_err(|e| format!("models.json: {e}"))
}

/// Write `models.json` at the repo root (temp file + rename).
pub fn save_models(models: &Models) -> Result {
    crate::write(&crate::root().join("models.json"), &models.to_json())
}

// ---- HTTP ---------------------------------------------------------------------------

fn bearer(key_env: &Option<String>) -> Option<String> {
    key_env.as_ref().and_then(|v| std::env::var(v).ok()).map(|k| format!("Bearer {k}"))
}

/// The body of a 2xx answer; anything else is an error with its status.
fn http(method: &str, url: &str, key_env: &Option<String>, body: Option<&str>) -> Result<String> {
    let read = |mut resp: ureq::http::Response<ureq::Body>| -> Result<String> {
        let status = resp.status();
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        if status.is_success() { Ok(text) } else { Err(format!("{status} from {url}")) }
    };
    let auth = bearer(key_env);
    if method == "POST" {
        let mut call = ureq::post(url).header("Content-Type", "application/json");
        if let Some(a) = &auth {
            call = call.header("Authorization", a);
        }
        let resp = call
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .build()
            .send(body.unwrap_or("{}"))
            .map_err(|e| e.to_string())?;
        read(resp)
    } else {
        let mut call = ureq::get(url);
        if let Some(a) = &auth {
            call = call.header("Authorization", a);
        }
        let resp = call.config().http_status_as_error(false).timeout_global(Some(TIMEOUT)).build().call().map_err(|e| e.to_string())?;
        read(resp)
    }
}

/// What one server said.
#[derive(Debug, Default)]
pub struct Probe {
    pub kind: ServerKind,
    pub models: Vec<ServedModel>,
    /// Why an endpoint gave nothing (none answered: the server is unreachable).
    pub errors: Vec<String>,
}

/// Ask the server at `base_url` for its models: `GET {base}/models`, then
/// LM Studio's or Ollama's own endpoint for metadata. Read only: nothing
/// is loaded.
pub fn probe(base_url: &str, key_env: &Option<String>) -> Probe {
    let mut p = Probe::default();
    let base = base_url.trim_end_matches('/');
    let root = served::server_root(base);
    match http("GET", &format!("{base}/models"), key_env, None).and_then(|b| served::parse_openai(&b)) {
        Ok(m) => p.models = m,
        Err(e) => p.errors.push(format!("{base}/models: {e}")),
    }
    // LM Studio's metadata endpoint.
    if let Ok(m) = http("GET", &format!("{root}/api/v0/models"), key_env, None).and_then(|b| served::parse_lmstudio(&b)) {
        p.kind = ServerKind::LmStudio;
        p.models = served::merge(std::mem::take(&mut p.models), m);
        return p;
    }
    // Ollama's.
    if let Ok(mut m) = http("GET", &format!("{root}/api/tags"), key_env, None).and_then(|b| served::parse_ollama_tags(&b)) {
        p.kind = ServerKind::Ollama;
        if let Err(e) = http("GET", &format!("{root}/api/ps"), key_env, None).and_then(|b| served::apply_ollama_ps(&mut m, &b)) {
            p.errors.push(format!("{root}/api/ps: {e}"));
        }
        for model in &mut m {
            let ask = serde_json::json!({ "model": model.id }).to_string();
            if let Err(e) =
                http("POST", &format!("{root}/api/show"), key_env, Some(&ask)).and_then(|b| served::apply_ollama_show(model, &b))
            {
                p.errors.push(format!("{root}/api/show {}: {e}", model.id));
            }
        }
        p.models = served::merge(std::mem::take(&mut p.models), m);
    }
    p
}

fn key_word(key_env: &Option<String>) -> String {
    match key_env {
        None => "no key".into(),
        Some(v) if std::env::var(v).is_ok() => format!("key in ${v} (set)"),
        Some(v) => format!("key in ${v} (NOT set)"),
    }
}

// ---- reading -----------------------------------------------------------------------

/// Every model setting in `models.json` and, with `ask`, what each server
/// it names reports (each server once). `only`: one setting.
pub fn models_report(models: &Models, only: Option<&str>, ask: bool) -> Result<String> {
    let settings: Vec<_> = models.models.iter().filter(|m| only.is_none_or(|o| m.name == o)).collect();
    if let Some(o) = only
        && settings.is_empty()
    {
        let names: Vec<&str> = models.models.iter().map(|m| m.name.as_str()).collect();
        return Err(format!("no setting `{o}` in models.json; they are {}", names.join(", ")));
    }
    let mut out = vec!["models.json settings:".to_string()];
    let mut servers: Vec<(String, Option<String>, Vec<String>)> = Vec::new();
    for m in &settings {
        let default = if m.name == models.default { " (default)" } else { "" };
        match models.inference(m) {
            Ok(ai) if m.is_cli() => {
                // A CLI model (`dry_core::cli`): the program, found or not.
                let found = dry_core::cli::from_base(&ai.base_url).map(|c| crate::cli::find(&c, ask).line()).unwrap_or_else(|e| e);
                out.push(format!(
                    "- {}{default}: cli · model {} · {found} · context {}{}",
                    m.name,
                    if ai.model.is_empty() { "(none chosen)" } else { &ai.model },
                    ai.context.unwrap_or_default(),
                    if m.context.is_some() { "" } else { " (the CLI's default)" }
                ));
            }
            Ok(ai) => {
                let context = m.context.map_or("unknown (not set)".into(), |c| c.to_string());
                out.push(format!(
                    "- {}{default}: model {} · {} @ {} · {} · context {context}",
                    m.name,
                    if ai.model.is_empty() { "(none chosen)" } else { &ai.model },
                    ai.api.as_str(),
                    ai.base_url,
                    key_word(&ai.key_env)
                ));
                match servers.iter_mut().find(|(b, _, _)| *b == ai.base_url) {
                    Some((_, _, names)) => names.push(m.name.clone()),
                    None => servers.push((ai.base_url.clone(), ai.key_env.clone(), vec![m.name.clone()])),
                }
            }
            Err(e) => out.push(format!("- {}{default}: {} (broken: {e})", m.name, m.id)),
        }
    }
    if !ask {
        return Ok(out.join("\n"));
    }
    if only.is_none() || settings.iter().any(|m| m.is_cli()) {
        out.push(String::new());
        out.push("CLIs on this computer (kind \"cli\" models run one of these):".into());
        out.extend(crate::cli::discover(true).iter().map(|f| format!("  - {}", f.line())));
    }
    for (base, key_env, names) in servers {
        out.push(String::new());
        out.push(format!("{base} (for {}):", names.join(", ")));
        if let Some(var) = &key_env
            && std::env::var(var).is_err()
        {
            out.push(format!("  not asked: ${var} is not set"));
            continue;
        }
        let p = probe(&base, &key_env);
        if p.models.is_empty() {
            out.push(format!("  unreachable or empty: {}", p.errors.join("; ")));
            continue;
        }
        out.push(format!("  server: {} · {} model(s)", p.kind.as_str(), p.models.len()));
        out.extend(p.models.iter().map(|m| format!("  - {}", m.describe())));
        out.extend(p.errors.iter().map(|e| format!("  (no answer: {e})")));
    }
    Ok(out.join("\n"))
}

/// The engine with id or name `which`.
pub fn find(state: &State, which: &str) -> Result<usize> {
    state.engines.iter().position(|e| e.id == which).or_else(|| state.engines.iter().position(|e| e.name == which)).ok_or_else(|| {
        let ids: Vec<&str> = state.engines.iter().map(|e| e.id.as_str()).collect();
        format!("no engine `{which}`; they are {}", if ids.is_empty() { "none".into() } else { ids.join(", ") })
    })
}

/// One engine's model, endpoint and every sampling setting (`default`
/// where it is the server's choice).
pub fn engine_report(e: &Engine) -> String {
    let ai = &e.inference;
    let mut out = vec![format!("{} ({}): {}", e.id, e.kind(), e.name)];
    if let Some(s) = &e.session {
        out.push(format!("a {} session: its program picks its model; the settings below are not sent anywhere.", s.tool));
    }
    out.push(format!("model: {}", if ai.model.is_empty() { "(none)" } else { &ai.model }));
    out.push(format!(
        "endpoint: {} @ {} · {}",
        ai.api.as_str(),
        if ai.base_url.is_empty() { "(none)" } else { &ai.base_url },
        key_word(&ai.key_env)
    ));
    out.push(format!("context: {}", ai.context.map_or("default (asked from the server)".into(), |c| format!("{c} tokens"))));
    out.push(format!("answer format: {}", e.format.name()));
    out.push("sampling:".into());
    for k in sampling::KEYS {
        out.push(format!("- {}: {}", k.name, ai.sampling.get(k.name).map_or("default".into(), Value::to_string)));
    }
    for (k, v) in ai.sampling.iter().filter(|(k, _)| k.starts_with(sampling::FREE)) {
        out.push(format!("- {k}: {v}"));
    }
    if let Some(note) = api_note(ai) {
        out.push(note);
    }
    out.join("\n")
}

fn api_note(ai: &OpenAi) -> Option<String> {
    let off = ai.sampling.not_for(ai.api);
    (!off.is_empty() && ai.api == Api::Responses).then(|| {
        format!("note: the Responses API has no {}; OpenAI refuses the request with it, a local server may ignore it.", off.join(", "))
    })
}

// ---- changing ----------------------------------------------------------------------

fn model_engine(e: &Engine) -> Result {
    match &e.session {
        Some(s) => Err(format!("{} is a {} session: its program picks the model, and these settings would not be sent", e.id, s.tool)),
        None => Ok(()),
    }
}

/// Point engine `which` at another model: `setting` takes a `models.json`
/// setting's endpoint, key and model; `model` sets the model id the server
/// knows (on the setting's endpoint, or the engine's own). Its sampling
/// settings stay. Saved to `store`.
pub fn set_model(store: &dyn Store, models: &Models, which: &str, setting: Option<&str>, model: Option<&str>) -> Result<String> {
    if setting.is_none() && model.is_none() {
        return Err("give a setting (a name in models.json), a model (an id the server knows), or both".into());
    }
    let mut state = crate::store::load_or_first(store)?;
    let i = find(&state, which)?;
    model_engine(&state.engines[i])?;
    let before = state.engines[i].inference.model.clone();
    let mut ai = state.engines[i].inference.clone();
    if let Some(name) = setting {
        let m = models.get(name).ok_or_else(|| format!("no setting `{name}` in models.json"))?;
        let from = models.inference(m)?;
        ai = OpenAi { sampling: ai.sampling, ..from };
    }
    if let Some(id) = model {
        if id.trim().is_empty() {
            return Err("an empty model id".into());
        }
        ai.model = id.trim().to_string();
    }
    if ai.base_url.is_empty() {
        return Err(format!("{which} has no endpoint; give a setting from models.json too"));
    }
    let mut lines = vec![format!("{}: model {before} → {} ({} @ {})", state.engines[i].id, ai.model, ai.api.as_str(), ai.base_url)];
    // Is it one the server has? Asked, never assumed. A CLI model has no
    // server to ask: the CLI says at the first call.
    if dry_core::cli::is_cli(&ai.base_url) {
        if model.is_some() {
            lines.push("not checked: a CLI model is checked by the CLI at the first call".into());
        }
    } else if model.is_some() {
        let p = probe(&ai.base_url, &ai.key_env);
        if p.models.is_empty() {
            lines.push(format!("not checked: the server did not list its models ({})", p.errors.join("; ")));
        } else if !p.models.iter().any(|m| m.id == ai.model) {
            lines.push(format!(
                "warning: {} does not list {}; it has {}",
                ai.base_url,
                ai.model,
                p.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    if setting.is_none()
        && let Some(c) = ai.context
    {
        lines.push(format!("context is still {c} tokens (set for the model before); change it in engine.md if this model differs"));
    }
    if !ai.sampling.is_empty() {
        lines.push(format!("sampling kept: {}", ai.sampling.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ")));
    }
    state.engines[i].inference = ai;
    store.save(&state)?;
    lines.push(saved_note(store));
    Ok(lines.join("\n"))
}

/// Change engine `which`'s sampling settings: each `(key, value)`, where
/// no value (or `"default"`) unsets it. All are checked before any is
/// saved. Saved to `store`.
pub fn set_sampling(store: &dyn Store, which: &str, changes: &[(String, Option<Value>)]) -> Result<String> {
    if changes.is_empty() {
        return Err("nothing to change".into());
    }
    let mut state = crate::store::load_or_first(store)?;
    let i = find(&state, which)?;
    model_engine(&state.engines[i])?;
    let ai = &mut state.engines[i].inference;
    let mut lines = Vec::new();
    for (key, value) in changes {
        match value.as_ref().filter(|v| !v.is_null() && v.as_str() != Some("default")) {
            None => {
                if !sampling::owns(key) {
                    sampling::check(key, &Value::Null)?;
                }
                let was = ai.sampling.unset(key);
                lines.push(format!("{key}: default{}", if was { "" } else { " (it was not set)" }));
            }
            Some(v) => {
                let v = ai.sampling.set(key, v)?;
                let honoured =
                    sampling::key(key).map_or("whichever server takes it (a free key)".to_string(), |k| k.honoured_by.to_string());
                lines.push(format!("{key}: {v} (honoured by {honoured})"));
            }
        }
    }
    if let Some(note) = api_note(ai) {
        lines.push(note);
    }
    let id = state.engines[i].id.clone();
    store.save(&state)?;
    lines.insert(0, format!("{id}:"));
    lines.push(saved_note(store));
    Ok(lines.join("\n"))
}

fn saved_note(store: &dyn Store) -> String {
    format!("saved to {}; a running app reads it with the engine's Reload spec (its own Save would write its copy back)", store.describe())
}

/// Read a `dry engine set` command line's value: `default` unsets, several
/// words are a list (for `stop`), one is read as JSON or else as text.
pub fn value_from_words(words: &[&str]) -> Option<Value> {
    match words {
        [] | ["default"] => None,
        [one] => Some(sampling::read_value(one)),
        many => Some(Value::from(many.iter().map(|w| w.to_string()).collect::<Vec<_>>())),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::store::{MdStore, SqliteStore, StateStore as _};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dry-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// No server: the base URL points at a closed port, so nothing here
    /// touches a real model server.
    const MODELS: &str = r#"{"default":"a","models":[
        {"name":"a","id":"openai/c/m1","base_url":"http://127.0.0.1:9/v1"},
        {"name":"b","id":"openai/r/m2","base_url":"http://127.0.0.1:9/v1","context":8192},
        {"name":"c","kind":"cli","provider":"claude","model":"haiku","program":"/nonexistent/claude"}]}"#;

    fn stores(dir: &std::path::Path) -> Vec<Box<dyn Store>> {
        vec![Box::new(MdStore { root: dir.join("md") }), Box::new(SqliteStore { path: dir.join("s.db") })]
    }

    #[test]
    fn settings_are_set_checked_unset_and_kept_by_both_stores() {
        let dir = scratch("set");
        for store in stores(&dir) {
            store.save(&dry_core::files::sample()).unwrap();
            let out =
                set_sampling(&*store, "engine-1", &[("temperature".into(), Some(json!(0))), ("seed".into(), Some(json!("42")))]).unwrap();
            assert!(out.contains("temperature: 0.0") && out.contains("seed: 42"), "{out}");
            assert!(out.contains("Responses API has no seed"), "the sample engine is on responses: {out}");
            let e = &store.load().unwrap().engines[0];
            assert_eq!(e.inference.sampling.get("seed"), Some(&json!(42)));
            assert!(engine_report(e).contains("- top_p: default"));

            let err = set_sampling(&*store, "engine-1", &[("top_p".into(), Some(json!(0.5))), ("temperature".into(), Some(json!(3)))])
                .unwrap_err();
            assert!(err.contains("0 to 2"), "{err}");
            assert_eq!(store.load().unwrap().engines[0].inference.sampling.get("top_p"), None, "a bad value saves none of them");

            set_sampling(&*store, "engine-1", &[("seed".into(), Some(json!("default"))), ("temperature".into(), None)]).unwrap();
            let s = &store.load().unwrap().engines[0].inference.sampling;
            assert!(s.get("seed").is_none() && s.get("temperature").is_none());
            assert!(set_sampling(&*store, "claude-2", &[("temperature".into(), Some(json!(1)))]).unwrap_err().contains("session"));
            assert!(set_sampling(&*store, "nope", &[("temperature".into(), Some(json!(1)))]).is_err());
            assert!(set_sampling(&*store, "engine-1", &[("temprature".into(), None)]).is_err(), "a typo is not silently unset");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_model_changes_and_the_sampling_stays() {
        let dir = scratch("model");
        let models = Models::from_json(MODELS).unwrap();
        let store = MdStore { root: dir.clone() };
        store.save(&dry_core::files::sample()).unwrap();
        let out = set_model(&store, &models, "engine-1", Some("a"), None).unwrap();
        assert!(out.contains("model m → m1") && out.contains("sampling kept"), "{out}");
        let ai = store.load().unwrap().engines[0].inference.clone();
        assert_eq!((ai.api, ai.base_url.as_str(), ai.key_env.clone()), (Api::ChatCompletions, "http://127.0.0.1:9/v1", None));
        assert_eq!(ai.sampling.get("temperature"), Some(&json!(0.2)));

        let out = set_model(&store, &models, "engine-1", None, Some("other")).unwrap();
        assert!(out.contains("not checked"), "the closed port is not a model list: {out}");
        assert_eq!(store.load().unwrap().engines[0].inference.model, "other");
        assert!(set_model(&store, &models, "engine-1", None, None).is_err());
        assert!(set_model(&store, &models, "engine-1", Some("zzz"), None).is_err());
        assert!(set_model(&store, &models, "claude-2", Some("a"), None).is_err());

        let out = set_model(&store, &models, "engine-1", Some("c"), Some("sonnet")).unwrap();
        assert!(
            out.contains("model other → sonnet (chat-completions @ cli:claude?program=/nonexistent/claude)")
                && out.contains("checked by the CLI"),
            "{out}"
        );
        let ai = store.load().unwrap().engines[0].inference.clone();
        assert_eq!(
            (ai.base_url.as_str(), ai.model.as_str(), ai.context),
            ("cli:claude?program=/nonexistent/claude", "sonnet", Some(200_000))
        );

        let report = models_report(&models, None, false).unwrap();
        assert!(
            report.contains("- a (default): model m1 · chat-completions @ http://127.0.0.1:9/v1 · no key · context unknown"),
            "{report}"
        );
        assert!(report.contains("context 8192"));
        assert!(report.contains("- c: cli · model haiku · claude: unavailable (/nonexistent/claude is not on PATH"), "{report}");
        assert!(models_report(&models, Some("zzz"), false).is_err());
        let asked = models_report(&models, Some("a"), true).unwrap();
        assert!(asked.contains("unreachable"), "{asked}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_line_values() {
        assert_eq!(value_from_words(&["default"]), None);
        assert_eq!(value_from_words(&["0.2"]), Some(json!(0.2)));
        assert_eq!(value_from_words(&["END", "STOP"]), Some(json!(["END", "STOP"])));
    }
}
