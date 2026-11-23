//! `kind: terminal`: the developer artifact. The engine's terminal
//! sessions: what is running, and what last ran.
//!
//! ```text
//! artifacts/term.md
//! kind: terminal
//! root: .
//! session: 1 run exit:0 1795356330 cargo test -q
//! session: 2 start running 1795356400 cargo build --release
//! session: 3 background running 1795356410 npm run dev
//! ```
//!
//! One line per session: its number, how it was started, its status
//! (`running`, `exit:<code>`, or `stopped`: ended with no code), when it
//! started (unix seconds) and its command (a line break in it written as
//! `\n`). What it printed is in `artifacts/<name>/<session>.log`, kept by
//! the shell, and only ever read into the snapshot.
//!
//! - `run(command)` waits for it; the result is its exit code and the last
//!   lines it printed.
//! - `start(command)` returns at once. When it ends, `settle` marks it and
//!   gives the callback message (`session 2 \`cargo build\` finished,
//!   exit 0`): the shell adds it as a message and runs a turn, the way an
//!   alert does. A callback is a new turn, not a budget.
//! - `start(command, wait="yes")`: the same job, and the turn ends after
//!   this step with no reply. The engine waits on it (`Engine::waiting`
//!   holds `<artifact>#<session>`, saved in `run.md`), so its parent hears
//!   nothing, and its end is the wake (`settle_waited`, through
//!   `State::add_wake`): the run goes on in the phase it was in. With
//!   several waits in one step the wake comes when the last has ended.
//!   A new ask cancels the wait (`engines::stop_waiting`); the job's end
//!   then comes as a plain callback.
//! - `background(command)` runs without a callback; its state is only in
//!   the snapshot.
//!
//! Commands run in the root folder. There is no approval step yet: an
//! engine that runs unattended will need one before it may run anything.
//!
//! Pure. The shell runs the processes and keeps the logs (`live::Host`).

use crate::calls::Call;
use crate::live::{Host, Job, Proc};

/// The tools a terminal artifact brings.
pub const TOOLS: [&str; 3] = ["run", "start", "background"];
/// The `start` input that ends the turn until the job ends.
pub const WAIT: &str = "wait";
/// Lines of output in a `run` result.
pub const RESULT_LINES: usize = 15;
/// Lines of output shown per session in the snapshot.
pub const TAIL_LINES: usize = 30;
/// Finished sessions named (beyond the last one, shown with its output).
pub const NAMED: usize = 8;
/// Most sessions kept in the state; past it the oldest finished go.
pub const MAX_SESSIONS: usize = 30;
/// Most characters of one output tail in the snapshot.
pub const TAIL_CHARS: usize = 4_000;

/// How a session was started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Run,
    /// Asked for a callback when it ends.
    Start,
    Background,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Run => "run",
            Mode::Start => "start",
            Mode::Background => "background",
        }
    }

    fn parse(word: &str) -> Option<Mode> {
        [Mode::Run, Mode::Start, Mode::Background].into_iter().find(|m| m.as_str() == word)
    }
}

/// Where a session is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Exit(i32),
    /// Ended without an exit code (a signal, or lost when the app stopped).
    Stopped,
}

impl Status {
    fn word(self) -> String {
        match self {
            Status::Running => "running".into(),
            Status::Exit(c) => format!("exit:{c}"),
            Status::Stopped => "stopped".into(),
        }
    }

    fn parse(word: &str) -> Option<Status> {
        match word {
            "running" => Some(Status::Running),
            "stopped" => Some(Status::Stopped),
            w => w.strip_prefix("exit:")?.parse().ok().map(Status::Exit),
        }
    }

    fn ended(code: Option<i32>) -> Status {
        code.map_or(Status::Stopped, Status::Exit)
    }

    /// `exit 0`, `no exit code (stopped)`.
    pub fn text(self) -> String {
        match self {
            Status::Running => "running".into(),
            Status::Exit(c) => format!("exit {c}"),
            Status::Stopped => "no exit code (stopped)".into(),
        }
    }
}

/// One session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: u32,
    pub mode: Mode,
    pub status: Status,
    /// Unix seconds.
    pub started: i64,
    pub command: String,
}

/// A terminal artifact's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Where its commands run: relative to the repo (`.`), or absolute
    /// (set by hand: the tools never change it).
    pub root: String,
    pub sessions: Vec<Session>,
}

impl Default for State {
    fn default() -> State {
        State { root: ".".into(), sessions: Vec::new() }
    }
}

impl State {
    pub fn parse(text: &str) -> Result<State, String> {
        let mut s = State::default();
        for line in text.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()) {
            if let Some(root) = line.strip_prefix("root:") {
                s.root = root.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("session:") {
                let bad = || format!("`{line}`: a session line is `session: <n> <run|start|background> <running|exit:N|stopped> <started> <command>`");
                let mut words = rest.trim().splitn(5, ' ');
                let mut next = || words.next().ok_or_else(bad);
                let id = next()?.parse().map_err(|_| bad())?;
                let mode = Mode::parse(next()?).ok_or_else(bad)?;
                let status = Status::parse(next()?).ok_or_else(bad)?;
                let started = next()?.parse().map_err(|_| bad())?;
                let command = unescape(next()?);
                s.sessions.push(Session { id, mode, status, started, command });
            } else {
                return Err(format!("a terminal artifact line is root: or session:, not `{line}`"));
            }
        }
        Ok(s)
    }

    pub fn to_text(&self) -> String {
        let mut out = format!("kind: terminal\nroot: {}\n", self.root);
        for x in &self.sessions {
            out.push_str(&format!("session: {} {} {} {} {}\n", x.id, x.mode.as_str(), x.status.word(), x.started, escape(&x.command)));
        }
        out
    }

    fn next_id(&self) -> u32 {
        self.sessions.iter().map(|x| x.id).max().unwrap_or(0) + 1
    }

    /// Keep at most `MAX_SESSIONS`: the oldest finished ones go first.
    fn trim(&mut self) {
        while self.sessions.len() > MAX_SESSIONS {
            match self.sessions.iter().position(|x| x.status != Status::Running) {
                Some(i) => {
                    self.sessions.remove(i);
                }
                None => break,
            }
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\r', "").replace('\n', "\\n")
}

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, false) => out.push(c),
            _ => match chars.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            },
        }
    }
    out
}

/// The command as one short line, for results and messages.
fn short(command: &str) -> String {
    let one = command.split('\n').next().unwrap_or("").trim();
    let cut: String = one.chars().take(80).collect();
    if cut.len() < one.len() || command.contains('\n') { format!("{cut}…") } else { cut }
}

/// The last `n` lines of `text`, each as it ended up on screen (after the
/// last carriage return, so a progress bar is one line).
pub fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().map(|l| l.split('\r').rfind(|seg| !seg.is_empty()).unwrap_or("")).collect();
    let tail = lines[lines.len().saturating_sub(n)..].join("\n");
    let chars = tail.chars().count();
    if chars > TAIL_CHARS { format!("…{}", tail.chars().skip(chars - TAIL_CHARS).collect::<String>()) } else { tail }
}

/// Whether an environment variable's name says it holds a secret: the
/// shell leaves those out of a command's environment, so no command can
/// print one into a log.
pub fn secret_env(name: &str) -> bool {
    let n = name.to_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "AUTH", "COOKIE", "SESSION"].iter().any(|w| n.contains(w))
}

fn job<'a>(engine: &'a str, artifact: &'a str, root: &'a str, x: &'a Session) -> Job<'a> {
    Job { engine, artifact, session: x.id, root, command: &x.command }
}

/// A `run` whose session is recorded and not yet run: everything the
/// shell needs to run it and wait, owned, so it can wait on another
/// thread while the act's other calls wait too (`crate::turn`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunJob {
    pub engine: String,
    pub artifact: String,
    pub session: u32,
    pub root: String,
    pub command: String,
}

impl RunJob {
    pub fn job(&self) -> Job<'_> {
        Job { engine: &self.engine, artifact: &self.artifact, session: self.session, root: &self.root, command: &self.command }
    }

    /// Run it and wait: its exit code and its output's last lines.
    pub fn wait(&self, host: &dyn Host) -> Result<(Option<i32>, String), String> {
        let code = host.run(&self.job())?;
        Ok((code, host.tail(&self.job(), RESULT_LINES)))
    }
}

/// The first half of `run(command)`: record the session as running and
/// give the job, which the shell runs (`RunJob::wait`) and `finish_run`
/// writes back.
pub fn prepare_run(engine: &str, artifact: &str, text: &mut String, call: &Call, host: &dyn Host) -> Result<RunJob, String> {
    let mut s = State::parse(text).map_err(|why| format!("the terminal artifact does not read: {why}"))?;
    let command = call.arg("command").map(str::trim).unwrap_or("").to_string();
    if command.is_empty() {
        return Err(format!("`{}` needs command=\"...\"", call.name));
    }
    no_wait(call, Mode::Run)?;
    guard(text, &command)?;
    let x = Session { id: s.next_id(), mode: Mode::Run, status: Status::Running, started: host.now(), command };
    let job = RunJob { engine: engine.into(), artifact: artifact.into(), session: x.id, root: s.root.clone(), command: x.command.clone() };
    s.sessions.push(x);
    s.trim();
    *text = s.to_text();
    Ok(job)
}

/// The second half: what the run came to (its exit code and last lines,
/// or why it could not run), marked on its session; the result line.
pub fn finish_run(text: &mut String, job: &RunJob, came: Result<(Option<i32>, String), String>) -> Result<String, String> {
    let mut s = State::parse(text).map_err(|why| format!("the terminal artifact does not read: {why}"))?;
    let status = match &came {
        Ok((code, _)) => Status::ended(*code),
        Err(_) => Status::Stopped,
    };
    if let Some(x) = s.sessions.iter_mut().find(|x| x.id == job.session) {
        x.status = status;
    }
    *text = s.to_text();
    let (_, tail) = came?;
    let tail = if tail.trim().is_empty() { "(no output)".to_string() } else { last_lines(&tail, RESULT_LINES) };
    Ok(format!("session {} `{}`: {}\n{tail}", job.session, short(&job.command), status.text()))
}

/// Run one of the terminal tools on artifact `artifact` of engine
/// `engine`, whose state is `text`.
pub fn call(engine: &str, artifact: &str, text: &mut String, call: &Call, host: &dyn Host) -> Result<String, String> {
    let mut s = State::parse(text).map_err(|why| format!("the terminal artifact does not read: {why}"))?;
    let mode = match call.name.as_str() {
        "run" => Mode::Run,
        "start" => Mode::Start,
        "background" => Mode::Background,
        other => return Err(format!("`{other}` is not a terminal tool")),
    };
    let command = call.arg("command").map(str::trim).unwrap_or("").to_string();
    if command.is_empty() {
        return Err(format!("`{}` needs command=\"...\"", call.name));
    }
    let wait = no_wait(call, mode)?;
    guard(text, &command)?;
    let x = Session { id: s.next_id(), mode, status: Status::Running, started: host.now(), command };
    let (id, shown) = (x.id, short(&x.command));
    if mode == Mode::Run {
        // Both halves at once: the shell waits on the job off this thread
        // (`crate::turn` plans a run as a waiting call).
        let job = prepare_run(engine, artifact, text, call, host)?;
        let came = job.wait(host);
        return finish_run(text, &job, came);
    }
    host.spawn(&job(engine, artifact, &s.root, &x))?;
    s.sessions.push(x);
    let out = match mode {
        Mode::Start if wait => waiting_text(id, &shown),
        Mode::Start => started_text(id, &shown),
        _ => format!("session {id} `{shown}` started in the background (no message when it ends)"),
    };
    s.trim();
    *text = s.to_text();
    Ok(out)
}

/// Mark the sessions the shell says have ended. Returns the callback
/// message of each `start` session among them, to add as a message that
/// runs a turn. A session is marked once, so its callback comes once.
pub fn settle(engine: &str, artifact: &str, text: &mut String, host: &dyn Host) -> Vec<String> {
    let Ok(mut s) = State::parse(text) else { return Vec::new() };
    let mut messages = Vec::new();
    let root = s.root.clone();
    for x in s.sessions.iter_mut().filter(|x| x.status == Status::Running) {
        if let Proc::Ended(code) = host.status(&job(engine, artifact, &root, x)) {
            x.status = Status::ended(code);
            if x.mode == Mode::Start {
                messages.push(finished_text(artifact, x));
            }
        }
    }
    let new = s.to_text();
    if new != *text {
        *text = new;
    }
    messages
}

/// Every callback message engine `e`'s terminals have now (`settle` on
/// each), its state marked.
pub fn settle_engine(e: &mut crate::state::Engine, host: &dyn Host) -> Vec<String> {
    let names: Vec<String> =
        crate::live::present(e).into_iter().filter(|(_, l)| *l == crate::live::Live::Terminal).map(|(n, _)| n).collect();
    let id = e.id.clone();
    let mut out = Vec::new();
    for name in names {
        if let Some(text) = e.artifacts.get_mut(&name) {
            let header = crate::capability::first_line(text);
            out.extend(settle(&id, &name, text, host));
            crate::capability::keep_header(text, &header);
        }
    }
    out
}

/// Whether `call` asks to wait for its job (`wait="yes"`).
fn wants_wait(call: &Call) -> Result<bool, String> {
    match call.arg(WAIT).map(|w| w.trim().to_ascii_lowercase()) {
        None => Ok(false),
        Some(w) if ["yes", "true", "1"].contains(&w.as_str()) => Ok(true),
        Some(w) if ["no", "false", "0", ""].contains(&w.as_str()) => Ok(false),
        Some(w) => Err(format!("wait=\"{w}\": wait is \"yes\" or \"no\"")),
    }
}

/// `wait` read for a call of `mode`: only `start` takes it.
fn no_wait(call: &Call, mode: Mode) -> Result<bool, String> {
    let wait = wants_wait(call)?;
    if wait && mode != Mode::Start {
        return Err(format!("`{}` takes no wait: only start(..., wait=\"yes\") waits for its job", call.name));
    }
    Ok(wait)
}

fn started_text(id: u32, shown: &str) -> String {
    format!("session {id} `{shown}` started; a message comes when it ends")
}

/// The result of `start(..., wait="yes")` until the wait is over.
fn waiting_text(id: u32, shown: &str) -> String {
    format!("waiting for session {id} `{shown}` to end: your turn ends here with no reply, and goes on in this phase when it ends")
}

/// The callback message of a `start` session that ended. One with no
/// exit code says its result is not known (killed, or lost when the app
/// stopped while it ran: a restart finds it so).
fn finished_text(artifact: &str, x: &Session) -> String {
    let unknown = if x.status == Status::Stopped { "; its result is not known (it was killed, or the app stopped while it ran)" } else { "" };
    format!("session {} `{}` finished, {}{unknown} (terminal {artifact})", x.id, short(&x.command), x.status.text())
}

/// The `Engine::waiting` entry for session `session` of terminal
/// `artifact`: `term#3` (a child engine's id has no `#`).
pub fn wait_entry(artifact: &str, session: u32) -> String {
    format!("{artifact}#{session}")
}

/// A waiting entry as (terminal artifact, session), when it is a job's.
pub fn parse_wait(entry: &str) -> Option<(&str, u32)> {
    let (artifact, n) = entry.rsplit_once('#')?;
    Some((artifact, n.parse().ok()?))
}

/// The session `call` (a `start` that just ran on `text`) asked to wait
/// for: the newest one.
pub fn waited(call: &Call, text: &str) -> Option<u32> {
    if call.name != TOOLS[1] || !wants_wait(call).unwrap_or(false) {
        return None;
    }
    State::parse(text).ok()?.sessions.iter().map(|x| x.id).max()
}

/// How a waiting entry reads for people: `session 3 (terminal term)`.
pub fn wait_label(entry: &str) -> Option<String> {
    parse_wait(entry).map(|(a, n)| format!("session {n} (terminal {a})"))
}

/// The waits in `entries` are over (the jobs ended, or a new ask came):
/// their placeholder results in `e`'s history read as a plain `start`.
pub fn release(e: &mut crate::state::Engine, entries: &[String]) {
    for (artifact, n) in entries.iter().filter_map(|w| parse_wait(w)) {
        let shown = e
            .artifacts
            .get(artifact)
            .and_then(|t| State::parse(t).ok())
            .and_then(|s| s.sessions.into_iter().find(|x| x.id == n))
            .map(|x| short(&x.command));
        let Some(shown) = shown else { continue };
        let (from, to) = (waiting_text(n, &shown), started_text(n, &shown));
        if let Some(m) = e.messages.iter_mut().rev().find(|m| m.role == crate::inference::Role::Tool && m.text.contains(&from)) {
            m.text = m.text.replacen(&from, &to, 1);
        }
    }
}

/// The wake that ends engine `e`'s wait on its own jobs
/// (`start(..., wait="yes")`): nothing while a child engine is still
/// waited for or a job still runs; once all have ended, every job's end
/// (and any other ended `start` session's) as messages, the waits taken
/// off. A job that is gone from its terminal is said to be gone.
pub fn settle_waited(e: &mut crate::state::Engine, host: &dyn Host) -> Vec<String> {
    let jobs: Vec<String> = e.waiting.iter().filter(|w| parse_wait(w).is_some()).cloned().collect();
    if jobs.is_empty() || jobs.len() != e.waiting.len() {
        return Vec::new();
    }
    let mut gone = Vec::new();
    let mut done = Vec::new();
    for entry in &jobs {
        let (artifact, n) = parse_wait(entry).expect("filtered");
        let found = e.artifacts.get(artifact).and_then(|t| State::parse(t).ok()).and_then(|s| {
            let x = s.sessions.iter().find(|x| x.id == n)?.clone();
            Some((s.root, x))
        });
        match found {
            None => gone.push(format!("session {n} is gone from terminal {artifact}: its result is not known")),
            Some((root, x)) if x.status == Status::Running => {
                if host.status(&job(&e.id, artifact, &root, &x)) == Proc::Running {
                    return Vec::new();
                }
            }
            // Marked already by another path: its end, said here.
            Some((_, x)) => done.push(finished_text(artifact, &x)),
        }
    }
    e.waiting.clear();
    release(e, &jobs);
    let mut out = settle_engine(e, host);
    out.extend(done);
    out.extend(gone);
    out
}

/// `1h 02m`, `3m 05s`, `12s`.
fn elapsed(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    }
}

/// What the model reads: each running session as a terminal (command, how
/// long, the last lines), the last finished one with its exit code and
/// last lines, and the finished ones before it by name only.
pub fn snapshot(engine: &str, artifact: &str, text: &str, host: &dyn Host) -> String {
    let s = match State::parse(text) {
        Ok(s) => s,
        Err(why) => return format!("This terminal artifact does not read: {why}"),
    };
    let now = host.now();
    let mut out = format!("root: {}", s.root);
    let tail = |x: &Session| {
        let t = host.tail(&job(engine, artifact, &s.root, x), TAIL_LINES);
        if t.trim().is_empty() { "(no output yet)".to_string() } else { last_lines(&t, TAIL_LINES) }
    };
    let running: Vec<&Session> = s.sessions.iter().filter(|x| x.status == Status::Running).collect();
    for x in &running {
        let how = match host.status(&job(engine, artifact, &s.root, x)) {
            Proc::Running => format!("running {}", elapsed(now - x.started)),
            Proc::Ended(code) => format!("ended, {} (not yet marked)", Status::ended(code).text()),
        };
        let callback = if x.mode == Mode::Start { ", a message when it ends" } else { "" };
        out.push_str(&format!("\n\n#### session {} — {}, {how}{callback}\n$ {}\n{}", x.id, x.mode.as_str(), x.command, tail(x)));
    }
    let done: Vec<&Session> = s.sessions.iter().filter(|x| x.status != Status::Running).collect();
    if let Some(last) = done.last() {
        out.push_str(&format!(
            "\n\n#### last: session {} — {}, {} ago\n$ {}\n{}",
            last.id,
            last.status.text(),
            elapsed(now - last.started),
            last.command,
            tail(last)
        ));
    }
    if done.len() > 1 {
        let before = &done[..done.len() - 1];
        let named: Vec<String> =
            before.iter().rev().take(NAMED).map(|x| format!("{} `{}` {}", x.id, short(&x.command), x.status.text())).collect();
        let more = before.len().saturating_sub(NAMED);
        let more = if more > 0 { format!(", and {more} older") } else { String::new() };
        out.push_str(&format!("\n\nEarlier: {}{more}", named.join(" · ")));
    }
    if running.is_empty() && done.is_empty() {
        out.push_str("\n\nNo sessions yet: run(command=\"...\") runs one and waits; start(...) tells you when it ends; background(...) does not.");
    }
    out
}

// ---- read mode: `uses: [terminal:read]` ----

/// Refuse a command known to write, when the terminal artifact `text` is
/// read-only (`uses: [terminal:read]`, `crate::capability::READ`).
fn guard(text: &str, command: &str) -> Result<(), String> {
    if !crate::capability::read_only(text).iter().any(|id| id == "terminal") {
        return Ok(());
    }
    match writes(command) {
        None => Ok(()),
        Some(why) => Err(format!(
            "refused: this terminal is read-only (uses: [terminal:read]) and {why}. Run only commands that read (git status, git diff, git log, git show, grep, cat, ls) and the project's checks (tests, build, lint without --fix or --write); say what should change in your reply"
        )),
    }
}

/// Why `command` is known to write files, if it is: an unquoted `>`
/// redirect to a file, a program that writes (`rm`, `mv`, `cp`, `tee`,
/// `sed -i`, ...), a git command that changes the tree, the index, the
/// refs or the config, an install, or a formatter or fixer that rewrites.
/// `sh -c "..."` is read inside, and `env`, `xargs`, `npx`, `bunx`,
/// `bun x` are looked through.
///
/// A cheap guard, word by word: it stops the usual slips, not a program
/// that writes by itself (a script, a test, `node -e`, a package.json
/// script, a build's own output). It is not a sandbox.
pub fn writes(command: &str) -> Option<String> {
    let (segments, redirect) = split(command);
    if let Some(r) = redirect {
        return Some(format!("`{r}` writes a file"));
    }
    segments.iter().find_map(|words| writes_words(words))
}

/// The command's segments (split at `;`, `&&`, `||`, `|`, `&`, a line
/// break, `(` and `)`), each as words with the quotes taken off; and the
/// first redirect to a file, if any (`2>&1`, `>&2` and `/dev/null` are not
/// files).
fn split(command: &str) -> (Vec<Vec<String>>, Option<String>) {
    let mut segments: Vec<Vec<String>> = vec![Vec::new()];
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut redirect = None;
    let mut chars = command.chars().peekable();
    fn end_word(word: &mut String, segments: &mut [Vec<String>]) {
        if !word.is_empty()
            && let Some(last) = segments.last_mut()
        {
            last.push(std::mem::take(word));
        }
    }
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => {
                if let Some(n) = chars.next() {
                    word.push(n);
                }
            }
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => quote = Some(c),
            (None, ' ' | '\t') => end_word(&mut word, &mut segments),
            // `&>file` is a redirect: the `>` is read next.
            (None, '&') if chars.peek() == Some(&'>') => {}
            (None, ';' | '|' | '&' | '\n' | '(' | ')') => {
                end_word(&mut word, &mut segments);
                segments.push(Vec::new());
            }
            (None, '>') => {
                // A file descriptor before it (`2>`) is not a word.
                if !word.chars().all(|d| d.is_ascii_digit()) {
                    end_word(&mut word, &mut segments);
                }
                word.clear();
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                // `2>&1`, `>&2`: a stream, not a file.
                if chars.peek() == Some(&'&') {
                    chars.next();
                    while chars.peek().is_some_and(|d| d.is_ascii_digit() || *d == '-') {
                        chars.next();
                    }
                    continue;
                }
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                let mut target = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || ";|&()<>".contains(n) {
                        break;
                    }
                    target.push(n);
                    chars.next();
                }
                let target = target.trim_matches(|q| q == '"' || q == '\'').to_string();
                if !["/dev/null", "/dev/stdout", "/dev/stderr"].contains(&target.as_str()) && redirect.is_none() {
                    redirect = Some(format!("> {target}"));
                }
            }
            (None, c) => word.push(c),
        }
    }
    end_word(&mut word, &mut segments);
    segments.retain(|s| !s.is_empty());
    (segments, redirect)
}

/// Programs that write files whatever their arguments.
const WRITERS: &[&str] = &[
    "rm", "rmdir", "mv", "cp", "mkdir", "touch", "chmod", "chown", "chgrp", "ln", "truncate", "dd", "install", "patch", "shred", "unlink",
    "rsync", "tee", "eval",
];
/// Git subcommands that change the work tree, the index or the refs.
const GIT_WRITES: &[&str] = &[
    "add",
    "am",
    "apply",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "fetch",
    "gc",
    "init",
    "merge",
    "mv",
    "notes",
    "prune",
    "pull",
    "push",
    "rebase",
    "replace",
    "reset",
    "restore",
    "revert",
    "rm",
    "submodule",
    "switch",
    "update-index",
    "update-ref",
    "worktree",
];
/// Package manager and toolchain subcommands that install, remove, or
/// rewrite files (a lock file, the sources).
const INSTALLS: &[&str] = &[
    "install",
    "i",
    "ci",
    "add",
    "remove",
    "rm",
    "uninstall",
    "un",
    "update",
    "upgrade",
    "up",
    "link",
    "unlink",
    "prune",
    "dedupe",
    "init",
    "create",
    "publish",
    "patch",
    "sync",
    "fmt",
    "fix",
    "new",
    "clean",
];

fn writes_words(words: &[String]) -> Option<String> {
    // Look through `A=b`, `env`, `time`, `nice`, `nohup`, `command`,
    // `exec`, `sudo`, `xargs` (and its flags).
    let mut i = 0;
    while let Some(w) = words.get(i) {
        let assign = w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let wrapper = ["env", "time", "nice", "nohup", "command", "exec", "xargs", "sudo"].contains(&w.as_str());
        let xargs_flag = i > 0 && words[i - 1] == "xargs" && w.starts_with('-');
        if !(assign || wrapper || xargs_flag) {
            break;
        }
        i += 1;
    }
    let words = &words[i..];
    let prog = words.first()?.rsplit('/').next().unwrap_or("").to_string();
    let args = &words[1..];
    let has = |f: &str| args.iter().any(|a| a == f);
    let first_arg = args.iter().find(|a| !a.starts_with('-')).map_or("", String::as_str);
    let why = |what: &str| Some(format!("`{}` {what}", words.iter().take(3).cloned().collect::<Vec<_>>().join(" ")));
    match prog.as_str() {
        p if WRITERS.contains(&p) => why("writes or removes files"),
        "sh" | "bash" | "zsh" | "dash" => {
            let at = args.iter().position(|a| a == "-c" || a == "-lc" || a == "-ec")?;
            writes(args.get(at + 1)?)
        }
        "sed" | "perl" if args.iter().any(|a| a.starts_with("-i") || a == "--in-place") => why("edits files in place"),
        "find" if ["-delete", "-exec", "-execdir", "-ok", "-fprint"].iter().any(|f| has(f)) => why("can change files"),
        "git" => {
            // The subcommand: the first word that is not a flag or a
            // flag's value (`-C dir`, `-c k=v`).
            let mut k = 0;
            while let Some(a) = args.get(k) {
                if ["-C", "-c", "--git-dir", "--work-tree"].contains(&a.as_str()) {
                    k += 2;
                } else if a.starts_with('-') {
                    k += 1;
                } else {
                    break;
                }
            }
            let sub = args.get(k).map_or("", String::as_str);
            let rest = &args[(k + 1).min(args.len())..];
            let flag = |f: &[&str]| rest.iter().any(|a| f.contains(&a.as_str()));
            let positional = rest.iter().any(|a| !a.starts_with('-'));
            let names = (sub == "branch" || sub == "tag")
                && ((positional && !flag(&["--list", "-l", "--contains", "--merged", "--points-at"]))
                    || flag(&["-d", "-D", "-m", "-M", "--delete"]));
            let stash = sub == "stash" && !matches!(rest.first().map(String::as_str), Some("list" | "show"));
            let config = sub == "config" && !flag(&["--get", "--list", "-l", "--get-all", "--get-regexp"]);
            if GIT_WRITES.contains(&sub) || names || stash || config { why("changes the tree, the index or the refs") } else { None }
        }
        "npm" | "pnpm" | "yarn" | "bun" | "deno" | "pip" | "pip3" | "uv" | "poetry" | "cargo" | "go" | "brew" | "apt" | "apt-get"
        | "gem" => {
            // Look through the runners: `bun x`, `npm exec`, `pnpm dlx`.
            if ["x", "exec", "dlx"].contains(&first_arg) && !["cargo", "go"].contains(&prog.as_str()) {
                let at = args.iter().position(|a| a == first_arg)?;
                return writes_words(&args[at + 1..]);
            }
            match (prog.as_str(), first_arg) {
                ("cargo" | "deno", "fmt") if has("--check") => None,
                ("cargo", "clippy") if has("--fix") => why("rewrites files"),
                ("go", s) if ["get", "install", "mod", "generate", "fmt"].contains(&s) => why("installs or rewrites files"),
                ("go", _) => None,
                ("bun" | "npm" | "pnpm" | "yarn", "test") if has("-u") || has("--update-snapshots") => why("rewrites snapshot files"),
                (_, s) if INSTALLS.contains(&s) => why("installs, removes or rewrites files"),
                _ => None,
            }
        }
        "npx" | "bunx" | "pnpx" => writes_words(args),
        "prettier" | "biome" | "eslint" | "stylelint" | "ruff" | "black" | "isort" | "rustfmt" | "gofmt" | "goimports" | "dprint"
        | "jest" | "vitest" => {
            let flagged = ["--write", "-w", "--fix", "--apply", "--update-snapshots"].iter().any(|f| has(f));
            let snapshots = ["jest", "vitest"].contains(&prog.as_str()) && has("-u");
            let by_default = ["rustfmt", "black", "isort", "dprint"].contains(&prog.as_str()) && !has("--check");
            let ruff_format = prog == "ruff" && first_arg == "format" && !has("--check");
            if flagged || snapshots || by_default || ruff_format { why("rewrites files") } else { None }
        }
        _ if has("--update-snapshots") => why("rewrites snapshot files"),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::live::Content;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Processes in memory: `run` prints `ran <command>` and exits 0 (or 1
    /// for a command with `fail` in it); a started one runs until `end`.
    #[derive(Default)]
    pub struct FakeShell {
        pub now: RefCell<i64>,
        pub logs: RefCell<BTreeMap<u32, String>>,
        pub ended: RefCell<BTreeMap<u32, Option<i32>>>,
        pub started: RefCell<Vec<String>>,
    }

    impl FakeShell {
        pub fn end(&self, session: u32, code: Option<i32>, printed: &str) {
            self.logs.borrow_mut().entry(session).or_default().push_str(printed);
            self.ended.borrow_mut().insert(session, code);
        }
    }

    impl Host for FakeShell {
        fn now(&self) -> i64 {
            *self.now.borrow()
        }
        fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }
        fn read(&self, _: &str, p: &str) -> Content {
            Content::Missing(format!("no {p}"))
        }
        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Err("read-only".into())
        }
        fn run(&self, job: &Job) -> Result<Option<i32>, String> {
            self.logs.borrow_mut().insert(job.session, format!("compiling\nran {}\n", job.command));
            let code = if job.command.contains("fail") { 1 } else { 0 };
            self.ended.borrow_mut().insert(job.session, Some(code));
            Ok(Some(code))
        }
        fn spawn(&self, job: &Job) -> Result<(), String> {
            self.started.borrow_mut().push(format!("{} {} in {}", job.session, job.command, job.root));
            Ok(())
        }
        fn status(&self, job: &Job) -> Proc {
            self.ended.borrow().get(&job.session).map_or(Proc::Running, |c| Proc::Ended(*c))
        }
        fn tail(&self, job: &Job, lines: usize) -> String {
            last_lines(self.logs.borrow().get(&job.session).map_or("", String::as_str), lines)
        }
    }

    fn c(s: &str) -> Call {
        crate::calls::parse(s).unwrap().remove(0)
    }

    #[test]
    fn the_state_reads_back_as_written() {
        let s = State {
            root: "/work/x".into(),
            sessions: vec![
                Session { id: 1, mode: Mode::Run, status: Status::Exit(-1), started: 5, command: "echo 'a b'\necho \\".into() },
                Session { id: 2, mode: Mode::Start, status: Status::Running, started: 6, command: "cargo test".into() },
                Session { id: 3, mode: Mode::Background, status: Status::Stopped, started: 7, command: "x".into() },
            ],
        };
        assert_eq!(State::parse(&s.to_text()), Ok(s.clone()));
        assert!(s.to_text().contains("session: 1 run exit:-1 5 echo 'a b'\\necho \\\\\nsession: 2 start running 6 cargo test\n"));
        assert!(State::parse("kind: terminal\nsession: 1 fly").is_err());
    }

    #[test]
    fn run_waits_and_start_and_background_return_at_once() {
        let sh = FakeShell::default();
        let mut text = "kind: terminal\nroot: .".to_string();
        assert_eq!(call("e", "term", &mut text, &c("run(command=\"cargo check\")"), &sh), Ok("session 1 `cargo check`: exit 0\ncompiling\nran cargo check".into()));
        assert!(call("e", "term", &mut text, &c("run(command=\"fail now\")"), &sh).unwrap().starts_with("session 2 `fail now`: exit 1"));
        assert_eq!(call("e", "term", &mut text, &c("start(command=\"cargo test\")"), &sh), Ok("session 3 `cargo test` started; a message comes when it ends".into()));
        assert!(call("e", "term", &mut text, &c("background(command=\"npm run dev\")"), &sh).unwrap().contains("no message when it ends"));
        assert!(call("e", "term", &mut text, &c("run()"), &sh).unwrap_err().contains("needs command"));
        assert_eq!(*sh.started.borrow(), ["3 cargo test in .", "4 npm run dev in ."], "commands run in the root");
        assert!(text.contains("session: 3 start running 0 cargo test\nsession: 4 background running 0 npm run dev"), "{text}");
    }

    #[test]
    fn a_started_session_calls_back_once_and_a_background_one_never() {
        let sh = FakeShell::default();
        let mut text = "kind: terminal".to_string();
        call("e", "t", &mut text, &c("start(command=\"cargo test\")"), &sh).unwrap();
        call("e", "t", &mut text, &c("background(command=\"serve\")"), &sh).unwrap();
        assert!(settle("e", "t", &mut text, &sh).is_empty(), "both still running");
        sh.end(1, Some(0), "test result: ok\n");
        sh.end(2, None, "");
        assert_eq!(settle("e", "t", &mut text, &sh), ["session 1 `cargo test` finished, exit 0 (terminal t)"]);
        assert!(text.contains("session: 1 start exit:0") && text.contains("session: 2 background stopped"));
        assert!(settle("e", "t", &mut text, &sh).is_empty(), "once");
    }

    #[test]
    fn the_snapshot_shows_running_then_the_last_then_names() {
        let sh = FakeShell::default();
        let mut text = "kind: terminal".to_string();
        assert!(snapshot("e", "t", &text, &sh).contains("No sessions yet"));
        for cmd in ["ls", "pwd", "echo hi"] {
            call("e", "t", &mut text, &c(&format!("run(command=\"{cmd}\")")), &sh).unwrap();
        }
        call("e", "t", &mut text, &c("start(command=\"cargo build\")"), &sh).unwrap();
        sh.logs.borrow_mut().insert(4, "Compiling a\r  Compiling b\nBuilding [==>  ] 3/9\r".into());
        *sh.now.borrow_mut() = 75;
        let snap = snapshot("e", "t", &text, &sh);
        assert!(snap.contains("#### session 4 — start, running 1m 15s, a message when it ends\n$ cargo build\n  Compiling b\n"), "{snap}");
        assert!(snap.contains("#### last: session 3 — exit 0, 1m 15s ago\n$ echo hi\ncompiling\nran echo hi"), "{snap}");
        assert!(snap.ends_with("Earlier: 2 `pwd` exit 0 · 1 `ls` exit 0"), "{snap}");
    }

    #[test]
    fn old_finished_sessions_go_first() {
        let sh = FakeShell::default();
        let mut text = "kind: terminal".to_string();
        call("e", "t", &mut text, &c("background(command=\"keep\")"), &sh).unwrap();
        for _ in 0..MAX_SESSIONS + 3 {
            call("e", "t", &mut text, &c("run(command=\"x\")"), &sh).unwrap();
        }
        let s = State::parse(&text).unwrap();
        assert_eq!(s.sessions.len(), MAX_SESSIONS);
        assert_eq!(s.sessions[0].command, "keep", "a running one stays");
        assert_eq!(s.sessions.last().unwrap().id, MAX_SESSIONS as u32 + 4);
    }

    #[test]
    fn secret_variables_are_named_so_the_shell_can_drop_them() {
        assert!(secret_env("OPENAI_API_KEY") && secret_env("github_token") && secret_env("AWS_SECRET_ACCESS_KEY"));
        assert!(!secret_env("PATH") && !secret_env("CARGO_HOME"));
    }

    #[test]
    fn a_read_only_terminal_refuses_the_commands_known_to_write() {
        for cmd in [
            "rm -rf src",
            "echo hi > a.txt",
            "echo hi >> a.txt",
            "cat x | tee out",
            "sed -i '' s/a/b/ f.ts",
            "git checkout -- greet.ts",
            "git -C . commit -m x",
            "git stash",
            "git branch new",
            "git status && git add .",
            "bun add left-pad",
            "npm install",
            "cargo fmt",
            "bunx prettier --write .",
            "bun x biome check --apply",
            "sh -c 'mv a b'",
            "find . -name x -delete",
            "ls | xargs -0 rm",
            "FOO=1 env touch x",
            "bun test -u",
        ] {
            assert!(writes(cmd).is_some(), "{cmd}");
        }
        for cmd in [
            "git status --short && git diff --stat",
            "git diff -- greet.ts",
            "git log --oneline -5",
            "git branch",
            "git branch --list",
            "git stash list",
            "git config --get user.name",
            "bun test",
            "bun run index.ts Ada --shout",
            "bun build index.ts --target bun 2>&1",
            "cargo test -q 2>/dev/null",
            "cargo fmt --check",
            "grep -n 'a > b' greet.ts",
            "echo \"x > y\"",
            "rg -n greet",
            "ls -la && cat greet.ts",
            "kill 123",
        ] {
            assert_eq!(writes(cmd), None, "{cmd}");
        }
        let sh = FakeShell::default();
        let mut text = "uses: [terminal:read]\nroot: .".to_string();
        let why = call("e", "t", &mut text, &c("run(command=\"git commit -am x\")"), &sh).unwrap_err();
        assert!(
            why.starts_with("refused: this terminal is read-only (uses: [terminal:read]) and `git commit -am` changes the tree"),
            "{why}"
        );
        assert!(call("e", "t", &mut text, &c("background(command=\"rm -rf .\")"), &sh).is_err());
        assert!(sh.started.borrow().is_empty() && !text.contains("session:"), "nothing ran: {text}");
        assert!(call("e", "t", &mut text, &c("run(command=\"git status --short\")"), &sh).unwrap().contains("exit 0"));
        // Without the mode, the same command runs.
        let mut open = "kind: terminal\nroot: .".to_string();
        assert!(call("e", "t", &mut open, &c("run(command=\"git commit -am x\")"), &sh).is_ok());
    }
}
