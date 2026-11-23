//! Cards: the one UI element every backend object is shown as.
//!
//! A tool, an engine (model or session), later a model setting or a route:
//! each says what it is as a `Card` (an icon, a title, its kind, a few
//! lines) through `AsCard`. Its `Status` comes from whoever knows it (the
//! shell knows whether a program is installed or an engine is working),
//! so the object stays pure. Every shell draws a `Card` one way, so every
//! object looks the same and a new object only has to say what it is.

use crate::inference::Api;
use crate::models::{ModelConfig, Models};
use crate::session::Tool;
use crate::state::Engine;

/// What an object is doing, or whether it can.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    /// Nothing to say.
    #[default]
    Idle,
    /// Usable (a program found on PATH, an engine with an answer).
    Ready,
    /// Finished something, with what came of it (`3 models found`).
    Done(String),
    /// Busy right now: what it is doing.
    Working(String),
    /// The last thing it did went wrong: why.
    Failed(String),
    /// It cannot run: what is missing.
    Missing(String),
}

/// How a status should read at a glance; each shell maps it to a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Good,
    Busy,
    Bad,
}

impl Status {
    pub fn tone(&self) -> Tone {
        match self {
            Status::Idle => Tone::Neutral,
            Status::Ready | Status::Done(_) => Tone::Good,
            Status::Working(_) => Tone::Busy,
            Status::Failed(_) | Status::Missing(_) => Tone::Bad,
        }
    }

    /// One word for the status line.
    pub fn word(&self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Ready => "ready",
            Status::Done(_) => "ok",
            Status::Working(_) => "working",
            Status::Failed(_) => "failed",
            Status::Missing(_) => "missing",
        }
    }

    /// The detail, if any (`what`, `why`).
    pub fn detail(&self) -> Option<&str> {
        match self {
            Status::Working(s) | Status::Done(s) | Status::Failed(s) | Status::Missing(s) => Some(s),
            Status::Idle | Status::Ready => None,
        }
    }

    /// The word and the detail, for a single line.
    pub fn line(&self) -> String {
        match self.detail() {
            Some(d) => format!("{}: {d}", self.word()),
            None => self.word().to_string(),
        }
    }
}

/// The element: everything a shell needs to draw any object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Card {
    /// A glyph, used as it is (`✳`, `◎`).
    pub icon: String,
    pub title: String,
    /// What sort of object: `program`, `model`, `claude`, ...
    pub kind: String,
    pub status: Status,
    /// Short facts, one per line.
    pub lines: Vec<String>,
}

impl Card {
    pub fn with_status(mut self, status: Status) -> Card {
        self.status = status;
        self
    }

    pub fn with_icon(mut self, icon: impl Into<String>) -> Card {
        self.icon = icon.into();
        self
    }
}

/// An object that can be shown as a card.
pub trait AsCard {
    fn card(&self) -> Card;
}

impl AsCard for Tool {
    fn card(&self) -> Card {
        let program = self.start.first().cloned().unwrap_or_default();
        let resumes = if self.resume.is_empty() { "each turn starts fresh" } else { "continues its own session" };
        Card {
            icon: self.icon.clone(),
            title: self.name.clone(),
            kind: "program".into(),
            status: Status::Idle,
            lines: vec![self.description.clone(), format!("runs `{program}` · {resumes}")],
        }
    }
}

impl AsCard for Engine {
    fn card(&self) -> Card {
        let mut facts = vec![format!("{} message(s)", self.messages.len())];
        match &self.session {
            Some(s) => {
                facts.push(format!("{} metadata value(s)", self.meta.len()));
                facts.push(s.id.as_deref().map_or("not started".into(), |id| format!("session {}", id.chars().take(8).collect::<String>())));
            }
            None => facts.push(self.inference.model.clone()),
        }
        let mut lines = Vec::new();
        if !self.description.is_empty() {
            lines.push(self.description.clone());
        }
        lines.push(facts.join(" · "));
        Card { icon: "✦".into(), title: self.name.clone(), kind: self.kind().to_string(), status: Status::Idle, lines }
    }
}

/// A model setting, with the file it is in (which says the default and
/// the providers its id points to).
impl AsCard for (&Models, &ModelConfig) {
    fn card(&self) -> Card {
        let (models, m) = *self;
        let mut lines = vec![m.id.clone()];
        let status = match models.inference(m) {
            Ok(ai) if m.is_cli() => {
                // A CLI model (`crate::cli`): the program, not a server.
                lines[0] = format!("cli · {}", m.provider.as_deref().unwrap_or_default());
                lines.push(format!("runs {} one-shot", m.program.as_deref().unwrap_or(m.provider.as_deref().unwrap_or_default())));
                lines.push(if ai.model.is_empty() { "no model chosen".into() } else { format!("model {}", ai.model) });
                lines.push(format!("context {} tokens", crate::context::short(ai.context.unwrap_or_default() as usize)));
                if ai.model.is_empty() { Status::Missing("no model chosen".into()) } else { Status::Idle }
            }
            Ok(ai) => {
                let api = match ai.api {
                    Api::ChatCompletions => "chat completions",
                    Api::Responses => "responses",
                };
                let key = ai.key_env.as_deref().map(|k| format!(" · key ${k}")).unwrap_or_default();
                lines.push(format!("{api} · {}{key}", ai.base_url));
                lines.push(if ai.model.is_empty() { "no model chosen (Detect)".into() } else { format!("model {}", ai.model) });
                lines.push(match ai.context {
                    Some(n) => format!("context {} tokens", crate::context::short(n as usize)),
                    None => "context: asked of the server at the first call".into(),
                });
                if ai.model.is_empty() { Status::Missing("no model chosen".into()) } else { Status::Idle }
            }
            Err(e) => Status::Missing(e),
        };
        Card {
            icon: m.icon.clone().unwrap_or_else(|| "◇".into()),
            title: m.name.clone(),
            kind: if m.name == models.default { "default".into() } else { "model".into() },
            status,
            lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;
    use crate::session::Sessions;

    #[test]
    fn a_tool_and_an_engine_become_the_same_element() {
        let sessions = Sessions::from_json(include_str!("../../../sessions.json")).unwrap();
        let tool = sessions.get("claude").unwrap().card().with_status(Status::Ready);
        assert_eq!((tool.kind.as_str(), tool.status.tone()), ("program", Tone::Good));
        assert!(!tool.icon.is_empty(), "every program in sessions.json has an icon");
        assert!(tool.lines[1].contains("continues its own session"));

        let engine = Engine::new("Engine 1", "", OpenAi::default()).card().with_status(Status::Failed("no model".into()));
        assert_eq!(engine.kind, "model");
        assert_eq!(engine.status.line(), "failed: no model");
        assert_eq!(engine.status.tone(), Tone::Bad);
    }

    #[test]
    fn a_model_setting_is_a_card_too() {
        let models = Models::from_json(include_str!("../../../models.json")).unwrap();
        let m = models.default_model().unwrap();
        let card = (&models, m).card();
        assert_eq!(card.kind, "default");
        assert_eq!(card.lines[0], m.id);
        assert_eq!(Status::Done("3 models found".into()).line(), "ok: 3 models found");
    }
}
