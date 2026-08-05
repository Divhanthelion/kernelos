use std::cell::RefCell;
use std::rc::Rc;

use yew::prelude::*;
use web_sys::{HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};

use crate::agent::{
    diff_against_trunk, load_api_key, promote_all, promote_path, prompt_branch_name,
    prompt_restore_name, run_agent_loop, save_api_key, stream_completion, tool_definitions,
    Branch, BranchDiff, ChatRequest, FileDelta, Journal, LoopConfig, LoopEvent, LoopStopReason,
    PathState, RestorePointStore, SseEvent, StreamError, TranscriptTurn, TurnAccumulator,
    UsageAccum, WorkspaceId,
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
    restore_points: RestorePointStore,
    /// Session-scoped RAM forks. Never persisted.
    branches: Vec<Branch>,
    active: WorkspaceId,
    /// Diff panel open for branch↔trunk paths.
    branch_diff_open: Vec<String>,
}

pub enum AgentMsg {
    SetPrompt(String),
    SetApiKey(String),
    Submit,
    Stop,
    UndoRun,
    SaveRestorePoint,
    RestorePoint(String),
    DeleteRestorePoint(String),
    ForkBranch,
    SwitchWorkspace(WorkspaceId),
    DiscardBranch(String),
    PromoteAll,
    PromotePath(String),
    ToggleBranchDiff(String),
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

impl Agent {
    fn active_fs(&self, trunk: &Rc<RefCell<FileSystem>>) -> Rc<RefCell<FileSystem>> {
        match &self.active {
            WorkspaceId::Trunk => Rc::clone(trunk),
            WorkspaceId::Branch(id) => self
                .branches
                .iter()
                .find(|b| b.id == *id)
                .map(|b| Rc::clone(&b.fs))
                .unwrap_or_else(|| Rc::clone(trunk)),
        }
    }
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
            restore_points: RestorePointStore::load(),
            branches: Vec::new(),
            active: WorkspaceId::Trunk,
            branch_diff_open: Vec::new(),
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
                let fs = self.active_fs(&ctx.props().fs);
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
                let fs_rc = self.active_fs(&ctx.props().fs);
                let mut fs = fs_rc.borrow_mut();
                let journal = self.journal.borrow();
                match journal.revert(&mut fs) {
                    Ok(()) => {
                        drop(journal);
                        *self.journal.borrow_mut() = Journal::new();
                        self.undo_available = false;
                        self.status = Some("Undid agent run".into());
                        self.error = None;
                        drop(fs);
                        if matches!(self.active, WorkspaceId::Trunk) {
                            ctx.props().on_vfs_mutated.emit(());
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Undo failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::SaveRestorePoint => {
                if self.streaming {
                    return false;
                }
                let default = chrono_ish_default_name();
                let Some(name) = prompt_restore_name(&default) else {
                    return false;
                };
                // Restore points always snapshot trunk, not the active branch.
                let fs = ctx.props().fs.borrow();
                match self.restore_points.save_point(&fs, &name) {
                    Ok(point) => {
                        self.status = Some(format!("Saved restore point “{}”", point.name));
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(format!("Save restore point failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::RestorePoint(id) => {
                if self.streaming {
                    return false;
                }
                let mut fs = ctx.props().fs.borrow_mut();
                match self.restore_points.restore(&mut fs, &id) {
                    Ok(()) => {
                        *self.journal.borrow_mut() = Journal::new();
                        self.undo_available = false;
                        self.active = WorkspaceId::Trunk;
                        let label = self
                            .restore_points
                            .get(&id)
                            .map(|p| p.name.clone())
                            .unwrap_or(id);
                        self.status = Some(format!("Restored “{label}” on trunk"));
                        self.error = None;
                        drop(fs);
                        ctx.props().on_vfs_mutated.emit(());
                    }
                    Err(e) => {
                        self.error = Some(format!("Restore failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::DeleteRestorePoint(id) => {
                if self.streaming {
                    return false;
                }
                match self.restore_points.delete(&id) {
                    Ok(true) => {
                        self.status = Some("Deleted restore point".into());
                        self.error = None;
                    }
                    Ok(false) => {
                        self.error = Some("Restore point not found".into());
                    }
                    Err(e) => {
                        self.error = Some(format!("Delete restore point failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::ForkBranch => {
                if self.streaming {
                    return false;
                }
                let Some(name) = prompt_branch_name("experiment") else {
                    return false;
                };
                let trunk = ctx.props().fs.borrow();
                match Branch::from_trunk(&trunk, &name) {
                    Ok(branch) => {
                        let id = branch.id.clone();
                        let label = branch.name.clone();
                        self.branches.push(branch);
                        self.active = WorkspaceId::Branch(id);
                        *self.journal.borrow_mut() = Journal::new();
                        self.undo_available = false;
                        self.branch_diff_open.clear();
                        self.status = Some(format!("Forked branch “{label}” (RAM only)"));
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(format!("Fork failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::SwitchWorkspace(id) => {
                if self.streaming {
                    return false;
                }
                if let WorkspaceId::Branch(ref bid) = id {
                    if !self.branches.iter().any(|b| b.id == *bid) {
                        self.error = Some("Branch not found".into());
                        return true;
                    }
                }
                self.active = id;
                *self.journal.borrow_mut() = Journal::new();
                self.undo_available = false;
                self.branch_diff_open.clear();
                self.status = Some(match &self.active {
                    WorkspaceId::Trunk => "Switched to trunk".into(),
                    WorkspaceId::Branch(id) => {
                        let name = self
                            .branches
                            .iter()
                            .find(|b| b.id == *id)
                            .map(|b| b.name.clone())
                            .unwrap_or_else(|| id.clone());
                        format!("Switched to branch “{name}”")
                    }
                });
                true
            }
            AgentMsg::DiscardBranch(id) => {
                if self.streaming {
                    return false;
                }
                self.branches.retain(|b| b.id != id);
                if matches!(&self.active, WorkspaceId::Branch(active) if active == &id) {
                    self.active = WorkspaceId::Trunk;
                    *self.journal.borrow_mut() = Journal::new();
                    self.undo_available = false;
                }
                self.branch_diff_open.clear();
                self.status = Some("Discarded branch".into());
                true
            }
            AgentMsg::PromoteAll => {
                if self.streaming {
                    return false;
                }
                let WorkspaceId::Branch(id) = &self.active else {
                    self.error = Some("Switch to a branch to promote".into());
                    return true;
                };
                let Some(branch) = self.branches.iter().find(|b| b.id == *id) else {
                    self.error = Some("Branch not found".into());
                    return true;
                };
                let branch_fs = Rc::clone(&branch.fs);
                let mut trunk = ctx.props().fs.borrow_mut();
                match promote_all(&mut trunk, &branch_fs.borrow()) {
                    Ok(n) => {
                        self.status = Some(format!("Promoted {n} path(s) to trunk"));
                        self.error = None;
                        drop(trunk);
                        ctx.props().on_vfs_mutated.emit(());
                    }
                    Err(e) => {
                        self.error = Some(format!("Promote failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::PromotePath(path) => {
                if self.streaming {
                    return false;
                }
                let WorkspaceId::Branch(id) = &self.active else {
                    self.error = Some("Switch to a branch to cherry-pick".into());
                    return true;
                };
                let Some(branch) = self.branches.iter().find(|b| b.id == *id) else {
                    self.error = Some("Branch not found".into());
                    return true;
                };
                let branch_fs = Rc::clone(&branch.fs);
                let mut trunk = ctx.props().fs.borrow_mut();
                match promote_path(&mut trunk, &branch_fs.borrow(), &path) {
                    Ok(()) => {
                        self.status = Some(format!("Cherry-picked {path} → trunk"));
                        self.error = None;
                        drop(trunk);
                        ctx.props().on_vfs_mutated.emit(());
                    }
                    Err(e) => {
                        self.error = Some(format!("Cherry-pick failed: {e}"));
                    }
                }
                true
            }
            AgentMsg::ToggleBranchDiff(path) => {
                if let Some(i) = self.branch_diff_open.iter().position(|p| p == &path) {
                    self.branch_diff_open.remove(i);
                } else {
                    self.branch_diff_open.push(path);
                }
                true
            }
            AgentMsg::RevertPath(path) => {
                if self.streaming {
                    return false;
                }
                let fs_rc = self.active_fs(&ctx.props().fs);
                let mut fs = fs_rc.borrow_mut();
                let mut journal = self.journal.borrow_mut();
                match journal.revert_path(&mut fs, &path) {
                    Ok(()) => {
                        self.undo_available = !journal.changed_paths().is_empty();
                        self.status = Some(format!("Reverted {path}"));
                        self.error = None;
                        drop(journal);
                        drop(fs);
                        if matches!(self.active, WorkspaceId::Trunk) {
                            ctx.props().on_vfs_mutated.emit(());
                        }
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
                if mutated && matches!(self.active, WorkspaceId::Trunk) {
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
        let on_prompt = ctx.link().callback(|e: InputEvent| {            let input: HtmlTextAreaElement = e.target_unchecked_into();
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
        let on_save_point = ctx.link().callback(|_| AgentMsg::SaveRestorePoint);
        let on_fork = ctx.link().callback(|_| AgentMsg::ForkBranch);
        let on_promote_all = ctx.link().callback(|_| AgentMsg::PromoteAll);
        let on_trunk = ctx.link().callback(|_| AgentMsg::SwitchWorkspace(WorkspaceId::Trunk));

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
        let restore_points: Vec<_> = self.restore_points.points().to_vec();
        let on_branch = matches!(self.active, WorkspaceId::Branch(_));
        let branch_diffs: Vec<BranchDiff> = if let WorkspaceId::Branch(ref id) = self.active {
            self.branches
                .iter()
                .find(|b| b.id == *id)
                .map(|b| {
                    diff_against_trunk(&b.fs.borrow(), &ctx.props().fs.borrow())
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let active_label = match &self.active {
            WorkspaceId::Trunk => "trunk".to_string(),
            WorkspaceId::Branch(id) => self
                .branches
                .iter()
                .find(|b| b.id == *id)
                .map(|b| format!("branch: {}", b.name))
                .unwrap_or_else(|| "branch:?".into()),
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
                    <div class="agent-workspace-bar">
                        <span class="agent-workspace-label">{ format!("Workspace: {active_label}") }</span>
                        <button
                            class={classes!("agent-btn", "agent-btn-ws", (!on_branch).then_some("agent-btn-ws-active"))}
                            onclick={on_trunk}
                            disabled={self.streaming}
                        >
                            { "Trunk" }
                        </button>
                        {
                            self.branches.iter().map(|b| {
                                let id = b.id.clone();
                                let active = matches!(&self.active, WorkspaceId::Branch(a) if a == &id);
                                let switch = ctx.link().callback({
                                    let id = id.clone();
                                    move |_| AgentMsg::SwitchWorkspace(WorkspaceId::Branch(id.clone()))
                                });
                                let discard = ctx.link().callback({
                                    let id = id.clone();
                                    move |_| AgentMsg::DiscardBranch(id.clone())
                                });
                                html! {
                                    <span class="agent-branch-chip" key={b.id.clone()}>
                                        <button
                                            class={classes!("agent-btn", "agent-btn-ws", active.then_some("agent-btn-ws-active"))}
                                            onclick={switch}
                                            disabled={self.streaming}
                                        >
                                            { b.name.clone() }
                                        </button>
                                        <button
                                            class="agent-btn agent-btn-delete-point"
                                            onclick={discard}
                                            disabled={self.streaming}
                                            title="Discard this RAM branch"
                                        >
                                            { "×" }
                                        </button>
                                    </span>
                                }
                            }).collect::<Html>()
                        }
                        <button
                            class="agent-btn agent-btn-fork"
                            onclick={on_fork}
                            disabled={self.streaming}
                            title="Fork trunk into a RAM-only branch"
                        >
                            { "Fork" }
                        </button>
                    </div>
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
                        <button
                            class="agent-btn agent-btn-save-point"
                            onclick={on_save_point}
                            disabled={self.streaming}
                            title="Snapshot trunk under a name"
                        >
                            { "Save restore point" }
                        </button>
                        if on_branch {
                            <button
                                class="agent-btn agent-btn-promote"
                                onclick={on_promote_all}
                                disabled={self.streaming || branch_diffs.is_empty()}
                                title="Copy all branch changes onto trunk"
                            >
                                { "Promote all → trunk" }
                            </button>
                        }
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

                if on_branch && !branch_diffs.is_empty() {
                    <div class="agent-branch-diff">
                        <div class="agent-restore-points-label">{ "Diff vs trunk" }</div>
                        {
                            branch_diffs.into_iter().map(|d| {
                                let path = d.path.clone();
                                let open = self.branch_diff_open.iter().any(|p| p == &path);
                                let toggle = ctx.link().callback({
                                    let path = path.clone();
                                    move |_| AgentMsg::ToggleBranchDiff(path.clone())
                                });
                                let promote = ctx.link().callback({
                                    let path = path.clone();
                                    move |_| AgentMsg::PromotePath(path.clone())
                                });
                                html! {
                                    <div class="agent-file-diff" key={d.path.clone()}>
                                        <div class="agent-file-diff-bar">
                                            <button type="button" class="agent-turn-label agent-turn-toggle" onclick={toggle}>
                                                { if open { format!("▾ {}", d.path) } else { format!("▸ {}", d.path) } }
                                            </button>
                                            <button
                                                type="button"
                                                class="agent-btn agent-btn-restore"
                                                onclick={promote}
                                                disabled={self.streaming}
                                                title="Cherry-pick this path onto trunk"
                                            >
                                                { "Cherry-pick" }
                                            </button>
                                        </div>
                                        if open {
                                            <pre class="agent-turn-body agent-diff-body">{
                                                format_branch_diff(&d)
                                            }</pre>
                                        }
                                    </div>
                                }
                            }).collect::<Html>()
                        }
                    </div>
                }

                if !restore_points.is_empty() {
                    <div class="agent-restore-points">
                        <div class="agent-restore-points-label">{ "Restore points" }</div>
                        {
                            restore_points.into_iter().rev().map(|point| {
                                let id_restore = point.id.clone();
                                let id_delete = point.id.clone();
                                let on_restore = ctx.link().callback(move |_| {
                                    AgentMsg::RestorePoint(id_restore.clone())
                                });
                                let on_delete = ctx.link().callback(move |_| {
                                    AgentMsg::DeleteRestorePoint(id_delete.clone())
                                });
                                html! {
                                    <div class="agent-restore-point" key={point.id.clone()}>
                                        <span class="agent-restore-point-name">{ point.name.clone() }</span>
                                        <button
                                            class="agent-btn agent-btn-restore"
                                            onclick={on_restore}
                                            disabled={self.streaming}
                                            title="Replace trunk with this snapshot"
                                        >
                                            { "Restore" }
                                        </button>
                                        <button
                                            class="agent-btn agent-btn-delete-point"
                                            onclick={on_delete}
                                            disabled={self.streaming}
                                            title="Delete this restore point"
                                        >
                                            { "Delete" }
                                        </button>
                                    </div>
                                }
                            }).collect::<Html>()
                        }
                    </div>
                }

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

fn format_branch_diff(d: &BranchDiff) -> String {
    let prior = path_state_lines(&d.trunk);
    let after = path_state_lines(&d.branch);
    let mut out = String::new();
    out.push_str(&format!("--- trunk:{}\n", d.path));
    out.push_str(&format!("+++ branch:{}\n", d.path));
    for line in &prior {
        out.push_str(&format!("- {line}\n"));
    }
    for line in &after {
        out.push_str(&format!("+ {line}\n"));
    }
    out
}

fn chrono_ish_default_name() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new_0();
        format!(
            "point {}-{:02}-{:02} {:02}:{:02}",
            d.get_full_year(),
            d.get_month() + 1,
            d.get_date(),
            d.get_hours(),
            d.get_minutes()
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "restore point".into()
    }
}
