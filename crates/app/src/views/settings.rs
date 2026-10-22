//! Connection and node settings.

use dioxus::prelude::*;
use rolcb::NodeId;

use crate::bus::{Bus, ConnStatus};
use crate::storage::{save_settings, ConnKind, Settings};
use crate::views::panel::Monitor;
use crate::BridgeInfo;

#[component]
pub fn SettingsView() -> Element {
    let bus = use_context::<Bus>();
    let mut settings = use_context::<Signal<Settings>>();
    let bridge = use_context::<Signal<Option<BridgeInfo>>>();
    let mut form = use_signal(|| settings.read().clone());
    let mut node_err = use_signal(|| None::<String>);
    let conn = bus.conn.read().clone();
    let (alias, node, dup) = bus.view(|d| (d.alias(), d.node_id(), d.duplicate_node_id()));
    let f = form.read().clone();
    let serial_ok = web_serial_supported();

    let save = move |_| {
        let new = form.read().clone();
        let node_id = match new.node_id.parse::<NodeId>() {
            Ok(n) => n,
            Err(_) => {
                node_err.set(Some(
                    "Node ID must be 6 hex bytes, e.g. 05.01.01.01.8D.42".into(),
                ));
                return;
            }
        };
        node_err.set(None);
        let old = settings.read().clone();
        save_settings(&new);
        settings.set(new.clone());
        bus.set_monitor(new.show_monitor);
        if old.node_id != new.node_id {
            bus.reset_node(node_id);
            bus.set_monitor(new.show_monitor);
        }
        let link_changed = old.conn != new.conn || old.ws_url != new.ws_url || old.baud != new.baud;
        let node_changed = old.node_id != new.node_id;
        if (link_changed || node_changed) && (bus.is_wanted() || node_changed) {
            bus.disconnect();
            if new.conn != ConnKind::Serial {
                bus.connect(&new, false);
            }
        }
    };

    rsx! {
        div { class: "settings-view",
            section { class: "card",
                h3 { "Connection" }
                for kind in [ConnKind::Simulator, ConnKind::Serial, ConnKind::WebSocket] {
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
                        p { class: "muted", "A simulated layout and command station running in your browser. Blocks change occupancy on their own, turnouts report feedback, and any DCC address can be acquired." }
                    },
                    ConnKind::Serial => rsx! {
                        p { class: "muted",
                            "Plug in a GridConnect USB adapter (RR-CirKits LCC Buffer-USB, TCS USB, SPROG CANISB…) and press Connect. "
                            "Needs Chrome or Edge, with the page served over https or from localhost."
                        }
                        if !serial_ok {
                            p { class: "warn", "This browser doesn't support Web Serial." }
                        }
                        label { "Baud rate"
                            input { r#type: "number", value: "{f.baud}", oninput: move |e| if let Ok(v) = e.value().parse() { form.write().baud = v } }
                        }
                    },
                    ConnKind::WebSocket => rsx! {
                        p { class: "muted",
                            "Connect to an " code { "s4d-bridge" } " or " code { "rolcb-hub" } " on your network, which relays to JMRI, an LCC buffer or a USB adapter."
                        }
                        label { "WebSocket URL"
                            input { class: "mono", value: "{f.ws_url}", placeholder: "ws://raspberrypi.local:8080/ws", oninput: move |e| form.write().ws_url = e.value() }
                        }
                        p { class: "muted small",
                            "Pages loaded over https can only reach ws:// on localhost. For other machines, open the app from the bridge itself (http://<bridge>:8080)."
                        }
                    },
                }
                label { class: "check",
                    input { r#type: "checkbox", checked: f.auto_connect, onchange: move |e| form.write().auto_connect = e.checked() }
                    "Connect automatically when the app opens"
                }
            }

            section { class: "card",
                h3 { "This panel's OpenLCB node" }
                label { "Node ID"
                    input { class: "mono", value: "{f.node_id}", oninput: move |e| form.write().node_id = e.value() }
                }
                if let Some(e) = node_err() { p { class: "warn", "{e}" } }
                p { class: "muted small",
                    "Every browser running System4Dispatch is its own node and needs a unique Node ID. "
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

            if let Some(b) = bridge() {
                section { class: "card",
                    h3 { "Bridge" }
                    p { class: "muted", "Served by s4d-bridge {b.version}." }
                    if !b.uplinks.is_empty() {
                        p { class: "muted small", "Uplinks: {b.uplinks.join(\", \")}" }
                    }
                }
            }

            Monitor {}

            section { class: "card about",
                h3 { "About" }
                p { "System4Dispatch {env!(\"CARGO_PKG_VERSION\")}: an OpenLCB / LCC dispatch panel and throttle that runs entirely in your browser." }
            }
        }
    }
}

fn web_serial_supported() -> bool {
    let Some(w) = web_sys::window() else {
        return false;
    };
    js_sys::Reflect::has(&w.navigator(), &"serial".into()).unwrap_or(false)
}
