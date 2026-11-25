//! The web side end to end: the real `dry-server` binary on a free port,
//! serving a scratch root (`DRY_ROOT`) made of this repo's page, theme and
//! settings files and a sample store (`dry_core::files::sample`). Nothing
//! of the checkout's own state is read or written.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use dry_tools::store::{MdStore, StateStore as Store};

/// The server, stopped (and its scratch root removed) when dropped.
struct Server {
    child: Child,
    port: u16,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

/// A scratch root: the repo's routes, components, themes, scenes, fonts
/// and settings files, and the sample state in an `md:state` store.
fn scratch(name: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("dry-web-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["routes", "components", "themes", "scenes", "assets/fonts"] {
        copy_dir(&repo.join(dir), &root.join(dir));
    }
    for file in ["models.json", "sessions.json", "responses.md"] {
        std::fs::copy(repo.join(file), root.join(file)).unwrap();
    }
    // The sample state names no theme: the default one (the tests edit
    // `themes/default/theme.md` and look for it on the page).
    MdStore { root: root.join("state") }.save(&dry_core::files::sample()).unwrap();
    root
}

fn start(name: &str) -> Server {
    let root = scratch(name);
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let child = Command::new(env!("CARGO_BIN_EXE_dry-server"))
        .args(["--port", &port.to_string()])
        .env("DRY_ROOT", &root)
        .env_remove("DRY_STORE")
        .env_remove("DRY_SERVER_PORT")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server { child, port, root };
    let until = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < until, "the server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    server
}

/// One request; the status and the body.
fn request(s: &Server, method: &str, path: &str) -> (u16, String) {
    let (status, _, body) = exchange(s, method, path);
    (status, body)
}

/// The status, the head and the body of one request.
fn exchange(s: &Server, method: &str, path: &str) -> (u16, String, String) {
    let mut tcp = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(tcp, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    tcp.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, head.to_lowercase(), body.to_string())
}

fn get(s: &Server, path: &str) -> String {
    let (status, body) = request(s, "GET", path);
    assert_eq!(status, 200, "GET {path}: {body}");
    body
}

#[test]
fn every_route_is_served_with_its_key_content() {
    let s = start("routes");
    let home = get(&s, "/");
    for part in ["<title>Home · dry</title>", "class=\"glass tile\"", "Engines", "Need you", "id=\"backdrop\""] {
        assert!(home.contains(part), "/ lacks {part}");
    }
    // The navigation: fixed routes, then LIVE with one entry per engine.
    for part in ["href=\"/desk\"", "href=\"/analytics\"", "href=\"/settings\"", ">LIVE<", "href=\"/desk/0\"", "href=\"/desk/1\"", "New thread", "data-act=\"new-thread\""] {
        assert!(home.contains(part), "nav lacks {part}");
    }
    assert!(home.contains("data-mode=\"light\""), "the sample state is in light mode");

    let desk = get(&s, "/desk");
    assert!(desk.contains("Engine 1") && desk.contains("claude 2") && desk.contains("class=\"desk\""), "the tree names every engine");
    assert!(desk.contains("Engine 1&#39;s thread") && desk.contains("data-thread=\"engine-1\""), "the first root's thread, and its composer posts to it");
    assert!(desk.contains("composer pinned"), "the composer is pinned under the thread");
    assert!(!home.contains("New engine") && !desk.contains("Model engine"), "no New engine left");

    // The old addresses send you on to the desk, the rest of the path kept.
    for (old, new) in [("/engines", "/desk"), ("/engines/0", "/desk/0"), ("/engines/0/artifacts/plan?x=1", "/desk/0/artifacts/plan?x=1")] {
        let (status, head, _) = exchange(&s, "GET", old);
        assert_eq!(status, 302, "GET {old}: {head}");
        assert!(head.lines().any(|l| l.trim() == format!("location: {new}")), "GET {old} goes to {new}: {head}");
    }

    let engine = get(&s, "/desk/0");
    for part in ["id=\"engine-header\"", ">Session<", "composer pinned", ">Artifacts<", "/desk/0/artifacts/plan", ">Tools<", ">Prompt<", ">Response<", ">Metadata<", "class=\"columns\""] {
        assert!(engine.contains(part), "/desk/0 lacks {part}");
    }
    assert!(engine.contains("<a href=\"/desk\">Desk</a>"), "the trail links back up");
    assert!(engine.contains("Waiting") || engine.contains("waiting"), "engine 1 waits on its child: its activity shows");
    assert!(engine.contains("--tint:var(--activity_wait)"), "and tints its header");

    let artifact = get(&s, "/desk/0/artifacts/plan");
    assert!(artifact.contains("Snapshot · as sent") && artifact.contains("1. a"), "{artifact}");
    assert!(artifact.contains("<a href=\"/desk/0\">Engine 1</a>"), "the trail names the engine");

    let settings = get(&s, "/settings");
    for part in ["APPEARANCE", "THEME · themes/", "MODELS · models.json", "ROUTES · routes/", "NEW PAGE", "DIAGNOSTICS", "PROGRAMS · sessions.json"] {
        assert!(settings.contains(part), "/settings lacks {part}");
    }
    let tokens = get(&s, "/settings/ui");
    assert!(tokens.contains("background_body") && tokens.contains("class=\"swatch\""));

    let code = get(&s, "/code?file=models.json");
    for part in ["EXPLORER", "TERMINAL", "class=\"panes axis-column\"", "models.json", "class=\"bar status end\"", "Dark Modern"] {
        assert!(code.contains(part), "/code lacks {part}");
    }
    assert!(code.contains("data-frame=\"fill\""), "the Code page fills its box");
    assert!(code.contains("--radius:0px;"), "the route's own overrides reach its tokens");

    let (status, missing) = request(&s, "GET", "/nope");
    assert_eq!(status, 404);
    assert!(missing.contains("Nothing is registered for /nope."));

    let css = get(&s, "/assets/app.css");
    assert!(css.contains("@font-face{font-family:\"Amarante\"") && css.contains("--content-max") && css.contains(".glass{"));
    assert!(get(&s, "/assets/app.js").contains("EventSource"));
    let (status, body) = request(&s, "POST", "/api/engines/0/messages");
    assert_eq!(status, 415, "a write is JSON: {body}");
}

#[test]
fn an_unknown_component_shows_a_placeholder_naming_it() {
    let s = start("unknown");
    let dir = s.root.join("routes/odd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("page.json"), r#"{"title":"Odd","view":[{"type":"heading","text":"Still here"},{"type":"hologram","size":3}]}"#).unwrap();
    let odd = get(&s, "/odd");
    assert!(odd.contains("Still here"), "the components it knows still show");
    assert!(odd.contains("data-component=\"hologram\""), "{odd}");
    let settings = get(&s, "/settings");
    assert!(settings.contains("routes/odd/page.json"), "and the file is listed as not reading");
}

/// Read server-sent events until one named `name` arrives (or time runs out).
fn wait_for(lines: &mut impl BufRead, name: &str, within: Duration) -> Option<String> {
    let until = Instant::now() + within;
    let mut event = String::new();
    while Instant::now() < until {
        let mut line = String::new();
        match lines.read_line(&mut line) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => return None,
        }
        let line = line.trim_end();
        if let Some(e) = line.strip_prefix("event: ") {
            event = e.to_string();
        } else if let Some(data) = line.strip_prefix("data: ")
            && event == name
        {
            return Some(data.to_string());
        }
    }
    None
}

#[test]
fn the_event_stream_says_when_the_store_or_the_files_change() {
    let s = start("events");
    let mut tcp = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    write!(tcp, "GET /events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n").unwrap();
    let mut lines = BufReader::new(tcp);
    let hello = wait_for(&mut lines, "hello", Duration::from_secs(10)).expect("hello");
    assert!(hello.contains("\"gen\":0"), "{hello}");

    // The GPUI window saves: the store's files change.
    let mut state = dry_core::files::sample();
    state.engines[0].name = "Renamed".into();
    MdStore { root: s.root.join("state") }.save(&state).unwrap();
    let change = wait_for(&mut lines, "change", Duration::from_secs(10)).expect("a change after the store was saved");
    assert!(change.contains("\"store\""), "{change}");
    assert!(get(&s, "/").contains("Renamed"), "and the page drawn now shows it");

    // A theme file is edited.
    let theme = s.root.join("themes/default/theme.md");
    let text = std::fs::read_to_string(&theme).unwrap();
    std::fs::write(&theme, text.replace("| radius | 15 |", "| radius | 9 |")).unwrap();
    let change = wait_for(&mut lines, "change", Duration::from_secs(10)).expect("a change after a theme was edited");
    assert!(change.contains("\"files\""), "{change}");
    assert!(get(&s, "/").contains("--radius:9px;"));
}
