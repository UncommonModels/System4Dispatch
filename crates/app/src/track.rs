//! SVG rendering of the track grid, shared by the dispatch panel and editor.
//! Coordinates are in tile units: tile (x, y) spans x..x+1, y..y+1.

use std::collections::BTreeMap;

use dioxus::prelude::*;
use s4d_core::layout::{dir_offset, Seg, CENTER};
use s4d_core::{Element as Tile, Layout, Occupancy, Position, TurnoutStatus};

fn pt(x: u16, y: u16, dir: u8) -> (f32, f32) {
    let (dx, dy) = dir_offset(dir);
    (x as f32 + 0.5 + dx, y as f32 + 0.5 + dy)
}

/// SVG path data for one segment of tile (x, y).
pub fn seg_path(x: u16, y: u16, (a, b): Seg) -> String {
    let (ax, ay) = pt(x, y, a);
    let (bx, by) = pt(x, y, b);
    let (cx, cy) = pt(x, y, CENTER);
    if a == CENTER || b == CENTER {
        // Bumper: line to the centre plus an end cap across it.
        let (ex, ey) = if a == CENTER { (bx, by) } else { (ax, ay) };
        let (vx, vy) = (cx - ex, cy - ey);
        let len = (vx * vx + vy * vy).sqrt().max(1e-6);
        let (px, py) = (-vy / len * 0.22, vx / len * 0.22);
        format!(
            "M{ex:.3} {ey:.3} L{cx:.3} {cy:.3} M{:.3} {:.3} L{:.3} {:.3}",
            cx + px,
            cy + py,
            cx - px,
            cy - py
        )
    } else if (a + 4) % 8 == b {
        format!("M{ax:.3} {ay:.3} L{bx:.3} {by:.3}")
    } else {
        format!("M{ax:.3} {ay:.3} Q{cx:.3} {cy:.3} {bx:.3} {by:.3}")
    }
}

fn occ_class(block: Option<&String>, blocks: &BTreeMap<String, Occupancy>) -> &'static str {
    match block.and_then(|b| blocks.get(b)) {
        None => "occ-none",
        Some(Occupancy::Unknown) => "occ-unknown",
        Some(Occupancy::Clear) => "occ-clear",
        Some(Occupancy::Occupied) => "occ-occupied",
    }
}

pub struct Drawn {
    pub d: String,
    pub class: String,
}

/// All the strokes for one tile, back to front.
pub fn tile_strokes(
    el: &Tile,
    blocks: &BTreeMap<String, Occupancy>,
    turnouts: &BTreeMap<String, TurnoutStatus>,
) -> Vec<Drawn> {
    let Tile::Track {
        x,
        y,
        shape,
        rot,
        block,
        turnout,
    } = el
    else {
        return Vec::new();
    };
    let legs = shape.legs().rotated(*rot);
    let occ = occ_class(block.as_ref(), blocks);
    let st = turnout
        .as_ref()
        .and_then(|t| turnouts.get(t))
        .copied()
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut push = |segs: &[Seg], class: String| {
        for s in segs {
            out.push(Drawn {
                d: seg_path(*x, *y, *s),
                class: class.clone(),
            });
        }
    };
    // Unselected leg first so the selected route draws on top.
    let (normal_cls, reverse_cls) = match st.position {
        Position::Normal => (format!("track {occ}"), "track leg-off".to_string()),
        Position::Reverse => ("track leg-off".to_string(), format!("track {occ}")),
        Position::Unknown => (
            format!("track {occ} leg-unknown"),
            format!("track {occ} leg-unknown"),
        ),
    };
    let pending = |p: Position| {
        if st.pending == Some(p) {
            " pending"
        } else {
            ""
        }
    };
    if st.position == Position::Reverse {
        push(&legs.normal, normal_cls + pending(Position::Normal));
        push(&legs.reverse, reverse_cls + pending(Position::Reverse));
    } else {
        push(&legs.reverse, reverse_cls + pending(Position::Reverse));
        push(&legs.normal, normal_cls + pending(Position::Normal));
    }
    push(&legs.fixed, format!("track {occ}"));
    out
}

/// Interaction mode for [`TrackView`].
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Dispatch,
    Edit,
}

#[derive(Props, Clone, PartialEq)]
pub struct TrackViewProps {
    pub layout: Layout,
    pub blocks: BTreeMap<String, Occupancy>,
    pub turnouts: BTreeMap<String, TurnoutStatus>,
    pub mode: Mode,
    #[props(default)]
    pub selected: Option<(u16, u16)>,
    /// Edit mode: a cell was pressed (`false`) or dragged over (`true`).
    #[props(default)]
    pub on_cell: Option<EventHandler<(u16, u16, bool)>>,
    /// Dispatch mode: a turnout tile was clicked.
    #[props(default)]
    pub on_turnout: Option<EventHandler<String>>,
}

#[component]
pub fn TrackView(props: TrackViewProps) -> Element {
    let mut painting = use_signal(|| false);
    let cols = props.layout.grid.cols;
    let rows = props.layout.grid.rows;
    let edit = props.mode == Mode::Edit;
    let view_box = format!("-0.25 -0.25 {} {}", cols as f32 + 0.5, rows as f32 + 0.5);
    let names: BTreeMap<String, String> = props
        .layout
        .turnouts
        .iter()
        .map(|t| {
            (
                t.id.clone(),
                if t.name.is_empty() {
                    t.id.clone()
                } else {
                    format!("{} ({})", t.name, t.id)
                },
            )
        })
        .collect();

    rsx! {
        svg {
            class: if edit { "track-svg editing" } else { "track-svg" },
            "viewBox": "{view_box}",
            "preserveAspectRatio": "xMidYMid meet",
            onpointerup: move |_| painting.set(false),
            onpointerleave: move |_| painting.set(false),
            if edit {
                for gx in 0..=cols {
                    line { class: "grid-line", "x1": "{gx}", "y1": "0", "x2": "{gx}", "y2": "{rows}" }
                }
                for gy in 0..=rows {
                    line { class: "grid-line", "x1": "0", "y1": "{gy}", "x2": "{cols}", "y2": "{gy}" }
                }
            }
            for el in props.layout.elements.iter() {
                match el {
                    Tile::Label { x, y, text } => rsx! {
                        text { class: "label", "x": "{*x as f32 + 0.1}", "y": "{*y as f32 + 0.72}", "{text}" }
                    },
                    Tile::Track { .. } => rsx! {
                        for s in tile_strokes(el, &props.blocks, &props.turnouts) {
                            path { class: "{s.class}", "d": "{s.d}" }
                        }
                    },
                }
            }
            if let Some((sx, sy)) = props.selected {
                rect { class: "sel", "x": "{sx}", "y": "{sy}", "width": "1", "height": "1" }
            }
            if !edit {
                for el in props.layout.elements.iter() {
                    if let Tile::Track { x, y, turnout: Some(t), .. } = el {
                        {
                            let t = t.clone();
                            let title = names.get(&t).cloned().unwrap_or_else(|| t.clone());
                            let handler = props.on_turnout;
                            rsx! {
                                rect {
                                    class: "hit",
                                    "x": "{x}", "y": "{y}", "width": "1", "height": "1",
                                    onclick: move |_| if let Some(h) = handler { h.call(t.clone()) },
                                    title { "{title}" }
                                }
                            }
                        }
                    }
                }
            }
            if edit {
                for cy in 0..rows {
                    for cx in 0..cols {
                        {
                            let handler = props.on_cell;
                            rsx! {
                                rect {
                                    class: "cell",
                                    "x": "{cx}", "y": "{cy}", "width": "1", "height": "1",
                                    onpointerdown: move |e| {
                                        e.prevent_default();
                                        painting.set(true);
                                        if let Some(h) = handler { h.call((cx, cy, false)) }
                                    },
                                    onpointerenter: move |_| {
                                        if painting() {
                                            if let Some(h) = handler { h.call((cx, cy, true)) }
                                        }
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
