//! `kind: browser`: what an app the engine started shows, as evidence for
//! its test phase (docs/developer-loop.md, "Running the app and showing it to the tester").
//!
//! ```text
//! artifacts/page.md
//! kind: browser
//! url: http://127.0.0.1:5173/
//! shot: /tmp/dry-companion-browser/page-3.png
//! ```
//!
//! - `browse(url)` opens a page (only this machine's: localhost, 127.0.0.1,
//!   unless the shell was told more), `click(target)` clicks the element a
//!   CSS selector or its visible text names, `type(target, text)` types
//!   into one. Each result is one line; each takes a screenshot.
//! - The snapshot, read again at every render: the page as text (its
//!   accessibility tree: roles, names, values), its console errors, and the
//!   path of the last screenshot (a file, for when vision lands).
//!
//! Pure. The shell drives the browser (`live::Host::browser`: the companion
//! runs the machine's Chrome over the DevTools protocol).

use serde_json::Value;

use crate::calls::Call;
use crate::live::Host;

/// The tools a browser artifact brings.
pub const TOOLS: [&str; 3] = ["browse", "click", "type"];
/// The most page text shown in the snapshot.
pub const TEXT_CHARS: usize = 6_000;

/// A browser artifact's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub url: Option<String>,
    pub shot: Option<String>,
}

impl State {
    pub fn parse(text: &str) -> Result<State, String> {
        let mut s = State::default();
        for line in text.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()) {
            match line.split_once(':').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("url", v)) => s.url = Some(v.to_string()).filter(|v| !v.is_empty()),
                Some(("shot", v)) => s.shot = Some(v.to_string()).filter(|v| !v.is_empty()),
                _ => return Err(format!("a browser artifact line is url: or shot:, not `{line}`")),
            }
        }
        Ok(s)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("kind: browser\n");
        if let Some(u) = &self.url {
            out.push_str(&format!("url: {u}\n"));
        }
        if let Some(s) = &self.shot {
            out.push_str(&format!("shot: {s}\n"));
        }
        out
    }
}

/// What the shell's browser said about a page (`Host::browser`'s JSON).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub text: String,
    pub errors: Vec<String>,
    pub shot: Option<String>,
    /// What an action did (`clicked button "Save"`).
    pub did: Option<String>,
}

impl Page {
    pub fn from_json(text: &str) -> Result<Page, String> {
        let v: Value = serde_json::from_str(text).map_err(|_| "the browser's answer does not read".to_string())?;
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            return Err(e.to_string());
        }
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        Ok(Page {
            url: s("url"),
            title: s("title"),
            text: s("text"),
            errors: v
                .get("errors")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
            shot: v.get("screenshot").and_then(Value::as_str).map(str::to_string),
            did: v.get("did").and_then(Value::as_str).map(str::to_string),
        })
    }
}

/// The page key a shell keeps per artifact, so two artifacts are two tabs.
pub fn key(engine: &str, artifact: &str) -> String {
    format!("{engine}/{artifact}")
}

/// Run one of its tools. The result is one line; the page goes in the snapshot.
pub fn call(engine: &str, artifact: &str, text: &mut String, call: &Call, host: &dyn Host) -> Result<String, String> {
    let mut s = State::parse(text).map_err(|why| format!("the browser artifact does not read: {why}"))?;
    let key = key(engine, artifact);
    let need = |k: &str| call.arg(k).map(str::to_string).ok_or_else(|| format!("`{}` needs {k}=\"...\"", call.name));
    let (op, args): (&str, Vec<(String, String)>) = match call.name.as_str() {
        "browse" => ("open", vec![("url".into(), need("url")?)]),
        "click" => ("click", vec![("target".into(), need("target")?)]),
        "type" => {
            let mut a = vec![("target".into(), need("target")?), ("text".into(), need("text")?)];
            if let Some(k) = call.arg("key") {
                a.push(("press".into(), k.to_string()));
            }
            ("type", a)
        }
        other => return Err(format!("a browser artifact has no tool `{other}`")),
    };
    if op != "open" && s.url.is_none() {
        return Err("no page is open: browse(url=\"http://127.0.0.1:…/\") first".into());
    }
    let mut args = args;
    args.push(("page".into(), key));
    let page = Page::from_json(&host.browser(op, &args)?)?;
    s.url = Some(page.url.clone()).filter(|u| !u.is_empty()).or(s.url);
    if page.shot.is_some() {
        s.shot.clone_from(&page.shot);
    }
    *text = s.to_text();
    let errors = match page.errors.len() {
        0 => String::new(),
        n => format!(" · {n} console error{}", if n == 1 { "" } else { "s" }),
    };
    Ok(match page.did {
        Some(d) => format!("{d}; now {} \"{}\"{errors}", page.url, page.title),
        None => format!("opened {} \"{}\"{errors}", page.url, page.title),
    })
}

/// The page as it is now: its text, its errors, the last screenshot.
pub fn snapshot(engine: &str, artifact: &str, text: &str, host: &dyn Host) -> String {
    let s = match State::parse(text) {
        Ok(s) => s,
        Err(why) => return format!("(does not read: {why})"),
    };
    let Some(url) = &s.url else { return "no page open: browse(url=\"...\") opens one (this machine's addresses)".into() };
    let look = host.browser("look", &[("page".into(), key(engine, artifact)), ("url".into(), url.clone())]).and_then(|t| Page::from_json(&t));
    let page = match look {
        Ok(p) => p,
        Err(why) => return format!("url: {url}\n(the page could not be read now: {why})"),
    };
    let mut body = page.text.clone();
    if body.chars().count() > TEXT_CHARS {
        body = format!("{}\n… (cut at {TEXT_CHARS} characters)", body.chars().take(TEXT_CHARS).collect::<String>());
    }
    let errors = if page.errors.is_empty() { "none".to_string() } else { page.errors.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n") };
    let shot = page.shot.or(s.shot).unwrap_or_else(|| "none yet".into());
    format!("url: {} — \"{}\"\n#### page\n{body}\n#### console errors\n{errors}\n#### screenshot\n{shot}", page.url, page.title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{Content, Job, Proc};
    use std::cell::RefCell;

    /// A browser that answers from a script and keeps what it was asked.
    /// An op and its arguments.
    type Asked = (String, Vec<(String, String)>);

    #[derive(Default)]
    struct Fake {
        asked: RefCell<Vec<Asked>>,
    }

    impl Host for Fake {
        fn now(&self) -> i64 {
            0
        }
        fn list(&self, _: &str, _: usize) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }
        fn read(&self, _: &str, _: &str) -> Content {
            Content::Missing(String::new())
        }
        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, _: &Job) -> Result<Option<i32>, String> {
            Ok(None)
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
        fn browser(&self, op: &str, args: &[(String, String)]) -> Result<String, String> {
            self.asked.borrow_mut().push((op.to_string(), args.to_vec()));
            let url = args.iter().find(|(k, _)| k == "url").map_or("http://127.0.0.1:5173/", |(_, v)| v.as_str());
            if url.contains("example.com") {
                return Err("only this machine's pages (localhost, 127.0.0.1)".into());
            }
            let did = if op == "click" { ",\"did\":\"clicked button \\\"Add\\\"\"" } else { "" };
            Ok(format!(
                "{{\"url\":\"{url}\",\"title\":\"Todo\",\"text\":\"heading \\\"Todo\\\"\\nbutton \\\"Add\\\"\",\"errors\":[\"TypeError: x is undefined\"],\"screenshot\":\"/tmp/s-{op}.png\"{did}}}"
            ))
        }
    }

    fn call_of(text: &str) -> Call {
        crate::calls::parse(text).unwrap().remove(0)
    }

    #[test]
    fn its_tools_change_only_the_url_and_the_shot() {
        let host = Fake::default();
        let mut text = "kind: browser\n".to_string();
        assert!(call("e1", "page", &mut text, &call_of("click(target=\"Add\")"), &host).unwrap_err().contains("browse"), "nothing open yet");
        let r = call("e1", "page", &mut text, &call_of("browse(url=\"http://127.0.0.1:5173/\")"), &host).unwrap();
        assert_eq!(r, "opened http://127.0.0.1:5173/ \"Todo\" · 1 console error");
        assert_eq!(text, "kind: browser\nurl: http://127.0.0.1:5173/\nshot: /tmp/s-open.png\n");
        let r = call("e1", "page", &mut text, &call_of("click(target=\"Add\")"), &host).unwrap();
        assert!(r.starts_with("clicked button \"Add\"; now http://127.0.0.1:5173/"), "{r}");
        assert!(text.contains("shot: /tmp/s-click.png"));
        let asked = host.asked.borrow();
        assert_eq!(asked[1].1, vec![("target".to_string(), "Add".to_string()), ("page".to_string(), "e1/page".to_string())]);
        drop(asked);
        assert!(call("e1", "page", &mut text, &call_of("browse(url=\"https://example.com/\")"), &host).unwrap_err().contains("this machine"));
        assert!(call("e1", "page", &mut text, &call_of("type(target=\"#q\")"), &host).unwrap_err().contains("text="));
        assert_eq!(State::parse(&text).unwrap().to_text(), text, "reads back");
        assert!(State::parse("kind: browser\nnonsense").is_err());
    }

    #[test]
    fn the_snapshot_is_the_page_its_errors_and_the_screenshot() {
        let host = Fake::default();
        assert!(snapshot("e1", "page", "kind: browser\n", &host).starts_with("no page open"));
        let snap = snapshot("e1", "page", "kind: browser\nurl: http://127.0.0.1:5173/\n", &host);
        assert!(snap.contains("#### page\nheading \"Todo\"\nbutton \"Add\""), "{snap}");
        assert!(snap.contains("#### console errors\n- TypeError: x is undefined"));
        assert!(snap.ends_with("#### screenshot\n/tmp/s-look.png"));
        // A shell with no browser says so, in the snapshot and the results.
        let none = crate::turn::NoHost;
        assert!(snapshot("e1", "page", "kind: browser\nurl: http://127.0.0.1:1/\n", &none).contains("could not be read now"));
    }
}
