//! Models: the model settings the app can use, kept in a local
//! `models.json`. A state object like `State`, but its own file, because it
//! is configuration (servers, model ids, which key variable) rather than
//! work.
//!
//! Every value lives in the file; this code holds no URLs or model names.
//!
//! ```json
//! {
//!   "default": "local",
//!   "providers": {
//!     "openai": { "base_url": "https://api.openai.com/v1", "key_env": "OPENAI_API_KEY" }
//!   },
//!   "models": [
//!     { "name": "local", "id": "openai/c/qwen3-8b", "base_url": "http://127.0.0.1:1234/v1", "context": 32768 },
//!     { "name": "gpt", "id": "openai/r/gpt-5-mini" }
//!   ]
//! }
//! ```
//!
//! A model's `id` is `provider/api/model`:
//! - `provider` names an entry in `providers`, which gives the base URL and
//!   the key variable;
//! - `api` is the marker for which API to call: `r` = Responses,
//!   `c` = Chat Completions;
//! - `model` is the id the server knows the model by (it may itself contain
//!   `/`, like `qwen/qwen3-8b`; everything after the second `/` is the model).
//!
//! A model may set its own `base_url`: then it talks to that server (a
//! local one, a proxy) with the same API, and uses only its own `key_env`
//! (none for a local server), because the provider's key belongs to the
//! provider's server. Without a `base_url` it goes straight to the provider
//! (for `openai`, the OpenAI API) with the provider's key, unless it names
//! its own `key_env`.
//!
//! `context` (optional) is the model's context window in tokens: the only
//! limit on a turn (`crate::context`). Left out, the app asks the server.
//!
//! A CLI model (`crate::cli`) has no id: `kind: "cli"` and the CLI that
//! answers (`provider`: `claude`, `codex`, `copilot`), the `model` it is
//! given, and optionally `program` (its path, when not the one on `PATH`)
//! and `context`. Its base URL is `cli:<provider>`:
//!
//! ```json
//! { "name": "claude/haiku", "kind": "cli", "provider": "claude", "model": "haiku" }
//! ```
//!
//! `jev: true` (on a model, or on its provider) marks a JEV decision model
//! (`crate::jev`, `docs/jev.md`): one asked typed questions, not chat.
//! `Models::jev_endpoint` finds it; none marked is "no JEV endpoint
//! configured". Calling it is not built yet.
//!
//! `key_env` names an environment variable; the key itself is never stored.
//! Reading and writing the file is the shell's job; this file only turns the
//! JSON into values and back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::inference::{Api, OpenAi};

/// Where a provider's API lives, and which variable holds its key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    /// Up to and including `/v1`.
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    /// It serves JEV decisions (`crate::jev`, `docs/jev.md`): a decisions
    /// endpoint beside its chat one. Not set: it does not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub jev: bool,
}

/// One model setting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// The name people pick it by.
    pub name: String,
    /// `provider/api/model`, e.g. `openai/r/gpt-5-mini`. Empty for a CLI model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// `cli` for a CLI model (`crate::cli`); not set: an HTTP model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// A CLI model's CLI (`claude`, `codex`, `copilot`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// A CLI model's model (`haiku`), passed with the CLI's model flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A CLI model's program, when not the provider's name on `PATH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Overrides the provider's base URL (a local server, a proxy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Its own key variable. With a `base_url` this is the only key used;
    /// without one it replaces the provider's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    /// Its context window in tokens. Not set: the app asks the server
    /// (`crate::context::probes`), else assumes a default it says it assumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u32>,
    /// A glyph for its card (`◇` when not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// It is a JEV decision model (`crate::jev`): asked typed questions on
    /// a decisions endpoint, never chat. Not set: it is not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub jev: bool,
}

/// A model id taken apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelId<'a> {
    pub provider: &'a str,
    pub api: Api,
    /// Empty until one is chosen (`openai/c/`).
    pub model: &'a str,
}

impl ModelId<'_> {
    /// Read `provider/api/model`.
    pub fn parse(id: &str) -> Result<ModelId<'_>, String> {
        let mut parts = id.splitn(3, '/');
        let (Some(provider), Some(marker), Some(model)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("\"{id}\" is not provider/api/model"));
        };
        let api = match marker {
            "r" => Api::Responses,
            "c" => Api::ChatCompletions,
            other => return Err(format!("\"{other}\" in \"{id}\" is not an API marker (r = responses, c = chat completions)")),
        };
        if provider.is_empty() {
            return Err(format!("\"{id}\" has no provider"));
        }
        Ok(ModelId { provider, api, model })
    }

    /// Write it back as `provider/api/model`.
    pub fn to_id(self) -> String {
        let marker = match self.api {
            Api::Responses => "r",
            Api::ChatCompletions => "c",
        };
        format!("{}/{marker}/{}", self.provider, self.model)
    }
}

impl ModelConfig {
    /// Is it a CLI model?
    pub fn is_cli(&self) -> bool {
        self.kind.as_deref() == Some("cli")
    }

    /// Its id, taken apart.
    pub fn parsed(&self) -> Result<ModelId<'_>, String> {
        ModelId::parse(&self.id)
    }

    /// Point this setting at another model on the same provider and API.
    pub fn set_model(&mut self, model: &str) -> Result<(), String> {
        if self.is_cli() {
            self.model = Some(model.to_string());
            return Ok(());
        }
        let id = ModelId { model, ..self.parsed()? }.to_id();
        self.id = id;
        Ok(())
    }
}

/// Every provider and model setting, and which model is the default.
/// `Models::default()` is empty: the values come from the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Models {
    /// Name of the default model.
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub providers: BTreeMap<String, Provider>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

impl Models {
    /// Read `models.json` text.
    pub fn from_json(text: &str) -> Result<Models, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// Write `models.json` text (pretty, so people can edit it by hand).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("models always serialise") + "\n"
    }

    /// The default model; the first one if `default` names none.
    pub fn default_model(&self) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.name == self.default).or(self.models.first())
    }

    pub fn get(&self, name: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ModelConfig> {
        self.models.iter_mut().find(|m| m.name == name)
    }

    /// Make `name` the default. False if there is no such model.
    pub fn set_default(&mut self, name: &str) -> bool {
        let known = self.get(name).is_some();
        if known {
            self.default = name.to_string();
        }
        known
    }

    /// The inference object for `model`: the API from its id's marker, the
    /// base URL and key variable from its own override or else its provider.
    pub fn inference(&self, model: &ModelConfig) -> Result<OpenAi, String> {
        if model.is_cli() {
            return cli_inference(model);
        }
        if let Some(kind) = &model.kind {
            return Err(format!("\"{}\": kind \"{kind}\" is not one there is (cli, or none for an HTTP model)", model.name));
        }
        let id = model.parsed()?;
        let provider = self.providers.get(id.provider);
        let base_url = model.base_url.clone().or_else(|| provider.map(|p| p.base_url.clone())).ok_or_else(|| {
            format!("\"{}\": provider \"{}\" is not in providers and the model sets no base_url", model.name, id.provider)
        })?;
        // The provider's key only goes to the provider's own server.
        let key_env = match &model.base_url {
            Some(_) => model.key_env.clone(),
            None => model.key_env.clone().or_else(|| provider.and_then(|p| p.key_env.clone())),
        };
        Ok(OpenAi { api: id.api, base_url, model: id.model.to_string(), key_env, context: model.context, sampling: Default::default() })
    }
}

/// Where a JEV decision model is asked: the base URL, the model id and
/// the key variable. Only the address: calling it is not built yet.
#[derive(Clone, Debug, PartialEq)]
pub struct JevEndpoint {
    pub base_url: String,
    pub model: String,
    pub key_env: Option<String>,
}

/// What is said when no model can answer jev.
pub const NO_JEV_ENDPOINT: &str = "no JEV endpoint configured: no model or provider in models.json says `\"jev\": true`";

impl Models {
    /// The JEV endpoint `model` names: one marked `jev` itself or by its
    /// provider. A CLI model never is (a CLI answers chat only).
    pub fn jev_endpoint(&self, model: &ModelConfig) -> Result<JevEndpoint, String> {
        if model.is_cli() {
            return Err(format!("\"{}\" is a CLI model: a CLI cannot answer jev; {NO_JEV_ENDPOINT}", model.name));
        }
        let id = model.parsed()?;
        let provider = self.providers.get(id.provider);
        if !model.jev && !provider.is_some_and(|p| p.jev) {
            return Err(format!("\"{}\": {NO_JEV_ENDPOINT}", model.name));
        }
        let ai = self.inference(model)?;
        Ok(JevEndpoint { base_url: ai.base_url, model: ai.model, key_env: ai.key_env })
    }

    /// The first model that can answer jev, if any.
    pub fn any_jev(&self) -> Result<(&ModelConfig, JevEndpoint), String> {
        self.models.iter().find_map(|m| self.jev_endpoint(m).ok().map(|e| (m, e))).ok_or_else(|| NO_JEV_ENDPOINT.to_string())
    }
}

/// A CLI model's inference object: Chat Completions to `cli:<provider>`,
/// no key, its context from the setting or the CLI's.
fn cli_inference(model: &ModelConfig) -> Result<OpenAi, String> {
    let provider = model
        .provider
        .as_deref()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| format!("\"{}\": a cli model needs a provider ({})", model.name, crate::cli::PROVIDERS.join(", ")))?;
    let cli = crate::cli::Cli::named(provider)
        .ok_or_else(|| format!("\"{}\": no CLI provider \"{provider}\" (there are {})", model.name, crate::cli::PROVIDERS.join(", ")))?;
    Ok(OpenAi {
        api: Api::ChatCompletions,
        base_url: crate::cli::base_url(provider, model.program.as_deref()),
        model: model.model.clone().unwrap_or_default(),
        key_env: None,
        context: model.context.or(Some(cli.context)),
        sampling: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
        "default": "local",
        "providers": { "openai": { "base_url": "https://api.openai.com/v1", "key_env": "OPENAI_API_KEY" } },
        "models": [
            { "name": "local", "id": "openai/c/qwen/qwen3-8b", "base_url": "http://127.0.0.1:1234/v1" },
            { "name": "gpt", "id": "openai/r/gpt-5-mini" }
        ]
    }"#;

    #[test]
    fn the_id_marks_the_api_and_the_model_may_contain_slashes() {
        let id = ModelId::parse("openai/c/qwen/qwen3-8b").unwrap();
        assert_eq!((id.provider, id.api, id.model), ("openai", Api::ChatCompletions, "qwen/qwen3-8b"));
        assert_eq!(id.to_id(), "openai/c/qwen/qwen3-8b");
        assert_eq!(ModelId::parse("openai/r/").unwrap().model, "", "no model chosen yet");
        assert!(ModelId::parse("openai/x/m").is_err());
        assert!(ModelId::parse("gpt-5").is_err());
    }

    #[test]
    fn a_model_without_overrides_goes_to_its_provider_and_overrides_win() {
        let models = Models::from_json(FILE).unwrap();
        let gpt = models.inference(models.get("gpt").unwrap()).unwrap();
        assert_eq!(
            gpt,
            OpenAi {
                api: Api::Responses,
                base_url: "https://api.openai.com/v1".into(),
                model: "gpt-5-mini".into(),
                key_env: Some("OPENAI_API_KEY".into()),
                context: None,
                sampling: Default::default(),
            }
        );
        let local = models.inference(models.default_model().unwrap()).unwrap();
        assert_eq!(local.base_url, "http://127.0.0.1:1234/v1");
        assert_eq!(local.key_env, None, "the provider's key is not sent to another server");
        assert_eq!(local.api, Api::ChatCompletions);
    }

    #[test]
    fn a_cli_model_points_at_its_cli_and_needs_no_id() {
        let models = Models::from_json(
            r#"{"models":[
                {"name":"claude/haiku","kind":"cli","provider":"claude","model":"haiku"},
                {"name":"codex/mine","kind":"cli","provider":"codex","model":"m","program":"/opt/codex","context":9000},
                {"name":"bad","kind":"cli","provider":"gemini"},
                {"name":"odd","kind":"pipe","id":"openai/c/m"}]}"#,
        )
        .unwrap();
        let ai = models.inference(models.get("claude/haiku").unwrap()).unwrap();
        assert_eq!((ai.base_url.as_str(), ai.model.as_str(), ai.key_env.clone(), ai.context), ("cli:claude", "haiku", None, Some(200_000)));
        let ai = models.inference(models.get("codex/mine").unwrap()).unwrap();
        assert_eq!((ai.base_url.as_str(), ai.context), ("cli:codex?program=/opt/codex", Some(9000)));
        assert!(models.inference(models.get("bad").unwrap()).unwrap_err().contains("gemini"));
        assert!(models.inference(models.get("odd").unwrap()).unwrap_err().contains("pipe"));
        let mut again = models.clone();
        again.get_mut("claude/haiku").unwrap().set_model("sonnet").unwrap();
        assert_eq!(again.get("claude/haiku").unwrap().model.as_deref(), Some("sonnet"));
        let json = again.to_json();
        assert!(!json.contains("\"id\": \"\""), "no empty id written: {json}");
        assert_eq!(Models::from_json(&json).unwrap(), again);
    }

    #[test]
    fn an_unknown_provider_without_a_base_url_is_an_error() {
        let models = Models::from_json(r#"{"models":[{"name":"x","id":"nowhere/c/m"}]}"#).unwrap();
        assert!(models.inference(&models.models[0]).unwrap_err().contains("nowhere"));
    }

    #[test]
    fn models_written_as_json_read_back_the_same_and_set_model_keeps_the_api() {
        let mut models = Models::from_json(FILE).unwrap();
        models.get_mut("gpt").unwrap().set_model("gpt-5").unwrap();
        assert_eq!(models.get("gpt").unwrap().id, "openai/r/gpt-5");
        assert!(models.set_default("gpt"));
        assert!(!models.set_default("nope"));
        let json = models.to_json();
        assert!(!json.contains("null"), "missing overrides are left out: {json}");
        assert_eq!(Models::from_json(&json).unwrap(), models);
        assert!(Models::from_json("{").is_err());
    }

    #[test]
    fn a_jev_endpoint_is_only_a_model_or_provider_marked_jev() {
        let models = Models::from_json(FILE).unwrap();
        assert_eq!(models.any_jev().unwrap_err(), NO_JEV_ENDPOINT, "none is marked");
        let marked = Models::from_json(r#"{
            "providers": { "typesafe": { "base_url": "https://api.typesafe.ai/v1", "key_env": "TYPESAFE_API_KEY", "jev": true } },
            "models": [
                { "name": "chat", "id": "openai/c/qwen3-8b", "base_url": "http://127.0.0.1:1234/v1" },
                { "name": "jev", "id": "typesafe/c/jev-latest" },
                { "name": "claude/haiku", "kind": "cli", "provider": "claude", "model": "haiku", "jev": true }
            ]
        }"#).unwrap();
        let (m, at) = marked.any_jev().unwrap();
        assert_eq!((m.name.as_str(), at.model.as_str(), at.key_env.as_deref()), ("jev", "jev-latest", Some("TYPESAFE_API_KEY")));
        assert!(marked.jev_endpoint(marked.get("claude/haiku").unwrap()).unwrap_err().contains("a CLI cannot answer jev"));
        assert!(marked.jev_endpoint(marked.get("chat").unwrap()).unwrap_err().contains(NO_JEV_ENDPOINT));
        assert!(!marked.to_json().contains("\"jev\": false"), "unset is left out");
    }
}
