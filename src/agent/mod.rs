//! In-tab AI agent streaming transport (M1).

pub mod accum;
pub mod stream;

pub use accum::{ToolCallAccum, TurnAccumulator};
pub use stream::{
    stream_completion, ChatMessage, ChatRequest, SseEvent, SseParser, StreamError,
    DEEPSEEK_API_URL,
};

/// localStorage key for the DeepSeek API key. Deliberately outside the VFS —
/// no `kernelosv2_file:` prefix, no FileSystem path.
pub const API_KEY_STORAGE_KEY: &str = "kernelosv2_deepseek_api_key";

#[cfg(target_arch = "wasm32")]
pub fn load_api_key() -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(API_KEY_STORAGE_KEY)
        .ok()?
        .filter(|k| !k.is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_api_key() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn save_api_key(key: &str) -> Result<(), String> {
    web_sys::window()
        .ok_or_else(|| "no window".to_string())?
        .local_storage()
        .map_err(|e| format!("{e:?}"))?
        .ok_or_else(|| "localStorage unavailable".to_string())?
        .set_item(API_KEY_STORAGE_KEY, key)
        .map_err(|e| format!("{e:?}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_api_key(_key: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::API_KEY_STORAGE_KEY;
    use crate::plugin::imports::allow_vfs_path;

    /// VFS file bodies are stored at `kernelosv2_file:<path>`. The API key uses
    /// a separate top-level localStorage key with no VFS involvement.
    const VFS_CONTENT_PREFIX: &str = "kernelosv2_file:";

    #[test]
    fn api_key_not_reachable_via_vfs_grants() {
        assert!(
            !API_KEY_STORAGE_KEY.starts_with(VFS_CONTENT_PREFIX),
            "API key storage key must not use the VFS content prefix"
        );

        let grant_prefixes = [
            "/",
            "/home",
            "/home/documents",
            "/system",
            "/system/config",
            "/home/documents/",
            "/system/config/",
        ];

        let probe_paths = [
            "/kernelosv2_deepseek_api_key",
            "/system/config/kernelosv2_deepseek_api_key",
            "/system/kernelosv2_deepseek_api_key",
            "/kernelosv2_file:kernelosv2_deepseek_api_key",
            API_KEY_STORAGE_KEY,
        ];

        for prefix in grant_prefixes {
            for path in probe_paths {
                if let Some(normalized) = allow_vfs_path(prefix, path) {
                    let vfs_storage_key = format!("{VFS_CONTENT_PREFIX}{normalized}");
                    assert_ne!(
                        vfs_storage_key, API_KEY_STORAGE_KEY,
                        "grant {prefix:?} + path {path:?} would collide with API key storage"
                    );
                }
            }
        }
    }
}
