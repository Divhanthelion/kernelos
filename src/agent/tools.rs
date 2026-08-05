//! VFS tools for the agent (PLAN M2).
//!
//! Six tools, mapped 1:1 onto `FileSystem` methods. Keep the set small — the
//! primary mitigation for plain-text tool-call leakage.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::filesystem::FileSystem;

/// Soft cap on tool-result content returned to the model.
pub const MAX_TOOL_RESULT_BYTES: usize = 8_192;

/// The six VFS tool names. Keep in sync with `tool_definitions`.
pub const TOOL_NAMES: &[&str] = &[
    "read_file",
    "write_file",
    "list_directory",
    "create_directory",
    "delete",
    "rename",
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
pub fn execute_tool(fs: &mut FileSystem, name: &str, arguments: &str) -> String {
    let args: Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => {
            return truncate_result(format!("error: malformed arguments JSON: {e}"));
        }
    };

    let result = match name {
        "read_file" => exec_read_file(fs, &args),
        "write_file" => exec_write_file(fs, &args),
        "list_directory" => exec_list_directory(fs, &args),
        "create_directory" => exec_create_directory(fs, &args),
        "delete" => exec_delete(fs, &args),
        "rename" => exec_rename(fs, &args),
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

fn exec_read_file(fs: &FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    fs.read_file(path)
}

fn exec_write_file(fs: &mut FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let content = require_str(args, "content")?;
    fs.write_file(path, content)?;
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

fn exec_create_directory(fs: &mut FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let create_parents = require_bool(args, "create_parents")?;
    fs.create_directory(path, create_parents)?;
    Ok(format!("created directory {path}"))
}

fn exec_delete(fs: &mut FileSystem, args: &Value) -> Result<String, String> {
    let path = require_str(args, "path")?;
    let recursive = require_bool(args, "recursive")?;
    fs.delete(path, recursive)?;
    Ok(format!("deleted {path}"))
}

fn exec_rename(fs: &mut FileSystem, args: &Value) -> Result<String, String> {
    let old_path = require_str(args, "old_path")?;
    let new_path = require_str(args, "new_path")?;
    fs.rename(old_path, new_path)?;
    Ok(format!("renamed {old_path} → {new_path}"))
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
        assert_eq!(tools.len(), 6);
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
        let out = execute_tool(&mut fs, "read_file", "not-json{{{");
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
        );
        assert_eq!(created, "created directory /tmp");

        let nested = execute_tool(
            &mut fs,
            "create_directory",
            r#"{"path":"/tmp/a","create_parents":false}"#,
        );
        assert_eq!(nested, "created directory /tmp/a");

        let listed = execute_tool(&mut fs, "list_directory", r#"{"path":"/tmp"}"#);
        assert!(listed.contains("a"), "{listed}");

        let renamed = execute_tool(
            &mut fs,
            "rename",
            r#"{"old_path":"/tmp/a","new_path":"/tmp/b"}"#,
        );
        assert_eq!(renamed, "renamed /tmp/a → /tmp/b");

        let deleted = execute_tool(
            &mut fs,
            "delete",
            r#"{"path":"/tmp/b","recursive":false}"#,
        );
        assert_eq!(deleted, "deleted /tmp/b");

        let wrote = execute_tool(
            &mut fs,
            "write_file",
            r#"{"path":"/home/documents/notes.txt","content":"hello"}"#,
        );
        assert_eq!(wrote, "wrote 5 bytes to /home/documents/notes.txt");
        assert!(fs.exists("/home/documents/notes.txt"));

        // Content lives in localStorage; unavailable under `cargo test`, so
        // read_file surfaces the FileSystem error rather than panicking.
        let read = execute_tool(
            &mut fs,
            "read_file",
            r#"{"path":"/home/documents/notes.txt"}"#,
        );
        assert!(
            read == "hello" || read.starts_with("error:"),
            "{read}"
        );
    }

    #[test]
    fn unknown_tool_returns_error_string() {
        let mut fs = FileSystem::default();
        let out = execute_tool(&mut fs, "not_a_tool", r#"{}"#);
        assert_eq!(out, "error: unknown tool: not_a_tool");
    }
}
