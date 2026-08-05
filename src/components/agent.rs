use std::cell::RefCell;
use std::rc::Rc;

use yew::prelude::*;
use web_sys::{HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};

use crate::agent::{
    load_api_key, run_agent_loop, save_api_key, stream_completion, tool_definitions, ChatRequest,
    LoopConfig, LoopEvent, LoopStopReason, SseEvent, StreamError, TranscriptTurn, TurnAccumulator,
    UsageAccum,
};
use crate::filesystem::FileSystem;

#[derive(Properties, Clone, PartialEq)]
pub struct AgentProps {
    pub fs: Rc<RefCell<FileSystem>>,
}

pub struct Agent {
    prompt: String,
    api_key: String,
    transcript: Vec<TranscriptTurn>,
    live_content: String,
    live_reasoning: String,
    usage: UsageAccum,
    status: Option<String>,
    error: Option<String>,
    streaming: bool,
    abort: Option<web_sys::AbortController>,
    reasoning_open: Vec<bool>,
}

pub enum AgentMsg {
    SetPrompt(String),
    SetApiKey(String),
    Submit,
    Stop,
    Delta { content: String, reasoning: String },
    Turn(TranscriptTurn),
    Usage(UsageAccum),
    StreamEnd {
        result: Result<(), StreamError>,
        status: Option<String>,
    },
    ToggleReasoning(usize),
}

impl Component for Agent {
    type Message = AgentMsg;
    type Properties = AgentProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            prompt: String::new(),
            api_key: load_api_key().unwrap_or_default(),
            transcript: Vec::new(),
            live_content: String::new(),
            live_reasoning: String::new(),
            usage: UsageAccum::default(),
            status: None,
            error: None,
            streaming: false,
            abort: None,
            reasoning_open: Vec::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            AgentMsg::SetPrompt(p) => {
                self.prompt = p;
                true
            }
            AgentMsg::SetApiKey(key) => {
                self.api_key = key.clone();
                let _ = save_api_key(&key);
                true
            }
            AgentMsg::Submit => {
                if self.streaming || self.prompt.trim().is_empty() {
                    return false;
                }
                if self.api_key.trim().is_empty() {
                    self.error = Some("Enter an API key first".into());
                    return true;
                }

                self.transcript.clear();
                self.reasoning_open.clear();
                self.live_content.clear();
                self.live_reasoning.clear();
                self.usage = UsageAccum::default();
                self.status = None;
                self.error = None;
                self.streaming = true;

                let abort = web_sys::AbortController::new().expect("AbortController");
                self.abort = Some(abort.clone());

                let link = ctx.link().clone();
                let api_key = self.api_key.clone();
                let prompt = self.prompt.clone();
                let fs = Rc::clone(&ctx.props().fs);

                wasm_bindgen_futures::spawn_local(async move {
                    let tools = tool_definitions();
                    let mut request = ChatRequest::streaming_with_tools(&prompt, tools);
                    let config = LoopConfig::default();
                    let abort_flag = abort.clone();
                    let api_key = api_key.clone();
                    let abort_for_stream = abort.clone();
                    let link_for_stream = link.clone();

                    let outcome = run_agent_loop(
                        &fs,
                        &mut request,
                        &config,
                        || abort_flag.signal().aborted(),
                        move |req| {
                            let api_key = api_key.clone();
                            let abort = abort_for_stream.clone();
                            let link = link_for_stream.clone();
                            let req = req.clone();
                            async move {
                                if abort.signal().aborted() {
                                    return Err(StreamError::Aborted);
                                }
                                let mut accum = TurnAccumulator::default();
                                stream_completion(&api_key, &req, &abort, |event| {
                                    if let SseEvent::Data(chunk) = event {
                                        accum.apply_chunk(&chunk);
                                        link.send_message(AgentMsg::Delta {
                                            content: accum.content.clone(),
                                            reasoning: accum.reasoning.clone(),
                                        });
                                    }
                                })
                                .await?;
                                Ok(accum)
                            }
                        },
                        |event| match event {
                            LoopEvent::Delta { content, reasoning } => {
                                link.send_message(AgentMsg::Delta { content, reasoning });
                            }
                            LoopEvent::Turn(turn) => {
                                link.send_message(AgentMsg::Turn(turn));
                            }
                            LoopEvent::Usage(u) => {
                                link.send_message(AgentMsg::Usage(u));
                            }
                            LoopEvent::Done(_) => {}
                        },
                    )
                    .await;

                    let (result, status) = match outcome {
                        Ok(o) => {
                            let status = match o.stop {
                                LoopStopReason::Completed => None,
                                LoopStopReason::IterationCap => Some(format!(
                                    "Stopped: hit iteration cap ({})",
                                    o.iterations
                                )),
                                LoopStopReason::Repetition { name, arguments } => Some(format!(
                                    "Stopped: repeated {name} with identical args ({arguments})"
                                )),
                                LoopStopReason::Aborted => Some("Stopped by user".into()),
                            };
                            (Ok(()), status)
                        }
                        Err(StreamError::Aborted) => (Ok(()), Some("Stopped by user".into())),
                        Err(e) => (Err(e), None),
                    };
                    link.send_message(AgentMsg::StreamEnd { result, status });
                });

                true
            }
            AgentMsg::Stop => {
                if let Some(abort) = &self.abort {
                    abort.abort();
                }
                // Leave streaming=true until StreamEnd so Submit cannot race.
                true
            }
            AgentMsg::Delta { content, reasoning } => {
                self.live_content = content;
                self.live_reasoning = reasoning;
                true
            }
            AgentMsg::Turn(turn) => {
                self.live_content.clear();
                self.live_reasoning.clear();
                self.reasoning_open.push(false);
                self.transcript.push(turn);
                true
            }
            AgentMsg::Usage(u) => {
                self.usage = u;
                true
            }
            AgentMsg::StreamEnd { result, status } => {
                self.streaming = false;
                self.abort = None;
                self.live_content.clear();
                self.live_reasoning.clear();
                self.status = status;
                if let Err(e) = result {
                    if !matches!(e, StreamError::Aborted) {
                        self.error = Some(e.user_message().to_string());
                    }
                }
                true
            }
            AgentMsg::ToggleReasoning(i) => {
                if let Some(open) = self.reasoning_open.get_mut(i) {
                    *open = !*open;
                }
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_prompt = ctx.link().callback(|e: InputEvent| {
            let input: HtmlTextAreaElement = e.target_unchecked_into();
            AgentMsg::SetPrompt(input.value())
        });
        let on_key_submit = {
            let link = ctx.link().clone();
            Callback::from(move |e: KeyboardEvent| {
                if e.key() == "Enter" && !e.shift_key() {
                    e.prevent_default();
                    link.send_message(AgentMsg::Submit);
                }
            })
        };
        let on_api_key = ctx.link().callback(|e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            AgentMsg::SetApiKey(input.value())
        });
        let on_submit = ctx.link().callback(|_| AgentMsg::Submit);
        let on_stop = ctx.link().callback(|_| AgentMsg::Stop);

        let usage = &self.usage;
        let hit = usage.prompt_cache_hit_tokens;
        let miss = usage.prompt_cache_miss_tokens;
        let cache_total = hit + miss;
        let hit_pct = if cache_total == 0 {
            0
        } else {
            (hit * 100) / cache_total
        };

        html! {
            <div class="agent-app">
                <div class="agent-controls">
                    <label class="agent-label">
                        { "API Key" }
                        <input
                            class="agent-input agent-key-input"
                            type="password"
                            placeholder="sk-..."
                            value={self.api_key.clone()}
                            oninput={on_api_key}
                            disabled={self.streaming}
                        />
                    </label>
                    <label class="agent-label">
                        { "Prompt" }
                        <textarea
                            class="agent-input agent-prompt"
                            placeholder="Ask DeepSeek..."
                            value={self.prompt.clone()}
                            oninput={on_prompt}
                            onkeydown={on_key_submit}
                            disabled={self.streaming}
                        />
                    </label>
                    <div class="agent-buttons">
                        <button
                            class="agent-btn agent-btn-primary"
                            onclick={on_submit}
                            disabled={self.streaming || self.prompt.trim().is_empty()}
                        >
                            { "Send" }
                        </button>
                        <button
                            class="agent-btn agent-btn-stop"
                            onclick={on_stop}
                            disabled={!self.streaming}
                        >
                            { "Stop" }
                        </button>
                    </div>
                </div>

                <div class="agent-usage">
                    <span>{ format!("cache hit {hit}") }</span>
                    <span class="agent-usage-sep">{ "·" }</span>
                    <span>{ format!("miss {miss}") }</span>
                    <span class="agent-usage-sep">{ "·" }</span>
                    <span>{ format!("{hit_pct}% hit") }</span>
                    <span class="agent-usage-sep">{ "·" }</span>
                    <span>{ format!("out {}", usage.completion_tokens) }</span>
                </div>

                if let Some(err) = &self.error {
                    <div class="agent-error">{ err }</div>
                }
                if let Some(status) = &self.status {
                    <div class="agent-status">{ status }</div>
                }

                <div class="agent-transcript">
                    {
                        self.transcript.iter().enumerate().map(|(i, turn)| {
                            let open = self.reasoning_open.get(i).copied().unwrap_or(false);
                            let toggle = ctx.link().callback(move |_| AgentMsg::ToggleReasoning(i));
                            render_turn(i, turn, open, toggle)
                        }).collect::<Html>()
                    }
                    if self.streaming && (!self.live_content.is_empty() || !self.live_reasoning.is_empty()) {
                        <div class="agent-turn agent-turn-live">
                            if !self.live_reasoning.is_empty() {
                                <div class="agent-turn-reasoning">
                                    <div class="agent-turn-label">{ "Reasoning (live)" }</div>
                                    <pre class="agent-turn-body">{ &self.live_reasoning }</pre>
                                </div>
                            }
                            if !self.live_content.is_empty() {
                                <div class="agent-turn-content">
                                    <div class="agent-turn-label">{ "Response (live)" }</div>
                                    <pre class="agent-turn-body">{ &self.live_content }</pre>
                                </div>
                            }
                        </div>
                    }
                </div>
            </div>
        }
    }
}

fn render_turn(
    index: usize,
    turn: &TranscriptTurn,
    reasoning_open: bool,
    toggle: Callback<MouseEvent>,
) -> Html {
    let salvaged = if turn.salvaged {
        html! { <span class="agent-salvaged">{ "salvaged" }</span> }
    } else {
        html! {}
    };

    html! {
        <div class="agent-turn">
            <div class="agent-turn-index">{ format!("Turn {}", index + 1) }{ salvaged }</div>

            if !turn.reasoning.is_empty() {
                <div class="agent-turn-reasoning">
                    <button type="button" class="agent-turn-label agent-turn-toggle" onclick={toggle}>
                        { if reasoning_open { "▾ Reasoning" } else { "▸ Reasoning" } }
                    </button>
                    if reasoning_open {
                        <pre class="agent-turn-body">{ &turn.reasoning }</pre>
                    }
                </div>
            }

            {
                turn.tools.iter().map(|t| {
                    html! {
                        <div class="agent-turn-tool">
                            <div class="agent-turn-label">
                                { format!("{}({})", t.name, t.arguments) }
                            </div>
                            <pre class="agent-turn-body agent-tool-result">{ &t.result }</pre>
                        </div>
                    }
                }).collect::<Html>()
            }

            if !turn.content.is_empty() {
                <div class="agent-turn-content">
                    <div class="agent-turn-label">{ "Response" }</div>
                    <pre class="agent-turn-body">{ &turn.content }</pre>
                </div>
            }
        </div>
    }
}
