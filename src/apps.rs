//! The application registry.
//!
//! Every app is declared once, here. The start menu, desktop icons, taskbar
//! quick launch, window sizing and window rendering all read from this list, so
//! adding an app means adding one entry plus its component.

use yew::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;

use crate::filesystem::FileSystem;
use crate::components::terminal::Terminal;
use crate::components::file_explorer::FileExplorer;
use crate::components::text_editor::TextEditor;
use crate::components::clock::Clock;
use crate::components::calculator::Calculator;
use crate::components::settings::Settings;
use crate::components::paint::Paint;
use crate::components::minesweeper::Minesweeper;
use crate::components::browser::Browser;
use crate::components::agent::Agent;

/// Everything an app can reach for when rendering. Passing this as one value
/// keeps app signatures uniform, which is what lets `render` be a plain fn
/// pointer rather than a boxed closure per app.
#[derive(Clone, PartialEq)]
pub struct AppContext {
    pub fs: Rc<RefCell<FileSystem>>,
    /// Launch argument — the file a text editor should open, the URL a browser
    /// should load. `None` for a plain launch.
    pub arg: Option<String>,
    pub on_open_file: Callback<(String, String)>,
    pub on_notification: Callback<(String, String, String)>,
    pub on_theme_change: Callback<String>,
    pub on_wallpaper_change: Callback<String>,
    pub on_accent_change: Callback<String>,
}

pub struct AppDefinition {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub category: &'static str,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    /// Whether the app gets a desktop icon.
    pub on_desktop: bool,
    /// Whether the app gets a taskbar quick-launch button.
    pub in_quick_launch: bool,
    pub render: fn(&AppContext) -> Html,
}

pub const APPS: &[AppDefinition] = &[
    AppDefinition {
        id: "file-explorer",
        title: "File Explorer",
        icon: "📁",
        category: "System",
        width: 650, height: 450, min_width: 300, min_height: 200,
        on_desktop: true, in_quick_launch: true,
        render: render_file_explorer,
    },
    AppDefinition {
        id: "terminal",
        title: "Terminal",
        icon: "💻",
        category: "System",
        width: 650, height: 450, min_width: 300, min_height: 200,
        on_desktop: true, in_quick_launch: true,
        render: render_terminal,
    },
    AppDefinition {
        id: "browser",
        title: "Browser",
        icon: "🌐",
        category: "Internet",
        width: 900, height: 620, min_width: 400, min_height: 300,
        on_desktop: true, in_quick_launch: true,
        render: render_browser,
    },
    AppDefinition {
        id: "text-editor",
        title: "Text Editor",
        icon: "📝",
        category: "Productivity",
        width: 650, height: 450, min_width: 300, min_height: 200,
        on_desktop: true, in_quick_launch: true,
        render: render_text_editor,
    },
    AppDefinition {
        id: "calculator",
        title: "Calculator",
        icon: "🔢",
        category: "Utilities",
        width: 320, height: 480, min_width: 280, min_height: 400,
        on_desktop: true, in_quick_launch: true,
        render: render_calculator,
    },
    AppDefinition {
        id: "clock",
        title: "Clock",
        icon: "🕐",
        category: "Utilities",
        width: 300, height: 200, min_width: 200, min_height: 150,
        on_desktop: false, in_quick_launch: false,
        render: render_clock,
    },
    AppDefinition {
        id: "paint",
        title: "Paint",
        icon: "🎨",
        category: "Creative",
        width: 800, height: 600, min_width: 400, min_height: 300,
        on_desktop: true, in_quick_launch: false,
        render: render_paint,
    },
    AppDefinition {
        id: "minesweeper",
        title: "Minesweeper",
        icon: "💣",
        category: "Games",
        width: 400, height: 500, min_width: 300, min_height: 400,
        on_desktop: true, in_quick_launch: false,
        render: render_minesweeper,
    },
    AppDefinition {
        id: "agent",
        title: "AI Agent",
        icon: "🤖",
        category: "Productivity",
        width: 700, height: 550, min_width: 400, min_height: 350,
        on_desktop: true, in_quick_launch: false,
        render: render_agent,
    },
    AppDefinition {
        id: "settings",
        title: "Settings",
        icon: "⚙️",
        category: "System",
        width: 700, height: 500, min_width: 500, min_height: 400,
        on_desktop: true, in_quick_launch: false,
        render: render_settings,
    },
    AppDefinition {
        id: "about",
        title: "About KernelOS",
        icon: "ℹ️",
        category: "System",
        width: 420, height: 340, min_width: 300, min_height: 260,
        on_desktop: false, in_quick_launch: false,
        render: render_about,
    },
];

pub fn find(id: &str) -> Option<&'static AppDefinition> {
    APPS.iter().find(|app| app.id == id)
}

/// Lightweight view of an app for menus, icons and the taskbar. Owned because
/// plugin apps are runtime-registered, not `'static`.
pub struct AppInfo {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub category: String,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub on_desktop: bool,
    pub in_quick_launch: bool,
}

pub fn builtin_apps() -> Vec<AppInfo> {
    APPS.iter()
        .map(|a| AppInfo {
            id: a.id.to_string(),
            title: a.title.to_string(),
            icon: a.icon.to_string(),
            category: a.category.to_string(),
            width: a.width,
            height: a.height,
            min_width: a.min_width,
            min_height: a.min_height,
            on_desktop: a.on_desktop,
            in_quick_launch: a.in_quick_launch,
        })
        .collect()
}

/// Everything launchable — builtins plus runtime-registered plugins. This is
/// what the start menu, desktop icons and taskbar quick launch read from, so a
/// new plugin shows up everywhere with no further changes.
pub fn all_apps() -> Vec<AppInfo> {
    let mut apps = builtin_apps();
    for p in crate::plugin::apps() {
        apps.push(AppInfo {
            id: p.id,
            title: p.name,
            icon: p.icon,
            category: p.category,
            width: p.width,
            height: p.height,
            min_width: p.min_width,
            min_height: p.min_height,
            on_desktop: p.on_desktop,
            in_quick_launch: p.in_quick_launch,
        });
    }
    apps
}

/// Window geometry for any app id — builtin or plugin. Plugin geometry comes
/// from its manifest, so there is no second place to keep in sync.
pub fn geometry_for(id: &str) -> Option<(i32, i32, i32, i32)> {
    if let Some(app) = find(id) {
        return Some((app.width, app.height, app.min_width, app.min_height));
    }
    crate::plugin::manifest(id)
        .map(|m| (m.width, m.height, m.min_width, m.min_height))
}

/// Icon for an app id — builtin or plugin — falling back to a generic glyph.
pub fn icon_for(id: &str) -> String {
    find(id)
        .map(|app| app.icon.to_string())
        .or_else(|| crate::plugin::manifest(id).map(|m| m.icon))
        .unwrap_or_else(|| "📄".to_string())
}

fn render_file_explorer(cx: &AppContext) -> Html {
    html! { <FileExplorer fs={cx.fs.clone()} on_open_file={cx.on_open_file.clone()} /> }
}

fn render_terminal(cx: &AppContext) -> Html {
    html! { <Terminal fs={cx.fs.clone()} on_notification={cx.on_notification.clone()} /> }
}

fn render_browser(cx: &AppContext) -> Html {
    html! { <Browser fs={cx.fs.clone()} initial_url={cx.arg.clone()} /> }
}

fn render_text_editor(cx: &AppContext) -> Html {
    html! {
        <TextEditor
            fs={cx.fs.clone()}
            file_path={cx.arg.clone()}
            on_notification={cx.on_notification.clone()}
        />
    }
}

fn render_calculator(_cx: &AppContext) -> Html {
    html! { <Calculator /> }
}

fn render_clock(_cx: &AppContext) -> Html {
    html! { <Clock /> }
}

fn render_paint(_cx: &AppContext) -> Html {
    html! { <Paint /> }
}

fn render_minesweeper(_cx: &AppContext) -> Html {
    html! { <Minesweeper /> }
}

fn render_agent(cx: &AppContext) -> Html {
    html! { <Agent fs={cx.fs.clone()} /> }
}

fn render_settings(cx: &AppContext) -> Html {
    html! {
        <Settings
            on_theme_change={cx.on_theme_change.clone()}
            on_wallpaper_change={cx.on_wallpaper_change.clone()}
            on_accent_change={cx.on_accent_change.clone()}
        />
    }
}

fn render_about(_cx: &AppContext) -> Html {
    html! {
        <div class="about-app">
            <div class="about-logo">{ "🖥️" }</div>
            <h1 class="about-title">{ "KernelOS" }</h1>
            <p class="about-version">{ "Version 2.0" }</p>
            <p class="about-blurb">
                { "A WebAssembly-based desktop environment built with Rust and Yew." }
            </p>
            <p class="about-copyright">{ "© 2025 KernelOS Project" }</p>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_are_unique() {
        let mut ids: Vec<&str> = APPS.iter().map(|a| a.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate app id in the registry");
    }

    #[test]
    fn every_app_has_workable_dimensions() {
        for app in APPS {
            assert!(app.min_width <= app.width, "{} min_width exceeds width", app.id);
            assert!(app.min_height <= app.height, "{} min_height exceeds height", app.id);
            assert!(!app.title.is_empty(), "{} has no title", app.id);
            assert!(!app.icon.is_empty(), "{} has no icon", app.id);
        }
    }

    #[test]
    fn lookup_finds_registered_apps_only() {
        assert!(find("terminal").is_some());
        assert!(find("browser").is_some());
        assert!(find("nonexistent").is_none());
        assert_eq!(icon_for("terminal"), "💻");
        assert_eq!(icon_for("nonexistent"), "📄");
    }
}
