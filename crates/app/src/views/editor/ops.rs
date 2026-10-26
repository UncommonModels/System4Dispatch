//! Editor operations on the draft layout, and undo/redo history.

use std::collections::BTreeSet;

use dioxus::prelude::*;
use s4d_core::{Element as Tile, Layout, Shape};

use crate::bus::Bus;
use crate::History;

pub type Cell = (u16, u16);

const MAX_UNDO: usize = 200;

/// The largest grid the editor makes, in columns or rows.
pub const MAX_GRID: u16 = 400;

/// The layout being edited: the draft if there is one, else the applied layout.
pub fn current(draft: Signal<Option<Layout>>, bus: Bus) -> Layout {
    draft
        .read()
        .clone()
        .unwrap_or_else(|| bus.peek(|d| d.layout().clone()))
}

/// Remember `before` so it can be restored with undo.
fn record(before: Layout) {
    if let Some(h) = try_consume_context::<History>() {
        let (mut undo, mut redo) = (h.undo, h.redo);
        let mut u = undo.write();
        if u.last() != Some(&before) {
            u.push(before);
            if u.len() > MAX_UNDO {
                u.remove(0);
            }
        }
        redo.write().clear();
    }
}

/// Replace the draft with `next`, as one undoable step.
pub fn set_draft(mut draft: Signal<Option<Layout>>, bus: Bus, next: Layout) {
    let before = current(draft, bus);
    if before != next {
        record(before);
        draft.set(Some(next));
    }
}

/// Apply `f` to the draft, as one undoable step.
pub fn edit_draft(draft: Signal<Option<Layout>>, bus: Bus, f: impl FnOnce(&mut Layout)) {
    let mut l = current(draft, bus);
    f(&mut l);
    set_draft(draft, bus, l);
}

/// Like [`edit_draft`] but not recorded: used while dragging, so a whole
/// stroke undoes in one step (the press that started it was recorded).
pub fn edit_draft_quiet(mut draft: Signal<Option<Layout>>, bus: Bus, f: impl FnOnce(&mut Layout)) {
    let mut l = current(draft, bus);
    f(&mut l);
    draft.set(Some(l));
}

pub fn undo(mut draft: Signal<Option<Layout>>, bus: Bus) {
    let Some(h) = try_consume_context::<History>() else {
        return;
    };
    let (mut undo, mut redo) = (h.undo, h.redo);
    let prev = undo.write().pop();
    if let Some(prev) = prev {
        redo.write().push(current(draft, bus));
        draft.set(Some(prev));
    }
}

pub fn redo(mut draft: Signal<Option<Layout>>, bus: Bus) {
    let Some(h) = try_consume_context::<History>() else {
        return;
    };
    let (mut undo, mut redo) = (h.undo, h.redo);
    let next = redo.write().pop();
    if let Some(next) = next {
        undo.write().push(current(draft, bus));
        draft.set(Some(next));
    }
}

/// All cells in the rectangle spanned by `a` and `b`.
pub fn rect_cells(a: Cell, b: Cell) -> impl Iterator<Item = Cell> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
}

/// Move the tiles in `sel` by (dx, dy); `None` if any would leave the grid
/// or land on another tile.
pub fn move_tiles(
    l: &mut Layout,
    sel: &BTreeSet<Cell>,
    dx: i32,
    dy: i32,
) -> Option<BTreeSet<Cell>> {
    l.move_cells(sel, dx, dy).ok()
}

/// Rotate the track tiles in `sel` in place by `delta` × 45°.
pub fn rotate_tiles(l: &mut Layout, sel: &BTreeSet<Cell>, delta: u8) {
    l.rotate_cells(sel, delta);
}

/// Put the track tiles in `sel` into `block`.
pub fn assign_block(l: &mut Layout, sel: &BTreeSet<Cell>, block: Option<&str>) {
    l.assign_block(sel, block);
}

/// Place a track tile (turnout tiles keep or get a turnout). Without a
/// block of its own, a line that now joins two ends of one block takes that
/// block.
pub fn place_track(l: &mut Layout, x: u16, y: u16, shape: Shape, rot: u8, block: Option<String>) {
    let adopt = block.is_none();
    l.place_track(x, y, shape, rot, block);
    if adopt {
        l.adopt_block(x, y);
    }
}

/// Change a tile's shape, keeping its rotation and block.
pub fn set_tile_shape(l: &mut Layout, x: u16, y: u16, shape: Shape) {
    l.set_tile_shape(x, y, shape);
}

/// Remove a block and clear it from tiles.
pub fn remove_block(l: &mut Layout, id: &str) {
    l.remove_block(id);
}

/// Remove a turnout, its link, and clear it from tiles.
pub fn remove_turnout(l: &mut Layout, id: &str) {
    l.remove_turnout(id);
}

/// Paste copied tiles at the first diagonal offset where they all fit.
pub fn paste_tiles(l: &mut Layout, clip: &[Tile]) -> Option<BTreeSet<Cell>> {
    l.paste(clip).filter(|c| !c.is_empty())
}

/// The largest occupied column and row (for not shrinking the grid past them).
pub fn used_extent(l: &Layout) -> (u16, u16) {
    l.elements.iter().fold((0, 0), |(c, r), e| {
        let (x, y) = e.pos();
        (c.max(x + 1), r.max(y + 1))
    })
}

pub fn file_stem(name: &str) -> String {
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

/// A key press from the global editor shortcut listener.
#[derive(Debug, serde::Deserialize)]
pub struct KeyMsg {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
}

/// Listens for shortcut keys on the whole page while the editor's window
/// is the one in front, ignoring keys typed into text fields.
pub const KEY_LISTENER: &str = r#"
    const h = (e) => {
        const t = e.target;
        if (t && t.closest && t.closest("input, select, textarea, [contenteditable]")) return;
        // Enter on a focused button presses the button, not the grid.
        if (e.key === "Enter" && t && t.closest && t.closest("button, a")) return;
        const ed = document.querySelector(".ed");
        if (!ed || !ed.closest(".win.top")) return;
        const ctrl = e.ctrlKey || e.metaKey;
        const k = e.key;
        const plain = ["r", "R", "Delete", "Backspace", "Escape", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown",
                       "Enter", "v", "V", "s", "S", "e", "E", "t", "T", "a", "A", "1", "2", "3", "4", "5", "6", "7", "8", "9", "+", "=", "-", "0"];
        const combo = ["z", "Z", "y", "Y", "a", "A", "c", "C", "x", "X", "v", "V", "d", "D", "s", "S", "o", "O", "=", "+", "-", "0"];
        if (ctrl ? !combo.includes(k) : (e.altKey || !plain.includes(k))) return;
        e.preventDefault();
        dioxus.send({ key: k, ctrl, shift: e.shiftKey });
    };
    document.addEventListener("keydown", h);
    await dioxus.recv();
    document.removeEventListener("keydown", h);
"#;
