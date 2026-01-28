use yew::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use web_sys::MouseEvent;
use wasm_bindgen::JsCast;

use crate::filesystem::FileSystem;
use crate::components::window::{Window, WindowState, WindowContentType};
use crate::components::taskbar::{Taskbar, TaskbarWindow};
use crate::components::start_menu::StartMenu;
use crate::components::context_menu::{ContextMenu, get_desktop_context_menu};
use crate::components::notification::{NotificationContainer, NotificationManager, NotificationType};

pub struct Desktop {
    fs: Rc<RefCell<FileSystem>>,
    windows: Vec<Rc<RefCell<WindowState>>>,
    next_window_id: u32,
    start_menu_visible: bool,
    context_menu: Option<(i32, i32)>,
    notifications: NotificationManager,
    theme: String,
    wallpaper: String,
}

pub enum DesktopMsg {
    // Window management
    OpenWindow(WindowContentType, String),
    CloseWindow(String),
    FocusWindow(String),
    MinimizeWindow(String),
    MaximizeWindow(String),
    
    // Start menu
    ToggleStartMenu,
    CloseStartMenu,
    LaunchApp(String),
    
    // Context menu
    ShowContextMenu(i32, i32),
    HideContextMenu,
    ContextMenuSelect(String),
    
    // Desktop events
    DesktopClick,
    DesktopContextMenu(MouseEvent),
    
    // Open file from file explorer
    OpenFile(String, String),
    
    // Notifications
    ShowNotification(String, String, String),
    DismissNotification(u32),
    
    // Settings
    SetTheme(String),
    SetWallpaper(String),
}

impl Component for Desktop {
    type Message = DesktopMsg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        let fs = Rc::new(RefCell::new(FileSystem::default()));
        
        Self {
            fs,
            windows: Vec::new(),
            next_window_id: 1,
            start_menu_visible: false,
            context_menu: None,
            notifications: NotificationManager::new(),
            theme: "dark".to_string(),
            wallpaper: "gradient1".to_string(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DesktopMsg::OpenWindow(content_type, title) => {
                self.create_window(content_type, title, ctx);
                true
            }
            DesktopMsg::CloseWindow(id) => {
                self.windows.retain(|w| w.borrow().id != id);
                true
            }
            DesktopMsg::FocusWindow(id) => {
                for window in &self.windows {
                    let mut w = window.borrow_mut();
                    w.is_focused = w.id == id;
                    if w.id == id && w.is_minimized {
                        w.is_minimized = false;
                    }
                }
                true
            }
            DesktopMsg::MinimizeWindow(id) => {
                for window in &self.windows {
                    let mut w = window.borrow_mut();
                    if w.id == id {
                        w.is_minimized = true;
                        w.is_focused = false;
                    }
                }
                true
            }
            DesktopMsg::MaximizeWindow(id) => {
                for window in &self.windows {
                    let mut w = window.borrow_mut();
                    if w.id == id {
                        if w.is_maximized {
                            // Restore
                            if let Some((x, y, width, height)) = w.restore_rect {
                                w.x = x;
                                w.y = y;
                                w.width = width;
                                w.height = height;
                            }
                            w.is_maximized = false;
                        } else {
                            // Save current rect and maximize
                            w.restore_rect = Some((w.x, w.y, w.width, w.height));
                            w.is_maximized = true;
                        }
                    }
                }
                true
            }
            DesktopMsg::ToggleStartMenu => {
                self.start_menu_visible = !self.start_menu_visible;
                self.context_menu = None;
                true
            }
            DesktopMsg::CloseStartMenu => {
                self.start_menu_visible = false;
                true
            }
            DesktopMsg::LaunchApp(app_id) => {
                self.launch_app(&app_id, ctx);
                self.start_menu_visible = false;
                true
            }
            DesktopMsg::ShowContextMenu(x, y) => {
                self.context_menu = Some((x, y));
                self.start_menu_visible = false;
                true
            }
            DesktopMsg::HideContextMenu => {
                self.context_menu = None;
                true
            }
            DesktopMsg::ContextMenuSelect(action) => {
                self.handle_context_menu_action(&action, ctx);
                self.context_menu = None;
                true
            }
            DesktopMsg::DesktopClick => {
                self.start_menu_visible = false;
                self.context_menu = None;
                // Unfocus all windows
                for window in &self.windows {
                    window.borrow_mut().is_focused = false;
                }
                true
            }
            DesktopMsg::DesktopContextMenu(event) => {
                event.prevent_default();
                self.context_menu = Some((event.client_x(), event.client_y()));
                self.start_menu_visible = false;
                true
            }
            DesktopMsg::OpenFile(path, _file_type) => {
                let title = path.rsplit('/').next().unwrap_or("File").to_string();
                self.create_window(
                    WindowContentType::TextEditor { file_path: Some(path) },
                    title,
                    ctx
                );
                true
            }
            DesktopMsg::ShowNotification(title, message, notification_type) => {
                let notif_type = match notification_type.as_str() {
                    "success" => NotificationType::Success,
                    "error" => NotificationType::Error,
                    "warning" => NotificationType::Warning,
                    _ => NotificationType::Info,
                };
                self.notifications.add(title, message, notif_type);
                true
            }
            DesktopMsg::DismissNotification(id) => {
                self.notifications.remove(id);
                true
            }
            DesktopMsg::SetTheme(theme) => {
                self.theme = theme;
                true
            }
            DesktopMsg::SetWallpaper(wallpaper) => {
                self.wallpaper = wallpaper;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let wallpaper_style = self.get_wallpaper_style();
        
        let on_desktop_click = ctx.link().callback(|_| DesktopMsg::DesktopClick);
        let on_context_menu = ctx.link().callback(DesktopMsg::DesktopContextMenu);
        
        let taskbar_windows: Vec<TaskbarWindow> = self.windows.iter()
            .map(|w| {
                let w = w.borrow();
                TaskbarWindow {
                    id: w.id.clone(),
                    title: w.title.clone(),
                    icon: self.get_window_icon(&w.content_type),
                    is_minimized: w.is_minimized,
                    is_focused: w.is_focused,
                }
            })
            .collect();

        html! {
            <div 
                class="desktop"
                style={format!(
                    "position: relative; width: 100vw; height: 100vh; overflow: hidden; {}",
                    wallpaper_style
                )}
                onclick={on_desktop_click}
                oncontextmenu={on_context_menu}
            >
                // Desktop icons
                <div style="position: absolute; top: 16px; left: 16px; display: flex; flex-direction: column; gap: 8px;">
                    { self.render_desktop_icon(ctx, "📁", "Files", "file-explorer") }
                    { self.render_desktop_icon(ctx, "💻", "Terminal", "terminal") }
                    { self.render_desktop_icon(ctx, "📝", "Notes", "text-editor") }
                    { self.render_desktop_icon(ctx, "🔢", "Calculator", "calculator") }
                    { self.render_desktop_icon(ctx, "🎨", "Paint", "paint") }
                    { self.render_desktop_icon(ctx, "💣", "Minesweeper", "minesweeper") }
                    { self.render_desktop_icon(ctx, "⚙️", "Settings", "settings") }
                </div>
                
                // Windows
                {
                    self.windows.iter().map(|window| {
                        let window_id = window.borrow().id.clone();
                        let fs = Rc::clone(&self.fs);
                        
                        let on_close = ctx.link().callback(DesktopMsg::CloseWindow);
                        let on_focus = ctx.link().callback(DesktopMsg::FocusWindow);
                        let on_minimize = ctx.link().callback(DesktopMsg::MinimizeWindow);
                        let on_maximize = ctx.link().callback(DesktopMsg::MaximizeWindow);
                        let on_open_file = ctx.link().callback(|(path, file_type)| {
                            DesktopMsg::OpenFile(path, file_type)
                        });
                        let on_notification = ctx.link().callback(|(title, message, ntype)| {
                            DesktopMsg::ShowNotification(title, message, ntype)
                        });
                        
                        html! {
                            <Window
                                key={window_id}
                                window={Rc::clone(window)}
                                {fs}
                                {on_close}
                                {on_focus}
                                {on_minimize}
                                {on_maximize}
                                {on_open_file}
                                {on_notification}
                            />
                        }
                    }).collect::<Html>()
                }
                
                // Taskbar
                <Taskbar 
                    windows={taskbar_windows}
                    on_window_click={ctx.link().callback(DesktopMsg::FocusWindow)}
                    on_start_click={ctx.link().callback(|_| DesktopMsg::ToggleStartMenu)}
                    on_quick_launch={ctx.link().callback(DesktopMsg::LaunchApp)}
                />
                
                // Start Menu
                <StartMenu 
                    visible={self.start_menu_visible}
                    on_close={ctx.link().callback(|_| DesktopMsg::CloseStartMenu)}
                    on_launch_app={ctx.link().callback(DesktopMsg::LaunchApp)}
                />
                
                // Context Menu
                {
                    if let Some((x, y)) = self.context_menu {
                        html! {
                            <ContextMenu 
                                x={x}
                                y={y}
                                items={get_desktop_context_menu()}
                                on_select={ctx.link().callback(DesktopMsg::ContextMenuSelect)}
                                on_close={ctx.link().callback(|_| DesktopMsg::HideContextMenu)}
                            />
                        }
                    } else {
                        html! {}
                    }
                }
                
                // Notifications
                <NotificationContainer 
                    notifications={self.notifications.get_all()}
                    on_dismiss={ctx.link().callback(DesktopMsg::DismissNotification)}
                />
            </div>
        }
    }
}

impl Desktop {
    fn create_window(&mut self, content_type: WindowContentType, title: String, ctx: &Context<Self>) {
        let id = format!("window-{}", self.next_window_id);
        self.next_window_id += 1;
        
        // Unfocus all existing windows
        for window in &self.windows {
            window.borrow_mut().is_focused = false;
        }
        
        // Offset new windows
        let offset = (self.windows.len() as i32 % 10) * 30;
        
        let content_type = match content_type {
            WindowContentType::Settings { .. } => {
                WindowContentType::Settings {
                    on_theme_change: ctx.link().callback(DesktopMsg::SetTheme),
                    on_wallpaper_change: ctx.link().callback(DesktopMsg::SetWallpaper),
                }
            }
            other => other,
        };
        
        let mut window = WindowState::new(id, title, content_type);
        window.x += offset;
        window.y += offset;
        
        self.windows.push(Rc::new(RefCell::new(window)));
    }
    
    fn launch_app(&mut self, app_id: &str, ctx: &Context<Self>) {
        let (content_type, title) = match app_id {
            "file-explorer" => (WindowContentType::FileExplorer, "File Explorer"),
            "terminal" => (WindowContentType::Terminal, "Terminal"),
            "text-editor" => (WindowContentType::TextEditor { file_path: None }, "Text Editor"),
            "calculator" => (WindowContentType::Calculator, "Calculator"),
            "clock" => (WindowContentType::Clock, "Clock"),
            "paint" => (WindowContentType::Paint, "Paint"),
            "minesweeper" => (WindowContentType::Minesweeper, "Minesweeper"),
            "settings" => (
                WindowContentType::Settings { 
                    on_theme_change: ctx.link().callback(DesktopMsg::SetTheme),
                    on_wallpaper_change: ctx.link().callback(DesktopMsg::SetWallpaper),
                },
                "Settings"
            ),
            "about" => (WindowContentType::About, "About KernelOS"),
            _ => return,
        };
        
        self.create_window(content_type, title.to_string(), ctx);
    }
    
    fn handle_context_menu_action(&mut self, action: &str, ctx: &Context<Self>) {
        match action {
            "new-folder" => {
                let path = format!("/home/New Folder {}", self.next_window_id);
                match self.fs.borrow_mut().create_directory(&path, false) {
                    Ok(_) => {
                        self.notifications.add(
                            "Folder Created".to_string(),
                            format!("Created {}", path),
                            NotificationType::Success
                        );
                    }
                    Err(e) => {
                        self.notifications.add(
                            "Error".to_string(),
                            e,
                            NotificationType::Error
                        );
                    }
                }
            }
            "new-file" => {
                let path = format!("/home/untitled_{}.txt", self.next_window_id);
                let result = self.fs.borrow_mut().write_file(&path, "");
                match result {
                    Ok(_) => {
                        self.notifications.add(
                            "File Created".to_string(),
                            format!("Created {}", path),
                            NotificationType::Success
                        );
                        self.launch_app("file-explorer", ctx);
                    }
                    Err(e) => {
                        self.notifications.add(
                            "Error".to_string(),
                            e,
                            NotificationType::Error
                        );
                    }
                }
            }
            "file-explorer" | "terminal" | "text-editor" | "calculator" | 
            "paint" | "minesweeper" | "settings" | "about" => {
                self.launch_app(action, ctx);
            }
            "wallpaper-gradient1" => ctx.link().send_message(DesktopMsg::SetWallpaper("gradient1".to_string())),
            "wallpaper-gradient2" => ctx.link().send_message(DesktopMsg::SetWallpaper("gradient2".to_string())),
            "wallpaper-gradient3" => ctx.link().send_message(DesktopMsg::SetWallpaper("gradient3".to_string())),
            "wallpaper-gradient4" => ctx.link().send_message(DesktopMsg::SetWallpaper("gradient4".to_string())),
            "wallpaper-solid" => ctx.link().send_message(DesktopMsg::SetWallpaper("solid1".to_string())),
            _ => {}
        }
    }
    
    fn render_desktop_icon(&self, ctx: &Context<Self>, icon: &str, label: &str, app_id: &str) -> Html {
        let app_id_str = app_id.to_string();
        let on_dblclick = ctx.link().callback(move |e: MouseEvent| {
            e.stop_propagation();
            DesktopMsg::LaunchApp(app_id_str.clone())
        });
        
        html! {
            <div 
                style="display: flex; flex-direction: column; align-items: center; padding: 8px; \
                       border-radius: 8px; cursor: pointer; width: 80px; transition: background-color 0.2s ease;"
                ondblclick={on_dblclick}
                onmouseover={Callback::from(|e: MouseEvent| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        let _ = el.style().set_property("background-color", "rgba(255,255,255,0.1)");
                    }
                })}
                onmouseout={Callback::from(|e: MouseEvent| {
                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                        let _ = el.style().set_property("background-color", "transparent");
                    }
                })}
            >
                <span style="font-size: 36px; margin-bottom: 4px;">{ icon }</span>
                <span style="color: white; font-size: 11px; text-align: center; text-shadow: 1px 1px 2px rgba(0,0,0,0.8); word-wrap: break-word; max-width: 70px;">
                    { label }
                </span>
            </div>
        }
    }
    
    fn get_window_icon(&self, content_type: &WindowContentType) -> String {
        match content_type {
            WindowContentType::FileExplorer => "📁".to_string(),
            WindowContentType::Terminal => "💻".to_string(),
            WindowContentType::TextEditor { .. } => "📝".to_string(),
            WindowContentType::Calculator => "🔢".to_string(),
            WindowContentType::Clock => "🕐".to_string(),
            WindowContentType::Paint => "🎨".to_string(),
            WindowContentType::Minesweeper => "💣".to_string(),
            WindowContentType::Settings { .. } => "⚙️".to_string(),
            WindowContentType::About => "ℹ️".to_string(),
            WindowContentType::Empty => "📄".to_string(),
        }
    }
    
    fn get_wallpaper_style(&self) -> String {
        match self.wallpaper.as_str() {
            "gradient1" => "background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);".to_string(),
            "gradient2" => "background: linear-gradient(135deg, #f093fb 0%, #f5576c 50%, #4facfe 100%);".to_string(),
            "gradient3" => "background: linear-gradient(135deg, #134e5e 0%, #71b280 100%);".to_string(),
            "gradient4" => "background: linear-gradient(135deg, #0f0c29 0%, #302b63 50%, #24243e 100%);".to_string(),
            "gradient5" => "background: linear-gradient(135deg, #00c6ff 0%, #0072ff 50%, #7c3aed 100%);".to_string(),
            "gradient6" => "background: linear-gradient(135deg, #ff416c 0%, #ff4b2b 100%);".to_string(),
            "solid1" => "background-color: #1a1a1a;".to_string(),
            "solid2" => "background-color: #1e3a5f;".to_string(),
            _ => "background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);".to_string(),
        }
    }
}
