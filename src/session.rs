//! Persisted desktop state.
//!
//! Both files live in the virtual filesystem rather than in raw local storage,
//! so they are inspectable from inside the OS — `cat /system/config/theme.json`
//! works, and editing it in the text editor is a legitimate way to change
//! settings.

use serde::{Serialize, Deserialize};

use crate::filesystem::FileSystem;

pub const THEME_CONFIG_PATH: &str = "/system/config/theme.json";
pub const SESSION_PATH: &str = "/system/config/session.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub theme: String,
    pub accent: String,
    pub wallpaper: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            accent: "#4a9eff".to_string(),
            wallpaper: "gradient1".to_string(),
        }
    }
}

impl ThemeConfig {
    pub fn load(fs: &FileSystem) -> Self {
        fs.read_file(THEME_CONFIG_PATH)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, fs: &mut FileSystem) {
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = fs.write_file(THEME_CONFIG_PATH, &raw);
        }
    }
}

/// One restored window. Focus and stacking are not persisted — the Desktop
/// reassigns those on restore so the invariants stay in one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionWindow {
    pub app_id: String,
    pub arg: Option<String>,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_maximized: bool,
    pub is_minimized: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub windows: Vec<SessionWindow>,
}

impl Session {
    pub fn load(fs: &FileSystem) -> Self {
        fs.read_file(SESSION_PATH)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, fs: &mut FileSystem) {
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = fs.write_file(SESSION_PATH, &raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_config_round_trips() {
        let config = ThemeConfig {
            theme: "light".to_string(),
            accent: "#d13438".to_string(),
            wallpaper: "gradient3".to_string(),
        };

        let raw = serde_json::to_string(&config).unwrap();
        let parsed: ThemeConfig = serde_json::from_str(&raw).unwrap();

        assert_eq!(parsed, config);
    }

    #[test]
    fn theme_config_falls_back_when_unparseable() {
        assert!(serde_json::from_str::<ThemeConfig>("not json").is_err());
        assert_eq!(ThemeConfig::default().theme, "dark");
        assert_eq!(ThemeConfig::default().wallpaper, "gradient1");
    }

    #[test]
    fn session_round_trips() {
        let session = Session {
            windows: vec![SessionWindow {
                app_id: "terminal".to_string(),
                arg: None,
                title: "Terminal".to_string(),
                x: 120,
                y: 80,
                width: 650,
                height: 450,
                is_maximized: false,
                is_minimized: false,
            }],
        };

        let raw = serde_json::to_string(&session).unwrap();
        let parsed: Session = serde_json::from_str(&raw).unwrap();

        assert_eq!(parsed, session);
    }

    #[test]
    fn an_empty_session_is_the_default() {
        assert_eq!(Session::default().windows.len(), 0);
    }
}
