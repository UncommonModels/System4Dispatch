//! The dispatcher's panel: live track diagram, turnout controls, block list.

use dioxus::prelude::*;
use s4d_core::{LinkState, Occupancy, Position, Shape};

use crate::bus::Bus;
use crate::platform;
use crate::storage::Settings;
use crate::track::{Mode, TrackView};
use crate::zoom::use_zoom_pan;

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
    let (layout, blocks, turnouts, turntables, online) = bus.view(|d| {
        (
            d.layout().clone(),
            d.blocks.clone(),
            d.turnouts.clone(),
            d.turntables.clone(),
            d.link_state() == LinkState::Online,
        )
    });

    let throw = move |id: String, pos: Option<Position>| {
        let name = bus.peek(|d| {
            d.layout()
                .turnout(&id)
                .map(|t| t.display_name().to_string())
        });
        let name = name.unwrap_or_else(|| id.clone());
        let ask = settings.read().confirm_throws;
        spawn(async move {
            if ask && !platform::confirm(&format!("Throw turnout {name}?")).await {
                return;
            }
            bus.act(|d, _| match pos {
                Some(p) => d.throw(&id, p),
                None => d.toggle(&id),
            });
        });
    };

    // Turntables: `Some(i)` turns to track i the short way, `None` with
    // `step` sends a single clockwise / counter-clockwise step.
    let turn = move |id: String, to: Option<usize>, step: bool| {
        let names = bus.peek(|d| {
            d.layout().turntable(&id).map(|t| {
                let target = match to {
                    Some(i) => t
                        .tracks
                        .get(i)
                        .map(|tr| tr.display_name())
                        .unwrap_or_default(),
                    None if step => "the next track clockwise".into(),
                    None => "the next track counter-clockwise".into(),
                };
                (t.display_name().to_string(), target)
            })
        });
        let (name, target) = names.unwrap_or_else(|| (id.clone(), String::new()));
        let ask = settings.read().confirm_throws;
        spawn(async move {
            if ask && !platform::confirm(&format!("Turn {name} to {target}?")).await {
                return;
            }
            bus.act(|d, _| match to {
                Some(i) => d.turn_to(&id, i),
                None => d.step_turntable(&id, step),
            });
        });
    };
    let pair = move |shape: Shape, left: String, right: String| {
        let ask = settings.read().confirm_throws;
        spawn(async move {
            let what = shape.label();
            if ask
                && !platform::confirm(&format!("Change {what} {left}/{right} to its next route?"))
                    .await
            {
                return;
            }
            bus.act(|d, _| d.next_route(shape, &left, &right));
        });
    };

    let zoom = use_zoom_pan(".panel-scroll", true, None);

    rsx! {
        div { class: "panel-view",
            div { class: "panel-board",
                if layout.elements.is_empty() {
                    div { class: "empty",
                        p { "This layout has no track yet." }
                        p { "Open the " b { "Editor" } " from the dock to draw your panel, or load the demo layout there." }
                    }
                } else {
                    div { class: "zoom-bar",
                        button { class: "icon-btn", title: "Zoom out (Ctrl+wheel)", onclick: move |_| zoom.zoom_by(1.0 / 1.25), "−" }
                        button { class: "zoom-label", title: "Fit the whole height of the layout", onclick: move |_| zoom.set(None), "{zoom.label()}" }
                        button { class: "icon-btn", title: "Zoom in (Ctrl+wheel)", onclick: move |_| zoom.zoom_by(1.25), "+" }
                        button {
                            class: "icon-btn",
                            title: "Ask the layout again for every block's occupancy and turnout's position",
                            disabled: !online,
                            onclick: move |_| bus.act(|d, _| d.query_state()),
                            "⟳"
                        }
                    }
                    div { class: "panel-scroll", title: "Drag to scroll, Ctrl+wheel to zoom",
                        TrackView {
                            layout: layout.clone(),
                            blocks: blocks.clone(),
                            turnouts: turnouts.clone(),
                            turntables: turntables.clone(),
                            mode: Mode::Dispatch,
                            cell_px: (zoom.px)(),
                            on_turnout: move |id: String| throw(id, None),
                            on_turntable: move |(id, i): (String, usize)| turn(id, Some(i), false),
                            on_pair: move |(shape, l, r): (Shape, String, String)| pair(shape, l, r),
                        }
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
                                    span { class: "name", title: "{t.id}", "{t.display_name()}" }
                                    if let Some((p, _)) = layout.linked_partner(&t.id) {
                                        span { class: "link-badge", title: "Thrown together with {p}", "⇄ {p}" }
                                    }
                                    if st.stalled {
                                        span {
                                            class: "badge warn",
                                            title: "The last command was not confirmed by the turnout's feedback sensor",
                                            "no feedback"
                                        }
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
                if !layout.turntables.is_empty() {
                    section {
                        h3 { "Turntables" }
                        for t in layout.turntables.iter() {
                            {
                                let st = turntables.get(&t.id).copied().unwrap_or_default();
                                let known = st.pending.or(st.position).is_some();
                                let at = st
                                    .position
                                    .and_then(|i| t.tracks.get(i))
                                    .map(|tr| tr.display_name())
                                    .unwrap_or_else(|| "Unknown".into());
                                let (ccw_id, cw_id, set_id) = (t.id.clone(), t.id.clone(), t.id.clone());
                                let selected = st.position.map(|i| i.to_string()).unwrap_or_default();
                                rsx! {
                                    div { class: "row",
                                        span { class: "name", title: "{t.id}", "{t.display_name()}" }
                                        if st.stalled {
                                            span {
                                                class: "badge warn",
                                                title: "The turntable did not report reaching the last track",
                                                "no feedback"
                                            }
                                        }
                                        span { class: if st.pending.is_some() { "badge pending" } else { "badge" },
                                            if st.pending.is_some() { "Turning…" } else { "{at}" }
                                        }
                                        div { class: "seg-buttons",
                                            button {
                                                disabled: !online || t.ccw_event.is_none(),
                                                title: "Turn one track counter-clockwise",
                                                onclick: move |_| turn(ccw_id.clone(), None, false),
                                                "⟲"
                                            }
                                            button {
                                                disabled: !online || t.cw_event.is_none(),
                                                title: "Turn one track clockwise",
                                                onclick: move |_| turn(cw_id.clone(), None, true),
                                                "⟳"
                                            }
                                        }
                                    }
                                    div { class: "tt-positions",
                                        for (i, tr) in t.tracks.iter().enumerate() {
                                            {
                                                let id = t.id.clone();
                                                let class = if st.pending == Some(i) {
                                                    "pending"
                                                } else if st.position == Some(i) && st.pending.is_none() {
                                                    "on"
                                                } else {
                                                    ""
                                                };
                                                rsx! {
                                                    button {
                                                        class: "{class}",
                                                        disabled: !online || !known,
                                                        title: if known { "Turn the bridge here" } else { "Set where the bridge is first" },
                                                        onclick: move |_| turn(id.clone(), Some(i), false),
                                                        "{tr.display_name()}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    label { class: "tt-at",
                                        "Bridge is at "
                                        select {
                                            onchange: move |e| {
                                                let v = e.value().parse::<usize>().ok();
                                                let id = set_id.clone();
                                                bus.act(move |d, _| { d.set_turntable_position(&id, v); true });
                                            },
                                            option { value: "", selected: selected.is_empty(), "Unknown" }
                                            for (i, tr) in t.tracks.iter().enumerate() {
                                                option { value: "{i}", selected: selected == i.to_string(), "{tr.display_name()}" }
                                            }
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
                                    span { class: "name", title: "{b.id}", "{b.display_name()}" }
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

/// The bus monitor in a window of its own. Logging is on while it's open.
#[component]
pub fn MonitorWindow() -> Element {
    let bus = use_context::<Bus>();
    let settings = use_context::<Signal<Settings>>();
    use_hook(move || bus.set_monitor(true));
    use_drop(move || bus.set_monitor(settings.peek().show_monitor));
    // While paused, the lines as they were when Pause was pressed.
    let mut frozen = use_signal(|| None::<Vec<String>>);
    let live: Vec<String> = bus.view(|d| d.traffic.iter().cloned().collect());
    let paused = frozen.read().is_some();
    let lines = frozen.read().clone().unwrap_or(live);
    rsx! {
        div { class: "term",
            div { class: "term-bar",
                span { class: if paused { "term-state paused" } else { "term-state" },
                    if paused { "❚❚ paused" } else { "● live" }
                }
                span { class: "term-count", "{lines.len()} lines" }
                button {
                    onclick: move |_| {
                        let now = if frozen.peek().is_some() {
                            None
                        } else {
                            Some(bus.view(|d| d.traffic.iter().cloned().collect()))
                        };
                        frozen.set(now);
                    },
                    if paused { "Resume" } else { "Pause" }
                }
                button {
                    onclick: move |_| {
                        bus.act(|d, _| {
                            d.traffic.clear();
                            d.revision += 1;
                        });
                        if frozen.peek().is_some() {
                            frozen.set(Some(Vec::new()));
                        }
                    },
                    "Clear"
                }
            }
            // A reversed column keeps the newest line in view as lines arrive.
            div { class: "term-scroll",
                div { class: "term-lines",
                    if lines.is_empty() {
                        div { class: "term-line sys", "Waiting for traffic…" }
                    }
                    for (i, l) in lines.iter().enumerate() {
                        {
                            let (class, text) = if let Some(t) = l.strip_prefix("→ ") {
                                ("term-line tx", t)
                            } else if let Some(t) = l.strip_prefix("← ") {
                                ("term-line rx", t)
                            } else {
                                ("term-line sys", l.as_str())
                            };
                            let mark = match class {
                                "term-line tx" => "→",
                                "term-line rx" => "←",
                                _ => "•",
                            };
                            rsx! {
                                div { key: "{i}", class: "{class}",
                                    span { class: "term-mark", "{mark}" }
                                    span { "{text}" }
                                }
                            }
                        }
                    }
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
