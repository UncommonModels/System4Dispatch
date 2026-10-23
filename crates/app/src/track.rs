//! SVG rendering of the track grid, shared by the dispatch panel and editor.
//! Coordinates are in tile units: tile (x, y) spans x..x+1, y..y+1.

use std::collections::BTreeMap;

use dioxus::prelude::*;
use s4d_core::layout::{dir_offset, Seg, CENTER};
use s4d_core::{
    Element as Tile, Layout, Occupancy, Position, Shape, TurnoutStatus, Turntable, TurntableStatus,
};

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
        // Bends are drawn as two straight legs meeting at the tile centre.
        format!("M{ax:.3} {ay:.3} L{cx:.3} {cy:.3} L{bx:.3} {by:.3}")
    }
}

/// Direction `d` turned clockwise by `rot` × 45° (the centre stays put).
fn rot_dir(d: u8, rot: u8) -> u8 {
    if d == CENTER {
        d
    } else {
        (d + rot) % 8
    }
}

/// Path data for one segment of a `shape` tile at (x, y): like
/// [`seg_path`], except that a slip's slip roads bow out to the side of
/// the crossing so they can be told apart from it.
pub fn leg_path(shape: Shape, x: u16, y: u16, (a, b): Seg) -> String {
    if !shape.is_slip_road((a, b)) {
        return seg_path(x, y, (a, b));
    }
    let (ax, ay) = pt(x, y, a);
    let (bx, by) = pt(x, y, b);
    let (cx, cy) = pt(x, y, CENTER);
    let unit = |d: u8| {
        let (ux, uy) = dir_offset(d);
        let len = (ux * ux + uy * uy).sqrt().max(1e-6);
        (ux / len, uy / len)
    };
    let ((ux, uy), (vx, vy)) = (unit(a), unit(b));
    let (sx, sy) = (ux + vx, uy + vy);
    let len = (sx * sx + sy * sy).sqrt().max(1e-6);
    let (kx, ky) = (cx + sx / len * 0.2, cy + sy / len * 0.2);
    format!("M{ax:.3} {ay:.3} L{kx:.3} {ky:.3} L{bx:.3} {by:.3}")
}

/// A circle as path data.
pub fn circle_path(cx: f32, cy: f32, r: f32) -> String {
    format!(
        "M{:.3} {cy:.3} A{r:.3} {r:.3} 0 1 0 {:.3} {cy:.3} A{r:.3} {r:.3} 0 1 0 {:.3} {cy:.3}",
        cx - r,
        cx + r,
        cx - r
    )
}

fn occ_class(block: Option<&String>, blocks: &BTreeMap<String, Occupancy>) -> &'static str {
    match block.and_then(|b| blocks.get(b)) {
        None => "occ-none",
        Some(Occupancy::Unknown) => "occ-unknown",
        Some(Occupancy::Clear) => "occ-clear",
        Some(Occupancy::Occupied) => "occ-occupied",
    }
}

/// Helper for matching tiles worked by two turnouts in `rsx!`.
trait FilterTwo {
    fn filter_two(self) -> Tile;
}

impl FilterTwo for Tile {
    /// The tile itself if it's a switch with two turnouts, else a label
    /// (which no track pattern matches).
    fn filter_two(self) -> Tile {
        match &self {
            Tile::Track { shape, .. } if shape.has_two_turnouts() => self,
            _ => Tile::Label {
                x: 0,
                y: 0,
                text: String::new(),
            },
        }
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
            block,
            block2,
            ..
        } = el
        {
            for b in block.iter().chain(block2) {
                by_block.entry(b.as_str()).or_default().push((*x, *y));
            }
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
        block2,
        turnout,
        turnout2,
        ..
    } = el
    else {
        return Vec::new();
    };
    let legs = shape.legs().rotated(*rot);
    // In the editor, colour by block instead of by occupancy.
    let look = |block: Option<&String>| match colors {
        Some(c) => match block.and_then(|b| c.get(b)) {
            Some(color) => ("block-colored", format!("stroke:{color}")),
            None => ("unassigned", String::new()),
        },
        None => (occ_class(block, blocks), String::new()),
    };
    let (occ, style) = look(block.as_ref());
    // The second track through a crossing may be in its own block.
    let (occ2, style2) = look(block2.as_ref().or(block.as_ref()));
    let second = |(a, b): Seg| {
        let back = 8 - rot % 8;
        shape.on_second_track((rot_dir(a, back), rot_dir(b, back)))
    };
    // Recolour a stroke that belongs to the second track.
    let restyle = |d: Drawn, seg: Seg| {
        if second(seg) && !d.class.contains("leg-off") {
            Drawn {
                class: d.class.replace(occ, occ2),
                style: style2.clone(),
                ..d
            }
        } else {
            d
        }
    };
    let faded = match focus {
        Some(f) if block.as_deref() != Some(f) && block2.as_deref() != Some(f) => " faded",
        _ => "",
    };
    let st = turnout
        .as_ref()
        .and_then(|t| turnouts.get(t))
        .copied()
        .unwrap_or_default();
    let mut out = Vec::new();
    if *shape == Shape::Portal {
        let (track, arrow) = portal_paths(*x, *y, *rot);
        out.push(Drawn {
            d: track,
            class: format!("track {occ}{faded}"),
            style: style.clone(),
        });
        out.push(Drawn {
            d: arrow,
            class: format!("portal-arrow {occ}{faded}"),
            style,
        });
        return out;
    }
    if shape.has_two_turnouts() {
        let st2 = turnout2
            .as_ref()
            .and_then(|t| turnouts.get(t))
            .copied()
            .unwrap_or_default();
        let shown = |s: TurnoutStatus| s.pending.unwrap_or(s.position);
        let routes = shape.routes();
        let route_for = |p| routes.iter().position(|(r, _)| *r == p);
        let set = route_for((st.position, st2.position));
        let target = route_for((shown(st), shown(st2)));
        let moving = st.pending.is_some() || st2.pending.is_some();
        let mut order: Vec<usize> = (0..routes.len()).collect();
        // The set route draws last, on top.
        order.sort_by_key(|i| Some(*i) == set);
        for i in order {
            let (a, b) = routes[i].1;
            let segment = (rot_dir(a, *rot), rot_dir(b, *rot));
            let class = match set {
                Some(s) if s == i => format!("track {occ}"),
                Some(_) => "track leg-off".to_string(),
                None => format!("track {occ} leg-unknown"),
            };
            let pending = if moving && target == Some(i) {
                " pending"
            } else {
                ""
            };
            out.push(restyle(
                Drawn {
                    d: leg_path(*shape, *x, *y, segment),
                    class: format!("{class}{pending}{faded}"),
                    style: if class.contains("leg-off") {
                        String::new()
                    } else {
                        style.clone()
                    },
                },
                segment,
            ));
        }
        return out;
    }
    let mut push = |segs: &[Seg], class: String| {
        for s in segs {
            let style = if class.contains("leg-off") {
                String::new()
            } else {
                style.clone()
            };
            out.push(restyle(
                Drawn {
                    d: seg_path(*x, *y, *s),
                    class: format!("{class}{faded}"),
                    style,
                },
                *s,
            ));
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

/// How far stall tracks reach beyond the pit, in tile units.
const STALL_LEN: f32 = 1.5;

/// Unit vector for a compass angle in degrees (0 = N, clockwise; y down).
fn unit_deg(angle: f32) -> (f32, f32) {
    let a = angle.to_radians();
    (a.sin(), -a.cos())
}

/// Pit radius of a turntable `size` cells across.
pub fn pit_radius(t: Option<&Turntable>) -> f32 {
    t.map_or(1, |t| t.size.max(1)) as f32 * 0.5 - 0.08
}

/// The engine house around a turntable's stalls, as a closed polygon: a
/// faceted ring with a corner between each pair of neighbouring stalls.
/// `None` if there are no stalls.
fn roundhouse_path(cx: f32, cy: f32, r: f32, t: &Turntable) -> Option<String> {
    let mut angles: Vec<f32> = t
        .tracks
        .iter()
        .filter(|tr| tr.stall)
        .map(|tr| tr.angle as f32)
        .collect();
    if angles.is_empty() {
        return None;
    }
    angles.sort_by(f32::total_cmp);
    // Start just after the widest gap, so the house wraps the stalls.
    let n = angles.len();
    let gap_after = |i: usize| (angles[(i + 1) % n] - angles[i]).rem_euclid(360.0);
    let widest = (0..n)
        .max_by(|&a, &b| gap_after(a).total_cmp(&gap_after(b)))
        .unwrap_or(0);
    let ordered: Vec<f32> = (1..=n)
        .map(|k| angles[(widest + k) % n])
        .scan(None::<f32>, |prev, a| {
            let a = match *prev {
                Some(p) => p + (a - p).rem_euclid(360.0),
                None => a,
            };
            *prev = Some(a);
            Some(a)
        })
        .collect();
    let spacing = if n > 1 {
        (ordered[n - 1] - ordered[0]) / (n - 1) as f32
    } else {
        15.0
    };
    let pad = (spacing / 2.0).clamp(4.0, 12.0);
    let mut edges = vec![ordered[0] - pad];
    edges.extend(ordered.windows(2).map(|w| (w[0] + w[1]) / 2.0));
    edges.push(ordered[n - 1] + pad);
    let (r0, r1) = (r + 0.4, r + STALL_LEN + 0.05);
    let at = |a: f32, rr: f32| {
        let (ux, uy) = unit_deg(a);
        format!("{:.3} {:.3}", cx + ux * rr, cy + uy * rr)
    };
    let outer: Vec<String> = edges.iter().map(|&a| at(a, r1)).collect();
    let inner: Vec<String> = edges.iter().rev().map(|&a| at(a, r0)).collect();
    Some(format!("M{} L{} Z", outer.join(" L"), inner.join(" L")))
}

/// The strokes for a turntable: roundhouse, pit, the tracks leaving it, and
/// the bridge at its known (or pending) position. In the editor (`colors`
/// set) the bridge is drawn at the first track.
pub fn turntable_strokes(
    el: &Tile,
    table: Option<&Turntable>,
    st: TurntableStatus,
    blocks: &BTreeMap<String, Occupancy>,
    colors: Option<&BTreeMap<String, String>>,
    focus: Option<&str>,
) -> Vec<Drawn> {
    let Tile::Track { x, y, block, .. } = el else {
        return Vec::new();
    };
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
    let (cx, cy) = pt(*x, *y, CENTER);
    let r = pit_radius(table);
    let mut out = Vec::new();
    if let Some(house) = table.and_then(|t| roundhouse_path(cx, cy, r, t)) {
        out.push(Drawn {
            d: house,
            class: format!("stall-house{faded}"),
            style: String::new(),
        });
    }
    let tracks = table.map(|t| t.tracks.as_slice()).unwrap_or_default();
    for tr in tracks {
        let (ux, uy) = unit_deg(tr.angle as f32);
        let end = if tr.stall {
            r + STALL_LEN
        } else {
            // Leads reach the edge (or corner) of the tile to meet the grid.
            let edge = match tr.dir() {
                Some(d) if d % 2 == 1 => std::f32::consts::FRAC_1_SQRT_2,
                _ => 0.5,
            };
            edge.max(r + 0.12)
        };
        out.push(Drawn {
            d: format!(
                "M{:.3} {:.3} L{:.3} {:.3}",
                cx + ux * r,
                cy + uy * r,
                cx + ux * end,
                cy + uy * end
            ),
            class: format!("track {occ}{faded}"),
            style: style.clone(),
        });
    }
    out.push(Drawn {
        d: circle_path(cx, cy, r),
        class: format!("tt-pit{faded}"),
        style: String::new(),
    });
    let bridge = |angle: u16, class: &str| {
        let (ux, uy) = unit_deg(angle as f32);
        let (nx, ny) = (-uy, ux);
        let rr = r - 0.04;
        // A small arrowhead marks the head end.
        let (tx, ty) = (cx + ux * (rr - 0.02), cy + uy * (rr - 0.02));
        let (bx, by) = (cx + ux * (rr - 0.22), cy + uy * (rr - 0.22));
        [
            Drawn {
                d: format!(
                    "M{:.3} {:.3} L{:.3} {:.3}",
                    cx - ux * rr,
                    cy - uy * rr,
                    cx + ux * rr,
                    cy + uy * rr
                ),
                class: format!("track {class}{faded}"),
                style: if class.contains(occ) {
                    style.clone()
                } else {
                    String::new()
                },
            },
            Drawn {
                d: format!(
                    "M{tx:.3} {ty:.3} L{:.3} {:.3} L{:.3} {:.3} Z",
                    bx + nx * 0.1,
                    by + ny * 0.1,
                    bx - nx * 0.1,
                    by - ny * 0.1
                ),
                class: format!("tt-head{faded}"),
                style: String::new(),
            },
        ]
    };
    let angle_of = |i: usize| tracks.get(i).map(|tr| tr.angle);
    let shown = if colors.is_some() {
        tracks.first().map(|tr| tr.angle)
    } else {
        st.position.and_then(angle_of)
    };
    if let Some(a) = st.pending.and_then(angle_of).filter(|a| Some(*a) != shown) {
        out.extend(bridge(a, "leg-off pending"));
    }
    if let Some(a) = shown {
        out.extend(bridge(a, occ));
    }
    out
}

/// A portal at tile (x, y): the track from its entry edge, and an arrow
/// showing it carries on elsewhere.
pub fn portal_paths(x: u16, y: u16, rot: u8) -> (String, String) {
    let entry = (6 + rot) % 8;
    let (ex, ey) = pt(x, y, entry);
    let (cx, cy) = pt(x, y, CENTER);
    let (ux, uy) = {
        let (a, b) = dir_offset(entry);
        let len = (a * a + b * b).sqrt().max(1e-6);
        (-a / len, -b / len)
    };
    let (nx, ny) = (-uy, ux);
    let (tx, ty) = (cx + ux * 0.38, cy + uy * 0.38);
    let (bx, by) = (cx + ux * 0.06, cy + uy * 0.06);
    (
        format!(
            "M{ex:.3} {ey:.3} L{:.3} {:.3}",
            cx + ux * 0.1,
            cy + uy * 0.1
        ),
        format!(
            "M{:.3} {:.3} L{tx:.3} {ty:.3} L{:.3} {:.3}",
            bx + nx * 0.24,
            by + ny * 0.24,
            bx - nx * 0.24,
            by - ny * 0.24
        ),
    )
}

/// Where to write a portal's name: beside the arrow.
fn portal_label_pos(x: u16, y: u16, rot: u8) -> (f32, f32) {
    let entry = (6 + rot) % 8;
    let (cx, cy) = pt(x, y, CENTER);
    let (a, b) = dir_offset((entry + 2) % 8);
    let len = (a * a + b * b).sqrt().max(1e-6);
    (cx + a / len * 0.3, cy + b / len * 0.3 + 0.12)
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
    #[props(default)]
    pub turntables: BTreeMap<String, TurntableStatus>,
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
    /// Dispatch mode: a turntable track was clicked (turntable id and the
    /// track's index).
    #[props(default)]
    pub on_turntable: Option<EventHandler<(String, usize)>>,
    /// Dispatch mode: a switch worked by two turnouts (3-way or slip) was
    /// clicked: its shape and left-hand and right-hand turnout ids.
    #[props(default)]
    pub on_pair: Option<EventHandler<(Shape, String, String)>>,
    /// Edit mode: the cell under the pointer (None when it leaves the grid).
    #[props(default)]
    pub on_hover: Option<EventHandler<Option<(u16, u16)>>>,
    /// Edit mode: a translucent preview of the tiles about to be placed.
    #[props(default)]
    pub ghost: Vec<Tile>,
    /// Edit mode: fixed size of one grid cell in CSS pixels (zoom). `None`
    /// scales the drawing to the available width.
    #[props(default)]
    pub cell_px: Option<f32>,
    /// Extra CSS class on the `<svg>` (e.g. the active tool, for cursors).
    #[props(default)]
    pub class: String,
}

/// Click targets for turntable tracks: a narrow wedge from the pit's
/// centre out over each track. Yields (turntable id, turntable name, track
/// name, track index, path data).
fn turntable_hits(layout: &Layout) -> Vec<(String, String, String, usize, String)> {
    let mut out = Vec::new();
    for el in &layout.elements {
        let Tile::Track {
            x,
            y,
            shape: Shape::Turntable,
            turntable: Some(id),
            ..
        } = el
        else {
            continue;
        };
        let Some(t) = layout.turntable(id) else {
            continue;
        };
        let (cx, cy) = pt(*x, *y, CENTER);
        let r = pit_radius(Some(t)) + STALL_LEN;
        let n = t.tracks.len();
        for (i, tr) in t.tracks.iter().enumerate() {
            let a = tr.angle as f32;
            // Half the gap to each neighbour, at most 20°.
            let gap = |j: usize| {
                let b = t.tracks[j % n].angle as f32;
                ((b - a).rem_euclid(360.0)).min((a - b).rem_euclid(360.0))
            };
            let w = if n > 1 {
                (gap(i + 1).min(gap(i + n - 1)) / 2.0).min(20.0)
            } else {
                20.0
            };
            let at = |ang: f32| {
                let (ux, uy) = unit_deg(ang);
                (cx + r * ux, cy + r * uy)
            };
            let (ax, ay) = at(a - w);
            let (bx, by) = at(a + w);
            out.push((
                t.id.clone(),
                t.display_name().to_string(),
                tr.display_name(),
                i,
                format!("M{cx:.3} {cy:.3} L{ax:.3} {ay:.3} L{bx:.3} {by:.3} Z"),
            ));
        }
    }
    out
}

/// Grid lines (edit mode). A component of its own so it only re-renders
/// when the grid size changes.
#[component]
fn GridLines(cols: u16, rows: u16) -> Element {
    rsx! {
        for gx in 0..=cols {
            line { class: "grid-line", "x1": "{gx}", "y1": "0", "x2": "{gx}", "y2": "{rows}" }
        }
        for gy in 0..=rows {
            line { class: "grid-line", "x1": "0", "y1": "{gy}", "x2": "{cols}", "y2": "{gy}" }
        }
    }
}

/// The drawing itself: tiles, turntables, portal names and block tags.
/// Only re-renders when the layout or its state changes, not on hover,
/// selection or zoom.
#[component]
fn TrackLayer(
    layout: Layout,
    blocks: BTreeMap<String, Occupancy>,
    turnouts: BTreeMap<String, TurnoutStatus>,
    turntables: BTreeMap<String, TurntableStatus>,
    block_colors: Option<BTreeMap<String, String>>,
    focus_block: Option<String>,
) -> Element {
    rsx! {
        for el in layout.elements.iter() {
            match el {
                Tile::Label { x, y, text } => rsx! {
                    text { class: "label", "x": "{*x as f32 + 0.1}", "y": "{*y as f32 + 0.72}", "{text}" }
                },
                // Turntables draw last: their roundhouses overlap other cells.
                Tile::Track { shape: Shape::Turntable, .. } => rsx! {},
                Tile::Track { .. } => rsx! {
                    for s in tile_strokes(el, &blocks, &turnouts, block_colors.as_ref(), focus_block.as_deref()) {
                        path { class: "{s.class}", style: "{s.style}", "d": "{s.d}" }
                    }
                },
            }
        }
        for el in layout.elements.iter() {
            if let Tile::Track { shape: Shape::Turntable, turntable, .. } = el {
                {
                    let table = turntable.as_deref().and_then(|t| layout.turntable(t));
                    let st = turntable.as_ref().and_then(|t| turntables.get(t)).copied().unwrap_or_default();
                    rsx! {
                        for s in turntable_strokes(el, table, st, &blocks, block_colors.as_ref(), focus_block.as_deref()) {
                            path { class: "{s.class}", style: "{s.style}", "d": "{s.d}" }
                        }
                    }
                }
            }
        }
        for el in layout.elements.iter() {
            if let Tile::Track { x, y, rot, shape: Shape::Portal, portal: Some(name), .. } = el {
                {
                    let (lx, ly) = portal_label_pos(*x, *y, *rot);
                    rsx! { text { class: "portal-name", "x": "{lx:.3}", "y": "{ly:.3}", "{name}" } }
                }
            }
        }
        if let Some(colors) = block_colors.as_ref() {
            for (b, bx, by) in block_label_positions(&layout) {
                {
                    let color = colors.get(&b).cloned().unwrap_or_default();
                    let w = 0.32 * b.chars().count() as f32 + 0.3;
                    let faded = focus_block.as_ref().is_some_and(|f| *f != b);
                    rsx! {
                        g { class: if faded { "block-tag faded" } else { "block-tag" },
                            rect {
                                "x": "{bx as f32 + 0.5 - w / 2.0}", "y": "{by as f32 - 0.32}",
                                "width": "{w}", "height": "0.5", "rx": "0.04",
                                style: "fill:{color}",
                            }
                            text { "x": "{bx as f32 + 0.5}", "y": "{by as f32 + 0.06}", "{b}" }
                        }
                    }
                }
            }
        }
    }
}

/// Click targets for turnouts, switches and turntables (dispatch mode).
#[component]
fn HitLayer(
    layout: Layout,
    on_turnout: Option<EventHandler<String>>,
    on_turntable: Option<EventHandler<(String, usize)>>,
    on_pair: Option<EventHandler<(Shape, String, String)>>,
) -> Element {
    let names: BTreeMap<String, String> = layout
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
        for (tid, tname, pname, dir, d) in turntable_hits(&layout) {
            {
                rsx! {
                    path {
                        class: "hit tt-hit",
                        "d": "{d}",
                        onclick: move |_| if let Some(h) = on_turntable { h.call((tid.clone(), dir)) },
                        title { "{tname}: turn to {pname}" }
                    }
                }
            }
        }
        for el in layout.elements.iter() {
            if let Tile::Track { x, y, shape, turnout: Some(t), turnout2: Some(t2), .. } = el.clone().filter_two() {
                {
                    let title = format!(
                        "{}: {} / {}",
                        shape.label(),
                        names.get(&t).cloned().unwrap_or_else(|| t.clone()),
                        names.get(&t2).cloned().unwrap_or_else(|| t2.clone())
                    );
                    rsx! {
                        rect {
                            class: "hit",
                            "x": "{x}", "y": "{y}", "width": "1", "height": "1",
                            onclick: move |_| if let Some(h) = on_pair { h.call((shape, t.clone(), t2.clone())) },
                            title { "{title}" }
                        }
                    }
                }
            } else if let Tile::Track { x, y, turnout: Some(t), .. } = el {
                {
                    let t = t.clone();
                    let title = names.get(&t).cloned().unwrap_or_else(|| t.clone());
                    rsx! {
                        rect {
                            class: "hit",
                            "x": "{x}", "y": "{y}", "width": "1", "height": "1",
                            onclick: move |_| if let Some(h) = on_turnout { h.call(t.clone()) },
                            title { "{title}" }
                        }
                    }
                }
            }
        }
    }
}

/// One invisible target per grid cell (edit mode), for clicking and
/// dragging. Only re-renders when the grid size changes.
#[component]
fn CellLayer(
    cols: u16,
    rows: u16,
    painting: Signal<bool>,
    on_cell: Option<EventHandler<CellEvent>>,
    on_hover: Option<EventHandler<Option<(u16, u16)>>>,
) -> Element {
    let mut painting = painting;
    rsx! {
        for cy in 0..rows {
            for cx in 0..cols {
                rect {
                    class: "cell",
                    "x": "{cx}", "y": "{cy}", "width": "1", "height": "1",
                    onpointerdown: move |e| {
                        // The middle and right buttons don't draw (the middle one pans).
                        use dioxus::html::input_data::MouseButton;
                        if matches!(e.trigger_button(), Some(MouseButton::Auxiliary | MouseButton::Secondary)) {
                            return;
                        }
                        e.prevent_default();
                        painting.set(true);
                        let m = e.modifiers();
                        let extend = m.shift() || m.ctrl() || m.meta();
                        if let Some(h) = on_cell { h.call(CellEvent { x: cx, y: cy, drag: false, extend }) }
                    },
                    onpointerenter: move |e| {
                        if let Some(h) = on_hover { h.call(Some((cx, cy))) }
                        if *painting.peek() {
                            let m = e.modifiers();
                            let extend = m.shift() || m.ctrl() || m.meta();
                            if let Some(h) = on_cell { h.call(CellEvent { x: cx, y: cy, drag: true, extend }) }
                        }
                    },
                }
            }
        }
    }
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
                GridLines { cols, rows }
            }
            TrackLayer {
                layout: props.layout.clone(),
                blocks: props.blocks.clone(),
                turnouts: props.turnouts.clone(),
                turntables: props.turntables.clone(),
                block_colors: props.block_colors.clone(),
                focus_block: props.focus_block.clone(),
            }
            if !props.ghost.is_empty() {
                g { class: "ghost",
                    for ghost in props.ghost.iter() {
                        match ghost {
                            Tile::Label { x, y, text } => rsx! {
                                text { class: "label", "x": "{*x as f32 + 0.1}", "y": "{*y as f32 + 0.72}", "{text}" }
                            },
                            Tile::Track { shape: Shape::Turntable, .. } => rsx! {
                                for s in turntable_strokes(ghost, None, TurntableStatus::default(), &BTreeMap::new(), props.block_colors.as_ref(), None) {
                                    path { class: "{s.class}", style: "{s.style}", "d": "{s.d}" }
                                }
                            },
                            Tile::Track { .. } => rsx! {
                                for s in tile_strokes(ghost, &BTreeMap::new(), &BTreeMap::new(), props.block_colors.as_ref(), None) {
                                    path { class: "{s.class}", style: "{s.style}", "d": "{s.d}" }
                                }
                            },
                        }
                    }
                }
            }
            for (sx, sy) in props.selected.iter() {
                rect { class: "sel", "x": "{sx}", "y": "{sy}", "width": "1", "height": "1" }
            }
            if !edit {
                HitLayer {
                    layout: props.layout.clone(),
                    on_turnout: props.on_turnout,
                    on_turntable: props.on_turntable,
                    on_pair: props.on_pair,
                }
            }
            if edit {
                CellLayer { cols, rows, painting, on_cell: props.on_cell, on_hover: props.on_hover }
            }
        }
    }
}
