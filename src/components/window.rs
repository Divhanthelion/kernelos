use yew::prelude::*;
use web_sys::MouseEvent;
use std::rc::Rc;
use std::cell::RefCell;
use gloo_timers::callback::Timeout;

use crate::filesystem::FileSystem;
use crate::components::terminal::Terminal;
use crate::components::file_explorer::FileExplorer;
use crate::components::text_editor::TextEditor;
use crate::components::clock::Clock;
use crate::components::calculator::Calculator;
use crate::components::settings::Settings;
use crate::components::paint::Paint;
use crate::components::minesweeper::Minesweeper;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    pub id: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub is_focused: bool,
    pub content_type: WindowContentType,
    // Store pre-maximize dimensions
    pub restore_rect: Option<(i32, i32, i32, i32)>,
}

impl WindowState {
    pub fn new(id: String, title: String, content_type: WindowContentType) -> Self {
        let (width, height, min_width, min_height) = match &content_type {
            WindowContentType::Calculator => (320, 480, 280, 400),
            WindowContentType::Clock => (300, 200, 200, 150),
            WindowContentType::Settings { .. } => (700, 500, 500, 400),
            WindowContentType::Paint => (800, 600, 400, 300),
            WindowContentType::Minesweeper => (400, 500, 300, 400),
            _ => (650, 450, 300, 200),
        };

        Self {
            id,
            title,
            x: 100,
            y: 50,
            width,
            height,
            min_width,
            min_height,
            is_minimized: false,
            is_maximized: false,
            is_focused: true,
            content_type,
            restore_rect: None,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum WindowContentType {
    Empty,
    Terminal,
    FileExplorer,
    TextEditor { file_path: Option<String> },
    Clock,
    Calculator,
    Settings { on_theme_change: Callback<String>, on_wallpaper_change: Callback<String> },
    Paint,
    Minesweeper,
    About,
}

#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    None,
    Move,
    ResizeRight,
    ResizeBottom,
    ResizeCorner,
}

#[derive(Properties, Clone, PartialEq)]
pub struct WindowProps {
    pub window: Rc<RefCell<WindowState>>,
    pub fs: Rc<RefCell<FileSystem>>,
    pub on_close: Callback<String>,
    pub on_focus: Callback<String>,
    pub on_minimize: Callback<String>,
    pub on_maximize: Callback<String>,
    pub on_open_file: Callback<(String, String)>,
    pub on_notification: Callback<(String, String, String)>,
}

pub struct Window {
    drag_mode: DragMode,
    drag_start_x: i32,
    drag_start_y: i32,
    window_start_x: i32,
    window_start_y: i32,
    window_start_width: i32,
    window_start_height: i32,
    node_ref: NodeRef,
}

pub enum WindowMsg {
    StartMove(i32, i32),
    StartResizeRight(i32, i32),
    StartResizeBottom(i32, i32),
    StartResizeCorner(i32, i32),
    Drag(i32, i32),
    StopDrag,
    Close,
    Minimize,
    Maximize,
    Focus,
}

impl Component for Window {
    type Message = WindowMsg;
    type Properties = WindowProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            drag_mode: DragMode::None,
            drag_start_x: 0,
            drag_start_y: 0,
            window_start_x: 0,
            window_start_y: 0,
            window_start_width: 0,
            window_start_height: 0,
            node_ref: NodeRef::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            WindowMsg::StartMove(x, y) => {
                let window = ctx.props().window.borrow();
                if window.is_maximized {
                    return false;
                }
                self.drag_mode = DragMode::Move;
                self.drag_start_x = x;
                self.drag_start_y = y;
                self.window_start_x = window.x;
                self.window_start_y = window.y;
                true
            }
            WindowMsg::StartResizeRight(x, y) => {
                let window = ctx.props().window.borrow();
                if window.is_maximized {
                    return false;
                }
                self.drag_mode = DragMode::ResizeRight;
                self.drag_start_x = x;
                self.window_start_width = window.width;
                true
            }
            WindowMsg::StartResizeBottom(x, y) => {
                let window = ctx.props().window.borrow();
                if window.is_maximized {
                    return false;
                }
                self.drag_mode = DragMode::ResizeBottom;
                self.drag_start_y = y;
                self.window_start_height = window.height;
                true
            }
            WindowMsg::StartResizeCorner(x, y) => {
                let window = ctx.props().window.borrow();
                if window.is_maximized {
                    return false;
                }
                self.drag_mode = DragMode::ResizeCorner;
                self.drag_start_x = x;
                self.drag_start_y = y;
                self.window_start_width = window.width;
                self.window_start_height = window.height;
                true
            }
            WindowMsg::Drag(x, y) => {
                match self.drag_mode {
                    DragMode::Move => {
                        let mut window = ctx.props().window.borrow_mut();
                        window.x = self.window_start_x + (x - self.drag_start_x);
                        window.y = (self.window_start_y + (y - self.drag_start_y)).max(0);
                        true
                    }
                    DragMode::ResizeRight => {
                        let mut window = ctx.props().window.borrow_mut();
                        let new_width = self.window_start_width + (x - self.drag_start_x);
                        window.width = new_width.max(window.min_width);
                        true
                    }
                    DragMode::ResizeBottom => {
                        let mut window = ctx.props().window.borrow_mut();
                        let new_height = self.window_start_height + (y - self.drag_start_y);
                        window.height = new_height.max(window.min_height);
                        true
                    }
                    DragMode::ResizeCorner => {
                        let mut window = ctx.props().window.borrow_mut();
                        let new_width = self.window_start_width + (x - self.drag_start_x);
                        let new_height = self.window_start_height + (y - self.drag_start_y);
                        window.width = new_width.max(window.min_width);
                        window.height = new_height.max(window.min_height);
                        true
                    }
                    DragMode::None => false,
                }
            }
            WindowMsg::StopDrag => {
                self.drag_mode = DragMode::None;
                true
            }
            WindowMsg::Close => {
                ctx.props().on_close.emit(ctx.props().window.borrow().id.clone());
                false
            }
            WindowMsg::Minimize => {
                ctx.props().on_minimize.emit(ctx.props().window.borrow().id.clone());
                false
            }
            WindowMsg::Maximize => {
                ctx.props().on_maximize.emit(ctx.props().window.borrow().id.clone());
                false
            }
            WindowMsg::Focus => {
                ctx.props().on_focus.emit(ctx.props().window.borrow().id.clone());
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let window = ctx.props().window.borrow();
        
        let (x, y, width, height) = if window.is_maximized {
            (0, 0, 
             web_sys::window().map(|w| w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(800.0) as i32).unwrap_or(800),
             web_sys::window().map(|w| w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(600.0) as i32 - 48).unwrap_or(552))
        } else {
            (window.x, window.y, window.width, window.height)
        };

        let window_style = format!(
            "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; \
             z-index: {}; display: {}; border-radius: {}; \
             overflow: hidden; box-shadow: {};",
            x, y, width, height,
            if window.is_focused { 100 } else { 50 },
            if window.is_minimized { "none" } else { "flex" },
            if window.is_maximized { "0" } else { "8px" },
            if window.is_focused { 
                "0 8px 32px rgba(0, 0, 0, 0.3)" 
            } else { 
                "0 4px 16px rgba(0, 0, 0, 0.2)" 
            }
        );

        let on_titlebar_mousedown = ctx.link().callback(|e: MouseEvent| {
            e.prevent_default();
            WindowMsg::StartMove(e.client_x(), e.client_y())
        });

        let on_titlebar_dblclick = ctx.link().callback(|_| WindowMsg::Maximize);

        let on_mousemove = ctx.link().callback(|e: MouseEvent| {
            WindowMsg::Drag(e.client_x(), e.client_y())
        });

        let on_mouseup = ctx.link().callback(|_| WindowMsg::StopDrag);
        let on_mouseleave = ctx.link().callback(|_| WindowMsg::StopDrag);
        let on_focus = ctx.link().callback(|_| WindowMsg::Focus);
        let on_close = ctx.link().callback(|e: MouseEvent| {
            e.stop_propagation();
            WindowMsg::Close
        });
        let on_minimize = ctx.link().callback(|e: MouseEvent| {
            e.stop_propagation();
            WindowMsg::Minimize
        });
        let on_maximize = ctx.link().callback(|e: MouseEvent| {
            e.stop_propagation();
            WindowMsg::Maximize
        });

        let on_resize_right = ctx.link().callback(|e: MouseEvent| {
            e.prevent_default();
            e.stop_propagation();
            WindowMsg::StartResizeRight(e.client_x(), e.client_y())
        });

        let on_resize_bottom = ctx.link().callback(|e: MouseEvent| {
            e.prevent_default();
            e.stop_propagation();
            WindowMsg::StartResizeBottom(e.client_x(), e.client_y())
        });

        let on_resize_corner = ctx.link().callback(|e: MouseEvent| {
            e.prevent_default();
            e.stop_propagation();
            WindowMsg::StartResizeCorner(e.client_x(), e.client_y())
        });

        let title = window.title.clone();
        let is_maximized = window.is_maximized;
        drop(window);

        html! {
            <div 
                class={classes!("window", if ctx.props().window.borrow().is_focused { Some("focused") } else { None })}
                style={window_style}
                onclick={on_focus}
                onmousemove={on_mousemove.clone()}
                onmouseup={on_mouseup.clone()}
                onmouseleave={on_mouseleave}
                ref={self.node_ref.clone()}
            >
                <div 
                    class="window-titlebar"
                    style="display: flex; align-items: center; justify-content: space-between; padding: 8px 12px; background-color: #3d3d3d; cursor: move; user-select: none;"
                    onmousedown={on_titlebar_mousedown}
                    ondblclick={on_titlebar_dblclick}
                >
                    <span class="window-title" style="font-size: 13px; font-weight: 500; color: white; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                        { &title }
                    </span>
                    <div class="window-controls" style="display: flex; gap: 8px;">
                        <button 
                            class="window-control minimize"
                            style="width: 12px; height: 12px; border-radius: 50%; border: none; cursor: pointer; background-color: #ffbd2e;"
                            onclick={on_minimize}
                            title="Minimize"
                        />
                        <button 
                            class="window-control maximize"
                            style="width: 12px; height: 12px; border-radius: 50%; border: none; cursor: pointer; background-color: #28ca41;"
                            onclick={on_maximize}
                            title={if is_maximized { "Restore" } else { "Maximize" }}
                        />
                        <button 
                            class="window-control close"
                            style="width: 12px; height: 12px; border-radius: 50%; border: none; cursor: pointer; background-color: #ff5f57;"
                            onclick={on_close}
                            title="Close"
                        />
                    </div>
                </div>
                <div class="window-content" style="flex: 1; overflow: auto; background-color: #2d2d2d;">
                    { self.render_content(ctx) }
                </div>
                
                // Resize handles (only if not maximized)
                {
                    if !is_maximized {
                        html! {
                            <>
                                <div 
                                    class="resize-handle right"
                                    style="position: absolute; right: 0; top: 0; width: 6px; height: 100%; cursor: ew-resize;"
                                    onmousedown={on_resize_right}
                                />
                                <div 
                                    class="resize-handle bottom"
                                    style="position: absolute; bottom: 0; left: 0; width: 100%; height: 6px; cursor: ns-resize;"
                                    onmousedown={on_resize_bottom}
                                />
                                <div 
                                    class="resize-handle corner"
                                    style="position: absolute; right: 0; bottom: 0; width: 16px; height: 16px; cursor: nwse-resize;"
                                    onmousedown={on_resize_corner}
                                />
                            </>
                        }
                    } else {
                        html! {}
                    }
                }
            </div>
        }
    }
}

impl Window {
    fn render_content(&self, ctx: &Context<Self>) -> Html {
        let window = ctx.props().window.borrow();
        let fs = Rc::clone(&ctx.props().fs);
        let on_open_file = ctx.props().on_open_file.clone();
        let on_notification = ctx.props().on_notification.clone();
        
        match &window.content_type {
            WindowContentType::Empty => html! {
                <div style="display: flex; align-items: center; justify-content: center; height: 100%; color: #888;">
                    { "Empty Window" }
                </div>
            },
            WindowContentType::Terminal => {
                html! { <Terminal fs={fs} on_notification={on_notification} /> }
            }
            WindowContentType::FileExplorer => {
                html! { <FileExplorer fs={fs} on_open_file={on_open_file} /> }
            }
            WindowContentType::TextEditor { file_path } => {
                html! { <TextEditor fs={fs} file_path={file_path.clone()} on_notification={on_notification} /> }
            }
            WindowContentType::Clock => {
                html! { <Clock /> }
            }
            WindowContentType::Calculator => {
                html! { <Calculator /> }
            }
            WindowContentType::Settings { on_theme_change, on_wallpaper_change } => {
                html! { <Settings on_theme_change={on_theme_change.clone()} on_wallpaper_change={on_wallpaper_change.clone()} /> }
            }
            WindowContentType::Paint => {
                html! { <Paint /> }
            }
            WindowContentType::Minesweeper => {
                html! { <Minesweeper /> }
            }
            WindowContentType::About => {
                html! {
                    <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%); color: white; text-align: center; padding: 24px;">
                        <div style="font-size: 48px; margin-bottom: 16px;">{ "🖥️" }</div>
                        <h1 style="margin: 0 0 8px 0; font-weight: 300;">{ "KernelOS" }</h1>
                        <p style="color: rgba(255,255,255,0.7); margin: 0 0 24px 0;">{ "Version 2.0" }</p>
                        <p style="color: rgba(255,255,255,0.5); font-size: 13px; max-width: 300px;">
                            { "A WebAssembly-based desktop environment built with Rust and Yew." }
                        </p>
                        <p style="color: rgba(255,255,255,0.4); font-size: 12px; margin-top: 24px;">
                            { "© 2025 KernelOS Project" }
                        </p>
                    </div>
                }
            }
        }
    }
}
