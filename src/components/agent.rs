use std::cell::RefCell;
use std::rc::Rc;

use yew::prelude::*;
use web_sys::{HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};

use crate::agent::{
    load_api_key, run_agent_loop, save_api_key, stream_completion, tool_definitions, ChatRequest,
    FileDelta, Journal, LoopConfig, LoopEvent, LoopStopReason, PathState, SseEvent, StreamError,
    TranscriptTurn, TurnAccumulator, UsageAccum,
};
use crate::filesystem::FileSystem;

#[derive(Properties, Clone, PartialEq)]
pub struct AgentProps {
    pub fs: Rc<RefCell<FileSystem>>,
    /// Bumped by Desktop when the VFS changes so sibling apps (editor, explorer)
    /// can re-read. Agent emits via `on_vfs_mutated` after mutating tools / undo.
    #[prop_or_default]
    pub on_vfs_mutated: Callback<()>,
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
    /// Diff panels open per (turn_idx, path).
    diff_open: Vec<(usize, String)>,
    journal: Rc<RefCell<Journal>>,
    /// True after a run that left a non-empty set of changed paths.
    undo_available: bool,
}

pub enum AgentMsg {
    SetPrompt(String),
    SetApiKey(String),
    Submit,
    Stop,
    UndoRun,
    RevertPath(String),
    ToggleDiff { turn: usize, path: String },
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
            diff_open: Vec::new(),
            journal: Rc::new(RefCell::new(Journal::new())),
            undo_available: false,
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
                self.diff_open.clear();
                self.live_content.clear();
                self.live_reasoning.clear();
                self.usage = UsageAccum::default();
                self.status = None;
                self.error = None;
                self.streaming = true;
                self.undo_available = false;
                *self.journal.borrow_mut() = Journal::new();

                let abort = web_sys::AbortController::new().expect("AbortController");
                self.abort = Some(abort.clone());

                let link = ctx.link().clone();
                let api_key = self.api_key.clone();
                let prompt = self.prompt.clone();
                let fs = Rc::clone(&ctx.props().fs);
                let journal = Rc::clone(&self.journal);

                wasm_bindgen_futures::spawn_local(async move {
                    // Optional runtimes — never abort the agent loop if a 5–20MB
                    // asset fails to load. Tools degrade to clean "not loaded"
                    // errors; surface a cheap status note when useful.
                    let mut load_notes: Vec<String> = Vec::new();
                    if let Err(e) = crate::agent::ensure_typescript_loaded().await {
                        load_notes.push(format!("TypeScript unavailable: {e}"));
                    }
                    if let Err(e) = crate::agent::ensure_python_loaded().await {
                        load_notes.push(format!("Python unavailable: {e}"));
                    }
                    let load_note = if load_notes.is_empty() {
                        None
                    } else {
                        Some(load_notes.join(" · "))
                    };

                    let tools = tool_definitions();
                    let mut request = ChatRequest::streaming_with_tools(&prompt, tools);
                    let config = LoopConfig::default();
                    let abort_flag = abort.clone();
                    let api_key = api_key.clone();
                    let abort_for_stream = abort.clone();
                    let link_for_stream = link.clone();

                    let outcome = run_agent_loop(
                        &fs,
                        &journal,
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
                    let status = match (status, load_note) {
                        (Some(s), Some(w)) => Some(format!("{s} · {w}")),
                        (None, Some(w)) => Some(w),
                        (s, None) => s,
                    };
                    link.send_message(AgentMsg::StreamEnd { result, status });
                });

                true
            }
            AgentMsg::Stop => {
                if let Some(abort) = &self.abort {
                    abort.abort();
                }
                true
            }
            AgentMsg::UndoRun => {
                if self.streaming || !self.undo_available {
                    return false;
                }
                let mut fs = ctx.props().fs.borrow_mut();
                let journal = self.journal.borrow();
                match journal.revert(&mut fs) {
                    Ok(()) => {
                        drop(journal);
                        *self.journal.borrow_mut() = Journal::new();
                        self.undo_available = false;
                        self.status = Some("Undid agent run".into());
                        self.error = None;
                        ctx.props().on_vfs_mutated.emit(());
                    }
                    Err(e) => {
                        self.error = Some(format!("Undo failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::RevertPath(path) => {
                if self.streaming {
                    return false;
                }
                let mut fs = ctx.props().fs.borrow_mut();
                let mut journal = self.journal.borrow_mut();
                match journal.revert_path(&mut fs, &path) {
                    Ok(()) => {
                        self.undo_available = !journal.changed_paths().is_empty();
                        self.status = Some(format!("Reverted {path}"));
                        self.error = None;
                        drop(journal);
                        drop(fs);
                        ctx.props().on_vfs_mutated.emit(());
                    }
                    Err(e) => {
                        self.error = Some(format!("Revert failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::ToggleDiff { turn, path } => {
                let key = (turn, path);
                if let Some(i) = self.diff_open.iter().position(|k| k == &key) {
                    self.diff_open.remove(i);
                } else {
                    self.diff_open.push(key);
                }
                true
            }
            AgentMsg::Delta { content, reasoning } => {
                self.live_content = content;
                self.live_reasoning = reasoning;
                true
            }
            AgentMsg::Turn(turn) => {
                let mutated = turn.tools.iter().any(|t| !t.changed_paths.is_empty());
                self.live_content.clear();
                self.live_reasoning.clear();
                self.reasoning_open.push(false);
                self.transcript.push(turn);
                if mutated {
                    ctx.props().on_vfs_mutated.emit(());
                }
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
                self.undo_available = !self.journal.borrow().changed_paths().is_empty();
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
        let on_undo = ctx.link().callback(|_| AgentMsg::UndoRun);

        let usage = &self.usage;
        let hit = usage.prompt_cache_hit_tokens;
        let miss = usage.prompt_cache_miss_tokens;
        let cache_total = hit + miss;
        let hit_pct = if cache_total == 0 {
            0
        } else {
            (hit * 100) / cache_total
        };

        let journal = self.journal.borrow();

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
                        if self.undo_available {
                            <button
                                class="agent-btn agent-btn-undo"
                                onclick={on_undo}
                                disabled={self.streaming}
                                title="Revert every file this run touched"
                            >
                                { "Undo agent run" }
                            </button>
                        }
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
                            let link = ctx.link().clone();
                            let diff_open = self.diff_open.clone();
                            render_turn(i, turn, open, toggle, &journal, &diff_open, link)
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
    journal: &Journal,
    diff_open: &[(usize, String)],
    link: yew::html::Scope<Agent>,
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
                    let diffs = t
                        .changed_paths
                        .iter()
                        .filter_map(|p| journal.get(p).cloned())
                        .filter(|e| e.prior != e.after)
                        .collect::<Vec<_>>();
                    html! {
                        <div class="agent-turn-tool">
                            <div class="agent-turn-label">
                                { format!("{}({})", t.name, t.arguments) }
                            </div>
                            <pre class="agent-turn-body agent-tool-result">{ &t.result }</pre>
                            {
                                diffs.into_iter().map(|delta| {
                                    let path = delta.path.clone();
                                    let open = diff_open.iter().any(|(ti, p)| *ti == index && p == &path);
                                    let path_toggle = path.clone();
                                    let path_revert = path.clone();
                                    let toggle_diff = link.callback(move |_| AgentMsg::ToggleDiff {
                                        turn: index,
                                        path: path_toggle.clone(),
                                    });
                                    let revert = link.callback(move |_| {
                                        AgentMsg::RevertPath(path_revert.clone())
                                    });
                                    render_file_diff(&delta, open, toggle_diff, revert)
                                }).collect::<Html>()
                            }
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

fn render_file_diff(
    delta: &FileDelta,
    open: bool,
    toggle: Callback<MouseEvent>,
    revert: Callback<MouseEvent>,
) -> Html {
    let summary = format!("{}  (diff)", delta.path);
    html! {
        <div class="agent-file-diff">
            <div class="agent-file-diff-bar">
                <button type="button" class="agent-turn-label agent-turn-toggle" onclick={toggle}>
                    { if open { format!("▾ {summary}") } else { format!("▸ {summary}") } }
                </button>
                <button
                    type="button"
                    class="agent-btn agent-btn-revert"
                    onclick={revert}
                    title={format!("Revert {}", delta.path)}
                >
                    { "Revert" }
                </button>
            </div>
            if open {
                <pre class="agent-turn-body agent-diff-body">{ format_diff(delta) }</pre>
            }
        </div>
    }
}

fn format_diff(delta: &FileDelta) -> String {
    let prior = path_state_lines(&delta.prior);
    let after = path_state_lines(&delta.after);
    let mut out = String::new();
    out.push_str(&format!("--- {}\n", delta.path));
    out.push_str(&format!("+++ {}\n", delta.path));
    for line in &prior {
        out.push_str(&format!("- {line}\n"));
    }
    for line in &after {
        out.push_str(&format!("+ {line}\n"));
    }
    out
}

fn path_state_lines(state: &PathState) -> Vec<String> {
    state.display_body().lines().map(|l| l.to_string()).collect()
}
