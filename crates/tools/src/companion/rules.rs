//! The contract's rules, pure (docs/companion-protocol.md): origins and
//! Host, secret-looking names, the folder's paths, terminal keys,
//! publishing commands, store paths, providers.json entries. The same
//! rules the Node companion keeps; tested here and by `conformance/`.

use serde_json::{Map, Value, json};

/// Folders and files never listed (`dry_core::workbench::HIDDEN`).
pub const HIDDEN: [&str; 4] = [".git", "target", "node_modules", ".DS_Store"];
/// The folder dry keeps its own files in, inside the workspace folder.
pub const DRY_DIR: &str = ".dry";
/// A file read whole up to this (the editor's limit).
pub const MAX_READ: u64 = 1_000_000;

/// An answer that is an error: its status, its code (for programs) and a
/// sentence (for people). docs/companion-protocol.md §2.
#[derive(Clone, Debug, PartialEq)]
pub struct Fail {
    pub status: u16,
    pub code: &'static str,
    pub error: String,
}

pub fn fail(status: u16, code: &'static str, error: impl Into<String>) -> Fail {
    Fail { status, code, error: error.into() }
}

impl Fail {
    pub fn json(&self) -> Value {
        json!({ "error": self.error, "code": self.code })
    }
}

/// Origins a page may call from: the hosted app, any local copy, and the extras given.
pub fn origin_allowed(origin: &str, extra: &[String]) -> bool {
    if origin.is_empty() || origin == "null" {
        return false;
    }
    if origin == "https://kaush4l.github.io" {
        return true;
    }
    for host in ["http://localhost", "http://127.0.0.1", "http://[::1]"] {
        if let Some(rest) = origin.strip_prefix(host) {
            if rest.is_empty() {
                return true;
            }
            if let Some(port) = rest.strip_prefix(':') {
                return (1..=5).contains(&port.len()) && port.bytes().all(|b| b.is_ascii_digit());
            }
        }
    }
    extra.iter().any(|e| e == origin.trim_end_matches('/'))
}

/// The Host header names this server by a loopback name and its port
/// (DNS rebinding is refused).
pub fn host_allowed(host: &str, port: u16) -> bool {
    let h = host.to_ascii_lowercase();
    [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")].contains(&h)
}

/// A secret-looking name (one part of a path): `dry_core::file::secret_file`,
/// plus `.git-credentials` and the key folders (docs/companion-protocol.md §6).
pub fn secret_name(name: &str) -> bool {
    let n = name.to_lowercase();
    dry_core::file::secret_file(&n) || n == ".git-credentials" || [".ssh", ".gnupg", ".aws", ".kube", ".docker"].contains(&n.as_str())
}

/// A path the page gives for the folder: relative, `/`-separated (a `\`
/// is a `/`), no part that leaves it; `""` or `.` is the folder. Its
/// parts, or why not: an absolute path or `..` past the folder is
/// `path_outside`, a secret-looking part `secret_path`, `.dry/` `dry_state`.
pub fn folder_parts(p: &str) -> Result<Vec<String>, Fail> {
    if p.contains('\0') {
        return Err(fail(400, "bad_request", "a path is text with no zero byte"));
    }
    let t = p.replace('\\', "/");
    let drive = t.len() >= 2 && t.as_bytes()[0].is_ascii_alphabetic() && t.as_bytes()[1] == b':';
    if t.starts_with('/') || drive {
        return Err(fail(403, "path_outside", format!("{p} is absolute: paths are relative to the folder")));
    }
    let mut parts: Vec<String> = Vec::new();
    for s in t.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(fail(403, "path_outside", format!("{p} leads outside the folder")));
                }
            }
            s => parts.push(s.to_string()),
        }
    }
    check_parts(&parts)?;
    Ok(parts)
}

/// The parts of a real path inside the folder: no secret, not `.dry/`.
pub fn check_parts(parts: &[String]) -> Result<(), Fail> {
    if let Some(s) = parts.iter().find(|s| secret_name(s)) {
        return Err(fail(403, "secret_path", format!("{s} may hold secrets, so it is never read, written or listed")));
    }
    if parts.first().is_some_and(|s| s == DRY_DIR) {
        return Err(fail(403, "dry_state", format!("{DRY_DIR}/ is dry's own state: it goes through /v1/store and /v1/workspace")));
    }
    Ok(())
}

/// A terminal session's key: `[A-Za-z0-9._:/-]{1,200}`.
pub fn key_ok(k: &str) -> bool {
    (1..=200).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
}

/// A store path: relative, `/`-separated, no empty, `.` or `..` part.
pub fn store_path_ok(p: &str) -> bool {
    !p.is_empty()
        && p.len() < 512
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains('\0')
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Whether `word` is `verb` followed by nothing or by a character that
/// ends a word (`push`, `push;`, not `pushx`).
fn is_verb(word: &str, verb: &str) -> bool {
    word.strip_prefix(verb).is_some_and(|rest| rest.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_')))
}

/// Commands that publish or fetch from outside (`git push`, `git clone`,
/// `gh pr create`, …): only through the git endpoints with a person's
/// click (`428 needs_confirm`). A guard against an engine wandering into
/// a publish, read from the command text, not a sandbox.
pub fn needs_confirm(command: &str) -> bool {
    let spaced: String = command.chars().map(|c| if ";&|(`".contains(c) { ' ' } else { c }).collect();
    let words: Vec<&str> = spaced.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if *w == "git" {
            // Options (`-C dir`, `-c k=v`) may come before the verb.
            let mut after_option = false;
            for t in &words[i + 1..] {
                if is_verb(t, "push") || is_verb(t, "clone") {
                    return true;
                }
                if t.starts_with('-') {
                    after_option = true;
                } else if after_option {
                    after_option = false;
                } else {
                    break;
                }
            }
        }
        if *w == "gh" {
            let next = |k: usize| words.get(i + k).copied().unwrap_or("");
            let publishing = match next(1) {
                "repo" => ["create", "clone", "delete", "fork"].iter().any(|v| is_verb(next(2), v)),
                "pr" => ["create", "merge"].iter().any(|v| is_verb(next(2), v)),
                "release" => is_verb(next(2), "create"),
                _ => false,
            };
            if publishing {
                return true;
            }
        }
    }
    false
}

// ---- providers.json (docs/companion-protocol.md §8) ----

pub const PROVIDER_KINDS: [&str; 3] = ["cli", "port", "native"];
pub const CLIS: [&str; 3] = ["claude", "codex", "copilot"];
pub const RUNNERS: [&str; 3] = ["auto", "mlx", "llama.cpp"];

/// How to get each program the providers name: printed when it is missing, never run.
pub fn install_hint(what: &str) -> &'static str {
    match what {
        "claude" => "npm install -g @anthropic-ai/claude-code",
        "codex" => "npm install -g @openai/codex",
        "copilot" => "npm install -g @github/copilot",
        "mlx" => "pip install mlx-lm   (Apple silicon; gives mlx_lm.server)",
        _ => {
            "brew install llama.cpp, or build https://github.com/ggml-org/llama.cpp (-DGGML_CUDA=ON on NVIDIA, -DGGML_VULKAN=ON for Vulkan)"
        }
    }
}

/// The runner's program name.
pub fn runner_bin(runner: &str) -> &'static str {
    if runner == "mlx" { "mlx_lm.server" } else { "llama-server" }
}

/// One declared provider, checked and normalised.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Entry {
    pub name: String,
    pub kind: String,
    pub cli: Option<String>,
    pub path: Option<String>,
    pub model: Option<String>,
    pub url: Option<String>,
    pub runner: Option<String>,
    pub port: Option<u16>,
}

impl Entry {
    /// As written into providers.json (and in an answer's `entry`).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), json!(self.name));
        m.insert("kind".into(), json!(self.kind));
        for (k, v) in [("model", &self.model), ("path", &self.path), ("cli", &self.cli), ("url", &self.url), ("runner", &self.runner)] {
            if let Some(v) = v {
                m.insert(k.into(), json!(v));
            }
        }
        if let Some(p) = self.port {
            m.insert("port".into(), json!(p));
        }
        Value::Object(m)
    }
}

fn model_ok(m: &str) -> bool {
    (1..=200).contains(&m.len()) && m.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:@/+-".contains(&b))
}

fn name_ok(n: &str) -> bool {
    let b = n.as_bytes();
    (1..=40).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// A port provider's URL: http(s) with a host; a trailing `/` and `/v1` dropped.
fn base_url(u: &str) -> Option<String> {
    let rest = u.strip_prefix("http://").or_else(|| u.strip_prefix("https://"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    let t = u.trim_end_matches('/');
    Some(t.strip_suffix("/v1").unwrap_or(t).trim_end_matches('/').to_string())
}

/// One providers.json entry, checked and normalised, or why not.
pub fn check_provider(e: &Value) -> Result<Entry, String> {
    let o = e.as_object().ok_or("a provider is an object")?;
    let name = match o.get("name") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    if !name_ok(&name) {
        return Err(format!(
            "name: {} is not a provider name (lower case letters, digits, - and _)",
            o.get("name").cloned().unwrap_or(Value::Null)
        ));
    }
    let kind = o.get("kind").and_then(Value::as_str).unwrap_or("");
    if !PROVIDER_KINDS.contains(&kind) {
        return Err(format!("{name}: kind is one of {}", PROVIDER_KINDS.join(", ")));
    }
    let mut out = Entry { name: name.clone(), kind: kind.into(), ..Entry::default() };
    let text = |k: &str, why: &str| -> Result<Option<String>, String> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.is_empty() => Ok(None),
            Some(Value::String(s)) if !s.contains('\0') => Ok(Some(s.clone())),
            _ => Err(format!("{name}: {k} {why}")),
        }
    };
    out.model = text("model", "is a model name or path")?;
    if out.model.as_deref().is_some_and(|m| !model_ok(m)) {
        return Err(format!("{name}: model is a model name or path"));
    }
    out.path = text("path", "is the program's path")?;
    if let Some(p) = &out.path
        && !std::path::Path::new(&home(p)).is_absolute()
    {
        return Err(format!("{name}: path is an absolute path to the program"));
    }
    match kind {
        "cli" => {
            let cli = o.get("cli").and_then(Value::as_str).unwrap_or(&name).to_string();
            if !CLIS.contains(&cli.as_str()) {
                return Err(format!("{name}: cli is one of {}", CLIS.join(", ")));
            }
            out.cli = Some(cli);
        }
        "port" => {
            let u = o.get("url").and_then(Value::as_str).unwrap_or("");
            if !(u.contains("://")) {
                return Err(format!("{name}: url is the server's address (http://127.0.0.1:1234)"));
            }
            out.url = Some(base_url(u).ok_or_else(|| format!("{name}: url is http:// or https://"))?);
        }
        _ => {
            let runner = o.get("runner").and_then(Value::as_str).unwrap_or("auto");
            if !RUNNERS.contains(&runner) {
                return Err(format!("{name}: runner is one of {}", RUNNERS.join(", ")));
            }
            out.runner = Some(runner.into());
            if out.model.is_none() {
                return Err(format!("{name}: a native provider needs a model (a local file or folder, or a model id already downloaded)"));
            }
            if let Some(p) = o.get("port").filter(|p| !p.is_null()) {
                match p.as_u64() {
                    Some(n @ 1024..=65535) => out.port = Some(n as u16),
                    _ => return Err(format!("{name}: port is 1024-65535")),
                }
            }
        }
    }
    Ok(out)
}

/// A providers.json text: its entries, and the ones that did not read
/// (`{entry, error}`).
pub fn parse_providers(text: &str) -> (Vec<Entry>, Vec<Value>) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return (Vec::new(), vec![json!({ "entry": null, "error": format!("providers.json is not JSON: {e}") })]),
    };
    let Some(list) = v.get("providers").and_then(Value::as_array) else {
        return (Vec::new(), vec![json!({ "entry": null, "error": "providers.json needs \"providers\": [ ... ]" })]);
    };
    let (mut out, mut errors) = (Vec::new(), Vec::new());
    for e in list {
        match check_provider(e) {
            Ok(p) if out.iter().any(|x: &Entry| x.name == p.name) => {
                errors.push(json!({ "entry": p.name, "error": format!("a second provider named {}", p.name) }))
            }
            Ok(p) => out.push(p),
            Err(error) => errors.push(json!({ "entry": e.get("name").cloned().unwrap_or(Value::Null), "error": error })),
        }
    }
    (out, errors)
}

/// The runner a native provider uses on this machine, and its
/// acceleration: Apple silicon runs mlx, NVIDIA llama.cpp with CUDA,
/// anything else llama.cpp on the CPU.
pub fn choose_runner(runner: &str, os: &str, arch: &str, nvidia: bool) -> (&'static str, &'static str) {
    match runner {
        "mlx" => ("mlx", "metal"),
        "llama.cpp" => (
            "llama.cpp",
            if nvidia {
                "cuda"
            } else if os == "macos" {
                "metal"
            } else {
                "cpu"
            },
        ),
        _ if os == "macos" && arch == "aarch64" => ("mlx", "metal"),
        _ => ("llama.cpp", if nvidia { "cuda" } else { "cpu" }),
    }
}

/// The runner's argv: the model it serves on a local port. Never a
/// download flag (llama.cpp's `-hf`): the model is on disk.
pub fn runner_argv(runner: &str, bin: &str, model: &str, port: u16) -> Vec<String> {
    let flag = if runner == "mlx" { "--model" } else { "-m" };
    [bin, flag, model, "--host", "127.0.0.1", "--port", &port.to_string()].iter().map(|s| s.to_string()).collect()
}

/// `~` and `~/…` as the home folder.
pub fn home(p: &str) -> String {
    let h = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    if p == "~" {
        h
    } else if let Some(rest) = p.strip_prefix("~/") {
        format!("{h}/{rest}")
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_and_hosts() {
        for o in ["https://kaush4l.github.io", "http://localhost", "http://localhost:5173", "http://127.0.0.1:9", "http://[::1]:80"] {
            assert!(origin_allowed(o, &[]), "{o}");
        }
        for o in ["", "null", "https://evil.example", "http://localhost.evil.example", "http://localhost:123456", "http://192.168.1.5:8080"]
        {
            assert!(!origin_allowed(o, &[]), "{o}");
        }
        assert!(origin_allowed("https://x.test/", &["https://x.test".into()]));
        assert!(host_allowed("LOCALHOST:7", 7) && host_allowed("[::1]:7", 7) && !host_allowed("localhost", 7));
        assert!(!host_allowed("127.0.0.1:8", 7) && !host_allowed("evil.example:7", 7));
    }

    #[test]
    fn folder_paths_and_secrets() {
        assert_eq!(folder_parts("src/./deep/../b.rs").unwrap(), ["src", "b.rs"]);
        assert_eq!(folder_parts("").unwrap(), Vec::<String>::new());
        for (p, code) in [
            ("../x", "path_outside"),
            ("/etc/passwd", "path_outside"),
            ("C:\\Windows\\win.ini", "path_outside"),
            ("a/../../b", "path_outside"),
            (".env", "secret_path"),
            ("src/.env.local", "secret_path"),
            (".aws/config", "secret_path"),
            ("new/.npmrc", "secret_path"),
            ("x/.git-credentials", "secret_path"),
            (".dry/state/x", "dry_state"),
        ] {
            assert_eq!(folder_parts(p).unwrap_err().code, code, "{p}");
        }
        assert!(secret_name("deploy.PEM") && secret_name("id_ed25519.pub") && !secret_name("main.rs"));
    }

    #[test]
    fn keys_store_paths_and_publishing() {
        assert!(key_ok("e1/term/1") && !key_ok("has space") && !key_ok(""));
        assert!(store_path_ok("engines/e1/spec/goal.md"));
        for bad in ["../x.md", "/abs.md", "a/../../b.md", "a//b.md", "a/./b"] {
            assert!(!store_path_ok(bad), "{bad}");
        }
        for c in ["git push origin main", "cd x && git push", "git -C repo push", "git clone https://x", "gh pr create", "gh repo fork x"] {
            assert!(needs_confirm(c), "{c}");
        }
        for c in ["git status", "echo legit push", "git pushx", "xgit push", "gh pr list", "ls"] {
            assert!(!needs_confirm(c), "{c}");
        }
    }

    #[test]
    fn provider_entries() {
        let p = check_provider(&json!({"name": "fake", "kind": "port", "url": "http://127.0.0.1:1234/v1/"})).unwrap();
        assert_eq!(p.url.as_deref(), Some("http://127.0.0.1:1234"));
        assert_eq!(check_provider(&json!({"name": "c", "kind": "cli"})).unwrap_err(), "c: cli is one of claude, codex, copilot");
        assert_eq!(check_provider(&json!({"name": "claude", "kind": "cli"})).unwrap().cli.as_deref(), Some("claude"));
        for bad in [
            json!({"name": "x", "kind": "port", "url": "ftp://nope"}),
            json!({"name": "x", "kind": "native", "runner": "mlx"}),
            json!({"name": "x", "kind": "cli", "cli": "gemini"}),
            json!({"name": "Bad", "kind": "port", "url": "http://a"}),
            json!({"name": "x", "kind": "cloud"}),
            json!({"name": "x", "kind": "cli", "cli": "claude", "path": "relative/claude"}),
        ] {
            assert!(check_provider(&bad).is_err(), "{bad}");
        }
        let (ok, errors) = parse_providers(
            r#"{"providers": [{"name": "a", "kind": "port", "url": "http://a"}, {"name": "a", "kind": "port", "url": "http://b"}, {"name": "BAD NAME"}]}"#,
        );
        assert_eq!(ok.len(), 1);
        assert_eq!(errors.len(), 2);
        assert_eq!(choose_runner("auto", "macos", "aarch64", false), ("mlx", "metal"));
        assert_eq!(choose_runner("auto", "linux", "x86_64", true), ("llama.cpp", "cuda"));
        assert!(!runner_argv("llama.cpp", "llama-server", "/m.gguf", 9000).iter().any(|a| a == "-hf"));
    }
}
