//! Loading and saving: the server's `/api/document` first, browser storage as a fallback.

use std::cell::Cell;
use std::time::Duration;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

use vellum_schema::{Document, Drawing};

use crate::store::Store;

const DOC_KEY: &str = "vellum.document";
const DRAWING_KEY: &str = "vellum.drawing";
const API: &str = "/api/document";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn read_local<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    serde_json::from_str(&storage()?.get_item(key).ok()??).ok()
}

fn write_local<T: serde::Serialize>(key: &str, value: &T) {
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(value)) {
        let _ = s.set_item(key, &json);
    }
}

pub fn load_drawing() -> Drawing {
    read_local(DRAWING_KEY).unwrap_or_default()
}

pub fn save_drawing(drawing: &Drawing) {
    write_local(DRAWING_KEY, drawing);
}

async fn fetch_document() -> Result<Option<Document>, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let resp: Response = JsFuture::from(window.fetch_with_str(API))
        .await?
        .dyn_into()?;
    if resp.status() == 404 {
        return Ok(None);
    }
    if !resp.ok() {
        return Err(format!("server answered {}", resp.status()).into());
    }
    let text = JsFuture::from(resp.text()?)
        .await?
        .as_string()
        .unwrap_or_default();
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| e.to_string().into())
}

async fn put_document(doc: &Document) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let init = RequestInit::new();
    init.set_method("PUT");
    init.set_body(&JsValue::from_str(
        &serde_json::to_string(doc).map_err(|e| e.to_string())?,
    ));
    let req = Request::new_with_str_and_init(API, &init)?;
    req.headers().set("content-type", "application/json")?;
    let resp: Response = JsFuture::from(window.fetch_with_request(&req))
        .await?
        .dyn_into()?;
    if resp.ok() {
        Ok(())
    } else {
        Err(format!("server answered {}", resp.status()).into())
    }
}

/// Loads the project document, falling back to browser storage, then to `starter`.
pub fn load(store: Store, starter: impl FnOnce() -> Document + 'static) {
    spawn_local(async move {
        let doc = match fetch_document().await {
            Ok(Some(doc)) => {
                store.save_status.set("Saved");
                doc
            }
            Ok(None) => {
                store.save_status.set("New project");
                read_local(DOC_KEY).unwrap_or_else(starter)
            }
            Err(_) => {
                store.save_status.set("Offline: saving in this browser");
                read_local(DOC_KEY).unwrap_or_else(starter)
            }
        };
        store.doc.set(doc);
        store.loaded.set(true);
    });
}

thread_local! {
    static SAVE_GENERATION: Cell<u32> = const { Cell::new(0) };
}

/// Saves shortly after the last change, so a drag or a typing burst becomes one write.
pub fn schedule_save(store: Store, doc: Document) {
    write_local(DOC_KEY, &doc);
    let generation = SAVE_GENERATION.with(|g| {
        g.set(g.get().wrapping_add(1));
        g.get()
    });
    store.save_status.set("Saving…");
    set_timeout(
        move || {
            if SAVE_GENERATION.with(|g| g.get()) != generation {
                return;
            }
            spawn_local(async move {
                let status = match put_document(&doc).await {
                    Ok(()) => "Saved",
                    Err(_) => "Offline: saving in this browser",
                };
                store.save_status.set(status);
            });
        },
        Duration::from_millis(400),
    );
}
