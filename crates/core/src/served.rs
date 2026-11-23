//! Served models: what a model server says it has, with what it says
//! about each (context length, family, quantization, loaded or not).
//!
//! Every server speaking the OpenAI format answers `GET {base}/models`
//! with ids only. Two local servers say more on their own endpoints, at the
//! server's root (the base URL without `/v1`):
//!
//! - LM Studio: `GET /api/v0/models` (type, publisher, arch, quantization,
//!   state, max and loaded context length, capabilities).
//! - Ollama: `GET /api/tags` (family, parameter size, quantization, size),
//!   `POST /api/show` per model (context length, capabilities) and
//!   `GET /api/ps` (which are loaded).
//!
//! What no endpoint said stays `None` and shows as "unknown": nothing is
//! guessed. Pure: these read the bodies; the shell does the HTTP.

use serde_json::Value;

/// Which kind of server answered, from the endpoints that answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServerKind {
    LmStudio,
    Ollama,
    /// Only the OpenAI endpoints answered (OpenAI itself, vLLM, a proxy...).
    #[default]
    OpenAiCompatible,
}

impl ServerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerKind::LmStudio => "LM Studio",
            ServerKind::Ollama => "Ollama",
            ServerKind::OpenAiCompatible => "OpenAI-compatible (no metadata endpoint answered)",
        }
    }
}

/// One model a server has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServedModel {
    pub id: String,
    /// `llm`, `vlm`, `embeddings`...
    pub kind: Option<String>,
    /// Architecture or family (`qwen3`, `llama`).
    pub family: Option<String>,
    /// Who made or owns it.
    pub publisher: Option<String>,
    pub parameters: Option<String>,
    pub quantization: Option<String>,
    /// The file format (`gguf`, `mlx`).
    pub format: Option<String>,
    /// Size on disk, in bytes.
    pub size: Option<u64>,
    /// The most context the model can take, in tokens.
    pub max_context: Option<u64>,
    /// The context it is loaded with now, in tokens.
    pub loaded_context: Option<u64>,
    pub loaded: Option<bool>,
    pub capabilities: Option<Vec<String>>,
}

impl ServedModel {
    /// One line: every field, `unknown` where nothing said.
    pub fn describe(&self) -> String {
        fn or<T: ToString>(v: &Option<T>) -> String {
            v.as_ref().map_or("unknown".into(), T::to_string)
        }
        let loaded = match self.loaded {
            Some(true) => "loaded".to_string(),
            Some(false) => "not loaded".to_string(),
            None => "unknown".to_string(),
        };
        let caps = self.capabilities.as_ref().map_or("unknown".into(), |c| if c.is_empty() { "none listed".into() } else { c.join(", ") });
        format!(
            "{} · state: {loaded} · type: {} · family: {} · publisher: {} · parameters: {} · quantization: {} · format: {} · size: {} · max context: {} · loaded context: {} · capabilities: {caps}",
            self.id,
            or(&self.kind),
            or(&self.family),
            or(&self.publisher),
            or(&self.parameters),
            or(&self.quantization),
            or(&self.format),
            self.size.map_or("unknown".into(), human_bytes),
            or(&self.max_context),
            or(&self.loaded_context),
        )
    }
}

fn human_bytes(n: u64) -> String {
    let gb = n as f64 / 1e9;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", n as f64 / 1e6) }
}

/// The server's root: the base URL without a trailing `/v1`.
pub fn server_root(base_url: &str) -> &str {
    let base = base_url.trim_end_matches('/');
    base.strip_suffix("/v1").unwrap_or(base)
}

fn json(body: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
    if let Some(m) = v.pointer("/error/message").and_then(Value::as_str).or(v.get("error").and_then(Value::as_str)) {
        return Err(m.to_string());
    }
    Ok(v)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

fn texts(v: &Value, key: &str) -> Option<Vec<String>> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

/// `GET {base}/models`: `{"data":[{"id", "owned_by"}]}`.
pub fn parse_openai(body: &str) -> Result<Vec<ServedModel>, String> {
    let v = json(body)?;
    let data = v.get("data").and_then(Value::as_array).ok_or("no data list")?;
    Ok(data
        .iter()
        .filter_map(|m| Some(ServedModel { id: text(m, "id")?, publisher: text(m, "owned_by"), ..ServedModel::default() }))
        .collect())
}

/// LM Studio's `GET /api/v0/models`.
pub fn parse_lmstudio(body: &str) -> Result<Vec<ServedModel>, String> {
    let v = json(body)?;
    let data = v.get("data").and_then(Value::as_array).ok_or("no data list")?;
    Ok(data
        .iter()
        .filter_map(|m| {
            Some(ServedModel {
                id: text(m, "id")?,
                kind: text(m, "type"),
                family: text(m, "arch"),
                publisher: text(m, "publisher"),
                quantization: text(m, "quantization"),
                format: text(m, "compatibility_type"),
                max_context: m.get("max_context_length").and_then(Value::as_u64),
                loaded_context: m.get("loaded_context_length").and_then(Value::as_u64),
                loaded: text(m, "state").map(|s| s == "loaded"),
                capabilities: texts(m, "capabilities"),
                ..ServedModel::default()
            })
        })
        .collect())
}

/// Ollama's `GET /api/tags`. Loaded state and context come from
/// `apply_ollama_ps` and `apply_ollama_show`.
pub fn parse_ollama_tags(body: &str) -> Result<Vec<ServedModel>, String> {
    let v = json(body)?;
    let models = v.get("models").and_then(Value::as_array).ok_or("no models list")?;
    Ok(models
        .iter()
        .filter_map(|m| {
            let d = m.get("details").cloned().unwrap_or(Value::Null);
            Some(ServedModel {
                id: text(m, "name").or_else(|| text(m, "model"))?,
                family: text(&d, "family"),
                parameters: text(&d, "parameter_size"),
                quantization: text(&d, "quantization_level"),
                format: text(&d, "format"),
                size: m.get("size").and_then(Value::as_u64),
                ..ServedModel::default()
            })
        })
        .collect())
}

/// Ollama's `GET /api/ps`: the loaded ones are marked loaded (with their
/// context when it says), every other one not loaded.
pub fn apply_ollama_ps(models: &mut [ServedModel], body: &str) -> Result<(), String> {
    let v = json(body)?;
    let running = v.get("models").and_then(Value::as_array).ok_or("no models list")?;
    for m in models.iter_mut() {
        let hit = running.iter().find(|r| text(r, "name").as_deref() == Some(&m.id) || text(r, "model").as_deref() == Some(&m.id));
        m.loaded = Some(hit.is_some());
        if let Some(r) = hit {
            m.loaded_context = r.get("context_length").and_then(Value::as_u64).or(m.loaded_context);
        }
    }
    Ok(())
}

/// Ollama's `POST /api/show` for one model: `<arch>.context_length` in
/// `model_info`, and `capabilities`.
pub fn apply_ollama_show(model: &mut ServedModel, body: &str) -> Result<(), String> {
    let v = json(body)?;
    if let Some(info) = v.get("model_info").and_then(Value::as_object) {
        let arch = info.get("general.architecture").and_then(Value::as_str);
        model.max_context = arch.and_then(|a| info.get(&format!("{a}.context_length"))).and_then(Value::as_u64).or(model.max_context);
        if model.family.is_none() {
            model.family = arch.map(str::to_string);
        }
    }
    if let Some(c) = texts(&v, "capabilities") {
        model.capabilities = Some(c);
    }
    Ok(())
}

/// Lay `extra` (from a metadata endpoint) over `base` (from `/models`),
/// by id: a field `extra` knows wins; a model only one list has is kept.
pub fn merge(base: Vec<ServedModel>, extra: Vec<ServedModel>) -> Vec<ServedModel> {
    let mut out = base;
    for x in extra {
        match out.iter_mut().find(|m| m.id == x.id) {
            Some(m) => {
                let publisher = m.publisher.take();
                *m = ServedModel { publisher: x.publisher.clone().or(publisher), ..x };
            }
            None => out.push(x),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As LM Studio answered on this machine (2026-11-22), trimmed.
    const LMSTUDIO: &str = r#"{"data":[
        {"id":"ripple-65b-abliterated","object":"model","type":"llm","publisher":"orcarouter","arch":"ricursive","compatibility_type":"gguf","state":"loaded","max_context_length":2097152,"loaded_context_length":4096,"capabilities":["tool_use"]},
        {"id":"q4-ple4@?","object":"model","type":"vlm","publisher":"Qwen","compatibility_type":"gguf","state":"not-loaded"}]}"#;

    #[test]
    fn lm_studio_says_the_most_and_the_rest_is_unknown() {
        let models = parse_lmstudio(LMSTUDIO).unwrap();
        assert_eq!(models[0].max_context, Some(2_097_152));
        assert_eq!(models[0].loaded_context, Some(4096));
        assert_eq!(models[0].loaded, Some(true));
        assert_eq!(models[0].family.as_deref(), Some("ricursive"));
        let line = models[1].describe();
        assert!(
            line.contains("state: not loaded") && line.contains("quantization: unknown") && line.contains("max context: unknown"),
            "{line}"
        );
        assert!(line.contains("capabilities: unknown"), "no list is not an empty list: {line}");
    }

    #[test]
    fn the_openai_list_gives_ids_and_metadata_is_laid_over_it() {
        let base =
            parse_openai(r#"{"data":[{"id":"ripple-65b-abliterated","owned_by":"organization_owner"},{"id":"only-here"}]}"#).unwrap();
        assert_eq!(base[0].describe().matches("unknown").count(), 10, "{}", base[0].describe());
        let merged = merge(base, parse_lmstudio(LMSTUDIO).unwrap());
        assert_eq!(merged.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["ripple-65b-abliterated", "only-here", "q4-ple4@?"]);
        assert_eq!(merged[0].publisher.as_deref(), Some("orcarouter"));
        assert_eq!(merged[0].loaded, Some(true));
        assert_eq!(merged[1].loaded, None);
    }

    #[test]
    fn ollama_tags_then_ps_and_show_fill_in_state_and_context() {
        let tags = r#"{"models":[{"name":"llama3.2:3b","model":"llama3.2:3b","size":2019393189,"details":{"format":"gguf","family":"llama","parameter_size":"3.2B","quantization_level":"Q4_K_M"}},{"name":"qwen3:8b","details":{}}]}"#;
        let mut models = parse_ollama_tags(tags).unwrap();
        assert_eq!(models[0].parameters.as_deref(), Some("3.2B"));
        apply_ollama_ps(&mut models, r#"{"models":[{"name":"llama3.2:3b","context_length":8192}]}"#).unwrap();
        assert_eq!((models[0].loaded, models[0].loaded_context), (Some(true), Some(8192)));
        assert_eq!(models[1].loaded, Some(false));
        let show = r#"{"model_info":{"general.architecture":"qwen3","qwen3.context_length":40960},"capabilities":["completion","tools"]}"#;
        apply_ollama_show(&mut models[1], show).unwrap();
        assert_eq!(models[1].max_context, Some(40960));
        assert_eq!(models[1].family.as_deref(), Some("qwen3"));
        assert_eq!(models[1].capabilities.as_ref().unwrap(), &["completion", "tools"]);
        assert!(models[0].describe().contains("size: 2.0 GB"));
    }

    #[test]
    fn errors_and_roots() {
        assert!(parse_openai(r#"{"error":{"message":"bad key"}}"#).unwrap_err().contains("bad key"));
        assert!(parse_ollama_tags(r#"{"error":"not found"}"#).is_err());
        assert!(parse_lmstudio("<html>").is_err());
        assert_eq!(server_root("http://127.0.0.1:1234/v1/"), "http://127.0.0.1:1234");
        assert_eq!(server_root("http://host:11434"), "http://host:11434");
    }
}
