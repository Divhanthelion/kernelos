//! VFS tools for the agent (PLAN M2), TypeScript typecheck (M5a), Python (M5b).
//!
//! Eight tools is the ceiling (PLAN §2 plain-text leak risk). New tools append
//! only — never reorder — so DeepSeek's prefix cache stays warm.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::agent::journal::{
    confirm_gate, is_protected_path, Journal, RECURSIVE_DELETE_GATE_THRESHOLD,
};
use crate::agent::python::{collect_python_inputs, run_python_files};
use crate::agent::typecheck::{collect_typecheck_inputs, typecheck_files};
use crate::filesystem::FileSystem;

/// Soft cap on tool-result content returned to the model.
pub const MAX_TOOL_RESULT_BYTES: usize = 8_192;

/// Hard ceiling on tool count — PLAN §2 correlates leaks with 40+ definitions;
/// we stay at the ~6–8 band. Append only; never insert.
pub const MAX_TOOLS: usize = 8;

/// The eight tool names. Keep in sync with `tool_definitions`.
/// `run_python` must remain last — prefix-cache stability.
pub const TOOL_NAMES: &[&str] = &[
    "read_file",
    "write_file",
    "list_directory",
    "create_directory",
    "delete",
    "rename",
    "typecheck",
    "run_python",
];

const TRUNCATION_MARKER_PREFIX: &str = "\n\n[truncated, ";
const TRUNCATION_MARKER_SUFFIX: &str = " bytes omitted]";

/// Stable, byte-identical tool definitions for DeepSeek prefix caching.
/// Built with `BTreeMap` so key order never depends on `HashMap` iteration.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool_def(
            "read_file",            "Read the contents of a file at the given absolute VFS path.",
            props(&[("path", prop_string("Absolute path to the file to read."))]),
            &["path"],
        ),
        tool_def(
            "write_file",
            "Write text contents to a file at the given absolute VFS path. \
             Creates the file if it does not exist; parent directory must exist.",
            props(&[
                ("path", prop_string("Absolute path to the file to write.")),
                ("content", prop_string("Full text content to write.")),
            ]),
            &["path", "content"],
        ),
        tool_def(
            "list_directory",
            "List entries in a directory at the given absolute VFS path.",
            props(&[("path", prop_string("Absolute path of the directory to list."))]),
            &["path"],
        ),
        tool_def(
            "create_directory",
            "Create a directory at the given absolute VFS path. \
             Set create_parents to true to create missing parents.",
            props(&[
                ("path", prop_string("Absolute path of the directory to create.")),
                (
                    "create_parents",
                    prop_bool("If true, create missing parent directories."),
                ),
            ]),
            &["path", "create_parents"],
        ),
        tool_def(
            "delete",
            "Delete a file or directory at the given absolute VFS path. \
             Set recursive to true to delete a non-empty directory.",
            props(&[
                ("path", prop_string("Absolute path to delete.")),
                (
                    "recursive",
                    prop_bool("If true, recursively delete directory contents."),
                ),
            ]),
            &["path", "recursive"],
        ),
        tool_def(
            "rename",
            "Rename or move a file or directory from old_path to new_path.",
            props(&[
                ("old_path", prop_string("Current absolute path.")),
                ("new_path", prop_string("Destination absolute path.")),
            ]),
            &["old_path", "new_path"],
        ),
        // Appended after the six VFS tools — do not reorder.
        tool_def(
            "typecheck",
            "Run the TypeScript compiler over a .ts/.tsx file or a directory of \
             them in the VFS. Returns diagnostics (or 'no errors'). Read-only.",
            props(&[(
                "path",
                prop_string(
                    "Absolute VFS path to a .ts/.tsx file, or a directory to \
                     typecheck recursively.",
                ),
            )]),
            &["path"],
        ),
        // Appended last — do not reorder; DeepSeek prefix caching depends on it.
        tool_def(
            "run_python",
            "Execute a Python (.py) file from the VFS with the in-browser CPython \
             (Pyodide). Returns stdout, stderr, and tracebacks. Stdlib only; \
             read-only with respect to the VFS. Warning: infinite loops hang the tab.",
            props(&[(
                "path",
                prop_string("Absolute VFS path to a .py file to execute."),
            )]),
            &["path"],
        ),
    ]
}

fn prop_string(description: &str) -> Value {
    let mut m = BTreeMap::new();
    m.insert("type".into(), json!("string"));
    m.insert("description".into(), json!(description));
    Value::Object(m.into_iter().collect())
}

fn prop_bool(description: &str) -> Value {
    let mut m = BTreeMap::new();
    m.insert("type".into(), json!("boolean"));
    m.insert("description".into(), json!(description));
    Value::Object(m.into_iter().collect())
}

fn props(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn tool_def(
    name: &str,
    description: &str,
    properties: BTreeMap<String, Value>,
    required: &[&str],
) -> Value {
    // Nested BTreeMaps keep every object key-ordered for cache-stable bytes.
    let mut parameters = BTreeMap::new();
    parameters.insert("type".into(), json!("object"));
    parameters.insert(
        "properties".into(),
        Value::Object(properties.into_iter().collect()),
    );
    parameters.insert(
        "required".into(),
        json!(required.iter().copied().collect::<Vec<_>>()),
    );
    parameters.insert("additionalProperties".into(), json!(false));

    let mut function = BTreeMap::new();
    function.insert("name".into(), json!(name));
    function.insert("description".into(), json!(description));
    function.insert("strict".into(), json!(true));
    function.insert(
        "parameters".into(),
        Value::Object(parameters.into_iter().collect()),
    );

    let mut tool = BTreeMap::new();
    tool.insert("type".into(), json!("function"));
    tool.insert(
        "function".into(),
        Value::Object(function.into_iter().collect()),
    );
    Value::Object(tool.into_iter().collect())
}

/// Execute one tool call. `arguments` is a JSON-encoded string (may be malformed).
/// Returns a string content payload suitable for a `{role:"tool"}` message.
///
/// When `journal` is provided, mutating tools record first-touch priors and
/// refresh `after` snapshots. Read-only tools do not touch the journal.
pub fn execute_tool(
    fs: &mut FileSystem,
    name: &str,
    arguments: &str,
    journal: Option<&mut Journal>,
) -> String {
    let args: Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => {
            return truncate_result(format!("error: malformed arguments JSON: {e}"));
        }
    };

    let result = match name {
        "read_file" => exec_read_file(fs, &args),
        "write_file" => exec_write_file(fs, &args, journal),
        "list_directory" => exec_list_directory(fs, &args),
        "create_directory" => exec_create_directory(fs, &args, journal),
        "delete" => exec_delete(fs, &args, journal),
        "rename" => exec_rename(fs, &args, journal),
        "typecheck" => exec_typecheck(fs, &args),
        "run_python" => exec_run_python(fs, &args),
        other => Err(format!("unknown tool: {other}")),
    };

    let body = match result {
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    };
    truncate_result(body)
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing or non-string argument '{key}'"))
}

fn require_bool(args: &Value, key: &str) -> Result<bool, String> {
    args.get(key)
        .and_then(|v| v.as_bool())
        .ok_or_else(|| format!("missing or non-boolean argument '{key}'"))
}

fn gate_protected(path: &str) -> Result<(), String> {
    if is_protected_path(path) {
        let msg = format!(
            "modification of '{path}' is gated (/system/config). Proceed?"
        );
        if !confirm_gate(&msg) {
            return Err(format!(
                "refused: '{path}' is under /system/config (gated)"
            ));
        }
    }
    Ok(())
}

fn exec_read_file(fs: &FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    fs.read_file(path)
}

fn exec_write_file(
    fs: &mut FileSystem,
    args: &Value,
    mut journal: Option<&mut Journal>,
) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let content = require_str(args, "content")?;
    gate_protected(path)?;
    if let Some(j) = journal.as_deref_mut() {
        j.touch(fs, path);
    }
    fs.write_file(path, content)?;
    if let Some(j) = journal.as_deref_mut() {
        j.record_after(fs, path);
    }
    Ok(format!("wrote {} bytes to {path}", content.len()))
}

fn exec_list_directory(fs: &FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let entries = fs.list_directory(path)?;
    let lines: Vec<String> = entries
        .iter()
        .map(|e| {
            let kind = match e.file_type {
                crate::filesystem::FileType::Directory => "dir",
                crate::filesystem::FileType::File => "file",
            };
            format!("{kind}\t{}\t{}", e.name, e.size)
        })
        .collect();
    Ok(lines.join("\n"))
}

fn exec_create_directory(
    fs: &mut FileSystem,
    args: &Value,
    mut journal: Option<&mut Journal>,
) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let create_parents = require_bool(args, "create_parents")?;
    gate_protected(path)?;
    if let Some(j) = journal.as_deref_mut() {
        j.touch(fs, path);
        if create_parents {
            let mut cursor = std::path::Path::new(path);
            while let Some(parent) = cursor.parent() {
                let p = parent.to_string_lossy();
                if p.is_empty() || p == "/" {
                    break;
                }
                if !fs.exists(p.as_ref()) {
                    j.touch(fs, p.as_ref());
                }
                cursor = parent;
            }
        }
    }
    fs.create_directory(path, create_parents)?;
    if let Some(j) = journal.as_deref_mut() {
        j.record_after(fs, path);
        // Parents created via create_parents were touched as Absent — refresh.
        let mut cursor = std::path::Path::new(path);
        while let Some(parent) = cursor.parent() {
            let p = parent.to_string_lossy();
            if p.is_empty() || p == "/" {
                break;
            }
            j.record_after(fs, p.as_ref());
            cursor = parent;
        }
    }
    Ok(format!("created directory {path}"))
}

fn exec_delete(
    fs: &mut FileSystem,
    args: &Value,
    mut journal: Option<&mut Journal>,
) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let recursive = require_bool(args, "recursive")?;
    gate_protected(path)?;

    if recursive && fs.is_directory(path) {
        let n = 1 + fs.descendants_of(path).len();
        if n >= RECURSIVE_DELETE_GATE_THRESHOLD {
            let msg = format!(
                "Recursive delete of {n} entries under '{path}' exceeds threshold \
                 ({RECURSIVE_DELETE_GATE_THRESHOLD}). Proceed?"
            );
            if !confirm_gate(&msg) {
                return Err(format!(
                    "refused: recursive delete of {n} entries under '{path}' \
                     exceeds gate threshold {RECURSIVE_DELETE_GATE_THRESHOLD}"
                ));
            }
        }
    }

    if let Some(j) = journal.as_deref_mut() {
        j.touch_delete(fs, path, recursive);
    }
    fs.delete(path, recursive)?;
    if let Some(j) = journal.as_deref_mut() {
        j.record_after_delete(fs, path);
    }
    Ok(format!("deleted {path}"))
}

fn exec_rename(
    fs: &mut FileSystem,
    args: &Value,
    mut journal: Option<&mut Journal>,
) -> Result<String, String> {
    let old_path = require_str(args, "old_path")?;
    let new_path = require_str(args, "new_path")?;
    gate_protected(old_path)?;
    gate_protected(new_path)?;
    if let Some(j) = journal.as_deref_mut() {
        j.touch_rename(fs, old_path, new_path);
    }
    fs.rename(old_path, new_path)?;
    if let Some(j) = journal.as_deref_mut() {
        j.record_after_rename(fs, old_path, new_path);
    }
    Ok(format!("renamed {old_path} → {new_path}"))
}

/// Read-only — must not touch the journal.
fn exec_typecheck(fs: &FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let (roots, files) = collect_typecheck_inputs(fs, path)?;
    if roots.is_empty() {
        return Ok(format!("no TypeScript files found under {path}"));
    }
    typecheck_files(&files, &roots)
}

/// Read-only with respect to the VFS — takes `&FileSystem`, no journal.
/// Python-side writes do not land in the KernelOS VFS (M5b scope).
fn exec_run_python(fs: &FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let (entry, files) = collect_python_inputs(fs, path)?;
    run_python_files(&files, &entry)
}

/// Truncate oversized tool results with an explicit marker.
pub fn truncate_result(s: String) -> String {
    if s.len() <= MAX_TOOL_RESULT_BYTES {
        return s;
    }

    let omitted = s.len().saturating_sub(MAX_TOOL_RESULT_BYTES);
    let marker = format!("{TRUNCATION_MARKER_PREFIX}{omitted}{TRUNCATION_MARKER_SUFFIX}");
    let keep = MAX_TOOL_RESULT_BYTES.saturating_sub(marker.len());

    // Walk back to a char boundary so we don't split a UTF-8 sequence.
    let mut end = keep.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }

    let mut out = s[..end].to_string();
    let omitted = s.len() - end;
    out.push_str(&format!(
        "{TRUNCATION_MARKER_PREFIX}{omitted}{TRUNCATION_MARKER_SUFFIX}"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::journal::PathState;
    use crate::filesystem::FileSystem;

    #[test]
    fn tool_schema_serialization_is_byte_identical_across_calls() {
        let a = serde_json::to_string(&tool_definitions()).unwrap();
        let b = serde_json::to_string(&tool_definitions()).unwrap();
        assert_eq!(a, b);
        let c = serde_json::to_string(&tool_definitions()).unwrap();
        assert_eq!(a, c);
    }

    #[test]
    fn tool_schemas_have_strict_mode_and_no_additional_properties() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 8);
        assert_eq!(TOOL_NAMES.len(), 8);
        assert!(
            TOOL_NAMES.len() <= MAX_TOOLS,
            "tool count {} exceeds ceiling {MAX_TOOLS} — PLAN §2 leak risk",
            TOOL_NAMES.len()
        );
        assert_eq!(TOOL_NAMES.last(), Some(&"run_python"));
        assert_eq!(
            tools.last().and_then(|t| t["function"]["name"].as_str()),
            Some("run_python"),
            "run_python must be last for prefix-cache stability"
        );
        assert_eq!(TOOL_NAMES[TOOL_NAMES.len() - 2], "typecheck");
        for tool in &tools {
            let func = &tool["function"];
            assert_eq!(func["strict"], true);
            assert_eq!(func["parameters"]["additionalProperties"], false);
            let props = func["parameters"]["properties"].as_object().unwrap();
            let required = func["parameters"]["required"].as_array().unwrap();
            assert_eq!(props.len(), required.len());
            for key in props.keys() {
                assert!(
                    required.iter().any(|r| r.as_str() == Some(key)),
                    "property {key} missing from required"
                );
            }
        }
    }

    #[test]
    fn malformed_arguments_produce_tool_error_not_panic() {
        let mut fs = FileSystem::default();
        let out = execute_tool(&mut fs, "read_file", "not-json{{{", None);
        assert!(out.starts_with("error: malformed arguments JSON:"), "{out}");
    }

    #[test]
    fn result_truncation_inserts_marker() {
        let big = "x".repeat(MAX_TOOL_RESULT_BYTES + 500);
        let out = truncate_result(big);
        assert!(out.contains("[truncated, "));
        assert!(out.contains(" bytes omitted]"));
        assert!(out.len() <= MAX_TOOL_RESULT_BYTES + 64);
    }

    #[test]
    fn each_tool_maps_onto_filesystem() {
        let mut fs = FileSystem::default();

        let created = execute_tool(
            &mut fs,
            "create_directory",
            r#"{"path":"/tmp","create_parents":true}"#,
            None,
        );
        assert_eq!(created, "created directory /tmp");

        let nested = execute_tool(
            &mut fs,
            "create_directory",
            r#"{"path":"/tmp/a","create_parents":false}"#,
            None,
        );
        assert_eq!(nested, "created directory /tmp/a");

        let listed = execute_tool(&mut fs, "list_directory", r#"{"path":"/tmp"}"#, None);
        assert!(listed.contains("a"), "{listed}");

        let renamed = execute_tool(
            &mut fs,
            "rename",
            r#"{"old_path":"/tmp/a","new_path":"/tmp/b"}"#,
            None,
        );
        assert_eq!(renamed, "renamed /tmp/a → /tmp/b");

        let deleted = execute_tool(
            &mut fs,
            "delete",
            r#"{"path":"/tmp/b","recursive":false}"#,
            None,
        );
        assert_eq!(deleted, "deleted /tmp/b");

        let wrote = execute_tool(
            &mut fs,
            "write_file",
            r#"{"path":"/home/documents/notes.txt","content":"hello"}"#,
            None,
        );
        assert_eq!(wrote, "wrote 5 bytes to /home/documents/notes.txt");
        assert!(fs.exists("/home/documents/notes.txt"));

        let read = execute_tool(
            &mut fs,
            "read_file",
            r#"{"path":"/home/documents/notes.txt"}"#,
            None,
        );
        assert_eq!(read, "hello");
    }

    #[test]
    fn unknown_tool_returns_error_string() {
        let mut fs = FileSystem::default();
        let out = execute_tool(&mut fs, "not_a_tool", r#"{}"#, None);
        assert_eq!(out, "error: unknown tool: not_a_tool");
    }

    #[test]
    fn typecheck_is_dispatched_not_unknown() {
        let mut fs = FileSystem::default();
        let _ = fs.write_file("/home/documents/x.ts", "const n: number = 1;\n");
        let out = execute_tool(
            &mut fs,
            "typecheck",
            r#"{"path":"/home/documents/x.ts"}"#,
            None,
        );
        // Host stub — never "unknown tool".
        assert!(
            !out.contains("unknown tool"),
            "typecheck must be dispatched, got {out}"
        );
        assert!(
            out.contains("typecheck unavailable outside the browser")
                || out.contains("no errors")
                || out.contains("error TS"),
            "{out}"
        );
    }

    #[test]
    fn typecheck_leaves_journal_empty() {
        let mut fs = FileSystem::default();
        let mut journal = Journal::new();
        let _ = fs.write_file("/home/documents/j.ts", "const n: number = 1;\n");
        let _ = execute_tool(
            &mut fs,
            "typecheck",
            r#"{"path":"/home/documents/j.ts"}"#,
            Some(&mut journal),
        );
        assert!(
            journal.is_empty(),
            "typecheck must not touch the journal (undo safety)"
        );
    }

    #[test]
    fn typecheck_host_stub_returns_unavailable_error() {
        let mut fs = FileSystem::default();
        let _ = fs.write_file("/home/documents/stub.ts", "export {}\n");
        let out = execute_tool(
            &mut fs,
            "typecheck",
            r#"{"path":"/home/documents/stub.ts"}"#,
            None,
        );
        assert!(
            out.starts_with("error: typecheck unavailable outside the browser"),
            "{out}"
        );
    }

    #[test]
    fn typecheck_empty_directory_reports_no_files_found() {
        let mut fs = FileSystem::default();
        fs.create_directory("/home/projects/emptyts", true).unwrap();
        fs.write_file("/home/projects/emptyts/note.txt", "x\n")
            .unwrap();
        let out = execute_tool(
            &mut fs,
            "typecheck",
            r#"{"path":"/home/projects/emptyts"}"#,
            None,
        );
        assert!(
            out.contains("no TypeScript files found under /home/projects/emptyts"),
            "{out}"
        );
        assert!(!out.contains("no errors"), "{out}");
    }

    #[test]
    fn typecheck_includes_dts_in_collected_files() {
        let mut fs = FileSystem::default();
        fs.create_directory("/home/projects/dts", true).unwrap();
        fs.write_file(
            "/home/projects/dts/types.d.ts",
            "declare const MAGIC: number;\n",
        )
        .unwrap();
        fs.write_file("/home/projects/dts/main.ts", "const n: number = MAGIC;\n")
            .unwrap();
        let (roots, files) =
            collect_typecheck_inputs(&fs, "/home/projects/dts").unwrap();
        assert_eq!(roots, vec!["/home/projects/dts/main.ts".to_string()]);
        assert!(files.contains_key("/home/projects/dts/types.d.ts"));
    }

    #[test]
    fn run_python_is_dispatched_and_leaves_journal_empty() {
        let mut fs = FileSystem::default();
        let mut journal = Journal::new();
        fs.write_file("/home/documents/hi.py", "print('hi')\n")
            .unwrap();
        let out = execute_tool(
            &mut fs,
            "run_python",
            r#"{"path":"/home/documents/hi.py"}"#,
            Some(&mut journal),
        );
        assert!(!out.contains("unknown tool"), "{out}");
        assert!(journal.is_empty(), "run_python must not touch the journal");
        assert!(
            out.starts_with("error: python unavailable outside the browser"),
            "{out}"
        );
    }

    #[test]
    fn mutating_tools_record_first_touch_in_journal() {
        let mut fs = FileSystem::default();
        let mut journal = Journal::new();
        let _ = execute_tool(
            &mut fs,
            "write_file",
            r#"{"path":"/home/documents/j.txt","content":"one"}"#,
            Some(&mut journal),
        );
        let _ = execute_tool(
            &mut fs,
            "write_file",
            r#"{"path":"/home/documents/j.txt","content":"two"}"#,
            Some(&mut journal),
        );
        assert_eq!(journal.len(), 1);
        assert_eq!(journal.changed_paths().len(), 1);
        match &journal.get("/home/documents/j.txt").unwrap().after {
            PathState::File { content, .. } => assert_eq!(content, "two"),
            other => panic!("expected file after, got {other:?}"),
        }
    }

    #[test]
    fn gated_recursive_delete_refused_on_host() {
        let mut fs = FileSystem::default();
        // Seed enough files under /tmp/wipe to exceed the gate threshold.
        fs.create_directory("/tmp/wipe", true).unwrap();
        for i in 0..RECURSIVE_DELETE_GATE_THRESHOLD {
            let p = format!("/tmp/wipe/f{i}.txt");
            fs.write_file(&p, "x").unwrap();
        }
        let out = execute_tool(
            &mut fs,
            "delete",
            r#"{"path":"/tmp/wipe","recursive":true}"#,
            None,
        );
        assert!(
            out.starts_with("error: refused:"),
            "expected gate refusal on host, got {out}"
        );
        assert!(fs.exists("/tmp/wipe"));
    }
}
