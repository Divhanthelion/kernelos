//! Turn accumulator — merges streaming delta fragments into a complete turn.

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallAccum {
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

/// Token / cache usage from `stream_options.include_usage` chunks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageAccum {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub prompt_cache_hit_tokens: u64,
    pub prompt_cache_miss_tokens: u64,
}

impl UsageAccum {
    pub fn add_from_chunk(&mut self, chunk: &Value) {
        let Some(usage) = chunk.get("usage") else {
            return;
        };
        // Within a single turn, later chunks overwrite (DeepSeek sends one
        // definitive usage object on the trailing chunk). Across loop
        // iterations, callers sum via `absorb`.
        if let Some(v) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
            self.prompt_tokens = v;
        }
        if let Some(v) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
            self.completion_tokens = v;
        }
        if let Some(v) = usage.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()) {
            self.prompt_cache_hit_tokens = v;
        }
        if let Some(v) = usage.get("prompt_cache_miss_tokens").and_then(|v| v.as_u64()) {
            self.prompt_cache_miss_tokens = v;
        }
    }

    pub fn absorb(&mut self, other: &UsageAccum) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.prompt_cache_hit_tokens += other.prompt_cache_hit_tokens;
        self.prompt_cache_miss_tokens += other.prompt_cache_miss_tokens;
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.prompt_cache_hit_tokens + self.prompt_cache_miss_tokens;
        if total == 0 {
            0.0
        } else {
            self.prompt_cache_hit_tokens as f64 / total as f64
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnAccumulator {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCallAccum>,
    pub finish_reason: Option<String>,
    pub usage: UsageAccum,
}

impl TurnAccumulator {
    /// Apply one `chat.completion.chunk` JSON object.
    pub fn apply_chunk(&mut self, chunk: &Value) {
        self.usage.add_from_chunk(chunk);

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
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "prompt_cache_hit_tokens": 8,
                "prompt_cache_miss_tokens": 2
            }
        }));
        assert!(accum.content.is_empty());
        assert!(accum.reasoning.is_empty());
        assert!(accum.tool_calls.is_empty());
        assert!(accum.finish_reason.is_none());
        assert_eq!(accum.usage.prompt_tokens, 10);
        assert_eq!(accum.usage.completion_tokens, 5);
        assert_eq!(accum.usage.prompt_cache_hit_tokens, 8);
        assert_eq!(accum.usage.prompt_cache_miss_tokens, 2);
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

    #[test]
    fn usage_absorbs_across_iterations() {
        let mut total = UsageAccum::default();
        total.absorb(&UsageAccum {
            prompt_tokens: 100,
            completion_tokens: 20,
            prompt_cache_hit_tokens: 80,
            prompt_cache_miss_tokens: 20,
        });
        total.absorb(&UsageAccum {
            prompt_tokens: 50,
            completion_tokens: 10,
            prompt_cache_hit_tokens: 40,
            prompt_cache_miss_tokens: 10,
        });
        assert_eq!(total.prompt_cache_hit_tokens, 120);
        assert_eq!(total.prompt_cache_miss_tokens, 30);
        assert_eq!(total.completion_tokens, 30);
        assert!((total.cache_hit_rate() - 0.8).abs() < f64::EPSILON);
    }
}
