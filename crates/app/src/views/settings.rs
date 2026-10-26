//! Connection and node settings.

use dioxus::prelude::*;
use rolcb::NodeId;

use crate::bus::{Bus, ConnStatus};
use crate::platform;
use crate::storage::{
    save_settings, settings_file_from_json, settings_file_json, storage_locations, ConnKind,
    Settings, ThemePref,
};
use crate::views::panel::Monitor;

#[component]
pub fn SettingsView() -> Element {
    let bus = use_context::<Bus>();
    let settings = use_context::<Signal<Settings>>();
    let mut form = use_signal(|| settings.read().clone());
    let mut node_err = use_signal(|| None::<String>);
    let mut file_msg = use_signal(|| None::<String>);
    let mut draft = use_context::<crate::Draft>().0;
    let mut ports = use_signal(platform::serial_ports);
    let conn = bus.conn.read().clone();
    let (alias, node, dup) = bus.view(|d| (d.alias(), d.node_id(), d.duplicate_node_id()));
    let f = form.read().clone();
    let desktop = platform::TCP_AVAILABLE;

    let save = move |_| {
        let new = form.read().clone();
        match apply_settings(bus, settings, new) {
            Ok(()) => node_err.set(None),
            Err(e) => node_err.set(Some(e)),
        }
    };
    let (settings_path, layout_path) = storage_locations();

    rsx! {
        div { class: "settings-view",
            section { class: "card",
                h3 { "Connection" }
                for kind in ConnKind::available() {
                    label { class: "radio",
                        input {
                            r#type: "radio", name: "conn",
                            checked: f.conn == kind,
                            onchange: move |_| form.write().conn = kind,
                        }
                        span { "{kind.label()}" }
                    }
                }
                match f.conn {
                    ConnKind::Simulator => rsx! {
                        p { class: "muted", "A simulated layout and command station. Blocks change occupancy on their own, turnouts report feedback, and any DCC address can be acquired." }
                    },
                    ConnKind::Serial if desktop => rsx! {
                        p { class: "muted", "A GridConnect USB adapter (RR-CirKits LCC Buffer-USB, TCS USB, SPROG CANISB…) connected to this computer." }
                        div { class: "pair",
                            label { "Port"
                                select {
                                    onchange: move |e| form.write().serial_port = e.value(),
                                    option { value: "", selected: f.serial_port.is_empty(), "— choose —" }
                                    for p in ports.read().iter() {
                                        option { value: "{p}", selected: f.serial_port == *p, "{p}" }
                                    }
                                    if !f.serial_port.is_empty() && !ports.read().contains(&f.serial_port) {
                                        option { value: "{f.serial_port}", selected: true, "{f.serial_port} (not found)" }
                                    }
                                }
                            }
                            label { "Baud rate"
                                input { r#type: "number", value: "{f.baud}", oninput: move |e| if let Ok(v) = e.value().parse() { form.write().baud = v } }
                            }
                        }
                        button { onclick: move |_| ports.set(platform::serial_ports()), "Refresh ports" }
                    },
                    ConnKind::Serial => rsx! {
                        p { class: "muted",
                            "Plug in a GridConnect USB adapter (RR-CirKits LCC Buffer-USB, TCS USB, SPROG CANISB…) and press Connect; your browser will ask which port to use. "
                            "Needs Chrome or Edge on a desktop computer."
                        }
                        if !platform::serial_supported() {
                            p { class: "warn", "This browser doesn't support USB serial. Use Chrome or Edge, or the desktop app." }
                        }
                        label { "Baud rate"
                            input { r#type: "number", value: "{f.baud}", oninput: move |e| if let Ok(v) = e.value().parse() { form.write().baud = v } }
                        }
                    },
                    ConnKind::Tcp => rsx! {
                        p { class: "muted",
                            "A GridConnect TCP hub: JMRI's LCC hub, an Ethernet LCC buffer, or "
                            code { "rolcb-hub" } " (port 12021 by default)."
                        }
                        label { "Hub address (host:port)"
                            input { class: "mono", value: "{f.tcp_addr}", placeholder: "jmri.local:12021", oninput: move |e| form.write().tcp_addr = e.value() }
                        }
                    },
                }
                if !desktop {
                    p { class: "muted small", "Need to connect over the network to JMRI or an Ethernet LCC buffer? Use the System4Dispatch desktop app." }
                }
                label { class: "check",
                    input { r#type: "checkbox", checked: f.auto_connect, onchange: move |e| form.write().auto_connect = e.checked() }
                    "Connect automatically when the app opens"
                }
                p { class: "muted small", "The simulator never connects by itself; press Connect to start it." }
            }

            section { class: "card",
                h3 { "This panel's OpenLCB node" }
                label { "Node ID"
                    input { class: "mono", value: "{f.node_id}", oninput: move |e| form.write().node_id = e.value() }
                }
                if let Some(e) = node_err() { p { class: "warn", "{e}" } }
                p { class: "muted small",
                    "Every running System4Dispatch is its own node and needs a unique Node ID. "
                    "A random one was generated; if you have your own assigned range, use an ID from it."
                }
                if dup {
                    p { class: "warn", "Another node on the bus is using Node ID {node}. Choose a different one." }
                }
                p { class: "muted small",
                    "Current node {node}"
                    if let Some(a) = alias { ", alias {a:03X}" }
                }
            }

            section { class: "card",
                h3 { "Appearance" }
                div { class: "seg-buttons theme-picker",
                    for t in [ThemePref::Auto, ThemePref::Light, ThemePref::Dark] {
                        button {
                            class: if f.theme == t { "on" } else { "" },
                            onclick: move |_| form.write().theme = t,
                            "{t.label()}"
                        }
                    }
                }
            }

            section { class: "card",
                h3 { "Operation" }
                label { class: "check",
                    input { r#type: "checkbox", checked: f.confirm_throws, onchange: move |e| form.write().confirm_throws = e.checked() }
                    "Ask before throwing a turnout"
                }
                label { class: "check",
                    input { r#type: "checkbox", checked: f.show_monitor, onchange: move |e| form.write().show_monitor = e.checked() }
                    "Show bus monitor on the panel"
                }
            }

            div { class: "buttons sticky",
                button { class: "primary", disabled: f == *settings.read(), onclick: save, "Save settings" }
                match conn {
                    ConnStatus::Error(e) => rsx! { span { class: "warn", "{e}" } },
                    _ => rsx! {},
                }
            }

            section { class: "card",
                h3 { "Settings file" }
                p { class: "muted small",
                    "Settings are saved when you press Save settings, and the track layout when you press Apply & save in the editor. Both are loaded every time the app opens: settings from "
                    code { "{settings_path}" } ", the layout from " code { "{layout_path}" } "."
                }
                p { class: "muted small",
                    "Export puts both in one file; importing that file replaces your settings and layout. "
                    "Live state (block occupancy, turnout positions, throttles) is never saved."
                }
                div { class: "buttons",
                    button {
                        onclick: move |_| {
                            let layout = bus.peek(|d| d.layout().clone());
                            platform::save_file("system4dispatch-settings.json", &settings_file_json(&settings.read(), &layout));
                        },
                        "Export settings…"
                    }
                    button {
                        onclick: move |_| async move {
                            let Some(text) = platform::open_file().await else { return };
                            match settings_file_from_json(&text) {
                                Ok((new, layout)) => {
                                    if let Some(l) = layout {
                                        // The editor's unsaved copy is of the old layout.
                                        draft.set(None);
                                        crate::save_layout(bus, l);
                                    }
                                    form.set(new.clone());
                                    match apply_settings(bus, settings, new) {
                                        Ok(()) => file_msg.set(Some("Settings imported.".into())),
                                        Err(e) => file_msg.set(Some(e)),
                                    }
                                }
                                Err(e) => file_msg.set(Some(format!("That file isn't a settings file: {e}"))),
                            }
                        },
                        "Import settings…"
                    }
                }
                if let Some(m) = file_msg() { p { class: "muted small", "{m}" } }
            }

            Monitor {}

            section { class: "card about",
                h3 { "About" }
                p {
                    "System4Dispatch {env!(\"CARGO_PKG_VERSION\")}: an OpenLCB / LCC dispatch panel and throttle"
                    if desktop { " (desktop app)." } else { " running in your browser." }
                }
            }
        }
    }
}

/// Save and apply new settings: theme, monitor, Node ID, and reconnect if
/// the link changed while connected.
fn apply_settings(bus: Bus, mut settings: Signal<Settings>, new: Settings) -> Result<(), String> {
    let node_id = new
        .node_id
        .parse::<NodeId>()
        .map_err(|_| "Node ID must be 6 hex bytes, e.g. 05.01.01.01.8D.42".to_string())?;
    let old = settings.peek().clone();
    save_settings(&new);
    settings.set(new.clone());
    bus.set_monitor(new.show_monitor);
    platform::apply_theme(new.theme);
    let node_changed = old.node_id != new.node_id;
    // Captured before reset_node, which disconnects.
    let was_wanted = bus.is_wanted();
    if node_changed {
        bus.reset_node(node_id);
        bus.set_monitor(new.show_monitor);
    }
    let link_changed = old.conn != new.conn
        || old.tcp_addr != new.tcp_addr
        || old.serial_port != new.serial_port
        || old.baud != new.baud;
    if (link_changed || node_changed) && was_wanted {
        bus.disconnect();
        // Web Serial needs a click on Connect to pick the port.
        if platform::TCP_AVAILABLE || new.conn != ConnKind::Serial {
            bus.connect(&new, false);
        }
    }
    Ok(())
}
