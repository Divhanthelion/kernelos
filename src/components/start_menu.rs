use yew::prelude::*;
use wasm_bindgen::JsCast;

#[derive(Clone, PartialEq)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub category: String,
}

impl AppEntry {
    pub fn new(id: &str, name: &str, icon: &str, category: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            category: category.to_string(),
        }
    }
}

pub fn get_default_apps() -> Vec<AppEntry> {
    vec![
        AppEntry::new("file-explorer", "File Explorer", "📁", "System"),
        AppEntry::new("terminal", "Terminal", "💻", "System"),
        AppEntry::new("text-editor", "Text Editor", "📝", "Productivity"),
        AppEntry::new("calculator", "Calculator", "🔢", "Utilities"),
        AppEntry::new("clock", "Clock", "🕐", "Utilities"),
        AppEntry::new("paint", "Paint", "🎨", "Creative"),
        AppEntry::new("minesweeper", "Minesweeper", "💣", "Games"),
        AppEntry::new("settings", "Settings", "⚙️", "System"),
        AppEntry::new("about", "About KernelOS", "ℹ️", "System"),
    ]
}

#[derive(Properties, Clone, PartialEq)]
pub struct StartMenuProps {
    pub visible: bool,
    pub on_close: Callback<()>,
    pub on_launch_app: Callback<String>,
}

pub struct StartMenu {
    search_query: String,
    apps: Vec<AppEntry>,
}

pub enum StartMenuMsg {
    SearchChanged(String),
    LaunchApp(String),
    Close,
}

impl Component for StartMenu {
    type Message = StartMenuMsg;
    type Properties = StartMenuProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            search_query: String::new(),
            apps: get_default_apps(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            StartMenuMsg::SearchChanged(query) => {
                self.search_query = query;
                true
            }
            StartMenuMsg::LaunchApp(id) => {
                ctx.props().on_launch_app.emit(id);
                ctx.props().on_close.emit(());
                false
            }
            StartMenuMsg::Close => {
                ctx.props().on_close.emit(());
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        if !ctx.props().visible {
            return html! {};
        }

        let filtered_apps: Vec<&AppEntry> = self.apps
            .iter()
            .filter(|app| {
                if self.search_query.is_empty() {
                    true
                } else {
                    app.name.to_lowercase().contains(&self.search_query.to_lowercase()) ||
                    app.category.to_lowercase().contains(&self.search_query.to_lowercase())
                }
            })
            .collect();

        let on_search = ctx.link().callback(|e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            StartMenuMsg::SearchChanged(input.value())
        });

        let on_overlay_click = ctx.link().callback(|_| StartMenuMsg::Close);

        html! {
            <>
                <div 
                    style="position: fixed; top: 0; left: 0; width: 100%; height: 100%; z-index: 1400;"
                    onclick={on_overlay_click}
                />
                <div class="start-menu" style="position: fixed; bottom: 56px; left: 8px; width: 320px; max-height: 500px; background-color: #2d2d2d; border-radius: 12px; box-shadow: 0 8px 32px rgba(0,0,0,0.3); z-index: 1500; overflow: hidden; border: 1px solid rgba(255,255,255,0.1);">
                    <div class="start-menu-header" style="padding: 16px; background: linear-gradient(135deg, #4a9eff 0%, #e94560 100%); color: white;">
                        <input 
                            type="text"
                            placeholder="Search apps..."
                            value={self.search_query.clone()}
                            oninput={on_search}
                            style="width: 100%; padding: 10px 14px; border: none; border-radius: 8px; background-color: rgba(255,255,255,0.2); color: white; font-size: 14px; outline: none;"
                        />
                    </div>
                    <div class="start-menu-apps" style="padding: 8px; max-height: 380px; overflow-y: auto;">
                        {
                            filtered_apps.iter().map(|app| {
                                let app_id = app.id.clone();
                                let on_click = ctx.link().callback(move |_| {
                                    StartMenuMsg::LaunchApp(app_id.clone())
                                });
                                
                                html! {
                                    <div 
                                        class="start-menu-app"
                                        style="display: flex; align-items: center; padding: 12px; border-radius: 8px; cursor: pointer; transition: background-color 0.15s ease;"
                                        onclick={on_click}
                                        onmouseover={Callback::from(|e: MouseEvent| {
                                            if let Some(target) = e.target() {
                                                if let Some(el) = target.dyn_ref::<web_sys::HtmlElement>() {
                                                    let _ = el.style().set_property("background-color", "rgba(255,255,255,0.1)");
                                                }
                                            }
                                        })}
                                        onmouseout={Callback::from(|e: MouseEvent| {
                                            if let Some(target) = e.target() {
                                                if let Some(el) = target.dyn_ref::<web_sys::HtmlElement>() {
                                                    let _ = el.style().set_property("background-color", "transparent");
                                                }
                                            }
                                        })}
                                    >
                                        <span style="font-size: 28px; margin-right: 14px;">{ &app.icon }</span>
                                        <div>
                                            <div style="color: white; font-size: 14px; font-weight: 500;">{ &app.name }</div>
                                            <div style="color: rgba(255,255,255,0.5); font-size: 11px;">{ &app.category }</div>
                                        </div>
                                    </div>
                                }
                            }).collect::<Html>()
                        }
                        {
                            if filtered_apps.is_empty() {
                                html! {
                                    <div style="padding: 24px; text-align: center; color: rgba(255,255,255,0.5);">
                                        { "No apps found" }
                                    </div>
                                }
                            } else {
                                html! {}
                            }
                        }
                    </div>
                </div>
            </>
        }
    }
}
