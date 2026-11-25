//! This browser's own settings (`local/settings.json` in its store): the
//! model server and model new engines use, an optional key, and the
//! theme. Pure: read and written as JSON text.

use dry_core::inference::{Api, OpenAi};
use serde_json::{Value, json};

/// LM Studio on this machine, its default port.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:1234/v1";

/// The local model servers asked on a first visit, in order, by name and
/// endpoint: each one's own default port. `localhost`, not `127.0.0.1`:
/// a hosted (https) page may call `http://localhost` (docs/browser.md).
pub const LOCAL_SERVERS: &[(&str, &str)] =
    &[("LM Studio", DEFAULT_ENDPOINT), ("oMLX", "http://localhost:8873/v1"), ("Ollama", "http://localhost:11434/v1")];

/// Where the settings live in the store.
pub const PATH: &str = "local/settings.json";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Up to and including `/v1`, no trailing slash.
    pub endpoint: String,
    pub model: String,
    pub api: Api,
    /// Only for a cloud server. Kept in this browser's store, never in the
    /// bundle, never drawn back.
    pub key: Option<String>,
    /// The theme chosen here (`None`: app.md's `theme:`, else `default`).
    pub theme: Option<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { endpoint: DEFAULT_ENDPOINT.into(), model: String::new(), api: Api::ChatCompletions, key: None, theme: None }
    }
}

impl Settings {
    pub fn from_json(text: &str) -> Settings {
        let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let d = Settings::default();
        Settings {
            endpoint: s("endpoint").map(|e| clean_endpoint(&e)).filter(|e| !e.is_empty()).unwrap_or(d.endpoint),
            model: s("model").unwrap_or_default(),
            api: s("api").and_then(|a| Api::parse(&a)).unwrap_or_default(),
            key: s("key").filter(|k| !k.is_empty()),
            theme: s("theme").filter(|t| !t.is_empty()),
        }
    }

    pub fn to_json(&self) -> String {
        let mut v = json!({ "endpoint": self.endpoint, "model": self.model, "api": self.api.as_str() });
        if let Some(k) = &self.key {
            v["key"] = json!(k);
        }
        if let Some(t) = &self.theme {
            v["theme"] = json!(t);
        }
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    /// The inference new engines get: this server and model. No key name
    /// (`key_env`): the browser has no environment; the key, if any, is
    /// added by the sender.
    pub fn inference(&self) -> OpenAi {
        OpenAi { api: self.api, base_url: self.endpoint.clone(), model: self.model.clone(), ..OpenAi::default() }
    }

    /// The model setting shown in Settings → Models.
    pub fn models_json(&self) -> String {
        json!({
            "default": "browser",
            "models": [{ "name": "browser", "id": format!("openai/{}/{}", if self.api == Api::Responses { "r" } else { "c" }, if self.model.is_empty() { "none" } else { &self.model }), "base_url": self.endpoint }]
        })
        .to_string()
    }
}

/// An endpoint as typed: trimmed, no trailing `/`, `/v1` added to a bare
/// `http://host:port`.
pub fn clean_endpoint(e: &str) -> String {
    let e = e.trim().trim_end_matches('/');
    if e.is_empty() {
        return String::new();
    }
    let has_path = e.split_once("://").is_some_and(|(_, rest)| rest.contains('/'));
    if has_path { e.to_string() } else { format!("{e}/v1") }
}

/// What a redraw puts back in a Settings → Models field: the text
/// `typed` in it, but only while the app draws the same value as before
/// (`drawn_before`, `drawn_now`: the field's `value` attribute, `None`
/// for the key, which is never drawn). A value the app changed since (a
/// model picked from the list, the first one Detect took) is shown as
/// drawn, so Save does not put the old text back over it.
pub fn kept_field<'a>(typed: &'a str, drawn_before: Option<&str>, drawn_now: Option<&str>) -> Option<&'a str> {
    (drawn_before == drawn_now).then_some(typed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_back_and_fall_back() {
        let s = Settings { model: "m".into(), key: Some("k".into()), theme: Some("cat".into()), ..Settings::default() };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json("not json"), Settings::default());
        assert_eq!(clean_endpoint(" http://127.0.0.1:1234/ "), "http://127.0.0.1:1234/v1");
        assert_eq!(clean_endpoint("https://api.example.com/v1/"), "https://api.example.com/v1");
        let ai = s.inference();
        assert_eq!((ai.base_url.as_str(), ai.model.as_str(), ai.key_env), (DEFAULT_ENDPOINT, "m", None));
        assert!(dry_core::models::Models::from_json(&s.models_json()).is_ok());
    }

    /// A redraw keeps what was typed in a field only while the app still
    /// draws the value it drew before; a value the app changed (a picked
    /// model, the first one Detect took) is shown, not typed over.
    #[test]
    fn a_redraw_keeps_typed_text_but_shows_a_value_the_app_changed() {
        // Typing an endpoint, then Detect: nothing saved yet, the text stays.
        assert_eq!(kept_field("http://other:1/v1", Some("http://localhost:1234/v1"), Some("http://localhost:1234/v1")), Some("http://other:1/v1"));
        // A model picked from the list: the field shows it, not the old text.
        assert_eq!(kept_field("model-a", Some("model-a"), Some("model-b")), None);
        // Detect took the first model while the field was empty.
        assert_eq!(kept_field("", Some(""), Some("model-a")), None);
        // The key field is never drawn back: what was typed stays.
        assert_eq!(kept_field("sk-typed", None, None), Some("sk-typed"));
    }
}
