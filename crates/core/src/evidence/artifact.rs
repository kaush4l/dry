//! `kind: evidence`: the project's evidence, as an artifact its test phase
//! runs and reads (docs/evidence.md).
//!
//! ```text
//! artifacts/evidence.md
//! kind: evidence
//! root: .
//! command: dry evidence
//! ```
//!
//! - `evidence(op="run")` runs the project's evidence (`command` in
//!   `root`, `--expect` and `--update` passed on), then reads its
//!   manifest; `evidence(op="read")` only reads it. The result is one
//!   line (`12 shots · 1 changed (home@390x844 2.10%) · fail`).
//! - The snapshot is `evidence/summary.md` as it is now: which shots
//!   changed, by how much, the page-text lines that changed.
//!
//! Pure: the shell runs the command and reads the files (`live::Host`).

use crate::calls::Call;
use crate::live::{Content, Host, Job};

use super::report::{Manifest, Status};

/// The one tool it brings.
pub const TOOLS: [&str; 1] = ["evidence"];
/// The command it runs when its state names none.
pub const COMMAND: &str = "dry evidence";
/// The evidence folder, under the root.
pub const FOLDER: &str = "evidence";
/// The most summary text in the snapshot.
pub const SUMMARY_CHARS: usize = 8_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub root: String,
    pub command: String,
}

impl Default for State {
    fn default() -> State {
        State { root: ".".into(), command: COMMAND.into() }
    }
}

impl State {
    pub fn parse(text: &str) -> Result<State, String> {
        let mut s = State::default();
        for line in text.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()) {
            match line.split_once(':').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("root", v)) if !v.is_empty() => s.root = v.to_string(),
                Some(("command", v)) if !v.is_empty() => s.command = v.to_string(),
                _ => return Err(format!("an evidence artifact line is root: or command:, not `{line}`")),
            }
        }
        Ok(s)
    }

    pub fn to_text(&self) -> String {
        format!("kind: evidence\nroot: {}\ncommand: {}\n", self.root, self.command)
    }
}

/// The manifest as it is on disk now.
pub fn manifest(root: &str, host: &dyn Host) -> Result<Manifest, String> {
    match host.read(root, &format!("{FOLDER}/manifest.json")) {
        Content::Text(t) => Manifest::from_json(&t),
        Content::Missing(_) => Err("no evidence yet: evidence(op=\"run\") takes it".into()),
        _ => Err("evidence/manifest.json is not text".into()),
    }
}

/// The command a run sends: the artifact's, with what the call adds.
pub fn command(s: &State, call: &Call) -> Result<String, String> {
    let mut cmd = s.command.clone();
    if let Some(e) = call.arg("expect").map(str::trim).filter(|e| !e.is_empty()) {
        if !e.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ',' | '@' | ' ')) {
            return Err("expect= is scenario names (or name@WxH), comma separated".into());
        }
        cmd.push_str(&format!(" --expect {}", e.replace(' ', "")));
    }
    if matches!(call.arg("update"), Some("yes" | "true" | "1")) {
        cmd.push_str(" --update");
    }
    Ok(cmd)
}

/// Run its tool. One line back; the summary goes in the snapshot.
pub fn call(engine: &str, artifact: &str, text: &str, call: &Call, host: &dyn Host) -> Result<String, String> {
    let s = State::parse(text).map_err(|why| format!("the evidence artifact does not read: {why}"))?;
    let op = call.arg("op").unwrap_or("read");
    match op {
        "read" => manifest(&s.root, host).map(|m| format!("evidence: {}", m.line())),
        "run" => {
            let cmd = command(&s, call)?;
            let job = Job { engine, artifact, session: 1, root: &s.root, command: &cmd };
            let code = host.run(&job)?;
            match manifest(&s.root, host) {
                Ok(m) => Ok(format!(
                    "evidence: {} (exit {}); the summary is in the snapshot",
                    m.line(),
                    code.map_or("killed".into(), |c| c.to_string())
                )),
                Err(_) => Err(format!(
                    "`{cmd}` exited {} with no manifest: {}",
                    code.map_or("by a signal".into(), |c| c.to_string()),
                    crate::terminal::last_lines(&host.tail(&job, 8), 8).replace('\n', " ⏎ ")
                )),
            }
        }
        other => Err(format!("evidence(op=...) is run or read, not `{other}`")),
    }
}

/// The summary as it is now.
pub fn snapshot(text: &str, host: &dyn Host) -> String {
    let s = match State::parse(text) {
        Ok(s) => s,
        Err(why) => return format!("(does not read: {why})"),
    };
    let summary = match host.read(&s.root, &format!("{FOLDER}/summary.md")) {
        Content::Text(t) => t,
        _ => return format!("no evidence yet in {}/{FOLDER}: evidence(op=\"run\") takes it (`{}`)", s.root, s.command),
    };
    let mut body = summary.trim().to_string();
    if body.chars().count() > SUMMARY_CHARS {
        body = format!("{}\n… (cut at {SUMMARY_CHARS} characters)", body.chars().take(SUMMARY_CHARS).collect::<String>());
    }
    format!("root: {} · command: `{}`\n{body}", s.root, s.command)
}

/// One shot as an artifact page shows it: before, and for a changed one
/// the after and the diff. Paths are under the root.
#[derive(Clone, Debug, PartialEq)]
pub struct ShotView {
    pub name: String,
    pub status: Status,
    pub changed: f64,
    pub regions: usize,
    pub text: Vec<String>,
    /// The baseline (or, for a new or accepted shot, the capture).
    pub png: String,
    /// The capture, when it is not the baseline yet (a failed change).
    pub after: Option<String>,
    pub diff: Option<String>,
}

/// The shots of the manifest, changed ones first.
pub fn shots(m: &Manifest) -> Vec<ShotView> {
    let mut out: Vec<ShotView> = m
        .shots
        .iter()
        .map(|s| ShotView {
            name: s.name.clone(),
            status: s.status,
            changed: s.changed,
            regions: s.regions.len(),
            text: s.text.clone(),
            png: format!("{FOLDER}/{}.png", s.name),
            after: matches!(s.status, Status::Changed | Status::Flaky).then(|| format!("{FOLDER}/{}.new.png", s.name)),
            diff: (s.status.differs() && !s.regions.is_empty()).then(|| format!("{FOLDER}/{}.diff.png", s.name)),
        })
        .collect();
    out.sort_by_key(|v| !v.status.differs());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::report::Shot;
    use crate::live::Proc;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        ran: RefCell<Vec<String>>,
        manifest: RefCell<Option<String>>,
    }

    impl Host for Fake {
        fn now(&self) -> i64 {
            0
        }
        fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }
        fn read(&self, _: &str, path: &str) -> Content {
            match (path, self.manifest.borrow().clone()) {
                ("evidence/manifest.json", Some(m)) => Content::Text(m),
                ("evidence/summary.md", Some(m)) => Content::Text(Manifest::from_json(&m).unwrap().summary()),
                _ => Content::Missing("no file".into()),
            }
        }
        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, job: &Job) -> Result<Option<i32>, String> {
            self.ran.borrow_mut().push(format!("{} in {}", job.command, job.root));
            *self.manifest.borrow_mut() = Some(manifest_text());
            Ok(Some(1))
        }
        fn spawn(&self, _: &Job) -> Result<(), String> {
            Ok(())
        }
        fn status(&self, _: &Job) -> Proc {
            Proc::Ended(None)
        }
        fn tail(&self, _: &Job, _: usize) -> String {
            String::new()
        }
    }

    fn manifest_text() -> String {
        let shot = |name: &str, status, changed| Shot {
            name: name.into(),
            scenario: name.split('@').next().unwrap().into(),
            viewport: "1280x800".into(),
            sha256: "a".into(),
            baseline: Some("b".into()),
            text_sha256: String::new(),
            changed,
            regions: if changed > 0.0 { vec![[0, 0, 8, 8]] } else { vec![] },
            text: vec![],
            text_changed: 0,
            status,
            ms: 100,
        };
        Manifest {
            build: "x".into(),
            commit: "c".into(),
            time: 0,
            viewports: vec!["1280x800".into()],
            pixel: 8,
            percent: 0.0,
            shots: vec![shot("landing@1280x800", Status::Same, 0.0), shot("home@1280x800", Status::Changed, 1.5)],
            removed: vec![],
            status: "fail".into(),
            ms: 200,
        }
        .to_json()
    }

    fn call_of(text: &str) -> Call {
        crate::calls::parse(text).unwrap().remove(0)
    }

    #[test]
    fn run_runs_the_command_and_answers_one_line() {
        let host = Fake::default();
        let text = "kind: evidence\nroot: app\ncommand: cargo run -q -p dry-tools -- evidence\n".to_string();
        assert!(call("e1", "ev", &text, &call_of("evidence(op=\"read\")"), &host).unwrap_err().contains("no evidence yet"));
        assert!(snapshot(&text, &host).starts_with("no evidence yet in app/evidence"));
        let r = call("e1", "ev", &text, &call_of("evidence(op=\"run\", expect=\"home, settings\")"), &host).unwrap();
        assert_eq!(r, "evidence: 2 shots · 1 changed (home@1280x800 1.50%) · fail (exit 1); the summary is in the snapshot");
        assert_eq!(host.ran.borrow()[0], "cargo run -q -p dry-tools -- evidence --expect home,settings in app");
        let snap = snapshot(&text, &host);
        assert!(snap.contains("| home@1280x800 | changed | 1.50% |"), "{snap}");
        assert!(call("e1", "ev", &text, &call_of("evidence(op=\"run\", expect=\"a;rm -rf\")"), &host).is_err(), "no shell in expect=");
        assert!(call("e1", "ev", &text, &call_of("evidence(op=\"look\")"), &host).unwrap_err().contains("run or read"));
        assert_eq!(State::parse(&text).unwrap().to_text(), text, "reads back");
        assert_eq!(State::parse("kind: evidence\n").unwrap(), State::default());
        assert!(State::parse("kind: evidence\nurl: x").is_err());
    }

    #[test]
    fn changed_shots_come_first_with_before_after_and_diff() {
        let m = Manifest::from_json(&manifest_text()).unwrap();
        let v = shots(&m);
        assert_eq!(v[0].name, "home@1280x800");
        assert_eq!(
            (v[0].png.as_str(), v[0].after.as_deref(), v[0].diff.as_deref()),
            ("evidence/home@1280x800.png", Some("evidence/home@1280x800.new.png"), Some("evidence/home@1280x800.diff.png"))
        );
        assert_eq!((v[1].after.as_deref(), v[1].diff.as_deref()), (None, None));
    }
}
