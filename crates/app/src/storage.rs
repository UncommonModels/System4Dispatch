//! Settings and layout persistence (browser localStorage, plus the bridge's
//! shared `/layout.json` when the app is served by `s4d-bridge`).

use s4d_core::Layout;
use serde::{Deserialize, Serialize};

const SETTINGS_KEY: &str = "s4d.settings";
const LAYOUT_KEY: &str = "s4d.layout";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnKind {
    Simulator,
    Serial,
    WebSocket,
}

impl ConnKind {
    pub fn label(self) -> &'static str {
        match self {
            ConnKind::Simulator => "Simulator",
            ConnKind::Serial => "USB serial (GridConnect)",
            ConnKind::WebSocket => "Network (WebSocket)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub conn: ConnKind,
    pub ws_url: String,
    pub baud: u32,
    pub node_id: String,
    pub confirm_throws: bool,
    pub auto_connect: bool,
    pub show_monitor: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            conn: ConnKind::Simulator,
            ws_url: String::new(),
            baud: 115_200,
            node_id: random_node_id(),
            confirm_throws: false,
            auto_connect: true,
            show_monitor: false,
        }
    }
}

/// A random Node ID so that several browsers don't collide by default.
/// Users with their own assigned range should replace it in Settings.
fn random_node_id() -> String {
    let r = (js_sys::Math::random() * 65536.0) as u16;
    format!("05.01.01.01.8D.{:02X}", (r & 0xFF) as u8 ^ (r >> 8) as u8)
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

fn set(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

pub fn load_settings() -> Settings {
    let s = get(SETTINGS_KEY).and_then(|s| serde_json::from_str(&s).ok());
    match s {
        Some(s) => s,
        None => {
            // Persist the generated Node ID so it stays stable.
            let s = Settings::default();
            save_settings(&s);
            s
        }
    }
}

pub fn save_settings(s: &Settings) {
    set(SETTINGS_KEY, &serde_json::to_string(s).unwrap());
}

pub fn load_local_layout() -> Option<Layout> {
    get(LAYOUT_KEY).and_then(|s| Layout::from_json(&s).ok())
}

pub fn save_local_layout(l: &Layout) {
    set(LAYOUT_KEY, &serde_json::to_string(l).unwrap());
}
