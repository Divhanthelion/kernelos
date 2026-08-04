use yew::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use web_sys::MouseEvent;
use wasm_bindgen::JsCast;

use crate::apps;
use crate::plugin;
use crate::filesystem::FileSystem;
use crate::session::{Session, SessionWindow, ThemeConfig};
use crate::components::window::{Window, WindowState};
use crate::components::taskbar::{Taskbar, TaskbarWindow};
use crate::components::start_menu::StartMenu;
use crate::components::context_menu::{ContextMenu, get_desktop_context_menu};
use crate::components::notification::{NotificationContainer, NotificationManager, NotificationType};

pub struct Desktop {
    fs: Rc<RefCell<FileSystem>>,
    windows: Vec<Rc<RefCell<WindowState>>>,
    next_window_id: u32,
    /// Monotonic counter handing out stacking order; the newest raise wins.
    next_z_index: u32,
    start_menu_visible: bool,
    context_menu: Option<(i32, i32)>,
    notifications: NotificationManager,
    theme: String,
    accent: String,
    wallpaper: String,
    /// Session restore waits until asynchronous plugin storage is ready.
    pending_session: Option<Session>,
}

pub enum DesktopMsg {
    // Window management
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
    
    /// A drag or resize finished; persist the new geometry.
    WindowGeometryChanged,

    // Settings
    SetTheme(String),
    SetAccent(String),
    SetWallpaper(String),

    /// A plugin finished loading in the background; re-render so menus, icons
    /// and quick launch pick up the new entry.
    PluginsChanged,
    /// Persisted plugin bytes are loaded and saved windows can be restored.
    PluginsReady,
}

impl Component for Desktop {
    type Message = DesktopMsg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let fs = Rc::new(RefCell::new(FileSystem::default()));

        let config = ThemeConfig::load(&fs.borrow());
        let session = Session::load(&fs.borrow());

        // Plugin notifications arrive as (title, body); Desktop adds the kind.
        let on_plugin_notify = {
            let link = ctx.link().clone();
            Callback::from(move |(title, body): (String, String)| {
                link.send_message(DesktopMsg::ShowNotification(title, body, "info".to_string()));
            })
        };
        let on_plugins_changed = ctx.link().callback(|_| DesktopMsg::PluginsChanged);
        let on_plugins_ready = ctx.link().callback(|_| DesktopMsg::PluginsReady);

        let desktop = Self {
            fs,
            windows: Vec::new(),
            next_window_id: 1,
            next_z_index: 1,
            start_menu_visible: false,
            context_menu: None,
            notifications: NotificationManager::new(),
            theme: config.theme,
            accent: config.accent,
            wallpaper: config.wallpaper,
            pending_session: Some(session),
        };

        // IndexedDB plugin bytes load asynchronously. Session restore is
        // deferred until the registry is ready.
        plugin::init(
            Rc::clone(&desktop.fs),
            on_plugin_notify.clone(),
            on_plugins_changed,
            on_plugins_ready,
        );

        desktop
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            self.apply_theme_to_document();
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            DesktopMsg::CloseWindow(id) => {
                self.windows.retain(|w| w.borrow().id != id);
                self.save_session();
                true
            }
            DesktopMsg::FocusWindow(id) => {
                let already_on_top = self.windows.iter().any(|w| {
                    let w = w.borrow();
                    w.id == id && w.is_focused && !w.is_minimized
                });
                if already_on_top {
                    return false;
                }

                let z = self.take_z_index();
                for window in &self.windows {
                    let mut w = window.borrow_mut();
                    w.is_focused = w.id == id;
                    if w.id == id {
                        w.is_minimized = false;
                        w.z_index = z;
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
                self.save_session();
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
                self.save_session();
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
                self.create_window("text-editor", title, Some(path));
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
                self.apply_theme_to_document();
                self.save_theme_config();
                true
            }
            DesktopMsg::SetAccent(color) => {
                self.accent = color;
                self.apply_theme_to_document();
                self.save_theme_config();
                true
            }
            DesktopMsg::SetWallpaper(wallpaper) => {
                self.wallpaper = wallpaper;
                self.save_theme_config();
                true
            }
            DesktopMsg::WindowGeometryChanged => {
                // Fired once when a drag or resize ends, not per frame.
                self.save_session();
                false
            }
            DesktopMsg::PluginsChanged => {
                // Menus, icons and quick launch read the registry fresh each
                // render, so a re-render is all that is needed.
                true
            }
            DesktopMsg::PluginsReady => {
                if let Some(session) = self.pending_session.take() {
                    let link = ctx.link().clone();
                    let on_plugin_notify = Callback::from(move |(title, body): (String, String)| {
                        link.send_message(DesktopMsg::ShowNotification(
                            title,
                            body,
                            "info".to_string(),
                        ));
                    });
                    self.restore_session(session, &on_plugin_notify);
                    // Persist the merged result, including any windows the user
                    // opened while plugin storage was still hydrating.
                    self.save_session();
                }
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
                    icon: apps::icon_for(&w.app_id).to_string(),
                    is_minimized: w.is_minimized,
                    is_focused: w.is_focused,
                }
            })
            .collect();

        html! {
            <div
                class="desktop"
                style={wallpaper_style}
                onclick={on_desktop_click}
                oncontextmenu={on_context_menu}
            >
                // Desktop icons
                <div class="desktop-icons">
                    {
                        apps::all_apps().into_iter()
                            .filter(|app| app.on_desktop)
                            .map(|app| self.render_desktop_icon(ctx, &app.icon, &app.title, &app.id))
                            .collect::<Html>()
                    }
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
                                on_theme_change={ctx.link().callback(DesktopMsg::SetTheme)}
                                on_wallpaper_change={ctx.link().callback(DesktopMsg::SetWallpaper)}
                                on_accent_change={ctx.link().callback(DesktopMsg::SetAccent)}
                                on_geometry_changed={ctx.link().callback(|_| DesktopMsg::WindowGeometryChanged)}
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
    fn restore_session(&mut self, session: Session, on_plugin_notify: &Callback<(String, String)>) {
        for saved in session.windows {
            // Drop windows whose app has since left the registry rather than
            // resurrecting an unopenable shell. Plugins that are still
            // installed come back with a fresh per-window instance.
            if apps::find(&saved.app_id).is_none() {
                if plugin::is_installed(&saved.app_id) {
                    self.restore_plugin_window(&saved, on_plugin_notify);
                }
                continue;
            }

            let id = format!("window-{}", self.next_window_id);
            self.next_window_id += 1;
            let z_index = self.take_z_index();

            let mut window = WindowState::new(id, &saved.app_id, saved.title, saved.arg);
            window.x = saved.x;
            window.y = saved.y;
            window.width = saved.width;
            window.height = saved.height;
            window.is_maximized = saved.is_maximized;
            window.is_minimized = saved.is_minimized;
            window.is_focused = false;
            window.z_index = z_index;
            if saved.is_maximized {
                window.restore_rect = Some((saved.x, saved.y, saved.width, saved.height));
            }

            self.windows.push(Rc::new(RefCell::new(window)));
        }

        // Focus the topmost restored window so the desktop comes back usable.
        if let Some(last) = self.windows.last() {
            last.borrow_mut().is_focused = true;
        }
    }

    /// Restore one plugin-backed window: re-instantiate the guest and rebuild
    /// the `WindowState` with the saved geometry. A failed instantiation logs
    /// and drops the window rather than showing a dead shell.
    fn restore_plugin_window(
        &mut self,
        saved: &SessionWindow,
        on_plugin_notify: &Callback<(String, String)>,
    ) {
        match plugin::instantiate(&saved.app_id, &self.fs, on_plugin_notify.clone()) {
            Ok(handle) => {
                let id = format!("window-{}", self.next_window_id);
                self.next_window_id += 1;
                let z_index = self.take_z_index();

                let mut window = WindowState::new(
                    id,
                    &saved.app_id,
                    saved.title.clone(),
                    saved.arg.clone(),
                );
                window.x = saved.x;
                window.y = saved.y;
                window.width = saved.width;
                window.height = saved.height;
                window.is_maximized = saved.is_maximized;
                window.is_minimized = saved.is_minimized;
                window.is_focused = false;
                window.z_index = z_index;
                window.plugin_handle = Some(Rc::new(RefCell::new(handle)));
                if saved.is_maximized {
                    window.restore_rect = Some((saved.x, saved.y, saved.width, saved.height));
                }

                self.windows.push(Rc::new(RefCell::new(window)));
            }
            Err(e) => {
                log::warn!("plugin '{}' failed to restore: {e}", saved.app_id);
            }
        }
    }

    fn save_session(&self) {
        // Do not overwrite the previous session while IndexedDB-backed plugin
        // state is still loading and its windows have not been restored.
        if self.pending_session.is_some() {
            return;
        }
        let session = Session {
            windows: self.windows.iter().map(|window| {
                let w = window.borrow();
                SessionWindow {
                    app_id: w.app_id.clone(),
                    arg: w.arg.clone(),
                    title: w.title.clone(),
                    // Persist the pre-maximize rect so restoring a maximized
                    // window still knows where to un-maximize to.
                    x: w.restore_rect.map(|r| r.0).unwrap_or(w.x),
                    y: w.restore_rect.map(|r| r.1).unwrap_or(w.y),
                    width: w.restore_rect.map(|r| r.2).unwrap_or(w.width),
                    height: w.restore_rect.map(|r| r.3).unwrap_or(w.height),
                    is_maximized: w.is_maximized,
                    is_minimized: w.is_minimized,
                }
            }).collect(),
        };

        session.save(&mut self.fs.borrow_mut());
    }

    fn save_theme_config(&self) {
        let config = ThemeConfig {
            theme: self.theme.clone(),
            accent: self.accent.clone(),
            wallpaper: self.wallpaper.clone(),
        };
        config.save(&mut self.fs.borrow_mut());
    }

    /// Push the theme onto the document root. `styles.css` keys its whole light
    /// palette off `[data-theme="light"]`, and every component colour reads from
    /// these variables, so this one attribute is what makes the theme real.
    fn apply_theme_to_document(&self) {
        let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        else {
            return;
        };

        let _ = root.set_attribute("data-theme", &self.theme);

        if let Some(root) = root.dyn_ref::<web_sys::HtmlElement>() {
            let _ = root.style().set_property("--accent-primary", &self.accent);
        }
    }

    fn take_z_index(&mut self) -> u32 {
        let z = self.next_z_index;
        self.next_z_index += 1;
        z
    }

    fn create_window(&mut self, app_id: &str, title: String, arg: Option<String>) {
        let id = format!("window-{}", self.next_window_id);
        self.next_window_id += 1;
        let z_index = self.take_z_index();

        // Unfocus all existing windows
        for window in &self.windows {
            window.borrow_mut().is_focused = false;
        }

        // Offset new windows
        let offset = (self.windows.len() as i32 % 10) * 30;

        let mut window = WindowState::new(id, app_id, title, arg);
        window.x += offset;
        window.y += offset;
        window.z_index = z_index;

        self.windows.push(Rc::new(RefCell::new(window)));
        self.save_session();
    }

    fn launch_app(&mut self, app_id: &str, ctx: &Context<Self>) {
        if let Some(app) = apps::find(app_id) {
            self.create_window(app.id, app.title.to_string(), None);
            return;
        }
        // Runtime-registered plugin: spin up a fresh per-window instance.
        if plugin::is_installed(app_id) {
            self.launch_plugin_window(app_id, ctx);
        }
    }

    /// Open a window for an installed plugin, instantiating its WASM module.
    fn launch_plugin_window(&mut self, app_id: &str, ctx: &Context<Self>) {
        let Some(info) = plugin::manifest(app_id) else {
            return;
        };
        // Plugin notifications arrive as (title, body); the desktop adds the kind.
        let on_notify = {
            let link = ctx.link().clone();
            Callback::from(move |(title, body): (String, String)| {
                link.send_message(DesktopMsg::ShowNotification(title, body, "info".to_string()));
            })
        };

        match plugin::instantiate(app_id, &self.fs, on_notify) {
            Ok(handle) => {
                let id = format!("window-{}", self.next_window_id);
                self.next_window_id += 1;
                let z_index = self.take_z_index();

                // Unfocus all existing windows
                for window in &self.windows {
                    window.borrow_mut().is_focused = false;
                }

                let offset = (self.windows.len() as i32 % 10) * 30;
                let mut window = WindowState::new(id, app_id, info.name, None);
                window.x += offset;
                window.y += offset;
                window.z_index = z_index;
                window.plugin_handle = Some(Rc::new(RefCell::new(handle)));

                self.windows.push(Rc::new(RefCell::new(window)));
                self.save_session();
            }
            Err(e) => {
                self.notifications
                    .add("Plugin Error".to_string(), e, NotificationType::Error);
            }
        }
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
        
        // Hover styling is CSS (.desktop-icon:hover), not hand-rolled JS.
        html! {
            <div class="desktop-icon" ondblclick={on_dblclick}>
                <span class="desktop-icon-image">{ icon }</span>
                <span class="desktop-icon-label">{ label }</span>
            </div>
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
