//! System4Dispatch (Dioxus): runs in the browser (WASM, feature `web`) or as
//! a native desktop app (feature `desktop`).

use dioxus::prelude::*;
use rolcb::NodeId;
use s4d_core::{Layout, LinkState};

mod bus;
mod platform;
mod storage;
mod track;
mod zoom;
mod views {
    pub mod editor;
    pub mod help;
    pub mod panel;
    pub mod settings;
    pub mod throttle;
}

use bus::{Bus, ConnStatus};
use storage::Settings;

const STYLE: Asset = asset!("/assets/style.css");
const MANIFEST: Asset = asset!("/assets/manifest.webmanifest");
const ICON: Asset = asset!("/assets/icon.svg");

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Panel,
    Throttles,
    Editor,
    Settings,
    Help,
}

/// The editor's unsaved copy of the layout (kept while switching tabs).
#[derive(Clone, Copy)]
pub struct Draft(pub Signal<Option<Layout>>);

/// Editor undo/redo stacks (kept while switching tabs).
#[derive(Clone, Copy)]
pub struct History {
    pub undo: Signal<Vec<Layout>>,
    pub redo: Signal<Vec<Layout>>,
}

fn main() {
    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::tao::window::Theme;
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        let dark = match storage::load_settings().theme {
            storage::ThemePref::Dark => true,
            storage::ThemePref::Light => false,
            storage::ThemePref::Auto => platform::system_dark().unwrap_or(true),
        };
        let window = WindowBuilder::new()
            .with_title("System4Dispatch")
            .with_inner_size(LogicalSize::new(1320.0, 860.0))
            .with_min_inner_size(LogicalSize::new(420.0, 520.0))
            .with_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
        // Matches --bg so there's no white flash before the page loads.
        let bg = if dark {
            (20, 24, 28, 255)
        } else {
            (238, 241, 244, 255)
        };
        let cfg = Config::new()
            .with_menu(None)
            .with_window(window)
            .with_background_color(bg)
            .with_disable_context_menu(!cfg!(debug_assertions));
        dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
    }
    #[cfg(not(feature = "desktop"))]
    dioxus::launch(App);
}

/// Persist a layout and apply it to the dispatcher.
pub fn save_layout(bus: Bus, layout: Layout) {
    storage::save_local_layout(&layout);
    bus.set_layout(layout);
}

#[component]
fn App() -> Element {
    let settings = use_context_provider(|| Signal::new(storage::load_settings()));
    use_context_provider(|| Draft(Signal::new(None)));
    use_context_provider(|| History {
        undo: Signal::new(Vec::new()),
        redo: Signal::new(Vec::new()),
    });
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
    // Saved data that couldn't be read (settings and layout are loaded above).
    let mut load_error = use_signal(storage::take_load_error);

    // Startup: offline support (web) and auto-connect. An effect, not a hook,
    // because connecting writes signals, which mustn't happen during render.
    // It reads signals only with `peek`, so it runs once.
    use_effect(move || {
        platform::register_service_worker();
        let s = settings.peek().clone();
        platform::apply_theme(s.theme);
        if s.auto_connect {
            bus.connect(&s, true);
        }
    });

    // Protocol timers + simulator.
    use_future(move || async move {
        loop {
            platform::sleep_ms(50).await;
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
            if let Some(e) = load_error() {
                div {
                    class: "error",
                    role: "alert",
                    title: "Click to dismiss",
                    onclick: move |_| load_error.set(None),
                    "{e} (click to dismiss)"
                }
            }
            main {
                match tab() {
                    Tab::Panel => rsx! { views::panel::PanelView {} },
                    Tab::Throttles => rsx! { views::throttle::ThrottleView {} },
                    Tab::Editor => rsx! { views::editor::EditorView {} },
                    Tab::Settings => rsx! { views::settings::SettingsView {} },
                    Tab::Help => rsx! { views::help::HelpView {} },
                }
            }
            nav { class: "tabbar",
                for (t, icon, label) in [(Tab::Panel, "▦", "Panel"), (Tab::Throttles, "◉", "Throttles"), (Tab::Editor, "✎", "Editor"), (Tab::Settings, "⚙", "Settings"), (Tab::Help, "?", "Help")] {
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
