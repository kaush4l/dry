//! Live artifacts: artifacts that show the state as it is now.
//!
//! An artifact whose text starts with a `kind: <kind>` line is live. What
//! is stored is only that line; at every render its body is replaced by a
//! snapshot of the state it names, and the snapshot is never written into
//! the engine's messages, so the model always reads the state as it is,
//! not as it was some turns ago.
//!
//! ```text
//! artifacts/now.md       kind: clock      → 2026-11-22 Sunday 14:05 UTC
//! artifacts/scripts.md   kind: scripts    → - build_check(file): ...
//! artifacts/code.md      kind: file       → the files under its root, the open ones
//! artifacts/term.md      kind: terminal   → what runs, what last ran
//! artifacts/kids.md      kind: engines    → its child engines, their replies
//! artifacts/shared.md    kind: shared     → its thread (its root's)
//! artifacts/learn.md     kind: learning   → its learning's room, a collection
//! artifacts/page.md      kind: browser    → a page of the app it started: text, errors, screenshot
//! artifacts/ev.md        kind: evidence   → the project's evidence: shots, what changed (docs/evidence.md)
//! ```
//!
//! Most kinds store only that line. `file`, `terminal`, `engines` and
//! `learning` keep their state in the lines after it (`crate::file`,
//! `crate::terminal`, `crate::engines`, `crate::learning`), changed only
//! through their tools; `write` refuses them. `shared` keeps its state at
//! its level, outside the engine (`crate::shared`).
//!
//! The kinds are a closed enum, so every kind has one fixed meaning (the
//! line the model is told), and a kind may bring tools that act on its
//! state (`tools`): the toolbox at a step is the engine's own tools plus
//! those of the live artifacts it holds. A new kind is one new variant.
//!
//! Pure. What only the shell can know (the time, the disk, processes)
//! comes in as `Snapshots` and through `Host`; everything else is read
//! from the engine.

use crate::calls::Call;
use crate::state::Engine;

/// A kind of live artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Live {
    /// The date and time now. Rendered last: it changes every minute, so
    /// everything before it stays the same text between renders.
    Clock,
    /// The engine's scripts, one line each. Brings the `script` tool: a
    /// script can only be run while the model can see the list.
    Scripts,
    /// The engine's alerts, one line each.
    Alerts,
    /// Files on disk under a root, and the ones the engine has open
    /// (`crate::file`). Brings open, focus, edit, save.
    File,
    /// The engine's terminal sessions (`crate::terminal`). Brings run,
    /// start, background.
    Terminal,
    /// Its child engines (`crate::engines`). Brings engine_call,
    /// engine_start.
    Engines,
    /// Its root's thread (`crate::shared`). Brings post,
    /// edit_post.
    Shared,
    /// Its learning file and curated collections (`crate::learning`).
    /// Brings collect, learn.
    Learning,
    /// A page in a headless browser (`crate::browser`): its text, its
    /// console errors, a screenshot. Brings browse, click, type.
    Browser,
    /// The project's evidence (`crate::evidence::artifact`): its shots
    /// against the committed baseline. Brings evidence.
    Evidence,
}

impl Live {
    pub const ALL: [Live; 10] = [
        Live::Clock,
        Live::Scripts,
        Live::Alerts,
        Live::File,
        Live::Terminal,
        Live::Engines,
        Live::Shared,
        Live::Learning,
        Live::Browser,
        Live::Evidence,
    ];

    /// Its word in the `kind:` line.
    pub fn as_str(self) -> &'static str {
        match self {
            Live::Clock => "clock",
            Live::Scripts => "scripts",
            Live::Alerts => "alerts",
            Live::File => "file",
            Live::Terminal => "terminal",
            Live::Engines => "engines",
            Live::Shared => "shared",
            Live::Learning => "learning",
            Live::Browser => "browser",
            Live::Evidence => "evidence",
        }
    }

    /// The one line the model is told about it.
    pub fn meaning(self) -> &'static str {
        match self {
            Live::Clock => "the date and time now",
            Live::Scripts => "your scripts: call sequences you run with one script(...) call",
            Live::Alerts => "your alerts: what wakes you, and when",
            Live::File => "files on disk as they are now, and the ones you have open",
            Live::Terminal => "your terminal: what is running and what last ran",
            Live::Engines => "your child engines: their goals, what they are doing, their replies",
            Live::Shared => "your thread: the user and every engine of your tree; how you talk to each other",
            Live::Learning => "what you learned, and the context you collected to learn from",
            Live::Browser => "a page in a browser as it is now: its text, its console errors, its last screenshot",
            Live::Evidence => {
                "the project's evidence: every scenario's screenshot and page text against the committed baseline, and what changed"
            }
        }
    }

    /// The tools that come with it (STANDARDS L3e in the old project): they
    /// exist only while the artifact does. Alerts bring none yet: whether an
    /// engine makes alerts with a tool or by writing files is still open.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Live::Scripts => &["script"],
            Live::File => &crate::file::TOOLS,
            Live::Terminal => &crate::terminal::TOOLS,
            Live::Engines => &crate::engines::TOOLS,
            Live::Shared => &crate::shared::TOOLS,
            Live::Learning => &crate::learning::TOOLS,
            Live::Browser => &crate::browser::TOOLS,
            Live::Evidence => &crate::evidence::artifact::TOOLS,
            Live::Clock | Live::Alerts => &[],
        }
    }

    /// The kind of an artifact's text: its first line is `kind: <kind>`.
    ///
    /// `uses: [a, b]` (`crate::capability`): the first capability it uses
    /// that a kind runs in core.
    pub fn of(text: &str) -> Option<Live> {
        crate::capability::uses(text)?.iter().find_map(|id| crate::capability::compiled_live(id))
    }

    /// Whether its state is in its own file, changed only by its tools.
    pub fn has_state(self) -> bool {
        matches!(self, Live::File | Live::Terminal | Live::Engines | Live::Learning | Live::Browser | Live::Evidence)
    }

    /// Artifact `name`'s body at this render. `None`: it needs something
    /// the shell did not pass (a render for showing, not for sending).
    pub fn snapshot(self, name: &str, e: &Engine, outside: &Snapshots) -> Option<String> {
        let text = e.artifacts.get(name).map_or("", String::as_str);
        match self {
            Live::Clock => outside.clock.map(|c| c.to_string()),
            Live::Scripts => Some(crate::script::snapshot(&e.scripts)),
            Live::Alerts => Some(crate::alert::snapshot(&e.alerts)),
            Live::File => outside.host.map(|h| crate::file::snapshot(text, h)),
            Live::Terminal => outside.host.map(|h| crate::terminal::snapshot(&e.id, name, text, h)),
            Live::Engines => outside.family.map(|f| crate::engines::snapshot(text, e, f)),
            Live::Shared => outside.family.map(|f| crate::shared::snapshot(&f.shared, &e.id, &f.thread, &f.others, f.offset)),
            Live::Learning => Some(crate::learning::snapshot(text, e)),
            Live::Browser => outside.host.map(|h| crate::browser::snapshot(&e.id, name, text, h)),
            Live::Evidence => outside.host.map(|h| crate::evidence::artifact::snapshot(text, h)),
        }
    }
}

/// The live artifacts engine `e` holds, `(artifact name, kind)`, in
/// render order: by kind, the clock first in the enum but placed last by
/// `prompt` (see there), then by name.
///
/// Only those whose capability is present on this machine
/// (`crate::capability::equipped`): an absent one is not rendered and
/// brings no tools.
pub fn present(e: &Engine) -> Vec<(String, Live)> {
    let mut out: Vec<(String, Live)> = crate::capability::equipped(e).into_iter().filter_map(|q| q.live.map(|l| (q.name, l))).collect();
    out.sort_by_key(|(n, l)| (*l == Live::Clock, *l, n.clone()));
    out
}

/// The tools the engine may call at this step: its own, then those its
/// live artifacts bring (each once).
pub fn toolbox(e: &Engine) -> Vec<String> {
    let mut tools = e.tools.clone();
    // Skills: `skill` while there is one to load, the loaded ones' tools.
    let skills = e.visible_skills();
    if !skills.is_empty() && !tools.iter().any(|t| t == crate::skill::LOAD) {
        tools.push(crate::skill::LOAD.to_string());
    }
    for (_, text) in skills.iter().filter(|(n, _)| e.loaded_skills.iter().any(|l| l == n)) {
        for t in crate::skill::parse(text).map(|s| s.tools).unwrap_or_default() {
            if !tools.contains(&t) {
                tools.push(t);
            }
        }
    }
    // The current phase's tools (and goto_phase while there is another).
    for t in e.run.tools(&e.phase_list().unwrap_or_default()) {
        if !tools.contains(&t) {
            tools.push(t);
        }
    }
    // Every present capability its artifacts use: a compiled one's tools by
    // their names, an outside one's as `<id>.<tool>`.
    // A capability held in read mode (`uses: [file:read]`) leaves its
    // writing tools out.
    for q in &crate::capability::equipped(e) {
        let names = q.compiled_tools().into_iter().chain(q.outside.iter().flat_map(|c| c.tools.iter().map(|t| c.call_name(t))));
        for name in names {
            if !tools.contains(&name) {
                tools.push(name);
            }
        }
    }
    tools
}

/// The kind a tool call belongs to and the artifact of that kind it acts
/// on (`artifact="..."`, needed only when there are several). `None`: no
/// kind brings it, or the engine holds none of that kind.
fn target(e: &Engine, call: &Call) -> Option<Result<(Live, String), String>> {
    let (id, kind) = crate::capability::compiled_for_tool(&call.name).filter(|(_, k)| *k != Live::Scripts)?;
    let names: Vec<String> =
        crate::capability::equipped(e).into_iter().filter(|q| q.compiled.iter().any(|c| c.id == id)).map(|q| q.name).collect();
    Some(match (call.arg("artifact"), names.as_slice()) {
        (Some(n), _) if names.iter().any(|have| have == n) => Ok((kind, n.to_string())),
        (Some(n), _) => Err(format!("no {id} artifact `{n}`; yours: {}", names.join(", "))),
        (None, [one]) => Ok((kind, one.clone())),
        (None, []) => return None,
        (None, _) => Err(format!("you have several {id} artifacts ({}): say which with artifact=\"...\"", names.join(", "))),
    })
}

/// Whether a call to a kind's tool waits (the terminal's `run`): it is
/// prepared here and run by the shell with the act's other waits.
pub fn waits(e: &Engine, call: &Call) -> bool {
    call.name == crate::terminal::TOOLS[0] && matches!(target(e, call), Some(Ok((Live::Terminal, _))))
}

/// The first half of a waiting call (`waits`): its session recorded, the
/// job the shell runs.
pub fn prepare(e: &mut Engine, call: &Call, host: &dyn Host) -> Result<crate::terminal::RunJob, String> {
    let (_, target) = target(e, call).ok_or("no terminal artifact")??;
    let id = e.id.clone();
    let text = e.artifacts.get_mut(&target).ok_or("no terminal artifact")?;
    let header = crate::capability::first_line(text);
    let job = crate::terminal::prepare_run(&id, &target, text, call, host);
    crate::capability::keep_header(text, &header);
    job
}

/// Run a call to a tool a kind brings (`open`, `run`, `engine_call`, ...)
/// on the artifact it names (`artifact="..."`, needed only when the
/// engine holds more than one of that kind). `family`: what the shell
/// knows of the other engines (`None`: the engines and shared tools say
/// they cannot work here). `None`: not such a tool.
pub fn call(e: &mut Engine, call: &Call, host: &dyn Host, family: Option<&mut crate::engines::Family>) -> Option<Result<String, String>> {
    let (kind, target) = match target(e, call)? {
        Ok(found) => found,
        Err(why) => return Some(Err(why)),
    };
    // A writing tool on an artifact that holds its capability in read mode
    // is refused, however the call got here.
    if let Some(text) = e.artifacts.get_key_value(&target)
        && let Some(why) = crate::capability::refusal([text], &call.name)
    {
        return Some(Err(why));
    }
    let id = e.id.clone();
    let header = e.artifacts.get(&target).map(|t| crate::capability::first_line(t)).unwrap_or_default();
    let done = call_kind(e, kind, &id, &target, call, host, family);
    // A `uses: [..]` artifact keeps its header through its kind's writes.
    if let Some(text) = e.artifacts.get_mut(&target) {
        crate::capability::keep_header(text, &header);
    }
    done
}

fn call_kind(
    e: &mut Engine,
    kind: Live,
    id: &str,
    target: &str,
    call: &Call,
    host: &dyn Host,
    family: Option<&mut crate::engines::Family>,
) -> Option<Result<String, String>> {
    let (id, target) = (id.to_string(), target.to_string());
    let no_family = || Err("this shell has no other engines to reach".to_string());
    Some(match kind {
        Live::File => crate::file::call(e.artifacts.get_mut(&target)?, call, host),
        Live::Terminal => {
            let done = crate::terminal::call(&id, &target, e.artifacts.get_mut(&target)?, call, host);
            // `start(..., wait="yes")`: the turn waits on the job.
            if done.is_ok()
                && let Some(n) = e.artifacts.get(&target).and_then(|t| crate::terminal::waited(call, t))
            {
                e.waiting.push(crate::terminal::wait_entry(&target, n));
            }
            done
        }
        Live::Browser => crate::browser::call(&id, &target, e.artifacts.get_mut(&target)?, call, host),
        Live::Evidence => crate::evidence::artifact::call(&id, &target, e.artifacts.get(&target)?, call, host),
        Live::Engines => family.map_or_else(no_family, |f| crate::engines::call(e, &target, call, f)),
        Live::Shared => family.map_or_else(no_family, |f| crate::engines::shared_call(e, call, f, host.now())),
        Live::Learning if call.name == crate::learning::TOOLS[0] => crate::learning::collect(e, &target, call.arg("what").unwrap_or("")),
        Live::Learning if call.name == crate::learning::TOOLS[2] => {
            crate::learning::analytics(e, &target, call.arg("query").unwrap_or(""), host)
        }
        Live::Learning => {
            let section = call.arg("section").unwrap_or("");
            let entry = call.arg("entry").unwrap_or("").to_string();
            let old = call.arg("old").map(str::to_string);
            crate::learning::learn(e, section, &entry, old.as_deref(), host.now())
        }
        _ => return None,
    })
}

/// What the shell knows at render time that the core cannot.
#[derive(Clone, Copy, Default)]
pub struct Snapshots<'a> {
    /// The time now. `None`: a render for showing; live clocks say so.
    pub clock: Option<Clock>,
    /// The disk and the processes, for `file` and `terminal` snapshots.
    /// `None`: a render for showing; those say they are taken when sent.
    pub host: Option<&'a dyn Host>,
    /// The other engines, for `engines` and `shared` snapshots
    /// (`crate::engines::family`). `None`: those say they are taken when sent.
    pub family: Option<&'a crate::engines::Family>,
}

/// A file as the shell read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Text(String),
    /// Not text (a zero byte, or not UTF-8): its size, and whether its
    /// name says it is an image.
    Binary {
        bytes: u64,
        image: bool,
    },
    /// Bigger than the shell reads.
    TooLarge(u64),
    /// Not there, or not readable: why.
    Missing(String),
}

/// How a process the shell started is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proc {
    Running,
    /// It ended: its exit code, or `None` (killed, or lost when the app
    /// restarted).
    Ended(Option<i32>),
}

/// One terminal session's process, as the shell knows it: the log is
/// `artifacts/<artifact>/<session>.log` in engine `engine`'s folder.
#[derive(Clone, Copy, Debug)]
pub struct Job<'a> {
    pub engine: &'a str,
    pub artifact: &'a str,
    pub session: u32,
    /// The folder it runs in, as the artifact says (`.` is the repo).
    pub root: &'a str,
    pub command: &'a str,
}

/// What only the shell can do: read and write files under a root, run
/// processes. The core decides everything else. Paths given are already
/// checked to stay inside the root (`crate::file::check_path`); the shell
/// also checks where links lead.
pub trait Host {
    /// Unix seconds now.
    fn now(&self) -> i64;
    /// The files under `root`, relative paths, folders' insides after the
    /// folder's own files, at most `max`.
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String>;
    fn read(&self, root: &str, path: &str) -> Content;
    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String>;
    /// Run a command and wait for it; its output goes to its log. Its exit
    /// code (`None`: killed by a signal).
    fn run(&self, job: &Job) -> Result<Option<i32>, String>;
    /// Start a command and return at once.
    fn spawn(&self, job: &Job) -> Result<(), String>;
    fn status(&self, job: &Job) -> Proc;
    /// The log's last `lines` lines.
    fn tail(&self, job: &Job, lines: usize) -> String;
    /// Drive a headless browser for a `browser` artifact (`crate::browser`):
    /// `op` is open, click, type or look, `args` its inputs (`url`,
    /// `target`, `text`, `press`: a key after typing, `page`: which page). The page as JSON (url,
    /// title, text, errors, screenshot, did), or why not. Most shells have
    /// none; the companion has one (docs/companion.md).
    fn browser(&self, op: &str, args: &[(String, String)]) -> Result<String, String> {
        let _ = (op, args);
        Err("no browser in this shell: connect a companion (docs/companion.md) to drive one".into())
    }
    /// The spans engine `engine` has recorded (`crate::spans`), for the
    /// `analytics` tool. A shell that keeps none says why.
    fn spans(&self, engine: &str) -> Result<Vec<crate::spans::Span>, String> {
        let _ = engine;
        Err("this shell keeps no spans; analytics needs the host, the window or the browser app".into())
    }
    /// Seconds east of UTC of this shell's clock (local days for analytics).
    fn offset(&self) -> i32 {
        0
    }
}

/// A moment on a wall clock: unix seconds plus the clock's offset east of
/// UTC. The core has no time zone database, so the shell passes the
/// offset; the text always names it, so a UTC clock is never mistaken for
/// local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub unix: i64,
    /// Seconds east of UTC (`-18000` is UTC-05:00).
    pub offset: i32,
}

/// Where a shell's clock comes from: the system's (`dry_tools::live`),
/// the browser's `Date`, or a fixed one (a `Clock` is its own source).
pub trait Time {
    fn clock(&self) -> Clock;
}

impl Time for Clock {
    fn clock(&self) -> Clock {
        *self
    }
}

/// Day 0 (1970-01-01) was a Thursday.
const WEEKDAYS: [&str; 7] = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"];

impl Clock {
    pub fn new(unix: i64, offset: i32) -> Clock {
        Clock { unix, offset }
    }

    fn local(self) -> i64 {
        self.unix + i64::from(self.offset)
    }

    /// Days since 1970-01-01 on this clock.
    pub fn day(self) -> i64 {
        self.local().div_euclid(86_400)
    }

    /// Minutes since midnight on this clock.
    pub fn minute(self) -> u32 {
        (self.local().rem_euclid(86_400) / 60) as u32
    }

    /// Saturday or Sunday on this clock.
    pub fn is_weekend(self) -> bool {
        is_weekend(self.day())
    }

    /// The unix second at `minute` past midnight of local `day`.
    pub fn at(self, day: i64, minute: u32) -> i64 {
        day * 86_400 + i64::from(minute) * 60 - i64::from(self.offset)
    }
}

/// Whether local day `day` (since 1970-01-01) is a Saturday or Sunday.
pub fn is_weekend(day: i64) -> bool {
    matches!(day.rem_euclid(7), 2 | 3)
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// The civil date of a day since 1970-01-01 (the inverse).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

impl std::fmt::Display for Clock {
    /// `2026-11-22 Sunday 14:05 UTC` (or `UTC+05:30`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (y, m, d) = civil_from_days(self.day());
        let min = self.minute();
        let zone = match self.offset {
            0 => "UTC".to_string(),
            o => format!("UTC{}{:02}:{:02}", if o < 0 { '-' } else { '+' }, o.abs() / 3600, o.abs() % 3600 / 60),
        };
        write!(f, "{y:04}-{m:02}-{d:02} {} {:02}:{:02} {zone}", WEEKDAYS[self.day().rem_euclid(7) as usize], min / 60, min % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::OpenAi;

    /// 2026-11-22 14:05:30 UTC, a Sunday.
    pub const SUNDAY: i64 = 1_795_356_330;

    #[test]
    fn a_clock_reads_as_a_date_a_weekday_and_its_zone() {
        assert_eq!(Clock::new(SUNDAY, 0).to_string(), "2026-11-22 Sunday 14:05 UTC");
        assert_eq!(Clock::new(SUNDAY, -5 * 3600).to_string(), "2026-11-22 Sunday 09:05 UTC-05:00");
        assert_eq!(Clock::new(SUNDAY, 11 * 3600).to_string(), "2026-11-23 Monday 01:05 UTC+11:00");
        assert!(Clock::new(SUNDAY, 0).is_weekend() && !Clock::new(SUNDAY, 11 * 3600).is_weekend());
        for days in [-1, 0, 59, 20_414, 2_932_896] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn an_artifact_is_live_when_its_first_line_says_so() {
        assert_eq!(Live::of("kind: clock"), Some(Live::Clock));
        assert_eq!(Live::of("kind:scripts\nanything after is ignored"), Some(Live::Scripts));
        assert_eq!(Live::of("kind: weather"), None, "only the known kinds");
        assert_eq!(Live::of("notes\nkind: clock"), None, "only the first line");
    }

    #[test]
    fn live_artifacts_bring_their_tools_and_the_clock_goes_last() {
        let mut e = Engine::new("E", "", OpenAi::default());
        e.tools = vec!["reply".into()];
        assert_eq!(toolbox(&e), ["reply"]);
        e.artifacts.insert("a-now".into(), "kind: clock".into());
        e.artifacts.insert("mine".into(), "kind: scripts".into());
        e.artifacts.insert("plan".into(), "1. read".into());
        assert_eq!(toolbox(&e), ["reply", "script"]);
        let names: Vec<String> = present(&e).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["mine", "a-now"]);
    }
}
