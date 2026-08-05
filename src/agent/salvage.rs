//! Recover tool calls that leaked into plain-text `content`.
//!
//! DeepSeek occasionally (~11% under load) emits a tool call as prose with
//! `finish_reason: "stop"` and `tool_calls: null`. Without salvage the loop
//! exits mid-task. Conservative: only fires when a known tool name is followed
//! by a parseable JSON object — mere mentions of a tool name do not match.

use crate::agent::accum::ToolCallAccum;
use crate::agent::tools::TOOL_NAMES;

/// Try to recover one or more tool calls from assistant prose.
/// Returns `None` when nothing looks like an executable call.
pub fn salvage_tool_calls(content: &str) -> Option<Vec<ToolCallAccum>> {
    let mut found = Vec::new();
    let mut search_from = 0;

    while search_from < content.len() {
        let Some((name, name_end)) = find_tool_name(content, search_from) else {
            break;
        };

        let after_name = content[name_end..].trim_start();
        let abs_after = content.len() - after_name.len();

        // Skip optional junk: parentheses, code-fence markers, whitespace.
        let args_start = skip_preamble(content, abs_after);
        let Some(json) = extract_json_object(content, args_start) else {
            search_from = name_end;
            continue;
        };

        // Must be a real object — reject empty or non-object parses.
        if serde_json::from_str::<serde_json::Value>(json).ok().is_none_or(|v| !v.is_object()) {
            search_from = name_end;
            continue;
        }

        let id = format!("salvaged_{}", found.len());
        found.push(ToolCallAccum {
            id: Some(id),
            name: Some(name.to_string()),
            arguments: json.to_string(),
        });

        search_from = args_start + json.len();
    }

    if found.is_empty() {
        None
    } else {
        log::warn!(
            "salvaged {} plain-text tool call(s) from content",
            found.len()
        );
        Some(found)
    }
}

fn find_tool_name(content: &str, from: usize) -> Option<(&str, usize)> {
    let slice = &content[from..];
    let mut best: Option<(usize, &str)> = None;

    for name in TOOL_NAMES {
        if let Some(rel) = find_word_boundary(slice, name) {
            let abs = from + rel;
            if best.is_none_or(|(b, _)| abs < b) {
                best = Some((abs, *name));
            }
        }
    }

    best.map(|(abs, name)| (name, abs + name.len()))
}

/// Find `needle` in `haystack` only when bounded by non-identifier chars.
fn find_word_boundary(haystack: &str, needle: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(rel) = haystack[start..].find(needle) {
        let abs = start + rel;
        let before_ok = abs == 0
            || !haystack
                .get(abs - 1..abs)
                .is_some_and(|c| is_ident_char(c.chars().next().unwrap()));
        let after = abs + needle.len();
        let after_ok = after >= haystack.len()
            || !haystack
                .get(after..after + 1)
                .is_some_and(|c| is_ident_char(c.chars().next().unwrap()));
        if before_ok && after_ok {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn skip_preamble(content: &str, from: usize) -> usize {
    let rest = content[from..].trim_start();
    let mut idx = content.len() - rest.len();

    // Skip decorative separators: "write_file:" / "call write_file →"
    while idx < content.len() {
        let c = content[idx..].chars().next().unwrap();
        if c == ':' || c == '-' || c == '→' || c == '>' || c == ',' {
            idx += c.len_utf8();
            let trimmed = content[idx..].trim_start();
            idx = content.len() - trimmed.len();
            continue;
        }
        break;
    }

    // Optional opening paren: write_file({...})
    if content[idx..].starts_with('(') {
        idx += 1;
        let trimmed = content[idx..].trim_start();
        idx = content.len() - trimmed.len();
    }

    // Optional markdown fence: ```json
    if content[idx..].starts_with("```") {
        idx += 3;
        // skip language tag up to newline
        while idx < content.len() {
            let c = content.as_bytes()[idx];
            if c == b'\n' {
                idx += 1;
                break;
            }
            if !c.is_ascii_alphanumeric() && c != b'-' {
                break;
            }
            idx += 1;
        }
        let trimmed = content[idx..].trim_start();
        idx = content.len() - trimmed.len();
    }

    idx
}

/// Extract a balanced `{...}` JSON object starting at or after `from`.
fn extract_json_object(content: &str, from: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'{' {
        // Allow only whitespace before the object; anything else → no match.
        if !bytes[i].is_ascii_whitespace() {
            return None;
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }

    let start = i;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&content[start..=i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salvage_recovers_function_call_form() {
        let content = r#"I will write the file now.
write_file({"path":"/home/documents/notes.md","content":"hello"})
"#;
        let calls = salvage_tool_calls(content).expect("salvaged");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_deref(), Some("write_file"));
        assert!(calls[0].arguments.contains("/home/documents/notes.md"));
    }

    #[test]
    fn salvage_recovers_fenced_json_after_name() {
        let content = "Calling list_directory:\n```json\n{\"path\":\"/home\"}\n```\n";
        let calls = salvage_tool_calls(content).expect("salvaged");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name.as_deref(), Some("list_directory"));
        assert_eq!(calls[0].arguments, r#"{"path":"/home"}"#);
    }

    #[test]
    fn salvage_does_not_false_positive_on_mere_mention() {
        let prose = "I could use read_file to inspect the notes, or write_file later.";
        assert!(salvage_tool_calls(prose).is_none());

        let prose2 = "The read_file tool would help here, but I need more context first.";
        assert!(salvage_tool_calls(prose2).is_none());
    }

    #[test]
    fn salvage_rejects_non_json_after_name() {
        let content = "read_file(/home/documents/notes.md)";
        assert!(salvage_tool_calls(content).is_none());
    }
}
