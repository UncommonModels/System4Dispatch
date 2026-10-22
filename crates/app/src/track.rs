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
    pub style: String,
}

/// Distinct colours for identifying blocks in the editor (golden-angle hues).
pub fn block_colors(layout: &Layout) -> BTreeMap<String, String> {
    layout
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                b.id.clone(),
                format!("hsl({:.0} 72% 52%)", (i as f32 * 137.508 + 20.0) % 360.0),
            )
        })
        .collect()
}

/// Where to put a block's name tag: the block's tile nearest its centroid.
pub fn block_label_positions(layout: &Layout) -> Vec<(String, u16, u16)> {
    let mut by_block: BTreeMap<&str, Vec<(u16, u16)>> = BTreeMap::new();
    for el in &layout.elements {
        if let Tile::Track {
            x,
            y,
            block: Some(b),
            ..
        } = el
        {
            by_block.entry(b.as_str()).or_default().push((*x, *y));
        }
    }
    by_block
        .into_iter()
        .map(|(b, tiles)| {
            let n = tiles.len() as f32;
            let cx = tiles.iter().map(|t| t.0 as f32).sum::<f32>() / n;
            let cy = tiles.iter().map(|t| t.1 as f32).sum::<f32>() / n;
            let best = tiles
                .iter()
                .min_by(|a, b| {
                    let da = (a.0 as f32 - cx).powi(2) + (a.1 as f32 - cy).powi(2);
                    let db = (b.0 as f32 - cx).powi(2) + (b.1 as f32 - cy).powi(2);
                    da.total_cmp(&db)
                })
                .copied()
                .unwrap();
            (b.to_string(), best.0, best.1)
        })
        .collect()
}

/// All the strokes for one tile, back to front.
pub fn tile_strokes(
    el: &Tile,
    blocks: &BTreeMap<String, Occupancy>,
    turnouts: &BTreeMap<String, TurnoutStatus>,
    colors: Option<&BTreeMap<String, String>>,
    focus: Option<&str>,
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
    // In the editor, colour by block instead of by occupancy.
    let (occ, style) = match colors {
        Some(c) => match block.as_ref().and_then(|b| c.get(b)) {
            Some(color) => ("block-colored", format!("stroke:{color}")),
            None => ("unassigned", String::new()),
        },
        None => (occ_class(block.as_ref(), blocks), String::new()),
    };
    let faded = match focus {
        Some(f) if block.as_deref() != Some(f) => " faded",
        _ => "",
    };
    let st = turnout
        .as_ref()
        .and_then(|t| turnouts.get(t))
        .copied()
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut push = |segs: &[Seg], class: String| {
        for s in segs {
            let style = if class.contains("leg-off") {
                String::new()
            } else {
                style.clone()
            };
            out.push(Drawn {
                d: seg_path(*x, *y, *s),
                class: format!("{class}{faded}"),
                style,
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

/// A pointer action on a grid cell in edit mode.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CellEvent {
    pub x: u16,
    pub y: u16,
    /// Dragged onto this cell with the pointer held down.
    pub drag: bool,
    /// Shift / Ctrl / Cmd was held: extend the selection.
    pub extend: bool,
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
    pub selected: Vec<(u16, u16)>,
    /// Edit mode: colour tiles by block and show block name tags.
    #[props(default)]
    pub block_colors: Option<BTreeMap<String, String>>,
    /// Edit mode: dim every block except this one.
    #[props(default)]
    pub focus_block: Option<String>,
    /// Edit mode: a cell was pressed or dragged over.
    #[props(default)]
    pub on_cell: Option<EventHandler<CellEvent>>,
    /// Dispatch mode: a turnout tile was clicked.
    #[props(default)]
    pub on_turnout: Option<EventHandler<String>>,
    /// Edit mode: the cell under the pointer (None when it leaves the grid).
    #[props(default)]
    pub on_hover: Option<EventHandler<Option<(u16, u16)>>>,
    /// Edit mode: a translucent preview of the tile about to be placed.
    #[props(default)]
    pub ghost: Option<Tile>,
    /// Edit mode: fixed size of one grid cell in CSS pixels (zoom). `None`
    /// scales the drawing to the available width.
    #[props(default)]
    pub cell_px: Option<f32>,
    /// Extra CSS class on the `<svg>` (e.g. the active tool, for cursors).
    #[props(default)]
    pub class: String,
}

#[component]
pub fn TrackView(props: TrackViewProps) -> Element {
    let mut painting = use_signal(|| false);
    let cols = props.layout.grid.cols;
    let rows = props.layout.grid.rows;
    let edit = props.mode == Mode::Edit;
    let view_box = format!("-0.25 -0.25 {} {}", cols as f32 + 0.5, rows as f32 + 0.5);
    let size_style = match props.cell_px {
        Some(px) => format!(
            "width:{:.0}px;height:{:.0}px",
            (cols as f32 + 0.5) * px,
            (rows as f32 + 0.5) * px
        ),
        None => String::new(),
    };
    let on_hover = props.on_hover;
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
            class: if edit { "track-svg editing {props.class}" } else { "track-svg" },
            style: "{size_style}",
            "viewBox": "{view_box}",
            "preserveAspectRatio": "xMidYMid meet",
            onpointerup: move |_| painting.set(false),
            onpointerleave: move |_| {
                painting.set(false);
                if let Some(h) = on_hover { h.call(None) }
            },
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
                        for s in tile_strokes(el, &props.blocks, &props.turnouts, props.block_colors.as_ref(), props.focus_block.as_deref()) {
                            path { class: "{s.class}", style: "{s.style}", "d": "{s.d}" }
                        }
                    },
                }
            }
            if let Some(ghost) = props.ghost.as_ref() {
                g { class: "ghost",
                    match ghost {
                        Tile::Label { x, y, text } => rsx! {
                            text { class: "label", "x": "{*x as f32 + 0.1}", "y": "{*y as f32 + 0.72}", "{text}" }
                        },
                        Tile::Track { .. } => rsx! {
                            for s in tile_strokes(ghost, &BTreeMap::new(), &BTreeMap::new(), props.block_colors.as_ref(), None) {
                                path { class: "{s.class}", style: "{s.style}", "d": "{s.d}" }
                            }
                        },
                    }
                }
            }
            for (sx, sy) in props.selected.iter() {
                rect { class: "sel", "x": "{sx}", "y": "{sy}", "width": "1", "height": "1" }
            }
            if let Some(colors) = props.block_colors.as_ref() {
                for (b, bx, by) in block_label_positions(&props.layout) {
                    {
                        let color = colors.get(&b).cloned().unwrap_or_default();
                        let w = 0.32 * b.chars().count() as f32 + 0.3;
                        let faded = props.focus_block.as_ref().is_some_and(|f| *f != b);
                        rsx! {
                            g { class: if faded { "block-tag faded" } else { "block-tag" },
                                rect {
                                    "x": "{bx as f32 + 0.5 - w / 2.0}", "y": "{by as f32 - 0.32}",
                                    "width": "{w}", "height": "0.5", "rx": "0.12",
                                    style: "fill:{color}",
                                }
                                text { "x": "{bx as f32 + 0.5}", "y": "{by as f32 + 0.06}", "{b}" }
                            }
                        }
                    }
                }
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
                                        let m = e.modifiers();
                                        let extend = m.shift() || m.ctrl() || m.meta();
                                        if let Some(h) = handler { h.call(CellEvent { x: cx, y: cy, drag: false, extend }) }
                                    },
                                    onpointerenter: move |e| {
                                        if let Some(h) = on_hover { h.call(Some((cx, cy))) }
                                        if painting() {
                                            let m = e.modifiers();
                                            let extend = m.shift() || m.ctrl() || m.meta();
                                            if let Some(h) = handler { h.call(CellEvent { x: cx, y: cy, drag: true, extend }) }
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
