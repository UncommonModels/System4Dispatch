//! The editor's right-hand panels: properties of the selection, and the
//! block and turnout lists.

use std::collections::{BTreeMap, BTreeSet};

use dioxus::prelude::*;
use rolcb::EventId;
use s4d_core::layout::{Block, Feedback, Turnout, Turntable, TurntableTrack, DIR_NAMES};
use s4d_core::{Element as Tile, Layout, Shape};

use super::ops::*;
use super::{EditorState, PanelTab, Tool};
use crate::bus::Bus;
use crate::Draft;

fn pos_letter(p: s4d_core::Position) -> &'static str {
    match p {
        s4d_core::Position::Reverse => "R",
        _ => "N",
    }
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

fn color_of(colors: &BTreeMap<String, String>, id: Option<&str>) -> String {
    id.and_then(|b| colors.get(b))
        .cloned()
        .unwrap_or_else(|| "transparent".into())
}

// ---- Properties -------------------------------------------------------------

#[component]
pub fn PropertiesPanel(
    layout: Layout,
    colors: BTreeMap<String, String>,
    sel: BTreeSet<Cell>,
    issues: Vec<String>,
) -> Element {
    match sel.len() {
        0 => rsx! { LayoutProperties { layout, colors, issues } },
        1 => {
            let (x, y) = *sel.iter().next().unwrap();
            match layout.element_at(x, y) {
                Some(i) => {
                    let el = layout.elements[i].clone();
                    rsx! { TileProperties { el, layout, colors } }
                }
                None => rsx! {},
            }
        }
        _ => rsx! { SelectionProperties { sel, layout, colors } },
    }
}

#[component]
fn LayoutProperties(
    layout: Layout,
    colors: BTreeMap<String, String>,
    issues: Vec<String>,
) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let st = use_context::<EditorState>();
    let mut paint = st.paint_block;
    let (min_cols, min_rows) = used_extent(&layout);
    let painting = matches!((st.tool)(), Tool::Place(_));
    rsx! {
        div { class: "prop-group",
            h4 { "Layout" }
            label { class: "field", span { "Name" }
                input { value: "{layout.name}", onchange: move |e| edit_draft(draft, bus, |l| l.name = e.value()) }
            }
            div { class: "field-row",
                label { class: "field", span { "Columns" }
                    input { r#type: "number", min: "{min_cols.max(4)}", max: "400", value: "{layout.grid.cols}",
                        onchange: move |e| if let Ok(v) = e.value().parse::<u16>() {
                            edit_draft(draft, bus, |l| { let (c, _) = used_extent(l); l.grid.cols = v.clamp(c.max(4), 400) })
                        } }
                }
                label { class: "field", span { "Rows" }
                    input { r#type: "number", min: "{min_rows.max(4)}", max: "400", value: "{layout.grid.rows}",
                        onchange: move |e| if let Ok(v) = e.value().parse::<u16>() {
                            edit_draft(draft, bus, |l| { let (_, r) = used_extent(l); l.grid.rows = v.clamp(r.max(4), 400) })
                        } }
                }
            }
        }
        div { class: if painting { "prop-group highlight" } else { "prop-group" },
            h4 { "New track" }
            BlockSelect {
                label: "Put new tiles in",
                layout: layout.clone(), colors: colors.clone(), value: paint(), mixed: false,
                on_pick: move |v| paint.set(v),
            }
            p { class: "hint", "Pick a track piece from the left, then click or drag on the grid." }
        }
        if !issues.is_empty() {
            div { class: "prop-group",
                h4 { "Issues" }
                ul { class: "issues",
                    for i in issues.iter() { li { "{i}" } }
                }
            }
        }
        details { class: "prop-group shortcuts-box",
            summary { "Keyboard shortcuts" }
            dl { class: "shortcuts",
                dt { kbd { "V" } } dd { "Select" }
                dt { kbd { "1" } "–" kbd { "9" } } dd { "Track pieces" }
                dt { kbd { "T" } " " kbd { "E" } } dd { "Label, Erase" }
                dt { kbd { "R" } } dd { "Rotate (Shift+R back)" }
                dt { kbd { "←↑→↓" } } dd { "Move selection" }
                dt { kbd { "Del" } } dd { "Delete selection" }
                dt { kbd { "Ctrl" } kbd { "Z" } " " kbd { "Y" } } dd { "Undo, redo" }
                dt { kbd { "Ctrl" } kbd { "X" } kbd { "C" } kbd { "V" } } dd { "Cut, copy, paste" }
                dt { kbd { "Ctrl" } kbd { "D" } } dd { "Duplicate" }
                dt { kbd { "Ctrl" } kbd { "A" } } dd { "Select all" }
                dt { kbd { "Ctrl" } kbd { "S" } } dd { "Apply & save" }
                dt { kbd { "+" } " " kbd { "−" } " " kbd { "0" } } dd { "Zoom" }
                dt { kbd { "Shift" } "-click" } dd { "Add to selection" }
            }
        }
    }
}

#[component]
fn TileProperties(el: Tile, layout: Layout, colors: BTreeMap<String, String>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let st = use_context::<EditorState>();
    let (mut selection, mut tab, mut expanded) = (st.selection, st.tab, st.expanded);
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
        selection.set(BTreeSet::new());
    };

    match el {
        Tile::Label { text, .. } => rsx! {
            div { class: "prop-group",
                h4 { "Label" span { class: "muted", "  {x}, {y}" } }
                label { class: "field", span { "Text" }
                    input { value: "{text}", onchange: move |e| {
                        let v = e.value();
                        update(Box::new(move |t| if let Tile::Label { text, .. } = t { *text = v }));
                    } }
                }
                button { class: "text-btn danger", onclick: delete, "Delete label" }
            }
        },
        Tile::Track {
            shape,
            rot,
            block,
            block2,
            turnout,
            turnout2,
            turntable,
            portal,
            ..
        } => {
            let block_label: &'static str = if shape.has_two_tracks() {
                "Block (W–E track)"
            } else {
                "Block"
            };
            let turnout_v = turnout.clone().unwrap_or_default();
            let turnout2_v = turnout2.clone().unwrap_or_default();
            let portal_v = portal.clone().unwrap_or_default();
            let other_end = layout.portal_partner(x, y);
            let turntable_v = turntable.clone().unwrap_or_default();
            let table = turntable
                .as_deref()
                .and_then(|t| layout.turntable(t))
                .cloned();
            let partner = turnout.as_deref().and_then(|t| layout.linked_partner(t));
            rsx! {
                div { class: "prop-group",
                    h4 { "{shape.label()}" span { class: "muted", "  {x}, {y}" } }
                    label { class: "field", span { "Shape" }
                        select {
                            onchange: move |e| {
                                if let Some(s) = e.value().parse::<usize>().ok().and_then(|i| Shape::ALL.get(i).copied()) {
                                    edit_draft(draft, bus, |l| set_tile_shape(l, x, y, s));
                                }
                            },
                            for (i, s) in Shape::ALL.into_iter().enumerate() {
                                option { value: "{i}", selected: s == shape, "{s.label()}" }
                            }
                        }
                    }
                    if !shape.is_turntable() {
                    div { class: "field", span { "Rotation" }
                        div { class: "stepper",
                            button { class: "icon-btn", title: "Rotate left (Shift+R)", onclick: move |_| update(Box::new(|t| if let Tile::Track { rot, .. } = t { *rot = (*rot % 8 + 7) % 8 })), "⟲" }
                            span { "{(rot % 8) as u16 * 45}°" }
                            button { class: "icon-btn", title: "Rotate right (R)", onclick: move |_| update(Box::new(|t| if let Tile::Track { rot, .. } = t { *rot = (*rot % 8 + 1) % 8 })), "⟳" }
                        }
                    }
                    }
                    BlockSelect {
                        label: block_label,
                        layout: layout.clone(), colors: colors.clone(), value: block, mixed: false,
                        on_pick: move |v: Option<String>| update(Box::new(move |t| if let Tile::Track { block, .. } = t { *block = v })),
                    }
                    if shape.has_two_tracks() {
                        BlockSelect {
                            label: "Block (other track)",
                            layout: layout.clone(), colors: colors.clone(), value: block2.clone(), mixed: false,
                            on_pick: move |v: Option<String>| update(Box::new(move |t| if let Tile::Track { block2, .. } = t { *block2 = v })),
                        }
                        p { class: "hint", "The two tracks through a crossing can be in different blocks. Leave the other track on “No block” to use the same block for both." }
                    }
                    if shape.is_turnout() {
                        label { class: "field", span { if shape.has_two_turnouts() { "Left-hand turnout" } else { "Turnout" } }
                            select {
                                onchange: move |e| {
                                    let v = Some(e.value()).filter(|v| !v.is_empty());
                                    update(Box::new(move |t| if let Tile::Track { turnout, .. } = t { *turnout = v }));
                                },
                                option { value: "", selected: turnout_v.is_empty(), "— none —" }
                                for t in layout.turnouts.iter() {
                                    option { value: "{t.id}", selected: turnout_v == t.id, "{t.id}  {t.name}" }
                                }
                            }
                        }
                        if let Some((p, inv)) = partner {
                            p { class: "hint", "Thrown together with {p}" if inv { " (opposite position)" } "." }
                        }
                        if shape.has_two_turnouts() {
                            label { class: "field", span { "Right-hand turnout" }
                                select {
                                    onchange: move |e| {
                                        let v = Some(e.value()).filter(|v| !v.is_empty());
                                        update(Box::new(move |t| if let Tile::Track { turnout2, .. } = t { *turnout2 = v }));
                                    },
                                    option { value: "", selected: turnout2_v.is_empty(), "— none —" }
                                    for t in layout.turnouts.iter() {
                                        option { value: "{t.id}", selected: turnout2_v == t.id, "{t.id}  {t.name}" }
                                    }
                                }
                            }
                            p { class: "hint",
                                "Routes (left-hand, right-hand turnout): "
                                {shape.routes().iter().map(|((a, b), (p, q))| format!("{}/{} sets {}–{}", pos_letter(*a), pos_letter(*b), DIR_NAMES[*p as usize], DIR_NAMES[*q as usize])).collect::<Vec<_>>().join(", ")}
                                ". Directions are before rotation. Clicking it on the panel steps through them in this order."
                            }
                        }
                        if let Some(t) = turnout.clone() {
                            button { class: "text-btn", onclick: move |_| { expanded.set(Some(t.clone())); tab.set(PanelTab::Turnouts); }, "Edit turnout events →" }
                        }
                    }
                    if shape.is_portal() {
                        label { class: "field", span { "Portal name" }
                            input { value: "{portal_v}", placeholder: "e.g. A",
                                onchange: move |e| {
                                    let v = Some(e.value().trim().to_string()).filter(|v| !v.is_empty());
                                    update(Box::new(move |t| if let Tile::Track { portal, .. } = t { *portal = v }));
                                } }
                        }
                        match other_end {
                            Some((ox, oy)) => rsx! {
                                p { class: "hint", "The track carries on at the other “{portal_v}” portal, at {ox}, {oy}." }
                                button { class: "text-btn", onclick: move |_| selection.set(BTreeSet::from([(ox, oy)])), "Select the other end" }
                            },
                            None => rsx! {
                                p { class: "hint", "Place another portal (or give one the same name) where this track comes back onto the panel." }
                            },
                        }
                    }
                    if shape.is_turntable() {
                        label { class: "field", span { "Turntable" }
                            select {
                                onchange: move |e| {
                                    let v = Some(e.value()).filter(|v| !v.is_empty());
                                    update(Box::new(move |t| if let Tile::Track { turntable, .. } = t { *turntable = v }));
                                },
                                option { value: "", selected: turntable_v.is_empty(), "— none —" }
                                for t in layout.turntables.iter() {
                                    option { value: "{t.id}", selected: turntable_v == t.id, "{t.id}  {t.name}" }
                                }
                            }
                        }
                        if let Some(t) = table {
                            p { class: "hint",
                                "{t.tracks.iter().filter(|tr| !tr.stall).count()} leads and {t.tracks.iter().filter(|tr| tr.stall).count()} stalls. R turns the whole turntable."
                            }
                            {
                                let (id1, id2) = (t.id.clone(), t.id.clone());
                                rsx! {
                                    button { class: "text-btn", onclick: move |_| { let id = id1.clone(); edit_draft(draft, bus, move |l| { l.add_connected_leads(&id); }); }, "Add leads for connected track" }
                                    button { class: "text-btn", onclick: move |_| { expanded.set(Some(id2.clone())); tab.set(PanelTab::Turntables); }, "Edit tracks, stalls and events →" }
                                }
                            }
                        }
                    }
                    button { class: "text-btn danger", onclick: delete, "Delete tile" }
                }
            }
        }
    }
}

#[component]
fn SelectionProperties(
    sel: BTreeSet<Cell>,
    layout: Layout,
    colors: BTreeMap<String, String>,
) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let mut selection = use_context::<EditorState>().selection;
    let blocks: BTreeSet<Option<String>> = layout
        .elements
        .iter()
        .filter(|e| sel.contains(&e.pos()))
        .filter_map(|e| match e {
            Tile::Track { block, .. } => Some(block.clone()),
            _ => None,
        })
        .collect();
    let n_track = blocks.len();
    let mixed = blocks.len() > 1;
    let common = if mixed {
        None
    } else {
        blocks.into_iter().next().flatten()
    };
    let n = sel.len();
    let (s1, s2, s3, s4) = (sel.clone(), sel.clone(), sel.clone(), sel.clone());
    rsx! {
        div { class: "prop-group",
            h4 { "{n} tiles selected" }
            if n_track > 0 {
                BlockSelect {
                    label: "Block",
                    layout: layout.clone(), colors, value: common, mixed,
                    on_pick: move |v: Option<String>| {
                        let sel = s1.clone();
                        edit_draft(draft, bus, move |l| assign_block(l, &sel, v.as_deref()))
                    },
                }
                div { class: "field", span { "Rotate each" }
                    div { class: "stepper",
                        button { class: "icon-btn", onclick: move |_| { let sel = s2.clone(); edit_draft(draft, bus, move |l| rotate_tiles(l, &sel, 7)) }, "⟲" }
                        button { class: "icon-btn", onclick: move |_| { let sel = s3.clone(); edit_draft(draft, bus, move |l| rotate_tiles(l, &sel, 1)) }, "⟳" }
                    }
                }
            }
            p { class: "hint", "Arrow keys move the selection." }
            div { class: "row-actions",
                button { class: "text-btn", onclick: move |_| selection.set(BTreeSet::new()), "Deselect" }
                button { class: "text-btn danger", onclick: move |_| {
                    let sel = s4.clone();
                    edit_draft(draft, bus, move |l| l.elements.retain(|el| !sel.contains(&el.pos())));
                    selection.set(BTreeSet::new());
                }, "Delete {n} tiles" }
            }
        }
    }
}

/// Block picker with a colour swatch.
#[component]
fn BlockSelect(
    label: &'static str,
    layout: Layout,
    colors: BTreeMap<String, String>,
    value: Option<String>,
    mixed: bool,
    on_pick: EventHandler<Option<String>>,
) -> Element {
    let color = if mixed {
        "repeating-linear-gradient(45deg, var(--muted) 0 3px, transparent 3px 6px)".to_string()
    } else {
        color_of(&colors, value.as_deref())
    };
    let v = value.clone().unwrap_or_default();
    rsx! {
        label { class: "field", span { "{label}" }
            div { class: "with-swatch",
                i { class: "swatch", style: "background:{color}" }
                select {
                    onchange: move |e| on_pick.call(Some(e.value()).filter(|v| !v.is_empty() && v != "\u{0}")),
                    if mixed { option { value: "\u{0}", selected: true, disabled: true, "Mixed" } }
                    option { value: "", selected: !mixed && v.is_empty(), "No block" }
                    for b in layout.blocks.iter() {
                        option { value: "{b.id}", selected: !mixed && v == b.id, "{b.id}  {b.name}" }
                    }
                }
            }
        }
    }
}

/// A labelled event ID field, with a button that fills in the newest event
/// seen on the bus.
#[component]
fn EventField(
    label: &'static str,
    value: Option<EventId>,
    recent: Option<EventId>,
    on_set: EventHandler<Option<EventId>>,
) -> Element {
    let mut error = use_context::<EditorState>().error;
    rsx! {
        label { class: "field", span { "{label}" }
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
                    button { class: "icon-btn", title: "Use the last event seen on the bus ({r})", onclick: move |e| { e.prevent_default(); on_set.call(Some(r)) }, "⤓" }
                }
            }
        }
    }
}

// ---- Blocks -----------------------------------------------------------------

#[component]
pub fn BlocksPanel(
    layout: Layout,
    colors: BTreeMap<String, String>,
    recent: Vec<EventId>,
) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let st = use_context::<EditorState>();
    let (mut expanded, mut focus) = (st.expanded, st.focus_block);
    let last = recent.first().copied();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for el in &layout.elements {
        if let Tile::Track { block, block2, .. } = el {
            for b in block.iter().chain(block2) {
                *counts.entry(b.as_str()).or_default() += 1;
            }
        }
    }
    let unassigned = layout
        .elements
        .iter()
        .filter(|e| matches!(e, Tile::Track { block: None, .. }))
        .count();
    let set = move |id: String, f: Box<dyn FnOnce(&mut Block)>| {
        edit_draft(draft, bus, move |l| {
            if let Some(b) = l.blocks.iter_mut().find(|b| b.id == id) {
                f(b)
            }
        })
    };
    rsx! {
        div { class: "list-head",
            span { class: "muted", "{layout.blocks.len()} blocks" }
            button { class: "text-btn", onclick: move |_| {
                let mut new_id = String::new();
                edit_draft(draft, bus, |l| {
                    new_id = l.next_id("B");
                    l.blocks.push(Block { id: new_id.clone(), ..Default::default() });
                });
                expanded.set(Some(new_id));
            }, "+ New block" }
        }
        if unassigned > 0 {
            p { class: "hint", "{unassigned} track tiles aren't in any block (drawn thin and grey)." }
        }
        if focus().is_some() {
            button { class: "text-btn", onclick: move |_| focus.set(None), "Show all blocks" }
        }
        ul { class: "item-list",
            for b in layout.blocks.iter() {
                {
                    let id = b.id.clone();
                    let open = expanded() == Some(b.id.clone());
                    let focused = focus() == Some(b.id.clone());
                    let count = counts.get(b.id.as_str()).copied().unwrap_or(0);
                    let color = color_of(&colors, Some(&b.id));
                    let (i1, i2, i3, i4, i5, i6) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                    rsx! {
                        li { key: "{b.id}", class: if open { "item open" } else { "item" },
                            div { class: "item-row", onclick: move |_| expanded.set(if open { None } else { Some(i1.clone()) }),
                                button {
                                    class: if focused { "swatch-btn on" } else { "swatch-btn" },
                                    title: "Highlight this block on the drawing",
                                    style: "background:{color}",
                                    onclick: move |e| { e.stop_propagation(); focus.set(if focused { None } else { Some(i2.clone()) }) },
                                }
                                span { class: "item-name", if b.name.is_empty() { "{b.id}" } else { "{b.name}" } }
                                span { class: "item-meta", "{b.id} · {count}" }
                                span { class: "chevron", if open { "▾" } else { "▸" } }
                            }
                            if open {
                                div { class: "item-detail",
                                    label { class: "field", span { "Name" }
                                        input { value: "{b.name}", placeholder: "e.g. West Main", onchange: move |e| { let v = e.value(); set(i3.clone(), Box::new(move |b| b.name = v)) } }
                                    }
                                    EventField { label: "Occupied event", value: b.occupied_event, recent: last, on_set: move |v| set(i4.clone(), Box::new(move |b| b.occupied_event = v)) }
                                    EventField { label: "Clear event", value: b.clear_event, recent: last, on_set: move |v| set(i5.clone(), Box::new(move |b| b.clear_event = v)) }
                                    div { class: "row-actions",
                                        SelectTilesButton { block: Some(id.clone()), turnout: None, count }
                                        button { class: "text-btn danger", onclick: move |_| { let id = i6.clone(); edit_draft(draft, bus, |l| remove_block(l, &id)); }, "Delete" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        RecentEvents { recent }
    }
}

#[component]
fn SelectTilesButton(
    block: Option<String>,
    turnout: Option<String>,
    #[props(default)] turntable: Option<String>,
    count: usize,
) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let st = use_context::<EditorState>();
    let (mut selection, mut tool) = (st.selection, st.tool);
    rsx! {
        button {
            class: "text-btn",
            disabled: count == 0,
            onclick: move |_| {
                let l = current(draft, bus);
                let cells = l.elements.iter().filter_map(|e| match e {
                    Tile::Track { x, y, block: b, block2: b2, turnout: t, turntable: tt, .. }
                        if (block.is_some() && (*b == block || *b2 == block))
                            || (turnout.is_some() && *t == turnout)
                            || (turntable.is_some() && *tt == turntable) => Some((*x, *y)),
                    _ => None,
                }).collect();
                selection.set(cells);
                tool.set(Tool::Select);
            },
            if count == 1 { "Select tile" } else { "Select {count} tiles" }
        }
    }
}

#[component]
fn RecentEvents(recent: Vec<EventId>) -> Element {
    if recent.is_empty() {
        return rsx! {};
    }
    rsx! {
        details { class: "prop-group recent",
            summary { "Recent bus events" }
            p { class: "hint", "Press a button or trigger a detector on the layout; its event appears here, and ⤓ fills it into a field." }
            for e in recent.iter() {
                code { class: "event", "{e}" }
            }
        }
    }
}

// ---- Turnouts ---------------------------------------------------------------

#[component]
pub fn TurnoutsPanel(layout: Layout, recent: Vec<EventId>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let mut expanded = use_context::<EditorState>().expanded;
    let last = recent.first().copied();
    let set = move |id: String, f: Box<dyn FnOnce(&mut Turnout)>| {
        edit_draft(draft, bus, move |l| {
            if let Some(t) = l.turnouts.iter_mut().find(|t| t.id == id) {
                f(t)
            }
        })
    };
    let mut tiles: BTreeMap<&str, usize> = BTreeMap::new();
    for el in &layout.elements {
        if let Tile::Track {
            turnout: Some(t), ..
        } = el
        {
            *tiles.entry(t.as_str()).or_default() += 1;
        }
    }
    rsx! {
        div { class: "list-head",
            span { class: "muted", "{layout.turnouts.len()} turnouts" }
            button { class: "text-btn", onclick: move |_| {
                let mut new_id = String::new();
                edit_draft(draft, bus, |l| {
                    new_id = l.next_id("T");
                    l.turnouts.push(Turnout { id: new_id.clone(), ..Default::default() });
                });
                expanded.set(Some(new_id));
            }, "+ New turnout" }
        }
        ul { class: "item-list",
            for t in layout.turnouts.iter() {
                {
                    let open = expanded() == Some(t.id.clone());
                    let fbn = t.feedback.as_ref().and_then(|f| f.normal_event);
                    let fbr = t.feedback.as_ref().and_then(|f| f.reverse_event);
                    let partner = layout.linked_partner(&t.id);
                    let (pid, inv) = partner.clone().unwrap_or_default();
                    let count = tiles.get(t.id.as_str()).copied().unwrap_or(0);
                    let configured = t.normal_event.is_some() && t.reverse_event.is_some();
                    let id = t.id.clone();
                    let (i1, i2, i3, i4, i5, i6, i7, i8, i9) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                    let pid2 = pid.clone();
                    rsx! {
                        li { key: "{t.id}", class: if open { "item open" } else { "item" },
                            div { class: "item-row", onclick: move |_| expanded.set(if open { None } else { Some(i1.clone()) }),
                                span { class: if configured { "dot ok" } else { "dot" }, title: if configured { "Throw events set" } else { "Throw events missing" } }
                                span { class: "item-name", if t.name.is_empty() { "{t.id}" } else { "{t.name}" } }
                                if partner.is_some() { span { class: "link-badge", "⇄ {pid}" } }
                                span { class: "item-meta", "{t.id}" }
                                span { class: "chevron", if open { "▾" } else { "▸" } }
                            }
                            if open {
                                div { class: "item-detail",
                                    label { class: "field", span { "Name" }
                                        input { value: "{t.name}", placeholder: "e.g. Hilltown West", onchange: move |e| { let v = e.value(); set(i2.clone(), Box::new(move |t| t.name = v)) } }
                                    }
                                    div { class: "field-caption", "Throw" }
                                    EventField { label: "Normal", value: t.normal_event, recent: last, on_set: move |v| set(i3.clone(), Box::new(move |t| t.normal_event = v)) }
                                    EventField { label: "Reverse", value: t.reverse_event, recent: last, on_set: move |v| set(i4.clone(), Box::new(move |t| t.reverse_event = v)) }
                                    div { class: "field-caption", "Feedback (optional)" }
                                    EventField { label: "Normal", value: fbn, recent: last, on_set: move |v| set(i5.clone(), Box::new(move |t| set_feedback(t, v, true))) }
                                    EventField { label: "Reverse", value: fbr, recent: last, on_set: move |v| set(i6.clone(), Box::new(move |t| set_feedback(t, v, false))) }
                                    label { class: "field", span { "Thrown together with" }
                                        select {
                                            onchange: move |e| {
                                                let v = e.value();
                                                let me = i7.clone();
                                                edit_draft(draft, bus, move |l| l.set_link(&me, Some(v.as_str()).filter(|v| !v.is_empty()), inv));
                                            },
                                            option { value: "", selected: pid.is_empty(), "Nothing" }
                                            for o in layout.turnouts.iter().filter(|o| o.id != t.id) {
                                                option { value: "{o.id}", selected: pid == o.id, "{o.id}  {o.name}" }
                                            }
                                        }
                                    }
                                    if partner.is_some() {
                                        label { class: "check",
                                            input { r#type: "checkbox", checked: inv, onchange: move |e| {
                                                let (me, p, on) = (i8.clone(), pid2.clone(), e.checked());
                                                edit_draft(draft, bus, move |l| l.set_link(&me, Some(p.as_str()), on));
                                            } }
                                            "Throw it to the opposite position"
                                        }
                                    }
                                    div { class: "row-actions",
                                        SelectTilesButton { block: None, turnout: Some(id.clone()), count }
                                        button { class: "text-btn danger", onclick: move |_| { let id = i9.clone(); edit_draft(draft, bus, |l| remove_turnout(l, &id)); }, "Delete" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        RecentEvents { recent }
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

// ---- Turntables -------------------------------------------------------------

#[component]
pub fn TurntablesPanel(layout: Layout, recent: Vec<EventId>) -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let st = use_context::<EditorState>();
    let (mut expanded, mut tool) = (st.expanded, st.tool);
    let last = recent.first().copied();
    let set = move |id: String, f: Box<dyn FnOnce(&mut Turntable)>| {
        edit_draft(draft, bus, move |l| {
            if let Some(t) = l.turntables.iter_mut().find(|t| t.id == id) {
                f(t)
            }
        })
    };
    // Edit track `i`; re-sorts afterwards in case the angle changed.
    let set_track = move |id: String, i: usize, f: Box<dyn FnOnce(&mut TurntableTrack)>| {
        set(
            id,
            Box::new(move |t| {
                if let Some(tr) = t.tracks.get_mut(i) {
                    f(tr)
                }
                t.sort_tracks();
            }),
        )
    };
    let mut tiles: BTreeMap<&str, usize> = BTreeMap::new();
    for el in &layout.elements {
        if let Tile::Track {
            turntable: Some(t), ..
        } = el
        {
            *tiles.entry(t.as_str()).or_default() += 1;
        }
    }
    rsx! {
        div { class: "list-head",
            span { class: "muted", "{layout.turntables.len()} turntables" }
            button { class: "text-btn", onclick: move |_| tool.set(Tool::Roundhouse), "+ Turntable & roundhouse" }
        }
        if layout.turntables.is_empty() {
            p { class: "hint", "Pick the turntable & roundhouse tool and click where the pit should go, or place a turntable piece at the end of existing track." }
        }
        ul { class: "item-list",
            for t in layout.turntables.iter() {
                {
                    let open = expanded() == Some(t.id.clone());
                    let configured = t.cw_event.is_some() && t.ccw_event.is_some();
                    let count = tiles.get(t.id.as_str()).copied().unwrap_or(0);
                    let stalls = t.tracks.iter().filter(|tr| tr.stall).count();
                    let free_dirs: Vec<u8> = (0..8u8)
                        .filter(|d| t.tracks.iter().all(|tr| tr.angle != *d as u16 * 45))
                        .collect();
                    let id = t.id.clone();
                    let (i1, i2, i3, i4, i5, i6, i7, i8, i9) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                    rsx! {
                        li { key: "{t.id}", class: if open { "item open" } else { "item" },
                            div { class: "item-row", onclick: move |_| expanded.set(if open { None } else { Some(i1.clone()) }),
                                span { class: if configured { "dot ok" } else { "dot" }, title: if configured { "Turn events set" } else { "Turn events missing" } }
                                span { class: "item-name", "{t.display_name()}" }
                                span { class: "item-meta", "{t.id} · {stalls} stalls" }
                                span { class: "chevron", if open { "▾" } else { "▸" } }
                            }
                            if open {
                                div { class: "item-detail",
                                    label { class: "field", span { "Name" }
                                        input { value: "{t.name}", placeholder: "e.g. Engine Terminal", onchange: move |e| { let v = e.value(); set(i2.clone(), Box::new(move |t| t.name = v)) } }
                                    }
                                    label { class: "field", span { "Pit size (cells)" }
                                        input { r#type: "number", min: "1", max: "8", value: "{t.size}",
                                            onchange: move |e| if let Ok(v) = e.value().parse::<u8>() { set(i3.clone(), Box::new(move |t| t.size = v.clamp(1, 8))) } }
                                    }
                                    div { class: "field-caption", "Turn" }
                                    EventField { label: "Clockwise", value: t.cw_event, recent: last, on_set: move |v| set(i4.clone(), Box::new(move |t| t.cw_event = v)) }
                                    EventField { label: "Counter-clockwise", value: t.ccw_event, recent: last, on_set: move |v| set(i5.clone(), Box::new(move |t| t.ccw_event = v)) }
                                    p { class: "hint", "Each event turns the bridge one track on, like throwing a turnout. Tracks are stops in clockwise order from north." }
                                    div { class: "field-caption", "Tracks" }
                                    for (i, tr) in t.tracks.iter().enumerate() {
                                        {
                                            let (a, b, c, d, f) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
                                            rsx! {
                                                div { class: "tt-track",
                                                    input { class: "tt-angle", r#type: "number", min: "0", max: "359", value: "{tr.angle}", title: "Angle in degrees, clockwise from north",
                                                        onchange: move |e| if let Ok(v) = e.value().parse::<u16>() { set_track(a.clone(), i, Box::new(move |tr| tr.angle = v % 360)) } }
                                                    span { class: "muted", "°" }
                                                    input { class: "tt-name", value: "{tr.name}", placeholder: if tr.stall { "Stall" } else { "Lead" },
                                                        onchange: move |e| { let v = e.value(); set_track(b.clone(), i, Box::new(move |tr| tr.name = v)) } }
                                                    label { class: "check", title: "Drawn inside the roundhouse",
                                                        input { r#type: "checkbox", checked: tr.stall, onchange: move |e| { let on = e.checked(); set_track(c.clone(), i, Box::new(move |tr| tr.stall = on)) } }
                                                        "Stall"
                                                    }
                                                    button { class: "icon-btn", title: "Remove this track", onclick: move |_| set(d.clone(), Box::new(move |t| { t.tracks.remove(i); })), "✕" }
                                                }
                                                EventField { label: "Arrived (optional)", value: tr.feedback_event, recent: last, on_set: move |v| set_track(f.clone(), i, Box::new(move |tr| tr.feedback_event = v)) }
                                            }
                                        }
                                    }
                                    div { class: "row-actions",
                                        button { class: "text-btn", onclick: move |_| set(i6.clone(), Box::new(|t| {
                                            let a = t.free_stall_angle();
                                            let n = t.tracks.iter().filter(|tr| tr.stall).count() + 1;
                                            t.add_track(a, &format!("Stall {n}"), true);
                                        })), "+ Stall" }
                                        if !free_dirs.is_empty() {
                                            select {
                                                onchange: move |e| if let Ok(d) = e.value().parse::<u16>() {
                                                    set(i7.clone(), Box::new(move |t| { t.add_track(d * 45, "Lead", false); }))
                                                },
                                                option { value: "", selected: true, disabled: true, "+ Lead…" }
                                                for d in free_dirs.iter() {
                                                    option { value: "{d}", "{DIR_NAMES[*d as usize]}" }
                                                }
                                            }
                                        }
                                        button { class: "text-btn", title: "Add a lead for every track that runs into the pit", onclick: move |_| { let id = i8.clone(); edit_draft(draft, bus, move |l| { l.add_connected_leads(&id); }); }, "Connected leads" }
                                    }
                                    div { class: "row-actions",
                                        SelectTilesButton { block: None, turnout: None, turntable: Some(id.clone()), count }
                                        button { class: "text-btn danger", onclick: move |_| { let id = i9.clone(); edit_draft(draft, bus, |l| { l.remove_turntable(&id); }); }, "Delete" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        RecentEvents { recent }
    }
}

/// Small preview of a tile shape, for the tool rail.
#[component]
pub fn ShapeIcon(shape: Shape, rot: u8) -> Element {
    if shape == Shape::Turntable {
        return rsx! {
            svg { class: "shape-icon", "viewBox": "-0.1 -0.1 1.2 1.2",
                path { class: "icon-fill", "d": "{crate::track::circle_path(0.5, 0.5, 0.42)}" }
                path { "d": "M0.18 0.5 L0.82 0.5" }
            }
        };
    }
    if shape == Shape::Portal {
        let (track, arrow) = crate::track::portal_paths(0, 0, rot);
        return rsx! {
            svg { class: "shape-icon", "viewBox": "-0.1 -0.1 1.2 1.2",
                path { "d": "{track}" }
                path { class: "icon-thin", "d": "{arrow}" }
            }
        };
    }
    let legs = shape.legs().rotated(rot);
    let segs: Vec<_> = legs
        .fixed
        .iter()
        .chain(&legs.normal)
        .chain(&legs.reverse)
        .copied()
        .collect();
    rsx! {
        svg { class: "shape-icon", "viewBox": "-0.1 -0.1 1.2 1.2",
            for s in segs {
                path { "d": "{crate::track::leg_path(shape, 0, 0, s)}" }
            }
        }
    }
}
