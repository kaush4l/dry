//! Inference: how an engine asks a model for a reply.
//!
//! `Inference` is the abstract trait. `OpenAi` is the first object that
//! implements it: any server speaking the OpenAI wire format (OpenAI itself,
//! LM Studio, Ollama, vLLM, ...) through either of its two APIs:
//!
//! - Chat Completions: `POST {base}/chat/completions` with `messages`,
//!   reply in `choices[0].message.content`.
//! - Responses: `POST {base}/responses` with `input`, reply in the
//!   `output_text` parts of `output[]`.
//!
//! Core stays pure: an `Inference` never opens a connection. It *builds* the
//! request (`request`) and *reads* the reply body (`reply`). A shell or the
//! server does the actual HTTP in between, so all of this is testable
//! without a network.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Who said a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    /// What the engine's tools returned. Sent to the model as a user
    /// message (plain chat has no tool turn without call ids).
    Tool,
    /// A summary of the history before it (`crate::context`): kept in
    /// the log where the cut was made, never sent as a message itself;
    /// the prompt carries the newest one in the history slot.
    Summary,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User | Role::Tool | Role::Summary => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// One message in the conversation sent to the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// An HTTP request ready to send: POST `body` (JSON) to `url`. When
/// `key_env` is set, the sender reads that environment variable and adds
/// `Authorization: Bearer <value>`; the key itself is never in here.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub url: String,
    pub body: String,
    pub key_env: Option<String>,
}

/// Why a reply could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum InferenceError {
    /// The body was not JSON.
    NotJson(String),
    /// The server answered with an error object.
    Api(String),
    /// JSON, but no reply text where this API puts it.
    NoText,
}

/// The abstract inference contract. Anything that can turn messages into a
/// request and a response body into text is an inference.
pub trait Inference {
    /// Build the request for `messages`.
    fn request(&self, messages: &[Message]) -> Request;
    /// Read the reply text out of a response body.
    fn reply(&self, body: &str) -> Result<String, InferenceError>;
    /// One line for people: which API, model and server.
    fn describe(&self) -> String;
}

/// Which of the two OpenAI APIs to call. Written as `chat-completions` /
/// `responses` in files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Api {
    #[default]
    ChatCompletions,
    Responses,
}

impl Api {
    /// The name used in `state.txt`.
    pub fn as_str(self) -> &'static str {
        match self {
            Api::ChatCompletions => "chat-completions",
            Api::Responses => "responses",
        }
    }

    pub fn parse(name: &str) -> Option<Api> {
        match name {
            "chat-completions" => Some(Api::ChatCompletions),
            "responses" => Some(Api::Responses),
            _ => None,
        }
    }
}

/// An OpenAI-compatible endpoint. Plain data, so an engine can hold it,
/// copy it, compare it and save it. `OpenAi::default()` is empty (no
/// server, no model): real values come from `models.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenAi {
    pub api: Api,
    /// Up to and including `/v1`, no trailing slash.
    pub base_url: String,
    pub model: String,
    /// Name of the environment variable holding the API key; `None` for a
    /// local server that needs no key.
    pub key_env: Option<String>,
    /// The model's context window in tokens, when the setting says it
    /// (`models.json`, `engine.md`). `None`: ask the server
    /// (`crate::context::probes`), else the assumed default.
    pub context: Option<u32>,
    /// Its sampling settings (temperature, top_p, ...; `crate::sampling`).
    /// Only the set ones go into a request; none set: the server's defaults.
    pub sampling: crate::sampling::Sampling,
}

impl Inference for OpenAi {
    fn request(&self, messages: &[Message]) -> Request {
        let turns: Vec<Value> = messages.iter().map(|m| json!({ "role": m.role.as_str(), "content": m.text })).collect();
        let (path, mut body) = match self.api {
            Api::ChatCompletions => ("chat/completions", json!({ "model": self.model, "messages": turns })),
            Api::Responses => ("responses", json!({ "model": self.model, "input": turns })),
        };
        if let Some(body) = body.as_object_mut() {
            self.sampling.apply(body, self.api);
        }
        // A CLI model (`crate::cli`): the transport runs the program the
        // base names; the body is the same Chat Completions body.
        let url = if crate::cli::is_cli(&self.base_url) {
            self.base_url.clone()
        } else {
            format!("{}/{path}", self.base_url.trim_end_matches('/'))
        };
        Request { url, body: body.to_string(), key_env: self.key_env.clone() }
    }

    fn reply(&self, body: &str) -> Result<String, InferenceError> {
        let v: Value = serde_json::from_str(body).map_err(|e| InferenceError::NotJson(e.to_string()))?;
        if let Some(message) = v.pointer("/error/message").and_then(Value::as_str) {
            return Err(InferenceError::Api(message.to_string()));
        }
        let text = match self.api {
            Api::ChatCompletions => v.pointer("/choices/0/message/content").and_then(Value::as_str).map(str::to_string),
            Api::Responses => responses_text(&v),
        };
        text.ok_or(InferenceError::NoText)
    }

    fn describe(&self) -> String {
        format!("{} · {} @ {}", self.api.as_str(), self.model, self.base_url)
    }
}

/// The model ids a server offers, from its `GET {base}/models` body
/// (`{"data":[{"id":"..."}]}`, the same on both APIs).
pub fn parse_model_ids(body: &str) -> Result<Vec<String>, InferenceError> {
    let v: Value = serde_json::from_str(body).map_err(|e| InferenceError::NotJson(e.to_string()))?;
    if let Some(message) = v.pointer("/error/message").and_then(Value::as_str) {
        return Err(InferenceError::Api(message.to_string()));
    }
    let ids = v.get("data").and_then(Value::as_array).ok_or(InferenceError::NoText)?;
    Ok(ids.iter().filter_map(|m| m.get("id")?.as_str()).map(str::to_string).collect())
}

/// The Responses API puts text in `output[]` items of type `message`, each
/// with `content[]` parts of type `output_text`. Some servers also add a
/// top-level `output_text` shortcut; use it when present.
fn responses_text(v: &Value) -> Option<String> {
    if let Some(text) = v.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    let parts: Vec<&str> = v
        .get("output")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|item| item.get("content")?.as_array())
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text")?.as_str())
        .collect();
    if parts.is_empty() { None } else { Some(parts.concat()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Vec<Message> {
        vec![Message { role: Role::User, text: "hi \"there\"".into() }]
    }

    #[test]
    fn chat_completions_posts_messages_and_reads_the_first_choice() {
        let ai = OpenAi { base_url: "http://x/v1/".into(), model: "m".into(), ..OpenAi::default() };
        let req = ai.request(&hello());
        assert_eq!(req.url, "http://x/v1/chat/completions");
        let body: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(body["messages"][0]["content"], "hi \"there\"");
        assert_eq!(body["model"], "m");

        let reply = r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        assert_eq!(ai.reply(reply), Ok("hello".into()));
    }

    #[test]
    fn responses_posts_input_and_joins_the_output_text_parts() {
        let ai = OpenAi { api: Api::Responses, ..OpenAi::default() };
        let req = ai.request(&hello());
        assert!(req.url.ends_with("/responses"));
        let body: Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(body["input"][0]["role"], "user");

        let reply = r#"{"output":[{"type":"reasoning"},{"type":"message","content":[{"type":"output_text","text":"hel"},{"type":"output_text","text":"lo"}]}]}"#;
        assert_eq!(ai.reply(reply), Ok("hello".into()));
    }

    #[test]
    fn a_request_carries_only_the_sampling_settings_that_are_set() {
        let mut ai = OpenAi { base_url: "http://x/v1".into(), model: "m".into(), ..OpenAi::default() };
        let plain: Value = serde_json::from_str(&ai.request(&hello()).body).unwrap();
        let mut keys: Vec<&String> = plain.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["messages", "model"], "nothing set: the body as before");
        ai.sampling.set_text("temperature", "0.2").unwrap();
        ai.sampling.set_text("max_tokens", "64").unwrap();
        let chat: Value = serde_json::from_str(&ai.request(&hello()).body).unwrap();
        assert_eq!((chat["temperature"].clone(), chat["max_tokens"].clone(), chat["model"].clone()), (json!(0.2), json!(64), json!("m")));
        assert!(chat.get("top_p").is_none());
        ai.api = Api::Responses;
        let responses: Value = serde_json::from_str(&ai.request(&hello()).body).unwrap();
        assert_eq!(responses["max_output_tokens"], 64);
        assert!(responses.get("max_tokens").is_none() && responses.get("input").is_some());
    }

    #[test]
    fn a_model_list_gives_its_ids() {
        let body = r#"{"object":"list","data":[{"id":"qwen3-8b","object":"model"},{"id":"nomic-embed"}]}"#;
        assert_eq!(parse_model_ids(body), Ok(vec!["qwen3-8b".to_string(), "nomic-embed".to_string()]));
    }

    #[test]
    fn an_error_object_or_a_body_with_no_text_is_an_error_not_an_empty_reply() {
        let ai = OpenAi::default();
        assert_eq!(ai.reply(r#"{"error":{"message":"bad key"}}"#), Err(InferenceError::Api("bad key".into())));
        assert_eq!(ai.reply(r#"{"choices":[]}"#), Err(InferenceError::NoText));
        assert!(matches!(ai.reply("not json"), Err(InferenceError::NotJson(_))));
    }
}
