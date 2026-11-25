//! HTTP to model servers for a native host: the blocking client a turn
//! sends its requests with (`dry_core::runner::Transport` is made from
//! it by the host). The same calls as the window's (`crates/gpui/src/
//! net.rs`): a POST with no timeout, so a slow local model is never cut
//! off, and a short GET.
//!
//! Keys are read from the environment variable an `OpenAi` names
//! (`key_env`), at the moment of the call; nothing is kept.
//!
//! A `cli:` URL is a CLI model (`dry_core::cli`): `post`, `stream` and
//! `get` run its program instead (`crate::cli`).

use std::time::Duration;

use dry_core::inference::Request;

/// `Authorization: Bearer <key>` from the named environment variable.
fn bearer(key_env: &Option<String>) -> Result<Option<String>, String> {
    match key_env {
        None => Ok(None),
        Some(var) => std::env::var(var).map(|key| Some(format!("Bearer {key}"))).map_err(|_| format!("environment variable {var} is not set")),
    }
}

/// No timeout on a model call: a local model may load, queue and think
/// for as long as it needs (owner, 2026-11-24). A server that is not
/// there refuses the connection at once.
///
/// POST the request's JSON body; the response body, whatever the status
/// (an API error body is still read, so its message can be shown).
pub fn post(req: &Request) -> Result<String, String> {
    if dry_core::cli::is_cli(&req.url) {
        return crate::cli::post(req);
    }
    let mut call = ureq::post(&req.url).header("Content-Type", "application/json");
    if let Some(auth) = bearer(&req.key_env)? {
        call = call.header("Authorization", &auth);
    }
    let mut resp = call
        .config()
        .http_status_as_error(false)
        .build()
        .send(req.body.as_str())
        .map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// POST asking to stream (`req` from `dry_core::stream::streaming`): the
/// body read as it comes, each delta to `on`; the body a plain post would
/// have given (`Sse::finish`). A server that does not stream answers with
/// a plain body, kept as it is; one that refuses `stream` with an error is
/// asked again without it.
pub fn stream(req: &Request, on: &mut dyn FnMut(dry_core::stream::Delta)) -> Result<String, String> {
    use std::io::Read;
    if dry_core::cli::is_cli(&req.url) {
        return crate::cli::stream(req, on);
    }
    let mut call = ureq::post(&req.url).header("Content-Type", "application/json").header("Accept", "text/event-stream");
    if let Some(auth) = bearer(&req.key_env)? {
        call = call.header("Authorization", &auth);
    }
    let mut resp = call
        .config()
        .http_status_as_error(false)
        .build()
        .send(req.body.as_str())
        .map_err(|e| e.to_string())?;
    let mut sse = dry_core::stream::Sse::new(req);
    let mut reader = resp.body_mut().as_reader();
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        for d in sse.push(&buf[..n]) {
            on(d);
        }
    }
    let streamed = sse.streamed();
    let body = sse.finish();
    if dry_core::stream::retry_plain(streamed, &body) {
        return post(&dry_core::stream::plain(req));
    }
    Ok(body)
}

/// GET `url`; the response body.
pub fn get(url: &str, key_env: &Option<String>) -> Result<String, String> {
    if dry_core::cli::is_cli(url) {
        return crate::cli::get(url);
    }
    let mut call = ureq::get(url);
    if let Some(auth) = bearer(key_env)? {
        call = call.header("Authorization", &auth);
    }
    let mut resp =
        call.config().http_status_as_error(false).timeout_global(Some(Duration::from_secs(10))).build().call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// The native transport: `post` and `get` above, for `dry_core::runner`.
/// Its futures are ready at once (the call blocks), so a host drives a
/// turn on its own thread with `dry_core::store::ready`.
pub struct Native;

impl dry_core::runner::Transport for Native {
    async fn post(&self, req: &Request) -> Result<String, String> {
        post(req)
    }
    async fn get(&self, url: &str, key_env: &Option<String>) -> Result<String, String> {
        get(url, key_env)
    }
    async fn stream(&self, req: &Request, on: &mut dyn FnMut(dry_core::stream::Delta)) -> Result<String, String> {
        stream(req, on)
    }
}

#[cfg(test)]
mod tests {
    use dry_core::inference::{Inference, Message, OpenAi, Role};
    use dry_core::stream::{Delta, Reader};
    use std::time::Instant;

    /// The loaded model of the local server (`DRY_TEST_BASE`, default LM
    /// Studio's), or `DRY_TEST_MODEL`.
    fn model() -> OpenAi {
        let base = std::env::var("DRY_TEST_BASE").unwrap_or_else(|_| "http://localhost:1234/v1".into());
        let model = std::env::var("DRY_TEST_MODEL").ok().unwrap_or_else(|| {
            let root = base.trim_end_matches("/v1");
            let body = super::get(&format!("{root}/api/v0/models"), &None).expect("LM Studio answers");
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            let loaded = v["data"].as_array().unwrap().iter().find(|m| m["state"] == "loaded" && m["type"] != "embeddings");
            loaded.and_then(|m| m["id"].as_str()).expect("a loaded model").to_string()
        });
        OpenAi { base_url: base, model, ..OpenAi::default() }
    }

    /// A reply streamed from the real model with the real prompt: when the
    /// first text came, when the component was known (its kind closed),
    /// when the answer ended. The streamed body reads to the same text as
    /// the deltas, and the view's final reply is what the full parser reads.
    /// `cargo test -p dry-tools -- --ignored --nocapture streams_a_reply`
    #[test]
    #[ignore]
    fn streams_a_reply_and_the_component_comes_before_the_end() {
        let tasks = [
            ("choice", "Ask me which one database to use: Postgres, SQLite or MySQL."),
            ("table", "Show me these users as a table: Ada, 36, London; Bob, 41, Paris; Cy, 29, Rome."),
            ("form", "Book a table for me. Ask me the day, how many guests, and inside or terrace, all in one go."),
            ("table+choice", "Open tickets: #12 login bug (high), #15 typo (low), #19 crash (high). Show them in a table and ask me which to fix first."),
        ];
        let ai = model();
        let mut lines = Vec::new();
        for (name, task) in tasks {
            let mut e = dry_core::state::Engine::new("test", "You are a helpful assistant.", ai.clone());
            (e.response, e.format) = crate::read_responses().expect("responses.md");
            e.messages.push(Message { role: Role::User, text: task.to_string() });
            let req = dry_core::stream::streaming(&e.inference.request(&dry_core::prompt::render(&e).messages()));
            let mut reader = Reader::new(&e);
            let start = Instant::now();
            let (mut first, mut component) = (None, None);
            let body = super::stream(&req, &mut |d: Delta| {
                first.get_or_insert_with(|| start.elapsed());
                reader.feed(&d);
                if reader.view.has_component() {
                    component.get_or_insert_with(|| start.elapsed());
                }
            })
            .expect("the server answers");
            let total = start.elapsed();
            reader.finish();
            let text = e.inference.reply(&body).unwrap_or_else(|err| panic!("{err:?}: {body}"));
            assert_eq!(text, reader.text, "the body is the deltas");
            let full = dry_core::ask::reply_call(&e, &text).map(|c| dry_core::ask::Parsed::of(&c));
            if let (Some(p), Some(r)) = (&full, &reader.view.reply) {
                assert_eq!((&p.ask, &p.show), (&r.ask, &r.show), "the view's final reply is the full parser's");
            }
            let ms = |d: Option<std::time::Duration>| d.map_or("-".to_string(), |d| format!("{} ms", d.as_millis()));
            let line = format!(
                "{name}: first text {}, component {}, whole answer {} ms ({} chars); {}",
                ms(first),
                ms(component),
                total.as_millis(),
                text.chars().count(),
                dry_core::ask::reply_call(&e, &text).map_or("no reply".to_string(), |c| c.to_string())
            );
            println!("{line}");
            lines.push(line);
        }
        println!("=== {}", lines.join("\n=== "));
    }
}
