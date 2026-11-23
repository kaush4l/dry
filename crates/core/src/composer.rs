//! The composer: the one text area the whole app types into. A page puts it
//! down with `{ "type": "composer" }`; it is an input, a send button and a
//! row of options, the same on every page and in every shell. Where the
//! text goes is decided by the page, not the composer: on an engine's page
//! it goes to that engine; anywhere else it starts a new engine.
//!
//! Some options need something the runtime may not have (a microphone and
//! a speech-to-text program, for voice). Those are *capabilities*: each
//! shell checks them once at start (`Capabilities`), and the composer
//! shows an option it cannot use as off, with the reason, instead of
//! hiding it or failing when pressed.
//!
//! ```json
//! { "type": "composer", "actions": ["send", "voice", "clear"] }
//! ```
//!
//! Pure: no I/O. Checking a capability is the shell's job.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::card::Status;

/// One button on the composer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// Send the text (Enter does the same).
    Send,
    /// Speak instead of typing: record, turn the speech into text, put it
    /// in the box.
    Voice,
    /// Empty the box.
    Clear,
}

impl Action {
    pub const ALL: [Action; 3] = [Action::Send, Action::Voice, Action::Clear];

    pub fn icon(self) -> &'static str {
        match self {
            Action::Send => "↑",
            Action::Voice => "●",
            Action::Clear => "✕",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Send => "Send",
            Action::Voice => "Voice",
            Action::Clear => "Clear",
        }
    }

    /// What the runtime must have for this button to work.
    pub fn needs(self) -> Option<Capability> {
        match self {
            Action::Voice => Some(Capability::Voice),
            _ => None,
        }
    }

    /// Does it only make sense when the text goes to an engine that
    /// exists (not on a page that would start a new one)? None does now:
    /// the response format is the engine's setting, not a composer button.
    pub fn needs_engine(self) -> bool {
        false
    }
}

/// The buttons a composer gets when its page does not name them.
pub fn default_actions() -> Vec<Action> {
    Action::ALL.to_vec()
}

/// Something the runtime may or may not offer. Shells check each at start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    /// Record from a microphone and turn it into text.
    Voice,
}

impl Capability {
    pub const ALL: [Capability; 1] = [Capability::Voice];

    pub fn name(self) -> &'static str {
        match self {
            Capability::Voice => "voice",
        }
    }
}

/// What this runtime can do: each capability's status, as the shell found
/// it. One not checked counts as missing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Capabilities(BTreeMap<Capability, Status>);

impl Capabilities {
    pub fn set(&mut self, c: Capability, status: Status) {
        self.0.insert(c, status);
    }

    pub fn status(&self, c: Capability) -> Status {
        self.0.get(&c).cloned().unwrap_or_else(|| Status::Missing("not checked in this runtime".into()))
    }

    pub fn has(&self, c: Capability) -> bool {
        self.status(c) == Status::Ready
    }

    pub fn iter(&self) -> impl Iterator<Item = (Capability, &Status)> {
        self.0.iter().map(|(c, s)| (*c, s))
    }
}

/// One button as it should be drawn: on, or off and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub action: Action,
    pub enabled: bool,
    pub why: Option<String>,
}

/// The buttons a composer with `actions` draws in this runtime, for a page
/// with (`engine`) or without an engine. Send and Clear are always on;
/// one needing a capability the runtime lacks is off with its reason; one
/// needing an engine is left out where there is none.
pub fn buttons(actions: &[Action], caps: &Capabilities, engine: bool) -> Vec<Button> {
    actions
        .iter()
        .filter(|a| engine || !a.needs_engine())
        .map(|&action| match action.needs().map(|c| caps.status(c)) {
            Some(Status::Ready) | None => Button { action, enabled: true, why: None },
            Some(Status::Missing(why) | Status::Failed(why)) => Button { action, enabled: false, why: Some(why) },
            Some(other) => Button { action, enabled: false, why: Some(other.word().to_string()) },
        })
        .collect()
}

/// How the runtime turns speech into text: two programs run one after the
/// other, from `voice.json`. `{file}` is the recording, `{dir}` its folder,
/// `{seconds}` how long to record. The transcriber writes `<file stem>.txt`
/// into `{dir}`.
///
/// `check` (optional) lists the input devices; voice is ready only if a
/// device (a `[0]` line) is listed after the line holding `check_after`.
/// That finds a microphone without recording from it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    pub record: Vec<String>,
    pub transcribe: Vec<String>,
    #[serde(default = "seconds")]
    pub seconds: u32,
    #[serde(default)]
    pub check: Vec<String>,
    #[serde(default)]
    pub check_after: String,
}

fn seconds() -> u32 {
    5
}

impl Voice {
    pub fn from_json(text: &str) -> Result<Voice, String> {
        let v: Voice = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if v.record.is_empty() || v.transcribe.is_empty() {
            return Err("record and transcribe each need a command".into());
        }
        Ok(v)
    }

    /// The programs that must be installed: the first word of each command.
    pub fn programs(&self) -> Vec<&str> {
        [&self.record, &self.transcribe].iter().filter_map(|c| c.first().map(String::as_str)).collect()
    }

    /// The status, given whether each program was found on PATH and, when
    /// there is a `check`, what it printed.
    pub fn status(&self, found: &[(&str, bool)], check_output: Option<&str>) -> Status {
        let missing: Vec<&str> = found.iter().filter(|(_, ok)| !ok).map(|(p, _)| *p).collect();
        if !missing.is_empty() {
            return Status::Missing(format!("not on PATH: {} (voice.json)", missing.join(", ")));
        }
        match check_output {
            Some(out) if !self.lists_a_device(out) => Status::Missing("no microphone found".into()),
            _ => Status::Ready,
        }
    }

    /// Does `check`'s output list a device after `check_after`?
    pub fn lists_a_device(&self, output: &str) -> bool {
        output.lines().skip_while(|l| !l.contains(&self.check_after)).skip(1).any(|l| l.contains("[0]"))
    }

    fn fill(cmd: &[String], file: &str, dir: &str, seconds: u32) -> Vec<String> {
        cmd.iter().map(|a| a.replace("{file}", file).replace("{dir}", dir).replace("{seconds}", &seconds.to_string())).collect()
    }

    pub fn record_argv(&self, file: &str, dir: &str) -> Vec<String> {
        Self::fill(&self.record, file, dir, self.seconds)
    }

    pub fn transcribe_argv(&self, file: &str, dir: &str) -> Vec<String> {
        Self::fill(&self.transcribe, file, dir, self.seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_capability_turns_its_button_off_with_the_reason() {
        let mut caps = Capabilities::default();
        let b = buttons(&default_actions(), &caps, true);
        assert_eq!(b.len(), 3);
        let voice = b.iter().find(|b| b.action == Action::Voice).unwrap();
        assert!(!voice.enabled);
        assert!(voice.why.as_deref().unwrap().contains("not checked"));
        caps.set(Capability::Voice, Status::Ready);
        assert!(buttons(&default_actions(), &caps, true).iter().all(|b| b.enabled));
    }

    #[test]
    fn the_composer_has_no_format_button() {
        let b = buttons(&default_actions(), &Capabilities::default(), true);
        assert_eq!(b.iter().map(|b| b.action).collect::<Vec<_>>(), [Action::Send, Action::Voice, Action::Clear]);
        let b = buttons(&default_actions(), &Capabilities::default(), false);
        assert!(b.iter().any(|b| b.action == Action::Send && b.enabled));
    }

    #[test]
    fn voice_reads_its_commands_and_names_what_is_missing() {
        let v = Voice::from_json(r#"{"record":["ffmpeg","-t","{seconds}","{file}"],"transcribe":["whisper","{file}","--output_dir","{dir}"]}"#).unwrap();
        assert_eq!(v.programs(), ["ffmpeg", "whisper"]);
        assert_eq!(v.record_argv("/t/a.wav", "/t"), ["ffmpeg", "-t", "5", "/t/a.wav"]);
        assert_eq!(v.status(&[("ffmpeg", true), ("whisper", true)], None), Status::Ready);
        assert_eq!(v.status(&[("ffmpeg", true), ("whisper", false)], None), Status::Missing("not on PATH: whisper (voice.json)".into()));
        assert!(Voice::from_json(r#"{"record":[],"transcribe":["x"]}"#).is_err());
    }

    #[test]
    fn the_check_finds_a_microphone_only_among_audio_devices() {
        let v = Voice { check_after: "audio devices:".into(), ..Voice::from_json(r#"{"record":["r"],"transcribe":["t"]}"#).unwrap() };
        let with = "x video devices:\nx [0] Camera\nx audio devices:\nx [0] MacBook Pro Microphone\nError opening input";
        let without = "x video devices:\nx [0] Camera\nx audio devices:\nError opening input";
        assert!(v.lists_a_device(with));
        assert!(!v.lists_a_device(without));
        assert_eq!(v.status(&[], Some(without)), Status::Missing("no microphone found".into()));
    }

    #[test]
    fn a_page_names_its_actions_in_kebab_case() {
        let a: Vec<Action> = serde_json::from_str(r#"["send","voice"]"#).unwrap();
        assert_eq!(a, [Action::Send, Action::Voice]);
    }
}
