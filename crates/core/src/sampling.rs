//! Sampling: how an engine's model picks its words (temperature, top_p,
//! ...). Every setting is optional; one that is not set is not sent, so the
//! server uses its own default. An engine with nothing set sends exactly
//! the request it sent before these settings existed.
//!
//! The settings live with the engine, in its inference (`OpenAi::sampling`),
//! and are kept in `spec/engine.md` as `key: value` lines (the value as
//! JSON: `temperature: 0.2`, `stop: ["END"]`). A key the list below does
//! not know, but the server takes, is a free one: `param.<name>`
//! (`param.min_p: 0.05`), sent as `<name>`.
//!
//! Pure: checking a value and adding the set ones to a request body.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::inference::Api;

/// The prefix of a free key: `param.min_p` is sent as `min_p`.
pub const FREE: &str = "param.";

/// What kind of value a setting takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A number from `min` to `max`, both included.
    Number { min: f64, max: f64 },
    /// A whole number from `min` to `max`, both included.
    Whole { min: i64, max: i64 },
    /// One to four texts (one text is read as a list of one).
    Texts,
}

/// One setting an engine can change.
pub struct Key {
    pub name: &'static str,
    pub kind: Kind,
    /// What it does, for people and for the tool description.
    pub meaning: &'static str,
    /// Which servers honour it (as their documentation says).
    pub honoured_by: &'static str,
    /// Does the Responses API take it? (Chat Completions takes every key.)
    pub responses: bool,
}

/// Every setting with a name of its own. Anything else is a free key.
pub const KEYS: &[Key] = &[
    Key {
        name: "temperature",
        kind: Kind::Number { min: 0.0, max: 2.0 },
        meaning: "randomness: 0 = most deterministic (the likeliest word each time), around 1 = creative, up to 2 = wild",
        honoured_by: "OpenAI chat and responses (not reasoning models such as gpt-5 / o-series, which refuse it), LM Studio, Ollama, vLLM, llama.cpp",
        responses: true,
    },
    Key {
        name: "top_p",
        kind: Kind::Number { min: 0.0, max: 1.0 },
        meaning: "nucleus sampling: only the likeliest words whose chances add up to this; lower = safer",
        honoured_by: "OpenAI chat and responses (not reasoning models), LM Studio, Ollama, vLLM, llama.cpp",
        responses: true,
    },
    Key {
        name: "top_k",
        kind: Kind::Whole { min: 0, max: 1_000_000 },
        meaning: "only the k likeliest words (0 = no limit on llama.cpp and LM Studio)",
        honoured_by: "LM Studio, vLLM, llama.cpp; not OpenAI (refused); Ollama's /v1 ignores it",
        responses: false,
    },
    Key {
        name: "max_tokens",
        kind: Kind::Whole { min: 1, max: 10_000_000 },
        meaning: "the most tokens one answer may have; sent as max_tokens (chat completions) or max_output_tokens (responses)",
        honoured_by: "LM Studio, Ollama, vLLM, llama.cpp, OpenAI responses (as max_output_tokens); OpenAI chat reasoning models refuse max_tokens and want param.max_completion_tokens",
        responses: true,
    },
    Key {
        name: "seed",
        kind: Kind::Whole { min: i64::MIN, max: i64::MAX },
        meaning: "a fixed seed: the same prompt and settings give the same answer (best effort on OpenAI)",
        honoured_by: "OpenAI chat (best effort), LM Studio, Ollama, vLLM, llama.cpp; not the Responses API",
        responses: false,
    },
    Key {
        name: "stop",
        kind: Kind::Texts,
        meaning: "up to four texts; the answer ends before any of them",
        honoured_by: "OpenAI chat (not the newest reasoning models), LM Studio, Ollama, vLLM, llama.cpp; not the Responses API",
        responses: false,
    },
    Key {
        name: "presence_penalty",
        kind: Kind::Number { min: -2.0, max: 2.0 },
        meaning: "above 0 pushes toward new topics (a word that appeared at all is less likely)",
        honoured_by: "OpenAI chat, LM Studio, Ollama, vLLM, llama.cpp; not the Responses API",
        responses: false,
    },
    Key {
        name: "frequency_penalty",
        kind: Kind::Number { min: -2.0, max: 2.0 },
        meaning: "above 0 means less repetition (a word is less likely the more often it appeared)",
        honoured_by: "OpenAI chat, LM Studio, Ollama, vLLM, llama.cpp; not the Responses API",
        responses: false,
    },
    Key {
        name: "repeat_penalty",
        kind: Kind::Number { min: 0.0, max: 2.0 },
        meaning: "llama.cpp's repetition penalty: 1 = off, above 1 = less repetition",
        honoured_by: "LM Studio, llama.cpp; vLLM calls it repetition_penalty (use param.repetition_penalty); not OpenAI; Ollama's /v1 ignores it",
        responses: false,
    },
];

/// Body keys the request itself owns; no setting may replace them.
const RESERVED: &[&str] = &["model", "messages", "input", "stream", "tools", "tool_choice"];

/// The known setting called `name`.
pub fn key(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.name == name)
}

/// An engine's sampling settings: only the set ones, by name (`temperature`,
/// or `param.<name>` for a free key), each as the JSON value sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampling(BTreeMap<String, Value>);

impl Sampling {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The set settings, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// Set `name` to `value` (checked first; see `check`). Returns the
    /// value as it will be sent.
    pub fn set(&mut self, name: &str, value: &Value) -> Result<Value, String> {
        let v = check(name, value)?;
        self.0.insert(name.to_string(), v.clone());
        Ok(v)
    }

    /// Set `name` from text, as typed on a command line or written in
    /// `engine.md`: JSON when it reads as JSON, else the text itself.
    pub fn set_text(&mut self, name: &str, text: &str) -> Result<Value, String> {
        self.set(name, &read_value(text))
    }

    /// Back to the server's default. True if it was set.
    pub fn unset(&mut self, name: &str) -> bool {
        self.0.remove(name).is_some()
    }

    /// Add every set setting to a request `body` (a JSON object) for `api`.
    /// Nothing set: the body is left exactly as it was.
    pub fn apply(&self, body: &mut Map<String, Value>, api: Api) {
        for (name, value) in &self.0 {
            body.insert(wire_name(name, api).to_string(), value.clone());
        }
    }

    /// The `engine.md` lines, one `name: <json>` per set setting.
    pub fn to_lines(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}: {v}\n")).collect()
    }

    /// Settings `api` does not take (the Responses API has no seed, stop
    /// or penalties). OpenAI refuses them; a local server may ignore them.
    pub fn not_for(&self, api: Api) -> Vec<&str> {
        match api {
            Api::ChatCompletions => Vec::new(),
            Api::Responses => self.0.keys().filter(|k| key(k).is_some_and(|k| !k.responses)).map(String::as_str).collect(),
        }
    }
}

/// Is `name` a sampling line in `engine.md` (a known key or `param.<name>`)?
pub fn owns(name: &str) -> bool {
    key(name).is_some() || name.starts_with(FREE)
}

/// The name a setting is sent as.
fn wire_name(name: &str, api: Api) -> &str {
    match (name, api) {
        ("max_tokens", Api::Responses) => "max_output_tokens",
        _ => name.strip_prefix(FREE).unwrap_or(name),
    }
}

/// Text to a value: JSON when it reads as JSON (`0.2`, `["a"]`, `true`),
/// else the text itself as a string.
pub fn read_value(text: &str) -> Value {
    let text = text.trim();
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

/// A number given as a number or as text.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Check `value` for setting `name`, and give it back as it will be sent.
pub fn check(name: &str, value: &Value) -> Result<Value, String> {
    if let Some(free) = name.strip_prefix(FREE) {
        if free.is_empty() || !free.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("{name}: a free key is param.<name>, the name in letters, digits and _"));
        }
        if RESERVED.contains(&free) {
            return Err(format!("{name}: `{free}` is part of the request itself and cannot be a setting"));
        }
        if key(free).is_some() {
            return Err(format!("{name}: `{free}` is a setting of its own; set {free}"));
        }
        if value.is_null() {
            return Err(format!("{name}: no value (to go back to the default, unset it)"));
        }
        return Ok(value.clone());
    }
    let Some(k) = key(name) else {
        let names: Vec<&str> = KEYS.iter().map(|k| k.name).collect();
        return Err(format!(
            "unknown setting `{name}`; the settings are {}, or param.<name> for another the server takes",
            names.join(", ")
        ));
    };
    match k.kind {
        Kind::Number { min, max } => {
            let n = number(value).filter(|n| n.is_finite()).ok_or_else(|| format!("{name} is a number, not {value}"))?;
            if !(min..=max).contains(&n) {
                return Err(format!("{name} is from {min} to {max}, not {n}"));
            }
            serde_json::Number::from_f64(n).map(Value::Number).ok_or_else(|| format!("{name}: {n} is not a number"))
        }
        Kind::Whole { min, max } => {
            let n = number(value).ok_or_else(|| format!("{name} is a whole number, not {value}"))?;
            if n.fract() != 0.0 {
                return Err(format!("{name} is a whole number, not {n}"));
            }
            // Read exactly when it is written as an integer (a big seed).
            let exact = match value {
                Value::Number(v) => v.as_i64(),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            let n = exact.unwrap_or(n as i64);
            if !(min..=max).contains(&n) {
                return Err(format!("{name} is from {min} to {max}, not {n}"));
            }
            Ok(Value::from(n))
        }
        Kind::Texts => {
            let texts: Vec<String> = match value {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a
                    .iter()
                    .map(|v| v.as_str().map(str::to_string))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("{name} is a list of texts, not {value}"))?,
                other => return Err(format!("{name} is a text or a list of texts, not {other}")),
            };
            if texts.is_empty() || texts.len() > 4 || texts.iter().any(String::is_empty) {
                return Err(format!("{name} is one to four texts, none empty; got {}", texts.len()));
            }
            Ok(Value::from(texts))
        }
    }
}

/// One line per known setting: its name, range and meaning, and who
/// honours it. For tool descriptions and `dry engine keys`.
pub fn describe_keys() -> String {
    KEYS.iter()
        .map(|k| {
            let range = match k.kind {
                Kind::Number { min, max } => format!("{min} to {max}"),
                Kind::Whole { min: i64::MIN, .. } => "a whole number".into(),
                Kind::Whole { min, max } => format!("whole, {min} to {max}"),
                Kind::Texts => "1 to 4 texts".into(),
            };
            format!("- {} ({range}): {}. Honoured by: {}.", k.name, k.meaning, k.honoured_by)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn values_are_checked_against_their_ranges() {
        let mut s = Sampling::default();
        assert_eq!(s.set_text("temperature", "0.2"), Ok(json!(0.2)));
        assert_eq!(s.set("temperature", &json!("1.5")), Ok(json!(1.5)), "a number as text");
        assert!(s.set_text("temperature", "2.5").unwrap_err().contains("0 to 2"));
        assert!(s.set_text("temperature", "-0.1").is_err());
        assert!(s.set_text("temperature", "warm").is_err());
        assert!(s.set_text("top_p", "1.1").is_err());
        assert_eq!(s.set_text("top_p", "1"), Ok(json!(1.0)));
        assert_eq!(s.set_text("top_k", "40"), Ok(json!(40)));
        assert!(s.set_text("top_k", "4.5").is_err());
        assert!(s.set_text("max_tokens", "0").is_err());
        assert_eq!(s.set_text("seed", "-9007199254740993"), Ok(json!(-9_007_199_254_740_993_i64)), "exact past 2^53");
        assert!(s.set_text("presence_penalty", "-2.1").is_err());
        assert!(s.set_text("repeat_penalty", "1.1").is_ok());
        assert_eq!(s.set_text("stop", "END"), Ok(json!(["END"])));
        assert_eq!(s.set_text("stop", r#"["a","\n\n"]"#), Ok(json!(["a", "\n\n"])));
        assert!(s.set_text("stop", r#"["a","b","c","d","e"]"#).is_err());
        assert!(s.set_text("stop", r#"[""]"#).is_err());
        assert!(s.set_text("temprature", "1").unwrap_err().contains("param.<name>"), "a typo is not a free key");
    }

    #[test]
    fn a_free_key_takes_any_value_but_not_the_requests_own_keys() {
        let mut s = Sampling::default();
        assert_eq!(s.set_text("param.min_p", "0.05"), Ok(json!(0.05)));
        assert_eq!(s.set_text("param.mirostat_mode", "mirostat"), Ok(json!("mirostat")));
        assert!(s.set_text("param.model", "x").is_err());
        assert!(s.set_text("param.messages", "[]").is_err());
        assert!(s.set_text("param.temperature", "1").is_err());
        assert!(s.set_text("param.", "1").is_err());
        assert!(s.set_text("param.a-b", "1").is_err());
        assert!(owns("param.min_p") && owns("seed") && !owns("model") && !owns("context"));
    }

    #[test]
    fn only_the_set_keys_go_into_a_body_and_max_tokens_follows_the_api() {
        let mut body = Map::new();
        Sampling::default().apply(&mut body, Api::ChatCompletions);
        assert!(body.is_empty(), "nothing set, nothing sent");

        let mut s = Sampling::default();
        s.set_text("max_tokens", "256").unwrap();
        s.set_text("param.min_p", "0.05").unwrap();
        s.set_text("seed", "7").unwrap();
        let mut chat = Map::new();
        s.apply(&mut chat, Api::ChatCompletions);
        assert_eq!(Value::Object(chat), json!({ "max_tokens": 256, "min_p": 0.05, "seed": 7 }));
        let mut responses = Map::new();
        s.apply(&mut responses, Api::Responses);
        assert_eq!(Value::Object(responses), json!({ "max_output_tokens": 256, "min_p": 0.05, "seed": 7 }));
        assert_eq!(s.not_for(Api::Responses), ["seed"]);
        assert!(s.not_for(Api::ChatCompletions).is_empty());

        assert!(s.unset("seed"));
        assert!(!s.unset("seed"));
        assert_eq!(s.to_lines(), "max_tokens: 256\nparam.min_p: 0.05\n");
    }

    #[test]
    fn every_key_is_described() {
        let text = describe_keys();
        for k in KEYS {
            assert!(text.contains(&format!("- {} (", k.name)), "{}", k.name);
        }
    }
}
