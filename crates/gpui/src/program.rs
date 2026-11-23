//! Running a session's agent program: the only code that starts one. Core's
//! `session::Tool` builds the command line and reads what it printed; this
//! runs it in between. Blocking, so it runs on GPUI's background executor.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// What one run of the program left.
pub struct Run {
    pub argv: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    /// The exit code; none when a signal stopped it.
    pub code: Option<i32>,
    pub ms: u128,
    /// The usage file the program wrote, if it was given one.
    pub meta: Option<String>,
}

impl Run {
    pub fn code_text(&self) -> String {
        self.code.map_or("signal".into(), |c| c.to_string())
    }

    /// The last line of what it wrote to stderr.
    pub fn stderr_tail(&self) -> String {
        self.stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
    }

    /// Facts about the run itself, kept with the program's own metadata.
    pub fn facts(&self) -> Vec<(String, String)> {
        let mut facts = vec![
            ("run.program".into(), self.argv.first().cloned().unwrap_or_default()),
            ("run.exit".into(), self.code_text()),
            ("run.ms".into(), self.ms.to_string()),
            ("run.cwd".into(), dry_tools::root().display().to_string()),
        ];
        let tail = self.stderr_tail();
        if !tail.is_empty() {
            facts.push(("run.stderr".into(), tail));
        }
        facts
    }
}

/// A usage-report file for engine `engine`'s turn, in the temp folder.
pub fn meta_file(engine: usize) -> PathBuf {
    std::env::temp_dir().join(format!("dry-session-{}-{engine}.json", std::process::id()))
}

/// Run `argv` in the repo root with no input and wait for it. The prompt
/// is one of the arguments: no shell reads it. `env`: more variables
/// (`sessions.json`'s `env`, filled by `dry_tools::worker::prepare`).
pub fn run(argv: &[String], env: &[(String, String)], meta_file: Option<&Path>) -> Result<Run, String> {
    let (bin, args) = argv.split_first().ok_or("sessions.json: empty command")?;
    if let Some(f) = meta_file {
        let _ = std::fs::remove_file(f);
    }
    let started = Instant::now();
    let out = Command::new(bin)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(dry_tools::root())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not start `{bin}`: {e}"))?;
    let meta = meta_file.and_then(|f| std::fs::read_to_string(f).ok());
    if let Some(f) = meta_file {
        let _ = std::fs::remove_file(f);
    }
    Ok(Run {
        argv: argv.to_vec(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code(),
        ms: started.elapsed().as_millis(),
        meta,
    })
}

#[cfg(test)]
mod tests {
    /// One real turn of the program named in `DRY_TEST_SESSION` (default
    /// `claude`), through the same path a session engine takes: the
    /// command from `sessions.json`, `run`, then `read`. Starts the real
    /// program (and may cost), so it is ignored:
    /// `DRY_TEST_SESSION=codex cargo test -p dry-gpui -- --ignored --nocapture one_real_turn`
    #[test]
    #[ignore]
    fn one_real_turn() {
        let sessions = dry_tools::read_sessions().expect("sessions.json");
        let name = std::env::var("DRY_TEST_SESSION").unwrap_or_else(|_| "claude".into());
        let tool = sessions.get(&name).expect("program in sessions.json");
        let file = tool.wants_meta_file().then(|| super::meta_file(99));
        let prompt = std::env::var("DRY_TEST_PROMPT").unwrap_or_else(|_| "Reply with just: ok".into());
        let p = dry_tools::worker::prepare(
            tool,
            "test-session",
            &prompt,
            None,
            &file.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        )
        .expect("its settings file");
        let run = super::run(&p.argv, &p.env, file.as_deref()).expect("program starts");
        let mut turn = tool.read(&run.stdout, run.meta.as_deref());
        turn.meta.extend(run.facts());
        println!("reply {:?}\nid {:?}\nerror {:?}\n{} metadata values:", turn.reply, turn.id, turn.error, turn.meta.len());
        for (k, v) in &turn.meta {
            println!("  {k} = {v}");
        }
        assert!(turn.reply.is_some() || turn.error.is_some(), "neither a reply nor an error: {}", run.stderr_tail());
    }
}
