//! Layout editor, laid out like a drawing program: menus and history in a
//! top bar, a tool rail on the left, the canvas in the middle, a properties
//! panel on the right and a status bar underneath.

mod ops;
mod panels;

use std::collections::BTreeSet;

use dioxus::prelude::*;
use rolcb::EventId;
use s4d_core::{Element as Tile, Layout, Shape};

use crate::bus::Bus;
use crate::platform;
use crate::track::{block_colors, CellEvent, Mode, TrackView};
use crate::zoom::use_zoom_pan;
use crate::{Draft, History};
use ops::*;
use panels::{BlocksPanel, PropertiesPanel, ShapeIcon, TurnoutsPanel, TurntablesPanel};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tool {
    Select,
    Place(Shape),
    /// A turntable with a lead track and a ring of roundhouse stalls.
    Roundhouse,
    /// Click two points and track is drawn between them.
    AutoTrack,
    Label,
    Erase,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Place(s) => s.label(),
            Tool::Roundhouse => "Turntable & roundhouse",
            Tool::AutoTrack => "Auto track",
            Tool::Label => "Label",
            Tool::Erase => "Erase",
        }
    }

    fn css(self) -> &'static str {
        match self {
            Tool::Select => "tool-select",
            Tool::Place(_) | Tool::Roundhouse | Tool::AutoTrack | Tool::Label => "tool-place",
            Tool::Erase => "tool-erase",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PanelTab {
    Properties,
    Blocks,
    Turnouts,
    Turntables,
}

/// Editor UI state shared by the editor's sub-components.
#[derive(Clone, Copy)]
pub(crate) struct EditorState {
    pub tool: Signal<Tool>,
    pub rot: Signal<u8>,
    pub selection: Signal<BTreeSet<Cell>>,
    pub paint_block: Signal<Option<String>>,
    pub focus_block: Signal<Option<String>>,
    pub error: Signal<Option<String>>,
    pub tab: Signal<PanelTab>,
    /// Block, turnout or turntable id expanded in its panel list.
    pub expanded: Signal<Option<String>>,
}

/// Pit size (cells across) and stall count for the roundhouse tool.
const ROUNDHOUSE_SIZE: u8 = 2;
const ROUNDHOUSE_STALLS: u16 = 7;

#[component]
pub fn EditorView() -> Element {
    let bus = use_context::<Bus>();
    let draft = use_context::<Draft>().0;
    let history = use_context::<History>();

    let st = use_context_provider(|| EditorState {
        tool: Signal::new(Tool::Select),
        rot: Signal::new(0),
        selection: Signal::new(BTreeSet::new()),
        paint_block: Signal::new(None),
        focus_block: Signal::new(None),
        error: Signal::new(None),
        tab: Signal::new(PanelTab::Properties),
        expanded: Signal::new(None),
    });
    let EditorState {
        mut tool,
        mut rot,
        mut selection,
        paint_block,
        focus_block,
        mut error,
        ..
    } = st;
    // Drag-select: where the drag started and what was selected before it.
    let mut anchor = use_signal(|| None::<(Cell, BTreeSet<Cell>)>);
    let mut multi = use_signal(|| false);
    let mut show_tags = use_signal(|| true);
    let mut clipboard = use_signal(Vec::<Tile>::new);
    let mut hover = use_signal(|| None::<Cell>);
    let zoom = use_zoom_pan(".ed-canvas", false, Some(32.0));
    let mut menu = use_signal(|| None::<&'static str>);
    // Auto track: the first point picked, waiting for the second.
    let mut route_start = use_signal(|| None::<Cell>);
    use_effect(move || {
        if tool() != Tool::AutoTrack {
            route_start.set(None);
        }
    });

    let applied = bus.view(|d| d.layout().clone());
    let recent: Vec<EventId> = bus.view(|d| d.recent_events.iter().copied().collect());
    let layout = draft.read().clone().unwrap_or_else(|| applied.clone());
    let dirty = draft.read().as_ref().is_some_and(|d| *d != applied);
    let issues = layout.validate();
    let colors = block_colors(&layout);
    let sel: BTreeSet<Cell> = selection
        .read()
        .iter()
        .copied()
        .filter(|(x, y)| layout.element_at(*x, *y).is_some())
        .collect();
    let can_undo = !history.undo.read().is_empty();
    let can_redo = !history.redo.read().is_empty();

    // ---- Actions (shared by menus, toolbar and shortcuts) ---------------

    let on_cell = move |e: CellEvent| {
        let cell = (e.x, e.y);
        match tool() {
            Tool::Select => {
                let l = current(draft, bus);
                let occupied = |c: &Cell| l.element_at(c.0, c.1).is_some();
                if !e.drag {
                    let extend = e.extend || multi();
                    let base = if extend {
                        selection.read().clone()
                    } else {
                        BTreeSet::new()
                    };
                    let mut next = base.clone();
                    if extend && next.contains(&cell) {
                        next.remove(&cell);
                    } else if occupied(&cell) {
                        next.insert(cell);
                    }
                    anchor.set(Some((cell, base)));
                    selection.set(next);
                } else if let Some((start, base)) = anchor() {
                    let mut next = base;
                    next.extend(rect_cells(start, cell).filter(occupied));
                    selection.set(next);
                }
            }
            Tool::AutoTrack => {
                if e.drag {
                    return;
                }
                match route_start() {
                    None => {
                        let l = current(draft, bus);
                        if l.element_at(e.x, e.y).is_some() && l.free_ends(e.x, e.y).is_empty() {
                            error.set(Some("That track has no free end to join: pick the end of a line or an empty cell.".into()));
                        } else {
                            route_start.set(Some(cell));
                        }
                    }
                    Some(start) => {
                        let mut l = current(draft, bus);
                        match l.draw_route(start, cell, paint_block()) {
                            Ok(cells) => {
                                set_draft(draft, bus, l);
                                selection.set(cells);
                                route_start.set(None);
                            }
                            Err(why) => error.set(Some(format!("Can't draw track there: {why}."))),
                        }
                    }
                }
            }
            Tool::Erase if e.drag => edit_draft_quiet(draft, bus, |l| l.remove_at(e.x, e.y)),
            Tool::Erase => edit_draft(draft, bus, |l| l.remove_at(e.x, e.y)),
            Tool::Label => {
                if !e.drag {
                    edit_draft(draft, bus, |l| {
                        l.put(Tile::Label {
                            x: e.x,
                            y: e.y,
                            text: "Label".into(),
                        })
                    });
                    selection.set(BTreeSet::from([cell]));
                    tool.set(Tool::Select);
                }
            }
            Tool::Roundhouse => {
                if !e.drag {
                    let mut l = current(draft, bus);
                    match l.place_roundhouse(e.x, e.y, ROUNDHOUSE_SIZE, rot(), 2, ROUNDHOUSE_STALLS, paint_block()) {
                        Ok(_) => {
                            set_draft(draft, bus, l);
                            selection.set(BTreeSet::from([cell]));
                            tool.set(Tool::Select);
                        }
                        Err(why) => error.set(Some(format!(
                            "No room for a turntable and roundhouse here ({why}). The lead goes {} and the roundhouse round the other side; press R to turn it.",
                            ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][((6 + rot()) % 8) as usize]
                        ))),
                    }
                }
            }
            Tool::Place(shape) => {
                let block = paint_block();
                let r = rot();
                let place = move |l: &mut Layout| place_track(l, e.x, e.y, shape, r, block);
                if e.drag {
                    edit_draft_quiet(draft, bus, place);
                } else {
                    edit_draft(draft, bus, place);
                }
            }
        }
    };

    let mut nudge = move |dx: i32, dy: i32| {
        let sel = selection.read().clone();
        if sel.is_empty() {
            return;
        }
        let mut l = current(draft, bus);
        match move_tiles(&mut l, &sel, dx, dy) {
            Some(moved) => {
                set_draft(draft, bus, l);
                selection.set(moved);
            }
            None => error.set(Some(
                "Can't move there: tiles would leave the grid or overlap other track.".into(),
            )),
        }
    };
    let mut delete_selection = move || {
        let sel = selection.read().clone();
        if sel.is_empty() {
            return;
        }
        edit_draft(draft, bus, |l| {
            l.elements.retain(|el| !sel.contains(&el.pos()))
        });
        selection.set(BTreeSet::new());
    };
    let mut copy_selection = move || {
        let sel = selection.read().clone();
        let l = current(draft, bus);
        clipboard.set(
            l.elements
                .iter()
                .filter(|e| sel.contains(&e.pos()))
                .cloned()
                .collect(),
        );
    };
    let mut paste = move || {
        let clip = clipboard.read().clone();
        if clip.is_empty() {
            return;
        }
        let mut l = current(draft, bus);
        match paste_tiles(&mut l, &clip) {
            Some(cells) => {
                set_draft(draft, bus, l);
                selection.set(cells);
                tool.set(Tool::Select);
            }
            None => error.set(Some(
                "No room to paste: make the grid bigger or clear some space.".into(),
            )),
        }
    };
    let mut rotate = move |back: bool| {
        let sel = selection.read().clone();
        let delta = if back { 7 } else { 1 };
        if sel.is_empty() || tool() != Tool::Select {
            rot.set((rot() + delta) % 8);
        } else {
            edit_draft(draft, bus, |l| rotate_tiles(l, &sel, delta));
        }
    };
    // Insert an empty row or column before or after the selection.
    let mut insert_line = move |column: bool, after: bool| {
        let sel = selection.read().clone();
        let across = |c: &Cell| if column { c.0 } else { c.1 };
        let at = if after {
            sel.iter().map(across).max().map(|v| v + 1)
        } else {
            sel.iter().map(across).min()
        };
        let Some(at) = at else {
            return;
        };
        let mut l = current(draft, bus);
        let size = if column { l.grid.cols } else { l.grid.rows };
        let done = if size >= MAX_GRID {
            None
        } else if column {
            l.insert_column(at)
        } else {
            l.insert_row(at)
        };
        let what = if column { "column" } else { "row" };
        let Some(done) = done else {
            error.set(Some(format!(
                "The grid can't grow past {MAX_GRID} {what}s."
            )));
            return;
        };
        set_draft(draft, bus, l);
        if !done.gaps.is_empty() {
            let cells: Vec<String> = done.gaps.iter().map(|(x, y)| format!("{x}, {y}")).collect();
            error.set(Some(format!(
                "Track crossing the new {what} couldn't be joined next to {} (switches on both sides, or a bend too sharp): draw those pieces yourself.",
                cells.join("; ")
            )));
        }
        selection.set(
            sel.into_iter()
                .map(|(x, y)| match (column, if column { x } else { y } >= at) {
                    (true, true) => (x + 1, y),
                    (false, true) => (x, y + 1),
                    _ => (x, y),
                })
                .collect(),
        );
    };
    let mut select_all = move || {
        let l = current(draft, bus);
        selection.set(l.elements.iter().map(|e| e.pos()).collect());
        tool.set(Tool::Select);
    };
    let reset_editor = move || {
        let mut s = st;
        s.selection.set(BTreeSet::new());
        s.paint_block.set(None);
        s.focus_block.set(None);
        s.expanded.set(None);
    };
    let apply = move || {
        if let Some(l) = draft.read().clone() {
            crate::save_layout(bus, l);
        }
        let mut d = draft;
        d.set(None);
    };
    let export = move || {
        let l = current(draft, bus);
        platform::save_file(&format!("{}.json", file_stem(&l.name)), &l.to_json());
    };
    let import = move || {
        spawn(async move {
            if let Some(text) = platform::open_file().await {
                match Layout::from_json(&text) {
                    Ok(l) => {
                        set_draft(draft, bus, l);
                        reset_editor();
                    }
                    Err(e) => error.set(Some(format!("Could not read layout: {e}"))),
                }
            }
        });
    };
    let new_layout = move || {
        spawn(async move {
            if platform::confirm("Start a new, empty layout? (You can undo this.)").await {
                set_draft(draft, bus, Layout::empty("New layout"));
                reset_editor();
            }
        });
    };

    // ---- Keyboard shortcuts --------------------------------------------

    let listener = use_hook(move || {
        let mut eval = document::eval(KEY_LISTENER);
        spawn(async move {
            while let Ok(k) = eval.recv::<KeyMsg>().await {
                let key = k.key.to_lowercase();
                match (k.ctrl, key.as_str()) {
                    (true, "z") if k.shift => redo(draft, bus),
                    (true, "z") => undo(draft, bus),
                    (true, "y") => redo(draft, bus),
                    (true, "a") => select_all(),
                    (true, "c") => copy_selection(),
                    (true, "x") => {
                        copy_selection();
                        delete_selection();
                    }
                    (true, "v") => paste(),
                    (true, "d") => {
                        copy_selection();
                        paste();
                    }
                    (true, "s") => apply(),
                    (true, "o") => import(),
                    (_, "+") | (_, "=") => zoom.zoom_by(1.25),
                    (_, "-") => zoom.zoom_by(1.0 / 1.25),
                    (_, "0") => zoom.set(Some(32.0)),
                    (false, "r") => rotate(k.shift),
                    (false, "delete") | (false, "backspace") => delete_selection(),
                    (false, "escape") => {
                        route_start.set(None);
                        selection.set(BTreeSet::new());
                        tool.set(Tool::Select);
                        menu.set(None);
                    }
                    (false, "arrowleft") => nudge(-1, 0),
                    (false, "arrowright") => nudge(1, 0),
                    (false, "arrowup") => nudge(0, -1),
                    (false, "arrowdown") => nudge(0, 1),
                    (false, "v") | (false, "s") => tool.set(Tool::Select),
                    (false, "e") => tool.set(Tool::Erase),
                    (false, "a") => tool.set(Tool::AutoTrack),
                    (false, "t") => tool.set(Tool::Label),
                    (false, n) if n.len() == 1 && ("1"..="9").contains(&n) => {
                        let i = n.parse::<usize>().unwrap_or(1) - 1;
                        tool.set(Tool::Place(Shape::ALL[i]));
                    }
                    _ => {}
                }
            }
        });
        eval
    });
    use_drop(move || {
        let _ = listener.send("stop");
    });

    // ---- View ------------------------------------------------------------

    let ghost: Vec<Tile> = match (tool(), hover()) {
        (Tool::Place(shape), Some((x, y))) => vec![Tile::Track {
            x,
            y,
            shape,
            rot: rot(),
            block: paint_block(),
            turnout: None,
            turnout2: None,
            block2: None,
            turntable: None,
            portal: None,
        }],
        (Tool::Roundhouse, Some((x, y))) => vec![Tile::Track {
            x,
            y,
            shape: Shape::Turntable,
            rot: 0,
            block: paint_block(),
            turnout: None,
            turnout2: None,
            block2: None,
            turntable: None,
            portal: None,
        }],
        (Tool::Label, Some((x, y))) => vec![Tile::Label {
            x,
            y,
            text: "Label".into(),
        }],
        (Tool::AutoTrack, Some(end)) => match route_start() {
            Some(start) if start != end => {
                let l = current(draft, bus);
                l.route_track(start, end, paint_block()).unwrap_or_default()
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let status_cell = hover()
        .map(|(x, y)| format!("{x}, {y}"))
        .unwrap_or_else(|| "–".into());
    let hover_block =
        hover().and_then(
            |(x, y)| match layout.element_at(x, y).map(|i| &layout.elements[i]) {
                Some(Tile::Track { block: Some(b), .. }) => Some(b.clone()),
                _ => None,
            },
        );
    let n_issues = issues.len();

    rsx! {
        div { class: "ed",
            // Top bar: menus, history, zoom, apply.
            div { class: "ed-bar",
                div { class: "ed-menus",
                    MenuButton { id: "file", label: "File", menu,
                        MenuItem { label: "New layout", onclick: move |_| new_layout() }
                        MenuItem { label: "Open…", keys: "Ctrl+O", onclick: move |_| import() }
                        MenuItem { label: "Save to disk…", onclick: move |_| export() }
                        MenuSep {}
                        MenuItem { label: "Load demo layout", onclick: move |_| { set_draft(draft, bus, Layout::demo()); reset_editor(); } }
                        for (title, _) in s4d_core::layout::EXAMPLES.iter() {
                            MenuItem { label: "Load example: {title}", onclick: move |_| {
                                if let Some(l) = s4d_core::layout::example(title) {
                                    set_draft(draft, bus, l);
                                    reset_editor();
                                }
                            } }
                        }
                        MenuSep {}
                        MenuItem { label: "Apply & save", keys: "Ctrl+S", disabled: !dirty, onclick: move |_| apply() }
                        MenuItem { label: "Discard changes", disabled: !dirty, onclick: move |_| { let mut d = draft; d.set(None); } }
                    }
                    MenuButton { id: "edit", label: "Edit", menu,
                        MenuItem { label: "Undo", keys: "Ctrl+Z", disabled: !can_undo, onclick: move |_| undo(draft, bus) }
                        MenuItem { label: "Redo", keys: "Ctrl+Y", disabled: !can_redo, onclick: move |_| redo(draft, bus) }
                        MenuSep {}
                        MenuItem { label: "Cut", keys: "Ctrl+X", disabled: sel.is_empty(), onclick: move |_| { copy_selection(); delete_selection(); } }
                        MenuItem { label: "Copy", keys: "Ctrl+C", disabled: sel.is_empty(), onclick: move |_| copy_selection() }
                        MenuItem { label: "Paste", keys: "Ctrl+V", disabled: clipboard.read().is_empty(), onclick: move |_| paste() }
                        MenuItem { label: "Duplicate", keys: "Ctrl+D", disabled: sel.is_empty(), onclick: move |_| { copy_selection(); paste(); } }
                        MenuItem { label: "Delete", keys: "Del", disabled: sel.is_empty(), onclick: move |_| delete_selection() }
                        MenuSep {}
                        MenuItem { label: "Select all", keys: "Ctrl+A", onclick: move |_| select_all() }
                        MenuItem { label: "Rotate", keys: "R", onclick: move |_| rotate(false) }
                    }
                    MenuButton { id: "grid", label: "Grid", menu,
                        MenuItem { label: "Insert row above", disabled: sel.is_empty(), onclick: move |_| insert_line(false, false) }
                        MenuItem { label: "Insert row below", disabled: sel.is_empty(), onclick: move |_| insert_line(false, true) }
                        MenuSep {}
                        MenuItem { label: "Insert column left", disabled: sel.is_empty(), onclick: move |_| insert_line(true, false) }
                        MenuItem { label: "Insert column right", disabled: sel.is_empty(), onclick: move |_| insert_line(true, true) }
                    }
                    MenuButton { id: "view", label: "View", menu,
                        MenuItem { label: "Zoom in", keys: "+", onclick: move |_| zoom.zoom_by(1.25) }
                        MenuItem { label: "Zoom out", keys: "−", onclick: move |_| zoom.zoom_by(1.0 / 1.25) }
                        MenuItem { label: "Actual size", keys: "0", onclick: move |_| zoom.set(Some(32.0)) }
                        MenuItem { label: "Fit to window", onclick: move |_| zoom.set(None) }
                        MenuSep {}
                        MenuItem { label: if show_tags() { "Hide block tags" } else { "Show block tags" }, onclick: move |_| show_tags.set(!show_tags()) }
                        MenuItem { label: if multi() { "Tap selects one tile" } else { "Tap adds to selection" }, onclick: move |_| multi.set(!multi()) }
                    }
                }
                div { class: "ed-icons",
                    button { class: "icon-btn", title: "Undo (Ctrl+Z)", disabled: !can_undo, onclick: move |_| undo(draft, bus), "↶" }
                    button { class: "icon-btn", title: "Redo (Ctrl+Y)", disabled: !can_redo, onclick: move |_| redo(draft, bus), "↷" }
                    span { class: "ed-divider" }
                    button { class: "icon-btn", title: "Zoom out (−, Ctrl+wheel)", onclick: move |_| zoom.zoom_by(1.0 / 1.25), "−" }
                    button { class: "zoom-label", title: "Actual size (0)", onclick: move |_| zoom.set(Some(32.0)), "{zoom.label()}" }
                    button { class: "icon-btn", title: "Zoom in (+, Ctrl+wheel)", onclick: move |_| zoom.zoom_by(1.25), "+" }
                }
                div { class: "ed-title", "{layout.name}" }
                div { class: "ed-save",
                    if dirty {
                        span { class: "unsaved", "● Unsaved changes" }
                        button { class: "text-btn", onclick: move |_| { let mut d = draft; d.set(None); }, "Discard" }
                    }
                    button { class: "primary", disabled: !dirty, title: "Apply to the panel and save (Ctrl+S)", onclick: move |_| apply(), "Apply" }
                }
            }
            if menu().is_some() {
                div { class: "menu-backdrop", onclick: move |_| menu.set(None) }
            }

            div { class: "ed-body",
                // Tool rail.
                nav { class: "ed-rail",
                    RailButton { active: tool() == Tool::Select, title: "Select (V)", onclick: move |_| tool.set(Tool::Select),
                        svg { "viewBox": "0 0 24 24", path { "d": "M5 3l14 8-6 1.5L10 19z" } }
                    }
                    span { class: "rail-sep" }
                    for (i, s) in Shape::ALL.into_iter().enumerate() {
                        RailButton {
                            active: tool() == Tool::Place(s),
                            title: if i < 9 { format!("{} ({})", s.label(), i + 1) } else { s.label().to_string() },
                            onclick: move |_| tool.set(Tool::Place(s)),
                            ShapeIcon { shape: s, rot: rot() }
                        }
                    }
                    RailButton { active: tool() == Tool::AutoTrack, title: "Auto track (A): click where the track starts, then where it ends", onclick: move |_| tool.set(Tool::AutoTrack),
                        svg { "viewBox": "0 0 24 24",
                            circle { cx: "4", cy: "18", r: "2" }
                            circle { cx: "20", cy: "6", r: "2" }
                            path { "d": "M6 18h4l4-8 M14 10l2-4h2", "stroke-dasharray": "2 2" }
                        }
                    }
                    RailButton { active: tool() == Tool::Roundhouse, title: "Turntable & roundhouse: a turntable with a lead track and a seven-stall roundhouse", onclick: move |_| tool.set(Tool::Roundhouse),
                        svg { "viewBox": "0 0 24 24",
                            circle { cx: "12", cy: "14", r: "4" }
                            path { "d": "M2 14h6 M12 10V3 M15 11l5-5 M16 14h6 M15 17l5 5" }
                        }
                    }
                    span { class: "rail-sep" }
                    RailButton { active: tool() == Tool::Label, title: "Label (T)", onclick: move |_| tool.set(Tool::Label),
                        span { class: "rail-text", "T" }
                    }
                    RailButton { active: tool() == Tool::Erase, title: "Erase (E)", onclick: move |_| tool.set(Tool::Erase),
                        svg { "viewBox": "0 0 24 24", path { "d": "M15 4l5 5-9 9H6l-3-3z M9 10l5 5" } }
                    }
                    span { class: "rail-sep" }
                    RailButton { active: false, title: "Rotate (R)", onclick: move |_| rotate(false),
                        svg { "viewBox": "0 0 24 24", path { "d": "M19 12a7 7 0 1 1-2-5 M19 4v4h-4" } }
                    }
                }

                // Canvas.
                div { class: if show_tags() { "ed-canvas" } else { "ed-canvas hide-tags" },
                    if let Some(msg) = error() {
                        div { class: "ed-toast", onclick: move |_| error.set(None), "{msg}" span { class: "muted", "  ✕" } }
                    }
                    TrackView {
                        layout: layout.clone(),
                        blocks: Default::default(),
                        turnouts: Default::default(),
                        mode: Mode::Edit,
                        selected: sel.iter().copied().chain(route_start()).collect::<Vec<_>>(),
                        block_colors: Some(colors.clone()),
                        focus_block: focus_block(),
                        on_cell: on_cell,
                        on_hover: move |c| hover.set(c),
                        ghost: ghost,
                        cell_px: (zoom.px)(),
                        class: tool().css().to_string(),
                    }
                }

                // Properties panel.
                aside { class: "ed-panel",
                    div { class: "ed-tabs",
                        for (t, label) in [(PanelTab::Properties, "Properties"), (PanelTab::Blocks, "Blocks"), (PanelTab::Turnouts, "Turnouts"), (PanelTab::Turntables, "Turntables")] {
                            button {
                                class: if (st.tab)() == t { "on" } else { "" },
                                onclick: move |_| { let mut tab = st.tab; tab.set(t) },
                                "{label}"
                            }
                        }
                    }
                    div { class: "ed-panel-body",
                        match (st.tab)() {
                            PanelTab::Properties => rsx! {
                                PropertiesPanel { layout: layout.clone(), colors: colors.clone(), sel: sel.clone(), issues: issues.iter().map(|i| i.0.clone()).collect::<Vec<_>>() }
                            },
                            PanelTab::Blocks => rsx! {
                                BlocksPanel { layout: layout.clone(), colors: colors.clone(), recent: recent.clone() }
                            },
                            PanelTab::Turnouts => rsx! {
                                TurnoutsPanel { layout: layout.clone(), recent: recent.clone() }
                            },
                            PanelTab::Turntables => rsx! {
                                TurntablesPanel { layout: layout.clone(), recent: recent.clone() }
                            },
                        }
                    }
                }
            }

            // Status bar.
            div { class: "ed-status",
                span { "{tool().name()}" }
                if tool() == Tool::AutoTrack {
                    span { if route_start().is_some() { "Click where the track should end (Esc to cancel)" } else { "Click where the track should start" } }
                }
                if matches!(tool(), Tool::Place(_) | Tool::Roundhouse) {
                    span { "Rotation {rot() as u16 * 45}°" }
                }
                span { "Cell {status_cell}" }
                if let Some(b) = hover_block {
                    span { class: "status-block",
                        i { class: "swatch", style: "background:{colors.get(&b).cloned().unwrap_or_default()}" }
                        "{b}"
                    }
                }
                if !sel.is_empty() {
                    span { "{sel.len()} selected" }
                }
                span { class: "spacer" }
                if n_issues > 0 {
                    button {
                        class: "status-issues",
                        onclick: move |_| { let mut tab = st.tab; tab.set(PanelTab::Properties); selection.set(BTreeSet::new()); },
                        "⚠ {n_issues} issue" if n_issues != 1 { "s" }
                    }
                } else {
                    span { class: "muted", "No issues" }
                }
            }
        }
    }
}

#[component]
fn MenuButton(
    id: &'static str,
    label: &'static str,
    menu: Signal<Option<&'static str>>,
    children: Element,
) -> Element {
    let mut menu = menu;
    let open = menu() == Some(id);
    rsx! {
        div { class: "menu",
            button {
                class: if open { "menu-btn open" } else { "menu-btn" },
                onclick: move |_| menu.set(if open { None } else { Some(id) }),
                // Slide between menus while one is open, like desktop menu bars.
                onmouseenter: move |_| if menu().is_some() && !open { menu.set(Some(id)) },
                "{label}"
            }
            if open {
                div { class: "menu-list", onclick: move |_| menu.set(None), {children} }
            }
        }
    }
}

#[component]
fn MenuItem(
    label: String,
    #[props(default)] keys: &'static str,
    #[props(default)] disabled: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            class: "menu-item",
            disabled,
            onclick: move |e| onclick.call(e),
            span { "{label}" }
            span { class: "keys", "{keys}" }
        }
    }
}

#[component]
fn MenuSep() -> Element {
    rsx! { div { class: "menu-sep" } }
}

#[component]
fn RailButton(
    active: bool,
    title: String,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: if active { "rail-btn on" } else { "rail-btn" },
            title: "{title}",
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}
