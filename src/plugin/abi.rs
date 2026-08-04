//! Plugin ABI types — must stay in sync with `kernelos-pdk`.

use serde::{Deserialize, Serialize};

pub const ABI_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum UiOp {
    BeginVBox { gap: u8 },
    BeginHBox { gap: u8 },
    End,
    Label {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
    },
    Button { id: u32, text: String },
    Input {
        id: u32,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Checkbox {
        id: u32,
        checked: bool,
        label: String,
    },
    List {
        items: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Init,
    Click { id: u32 },
    InputChanged { id: u32, value: String },
    Tick { millis: u64 },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "cap", rename_all = "snake_case")]
pub enum Capability {
    VfsRead { prefix: String },
    VfsWrite { prefix: String },
    Notify,
    Clipboard,
    Persist,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub icon: String,
    #[serde(default = "default_abi_version")]
    pub abi_version: u32,
    #[serde(default)]
    pub requests: Vec<Capability>,
    /// Optional SHA-256 hex hash of wasm bytes for install verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_hash: Option<String>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_true")]
    pub on_desktop: bool,
    #[serde(default)]
    pub in_quick_launch: bool,
    #[serde(default = "default_width")]
    pub width: i32,
    #[serde(default = "default_height")]
    pub height: i32,
    #[serde(default = "default_min_width")]
    pub min_width: i32,
    #[serde(default = "default_min_height")]
    pub min_height: i32,
}

fn default_abi_version() -> u32 {
    ABI_VERSION
}
fn default_category() -> String {
    "Plugins".into()
}
fn default_true() -> bool {
    true
}
fn default_width() -> i32 {
    400
}
fn default_height() -> i32 {
    300
}
fn default_min_width() -> i32 {
    200
}
fn default_min_height() -> i32 {
    150
}

/// Approved capability subset for a plugin instance.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Grant(pub Vec<Capability>);

impl Grant {
    pub fn has(&self, cap: &Capability) -> bool {
        self.0.iter().any(|c| c == cap)
    }

    pub fn vfs_read_prefix(&self) -> Option<&str> {
        self.0.iter().find_map(|c| match c {
            Capability::VfsRead { prefix } => Some(prefix.as_str()),
            _ => None,
        })
    }

    pub fn vfs_write_prefix(&self) -> Option<&str> {
        self.0.iter().find_map(|c| match c {
            Capability::VfsWrite { prefix } => Some(prefix.as_str()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PermissionsStore {
    #[serde(default)]
    pub grants: std::collections::HashMap<String, Grant>,
}

pub fn encode_event(event: &Event) -> Result<Vec<u8>, String> {
    serde_json::to_vec(event).map_err(|e| e.to_string())
}

pub fn decode_ui_ops(bytes: &[u8]) -> Result<Vec<UiOp>, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_round_trip() {
        let ev = Event::Click { id: 42 };
        let bytes = encode_event(&ev).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn ui_ops_round_trip() {
        let ops = vec![
            UiOp::BeginVBox { gap: 8 },
            UiOp::Label {
                text: "hi".into(),
                class: None,
            },
            UiOp::End,
        ];
        let bytes = serde_json::to_vec(&ops).unwrap();
        let back = decode_ui_ops(&bytes).unwrap();
        assert_eq!(back, ops);
    }
}
