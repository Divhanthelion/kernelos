use yew::prelude::*;
use gloo_timers::callback::Interval;

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
            <div class="taskbar" style="position: fixed; bottom: 0; left: 0; right: 0; height: 48px; background-color: rgba(30,30,30,0.95); backdrop-filter: blur(20px); display: flex; align-items: center; padding: 0 8px; z-index: 1000; border-top: 1px solid rgba(255,255,255,0.1);">
                // Start Button
                <button 
                    class="start-button"
                    style="width: 40px; height: 40px; border-radius: 8px; border: none; background: linear-gradient(135deg, #4a9eff, #e94560); color: white; font-size: 18px; cursor: pointer; display: flex; align-items: center; justify-content: center;"
                    onclick={Callback::from(move |_| on_start_click.emit(()))}
                    title="Start Menu"
                >
                    { "⊞" }
                </button>

                // Quick Launch
                <div class="quick-launch" style="display: flex; gap: 4px; margin-left: 12px; padding-left: 12px; border-left: 1px solid rgba(255,255,255,0.1);">
                    { self.render_quick_launch_button(ctx, "file-explorer", "📁", "File Explorer") }
                    { self.render_quick_launch_button(ctx, "terminal", "💻", "Terminal") }
                    { self.render_quick_launch_button(ctx, "text-editor", "📝", "Text Editor") }
                    { self.render_quick_launch_button(ctx, "calculator", "🔢", "Calculator") }
                </div>

                // Window Buttons
                <div class="taskbar-windows" style="display: flex; gap: 4px; margin-left: 12px; flex: 1; overflow-x: auto;">
                    {
                        ctx.props().windows.iter().map(|window| {
                            let window_id = window.id.clone();
                            let on_click = ctx.props().on_window_click.clone();
                            
                            let button_style = format!(
                                "height: 36px; padding: 0 12px; border-radius: 6px; border: none; \
                                 background: {}; color: white; font-size: 12px; cursor: pointer; \
                                 display: flex; align-items: center; gap: 8px; white-space: nowrap; \
                                 max-width: 180px; overflow: hidden; text-overflow: ellipsis; {}",
                                if window.is_focused { 
                                    "rgba(255,255,255,0.2)" 
                                } else if window.is_minimized {
                                    "rgba(255,255,255,0.05)"
                                } else {
                                    "rgba(255,255,255,0.1)"
                                },
                                if window.is_focused {
                                    "border-bottom: 2px solid #4a9eff;"
                                } else {
                                    ""
                                }
                            );
                            
                            html! {
                                <button 
                                    style={button_style}
                                    onclick={Callback::from(move |_| on_click.emit(window_id.clone()))}
                                    title={window.title.clone()}
                                >
                                    <span>{ &window.icon }</span>
                                    <span style="overflow: hidden; text-overflow: ellipsis;">{ &window.title }</span>
                                </button>
                            }
                        }).collect::<Html>()
                    }
                </div>

                // System Tray
                <div class="system-tray" style="display: flex; align-items: center; gap: 12px; margin-left: auto; padding-left: 12px; border-left: 1px solid rgba(255,255,255,0.1);">
                    <div style="display: flex; flex-direction: column; align-items: flex-end; padding: 0 8px;">
                        <span style="color: white; font-size: 12px; font-weight: 500;">
                            { &self.current_time }
                        </span>
                        <span style="color: rgba(255,255,255,0.6); font-size: 10px;">
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
                style="width: 36px; height: 36px; border-radius: 6px; border: none; background: transparent; color: white; font-size: 18px; cursor: pointer; display: flex; align-items: center; justify-content: center;"
                onclick={Callback::from(move |_| on_click.emit(app_id.clone()))}
                title={title.to_string()}
            >
                { icon }
            </button>
        }
    }
}
