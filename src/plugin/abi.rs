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
    /// Soft cap on guest linear memory, in WASM pages (64 KiB each).
    /// Default 256 → 16 MiB. Existing manifests without the field keep parsing.
    #[serde(default = "default_max_pages")]
    pub max_pages: u32,
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
fn default_max_pages() -> u32 {
    256
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

/// Human-readable capability line for consent prompts (not Debug).
pub fn describe_capability(cap: &Capability) -> String {
    match cap {
        Capability::VfsRead { prefix } => format!("read files under {prefix}"),
        Capability::VfsWrite { prefix } => format!("write files under {prefix}"),
        Capability::Notify => "show notifications".into(),
        Capability::Clipboard => "access the clipboard".into(),
        Capability::Persist => "persist data across reloads".into(),
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

    #[test]
    fn manifest_defaults_max_pages_without_field() {
        // plugins/hello.json has no max_pages — must still parse as 256.
        let raw = r#"{
            "id": "hello",
            "name": "Hello Plugin",
            "icon": "👋"
        }"#;
        let m: PluginManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.max_pages, 256);
    }

    #[test]
    fn describe_capability_includes_vfs_prefix() {
        let cap = Capability::VfsRead {
            prefix: "/home/documents".into(),
        };
        let text = describe_capability(&cap);
        assert!(text.contains("/home/documents"), "got: {text}");
        assert!(!text.contains("VfsRead"), "must not use Debug form: {text}");
    }

    #[test]
    fn permissions_store_round_trips() {
        let mut store = PermissionsStore::default();
        store.grants.insert(
            "hello".into(),
            Grant(vec![Capability::Notify]),
        );
        let json = serde_json::to_string(&store).unwrap();
        let back: PermissionsStore = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.grants.get("hello"),
            Some(&Grant(vec![Capability::Notify]))
        );
    }
}
