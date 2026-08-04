//! Document viewer plugin — exercises persisted state and capability-scoped
//! reads from the KernelOS virtual filesystem.

use kernelos_pdk::{Event, Plugin, UiOp};

const PATH_INPUT: u32 = 1;
const LOAD_BUTTON: u32 = 2;
const SAVE_BUTTON: u32 = 3;
const DEFAULT_PATH: &str = "/home/documents/welcome.txt";
const MAX_PREVIEW_CHARS: usize = 4_000;

struct DocViewer {
    path: String,
    content: String,
    status: String,
}

impl Default for DocViewer {
    fn default() -> Self {
        Self {
            path: DEFAULT_PATH.to_string(),
            content: String::new(),
            status: "Ready.".to_string(),
        }
    }
}

impl DocViewer {
    fn load(&mut self) {
        match kernelos_pdk::vfs_read(&self.path) {
            Some(content) => {
                let total_chars = content.chars().count();
                self.content = content.chars().take(MAX_PREVIEW_CHARS).collect();
                self.status = if total_chars > MAX_PREVIEW_CHARS {
                    format!(
                        "Loaded {total_chars} characters; showing the first {MAX_PREVIEW_CHARS}."
                    )
                } else {
                    format!("Loaded {total_chars} characters.")
                };
            }
            None => {
                self.content.clear();
                self.status = "Read failed: path missing or outside the granted directory.".into();
            }
        }
    }

    fn save_path(&mut self) {
        self.status = if kernelos_pdk::persist_write(&self.path) {
            "Saved as the startup document.".into()
        } else {
            "Could not persist the startup document.".into()
        };
    }
}

impl Plugin for DocViewer {
    fn init(&mut self) {
        if let Some(saved_path) = kernelos_pdk::persist_read() {
            let saved_path = saved_path.trim();
            if !saved_path.is_empty() {
                self.path = saved_path.to_string();
            }
        }
        self.load();
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Init => true,
            Event::InputChanged {
                id: PATH_INPUT,
                value,
            } => {
                self.path = value;
                true
            }
            Event::Click { id: LOAD_BUTTON } => {
                self.load();
                true
            }
            Event::Click { id: SAVE_BUTTON } => {
                self.save_path();
                true
            }
            _ => false,
        }
    }

    fn render(&self) -> Vec<UiOp> {
        vec![
            UiOp::BeginVBox { gap: 10 },
            UiOp::Label {
                text: "Document Viewer".into(),
                class: Some("plugin-title".into()),
            },
            UiOp::Input {
                id: PATH_INPUT,
                value: self.path.clone(),
                placeholder: Some("/home/documents/welcome.txt".into()),
            },
            UiOp::BeginHBox { gap: 8 },
            UiOp::Button {
                id: LOAD_BUTTON,
                text: "Load".into(),
            },
            UiOp::Button {
                id: SAVE_BUTTON,
                text: "Remember path".into(),
            },
            UiOp::End,
            UiOp::Label {
                text: self.status.clone(),
                class: Some("plugin-status".into()),
            },
            UiOp::Label {
                text: self.content.clone(),
                class: Some("plugin-document".into()),
            },
            UiOp::End,
        ]
    }
}

kernelos_pdk::kernelos_plugin!(DocViewer);
