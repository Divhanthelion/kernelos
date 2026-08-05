use yew::prelude::*;

#[derive(Clone, PartialEq)]
pub enum SettingsTab {
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
    #[prop_or_default]
    pub on_accent_change: Callback<String>,
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
                self.accent_color = color.clone();
                ctx.props().on_accent_change.emit(color);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="settings">
                // Sidebar
                <div class="settings-sidebar">
                    <div class="settings-sidebar-title">
                        { "⚙️ Settings" }
                    </div>
                    { self.render_nav_item(ctx, "🎨", "Appearance", SettingsTab::Appearance) }
                    { self.render_nav_item(ctx, "🖼️", "Wallpaper", SettingsTab::Wallpaper) }
                    { self.render_nav_item(ctx, "ℹ️", "About", SettingsTab::About) }
                </div>
                
                // Content
                <div class="settings-content">
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
                class={classes!("settings-nav-item", is_active.then_some("active"))}
                onclick={ctx.link().callback(move |_| SettingsMsg::SetTab(tab_clone.clone()))}
            >
                <span class="settings-nav-icon">{ icon }</span>
                <span>{ label }</span>
            </div>
        }
    }

    fn render_appearance_tab(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <h2 class="settings-heading">{ "Appearance" }</h2>
                
                // Theme selection
                <div class="settings-section">
                    <h3 class="settings-section-title">{ "Theme" }</h3>
                    <div class="settings-swatch-row">
                        { self.render_theme_option(ctx, "dark", "Dark", "#1a1a2e", "#ffffff") }
                        { self.render_theme_option(ctx, "light", "Light", "#f5f5f5", "#1a1a1a") }
                        { self.render_theme_option(ctx, "midnight", "Midnight", "#0d1117", "#58a6ff") }
                    </div>
                </div>
                
                // Accent color
                <div class="settings-section">
                    <h3 class="settings-section-title">{ "Accent Color" }</h3>
                    <div class="settings-color-row">
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
                    <h3 class="settings-section-title">{ "Window Effects" }</h3>
                    <div class="settings-toggle-group">
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
                class={classes!("settings-swatch", is_selected.then_some("selected"))}
                onclick={ctx.link().callback(move |_| SettingsMsg::SetTheme(value_str.clone()))}
            >
                <div class="settings-swatch-preview" style={format!("background-color: {};", bg)}>
                    <div style={format!("color: {};", fg)}>{ "Aa" }</div>
                </div>
                <div class="settings-swatch-label">
                    <span>{ label }</span>
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
                class={classes!("settings-color", is_selected.then_some("selected"))}
                style={format!("background-color: {};", color)}
                onclick={ctx.link().callback(move |_| SettingsMsg::SetAccentColor(color_str.clone()))}
                title={name_str}
            />
        }
    }

    fn render_toggle_option(&self, label: &str, enabled: bool) -> Html {
        html! {
            <div class="settings-option">
                <span class="settings-option-label">{ label }</span>
                <div class={classes!("settings-toggle", enabled.then_some("on"))}>
                    <div class="settings-toggle-knob" />
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
                <h2 class="settings-heading">{ "Wallpaper" }</h2>
                
                <div class="settings-wallpaper-grid">
                    {
                        wallpapers.iter().map(|(id, name, style)| {
                            let is_selected = &self.selected_wallpaper == *id;
                            let id_str = id.to_string();
                            
                            html! {
                                <div
                                    class={classes!("settings-swatch", is_selected.then_some("selected"))}
                                    onclick={ctx.link().callback(move |_| SettingsMsg::SetWallpaper(id_str.clone()))}
                                >
                                    <div class="settings-wallpaper-preview" style={format!("background: {};", style)} />
                                    <div class="settings-swatch-label">
                                        <span>{ *name }</span>
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
            <div class="settings-about">
                <div class="settings-about-logo">{ "🖥️" }</div>
                <h1 class="settings-about-title">{ "KernelOS" }</h1>
                <p class="settings-about-version">{ "Version 2.0.0" }</p>
                
                <div class="settings-about-table">
                    <div class="settings-about-row">
                        <span class="settings-about-key">{ "Built with" }</span>
                        <span class="settings-about-value">{ "Rust + Yew + WASM" }</span>
                    </div>
                    <div class="settings-about-row">
                        <span class="settings-about-key">{ "Platform" }</span>
                        <span class="settings-about-value">{ "WebAssembly" }</span>
                    </div>
                    <div class="settings-about-row">
                        <span class="settings-about-key">{ "Storage" }</span>
                        <span class="settings-about-value">{ "LocalStorage VFS" }</span>
                    </div>
                    <div class="settings-about-row last">
                        <span class="settings-about-key">{ "License" }</span>
                        <span class="settings-about-value">{ "MIT" }</span>
                    </div>
                </div>
                
                <p class="settings-about-footer">
                    { "A demonstration of modern web technologies." }
                </p>
            </div>
        }
    }
}
