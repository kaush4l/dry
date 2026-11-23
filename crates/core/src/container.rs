//! Containers: a Linux the engine can build and run things in, and look at
//! (docs/container.md). Pure: the trait a shell implements, and the two
//! small protocols every backend that talks to a shell over a console
//! shares (the exec markers, HTTP/1.0 to a port).
//!
//! Backends (one built so far):
//!
//! | backend | where | how |
//! |---|---|---|
//! | `BrowserVm` (`dry_web::vm`) | the page | v86 (x86 in WebAssembly) booting Alpine; the serial console is the shell |
//! | `Local` (later, `dry_tools`) | this computer | docker / podman |
//! | `Remote` (later) | the dry server host | the host's `Local`, over its API |
//!
//! The `terminal` artifact's run / start-with-callback / background are
//! meant to target a container through this trait (not wired yet).

/// What a finished command left: its exit code and everything it printed
/// (stdout and stderr together, as a terminal shows them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub code: i32,
    pub text: String,
}

/// A folder of the shell's put inside the container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub from: String,
    pub to: String,
}

/// A container, whatever runs it. A sketch: `BrowserVm` implements it;
/// `Local` and `Remote` come later (docs/container.md).
#[allow(async_fn_in_trait)]
pub trait Container {
    /// Start one from `image` with `mounts`.
    async fn create(&mut self, image: &str, mounts: &[Mount]) -> Result<(), String>;
    /// Run `cmd` (a `sh -c` line) to its end.
    async fn exec(&mut self, cmd: &str) -> Result<Output, String>;
    /// Start `cmd` and return its id at once; `done` is called with its
    /// output when it ends (the terminal artifact's start-with-callback).
    fn start(&mut self, cmd: &str, done: Box<dyn FnOnce(Output)>) -> Result<u64, String>;
    async fn stop(&mut self) -> Result<(), String>;
    /// An HTTP GET of `path` on `port` inside the container: what a
    /// server there serves, as evidence.
    async fn get(&mut self, port: u16, path: &str) -> Result<Http, String>;
    /// Keep what the container holds now, so it comes back after a reload.
    async fn snapshot(&mut self) -> Result<String, String>;
}

// ---- the exec markers ----
//
// A console shell gives one stream of text. Each exec runs as
// `sh -c '<cmd>' </dev/null 2>&1` and is followed by a line that prints a
// marker with its id and `$?`; the output is the text up to that marker.
// The marker as typed holds `$?`, not digits, so an echoed command line is
// never taken for the marker.

fn marker(id: u64) -> String {
    format!("__dry_exit_{id}_")
}

/// Quote `s` for `sh` in single quotes.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The line to type for exec `id`.
pub fn exec_line(id: u64, cmd: &str) -> String {
    format!("sh -c {} </dev/null 2>&1; printf '\\n{}%d__\\n' $?\n", sh_quote(cmd), marker(id))
}

/// The line to type to start `cmd` in the background as job `id`: its
/// output goes to `/tmp/dry-job-<id>.log`, and its marker comes when it
/// ends (`take` on the log's text then gives its output).
pub fn start_line(id: u64, cmd: &str) -> String {
    format!(
        "( sh -c {} </dev/null >/tmp/dry-job-{id}.log 2>&1; printf '\\n{}%d__\\n' $? ) &\n",
        sh_quote(cmd),
        marker(id)
    )
}

/// Find exec `id`'s marker in `buf`: its output (without the newline the
/// marker line starts with, and with `\r\n` as `\n`) and exit code, and
/// the text after the marker line. `None` until the marker has come.
pub fn take(buf: &str, id: u64) -> Option<(Output, String)> {
    let m = marker(id);
    let mut from = 0;
    while let Some(at) = buf[from..].find(&m).map(|i| i + from) {
        let rest = &buf[at + m.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with("__") {
            let code = digits.parse().unwrap_or(-1);
            let mut text = buf[..at].replace("\r\n", "\n");
            if text.ends_with('\n') {
                text.pop();
            }
            let after = rest[digits.len() + 2..].trim_start_matches(['\r', '\n']).to_string();
            return Some((Output { code, text }, after));
        }
        from = at + m.len();
    }
    None
}

// ---- HTTP to a port ----

/// What a GET to a port in the container answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Http {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// An HTTP/1.0 GET (the server closes the connection after it answers).
pub fn http_get(path: &str) -> String {
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    format!("GET {path} HTTP/1.0\r\nHost: localhost\r\nUser-Agent: dry\r\nAccept: */*\r\nConnection: close\r\n\r\n")
}

/// Read what came back.
pub fn http_parse(raw: &[u8]) -> Result<Http, String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").or_else(|| text.split_once("\n\n")).ok_or("no HTTP header in the answer")?;
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let status = first.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("not an HTTP status line: {first}"))?;
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    Ok(Http { status, headers, body: body.to_string() })
}

/// The visible text of an HTML page, roughly: tags and scripts dropped,
/// whitespace folded. What an engine reads as evidence.
pub fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        out.push(' ');
        rest = &rest[i..];
        let lower = rest.get(..8).unwrap_or(rest).to_ascii_lowercase();
        let skip = if lower.starts_with("<script") {
            Some("</script>")
        } else if lower.starts_with("<style") {
            Some("</style>")
        } else {
            None
        };
        rest = match skip.and_then(|end| rest.to_ascii_lowercase().find(end).map(|j| j + end.len())) {
            Some(j) => &rest[j..],
            None => rest.find('>').map_or("", |j| &rest[j + 1..]),
        };
    }
    out.push_str(rest);
    let out = out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_line_quotes_and_marks() {
        let l = exec_line(3, "echo 'hi' && false");
        assert_eq!(l, "sh -c 'echo '\\''hi'\\'' && false' </dev/null 2>&1; printf '\\n__dry_exit_3_%d__\\n' $?\n");
    }

    #[test]
    fn take_finds_the_marker_not_the_echo() {
        // An echoed command line has `%d` and `$?`, never digits.
        let echoed = exec_line(7, "ls");
        let buf = format!("{echoed}a\r\nb\r\n\r\n__dry_exit_7_2__\r\nnext");
        let (out, rest) = take(&buf, 7).unwrap();
        assert_eq!(out.code, 2);
        assert!(out.text.ends_with("a\nb\n"), "{:?}", out.text);
        assert_eq!(rest, "next");
        assert!(take("a\r\n__dry_exit_7_", 7).is_none());
        assert!(take("__dry_exit_8_0__", 7).is_none());
    }

    #[test]
    fn take_without_output() {
        let (out, rest) = take("\r\n__dry_exit_1_0__\r\n", 1).unwrap();
        assert_eq!(out, Output { code: 0, text: String::new() });
        assert_eq!(rest, "");
    }

    #[test]
    fn http_round_trip() {
        assert!(http_get("x").starts_with("GET /x HTTP/1.0\r\n"));
        let h = http_parse(b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<h1>hi</h1>").unwrap();
        assert_eq!(h.status, 200);
        assert_eq!(h.header("content-type"), Some("text/html"));
        assert_eq!(h.body, "<h1>hi</h1>");
        assert!(http_parse(b"garbage").is_err());
    }

    #[test]
    fn html_text_drops_tags_and_scripts() {
        let t = html_text("<html><head><style>b{}</style><script>x<1</script></head><body><h1>Directory listing</h1><a href=\"a\">a&amp;b</a></body></html>");
        assert_eq!(t, "Directory listing a&b");
    }
}
