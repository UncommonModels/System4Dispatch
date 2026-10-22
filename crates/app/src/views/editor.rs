//! Layout editor: paint track tiles on the grid, assign them to blocks and
//! turnouts, and enter (or learn) the OpenLCB events for each.

use std::collections::BTreeMap;

use dioxus::prelude::*;
use rolcb::EventId;
use s4d_core::layout::{Block, Feedback, Turnout};
use s4d_core::{Element as Tile, Layout, Shape};

use crate::bus::Bus;
use crate::js;
use crate::track::{Mode, TrackView};
use crate::Draft;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Place(Shape),
    Label,
    Erase,
}

fn ev_str(e: Option<EventId>) -> String {
    e.map(|e| e.to_string()).unwrap_or_default()
}

/// Parse an event field: empty clears it, invalid input is reported.
fn parse_ev(s: &str) -> Result<Option<EventId>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<EventId>().map(Some).map_err(|_| {
        format!("“{s}” is not an event ID (expected 8 hex bytes, e.g. 05.01.01.01.22.00.00.10)")
    })
}

#[component]
pub fn EditorView() -> Element {
    let bus = use_context::<Bus>();
    let mut draft = use_context::<Draft>().0;
    let mut tool = use_signal(|| Tool::Place(Shape::Straight));
    let mut rot = use_signal(|| 0u8);
    let mut selected = use_signal(|| None::<(u16, u16)>);
    let mut paint_block = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);

    let applied = bus.view(|d| d.layout().clone());
    let recent: Vec<EventId> = bus.view(|d| d.recent_events.iter().copied().collect());
    let layout = draft.read().clone().unwrap_or_else(|| applied.clone());
    let dirty = draft.read().as_ref().is_some_and(|d| *d != applied);
    let issues = layout.validate();

    // Mutate the draft copy.
    let mut edit = move |f: &mut dyn FnMut(&mut Layout)| {
        let mut l = draft
            .read()
            .clone()
            .unwrap_or_else(|| bus.peek(|d| d.layout().clone()));
        f(&mut l);
        draft.set(Some(l));
    };

    let on_cell = move |(x, y, drag): (u16, u16, bool)| match tool() {
        Tool::Select => {
            if !drag {
                selected.set(Some((x, y)));
            }
        }
        Tool::Erase => edit(&mut |l| l.remove_at(x, y)),
        Tool::Label => {
            if !drag {
                edit(&mut |l| {
                    l.put(Tile::Label {
                        x,
                        y,
                        text: "Label".into(),
                    })
                });
                selected.set(Some((x, y)));
                tool.set(Tool::Select);
            }
        }
        Tool::Place(shape) => {
            let block = paint_block();
            edit(&mut |l| {
                // Keep an existing turnout assignment when re-shaping a tile.
                let turnout = match l.element_at(x, y).map(|i| &l.elements[i]) {
                    Some(Tile::Track { turnout, .. }) if shape.is_turnout() => turnout.clone(),
                    _ => None,
                };
                let turnout = turnout.or_else(|| {
                    if shape.is_turnout() {
                        let id = l.next_id("T");
                        l.turnouts.push(Turnout {
                            id: id.clone(),
                            name: String::new(),
                            normal_event: None,
                            reverse_event: None,
                            feedback: None,
                        });
                        Some(id)
                    } else {
                        None
                    }
                });
                l.put(Tile::Track {
                    x,
                    y,
                    shape,
                    rot: rot(),
                    block: block.clone(),
                    turnout,
                });
            });
            if shape.is_turnout() && !drag {
                selected.set(Some((x, y)));
            }
        }
    };

    let apply = move |_| {
        if let Some(l) = draft.read().clone() {
            crate::save_layout(bus, l);
        }
        draft.set(None);
    };

    let sel_el =
        selected().and_then(|(x, y)| layout.element_at(x, y).map(|i| layout.elements[i].clone()));

    rsx! {
        div { class: "editor-view",
            div { class: "toolbar",
                div { class: "tools",
                    button { class: if tool() == Tool::Select { "on" } else { "" }, onclick: move |_| tool.set(Tool::Select), "☝ Select" }
                    for s in Shape::ALL {
                        button {
                            class: if tool() == Tool::Place(s) { "on tool" } else { "tool" },
                            title: "{s.label()}",
                            onclick: move |_| tool.set(Tool::Place(s)),
                            ShapeIcon { shape: s, rot: rot() }
                            span { "{s.label()}" }
                        }
                    }
                    button { class: if tool() == Tool::Label { "on" } else { "" }, onclick: move |_| tool.set(Tool::Label), "T Label" }
                    button { class: if tool() == Tool::Erase { "on" } else { "" }, onclick: move |_| tool.set(Tool::Erase), "⌫ Erase" }
                }
                div { class: "tools",
                    button { title: "Rotate new tiles (45°)", onclick: move |_| rot.set((rot() + 1) % 8), "⟳ Rotate ({rot() * 45}°)" }
                    label { "Paint block"
                        select {
                            onchange: move |e| paint_block.set(Some(e.value()).filter(|v| !v.is_empty())),
                            option { value: "", selected: paint_block().is_none(), "— none —" }
                            for b in layout.blocks.iter() {
                                option { value: "{b.id}", selected: paint_block() == Some(b.id.clone()), "{b.id} {b.name}" }
                            }
                        }
                    }
                }
            }

            if let Some(e) = error() {
                div { class: "error", onclick: move |_| error.set(None), "{e}  ✕" }
            }

            div { class: "editor-main",
                div { class: "editor-board",
                    TrackView {
                        layout: layout.clone(),
                        blocks: BTreeMap::new(),
                        turnouts: BTreeMap::new(),
                        mode: Mode::Edit,
                        selected: selected(),
                        on_cell: on_cell,
                    }
                }
                div { class: "inspector",
                    section {
                        h3 { "Layout" }
                        label { "Name"
                            input { value: "{layout.name}", onchange: move |e| edit(&mut |l| l.name = e.value()) }
                        }
                        div { class: "pair",
                            label { "Columns"
                                input { r#type: "number", min: "4", max: "200", value: "{layout.grid.cols}",
                                    onchange: move |e| if let Ok(v) = e.value().parse::<u16>() { edit(&mut |l| l.grid.cols = v.clamp(4, 200)) } }
                            }
                            label { "Rows"
                                input { r#type: "number", min: "4", max: "200", value: "{layout.grid.rows}",
                                    onchange: move |e| if let Ok(v) = e.value().parse::<u16>() { edit(&mut |l| l.grid.rows = v.clamp(4, 200)) } }
                            }
                        }
                        div { class: "buttons",
                            button { class: "primary", disabled: !dirty, onclick: apply, "Apply & save" }
                            button { disabled: !dirty, onclick: move |_| draft.set(None), "Revert" }
                        }
                        div { class: "buttons",
                            button { onclick: move |_| js::download(&format!("{}.json", file_stem(&layout_name(draft, bus))), &current(draft, bus).to_json()), "Export…" }
                            button {
                                onclick: move |_| async move {
                                    if let Some(text) = js::pick_file(".json,application/json").await {
                                        match Layout::from_json(&text) {
                                            Ok(l) => { draft.set(Some(l)); selected.set(None); }
                                            Err(e) => error.set(Some(format!("Could not read layout: {e}"))),
                                        }
                                    }
                                },
                                "Import…"
                            }
                            button { onclick: move |_| { draft.set(Some(Layout::demo())); selected.set(None); }, "Demo" }
                            button { onclick: move |_| if js::confirm("Start a new, empty layout?") { draft.set(Some(Layout::empty("New layout"))); selected.set(None); }, "New" }
                        }
                        if !issues.is_empty() {
                            ul { class: "issues",
                                for i in issues.iter() { li { "{i.0}" } }
                            }
                        }
                    }

                    if let Some(el) = sel_el {
                        TileInspector { el, layout: layout.clone(), selected }
                    } else {
                        section { p { class: "muted", "Choose ☝ Select and click a tile to edit it." } }
                    }
                    if !recent.is_empty() {
                        section {
                            h3 { "Recent bus events" }
                            p { class: "muted small", "Press a button or trigger a detector on the layout, then copy its event here." }
                            for e in recent.iter() {
                                code { class: "event", "{e}" }
                            }
                        }
                    }
                }
            }

            BlockTable { layout: layout.clone(), recent: recent.clone(), error }
            TurnoutTable { layout: layout.clone(), recent: recent.clone(), error }
        }
    }
}

fn current(draft: Signal<Option<Layout>>, bus: Bus) -> Layout {
    draft
        .read()
        .clone()
        .unwrap_or_else(|| bus.peek(|d| d.layout().clone()))
}

fn layout_name(draft: Signal<Option<Layout>>, bus: Bus) -> String {
    current(draft, bus).name
}

fn file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "layout".into()
    } else {
        s
    }
}

/// Apply `f` to the shared draft layout.
fn edit_draft(mut draft: Signal<Option<Layout>>, bus: Bus, f: impl FnOnce(&mut Layout)) {
    let mut l = current(draft, bus);
    f(&mut l);
    draft.set(Some(l));
}

#[component]
fn TileInspector(el: Tile, layout: Layout, selected: Signal<Option<(u16, u16)>>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let (x, y) = el.pos();
    let update = move |f: Box<dyn FnOnce(&mut Tile)>| {
        edit_draft(draft, bus, move |l| {
            if let Some(i) = l.element_at(x, y) {
                f(&mut l.elements[i]);
            }
        })
    };
    let delete = move |_| {
        edit_draft(draft, bus, |l| l.remove_at(x, y));
        selected.set(None);
    };

    match el {
        Tile::Label { text, .. } => rsx! {
            section {
                h3 { "Label at {x},{y}" }
                label { "Text"
                    input { value: "{text}", onchange: move |e| {
                        let v = e.value();
                        update(Box::new(move |t| if let Tile::Label { text, .. } = t { *text = v }));
                    } }
                }
                button { class: "danger", onclick: delete, "Delete" }
            }
        },
        Tile::Track {
            shape,
            rot,
            block,
            turnout,
            ..
        } => {
            let block_v = block.clone().unwrap_or_default();
            let turnout_v = turnout.clone().unwrap_or_default();
            rsx! {
                section {
                    h3 { "{shape.label()} at {x},{y}" }
                    label { "Shape"
                        select {
                            onchange: move |e| {
                                let v = e.value();
                                if let Some(s) = Shape::ALL.iter().copied().find(|s| format!("{s:?}") == v) {
                                    update(Box::new(move |t| if let Tile::Track { shape, .. } = t { *shape = s }));
                                }
                            },
                            for s in Shape::ALL {
                                option { value: "{s:?}", selected: s == shape, "{s.label()}" }
                            }
                        }
                    }
                    div { class: "buttons",
                        button { onclick: move |_| update(Box::new(|t| if let Tile::Track { rot, .. } = t { *rot = (*rot + 7) % 8 })), "⟲" }
                        span { class: "muted", "{rot as u16 * 45}°" }
                        button { onclick: move |_| update(Box::new(|t| if let Tile::Track { rot, .. } = t { *rot = (*rot + 1) % 8 })), "⟳" }
                    }
                    label { "Block"
                        select {
                            onchange: move |e| {
                                let v = Some(e.value()).filter(|v| !v.is_empty());
                                update(Box::new(move |t| if let Tile::Track { block, .. } = t { *block = v }));
                            },
                            option { value: "", selected: block_v.is_empty(), "— none —" }
                            for b in layout.blocks.iter() {
                                option { value: "{b.id}", selected: block_v == b.id, "{b.id} {b.name}" }
                            }
                        }
                    }
                    if shape.is_turnout() {
                        label { "Turnout"
                            select {
                                onchange: move |e| {
                                    let v = Some(e.value()).filter(|v| !v.is_empty());
                                    update(Box::new(move |t| if let Tile::Track { turnout, .. } = t { *turnout = v }));
                                },
                                option { value: "", selected: turnout_v.is_empty(), "— none —" }
                                for t in layout.turnouts.iter() {
                                    option { value: "{t.id}", selected: turnout_v == t.id, "{t.id} {t.name}" }
                                }
                            }
                        }
                    }
                    button { class: "danger", onclick: delete, "Delete tile" }
                }
            }
        }
    }
}

/// A text box for an event ID, with a button to fill in the newest event
/// seen on the bus.
#[component]
fn EventField(
    value: Option<EventId>,
    recent: Option<EventId>,
    on_set: EventHandler<Option<EventId>>,
    error: Signal<Option<String>>,
) -> Element {
    let mut error = error;
    rsx! {
        div { class: "event-field",
            input {
                class: "mono",
                placeholder: "05.01.01.01.…",
                value: "{ev_str(value)}",
                onchange: move |e| match parse_ev(&e.value()) {
                    Ok(v) => on_set.call(v),
                    Err(msg) => error.set(Some(msg)),
                },
            }
            if let Some(r) = recent {
                button { class: "learn", title: "Use last bus event {r}", onclick: move |_| on_set.call(Some(r)), "⤓" }
            }
        }
    }
}

#[component]
fn BlockTable(layout: Layout, recent: Vec<EventId>, error: Signal<Option<String>>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let last = recent.first().copied();
    let set = move |i: usize, f: Box<dyn FnOnce(&mut Block)>| {
        edit_draft(draft, bus, move |l| {
            if let Some(b) = l.blocks.get_mut(i) {
                f(b)
            }
        })
    };
    rsx! {
        section { class: "table-section",
            div { class: "table-head",
                h3 { "Blocks" }
                button { onclick: move |_| edit_draft(draft, bus, |l| {
                    let id = l.next_id("B");
                    l.blocks.push(Block { id, name: String::new(), occupied_event: None, clear_event: None });
                }), "+ Add block" }
            }
            div { class: "table-scroll",
                table {
                    thead { tr { th { "ID" } th { "Name" } th { "Occupied event" } th { "Clear event" } th {} } }
                    tbody {
                        for (i, b) in layout.blocks.iter().enumerate() {
                            tr { key: "{b.id}",
                                td { class: "mono", "{b.id}" }
                                td { input { value: "{b.name}", onchange: move |e| { let v = e.value(); set(i, Box::new(move |b| b.name = v)); } } }
                                td { EventField { value: b.occupied_event, recent: last, error, on_set: move |v| set(i, Box::new(move |b| b.occupied_event = v)) } }
                                td { EventField { value: b.clear_event, recent: last, error, on_set: move |v| set(i, Box::new(move |b| b.clear_event = v)) } }
                                td {
                                    button { class: "ghost", title: "Delete block", onclick: move |_| edit_draft(draft, bus, |l| {
                                        let id = l.blocks.remove(i).id;
                                        for el in &mut l.elements {
                                            if let Tile::Track { block, .. } = el {
                                                if block.as_deref() == Some(id.as_str()) { *block = None; }
                                            }
                                        }
                                    }), "✕" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TurnoutTable(layout: Layout, recent: Vec<EventId>, error: Signal<Option<String>>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let last = recent.first().copied();
    let set = move |i: usize, f: Box<dyn FnOnce(&mut Turnout)>| {
        edit_draft(draft, bus, move |l| {
            if let Some(t) = l.turnouts.get_mut(i) {
                f(t)
            }
        })
    };
    rsx! {
        section { class: "table-section",
            div { class: "table-head",
                h3 { "Turnouts" }
                button { onclick: move |_| edit_draft(draft, bus, |l| {
                    let id = l.next_id("T");
                    l.turnouts.push(Turnout { id, name: String::new(), normal_event: None, reverse_event: None, feedback: None });
                }), "+ Add turnout" }
            }
            p { class: "muted small", "Throw events are sent to move the points. Feedback events (optional) report the actual position from the turnout's sensors." }
            div { class: "table-scroll",
                table {
                    thead { tr { th { "ID" } th { "Name" } th { "Throw normal" } th { "Throw reverse" } th { "Feedback normal" } th { "Feedback reverse" } th {} } }
                    tbody {
                        for (i, t) in layout.turnouts.iter().enumerate() {
                            {
                                let fbn = t.feedback.as_ref().and_then(|f| f.normal_event);
                                let fbr = t.feedback.as_ref().and_then(|f| f.reverse_event);
                                rsx! {
                                    tr { key: "{t.id}",
                                        td { class: "mono", "{t.id}" }
                                        td { input { value: "{t.name}", onchange: move |e| { let v = e.value(); set(i, Box::new(move |t| t.name = v)); } } }
                                        td { EventField { value: t.normal_event, recent: last, error, on_set: move |v| set(i, Box::new(move |t| t.normal_event = v)) } }
                                        td { EventField { value: t.reverse_event, recent: last, error, on_set: move |v| set(i, Box::new(move |t| t.reverse_event = v)) } }
                                        td { EventField { value: fbn, recent: last, error, on_set: move |v| set(i, Box::new(move |t| set_feedback(t, v, true))) } }
                                        td { EventField { value: fbr, recent: last, error, on_set: move |v| set(i, Box::new(move |t| set_feedback(t, v, false))) } }
                                        td {
                                            button { class: "ghost", title: "Delete turnout", onclick: move |_| edit_draft(draft, bus, |l| {
                                                let id = l.turnouts.remove(i).id;
                                                for el in &mut l.elements {
                                                    if let Tile::Track { turnout, .. } = el {
                                                        if turnout.as_deref() == Some(id.as_str()) { *turnout = None; }
                                                    }
                                                }
                                            }), "✕" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn set_feedback(t: &mut Turnout, v: Option<EventId>, normal: bool) {
    let fb = t.feedback.get_or_insert_with(Feedback::default);
    if normal {
        fb.normal_event = v;
    } else {
        fb.reverse_event = v;
    }
    if fb.normal_event.is_none() && fb.reverse_event.is_none() {
        t.feedback = None;
    }
}

/// Small preview of a tile shape for the palette.
#[component]
fn ShapeIcon(shape: Shape, rot: u8) -> Element {
    let legs = shape.legs().rotated(rot);
    let segs: Vec<_> = legs
        .fixed
        .iter()
        .chain(&legs.normal)
        .chain(&legs.reverse)
        .copied()
        .collect();
    rsx! {
        svg { class: "shape-icon", "viewBox": "0 0 1 1",
            for s in segs {
                path { "d": "{crate::track::seg_path(0, 0, s)}" }
            }
        }
    }
}
