use yew::prelude::*;
use web_sys::{HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};

use crate::agent::{
    load_api_key, save_api_key, stream_completion, ChatRequest, SseEvent, StreamError,
    TurnAccumulator,
};

pub struct Agent {
    prompt: String,
    api_key: String,
    content: String,
    reasoning: String,
    error: Option<String>,
    streaming: bool,
    abort: Option<web_sys::AbortController>,
}

pub enum AgentMsg {
    SetPrompt(String),
    SetApiKey(String),
    Submit,
    Stop,
    Delta { content: String, reasoning: String },
    StreamEnd(Result<(), StreamError>),
}

impl Component for Agent {
    type Message = AgentMsg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            prompt: String::new(),
            api_key: load_api_key().unwrap_or_default(),
            content: String::new(),
            reasoning: String::new(),
            error: None,
            streaming: false,
            abort: None,
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

                self.content.clear();
                self.reasoning.clear();
                self.error = None;
                self.streaming = true;

                let abort = web_sys::AbortController::new().expect("AbortController");
                self.abort = Some(abort.clone());

                let link = ctx.link().clone();
                let api_key = self.api_key.clone();
                let prompt = self.prompt.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    let request = ChatRequest::streaming(&prompt);
                    let mut accum = TurnAccumulator::default();

                    let result = stream_completion(&api_key, &request, &abort, |event| {
                        if let SseEvent::Data(chunk) = event {
                            accum.apply_chunk(&chunk);
                            link.send_message(AgentMsg::Delta {
                                content: accum.content.clone(),
                                reasoning: accum.reasoning.clone(),
                            });
                        }
                    })
                    .await;

                    link.send_message(AgentMsg::StreamEnd(result));
                });

                true
            }
            AgentMsg::Stop => {
                if let Some(abort) = &self.abort {
                    abort.abort();
                }
                self.streaming = false;
                true
            }
            AgentMsg::Delta { content, reasoning } => {
                self.content = content;
                self.reasoning = reasoning;
                true
            }
            AgentMsg::StreamEnd(result) => {
                self.streaming = false;
                self.abort = None;
                if let Err(e) = result {
                    if !matches!(e, StreamError::Aborted) {
                        self.error = Some(e.user_message().to_string());
                    }
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

                if let Some(err) = &self.error {
                    <div class="agent-error">{ err }</div>
                }

                <div class="agent-output">
                    <div class="agent-pane agent-reasoning-pane">
                        <div class="agent-pane-header">{ "Reasoning" }</div>
                        <pre class="agent-pane-body">{ &self.reasoning }</pre>
                    </div>
                    <div class="agent-pane agent-content-pane">
                        <div class="agent-pane-header">{ "Response" }</div>
                        <pre class="agent-pane-body">{ &self.content }</pre>
                    </div>
                </div>
            </div>
        }
    }
}
