use yew::prelude::*;

use crate::apps::{self, AppDefinition};

#[derive(Properties, Clone, PartialEq)]
pub struct StartMenuProps {
    pub visible: bool,
    pub on_close: Callback<()>,
    pub on_launch_app: Callback<String>,
}

pub struct StartMenu {
    search_query: String,
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

        let query = self.search_query.to_lowercase();
        let filtered_apps: Vec<apps::AppInfo> = apps::all_apps()
            .into_iter()
            .filter(|app| {
                query.is_empty()
                    || app.title.to_lowercase().contains(&query)
                    || app.category.to_lowercase().contains(&query)
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
                    class="start-menu-backdrop"
                    onclick={on_overlay_click}
                />
                <div class="start-menu">
                    <div class="start-menu-header">
                        <input 
                            type="text"
                            placeholder="Search apps..."
                            value={self.search_query.clone()}
                            oninput={on_search}
                            class="start-menu-search"
                        />
                    </div>
                    <div class="start-menu-apps">
                        {
                            filtered_apps.iter().map(|app| {
                                let app_id = app.id.clone();
                                let on_click = ctx.link().callback(move |_| {
                                    // callback is Fn and may run many times, so
                                    // clone out of the capture each invocation.
                                    StartMenuMsg::LaunchApp(app_id.clone())
                                });
                                
                                html! {
                                    <div class="start-menu-app" onclick={on_click}>
                                        <span class="start-menu-app-icon">{ &app.icon }</span>
                                        <div>
                                            <div class="start-menu-app-name">{ &app.title }</div>
                                            <div class="start-menu-app-category">{ &app.category }</div>
                                        </div>
                                    </div>
                                }
                            }).collect::<Html>()
                        }
                        {
                            if filtered_apps.is_empty() {
                                html! {
                                    <div class="start-menu-empty">
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
