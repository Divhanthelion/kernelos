//! Raw plugin WASM persistence.
//!
//! Manifests remain in the text VFS so they stay inspectable from Terminal.
//! Binary payloads live in IndexedDB instead of consuming the much smaller
//! localStorage quota as base64 strings.

#[cfg(target_arch = "wasm32")]
use js_sys::Uint8Array;
#[cfg(target_arch = "wasm32")]
use rexie::{ObjectStore, Rexie, TransactionMode, TransactionResult};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
const DB_NAME: &str = "kernelosv2_plugins";
#[cfg(target_arch = "wasm32")]
const STORE_NAME: &str = "wasm";
#[cfg(target_arch = "wasm32")]
const DB_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
async fn open() -> Result<Rexie, String> {
    Rexie::builder(DB_NAME)
        .version(DB_VERSION)
        .add_object_store(ObjectStore::new(STORE_NAME))
        .build()
        .await
        .map_err(|e| format!("open plugin database: {e}"))
}

/// Store or replace one raw WASM module.
#[cfg(target_arch = "wasm32")]
pub async fn put(id: &str, bytes: &[u8]) -> Result<(), String> {
    let db = open().await?;
    let transaction = db
        .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
        .map_err(|e| format!("start plugin database write: {e}"))?;
    let store = transaction
        .store(STORE_NAME)
        .map_err(|e| format!("open plugin byte store: {e}"))?;

    let key = JsValue::from_str(id);
    let array = Uint8Array::from(bytes);
    let value: JsValue = array.buffer().into();
    store
        .put(&value, Some(&key))
        .await
        .map_err(|e| format!("store plugin '{id}' bytes: {e}"))?;
    let result = transaction
        .done()
        .await
        .map_err(|e| format!("commit plugin '{id}' bytes: {e}"))?;
    if result != TransactionResult::Committed {
        return Err(format!("store plugin '{id}' bytes: transaction aborted"));
    }
    Ok(())
}

/// Load one raw WASM module.
#[cfg(target_arch = "wasm32")]
pub async fn get(id: &str) -> Result<Option<Vec<u8>>, String> {
    let db = open().await?;
    let transaction = db
        .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
        .map_err(|e| format!("start plugin database read: {e}"))?;
    let store = transaction
        .store(STORE_NAME)
        .map_err(|e| format!("open plugin byte store: {e}"))?;

    let value = store
        .get(JsValue::from_str(id))
        .await
        .map_err(|e| format!("load plugin '{id}' bytes: {e}"))?;
    let result = transaction
        .done()
        .await
        .map_err(|e| format!("finish plugin '{id}' read: {e}"))?;
    if result != TransactionResult::Committed {
        return Err(format!("load plugin '{id}' bytes: transaction aborted"));
    }

    Ok(value.map(|value| Uint8Array::new(&value).to_vec()))
}

/// Remove one raw WASM module.
#[cfg(target_arch = "wasm32")]
pub async fn delete(id: &str) -> Result<(), String> {
    let db = open().await?;
    let transaction = db
        .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
        .map_err(|e| format!("start plugin database delete: {e}"))?;
    let store = transaction
        .store(STORE_NAME)
        .map_err(|e| format!("open plugin byte store: {e}"))?;

    store
        .delete(JsValue::from_str(id))
        .await
        .map_err(|e| format!("delete plugin '{id}' bytes: {e}"))?;
    let result = transaction
        .done()
        .await
        .map_err(|e| format!("commit plugin '{id}' deletion: {e}"))?;
    if result != TransactionResult::Committed {
        return Err(format!("delete plugin '{id}' bytes: transaction aborted"));
    }
    Ok(())
}

// Native tests compile the browser host as an rlib. Keep a small in-memory
// implementation so async plugin code type-checks without IndexedDB.
#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static NATIVE_STORE: std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn put(id: &str, bytes: &[u8]) -> Result<(), String> {
    NATIVE_STORE.with(|store| {
        store.borrow_mut().insert(id.to_string(), bytes.to_vec());
    });
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn get(id: &str) -> Result<Option<Vec<u8>>, String> {
    Ok(NATIVE_STORE.with(|store| store.borrow().get(id).cloned()))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn delete(id: &str) -> Result<(), String> {
    NATIVE_STORE.with(|store| {
        store.borrow_mut().remove(id);
    });
    Ok(())
}
