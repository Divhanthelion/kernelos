//! Hello plugin — label, counter button, crash button for isolation testing.

use kernelos_pdk::{Event, Plugin, UiOp};

const BTN_INCREMENT: u32 = 1;
const BTN_CRASH: u32 = 2;
const BTN_NOTIFY: u32 = 3;

struct HelloPlugin {
    count: u32,
}

impl Default for HelloPlugin {
    fn default() -> Self {
        Self { count: 0 }
    }
}

impl Plugin for HelloPlugin {
    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Init => true,
            Event::Click { id: BTN_INCREMENT } => {
                self.count += 1;
                true
            }
            Event::Click { id: BTN_CRASH } => {
                panic!("hello plugin deliberate crash");
            }
            Event::Click { id: BTN_NOTIFY } => {
                kernelos_pdk::notify("Hello Plugin", "Notification from WASM!");
                false
            }
            _ => false,
        }
    }

    fn render(&self) -> Vec<UiOp> {
        vec![
            UiOp::BeginVBox { gap: 12 },
            UiOp::Label {
                text: "Hello from a WASM plugin!".into(),
                class: Some("plugin-title".into()),
            },
            UiOp::Label {
                text: format!("Count: {}", self.count),
                class: None,
            },
            UiOp::Button {
                id: BTN_INCREMENT,
                text: "Increment".into(),
            },
            UiOp::Button {
                id: BTN_CRASH,
                text: "Crash (test)".into(),
            },
            UiOp::Button {
                id: BTN_NOTIFY,
                text: "Notify".into(),
            },
            UiOp::End,
        ]
    }
}

kernelos_pdk::kernelos_plugin!(HelloPlugin);
