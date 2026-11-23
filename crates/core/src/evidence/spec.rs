//! `evidence.md`: what a project shows when it runs, as a table of
//! scenarios and a few settings sections, written like a template
//! (`crate::template`): a top table, then `## ` sections of `key: value`
//! lines.
//!
//! ```markdown
//! # Evidence: the web build
//!
//! | scenario | route    | viewports | steps                                    |
//! |----------|----------|-----------|------------------------------------------|
//! | landing  | /        |           |                                          |
//! | home     | /app/#/  | 1280x800  | click "Load demo"; wait-for "Trip planner" |
//!
//! ## build
//! command: trunk build --release --dist {out}/app index.html
//!
//! ## run
//! command: python3 -m http.server {port} --bind 127.0.0.1 --directory {out}
//! ready: http://127.0.0.1:{port}/app/
//! url: http://127.0.0.1:{port}
//!
//! ## viewports
//! 1280x800
//! 390x844
//! scale: 1
//!
//! ## capture
//! time: 2026-11-23T12:00:00Z
//! seed: 42
//!
//! ## masks
//! selector .clock
//! home@390x844: region 0,0,390,40
//!
//! ## threshold
//! pixel: 8
//! percent: 0
//! ```
//!
//! - **scenarios**: one row each, `scenario` a name (letters, digits, `-`,
//!   `_`), `route` the path after `url`, `viewports` empty for all of them
//!   (else some of the `## viewports`, comma separated), `steps` what to do
//!   before the shot, `;` between them (`Step`).
//! - **build**: `command:` run once before the app starts (optional).
//! - **run**: `command:` starts the app; `ready:` a URL that must answer
//!   200, or `log <text>` a line it prints; `url:` where the routes are
//!   (default: the ready URL's origin); `timeout:` seconds to wait (60).
//! - **viewports**: `WxH` lines, fixed sizes; `scale:` the device pixel
//!   ratio (1). Default: 1280x800.
//! - **capture**: `time:` the pinned clock (ISO, or unix seconds), `seed:`
//!   Math.random's seed, `timezone:` (UTC), `locale:` (en-US), `scheme:`
//!   light or dark (unset: the page's own), `settle:` ms to wait after the
//!   steps (300).
//! - **masks**: regions that change on purpose, painted flat before the
//!   shot: `selector <css>` or `region x,y,w,h`, for every shot or, after
//!   a `scenario:` or `scenario@WxH:` scope, for those.
//! - **threshold**: `pixel:` the per-channel tolerance (0-255, default 8),
//!   `percent:` how much may change before a shot counts as changed (0).
//!
//! Prose between the title and the table is for people and is skipped.
//! `{port}` (a free port), `{out}` (the build's scratch folder) and
//! `{project}` (the project folder) are filled in by the runner
//! (`expand`). Every error names its line. Pure.

use super::image::Rect;

/// A fixed viewport, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn name(self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    pub fn parse(s: &str) -> Option<Viewport> {
        let (w, h) = s.trim().split_once('x')?;
        let (width, height) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
        (width > 0 && height > 0 && width <= 8192 && height <= 8192).then_some(Viewport { width, height })
    }
}

/// One thing done on the page before its shot.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Click a CSS selector or the element showing this text.
    Click(String),
    /// Type into a field (a selector, its label, placeholder or name).
    Type(String, String),
    /// Press a key (`Enter`, `Tab`, `Escape`).
    Press(String),
    /// Wait this many milliseconds.
    Wait(u64),
    /// Wait until this text is on the page (10 s at most).
    WaitFor(String),
    /// Scroll the page to this y.
    Scroll(i64),
    /// Go to another route of the app.
    Goto(String),
}

impl Step {
    /// As written in the table.
    pub fn text(&self) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\\\""));
        match self {
            Step::Click(t) => format!("click {}", q(t)),
            Step::Type(t, x) => format!("type {} {}", q(t), q(x)),
            Step::Press(k) => format!("press {k}"),
            Step::Wait(ms) => format!("wait {ms}"),
            Step::WaitFor(t) => format!("wait-for {}", q(t)),
            Step::Scroll(y) => format!("scroll {y}"),
            Step::Goto(r) => format!("goto {}", q(r)),
        }
    }
}

/// The step words, for errors.
pub const STEPS: [&str; 7] =
    ["click \"…\"", "type \"…\" \"…\"", "press Enter", "wait 300", "wait-for \"…\"", "scroll 400", "goto \"/route\""];

/// One row of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    pub name: String,
    pub route: String,
    /// The viewports it is shot at (all of them when its cell is empty).
    pub viewports: Vec<Viewport>,
    pub steps: Vec<Step>,
}

/// How the app says it is up.
#[derive(Clone, Debug, PartialEq)]
pub enum Ready {
    /// This URL answers 200.
    Url(String),
    /// The app printed a line containing this.
    Log(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub command: String,
    pub ready: Ready,
    /// Where the routes are. Empty: the ready URL's origin.
    pub url: String,
    pub timeout: u64,
}

impl Run {
    /// The base URL routes are appended to.
    pub fn base(&self) -> String {
        if !self.url.is_empty() {
            return self.url.trim_end_matches('/').to_string();
        }
        match &self.ready {
            Ready::Url(u) => origin(u),
            Ready::Log(_) => String::new(),
        }
    }
}

/// `http://host:port` of a URL.
pub fn origin(url: &str) -> String {
    let after = url.find("://").map_or(0, |i| i + 3);
    match url[after..].find('/') {
        Some(i) => url[..after + i].to_string(),
        None => url.to_string(),
    }
}

/// What makes a capture the same every time.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    /// The pinned clock, milliseconds since 1970 (UTC).
    pub time_ms: i64,
    pub seed: u32,
    pub timezone: String,
    pub locale: String,
    /// `light` or `dark`; empty: the page's own.
    pub scheme: String,
    pub settle_ms: u64,
    /// The device pixel ratio.
    pub scale: f64,
}

impl Default for Capture {
    fn default() -> Capture {
        // 2026-01-01T00:00:00Z.
        Capture {
            time_ms: 1_767_225_600_000,
            seed: 1,
            timezone: "UTC".into(),
            locale: "en-US".into(),
            scheme: String::new(),
            settle_ms: 300,
            scale: 1.0,
        }
    }
}

/// What a mask covers.
#[derive(Clone, Debug, PartialEq)]
pub enum Cover {
    /// Every element this CSS selector matches.
    Selector(String),
    /// A fixed region, in CSS pixels.
    Region(Rect),
}

/// A region that changes on purpose, painted flat before the shot.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    /// Only this scenario (and only this viewport); none: every shot.
    pub scenario: Option<String>,
    pub viewport: Option<Viewport>,
    pub cover: Cover,
}

impl Mask {
    pub fn applies(&self, scenario: &str, v: Viewport) -> bool {
        self.scenario.as_ref().is_none_or(|s| s == scenario) && self.viewport.is_none_or(|w| w == v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Threshold {
    /// A pixel changed when a channel moved more than this (0-255).
    pub pixel: u8,
    /// A shot changed when more than this percent of its pixels did.
    pub percent: f64,
}

impl Default for Threshold {
    fn default() -> Threshold {
        Threshold { pixel: 8, percent: 0.0 }
    }
}

/// A read `evidence.md`.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub title: String,
    pub scenarios: Vec<Scenario>,
    pub build: Option<String>,
    pub run: Option<Run>,
    pub viewports: Vec<Viewport>,
    pub capture: Capture,
    pub masks: Vec<Mask>,
    pub threshold: Threshold,
}

impl Spec {
    /// Every shot, in order: each scenario at each of its viewports.
    pub fn shots(&self) -> Vec<(&Scenario, Viewport)> {
        self.scenarios.iter().flat_map(|s| s.viewports.iter().map(move |v| (s, *v))).collect()
    }
}

/// A shot's name, and the stem of its files: `home@1280x800`.
pub fn shot_name(scenario: &str, v: Viewport) -> String {
    format!("{scenario}@{}", v.name())
}

/// The sections `evidence.md` may have.
pub const SECTIONS: [&str; 6] = ["build", "run", "viewports", "capture", "masks", "threshold"];
/// The table's columns, in this order.
pub const COLUMNS: [&str; 4] = ["scenario", "route", "viewports", "steps"];

/// Fill in `{port}`, `{out}` and `{project}`.
pub fn expand(text: &str, vars: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Read `evidence.md`.
pub fn parse(md: &str) -> Result<Spec, String> {
    let err = |n: usize, why: String| format!("evidence.md line {n}: {why}");
    let mut title = String::new();
    let mut table: Vec<(usize, &str)> = Vec::new();
    // (line, heading, its lines with their numbers)
    type Lines<'a> = Vec<(usize, &'a str)>;
    let mut sections: Vec<(usize, String, Lines)> = Vec::new();
    for (i, line) in md.lines().enumerate() {
        let n = i + 1;
        if let Some(head) = line.strip_prefix("## ") {
            let head = head.trim().to_lowercase();
            if !SECTIONS.contains(&head.as_str()) {
                return Err(err(n, format!("`## {head}` is not a section; the sections are {}", SECTIONS.join(", "))));
            }
            if sections.iter().any(|(_, h, _)| *h == head) {
                return Err(err(n, format!("`## {head}` twice")));
            }
            sections.push((n, head, Vec::new()));
            continue;
        }
        match sections.last_mut() {
            Some((_, _, lines)) => {
                let t = line.trim();
                if !t.is_empty() && !t.starts_with("<!--") {
                    lines.push((n, t));
                }
            }
            None if line.trim_start().starts_with('|') => table.push((n, line)),
            None if line.starts_with("# ") => title = line[2..].trim().to_string(),
            // Prose before the sections says what the evidence is for.
            None => {}
        }
    }

    let mut spec = Spec {
        title,
        scenarios: Vec::new(),
        build: None,
        run: None,
        viewports: Vec::new(),
        capture: Capture::default(),
        masks: Vec::new(),
        threshold: Threshold::default(),
    };
    let kv = |n: usize, line: &str| -> Result<(String, String), String> {
        let (k, v) = line.split_once(':').ok_or_else(|| err(n, format!("`{line}` is not `key: value`")))?;
        Ok((k.trim().to_lowercase(), v.trim().to_string()))
    };
    for (at, head, lines) in &sections {
        match head.as_str() {
            "build" => {
                for (n, line) in lines {
                    match kv(*n, line)? {
                        (k, v) if k == "command" && !v.is_empty() => spec.build = Some(v),
                        (k, _) => return Err(err(*n, format!("## build takes `command:`, not `{k}:`"))),
                    }
                }
            }
            "run" => {
                let (mut command, mut ready, mut url, mut timeout) = (None, None, String::new(), 60);
                for (n, line) in lines {
                    let (k, v) = kv(*n, line)?;
                    match k.as_str() {
                        "command" => command = Some(v),
                        "ready" => {
                            ready = Some(match v.strip_prefix("log ") {
                                Some(t) if !t.trim().is_empty() => Ready::Log(t.trim().trim_matches('"').to_string()),
                                _ if v.starts_with("http://") || v.starts_with("https://") => Ready::Url(v),
                                _ => return Err(err(*n, format!("ready: is a URL (http://…) or `log <text>`, not `{v}`"))),
                            });
                        }
                        "url" => url = v,
                        "timeout" => timeout = v.parse().map_err(|_| err(*n, format!("timeout: is whole seconds, not `{v}`")))?,
                        _ => return Err(err(*n, format!("## run takes command:, ready:, url: and timeout:, not `{k}:`"))),
                    }
                }
                let command =
                    command.filter(|c| !c.is_empty()).ok_or_else(|| err(*at, "## run needs `command:` (what starts the app)".into()))?;
                let ready = ready.ok_or_else(|| err(*at, "## run needs `ready:` (a URL that answers 200, or `log <text>`)".into()))?;
                if url.is_empty() && matches!(ready, Ready::Log(_)) {
                    return Err(err(*at, "## run with `ready: log …` needs `url:` (where the routes are)".into()));
                }
                spec.run = Some(Run { command, ready, url, timeout });
            }
            "viewports" => {
                for (n, line) in lines {
                    if let Some(v) = Viewport::parse(line) {
                        if spec.viewports.contains(&v) {
                            return Err(err(*n, format!("viewport {} twice", v.name())));
                        }
                        spec.viewports.push(v);
                        continue;
                    }
                    let not = || err(*n, format!("`{line}` is not a viewport like 1280x800, or `scale: 1`"));
                    match kv(*n, line).map_err(|_| not())? {
                        (k, v) if k == "scale" => {
                            spec.capture.scale = v
                                .parse()
                                .ok()
                                .filter(|s: &f64| *s > 0.0 && *s <= 4.0)
                                .ok_or_else(|| err(*n, format!("scale: is a ratio like 1 or 2, not `{v}`")))?;
                        }
                        _ => return Err(not()),
                    }
                }
            }
            "capture" => {
                for (n, line) in lines {
                    let (k, v) = kv(*n, line)?;
                    let c = &mut spec.capture;
                    match k.as_str() {
                        "time" => {
                            c.time_ms = parse_time(&v)
                                .ok_or_else(|| err(*n, format!("time: is like 2026-11-23T12:00:00Z or unix seconds, not `{v}`")))?
                        }
                        "seed" => c.seed = v.parse().map_err(|_| err(*n, format!("seed: is a whole number, not `{v}`")))?,
                        "timezone" => c.timezone = v,
                        "locale" => c.locale = v,
                        "scheme" if v.is_empty() || v == "light" || v == "dark" => c.scheme = v,
                        "scheme" => return Err(err(*n, format!("scheme: is light or dark, not `{v}`"))),
                        "settle" => {
                            c.settle_ms = v
                                .trim_end_matches("ms")
                                .trim()
                                .parse()
                                .map_err(|_| err(*n, format!("settle: is milliseconds, not `{v}`")))?
                        }
                        _ => {
                            return Err(err(
                                *n,
                                format!("## capture takes time:, seed:, timezone:, locale:, scheme: and settle:, not `{k}:`"),
                            ));
                        }
                    }
                }
            }
            "masks" => {
                for (n, line) in lines {
                    let line = line.trim_start_matches("- ").trim();
                    let (scope, rest) = if line.starts_with("selector ") || line.starts_with("region ") {
                        (None, line)
                    } else {
                        match line.split_once(':') {
                            Some((s, r)) => (Some(s.trim()), r.trim()),
                            None => {
                                return Err(err(
                                    *n,
                                    format!("a mask is `selector <css>` or `region x,y,w,h`, maybe after `scenario:`; not `{line}`"),
                                ));
                            }
                        }
                    };
                    let (scenario, viewport) = match scope {
                        None | Some("*") => (None, None),
                        Some(s) => match s.split_once('@') {
                            Some((name, v)) => (
                                Some(name.to_string()),
                                Some(Viewport::parse(v).ok_or_else(|| err(*n, format!("`{v}` is not a viewport like 1280x800")))?),
                            ),
                            None => (Some(s.to_string()), None),
                        },
                    };
                    let cover = if let Some(css) = rest.strip_prefix("selector ") {
                        Cover::Selector(css.trim().to_string())
                    } else if let Some(r) = rest.strip_prefix("region ") {
                        let p: Vec<u32> = r.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                        match p.as_slice() {
                            [x, y, w, h] if *w > 0 && *h > 0 => Cover::Region(Rect::new(*x, *y, *w, *h)),
                            _ => return Err(err(*n, format!("region is x,y,w,h in pixels, not `{}`", r.trim()))),
                        }
                    } else {
                        return Err(err(*n, format!("a mask is `selector <css>` or `region x,y,w,h`, not `{rest}`")));
                    };
                    spec.masks.push(Mask { scenario, viewport, cover });
                }
            }
            "threshold" => {
                for (n, line) in lines {
                    let (k, v) = kv(*n, line)?;
                    match k.as_str() {
                        "pixel" => spec.threshold.pixel = v.parse().map_err(|_| err(*n, format!("pixel: is 0 to 255, not `{v}`")))?,
                        "percent" => {
                            spec.threshold.percent = v
                                .trim_end_matches('%')
                                .trim()
                                .parse()
                                .ok()
                                .filter(|p: &f64| (0.0..=100.0).contains(p))
                                .ok_or_else(|| err(*n, format!("percent: is 0 to 100, not `{v}`")))?;
                        }
                        _ => return Err(err(*n, format!("## threshold takes pixel: and percent:, not `{k}:`"))),
                    }
                }
            }
            _ => unreachable!("checked against SECTIONS"),
        }
    }
    if spec.viewports.is_empty() {
        spec.viewports.push(Viewport { width: 1280, height: 800 });
    }
    spec.scenarios = scenarios(&table, &spec.viewports)?;
    for m in &spec.masks {
        if let Some(s) = &m.scenario
            && !spec.scenarios.iter().any(|x| &x.name == s)
        {
            return Err(format!("evidence.md: a mask names scenario `{s}`, which is not in the table"));
        }
    }
    Ok(spec)
}

/// A table row's cells, without the outer `|`.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|c| c.trim().to_string()).collect()
}

fn scenarios(table: &[(usize, &str)], all: &[Viewport]) -> Result<Vec<Scenario>, String> {
    let err = |n: usize, why: String| format!("evidence.md line {n}: {why}");
    let Some(((hn, head), rows)) = table.split_first() else {
        return Err("evidence.md: no scenario table at the top (| scenario | route | viewports | steps |)".into());
    };
    let columns = cells(head);
    if columns.first().map(String::as_str) != Some("scenario") {
        return Err(err(*hn, "the table's first column is `scenario`".into()));
    }
    for c in &columns {
        if !COLUMNS.contains(&c.as_str()) {
            return Err(err(*hn, format!("the table has no column `{c}`; the columns are {}", COLUMNS.join(", "))));
        }
    }
    if !columns.iter().any(|c| c == "route") {
        return Err(err(*hn, "the table needs a `route` column".into()));
    }
    let mut out: Vec<Scenario> = Vec::new();
    for (n, line) in rows {
        let row = cells(line);
        if row.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
            continue;
        }
        if row.len() != columns.len() {
            return Err(err(*n, format!("this row has {} cells, the header {}", row.len(), columns.len())));
        }
        let cell = |name: &str| columns.iter().position(|c| c == name).map_or("", |i| row[i].as_str());
        let name = cell("scenario").to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(err(*n, format!("a scenario is named with letters, digits, - and _, not `{name}`")));
        }
        if out.iter().any(|s| s.name == name) {
            return Err(err(*n, format!("two scenarios named `{name}`")));
        }
        let route = cell("route").to_string();
        if !route.starts_with('/') {
            return Err(err(*n, format!("scenario `{name}`: a route starts with /, not `{route}`")));
        }
        let viewports = match cell("viewports") {
            "" | "all" | "*" => all.to_vec(),
            list => {
                let mut vs = Vec::new();
                for v in list.split(',').map(str::trim) {
                    let vp =
                        Viewport::parse(v).ok_or_else(|| err(*n, format!("scenario `{name}`: `{v}` is not a viewport like 1280x800")))?;
                    if !all.contains(&vp) {
                        return Err(err(
                            *n,
                            format!(
                                "scenario `{name}`: viewport {v} is not under ## viewports ({})",
                                all.iter().map(|v| v.name()).collect::<Vec<_>>().join(", ")
                            ),
                        ));
                    }
                    vs.push(vp);
                }
                vs
            }
        };
        let steps = parse_steps(cell("steps")).map_err(|why| err(*n, format!("scenario `{name}`: {why}")))?;
        out.push(Scenario { name, route, viewports, steps });
    }
    if out.is_empty() {
        return Err(err(*hn, "the table has no scenarios".into()));
    }
    Ok(out)
}

/// The words of a step: bare words and `"quoted strings"` (`\"` inside).
fn words(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut w = String::new();
        if c == '"' {
            chars.next();
            let mut closed = false;
            while let Some(c) = chars.next() {
                match c {
                    '\\' if chars.peek() == Some(&'"') => {
                        w.push('"');
                        chars.next();
                    }
                    '"' => {
                        closed = true;
                        break;
                    }
                    _ => w.push(c),
                }
            }
            if !closed {
                return Err(format!("a quote is not closed in `{s}`"));
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                w.push(c);
                chars.next();
            }
        }
        out.push(w);
    }
    Ok(out)
}

/// Read a steps cell: steps between `;` (a `;` inside quotes stays).
pub fn parse_steps(cell: &str) -> Result<Vec<Step>, String> {
    let mut parts = Vec::new();
    let (mut cur, mut quoted, mut prev) = (String::new(), false, ' ');
    for c in cell.chars() {
        if c == '"' && prev != '\\' {
            quoted = !quoted;
        }
        if c == ';' && !quoted {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        prev = c;
    }
    parts.push(cur);
    let mut out = Vec::new();
    for part in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let w = words(part)?;
        let num = |s: &str| s.trim_end_matches("ms").parse::<u64>().map_err(|_| format!("`{part}`: {s} is not milliseconds"));
        out.push(match w.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["click", t] => Step::Click(t.to_string()),
            ["type", t, x] => Step::Type(t.to_string(), x.to_string()),
            ["press", k] => Step::Press(k.to_string()),
            ["wait", ms] => Step::Wait(num(ms)?),
            ["wait-for", t] => Step::WaitFor(t.to_string()),
            ["scroll", y] => Step::Scroll(y.parse().map_err(|_| format!("`{part}`: scroll takes a y in pixels"))?),
            ["goto", r] if r.starts_with('/') => Step::Goto(r.to_string()),
            _ => return Err(format!("`{part}` is not a step; the steps are {}", STEPS.join(", "))),
        });
    }
    Ok(out)
}

/// `2026-11-23T12:00:00Z`, `2026-11-23 12:00`, `2026-11-23`, or unix
/// seconds: milliseconds since 1970, UTC.
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(secs) = s.parse::<i64>() {
        return Some(secs * 1000);
    }
    let s = s.trim_end_matches('Z');
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, t),
        None => (s, "00:00"),
    };
    let d: Vec<i64> = date.split('-').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let t: Vec<i64> = time.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let ([y, m, day], [hh, mm, rest @ ..]) = (d.as_slice(), t.as_slice()) else { return None };
    let (y, m, day, hh, mm) = (*y, *m, *day, *hh, *mm);
    let ss = rest.first().copied().unwrap_or(0);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..61).contains(&ss) {
        return None;
    }
    Some((days_from_civil(y, m, day) * 86_400 + hh * 3600 + mm * 60 + ss) * 1000)
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD: &str = "# Evidence: the web build\n\n| scenario | route | viewports | steps |\n|---|---|---|---|\n| landing | / | | |\n| home | /app/#/ | 390x844 | click \"Load demo\"; wait-for \"Trip; planner\"; wait 200 |\n\n## build\ncommand: trunk build --dist {out}/app\n\n## run\ncommand: python3 -m http.server {port}\nready: http://127.0.0.1:{port}/app/\n\n## viewports\n1280x800\n390x844\nscale: 1\n\n## capture\ntime: 2026-11-23T12:00:00Z\nseed: 42\nscheme: dark\n\n## masks\nselector .clock\nhome@390x844: region 0,0,390,40\n\n## threshold\npixel: 4\npercent: 0.5%\n";

    #[test]
    fn the_table_is_the_scenarios_and_the_sections_the_settings() {
        let s = parse(MD).unwrap();
        assert_eq!(s.title, "Evidence: the web build");
        assert_eq!(s.scenarios.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["landing", "home"]);
        let v = |w, h| Viewport { width: w, height: h };
        assert_eq!(s.scenarios[0].viewports, vec![v(1280, 800), v(390, 844)], "an empty cell is every viewport");
        assert_eq!(s.scenarios[1].viewports, vec![v(390, 844)]);
        assert_eq!(s.scenarios[1].steps, vec![Step::Click("Load demo".into()), Step::WaitFor("Trip; planner".into()), Step::Wait(200)]);
        assert_eq!(s.shots().len(), 3);
        assert_eq!(s.build.as_deref(), Some("trunk build --dist {out}/app"));
        let run = s.run.clone().unwrap();
        assert_eq!(run.ready, Ready::Url("http://127.0.0.1:{port}/app/".into()));
        assert_eq!(expand(&run.base(), &[("port", "4123")]), "http://127.0.0.1:4123", "the routes are under the ready URL's origin");
        assert_eq!((s.capture.time_ms, s.capture.seed, s.capture.scheme.as_str()), (1_795_435_200_000, 42, "dark"));
        assert_eq!(s.masks.len(), 2);
        assert!(s.masks[0].applies("landing", v(1280, 800)));
        assert!(
            s.masks[1].applies("home", v(390, 844))
                && !s.masks[1].applies("home", v(1280, 800))
                && !s.masks[1].applies("landing", v(390, 844))
        );
        assert_eq!(s.masks[1].cover, Cover::Region(Rect::new(0, 0, 390, 40)));
        assert_eq!((s.threshold.pixel, s.threshold.percent), (4, 0.5));
        assert_eq!(shot_name("home", v(390, 844)), "home@390x844");
    }

    #[test]
    fn a_bare_table_takes_the_defaults() {
        let s = parse("| scenario | route |\n|-|-|\n| a | / |\n").unwrap();
        assert_eq!(s.viewports, vec![Viewport { width: 1280, height: 800 }]);
        assert_eq!((s.threshold, s.run, s.build), (Threshold::default(), None, None));
        assert_eq!(s.capture, Capture::default());
    }

    #[test]
    fn every_mistake_names_its_line() {
        let e = |md: &str| parse(md).unwrap_err();
        let t = "| scenario | route |\n|-|-|\n| a | / |\n";
        assert_eq!(e("| name | route |\n|-|-|\n| a | / |\n"), "evidence.md line 1: the table's first column is `scenario`");
        assert!(e("| scenario | path |\n").contains("no column `path`"));
        assert!(e("| scenario | route |\n|-|-|\n| a | x |\n").contains("line 3: scenario `a`: a route starts with /"));
        assert!(e("| scenario | route |\n|-|-|\n| a | / |\n| a | / |\n").contains("line 4: two scenarios named `a`"));
        assert!(e("| scenario | route |\n|-|-|\n| a b | / |\n").contains("letters, digits"));
        assert!(e("| scenario | route | steps |\n|-|-|-|\n| a | / | jump |\n").contains("line 3: scenario `a`: `jump` is not a step"));
        assert!(e("| scenario | route | steps |\n|-|-|-|\n| a | / | click \"x |\n").contains("not closed"));
        assert!(e("| scenario | route | viewports |\n|-|-|-|\n| a | / | 10x10 |\n").contains("not under ## viewports"));
        assert!(e(&format!("{t}\n## frames\n")).contains("line 5: `## frames` is not a section"));
        assert!(e(&format!("{t}\n## run\ncommand: x\n")).contains("line 5: ## run needs `ready:`"));
        assert!(e(&format!("{t}\n## run\ncommand: x\nready: soon\n")).contains("line 7: ready: is a URL"));
        assert!(e(&format!("{t}\n## run\ncommand: x\nready: log up\n")).contains("needs `url:`"));
        assert!(e(&format!("{t}\n## viewports\nwide\n")).contains("line 6: `wide` is not a viewport"));
        assert!(e(&format!("{t}\n## capture\ntime: tomorrow\n")).contains("line 6: time:"));
        assert!(e(&format!("{t}\n## masks\nregion 1,2,3\n")).contains("line 6: region is x,y,w,h"));
        assert!(e(&format!("{t}\n## masks\nb: selector .x\n")).contains("names scenario `b`"));
        assert!(e(&format!("{t}\n## threshold\npercent: 300\n")).contains("line 6: percent: is 0 to 100"));
        assert!(e(&format!("{t}\n## build\nrun: x\n")).contains("## build takes `command:`"));
        assert!(e(&format!("{t}\n## build\n## build\n")).contains("twice"));
        assert!(e("# Title\n").contains("no scenario table"));
    }

    #[test]
    fn steps_read_back_as_written() {
        let cell = "click \"Load demo\"; type \"#q\" \"say \\\"hi\\\"\"; press Enter; wait 300; wait-for \"Done\"; scroll 400; goto \"/app/#/settings\"";
        let steps = parse_steps(cell).unwrap();
        assert_eq!(steps.len(), 7);
        assert_eq!(steps[1], Step::Type("#q".into(), "say \"hi\"".into()));
        let again = steps.iter().map(Step::text).collect::<Vec<_>>().join("; ");
        assert_eq!(parse_steps(&again).unwrap(), steps);
        assert!(parse_steps("").unwrap().is_empty());
        assert!(parse_steps("wait soon").unwrap_err().contains("not milliseconds"));
    }

    #[test]
    fn times_read_in_utc() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-23"), Some(1_795_392_000_000));
        assert_eq!(parse_time("2026-11-23 12:00"), Some(1_795_435_200_000));
        assert_eq!(parse_time("1795435200"), Some(1_795_435_200_000));
        assert_eq!(parse_time("2026-13-01"), None);
        assert_eq!(origin("http://127.0.0.1:4000/app/"), "http://127.0.0.1:4000");
    }
}
