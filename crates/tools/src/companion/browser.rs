//! The browser capability: this machine's Chrome, headless, for its own
//! pages (localhost): open, look, click, type, screenshot. Driven with
//! dry's own DevTools client (`crate::cdp`) and the evidence runner's
//! page steps (`crate::evidence`: load and settle, find by selector or
//! text, click, the page as accessibility-tree text). Only this machine's
//! pages: after every step the address is checked again.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use super::rules::{Fail, fail};
use super::state::Companion;
use crate::cdp::Chrome;

/// One tab, by the key a caller names it (`<engine>/<artifact>`).
struct Page {
    session: String,
    target: String,
    shots: u32,
    last_shot: Option<String>,
}

pub struct Browser {
    chrome: Chrome,
    pages: BTreeMap<String, Page>,
}

/// The tools, for the manifest: name, doc, properties, required.
pub fn tools() -> Vec<(&'static str, &'static str, Value, &'static [&'static str])> {
    let s = json!({ "type": "string" });
    vec![
        (
            "browser_open",
            "Open a page of this machine (localhost) in a headless browser: its text, console errors and a screenshot.",
            json!({ "url": s }),
            &["url"],
        ),
        ("browser_look", "The open page as text (its accessibility tree) and its console errors.", json!({}), &[]),
        ("browser_click", "Click an element of the open page: a CSS selector or the text it shows.", json!({ "target": s }), &["target"]),
        (
            "browser_type",
            "Type into a field of the open page; press: a key after (Enter).",
            json!({ "target": s, "text": s, "press": s }),
            &["target", "text"],
        ),
        ("browser_screenshot", "A screenshot of the open page (a PNG file, and a small JPEG).", json!({}), &[]),
    ]
}

/// Only this machine's pages.
pub fn url_allowed(u: &str) -> bool {
    if u == "about:blank" {
        return true;
    }
    let Some(rest) = u.strip_prefix("http://").or_else(|| u.strip_prefix("https://")) else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = if host.starts_with('[') {
        host.split(']').next().map(|h| format!("{h}]")).unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    let h = host.to_ascii_lowercase();
    h == "localhost" || h == "127.0.0.1" || h == "[::1]" || h.ends_with(".localhost")
}

fn err(e: String) -> Fail {
    fail(502, "upstream", e)
}

fn shot_dir() -> PathBuf {
    std::env::temp_dir().join("dry-companion-shots")
}

impl Browser {
    fn start(c: &Companion) -> Result<Browser, Fail> {
        let bin = super::caps::chrome(c)
            .ok_or_else(|| fail(503, "unavailable", "no Chrome, Chromium, Edge or Brave found (give one with --chrome PATH)"))?;
        let profiles = std::env::temp_dir().join("dry-companion-chrome");
        let chrome = Chrome::launch(&bin, &profiles).map_err(|e| fail(503, "unavailable", e))?;
        c.log(&format!("browser started: {} (headless)", bin.file_name().unwrap_or_default().to_string_lossy()));
        Ok(Browser { chrome, pages: BTreeMap::new() })
    }

    fn page(&mut self, key: &str) -> Result<String, Fail> {
        if let Some(p) = self.pages.get(key) {
            return Ok(p.session.clone());
        }
        let t = self.chrome.call("Target.createTarget", json!({ "url": "about:blank" }), None).map_err(err)?;
        let target = t["targetId"].as_str().unwrap_or("").to_string();
        let a = self.chrome.call("Target.attachToTarget", json!({ "targetId": target, "flatten": true }), None).map_err(err)?;
        let session = a["sessionId"].as_str().unwrap_or("").to_string();
        for m in ["Page.enable", "Network.enable", "Runtime.enable", "Log.enable"] {
            self.chrome.call(m, json!({}), Some(&session)).map_err(err)?;
        }
        self.chrome
            .call(
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": 1280, "height": 800, "deviceScaleFactor": 1, "mobile": false }),
                Some(&session),
            )
            .map_err(err)?;
        self.pages.insert(key.into(), Page { session: session.clone(), target, shots: 0, last_shot: None });
        Ok(session)
    }

    /// The address now; a page that went elsewhere is sent to about:blank.
    fn guard(&mut self, s: &str) -> Result<String, Fail> {
        let href = self.chrome.eval(s, "location.href").map_err(err)?.as_str().unwrap_or("").to_string();
        if !url_allowed(&href) {
            let _ = self.chrome.call("Page.navigate", json!({ "url": "about:blank" }), Some(s));
            return Err(fail(403, "url_refused", format!("the page went to {href}, which is not this machine's")));
        }
        Ok(href)
    }

    fn shoot(&mut self, key: &str, s: &str) -> Result<(String, String), Fail> {
        let png = self.chrome.call("Page.captureScreenshot", json!({ "format": "png" }), Some(s)).map_err(err)?;
        let bytes = crate::cdp::base64(png["data"].as_str().unwrap_or("")).map_err(err)?;
        let dir = shot_dir();
        let _ = std::fs::create_dir_all(&dir);
        let page = self.pages.get_mut(key).expect("a page");
        page.shots += 1;
        let safe: String = key.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' }).collect();
        let file = dir.join(format!("{safe}-{}.png", page.shots));
        std::fs::write(&file, bytes).map_err(|e| fail(500, "internal", e.to_string()))?;
        page.last_shot = Some(file.display().to_string());
        let clip = json!({ "format": "jpeg", "quality": 60, "clip": { "x": 0, "y": 0, "width": 1280, "height": 800, "scale": 0.4 } });
        let jpeg = self.chrome.call("Page.captureScreenshot", clip, Some(s)).map_err(err)?;
        Ok((file.display().to_string(), jpeg["data"].as_str().unwrap_or("").to_string()))
    }

    /// Console errors and exceptions since the last look.
    fn errors(&mut self, s: &str) -> Vec<String> {
        let _ = self.chrome.pump(Duration::from_millis(50));
        let mut out = Vec::new();
        for e in self.chrome.take(s, &["Runtime.exceptionThrown", "Runtime.consoleAPICalled", "Log.entryAdded"]) {
            let p = &e["params"];
            let line = match e["method"].as_str() {
                Some("Runtime.exceptionThrown") => p
                    .pointer("/exceptionDetails/exception/description")
                    .or(p.pointer("/exceptionDetails/text"))
                    .and_then(Value::as_str)
                    .map(String::from),
                Some("Runtime.consoleAPICalled") if matches!(p["type"].as_str(), Some("error" | "assert")) => Some(
                    p["args"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|a| {
                            a["value"].as_str().map(String::from).unwrap_or_else(|| a["description"].as_str().unwrap_or("").to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                Some("Log.entryAdded") if p.pointer("/entry/level").and_then(Value::as_str) == Some("error") => {
                    p.pointer("/entry/text").and_then(Value::as_str).map(String::from)
                }
                _ => None,
            };
            if let Some(l) = line {
                out.push(l.chars().take(500).collect());
            }
        }
        out
    }

    fn describe(&mut self, key: &str, s: &str, shot: bool, did: Option<String>) -> Result<Value, Fail> {
        let href = self.guard(s)?;
        let title = self.chrome.eval(s, "document.title").map_err(err)?;
        let text = crate::evidence::page_text(&mut self.chrome, s).map_err(err)?;
        let errors = self.errors(s);
        let mut v = json!({ "url": href, "title": title, "text": text.chars().take(20_000).collect::<String>(), "errors": errors,
            "screenshot": self.pages.get(key).and_then(|p| p.last_shot.clone()) });
        if shot {
            let (file, jpeg) = self.shoot(key, s)?;
            v["screenshot"] = json!(file);
            v["jpeg"] = json!(jpeg);
        }
        if let Some(d) = did {
            v["did"] = json!(d);
        }
        Ok(v)
    }

    fn op(&mut self, op: &str, b: &Value) -> Result<Value, Fail> {
        let key = b["page"].as_str().unwrap_or("default").to_string();
        if op == "close" {
            if let Some(p) = self.pages.remove(&key) {
                let _ = self.chrome.call("Target.closeTarget", json!({ "targetId": p.target }), None);
                self.chrome.forget(&p.session);
            }
            return Ok(json!({ "ok": true }));
        }
        let s = self.page(&key)?;
        let target = b["target"].as_str().unwrap_or("");
        match op {
            "open" => {
                let url = b["url"].as_str().unwrap_or("");
                if !url_allowed(url) {
                    return Err(fail(403, "url_refused", format!("{url} is not this machine's (localhost, 127.0.0.1)")));
                }
                let _ = self.errors(&s);
                crate::evidence::goto(&mut self.chrome, &s, url).map_err(err)?;
                self.describe(&key, &s, true, None)
            }
            "look" => self.describe(&key, &s, false, None),
            "screenshot" => {
                let href = self.guard(&s)?;
                let (file, jpeg) = self.shoot(&key, &s)?;
                Ok(json!({ "url": href, "screenshot": file, "jpeg": jpeg }))
            }
            "click" => {
                let (x, y) = crate::evidence::locate(&mut self.chrome, &s, target, false).map_err(|e| fail(404, "not_found", e))?;
                crate::evidence::click_at(&mut self.chrome, &s, x, y).map_err(err)?;
                self.settle(&s);
                self.describe(&key, &s, true, Some(format!("clicked {target}")))
            }
            "type" => {
                let (x, y) = crate::evidence::locate(&mut self.chrome, &s, target, true).map_err(|e| fail(404, "not_found", e))?;
                crate::evidence::click_at(&mut self.chrome, &s, x, y).map_err(err)?;
                self.chrome.call("Input.insertText", json!({ "text": b["text"].as_str().unwrap_or("") }), Some(&s)).map_err(err)?;
                let press = b["press"].as_str().unwrap_or("");
                if !press.is_empty() {
                    let code = match press {
                        "Enter" => 13,
                        "Tab" => 9,
                        "Escape" => 27,
                        _ => 0,
                    };
                    let mut down = json!({ "type": "keyDown", "key": press, "code": press, "windowsVirtualKeyCode": code });
                    if press == "Enter" {
                        down["text"] = json!("\r");
                    }
                    self.chrome.call("Input.dispatchKeyEvent", down, Some(&s)).map_err(err)?;
                    self.chrome
                        .call(
                            "Input.dispatchKeyEvent",
                            json!({ "type": "keyUp", "key": press, "code": press, "windowsVirtualKeyCode": code }),
                            Some(&s),
                        )
                        .map_err(err)?;
                }
                self.settle(&s);
                let did = format!("typed into {target}{}", if press.is_empty() { String::new() } else { format!(" and pressed {press}") });
                self.describe(&key, &s, true, Some(did))
            }
            _ => Err(fail(404, "not_found", format!("no browser operation {op}"))),
        }
    }

    fn settle(&mut self, s: &str) {
        let _ = self.chrome.pump(Duration::from_millis(400));
        let _ = self.chrome.network_idle(s, Duration::from_millis(300), Duration::from_secs(5));
    }
}

/// `POST /v1/browser/<op>`: the browser started on first use.
pub fn op(c: &Companion, op: &str, b: &Value) -> Result<Value, Fail> {
    c.need("browser")?;
    let mut slot = c.browser.lock().unwrap();
    if slot.is_none() {
        *slot = Some(Browser::start(c)?);
    }
    let r = slot.as_mut().expect("started").op(op, b);
    if r.as_ref().is_err_and(|f| f.status == 502) {
        // Chrome may have gone: the next call starts it again.
        *slot = None;
    }
    r
}

/// An MCP `browser_*` call: the page as text (and the small JPEG as an image).
pub fn mcp(c: &Companion, name: &str, args: &Value) -> Result<Value, Fail> {
    let mut b = if args.is_object() { args.clone() } else { json!({}) };
    b["page"] = json!("mcp");
    let r = op(c, name.trim_start_matches("browser_"), &b)?;
    let text = if r.get("text").is_some() {
        let errors: Vec<&str> = r["errors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        format!(
            "{}{} {}\n{}\nconsole errors: {}\nscreenshot: {}",
            r["did"].as_str().map(|d| format!("{d}\n")).unwrap_or_default(),
            r["url"].as_str().unwrap_or(""),
            r["title"],
            r["text"].as_str().unwrap_or(""),
            if errors.is_empty() { "none".into() } else { errors.join("\n") },
            r["screenshot"].as_str().unwrap_or("none")
        )
    } else {
        format!("screenshot: {}", r["screenshot"].as_str().unwrap_or(""))
    };
    let mut content = vec![json!({ "type": "text", "text": text })];
    if let Some(j) = r["jpeg"].as_str().filter(|j| !j.is_empty()) {
        content.push(json!({ "type": "image", "data": j, "mimeType": "image/jpeg" }));
    }
    Ok(json!({ "content": content, "isError": false }))
}

/// Close it (stop-all, and when the companion stops): whether it was open.
pub fn close(c: &Companion) -> bool {
    let had = c.browser.lock().unwrap().take();
    let open = had.is_some();
    drop(had);
    if open {
        c.log("browser closed");
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_machines_pages() {
        for u in ["http://localhost:3000/x", "https://127.0.0.1", "http://[::1]:8/", "http://app.localhost/", "about:blank"] {
            assert!(url_allowed(u), "{u}");
        }
        for u in [
            "https://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://localhost.evil.example/",
            "http://127.0.0.1.nip.io",
        ] {
            assert!(!url_allowed(u), "{u}");
        }
    }
}
