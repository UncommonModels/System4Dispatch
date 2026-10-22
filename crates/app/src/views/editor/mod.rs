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
use crate::{Draft, History};
use ops::*;
use panels::{BlocksPanel, PropertiesPanel, ShapeIcon, TurnoutsPanel};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tool {
    Select,
    Place(Shape),
    Label,
    Erase,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Place(s) => s.label(),
            Tool::Label => "Label",
            Tool::Erase => "Erase",
        }
    }

    fn css(self) -> &'static str {
        match self {
            Tool::Select => "tool-select",
            Tool::Place(_) | Tool::Label => "tool-place",
            Tool::Erase => "tool-erase",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PanelTab {
    Properties,
    Blocks,
    Turnouts,
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
    /// Block or turnout id expanded in its panel list.
    pub expanded: Signal<Option<String>>,
}

const MIN_ZOOM: f32 = 12.0;
const MAX_ZOOM: f32 = 96.0;

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
    let mut zoom = use_signal(|| 32.0f32);
    let mut menu = use_signal(|| None::<&'static str>);

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
    let mut select_all = move || {
        let l = current(draft, bus);
        selection.set(l.elements.iter().map(|e| e.pos()).collect());
        tool.set(Tool::Select);
    };
    let mut set_zoom = move |z: f32| zoom.set(z.clamp(MIN_ZOOM, MAX_ZOOM));
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
                    (_, "+") | (_, "=") => set_zoom(zoom() * 1.25),
                    (_, "-") => set_zoom(zoom() / 1.25),
                    (_, "0") => zoom.set(32.0),
                    (false, "r") => rotate(k.shift),
                    (false, "delete") | (false, "backspace") => delete_selection(),
                    (false, "escape") => {
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
                    (false, "t") => tool.set(Tool::Label),
                    (false, n) if n.len() == 1 && ("1"..="8").contains(&n) => {
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

    let ghost = match (tool(), hover()) {
        (Tool::Place(shape), Some((x, y))) => Some(Tile::Track {
            x,
            y,
            shape,
            rot: rot(),
            block: paint_block(),
            turnout: None,
        }),
        (Tool::Label, Some((x, y))) => Some(Tile::Label {
            x,
            y,
            text: "Label".into(),
        }),
        _ => None,
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
    let zoom_pct = (zoom() / 32.0 * 100.0).round();
    let n_issues = issues.len();

    rsx! {
        div { class: "ed",
            // Top bar: menus, history, zoom, apply.
            div { class: "ed-bar",
                div { class: "ed-menus",
                    MenuButton { id: "file", label: "File", menu,
                        MenuItem { label: "New layout", onclick: move |_| new_layout() }
                        MenuItem { label: "Open…", keys: "Ctrl+O", onclick: move |_| import() }
                        MenuItem { label: "Export…", onclick: move |_| export() }
                        MenuSep {}
                        MenuItem { label: "Load demo layout", onclick: move |_| { set_draft(draft, bus, Layout::demo()); reset_editor(); } }
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
                    MenuButton { id: "view", label: "View", menu,
                        MenuItem { label: "Zoom in", keys: "+", onclick: move |_| set_zoom(zoom() * 1.25) }
                        MenuItem { label: "Zoom out", keys: "−", onclick: move |_| set_zoom(zoom() / 1.25) }
                        MenuItem { label: "Actual size", keys: "0", onclick: move |_| zoom.set(32.0) }
                        MenuSep {}
                        MenuItem { label: if show_tags() { "Hide block tags" } else { "Show block tags" }, onclick: move |_| show_tags.set(!show_tags()) }
                        MenuItem { label: if multi() { "Tap selects one tile" } else { "Tap adds to selection" }, onclick: move |_| multi.set(!multi()) }
                    }
                }
                div { class: "ed-icons",
                    button { class: "icon-btn", title: "Undo (Ctrl+Z)", disabled: !can_undo, onclick: move |_| undo(draft, bus), "↶" }
                    button { class: "icon-btn", title: "Redo (Ctrl+Y)", disabled: !can_redo, onclick: move |_| redo(draft, bus), "↷" }
                    span { class: "ed-divider" }
                    button { class: "icon-btn", title: "Zoom out (−)", onclick: move |_| set_zoom(zoom() / 1.25), "−" }
                    button { class: "zoom-label", title: "Actual size (0)", onclick: move |_| zoom.set(32.0), "{zoom_pct}%" }
                    button { class: "icon-btn", title: "Zoom in (+)", onclick: move |_| set_zoom(zoom() * 1.25), "+" }
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
                        RailButton { active: tool() == Tool::Place(s), title: "{s.label()} ({i + 1})", onclick: move |_| tool.set(Tool::Place(s)),
                            ShapeIcon { shape: s, rot: rot() }
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
                    onwheel: move |e| {
                        if e.modifiers().ctrl() {
                            e.prevent_default();
                            let dy = e.delta().strip_units().y;
                            set_zoom(if dy < 0.0 { zoom() * 1.1 } else { zoom() / 1.1 });
                        }
                    },
                    if let Some(msg) = error() {
                        div { class: "ed-toast", onclick: move |_| error.set(None), "{msg}" span { class: "muted", "  ✕" } }
                    }
                    TrackView {
                        layout: layout.clone(),
                        blocks: Default::default(),
                        turnouts: Default::default(),
                        mode: Mode::Edit,
                        selected: sel.iter().copied().collect::<Vec<_>>(),
                        block_colors: Some(colors.clone()),
                        focus_block: focus_block(),
                        on_cell: on_cell,
                        on_hover: move |c| hover.set(c),
                        ghost: ghost,
                        cell_px: Some(zoom()),
                        class: tool().css().to_string(),
                    }
                }

                // Properties panel.
                aside { class: "ed-panel",
                    div { class: "ed-tabs",
                        for (t, label) in [(PanelTab::Properties, "Properties"), (PanelTab::Blocks, "Blocks"), (PanelTab::Turnouts, "Turnouts")] {
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
                        }
                    }
                }
            }

            // Status bar.
            div { class: "ed-status",
                span { "{tool().name()}" }
                if matches!(tool(), Tool::Place(_)) {
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
