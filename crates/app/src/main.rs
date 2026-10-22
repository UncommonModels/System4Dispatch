//! System4Dispatch web app (Dioxus, compiled to WASM).

use dioxus::prelude::*;
use rolcb::NodeId;
use s4d_core::{Layout, LinkState};
use serde::Deserialize;

mod bus;
mod js;
mod storage;
mod track;
mod views {
    pub mod editor;
    pub mod panel;
    pub mod settings;
    pub mod throttle;
}

use bus::{Bus, ConnStatus};
use storage::{ConnKind, Settings};

const STYLE: Asset = asset!("/assets/style.css");
const MANIFEST: Asset = asset!("/assets/manifest.webmanifest");
const ICON: Asset = asset!("/assets/icon.svg");

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Panel,
    Throttles,
    Editor,
    Settings,
}

/// The editor's unsaved copy of the layout (kept while switching tabs).
#[derive(Clone, Copy)]
pub struct Draft(pub Signal<Option<Layout>>);

/// Present when the page was served by `s4d-bridge`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BridgeInfo {
    pub version: String,
    #[serde(default)]
    pub uplinks: Vec<String>,
    #[serde(default)]
    pub shared_layout: bool,
}

fn main() {
    dioxus::launch(App);
}

/// Persist a layout locally, push it to the bridge if there is one, and
/// apply it to the dispatcher.
pub fn save_layout(bus: Bus, layout: Layout) {
    storage::save_local_layout(&layout);
    let shared = try_consume_context::<Signal<Option<BridgeInfo>>>()
        .and_then(|b| b.peek().clone())
        .is_some_and(|b| b.shared_layout);
    if shared {
        let json = layout.to_json();
        dioxus::core::spawn_forever(async move {
            let _ = js::fetch("layout.json", "PUT", Some(&json)).await;
        });
    }
    bus.set_layout(layout);
}

#[component]
fn App() -> Element {
    let settings = use_context_provider(|| Signal::new(storage::load_settings()));
    let bridge = use_context_provider(|| Signal::new(None::<BridgeInfo>));
    use_context_provider(|| Draft(Signal::new(None)));
    let bus = use_context_provider(|| {
        let s = settings.peek();
        let node = s
            .node_id
            .parse::<NodeId>()
            .unwrap_or(NodeId::new(0x0501_0101_8D00));
        let layout = storage::load_local_layout().unwrap_or_else(Layout::demo);
        let bus = Bus::new(node, layout);
        bus.set_monitor(s.show_monitor);
        bus
    });
    let mut tab = use_signal(|| Tab::Panel);

    // Startup: detect the bridge, load the shared layout, auto-connect.
    use_hook(move || {
        js::register_service_worker();
        spawn(async move {
            let mut settings = settings;
            let mut bridge = bridge;
            if let Some(info) = js::fetch("api/info", "GET", None)
                .await
                .and_then(|t| serde_json::from_str::<BridgeInfo>(&t).ok())
            {
                if info.shared_layout {
                    if let Some(l) = js::fetch("layout.json", "GET", None)
                        .await
                        .and_then(|t| Layout::from_json(&t).ok())
                    {
                        storage::save_local_layout(&l);
                        bus.set_layout(l);
                    }
                }
                // Served by the bridge: default to talking back to it.
                let mut s = settings.peek().clone();
                if s.ws_url.is_empty() {
                    s.ws_url = js::default_ws_url().await;
                    s.conn = ConnKind::WebSocket;
                    storage::save_settings(&s);
                    settings.set(s);
                }
                bridge.set(Some(info));
            } else if settings.peek().ws_url.is_empty() {
                settings.write().ws_url = "ws://localhost:12022".into();
            }
            let s = settings.peek().clone();
            if s.auto_connect {
                bus.connect(&s, true);
            }
        });
    });

    // Protocol timers + simulator.
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(50).await;
            bus.tick();
        }
    });

    rsx! {
        document::Title { "System4Dispatch" }
        document::Stylesheet { href: STYLE }
        document::Link { rel: "manifest", href: MANIFEST }
        document::Link { rel: "icon", href: ICON }
        document::Meta { name: "theme-color", content: "#14181c" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        div { class: "app",
            Header { tab }
            main {
                match tab() {
                    Tab::Panel => rsx! { views::panel::PanelView {} },
                    Tab::Throttles => rsx! { views::throttle::ThrottleView {} },
                    Tab::Editor => rsx! { views::editor::EditorView {} },
                    Tab::Settings => rsx! { views::settings::SettingsView {} },
                }
            }
            nav { class: "tabbar",
                for (t, icon, label) in [(Tab::Panel, "▦", "Panel"), (Tab::Throttles, "◉", "Throttles"), (Tab::Editor, "✎", "Editor"), (Tab::Settings, "⚙", "Settings")] {
                    button {
                        class: if tab() == t { "on" } else { "" },
                        onclick: move |_| tab.set(t),
                        span { class: "icon", "{icon}" }
                        span { "{label}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Header(tab: Signal<Tab>) -> Element {
    let bus = use_context::<Bus>();
    let settings = use_context::<Signal<Settings>>();
    let conn = bus.conn.read().clone();
    let (name, link, alias, has_throttles) = bus.view(|d| {
        (
            d.layout().name.clone(),
            d.link_state(),
            d.alias(),
            !d.throttles.is_empty(),
        )
    });
    let kind = settings.read().conn;
    let (cls, text) = match (&conn, link) {
        (ConnStatus::Error(_), _) => ("pill err", "Error".to_string()),
        (ConnStatus::Connecting, _) => ("pill warn", "Connecting…".to_string()),
        (_, LinkState::LoggingIn) => ("pill warn", "Logging in…".to_string()),
        (_, LinkState::Online) => (
            "pill ok",
            format!(
                "Online{}",
                alias.map(|a| format!(" · {a:03X}")).unwrap_or_default()
            ),
        ),
        _ => ("pill", "Offline".to_string()),
    };
    let connected = !matches!(conn, ConnStatus::Disconnected | ConnStatus::Error(_));
    let title = match &conn {
        ConnStatus::Error(e) => e.clone(),
        _ => kind.label().to_string(),
    };

    rsx! {
        header { class: "topbar",
            div { class: "brand",
                img { src: ICON, alt: "" }
                div {
                    div { class: "title", "System4Dispatch" }
                    div { class: "subtitle", "{name}" }
                }
            }
            div { class: "top-actions",
                if has_throttles {
                    button { class: "stopall", onclick: move |_| bus.act(|d, _| d.stop_all()), "STOP ALL" }
                }
                span { class: "{cls}", title: "{title}", "{text}" }
                if connected {
                    button { onclick: move |_| bus.disconnect(), "Disconnect" }
                } else {
                    button {
                        class: "primary",
                        onclick: move |_| {
                            let s = settings.read().clone();
                            bus.connect(&s, false);
                        },
                        "Connect"
                    }
                }
            }
        }
    }
}
