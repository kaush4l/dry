//! The browser's stores, in a real browser: what a save writes, a fresh
//! open reads back (as after a reload), only changes are written, and a
//! removed engine leaves nothing.
//!
//! Run: `cd crates/web && NO_HEADLESS=1 cargo test --target
//! wasm32-unknown-unknown --test idb`, then open the address it prints
//! (or with a matching chromedriver/geckodriver, without NO_HEADLESS).
#![cfg(target_arch = "wasm32")]

use dry_core::store::{self, Changes, TextStore, Texts};
use dry_web::idb::{Backend, Idb, Local};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

async fn round_trip<S: TextStore>(s: &S) {
    // Start clean.
    let all = s.read_all().await.unwrap();
    s.write(&Changes { put: Texts::new(), remove: all.into_keys().collect() }).await.unwrap();
    let (state, mut saver) = store::load(s).await.unwrap();
    assert!(state.is_none(), "nothing stored yet");

    let sample = dry_core::files::sample();
    let mut local = Texts::new();
    local.insert("local/settings.json".into(), "{\"model\":\"m\"}".into());
    let n = store::save(s, &mut saver, &sample, &local).await.unwrap();
    assert!(n > 5);
    assert_eq!(store::save(s, &mut saver, &sample, &local).await.unwrap(), 0, "nothing changed, nothing written");

    let (back, saver2) = store::load(s).await.unwrap();
    assert_eq!(back.as_ref(), Some(&sample), "a fresh load (a reload) gives the same state");
    assert_eq!(saver2.last.get("local/settings.json").map(String::as_str), Some("{\"model\":\"m\"}"));

    let mut less = sample.clone();
    let gone = less.engines.remove(1).id;
    store::save(s, &mut saver, &less, &local).await.unwrap();
    let texts = s.read_all().await.unwrap();
    assert!(!texts.keys().any(|k| k.starts_with(&format!("engines/{gone}/"))));
    assert_eq!(store::from_texts(&texts).unwrap(), less);
}

#[wasm_bindgen_test]
async fn indexeddb_keeps_the_state() {
    round_trip(&Idb::open().await.unwrap()).await;
}

#[wasm_bindgen_test]
async fn localstorage_keeps_the_state() {
    round_trip(&Local::open().unwrap()).await;
}

#[wasm_bindgen_test]
async fn the_backend_opens_indexeddb_first() {
    let (b, note) = Backend::open().await;
    assert_eq!(b.describe(), "indexeddb:dry");
    assert!(note.is_none());
}
