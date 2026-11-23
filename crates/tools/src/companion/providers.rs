//! Model providers (docs/companion-protocol.md §8): declared in
//! providers.json, never scanned; each probed (a CLI's `--version`, a
//! port's `/v1/models` within 3 s, a native runner started on a local
//! port), its models `<name>/<model>`, a chat passed through or run by
//! its CLI. Nothing is ever installed or downloaded.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::rules::{self, Entry, Fail, fail, home};
use super::state::{Companion, ProviderState, write_atomic};

/// The template a new providers.json starts from (the repo's).
pub const TEMPLATE: &str = include_str!("../../../../providers.json");

fn template() -> Value {
    serde_json::from_str(TEMPLATE).unwrap_or_else(|_| json!({ "version": 1, "providers": [] }))
}

/// GET with a timeout; the body, or why not.
fn get(url: &str, timeout: Duration) -> Result<String, String> {
    let mut resp =
        ureq::get(url).config().timeout_global(Some(timeout)).http_status_as_error(false).build().call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status >= 300 {
        return Err(format!("{url} answered {status}"));
    }
    Ok(body)
}

/// A server's models: `GET <url>/v1/models`, its `data[].id`.
fn models_at(url: &str, timeout: Duration) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(&get(&format!("{url}/v1/models"), timeout)?).map_err(|e| format!("not JSON: {e}"))?;
    Ok(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
}

/// What providers.json declares (and `--lmstudio` / `--ollama`, as ports),
/// the entries that did not read, and whether the file is there.
pub fn declared(c: &Companion) -> (Vec<Entry>, Vec<Value>, bool) {
    let text = std::fs::read_to_string(&c.providers_file).ok();
    let (mut list, errors) = text.as_deref().map(rules::parse_providers).unwrap_or_default();
    for (name, url) in [("lmstudio", &c.lmstudio), ("ollama", &c.ollama)] {
        if let Some(u) = url
            && !list.iter().any(|p| p.name == name)
            && let Ok(e) = rules::check_provider(&json!({ "name": name, "kind": "port", "url": u }))
        {
            list.push(e);
        }
    }
    (list, errors, text.is_some())
}

fn hf_cached(id: &str) -> bool {
    let hub = std::env::var("HF_HUB_CACHE").unwrap_or_else(|_| {
        let hf = std::env::var("HF_HOME").unwrap_or_else(|_| home("~/.cache/huggingface"));
        format!("{hf}/hub")
    });
    Path::new(&hub).join(format!("models--{}", id.replace('/', "--"))).exists()
}

fn blank(entry: &Entry) -> ProviderState {
    ProviderState {
        entry: entry.clone(),
        state: "failed",
        why: String::new(),
        hint: String::new(),
        models: Vec::new(),
        url: None,
        bin: None,
        version: None,
        runner: None,
        child: None,
    }
}

/// A program by its given path, else on PATH.
fn program(path: &Option<String>, name: &str) -> Option<String> {
    match path {
        Some(p) => {
            let p = home(p);
            Path::new(&p).is_file().then_some(p)
        }
        None => crate::cli::which(name).map(|p| p.display().to_string()),
    }
}

/// One provider probed: ok (its models), failed (why), absent (the
/// program is not here: the hint says how to get it).
pub fn probe(c: &Companion, entry: &Entry) -> ProviderState {
    let mut r = blank(entry);
    match entry.kind.as_str() {
        "cli" => {
            let cli = entry.cli.clone().unwrap_or_default();
            match program(&entry.path, &cli) {
                None => {
                    r.state = "absent";
                    r.why = match &entry.path {
                        Some(p) => format!("{p}: no such program"),
                        None => format!("{cli} is not on PATH"),
                    };
                    r.hint = match &entry.path {
                        Some(_) => format!("fix \"path\", or {}", rules::install_hint(&cli)),
                        None => rules::install_hint(&cli).into(),
                    };
                }
                Some(bin) => match crate::cli::version(Path::new(&bin)) {
                    Ok(v) => {
                        r.state = "ok";
                        r.version = Some(v.chars().take(120).collect());
                        r.models = match &entry.model {
                            Some(m) => vec![m.clone()],
                            None => match cli.as_str() {
                                "claude" => vec!["sonnet".into(), "opus".into(), "haiku".into()],
                                "codex" => vec!["default".into()],
                                _ => vec!["auto".into()],
                            },
                        };
                        r.bin = Some(bin);
                    }
                    Err(e) => {
                        r.why = format!("{bin} --version did not answer: {e}");
                        r.bin = Some(bin);
                    }
                },
            }
        }
        "port" => {
            let url = entry.url.clone().unwrap_or_default();
            match models_at(&url, Duration::from_secs(3)) {
                Ok(m) => {
                    r.state = "ok";
                    r.models = m;
                }
                Err(e) => {
                    r.why = format!("GET {url}/v1/models: {e}");
                    r.hint = "is its server running, on that port?".into();
                }
            }
            r.url = Some(url);
        }
        _ => {
            let nvidia = c.program("nvidia-smi").is_some();
            let (runner, accel) =
                rules::choose_runner(entry.runner.as_deref().unwrap_or("auto"), std::env::consts::OS, std::env::consts::ARCH, nvidia);
            r.runner = Some((runner, accel));
            let model = home(entry.model.as_deref().unwrap_or(""));
            match program(&entry.path, rules::runner_bin(runner)) {
                None => {
                    r.state = "absent";
                    r.why = format!(
                        "{} ({runner}, {accel}) is not on this machine",
                        entry.path.clone().unwrap_or_else(|| rules::runner_bin(runner).into())
                    );
                    r.hint = rules::install_hint(runner).into();
                }
                Some(_) if !Path::new(&model).exists() && !(runner == "mlx" && hf_cached(&model)) => {
                    r.why = format!("the model {model} is not on this machine (the companion never downloads)");
                    r.hint = if runner == "mlx" {
                        format!("download it yourself: huggingface-cli download {model}")
                    } else {
                        "give \"model\" the path of a .gguf file on this machine".into()
                    };
                }
                Some(bin) => {
                    r.bin = Some(bin.clone());
                    // A runner this companion already runs is asked again; else one is started.
                    let running = {
                        let mut all = c.providers.lock().unwrap();
                        all.get_mut(&entry.name).and_then(|st| {
                            let alive = st.child.as_mut().is_some_and(|ch| matches!(ch.try_wait(), Ok(None)));
                            if alive { st.url.clone() } else { None }
                        })
                    };
                    match running {
                        Some(url) => {
                            match models_at(&url, Duration::from_secs(3)) {
                                Ok(m) => {
                                    r.state = "ok";
                                    r.models = m;
                                }
                                Err(_) => {
                                    r.state = "starting";
                                    r.why = "the runner is starting".into();
                                }
                            }
                            r.url = Some(url);
                        }
                        None => start_runner(c, entry, runner, &bin, &model, &mut r),
                    }
                }
            }
        }
    }
    r
}

/// A native runner started on a local port, then registered as a port
/// (its `/v1/models` answering is what makes it ok).
fn start_runner(c: &Companion, entry: &Entry, runner: &str, bin: &str, model: &str, r: &mut ProviderState) {
    let port = match entry.port.map(Ok).unwrap_or_else(crate::evidence::free_port) {
        Ok(p) => p,
        Err(e) => {
            r.why = e;
            return;
        }
    };
    let argv = rules::runner_argv(runner, bin, model, port);
    let url = format!("http://127.0.0.1:{port}");
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    for (name, _) in std::env::vars_os() {
        if dry_core::terminal::secret_env(&name.to_string_lossy()) {
            cmd.env_remove(&name);
        }
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = match cmd.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            r.why = format!("could not start {bin}: {e}");
            return;
        }
    };
    let err = Arc::new(Mutex::new(String::new()));
    if let Some(mut pipe) = child.stderr.take() {
        let err = err.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut e = err.lock().unwrap();
                e.push_str(&String::from_utf8_lossy(&buf[..n]));
                if e.len() > 2000 {
                    let cut = e.len() - 2000;
                    let at = (cut..e.len()).find(|i| e.is_char_boundary(*i)).unwrap_or(cut);
                    *e = e[at..].to_string();
                }
            }
        });
    }
    c.log(&format!(
        "provider {}: started {} ({runner}) on {url}",
        entry.name,
        Path::new(bin).file_name().unwrap_or_default().to_string_lossy()
    ));
    let last = |e: &Mutex<String>| e.lock().unwrap().trim().lines().last().unwrap_or("no output").to_string();
    r.url = Some(url.clone());
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(120) {
        if let Ok(Some(st)) = child.try_wait() {
            r.why = format!("the runner exited ({}): {}", st.code().map_or("a signal".into(), |c| c.to_string()), last(&err));
            return;
        }
        if let Ok(m) = models_at(&url, Duration::from_secs(1)) {
            r.state = "ok";
            r.models = m;
            r.child = Some(child);
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    r.state = "starting";
    r.why = "the runner is still loading its model".into();
    r.child = Some(child);
}

/// Stop a runner this companion started (its process group).
pub fn stop_runner(st: &mut ProviderState) {
    if let Some(mut ch) = st.child.take() {
        super::sys::kill_group(ch.id(), super::sys::SIGTERM);
        let _ = ch.kill();
        let _ = ch.wait();
    }
}

/// Keep a probe's state (a runner already running is kept with it).
fn keep(c: &Companion, mut r: ProviderState) {
    let mut all = c.providers.lock().unwrap();
    if let Some(old) = all.get_mut(&r.entry.name)
        && r.child.is_none()
        && old.child.is_some()
    {
        if r.url.is_some() && r.url == old.url {
            r.child = old.child.take();
        } else {
            stop_runner(old);
        }
    }
    all.insert(r.entry.name.clone(), r);
}

/// Every declared provider probed (at once); the ones no longer declared forgotten.
pub fn probe_all(c: &Companion) {
    let (list, _, _) = declared(c);
    let states: Vec<ProviderState> = std::thread::scope(|sc| {
        let hs: Vec<_> = list.iter().map(|e| sc.spawn(move || probe(c, e))).collect();
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for st in states {
        keep(c, st);
    }
    let mut all = c.providers.lock().unwrap();
    let gone: Vec<String> = all.keys().filter(|n| !list.iter().any(|e| &&e.name == n)).cloned().collect();
    for n in gone {
        if let Some(mut st) = all.remove(&n) {
            stop_runner(&mut st);
        }
    }
}

pub fn public(r: &ProviderState) -> Value {
    let mut v = json!({ "name": r.entry.name, "kind": r.entry.kind, "entry": r.entry.to_json(), "state": r.state, "why": r.why, "hint": r.hint,
        "models": r.models.iter().map(|m| format!("{}/{m}", r.entry.name)).collect::<Vec<_>>() });
    if let Some(u) = &r.url {
        v["url"] = json!(u);
    }
    if let Some((runner, accel)) = r.runner {
        v["runner"] = json!(runner);
        v["accel"] = json!(accel);
    }
    if let Some(ver) = &r.version {
        v["version"] = json!(ver);
    }
    v
}

/// `GET /v1/providers`: probed when asked (`refresh`) or when one is new.
pub fn now(c: &Companion, refresh: bool) -> Value {
    let (list, errors, exists) = declared(c);
    let unknown = { list.iter().any(|e| !c.providers.lock().unwrap().contains_key(&e.name)) };
    if refresh || unknown {
        probe_all(c);
    }
    let all = c.providers.lock().unwrap();
    let providers: Vec<Value> = list
        .iter()
        .map(|e| match all.get(&e.name) {
            Some(st) => public(st),
            None => public(&ProviderState { why: "not probed".into(), ..blank(e) }),
        })
        .collect();
    json!({ "file": c.providers_file, "exists": exists, "template": template(), "errors": errors, "providers": providers })
}

/// A new entry into providers.json (from the page's form): checked, its
/// name not taken, written, probed.
pub fn add(c: &Companion, entry: &Value) -> Result<Value, Fail> {
    let p = rules::check_provider(entry).map_err(|e| fail(400, "bad_provider", e))?;
    let mut v: Value = std::fs::read_to_string(&c.providers_file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(template);
    if !v["providers"].is_array() {
        v["providers"] = json!([]);
    }
    let taken = v["providers"].as_array().is_some_and(|a| a.iter().any(|x| x["name"].as_str() == Some(&p.name)))
        || declared(c).0.iter().any(|x| x.name == p.name);
    if taken {
        return Err(fail(409, "exists", format!("a provider named {} is already declared", p.name)));
    }
    v["providers"].as_array_mut().expect("an array").push(p.to_json());
    write_atomic(&c.providers_file, &(serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"))
        .map_err(|e| fail(500, "internal", format!("{}: {e}", c.providers_file.display())))?;
    c.log(&format!("provider {} ({}) added to {}", p.name, p.kind, c.providers_file.display()));
    let r = probe(c, &p);
    let out = public(&r);
    keep(c, r);
    c.notify("notifications/tools/list_changed", json!({}));
    Ok(out)
}

/// Every model the ok providers serve, as `<provider>/<model>`.
pub fn models(c: &Companion) -> Value {
    let v = now(c, false);
    let data: Vec<Value> = v["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["state"] == "ok")
        .flat_map(|p| {
            p["models"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |id| json!({ "id": id, "object": "model", "owned_by": p["name"], "provider": p["name"], "kind": p["kind"] }))
        })
        .collect();
    json!({ "object": "list", "data": data })
}

/// What a chat needs of its provider: ok, and how to reach it.
enum Route {
    Port(String),
    Cli { cli: String, bin: String },
}

fn route(c: &Companion, name: &str) -> Result<Route, Fail> {
    if !c.providers.lock().unwrap().contains_key(name) {
        now(c, false);
    }
    let all = c.providers.lock().unwrap();
    let st = all.get(name).ok_or_else(|| {
        fail(
            404,
            "no_provider",
            format!("no provider {name}: declare it in {} (docs/companion-protocol.md §8)", c.providers_file.display()),
        )
    })?;
    if st.state != "ok" {
        return Err(fail(503, "provider_down", format!("the provider {name} is {}: {}", st.state, st.why)));
    }
    Ok(match st.entry.kind.as_str() {
        "cli" => Route::Cli { cli: st.entry.cli.clone().unwrap_or_default(), bin: st.bin.clone().unwrap_or_default() },
        _ => Route::Port(st.url.clone().unwrap_or_default()),
    })
}

/// `POST /v1/chat/completions`: `model` is `<name>/<model>`. A port's (or
/// a runner's) is passed through and streamed back as it streams; a CLI's
/// is run. Writes the answer itself; its status.
pub fn chat(c: &Companion, body: &Value, s: &TcpStream, cors: &[(String, String)]) -> Result<u16, Fail> {
    let model = body["model"].as_str().unwrap_or("").to_string();
    let (name, id) = model.split_once('/').unwrap_or((&model, ""));
    match route(c, name)? {
        Route::Port(url) => {
            let mut b = body.clone();
            b["model"] = json!(id);
            let mut resp = ureq::post(&format!("{url}/v1/chat/completions"))
                .header("Content-Type", "application/json")
                .config()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(600)))
                .build()
                .send(b.to_string().as_str())
                .map_err(|e| fail(502, "upstream", format!("could not reach {url} ({e}): is the model server running?")))?;
            let status = resp.status().as_u16();
            let ctype = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/json").to_string();
            let mut headers = cors.to_vec();
            headers.push(("content-type".into(), ctype));
            let mut w = s;
            let _ = w.write_all(&super::http::head(status, &headers, None));
            let mut reader = resp.body_mut().as_reader();
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || w.write_all(&buf[..n]).is_err() {
                    break;
                }
                let _ = w.flush();
            }
            Ok(status)
        }
        Route::Cli { cli, bin } => {
            let reply = cli_chat(&cli, &bin, id, body)?;
            let content = reply["choices"][0]["message"]["content"].clone();
            let (status, ctype, text) = if body["stream"].as_bool() == Some(true) {
                let mut chunk = reply.clone();
                chunk["object"] = json!("chat.completion.chunk");
                chunk["choices"] = json!([{ "index": 0, "delta": { "role": "assistant", "content": content }, "finish_reason": "stop" }]);
                (200, "text/event-stream", format!("data: {chunk}\n\ndata: [DONE]\n\n"))
            } else {
                (200, "application/json; charset=utf-8", reply.to_string())
            };
            let mut headers = cors.to_vec();
            headers.push(("content-type".into(), ctype.into()));
            super::http::write(s, status, &headers, text.as_bytes());
            Ok(status)
        }
    }
}

fn completion(model: &str, content: &str, usage: Value) -> Value {
    let id: String = super::sys::random(8).iter().map(|b| format!("{b:02x}")).collect();
    json!({ "id": format!("chatcmpl-{id}"), "object": "chat.completion", "created": super::sys::now(), "model": model,
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }], "usage": usage })
}

/// A chat for a CLI provider, as the protocol runs them: argument arrays,
/// in an empty folder, no tools (`claude -p --output-format json --tools ""`,
/// `codex exec --sandbox read-only --ephemeral -o FILE -`, `copilot -p …
/// --available-tools ""`). The chat flattened by `dry_core::cli::flatten`.
fn cli_chat(cli: &str, bin: &str, model: &str, body: &Value) -> Result<Value, Fail> {
    let input = dry_core::cli::flatten(&body.to_string()).map_err(|e| fail(400, "bad_request", e))?;
    let (system, prompt) = (input.system, input.prompt);
    let with_system = if system.is_empty() { prompt.clone() } else { format!("{system}\n\n{prompt}") };
    let full = format!("{cli}/{model}");
    let opts = crate::cli::Options { timeout: Duration::from_secs(600), cancel: None };
    let run = |args: Vec<String>, stdin: Option<String>| {
        let inv = dry_core::cli::Invocation { program: bin.to_string(), args, stdin };
        crate::cli::spawn(&inv, &opts, &mut |_| {}).map_err(|e| fail(502, "upstream", format!("{cli}: {e}")))
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match cli {
        "claude" => {
            let mut args = s(&["-p", "--output-format", "json", "--no-session-persistence", "--tools", "", "--strict-mcp-config"]);
            if !model.is_empty() && model != "default" {
                args.extend(s(&["--model", model]));
            }
            if !system.is_empty() {
                args.extend(["--system-prompt".to_string(), system.clone()]);
            }
            let (ok, out, err) = run(args, Some(prompt))?;
            let v: Option<Value> = serde_json::from_str(&out).ok();
            match v {
                Some(v) if ok && v["is_error"] != json!(true) => {
                    let u = &v["usage"];
                    let n = |k: &str| u[k].as_u64().unwrap_or(0);
                    let pin = n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
                    let usage =
                        json!({ "prompt_tokens": pin, "completion_tokens": n("output_tokens"), "total_tokens": pin + n("output_tokens") });
                    Ok(completion(&full, v["result"].as_str().unwrap_or(""), usage))
                }
                v => {
                    let why = v.and_then(|v| v["result"].as_str().map(String::from)).unwrap_or(if err.is_empty() { out } else { err });
                    Err(fail(502, "upstream", format!("claude -p failed: {}", why.chars().take(400).collect::<String>())))
                }
            }
        }
        "codex" => {
            let file = std::env::temp_dir()
                .join(format!("dry-codex-{}.txt", super::sys::random(6).iter().map(|b| format!("{b:02x}")).collect::<String>()));
            let mut args = s(&["exec", "--color", "never", "--sandbox", "read-only", "--skip-git-repo-check", "--ephemeral", "-o"]);
            args.push(file.display().to_string());
            if !model.is_empty() && model != "default" {
                args.extend(s(&["--model", model]));
            }
            args.push("-".into());
            let (ok, out, err) = run(args, Some(with_system))?;
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let _ = std::fs::remove_file(&file);
            if !ok && text.is_empty() {
                let why = if err.is_empty() { out } else { err };
                return Err(fail(
                    502,
                    "upstream",
                    format!("codex exec failed: {}", why.chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()),
                ));
            }
            Ok(completion(&full, text.trim(), json!({ "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 })))
        }
        _ => {
            let mut args = vec!["-p".to_string(), with_system];
            args.extend(s(&["-s", "--no-color", "--available-tools", ""]));
            if !model.is_empty() && model != "auto" {
                args.extend(s(&["--model", model]));
            }
            let (ok, out, err) = run(args, None)?;
            if !ok {
                let why = if err.is_empty() { out } else { err };
                return Err(fail(502, "upstream", format!("copilot -p failed: {}", why.chars().take(400).collect::<String>())));
            }
            Ok(completion(&full, out.trim(), json!({ "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 })))
        }
    }
}

/// Every runner this companion started, stopped (it is stopping).
pub fn stop_all(c: &Companion) {
    for st in c.providers.lock().unwrap().values_mut() {
        stop_runner(st);
    }
}
