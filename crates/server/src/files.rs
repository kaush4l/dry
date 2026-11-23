//! The intents that change the app's own files rather than the store, and
//! the scene's assets. Each was a `HOST_LATER` control on the host's pages
//! (docs/web.md, "The page"); now a page sends it here and the host makes
//! the write, the same one the desktop app makes (`dry_tools`), one at a
//! time (`FILES`), then bumps the generation so every open page redraws
//! at once (the watcher sees the file too, a moment later).
//!
//! ```text
//! POST /api/theme                 {value}                        the active theme (themes/active)   → 200 {gen}
//! POST /api/theme/reset           {}                             every token of it back to its default → 200 {gen}
//! POST /api/routes/hide           {value: pattern}               hide or show its navigation button (state/local.md)
//! POST /api/routes/off            {value: pattern}               turn a standard route off or on (state/local.md)
//! POST /api/routes/delete         {value: pattern}               delete a route's page.json
//! POST /api/routes/create         {path, title, icon?}           a new page → 200 {gen, pattern}
//! POST /api/models/:name/default  {}                             the default model setting (models.json)
//! POST /api/engines/:id/reload    {what: responses|spec, base}   the engine's contract again (the store)
//! POST /api/terminal              {line}                         a line typed into the Code page's terminal
//! GET  /assets/scene.js, scene.wasm                              the scene behind the glass (crates/scene)
//! GET  /assets/scenes/<name>.md                                  a scene file from scenes/
//! ```

use std::sync::Mutex;

use actix_web::{HttpRequest, HttpResponse, web};
use serde_json::{Value, json};

use crate::{Data, error, intent, let_in, refused, reply};

/// One writer of the app's files at a time.
static FILES: Mutex<()> = Mutex::new(());

/// The scene module (scripts/scene-wasm.sh builds it from crates/scene).
const SCENE_WASM: &[u8] = include_bytes!("../assets/scene.wasm");

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/assets/scene.js", web::get().to(scene_js))
        .route("/assets/scene.wasm", web::get().to(scene_wasm))
        .route("/assets/scenes/{file}", web::get().to(scene_file))
        .route("/api/theme", web::post().to(theme))
        .route("/api/theme/reset", web::post().to(theme_reset))
        .route("/api/routes/{what}", web::post().to(route))
        .route("/api/models/{name}/default", web::post().to(model_default))
        .route("/api/engines/{id}/reload", web::post().to(reload))
        .route("/api/terminal", web::post().to(terminal));
}

fn word(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().trim().to_string()
}

/// Make a file write as the host, then tell every page.
async fn write(req: HttpRequest, s: Data, body: web::Bytes, f: impl FnOnce(&Value) -> Result<Value, String> + Send + 'static) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let done = web::block(move || {
        let _one = FILES.lock().unwrap_or_else(|e| e.into_inner());
        f(&v)
    })
    .await;
    match done {
        Ok(Ok(mut out)) => {
            out["gen"] = json!(host.live.bump(&["files"]));
            reply(200, out)
        }
        Ok(Err(why)) => error(400, why),
        Err(e) => error(500, e.to_string()),
    }
}

async fn theme(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    write(req, s, body, |v| dry_tools::set_active_theme(&word(v, "value")).map(|_| json!({}))).await
}

async fn theme_reset(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    write(req, s, body, |_| {
        let mut theme = dry_tools::read_theme()?;
        theme.reset(None);
        dry_tools::save_theme(&theme).map(|_| json!({}))
    })
    .await
}

async fn route(req: HttpRequest, s: Data, what: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let what = what.into_inner();
    write(req, s, body, move |v| {
        let pattern = word(v, "value");
        let local = || {
            let mut view = dry_core::routes::ViewState::default();
            view.read_local_md(&dry_tools::read_local_md());
            view
        };
        match what.as_str() {
            "hide" => {
                let mut view = local();
                view.toggle_hidden(&pattern);
                dry_tools::save_local_routes(&view).map(|_| json!({ "pattern": pattern }))
            }
            "off" => {
                let mut view = local();
                view.toggle_removed(&pattern)?;
                dry_tools::save_local_routes(&view).map(|_| json!({ "pattern": pattern }))
            }
            "delete" => dry_tools::delete_page(&pattern).map(|p| json!({ "pattern": p })),
            "create" => {
                let path = word(v, "path");
                if path.is_empty() {
                    return Err("a page needs a path, like /notes".into());
                }
                dry_tools::create_page(&path, &word(v, "title"), &word(v, "icon")).map(|p| json!({ "pattern": p }))
            }
            other => Err(format!("no route action {other}: hide, off, delete or create")),
        }
    })
    .await
}

async fn model_default(req: HttpRequest, s: Data, name: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let name = name.into_inner();
    write(req, s, body, move |_| {
        let mut models = dry_tools::settings::read_models()?;
        if !models.set_default(&name) {
            return Err(format!("no model setting {name} in models.json"));
        }
        dry_tools::settings::save_models(&models).map(|_| json!({ "default": name }))
    })
    .await
}

async fn reload(req: HttpRequest, s: Data, id: web::Path<String>, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let target = id.into_inner();
    let what = word(&v, "what");
    let base = v.get("base").and_then(Value::as_u64);
    match web::block(move || host.reload(&target, &what, base)).await {
        Ok(Ok(g)) => reply(200, json!({ "gen": g })),
        Ok(Err(r)) => refused(r, &s.live),
        Err(e) => error(500, e.to_string()),
    }
}

async fn terminal(req: HttpRequest, s: Data, body: web::Bytes) -> HttpResponse {
    let (host, v) = match intent(&req, &s, &body) {
        Ok(x) => x,
        Err(no) => return no.response(),
    };
    let line = v.get("line").and_then(Value::as_str).unwrap_or_default().to_string();
    match host.run_typed(&line) {
        Ok(g) => reply(200, json!({ "gen": g })),
        Err(r) => refused(r, &s.live),
    }
}

// ---- the scene ----

async fn scene_js() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/javascript; charset=utf-8")
        .insert_header(("Cache-Control", "no-cache"))
        .body(include_str!("../assets/scene.js"))
}

async fn scene_wasm() -> HttpResponse {
    HttpResponse::Ok().content_type("application/wasm").insert_header(("Cache-Control", "no-cache")).body(SCENE_WASM)
}

/// `scenes/<name>.md`, only for a scene name (no path).
async fn scene_file(req: HttpRequest, s: Data, file: web::Path<String>) -> HttpResponse {
    if let Err(no) = let_in(&req, &s) {
        return no.response();
    }
    let file = file.into_inner();
    let Some(name) = file.strip_suffix(".md").filter(|n| dry_core::theme::valid_scene_name(n)) else {
        return HttpResponse::NotFound().finish();
    };
    match std::fs::read_to_string(dry_tools::scenes_dir().join(format!("{name}.md"))) {
        Ok(text) => HttpResponse::Ok().content_type("text/markdown; charset=utf-8").insert_header(("Cache-Control", "no-cache")).body(text),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `assets/scene.wasm` is built from the scene code as it is now; run
    /// scripts/scene-wasm.sh after changing crates/core/src/scene or crates/scene.
    #[test]
    fn the_scene_module_is_built_from_the_scene_code_as_it_is() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let want = format!("dry-scene-src:{}", dry_scene::source_hash(&here.join("../core/src/scene"), &here.join("../scene/src/lib.rs")));
        let found = SCENE_WASM.windows(want.len()).any(|w| w == want.as_bytes());
        assert!(found, "crates/server/assets/scene.wasm is stale: run scripts/scene-wasm.sh");
    }
}
