//! The dispatcher's panel: live track diagram, turnout controls, block list.

use dioxus::prelude::*;
use s4d_core::{LinkState, Occupancy, Position};

use crate::bus::Bus;
use crate::js;
use crate::storage::Settings;
use crate::track::{Mode, TrackView};

fn position_label(p: Position) -> &'static str {
    match p {
        Position::Normal => "Normal",
        Position::Reverse => "Reverse",
        Position::Unknown => "?",
    }
}

#[component]
pub fn PanelView() -> Element {
    let bus = use_context::<Bus>();
    let settings = use_context::<Signal<Settings>>();
    let (layout, blocks, turnouts, online) = bus.view(|d| {
        (
            d.layout().clone(),
            d.blocks.clone(),
            d.turnouts.clone(),
            d.link_state() == LinkState::Online,
        )
    });

    let throw = move |id: String, pos: Option<Position>| {
        let name = bus.peek(|d| {
            d.layout().turnout(&id).map(|t| {
                if t.name.is_empty() {
                    t.id.clone()
                } else {
                    t.name.clone()
                }
            })
        });
        let name = name.unwrap_or_else(|| id.clone());
        if settings.read().confirm_throws && !js::confirm(&format!("Throw turnout {name}?")) {
            return;
        }
        bus.act(|d, _| match pos {
            Some(p) => d.throw(&id, p),
            None => d.toggle(&id),
        });
    };

    rsx! {
        div { class: "panel-view",
            div { class: "panel-board",
                if layout.elements.is_empty() {
                    div { class: "empty",
                        p { "This layout has no track yet." }
                        p { "Open the " b { "Editor" } " tab to draw your panel, or load the demo layout there." }
                    }
                } else {
                    TrackView {
                        layout: layout.clone(),
                        blocks: blocks.clone(),
                        turnouts: turnouts.clone(),
                        mode: Mode::Dispatch,
                        on_turnout: move |id: String| throw(id, None),
                    }
                }
                if !online {
                    div { class: "offline-banner", "Not connected. Turnouts can't be thrown and states may be stale." }
                }
            }
            div { class: "panel-side",
                section {
                    h3 { "Turnouts" }
                    if layout.turnouts.is_empty() { p { class: "muted", "None defined." } }
                    for t in layout.turnouts.iter() {
                        {
                            let st = turnouts.get(&t.id).copied().unwrap_or_default();
                            let id_n = t.id.clone();
                            let id_r = t.id.clone();
                            let shown = st.pending.unwrap_or(st.position);
                            rsx! {
                                div { class: "row",
                                    span { class: "name", title: "{t.id}",
                                        if t.name.is_empty() { "{t.id}" } else { "{t.name}" }
                                    }
                                    span { class: if st.pending.is_some() { "badge pending" } else { "badge" },
                                        "{position_label(shown)}"
                                    }
                                    div { class: "seg-buttons",
                                        button {
                                            class: if st.position == Position::Normal { "on" } else { "" },
                                            disabled: !online,
                                            onclick: move |_| throw(id_n.clone(), Some(Position::Normal)),
                                            "N"
                                        }
                                        button {
                                            class: if st.position == Position::Reverse { "on" } else { "" },
                                            disabled: !online,
                                            onclick: move |_| throw(id_r.clone(), Some(Position::Reverse)),
                                            "R"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                section {
                    h3 { "Blocks" }
                    if layout.blocks.is_empty() { p { class: "muted", "None defined." } }
                    for b in layout.blocks.iter() {
                        {
                            let occ = blocks.get(&b.id).copied().unwrap_or_default();
                            let (cls, label) = match occ {
                                Occupancy::Occupied => ("dot occ-occupied", "Occupied"),
                                Occupancy::Clear => ("dot occ-clear", "Clear"),
                                Occupancy::Unknown => ("dot occ-unknown", "Unknown"),
                            };
                            rsx! {
                                div { class: "row",
                                    span { class: "{cls}" }
                                    span { class: "name", title: "{b.id}",
                                        if b.name.is_empty() { "{b.id}" } else { "{b.name}" }
                                    }
                                    span { class: "muted small", "{label}" }
                                }
                            }
                        }
                    }
                }
                if settings.read().show_monitor {
                    Monitor {}
                }
            }
        }
    }
}

/// Recent decoded bus traffic.
#[component]
pub fn Monitor() -> Element {
    let bus = use_context::<Bus>();
    let lines: Vec<String> = bus.view(|d| d.traffic.iter().rev().take(80).cloned().collect());
    rsx! {
        section { class: "monitor",
            h3 { "Bus monitor" }
            pre {
                for l in lines.iter() {
                    "{l}\n"
                }
            }
        }
    }
}
