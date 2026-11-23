//! CLI models, run: the program a `cli:` model names (`dry_core::cli`),
//! spawned once per request with an argv array (no shell, nothing
//! interpolated), the prompt on its stdin, its stdout read line by line
//! (each text delta to the caller as it comes), its stderr kept for the
//! error line, a timeout, and a cancel flag.
//!
//! `net::post`, `net::stream` and `net::get` send a `cli:` URL here, so
//! every native caller (the host's runner through `net::Native`, the
//! window's Test, `dry models`) runs CLI models with no change of its own.
//!
//! The program runs in an empty folder of its own (`workdir`), so it
//! finds no project instructions (`CLAUDE.md`, `AGENTS.md`) to act on.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dry_core::cli::{self, Cli, Input, Reply};
use dry_core::inference::Request;
use dry_core::stream::Delta;

use crate::Result;

/// As long as a model server is given (`net`): a CLI answers in seconds,
/// but a long prompt on a big model may take minutes.
pub const TIMEOUT: Duration = Duration::from_secs(600);

/// Most of its stderr kept for the error line.
const STDERR_CAP: usize = 64 * 1024;

/// How a run may end early.
#[derive(Clone, Debug)]
pub struct Options {
    pub timeout: Duration,
    /// Set it to stop the run: the program is killed.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for Options {
    fn default() -> Options {
        Options { timeout: TIMEOUT, cancel: None }
    }
}

/// The program's path: a path as it is (if it exists), a name on `PATH`.
pub fn which(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 {
        return p.is_file().then(|| p.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(program)).find(|p| p.is_file())
}

/// The empty folder the programs run in.
pub fn workdir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("dry-cli");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// One provider as found on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    pub program: String,
    pub path: Option<PathBuf>,
    /// Its `--version` line, when asked; why not, if it did not answer.
    pub version: Option<std::result::Result<String, String>>,
}

impl Found {
    /// One line for people.
    pub fn line(&self) -> String {
        match (&self.path, &self.version) {
            (None, _) => {
                format!("{}: unavailable ({} is not on PATH; install it or set \"program\" in models.json)", self.name, self.program)
            }
            (Some(p), Some(Ok(v))) => format!("{}: {v} at {}", self.name, p.display()),
            (Some(p), Some(Err(e))) => format!("{}: at {}, but --version failed: {e}", self.name, p.display()),
            (Some(p), None) => format!("{}: at {}", self.name, p.display()),
        }
    }
}

/// `cli` on this computer: where, and (with `ask`) its version.
pub fn find(cli: &Cli, ask: bool) -> Found {
    let path = which(&cli.program);
    let version = match (&path, ask) {
        (Some(p), true) => Some(version(p)),
        _ => None,
    };
    Found { name: cli.name.clone(), program: cli.program.clone(), path, version }
}

/// Every provider there is an override for.
pub fn discover(ask: bool) -> Vec<Found> {
    cli::PROVIDERS.iter().filter_map(|n| Cli::named(n)).map(|c| find(&c, ask)).collect()
}

/// `<program> --version`, its first line (10 s at most).
pub fn version(path: &Path) -> std::result::Result<String, String> {
    let inv = cli::Invocation { program: path.display().to_string(), args: vec!["--version".into()], stdin: None };
    let opts = Options { timeout: Duration::from_secs(10), cancel: None };
    let (ok, out, err) = spawn(&inv, &opts, &mut |_| {})?;
    if ok { Ok(cli::version_line(&out)) } else { Err(last_line(&err)) }
}

fn last_line(text: &str) -> String {
    text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("(nothing on stderr)").to_string()
}

/// Run `inv`: each stdout line to `line` as it comes. Whether it exited
/// with 0, all of stdout, and its stderr. A timeout or a cancel kills it
/// and is an error.
pub(crate) fn spawn(
    inv: &cli::Invocation,
    opts: &Options,
    line: &mut dyn FnMut(&str),
) -> std::result::Result<(bool, String, String), String> {
    let path =
        which(&inv.program).ok_or_else(|| format!("{} is not on PATH (install it, or set \"program\" in models.json)", inv.program))?;
    let mut cmd = Command::new(&path);
    cmd.args(&inv.args)
        .current_dir(workdir()?)
        .stdin(if inv.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group, so a stop takes the programs it started too
    // (they hold its stdout open).
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", path.display()))?;
    // Written on its own thread: a program may print before it has read all of a long prompt.
    let writer = child.stdin.take().zip(inv.stdin.clone()).map(|(mut pipe, text)| {
        std::thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
            // Dropped: closing stdin says the prompt is complete.
        })
    });
    let mut err_pipe = child.stderr.take().expect("piped");
    let stderr = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err_pipe.by_ref().take(STDERR_CAP as u64).read_to_string(&mut s);
        // The rest is drained, so a chatty program never blocks on a full pipe.
        let _ = std::io::copy(&mut err_pipe, &mut std::io::sink());
        s
    });
    let stdout = child.stdout.take().expect("piped");
    let child = Arc::new(Mutex::new(child));
    let done = Arc::new(AtomicBool::new(false));
    let killed: Arc<Mutex<Option<String>>> = Arc::default();
    let watchdog = {
        let (child, done, killed, cancel, timeout) = (child.clone(), done.clone(), killed.clone(), opts.cancel.clone(), opts.timeout);
        std::thread::spawn(move || {
            let start = Instant::now();
            while !done.load(Ordering::Acquire) {
                let why = if cancel.as_ref().is_some_and(|c| c.load(Ordering::Acquire)) {
                    Some("cancelled".to_string())
                } else if start.elapsed() >= timeout {
                    Some(format!("no answer in {timeout:?}; stopped"))
                } else {
                    None
                };
                if let Some(why) = why {
                    *killed.lock().unwrap() = Some(why);
                    kill(&mut child.lock().unwrap());
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        })
    };
    let mut out = String::new();
    let mut reader = BufReader::new(stdout);
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                out.push_str(&buf);
                line(buf.trim_end_matches(['\n', '\r']));
            }
        }
    }
    let status = loop {
        if let Some(s) = child.lock().unwrap().try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    done.store(true, Ordering::Release);
    let _ = watchdog.join();
    if let Some(w) = writer {
        let _ = w.join();
    }
    let err = stderr.join().unwrap_or_default();
    if let Some(why) = killed.lock().unwrap().take() {
        return Err(why);
    }
    Ok((status.success(), out, err))
}

/// Stop a run: its whole process group on unix, else the program.
fn kill(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        let _ = Command::new("kill").args(["-KILL", "--", &group]).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
}

/// Run `cli` on `input`: each streamed delta to `on`; the reply, or why
/// not (`<provider>: <its last stderr line or error message>`).
pub fn run(cli: &Cli, input: &Input, opts: &Options, on: &mut dyn FnMut(Delta)) -> Result<Reply> {
    let inv = cli.invocation(input);
    let mut parser = cli.parser();
    let (ok, _, err) = spawn(&inv, opts, &mut |l| {
        for d in parser.line(l) {
            on(d);
        }
    })
    .map_err(|e| format!("{}: {e}", cli.name))?;
    parser.finish(ok, &err).map_err(|e| format!("{}: {e}", cli.name))
}

/// One prompt, the whole reply: the common caller.
pub fn run_cli(cli: &Cli, input: &Input) -> Result<Reply> {
    run(cli, input, &Options::default(), &mut |_| {})
}

/// `net::post` for a `cli:` URL: the request body flattened, the program
/// run, a Chat Completions body back.
pub fn post(req: &Request) -> Result<String> {
    stream(req, &mut |_| {})
}

/// `net::stream` for a `cli:` URL: the same, each delta to `on`.
pub fn stream(req: &Request, on: &mut dyn FnMut(Delta)) -> Result<String> {
    let cli = cli::from_base(&req.url)?;
    let input = cli::flatten(&req.body)?;
    run(&cli, &input, &Options::default(), on).map(|r| cli::completion_body(&r))
}

/// `net::get` for a `cli:` URL: `…/models` is the CLI's model names.
pub fn get(url: &str) -> Result<String> {
    // `{base}/models`, the base perhaps with its `?program=`.
    let Some(base) = url.strip_suffix("/models") else { return Err(format!("{url}: a CLI model has nothing to GET but /models")) };
    cli::models_body(&cli::from_base(base)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake CLI: a shell script that prints what a real one did.
    fn fake(name: &str, script: &str) -> String {
        let dir = std::env::temp_dir().join(format!("dry-fake-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn input(prompt: &str) -> Input {
        Input { model: "haiku".into(), system: "Be brief.".into(), prompt: prompt.into() }
    }

    #[test]
    fn a_fake_claude_streams_and_gets_its_args_and_stdin() {
        // Echoes its stdin back as two text deltas, then the result; its args go to a file.
        let args_file = std::env::temp_dir().join(format!("dry-fake-args-{}", std::process::id()));
        let program = fake(
            "claude",
            &format!(
                r#"printf '%s\n' "$@" > {args}
read -r p
echo '{{"type":"system","subtype":"init","model":"fake-haiku"}}'
echo '{{"type":"stream_event","event":{{"type":"content_block_delta","delta":{{"type":"text_delta","text":"you said "}}}}}}'
echo "{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"$p\"}}}}}}"
echo "{{\"type\":\"result\",\"is_error\":false,\"result\":\"you said $p\",\"total_cost_usd\":0.5,\"usage\":{{\"input_tokens\":3,\"output_tokens\":2}}}}""#,
                args = args_file.display()
            ),
        );
        let cli = Cli { program, ..Cli::claude() };
        let mut deltas = Vec::new();
        let reply = run(&cli, &input("ping"), &Options::default(), &mut |d| deltas.push(d)).unwrap();
        assert_eq!(deltas, vec![Delta::Text("you said ".into()), Delta::Text("ping".into())]);
        assert_eq!((reply.text.as_str(), reply.model.as_deref(), reply.cost_usd), ("you said ping", Some("fake-haiku"), Some(0.5)));
        let args = std::fs::read_to_string(&args_file).unwrap();
        let args: Vec<&str> = args.lines().collect();
        assert!(
            args.windows(2).any(|w| w == ["--model", "haiku"]) && args.windows(2).any(|w| w == ["--system-prompt", "Be brief."]),
            "{args:?}"
        );
        assert!(args.windows(2).any(|w| w == ["--tools", ""]), "an empty arg passes as one, no shell: {args:?}");
        let _ = std::fs::remove_file(args_file);
    }

    #[test]
    fn a_failed_run_says_its_stderr_and_a_slow_one_is_stopped() {
        let cli = Cli { program: fake("broken", "echo 'warming up' >&2; echo 'Error: not logged in' >&2; exit 3"), ..Cli::copilot() };
        assert_eq!(run_cli(&cli, &input("x")).unwrap_err(), "copilot: Error: not logged in");

        let cli = Cli { program: fake("slow", "sleep 30"), ..Cli::codex() };
        let start = Instant::now();
        let err = run(&cli, &input("x"), &Options { timeout: Duration::from_millis(300), cancel: None }, &mut |_| {}).unwrap_err();
        assert!(err.contains("no answer in 300ms") && start.elapsed() < Duration::from_secs(5), "{err}");

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::Release);
        });
        let err = run(&cli, &input("x"), &Options { timeout: TIMEOUT, cancel: Some(cancel) }, &mut |_| {}).unwrap_err();
        assert_eq!(err, "codex: cancelled");

        let missing = Cli { program: "dry-no-such-cli".into(), ..Cli::claude() };
        assert!(run_cli(&missing, &input("x")).unwrap_err().contains("not on PATH"));
        assert!(find(&missing, true).line().contains("unavailable"));
    }

    #[test]
    fn a_cli_url_runs_through_net_and_reads_as_a_completion() {
        use dry_core::inference::{Inference, Message, OpenAi, Role};
        let program = fake(
            "codex",
            r#"cat > /dev/null; echo '{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}'; echo '{"type":"turn.completed","usage":{"input_tokens":5,"output_tokens":1}}'"#,
        );
        let ai = OpenAi { base_url: cli::base_url("codex", Some(&program)), model: "m".into(), ..OpenAi::default() };
        let req = ai.request(&[Message { role: Role::User, text: "hi".into() }]);
        let body = crate::net::post(&req).unwrap();
        assert_eq!(ai.reply(&body), Ok("hello".into()));
        let mut seen = String::new();
        let streamed = crate::net::stream(&dry_core::stream::streaming(&req), &mut |d| {
            if let Delta::Text(t) = d {
                seen.push_str(&t)
            }
        });
        assert_eq!((ai.reply(&streamed.unwrap()), seen.as_str()), (Ok("hello".into()), "hello"));
        assert!(crate::net::get("cli:claude/models", &None).unwrap().contains("haiku"));
        assert!(
            crate::net::get("cli:claude?program=/opt/claude/models", &None).unwrap().contains("haiku"),
            "the program is not /opt/claude/models"
        );
        assert!(version(Path::new(&fake("v", "echo 'tool 1.2.3'"))).unwrap() == "tool 1.2.3");
    }

    /// The real CLIs, one tiny prompt each (uses the subscription):
    /// `cargo test -p dry-tools -- --ignored --nocapture real_clis`
    #[test]
    #[ignore]
    fn real_clis_answer_a_tiny_prompt() {
        let models = [("claude", "haiku"), ("codex", "gpt-5.6-luna"), ("copilot", "auto")];
        for (name, model) in models {
            let cli = Cli::named(name).unwrap();
            let found = find(&cli, true);
            println!("{}", found.line());
            if found.path.is_none() {
                continue;
            }
            let start = Instant::now();
            let reply = run_cli(&cli, &Input { model: model.into(), system: "Answer in one word.".into(), prompt: "Say pong.".into() });
            println!("  {name}/{model}: {:?} in {} ms", reply.as_ref().map(|r| r.text.clone()), start.elapsed().as_millis());
            assert!(reply.unwrap().text.to_lowercase().contains("pong"));
        }
    }
}
