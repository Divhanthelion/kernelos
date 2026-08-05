use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub struct ContextMenuItem {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub shortcut: Option<String>,
    pub divider_after: bool,
    pub submenu: Option<Vec<ContextMenuItem>>,
}

impl ContextMenuItem {
    pub fn new(id: &str, label: &str, icon: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            icon: icon.to_string(),
            shortcut: None,
            divider_after: false,
            submenu: None,
        }
    }

    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.to_string());
        self
    }

    pub fn with_divider(mut self) -> Self {
        self.divider_after = true;
        self
    }

    pub fn with_submenu(mut self, items: Vec<ContextMenuItem>) -> Self {
        self.submenu = Some(items);
        self
    }
}

pub fn get_desktop_context_menu() -> Vec<ContextMenuItem> {
    vec![
        ContextMenuItem::new("new-folder", "New Folder", "📁"),
        ContextMenuItem::new("new-file", "New Text File", "📄").with_divider(),
        ContextMenuItem::new("file-explorer", "Open File Explorer", "📂"),
        ContextMenuItem::new("terminal", "Open Terminal", "💻"),
        ContextMenuItem::new("text-editor", "Open Text Editor", "📝").with_divider(),
        ContextMenuItem::new("calculator", "Calculator", "🔢"),
        ContextMenuItem::new("paint", "Paint", "🎨"),
        ContextMenuItem::new("minesweeper", "Minesweeper", "💣").with_divider(),
        ContextMenuItem::new("wallpaper", "Change Wallpaper", "🖼️")
            .with_submenu(vec![
                ContextMenuItem::new("wallpaper-gradient1", "Ocean", "🌊"),
                ContextMenuItem::new("wallpaper-gradient2", "Sunset", "🌅"),
                ContextMenuItem::new("wallpaper-gradient3", "Forest", "🌲"),
                ContextMenuItem::new("wallpaper-gradient4", "Night", "🌙"),
                ContextMenuItem::new("wallpaper-solid", "Solid Color", "🎨"),
            ]),
        ContextMenuItem::new("settings", "Settings", "⚙️").with_divider(),
        ContextMenuItem::new("about", "About KernelOS", "ℹ️"),
    ]
}

#[derive(Properties, Clone, PartialEq)]
pub struct ContextMenuProps {
    pub x: i32,
    pub y: i32,
    pub items: Vec<ContextMenuItem>,
    pub on_select: Callback<String>,
    pub on_close: Callback<()>,
}

pub struct ContextMenu {
    adjusted_x: i32,
    adjusted_y: i32,
    active_submenu: Option<String>,
}

pub enum ContextMenuMsg {
    Select(String),
    Close,
    ShowSubmenu(String),
    HideSubmenu,
}

impl Component for ContextMenu {
    type Message = ContextMenuMsg;
    type Properties = ContextMenuProps;

    fn create(ctx: &Context<Self>) -> Self {
        // Adjust position to keep menu on screen
        let window = web_sys::window().unwrap();
        let inner_width = window.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(800.0) as i32;
        let inner_height = window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(600.0) as i32;
        
        let menu_width = 220;
        let menu_height = (ctx.props().items.len() as i32 * 40) + 16;
        
        let adjusted_x = if ctx.props().x + menu_width > inner_width {
            inner_width - menu_width - 8
        } else {
            ctx.props().x
        };
        
        let adjusted_y = if ctx.props().y + menu_height > inner_height - 48 {
            (inner_height - 48 - menu_height).max(8)
        } else {
            ctx.props().y
        };

        Self {
            adjusted_x,
            adjusted_y,
            active_submenu: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ContextMenuMsg::Select(id) => {
                ctx.props().on_select.emit(id);
                ctx.props().on_close.emit(());
                false
            }
            ContextMenuMsg::Close => {
                ctx.props().on_close.emit(());
                false
            }
            ContextMenuMsg::ShowSubmenu(id) => {
                self.active_submenu = Some(id);
                true
            }
            ContextMenuMsg::HideSubmenu => {
                self.active_submenu = None;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_overlay_click = ctx.link().callback(|_| ContextMenuMsg::Close);
        
        let menu_style = format!(
            "position: fixed; left: {}px; top: {}px; min-width: 200px; \
             background-color: #2d2d2d; border-radius: 8px; \
             box-shadow: 0 8px 32px rgba(0,0,0,0.3); padding: 8px 0; z-index: 2000; \
             border: 1px solid rgba(255,255,255,0.1);",
            self.adjusted_x, self.adjusted_y
        );

        html! {
            <>
                <div 
                    class="context-menu-backdrop"
                    onclick={on_overlay_click}
                    oncontextmenu={ctx.link().callback(|e: MouseEvent| {
                        e.prevent_default();
                        ContextMenuMsg::Close
                    })}
                />
                <div class="context-menu" style={menu_style}>
                    { self.render_items(ctx, &ctx.props().items, false) }
                </div>
            </>
        }
    }
}

impl ContextMenu {
    fn render_items(&self, ctx: &Context<Self>, items: &[ContextMenuItem], _is_submenu: bool) -> Html {
        html! {
            <>
                {
                    items.iter().map(|item| {
                        let item_id = item.id.clone();
                        let has_submenu = item.submenu.is_some();
                        
                        let on_click = if has_submenu {
                            ctx.link().callback(move |e: MouseEvent| {
                                e.stop_propagation();
                                ContextMenuMsg::ShowSubmenu(item_id.clone())
                            })
                        } else {
                            let id = item.id.clone();
                            ctx.link().callback(move |e: MouseEvent| {
                                e.stop_propagation();
                                ContextMenuMsg::Select(id.clone())
                            })
                        };
                        
                        let item_id_hover = item.id.clone();
                        let on_mouse_enter = if has_submenu {
                            Some(ctx.link().callback(move |_| {
                                ContextMenuMsg::ShowSubmenu(item_id_hover.clone())
                            }))
                        } else {
                            Some(ctx.link().callback(|_| ContextMenuMsg::HideSubmenu))
                        };

                        let is_active = self.active_submenu.as_ref() == Some(&item.id);

                        html! {
                            <>
                                <div
                                    class={classes!(
                                        "context-menu-item",
                                        "context-menu-submenu",
                                        is_active.then_some("active"),
                                    )}
                                    onclick={on_click}
                                    onmouseenter={on_mouse_enter}
                                >
                                    <span class="context-menu-item-icon">{ &item.icon }</span>
                                    <span class="context-menu-item-label">{ &item.label }</span>
                                    {
                                        if let Some(shortcut) = &item.shortcut {
                                            html! {
                                                <span class="context-menu-shortcut">
                                                    { shortcut }
                                                </span>
                                            }
                                        } else {
                                            html! {}
                                        }
                                    }
                                    {
                                        if has_submenu {
                                            html! {
                                                <span class="context-menu-arrow">{ "▶" }</span>
                                            }
                                        } else {
                                            html! {}
                                        }
                                    }
                                    {
                                        if is_active {
                                            if let Some(submenu) = &item.submenu {
                                                html! {
                                                    <div class="context-menu-submenu-panel">
                                                        {
                                                            submenu.iter().map(|sub_item| {
                                                                let sub_id = sub_item.id.clone();
                                                                let on_sub_click = ctx.link().callback(move |e: MouseEvent| {
                                                                    e.stop_propagation();
                                                                    ContextMenuMsg::Select(sub_id.clone())
                                                                });
                                                                
                                                                html! {
                                                                    <div
                                                                        class="context-menu-item"
                                                                        onclick={on_sub_click}
                                                                    >
                                                                        <span class="context-menu-item-icon">{ &sub_item.icon }</span>
                                                                        <span>{ &sub_item.label }</span>
                                                                    </div>
                                                                }
                                                            }).collect::<Html>()
                                                        }
                                                    </div>
                                                }
                                            } else {
                                                html! {}
                                            }
                                        } else {
                                            html! {}
                                        }
                                    }
                                </div>
                                {
                                    if item.divider_after {
                                        html! {
                                            <div class="context-menu-separator" />
                                        }
                                    } else {
                                        html! {}
                                    }
                                }
                            </>
                        }
                    }).collect::<Html>()
                }
            </>
        }
    }
}
