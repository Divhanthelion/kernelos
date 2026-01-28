use yew::prelude::*;

#[derive(Clone, PartialEq)]
enum SettingsTab {
    Appearance,
    Wallpaper,
    About,
}

pub struct Settings {
    active_tab: SettingsTab,
    selected_theme: String,
    selected_wallpaper: String,
    accent_color: String,
}

pub enum SettingsMsg {
    SetTab(SettingsTab),
    SetTheme(String),
    SetWallpaper(String),
    SetAccentColor(String),
}

#[derive(Properties, Clone, PartialEq)]
pub struct SettingsProps {
    pub on_theme_change: Callback<String>,
    pub on_wallpaper_change: Callback<String>,
}

impl Component for Settings {
    type Message = SettingsMsg;
    type Properties = SettingsProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            active_tab: SettingsTab::Appearance,
            selected_theme: "dark".to_string(),
            selected_wallpaper: "gradient1".to_string(),
            accent_color: "#4a9eff".to_string(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            SettingsMsg::SetTab(tab) => {
                self.active_tab = tab;
                true
            }
            SettingsMsg::SetTheme(theme) => {
                self.selected_theme = theme.clone();
                ctx.props().on_theme_change.emit(theme);
                true
            }
            SettingsMsg::SetWallpaper(wallpaper) => {
                self.selected_wallpaper = wallpaper.clone();
                ctx.props().on_wallpaper_change.emit(wallpaper);
                true
            }
            SettingsMsg::SetAccentColor(color) => {
                self.accent_color = color;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="settings" style="display: flex; height: 100%; background-color: #252526;">
                // Sidebar
                <div style="width: 200px; background-color: #1e1e1e; border-right: 1px solid #333; padding: 12px 0;">
                    <div style="padding: 12px 16px; color: white; font-size: 16px; font-weight: 600; border-bottom: 1px solid #333; margin-bottom: 8px;">
                        { "⚙️ Settings" }
                    </div>
                    { self.render_nav_item(ctx, "🎨", "Appearance", SettingsTab::Appearance) }
                    { self.render_nav_item(ctx, "🖼️", "Wallpaper", SettingsTab::Wallpaper) }
                    { self.render_nav_item(ctx, "ℹ️", "About", SettingsTab::About) }
                </div>
                
                // Content
                <div style="flex: 1; padding: 24px; overflow-y: auto;">
                    {
                        match self.active_tab {
                            SettingsTab::Appearance => self.render_appearance_tab(ctx),
                            SettingsTab::Wallpaper => self.render_wallpaper_tab(ctx),
                            SettingsTab::About => self.render_about_tab(),
                        }
                    }
                </div>
            </div>
        }
    }
}

impl Settings {
    fn render_nav_item(&self, ctx: &Context<Self>, icon: &str, label: &str, tab: SettingsTab) -> Html {
        let is_active = self.active_tab == tab;
        let tab_clone = tab.clone();
        
        html! {
            <div 
                style={format!(
                    "display: flex; align-items: center; padding: 12px 16px; cursor: pointer; \
                     transition: background-color 0.15s ease; {} {}",
                    if is_active { "background-color: rgba(74, 158, 255, 0.2); border-left: 3px solid #4a9eff;" } else { "border-left: 3px solid transparent;" },
                    if is_active { "color: white;" } else { "color: #d4d4d4;" }
                )}
                onclick={ctx.link().callback(move |_| SettingsMsg::SetTab(tab_clone.clone()))}
            >
                <span style="margin-right: 12px;">{ icon }</span>
                <span>{ label }</span>
            </div>
        }
    }

    fn render_appearance_tab(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <h2 style="color: white; font-size: 24px; margin: 0 0 24px 0; font-weight: 400;">{ "Appearance" }</h2>
                
                // Theme selection
                <div style="margin-bottom: 32px;">
                    <h3 style="color: #d4d4d4; font-size: 14px; margin: 0 0 16px 0; text-transform: uppercase; letter-spacing: 1px;">
                        { "Theme" }
                    </h3>
                    <div style="display: flex; gap: 16px;">
                        { self.render_theme_option(ctx, "dark", "Dark", "#1a1a2e", "#ffffff") }
                        { self.render_theme_option(ctx, "light", "Light", "#f5f5f5", "#1a1a1a") }
                        { self.render_theme_option(ctx, "midnight", "Midnight", "#0d1117", "#58a6ff") }
                    </div>
                </div>
                
                // Accent color
                <div style="margin-bottom: 32px;">
                    <h3 style="color: #d4d4d4; font-size: 14px; margin: 0 0 16px 0; text-transform: uppercase; letter-spacing: 1px;">
                        { "Accent Color" }
                    </h3>
                    <div style="display: flex; gap: 12px; flex-wrap: wrap;">
                        { self.render_color_option(ctx, "#4a9eff", "Blue") }
                        { self.render_color_option(ctx, "#e94560", "Red") }
                        { self.render_color_option(ctx, "#28ca41", "Green") }
                        { self.render_color_option(ctx, "#ffbd2e", "Yellow") }
                        { self.render_color_option(ctx, "#a855f7", "Purple") }
                        { self.render_color_option(ctx, "#f97316", "Orange") }
                        { self.render_color_option(ctx, "#ec4899", "Pink") }
                        { self.render_color_option(ctx, "#14b8a6", "Teal") }
                    </div>
                </div>
                
                // Window effects
                <div>
                    <h3 style="color: #d4d4d4; font-size: 14px; margin: 0 0 16px 0; text-transform: uppercase; letter-spacing: 1px;">
                        { "Window Effects" }
                    </h3>
                    <div style="background-color: #1e1e1e; border-radius: 8px; overflow: hidden;">
                        { self.render_toggle_option("Transparency effects", true) }
                        { self.render_toggle_option("Window animations", true) }
                        { self.render_toggle_option("Blur effects", true) }
                    </div>
                </div>
            </div>
        }
    }

    fn render_theme_option(&self, ctx: &Context<Self>, value: &str, label: &str, bg: &str, fg: &str) -> Html {
        let is_selected = self.selected_theme == value;
        let value_str = value.to_string();
        
        html! {
            <div 
                style={format!(
                    "cursor: pointer; border-radius: 12px; overflow: hidden; \
                     border: 3px solid {}; transition: all 0.2s ease;",
                    if is_selected { "#4a9eff" } else { "transparent" }
                )}
                onclick={ctx.link().callback(move |_| SettingsMsg::SetTheme(value_str.clone()))}
            >
                <div style={format!("width: 120px; height: 80px; background-color: {}; display: flex; align-items: center; justify-content: center;", bg)}>
                    <div style={format!("color: {}; font-size: 12px;", fg)}>
                        { "Aa" }
                    </div>
                </div>
                <div style="padding: 8px; background-color: #2d2d2d; text-align: center;">
                    <span style="color: #d4d4d4; font-size: 13px;">{ label }</span>
                </div>
            </div>
        }
    }

    fn render_color_option(&self, ctx: &Context<Self>, color: &str, name: &str) -> Html {
        let is_selected = self.accent_color == color;
        let color_str = color.to_string();
        let name_str = name.to_string();

        html! {
            <div
                style={format!(
                    "width: 40px; height: 40px; border-radius: 50%; cursor: pointer; \
                     background-color: {}; border: 3px solid {}; \
                     transition: transform 0.2s ease; box-shadow: 0 2px 8px rgba(0,0,0,0.3);",
                    color,
                    if is_selected { "#ffffff" } else { "transparent" }
                )}
                onclick={ctx.link().callback(move |_| SettingsMsg::SetAccentColor(color_str.clone()))}
                title={name_str}
            />
        }
    }

    fn render_toggle_option(&self, label: &str, enabled: bool) -> Html {
        html! {
            <div style="display: flex; align-items: center; justify-content: space-between; padding: 16px; border-bottom: 1px solid #333;">
                <span style="color: #d4d4d4; font-size: 14px;">{ label }</span>
                <div style={format!(
                    "width: 44px; height: 24px; border-radius: 12px; cursor: pointer; \
                     background-color: {}; position: relative; transition: background-color 0.2s ease;",
                    if enabled { "#4a9eff" } else { "#555" }
                )}>
                    <div style={format!(
                        "width: 20px; height: 20px; border-radius: 50%; background-color: white; \
                         position: absolute; top: 2px; transition: left 0.2s ease; \
                         left: {}px; box-shadow: 0 2px 4px rgba(0,0,0,0.3);",
                        if enabled { 22 } else { 2 }
                    )} />
                </div>
            </div>
        }
    }

    fn render_wallpaper_tab(&self, ctx: &Context<Self>) -> Html {
        let wallpapers = vec![
            ("gradient1", "Ocean", "linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%)"),
            ("gradient2", "Sunset", "linear-gradient(135deg, #f093fb 0%, #f5576c 50%, #4facfe 100%)"),
            ("gradient3", "Forest", "linear-gradient(135deg, #134e5e 0%, #71b280 100%)"),
            ("gradient4", "Night", "linear-gradient(135deg, #0f0c29 0%, #302b63 50%, #24243e 100%)"),
            ("gradient5", "Aurora", "linear-gradient(135deg, #00c6ff 0%, #0072ff 50%, #7c3aed 100%)"),
            ("gradient6", "Ember", "linear-gradient(135deg, #ff416c 0%, #ff4b2b 100%)"),
            ("solid1", "Dark", "#1a1a1a"),
            ("solid2", "Navy", "#1e3a5f"),
        ];

        html! {
            <div>
                <h2 style="color: white; font-size: 24px; margin: 0 0 24px 0; font-weight: 400;">{ "Wallpaper" }</h2>
                
                <div style="display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 16px;">
                    {
                        wallpapers.iter().map(|(id, name, style)| {
                            let is_selected = &self.selected_wallpaper == *id;
                            let id_str = id.to_string();
                            
                            html! {
                                <div 
                                    style={format!(
                                        "cursor: pointer; border-radius: 12px; overflow: hidden; \
                                         border: 3px solid {}; transition: all 0.2s ease;",
                                        if is_selected { "#4a9eff" } else { "transparent" }
                                    )}
                                    onclick={ctx.link().callback(move |_| SettingsMsg::SetWallpaper(id_str.clone()))}
                                >
                                    <div style={format!(
                                        "height: 120px; background: {};",
                                        style
                                    )} />
                                    <div style="padding: 12px; background-color: #2d2d2d;">
                                        <span style="color: #d4d4d4; font-size: 14px;">{ *name }</span>
                                    </div>
                                </div>
                            }
                        }).collect::<Html>()
                    }
                </div>
            </div>
        }
    }

    fn render_about_tab(&self) -> Html {
        html! {
            <div style="text-align: center; padding: 48px 24px;">
                <div style="font-size: 64px; margin-bottom: 24px;">{ "🖥️" }</div>
                <h1 style="color: white; font-size: 32px; margin: 0 0 8px 0; font-weight: 300;">
                    { "KernelOS" }
                </h1>
                <p style="color: #888; font-size: 16px; margin: 0 0 32px 0;">
                    { "Version 2.0.0" }
                </p>
                
                <div style="background-color: #1e1e1e; border-radius: 12px; padding: 24px; max-width: 400px; margin: 0 auto; text-align: left;">
                    <div style="display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #333;">
                        <span style="color: #888;">{ "Built with" }</span>
                        <span style="color: #d4d4d4;">{ "Rust + Yew + WASM" }</span>
                    </div>
                    <div style="display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #333;">
                        <span style="color: #888;">{ "Platform" }</span>
                        <span style="color: #d4d4d4;">{ "WebAssembly" }</span>
                    </div>
                    <div style="display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #333;">
                        <span style="color: #888;">{ "Storage" }</span>
                        <span style="color: #d4d4d4;">{ "LocalStorage VFS" }</span>
                    </div>
                    <div style="display: flex; justify-content: space-between; padding: 8px 0;">
                        <span style="color: #888;">{ "License" }</span>
                        <span style="color: #d4d4d4;">{ "MIT" }</span>
                    </div>
                </div>
                
                <p style="color: #666; font-size: 12px; margin-top: 32px;">
                    { "A demonstration of modern web technologies." }
                </p>
            </div>
        }
    }
}
