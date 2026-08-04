use yew::prelude::*;
use gloo_timers::callback::Timeout;
use std::collections::VecDeque;

#[derive(Clone, PartialEq, Debug)]
pub enum NotificationType {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Notification {
    pub id: u32,
    pub title: String,
    pub message: String,
    pub notification_type: NotificationType,
}

impl Notification {
    pub fn new(id: u32, title: String, message: String, notification_type: NotificationType) -> Self {
        Self {
            id,
            title,
            message,
            notification_type,
        }
    }
}

#[derive(Clone, PartialEq, Default)]
pub struct NotificationManager {
    notifications: VecDeque<Notification>,
    next_id: u32,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: VecDeque::new(),
            next_id: 0,
        }
    }

    pub fn add(&mut self, title: String, message: String, notification_type: NotificationType) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        
        let notification = Notification::new(id, title, message, notification_type);
        self.notifications.push_back(notification);
        
        // Keep only last 5 notifications
        while self.notifications.len() > 5 {
            self.notifications.pop_front();
        }
        
        id
    }

    pub fn remove(&mut self, id: u32) {
        self.notifications.retain(|n| n.id != id);
    }

    pub fn get_all(&self) -> Vec<Notification> {
        self.notifications.iter().cloned().collect()
    }
}

#[derive(Properties, Clone, PartialEq)]
pub struct NotificationContainerProps {
    pub notifications: Vec<Notification>,
    pub on_dismiss: Callback<u32>,
}

pub struct NotificationContainer;

impl Component for NotificationContainer {
    type Message = ();
    type Properties = NotificationContainerProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="notification-container">
                {
                    ctx.props().notifications.iter().map(|notification| {
                        let id = notification.id;
                        let on_dismiss = ctx.props().on_dismiss.clone();
                        
                        let border_color = match notification.notification_type {
                            NotificationType::Info => "#4a9eff",
                            NotificationType::Success => "#28ca41",
                            NotificationType::Warning => "#ffbd2e",
                            NotificationType::Error => "#ff5f57",
                        };
                        
                        let icon = match notification.notification_type {
                            NotificationType::Info => "ℹ️",
                            NotificationType::Success => "✅",
                            NotificationType::Warning => "⚠️",
                            NotificationType::Error => "❌",
                        };
                        
                        html! {
                            <NotificationItem 
                                notification={notification.clone()}
                                border_color={border_color.to_string()}
                                icon={icon.to_string()}
                                on_dismiss={Callback::from(move |_| on_dismiss.emit(id))}
                            />
                        }
                    }).collect::<Html>()
                }
            </div>
        }
    }
}

#[derive(Properties, Clone, PartialEq)]
struct NotificationItemProps {
    notification: Notification,
    border_color: String,
    icon: String,
    on_dismiss: Callback<()>,
}

struct NotificationItem {
    _timeout: Option<Timeout>,
}

pub enum NotificationItemMsg {
    Dismiss,
}

impl Component for NotificationItem {
    type Message = NotificationItemMsg;
    type Properties = NotificationItemProps;

    fn create(ctx: &Context<Self>) -> Self {
        // Auto-dismiss after 5 seconds
        let link = ctx.link().clone();
        let timeout = Timeout::new(5000, move || {
            link.send_message(NotificationItemMsg::Dismiss);
        });
        
        Self {
            _timeout: Some(timeout),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            NotificationItemMsg::Dismiss => {
                ctx.props().on_dismiss.emit(());
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let notification = &ctx.props().notification;
        let on_dismiss = ctx.link().callback(|_| NotificationItemMsg::Dismiss);
        
        // Only the accent stripe is dynamic; the rest is themed by CSS.
        let accent = format!("border-left-color: {};", ctx.props().border_color);

        html! {
            <div class="notification" style={accent}>
                <button class="notification-close" onclick={on_dismiss}>{ "×" }</button>
                <div class="notification-body">
                    <span class="notification-icon">{ &ctx.props().icon }</span>
                    <div>
                        <div class="notification-title">{ &notification.title }</div>
                        <div class="notification-message">{ &notification.message }</div>
                    </div>
                </div>
            </div>
        }
    }
}
