//! In-tab AI agent: streaming transport (M1), VFS tools (M2), multi-turn loop (M3),
//! copy-on-write journal / undo (M4), TypeScript typecheck (M5a), Python (M5b),
//! named restore points (M7a), user-directed forks (M7b).

pub mod accum;
pub mod branch;
pub mod journal;
pub mod python;
pub mod restore;
pub mod roundtrip;
pub mod salvage;
pub mod stream;
pub mod tools;
pub mod typecheck;

pub use accum::{ToolCallAccum, TurnAccumulator, UsageAccum};
pub use branch::{
    diff_against_trunk, promote_all, promote_path, prompt_branch_name, Branch, BranchDiff,
    WorkspaceId,
};
pub use journal::{FileDelta, Journal, PathState, RECURSIVE_DELETE_GATE_THRESHOLD};
pub use python::ensure_python_loaded;
pub use restore::{
    prompt_restore_name, RestorePoint, RestorePointStore, MAX_RESTORE_POINTS,
    RESTORE_POINTS_STORAGE_KEY,
};
pub use roundtrip::{
    run_agent_loop, tool_round_trip, LoopConfig, LoopEvent, LoopOutcome, LoopStopReason,
    TranscriptTurn, ToolInvocation, DEFAULT_MAX_ITERATIONS, REPETITION_LIMIT,
};
pub use salvage::salvage_tool_calls;
pub use stream::{
    stream_completion, AssistantFunctionCall, AssistantToolCall, ChatMessage, ChatRequest,
    SseEvent, SseParser, StreamError, ThinkingConfig, DEEPSEEK_API_URL, DEEPSEEK_BETA_API_URL,
    DEEPSEEK_MODEL,
};
pub use tools::{
    execute_tool, tool_definitions, truncate_result, MAX_TOOL_RESULT_BYTES, TOOL_NAMES,
};
pub use typecheck::ensure_typescript_loaded;

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
    use crate::agent::RESTORE_POINTS_STORAGE_KEY;
    use crate::filesystem::content_key;
    use crate::plugin::imports::allow_vfs_path;

    /// VFS file bodies are stored at `kernelosv2_file:<path>`. The API key uses
    /// a separate top-level localStorage key with no VFS involvement.
    #[test]
    fn api_key_not_reachable_via_vfs_grants() {
        // content_key("") yields the storage prefix; the key must not share it.
        assert!(
            !API_KEY_STORAGE_KEY.starts_with(&content_key("")),
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
                    let vfs_storage_key = content_key(&normalized);
                    assert_ne!(
                        vfs_storage_key, API_KEY_STORAGE_KEY,
                        "grant {prefix:?} + path {path:?} would collide with API key storage"
                    );
                }
            }
        }
    }

    #[test]
    fn restore_points_key_outside_vfs_content_prefix() {
        assert!(
            !RESTORE_POINTS_STORAGE_KEY.starts_with(&content_key("")),
            "restore points must not use the VFS content prefix"
        );
        assert_ne!(RESTORE_POINTS_STORAGE_KEY, API_KEY_STORAGE_KEY);
    }
}
