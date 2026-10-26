//! Settings and layout persistence (localStorage on the web, JSON files in
//! the user's config directory on desktop).

use std::cell::RefCell;

use s4d_core::Layout;
use serde::{Deserialize, Serialize};

use crate::platform;

const SETTINGS_KEY: &str = "s4d.settings";
const LAYOUT_KEY: &str = "s4d.layout";

thread_local! {
    /// Problems found while loading, shown once by the UI.
    static LOAD_ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn note_load_error(msg: String) {
    LOAD_ERRORS.with(|e| {
        let mut e = e.borrow_mut();
        if !e.contains(&msg) {
            e.push(msg);
        }
    });
}

/// Problems found while loading saved data, joined into one message. Each is
/// returned only once.
pub fn take_load_error() -> Option<String> {
    let errors = LOAD_ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()));
    (!errors.is_empty()).then(|| errors.join(" "))
}

/// Keep unreadable saved data under `<key>.bak` and report it.
fn keep_unreadable(key: &str, raw: &str, what: &str, err: &str) {
    let bak = format!("{key}.bak");
    platform::storage_set(&bak, raw);
    note_load_error(format!(
        "Your saved {what} could not be read ({err}). A copy was kept as {bak}; \
         defaults are used until you save again."
    ));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnKind {
    Serial,
    Tcp,
    /// Unknown values from older versions fall back to the simulator.
    #[serde(other)]
    Simulator,
}

impl ConnKind {
    pub fn label(self) -> &'static str {
        match self {
            ConnKind::Simulator => "Simulator",
            ConnKind::Serial => "USB serial (GridConnect)",
            ConnKind::Tcp => "Network (GridConnect TCP)",
        }
    }

    /// Connection kinds this build can offer.
    pub fn available() -> Vec<ConnKind> {
        let mut v = vec![ConnKind::Simulator, ConnKind::Serial];
        if platform::TCP_AVAILABLE {
            v.push(ConnKind::Tcp);
        }
        v
    }
}

/// Colour scheme. `Auto` follows the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemePref {
    Light,
    Dark,
    #[default]
    #[serde(other)]
    Auto,
}

impl ThemePref {
    pub fn label(self) -> &'static str {
        match self {
            ThemePref::Auto => "Match system",
            ThemePref::Light => "Light",
            ThemePref::Dark => "Dark",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub conn: ConnKind,
    /// GridConnect TCP hub, host:port (desktop).
    pub tcp_addr: String,
    /// Serial port name, e.g. /dev/ttyACM0 or COM3 (desktop; the web build
    /// uses the browser's port picker instead).
    pub serial_port: String,
    pub baud: u32,
    pub node_id: String,
    pub confirm_throws: bool,
    /// Connect when the app opens. Never applies to the simulator.
    pub auto_connect: bool,
    pub show_monitor: bool,
    pub theme: ThemePref,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            // Real hardware by default; the simulator is there to try things.
            conn: if platform::TCP_AVAILABLE {
                ConnKind::Tcp
            } else {
                ConnKind::Serial
            },
            tcp_addr: "localhost:12021".into(),
            serial_port: String::new(),
            baud: 115_200,
            node_id: random_node_id(),
            confirm_throws: false,
            auto_connect: false,
            show_monitor: false,
            theme: ThemePref::Auto,
        }
    }
}

/// A random Node ID so that several panels don't collide by default.
/// Users with their own assigned range should replace it in Settings.
fn random_node_id() -> String {
    let r = platform::random_u16();
    format!("05.01.01.01.8D.{:02X}", (r & 0xFF) as u8 ^ (r >> 8) as u8)
}

pub fn load_settings() -> Settings {
    let mut s = match platform::storage_get(SETTINGS_KEY) {
        Some(raw) => match serde_json::from_str::<Settings>(&raw) {
            Ok(s) => s,
            Err(e) => {
                // Don't save over it: the user may want their Node ID back.
                keep_unreadable(SETTINGS_KEY, &raw, "settings", &e.to_string());
                Settings::default()
            }
        },
        None => {
            // Persist the generated Node ID so it stays stable.
            let s = Settings::default();
            save_settings(&s);
            s
        }
    };
    if !ConnKind::available().contains(&s.conn) {
        s.conn = ConnKind::Simulator;
    }
    s
}

pub fn save_settings(s: &Settings) {
    platform::storage_set(SETTINGS_KEY, &settings_json(s));
}

/// Settings as the JSON saved on disk and exported to a file.
pub fn settings_json(s: &Settings) -> String {
    serde_json::to_string_pretty(s).unwrap()
}

/// Settings from an exported file. Anything missing gets its default.
pub fn settings_from_json(raw: &str) -> Result<Settings, String> {
    let mut s: Settings = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if !ConnKind::available().contains(&s.conn) {
        s.conn = ConnKind::Simulator;
    }
    Ok(s)
}

/// Where the settings are kept, for display.
pub fn settings_location() -> String {
    platform::storage_location(SETTINGS_KEY)
}

/// The saved layout, if there is a readable one. An unreadable one is
/// copied to `s4d.layout.bak` and reported through [`take_load_error`].
pub fn load_local_layout() -> Option<Layout> {
    let raw = platform::storage_get(LAYOUT_KEY)?;
    match Layout::from_json(&raw) {
        Ok(l) => Some(l),
        Err(e) => {
            keep_unreadable(LAYOUT_KEY, &raw, "layout", &e);
            None
        }
    }
}

pub fn save_local_layout(l: &Layout) {
    platform::storage_set(LAYOUT_KEY, &serde_json::to_string(l).unwrap());
}
