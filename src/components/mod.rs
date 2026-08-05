// KernelOS v2 Components

pub mod window;
pub mod desktop;
pub mod taskbar;
pub mod terminal;
pub mod browser;
pub mod file_explorer;
pub mod text_editor;
pub mod clock;
pub mod calculator;
pub mod settings;
pub mod paint;
pub mod minesweeper;
pub mod notification;
pub mod start_menu;
pub mod context_menu;
pub mod agent;

pub use desktop::Desktop;
pub use notification::{Notification, NotificationType, NotificationManager};
