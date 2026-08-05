//! Turn accumulator — merges streaming delta fragments into a complete turn.

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallAccum {
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnAccumulator {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCallAccum>,
    pub finish_reason: Option<String>,
}

impl TurnAccumulator {
    /// Apply one `chat.completion.chunk` JSON object.
    pub fn apply_chunk(&mut self, chunk: &Value) {
        let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) else {
            return;
        };
        if choices.is_empty() {
            return;
        }

        let choice = &choices[0];

        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            self.finish_reason = Some(fr.to_string());
        }

        let Some(delta) = choice.get("delta") else {
            return;
        };

        if let Some(c) = delta.get("content").and_then(|v| v.as_str()) {
            self.content.push_str(c);
        }
        if let Some(r) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
            self.reasoning.push_str(r);
        }

        if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                while self.tool_calls.len() <= index {
                    self.tool_calls.push(ToolCallAccum::default());
                }
                let accum = &mut self.tool_calls[index];
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    accum.id = Some(id.to_string());
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                        accum.name = Some(name.to_string());
                    }
                    if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                        accum.arguments.push_str(args);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_call_arguments_split_across_three_chunks() {
        let mut accum = TurnAccumulator::default();
        accum.apply_chunk(&json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"pa"
                        }
                    }]
                }
            }]
        }));
        accum.apply_chunk(&json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "th\":" }
                    }]
                }
            }]
        }));
        accum.apply_chunk(&json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "\"/foo\"}" }
                    }]
                }
            }]
        }));
        assert_eq!(accum.tool_calls.len(), 1);
        assert_eq!(accum.tool_calls[0].name.as_deref(), Some("read_file"));
        assert_eq!(accum.tool_calls[0].arguments, r#"{"path":"/foo"}"#);
    }

    #[test]
    fn usage_only_chunk_with_empty_choices() {
        let mut accum = TurnAccumulator::default();
        accum.apply_chunk(&json!({
            "choices": [],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        }));
        assert!(accum.content.is_empty());
        assert!(accum.reasoning.is_empty());
        assert!(accum.tool_calls.is_empty());
        assert!(accum.finish_reason.is_none());
    }

    #[test]
    fn reasoning_content_is_retained() {
        let mut accum = TurnAccumulator::default();
        accum.apply_chunk(&json!({
            "choices": [{ "delta": { "reasoning_content": "think " } }]
        }));
        accum.apply_chunk(&json!({
            "choices": [{ "delta": { "reasoning_content": "more" } }]
        }));
        assert_eq!(accum.reasoning, "think more");
    }
}
