//! Everything that differs between the web build (WASM in a browser) and
//! the native desktop build. Both expose the same functions.
//!
//! | | web | desktop |
//! |---|---|---|
//! | USB serial | Web Serial (Chrome/Edge) | native serial port |
//! | GridConnect TCP | — (browsers can't open TCP sockets) | yes |
//! | storage | localStorage | JSON files in the config directory |

use dioxus::document;
use futures::channel::mpsc::UnboundedReceiver;
use serde::Deserialize;

#[cfg(feature = "desktop")]
mod desktop;
#[cfg(feature = "desktop")]
pub use desktop::*;

#[cfg(all(feature = "web", not(feature = "desktop")))]
mod web;
#[cfg(all(feature = "web", not(feature = "desktop")))]
pub use web::*;

/// Events from an open link.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum LinkMsg {
    Open,
    Data { d: String },
    Closed { e: String },
    Error { e: String },
}

/// Commands to an open link.
pub enum LinkOut {
    /// GridConnect text to transmit.
    Data(String),
    Close,
}

/// An opened (or opening) transport carrying GridConnect text.
pub struct Link {
    pub tx: Box<dyn Fn(LinkOut)>,
    pub rx: UnboundedReceiver<LinkMsg>,
}

/// Apply the colour scheme to the page (and, on desktop, the native window).
pub fn apply_theme(pref: crate::storage::ThemePref) {
    use crate::storage::ThemePref;
    let dark = match pref {
        ThemePref::Dark => Some(true),
        ThemePref::Light => Some(false),
        ThemePref::Auto => system_dark(),
    };
    let js = match dark {
        Some(true) => "document.documentElement.dataset.theme = 'dark';",
        Some(false) => "document.documentElement.dataset.theme = 'light';",
        // Let CSS `prefers-color-scheme` decide.
        None => "delete document.documentElement.dataset.theme;",
    };
    let _ = document::eval(js);
    set_native_theme(dark);
}

/// Ask a yes/no question. Answers "no" if the dialog can't be shown.
pub async fn confirm(question: &str) -> bool {
    let script = format!(
        "return confirm({});",
        serde_json::to_string(question).unwrap()
    );
    document::eval(&script)
        .join::<bool>()
        .await
        .unwrap_or(false)
}
