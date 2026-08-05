//! Multi-turn agent loop (PLAN M3).
//!
//! Streams completions until the model stops calling tools (or a safety cap
//! fires). Every assistant turn that carries `tool_calls` must echo
//! `reasoning_content` verbatim into the next request — dropping it causes
//! silent `finish_reason: "stop"` mid-task.

use crate::agent::accum::{ToolCallAccum, TurnAccumulator, UsageAccum};
use crate::agent::journal::Journal;
use crate::agent::salvage::salvage_tool_calls;
use crate::agent::stream::{
    AssistantFunctionCall, AssistantToolCall, ChatMessage, ChatRequest, StreamError,
};
use crate::agent::tools::execute_tool;
use crate::filesystem::FileSystem;
use std::cell::RefCell;
use std::rc::Rc;

/// Default hard cap on tool-call iterations.
pub const DEFAULT_MAX_ITERATIONS: usize = 25;

/// How many consecutive identical (name, args) calls trigger a break.
pub const REPETITION_LIMIT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopStopReason {
    /// Model returned finish_reason stop/length with no (salvaged) tool calls.
    Completed,
    /// Hit `max_iterations`.
    IterationCap,
    /// Same tool + identical args seen `REPETITION_LIMIT` times in a row.
    Repetition { name: String, arguments: String },
    /// Caller aborted between or during iterations.
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub result: String,
    /// Paths this tool mutated (subset of the run journal). Used for per-file
    /// diff / revert controls in the transcript.
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptTurn {
    pub reasoning: String,
    pub content: String,
    pub tools: Vec<ToolInvocation>,
    /// True when tool calls were recovered from plain-text content.
    pub salvaged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopOutcome {
    pub turns: Vec<TranscriptTurn>,
    pub final_content: String,
    pub usage: UsageAccum,
    pub stop: LoopStopReason,
    pub iterations: usize,
}

#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub max_iterations: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}

/// Progress events for live UI updates.
#[derive(Debug, Clone)]
pub enum LoopEvent {
    /// Streaming deltas for the in-flight turn.
    Delta {
        content: String,
        reasoning: String,
    },
    /// A turn finished (tools executed if any).
    Turn(TranscriptTurn),
    /// Cumulative usage after a turn.
    Usage(UsageAccum),
    /// Loop terminated.
    Done(LoopOutcome),
}

/// Build the assistant + tool-result messages for one tool-calling turn.
pub fn tool_round_trip(
    fs: &mut FileSystem,
    turn: &TurnAccumulator,
    journal: &mut Journal,
) -> Option<(ChatMessage, Vec<ChatMessage>, Vec<ToolInvocation>)> {
    if turn.tool_calls.is_empty() {
        return None;
    }

    let mut assistant_calls = Vec::with_capacity(turn.tool_calls.len());
    let mut tool_messages = Vec::with_capacity(turn.tool_calls.len());
    let mut invocations = Vec::with_capacity(turn.tool_calls.len());

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

        let content = execute_tool(fs, &name, &tc.arguments, Some(journal));
        let changed_paths = paths_touched_by_call(&name, &tc.arguments, journal);

        invocations.push(ToolInvocation {
            id: id.clone(),
            name: name.clone(),
            arguments: tc.arguments.clone(),
            result: content.clone(),
            changed_paths,
        });
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
    Some((assistant, tool_messages, invocations))
}

fn paths_touched_by_call(name: &str, arguments: &str, journal: &Journal) -> Vec<String> {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    match name {
        "write_file" | "create_directory" | "delete" => {
            if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
                let p = FileSystem::normalize_path(p);
                for e in journal.entries() {
                    if (e.path == p || FileSystem::is_inside(&p, &e.path))
                        && e.prior != e.after
                        && !paths.contains(&e.path)
                    {
                        paths.push(e.path.clone());
                    }
                }
            }
        }
        "rename" => {
            for key in ["old_path", "new_path"] {
                if let Some(p) = args.get(key).and_then(|v| v.as_str()) {
                    let p = FileSystem::normalize_path(p);
                    for e in journal.entries() {
                        if (e.path == p || FileSystem::is_inside(&p, &e.path))
                            && e.prior != e.after
                            && !paths.contains(&e.path)
                        {
                            paths.push(e.path.clone());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    paths
}

/// Resolve tool calls from a completed turn — structured first, then salvage.
pub fn resolve_tool_calls(turn: &TurnAccumulator) -> (Vec<ToolCallAccum>, bool) {
    if !turn.tool_calls.is_empty() {
        return (turn.tool_calls.clone(), false);
    }

    let finish = turn.finish_reason.as_deref().unwrap_or("stop");
    if finish == "stop" || finish == "length" {
        if let Some(salvaged) = salvage_tool_calls(&turn.content) {
            return (salvaged, true);
        }
    }

    (Vec::new(), false)
}

/// Run the multi-turn tool loop.
///
/// `stream` performs one completion against the current request and returns a
/// fully accumulated turn. The UI passes an SSE-backed future; tests pass a
/// fake that yields canned `TurnAccumulator`s. There is exactly one loop
/// implementation — this one.
///
/// `is_aborted` is checked before each iteration so Stop cancels the loop, not
/// just the in-flight HTTP request. (Portable across wasm and host tests;
/// the UI wires it to `AbortController::signal().aborted()`.)
///
/// The VFS is borrowed only while executing tools — never across an await —
/// so the UI can keep an `Rc<RefCell<FileSystem>>` live for other apps.
pub async fn run_agent_loop<F, Fut>(
    fs: &Rc<RefCell<FileSystem>>,
    journal: &Rc<RefCell<Journal>>,
    request: &mut ChatRequest,
    config: &LoopConfig,
    is_aborted: impl Fn() -> bool,
    mut stream: F,
    mut on_event: impl FnMut(LoopEvent),
) -> Result<LoopOutcome, StreamError>
where
    F: FnMut(&ChatRequest) -> Fut,
    Fut: std::future::Future<Output = Result<TurnAccumulator, StreamError>>,
{
    let mut turns = Vec::new();
    let mut usage = UsageAccum::default();
    let mut iterations = 0usize;
    let mut recent: Vec<(String, String)> = Vec::new();
    let mut final_content = String::new();

    loop {
        if is_aborted() {
            let outcome = LoopOutcome {
                turns,
                final_content,
                usage,
                stop: LoopStopReason::Aborted,
                iterations,
            };
            on_event(LoopEvent::Done(outcome.clone()));
            return Ok(outcome);
        }

        if iterations >= config.max_iterations {
            let outcome = LoopOutcome {
                turns,
                final_content,
                usage,
                stop: LoopStopReason::IterationCap,
                iterations,
            };
            on_event(LoopEvent::Done(outcome.clone()));
            return Ok(outcome);
        }

        iterations += 1;

        let mut turn = stream(request).await?;
        usage.absorb(&turn.usage);
        on_event(LoopEvent::Usage(usage.clone()));
        on_event(LoopEvent::Delta {
            content: turn.content.clone(),
            reasoning: turn.reasoning.clone(),
        });

        let (calls, salvaged) = resolve_tool_calls(&turn);
        if salvaged {
            // Replace prose-as-call with structured calls; clear content so it
            // is not shown as the final answer.
            turn.tool_calls = calls.clone();
            turn.content.clear();
            turn.finish_reason = Some("tool_calls".into());
        }

        if turn.tool_calls.is_empty() {
            final_content = turn.content.clone();
            let transcript = TranscriptTurn {
                reasoning: turn.reasoning.clone(),
                content: turn.content.clone(),
                tools: Vec::new(),
                salvaged: false,
            };
            turns.push(transcript.clone());
            on_event(LoopEvent::Turn(transcript));

            let outcome = LoopOutcome {
                turns,
                final_content,
                usage,
                stop: LoopStopReason::Completed,
                iterations,
            };
            on_event(LoopEvent::Done(outcome.clone()));
            return Ok(outcome);
        }

        // Repetition detector — consecutive identical (name, args).
        for tc in &turn.tool_calls {
            let name = tc.name.clone().unwrap_or_default();
            let args = tc.arguments.clone();
            recent.push((name.clone(), args.clone()));
            if trailing_identical_count(&recent) >= REPETITION_LIMIT {
                // Still execute this turn's tools so the transcript is honest,
                // then stop before the next request.
                let (assistant, tool_msgs, tools) = {
                    let mut filesystem = fs.borrow_mut();
                    let mut j = journal.borrow_mut();
                    tool_round_trip(&mut filesystem, &turn, &mut j)
                        .expect("tool_calls non-empty")
                };
                let transcript = TranscriptTurn {
                    reasoning: turn.reasoning.clone(),
                    content: turn.content.clone(),
                    tools,
                    salvaged,
                };
                turns.push(transcript.clone());
                on_event(LoopEvent::Turn(transcript));
                request.messages.push(assistant);
                request.messages.extend(tool_msgs);

                let outcome = LoopOutcome {
                    turns,
                    final_content,
                    usage,
                    stop: LoopStopReason::Repetition {
                        name,
                        arguments: args,
                    },
                    iterations,
                };
                on_event(LoopEvent::Done(outcome.clone()));
                return Ok(outcome);
            }
        }

        let (assistant, tool_msgs, tools) = {
            let mut filesystem = fs.borrow_mut();
            let mut j = journal.borrow_mut();
            tool_round_trip(&mut filesystem, &turn, &mut j).expect("tool_calls non-empty")
        };
        let transcript = TranscriptTurn {
            reasoning: turn.reasoning.clone(),
            content: turn.content.clone(),
            tools,
            salvaged,
        };
        turns.push(transcript.clone());
        on_event(LoopEvent::Turn(transcript));

        // Append-only history — never rewrite the front of the array (cache).
        request.messages.push(assistant);
        request.messages.extend(tool_msgs);
    }
}

fn trailing_identical_count(recent: &[(String, String)]) -> usize {
    let Some(last) = recent.last() else {
        return 0;
    };
    recent
        .iter()
        .rev()
        .take_while(|sig| *sig == last)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream::ChatRequest;
    use crate::agent::tools::tool_definitions;
    use crate::filesystem::FileSystem;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn test_fs() -> Rc<RefCell<FileSystem>> {
        Rc::new(RefCell::new(FileSystem::default()))
    }

    fn test_journal() -> Rc<RefCell<Journal>> {
        Rc::new(RefCell::new(Journal::new()))
    }

    fn canned_tool_turn(
        id: &str,
        name: &str,
        args: &str,
        reasoning: &str,
        usage: UsageAccum,
    ) -> TurnAccumulator {
        TurnAccumulator {
            content: String::new(),
            reasoning: reasoning.into(),
            tool_calls: vec![ToolCallAccum {
                id: Some(id.into()),
                name: Some(name.into()),
                arguments: args.into(),
            }],
            finish_reason: Some("tool_calls".into()),
            usage,
        }
    }

    fn canned_stop(content: &str, reasoning: &str, usage: UsageAccum) -> TurnAccumulator {
        TurnAccumulator {
            content: content.into(),
            reasoning: reasoning.into(),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
            usage,
        }
    }

    #[test]
    fn three_iteration_run_stops_on_finish_reason_stop() {
        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let script = vec![
            canned_tool_turn(
                "c1",
                "create_directory",
                r#"{"path":"/tmp/m3","create_parents":true}"#,
                "mkdir",
                UsageAccum {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    prompt_cache_hit_tokens: 0,
                    prompt_cache_miss_tokens: 10,
                },
            ),
            canned_tool_turn(
                "c2",
                "write_file",
                r#"{"path":"/home/documents/m3.txt","content":"x"}"#,
                "write",
                UsageAccum {
                    prompt_tokens: 20,
                    completion_tokens: 5,
                    prompt_cache_hit_tokens: 15,
                    prompt_cache_miss_tokens: 5,
                },
            ),
            canned_stop(
                "done",
                "final",
                UsageAccum {
                    prompt_tokens: 30,
                    completion_tokens: 3,
                    prompt_cache_hit_tokens: 25,
                    prompt_cache_miss_tokens: 5,
                },
            ),
        ];
        let mut idx = 0;
        let outcome = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &LoopConfig::default(),
            || false,
            |_| {
                let t = script[idx].clone();
                idx += 1;
                async move { Ok(t) }
            },
            |_| {},
        ))
        .unwrap();

        assert_eq!(outcome.stop, LoopStopReason::Completed);
        assert_eq!(outcome.iterations, 3);
        assert_eq!(outcome.final_content, "done");
        assert_eq!(outcome.turns.len(), 3);
        assert_eq!(outcome.turns[0].tools.len(), 1);
        assert_eq!(outcome.turns[1].tools.len(), 1);
        assert!(outcome.turns[2].tools.is_empty());
    }

    #[test]
    fn iteration_cap_fires_at_limit() {
        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let config = LoopConfig { max_iterations: 2 };
        let mut n = 0;
        let outcome = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &config,
            || false,
            |_| {
                n += 1;
                // Alternate paths so repetition detector does not fire first.
                let path = format!(r#"{{"path":"/tmp/cap{n}","create_parents":true}}"#);
                let turn = canned_tool_turn(
                    &format!("c{n}"),
                    "create_directory",
                    &path,
                    "again",
                    UsageAccum::default(),
                );
                async move { Ok(turn) }
            },
            |_| {},
        ))
        .unwrap();

        assert_eq!(outcome.stop, LoopStopReason::IterationCap);
        assert_eq!(outcome.iterations, 2);
        assert_eq!(outcome.turns.len(), 2);
    }

    #[test]
    fn repetition_detector_breaks_on_third_identical_tool_args() {
        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let args = r#"{"path":"/home/documents/welcome.txt"}"#;
        let mut n = 0;
        let outcome = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &LoopConfig::default(),
            || false,
            |_| {
                n += 1;
                let turn = canned_tool_turn(
                    &format!("r{n}"),
                    "read_file",
                    args,
                    "reread",
                    UsageAccum::default(),
                );
                async move { Ok(turn) }
            },
            |_| {},
        ))
        .unwrap();

        match &outcome.stop {
            LoopStopReason::Repetition { name, arguments } => {
                assert_eq!(name, "read_file");
                assert_eq!(arguments, args);
            }
            other => panic!("expected Repetition, got {other:?}"),
        }
        assert_eq!(outcome.iterations, 3);
    }

    #[test]
    fn assistant_reasoning_content_appears_in_serialized_next_request() {
        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let script = vec![
            canned_tool_turn(
                "c1",
                "list_directory",
                r#"{"path":"/home"}"#,
                "I should list /home first",
                UsageAccum::default(),
            ),
            canned_stop("listed", "", UsageAccum::default()),
        ];
        let mut idx = 0;
        let mut saw_reasoning_on_wire = false;

        let _ = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &LoopConfig::default(),
            || false,
            |req| {
                if idx == 1 {
                    // Second request must carry reasoning_content from turn 1.
                    let body = serde_json::to_string(req).expect("serialize");
                    assert!(
                        body.contains("\"reasoning_content\":\"I should list /home first\""),
                        "wire body missing reasoning_content: {body}"
                    );
                    saw_reasoning_on_wire = true;
                }
                let t = script[idx].clone();
                idx += 1;
                async move { Ok(t) }
            },
            |_| {},
        ))
        .unwrap();

        assert!(saw_reasoning_on_wire);
    }

    #[test]
    fn tool_messages_one_per_call_in_order() {
        let mut fs = FileSystem::default();
        let turn = TurnAccumulator {
            content: String::new(),
            reasoning: "both".into(),
            tool_calls: vec![
                ToolCallAccum {
                    id: Some("call_a".into()),
                    name: Some("create_directory".into()),
                    arguments: r#"{"path":"/tmp/ord","create_parents":true}"#.into(),
                },
                ToolCallAccum {
                    id: Some("call_b".into()),
                    name: Some("write_file".into()),
                    arguments: r#"{"path":"/home/documents/ord.txt","content":"z"}"#.into(),
                },
            ],
            finish_reason: Some("tool_calls".into()),
            usage: UsageAccum::default(),
        };
        let (assistant, tool_msgs, inv) = tool_round_trip(&mut fs, &turn, &mut Journal::new()).unwrap();
        assert_eq!(tool_msgs.len(), 2);
        assert_eq!(inv.len(), 2);
        assert_eq!(tool_msgs[0].tool_call_id.as_deref(), Some("call_a"));
        assert_eq!(tool_msgs[1].tool_call_id.as_deref(), Some("call_b"));
        let ids: Vec<_> = assistant
            .tool_calls
            .as_ref()
            .unwrap()
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(ids, ["call_a", "call_b"]);
    }

    #[test]
    fn loop_salvages_plain_text_tool_call() {
        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let script = vec![
            TurnAccumulator {
                content: r#"write_file({"path":"/home/documents/salv.txt","content":"ok"})"#.into(),
                reasoning: "leak".into(),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
                usage: UsageAccum::default(),
            },
            canned_stop("wrote it", "", UsageAccum::default()),
        ];
        let mut idx = 0;
        let outcome = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &LoopConfig::default(),
            || false,
            |_| {
                let t = script[idx].clone();
                idx += 1;
                async move { Ok(t) }
            },
            |_| {},
        ))
        .unwrap();

        assert_eq!(outcome.stop, LoopStopReason::Completed);
        assert!(outcome.turns[0].salvaged);
        assert_eq!(outcome.turns[0].tools[0].name, "write_file");
        assert!(fs.borrow().exists("/home/documents/salv.txt"));
        assert_eq!(outcome.final_content, "wrote it");
    }

    #[test]
    fn usage_accumulates_across_iterations() {
        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let script = vec![
            canned_tool_turn(
                "c1",
                "list_directory",
                r#"{"path":"/home"}"#,
                "a",
                UsageAccum {
                    prompt_tokens: 100,
                    completion_tokens: 10,
                    prompt_cache_hit_tokens: 60,
                    prompt_cache_miss_tokens: 40,
                },
            ),
            canned_stop(
                "ok",
                "b",
                UsageAccum {
                    prompt_tokens: 150,
                    completion_tokens: 20,
                    prompt_cache_hit_tokens: 120,
                    prompt_cache_miss_tokens: 30,
                },
            ),
        ];
        let mut idx = 0;
        let outcome = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &LoopConfig::default(),
            || false,
            |_| {
                let t = script[idx].clone();
                idx += 1;
                async move { Ok(t) }
            },
            |_| {},
        ))
        .unwrap();

        assert_eq!(outcome.usage.prompt_cache_hit_tokens, 180);
        assert_eq!(outcome.usage.prompt_cache_miss_tokens, 70);
        assert_eq!(outcome.usage.completion_tokens, 30);
        assert_eq!(outcome.usage.prompt_tokens, 250);
    }

    #[test]
    fn abort_between_iterations_stops_loop() {
        use std::cell::Cell;

        let fs = test_fs();
        let journal = test_journal();
        let mut request = ChatRequest::streaming_with_tools("go", tool_definitions());
        let n = Cell::new(0);
        let outcome = pollster::block_on(run_agent_loop(
            &fs,
            &journal,
            &mut request,
            &LoopConfig::default(),
            || n.get() >= 1, // abort before second iteration
            |_| {
                n.set(n.get() + 1);
                let turn = canned_tool_turn(
                    "c1",
                    "list_directory",
                    r#"{"path":"/home"}"#,
                    "x",
                    UsageAccum::default(),
                );
                async move { Ok(turn) }
            },
            |_| {},
        ))
        .unwrap();

        assert_eq!(outcome.stop, LoopStopReason::Aborted);
        assert_eq!(outcome.iterations, 1);
    }

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
            usage: UsageAccum::default(),
        };

        let (assistant, tool_msgs, inv) =
            tool_round_trip(&mut fs, &turn, &mut Journal::new()).expect("tool calls present");
        assert_eq!(assistant.role, "assistant");
        assert_eq!(
            assistant.reasoning_content.as_deref(),
            Some("I should create the file")
        );
        assert_eq!(tool_msgs.len(), 2);
        assert_eq!(inv.len(), 2);
        assert_eq!(tool_msgs[0].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_msgs[1].tool_call_id.as_deref(), Some("call_2"));
    }

    #[test]
    fn round_trip_none_without_tool_calls() {
        let mut fs = FileSystem::default();
        let turn = TurnAccumulator {
            content: "hello".into(),
            reasoning: "think".into(),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
            usage: UsageAccum::default(),
        };
        assert!(tool_round_trip(&mut fs, &turn, &mut Journal::new()).is_none());
    }
}
