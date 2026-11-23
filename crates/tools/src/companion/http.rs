//! The HTTP/1.1 the companion speaks, on std's `TcpListener`: one request
//! per connection (answered with `connection: close`), a body by
//! `content-length` or chunked (at most 8 MB), and server-sent streams
//! that stay open on the same socket. Enough for a page's `fetch`, the
//! conformance suite and MCP clients on 127.0.0.1; nothing else listens.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

/// A body is at most this (`413 too_large`).
pub const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_HEAD: usize = 64 * 1024;

/// One request, read.
#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    /// The path, percent-decoded (`/v1/fs/read`).
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Lower-case names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body was over `MAX_BODY` (and was not read).
    pub too_large: bool,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn q(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// `%XX` (and in a query, `+`) decoded; bad escapes kept as they are.
pub fn decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        match b[i] {
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' if plus => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read one request from `s`. `None`: the peer closed or sent no HTTP.
pub fn read(s: &TcpStream) -> Option<Request> {
    let mut r = BufReader::new(s);
    let mut line = String::new();
    let mut head = 0usize;
    r.read_line(&mut line).ok()?;
    head += line.len();
    let mut words = line.split_whitespace();
    let (method, target) = (words.next()?.to_string(), words.next()?.to_string());
    let mut req = Request { method, ..Request::default() };
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    req.path = decode(path, false);
    req.query = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k, true), decode(v, true))
        })
        .collect();
    loop {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        head += line.len();
        if head > MAX_HEAD {
            return None;
        }
        let t = line.trim_end_matches(['\r', '\n']);
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            req.headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    if req.header("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) {
        loop {
            line.clear();
            r.read_line(&mut line).ok()?;
            let n = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16).ok()?;
            if n == 0 {
                // Trailers, then the empty line.
                loop {
                    line.clear();
                    if r.read_line(&mut line).ok()? == 0 || line.trim().is_empty() {
                        break;
                    }
                }
                break;
            }
            if req.body.len() + n > MAX_BODY {
                req.too_large = true;
                return Some(req);
            }
            let at = req.body.len();
            req.body.resize(at + n, 0);
            r.read_exact(&mut req.body[at..]).ok()?;
            line.clear();
            r.read_line(&mut line).ok()?;
        }
    } else if let Some(n) = req.header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        if n > MAX_BODY {
            req.too_large = true;
            return Some(req);
        }
        req.body.resize(n, 0);
        r.read_exact(&mut req.body).ok()?;
    }
    Some(req)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        428 => "Precondition Required",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// The status line and headers; `length`: a `content-length` (none: the
/// body runs until the connection closes, as a stream does).
pub fn head(status: u16, headers: &[(String, String)], length: Option<usize>) -> Vec<u8> {
    let mut h = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(n) = length {
        h.push_str(&format!("content-length: {n}\r\n"));
    }
    h.push_str("connection: close\r\n\r\n");
    h.into_bytes()
}

/// A whole answer.
pub fn write(mut s: &TcpStream, status: u16, headers: &[(String, String)], body: &[u8]) {
    let mut out = head(status, headers, Some(body.len()));
    out.extend_from_slice(body);
    let _ = s.write_all(&out);
    let _ = s.flush();
}

/// An open stream's socket, written from any thread.
pub type Sink = Arc<Mutex<TcpStream>>;

/// Write to a stream; false: it is gone.
pub fn send(sink: &Sink, text: &str) -> bool {
    let Ok(mut s) = sink.lock() else { return false };
    s.write_all(text.as_bytes()).and_then(|_| s.flush()).is_ok()
}

/// Wait until the peer closes the stream's socket (or it is shut down).
pub fn wait_closed(s: &TcpStream) {
    let _ = s.set_read_timeout(None);
    let mut buf = [0u8; 512];
    let mut r = s;
    while let Ok(n) = r.read(&mut buf) {
        if n == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(decode("a%2Fb+c", true), "a/b c");
        assert_eq!(decode("a+b", false), "a+b");
        assert_eq!(decode("100%", true), "100%");
        assert_eq!(decode("%E2%9C%93", false), "✓");
    }
}
