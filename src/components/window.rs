use yew::prelude::*;
use web_sys::{MouseEvent, PointerEvent};
use std::rc::Rc;
use std::cell::RefCell;

use crate::apps::{self, AppContext};
use crate::filesystem::FileSystem;
use crate::plugin::{self, PluginHandle};
use crate::plugin::abi::Event;
use crate::plugin::render::{render_ops, RenderContext};

#[derive(Debug, Clone)]
pub struct WindowState {
    pub id: String,
    pub title: String,
    /// Which registry app this window hosts.
    pub app_id: String,
    /// Launch argument for that app — a file path, a URL, or nothing.
    pub arg: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub is_minimized: bool,
    pub is_maximized: bool,
    pub is_focused: bool,
    /// Stacking order, assigned by the Desktop. Higher is nearer the viewer.
    pub z_index: u32,
    // Store pre-maximize dimensions
    pub restore_rect: Option<(i32, i32, i32, i32)>,
    /// Per-window plugin instance. `Some` exactly when `app_id` names a plugin.
    pub plugin_handle: Option<Rc<RefCell<PluginHandle>>>,
}

impl PartialEq for WindowState {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.title == other.title
            && self.app_id == other.app_id
            && self.arg == other.arg
            && self.x == other.x
            && self.y == other.y
            && self.width == other.width
            && self.height == other.height
            && self.min_width == other.min_width
            && self.min_height == other.min_height
            && self.is_minimized == other.is_minimized
            && self.is_maximized == other.is_maximized
            && self.is_focused == other.is_focused
            && self.z_index == other.z_index
            && self.restore_rect == other.restore_rect
            && match (&self.plugin_handle, &other.plugin_handle) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl WindowState {
    /// Build a window for a registered app. Geometry comes from the registry so
    /// there is no second place to keep in sync.
    pub fn new(id: String, app_id: &str, title: String, arg: Option<String>) -> Self {
        let (width, height, min_width, min_height) = apps::geometry_for(app_id)
            .unwrap_or((650, 450, 300, 200));

        Self {
            id,
            title,
            app_id: app_id.to_string(),
            arg,
            x: 100,
            y: 50,
            width,
            height,
            min_width,
            min_height,
            is_minimized: false,
            is_maximized: false,
            is_focused: true,
            z_index: 0,
            restore_rect: None,
            plugin_handle: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum DragMode {
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
    pub on_theme_change: Callback<String>,
    pub on_wallpaper_change: Callback<String>,
    pub on_accent_change: Callback<String>,
    pub on_geometry_changed: Callback<()>,
    pub vfs_epoch: u64,
    pub on_vfs_mutated: Callback<()>,
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
    /// (x, y, pointer_id) — the pointer is captured so the drag survives the
    /// cursor outrunning the render loop and leaving the element.
    StartDrag(DragMode, i32, i32, i32),
    Drag(i32, i32),
    StopDrag(i32),
    Close,
    Minimize,
    Maximize,
    Focus,
    /// A widget inside a plugin UI fired — deliver it to the guest.
    PluginEvent(Event),
    /// The guest crashed; re-instantiate it from the registry.
    ReloadPlugin,
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
            WindowMsg::StartDrag(mode, x, y, pointer_id) => {
                // Any grab also raises the window.
                ctx.props().on_focus.emit(ctx.props().window.borrow().id.clone());

                let window = ctx.props().window.borrow();
                if window.is_maximized {
                    return false;
                }

                self.drag_mode = mode;
                self.drag_start_x = x;
                self.drag_start_y = y;
                self.window_start_x = window.x;
                self.window_start_y = window.y;
                self.window_start_width = window.width;
                self.window_start_height = window.height;
                drop(window);

                self.set_pointer_capture(pointer_id);
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
            WindowMsg::StopDrag(pointer_id) => {
                if self.drag_mode == DragMode::None {
                    return false;
                }
                self.drag_mode = DragMode::None;
                self.release_pointer_capture(pointer_id);
                // Persist once per gesture rather than on every pointer move.
                ctx.props().on_geometry_changed.emit(());
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
            WindowMsg::PluginEvent(event) => {
                let handle = ctx.props().window.borrow().plugin_handle.clone();
                if let Some(handle) = handle {
                    if let Err(e) = handle.borrow_mut().send(&event) {
                        log::warn!("plugin event dispatch failed: {e}");
                    }
                }
                // Re-render either way: the guest may have mutated state (or
                // crashed, which switches the view to the crash screen).
                true
            }
            WindowMsg::ReloadPlugin => {
                let app_id = ctx.props().window.borrow().app_id.clone();
                match plugin::instantiate(&app_id, &ctx.props().fs, self.plugin_notify(ctx)) {
                    Ok(mut handle) => {
                        let _ = handle.send(&Event::Init);
                        let handle = Rc::new(RefCell::new(handle));
                        if let Some(mut window) = ctx.props().window.try_borrow_mut().ok() {
                            window.plugin_handle = Some(handle);
                        }
                    }
                    Err(e) => {
                        log::error!("plugin reload failed: {e}");
                        if let Some(handle) = ctx.props().window.borrow().plugin_handle.clone() {
                            handle.borrow_mut().set_crash(e);
                        }
                    }
                }
                true
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

        // Only geometry stays inline — it is per-window and dynamic. Everything
        // cosmetic lives in styles.css so themes can reach it.
        let window_style = format!(
            "left: {}px; top: {}px; width: {}px; height: {}px; z-index: {};",
            x, y, width, height, window.z_index
        );

        // One grab handler for the titlebar and all three resize handles.
        let grab = |mode: DragMode| {
            ctx.link().callback(move |e: PointerEvent| {
                e.prevent_default();
                e.stop_propagation();
                WindowMsg::StartDrag(mode, e.client_x(), e.client_y(), e.pointer_id())
            })
        };

        let on_titlebar_pointerdown = grab(DragMode::Move);
        let on_titlebar_dblclick = ctx.link().callback(|_| WindowMsg::Maximize);

        let on_pointermove = ctx.link().callback(|e: PointerEvent| {
            WindowMsg::Drag(e.client_x(), e.client_y())
        });

        let on_pointerup = ctx.link().callback(|e: PointerEvent| WindowMsg::StopDrag(e.pointer_id()));
        let on_pointercancel = ctx.link().callback(|e: PointerEvent| WindowMsg::StopDrag(e.pointer_id()));

        // Raise on any press inside the window, and keep the press from reaching
        // the desktop, whose click handler unfocuses everything.
        let on_focus = ctx.link().callback(|e: PointerEvent| {
            e.stop_propagation();
            WindowMsg::Focus
        });
        let swallow_click = Callback::from(|e: MouseEvent| e.stop_propagation());
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
        let on_controls_pointerdown = ctx.link().callback(|e: PointerEvent| {
            // The controls live inside the draggable titlebar. Stop the press
            // here so the titlebar does not capture the pointer and swallow the
            // button's subsequent click.
            e.stop_propagation();
            WindowMsg::Focus
        });

        let on_resize_right = grab(DragMode::ResizeRight);
        let on_resize_bottom = grab(DragMode::ResizeBottom);
        let on_resize_corner = grab(DragMode::ResizeCorner);

        let title = window.title.clone();
        let is_maximized = window.is_maximized;
        let window_is_minimized = window.is_minimized;
        drop(window);

        html! {
            <div 
                class={classes!(
                    "window",
                    ctx.props().window.borrow().is_focused.then_some("focused"),
                    window_is_minimized.then_some("minimized"),
                    is_maximized.then_some("maximized"),
                )}
                style={window_style}
                onpointerdown={on_focus}
                onclick={swallow_click}
                onpointermove={on_pointermove}
                onpointerup={on_pointerup}
                onpointercancel={on_pointercancel}
                ref={self.node_ref.clone()}
            >
                <div
                    class="window-titlebar"
                    onpointerdown={on_titlebar_pointerdown}
                    ondblclick={on_titlebar_dblclick}
                >
                    <span class="window-title">{ &title }</span>
                    <div class="window-controls" onpointerdown={on_controls_pointerdown}>
                        <button
                            class="window-control minimize"
                            onclick={on_minimize}
                            title="Minimize"
                            aria-label="Minimize window"
                        />
                        <button
                            class="window-control maximize"
                            onclick={on_maximize}
                            title={if is_maximized { "Restore" } else { "Maximize" }}
                            aria-label={if is_maximized { "Restore window" } else { "Maximize window" }}
                        />
                        <button
                            class="window-control close"
                            onclick={on_close}
                            title="Close"
                            aria-label="Close window"
                        />
                    </div>
                </div>
                <div class="window-content">
                    { self.render_content(ctx) }
                </div>
                
                // Resize handles (only if not maximized)
                {
                    if !is_maximized {
                        html! {
                            <>
                                <div class="resize-handle right" onpointerdown={on_resize_right} />
                                <div class="resize-handle bottom" onpointerdown={on_resize_bottom} />
                                <div class="resize-handle corner" onpointerdown={on_resize_corner} />
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
    /// Route all further events for this pointer to the window root, so a drag
    /// keeps tracking even when the cursor moves off the element or off-screen.
    fn set_pointer_capture(&self, pointer_id: i32) {
        if let Some(element) = self.node_ref.cast::<web_sys::Element>() {
            let _ = element.set_pointer_capture(pointer_id);
        }
    }

    fn release_pointer_capture(&self, pointer_id: i32) {
        if let Some(element) = self.node_ref.cast::<web_sys::Element>() {
            let _ = element.release_pointer_capture(pointer_id);
        }
    }

    fn render_content(&self, ctx: &Context<Self>) -> Html {
        let window = ctx.props().window.borrow();

        // Plugin windows carry their own instance — render its current frame.
        if let Some(handle) = window.plugin_handle.clone() {
            return self.render_plugin(ctx, handle);
        }

        let Some(app) = apps::find(&window.app_id) else {
            return html! {
                <div class="window-missing-app">
                    { format!("Unknown application '{}'", window.app_id) }
                </div>
            };
        };

        let context = AppContext {
            fs: Rc::clone(&ctx.props().fs),
            arg: window.arg.clone(),
            on_open_file: ctx.props().on_open_file.clone(),
            on_notification: ctx.props().on_notification.clone(),
            on_theme_change: ctx.props().on_theme_change.clone(),
            on_wallpaper_change: ctx.props().on_wallpaper_change.clone(),
            on_accent_change: ctx.props().on_accent_change.clone(),
            vfs_epoch: ctx.props().vfs_epoch,
            on_vfs_mutated: ctx.props().on_vfs_mutated.clone(),
        };

        (app.render)(&context)
    }

    fn render_plugin(&self, ctx: &Context<Self>, handle: Rc<RefCell<PluginHandle>>) -> Html {
        let handle = handle.borrow();

        if let Some(message) = handle.crash_message() {
            let on_reload = ctx.link().callback(|_| WindowMsg::ReloadPlugin);
            let message = message.to_string();
            return html! {
                <div class="plugin-crash">
                    <div class="plugin-crash-icon">{ "💥" }</div>
                    <div class="plugin-crash-title">{ "This app crashed" }</div>
                    <div class="plugin-crash-message">{ message }</div>
                    <button class="plugin-crash-reload" onclick={on_reload}>
                        { "Reload" }
                    </button>
                </div>
            };
        }

        let on_event = ctx.link().callback(WindowMsg::PluginEvent);
        let render_ctx = RenderContext { on_event };
        render_ops(handle.ops(), &render_ctx)
    }

    /// Map Desktop's `(title, body, kind)` notification callback down to the
    /// `(title, body)` pair the plugin host import expects.
    fn plugin_notify(&self, ctx: &Context<Self>) -> Callback<(String, String)> {
        let cb = ctx.props().on_notification.clone();
        Callback::from(move |(title, body): (String, String)| {
            cb.emit((title, body, "info".to_string()));
        })
    }
}
