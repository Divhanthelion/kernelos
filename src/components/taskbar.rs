use yew::prelude::*;
use gloo_timers::callback::Interval;

use crate::apps;

#[derive(Clone, PartialEq)]
pub struct TaskbarWindow {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub is_minimized: bool,
    pub is_focused: bool,
}

#[derive(Properties, Clone, PartialEq)]
pub struct TaskbarProps {
    pub windows: Vec<TaskbarWindow>,
    pub on_window_click: Callback<String>,
    pub on_start_click: Callback<()>,
    pub on_quick_launch: Callback<String>,
}

pub struct Taskbar {
    current_time: String,
    current_date: String,
    _interval: Option<Interval>,
}

pub enum TaskbarMsg {
    UpdateTime,
}

impl Component for Taskbar {
    type Message = TaskbarMsg;
    type Properties = TaskbarProps;

    fn create(ctx: &Context<Self>) -> Self {
        let link = ctx.link().clone();
        let interval = Interval::new(1000, move || {
            link.send_message(TaskbarMsg::UpdateTime);
        });

        let mut taskbar = Self {
            current_time: String::new(),
            current_date: String::new(),
            _interval: Some(interval),
        };
        taskbar.update_time();
        taskbar
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            TaskbarMsg::UpdateTime => {
                self.update_time();
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_start_click = ctx.props().on_start_click.clone();

        html! {
            <div class="taskbar">
                // Start Button
                <button
                    class="start-button"
                    onclick={Callback::from(move |e: MouseEvent| {
                        // Without this the click reaches the desktop, whose
                        // handler closes the menu we just opened.
                        e.stop_propagation();
                        on_start_click.emit(())
                    })}
                    title="Start Menu"
                >
                    { "⊞" }
                </button>

                // Quick Launch
                <div class="quick-launch">
                    {
                        apps::all_apps().iter()
                            .filter(|app| app.in_quick_launch)
                            .map(|app| self.render_quick_launch_button(ctx, &app.id, &app.icon, &app.title))
                            .collect::<Html>()
                    }
                </div>

                // Window Buttons
                <div class="taskbar-windows">
                    {
                        ctx.props().windows.iter().map(|window| {
                            let window_id = window.id.clone();
                            let on_click = ctx.props().on_window_click.clone();
                            
                            html! {
                                <button
                                    class={classes!(
                                        "taskbar-window-button",
                                        window.is_focused.then_some("active"),
                                        window.is_minimized.then_some("minimized"),
                                    )}
                                    onclick={Callback::from(move |e: MouseEvent| {
                                        e.stop_propagation();
                                        on_click.emit(window_id.clone())
                                    })}
                                    title={window.title.clone()}
                                >
                                    <span>{ &window.icon }</span>
                                    <span class="taskbar-window-title">{ &window.title }</span>
                                </button>
                            }
                        }).collect::<Html>()
                    }
                </div>

                // System Tray
                <div class="system-tray">
                    <div class="system-tray-clock">
                        <span class="system-tray-time">
                            { &self.current_time }
                        </span>
                        <span class="system-tray-date">
                            { &self.current_date }
                        </span>
                    </div>
                </div>
            </div>
        }
    }
}

impl Taskbar {
    fn update_time(&mut self) {
        let date = js_sys::Date::new_0();
        
        let hours = date.get_hours();
        let minutes = date.get_minutes();
        let period = if hours >= 12 { "PM" } else { "AM" };
        let display_hours = if hours % 12 == 0 { 12 } else { hours % 12 };
        
        self.current_time = format!("{:02}:{:02} {}", display_hours, minutes, period);
        
        let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        
        let day = days[date.get_day() as usize];
        let month = months[date.get_month() as usize];
        let date_num = date.get_date();
        
        self.current_date = format!("{}, {} {}", day, month, date_num);
    }

    fn render_quick_launch_button(&self, ctx: &Context<Self>, id: &str, icon: &str, title: &str) -> Html {
        let app_id = id.to_string();
        let on_click = ctx.props().on_quick_launch.clone();
        
        html! {
            <button
                class="quick-launch-button"
                onclick={Callback::from(move |e: MouseEvent| {
                    e.stop_propagation();
                    on_click.emit(app_id.clone())
                })}
                title={title.to_string()}
            >
                { icon }
            </button>
        }
    }
}
