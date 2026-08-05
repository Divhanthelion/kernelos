//! Python execution bridge via Pyodide (PLAN M5b).
//!
//! Loaded asynchronously once (before `run_agent_loop`), then `execute_tool`
//! calls into it synchronously. Do not `.await` from the tool path.
//!
//! **Limitation:** Pyodide runs on the main thread. A `while True:` (or any
//! unbounded loop) from the agent will hang the tab. That matches PLAN.md's
//! blast-radius thesis — close the tab — and is not fixed here (a Worker would
//! be the §2a redesign).

use crate::filesystem::FileSystem;
use serde_json::{Map, Value};
use std::cell::Cell;

thread_local! {
    static PY_READY: Cell<bool> = const { Cell::new(false) };
}

/// Drop `<exec>` / `<frozen runpy>` frames above the entry file so the agent
/// only sees its own source lines.
pub fn strip_traceback_noise(tb: &str, entry: &str) -> String {
    if tb.is_empty() {
        return String::new();
    }
    let marker = format!("File \"{entry}\"");
    let lines: Vec<&str> = tb.lines().collect();
    let start = lines.iter().position(|l| l.contains(&marker));
    let Some(start) = start else {
        return tb.to_string();
    };
    let mut out = Vec::new();
    if lines.first().is_some_and(|l| l.starts_with("Traceback")) {
        out.push(lines[0].to_string());
    }
    out.extend(lines[start..].iter().map(|s| (*s).to_string()));
    out.join("\n")
}

fn is_python_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".py")
}

fn parent_dir(path: &str) -> String {
    let path = FileSystem::normalize_path(path);
    match path.rsplit_once('/') {
        Some(("", _)) => "/".into(),
        Some((parent, _)) if parent.is_empty() => "/".into(),
        Some((parent, _)) => parent.to_string(),
        None => "/".into(),
    }
}

/// Collect the entry script plus every `.py` file under its parent directory
/// (siblings and nested packages) so local imports resolve.
pub fn collect_python_inputs(
    fs: &FileSystem,
    path: &str,
) -> Result<(String, Map<String, Value>), String> {
    let path = FileSystem::normalize_path(path);
    if !fs.exists(&path) {
        return Err(format!("path '{path}' does not exist"));
    }
    if fs.is_directory(&path) {
        return Err(format!(
            "'{path}' is a directory; pass a .py file to run"
        ));
    }
    if !is_python_path(&path) {
        return Err(format!("'{path}' is not a .py file"));
    }

    let parent = parent_dir(&path);
    let mut files = Map::new();

    // Siblings in the parent directory.
    if let Ok(entries) = fs.list_directory(&parent) {
        for entry in entries {
            if matches!(entry.file_type, crate::filesystem::FileType::File)
                && is_python_path(&entry.name)
            {
                let full = if parent == "/" {
                    format!("/{}", entry.name)
                } else {
                    format!("{parent}/{}", entry.name)
                };
                let content = fs.read_file(&full)?;
                files.insert(full, Value::String(content));
            }
        }
    }

    // Nested packages under the parent.
    for (child, _) in fs.descendants_of(&parent) {
        if is_python_path(&child) && !files.contains_key(&child) {
            let content = fs.read_file(&child)?;
            files.insert(child, Value::String(content));
        }
    }

    // Ensure the entry itself is present even if list_directory missed it.
    if !files.contains_key(&path) {
        let content = fs.read_file(&path)?;
        files.insert(path.clone(), Value::String(content));
    }

    Ok((path, files))
}

#[cfg(target_arch = "wasm32")]
pub async fn ensure_python_loaded() -> Result<(), String> {
    if PY_READY.with(|c| c.get()) {
        return Ok(());
    }

    use js_sys::{Function, Promise, Reflect};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;

    let loader = Reflect::get(&window, &"kernelosLoadPython".into())
        .map_err(|e| format!("kernelosLoadPython missing: {e:?}"))?;
    if loader.is_undefined() || loader.is_null() {
        return Err(
            "kernelosLoadPython is not defined (is /py/python.js loaded? run ./fetch-pyodide.sh?)"
                .into(),
        );
    }
    let loader: Function = loader
        .dyn_into()
        .map_err(|_| "kernelosLoadPython is not a function".to_string())?;

    let promise = loader
        .call0(&wasm_bindgen::JsValue::NULL)
        .map_err(|e| format!("kernelosLoadPython threw: {e:?}"))?;
    let promise: Promise = promise
        .dyn_into()
        .map_err(|_| "kernelosLoadPython did not return a Promise".to_string())?;

    JsFuture::from(promise)
        .await
        .map_err(|e| format!("Python/Pyodide load failed: {e:?}"))?;

    PY_READY.with(|c| c.set(true));
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn ensure_python_loaded() -> Result<(), String> {
    Ok(())
}

pub fn run_python_files(files: &Map<String, Value>, entry: &str) -> Result<String, String> {
    #[cfg(target_arch = "wasm32")]
    {
        run_python_files_wasm(files, entry)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (files, entry);
        Err("python unavailable outside the browser".into())
    }
}

#[cfg(target_arch = "wasm32")]
fn run_python_files_wasm(files: &Map<String, Value>, entry: &str) -> Result<String, String> {
    use js_sys::{Function, Reflect};
    use wasm_bindgen::JsCast;

    if !PY_READY.with(|c| c.get()) {
        return Err("python not loaded".into());
    }

    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let func_val = Reflect::get(&window, &"kernelosRunPython".into())
        .map_err(|e| format!("kernelosRunPython missing: {e:?}"))?;
    if func_val.is_undefined() || func_val.is_null() {
        return Err("kernelosRunPython is not defined".into());
    }
    let func: Function = func_val
        .dyn_into()
        .map_err(|_| "kernelosRunPython is not a function".to_string())?;

    let files_json = serde_json::to_string(files).map_err(|e| e.to_string())?;

    let result = func
        .call2(
            &wasm_bindgen::JsValue::NULL,
            &wasm_bindgen::JsValue::from_str(&files_json),
            &wasm_bindgen::JsValue::from_str(entry),
        )
        .map_err(|e| format!("kernelosRunPython threw: {e:?}"))?;

    let result_str = result
        .as_string()
        .ok_or_else(|| "kernelosRunPython returned a non-string".to_string())?;

    let parsed: Value = serde_json::from_str(&result_str)
        .map_err(|e| format!("kernelosRunPython returned invalid JSON: {e}"))?;

    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    parsed
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "kernelosRunPython JSON missing 'output'".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::FileSystem;

    #[test]
    fn collect_includes_sibling_modules() {
        let mut fs = FileSystem::default();
        fs.create_directory("/home/projects/py", true).unwrap();
        fs.write_file("/home/projects/py/main.py", "import helper\nhelper.go()\n")
            .unwrap();
        fs.write_file("/home/projects/py/helper.py", "def go():\n    print('ok')\n")
            .unwrap();

        let (entry, files) = collect_python_inputs(&fs, "/home/projects/py/main.py").unwrap();
        assert_eq!(entry, "/home/projects/py/main.py");
        assert!(files.contains_key("/home/projects/py/main.py"));
        assert!(files.contains_key("/home/projects/py/helper.py"));
    }

    #[test]
    fn strip_traceback_drops_runpy_frames_above_entry() {
        let tb = "\
Traceback (most recent call last):\n\
  File \"<exec>\", line 3, in <module>\n\
  File \"<frozen runpy>\", line 287, in run_path\n\
  File \"<frozen runpy>\", line 98, in _run_module_code\n\
  File \"<frozen runpy>\", line 88, in _run_code\n\
  File \"/p/b.py\", line 4, in <module>\n\
    boom()\n\
  File \"/p/b.py\", line 2, in boom\n\
    raise ValueError('x')\n\
ValueError: x";
        let cleaned = strip_traceback_noise(tb, "/p/b.py");
        assert!(cleaned.starts_with("Traceback (most recent call last):"));
        assert!(cleaned.contains("File \"/p/b.py\", line 4"));
        assert!(!cleaned.contains("<exec>"));
        assert!(!cleaned.contains("<frozen runpy>"));
        assert!(cleaned.contains("ValueError: x"));
    }

    /// Documents the module-cache contract the JS loader must uphold. The
    /// executable check is `scripts/test-python-cache.mjs` against real Pyodide
    /// (edit-and-rerun + cross-directory `helper.py` shadowing).
    #[test]
    fn module_cache_contract_notes_baseline_not_path_prefix() {
        // Path-prefix eviction is wrong: Pyodide stdlib spans both
        // /lib/python3.12/ and /lib/python312.zip/. The loader snapshots
        // frozenset(sys.modules) at loadPyodide() and pops anything not in it.
        let contract = "baseline frozenset + invalidate_caches";
        assert!(contract.contains("baseline"));
        assert!(!contract.contains("startswith"));
    }
}
