//! The model server, reached with `fetch` (`dry_core::runner::Transport`).
//! JSON requests are not "simple" requests, so the browser sends a CORS
//! preflight first; the server must answer it (LM Studio: Enable CORS).
//! A key, if this browser has one, goes in `Authorization`.

use dry_core::inference::Request;
use dry_core::runner::Transport;
use dry_core::stream::{Delta, Sse};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, RequestInit, RequestMode, Response};

pub struct Fetch {
    /// `Bearer` key for a cloud server; `None` for LM Studio.
    pub key: Option<String>,
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

impl Fetch {
    /// The response to `method url`, once its headers are in.
    async fn response(&self, method: &str, url: &str, body: Option<&str>, accept: Option<&str>) -> Result<Response, String> {
        let init = RequestInit::new();
        init.set_method(method);
        init.set_mode(RequestMode::Cors);
        let headers = Headers::new().map_err(js_err)?;
        if let Some(b) = body {
            headers.set("Content-Type", "application/json").map_err(js_err)?;
            init.set_body(&JsValue::from_str(b));
        }
        if let Some(a) = accept {
            headers.set("Accept", a).map_err(js_err)?;
        }
        // To a paired companion: its session token (memory only), never this browser's key.
        if let Some(k) = crate::companion::token_for(url).or_else(|| self.key.clone()) {
            headers.set("Authorization", &format!("Bearer {k}")).map_err(js_err)?;
        }
        init.set_headers(&headers);
        let req = web_sys::Request::new_with_str_and_init(url, &init).map_err(js_err)?;
        let window = web_sys::window().ok_or("no window")?;
        let resp = JsFuture::from(window.fetch_with_request(&req)).await.map_err(|e| {
            format!(
                "could not reach {url} ({}). Is the server running, with CORS enabled? (LM Studio: Developer → Settings → Enable CORS; see docs/browser.md)",
                js_err(e)
            )
        })?;
        resp.dyn_into().map_err(js_err)
    }

    /// A body read whole; a failed status with no error object says so.
    async fn text(url: &str, resp: &Response) -> Result<String, String> {
        let text = JsFuture::from(resp.text().map_err(js_err)?).await.map_err(js_err)?.as_string().unwrap_or_default();
        if !resp.ok() {
            // The body says why (an error object); the reader shows it.
            if text.trim_start().starts_with('{') {
                return Ok(text);
            }
            return Err(format!("{url} answered {} {}", resp.status(), text.chars().take(300).collect::<String>()));
        }
        Ok(text)
    }

    async fn send(&self, method: &str, url: &str, body: Option<&str>) -> Result<String, String> {
        let resp = self.response(method, url, body, None).await?;
        Fetch::text(url, &resp).await
    }

    /// The streamed body read as it comes (`ReadableStream`), each delta
    /// to `on`; what a plain post would have answered (`Sse::finish`).
    async fn read_stream(&self, req: &Request, on: &mut dyn FnMut(Delta)) -> Result<String, String> {
        let resp = self.response("POST", &req.url, Some(&req.body), Some("text/event-stream")).await?;
        let Some(body) = resp.body().filter(|_| resp.ok()) else { return Fetch::text(&req.url, &resp).await };
        let reader: web_sys::ReadableStreamDefaultReader = body.get_reader().unchecked_into();
        let mut sse = Sse::new(req);
        loop {
            let chunk = JsFuture::from(reader.read()).await.map_err(js_err)?;
            if js_sys::Reflect::get(&chunk, &"done".into()).ok().and_then(|d| d.as_bool()).unwrap_or(true) {
                break;
            }
            let value = js_sys::Reflect::get(&chunk, &"value".into()).map_err(js_err)?;
            for d in sse.push(&js_sys::Uint8Array::new(&value).to_vec()) {
                on(d);
            }
        }
        let streamed = sse.streamed();
        let body = sse.finish();
        if dry_core::stream::retry_plain(streamed, &body) {
            return self.post(&dry_core::stream::plain(req)).await;
        }
        Ok(body)
    }
}

/// A CLI model (`dry_core::cli`) runs a program, which a page cannot
/// start: a host (`dry-server`, the desktop app) runs it.
fn no_cli(url: &str) -> Result<(), String> {
    if dry_core::cli::is_cli(url) {
        return Err(format!(
            "{url} is a CLI model: it runs a program on a computer, which a browser page cannot start; run this engine on the host (dry-server) or the desktop app"
        ));
    }
    Ok(())
}

impl Transport for Fetch {
    async fn post(&self, req: &Request) -> Result<String, String> {
        no_cli(&req.url)?;
        self.send("POST", &req.url, Some(&req.body)).await
    }
    async fn get(&self, url: &str, _: &Option<String>) -> Result<String, String> {
        no_cli(url)?;
        self.send("GET", url, None).await
    }
    async fn stream(&self, req: &Request, on: &mut dyn FnMut(Delta)) -> Result<String, String> {
        no_cli(&req.url)?;
        self.read_stream(req, on).await
    }
}
