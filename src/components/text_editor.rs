use yew::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use web_sys::{HtmlTextAreaElement, KeyboardEvent};
use crate::filesystem::FileSystem;

pub struct TextEditor {
    fs: Rc<RefCell<FileSystem>>,
    content: String,
    file_path: Option<String>,
    is_modified: bool,
    line_count: usize,
    cursor_line: usize,
    cursor_col: usize,
    textarea_ref: NodeRef,
    word_wrap: bool,
    font_size: u32,
}

pub enum TextEditorMsg {
    ContentChanged(String),
    Save,
    SaveAs(String),
    KeyDown(KeyboardEvent),
    UpdateCursor,
    ToggleWordWrap,
    IncreaseFontSize,
    DecreaseFontSize,
}

#[derive(Properties, Clone, PartialEq)]
pub struct TextEditorProps {
    pub fs: Rc<RefCell<FileSystem>>,
    #[prop_or_default]
    pub file_path: Option<String>,
    #[prop_or_default]
    pub on_notification: Callback<(String, String, String)>,
}

impl Component for TextEditor {
    type Message = TextEditorMsg;
    type Properties = TextEditorProps;

    fn create(ctx: &Context<Self>) -> Self {
        let fs = Rc::clone(&ctx.props().fs);
        let file_path = ctx.props().file_path.clone();
        
        let content = if let Some(ref path) = file_path {
            fs.borrow().read_file(path).unwrap_or_default()
        } else {
            String::new()
        };
        
        let line_count = content.lines().count().max(1);

        Self {
            fs,
            content,
            file_path,
            is_modified: false,
            line_count,
            cursor_line: 1,
            cursor_col: 1,
            textarea_ref: NodeRef::default(),
            word_wrap: true,
            font_size: 14,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            TextEditorMsg::ContentChanged(content) => {
                self.content = content;
                self.is_modified = true;
                self.line_count = self.content.lines().count().max(1);
                ctx.link().send_message(TextEditorMsg::UpdateCursor);
                true
            }
            TextEditorMsg::Save => {
                if let Some(ref path) = self.file_path {
                    match self.fs.borrow_mut().write_file(path, &self.content) {
                        Ok(_) => {
                            self.is_modified = false;
                            ctx.props().on_notification.emit((
                                "File Saved".to_string(),
                                format!("Saved to {}", path),
                                "success".to_string()
                            ));
                        }
                        Err(e) => {
                            ctx.props().on_notification.emit((
                                "Save Failed".to_string(),
                                e,
                                "error".to_string()
                            ));
                        }
                    }
                } else {
                    // No file path - save as new file
                    let new_path = "/home/documents/untitled.txt".to_string();
                    ctx.link().send_message(TextEditorMsg::SaveAs(new_path));
                }
                true
            }
            TextEditorMsg::SaveAs(path) => {
                match self.fs.borrow_mut().write_file(&path, &self.content) {
                    Ok(_) => {
                        self.file_path = Some(path.clone());
                        self.is_modified = false;
                        ctx.props().on_notification.emit((
                            "File Saved".to_string(),
                            format!("Saved to {}", path),
                            "success".to_string()
                        ));
                    }
                    Err(e) => {
                        ctx.props().on_notification.emit((
                            "Save Failed".to_string(),
                            e,
                            "error".to_string()
                        ));
                    }
                }
                true
            }
            TextEditorMsg::KeyDown(event) => {
                if event.ctrl_key() || event.meta_key() {
                    match event.key().as_str() {
                        "s" => {
                            event.prevent_default();
                            ctx.link().send_message(TextEditorMsg::Save);
                        }
                        "+" | "=" => {
                            event.prevent_default();
                            ctx.link().send_message(TextEditorMsg::IncreaseFontSize);
                        }
                        "-" => {
                            event.prevent_default();
                            ctx.link().send_message(TextEditorMsg::DecreaseFontSize);
                        }
                        _ => {}
                    }
                }
                // Update cursor position after key press
                ctx.link().send_message(TextEditorMsg::UpdateCursor);
                false
            }
            TextEditorMsg::UpdateCursor => {
                if let Some(textarea) = self.textarea_ref.cast::<HtmlTextAreaElement>() {
                    let selection_start = textarea.selection_start().ok().flatten().unwrap_or(0) as usize;
                    let text_before_cursor = &self.content[..selection_start.min(self.content.len())];
                    self.cursor_line = text_before_cursor.lines().count().max(1);
                    self.cursor_col = text_before_cursor.lines().last().map(|l| l.len() + 1).unwrap_or(1);
                }
                true
            }
            TextEditorMsg::ToggleWordWrap => {
                self.word_wrap = !self.word_wrap;
                true
            }
            TextEditorMsg::IncreaseFontSize => {
                self.font_size = (self.font_size + 2).min(32);
                true
            }
            TextEditorMsg::DecreaseFontSize => {
                self.font_size = (self.font_size - 2).max(10);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let oninput = ctx.link().callback(|e: InputEvent| {
            let textarea: HtmlTextAreaElement = e.target_unchecked_into();
            TextEditorMsg::ContentChanged(textarea.value())
        });
        
        let onkeydown = ctx.link().callback(TextEditorMsg::KeyDown);
        let onclick = ctx.link().callback(|_| TextEditorMsg::UpdateCursor);

        let file_name = self.file_path
            .as_ref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or("Untitled");

        let title = if self.is_modified {
            format!("• {}", file_name)
        } else {
            file_name.to_string()
        };

        html! {
            <div class="text-editor">
                // Toolbar
                <div class="text-editor-toolbar">
                    <span class="text-editor-title">{ title }</span>
                    <div class="text-editor-spacer" />
                    <button 
                        class="btn btn-primary btn-small"
                        onclick={ctx.link().callback(|_| TextEditorMsg::Save)}
                        title="Save (Ctrl+S)"
                    >
                        { "💾 Save" }
                    </button>
                    <button 
                        class="btn btn-secondary btn-small"
                        onclick={ctx.link().callback(|_| TextEditorMsg::ToggleWordWrap)}
                        title="Toggle Word Wrap"
                    >
                        { if self.word_wrap { "↩️ Wrap" } else { "→ No Wrap" } }
                    </button>
                    <button 
                        class="btn btn-secondary btn-small"
                        onclick={ctx.link().callback(|_| TextEditorMsg::DecreaseFontSize)}
                        title="Decrease Font Size (Ctrl+-)"
                    >
                        { "A-" }
                    </button>
                    <span class="text-editor-fontsize">
                        { format!("{}px", self.font_size) }
                    </span>
                    <button 
                        class="btn btn-secondary btn-small"
                        onclick={ctx.link().callback(|_| TextEditorMsg::IncreaseFontSize)}
                        title="Increase Font Size (Ctrl++)"
                    >
                        { "A+" }
                    </button>
                </div>
                
                // Editor area with line numbers
                <div class="text-editor-body">
                    // Line numbers
                    <div class="text-editor-gutter">
                        <div class="text-editor-linenumbers" style={format!("font-size: {}px;", self.font_size)}>
                            {
                                (1..=self.line_count).map(|n| {
                                    let is_current = n == self.cursor_line;
                                    html! {
                                        <div class={classes!("text-editor-linenumber", is_current.then_some("current"))}>
                                            { n }
                                        </div>
                                    }
                                }).collect::<Html>()
                            }
                        </div>
                    </div>
                    
                    // Text area
                    <textarea
                        ref={self.textarea_ref.clone()}
                        class={classes!("text-editor-textarea", (!self.word_wrap).then_some("nowrap"))}
                        style={format!("font-size: {}px;", self.font_size)}
                        value={self.content.clone()}
                        {oninput}
                        {onkeydown}
                        {onclick}
                        spellcheck="false"
                    />
                </div>
                
                // Status bar
                <div class="text-editor-status">
                    <div class="text-editor-status-group">
                        <span>{ format!("Ln {}, Col {}", self.cursor_line, self.cursor_col) }</span>
                        <span>{ format!("{} lines", self.line_count) }</span>
                        <span>{ format!("{} chars", self.content.len()) }</span>
                    </div>
                    <div class="text-editor-status-group">
                        <span>{ if self.word_wrap { "Word Wrap: On" } else { "Word Wrap: Off" } }</span>
                        <span>{ "UTF-8" }</span>
                    </div>
                </div>
            </div>
        }
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            if let Some(textarea) = self.textarea_ref.cast::<HtmlTextAreaElement>() {
                let _ = textarea.focus();
            }
        }
    }
}
