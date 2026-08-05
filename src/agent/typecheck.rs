//! TypeScript typecheck bridge (PLAN M5a).
//!
//! The compiler is loaded asynchronously once (before `run_agent_loop`), then
//! `execute_tool` calls into it synchronously. Do not `.await` from the tool
//! path — that would break the single-loop design.

use crate::filesystem::FileSystem;
use serde_json::{Map, Value};
use std::cell::Cell;

thread_local! {
    /// Set after a successful `ensure_typescript_loaded`. Host builds never
    /// flip this — the stub always returns the unavailable error.
    static TS_READY: Cell<bool> = const { Cell::new(false) };
}

fn is_declaration_file(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".d.ts")
}

/// `.ts` / `.tsx` sources — not declaration files (which also end in `.ts`).
fn is_ts_source(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    (lower.ends_with(".ts") || lower.ends_with(".tsx")) && !is_declaration_file(&lower)
}

fn is_ts_or_dts(path: &str) -> bool {
    is_ts_source(path) || is_declaration_file(path)
}

/// Collect root names and file contents for a typecheck invocation.
///
/// Directory → every `.ts`/`.tsx` descendant is a root; `.d.ts` files are
/// included in the file set (so project ambient types resolve) but never as
/// roots. Single file → that file alone is the root.
pub fn collect_typecheck_inputs(
    fs: &FileSystem,
    path: &str,
) -> Result<(Vec<String>, Map<String, Value>), String> {
    let path = FileSystem::normalize_path(path);
    let mut roots = Vec::new();
    let mut files = Map::new();

    if !fs.exists(&path) {
        return Err(format!("path '{path}' does not exist"));
    }

    if fs.is_directory(&path) {
        let mut children: Vec<(String, String)> = fs.descendants_of(&path);
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for (child, _) in children {
            if !is_ts_or_dts(&child) {
                continue;
            }
            let content = fs.read_file(&child)?;
            files.insert(child.clone(), Value::String(content));
            if is_ts_source(&child) {
                roots.push(child);
            }
        }
    } else if is_declaration_file(&path) {
        return Err(format!(
            "'{path}' is a declaration file; pass a .ts/.tsx source"
        ));
    } else if is_ts_source(&path) {
        let content = fs.read_file(&path)?;
        files.insert(path.clone(), Value::String(content));
        roots.push(path);
    } else {
        return Err(format!(
            "'{path}' is not a .ts/.tsx file or a directory containing them"
        ));
    }

    Ok((roots, files))
}

/// Await the one-time TypeScript loader. Call from the agent UI before
/// `run_agent_loop` — never from `execute_tool`. Failure is non-fatal for the
/// agent loop; `typecheck` then returns a clean "not loaded" error.
#[cfg(target_arch = "wasm32")]
pub async fn ensure_typescript_loaded() -> Result<(), String> {
    if TS_READY.with(|c| c.get()) {
        return Ok(());
    }

    use js_sys::{Function, Promise, Reflect};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;

    let loader = Reflect::get(&window, &"kernelosLoadTypescript".into())
        .map_err(|e| format!("kernelosLoadTypescript missing: {e:?}"))?;
    if loader.is_undefined() || loader.is_null() {
        return Err(
            "kernelosLoadTypescript is not defined (is /ts/typecheck.js loaded?)"
                .into(),
        );
    }
    let loader: Function = loader
        .dyn_into()
        .map_err(|_| "kernelosLoadTypescript is not a function".to_string())?;

    let promise = loader
        .call0(&wasm_bindgen::JsValue::NULL)
        .map_err(|e| format!("kernelosLoadTypescript threw: {e:?}"))?;
    let promise: Promise = promise
        .dyn_into()
        .map_err(|_| "kernelosLoadTypescript did not return a Promise".to_string())?;

    JsFuture::from(promise)
        .await
        .map_err(|e| format!("TypeScript load failed: {e:?}"))?;

    TS_READY.with(|c| c.set(true));
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn ensure_typescript_loaded() -> Result<(), String> {
    // Host tests never load the browser compiler; tools use the stub.
    Ok(())
}

/// Run a synchronous typecheck over the collected VFS files.
pub fn typecheck_files(files: &Map<String, Value>, roots: &[String]) -> Result<String, String> {
    #[cfg(target_arch = "wasm32")]
    {
        typecheck_files_wasm(files, roots)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (files, roots);
        Err("typecheck unavailable outside the browser".into())
    }
}

#[cfg(target_arch = "wasm32")]
fn typecheck_files_wasm(files: &Map<String, Value>, roots: &[String]) -> Result<String, String> {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::JsCast;

    if !TS_READY.with(|c| c.get()) {
        return Err("typescript not loaded".into());
    }

    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let func_val = Reflect::get(&window, &"kernelosTypecheck".into())
        .map_err(|e| format!("kernelosTypecheck missing: {e:?}"))?;
    if func_val.is_undefined() || func_val.is_null() {
        return Err("kernelosTypecheck is not defined".into());
    }
    let func: Function = func_val
        .dyn_into()
        .map_err(|_| "kernelosTypecheck is not a function".to_string())?;

    let files_json = serde_json::to_string(files).map_err(|e| e.to_string())?;
    let roots_json = serde_json::to_string(roots).map_err(|e| e.to_string())?;

    let result = func
        .call2(
            &wasm_bindgen::JsValue::NULL,
            &wasm_bindgen::JsValue::from_str(&files_json),
            &wasm_bindgen::JsValue::from_str(&roots_json),
        )
        .map_err(|e| format!("kernelosTypecheck threw: {e:?}"))?;

    let result_str = result
        .as_string()
        .ok_or_else(|| "kernelosTypecheck returned a non-string".to_string())?;

    let parsed: Value = serde_json::from_str(&result_str)
        .map_err(|e| format!("kernelosTypecheck returned invalid JSON: {e}"))?;

    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    parsed
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "kernelosTypecheck JSON missing 'output'".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::FileSystem;

    #[test]
    fn dts_files_are_in_file_set_but_not_roots() {
        let mut fs = FileSystem::default();
        fs.create_directory("/home/projects/app", true).unwrap();
        fs.write_file("/home/projects/app/types.d.ts", "declare const MAGIC: number;\n")
            .unwrap();
        fs.write_file("/home/projects/app/main.ts", "const n: number = MAGIC;\n")
            .unwrap();

        let (roots, files) =
            collect_typecheck_inputs(&fs, "/home/projects/app").unwrap();

        assert_eq!(roots, vec!["/home/projects/app/main.ts".to_string()]);
        assert!(files.contains_key("/home/projects/app/types.d.ts"));
        assert!(files.contains_key("/home/projects/app/main.ts"));
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn empty_directory_yields_no_roots() {
        let mut fs = FileSystem::default();
        fs.create_directory("/home/projects/empty", true).unwrap();
        fs.write_file("/home/projects/empty/readme.txt", "hi\n")
            .unwrap();

        let (roots, files) =
            collect_typecheck_inputs(&fs, "/home/projects/empty").unwrap();
        assert!(roots.is_empty());
        assert!(files.is_empty());
    }
}
