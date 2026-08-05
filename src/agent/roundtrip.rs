//! Helper: turn a completed accumulator into the follow-up messages for one
//! tool round-trip (assistant + tool results). M3 owns the multi-turn loop.

use crate::agent::accum::TurnAccumulator;
use crate::agent::stream::{AssistantFunctionCall, AssistantToolCall, ChatMessage};
use crate::agent::tools::execute_tool;
use crate::filesystem::FileSystem;

/// If the turn ended in `tool_calls`, execute each tool in order and return
/// the assistant message (with `reasoning_content`) plus one tool result per
/// call. Returns `None` when there are no tool calls to run.
pub fn tool_round_trip(
    fs: &mut FileSystem,
    turn: &TurnAccumulator,
) -> Option<(ChatMessage, Vec<ChatMessage>)> {
    if turn.tool_calls.is_empty() {
        return None;
    }

    let mut assistant_calls = Vec::with_capacity(turn.tool_calls.len());
    let mut tool_messages = Vec::with_capacity(turn.tool_calls.len());

    for tc in &turn.tool_calls {
        let id = tc
            .id
            .clone()
            .unwrap_or_else(|| "missing_tool_call_id".into());
        let name = tc.name.clone().unwrap_or_else(|| "unknown".into());

        assistant_calls.push(AssistantToolCall {
            id: id.clone(),
            type_: "function".into(),
            function: AssistantFunctionCall {
                name: name.clone(),
                arguments: tc.arguments.clone(),
            },
        });

        let content = execute_tool(fs, &name, &tc.arguments);
        tool_messages.push(ChatMessage::tool_result(id, content));
    }

    let content = if turn.content.is_empty() {
        None
    } else {
        Some(turn.content.clone())
    };
    let reasoning = if turn.reasoning.is_empty() {
        None
    } else {
        Some(turn.reasoning.clone())
    };

    let assistant = ChatMessage::assistant_with_tools(content, reasoning, assistant_calls);
    Some((assistant, tool_messages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::accum::ToolCallAccum;
    use crate::filesystem::FileSystem;

    #[test]
    fn round_trip_emits_tool_messages_in_call_order_with_reasoning() {
        let mut fs = FileSystem::default();
        let turn = TurnAccumulator {
            content: String::new(),
            reasoning: "I should create the file".into(),
            tool_calls: vec![
                ToolCallAccum {
                    id: Some("call_1".into()),
                    name: Some("create_directory".into()),
                    arguments: r#"{"path":"/tmp/rt","create_parents":true}"#.into(),
                },
                ToolCallAccum {
                    id: Some("call_2".into()),
                    name: Some("write_file".into()),
                    arguments: r#"{"path":"/home/documents/rt.txt","content":"hi"}"#.into(),
                },
            ],
            finish_reason: Some("tool_calls".into()),
        };

        let (assistant, tools) = tool_round_trip(&mut fs, &turn).expect("tool calls present");
        assert_eq!(assistant.role, "assistant");
        assert_eq!(
            assistant.reasoning_content.as_deref(),
            Some("I should create the file")
        );
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tools[1].tool_call_id.as_deref(), Some("call_2"));
        assert!(tools[0].content.as_ref().unwrap().contains("created directory"));
        assert!(tools[1].content.as_ref().unwrap().contains("wrote 2 bytes"));
    }

    #[test]
    fn round_trip_none_without_tool_calls() {
        let mut fs = FileSystem::default();
        let turn = TurnAccumulator {
            content: "hello".into(),
            reasoning: "think".into(),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
        };
        assert!(tool_round_trip(&mut fs, &turn).is_none());
    }
}
