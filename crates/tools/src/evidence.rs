//! `dry evidence`: build the project, start its app, shoot every scenario
//! at every viewport in headless Chrome, compare with the committed
//! evidence, write the folder, and stop what it started (docs/evidence.md).
//!
//! ```text
//! dry evidence [--project <dir>] [--update] [--expect a,b@390x844] [--out <dir>]
//!              [--repeat N] [--only <scenario>] [--no-build] [--chrome <path>]
//!
//! evidence.md ─▶ build ({out}) ─▶ run ({port}) ─ready─▶ Chrome ─▶ per shot:
//!   fresh context · fixed viewport · reduced motion · pinned clock and seed ·
//!   only the app's origin loads · load + network idle + fonts · steps ·
//!   animations stopped · masks painted · PNG + page text
//! ─▶ compare (dry_core::evidence) ─▶ evidence/*.png *.txt *.diff.png manifest.json summary.md
//! ```
//!
//! Exit 0: every shot is the same, new, expected or updated. Exit 1: an
//! unexpected change (the summary says which). Exit 2: it could not run
//! (the spec, the build, the app, Chrome, a step).
//!
//! The app is started like a terminal `start` session: in its own process
//! group, its output in `{out}/run.log`, and stopped (TERM, then KILL) when
//! the run ends, whatever happened.

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use dry_core::evidence::image::{self, MASK, Rect};
use dry_core::evidence::report::{self, Manifest, Pair, Status};
use dry_core::evidence::spec::{self, Cover, Ready, Scenario, Spec, Step, Viewport};
use serde_json::{Value, json};

use crate::cdp::{self, Chrome};

type Result<T> = std::result::Result<T, String>;

/// What the command line asked.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub project: PathBuf,
    pub update: bool,
    pub expect: Vec<String>,
    pub out: Option<PathBuf>,
    pub repeat: u32,
    pub only: Vec<String>,
    pub no_build: bool,
    pub chrome: Option<String>,
    pub quiet: bool,
}

impl Options {
    pub fn parse(args: &[&str]) -> Result<Options> {
        let mut o = Options { project: PathBuf::from("."), repeat: 1, ..Options::default() };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = |name: &str| it.next().map(|v| v.to_string()).ok_or_else(|| format!("{name} needs a value"));
            match *a {
                "--project" => o.project = PathBuf::from(value("--project")?),
                "--update" => o.update = true,
                "--expect" => o.expect.extend(value("--expect")?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
                "--out" => o.out = Some(PathBuf::from(value("--out")?)),
                "--repeat" => o.repeat = value("--repeat")?.parse().ok().filter(|n| *n >= 1).ok_or("--repeat is 1 or more")?,
                "--only" => o.only.extend(value("--only")?.split(',').map(|s| s.trim().to_string())),
                "--no-build" => o.no_build = true,
                "--chrome" => o.chrome = Some(value("--chrome")?),
                "--quiet" => o.quiet = true,
                other => {
                    return Err(format!(
                        "dry evidence: no option `{other}` (--project, --update, --expect, --out, --repeat, --only, --no-build, --chrome, --quiet)"
                    ));
                }
            }
        }
        Ok(o)
    }
}

/// `dry evidence ...`: the exit code as the module says.
pub fn command(args: &[&str]) -> ExitCode {
    let o = match Options::parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match run(&o) {
        Ok(m) => {
            println!("evidence: {}", m.line());
            if m.status == "pass" { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        Err(e) => {
            eprintln!("dry evidence: {e}");
            ExitCode::from(2)
        }
    }
}

/// A process group started for the run, stopped when dropped.
struct App {
    child: Child,
    log: PathBuf,
}

impl App {
    fn start(command: &str, dir: &Path, log: PathBuf) -> Result<App> {
        let file = fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        let err = file.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command).current_dir(dir).stdin(Stdio::null()).stdout(file).stderr(err);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd.spawn().map_err(|e| format!("could not start `{command}`: {e}"))?;
        Ok(App { child, log })
    }

    fn tail(&self) -> String {
        let t = fs::read_to_string(&self.log).unwrap_or_default();
        dry_core::terminal::last_lines(&t, 12)
    }

    fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let pid = self.child.id();
        #[cfg(unix)]
        {
            let group = format!("-{pid}");
            let _ = Command::new("kill").args(["-TERM", "--", &group]).stderr(Stdio::null()).status();
            let until = Instant::now() + Duration::from_secs(3);
            while Instant::now() < until && self.running() {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = Command::new("kill").args(["-KILL", "--", &group]).stderr(Stdio::null()).status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A free port on 127.0.0.1 (the system's pick, never the owner's 8080,
/// 8090 or 7777).
pub fn free_port() -> Result<u16> {
    for _ in 0..10 {
        let l = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no free port: {e}"))?;
        let p = l.local_addr().map_err(|e| e.to_string())?.port();
        if ![8080, 8090, 7777].contains(&p) {
            return Ok(p);
        }
    }
    Err("no free port".into())
}

fn say(o: &Options, line: &str) {
    if !o.quiet {
        eprintln!("{line}");
    }
}

/// Run `sh -c command` in `dir`, its output to `log`; error with its tail.
fn sh(command: &str, dir: &Path, log: &Path) -> Result<()> {
    let file = fs::File::create(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let err = file.try_clone().map_err(|e| e.to_string())?;
    let st = Command::new("sh").arg("-c").arg(command).current_dir(dir).stdin(Stdio::null()).stdout(file).stderr(err).status();
    match st {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            Err(format!("`{command}` failed ({s}):\n{}", dry_core::terminal::last_lines(&fs::read_to_string(log).unwrap_or_default(), 15)))
        }
        Err(e) => Err(format!("could not run `{command}`: {e}")),
    }
}

fn wait_ready(app: &mut App, ready: &Ready, timeout: u64) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(timeout);
    loop {
        if !app.running() {
            return Err(format!("the app exited before it was ready:\n{}", app.tail()));
        }
        let up = match ready {
            Ready::Url(u) => ureq::get(u)
                .config()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(2)))
                .build()
                .call()
                .is_ok_and(|r| r.status().as_u16() == 200),
            Ready::Log(t) => fs::read_to_string(&app.log).unwrap_or_default().contains(t.as_str()),
        };
        if up {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(format!("the app was not ready within {timeout} s:\n{}", app.tail()));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// `git rev-parse --short HEAD`, with `+dirty` when the tree (the evidence
/// folder aside) has changes; `none` outside git.
fn commit(project: &Path) -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(project)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let Some(head) = git(&["rev-parse", "--short", "HEAD"]) else { return "none".into() };
    let dirty = git(&["status", "--porcelain", "--", ".", ":(exclude)evidence"]).is_some_and(|s| !s.is_empty());
    if dirty { format!("{head}+dirty") } else { head }
}

/// The build output's fingerprint: every file's path and hash, hashed.
fn fingerprint(dir: &Path) -> String {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.filter_map(std::result::Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if let Ok(bytes) = fs::read(&p) {
                let rel = p.strip_prefix(base).unwrap_or(&p).display().to_string();
                if !rel.ends_with(".log") {
                    out.push((rel, dry_core::evidence::sha::sha256(&bytes)));
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    if files.is_empty() {
        return "none".into();
    }
    let all: String = files.iter().map(|(p, h)| format!("{p} {h}\n")).collect();
    dry_core::evidence::sha::sha256(all.as_bytes())[..12].to_string()
}

/// Build, start, capture, compare, write. The manifest.
pub fn run(o: &Options) -> Result<Manifest> {
    let project = fs::canonicalize(&o.project).map_err(|e| format!("{}: {e}", o.project.display()))?;
    let md = fs::read_to_string(project.join("evidence.md")).map_err(|e| format!("{}/evidence.md: {e}", project.display()))?;
    let spec = spec::parse(&md)?;
    for name in o.expect.iter().chain(&o.only) {
        let scen = name.split('@').next().unwrap_or("");
        if !spec.scenarios.iter().any(|s| s.name == scen) {
            return Err(format!("no scenario `{scen}` in evidence.md"));
        }
    }
    let out = o.out.clone().unwrap_or_else(|| project.join("target").join("evidence"));
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let out = fs::canonicalize(&out).map_err(|e| e.to_string())?;
    let port = free_port()?.to_string();
    let (outs, projs) = (out.display().to_string(), project.display().to_string());
    let vars = [("port", port.as_str()), ("out", outs.as_str()), ("project", projs.as_str())];

    if let Some(b) = spec.build.as_ref().filter(|_| !o.no_build) {
        let t = Instant::now();
        let cmd = spec::expand(b, &vars);
        say(o, &format!("build: {cmd}"));
        sh(&cmd, &project, &out.join("build.log"))?;
        say(o, &format!("built in {:.1} s", t.elapsed().as_secs_f64()));
    }
    let build = fingerprint(&out);

    let (_app, base) = match &spec.run {
        Some(r) => {
            let cmd = spec::expand(&r.command, &vars);
            say(o, &format!("run: {cmd}"));
            let mut app = App::start(&cmd, &project, out.join("run.log"))?;
            let ready = match &r.ready {
                Ready::Url(u) => Ready::Url(spec::expand(u, &vars)),
                Ready::Log(t) => Ready::Log(spec::expand(t, &vars)),
            };
            let t = Instant::now();
            wait_ready(&mut app, &ready, r.timeout)?;
            say(o, &format!("ready in {:.1} s", t.elapsed().as_secs_f64()));
            (Some(app), spec::expand(&r.base(), &vars))
        }
        None => (None, String::new()),
    };
    if base.is_empty() {
        return Err("evidence.md has no ## run (what starts the app, and where it is)".into());
    }

    let bin =
        cdp::find_chrome(o.chrome.as_deref()).ok_or("no Chrome, Chromium, Edge or Brave on this machine (give one with --chrome PATH)")?;
    let mut chrome = Chrome::launch(&bin, &out)?;
    chrome.allow = vec![format!("{}/", spec::origin(&base))];

    let dir = project.join("evidence");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let previous = fs::read_to_string(dir.join("manifest.json")).ok().and_then(|t| Manifest::from_json(&t).ok());

    let started = Instant::now();
    let mut shots = Vec::new();
    let mut taken = Vec::new();
    for (scenario, v) in spec.shots() {
        let name = spec::shot_name(&scenario.name, v);
        if !o.only.is_empty() && !o.only.iter().any(|x| *x == scenario.name || *x == name) {
            continue;
        }
        let t = Instant::now();
        let mut captures = Vec::new();
        for _ in 0..o.repeat {
            captures.push(capture(&mut chrome, &spec, scenario, v, &base).map_err(|e| format!("{name}: {e}"))?);
        }
        let ms = t.elapsed().as_millis() as u64 / u64::from(o.repeat);
        let (png, text) = captures[0].clone();
        let flaky = captures.iter().any(|(p, t)| *p != png || *t != text);
        let before_png = fs::read(dir.join(format!("{name}.png"))).ok();
        let before_text = fs::read_to_string(dir.join(format!("{name}.txt"))).ok();
        let pair = Pair {
            name: &name,
            scenario: &scenario.name,
            viewport: &v.name(),
            before_png: before_png.as_deref(),
            before_text: before_text.as_deref(),
            before_sha: previous.as_ref().and_then(|m| m.sha_of(&name)),
            after_png: &png,
            after_text: &text,
        };
        let mut c = report::compare(&pair, spec.threshold)?;
        c.shot.ms = ms;
        if flaky {
            c.shot.status = Status::Flaky;
        } else if c.shot.status == Status::Changed {
            if o.expect.iter().any(|x| *x == scenario.name || *x == name) {
                c.shot.status = Status::Expected;
            } else if o.update {
                c.shot.status = Status::Updated;
            }
        }
        write_shot(&dir, &name, &c, &png, &text, o.update)?;
        say(o, &format!("{name}: {} {:.2}% · {ms} ms", c.shot.status.word(), c.shot.changed));
        taken.push(name);
        shots.push(c.shot);
    }
    if !chrome.blocked.is_empty() {
        say(
            o,
            &format!(
                "blocked {} request(s) outside the app's origin, e.g. {}",
                chrome.blocked.len(),
                chrome.blocked.iter().next().map_or("", String::as_str)
            ),
        );
    }
    let ms = started.elapsed().as_millis() as u64;
    drop(chrome);

    // Shots the last run took that this one did not: a scenario left the table.
    let mut removed = Vec::new();
    if o.only.is_empty()
        && let Some(prev) = &previous
    {
        for s in prev.shots.iter().filter(|s| !taken.contains(&s.name)) {
            if o.update {
                for ext in ["png", "txt", "new.png", "new.txt", "diff.png"] {
                    let _ = fs::remove_file(dir.join(format!("{}.{ext}", s.name)));
                }
            } else {
                removed.push(s.name.clone());
            }
        }
    }
    // A partial run (--only) keeps the other shots' rows as they were.
    if !o.only.is_empty()
        && let Some(prev) = &previous
    {
        for s in prev.shots.iter().filter(|s| !taken.contains(&s.name)) {
            shots.push(s.clone());
        }
        let order: Vec<String> = spec.shots().iter().map(|(s, v)| spec::shot_name(&s.name, *v)).collect();
        shots.sort_by_key(|s| order.iter().position(|n| *n == s.name).unwrap_or(usize::MAX));
    }
    let fail = shots.iter().any(|s| matches!(s.status, Status::Changed | Status::Flaky));
    let m = Manifest {
        build,
        commit: commit(&project),
        time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64),
        viewports: spec.viewports.iter().map(|v| v.name()).collect(),
        pixel: spec.threshold.pixel,
        percent: spec.threshold.percent,
        shots,
        removed,
        status: if fail { "fail" } else { "pass" }.into(),
        ms,
    };
    write(&dir.join("manifest.json"), m.to_json().as_bytes())?;
    write(&dir.join("summary.md"), m.summary().as_bytes())?;
    Ok(m)
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write one shot's files as its status says; the baseline stays unless
/// the change was accepted. `update`: a capture within the threshold but
/// not byte for byte the baseline replaces it too, so the manifest's hash
/// is the file's.
fn write_shot(dir: &Path, name: &str, c: &report::Compared, png: &[u8], text: &str, update: bool) -> Result<()> {
    let p = |ext: &str| dir.join(format!("{name}.{ext}"));
    let rm = |ext: &str| {
        let _ = fs::remove_file(p(ext));
    };
    match c.shot.status {
        Status::New | Status::Expected | Status::Updated => {
            write(&p("png"), png)?;
            write(&p("txt"), text.as_bytes())?;
            rm("new.png");
            rm("new.txt");
        }
        Status::Changed | Status::Flaky => {
            write(&p("new.png"), png)?;
            write(&p("new.txt"), text.as_bytes())?;
        }
        Status::Same => {
            if update && c.shot.baseline.as_deref() != Some(c.shot.sha256.as_str()) {
                write(&p("png"), png)?;
                write(&p("txt"), text.as_bytes())?;
            }
            rm("new.png");
            rm("new.txt");
        }
    }
    match &c.diff_png {
        Some(d) if c.shot.status.differs() => write(&p("diff.png"), d)?,
        _ => rm("diff.png"),
    }
    Ok(())
}

/// What the page runs before its own scripts: the clock pinned, random
/// seeded, motion stopped (docs/evidence.md, determinism).
fn init_script(spec: &Spec) -> String {
    format!(
        r#"(() => {{
  const T = {time};
  const RealDate = Date;
  const Fixed = new Proxy(RealDate, {{
    construct(target, args, nt) {{ return Reflect.construct(target, args.length ? args : [T], nt); }},
    apply() {{ return new RealDate(T).toString(); }},
  }});
  RealDate.now = () => T;
  window.Date = Fixed;
  let s = {seed} >>> 0;
  const next = () => {{ s = (s + 0x6D2B79F5) >>> 0; let t = s; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }};
  Math.random = next;
  if (window.crypto) {{
    const fill = (a) => {{ const b = new Uint8Array(a.buffer, a.byteOffset, a.byteLength); for (let i = 0; i < b.length; i++) b[i] = Math.floor(next() * 256); return a; }};
    try {{ Object.defineProperty(crypto, 'getRandomValues', {{ value: fill, configurable: true }}); }} catch (e) {{}}
    try {{ Object.defineProperty(crypto, 'randomUUID', {{ value: () => {{ const h = [...fill(new Uint8Array(16))].map((x) => x.toString(16).padStart(2, '0')).join(''); return `${{h.slice(0, 8)}}-${{h.slice(8, 12)}}-4${{h.slice(13, 16)}}-a${{h.slice(17, 20)}}-${{h.slice(20, 32)}}`; }}, configurable: true }}); }} catch (e) {{}}
  }}
  const css = `{css}`;
  const put = () => {{ if (document.getElementById('dry-evidence-still')) return; const st = document.createElement('style'); st.id = 'dry-evidence-still'; st.textContent = css; (document.head || document.documentElement).appendChild(st); }};
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', put); else put();
}})();"#,
        time = spec.capture.time_ms,
        seed = spec.capture.seed,
        css = STILL_CSS,
    )
}

/// Motion off, carets and scrollbars hidden.
const STILL_CSS: &str = "*,*::before,*::after{animation-duration:0s!important;animation-delay:0s!important;animation-iteration-count:1!important;transition-duration:0s!important;transition-delay:0s!important;caret-color:transparent!important;scroll-behavior:auto!important}::-webkit-scrollbar{display:none!important}html{scrollbar-width:none!important}";

/// Just before the shot: every animation finished (or held at its start),
/// focus dropped (no caret or focus ring), two frames painted.
const FREEZE: &str = r#"(async () => {
  for (const a of document.getAnimations()) { try { a.finish(); } catch (e) { a.pause(); a.currentTime = 0; } }
  if (document.activeElement && document.activeElement !== document.body) document.activeElement.blur();
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  return true;
})()"#;

/// Paint every element a selector matches with a flat box (fixed, over all).
fn mask_script(selectors: &[String]) -> String {
    format!(
        r#"(async () => {{
  for (const old of document.querySelectorAll('.dry-evidence-mask')) old.remove();
  let n = 0;
  for (const sel of {sel}) {{
    let els = [];
    try {{ els = [...document.querySelectorAll(sel)]; }} catch (e) {{ throw new Error('mask selector ' + sel + ' does not read'); }}
    for (const el of els) {{
      for (const r of el.getClientRects()) {{
        const d = document.createElement('div');
        d.className = 'dry-evidence-mask';
        d.style.cssText = `position:fixed;left:${{r.left}}px;top:${{r.top}}px;width:${{r.width}}px;height:${{r.height}}px;background:rgb({c0},{c1},{c2});z-index:2147483647;pointer-events:none`;
        document.documentElement.appendChild(d);
        n++;
      }}
    }}
  }}
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  return n;
}})()"#,
        sel = serde_json::to_string(selectors).unwrap_or_else(|_| "[]".into()),
        c0 = MASK[0],
        c1 = MASK[1],
        c2 = MASK[2],
    )
}

/// Find an element by a CSS selector or the text it shows; scroll to it;
/// its centre (the companion's `FIND`).
const FIND: &str = r#"(target, forTyping) => {
  let el = null;
  try { el = document.querySelector(target); } catch {}
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim().toLowerCase();
  const want = norm(target);
  if (!el) {
    const sel = forTyping ? 'input,textarea,select,[contenteditable=""],[contenteditable=true]' : 'a,button,input,select,textarea,summary,label,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[onclick],[data-act]';
    const all = [...document.querySelectorAll(sel)].filter((e) => e.getClientRects().length);
    const names = (e) => [e.innerText, e.value, e.getAttribute('aria-label'), e.getAttribute('placeholder'), e.getAttribute('title'), e.getAttribute('name'),
      e.id && document.querySelector('label[for="' + e.id + '"]')?.innerText, e.closest('label')?.innerText].map(norm);
    el = all.find((e) => names(e).includes(want)) || all.find((e) => names(e).some((n) => n && n.includes(want)));
  }
  if (!el) return null;
  el.scrollIntoView({ block: 'center', inline: 'center' });
  const r = el.getBoundingClientRect();
  return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
}"#;

/// Load, settle: the load event, a quiet network, fonts, two frames.
fn settle(chrome: &mut Chrome, s: &str) -> Result<()> {
    chrome.network_idle(s, Duration::from_millis(300), Duration::from_secs(10))?;
    chrome.eval(s, "document.fonts ? document.fonts.ready.then(() => true) : true")?;
    chrome.eval(s, "new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))")?;
    Ok(())
}

pub(crate) fn goto(chrome: &mut Chrome, s: &str, url: &str) -> Result<()> {
    let r = chrome.call("Page.navigate", json!({"url": url}), Some(s))?;
    if let Some(e) = r.get("errorText").and_then(Value::as_str) {
        return Err(format!("could not open {url}: {e}"));
    }
    // A route that changes only the #hash fires no load event.
    let same_doc = r.get("loaderId").is_none();
    if !same_doc {
        chrome.wait_event("Page.loadEventFired", s, Duration::from_secs(20))?;
    }
    settle(chrome, s)
}

pub(crate) fn click_at(chrome: &mut Chrome, s: &str, x: f64, y: f64) -> Result<()> {
    for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
        let count = if kind == "mouseMoved" { 0 } else { 1 };
        chrome.call("Input.dispatchMouseEvent", json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": count}), Some(s))?;
    }
    Ok(())
}

pub(crate) fn locate(chrome: &mut Chrome, s: &str, target: &str, typing: bool) -> Result<(f64, f64)> {
    let r = chrome.eval(s, &format!("({FIND})({}, {typing})", serde_json::to_string(target).unwrap_or_default()))?;
    match (r.get("x").and_then(Value::as_f64), r.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(format!("nothing on the page matches \"{target}\" (a CSS selector, or the text it shows)")),
    }
}

fn step(chrome: &mut Chrome, s: &str, st: &Step, base: &str) -> Result<()> {
    match st {
        Step::Click(t) => {
            let (x, y) = locate(chrome, s, t, false)?;
            click_at(chrome, s, x, y)?;
            chrome.pump(Duration::from_millis(100))?;
            settle(chrome, s)
        }
        Step::Type(t, text) => {
            let (x, y) = locate(chrome, s, t, true)?;
            click_at(chrome, s, x, y)?;
            chrome.call("Input.insertText", json!({"text": text}), Some(s))?;
            settle(chrome, s)
        }
        Step::Press(k) => {
            let code = match k.as_str() {
                "Enter" => 13,
                "Tab" => 9,
                "Escape" => 27,
                _ => 0,
            };
            let mut down = json!({"type": "keyDown", "key": k, "code": k, "windowsVirtualKeyCode": code});
            if k == "Enter" {
                down["text"] = json!("\r");
            }
            chrome.call("Input.dispatchKeyEvent", down, Some(s))?;
            chrome.call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": k, "code": k, "windowsVirtualKeyCode": code}), Some(s))?;
            settle(chrome, s)
        }
        Step::Wait(ms) => chrome.pump(Duration::from_millis(*ms)),
        Step::WaitFor(t) => {
            let until = Instant::now() + Duration::from_secs(10);
            let expr =
                format!("document.body ? document.body.innerText.includes({}) : false", serde_json::to_string(t).unwrap_or_default());
            while chrome.eval(s, &expr)? != Value::Bool(true) {
                if Instant::now() >= until {
                    let now = chrome.eval(s, "document.body ? document.body.innerText.slice(0, 300) : ''").unwrap_or_default();
                    let now = now.as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                    return Err(format!("not on the page after 10 s; it shows: {now}"));
                }
                chrome.pump(Duration::from_millis(100))?;
            }
            settle(chrome, s)
        }
        Step::Scroll(y) => {
            chrome.eval(s, &format!("window.scrollTo(0, {y}); true"))?;
            settle(chrome, s)
        }
        Step::Goto(r) => goto(chrome, s, &format!("{base}{r}")),
    }
}

/// One shot: a fresh browser context, the page, the steps, the picture
/// and the page as text. Masked; PNG as dry encodes it.
fn capture(chrome: &mut Chrome, spec: &Spec, sc: &Scenario, v: Viewport, base: &str) -> Result<(Vec<u8>, String)> {
    let ctx = chrome.call("Target.createBrowserContext", json!({"disposeOnDetach": true}), None)?;
    let ctx_id = ctx.get("browserContextId").and_then(Value::as_str).ok_or("no browser context")?.to_string();
    let result = (|| -> Result<(Vec<u8>, String)> {
        let t = chrome.call(
            "Target.createTarget",
            json!({"url": "about:blank", "browserContextId": ctx_id, "width": v.width, "height": v.height}),
            None,
        )?;
        let target = t.get("targetId").and_then(Value::as_str).ok_or("no target")?.to_string();
        let a = chrome.call("Target.attachToTarget", json!({"targetId": target, "flatten": true}), None)?;
        let s = a.get("sessionId").and_then(Value::as_str).ok_or("no session")?.to_string();
        let s = s.as_str();
        let c = &spec.capture;
        for m in ["Page.enable", "Network.enable"] {
            chrome.call(m, json!({}), Some(s))?;
        }
        chrome.call("Fetch.enable", json!({"patterns": [{"urlPattern": "*"}]}), Some(s))?;
        let mobile = v.width < 768;
        chrome.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": v.width, "height": v.height, "deviceScaleFactor": c.scale, "mobile": mobile, "screenWidth": v.width, "screenHeight": v.height}),
            Some(s),
        )?;
        let mut media = vec![json!({"name": "prefers-reduced-motion", "value": "reduce"})];
        if !c.scheme.is_empty() {
            media.push(json!({"name": "prefers-color-scheme", "value": c.scheme}));
        }
        chrome.call("Emulation.setEmulatedMedia", json!({"features": media}), Some(s))?;
        chrome.call("Emulation.setTimezoneOverride", json!({"timezoneId": c.timezone}), Some(s))?;
        chrome.call("Emulation.setLocaleOverride", json!({"locale": c.locale}), Some(s)).ok();
        chrome.call("Emulation.setScrollbarsHidden", json!({"hidden": true}), Some(s)).ok();
        chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source": init_script(spec)}), Some(s))?;
        goto(chrome, s, &format!("{base}{}", sc.route))?;
        for st in &sc.steps {
            step(chrome, s, st, base).map_err(|e| format!("{}: {e}", st.text()))?;
        }
        chrome.pump(Duration::from_millis(c.settle_ms))?;
        settle(chrome, s)?;
        chrome.eval(s, FREEZE)?;
        let selectors: Vec<String> = spec
            .masks
            .iter()
            .filter(|m| m.applies(&sc.name, v))
            .filter_map(|m| match &m.cover {
                Cover::Selector(css) => Some(css.clone()),
                Cover::Region(_) => None,
            })
            .collect();
        if !selectors.is_empty() {
            chrome.eval(s, &mask_script(&selectors))?;
        }
        let shot = chrome.call(
            "Page.captureScreenshot",
            json!({"format": "png", "fromSurface": true, "captureBeyondViewport": false}),
            Some(s),
        )?;
        let png = cdp::base64(shot.get("data").and_then(Value::as_str).ok_or("no screenshot data")?)?;
        let text = page_text(chrome, s)?;
        chrome.forget(s);
        // Region masks, then dry's own encoding (the same pixels, the same bytes).
        let mut img = image::decode(&png)?;
        for m in spec.masks.iter().filter(|m| m.applies(&sc.name, v)) {
            if let Cover::Region(r) = m.cover {
                let k = c.scale;
                let scaled = Rect::new(
                    (f64::from(r.x) * k) as u32,
                    (f64::from(r.y) * k) as u32,
                    (f64::from(r.w) * k).ceil() as u32,
                    (f64::from(r.h) * k).ceil() as u32,
                );
                img.fill(scaled, MASK);
            }
        }
        Ok((image::encode(&img), text))
    })();
    let _ = chrome.call("Target.disposeBrowserContext", json!({"browserContextId": ctx_id}), None);
    result
}

/// The page as text: its accessibility tree (roles, names, values), one
/// node a line, indented by depth; the masks' boxes left out.
pub(crate) fn page_text(chrome: &mut Chrome, s: &str) -> Result<String> {
    let r = chrome.call("Accessibility.getFullAXTree", json!({}), Some(s))?;
    let nodes = r.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    let by: std::collections::HashMap<String, &Value> =
        nodes.iter().filter_map(|n| Some((n.get("nodeId")?.as_str()?.to_string(), n))).collect();
    let skip = ["generic", "none", "InlineTextBox", "LineBreak", "RootWebArea", "presentation"];
    let mut lines: Vec<String> = Vec::new();
    fn val(n: &Value, k: &str) -> String {
        match n.pointer(&format!("/{k}/value")) {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }
    fn walk(n: &Value, depth: usize, parent: &str, by: &std::collections::HashMap<String, &Value>, skip: &[&str], lines: &mut Vec<String>) {
        if lines.len() > 2000 {
            return;
        }
        let role = val(n, "role");
        let name = val(n, "name");
        let value = val(n, "value");
        let ignored = n.get("ignored").and_then(Value::as_bool).unwrap_or(false);
        let mut next = depth;
        let show = !ignored && !skip.contains(&role.as_str()) && !(role == "StaticText" && (name.is_empty() || name == parent));
        if show {
            let r = if role == "StaticText" { "text" } else { role.as_str() };
            let mut line = format!("{}{r}", "  ".repeat(depth));
            if !name.is_empty() {
                line.push_str(&format!(" \"{name}\""));
            }
            if !value.is_empty() {
                line.push_str(&format!(" = \"{value}\""));
            }
            lines.push(line);
            next = depth + 1;
        }
        let pass = if name.is_empty() { parent.to_string() } else { name };
        for c in n.get("childIds").and_then(Value::as_array).into_iter().flatten() {
            if let Some(child) = c.as_str().and_then(|id| by.get(id)) {
                walk(child, next, &pass, by, skip, lines);
            }
        }
    }
    if let Some(root) = nodes.first() {
        walk(root, 0, "", &by, &skip, &mut lines);
    }
    let mut text = lines.join("\n");
    text.push('\n');
    Ok(text)
}

/// An evidence image an artifact page shows: `file` (a shot's PNG, no
/// folder) in `<root>/evidence/`, `root` relative to `repo` as artifacts'
/// roots are. Only `.png` names of shots; never outside the folder.
pub fn image(repo: &Path, root: &str, file: &str) -> Result<Vec<u8>> {
    let ok = file.ends_with(".png")
        && !file.starts_with('.')
        && file.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '@' | '.'));
    if !ok {
        return Err(format!("not an evidence image: {file}"));
    }
    let base = if Path::new(root).is_absolute() { PathBuf::from(root) } else { repo.join(root) };
    let dir = fs::canonicalize(base.join(dry_core::evidence::artifact::FOLDER)).map_err(|_| "no evidence folder".to_string())?;
    let full = fs::canonicalize(dir.join(file)).map_err(|_| format!("no {file}"))?;
    if !full.starts_with(&dir) {
        return Err(format!("{file} leads outside the evidence folder"));
    }
    fs::read(&full).map_err(|e| format!("{file}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_only_shots_inside_the_folder() {
        let dir = std::env::temp_dir().join(format!("dry-evidence-image-{}", std::process::id()));
        fs::create_dir_all(dir.join("evidence")).unwrap();
        fs::write(dir.join("evidence/home@1280x800.png"), b"png").unwrap();
        fs::write(dir.join("secret.png"), b"no").unwrap();
        assert_eq!(image(&dir, ".", "home@1280x800.png").unwrap(), b"png");
        assert!(image(&dir, ".", "../secret.png").is_err());
        assert!(image(&dir, ".", "manifest.json").is_err());
        assert!(image(&dir, ".", "none.png").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn options_read_and_mistakes_say_which() {
        let o = Options::parse(&["--project", "app", "--update", "--expect", "home, settings@390x844", "--repeat", "2"]).unwrap();
        assert_eq!(
            (o.project, o.update, o.expect, o.repeat),
            (PathBuf::from("app"), true, vec!["home".to_string(), "settings@390x844".to_string()], 2)
        );
        assert!(Options::parse(&["--fast"]).unwrap_err().contains("no option `--fast`"));
        assert!(Options::parse(&["--repeat", "0"]).is_err());
        assert!(Options::parse(&["--out"]).unwrap_err().contains("needs a value"));
    }

    #[test]
    fn a_free_port_is_never_the_owners() {
        let p = free_port().unwrap();
        assert!(![8080, 8090, 7777].contains(&p));
    }

    #[test]
    fn the_init_script_pins_the_clock_and_the_seed() {
        let spec = spec::parse("| scenario | route |\n|-|-|\n| a | / |\n\n## capture\ntime: 1795435200\nseed: 7\n").unwrap();
        let s = init_script(&spec);
        assert!(s.contains("const T = 1795435200000;"));
        assert!(s.contains("let s = 7 >>> 0;"));
        assert!(s.contains("animation-duration:0s"));
    }
}
