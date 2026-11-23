//! The browser's stores (`dry_core::store::TextStore`): the state's texts
//! by path, kept across reloads.
//!
//! - `Idb`: IndexedDB, database `dry`, object store `texts` (key: the
//!   path, value: the text). Each save is one transaction of puts and
//!   deletes, so a reload never sees half a save.
//! - `Local`: localStorage, one item per path under `dry:`. Smaller (a few
//!   MB) and blocking, used when IndexedDB is not there (some private
//!   modes, old browsers).
//! - `Memory` (the core's): nothing kept, when neither works.
//!
//! `Backend` picks the first that opens. Another place (OPFS, a remote
//! host over HTTP) is one more `TextStore` and one more arm here.

use dry_core::store::{Changes, Memory, TextStore, Texts};
use js_sys::{Array, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbRequest, IdbTransactionMode};

const DB: &str = "dry";
/// The offline copy of a companion's workspace state (docs/companion-protocol.md, "The workspace's state").
pub const WORKSPACE_DB: &str = "dry-workspace";
/// Where Reset history keeps a copy of what it cleared (`<dry.reset-<stamp>>/<path>`).
pub const RESET_DB: &str = "dry-reset";
const STORE: &str = "texts";
const PREFIX: &str = "dry:";

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{e:?}"))
}

/// A request's result, once it succeeds.
async fn done(req: &IdbRequest) -> Result<JsValue, String> {
    let r = req.clone();
    let p = Promise::new(&mut |resolve, reject| {
        let ok = r.clone();
        let on_ok = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = resolve.call1(&JsValue::NULL, &ok.result().unwrap_or(JsValue::UNDEFINED));
        });
        let bad = r.clone();
        let on_err = Closure::once_into_js(move |_: web_sys::Event| {
            let why = bad.error().ok().flatten().map(|e| e.message()).unwrap_or_else(|| "IndexedDB request failed".into());
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&why));
        });
        r.set_onsuccess(Some(on_ok.unchecked_ref()));
        r.set_onerror(Some(on_err.unchecked_ref()));
    });
    JsFuture::from(p).await.map_err(js_err)
}

/// IndexedDB.
pub struct Idb {
    db: IdbDatabase,
    name: &'static str,
}

impl Idb {
    pub async fn open() -> Result<Idb, String> {
        Idb::open_named(DB).await
    }

    /// Another database of the same shape (the workspace's offline copy).
    pub async fn open_named(name: &'static str) -> Result<Idb, String> {
        let factory = web_sys::window().ok_or("no window")?.indexed_db().map_err(js_err)?.ok_or("no IndexedDB")?;
        let req = factory.open_with_u32(name, 1).map_err(js_err)?;
        let upgrade = Closure::<dyn FnMut(web_sys::IdbVersionChangeEvent)>::new(move |e: web_sys::IdbVersionChangeEvent| {
            let Some(target) = e.target() else { return };
            let Ok(r) = target.dyn_into::<IdbRequest>() else { return };
            if let Ok(db) = r.result().and_then(|d| d.dyn_into::<IdbDatabase>())
                && !db.object_store_names().contains(STORE)
            {
                let _ = db.create_object_store(STORE);
            }
        });
        req.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
        let db = done(&req).await?;
        drop(upgrade);
        Ok(Idb { db: db.dyn_into().map_err(js_err)?, name })
    }
}

impl TextStore for Idb {
    fn describe(&self) -> String {
        format!("indexeddb:{}", self.name)
    }

    async fn read_all(&self) -> Result<Texts, String> {
        let tx = self.db.transaction_with_str(STORE).map_err(js_err)?;
        let store = tx.object_store(STORE).map_err(js_err)?;
        let keys: Array = done(&store.get_all_keys().map_err(js_err)?).await?.unchecked_into();
        let values: Array = done(&store.get_all().map_err(js_err)?).await?.unchecked_into();
        let mut out = Texts::new();
        for (k, v) in keys.iter().zip(values.iter()) {
            if let (Some(k), Some(v)) = (k.as_string(), v.as_string()) {
                out.insert(k, v);
            }
        }
        Ok(out)
    }

    async fn write(&self, changes: &Changes) -> Result<(), String> {
        let tx = self.db.transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite).map_err(js_err)?;
        let store = tx.object_store(STORE).map_err(js_err)?;
        for (k, v) in &changes.put {
            store.put_with_key(&JsValue::from_str(v), &JsValue::from_str(k)).map_err(js_err)?;
        }
        for k in &changes.remove {
            store.delete(&JsValue::from_str(k)).map_err(js_err)?;
        }
        // Done when the whole transaction is.
        let t = tx.clone();
        let p = Promise::new(&mut |resolve, reject| {
            let on_ok = Closure::once_into_js(move |_: web_sys::Event| {
                let _ = resolve.call0(&JsValue::NULL);
            });
            let on_err = Closure::once_into_js(move |_: web_sys::Event| {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("the IndexedDB save failed"));
            });
            t.set_oncomplete(Some(on_ok.unchecked_ref()));
            t.set_onerror(Some(on_err.unchecked_ref()));
        });
        JsFuture::from(p).await.map(|_| ()).map_err(js_err)
    }
}

/// localStorage.
pub struct Local {
    storage: web_sys::Storage,
}

impl Local {
    pub fn open() -> Result<Local, String> {
        let storage = web_sys::window().ok_or("no window")?.local_storage().map_err(js_err)?.ok_or("no localStorage")?;
        Ok(Local { storage })
    }
}

impl TextStore for Local {
    fn describe(&self) -> String {
        "localstorage".into()
    }

    async fn read_all(&self) -> Result<Texts, String> {
        let mut out = Texts::new();
        let n = self.storage.length().map_err(js_err)?;
        for i in 0..n {
            if let Ok(Some(k)) = self.storage.key(i)
                && let Some(path) = k.strip_prefix(PREFIX)
                && let Ok(Some(v)) = self.storage.get_item(&k)
            {
                out.insert(path.to_string(), v);
            }
        }
        Ok(out)
    }

    async fn write(&self, changes: &Changes) -> Result<(), String> {
        for (k, v) in &changes.put {
            self.storage.set_item(&format!("{PREFIX}{k}"), v).map_err(|e| format!("localStorage is full or off: {}", js_err(e)))?;
        }
        for k in &changes.remove {
            self.storage.remove_item(&format!("{PREFIX}{k}")).map_err(js_err)?;
        }
        Ok(())
    }
}

/// The store this browser has: IndexedDB, else localStorage, else memory.
pub enum Backend {
    Idb(Idb),
    Local(Local),
    Memory(Memory),
    /// The paired companion's store (docs/companion.md), when this tab
    /// chose it and still holds a pairing.
    Companion(crate::companion::Store),
}

impl Backend {
    /// The first that opens; why the better ones did not, for the footer.
    pub async fn open() -> (Backend, Option<String>) {
        let mut why_not = None;
        if let Some(mut c) = crate::companion::chosen_store() {
            // The workspace's offline copy: written with every save, read while the companion is away.
            c.cache = Idb::open_named(WORKSPACE_DB).await.ok();
            match c.check().await {
                Ok(()) => return (Backend::Companion(c), None),
                Err(why) if c.cache.is_some() => {
                    c.offline();
                    let note = format!(
                        "the companion is away ({why}): this browser's copy of the folder's state; pair again and your changes go back"
                    );
                    return (Backend::Companion(c), Some(note));
                }
                Err(why) => why_not = Some(format!("the companion's store is not reachable ({why}); using this browser's")),
            }
        }
        let (b, note) = Backend::open_here().await;
        (b, why_not.or(note))
    }

    /// The first of this browser's stores that opens.
    async fn open_here() -> (Backend, Option<String>) {
        match Idb::open().await {
            Ok(db) => (Backend::Idb(db), None),
            Err(why) => match Local::open() {
                Ok(l) => (Backend::Local(l), Some(format!("IndexedDB did not open ({why}); using localStorage"))),
                Err(why2) => {
                    (Backend::Memory(Memory::default()), Some(format!("no IndexedDB ({why}) or localStorage ({why2}): nothing is kept")))
                }
            },
        }
    }
}

impl TextStore for Backend {
    fn describe(&self) -> String {
        match self {
            Backend::Idb(s) => s.describe(),
            Backend::Local(s) => s.describe(),
            Backend::Memory(s) => TextStore::describe(s),
            Backend::Companion(s) => s.describe(),
        }
    }
    async fn read_all(&self) -> Result<Texts, String> {
        match self {
            Backend::Idb(s) => s.read_all().await,
            Backend::Local(s) => s.read_all().await,
            Backend::Memory(s) => s.read_all().await,
            Backend::Companion(s) => s.read_all().await,
        }
    }
    async fn write(&self, changes: &Changes) -> Result<(), String> {
        match self {
            Backend::Idb(s) => s.write(changes).await,
            Backend::Local(s) => s.write(changes).await,
            Backend::Memory(s) => s.write(changes).await,
            Backend::Companion(s) => s.write(changes).await,
        }
    }
}

/// Remove everything this page keeps (Settings → Reset), in every store.
pub async fn wipe(b: &Backend) -> Result<(), String> {
    let all = b.read_all().await?;
    b.write(&Changes { put: Texts::new(), remove: all.into_keys().collect() }).await
}
