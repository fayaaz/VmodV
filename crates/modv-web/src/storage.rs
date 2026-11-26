//! `localStorage` persistence for the browser shell.
//!
//! The engine state and the Scene library are serialised by
//! [`crate::state::WebState::to_persisted_json`] and stored as one JSON string.
//! Browser storage is best-effort: private modes and quota errors simply skip
//! the save, and the shell keeps working for the session.

use serde_json::Value;

/// Storage key. The version lives inside the document so a future format can
/// ignore older saves without a key migration.
const KEY: &str = "vmodv-web-state";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// Load the last saved document, if any.
pub fn load() -> Option<Value> {
    let raw = storage()?.get_item(KEY).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

/// Save a document. Returns `true` when the write succeeded.
pub fn save(value: &Value) -> bool {
    let Some(storage) = storage() else {
        return false;
    };
    let Ok(raw) = serde_json::to_string(value) else {
        return false;
    };
    storage.set_item(KEY, &raw).is_ok()
}
